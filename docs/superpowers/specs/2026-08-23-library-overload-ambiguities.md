# The bootstrap root checks: `var` in an api, and four ambiguous overload sets

**Date:** 2026-08-23.
**Result: `Library/FortressLibrary.fsi` CHECKS.** 407 declarations, headers
resolved, bounds discharged. Corpus 460 -> 466, zero lost, zero crashes. The two
apis gained are `Library/String.fsi` and `Library/FortressLibrary.fsi` itself.

Two walls came down and they are unrelated to each other. One was ours -- a
parser gap in front of correct 1.0 source. The other was the library's, four
times over.

---

## The record said one wall behind `var`. There were four, and the reporter was
## picking one at random.

`04-state.md` and the comment at `end_to_end.rs` both said: neutralise
`Library/String.fsi:43` and the root walks to `:878`, `opr SQCAP` ambiguous for
a pair of `Just` instantiations. That is a true sentence about one run.

The same binary, on the same input, twelve times:

```
  5  `SQCAP` is ambiguous for (Just..., Just...)
  3  `>` is ambiguous for (EqualTo, EqualTo)
  3  `>=` is ambiguous for (EqualTo, EqualTo)
  1  `<=` is ambiguous for (EqualTo, EqualTo)
```

240 runs put it beyond argument: 64 / 62 / 58 / 56, four sets sharing the draw
almost uniformly. `api_overloads_are_unambiguous` (`types/src/lib.rs:1613`)
iterated `self.functions`, a `HashMap`, and returned at the first violation it
met.

**THIS IS THE SECOND TIME THIS PROJECT HAS PAID FOR THAT EXACT SHAPE.**
`comprises::check` reported out of a `HashMap` until 2026-08-22 and named
`XXXComprisesHidden.fss` against `T` on one run and `S` on the next; the fix
there was to carry declaration order alongside the map, and its own comment
says why. The overload check had the same defect and nobody looked, because
the tell is identical: every draw is a CORRECT refusal of the file, so nothing
goes red. What it costs is the record -- five separate documents in this repo
say the wall behind `var` is SQCAP, and that was one draw out of four.

Fixed the same way: the sets are sorted by their earliest span, ties broken on
the name, before the walk. Twenty runs, one answer. Asserted by repetition in
`the_bootstrap_root_answers_the_same_on_five_runs`, because no one-line
mutation can reach an iteration order.

---

## All four are ONE shape

A generic trait inherited at two instantiations, where the self position and a
parameter position pull opposite ways.

```
trait StandardPartialOrder[\T extends StandardPartialOrder[\T\]\]
    opr <=(self, other:T): Boolean          (*) and <, >, >=, =, CMP
end
trait StandardTotalOrder[\T ...\] extends { StandardPartialOrder[\T\], ... } end

trait Comparison       extends { StandardPartialOrder[\Comparison\] } end
trait TotalComparison  extends { Comparison, StandardTotalOrder[\TotalComparison\] } end
object EqualTo         extends TotalComparison end
```

`TotalComparison` reaches `StandardPartialOrder` TWICE -- at `Comparison` via
one edge and at `TotalComparison` via the other -- so for the tuple
`(EqualTo, EqualTo)` the set holds

```
  <=(self: StandardPartialOrder[\Comparison\],      other: Comparison)
  <=(self: StandardPartialOrder[\TotalComparison\], other: TotalComparison)
```

Static arguments are INVARIANT, so neither self type is below the other, while
`TotalComparison` IS below `Comparison` in the second column. They cross.
Neither is most specific.

`SQCAP` is the same thing with the arms swapped: `Maybe[\T\]` extends
`UniqueItem[\T\]`, so the inherited `SQCAP(self: Maybe, o: Maybe)` and `Just`'s
own `SQCAP(self: Just, o: UniqueItem)` are below each other in opposite columns.

**IT IS NOT ONE WALL AND THEN ANOTHER. It is one rule question with four
witnesses**, which is why patching them one at a time was never going to
converge -- and the first spike proved it, moving the ambiguity from `<=` to a
different pair rather than removing it.

---

## The rule question, settled three ways

### 1. The specification PERMITS the double instantiation

`trait-parameters.tex:339-351`: "Trait declarations are allowed to extend other
instantiations of themselves." `:365-371` goes further -- a trait extending
`D[\T\]` under a where clause "is a subtrait of every instantiation of
parametric trait `D` ... it really contains infinitely many methods". There is
no Java-style `cannot inherit Comparable<A> and Comparable<B>` rule anywhere in
the document. The only extend-time prohibitions are the `excludes` rule
(`traits.tex:218-228`) and the `comprises` rule (`:230-241`).

Nor does inheritance suppression save it: `traits.tex:461-467` drops an
inherited declaration only when it is overridden or its parameter type NOT
COUNTING SELF is EQUAL to one declared locally. `TotalComparison` declares no
`<=` at all, and neither does `EqualTo`.

### 2. This compiler's specificity rule is 1.0's OWN implementation

`more_specific` (`types/src/lib.rs:311-313`) is pointwise subtyping over the
self-included tuple, strict overall. `basic/overloading.tex:280-291` writes the
DOTTED form conjunctively-strict, which is stricter -- but 1.0's own
`OverloadingOracle.scala:66-70` builds `makeDomainWithSelfFromArrow` for both
kinds and asks plain `subtypeED`, which is exactly what this compiler does.
`basic/overloading.tex:156-162` puts the self parameter in its WRITTEN position
inside `P`, with "the static type of the self parameter is the trait or object
trait type in which the declaration occurs" -- which is what
`substitute_self` (`types/src/lib.rs:470`) does.

**So the compiler's rule is not the bug.** What the rule is applied TO is the
question, and `advanced/overloading.tex:377-394` answers it: "treating
functional methods as top-level functions for determining valid overloading is
too restrictive". The relaxed form is

> **The Meet Rule for Functional Methods** (`advanced/overloading.tex:396-411`):
> valid if `i = j` and *if there exists a trait or object C that provides both
> f(P) and f(Q)* then `P /= Q` and there is a declaration `f(P INTER Q)`
> provided by C having self parameter at `i`.

`C` is `TotalComparison`, which provides both. `P INTER Q` is
`(TotalComparison, TotalComparison)`. No such declaration exists. **The library
violates 1.0's own rule**, and the relaxation the spec offers does not reach it.

### 3. 1.0's OWN TEAM PATCHED THE SOURCE, not the compiler

This is the one that settles it. Every compiler-path copy of this hierarchy in
this repository has the second instantiation CUT OUT BY HAND:

| file | what was done |
|---|---|
| `ProjectFortress/LibraryBuiltin/CompilerBuiltin.fsi:687-689` | `extends { Comparison, StandardTotalOrder[\TotalComparison\] }` commented out, `extends { Comparison }` in its place, and `CMP`, `=`, `<`, `>`, `<=`, `>=` hand-written above a comment reading "This stuff ought to be provided by StandardPartialOrder" |
| `ProjectFortress/other_compiler_tests/TestComparisonLibrary.fsi:15, 29-34, 42-43` | the same surgery, 2011 |
| `ProjectFortress/not_working_library_tests/ComparisonLibrary.fsi:16, 39-40` | the same, in a directory called `not_working_library_tests` |

And the author knew. `Library/FortressLibrary.fss:161-163` says outright:
"We're both a partial order (including Unordered) and a total order
(TotalComparison alone). **Avoid ambiguity between the default definitions of
CMP and >=**." They then wrote disambiguators for `=`, `CMP`, `<` and `>=` and
never wrote them for `<=` or `>`. The `.fsi` is a further-degraded copy of that
`.fss` -- it carries only `=`, `CMP` and `>=`.

**And the two inherited `<=` are genuinely different code**, so there is no
"same body, no harm" escape: `FortressLibrary.fss:224` defines `<=` as
`other >= self` and `:278` defines it as `NOT (other < self)`.

What would 1.0 have done at run time? `basic/overloading.tex:26-27, 274-276`:
"we choose an arbitrary declaration among the declarations such that no other
applicable declaration is more specific". **An arbitrary winner.** That is the
signed-off M3c deviation this compiler already carries -- an ambiguous call is a
compile error naming the tuple and both declarations, because an arbitrary
winner is a silently wrong answer.

---

## The four corrections

Preston's ruling from the fourteen topological corrections stands and applies
unchanged: patch the source, do not weaken the compiler.

`Library/FortressLibrary.fsi`, `trait TotalComparison` -- three declarations,
the meet the rule asks C for:

```
    opr <=(self, other:TotalComparison): Boolean
    opr >=(self, other:TotalComparison): Boolean
    opr >(self, other:TotalComparison): Boolean
```

`Library/FortressLibrary.fsi`, `value object Just[\T\]` -- one declaration, and
**this file's own sibling is the precedent**: `Nothing[\T\]` twenty-five lines
below declares BOTH `SQCAP(self, o: Maybe[\T\])` and
`SQCAP(self, o: UniqueItem[\T\])`. `Just` declared only the second.

```
    opr SQCAP(self, o: Maybe[\T\]): Maybe[\T\]
```

`<` is the same precedent at the other end: it is NOT ambiguous, because
`LessThan`, `GreaterThan` and `EqualTo` each declare
`opr <(self, other:TotalComparison)`. The library disambiguated one operator of
four and the compiler found the other three.

Every one is marked `v1 SOURCE CORRECTION` at the site, and all four were named
by the compiler -- one run per correction, in place.

**WE FIX IT THE SMALLER WAY THAN 1.0 DID.** Their three compiler-path copies
CUT THE EDGE: `extends { Comparison, StandardTotalOrder[\TotalComparison\] }`
commented out, `extends { Comparison }` in its place, and six operators
hand-written to replace what the lost supertrait provided. That destroys the
subtype relation as well as the ambiguity -- nothing below `TotalComparison` is
a `StandardTotalOrder` any more. Adding the meet keeps every edge and every
inherited declaration and removes only the tie, which is precisely what
`advanced/overloading.tex:396-411` asks for. Four lines against three
rewritten traits.

### What is NOT corrected, and why

- **`Library/FortressLibrary.fss`.** It has the identical defect. A `.fss`
  declaration needs a BODY, and the two inherited implementations disagree
  (`other >= self` against `NOT (other < self)`), so choosing one is a semantic
  decision the compiler cannot name. That file does not check yet -- it stops
  on `throw` -- so the choice would also be unverifiable. **Owed, not done.**
- **`CompilerLibrary/FortressLibrary.fsi`** IS corrected, for parity with its
  twin, and the marker says so: that copy stops at `:86` on `Self`, so the
  correction there is carried rather than independently compiler-named.

---

## `var`, the other wall

`Library/String.fsi:43` writes `var maxLeafSize: ZZ32`, which is correct 1.0
(`Variable.rats:42-45`, `AbsVarDecl = AbsVarMods? VarWTypes`, no initializer).
The parser could not read it, so the resolver skipped the whole api as
unreadable and `StringStats` never reached the root. **The source is
untouched** and a test asserts it stays untouched.

`ValueDecl` already carried `mutable`, and codegen already gives every
top-level value an internal global with a real store
(`codegen/src/lib.rs:551-563`, `:981-996`, `:1021-1026`, `:2958-2981`) -- so
`var x: ZZ32 = 10` needed no new lowering, only the keyword. `varvalue.fss` is
compiled AND RUN, because a flag that parsed and lowered to a constant would
pass any check that stopped at the diagnostic.

**THE RECORDED CLASS WAS WRONG AND THE CEILING WITH IT.** `04-state.md` filed
`String.fsi:43` under "`expected an expression, found KwVar`, 58
first-blockers". It is not in that class. The 99 KwVar first-blockers are three
DISJOINT messages and three different features:

| count | message | shape | this milestone |
|---:|---|---|---|
| 24 | `expected a function name, found KwVar` | top-level / api declaration | **taken** |
| 17 | `expected a parameter name, found KwVar` | `object O(var x: T)` | not taken |
| 58 | `expected an expression, found KwVar` | local `var x: T`, no initializer | **refused by name** |

`var` in a value parameter is a `Param` flag this AST does not have and a field
`object_fields` (`types/src/lib.rs:836-843`) hard-codes to `mutable: false`;
bounded work, measured at 17 first-blockers, and not on this milestone's path.

The 58 are DELIBERATELY NOT TAKEN. `variables.tex:203-210` makes referring to
such a variable before its first assignment a STATIC ERROR -- a
definite-assignment analysis. An `alloca` with no store and no analysis is a
silent wrong answer, not a missing feature, so the form is refused by name
(`DelayedInitializationUnsupported`) rather than left to fall out of the
parser's shape.

Also refused by name: the parenthesised list `var (x: T, y: U)`, which needs
the tuple value this backend has no representation for; and
`var x = 5` with no type, which `Variable.rats:17` lists as an explicit
ERROR PRODUCTION and which the checker already refused under `:=`.

---

## Carried forward, found on the way and not fixed here

- **`TypedValue.mutable` (`types/src/types.rs:921`) is written and read by
  NOTHING.** Safe only because codegen makes every top-level value a cell
  regardless and immutability is enforced entirely in the checker. Recorded, not
  removed: the next thing that reads it will want it to be right.
- **`conform.rs:145-193` ignores `mutable` entirely**, so an api's
  `var x: T` and a component's immutable `x: T = e` are indistinguishable to
  conformance. A gap, not a wrong answer, and adjacent to the already-recorded
  "an api's value declaration is not conformance-checked".
- **`winner` reports the first TWO of `maximal` and says "both"** even when
  three or more tie (`types/src/lib.rs:4944-4945`).
- **An imported span is still rendered against the importing file.** Every one
  of the four ambiguities pointed its header at a COMMENT LINE in
  `FortressLibrary.fsi` and both of its notes at one declaration. That cost a
  wrong diagnosis on the first spike, and it will tax every remaining
  cross-file wall.
