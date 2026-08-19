# Fortress M4: parallel execution — what shipped

Date: 2026-08-19
Status: **landed** on `m4/parallel-design`, not pushed.
Design reviewed and approved as
`2026-08-19-m4-parallel-execution-design.md`; this note records what the code
does and where the design was **wrong**.

Legacy corpus **262 -> 266**, parse **625 -> 637**. Nine gates green, 257 cargo
tests, clippy 0. `tools/parallel-gate.sh` **26/0**, six mutations, none survived.

## The four success criteria, measured

| criterion | floor | measured |
|---|---|---|
| 1M array filled in parallel, byte identical to serial | identical | **identical** at 1, 4 and 14 workers, sha256 of all 1 000 000 lines |
| the static partition matches an independent computation | exact | **exact** on 5 cases, including uneven splits and ranges smaller than the worker count |
| speedup, allocation-free body | > 2x | **6.4x** on 8 workers, 9.6x on 14 |
| speedup, allocating body | > 1x | **4.2x** on 8 workers |

```
parallelcollatz  (no allocation)      1141ms -> 177ms on 8, 118ms on 14
parallelalloc    (3M string allocs)    226ms ->  53ms on 8
```

## Where the design document was wrong

**It claimed the distribution collector had no parallel marking.** It does.
`GC_get_parallel()` returns 0 until the marker threads actually start, and the
probe that produced the claim called it immediately after `GC_INIT()` and before
any thread existed. Called after `GC_allow_register_threads()`, both the
distribution build and the source build report **13 markers**.

So the rebuild's justification is narrower than the design said, and the honest
list is:

* **static linking** — the operational win, and the real one. A Fortress binary
  now carries its collector and needs no `LD_LIBRARY_PATH`, which is what makes
  it launchable under `srun`.
* **we own the version and the flags**, rather than inheriting a distribution's.
* **thread-local allocation**, worth about 20% at 4 threads in the C
  microbenchmark (3.74x against 3.13x).

The thing that actually made allocating loops scale was **neither**: it was heap
policy. An allocating loop measured **1.14x** on 8 threads with the default heap
and **3.28x** with 64 MB of headroom, because N threads allocate N times faster
and therefore collect N times as often, and every collection is stop the world.

`fortress_runtime_init` does **not** apply that policy. It is applied on the
**first parallel loop**, by `fortress_parallel_heap`, so a program with no
parallel loop keeps exactly the memory behaviour it had — which is also what
keeps `tools/memory-gate.sh` measuring something, since a pre-expanded heap
would make its leak ratio pass trivially.

**Stated, because it is a claim I could not reproduce:** the heap policy makes
no measurable difference to `parallelalloc`, which *retains* every string it
allocates in the array — nothing is garbage, so the heap has to grow regardless.
It is measured to matter for garbage-heavy loops in C and I could not build an
in-language workload where it did. It is kept on the C measurement and this
sentence, not on a Fortress one.

## The engine

`fortress_parallel_for(lo, hi, body, env, requested)` in `runtime/shims.c`.
Fixed pool, `min(nproc, 16)`, created on first use. **The calling thread takes
chunk 0 and runs it itself**, so P-way parallelism costs P-1 threads and a
one-core machine spawns none.

The split is a pure function of `(lo, hi, workers)`, exported as
`fortress_parallel_chunk` so the gate can compute the same partition in bash and
compare. The first `remainder` chunks take one extra iteration, so every index
belongs to exactly one worker.

Three ranges never reach the pool: shorter than 4096, already inside a parallel
loop, and `seq(...)`. The last is a promise about **order**, so it is honoured
whatever the size — 5000 iterations, above the inline threshold, still print
0..4999 in order.

### GC

`#define GC_THREADS` before `<gc.h>`, which makes `gc.h:1807` pull in
`gc_pthread_redirects.h` and redefine `pthread_create` as `GC_pthread_create`.
Without it the program aborts with the collector's own **`Collecting from
unknown thread`**, and that is the gate's first mutation.

## Codegen: the outliner

The body becomes a real function `$loopN(i64 index, ptr env)`. The values it
reads from the enclosing scope are copied into one environment struct,
**allocated once, before the loop** — allocation inside the parallel region is
what the heap measurement above is about, and the end-to-end test asserts
exactly one `fortress_env_alloc` call in the enclosing function.

The captures are not recomputed by walking the typed tree. The **scope stack
already knows**: a lookup that resolves below the loop's floor crossed the
boundary, so `lookup_capturing` records it as it happens.

The environment is **scanned**, not atomic: a capture may be a String or an
Array and the collector has to see through the environment to it while a worker
still holds it.

## The scope boundary

Every rule is syntactic. There is no dataflow analysis in M4 and none is needed.

```rust
fn escapes_loop(&self, name: &str, floor: usize) -> bool {
    matches!(self.depth_of(name), Some(depth) if depth < floor)
}
```

A parallel body may not assign to a name that resolves **below** the loop's own
scope. That one comparison is the whole of M4's race freedom.

Array element assignment takes one more rule, and **the first version of it was
wrong**: it required the index to be the binder, which refused `scratch[0] := i`
for an array the body had *created itself*. A loop-local array is fresh per
iteration, so any index into it is private. The rule is now: **the base must be
loop-local, or the index must be the binder.**

Out, each with a diagnostic that names it: `atomic`, reduction variables, `at`,
tuple binders, array generators, and a body with a value.

## What the gates caught

* **The MPI gate caught a portability break.** Apptainer passes the host
  environment through and bind-mounts `$HOME`, so the in-image link was picking
  up the *host's* static `libgc.a` — compiled against Fedora's glibc, and it
  fails inside Rocky 9 with an undefined `__isoc23_strtol`. The image builds the
  same collector from the same tarball now, and `mpicc-in-image.sh` points
  `CPATH`/`LIBRARY_PATH` at the image's own prefix.
* **The memory gate caught two assertions testing the wrong thing.** A static
  collector's symbols are *defined* in the binary, not undefined, and `ldd`
  showing no libgc is now the assertion rather than the failure.
* **And a trap worth keeping:** `nm ... | grep -q` under `set -o pipefail`
  reports failure even when the symbol is found — `grep -q` exits on the first
  match, `nm` takes SIGPIPE, pipefail fails the pipeline. The old form survived
  only because `nm -u` output fit in the pipe buffer. Counted with `grep -c`.
* **A mutation that survived, and was right to.** `task.workers = parallelism -
  1` leaves a worker idle but still covers every index, so it is a performance
  change and not a defect — nothing should catch it. Replaced with a real one:
  the calling thread skipping its own chunk.

## The corpus metric had a leak

`examples/` at the repository root is hand-written demo code, and one of its
files was compiling and inflating the count. Both walkers skip it now — **by
path, not by name**, because `SpecData/examples` *is* corpus and pruning the
name took 137 legacy files out of the metric before the mistake was caught.

## Next

`atomic` is the obvious next parallel construct — 102 corpus files use it, and
it is what a reduction needs before reduction variables can exist. Array
generators (`for x <- a`) and tuple binders are the two cheapest widenings of
the subset. The pool is static by choice; work stealing is the answer when a
gate exists that can prove it correct, and not before.
