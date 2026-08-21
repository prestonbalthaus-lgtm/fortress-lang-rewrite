# D5. The component algebra: separate compilation, in or out

**Decision: the component algebra is CUT from v1. Api-based name resolution stays.
The compilation unit is the WHOLE PROGRAM, not the component.**

Status: **drafted, not adopted.** Written against master `f81f41ace` on 2026-08-21.
Every measurement below was reproduced by hand with a sha256-pinned driver
(`7e103205cb54`), not quoted.

This is the decision M3c and M3d are already built on without anyone having taken
it. Both are already correct under it, so the cost of taking it now is zero and
the cost of taking the other branch rises every milestone.

---

## 1. What is actually being decided

`Specification/basic/components/` is 10 `.tex` files and 2,011 lines, plus
`advanced-lib/components.tex` (the reflective `Fortress.Components` api) and
`appendices/components.tex` (the calculus). It specifies a **persistent component
database** — the spec calls it *a fortress* — with these operations
(`basic.tex:32,76,171,248,419,558,580`, `advanced.tex:63,89,105,183`):

| Operation | Signature as the spec gives it |
|---|---|
| compile | `compile(file:String):()` |
| link | `link(result:String, constituents:String...):()` |
| execute | `execute(componentName:String, args:String...):()` |
| upgrade | `upgrade(target:String, replacement:String, result = target):()` |
| extract / install | — |
| uninstall | `uninstall(file:String):()` |
| constrain / hide | — |

Plus per-component upgrade policy: a component may export the api `Upgradable`
and define `isValidUpgrade(that:Component):Boolean` and
`upgrade(that:Component):Component` (`basic.tex:276-303`), which the fortress
calls during an upgrade.

**Two things are being conflated everywhere and they are not the same feature.**

- **(A) Api-based name resolution.** A component `import`s an api; names cross a
  file boundary; the compiler resolves them. `overview.tex:26-33`: *"components
  never refer to other components directly; all external references are to
  apis."*
- **(B) The component algebra.** The persistent database, link/upgrade/hide/
  constrain, encapsulated constituents, and the reflective api that hands a
  running program a `Component` object.

**Phase 3's exit criterion requires (A) and none of (B).** Decision 4's "v1 is
the complete 1.0 specification minus syntax abstraction" literally includes (B).
Neither reading is written down and they disagree. That is the whole of D5.

---

## 2. The evidence, measured

**(A) is the critical path. 494 of 1956 corpus files carry a real Fortress
`import`** (1,427 have none; 35 have only `import java <foreign>`). Top imported
api names: List 167, Set 80, Map 66, File 48, FileSupport 40, FortressAst 37.
Nothing about phase 3 is optional.

**(B) has ZERO witnesses of any kind.**

- **No corpus program uses it.** The operations are shell commands typed at a
  fortress, not source syntax, so no `.fss` file *can* witness them. Grepping for
  them as source finds three false positives — `Library/Heap.fss:270` and
  `ProjectFortress/BirdyLib/Maybe.fss:35` declare ordinary functions named
  `link` and `extract`.
- **The reflective api does not exist as Fortress source.** `Fortress.Components`
  appears in no `.fss` or `.fsi` file in the tree. It exists only as
  `%`-commented LaTeX inside `Specification/advanced-lib/components.tex` — 76 of
  its 180 lines are commented out. Implementing decision 4 literally here would
  start by *writing the api declarations*, from scratch, with no test to run them
  against. That is the same shape as `Fortress.Operators.fsi.INCOMPLETE` in D7.
- **The oracle has no case for it.** None of the 373 `.test` files exercises
  link, upgrade, hide or constrain — they drive `compile`, `link` (the *object*
  link), and `run` on single sources.

So (B) is 2,011 lines of specification with no implementation, no library
declarations, no corpus program and no recorded expected behaviour. It is the
only decision-4 item in that position.

---

## 3. The decision

### 3.1 CUT the component algebra from v1

The persistent fortress database, `link`/`upgrade`/`extract`/`install`/
`uninstall`/`constrain`/`hide`, component encapsulation with per-component
constituent copies, the `Upgradable` api, and the reflective `Fortress.Components`
api are **out of v1**. This is a **named deviation from decision 4**, of the same
kind as M3c's two signed-off deviations, and it must be recorded in the ROADMAP's
"Out of scope for v1" section — which today names Eclipse, Emacs, Fortify, Vim
and `contrib/` and nothing else.

Rationale, in order of weight:

1. **It has no acceptance criterion and cannot acquire one.** Every other v1 item
   has corpus witnesses to differential-test against. This has none, and the
   `.test` oracle has no case for it. "Done" would be unfalsifiable.
2. **It is a deployment and packaging system, not a language feature.** Nothing
   in the type system, the dispatch semantics or the generated code depends on
   it. Cutting it removes no expressiveness from any program anyone has written.
3. **The HPC target does not want it.** Fortress binaries run under Slurm inside
   an Apptainer image. A persistent mutable component database on a compute node
   is the opposite of a reproducible batch job, and Apptainer images are
   immutable by design.
4. **Keeping it re-opens M3c and M3d** — see §4.

### 3.2 KEEP api-based separate compilation, and it is phase 3 unchanged

`import`, `export`, api resolution, the source path, qualified names, and
component-satisfies-api conformance are all **in v1** and unaffected by this
decision. Phase 3's exit criterion stands as written.

### 3.3 THE COMPILATION UNIT IS THE WHOLE PROGRAM

This is the operative half of D5 and the part M3c and M3d actually need.

**A Fortress *component* is a unit of SOURCE organisation. It is not a unit of
CODE GENERATION.** `fortressc` takes the root source plus everything reachable
through its imports along the source path, resolves the whole set, and emits
**one** object. There is no `.o` per component, no archive of components, and no
linking of two `fortressc` outputs.

That is what preserves the closed world:

- **M3c dispatch is closed-world** — every tuple of concrete types reaching an
  overload set is enumerated whole-program, and that one computation is the
  ambiguity check, the dispatch table *and* the exhaustiveness proof.
- **M3d is whole-program monomorphization**, with `registry.concrete` and every
  32-bit type tag frozen at `Checker::new`
  (`crates/types/src/lib.rs:49-50`).

Under whole-program compilation both remain sound with **no change**. Under
component-at-a-time compilation to relocatable objects, both are wrong: a tag
assigned in one compilation cannot be reconciled with a tag assigned in another,
an overload set cannot be enumerated when a future object may add a concrete
type to it, and `MAX_INSTANTIATIONS = 4096` becomes a per-object limit on a
whole-program quantity.

**So the sentence to put in the ROADMAP is not "separate compilation is cut" —
it is "the closed world is the transitive import closure of one `fortressc`
invocation."** Everything M3c and M3d assume follows from that, and the
component algebra is cut because it is precisely what would break it.

---

## 4. What this commits, and what re-opens it

**Committed by this decision:**

- M3c's closed-world exclusion and M3d's monomorphization stay as they are. No
  re-opening, no boxing, no runtime type descriptors.
- `MAX_INSTANTIATIONS` remains a whole-program budget, which is what its comment
  already says.
- Incremental and cached compilation are out of scope for v1 as a consequence,
  not as a separate decision. One invocation, one object, every time.

**What re-opens it, and each of these is a real trigger, not a formality:**

1. **Compile time on a real program.** Today the largest source that has ever
   compiled end to end is **1,670 bytes** and the whole compiling set is 162 KB.
   Two whole-program algorithms have only ever seen toys. If whole-program
   compilation of the actual `Library/` turns out to be super-linear enough to
   hurt, the answer is caching within one invocation first, and only then
   separate compilation — and separate compilation costs M3c and M3d.
2. **A user demanding a shipped binary library.** Not a v1 requirement and
   nobody has asked.
3. **`MAX_INSTANTIATIONS` being hit by honest code.** That is a signal the
   whole-program budget is wrong, not that the world should open.

---

## 5. What is a prerequisite EITHER WAY, and is not done

Whatever D5 says, the backend cannot currently produce two objects that coexist.
Reproduced by hand at `f81f41ace`:

```
$ nm liba.o | grep -v ' U '        $ nm obj.o | grep -v ' U '
0000000000000000 T helperA         0000000000000050 T main
0000000000000030 T main            0000000000000040 T N$m$bump
0000000000000010 T run             0000000000000000 T N$new
                                   0000000000000020 T run
$ cc liba.o libb.o -o both -lgc -lm
ld: multiple definition of `run'
ld: multiple definition of `main'
```

Three facts follow, and all three are work that D5 does not remove:

1. **Top-level functions are bare, unmangled, external globals.** `helperA` is
   `T helperA` in the flat C namespace. Two components each declaring `helper`
   collide, and so does any Fortress function whose name matches a libc symbol.
   Members *are* mangled (`N$m$bump`, `N$new`), so the scheme exists — it just
   does not cover top-level declarations and carries no component qualifier.
2. **Every object carries `main` unconditionally.** A component with no `run()`
   and no `export Executable` still emits `T main`:
   `component onlylib; helper(x: ZZ32): ZZ32 = x + 1; end` → `T helper`,
   `T main`. Entry-point emission must become conditional on the component
   exporting `Executable`.
3. **The component name is in no symbol.** There is no qualifier available to
   disambiguate with even if one were wanted.

**Under this decision these are internal hygiene, not an ABI.** Because there is
exactly one object and it is never linked against another `fortressc` output,
the mangling scheme is private and can change freely. Recommended, in this order,
and none of it is urgent:

- give every generated symbol **internal linkage** except `main` and the runtime
  shims — that closes the libc collision and the whole class at once;
- emit `main` **only** when the root component exports `Executable`;
- qualify top-level functions the way members already are, so a diagnostic that
  quotes a symbol names the declaration a reader can find.

Anything that instead treats these symbols as a stable interface between objects
is building the branch this decision cuts.

---

## 6. What this document does NOT decide

- **The source path and collision policy** — ten api names exist in more than one
  directory and they are *different libraries*. That is `SPIKE-IMPORT-RESOLUTION`
  and the legacy answer is on disk at `default_repository/configuration:44`.
- **Component-satisfies-api conformance** (`source-code.tex:313-320`). In v1,
  gated on varargs, and unaffected by this decision.
- **Whether `component` must match the file name.** Today nothing compares them
  and a headerless file compiles with an empty component name. That is a
  §5.2 hollow-construct item, not this one.
- **`native component`.** Three bootstrap files (`Library/CompilerSystem.fss`,
  `ProjectFortress/LibraryBuiltin/{CompilerBuiltin,System}.fss`) reach the JVM
  through `import java <foreign>` and have no other implementation in the tree.
  Those bodies are **C-shim work**, not component-algebra work, and cutting the
  algebra does not touch them.
