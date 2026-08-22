//! Shared vocabulary. Types only, no logic, so that `parser` and `codegen`
//! never need to depend on each other.

mod nodes;

pub use nodes::{
    Assign, BinOp, Binding, BlockItem, BoundObligation, CaseArm, Component, Decl, DimDecl, DimExpr,
    Expr, ExtentForm, ExtentRange, FieldDecl, Fixity, FnDecl, ImportDecl, ImportItems,
    ImportedName, Member, MethodDecl, Modifiers, ObjectDecl, Param, ShapeSpelling, StaticExpr,
    StaticKind, StaticOp, StaticParam, TraitDecl, TypeCaseArm, TypeRef, UnOp, UnitDecl,
};

/// Byte offsets into the source. Line and column are derived on demand by the
/// diagnostic renderer rather than carried on every token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    #[must_use]
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.start >= self.end
    }
}
