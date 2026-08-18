//! Recursive descent over the M1 subset.
//!
//! Two things make this parser Fortress-specific rather than generic.
//!
//! Newlines are tokens, and where they may be skipped is a property of the
//! position: the reference grammar's `w`/`wr` contexts skip them, its `s`/`sr`
//! contexts do not, and that distinction is the whole of statement termination.
//!
//! Operator fixity is decided from byte-span adjacency rather than from the
//! token, because `x-1`, `x - 1` and `x -1` lex identically and mean three
//! different things.

mod error;

pub use error::ParseError;

use fortress_ast::{
    Assign, BinOp, Binding, BlockItem, Component, Decl, Expr, FieldDecl, Fixity, FnDecl,
    ImportDecl, Member, MethodDecl, ObjectDecl, Param, Span, TraitDecl, TypeRef, UnOp,
};
use fortress_lexer::{Kind, Token};

type Parsed<T> = Result<T, ParseError>;

pub fn parse(tokens: &[Token<'_>]) -> Parsed<Component> {
    Parser { tokens, pos: 0 }.component()
}

struct Parser<'t, 'a> {
    tokens: &'t [Token<'a>],
    pos: usize,
}

impl<'t, 'a> Parser<'t, 'a> {
    // ----------------------------------------------------------- primitives

    fn peek(&self) -> Option<&'t Token<'a>> {
        self.tokens.get(self.pos)
    }

    fn peek_kind(&self) -> Option<&'t Kind<'a>> {
        self.peek().map(|t| &t.kind)
    }

    fn at(&self, kind: &Kind<'_>) -> bool {
        self.peek_kind().is_some_and(|k| k == kind)
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek_kind(), Some(Kind::Eof) | None)
    }

    fn bump(&mut self) -> Option<&'t Token<'a>> {
        let token = self.tokens.get(self.pos);
        if token.is_some() {
            self.pos += 1;
        }
        token
    }

    fn span_here(&self) -> Span {
        self.peek().map_or(Span::new(0, 0), |t| t.span)
    }

    fn error(&self, expected: &'static str) -> ParseError {
        match self.peek() {
            None => ParseError::UnexpectedEndOfInput { expected },
            Some(t) => ParseError::UnexpectedToken {
                span: t.span,
                expected,
                found: format!("{:?}", t.kind),
            },
        }
    }

    fn expect(&mut self, kind: &Kind<'_>, expected: &'static str) -> Parsed<&'t Token<'a>> {
        if self.at(kind) {
            self.bump()
                .ok_or(ParseError::UnexpectedEndOfInput { expected })
        } else {
            Err(self.error(expected))
        }
    }

    /// A `w`/`wr` context: newlines carry no meaning here.
    fn skip_newlines(&mut self) {
        while self.at(&Kind::Newline) {
            self.pos += 1;
        }
    }

    /// A `br` context: at least one terminator is required. `Newline+` or one
    /// `Semi` followed by newlines. `a;;b` and `a\n;b` have no parse, matching
    /// the reference, because `br` consumes exactly one semicolon.
    fn expect_separator(&mut self) -> Parsed<()> {
        if self.at(&Kind::Semi) {
            self.pos += 1;
            self.skip_newlines();
            return Ok(());
        }
        if self.at(&Kind::Newline) {
            self.skip_newlines();
            return Ok(());
        }
        Err(self.error("a newline or `;`"))
    }

    // ------------------------------------------------------------ span math

    /// True when the token at `index` begins exactly where the previous token
    /// ended, i.e. no whitespace and no comment between them.
    fn glued_left(&self, index: usize) -> bool {
        let (Some(prev), Some(here)) = (
            index.checked_sub(1).and_then(|i| self.tokens.get(i)),
            self.tokens.get(index),
        ) else {
            return false;
        };
        if matches!(prev.kind, Kind::Newline) {
            return false;
        }
        prev.span.end == here.span.start
    }

    fn glued_right(&self, index: usize) -> bool {
        let (Some(here), Some(next)) = (self.tokens.get(index), self.tokens.get(index + 1)) else {
            return false;
        };
        if matches!(next.kind, Kind::Newline) {
            return false;
        }
        here.span.end == next.span.start
    }

    /// The four readings of an operator, from adjacency alone.
    fn fixity_at(&self, index: usize) -> OperatorShape {
        match (self.glued_left(index), self.glued_right(index)) {
            (true, true) => OperatorShape::TightInfix,
            (false, false) => OperatorShape::LooseInfix,
            (false, true) => OperatorShape::Prefix,
            (true, false) => OperatorShape::Postfix,
        }
    }

    // ------------------------------------------------------------ component

    fn component(&mut self) -> Parsed<Component> {
        self.skip_newlines();
        let start = self.span_here();
        // Three shapes. `component Foo ... end`; `api Foo ... end`, which parses
        // so the corpus metric can move and which `check` refuses because a
        // declaration without a body is not something to emit code for; and a
        // headerless file, which `Compilation.rats:14-19` gives four productions
        // for -- exports, imports and declarations straight to end of file with
        // no wrapper and no `end`.
        let headerless = !self.at(&Kind::KwComponent) && !self.at(&Kind::KwApi);
        let mut is_api = false;
        let mut name = String::new();
        if !headerless {
            is_api = self.at(&Kind::KwApi);
            self.pos += 1;
            name = self.dotted_name()?;
            self.expect_separator()?;
        }

        // The reference grammar puts imports before exports and has an error
        // production for the other order, so accept either, interleaved.
        let mut exports = Vec::new();
        let mut imports = Vec::new();
        loop {
            if self.at(&Kind::KwExport) {
                self.pos += 1;
                exports.push(self.identifier("an export name")?.0);
            } else if self.at(&Kind::KwImport) {
                imports.push(self.import_decl()?);
            } else {
                break;
            }
            self.expect_separator()?;
        }

        let mut decls = Vec::new();
        while !self.at(&Kind::KwEnd) && !self.at_eof() {
            decls.push(self.decl(is_api)?);
            if self.at(&Kind::KwEnd) || self.at_eof() {
                break;
            }
            self.expect_separator()?;
        }

        let end = if headerless {
            self.span_here()
        } else {
            self.expect(&Kind::KwEnd, "`end`")?.span
        };
        Ok(Component {
            name,
            exports,
            imports,
            decls,
            is_api,
            span: Span::new(start.start, end.end),
        })
    }

    /// `import Foo.Bar.{...}`, `import api Foo`, `import Foo.{X as Y} except {Z}`.
    /// The dotted name is parsed for real; the brace group and the `except`
    /// clause are consumed as balanced token runs. Aliasing an operator needs a
    /// precedence map, and recording a name we cannot resolve yet would be
    /// pretending.
    fn import_decl(&mut self) -> Parsed<ImportDecl> {
        let start = self.expect(&Kind::KwImport, "`import`")?.span;
        let is_api = self.at(&Kind::KwApi);
        if is_api {
            self.pos += 1;
        }
        // `import api {A, B}` names a set with no leading dotted name.
        let api_name = if self.at(&Kind::LBrace) {
            String::new()
        } else {
            self.dotted_name()?
        };
        let mut end = self.span_here();
        if self.at(&Kind::LBrace) {
            end = self.skip_braces()?;
        }
        if self.at(&Kind::KwExcept) {
            self.pos += 1;
            self.skip_newlines();
            end = if self.at(&Kind::LBrace) {
                self.skip_braces()?
            } else {
                self.identifier("a name after `except`")?.1
            };
        }
        Ok(ImportDecl {
            api_name,
            is_api,
            span: Span::new(start.start, end.end),
        })
    }

    /// Consumes a balanced `{ ... }` and answers with the closing brace's span.
    fn skip_braces(&mut self) -> Parsed<Span> {
        self.expect(&Kind::LBrace, "`{`")?;
        let mut depth = 1usize;
        loop {
            let span = self.span_here();
            match self.peek_kind() {
                Some(Kind::LBrace) => depth += 1,
                Some(Kind::RBrace) => {
                    depth -= 1;
                    if depth == 0 {
                        self.pos += 1;
                        return Ok(span);
                    }
                }
                None | Some(Kind::Eof) => return Err(self.error("`}`")),
                _ => {}
            }
            self.pos += 1;
        }
    }

    fn decl(&mut self, signature_only: bool) -> Parsed<Decl> {
        match self.peek_kind() {
            Some(Kind::KwTrait) => Ok(Decl::Trait(self.trait_decl()?)),
            Some(Kind::KwObject) => Ok(Decl::Object(self.object_decl()?)),
            _ => Ok(Decl::Function(self.fn_decl(signature_only)?)),
        }
    }

    // ------------------------------------------------------ traits and objects

    /// `comprises` and `excludes` are recorded and never read: exclusion is
    /// decided from the concrete types the program actually declares, which a
    /// whole-program compiler can see and a modular one cannot.
    fn trait_decl(&mut self) -> Parsed<TraitDecl> {
        let start = self.expect(&Kind::KwTrait, "`trait`")?.span;
        let (name, _) = self.identifier("a trait name")?;
        self.reject_static_parameters()?;
        let extends = self.extends_clause()?;
        let comprises = self.type_set_after(&Kind::KwComprises)?;
        let excludes = self.type_set_after(&Kind::KwExcludes)?;
        self.skip_where()?;
        let members = self.members()?;
        let end = self.expect(&Kind::KwEnd, "`end`")?.span;
        Ok(TraitDecl {
            name,
            extends,
            comprises,
            excludes,
            members,
            span: Span::new(start.start, end.end),
        })
    }

    /// No parameter list at all is a singleton: one instance, constructed once
    /// before `run`. `object O() ... end` is a constructor taking nothing.
    fn object_decl(&mut self) -> Parsed<ObjectDecl> {
        let start = self.expect(&Kind::KwObject, "`object`")?.span;
        let (name, _) = self.identifier("an object name")?;
        self.reject_static_parameters()?;
        let params = if self.at(&Kind::LParen) {
            self.pos += 1;
            let params = self.params()?;
            self.expect(&Kind::RParen, "`)`")?;
            Some(params)
        } else {
            None
        };
        let extends = self.extends_clause()?;
        self.skip_where()?;
        let members = self.members()?;
        let end = self.expect(&Kind::KwEnd, "`end`")?.span;
        Ok(ObjectDecl {
            name,
            params,
            extends,
            members,
            span: Span::new(start.start, end.end),
        })
    }

    fn reject_static_parameters(&self) -> Parsed<()> {
        if self.at(&Kind::LGeneric) {
            return Err(ParseError::StaticParametersUnsupported {
                span: self.span_here(),
            });
        }
        Ok(())
    }

    fn extends_clause(&mut self) -> Parsed<Vec<TypeRef>> {
        self.type_set_after(&Kind::KwExtends)
    }

    fn type_set_after(&mut self, keyword: &Kind<'_>) -> Parsed<Vec<TypeRef>> {
        if !self.at(keyword) {
            return Ok(Vec::new());
        }
        self.pos += 1;
        self.skip_newlines();
        if !self.at(&Kind::LBrace) {
            return Ok(vec![self.type_ref()?]);
        }
        self.pos += 1;
        self.skip_newlines();
        let mut out = Vec::new();
        if self.at(&Kind::RBrace) {
            self.pos += 1;
            return Ok(out);
        }
        loop {
            out.push(self.type_ref()?);
            self.skip_newlines();
            if !self.at(&Kind::Comma) {
                break;
            }
            self.pos += 1;
            self.skip_newlines();
        }
        self.expect(&Kind::RBrace, "`}`")?;
        Ok(out)
    }

    /// `where {T extends U}`. Consumed and discarded: there are no static
    /// parameters to constrain until generics land.
    fn skip_where(&mut self) -> Parsed<()> {
        if !self.at(&Kind::KwWhere) {
            return Ok(());
        }
        self.pos += 1;
        self.skip_newlines();
        self.expect(&Kind::LBrace, "`{`")?;
        let mut depth = 1usize;
        while depth > 0 {
            match self.peek_kind() {
                Some(Kind::LBrace) => depth += 1,
                Some(Kind::RBrace) => depth -= 1,
                None | Some(Kind::Eof) => return Err(self.error("`}`")),
                _ => {}
            }
            self.pos += 1;
        }
        Ok(())
    }

    fn members(&mut self) -> Parsed<Vec<Member>> {
        let mut members = Vec::new();
        self.skip_newlines();
        while !self.at(&Kind::KwEnd) && !self.at_eof() {
            members.push(self.member()?);
            if self.at(&Kind::KwEnd) {
                break;
            }
            self.expect_separator()?;
        }
        Ok(members)
    }

    /// A field (`x: T`, `var x: T = e`) or a dotted method. Methods are parsed
    /// and never checked, so their bodies may say anything the grammar allows.
    fn member(&mut self) -> Parsed<Member> {
        let start = self.span_here();
        let mutable = if self.at(&Kind::KwVar) {
            self.pos += 1;
            true
        } else {
            false
        };
        let (name, name_span) = self.identifier("a field or method name")?;

        if self.at(&Kind::LParen) {
            if mutable {
                return Err(self.error("a field name after `var`"));
            }
            self.pos += 1;
            let params = self.params()?;
            let rparen = self.expect(&Kind::RParen, "`)`")?.span;
            let return_type = if self.at(&Kind::Colon) {
                self.pos += 1;
                Some(self.type_ref()?)
            } else {
                None
            };
            self.skip_where()?;
            let body = self.optional_definition()?;
            let end = body.as_ref().map_or(rparen, Expr::span);
            return Ok(Member::Method(MethodDecl {
                name,
                params,
                return_type,
                body,
                span: Span::new(start.start, end.end),
            }));
        }

        self.expect(&Kind::Colon, "`:` or `(`")?;
        let ty = self.type_ref()?;
        let init = self.optional_definition()?;
        let end = init.as_ref().map_or(ty.span, Expr::span);
        Ok(Member::Field(FieldDecl {
            name,
            ty,
            init,
            mutable,
            span: Span::new(name_span.start, end.end),
        }))
    }

    /// `= e`, where the `=` may sit on the following line. Restores the
    /// position when there is none, so the separator the caller needs survives.
    fn optional_definition(&mut self) -> Parsed<Option<Expr>> {
        let save = self.pos;
        self.skip_newlines();
        if !self.at(&Kind::Eq) {
            self.pos = save;
            return Ok(None);
        }
        self.pos += 1;
        self.skip_newlines();
        Ok(Some(self.expr()?))
    }

    /// Component names may be dotted (`Compiled2.h`), which the lexer sees as
    /// separate `Ident` and `Dot` tokens.
    fn dotted_name(&mut self) -> Parsed<String> {
        let (mut name, _) = self.identifier("a component name")?;
        // A `.` followed by `{` opens an import's brace group and is not part
        // of the name.
        while self.at(&Kind::Dot) && !matches!(self.peek_ahead(1), Some(Kind::LBrace)) {
            self.pos += 1;
            let (part, _) = self.identifier("a component name")?;
            name.push('.');
            name.push_str(&part);
        }
        if self.at(&Kind::Dot) {
            self.pos += 1;
        }
        Ok(name)
    }

    fn peek_ahead(&self, n: usize) -> Option<&'t Kind<'a>> {
        self.tokens.get(self.pos + n).map(|t| &t.kind)
    }

    fn identifier(&mut self, expected: &'static str) -> Parsed<(String, Span)> {
        match self.peek() {
            Some(Token {
                kind: Kind::Ident(name),
                span,
            }) => {
                self.pos += 1;
                Ok(((*name).to_owned(), *span))
            }
            Some(Token {
                kind: Kind::Reserved(word),
                span,
            }) => Err(ParseError::ReservedWord {
                span: *span,
                word: (*word).to_owned(),
            }),
            _ => Err(self.error(expected)),
        }
    }

    // ------------------------------------------------------------- fn decls

    fn fn_decl(&mut self, signature_only: bool) -> Parsed<FnDecl> {
        let (name, name_span) = self.identifier("a function name")?;
        self.expect(&Kind::LParen, "`(`")?;
        let params = self.params()?;
        let rparen = self.expect(&Kind::RParen, "`)`")?.span;

        let return_type = if self.at(&Kind::Colon) {
            self.pos += 1;
            Some(self.type_ref()?)
        } else {
            None
        };
        self.skip_where()?;

        // `w equals w` at top level: a newline is permitted on both sides.
        // Inside an `api` there is no `=` at all and the declaration ends here.
        let body = match self.optional_definition()? {
            Some(body) => Some(body),
            None if signature_only => None,
            None => {
                self.skip_newlines();
                return Err(self.error("`=`"));
            }
        };
        let end = body.as_ref().map_or(rparen, Expr::span);
        let span = Span::new(name_span.start, end.end);
        Ok(FnDecl {
            name,
            params,
            return_type,
            body,
            span,
        })
    }

    fn params(&mut self) -> Parsed<Vec<Param>> {
        let mut params = Vec::new();
        self.skip_newlines();
        if self.at(&Kind::RParen) {
            return Ok(params);
        }
        loop {
            let (name, name_span) = self.identifier("a parameter name")?;
            self.expect(&Kind::Colon, "`:`")?;
            let ty = self.type_ref()?;
            let span = Span::new(name_span.start, ty.span.end);
            params.push(Param { name, ty, span });
            self.skip_newlines();
            if !self.at(&Kind::Comma) {
                break;
            }
            self.pos += 1;
            self.skip_newlines();
        }
        Ok(params)
    }

    fn type_ref(&mut self) -> Parsed<TypeRef> {
        let (name, span) = self.identifier("a type name")?;
        if !self.at(&Kind::LGeneric) {
            return Ok(TypeRef {
                name,
                argument: None,
                span,
            });
        }
        // One static argument, which is all `Array[\T\]` needs. Generics
        // proper arrive with traits.
        self.pos += 1;
        let argument = self.type_ref()?;
        let close = self.expect(&Kind::RGeneric, "`\\]`")?.span;
        Ok(TypeRef {
            name,
            argument: Some(Box::new(argument)),
            span: Span::new(span.start, close.end),
        })
    }

    // --------------------------------------------------------- expressions

    fn expr(&mut self) -> Parsed<Expr> {
        self.comparison()
    }

    fn comparison(&mut self) -> Parsed<Expr> {
        let mut lhs = self.additive()?;
        while let Some(op) = self.peek_kind().and_then(comparison_op) {
            let index = self.pos;
            let Some(fixity) = self.infix_fixity(index)? else {
                break;
            };
            self.pos += 1;
            self.skip_newlines(); // a newline may follow an infix operator
            let rhs = self.additive()?;
            lhs = infix(op, fixity, lhs, rhs);
        }
        Ok(lhs)
    }

    fn additive(&mut self) -> Parsed<Expr> {
        let mut lhs = self.multiplicative()?;
        loop {
            let op = match self.peek_kind() {
                Some(Kind::Plus) => BinOp::Add,
                Some(Kind::Minus) => BinOp::Sub,
                _ => break,
            };
            let index = self.pos;
            let Some(fixity) = self.infix_fixity(index)? else {
                break;
            };
            self.pos += 1;
            self.skip_newlines();
            let rhs = self.multiplicative()?;
            lhs = infix(op, fixity, lhs, rhs);
        }
        Ok(lhs)
    }

    fn multiplicative(&mut self) -> Parsed<Expr> {
        let mut lhs = self.juxtaposition()?;
        loop {
            let op = match self.peek_kind() {
                Some(Kind::Star) => BinOp::Mul,
                Some(Kind::Slash) => BinOp::Div,
                _ => break,
            };
            let index = self.pos;
            let Some(fixity) = self.infix_fixity(index)? else {
                break;
            };
            self.pos += 1;
            self.skip_newlines();
            let rhs = self.juxtaposition()?;
            lhs = infix(op, fixity, lhs, rhs);
        }
        Ok(lhs)
    }

    /// `None` means "this operator is not infix here, stop climbing"; the
    /// juxtaposition layer picks it up as a prefix operator instead.
    fn infix_fixity(&self, index: usize) -> Parsed<Option<Fixity>> {
        match self.fixity_at(index) {
            OperatorShape::TightInfix => Ok(Some(Fixity::Tight)),
            OperatorShape::LooseInfix => Ok(Some(Fixity::Loose)),
            OperatorShape::Prefix => Ok(None),
            OperatorShape::Postfix => Err(ParseError::PostfixOperatorUnsupported {
                span: self.tokens.get(index).map_or(Span::new(0, 0), |t| t.span),
            }),
        }
    }

    /// Juxtaposition binds tighter than any loose infix operator and has no
    /// token of its own. The run stays flat: whether it is multiplication or
    /// string concatenation depends on operand types.
    fn juxtaposition(&mut self) -> Parsed<Expr> {
        let first = self.unary()?;
        let mut items = vec![first];
        while self.starts_juxt_operand() {
            items.push(self.unary()?);
        }
        if items.len() == 1 {
            return items.pop().ok_or(ParseError::UnexpectedEndOfInput {
                expected: "an operand",
            });
        }
        let span = match (items.first(), items.last()) {
            (Some(a), Some(b)) => Span::new(a.span().start, b.span().end),
            _ => Span::new(0, 0),
        };
        Ok(Expr::Juxt { items, span })
    }

    fn starts_juxt_operand(&self) -> bool {
        match self.peek_kind() {
            Some(
                Kind::IntLit { .. }
                | Kind::FloatLit { .. }
                | Kind::StrLit(_)
                | Kind::True
                | Kind::False
                | Kind::Ident(_)
                | Kind::LParen
                | Kind::LBracket,
            ) => true,
            // A minus that is spaced on the left and glued on the right is a
            // prefix operator on the next operand, not subtraction.
            Some(Kind::Minus | Kind::Plus) => {
                matches!(self.fixity_at(self.pos), OperatorShape::Prefix)
            }
            _ => false,
        }
    }

    fn unary(&mut self) -> Parsed<Expr> {
        let prefix = match self.peek_kind() {
            Some(Kind::Minus) => Some(UnOp::Neg),
            Some(Kind::Plus) => Some(UnOp::Pos),
            _ => None,
        };
        if let Some(op) = prefix {
            let span = self.span_here();
            self.pos += 1;
            let operand = self.unary()?;
            let full = Span::new(span.start, operand.span().end);
            return Ok(Expr::Prefix {
                op,
                operand: Box::new(operand),
                span: full,
            });
        }
        self.postfix()
    }

    fn postfix(&mut self) -> Parsed<Expr> {
        let mut expr = self.primary()?;
        // Only a glued `(` is an application. A spaced one is a juxtaposed
        // parenthesized expression, which the juxtaposition layer handles.
        loop {
            if self.at(&Kind::LParen) && self.glued_left(self.pos) {
                self.pos += 1;
                let args = self.args()?;
                let close = self.expect(&Kind::RParen, "`)`")?.span;
                let span = Span::new(expr.span().start, close.end);
                expr = Expr::Call {
                    callee: Box::new(expr),
                    args,
                    span,
                };
                continue;
            }
            if self.at(&Kind::Dot) {
                self.pos += 1;
                let (name, name_span) = self.identifier("a field or method name")?;
                let span = Span::new(expr.span().start, name_span.end);
                expr = Expr::Field {
                    base: Box::new(expr),
                    name,
                    span,
                };
                continue;
            }
            // A glued `[` subscripts; a spaced one opens an array literal that
            // the juxtaposition layer will pick up.
            if self.at(&Kind::LBracket) && self.glued_left(self.pos) {
                self.pos += 1;
                self.skip_newlines();
                let index = self.expr()?;
                self.skip_newlines();
                let close = self.expect(&Kind::RBracket, "`]`")?.span;
                let span = Span::new(expr.span().start, close.end);
                expr = Expr::Index {
                    base: Box::new(expr),
                    index: Box::new(index),
                    span,
                };
                continue;
            }
            break;
        }
        Ok(expr)
    }

    fn args(&mut self) -> Parsed<Vec<Expr>> {
        let mut args = Vec::new();
        self.skip_newlines();
        if self.at(&Kind::RParen) {
            return Ok(args);
        }
        loop {
            args.push(self.expr()?);
            self.skip_newlines();
            if !self.at(&Kind::Comma) {
                break;
            }
            self.pos += 1;
            self.skip_newlines();
        }
        Ok(args)
    }

    fn primary(&mut self) -> Parsed<Expr> {
        let Some(token) = self.peek() else {
            return Err(ParseError::UnexpectedEndOfInput {
                expected: "an expression",
            });
        };
        let span = token.span;
        match &token.kind {
            Kind::IntLit { digits, .. } => {
                self.pos += 1;
                Ok(Expr::IntLit {
                    digits: digits.clone(),
                    span,
                })
            }
            Kind::FloatLit {
                int_digits,
                frac_digits,
                ..
            } => {
                self.pos += 1;
                Ok(Expr::FloatLit {
                    int_digits: int_digits.clone(),
                    frac_digits: frac_digits.clone(),
                    span,
                })
            }
            Kind::StrLit(value) => {
                self.pos += 1;
                Ok(Expr::StrLit {
                    value: value.clone(),
                    span,
                })
            }
            Kind::True => {
                self.pos += 1;
                Ok(Expr::BoolLit { value: true, span })
            }
            Kind::False => {
                self.pos += 1;
                Ok(Expr::BoolLit { value: false, span })
            }
            Kind::Ident(name) => {
                let name = (*name).to_owned();
                self.pos += 1;
                Ok(Expr::Var { name, span })
            }
            // Only reachable inside a method body, which is parsed and never
            // checked, so `self` never has to resolve to anything.
            Kind::KwSelf => {
                self.pos += 1;
                Ok(Expr::Var {
                    name: "self".to_owned(),
                    span,
                })
            }
            Kind::LParen => {
                self.pos += 1;
                self.skip_newlines();
                let inner = self.expr()?;
                self.skip_newlines();
                self.expect(&Kind::RParen, "`)`")?;
                Ok(inner)
            }
            Kind::KwIf => self.if_expr(),
            Kind::KwDo => self.block(),
            Kind::KwWhile => self.while_expr(),
            Kind::LBracket => self.array_literal(),
            Kind::Reserved(word) => Err(ParseError::ReservedWord {
                span,
                word: (*word).to_owned(),
            }),
            _ => Err(self.error("an expression")),
        }
    }

    fn array_literal(&mut self) -> Parsed<Expr> {
        let start = self.expect(&Kind::LBracket, "`[`")?.span;
        let mut items = Vec::new();
        self.skip_newlines();
        if !self.at(&Kind::RBracket) {
            loop {
                items.push(self.expr()?);
                self.skip_newlines();
                if !self.at(&Kind::Comma) {
                    break;
                }
                self.pos += 1;
                self.skip_newlines();
            }
        }
        let close = self.expect(&Kind::RBracket, "`]`")?.span;
        Ok(Expr::ArrayLit {
            items,
            span: Span::new(start.start, close.end),
        })
    }

    /// `while cond do ... end`. The only loop in the language until generators
    /// arrive, and deliberately so: `for` is parallel by default in Fortress
    /// and cannot be faked with a counter.
    fn while_expr(&mut self) -> Parsed<Expr> {
        let start = self.expect(&Kind::KwWhile, "`while`")?.span;
        self.skip_newlines();
        let cond = self.expr()?;
        self.skip_newlines();
        self.expect(&Kind::KwDo, "`do`")?;
        let body = self.block_body(&[Kind::KwEnd])?;
        let end = self.expect(&Kind::KwEnd, "`end`")?.span;
        Ok(Expr::While {
            cond: Box::new(cond),
            body: Box::new(body),
            span: Span::new(start.start, end.end),
        })
    }

    fn if_expr(&mut self) -> Parsed<Expr> {
        let start = self.expect(&Kind::KwIf, "`if`")?.span;
        self.skip_newlines();
        let cond = self.expr()?;
        self.skip_newlines();
        self.expect(&Kind::KwThen, "`then`")?;
        let then_branch = self.block_body(&[Kind::KwElse, Kind::KwElif, Kind::KwEnd])?;

        let else_branch = if self.at(&Kind::KwElse) {
            self.pos += 1;
            Some(Box::new(self.block_body(&[Kind::KwEnd])?))
        } else if self.at(&Kind::KwElif) {
            self.pos += 1;
            Some(Box::new(self.elif_tail()?))
        } else {
            None
        };

        let end = self.expect(&Kind::KwEnd, "`end`")?.span;
        Ok(Expr::If {
            cond: Box::new(cond),
            then_branch: Box::new(then_branch),
            else_branch,
            span: Span::new(start.start, end.end),
        })
    }

    /// `elif` is `else if` without its own `end`, so the tail reuses the
    /// enclosing `end`.
    fn elif_tail(&mut self) -> Parsed<Expr> {
        self.skip_newlines();
        let start = self.span_here();
        let cond = self.expr()?;
        self.skip_newlines();
        self.expect(&Kind::KwThen, "`then`")?;
        let then_branch = self.block_body(&[Kind::KwElse, Kind::KwElif, Kind::KwEnd])?;
        let else_branch = if self.at(&Kind::KwElse) {
            self.pos += 1;
            Some(Box::new(self.block_body(&[Kind::KwEnd])?))
        } else if self.at(&Kind::KwElif) {
            self.pos += 1;
            Some(Box::new(self.elif_tail()?))
        } else {
            None
        };
        let end = self.span_here();
        Ok(Expr::If {
            cond: Box::new(cond),
            then_branch: Box::new(then_branch),
            else_branch,
            span: Span::new(start.start, end.end),
        })
    }

    fn block(&mut self) -> Parsed<Expr> {
        let start = self.expect(&Kind::KwDo, "`do`")?.span;
        let body = self.block_body(&[Kind::KwEnd])?;
        let end = self.expect(&Kind::KwEnd, "`end`")?.span;
        match body {
            Expr::Block { items, .. } => Ok(Expr::Block {
                items,
                span: Span::new(start.start, end.end),
            }),
            other => Ok(Expr::Block {
                items: vec![BlockItem::Expr(other)],
                span: Span::new(start.start, end.end),
            }),
        }
    }

    /// A run of block elements up to, but not consuming, one of `terminators`.
    fn block_body(&mut self, terminators: &[Kind<'_>]) -> Parsed<Expr> {
        let start = self.span_here();
        let mut items = Vec::new();
        self.skip_newlines();

        while !self.at_any(terminators) && !self.at_eof() {
            items.push(self.block_item()?);
            if self.at_any(terminators) {
                break;
            }
            self.expect_separator()?;
        }

        let end = self.span_here();
        if items.len() == 1 {
            if let Some(BlockItem::Expr(_)) = items.first() {
                if let Some(BlockItem::Expr(e)) = items.pop() {
                    return Ok(e);
                }
            }
        }
        Ok(Expr::Block {
            items,
            span: Span::new(start.start, end.start),
        })
    }

    fn at_any(&self, kinds: &[Kind<'_>]) -> bool {
        kinds.iter().any(|k| self.at(k))
    }

    /// A binding is `Ident (: Type)? = Expr`. `LocalDecl.rats:159` writes `s`
    /// before the `=`, so a newline there means this is not a binding at all.
    fn block_item(&mut self) -> Parsed<BlockItem> {
        let save = self.pos;
        if let Some(binding) = self.try_binding()? {
            return Ok(BlockItem::Binding(binding));
        }
        self.pos = save;
        let target = self.expr()?;
        if !self.at(&Kind::ColonEq) {
            return Ok(BlockItem::Expr(target));
        }
        self.pos += 1;
        self.skip_newlines();
        let value = self.expr()?;
        let span = Span::new(target.span().start, value.span().end);
        Ok(BlockItem::Assign(Assign {
            target,
            value,
            span,
        }))
    }

    fn try_binding(&mut self) -> Parsed<Option<Binding>> {
        let Some(Token {
            kind: Kind::Ident(name),
            span: name_span,
        }) = self.peek()
        else {
            return Ok(None);
        };
        let name = (*name).to_owned();
        let name_span = *name_span;
        let save = self.pos;
        self.pos += 1;

        let ty = if self.at(&Kind::Colon) {
            self.pos += 1;
            match self.type_ref() {
                Ok(t) => Some(t),
                Err(_) => {
                    self.pos = save;
                    return Ok(None);
                }
            }
        } else {
            None
        };

        // No `skip_newlines` here on purpose: the `=` must be on this line.
        // `:=` declares a mutable binding, but only with a type annotation:
        // without one `i := 0` would be a declaration in some scopes and an
        // assignment in others, which is how a typo silently shadows.
        let mutable = match self.peek_kind() {
            Some(Kind::Eq) => false,
            Some(Kind::ColonEq) if ty.is_some() => true,
            _ => {
                self.pos = save;
                return Ok(None);
            }
        };
        self.pos += 1;
        self.skip_newlines(); // `w` after the `=` does permit one

        let value = self.expr()?;
        let span = Span::new(name_span.start, value.span().end);
        Ok(Some(Binding {
            name,
            ty,
            value,
            mutable,
            span,
        }))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OperatorShape {
    TightInfix,
    LooseInfix,
    Prefix,
    Postfix,
}

const fn comparison_op(kind: &Kind<'_>) -> Option<BinOp> {
    match kind {
        Kind::Lt => Some(BinOp::Lt),
        Kind::Gt => Some(BinOp::Gt),
        Kind::Le => Some(BinOp::Le),
        Kind::Ge => Some(BinOp::Ge),
        Kind::EqEqEq => Some(BinOp::Eq),
        Kind::NotEq => Some(BinOp::Ne),
        _ => None,
    }
}

fn infix(op: BinOp, fixity: Fixity, lhs: Expr, rhs: Expr) -> Expr {
    let span = Span::new(lhs.span().start, rhs.span().end);
    Expr::Infix {
        op,
        fixity,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
        span,
    }
}
