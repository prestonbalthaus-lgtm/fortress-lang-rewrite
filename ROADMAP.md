# Roadmap

Goal: a native Fortress compiler producing ELF binaries that run under Slurm,
linked against OpenMPI over InfiniBand. Work that serves that ships. Work that
does not gets cut.

Every phase has one exit criterion. The measure throughout is the 1956 `.fss`
and `.fsi` files already in this tree.

**THE "DIFFERENTIAL BASELINE AGAINST THE LEGACY INTERPRETER" IN PHASE 0 DOES NOT
EXIST AND NEVER WILL.** The JVM path was cancelled as a side effect of the
no-JVM decision and this file was never amended. The real oracle needs no JVM:
it is the 373 `.test` files the legacy implementation shipped, on disk, 266 of
them carrying the exact compile error 1.0 gave. `tools/oracle-gate.sh` is the
instrument. Phases 4 and 5 below inherit the dead reference in their exit
criteria; read "the legacy interpreter" as "the recorded `.test` set".

## Where the work actually is, 2026-08-19

Phases 1, 2 and 5 are done for a subset, and phase 6's C ABI half is done and
gated: a Fortress program calls MPI and runs as four ranks under `mpirun`,
inside an Apptainer image.

The rest of phase 6 and all of phase 8 are **shelved**. There is no cluster to
test on, and MPI is not what is blocking the language. Slurm, `sbatch`,
multi-node runs and the InfiniBand fabric wait until the compiler is finished.

Language completion was the plan, in this order, and all six are now done:

1. **Memory.** Boehm collector. `specs/2026-08-18-m3a-memory.md`.
2. **Arrays and iteration.** `specs/2026-08-18-m3b-arrays.md`. `Array[\T\]`,
   `ZZ64` subscripts, bounds checking, `while`, mutable bindings. It forced the
   scannable allocator, as expected.
3. **Traits and symmetric multiple dispatch.**
   `specs/2026-08-18-m3c-dispatch-design.md`. Whole-program enumeration of the
   concrete tuples reaching each overload set, in place of 1.0's modular rules.
4. **Generics, by monomorphization.**
   `specs/2026-08-19-m3d-generics-design.md`. Concrete copies at compile time,
   expanded to a fixpoint before the type checker exists so the dispatch tables
   are built against a closed world.

Held back deliberately: `for`, generators, comprehensions and reductions. `for`
is parallel by default and cannot be faked with a counter, so it belongs with
phase 7 rather than with iteration.

**A measurement that changed the plan.** M3d was expected to open the corpus,
because 737 of the 1956 files use `[\...\]`. It does not. Erasing every static
argument from all 737 and re-running the compiler -- simulating generics that
parse perfectly and cost nothing -- got ten more files past the parser. The wall
was the *lexer*: 319 of those 737 died on `|` and `=>`, and every load-bearing
library file was a lexer casualty. Clearing that took the lexer from 1277 to
1780 of 1956 and the parser from 84 to 154, for about thirty lines of code, and
generics then took the parser to 168.

The lesson is recorded rather than buried: count what the compiler actually does,
not what the blocker histogram implies. The same estimate done by counting was
wrong by an order of magnitude one milestone earlier.

5. **The unit type `()`, with syntax for tuples and arrows.**
   `specs/2026-08-19-m3e-unit-tuple-arrow-design.md`. `()` full stack onto the
   `Type::Void` that already existed; tuple types, tuple expressions and arrow
   types parsed and refused with a diagnostic, so `Type` stays `Copy`.

**And the lesson paid for itself a third time.** "Tuple and arrow types" was
billed here as the top parser blocker, at 536 files. It was the right number
attached to the wrong name: 485 of those 536 were `()`, the unit type, which the
type system already had. Measured by construct rather than by first-blocker
count -- spike it, run the real driver, revert -- unit was worth +232, tuples
+15, arrows +13.

Parser 168 -> 428 of the 1780 files that lex, 9.4% -> 24.0%, the largest single
parser move in the project. Files that compile end to end 52 -> 151, which the
design document had predicted would barely move.

Two things fell out of the sweep that runs over all 1956 files with the real
driver. A void-valued binding (`x = println("hi")`) typechecked and then died in
codegen as an internal error, exit 70, on ordinary source. And `run(args:String)`
generated a module LLVM rejected, because the generated `main` calls `run` with
no arguments. Both are diagnostics now. Neither was reachable before, because
every file carrying them died at the parser on `()` first.

6. **Juxtaposition as function application, and chained comparison.**
   `specs/2026-08-19-m3f-application-chaining-design.md`. `println "Hello"` is a
   call; `a < b < c` is a chain whose interior operands evaluate once.

**And the histogram was wrong a fourth time, which makes it a pattern.** Eleven
candidate constructs were spiked behind an environment switch, one switch each,
and measured against the real corpus. `var` bindings carry 105 first-blockers
and are worth **6** files. `opr` declarations carry 97 and are worth **5**.
Chained `=` is a footnote at 51 blockers and is worth **49**, a 96% conversion.
The named front-runners were the two worst buys on the board.

The compile metric turned out to be a different lever again. Of the 297 files
that parsed and did not compile, the largest single cause was `unknown name
println`, 48 files -- not a missing builtin, but `println "Hello"`:
**juxtaposition as function application**, spec rule (c) at
`juxtameaning.tex:44-46`. That alone was compile 151 -> 181.

Parser 428 -> 476 of the 1780 that lex, 24.0% -> 26.7%. Files that compile end
to end 151 -> 187. The one file the parser *lost* is
`ProjectFortress/parser_tests/XXXchain1.fss`, whose own source says
`(* SHOULD NOT PARSE *)`: it is the legacy suite's negative test for the rule
that a chain may not mix two senses of ordering, and it is now refused by name.

The full-corpus driver sweep earned its keep a second time. Two files exited
**101**, a Rust panic: an integer literal that takes `RR64` from context was
still lowered as an i64, so `halve(x: RR64): RR64 = x/2` -- ordinary Fortress --
reached `arith` where it requires a float. Pre-existing, unreachable until
`println (halve(3.5))` started parsing as a call.

Two rules were spiked, measured at **zero** corpus files, and handled
differently on purpose. The specification's n-ary juxtaposition reassociation is
a real algorithm and is refused with a diagnostic. `f ()` as the zero-argument
call is four lines and is *kept*, because it makes valid Fortress compile rather
than because it moves a number.

**What is in front now.** The remaining nine constructs, blocker count against
measured parse delta, so the trap is visible: `getter`/`setter` 131 -> +31,
top-level value declarations 113 -> +31, `self` parameters 46 -> +25,
`import java com...` 34 -> +25, object expressions 19 -> +13, dotted export
names 21 -> +12, untyped parameters 32 -> +9, `var` bindings 105 -> **+6**,
`opr` declarations 97 -> **+5**, enclosing operators 30 -> +5. They are
superadditive: getter + `self` + top-level values together were +98, and all
nine plus this milestone's two were +281.

## Phases

**0. Baseline.** ~~Get the legacy interpreter building and running.~~
**CANCELLED, and the exit was met another way.** Running the JVM implementation
was dropped with the no-JVM decision. The pass/fail set is already in the repo
and always was: 373 `.test` files, 266 with the exact legacy compile error.
*Exit, as met:* `tools/oracle-gate.sh` reads that set, builds and RUNS every
compiling corpus file, and carries a must-fail ratchet. It needs no JVM.

**1. Lexer.** `logos` based, newline aware (see decision 2).
*Exit:* tokenizes all 1950 corpus files without panicking, with token counts
stable across runs.

**2. Parser.** Recursive descent over the core grammar, ported from the 27
`.rats` modules under `ProjectFortress/src/com/sun/fortress/parser/`.
*Exit:* parses 90% of the corpus to an AST. The remaining 10% is catalogued with
a reason each.
*Where it is, re-measured 2026-08-24:* 1909 of 1956 lex (98%), 1161 of those
parse (61%). Both numbers are ratchets in the corpus tests rather than
commentary, so a regression fails the build. This line read "1845 lex, 839
parse" until it was re-run; a `.rats` port advances in small named steps and the
prose does not follow on its own.

*What is left is a PARSER queue and not a type-system one.* The 1382 files that
do not parse are catalogued by `tools/triage.sh`, and the top buckets are all
grammar: an all-caps operator word used as a name, `grammar` and the other
syntax-abstraction reserved words, trait value parameters, untyped parameters.
Two traps this phase has now paid for twice. A first-blocker count is not a
ceiling -- it says what the compiler hit FIRST, and this project's counts have
been wrong by up to 20x in both directions -- so the cheapest way to turn one
into a ceiling is to PARSE the construct and refuse it BY NAME, then re-measure.
And a feature that makes an INVALID program compile shows up as a gain, so read
what a gained file compiles TO, never just the count.

**3. Names and modules.** Component and API resolution, imports, scoping.
*Exit:* `Library/` resolves clean with no unresolved references.
*Where it is, 2026-08-23:* the resolver merges an api's TYPES transitively, into
apis and now into COMPONENTS as well -- `source-code.tex:305`'s implicit core
import, both halves. `unknown type` as a first blocker fell from 93 corpus files
to 26. What an api's FUNCTIONS and VALUES declare is still not merged, and is
not meant to be: `source-code.tex:313-320` makes satisfying them the importing
component's obligation. See
`docs/superpowers/specs/2026-08-23-link-5-component-side-core-import.md`.

**4. Types.** Traits, polymorphism and overload resolution. The legacy
implementation never finished this, so the specification is the authority here,
not the old behaviour.

*This line said "Hindley-Milner inference" until 2026-08-21 and that was never
true of this compiler.* There is no HM engine and no ADT resolution: `unify`,
`occurs_check` and `TypeVar` have zero hits across every crate and `struct
Substitution` has none either (the three bare `Substitution` hits in `mono.rs`
are comment prose about monomorphization),
and `Type` (`crates/types/src/types.rs:161-227`) is a `Copy` enum with **no
variable case** -- twelve variants as of 2026-08-22, none of them a variable --
so there is nowhere for an inference variable to live. What
exists is bidirectional checking (`expected: Option<Type>`, which pins literals
and asserts subtyping and never converts anything) over a monomorphizer that
requires every static argument to be written. **Phase 4 is a build, not an
extension** -- which is why decision 4 asks for it to be split before it starts.
See `docs/superpowers/specs/2026-08-21-d6-phase4-split.md`.
*Exit:* type checks `Library/` and the corpus, disagreements with the recorded
`.test` set documented rather than silently matched.

**5. Codegen, sequential.** LLVM IR via `inkwell`. No parallelism yet.
*Exit:* hello world plus the single threaded half of the corpus compiles, links
and produces the output the recorded `.test` set gives.

**6. Runtime and the C ABI.** Memory management -- **DECIDED: Boehm-Demers-Weiser,
landed, linked into every binary** (`docs/superpowers/specs/2026-08-18-m3a-memory.md`)
-- the `extern "C"` boundary, OpenMPI linkage.
*Exit:* a Fortress program calls `MPI_Init` and `MPI_Comm_size` and returns the
right rank count on two nodes.

**ANONYMOUS OBJECTS, LANDED 2026-08-23.** `object ... end` in expression
position, hoisted the way a `fn` already was: a minted top-level `ObjectDecl`
whose value parameters are the locals its members read, and a construction of it
left behind, so no member body is rewritten and codegen gained nothing. 423
objects and 126 apis, +7 and nothing lost, and the oracle pass floor moves 345
-> 348. A CAPTURE COPIES, so closing over a local declared `:=` is now REFUSED
BY NAME at both hoists -- reading one printed the value as of construction and
exited 0, and 1.0 captures the cell. Measured at zero corpus files.
`objectCC_mutVar1.fss` is 1.0's own test for the cell semantics and is still
blocked earlier, on `object O(var v: ZZ32)`.

**`var` VALUE PARAMETERS AND `:=` FIELD INITIALIZERS, LANDED 2026-08-23.** Two
halves of one grammar rule. `AbsVarMod` is legal in an OBJECT's parameter list
and nowhere else -- an object's value parameters ARE its fields -- and
`InitVal = ("=" / ":=")` (`Variable.rats:37`) makes `:=` a field initializer
that also declares the field mutable. 426 objects, 126 apis, oracle 350. The
flag has to survive monomorphization: `mono::params` rebuilt every `Param` and
defaulted it, so `object O(var v: ZZ32)` parsed and the assignment then reported
`v` immutable. Five mutation rows, one per axis.

**RULE 3 RETIRED AND THE MEET RULE LANDED, 2026-08-23.** A component sees the
merged declarations that discharge its own generic's obligations. Three parts,
and the middle one is what the other two were waiting on.

`Library/CompilerAlgebra.fss` declares `trait Equality[\T\]` with
`opr =(self, other: T)`. `AnyMaybe` reaches `Equality` twice -- directly and
through `AnyUniqueItem` -- so monomorphization stamps that one line at both
instantiations and every `Just` inherits two `=`. The declaration that settles
it is `FortressLibrary.fsi:896`'s BODILESS `opr =(self, other:AnyMaybe)`, and a
component could not see it.

1. **The concatenation fallback tested for the NAME.** `opr ||(a: Blob, b:
   Blob)` anywhere in scope took `"U" || "x"` away and reported `expected Blob,
   found String`, because with one candidate `agreed` hands position 0 that
   candidate's parameter type. It tests APPLICABILITY now, in both directions:
   a pair of Strings no declaration reaches goes to the builtin, and a pair the
   builtin cannot concatenate goes to a declaration that reaches it. THIS ONE
   DEFECT WAS THE WHOLE COST OF RULE 3 -- 25 files, measured by list diff.
2. **A merged functional method is LIFTED.** With (1) in, it costs zero files.
3. **THE MEET RULE.** `advanced/overloading.tex:396-411` is discharged by a
   declaration applicable to the meet and more specific than both, and it does
   not ask that one for a body. `typing_candidates` takes implementations
   first, which is right for a CALL and wrong for a declaration SET, so the
   full set is consulted to RESOLVE a tie and never to create one -- an
   inherited requirement still cannot tie with the implementation beneath it.
   A bodiless meet makes the SET valid; the CALL is still refused, because
   dispatch has no target.

426 objects, 126 apis, oracle 350 -- all unchanged. Twenty-five files moved onto
a LATER blocker and `unknown name X` became `no declaration of X applies to
(T)`, which is the lift becoming visible.

**LIST COMPREHENSIONS AND `List[\T\]`, LANDED 2026-08-23. ROUTE 4.**
`<|[\T\] e | x <- lo:hi, p |>` lowers onto a REAL MONOMORPHIZED `List[\T\]`.

The collection is written in Fortress -- `crates/types/src/List.fss`, embedded
with `include_str!` -- and MINTED into the component that used a comprehension,
the way `closure.rs` mints an arrow trait. Expansion then stamps one `List` per
element type and nothing in codegen had to learn a collection.

ONE ALLOCATION PATH, BY CONSTRUCTION. Storage is an ordinary `Array[\T\]` and
growth is `array(2 n + 8)` plus a copy, so every byte still comes from
`fortress_array_alloc`. No new shim, no second allocator, no two-pass hack and
no pre-size-and-fill. Amortised because the bound is `length(store)`: the value
parameter records the capacity the list was BUILT with, so comparing against it
would grow on every append after the first.

SEQUENTIAL, A NAMED DEVIATION. 1.0 defines a comprehension as a `BIG` reduction,
parallel unless every generator is `seq`. The lowering emits a `while` rather
than a `for` for two halves of one reason: a `for` body is OUTLINED and its
iterations may run on several workers, so appending to one shared list would be
a data race, and a list comprehension's ORDER is defined by its generator. The
parallel version needs an associative CONCAT reduction over a list monoid --
a milestone, not a lowering.

THE ELEMENT TYPE IS WRITTEN, NEVER INFERRED, like every other static argument
here: on the comprehension (`<|[\ZZ64\] ... |>`, which is 1.0's own spelling at
`parser_tests/XXXPreparser.ad.fss`), or on the slot it fills -- a binding, a
field, or a function's return type. Neither, and it is refused by name.

Ranges (`lo:hi` and `lo#n`), guards, several generators, and a generic function
whose `List[\T\]` is stamped per instantiation all work. 426 objects and 126
apis, UNCHANGED: nine corpus files moved onto a more specific diagnostic and
none onto the compile list, because every corpus comprehension is a set or map
one or ranges over a collection. Set, map and array comprehensions, and a
generator over a collection, are refused by name.

**A GENERATOR OVER A COLLECTION LANDED 2026-08-24**, one section below. The
last sentence above is superseded for that one case and still true for set, map
and array comprehensions.

**ARITY FLATTENING AND THE NON-MATERIALISING CALLING CONVENTION, LANDED
2026-08-23.** `overloading.tex:125` -- "Recall that a functional has a single
parameter, which may be a tuple". So `f(x: (A,B))` and `f(a: A, b: B)` ARE ONE
DECLARATION, and the honest way to have the first is to lower it into the
second. A tuple-typed name becomes SEVERAL names, `x$0` and `x$1`, and a tuple
is never built, stored or passed: there is nothing to box because nothing is
ever whole. An AST-to-AST pass ahead of expansion, because it changes ARITIES
and every signature the registry is built from has to be the flattened one.

Parameters, component-level values, local bindings, `(a,b) = x`, `f(t)` and
`f((p,q))` all flatten. A whole tuple is spread into a call ONLY where a
declaration of that arity exists; otherwise it is written back out as a tuple
and the checker refuses it for what it is -- spreading unconditionally reported
`o takes 1 argument(s), found 4` on `o(x: Any)`, an arity nobody wrote, and
`println(t)` the same.

432 objects and 126 apis, +6 and nothing lost. Oracle 350 -> 356. ALL 426
PRE-EXISTING OBJECTS EMIT BYTE-IDENTICAL IR, measured file by file with two
binaries and the instrument self-tested both ways first.

REFUSED BY NAME, each because it needs the half this milestone does not build:
a tuple RESULT (the callee would have to hand back several values -- an
aggregate return, and `(left, right) = split()` is its two corpus witnesses;
**LANDED 2026-08-24, two sections below**); a MUTABLE tuple local; a tuple binding whose initialiser is neither a written
tuple nor another flattened name; a NESTED tuple type; an object's tuple-typed
value parameter or field, because those decide a layout; and a flattened name
used as anything but a whole argument or a destructuring's right-hand side.

**A BINDING CONDITION PARSES, 2026-08-23.** `if x <- g then ... end` and
`while (a,b) <- g do ... end`. `DelimitedExpr.rats:37,39,40,216` makes the
condition a `GeneratorClause` and NOT an expression, so the decision needs
LOOKAHEAD -- a `<-` at depth zero before the closing keyword -- and without it
`if x <- g` reads `x < -g` and reports `expected then, found Lt`, which is what
27 corpus files were doing. `then` is OPTIONAL and MAY SIT ON THE NEXT LINE;
nine of the 27 write it there. `elif` takes one too.

THE LOWERING WAS REFUSED BY NAME AND LANDED ON 2026-08-24; see the next
section. What is refused now is the SOURCE, not the construct: a binding
condition iterates a `Condition[\E\]`, and a `ZZ32` is not one.

432 objects and 126 apis, UNCHANGED, and the triage bucket predicted exactly
that -- `generator-bindings` had 27 first blockers and an `alone*` ceiling of
ZERO. `expected then, found Lt` goes from 27 files to NONE and exactly ONE
lands on the lowering; the other 26 walk on to a later wall, as far as
`println does not accept Just$Boolean$e`.

**THE GENERATOR PROTOCOL, LANDED 2026-08-24. `Indexed`, EXTERNALLY.**
`for x <- someCollection`, a comprehension over a collection, and the binding
condition's lowering -- the three milestones that all stopped here.

IT IS NOT 1.0's `generate`, AND THAT IS MEASURED RATHER THAN CHOSEN.
`Specification/advanced/parallelism-locality/defining-generators.tex` makes the
protocol `generate[\R\](r: Reduction[\R\], body: E->R): R`, with
`loop(f) = generate[\()\](VoidReduction, f)` as the specialisation a `for`
desugars through. Three things block that form here:

  * there is no first-class `Reduction`. `TypedReduction` and
    `fortress_reduction_alloc` are a compiler-recognised SHAPE over ZZ32/ZZ64/
    RR64 accumulators, not an object a program can pass, so `generate` cannot
    be given its first argument
  * a `()` arrow CODOMAIN is refused by name, and that is the arrow `loop`
    takes -- not an edge case for this protocol but the whole of it
  * a COMPONENT cannot name `Generator`, `Indexed` or `Condition`: the implicit
    core-api import is api-side only and Link 5 is architecturally out. So
    NOMINAL membership in the protocol is unavailable from a `.fss` whatever
    the protocol is, which is the decisive one -- it forces a structural check.

THE MEMBERS ARE 1.0's OWN. `Library/FortressLibrary.fsi:1205`'s
`trait Indexed[\E, I\]` declares `getter size()` and `opr [i: I]: E`, and its
own doc comment is the licence for walking it by index: "`self[i] = v`",
"stripping away the `i` yields exactly the results of `v <- self`". Both
spellings of `size` are accepted because both are real Fortress -- 1.0 declares
the getter, the minted `List[\T\]` declares the plain method -- and the checker
reads `accessors` to know which this component wrote. The extent is ZZ64 and
not `Indexed`'s ZZ32, for the reason array subscripts are: the JVM's 2^31
ceiling is why this rewrite exists.

CUTTING THE PROTOCOL DOWN IS 1.0's OWN PRECEDENT, in this repository.
`Library/CompilerLibrary.fsi` is 1.0's NATIVE-compiler library, as opposed to
the interpreter's `Library/FortressLibrary.*`. It throws the generic
`Generator[\E\]` away and declares a MONOMORPHIC `trait GeneratorZZ32` whose
`generate` is overloaded at two ground result types instead of generic in `R`,
with no `map`, `nest`, `cross`, `mapReduce`, `reduce` or `reverse`; `Reduction`
collapses to two ground traits carrying only `empty` and `join`.

`opr []` NOW DISPATCHES ON AN OBJECT, and that one change makes the element half
free. The declaration already parsed -- registered as `[_]`, with `[_]:=` as its
sibling -- and only the USE was refused. Every desugar below writes `src[i]`,
which means the array subscript on an array and the object's own declaration on
an object, with nothing choosing between them.

ONE RESOLVER, `generator_extent`, WITH EXACTLY TWO CALLERS:

  * `for x <- g`. The ARRAY path is unchanged byte for byte -- 432 pre-existing
    objects, identical IR, instrument self-tested both ways. Rank above one
    keeps its refusal.
  * a comprehension over a collection. `comprehension.rs` runs before there are
    any types, so it emits `Expr::SeqIterate` and the CHECKER lowers it -- a
    `while` and NOT a `for`, because a `for` body is outlined and may run on
    several workers and a comprehension appends to one shared `List`.

THE BINDING CONDITION IS A DIFFERENT AND SMALLER PROTOCOL. `Condition[\E\]`
(`Library/FortressLibrary.fsi:847`) is "a generator that generates 0 or 1
element" and declares `holds` and `get`. 1.0 desugars through
`__cond(e, fn (binds) => B, thunk(C))`, whose arrows both have the refused `()`
codomain; the direct lowering needs no arrow at all. The `while` form
RE-EVALUATES its source once per round, which is what makes it a
while-CONDITION rather than a walk over one value.

432 objects and 126 apis, UNCHANGED, AND THAT WAS PREDICTED. 172 corpus files
write a generator construct and 144 of them die in the PARSER; almost all the
rest import a `Library` module whose `.fss` does not compile, and an imported
object is declared by an api and never defined. So the protocol is NECESSARY
for all 172 and SUFFICIENT for none: it is a prerequisite behind Link 5, not a
lever, and it was built with that written down first. Three corpus files walked
off the comprehension refusal onto `ComprehensionListTaken`, which is the next
wall and is named in the state file.

`tools/generator-gate.sh`, 44 checks and 6 mutation rows. THE ORDER IS THE
ASSERTION throughout: an exit code cannot tell an ordered walk from a shuffled
one, and one mutation row makes the minted `List`'s `opr []` ignore its index --
a silent wrong answer only a value assertion catches.

AND A DEFECT THE GATE FOUND THAT NO SCRATCH-DIRECTORY PROBE COULD HAVE.
`dispatch_method` took its argument hints from every candidate of the right
ARITY. Inside the repo the core apis resolve, so `opr [i: ZZ64]` on an object is
one `[_]` among every `[_]` they declare, `agreed` found no agreement at the
index position, an integer literal fell back to `ZZ32`, and `r[0]` reported
`no declaration of [_] applies to (Row, ZZ32)` -- in any component ON the source
path and nowhere else. The hint pool is narrowed by the RECEIVER now, which is
already typed and can only make a hint more specific.
See docs/superpowers/specs/2026-08-24-generator-protocol-indexed.md.

**MULTI-VALUE RETURN, LANDED 2026-08-24. THE OTHER HALF OF THE CALLING
CONVENTION.** `f(): (ZZ64, ZZ64)` lowers to `{i64, i64} @f()`, built with
`insertvalue` and taken apart with `extractvalue` at the call.

IT IS STILL NON-MATERIALISING, and that distinction is the whole milestone: the
aggregate lives in SSA registers. No `fortress_alloc`, no GC block, no 32-bit
tag, no `alloca`. The allocation rule -- one path, through `fortress_alloc` --
is not touched, because nothing is allocated. LLVM's own ABI lowering decides
whether the pair travels in two registers or through a hidden pointer, and that
decision belongs to the target rather than to this compiler. `tuple-gate` reads
both facts off the emitted IR rather than taking them from a comment.

`Specification/basic/types-vals-vars.tex:246-284` is what the shape follows: a
tuple type is "a parenthesized, comma-separated list of TWO OR MORE types",
tuple types are COVARIANT, and a tuple type "excludes every tuple type that does
not have the same number of element types" -- which is why an arity disagreement
between a result and its binder is a refusal and not a truncation.

THREE TOUCHPOINTS, EACH NARROW ON PURPOSE:

  * `Expr::Tuple` is taken only where a tuple is EXPECTED, and there is exactly
    one such position -- the result of a function whose declared result is a
    tuple. `println(t)`, `t.m()` and `typecase (x,y)` keep the refusal they had,
    because the expectation is the gate: with no written tuple result nothing
    hands one down.
  * the result-side `tuple_free` becomes `representable_tuple`. A NESTED tuple
    is refused rather than lowered to a nested struct -- it would make the ABI
    decision depend on a type's shape two levels down -- and a `()` element
    keeps `VoidNotStorable`'s own wording, as it does at every other boundary.
  * `(a, b) = f(...)` gets its own typed variant, `TupleDestructure`, and NOT
    `TupleBinding`. That one carries one expression per name, which is right when
    the source WROTE a tuple; here the call must happen ONCE. It is also why
    this cannot be done in `tuple.rs`: that pass has no types, so splitting the
    binder there would either duplicate the call or need a whole-tuple
    temporary, and a temporary is the representation this backend does not have.

AND A REFUSAL THIS MILESTONE HAD TO ADD RATHER THAN REMOVE. `t = split()` bound
a whole aggregate to one local and COMPILED -- nothing on the plain-binding path
asked, and it was inert only as long as nothing read `t`. An accidental
capability is exactly what a subset boundary exists to keep out, so it is refused
by name with the destructuring spelled out.

434 objects and 126 apis, +2 and nothing lost: `tupleTypeParam.fss` and
`Expr.VarRef.fss`. THE ESTIMATE WAS "ROUGHLY TEN" AND THE ANSWER IS TWO, and
that was said in the design document before the build rather than discovered
after it. The other named witnesses walk onto later walls -- a missing `DIV`, a
generic `split` whose result type disagrees with its declaration, and `only a
variable or an array element can be assigned to`. The tuple first-blocker list
went 53 -> 32, so 21 files came off it and two onto the compile list: a
wall-unstacking milestone. ALL 432 PRE-EXISTING OBJECTS EMIT BYTE-IDENTICAL IR.
See docs/superpowers/specs/2026-08-24-multi-value-return.md.

**A WRAPPED VALUE-PARAMETER LIST, 2026-08-24.** An object's value-parameter
list may begin on the line AFTER the header, which is what
`Library/GeneratorLibrary.fsi:131`, `Random.fsi:211` and `Sparse.fsi:28` write
once the static parameters have made the first line long. ONE
`skip_newlines_before(&Kind::LParen)`, the same exception the STATIC parameter
list already had one line above.

435 objects and 134 apis, +9 and nothing lost. `API_FLOOR` moves 126 -> 134 and
all 434 pre-existing objects emit byte-identical IR.

MEASURED FIRST, AND THE BUCKET WAS THREE FEATURES. `expected a field or method
name, found LParen` was 29 corpus files; reading all 29 gave 8 wrapped parameter
lists, 16 `trait T(a:ZZ32, b:String)` -- trait VALUE parameters, a
pattern-matching feature, almost all under `parser_tests` -- and 2 tuple
bindings written as an object's first member. Only the first is built.

EIGHT OF THE NINE GAINS ARE `BirdyLib/*.fsi`, AND THAT IS THE FINDING. They were
first-blocked on `unknown type DefaultGeneratorImplementation`, a trait declared
in `Library/GeneratorLibrary.fsi` -- a file that did not PARSE. The resolver
takes an imported api's declarations after PARSING it, so a CHECK error in an
imported api does not block its importer and a PARSE error does.
`GeneratorLibrary.fsi` still fails to check and its importers no longer care.
**A diagnostic that names a missing type is not always about the type: ask
whether the file declaring it parsed.**

AND IT UNCOVERED THE NEXT WALL WITHOUT CLOSING IT. `GeneratorLibrary.fsi`
declares its own `trait ReductionWithZeroes[\R\]` while the implicit core
import brings in `FortressLibrary`'s `[\R,L\]`, and `FortressLibrary`'s OWN
references to that name are re-resolved in the IMPORTER's scope, where the
file's one-parameter declaration wins -- `ReductionWithZeroes takes 1 static
argument(s), found 2`, reported against a line that does not mention it. That is
a NAME-RESOLUTION milestone, the same family as Link 5: a merged declaration's
type references belong to the api that declared them.

**EXCEPTIONS, PARKED 2026-08-23.** `throw` is built -- an uncaught throw halts,
naming the exception, with no unwinding and no cost on the path that does not
throw -- and `try`/`catch`/`forbid`/`finally` PARSE and are refused by name. The
Result-style tagged-union LOWERING is parked: with the parse in, the measured
ceiling is FOUR corpus files, all four exception tests, against a
throwing-function fixpoint over the call graph, an ABI change on every function
that can throw, `finally` on both edges and a rule for a throw inside an
outlined parallel body. The design is written and stays valid --
`docs/superpowers/specs/2026-08-23-exceptions-design.md`, and the exception
object's existing 32-bit tag is the discriminator. Unpark it when something
raises the ceiling.

**7. Parallelism. PASSED.** Parallel `for`, `atomic`, `spawn`, `also`,
generators and reductions lowered to real threads.
*Exit, MET and gated by `tools/phase7-gate.sh`:* a parallel reduction over 10^9
elements beats the sequential version on one node -- 0.80 s at one worker to
0.09 s at fourteen -- and `ZZ64` indexing works past 2^31: index 2,999,999,999
of a three-billion-element `Array[\Boolean\]` is written and read. That second
half is the reason the rewrite exists.

**8. Cluster shipping.** Apptainer image, Slurm batch scripts, AVX-512 tuning
for the Platinum 8160s.
*Exit:* a Fortress job runs across 4 nodes under `sbatch` and the numbers hold.

## Decisions to make before writing code

**1. The macro tier. Measured, and the answer is cut it.** Fortress has user
definable syntax. `Syntax.rats` is the macro language and `templateparser/` is
another 28 grammar files serving it. A `logos` plus recursive descent frontend
cannot do user extensible grammar cheaply, so the question was how much real code
depends on it. Counted across all 1956 `.fss` and `.fsi` files:

* 34 files declare a `grammar`. Every one is in
  `ProjectFortress/syntax_abstraction_tests/` (110 files total with its consumer
  cases), which is the feature testing itself.
* `Library/` has 126 source files and zero grammar declarations.
* Three files in `Library/` touch the macro APIs (`FortressSyntax.fsi`,
  `FortressAstUtil.fss`, `FortressAstUtil.fsi`, 218 lines together). They import
  each other and nothing else in `Library/` imports them. `FortressLibrary.fss`
  does not.

So the standard library does not use syntax abstraction at all. Cutting it from
v1 costs the 110 test files and those 218 lines. Nothing else breaks.

Two things that follow. The 110 files come out of the conformance denominator, so
corpus percentages in phases 1 and 2 should be quoted against 1846, not 1956. And
the specification still documents the feature, so v1 is a Fortress dialect rather
than the whole language. Say so in the README when v1 ships.

**THE 1846 DENOMINATOR WAS NEVER ADOPTED AND SHOULD NOT BE.** `grep -rn 1846`
across `tools/` and `fortressc/crates` returns nothing: every instrument in the
tree walks and quotes against 1956, and the syntax-abstraction files are counted
like any other. Changing it now would move every recorded number in this repo
against a denominator no gate uses. The 110 files ARE out of scope for v1
conformance; they are not out of the corpus walk. Quote 1956.

**2. Whitespace and newlines.** The grammar has dedicated `Spacing`,
`NoSpaceLiteral`, `MayNewlineHeader` and `NoNewlineHeader` modules. Newlines are
significant and spacing changes how expressions parse. The lexer needs an
explicit newline and layout layer. This is a constraint, not an option.

**3. ASCII core with a Unicode alias layer. Settled.** The grammar stays ASCII.
Every construct has an ASCII spelling that always works, and Unicode spellings
are aliases on top. `SUM` is the name; `∑` is another name for the same thing.
Nobody is required to type a character they cannot produce on a keyboard, and a
file that uses only ASCII is always valid.

This splits across two layers and only one of them is the library.

*Lexer:* an explicit allowlist of codepoints legal in identifiers and operators.
This part cannot live in a library, because a library cannot bind a character the
lexer refuses to tokenize. The allowlist is a table, not grammar extension, so it
stays cheap.

*Library:* the actual bindings, written in Fortress, in `Fortress.Math.Unicode`
per `02-stack.md`. Zero compiler involvement. Adding a symbol is a library commit.

The allowlist is the important half of this decision, and it is deliberately not
what Sun did. `parser/Unicode.rats` in this tree is a mechanical dump of Unicode
5.0 `ID_Start` and `ID_Continue`, generated by
`parser_util/unicode.id.codes.pl` from `UnicodeData.500.txt`. That is every
script, CJK, Devanagari, surrogate pairs for the math alphanumeric block, the
whole thing. It is also from 2008 and Unicode has moved 11 major versions since,
so it is a maintenance treadmill. It forced Sun to write
`useful/UnicodeCollisions.java` to hunt homoglyph collisions after the fact.

A curated allowlist of mathematical codepoints avoids all of that. Pick
characters with no decomposition and normalization never has to run. Pick no
confusable pairs and the homoglyph problem is gone by construction rather than by
detection. The list is auditable because a human wrote it.

Reusable from the legacy tree: `unicode/NamedXForm.java` is the ASCII name to
symbol alias mechanism, and `tests/unicodeTest.fss` is a test case that already
exists.

**4. v1 language scope. Settled: everything ships.** The old README listed 16
features the Sun implementation never finished. All of them are v1:

reduction variables, distributions, `ZZ64` indexing, non-`RR64` floats, bits and
storage types, integers beyond `ZZ32`/`ZZ64`, dimensions and units, keyword
arguments, where clauses, coercion, modifiers, radix numerals, the types that
classify operator properties, constraint solving for `nat` parameters, static
arguments (`nat` with minus, `int`, `bool`, `dimension`, `unit`), and Unicode
names.

The call was made deliberately with the cost known: v1 is now the complete 1.0
specification minus syntax abstraction, and Sun did not finish this list in five
years with a funded team. What it buys is that nothing gets retrofitted into the
type system later, which is the expensive direction.

Two consequences for the plan above.

Phase 4 is no longer one phase. Dimensions and units, coercion, where clauses and
`nat` constraint solving are four separate inference problems that happen to live
in the same checker, and unit algebra in particular has to survive inference
rather than being checked after it. Split phase 4 before starting it and give
each part its own exit criterion.

**AMENDED 2026-08-21, D7 ADOPTED.** Two of those four have answers now and the
line above bundles two items with OPPOSITE demand, which is what D7 §4 found:

* **`nat`/`int`/`bool` static parameters and arguments ARE IMPLEMENTED.** A value
  parameter is substituted with a number; a static argument must be statically
  evaluable — a literal, an enclosing value parameter, or `+`, `-` and
  juxtaposition-as-product over those. Evaluation happens at the substitution, so
  `[\2 + 3\]` and `[\5\]` are one stamp against `MAX_INSTANTIATIONS`.
* **"Constraint solving for `nat` parameters" HAS ZERO DEMAND AND IS NOT BUILT.**
  Measured: not one `where { k < n }` exists in 1956 corpus files, while `nat`
  PARAMETERS have 61 files and 842 sites. A bound on a value parameter is refused
  by name rather than dropped. Re-open it when a corpus file writes one.
* `unit` and `dim` stay in v1 and stay deferred to sub-phase 4d, gated on
  `SPIKE-COMPOSITE-TYPE` rather than on D7 — `unit` is 6 corpus files and zero
  library files, `dim` has no corpus witness at all.
* `NatReflect.reflect`, which turns a run-time `ZZ32` into a static parameter, is
  a **named deviation** refused by name: a monomorphizing compiler cannot stamp a
  specialisation for a value it does not know.

Unicode names are now scoped by decision 3 rather than contradicting it. The item
means Unicode spellings drawn from a curated allowlist, aliased to ASCII names in
the library. It does not mean Sun's full `ID_Start` and `ID_Continue` sets, and
arbitrary Unicode identifiers are not in v1.

## Out of scope for v1

Eclipse and Emacs tooling, Fortify LaTeX rendering, the Vim files, and anything
in `contrib/`. The legacy interpreter stays as reference material and **IS NOT
TO BE DELETED.** This line used to say it "gets deleted once phase 7 passes";
phase 7 has passed, and executing that would destroy the measuring stick.
`ProjectFortress/` holds all 373 `.test` files, `LibraryBuiltin/`, and most of
the 1956-file corpus.
