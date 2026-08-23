//! Shared vocabulary. Types only, no logic, so that `parser` and `codegen`
//! never need to depend on each other.

mod nodes;

pub use nodes::{
    Accessor, Assign, BinOp, Binding, BlockItem, BoundObligation, CaseArm, Component, CutMember,
    Decl, DimDecl, DimExpr, Expr, ExtentForm, ExtentRange, FieldDecl, Fixity, FnDecl,
    GeneratorClause, ImportDecl, ImportItems, ImportedName, Member, MethodDecl, Modifiers,
    ObjectDecl, Param, ShapeSpelling, StaticExpr, StaticKind, StaticOp, StaticParam, TraitDecl,
    TupleBinding, TypeCaseArm, TypeRef, UnOp, UnitDecl, ValueDecl,
};

/// The type the parser gives a `self` parameter, standing for the enclosing
/// trait or object until the checker substitutes it.
///
/// IT IS UNWRITABLE ON PURPOSE, and the `$` is what makes it so: no identifier
/// lexes one. It used to be the bare name `Self`, which was safe only while
/// `Self` was refused in every type position -- and `Self` IS A NAME in 1.0
/// (`Type.rats:499`, `SelfTypeId` feeds `makeVarType`, the same node an `Id`
/// produces). The moment a static parameter may be CALLED `Self`, a bare
/// placeholder is substituted by monomorphization along with it: a functional
/// method on `trait Equality[\Self\]` would take `ZZ32` as its receiver where
/// it must take `Equality[\ZZ32\]`.
pub const SELF_TYPE_PLACEHOLDER: &str = "$Self";

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
