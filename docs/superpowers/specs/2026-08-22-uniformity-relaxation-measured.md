# The uniformity relaxation, measured — and why it is not a `check_uniformity` change

**Date:** 2026-08-22. **Answers:** 04-state.md next-step **F**, "is the uniformity
rule over-broad?", and the standing instruction to "refine `check_uniformity` so
it accepts SimpleBounds1-5.fss and Compiled14.fss".

**Verdict: the six files are refused by THREE different mechanisms, and only one
of them is `check_uniformity`. Relaxing `check_uniformity` on its own gains
exactly one file, and that file is a must-FAIL.**

Everything below is a compiler run, not a reading.

---

## 1. What a blanket relaxation actually does

Spike: a GROUND member (zero static parameters) no longer constrains the
generic members' shape. Then compile the seven files.

| file | result under the relaxation |
|---|---|
| SimpleBounds1 | still refused — `` `f` is generic; write its static arguments `` |
| SimpleBounds2 | still refused — same, on `Foo` |
| SimpleBounds3 | still refused — same |
| SimpleBounds4 | still refused — same, on `Foo` |
| SimpleBounds5 | still refused — same, on `Foo` |
| Compiled14 | still refused — `declarations of \`f\` differ in their static parameters` |
| **Compiled6.ak** | **COMPILES** |

One file moves and it is `Compiled6.ak.fss`, which is on
`tools/oracle-accepted-must-fail.txt`'s list of programs the legacy REFUSES.
A blanket relaxation is therefore strictly negative: it buys nothing and
accepts a program that must fail.

This reproduces the note already in 04-state.md ("gains three files and one of
them is a must-FAIL") at a finer grain: the gain is ONE, not three, once the
growing-member cut and the getter fix are in.

## 2. SimpleBounds1-5 are not a uniformity problem at all

The real wall is on the CALL, not the declaration:

    SimpleBounds1.fss:32:13: `f` is generic; write its static arguments,
                             as in `f[\ZZ64\]`. They are never inferred

The programs are `println(f(x))` against `f(x: Any)` and
`f[\X extends B\](x: X)`. Selecting between them means INFERRING `X` from the
argument. **Static arguments are WRITTEN, NEVER INFERRED — a decision measured
and declined TWICE**: 2026-08-19 (m3g, 24 files) and 2026-08-21 (20 files, true
ceiling of 4), recorded in `2026-08-21-static-argument-inference-rejected.md`.

So SimpleBounds1-5 are witnesses for static-argument inference, filed under the
wrong rule. They are not reachable by any change to `check_uniformity`, and
"refine `check_uniformity` so it accepts them" cannot be executed as written.

## 3. Compiled14 needs a second thing, and the second thing is a real defect

`Compiled14.fss` is two GENERIC declarations, so §1's relaxation never applies
to it. They differ in BOUND COUNT:

    f[\T\]():ZZ32 = 2
    f[\T extends ZZ32\](x: T):ZZ32 = f[\T\]() + x

Its own source says `THIS WORKS` and its `.test` says `run_out_equals=pass\n`.
The two are distinguished by VALUE ARITY — 0 and 1 — and each carries its own
bound.

Spike: drop `a.bounds.len() == b.bounds.len()` from the shape comparison,
keeping count and kind. The file gets further and then fails on:

    Compiled14.fss:24:7: String does not satisfy `T extends ZZ32`
       |
    24 |   c = f[\String\]()     (*) 2

`f[\String\]()` has ZERO value arguments, so it selects `f[\T\]()`, whose `T` is
unbounded. **It is being charged the OTHER declaration's bound.** `expand_types`
calls `record_bounds(params, &subst, job.span, None)` once per member of the
overload set at the same static arguments, and `None` makes every one a HARD
obligation.

A `speculative` channel already exists for exactly this shape — `BoundObligation
::speculative` is `(owner type, mangled method name)` and the checker PRUNES the
stamp instead of refusing the component — but it is wired for METHOD stamps
only. Functions pass `None`.

**So accepting Compiled14 is: (a) bound count out of the shape comparison, and
(b) a per-member speculative bound obligation for function overload sets, so a
member whose bound fails at these arguments is pruned rather than fatal.**
That is a milestone with a defect fix inside it, not a refinement.

## 4. What WAS done

The separable half of the instruction landed on its own, because it is free:
`check_uniformity` walked `Decl::Function` alone, so a TRAIT or OBJECT overload
set was checked by nothing — `trait Holder[\T\]` beside `trait Holder` compiled
to exit 0. Extending it to every declaration costs **zero**, measured over all
1956 corpus files: 397 compiling either way, 0 gained, 0 lost, 0 of the 397 IR
bodies changed by a byte. Fixtures `traituniformity.fss` and
`objectuniformity.fss` carry the rule, because no corpus file does.

## 5. Recommendation

1. **Do not relax `check_uniformity` for ground-vs-generic.** It gains one
   must-FAIL and nothing else. DEV-6's "ENFORCED AND PERMANENT" is over-broad
   against the oracle, and that remains true — but the breadth is not what is
   costing these six files.
2. **Compiled14 is the tractable one.** Its blocker is a real, live defect —
   a bound obligation attributed to the wrong member of an overload set — and
   that defect is worth fixing whether or not the uniformity rule ever moves.
3. **SimpleBounds1-5 go back in the static-argument-inference pile.** Re-filing
   them there is the accurate bookkeeping; they have been counted against
   uniformity and they were never its cost.
