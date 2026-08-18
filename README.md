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

The compiler lives in `fortressc/`: a six crate Rust workspace, lexer through
LLVM codegen. See `docs/superpowers/specs/` for the M1 design, the lexer plan
and the M2a MPI boundary, all of which record why the rules are what they are.

What M1 does not do: traits, objects, generics, arrays, `for`, parallelism,
`atomic`, or user definable syntax. It parses about 9% of the legacy corpus,
which is the point rather than a shortfall.

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
export LLVM_SYS_221_PREFIX=...        # the script prints the value
cargo build --workspace
cargo test --workspace
```

Needs LLVM 22 and a C compiler. `setup-llvm.sh` exists because Fedora splits
LLVM across `llvm-libs` and `llvm-devel`, and the latter needs root; the script
unpacks it into `~/.local` instead. With root, `dnf install llvm-devel` does the
same job.

The legacy interpreter builds with Ant against Java 6 era code. It has not been
verified to still work.

## License

The legacy tree is under the Sun/Oracle terms in `LICENSE`. New code is
unlicensed so far, pick something before the first release.
