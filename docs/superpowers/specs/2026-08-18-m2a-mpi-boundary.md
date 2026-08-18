# Fortress M2a: the MPI boundary

Date: 2026-08-18
Status: implemented, gated

M2 is "MPI hello across two nodes". M2a is the half that can be proved on this
laptop: the language surface, the C ABI boundary, the link path and the image.
M2b is Slurm, `sbatch`, and two real compute nodes over InfiniBand. Splitting
them keeps the cluster out of the loop while the boundary is still moving.

## The one decision everything else follows from

`MPI_COMM_WORLD` is not a value. It is a macro, and its expansion is
implementation specific:

* OpenMPI expands it to `&ompi_mpi_comm_world`, the address of a global struct.
* MPICH expands it to the integer constant `0x44000000`.

A compiler that emits either one into LLVM IR is pinned to whichever MPI the
build host had when the compiler was built. On a cluster that is a silent wrong
answer, not a link error, because the two are the same size.

So generated code never names an MPI symbol at all. It calls four shims with a
`fortress_mpi_` prefix, and `runtime/mpi_shims.c` is the only file in the tree
that includes `<mpi.h>`. That file is compiled by the cluster's own `mpicc`,
against the cluster's own headers, at link time.

The prefix is doing work too. `libmpi` exports `MPI_Comm_rank`, the Fortran
bindings export `mpi_comm_rank_`, and a Fortress program is allowed to define a
function called `mpiCommRank`. Prefixed shim symbols collide with none of them.

## The language surface

Four builtins, all arity zero:

| Fortress | Type | Symbol |
|----------|------|--------|
| `mpiInit()` | `()` | `fortress_mpi_init` |
| `mpiCommRank()` | `ZZ32` | `fortress_mpi_comm_rank` |
| `mpiCommSize()` | `ZZ32` | `fortress_mpi_comm_size` |
| `mpiFinalize()` | `()` | `fortress_mpi_finalize` |

No communicator argument, because there is no communicator type yet and
inventing one before it is needed would be a guess. `MPI_COMM_WORLD` is fixed
inside the shim.

They resolve in `Checker::call` ahead of user functions, the same way `println`
and `widen` do. `MPI_Init` is passed `NULL, NULL` rather than `argc`/`argv`,
which MPI-2 onwards permits.

The M1 rules apply unchanged. A rank is `ZZ32`, and `f():ZZ64 = mpiCommRank()`
is `ImplicitWideningRejected` with the usual "write `widen(...)`" diagnostic;
there is a test for exactly that, because a builtin is the obvious place for the
rule to quietly not hold.

## uses_mpi

`TypedComponent` carries `uses_mpi`, set when any function resolves an MPI
target. Two things read it:

* Codegen declares the four externs only for a component that uses one. A
  program that never touches MPI therefore names no MPI symbol in its IR, and
  `fortressc tests/fact.fss` still links with a plain `cc` against nothing but
  libc. The M1 guarantee is not weakened by MPI existing.
* The driver puts `mpi_shims.c` into the link only when it is set. That file
  cannot be compiled where there is no `<mpi.h>`, so linking it unconditionally
  would make every Fortress program need an MPI installation.

## Where each half runs

The compiler is a build-host tool and stays one. Rocky 9 has no LLVM 22, and a
binary built against Fedora 44's glibc will not run on glibc 2.34, so building
`fortressc` inside the cluster image was never on the table.

```
host (Fedora 44, LLVM 22)          image (Rocky 9, OpenMPI 4.1.1)
  fortressc  ── .o ──────────────►  mpicc links it
                                    mpirun launches it
```

Two driver flags carry that split:

* `--emit-obj` stops after the object and writes it at exactly `-o`. Compiling
  and linking happen on different machines under different C libraries.
* `--cc <driver>` overrides the link driver, default `cc`. Pointing it at
  `mpicc` is the whole of the compiler's MPI knowledge.

`tools/mpicc-in-image.sh` is a `--cc` that runs `apptainer exec $SIF mpicc`, so
the local gate exercises the real flag and the real link rather than a stub.
The object is x86-64 PIC and references only our own symbols, so it links
cleanly against a C library it was not compiled against.

## The gate

`tools/mpi-gate.sh`, 17 checks. Two of them are about the gate itself.

**The assertion is a set comparison.** `mpirun` interleaves stdout, so an
ordered diff would be flaky. The gate builds the expected multiset
`{rank 0 of N .. rank N-1 of N}`, sorts both sides and compares. A missing
rank, a duplicated rank and a wrong world size all fail it.

**The self test runs first, every time.** It feeds the assertion four known
inputs: a correct two-rank set, two ranks presented as three, a duplicated rank
and a short set. An assertion that always returns true and one that always
returns false both fail this. `--selftest` runs it alone, with no image.

**The singleton control** runs the binary with no launcher. OpenMPI's singleton
init gives a world of one, and the check requires exactly `rank 0 of 1`. It is
what makes `-np 4` mean something: without it, a program that printed a
hardcoded string would pass every launcher case that only looked for "rank".
Note that it still runs through `apptainer exec`, like everything else the image
linked. Running it on the host would fail on glibc and look like a gate bug.

**Both mutations were run before the gate was trusted.**

* `MPI_COMM_WORLD` changed to `MPI_COMM_SELF` in `fortress_mpi_comm_size`:
  gate 15/2, exit 1, and the diagnostic shows four processes each reporting
  `of 1`. That is the proof the rank counts come from MPI rather than from a
  constant the program could have printed on its own.
* `uses_mpi` never set: gate 6/4, exit 1, `no runtime symbol
  fortress_mpi_init`, and every downstream check refuses for want of an
  artifact rather than passing on a stale one.

The second mutation found a real hole on its first run. The gate reused
`fortressc/build/`, so a failed compile left the previous run's binary in place
and the rank checks measured it. `preflight` now wipes the directory and every
later stage refuses outright if the artifact it needs is missing.

## The image

`apptainer/fortress-mpi.def`, Rocky 9, `gcc binutils openmpi openmpi-devel`,
built with `--fakeroot`. It carries `mpicc`, `mpirun` and `libmpi` and nothing
Fortress specific, so the compiler can change without rebuilding it.

Rocky keeps the MPI toolchain at `/usr/lib64/openmpi/bin` and expects an
environment module to put it on `PATH`. There are no modules in a container, so
`%environment` sets `PATH` and `LD_LIBRARY_PATH` directly and `%post` asserts
both binaries exist at build time.

`OMPI_MCA_btl_vader_single_copy_mechanism=none` is set because cross memory
attach needs `ptrace` on the peer, which Yama blocks between unrelated processes
on most hosts. Without it the shared memory transport fails at rank startup.

The image does not disable the OpenFabrics BTL, so a run on a host with no
active InfiniBand port prints a warning per rank. That noise is left in on
purpose: the cluster does have InfiniBand and silencing it in the image would
hide the case that matters. The gate filters on `^rank N of M`, so the warnings
cannot affect a result.

## What M2a does not do

No Slurm, no `sbatch`, no second node, no InfiniBand fabric. Four ranks on one
laptop under `mpirun`.

Also unresolved, and a real constraint on M2b: `write_object` builds the target
machine from `get_host_cpu_name` and `get_host_cpu_features`. The build host and
the compute node are the same machine here and will not be on the cluster. A
`--target-cpu` flag, or a default of `x86-64-v3`, is needed before anything is
compiled on a login node and run on a Platinum 8160.
