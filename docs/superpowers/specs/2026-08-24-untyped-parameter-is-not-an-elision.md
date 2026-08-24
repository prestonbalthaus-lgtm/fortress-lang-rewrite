# An untyped parameter is not an elided name

**Landed 2026-08-24** as phases H and I of the parser offensive.
Commits `e3c358b76` (H) and `951b14881` (I).

## The two rules the corpus was being told apart wrongly

`Parameter.rats:96` and `:104` are two different productions and the frozen
grammar keeps them apart on purpose:

    Param    ::= BindId (w IsTypeOrPattern)?          -- a CONCRETE declaration
    AbsParam ::= BindId w IsType | Type               -- an ABSTRACT one

So on a declaration **with a body** a parameter's TYPE may be omitted and its
NAME may not; on one **without** a body the name may be elided and the type may
not. `functions.tex:384-385` is the prose half of the second rule and says both
halves in one sentence, which is how they came to be implemented as one.

They are not one. Before this change every typeless parameter on a bodied
declaration was reported as an illegal *elided name*, citing
`functions.tex:384-385` -- a rule about abstract declarations -- at 35 corpus
programs that were not attempting elision at all, plus 7 more through the
object-field wording.

### And elision is not even reachable on an object's value parameters

`TraitObject.rats:184-185` is `ObjectValParam ::= ( (w ObjectParams)? w )`, and
`ObjectParams` is built from the same `Param` a function's parameter list uses.
There is no `AbsParam` alternative in reach, so `object O(x)` is an untyped
FIELD and can be nothing else. The message now says `field`.

## The discriminator is the written SHAPE, and it is exact

A bare identifier is the **only** shape the two readings share. `List[\T\]`, an
arrow and a tuple are not `BindId`s, so nothing can read them as a name, and a
declaration that writes one where a name belongs really is eliding. So:

| written | reading | message |
|---|---|---|
| `f(v) = 2` | untyped parameter | blocked on inference |
| `object O(x)` | untyped field | blocked on inference |
| `f(List[\T\]) = 2` | elided name | `functions.tex:384-385`, unchanged |
| `object O(List[\T\])` | elided name | FIELDS wording, unchanged |
| `bar(ZZ64): ZZ64` in an api | elided name, LICENSED | compiles |

No case analysis on capitalisation: `f(ZZ32) = e` reads as an untyped parameter
NAMED `ZZ32`, because that is what `Param ::= BindId ...` says it is.

**All 42 corpus files are the bare-identifier shape.** The elision branch that
survives has ZERO corpus exercisers and is a backstop, held by
`badelidedbody.fss` and `badelidedfield.fss`, which are rewritten to the
structured shape that actually reaches it.

## Why the new message names inference, and why nobody should schedule it

Verified against the frozen specification rather than quoted:

* `basic/inference.tex` is **27 lines** and its entire chapter is one `\note{}`:
  "This chapter will include the Fortress static type inference mechanism."
  `:19-22` records an unresolved **circular dependency between inference and
  juxtaposition disambiguation** -- you need arrow types to disambiguate a
  juxtaposition, and you need inference to know them.
* `basic/components/type-inference.tex:15-16` makes inference a procedure over a
  whole program, adapted per component, and `:45` performs it "over all program
  constructs that still include elided types" only after the component has been
  expanded with every imported api's declarations.

So this is not a parser gap with a bounded fix. It is whole-component inference
against a chapter the specification never wrote, in a compiler whose `Type` is a
`Copy` enum with no variable case. Refused by name is the correct end state.

## Phase H, and why it is in the same document

`LocalDecl.rats:75` is `Id (w StaticParams)? w ValParam` with `w = Whitespace*`,
so `g (x: ZZ32): ZZ32 = e` is one declaration. The block-level probe required the
parenthesis to be GLUED, which read the spelling and not the grammar. Relaxing it
is one conjunct and it moved exactly ONE corpus file, `funny.fss:29` -- so the
local-function bucket is 38 -> 39 and **39 is a true ceiling rather than a
glued-spelling one**, which was the whole deliverable.

A newline still ends the header for free: a newline is a token here, so
`peek_ahead(1)` is `Newline` and not `LParen`. 1.0's `w` spans newlines and would
join two block elements into one declaration; no corpus file writes that.

**The loss class is real and measured at zero.** A block-level `a (b) = 6` -- a
discarded juxtaposition equality -- is now the declaration reading and is
refused. 1.0's own `BlockElem` is an ordered choice with `LocalVarFnDecl` FIRST,
so the declaration reading is the oracle's behaviour.

## What both phases cost, measured

corpus 579 -> 579 across both. ZERO gained, ZERO lost, the rc=0 set identical
file for file, and all 579 compiling files emit **byte-identical IR** -- the
instrument self-tested both ways on a changed integer LITERAL, because a
comment-only edit does not change emitted IR and proves nothing. parse
1161 -> 1161. 43 files moved message: 1 from H, 42 from I.

## Two instruments were found lying, both quietly

* `tools/triage.sh`'s `untyped-parameters` rule still matched
  `expected `:`, found RParen`, the message G7 stopped emitting two milestones
  ago. The bucket reported **zero** for 42 files. A triage rule does not go red
  when it goes stale; it goes quiet. Re-ground it with `--raw` after any
  milestone that renames a diagnostic.
* `README.md`'s metric table had drifted several milestones behind the ratchets
  it cites -- parse 1113 against 1161, objects 435 against 443, apis 134 against
  136, oracle 356 against 359. Re-measured by running the instruments.

## The harvest that only appears under `--all-targets`

Adding `ParameterTypeInferred` forced exactly one arm, in
`crates/parser/tests/corpus.rs`. `cargo build` does not build tests, so the
E0004 is invisible to it. Run `cargo clippy --workspace --all-targets` and
`cargo test`, not `cargo build`, before believing a variant addition is done.

---

# Addendum: phase J, `end` elided from a parenthesised `if`

**Landed 2026-08-24**, commit `5e9f5d5bc`. Filed here because it is the same
session and the same lesson: the bucket said 19 and the answer is 3.

`if.tex:71-73`: "The reserved word `end` may be elided if the `if` expression is
immediately enclosed by parentheses. In such a case, an `else` clause is
required." 1.0 carries it as its own production, `DelimitedExpr.rats:40`:

    ( w if w GeneratorClause w then w BlockElems (w Elifs)? w Else (w end)? w )

`Else` is mandatory there and only `end` is optional -- the prose's second
sentence written into the grammar. Both halves are implemented, and the second
is what stops the first accepting programs 1.0 refuses: an `if` with no `else`
has type `()`, so the missing branch would read as a void statement.

## Three things this cost that the design did not predict

**The `19` was not one feature and was not even all `if`.** Two of the nineteen
files under `expected a newline or `;`, found RParen` -- `Compiled2.j.fss:17` and
`Compiled2.p.fss:17` -- have an UNBALANCED closing parenthesis and are correctly
still refused. Of the 17 that were `if` files, 3 compile and run, 10 reach a
checker error, 4 move from one parse error to another. Hence parse moves 13 and
objects move 3.

**Every block inside the if-parse needs `RParen`, not just the `else` arm.** The
first design added it to the else sets only. `(if b then 1)` then runs the THEN
block onto the closing parenthesis and reports the generic `expected a newline or
`;``, which is *the message these files already gave* -- so the named refusal is
unreachable and its fixture would have been written around the generic message,
losing the point entirely. Caught by probing the no-`else` shape before writing
the fixture.

**`saw_else` must be tracked during the parse, not read off the tree.** An `elif`
chain fills `else_branch` with the nested `if`, so "does this node have an
`else_branch`" is `Some` for `(if a then 1 elif b then 2)` -- which has no `else`
at all and is exactly the program the refusal exists to catch.
`badifnoendelif.fss` is that program and is a separate fixture for that reason.

## The licensing test

"Immediately enclosed" is decidable at `if_expr` itself: look backward from the
`if`'s own token, past any newline, for an `LParen`. No threading through the
expression parser, and it covers both sites at once -- the parenthesised atom and
a glued CALL's argument list, which are one production in 1.0 and two here.

It correctly refuses `(1 + if c then 2 else 3)` (the `if` follows `Plus`),
`f(1, if ...)` (follows `Comma`), and `(if a then 1 else if b then 2)`, whose
inner `if` follows `else` and so still needs its own `end`. That last case falls
out of the test rather than being special-cased.

## What was read rather than counted

`ifTest.fss` is the corpus's own test for this construct and it exercises both
branch directions: `1=1` takes the `then` arm, `0=1` takes the `else` arm. It
prints `a pass` / `b pass` and never a `fail`. All three gained binaries were
built and run before the gain was reported, which is G7's rule.

The 579 files that already compiled emit **byte-identical IR** against the
pre-H baseline across all three of H, I and J.
