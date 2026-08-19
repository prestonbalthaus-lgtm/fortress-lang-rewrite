# Fortress M3e: the unit type `()`, with syntax for tuples and arrows

Date: 2026-08-19
Status: **landed** on `m3e/unit-tuple-arrow`. Implementation plan:
`../plans/2026-08-19-m3e-unit-tuple-arrow.md`.

Every corpus number below was predicted by the pre-implementation spike and then
hit exactly by the implementation: 298, 303, 314, 418, 428. The compile metric
in §9 was the one prediction that was wrong -- it said the number of files
compiling end to end would move by a small amount, and it went 52 -> 151.

Two defects the milestone's own corpus sweep found and fixed, neither of them
reachable before because the files carrying them died at the parser on `()`:
a void-valued binding was exit 70 rather than a diagnostic, and `run(args:String)`
built a module LLVM rejected.

`()` becomes a writable type and a writable value, resolving to the `Type::Void`
that already exists. Tuple types, tuple expressions and arrow types get a real
AST shape and a clean diagnostic. `Type` does not grow a variant and stays
`Copy`.

## The measurement first, because it renames the milestone

The roadmap calls the next item "tuple and arrow types (the top parser blocker)"
and `04-state.md` puts the number at 536. The number is right. The name is not.

Running the real driver over all 1956 corpus files, the 536 files whose first
failure is `expected a type name` break down as:

| count | what is actually there |
|---|---|
| 485 | `()` — the unit type, as in `run():() = ...` |
| 30 | `(A, B, ...)` — a real tuple type |
| 11 | `Foo.Bar` — a qualified name |
| 8 | `[\E,0\]` — a `nat` static argument |
| 2 | `(A)` — a parenthesised type |

**90% of the top parser blocker is the unit type.** Arrow types are not in that
histogram at all, because `->` lexes as `Minus` `Gt` and fails one level up: 30
of the 125 `expected )` failures are `found Minus`.

So the estimate is an experiment rather than a count, the same way M3d's was.
`type_ref` was spiked to accept `()`, `(A)`, `(A, B, …)` and `A -> B`, and
`primary` to accept `()` and `(a, b)`, each behind an environment switch so the
constructs could be measured one at a time. Synthetic names were used for tuples
and arrows and thrown away — this measures **parser reach only**, nothing
downstream. The spike was reverted and the baseline re-verified at 168.

| what was switched on | files that parse | delta on baseline |
|---|---|---|
| baseline | **168** | |
| `()` and `(A)` in type position | 298 | +130 |
| … + tuple types | 303 | +135 |
| … + arrow types | 314 | +146 |
| `()` as an expression, alone | 203 | +35 |
| unit package (`()` type, `(A)`, `()` expression) | **400** | **+232** |
| everything | **428** | **+260** |

Decomposed by construct rather than by the order they were switched on:

* **unit, type and expression position: +232**
* **tuples, type and expression position: +15**
* **arrows: +13**

The milestone as named is worth 28 files of 260. The piece nobody named is worth
232. 9.4% → 24.0% of the files that lex; the largest single parser move in the
project, ahead of the lexer pass's +70.

**This is the second time the blocker histogram has pointed at the wrong thing.**
M3c predicted 562 corpus files and delivered 32. M3d was sold on 737 bracket
files; the erasure experiment said ten before a line was written, and the
milestone delivered fourteen. The roadmap already records the lesson — "count what the
compiler actually does, not what the blocker histogram implies" — and it has now
paid for itself twice. It is cheap to re-run and it should be re-run before every
milestone that quotes a corpus number.

Named **M3e** for the unit type. Static-argument inference, previously pencilled
in as M3e, moves to M3f; nothing about this milestone touches it.

## What the specification says, and it agrees with the split

`()` is not a tuple. Specification 1.0, `basic/types-vals-vars.tex:470-472`:

> The type `()` is the type of the value `()`. Its only supertype (other than
> itself) is `Any`, and it excludes every other type.

and `:496` names it: `()` (pronounced "void"). A tuple, `:260-261`:

> A tuple type consists of a parenthesized, comma-separated list of two or more
> types.

Two or more. `(A)` is `A` in parentheses and nothing else.

The legacy grammar agrees, independently and in three separate productions —
`ProjectFortress/src/com/sun/fortress/parser/Type.rats:207` `VoidType ::= ( w )`,
`:181` `TupleType ::= ( w Type w , w TypeList w )`, and `:153`
`ParenthesizedType ::= ( w Type w )`. Getting a lone `(A)` wrong by folding it
into the tuple case would be a silent type error, not a parse error.

`TypeInfixOp` at `Type.rats:354` is `rightarrow` and nothing else, and
`Symbol.rats:226` defines it as `"->" / "→"`. The Unicode arrow is out of
scope: non-ASCII outside comments and strings is already refused by the lexer,
and the core grammar stays ASCII by standing rule.

The grammar also allows an effect clause, `A -> B throws {E}`. **Zero corpus
files use it** (measured, not assumed), so it is out.

## 1. What ships

| | |
|---|---|
| **Implemented, full stack** | `()` in type position; `()` in expression position; `(A)` as parenthesised `A` |
| **Parsed, refused by the checker** | tuple types `(A, B, …)`; tuple expressions `(a, b)`; arrow types `A -> B` |
| **Out** | function values and lambdas; the `throws` effect clause; the Unicode `→`; `Any` and `BottomType`; tuple subtyping, covariance and exclusion rules |

Arrow types are parse-only close to by necessity, not by preference. There are no
lambdas and no function-valued expressions in this subset, so an arrow type is
uninhabited: a corpus file that writes `g: ZZ32 -> ZZ32` also calls `g(1)`, and
resolving an overload set down to a single value is its own milestone. Parsing
the syntax and refusing the meaning is the honest version.

The repository already has this shape twice — enclosing operators are tokenised
but not parsed, imports are recorded and ignored. A third instance is consistent,
not a new kind of debt.

## 2. The AST: `TypeRef` becomes an enum

A tuple is not a name applied to arguments, and an arrow is not either. Encoding
them as `TypeRef { name: "(tuple)", args }` was fine for a throwaway measurement
and would be wrong here: the magic name reaches every diagnostic that prints a
type, and it collides with any user type spelled the same way.

```rust
pub enum TypeRef {
    Named { name: String, args: Vec<TypeRef>, span: Span },
    Unit  { span: Span },
    Tuple { elems: Vec<TypeRef>, span: Span },   // two or more, by construction
    Arrow { from: Box<TypeRef>, to: Box<TypeRef>, span: Span },
}
```

with a `span()` accessor. `Arrow`'s children are boxed because it is the only
form whose recursion is not already behind a `Vec`.

M3d's rule applies unchanged: a shape mistake here is cheap now and expensive
later. The invariant "two or more" belongs to the constructor — a one-element
parenthesised list is unwrapped in the parser and can never reach `Tuple`.

Ripple, all of it verified in the tree rather than guessed:

* `crates/types/src/mono.rs:221` `fn ty` becomes a match. Substitution stays
  structural, so `T` inside `(T, T)` still substitutes and a static parameter
  inside an arrow still substitutes. None of the three new forms is ever an
  instantiation request, so `request` is untouched.
* `crates/types/src/registry.rs:122` gains three arms.
* Five construction sites in `crates/parser/src/lib.rs`.
* `TraitDecl::extends` / `comprises` / `excludes`, `StaticParam::bounds` and
  `BoundObligation` all hold `TypeRef`. The specification says tuple types cannot
  be extended by trait types; a non-`Named` in any of those positions is the same
  diagnostic as everywhere else, not a special case.

## 3. Parsing

`type_ref` splits in two:

```
type_ref   ::= type_atom ( '->' type_ref )?          right associative
type_atom  ::= '(' ')'                               Unit
             | '(' type_ref ')'                      the inner type, span widened
             | '(' type_ref (',' type_ref)+ ')'      Tuple
             | ident ( '[\' type_args '\]' )?        Named
```

`A -> B -> C` is `A -> (B -> C)`, which falls out of the right recursion.

**`->` is recognised without a lexer change.** It stays `Minus` `Gt`, and the
parser accepts it only when the two are glued and only in type position. This is
the parser's existing idiom, stated in its own header at
`crates/parser/src/lib.rs:9-11`: fixity comes from byte-span adjacency rather
than from the token, because `x-1`, `x - 1` and `x -1` lex identically. Adding an
`Arrow` token would change how every `->` in the corpus lexes, and the lexer
ratchet at 1780 is a separate ratchet that this milestone has no reason to
disturb.

Expression position, in `primary`'s `LParen` arm:

```
'(' ')'                      -> Expr::Unit
'(' expr (',' expr)+ ')'     -> Expr::Tuple
'(' expr ')'                 -> unchanged, the parenthesised expression
```

The glued-`(` application rule at `lib.rs:775-778` is untouched: `f()` is still a
call with zero arguments and reaches `args()`, never `primary`.

## 4. The types crate

`Type` gains nothing and stays `Copy`. That is the entire reason tuples and
arrows are parse-only — both are structurally recursive, and `Type` being `Copy`
and comparable by value is load bearing across dispatch, the registry and the
monomorphizer.

`registry.rs::resolve`:

* `TypeRef::Unit` → `Ok(Type::Void)`. One arm. `Type::Void` already exists at
  `crates/types/src/types.rs:93` and already prints as `"()"` at `:113`.
* `TypeRef::Tuple` and `TypeRef::Arrow` → `TypeError::TypeNotImplemented { span, form }`.

Checker:

* `Expr::Unit` → `TypedExpr { kind: TypedExprKind::Unit, ty: Type::Void }`.
* `Expr::Tuple` → the same `TypeNotImplemented`.

## 5. Void where it cannot be stored, which is the part to review hardest

Void is currently **unnameable**, so no signature can ask for it. Making `()`
writable opens four positions where it can now be asked for — and one of them is
already broken without this milestone.

**The binding case is a live defect on master, measured not inferred.** `Type`
`Void` is reachable today as the type of an *expression*, and the checker accepts
it as the right-hand side of a binding:

```
$ fortressc v1.fss                # run(): ZZ32 = do x = println("hi") ; 0 end
fortressc: lexed 27 tokens, parsed and typechecked `v1` with 1 function(s)
fortressc: internal error: a void expression used as a value
exit=70
```

`x = while ... end` does the same. Exit 70 is `EXIT_INTERNAL_ERROR`, which this
driver reserves for compiler bugs, and 03-guidelines says in as many words that
malformed input is a diagnostic and not a crash. Ordinary user source currently
reports a compiler bug. `VoidNotStorable` on bindings **fixes that**, and it is
worth landing on its own terms rather than as a side effect of `()` becoming
writable.

`crates/codegen/src/lib.rs:167` maps `Type::Void` to `None` — it has no LLVM
representation by design. Four positions would therefore build a broken
signature or a broken layout rather than a diagnostic:

* a parameter, `f(x: ())`
* a field, `object O(x: ()) end`
* an array element, `Array[\()\]`
* a binding, `x: () = ()`

All four are refused with a new `TypeError::VoidNotStorable { span, position }`.
Missing one does not produce a bad error message; it produces malformed IR, which
is an internal error under this compiler's own exit-code contract. The array case
is already half-covered — `Elem::of(Type::Void)` returns `None` — but it returns
`UnsupportedElementType`, which names the wrong cause.

Void stays legal in exactly two places: a declared return type, and the type of
an expression.

## 6. Codegen

One arm. `TypedExprKind::Unit` produces no value, which is a case codegen
already has: a call returning `Type::Void` yields `None` at `lib.rs:1019`, and a
void return is handled at `:605`. There is no new runtime shim, no new
allocation, and nothing new in `runtime/`.

## 7. Gates and tests

`tools/unit-gate.sh`, `--selftest` and `--mutate` like the other five, self-testing
its own assertions before it runs anything.

What it has to prove:

* `run():() = ()` compiles, links and runs, exit 0. This is the single most
  common function shape in the corpus and it has never compiled.
* `f():() = ()` called from `run` — a void call in statement position.
* `(A)` resolves to `A`, checked by giving the parenthesised form and the bare
  form to the same overload and getting one symbol.
* `f(x: ())` is refused with `VoidNotStorable`, and so are the field, element and
  binding forms. Four refusals, not one.
* a tuple type, a tuple expression and an arrow type each produce a diagnostic
  and exit 1, not a panic and not exit 70.

Mutations, and a gate is not trusted until it has refused:

1. Drop the `VoidNotStorable` guard on parameters. Expect a void parameter to
   reach codegen and the gate to catch the malformed signature — the numbers to
   state are which check failed and what the driver's exit code was.
2. Fold the one-element parenthesised case into `Tuple`. Expect `(A)` to stop
   resolving to `A`.
3. Turn the tuple-type refusal into a silent accept. Expect a program that should
   be a diagnostic to compile with status 0.

Cargo tests: the parser gets the grammar cases above, the types crate gets the
four refusals and the Void return, and `end_to_end` gets `run():() = ()`.

## 8. Ratchets

* Parser floor 168 → the measured number, expected **428**.
* Lexer floor **1780, unchanged**. Nothing in this milestone touches the lexer,
  which is the point of not adding an `Arrow` token.

Both are already assertions — `crates/parser/tests/corpus.rs:112` and
`crates/lexer/tests/corpus.rs:131` — so a regression fails the build rather than
being noticed a milestone later.

## 9. What this does not do, stated so it is not discovered later

**It moves the parse metric, not the compile metric.** M3d reported both and this
will too. Most of the 260 newly-parsing files fail in the checker for reasons
this milestone does not address — `getter`/`setter` at 126 files, `opr` at 79,
and the new top blocker after the change is `expected a newline or ;` at 204.
The number of files that compile end to end will move by a small amount, driven
by the `run():() = ()` shape alone.

**It does not make tuples real.** No tuple subtyping, no covariance, no
arity-based exclusion, none of it in the subtype relation and none of it in
M3c's dispatch matrix. When that is wanted, the way to keep `Type` `Copy` is the
`intern()` pattern that `types.rs:14` already uses for type names — a leaked
`&'static [Type]` — and not a `Box` or an arena. That is a milestone of its own
and it touches the dispatch code that M3d deliberately never had to change.

**It does not add function values.** Arrow types will parse and be refused, and
that refusal is the honest signal that the feature is absent.
