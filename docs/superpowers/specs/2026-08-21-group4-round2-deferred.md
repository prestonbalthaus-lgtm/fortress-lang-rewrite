# Group 4, round 2 — what was not built, and the measurement that says why

**Date:** 2026-08-21. **Branch:** `spike/group4-codegen`. Everything below was
costed against the corpus and the runtime before being left out; none of it is
"ran out of time". Each item names what would unblock it.

The round landed `fn` with captures, array generators, BIG reductions over
ranges (SUM/PROD/MAX/MIN), `also do`, and three silent-acceptance fixes. This is
the other half of the list.

---

## 1. `spawn` — NOT LANDED, and serialisation does not rescue it

`also do` was serialised on a licence: `parallelism.tex:88-90` permits an
implementation to serialise any group of IMPLICIT threads, and `also.tex:24-27`
leaves a group with no value to combine. **Neither sentence covers `spawn`**, and
the corpus refutes the equivalent shortcut directly.

**Option D, run the body inline at the `spawn` site**, is blessed by
`parallelism.tex:139-147` and killed by two corpus lines:

* `ProjectFortress/tests/Spawn3.fss:19` spawns a body containing
  `while (x = 0) do end`, and the parent stores `x := 1` at `:20`. Inline, that
  is an infinite loop.
* `ProjectFortress/tests/Spawn6.fss:25` asserts `ready()` is **false**
  immediately after the spawn. Inline, it is always true.

So `spawn` needs a real thread of some kind. Four options, each measured against
this runtime rather than against a design note:

| option | what it costs | verdict |
|---|---|---|
| **A** — a detached `pthread` per spawn | `fortress_in_parallel` starts at 0 in the child, so a `for` inside a spawned body reaches the pool and clobbers the single `fortress_task` — and `Compiled160.fss:22-25` and `Compiled6.aa.fss:18-21` are exactly `spawn <for loop>`. Fixable by pinning the flag in the trampoline. Unbounded thread creation (`Library/Lazy.fss:39` spawns per lazy thunk) and one more stack for every stop-the-world collection, which `shims.c:188-196` already calls the dominant parallel-GC cost. | workable, expensive |
| **B** — a second dedicated pool | **Killed by the corpus on its own terms.** A spawned thread has unbounded lifetime: `Spawn3.fss:19` runs until the parent stores. A bounded pool of size N deadlocks at N+1 live spawns and nothing bounds how many a program makes. | rejected |
| **C** — a task queue on the existing pool | Replaces the single `fortress_task` plus generation-broadcast with a queue and per-task completion state — **and that state IS the handle**, so `Thread[\T\]` falls out of the refactor rather than being bolted on. Zero new GC cost, no new threads. Starvation is constructible (a task blocking on another task) but **no corpus file exercises it**; `FORTRESS_WORKERS=1` degenerates to inline, which is option D and its refutations. | the right answer, and a milestone |
| **D** — inline | refuted above | rejected |

**The recommendation is C, as its own milestone**, because it is a rewrite of the
pool's central data structure and the failure mode of getting it wrong is a
HANG — the class atomic-gate mutations 1 and 4 already measure, where a worker
parks on a mutex the exiting thread holds. It should not ride along with a
feature.

**One hazard that survives every option** and is worth writing down now:
`fortress_atomic_depth` is `__thread`, so a spawned child correctly does not
inherit the parent's recursion count — and therefore BLOCKS on the process-wide
mutex if the parent holds it. `spawn.tex:28-31` forbids spawning inside `atomic`,
which closes the direct case and not the indirect one: `t.val()` inside an
`atomic`, on a thread spawned outside it, still deadlocks.

---

## 2. Tuples — NOT LANDED, and the type variant is not the work

`SPIKE-COMPOSITE-TYPE` priced the type-level half and it is cheap: one interned
`Tuple(&'static [Type])`, `Copy` preserved, **4 forced match arms, 2 files**.
That is real and it is not the milestone. What it does not provide:

**A run-time representation, because there is no boxing.** `basic_type` is total
over `Type` and has no aggregate case: scalars are machine types, everything else
is a pointer, `Void` is `None`. There are exactly two LLVM struct types in the
whole backend — the object layout and the loop environment — and both are blocks
reached by pointer. So a tuple value is one of three things:

1. **A heap block like an object.** An allocation per tuple, which for
   `(a, b) := (1, 2)` in a loop is the nested-environment problem this project
   already lists as debt. Gets `Type::Tuple` in every position for free.
2. **Never materialised** — a tuple lives only as an argument list and a
   multi-value return, `f(x: (ZZ32, ZZ32))` lowering to `i32 (i32, i32)`. This
   is precisely what `overloading.tex:124-126` describes, and it is what makes
   `tupleTypeParam2.fss` correct on purpose rather than by luck. Needs LLVM's
   literal struct return, fine at -O0. Cannot be stored in an array or a field,
   which matches `Elem`'s existing design intent.
3. Both, with (2) as the calling convention and (1) as the boxed fallback.

**(2) is the one to build first**, and the reason is conformance rather than
cost: `overloading.tex:124-126` makes a functional's parameter a single value
that MAY be a tuple, so `f(x: (ZZ32, ZZ32))` and `f(a: ZZ32, b: ZZ32)` are the
same declaration. M3c's symmetric dispatch compares `Vec<Type>` elementwise and
would have to arity-flatten. `ProjectFortress/tests/tupleTypeParam2.fss` is the
live witness and it currently prints 7 only because its abstract member is
EXCUSED — see `excusable` in `crates/types/src/lib.rs`.

Census: 213 type sites, 653 multi-target assignments in 131 files, 182
unambiguous tuple expressions, **37 first blockers**. Also 64 sites in 21 files
are `typecase`/`case` arm binders — one feature away now rather than two, since
this branch landed typecase.

**Interaction to remember:** `closure.rs`'s `liftable` refuses a tuple domain,
which is why `fortressc/tests/badarrowtype.fss` had to move to
`(ZZ32, ZZ32) -> String`. The day tuples become storable that fixture stops
refusing and must move again IN THE SAME COMMIT, or it silently becomes a test
of nothing.

---

## 3. The numeric tower — NOT LANDED, and its payoff is not corpus files

| name | sites | files | non-library files | first blockers |
|---|---|---|---|---|
| `RR32` | 121 | 14 | 4 | **0** |
| `NN32` | 226 | 23 | 13 | **0** |
| `NN64` | 264 | 15 | 5 | **0** |
| `ZZ` (unbounded) | 365 | 33 | 23 | **1** |
| `QQ` | 168 | 7 | 1 | **0** |
| bits, storage types, `ZZ128`, `RR128`, `ZZ16`, `ZZ8`, `NN16`, `NN8` | **0** | 0 | 0 | 0 |

**So do not argue for it with a file count.** Its value is that it is a
prerequisite for the library bootstrap:
`CompilerLibrary/FortressLibrary.fsi:293` declares
`trait RR64 extends Number comprises { Float, FloatLiteral, RR32, QQ }`, so
`RR64` cannot be registered from the real library without `RR32` and `QQ` as
names.

Per type, what it actually costs:

* **`RR32`** is the cheap one but not free: a `Type` variant, `f32_type()` in
  `basic_type`, an `Elem` variant at 4 bytes — **and new runtime shims**, because
  `to_string`/`println` for f32 do not exist and every one of the four
  non-library files goes through `narrow`, which is not a builtin at all.
* **`NN32`/`NN64`** need no new LLVM type and no storage shim, but they are not a
  variant-plus-arms job either: `udiv`/`urem` rather than `sdiv`/`srem`, unsigned
  compares, unsigned print, and defined overflow. The corpus tests exactly that —
  `library_tests/NN32.fss:6` is `shouldOverflow(f: () -> NN32): Boolean`. One new
  `to_string` per width plus a signedness bit threaded through every arithmetic
  and comparison arm.
* **`ZZ` unbounded** is a different category and must not be scheduled with the
  others: a heap representation (scanned, since it holds a limb pointer), a full
  arithmetic library behind C shims, and a decision about inline small values.
  Its own spike, exactly as `SPIKE-COMPOSITE-TYPE` concluded.
* **Bits and storage types: zero corpus files. Do not build them.**

---

## 4. Dimensions and units — NOT LANDED

`dimensions.tex:22-45` makes `DimType` a production of `TypeRef` composed by
product, quotient and natural power — an abelian group of exponents, which a
`Copy` enum of names cannot hold and interning alone does not solve. The gap
analysis's judgement that the corpus payoff is tiny holds. Nothing here is
blocked on it and nothing else needs it first.

---

## 5. The generator protocol — NOT LANDED, and it is not blocked on closures

This is the item most likely to be mis-scheduled, because the closure
representation looks like its prerequisite. It is not the binding constraint.

Of 238 bare-identifier `for` sources in the corpus, **five** resolve to an
`Array` and **134** to `List`/`Map`/`Set`/`Generator`; all 135 dotted sources are
library methods. `FortressLibrary.fsi:620`'s
`generate[\R\](r: Reduction[\R\], body: E->R): R` needs a name to cross a file
boundary before any of it can be typed. **The wall behind generators is the
import wall**, for the eighth time running on this project.

Array generators landed this round precisely because they need none of that:
one AST node, one checker arm, zero minted traits.

---

## What to do next, in the order the evidence supports

1. **Imports** (`SPIKE-IMPORT-RESOLUTION`), which is Group 2's and is upstream of
   the generator protocol, comprehensions, and most of the numeric tower's real
   value.
2. **Tuples by route (2)**, with M3c's dispatch arity-flattening in the same
   change, and `badarrowtype.fss` moved in the same commit.
3. **The spawn pool refactor (option C)** as its own milestone, gated by a test
   that a task blocking on a task cannot hang the pool.
4. `RR32`, then `NN32`/`NN64`, only once the library bootstrap needs them.
5. `ZZ` and dimensions: separate spikes, no dependants today.
