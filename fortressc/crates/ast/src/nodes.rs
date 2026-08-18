//! The M1 AST. Types only: `parser` builds these and `codegen` consumes them,
//! so neither depends on the other.

use crate::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component {
    pub name: String,
    pub exports: Vec<String>,
    pub decls: Vec<Decl>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decl {
    Function(FnDecl),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<TypeRef>,
    pub body: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub name: String,
    pub ty: TypeRef,
    pub span: Span,
}

/// Types are bare names (`ZZ32`, `ZZ64`, `RR64`) or a name with one static
/// argument (`Array[\ZZ64\]`). Resolution happens in the types crate; the
/// parser only records what was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeRef {
    pub name: String,
    pub argument: Option<Box<TypeRef>>,
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
            | Self::While { span, .. } => *span,
        }
    }
}
