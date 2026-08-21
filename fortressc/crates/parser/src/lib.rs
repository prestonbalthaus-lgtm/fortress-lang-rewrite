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
    ImportDecl, Member, MethodDecl, Modifiers, ObjectDecl, Param, Span, StaticParam, TraitDecl,
    TypeRef, UnOp,
};
use fortress_lexer::{Kind, Token};

type Parsed<T> = Result<T, ParseError>;

/// A parenthesised type is the type itself, but its span covers the
/// parentheses, so a diagnostic points at what was written.
fn widen(t: TypeRef, span: Span) -> TypeRef {
    match t {
        TypeRef::Named { name, args, .. } => TypeRef::Named { name, args, span },
        TypeRef::Unit { .. } => TypeRef::Unit { span },
        TypeRef::Tuple { elems, .. } => TypeRef::Tuple { elems, span },
        TypeRef::Arrow { from, to, .. } => TypeRef::Arrow { from, to, span },
    }
}

/// What `opr` parsed, before the caller decides whether a body follows.
struct OprSignature {
    name: String,
    static_params: Vec<StaticParam>,
    params: Vec<Param>,
    return_type: Option<TypeRef>,
    end: Span,
}

/// The characters an operator name may be spelled out of, as their own text.
///
/// `[\` and `\]` are deliberately absent: they open a static-parameter list,
/// which is part of the declaration and not part of the operator's name. `(`
/// is absent for the same reason.
fn operator_text(kind: &Kind<'_>) -> Option<&'static str> {
    Some(match kind {
        Kind::Plus => "+",
        Kind::Minus => "-",
        Kind::Star => "*",
        Kind::Slash => "/",
        Kind::SlashSlash => "//",
        Kind::SlashSlashSlash => "///",
        Kind::Caret => "^",
        Kind::Hash => "#",
        Kind::Bar => "|",
        Kind::BarBar => "||",
        Kind::LeftBar => "<|",
        Kind::RightBar => "|>",
        Kind::Backslash => "\\",
        Kind::Lt => "<",
        Kind::Gt => ">",
        Kind::Le => "<=",
        Kind::Ge => ">=",
        Kind::Eq => "=",
        Kind::EqEqEq => "===",
        Kind::NotEq => "=/=",
        Kind::FatArrow => "=>",
        Kind::Colon => ":",
        Kind::ColonEq => ":=",
        Kind::LBracket => "[",
        Kind::RBracket => "]",
        Kind::LBrace => "{",
        Kind::RBrace => "}",
        _ => return None,
    })
}

fn join(run: &[&'static str]) -> String {
    run.concat()
}

pub fn parse(tokens: &[Token<'_>]) -> Parsed<Component> {
    Parser {
        tokens,
        pos: 0,
        chain_temps: 0,
    }
    .component()
}

struct Parser<'t, 'a> {
    tokens: &'t [Token<'a>],
    pos: usize,
    /// Monotonic and never reset, so nested chains cannot collide. `$` cannot
    /// appear in a source identifier -- the property `mangle_type` already
    /// relies on -- so a temporary cannot shadow anything the user wrote.
    chain_temps: usize,
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

    /// The span of the token just consumed, for a construct whose end is a
    /// statement rather than a closing token.
    fn previous_span(&self) -> Span {
        self.pos
            .checked_sub(1)
            .and_then(|i| self.tokens.get(i))
            .map_or_else(|| self.span_here(), |t| t.span)
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

    /// `x += e`. `+=` is two tokens -- `Plus` then `Eq` -- and adjacency is
    /// what joins them, the same trade `<-` and `for` take: no lexer change, so
    /// no file in the corpus lexes differently. The last one was M3h.
    fn compound_op_at(&self, index: usize) -> Option<BinOp> {
        let op = match self.tokens.get(index)?.kind {
            Kind::Plus => BinOp::Add,
            Kind::Minus => BinOp::Sub,
            _ => return None,
        };
        if !matches!(self.tokens.get(index + 1)?.kind, Kind::Eq) {
            return None;
        }
        self.glued_right(index).then_some(op)
    }

    /// `...` after a parameter's type. `Symbol.rats:212` makes `ellipses` one
    /// lexical token; this parser has three `Dot`s, so the three must be glued
    /// to EACH OTHER -- the same trade `->`, `<-` and `+=` take, and no file in
    /// the corpus lexes differently for it.
    ///
    /// `Parameter.rats:88` is `BindId w colon w Type w ellipses`, so the run is
    /// NOT required to be glued to the type: `Any...` and `Any ...` are the
    /// same declaration.
    fn at_ellipsis(&self) -> bool {
        let three = (0..3).all(|n| matches!(self.peek_ahead(n), Some(Kind::Dot)));
        three && self.glued_right(self.pos) && self.glued_right(self.pos + 1)
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
        // `native component File`. The modifier belongs to the component, not
        // to whatever follows it, and reading it here is what stops `native`
        // being consumed by `decl` and leaving `component` where a function
        // name was expected. It is read and DROPPED: `Component` has no
        // modifiers field, and a native component's bodies live in C -- which
        // is a milestone, not a flag.
        self.modifiers();
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
        } else if is_api {
            self.named_end(&Kind::KwApi, &name)?
        } else {
            self.named_end(&Kind::KwComponent, &name)?
        };
        Ok(Component {
            name,
            exports,
            imports,
            decls,
            bounds: Vec::new(),
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
        // `value object`, `private scale(x: ZZ32) = ...`, `abstract` -- the
        // modifiers come before whatever they modify, so they are read before
        // the shape is decided rather than in each branch.
        let modifiers = self.modifiers();
        match self.peek_kind() {
            Some(Kind::KwTrait) => Ok(Decl::Trait(self.trait_decl(modifiers)?)),
            Some(Kind::KwObject) => Ok(Decl::Object(self.object_decl(modifiers)?)),
            // `opr` reached the parser only to be refused as a reserved word,
            // so this branch cannot change how anything that parses today
            // parses -- it is entered on a token that was always an error.
            Some(Kind::Reserved("opr")) => {
                Ok(Decl::Function(self.opr_decl(modifiers, signature_only)?))
            }
            // `pi: RR64 = 3.14`, `v = 1`, `x := 0`, and the initializer-less
            // `stdIn: Reader` an api declares. A function declaration is always
            // `Ident` then `[\` or `(`, so none of these three tokens can begin
            // one and the branch cannot steal a function.
            Some(Kind::Ident(_))
                if matches!(
                    self.peek_ahead(1),
                    Some(Kind::Colon | Kind::Eq | Kind::ColonEq)
                ) =>
            {
                Ok(Decl::Function(self.value_decl(modifiers)?))
            }
            _ => Ok(Decl::Function(self.fn_decl(modifiers, signature_only)?)),
        }
    }

    /// A component-level value declaration, carried as a nullary `FnDecl`
    /// because the AST has no declaration node for a value yet. Deliberate
    /// approximation: this is a parse-only spike, and mutability is dropped.
    fn value_decl(&mut self, modifiers: Modifiers) -> Parsed<FnDecl> {
        let (name, name_span) = self.identifier("a value name")?;
        let ty = if self.at(&Kind::Colon) {
            self.pos += 1;
            Some(self.type_ref()?)
        } else {
            None
        };
        // `:=` needs no type annotation here, unlike in a block: component
        // level has no assignment statements, so there is nothing for a bare
        // `x := 0` to be confused with.
        let save = self.pos;
        self.skip_newlines();
        let body = if self.at(&Kind::Eq) || self.at(&Kind::ColonEq) {
            self.pos += 1;
            self.skip_newlines();
            Some(self.expr()?)
        } else {
            self.pos = save;
            None
        };
        let end = body
            .as_ref()
            .map_or_else(|| ty.as_ref().map_or(name_span, TypeRef::span), Expr::span);
        Ok(FnDecl {
            modifiers,
            name,
            static_params: Vec::new(),
            params: Vec::new(),
            return_type: ty,
            body,
            value_binding: true,
            span: Span::new(name_span.start, end.end),
        })
    }

    // ------------------------------------------------------ traits and objects

    /// `comprises` and `excludes` are recorded and never read: exclusion is
    /// decided from the concrete types the program actually declares, which a
    /// whole-program compiler can see and a modular one cannot.
    fn trait_decl(&mut self, modifiers: Modifiers) -> Parsed<TraitDecl> {
        let start = self.expect(&Kind::KwTrait, "`trait`")?.span;
        let (name, _) = self.identifier("a trait name")?;
        let static_params = self.static_params()?;
        let (extends, comprises, excludes) = self.topology_clauses()?;
        self.skip_where()?;
        let members = self.members()?;
        let end = self.named_end(&Kind::KwTrait, &name)?;
        Ok(TraitDecl {
            modifiers,
            name,
            static_params,
            extends,
            comprises,
            excludes,
            members,
            span: Span::new(start.start, end.end),
        })
    }

    /// No parameter list at all is a singleton: one instance, constructed once
    /// before `run`. `object O() ... end` is a constructor taking nothing.
    fn object_decl(&mut self, modifiers: Modifiers) -> Parsed<ObjectDecl> {
        let start = self.expect(&Kind::KwObject, "`object`")?.span;
        let (name, _) = self.identifier("an object name")?;
        let static_params = self.static_params()?;
        let params = if self.at(&Kind::LParen) {
            self.pos += 1;
            let params = self.params()?;
            self.expect(&Kind::RParen, "`)`")?;
            // `objects.tex:100` spells an object's varargs parameter
            // `transient Varargs`, so the bare form is a static error rather
            // than a declaration this parser has not got to yet. `transient`
            // is not even a reserved word here, so the modifier-carrying form
            // cannot be written at all -- which makes refusing the whole
            // shape the honest reading.
            if let Some(p) = params.iter().find(|p| p.varargs) {
                return Err(ParseError::ObjectVarargsParameter {
                    span: p.span,
                    name: p.name.clone(),
                });
            }
            Some(params)
        } else {
            None
        };
        let (extends, comprises, excludes) = self.topology_clauses()?;
        self.skip_where()?;
        let members = self.members()?;
        let end = self.named_end(&Kind::KwObject, &name)?;
        Ok(ObjectDecl {
            modifiers,
            name,
            static_params,
            params,
            extends,
            comprises,
            excludes,
            members,
            span: Span::new(start.start, end.end),
        })
    }

    /// Any run of `abstract`, `value`, `native` and `private`, in any order.
    ///
    /// All four are already RESERVED words, so this branch is reachable only on
    /// a token that was previously always an error -- which is the whole
    /// regression argument, the same one the `opr` intercept rests on.
    ///
    /// `atomic`, `io` and `test` are NOT read here. The first two are named
    /// deviations with diagnostics of their own and swallowing them would make
    /// them silent; `test` waits for a measurement rather than a guess.
    fn modifiers(&mut self) -> Modifiers {
        let mut found = Modifiers::default();
        loop {
            let slot = match self.peek_kind() {
                Some(Kind::Reserved("abstract")) => &mut found.abstract_,
                Some(Kind::Reserved("value")) => &mut found.value,
                Some(Kind::Reserved("native")) => &mut found.native,
                Some(Kind::Reserved("private")) => &mut found.private,
                _ => return found,
            };
            *slot = true;
            self.pos += 1;
        }
    }

    /// `extends`, `comprises` and `excludes`, in ANY order and each ON ITS OWN
    /// LINE if the source puts it there.
    ///
    /// The newline is what 22 of the library's 114 files were dying on, and the
    /// diagnostic said so from the wrong side: the clause landed where a member
    /// was expected, so the parser reported `expected a field or method name,
    /// found KwExtends` on something that is not a member at all.
    ///
    /// ```text
    /// object KeyOverlap[\Key, Val\](key: Key, val1: Val, val2: Val)
    ///         extends UncheckedException
    /// end
    /// ```
    ///
    /// Reading them in a loop rather than in a fixed order costs nothing and
    /// removes a second way to be wrong; `members()` opens with `skip_newlines`
    /// either way, so the position is restored when no clause follows.
    fn topology_clauses(&mut self) -> Parsed<(Vec<TypeRef>, Vec<TypeRef>, Vec<TypeRef>)> {
        let (mut extends, mut comprises, mut excludes) = (Vec::new(), Vec::new(), Vec::new());
        loop {
            let save = self.pos;
            self.skip_newlines();
            let slot = match self.peek_kind() {
                Some(Kind::KwExtends) => &mut extends,
                Some(Kind::KwComprises) => &mut comprises,
                Some(Kind::KwExcludes) => &mut excludes,
                _ => {
                    self.pos = save;
                    return Ok((extends, comprises, excludes));
                }
            };
            if !slot.is_empty() {
                return Err(self.error("one `extends`, `comprises` or `excludes` clause each"));
            }
            self.pos += 1;
            *slot = self.type_set()?;
        }
    }

    /// `T extends U` in a static-parameter list. A bound is not a topology
    /// clause -- it constrains one parameter and cannot be followed by
    /// `comprises` -- so it reads the type list directly.
    fn extends_clause(&mut self) -> Parsed<Vec<TypeRef>> {
        if !self.at(&Kind::KwExtends) {
            return Ok(Vec::new());
        }
        self.pos += 1;
        self.type_set()
    }

    /// The type list a topology clause names: one type, or a braced set.
    fn type_set(&mut self) -> Parsed<Vec<TypeRef>> {
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
        // `comprises { ... }` -- 32 corpus sites -- says the set is open rather
        // than naming it. There is no `...` token, so it is three `Dot`s. The
        // marker is DROPPED, which is honest while nothing reads `comprises`:
        // an open set and an unwritten one are the same empty list today.
        if self.at(&Kind::Dot) {
            while self.at(&Kind::Dot) {
                self.pos += 1;
            }
            self.skip_newlines();
            self.expect(&Kind::RBrace, "`}`")?;
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
        // A member takes the same modifiers a declaration does -- `abstract opr
        // <(self, other: T)`, `private Min_W: ZZ32 = -1` -- and they come
        // first, before `getter`/`setter`.
        let modifiers = self.modifiers();
        // `getter f(): T = e` and `setter f(x: T) = e`. The modifier changes
        // how the member is *invoked* -- `x.f` rather than `x.f()` -- so it is
        // recorded and M3i leaves accessors out of the dotted method sets.
        let accessor = self.at(&Kind::KwGetter) || self.at(&Kind::KwSetter);
        if accessor {
            self.pos += 1;
        }
        let mutable = if self.at(&Kind::KwVar) {
            self.pos += 1;
            true
        } else {
            false
        };
        // Same intercept as at declaration level, and the library needs both:
        // `opr |self| : ZZ64` is a member of `trait Integral`, `opr COMPOSE`
        // is top level.
        if !accessor && !mutable && self.at(&Kind::Reserved("opr")) {
            return Ok(Member::Method(self.opr_member(modifiers)?));
        }
        let (name, name_span) = self.identifier("a field or method name")?;

        let static_params = self.static_params()?;
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
                modifiers,
                name,
                static_params,
                params,
                return_type,
                body,
                accessor,
                span: Span::new(start.start, end.end),
            }));
        }

        self.expect(&Kind::Colon, "`:` or `(`")?;
        let ty = self.type_ref()?;
        let init = self.optional_definition()?;
        let end = init.as_ref().map_or_else(|| ty.span(), Expr::span);
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

    /// `end`, `end Stream`, `end trait Stream`. `TraitObject.rats:13` writes
    /// the tail as `((s "trait")? s Id)?`, and `s` -- space WITHOUT a line
    /// terminator -- is the whole disambiguation: `end` then a NEWLINE then a
    /// name is the end of this declaration followed by the next one, and only a
    /// name on the SAME LINE belongs to the `end`. The newline is a token here,
    /// so "same line" is just "the next token is not `Newline`".
    ///
    /// Only the three declaration forms the grammar gives the tail to reach
    /// this. `do ... end`, `if ... end` and `while ... end` deliberately do not:
    /// `end out` and `end loop` in the corpus close a LABELLED BLOCK, which is
    /// a different production and a feature this compiler does not have.
    fn named_end(&mut self, keyword: &Kind<'_>, own: &str) -> Parsed<Span> {
        let end = self.expect(&Kind::KwEnd, "`end`")?.span;
        // Only step over the keyword when a name actually follows it, so a
        // stray `end trait` cannot consume a token and then fail elsewhere.
        if self.at(keyword) && matches!(self.peek_ahead(1), Some(Kind::Ident(_))) {
            self.pos += 1;
        }
        if !matches!(self.peek_kind(), Some(Kind::Ident(_))) {
            return Ok(end);
        }
        let start = self.span_here();
        let name = self.dotted_name()?;
        let span = Span::new(start.start, self.previous_span().end);
        if name != own {
            return Err(ParseError::ClosingNameDiffers {
                span,
                found: name,
                expected: own.to_owned(),
            });
        }
        Ok(Span::new(end.start, span.end))
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

    fn fn_decl(&mut self, modifiers: Modifiers, signature_only: bool) -> Parsed<FnDecl> {
        let (name, name_span) = self.identifier("a function name")?;
        let static_params = self.static_params()?;
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
            modifiers,
            name,
            static_params,
            params,
            return_type,
            body,
            value_binding: false,
            span,
        })
    }

    // ------------------------------------------------------------ operators

    /// `opr` at declaration level. Lifted to an ordinary `FnDecl` whose name is
    /// the operator's own text, which is what makes this a parse spike and not
    /// a language feature: nothing downstream learns a new node.
    fn opr_decl(&mut self, modifiers: Modifiers, signature_only: bool) -> Parsed<FnDecl> {
        let start = self.span_here();
        self.pos += 1;
        let sig = self.opr_signature()?;
        let body = match self.optional_definition()? {
            Some(body) => Some(body),
            None if signature_only => None,
            None => {
                self.skip_newlines();
                return Err(self.error("`=`"));
            }
        };
        let end = body.as_ref().map_or(sig.end, Expr::span);
        Ok(FnDecl {
            modifiers,
            name: sig.name,
            static_params: sig.static_params,
            params: sig.params,
            return_type: sig.return_type,
            body,
            value_binding: false,
            span: Span::new(start.start, end.end),
        })
    }

    /// `opr` inside a trait or object. A member, so its body is optional in a
    /// `.fss` too -- an abstract operator declaration is ordinary Fortress.
    fn opr_member(&mut self, modifiers: Modifiers) -> Parsed<MethodDecl> {
        let start = self.span_here();
        self.pos += 1;
        let sig = self.opr_signature()?;
        let body = self.optional_definition()?;
        let end = body.as_ref().map_or(sig.end, Expr::span);
        Ok(MethodDecl {
            modifiers,
            name: sig.name,
            static_params: sig.static_params,
            params: sig.params,
            return_type: sig.return_type,
            body,
            accessor: false,
            span: Span::new(start.start, end.end),
        })
    }

    /// Everything between `opr` and the optional `= body`, which is the same
    /// text in both positions.
    ///
    /// FOUR SHAPES, and the corpus needs all four -- `Library/FortressLibrary.fsi`
    /// alone writes 450 of them:
    ///
    /// * infix or prefix, `opr CMP(self, o: T): Comparison` and `opr -(self)`,
    ///   which is the bulk;
    /// * ENCLOSING, `opr |self| : ZZ64` and `opr |\self/| : ZZ64` -- the
    ///   operand is written INSIDE the brackets, so there is no parameter list
    ///   in the ordinary place;
    /// * a LEADING OPERAND, `opr (l: I)::[\I\](s: I): I`, where the left
    ///   operand is written before the operator. Both lists flatten into one;
    /// * `BIG`, which is a modifier rather than a name: `opr BIG SQCAP[\T\]()`
    ///   folds to the name `BIG SQCAP` and then reads like any of the above.
    fn opr_signature(&mut self) -> Parsed<OprSignature> {
        let mut name = String::new();
        // `BIG` is a RESERVED word rather than an identifier, so it cannot be
        // read as the operator's own name by accident.
        if self.at(&Kind::Reserved("BIG")) {
            self.pos += 1;
            name.push_str("BIG ");
        }

        // The leading-operand form. Its parameters are written first and join
        // the trailing ones, because a lifted operator is one function.
        let mut params = Vec::new();
        if self.at(&Kind::LParen) {
            self.pos += 1;
            params = self.params()?;
            self.expect(&Kind::RParen, "`)`")?;
        }

        if let Some(Kind::Ident(word)) = self.peek_kind() {
            name.push_str(word);
            self.pos += 1;
            let static_params = self.static_params()?;
            return self.opr_tail(name, params, static_params);
        }

        let open = self.operator_run(usize::MAX);
        if open.is_empty() {
            return Err(self.error("an operator name after `opr`"));
        }

        // An enclosing operator carries its static parameters BETWEEN the
        // opener and the operand -- `opr <|[\E\] xs: E... |>` -- which is
        // where the six library declarations of the list, set and prefix-set
        // brackets put them. Reading them here rather than in `opr_tail` is
        // what lets the branch below see the operand.
        //
        // The infix and prefix forms write them in the same place relative to
        // the name (`opr +[\T\](a: T, b: T)`), so hoisting the call out of
        // `opr_tail` moves no declaration that already parsed.
        let static_params = self.static_params()?;

        // An operand written inside the brackets makes this an enclosing
        // operator: `|self|`, `|\self/|`, `|/self\|`, `[i: ZZ32]`.
        //
        // The operand is OPTIONAL. `opr BIG <|[\T\]|>` is the comprehension
        // bracket and writes none, so what identifies the form is the opener
        // closing again rather than the operand being there. `operator_run`
        // stops at `(`, `[\` and an identifier, so an infix declaration can
        // never produce a non-empty run in that position.
        let has_operand =
            self.at(&Kind::KwSelf) || matches!(self.peek_kind(), Some(Kind::Ident(_)));
        let mark = self.pos;
        let inner = if has_operand {
            self.params()?
        } else {
            Vec::new()
        };
        let close = self.closing_operator_run();
        if !close.is_empty() {
            params.extend(inner);
            // `_` marks where the operand goes, and it is what keeps the
            // enclosing `|self|` from being given the name `||`, which is a
            // real and different infix operator.
            name.push_str(&join(&open));
            name.push('_');
            name.push_str(&join(&close));
            let mut end = self.previous_span();
            let return_type = if self.at(&Kind::Colon) {
                self.pos += 1;
                let ty = self.type_ref()?;
                end = ty.span();
                Some(ty)
            } else {
                None
            };
            self.skip_throws()?;
            return Ok(OprSignature {
                name,
                static_params,
                params,
                return_type,
                end,
            });
        }
        if has_operand {
            return Err(self.error("the closing half of an enclosing operator"));
        }

        // The run was the operator's own name after all.
        self.pos = mark;
        name.push_str(&join(&open));
        self.opr_tail(name, params, static_params)
    }

    /// The part an operator declaration shares with a function's:
    /// `[\statics\] (params): T where {...} throws E`.
    fn opr_tail(
        &mut self,
        name: String,
        mut params: Vec<Param>,
        static_params: Vec<StaticParam>,
    ) -> Parsed<OprSignature> {
        self.expect(&Kind::LParen, "`(`")?;
        params.extend(self.params()?);
        let mut end = self.expect(&Kind::RParen, "`)`")?.span;
        let return_type = if self.at(&Kind::Colon) {
            self.pos += 1;
            let ty = self.type_ref()?;
            end = ty.span();
            Some(ty)
        } else {
            None
        };
        self.skip_where()?;
        self.skip_throws()?;
        Ok(OprSignature {
            name,
            static_params,
            params,
            return_type,
            end,
        })
    }

    /// The closing half of an enclosing operator, which is NOT the same run as
    /// the opening half and cannot be read with the opener's length as a limit.
    /// `Library/Map.fsi:100` writes `opr {|->[\Key,Val\] xs:(Key,Val)... }`:
    /// four characters open it and one closes it.
    ///
    /// What bounds it instead is the three tokens that can only END a
    /// declaration -- `:` before a return type, `=` before a body, `:=` -- none
    /// of which is a bracket, and all three of which `operator_text` maps
    /// because an INFIX operator may be named out of them.
    fn closing_operator_run(&mut self) -> Vec<&'static str> {
        let mut run = Vec::new();
        loop {
            if matches!(
                self.peek_kind(),
                Some(Kind::Colon | Kind::ColonEq | Kind::Eq)
            ) {
                break;
            }
            let Some(text) = self.peek_kind().and_then(operator_text) else {
                break;
            };
            if !run.is_empty() && !self.glued_right(self.pos - 1) {
                break;
            }
            run.push(text);
            self.pos += 1;
        }
        run
    }

    /// Up to `limit` operator characters, glued to each other. Adjacency is what
    /// makes `<->` one name and stops the run reaching into whatever follows --
    /// the same rule six milestones have used for `->`, `+=` and `**`, and it
    /// needs no lexer token per operator.
    ///
    /// It stops at `(`, `[\`, an identifier and `self` on purpose: each of those
    /// begins the operand, and which one it is decides the shape.
    fn operator_run(&mut self, limit: usize) -> Vec<&'static str> {
        let mut run = Vec::new();
        while run.len() < limit {
            let Some(text) = self.peek_kind().and_then(operator_text) else {
                break;
            };
            if !run.is_empty() && !self.glued_right(self.pos - 1) {
                break;
            }
            run.push(text);
            self.pos += 1;
        }
        run
    }

    /// `throws NotFound`. Recorded nowhere: this is a parse spike, and an
    /// exception clause has no meaning until the language has exceptions.
    fn skip_throws(&mut self) -> Parsed<()> {
        if !self.at(&Kind::Reserved("throws")) {
            return Ok(());
        }
        self.pos += 1;
        self.type_ref()?;
        while self.at(&Kind::Comma) {
            self.pos += 1;
            self.type_ref()?;
        }
        Ok(())
    }

    fn params(&mut self) -> Parsed<Vec<Param>> {
        let mut params = Vec::new();
        self.skip_newlines();
        if self.at(&Kind::RParen) {
            return Ok(params);
        }
        loop {
            // `self` is a parameter with no written type: it stands for the
            // enclosing trait or object, which is what makes the declaration a
            // functional method rather than a function. `Self` is a reserved
            // word, so the placeholder cannot collide with a declared type.
            if self.at(&Kind::KwSelf) {
                let span = self.span_here();
                self.pos += 1;
                params.push(Param {
                    name: "self".to_owned(),
                    ty: TypeRef::Named {
                        name: "Self".to_owned(),
                        args: Vec::new(),
                        span,
                    },
                    varargs: false,
                    span,
                });
                self.skip_newlines();
                if !self.at(&Kind::Comma) {
                    break;
                }
                self.pos += 1;
                self.skip_newlines();
                continue;
            }
            let (name, name_span) = self.identifier("a parameter name")?;
            self.expect(&Kind::Colon, "`:`")?;
            let ty = self.type_ref()?;
            let mut end = ty.span().end;
            let varargs = self.at_ellipsis();
            if varargs {
                self.pos += 3;
                end = self.previous_span().end;
            }
            let span = Span::new(name_span.start, end);
            params.push(Param {
                name,
                ty,
                varargs,
                span,
            });
            self.skip_newlines();
            if !self.at(&Kind::Comma) {
                break;
            }
            self.pos += 1;
            self.skip_newlines();
        }
        Ok(params)
    }

    /// `A -> B`, right associative. `->` is not a token: it is `Minus` glued to
    /// `Gt`, decided by span adjacency the same way operator fixity is, so the
    /// lexer does not have to learn a token that would change how every `->` in
    /// the corpus lexes.
    fn type_ref(&mut self) -> Parsed<TypeRef> {
        let from = self.type_atom()?;
        if !(self.at(&Kind::Minus)
            && self.glued_right(self.pos)
            && matches!(self.peek_ahead(1), Some(Kind::Gt)))
        {
            return Ok(from);
        }
        let start = from.span().start;
        self.pos += 2;
        self.skip_newlines();
        let to = self.type_ref()?;
        let end = to.span().end;
        Ok(TypeRef::Arrow {
            from: Box::new(from),
            to: Box::new(to),
            span: Span::new(start, end),
        })
    }

    fn type_atom(&mut self) -> Parsed<TypeRef> {
        if self.at(&Kind::LParen) {
            let start = self.expect(&Kind::LParen, "`(`")?.span.start;
            self.skip_newlines();
            if self.at(&Kind::RParen) {
                let end = self.expect(&Kind::RParen, "`)`")?.span.end;
                return Ok(TypeRef::Unit {
                    span: Span::new(start, end),
                });
            }
            let mut elems = vec![self.type_ref()?];
            self.skip_newlines();
            while self.at(&Kind::Comma) {
                self.pos += 1;
                self.skip_newlines();
                elems.push(self.type_ref()?);
                self.skip_newlines();
            }
            let end = self.expect(&Kind::RParen, "`)`")?.span.end;
            let span = Span::new(start, end);
            // Two or more is the whole definition of a tuple, and this is the
            // only place the invariant is enforced.
            if elems.len() == 1 {
                return Ok(widen(elems.remove(0), span));
            }
            return Ok(TypeRef::Tuple { elems, span });
        }
        let (name, span) = self.identifier("a type name")?;
        if !self.at(&Kind::LGeneric) {
            return Ok(TypeRef::Named {
                name,
                args: Vec::new(),
                span,
            });
        }
        self.pos += 1;
        let args = self.type_args()?;
        let close = self.expect(&Kind::RGeneric, "`\\]`")?.span;
        Ok(TypeRef::Named {
            name,
            args,
            span: Span::new(span.start, close.end),
        })
    }

    /// The inside of a `[\ ... \]`, with the opening bracket already consumed.
    fn type_args(&mut self) -> Parsed<Vec<TypeRef>> {
        let mut args = Vec::new();
        self.skip_newlines();
        loop {
            args.push(self.type_ref()?);
            self.skip_newlines();
            if !self.at(&Kind::Comma) {
                break;
            }
            self.pos += 1;
            self.skip_newlines();
        }
        Ok(args)
    }

    /// A declaration's static parameter list. Type parameters only: the other
    /// six kinds are reserved words, and each is refused by name rather than
    /// falling out as "expected a static parameter name".
    fn static_params(&mut self) -> Parsed<Vec<StaticParam>> {
        if !self.at(&Kind::LGeneric) {
            return Ok(Vec::new());
        }
        self.pos += 1;
        self.skip_newlines();
        let mut out = Vec::new();
        loop {
            out.push(self.static_param()?);
            self.skip_newlines();
            if !self.at(&Kind::Comma) {
                break;
            }
            self.pos += 1;
            self.skip_newlines();
        }
        self.expect(&Kind::RGeneric, "`\\]`")?;
        Ok(out)
    }

    fn static_param(&mut self) -> Parsed<StaticParam> {
        if let Some(Kind::Reserved(word)) = self.peek_kind() {
            if matches!(*word, "nat" | "int" | "bool" | "unit" | "dim" | "opr") {
                return Err(ParseError::StaticParameterKindUnsupported {
                    span: self.span_here(),
                    kind: (*word).to_owned(),
                });
            }
        }
        let (name, span) = self.identifier("a static parameter name")?;
        let bounds = self.extends_clause()?;
        Ok(StaticParam { name, bounds, span })
    }

    // --------------------------------------------------------- expressions

    fn expr(&mut self) -> Parsed<Expr> {
        self.disjunction()
    }

    /// `a OR b`, left associative, and below every conjunction --
    /// `appendices/operators.tex:840-851`.
    fn disjunction(&mut self) -> Parsed<Expr> {
        let mut lhs = self.conjunction()?;
        while self.at_word_op("OR") {
            self.pos += 1;
            self.skip_newlines();
            let rhs = self.conjunction()?;
            lhs = infix(BinOp::Or, Fixity::Loose, lhs, rhs);
        }
        Ok(lhs)
    }

    /// `a AND b`, left associative, and below every relational operator --
    /// which is what puts `comparison` underneath it and makes
    /// `a = 3 AND b = 8` mean `(a = 3) AND (b = 8)`.
    fn conjunction(&mut self) -> Parsed<Expr> {
        let mut lhs = self.comparison()?;
        while self.at_word_op("AND") {
            self.pos += 1;
            self.skip_newlines();
            let rhs = self.comparison()?;
            lhs = infix(BinOp::And, Fixity::Loose, lhs, rhs);
        }
        Ok(lhs)
    }

    /// A word operator is an all-capitals identifier the parser reads as an
    /// operator rather than as a name.
    ///
    /// Its shape is NEVER read from `fixity_at`. A word operator cannot be
    /// glued on its left -- the lexer would have merged the letters into one
    /// identifier -- so `a AND (b)` reads as `Prefix` and the operator would be
    /// left unconsumed, turning a correct program into a parse error.
    fn at_word_op(&self, word: &str) -> bool {
        matches!(self.peek_kind(), Some(Kind::Ident(name)) if *name == word)
    }

    /// An infix word operator, which the juxtaposition run must stop at. `NOT`
    /// is deliberately not one: it is prefix, it DOES start an operand, and
    /// `unary` consumes it there.
    fn word_operator_here(&self) -> bool {
        self.at_word_op("AND") || self.at_word_op("OR")
    }

    /// Comparison operators chain. One operator is left exactly as it was: no
    /// block, no temporaries, and nothing about existing generated code moves.
    fn comparison(&mut self) -> Parsed<Expr> {
        let first = self.additive()?;
        let mut operands = vec![first];
        let mut ops: Vec<(BinOp, Fixity, Span)> = Vec::new();
        let mut sense: Option<(Sense, BinOp)> = None;

        while let Some(op) = self.peek_kind().and_then(comparison_op) {
            let index = self.pos;
            let Some(fixity) = self.infix_fixity(index)? else {
                break;
            };
            let op_span = self.span_here();
            if let Some(this) = chain_sense(op) {
                match sense {
                    Some((seen, earlier)) if seen != this => {
                        return Err(ParseError::ChainedOperatorsDiffer {
                            span: op_span,
                            first: op_text(earlier),
                            second: op_text(op),
                        });
                    }
                    Some(_) => {}
                    None => sense = Some((this, op)),
                }
            }
            self.pos += 1;
            self.skip_newlines(); // a newline may follow an infix operator
            operands.push(self.additive()?);
            ops.push((op, fixity, op_span));
        }

        if ops.is_empty() {
            return operands.pop().ok_or(missing_operand());
        }
        if ops.len() == 1 {
            let (op, fixity, _) = ops.first().copied().ok_or_else(missing_operand)?;
            let mut both = operands.into_iter();
            let lhs = both.next().ok_or_else(missing_operand)?;
            let rhs = both.next().ok_or_else(missing_operand)?;
            return Ok(infix(op, fixity, lhs, rhs));
        }
        self.desugar_chain(&operands, &ops)
    }

    /// `a < b < c` becomes a block of one binding per operand and a nested
    /// `if`. The bindings are what the specification's "evaluated only once"
    /// requires, and the nested `if` is the conjunction.
    ///
    /// M3k gave the subset a real `AND`, and this still does not use it: the
    /// nested `if` IS what `AND` desugars to, so routing the chain through it
    /// would add a node and change nothing.
    fn desugar_chain(&mut self, operands: &[Expr], ops: &[(BinOp, Fixity, Span)]) -> Parsed<Expr> {
        let start = operands.first().map_or(0, |e| e.span().start);
        let end = operands.last().map_or(0, |e| e.span().end);
        let span = Span::new(start, end);

        let mut items = Vec::with_capacity(operands.len() + 1);
        let mut refs: Vec<Expr> = Vec::with_capacity(operands.len());
        for operand in operands {
            // A literal is a constant, so binding it would buy nothing and cost
            // the bidirectional typing `infix` does for a bare literal operand,
            // which is what decides ZZ32 against ZZ64. Only what can actually be
            // evaluated is bound.
            if is_literal(operand) {
                refs.push(operand.clone());
                continue;
            }
            let name = format!("$chain{}", self.chain_temps);
            self.chain_temps += 1;
            let operand_span = operand.span();
            items.push(BlockItem::Binding(Binding {
                name: name.clone(),
                ty: None,
                value: operand.clone(),
                mutable: false,
                span: operand_span,
            }));
            // Reading the temporary, not the operand, is the whole of
            // evaluate-once. Nothing else in this function depends on it.
            refs.push(Expr::Var {
                name,
                span: operand_span,
            });
        }

        let link = |index: usize| -> Parsed<Expr> {
            let (op, fixity, _) = ops.get(index).copied().ok_or_else(missing_operand)?;
            let lhs = refs.get(index).cloned().ok_or_else(missing_operand)?;
            let rhs = refs.get(index + 1).cloned().ok_or_else(missing_operand)?;
            Ok(infix(op, fixity, lhs, rhs))
        };

        let mut tail = link(ops.len().saturating_sub(1))?;
        for index in (0..ops.len().saturating_sub(1)).rev() {
            tail = Expr::If {
                cond: Box::new(link(index)?),
                then_branch: Box::new(tail),
                else_branch: Some(Box::new(Expr::BoolLit { value: false, span })),
                span,
            };
        }
        items.push(BlockItem::Expr(tail));
        Ok(Expr::Block { items, span })
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
            // `count+= 1` reads as a tight infix `+` whose right operand is
            // `=`, which is where "expected an expression, found Eq" came
            // from. The compound form is one operator and belongs to the
            // statement above, so the climb stops here and leaves it.
            if self.compound_op_at(index).is_some() {
                break;
            }
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
        // A word operator is an identifier to the lexer, so without this the
        // juxtaposition run swallows `AND` and the layer above never sees it.
        // `NOT` is left in: it does start an operand, and `a NOT b` then fails
        // as the multiplication of `a` by a Boolean rather than as a name that
        // does not exist.
        if self.word_operator_here() {
            return false;
        }
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
            //
            // `x += 1` reads as Prefix by that rule -- spaced left, glued
            // right -- so without the compound test the run consumes the `+`
            // and then asks for an operand and finds `=`. That is why the
            // spaced form failed while `count+= 1` worked: the glued one reads
            // as a tight infix and never reaches here.
            Some(Kind::Minus | Kind::Plus) => {
                self.compound_op_at(self.pos).is_none()
                    && matches!(self.fixity_at(self.pos), OperatorShape::Prefix)
            }
            _ => false,
        }
    }

    /// `for i <- generator do body end`.
    ///
    /// `<-` is NOT a token and does not need to be: it is `Lt` glued to
    /// `Minus`, decided by span adjacency exactly as `->` already is in
    /// `type_ref`. Adding a token would change how every file in the corpus
    /// lexes, for nothing.
    fn for_expr(&mut self) -> Parsed<Expr> {
        let start = self.span_here();
        self.pos += 1; // `for`
        self.skip_newlines();
        let (binder, _) = self.identifier("a loop variable")?;
        self.skip_newlines();
        if !self.at_left_arrow() {
            return Err(self.error("`<-` after the loop variable"));
        }
        self.pos += 2;
        self.skip_newlines();

        // `seq(...)` is recognised HERE rather than as a call, because it is
        // what decides whether the loop is parallel and the checker must not
        // have to guess that back from an application node.
        let sequential = self.at_word_op("seq") && matches!(self.peek_ahead(1), Some(Kind::LParen));
        if sequential {
            self.pos += 2;
            self.skip_newlines();
        }
        let lo = self.expr()?;
        let (hi, inclusive) = match self.peek_kind() {
            Some(Kind::Colon) => {
                self.pos += 1;
                self.skip_newlines();
                (self.expr()?, true)
            }
            Some(Kind::Hash) => {
                self.pos += 1;
                self.skip_newlines();
                (self.expr()?, false)
            }
            _ => return Err(self.error("`:` or `#` to close the generator range")),
        };
        if sequential {
            self.expect(&Kind::RParen, "`)` to close `seq(`")?;
        }
        self.skip_newlines();
        self.expect(&Kind::KwDo, "`do`")?;
        let body = self.block_body(&[Kind::KwEnd])?;
        let end = self.expect(&Kind::KwEnd, "`end`")?.span;
        Ok(Expr::For {
            binder,
            lo: Box::new(lo),
            hi: Box::new(hi),
            inclusive,
            sequential,
            body: Box::new(body),
            span: Span::new(start.start, end.end),
        })
    }

    /// `<-`, as two glued tokens.
    fn at_left_arrow(&self) -> bool {
        self.at(&Kind::Lt)
            && self.glued_right(self.pos)
            && matches!(self.peek_ahead(1), Some(Kind::Minus))
    }

    fn unary(&mut self) -> Parsed<Expr> {
        // `NOT` is a prefix operator, and 1.0 puts prefix operators above every
        // infix operator, so `NOT a AND b` is `(NOT a) AND b`.
        if self.at_word_op("NOT") {
            let span = self.span_here();
            self.pos += 1;
            self.skip_newlines();
            let operand = self.unary()?;
            let full = Span::new(span.start, operand.span().end);
            return Ok(Expr::Prefix {
                op: UnOp::Not,
                operand: Box::new(operand),
                span: full,
            });
        }
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
            // `f[\ZZ64\]`, glued, in expression position. A spaced `[\` cannot
            // start anything, so gluing is what distinguishes this from noise.
            if self.at(&Kind::LGeneric) && self.glued_left(self.pos) {
                let start = expr.span().start;
                self.pos += 1;
                let args = self.type_args()?;
                let close = self.expect(&Kind::RGeneric, "`\\]`")?.span;
                expr = Expr::Instantiate {
                    callee: Box::new(expr),
                    args,
                    span: Span::new(start, close.end),
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
            // `a^b`. Superscripting sits in the same group as subscripting --
            // above tight juxtaposition, above everything -- and the group is
            // LEFT associative, so `2^3^4` is `(2^3)^4`. That is why the
            // exponent is a `primary` and not a `postfix`: parsing it at this
            // level would consume the next `^` and make it right associative.
            if self.at(&Kind::Caret) {
                self.pos += 1;
                self.skip_newlines();
                let exponent = self.primary()?;
                expr = infix(BinOp::Pow, Fixity::Tight, expr, exponent);
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
                if self.at(&Kind::RParen) {
                    let close = self.expect(&Kind::RParen, "`)`")?.span;
                    return Ok(Expr::Unit {
                        span: Span::new(span.start, close.end),
                    });
                }
                let inner = self.expr()?;
                self.skip_newlines();
                if self.at(&Kind::Comma) {
                    let mut items = vec![inner];
                    while self.at(&Kind::Comma) {
                        self.pos += 1;
                        self.skip_newlines();
                        items.push(self.expr()?);
                        self.skip_newlines();
                    }
                    let close = self.expect(&Kind::RParen, "`)`")?.span;
                    return Ok(Expr::Tuple {
                        items,
                        span: Span::new(span.start, close.end),
                    });
                }
                self.expect(&Kind::RParen, "`)`")?;
                Ok(inner)
            }
            Kind::KwIf => self.if_expr(),
            Kind::KwDo => self.block(),
            Kind::KwWhile => self.while_expr(),
            Kind::LBracket => self.array_literal(),
            // `for` is one of the 66 reserved words the lexer keeps out of the
            // identifier namespace. Intercepting it here rather than giving it
            // a keyword token is the same trade `<-` takes: no lexer change,
            // so no file in the corpus lexes differently.
            Kind::Reserved("for") => self.for_expr(),
            // `atomic` is one of the 66 words the lexer keeps out of the
            // identifier namespace, so it is intercepted here rather than
            // given a keyword token -- again, no lexer change.
            Kind::Reserved("atomic") => self.atomic_expr(),
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
        // `atomic <statement>`. The specification writes both `atomic do ...
        // end` and a bare `atomic sum += a[i]`, and an assignment is not an
        // expression here, so the modifier is read at statement level and
        // whatever it covers becomes a one-item block. `atomic do ... end`
        // takes the same path and its block is already the body.
        if matches!(self.peek_kind(), Some(Kind::Reserved("atomic"))) {
            let start = self.span_here();
            self.pos += 1;
            self.skip_newlines();
            let inner = self.block_item()?;
            let end = self.previous_span();
            let span = Span::new(start.start, end.end);
            let body = match inner {
                BlockItem::Expr(e) => e,
                other => Expr::Block {
                    items: vec![other],
                    span,
                },
            };
            return Ok(BlockItem::Expr(Expr::Atomic {
                body: Box::new(body),
                span,
            }));
        }
        let save = self.pos;
        if let Some(binding) = self.try_binding()? {
            return Ok(BlockItem::Binding(binding));
        }
        self.pos = save;
        // `f(x) = e`: a local function declaration, not a discarded equality.
        // Guarded on tokens rather than on the parsed tree, because a body that
        // is itself an equality (`isZero(x) = x = 0`) collects into a chain and
        // desugars into a block before any tree match could see it.
        if matches!(self.peek_kind(), Some(Kind::Ident(_)))
            && matches!(self.peek_ahead(1), Some(Kind::LParen))
            && self.glued_left(self.pos + 1)
        {
            let probe = self.pos;
            if let Ok(Expr::Call { callee, span, .. }) = self.postfix() {
                if matches!(*callee, Expr::Var { .. }) && self.at(&Kind::Eq) {
                    return Err(ParseError::LocalFunctionDeclarationUnsupported { span });
                }
            }
            self.pos = probe;
        }
        let target = self.expr()?;
        let op = if let Some(op) = self.compound_op_at(self.pos) {
            self.pos += 2;
            Some(op)
        } else if self.at(&Kind::ColonEq) {
            self.pos += 1;
            None
        } else {
            return Ok(BlockItem::Expr(target));
        };
        self.skip_newlines();
        let value = self.expr()?;
        let span = Span::new(target.span().start, value.span().end);
        Ok(BlockItem::Assign(Assign {
            target,
            op,
            value,
            span,
        }))
    }

    /// `atomic <expr>`, for the operand positions `block_item` never reaches.
    fn atomic_expr(&mut self) -> Parsed<Expr> {
        let start = self.span_here();
        self.pos += 1;
        self.skip_newlines();
        let body = self.expr()?;
        let span = Span::new(start.start, body.span().end);
        Ok(Expr::Atomic {
            body: Box::new(body),
            span,
        })
    }

    fn try_binding(&mut self) -> Parsed<Option<Binding>> {
        let save = self.pos;
        // 1.0 spells a mutable local two ways and the corpus uses both:
        // `var count: ZZ32 = 0` with the modifier and an ordinary `=`, and
        // `count: ZZ32 := 0` with the operator. The modifier is what makes the
        // `=` form unambiguous, so it does not need the type annotation the
        // bare `:=` form does.
        let modifier = self.at(&Kind::KwVar);
        if modifier {
            self.pos += 1;
        }
        let Some(Token {
            kind: Kind::Ident(name),
            span: name_span,
        }) = self.peek()
        else {
            self.pos = save;
            return Ok(None);
        };
        let name = (*name).to_owned();
        let name_span = *name_span;
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
            Some(Kind::Eq) => modifier,
            Some(Kind::ColonEq) if ty.is_some() => true,
            _ => {
                self.pos = save;
                return Ok(None);
            }
        };
        self.pos += 1;
        self.skip_newlines(); // `w` after the `=` does permit one

        let value = self.expr()?;
        let start = self
            .tokens
            .get(save)
            .map_or(name_span.start, |t| t.span.start);
        let span = Span::new(start, value.span().end);
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

const fn is_literal(e: &Expr) -> bool {
    matches!(
        e,
        Expr::IntLit { .. } | Expr::FloatLit { .. } | Expr::StrLit { .. } | Expr::BoolLit { .. }
    )
}

/// Unreachable in practice: `comparison` only reaches these after collecting at
/// least one operand. Written as a diagnostic rather than an index, because a
/// parser that panics on its own bookkeeping is worse than one that reports.
const fn missing_operand() -> ParseError {
    ParseError::UnexpectedEndOfInput {
        expected: "an operand",
    }
}

/// A chain's ordering sense. Equivalence operators carry none and mix freely;
/// two ordering operators must agree. `chained-multifix.tex:16-34`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sense {
    Increasing,
    Decreasing,
}

const fn chain_sense(op: BinOp) -> Option<Sense> {
    match op {
        BinOp::Lt | BinOp::Le => Some(Sense::Increasing),
        BinOp::Gt | BinOp::Ge => Some(Sense::Decreasing),
        _ => None,
    }
}

const fn op_text(op: BinOp) -> &'static str {
    match op {
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::Eq => "=",
        BinOp::Ne => "=/=",
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Pow => "^",
        BinOp::And => "AND",
        BinOp::Or => "OR",
    }
}

/// `=` is here because every definition site consumes its own `=` first:
/// `member` takes a field's or a function's through `optional_definition`, and
/// `try_binding` takes a binding's. An `=` that reaches this point is equality.
/// The one shape that slips through, `f(x) = e` in block position, is refused
/// by `block_item`.
const fn comparison_op(kind: &Kind<'_>) -> Option<BinOp> {
    match kind {
        Kind::Eq => Some(BinOp::Eq),
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
