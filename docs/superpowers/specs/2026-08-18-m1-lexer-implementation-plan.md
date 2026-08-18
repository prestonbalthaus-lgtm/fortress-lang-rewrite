# M1 lexer implementation plan

Date: 2026-08-18
Design: `2026-08-18-fortress-m1-design.md`
Status: step 1 complete and verified, steps 2 to 5 pending decisions

## Provenance

The lexical rules below were derived from the legacy sources rather than from
memory: six parallel agents over `ProjectFortress/src/com/sun/fortress/parser/*.rats`
and `Specification/*.tex`, producing 178 rules, then three adversarial verifiers
that refuted 21 of them and found 27 gaps, then a synthesis pass. Every rule
carries file:line evidence.

I independently re-checked the four load bearing claims against the files:

* `Space = " " / "\f" / "&" s Whitespace / InvalidSpace / NoNewlineComment`
  at `Spacing.rats:26-32`. Confirmed.
* `compOp = "===" / "=/=" / "<=" / ">="` at `Symbol.rats:139-143`. Confirmed.
  `==` is not an operator in this language.
* `!(rightEncloserMulti) "///"` and `"//"` at `Symbol.rats:171-172`. Confirmed,
  these are operators and never comment openers.
* `Keyword.rats:21-49` contains exactly 90 distinct reserved words. Confirmed by
  count.

Unverified rules are marked where they matter.

## Step 1: workspace scaffolding and toolchain

COMPLETE. `cargo check` and `cargo clippy --workspace --all-targets` both clean
under rustc 1.97.1.

* Six crates: `ast`, `lexer`, `parser`, `types`, `codegen`, `driver`.
* `[workspace.lints]` denies `unwrap_used`, `expect_used`, `panic`, `todo`,
  `unimplemented`, `indexing_slicing`, `integer_division`, and `unsafe_code`.
  `clippy.toml` exempts test code only.
* Verified the deny is load bearing rather than decorative: an `.unwrap()` planted
  in `fortress-ast` failed the build with exit 101 and
  `-D clippy::unwrap-used`. Probe removed, tree re-verified clean.
* `rust-toolchain.toml` pins channel 1.97 with clippy and rustfmt. Cargo enforces
  the same floor through `workspace.package.rust-version` regardless of rustup.
* Exit codes are fixed now because the test harness depends on them: `1` for a
  user diagnostic, `70` for a compiler bug.

### Outstanding gate, blocks codegen not the lexer

Fedora 44 ships LLVM 22.1.8. `inkwell` binds specific LLVM versions by feature
flag and lags upstream, so the system LLVM is almost certainly unusable. The box
has `llvm11-devel` through `llvm21-devel` available as side installs. Before any
codegen work: determine inkwell's supported range, install the matching
`llvmNN-devel`, and pin `LLVM_SYS_NNN_PREFIX`. The `codegen` crate deliberately
declares no dependencies until that is settled.

## Step 2: token set

`Gap` is a field on every token, not a token variant:

```
Gap = None | Space | Break
```

`None` means zero whitespace characters since the previous token. `Space` means a
whitespace run with no surviving line break. `Break` means a run containing at
least one. The three-way split is forced by `Spacing.rats:92-96`, which defines
four separate predicates (`w`, `wr`, `s`, `sr`) over exactly this classification.

Token kinds for M1:

| Group | Variants |
|-------|----------|
| Layout | `Newline` (the only whitespace bearing token), `Eof` |
| Keywords | `KwComponent`, `KwExport`, `KwEnd`, `KwDo`, `KwIf`, `KwThen`, `KwElse`, `KwElif` |
| Reserved | `Reserved(&str)` for the other 82, rejected by the parser with "not in the M1 subset" |
| Names | `Ident(&str)` |
| Literals | `IntLit`, `FloatLit`, `StrLit`, `True`, `False` |
| Delimiters | `LParen`, `RParen`, `Comma`, `Semi`, `Colon`, `Dot` |
| Operators | `Eq`, `ColonEq`, `Plus`, `Minus`, `Star`, `Slash`, `Lt`, `Gt`, `Le`, `Ge`, `EqEqEq`, `NotEq`, `SlashSlash`, `SlashSlashSlash` |

There is no `Juxtaposition`, `Apply`, `Call`, `Whitespace`, `Indent`, `Dedent` or
`Void` token. Juxtaposition has no token at all; the parser infers it from
adjacency plus gap class.

Reserve all 90 words from day one. Reserving late is a breaking change, and the
corpus test needs the full set to classify identifiers correctly.

`run`, `Executable`, `widen`, `println`, `ZZ32`, `ZZ64`, `RR64`, `Boolean` and
`String` are NOT reserved. They are ordinary identifiers, which is what makes the
acceptance program legal. Note `widens` IS reserved while `widen` is not.

### Traps in this step

* `===` and `=/=` are single tokens and must beat `=`, `<` and `>` in maximal
  munch. There is no `==`.
* `:=` must be one token or `j:=x` mislexes as `Colon` `Eq`.
* `//` and `///` are operators. A lexer that starts a line comment on `//` is
  wrong. Comments are `(* *)`, and `(*)` starts a line comment.
* `star = !("**") "*"` and `Op ... !(Symbol)` where `Symbol = [+]`. `+` is the
  only character with an adjacency restriction; the wider class is commented out
  at `Symbol.rats:137`.
* `Eq` is emitted only when the next character is not an operator character
  (`Symbol.rats:201`). A lone `=` serves both definition and equality; the parser
  disambiguates.

## Step 3: newline and whitespace layer

Two layers. Layer A is a character DFA that consumes one maximal whitespace run
and returns `Gap{space, brk}`. Layer B is three lines that turn `brk` into an
emitted `Newline`. Layer B needs no bracket depth tracking and no previous token
heuristics, which is what makes it equivalent to the reference grammar.

### Layer A, gap scanner

State GAP, entering with `space=false, brk=false`:

| Input | Action |
|-------|--------|
| `' '`, U+000C | consume, `space=true` |
| `\r\n`, `\r`, `\n`, U+2028, U+2029 | consume, `brk=true` |
| `\t`, U+000B, U+001C..U+001F | `LexError`. `Spacing.rats:34-42` logs and continues; M1 fails fast |
| `&` | consume, go to AMP |
| `(*` | run COMMENT, `space=true`, `brk \|= comment_broke` |
| anything else, EOF | stop, return `Gap` |

State AMP, reached after consuming `&`. This is the line continuation mechanism
and it is the single most surprising rule in the language.

`Space = ... / "&" s Whitespace` with `s = Space*`. Rats! repetition is
possessive, so `s` consumes every reachable Space and the required trailing
`Whitespace` can only be a `Newline`. The production is therefore exactly
`"&" Space* Newline`.

| Input | Action |
|-------|--------|
| `' '`, U+000C | consume, `space=true`, stay AMP |
| line terminator | consume, `space=true`, `brk` UNCHANGED, return to GAP |
| `(* ... *)` containing a terminator | consume, `space=true`, `brk` unchanged, return to GAP |
| `(* ... *)` with no terminator | consume, stay AMP |
| anything else, EOF | `LexError` |

AMP never sets `brk`. That is its entire purpose: `&` at end of line cancels the
statement terminator, turning a break into a space.

Verified against `ProjectFortress/tests/ampersand.fss:19-20`, which reads
`assert(9, 3&` / newline / `x)` with `x = 3`. It asserts 9, so `3 x` is a loose
juxtaposition, meaning the break became a space.

COMMENT sub-machine: block comments `(* *)` NEST and track depth. `(*)` opens a
line comment. Inside a line comment, `(*)` is inert, an unmatched `*)` stops the
scan WITHOUT consuming, and tabs are legal. `Library/CompilerLibrary.fsi:168` has
two `(*)` on one line and a scanner treating the second as a nested opener errors
on a shipped library file.

Deliberate divergence from the reference, both unobservable on the corpus (zero
U+2028/U+2029 in any `.fss` or `.fsi`) and in the ASCII M1 subset: the reference
uses `Character.CONTROL` to classify comment content, so U+2028 (Zl) and U+2029
(Zp) slip through and a comment containing only those classifies as space rather
than newline. M1 uses the specification's line terminator set uniformly.

### Layer B, terminator emission

Carry `prev_significant: Option<Kind>`. Before pushing the next significant token
`T`:

```
emit Newline  iff  gap.brk
              and  prev_significant.is_some()   (suppress leading break)
              and  T is not Eof                 (suppress trailing break)
then push T with gap_before set from the run
```

At most one `Newline` per run however many terminators it held. Nothing else. No
bracket depth, no "suppress after a dangling operator", no "suppress before
`end`". Those are parser responsibilities and pushing them into the lexer breaks
the verified continuation behaviour.

### The parser contract, which is the other half

The token stream is unusable without this, so it goes in the plan even though the
parser is a later milestone.

* Where the reference grammar writes `w` or `wr`: SKIP `Newline`. That one rule
  covers newlines inside parens, around `then`/`else`/`end`, after `do`, and both
  sides of a top level `=`.
* Where it writes `br`: REQUIRE a separator, which in M1 is exactly
  `Newline+ | Semi Newline*`. `a;;b` and `a\n;b` both fail, matching the
  reference, because `br = nl / s semicolon w` consumes exactly one semicolon.
* Where it writes `s` or `sr`: do NOT skip `Newline`. Its presence ends the
  construct. This is the whole of statement termination.
* Operator continuation, verified against `Library/String.fss:129-131` and
  `Library/Avl.fss:394-395`: a newline may FOLLOW a loose infix operator but never
  PRECEDE it.
  `a +` newline `b` is one statement. `a` newline `+ b` is two.
* `j:ZZ64` newline `= widen(20)` is NOT a local declaration.
  `LocalDecl.rats:159` is `VarMayTypes s equals w NoNewlineExpr`; `s` forbids a
  newline before `=`. Top level function declarations differ (`w equals w`).

## Step 4: lexer interface and spans

```rust
pub fn lex(source: &str) -> Result<Vec<Token>, LexError>
```

Locked. `Token` carries `kind`, `span: Span`, and `gap_before: Gap`. `Span` is
byte offsets, already defined in `fortress-ast`. Line and column are derived on
demand by the diagnostic renderer rather than carried per token, because the
lexer runs over the whole 1846 file corpus and per token line counting is waste.

`LexError` carries a `Span` and is one of the variants already scaffolded, plus
whatever the decisions below add.

Byte offsets rather than char offsets: the corpus contains non-ASCII (U+202F
inside numerals is legal and live at `ProjectFortress/tests/NumeralTest.fss:47`),
so the renderer must handle multi byte characters when it converts a span to a
column. That is the renderer's problem, not the lexer's.

## Step 5: corpus verification

Harness feeds all 1846 `.fss` and `.fsi` files through `lex` and asserts no panic
and no infinite loop.

`Err` is a PASSING outcome. The criterion is "does not panic", and the M1 subset
cannot lex radix numerals, character literals or non-ASCII operators, all of which
are live in the shipped library. Track the `Ok` rate as an informational metric,
not a gate.

Additional required tests:

* Every rule in step 3 gets a unit test with the source snippet inline.
* `ampersand.fss` lines 19-20 specifically, asserting the break became a space.
* `Library/CompilerLibrary.fsi:168`, the double `(*)` line.
* `ProjectFortress/demos/Cfa.fss:119`, which contains `(*` and `*)` inside a
  string literal. A scanner that looks for comments before literals eats the rest
  of that file.
* `ProjectFortress/demos/GenomeUtil2a.fss:126`, a line comment containing an
  unmatched apostrophe and a semicolon, both of which must be inert.
* The acceptance program's exact token stream, gap flags included.

## Literal rules that change the token stream

* A numeral is `[0-9a-zA-Z]+` with `'`, U+202F and `.` as separators, maximal
  munch. So `2x` is ONE numeral token that then errors, never a tight
  juxtaposition. Implicit multiplication by a literal must be written `3 n` or
  `3(n)`.
* `1.x` and `12.52.23` are single erroneous numeral tokens. A numeral is never
  followed directly by a selection dot. A lexer that emits `Dot` unconditionally
  accepts programs the reference rejects.
* Digit separators are deleted before the value is computed, so `1'000'000` is
  `1000000`. Values are arbitrary precision at lex time, not `i64`.
* Floats are exact rationals at lex time, not IEEE doubles. There is no exponent
  syntax and no suffix; `1e10` is an error.
* String escapes are exactly `\b \t \n \f \r \" \\` plus the two curly quotes.
  No `\u`, no `\x`, no octal. A raw newline inside a string is an error, and `&`
  continuation does not apply inside one.

## Decisions needed before the lexer is written

Recommendations given; six of these are cheap and three change the shape of the
work.

1. **Operator words.** The reference steals all caps words like `MAX` from the
   identifier namespace by an open ended shape predicate, not a table. Recommend
   deferring: lex them as identifiers in M1. Adding the predicate later only takes
   names away from users. Note `ZZ`, `QQ` and `CT_` must stay identifiers.
2. **Non-ASCII policy.** Recommend `LexError` on any non-ASCII outside comments
   and strings, with the single exception of U+202F inside a numeral. This
   determines whether the corpus test returns `Ok` or `Err` on roughly 40% of
   library files. Either is a pass.
3. **Token stream shape.** This model emits `Newline` AND carries `gap_before`,
   which is mildly redundant since the token after a `Newline` always has
   `gap_before == Break`. Recommend keeping both: uniform skip in `w` contexts is
   what makes recursive descent readable. Confirm before the crate boundary sets.
4. **Radix numerals, character literals, curly quote strings.** Recommend
   `LexError` for all three with clear messages.
5. **Sub-token operator guards.** Recommend reproducing `!("**")` on `*` and the
   `!(Symbol)` guard on `+`. Two characters of lookahead, and it stops `a++b`
   being silently accepted.
6. **Second reserved tier.** `juxtaposition`, `in`, `per`, `square`, `cubic`,
   `inverse`, `squared`, `cubed` are barred from identifier use outside the 90
   word set. Recommend reserving them so the namespace stays closed for
   dimensions and units in v1.
7. **Unary minus.** `x-1`, `x - 1` and `x -1` are three different programs:
   tight infix, loose infix, and a loose juxtaposition with a tight prefix minus.
   The acceptance program's `f(x-1)` needs the tight infix reading. Lexer emits
   `Minus` with gap flags either way, but this must be answered before the
   negative tests are written.
8. **Fail fast confirmed.** The reference logs and continues on tabs, unbalanced
   comments and bad radices. This plan assumes `LexError` throughout per the
   design doc. Worth recording: reference diagnostics fire during speculative
   parsing and are not rolled back, so it emits phantom and duplicate errors. Not
   reproducing that is an improvement, not a divergence.
9. **Word pasting.** The spec mandates a pre-lexing pass splicing `word&` at end
   of line with `&word` on the next into one token. The reference implements none
   of it and the spec admits so at `lexical-structure.tex:15-18`. Recommend
   skipping it, matching the reference. Nothing in the corpus exercises it.

## Known unresolved, deferred past M1

Is `(*` after a primary a comment or `(` followed by `*`? The reference is
scannerless and tries the expression alternative first, so whether the text is a
comment depends on whether the comment body happens to parse as an expression.
This contradicts the specification's own claim that partitioning into input
elements is uniquely determined by the characters. Live at
`Library/Random.fss:55`. Nobody could settle it by execution because the
distribution jars in this tree are 37 byte stubs. M1 always treats it as a
comment opener, which is safe because the construction requires static argument
brackets that M1 excludes. This becomes real at M3.
