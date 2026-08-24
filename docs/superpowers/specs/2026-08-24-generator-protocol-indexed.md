# The generator protocol, built: EXTERNAL INDEXED iteration over `Indexed`

**2026-08-24, Phase E.** The 2026-08-22 measurement
(`2026-08-22-generator-protocol-measured.md`) priced this milestone at ZERO
corpus files and recommended sequencing it after tuples and after a first-class
`Reduction`. Tuples landed 2026-08-22; `Reduction` did not. This document records
what changed, what was built instead of 1.0's `generate`, and the honest payoff.

## 1. What 1.0's protocol is, and why it is not what got built

`Specification/advanced/parallelism-locality/defining-generators.tex`: a
`Generator[\E\]` "only needs to define the `generate` method"

```
generate[\R\](r: Reduction[\R\], body: E->R): R
```

and every looping construct desugars through it:

```
C[x <- g, gs] body  =  g.generate(r, fn (x) => C[gs] body)
```

with `loop(f: E->()): () = generate[\()\](VoidReduction, f)` as the
reduction-variable-free specialisation. 1.0's own implementation emits
`exp.loop(fn x => body)` for a `for` (`PreDisambiguationDesugaringVisitor`).

THREE THINGS BLOCK THAT FORM HERE, and all three are measured rather than
argued:

1. **There is no first-class `Reduction`.** `TypedReduction` and
   `fortress_reduction_alloc` are a compiler-recognised SHAPE over ZZ32/ZZ64/RR64
   accumulators, not an object a program can pass. `generate` cannot be given
   its first argument.
2. **A `()` arrow CODOMAIN is refused by name** (`closure.rs`, `liftable_domain`),
   which is exactly the arrow `loop` takes. Recorded already: "zero files gained
   and one XXX must-fail lost". Every corpus binding condition and nearly every
   `for` body is statement-shaped, so the `()` codomain is not an edge case for
   this protocol -- it is the whole of it.
3. **A component cannot name `Generator`, `Indexed` or `Condition`.** The
   implicit core-api import is api-side only and Link 5 is architecturally out
   (04-state.md, item P). So NOMINAL membership in the protocol is unavailable
   from a `.fss` today, whatever the protocol is.

(3) is the decisive one. It means the protocol cannot be a trait a program
extends; it has to be a set of MEMBERS the checker recognises structurally.

## 2. What got built

`Library/FortressLibrary.fsi:1205` declares

```
trait Indexed[\E, I\] extends Generator[\E\]
    abstract getter size(): ZZ32
    abstract opr |self| : ZZ32
    abstract opr [i: I] : E          (* subscripting.tex *)
```

and its own doc comment states the contract that makes external iteration
legitimate: "`self[i] = v`", "stripping away the `i` yields exactly the results
of `v <- self`". So walking an `Indexed` by index IS its own generation order,
per the spec, not per an invention.

**THE PROTOCOL THIS COMPILER IMPLEMENTS IS `Indexed`, EXTERNALLY:**

| member | spelling here | 1.0 | deviation |
|---|---|---|---|
| extent | `size(): ZZ64`, or `getter size` | `abstract getter size(): ZZ32` | ZZ64, and a plain method is accepted beside the getter |
| element | `opr [i: ZZ64]: E` | `abstract opr [i: I]: E` | none -- same declaration |

`ZZ64` is not a preference: array subscripts are ZZ64 because the JVM's 2^31
ceiling is why this rewrite exists, and a `for` bound is ZZ64 for the same
reason. A `size()` returning `ZZ32` gets the ordinary
`a ZZ32 value is not implicitly converted to ZZ64` refusal, which is the same
answer every other narrowing gets here.

1.0's PRECEDENT FOR CUTTING THE PROTOCOL DOWN IS IN THIS REPOSITORY.
`Library/CompilerLibrary.fsi` is 1.0's own NATIVE-compiler library, as opposed
to the interpreter's `Library/FortressLibrary.*`. It throws the generic
`Generator[\E\]` away entirely and declares a MONOMORPHIC `trait GeneratorZZ32`
whose `generate` is overloaded at two ground result types instead of being
generic in `R`, with no `map`, `nest`, `cross`, `mapReduce`, `reduce` or
`reverse`; `Reduction` collapses to two ground traits with only `empty`/`join`.
A shipped, working existence proof that the protocol is meant to be cut for a
native backend.

## 3. Three lowerings, one resolver

`opr []` NOW DISPATCHES ON AN OBJECT. `Expr::Index` on a `Type::Object` reaches
the object's own `[_]` declaration through the ordinary whole-program dispatch,
instead of `expected an array`. That declaration already PARSED before this
milestone (the name is `[_]`, and `[_]:=` is its sibling) -- only the use was
refused. This is the single change that makes the element half of the protocol
free: the desugars below write `src[i]` and it means the array subscript on an
array and the declared operator on an object, with nothing choosing between
them.

1. **`for x <- g`** (`for_in`). The array path is UNCHANGED, byte for byte:
   `$in = g`, then `for $at <- 0 # length($in)` with `x = $in[$at]`. The object
   path is the same shape with `$in.size()` for the bound. Rank above one keeps
   its refusal.
2. **`<| e | x <- g, p |>`** over a collection. `comprehension.rs` runs BEFORE
   there are any types, so it cannot ask which spelling the bound has. It emits
   `Expr::SeqIterate`, a node the CHECKER lowers -- a sequential `while` walk,
   NOT a `for`, because a `for` body is outlined and may run on several workers
   and a comprehension appends to one shared list.
3. **`if x <- g then B else C end` / `while x <- g do B end`**. A different and
   much smaller protocol: `Condition[\E\]`
   (`Library/FortressLibrary.fsi:847`) is "a generator that generates 0 or 1
   element" and declares `getter holds(): Boolean` and `getter get(): E`. 1.0
   desugars through `__cond(e, fn (binds) => B, thunk(C))`, which needs the same
   `()` arrow codomain; the direct lowering does not:

   ```
   if x <- g then B else C end   -->   do  $c = g
                                           if $c.holds then x = $c.get; B
                                                       else C end end

   while x <- g do B end         -->   do  $c: T := g
                                           while $c.holds do
                                             x = $c.get ; B ; $c := g end end
   ```

   The `while` form RE-EVALUATES `g`, because a while-condition is evaluated
   once per round.

## 4. What is at risk of being a SILENT WRONG ANSWER

* **Order under a parallel source.** A comprehension's result order is its
  generator's. `SeqIterate` walks indices in order on the calling thread, so for
  any `Indexed` the order is the index order -- which is the contract. There is
  no parallel path to get this wrong, which is why `SeqIterate` exists instead
  of a `for`.
* **`append` from an outlined body.** Not reachable: the comprehension never
  emits a `for`.
* **A `size()` that lies.** If a user object's `size()` exceeds its subscript
  range, the walk indexes out of bounds -- and the object's own `opr []` decides
  what that means. Nothing here can check it, exactly as nothing checks a
  hand-written `while i < n`.
* **The getter/method split.** `size` is accepted as a nullary method OR a
  getter, and the checker picks the spelling from `accessors`. Getting that
  backwards would report `AccessorUnsupported` -- a refusal, not a wrong answer.

## 5. Honest payoff

Measured at `12ca542e3` over all 1956 corpus files with the pinned binary:

| first-blocker | files |
|---|---|
| `a generator over a collection ... in a comprehension` | 3 |
| `SUM`/`MIN` over a collection needs the generator protocol | 4 |
| `expected an array, found String` | 3 |
| `while x <- g` lowering | 1 (a `DXX` must-fail, must stay refused) |

and 172 corpus files WRITE a generator construct while 144 of them die in the
PARSER before the checker sees it. The protocol is NECESSARY for all 172 and
SUFFICIENT for none: 128 import a `Library` module whose `.fss` does not
compile, and an imported object is declared by an api and never defined
(`MergedObjectNotConstructible`), so the payoff is gated behind Link 5.

**So this milestone is built as a PREREQUISITE, with its own fixtures and gate
rows, and its corpus delta is expected to be small.** That is stated up front
rather than discovered afterwards.
