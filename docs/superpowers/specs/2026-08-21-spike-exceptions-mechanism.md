# SPIKE-EXCEPTIONS-MECHANISM — the branch, measured

**Date:** 2026-08-21. **Tree:** master `f81f41ace`; nothing in the compiler was
changed to run this. **Verdict: `invoke` SURVIVES.** Exceptions can land as an
ordinary codegen feature. The threaded error-return ABI does **not** win, so
**every generated call signature does not have to change** — which is the
sentence that unblocked `SPIKE-CLOSURE-REPRESENTATION`, and the reason that
spike was run second rather than first.

The gap analysis (§3.2) asks for exactly this and says so: *"a mechanism spike,
not a corpus-count spike"*. No file count is quoted below because none would
mean anything yet.

---

## How it was run

Everything happened **off-tree**, in the scratchpad. The method is the one §3.2
prescribes: take the IR the real compiler emits for a program with a parallel
loop and one with an `atomic` region, hand-edit the chosen `call` into an
`invoke` with a landing pad and a personality routine, link the result against
the repository's own `runtime/shims.c`, and *observe*.

Toolchain, verified before being relied on rather than after:

| claim | measured |
|---|---|
| `/usr/bin/llc` is LLVM 22.1.8 | **confirmed** — `LLVM version 22.1.8`, default target `x86_64-redhat-linux-gnu` |
| no `clang`/`clang++` on the box | confirmed; `g++` compiled the throwing scratch code, `cc` compiled `shims.c` |
| **`lld` is not installed** (`02-stack.md`, and the brief repeated it) | **WRONG.** `/usr/bin/lld` and `/usr/bin/ld.lld` both exist — `LLD 22.1.8 (compatible with GNU linkers)`. Re-checked by hand. Nothing was changed to use it; `cc` stays the driver's linker. **`02-stack.md` should be corrected.** |

---

## 1. A parallel loop body

Three cases, and only the third is a defect.

**(1a) Thrown inside a worker thread's outlined body with no handler in the
body → the process ABORTS.** Exit 134 (SIGABRT), `terminate called after
throwing an instance of 'FortressBoom'`, core dumped. Reproduced at
`FORTRESS_WORKERS=2` (threw at index 99999, chunk 1) and at `4` (chunk 3). It
does not cross the done-wait, does not terminate just that thread, and does not
hang.

This is **not** an `invoke` defect and must not be recorded as one: C++
two-phase unwinding never crosses a thread boundary anywhere. Phase 1 finds no
handler, reaches end-of-stack, and `__cxa_throw` calls `std::terminate`.

**(1c) The outlined body catches it ITSELF → works.** `invoke` + `landingpad`
inside `$loop1`, running on the worker: *"distinct chunks that ran = 4, max
chunk id = 3 (DISTRIBUTED)"*, *"exception caught INSIDE the worker body =
YES"*, exit 0. **Catch-and-carry at the outlined-body root is one boundary, not
a change to every call signature.**

**(1b2) Unwinding THROUGH `fortress_parallel_for` — the finding.** Chunk 0 runs
on the calling thread (`shims.c:296-298`), so an exception thrown there with
`invoke void @fortress_parallel_for` and a landing pad in `run()` **does** cross
the plain-C frame and **is** caught. Exit 0. And the runtime is left damaged,
silently:

* `shims.c:297` sets `fortress_in_parallel = 1`; the reset at `:299` is skipped
  by the unwind, so **the very next parallel loop reported `distinct chunks that
  ran = 1, max chunk id = 0 (RAN INLINE)`** instead of distributing over 4. No
  error, no diagnostic, exit 0 — the program simply stops being parallel on that
  thread, for good.
* The done-wait at `:301-305` is skipped too. An earlier, unsettled version of
  the run showed chunk flags contaminated by workers still running after `run()`
  had unwound past the join, which is how the skipped join made itself visible.

**The rule the data forces:** an exception must never unwind *through*
`fortress_parallel_for`. The body root catches, stashes, and the caller rethrows
after the join has returned normally.

---

## 2. An `atomic` region

**Verdict: survives, entirely conditional on codegen emitting a cleanup pad that
calls `fortress_atomic_leave`.**

**A method correction first, because it is reusable.** The obvious probe — catch
outside the region, then re-enter `atomic`, and call a deadlock proof that the
mutex was held — **cannot detect the fault**. The mutex is
`PTHREAD_MUTEX_RECURSIVE` (`shims.c:344-350`) and `fortress_atomic_depth` is
`__thread` (`shims.c:341`), so the thread that unwound already owns the lock and
re-entering always succeeds. It printed *"same-thread re-enter+leave RETURNED
(!)"* in **both** the broken and the fixed variant, and the depth counter went
1 → 2 → 1 so that probe's own leave never unlocked either. **The real detector is
a SECOND thread calling `fortress_atomic_enter` under `pthread_timedjoin_np`
with a deadline.**

| variant | what codegen would be doing | result |
|---|---|---|
| **A** — landing pad catches, no `fortress_atomic_leave` | no cleanup pad emitted; the straight-line leave is jumped over | **`PROBE: BLOCKED — other thread could not enter atomic in 3s (rc=110) => MUTEX STILL HELD`**. `rc=110` is `ETIMEDOUT`. The process-wide mutex is leaked for the life of the program. And the next parallel loop reported `RAN INLINE`, because `fortress_atomic_enter` sets `fortress_in_parallel = 1` at depth 0 (`:355-357`) and only the leave restores it (`:362-364`). |
| **B** — landing pad calls the leave before completing the catch | one leave per enter | `PROBE: OK — other thread entered atomic (mutex was free)`, immediately; the next parallel loop `DISTRIBUTED` over 4. Exit 0. |
| **C** — nested depth 2, handler in a CALLER, pad is `cleanup` only (two leaves then `resume`) | the shape codegen actually emits when the handler is not local | `PROBE: OK`; `DISTRIBUTED`; exit 0. **Phase-2 cleanup pads unwind nested atomic depth 2 to 0 correctly**, with no extra mechanism. |

**Independently reproduced.** I re-ran `atomA.bin`, `atomB.bin`, `atomA2.bin`
and `atomB2.bin` myself rather than taking the agent's word: A and A2 print
`MUTEX STILL HELD` (rc=110) and A2 additionally `RAN INLINE`; B and B2 print
`OTHER THREAD ACQUIRED ATOMIC`, and B2 `DISTRIBUTED` over 14 chunks at this
machine's worker count.

**This is what makes `ExitCrossesAtomic` a refusal rather than a nicety.**
`label`/`exit` landed the same day, and `atomic.tex:59-70`'s writes-retained arm
re-opens with it: an `exit` crossing an `atomic` boundary would jump past the
leave, which is variant A, which is a leaked process-wide mutex. It is refused
by name.

---

## 3. Boehm under a foreign unwind

**Tested, with a negative control, so the green means something.** Method copied
from `runtime/tests/array_trace.c`: reading the bytes back is not a test, so the
measurement is live heap after a forced collection with one array as the only
reference. 1024 slots, each an 8 KiB heap string from the real
`concat_string_string` — 8,388,608 bytes of payload — built through the real
`fortress_array_alloc(count, 8, holds_pointers=1)`, thrown through three nested
C++ frames holding live GC locals, caught in generated IR, then `GC_gcollect()`
twice.

| | live bytes | payload | verdict |
|---|---|---|---|
| main thread | 12,599,296 | 8,388,608 | `TRACE OK`, length 1024, corrupt 0, exit 0 |
| GC-registered worker thread (`FORTRESS_WORKERS=4`) | 12,599,296 | 8,388,608 | identical; all 4 chunks ran |
| **negative control** (slots on `GC_malloc_atomic`) | **28,672** | 8,388,608 | `TRACE REFUSED` — **439×** apart |

A 200,000-allocation soak after the unwind completed normally in both live
cases, and the array was still length 1024 after a further collection.

**Not tested, stated rather than implied:** throwing from inside a GC callback or
a finalizer, and a collection triggered *while* an unwind is in progress.

---

## 4. What this obliges, if exceptions are built

1. **An exception may not unwind through `fortress_parallel_for`.** Catch at the
   outlined-body root, stash, rethrow after the join returns normally. Otherwise
   `fortress_in_parallel` stays set and the process silently stops being
   parallel.
2. **Every `atomic` region needs a cleanup pad**, one `fortress_atomic_leave`
   per `fortress_atomic_enter`. Nesting needs nothing more: per-region pads
   compose (variant C).
3. **The detector for (2) in any future gate is a second thread under a
   timed join.** A same-thread re-entry probe passes on a broken compiler.
4. Boehm needs nothing, on either thread kind.
5. There is still no bottom type, so `var a: ZZ32 = throw FooExn` has no typing,
   and `Exception`/`UncheckedException` do not exist as names. Those are the
   type-side work the branch answer unblocks, not part of it.

**Confidence: high on (1) and (2)** — each was produced *and* refuted on demand,
which is this project's standard for a gate. **Lower on the interaction with a
collection that starts mid-unwind**, which was not measured and should be before
any of this ships.
