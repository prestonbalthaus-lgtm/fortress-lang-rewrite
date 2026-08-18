# Fortress M3d: generics by monomorphization

Date: 2026-08-19
Status: **design, for review. Nothing implemented.**

Monomorphization, not erasure and not boxing: concrete copies stamped out at
compile time, so an `Array[\ZZ64\]` stays a block of `i64` and never becomes a
block of pointers to boxes.

## The measurement first, because it changes what M3d is sold as

M3c predicted 562 corpus files and delivered 32. So this time the estimate is an
experiment, not a count. Every depth-0 `[\ ... \]` span was erased from all 737
bracket files with a comment- and string-aware scanner — simulating generics
that parse perfectly and cost nothing — and the real driver was re-run on each.

**Ten files got past the parser.** Zero of the 737 parse today, so M3d moves the
parse metric from 84/1956 to about 94, and the number that compile end to end by
**one, and only if `nat` is in scope** — the single survivor that runs is
`ProjectFortress/tests/instantiateNatParam.fss`.

The first run of that experiment said eight, and the error is worth keeping:
erasing a span to spaces preserves byte offsets but destroys token **gluing**, and
this parser decides application from adjacency, so `O[\P\]()` erased becomes
`O` + spaces + `()`, which is a juxtaposition and not a call. The measurement had
the same class of bug the milestone is meant to avoid.

The wall is not generics. **319 of the 737 die in the lexer** — 168 on `|`, 72 on
`=>` — and `import` blocks 529 files across the corpus. Every load-bearing
Library file (`List`, `Map`, `Set`, `FortressLibrary`, `RangeInternals`) is a
lexer casualty, not a generics casualty.

One more thing the measurement says about scope: **static arguments bind harder
than static parameters.** 104 of the 722 files put a numeral or arithmetic inside
brackets while only 89 declare a non-type parameter. `Library/List.fss` declares
nothing but plain type parameters and still writes
`PrimImmutableArray[\E,0\]`, so a type-parameters-only subset does not parse
`List` either.

**M3d is justified by language completeness, not by corpus movement.** A language
without generics is not the language. But if the goal on any given week is the
metric, `|`, `=>` and `import` are cheaper and worth more.

## The observation the whole design rests on

Specification 1.0, `basic/overloading.tex:100-108`, restated at
`advanced/overloading.tex:95-103`:

> Although there may be multiple declarations with the same functional name, it
> is an error for their static parameters to differ (up to alpha-equivalence),
> or for one declaration to have static parameters and another to not have them.
> Hence, static parameters do not enter into the determination of which
> declarations are applicable.

Read the line immediately above it in the same file, because it is load bearing:

> `\note{This restriction will be relaxed.}`

The specification's own authors marked this rule as temporary. M3d rests on it
anyway, deliberately, and that is the design's single largest structural risk:
if the rule is ever relaxed, candidate growth comes back and the phase split in
§3 stops being sound. It is named here rather than discovered later.

So an overload set is **uniformly generic or uniformly ground**. Monomorphizing a
uniformly-generic set at a static-argument tuple produces a fresh, disjoint
ground set of the same cardinality under a distinct mangled name. It can never
add a member to a pre-existing set.

That kills *candidate growth* — an instantiation appearing inside an
already-decided overload set and flipping a winner — by construction. It is the
nastier of the two ways monomorphization can disturb M3c, and 1.0 hands it to us
for the price of enforcing a rule it already states.

And with static arguments written rather than inferred, instantiation demand is
a **syntactic** property of the source. Then monomorphization is an AST-to-AST
pass that runs to a fixpoint **before `Checker::new` is constructed**, and what
the checker receives is a ground component containing zero generic declarations —
indistinguishable from hand-written M3c source.

## 1. Parsing and the AST

`TypeRef` carries one optional argument today. It becomes a list, and the list is
**not** `Vec<TypeRef>`: a static argument list mixes kinds (`Matrix[\T, nat s0, nat s1\]`),
so it is `Vec<StaticArg>` over an enum. Getting that wrong is cheap now and
expensive at M3e.

```
StaticParam { name, kind: Type | Nat | Int | Bool | Opr | Unit, bounds: Vec<TypeRef> }
StaticArg   { Type(TypeRef) | Nat(u64) | Bool(bool) | Opr(String) }
```

Four things the change surface actually needs, all verified in the code:

* `reject_static_parameters` is called only from `trait_decl` and `object_decl`.
  Generic **functions** currently fail as ``expected `(` ``, so the blocker
  histogram attributes 3,018 generic function and method declarations to the
  wrong cause. The real parse surface is larger than `StaticParametersUnsupported`
  suggests.
* Use-site instantiation lives in expression position — `empty[\ZZ64\]()`, 3,558
  sites across 390 files — and `postfix` has no `LGeneric` arm. `call()` only
  ever sees a bare `Expr::Var` callee, so static arguments have nowhere to
  arrive. New node: `Expr::Instantiate { callee, args }`.
* Bounds live in the bracket position **or** in a `where` clause, and
  `skip_where` currently brace-counts and discards. Both positions merge into
  `StaticParam.bounds`. (`where {` is only 21 real uses in 15 files — the other
  114 `where` tokens are prose inside comments.)
* `nat`, `int`, `bool`, `unit`, `dim`, `opr` are `Kind::Reserved`, and
  `Parser::identifier` turns a `Reserved` into a hard error. Something inside the
  brackets has to accept reserved words before any static parameter list parses.

## 2. The pipeline

Eager, whole-component, and **before the checker**:

1. Collect every written static-argument list. That is the seed set; demand is
   syntactic, so no type information is required to find it.
2. Worklist to a fixpoint, memoised on `(declaration index, canonical argument tuple)`.
   Instantiating substitutes the arguments through the declaration's body,
   its `extends` clause and its field types, discovering further demand.
3. Emit each instantiation **at its generic declaration's position** in `decls`,
   ordered by canonical display name from a `BTreeMap`.
4. Hand the ground component to `Checker::new` unchanged.

Step 3 is not cosmetic. Tags come from `registry.concrete.len()` in source order,
and `registry.rs:34-36` says in as many words that this is what keeps the emitted
module deterministic — switch arms follow tag order. Worklist discovery order is
not a property of the source text. Emitting at the declaration's position, sorted
by name, makes declaration order a pure function of the source again.

## 3. Interaction with M3c multiple dispatch

**The hazard, verified rather than assumed.** `dispatch_target` builds its domain
from `registry.concretes_below` and memoises the finished `DispatchFn` with
`.entry(symbol).or_insert_with(...)`, and it is called from `user_call` *during
body checking*. `registry.concrete` is written in exactly one place,
`Checker::new`. Today the freeze happens before any table is built and the
invariant holds by accident of ordering. The moment instantiation can append a
concrete type during body checking, an instantiation that lands under a trait an
earlier table already switched on leaves that table with no arm for it — and
`tree` only emits arms for candidates present in the domain, so the missing tag
falls into the `fail` arm and calls `fortress_dispatch_failed`, `exit(1)`, at run
time, on a program the checker approved. That is precisely the guarantee M3c was
built and gated to make.

**The answer is the phase split.** Expansion completes before `Checker::new`, so
the world is closed before the first tag is assigned and every table is built
exactly once from a domain that can never widen. Zero lines of the dispatch code
change.

Reachability is what makes the split legal: closing the instantiation set never
requires deciding a winner, because `applicable` is pointwise `is_subtype` and
nothing else. The loop closes over (concrete types × instantiations) alone, and
dispatch tables sit in a stratum above it. Julia closes the same loop by
invalidating and recompiling, which needs a JIT; building tables incrementally
here would be re-implementing world age with the recompilation step deleted.

A generic member of an overload set does not exist after expansion — `f[\ZZ64\]`
and `f[\String\]` are separate mangled symbols in separate sets. A source program
that mixes generic and ground declarations under one name is refused with
`OverloadSetStaticParamsDiffer`, naming both.

Worth recording: `Papers/Dispatch/body.tex:371-373` explicitly excludes
parametric traits and objects from the POPL dispatch model M3c is built on, and
lists them as future work. There is no reference answer to graft. This is ours.

## 4. Scope boundary

| | |
|---|---|
| **Implemented** | type static parameters of any arity on trait, object and top-level function declarations; nested static arguments (`Map[\ZZ64, List[\String\]\]`); use-site instantiation; upper bounds including F-bounded ones, checked structurally at instantiation; eager whole-component expansion |
| **Parsed, not checked** | `where` clauses, `nat`/`int`/`bool`/`opr`/`unit` static parameters, `covariant`/`contravariant` |
| **Out** | static-argument **inference**; generic arrays; `dim` (0 files in the corpus); `NatReflect`'s runtime-integer-to-`nat` trick |

**Static-argument inference is out, and that is what keeps M3d small.** Explicit
instantiation is what makes demand syntactic, which is what lets expansion run
before the checker, which is what leaves M3c untouched. It is also exactly what
M3e has to undo: with inference, demand depends on inferred types, inferred types
come from the checker, and the checker's tables come from the closed world that
expansion was supposed to have closed. That is the genuine fixpoint, and it is a
milestone of its own.

**Generic arrays are out for a measured reason, not an aesthetic one.**
`fortress_array_alloc` fills every pointer slot with a static `""`
(`runtime/shims.c:149-154`), an invariant that is only correct for `String`. An
`Array[\SomeObject\]` slot read before it is written hands dispatch a tag load
four bytes into a one-byte `.rodata` object. `Elem` also stays the closed
five-scalar enum and is `const fn` throughout, which an interned instantiated
name cannot satisfy. Fix the fill policy first, on its own, with its own gate.

## Termination, which is bought and never proven

Deciding whether an arbitrary program's instantiation set is finite is
undecidable. Four mechanisms, in the order they do work:

1. **The memo table**, keyed on `(declaration, canonical argument tuple)`, and it
   is the entire answer to F-boundedness. `trait Equality[\T extends Equality[\T\]\]`
   at `T := ZZ32` demands `Equality[\ZZ32\]`, which is the instantiation already
   being constructed: a hit on the first lookup, fixed point in one step. Counts
   of F-bounds range 435–790 depending on whether you count declaration sites,
   occurrences, or `.fss`/`.fsi` duplicates — but the number that matters was
   measured three times and agrees every time: **expansive F-bounds = 0.** No
   bound in the tree nests its own parameter under another type constructor.
   **A "no self-referential bounds" rule therefore defends against nothing and
   rejects `Equality`, `Integral`, `StandardTotalOrder` and `List`, which is the
   whole prelude.**
2. **An instantiation depth ceiling**, with the chain in the diagnostic.
3. **A type size ceiling**, not redundant with depth: *n* nested `Pair[\T,T\]`
   produces a type of size 2^*n* at depth *n*. rustc carries both.
4. **A total instantiation ceiling, and it is mandatory rather than a backstop.**
   Depth and size are both insufficient: an *acyclic* call DAG of *k*+1 distinct
   declarations, with no recursion and no cycle anywhere, where each level hands
   its callee two **different** wrapper types, produces 2^(*k*+1)−1 distinct
   instantiations at depth *k* with every type small. "Bounded by the source text
   except through recursion" is false, and this is the counterexample that shows it.
5. All three refuse with a diagnostic rather than hanging — the same call as
   `MAX_DISPATCH_CELLS` and as refusing an unrecognised `--target-cpu`.

**Name the casualty rather than calling the cap theoretical.** The corpus contains
real polymorphic recursion in shipped library code: `Library/PureList.fss:137`
calls `arrayToFingerTree[\D23[\E\]\]` from inside `arrayToFingerTree[\E\]`. **A
monomorphizing compiler cannot compile `PureList` as written, at any limit.**

## Deviations from 1.0

1. **The uniformity rule is enforced as a hard error** — and 1.0 marks it
   "will be relaxed", so this is a deviation from where the specification was
   heading, not from where it stood. The legacy
   implementation deliberately broke it and 22 corpus files depend on
   runtime-inference dispatch over static parameters
   (`ProjectFortress/other_compiler_tests/IGO1.fss` documents its own model in a
   comment). Enforcing it is what makes candidate growth impossible.
2. **Static arguments must be written.** 1.0 infers them. Deferred to M3e.
3. **Some programs 1.0 accepts are refused.** `PureList` is the named one.

For context on what we are not inheriting: the legacy implementation
monomorphized *lazily at JVM class-load time* through a custom classloader —
5,021 of 7,871 lines in `runtimeSystem/` are that machinery — and everything on
the JVM was boxed. The only unboxed arrays in the legacy tree are hand-written
monomorphic special cases, which is exactly what monomorphization gives for free.

## How it gets gated

`tools/generics-gate.sh`, and it is not believed until it has refused.

* A **determinism check**: build the same source twice and compare the object
  bytes. Tag order is the guarantee most likely to rot silently.
* An instantiation matrix: one generic function over several argument tuples,
  each instantiation returning a distinct value, checked against a table the gate
  computes rather than reads out of the program.
* Mutations, each expected to be caught: break the memo table (expect the depth
  cap to fire rather than a hang); assign tags in worklist discovery order
  (expect the determinism check to refuse); drop the uniformity check (expect a
  mixed generic/ground overload set to compile and dispatch wrongly).
* A negative fixture that must be **refused**: `PureList`-shaped polymorphic
  recursion, with the instantiation chain in the diagnostic.

## The review asks

Five decisions are the architect's, not the implementer's. Nothing gets written
until they are answered.

1. **Enforce the uniformity rule?** It is what makes candidate growth impossible
   and therefore what makes the phase split sound. 1.0 states it and marks it
   "will be relaxed"; the legacy implementation broke it deliberately. Enforcing
   is recommended. Declining means M3d owns a genuine fixpoint over dispatch
   tables, and that is a different and much larger milestone.
2. **Is `nat` in or out?** This is the only place the corpus has an opinion, and
   it is exactly the difference between +1 and +0 files compiling end to end.
   `nat` with **literal** arguments only — no arithmetic — costs one extra
   `StaticArg` variant and no inference. `Array1`/`Array2`/`Array3` in
   `FortressLibrary` are the reason it exists.
3. **What is `MAX_INSTANTIATIONS`?** The acyclic-DAG counterexample makes a total
   ceiling mandatory. A number has to be picked rather than derived; 65,536 is a
   defensible start. Say the number in the spec so the diagnostic can quote it.
4. **Is `PureList` declared out of scope in writing?** `Library/PureList.fss:137`
   is genuine polymorphic recursion in shipped library code, not a pathological
   fixture. No monomorphizing compiler compiles it at any limit. Better named in
   the spec than discovered by the gate.
5. **What does `array(n)` do for object elements** if `Elem` ever widens? The
   current pointer fill is only correct for `String`. Refuse, null-plus-check, or
   require a fill value — decided, not shrugged at.

One implementation note that is not an architect question but will bite whoever
starts: `dispatch_target` returns `Target::UserFn` on two paths — the
single-candidate early return and the post-tree collapse — and `Apply` stores its
`Target` **by value**. So a call site's symbol is a pure function of
`(name, static args)` but its `Target` is not, and any scheme that defers
dispatch decisions past the freeze has to say how the `Target` gets back into the
already-built typed AST.
