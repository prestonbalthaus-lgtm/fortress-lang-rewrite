# Multi-value return: the other half of the calling convention

**2026-08-24, Phase F.** Arity flattening (2026-08-23) built the ARGUMENT
direction of the tuple calling convention: `f(x: (A,B))` and `f(a:A, b:B)` are
ONE declaration (`overloading.tex:125`), so the first is lowered into the second
and nothing is ever whole. `tuple.rs`'s own header names what it left:

> A TUPLE RESULT. `tuple_free`'s existing refusal, untouched: returning one
> needs the CALLEE to hand back several values, which is an LLVM aggregate
> return and a milestone of its own -- the argument direction is what a calling
> convention is.

This is that milestone.

## 1. What the spec pins

`Specification/basic/types-vals-vars.tex:246-284`, the tuple-types section:

* a tuple type is "a parenthesized, comma-separated list of **two or more**
  types" -- there is no one-element tuple and no zero-element one
* "Every tuple type is a subtype of `Any`. No other type encompasses all tuple
  types. Tuple types cannot be extended by trait types."
* **covariant**: `X <: Y` iff same arity and each element `X_i <: Y_i`
* "A tuple type excludes any non-tuple type other than `Any`", and excludes
  every tuple type of a different arity

`Specification/basic/expressions/tuple-expr.tex`: "There must be at least two
element expressions... Each element of a tuple is evaluated in parallel in a
separate implicit thread." `parallelism.tex:88-90` permits an implementation to
serialise ANY group of implicit threads, which is the licence `also do` and
arity flattening already run on, and this milestone runs on it too: **elements
are evaluated left to right.**

## 2. The representation: an LLVM aggregate, and it is still non-materialising

A tuple result lowers to an LLVM STRUCT RETURN. `f(): (ZZ64, ZZ64)` becomes
`{i64, i64} @f()`, built with `insertvalue` and taken apart with `extractvalue`.

**THAT IS STILL NON-MATERIALISING**, and the distinction is the whole point: the
aggregate lives in SSA registers. There is no `fortress_alloc`, no GC block, no
32-bit type tag, no `alloca`. Nothing about the memory model changes, and the
allocation rule -- one path, through `fortress_alloc` -- is not touched because
nothing is allocated. LLVM's own ABI lowering decides whether the pair travels
in two registers or through a hidden pointer, and that decision belongs to the
target, not to this compiler.

So `Type::Tuple` gains a `basic_type` mapping where it had `None`. THE TUPLE
GATE ASSERTS THAT ARM TODAY -- "codegen has no `unreachable!` for a tuple type"
-- and that row's invariant MOVES rather than goes away: codegen must now BUILD
a struct type for a tuple and must still never panic on one.

## 3. Three touchpoints, and why each is where it is

1. **`Expr::Tuple` in expression position, when a tuple is EXPECTED.** Today it
   is refused by name (`TypeNotImplemented { form: "a tuple expression" }`) and
   that refusal STAYS for every other position -- `println(t)`, `t.m()`,
   `typecase (x,y)` still want a value there is no representation for. What
   changes is narrow: checked against `Some(Type::Tuple(elems))` of the same
   arity, each element is checked against its own expected type and the result
   is a `TypedExprKind::TupleValue`. The expectation comes from exactly one
   place -- a function whose DECLARED result is a tuple -- which is what keeps
   this from becoming a general tuple value.

2. **`tuple_free(ty, t, "the result")` at `lib.rs:1413` stops refusing.** The
   PARAMETER refusal beside it stays: flattening owns parameters, and a tuple
   parameter that reached codegen would mean the flattening pass had missed a
   site.

3. **`tuple_binding`'s "unless it is written as a tuple" refusal is narrowed.**
   `(a, b) = f(...)` where `f`'s result is `Type::Tuple([A,B])` of matching
   arity declares `a: A` and `b: B` and emits one call plus two
   `extractvalue`s. THE CALL HAPPENS ONCE, which is the reason this cannot be
   done in `tuple.rs`: that pass has no types, so splitting the binder there
   would either duplicate the call or need a whole-tuple temporary.

## 4. What is at risk of being a SILENT WRONG ANSWER

* **Element ORDER.** `(a, b) = f()` extracting field 1 into `a` is a wrong
  answer that exits 0. The fixtures assert VALUES and use DISTINCT values per
  position, because `(1, 1)` passes a swapped extraction. This is the same
  lesson `(a,a) = (1,2)` cost on the day the binder landed.
* **Arity.** A tuple type excludes every tuple type of a different arity, so a
  3-element result assigned to a 2-name binder must be refused, not truncated.
* **Covariance.** `(ZZ32, ZZ64)` is not `(ZZ64, ZZ64)`: this compiler refuses
  implicit widening everywhere, so an element mismatch is a refusal and must
  not become a silent bitcast.
* **The `()` element.** `()` has no value at all, so a tuple containing one has
  no representation. It keeps `VoidNotStorable`'s own wording, as it does at
  every other boundary.
* **A NESTED tuple result.** Measured at zero corpus files for the parameter
  direction; the result direction is refused the same way rather than lowered
  to a nested struct, because a nested aggregate makes the ABI decision depend
  on a type's shape two levels down.

## 5. Predicted payoff

The state file's estimate is "roughly ten" files -- `TupleCastPass3/4/5`,
`TupleCastGeneric4-8`, `Compiled6.ap`, `Compiled17dddd/ddddd`. THAT IS A
HYPOTHESIS AND NOT A MEASUREMENT, and first-blocker estimates on this project
have been wrong by up to 20x in both directions. The number in this section is
replaced by a measured one before the milestone is claimed.
