# Fortress

Fortress is a parallel programming language built at Sun Labs as their entry in
the DARPA HPCS program, alongside Cray's Chapel and IBM's X10. It has implicit
parallelism, traits and polymorphism, transactional `atomic` blocks, generators
and comprehensions, and mathematical notation that renders as real math. Sun
shipped a working interpreter, then cancelled the project. Upstream development
stopped in 2012.

This repository is a fork of that codebase plus an ahead of time compiler being
written from scratch in Rust, targeting LLVM.

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

A native AOT compiler removes all four. The target is a language you can run
under Slurm on real hardware.

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

About 1950 `.fss` and `.fsi` files are scattered across `Library/` and
`ProjectFortress/`. Those are valid Fortress programs and they are the
conformance suite for the new compiler.

The legacy build is Ant and Java 6. It has not been verified to still work.

See [ROADMAP.md](ROADMAP.md) for the plan.

## License

The legacy tree is under the Sun/Oracle terms in `LICENSE`. New code is
unlicensed so far, pick something before the first release.
