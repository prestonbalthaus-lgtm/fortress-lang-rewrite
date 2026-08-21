# Frontend spikes: varargs, operator expressions, Unicode, import resolution

Branch `spike/frontend-lexparse`, ten commits off `f81f41ace`, in the worktree
at `/home/prestonalthaus/claude/fortress-wt-lexparse`. Nothing is merged and
nothing is pushed.

## Headline numbers, all measured from a clean tree against f81f41ace

|  | before | after |
|---|---|---|
| corpus compiles | 291 / 1956 | **309** / 1956 |
| corpus compiles, `--real` | 234 / 1588 | **252** / 1588 |
| api census, terminus-or-better | 23 / 126 | **32** / 126 |
| Library + CompilerLibrary top-level `.fsi` | 16 / 62 | **25** / 62 |
| lexer corpus | 1807 lex | **1845** lex |
| parser corpus | 732 parse | **839** parse |
| `cargo test` | 285 | **333** |

IR over the 291 files that compiled at f81f41ace: **289 byte-identical**. The
two that changed both went from "compiles" to "refused", and both are must-FAIL
tests (below).

Every commit carries its own list diff, IR diff and floor ratchet. Floors are
now lexer 1845, parser 839.

## The three things somebody else has to act on

### 1. `apply-gate.sh`'s COMPILE_FLOOR has no margin, and I must not touch it

The operator-word rule correctly turns two must-FAIL tests red:

* `parser_tests/XXXWrongTraitName.fss` — `trait XML end`. `XML` is an operator
  word (`lexical-structure.tex:1167-1172`) and cannot be a trait name. The
  filename says so.
* `parser_tests/XXXtest.OPR.name.fss` — `component XXXtest.OPR.name`. `OPR` is
  an operator word and cannot be a name segment.

That took the count 292 → 290, exactly the floor, and the branch has since
raised it to 309 for unrelated reasons. **The floor belongs at 289 measured from
f81f41ace, or those two files belong in a refuse-list.** Either is the gates
owner's call. Neither is a reason to soften the rule: those two files passing
WAS the bug, and the gap analysis's section 5 is the argument.

### 2. THE API CENSUS IS INFLATED, and now there is a number for it

`--resolve-imports` (new, opt-in, off by default) resolves an api's `.fsi` and
puts its types in scope. With it on:

* Library + CompilerLibrary `.fsi` reaching the terminus goes **25 → 18**.
* Whole census goes **32 → 25**.

Seven apis were passing ONLY because imports were inert:

    Map, CompilerLibrary/Map, PrefixMap, PrefixSet, Relation, SetClosure
        -> unknown type `UncheckedException`
    Reader
        -> unknown type `Generator`

The gap analysis section 2.1(b) predicted this and named `Relation.fsi`. It is
confirmed and it is six files wider than the prediction.

The corpus moves the other way with the flag on: 309 → 313, 252 → 256 on
`--real`. Six gained — the first real cross-file compilations in this compiler —
and two lost, one of them a must-fail.

**It is off by default on purpose.** Three agents are measuring against these
baselines right now, and flipping it on re-bases every one of them silently.
Turning it on is a decision for whoever owns the instruments.

### 3. A parser collision with `fix/semantics-correctness`

That branch carries `7ed9e4230 fix(parser): a where clause is parsed, and one of
its thirteen forms is implemented`. This branch also touches `skip_where`: it
made the clause reachable on a CONTINUATION LINE, taught it the second shape
(`where [\ bindings \]`, `NoNewlineHeader.rats:48-52`), and made a `where`
binding refuse the same kinds a static-parameter list refuses. Real parsing
supersedes the skip; the continuation-line half and the kind refusal do not.
Merge theirs and re-apply those two.

## What landed, by commit

1. **`f977222a5` varargs.** `...` after a parameter type by span adjacency, no
   lexer change. Static parameters between an enclosing operator's opener and
   its operand; an encloser with no operand; a closing half that need not match
   the opening half. `Param::varargs` is recorded and read by nothing — what
   `T...` lowers to is undecided, `functions.tex:174-182` says
   `HeapSequence[\T\]` and no such type exists. Corpus unchanged at 291 because
   the only two files it unlocked were must-fail `object O(x: ZZ32...)`, refused
   by `objects.tex:100`. Census 16 → 22 of 62.
2. **`f94527c52` named `end`.** `end Stream`, `end trait Stream`, `end component
   C`. `s` and not `w` is the whole disambiguation. The closing name must MATCH,
   which caught `XXXending.Name.fss` on the first run.
3. **`77aa8dcf8` continuation-line headers.** Static parameters, parameter list,
   return type, `where` and `throws`, each on the line below. Found three
   defects: `throws` never parsed at top level, `where` had a second shape, and
   the bracket form had to refuse the kinds M3d locks out.
4. **`0396e980c` the `=` guard.** `equals = "=" (!op)` is a BINDING-position
   rule that had been hoisted into the lexer. Moved to `definition_equals_at`.
5. **`ebf93f3c9` six operator characters and the vertical-line run.**
   `! ? ~ $ % @` had no arm at all; `|||` split into `BarBar`+`Bar`.
   `UnrecognizedCharacter` as a first blocker: 40 → 2.
6. **`71f43876a` operator words, named infix, precedence as a partial order.**
   The one that closes a silent wrong answer: `println(3 SUBSET 4)` printed 24.
   Lowers to a `Call` to a function named for the operator, which is what `opr`
   already lifts to — zero changes in `types` or `codegen`. Precedence is
   enforced as a PARTIAL relation for every pair involving a new operator.
7. **`aef49bf22` enclosing application.** `|x|`, `<|a, b|>`, `{a, b}`. Closes an
   exact declaration/expression asymmetry.
8. **`2cef15e23` the Unicode allowlist.** Eighteen codepoints, measured. One
   rule: a codepoint the grammar lists as an alternative SPELLING is that token;
   every other allowlisted codepoint is an operator character carrying its own
   text. Both decisions the task asked for are written up in
   `2026-08-21-unicode-allowlist-decision.md`.
9. **`ea19f877d` imports and exports name things.** Dotted and braced exports
   (+17 corpus files), the import list recorded, qualified type names, foreign
   imports refused by name.
10. **`7c51130c4` api resolution.** Above.

## What is deliberately NOT done, and why

* **An added operator used PREFIX or POSTFIX.** The twelve-row table classifies
  both; the expression level takes only `Infix`. `!x` and `x!` fail exactly as
  they did.
* **`+ - * / < > =` still read fixity from `fixity_at`, not from the table.**
  Moving them changes how programs that compile today are GROUPED. That is a
  measurement and a commit of its own, and it needs the lopsided and
  tight/loose blast radius over the 291 measured first — `precedence.tex:193-204`
  already forbids `a * b+c`, which compiles today and prints 50.
* **Retroactive precedence enforcement on existing-operator pairs.** Same
  reason, same gate.
* **`|\_/|` applied in expression position.** `|\3/|` reads `3/|` as a tight
  division before the closing run is consulted; that pair needs the operand
  grammar to know its own closer.
* **Aggregate literals COMPILING.** `<|1, 2, 3|>` now parses and resolves to
  `<|_|>`, then fails on arity against `opr <|[\E\] xs: E... |>` because the
  callee side of varargs is accept-and-ignore. It stays refused until what
  `T...` lowers to is decided.
* **One operator, one name.** `Symbol.rats:214-222` gives some operators an
  ASCII WORD spelling (`LE`, `NOT`, `OR`) alongside the symbol. Under the
  operator-word rule `a LE b` resolves to a function named `LE` while `a <= b`
  is `BinOp::Le`. That belongs with `SPIKE-OPRSTATIC`.
* **Merging an api's FUNCTION declarations.** Measured and reverted: it makes
  the checker demand a body for every one. Nine files lost. Types only.

## The instruments I used, and where they are

`.spike/` in the worktree, untracked (in `.git/info/exclude`):

* `measure.py sweep|census|ir|diff|irdiff` — the full 1956-file sweep is 1.1s.
  Same walk as `tools/triage.sh` and it reproduces 291/234 exactly. `FCFLAGS`
  passes extra driver flags, which is how the resolution numbers were taken.
* `opword.py` — which compiling files the operator-word rule would reclassify.
  It said 14, of which 2 mattered, and both were must-fail. That measurement is
  why commit 6 could be planned rather than discovered.
* `nonascii.py` — every non-ASCII codepoint outside comments and strings. It
  reproduces the gap analysis's 18 exactly.
* `fsstrip.py` — the comment and string stripper both of those share. It mirrors
  `lexer/src/raw.rs:51-92`, including that `(*)` is INERT inside a block
  comment — the same trap SYNTAX_GUIDE.md's counter hit.

I did not run or touch `tools/triage.sh`; another agent is editing it and its
cache is shared state.
