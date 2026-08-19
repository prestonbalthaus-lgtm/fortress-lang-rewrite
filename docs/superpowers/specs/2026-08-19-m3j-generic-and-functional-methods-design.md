# Fortress M3j: generic dotted methods, and functional methods

Date: 2026-08-19
Status: **landed** on `m3j/generic-and-functional-methods`, five commits, not
pushed. Named as next by `2026-08-19-m3i-dotted-methods-design.md`.

Compile **222 -> PLACEHOLDER** of 1956. Parse unchanged at **exactly 614** --
the parser changed by zero lines. **Codegen changed by zero lines**, again, and
that is a measured result and not an omission: a stamp is a `TypedFn`, a lifted
functional method is a `TypedFn`, and codegen already compiled both.

Baseline measured on the M3i tip, not taken from a document: 1956 files,
**222 exit 0, 1734 exit 1, 0 anything else**.

## Three parts, and the third was not in the brief

The brief named two. Reading the expander turned up a third that both stand on,
so it is stated and measured as its own step.

| step | compile | what |
|---|---|---|
| baseline | 222 | the M3i tip |
| Part 0 | **224** | a ground method is substituted whole |
| Part 2 | **229** | functional methods |
| Part 1 | **PLACEHOLDER** | generic dotted methods |

M3h's lesson held again in the other direction: measured separately these are
+2, +5 and +13, and the parts do not simply add, because a file blocked on one
usually contains another.

## Part 0: a method's return type and body were not substituted

`Expander::members` substituted a method's *parameters* and cloned its return
type and body. The comment said why -- "method bodies are not walked, dotted
methods are parsed and never checked" -- and that stopped being true at M3i,
which checks them.

The consequence was a hard refusal on ordinary code. `object Cell[\T\](v: T)`
with `get(): T = v` reported **`unknown type T`**, because `T` reached
`build_method_signatures` unsubstituted and the `Err(_) if abstract_` escape
only covers bodiless declarations. The same clone swallowed demand: a method
body calling `foo[\ZZ32\]()` raised no instantiation request at all.

### And a slot map keyed by something that is not unique

`method_slots` was keyed by the declaration's start offset. Two instantiations
of one generic type are **clones of the same members and carry the same span**,
so `Cell[\ZZ32\].get` and `Cell[\String\].get` shared one entry and the second
overwrote the first.

Rekeyed to `(owner, member index)`, which is unique by construction and reads
identically in both passes -- so it cannot desynchronise the way the running
positional index M3i's comment warns about would.

`fortressc/tests/genericowner.fss` prints `7 hi 7 hi` and the emitted module
carries `Cell$ZZ32$e$m$get` returning `i32` beside `Cell$String$e$m$get`
returning `ptr`. Two instantiations, two methods, two return types.

## Part 1: generic dotted methods, by over-approximation

`o.m[\String\]()` refused. Expansion is untyped, so it cannot know what `o` is,
and it runs before the checker, so nothing later can raise the demand -- the
M3g fixpoint in miniature.

The answer that keeps the phase split is the one M3i named: **stamp `m` at
those arguments into every type declaring a generic `m` of matching static and
value arity.** Demand stays syntactic; the receiver is never consulted.

Correctness comes from what happens next. A stamp is an ordinary ground method,
so M3c's symmetric dispatch picks the winner by receiver type and the stamps
nothing reaches are dead code. `MAX_INSTANTIATIONS` counts stamps and type
instantiations against **one** ceiling, so the guessing is bounded by a limit
that already existed.

The call site keeps its shape and only its name changes:

```
Call{ callee: Instantiate{ callee: Field{o, "m"}, args: [String] } }
  ->  Call{ callee: Field{o, "m$String$e"} }
```

and the unqualified form inside a method body -- `f[\S\]()`, which
`compiler_tests/Compiled15.fss` writes -- becomes `Var("f$ZZ32$e")` and lands
on M3i's `m()` means `self.m()` path. The value arity is read at the
application, which is why the rewrite lives in the `Call` arm and not in the
`Instantiate` arm below it.

**Two substitutions, composed, applied once.** A generic method inside a
generic type has an owner-level substitution and a method-level one. Walking
with either alone is wrong -- the owner's alone meets the method's own
parameters unbound and would mangle a request for a type that does not exist --
so the template records the owner's and the stamp walks once with the union,
the method's own parameters winning where names collide.

**One fixpoint, two kinds of job.** A stamped body can demand a type
instantiation and a type instance registers method templates a stamp still has
to be made from, so `expand_types` and `stamp_methods` run in one loop to a
joint fixpoint rather than one after the other.

`fortressc/tests/genericmethod.fss` prints `1 2 1 2 6`: the receiver decides,
twice of it at run time through a trait. `Unused` never appears at a call site
and still carries both stamps -- the over-approximation, read off the object
rather than taken on trust -- and `Spare`, which declares `f` at an arity
nothing demands, carries none.

### What a wrong guess costs, and where the line is

Stamping into a type the receiver could never have puts a body in front of the
checker that the program never asked to compile. Two failure modes, and they
are **not** the same:

* **A bound fails on a stamp.** The obligation is tagged with the stamp's
  identity. `discharge_bounds` runs at the top of `run`, before any body is
  checked and before any dispatch table is memoised, so a failure there
  **withdraws the stamp** instead of refusing the component. A call whose
  receiver domain includes the withdrawn type then fails the exactly-one-winner
  check M3c already runs. No new rule.
* **A stamped body does not typecheck.** That is a **hard error**, deliberately.
  Dropping a would-be winner after signatures exist reroutes dispatch to a less
  specific applicable member, which is a silently wrong answer. It did not bite
  the corpus: the sweep is clean.

**A withdrawn stamp leaves the candidate set entirely, not merely the target
list**, and that half is measured too. Left in, its wrongly instantiated
parameter types reach `agreed`, the two declarations disagree on a column, a
literal takes no hint and defaults to `ZZ32`, and the program is blamed for a
guess expansion made. `tests/prunedstamp.fss` prints `1 2 3`; with either half
reverted it reports a diagnostic against source that is correct.

Stated limit: withdrawal covers the direct obligation. A *type* instantiation
demanded only by a wrong stamp records ordinary, untagged obligations, so a
bound failure one level down still refuses the component. Not built, because
not measured.

## Part 2: functional methods

A member with a `self` parameter is a *functional* method. 1.0 lifts it into
the **top-level overload set of its name** -- `functions`, not `methods` -- and
it is written `f(x, y)`, never `x.f(y)`.

`self` keeps its **written position**. `area(self, k: ZZ32)` lifts to
`(Owner, ZZ32)` and `scaled(k: ZZ32, self)` to `(ZZ32, Owner)`. Symmetric
dispatch does not care which column holds the interesting type, so forcing the
receiver to position 0 would be code with a chance of being wrong and no chance
of being right in a new way.

`Self` resolves to the owner across the whole signature -- the `self`
parameter, the other parameters, and the return type -- and **dotted methods
resolve it the same way**, because two kinds of method disagreeing about `Self`
would be a difference with no reason behind it. For a trait owner it resolves
to the trait type; under closed-world dispatch that is the sound reading, and
it is a stated deviation from 1.0, where `Self` is the receiver's run-time type.

Symbols are always owner qualified (`Owner$f$name`), never bare, because a bare
one collides with a real top-level `f`. The overload count is taken over the
**merged** set -- top-level declarations and functional methods together -- or
two members take one symbol and codegen defines the second against the first's
declaration, which is the bug `method_symbol`'s comment already records.

`tests/functionalmethod.fss` prints `16 0 107 15 9 0`, and each number is a
different rule: the override, the inherited default, a real top-level member of
the same set, `self` written second, and dispatch deciding on the run-time type
twice.

**A generic functional method is out of scope, and gets a named diagnostic.**
`unknown name` about a declaration three lines up is false, and files the
program under the wrong blocker -- and milestones are chosen off those buckets.
Five corpus files say so by name now; before, they were spread across
`unknown name` and `takes no static arguments`.

## The deviation this milestone had to decide: a requirement is not an implementation

M3i's note says a bodiless declaration "types a call and is never a target".
The code only had the second half: `applicable` filtered on `concrete` before
the static return type was computed, so a trait declaring `f` abstractly with
every object beneath it implementing `f` **refused** -- ordinary Fortress, and
the shape `compiler_tests/Compiled15.fss` is written in.

Making abstract declarations simply applicable is also wrong, and the corpus
said so within one sweep: in
`long_term_not_working/abstract/DiamondInheritance7.fss` an object inherits a
concrete `m` from `S` and an abstract `m` from `T`, and the two tie -- an
ambiguity reported between an implementation and a *requirement*, which is not
an ambiguity at all.

The rule, and it is a deviation worth signing off rather than discovering in a
diff: **what types a call is the implementations, and a bodiless declaration
only when there is no implementation applicable at all.** Two witnesses, both
in the gate:

| witness | before | after |
|---|---|---|
| `Compiled15.fss` | refused, `no declaration of f$ZZ32$e applies to (T)` | compiles, prints `pass` |
| `DiamondInheritance7.fss` | ambiguity between `S.m` and abstract `T.m` | compiles, prints `S.m cd` |

`badabstract.fss` is still refused and its diagnostic got **better**: it now
names `(Rock)`, the type that is missing the implementation, rather than
`(Animal)`, the trait that is not.

## What it cost

Two files that compiled before do not now, and both are honest:
`compiler_tests/Compiled10.d.fss` and `Compiled10.k.fss` are the legacy suite's
own `comprises` tests -- `.k` annotates each call `(* Yes *)` or `(* No *)` and
contains calls marked *No* -- and their functional method bodies are checked
for the first time. A file that compiled because its body was never looked at
was not a compiling file.

Three of M3i's six regressions come back: `Compiled1.ai.fss` and
`TestImports1/2.fss` all needed a generic method to resolve.

## Gates

`tools/dispatch-gate.sh` **23/0 -> PLACEHOLDER**, with the four programs above
asserted by output, the over-approximation asserted in symbols, and two
refusals asserted by name.

`tools/apply-gate.sh` `COMPILE_FLOOR` **222 -> PLACEHOLDER**.

Mutations, every one **shown to refuse** before any green was reported:

| mutation | result |
|---|---|
| file a method slot under its span, which two instantiations share | REFUSED, 9 checks |
| stamp no generic method anywhere | REFUSED, 5 checks |
| drop the first static argument of every stamp | REFUSED, 2 checks |
| refuse the component instead of withdrawing a wrong stamp | REFUSED, 2 checks |
| leave a withdrawn stamp in the candidate set | REFUSED, 2 checks |
| let a requirement tie with an implementation | REFUSED, 4 checks |
| let a bodiless functional declaration be a dispatch target | REFUSED, 1 check |

Full run: dispatch **12 mutations, 0 survived, 0 could not be applied**.

Two mutations in *other* gates went to **could not be applied** on the first
run, which is not a pass and is easy to skim past -- the generics gate's
emission-order mutation because the loop it names was rewritten, and the unit
gate's void-guard mutation because a second call site made its pattern
ambiguous. Both are repointed, and the top-level parameter loop was written out
long-hand so its mutation target is unique and contains no `|` -- the mutation
table is split on `IFS='|'`, so a Rust closure in one becomes unparseable and
is reported as *could not be applied*.

**The four mutating gates now restore from `HEAD`, not from the index**, and
refuse to run unless the tree already matches `HEAD`. Restoring from the index
faithfully puts a defect back if anything stages during a run, and the worktree
and the index then agree with each other while both are wrong. The apply gate's
corpus walker also skips `.claude`, which holds agent worktrees -- full repo
copies, and one left in place reads the corpus at several times its size.

## Next

`compiler_tests/Compiled17.fss` -- generic methods on generic types, both
halves of this milestone at once -- now gets as far as **`unknown name AND`**,
which is library surface rather than a language rule.

The blocker histogram moved where it should: dotted method 14 -> 3, unknown
name 97 -> 87, unknown type 89 -> 84, `takes no static arguments` 7 -> 4, and a
new honest bucket of 5 for generic functional methods.

Levers, all measured against the stale 476 parser baseline and therefore due a
re-spike -- and the bias is **low**: dotted/braced/foreign imports, `var`
bindings, `opr` declarations, object expressions.

This note **supersedes** the "Generic methods are out" and "Methods with a
`self` parameter are out" scope items in the M3i note. Both were correct when
written.
