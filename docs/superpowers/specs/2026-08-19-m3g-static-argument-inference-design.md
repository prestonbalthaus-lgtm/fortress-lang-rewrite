# Fortress M3g: static-argument inference, measured and declined

Date: 2026-08-19
Status: **not built, deliberately.** The measurement that was supposed to scope
it falsified it instead. What landed under this milestone is the pair of
structural defects the measurement uncovered in the M3d expander.
Commit: `2fc7af812` on `m3-unified-sprint`.

M3g was specified as the structural milestone: make type inference drive
instantiation demand, and redesign the compiler loop around the cyclic
dependency that creates. The stated reason was that stacking more syntax on
M3d's expansion-then-check phase split would make the fixpoint harder to
untangle later.

Phase 1 measured the thing before building it, which is this project's rule.
Both halves of the premise turned out to be false, and the second one is the
interesting one.

## The measurement

At the M3f baseline — 476 of 1780 lexing files parse, 187 of 1956 compile end to
end — 24 files' first diagnostic is
`` `X` is generic; write its static arguments … They are never inferred``.
That count is the whole apparent case for the milestone.

The experiment: for each of those 24 files, **hand-write the correct static
arguments at every call site that lacks them** and re-run the real driver. That
simulates a perfect inference engine exactly, at no implementation cost, and the
number it produces is a tight upper bound on what the milestone can buy.

**The ceiling is 0 of 24.** Not one file reaches exit 0.

Where they go instead:

| next blocker after perfect inference | files |
|---|---|
| dotted methods | 13 |
| generic overload set emitted as a duplicate | 4 |
| arrow types / first-class functions | 4 |
| missing prelude types (`Object`, `Integral`) | 2 |
| refused correctly — it is a negative test | 1 |

Three of the 24 are not compilable by anything: `Compiled5.af.fss` is a legacy
negative test whose `.test` file asserts the error verbatim, `GenericOverload4.fss`
carries `(* Should not compile *)` in its own header, and
`genericFunctionalMethods.fss` calls a function that is commented out in its own
source.

### 9 of the 24 were never inference problems

`mono.rs` handled `Expr::Instantiate` by destructuring its callee to `Expr::Var`
and, on anything else, returning `StaticArgumentsRequired { name: "<expression>" }`
as a catch-all. A generic *dotted* call — `o.m[\String\]()` — parses as
`Instantiate` over `Field`, so it tripped that branch **while its static
arguments were written**. Nine files were mislabelled by 37%-contaminating the
one bucket the milestone was being scoped from.

Expansion runs before `Checker::new`, so this pass reports the site or nothing
does. It now names dotted method dispatch. The bucket is 15, not 24, and
dotted-method first-blockers went 35 → 44 — making dotted methods the largest
checker-stage blocker in the tree.

## Why a scoped engine is worse than none

The easy tier — unify one static parameter against a syntactically apparent
argument type, single candidate — covers the full inference demand of 4 files
and part of 7 more. It unlocks **zero**, and on two files it is actively wrong:

In `OverloadConstructor2.fss`, `Thing()` names an overload set holding a ground
zero-argument function *and* a generic constructor. Writing a static argument
there retargets the call to the constructor and fails on arity. **There is no
static argument that is a correct answer.** Demand has to be decided *after*
overload resolution, not before it.

That is not an engineering gap. It is the point where 1.0 stops defining the
answer:

* `GenericOverload4.fss` admits two solutions that both typecheck, `f[\A,b\]`
  and `f[\A,B\]`, and 1.0 states no most-specific rule to choose between them.
* `Compiled180.fss` and `Compiled6.ab.fss` have **phantom** static parameters —
  `T` occurs nowhere in the signature — so `T` is genuinely underdetermined for
  the candidate that wins, and "any solution" is not a rule.
* `GenFun6.fss` needs an unsatisfied bound to silently *drop* a candidate.
  Today bound discharge is a hard error that fires after expansion has already
  registered the instantiation, so it can never act as a filter.

Building to a rule the specification does not state means inventing language
semantics to move a metric by zero files. Declined.

## The premise that the phase split is fragile

It is not, and the reason is worth writing down because it is the structural
answer the milestone was actually asking for.

**Inference does not have to be interleaved with checking.** It belongs in a new
pass — *elaboration* — between parse and expand, which solves static arguments
and **writes them into the AST**. After it runs, demand is syntactic again, and
`expand` and `Checker` are unchanged. The phase split is preserved, not undone.

Two properties make that work, and both hold today:

1. **Elaboration needs signatures, never bodies.** So per-declaration
   elaboration is independent and there is no fixpoint to solve.
2. **It solves symbolically, in the scope of the enclosing declaration's static
   parameters.** Inside `f[\T\](x:T)`, a call `g(x)` elaborates to `g[\T\]` —
   writing `T`, not a ground type. Expansion substitutes later, as it already
   does.

Polymorphic recursion stays refused unchanged: it is bounded by
`MAX_INSTANTIATIONS` in the expander's worklist, and elaboration runs before
that worklist exists.

So the cost of adding syntax first is **zero**. Whenever inference is wanted, it
lands in front of the existing pipeline rather than through it. `mono.rs`'s
header and `02-stack.md`'s "static arguments are WRITTEN, never inferred" both
remain true and are left alone.

## What did land

Two defects in shipped M3d code, both found by the probe rather than by review.

### The expander dropped members of a generic overload set

`generics: BTreeMap<String, &Decl>`, filled with `insert(name, decl)` — one
template per name, **last wins**. Every member of a generic overload set but one
was silently discarded, and the survivor was the wrong overload.

The emission loop compounded it: it matched instances by name alone, so with two
source declarations named `f` it pushed each instance twice. The checker's
duplicate test (`same name and same parameter types`) then fired, and the user
saw `` `f$A$e` is defined twice`` — a confusing diagnostic on a valid program.

The duplicate test firing is what kept this from being a miscompile. It is not a
defence: it fires *because* the two emitted bodies are identical clones of the
wrong template. Remove one accident and the other becomes wrong output.

Fixed by keying instances on `(mangled name, member)` and giving each source
declaration its own instances at emission. Each member substitutes under its own
static-parameter names, because `check_uniformity` compares parameter *counts*,
not spellings — `f[\T\]` and `f[\S\]` are a legal set.

`fortressc/tests/genericoverload.fss` is the fixture: a two-member set at two
static arguments must print `1 2 1` and emit four definitions.

### The catch-all diagnostic

Described above. `Expr::Field` callees now report `DottedMethodUnsupported`.

## Gate

`tools/generics-gate.sh` 20/0 → **23/0**. Two new assertions: the program's
output, and the count of emitted definitions.

Two new mutations, both **shown to refuse** before the green result was reported:

| mutation | result |
|---|---|
| emit every instance once per source declaration of its name | REFUSED, 2 checks |
| instantiate only the first member of an overload set | REFUSED, 2 checks |

Full run: 5 mutations, **0 survived, 0 could not be applied.**

## What this does not buy

**Zero corpus files.** The files that die on `defined twice` only reach it after
hand-annotation; unannotated they still stop at unwritten static arguments.
The evidence for this change is fixtures, not the metric — the same standing
M3f gave `f ()`: making valid Fortress compile is not the same thing as moving a
number.

## Named and walked away from

**Dotted methods is the next milestone.** 44 checker-stage first-blockers and 13
of the 24 probe files, now that the bucket is labelled honestly. It is three
pieces, not one: the checker refuses `Expr::Field` callees outright; `mono` must
route `Instantiate`-over-`Field` to a method; and `mono` does not walk method
bodies at all, so a generic call inside one creates no demand. It also has to
enter M3c's symmetric whole-program dispatch matrix. That is M3i.
