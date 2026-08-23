# Link 5: the component-side implicit core import

**Date:** 2026-08-23. **Result:** corpus 520 -> 539. +20 gained, -1 lost.
`unknown type` as a first blocker: 93 corpus files -> 26. `API_FLOOR` 125 -> 126,
`PASS_FLOOR` 344 -> 345, binaries built and RUN 402 -> 421.

`basic/components/source-code.tex:305`: *every component implicitly imports the
Fortress core APIs*. The api half landed on 2026-08-22. This is the other half,
and it was banned until now.

---

## The ban was right and its stated reason was not the blocker

`resolve.rs` said the component half was out because a merged OBJECT takes a
32-bit type tag and a merged SINGLETON is constructed in `main`. Both true. But
`2026-08-23-trait-only-core-import-declined.md` measured a trait-only variant
built to dodge exactly that, and it lost the same 402 files. What actually
stopped it was a **name collision with the builtin scalars** -- 87 files
reporting `an integer literal cannot be used where ZZ32 is required` and 40
reporting `expected String, found String`, which is the whole story in five
words: `CompilerBuiltin.fsi:25`'s `trait String` and `Type::String` print the
same and are not the same type.

So link 5 is a name-resolution milestone with a codegen tail, and it took **four
rules**. Each was measured against what happens without it.

## Rule 1 -- a merged declaration LOSES to a builtin of its own name

Component-side, a merged declaration whose name is in `types::BUILTIN_TYPE_NAMES`
is skipped. That list is now `pub` rather than `pub(crate)` and is still THE ONE
LIST; `end_to_end.rs`'s "the builtin type names agree across the passes" holds it.

*Without it:* 402 files lost, the two `String`s in conflict.

## Rule 2 -- a merged trait's supertype edge to a builtin is DROPPED

`CompilerBuiltin.fsi:51` writes `trait JavaString extends String`. With rule 1
the edge points at a name that is no longer a trait. It is dropped rather than
refused because it could never have been honoured here: a scalar has no trait
representation in this backend, which is the boxing decision and not a gap.
Dropping an edge narrows what typechecks and cannot make anything type that
should not.

*Without it:* 401 files, all reporting `` `String` is not a trait ``.

## Rule 3 -- a merged functional method is NOT lifted into a component

`traits.tex:484-494` makes a functional method a TOP-LEVEL declaration, and the
resolver already refuses to merge an api's top-level functions because those are
obligations the importer must SATISFY. Lifting them merges a function by the
back door.

*Without it:* 24 files stop at `println("U" || x)` with `expected FlatString,
found String`. `||` is the one builtin a USER declaration beats -- deliberately,
because it is an ordinary library operator -- and a MERGED declaration was
beating it too, which is not the same thing.

The same rule applies to the accessor NAME set, which has no owner: a merged
trait declaring `getter id()` made every `o.id()` in the importing file an
error. `objectTest3.fss` and `Compiled260.fss` are the witnesses.

And to two passes that match on a method's NAME and arity alone:

* **monomorphization's method stamping.** `G[\E\]`'s `generate[\R\](body,
  combine)` and the merged `Generator[\E\]`'s `generate[\R\](r, body)` are the
  same shape, so both were stamped and the checker had two live candidates.
* **`closure.rs`'s arrow-parameter lifting**, for the same reason.

`Compiled17d.fss` is the witness for both and it self-checks: it prints `34`.
Without them it read `plus` at
`(Reduction[\(Number,Number)\], Reduction[\(Number,Number)\]) -> String`.

## Rule 4 -- a merged object is lowered only ON DEMAND, and never if it is a singleton

Three conditions, three separate costs:

| condition | what it costs to drop |
|---|---|
| the file NAMES it | a hello world goes 125 -> 205 lines of IR, nine object layouts and nine constructors it never calls |
| not a SINGLETON | eighty library singletons constructed in every program's `main`, ahead of the file's own objects -- generics-gate's ordering assertion caught it in one run |
| the layout is buildable | `field \`x\` has no storage type` out of codegen, 394 files' worth: an api may declare a field of a type this backend cannot store |

The demand set is syntactic and conservative -- `closure::free_names` over the
file's own bodies -- because a false positive costs one unused layout and a
false negative is `unknown function \`O$new\`` out of codegen.

**AN api DECLARES AND A COMPONENT DEFINES**, so only an api's declarations are
marked merged at all. `compiler_regressions/object_from_diff_component.fss`
imports a `.fss` and constructs what it finds there; its own comment calls
cross-component construction the thing it exists to assert.

Constructing a merged object that was NOT lowered is refused by name:
`MergedObjectNotConstructible`, above the argument check, because the reason is
the object and not the call.

## What it cost

ONE file: `Library/CompilerAlgebra.fss`, on `` `=` is ambiguous `` for a pair of
`Just[\(Reduction[\(Number,Number)\], ...)\]` instantiations.

**THIS SECTION SAID "ANOTHER COMPARISON MEET RULE GAP" AND THAT WAS WRONG.**
Corrected 2026-08-23 after the probe was run rather than the chain reasoned out
-- the same mistake, and the same correction, as the `NN32` case. It is not in
the Comparison hierarchy, it is not a missing declaration, and it is not a
source defect. BOTH COLLIDING CANDIDATES CARRY THE SAME SPAN: `Library/
CompilerAlgebra.fss:26`, `opr =(self, other: T): Boolean = (self === other)`,
monomorphized at `T = AnyMaybe` and at `T = AnyUniqueItem`. One declaration,
two stamps.

And the meet the Meet Rule asks for IS ALREADY WRITTEN:
`Library/FortressLibrary.fsi:896` declares `opr =(self, other:AnyMaybe):
Boolean`, which dominates both stamps. It is not in the component's overload
set, because **Rule 3 above filtered it out** -- a merged functional method is
not lifted into a component -- while monomorphization stamps the file's OWN
generic trait at the merged types and those stamps do enter. So a component sees
the obligations its own generic creates and none of the merged declarations that
discharge them. The whole arity-2 `=` group is thirteen entries and every one is
a stamp of that one line.

That is a real compiler defect and it is Rule 3's cost, stated. It is NOT fixed
by lifting merged methods on their own: `typing_candidates` prefers concrete and
`dispatch_target` takes `applicable(.., true)`, so a bodiless meet can never beat
a concrete stamp. Whatever fixes it has to answer that too. Recorded in
`04-state.md`; not built here.

## The IR moved, and it is tags

150 of 395 files' IR differs, and a representative diff is six lines: `i32 1`
became `i32 91`. Merged declarations take tags ahead of the file's own. Nothing
else changed, and the oracle proves behaviour by BUILDING AND RUNNING 421
binaries rather than by comparing text -- pass went 344 -> 345.

## Gate

Five mutation rows, one per rule plus the accessor set. `implicitbuiltin.fss` is
rewritten: it used to assert the component half was OUT and now asserts it is
IN, built and RUN, with a literal, a `String` and a `||` in it because the whole
risk is a merged declaration shadowing a builtin. `badmergedfunction.fss` and
`badmergedconstruct.fss` are the refusals.
