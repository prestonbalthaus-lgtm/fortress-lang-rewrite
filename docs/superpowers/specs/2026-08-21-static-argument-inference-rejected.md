# SPIKE-STATIC-ARGUMENT-INFERENCE — REJECTED

**Decision: static-argument inference is CUT. `[\...\]` stays written, never
inferred. The M3d locked constraint holds, and this document is the evidence
that taking it was right rather than merely convenient.**

Status: **decided.** Measured on master `abbbdc7a3` (the step-1 merge), 285
corpus files compiling, with a sha256-pinned driver.

**AND THIS IS THE SECOND TIME, WITH THE SAME METHOD AND THE SAME ANSWER.**
`docs/superpowers/specs/2026-08-19-m3g-static-argument-inference-design.md` is
titled *"static-argument inference, measured and declined"* and its status line
reads *"not built, deliberately. The measurement that was supposed to scope it
falsified it instead."* It ran the identical experiment on 2026-08-19 at the M3f
baseline — 24 files first-blocked, static arguments hand-written into each — and
declined. This document re-ran it at a baseline 98 files later (187 → 285
compiling) without knowledge of that result, and reached the same conclusion from
a different corpus state. **Two independent measurements, two baselines, one
answer.** The re-run was still worth doing: the *reasons* are sharper now
(§2 (b) and (c) are new), and a decision that has been confirmed twice on moving
ground is a different thing from one taken once.

The spike was authorised with an explicit escape: *"If it relies heavily on deep
inference or worsens diagnostics for little gain, formally reject the spike and
document the rejection."* Both halves of that condition are met, and the
measurements are worse for the feature than the guess was.

---

## 1. The prize is 4 files, not 21

The bucket is **20 files** on this tree, not the 21 the gap analysis recorded.
The 21st, `ProjectFortress/tests/BitTwiddle.fss`, moved *upstream* to
`unknown type 'Integral'` when `17c315177` made a generic declaration header
resolve its type names. No file left the bucket for a reason relevant to
inference.

That is the count of files **first-blocked** on `X is generic; write its static
arguments`. It is not the count of files a working inference engine would
unlock, and the difference is the whole decision.

**The static arguments were written in by hand — the exact answers a perfect,
fully general inference engine would produce — and each file recompiled. Five of
the twenty reach exit 0, and one of those five needed a type-annotation edit
rather than a call-site one.** The rest hit a *second, unrelated* wall:

| what stops it after the static arguments are supplied | files |
|---|---|
| `an arrow type is not implemented in this subset` | `HiLoInference`, `InferTest`, `VoidArrowTest3`, `contraUnification` |
| `unknown type 'Object'` / `'Any'` | `GenFun7`, `ArrowRTTI3` |
| `NOT takes Boolean operands; this one is Foo` | `EqualityTest1`, `EqualityTest5` |
| `unknown name 'builtinPrimitive'` | `nativeTestFn` |
| still `Thing is generic` — a *different* defect (§5) | `OverloadConstructor2`, `OverloadConstructor3` |
| correctly refused: `A does not satisfy 'T extends B'` | `Compiled5.af` (a must-fail file) |
| `om$String$e is defined twice` | `GenFun6` |
| condition/type errors of its own | `OprDecl.Infix`, `GenericOverload4` |

**True ceiling: 4 files** — `Gt0h`, `commonSuper`, `Compiled180`,
`Compiled6.ah`. And counting `Compiled180` is generous: its own source asks
*"Generic overloaded functions at top level... Can this actually work?"* and two
of its three overloads print `"FAIL WITH A"` / `"FAIL WITH P"`.
`GenericOverload4.fss:15` says `(* Should not compile *)` in its own source.
Three of the twenty live under `not_passing_yet/`.

## 2. The candidate syntactic rule is unsound, four separate ways

The rule assessed was: *one static parameter `T`, one value parameter written
exactly `T`, a literal argument → instantiate at the literal's type.* Each
refutation below was reproduced, not reasoned.

**(a) For integer literals it is circular, and breaking the cycle is wrong.**
`int_literal` (`crates/types/src/lib.rs:2307-2318`) gives a literal no type of
its own: it is ZZ32, ZZ64 or RR64 depending on `expected`, and `expected` comes
from the callee's parameter type — which, for a generic callee, **is `T`**. The
rule breaks the cycle by taking the `expected: None` default, ZZ32. Measured,
the same literal legitimately wants three different answers:

    sink(v:ZZ64);   sink(id[\ZZ64\](3))   -> exit 0
    sinkf(v:RR64);  sinkf(id[\RR64\](3))  -> exit 0
                    sink(id[\ZZ32\](3))   -> a ZZ32 value is not implicitly
                                             converted to ZZ64; write `widen(...)`

There is no implicit widening to paper over a wrong choice. **This is the trap
the spike was flagged for, and it is real.**

**(b) Instantiating at the argument's own type is wrong under an F-bound.**

    trait Eq[\S\] end
    trait Foo extends Eq[\Foo\] end
    object Baz extends Foo end
    h[\T extends Eq[\T\]\](a:T):() = println("h")

`h[\Baz\](Baz)` is refused — `Baz does not satisfy 'T extends Eq$Baz$e'` —
while `h[\Foo\](Baz)` compiles. The argument's type is `Baz`; the only sound
instantiation is a **proper supertype**. A rule that reads the argument cannot
find it. `EqualityTest5.fss` is the corpus case and behaves identically.

**(c) It converts an honest diagnostic into one that reads as a compiler bug.**
Uniformity is enforced, so a generic name is usually a *set*. `GenFun6.fss`
declares `om[\T extends Object\](x:T)` and `om[\T extends String\](x:T)`.
Applying the rule to `om("cat")` instantiates **both** at `String`; both mangle
to one symbol:

    GenFun6.fss:14:1: `om$String$e` is defined twice

`write its static arguments` is a true statement about an unimplemented feature.
`defined twice` is a lie about the user's program.

**(d) Three of its own targets have no `Expr::Call` to hook.** In
`EqualityTest1`, `EqualityTest5` and `OprDecl.Infix` the generic name arrives as
a bare `Expr::Var` inside a juxtaposition or as an infix application. Under
either reading of "exactly one value parameter written `T`" the rule fires on
**4 or 5 sites**, and clears — after the second walls above — approximately
**one file**.

Narrowing to **string and boolean literals only** removes defect (a) entirely,
since `String` and `Boolean` are unconditional. It does not touch (b), (c) or
(d), and it drops the count further.

## 3. The phase split was never the obstacle — that part of the guess was wrong

It is worth recording, because it is the reason the spike looked plausible:
**inferring from a literal does not break M3d's phase ordering.** It needs the
literal's *token class*, not its type, so demand stays syntactic and expansion
still runs before `Checker::new`. The constraint at
`error.rs::StaticArgumentsRequired` — *"written, never inferred: that is what
makes instantiation demand syntactic"* — would have survived a literal rule
intact.

The spike dies on **soundness and value**, not on architecture. That distinction
matters for anyone who revisits it: widening the *syntactic* class of demand is
allowed; what is not allowed is guessing a type.

## 4. And real inference is a re-architecture, not a reordering

For completeness, since the fallback would be "type first, then expand". The
codebase already has the *speculative pass* precedent —
`resolve_inferred_returns` plus `discard_speculative_walk`, which handles the
`dispatch_target` memoisation hazard. That is solved. What is not:

1. **`Checker::new` hands out type tags by position** (`FIRST_TAG +
   registry.concrete.len()`). An instantiation discovered after typing makes a
   program's tags a function of when inference converged rather than of its
   source text — the thing `registry.rs`'s ordering comment exists to prevent.
2. **Every dispatch table computed before that instantiation has no arm for the
   new tag**, which lands on `fortress_dispatch_failed` at run time on a program
   the checker approved. Clearing the memo does not help; the domain lives in
   the registry.
3. **Signatures are built in `Checker::new` from the whole component**, so a new
   instantiation changes an overload set, which changes `agreed()`, which
   changes the literal hints, which changes the inference — a second cycle
   inside the first.

The existing cross-phase mechanism does not generalise: a `BoundObligation` is a
*predicate about an instantiation that already exists*. An inferred static
argument *determines which instantiation exists at all*. The checker can answer
mono's question; it cannot ask mono to make a stamp it did not make, because by
then the tags are frozen.

Both orderings that do work — a types-lite pre-pass, or one interleaved
expansion/checking fixpoint — are milestones. **Neither is a spike.** And note
this is the *same* decision as the where-clause-variable collision D6 §1 already
settled in the same direction.

## 5. The one thing in this area worth doing, and it is not inference

`OverloadConstructor2.fss` and `OverloadConstructor3.fss` are blocked by a
**name-resolution defect**. `mono.rs:691` asks `self.generics.contains_key(name)`
and stops; it never checks whether a *ground* declaration of the same name
matches the call's arity. So a generic `object Thing[\T\](x:T)` plus a ground
`Thing():Thing[\ZZ32\]` makes `Thing()` report *"`Thing` is generic; write its
static arguments"*. `check_uniformity` cannot catch the mixed set either — it
iterates `Decl::Function` only, so an object and a function sharing a name are
never compared.

Independent of this decision, does not touch the phase split, clears 2 of the 20
from the bucket. **Queued, not done here** — neither file compiles afterwards
for other reasons, so it is a diagnostic-accuracy fix rather than a corpus one.

---

## What this forecloses, stated plainly

Programs must write `f[\ZZ64\](3)`. That is a real ergonomic cost and 1.0 does
not ask for it. It is a **named deviation**, not an oversight, and it re-opens
only with one of the two re-architectures in §4 — at which point the prize
should be re-measured, because on today's compiler it is four files and half of
those are tests that say in their own source that they should not compile.
