# The implicit builtin import, and the three defects landing it exposed

**Date:** 2026-08-22. **Link 3** of `2026-08-22-library-bootstrap-measured.md`,
which recorded it as "api-side written and REVERTED: -57 files". It lands now
because link 2 landed: `CompilerBuiltin.fsi` checks, so merging it no longer
poisons every importer with its own remaining error.

**Result: 450 -> 460. Twelve apis gained, two lost, and the `.fss` count does
not move by one — 383 before and after.**

---

## The rule

`Specification-1.0-frozen/library/structure.tex:16-18`:

> the libraries that are automatically imported by every Fortress component and
> API, chiefly `FortressLibrary` and `FortressBuiltin`

Five lines in `resolve.rs`: a synthetic `import CompilerBuiltin` pushed onto the
queue at index **zero**, so the loop (which POPS) reaches it LAST and an
explicitly written import claims a contested name first.

### The api half only, and that is architectural

Merged declarations land in `component.decls`. A merged OBJECT takes a 32-bit
type tag, which shifts every dispatch table built after it; a merged SINGLETON
is CONSTRUCTED in that program's `main`, because `emit_main` walks
`component.objects`. Doing it component-side would perturb the emitted IR of
every module that already compiles. An api is checked and never lowered.

`fortressc/tests/implicitbuiltin.fsi` and `.fss` are the same six lines on both
sides of that line: the api checks, the component is still `unknown type RR32`.

### Not into the builtin, and not into what it reaches

Only the TOP-LEVEL file gets the implicit import. An api pulled in through the
queue is resolved with the imports it WRITES — `CompilerBuiltin` imports
`AnyType` and `CompilerAlgebra`, and injecting the reverse edge would make the
graph the api-first design exists to keep acyclic. Gated on the count the driver
prints: `CompilerBuiltin.fsi` writes two imports and must resolve two.

## Three defects it exposed, none of them new

The first sweep was **445 — twelve gained and SEVENTEEN lost.** Every loss was a
pre-existing defect that only an implicit import makes common.

### 1. A static parameter in a `comprises` clause was read as a type name

`Library/CompilerAlgebra.fsi:24` writes `trait Equality[\T\] comprises T`, where
`T` IS THE STATIC PARAMETER. `comprises::check` looked `T` up among the
declarations and found whatever unrelated `T` the importing file happened to
declare — `ProjectFortress/test_library/Compiled3.f.fsi` declares `trait T` —
and reported the two against each other. **Fifteen files.**

### 2. A merged `comprises` clause was reported against its importer

`ProjectFortress/BirdyLib/Comparison.fsi` declares `object LessThan extends
Comparison`; the merged builtin declares `trait TotalComparison comprises
{ LessThan, ... }` and means ITS `LessThan`, which the resolver deliberately
skipped because this file had taken the name. Together they reported that
BirdyLib's `LessThan` fails to extend a trait BirdyLib has never heard of.

The open-comprises rule in the same file already carried exactly this guard, for
exactly this reason, and said so in a comment. The comprises-must-extend rule
did not. **One file.**

### 3. The resolver keyed `seen` on the api NAME alone

`ComparisonLibrary.fsi` writes `import CompilerAlgebra.{Equality, opr =}`;
`CompilerBuiltin` writes `import CompilerAlgebra.{Equality,
StandardTotalOrder}`. Whichever was reached first decided, the other was
silently dropped, and a merged builtin trait was left extending a
`StandardTotalOrder` nothing had brought in. The key is `(name, items)` now, and
it still terminates on a cycle because `RecA` importing `RecB` importing `RecA`
is the same pair both times. **One file.**

## And one it exposed that IS load bearing: a nondeterministic diagnostic

`comprises::check` reported the FIRST violation out of a `HashMap`, so the SAME
BINARY named `XXXComprisesHidden.fss`'s defect against `T` on one run and `S` on
the next. Both are correct refusals of the same file, which is why it went
unnoticed for as long as it did — and this project asserts MESSAGES, so a
nondeterministic one is a flaky gate waiting to happen. It became load bearing
the moment `FortressLibrary.fsi`'s new wall had to be pinned: the test caught
one message and the terminal showed another.

Declaration order is carried alongside the map now. Asserted by repetition
rather than by a mutation row — swapping the iteration back is not a one-line
change, and the gate says so at the site.

## The two remaining losses are correct refusals

`ProjectFortress/test_library/RecA.fsi` declares `odd(x:ZZ32): Boolean`, and
`CompilerBuiltin.fsi:141` declares `odd(self): Boolean` on `trait ZZ32`.
`traits.tex:484-494` says a functional method "can be viewed as TOP-LEVEL
FUNCTION DECLARATIONS", so those are one signature declared twice, and 1.0 sees
the same collision because 1.0 imports the same builtins implicitly. `RecB.fsi`
is `even` in the same shape. Both are `test_library` support files; no `.test`
names either, and nothing that imports them regressed.

**Open, and not fixed here:** the resolver's own rule is that declarations the
file makes always win a contested name. Functional-method LIFTING does not
honour it — a merged trait's `odd(self)` becomes a top-level `odd` that collides
with the file's own. That is the mechanism behind these two, and widening the
resolver's rule to cover it is a milestone of its own.

## Where the bootstrap root is now

`Library/FortressLibrary.fsi` is **past `RR32`** — the wall this link existed to
clear — and stops at :335 on the library CONTRADICTING ITSELF:

    FortressLibrary.fsi:335  trait RR64 extends Number comprises { Float, FloatLiteral, RR32, QQ }
    CompilerBuiltin.fsi:447  trait FloatLiteral excludes {RR32, RR64}

One file says `FloatLiteral` is one of the traits immediately below `RR64`; the
other says the two cannot share a value. `traits.tex:232-235` makes the first an
error unless `FloatLiteral` explicitly extends `RR64`, and it does not. That is
the same class as the `__cond` uniformity violation DEV-15 answers: **the
shipped library is not conformant with the shipped specification**, and it is
link 4's territory rather than a compiler defect.

## Measured

| | link 2 | link 3 |
|---|---|---|
| `.fss` -> object | 383 | **383** |
| `.fsi` check | 67 | **77** |
| total | 450 | **460** |
| crashes | 0 | 0 |
| oracle pass | 343 | 343 |
| must-fail accepted | 38 | 38 |

GAINED (12): `CompilerLibrary/FlatString.fsi`, `Library/CompilerSystem.fsi`,
`Library/Constants.fsi`, `Library/Containment.fsi`, `Library/FlatString.fsi`,
`Library/Format.fsi`, `Library/Reader.fsi`, `Library/Stream.fsi`,
`Library/Writer.fsi`, `ProjectFortress/BirdyLib/Maybe.fsi`,
`ProjectFortress/BirdyLib/PureList.fsi`, `ProjectFortress/BirdyLib/Util.fsi`.

LOST (2): `RecA.fsi`, `RecB.fsi`, both above.

The ceiling the bootstrap doc gave for an api-side fix was **82**, called a
ceiling and not a forecast because first-blocker counts on this project have
been wrong by up to 20x. Delivered: 10 net. That is the fifth milestone running
where the first-blocker number was an over-estimate.
