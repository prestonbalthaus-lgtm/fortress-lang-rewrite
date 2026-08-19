//! The M1 AST. Types only: `parser` builds these and `codegen` consumes them,
//! so neither depends on the other.

use crate::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component {
    pub name: String,
    pub exports: Vec<String>,
    /// Recorded and not read. Whole-program monomorphization has no separate
    /// compilation, so there is nothing for an import to resolve against yet.
    pub imports: Vec<ImportDecl>,
    pub decls: Vec<Decl>,
    /// Empty until monomorphization has run.
    pub bounds: Vec<BoundObligation>,
    /// `api Foo ... end` rather than `component Foo ... end`. Parsed so the
    /// corpus metric can move; an api has no bodies and is not executable, so
    /// the type checker refuses it rather than pretending to compile one.
    pub is_api: bool,
    pub span: Span,
}

/// `import Foo.Bar.{...}`. The name is kept; the brace group and any `except`
/// clause are consumed without being interpreted, because aliasing an operator
/// (`opr OPLUS => MYPLUS`) needs a precedence map that does not exist yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportDecl {
    pub api_name: String,
    /// `import api Foo` rather than `import Foo.{...}`.
    pub is_api: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decl {
    Function(FnDecl),
    Trait(TraitDecl),
    Object(ObjectDecl),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnDecl {
    pub name: String,
    pub static_params: Vec<StaticParam>,
    pub params: Vec<Param>,
    pub return_type: Option<TypeRef>,
    /// `None` only inside an `api`, where a declaration is a signature.
    pub body: Option<Expr>,
    pub span: Span,
}

/// `trait T extends {A, B} comprises {...} excludes {...} ... end`.
/// Only `extends` is checked; the rest is recorded and ignored, because
/// exclusion is decided extensionally from the concrete types in the program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraitDecl {
    pub name: String,
    pub static_params: Vec<StaticParam>,
    pub extends: Vec<TypeRef>,
    pub comprises: Vec<TypeRef>,
    pub excludes: Vec<TypeRef>,
    pub members: Vec<Member>,
    pub span: Span,
}

/// `object O(x: T) extends {A} ... end`, or without the parentheses, a
/// singleton: one instance, built once before `run`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectDecl {
    pub name: String,
    pub static_params: Vec<StaticParam>,
    /// `None` is a singleton. `Some(vec![])` is `object O() ... end`.
    pub params: Option<Vec<Param>>,
    pub extends: Vec<TypeRef>,
    pub members: Vec<Member>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Member {
    Field(FieldDecl),
    /// A dotted method. Parsed, never checked: the specification gives dotted
    /// and functional methods separate namespaces, and desugaring one into the
    /// other would have to be unbuilt later.
    Method(MethodDecl),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDecl {
    pub name: String,
    pub ty: TypeRef,
    pub init: Option<Expr>,
    pub mutable: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodDecl {
    pub name: String,
    pub static_params: Vec<StaticParam>,
    pub params: Vec<Param>,
    pub return_type: Option<TypeRef>,
    pub body: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub name: String,
    pub ty: TypeRef,
    pub span: Span,
}

/// Types are bare names (`ZZ32`), a name applied to static arguments
/// (`Map[\ZZ64, List[\String\]\]`), the unit type `()`, a tuple of two or more,
/// or an arrow. Resolution happens in the types crate; the parser only records
/// what was written. After monomorphization no `TypeRef` in a component has
/// static arguments -- expansion rewrites every one to a ground name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeRef {
    Named {
        name: String,
        args: Vec<TypeRef>,
        span: Span,
    },
    /// `()`. The specification's special type, pronounced void; not a tuple.
    Unit { span: Span },
    /// Two or more, by construction. A one-element parenthesised list is
    /// unwrapped by the parser and can never arrive here.
    Tuple { elems: Vec<TypeRef>, span: Span },
    /// `A -> B`, right associative. Parsed, never resolved: this subset has no
    /// function values, so an arrow type is uninhabited.
    Arrow {
        from: Box<TypeRef>,
        to: Box<TypeRef>,
        span: Span,
    },
}

impl TypeRef {
    #[must_use]
    pub const fn span(&self) -> Span {
        match self {
            Self::Named { span, .. }
            | Self::Unit { span }
            | Self::Tuple { span, .. }
            | Self::Arrow { span, .. } => *span,
        }
    }

    /// The type as the user wrote it, for diagnostics.
    #[must_use]
    pub fn written(&self) -> String {
        match self {
            Self::Named { name, args, .. } if args.is_empty() => name.clone(),
            Self::Named { name, args, .. } => {
                let inner: Vec<String> = args.iter().map(Self::written).collect();
                format!("{name}[\\{}\\]", inner.join(", "))
            }
            Self::Unit { .. } => "()".to_owned(),
            Self::Tuple { elems, .. } => {
                let inner: Vec<String> = elems.iter().map(Self::written).collect();
                format!("({})", inner.join(", "))
            }
            Self::Arrow { from, to, .. } => format!("{} -> {}", from.written(), to.written()),
        }
    }
}

/// `[\T extends {Foo, Bar}\]`. Type parameters only: `nat`, `int`, `bool`,
/// `opr`, `unit` and `dim` are refused by the parser, because mixing static
/// integers with type parameters is a dependent type system and this is not one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticParam {
    pub name: String,
    pub bounds: Vec<TypeRef>,
    pub span: Span,
}

/// Recorded by monomorphization, discharged by the type checker. A bound cannot
/// be checked while it is being substituted -- subtyping needs the registry, and
/// the registry is built from the ground component expansion produces -- so the
/// obligation is carried across the phase boundary instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundObligation {
    pub subject: TypeRef,
    pub bound: TypeRef,
    /// The static parameter this came from, for the diagnostic.
    pub parameter: String,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    Ne,
}

/// Decided from byte-span adjacency, not from a token. Tight juxtaposition
/// binds tighter than tight `/`, which binds tighter than loose juxtaposition,
/// which binds tighter than any loose infix operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fixity {
    /// No whitespace on either side: `x-1`.
    Tight,
    /// Whitespace on both sides: `x - 1`.
    Loose,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Pos,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    /// Digits with group separators already removed. Arbitrary precision at
    /// this stage: the types crate decides `ZZ32` versus `ZZ64`.
    IntLit {
        digits: String,
        span: Span,
    },
    FloatLit {
        int_digits: String,
        frac_digits: String,
        span: Span,
    },
    StrLit {
        value: String,
        span: Span,
    },
    BoolLit {
        value: bool,
        span: Span,
    },
    Var {
        name: String,
        span: Span,
    },

    /// An unresolved run of juxtaposed operands. Whether this is multiplication
    /// or string concatenation depends on operand types, so it stays flat until
    /// the types crate folds it. The reference implementation does the same.
    Juxt {
        items: Vec<Expr>,
        span: Span,
    },

    Infix {
        op: BinOp,
        fixity: Fixity,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        span: Span,
    },
    Prefix {
        op: UnOp,
        operand: Box<Expr>,
        span: Span,
    },
    /// A tight application: the `(` is glued to the callee.
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
        span: Span,
    },
    If {
        cond: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Option<Box<Expr>>,
        span: Span,
    },
    Block {
        items: Vec<BlockItem>,
        span: Span,
    },
    /// `[1, 2, 3]`. Homogeneous and one dimensional; the element type comes
    /// from the elements, or from context when the literal is empty.
    ArrayLit {
        items: Vec<Expr>,
        span: Span,
    },
    /// `a[i]`, a tight subscript. Spaced, `a [i]` is a juxtaposition, exactly
    /// as `f (x)` is.
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },
    While {
        cond: Box<Expr>,
        body: Box<Expr>,
        span: Span,
    },
    /// `x.f`. A field read, or -- under a glued `(` -- the receiver of a
    /// dotted method call, which the checker refuses.
    Field {
        base: Box<Expr>,
        name: String,
        span: Span,
    },
    /// `f[\ZZ64\]` in expression position. Monomorphization rewrites this to a
    /// plain `Var` naming the instantiation, so nothing downstream sees it.
    Instantiate {
        callee: Box<Expr>,
        args: Vec<TypeRef>,
        span: Span,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockItem {
    Binding(Binding),
    Assign(Assign),
    Expr(Expr),
}

/// `x := e` or `a[i] := e`. The target is checked in the types crate, which is
/// where a good diagnostic can be written for `f(x) := 1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assign {
    pub target: Expr,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub name: String,
    pub ty: Option<TypeRef>,
    pub value: Expr,
    /// `:=` rather than `=`. A mutable binding is the only thing assignment can
    /// target, and the only thing codegen puts in an `alloca`.
    pub mutable: bool,
    pub span: Span,
}

impl Expr {
    #[must_use]
    pub const fn span(&self) -> Span {
        match self {
            Self::IntLit { span, .. }
            | Self::FloatLit { span, .. }
            | Self::StrLit { span, .. }
            | Self::BoolLit { span, .. }
            | Self::Var { span, .. }
            | Self::Juxt { span, .. }
            | Self::Infix { span, .. }
            | Self::Prefix { span, .. }
            | Self::Call { span, .. }
            | Self::If { span, .. }
            | Self::Block { span, .. }
            | Self::ArrayLit { span, .. }
            | Self::Index { span, .. }
            | Self::While { span, .. }
            | Self::Field { span, .. }
            | Self::Instantiate { span, .. } => *span,
        }
    }
}
