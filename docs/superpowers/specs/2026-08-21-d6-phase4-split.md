# D6. The phase-4 split, the where-clause decision, and two corrections

Decision 4 says: *"Dimensions and units, coercion, where clauses and `nat`
constraint solving are four separate inference problems that happen to live in
the same checker... Split phase 4 before starting it and give each part its own
exit criterion."* The split has never been written. This is it.

Status: **drafted, not adopted.** Written against master `f81f41ace` on
2026-08-21; every measurement reproduced by hand with a sha256-pinned driver
(`7e103205cb54`).

**§1 is the part someone is blocked on. It is decided and it is actionable
today.** §2 is the split. §3 and §4 are the two corrections the split cannot be
written without.

---

## 1. The where-clause decision

### 1.1 The decision

**v1 where clauses are CONSTRAINTS OVER DECLARED STATIC PARAMETERS. Where-clause
VARIABLES are refused, by name, with their own diagnostic.**

Concretely:

| Form | v1 |
|---|---|
| `where { T extends Foo }`, `T` in the enclosing `[\...\]` list | **in** — routes to the existing `BoundObligation` / `discharge_bounds` path |
| `where { T coerces U }`, `T widens U` | out, refused by name — coercion is sub-phase 4c |
| `where { NatConstraint }`, `IntConstraint`, `BoolConstraint`, `UnitConstraint` | out, refused by name — gated on D7 |
| `where { type X = Foo }` (type alias) | out, refused by name |
| **`where [\ bool b, nat n \] { ... }`** — the BINDER form, which introduces static variables bound nowhere else | **out, refused by name.** This is the where-clause-variable feature |
| `where { T extends Foo }` where `T` is NOT a declared static parameter | **out, refused by name.** Same feature as the row above, written in the constraint form |

**This makes the narrow discard fix safe.** `skip_where` today brace-matches
*tokens* and returns `Ok(())` at five call sites
(`crates/parser/src/lib.rs:448, 478, 608, 684, 783, 956`), so
`f(x: ZZ32): ZZ32 where { this is total garbage } = x` compiles, links and runs.
Under this decision the fix is bounded: parse the trait-constraint form, check
its subject is a declared static parameter, route it to `discharge_bounds`, and
refuse every other form with a diagnostic that names it. No inference, no new
substrate, no interaction with M3d's phase order.

**Expect losses in both directions and say so in the commit.** Text that
compiles today because the clause was discarded will start refusing. That is the
point.

### 1.2 Why this is safe, measured

The whole corpus writes 16 where clauses across 1956 files. **5 survive
`triage --real`. ZERO are in the 114-file census set** — the library bootstrap
needs no where clause at all.

And every one of the five is **already blocked on something else**, so refusing
where-clause variables costs **zero files today**:

```
Fortress.Convenience.fss           expected a newline or `;`, found Dot
Fortress.PartialTotalOrders.fss    `opr` static parameters are not implemented
whereTest.fss                      `int` static parameters are not implemented
GenericFnWithExcludes.fss          `nat` static parameters are not implemented
conditionalExtension.fss           `opr` static parameters are not implemented
```

Three of the five write the binder form and all three also need `nat`/`int`/
`bool`/`unit` parameters, which D7 defers. The two that use only the constraint
form are `Fortress.PartialTotalOrders.fss` — where `T` **is** a declared
parameter (`trait HasMaximalElement[\T extends ..., opr PRECEQ\] ...
where { T coerces MaximalElement[\PRECEQ\] }`), so it is in-scope shape blocked
on `opr` — and `Fortress.Convenience.fss:44`
(`object Nothing extends Maybe[\T\] excludes Just[\T\] where {T extends Object}`,
where `Nothing` has no static parameter list), which is the single genuine
where-clause-variable site in the tree and is under `Library/incomplete/`.

### 1.3 Why the other branch is not available

1.0 demands where-clause variables **semantically**, not as sugar.
`trait-parameters.tex:340-352`: `trait C[\S\] extends C[\T\] where {S extends T,
T extends Object}` means *for every subtype `S` of `T`, `C[\S\]` is a subtype of
`C[\T\]`* — variance expressed by quantification. `:365-374` is sharper still:
`trait C extends D[\T\] where {T extends Object}` makes `C` a subtrait of
**every** instantiation of `D`, and the spec says outright that such a trait
"really contains infinitely many methods (one for each instantiation of `T`)"
and that "it must be possible to **infer** which method is referred to at the
call site."

That is inference over an unbounded instantiation set. It collides head-on with
two things that are already locked and already shipped:

- M3d's **"static arguments are WRITTEN, never inferred"** — the property that
  makes demand syntactic and lets expansion run as an AST-to-AST pass *before*
  `Checker::new`;
- M3c/M3d's frozen `registry.concrete` and 32-bit tags, and
  `MAX_INSTANTIATIONS = 4096`, which is a bound on a **finite** instantiation
  set.

Supporting where-clause variables means re-ordering M3d's phase split, which is
`04-state`'s own "THE PHASE SPLIT IS LOAD BEARING". Paying that for one file
under `Library/incomplete/` is the wrong trade.

**Recorded as a named deviation from 1.0, of the M3c kind.** It re-opens if the
library bootstrap turns out to need it after `SPIKE-VARARGS` and D7 land — that
is the trigger, and the api census is the instrument that will show it.

### 1.4 One caveat for whoever writes the parser half

`trait-parameters.tex:286` carries the spec's own
`\note{The where clause syntax in this section is out of date.}` **directly above
the grammar**. The corpus proves it: the grammar gives only
`where { WhereClauseList }`, but **10 corpus files write the binder form
`where [\ ... \]`** and only 8 write the braced form. Do not implement the
printed grammar and assume the corpus matches it. Refusing the binder form by
name is also what stops that mismatch being silent.

---

## 2. The split

Four sub-phases. Each has its own exit criterion. **They are not independent and
the ordering below is the dependency order, not a preference** — nat solving,
unit equality and coercion all arrive through where-clause syntax, so
sequencing them as parallel workstreams builds the same substrate three times.

### 4a. The checking substrate

*Not on decision 4's list, and it has to come first — see §3.*

Build what the other three extend: a type representation that can hold composite
types, and a constraint form that survives inference rather than being checked
after it.

*Exit:* `SPIKE-COMPOSITE-TYPE` has landed, tuples are a real `Type`, and the
where-clause trait-constraint form of §1 is enforced with negative fixtures at
both call sites (object and function).
*Depends on:* nothing. *Blocks:* all three below.

### 4b. `nat` / `int` / `bool` constraint solving

The static-expression sublanguage, `nat` and `int` static parameters, and the
constraint solver over them.

*Exit:* the library's array types (`ReadableArray1[\T, nat b0, nat s0\]` and the
`Array2`/`Generator2` family) parse, check and instantiate; the api census shows
the `nat` bucket cleared; **and the D7 restriction is enforced by name** — see
D7, which must be taken before this sub-phase starts.
*Depends on:* 4a. *Blocks:* dimensions (a dimension exponent is an `int`), array
type syntax, the numeric tower's width variants.

### 4c. Coercion

`coerce`/`coerces`/`widens` declarations, the coercion set, and the hook in
overload applicability — which is where 1.0 puts it
(`conversions-coercions.tex:40-53`).

*Exit:* `CompilerBuiltin`'s numeric tower coercions (15 of the corpus's 48
`coerce` uses) are declared and applied; the `widen` dead end is closed (there is
**no expressible way to get an `RR64` from a `ZZ32` anywhere in the language**
today).
*Depends on:* 4a — `conversions-coercions.tex:51-52` provides implicit coercions
for **tuple and arrow** types, so coercion cannot be finished before those are
real. *Blocks:* the numeric tower.

### 4d. Dimensions and units

`dim`/`unit` as a type-level abelian group of exponents under product, quotient
and natural power (`dimensions.tex:22-45`), surviving inference.

*Exit:* the seven unit-operator words lex, unit equality is decided in the
checker rather than after it, and a unit mismatch is a static error with a
diagnostic naming both units.
*Depends on:* 4a for the representation and 4b for the exponents.
**Size this as representation cost, not corpus payoff** — only 6 corpus files
carry a `unit` static parameter and 0 carry a `dim` one.

---

## 3. Correction: there is no Hindley-Milner engine to extend

`02-stack.md` and ROADMAP phase 4 both describe *"Hindley-Milner type inference
with traits, polymorphism and overload resolution"*, and the ROADMAP's phase-4
prose reads as though the phase extends an existing engine.

**There is none.** `grep` across `fortressc/crates` for `unify`, `TypeVar`,
`Substitution`, `occurs_check` and `fresh_var` returns **zero hits**, and `Type`
(`crates/types/src/types.rs:87`) has no variable case — it is a `Copy` enum of
nine ground variants.

What exists is:

- **bidirectional checking** against `expected: Option<Type>`, 33 sites in
  `types/src/lib.rs`; and
- **one declaration-order fixpoint** over inferred *return* types
  (`resolve_inferred_returns`, landed in `a0e75d60b`).

That is a real and working design, and it is not HM. The difference between
"phase 4 extends an engine" and "phase 4 builds one" is the entire cost of the
phase, and it is currently mis-stated in two places.

**Action:** amend `02-stack.md`'s `<type_system>` line and ROADMAP phase 4 to say
bidirectional checking with a declaration-order fixpoint, and let sub-phase 4a
decide whether an inference variable is needed at all. **It may not be.** Every
1.0 feature on decision 4's list is a *constraint* problem over written static
arguments, not a *reconstruction* problem — M3d already locked "static arguments
are WRITTEN, never inferred". Adopting HM machinery because a stale ROADMAP line
names it would be the most expensive possible way to honour a typo.

---

## 4. The `Library/QuickSort.fsi` deviation

**Phase 4's exit criterion — "type checks `Library/`" — is unreachable as
written, and the obstacle is 1.0's own shipped library breaking a rule 1.0
states.**

`Library/QuickSort.fsi:16-20` declares five `quicksort` overloads:

```
16  quicksort[\T\](lt:(T,T)->Boolean, arr:Array[\T,ZZ32\], left:ZZ32, right:ZZ32):()
17  quicksort[\T\](lt:(T,T)->Boolean, arr:Array[\T,ZZ32\]):()
18  quicksort[\T extends StandardTotalOrder[\T\]\](arr:Array[\T,ZZ32\]):()
19  quicksort[\T\](lt:(T,T)->Boolean, xs:List[\T\]):List[\T\]
20  quicksort[\T\](xs:List[\T\]):List[\T\]
```

`Specification/basic/overloading.tex:100-105` forbids that verbatim: *"it is an
error for their static parameters to differ (up to α-equivalence), or for one
declaration to have static parameters and another to not have them."* The
compiler refuses it correctly:

```
Library/QuickSort.fsi: 581..648: declarations of `quicksort` differ in their
static parameters (the other is at 442..519); an overload set is uniformly
generic or uniformly ground
```

**This is a genuine violation and not an artefact of a coarse check.** The
implementation (`crates/types/src/mono.rs:998-1003`) compares
`static_params.len()` and each parameter's `bounds.len()`, which is coarser than
α-equivalence — but here line 18 has one parameter with one bound and the other
four have one parameter with zero bounds, so it differs under α-equivalence too.
Relaxing the check to true α-equivalence would not admit this file.

**And the neighbouring relaxation note does not apply.** `overloading.tex:98`
carries `\note{This restriction will be relaxed.}` — it attaches to the
**operator-method** overloading restriction at `:87-96` immediately above it, not
to the static-parameter uniformity rule at `:100-105`. Do not cite it here.

### The decision

**Keep the refusal. Record `Library/QuickSort.fsi` as a known 1.0-library
violation, and rewrite phase 4's exit criterion so it does not require accepting
it.**

Rationale: 1.0's static-parameter uniformity rule is already recorded in
`02-stack.md` as *"ENFORCED and permanent"*, because it is what kills candidate
growth by construction under monomorphization. Relaxing it to admit one library
file would trade a locked constraint for a file nothing imports.

Phase 4's exit becomes:

> *type checks the census set and the corpus, with every disagreement against the
> legacy implementation's recorded behaviour documented rather than silently
> matched; `Library/QuickSort.fsi` is a recorded violation of
> `overloading.tex:100-105` by 1.0's own library and is expected to be refused.*

That is the same "document, don't silently match" the criterion already asks for
— it just now has its first entry.

**Before generalising this, run `uniformity-vs-Library` first.** 22 corpus files
report "declarations differ in their static parameters". They are **not** all
QuickSort's shape: the check iterates only `Decl::Function`, so methods and
functional methods are unchecked, and it compares lengths rather than
α-equivalence. Open all 22 and separate genuine spec violations from artefacts of
the coarse check **before** deciding whether the deviation list has one entry or
twenty. That is no-code work and it belongs to this document's follow-up, not to
sub-phase 4a.
