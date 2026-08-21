# D8. Distributions, regions and `at` — scope

**Decision: DISTRIBUTIONS ARE CUT from v1, as a named deviation from decision 4.
The `Region` model is already met by the library and needs no compiler work.
`at` is separable, cheap, and rides along with `SPIKE-CONTROL-FLOW-EXTRAS` —
it is not a v1 exit item on its own.**

Status: **drafted, not adopted.** Written against master `f81f41ace` on
2026-08-21; every measurement reproduced by hand.

---

## 1. The problem: a v1 item with no acceptance criterion

Decision 4 says v1 is the complete 1.0 specification minus syntax abstraction,
and names **distributions** explicitly among the sixteen in-scope features.

`Specification/advanced/parallelism-locality/` is 1,132 lines across 10 files,
of which `distributions.tex` (127), `primitives-distributions.tex` (48) and
`arrays-distributed.tex` (77) are the distribution material.

**`distributions.tex:15-16` carries the specification's own note:**

> *"Distributions are not yet supported. Examples in this section are not
> tested."*

So 1.0 itself never shipped this, and it says so at the top of the chapter.

**And the corpus has nothing.** Measured over all 1956 files:

| name | files |
|---|---|
| `Distribution` | **0** |
| `blocked` | **0** |
| `subdivided` | **0** |
| `distributeAcross` | **0** |

Every other decision-4 item has corpus witnesses to differential-test against.
This one has none, the `.test` oracle has no case for it, and the reference
implementation never ran it. **"Done" would be unfalsifiable.**

---

## 2. The decision

### 2.1 Distributions are CUT from v1

`Distribution`, `distribute`, `distributeFromTo`, the distribution-aware array
allocation of `arrays-distributed.tex`, and the primitive distributions of
`primitives-distributions.tex` are **out of v1**. Recorded as a named deviation
from decision 4, of the same kind as M3c's two, and added to the ROADMAP's
"Out of scope for v1" section.

Rationale:

1. **No acceptance criterion is constructible.** No corpus witness, no oracle
   case, no reference behaviour — the spec disclaims its own examples.
2. **It is untestable on the current hardware posture.** `02-stack.md` records
   that Slurm, `sbatch`, multi-node and the physical fabric are **SHELVED until
   the language is complete**. A distribution over one node is an identity
   function; a gate over it would assert nothing. Building it now means writing
   the one feature whose correctness cannot be observed, on the one axis that has
   been deliberately switched off.
3. **It is the only decision-4 item that is a *performance mapping*, not a
   language semantics.** Cutting it removes no program's meaning; it removes a
   locality hint. Every affected program still runs, on the Global region.

**The trigger that re-opens it is explicit and it is not "v1 is otherwise
done":** when the cluster comes back off the shelf — real Slurm, real
InfiniBand, more than one node — distributions become both testable and
motivated, and they get their own phase with an exit criterion measured on that
hardware. Until then the honest status is *deferred with a stated trigger*, not
*pending*.

### 2.2 The `Region` model needs no compiler work — it is already met

This is the part that looks like scope and is not.

`regions-threads.tex:25` carries a footnote that licenses the whole shortcut:

> *"Note: the initial implementation of the Fortress language assumes a single
> machine with shared memory and exposes only the `Global` region."*

And the library already ships exactly that, as ordinary Fortress source:

```
Library/FortressLibrary.fss:75   trait Region extends Equality[\Region\]
                          :76       isLocalTo(r: Region): Boolean = false
                          :80   object Global extends Region
                          :82       isLocalTo(_: Region): Boolean = true
                          :85   region(a:Any): Region = Global
                          :90   here(): Region = Global
```

with the matching signatures at `FortressLibrary.fsi:55-63`. `region()` and
`here()` are **constant functions returning `Global`**. There is no compiler
feature here at all — the region hierarchy is a library data type with one
inhabitant, and it will compile the day `FortressLibrary` compiles.

**So "regions" should be struck from any remaining v1 work list.** It is not
deferred and not cut; it is *already satisfied by the library under the spec's
own single-machine licence*, and its only dependency is the phase-3 bootstrap.

### 2.3 `at` is separable and is NOT a v1 exit item on its own

`at` is a reserved word refused at the parser. Under the single-region model it
has a trivial lowering: `at r do e end` evaluates `e`, because `Global` is the
only region and every object is local to it (`isLocalTo(_) = true`).

**But it has no positive corpus witness.** `at` is the first blocker of exactly
**3 of 1956** files, and all three are
`ProjectFortress/parser_tests/XXXPreparser.a{i,j,k}.fss` — **must-fail negative
tests in a directory `triage --real` drops**. Inside `--real`, `at` first-blocks
**zero** files. By comparison, its neighbours in the same chapter family do have
witnesses: `spawn` 9 first-blockers, `also` 6.

**Recommendation: fold `at` into `SPIKE-CONTROL-FLOW-EXTRAS` and let that spike
decide.** If parsing it and lowering it to its body is genuinely a few lines
alongside `case`/`typecase`/`label`/`exit`, take it — it closes a reserved word
and costs nothing. If it is not free, drop it; nothing is waiting.

What it must **not** become is a standalone work item justified by decision 4's
list, because the thing decision 4's list is pointing at — locality — is the
part that is cut in §2.1.

---

## 3. What this document does NOT decide

- **`spawn`.** 9 first-blockers, a real feature, and gated on
  `SPIKE-CLOSURE-REPRESENTATION` (`spawn.tex` desugars to
  `Thread[\Any\](fn()=>e)`). It is in v1 and it is not a locality item.
- **`also do`.** 6 first-blockers, in v1, and independent of this.
- **The generator/`Reduction` protocol** of `defining-generators.tex` (489 lines,
  the largest file in the chapter). In v1, gated on closures, and orthogonal to
  distributions — a generator that does not distribute is still a generator.
- **`shared`/`local`** (`shared-local.tex`) and early termination
  (`early-termination.tex`). Not examined here; neither is a distribution.
- **Phase 8.** Already shelved by standing decision. This document does not
  change that; it records that one decision-4 *language* item was silently
  depending on it.
