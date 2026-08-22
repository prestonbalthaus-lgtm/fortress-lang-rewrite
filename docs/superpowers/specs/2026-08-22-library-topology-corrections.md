# Fourteen corrections to the shipped 1.0 library, every one named by the compiler

**Date:** 2026-08-22. **Authorized by Preston:** "we are going to take the exact
same route we took with the SI library: patch the source. We are not going to
weaken the compiler's strict topological graph checks just because the original
developers couldn't keep their `excludes` lists straight."

**Result: `Library/FortressLibrary.fsi` is past every topological wall.** The
corpus does not move -- 460, zero gained, zero lost -- because the file still
does not check; what changed is which wall it stops on, and the new one is not
the library's fault.

---

## Method: census by iterated refusal, in place

The checker reports the FIRST violation and stops. So: run the compiler, read
the diagnostic, correct exactly what it named, run again. Repeat until the error
class changes. **Every one of the fourteen was named by the compiler; none was
found by reading, and none was removed on suspicion.**

Two rules held the census honest:
- **IN PLACE.** A census run outside the tree measures the census -- copying a
  file to a scratch directory breaks its import path, which this project has
  already paid for once.
- **THE DIAGNOSTIC IS DETERMINISTIC NOW.** `comprises::check` reported out of a
  `HashMap` until earlier today, so "the first violation" was not a stable
  thing to iterate on. That fix is what made this method work at all.

## 1. Four `comprises` contradictions with the builtin

`traits.tex:232-235`: the listed traits "are exactly the traits that immediately
extend T and they must explicitly extend T".

| in `Library/FortressLibrary.fsi` | in `LibraryBuiltin/CompilerBuiltin.fsi` |
|---|---|
| `RR64 comprises { ..., FloatLiteral, ... }` | `:447 trait FloatLiteral excludes {RR32, RR64}` |
| `RR64 comprises { ..., RR32, ... }` | `:443 trait RR32 ... excludes { ZZ64, ZZ32, RR64 }` |
| `NN64 comprises { ..., NN32 }` | `:313 trait NN64 ... excludes { ..., NN32, ... }` |
| `ZZ32 comprises { ..., IntLiteral }` | `:369 trait IntLiteral ... excludes {ZZ32, ...}` |

Three of the four are EXPLICIT MUTUAL CONTRADICTIONS -- one file says the type
is immediately below, the other says the two cannot share a value.

**THE BUILTIN IS THE ONE THE SPECIFICATION AGREES WITH.**
`conversions-coercions.tex:850-866` writes `trait RR64 coerce(x:RR32) widens`:
widening a numeric is a COERCION, not inheritance. Every numeric trait in the
builtin extends `Number` and excludes its neighbours, and every one carries a
`coerce`. So the library's side is corrected.

**AND THEY ARE THE SAME PAIR BY CONFIGURATION, not by our arrangement.**
`default_repository/configuration:44` puts `LibraryBuiltin` and `Library` on ONE
source path, and `Library/` declares no `Boolean`, `RR32`, `NN32`, `IntLiteral`
or `FloatLiteral` of its own. It takes the builtin's.

**`Float`, `Int`, `UnsignedLong` and `BigNum` ARE LEFT IN.** They are declared
NOWHERE, so the clause says nothing checkable about them and the compiler never
complained. Pruning them would be guessing.

**`ZZ64 comprises { Long, ZZ32 }` NEVER FIRED**, and that is the resolver
working: `FortressLibrary.fsi:467` declares its OWN `ZZ32` which does extend
`ZZ64`, and a file's own declaration beats a merged one.

## 2. Eight Boolean operators the builtin already declares

`CompilerBuiltin.fsi:458-470` declares `opr NOT(self)`, `AND`, `OR`, `XOR`,
`->` and `<->` as functional methods on `trait Boolean`.
`FortressLibrary.fsi:2515-2523` declares the same eight as top-level functions.

`traits.tex:484-494`: a functional method "can be viewed as TOP-LEVEL FUNCTION
DECLARATIONS". So `opr NOT(self):Boolean` and `opr NOT(a:Boolean):Boolean` are
one declaration written twice, and `basic/overloading.tex` makes that an error.

**`opr ->(a: Boolean, b:()->Boolean)` IS LEFT IN**, and that is the census being
precise rather than tidy: the builtin declares no `->(self, other:()->Boolean)`,
so nothing collides and the compiler never named it.

## 3. Two methods `trait String` declares twice

`:2354-2355` declare `splitWithOffsets` and `split` as `abstract`; `:2370-2371`
declare them again with the doc comment that describes them. One trait, one
signature, two declarations. In an api the `abstract` modifier changes nothing
-- M3c decides abstractness from the absent body -- so the two are identical and
the DOCUMENTED pair is kept.

## Carriers

Patched: `Library/FortressLibrary.fsi`, `CompilerLibrary/FortressLibrary.fsi`
(the comprises clauses, the eight operators and the duplicate pair), and
`Library/FortressLibrary.fss` (the comprises clauses).

**`Library/FortressLibrary.fss`'s OPERATOR DEFINITIONS ARE LEFT ALONE**, and
that is a decision rather than an oversight. They are DEFINITIONS with bodies,
and the implicit builtin import is api-side only, so nothing is merged into a
`.fss` and nothing collides there. Removing a definition because its declaration
moved would be speculation, and the SI precedent patched both halves only
because both halves carried the same dangling NAME.

## Where the bootstrap root stops now, and it is not the library

    Library/FortressLibrary.fsi:2423:13: unknown type `StringStats`

`StringStats` is declared at `Library/String.fsi:25` and IMPORTED at
`FortressLibrary.fsi:16`. The resolver skips `String.fsi` because it does not
PARSE:

    Library/String.fsi:43:3: expected a function name, found KwVar
    43 |   var maxLeafSize: ZZ32

**That source is CORRECT FORTRESS and is deliberately not "corrected".** It is
the `expected an expression, found KwVar` class -- 58 first-blockers, the
largest single one in the corpus. Patching valid source to work around a
compiler gap is the opposite of what these fourteen corrections are.

**AND ONE MORE WALL IS MEASURED BEHIND IT.** Neutralise that single line and
`String.fsi` CHECKS CLEAN -- 60 declarations, headers resolved, bounds
discharged -- and `FortressLibrary.fsi` walks from :2423 all the way back to
:878:

    opr SQCAP(self, o: Maybe[\T\]): Maybe[\T\]  is ambiguous for a pair of
    `Just` instantiations: both declarations are most specific

That is M3c's symmetric dispatch refusing an ambiguous call, which is a
signed-off deviation from 1.0 (an ambiguous call is a compile error naming both
declarations rather than an arbitrary winner). So the next two walls are, in
order: `var` in an api, then a real ambiguity in `Maybe`.

## Measured

| | before | after |
|---|---|---|
| `.fss` -> object | 383 | 383 |
| `.fsi` check | 77 | 77 |
| total | 460 | 460 |
| crashes | 0 | 0 |
| oracle pass | 343 | 343 |
| must-fail accepted | 38 | 38 |

ONE message changed in the whole corpus, and it is the file being patched.
Nineteen gates green. No `.rs` file changed except the test that pins this
file's wall, so no mutation table is owed a re-run;
`tools/mutation-patterns.py` is clean at 109 rows over 15 tables.
