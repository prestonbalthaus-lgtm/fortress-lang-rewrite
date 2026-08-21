# SPIKE-TEMPLATE-WELLFORMEDNESS — result, and the branch it picks

The spike register asks two things of this spike and states the branch:

> *Stub:* check templates at opaque parameters before expansion; resolve
> declaration-header names at registration.
> *Re-run:* full 1956 sweep + api census.
> *Branch:* small drop → proceed on today's numbers; large drop → re-base
> everything.

**Answer: the two halves have opposite answers, and the branch is neither of the
two the register anticipated.**

- **Half one — header names — is not a spike at all. It is a fix, and it has
  landed.** 288 → 285, and all three lost files are must-fail tests whose
  expected error the legacy implementation recorded at the *same line and
  column* as ours.
- **Half two — checking bodies at opaque parameters — should NOT be built as
  specified.** Not because the drop is large, but because the drop would be
  dominated by *false* refusals: the encoding cannot express the bound that 23
  of the 139 static parameters in the compiling corpus actually write.

Everything below was measured on this tree, not inferred.

---

## Half one: resolving declaration-header names — LANDED

### The mechanism, which is not the one the register assumed

The register says "resolve declaration-header names **at registration**". There
is nothing to resolve at registration, because **`mono::emit` deletes the
declaration first**. `mono.rs:368-394` walks the source declarations and, for a
generic one, pushes only the *instances* built from it. Zero instances means
zero pushes, and the module's own doc comment says so (`mono.rs:9-10`: "hands
the checker a component containing no generic declarations at all"). So
`Checker::new` never builds a name table entry for it and `build_hierarchy` is
never asked about its supertypes.

There are two resolution moments for a generic header today and **both are
gated on instantiation**:

| written | today | why |
|---|---|---|
| `trait R[\T\] extends Nowhere[\T\] end` | exit 0 | deleted before anything resolves |
| `trait R extends Nowhere end` | exit 1, `unknown type Nowhere` | ground, so `build_hierarchy` sees it |
| the first, plus `object O extends {R[\ZZ32\]} end` | exit 1 | now instantiated, and `mono.rs:515-520` refuses the applied name |
| `trait R[\T\] extends Nowhere end` (no args) | exit 0 | a bare name survives `ty` untouched (`mono.rs:501`) |

So the check has to happen **before expansion**, over the source declarations,
which is where it now is: `check_template_headers`, called from `expand`.

### Scope, and why it has no false positives by construction

**Names only.** It resolves each name a generic declaration's *header* writes —
static-parameter bounds, `extends`/`comprises`/`excludes`, constructor parameter
types, function parameter and return types — against the component's own
declared names plus the builtins plus that declaration's own static parameters.

No body is checked, no parameter is made opaque, no bound is discharged. It
cannot refuse a program for a reason that depends on a type it does not have.
**Members are deliberately not walked**: a method's header inside a generic
object may legitimately name its *owner's* static parameters, which are not in
scope at the top level, and walking members would refuse those.

### The measurement

    switch off:  1956 files, 288 compile, 1668 refuse, 0 crash
    switch on:   1956 files, 285 compile, 1671 refuse, 0 crash

Three files, and **every one is a must-fail test whose recorded expectation
matches ours at the same line and column**:

| file | `.test` record | legacy expected | ours |
|---|---|---|---|
| `Compiled1.ae.fss` | `XXX1ae.test` | `14:32: D is undefined.` | `14:32: unknown type \`D\`` |
| `Compiled1.n.fss` | `XXX1n.test` | `15:25-30: Garbage is undefined.` | `15:25: unknown type \`Garbage\`` |
| `Compiled10.e.fss` | `XXX10e.test` | `20:26: S is undefined.` | `20:26: unknown type \`S\`` |

That is three of the five `X is undefined` cases the gap analysis counted among
the 45 accepted must-fail files (§5.1). It is also an **independent check on the
new `line:col` renderer**: the columns were computed by this compiler and they
agree with a Java implementation's own column arithmetic at three different
positions in three different files.

`COMPILE_FLOOR` 287 → 284, with the dated comment `apply-gate.sh`'s convention
requires.

### The one leniency, and its price

`Object` and `Any` are in the known-names set and **neither is a type this
compiler has**. They are 1.0's root traits; seeding them is
`SPIKE-OBJECT-ANY-REMEASURE`, which belongs to somebody else, and refusing them
here would take that decision out of their hands.

Measured, so the tolerance is priced rather than assumed: refusing the two names
costs **four more files** — `Compiled12.a0.fss`, `Compiled12.b0.fss`,
`Go0b.fss` (`T extends Object`) and `Library/TypeProxy.fss` (`T extends Any`).
The last of those is **one of the four `Library/` files that compile end to
end**, i.e. a quarter of the headline library number.

---

## Half two: checking bodies at opaque parameters — DO NOT BUILD AS SPECIFIED

### How much of the count is not real — measured by mutation

Of the 285 files that compile, **62 declare a generic**, carrying **112
declaration headers** and **139 static parameters**. For each of the 112, the
first static parameter was rewritten to `<name> extends NoSuchTypeXYZ` and the
file recompiled:

    57 refused    (39 with `unknown type NoSuchTypeXYZ`, the rest downstream)
    55 SURVIVED at exit 0

For 36 of the 55 — the trait, object and top-level-function headers — survival
is **hard proof the declaration is never instantiated**: `record_bounds` files a
ground obligation with `speculative: None` and `discharge_bounds` turns that
into a hard error, so an instantiated generic could not have survived.

**The other 19 are generic methods and their survival is ambiguous, for a reason
that is a defect in its own right.** Method stamps record obligations with
`speculative: Some(...)` (`mono.rs:263-268`) and a failed bound is answered by
`prune_stamp`, never by an error. `GenMet0.fss` shows the consequence: mutating
the bound on trait `b`'s override prunes only that stamp, trait `a`'s override
survives and wins, and the program **exits 0 having silently called the wrong
override**. A written-but-unsatisfiable bound on a method can change which
implementation runs, with no diagnostic anywhere. That is a separate bug and it
is not fixed here.

### What the 55 unchecked declarations contain

    23  pure pass-through, or the parameter is unused        would PASS an opaque check
    18  bodiless or empty                                    nothing to check
    11  would FAIL an opaque check today
     3  XXXGenericOverload.fss — the SET is the point, each body passes alone

The 11, with what breaks them:

- **a method call on the parameter** — `Compiled5.bh.fss:18`, `Compiled6.ae.fss:13-15`
- **string juxtaposition on the parameter** — `GenMet0/1/2.fss`, five sites, all
  `println("a" x)`. Until this session that path was **exit 70**, an internal
  error; it is a diagnostic now, which means the spike's first run would report
  a refusal rather than a crash. That fix was a prerequisite and it has landed.
- **an operator on the parameter** — `EqualityTest9a.fss:16,19`, `CompilerAlgebra.fss:26`
- **an unresolvable callee** — `oddJuxtComp.fss:17-18`

Plus roughly eleven *instantiated* templates whose ground stamps pass today and
whose templates would not: `Compiled17.fss:27` (`g[\U extends ZZ32\](u: U): ZZ32
= f(self) + u` — arithmetic directly on the parameter), `Compiled10/11/15/15a/15b`
(`Expr::Instantiate` at an opaque parameter), `fmTest5.fss:16`, `CoBoA.fss:32`,
`Compiled13/13a/13b:28`, `BuiltinBound.fss:22`.

So the drop would be roughly **20+ of 285**. But the number is not the reason to
stop.

### Why the encoding in the stub does not work

The stub proposes a synthetic trait per static parameter, seeded with the
bound's supertraits. Assessed against the actual `Registry`:

1. **Most bounds cannot be encoded as supertraits.** Of the 139 static
   parameters: 91 have no bound at all, **23 are bounded by a builtin scalar**
   (`T extends ZZ32`, `T extends String`), 16 name a plain user type, 5 are
   F-bounds or applied generics, 4 are `Object`/`Any`.
   `TraitInfo.supertraits` is a set of *trait* names, and `is_subtype`
   (`registry.rs:51-53`) requires the supertype to be a `Trait` or else plain
   equality — **scalars are outside the hierarchy by design**. So
   `T extends ZZ32` has no representation, and `Compiled17.fss`'s
   `g[\U extends ZZ32\](u: U): ZZ32 = f(self) + u` — which is *legitimate
   Fortress*, the bound says `U` is `ZZ32` — would be refused as
   `MixedNumericOperands`. **That is a false refusal, and no bound can rescue
   it.** `BuiltinBound.fss`'s own comment is the corpus asking for exactly this
   ("Make sure we can always use a builtin type as the bound for a type
   variable").
2. **`Array[\T\]` is unrepresentable at an opaque parameter.** `Elem::of` is
   `None` for a trait, so the template refuses `UnsupportedElementType` while
   every instantiation at a scalar passes.
3. **The checker cannot read a template body as written.** `Expr::Instantiate`
   is a hard error (`lib.rs:1733-1741`, "Unreachable: expansion rewrites every
   instantiation"), and six corpus templates contain one. The encoding would
   have to stamp through `mono` at a synthetic `TypeRef` and check the *stamp* —
   which lands in `self.instances`, counts against `MAX_INSTANTIATIONS`, and has
   to be filtered back out of `emit`.
4. **The dispatch half of the check is vacuous.** A synthetic trait has no
   concrete implementors, so its column in `dispatch_target`'s domain is empty,
   `cartesian` yields zero rows, and the per-cell loop never runs — no ambiguity
   check, no exhaustiveness, no `ReturnTypeNotCovariant`. M3c's exactly-one-
   winner proof does **not** transfer to templates. You get argument typing,
   not dispatch checking.
5. **The name has to be mangled and then un-mangled for diagnostics.**
   Registering the synthetic under the written name `T` collides with a real
   type of that name in 26 of the 1956 files; registering it as `$param$f$T`
   makes users read `expected ZZ32, found $param$f$T`. A display-name mapping is
   part of the change, not polish.

### The branch this actually picks

**Neither "proceed on today's numbers" nor "re-base everything".** Half one is
landed and the baseline moved by three files, all of them corrections. Half two
is not a measurement problem; it is a **design** problem, and the right next
step is a different, smaller spike:

> **`SPIKE-BOUND-DIRECTED-TEMPLATE-CHECK`.** Where a static parameter's bound
> names a *concrete* type — the 23 scalar-bounded parameters, plus the 16 plain
> user names — substitute **the bound itself** and check the stamp as an
> ordinary ground declaration. It needs no synthetic traits, no registry
> surgery, and no display-name mapping, and it is exactly what makes
> `Compiled17.fss` and `BuiltinBound.fss` checkable rather than refusable.
> The 91 unbounded parameters stay unchecked, honestly and by decision: an
> unbounded parameter licenses nothing, so *every* use of it is an error, and
> refusing 23 corpus templates to say so is not a v1 trade.

**And the number the whole plan should carry forward:** of 112 generic headers
in the files that compile, **55 are never instantiated**. Any statement of the
form "N corpus files compile" is, for the generic half of the corpus, a
statement about headers rather than about bodies. That is not fixed by this
spike and it is not fixable without deciding the above.
