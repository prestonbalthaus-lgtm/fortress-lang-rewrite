# `()` as a static argument: a `()` parameter is no parameter

**Date:** 2026-08-22. **Link 2** of `2026-08-22-library-bootstrap-measured.md`,
and the only one of the five that is pure implementation.

**Result: `ProjectFortress/LibraryBuiltin/CompilerBuiltin.fsi` CHECKS CLEAN with
all four `Condition[\()\]` sites intact.** Corpus 449 -> 450, zero lost, zero
crashes. Oracle pass 342 -> 343, and the gained case IS this file
(`CompilerBuiltinAPI.test`).

---

## The wall

    CompilerBuiltin.fsi:453:59: `()` has no value, so it cannot be stored in a parameter
    453 | trait Boolean   extends { Equality[\Boolean\], Condition[\()\] }

`trait Condition[\E18 extends Any\]` declares `getDefault(defaultValue: E18)`.
Monomorphized at `E18 = ()` that member takes a `()` parameter, and every
position that has to store a value refused one.

## Two changes, and they answer two different questions

### 1. A `()` parameter is no parameter

`Specification-1.0-frozen/basic/functions.tex:148-151` is the oracle:

> Note that it is permitted to have a single plain binding, or to have no
> bindings. The latter case, `()`, is considered equivalent to a single plain
> binding of the ignored identifier `_` of type `()`, that is, `(_: ())`.

So `f()` and `f(_: ())` are ONE declaration, and a functional written with a
`()` parameter is a functional of no parameters. Dropped in
`Expander::functional_params`, which is the single gate every instantiated and
every ground functional signature goes through -- before `Checker::new`, so
`overload_counts`, dispatch and conform all see the same arity.

**FUNCTIONALS ONLY.** An object's value parameters are its FIELDS: they decide a
layout and a constructor arity, so they keep the refusal. `Cell[\()\](())` is
still `` `()` has no value, so it cannot be stored in a field ``.

### 2. `()` is a subtype of `Any`, and still has no representation

With the parameter gone the wall moved to `() does not satisfy E18 extends Any`.

`basic/types-vals-vars.tex:469-471`: "The type `()` is the type of the value
`()`. Its only supertype (other than itself) is `Any`, and it excludes every
other type." :136 and `basic-lib/objects.tex:20` list the immediate subtypes of
`Any` as tuple types, arrow types, `()` and `Object` -- so `()` does NOT sit
under `Object`, and `fortressc/tests/voidnotobject.fss` is the fixture that
keeps it from being written that way.

**AND THAT IS WHERE IT EXITED 70.** With `() <: Any` true, `f(x: Any) = 1`
called as `f(())` type-checked and reached codegen, which has nothing to put in
a tagged pointer:

    fortressc: internal error: a void expression used as a value

User source crashing the compiler. The fix is the split this codebase already
draws for tuples -- RESOLVING IS NOT HAVING A REPRESENTATION -- applied at
`require`, the one place a computed value is checked against its context.
`fortressc/tests/badvoidvalue.fss` is that program and it is now a diagnostic.

**THE GUARD ASKS FOR THE SUBTYPE RELATION TOO, and that is not redundant.**
Without it the arm fires ahead of `Mismatch` for every wider slot, and
`tupleTest3.fss`'s `expected ZZ32, found ()` -- which names the type the reader
wants -- became a sentence about representations. Caught by re-reading the
message diff, not by any test.

## DEV-16: every `()` parameter, not just a sole one

The citation covers the SOLE case exactly. 1.0 keeps a `()` BESIDE other
parameters as a real value, and `ProjectFortress/other_compiler_tests/
VoidArrowTest2.fss` is the witness: it declares `oneVoidOneString(x: (), y:
String)` and calls it as `((),"test")`, and its `.test` expects the program to
run.

**THE NARROW RULE WAS SPIKED AND DOES NOT PAY.** Restricting the drop to a sole
parameter leaves `CompilerBuiltin.fsi:453` exactly where it was:
`Condition[\()\]` declares `reduce(_:(E18,E18)->E18, id:E18)`, which puts a `()`
beside an arrow. Measured with the compiler, not argued.

**IT CANNOT PRODUCE A WRONG ANSWER.** A call that still writes the `()` fails
the arity check: `oneVoid` takes 0 argument(s), found 1. That is a refusal
naming the wrong thing, recorded as DEF-VOIDARITY. `VoidArrowTest2.fss` was
blocked before this milestone and is blocked after it -- on a different message.

Also pre-existing and unrelated: `f(())` against `f()` is refused on the
baseline binary too. `(())` is not folded to the empty argument list.

## Measured

| | 449 (DEV-14 retired) | this |
|---|---|---|
| `.fss` -> object | 383 | 383 |
| `.fsi` check | 66 | **67** |
| total | 449 | **450** |
| crashes | 0 | 0 |
| oracle pass | 342 | **343** |
| must-fail accepted | 38 | 38 |
| section C signals | 3 baselined | 3 baselined |

GAINED: `ProjectFortress/LibraryBuiltin/CompilerBuiltin.fsi`. LOST: none.
Four blocked files moved to a later diagnostic, two of them off this wall
(`Compiled6.u.fss`, `VoidArrowTest2.fss`) and two past a `()` bound that now
discharges (`Compiled3.l.fss`, `Compiled3.o.fss`, on to `String does not
satisfy X extends Any`).

`Library/FortressLibrary.fsi` still stops at :362 `unknown type RR32`, which is
what link 3 -- the implicit builtin import -- exists to answer.

## Gated

`tools/unit-gate.sh`, whose subject this milestone changed sides on.
`badvoidparam.fss` is `unitparam.fss` and an ACCEPTANCE now, proved by
comparing its emitted module against `unitnoparam.fss`'s rather than by both
compiling. Plus `condunit.fsi` (the `Condition[\()\]` shape in ten lines, so a
failure names the feature and not a 754-line file), `CompilerBuiltin.fsi`
itself, and `unitinstance.fss` -- a generic monomorphized at `()` that compiles,
LINKS and RUNS, printing 7.
