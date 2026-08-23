# Exceptions: the design, measured before it was drawn

**Date:** 2026-08-23. **Status: DESIGN, nothing built yet.** Phase 2 of the
continuous-execution pipeline.

**The call, from Preston:** no LLVM `invoke`/`landingpad`. Lower
`throw`/`try`/`catch`/`finally` into a hidden tagged union with explicit
branching, predictable control flow, no allocation on the hot path.

Everything below is measured against the corpus at `d548f487b` before any of it
is drawn, because the census changes the plan.

---

## The census, and it splits the milestone in two

39 corpus files are first-blocked on the exception family. Splitting them by
whether the FILE writes anything but `throw`:

```
THROW-ONLY, needs NO catch machinery     23 files, 19 of them real targets
NEEDS try / catch / finally              16 files, 16 of them real targets
```

**Nineteen files need only `throw`, and an uncaught `throw` is a halt.** They
have no `catch` anywhere, so every throw in them terminates the program. That is
not a subset of the tagged-union design; it is a different, much smaller piece
of work that needs no control-flow change at all.

And the sixteen that do need `catch` include the one that matters:
**`Library/FortressLibrary.fss`** — the standard library's implementation file,
which is where the bootstrap stops today.

## Three facts that shape the lowering

**1. `throw` almost always throws a SINGLETON.** The corpus's top throw sites:

```
51  throw BrokenInvariant          9  throw MatchFailure
33  throw CompilerFailureDetectedAtRunTime   8  throw IntegerDomainError
32  throw NotFound                 7  throw EmptyReduction
```

A handful construct — `throw TestFailCalled(s)`,
`throw KeyOverlap[\Key,Val\](pk,pv,cv)` — but the common case allocates nothing
because a singleton is already a constructed global.

**2. NO `.test` FILE RECORDS AN EXPECTED EXCEPTION.** Zero, across all 373. So
the oracle has nothing for a throw to disagree with, and stage 1 cannot regress
the must-fail ratchet by choosing a halt. `shouldRaise[\Ex\](expr: ()->())` is
declared at `Library/FortressLibrary.fsi:278` and is used in exactly TWO corpus
files -- it is stage 2's verifier, not a blocker.

**3. AN EXCEPTION IS AN OBJECT, AND AN OBJECT ALREADY CARRIES ITS TAG.** Every
object in this backend is one heap block with a 32-bit concrete type tag at
offset 0, and trait membership is a compile-time fact about a tag. So the
"tagged union" the call asks for does not need a new tag: `catch e: NotFound` is
a tag test, over the same closed-world set symmetric dispatch already
enumerates. The union is `{ i1 threw, ptr payload }` and the discriminator is
already inside the payload.

---

## Stage 1 -- `throw` is a halt. No unwinding, no union, no ABI change.

`throw E` lowers to a call to a runtime shim beside `fortress_halt`: print
`fortress: uncaught exception E` and `_exit(1)`. Nothing else in the language
moves. The non-throwing path costs zero instructions, which is the whole point
of the no-`invoke` call.

Correct for all nineteen, because none of them has a `catch`: an uncaught throw
IS a halt, and this is the halt.

**Gate:** a fixture that throws and exits 1 with the exception named, plus the
existing halt machinery's self-tests. Expect +19 or fewer -- the number is a
ceiling and every one of the nineteen gets re-run, not assumed.

## Stage 2 -- the Result lowering, and the three things it has to answer

**Which functions get the extra return.** A function is THROWING if its body can
reach a `throw` or call a throwing function. That is a fixpoint over the call
graph, and it is tractable here for the same reason dispatch is: this is a
whole-program compiler with a closed world. A non-throwing function keeps its
signature byte for byte, which is what keeps the IR of the 395 files that
compile today unchanged.

**What the return looks like.** `{ i1, ptr }` beside the real result, or a
sret-style out-parameter -- to be decided against what SROA actually does to it,
measured on the emitted IR rather than argued. The `ptr` is the exception object
and its tag is the discriminator. `catch e: T` is a tag test; `finally` is the
block emitted on both edges.

**What a throw inside an OUTLINED body does, and this is the real constraint.**
Every `for` body in this compiler is outlined into a function the parallel
runner calls; that is already why `exit` cannot leave a `for` body. A throw
inside one has to reach the runner, and the runner has to stop the other
iterations and re-throw at the loop. MEASURED: six throw sites inside a `for`
region, across five files. **Stage 2 should REFUSE a throw inside a parallel
body by name** and revisit it with `at`/distributions, rather than build
cross-iteration propagation for five files.

## What is NOT decided and needs Preston

Whether stage 2 is worth its complexity for sixteen files. It is -- but only
because one of them is `Library/FortressLibrary.fss`. If that file turns out to
stop on something else immediately behind `throw`, the honest answer changes,
so **the first thing stage 2 does is neutralise the exception sites in that one
file and see how far it walks.** That spike costs minutes and decides the
milestone.
