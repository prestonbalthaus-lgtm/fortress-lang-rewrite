# Fortress M3i: dotted methods

Date: 2026-08-19
Status: **landed**, commit `6644cc44b` on `m3i/dotted-methods`.
Named as next by `2026-08-19-m3g-static-argument-inference-design.md`, which is
where the 44 first-blockers were counted honestly for the first time.

Compile **205 → 222** of 1956. Parse unchanged at **614** — no parser work.
Zero non-clean exits. Dotted-method first-blockers **44 → 14**.

## The one move

A dotted method lifts to a `TypedFn` whose **parameter 0 is the receiver**, held
in a method namespace separate from `functions`.

The separation is not tidiness. 1.0 gives `x.f(y)` its own namespace and its own
shadowing rules — it is *not* `f(x, y)` — so a method must never collide with a
top-level function of the same name. `fortressc/tests/dottedmethod.fss` has both
and they stay apart.

Receiver-first is what makes the rest free. A method call becomes an ordinary
tuple, so **single dispatch is a special case of M3c's symmetric dispatch**, and
four behaviours fall out of rules that already existed:

| behaviour | the rule that already produced it |
|---|---|
| an override beats an inherited default | `Object <: Trait`, so the override is strictly more specific |
| an unimplemented abstract method is refused | the exactly-one-winner check finds no winner for that tag |
| two defaulted methods with no most specific one | M3c's ambiguity error, naming both declarations |
| return covariance across overrides | `ReturnTypeNotCovariant`, unchanged |

**Codegen changed by zero lines.** The milestone brief asked for a codegen
update; the measured answer is that none was needed. A lifted method is a
`TypedFn` and a method call is a `DispatchFn`, and codegen already compiles
both. Stating the zero is the point — the alternative is claiming work that did
not happen.

## Abstract declarations type a call and are never a target

A bodiless declaration carries `concrete: false`. It is kept in the method set,
so its name resolves and its return type is known, and it is filtered out of
`applicable`, so it can never be a dispatch target.

Both halves are needed. Dropping bodiless declarations entirely — the first
implementation — made `foo()` in `Compiled1.ai.fss` report `unknown name`, which
is false: the name exists, the implementation does not. Keeping them as targets
would let a program dispatch to a body that is not there.

One concession, stated: an abstract declaration on a *generic* trait can mention
that trait's static parameter, which this pass cannot resolve. Such a signature
is skipped rather than failing the component, because it contributes no dispatch
target and refusing a whole program over a signature nothing calls is worse.

## Two smaller rules that dotted methods require

**A bare name in a method body is a field of the receiver.** Locals win, which
is the shadowing a parameter needs. `Point(3,4).sum()` with `sum(): ZZ32 = x + y`
returns 7.

**An unqualified call inside a method body is a call on `self`.** 1.0 lets `m()`
mean `self.m()`. A top-level function of the same name wins, which is the
shadowing direction the two namespaces already imply.

## A getter read is reported as a getter

`self.myFirst` where `myFirst` is declared `getter` used to report
`Roger has no field myFirst`. The field exists as an accessor; what is missing is
accessor support. It now says so.

This is the same defect class M3g found in the static-argument catch-all, and it
is worth naming as a class: **a diagnostic that describes the wrong mechanism
moves files into the wrong bucket, and milestones get chosen off those buckets.**

## What it cost, stated plainly

Six files that compiled before do not now, because **their method bodies are
checked for the first time**. Every one is an honest refusal:

| file | why |
|---|---|
| `Compiled6.i.fss` | declares the same method twice, identically |
| `Compiled6.ad.fss` | reads a getter |
| `Compiled7.g.fss` | needs arrow types |
| `TestImports1/2.fss` | call `myname()` where no supertype declares it |
| `Compiled1.ai.fss` | abstract method on a generic trait, static parameter unresolvable |

Net is +17. A file that compiled because its body was never looked at was not a
compiling file; it was an unchecked one.

## Scope limits, deliberate

* **Methods with a `self` parameter are out.** Those are *functional* methods,
  which 1.0 lifts into the **top-level** overload set of their name — a
  different namespace and a different milestone. `selfgetter.fss` stays
  parse-only.
* **Accessors are out** of the dotted sets. `o.size` is a read, not a callee.
  M3h's position that a getter parses and is not read is unchanged.
* **Generic methods are out.** `o.m[\String\]()` still refuses. Expansion is
  untyped and cannot resolve the receiver, which is the M3g fixpoint in
  miniature; the answer that keeps the phase split is to over-approximate and
  stamp `m` into every type declaring a generic `m` of matching arity. Not
  built, because it is not measured.
* Methods are not inherited *as declarations*; inheritance is subtyping on
  parameter 0, which is why no copying happens anywhere.

## Gate

`tools/dispatch-gate.sh` 19/0 → **23/0**, with three new assertions: the
override/default/field program's output, the abstract refusal, and the diamond
refusal by name.

Two new mutations, both **shown to refuse** before the green was reported:

| mutation | result |
|---|---|
| let a bodiless declaration be a dispatch target | REFUSED, 1 check |
| stop giving a receiver field its real type in a method body | REFUSED, 2 checks |

Full run: 5 mutations, **0 survived, 0 could not be applied**. The three
pre-existing mutations now also trip the method assertions — inverting
specificity fails 7 checks where it used to fail 6 — which is the evidence that
methods really do run through the same matrix rather than beside it.

A gate-authoring trap worth keeping: the mutation table is parsed with
`IFS='|'`, so a mutation whose Rust contains a closure (`|(_, f)|`) silently
becomes unparseable and is reported as *could not be applied*, not as a pass.

`COMPILE_FLOOR` 205 → **222**.

## Next

**Generic dotted methods**, then **functional methods** (the `self`-parameter
form) lifting into the top-level overload set. Both are named above with the
reason they were not done here.
