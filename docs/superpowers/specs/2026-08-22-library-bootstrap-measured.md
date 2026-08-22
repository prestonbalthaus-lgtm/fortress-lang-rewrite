# The library bootstrap, measured: `coerce` is one link of five

**2026-08-22.** The brief asked to "complete the library bootstrap by linking
CompilerBuiltin.fsi, resolve the RR32 wall, and clear the 207-file
missing-library bucket", with "a fully type-checked standard library" as the
goal. Measured, that is a chain of at least five links; one of them landed, one
is architecturally out, and the last is a violation of the frozen specification
IN THE LIBRARY SOURCE that no compiler work can fix.

## The chain, in order

| link | state |
|---|---|
| 1. `coerce` parses | **LANDED**, +3 files, zero lost |
| 2. `Condition[\()\]` -- a `()` STATIC ARGUMENT | open, ONE wall class, 4 sites |
| 3. the implicit import of the builtins | api-side written and REVERTED: -57 files |
| 4. `Library/FortressLibrary.fsi`'s own uniformity violations | at least 4, and 1.0 calls them ERRORS |
| 5. the 61 no-import components | architecturally out this session |

### 1. `coerce` was the only parse blocker, and it landed

`ProjectFortress/LibraryBuiltin/CompilerBuiltin.fsi` writes fifteen `coerce`
declarations and they were the whole of why it did not parse. It is a
`Member::Coercion` now -- a variant of its own, RECORDED AND NEVER READ.

**Not a method named `coerce`**, and that is the safety argument: parsed as a
method it joins an overload set and can WIN A DISPATCH, which is a silent wrong
answer in a feature with no semantics. 446 -> 449, zero lost.

### 2. What is left in CompilerBuiltin.fsi is ONE class, 4 sites

Censused in place, whole declarations, with the import path working:

```
:453  `()` has no value, so it cannot be stored in a parameter
:653  same        :610  same        :597  same
CHECKS CLEAN after 4 removals
```

Every one is `Condition[\()\]` or its kin -- a generic instantiated at `()`.
Monomorphizing `Condition[\T\]` at `T = ()` gives a member a VOID PARAMETER,
and 1.0 reads a `()` parameter as NO parameter (a functional's single parameter
may be the empty tuple). Implementing that is arity-changing and reaches
dispatch; it is one feature, and it is the next one.

**THE CENSUS LIED TWICE BEFORE IT WAS RIGHT, and both are recorded because the
shape recurs.** Neutralising single LINES orphaned the continuation lines of
multi-line declarations and manufactured four parse walls that were my own.
Copying the file to a scratch directory broke its import path, and it then
reported `unknown type Equality` ten times -- for a name it IMPORTS at :14 and
resolves correctly in place. A census run outside the tree measures the census.

### 3. The implicit import is api-side only, and it still cost 57 files

`Specification-1.0-frozen/library/structure.tex:16-18` is explicit: the default
libraries "are automatically imported by every Fortress component and API".

**The component half is architecturally out and that is not caution.** Merged
declarations land in `component.decls`; a merged OBJECT takes a tag, which
shifts every dispatch table built after it, and a merged SINGLETON is
CONSTRUCTED in that program's `main`, because `emit_main` walks
`component.objects`. Doing it component-side would perturb the emitted IR of
every module that already compiles.

The api half was written -- it is five lines in the resolver -- and **measured at
446 -> 392, FIFTY-SEVEN FILES LOST**, so it was reverted. Every loss reported
the same thing: `()` has no value, so it cannot be stored in a parameter. That
is link 2, arriving in every api at once, because merging a library that does
not check poisons everything that imports it. **The implicit import cannot land
before CompilerBuiltin does.**

### 4. `FortressLibrary.fsi` violates 1.0's OWN overloading rule

With the builtins reachable the library clears `RR32` and walks from :362 to
:467. Behind that are at least four uniformity violations -- `__cond`,
`BIG //`, `#`, `:` -- of which the first is:

```
__cond[\E,R\](c:Condition[\E\], t:E->R, e:()->R): R
__cond[\E\](c:Condition[\E\], t:E->()): ()
```

`Specification-1.0-frozen/basic/overloading.tex:100-108`: "it is an error for
their static parameters to differ (up to alpha-equivalence), or for one
declaration to have static parameters and another to not have them." Two static
parameters against one. **Our compiler is conformant and the library is not.**
`CompilerLibrary/FortressLibrary.fsi` carries the identical pair at :764-765.

The `\note{This restriction will be relaxed.}` nearby was checked and does NOT
annotate this rule -- it sits above it, attached to the OPERATOR restriction.

So "a fully type-checked standard library" needs a NAMED DEVIATION from a rule
the frozen specification states plainly. That is a decision, not an
implementation, and it is Preston's. The narrow version -- relax uniformity for
BODILESS declarations only, where no demand is generated and the resolver never
merges function declarations into an importer -- is the same argument the
self-position rule already runs on, and it is the one to price first.

### 5. Sixty-one of the target files write no import at all

## What the 207-file bucket is actually worth

Re-measured at 449: **226 files** are blocked on an unknown type or name.

| | |
|---|---|
| unknown NAME (`print`, `sin`, `args`) | **83** -- these want the library's IMPLEMENTATIONS, i.e. separate compilation and linking. Shelved, phase 3. |
| unknown TYPE | **143** |
| ...of which write NO import | **61** -- reachable only component-side, which is out (see 3) |
| ceiling for an api-side or explicit-import fix | **82**, and only once links 2, 3 and 4 are all paid |

82 is a first-blocker count on a project where those have been wrong by up to
20x, so treat it as a ceiling and not a forecast.

## Recommendation

Link 2 (`()` as a static argument / a Void parameter meaning no parameter) is
the next milestone and the only one that is pure implementation. Link 4 is a
decision and should be taken deliberately, with the deviation written down,
because the specification is unambiguous and the library is what breaks it.
