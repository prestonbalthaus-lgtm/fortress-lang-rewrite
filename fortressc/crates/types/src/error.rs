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
    /// An operand of `AND`, `OR` or `NOT` that is not Boolean. Named after the
    /// operator rather than reported as an `if` whose condition is wrong, which
    /// is what desugaring in the parser would have produced.
    LogicalOperandNotBoolean {
        span: Span,
        op: &'static str,
        found: Type,
    },
    /// `<`, `>`, `<=`, `>=` on Boolean. Equality is defined on Boolean;
    /// ordering is not, and inventing one would be a silently wrong answer.
    BooleanNotOrdered {
        span: Span,
        op: &'static str,
    },
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
    /// A functional method that takes static parameters. 1.0 lifts a
    /// functional method into the top-level overload set of its name; a
    /// generic one needs the receiver's type to decide what to instantiate,
    /// and expansion runs before anything is typed. The name exists; the
    /// lifting does not, and saying `unknown name` about it puts the file in
    /// the wrong bucket.
    GenericFunctionalMethodUnsupported {
        span: Span,
        name: String,
    },
    /// A `getter`/`setter` member read as a field. It parses; it is not read.
    AccessorUnsupported {
        span: Span,
        name: String,
    },
    /// `o.f := e` where `f` was not declared `var`. The binding rule is the
    /// same one a local has: only `var` is storage.
    FieldIsImmutable {
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
    /// `assert(a, b)` on values `=` is not defined for. An assert is exactly
    /// as strong as equality is, and no stronger.
    NotComparable {
        span: Span,
        found: Type,
    },
    /// An assignment inside a parallel loop body to something declared outside
    /// it. THIS IS THE DATA RACE, and refusing it syntactically is what lets
    /// M4 ship without any dataflow analysis at all.
    ParallelEscape {
        span: Span,
        name: String,
    },
    /// `a[e] := ...` inside a parallel loop where `e` is not the loop binder.
    /// Distinct iterations touch distinct slots only when the index IS the
    /// binder; anything else needs a proof this compiler does not have.
    ParallelIndexNotBinder {
        span: Span,
        binder: String,
    },
    /// A loop-captured array handed to a call from inside a parallel body.
    /// M4's boundary is LEXICAL -- `assign` only ever sees an assignment
    /// written in the body itself -- and an array travels by pointer, so the
    /// callee's `a[j] := v` is checked against an empty loop context and
    /// refused by nothing. Measured: four million iterations calling one such
    /// function print 567137, 775186, 895320 on consecutive runs and 4000000
    /// under FORTRESS_WORKERS=1.
    ParallelSharedArrayArgument {
        span: Span,
        name: String,
    },
    /// `o.f := e` inside a parallel loop body where `o` is shared between
    /// iterations. The array rule above is `a[binder]`, an index this
    /// iteration provably owns; a field has no index, so there is no
    /// equivalent carve-out and the write is refused outright.
    ParallelFieldEscape {
        span: Span,
        name: String,
        /// The receiver is loop-LOCAL and still shared, because it was bound
        /// from something outside the loop. Naming the wrong one of these two
        /// mechanisms sends the reader to the wrong fix.
        aliased: bool,
    },
    /// A loop-captured object that can REACH mutable storage, handed to a call
    /// from inside a parallel body. The same hole as the array argument, one
    /// indirection further out: the callee's `o.f := v` is checked against an
    /// empty loop context. Reachability is computed over the registry, so an
    /// object holding an object holding an array is refused too.
    ParallelSharedObjectArgument {
        span: Span,
        name: String,
        /// What the reachability walk found, for the diagnostic: the field
        /// path that reaches mutable storage.
        path: String,
    },
    // ------------------------------------------------ control flow extras
    /// `case x of end`. Nothing to compare against and nothing to produce.
    CaseHasNoArms {
        span: Span,
    },
    /// A `case` whose value is used and whose arms may all miss. 1.0 throws
    /// `MatchFailure` there; this subset has no exceptions, so the `else` arm
    /// is what supplies the value instead.
    CaseNeedsElse {
        span: Span,
    },
    /// `typecase` on a scalar. A tag is a fact about an object block, and a
    /// `ZZ32` does not have one.
    TypeCaseSubjectNotReference {
        span: Span,
        found: Type,
    },
    /// An arm naming a type no value of the subject's type can have.
    TypeCaseArmUnrelated {
        span: Span,
        subject: Type,
        arm: Type,
    },
    /// An arm every one of whose tags an earlier arm already claimed. First
    /// arm wins, so this one can never run -- and dead code the reader
    /// believes in is worse than a refusal.
    TypeCaseArmDead {
        span: Span,
        arm: Type,
    },
    /// Two labels of the same name, one inside the other: `exit` would name
    /// the inner one and the outer one would be unreachable.
    LabelAlreadyOpen {
        span: Span,
        name: String,
    },
    UnknownLabel {
        span: Span,
        name: String,
    },
    /// `exit L with 1` where an earlier exit carried something else.
    ExitTypeMismatch {
        span: Span,
        name: String,
        expected: Type,
        found: Type,
    },
    /// A label whose exits carry a value and whose body can also run off the
    /// bottom. There is no value on that edge, and inventing a zero for it is
    /// the silent-wrong-answer class this compiler refuses to join.
    LabelFallsThrough {
        span: Span,
        name: String,
        expected: Type,
        found: Type,
    },
    /// An `exit` out of an `atomic` region. The branch would skip
    /// `fortress_atomic_leave` and leave one process-wide recursive mutex held
    /// for the rest of the process -- `atomic.tex:59-70`'s rollback rule, whose
    /// writes-retained arm this construct re-opens.
    ExitCrossesAtomic {
        span: Span,
        name: String,
    },
    /// An `exit` out of a `for` body. Every loop body is OUTLINED into its own
    /// function, `seq(...)` included, so this is a jump between functions --
    /// which is the unwinding `label` was chosen for not needing.
    ExitCrossesLoop {
        span: Span,
        name: String,
    },
    /// `x op= e` for an operator with no identity the compiler knows. `||=`,
    /// `UNIONCAT=` and the rest need `Monoid[\\T,op\\]` and a user-declared
    /// identity element.
    CompoundOperatorUnsupported {
        span: Span,
        op: &'static str,
    },
    /// A `for` construct outside the M4 subset, named rather than reported as
    /// a syntax error so the file lands in its own bucket.
    ParallelFormUnsupported {
        span: Span,
        form: &'static str,
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
            | Self::LogicalOperandNotBoolean { span, .. }
            | Self::BooleanNotOrdered { span, .. }
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
            | Self::FieldIsImmutable { span, .. }
            | Self::FieldNeedsInitializer { span, .. }
            | Self::SingletonNotConstructible { span, .. }
            | Self::SingletonInitializerRestricted { span, .. }
            | Self::NoApplicableDeclaration { span, .. }
            | Self::AmbiguousDispatch { span, .. }
            | Self::ReturnTypeNotCovariant { span, .. }
            | Self::DispatchTableTooLarge { span, .. }
            | Self::NotPrintable { span, .. }
            | Self::NotComparable { span, .. }
            | Self::ParallelEscape { span, .. }
            | Self::ParallelIndexNotBinder { span, .. }
            | Self::ParallelSharedArrayArgument { span, .. }
            | Self::CaseHasNoArms { span }
            | Self::CaseNeedsElse { span }
            | Self::TypeCaseSubjectNotReference { span, .. }
            | Self::TypeCaseArmUnrelated { span, .. }
            | Self::TypeCaseArmDead { span, .. }
            | Self::LabelAlreadyOpen { span, .. }
            | Self::UnknownLabel { span, .. }
            | Self::ExitTypeMismatch { span, .. }
            | Self::LabelFallsThrough { span, .. }
            | Self::ExitCrossesAtomic { span, .. }
            | Self::ExitCrossesLoop { span, .. }
            | Self::ParallelFieldEscape { span, .. }
            | Self::ParallelSharedObjectArgument { span, .. }
            | Self::CompoundOperatorUnsupported { span, .. }
            | Self::ParallelFormUnsupported { span, .. }
            | Self::AccessorUnsupported { span, .. }
            | Self::GenericFunctionalMethodUnsupported { span, .. }
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
            Self::LogicalOperandNotBoolean { op, found, .. } => write!(
                f,
                "`{op}` takes Boolean operands; this one is {}",
                found.name()
            ),
            Self::BooleanNotOrdered { op, .. } => write!(
                f,
                "`{op}` is not defined on Boolean; equality is, ordering is not"
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
            Self::GenericFunctionalMethodUnsupported { name, .. } => write!(
                f,
                "`{name}` is a generic functional method; it parses, but a \
                 static argument on one cannot be resolved before the \
                 receiver has a type"
            ),
            Self::AccessorUnsupported { name, .. } => write!(
                f,
                "`{name}` is a getter or setter; accessors parse but are not \
                 implemented, and `{name}` is read rather than called"
            ),
            Self::FieldIsImmutable { name, .. } => write!(
                f,
                "field `{name}` is immutable; declare it `var {name}: T = ...` to assign to it"
            ),
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
            Self::ParallelEscape { name, .. } => write!(
                f,
                "`{name}` is declared outside this loop, and a parallel loop \
                 body may not assign to it; iterations run in any order and on \
                 any thread. Write `for ... <- seq(...)` for a sequential loop"
            ),
            Self::ParallelIndexNotBinder { binder, .. } => write!(
                f,
                "a parallel loop may only assign to `a[{binder}]`, the element \
                 its own iteration owns; any other index needs a proof that two \
                 iterations cannot collide"
            ),
            Self::ParallelSharedArrayArgument { name, .. } => write!(
                f,
                "`{name}` is an array every iteration of this parallel loop \
                 shares, and passing it to a call puts any assignment to it \
                 out of reach of the loop's own rules, which are lexical. \
                 Wrap the call in `atomic`, or write `for ... <- seq(...)`"
            ),
            Self::ParallelFieldEscape {
                name,
                aliased: false,
                ..
            } => write!(
                f,
                "`{name}` is declared outside this loop, and a parallel loop \
                 body may not assign to a field of it; a field has no index, \
                 so there is no `a[i]` rule to make two iterations disjoint. \
                 Wrap the assignment in `atomic`, or write `for ... <- seq(...)`"
            ),
            Self::ParallelFieldEscape {
                name,
                aliased: true,
                ..
            } => write!(
                f,
                "`{name}` is declared inside this loop but bound from storage \
                 outside it, so every iteration writes the same object. \
                 Wrap the assignment in `atomic`, or write `for ... <- seq(...)`"
            ),
            Self::ParallelSharedObjectArgument { name, path, .. } => write!(
                f,
                "`{name}` is shared between iterations of this parallel loop \
                 and reaches mutable storage through `{path}`, so passing it \
                 to a call puts any assignment to it out of reach of the \
                 loop's own rules, which are lexical. Wrap the call in \
                 `atomic`, or write `for ... <- seq(...)`"
            ),
            Self::CaseHasNoArms { .. } => write!(
                f,
                "a `case` needs at least one arm; write `guard => expression`"
            ),
            Self::CaseNeedsElse { .. } => write!(
                f,
                "this `case` produces a value, so it needs an `else => ...` arm: \
                 there is nothing to return when no guard matches"
            ),
            Self::TypeCaseSubjectNotReference { found, .. } => write!(
                f,
                "`typecase` asks what concrete type a value has, and `{}` \
                 carries no type tag; only an object or a trait does",
                found.name()
            ),
            Self::TypeCaseArmUnrelated { subject, arm, .. } => write!(
                f,
                "no `{}` can be a `{}`, so this arm can never run",
                subject.name(),
                arm.name()
            ),
            Self::TypeCaseArmDead { arm, .. } => write!(
                f,
                "an earlier arm already claims every concrete type under `{}`; \
                 arms are matched in order and this one can never run",
                arm.name()
            ),
            Self::LabelAlreadyOpen { name, .. } => write!(
                f,
                "`{name}` is already an open label here, and `exit {name}` \
                 would name the inner one"
            ),
            Self::UnknownLabel { name, .. } => {
                write!(f, "`{name}` is not an open label at this point")
            }
            Self::ExitTypeMismatch {
                name,
                expected,
                found,
                ..
            } => write!(
                f,
                "`exit {name}` carries {}, but an earlier exit from `{name}` \
                 carried {}",
                found.name(),
                expected.name()
            ),
            Self::LabelFallsThrough {
                name,
                expected,
                found,
                ..
            } => write!(
                f,
                "`{name}` exits with {}, so its body may not also run off the \
                 bottom with {}: end the body with an `exit {name} with ...` \
                 or with a value of the same type",
                expected.name(),
                found.name()
            ),
            Self::ExitCrossesAtomic { name, .. } => write!(
                f,
                "`exit {name}` leaves an `atomic` region, and the branch would \
                 skip the unlock: one process-wide recursive mutex would stay \
                 held. Move the `exit` outside the `atomic`"
            ),
            Self::ExitCrossesLoop { name, .. } => write!(
                f,
                "`exit {name}` leaves a `for` body, and every loop body is a \
                 function of its own -- `seq(...)` included -- so this is a \
                 jump between functions. Use a `while` loop, or put the label \
                 inside the body"
            ),
            Self::CompoundOperatorUnsupported { op, .. } => write!(
                f,
                "`{op}=` needs an identity element for `{op}`; only `+=` and \
                 `-=` are recognised"
            ),
            Self::ParallelFormUnsupported { form, .. } => write!(
                f,
                "{form} is parsed but not implemented in parallel loops"
            ),
            Self::NotComparable { found, .. } => write!(
                f,
                "`assert` compares its arguments with `=`, which is not \
                 defined on {}",
                found.name()
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
