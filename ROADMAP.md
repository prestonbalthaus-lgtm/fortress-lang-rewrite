# Roadmap

Goal: a native Fortress compiler producing ELF binaries that run under Slurm,
linked against OpenMPI over InfiniBand. Work that serves that ships. Work that
does not gets cut.

Every phase has one exit criterion. The measure throughout is the ~1950 `.fss`
and `.fsi` files already in this tree, run against the legacy interpreter for a
differential baseline.

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

**3. ASCII core.** Decision already recorded in `02-stack.md`: the core grammar
stays ASCII and mathematical symbols come in through library aliasing. Worth
knowing the original grammar imports a `Unicode` module, so this is a choice
being made, not one inherited. Not reopening it.

**4. v1 language scope.** The old README listed what was never implemented. Some
of it is HPC critical and has to be in v1: reduction variables, distributions,
`ZZ64` indexing, non-`RR64` floats, bits and storage types. The rest (dimensions
and units, keyword arguments, where clauses, coercion, modifiers) is deferrable.
Draw the line explicitly before phase 4.

## Out of scope for v1

Eclipse and Emacs tooling, Fortify LaTeX rendering, the Vim files, and anything
in `contrib/`. The legacy interpreter stays only as a differential oracle and
gets deleted once phase 7 passes.
