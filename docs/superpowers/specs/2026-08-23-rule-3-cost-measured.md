# Rule 3's cost: two designs, both measured, both declined

**Date:** 2026-08-23. **Result: NOT LANDED.** Phase B of the free-fire pipeline.
Corpus 539 -> 539; the tree is unchanged.

LINK 5's Rule 3 says a merged functional method is not lifted into a component.
Its cost is that **a component sees the obligations its own generic creates and
none of the merged declarations that discharge them** --
`Library/CompilerAlgebra.fss:26` is the witness. Two mechanisms were built and
measured. Both are worse than the gap.

---

## First: how big is it actually?

Eleven corpus files are first-blocked on an ambiguity. A temporary probe over
`maximal` in `winner`, printing whether every colliding candidate shares one
span -- the signature of "two stamps of ONE declaration" -- gives:

```
sameSpan=true    CompilerAlgebra.fss  Compiled10.q  Compiled10.s
                 EqualityBug2  EqualityBug3  Gen0
sameSpan=false   Compiled2.f  Compiled250  Compiled3.w  XXXillegalOverloading
                 XXXGenericOverload2
```

Six with the signature, and **two of the six are must-FAILs**.
`Compiled10.q.fss` writes `trait Bar[\T, S\] comprises {T, S}` with
`f(self, x:Bar[\S, T\])`, and `XXX10q.test` records 1.0 refusing it. So the
remaining reach is FOUR files, one of them a real target
(`Library/CompilerAlgebra.fss`) and three tests. `NN32` and `IntLiteral` were
named as further instances and **are not**: neither appears in the list.

## Design 1 -- suppress an ambiguity whose candidates share one span

REFUTED BEFORE IT WAS BUILT, by the table above. `Compiled10.q` and
`Compiled10.s` carry the signature and 1.0 refuses both, so suppressing it
accepts two must-fails and the ratchet goes red. A collision between two stamps
of one line is not automatically benign -- it is exactly what the Meet Rule
exists to demand a declaration for.

## Design 2 -- lift merged functional methods for the VALIDITY CHECK ONLY

The reasoning was careful and the measurement killed it anyway.
`overloads_are_unambiguous` asks whether a declaration set is WELL FORMED, and
that question legitimately depends on declarations the importing file can see;
lifting merged functional methods into a second map read by nothing else --
never a call, never a dispatch table, never a slot, not one byte of IR -- looked
safe by construction.

**corpus 539 -> 118. Four hundred and twenty-one files lost.** The first thing
`Library/CompilerAlgebra.fss` reports afterwards is not its own ambiguity but

```
Library/CompilerAlgebra.fss:17:5: `||` is ambiguous for (FlatString, FlatString)
```

which is THE SELF-POSITION OPERATOR PAIR -- `opr ||(self, b:F)` beside
`opr ||(a:F, self)` in `Library/FlatString.fsi`, recorded in `04-state.md` as
UNDECIDED since the three-link session. Making merged functional methods visible
to the validity check surfaces every latent ambiguity in the shipped library, in
every component that implicitly imports it.

## What that means

The gap is real and it is not the next thing to fix. Paying it requires either

* settling the self-position operator pair first -- 1.0's own library writes
  both, so "refuse" is probably wrong and it is a genuine open question -- and
  then whatever else the union surfaces; or
* a mechanism that lets a bodiless meet DISAMBIGUATE a set without joining it,
  which `typing_candidates` and `dispatch_target` both push back on: a bodiless
  declaration types a call and can never be a dispatch target, so a meet that
  wins the typing question still leaves the cells ambiguous.

Four files, behind an undecided question. Recorded and left.
