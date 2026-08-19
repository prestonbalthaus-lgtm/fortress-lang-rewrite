# Fortress M4: parallel execution — architecture

Date: 2026-08-19
Status: **design for review. No compiler code written.**

1.0 `basic/expressions/for.tex:27-31`: *"the programmer must assume that each
loop iteration will occur independently in parallel unless every generator is
explicitly `sequential`."* That is the mandate. Two lines of the same section
size the milestone:

* **"The value and type of a `for` loop is `()`."** A loop returns nothing, so
  a first cut needs no reduction machinery at all.
* **`\note{Reduction variables are not yet supported.}`** — 1.0 shipped without
  them. Leaving them out of M4 follows the specification rather than dodging it.

Everything below rests on measurements taken on this machine (14 cores), not on
what the Boehm documentation says. The numbers are the design.

## 1. Syntax and AST

```
for GeneratorClauseList DoFront end
DoFront ::= [at Expr] [atomic] do [BlockElems]
```

`for` is a reserved word today and the parser refuses it by name. **The lexer
does not change.** `<-` is not a token and does not need to be: it is `Lt`
glued to `Minus`, decided by span adjacency exactly as `->` already is in
`type_ref` (`Minus` glued to `Gt`). Adding a token would change how every file
in the corpus lexes, for nothing.

New AST node:

```rust
Expr::For {
    binder: String,          // one binder in M4; a tuple binder is a later step
    generator: Box<Expr>,
    sequential: bool,        // set by `seq(...)`, which is stripped here
    body: Box<Expr>,
    span: Span,
}
```

`seq(g)` is recognised **in the parser**, not as a function call, and is what
sets `sequential`. That matters more than it looks: of 1525 `for` occurrences
in the corpus, 236 are written `for i <- seq(...)`, so the sequential path is
not an afterthought — it is what most existing code asks for.

Generators in M4: an integer range (`a:b`, `a#b`) and an `Array`. Everything
else is a diagnostic naming the generator form, not `unknown name`.

## 2. The runtime engine

**pthreads directly, in `shims.c`. Not OpenMP, not a work-stealing scheduler.**

OpenMP would put a second parallelism model and a second runtime (`-lgomp`)
into every link, and the cluster image would have to carry a matching one. The
guidelines already say generated code calls C shims only and never names an
implementation-specific symbol; `#pragma omp` in generated IR is exactly that.
Work stealing is the right answer eventually and the wrong first answer: it
cannot be proven correct by a gate that a static split can.

One entry point, and the body is **outlined** into a real function so a worker
can call it:

```c
typedef void (*fortress_loop_body)(int64_t index, void *env);
void fortress_parallel_for(int64_t lo, int64_t hi, fortress_loop_body body, void *env);
```

* A fixed pool of `min(nproc, 16)` workers, created once by
  `fortress_runtime_init` and joined at exit. Thread creation per loop is
  ~25 µs and would dominate every short loop.
* **Static contiguous chunks**, `[lo, hi)` split into one range per worker. No
  dynamic queue, no atomics on the hot path, and the split is a pure function
  of `(lo, hi, workers)` — which is what lets a gate compute the expected
  partition itself instead of reading it back out of the runtime.
* Below a threshold (~4096 iterations) it runs the range inline on the calling
  thread. A parallel loop that is slower than a serial one is a bug.
* Nested parallel `for` runs the inner loop sequentially. One pool, one level.

**Codegen's real work is outlining, not threading.** The body becomes a
`TypedFn` taking `(index, env)`; captured values go into one `#[repr(C)]`
environment struct **allocated once, before the loop**. That placement is not
tidiness — see §3.

## 3. Memory safety and the GC

Measured, not assumed.

**The installed collector is already thread-aware.** `libgc.so` exports
`GC_pthread_create`, `GC_register_my_thread`, `GC_allow_register_threads`. So
there is no `gc_pthreads` to link and **no `-lpthread` either** — glibc folds
pthreads into libc, and a 320k-allocation, 8-thread stress program links and
runs clean against `-lgc` alone.

**One line makes it correct**, in `shims.c`:

```c
#define GC_THREADS
#include <gc.h>
```

`gc.h:1807` then pulls in `gc_pthread_redirects.h`, whose line 103 is
`#define pthread_create GC_pthread_create`, so a plain `pthread_create` call
registers the thread with the collector transparently.

**Shown to fail, not merely asserted.** The same program with `GC_THREADS`
removed aborts **3 runs out of 3** with Boehm's own diagnostic:

```
Collecting from unknown thread
```

That is the M4 gate's first mutation, and it is already written.

### The finding that shapes the whole milestone

An allocating parallel loop on this collector is **slower than the serial one**:

| loop body | 1 thread | 8 threads | speedup |
|---|---|---|---|
| no allocation, trivial | 0.010s | 0.002s | **5.12x** |
| no allocation, 20 ops/iteration | 0.047s | 0.006s | **7.35x** |
| one `GC_malloc(32)` per iteration | 0.276s | 0.464s | **0.60x** |
| `GC_malloc(32)` + 20 ops | 0.087s | 0.092s | **0.94x** |

Two mechanisms could explain it and they have different fixes, so they were
separated. With collections **disabled** and a 1 GB heap pre-expanded, the same
allocating loop scales again — 1.77x / 2.07x / 1.82x at 2 / 4 / 8 threads.

So the dominant cost is **stop-the-world collection with single-threaded
marking**, not the allocation lock. `GC_get_parallel()` returns **0**: Fedora's
`gc` package is built without `--enable-parallel-mark`, and `nm -D` shows no
thread-local-allocation symbols either.

Three consequences, in order of how much they buy:

1. **`setup-gc.sh` should build libgc from source** with
   `--enable-parallel-mark --enable-thread-local-alloc` instead of unpacking
   Fedora's RPM. It is the same script, a different source, and it is the
   single largest lever on parallel performance. Measure before and after.
2. **The loop environment is allocated once, outside the parallel region**, and
   the M4 subset keeps allocation out of loop bodies entirely.
3. Until (1) lands, the honest claim is *"parallel loops scale on
   allocation-free bodies"* — with the table above printed, not a bare
   "M4 is parallel".

## 4. Scope boundary

The whole point is to prove the engine with **no data-race analysis**. Every
rule below is a syntactic check on the typed tree, not a dataflow one.

**In:**

* exactly one generator, over an integer range or an `Array`
* body typed `()`, which is what the specification says a loop is anyway
* reads of any binding from the enclosing scope
* `a[i] := e` where `i` **is** the loop binder, by name — distinct iterations
  touch distinct slots because the index function is the identity, and that is
  checkable by looking at the expression rather than by proving anything

**Out, each with a diagnostic that names the construct:**

* assignment to any binding declared outside the loop — this is the race, and
  refusing it syntactically is what buys the whole milestone
* `a[e] := ...` for any `e` that is not the bare binder
* `atomic` (102 corpus files use it; none of them are M4)
* reduction variables, `at`, tuple binders, nested generators
* a nested parallel loop, which is demoted to sequential rather than refused

**Expect the corpus number barely to move, and say so up front.** 325 files
contain a `for` loop and most need generators, tuple binders or `atomic` that
M4 does not have. Every milestone so far was judged on `COMPILE_FLOOR`; this
one should not be. Its evidence is a gate, not a ratchet:

* **correctness** — a 1M-element array filled in parallel, every slot written
  exactly once, byte-identical to the serial result over 20 runs
* **speedup** — wall clock on an allocation-free body, asserted `> 2x` on 8
  workers, with the number printed
* **the partition** — computed by the gate from `(lo, hi, workers)` and compared
  against what the runtime actually ran, so the gate does not take its answer
  from the thing it is testing
* **the GC rule** — `GC_THREADS` removed, and the abort shown

## Open question for you

**Do I build the collector from source in `setup-gc.sh` as part of M4, or land
M4 on the stock RPM and treat the GC rebuild as its own milestone?** The
measurement says parallel performance is a GC property before it is a compiler
property, so doing it inside M4 makes M4's numbers honest — but it also means
M4 changes the build prerequisites for every developer and for the cluster
image, which no previous milestone has done.
