# Roadmap

Goal: a native Fortress compiler producing ELF binaries that run under Slurm,
linked against OpenMPI over InfiniBand. Work that serves that ships. Work that
does not gets cut.

Every phase has one exit criterion. The measure throughout is the ~1950 `.fss`
and `.fsi` files already in this tree, run against the legacy interpreter for a
differential baseline.

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

**0. Baseline.** Get the legacy interpreter building and running. Ant and Java 6
era code, expect it to be broken.
*Exit:* legacy interpreter runs `ProjectFortress/tests/` and the pass/fail set is
recorded in the repo. That recorded set is the target, not the specification.

**1. Lexer.** `logos` based, newline aware (see decision 2).
*Exit:* tokenizes all 1950 corpus files without panicking, with token counts
stable across runs.

**2. Parser.** Recursive descent over the core grammar, ported from the 27
`.rats` modules under `ProjectFortress/src/com/sun/fortress/parser/`.
*Exit:* parses 90% of the corpus to an AST. The remaining 10% is catalogued with
a reason each.
*Where it is:* 1780 of 1956 lex (91%), 168 of those parse. Both numbers are
ratchets in the corpus tests rather than commentary, so a regression fails the
build.

**3. Names and modules.** Component and API resolution, imports, scoping.
*Exit:* `Library/` resolves clean with no unresolved references.

**4. Types.** Hindley-Milner inference with traits, polymorphism and overload
resolution. The legacy implementation never finished this, so the specification
is the authority here, not the old behaviour.
*Exit:* type checks `Library/` and the corpus, disagreements with the legacy
interpreter documented rather than silently matched.

**5. Codegen, sequential.** LLVM IR via `inkwell`. No parallelism yet.
*Exit:* hello world plus the single threaded half of the corpus compiles, links
and produces the same output as the interpreter.

**6. Runtime and the C ABI.** Memory management (ARC or Boehm), the `extern "C"`
boundary, OpenMPI linkage.
*Exit:* a Fortress program calls `MPI_Init` and `MPI_Comm_size` and returns the
right rank count on two nodes.

**7. Parallelism.** Parallel `for`, `atomic`, `spawn`, `also`, generators and
reductions lowered to real threads.
*Exit:* a parallel reduction over 10^9 elements beats the sequential version on
one node, and `ZZ64` indexing works past 2^31.

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
in `contrib/`. The legacy interpreter stays only as a differential oracle and
gets deleted once phase 7 passes.
