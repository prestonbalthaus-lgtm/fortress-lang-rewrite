# `unknown type Generator` is neither a missing import nor a missing type

**Date:** 2026-08-23. **Result: a measurement, not a change.** Nothing landed
from this investigation except the `true : Boolean` correction it turned up on
the way, which is a different defect.

`Library/Reader.fss` and `Library/Writer.fss` report `unknown type Generator`.
Two hypotheses were put: a missing import, or a `Generator` type that needs
structural wiring. **Neither holds.**

---

## `Generator` needs no wiring

`Library/Stream.fsi` CHECKS -- 185 declarations, headers resolved, bounds
discharged. `Generator[\E\]` is declared at `Library/FortressLibrary.fsi:666`
and resolves fine api-side, through the implicit core-api import.

Reader and Writer write no `Generator` of their own. It arrives through
`import Stream.{...}`, on `Stream.fsi:52`'s `writes(x:Generator[\Any\]):()`.

## It is not a missing import either

`basic/components/source-code.tex:305`: *every component implicitly imports the
Fortress core APIs*. Writing `import FortressLibrary.{...}` into Reader.fss
would be patching correct 1.0 source to work around a compiler gap -- the
`Library/String.fsi:43` anti-pattern this project already refuses by name.

## What it actually is, and what forcing it costs

`implicit_import` returns early on `!component.is_api`. The component half is
ARCHITECTURALLY OUT and the reason is recorded at the site: a merged OBJECT
takes a 32-bit type tag, which shifts every dispatch table built after it, and a
merged SINGLETON is CONSTRUCTED in that program's `main`.

Forcing it on today, measured over all 1956 files and reverted. Re-run at the
tip after `Self`, the constructor work and the `true : Boolean` correction
landed, because a gate is only ever evidence about the binary it ran:

```
                  before      after forcing it
  objects            395                     0
  apis               125                   118
  total              520                   118
  exit 70/101/139      0                   221
  lost                 -                   402
  gained               -                     0
```

## Simulating it correctly shows the NEXT wall

The honest simulation is to write BOTH core apis into the two files in place
(a census run outside the tree measures the census). Done, and reverted:

```
Library/Reader.fss:57:4   `=` is ambiguous for (EqualTo, GreaterThan)
Library/Writer.fss:54:36  `=` is ambiguous for (EqualTo, GreaterThan)
```

Another Comparison-hierarchy Meet Rule gap, the same family as this morning's
four and today's three on `NN32`. **It blocks zero corpus files today**, so
correcting it would gain nothing and could not be verified in the tree. Not
done, deliberately.

*(An intermediate spike writing only `FortressLibrary` reported `unknown type
RR32`. That is an artifact of the spike and not a wall: a core api pulled
through the queue is resolved with the imports it WRITES, so `CompilerBuiltin`
has to be named too. The real design queues both.)*

## The lever is 75 files, not two

Corpus files whose FIRST blocker is a type name declared in one of the two core
apis, counted with the compiler against the two files' own declarations:

```
69 .fss + 6 .fsi = 75
Number 12   LexicographicOrder 8   ZeroIndexed 8   Generator 7   Array1 7
UncheckedException 6   IntLiteral 4   MonoidReduction 3   Equality 3 ...
```

**The six apis were a different cause and five of them are now fixed.** Five are under
`CompilerLibrary/` and were waiting on `CompilerLibrary/FortressLibrary.fsi`
PARSING -- see `2026-08-23-true-is-a-reserved-word.md`. So the component-side
import speaks for the 69 `.fss`.

## Where this leaves Reader and Writer specifically

Behind link 5, and then behind at least one more wall. They are also `native
component` JVM glue: every body is `builtinPrimitive("com.sun.fortress
.interpreter.glue.prim...")`, which is the family the oracle already refuses
with *"a foreign import reaches a JVM implementation and this compiler emits
native code"*. They are correctly unreachable; what is wrong today is only the
diagnostic they stop on.

## The decision that is not mine

Merging only TRAITS component-side is a real design option -- a trait has no
run-time representation, so neither the tag nor the singleton objection applies
to it. It would also **reverse a gated decision**: `implicitbuiltin.fss` and
apply-gate's "implicitly import the builtins into COMPONENTS too" mutation row
exist to assert the component half is out. Reopening that is Preston's call.
