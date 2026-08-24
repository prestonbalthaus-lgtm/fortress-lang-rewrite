# A merged declaration that loses to a DIFFERENT static arity keeps its own name

**Date:** 2026-08-24. Phase G2 of Preston's Library parser offensive, which the
brief called "the `ReductionWithZeroes` name-resolution blocker".

**Answers:** `Library/GeneratorLibrary.fsi`, the head of the Library queue.
**Result:** corpus 569 -> 570, zero lost, zero exit 70/101/139, 435 objects
byte-identical IR.

---

## What was wrong

`crates/driver/src/resolve.rs` merges an imported api's declarations into ONE
FLAT list on the importing component, before `mono::expand` and before
`Checker::new`. Name resolution is therefore a single namespace with a
precedence order: the component's own declarations win, then earlier-merged
apis, and a later api's same-named declaration is DROPPED.

`Library/GeneratorLibrary.fsi:275` declares its own

    trait ReductionWithZeroes[\R extends Any\] extends Reduction[\R\]

`Library/FortressLibrary.fsi:1871` declares

    trait ReductionWithZeroes[\R,L\] extends ActualReduction[\R,L\]

and SIX of FortressLibrary's own objects name it at TWO arguments in their
`extends` clauses -- `LexicographicPartialReduction` (:78),
`LexicographicReduction` (:87), `UniqueItemMeetReduction` (:1018),
`UniqueItemJoinReduction` (:1031), `AndReduction` (:2009), `OrReduction`
(:2021). Those six do NOT collide, so they merged. FortressLibrary's
`ReductionWithZeroes` DID collide, so it was dropped -- and the six objects'
references re-resolved to GeneratorLibrary's ONE-parameter declaration:

    Library/GeneratorLibrary.fsi:43:49: `ReductionWithZeroes` takes 1 static
    argument(s), found 2

raised at `crates/types/src/mono.rs:372`. The span is `:43`, a line that does
not mention the name, because the span belongs to the imported file -- the
already-recorded imported-span defect on top.

## The rule

A merged declaration that loses a name collision is still DROPPED, and the
winner still speaks for both -- EXCEPT when the two declarations take a
DIFFERENT NUMBER OF STATIC PARAMETERS. Then the loser keeps its identity under
an unwritable private name, and every reference from ITS OWN api follows it.

**Why arity and not "any collision".** Measured over all 1956 corpus files with
an instrumented resolver, TWICE -- and the first instrument was the wrong one,
which is the finding, not a footnote.

The first census compared `static_params.len()` and the declaration KIND, and
reported 25,639 collisions across 1112 files with 24 mismatched. **That predicate
is not this codebase's own definition of "same shape", and the codebase learned
that the hard way.** `check_uniformity` (`crates/types/src/mono.rs:1694-1699`)
compares parameter COUNT, each parameter's `bounds.len()`, AND each parameter's
KIND, with a comment (`:1687-1693`) saying exactly why count alone was a bug:
*"An overload set mixing `f[\T\]` and `f[\nat n\]` has one parameter each with
no bounds either side, so the length comparison alone accepts it."* That was
D7's work and it is already paid for.

Re-measured with `check_uniformity`'s predicate, over the same 1956 files:

    collisions reaching the shape check                        25,637
      IDENTICAL under count + bounds + kind                     18,705
      SAME COUNT, DIFFERENT BOUNDS -- silently flattened         6,921
      differ in parameter COUNT -- what the rename fires on          11

**The 11 are all of it, and they are four files:** `GeneratorLibrary.fsi` and
`.fss` (`ReductionWithZeroes` 2-vs-1, `BigOperator` 4-vs-5, `BigReduction`
2-vs-3, `Comprehension` 4-vs-5), `not_working_library_tests/MaybeTest1.fss`
(`Nothing` and `NotUnique`, 1-vs-0) and `not_working_static_tests/
SingleImport.fss` (`List`, 1-vs-0). The 24 of the first census was 11 of these
plus 13 trait-versus-object pairs at EQUAL count, which the rule declines by
design.

568 of the 1112 colliding files compile today. The shipped libraries are LAYERED
COPIES -- `CompilerBuiltin` is the root and `FortressLibrary` layers on top --
and identifying their same-named declarations is what the layering is FOR.
Scoping every collision would give `FortressLibrary` its own `Comparison`,
`Equality` and `Generator`, unrelated to `CompilerBuiltin`'s, and break the thing
the design exists to do.

A different COUNT is proof they are not copies. No substitution makes `[\R\]`
and `[\R,L\]` one declaration.

**Why not kind.** `trait` vs `object` at the SAME arity is left alone, because
`CompilerBuiltin.fsi`'s `trait NN32` against `LibraryBuiltin/FortressBuiltin
.fsi`'s `value object NN32` is DELIBERATE (see
`2026-08-23-numeric-hierarchy-meet-rule.md`) and that file checks today.
Including kind would separate two declarations that are meant to be one type.

## The whole shape-mismatched set, measured

    Library/GeneratorLibrary.fsi         ReductionWithZeroes t[2]->t[1]  FIXED, now exit 0
                                         BigOperator t[4]->t[5]
                                         BigReduction o[2]->o[3]
                                         Comprehension o[4]->o[5]
    Library/GeneratorLibrary.fss         the same four                   moved to a NEW wall
    LibraryBuiltin/FortressBuiltin.fsi   NN32 IntLiteral RR32 FloatLiteral Boolean, t[0]->o[0]
    LibraryBuiltin/FortressBuiltin.fss   four of the same                arity equal, NOT touched
    compiler_tests/Compiled9.e.fss       Names.a.foo t[0]->o[0], bar t[0]->value
    compiler_tests/Compiled9.f.fss       the same two                    arity equal, NOT touched
    not_working_library_tests/MaybeTest1.fss  Nothing o[1]->o[0], NotUnique o[1]->o[0]
    not_working_static_tests/SingleImport.fss List t[1]->o[0]            still exit 0

Only `GeneratorLibrary.fsi` changes status. `SingleImport.fss` and
`FortressBuiltin.fsi` compiled before and compile after; both were checked by
name because both are cases the rule newly touches.

**And only ONE of the four mismatches on GeneratorLibrary was load bearing.**
Delta-debugged by deleting one declaration at a time: deleting
`ReductionWithZeroes` alone makes the file exit 0. `BigOperator`,
`BigReduction` and `Comprehension` are reached only from `opr BIG` FUNCTION
declarations, which `resolve.rs` skips outright, so nothing merged ever named
them at the wrong arity.

## Why the rename and not the three alternatives

**Keep both under one name and let `mono` pick by arity** is refused by the
compiler already: `check_uniformity` (`crates/types/src/mono.rs:1700-1706`)
raises `OverloadSetStaticParamsDiffer` on exactly that pair, and `expand` calls
it (`mono.rs:167`) before `expand_types` is reachable. `Checker::new`'s
`declared` map (`crates/types/src/lib.rs:659`) and `Registry.traits`
(`registry.rs:36`) have one slot per name besides.

**Let the api win instead of the importer** is refused by the witness file's own
line 283: `GeneratorLibrary.fsi` writes `extends { ReductionWithZeroes[\R\], ...
}` -- its own name at ONE argument. Flipping the winner mirrors the identical
defect into the other file and gains nothing. It also inverts
`source-code.tex`'s rule that satisfying the api is the component's obligation.

**Drop the declarations that reference the mismatched name** is bounded at
exactly the six objects above and would work today. It was declined because it
destroys information rather than modelling it: a file that later writes
`AndReduction` gets `unknown type AndReduction` pointing at nothing, which is
the failure this project has now paid for twice ("A DIAGNOSTIC THAT NAMES A
MISSING TYPE IS NOT ALWAYS ABOUT THE TYPE"). It is the fallback if the rename
ever has to come out.

## What the rename actually is

In this compiler THE NAME IS THE TYPE IDENTITY: `Type::Object(&'static str)` and
`Type::Trait(&'static str)` (`crates/types/src/types.rs:198,202`). So scoping a
namespace means putting the origin api into the identity, and the resolver is
the one boundary that can still see which api a declaration came out of. The
private name is `$<api>$<name>`, LEADING `$` the same way
`SELF_TYPE_PLACEHOLDER` is `$Self`: `$` lexes as an operator character and never
as part of an identifier, so no source file can write one or be shadowed by one,
and `mangle_static` builds `Name$Arg$e`, which never starts with `$`.

Three things make it complete rather than partial:

- **A POST-PASS.** An api's declarations arrive in source order, and
  FortressLibrary writes six of the references (:78, :2021) on both sides of the
  declaration itself (:1871). The collision is found after most of the
  references are already merged.
- **api-SIDE AND BODY-FREE, AND THE SECOND HALF IS CHECKED RATHER THAN
  ASSUMED.** `rename_types` walks type positions and not bodies, and NINE `Expr`
  variants can carry a `TypeRef` (`ObjectExpr` -- which recurses into whole
  members -- `Comprehension`, `Try`, `Instantiate`, `Lambda`, `TypeCaseArm`,
  `Binding`). So walking type positions alone is complete EXACTLY WHEN there are
  no bodies.
  THE PARSER DOES NOT ENFORCE THAT AN api HAS NONE. `member()` takes no
  signature-only flag -- that reaches `fn_decl` and `opr_decl`, top-level
  functions -- so a member method reads a body and a field reads an initializer
  identically in an `api` and a `component`. The only enforcement is
  checker-side, and AN IMPORTED api IS PARSED AND MERGED AND NEVER CHECKED.
  `ProjectFortress/parser_tests/XXXDefinitions.fsi:19` is the in-corpus
  existence proof: `api XXXDefinitions`, `trait T` with `m(): () = ()`, and
  `object O` with `var f: ZZ32 = 3`. It is imported by nothing, so the hazard is
  latent rather than live -- and `scopeable` now CHECKS it, over the WHOLE api
  rather than the one declaration, because a rename rewrites every declaration
  that api contributed and one body anywhere in it would make the rewrite
  partial. An api with a body scopes nothing and keeps today's drop.
  Every type position IS walked: `extends`, `comprises`, `excludes`,
  static-parameter BOUNDS, an object's value parameters, field types, and each
  method's static-parameter bounds, parameter types and return type. That is
  every declaration-side `TypeRef` path `crates/ast/src/nodes.rs` has.
  `scopeable` HAS NO MUTATION ROW ON PURPOSE. Every program that would separate
  it is broken BOTH ways -- with the guard the api's own references break, and
  without it the body's do -- so the difference is which wrong answer, not
  pass versus fail. A row that can never fail reports SURVIVED forever; this is
  the same call `tuple_value`'s element check got, and it is documented at the
  site instead.
- **ONE api'S REFERENCES AND NOBODY ELSE'S.** The importer's own declarations,
  and declarations merged from other apis, still resolve to the winner. That is
  what makes this a NARROWING rather than a change of who wins.
- **A STATIC PARAMETER'S NAME SHADOWS, AND THE REWRITE HONOURS IT.**
  `rename_in_static_params` rewrites a parameter's BOUNDS and never the
  parameter's own name, so without a carve-out the two come apart: a parameter
  called `Comprehension` would keep its name while every use of it in the
  signature became `$FortressLibrary$Comprehension`, `mono`'s `Subst` is keyed by
  `param.name`, and the reference would stop binding to the static argument and
  silently resolve to the renamed trait. A SILENT WRONG TYPE. A method opens its
  own scope on top of its owner's. Measured at ZERO live instances -- the
  contested names are `ReductionWithZeroes`, `BigOperator`, `BigReduction`,
  `Comprehension` and `List`, and those files' static parameters are `R`, `L`,
  `I`, `O`, `E` and `F` -- so it is a guard against a SHAPE, not a fix for a
  symptom. `comprises.rs`'s `Row::is_own_static` and the renaming of
  `SELF_TYPE_PLACEHOLDER` to `$Self` are the two times this project has already
  been burned by it.
- **AN EXTENT IS DELIBERATELY NOT WALKED.** `TypeRef::Shaped` rewrites its
  ELEMENT and not its extent, the same cut `references` makes and for the same
  reason: an extent is a static ARGUMENT and names no type. A name written there
  is a `nat` parameter or a literal, and rewriting one would point a VALUE at a
  trait.

## THE BIGGER, PRE-EXISTING GAP THIS MEASUREMENT UNCOVERED

**6,921 collisions are the same parameter COUNT and a different BOUND VECTOR,
and they are silently flattened -- before this change and after it.** They are
not a regression and they are not touched. They are the next measured question.

The witnesses are in the very file this milestone targets:

    GeneratorLibrary.fsi:259  trait MonoidReduction[\R extends Any\] extends GeneralReduction[\R,R,R\]
    FortressLibrary.fsi:1864  trait MonoidReduction[\R\]            extends ActualReduction[\R,R\]

Same count, different bound count, **and a different supertype entirely**. The
importer's wins, so every merged FortressLibrary declaration extending
`MonoidReduction[\X\]` now inherits `GeneralReduction[\R,R,R\]` instead of
`ActualReduction[\R,R\]`. `CommutativeReduction` (:248 vs :1861),
`CommutativeMonoidReduction` (:272 vs :1869) and `MapReduceReduction` (:267 vs
:2099, which also differs in a FIELD NAME) are the same shape.

So `Library/GeneratorLibrary.fsi` CHECKS, and its merged hierarchy is a BLEND.
That is the flat namespace's pre-existing bargain and this change makes it
strictly better by one binding without claiming to close the class.

**Do not widen the trigger to `check_uniformity`'s predicate without measuring
it.** The top of the same-count list is `SequentialGenerator`, `Reduction`,
`Condition`, `Equality`, `Generator` and `StandardTotalOrder` at 1106-1110 files
EACH. Widening would scope those and reverse the layering the shipped libraries
depend on. The measurement is one line in the census and it is written down
here; the decision is not made.

## Known limits, stated rather than discovered later

- A THIRD api that references the name resolves to the WINNER, not to the losing
  api's version. Correct scoping would ask what that third api's own imports
  say. No corpus file needs it; widening without a measurement is how the
  25,615 shape-identical collisions get broken.
- A NAMED import (`.{a, b}`) that does not name the colliding declaration merges
  nothing for it, so no rename is recorded and that api's other declarations
  keep pointing at the winner. This is exactly today's behaviour; all measured
  cases arrive through on-demand imports.
- A SCOPED NAME REACHES A USER-FACING DIAGNOSTIC, and this is MEASURED rather
  than feared. A throwaway api declaring `object Zed[\A,B\]` and a trait
  extending it, imported by a file declaring its own `Zed[\A\]`, reports:
      `$zzprobeapi$Zed$ZZ32$ZZ32$e` is not a trait, so nothing can extend it
  and it renders that against the IMPORTER'S line -- the imported-span defect on
  top, again. ACCEPTED WITH THIS COMMENT rather than fixed: the precedent is the
  already-recorded "an arity diagnostic on a generic names the MANGLED SYMBOL,
  not the source name", which is the same class and predates this. Nothing
  renders a scoped name back to `<api>.<name>`, and something should.
- AN api NAME MAY BE DOTTED (`import Compiled5.a.{...}` names the file
  `Compiled5.a.fsi`), so a scoped name can carry a `.` as well as two `$`. Still
  unwritable -- `.` is a separator in this grammar and never part of an
  identifier -- but the leading-`$` note is not the whole story.
- A HEADERLESS api DECLINES TO SCOPE. `scopeable` gates on `api.is_api`, and 26
  of the corpus's 229 apis omit the header, so they keep today's drop. That is
  conservative and deliberate: nobody should read this fix as general.
- FOUR NAME-KEYED READ SITES ARE LATENT AND NONE FIRES TODAY, listed so the next
  person does not have to find them: `conform.rs`'s `same_type` and its trait
  lookup compare RAW names, so a renamed clause can never string-equal an
  exported api's spelling; `deviations.rs` matches a carrier by the literal name
  `NatParam`, so a scoped `NatParam` would silently stop a NAMED DEVIATION
  firing; `closure.rs`'s `sanitize` maps both `$` and `.` to `_` and its minted
  traits are matched by name, so the alphabet this widens is one it already
  collides in; and a newly-KEPT declaration's getters enter the global
  `accessors` set api-side. That last one is measured harmless HERE --
  `GeneratorLibrary.fsi:286-289` already declares `body`, `reduction` and
  `unwrap` as getters itself, so the set gains nothing -- which is luck, not
  design.
- `merged` AND `origins` ARE PARALLEL VECTORS kept in lockstep by hand across
  three push sites. `Vec<(String, Decl)>` would make that unbreakable at no
  cost. DELIBERATELY NOT DONE IN THIS COMMIT: it is a pure refactor with no
  behaviour change, and landing it costs a full re-verification cycle
  (sweep, IR, twenty gates, sixteen mutation tables) to prove exactly that.
  Do it the next time this function is opened for a real reason.
- `mangle_static`'s injectivity comment (`mono.rs:1392-1394`) SAID `$` cannot
  appear in an identifier so no source name can collide with a mangled one.
  `scoped_name` breaks that premise and the comment is rewritten with the
  argument that actually holds: a scoped name always carries TWO `$` and
  `mangle_static` never emits a LEADING one, so the one collision that would
  matter -- `mangle_static("$A", [Foo, ZZ32])` against
  `mangle_static("$A$Foo", [ZZ32])` -- needs a declaration named `$A`, which
  neither party produces.
- `shapes` keeps the LAST declaration's arity for a name declared more than once
  in one file. `Library/File.fsi:16-18` writes a factory FUNCTION and an object
  at one name; if the function lands last, `static_arity` is `None` and the
  rename declines. Deterministic, conservative, and it declines toward today's
  behaviour.
- A renamed loser is KEPT, so its header is newly subjected to
  `check_template_headers` (`mono.rs:1851-1900`), which a dropped declaration
  never faced. Combined with `closure`'s supertypes-only narrowing, a renamed
  declaration whose header names a type the importer never reaches would be a
  NEW `unknown type` on a file that compiles today. Traced on the one exit-0
  file where the rule fires: `SingleImport.fss` renames `List.fsi`'s
  `List[\E\]` to `$List$List` and the post-pass rewrites `:67`'s
  `LexicographicOrder[\List[\E\],E\]` and `:131`'s `MonoidReduction
  [\List[\E\]\]`; both of those merge in via the implicit import, so every
  name resolves and the file stays exit 0. Verified by name, and by
  byte-identical IR.

## What holds it

- `tools/apply-gate.sh`, `implicit_builtin_import`: two new assertions -- the
  fixture pair `fortressc/tests/scopedarityapi.fsi` + `scopedarityuse.fsi`, and
  `Library/GeneratorLibrary.fsi` itself.
- ONE FIXTURE HOLDS BOTH INVARIANTS AND EACH HAS ITS OWN MUTATION ROW.
  `scopedarityapi.fsi` declares `Zeroed[\R,L\]` and an object naming it at TWO
  arguments (the arity rule), AND a `trait Shadower[\Zeroed\]` that an object
  instantiates at `[\ZZ32\]` (the shadowing carve-out).
      `if mine == theirs {` -> `if true {`
        -> `Zeroed takes 1 static argument(s), found 2`
      `if !bound.contains(name.as_str()) {` -> `if true {`
        -> ``$scopedarityapi$Zeroed` is generic; write its static arguments`
  Both were RUN BY HAND before being written down, and the table then confirmed
  them: 72 rows, 0 survived, 0 unapplied. WHAT EACH ROW BREAKS IS ITSELF
  EVIDENCE. The arity row fails TWO assertions -- the fixture AND
  `Library/GeneratorLibrary.fsi` -- because that is the whole feature. The
  carve-out row fails ONLY the fixture, which is an independent confirmation of
  the "measured at ZERO live instances" claim: no corpus file has an api that
  both loses a name on count and uses that identifier as a static parameter. The guard is TWO `if`s rather
  than one `||` precisely so a table can reach it -- a row splits on `IFS='|'`.
- THE SHADOWING FIXTURE TOOK THREE SHAPES TO GET RIGHT, and the first two
  reported SURVIVED. A method return type is not checked in an undemanded
  template, and neither is an object's value parameter: `check_template_headers`
  (`mono.rs:1851-1900`) says so itself -- *"NAMES ONLY ... Members are not walked
  either"* -- and a captured name is still a name that RESOLVES, because the
  renamed declaration is in the component. The capture is only observable at
  INSTANTIATION, so the fixture has to instantiate.
- The same file's `API_FLOOR`, ratcheted 134 -> 135.
- The neighbouring `comprisesuser.fsi` assertion is the other direction: a
  SAME-count merged clause naming a name the file also declares still lets the
  file's own declaration win.
