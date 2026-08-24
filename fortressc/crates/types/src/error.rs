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
    /// A member expansion refused to stamp, reached by a call. See
    /// `Component::cuts`.
    GrowingMemberNotStamped {
        span: Span,
        owner: String,
        origin: String,
        member: String,
    },
    UnknownType {
        span: Span,
        name: String,
    },
    /// A value written where a TYPE parameter was declared: `Cell[\ 3 \]`.
    /// Named separately because "unknown type `3`" sends the reader looking for
    /// a declaration that was never meant to exist.
    StaticValueWhereTypeRequired {
        span: Span,
        written: String,
    },
    /// A type written where a `nat`/`int`/`bool` parameter was declared.
    TypeWhereStaticValueRequired {
        span: Span,
        param: String,
        kind: &'static str,
        written: String,
    },
    /// `D7 §3.1`: a static argument must be STATICALLY EVALUABLE. This is the
    /// name that did not resolve to an enclosing value parameter.
    StaticArgumentNotConstant {
        span: Span,
        name: String,
    },
    /// `D7 §3.1` again, from the other side: a form the static-expression
    /// sublanguage does not contain. The sublanguage is what the corpus writes
    /// -- literals, names, `+`, `-` and juxtaposition -- and nothing else.
    StaticExpressionForm {
        span: Span,
        form: &'static str,
    },
    /// `D7 §3.2`, a NAMED DEVIATION. `NatReflect.reflect(z:ZZ32):NatParam`
    /// turns a RUN-TIME value into a static parameter, and a monomorphizing
    /// compiler cannot stamp a specialisation for a value it does not know.
    /// It must name the mechanism: the failure otherwise surfaces as an
    /// unrelated mismatch deep inside `ChunkedSparseArray`.
    NatReflectRuntimeArgument {
        span: Span,
    },
    /// A bound written on a value-kinded static parameter: `[\nat n extends
    /// Foo\]`. There is no constraint solver -- D7's own census found ZERO
    /// `where { k < n }` in 1956 files -- so a bound there is refused rather
    /// than silently dropped.
    StaticValueParameterBound {
        span: Span,
        name: String,
    },
    /// `traits.tex:161-162`: "In an API (but not a component), a `comprises`
    /// clause may include `...`". The marker was DROPPED by the parser until
    /// the clause gained a reader, so an open set and an unwritten one were
    /// the same empty list and a component could write either.
    OpenComprisesInComponent {
        span: Span,
        name: String,
    },
    /// `traits.tex:232-235`: the traits a `comprises` clause lists "are exactly
    /// the traits that immediately extend T and they must explicitly extend T".
    ComprisesNameDoesNotExtend {
        span: Span,
        trait_name: String,
        listed: String,
    },
    /// `traits.tex:236-241`: when a `comprises` clause includes `...`, a
    /// component exporting the api may extend the trait, "but these traits may
    /// not be declared or imported by the API".
    ExtendsOpenComprises {
        span: Span,
        trait_name: String,
        extender: String,
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
    /// A value whose type IS a subtype of the slot's and which still has no way
    /// to sit in one. `types-vals-vars.tex:121-122` makes every type a subtype
    /// of `Any`; a trait slot is a POINTER TO A TAGGED BLOCK -- 32-bit concrete
    /// type tag at offset 0 -- and only an object has one. A `ZZ32` is an
    /// unboxed `i32`, a `String` and an array are pointers with NO tag, and a
    /// tuple has no single value at all. There is no boxing in this backend.
    ///
    /// SUBTYPING IS NOT STORAGE, and this is the same split `VoidNotStorable`
    /// draws for `()` -- kept separate because `()` has no value whatever the
    /// slot is, while these have one and it is the wrong SHAPE.
    ///
    /// WITHOUT IT `g(x: Any)` beside `g(x: O)`, called as `g(3)`, reached
    /// codegen and LLVM rejected the module: `Call parameter type does not
    /// match function signature`. A single-candidate call was already refused,
    /// because the parameter type reaches the literal as a hint; two candidates
    /// that disagree on a position give no hint, so the argument types as
    /// `ZZ32` and the check has to happen after dispatch resolves.
    NoTraitRepresentation {
        span: Span,
        found: Type,
        required: Type,
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
    /// A juxtaposition with a String whose other operand has no `to_string`
    /// shim. Its own variant rather than `NotPrintable`, because a
    /// concatenation need not be inside a `println` -- and a diagnostic that
    /// names the wrong mechanism is the class this project has already lost an
    /// hour to twice.
    NotConcatenable {
        span: Span,
        found: Type,
    },
    /// An integer division whose divisor is the literal `0`. There is no
    /// quotient, and without this the program builds: LLVM's own constant
    /// folder turns the division into `poison` and the callee prints whatever
    /// that lowers to. RR64 does not reach here -- `1.0/0.0` is `inf`.
    DivisionByZero {
        span: Span,
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
    /// Two declarations in one overload set with the SAME parameter types.
    /// Both spans, because one is not enough to find the pair -- and the
    /// arguments, because with overloading the NAME is shared by design and it
    /// is the types that collide.
    DuplicateOverload {
        span: Span,
        first: Span,
        name: String,
        arguments: String,
    },
    DuplicateDefinition {
        span: Span,
        name: String,
    },
    NotAnArray {
        span: Span,
        found: Type,
    },
    /// A `for x <- g` or a comprehension whose source is an object that does
    /// not carry the indexed generator protocol. `missing` names the FIRST
    /// member it does not answer, because "it is not a generator" without
    /// saying which member is absent sends a reader looking at the wrong half.
    NotAGenerator {
        span: Span,
        found: Type,
        missing: &'static str,
    },
    /// `if x <- g` or `while x <- g` whose source is not a `Condition`.
    NotACondition {
        span: Span,
        found: Type,
        missing: &'static str,
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
    /// `dim Frequency = 1 / Tyme`. The same shape as an unresolved `comprises`
    /// name: a derivation over a name that is not declared is a claim about a
    /// dimension that does not exist.
    ///
    /// IT REFUSES THE SHIPPED 1.0 LIBRARY, and that is the rule working rather
    /// than a reason to weaken it: `Library/incomplete/basic/Fortress.SIUnits.fsi`
    /// writes `dim ElectricPotential = Power / Current` with no `Current`
    /// declared -- the dimension is `ElectricCurrent` -- and
    /// `dim AngularVelocity = Angle / Second`, where `Second` is a UNIT of
    /// `Time` and not a dimension at all.
    UnknownDimensionName {
        span: Span,
        name: String,
        wanted: &'static str,
        /// The name is a declared unit with an SI prefix on it. Worth saying,
        /// because the reader would otherwise go looking for a declaration
        /// that was never meant to be written.
        prefixed: bool,
    },
    /// `Float1[\meter, 8, 24\]`. See `bind_static`.
    DimensionalParameterInstantiated {
        span: Span,
        param: String,
        kind: &'static str,
    },
    DimensionDeclaredTwice {
        span: Span,
        name: String,
        kind: &'static str,
    },
    DimensionNameCollides {
        span: Span,
        name: String,
        kind: &'static str,
    },
    /// `x: RR64 meter`, `x: Length`. A dimension and a unit are their own
    /// namespace and neither is a type: `dimensions.tex:237-253` gives a
    /// dimensioned value a representation this backend has no boxing for, and
    /// `dimensions.tex:206-215` makes a unit mismatch a static error that
    /// nothing here can decide. Refused at `Registry::resolve`, which is the
    /// single gate.
    DimensionIsNotAType {
        span: Span,
        name: String,
        kind: &'static str,
    },
    /// `'a' + 'b'`. A `Char` is ORDERED and not NUMERIC: the six comparisons
    /// are defined on it and no arithmetic is. 1.0 spells the two conversions
    /// `char` and `codePoint` and neither is in this subset, so there is no
    /// implicit route to an integer either.
    CharNotNumeric {
        span: Span,
        op: &'static str,
    },
    /// A rank this compiler cannot represent. `ZZ32[2,3]` resolves now --
    /// `Type::Array` carries a rank -- so what is left here is a shape with NO
    /// dimensions at all, and one with more than a `u8` can hold.
    ArrayDimensions {
        span: Span,
        dimensions: usize,
    },
    /// `a[i]` on a `ZZ32[2,3]`, or `a[i,j]` on a `ZZ32[5]`. The rank is a fact
    /// about the TYPE and the count is a fact about the SOURCE, so only the
    /// checker can compare them -- the parser reads a list of whatever length
    /// was written.
    SubscriptArity {
        span: Span,
        rank: u8,
        found: usize,
    },
    /// Something a rank-one array can do and a higher one cannot yet. Named
    /// with the rank and the operation rather than refused as "not an array",
    /// which is what `length(a)` on a `ZZ32[2,3]` would otherwise say about a
    /// value that plainly is one.
    ArrayRankNotImplemented {
        span: Span,
        what: &'static str,
        rank: u8,
    },
    /// `ZZ32[0#5]` and `ZZ32[1:5]`. `traits.tex:106-108` gives an extent three
    /// spellings and only the bare size resolves here, because a lower bound
    /// other than zero has nowhere to live: `fortress_array_slot` indexes from
    /// zero and the header carries a length and no origin.
    ExtentRangeNotImplemented {
        span: Span,
        written: String,
    },
    /// `ZZ32[]`. `traits.tex:98` makes the size optional and nothing in this
    /// compiler can act on its absence -- an array type with no size is
    /// `Array[\T\]`, which is spelled that way.
    ArraySizeMissing {
        span: Span,
    },
    /// The size is not a number by the time the checker sees it. Every extent
    /// goes through `mono`'s substitution, so a name that survives to here
    /// resolved to nothing -- it is neither a value parameter in scope nor a
    /// literal.
    ArraySizeNotStatic {
        span: Span,
        written: String,
    },
    /// `a: ZZ32[5] = [1 2 3 4 5 6]`. The one place the declared extent and a
    /// literal's length are both in hand. See `check_declared_extent`.
    ArrayExtentMismatch {
        span: Span,
        declared: i64,
        found: usize,
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
    /// `o.f = v` in statement position. `=` is an equality operator in
    /// expression position, so without this the program COMPILES: the compare
    /// is emitted, its result is discarded, and the field is printed unchanged.
    /// `Specification/basic/expressions/blocks.tex:49-63` makes the program
    /// invalid twice over -- a non-final item must have type `()`, and an
    /// equality test in a block must be parenthesised -- so this is a refusal
    /// and not a missing feature. Field mutation is not implemented either, so
    /// the message must not send the reader to `:=`, which dead-ends on
    /// `InvalidAssignTarget` and then on `MutableFieldUnsupported`.
    FieldAssignmentUnsupported {
        span: Span,
        name: String,
    },
    /// `f(x = 2)` where `x` is a bound local. 1.0 reads that as a KEYWORD
    /// ARGUMENT and reserves extra parentheses for the equality test; the
    /// parser erases parentheses without a trace, so the two spellings are the
    /// same tree and the compiler cannot tell them apart. Until this refusal it
    /// silently chose the test and passed a Boolean -- a wrong argument, not a
    /// failed compile. Keyword arguments are not implemented, so the honest
    /// answer is to refuse the shape rather than to guess which one was meant.
    ///
    /// Only callees that COULD take a keyword argument are guarded: a user
    /// function, a method and a constructor. `assert(count = 1000)` and
    /// `println(x = 2)` are unambiguous equality tests -- no builtin has a
    /// named parameter -- and they are legal, working Fortress today.
    KeywordArgumentUnsupported {
        span: Span,
        name: String,
    },
    /// `widen` reaches ZZ32 -> ZZ64, ZZ32 -> RR64 and ZZ64 -> RR64, the three
    /// `Type::is_widening_of` recognises. Anything else has no widening.
    NotWidenable {
        span: Span,
        found: Type,
    },

    // ------------------------------------------------------------------ M3c
    /// An `api` parses, so the corpus metric can move, but there is nothing to
    /// emit for a file of signatures.
    /// An `api` declaration that carries a body. `source-code.tex:313-320`
    /// makes an api a set of DECLARATIONS; a declaration with a body is a
    /// definition, and definitions live in the component.
    ///
    /// This replaces `ApiNotExecutable`, which said the same thing about the
    /// whole FILE and said it before anything was checked. An api is checkable;
    /// what it is not is emittable.
    ApiDeclarationHasBody {
        span: Span,
        name: String,
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
    /// `o.n += e` where `n` is a declared `setter`. The plain form is a CALL,
    /// so the compound form is a read through the getter, the operator, and a
    /// call -- three steps, and the read half only became possible when getters
    /// became readable. Refused by name rather than half-built, because storing
    /// straight into the slot is the silent wrong answer this whole mechanism
    /// exists to stop.
    CompoundAssignThroughSetter {
        span: Span,
        name: String,
    },
    /// `throw FooExn` where `FooExn` does not extend `Exception`. 1.0's own
    /// message, recorded in `XXX9aa.test`, is "`throw` can only throw objects of
    /// Exception type. This expression is of type FooExn." -- and that file is
    /// in the oracle's must-fail set, so a laxer rule here shows up as a
    /// regression rather than as generosity.
    ThrownValueIsNotAnException {
        span: Span,
        found: Type,
    },
    /// `O(...)` where `O` was MERGED IN by the import resolver. The api that
    /// declared it gave a signature; the definition is in that api's own
    /// component, which this whole-program compiler never compiles. Reaching
    /// codegen with one produced `internal error: unknown function `O$new``.
    MergedObjectNotConstructible {
        span: Span,
        name: String,
    },
    /// A comprehension whose bracket is not `<| |>`. THE LIST FORM IS BUILT --
    /// it lowers onto a minted `List[\T\]` -- and a set, map or array
    /// comprehension needs the collection its own brackets name.
    ComprehensionUnsupported {
        span: Span,
        bracket: String,
    },
    /// `try ... catch ... end`. It PARSES -- the whole shape is in the AST --
    /// and the lowering is not built. Named rather than left as a reserved-word
    /// refusal so the file lands in the exceptions bucket and not the parser
    /// one.
    TryUnsupported {
        span: Span,
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
    /// `o.g()` where `g` is a getter. A GETTER IS READ, NEVER CALLED --
    /// `Compiled6.y.fss` has `println O.z` on one line and `println O.z()` on
    /// the next, and 1.0 refuses the second with "No such method O.z". The two
    /// spellings are the whole reason a getter is not simply a nullary method
    /// to the source, even though it is exactly that underneath.
    AccessorCalled {
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
    /// A parameter with the same name as a top-level value.
    /// `declarations.tex:476-533` lists every shadowing a Fortress program may
    /// contain -- a field or dotted method, a KEYWORD parameter, `self`,
    /// `result` -- and closes with "No other shadowing is permitted".
    IllegalShadowing {
        span: Span,
        name: String,
    },
    /// `x := 0` and `var x = 0` at component level, both with no declared
    /// type. The GRAMMAR forbids both: `variables.tex:22-27` gives the untyped
    /// form as `VarImmutableMods? BindIdOrBindIdTuple = Expr` -- IMMUTABLE
    /// modifiers and `=` only -- and every alternative that admits `var` or
    /// `:=` carries a `: Type`. `Variable.rats:17` goes further and lists
    /// `"var" BindIdOrBindIdTuple "=" Expr` as an explicit ERROR PRODUCTION.
    MutableValueNeedsType {
        span: Span,
        name: String,
    },
    /// Top-level values whose initializers depend on each other in a ring.
    /// `variables.tex:122-123` lets an initializer refer to a value declared
    /// LATER, so declaration order is not evaluation order -- but a cycle has
    /// no order at all.
    CyclicValueInitialization {
        span: Span,
        names: String,
    },
    /// `(a, a) = (1, 2)`. Two parts of one binder claiming the same name.
    DuplicateBinderName {
        span: Span,
        name: String,
    },
    /// `(a, b) = (1, 2, 3)`. The binder and the initializer disagree on how
    /// many elements there are.
    TupleArityMismatch {
        span: Span,
        names: usize,
        values: usize,
    },
    /// A tuple in a position a DEFINED function would have to lower. Naming
    /// one in an `api` signature is fine; an api is never lowered.
    TupleNotStorable {
        span: Span,
        position: &'static str,
    },
    /// `atomic (spawn ...)`. A RULE and not a gap -- spawn.tex:28-31 forbids
    /// it, and `ProjectFortress/compiler_tests/Compiled1.am.fss:15` carries the
    /// prohibition as a source comment.
    SpawnInsideAtomic {
        span: Span,
    },
    /// `val()` on a handle whose body produced no scalar. It needs a boxed
    /// representation this backend does not have.
    ThreadValueNotRepresentable {
        span: Span,
        result: Type,
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
    /// A `fn` form outside the subset this lowering can mint an object for.
    LambdaUnsupported {
        span: Span,
        form: &'static str,
    },
    /// A lambda closing over a name whose type is not written, or over `self`.
    /// A capture becomes a constructor parameter and a constructor parameter
    /// needs a written type; `self` would be shadowed by the generated
    /// object's own receiver.
    LambdaCaptureUntyped {
        span: Span,
        name: String,
    },
    /// An object's value parameter or a field written with a tuple type. Those
    /// decide a LAYOUT and a constructor arity, so flattening one would change
    /// what dispatch has already been told.
    TupleFieldNotFlattened {
        span: Span,
    },
    /// `if x <- g then ... end`. It PARSES -- `DelimitedExpr.rats:37,39,40,216`
    /// makes the condition a `GeneratorClause` -- and the lowering is not
    /// built: yielding IS the truth, and asking a value whether it yields is
    /// the generator protocol.
    BindingConditionUnsupported {
        span: Span,
        keyword: &'static str,
    },
    /// A tuple inside a tuple. Flattening it would make an arity depend on a
    /// type's shape two levels down; measured at zero corpus files.
    TupleNested {
        span: Span,
    },
    /// A flattened name used as anything but a whole argument or the right-hand
    /// side of a destructuring.
    TupleNameNotWhole {
        span: Span,
        name: String,
    },
    /// `var t: (A,B) := ...`. Splitting the binding would have to split every
    /// assignment to it, and `t := f()` cannot split at all.
    TupleLocalMutable {
        span: Span,
    },
    /// A tuple-typed binding whose initialiser is neither written as a tuple
    /// nor another flattened name.
    TupleLocalUnsplittable {
        span: Span,
    },
    /// A list comprehension in a position that writes neither its own `[\T\]`
    /// nor a `List[\T\]` slot for it to land in.
    ComprehensionElementUnwritten {
        span: Span,
    },
    /// A generator shape the lowering does not reach.
    ComprehensionGeneratorUnsupported {
        span: Span,
        form: &'static str,
    },
    /// The component already declares or imports a `List`, so the minted one
    /// would be a duplicate.
    ComprehensionListTaken {
        span: Span,
    },
    /// A `fn` or an anonymous `object` closing over a MUTABLE local. The hoist
    /// makes a capture a constructor argument, which copies it, so a later
    /// assignment to the local would not be seen and an assignment from inside
    /// would not be written back. 1.0 captures the cell.
    CaptureIsMutable {
        span: Span,
        name: String,
    },
    /// A function name in a slot that wants an arrow, where the overload set
    /// has no declaration with that exact signature, or more than one.
    FunctionValueUnresolved {
        span: Span,
        name: String,
        arrow: String,
        found: usize,
    },

    /// `do 3 also do 5 end`. `also.tex:24-27` requires every block of a group
    /// and the group itself to have type `()`. The legacy implementation says
    /// the same thing -- `XXX10a.test` expects
    /// "do-also expression has type IntLiteral, but it must have () type".
    AlsoBlockNotVoid {
        span: Span,
        found: Type,
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
            | Self::GrowingMemberNotStamped { span, .. }
            | Self::UnknownType { span, .. }
            | Self::StaticValueWhereTypeRequired { span, .. }
            | Self::TypeWhereStaticValueRequired { span, .. }
            | Self::StaticArgumentNotConstant { span, .. }
            | Self::StaticExpressionForm { span, .. }
            | Self::NatReflectRuntimeArgument { span }
            | Self::StaticValueParameterBound { span, .. }
            | Self::OpenComprisesInComponent { span, .. }
            | Self::ComprisesNameDoesNotExtend { span, .. }
            | Self::ExtendsOpenComprises { span, .. }
            | Self::TypeNotImplemented { span, .. }
            | Self::VoidNotStorable { span, .. }
            | Self::NoTraitRepresentation { span, .. }
            | Self::EntryPointTakesArguments { span, .. }
            | Self::ArityMismatch { span, .. }
            | Self::LiteralOutOfRange { span, .. }
            | Self::NotConcatenable { span, .. }
            | Self::DivisionByZero { span }
            | Self::LiteralNotApplicable { span, .. }
            | Self::ConditionNotBoolean { span, .. }
            | Self::BranchTypeMismatch { span, .. }
            | Self::MissingElseBranch { span }
            | Self::DuplicateOverload { span, .. }
            | Self::DuplicateDefinition { span, .. }
            | Self::NotAnArray { span, .. }
            | Self::NotAGenerator { span, .. }
            | Self::NotACondition { span, .. }
            | Self::ElementTypeUnknown { span }
            | Self::UnsupportedElementType { span, .. }
            | Self::UnknownDimensionName { span, .. }
            | Self::DimensionalParameterInstantiated { span, .. }
            | Self::DimensionDeclaredTwice { span, .. }
            | Self::DimensionNameCollides { span, .. }
            | Self::DimensionIsNotAType { span, .. }
            | Self::CharNotNumeric { span, .. }
            | Self::ArrayDimensions { span, .. }
            | Self::SubscriptArity { span, .. }
            | Self::ArrayRankNotImplemented { span, .. }
            | Self::ExtentRangeNotImplemented { span, .. }
            | Self::ArraySizeMissing { span }
            | Self::ArraySizeNotStatic { span, .. }
            | Self::ArrayExtentMismatch { span, .. }
            | Self::AssignToImmutable { span, .. }
            | Self::AssignToUndeclared { span, .. }
            | Self::InvalidAssignTarget { span }
            | Self::FieldAssignmentUnsupported { span, .. }
            | Self::KeywordArgumentUnsupported { span, .. }
            | Self::NotWidenable { span, .. }
            | Self::ApiDeclarationHasBody { span, .. }
            | Self::MissingBody { span, .. }
            | Self::TraitCycle { span, .. }
            | Self::IllegalShadowing { span, .. }
            | Self::MutableValueNeedsType { span, .. }
            | Self::CyclicValueInitialization { span, .. }
            | Self::DuplicateBinderName { span, .. }
            | Self::TupleArityMismatch { span, .. }
            | Self::TupleNotStorable { span, .. }
            | Self::SpawnInsideAtomic { span }
            | Self::ThreadValueNotRepresentable { span, .. }
            | Self::NotATrait { span, .. }
            | Self::UnknownField { span, .. }
            | Self::ThrownValueIsNotAnException { span, .. }
            | Self::MergedObjectNotConstructible { span, .. }
            | Self::TryUnsupported { span }
            | Self::ComprehensionUnsupported { span, .. }
            | Self::DottedMethodUnsupported { span, .. }
            | Self::CompoundAssignThroughSetter { span, .. }
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
            | Self::LambdaUnsupported { span, .. }
            | Self::LambdaCaptureUntyped { span, .. }
            | Self::CaptureIsMutable { span, .. }
            | Self::TupleFieldNotFlattened { span }
            | Self::BindingConditionUnsupported { span, .. }
            | Self::TupleNested { span }
            | Self::TupleNameNotWhole { span, .. }
            | Self::TupleLocalMutable { span }
            | Self::TupleLocalUnsplittable { span }
            | Self::ComprehensionElementUnwritten { span }
            | Self::ComprehensionGeneratorUnsupported { span, .. }
            | Self::ComprehensionListTaken { span }
            | Self::FunctionValueUnresolved { span, .. }
            | Self::AlsoBlockNotVoid { span, .. }
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
            | Self::AccessorCalled { span, .. }
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

impl TypeError {
    /// Secondary spans, for a renderer that has the source. Two variants point
    /// at a SECOND declaration, and their byte offsets used to be written into
    /// the message itself -- which is the one thing a `Display` with no source
    /// and no path cannot turn into a position.
    #[must_use]
    pub fn notes(&self) -> Vec<(Span, &'static str)> {
        match self {
            Self::AmbiguousDispatch { first, second, .. } => vec![
                (*first, "one declaration is here"),
                (*second, "and the other is here"),
            ],
            Self::OverloadSetStaticParamsDiffer { first, .. } => {
                vec![(*first, "the other declaration is here")]
            }
            _ => Vec::new(),
        }
    }
}

impl core::fmt::Display for TypeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
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
            Self::GrowingMemberNotStamped {
                owner,
                origin,
                member,
                ..
            } => write!(
                f,
                "`{member}` on `{owner}` returns `{origin}` at static arguments that \
                 properly contain its own, so every stamp demands a larger one; this \
                 compiler monomorphizes and the chain has no fixpoint. The member is \
                 declared and cannot be called"
            ),
            Self::UnknownType { name, .. } => write!(f, "unknown type `{name}`"),
            Self::StaticValueWhereTypeRequired { written, .. } => write!(
                f,
                "`{written}` is a static VALUE and this parameter is declared as a type"
            ),
            Self::TypeWhereStaticValueRequired {
                param,
                kind,
                written,
                ..
            } => write!(
                f,
                "`{written}` is a type and `{param}` is declared `{kind}`, which takes a \
                 statically-known value"
            ),
            Self::StaticArgumentNotConstant { name, .. } => write!(
                f,
                "a static argument must be known at compile time, and `{name}` is not an \
                 enclosing static parameter"
            ),
            Self::StaticExpressionForm { form, .. } => write!(
                f,
                "{form} is not part of the static-expression sublanguage; a static \
                 argument is a literal, an enclosing static parameter, or `+`, `-` and \
                 juxtaposition over those"
            ),
            Self::NatReflectRuntimeArgument { .. } => write!(
                f,
                "a `nat` static argument must be known at compile time, and \
                 `NatReflect.reflect` produces one at run time -- a monomorphizing \
                 compiler cannot stamp a specialisation for a value it does not know"
            ),
            Self::StaticValueParameterBound { name, .. } => write!(
                f,
                "`{name}` is a value static parameter and carries a bound; there is no \
                 constraint solver, and no corpus file writes one"
            ),
            Self::OpenComprisesInComponent { name, .. } => write!(
                f,
                "the `comprises` clause of `{name}` is open (`...`), which an api may \
                 write and a component may not"
            ),
            Self::ComprisesNameDoesNotExtend {
                trait_name,
                listed,
                ..
            } => write!(
                f,
                "`{listed}` is listed in the `comprises` clause of `{trait_name}` but does \
                 not explicitly extend `{trait_name}`"
            ),
            Self::ExtendsOpenComprises {
                trait_name,
                extender,
                ..
            } => write!(
                f,
                "`{extender}` extends `{trait_name}`, whose `comprises` clause is open \
                 (`...`) -- an api may not declare a trait that extends one of its own \
                 open-comprises traits"
            ),
            Self::TypeNotImplemented { form, .. } => {
                write!(f, "{form} is not implemented in this subset")
            }
            Self::VoidNotStorable { position, .. } => {
                write!(f, "`()` has no value, so it cannot be stored in {position}")
            }
            Self::NoTraitRepresentation {
                found,
                required,
                position,
                ..
            } => write!(
                f,
                "{} is a subtype of {} and has no representation in one: a \
                 trait slot is a pointer to a tagged object and there is no \
                 boxing in this compiler, so it cannot be stored in {position}",
                found.name(),
                required.name()
            ),
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
            Self::NotConcatenable { found, .. } => write!(
                f,
                "a juxtaposition with a String converts its other operands to String, \
                 and {} has no conversion",
                found.name()
            ),
            Self::DivisionByZero { .. } => {
                write!(f, "this division has a literal zero divisor")
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
            Self::DuplicateOverload {
                name,
                arguments,
                first,
                ..
            } => write!(
                f,
                "`{name}` is declared twice on the same argument types \
                 ({arguments}); the other is at byte {}",
                first.start
            ),
            Self::NotAnArray { found, .. } => {
                write!(f, "expected an array, found {}", found.name())
            }
            // THE MEMBER NAMES ARE 1.0'S. `Indexed[\E,I\]`
            // (`FortressLibrary.fsi:1205`) declares `getter size()` and
            // `opr [i: I]: E`, and this compiler iterates a generator by
            // walking those two -- so a diagnostic naming them is naming the
            // library's own protocol rather than an invention.
            Self::NotAGenerator { found, missing, .. } => write!(
                f,
                "{} is not a generator here: iterating one walks `size` and \
                 `opr []`, as `Indexed` declares them, and {} declares no `{missing}`",
                found.name(),
                found.name()
            ),
            Self::NotACondition { found, missing, .. } => write!(
                f,
                "{} is not a condition here: `if x <- g` yields zero or one \
                 value and reads `holds` and `get`, as `Condition` declares \
                 them, and {} declares no `{missing}`",
                found.name(),
                found.name()
            ),
            Self::ElementTypeUnknown { .. } => write!(
                f,
                "nothing here says what this array holds; annotate the binding, as in `a:Array[\\ZZ64\\] = ...`"
            ),
            Self::UnknownDimensionName {
                name,
                wanted,
                prefixed,
                ..
            } => {
                write!(f, "`{name}` is not a declared {wanted}")?;
                if *prefixed {
                    write!(f, "; SI prefixes are not generated")?;
                }
                Ok(())
            }
            Self::DimensionalParameterInstantiated { param, kind, .. } => write!(
                f,
                "`{param}` is a `{kind}` static parameter and instantiating one is not implemented; a value may not carry a dimension in this subset"
            ),
            Self::DimensionDeclaredTwice { name, kind, .. } => {
                write!(f, "the {kind} `{name}` is declared twice")
            }
            Self::DimensionNameCollides { name, kind, .. } => write!(
                f,
                "`{name}` is declared as a {kind} and as a type; they are separate namespaces and a name may be in only one"
            ),
            Self::DimensionIsNotAType { name, kind, .. } => write!(
                f,
                "`{name}` is {kind}, not a type; a value may not carry a dimension in this subset"
            ),
            Self::CharNotNumeric { op, .. } => write!(
                f,
                "`{op}` is not defined on Char; a character is ordered, not numeric"
            ),
            Self::ArrayDimensions { dimensions, .. } => write!(
                f,
                "this array type has {dimensions} dimensions, which this compiler cannot represent"
            ),
            Self::SubscriptArity { rank, found, .. } => write!(
                f,
                "a rank {rank} array takes {rank} subscript(s), found {found}"
            ),
            Self::ArrayRankNotImplemented { what, rank, .. } => write!(
                f,
                "{what} of a rank {rank} array is not in this subset"
            ),
            Self::ExtentRangeNotImplemented { written, .. } => write!(
                f,
                "`{written}` is an extent range; an array type in this subset writes its size and nothing else"
            ),
            Self::ArraySizeMissing { .. } => write!(
                f,
                "this array type writes no size; an array type with no size is written `Array[\\T\\]`"
            ),
            Self::ArraySizeNotStatic { written, .. } => write!(
                f,
                "`{written}` is not a number, so it cannot be an array size; a size is a literal or a value parameter in scope"
            ),
            Self::ArrayExtentMismatch {
                declared, found, ..
            } => write!(
                f,
                "this array is declared with {declared} element(s) and {found} are written"
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
            Self::FieldAssignmentUnsupported { name, .. } => write!(
                f,
                "`.{name} = ...` here is an equality test whose result is discarded, \
                 not an assignment; field mutation is not implemented. Write \
                 `ignore(...)` or `_ = ...` if the comparison is what you meant"
            ),
            Self::KeywordArgumentUnsupported { name, .. } => write!(
                f,
                "`{name} = ...` as an argument is a keyword argument, which is not \
                 implemented; it was being passed as the Boolean result of an equality \
                 test. Bind the value first, or compare inside `ignore(...)`"
            ),
            Self::NotWidenable { found, .. } => write!(
                f,
                "`widen` widens ZZ32 to ZZ64 or RR64 and ZZ64 to RR64; {} is not widened",
                found.name()
            ),
            Self::ApiDeclarationHasBody { name, .. } => write!(
                f,
                "`{name}` has a body, and an `api` is a set of declarations; \
                 the definition belongs in the component"
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
            Self::CompoundAssignThroughSetter { name, .. } => write!(
                f,
                "`{name}` is a setter, so `o.{name} := e` is a call; the \
                 compound form would have to read through the getter first and \
                 that is not implemented -- write the read out"
            ),
            Self::ThrownValueIsNotAnException { found, .. } => write!(
                f,
                "`throw` can only throw objects of Exception type, and this \
                 expression is of type {}",
                found.name()
            ),
            Self::MergedObjectNotConstructible { name, .. } => write!(
                f,
                "`{name}` comes from an imported api, which declares it and \
                 does not define it; this compiler has no separate compilation, \
                 so there is no constructor to call"
            ),
            Self::ComprehensionUnsupported { bracket, .. } => write!(
                f,
                "a `{bracket}` comprehension parses and its lowering is not \
                 implemented; only the list form `<| e | x <- lo:hi |>` is"
            ),
            Self::TryUnsupported { .. } => write!(
                f,
                "`try` parses and its lowering is not implemented; an uncaught \
                 `throw` halts, and nothing catches one yet"
            ),
            Self::GenericFunctionalMethodUnsupported { name, .. } => write!(
                f,
                "`{name}` is a generic functional method; it parses, but a \
                 static argument on one cannot be resolved before the \
                 receiver has a type"
            ),
            Self::AccessorCalled { name, .. } => write!(
                f,
                "`{name}` is a getter and is READ as `.{name}`, not called as \
                 `.{name}()`"
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
                name, arguments, ..
            } => write!(
                f,
                "`{name}` is ambiguous for ({arguments}): the declarations below are both \
                 most specific, and neither is more specific than the other"
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
            Self::LambdaUnsupported { form, .. } => {
                write!(f, "{form} is not implemented")
            }
            Self::LambdaCaptureUntyped { name, .. } if name == "self" => write!(
                f,
                "a `fn` may not close over `self`: the generated object binds \
                 `self` to the closure itself, and the capture would be \
                 shadowed rather than refused. Pass it as a parameter"
            ),
            Self::LambdaCaptureUntyped { name, .. } => write!(
                f,
                "`{name}` has no written type, so a `fn` closing over it has \
                 nothing to declare its constructor parameter with. Annotate \
                 it -- `{name}: T = ...`"
            ),
            Self::TupleFieldNotFlattened { .. } => write!(
                f,
                "a tuple is not flattened here: an object's value parameters \
                 and its fields decide a layout, and a tuple has no \
                 representation to give one"
            ),
            Self::BindingConditionUnsupported { keyword, .. } => write!(
                f,
                "`{keyword} x <- g` parses and its lowering is not implemented: \
                 the generator yields zero or one value and YIELDING IS THE \
                 TRUTH, which needs the generator protocol"
            ),
            Self::TupleNested { .. } => write!(
                f,
                "a tuple inside a tuple is not flattened; arity flattening goes \
                 one level and no corpus file writes two"
            ),
            Self::TupleNameNotWhole { name, .. } => write!(
                f,
                "`{name}` is a tuple and tuples are FLATTENED here, so it has \
                 no value of its own. Pass it whole to a call, or destructure \
                 it with `(a, b) = {name}`"
            ),
            Self::TupleLocalMutable { .. } => write!(
                f,
                "a mutable tuple binding is not flattened: every assignment to \
                 it would have to split, and one whose value is a call cannot"
            ),
            Self::TupleLocalUnsplittable { .. } => write!(
                f,
                "this tuple binding's initializer is neither written as a tuple \
                 nor another tuple name, so there is nothing to flatten it into"
            ),
            Self::ComprehensionElementUnwritten { .. } => write!(
                f,
                "this comprehension's element type is not written anywhere. A \
                 static argument is never inferred here -- write \
                 `<|[\\T\\] e | ... |>`, or give the binding it initialises \
                 the type `List[\\T\\]`"
            ),
            Self::ComprehensionGeneratorUnsupported { form, .. } => write!(
                f,
                "{form} is not implemented in a comprehension"
            ),
            Self::ComprehensionListTaken { .. } => write!(
                f,
                "a list comprehension mints its own `List`, and this component \
                 already has one"
            ),
            Self::CaptureIsMutable { name, .. } => write!(
                f,
                "`{name}` is mutable, and a closure captures it BY VALUE here: \
                 the hoist makes every capture a constructor argument. A later \
                 `{name} := ...` would not be seen inside. Pass it in, or bind \
                 an immutable copy first"
            ),
            Self::FunctionValueUnresolved {
                name, arrow, found, ..
            } => write!(
                f,
                "`{name}` is used as a value of type `{arrow}`, and {} \
                 declaration of `{name}` has that signature",
                if *found == 0 { "no" } else { "more than one" }
            ),
            Self::AlsoBlockNotVoid { found, .. } => write!(
                f,
                "a block of an `also` group has type {}, and every block of one \
                 must have type () -- the group produces no value to combine",
                found.name()
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
            Self::IllegalShadowing { name, .. } => write!(
                f,
                "`{name}` is already declared at the top level, and a \
                 parameter may not shadow it"
            ),
            Self::MutableValueNeedsType { name, .. } => write!(
                f,
                "the type of `{name}` is required: a mutable top-level value \
                 must write its type, whether it is spelled `var` or `:=`"
            ),
            Self::CyclicValueInitialization { names, .. } => write!(
                f,
                "the top-level values {names} initialize in a cycle, so there \
                 is no order that evaluates each before it is read"
            ),
            Self::DuplicateBinderName { name, .. } => {
                write!(f, "`{name}` is bound twice by one tuple binding")
            }
            Self::TupleArityMismatch { names, values, .. } => write!(
                f,
                "this binding names {names} value(s) and its initializer has \
                 {values}"
            ),
            // THE `position` CARRIES THE WHOLE PHRASE NOW. It used to be a noun
            // and this template appended " of a function with a body", which
            // read correctly for the two callers there were and produced "an
            // element of a tuple, which would nest an aggregate, of a function
            // with a body" for the third. A sentence a reader has to unpick is
            // a defect in a diagnostic.
            Self::TupleNotStorable { position, .. } => write!(
                f,
                "a tuple has no representation in this position: it cannot be {position}"
            ),
            Self::SpawnInsideAtomic { .. } => write!(
                f,
                "`spawn` may not appear inside an `atomic` region -- the \
                 spawned thread would block on the lock its parent holds"
            ),
            Self::ThreadValueNotRepresentable { result, .. } => write!(
                f,
                "`val()` cannot return `{}`; a thread's value has to be a \
                 scalar in this subset",
                result.name()
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
            Self::OverloadSetStaticParamsDiffer { name, .. } => write!(
                f,
                "declarations of `{name}` differ in their static parameters; an overload set \
                 is uniformly generic or uniformly ground"
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
