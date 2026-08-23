# The same Meet Rule defect, one hierarchy over: `NN32`

**Date:** 2026-08-23.
**Result:** `ProjectFortress/LibraryBuiltin/FortressBuiltin.fsi` CHECKS. 192
declarations, headers resolved, bounds discharged. Corpus 507 -> 508, zero lost.
`API_FLOOR` 116 -> 117.

Three `v1 SOURCE CORRECTION` declarations, and they are the SAME THREE
operators the Comparison hierarchy was missing on 2026-08-23 --
`docs/superpowers/specs/2026-08-23-library-overload-ambiguities.md`.

---

## The diagnostic named the file it was rendered against, not the file it meant

```
FortressBuiltin.fsi:23:36: `>` is ambiguous for (NN32, NN32)
FortressBuiltin.fsi:219:1: note: one declaration is here
FortressBuiltin.fsi:16:26: note: and the other is here
```

Line 23 is prose inside a doc comment and line 16 is the middle of a sentence.
Both notes carry a span that belongs to an IMPORTED file and is rendered against
the importing one -- the defect already recorded as "an imported span is
rendered against the importing file". **Reasoning the chain out is not the same
as knowing it**, so the two declarations were printed instead: a temporary
`eprintln!` over `maximal` in `winner`, one run, then reverted.

```
PROBE >: (NN64, NN64) -> Boolean                          span 11231..11263
PROBE >: (StandardTotalOrder$NN32$e, NN32) -> Boolean     span 548..577
```

Resolving those offsets against the candidate files:

```
ProjectFortress/LibraryBuiltin/CompilerBuiltin.fsi:338   opr >(self, other:NN64): Boolean
Library/CompilerAlgebra.fsi:18                           opr >(self, other:T): Boolean
```

**Not what the chain suggested.** The guess had been
`NN64 -> Integral[\NN64\] -> StandardTotalOrder[\NN64\]`, a generic trait at two
instantiations. It is not: the second declaration is on the GROUND trait `NN64`,
in `CompilerBuiltin`, which extends `{ Number, Equality[\NN64\] }` and reaches
no order trait at all. Two `NN64`s exist -- `CompilerBuiltin.fsi:313` and
`Library/FortressLibrary.fsi:489` -- and the merge takes the builtin's.

## The shape

```
value object NN32 extends { StandardTotalOrder[\NN32\], NN64 }
  StandardTotalOrder[\T\]  CompilerAlgebra.fsi:18    abstract opr >(self, other:T)
  NN64                     CompilerBuiltin.fsi:338            opr >(self, other:NN64)
```

`>` arrives at `NN32` twice, as `(StandardTotalOrder[\NN32\], NN32)` and as
`(NN64, NN64)`. The SELF positions are unrelated -- a generic trait's static
argument is invariant and `NN64` is not one of its instantiations -- while the
second parameter runs the other way, `NN32` below `NN64`. The two cross and
neither is most specific. It is the Comparison shape with one leg replaced by a
ground trait, which is why the same three operators come out.

`advanced/overloading.tex:396-410`, the Meet Rule for Functional Methods: "if
there exists a trait or object C that provides both f(P) and f(Q) then P /= Q
and there is a declaration f(P INTER Q) provided by C". C is `NN32` and
P INTER Q is `(NN32, NN32)`.

## Three, and the file already writes two of them

`value object NN32` declares `opr =(self, b:NN32)` and `opr <(self, b:NN32)` --
which is exactly why `=` and `<` do not collide and `>`, `<=` and `>=` do. The
correction writes the missing three in the same spelling, at the same place.

`CMP` is **not** owed: `NN64` declares none, so nothing collides.
`MIN`/`MAX`/`MINMAX` are not owed either -- this `StandardTotalOrder` (the
`CompilerAlgebra` one, five abstract operators) does not extend `StandardMinMax`
the way `Library/FortressLibrary.fsi`'s does.

`object IntLiteral`, further down the same file, is 1.0's own precedent for the
shape: it extends `ZZ32` and writes all five of `<`, `<=`, `>`, `>=` and `CMP`
at its own type.

## Patching one and watching it move is what said it was a class

Two minutes, before anything was written. `>` alone was added and the file was
re-run:

```
`>`  is ambiguous for (NN32, NN32)   ->   `<=` is ambiguous for (NN32, NN32)
```

The ambiguity moved rather than going away, which is the cheapest test that a
patch is treating a symptom. All three then, and the file checks.

## `FortressBuiltin.fss` HAS THE IDENTICAL DEFECT AND IS DELIBERATELY NOT CORRECTED

Same object, same clause, `FortressBuiltin.fss:387`. Left alone for the reason
`Library/FortressLibrary.fss` was on 2026-08-23:

* a `.fss` declaration needs a **body**, and the body here would be a
  `builtinPrimitive("...NN32$Greater")` -- a GUESS at the name of a Java glue
  method that this compiler will never call;
* the file does not check today, so the correction would be unverifiable. It
  stops at `:45`, `expected an expression, found Colon`, in the parser.

Resolve it when the implementation files start parsing. Three declarations are
owed -- `>`, `<=`, `>=` at `(NN32, NN32)` on `value object NN32`.

## What holds it

The **api floor**, and it is the only thing that can. No mutation row can reach
corpus source; `API_FLOOR` sits at the measurement with no slack, because an api
emits no object and none of the 38 accepted must-fails is inside that count.
Proved by refusal rather than asserted: with the three declarations reverted,
apply-gate reports `116 corpus .fsi files check / floor is 117` and goes 45/1.
