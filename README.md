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
`fortress_alloc` in `runtime/shims.c`, which now calls `GC_malloc_atomic`
instead of `malloc`. A program doing a million string concatenations and one
doing ten thousand use the same resident set:

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

`Array[\T\]` is one dimensional and homogeneous, subscripts are `ZZ64` so an
array can be longer than 2^31, and every subscript is bounds checked: out of
range prints `fortress: array index out of bounds (5, 3)` and exits 1 rather
than faulting. Array storage is allocated scannable, so the collector can see
the strings an `Array[\String\]` holds; `tools/array-gate.sh` measures that
rather than assuming it.

M3c is done: traits, objects and symmetric multiple dispatch.

```
trait Ink end
object Solid extends {Ink} end
object Dotted(width: ZZ32) extends {Ink} end

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

Still missing: `for` and generators, parallelism, `atomic`, dimensions and
units, coercion, enclosing operators, and user definable syntax. The lexer takes
1780 of the 1956 corpus files (91%) and the parser 168 of those. What blocks the
parser now is tuple and arrow types, `getter`/`setter`, and `opr` declarations --
not generics, which was measured before it was built.

The legacy Sun implementation is kept as reference material:

| Path | What it is |
|------|------------|
| `ProjectFortress/` | The Java and Scala interpreter, plus the Rats! parser grammars |
| `Library/` | The Fortress standard library, written in Fortress |
| `Specification/` | LaTeX source of the 1.0 language specification |
| `SpecData/` | Machine readable spec data: reserved words, examples |
| `Fortify/` | Renders Fortress source into LaTeX |

About 1950 `.fss` and `.fsi` files sit across `Library/` and `ProjectFortress/`.
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

`gc.h` and `-lgc` are needed wherever a Fortress program is *linked*, not just
where the compiler is built: `runtime/shims.c` is compiled by the linking C
compiler so that it matches the target's C library.

Gates that cargo cannot run:

```
./tools/array-gate.sh     arrays, bounds, the loop, and what the collector sees
./tools/memory-gate.sh    the collector, and the leak it replaced
./tools/mpi-gate.sh       the MPI link and four real ranks (needs the image)
```

Both take `--selftest`, which proves their assertions can refuse without needing
anything built.

The legacy interpreter builds with Ant against Java 6 era code. It has not been
verified to still work.

## License

The legacy tree is under the Sun/Oracle terms in `LICENSE`. New code is
unlicensed so far, pick something before the first release.
