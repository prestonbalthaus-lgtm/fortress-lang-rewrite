# Roadmap

Goal: a native Fortress compiler producing ELF binaries that run under Slurm,
linked against OpenMPI over InfiniBand. Work that serves that ships. Work that
does not gets cut.

Every phase has one exit criterion. The measure throughout is the 1956 `.fss`
and `.fsi` files already in this tree.

**THE "DIFFERENTIAL BASELINE AGAINST THE LEGACY INTERPRETER" IN PHASE 0 DOES NOT
EXIST AND NEVER WILL.** The JVM path was cancelled as a side effect of the
no-JVM decision and this file was never amended. The real oracle needs no JVM:
it is the 373 `.test` files the legacy implementation shipped, on disk, 264 of
them carrying the exact compile error 1.0 gave. THAT NUMBER WAS 266 HERE UNTIL
2026-08-25 AND 266 IS A DIFFERENT PREDICATE: `tools/oracle-gate.sh:761` selftests
`264 cases carry a non-empty compile_err_equals`, and 266 is the count under ANY
`compile_err_*` comparator, two files wider. "The exact compile error" is the
narrow one. `tools/oracle-gate.sh` is the instrument. Phases 4 and 5 below inherit the dead reference in their exit
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

**What is in front now.** *(M3f-era, 2026-08-19, AND FULLY SUPERSEDED -- kept
because the blocker-against-delta table is the evidence for the trap it names,
not because any of it is still in front. `getter`/`setter`, `self` parameters,
top-level value declarations, object expressions, `var` bindings and `opr`
declarations have all since landed; untyped parameters are parsed and refused by
name. AND IT SAYS NINE AND LISTS TEN, which nobody has ever noticed because
nobody re-added them. What is actually in front is in the G ledger under phase 2
and in `04-state.md`.)* The remaining nine constructs, blocker count against
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
*Exit:* tokenizes all 1956 corpus files without panicking, with token counts
stable across runs. THIS SAID 1950 FROM 2026-08-18 TO 2026-08-25 and 1950 is not
a denominator anything uses -- `grep -rn 1950 tools/ fortressc/crates` returns
nothing. It was a draft approximation that lost its tilde: the line above it read
"~1950" in the same commit, against a tree that already held exactly 1956.
*Where it is:* 1909 of 1956 lex. THE RATCHET IS 1845 AND NOT 1909
(`crates/lexer/tests/corpus.rs:139`), so a lex regression of up to sixty-four
files passes the build in silence. Raising it is a one-line edit nobody has
made.

**2. Parser.** Recursive descent over the core grammar, ported from the 27
`.rats` modules under `ProjectFortress/src/com/sun/fortress/parser/`.
*Exit:* parses 90% of the corpus to an AST. The remaining 10% is catalogued with
a reason each.
*Where it is, re-measured 2026-08-25:* 1909 of 1956 lex (98%), 1174 of those
parse (62%). THE PARSE HALF IS A RATCHET AND THE LEX HALF IS NOT, and this line
claimed both were until 2026-08-25. `crates/parser/tests/corpus.rs:261` asserts
`parsed >= 1174` and fails the build below it; the lexer's floor is 1845
(`crates/lexer/tests/corpus.rs:139`), sixty-four below the number quoted here.
Neither floor is a named constant -- both are inline literals inside an
`assert!`, which is why neither goes red when the prose drifts past it.
This line read "1845 lex, 839 parse" until it was re-run; a `.rats` port advances in small named steps and the
prose does not follow on its own.

*What is left is a PARSER queue and not a type-system one.* The 735 files that
do not parse -- 1909 that lex minus the 1174 that do -- are catalogued by
`tools/triage.sh`. THIS SAID "THE 1382 FILES THAT DO NOT PARSE" UNTIL 2026-08-25
AND IT WAS WRONG TWICE IN ONE SENTENCE. 1382 is not a parse denominator at all:
it is 1956 minus the 574 files that COMPILED at 5e061b97d, a whole-driver
refusal count carried across and relabelled. The driver's refusal count at this
tip is 1374, and neither number has ever been a parse figure.

The top buckets are still grammar, and the list this line used to give is what
they were before the G milestones cut into them. An all-caps operator word used
as a name: G3 paid the PREFIX form, which was most of it, and the bucket is
FIFTEEN rather than 54. `grammar` and the other syntax-abstraction reserved
words: out of v1 by decision 1. Trait value parameters: 1.0's pattern matching,
untouched. Untyped parameters: PARSED AND REFUSED BY NAME since G7 and I, which
is what turned a first-blocker count into a ceiling and showed the thing behind
it is whole-component inference rather than a parser gap.
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
import, both halves. The component half is LINK 5 and it LANDED, in bd76d11e3 on
2026-08-23, with four rules of which the first is that A MERGED DECLARATION
LOSES TO A BUILTIN OF ITS OWN NAME; `fortressc/tests/implicitbuiltin.fss` is
built and RUN by apply-gate to hold it. `unknown type` as a first blocker fell
from 93 corpus files to 26 then, and is TWELVE at this tip, re-measured
2026-08-25 with the driver over all 1956. What an api's FUNCTIONS and VALUES declare is still not merged, and is
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
-> 348. (EVERY ORACLE NUMBER IN THE MILESTONE SECTIONS IS A SNAPSHOT AT ITS OWN
TIP. The ladder runs 345 -> 348 -> 350 -> 356 down this file; the LIVE floor is
`PASS_FLOOR = 356` at `tools/oracle-gate.sh:329` and the pass count is 359 with
454 binaries built and run. Do not quote the first one you grep.) A CAPTURE COPIES, so closing over a local declared `:=` is now REFUSED
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
    core-api import is api-side only and Link 5 is architecturally out.

**THAT THIRD BULLET WAS FALSE WHEN IT WAS WRITTEN AND IS STRUCK, 2026-08-25.**
Link 5 landed the day BEFORE this section, in bd76d11e3, and the component half
of the implicit core import has been on ever since -- `implicit_import` is
called for every component and the api-only early return has been dead since
that commit. A component naming all three with no written import compiles: exit
0, seven apis resolved, probed at this tip. So NOMINAL membership was available
from a `.fss` the whole time, and this file said the opposite two sections
after phase 3 said the truth. The claim also propagated out of the repo into
`02-stack.md` and `04-state.md`, which both carried "Link 5 architecturally
out" as the largest remaining lever for a milestone already built.

WHAT SURVIVES IS THE DECISION, NOT ITS THIRD REASON. The structural check stays:
the first two bullets are untouched and each is sufficient on its own -- there
is no first-class `Reduction` to pass, and a `()` arrow codomain is refused by
name, which is the whole of `loop`. The protocol is `Indexed` walked externally
because `generate` cannot be given its arguments, not because the names could
not be spelled.

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
write a generator construct and 144 of them die in the PARSER (DATED 2026-08-24
AND NOT RE-MEASURED SINCE: parse has gone 1113 -> 1174 across G3, G4/G5, G7 and
J, so 144 is an upper bound at best. Re-measure it, and write the predicate down
this time); almost all the
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
all 434 pre-existing objects emit byte-identical IR. (Dated. G2 took `API_FLOOR`
to 135 the next day and the live constant is `tools/apply-gate.sh:441`; corpus
is 582 at this tip. Every count in a milestone section is what it was AT THAT
MILESTONE -- read the ratchets in `tools/` for what is true now.)

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
`GeneratorLibrary.fsi` did not check when this was written and its importers no
longer cared.
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

**CLOSED BY G2 THE NEXT DAY (ca9d55170), AND THIS SECTION SAID OTHERWISE UNTIL
2026-08-25.** `Library/GeneratorLibrary.fsi` CHECKS -- exit 0 against the binary
at this tip. The fix is parameter-COUNT scoping: a merged declaration that loses
a name collision to one taking a DIFFERENT NUMBER OF STATIC PARAMETERS keeps its
identity under `$<api>$<name>`, unwritable the way `$Self` is, and that api's own
references follow it. See the G2 section below. WHAT IS STILL OPEN is the wider
half and it is bigger: 6,921 collisions are the SAME parameter count with a
DIFFERENT BOUND VECTOR and are silently flattened.

**G2: A MERGED DECLARATION THAT LOSES TO A DIFFERENT ARITY KEEPS ITS OWN NAME,
2026-08-24.** `Library/GeneratorLibrary.fsi` CHECKS. Corpus 569 -> 570, zero
lost, 435 objects byte-identical IR.

The resolver merges an api's declarations into ONE FLAT list, so a name has one
meaning per component and a later api's same-named declaration is DROPPED.
`GeneratorLibrary.fsi:275` declares `ReductionWithZeroes[\R\]` and
`FortressLibrary.fsi:1871` declares `[\R,L\]`; six of the library's OWN objects
name it at two. Those six do not collide, so they merged. The declaration they
were written against did, so it did not, and their references re-resolved to the
IMPORTER's one-parameter declaration -- `takes 1 static argument(s), found 2`,
at a line that does not mention the name.

THE RULE IS A NARROWING, NOT A CHANGE OF WINNER. A merged declaration that loses
a collision is still dropped, EXCEPT where the two take a different NUMBER of
static parameters; then it keeps its identity under `$<api>$<name>`, unwritable
the way `$Self` is, and every reference from ITS OWN api follows it. Nobody
else's does.

AND THE CENSUS WAS RUN TWICE BECAUSE THE FIRST INSTRUMENT WAS THE WRONG ONE. It
compared `static_params.len()` and said 24 mismatches. That is not this
codebase's own definition of shape: `check_uniformity` (`mono.rs:1694-1699`)
compares count, each parameter's `bounds.len()` AND its kind, and its own
comment says count alone was a bug D7 already paid for. Re-measured with the
right predicate over the same 1956 files: 25,637 collisions reach the shape
check, 18,705 are IDENTICAL, 6,921 are the same count with DIFFERENT BOUNDS, and
just 11 differ in COUNT -- which is this rename's entire reach, across four
files. **THE 6,921 ARE STILL SILENTLY FLATTENED AND ARE THE BIGGER HALF.**

**G3: AN OPERATOR WORD IN EXPRESSION POSITION IS A PREFIX OPERATOR, 2026-08-24.**
parse 1113 -> 1133, corpus 570 -> 571, zero lost, 435 objects byte-identical IR.

THE LEXER WAS ALREADY RIGHT. `is_operator_word` is the specification's own rule
(`lexical-structure.tex:1167-1172`): all uppercase or `_`, no leading or
trailing `_`, at least TWO DIFFERENT letters, so `SQRT` is an operator and `AAA`
is not. `OpWord` already parsed as INFIX and already followed `BIG`. What was
missing was the PREFIX reading, and `opr-fixity.tex:34-55` says when that is --
an operator whose LEFT CONTEXT is another operator or a delimiter, which is
exactly where `unary` is reached. The arm asks `table_fixity_at`, the same
twelve-row table the infix path asks.

IT BUILDS A CALL AND NOT A `UnOp`, so both spellings reach ONE overload set:
`pfo1.fss` declares `opr FOO(x:Object)` beside a functional method
`opr FOO(self)`, runs, and the method wins.

AND THE 54 FIRST-BLOCKERS WERE NEVER THE CEILING. The prediction was written
before the sweep -- a library-declared `opr` never merges, because the resolver
skips `Decl::Function` from an api, so most files should move to a CHECK error
while only a file declaring the operator LOCALLY goes end to end. That is what
happened: 38 files moved forward and the single end-to-end gain declares its
own `opr`. **THE BUCKET IS FIFTEEN NOW, NOT 54**, and most of the remainder is
not buildable: three `OPR`, two `SIM`, two `ODOT`, one `QQ_NE` and one
`INTERSECTION` are `opr` STATIC PARAMETERS, refused by design under D7 section 4.
DO NOT SCHEDULE `OpWord` AS THE HEAD OF ANYTHING.

**G4/G5: `typed` IS AN ASCRIPTION AND IS BUILT; `asif` IS AN ASSUMPTION AND IS
REFUSED BY NAME, 2026-08-24.** parse 1133 -> 1152, corpus 571 -> 574, zero lost,
436 objects byte-identical IR.

ONE PRODUCTION, TWO FEATURES, ten lines apart in one spec file.
`type-annotation.tex:4-18` makes `typed` a type ASCRIPTION that "does not affect
the dynamic type", which is precisely what `expected: Option<Type>` already does
here, so its whole lowering is resolve, check the operand against it, hand the
type outward. `:36-53` makes `asif` a type ASSUMPTION over "both the static AND
THE DYNAMIC type ... for the purposes of the immediately enclosing ...
invocation", and the spec itself calls it a richer `super`.

SO `asif` IS REFUSED RATHER THAN QUIETLY READ AS THE ASCRIPTION. Dispatch here
is symmetric, whole-program and keyed on a concrete TAG, so honouring the
dynamic half means selecting a declaration the tag alone would not select.
Reading it as `typed` would compile `(self asif Generator[\E\]).asString` to the
object's OWN method: a silent wrong answer, not a missing feature.

BUILDING THE PARSE FIRST IS WHAT BOUGHT THE CEILING, and the ceiling is why the
rest is not scheduled. Of the 32 files first-blocked on the two keywords, THREE
compile end to end and only THREE of the remaining 29 stop on the `asif`
refusal; the other 26 walked past both keywords onto something else. The dynamic
half is a `super`-dispatch milestone for a three-file return. **SKIPPED BY
PRESTON'S RULING, 2026-08-24. Reopen only if something raises that ceiling, and
re-measure before believing it.**

**G6: A TYPED LOCAL FUNCTION REACHES THE REFUSAL THE UNTYPED ONE ALREADY
REACHED, 2026-08-24.** ZERO files gained, zero lost, the rc=0 set identical file
for file, 439 objects byte-identical IR -- AND THAT IS THE POINT. It buys a
measurement: the local-function first-blocker count goes 7 -> 36.

`block_item` found a local function by parsing `f(x)` as a CALL, and a typed
parameter list is not a call, so `f(w: ZZ32) = w+1` died at the `:` reporting
`expected )`. A second speculative parse reaches the refusal the untyped form
already reached; `params()` requiring `name: Type` is what stops it eating a
call whose arguments are expressions.

AND "LIFT TO COMPONENT LEVEL" IS THE WRONG MODEL, which is worth more than the
count. These local functions CAPTURE (`sideEffUpdate.fss:19`), EXIT NON-LOCALLY
(`labelExit.fss:58`) and are PASSED AS ARGUMENTS (`simplify1.fss:29`), which
needs arrow values. **IT IS A CLOSURE MILESTONE.** Three of the 36 are
must-FAIL.

**G7: AN ABSTRACT DECLARATION MAY ELIDE A PARAMETER NAME, AND ONLY AN ABSTRACT
ONE, 2026-08-24.** parse 1152 -> 1161, corpus 574 -> 579, zero lost, 439 objects
byte-identical IR.

`basic/functions.tex:384-385`, of a declaration with NO BODY: "Parameter names
may be elided but parameter types cannot be omitted." Both halves are enforced.
`params` takes a bare TYPE, told from a written name by ONE token -- a name is
followed by `:` -- and the synthesised name is `$N`, unwritable. Where elision
is NOT licensed a bare identifier IS the omitted-type case and is refused by
name: an object's value parameters, because they are its FIELDS, and any
declaration WITH A BODY.

THE SECOND GUARD IS NOT TIDINESS -- WITHOUT IT TWO WRONG PROGRAMS COMPILED.
`Object.Decl.Cons.fss` and `.ConsFn.fss` write `cons(x) = Cons(x,self)`, a
method with a body and an untyped parameter; read as an elided name that is a
parameter of type `x`, and both reached rc=0 and were counted as GAINS in the
first sweep. **THE FIRST "+7" WAS +5 PLUS TWO SILENT WRONG ANSWERS.** A feature
that makes an INVALID program compile shows up as a gain: read what a gained
file compiles TO, never just the count.

AND THE FIRST-BLOCKER COUNT SAID SIX, THE ANSWER IS FIVE, AND IT IS A DIFFERENT
SET. Two of the five were not predicted at all, because a first-blocker count
sees only the FIRST wall.

**H: A LOCAL FUNCTION'S PARAMETER LIST NEED NOT BE GLUED TO ITS NAME,
2026-08-24.** ZERO gained, zero lost, all 579 byte-identical IR, parse
1161 -> 1161. IT IS ONE FILE AND THAT IS THE DELIVERABLE.

`LocalDecl.rats:75` is `Id (w StaticParams)? w ValParam` and `w` is
`Whitespace*`, so `g (x: ZZ32): ZZ32 = e` is ONE declaration. The block-level
probe required the parenthesis to be GLUED, which read the SPELLING and not the
grammar, and it cost two different wrong messages: the typed spaced form died in
the parser at the `:`, and the untyped spaced form parsed as a JUXTAPOSITION and
reported `unknown name g` from the CHECKER. Exactly one file moved,
`ProjectFortress/tests/funny.fss:29`, which makes the local-function bucket a
TRUE ceiling rather than a glued-spelling one.

A NEWLINE STILL STOPS IT AND THAT IS FREE: a newline is a TOKEN here, so
`peek_ahead(1)` is `Newline` and not `LParen`. 1.0's own `w` spans newlines,
which would join two block elements into one declaration; no corpus file writes
that. THE LOSS CLASS IS REAL AND MEASURED AT ZERO -- a block-level `a (b) = 6`,
a discarded juxtaposition equality, is now the declaration reading and is
refused. 1.0's `BlockElem` is an ordered choice with `LocalVarFnDecl` FIRST, so
that is the oracle's behaviour and not a deviation.

**I: AN UNTYPED PARAMETER IS NOT AN ELIDED NAME, AND 42 FILES WERE TOLD IT WAS,
2026-08-24.** ZERO gained, zero lost, all 579 byte-identical IR. 42 files moved
MESSAGE.

THE SPEC HAS TWO PARAMETER PRODUCTIONS WHERE THIS COMPILER HAD ONE RULE.
`Parameter.rats:96` is `Param ::= BindId (w IsTypeOrPattern)?` and `:104` is
`AbsParam ::= BindId w IsType | Type`. So on a declaration WITH A BODY the TYPE
may be omitted and the NAME may not; without a body it is the other way round.
`functions.tex:384-385` says both halves in one sentence, which is how they came
to be implemented as one -- and 35 programs that were not attempting elision at
all were told they were, plus 7 more through the FIELDS wording.

AND ELISION IS NOT EVEN REACHABLE ON AN OBJECT'S VALUE PARAMETERS.
`TraitObject.rats:185` sends them through the SAME `Params` a function's go
through, so `object O(x)` is an untyped FIELD and can be nothing else. The
message says `field` now, and names it.

THE DIAGNOSTIC NAMES WHOLE-COMPONENT INFERENCE, and the citations were verified
rather than quoted: `basic/inference.tex` is 27 LINES whose entire chapter is
one note saying the mechanism will be described, and it records an unresolved
CIRCULAR DEPENDENCY between inference and juxtaposition disambiguation;
`components/type-inference.tex:15-16` runs inference over a WHOLE COMPONENT and
only after every imported api has been expanded into it. **Refused by name is
the correct end state here, not a deferral. Nobody should schedule untyped
parameters as a parser milestone.**

ALL 42 CORPUS FILES ARE THE BARE-IDENTIFIER SHAPE, so the surviving elision
branch has ZERO corpus exercisers and is a BACKSTOP held by two fixtures and one
mutation row.

**J: `end` MAY BE ELIDED FROM AN `if` IMMEDIATELY ENCLOSED BY PARENTHESES,
2026-08-24.** corpus 579 -> 582, parse 1161 -> 1174, zero lost, and the 579 that
already compiled emit byte-identical IR. objects 443 -> 446, apis 136, oracle
359 pass with 454 binaries built and run.

`if.tex:71-73`: "The reserved word `end` may be elided if the `if` expression is
immediately enclosed by parentheses. In such a case, an `else` clause is
required." 1.0 carries it as its OWN production, `DelimitedExpr.rats:40`, where
`Else` is MANDATORY and only `end` is optional -- the prose's second sentence
written into the grammar. BOTH HALVES ARE BUILT, and the second is what stops
the first accepting programs 1.0 refuses: an `if` with no `else` has type `()`,
so the missing branch would read as a void STATEMENT rather than as the error it
is.

THE LICENSING TEST NEEDS NO THREADING, which is what makes it cheap. Look
BACKWARD from the `if`'s own token, past any `Newline`, for an `LParen`. That
covers BOTH sites at once -- the parenthesised atom and a glued CALL's argument
list, one production in 1.0 and two here -- and it refuses `(1 + if c then 2
else 3)`, `f(1, if ...)` and the INNER `if` of `(if a then 1 else if b then 2)`,
which follows `else` and so still needs its own `end`. That last case FALLS OUT
of the test rather than being special-cased.

TWO DESIGN CORRECTIONS PAID FOR BY PROBING BEFORE WRITING. EVERY BLOCK INSIDE
THE IF-PARSE TAKES `RParen`, not just the `else` arm: with it on the else sets
only, `(if b then 1)` runs its THEN block onto the closing parenthesis and
reports the GENERIC message those files already gave, so the named refusal is
UNREACHABLE and its fixture would have been written around the generic text. And
`saw_else` IS TRACKED DURING THE PARSE, not read off the tree -- an `elif` chain
fills `else_branch` with the nested `if`, so "does this node have an
else_branch" is `Some` for exactly the program the refusal exists to catch.

AND THE BUCKET LIED TWICE. Nineteen first-blockers, recorded here as "ALL ONE
SHAPE": TWO were not `if` files at all (`Compiled2.j.fss` and `Compiled2.p.fss`,
an unbalanced closing parenthesis) and of the 17 real ones only THREE compile,
10 reach a CHECKER error and 4 move from one PARSE error to another. That is why
parse moves 13 and objects move 3. **A BUCKET IS NOT A FEATURE UNTIL YOU HAVE
READ EVERY FILE IN IT**, and reading the reported line of all nineteen is one
command.

STILL OPEN AND WORTH KNOWING: a nested `if` inside a licensed one is correctly
refused but reports the punctuation rather than the feature, because the outer
block's terminator set carries `RParen`. Zero corpus files write it. And J's
three gains have no `.test` file, so oracle-gate builds and RUNS them and
reaches no verdict -- the binary count moves 451 -> 454 and the pass count does
not. Expected, not a miss.

**SET COMPREHENSIONS, AND THE MINTED COLLECTION STOPS COLLIDING, 2026-08-25.**
`{ e | x <- lo:hi, p }` lowers onto a real monomorphized `Set[\T\]`, minted the
way `List[\T\]` is. ZERO CORPUS FILES GAINED, zero lost, and all 446
pre-existing objects emit BYTE-IDENTICAL IR -- measured with two binaries, the
instrument self-tested both ways on a changed integer LITERAL.

THE CEILING WAS MEASURED BEFORE ANY CODE WAS WRITTEN AND IT IS ZERO, which is
the finding. Four corpus files first-block on the `{_}` bracket. Reading all
four rather than counting them: `SetComprehension.fss` writes
`{[\ZZ32,ZZ32\] a | a<-3:10 }`, which is a MAP comprehension with two static
arguments and is now refused by name for exactly that; and the other three --
both `SpecData` examples and `desugarBug0.fss` -- write `s = { x DIV 2 | x <- t}`
with no static argument and no typed slot, so they walk off the "not
implemented" wall onto THE ELEMENT TYPE IS NEVER INFERRED, which is the same
rule the list form has always had. Behind that sits a second wall in the two
`SpecData` files, `t: Set[\ZZ32\] = {0, 1, 2, 3, 4}`: a set LITERAL, which is
`opr {[\E\] es: E... }` at `Library/Set.fsi:55` and never merges, because the
resolver skips `Decl::Function` from an api. THREE WALLS DEEP, AND THE
COMPREHENSION WAS THE FIRST OF THEM.

WHAT IT ACTUALLY BUYS IS THE COLLISION RULE, AND THAT IS WORTH MORE THAN THE
FEATURE. `ComprehensionListTaken` refused any component that already had a
`List` in scope, and "in scope" included a MERGED one -- so `import List.{...}`
beside a comprehension was fatal. That took `BirdyLib/Test3.fss`,
`tests/FunctionalMethodAsUnifyParam.fss` and `tests/importBig.fss` down, and it
was wrong: an api DECLARES and never DEFINES, so the merged `List` was
`MergedObjectNotConstructible` and could never have been written down anyway.
THE RULE IS NOW LINK 5's RULE 1 ONE LEVEL DOWN -- a merged declaration loses to
the minted collection the way it loses to a builtin -- and A DECLARATION THE
FILE WROTE ITSELF STILL WINS, because that one is constructible and the program
means it. All three files walked forward; `badcomplisttaken.fss` still refuses.

THE MINTED `Set[\T\]` IS `Array[\T\]` PLUS A LINEAR MEMBERSHIP SCAN OVER `=`,
and both halves are named deviations. 1.0's `Library/Set.fsi:56` bounds its
element by `StandardTotalOrder[\T\]` and keeps the elements SORTED; there is no
first-class ordering to demand of a written static argument here, so what is
preserved instead is INSERTION ORDER OF FIRST OCCURRENCE -- deterministic, and
a property a gate can assert. The set SEMANTICS, that duplicates collapse, is
exact. Storage is an ordinary `Array[\T\]`, so there is still exactly one
allocation path and codegen learned nothing.

AND THE DEDUP IS INVISIBLE TO AN EXIT CODE, which decides how it is gated.
`generator-gate.sh` part D builds nine elements from `x + y | x <- 0:2, y <- 0:2`
and asserts `size()` is FIVE and the walk is `0 1 2 3 4`: a `Set` that forgot to
be a set prints 9 and exits 0, and a shuffled walk prints the same five numbers
in the wrong order. FOUR NEW MUTATION ROWS, and the one that matters makes the
membership test answer `false` for everything -- the set silently becomes a
list, and nothing but that size assertion can tell.

**THE SET LITERAL, AND `opr IN`, 2026-08-25.** `{a, b, c}` lowers onto the same
minted `Set[\T\]` the comprehension builds. ZERO corpus files gained, zero lost,
all 446 pre-existing objects BYTE-IDENTICAL IR. TWO files moved MESSAGE, both
onto something more specific.

IT IS NOT A LITERAL NODE. `enclosed` (`parser/src/lib.rs:4314`) builds an
enclosing operator's name as `open + "_" + close`, so `{0, 1, 2}` is an ordinary
`Expr::Call` to a function named `{_}` with the elements as its arguments. The
lowering is therefore a rewrite in the same pass the comprehension lives in, and
it emits the obvious block: construct, `insert` each element in written order,
yield the accumulator.

**THE BRIEF ORDERED THE OTHER ROUTE AND THE OTHER ROUTE IS DEAD.** The plan was
to stop the resolver skipping `Decl::Function` from an api, so that
`Library/Set.fsi:55`'s `opr {[\E\] es: E... }: Set[\E\]` would merge. THREE
INDEPENDENT MEASUREMENTS SAY NO, and the first is decisive on its own:

  * **VARARGS ARE NOT IMPLEMENTED.** `f(es: ZZ32...)` parses as ONE parameter
    and `f(1,2,3)` reports `f takes 1 argument(s), found 3`. The api's
    declaration is varargs, so merging it would surface a declaration no call
    site could ever reach. The set literal would be no closer.
  * **THE DECLARATION IS BODILESS**, because an api has no bodies. A bodiless
    signature types a call and names a return and is never a dispatch target,
    so there is nothing to call even once the name resolves.
  * **THE SPEC PUTS IT THE OTHER WAY UP.** `source-code.tex:313-320` makes an
    api's function declarations OBLIGATIONS THE IMPORTING COMPONENT MUST
    SATISFY, not names it receives. Link 5's own rule 3 is the same rule one
    level down -- a merged functional method is not lifted into a component --
    and it was measured at 24 files when it was tried the other way.

So the resolver is untouched, and the literal is built the way the collection it
builds already was: minted, stamped by expansion, no new allocation path.

THE ELEMENT TYPE IS WRITTEN OR IT COMES OFF THE SLOT, AND HERE THAT IS
STRUCTURAL RATHER THAN STYLISTIC. `Set[\T\]` is STAMPED by monomorphization,
which runs before the checker exists; an element type discoverable only by
TYPING the elements cannot be stamped at all, because there are no types yet and
the pass that would make them runs after expansion has frozen the concrete set.
`SeqIterate` is not a precedent for deferring it -- that walks a collection that
already exists and mints nothing. `SetLiteralElementUnwritten` says exactly that,
and it is why the ceiling below is zero rather than one.

AND THE CEILING WAS MEASURED FIRST AND IT IS ZERO. Exactly ONE corpus file
first-blocks on `unknown name {_}`: `SpecData/examples/basic/Expr.Set.fss`,
which writes `3 IN {0, 1, 2, 3, 4, 5}` -- no slot to take an element type from,
so it moves onto the new refusal rather than compiling. `Documentation/
Specification/Code/If4.fss` moves too, from `unknown name z` -- true and useless
-- onto the same named wall, which is the second file and the reason this is
worth having anyway. THE TWO `SpecData` COMPREHENSION FILES DO NOT MOVE: they
write `t:Set[\ZZ32\] = {0, 1, 2, 3, 4}` on one line, which now works, and
`s = { x DIV 2 | x <- t}` on the next, which has no written element type either.

`opr IN` IS FREE AND IT IS 1.0's OWN SPELLING. `opr IN(x: T, self): Boolean` on
the minted set -- a FUNCTIONAL METHOD with `self` in the SECOND position,
because the element is on the left -- so `3 IN t` is the ordinary dispatch this
compiler already does. Two corpus files write it.

**MAP LITERALS AND MAP COMPREHENSIONS, 2026-08-25.** `{k |-> v, ...}` and
`{k |-> v | x <- g}` lower onto a minted `Map[\K,V\]`. ZERO corpus files gained,
zero lost, all 446 pre-existing objects BYTE-IDENTICAL IR. TEN files moved
MESSAGE.

**THE FIRST WALL WAS NOT THE COMPREHENSION, IT WAS `|->`.** Eleven corpus files
stopped at `expected an expression, found Gt`, and reading the reported line of
all eleven -- rather than counting them -- gives THREE features and not one:
five map comprehensions, two map LITERALS, and four files that have nothing to
do with maps (`Bazaar.fss` writes `|s| >> ...`, `BooleanOps.fss` writes
`a<->b`, `conditionalOp.fss` writes `->:`). The error did not even land on the
arrow: with no mapping rule, `{ 1 |-> 4 }` breaks its element loop on the
non-comma, `operator_run` reads the bare `|` as a closing run OF THE RIGHT
LENGTH, and the call is built against an operator called `{_|` -- so the
diagnostic points at the `>` two characters later.

`|->` IS RE-GLUED BY THE PARSER AND IS NOT A LEXER TOKEN, which is what `->`
and `<-` already do: neither is a token in ASCII, and `Symbol.rats:197` gives
the UNICODE spellings tokens of their own precisely because the ASCII ones are
runs. Decision 3's rule -- mathematical symbols never become new lexer tokens --
is kept. MEASURED BEFORE IT WAS WRITTEN: 439 sites write `|->` and ZERO write
`| ->`, so a maximal-munch reading takes no spelling away from anything.

`Expr::Mapping` IS ITS OWN NODE AND MUST NOT BE A TUPLE.
`not_passing_yet/mapConstants.fss:33` writes `("Hi",3) |-> ("Lois",23)`, whose
key AND value are tuples, so a two-element tuple cannot tell a mapping from its
own operands. The variant forced SEVEN arms through E0004 and each one is a
real decision: the four walkers recurse into both halves, and the CHECKER
refuses it by name -- a mapping is one entry of a map and has no representation
of its own, so anything reaching the checker was written where a value belongs.

THE BRACKET DOES NOT DECIDE THE COLLECTION, THE ELEMENT DOES. `{a, b}` is a set
and `{k |-> v}` is a map, on one encloser, which is how 1.0 spells them too
(`Library/Set.fsi:55` against `Library/Map.fsi`). `Kind` gained an `arity`, and
`kind_for` takes the shape of the element as well as the brackets. Three
refusals fall out and each names its own half: a literal MIXING an element and a
mapping is neither; two static arguments over a non-mapping body is neither;
and a mapping body in the LIST brackets is a BODY problem, not a bracket
problem -- saying "this bracket's lowering is not implemented" of `<| |>`,
which plainly works, would send the reader to the wrong place.

THE MINTED `Map[\K,V\]` IS TWO PARALLEL ARRAYS AND NOT AN ARRAY OF PAIRS,
because a pair would be a TUPLE VALUE and this backend has none: a tuple is
FLATTENED, never boxed, so two arrays are that flattening written down. A later
`insert` at a present key REPLACES its value, which is the whole difference
between a map and a multimap and is invisible to an exit code -- so the fixture
writes three entries at two keys and asserts `m[1]` is the SECOND value. Two
mutation rows attack exactly that.

AND THE CEILING IS ZERO, MEASURED FIRST. Of the 23 corpus files that write a map
comprehension, TWO already compile, ONE first-blocks on the map refusal, and the
other twenty stop on unrelated walls -- local functions, `MIN` over a
collection, untyped parameters, ordinary parse errors. The two best candidates
were read in full and neither is close: `mapConstants.fss` needs `//`, `assert`
and tuple keys and its own comment records a known bug in the construct, and
`mapCombine.fss` needs `println` of an object, `asif` (refused by name),
`identity` and five closures.

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
0.09 s at fourteen, ON FOURTEEN UNPINNED CORES, which is no longer how anything
here runs: since 2026-08-25 everything is confined to CPUs 2-7 and
`tools/phase7-gate.sh:59` still sweeps `WORKERS="1 2 4 8 14"`. The gate still
passes pinned, because the floor is a RATIO and the one-worker leg gets one core
either way, but the absolute 0.09 s will not reproduce on six cores -- and `ZZ64` indexing works past 2^31: index 2,999,999,999
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

* 34 files declare a plain `grammar`. Every one is in
  `ProjectFortress/syntax_abstraction_tests/` (110 files total with its consumer
  cases), which is the feature testing itself.
* `Library/` has 126 source files and zero plain `grammar` declarations.
* **THE WORD "PLAIN" IS LOAD BEARING AND WAS ADDED 2026-08-25.** Counting
  `native grammar` too makes it 35 files, and the one that is NOT under
  `syntax_abstraction_tests/` is `Library/FortressSyntax.fsi`, which carries
  eighteen `native grammar` declarations. So "every one is in the test
  directory" and "`Library/` has zero" are both true of the plain form and both
  false of the native one. It changes nothing about the decision -- that file is
  already one of the three macro-API files cut below, and `triage.sh` deliberately
  leaves it inside the 1846 for the same reason -- but a reader who greps
  `grammar` gets 35 and concludes this section is lying.
* Three files in `Library/` touch the macro APIs (`FortressSyntax.fsi`,
  `FortressAstUtil.fss`, `FortressAstUtil.fsi`, 218 lines together -- 144, 51
  and 23, re-verified 2026-08-25). They import each other and nothing else in
  `Library/` imports them. `FortressLibrary.fss` does not. There is a FOURTH
  file of the family the count leaves out and it costs nothing:
  `Library/FortressSyntax.fss`, 14 lines whose entire body is `component
  FortressSyntax / export FortressSyntax / end` -- the empty component half of
  the api, declaring no grammar at all.

So the standard library does not use syntax abstraction at all. Cutting it from
v1 costs the 110 test files and those 218 lines. Nothing else breaks.

Two things that follow. The 110 files come out of the conformance denominator, so
corpus percentages in phases 1 and 2 should be quoted against 1846, not 1956. And
the specification still documents the feature, so v1 is a Fortress dialect rather
than the whole language. Say so in the README when v1 ships.

**THE 1846 DENOMINATOR IS OPT-IN AND IS NOT THE DEFAULT.** This paragraph said
"NEVER ADOPTED" and that `grep -rn 1846` across `tools/` and `fortressc/crates`
"returns nothing", AND THAT WAS FALSE WHEN IT WAS WRITTEN. The exact grep returns
five hits, all in `tools/triage.sh`, and two of them are live code rather than
comment: `--conformance` filters the syntax-abstraction files out (:550-551) and
the selftest HARD-ASSERTS the denominator (`check('the conformance denominator is
1846', ...)`, :541). It landed 2026-08-21 in 91d37a295; the paragraph denying it
was written 2026-08-23 in faef66205, two days later, and nobody ran the grep it
quotes.

WHAT IS TRUE: `fortressc/crates` is genuinely clean, 0 hits. Every gate and every
default report walks and quotes 1956, so 1846 moves no recorded number unless a
reader asks for it. THE CUT IS BY PATH, NOT BY FEATURE, and `triage.sh:114-117`
says why -- cutting by feature would take `Library/FortressSyntax.fsi` with it
and 1846 would stop reproducing. Quote 1956 unless you mean conformance, and say
which one you mean.

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
  Measured: `nat` PARAMETERS have 61 files, reproduced exactly. A bound on a value
  parameter is refused by name rather than dropped.
  **CORRECTED 2026-08-25, TWO WAYS.** This said "not one `where { k < n }` exists
  in 1956 corpus files" and ONE DOES: `ProjectFortress/tests/whereTest.fss:21`
  writes `2 n + i < 2^8` inside a `where` clause, with `n` a `nat` and `i` an
  `int` parameter of the enclosing trait, and it was already there when the census
  ran. Both censuses missed it the same way -- the syntax is
  `where [\params\] { constraints }`, so a scan requiring `{` immediately after
  `where` sees 13 clauses and none of the 18 that really exist. THE RE-OPEN
  CONDITION THIS BULLET SETS FOR ITSELF IS THEREFORE MET ON ITS OWN TERMS, and
  the demand behind it is still one file: `whereTest.fss` is a PARSE smoke test
  whose `run()` prints "Where clasues can be parsed." Not a constraint solver's
  worth of demand, but the sentence was false and the count is not zero.
  And "842 sites" DOES NOT REPRODUCE and disagrees with its own source: D7's
  census (`2026-08-21-d7-reconcile-nat.md:179`) says 377. The two are a
  match-count and a line-count of the same thing; nobody wrote down which unit
  "sites" meant. The 61 FILES reproduce exactly on both instruments and are the
  number to quote.
* `unit` and `dim` stay in v1 and stay deferred to sub-phase 4d, gated on
  `SPIKE-COMPOSITE-TYPE` rather than on D7 — `unit` is 6 corpus files and zero
  library files AS A STATIC PARAMETER, and `dim` has no corpus witness AS A
  STATIC PARAMETER. Both qualifiers were added 2026-08-25 because the
  unqualified sentence is refuted by a grep: `unit` and `dim` DECLARATIONS are a
  different construct and do have witnesses -- `dim Length` at
  `ProjectFortress/tests/dimensionUnitDecl.fss:16`, and four library files under
  `Library/incomplete/basic/`. D7 worded it precisely and this line compressed
  the qualifier out.
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
