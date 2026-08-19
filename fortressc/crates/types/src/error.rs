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
    /// A juxtaposition led by a function element with more than two elements.
    /// The specification's reassociation rules (`juxtameaning.tex:70-111`) are
    /// not implemented, and were measured at zero corpus files, so this refuses
    /// rather than guesses.
    JuxtapositionNotBinary {
        span: Span,
        found: usize,
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
    /// A type form the parser accepts and this subset does not implement.
    /// `form` names it: "a tuple type", "an arrow type", "a tuple expression".
    TypeNotImplemented {
        span: Span,
        form: &'static str,
    },
    /// `Type::Void` has no representation -- `basic_type` maps it to `None` --
    /// so a position that has to store a value cannot hold one. Reaching
    /// codegen with one is malformed IR, which is exit 70 rather than a
    /// diagnostic, so it is refused here.
    VoidNotStorable {
        span: Span,
        position: &'static str,
    },
    /// Codegen's generated `main` calls `run` with no arguments, so a `run`
    /// that declares any is a module LLVM rejects. 1.0 gives the entry point an
    /// optional `String...` parameter; this subset does not have varargs.
    EntryPointTakesArguments {
        span: Span,
        found: usize,
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
    /// A component-level value binding. It parses, so the metric counts the
    /// files, but it is not a nullary function: its initializer runs at
    /// component initialization, and there is no component initialization yet.
    ValueBindingUnsupported {
        span: Span,
        name: String,
    },
    /// A `getter`/`setter` member read as a field. It parses; it is not read.
    AccessorUnsupported {
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

    // ------------------------------------------------------------------ M3d
    /// A generic named without its static arguments. They are written, never
    /// inferred: that is what makes instantiation demand syntactic, which is
    /// what lets expansion run before the checker.
    StaticArgumentsRequired {
        span: Span,
        name: String,
    },
    NotGeneric {
        span: Span,
        name: String,
    },
    StaticArgumentCountMismatch {
        span: Span,
        name: String,
        expected: usize,
        found: usize,
    },
    /// The total ceiling. Depth and type size are both insufficient on their
    /// own -- an acyclic graph of wrappers is exponential with every type small.
    TooManyInstantiations {
        span: Span,
        name: String,
        limit: usize,
    },
    /// Specification 1.0's static-parameter uniformity rule, enforced.
    OverloadSetStaticParamsDiffer {
        span: Span,
        name: String,
        first: Span,
    },
    /// Recorded by monomorphization, discharged here once the registry exists.
    BoundNotSatisfied {
        span: Span,
        parameter: String,
        subject: Type,
        bound: Type,
    },
    /// `array(n)` cannot hand out a block of pointers nothing has written: the
    /// runtime's fill is a one-byte empty string, and dispatch would read a tag
    /// four bytes into it.
    UninitialisedArrayOfReferences {
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
            | Self::JuxtapositionNotBinary { span, .. }
            | Self::MixedNumericOperands { span, .. }
            | Self::UnknownName { span, .. }
            | Self::UnknownType { span, .. }
            | Self::TypeNotImplemented { span, .. }
            | Self::VoidNotStorable { span, .. }
            | Self::EntryPointTakesArguments { span, .. }
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
            | Self::NotPrintable { span, .. }
            | Self::AccessorUnsupported { span, .. }
            | Self::ValueBindingUnsupported { span, .. }
            | Self::StaticArgumentsRequired { span, .. }
            | Self::NotGeneric { span, .. }
            | Self::StaticArgumentCountMismatch { span, .. }
            | Self::TooManyInstantiations { span, .. }
            | Self::OverloadSetStaticParamsDiffer { span, .. }
            | Self::BoundNotSatisfied { span, .. }
            | Self::UninitialisedArrayOfReferences { span, .. } => *span,
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
            Self::JuxtapositionNotBinary { found, .. } => write!(
                f,
                "a juxtaposition of {found} elements led by a function is not implemented; \
                 parenthesise the application"
            ),
            Self::MixedNumericOperands { left, right, .. } => write!(
                f,
                "operands are {} and {}; Fortress does not mix numeric types implicitly",
                left.name(),
                right.name()
            ),
            Self::UnknownName { name, .. } => write!(f, "unknown name `{name}`"),
            Self::UnknownType { name, .. } => write!(f, "unknown type `{name}`"),
            Self::TypeNotImplemented { form, .. } => {
                write!(f, "{form} is not implemented in this subset")
            }
            Self::VoidNotStorable { position, .. } => {
                write!(f, "`()` has no value, so it cannot be stored in {position}")
            }
            Self::EntryPointTakesArguments { found, .. } => write!(
                f,
                "`run` is the entry point and is called with no arguments, \
                 but this one declares {found}"
            ),
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
            Self::ValueBindingUnsupported { name, .. } => write!(
                f,
                "`{name}`: a component-level value declaration is parsed but \
                 not implemented; its initializer would have to run at \
                 component initialization, and it is not a nullary function"
            ),
            Self::AccessorUnsupported { name, .. } => write!(
                f,
                "`{name}` is a getter or setter; accessors parse but are not \
                 implemented, and `{name}` is read rather than called"
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
            Self::StaticArgumentsRequired { name, .. } => write!(
                f,
                "`{name}` is generic; write its static arguments, as in `{name}[\\ZZ64\\]`. \
                 They are never inferred"
            ),
            Self::NotGeneric { name, .. } => {
                write!(f, "`{name}` takes no static arguments")
            }
            Self::StaticArgumentCountMismatch {
                name,
                expected,
                found,
                ..
            } => write!(
                f,
                "`{name}` takes {expected} static argument(s), found {found}"
            ),
            Self::TooManyInstantiations { name, limit, .. } => write!(
                f,
                "instantiating `{name}` would pass {limit} instantiations in one component; \
                 this is what a generic that instantiates itself at a larger type looks like"
            ),
            Self::OverloadSetStaticParamsDiffer { name, first, .. } => write!(
                f,
                "declarations of `{name}` differ in their static parameters (the other is at \
                 {}..{}); an overload set is uniformly generic or uniformly ground",
                first.start, first.end
            ),
            Self::BoundNotSatisfied {
                parameter,
                subject,
                bound,
                ..
            } => write!(
                f,
                "{} does not satisfy `{parameter} extends {}`",
                subject.name(),
                bound.name()
            ),
            Self::UninitialisedArrayOfReferences { found, .. } => write!(
                f,
                "`array(n)` cannot make an array of {}; every slot would start as a value \
                 nothing wrote. Build it from a literal instead",
                found.name()
            ),
        }
    }
}

impl std::error::Error for TypeError {}
