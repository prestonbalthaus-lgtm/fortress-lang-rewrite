# Fortress M3j: generic dotted methods, and functional methods

Date: 2026-08-19
Status: **spec**, written before the code.
Named as next by `2026-08-19-m3i-dotted-methods-design.md`, which scoped both
halves and said why neither was built there.

Baseline, measured on the `m3i/dotted-methods` tip and not taken from a
document: **1956 corpus files, 222 exit 0, 1734 exit 1, 0 anything else.**

## Three parts, not two

The milestone brief names two. Reading the expander turned up a third that both
of them stand on, so it is stated as its own part and measured as its own step.

### Part 0: a method's return type and body are not substituted

`Expander::members` substitutes a method's *parameters* and clones its return
type and body verbatim. The comment says why — "method bodies are not walked,
dotted methods are parsed and never checked" — and that comment stopped being
true at M3i, which checks them.

The consequence is a hard refusal on ordinary code:

```
object Cell[\T\](v: T)
  get(): T = v
end
```

`unknown type T`, because `T` reached `build_method_signatures` unsubstituted
and `storable` could not resolve it. The `Err(_) if abstract_` escape only
covers bodiless declarations.

The same clone also swallows demand: a method body calling `foo[\ZZ32\]()`
raises no instantiation request, so the checker meets an `Expr::Instantiate`
and reports `NotGeneric` about a name that is.

Part 0 substitutes all three for a **ground** method. A **generic** method
cannot be walked here at all — its body may name its own static parameters, and
walking `Cell[\S\]` with `S` unbound would mangle a request for a type that
does not exist. It is registered as a template instead, which is what Part 1
consumes.

### Part 1: generic dotted methods, by over-approximation

`o.m[\String\]()` refuses today. Expansion is untyped, so it cannot know what
`o` is, and it runs before the checker, so nothing later can raise the demand.

The answer that keeps the phase split is the one M3i named: **stamp `m` at
those arguments into every type that declares a generic `m` of matching static
and value arity.** Demand stays syntactic. The receiver is never consulted.

Correctness comes from what happens after: a stamp is an ordinary ground
method, so M3c's symmetric dispatch picks the winner by receiver type and the
stamps nothing reaches are dead code. `MAX_INSTANTIATIONS` counts stamps, so
the over-approximation is bounded by the ceiling that already exists.

The call site rewrites to the mangled name and stays a dotted call:

```
Call{ callee: Instantiate{ callee: Field{o, "m"}, args: [String] } }
  ->  Call{ callee: Field{o, "m$String$e"} }
```

The unqualified form inside a method body — `f[\S\]()`, which
`compiler_tests/Compiled15.fss` writes — rewrites to `Var("f$ZZ32$e")` and
lands on M3i's `m()` means `self.m()` path.

**Two substitutions, composed, applied once.** A generic method inside a
generic type has an owner-level substitution and a method-level one. Walking
with either alone is wrong, so the template records the owner's substitution
and the stamp walks once with the union, the method's own parameters winning
where the names collide.

**One fixpoint, two kinds of job.** A stamped body can demand a type
instantiation and a type instance can register new method templates, so type
expansion and stamping run in one loop to a joint fixpoint rather than one
after the other.

#### What over-approximation costs, and where the line is

Stamping into a type the receiver could never have puts a body in front of the
checker that the program never asked to compile. Two failure modes, and they
are **not** the same:

* **A bound fails on a stamp.** `record_bounds` tags the obligation with the
  stamp's identity. `discharge_bounds` runs at the top of `run`, before any
  body is checked and before any dispatch table is memoised, so a failure there
  **prunes the stamp** instead of refusing the component. A call whose receiver
  domain includes the pruned type then fails the exactly-one-winner check that
  M3c already runs, naming the tuple. No new rule, and the closed-world answer.
* **A stamped body does not typecheck.** That is a **hard error**. Dropping a
  would-be winner after signatures exist reroutes dispatch to a less specific
  applicable member, which is a silently wrong answer — the class this compiler
  refuses to produce. If it bites the corpus the cost gets stated, the way
  M3i's six regressions were.

Stated limit: pruning covers the direct obligation. A *type* instantiation
demanded only by a wrong stamp records ordinary, untagged obligations, so a
bound failure one level down still refuses the component. Not built, because
not measured.

### Part 2: functional methods

A member with a `self` parameter is a *functional* method. 1.0 lifts it into
the **top-level overload set of its name** — `functions`, not `methods` — and
it is called `f(x, y)`, never `x.f(y)`.

`self` keeps its **written position**. `area(self, k: ZZ32)` lifts to
`(Owner, ZZ32)` and `foo(x: ZZ32, self)` lifts to `(ZZ32, Owner)`. Symmetric
dispatch does not care which column the interesting type is in, so forcing
position 0 would be extra code with a chance of being wrong.

`Self` resolves to the owner across the whole signature — the `self` parameter,
the other parameters, and the return type. For an object owner that is the
object type; for a trait owner it is the trait type, which under closed-world
dispatch is the sound reading and is stated as a deviation from 1.0's
run-time-type reading.

Symbols are always owner qualified (`Owner$f$name`), never bare, because a bare
one collides with a real top-level `f` of the same name. The overload count is
taken over the **merged** set — top-level declarations and functional methods
together — or two members get one symbol and codegen defines the second against
the first's declaration, which is the bug M3i's `method_symbol` comment records.

**A generic functional method is out of scope**, and it gets a named
diagnostic rather than the `unknown name` catch-all: the name exists, the
lifting does not. `not_passing_yet/genericFunctionalMethods.fss` is the corpus
witness. A wrong-mechanism diagnostic moves files into the wrong bucket and
milestones are chosen off those buckets — that lesson is two milestones old.

## A defect Part 1 forces out into the open

`method_slots` is keyed by `m.span.start`. Two instantiations of one generic
type clone the same members, so `Cell[\ZZ32\].get` and `Cell[\String\].get`
carry **the same span**, and the second overwrites the first. Stamps make it
worse — one template, many owners.

Rekeyed to `(owner, member index)`, which is unique by construction and
identical in both passes, so it cannot desynchronise the way the positional
index M3i's comment warns about did.

## Measurement plan

Per-construct deltas are measured, in this order, each against the previous:

| step | what |
|---|---|
| baseline | 222 |
| Part 0 | method substitution |
| Part 2 | functional methods, measured on Part 0 |
| Part 1 | generic dotted methods, measured on Part 0 + Part 2 |

M3h established that a delta taken against an older baseline is biased **low**,
not merely unreliable, so the combined number is what the ratchet takes.

Non-negotiable, from the guidelines: every gate self-tests, and a real mutation
is run against each new assertion and **shown to fail** before its green result
is reported. The mutation table is split on `IFS='|'`, so no mutation may
contain a Rust closure.

## Expected non-results, to be reported as results

Codegen is expected to change by **zero lines** again: a stamp is a `TypedFn`
and a lifted functional method is a `TypedFn`. If that holds it gets stated.
The parser is expected to change by zero lines; the parse floor must read
**exactly 614** afterwards, and anything else means work leaked into it.
