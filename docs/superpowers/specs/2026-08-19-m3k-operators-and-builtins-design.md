# Fortress M3k: primitive operators and builtins

Date: 2026-08-19
Status: **spec**, written before the code.
Named as next by `2026-08-19-m3j-generic-and-functional-methods-design.md`,
where `compiler_tests/Compiled17.fss` stopped at `unknown name AND` and that was
recorded as library surface rather than a language rule.

Baseline measured on the M3j merge, not taken from a document: 1956 files,
**242 exit 0, 1714 exit 1, 0 anything else**. Parse **614**.

## What the corpus actually asks for

First blockers, by the name that failed:

| name | files | what it is |
|---|---|---|
| `print` | 12 | `println` without the newline |
| `assert` | 10 | the test library's equality assert |
| `ignore` | 5 | evaluate and discard |
| `AND` | 5 | conjunction |
| `NOT` | 4 | negation |
| `^` (parse) | 17 | exponentiation, and it fails in the PARSER |

`AND`/`OR`/`NOT` are the milestone's title and they are **not** the largest
lever on this board. The builtins are. Both get spiked and measured, and the
ratchet takes what was measured.

## The three rules the specification actually states

Read out of `basic/operators/precedence.tex` and `appendices/operators.tex`
rather than assumed:

* A relational operator has higher precedence than any boolean operator, and a
  **conjunctive** boolean operator has higher precedence than a **disjunctive**
  one. `AND` and `OR` are both **left associative**.
* `^` "has higher precedence than any other operator" -- above tight
  juxtaposition, in the same group as subscripting, **left associative**.
* Precedence in Fortress is **not a total order**. Groups with no stated
  relationship may not be mixed without parentheses, and that is a *static
  error*, not a default. `MAX MIN REM MOD GCD LCM CHOOSE per` are arithmetic
  operators with no stated relationship to `+` or `*`; `MAX MIN GCD LCM` are
  left associative and `REM MOD CHOOSE per` are **nonassociative**.

The third rule decides the shape of this milestone. A guessed precedence for
`a + b MOD c` type-checks under either reading and silently computes a
different number -- the exact class this compiler exists to refuse. So the word
arithmetic operators land **only** with their refusals built, or they do not
land and the note says so.

## Logical operators: the checker desugars, codegen does not change

`AND` and `OR` are short-circuit. The construct that already emits a
conditional branch, two blocks and a phi is `TypedExprKind::If`, so:

```
a AND b   ->  if a then b     else false end
a OR  b   ->  if a then true  else b     end
```

built **in the checker, after both operands are checked as Boolean with a
diagnostic that names the operator**. Desugaring in the *parser* would be
cheaper still and is rejected: the error for `1 AND true` would then talk about
an `if` condition, and a diagnostic that describes the wrong mechanism moves
files into the wrong bucket. That lesson is three milestones old.

This means codegen changes by zero lines for `AND`/`OR`, and "short-circuit
using basic blocks" is a claim with two witnesses rather than one: the emitted
IR must contain the branch and the phi, **and** a program whose right operand
prints must not print. The grep alone does not prove semantics.

`NOT` is a prefix operator -- the specification puts prefix operators above
every infix operator -- and it is **not** desugared. It gets a real `Target`
lowering to one `xor`, because three basic blocks for one instruction is worse
code at `-O0` and `-O0` is where this project checks its claims.

Boolean `=` and `=/=` come with it: an equality on `i1` is the same `icmp` the
numeric path already emits. Ordering comparisons on Boolean stay refused.

## Parser

```
expr        -> disjunction
disjunction -> conjunction (OR conjunction)*      left associative
conjunction -> comparison  (AND comparison)*      left associative
comparison  -> unchanged
unary       -> NOT unary | (+|-)? postfix
```

The lexer does not change. `AND`, `OR` and `NOT` already lex as `Ident`,
because a maximal run of uppercase letters is an identifier; the parser decides
they are operators by name. Changing the lexer changes how every file in the
corpus lexes, and there is no reason to.

Two consequences to get right:

* `starts_juxt_operand` must **refuse** the word operator names, or `a AND b`
  keeps juxtaposing as it does today and the new layer never sees it.
* A word operator is never gated on `fixity_at`. It cannot be glued on its left
  -- the lexer would have merged the letters into one identifier -- so reading
  its shape would report `Prefix` for `a AND (b)` and silently leave the
  operator unconsumed. Word operators consume on name match, always `Loose`.

## Measurement plan

Cumulative, each against the previous, each swept with the real driver:

| step | what |
|---|---|
| baseline | 242 |
| A | `AND`, `OR`, `NOT`, Boolean equality |
| B | `print`, `ignore`, `assert` |
| C | `^` |
| D | the word arithmetic operators, with their refusals -- or not at all |

Step C moves the **parse** floor, not just the compile floor: its 17 first
blockers are parse errors, where A's are checker errors. Both floors get
ratcheted to measured numbers.

M3h and M3j both showed a delta taken against an older baseline is biased
**low**, so the combined number is what the ratchet takes.

Non-negotiable, from the guidelines: every gate self-tests, and every new
assertion has a real mutation run against it and **shown to fail** before its
green is reported. No mutation may contain a `|`; every mutation pattern must
match exactly once, in every gate, not only the one being written.
