use fortress_ast::Span;

use crate::types::Type;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeError {
    /// The locked M1 rule, stated as its own variant so the diagnostic can name
    /// the fix. A value is never implicitly converted; `widen` is explicit.
    ImplicitWideningRejected {
        span: Span,
        from: Type,
        to: Type,
    },
    Mismatch {
        span: Span,
        found: Type,
        required: Type,
    },
    /// A juxtaposition whose operands are neither uniformly numeric nor
    /// involving a string.
    UnresolvableJuxtaposition {
        span: Span,
        left: Type,
        right: Type,
    },
    /// Numeric juxtaposition or arithmetic across two different numeric types.
    /// Separate from `Mismatch` because neither side is "the required" one.
    MixedNumericOperands {
        span: Span,
        left: Type,
        right: Type,
    },
    UnknownName {
        span: Span,
        name: String,
    },
    UnknownType {
        span: Span,
        name: String,
    },
    ArityMismatch {
        span: Span,
        name: String,
        expected: usize,
        found: usize,
    },
    LiteralOutOfRange {
        span: Span,
        ty: Type,
    },
    /// An integer literal in a slot that wants something other than ZZ32/ZZ64.
    LiteralNotApplicable {
        span: Span,
        required: Type,
    },
    ConditionNotBoolean {
        span: Span,
        found: Type,
    },
    BranchTypeMismatch {
        span: Span,
        then_type: Type,
        else_type: Type,
    },
    /// An `if` used as a value must have an `else`.
    MissingElseBranch {
        span: Span,
    },
    DuplicateDefinition {
        span: Span,
        name: String,
    },
    NotAnArray {
        span: Span,
        found: Type,
    },
    /// `array(8)` or `[]` with nothing to say what it holds.
    ElementTypeUnknown {
        span: Span,
    },
    /// `Array[\Array[\ZZ64\]\]`, or `Array` with no argument at all.
    UnsupportedElementType {
        span: Span,
        name: String,
    },
    AssignToImmutable {
        span: Span,
        name: String,
    },
    AssignToUndeclared {
        span: Span,
        name: String,
    },
    InvalidAssignTarget {
        span: Span,
    },

    // ------------------------------------------------------------------ M3c
    /// An `api` parses, so the corpus metric can move, but there is nothing to
    /// emit for a file of signatures.
    ApiNotExecutable {
        span: Span,
    },
    MissingBody {
        span: Span,
        name: String,
    },
    /// `trait A extends B` and `trait B extends A`. The transitive closure has
    /// to terminate, and a cycle is a fact about the program, not a hang.
    TraitCycle {
        span: Span,
        name: String,
    },
    NotATrait {
        span: Span,
        name: String,
    },
    UnknownField {
        span: Span,
        found: Type,
        name: String,
    },
    /// `x.f(y)`. Dotted and functional methods have separate namespaces in the
    /// specification, so `x.f(y)` is not `f(x, y)` and will not be desugared
    /// into it.
    DottedMethodUnsupported {
        span: Span,
        name: String,
    },
    MutableFieldUnsupported {
        span: Span,
        name: String,
    },
    FieldNeedsInitializer {
        span: Span,
        name: String,
    },
    SingletonNotConstructible {
        span: Span,
        name: String,
    },
    /// A singleton's fields are computed once, in declaration order, before
    /// `run`. Letting one reach another singleton or a user function would put
    /// a null dereference one forward reference away.
    SingletonInitializerRestricted {
        span: Span,
        name: String,
    },
    NoApplicableDeclaration {
        span: Span,
        name: String,
        arguments: String,
    },
    /// The deliberate deviation from specification 1.0, which would choose one
    /// of the maximal declarations arbitrarily. An arbitrary winner is a
    /// silently wrong answer.
    AmbiguousDispatch {
        span: Span,
        name: String,
        arguments: String,
        first: Span,
        second: Span,
    },
    ReturnTypeNotCovariant {
        span: Span,
        name: String,
        arguments: String,
        found: Type,
        required: Type,
    },
    DispatchTableTooLarge {
        span: Span,
        name: String,
        cells: usize,
    },
    NotPrintable {
        span: Span,
        found: Type,
    },
}

impl TypeError {
    #[must_use]
    pub const fn span(&self) -> Span {
        match self {
            Self::ImplicitWideningRejected { span, .. }
            | Self::Mismatch { span, .. }
            | Self::UnresolvableJuxtaposition { span, .. }
            | Self::MixedNumericOperands { span, .. }
            | Self::UnknownName { span, .. }
            | Self::UnknownType { span, .. }
            | Self::ArityMismatch { span, .. }
            | Self::LiteralOutOfRange { span, .. }
            | Self::LiteralNotApplicable { span, .. }
            | Self::ConditionNotBoolean { span, .. }
            | Self::BranchTypeMismatch { span, .. }
            | Self::MissingElseBranch { span }
            | Self::DuplicateDefinition { span, .. }
            | Self::NotAnArray { span, .. }
            | Self::ElementTypeUnknown { span }
            | Self::UnsupportedElementType { span, .. }
            | Self::AssignToImmutable { span, .. }
            | Self::AssignToUndeclared { span, .. }
            | Self::InvalidAssignTarget { span }
            | Self::ApiNotExecutable { span }
            | Self::MissingBody { span, .. }
            | Self::TraitCycle { span, .. }
            | Self::NotATrait { span, .. }
            | Self::UnknownField { span, .. }
            | Self::DottedMethodUnsupported { span, .. }
            | Self::MutableFieldUnsupported { span, .. }
            | Self::FieldNeedsInitializer { span, .. }
            | Self::SingletonNotConstructible { span, .. }
            | Self::SingletonInitializerRestricted { span, .. }
            | Self::NoApplicableDeclaration { span, .. }
            | Self::AmbiguousDispatch { span, .. }
            | Self::ReturnTypeNotCovariant { span, .. }
            | Self::DispatchTableTooLarge { span, .. }
            | Self::NotPrintable { span, .. } => *span,
        }
    }
}

impl core::fmt::Display for TypeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let span = self.span();
        write!(f, "{}..{}: ", span.start, span.end)?;
        match self {
            Self::ImplicitWideningRejected { from, to, .. } => write!(
                f,
                "a {} value is not implicitly converted to {}; write `widen(...)`",
                from.name(),
                to.name()
            ),
            Self::Mismatch {
                found, required, ..
            } => {
                write!(f, "expected {}, found {}", required.name(), found.name())
            }
            Self::UnresolvableJuxtaposition { left, right, .. } => write!(
                f,
                "juxtaposition of {} and {} is neither multiplication nor concatenation",
                left.name(),
                right.name()
            ),
            Self::MixedNumericOperands { left, right, .. } => write!(
                f,
                "operands are {} and {}; Fortress does not mix numeric types implicitly",
                left.name(),
                right.name()
            ),
            Self::UnknownName { name, .. } => write!(f, "unknown name `{name}`"),
            Self::UnknownType { name, .. } => write!(f, "unknown type `{name}`"),
            Self::ArityMismatch {
                name,
                expected,
                found,
                ..
            } => {
                write!(f, "`{name}` takes {expected} argument(s), found {found}")
            }
            Self::LiteralOutOfRange { ty, .. } => {
                write!(f, "integer literal does not fit in {}", ty.name())
            }
            Self::LiteralNotApplicable { required, .. } => {
                write!(
                    f,
                    "an integer literal cannot be used where {} is required",
                    required.name()
                )
            }
            Self::ConditionNotBoolean { found, .. } => {
                write!(f, "condition must be Boolean, found {}", found.name())
            }
            Self::BranchTypeMismatch {
                then_type,
                else_type,
                ..
            } => write!(
                f,
                "branches disagree: then is {}, else is {}",
                then_type.name(),
                else_type.name()
            ),
            Self::MissingElseBranch { .. } => {
                write!(f, "an `if` used as a value needs an `else` branch")
            }
            Self::DuplicateDefinition { name, .. } => write!(f, "`{name}` is defined twice"),
            Self::NotAnArray { found, .. } => {
                write!(f, "expected an array, found {}", found.name())
            }
            Self::ElementTypeUnknown { .. } => write!(
                f,
                "nothing here says what this array holds; annotate the binding, as in `a:Array[\\ZZ64\\] = ...`"
            ),
            Self::UnsupportedElementType { name, .. } => write!(
                f,
                "`{name}` is not a supported array element type; arrays are one dimensional and hold a scalar"
            ),
            Self::AssignToImmutable { name, .. } => write!(
                f,
                "`{name}` is immutable; declare it with `:=` to assign to it"
            ),
            Self::AssignToUndeclared { name, .. } => write!(
                f,
                "`{name}` is not declared; write `{name}:T := ...` to declare it"
            ),
            Self::InvalidAssignTarget { .. } => {
                write!(f, "only a variable or an array element can be assigned to")
            }
            Self::ApiNotExecutable { .. } => write!(
                f,
                "an `api` is a set of signatures with no bodies; there is nothing to compile"
            ),
            Self::MissingBody { name, .. } => {
                write!(f, "`{name}` has no body; write `{name}(...) = ...`")
            }
            Self::TraitCycle { name, .. } => {
                write!(f, "`{name}` extends itself, directly or through another trait")
            }
            Self::NotATrait { name, .. } => {
                write!(f, "`{name}` is not a trait, so nothing can extend it")
            }
            Self::UnknownField { found, name, .. } => {
                write!(f, "{} has no field `{name}`", found.name())
            }
            Self::DottedMethodUnsupported { name, .. } => write!(
                f,
                "dotted method `.{name}` is parsed but not implemented; \
                 it is not the same declaration as a function `{name}`"
            ),
            Self::MutableFieldUnsupported { name, .. } => {
                write!(f, "`var {name}`: mutable fields are not implemented")
            }
            Self::FieldNeedsInitializer { name, .. } => write!(
                f,
                "field `{name}` is not a constructor parameter, so it needs `= ...`"
            ),
            Self::SingletonNotConstructible { name, .. } => write!(
                f,
                "`{name}` is a singleton object; write `{name}`, not `{name}(...)`"
            ),
            Self::SingletonInitializerRestricted { name, .. } => write!(
                f,
                "a singleton's fields are computed before `run`, so this one may not \
                 reach `{name}`"
            ),
            Self::NoApplicableDeclaration {
                name, arguments, ..
            } => write!(f, "no declaration of `{name}` applies to ({arguments})"),
            Self::AmbiguousDispatch {
                name,
                arguments,
                first,
                second,
                ..
            } => write!(
                f,
                "`{name}` is ambiguous for ({arguments}): the declarations at {}..{} and {}..{} \
                 are both most specific, and neither is more specific than the other",
                first.start, first.end, second.start, second.end
            ),
            Self::ReturnTypeNotCovariant {
                name,
                arguments,
                found,
                required,
                ..
            } => write!(
                f,
                "`{name}` for ({arguments}) returns {}, which is not a {}",
                found.name(),
                required.name()
            ),
            Self::DispatchTableTooLarge { name, cells, .. } => write!(
                f,
                "the dispatch table for `{name}` would have {cells} cells; \
                 narrow the parameter types"
            ),
            Self::NotPrintable { found, .. } => {
                write!(f, "`println` does not accept {}", found.name())
            }
        }
    }
}

impl std::error::Error for TypeError {}
