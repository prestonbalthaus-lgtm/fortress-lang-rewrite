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
    Accessor, Assign, BinOp, Binding, BlockItem, CaseArm, Component, Decl, DimDecl, DimExpr, Expr,
    ExtentForm, ExtentRange, FieldDecl, Fixity, FnDecl, GeneratorClause, ImportDecl, ImportItems,
    ImportedName, Member, MethodDecl, Modifiers, ObjectDecl, Param, ShapeSpelling, Span,
    StaticExpr, StaticKind, StaticOp, StaticParam, TraitDecl, TypeCaseArm, TypeRef, UnOp, UnitDecl,
    SELF_TYPE_PLACEHOLDER,
};
use fortress_lexer::{Kind, Token};

type Parsed<T> = Result<T, ParseError>;

/// The BIG reduction operators this lowering recognises at all. `MAX` and
/// `MIN` are here so that they are refused BY NAME rather than read as a
/// subscript, which is what they are today.
/// The type a lambda parameter carries when the source wrote none. It cannot
/// be lexed, so no declared type can collide with it, and closure lowering is
/// the only thing that ever resolves one.
pub const INFER: &str = "$infer";

const BIG_OPERATORS: [&str; 4] = ["SUM", "PROD", "MAX", "MIN"];

/// One `i <- lo:hi` clause. `for` and a BIG reduction share the parser for it.
struct Generator {
    binder: String,
    /// The range's lower bound, or -- when `hi` is `None` -- the whole source.
    lo: Expr,
    /// `None` for `x <- a`, where the source is a value and not a range.
    hi: Option<Expr>,
    inclusive: bool,
    sequential: bool,
}

/// Which of the operators this milestone adds built the node a precedence level
/// is returning, if any. `None` means the node came out of the arithmetic and
/// relational ladder, or out of a parenthesis, and may be combined freely.
type Mark<'a> = Option<(&'a str, Span)>;

/// A parenthesised type is the type itself, but its span covers the
/// parentheses, so a diagnostic points at what was written.
fn widen(t: TypeRef, span: Span) -> TypeRef {
    match t {
        TypeRef::Named { name, args, .. } => TypeRef::Named { name, args, span },
        TypeRef::Unit { .. } => TypeRef::Unit { span },
        TypeRef::Tuple { elems, .. } => TypeRef::Tuple { elems, span },
        TypeRef::Arrow { from, to, .. } => TypeRef::Arrow { from, to, span },
        // A static VALUE argument cannot be parenthesised into this function:
        // `widen` is only ever called on a parsed TYPE.
        TypeRef::Static { expr, .. } => TypeRef::Static { expr, span },
        TypeRef::Shaped {
            base,
            spelling,
            extents,
            ..
        } => TypeRef::Shaped {
            base,
            spelling,
            extents,
            span,
        },
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
fn operator_text<'a>(kind: &Kind<'a>) -> Option<&'a str> {
    Some(match kind {
        // A run of three or more vertical lines is ONE base operator
        // (`lexical-structure.tex:1174-1177`) and has no fixed length, so its
        // text is the source slice rather than a literal.
        Kind::BarRun(text) => text,
        // An operator WORD is a base operator like any other
        // (`lexical-structure.tex:1173-1176`), so `opr CMP` reaches the same
        // run the symbolic names do and needs no branch of its own.
        Kind::OpWord(text) => text,
        // An allowlisted Unicode operator character carries its own text, so
        // the name of `opr \u{2229}` is that character and nothing has to know
        // which one it is.
        Kind::UniOp(text) => text,
        Kind::Bang => "!",
        Kind::Question => "?",
        Kind::Tilde => "~",
        Kind::Dollar => "$",
        Kind::Percent => "%",
        Kind::At => "@",
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

/// The closing half of a one-character enclosing operator, for the positions
/// that must recognise a bracket PAIR written with a space in it -- an import
/// list is the only one. `|` and `||` mirror themselves.
const fn mirrored(open: &str) -> Option<&'static str> {
    Some(match open.as_bytes() {
        b"{" => "}",
        b"[" => "]",
        b"<|" => "|>",
        b"|" => "|",
        b"||" => "||",
        _ => return None,
    })
}

fn join(run: &[&str]) -> String {
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

#[derive(Default)]
struct Topology {
    extends: Vec<TypeRef>,
    comprises: Vec<TypeRef>,
    comprises_open: bool,
    excludes: Vec<TypeRef>,
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
    /// True when `kind` is the next token that is not a newline.
    ///
    /// This is what the grammar's `w` means in front of an OPTIONAL clause --
    /// `FnHeaderClause = (w NoNewlineIsType)? FnClauses`, `FnClause = w Where`,
    /// `Id (w StaticParams)? w ValParam`. The newlines may only be consumed if
    /// the clause is really there: if it is not, they are the statement
    /// separator and eating them merges two declarations into one.
    fn at_across_newlines(&self, kind: &Kind<'_>) -> bool {
        let mut index = self.pos;
        while matches!(self.tokens.get(index).map(|t| &t.kind), Some(Kind::Newline)) {
            index += 1;
        }
        self.tokens.get(index).map(|t| &t.kind) == Some(kind)
    }

    /// `w` in front of an optional clause: skip the newlines only when the
    /// clause follows them.
    fn skip_newlines_before(&mut self, kind: &Kind<'_>) -> bool {
        if !self.at_across_newlines(kind) {
            return false;
        }
        self.skip_newlines();
        true
    }

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

    /// `equals = "=" (!op)` (`Symbol.rats:201`): the `=` that INTRODUCES A
    /// DEFINITION is one not glued to an operator character. `Symbol.rats` has
    /// a second production for the same character -- `equalsOp`, the equality
    /// operator -- which carries no such restriction, and the reference grammar
    /// reaches `equals` only from `Function.rats:33`, `Method.rats:44`,
    /// `Variable.rats:40`, `LocalDecl.rats:159` and `Parameter.rats:93`: every
    /// one a binding or a keyword argument.
    ///
    /// It used to live in the LEXER, where it applied to every `=` in the file.
    /// `Library/QuickCheck.fsi`'s `opr ==>` and `Library/RangeInternals.fss:453`'s
    /// `ex=-1` -- an EQUALITY, inside the body of `opr =` -- were hard lex
    /// errors for it.
    ///
    /// BRACKETS ARE NOT OPERATORS HERE. `Symbol.rats:175-177` excludes
    /// `encloser`, `leftEncloser` and `rightEncloser` from `singleOp`, so
    /// `x =[1,2]` stays a definition; the set below is the one the lexer guard
    /// already used, moved rather than widened.
    fn definition_equals_at(&self, index: usize) -> bool {
        if !matches!(self.tokens.get(index).map(|t| &t.kind), Some(Kind::Eq)) {
            return false;
        }
        if !self.glued_right(index) {
            return true;
        }
        !matches!(
            self.tokens.get(index + 1).map(|t| &t.kind),
            Some(
                Kind::Plus
                    | Kind::Minus
                    | Kind::Star
                    | Kind::Slash
                    | Kind::SlashSlash
                    | Kind::SlashSlashSlash
                    | Kind::Lt
                    | Kind::Gt
                    | Kind::Le
                    | Kind::Ge
                    | Kind::Eq
                    | Kind::EqEqEq
                    | Kind::NotEq
                    | Kind::FatArrow
                    | Kind::Colon
                    | Kind::ColonEq
                    | Kind::Bang
                    | Kind::Question
                    | Kind::Tilde
                    | Kind::Dollar
                    | Kind::Percent
                    | Kind::At
            )
        )
    }

    /// `f(w: ZZ32) = e` and `go(n: ZZ32): R = e` -- a local function
    /// declaration whose parameters carry written types. Returns its span if
    /// this is one, having consumed it; the caller restores `self.pos` either
    /// way, which is all a speculative parse here has to undo because `params`
    /// builds a local `Vec` and touches no shared state.
    ///
    /// PARSED IN ORDER TO BE REFUSED BY NAME, exactly as the untyped form
    /// already is. It gains NOTHING that compiles -- the construct is refused
    /// on both paths -- and that is the point: it moves those files into the
    /// local-function bucket so the milestone can be PRICED rather than
    /// guessed. This project's own rule: a first-blocker count becomes a
    /// CEILING the moment the construct parses.
    ///
    /// `params` IS WHAT KEEPS THIS FROM EATING A CALL. It requires every
    /// parameter to be `name: Type`, so a call whose argument list is an
    /// expression -- `f(1:10)`, `assert(("A":"B":"C").toString, s)` -- fails
    /// inside it and the caller falls through unchanged.
    fn typed_local_function_here(&mut self) -> Option<Span> {
        let start = self.span_here();
        if !matches!(self.peek_kind(), Some(Kind::Ident(_))) {
            return None;
        }
        self.pos += 1;
        if !self.at(&Kind::LParen) {
            return None;
        }
        self.pos += 1;
        self.params(false).ok()?;
        if !self.at(&Kind::RParen) {
            return None;
        }
        self.pos += 1;
        // An optional written result type: `go(n: ZZ32, h: Heap[\K,V\]): R =`.
        if self.at(&Kind::Colon) {
            self.pos += 1;
            self.type_ref().ok()?;
        }
        if !self.definition_equals_at(self.pos) {
            return None;
        }
        Some(Span::new(start.start, self.span_here().end))
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

    /// `opr-fixity.tex:34-55`, all twelve rows.
    ///
    /// THIS IS NOT `fixity_at`. That one is `match (glued_left, glued_right)`
    /// and its own doc comment says "from adjacency alone", which cannot decide
    /// `|` in `a |b| c` -- the same `|`, the same spacing, and an encloser
    /// rather than an infix operator. The specification decides fixity from
    /// LEFT CONTEXT and RIGHT CONTEXT, with whitespace as a secondary
    /// discriminator on only some rows.
    ///
    /// Only the operators this milestone ADDS are routed through it. Moving
    /// `+ - * / < > =` off `fixity_at` would change how programs that compile
    /// today are grouped, and that is a measurement and a commit of its own.
    fn table_fixity_at(&self, index: usize) -> TableFixity {
        let left = self.left_context(index);
        let right = self.right_context(index);
        let space_left = !self.glued_left(index);
        let space_right = !self.glued_right(index);
        match (left, right) {
            (LeftContext::PrimaryTail, RightContext::PrimaryFront | RightContext::Operator) => {
                match (space_left, space_right) {
                    (true, true) | (false, false) => TableFixity::Infix,
                    // Lopsided: whitespace on one side and not the other.
                    (true, false) => TableFixity::Lopsided,
                    (false, true) => TableFixity::Postfix,
                }
            }
            (LeftContext::PrimaryTail, RightContext::Delimiter) => {
                if space_left {
                    TableFixity::Lopsided
                } else {
                    TableFixity::Postfix
                }
            }
            (LeftContext::PrimaryTail, RightContext::LineBreak) => {
                if space_left {
                    TableFixity::Infix
                } else {
                    TableFixity::Postfix
                }
            }
            (
                LeftContext::Operator | LeftContext::Delimiter,
                RightContext::PrimaryFront | RightContext::Operator,
            ) => TableFixity::Prefix,
            (LeftContext::Delimiter, RightContext::Delimiter) => TableFixity::Nofix,
            (LeftContext::Operator, _) | (LeftContext::Delimiter, RightContext::LineBreak) => {
                TableFixity::Nofix
            }
        }
    }

    /// A PRIMARY TAIL is "an identifier, a literal, a right encloser, or a
    /// superscripted postfix operator". Nothing else on the left is a primary,
    /// and a newline or the start of the file is neither -- both behave as a
    /// left encloser, which is what makes a leading `-` prefix.
    fn left_context(&self, index: usize) -> LeftContext {
        let Some(prev) = index.checked_sub(1).and_then(|i| self.tokens.get(i)) else {
            return LeftContext::Delimiter;
        };
        match &prev.kind {
            Kind::Ident(_)
            | Kind::KwSelf
            | Kind::IntLit { .. }
            | Kind::FloatLit { .. }
            | Kind::StrLit(_)
            | Kind::CharLit(_)
            | Kind::True
            | Kind::False
            | Kind::RParen
            | Kind::RBracket
            | Kind::RBrace
            | Kind::RGeneric
            | Kind::RightBar
            | Kind::KwEnd => LeftContext::PrimaryTail,
            Kind::Newline
            | Kind::Eof
            | Kind::Comma
            | Kind::Semi
            | Kind::LParen
            | Kind::LBracket
            | Kind::LBrace
            | Kind::LGeneric
            | Kind::LeftBar => LeftContext::Delimiter,
            _ => LeftContext::Operator,
        }
    }

    /// A PRIMARY FRONT is "an identifier, a literal, or a left encloser". The
    /// keywords that open a delimited expression -- `if`, `do`, `while`, `for`,
    /// `atomic` -- are primaries too, and leaving them out would read a prefix
    /// operator before one as nofix.
    fn right_context(&self, index: usize) -> RightContext {
        let Some(next) = self.tokens.get(index + 1) else {
            return RightContext::LineBreak;
        };
        match &next.kind {
            Kind::Ident(_)
            | Kind::KwSelf
            | Kind::IntLit { .. }
            | Kind::FloatLit { .. }
            | Kind::StrLit(_)
            | Kind::CharLit(_)
            | Kind::True
            | Kind::False
            | Kind::LParen
            | Kind::LBracket
            | Kind::LBrace
            | Kind::LGeneric
            | Kind::LeftBar
            | Kind::KwIf
            | Kind::KwDo
            | Kind::KwWhile
            | Kind::Reserved("for" | "atomic" | "spawn") => RightContext::PrimaryFront,
            Kind::Newline | Kind::Eof => RightContext::LineBreak,
            Kind::Comma
            | Kind::Semi
            | Kind::RParen
            | Kind::RBracket
            | Kind::RBrace
            | Kind::RGeneric
            | Kind::RightBar => RightContext::Delimiter,
            _ => RightContext::Operator,
        }
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
                // `Compilation.rats` gives the export the same APIName the
                // component header takes -- which is why `component Compiled5.a`
                // parsed and `export Compiled5.a` did not: the header read a
                // dotted name and the export, fourteen lines later, read an
                // identifier. `export {A, B}` is the set form.
                if self.at(&Kind::LBrace) {
                    exports.extend(self.name_set()?);
                } else {
                    exports.push(self.dotted_name()?);
                }
            } else if self.at(&Kind::KwImport) {
                imports.push(self.import_decl()?);
            } else {
                break;
            }
            self.expect_separator()?;
        }

        let mut decls = Vec::new();
        let mut dims = Vec::new();
        let mut units = Vec::new();
        while !self.at(&Kind::KwEnd) && !self.at_eof() {
            // A dimension declares no value and has no members, so it is taken
            // out of the declaration stream here rather than made a `Decl`
            // variant that thirty passes would have to answer "nothing" for.
            // `defining-dimensions.tex:33-36` puts it at top level only, which
            // is exactly what being parsed here and nowhere else means.
            if self.at_reserved("dim") {
                let (dim, unit) = self.dim_decl()?;
                dims.push(dim);
                units.extend(unit);
            } else if self.at_reserved("unit") || self.at_reserved("SI_unit") {
                units.push(self.unit_decl()?);
            } else {
                decls.push(self.decl(is_api)?);
            }
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
        // Everything after the closing `end` used to be SILENTLY DISCARDED,
        // including a whole second component: two complete components in one
        // file compiled at exit 0 and the second one was gone. Only UNLEXABLE
        // trailing text was caught, and the lexer caught that, not this.
        // `Compilation.rats` has one compilation unit per file.
        self.skip_newlines();
        if !self.at_eof() {
            // Two different mistakes reach here and the diagnostic names which:
            // a spare `end` is an unmatched delimiter, which is exactly what the
            // legacy implementation called it in XXX0e/XXX0u/XXX1c.test; anything
            // else is a second compilation unit in a file that has room for one.
            return Err(self.error(if self.at(&Kind::KwEnd) {
                "end of file; this `end` closes nothing"
            } else {
                "end of file after the component's `end`"
            }));
        }
        Ok(Component {
            name,
            exports,
            imports,
            decls,
            bounds: Vec::new(),
            cuts: Vec::new(),
            is_api,
            dims,
            units,
            span: Span::new(start.start, end.end),
        })
    }

    /// `OtherDecl.rats:29-33`, both `dim` productions. The first bundles a unit
    /// declaration into the same line -- `dim Length SI_unit meter meters m_` --
    /// and the reference implementation returns TWO nodes for it, which is why
    /// this returns a pair.
    fn dim_decl(&mut self) -> Parsed<(DimDecl, Option<UnitDecl>)> {
        let start = self.span_here().start;
        self.pos += 1;
        let (name, name_span) = self.identifier("a dimension name")?;
        let derivation = if self.at(&Kind::Eq) {
            self.pos += 1;
            // `Fortress.SIUnits.fss:31-32` breaks a definition across a line:
            // `SI_unit newton newtons N_ =` then the product on the next. The
            // reference grammar's `w` around the `=` allows it, and the run
            // still ENDS at a newline because `at_dim_atom` does not admit one.
            self.skip_newlines();
            Some(self.dim_expr()?)
        } else {
            None
        };
        // `dim Mass default kilogram; SI_unit gram grams g_: Mass` -- the
        // semicolon is an ordinary separator and the second half is a unit
        // declaration in its own right, so only the unseparated form bundles.
        if self.at_reserved("default") {
            self.pos += 1;
            let (unit, _) = self.identifier("a unit name")?;
            return Ok((
                DimDecl {
                    name,
                    derivation,
                    default_unit: Some(unit),
                    span: Span::new(start, self.previous_span().end),
                },
                None,
            ));
        }
        let bundled = if self.at_reserved("unit") || self.at_reserved("SI_unit") {
            Some(self.unit_decl_named_for(&name, name_span)?)
        } else {
            None
        };
        Ok((
            DimDecl {
                name,
                derivation,
                default_unit: None,
                span: Span::new(start, self.previous_span().end),
            },
            bundled,
        ))
    }

    fn unit_decl(&mut self) -> Parsed<UnitDecl> {
        let start = self.span_here().start;
        let si = self.at_reserved("SI_unit");
        self.pos += 1;
        let mut names = vec![self.identifier("a unit name")?.0];
        while matches!(self.peek_kind(), Some(Kind::Ident(_))) {
            names.push(self.identifier("a unit name")?.0);
        }
        let dimension = if self.at(&Kind::Colon) {
            self.pos += 1;
            Some(self.identifier("a dimension name")?.0)
        } else {
            None
        };
        let definition = if self.at(&Kind::Eq) {
            self.pos += 1;
            self.skip_newlines();
            Some(self.dim_expr()?)
        } else {
            None
        };
        Ok(UnitDecl {
            names,
            si,
            dimension,
            definition,
            span: Span::new(start, self.previous_span().end),
        })
    }

    /// The unit half of the bundled form, whose dimension is the `dim` it was
    /// written inside rather than a `: Dim` of its own.
    fn unit_decl_named_for(&mut self, dimension: &str, span: Span) -> Parsed<UnitDecl> {
        let mut unit = self.unit_decl()?;
        if unit.dimension.is_none() {
            unit.dimension = Some(dimension.to_owned());
        }
        let _ = span;
        Ok(unit)
    }

    /// `dimensions.tex:34-55`. ONE grammar for a `dim` right-hand side and a
    /// `unit` right-hand side; which namespace a name has to be in is the
    /// checker's question, not this one's.
    ///
    /// Loosest to tightest: quotient, product by juxtaposition, power, atom.
    fn dim_expr(&mut self) -> Parsed<DimExpr> {
        let mut left = self.dim_product()?;
        loop {
            // `dimensions.tex:40-43` makes `/` and `per` the same operator.
            let divides = self.at(&Kind::Slash) || self.at_reserved("per");
            if !divides {
                return Ok(left);
            }
            self.pos += 1;
            let right = self.dim_product()?;
            let span = Span::new(left.span().start, right.span().end);
            left = DimExpr::Quotient {
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }
    }

    fn dim_product(&mut self) -> Parsed<DimExpr> {
        let first = self.dim_power()?;
        let mut factors = vec![first];
        while self.at_dim_atom() {
            factors.push(self.dim_power()?);
        }
        if factors.len() == 1 {
            return Ok(factors.remove(0));
        }
        let span = Span::new(
            factors.first().map_or(0, |f| f.span().start),
            self.previous_span().end,
        );
        Ok(DimExpr::Product { factors, span })
    }

    fn dim_power(&mut self) -> Parsed<DimExpr> {
        let base = self.dim_atom()?;
        if !self.at(&Kind::Caret) {
            return Ok(base);
        }
        self.pos += 1;
        let negative = if self.at(&Kind::Minus) {
            self.pos += 1;
            true
        } else {
            false
        };
        let Some(Kind::IntLit { digits, .. }) = self.peek_kind() else {
            return Err(self.error("an integer dimension exponent"));
        };
        let magnitude: i64 = digits
            .parse()
            .map_err(|_| self.error("an integer dimension exponent"))?;
        self.pos += 1;
        let span = Span::new(base.span().start, self.previous_span().end);
        Ok(DimExpr::Power {
            base: Box::new(base),
            exponent: if negative { -magnitude } else { magnitude },
            span,
        })
    }

    /// The three sugar words `dimensions.tex:49-54` gives a prefix each are
    /// rewritten here, so nothing downstream has to know they exist.
    fn dim_atom(&mut self) -> Parsed<DimExpr> {
        let start = self.span_here().start;
        for (word, exponent) in [("square", 2), ("cubic", 3), ("inverse", -1)] {
            if self.at_reserved(word) {
                self.pos += 1;
                let base = self.dim_atom()?;
                let span = Span::new(start, self.previous_span().end);
                return Ok(DimExpr::Power {
                    base: Box::new(base),
                    exponent,
                    span,
                });
            }
        }
        let mut atom = if self.at(&Kind::LParen) {
            self.pos += 1;
            self.skip_newlines();
            let inner = self.dim_expr()?;
            self.expect(&Kind::RParen, "`)`")?;
            inner
        } else if let Some(Kind::IntLit { text, .. } | Kind::FloatLit { text, .. }) =
            self.peek_kind()
        {
            let written = (*text).to_owned();
            let span = self.span_here();
            self.pos += 1;
            DimExpr::Number { written, span }
        } else {
            let (name, span) = self.identifier("a dimension or unit name")?;
            DimExpr::Name { name, span }
        };
        for (word, exponent) in [("squared", 2), ("cubed", 3)] {
            if self.at_reserved(word) {
                self.pos += 1;
                let span = Span::new(start, self.previous_span().end);
                atom = DimExpr::Power {
                    base: Box::new(atom),
                    exponent,
                    span,
                };
            }
        }
        Ok(atom)
    }

    /// Whether another factor of a juxtaposition product starts here. A
    /// dimension expression ends at a newline, so the run is bounded by the
    /// same separator every other declaration uses.
    fn at_dim_atom(&self) -> bool {
        matches!(
            self.peek_kind(),
            Some(Kind::Ident(_) | Kind::IntLit { .. } | Kind::FloatLit { .. } | Kind::LParen)
        ) || self.at_reserved("square")
            || self.at_reserved("cubic")
            || self.at_reserved("inverse")
    }

    /// `import Foo.Bar.{...}`, `import api Foo`, `import Foo.{X as Y} except {Z}`.
    /// The dotted name is parsed for real; the brace group and the `except`
    /// clause are consumed as balanced token runs. Aliasing an operator needs a
    /// precedence map, and recording a name we cannot resolve yet would be
    /// pretending.
    fn import_decl(&mut self) -> Parsed<ImportDecl> {
        let start = self.expect(&Kind::KwImport, "`import`")?.span;
        // `import java com.sun.fortress.nativeHelpers.{...}` reaches a Fortress
        // body through the JVM. 39 corpus files write it and three of them are
        // bootstrap files whose bodies have NO other implementation in this
        // tree -- those three are C-shim work, not import work. What phase 3
        // owes it is a diagnostic that NAMES the construct instead of
        // `expected a newline or `;`, found Ident("com")`.
        if matches!(self.peek_kind(), Some(Kind::Ident("java"))) {
            return Err(ParseError::ForeignImportUnsupported {
                span: self.span_here(),
            });
        }
        let is_api = self.at(&Kind::KwApi);
        if is_api {
            self.pos += 1;
        }
        // `import api {A, B}` names a set with no leading dotted name.
        let (api_name, trailing) = if self.at(&Kind::LBrace) {
            (String::new(), None)
        } else {
            self.dotted_import_name()?
        };
        let mut end = self.span_here();
        // `import Foo.{...}` and `import api Foo` are IMPORT-ON-DEMAND;
        // `import Foo.{a, b as c}` and the single-member `import Foo.a` name
        // what they take.
        // `import api {File, FileSupport}` names APIS, not members of one, so
        // `is_api` is what a resolver reads to know which the list holds.
        let mut items = if is_api {
            ImportItems::OnDemand
        } else {
            trailing.map_or(ImportItems::OnDemand, |name| {
                ImportItems::Named(vec![ImportedName { name, alias: None }])
            })
        };
        if self.at(&Kind::LBrace) {
            let (names, close) = self.import_items()?;
            items = names;
            end = close;
        }
        let mut except = Vec::new();
        if self.at(&Kind::KwExcept) {
            self.pos += 1;
            self.skip_newlines();
            if self.at(&Kind::LBrace) {
                except = self.name_set()?;
                end = self.previous_span();
            } else {
                let (name, span) = self.identifier("a name after `except`")?;
                except.push(name);
                end = span;
            }
        }
        Ok(ImportDecl {
            api_name,
            is_api,
            items,
            except,
            span: Span::new(start.start, end.end),
        })
    }

    /// `Foo`, `Foo.Bar`, or `Foo.member`. The api name and a trailing single
    /// member are the same dotted run, and only the file system can say where
    /// one ends -- `import FlatString.FlatString` is the api `FlatString` and
    /// the name `FlatString` in it, while `import Compiled5.a.{...}` is a
    /// dotted api. So BOTH readings are carried: the whole run as the api name,
    /// and the last segment as a candidate member, and the resolver picks.
    fn dotted_import_name(&mut self) -> Parsed<(String, Option<String>)> {
        let name = self.dotted_name()?;
        if self.at(&Kind::LBrace) {
            return Ok((name, None));
        }
        match name.rsplit_once('.') {
            Some((head, last)) => Ok((head.to_owned(), Some(last.to_owned()))),
            None => Ok((name, None)),
        }
    }

    /// `{ ... }`, `{ a, b }`, `{ a as b }`. Three `Dot`s are the open-set marker
    /// `intro.tex:38-63` calls an import-on-demand; there is no `...` token, so
    /// it is the same glued run a varargs parameter uses.
    fn import_items(&mut self) -> Parsed<(ImportItems, Span)> {
        self.expect(&Kind::LBrace, "`{`")?;
        self.skip_newlines();
        if self.at_ellipsis() {
            self.pos += 3;
            self.skip_newlines();
            let close = self.expect(&Kind::RBrace, "`}`")?.span;
            return Ok((ImportItems::OnDemand, close));
        }
        let mut names = Vec::new();
        if !self.at(&Kind::RBrace) {
            loop {
                let name = self.import_name()?;
                names.push(name);
                self.skip_newlines();
                if !self.at(&Kind::Comma) {
                    break;
                }
                self.pos += 1;
                self.skip_newlines();
            }
        }
        let close = self.expect(&Kind::RBrace, "`}`")?.span;
        Ok((ImportItems::Named(names), close))
    }

    /// One imported name. An OPERATOR may be imported and aliased
    /// (`opr OPLUS => MYPLUS`), so the name is read as an operator run when it
    /// is not an identifier.
    fn import_name(&mut self) -> Parsed<ImportedName> {
        let name = self.imported_identifier()?;
        let alias = if self.at(&Kind::FatArrow) || self.at_word("as") {
            self.pos += 1;
            self.skip_newlines();
            Some(self.imported_identifier()?)
        } else {
            None
        };
        Ok(ImportedName { name, alias })
    }

    fn imported_identifier(&mut self) -> Parsed<String> {
        let mut name = String::new();
        if self.at(&Kind::Reserved("opr")) {
            self.pos += 1;
            self.skip_newlines();
            // `import Map.{...} except { opr BIG UNION, ... }` --
            // `Library/PrefixSet.fsi:35`. `BIG` is a modifier on the name and
            // not the name, exactly as `opr_signature` reads it.
            if self.at(&Kind::Reserved("BIG")) {
                self.pos += 1;
                name.push_str("BIG ");
            }
        }
        if matches!(self.peek_kind(), Some(Kind::Ident(_))) {
            name.push_str(&self.dotted_name()?);
            return Ok(name);
        }
        // `import Set.{ opr { } }` -- `simpleNameTest.fsi:15`. An ENCLOSING
        // operator is named by both halves, and in an import list they are
        // written with a SPACE between, so the run stops at the first.
        //
        // What says a second half follows is that the next token is the OPENER'S
        // OWN MIRROR, and nothing weaker will do. `opr BIG SYMDIFF }` ends an
        // except set and `opr <| => ||}` ends an alias list, so "an operator
        // character follows" reads the list's own `}` as half of a name.
        let mirror = self.peek_kind().and_then(operator_text).and_then(mirrored);
        let open = self.import_operator_run();
        if open.is_empty() {
            return Err(self.error("an imported name"));
        }
        name.push_str(&join(&open));
        if mirror.is_some() && self.peek_kind().and_then(operator_text) == mirror {
            let Some(close) = self.peek_kind().and_then(operator_text) else {
                return Ok(name);
            };
            self.pos += 1;
            name.push('_');
            name.push_str(close);
        }
        Ok(name)
    }

    /// An operator name inside an import list. It is `operator_run` with three
    /// tokens held back: `,` and `}` end the ITEM and the LIST, and `=>`
    /// introduces the alias. `import List.{opr <| => ||}` glues `||` to the
    /// closing brace, so a greedy run reads the list's own `}` into the name.
    fn import_operator_run(&mut self) -> Vec<&'a str> {
        let mut run = Vec::new();
        loop {
            if matches!(
                self.peek_kind(),
                Some(Kind::Comma | Kind::RBrace | Kind::FatArrow)
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

    fn at_word(&self, word: &str) -> bool {
        matches!(self.peek_kind(), Some(Kind::Ident(name)) if *name == word)
    }

    /// `{A, B}` where every element is a plain name -- an export set, or the
    /// set after `except`.
    fn name_set(&mut self) -> Parsed<Vec<String>> {
        self.expect(&Kind::LBrace, "`{`")?;
        self.skip_newlines();
        let mut names = Vec::new();
        if !self.at(&Kind::RBrace) {
            loop {
                names.push(self.imported_identifier()?);
                self.skip_newlines();
                if !self.at(&Kind::Comma) {
                    break;
                }
                self.pos += 1;
                self.skip_newlines();
            }
        }
        self.expect(&Kind::RBrace, "`}`")?;
        Ok(names)
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
            // `var maxLeafSize: ZZ32`. `Variable.rats:42-45` makes `var` an
            // AbsVarMod and NOTHING ELSE: it is not an FnMod anywhere in the
            // grammar, so it stays out of `modifiers()` -- folding it in there
            // would admit `var f(x: ZZ32) = x`, which no production writes.
            // This is the same shape `member` already uses at declaration
            // level for a field.
            Some(Kind::KwVar) => Ok(Decl::Value(self.value_decl(modifiers, true)?)),
            Some(Kind::Ident(_))
                if matches!(
                    self.peek_ahead(1),
                    Some(Kind::Colon | Kind::Eq | Kind::ColonEq)
                ) =>
            {
                Ok(Decl::Value(self.value_decl(modifiers, false)?))
            }
            _ => Ok(Decl::Function(self.fn_decl(modifiers, signature_only)?)),
        }
    }

    /// A top-level value declaration: `pi: RR64 = 3.14`, `x := 0`, the
    /// initializer-less `stdIn: Reader` an api writes, and -- with `keyword` --
    /// the `var` forms of all three.
    ///
    /// `keyword` is the `var` MODIFIER and not the same fact as `mutable`:
    /// `variables.tex:88-93` makes `var id: T := e` and `id: T := e` the same
    /// declaration, so the modifier is one of two spellings of one property.
    /// It is carried separately only until the name is read, because it is
    /// what decides where the span starts and whether a missing type is the
    /// grammar's own error production.
    fn value_decl(
        &mut self,
        modifiers: Modifiers,
        keyword: bool,
    ) -> Parsed<fortress_ast::ValueDecl> {
        let start = self.span_here();
        if keyword {
            self.pos += 1;
            // `VarWTypes` admits `(x: T, y: U)` and `BindIdOrBindIdTuple`
            // admits `(x, y)`. Refused HERE rather than at `identifier`,
            // which would report a missing name for a list that is written.
            //
            // NAMED rather than written inline so the mutation table has a
            // BAR-FREE, UNIQUE line to target: `if self.at(&Kind::LParen) {`
            // appears twice in this file.
            let parenthesised_list = self.at(&Kind::LParen);
            if parenthesised_list {
                return Err(ParseError::VariableListUnsupported {
                    span: self.span_here(),
                });
            }
        }
        let (name, name_span) = self.identifier("a value name")?;
        let ty = if self.at(&Kind::Colon) {
            self.pos += 1;
            self.skip_newlines();
            Some(self.type_ref()?)
        } else {
            None
        };
        // `:=` needs no type annotation here, unlike in a block: component
        // level has no assignment statements, so there is nothing for a bare
        // `x := 0` to be confused with.
        let save = self.pos;
        self.skip_newlines();
        // `:=` IS CARRIED AND NOT DROPPED. The parse-only spike this replaces
        // threw it away, and three corpus files write a mutable value at
        // component level -- `Compiled5.k.fss:15` is `x := 0`. Dropping the
        // flag makes those silently immutable.
        let mutable = keyword || self.at(&Kind::ColonEq);
        let init = if self.definition_equals_at(self.pos) || self.at(&Kind::ColonEq) {
            self.pos += 1;
            self.skip_newlines();
            Some(self.expr()?)
        } else {
            self.pos = save;
            None
        };
        let end = init
            .as_ref()
            .map_or_else(|| ty.as_ref().map_or(name_span, TypeRef::span), Expr::span);
        let head = if keyword {
            start.start
        } else {
            name_span.start
        };
        Ok(fortress_ast::ValueDecl {
            modifiers,
            name,
            ty,
            init,
            mutable,
            span: Span::new(head, end.end),
        })
    }

    // ------------------------------------------------------ traits and objects

    /// `comprises` and `excludes` are recorded and never read: exclusion is
    /// decided from the concrete types the program actually declares, which a
    /// whole-program compiler can see and a modular one cannot.
    fn trait_decl(&mut self, modifiers: Modifiers) -> Parsed<TraitDecl> {
        let start = self.expect(&Kind::KwTrait, "`trait`")?.span;
        let (name, _) = self.identifier("a trait name")?;
        self.skip_newlines_before(&Kind::LGeneric);
        let mut static_params = self.static_params()?;
        let topology = self.topology_clauses()?;
        self.where_clause(&mut static_params)?;
        let members = self.members()?;
        let end = self.named_end(&Kind::KwTrait, &name)?;
        Ok(TraitDecl {
            merged: false,
            modifiers,
            name,
            static_params,
            extends: topology.extends,
            comprises: topology.comprises,
            comprises_open: topology.comprises_open,
            excludes: topology.excludes,
            members,
            span: Span::new(start.start, end.end),
        })
    }

    /// No parameter list at all is a singleton: one instance, constructed once
    /// before `run`. `object O() ... end` is a constructor taking nothing.
    fn object_decl(&mut self, modifiers: Modifiers) -> Parsed<ObjectDecl> {
        let start = self.expect(&Kind::KwObject, "`object`")?.span;
        let (name, _) = self.identifier("an object name")?;
        self.skip_newlines_before(&Kind::LGeneric);
        let mut static_params = self.static_params()?;
        // A WRAPPED VALUE-PARAMETER LIST. The list may begin on the line AFTER
        // the header, which is what `Library/GeneratorLibrary.fsi:131`,
        // `Random.fsi:211` and `Sparse.fsi:28` write once the static parameters
        // have made the first line long. Newlines are significant here, so this
        // is a deliberate exception and not an accident -- the same one
        // `skip_newlines_before(&Kind::LGeneric)` above already makes for the
        // static parameter list.
        //
        // IT CANNOT SWALLOW A MEMBER. `at_across_newlines` looks past newlines
        // ONLY, so a body whose first declaration begins with anything else --
        // `extends`, a name, `opr`, `getter` -- is untouched; and a member
        // cannot begin with `(` today, which is precisely the error this
        // removes ("expected a field or method name, found LParen").
        self.skip_newlines_before(&Kind::LParen);
        let params = if self.at(&Kind::LParen) {
            self.pos += 1;
            let params = self.params(true)?;
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
        let topology = self.topology_clauses()?;
        self.where_clause(&mut static_params)?;
        let members = self.members()?;
        let end = self.named_end(&Kind::KwObject, &name)?;
        Ok(ObjectDecl {
            merged: false,
            modifiers,
            name,
            static_params,
            params,
            extends: topology.extends,
            comprises: topology.comprises,
            comprises_open: topology.comprises_open,
            excludes: topology.excludes,
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

    /// One declaration's three topology clauses, plus whether the `comprises`
    /// one carried the open marker. A four-tuple would read at the call sites
    /// as three lists and a mystery bool.
    ///
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
    fn topology_clauses(&mut self) -> Parsed<Topology> {
        let mut found = Topology::default();
        let (mut had_ext, mut had_comp, mut had_excl) = (false, false, false);
        loop {
            let save = self.pos;
            self.skip_newlines();
            // A clause written twice is the error, and an EMPTY clause is legal
            // -- `comprises { ... }` names nothing at all -- so the check
            // cannot ask whether the list is already non-empty.
            let (slot, seen, is_comprises) = match self.peek_kind() {
                Some(Kind::KwExtends) => (&mut found.extends, &mut had_ext, false),
                Some(Kind::KwComprises) => (&mut found.comprises, &mut had_comp, true),
                Some(Kind::KwExcludes) => (&mut found.excludes, &mut had_excl, false),
                _ => {
                    self.pos = save;
                    return Ok(found);
                }
            };
            if *seen {
                return Err(self.error("one `extends`, `comprises` or `excludes` clause each"));
            }
            *seen = true;
            self.pos += 1;
            let (types, open) = self.type_set()?;
            *slot = types;
            // The open marker is only meaningful on `comprises`; no corpus file
            // writes one on either of the other two, and reading it there would
            // invent a rule the specification does not state.
            if is_comprises {
                found.comprises_open = open;
            }
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
        Ok(self.type_set()?.0)
    }

    /// The type list a topology clause names: one type, or a braced set.
    fn type_set(&mut self) -> Parsed<(Vec<TypeRef>, bool)> {
        self.skip_newlines();
        if !self.at(&Kind::LBrace) {
            return Ok((vec![self.type_ref()?], false));
        }
        self.pos += 1;
        self.skip_newlines();
        let mut out = Vec::new();
        if self.at(&Kind::RBrace) {
            self.pos += 1;
            return Ok((out, false));
        }
        // `comprises { ... }` says the set is OPEN rather than naming it, and
        // `comprises { O, ... }` names part of it. There is no `...` token; it
        // is three `Dot`s. THE MARKER IS RECORDED: it used to be dropped, and
        // only the LEADING form was even accepted, so `{ O, ... }` reached
        // `type_ref` on a `Dot` and the api it was written in did not parse.
        let mut open = false;
        loop {
            if self.at(&Kind::Dot) {
                while self.at(&Kind::Dot) {
                    self.pos += 1;
                }
                open = true;
                self.skip_newlines();
                break;
            }
            out.push(self.type_ref()?);
            self.skip_newlines();
            if !self.at(&Kind::Comma) {
                break;
            }
            self.pos += 1;
            self.skip_newlines();
        }
        self.expect(&Kind::RBrace, "`}`")?;
        Ok((out, open))
    }

    /// `where {T extends U}`. Consumed and discarded: there are no static
    /// parameters to constrain until generics land.
    /// A `where` clause, PARSED. It used to be a token skip: brace-matched and
    /// thrown away, so `where { this is total garbage }` compiled and ran, and a
    /// bound written here was a silent no-op while the identical bound written
    /// in the bracket list was enforced.
    ///
    /// v1 accepts ONE of the spec's thirteen forms -- `Id extends Type`,
    /// `concrete-syntax.tex:513` -- and it needs no machinery of its own: the
    /// constraint is appended to the named static parameter's bounds, so
    /// `record_bounds` and `discharge_bounds` enforce it exactly as they
    /// enforce a bracket-list bound. Every other form is refused BY NAME.
    ///
    /// Zero corpus risk, and that is measured rather than hoped: 17 of the 1956
    /// files carry a real `where` token, all 17 exit 1 on the compiler as it
    /// stands, and 10 of them die earlier on `nat`/`int`/`opr` static
    /// parameters that M3d locks out. The payoff here is the silent acceptance,
    /// not a file count.
    /// `FnClause = w Where`, so the clause may sit on the line BELOW its header,
    /// which is how the corpus writes all eight of the files that put one on a
    /// continuation line. The diagnostic before that was `expected a field or
    /// method name, found KwWhere`, naming a mechanism a `where` clause is not.
    fn where_clause(&mut self, static_params: &mut [StaticParam]) -> Parsed<()> {
        if !self.at(&Kind::KwWhere) && !self.skip_newlines_before(&Kind::KwWhere) {
            return Ok(());
        }
        self.pos += 1;
        self.skip_newlines();
        if self.at(&Kind::LGeneric) {
            // D6 section 1 cuts where-VARIABLES from v1, so the binder form is
            // refused whole and BEFORE anything inside it is read. The frontend
            // lane's rule that a locked static-parameter kind may not slip
            // through a `where` is kept by this being a refusal rather than a
            // skip -- `where [\nat n\]` is refused either way -- and naming
            // the kind here would name the inner reason for an outer cut.
            return Err(ParseError::WhereClauseFormUnsupported {
                span: self.span_here(),
                form: "`where [\\ ... \\]` introduces fresh static variables, which are bound \
                       semantically rather than written"
                    .to_owned(),
            });
        }
        self.expect(&Kind::LBrace, "`{`")?;
        self.skip_newlines();
        if self.at(&Kind::RBrace) {
            self.pos += 1;
            return Ok(());
        }
        loop {
            self.where_constraint(static_params)?;
            self.skip_newlines();
            if !self.at(&Kind::Comma) {
                break;
            }
            self.pos += 1;
            self.skip_newlines();
        }
        self.expect(&Kind::RBrace, "`}`")?;
        Ok(())
    }

    /// One constraint. `X extends T` lands on `X`'s bounds; anything else is
    /// named and refused, because the alternative -- consuming it silently --
    /// is the defect this function exists to close.
    fn where_constraint(&mut self, static_params: &mut [StaticParam]) -> Parsed<()> {
        if let Some(Kind::Reserved(word)) = self.peek_kind() {
            return Err(ParseError::WhereClauseFormUnsupported {
                span: self.span_here(),
                form: format!("`{word}` is a form v1 does not implement"),
            });
        }
        let (name, span) = self.identifier("a static parameter name")?;
        if !self.at(&Kind::KwExtends) {
            return Err(ParseError::WhereClauseFormUnsupported {
                span: self.span_here(),
                form: format!(
                    "`{name}` is not followed by `extends`, and `extends` is the \
                               only constraint v1 implements"
                ),
            });
        }
        let bounds = self.extends_clause()?;
        let Some(param) = static_params.iter_mut().find(|p| p.name == name) else {
            return Err(ParseError::WhereClauseFormUnsupported {
                span,
                form: format!(
                    "`{name}` is not one of them. A member's where clause cannot yet \
                     constrain its owner's static parameters"
                ),
            });
        };
        param.bounds.extend(bounds);
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
        let accessor = if self.at(&Kind::KwGetter) {
            Some(Accessor::Getter)
        } else if self.at(&Kind::KwSetter) {
            Some(Accessor::Setter)
        } else {
            None
        };
        if accessor.is_some() {
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
        // `coerce(x: T)`. Parsed and RECORDED, never read -- see
        // `Member::Coercion`. Only its shape is consumed here; nothing may
        // depend on it until coercion has semantics.
        if accessor.is_none() && !mutable && self.at(&Kind::Reserved("coerce")) {
            let start = self.span_here();
            self.pos += 1;
            // THE PARAMETER TYPES ARE KEPT and everything after them is not.
            // The types are an edge in the trait hierarchy and the cycle check
            // reads them; the `widens` modifier and any body are consumed and
            // dropped, because nothing reads those.
            let mut from = Vec::new();
            if self.at(&Kind::LParen) {
                self.pos += 1;
                for p in self.params(false)? {
                    from.push(p.ty);
                }
                if self.at(&Kind::RParen) {
                    self.pos += 1;
                }
            }
            while !self.at(&Kind::Newline) && !self.at_eof() && !self.at(&Kind::KwEnd) {
                self.pos += 1;
            }
            let end = self.previous_span();
            return Ok(Member::Coercion {
                from,
                span: Span::new(start.start, end.end),
            });
        }
        if accessor.is_none() && !mutable && self.at(&Kind::Reserved("opr")) {
            return Ok(Member::Method(self.opr_member(modifiers)?));
        }
        let (name, name_span) = self.identifier("a field or method name")?;

        self.skip_newlines_before(&Kind::LGeneric);
        let mut static_params = self.static_params()?;
        if self.at(&Kind::LParen) || self.skip_newlines_before(&Kind::LParen) {
            if mutable {
                return Err(self.error("a field name after `var`"));
            }
            self.pos += 1;
            let params = self.params(false)?;
            let rparen = self.expect(&Kind::RParen, "`)`")?.span;
            let return_type = self.optional_return_type()?;
            self.where_clause(&mut static_params)?;
            // `getter get(): E throws NotFound` -- `FortressLibrary.fsi:772`.
            // A member takes the same `FnClause*` a top-level declaration does,
            // and this one was reading only `where`.
            self.skip_throws()?;
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
        self.skip_newlines();
        let ty = self.type_ref()?;
        // `InitVal = ("=" / ":=") w NoNewlineExpr` (`Variable.rats:37`), so a
        // FIELD takes either spelling where a method body takes only `=`. The
        // `:=` spelling also DECLARES the field mutable, the same rule
        // `value_decl` already applies at component level.
        let assigned = self.at_field_initializer();
        let mut is_a_mutable_field = mutable;
        let init = if assigned {
            is_a_mutable_field = true;
            self.pos += 1;
            self.skip_newlines();
            Some(self.expr()?)
        } else {
            self.optional_definition()?
        };
        let end = init.as_ref().map_or_else(|| ty.span(), Expr::span);
        Ok(Member::Field(FieldDecl {
            name,
            ty,
            init,
            mutable: is_a_mutable_field,
            span: Span::new(name_span.start, end.end),
        }))
    }

    /// `: T`. `FnHeaderClause = (w NoNewlineIsType)?` and
    /// `NoNewlineIsType = colon w NoNewlineType`, so a newline is permitted on
    /// BOTH sides of the colon -- `Library/Set.fsi:63` writes the return type
    /// on the line below the parameter list. `NoNewlineType` bounds what is
    /// inside the type, not what precedes it.
    ///
    /// A FIELD is a different production: `NoNewlineVarWType = BindId s
    /// NoNewlineIsType` uses `s`, so a field's colon must stay on its name's
    /// line. That asymmetry is why this is not the same helper `member` uses
    /// for a field.
    fn optional_return_type(&mut self) -> Parsed<Option<TypeRef>> {
        if !self.at(&Kind::Colon) && !self.skip_newlines_before(&Kind::Colon) {
            return Ok(None);
        }
        self.pos += 1;
        self.skip_newlines();
        Ok(Some(self.type_ref()?))
    }

    /// `= e`, where the `=` may sit on the following line. Restores the
    /// position when there is none, so the separator the caller needs survives.
    /// `:=` where a field's initializer goes. A NEWLINE MAY NOT PRECEDE IT:
    /// `NoNewlineVarWTypes` binds the type to the name, and the next line of an
    /// object body may itself open with an assignment statement.
    fn at_field_initializer(&self) -> bool {
        self.at(&Kind::ColonEq)
    }

    fn optional_definition(&mut self) -> Parsed<Option<Expr>> {
        let save = self.pos;
        self.skip_newlines();
        if !self.definition_equals_at(self.pos) {
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

    /// A name in a position where `Self` is one. 1.0 reserves the word and then
    /// spells it back in exactly two places -- `Type.rats:203`, a `TypeRef`,
    /// and `NoNewlineHeader.rats:343`, a static PARAMETER -- and both feed the
    /// same node an ordinary `Id` does (`makeVarType`, `makeStaticParamId` at
    /// `KindType`). So `Self` is a TYPE VARIABLE, never a self-type: six traits
    /// in `CompilerLibrary/FortressLibrary.fsi` write `[\Self extends
    /// Equality[\Self\]\]` where `Library/`'s copy writes `T`.
    ///
    /// EVERY OTHER POSITION STILL REFUSES IT BY NAME, which is why this is not
    /// simply dropping `Self` from `RESERVED`: `Self = 5` and `object Self` are
    /// errors in 1.0 and stay errors here.
    fn type_name(&mut self, expected: &'static str) -> Parsed<(String, Span)> {
        if let Some(Token {
            kind: Kind::Reserved("Self"),
            span,
        }) = self.peek()
        {
            let span = *span;
            self.pos += 1;
            return Ok(("Self".to_owned(), span));
        }
        self.identifier(expected)
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
        // `NamedFnHeaderFront = Id (w StaticParams)? w ValParam`. Both `w`s are
        // may-newline, and the whole corpus writes long headers across lines --
        // `Library/RangeInternals.fsi:576` breaks before the static parameters,
        // `Library/FortressLibrary.fsi:305` (`opr juxtaposition`) breaks before
        // the parameter list.
        self.skip_newlines_before(&Kind::LGeneric);
        let mut static_params = self.static_params()?;
        self.skip_newlines();
        self.expect(&Kind::LParen, "`(`")?;
        let params = self.params(false)?;
        let rparen = self.expect(&Kind::RParen, "`)`")?.span;

        let return_type = self.optional_return_type()?;
        self.where_clause(&mut static_params)?;
        // `FnHeaderClause = (w NoNewlineIsType)? FnClauses` and `FnClause`
        // covers `throws` as well as `where`. This branch read only `where`, so
        // a top-level `f(): T throws E` never parsed at all -- only the `opr`
        // form did, which is where the clause was first met.
        self.skip_throws()?;

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
            accessor: None,
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
            params = self.params(false)?;
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
            self.params(false)?
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
            // SUBSCRIPT ASSIGNMENT: `opr[i:I]:=(v:E): ()`. The subscript GET
            // stops at the closing bracket and reads a return type; the SET
            // continues with `:=` and a second parameter list, which is the
            // value being stored. `Library/FortressLibrary.fsi:1237` is the
            // declaration the bootstrap root died on, and 32 sites over 14
            // files write this form.
            //
            // THE NAME CARRIES THE `:=`. `[_]` and `[_]:=` are two different
            // members of the same object -- `a[i]` reads and `a[i] := v`
            // writes -- so giving them one name would collide them in the
            // method tables and make an object that declares both refuse as a
            // duplicate.
            let mut subscript_assign = false;
            if self.at(&Kind::ColonEq) {
                subscript_assign = true;
                self.pos += 1;
                name.push_str(":=");
                self.skip_newlines();
                let open_paren =
                    self.expect(&Kind::LParen, "`(` for the value of a subscript assignment")?;
                let value = self.params(false)?;
                let close_paren = self.expect(&Kind::RParen, "`)`")?;
                // subscripting.tex:47-49 -- the second list "must contain
                // exactly one non-keyword value parameter". It is the value
                // being stored and there is only ever one of those.
                if value.len() != 1 {
                    return Err(ParseError::SubscriptedAssignmentValueArity {
                        span: Span::new(open_paren.span.start, close_paren.span.end),
                        found: value.len(),
                    });
                }
                params.extend(value);
            }
            let mut end = self.previous_span();
            let return_type = self.optional_return_type()?;
            // subscripting.tex:53-54 -- "A result type may appear after the
            // second value parameter list, but it must be `()`." A setter
            // returns nothing, and the legacy records this refusal by name in
            // XXX5az.test. Written OUT rather than silently coerced, because a
            // declaration that says it returns a `ZZ32` and does not is the
            // silent-wrong-answer class.
            if subscript_assign {
                if let Some(ty) = return_type.as_ref() {
                    if !matches!(ty, TypeRef::Unit { .. }) {
                        return Err(ParseError::SubscriptedAssignmentReturnType {
                            span: ty.span(),
                            written: ty.written(),
                        });
                    }
                }
            }
            if let Some(ty) = return_type.as_ref() {
                end = ty.span();
            }
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
        mut static_params: Vec<StaticParam>,
    ) -> Parsed<OprSignature> {
        self.skip_newlines();
        // A POSTFIX DECLARATION HAS NO TRAILING PARAMETER LIST, because its
        // leading operand is its only one: `opr (x:I)#[\I extends
        // AnyIntegral\] : LeftRange[\I\]` is `x#`, the range that starts at
        // `x`. `Library/FortressLibrary.fsi:2171` is the declaration the
        // bootstrap root died on after subscript assignment landed.
        //
        // THE GUARD IS THE LEADING OPERAND AND NOT THE MISSING `(`. An INFIX
        // declaration written with a leading operand always has the trailing
        // list too, so requiring `params` to be non-empty is what stops a
        // malformed infix from being silently re-read as a postfix rather than
        // reported. With no leading operand this still demands the `(` and
        // says so.
        //
        // This is the DECLARATION only. A postfix operator in EXPRESSION
        // position is still refused by name -- `OperatorShape::Postfix` --
        // because that needs the operator table, which is a different piece of
        // work; what this buys is that the library's own api can be READ.
        let mut end;
        if params.is_empty() || self.at(&Kind::LParen) {
            self.expect(&Kind::LParen, "`(`")?;
            params.extend(self.params(false)?);
            end = self.expect(&Kind::RParen, "`)`")?.span;
        } else {
            end = self.previous_span();
        }
        let return_type = self.optional_return_type()?;
        if let Some(ty) = return_type.as_ref() {
            end = ty.span();
        }
        self.where_clause(&mut static_params)?;
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
    fn closing_operator_run(&mut self) -> Vec<&'a str> {
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
    fn operator_run(&mut self, limit: usize) -> Vec<&'a str> {
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
        // `FnClause = w Throws`, the same continuation rule as `where`.
        if !self.at(&Kind::Reserved("throws"))
            && !self.skip_newlines_before(&Kind::Reserved("throws"))
        {
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

    fn params(&mut self, mutable_allowed: bool) -> Parsed<Vec<Param>> {
        let mut params = Vec::new();
        self.skip_newlines();
        if self.at(&Kind::RParen) {
            return Ok(params);
        }
        loop {
            // `self` is a parameter with no written type: it stands for the
            // enclosing trait or object, which is what makes the declaration a
            // functional method rather than a function. The placeholder is
            // UNWRITABLE (`SELF_TYPE_PLACEHOLDER`) rather than the bare name
            // `Self`, because `Self` is an ordinary type name in 1.0 and a
            // static parameter may be called it.
            if self.at(&Kind::KwSelf) {
                let span = self.span_here();
                self.pos += 1;
                params.push(Param {
                    mutable: false,
                    name: "self".to_owned(),
                    ty: TypeRef::Named {
                        name: SELF_TYPE_PLACEHOLDER.to_owned(),
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
            // `var` here is a FIELD modifier, not a parameter one: an
            // object's value parameters ARE its fields, so `object O(var v: T)`
            // declares a mutable one. `Variable.rats:48-52` makes `var` an
            // AbsVarMod and nothing else, which is why a FUNCTION's parameter
            // list still refuses it at `identifier` -- a parameter is not
            // storage and there would be nothing for the modifier to say.
            let mutable = mutable_allowed && self.at(&Kind::KwVar);
            if mutable {
                self.pos += 1;
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
                mutable,
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
        // `->`, two tokens joined by adjacency, or U+2192, one token.
        let width = if self.at(&Kind::RightArrow) {
            1
        } else if self.at(&Kind::Minus)
            && self.glued_right(self.pos)
            && matches!(self.peek_ahead(1), Some(Kind::Gt))
        {
            2
        } else {
            return Ok(from);
        };
        let start = from.span().start;
        self.pos += width;
        self.skip_newlines();
        let to = self.type_ref()?;
        let end = to.span().end;
        Ok(TypeRef::Arrow {
            from: Box::new(from),
            to: Box::new(to),
            span: Span::new(start, end),
        })
    }

    /// A type atom plus at most one SHAPE SUFFIX. `traits.tex:97-101`.
    ///
    /// THE SUFFIX SITS HERE AND NOT IN `type_ref`, so `ZZ32[5] -> T` groups as
    /// `(ZZ32[5]) -> T`: a size binds tighter than an arrow, which is the only
    /// reading that makes sense of a function taking an array.
    ///
    /// `type_atom` HAS EXACTLY ONE CALLER, `type_ref`, which is why one hook
    /// here reaches every type position in the language -- parameters, return
    /// types, binding annotations, field types, `extends` clauses, static
    /// arguments, `typecase` arms and lambda signatures alike.
    fn type_atom(&mut self) -> Parsed<TypeRef> {
        let base = self.type_atom_base()?;
        self.shape_suffix(base)
    }

    /// AT MOST ONE, AND NEVER STACKED. 1.0 says so at three separate sites
    /// through `NodeUtil.isExponentiation`, and the reason is that
    /// `ZZ32[3][4]` and `ZZ32^2^3` have no meaning it defines. Returning after
    /// the first suffix rather than looping is what enforces it, and the
    /// second suffix is then whatever the caller expected next -- a `)` or a
    /// newline -- so it is reported as the caller's error and not swallowed.
    ///
    /// THE SUFFIX MUST BE GLUED. `crates/parser/tests/parser.rs` already
    /// records that a spaced bracket is a juxtaposition and not a subscript,
    /// and the same rule here keeps `x : ZZ32 [1,2,3]` reading the way it
    /// reads today. Measured cost: zero. All 62 corpus sites are glued.
    fn shape_suffix(&mut self, base: TypeRef) -> Parsed<TypeRef> {
        let start = base.span().start;
        let glued_bracket = self.at(&Kind::LBracket) && self.glued_left(self.pos);
        if glued_bracket {
            self.pos += 1;
            self.skip_newlines();
            let extents = self.extent_list(&Kind::RBracket)?;
            let end = self.expect(&Kind::RBracket, "`]`")?.span.end;
            return Ok(TypeRef::Shaped {
                base: Box::new(base),
                spelling: ShapeSpelling::Bracket,
                extents,
                span: Span::new(start, end),
            });
        }
        let glued_caret = self.at(&Kind::Caret) && self.glued_left(self.pos);
        if glued_caret {
            self.pos += 1;
            // `traits.tex:100-101`. The PARENTHESIS is the whole distinction
            // between the two caret productions, which is exactly how the
            // reference implementation reads it (`Type.rats:276-317`).
            let (extents, end) = if self.at(&Kind::LParen) {
                self.pos += 1;
                self.skip_newlines();
                let mut extents = vec![self.extent()?];
                self.skip_newlines();
                // `BY` is the ASCII cross, `Symbol.rats:232`. It is all caps
                // with two distinct letters, so the operator-word rule lexes
                // it `OpWord` and NOT `Ident` -- the same trap that silently
                // stopped the BIG reduction recogniser firing on `SUM`.
                while self.at_word_op("BY") {
                    self.pos += 1;
                    self.skip_newlines();
                    extents.push(self.extent()?);
                    self.skip_newlines();
                }
                (extents, self.expect(&Kind::RParen, "`)`")?.span.end)
            } else {
                let extent = self.extent()?;
                let end = extent.span.end;
                (vec![extent], end)
            };
            return Ok(TypeRef::Shaped {
                base: Box::new(base),
                spelling: ShapeSpelling::Caret,
                extents,
                span: Span::new(start, end),
            });
        }
        Ok(base)
    }

    /// `traits.tex:104`. One or more extents, comma separated. An empty list
    /// is ONE extent that writes no size rather than no extents at all, so
    /// `ZZ32[]` is refused as a missing size and not as a zero-dimensional
    /// array -- a diagnostic naming the wrong mechanism is the defect this
    /// project has paid for twice.
    fn extent_list(&mut self, close: &Kind<'_>) -> Parsed<Vec<ExtentRange>> {
        if self.at(close) {
            return Ok(vec![ExtentRange {
                lower: None,
                upper: None,
                form: ExtentForm::Size,
                span: self.span_here(),
            }]);
        }
        let mut extents = vec![self.extent()?];
        self.skip_newlines();
        while self.at(&Kind::Comma) {
            self.pos += 1;
            self.skip_newlines();
            extents.push(self.extent()?);
            self.skip_newlines();
        }
        Ok(extents)
    }

    /// `traits.tex:106-108`, all three spellings. `5`, `0#5`, `1:5`, and the
    /// open forms `#5`, `0#`, `:5`, `1:`.
    fn extent(&mut self) -> Parsed<ExtentRange> {
        let start = self.span_here().start;
        let leading = if self.at(&Kind::Hash) || self.at(&Kind::Colon) {
            None
        } else {
            Some(self.extent_arg()?)
        };
        let form = if self.at(&Kind::Hash) {
            ExtentForm::Hash
        } else if self.at(&Kind::Colon) {
            ExtentForm::Colon
        } else {
            // A single argument IS the size, and its lower bound is zero.
            let end = self.previous_span().end;
            return Ok(ExtentRange {
                lower: None,
                upper: leading,
                form: ExtentForm::Size,
                span: Span::new(start, end),
            });
        };
        self.pos += 1;
        let trailing = if self.at_extent_terminator() {
            None
        } else {
            Some(self.extent_arg()?)
        };
        Ok(ExtentRange {
            lower: leading,
            upper: trailing,
            form,
            span: Span::new(start, self.previous_span().end),
        })
    }

    fn at_extent_terminator(&self) -> bool {
        self.at(&Kind::Comma)
            || self.at(&Kind::RBracket)
            || self.at(&Kind::RParen)
            || self.at(&Kind::Hash)
            || self.at(&Kind::Colon)
            || self.at_word_op("BY")
    }

    /// One extent's argument. THE TYPE IS TRIED FIRST AND THE POSITION IS
    /// RESTORED IF IT DOES NOT REACH THE END, which is `static_arg`'s rule
    /// with this position's terminators: it is what lets `ZZ32[n]` keep `n` as
    /// a name for expansion to classify, while `ZZ32[2 n]` and `ZZ32[k+1]`
    /// fall through to the static-value sublanguage that already parses them.
    fn extent_arg(&mut self) -> Parsed<TypeRef> {
        let save = self.pos;
        if let Ok(t) = self.type_ref() {
            let after = self.pos;
            if self.at_extent_terminator() {
                self.pos = after;
                return Ok(t);
            }
        }
        self.pos = save;
        let start = self.span_here();
        let expr = self.static_expr()?;
        let end = self.previous_span();
        Ok(TypeRef::Static {
            expr,
            span: Span::new(start.start, end.end),
        })
    }

    fn type_atom_base(&mut self) -> Parsed<TypeRef> {
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
        let (mut name, span) = self.type_name("a type name")?;
        // A QUALIFIED type name. `source-code.tex:280-287` disambiguates "the
        // type `List` declared in the API `List` or the type `List` declared in
        // the API `PureList`" with exactly this, and with ten api names
        // duplicated across the source path the collision is not hypothetical.
        // It PARSES here and does not RESOLVE anywhere: a qualified name means
        // nothing until an import resolver exists, so it comes out as
        // `unknown type `List.List``, which is a diagnostic and not the
        // `expected `)`, found Dot` that named the wrong mechanism.
        let mut end = span;
        while self.at(&Kind::Dot) && matches!(self.peek_ahead(1), Some(Kind::Ident(_))) {
            self.pos += 1;
            let (part, part_span) = self.identifier("a type name")?;
            name.push('.');
            name.push_str(&part);
            end = part_span;
        }
        let span = Span::new(span.start, end.end);
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
            args.push(self.static_arg()?);
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

    /// THE SIX KINDS ARE THREE GROUPS NOW, AND ALL SIX STILL REFUSE.
    ///
    /// This is the hook `SPIKE-NAT` drops into and nothing more.
    /// `2026-08-21-d7-reconcile-nat.md` §3.4 is explicit that the decision comes
    /// before the parser change -- "doing (2) before (1) means the parser
    /// accepts a shape nobody has decided the meaning of, and
    /// `ChunkedSparseArray` will be the file that discovers it" -- and D7's own
    /// header says **drafted, not adopted**. So the split is diagnostics and a
    /// named place to put the code, with ZERO change to what parses.
    ///
    /// `nat` `int` `bool`   D7 §3.1 puts them IN v1, with every static ARGUMENT
    ///                      statically evaluable -- a literal, or an expression
    ///                      over the enclosing declaration's own static
    ///                      parameters. That sublanguage is sub-phase 4b and is
    ///                      NOT optional: `Library/Generator22D.fss` writes
    ///                      `[\T, 0, s0 + s2, 0, s1 + s3\]`, so "literals only"
    ///                      cannot compile the library's own array generators.
    ///                      `NatReflect.reflect`, which turns a run-time `ZZ32`
    ///                      into a static parameter, is a NAMED DEVIATION (§3.2)
    ///                      and must be refused by a diagnostic that says so --
    ///                      a monomorphizing compiler cannot stamp a
    ///                      specialisation for a value it does not know.
    ///                      Scope, measured: 8 census `.fsi` files block on
    ///                      `nat` today and 61 corpus files write it.
    /// `unit` `dim`         D7 §3.3 defers both to sub-phase 4d, gated on
    ///                      SPIKE-COMPOSITE-TYPE rather than on D7. Sized from
    ///                      the corpus and not the spec: `unit` is 6 corpus
    ///                      files and ZERO library files, `dim` is zero corpus
    ///                      files at all.
    /// `opr`                D7 §4 keeps this refusal in place when the other
    ///                      three open, and says so in the parser spike's
    ///                      scope. It is a different mechanism -- a name in
    ///                      OPERATOR position, which is SPIKE-OPEXPR territory
    ///                      and not arithmetic -- and it belongs with the
    ///                      operator-property traits, which begin by WRITING
    ///                      declarations that exist only as commented LaTeX.
    fn static_param(&mut self) -> Parsed<StaticParam> {
        let mut kind = StaticKind::Type;
        if let Some(Kind::Reserved(word)) = self.peek_kind() {
            // THE D7 GROUP IS OPEN. `nat`, `int` and `bool` are v1 with every
            // static ARGUMENT statically evaluable; this is the arm the
            // scaffold said would go when D7 was adopted, and it has.
            kind = match *word {
                "nat" => StaticKind::Nat,
                "int" => StaticKind::Int,
                "bool" => StaticKind::Bool,
                // SUB-PHASE 4d IS OPEN and these two are its kinds. They
                // PARSE and are RECORDED; instantiating one is refused by name
                // in expansion, because substituting a unit means deciding a
                // dimensioned value's representation and this backend has no
                // boxing. `opr` still waits on the operator-property traits,
                // which begin by WRITING declarations that exist only as
                // commented LaTeX -- D7 §4, unchanged.
                "unit" => StaticKind::Unit,
                "dim" => StaticKind::Dim,
                "opr" => {
                    return Err(ParseError::StaticParameterKindUnsupported {
                        span: self.span_here(),
                        kind: (*word).to_owned(),
                    })
                }
                _ => StaticKind::Type,
            };
            if kind.is_value() || kind.is_dimensional() {
                self.pos += 1;
            }
        }
        let (name, span) = self.type_name("a static parameter name")?;
        // `[\unit U absorbs unit\]`. The `unit` after `absorbs` is the
        // reserved word again, not a name.
        let mut absorbs_unit = false;
        if kind.is_dimensional() && self.at_reserved("absorbs") {
            self.pos += 1;
            if !self.at_reserved("unit") {
                return Err(self.error("`unit` after `absorbs`"));
            }
            self.pos += 1;
            absorbs_unit = true;
        }
        let bounds = self.extends_clause()?;
        // D7 leaves the constraint solver out of v1 and its own census is the
        // reason: NOT ONE `where { k < n }` exists in 1956 files. A bound on a
        // value parameter would have to be discharged by something that does
        // not exist, so it is refused rather than dropped in silence.
        if kind.is_value() && !bounds.is_empty() {
            return Err(ParseError::StaticValueParameterBound { span, name });
        }
        Ok(StaticParam {
            name,
            kind,
            absorbs_unit,
            bounds,
            span,
        })
    }

    /// One static ARGUMENT. A type, or -- D7 §3.1 -- a statically evaluable
    /// VALUE.
    ///
    /// THE TYPE IS TRIED FIRST AND THE POSITION IS RESTORED IF IT DOES NOT
    /// REACH THE END OF THE ARGUMENT, which is what makes `imax jmax kmax`
    /// work: `type_ref` happily parses `imax` and stops, and only the fact
    /// that the next token is neither `,` nor `\]` says it was a product.
    /// A BARE NAME IS LEFT AS A TYPE. `[\n\]` is `Named` whether `n` is a
    /// type or a value parameter, and expansion classifies it against the
    /// callee's declared kinds -- which is what keeps demand SYNTACTIC and
    /// lets expansion keep running before `Checker::new`.
    fn static_arg(&mut self) -> Parsed<TypeRef> {
        let save = self.pos;
        if let Ok(t) = self.type_ref() {
            let after = self.pos;
            self.skip_newlines();
            if self.at(&Kind::Comma) || self.at(&Kind::RGeneric) {
                self.pos = after;
                return Ok(t);
            }
        }
        self.pos = save;
        let start = self.span_here();
        let expr = self.static_expr()?;
        let end = self.previous_span();
        Ok(TypeRef::Static {
            expr,
            span: Span::new(start.start, end.end),
        })
    }

    /// `a + b`, `a - b`, left associative, loosest.
    fn static_expr(&mut self) -> Parsed<StaticExpr> {
        let mut left = self.static_product()?;
        loop {
            let op = match self.peek_kind() {
                Some(Kind::Plus) => StaticOp::Add,
                Some(Kind::Minus) => StaticOp::Sub,
                _ => return Ok(left),
            };
            self.pos += 1;
            self.skip_newlines();
            let right = self.static_product()?;
            left = StaticExpr::Bin {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
    }

    /// JUXTAPOSITION IS THE PRODUCT. `2 jmax imax` is a product in Fortress and
    /// the corpus writes it that way inside a static argument at thirteen
    /// sites. There is no `*` here because no corpus file writes one.
    fn static_product(&mut self) -> Parsed<StaticExpr> {
        let mut left = self.static_atom()?;
        loop {
            if !matches!(
                self.peek_kind(),
                Some(Kind::IntLit { .. } | Kind::Ident(_) | Kind::LParen)
            ) {
                return Ok(left);
            }
            let right = self.static_atom()?;
            left = StaticExpr::Bin {
                op: StaticOp::Mul,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
    }

    fn static_atom(&mut self) -> Parsed<StaticExpr> {
        let span = self.span_here();
        match self.peek_kind() {
            Some(Kind::IntLit { digits, .. }) => {
                let digits = digits.clone();
                self.pos += 1;
                digits
                    .parse::<i64>()
                    .map(StaticExpr::Int)
                    .map_err(|_| ParseError::StaticExpressionOutOfRange { span, digits })
            }
            Some(Kind::True) => {
                self.pos += 1;
                Ok(StaticExpr::Bool(true))
            }
            Some(Kind::False) => {
                self.pos += 1;
                Ok(StaticExpr::Bool(false))
            }
            Some(Kind::Minus) => {
                self.pos += 1;
                // `-n` is `0 - n`, so negation needs no node of its own and the
                // evaluator needs no unary case.
                let inner = self.static_atom()?;
                Ok(StaticExpr::Bin {
                    op: StaticOp::Sub,
                    left: Box::new(StaticExpr::Int(0)),
                    right: Box::new(inner),
                })
            }
            Some(Kind::Ident(name)) => {
                let name = (*name).to_owned();
                self.pos += 1;
                Ok(StaticExpr::Ref(name))
            }
            Some(Kind::LParen) => {
                self.pos += 1;
                self.skip_newlines();
                let inner = self.static_expr()?;
                self.skip_newlines();
                self.expect(&Kind::RParen, "`)`")?;
                Ok(inner)
            }
            _ => Err(self.error(
                "a static argument: a type, a literal, or arithmetic over \
                                 the enclosing static parameters",
            )),
        }
    }

    // --------------------------------------------------------- expressions

    /// `concrete-syntax.tex:906-907` puts BOTH type annotations at the
    /// OUTERMOST `Expr` level, which is here and is the loosest binding there
    /// is: `a + b typed T` is `(a + b) typed T`. Every corpus site bounds the
    /// operand with a delimiter anyway -- `(1 asif N)`, `f(anA() asif A)`,
    /// `<|0 asif ZZ32, 1, 2|>`, `(self asif Generator[\E\]).asString`.
    ///
    /// ONE PRODUCTION, TWO KEYWORDS, TWO FEATURES. `typed` is an ASCRIPTION and
    /// is implemented; `asif` is an ASSUMPTION and the checker refuses it by
    /// name. The parser does not decide that -- it records WHICH WORD WAS
    /// WRITTEN and lets the checker say what it means, so both land in their
    /// own bucket instead of one `expected )` for the pair.
    ///
    /// A LOOP AND NOT A SINGLE STEP: the production is left-recursive, so
    /// `e typed T asif U` is legal shape. No corpus file writes one; the loop
    /// costs one line and refusing it would need a reason.
    fn expr(&mut self) -> Parsed<Expr> {
        let mut value = self.disjunction()?.0;
        loop {
            let assumption = match self.peek_kind() {
                Some(Kind::Reserved("typed")) => false,
                Some(Kind::Reserved("asif")) => true,
                _ => return Ok(value),
            };
            self.pos += 1;
            self.skip_newlines();
            let ty = self.type_ref()?;
            let span = Span::new(value.span().start, ty.span().end);
            value = Expr::Annotated {
                value: Box::new(value),
                ty,
                assumption,
                span,
            };
        }
    }

    /// `precedence.tex:20-31`: Fortress precedence is a PARTIAL relation --
    /// "if there is no specific precedence relationship between two operators,
    /// then parentheses must be used". A total ladder can only ever ACCEPT, so
    /// adding a second operator family to one makes wrong grouping SILENT,
    /// which is the worst class this project recognises.
    ///
    /// Every level therefore reports back which operator built the node it
    /// returns, when that operator is one of the ones this milestone adds and
    /// so has no relation to the arithmetic and relational ladder. A level that
    /// is about to combine such a node with its own operator refuses instead.
    ///
    /// The mark is carried rather than read off the tree because the tree
    /// cannot tell `(a SUBSET b) + c` from `a SUBSET b + c`: `primary` returns
    /// a parenthesised expression unchanged, so both are the same node. What
    /// clears the mark is the parenthesis, and the only place that knows about
    /// it is the parse.
    fn require_unmarked(&self, mark: Mark<'a>, other: &str, span: Span) -> Parsed<()> {
        match mark {
            Some((name, _)) => Err(ParseError::OperatorsUnrelated {
                span,
                first: name.to_owned(),
                second: other.to_owned(),
            }),
            None => Ok(()),
        }
    }

    /// `a OR b`, left associative, and below every conjunction --
    /// `appendices/operators.tex:840-851`.
    fn disjunction(&mut self) -> Parsed<(Expr, Mark<'a>)> {
        let (mut lhs, mut mark) = self.conjunction()?;
        while self.at_word_op("OR") {
            let span = self.span_here();
            self.require_unmarked(mark, "OR", span)?;
            let conditional = self.at_conditional_word_op("OR");
            self.pos += 1;
            if conditional {
                self.pos += 1;
            }
            self.skip_newlines();
            let (rhs, rhs_mark) = self.conjunction()?;
            self.require_unmarked(rhs_mark, "OR", span)?;
            lhs = infix(BinOp::Or, Fixity::Loose, lhs, rhs);
            mark = None;
        }
        Ok((lhs, mark))
    }

    /// `a AND b`, left associative, and below every relational operator --
    /// which is what puts `comparison` underneath it and makes
    /// `a = 3 AND b = 8` mean `(a = 3) AND (b = 8)`.
    fn conjunction(&mut self) -> Parsed<(Expr, Mark<'a>)> {
        let (mut lhs, mut mark) = self.comparison()?;
        while self.at_word_op("AND") {
            let span = self.span_here();
            self.require_unmarked(mark, "AND", span)?;
            let conditional = self.at_conditional_word_op("AND");
            self.pos += 1;
            if conditional {
                self.pos += 1;
            }
            self.skip_newlines();
            let (rhs, rhs_mark) = self.comparison()?;
            self.require_unmarked(rhs_mark, "AND", span)?;
            lhs = infix(BinOp::And, Fixity::Loose, lhs, rhs);
            mark = None;
        }
        Ok((lhs, mark))
    }

    /// A word operator is an all-capitals identifier the parser reads as an
    /// operator rather than as a name.
    ///
    /// Its shape is NEVER read from `fixity_at`. A word operator cannot be
    /// glued on its left -- the lexer would have merged the letters into one
    /// identifier -- so `a AND (b)` reads as `Prefix` and the operator would be
    /// left unconsumed, turning a correct program into a parse error.
    fn at_word_op(&self, word: &str) -> bool {
        matches!(self.peek_kind(), Some(Kind::OpWord(name)) if *name == word)
    }

    /// `AND:` and `OR:`, the CONDITIONAL forms. `basic-lib/booleans.tex:211`:
    /// "The conditional logical AND operator `AND:` examines its first
    /// argument" -- it short circuits, where plain `AND` is an ordinary
    /// operator that evaluates both.
    ///
    /// THIS COMPILER'S `AND` AND `OR` ALREADY SHORT CIRCUIT, so the colon form
    /// maps onto the same node and gets the semantics the specification asks
    /// for exactly. The over-eager half is the OTHER one -- plain `AND` also
    /// short circuits here -- and that is pre-existing, recorded, and not made
    /// worse by this.
    ///
    /// The colon must be GLUED. `lexical-structure.tex` makes an operator
    /// followed immediately by a character part of one token, and a spaced
    /// `a AND : b` is not this operator.
    fn at_conditional_word_op(&self, word: &str) -> bool {
        let colon_is_glued_on =
            self.glued_right(self.pos) && matches!(self.peek_ahead(1), Some(Kind::Colon));
        self.at_word_op(word) && colon_is_glued_on
    }

    /// One of the 66 words the lexer keeps out of the identifier namespace,
    /// matched by spelling. `of` and `with` are punctuation of a construct
    /// rather than expressions, so they never earn a token of their own.
    fn at_reserved(&self, word: &str) -> bool {
        matches!(self.peek_kind(), Some(Kind::Reserved(found)) if *found == word)
    }

    /// An infix word operator, which the juxtaposition run must stop at. `NOT`
    /// is deliberately not one: it is prefix, it DOES start an operand, and
    /// `unary` consumes it there.
    fn word_operator_here(&self) -> bool {
        self.at_word_op("AND") || self.at_word_op("OR")
    }

    /// Comparison operators chain. One operator is left exactly as it was: no
    /// block, no temporaries, and nothing about existing generated code moves.
    fn comparison(&mut self) -> Parsed<(Expr, Mark<'a>)> {
        let (first, first_mark) = self.additive()?;
        let mut mark = first_mark;
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
            self.require_unmarked(mark, op_text(op), op_span)?;
            self.pos += 1;
            self.skip_newlines(); // a newline may follow an infix operator
            let (operand, operand_mark) = self.additive()?;
            self.require_unmarked(operand_mark, op_text(op), op_span)?;
            operands.push(operand);
            ops.push((op, fixity, op_span));
            mark = None;
        }

        if ops.is_empty() {
            return Ok((operands.pop().ok_or(missing_operand())?, mark));
        }
        if ops.len() == 1 {
            let (op, fixity, _) = ops.first().copied().ok_or_else(missing_operand)?;
            let mut both = operands.into_iter();
            let lhs = both.next().ok_or_else(missing_operand)?;
            let rhs = both.next().ok_or_else(missing_operand)?;
            return Ok((infix(op, fixity, lhs, rhs), None));
        }
        Ok((self.desugar_chain(&operands, &ops)?, None))
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

    fn additive(&mut self) -> Parsed<(Expr, Mark<'a>)> {
        let (mut lhs, mut mark) = self.multiplicative()?;
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
            let op_span = self.span_here();
            self.require_unmarked(mark, op_text(op), op_span)?;
            self.pos += 1;
            self.skip_newlines();
            let (rhs, rhs_mark) = self.multiplicative()?;
            self.require_unmarked(rhs_mark, op_text(op), op_span)?;
            lhs = infix(op, fixity, lhs, rhs);
            mark = None;
        }
        Ok((lhs, mark))
    }

    fn multiplicative(&mut self) -> Parsed<(Expr, Mark<'a>)> {
        let (mut lhs, mut mark) = self.operator_expr()?;
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
            let op_span = self.span_here();
            self.require_unmarked(mark, op_text(op), op_span)?;
            self.pos += 1;
            self.skip_newlines();
            let (rhs, rhs_mark) = self.operator_expr()?;
            self.require_unmarked(rhs_mark, op_text(op), op_span)?;
            lhs = infix(op, fixity, lhs, rhs);
            mark = None;
        }
        Ok((lhs, mark))
    }

    /// The operators this milestone adds, applied infix.
    ///
    /// `opr-fixity.tex:28-32` is what decides the shape of this: "the Fortress
    /// language dictates only the rules of syntax; whether an operator has a
    /// meaning when used in a particular way depends only on whether there is a
    /// definition in the program". So `a SUBSET b` must PARSE as an infix
    /// application whether or not any `opr SUBSET` exists, and only then fail to
    /// resolve. Driving the syntax off declarations inverts that.
    ///
    /// It lowers to an ordinary `Call` to a function whose NAME is the
    /// operator's own text, which is exactly what an `opr` declaration already
    /// lifts to -- so nothing downstream learns a new node, dispatch and
    /// codegen are untouched, and an undeclared operator comes out as
    /// `unknown name`, which is the specification's own second half.
    ///
    /// `AND`, `OR` and `NOT` are deliberately NOT here. They are operator words
    /// under the same lexical rule, and they already have real codegen through
    /// `BinOp::And`, `BinOp::Or` and `UnOp::Not`; routing them through a call to
    /// a function nobody declared would break every program that uses them.
    fn operator_expr(&mut self) -> Parsed<(Expr, Mark<'a>)> {
        let mut lhs = self.juxtaposition()?;
        let mut mark: Mark<'a> = None;
        while let Some((text, span)) = self.infix_added_operator()? {
            if let Some((first, _)) = mark {
                if first != text {
                    return Err(ParseError::OperatorsUnrelated {
                        span,
                        first: first.to_owned(),
                        second: text.to_owned(),
                    });
                }
            }
            self.pos += 1;
            self.skip_newlines();
            let rhs = self.juxtaposition()?;
            let full = Span::new(lhs.span().start, rhs.span().end);
            lhs = Expr::Call {
                callee: Box::new(Expr::Var {
                    name: text.to_owned(),
                    span,
                }),
                args: vec![lhs, rhs],
                span: full,
            };
            mark = Some((text, span));
        }
        Ok((lhs, mark))
    }

    /// The added operators, and only when the twelve-row table reads this
    /// occurrence as infix. That test is what stops `|` being taken here in
    /// `f(|x|)`: after a left encloser the table says PREFIX, and an enclosing
    /// application is a different production.
    fn infix_added_operator(&self) -> Parsed<Option<(&'a str, Span)>> {
        let Some(kind) = self.peek_kind() else {
            return Ok(None);
        };
        let text = match kind {
            Kind::OpWord(word) if !matches!(*word, "AND" | "OR" | "NOT") => *word,
            Kind::BarBar => "||",
            Kind::EqEqEq => "===",
            Kind::BarRun(text) => *text,
            Kind::Bang => "!",
            Kind::Question => "?",
            Kind::Tilde => "~",
            Kind::Dollar => "$",
            Kind::Percent => "%",
            Kind::At => "@",
            Kind::UniOp(text) => *text,
            // `#` is DELIBERATELY absent. `for i <- 0#n` writes the extent
            // form of a generator range with it (`for_expr` reads it at
            // :2030), so taking it here as an infix operator makes the range
            // unparseable -- measured: nine corpus files, every one of them a
            // `for` loop, and the IR diff is what caught it.
            _ => return Ok(None),
        };
        // `lexical-structure.tex:1216-1222`: an operator immediately followed
        // by `=` is ONE token, a compound assignment operator. Reading only the
        // operator half reports `x ||= e` as a LOPSIDED infix, which is a real
        // rule and not the one the program broke.
        if self.glued_right(self.pos) && matches!(self.peek_ahead(1), Some(Kind::Eq)) {
            return Err(ParseError::CompoundAssignmentUnsupported {
                span: self.span_here(),
                op: text.to_owned(),
            });
        }
        match self.table_fixity_at(self.pos) {
            TableFixity::Infix => Ok(Some((text, self.span_here()))),
            // `opr-fixity.tex:90-93` calls this row a static error outright.
            TableFixity::Lopsided => Err(ParseError::LopsidedOperator {
                span: self.span_here(),
                name: text.to_owned(),
            }),
            _ => Ok(None),
        }
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
        // `"Reader on " self.fileName.asExprString`. A receiver is an operand
        // in a juxtaposition run exactly as a name is, and `self` alone already
        // parses at the START of an expression -- `left_context` and
        // `right_context` have both counted it a primary all along. Five
        // `Library/` files write the juxtaposed spelling. It sits outside the
        // match below because a mutation row may not contain a bar.
        if matches!(self.peek_kind(), Some(Kind::KwSelf)) {
            return true;
        }
        match self.peek_kind() {
            Some(
                Kind::IntLit { .. }
                | Kind::FloatLit { .. }
                | Kind::StrLit(_)
                | Kind::CharLit(_)
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

    /// Whether a BIG reduction starts `offset` tokens ahead: one of the four
    /// operator names, then a `[` GLUED to it.
    ///
    /// The glue is what separates `SUM[i <- 1:10]` from `SUM [i]`, which is a
    /// juxtaposition, exactly as `f(x)` is separated from `f (x)`. And `[\` is
    /// its own token, so `SUM[\Number\]` -- the static-argument form, 58 corpus
    /// sites -- cannot be mistaken for one of these.
    fn big_reduction_here(&self, offset: usize) -> bool {
        // All four names are ALL-CAPS with two distinct letters, so the
        // operator-word rule lexes them as `OpWord` and not as `Ident`. That is
        // the right reading rather than an accident -- `SUM` is an operator in
        // 1.0, which is why `SUM[i <- 1:10]` is a reduction and not a subscript
        // -- but both kinds are matched because `BIG` may precede a name the
        // rule does not reach.
        let named = matches!(
            self.peek_ahead(offset),
            Some(Kind::Ident(name) | Kind::OpWord(name)) if BIG_OPERATORS.contains(name)
        );
        named
            && matches!(self.peek_ahead(offset + 1), Some(Kind::LBracket))
            && self.glued_left(self.pos + offset + 1)
    }

    /// `BIG <op>` in expression position, in both of the shapes the corpus
    /// writes it.
    ///
    /// * `BIG <op>[gens] body` -- a reduction over an ARBITRARY operator.
    ///   `BIG AND[x <- self] (x IN other)`, `BIG ||[e <- x] e`. Four of them
    ///   fold onto the accumulator over a range; the rest, and any generator
    ///   over a collection, are refused BY NAME by `big_reduction`.
    /// * `BIG <op>(...)` and `BIG <op>[\T\](...)` -- a VALUE. The reduction
    ///   OBJECT, passed to `__bigOperatorSugar`. `Library/FortressLibrary
    ///   .fss:130` writes `BIG LEXICO()` and `simpleBig.fss` writes
    ///   `BIG STAR[\T\]()`, and both are ordinary calls of a declared name.
    ///
    /// THE TWO ARE SEPARATED BY A GLUED `[` THAT IS NOT `[\`. A generator
    /// bracket is glued to the operator and holds a binder; a static-argument
    /// bracket is its own token. Nothing else can tell them apart, which is the
    /// same test `big_reduction_here` already makes for the four.
    fn big_operator(&mut self) -> Parsed<Expr> {
        let start = self.span_here();
        let save = self.pos;
        self.pos += 1; // `BIG`
        let Some((op, _)) = self.operator_name_here() else {
            self.pos = save;
            return Err(ParseError::ReservedWord {
                span: start,
                word: "BIG".to_owned(),
            });
        };
        let generator_bracket =
            matches!(self.peek_kind(), Some(Kind::LBracket)) && self.glued_left(self.pos);
        if generator_bracket {
            self.pos = save;
            return self.big_reduction();
        }
        Ok(Expr::Var {
            name: format!("BIG {op}"),
            span: Span::new(start.start, self.previous_span().end),
        })
    }

    /// An operator NAME here, consumed. Every spelling the corpus puts after a
    /// `BIG`: a word operator, an identifier, `||`, a bar run, and the single
    /// character operators.
    fn operator_name_here(&mut self) -> Option<(String, Span)> {
        let span = self.span_here();
        let text = match self.peek_kind()? {
            Kind::Ident(name) | Kind::OpWord(name) | Kind::UniOp(name) | Kind::BarRun(name) => {
                (*name).to_owned()
            }
            Kind::BarBar => "||".to_owned(),
            Kind::Bar => "|".to_owned(),
            Kind::LeftBar => "<|".to_owned(),
            Kind::Plus => "+".to_owned(),
            Kind::Star => "*".to_owned(),
            Kind::LBrace => "{".to_owned(),
            _ => return None,
        };
        self.pos += 1;
        Some((text, span))
    }

    /// `SUM[i <- lo:hi] e`, and `PROD` likewise.
    ///
    /// All four operators, and the identity is what separates them: 0 for SUM,
    /// 1 for PROD, and THE TYPE'S OWN EXTREMUM for MAX and MIN. A MAX slot
    /// starting at zero reports 0 as the maximum of a set of negative numbers,
    /// silently, which is why the identity is codegen's rather than the
    /// allocator's memset.
    fn big_reduction(&mut self) -> Parsed<Expr> {
        let start = self.span_here();
        if self.at(&Kind::Reserved("BIG")) {
            self.pos += 1;
        }
        let (name, name_span) = self
            .operator_name_here()
            .ok_or_else(|| self.error("a reduction operator"))?;
        let op = match name.as_str() {
            "SUM" => BinOp::Add,
            "PROD" => BinOp::Mul,
            "MAX" => BinOp::Max,
            "MIN" => BinOp::Min,
            other => {
                return Err(ParseError::BigReductionUnsupported {
                    span: name_span,
                    name: other.to_owned(),
                    reason: "is not one of the reduction operators this lowering reaches; \
                             SUM, PROD, MAX and MIN fold onto the accumulator and the rest \
                             need the Reduction trait",
                })
            }
        };
        self.expect(&Kind::LBracket, "`[`")?;
        self.skip_newlines();
        let Generator {
            binder,
            lo,
            hi,
            inclusive,
            sequential,
        } = self.generator_clause()?;
        let Some(hi) = hi else {
            return Err(ParseError::BigReductionUnsupported {
                span: name_span,
                name,
                reason: "over a collection needs the generator protocol; over a RANGE it \
                         folds onto the accumulator directly",
            });
        };
        self.skip_newlines();
        self.expect(&Kind::RBracket, "`]` to close the generator")?;
        self.skip_newlines();
        let body = self.expr()?;
        let span = Span::new(start.start, body.span().end);
        Ok(Expr::BigReduction {
            op,
            binder,
            lo: Box::new(lo),
            hi: Box::new(hi),
            inclusive,
            sequential,
            body: Box::new(body),
            span,
        })
    }

    /// `i <- lo:hi`, `i <- lo#n`, and either wrapped in `seq(...)`. ONE
    /// implementation, because `for` and a BIG reduction accept exactly the
    /// same generator and a second copy is a second place for them to drift.
    fn generator_clause(&mut self) -> Parsed<Generator> {
        let (binder, _) = self.identifier("a loop variable")?;
        self.skip_newlines();
        let Some(width) = self.left_arrow_width() else {
            return Err(self.error("`<-` after the loop variable"));
        };
        self.pos += width;
        self.skip_newlines();

        // `seq(...)` is recognised HERE rather than as a call, because it is
        // what decides whether the loop is parallel and the checker must not
        // have to guess that back from an application node.
        // `seq` is LOWERCASE and so is an ordinary identifier, not an operator
        // word: `lexical-structure.tex:1167-1172` admits only uppercase
        // letters and underscores. It shares no test with `AND` and `OR`.
        let sequential = matches!(self.peek_kind(), Some(Kind::Ident("seq")))
            && matches!(self.peek_ahead(1), Some(Kind::LParen));
        if sequential {
            self.pos += 2;
            self.skip_newlines();
        }
        let lo = self.expr()?;
        let range = match self.peek_kind() {
            Some(Kind::Colon) => {
                self.pos += 1;
                self.skip_newlines();
                Some((self.expr()?, true))
            }
            Some(Kind::Hash) => {
                self.pos += 1;
                self.skip_newlines();
                Some((self.expr()?, false))
            }
            // No `:` and no `#`: the source is a value rather than a range --
            // `for x <- a`. Which values are iterable is the checker's
            // question, and it answers `Array` today.
            _ => None,
        };
        if sequential {
            self.expect(&Kind::RParen, "`)` to close `seq(`")?;
        }
        let Some((hi, inclusive)) = range else {
            return Ok(Generator {
                binder,
                lo,
                hi: None,
                inclusive: false,
                sequential,
            });
        };
        Ok(Generator {
            binder,
            lo,
            hi: Some(hi),
            inclusive,
            sequential,
        })
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
        let Generator {
            binder,
            lo,
            hi,
            inclusive,
            sequential,
        } = self.generator_clause()?;
        self.skip_newlines();
        self.expect(&Kind::KwDo, "`do`")?;
        let body = self.block_body(&[Kind::KwEnd])?;
        let end = self.expect(&Kind::KwEnd, "`end`")?.span;
        let Some(hi) = hi else {
            return Ok(Expr::ForIn {
                binder,
                source: Box::new(lo),
                sequential,
                body: Box::new(body),
                span: Span::new(start.start, end.end),
            });
        };
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
    /// `Symbol.rats:197`: `leftarrow = "<-" / "\u2190"`. The ASCII spelling is
    /// two tokens joined by adjacency and the Unicode one is a single token, so
    /// this answers how many to step over rather than merely whether to.
    fn left_arrow_width(&self) -> Option<usize> {
        if self.at(&Kind::LeftArrow) {
            return Some(1);
        }
        (self.at(&Kind::Lt)
            && self.glued_right(self.pos)
            && matches!(self.peek_ahead(1), Some(Kind::Minus)))
        .then_some(2)
    }

    /// The operator word at the cursor, IF this occurrence is prefix.
    ///
    /// THREE EXCLUSIONS, each for its own reason. `AND`, `OR` and `NOT` have
    /// real codegen through `BinOp`/`UnOp` and routing them through a call to a
    /// function nobody declared would break every program that uses them --
    /// the same carve-out `infix_added_operator` makes, for the same reason.
    /// A BIG REDUCTION is checked FIRST because `primary` is downstream of
    /// `unary`: without this, `SUM[i <- 1:10] e` is taken here as a prefix
    /// operator applied to a subscript and `big_reduction` never runs.
    /// And the TABLE has the last word, so an occurrence it reads as infix,
    /// postfix or nofix is left to the layer that owns it.
    fn prefix_operator_word_here(&self) -> Option<&'a str> {
        let Some(Kind::OpWord(word)) = self.peek_kind() else {
            return None;
        };
        let word: &'a str = word;
        if CODEGEN_OPERATOR_WORDS.contains(&word) {
            return None;
        }
        if self.big_reduction_here(0) {
            return None;
        }
        if !matches!(self.table_fixity_at(self.pos), TableFixity::Prefix) {
            return None;
        }
        Some(word)
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
        // A PREFIX OPERATOR WORD, and the position is the whole argument.
        // `opr-fixity.tex:34-55` decides fixity from LEFT CONTEXT: an operator
        // whose left context is another OPERATOR or a DELIMITER is PREFIX, and
        // `unary` is reached at exactly that position. So this asks the same
        // twelve-row table `infix_added_operator` asks, and takes the occurrence
        // only where the table says prefix -- `delta_Y / SQRT rsq`, `(BITNOT
        // six) + 1`, `= CONVERSE other`.
        //
        // A CALL AND NOT A `UnOp`, because `UnOp` is a closed set with real
        // codegen and a word operator is an ordinary declaration: `opr SQRT(x)`
        // declares a function named `SQRT`. That is the same node
        // `operator_expr` builds for the INFIX case, with one argument instead
        // of two, so both spellings reach one overload set.
        if let Some(word) = self.prefix_operator_word_here() {
            let span = self.span_here();
            self.pos += 1;
            let operand = self.unary()?;
            let full = Span::new(span.start, operand.span().end);
            return Ok(Expr::Call {
                callee: Box::new(Expr::Var {
                    name: word.to_owned(),
                    span,
                }),
                args: vec![operand],
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
                // A COMMA SEPARATED LIST, `arrays.tex`'s `a[i,j]`. Before this
                // the second index was a parse error -- `expected `]`, found
                // Comma` -- which named the delimiter rather than the feature
                // and sent the reader to the wrong place.
                let mut indices = vec![self.expr()?];
                self.skip_newlines();
                while self.at(&Kind::Comma) {
                    self.pos += 1;
                    self.skip_newlines();
                    indices.push(self.expr()?);
                    self.skip_newlines();
                }
                let close = self.expect(&Kind::RBracket, "`]`")?.span;
                let span = Span::new(expr.span().start, close.end);
                expr = Expr::Index {
                    base: Box::new(expr),
                    indices,
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
            Kind::CharLit(value) => {
                self.pos += 1;
                Ok(Expr::CharLit {
                    value: *value,
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
            // Before the plain name arm: `SUM[i <- 1:10] e` is a reduction and
            // `SUM[i]` is a subscript, and only the guard tells them apart.
            Kind::Ident(_) | Kind::OpWord(_) if self.big_reduction_here(0) => self.big_reduction(),
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
            // `throw e`. A PREFIX over a full expression, which is what the
            // corpus writes: `throw NotFound`, `throw TestFailCalled(s)`,
            // `throw KeyOverlap[\Key,Val\](pk,pv,cv)`. It stops where any
            // expression stops, so `else throw E end` closes on the `end`.
            Kind::Reserved("try") => self.try_expr(),
            Kind::Reserved("throw") => {
                let start = self.span_here();
                self.pos += 1;
                self.skip_newlines();
                let value = self.expr()?;
                let span = Span::new(start.start, value.span().end);
                Ok(Expr::Throw {
                    value: Box::new(value),
                    span,
                })
            }
            // Same trade as `for` and `atomic`: intercepted here rather than
            // given a keyword token, so no file in the corpus lexes
            // differently.
            Kind::Reserved("spawn") => self.spawn_expr(),
            // The same trade `for` and `atomic` take: intercepted here rather
            // than given a keyword token, so no file in the corpus lexes
            // differently. `of`, `with`, `most`, `largest` and `smallest` stay
            // Reserved and are matched by word below.
            Kind::Reserved("case") => self.case_expr(),
            Kind::Reserved("typecase") => self.typecase_expr(),
            Kind::Reserved("label") => self.label_expr(),
            Kind::Reserved("exit") => self.exit_expr(),
            Kind::Reserved("fn") => self.lambda_expr(),
            // `BIG SUM[i <- 1:10] e`. `BIG` is a reserved word and optional in
            // the corpus, which writes both spellings.
            // `object extends T ... end` -- an anonymous object.
            // `DelimitedExpr.rats:50`: no name, no value parameters, and the
            // `end` closes nothing that could be named, so `named_end` does not
            // apply either.
            Kind::KwObject => {
                let start = self.span_here();
                self.pos += 1;
                let topology = self.topology_clauses()?;
                let members = self.members()?;
                let end = self.expect(&Kind::KwEnd, "`end`")?.span;
                Ok(Expr::ObjectExpr {
                    extends: topology.extends,
                    members,
                    span: Span::new(start.start, end.end),
                })
            }
            Kind::Reserved("BIG") if self.big_reduction_here(1) => self.big_reduction(),
            // EVERY OTHER `BIG`. It is a MODIFIER ON THE OPERATOR NAME, not a
            // keyword of its own -- `opr BIG SQCAP` and `opr SQCAP` are two
            // declarations and the declaration side has folded the two words
            // into one name since the `opr` spike. This is the use side, and it
            // folds the same way, so `BIG LEXICO()` is a call of the name
            // `BIG LEXICO` and `BIG ||[e <- x] e` is a reduction over the
            // operator `BIG ||`.
            Kind::Reserved("BIG") => self.big_operator(),
            Kind::Reserved(word) => Err(ParseError::ReservedWord {
                span,
                word: (*word).to_owned(),
            }),
            Kind::Bar
            | Kind::BarBar
            | Kind::BarRun(_)
            | Kind::LeftBar
            | Kind::LBrace
            // U+27E8 and U+27E9 are angle brackets, and the allowlist gives
            // them no ASCII spelling because the reference grammar gives them
            // none either -- so the pair they name is `\u{27E8}_\u{27E9}`.
            | Kind::UniOp(_) => self.enclosing_application(),
            _ => Err(self.error("an expression")),
        }
    }

    /// `|x|`, `<|a, b|>`, `{a, b}`, `|\x/|`. An enclosing operator writes its
    /// operands INSIDE the brackets, and `enclosing-ops.tex` gives the pair one
    /// name; this parser already spells that name `|_|`, `<|_|>`, `{_}` on the
    /// DECLARATION side, where `_` marks the operand position and is what keeps
    /// `|self|` from being given the name `||`. The application is the same
    /// name applied, so it is an ordinary `Call` like every other operator.
    ///
    /// `aggregate.tex:44-47` says the set, map and list literals ARE
    /// applications of a bracketing operator with a varargs parameter, so this
    /// is also what makes `<|1, 2, 3|>` a parse. It is NOT yet what makes it
    /// compile: the library declares `opr <|[\E\] xs: E... |>` with ONE
    /// varargs parameter, the callee side of varargs is still accept-and-ignore,
    /// so a three-element literal is refused on arity. That is the recorded
    /// consequence of not having decided what `T...` lowers to, not a defect
    /// here.
    ///
    /// `[` IS DELIBERATELY ABSENT. `[1, 2, 3]` is already an array literal with
    /// its own node and its own codegen; reading it as an application of `[_]`
    /// would change what every array-literal program means.
    ///
    /// A bare `|` is deliberately absent from the INFIX set for the same reason
    /// this production can exist at all: with `|` infix, `|x| + |y|` has two
    /// readings and adjacency cannot separate them.
    fn enclosing_application(&mut self) -> Parsed<Expr> {
        let start = self.span_here();
        let mut open = self.operator_run(usize::MAX);
        if open.is_empty() {
            return Err(self.error("an expression"));
        }
        // STATIC ARGUMENTS GO INSIDE THE OPENER. `DelimitedExpr.rats:298-309`:
        // `LeftEncloser (w StaticArgs)? w ...`. `<|[\E\]|>` is the empty list
        // AT an element type and 471 corpus sites write one, so without this the
        // whole family stops at `expected an expression, found LGeneric`.
        let mut static_args = Vec::new();
        if self.at(&Kind::LGeneric) {
            self.pos += 1;
            static_args = self.type_args()?;
            self.expect(&Kind::RGeneric, "`\\]`")?;
            self.skip_newlines();
        }
        // AN EMPTY ENCLOSER IS ONE RUN. `<||>` and `{}` have no operand to stop
        // the opening run, so it swallows the closing half as well: `<|` is
        // glued to `|>`. When the run is of even length and nothing that could
        // begin an expression follows it, the two halves ARE the pair.
        let mut args = Vec::new();
        if open.len().is_multiple_of(2) && !self.starts_an_operand() {
            let half = open.len().div_euclid(2);
            let close = open.split_off(half);
            return Ok(self.enclosed(start, &open, &close, static_args, args));
        }
        self.skip_newlines();
        let empty = self.closes_here(open.len());
        if !empty {
            loop {
                args.push(self.expr()?);
                self.skip_newlines();
                // A COMPREHENSION, and the separator is a BARE `|` WITH
                // WHITESPACE ON BOTH SIDES -- `DelimitedExpr.rats:298,306` write
                // it `wr bar wr`, and `Spacing.rats:93` makes `wr` mandatory.
                // `<|x|x<-s|>` does not parse in 1.0 either.
                if self.comprehension_bar_here() {
                    self.pos += 1;
                    self.skip_newlines();
                    let gens = self.generator_clause_list()?;
                    self.skip_newlines();
                    let close = self.operator_run(open.len());
                    if close.len() != open.len() {
                        return Err(self.error("the closing half of a comprehension"));
                    }
                    let mut bracket = join(&open);
                    bracket.push('_');
                    bracket.push_str(&join(&close));
                    let body = args.pop().unwrap_or(Expr::Unit { span: start });
                    return Ok(Expr::Comprehension {
                        bracket,
                        static_args,
                        body: Box::new(body),
                        gens,
                        span: Span::new(start.start, self.previous_span().end),
                    });
                }
                if !self.at(&Kind::Comma) {
                    break;
                }
                self.pos += 1;
                self.skip_newlines();
            }
        }
        // The closer is read with the OPENER'S LENGTH as its limit, which is
        // what stops `|a| + |b|` running the closing run on into the `+`. The
        // declaration side cannot use that rule -- `opr {|->[\K,V\] xs: ... }`
        // opens with four characters and closes with one -- but in expression
        // position the pair is symmetric and the limit is what disambiguates.
        let close = self.operator_run(open.len());
        if close.len() != open.len() {
            return Err(self.error("the closing half of an enclosing operator"));
        }
        Ok(self.enclosed(start, &open, &close, static_args, args))
    }

    /// A bare `|` that separates a comprehension's body from its generators.
    ///
    /// `Symbol.rats:51-58` decides this with UNBOUNDED LOOKAHEAD -- a `|` is the
    /// separator only if a whole generator clause list and a closer follow. The
    /// cheap test here is the SPACING rule the same grammar imposes
    /// (`wr bar wr`, whitespace REQUIRED on both sides) plus a scan for a `<-`
    /// before the closing run. Without the scan, `ps || <| ... |> || qs` --
    /// 160 corpus sites write `BIG ||` over one -- takes the wrong branch.
    fn comprehension_bar_here(&self) -> bool {
        if !matches!(self.peek_kind(), Some(Kind::Bar)) {
            return false;
        }
        if self.glued_left(self.pos) || self.glued_right(self.pos) {
            return false;
        }
        let mut depth = 0i32;
        for index in self.pos + 1..self.tokens.len() {
            match self.tokens.get(index).map(|t| &t.kind) {
                Some(Kind::LParen | Kind::LBracket | Kind::LGeneric | Kind::LBrace) => depth += 1,
                Some(Kind::RParen | Kind::RBracket | Kind::RGeneric | Kind::RBrace) => depth -= 1,
                // `<-` is two tokens joined by adjacency, the same way
                // `generator_clause` reads it.
                Some(Kind::Lt) if depth == 0 && self.glued_right(index) => {
                    if matches!(
                        self.tokens.get(index + 1).map(|t| &t.kind),
                        Some(Kind::Minus)
                    ) {
                        return true;
                    }
                }
                Some(Kind::LeftArrow) if depth == 0 => return true,
                Some(Kind::Newline | Kind::Semi | Kind::Eof) | None => return false,
                _ => {}
            }
        }
        false
    }

    /// `x <- g, p, (a,b) <- h`. A clause with no `<-` is a GUARD, and 1.0
    /// represents it the same way: a generator clause with an empty binder.
    fn generator_clause_list(&mut self) -> Parsed<Vec<GeneratorClause>> {
        let mut out = Vec::new();
        loop {
            let start = self.span_here();
            let save = self.pos;
            let binders = self.comprehension_binders();
            if binders.is_empty() {
                self.pos = save;
            }
            let init = self.expr()?;
            // THE SAME GENERATOR A `for` TAKES. `1:10` and `0#n` are two
            // expressions and a form, so `expr()` alone stops at the `:` and
            // the closing run then reports the wrong token.
            let (hi, inclusive) = if self.at(&Kind::Colon) {
                self.pos += 1;
                self.skip_newlines();
                (Some(self.expr()?), true)
            } else if self.at(&Kind::Hash) {
                self.pos += 1;
                self.skip_newlines();
                (Some(self.expr()?), false)
            } else {
                (None, false)
            };
            out.push(GeneratorClause {
                binders,
                init,
                hi,
                inclusive,
                span: Span::new(start.start, self.previous_span().end),
            });
            self.skip_newlines();
            if !self.at(&Kind::Comma) {
                break;
            }
            self.pos += 1;
            self.skip_newlines();
        }
        Ok(out)
    }

    /// `x <-` or `(a, b) <-`, consumed, or nothing consumed and an empty list.
    fn comprehension_binders(&mut self) -> Vec<String> {
        let save = self.pos;
        let mut names = Vec::new();
        if self.at(&Kind::LParen) {
            self.pos += 1;
            loop {
                match self.peek_kind() {
                    Some(Kind::Ident(n)) => {
                        names.push((*n).to_owned());
                        self.pos += 1;
                    }
                    _ => {
                        self.pos = save;
                        return Vec::new();
                    }
                }
                if self.at(&Kind::Comma) {
                    self.pos += 1;
                    continue;
                }
                break;
            }
            if !self.at(&Kind::RParen) {
                self.pos = save;
                return Vec::new();
            }
            self.pos += 1;
        } else if let Some(Kind::Ident(n)) = self.peek_kind() {
            names.push((*n).to_owned());
            self.pos += 1;
        } else {
            return Vec::new();
        }
        let Some(width) = self.left_arrow_width() else {
            self.pos = save;
            return Vec::new();
        };
        self.pos += width;
        self.skip_newlines();
        names
    }

    /// The name is the pair with `_` where the operands go, which is exactly
    /// what `opr_signature` builds on the declaration side.
    fn enclosed(
        &self,
        start: Span,
        open: &[&str],
        close: &[&str],
        static_args: Vec<TypeRef>,
        args: Vec<Expr>,
    ) -> Expr {
        let mut name = join(open);
        name.push('_');
        name.push_str(&join(close));
        let span = Span::new(start.start, self.previous_span().end);
        let callee = Expr::Var { name, span: start };
        let callee = if static_args.is_empty() {
            callee
        } else {
            Expr::Instantiate {
                callee: Box::new(callee),
                args: static_args,
                span,
            }
        };
        Expr::Call {
            callee: Box::new(callee),
            args,
            span,
        }
    }

    /// Whether anything that could begin an operand follows. Used only to tell
    /// an empty encloser from one whose opening run happens to be long.
    fn starts_an_operand(&self) -> bool {
        !matches!(
            self.peek_kind(),
            None | Some(
                Kind::RParen
                    | Kind::RBracket
                    | Kind::RGeneric
                    | Kind::Comma
                    | Kind::Semi
                    | Kind::Newline
                    | Kind::Eof
            )
        )
    }

    /// True when the operator run beginning here is exactly `len` tokens long,
    /// which is how an EMPTY encloser (`<||>`, `{}`) is told from one with an
    /// operand. Restores the position either way.
    fn closes_here(&mut self, len: usize) -> bool {
        let mark = self.pos;
        let run = self.operator_run(len);
        self.pos = mark;
        run.len() == len
    }

    /// `[1 2 3]`, `[1 2; 3 4]`, `[1 2;; 3 4]`, and a bare newline as a row
    /// separator. `aggregate.tex:29-34`: `RectElements ::= Expr MultiDimCons*`,
    /// `MultiDimCons ::= RectSeparator Expr`, `RectSeparator ::= ';'+ |
    /// Whitespace`.
    ///
    /// THE SEPARATOR LEVEL IS WHAT DECIDES THE SHAPE, and the mapping from
    /// level to DIMENSION is not the identity. `aggregate.tex:143-150` gives
    /// the oracle as a value rather than a shape: for
    /// `A: ZZ32[3,3] = [1 2 3; 4 5 6; 7 8 9]`, "then `A(1,0)` evaluates to 4".
    /// So `;` steps dimension 0 and whitespace steps dimension 1 -- rows and
    /// then columns -- and `ProjectFortress/tests/arrayTest2.fss` pins the rest:
    /// a `ZZ32[2,3,4]` written with `;;` asserts `a[0,0,1]` is the first
    /// element of the SECOND `;;` group, so `;;` steps dimension 2.
    ///
    /// A NEWLINE IS A LEVEL-ONE SEPARATOR and `SpecData/.../Expr.Array.b`
    /// through `.e` are four spellings the specification calls equivalent --
    /// `;`, a newline, a newline then `;`, and `;` then a newline. So the run
    /// between two elements is read as a WHOLE: its level is the number of
    /// semicolons in it, or one if it has none and holds a line break.
    /// COST OF THE NEWLINE RULE, measured before it was written: ZERO. Not one
    /// of the 411 files that compile writes a bracket literal with a line break
    /// inside it, so no existing program changes meaning.
    fn array_literal(&mut self) -> Parsed<Expr> {
        let start = self.expect(&Kind::LBracket, "`[`")?.span;
        let mut items: Vec<Expr> = Vec::new();
        // One per GAP, so `levels.len() + 1 == items.len()` whenever the
        // literal is non-empty.
        let mut levels: Vec<usize> = Vec::new();
        self.skip_newlines();
        if !self.at(&Kind::RBracket) {
            loop {
                // `self.expr()` swallows a whitespace-separated run as one
                // juxtaposition, so `[1 2 3]` would be ONE element holding 6.
                // 128 corpus sites over 65 files write the juxtaposed spelling.
                // Split after the fact rather than by suppressing juxtaposition
                // inside the brackets: the flag version needs five save/restore
                // sites and buys nothing. It does NOT see a run buried under an
                // infix operator -- `[a b + c d]` is one Infix over two Juxts --
                // which is unchanged and still open.
                match self.expr()? {
                    Expr::Juxt { items: run, .. } => items.extend(run),
                    single => items.push(single),
                }
                // A juxtaposition run is `n` elements with `n - 1` gaps and
                // every one of them is whitespace.
                levels.resize(items.len().saturating_sub(1), 0);
                let level = self.separator_run();
                if self.at(&Kind::RBracket) {
                    break;
                }
                if self.at(&Kind::Comma) {
                    self.pos += 1;
                    self.skip_newlines();
                    levels.push(level);
                    continue;
                }
                if level == 0 {
                    // Nothing separated the two, so nothing follows: let
                    // `expect` below name the token it actually found.
                    break;
                }
                levels.push(level);
            }
        }
        let close = self.expect(&Kind::RBracket, "`]`")?.span;
        let span = Span::new(start.start, close.end);
        let (items, extents) = Self::rectangle(items, &levels, span)?;
        Ok(Expr::ArrayLit {
            items,
            extents,
            span,
        })
    }

    /// The run of separators between two elements, as its LEVEL. Semicolons and
    /// line breaks are read together because the specification calls the four
    /// spellings equivalent, and a run of `; ;` with a space in it is still two
    /// semicolons -- `Expr.Array.f` writes exactly that.
    fn separator_run(&mut self) -> usize {
        let mut semicolons = 0;
        let mut line_break = false;
        loop {
            if self.at(&Kind::Semi) {
                semicolons += 1;
            } else if self.at(&Kind::Newline) {
                line_break = true;
            } else {
                break;
            }
            self.pos += 1;
        }
        if semicolons > 0 {
            semicolons
        } else {
            usize::from(line_break)
        }
    }

    /// Elements in source order plus the level of each gap, to elements in
    /// ROW-MAJOR order plus one extent per dimension.
    ///
    /// TWO PASSES AND THE FIRST ONE IS THE CHECK. An odometer alone would take
    /// each extent to be the largest index it happened to reach, which accepts
    /// `[1 2; 3]` as a 2 by 2 with a hole in it. `shape` recurses over the
    /// separator levels and refuses a group whose length differs from its
    /// siblings', by name and with both lengths.
    fn rectangle(
        items: Vec<Expr>,
        levels: &[usize],
        span: Span,
    ) -> Parsed<(Vec<Expr>, Vec<usize>)> {
        if items.is_empty() {
            return Ok((items, vec![0]));
        }
        let top = levels.iter().copied().max().unwrap_or(0);
        if top == 0 {
            let n = items.len();
            return Ok((items, vec![n]));
        }
        // Highest level first: `[groups, rows, columns]` for a `;;` literal.
        let by_level = Self::shape(levels, 0, items.len(), top, span)?;
        let rank = top + 1;
        let mut extents = vec![0usize; rank];
        for (level, count) in by_level.iter().rev().enumerate() {
            if let Some(slot) = extents.get_mut(Self::dimension_of(level)) {
                *slot = *count;
            }
        }

        let mut strides = vec![1usize; rank];
        for d in (0..rank.saturating_sub(1)).rev() {
            let next = strides
                .get(d + 1)
                .copied()
                .unwrap_or(1)
                .saturating_mul(extents.get(d + 1).copied().unwrap_or(1));
            if let Some(slot) = strides.get_mut(d) {
                *slot = next;
            }
        }
        let mut placed: Vec<Option<Expr>> = (0..items.len()).map(|_| None).collect();
        let mut at = vec![0usize; rank];
        for (index, item) in items.into_iter().enumerate() {
            let offset: usize = at
                .iter()
                .zip(&strides)
                .map(|(c, s)| c.saturating_mul(*s))
                .sum();
            if let Some(slot) = placed.get_mut(offset) {
                *slot = Some(item);
            }
            if let Some(level) = levels.get(index) {
                if let Some(slot) = at.get_mut(Self::dimension_of(*level)) {
                    *slot += 1;
                }
                for lower in 0..*level {
                    if let Some(slot) = at.get_mut(Self::dimension_of(lower)) {
                        *slot = 0;
                    }
                }
            }
        }
        // `shape` proved the literal rectangular, so every slot was written.
        let items = placed.into_iter().flatten().collect();
        Ok((items, extents))
    }

    /// Which DIMENSION a separator of this level steps. Whitespace steps the
    /// horizontal one -- `aggregate.tex:146-148`, "the horizontal dimension of
    /// an array is the last dimension mentioned in the array index" -- and `;`
    /// steps the vertical one, so the two lowest levels are swapped and every
    /// level above them is its own dimension.
    const fn dimension_of(level: usize) -> usize {
        match level {
            0 if Self::SWAP_LOWEST_TWO => 1,
            1 if Self::SWAP_LOWEST_TWO => 0,
            other => other,
        }
    }

    /// NAMED SO THAT A MUTATION CAN TURN IT OFF IN ONE LINE, and the mutation
    /// it enables is the one that matters: with this false the two lowest
    /// levels map to themselves, which is a BIJECTION -- a transposed array of
    /// the same shape. On a square literal every extent still checks out and
    /// only the VALUE differs, so `aggregate.tex:150`'s "A(1,0) evaluates to 4"
    /// is the single assertion that can see it.
    const SWAP_LOWEST_TWO: bool = true;

    /// The extent at each level from `level` down to zero, highest first.
    fn shape(
        levels: &[usize],
        lo: usize,
        hi: usize,
        level: usize,
        span: Span,
    ) -> Parsed<Vec<usize>> {
        if level == 0 {
            return Ok(vec![hi - lo]);
        }
        let mut groups: Vec<(usize, usize)> = Vec::new();
        let mut start = lo;
        for gap in lo..hi.saturating_sub(1) {
            if levels.get(gap).copied().unwrap_or(0) == level {
                groups.push((start, gap + 1));
                start = gap + 1;
            }
        }
        groups.push((start, hi));
        let mut inner: Option<Vec<usize>> = None;
        for (from, to) in &groups {
            let sub = Self::shape(levels, *from, *to, level - 1, span)?;
            match &inner {
                None => inner = Some(sub),
                Some(first) if *first == sub => {}
                Some(first) => {
                    return Err(ParseError::ArrayLiteralRagged {
                        span,
                        level: level - 1,
                        expected: first.first().copied().unwrap_or(0),
                        found: sub.first().copied().unwrap_or(0),
                    })
                }
            }
        }
        let mut out = vec![groups.len()];
        out.extend(inner.unwrap_or_default());
        Ok(out)
    }

    /// `while cond do ... end`. The only loop in the language until generators
    /// arrive, and deliberately so: `for` is parallel by default in Fortress
    /// and cannot be faked with a counter.
    fn while_expr(&mut self) -> Parsed<Expr> {
        let start = self.expect(&Kind::KwWhile, "`while`")?.span;
        self.skip_newlines();
        if let Some(binders) = self.binding_condition_here(&Kind::KwDo) {
            let source = self.generator_source(&binders)?;
            self.skip_newlines();
            self.expect(&Kind::KwDo, "`do`")?;
            let body = self.block_body(&[Kind::KwEnd])?;
            let end = self.expect(&Kind::KwEnd, "`end`")?.span;
            return Ok(Expr::BindingCondition {
                binders,
                source: Box::new(source),
                body: Box::new(body),
                loops: true,
                otherwise: None,
                span: Span::new(start.start, end.end),
            });
        }
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
        if let Some(binders) = self.binding_condition_here(&Kind::KwThen) {
            let source = self.generator_source(&binders)?;
            // `then` IS OPTIONAL after a generator clause -- 1.0's grammar
            // says so at `DelimitedExpr.rats:39` -- AND A NEWLINE MAY PRECEDE
            // IT. `Pairs.fss:24` writes the whole `then v else ... end` on the
            // line below, and nine corpus files do; without the skip the body
            // starts at `then` and reports `expected an expression`.
            self.then_keyword();
            let body = self.block_body(&[Kind::KwElse, Kind::KwElif, Kind::KwEnd])?;
            let otherwise = if self.at(&Kind::KwElse) {
                self.pos += 1;
                Some(Box::new(self.block_body(&[Kind::KwEnd])?))
            } else if self.at(&Kind::KwElif) {
                self.pos += 1;
                Some(Box::new(self.elif_tail()?))
            } else {
                None
            };
            let end = self.expect(&Kind::KwEnd, "`end`")?.span;
            return Ok(Expr::BindingCondition {
                binders,
                source: Box::new(source),
                body: Box::new(body),
                loops: false,
                otherwise,
                span: Span::new(start.start, end.end),
            });
        }
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

    /// `if x <- g then` and `while (a,b) <- g do`. 1.0's condition here is a
    /// `GeneratorClause` (`DelimitedExpr.rats:37,39,40,216`), not an
    /// expression, so the decision needs LOOKAHEAD: a `<-` at depth zero before
    /// the closing keyword. Without it `if x <- g` reads `x < -g` and reports
    /// `expected then, found Lt`, which is what 27 corpus files were doing.
    ///
    /// The binder list is CONSUMED only when the whole shape is there.
    fn binding_condition_here(&mut self, closer: &Kind) -> Option<Vec<String>> {
        let mut depth = 0i32;
        let mut found = false;
        for index in self.pos..self.tokens.len() {
            match self.tokens.get(index).map(|t| &t.kind) {
                Some(Kind::LParen | Kind::LBracket | Kind::LGeneric | Kind::LBrace) => depth += 1,
                Some(Kind::RParen | Kind::RBracket | Kind::RGeneric | Kind::RBrace) => depth -= 1,
                // `<-` is two tokens joined by adjacency, read the way
                // `generator_clause` reads it.
                Some(Kind::Lt) if depth == 0 && self.glued_right(index) => {
                    if matches!(
                        self.tokens.get(index + 1).map(|t| &t.kind),
                        Some(Kind::Minus)
                    ) {
                        found = true;
                        break;
                    }
                }
                Some(Kind::LeftArrow) if depth == 0 => {
                    found = true;
                    break;
                }
                Some(k) if k == closer && depth == 0 => return None,
                Some(Kind::Eof) | None => return None,
                _ => {}
            }
        }
        if !found {
            return None;
        }
        let save = self.pos;
        let binders = self.comprehension_binders();
        if binders.is_empty() {
            self.pos = save;
            return None;
        }
        Some(binders)
    }

    /// The optional `then` of a binding condition, over a newline. Restored
    /// when it is absent, so a body on the next line is still a body.
    fn then_keyword(&mut self) {
        let save = self.pos;
        self.skip_newlines();
        if self.at(&Kind::KwThen) {
            self.pos += 1;
        } else {
            self.pos = save;
        }
    }

    /// What a binding condition draws from. The binder list and its `<-` are
    /// already consumed; this is the expression after the arrow.
    fn generator_source(&mut self, binders: &[String]) -> Parsed<Expr> {
        let _ = binders;
        self.skip_newlines();
        self.expr()
    }

    /// `elif` is `else if` without its own `end`, so the tail reuses the
    /// enclosing `end`.
    fn elif_tail(&mut self) -> Parsed<Expr> {
        self.skip_newlines();
        let start = self.span_here();
        if let Some(binders) = self.binding_condition_here(&Kind::KwThen) {
            let source = self.generator_source(&binders)?;
            self.then_keyword();
            let body = self.block_body(&[Kind::KwElse, Kind::KwElif, Kind::KwEnd])?;
            let otherwise = if self.at(&Kind::KwElse) {
                self.pos += 1;
                Some(Box::new(self.block_body(&[Kind::KwEnd])?))
            } else if self.at(&Kind::KwElif) {
                self.pos += 1;
                Some(Box::new(self.elif_tail()?))
            } else {
                None
            };
            return Ok(Expr::BindingCondition {
                binders,
                source: Box::new(source),
                body: Box::new(body),
                loops: false,
                otherwise,
                span: Span::new(start.start, self.span_here().end),
            });
        }
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
        self.do_group(false)
    }

    /// `Do ::= (DoFront also)* DoFront end`, with
    /// `DoFront ::= [at Expr] [atomic] do [BlockElems]` --
    /// `concrete-syntax.tex:1025-1028`.
    ///
    /// `first_atomic` is how `atomic do A also do B end` gets the rule right:
    /// the grammar puts `[atomic]` INSIDE a DoFront, so it covers only the
    /// first block and not the group. The atomic intercepts hand it in rather
    /// than wrapping the whole thing, which is what they would do if this were
    /// an ordinary block.
    fn do_group(&mut self, first_atomic: bool) -> Parsed<Expr> {
        let start = self.expect(&Kind::KwDo, "`do`")?.span;
        let terminators = [Kind::KwEnd, Kind::Reserved("also")];
        let first_start = self.span_here();
        let first_body = self.block_body(&terminators)?;
        let mut blocks = vec![Self::as_block(first_start, first_body)];
        if first_atomic {
            if let Some(first) = blocks.first_mut() {
                let span = first.span();
                *first = Expr::Atomic {
                    body: Box::new(first.clone()),
                    span,
                };
            }
        }
        while self.at(&Kind::Reserved("also")) {
            self.pos += 1;
            self.skip_newlines();
            if self.at(&Kind::Reserved("at")) {
                return Err(ParseError::AlsoFormUnsupported {
                    span: self.span_here(),
                    form: "an `at` region on an `also` block",
                });
            }
            let atomic = self.at(&Kind::Reserved("atomic"));
            if atomic {
                self.pos += 1;
                self.skip_newlines();
            }
            let front = self.span_here();
            self.expect(&Kind::KwDo, "`do` after `also`")?;
            let body = self.block_body(&terminators)?;
            let body = Self::as_block(front, body);
            blocks.push(if atomic {
                let span = body.span();
                Expr::Atomic {
                    body: Box::new(body),
                    span,
                }
            } else {
                body
            });
        }
        let end = self.expect(&Kind::KwEnd, "`end`")?.span;
        let span = Span::new(start.start, end.end);
        if blocks.len() == 1 {
            let Some(only) = blocks.pop() else {
                return Err(self.error("a block"));
            };
            return Ok(match only {
                Expr::Block { items, .. } => Expr::Block { items, span },
                other => other,
            });
        }
        Ok(Expr::AlsoDo { blocks, span })
    }

    /// A block body as a block, whatever `block_body` collapsed it to.
    fn as_block(start: Span, body: Expr) -> Expr {
        match body {
            block @ Expr::Block { .. } => block,
            other => {
                let span = Span::new(start.start, other.span().end);
                Expr::Block {
                    items: vec![BlockItem::Expr(other)],
                    span,
                }
            }
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
            // As in `atomic_expr`: the modifier belongs to the first DoFront,
            // not to the group.
            if self.at(&Kind::KwDo) {
                return Ok(BlockItem::Expr(self.do_group(true)?));
            }
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
        // `(a, b) = e`. THIS MUST BE TRIED BEFORE THE EXPRESSION PATH, and it
        // is the whole reason the node exists. `try_binding` above requires an
        // `Ident`, so a `(` falls straight through to `self.expr()` below and
        // `(min, max) = (i MIN j, i MAX j)` parses as INFIX EQUALITY -- a
        // discarded Boolean comparison. `tupleTest1.fss` and `tupleTest2.fss`
        // have no asserts and no `.test`, so they would compile, exit 0, do
        // nothing at all, and be counted as files gained.
        if let Some(binding) = self.try_tuple_binding()? {
            return Ok(BlockItem::TupleBinding(binding));
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
                if matches!(*callee, Expr::Var { .. }) && self.definition_equals_at(self.pos) {
                    return Err(ParseError::LocalFunctionDeclarationUnsupported { span });
                }
            }
            self.pos = probe;
            // AND THE SAME DECLARATION WITH ITS PARAMETERS TYPED, which the
            // probe above cannot see: it reads `f(x) = e` by parsing `f(x)` as
            // a CALL, and `f(w: ZZ32)` is not a call. 33 corpus files stopped
            // at that `:` reporting `expected )`, which names the punctuation
            // and not the feature.
            if let Some(span) = self.typed_local_function_here() {
                return Err(ParseError::LocalFunctionDeclarationUnsupported { span });
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

    /// The body of one arm. ONE block element, not a block: arms are separated
    /// by newlines and the next arm starts with an ordinary expression, so a
    /// run terminated by `end` would swallow every following arm. An assignment
    /// is not an expression, which is why this is `block_item` and not `expr` --
    /// `LessThan => t := tt.left` is what the corpus writes. Several statements
    /// need `do ... end`, which this reaches through `block_item` anyway.
    fn arm_body(&mut self) -> Parsed<Expr> {
        let start = self.span_here();
        let item = self.block_item()?;
        let end = self.previous_span();
        Ok(match item {
            BlockItem::Expr(e) => e,
            other => Expr::Block {
                items: vec![other],
                span: Span::new(start.start, end.end),
            },
        })
    }

    /// `fn (x: T): R => e`, and every shorter spelling the corpus writes:
    /// `fn (x) => e`, `fn x => e`, `fn () => e`, `fn (): R => e`.
    ///
    /// AN UNWRITTEN PARAMETER TYPE IS RECORDED, NOT GUESSED. It becomes the
    /// placeholder `$infer`, which cannot be lexed and so cannot collide with a
    /// declared type; closure lowering fills it from the arrow of the slot the
    /// lambda lands in, and refuses by name when there is no such slot. 540 of
    /// the corpus's 1064 `fn` uses carry no annotation at all, so refusing them
    /// in the parser would have refused the majority shape.
    ///
    /// More than one parameter is still refused: the arrow would be
    /// `(A, B) -> C`, a tuple domain, which needs composite types.
    fn lambda_expr(&mut self) -> Parsed<Expr> {
        let start = self.span_here();
        self.pos += 1; // `fn`
        self.skip_newlines();
        let params = if self.at(&Kind::LParen) {
            self.pos += 1;
            self.skip_newlines();
            let mut out = Vec::new();
            while !self.at(&Kind::RParen) {
                out.push(self.lambda_param()?);
                self.skip_newlines();
                if !self.at(&Kind::Comma) {
                    break;
                }
                self.pos += 1;
                self.skip_newlines();
            }
            self.expect(&Kind::RParen, "`)`")?;
            out
        } else {
            // `fn x => e`, 154 corpus sites. One binder, no parentheses.
            vec![self.lambda_param()?]
        };
        let return_type = if self.at(&Kind::Colon) {
            self.pos += 1;
            Some(self.type_ref()?)
        } else {
            None
        };
        self.skip_newlines();
        self.expect(&Kind::FatArrow, "`=>`")?;
        self.skip_newlines();
        let body = self.expr()?;
        let span = Span::new(start.start, body.span().end);
        Ok(Expr::Lambda {
            params,
            return_type,
            body: Box::new(body),
            span,
        })
    }

    /// One lambda parameter: `x: T`, or `x` with its type left to the slot.
    fn lambda_param(&mut self) -> Parsed<Param> {
        let (name, name_span) = self.identifier("a lambda parameter name")?;
        if !self.at(&Kind::Colon) {
            return Ok(Param {
                name,
                ty: TypeRef::Named {
                    name: INFER.to_owned(),
                    args: Vec::new(),
                    span: name_span,
                },
                // A lambda parameter is never varargs: `fn (xs...) => e` has no
                // corpus witness and the arrow it would land in has no spelling.
                varargs: false,
                mutable: false,
                span: name_span,
            });
        }
        self.pos += 1;
        let ty = self.type_ref()?;
        let span = Span::new(name_span.start, ty.span().end);
        Ok(Param {
            name,
            ty,
            varargs: false,
            mutable: false,
            span,
        })
    }

    /// `case subject of guard => e ... else => e end`.
    ///
    /// TWO FORMS ARE REFUSED BY NAME rather than mis-parsed. `case most > of`
    /// is the extremum form and `case z IN of` puts an operator between the
    /// subject and `of`; both replace the `=` the arms are matched with. The
    /// extremum word is checked before the subject is parsed, because `most`
    /// would otherwise be read as a name.
    fn case_expr(&mut self) -> Parsed<Expr> {
        let start = self.span_here();
        self.pos += 1; // `case`
        self.skip_newlines();
        if let Some(Kind::Reserved(word @ ("most" | "largest" | "smallest"))) = self.peek_kind() {
            let form = match *word {
                "most" => "the extremum form `case most`",
                "largest" => "the extremum form `case largest`",
                _ => "the extremum form `case smallest`",
            };
            return Err(ParseError::CaseFormUnsupported {
                span: self.span_here(),
                form,
            });
        }
        let subject = self.expr()?;
        self.skip_newlines();
        if !self.at_reserved("of") {
            return Err(ParseError::CaseFormUnsupported {
                span: self.span_here(),
                form: "an operator between the subject and `of`",
            });
        }
        self.pos += 1;

        let mut arms = Vec::new();
        let mut else_arm = None;
        self.skip_newlines();
        while !self.at(&Kind::KwEnd) && !self.at_eof() {
            if self.at(&Kind::KwElse) {
                self.pos += 1;
                self.expect(&Kind::FatArrow, "`=>`")?;
                self.skip_newlines();
                else_arm = Some(Box::new(self.block_body(&[Kind::KwEnd])?));
                break;
            }
            let arm_start = self.span_here();
            let guard = self.expr()?;
            self.expect(&Kind::FatArrow, "`=>`")?;
            self.skip_newlines();
            let body = self.arm_body()?;
            let end = self.previous_span();
            arms.push(CaseArm {
                guard,
                body,
                span: Span::new(arm_start.start, end.end),
            });
            self.skip_newlines();
        }
        let end = self.expect(&Kind::KwEnd, "`end`")?.span;
        Ok(Expr::Case {
            subject: Box::new(subject),
            arms,
            else_arm,
            span: Span::new(start.start, end.end),
        })
    }

    /// `typecase subject of T => e ... else => e end`, with `x: T => e` for the
    /// binder form. The two are told apart by the `:` after an identifier,
    /// which is the only thing that can follow a binder.
    /// `try B catch x A* forbid T* finally B end`, exactly
    /// `DelimitedExpr.rats:141-142`. Every clause after the body is optional
    /// and they come in that order.
    fn try_expr(&mut self) -> Parsed<Expr> {
        let start = self.span_here();
        self.pos += 1; // `try`
        self.skip_newlines();
        let body = Box::new(self.block_body(&[
            Kind::KwEnd,
            Kind::Reserved("catch"),
            Kind::Reserved("forbid"),
            Kind::Reserved("finally"),
        ])?);
        let mut catch_binder = None;
        let mut arms = Vec::new();
        if self.at_reserved("catch") {
            self.pos += 1;
            let (name, _) = self.identifier("the name `catch` binds")?;
            catch_binder = Some(name);
            self.skip_newlines();
            // `Type => expr`, the same shape a typecase arm has. A `catch` with
            // no matching arm RE-THROWS, so there is no `else` and this is not
            // a typecase: the exhaustiveness question does not arise.
            while !self.at(&Kind::KwEnd)
                && !self.at_reserved("forbid")
                && !self.at_reserved("finally")
                && !self.at_eof()
            {
                let arm_start = self.span_here();
                let ty = self.type_ref()?;
                self.expect(&Kind::FatArrow, "`=>`")?;
                self.skip_newlines();
                let body = self.arm_body()?;
                let end = self.previous_span();
                arms.push(TypeCaseArm {
                    binder: None,
                    ty,
                    body,
                    span: Span::new(arm_start.start, end.end),
                });
                self.skip_newlines();
            }
        }
        let mut forbids = Vec::new();
        if self.at_reserved("forbid") {
            self.pos += 1;
            self.skip_newlines();
            forbids.push(self.type_ref()?);
            while self.at(&Kind::Comma) {
                self.pos += 1;
                self.skip_newlines();
                forbids.push(self.type_ref()?);
            }
            self.skip_newlines();
        }
        let mut finally = None;
        if self.at_reserved("finally") {
            self.pos += 1;
            self.skip_newlines();
            finally = Some(Box::new(self.block_body(&[Kind::KwEnd])?));
        }
        let end = self.expect(&Kind::KwEnd, "`end`")?.span;
        Ok(Expr::Try {
            body,
            catch_binder,
            arms,
            forbids,
            finally,
            span: Span::new(start.start, end.end),
        })
    }

    fn typecase_expr(&mut self) -> Parsed<Expr> {
        let start = self.span_here();
        self.pos += 1; // `typecase`
        self.skip_newlines();
        let subject = self.expr()?;
        self.skip_newlines();
        if !self.at_reserved("of") {
            return Err(self.error("`of` after the typecase subject"));
        }
        self.pos += 1;

        let mut arms = Vec::new();
        let mut else_arm = None;
        self.skip_newlines();
        while !self.at(&Kind::KwEnd) && !self.at_eof() {
            if self.at(&Kind::KwElse) {
                self.pos += 1;
                self.expect(&Kind::FatArrow, "`=>`")?;
                self.skip_newlines();
                else_arm = Some(Box::new(self.block_body(&[Kind::KwEnd])?));
                break;
            }
            let arm_start = self.span_here();
            let binder = if matches!(self.peek_kind(), Some(Kind::Ident(_)))
                && matches!(self.peek_ahead(1), Some(Kind::Colon))
            {
                let (name, _) = self.identifier("a typecase binder")?;
                self.pos += 1; // `:`
                self.skip_newlines();
                Some(name)
            } else {
                None
            };
            let ty = self.type_ref()?;
            self.expect(&Kind::FatArrow, "`=>`")?;
            self.skip_newlines();
            let body = self.arm_body()?;
            let end = self.previous_span();
            arms.push(TypeCaseArm {
                binder,
                ty,
                body,
                span: Span::new(arm_start.start, end.end),
            });
            self.skip_newlines();
        }
        let end = self.expect(&Kind::KwEnd, "`end`")?.span;
        let Some(else_arm) = else_arm else {
            return Err(ParseError::UnexpectedToken {
                span: Span::new(start.start, end.end),
                expected: "an `else` arm; a typecase needs one because `comprises` \
                           is not enforced and cannot prove the arms exhaustive",
                found: "a typecase without one".to_owned(),
            });
        };
        Ok(Expr::TypeCase {
            subject: Box::new(subject),
            arms,
            else_arm,
            span: Span::new(start.start, end.end),
        })
    }

    /// `label L ... end L`. The trailing name is what the corpus writes and it
    /// has to agree with the opening one, or the reader is looking at a
    /// different block from the one the compiler is.
    fn label_expr(&mut self) -> Parsed<Expr> {
        let start = self.span_here();
        self.pos += 1; // `label`
        let (name, _) = self.identifier("a label name")?;
        let body = self.block_body(&[Kind::KwEnd])?;
        let end = self.expect(&Kind::KwEnd, "`end`")?.span;
        let end = match self.peek_kind() {
            Some(Kind::Ident(trailing)) if *trailing == name => {
                let span = self.span_here();
                self.pos += 1;
                span
            }
            Some(Kind::Ident(_)) => return Err(self.error("the same label name after `end`")),
            _ => end,
        };
        Ok(Expr::Label {
            name,
            body: Box::new(body),
            span: Span::new(start.start, end.end),
        })
    }

    /// `exit L with e`, `exit L`, and a bare `exit`, which names the innermost
    /// enclosing label.
    fn exit_expr(&mut self) -> Parsed<Expr> {
        let start = self.span_here();
        self.pos += 1; // `exit`
        let name = match self.peek_kind() {
            Some(Kind::Ident(_)) => Some(self.identifier("a label name")?.0),
            _ => None,
        };
        let value = if self.at_reserved("with") {
            self.pos += 1;
            self.skip_newlines();
            Some(Box::new(self.expr()?))
        } else {
            None
        };
        let end = self.previous_span();
        Ok(Expr::Exit {
            name,
            value,
            span: Span::new(start.start, end.end),
        })
    }

    /// `atomic <expr>`, for the operand positions `block_item` never reaches.
    fn atomic_expr(&mut self) -> Parsed<Expr> {
        let start = self.span_here();
        self.pos += 1;
        self.skip_newlines();
        // `atomic do A also do B end`: the modifier is part of a DoFront, so it
        // covers A alone. Wrapping what the group parses to would make the
        // whole group atomic, which is a DIFFERENT program -- and one that
        // serialised execution cannot tell apart, because both readings print
        // the same thing. `do_group` takes the flag instead.
        if self.at(&Kind::KwDo) {
            return self.do_group(true);
        }
        let body = self.expr()?;
        let span = Span::new(start.start, body.span().end);
        Ok(Expr::Atomic {
            body: Box::new(body),
            span,
        })
    }

    /// `spawn do ... end` and `spawn f(x)`. The corpus writes both --
    /// `Spawn1.fss:17` and `Spawn5.fss:22` -- and both are one expression, so
    /// there is nothing to separate here the way `atomic` separates its
    /// `do`-group form.
    fn spawn_expr(&mut self) -> Parsed<Expr> {
        let start = self.span_here();
        self.pos += 1;
        self.skip_newlines();
        let body = self.expr()?;
        let span = Span::new(start.start, body.span().end);
        Ok(Expr::Spawn {
            body: Box::new(body),
            span,
        })
    }

    /// `(a, b) = e`, and nothing else: every name is a bare identifier, there
    /// are at least two, and the closing paren is followed by a definition `=`.
    ///
    /// TOKEN-GUARDED AND BACKTRACKING, like the `f(x) = e` probe below it. A
    /// tuple EXPRESSION in statement position is legal (`(a, b)` alone), and so
    /// is an equality between two of them, so nothing here may commit until the
    /// whole shape has been seen.
    ///
    /// A NESTED BINDER `((a, b), c)` IS NOT IN THE SUBSET and falls out of this
    /// shape rather than being refused: no corpus file writes one -- measured
    /// over all 1956 -- so it stays an ordinary expression and gets the
    /// diagnostic that position already had.
    fn try_tuple_binding(&mut self) -> Parsed<Option<fortress_ast::TupleBinding>> {
        let save = self.pos;
        let start = self.span_here();
        if !self.at(&Kind::LParen) {
            return Ok(None);
        }
        self.pos += 1;
        let mut names = Vec::new();
        loop {
            let Some(Kind::Ident(name)) = self.peek_kind() else {
                self.pos = save;
                return Ok(None);
            };
            names.push((*name).to_owned());
            self.pos += 1;
            if self.at(&Kind::Comma) {
                self.pos += 1;
                continue;
            }
            break;
        }
        if names.len() < 2 || !self.at(&Kind::RParen) {
            self.pos = save;
            return Ok(None);
        }
        self.pos += 1;
        if !self.definition_equals_at(self.pos) {
            self.pos = save;
            return Ok(None);
        }
        self.pos += 1;
        self.skip_newlines();
        let value = self.expr()?;
        let span = Span::new(start.start, value.span().end);
        Ok(Some(fortress_ast::TupleBinding { names, value, span }))
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
            Some(Kind::Eq) if self.definition_equals_at(self.pos) => modifier,
            Some(Kind::ColonEq) if ty.is_some() => true,
            // `var x: ZZ32` on its own line. `variables.tex:176-179` gives a
            // local this form and 58 corpus files write it -- the largest
            // single first-blocker in the corpus -- so it is refused BY NAME
            // rather than left to fall through to `expected an expression`.
            // Nothing else this could be: a `var` and a written type with no
            // `=` and no `:=` matches no other production.
            _ if modifier && ty.is_some() => {
                return Err(ParseError::DelayedInitializationUnsupported {
                    span: name_span,
                    name,
                })
            }
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

/// The five outcomes of `opr-fixity.tex`'s table. `Lopsided` and `Nofix` are
/// the rows the specification calls a STATIC ERROR: it names a recommended
/// reading for each so a parse can continue looking for further errors, and
/// this parser stops at the first error, so they are refusals.
/// The three operator words with REAL CODEGEN, excluded from the prefix arm.
/// `AND` and `OR` are `BinOp::And`/`BinOp::Or` and `NOT` is `UnOp::Not` and is
/// taken by `unary` a few lines above; routing any of them through a call to a
/// function nobody declared would break every program that uses them. The same
/// carve-out `infix_added_operator` makes, for the same reason.
///
/// A NAMED CONST AND NOT A `matches!`, because a mutation row splits on
/// `IFS='|'` and a match alternative cannot appear in a line a table has to
/// reach.
const CODEGEN_OPERATOR_WORDS: [&str; 3] = ["AND", "OR", "NOT"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TableFixity {
    Infix,
    Prefix,
    Postfix,
    /// Whitespace on one side and not the other.
    Lopsided,
    Nofix,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeftContext {
    PrimaryTail,
    Operator,
    /// A comma, a semicolon, a left encloser, a line break, or the start.
    Delimiter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RightContext {
    PrimaryFront,
    Operator,
    /// A comma, a semicolon, or a right encloser.
    Delimiter,
    LineBreak,
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
        // No source spells these; they exist for a BIG reduction's fold.
        BinOp::Max => "MAX",
        BinOp::Min => "MIN",
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
        // `===` IS NOT `=`. It used to map here, which read `a === b` as
        // numeric equality -- and `===` is an ORDINARY LIBRARY OPERATOR:
        // `Library/CompilerLibrary.fsi:30` declares `opr ===(a:Any, b:Any):
        // Boolean` and `.fss:63` defines it as `jSEQUIV`, reference identity,
        // with a separate `ZZ64` overload that IS `a = b`. Reading it as `=`
        // gets the numeric case right by luck and the reference case wrong by
        // construction. It goes to the overload set now, like `||`.
        //
        // MEASURED BEFORE THE RECLASSIFICATION, which is this project's rule:
        // ZERO of the files that compile today write a `===`.
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
