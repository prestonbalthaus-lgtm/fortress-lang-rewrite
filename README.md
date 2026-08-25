# Fortress

Fortress is a parallel programming language built at Sun Labs as their entry in
the DARPA HPCS program, alongside Cray's Chapel and IBM's X10. Sun shipped a
working interpreter and then cancelled the project. Upstream development stopped
in 2012.

This repository is a fork of that codebase plus an ahead of time compiler being
written from scratch in Rust, targeting LLVM.

## What the language looks like

From `ProjectFortress/demos/fact64.fss`, unchanged:

```
component fact64
export Executable

run() = do
   for i <- seq(0#20) do
     j:ZZ64 = widen(i)
     println("fact(" j ")= " f(j))
   end
end

f(x:ZZ64):ZZ64 = if x < 2 then 1 else x f(x-1) end
end
```

Three things in that snippet explain the design:

* `x f(x-1)` is multiplication. Juxtaposition is the multiplication operator, so
  code reads the way the math does.
* `for` loops run in parallel by default. The `seq()` wrapper is what makes this
  one sequential. Parallelism is the default and you opt out.
* Reductions are first class: `SUM[i <- 1#n] f(i)` distributes without you
  writing a single thread.

Types are traits with real polymorphism, `atomic` blocks are transactional, and
the standard library is written in Fortress rather than in the host language.

## Why rewrite it

The original implementation runs on the JVM, and that is where the ceiling comes
from:

* Arrays cap at 2^31 elements because the JVM indexes with 32 bit ints. `ZZ64`
  indexing was never implemented for exactly that reason. That alone rules it out
  for real cluster work.
* No native C ABI. Linking against OpenMPI or anything on the InfiniBand fabric
  has to go through JNI.
* JVM startup cost and GC pauses inside an MPI rank are not something you can
  schedule around.
* The features that would justify the language on a supercomputer (distributions,
  reduction variables, dimensions and units) were still unimplemented when work
  stopped.

A native AOT compiler removes all four. The target is a Fortress program that
compiles to an ELF binary, links against OpenMPI, and runs under Slurm on real
hardware.

If you need a production HPC language today, use Chapel. It is alive, funded and
good. This exists because Fortress's design deserves a working implementation and
never got one.

## What v1 will be

Everything the Sun implementation never finished is in scope. All 16 items off
its unimplemented list, including the cluster critical ones (`ZZ64` indexing past
2^31, reduction variables, distributions, non-`RR64` floats) and the expensive
ones (dimensions and units, coercion, where clauses, `nat` constraint solving),
plus a native C ABI. v1 is the 1.0 specification, less the one exception below.

Out of scope: user definable syntax. Fortress lets programs extend the grammar,
and measured across all 1956 source files in this tree, only its own test
directory uses it. The standard library does not touch it. Cutting it keeps the
frontend to a plain lexer and recursive descent parser. That makes v1 a Fortress
dialect rather than the full language, and this is the notice that says so.

## Status

M1 is done. A Fortress program compiles to a native ELF and runs:

```
$ fortressc tests/fact.fss -o fact
$ ./fact
fact(20) = 2432902008176640000
$ ldd fact
    linux-vdso.so.1
    libc.so.6 => /lib64/libc.so.6
    /lib64/ld-linux-x86-64.so.2
```

No JVM in the toolchain or the output. That factorial is real 64 bit recursion
through `f(x) = if x < 2 then 1 else x f(x-1) end`, where the juxtaposition
`x f(x-1)` resolved to a multiply and `"fact(20) = " f(j)` to a string
concatenation, from identical syntax, on operand types alone.

M2a is done: the MPI boundary. A Fortress program calls MPI and runs as real
ranks.

```
$ cat fortressc/tests/mpi_hello.fss
component mpi_hello
export Executable

run() = do
   mpiInit()
   rank:ZZ32 = mpiCommRank()
   size:ZZ32 = mpiCommSize()
   println("rank " rank " of " size)
   mpiFinalize()
end
end

$ fortressc tests/mpi_hello.fss -o mpi_hello --cc tools/mpicc-in-image.sh
$ apptainer exec apptainer/fortress-mpi.sif mpirun -np 4 ./mpi_hello
rank 0 of 4
rank 1 of 4
rank 2 of 4
rank 3 of 4
```

`MPI_COMM_WORLD` is a macro that expands to a pointer under OpenMPI and to an
integer under MPICH, so no MPI symbol is ever emitted into LLVM IR. Generated
code calls four `fortress_mpi_` shims, and `runtime/mpi_shims.c` is the only
file that includes `<mpi.h>`. The cluster's own `mpicc` compiles it at link
time, which is what `--cc` selects. Run `tools/mpi-gate.sh` to check the whole
path, including the ranks.

M3a is done: memory is collected. Every heap allocation goes through
`runtime/shims.c`, and there are TWO allocators there because the distinction is
load bearing: `fortress_alloc` is `GC_malloc_atomic` for pointer-free bytes such
as strings, and `fortress_alloc_scanned` is `GC_malloc` for anything that stores
a pointer, such as array storage and object blocks. Atomic memory is not
scanned, so putting a pointer-holding block on the atomic path frees what it is
still holding -- `runtime/tests/array_trace.c` measures that rather than
assuming it. A program doing a million string concatenations and one doing ten
thousand use the same resident set:

```
$ ./tools/memory-gate.sh
      10000 iterations, collected : 5768 KB
      1000000 iterations, collected : 5760 KB
      1000000 iterations, leaking   : 64076 KB
```

The third line is the negative control: the same object file linked against the
allocator M1 shipped. Without it the flat number would prove nothing.

Objects are also built for a chosen processor rather than for whatever ran the
compiler. `--target-cpu` defaults to `x86-64-v3` and accepts `skylake-avx512`
for the Platinum 8160s, or `native`.

M3b is done: arrays and iteration.

```
component arraysum
export Executable

run() = do
   n:ZZ64 = 100
   squares:Array[\ZZ64\] = array(n)

   i:ZZ64 := 0
   while i < n do
      squares[i] := i i
      i := i + 1
   end
   ...
```

Arrays are homogeneous and RANK IS PART OF THE TYPE: `Type::Array(Elem, u8)`,
so `ZZ32[5]` and `ZZ32[2,3]` are different types, and the matrix aggregate
`[3 4; 5 6]` builds the second. Subscripts are `ZZ64` so an array can be longer
than 2^31, every dimension is bounds checked separately and the halt names which
one: out of range prints `fortress: array index out of bounds (5, 3)` and exits
1 rather than faulting. Array storage is allocated scannable, so the collector can see
the strings an `Array[\String\]` holds; `tools/array-gate.sh` measures that
rather than assuming it.

M3c is done: traits, objects and symmetric multiple dispatch.

```
trait Ink end
object Solid extends {Ink} end
object Dotted(width: ZZ32) extends {Ink} end

trait Face end
object Round extends {Face} end

draw(i: Ink, f: Face): ZZ32 = 1000
draw(i: Solid, f: Face): ZZ32 = 2000
draw(i: Solid, f: Round): ZZ32 = 3000
```

Dispatch is symmetric: which declaration runs depends on the run-time types of
*all* the arguments, not just a receiver. Being a whole-program compiler makes
that cheap. Rather than implement specification 1.0's modular Subtype,
Incompatibility and Meet rules, the compiler enumerates every tuple of concrete
types that can reach an overload set and requires exactly one most-specific
winner per cell. That single computation is the ambiguity check, the dispatch
table, and the proof that no case is missing.

An object is one heap block with a 32-bit type tag at offset 0. Traits have no
run-time representation at all. The table is flattened into a nested switch on
those tags, every leaf a direct call, and a row whose winners agree collapses --
so almost every call in a program stays an ordinary direct call and only a
trait-typed argument costs a tag load.

Two deliberate departures from 1.0, both signed off: an ambiguous call is a
compile error naming the tuple and both declarations, where 1.0 would pick one
arbitrarily; and trait exclusion is closed-world.

M3d is done: generics, by monomorphization.

```
object Cell[\T\](held: T) end
pick[\T\](a: T, b: T, first: Boolean): T = if first then a else b end
```

```llvm
%"Cell$ZZ64$e"   = type { i32, i32, i64 }
%"Cell$String$e" = type { i32, i32, ptr }
define i64 @"pick$ZZ64$e"(i64 %a, i64 %b, i1 %first)
```

Concrete copies are stamped out at compile time, so a `ZZ64` cell holds an
`i64` rather than a pointer to a box. No erasure, no boxing: that is what keeps
`Array[\ZZ64\]` a block of integers.

Expansion is an AST-to-AST pass that runs to a fixpoint **before** the type
checker exists, which is what protects the dispatch tables -- an instantiation
creates a concrete type, and a table built before that type existed would have
no arm for it. Static arguments are written rather than inferred, which is what
makes instantiation demand syntactic and lets the pass run that early.

Monomorphization cannot compile polymorphic recursion, and the corpus contains
some: `Library/PureList.fss:137` calls `arrayToFingerTree[\D23[\E\]\]` from
inside `arrayToFingerTree[\E\]`. There is a hard ceiling of 4096 instantiations
per component and it refuses with a diagnostic rather than running out of memory.

The compiler lives in `fortressc/`: a six crate Rust workspace, lexer through
LLVM codegen. See `docs/superpowers/specs/` for the design of every milestone,
each of which records why the rules are what they are -- including where a
design turned out to be wrong.

M3e through M5 are done too, and this section is a summary rather than the
record -- `docs/superpowers/specs/` carries one design document per milestone.
Landed since M3d: unit, tuple and arrow types; juxtaposition as application and
chained comparison; getters, setters, `self` and top-level values; dotted
methods; generic and functional methods; operators and the builtin set; the
parallel `for` and `spawn`; `atomic` and reduction variables; multi-dimensional
arrays and the matrix aggregate; `Char`; radix numerals; dimensions and units as
far as declaration and checking; `Object`/`Any` as real root traits; LIST, SET
AND MAP comprehensions and the set and map LITERALS, on monomorphized
`List[\T\]`, `Set[\T\]` and `Map[\K,V\]`; arity flattening; and the GENERATOR
PROTOCOL.

**The generator protocol is `Indexed`, walked EXTERNALLY, and that is a named
deviation with three measured reasons.** 1.0's protocol is
`generate[\R\](r: Reduction[\R\], body: E->R): R`; there is no first-class
`Reduction` here, and a `()` arrow codomain -- the arrow `loop` takes -- is
refused by name. THE THIRD REASON THIS USED TO GIVE WAS FALSE AND CAME OUT ON
2026-08-25: it said a component cannot name `Generator`, `Indexed` or
`Condition` because the implicit core-api import is api-side only, and called
that the decisive one. Link 5 had already landed -- as the section below this
table says in the same file -- so a component names all three with no written
import and compiles. The two reasons above are each sufficient on their own, so
the check is still structural and the deviation still stands. The members are still 1.0's own --
`Library/FortressLibrary.fsi:1205` declares `getter size()` and `opr [i: I]: E`
on `Indexed`, and `opr []` now dispatches on an object -- and 1.0's own NATIVE
compiler library, `Library/CompilerLibrary.fsi`, cuts the protocol the same way
down to a monomorphic `GeneratorZZ32`. So `for x <- aCollection`, a comprehension
over one, and `if x <- g then` / `while x <- g do` all compile.

**Phase 7 has passed, and it is the reason the rewrite exists.** The JVM ceiling
in "Why rewrite it" above was arrays capping at 2^31 elements. `tools/phase7-gate
.sh` writes and reads index 2,999,999,999 of a three-billion-element
`Array[\Boolean\]`, and runs a 10^9-element parallel reduction that goes from
0.80 s at one worker to 0.09 s at fourteen. (Those absolute times are from
fourteen unpinned cores. Everything here now runs on CPUs 2-7; the gate's floor
is a ratio and still passes, the wall-clock will not reproduce.)

Where the numbers are, and every one is a ratchet in a test rather than
commentary:

| metric | today | floor | instrument |
|---|---|---|---|
| corpus files that LEX | 1909 of 1956 | 1845 | `crates/lexer/tests/corpus.rs` |
| corpus files that PARSE | 1174 | 1174 | `crates/parser/tests/corpus.rs` |
| `.fss` that compile and emit an object | 446 | 321 | `tools/apply-gate.sh` |
| `.fsi` that check | 136 | 135 | `tools/apply-gate.sh` |
| oracle cases that agree with 1.0 | 359 | 356 | `tools/oracle-gate.sh` |

Re-measured 2026-08-24 by running the instruments; the previous row values had
drifted several milestones behind the ratchets they cite. The api floor has no
slack, which is the one to watch.

The headline metric is object emission plus api checking, split on purpose: an
api emits no object, so one number stopped meaning one thing. Every compiling
corpus file is also LINKED AND RUN under a signal sweep -- 454 binaries.

**THE MODULE SYSTEM REACHES COMPONENTS.** `source-code.tex:305` says every
component implicitly imports the Fortress core APIs, and as of 2026-08-23 it
does: a `.fss` resolves `Generator`, `Maybe`, `Number` and the rest with no
written import. `unknown type` as a first blocker went from 93 corpus files to
26. Four rules make it safe, each measured against the alternative:

* a merged declaration LOSES to a builtin of the same name -- merging the
  library's own `trait String` shadowed `Type::String` and `expected String,
  found String` broke forty files;
* a merged trait's supertype edge to a builtin is DROPPED, because a scalar has
  no trait representation here and the edge could never have been honoured;
* a merged functional method is NOT lifted into the importing component, for the
  same reason an api's top-level functions are not merged -- they are
  obligations the component must satisfy, not names it may use;
* a merged object is lowered ONLY when this file names it and its layout is
  buildable, and never if it is a singleton. Lowering all of them put eighty
  library singletons into every program's `main` and took a hello world from
  125 lines of IR to 205.

Still missing: SET and MAP comprehensions (the list form landed), `at` and
distributions, coercion (recorded but never applied), unit algebra above
declaration, a NESTED tuple, and user definable syntax.

**The tuple calling convention is complete in both directions.** A tuple
PARAMETER flattens -- `overloading.tex:125` makes `f(x:(A,B))` and `f(a:A,b:B)`
one declaration, so the first is lowered into the second and nothing is ever
whole. A tuple RESULT is an LLVM aggregate return, `insertvalue` into a struct
and `extractvalue` at the call, and it is still non-materialising: the value
lives in SSA registers, so there is no allocation, no tag and no `alloca`, and
LLVM's own ABI lowering decides whether the fields travel in registers or
through a hidden pointer.

The legacy Sun implementation is kept as reference material:

| Path | What it is |
|------|------------|
| `ProjectFortress/` | The Java and Scala interpreter, plus the Rats! parser grammars |
| `Library/` | The Fortress standard library, written in Fortress |
| `Specification/` | LaTeX source of the 1.0 language specification |
| `SpecData/` | Machine readable spec data: reserved words, examples |
| `Fortify/` | Renders Fortress source into LaTeX |

1956 `.fss` and `.fsi` files sit across `Library/` and `ProjectFortress/`.
Those are valid Fortress programs and they are the conformance suite the new
compiler gets measured against.

## Building

```
cd fortressc
./setup-llvm.sh                       # only if llvm-devel is not installed
./setup-gc.sh                         # only if gc-devel is not installed
export LLVM_SYS_221_PREFIX=...        # each script prints the values
export CPATH=... LIBRARY_PATH=...
cargo build --workspace
cargo test --workspace
```

Needs LLVM 22, a C compiler and the Boehm collector. The two setup scripts exist
because Fedora splits both packages across a runtime half and a `-devel` half,
and the `-devel` half needs root; the scripts unpack it into `~/.local` instead.
With root, `dnf install llvm-devel gc-devel` does the same job.

### Parallelism, and why it is capped

`fortressc/.cargo/config.toml` caps the build at **6 jobs**, and every tool that
fans out — `triage.sh`, `oracle-gate.sh`, `api-census.sh`, `api-conformance.sh`
— sizes its pool from **`sched_getaffinity`, not `os.cpu_count()`**. Those are
different numbers the moment anything is pinned: under `taskset -c 2-7`,
`os.cpu_count()` still reports 14 and would put fourteen compilers on six cores.

The reason is the development box, and it generalises. It has **14 physical
cores and no SMT**, and its `systemd` is confined to CPUs 0-1. A sweep that
takes all fourteen is therefore not using spare capacity — it is competing with
the kernel's own threads for the two cores they are allowed on, and the desktop
locks up. `oracle-gate.sh` alone builds and runs 454 binaries.

So: run the tooling pinned.

```
taskset -c 2-7 cargo build --workspace
taskset -c 2-7 tools/oracle-gate.sh
```

Cargo needs no flag for that — `available_parallelism` respects affinity
(measured: 14 unpinned, 6 under the pin), so a pinned build limits itself and
the config line above is only for the unpinned case. Override either with
`cargo build -j N` or `--jobs N`.

`gc.h` and `-lgc` are needed wherever a Fortress program is *linked*, not just
where the compiler is built: `runtime/shims.c` is compiled by the linking C
compiler so that it matches the target's C library.

Gates that cargo cannot run: twenty-three shell scripts in `tools/`, nineteen
of them named `*-gate.sh`. **Read `docs/RUNNING-THE-GATES.md` before running any
of them** — it carries the rules that are not optional and the ten traps that
are already paid for.

`tools/oracle-gate.sh` is the primary correctness instrument and the one to run
first. The oracle is the 373 `.test` files the legacy implementation shipped, on
disk, needing no JVM: 264 of them record the exact compile error 1.0 gave under
`compile_err_equals`, the predicate the gate self-tests (266 is the looser count
across every `compile_err_*` comparator), which
is a MUST-FAIL ratchet -- `tools/oracle-accepted-must-fail.txt` names every
program we wrongly accept, a new acceptance outside the list is red, and a file
that starts being refused comes out of the list in the same commit.

Three rules, because getting them wrong produces a wrong number silently:

1. **`export FORTRESSC=<pinned copy>` before any sweep.** `cargo build` rewrites
   `fortressc/target/debug/fortressc` in place, and a sweep that reads that path
   while someone rebuilds mixes two compilers with no error and no warning.
   Twenty-two of the twenty-three honour it; the exception is
   `mpicc-in-image.sh`, which is a `cc` wrapper and never invokes the compiler.
2. **`FORTRESSC` and `--mutate` MUST NOT be combined,** and this is the inverse
   of rule 1 rather than a footnote to it. Every mutation rebuilds
   `target/debug`, so a pin makes each one a silent no-op and the table reports
   a clean escape. Sixteen gates refuse the combination and exit 2.
3. **Keep the pinned copy OUTSIDE `fortressc/build/`.** That directory is shared
   scratch: twenty-two of them write into it and sixteen `rm -rf` it. A pin was
   lost that way on 2026-08-21.

Every gate takes `--selftest`, which proves its assertions can refuse without
needing anything built. Sixteen take `--mutate`; a green gate is evidence only
when its mutation table has run and its numbers are stated.

The legacy interpreter builds with Ant against Java 6 era code. It has not been
verified to still work.

## License

The legacy tree is under the Sun/Oracle terms in `LICENSE`: BSD 3-clause, Sun
2007. The Apache-2.0 text further down that file covers the bundled Ant and BCEL
jars, not Fortress.

**The new code's licence is UNDECIDED and the repository currently contradicts
itself about it.** `fortressc/Cargo.toml:16` declares `license = "Apache-2.0"`
and all six crates inherit it, so that metadata has been asserting Apache-2.0
since the workspace was created. `docs/superpowers/specs/2026-08-21-d9-oracle-and-licence.md`
lays the three-way problem out and is explicitly drafted, not adopted. Settle it
before the first release; until then treat the `Cargo.toml` field as unreviewed
rather than as the answer.
