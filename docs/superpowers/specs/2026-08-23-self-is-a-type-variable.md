# `Self` is a type variable, and 1.0 has no self-type

**Date:** 2026-08-23.
**Result:** corpus 508 -> 514, three objects and three apis, nothing lost.
`API_FLOOR` 117 -> 120.

The task asked for "parsing and type-checker bounding for the `Self` type
keyword ... bound `Self` to the enclosing trait/object context". **There is
nothing to bound.** 1.0 has no self-type.

---

## What `Self` is, from the grammar

`Self` is a keyword (`ProjectFortress/src/com/sun/fortress/parser/Keyword.rats
:28`) only so the grammar can spell it back in exactly two places, and both feed
the node an ordinary `Id` feeds:

```
Type.rats:193/203        /* TypeRef ::= SelfTypeId */
                         / a1:SelfTypeId { makeVarType(...) }
Type.rats:499            Id SelfTypeId = a1:Self { makeId(...) };
NoNewlineHeader.rats:343 / a1:(Variance w)? a2:SelfTypeId
                           { makeStaticParamId(..., new KindType()) }
```

`makeVarType` is a **type variable**. `KindType` is an ordinary type-kind static
parameter. `Specification/` mentions `Self` nowhere at all.

The corpus agrees, in 24 files. `CompilerLibrary/FortressLibrary.fsi:86` writes

```
trait Equality[\Self extends Equality[\Self\]\]
    abstract opr =(self, other:Self): Boolean
```

where `Library/`'s copy of the same trait writes `T`. And
`ProjectFortress/not_working_static_tests/SelfTypeTest.fss` -- a file named for
the feature -- says so in its own comment: *"the type of other needs to be
`APO[\Self\]` rather than `Self`"*. That is a sentence about a type variable.

## The load-bearing half is the placeholder rename, and it comes first

The parser gives a `self` parameter the written type `Self`, and the comment at
that site said the collision was impossible **because `Self` was reserved**.
That is a real dependency, not a note: monomorphization runs before the checker
and substitutes written static parameters **by name**, so the moment a static
parameter may be called `Self`, expansion rewrites the placeholder along with
it. A functional method on `trait Holder[\Self\]` instantiated at `[\ZZ32\]`
would take `ZZ32` as its receiver where it must take `Holder[\ZZ32\]`.

`SELF_TYPE_PLACEHOLDER` is `$Self`, which no identifier lexes. It lives in
`fortress_ast` because the parser writes it and the checker reads it.

**The witness was run both ways.** `fortressc/tests/selftypeparam.fss` prints
`42` and `true`; with the bare name put back, `describe(Slot(40), 2)` fails with
`expected ZZ32, found Slot` -- the receiver had become the static ARGUMENT.

## A narrow acceptance, not a line deleted from `RESERVED`

`type_name` takes `Reserved("Self")` in the static-parameter and type-reference
positions and nowhere else. All four of these are still refused by name, and 1.0
refuses them too:

```
Self: ZZ32 = 5        object Self end        Self(): ZZ32 = 5        f(Self: ZZ32)
```

## NAMED DEVIATION: `[\Self extends Bound\]` is accepted and 1.0 does not accept it

The only live `StaticParam` alternative that matches `Self` is
`(Variance w)? SelfTypeId`, which passes `emptyList()` for the bounds
(`NoNewlineHeader.rats:343-347`). The alternative that takes `w Extends`
requires an `IdOrOpName`, and `Self` is a keyword, so `Id` cannot match it.

Thirteen corpus sites write the bound anyway, `CompilerLibrary/FortressLibrary
.fsi:86` among them -- **the shipped library is not parsable by the shipped
grammar**, which is the second time today that sentence has been true. Accepting
it costs nothing measurable and is what the files need.

## What moved

`CompilerLibrary/FortressLibrary.fsi` moved rather than fell, which is what a
2600-line file does: `:86` is past and it stopped on `expected a function name,
found True` -- which turned out to be its own separate defect, see
`2026-08-23-true-is-a-reserved-word.md`. `Compiled17ee.fss`, a full copy of the
Comparison hierarchy written with `Self`, reaches `OpWord("INVERSE")`.

Gained: `MatchErrorBug{,1,2}.fsi`, `MatchErrorBug{1,2}.fss`, `SelfTypeTest.fss`.

## What holds it

`selftypeparam.fss` built and RUN, `badselfvalue.fss` refused, two parser unit
tests, and four mutation rows: put the writable placeholder back, refuse `Self`
as a static parameter name, refuse it in type position, and stop reserving it
at all.
