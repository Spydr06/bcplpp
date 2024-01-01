use std::cell::RefCell;

use crate::{
    ast::{stmt::{Stmt, StmtKind}, expr::{Expr, ExprKind}, types::{TypeIndex, TypeKind}, LocalDecl}, 
    source_file::{WithLocation, Located, Location},
    token::{Token, TokenKind}
};

use super::{Parser, ParseResult, ParseError};

pub(super) enum StmtContext<'a> {
    ValOf(&'a RefCell<Option<Option<TypeIndex>>>, &'a StmtContext<'a>),
    Block(&'a StmtContext<'a>),
    NoBlock(&'a StmtContext<'a>),
    Function(&'a RefCell<Option<Option<TypeIndex>>>),
    Loop(&'a StmtContext<'a>),
    SwitchOn(&'a RefCell<Option<Location>>, &'a Option<TypeIndex>, &'a StmtContext<'a>),
    Empty
}

impl<'a> StmtContext<'a> {
    pub(super) fn last_valof_type(&self) -> Option<&RefCell<Option<Option<TypeIndex>>>> {
        match self {
            Self::ValOf(typ, _) => Some(typ),
            Self::Block(outer) 
                | Self::NoBlock(outer)
                | Self::Loop(outer) 
                | Self::SwitchOn(.., outer) => outer.last_valof_type(),
            Self::Function(_) => None,
            Self::Empty => None
        }
    }

    pub(super) fn function_return_type(&self) -> Option<&RefCell<Option<Option<TypeIndex>>>> {
        match self {
            Self::ValOf(_, outer) 
                | Self::Block(outer)
                | Self::NoBlock(outer)
                | Self::Loop(outer) 
                | Self::SwitchOn(.., outer) => outer.function_return_type(),
            Self::Function(return_type) => Some(return_type),
            Self::Empty => None
        }
    }

    fn require_semicolon(&self) -> bool {
        match self {
            Self::Block(_) => true,
            Self::Loop(outer) 
                | Self::SwitchOn(.. , outer) => outer.require_semicolon(),
            _ => false
        }
    }

    fn in_loop(&self) -> bool {
        match self {
            Self::Loop(_) => true,
            Self::ValOf(_, outer)
                | Self::Block(outer) 
                | Self::NoBlock(outer)
                | Self::SwitchOn(.., outer) => outer.in_loop(),
            Self::Empty
                | Self::Function(_) => false
        }
    }

    fn in_switchon(&self) -> Option<(&'a RefCell<Option<Location>>, &'a Option<TypeIndex>)> {
        match self {
            Self::SwitchOn(default_case, cond_typ, _) => Some((default_case, cond_typ)),
            Self::ValOf(_, outer)
                | Self::Block(outer)
                | Self::NoBlock(outer)
                | Self::Loop(outer) => outer.in_switchon(),
            Self::Empty
                | Self::Function(_) => None
        }
    }
}

impl<'a> Parser<'a> {
    pub(super) fn parse_stmt(&mut self, context: &StmtContext) -> ParseResult<'a, Stmt> {
        let stmt = match self.current().kind() {
            TokenKind::LBrace => self.parse_block(context),
            TokenKind::ResultIs => self.parse_resultis(context),
            TokenKind::Return => self.parse_return(context),
            TokenKind::If => self.parse_if(context),
            TokenKind::Unless => self.parse_unless(context),
            TokenKind::While => self.parse_while(context, false),
            TokenKind::Until => self.parse_while(context, true),
            TokenKind::For => self.parse_for(context),
            TokenKind::SwitchOn => self.parse_switchon(context),
            TokenKind::Case => self.parse_case(context),
            TokenKind::Default => self.parse_default_case(context),
            TokenKind::EndCase => self.parse_endcase(context),
            _ => self.parse_expr_stmt(context),
        }?;

        if let TokenKind::Compound = self.current().kind() {
            self.parse_compound(context, stmt)
        }
        else {
            Ok(stmt)
        }
    }

    fn parse_compound(&mut self, context: &StmtContext, left: Stmt) -> ParseResult<'a, Stmt> {
        let loc = self.current().location().clone();
        let mut stmts = vec![left];

        let context = StmtContext::NoBlock(context);
        while let TokenKind::Compound = self.current().kind() {
            self.advance()?;
            stmts.push(self.parse_stmt(&context)?);
        }

        Ok(Stmt::new(loc, StmtKind::Block(stmts)))
    }

    fn parse_block(&mut self, context: &StmtContext) -> ParseResult<'a, Stmt> {
        let loc = self.current().location().clone();
        self.expect(&[TokenKind::LBrace])?;

        let mut stmts = vec![];

        while self.current().kind() != &TokenKind::RBrace {
            stmts.push(self.parse_stmt(&StmtContext::Block(context))?)
        }

        self.advance()?;

        Ok(Stmt::new(loc, StmtKind::Block(stmts)))
    }

    fn parse_resultis(&mut self, context: &StmtContext) -> ParseResult<'a, Stmt> {
        let loc = self.expect(&[TokenKind::ResultIs])?.location().clone();
        
        let expr = self.parse_expr(context)?;
        let valof_typ = context.last_valof_type()
            .ok_or_else(|| 
                ParseError::InvalidStmt("resultis".into(), "valof".into())
                    .with_location(loc.clone())
            )?; 

        let vt = valof_typ.borrow().clone();
        let expr = match vt {
            Some(vt) if &vt != expr.typ() => Expr::new(loc.clone(), vt, ExprKind::ImplicitCast(Box::new(expr))),
            None => {
                *valof_typ.borrow_mut() = Some(expr.typ().clone());
                expr
            }
            _ => expr
        };

        self.semicolon_if_required(context)?;

        Ok(Stmt::new(loc, StmtKind::ResultIs(Box::new(expr))))
    }

    fn parse_return(&mut self, context: &StmtContext) -> ParseResult<'a, Stmt> {
        let loc = self.expect(&[TokenKind::Return])?.location().clone();

        let expr = self.parse_expr(context)?;
        let return_type = context.function_return_type()
            .ok_or_else(||
                ParseError::InvalidStmt("return".into(), "function".into())
                    .with_location(loc.clone())
            )?;
        
        let rt = return_type.borrow().clone();
        let expr = match rt {
            Some(rt) if &rt != expr.typ() => Expr::new(loc.clone(), rt, ExprKind::ImplicitCast(Box::new(expr))),
            None => {
                *return_type.borrow_mut() = Some(expr.typ().clone());
                expr
            }
            _ => expr
        };

        self.semicolon_if_required(context)?;

        Ok(Stmt::new(loc, StmtKind::Return(Box::new(expr))))
    }

    fn parse_if(&mut self, context: &StmtContext) -> ParseResult<'a, Stmt> {
        let loc = self.expect(&[TokenKind::If])?.location().clone();

        let mut condition = self.parse_expr(context)?;
        let bool_typ = self.get_type(TypeKind::Bool);
        if condition.typ() != &Some(bool_typ) {
            condition = condition.implicit_cast(bool_typ);
        }

        self.advance_if(&[TokenKind::Do])?;

        let if_branch = self.parse_stmt(context)?;
        let else_branch = if self.advance_if(&[TokenKind::Else])?.is_some() {
            Some(self.parse_stmt(context)?)
        }
        else {
            None
        };

        Ok(Stmt::new(loc, StmtKind::If(Box::new(condition), Box::new(if_branch), else_branch.map(Box::new))))       
    }

    fn parse_unless(&mut self, context: &StmtContext) -> ParseResult<'a, Stmt> {
        let loc = self.expect(&[TokenKind::Unless])?.location().clone();

        let mut condition = self.parse_expr(context)?;
        let bool_typ = self.get_type(TypeKind::Bool);
        if condition.typ() != &Some(bool_typ) {
            condition = condition.implicit_cast(bool_typ);
        }

        self.advance_if(&[TokenKind::Do])?;

        let branch = self.parse_stmt(context)?;
        Ok(Stmt::new(loc, StmtKind::Unless(Box::new(condition), Box::new(branch))))
    }

    fn parse_while(&mut self, context: &StmtContext, negate: bool) -> ParseResult<'a, Stmt> {
        let loc = self.expect(&[TokenKind::While, TokenKind::While])?.location().clone();

        let mut condition = self.parse_expr(context)?;
        let bool_typ = self.get_type(TypeKind::Bool);
        if condition.typ() != &Some(bool_typ) {
            condition = condition.implicit_cast(bool_typ);
        }

        self.advance_if(&[TokenKind::Do])?;

        let body = self.parse_stmt(&StmtContext::Loop(context))?;
        let kind = if negate { StmtKind::Until } else { StmtKind::While }
            (Box::new(condition), Box::new(body));
        Ok(Stmt::new(loc, kind))
    }

    fn parse_for(&mut self, context: &StmtContext) -> ParseResult<'a, Stmt> {
        let loc = self.expect(&[TokenKind::For])?.location().clone();

        let iter_loc = self.current().location().clone();
        let iter_ident = self.expect_ident()?;
        self.expect(&[TokenKind::Eq])?;
        let init = self.parse_expr(context)?;

        let limit = if self.advance_if(&[TokenKind::To])?.is_some() {
            let mut expr = self.parse_expr(context)?;
            if let Some(typ) = init.typ() && init.typ() != expr.typ() {
                expr = expr.implicit_cast(*typ) 
            }
            Some(expr)
        }
        else {
            None
        };

        let step = if self.advance_if(&[TokenKind::By])?.is_some() {
            let mut expr = self.parse_expr(context)?;
            if let Some(typ) = init.typ() && init.typ() != expr.typ() {
                expr = expr.implicit_cast(*typ) 
            }
            Some(expr)

        }
        else {
            None
        };

        self.advance_if(&[TokenKind::Do])?;

        let body = self.parse_stmt(&StmtContext::Loop(context))?;

        Ok(Stmt::new(
            loc,
            StmtKind::For(LocalDecl::new(iter_loc, iter_ident, init.typ().clone()),
                Box::new(init),
                limit.map(Box::new),
                step.map(Box::new),
                Box::new(body)
            )
        ))
    }

    fn parse_switchon(&mut self, context: &StmtContext) -> ParseResult<'a, Stmt> {
        let loc = self.expect(&[TokenKind::SwitchOn])?.location().clone();

        let condition = self.parse_expr(context)?;
        self.expect(&[TokenKind::Into])?;

        let default_case = RefCell::new(None);
        let body = self.parse_stmt(&StmtContext::SwitchOn(&default_case, condition.typ(), context))?;

        Ok(Stmt::new(loc, StmtKind::SwitchOn(Box::new(condition), Box::new(body))))
    }

    fn parse_case(&mut self, context: &StmtContext) -> ParseResult<'a, Stmt> {
        let loc = self.expect(&[TokenKind::Case])?.location().clone();
        
        let mut expr = self.parse_expr(context)?;
        self.expect(&[TokenKind::Colon])?;

        if let Some((_, cond_typ)) = context.in_switchon() {
            if let Some(cond_typ) = cond_typ && &Some(*cond_typ) != expr.typ() {
                expr = expr.implicit_cast(*cond_typ);
            }

            Ok(Stmt::new(loc, StmtKind::Case(Box::new(expr))))
        }
        else {
            Err(ParseError::InvalidStmt("case".into(), "switchon".into())
                .with_location(loc))
        }
    } 

    fn parse_default_case(&mut self, context: &StmtContext) -> ParseResult<'a, Stmt> {
        let loc = self.expect(&[TokenKind::Default])?.location().clone();
        self.expect(&[TokenKind::Colon])?;

        if let Some((default_case, _)) = context.in_switchon() {
            let mut default_case = default_case.borrow_mut();
            if let Some(prev) = default_case.as_ref() {
                Err(ParseError::Redefinition(prev.clone(), "default case".into())
                    .with_location(loc))
            }
            else {
                *default_case = Some(loc.clone());
                Ok(Stmt::new(loc, StmtKind::DefaultCase))
            }
        }
        else {
            Err(ParseError::InvalidStmt("default".into(), "switchon".into())
                .with_location(loc))
        }
    }

    fn parse_endcase(&mut self, context: &StmtContext) -> ParseResult<'a, Stmt> {
        let loc = self.expect(&[TokenKind::EndCase])?.location().clone();
        self.semicolon_if_required(context)?;

        context.in_switchon()
            .map(|_| Stmt::new(loc.clone(), StmtKind::EndCase))
            .ok_or_else(|| ParseError::InvalidStmt("endcase".into(), "switchon".into()).with_location(loc))
    }

    fn parse_expr_stmt(&mut self, context: &StmtContext) -> ParseResult<'a, Stmt> {
        let loc = self.current().location().clone();
        let expr = self.parse_expr(context)?;
        if !expr.has_sideeffect() {
            self.push_warning(ParseError::ExprWithoutSideEffect.with_location(loc.clone()))
        }

        self.semicolon_if_required(context)?;

        Ok(Stmt::new(loc, StmtKind::Expr(Box::new(expr))))
    }

    fn semicolon_if_required(&mut self, context: &StmtContext) -> ParseResult<'a, ()> {
        if context.require_semicolon() {
            self.expect(&[TokenKind::Semicolon])?;
        }
        Ok(())
    }
}
