//! The numeric tower and static overload resolution.
//!
//! Two rules that must stay distinct: values are never implicitly converted,
//! and literals are unfixed until context pins them.

use fortress_ast::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeError {
    /// A value of one type used where another is required. Never resolved by
    /// an implicit conversion; `widen` must be explicit.
    Mismatch { span: Span, found: String, required: String },
    /// Juxtaposition whose operands are neither both numeric nor both textual.
    UnresolvableJuxtaposition { span: Span, left: String, right: String },
    UnknownName { span: Span, name: String },
    NoMatchingOverload { span: Span, name: String },
}
