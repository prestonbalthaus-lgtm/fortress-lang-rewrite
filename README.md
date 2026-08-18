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

The rewrite has not started. There is no Rust in this tree yet. Everything here
is the legacy Sun implementation, kept as reference material.

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

Nothing to build yet. The new compiler does not exist.

The legacy interpreter builds with Ant against Java 6 era code. It has not been
verified to still work on a current toolchain, and getting it running is phase 0
of the roadmap precisely because the answer is unknown.

See [ROADMAP.md](ROADMAP.md) for the plan.

## License

The legacy tree is under the Sun/Oracle terms in `LICENSE`. New code is
unlicensed so far, pick something before the first release.
