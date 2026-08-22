# A trait extending a scalar was NAME RESOLUTION, not representation

**2026-08-22.** `Library/FortressLibrary.fsi:376` was the compiler's third wall on
that file:

```
trait QQ extends { RR64, StandardPartialOrder[\QQ\] }
                   ^^^^  `RR64` is not a trait, so nothing can extend it
```

It was filed as a REPRESENTATION question -- a scalar carries no tag, so nothing
below it can dispatch -- and scheduled as the head of the 102-file
traits-objects class. **Both halves of that were wrong, and both are measured
below.**

## 1. The file declares the trait itself, forty-one lines above

`Library/FortressLibrary.fsi:335`:

```
trait RR64 extends Number comprises { Float, FloatLiteral, RR32, QQ }
```

The specification agrees: `conversions-coercions.tex:850-866` writes
`trait RR64 ... coerce(x: RR32) widens = ...` in its own prose. **In 1.0 these
names are ordinary library trait declarations.** They are not primitives with a
reserved namespace.

`Registry::resolve_name` matched the six builtin scalar names FIRST and
unconditionally, before consulting `self.traits`. So the `trait RR64` at :335
was REGISTERED and then UNREACHABLE: it went into the trait table and no
`TypeRef` could ever resolve to it. Every mention of `RR64` in the file --
including its own `extends` clause forty-one lines down -- got the builtin.

**The fix is the ORDER.** A declared trait or object wins; the builtin list is
the bootstrap vocabulary underneath it. That is what
`types::BUILTIN_TYPE_NAMES` already says of `Any` and `Object`: seeded because
nothing can import them yet, and out on the day import resolution can supply
them. The scalars are the same trajectory one step earlier, and
`Compiled6.u.fss` says so in source with `import CompilerBuiltin.{...} except
Boolean` above its own `trait Boolean`.

With the declaration reachable, `RR64` in supertype position is
`Type::Trait("RR64")` and every pass downstream is the ordinary trait
machinery. **`is_subtype` is untouched. No tag, no boxing, no injection.**

## 2. The measurements, all four taken before the fix was written

| | |
|---|---|
| first-blockers on the refusal | **9**, of which `DontExtendMe`x2 and `Foo` are must-FAILs that must stay red -- so **6 real files**, three of them `.fsi` |
| the "102-file traits-objects class, `alone*` 208" | a GRAB BAG. `triage --category traits-objects` is 21 files on eight different messages: `Self`, `KwSelf` in a parameter list, `object` in expression position, `found LParen` on a getter. Different milestones. |
| ACCEPTING any scalar in supertype position, swept over all 1956 files | **+1 file, and it is `ProjectFortress/tests/XXXextendBoolean.fss`** -- `object Mumble() extends { Boolean }`, whose XXX prefix means 1.0 REFUSES it. The entire measured gain of the obvious fix is one program that must not compile. |
| the shadowing fix, swept over all 1956 files | **425 -> 425. Zero gained, zero lost, zero crashes.** |

**So step 1's corpus delivery is ZERO files, and that is the honest number.**
What it buys is that `FortressLibrary.fsi` walks **1354 lines**, from :376 to
:1730, and stops on `Maybe[\(Reduction[\R\],Reduction[\R\])\]` -- TUPLE TYPES,
which is its own milestone (E).

## 3. The accept-any-scalar cut also emitted malformed IR, and that is why it lost

`ProjectFortress/compiler_tests/Compiled6.u.fss` declares its own
`trait Boolean` with `value object trueTest extends Boolean`. Matching a
supertype by NAME made an object under the USER's trait satisfy the BUILTIN
`Type::Boolean` at `is_subtype`, and codegen emitted

```
Function return type does not match operand type of return inst!
  ret ptr %trueTest
 i1
```

**exit 70 on a corpus file.** Under the shadowing fix the same file is exit 1
with a diagnostic. Pinned by
`a_component_shadowing_boolean_gets_a_diagnostic_and_not_malformed_ir`.

## 4. Two holes, both measured at zero and neither built for

* **In a shadowing component the builtin becomes UNNAMEABLE**, so a float in a
  BODY there has no type to be. Zero files reach it: `FortressLibrary.fss` is
  far behind the `.fsi`, and no other compiling file declares a scalar-named
  trait.
* **Static arguments resolve during EXPANSION, before this registry exists**, so
  `[\RR64\]` in a shadowing component would stamp the builtin while the same
  name in a signature resolved to the trait. Nothing in the corpus writes one.

A third, cosmetic: in a shadowing component a diagnostic can name both types
`Boolean` (`NOT takes Boolean operands; this one is Boolean`). The message is
correct and unhelpful. Qualifying it needs the formatter to know which of two
same-named types it holds, which is a diagnostics change and not this one.

## 5. What is pinned

Four tests in `crates/driver/tests/end_to_end.rs`, and they are an ORDER so both
sides need pinning:

* a DECLARED `trait RR64` is reachable in supertype position
* an UNDECLARED `RR64` keeps the refusal
* `XXXextendBoolean.fss` -- an OBJECT extending an undeclared scalar -- stays
  refused, which is the must-FAIL the first cut broke
* `Compiled6.u.fss` is a diagnostic and never malformed IR

Plus the FortressLibrary.fsi wall test, **repinned deliberately** from the RR64
refusal to the tuple wall: it now fails when the file regresses AND when it
advances unremarked.

**Acceptance: the IR body of all 361 objects that compiled is byte for byte
unchanged**, `declare` lines filtered.

### The instrument reported a clean pass while reading nothing

Worth recording because it nearly stood. The first IR-diff harness ran
`--emit-ir -o <path>`. **`--emit-ir` writes to STDOUT and ignores `-o`**, so it
wrote 425 empty files and compared empty against empty. It reported
`same=20 diff=0` against a compiler deliberately mutated to rename
`fortress_runtime_init` -- a clean pass on an instrument reading nothing.

Two self-tests now run first: pin-against-pin must be all same with **zero
empty**, and pin-against-the-renamed mutant must REFUSE. It does: 9 same becomes
9 diff. The empty count is reported separately and loudly, because the 64 `.fsi`
apis legitimately emit no module and that is the one case where empty is right.
