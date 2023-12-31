use std::{collections::HashMap, sync::{Arc, Mutex}};

use colorize::AnsiColor;

use crate::{terminate, source_file::{SourceFile, SourceFileId, Located}, token::lexer::Lexer, ast, parser::{Parser, ParseError}, error::{CompilerError, IntoCompilerError}};

#[derive(Default)]
pub enum BuildKind {
    #[default]
    Executable,
    Object,
    SharedObject
}

impl BuildKind {
    fn ext(&self, os: &str) -> Option<&'static str> {
        match os {
            "linux" | "macos" | "unix" => Some(self.ext_unix()),
            "windows" => Some(self.ext_windows()),
            _ => None
        }
    }

    fn ext_unix(&self) -> &'static str {
        match self {
            Self::Executable => "",
            Self::Object => ".o",
            Self::SharedObject => ".so"
        }
    }

    fn ext_windows(&self) -> &'static str {
        match self {
            Self::Executable => ".exe",
            Self::Object => ".lib",
            Self::SharedObject => ".dll"
        }
    }
}

#[derive(Default)]
pub enum OutputFile {
    Name(String),
    #[default]
    Default
}

impl OutputFile {
    pub fn to_filename(self, build_kind: &BuildKind) -> String {
        match self {
            Self::Name(filename) => filename,
            Self::Default => format!("a{}", build_kind.ext(std::env::consts::OS).expect("invalid operating system"))
        }
    }
}

#[derive(Default)]
pub struct Context {
    program_name: String,
    output_file: OutputFile,

    build_kind: BuildKind,
    tags: Vec<String>,

    source_files: HashMap<SourceFileId, SourceFile>,

    ast: Arc<Mutex<ast::Program>>
}

impl Context {
    pub fn from_program_name(program_name: String) -> Self {
        let mut ctx = Self::default();
        ctx.program_name = program_name;
        ctx
    }
    
    pub fn set_output_file(&mut self, output_file: String) {
        self.output_file = OutputFile::Name(output_file);
    }

    pub fn program_name(&self) -> &String {
        &self.program_name
    }

    pub fn define_tag(&mut self, tag: String) {
        self.tags.push(tag);
    }

    pub fn set_build_kind(&mut self, build_kind: BuildKind) {
        self.build_kind = build_kind;
    }

    pub fn add_source_files(&mut self, source_files: HashMap<SourceFileId, SourceFile>) {
        self.source_files.extend(source_files);
    }

    pub fn fatal_error(self, err: &str) -> ! {
        eprintln!("{} {} {err}",
            format!("{}:", self.program_name()).bold(),
            format!("fatal error:").bold().red()
        );
        
        terminate();
    }

    pub fn highlight_error(&self, err: Located<impl IntoCompilerError>) {
        let file = self.source_files.get(&err.location().file_id()).expect("invalid file id");
        let loc = err.location().clone();
        let err: CompilerError = err.unwrap().into();        
        println!("{} {}:{}:{}: {}", err.severity(), file.path(), loc.line(), loc.column(), err.message());
        print!("{} {} ", format!(" {: >4}", loc.line()).bold().b_black(), "|".b_black());

        let line = file.line(loc.line()).unwrap();
        let mark_start = loc.column();
        let mark_end = loc.column() + loc.width();
        println!("{}{}{}", &line[..mark_start], (&line[mark_start..mark_end]).to_owned().bold().b_yellow(), &line[mark_end..]);

        print!("      {} {}{}", "|".b_black(), " ".repeat(mark_start), "~".repeat(loc.width()).yellow());

        if let Some(hint) = err.hint() {
            print!(" {} {} {}", "<-".b_black(), "hint:".bold().b_grey(), hint.clone().b_grey());
        }

        println!();

        for additional in err.additional {
            self.highlight_error(additional) 
        }
    }


    fn print_compiling_status(&self, filepath: &String) {
        println!("{} {filepath}", "Compiling:".bold().magenta());
    }

    pub fn compile(self) -> Result<(), ()> {
        if self.source_files.is_empty() {
            self.fatal_error("no input files.");
        }
            
        let mut had_errors = false;
        for file in self.source_files.values() {
            let mut parser = Parser::new(Lexer::from(file), self.ast.clone());
            self.print_compiling_status((**parser).path());
        
            if let Err(err) = parser.parse() {
                self.highlight_error(err);
                had_errors = true;
            }

            for warning in parser.warnings() {
                self.highlight_error(warning.clone());
            }
        }

        if had_errors {
            return Err(())
        }

        println!("generated ast: {:#?}", self.ast);
        Ok(())
    }
}
