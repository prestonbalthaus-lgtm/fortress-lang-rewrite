# Fortress M3c: traits and symmetric multiple dispatch

Date: 2026-08-18
Status: **design, for review. Nothing implemented.**

The hard one. What follows is the plan, the two places it deliberately departs
from the 1.0 specification, and the smallest subset that clears the milestone.

## The observation the whole design rests on

Specification 1.0 gives three declaration-level rules — Subtype, Incompatibility
and Meet (`Specification/advanced/overloading.tex`) — that a pair of overloaded
declarations must satisfy. They exist to guarantee, *modularly*, that no call is
ambiguous at run time, without anyone having to enumerate the calls.

We are a whole-program AOT compiler. We do not need the modular argument,
because we can do the enumeration:

> For each overload set, enumerate every tuple of **concrete** types that can
> reach its parameters. For each tuple, compute the applicable declarations and
> take the most specific. Require exactly one winner per tuple.

That single computation is both the static ambiguity check and the dispatch
table. It also sidesteps implementing intersection types for the Meet Rule: the
rule is checked extensionally, on the cells, rather than symbolically on the
declarations. The 1.0 rules come back the day `api` and separate compilation
land, because then the enumeration is no longer possible.

Say it plainly: **whole-program knowledge is load-bearing in three places** —
trait exclusion, table construction, and exhaustiveness of the switch.

## 1. Memory layout

An object is one heap block with a 32-bit **concrete type tag at offset 0**,
followed by its fields.

```
  +0   i32 tag        concrete type id, assigned at compile time
  +8   fields...      natural machine layout, pointers included
```

Fields start at +8, not +4: the tag pads out to the alignment of the widest
field, which is 8 for every type the language has.

Not vtables: a vtable answers "given the receiver, which method", which is
asymmetric, and symmetric dispatch asks about a *tuple*. Not fat pointers: they
double the element width of `Array[\T\]` and would reopen the layout M3b just
landed and gated. A header tag leaves arrays, strings and the C ABI untouched,
and Boehm is untroubled by it — a small integer is not pointer-like.

Every object is allocated through `fortress_alloc_scanned`, with no exception
for all-scalar objects: saving a word there re-arms precisely the landmine
`runtime/tests/array_trace.c` exists to catch.

Trait types have no run-time representation at all. A trait-typed value is a
pointer to a concrete object; its trait membership is a compile-time fact about
its tag.

## 2. Static versus dynamic

Everything except a tag load and a switch is static.

**In the types crate, at compile time:**

* The trait hierarchy, transitively closed. Subtyping, and the specificity
  order on parameter tuples (pointwise subtyping, strict in at least one
  position).
* The set of concrete types reaching each parameter of each overload set.
* The dispatch table: one winner per tuple, or a diagnostic.
* **The direct-call decision.** If the static argument types leave exactly one
  applicable declaration, the call site emits a plain `call`. This is the
  overwhelming majority of calls and it costs nothing. Every call written today
  stays exactly the code it is now.

**Deferred to run time, and only here:** a call site where more than one
declaration survives static filtering. It becomes a call to a compiler-generated
dispatch function, costing one load and one switch per dispatched position. The
types crate's existing invariant holds: every call still names one concrete
`Target`, and one of the possible targets is now a dispatch function.

## 3. The dispatch mechanism

`f(x: TraitA, y: TraitB)`, with three declarations, `Alpha extends TraitA`,
`Beta extends TraitB`, and `Gamma`, `Delta` the other concrete types reaching
the two positions. The table is computed at compile time and flattened into a
decision tree, one level per dispatched position:

```llvm
define i64 @f$dispatch(ptr %x, ptr %y) {
entry:
  %tx = load i32, ptr %x                       ; tag at offset 0
  switch i32 %tx, label %fail.x [ i32 1, label %x.alpha
                                  i32 3, label %x.gamma ]

x.alpha:                                       ; the Alpha row needs the column
  %ty = load i32, ptr %y
  switch i32 %ty, label %fail.y [ i32 2, label %alpha.beta
                                  i32 4, label %alpha.delta ]

alpha.beta:                                    ; (Alpha, Beta)
  %r0 = call i64 @f$Alpha_Beta(ptr %x, ptr %y)
  ret i64 %r0
alpha.delta:                                   ; (Alpha, TraitB)
  %r1 = call i64 @f$Alpha_TraitB(ptr %x, ptr %y)
  ret i64 %r1

x.gamma:                                       ; the whole row has one winner,
  %r2 = call i64 @f$TraitA_TraitB(ptr %x, ptr %y)   ; so no second switch
  ret i64 %r2

; One fail arm per switch. A single shared one would use %ty on a path where
; it is not defined, and the module would not verify.
fail.x:
  call void @fortress_dispatch_failed(ptr @f$name, i32 0, i32 %tx)
  unreachable
fail.y:
  call void @fortress_dispatch_failed(ptr @f$name, i32 1, i32 %ty)
  unreachable
}
```

A row whose winners are all the same collapses, so the tree is usually
shallower than the arity. Every leaf is a **direct call**, so callees stay
ordinary inlinable functions. The `fail` arms are statically unreachable and
exist because "unreachable" should mean a clean halt with a diagnostic — the
same choice as the array bounds check — not undefined behaviour. There is one
per switch rather than one per function: the shim takes the failing parameter
position and the tag found there, which is the only tag in scope at that point.

Table size is O(|concrete types|^k) for k dispatched positions, nothing at the
scale of one component, and only trait-typed positions dispatch. Per cell the
winner's return type must be a subtype of the statically computed one: the
Subtype Rule's covariance condition, checked cell by cell.

## 4. Scope boundary

Parsing the corpus and type-checking it are different bars, and M3c should take
the first without pretending to the second.

| | |
|---|---|
| **Implemented** | `trait T extends {A, B} end`; `object O extends {T} f:Type ... end` with immutable fields and field access; singleton objects; top-level overloaded functions; multiple dispatch over all of it |
| **Parsed, not checked** | trait method signatures and default bodies, dotted methods, `comprises`, `excludes`, `where`, `var` fields, `api` declarations |
| **Out** | static parameters (generics); scalars implementing traits; coercion; operator methods; object expressions |

Dotted methods are out on purpose: the specification gives dotted and functional
methods separate namespaces with their own shadowing rules, and desugaring
`x.f(y)` to `f(x, y)` conflates them into semantics that would have to be
unbuilt. Scalars implementing traits is out because it forces boxing, which
touches every allocation path M3a and M3b just landed.

**Generics are the real cliff, and M3c does not approach it.** 687 of the 1790
corpus files use `[\...\]`, and `Library/` is F-bounded throughout
(`trait Equality[\T extends Equality[\T\]\]`). So state the expected result
honestly: M3c moves the **parse** metric a lot — roughly 562 files whose first
blocker is one of `api`/`component` (291), `trait` (147), `object` (107) or
`var` (17) — and the **typecheck** metric barely at all. Generics are M3d and
they are where the corpus actually opens.

## Two deliberate deviations from 1.0

**Ambiguity is a compile error.** 1.0 says that if no single most specific
declaration exists, "any of the applicable declarations such that no other
applicable declaration is more specific than them is chosen"
(`Specification/basic/overloading.tex`). An arbitrary winner is a silently wrong
answer, and this compiler refuses those on principle — the same call as
rejecting an unrecognised `--target-cpu` rather than letting LLVM build the
baseline. A tied cell is a diagnostic naming the tuple and both declarations.

**Exclusion is closed-world.** Two traits exclude when no concrete type in the
program extends both. 1.0 has to be conservative, because a later component
could introduce one. We compile the whole program, so we can be exact. This is
the assumption that dies first when `api` arrives, and the 1.0 declaration rules
are what replaces it.

## How it gets gated

`tools/dispatch-gate.sh`, and it has to refuse before it is believed.

* A dispatch matrix: every concrete tuple called, each declaration returning a
  distinct value, compared against a table the gate computes itself rather than
  reads out of the program.
* A symmetrically ambiguous pair — `f(A, Any)` and `f(Any, B)` called with
  `(A, B)` — must be **rejected at compile time**, naming both declarations.
* Mutations, each expected to be caught by exactly one check: invert the
  specificity comparison; drop a case from a switch and prove
  `fortress_dispatch_failed` halts cleanly rather than falling through.
