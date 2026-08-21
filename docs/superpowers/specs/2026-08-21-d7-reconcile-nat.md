# D7. Reconcile `nat` — `02-stack` "permanent" against decision 4 "v1"

**Decision: `nat`, `int` and `bool` static parameters are IN v1, restricted to
STATICALLY KNOWN arguments. The runtime-instantiation half of
`trait-parameters.tex:82` is a named deviation and is refused by name.
`unit` and `dim` move to sub-phase 4d. `opr` is a fourth kind and is scoped
separately below.**

Status: **ADOPTED AND IMPLEMENTED, 2026-08-21, at the consolidation.** Written
against master `f81f41ace`; every measurement reproduced by hand with a
sha256-pinned driver (`7e103205cb54`).

**What landing it cost and bought, measured on the consolidated tree:**

* corpus **350 → 358**, zero lost. genericTest1, genericTest2, tparams0/1/2,
  Compiled1.av, Compiled6.af and `test_library/TestNative.fsi`.
  `genericTest1.fss` checks its own arithmetic — `f[\1\]() + g[\2,3\]() = 6`
  prints `pass`.
* oracle pass **301 → 303**, gate green, zero new must-fail acceptances.
* **`Library/FortressLibrary.fsi` MOVED FROM BYTE 37399 TO BYTE 44522** and its
  next wall is `abstract opr[i:I]:=(v:E) : ()` — an operator DECLARATION form,
  which is `SPIKE-OPEXPR` and not this decision. That is §3.4's predicted "wall
  behind the wall", and it is the honest measure of progress on the bootstrap
  root: 7,123 more bytes of the library's own api, and the blocker is now a
  different owner's.
* the api census moved by ONE file (42 → 43 over the 183-file `all` group; the
  114-file census set itself held at 14). The corpus moved eight. Both numbers
  are here because quoting either alone overstates or understates it.

**What was NOT built, and the reason is measurement rather than scope:** the
constraint solver. §4's census found ZERO `where { k < n }` in 1956 files, so a
bound on a value parameter is REFUSED BY NAME rather than dropped in silence.
Re-open it when a corpus file writes one.

**The sublanguage as implemented** is exactly what the corpus writes: integer
and Boolean literals, a reference to an enclosing value parameter, `+`, `-`, and
JUXTAPOSITION AS PRODUCT (`(imax jmax kmax) + (2 jmax imax)`, 13 sites). No `*`
and no `/` — neither is written anywhere in static-argument position, and a form
nobody writes is a refusal rather than a guess. Evaluation happens AT THE
SUBSTITUTION, which is what makes `[\2 + 3\]` and `[\5\]` one stamp against
`MAX_INSTANTIATIONS` rather than two.

---

## 1. The contradiction

Two current documents disagree and both are load-bearing.

- **`02-stack.md`**, under **"Locked constraints"** — which in that file means
  *permanent* — lists `` `nat`/`int`/`bool`/`opr`/`unit`/`dim` refused at the
  parser ``.
- **ROADMAP decision 4** lists `nat` constraint solving **and** static arguments
  (`nat` with minus, `int`, `bool`, `dimension`, `unit`) as **v1 items**.

The parser refusal is real and per-kind:

```
trait T[\nat  n\] end  ->  `nat`  static parameters are not implemented; M3d is type parameters only
trait T[\int  n\] end  ->  `int`  static parameters are not implemented; M3d is type parameters only
trait T[\bool n\] end  ->  `bool` static parameters are not implemented; M3d is type parameters only
trait T[\unit n\] end  ->  `unit` static parameters are not implemented; M3d is type parameters only
trait T[\dim  n\] end  ->  `dim`  static parameters are not implemented; M3d is type parameters only
```

`02-stack`'s "locked" is correct about **why** it was refused — M3d is
monomorphization and monomorphization needs a finite, statically-known
instantiation set. It is wrong to call the refusal permanent, because the
restriction that actually matters is narrower than the refusal.

---

## 2. Why this is a decision and not a spike

`Specification/basic/trait-parameters.tex:82`, on `nat` and `int` parameters:

> *"These parameters are instantiated **at runtime** with numeric values."*

**A monomorphizing compiler cannot do that.** You cannot stamp a specialisation
for a value you do not know until the program runs.

And this is not a theoretical corner of the spec — **1.0 ships the escape hatch
as a library api**. `ProjectFortress/LibraryBuiltin/NatReflect.fsi`:

```
trait NatParam
  abstract getter toZZ() : ZZ32
end
value object N[\nat n\] extends { NatParam } end
reflect(z:ZZ32):NatParam
```

with its own comment: *"you can call the function `reflect(n)` and it will return
an instance of `NatParam`. But every instance of `NatParam` is an object
`N[\n\]`, so we can write a function which takes `N[\n\]` and pass it a
`NatParam`; **within that function `n` becomes a static nat parameter**."*

That is a **runtime `ZZ32` becoming a static parameter**, and the library uses
it: `Library/ChunkedSparseArray.fss:126` is
`csa[\T, nat n\](_:N[\n\], t:T): Array[\T,ZZ32\] = ChunkedSparseArray[\T,n\](t)`
— a constructor whose size parameter arrives through the reflection witness.
`NatReflect` is also one of the two apis `Library/FortressLibrary.fss:4-5`
imports directly (`NativeArray` is the other), so it is on the bootstrap path,
not in a corner.

There are exactly two answers and both are deviations from something:

| Answer | Costs |
|---|---|
| **(a) Statically-known arguments only** | refuses `NatReflect.reflect` and every use of it. A deviation from 1.0. |
| **(b) Erase `nat` to a runtime value** | undoes the type-level guarantee, forces a runtime representation of static parameters, and re-opens M3d's "monomorphization, never erasure and never boxing". A deviation from a landed, locked decision. |

---

## 3. The decision

### 3.1 `nat` / `int` / `bool` are in v1, with arguments statically known

A `nat`, `int` or `bool` static **argument** must be a compile-time constant: an
integer or boolean literal, or a static expression over the enclosing
declaration's own static parameters. `MAX_INSTANTIATIONS = 4096` continues to
bound the result, unchanged, and it stays a whole-program budget (see D5).

**The static-expression sublanguage is not optional and is part of 4b.** The
corpus writes arithmetic in static-argument position at **17 sites across 8
files** — `Library/Generator22D.fss` alone writes
`[\T, 0, s0, 0, s1 + s2\]`, `[\T, s0 + s2, s1\]`,
`[\T, 0, s0 + s2, 0, s1 + s3\]`. So "literals only" is too narrow to compile the
library's own array generators; the rule is *statically evaluable*, not
*literal*.

### 3.2 `NatReflect`'s runtime path is a named deviation, refused by name

`reflect(z:ZZ32):NatParam` and the pattern of passing a `NatParam` where an
`N[\n\]` is expected are **out of v1**. This must be a diagnostic that names the
mechanism — something of the shape *"a `nat` static argument must be known at
compile time; `NatReflect.reflect` produces one at run time, which this compiler
does not implement"* — and not a generic type error, because the failure will
otherwise surface as an unrelated mismatch deep inside `ChunkedSparseArray`.

**What it costs, measured:** `ChunkedSparseArray` is the only library user, and
its `csa` entry point is the only site. `ChunkedSparseArray` is not in the
5-layer api dependency graph's lower layers and nothing in the bootstrap set
imports it.

**What re-opens it:** a corpus program that needs a runtime-sized array type and
cannot be written with `Array[\T,ZZ32\]`. None exists today.

### 3.3 `unit` and `dim` are deferred to sub-phase 4d, not cut

They stay decision-4 v1 items and they are gated on `SPIKE-COMPOSITE-TYPE`, not
on this decision. **Size them from the corpus, not from the spec's page count:**
`unit` static parameters appear in **6 corpus files and zero library files**;
`dim` static parameters appear in **zero corpus files at all**. `dimensions.tex`
is 4d's specification and the corpus is not its witness.

### 3.4 The ordering constraint this creates

**D7 must be taken before the parser change, and the parser change before the
solver.** The sequence is:

1. **D7 signed off** (this document) — decides *what a legal `nat` argument is*.
2. **A parser spike**: `nat`/`int`/`bool` static parameters parse; `unit`/`dim`
   keep their refusal; `opr` per §4. Re-run the api census immediately — the
   `329 → 74 → 234` precedent says expect a new wall behind this one.
3. **Sub-phase 4b**: the static-expression evaluator, then the constraint solver.

Doing (2) before (1) means the parser accepts a shape nobody has decided the
meaning of, and `ChunkedSparseArray` will be the file that discovers it.

---

## 4. `nat` is on the critical path, and the scope list has a hole

**It is how the library declares its array types.** Measured over the corpus
with comments and string literals stripped:

| kind | corpus files | of those, in `Library/` + `CompilerLibrary/` |
|---|---|---|
| `nat` | **61** (377 sites) | **15**, including `Library/FortressLibrary.fsi`, `Library/CompilerLibrary.fss`, `ChunkedSparseArray.{fss,fsi}` |
| `opr` | 15 | 1, and it is `Library/incomplete/advanced/Fortress.PartialTotalOrders.fss` |
| `bool` | 13 | 0 |
| `int` | 10 | 0 |
| `unit` | 6 | 0 |
| `dim` | **0** | 0 |

`Library/FortressLibrary.fsi` declares `trait Rank[\nat n\]`,
`trait Indexed1[\nat n\]`, `trait ReadableArray1[\T, nat b0, nat s0\]` and
`subarray[\nat b, nat s, nat o\]`. **5 census files block on `nat` today**, and
the 65-file `array-and-matrix-types` bucket under `--real` is downstream of the
same thing.

### The `opr` hole in decision 4's scope list

Decision 4's static-arguments item names *"`nat` with minus, `int`, `bool`,
`dimension`, `unit`"* and **omits `opr`** — while its separate *"the types that
classify operator properties"* item **requires** it. Those traits are
`Library/incomplete/advanced/Fortress.Operators.fsi.INCOMPLETE`, and every one of
them takes one or more `opr` parameters.

**Two facts make that item bigger than it looks, and both should be recorded
here rather than discovered later:**

1. **Those ~30 declarations are `%`-commented LaTeX, not Fortress source.** So
   the decision-4 item begins by *writing* the declarations. Same shape as
   `Fortress.Components` in D5.
2. **There is no live `opr` static parameter in the census set.**
   `Library/FortressLibrary.fss:2823`'s `trait Monoid[\T, opr OPLUS\]` and
   `FortressLibrary.fsi:2023`'s `embiggen[\T,opr OP\]` are both inside `(* *)`
   comment blocks. The single live library use is under `Library/incomplete/`,
   which is outside the 114-file census set by name and by decision.

**Recommendation: `opr` static parameters are a v1 item, scoped with the
operator-property traits and NOT with `nat`.** They are a different mechanism —
an `opr` parameter is a name in operator position, which is `SPIKE-OPEXPR`
territory, not arithmetic — and bundling them into the `nat` parser spike would
attach a feature with one live library witness to the one with fifteen. Keep the
`opr` refusal in place when `nat`/`int`/`bool` open, and say so in the parser
spike's scope.

---

## 5. What to write down where

- **`02-stack.md`**: move `nat`/`int`/`bool` out of "Locked constraints" and
  replace with the restriction that is actually locked — *a `nat`, `int` or
  `bool` static argument must be statically evaluable; `NatReflect`'s runtime
  path is a named deviation*. Leave `unit`/`dim`/`opr` refused, with a pointer
  to sub-phase 4d and to §4 above.
- **ROADMAP decision 4**: add `opr` to the static-arguments item, and note that
  the operator-property traits start from writing declarations that exist only as
  commented LaTeX.
- **The deviation register** (D6 §4 starts one): add `NatReflect.reflect`.
