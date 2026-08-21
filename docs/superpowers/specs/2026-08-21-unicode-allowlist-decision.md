# The Unicode allowlist, and the two cases decision 3 does not reach

Written while implementing the allowlist (`SPIKE-UNICODE`), because two of the
eighteen codepoints the corpus actually needs fall outside what decision 3 says.

## What decision 3 already settled

`02-stack.md`: the core grammar is strictly ASCII; mathematical symbols are NOT
reserved lexer tokens and are "handled via standard library symbol aliasing".
`2026-08-21-v1-gap-analysis.md` section 2.4 adds the shape: a CURATED allowlist,
explicitly not Sun's `ID_Start`/`ID_Continue` dump.

## The measurement the decision is made against

Over all 136 `Library/` and `CompilerLibrary/` files, comments and string
literals stripped: **18 distinct non-ASCII codepoints, and zero of them are
letters.** Over the whole 1956-file corpus there are 64, and the extra 46 are
Hangul, daggers, primes, connecting punctuation and one triple-vertical-bar —
every one of them inside a test whose subject IS the codepoint, and none inside
a file the bootstrap needs. The allowlist is the eighteen.

Reproduce with `.spike/nonascii.py`, which strips comments and strings the way
`lexer/src/raw.rs:51-92` does — including that `(*)` is INERT inside a block
comment.

## The rule that came out of it

**A codepoint the reference grammar lists as an alternative SPELLING of a token
lexes as that token. Every other allowlisted codepoint is an ordinary operator
character carrying its own text.**

That is the whole allowlist, and it needs no table of meanings:

| spelling | token | grammar |
|---|---|---|
| U+27E6 `⟦` U+27E7 `⟧` | `[\` `\]` | brackets, see below |
| U+21D2 `⇒` | `=>` | `Literal.rats:397` |
| U+2264 `≤` U+2265 `≥` | `<=` `>=` | `Symbol.rats:214,216` |
| U+2260 `≠` | `=/=` | `Literal.rats:308` |
| U+2254 `≔` | `:=` | `Symbol.rats:200` |
| U+2190 `←` U+2192 `→` | `<-` `->` | `Symbol.rats:197`, `Literal.rats:365` |

The other ten — `¬ ∈ ∨ ∩ ≪ ≫ ⊆ ⊇ ⟨ ⟩` — get ONE token between them, carrying
the source slice, and take their meaning from an `opr` declaration exactly as
`!`, `@` and `SUBSET` do. That is decision 3 honoured precisely: no lexer token
per symbol, and the library is what gives one meaning.

`←` and `→` need a token each in spite of the rule, because their ASCII
spellings are two tokens joined by span adjacency (`Lt` glued to `Minus`,
`Minus` glued to `Gt`) and a single codepoint cannot be two tokens. The two
sites that read them — the generator arrow and the arrow type — now answer how
WIDE the arrow is rather than merely whether one is there.

## Case 1: the brackets, U+27E6 and U+27E7

Decision 3 scopes the allowlist to "codepoints legal in identifiers and
operators". `⟦` and `⟧` are **brackets** — the Unicode spelling of `[\` and
`\]` — and a bracket is neither.

**Decided: they lex directly as `LGeneric` and `RGeneric`.**

The reason is not convenience. A library alias is a DECLARATION, and there is
nothing to declare: `[\` does not name a function, it opens a static-argument
list, and no `opr` declaration can introduce a bracket the parser must already
know about to read the declaration itself. So the aliasing mechanism decision 3
names cannot carry these two, and the only remaining place for them is the
lexer. They are the same token, spelled differently, which is exactly what
`Symbol.rats` says of every other pair in the table above.

Scope: 79 uses over 7 corpus files, 64 uses over 3 library files. It is the
single most common non-ASCII codepoint in the tree.

## Case 2: the curly quotes, U+201C and U+201D

`“` and `”` are **string delimiters**. Same argument, more sharply: a string
literal has no name at all, so there is nothing an alias could attach to.

**Decided: curly-quoted string literals are a real string form, and the marks
must match.**

`Literal.rats:151-155` gives `StringLiteralExpr` two delimiter pairs with
identical content, and :158-167 gives each MIXED pair its own error production
whose message is "The opening and closing marks of a string literal must
match." The corpus tests both halves and has since before this rewrite existed:

* `ProjectFortress/tests/matchingStringMarks.fss` prints "Hello, World!" twice,
  once through each pair. It compiles and runs as of this change.
* `ProjectFortress/parser_tests/XXXNotMatchingStringMarks.fss` writes both
  mixed pairs and is a must-FAIL test. It is refused, naming the rule.

`LexErrorKind::CurlyQuoteStringUnsupported` is deleted rather than kept: nothing
reaches it, and a diagnostic no input can produce is how a dead rule outlives
the reason for it. `MismatchedStringMarks` replaces it and is reachable from
three inputs — `"a”`, `“a"`, and a closing mark with no opener.

## What is deliberately still refused

Unicode IDENTIFIER characters. The only four corpus files needing letter
codepoints write Hangul and the full Hebrew alphabet, and they are exactly the
files decision 3 declines to serve. **No census file needs a single Unicode
identifier character**, so nothing on the critical path is waiting on it.

Also still refused, and worth naming because they are near neighbours: U+2018
and U+2019 remain character-literal errors, U+2AF4, U+21A6, U+2054, U+2034,
U+2020 and U+2021 remain `NonAsciiCharacter`. Each is used by exactly one test
whose subject is that codepoint.

## One inconsistency this creates, recorded rather than fixed

`Symbol.rats:214-222` gives several operators an ASCII WORD spelling as well:
`lessthanequal = "<=" / "LE" / "≤"`, `NOT = "NOT" / "¬"`,
`OR = "OR" / "∨"`. Under the operator-word lexical rule `LE` is an
`OpWord`, not `Le`, so `a LE b` resolves to a function named `LE` while `a <= b`
and `a ≤ b` are `BinOp::Le`. Likewise `∨` is an operator named `∨` and not
`OR`.

That is the honest state of one name per spelling not yet being enforced. It
belongs with the operator-property traits (`SPIKE-OPRSTATIC`), where the
canonical name of an operator is decided once for the declaration side and the
use side together — not here, where guessing at it would put the mapping in two
places.
