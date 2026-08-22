# The tuple milestone, measured: the value level is worth ONE file

**2026-08-22.** The brief priced three things: tuple values and types, tuple
destructuring, and M3c arity-flattening as "the core blocker for the 21 tuple
cases". Measured with the compiler at each step, one of the three is worth
building today and the other two are not.

## What landed, and what each was worth

| step | corpus | what it bought |
|---|---|---|
| tuple TYPES resolve | 428 -> **428** | `FortressLibrary.fsi` clears :1730 |
| DESTRUCTURING `(a,b) = (e1,e2)` | 428 -> **430** | both files verified BY VALUE |
| a tuple in STATEMENT position | 430 -> **431** | `atomicExpr.fss` prints PASS |

Zero lost at every step, zero crashes, and the IR body of all 364 pre-existing
objects is byte for byte unchanged throughout.

## 1. The type level costs nothing and buys nothing, exactly as predicted

`Registry::resolve` builds the interned `Type::Tuple` instead of refusing it.
**428 -> 428.** The round-2 deferred doc said so in advance: "SPIKE-COMPOSITE-TYPE
priced the type-level half and it is cheap... That is real and it is not the
milestone."

What it buys is the head file. `Library/FortressLibrary.fsi` clears
`Maybe[\(Reduction[\R\],Reduction[\R\])\]` at :1730 and walks on.

**THE NEW WALL IS NOT A TUPLE WALL AND IT IS NOT ARITY FLATTENING**, which is
what it was assumed to be before a diagnostic could say. It is
`Library/FlatString.fsi`, reached through `import FlatString.{FlatString}`,
declaring both

```
opr ||(self, b:FlatString): String
opr ||(a:FlatString, self): String
```

Those differ only in WHICH OPERAND IS THE RECEIVER. Both are `(FlatString,
FlatString)`, so both are one signature. Isolated in a ten-line test that needs
no import.

Finding it needed a diagnostic change: three sites shared the message
`` `||` is defined twice ``, and with overloading the NAME is shared by design,
so it said nothing about which pair collided. It names both declarations and the
argument types now.

**AND AN IMPORTED SPAN IS RENDERED AGAINST THE WRONG FILE.** The library reports
that collision at `:19:20`, which is inside a comment, because the span belongs
to FlatString.fsi. Recorded, not fixed.

## 2. Destructuring is the whole value of the milestone, and it materialises nothing

`(a, b) = (e1, e2)` is split by the checker into one ordinary binding per name.
No tuple value is built, stored or passed, so there is nothing to box. That is
the deferred doc's option (2) and it is why this milestone is boxing-free by
construction rather than by discipline.

### The silent-equality trap, which nearly landed two files in the win column

`try_binding` requires an `Ident`, so a `(` fell through to the expression path
and `(min, max) = (i MIN j, i MAX j)` parsed as **INFIX EQUALITY** -- a
typechecked, discarded Boolean comparison. `tupleTest1.fss` and `tupleTest2.fss`
have no asserts and no `.test`, so that reading would COMPILE, EXIT 0, DO
NOTHING, and be counted as two files gained. It would have passed oracle section
C too.

So the binder node landed BEFORE tuples became values anywhere, and every
fixture asserts VALUES:

* `Compiled5.Binding.fss` -- `fib 20` through two destructurings per recursion,
  prints **6765**
* `VarRefTest3.fss` -- a three-element binder, prints **PASS**
* the swap `(a2,b2) = (b,a)` prints **2 1**, which pins that elements are
  checked before any name is declared

`tupleTest1/2` did NOT come along. They moved honestly to `unknown name MIN` --
which is precisely what the silent reading would have hidden.

## 3. THE REST OF THE VALUE LEVEL IS WORTH ONE FILE. Measured, not argued.

A spike gave `Expr::Tuple` a type, relaxed every refusal, and let tuples flow
through the whole checker -- the CEILING of multi-value return, tuple
parameters, tuple arguments and arity flattening combined, short of a
representation.

**430 -> 431.** One file: `atomicExpr.fss`, which needs no tuple value at all --
only both elements evaluated for their effects. That one is now landed properly
and separately.

Seventeen files moved OFF a tuple wall under the spike. Where they landed says
why the ceiling is one:

```
  5  unknown name `foobar`
  4  an arrow type is not implemented in this subset
  1  typecase on a tuple            1  a dotted method
  1  only a variable or an array element can be assigned to
  1  expected (General, G), found (G, G)      ... and eight more, all different
```

The spike also produced **four exit-70 crashes**, which is the honest cost of a
value level with no representation behind it.

## 4. ARITY FLATTENING IS NOT THE BLOCKER FOR THE 21 CASES

The brief cites `overloading.tex:124-126`. The real statement is
`Specification-1.0-frozen/basic/overloading.tex:125` -- "Recall that a functional
has a single parameter, which may be a tuple (a dotted method has a receiver as
well)". The advanced file's :124-126 is the `makeSet` AMBIGUITY example, which is
a different claim.

The rule is real. It is not what those files are waiting for. Under the full
value spike, the four files named for it land here:

```
TupleOverload1  `s1`: a component-level value declaration ...
TupleOverload2  `s1`: a component-level value declaration ...
TupleOverload3  `s1`: a component-level value declaration ...
TupleOverload4  `s1`: a component-level value declaration ...
Compiled17f     an arrow type is not implemented in this subset
tupleTypeParam  `Obj$ZZ32$e` takes 2 argument(s), found 1
toops           expected (General, G), found (G, G)
```

**Not one of them is blocked on flattening.** All four TupleOverload files want
COMPONENT-LEVEL VALUES (`s1:String = "s" "1"` at top level), which is its own
45-file milestone.

There is also no correctness debt in leaving it: `f(x:(A,B))` on a function with
a body is refused, so it cannot collide with `f(a:A,b:B)` and be resolved wrongly.
Build it when something can reach it.

## 5. `TIMES=` IS WORTH ZERO, and three of its four files are corpus defects

Priced because the two `spawn <for loop>` files -- the only possible exercisers
for the spawn runner's `fortress_in_parallel` pin -- are blocked on it. A spike
took `TIMES=` all the way through both refusal sites. All four files still fail:

```
Compiled150   this array is declared with 5 element(s) and 6 are written
Compiled160   this array is declared with 5 element(s) and 6 are written
Compiled170   this array is declared with 5 element(s) and 6 are written
Compiled6.aa  reserved word `value` is not in the implemented subset
```

Three write `a:ZZ32[5] = [1 2 3 4 5 6]`. **That is a source defect and we are
right to refuse it**, so no amount of compiler work compiles them. The fourth
needs `s.value`, which is 1.0's `Thread` accessor and not the `val()` this
compiler implements.

**Consequence: the `fortress_in_parallel` pin has no reachable corpus exerciser
and will not get one from this milestone.** Its coverage stays where the spawn
gate's own note puts it.

### The spike took three iterations to apply, and the first two lied

Worth recording. `TIMES=` is refused at TWO sites, and patching only
`compound_op_at` left the infix reader refusing first -- the files reported the
ORIGINAL message and the spike looked like it had failed to help. Patching the
second site to fall through then produced `TIMES has whitespace on one side and
not the other`, because the infix loop consumed the operator instead of leaving
it to the statement level. Only returning `Ok(None)` there gave the real answer.
A half-applied spike reads exactly like a measured result.

## Recommendation

Tuples are DONE for now at the level that pays. The next thing standing between
this compiler and the 1.0 library is not a tuple feature:

1. **The self-position operator pair** in `FlatString.fsi` -- decide whether two
   declarations differing only in which operand is `self` are one declaration or
   an error. 1.0's own library writes both, so the answer is not "refuse".
2. **Component-level values**, 45 first-blockers, and the head of all four
   TupleOverload files.
3. **Arrow types**, 14.

The tuple value level is behind all three and stays refused by name.
