# D5–D8: independent validation, and three corrections to the record

D5, D6, D7 and D8 were drafted by the instruments lane while the semantics lane
was gathering evidence for the same four questions from primary sources, without
having read the drafts. This is what the second pass found.

**Headline: all four decisions survive. None is contradicted. Three are extended
with measurements that were not in them, and one of those extensions splits a
line item the ROADMAP bundles.** Three claims in the *recorded project
documents* are wrong and are corrected below.

Measured on master `abbbdc7a3`, 285 corpus files compiling (293 after the
Object/Any seeding in `d0e264c81`).

---

## D5 — component algebra CUT: **CONFIRMED**

The draft decides the compilation unit is the whole program. Independently
checked by asking the inverse question — what breaks if v1 *promised* separate
compilation — against the five invariants the compiler already relies on:

| invariant | whole-program | separate compilation |
|---|---|---|
| (a) exactly-one-most-specific-winner exhaustiveness | survives | **breaks**, and changes the legal program set |
| (b) closed-world `excludes` | survives | **breaks** |
| (c) monomorphization to a fixpoint + per-component cap | survives | **breaks** — fixpoint, and the cap becomes per-object |
| (d) declaration-order type tags | survives | **breaks** — both units start at `FIRST_TAG` |
| (e) the emitted dispatch tables | survives | **breaks** — `fortress_dispatch_failed` becomes reachable |

Five for five. This is not a decision with a cost on both sides: M3c and M3d are
already correct under the drafted answer and incorrect under the other, so the
draft records a state that already exists rather than choosing a new one. **Take
it as written.**

## D6 — the phase-4 split: **CONFIRMED**, and the no-HM correction is bigger than stated

§1 (the where-clause decision) is already FINALIZED and the semantics lane has
implemented against it — `7ed9e4230` parses a `where` clause for real and
implements exactly the `Id extends Type` form, refusing the other twelve by name.
That is §1 executed, not merely agreed.

The **no-HM-engine correction is right and understates itself in two ways**:

1. **The false claim is in two places, not one.** `~/claude/02-stack.md`
   `<toolchain_and_frontend>` says *"Hindley-Milner type inference engine with
   algebraic data type resolution"*, and **`ROADMAP.md:141` says the same thing
   again** — *"4. Types. Hindley-Milner inference with traits, polymorphism and
   overload resolution."* Correcting one leaves the other standing.
2. **The second half of the claim is false too.** "Algebraic data type
   resolution" is absent as well: there are no sum types and no ADT machinery of
   any kind.

The mechanical evidence: `unify`, `occurs_check`, `TypeVar`, `fresh_var` and
`Substitution` return **zero hits across every file in `fortressc/crates`**. And
the representation forecloses it — `types.rs:86-102` is a nine-variant `Copy`
enum **with no variable case**, so there is nowhere for an inference variable to
live.

*The trap for anyone re-running that grep:* `Subst` **does** exist,
`mono.rs:36`, `type Subst = BTreeMap<String, TypeRef>`. It is not an HM
substitution — it is monomorphization's map from a **written** static parameter
to a **written** static argument, applied before the checker exists.

What actually exists is two mechanisms of two different classes: bidirectional
checking (`expected: Option<Type>`, 33 sites, which *pins literals and asserts
subtyping and never converts anything*) and `resolve_inferred_returns`, a
declaration-order fixpoint over omitted return types that is explicitly
non-principal — a self-recursive inferred function keeps its `Void` placeholder
where HM would introduce a variable.

**Consequence for the split: phase 4 is a BUILD, not an extension.** Both
recorded documents describe extending an engine that does not exist.

## D7 — `nat` in v1, restricted: **CONFIRMED and EXTENDED**

The draft's decision — `nat`/`int`/`bool` in v1 restricted to statically known
arguments, `unit`/`dim` to a later sub-phase, `opr` scoped separately — is
supported by measurement it did not have. Two additions.

**(1) The six kinds are not one feature, and the single parser guard hides
that.** `static_param` refuses all six in one `matches!`. Measured separately:

| kind | files containing | declaration sites | files FIRST-blocked |
|---|---|---|---|
| `nat` | 61 | 842 | **39** |
| `opr` | 21 | 96 | **18** |
| `bool` | 13 | 39 | **4** |
| `int` | 10 | 14 | **4** |
| `unit` | 6 | 6 | **0** |
| `dim` | 0 | 0 | **0** |

`dim` has **no corpus witness at all**. `unit` never first-blocks. So the draft's
instinct to scope them apart is right, and the cost of the guard is not one
number.

**(2) The ROADMAP bundles two line items that have opposite demand, and this is
the finding.** Decision 4 asks for *"constraint solving for `nat` parameters"*.

> **Not one `where { k < n }` exists in 1956 files. Zero `nat`/`int` arithmetic
> constraints in the entire corpus.**

Meanwhile `nat` *parameters* have 61 files and 842 sites. And of the 1,642
argument positions where something sits in a `nat` slot: **81.1% are a bare name,
18.4% a decimal literal, and 0.5% a static expression.**

So the restricted form the draft proposes covers **99.5% of sites**, and the
constraint solver — the expensive half — has **zero** corpus demand. Those are
separable line items and the ROADMAP should stop bundling them.

**And the load-bearing file is blocked *behind* `nat`, not *on* it.**
`Library/FortressLibrary.fsi` carries 145 `nat` sites but dies first at
`234:34 expected ')', found Dot`. `nat` does not appear in the api first-blocker
histogram at the file that matters — and `FortressLibrary.fsi:1408` is
`trait ImmutableArray1[\T, nat b0, nat s0\]`, which is to say **`nat` is how the
standard library declares its array types**. It becomes an unavoidable wall the
moment three parse blockers clear.

## D8 — distributions CUT: **CONFIRMED**

Independently reproduced: `at` is the first blocker of **3 of 1956** files, and
the compiler has **no notion of locality anywhere**. The runtime is one process
with a thread pool plus an MPI boundary whose exposed surface is a handful of
builtins. A v1 distribution has no acceptance criterion that could be
differential-tested, which is the draft's own argument and it holds.

---

## The uniformity 22 — answered: **22 of 22 are genuine, zero artefacts**

Task: *"open the 22 cases and count the genuine violations."* Done, by opening
each and quoting the conflicting declarations.

**All are genuine 1.0 violations. Zero are artefacts of the coarse check.**
(20 remain first-blocked on uniformity on this tip; the other 2 are still genuine
violations, merely blocked earlier by the header-name fix.)

`check_uniformity` *is* coarser than alpha-equivalence — it compares
`static_params.len()` and each parameter's `bounds.len()`, and iterates
`Decl::Function` only. The artefact class is real and reproducible in a
synthetic file. **No corpus file exercises it**: nothing writes a bare `[\T\]`
against an explicit `[\T extends Object\]` for the same name.

The coarseness bites in the **other** direction, and there is an in-corpus
witness: `GenFun6.fss:13-14` declares `om[\T extends Object\](x:T)` and
`om[\T extends String\](x:T)` — equal counts, equal bounds-counts, **different
bound types**, so a genuine violation that `check_uniformity` **accepts**. A
false *accept*, not a false refuse.

**`QuickSort.fsi` is a genuine violation.** Its five `quicksort` declarations all
have one static parameter, but four write bare `[\T\]` (implicitly
`extends Object`) while one writes `[\T extends StandardTotalOrder[\T\]\]`.
Different bounds, so `overloading.tex` forbids the set. The consequence stands as
the gap analysis stated it: *"type checks `Library/`" cannot be satisfied while
enforcing a rule 1.0 states and 1.0's own shipped library breaks.* That needs a
signed-off deviation, not a fix.

*Caveat, stated because it was not closed:* the legacy implementation's own
behaviour here is established from `OverloadingChecker.scala:65-66` ("not (fully)
checked yet") plus two behavioural artefacts, not from a trace of the whole
pipeline. Running the legacy `fortress compile` over the 20 would settle it.

---

## The Object/Any seed's retirement trigger is NOT yet met — measured, not assumed

`d0e264c81` seeds `Object`/`Any` in `Checker::new` and says the seed dies when
import resolution can supply them from `LibraryBuiltin/AnyType.fss` and
`CompilerBuiltin.fsi`. Group 2 has since built api-first import resolution
(`crates/driver/src/resolve.rs`) and **turned it ON BY DEFAULT** — worth knowing,
because `04-state.md` still records it as opt-in and off, which was true of an
earlier tip and is not true of `e4dc5406b`.

It does **not** retire the seed. Measured with Group 2's own binary: a program
writing `trait Shape extends Object end` still answers `unknown type \`Object\``
there, and `Compiled9.DiamondOverriding.fss` and `tests/extendObject.fss` are
still refused on that branch while they compile here. **The two changes are
complementary, not competing**, and both are needed at merge. Re-test the trigger
when `CompilerBuiltin.fsi` itself parses; it does not today.

## Three corrections to the recorded documents

1. **`02-stack.md` and `ROADMAP.md:141`: there is no Hindley-Milner engine, and
   no ADT resolution either.** Both files state it. Correcting one is not enough.
2. **`02-stack.md` and the gap analysis: the spec DOES pin integer overflow.**
   `opr-overview.tex:195-200`. Already recorded in
   `2026-08-21-arithmetic-failure-decisions.md`; repeated here because the false
   claim is in a file people read first.
3. **The recorded `+7` for Object/Any was a `--real` number and nobody said so.**
   Re-taken today: **+8** on the full corpus (285 → 293) and **+7** on
   `triage --real` (227 → 234). The two figures were never in conflict; the
   denominator was just missing.
