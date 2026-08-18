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
            | Self::InvalidAssignTarget { span } => *span,
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
        }
    }
}

impl std::error::Error for TypeError {}
