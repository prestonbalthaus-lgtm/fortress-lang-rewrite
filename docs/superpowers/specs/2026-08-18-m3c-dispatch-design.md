# Fortress M3c: traits and symmetric multiple dispatch

Date: 2026-08-18
Status: **design approved, implemented on `m3c/dispatch`. See "As built" at the end.**

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

---

## As built

Approved and implemented on `m3c/dispatch`. Four things came out different from
the design above, and one prediction in it was simply wrong.

**The direct-call rule in §2 is not what got built, because as written it is
unsound.** "If the static argument types leave exactly one applicable
declaration, the call site emits a plain `call`" fails on `f(x: Alpha)` and
`f(x: TraitA)` called with a `TraitA`: only `f(TraitA)` is applicable to
`TraitA` itself, but the cell `(Alpha)` has both applicable and `f(Alpha)` wins
it. Binding statically would call the wrong declaration. What is implemented is
the §-1 sentence taken literally: **build the table, then let the collapse
decide**. A table whose cells all name the same winner is a leaf, and a leaf is
the direct call. Same outcome for every call that was direct before, and correct
for this one. `tests/specificity.fss` is that case, and the gate prints `1 2 1`
where a static binding prints `2 2 1`.

**The statically computed return type is required to exist, which is stricter
than raw cell enumeration.** `f(x: A)`, `f(x: B)`, called at a static `Top`
whose concretes are `OA` and `OB`: every cell has a winner, but nothing applies
to `Top` itself. That is refused, with `no declaration of f applies to (Top)`.
Two reasons: the call has to be statically well typed to have a return type at
all, and the same fact is what makes the table total. A declaration applicable
to the static tuple is applicable to every concrete tuple beneath it, so no cell
can be empty and the `fail` arms really are unreachable. `tests/ambiguous.fss`
carries a covering `pick(Top, Top)` for exactly this reason.

**Field initializers are restricted, and this is new.** A body field
`f: T = expr` may not reference a singleton, call a user function, or construct
another object. Without that, a singleton whose initializer reaches a singleton
declared later loads a null global, and two objects whose initializers construct
each other recurse until the stack goes. Both are segmentation faults out of
ordinary user source, which is the one outcome this compiler does not ship.
Constructor parameters are unrestricted -- the caller builds those values -- so
the restriction costs nothing that matters yet.

**`println` now refuses what there is no shim for.** It used to accept any type
and fail in codegen with an internal error for an array. It is a diagnostic now.

**The corpus prediction was wrong, and by a lot.** The design said M3c would
move the parse metric "roughly 562 files". It moved it from 52 to 84. What
actually moved was the *lexer*: `{` and `}` were not tokens at all, and adding
them took 939 of 1956 files to 1277 (48.0% to 65.3%). The parser's first
blockers are now modifiers in front of a declaration (`native component`, 295),
`import` (169) and static parameters (138) -- none of which the 562 estimate
accounted for, because it counted first blockers on the 939 files that lexed
before rather than the 1277 that lex now. Generics remain the cliff.

### The gate refused

`tools/dispatch-gate.sh` is 19/0 with a 9/0 self test, and
`./tools/dispatch-gate.sh --mutate` breaks the compiler three ways and requires
each break to fail it. All three were run:

| mutation | what it did |
|---|---|
| invert the specificity comparison (`strictly_below(&a, &b)` to `strictly_below(&b, &a)`) | matrix `3000 2000 1000 4000` became `1000 1000 1000 1000`; `specificity.fss` `1 2 1` became `2 2 2`; the table collapsed so completely that **no switch was emitted at all** (switches=0). 6 checks refused. |
| drop the last case from every switch | `fortress: no declaration of draw for argument 1 with type tag 4`, **status 1** -- a clean halt with a diagnostic, not a fault, which is the entire reason the `fail` arm exists. 2 checks refused. |
| accept a tie instead of reporting it (`if maximal.len() != 1` to `if false`) | `ambiguous.fss` compiled, status 0. Exactly 1 check refused, and it was the ambiguity check. |

3 run, 0 survived, 0 could not be applied.
