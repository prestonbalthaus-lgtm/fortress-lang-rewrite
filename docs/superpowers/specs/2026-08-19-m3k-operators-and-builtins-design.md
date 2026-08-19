# Fortress M3k: primitive operators and builtins

Date: 2026-08-19
Status: **landed** on `m3k/operators-and-builtins`, seven commits, not pushed.
Named as next by the M3j note, where `compiler_tests/Compiled17.fss` stopped at
`unknown name AND`.

Compile **242 -> 262** of 1956. Parse **614 -> 625**. **Zero regressions at
every step** -- no file that compiled before this milestone fails after it,
which is the first milestone since M3f that can say so.

Baseline measured on the M3j merge, not taken from a document: 1956 files,
242 exit 0, 1714 exit 1, 0 anything else.

## What landed, measured step by step

| step | compile | parse | what |
|---|---|---|---|
| baseline | 242 | 614 | the M3j merge |
| A | **251** | 614 | `AND`, `OR`, `NOT`, Boolean `=` and `=/=` |
| B | **256** | 614 | `print`, `ignore`, `assert` |
| C | **261** | **625** | `^` |
| C+ | **262** | 625 | `^` on mismatched operand types |

`compiler_tests/Compiled17.fss` -- the file that named this milestone --
compiles and prints `pass`.

## Short circuit, and where the branch comes from

`AND` and `OR` are checked as Boolean operands **with a diagnostic that names
the operator**, and then desugared to

```
a AND b   ->  if a then b     else false end
a OR  b   ->  if a then true  else b     end
```

Codegen's `If` already emits a conditional branch, two blocks and a phi, so
**codegen changed by zero lines for `AND` and `OR`** and the short circuit is
real rather than asserted.

Desugaring in the *parser* would have been cheaper still and was rejected: the
error for `n AND true` would then have talked about the condition of an `if`
the user never wrote. That is the wrong-mechanism-diagnostic class, and it is
now three milestones old.

Two witnesses, because one is not enough. `br i1` and `phi i1` in the emitted
module say the **shape** is right. They do not say the semantics are: a truth
table cannot tell a short-circuit `AND` from a strict one, since `true AND
false` is false either way. The semantic witness is a right operand that
**prints**, and its output being absent:

```
false AND loud()   ->  false          (and no RHS)
true  OR  loud()   ->  true           (and no RHS)
```

The gate also asserts no `select i1` is emitted, because a select computes both
sides.

`NOT` is **not** desugared. It is a prefix operator -- 1.0 puts prefix
operators above every infix operator, so `NOT x < y` is `(NOT x) < y` -- and it
gets one `xor`, because three basic blocks for one instruction is worse code at
`-O0` and `-O0` is where this project checks its claims. The precedence is
pinned by `tests/badnot.fss`, whose refusal names `NOT` and its ZZ32 operand
rather than blaming the comparison.

## A diagnostic that reported the wrong type, caught by its own fixture

The first version rewrote **both** `Mismatch` and `LiteralNotApplicable` into
"`AND` takes Boolean operands; this one is ...". `LiteralNotApplicable` carries
the type the slot **required**, not the type the operand **had**, so `1 AND
true` reported *"this one is Boolean"* about the integer `1`.

Only `Mismatch` is rewritten now; the literal rule's own message already names
the literal correctly. `tests/badlogical.fss` uses a ZZ32 *variable* on
purpose, and its comment records why.

## `^`

It sits in the postfix loop, which is where 1.0 puts superscripting: above
tight juxtaposition and above everything else. Its exponent is parsed as a
`primary` and not as a `postfix`, and that one choice is what keeps the group
**left associative** -- `2^3^2` is **64**, not 512, and the gate asserts the
64 because the number is the only thing that tells the two apart.

It is the one arithmetic operator with no instruction behind it, so it is a C
shim -- which is what keeps the negative-exponent rule in exactly one place. A
negative exponent on an integer has no integer answer and halts with a
diagnostic and exit 1, the stance `fortress_array_slot` already takes.

**Its operands may disagree, and that was a correction made mid-milestone.**
The first version required them to agree, consistent with `+`. Then
`ProjectFortress/tests/expTest.fss` turned up: it asserts all four of
`RR64^ZZ64`, `ZZ64^ZZ64`, `RR64^RR64`, `ZZ64^RR64`, and 1.0 declares `^` on
every base-exponent pair. Consistency with `+` would have been wrong about the
operator this milestone exists to add, so `^` became a target of its own
carrying both types, with one shim per pair. A real anywhere makes the result
real; two integers keep the base's width, because this language has no implicit
widening. That correction is the whole of the +1 from 261 to 262.

## The builtins

`print` is `println` without the newline and shares its path. `ignore(e)` is a
block whose only item is the expression and which has no tail -- the discard is
what a block statement already does, so it needs no target and no shim.

`assert` lands in the four shapes the corpus writes: a flag, a flag with a
message, two values, two values with a message. The two-argument forms are told
apart by the second argument's type. It becomes an `if`, a call to a halt shim,
and nothing else -- so **an assert is exactly as strong as `=` is and no
stronger**, and `assert("a", "b")` is refused by name rather than quietly
accepted. A failed assert halts with a diagnostic and exit 1.

## Declined on evidence: the word arithmetic operators

`MOD`, `DIV`, `REM`, `MAX`, `MIN`, `GCD`, `LCM`, `CHOOSE`, `DIVIDES`,
`BITAND`, `BITOR`, `BITXOR` are **not** built, and the reason is not effort.

They are not compiler primitives. `CompilerLibrary/FortressLibrary.fsi:430-437`
declares them as `opr DIV(self, b:I): I`, `opr MOD(self, b:I): I`,
`opr DIVIDES(self, b:I): Boolean` and so on -- **functional-method operator
declarations in the library**. `BITAND`, `BITOR` and `BITXOR` do not appear in
the specification's operator appendix at all. Building them as builtins would
put the library inside the compiler and would then have to be *unbuilt* the
moment `opr` declarations land, because the library's own declarations would
collide with them.

The measured lever agrees: `DIV` is the first blocker for exactly two files,
and `tests/intPrim.fss` -- the file that wants all twelve -- also reads
`a.minimum` and `a.maximum`, which are accessors and still refused. It would
not compile with all twelve built.

**What the milestone owes the next one is the refusal machinery, not the
operators.** 1.0's precedence is a *partial* order: `MAX MIN REM MOD GCD LCM
CHOOSE per` are arithmetic operators with **no stated relationship** to `+` or
`*`, so `a + b MOD c` is a *static error*, and `REM MOD CHOOSE per` are
**nonassociative** while `MAX MIN GCD LCM` are left associative. A guessed
precedence for `a + b MOD c` type-checks under either reading and silently
computes a different number, which is the class this compiler exists to refuse.
So when `opr` lands, the precedence run-collector and its two refusals land
with it -- and not one operator before.

This decision is the same shape as M3g's: declined on measurement, with the
design that would be correct recorded rather than half-built.

## What the layered grammar does and does not enforce

```
expr        -> disjunction
disjunction -> conjunction (OR conjunction)*      left associative
conjunction -> comparison  (AND comparison)*      left associative
```

1.0 forbids mixing a boolean operator with an arithmetic one without
parentheses; a layered grammar accepts `a OR b + c` as `a OR (b + c)`. It is
not silently wrong here, because `AND`/`OR` take only Boolean and the
arithmetic operators take only numeric -- **the type rules enforce the
non-mixing that the precedence relation would have**. That argument holds for
these two groups and for no others, which is exactly why the word arithmetic
operators above cannot ride on it.

The lexer did not change. `AND`, `OR` and `NOT` already lex as identifiers, and
the parser reads them as operators by name. Two things that had to be right:
`starts_juxt_operand` must refuse `AND` and `OR`, or the juxtaposition run
swallows them and the new layer never sees one; and a word operator is never
gated on `fixity_at`, because it cannot be glued on its left -- the lexer would
have merged the letters -- so `a AND (b)` would read as `Prefix` and leave the
operator unconsumed.

## First-blocker counting was biased HIGH, which is a new direction

| step | first blockers said | measured |
|---|---|---|
| A: `AND` + `NOT` | 9 | **9** |
| B: `print` + `assert` + `ignore` | 27 | **5** |
| C: `^` | 17 | **5** compile (11 parse) |

The rule that fits all of it, and it is worth carrying: **first-blocker
counting overestimates a lever that sits LATE in the pipeline and
underestimates one that sits EARLY.** A file that gets far enough to call
`print` has already passed the lexer, the parser and most of the checker, so
its *remaining* blockers are downstream and numerous. A file that dies in the
parser has everything still ahead of it, so unblocking it usually unblocks
several constructs at once -- which is the superadditivity M3h and M3j
measured. The existing note that the bias is "LOW, not merely unreliable" was
written from parser milestones only.

## Gates

New: `tools/operator-gate.sh`, **25/0**, with `--selftest` and six mutations.
It computes `AND`'s truth table itself rather than reading it out of the
program under test, and its central assertion is a **count of zero**.

Eight gates green: generics 24/0, dispatch 35/0, array 16/0, memory 17/0, MPI
17/0, unit 15/0, apply 21/0, operator 25/0. 253 cargo tests, clippy 0, fmt
clean.

**The array and memory gates caught a real cross-cutting regression**: they
compile `runtime/shims.c` with their own `cc` lines, and `^`'s float shim calls
`pow()`. The driver's own link had been given `-lm` and had therefore hidden
the dependency. That is the whole reason a gate that builds the runtime itself
earns its keep. The cluster image now asserts `math.h` the way it already
asserts `gc.h`, and `02-stack.md` records that every link takes `-lgc` and
`-lm`.

Parse floor **614 -> 625**, `COMPILE_FLOOR` **242 -> 262**.

## Next

The blocker board after M3k, and the two largest are unmoved by anything in
this milestone: `reserved word` 348, `expected an expression, found KwVar` 95.
`opr` declarations are the lever that unlocks the word arithmetic operators
above, and they arrive with the precedence refusals attached.
