# Fortress M3h: getters, setters, `self` parameters and component-level values

Date: 2026-08-19
Status: **landed**, commit `b816dd610` on `m3-unified-sprint`.
Preceded by `2026-08-19-m3g-static-argument-inference-design.md`, which is why
this milestone exists in the shape it does.

Parse **476 → 614** of the 1780 files that lex (26.7% → 34.5%).
Compile **187 → 205** of 1956. Zero regressions, zero non-clean exits.

## The measurement picked it, and the histogram was wrong a fifth time

Every candidate was spiked behind its own branch and measured with the real
corpus test — the method M3d through M3f established. The prior table in
`04-state.md` was measured against M3e's 428 baseline and is now stale in both
directions:

| construct | first-blockers | M3f-era estimate | **measured now** |
|---|---|---|---|
| component-level value declarations | 120 | +31 | **+53** |
| dotted / braced / foreign imports | 34 | +25 | **+46** |
| `self` parameters | 46 | +25 | **+36** |
| `getter`/`setter` | 131 | +31 | **+35** |
| `var` bindings | 92 | +6 | **+29** |
| `opr` declarations | 80 | +5 | **+23** |
| object expressions | 19 | +13 | **+14** |
| dotted method declarations | 50 | — | **+10** |

Two things worth keeping. First, **every construct measured higher than its
estimate**, because M3f unlocked files that then became newly blocked on these —
a delta measured against a stale baseline understates. Second, `var` and `opr`
were the two the previous milestone singled out as traps worth 11 files between
them; re-measured they are worth 52. The trap was real at the time and is not
now. **Re-spike before every milestone. Do not carry a number forward.**

Combinations remain superadditive:

| bundle | sum of parts | measured | 
|---|---|---|
| getter/setter + `self` + component values | +124 | **+138** |
| component values + imports + `self` | +135 | +137 |

The directed bundle wins on both the number and the effort — 85 lines against
118 — so it was taken unchanged.

## What each one is

**`getter` / `setter`** stop being reserved words and become member modifiers.
Both are declared like a method; only the invocation syntax differs, so the
parser records a marker on one `MethodDecl` rather than growing a node.

**A `self` parameter** is the receiver of a functional method: `area(self, k)`.
It parses as a parameter whose type is the enclosing trait or object applied to
its own static parameters.

Neither is **checked**. Both are read through dotted method dispatch, which is
not implemented, so they are a parse fact and nothing more. That is the point:
they are what 138 files were stuck behind, not what those files need to run.

**A component-level value declaration** — `pi: RR64 = 3.14`, `v = 1`, `x := 0`,
and the initializer-less `stdIn: Reader` an api declares — parses into a nullary
`FnDecl` carrying a new `value_binding` marker, because there is no value
declaration node yet.

The recognising branch cannot steal a function: a function declaration is always
an identifier followed by `[\` or `(`, and none of `Colon`, `Eq`, `ColonEq` can
begin one. Pinned by a test. M3f's `=`-as-equality does not fight it either —
the decision is made on tokens before any expression is parsed, the same shape
as the existing local-function guard, so `v = 1` can never be read as a
discarded comparison at component level.

## The marker is the milestone's real content

The spike carried a value binding as a plain nullary function and gained +26 on
the compile metric rather than +18. That is eight files' worth of temptation and
it is wrong in the worst available way — **it compiles**.

A value's initializer runs at component initialization. A nullary function's
body runs when it is called. Nothing can reference a component-level value in
this compiler today, so it is never called. Measured, not reasoned:

```
noisy: ZZ32 = do println("INITIALIZER RAN"); 7 end
run(): () = do println("run") end
```

built cleanly and printed `run`. The initializer never ran, and no diagnostic
said so. That is a program that compiles and does the wrong thing, which is the
one outcome this tree does not ship.

So the checker refuses it, by the marker, with `ValueBindingUnsupported`. The
files stay parsed and counted; eight of them stop compiling and that is correct.
`fortressc/tests/badvaluebinding.fss` is that exact program.

This is the standing pattern — tuple types, arrow types and dotted methods all
parse and are refused with a named diagnostic. Parsing a construct is not
claiming to implement it.

## Ratchets

Both floors moved with the measurement, not ahead of it:

* `crates/parser/tests/corpus.rs`: `parsed >= 476` → `>= 614`
* `tools/apply-gate.sh`: `COMPILE_FLOOR=187` → `205`

## Gate

`tools/apply-gate.sh` 20/0 → **21/0**, with `badvaluebinding` in the refusal set.

One new mutation, **shown to refuse** before the green result was reported:

| mutation | result |
|---|---|
| carry a component-level value binding as a nullary function | REFUSED |

Full run: 5 mutations, **0 survived, 0 could not be applied.** Three new parser
tests; the suite is 230 → 233.

All seven gates green with `--selftest`: generics 23/0, dispatch 19/0, array
16/0, memory 17/0, MPI 17/0, unit 15/0, apply 21/0.

## Scope limits, stated rather than hidden

* A getter is not readable and a setter is not writable. Both need dotted method
  dispatch.
* A `self` parameter does not participate in dispatch. Fortress lifts a
  functional method into the top-level overload set of its name, with `self` in
  M3c's symmetric matrix. That lifting is not done.
* `self` is recognised by the parameter's name being `self`. Sound today because
  `self` lexes as `KwSelf` and can never arrive as an identifier, but a real
  implementation wants an explicit flag.
* Multiple `self` parameters in one list parse. 1.0 allows exactly one.
* A component-level value is refused, so mutability, initialization order and
  the LLVM global it would need are all untouched.
* The `...` varargs skip eats any run of `Dot` tokens without counting them.

## What is next, and it is not close

**Dotted methods.** 44 checker-stage first-blockers, and now the thing standing
between every construct this milestone added and any of it actually running. It
is three pieces — the checker refuses `Expr::Field` callees outright, `mono`
must route `Instantiate`-over-`Field` to a method, and `mono` does not walk
method bodies at all — plus entry into M3c's whole-program dispatch matrix.
