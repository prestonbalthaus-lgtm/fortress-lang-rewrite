# The fixity and precedence family: what landed, what did not, and the one decision that unblocks two of them

Five defects sit in §6.1 of the v1 gap analysis under "silent wrong output".
One of them is contained and has landed. The other four are not, and this
records *why* with measurements rather than with an effort estimate, because
"cheapest first" has already been wrong once in this project's queue.

> **Checked against the sibling branch, not assumed.** `spike/frontend-lexparse`
> has landed `71f43876a` "operator words, named infix, and **precedence as a
> partial order**", which sounds like it supersedes this document. It does not.
> Compiled with **that branch's own binary** (`fortress-wt-lexparse`, tip
> `2cef15e23`), every row below still reproduces unchanged:
> `a * b+c` → 50, `a b/c d` → 8, `a -b` → -15, `[1 2 3]` → length 1.
> Their partial order governs the operator TABLE the new operators go into; it
> does not touch the operators the compiler already had. So the analysis stands
> and `(a)` was not duplicated work — but the *Paren-marker decision* below
> routes to that agent, because they own the file it lands in.

Reproduced on this tree with `a = 8`, `b = 6`, `c = 2`, `d = 3`, all `ZZ32`:

| written | prints | 1.0 requires | reference |
|---|---|---|---|
| `[1 2 3]` | length 1, `[0]` = 6 | three elements | `aggregate.tex:120-121` |
| `a * b+c` | 50 | **a static error** | `precedence.tex:196-204` |
| `a+b * c` | 20 | **a static error** (mirror) | same |
| `a - b+c` | 4 | **a static error** | same |
| `a b/c d` | 8 | **72** = `a (b/c) d` | `precedence.tex:185-187` |
| `a/b c` | 0 | **2** = `(a/b) c` | same |
| `a/b/c` | 0 | **a static error** | `precedence.tex:62-64` |
| `a -b` | -48 | **a static error (infix)** | `opr-fixity.tex:68-69` row 2 |
| `a- b` | REFUSED | **legal (postfix)** | `opr-fixity.tex:68-69` row 3 |

---

## (a) `[1 2 3]` — LANDED

One function, `array_literal`, splitting a top-level `Expr::Juxt` after the
fact. Zero of the compiling corpus files write a juxtaposed array literal and
zero of `fortressc/tests` do, so the IR-diff acceptance over the whole set is
trivially clean — measured, not assumed.

Two sub-decisions were **recorded rather than guessed**, and both are
multi-dimensional-array questions rather than this one:

- an element run buried under an infix operator — `[a b + c d]` is one `Infix`
  over two `Juxt`s and a post-split cannot see it;
- whether a newline inside `[ ... ]` is a **row** separator. `skip_newlines()`
  currently eats them, and `arrayTest2.fss:17`, `mm64x.fss:208` and
  `Expr.Array.b.fss:20` all write multi-line matrices that depend on the answer.

---

## (b) `a * b+c` and (d) `a/b/c` — BLOCKED ON ONE DECISION, and it is the same one

Neither of these is hard as a *rule*. `a * b+c` is a comparison of two `Fixity`
values at the `infix()` build sites; `a/b/c` is "refuse a tight `Div` whose left
operand is an unparenthesised tight `Div`".

**What blocks both is that parenthesisation does not survive into the AST.**
`primary`'s `LParen` branch ends `self.expect(&Kind::RParen, "`)`")?; Ok(inner)`
— no wrapper node, and the inner expression keeps its own span while the outer
one is discarded. So:

    a * b+c    illegal      \  identical trees, both print 50
    (a * b)+c  legal        /

    a/b/c      illegal      \  identical shape, both print 0
    (a/b)/c    legal        /

Any tree-level check refuses both or neither. Span-widening is **not** enough:
the outer operator's own fixity is computed from token adjacency rather than
from spans (`fixity_at:232-239`), and `(a/b)/c` has the same left-nesting shape
as `a/b/c`.

> **THE DECISION: does the AST get a `Paren` marker?**
>
> It is small — one `Expr` variant or one `bool` on `Infix`, set in one place —
> and it unblocks two defects at once. It is also a `crates/ast` +
> `crates/parser` change, which is why it needs a decision rather than a commit:
> those crates are being rewritten by SPIKE-OPEXPR. The *checks* themselves can
> then live in `types`, where nothing yet reads `Expr::Infix.fixity` at all.
>
> This is the only item in this document that is blocked on a decision rather
> than on a milestone.

---

## (c) `a b/c d` — SPIKE-PRECGROUPS, not a check

This is not a rule to add, it is a ladder to restructure. `Expr::Juxt` records
**no fixity at all** (`nodes.rs:325-328`), so the tight-juxtaposition /
loose-juxtaposition distinction the spec's five-level list needs is destroyed at
parse time and cannot be recovered downstream. Fixing it means splitting
`juxtaposition` into a tight run and a loose run with `multiplicative`'s tight
`/` between them, adding a fixity to `Juxt`, and teaching
`types::juxtaposition` the difference.

Note the shape of the bug: `a b/c` prints 24 and **agrees with the spec by
luck**. `a b/c d` prints 8 where 72 is required, and `a/b c` prints 0 where 2 is
required. A partial fix that gets one of the three right is worse than none.

---

## (e) lopsided infix — SPLITS, and half of it is a deviation-reversal

**Half 1, `a -b` must be a static error: contained, ONE site, but it needs a
recorded decision first.** The only path that gives `a -b` its juxtaposition
reading is `starts_juxt_operand`'s `Some(Kind::Minus | Kind::Plus)` arm, reached
only from `juxtaposition`'s `while` loop — which by construction has already
parsed at least one operand, so the left context there is *always* a primary
tail, exactly the spec's error row.

Two reasons not to just do it:

1. **The current reading is asserted BY NAME in three parser tests** —
   `minus_glued_only_on_the_right_is_a_prefix_juxtaposed_with_the_left`,
   `minus_glued_only_on_the_left_is_postfix_and_out_of_subset`,
   `the_three_spacings_produce_three_different_trees` — and explained as
   intentional in the comment block above the arm. Reversing it is a decision,
   not a bug fix.
2. **`+=` and `-=` depend on that exact reading.** `x += 1` is
   spaced-left-glued-right, which *is* the prefix reading; the compound-operator
   test must stay first in that arm. This is the trap the design note named and
   it is the first thing the fix trips over.

And the site is `starts_juxt_operand`, which is **precisely where SPIKE-OPEXPR
must widen the word-operator stop set** for `a SUBSET b`. Racing there is the one
real merge conflict on the table. Hand it over as a rider on that spike.

**Half 2, `a- b` (the legal postfix reading) must stop being refused: DEFERRED
outright.** There is no postfix `-` in the subset, so making it legal requires
user-declared postfix operators — the operator table, i.e. SPIKE-OPEXPR itself.
`PostfixOperatorUnsupported` already says why it refuses.

---

## The silent class is narrower than it looks

Only `+` and `-` misparse silently. Every other lopsided infix already dies as a
parse error, because only `Minus | Plus` appear in `starts_juxt_operand`'s
prefix arm: `a *b`, `a /b`, `a <b`, `a =b` are all "expected `)`, found
Star/Slash/Lt/Eq" today. That is load bearing for the sizing above — half 1 has
a tiny blast radius by construction.

## Corpus footprint of (b), (c), (d) and (e) among the files that compile: zero

Measured per defect across the compiling set: no file writes a juxtaposed array
literal, a spaced-left-glued-right infix operator, or a glued fraction chain. So
none of these four is holding a corpus number hostage, and none can be justified
on a file count. They are conformance and silent-wrong-answer items, which is a
better reason, and they should be scheduled as one piece of work with
SPIKE-OPEXPR rather than picked off.

**And `^` must not be "fixed" while anyone is in here.** `2^3^2 = 64` is
correct: `^` sits above juxtaposition as its own level and is LEFT associative,
`precedence.tex:45-50`. `XXXTwoThreeTwo.fss` must fail for some other reason.
