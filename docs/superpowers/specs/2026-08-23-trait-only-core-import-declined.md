# The trait-only component-side core import: authorised, measured, declined

**Date:** 2026-08-23. **Result: NOT LANDED.** Authorised on the condition that
it "unlocks the 69 blocked files without causing the wholesale implicit import
regression". It causes exactly the same regression: **520 -> 118, 402 lost, zero
gained.** Reverted; the tree is unchanged.

What the investigation DID produce is the real mechanism, which is not the one
recorded in `resolve.rs`'s own comment.

---

## Three variants, all measured at the tip, all reverted

| variant | total | lost | gained | exit 70 |
|---|---|---|---|---|
| baseline | **520** | -- | -- | 0 |
| full merge component-side | 118 | 402 | 0 | 221 |
| **traits only** | 118 | 402 | 0 | **0** |
| traits only, minus the builtin type names | 118 | 402 | 0 | 1 |

The trait-only run is the one that matters and its exit-70 column is the tell:
zero internal errors, 402 clean `unknown type` diagnostics.

## Why trait-only fails, and it is not tags or singletons

`resolve.rs` says the component half is out because a merged OBJECT takes a
32-bit type tag and a merged SINGLETON is constructed in `main`. That reasoning
is sound and **it is not what blocks this**. Every one of the 402 trait-only
losses is a clean `unknown type`, and the first one every program hits is:

```
Documentation/Specification/Code/HelloWorld.fss:20:4: unknown type `NoneObject`
```

`CompilerBuiltin.fsi:649` declares `trait Option[\E19\] ... comprises {
NoneObject[\E19\], Some[\E19\] }` and `:658` declares
`value object NoneObject[\E21\]`. **A merged trait's topology clauses name
objects.** Merge the traits without the objects and every `comprises` and every
`extends` in the core library dangles.

Skipping the nine `BUILTIN_TYPE_NAMES` instead moves it one layer along -- 401
files then report `X is not a trait, so nothing can extend it`, because the
library's own `trait String` and `trait Boolean` were the ones skipped and other
traits extend them.

## What the FULL merge actually dies of, which is the useful finding

Not tags. **Name collisions between the shipped library's declarations and this
compiler's builtin scalar types.** The 402 losses, by message:

```
 87  an integer literal cannot be used where ZZ32 is required
 40  expected String, found String
 16  X is not a supported array element type
 11  expected ZZ64, found ZZ32
  6  expected RR64, found RR64
```

`expected String, found String` is the whole story in five words: the merged
`trait String` and the builtin `Type::String` print the same and are not the
same type. Reproduced from source in three lines, in the tree:

```
component ZZProbe2
import CompilerBuiltin.{...}
import FortressLibrary.{...}
export Executable
f(): ZZ32 = 3          -- an integer literal cannot be used where ZZ32 is required
run(): () = ()
end
```

## AND A COMPONENT IMPORTING BOTH CORE APIS REACHES AN INTERNAL ERROR

Exit **70**, `internal error: field \`x\` has no storage type`, from a component
whose entire body is `run(): () = ()`. Each import ALONE is a clean exit 1; only
the two together produce it. No corpus file writes both, which is why the "no
corpus file crashes" gate is green and stays green -- but it is reachable from
source and it is a real defect. Recorded in `04-state.md`.

## What this means for link 5

The component-side implicit core import is a MILESTONE, not a flag, and the
milestone is **name resolution**, not codegen. Before it can land, a merged
declaration has to be able to lose to a builtin of the same name -- or the
builtins have to come from the library rather than from
`types::BUILTIN_TYPE_NAMES`. The tag and singleton objections are still true and
still have to be answered, but they are the second problem, not the first.

The 69-file lever is unchanged and still worth it.
