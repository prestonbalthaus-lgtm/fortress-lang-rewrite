# The fourth Comparison meet declaration: `=` at `(TotalComparison, TotalComparison)`

**Date:** 2026-08-23. **Result: landed, and it gains zero corpus files.**
One declaration on `trait TotalComparison`, in both `FortressLibrary.fsi`
copies, taking the 2026-08-23 morning's three corrections to four.

---

## The shape, which is the one already documented three times

```
Comparison       extends StandardPartialOrder[\Comparison\]      -> Equality[\Comparison\]
TotalComparison  extends { Comparison, StandardTotalOrder[\TotalComparison\] }
                                                                 -> Equality[\TotalComparison\]
```

`=` therefore arrives at `(EqualTo, GreaterThan)` twice:

* `(TotalComparison, Comparison)` -- this trait's own `opr =(self, other:Comparison)`
* `(Equality[\TotalComparison\], TotalComparison)` -- inherited

Position 1 favours the first (`TotalComparison` is below
`Equality[\TotalComparison\]`); position 2 favours the second (`TotalComparison`
is below `Comparison`). They cross, neither is most specific.

`advanced/overloading.tex:396-410`, the Meet Rule for Functional Methods: C is
`TotalComparison` and `P INTER Q` is `(TotalComparison, TotalComparison)`.

    opr =(self, other:TotalComparison): Boolean

## Why it is later than the other three

Nothing could reach it. It needs `CompilerAlgebra`'s `Equality` merged, which
only happens when a file imports both core apis -- and no corpus file does.
It surfaced behind the component-side import simulation
(`2026-08-23-component-side-core-import-measured.md`).

## How it was verified, since the corpus number does not move

`Library/Reader.fss`, in place, with both core apis written in temporarily:

```
before:  Library/Reader.fss:57:4: `=` is ambiguous for (EqualTo, GreaterThan)
after:   the ambiguity is gone; the file stops on the next wall
```

And in the other direction, on a three-line component: with the declaration the
`=` ambiguity does not appear, and with it deleted the same file reports
```
`=` is ambiguous for (EqualTo, GreaterThan): the declarations below are both
most specific, and neither is more specific than the other
```

## NOTHING IN THE TREE HOLDS IT, AND THAT IS SAID RATHER THAN PAPERED OVER

No corpus file reaches it, so the compile metric cannot. No mutation row can
reach corpus source. A positive gate fixture was written and then DELETED: a
component importing both core apis is exit **70**, not exit 0, so the fixture
would have asserted an internal error. The verification above is repeatable by
hand and is the record.

The full sweep after landing it: 520 -> 520, zero lost, zero gained, and the
only line that changed in 1956 is `FortressLibrary.fsi`'s own token count.
