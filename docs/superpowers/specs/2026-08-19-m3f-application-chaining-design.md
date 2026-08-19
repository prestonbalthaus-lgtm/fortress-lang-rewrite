# Fortress M3f: juxtaposition as function application, and chained comparison

Date: 2026-08-19
Status: **design, for review. Nothing implemented.**

Two independent language rules, both already written down in Specification 1.0,
both measured before being scoped. `println "Hello"` becomes a call. `a < b < c`
becomes a chain. Neither needs a new type, a new runtime shim, or a change to
`Type`.

## The measurement first, and this time it picked the milestone

M3e ended with 428 of the 1780 files that lex parsing, and 151 of 1956
compiling end to end. The remaining work was scouted by spiking **eleven**
candidate constructs behind an environment switch, one switch per construct, and
running the real corpus test on each — the method M3d and M3e established, now
run against a field rather than a single guess.

The blocker histogram disagreed with the measurement for the fourth time:

| construct | first-blockers | measured parse delta | conversion |
|---|---|---|---|
| chained `=` | 51 | **+49** | 96% |
| `getter`/`setter` | 131 | +31 | 24% |
| top-level value declarations | 113 | +31 | 27% |
| `self` parameters | 46 | +25 | 54% |
| `import java com…` | 34 | +25 | 74% |
| object expressions | 19 | +13 | 68% |
| dotted export names | 21 | +12 | 57% |
| untyped parameters | 32 | +9 | 28% |
| `var` bindings | 105 | **+6** | 6% |
| `opr` declarations | 97 | **+5** | 5% |
| `<\| \|>` enclosing operators | 30 | +5 | 17% |

`var` and `opr` carry the two largest blocker counts on the roadmap's shortlist
and are worth eleven files between them. Chained comparison is a footnote in the
histogram and converts at 96%.

**And the compile metric turned out to be a different lever entirely.** Of the
297 files that parse and do not compile, the largest single cause is
`unknown name println`, 48 files — which is not a missing builtin, because
`println` exists. It is `println "Hello, world!"`: **juxtaposition as function
application**. Spiked in the checker and swept with the real driver over all 1956
files, that alone is **compile 151 → 181, +30**. It is the only lever measured to
move the compile metric at all.

M3f is those two. Everything else in the table is real and none of it is next.

## What the specification says

**Juxtaposition**, `basic/operators/juxtameaning.tex:23-26`:

> if the left-hand-side expression is a function, juxtaposition performs function
> application; otherwise, juxtaposition performs the `juxtaposition` operator
> application.

and `:36-46`, which is the rule this design turns on:

> All we need to know is whether each element of a juxtaposition has an arrow
> type. There are actually three legitimate possibilities for each element …
> (a) it has an arrow type … (b) it has a type that is not an arrow type …
> (c) it is an identifier that has no visible declaration, in which case it is
> considered to be a function element.

**Chaining**, `basic/operators/chained-multifix.tex:16-34`:

> Certain infix mathematical operators that are traditionally regarded as
> *relational* operators … may be *chained*. … `A ⊆ B ⊂ C ⊆ D` is treated as
> being equivalent to `(A ⊆ B) ∧ (B ⊂ C) ∧ (C ⊆ D)` except that the expressions
> `B` and `C` are evaluated only once …
>
> Fortress restricts such chaining to a mixture of equivalence operators and
> ordering operators; if a chain contains two or more ordering operators, then
> they must be of the same kind and have the same sense of monotonicity …
>
> This transformation is done before type checking.

Three commitments fall straight out of that last paragraph and are honoured
below: evaluate the interior operands once, restrict the mixing, and desugar
before the checker runs.

## 1. Juxtaposition as function application

### 1.1 The classification, and why it is small here

Rule (a) is **unreachable in this subset**. Arrow types parse and are refused by
the checker (M3e), so no value can have an arrow type, so no element can be a
function element by having one. Rule (c) is the whole rule that remains.

An element is a **function element** iff all three hold:

* it is an unparenthesized identifier — `Expr::Var`;
* it is **not** bound as a local or a parameter, `Checker::lookup` returns `None`
  (`crates/types/src/lib.rs:544`);
* it names a declared function or a builtin — present in `Checker::functions`
  (`:72`) or an `MpiOp`, or it is `println`.

Everything else is a non-function element and reaches the existing juxtaposition
handling unchanged.

The second condition is doing real work and it is the spec's, not an invention:
it is what stops a local named `f` from turning `f x` into a call when the user
meant multiplication. The spec spends a paragraph on exactly this hazard
(`juxtameaning.tex:47-64`), where a variable and a functional method share a name.

### 1.2 Binary only, and that is a measurement not a shortcut

`Juxt[f, x]` where `f` is a function element becomes `f(x)`.

`Juxt[f, x, y, …]` with a leading function element is a **diagnostic**,
`JuxtapositionNotBinary`, naming the element count and saying the reassociation
rules are not implemented.

The spec's reassociation is genuinely involved — break into chunks wherever a
non-function is followed by a function, group non-functions left-associatively,
group each chunk right-associatively, plus two separate static-error rules about
unparenthesized identifiers that could be functional methods
(`juxtameaning.tex:70-111`). It was spiked anyway, both ways, and swept:

| | compile end to end |
|---|---|
| master | 151 |
| binary application only | **181** |
| binary + n-ary application | **181** |

The n-ary case is worth **zero files**. It is not built, and the diagnostic says
so rather than pretending.

### 1.3 Where it goes

`Checker::juxtaposition` (`crates/types/src/lib.rs:1058`), as the first thing it
does, before the literal probing. It cannot go in the parser: the rule asks
whether a name is a local, and only the checker has scopes.

Ordering matters and is deliberate. The existing juxtaposition already means
string concatenation and numeric multiplication; the application check runs first
and fires only on a leading function element, so every juxtaposition that works
today still takes the same path. The existing `UnresolvableJuxtaposition`
(`crates/types/src/error.rs:21`) keeps its meaning for everything that is not an
application.

## 2. Chained comparison

### 2.1 Which operators chain

| class | operators | sense |
|---|---|---|
| ordering | `<`, `<=` | increasing |
| ordering | `>`, `>=` | decreasing |
| equivalence | `=`, `=/=`, `===` | — |

A chain may mix equivalence operators freely with ordering operators of **one**
sense. Two senses is a compile error naming both operators and their spans.
`a <= b < c = d` is legal; `a <= b > c` is not. The corpus writes
`zero<=zero<one=one<two<=two`, so the legal mixed form is not hypothetical.

### 2.2 `=` becomes a comparison operator in expression position

`comparison_op` (`crates/parser/src/lib.rs:1237`) gains `Eq`. This is safe
because every definition site consumes its own `=` before the expression grammar
can see it: `fn_decl` takes the body's `=`, and `try_binding` takes a binding's.
An `=` that reaches `comparison()` is therefore genuinely equality, not a
definition the parser mislaid.

### 2.3 The desugar

In `comparison()` (`:704`), which already loops over comparison operators and
left-associates them. Collect the operands and operators; if **two or more**
operators were collected, emit:

```
a < b < c

  do
    $chain0 = a
    $chain1 = b
    $chain2 = c
    if $chain0 < $chain1 then $chain1 < $chain2 else false end
  end
```

Point by point against the specification:

* **Evaluated once.** One binding per operand, in source order. The spec calls
  this out explicitly and it is the only part of chaining that is observable
  from inside the language.
* **Short-circuit without a new operator.** Nested `if` gives `∧` for free. This
  subset has no `AND`, and adding one is not part of this milestone.
* **Before type checking.** It is a parser-level AST rewrite, so the checker
  receives an ordinary block and never learns that chaining exists.
* **No collisions.** `$` cannot appear in a source identifier — the same
  property `mangle_type` already relies on — and the counter lives on the
  `Parser`, so nested chains get distinct names.

A **single** comparison operator is left exactly as it is. `a < b` produces no
block and no temporaries, so nothing about existing generated code changes.

### 2.4 What this does not do

Multifix operators. `chained-multifix.tex:42-52` also describes non-chaining
operators being treated as multifix when the same operator separates three or
more operands. That is a separate rule about operator *definitions*, this subset
has no user-defined operators, and it is out.

## 3. Scope boundary

| | |
|---|---|
| **Implemented** | binary juxtaposition application under spec rule (c); chained comparison over `<`, `<=`, `>`, `>=`, `=`, `=/=`, `===` with evaluate-once semantics and the mixing restriction |
| **Refused with a diagnostic** | a juxtaposition of three or more elements led by a function element; a chain mixing two ordering senses |
| **Out** | the spec's n-ary reassociation algorithm (measured at +0 files); multifix operators; an `AND`/`OR` operator; functional methods; `print`, `assert`, `ignore` and the rest of the builtin surface |

`print` and `assert` are named in the "out" row on purpose. They appear in the
missing-name histogram alongside `println` and it would be easy to assume this
milestone covers them. It does not — they are library surface, not a language
rule, and they belong with whatever milestone brings in the standard library.

## 4. Diagnostics

* `TypeError::JuxtapositionNotBinary { span, found: usize }` — a juxtaposition
  led by a function element with `found` elements; the reassociation rules are
  not implemented.
* `ParseError::ChainedOperatorsDiffer { span, first: &'static str, second: &'static str }`
  — a chain mixing two senses of ordering, naming both.

## 5. Gate

`tools/apply-gate.sh`, `--selftest` and `--mutate`, self-testing its assertions
before it runs anything.

* `println "Hello"` compiles, links, runs and prints the right bytes.
* a user function applied by juxtaposition, `f x`, reaches `f`.
* **a local shadowing a function name is not application.** `f = 3` then `f x`
  stays a juxtaposition. This is the guard rule (§1.1) and it is the one that
  silently changes meaning if it is wrong.
* a three-element juxtaposition led by a function element exits 1.
* **the middle operand of a chain is evaluated exactly once**, proved by a side
  effect — a function that increments a counter, used as the middle operand, with
  the counter asserted afterwards. Nothing else can see this property from inside
  the language.
* `a <= b > c` is refused, naming both operators.

Mutations, and a gate is not trusted until it has refused:

1. Drop the `lookup(name).is_none()` guard. Expect the shadowing case to become a
   call.
2. Desugar by duplicating the operand instead of binding it. Expect the
   evaluate-once counter to read 2.
3. Drop the sense check. Expect `a <= b > c` to compile with status 0.

## 6. Ratchets, and a new one

* Parser floor 428 → whatever the implementation measures. Chained comparison
  measured **+49** at parse in the spike, so about 477, but the spike had no
  desugar and the real one may differ; the measured number wins.
* Lexer floor 1780, untouched. Neither part of this milestone changes the lexer.
* **The compile metric gets a floor for the first time.** It is the headline
  number of this milestone and nothing currently guards it — the parser corpus
  test stops at the parser. The gate records the count from a full driver sweep
  and fails if it drops. Measured target is 181 from juxtaposition; chained
  comparison's contribution to it is **not yet measured** and will be reported
  rather than predicted.

## 7. What is measured and what is not, stated plainly

**Measured, with the real driver over all 1956 files:** juxtaposition
application is compile 151 → 181, and the n-ary variant adds nothing.

**Measured, with the real corpus test:** chained comparison is parse 428 → 477.

**Not measured:** what chained comparison does to the compile metric. The
scouting spike was blunt — `Eq` added to the comparison table with no chaining
and no desugar — so any compile number taken from it would have been fiction.
M3e's design predicted its compile movement and was wrong by a factor of three in
the good direction; this one declines to predict and will report.
