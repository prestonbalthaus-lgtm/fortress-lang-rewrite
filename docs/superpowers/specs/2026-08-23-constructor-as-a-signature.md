# A constructor is a `Signature`, so it ties instead of shadowing

**Date:** 2026-08-23.
**Result:** corpus 514 -> 515, and the number is beside the point. Every one of
the 394 `.fss` that already compiled emits **byte-identical IR**.

Retires `ConstructorOverloadUnsupported`, landed earlier the same day
(`2026-08-23-declaration-collision-matrix.md`).

---

## The defect

`call` reached `construct` **by name**, in a match arm above the overload set:

```rust
_ if self.registry.is_object(name) => self.construct(...),
_                                   => self.user_call(...),
```

So a constructor and a top-level function of one name never met. The constructor
took every call and the function was unreachable -- not ambiguous, not refused,
**unreachable**:

```
object Wrap(x: Any) end
Wrap(x: ZZ32): ZZ32 = x + 100
Wrap(7)     -- constructs a Wrap holding 7, at exit 0, and the function never runs
```

## The fix

`declare_constructors` puts each constructor into `self.functions` as an ordinary
`Signature`: the object's value parameters, `Type::Object` as the return,
`Name$new` as the symbol. `call` falls through to `user_call` for an object name.
The arguments decide:

```
Wrap(7)     -> 107, the FUNCTION      (ZZ32 is more specific than Any)
Wrap(Dot)   -> the CONSTRUCTOR
```

**Nothing in codegen moves.** A set of one goes through `dispatch_target`'s
single-candidate arm to `Target::UserFn { Name$new }` and `call_direct`, which is
the same instruction `Target::ObjectNew { Name$new }` lowered to.

## IR identity is the claim, and it was measured

Not inferred from the paragraph above. Two binaries, `--emit-ir` over every
corpus `.fss` that compiled with the baseline, compared byte for byte:

```
compared 394 .fss files that compiled with the baseline
self test: two DIFFERENT programs compare unequal -> ok
self test: the emitter returns content -> ok
0 differ
```

Both self tests are there because `--emit-ir` writes to **stdout** and ignores
`-o`; an instrument that reads nothing reports a clean pass.

## Three exclusions, each with a mutation row

* **A singleton registers nothing.** It declares a value, not a constructor. An
  object name with no signature still reaches `construct`, which is what keeps
  `Marker()` saying *"is a singleton object; write `Marker`"*.
* **An api registers nothing.** An api has no calls, and `Library/File.fsi:16-18`
  declares the factory and the object at the **same** parameter type -- the api
  hides the `FlatString` the component's own pair (`File.fss:20`) uses.
  Registering there would make the shipped library a `DuplicateOverload` and take
  `API_FLOOR` down with it. This is the same line the earlier commit drew, now
  enforced by WHERE registration happens rather than by a refusal at the call.
* **Not into `self.slots`.** That list is indexed by declaration order over
  `Decl::Function` and is what `resolve_inferred_returns` writes through; a
  constructor has no return to infer. Appending after the loop leaves every index
  it recorded pointing where it did.

## Two behaviours arrive for free and both are correct

Identical signatures in a **component** are now `DuplicateOverload`
(`badctordup.fss`); a genuine tie is `AmbiguousDispatch` naming the tuple
(`badctortie.fss`). A constructor argument also passes
`arguments_fit_their_slots` for the first time -- `construct` never called it --
so the representation guard reaches a slot it did not before.

`ProjectFortress/tests/OverloadConstructor1.fss`, 1.0's own positive test for
matrix cell 5-3, now BUILDS AND RUNS and prints what its source says.
`OverloadConstructor2/3` stay refused, by the uniformity rule, which is a
different mechanism and the right one.
