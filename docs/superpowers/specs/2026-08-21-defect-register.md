# Tracked defects and named deviations

One place for defects that have an owner and a group, and for deviations from
1.0 that were decided rather than discovered. Opened 2026-08-21 out of the Group 0
instruments and the Group 1 decisions, because before this the two kinds were
mixed together in `04-state.md`'s `known_bugs` and nothing distinguished
*"we are wrong"* from *"we chose differently and wrote it down"*.

**Rules.** A defect leaves this file only when a gate refuses its reproducer. A
deviation never leaves it. Every entry names the group that owns it, per the gap
analysis §9 ordering.

---

## Defects

### DEF-1 — RESOLVED THE OTHER WAY. String juxtaposition is plain concatenation; the expectation is the quirk

| | |
|---|---|
| **Status** | **CLOSED as a defect. Reopened as DEV-11, a signed-off divergence** |
| **Adjudicated by** | the semantics lane, 2026-08-21, overturning this entry's original recommendation |

**Kept in full, because how it was decided is worth more than the verdict.**

The oracle gate found GenMet0-3 printing `bcat` where the `.test` records
`b cat` — silent wrong output at exit 0, four cases, and seven `.test` cases pin
the space (GenMet5/6/8 expect `b cat\na 3\n`, so it appears for `String ZZ32`
too). Traced to primary source:

```
ProjectFortress/LibraryBuiltin/CompilerBuiltin.fss:384
    opr juxtaposition(self, b:Object): String = self ||| b
:410  opr |||(self, b:Object) = jSmartConcatenate(...)
nativeHelpers/simpleConcatenate.java:20
    if s1 empty -> s2;  elif s2 empty -> s1;  else s1 + " " + s2
```

**This entry originally concluded: our shim is wrong, make it insert a space.
That was wrong, and the reason it was wrong is a lesson about evidence class.**

The two 1.0 libraries contradict each other — `CompilerBuiltin.fss:384` inserts
a space, `Library/FortressLibrary.fss:4049` is plain `||` — and
`juxtameaning.tex:103-110` does not decide between them: it only says a
juxtaposition containing a `String` becomes an application of the
`juxtaposition` operator, so the meaning is whatever the library declares.

**What decides it is how the corpus is WRITTEN, not what eight expectations
record.** Counted independently here rather than taken on trust: same-line
`println("LIT" ident)` sites, keywords excluded — **237 write a literal that
ENDS WITH A SPACE against 93 that do not**, including tab-terminated ones like
`println("Tolerance\t" errorTolerance)` and
`println("Expect\t\t" expect)`. Those 237 are authored for **plain**
concatenation; under the space-inserting rule every separator in them doubles.
`println("FAIL: " d ": unexpected value " n " at " i)` reads correctly only
under plain concatenation. The semantics lane counted 325 against 87 with a
broader regex — different numbers, same ratio, same answer.

**237 sites of authored intent beat 8 recorded expectations from one test
family.** The original recommendation weighed the oracle over the corpus because
the oracle is what the gate reads, which is exactly the bias a gate author is
prone to.

**Consequence, and it is now enforced rather than noted.** The gate grew a third
list, `tools/oracle-known-divergences.txt`, alongside the accepted-must-fail and
known-signal lists. GenMet0-3 are `divergence`, not `fail`; `fail` is now exactly
the 47 wrong acceptances; a line only goes in the list with a reason written
here; and a listed case that stops disagreeing is reported so the line gets
deleted. Numbers at `f81f41ace`: 285 pass, 47 fail, 4 divergence, 267 blocked,
6 unmodelled.

**If it is ever revisited** the switch is one function — `fn concatenation` in
`crates/types/src/lib.rs` — and it needs both empty-string carve-outs, which are
load bearing: without them `"" x` gains a leading space.

**And there is a real defect underneath it**, found by the semantics lane in the
same file and not to be confused with this one: a **written-but-unsatisfiable
bound on a method silently changes which implementation runs**. Method stamps
record obligations with `speculative: Some(...)` (`mono.rs:263-268`) and a failed
bound is answered by `prune_stamp`, never by an error — so in `GenMet0.fss`,
pruning trait `b`'s override lets trait `a`'s win and the program exits 0 having
called the wrong one. That one is open and it is theirs.

### DEF-2 — Three corpus binaries die on SIGSEGV

| | |
|---|---|
| **Owner** | Group 3 |
| **Class** | fault with no diagnostic |
| **Status** | open, **baselined by name** in `tools/oracle-known-signals.txt` — a fourth is red |

`ProjectFortress/long_term_not_working/overriding/{Diamond,Graph,RedundantGraph}OverridingParams.fss`
compile at exit 0 and their binaries die on signal 11. Cause is unbounded mutual
recursion through overridden methods (`m(z:String,t:String) = m()` alongside
`m() = self.m("a","b")`), so it is a stack overflow with no stack guard — in a
project whose stated rule is that a subscript "halts with a diagnostic and exit 1
rather than faulting".

**The prior question is not the stack guard.** Whether 1.0's overriding rules
made that recursion finite is what an oracle would settle, and **the `.test` set
has no case for these three files.** So this is a defect only if the recursion is
genuinely unbounded under 1.0's rules. Answer that first; it may turn out to be
an overriding-resolution defect wearing a stack overflow as a symptom.

---

### DEF-3 — Integer division by zero faults

| | |
|---|---|
| **Owner** | Group 3, item 11 |
| **Class** | fault with no diagnostic |
| **Status** | open, **ungated — there is no arithmetic gate at all** |

`d(a,b) = a/b` then `d(7,0)`: compiles at exit 0, binary dies with
`Floating point exception (core dumped)`, rc 136, on a bare `sdiv` with no guard.
`DivideByZero` is named across four spec files. RR64 division is correct
(`1.0/0.0` → `inf`).

By this project's own standard — *"a gate is not trusted until it has refused"* —
the arithmetic lowering has never been asked to refuse anything. The fix and its
gate are one item, not two.

---

### DEF-4 — `where { ... }` is a token skip

| | |
|---|---|
| **Owner** | Group 3, item 14 |
| **Class** | silent wrong acceptance |
| **Status** | **UNBLOCKED.** D6 §1 is FINALIZED and operative -- implement against it, it needs no further sign-off |

`skip_where` brace-matches tokens and returns `Ok(())` at five call sites, so
`f(x: ZZ32): ZZ32 where { this is total garbage } = x` compiles, links and runs.

**D6 §1 is the decision and it makes the fix bounded**: parse the
trait-constraint form, check its subject is a **declared** static parameter,
route it to the existing `BoundObligation` / `discharge_bounds` path, and refuse
every other where-clause form — including the binder form `where [\ ... \]` — by
name. Measured cost of the restriction: **zero files**. Expect losses in both
directions and say so; text that compiles today because the clause was discarded
will start refusing, and that is the point.

---

### DEF-5 — Silent wrong acceptance: `excludes`, `comprises`, trailing content

| | |
|---|---|
| **Owner** | Group 3 |
| **Class** | silent wrong acceptance; **all inside the 291 and all exit 0** |
| **Status** | open, **partially baselined** — 47 of these are named in `tools/oracle-accepted-must-fail.txt` |

- `trait A excludes {B}` + `object C extends {A, B}` → exit 0. M3c derives
  most-specific winners from a lattice the program was allowed to contradict.
- `trait T comprises { NoSuchTrait }` → exit 0; the identical name in `extends`
  is refused.
- Everything after the component's closing `end` is silently discarded,
  **including a whole second component**. Only *unlexable* trailing garbage is
  caught, and the lexer catches it, not the parser.

Read each of the 47 against decisions 1 and 4 **before** treating it as a bug —
a legacy static error for a feature v1 scopes differently is a **deviation**, and
its line stays in the file with a comment rather than being deleted.

---

### DEF-6 — 84 diagnostics carry both a `line:col` prefix and a leftover byte span

| | |
|---|---|
| **Owner** | the semantics lane (diagnostics) |
| **Class** | presentation; a reader is shown a byte offset they cannot use |
| **Status** | open, found by the api-conformance scaffold at the phase-1 merge |

The `line:col` conversion did not reach every diagnostic construction site, so
84 corpus files report the doubled form:

```
ProjectFortress/LibraryBuiltin/System.fss:13:8: 375..379: a foreign import
reaches a JVM implementation and this compiler emits native code
```

By message: the foreign-import one 39, two `fn` parameter forms 22, lopsided
infix 3, a `case` arm separator 2, object varargs 2, and a tail. Cheap to fix
and worth it — every instrument that strips a span now has to strip two shapes,
and `tools/{triage,api-census,api-conformance}.sh` each carry the same widened
regex because of it.

### DEF-7 — seven must-fail programs became reachable at the phase-1 merge

| | |
|---|---|
| **Owner** | Group 2 for five of them, the frontend lane for one, undiagnosed for one |
| **Class** | silent wrong acceptance, and **not a new break** |
| **Status** | baselined with reasons in `tools/oracle-accepted-must-fail.txt` |

The oracle gate went red with 7 new acceptances immediately after the merge.
Diagnosed rather than re-baselined: none of them is newly broken, they are newly
**reached**. Import resolution now loads the api, so each file gets past the
parse or import failure that used to stop it and arrives at a check that does
not exist.

**Two of them are the acceptance test for component-satisfies-api.**
`Compiled2.a` expects *"Component Compiled2.a exports API Compiled2.a but does
not define all declarations"* and `Compiled3.g` expects *"The following
declarations in API Compiled3.g are not matched"*. When that check lands, those
two must go red here and come **out** of the list in the same commit. Three more
(`Compiled3.q`, `Compiled5.bq`, `Compiled5.y`) expect `Invalid comprises
clause`, which is DEF-5's `comprises` hole and closes with the same work.
`Compiled1.al` is an operator-expression rule the frontend lane now parses past.
`Compiled1.i` is **undiagnosed** and is the one line in that file with no reason
beside it.

*Note on the mechanism:* `--refresh-lists` rewrites those files with a fixed
header and would **destroy** the per-line reasons above. Maintain them by hand;
the reader strips `#` comments, so a commented block costs nothing.

---

## Named deviations from 1.0

Decided, written down, and permanent unless their stated trigger fires. Not
defects.

| # | Deviation | Source | Trigger that re-opens it |
|---|---|---|---|
| DEV-1 | An ambiguous call is a **compile error** naming the tuple and both declarations, rather than an arbitrary winner | M3c | none |
| DEV-2 | Exclusion is **closed-world** | M3c | D5 — and D5 keeps it |
| DEV-3 | Reduction variables are implemented, against `reduction.tex:15` | M5 | none |
| DEV-4 | **The component algebra is cut.** The closed world is the transitive import closure of one `fortressc` invocation | **D5** | compile time on a real program; `MAX_INSTANTIATIONS` hit by honest code |
| DEV-5 | **Where-clause variables are refused.** v1 where clauses constrain declared static parameters only | **D6 §1** | the library bootstrap needing them after `SPIKE-VARARGS` and D7 |
| DEV-6 | **`Library/QuickSort.fsi` stays refused.** It violates `overloading.tex:100-105`, which `02-stack` records as enforced and permanent | **D6 §4** | none. But run `uniformity-vs-Library` over the other 21 before assuming they share this shape |
| DEV-7 | **`NatReflect.reflect` is refused.** A `nat` static argument must be statically evaluable | **D7** | a corpus program needing a runtime-sized array type |
| DEV-8 | **Distributions are cut** | **D8** | the cluster coming off the shelf |
| DEV-9 | `io` inside `atomic` is a static error in 1.0 and compiles here | M5 | an `io` modifier landing |
| DEV-11 | **String juxtaposition is plain concatenation.** The compiler path inserted a space; we follow the interpreter path, because 237 corpus sites write a space-terminated literal against 93 that do not | **DEF-1**, adjudicated by the semantics lane | a library-sourced `opr juxtaposition` once `CompilerBuiltin.fss` compiles — and then the two libraries' disagreement becomes a real decision, not a choice of default |
| DEV-12 | **`Compiled1.al.fss` is accepted, and the legacy refuses it.** `f(a)^b` at 15:48. VERIFIED against the new operator grammar rather than assumed: it still compiles, and it computes the right answer — `dbl(3)^2` is 81. The only spec rule that could refuse it is the superscripted-postfix rule, `lexical-structure.tex:1204-1213`, and it EXCLUDES this case by its own wording (`^T` is a postfix operator "provided that it is not immediately followed by a word character"; `^b` is not a simple operator preceded by `^` either). The broad reading that WOULD refuse it breaks **383 sites across 103 files**, 6 of them inside the compiling set — overwhelmingly `^2` and `^3`. So the spec supports us, not the legacy | `lexical-structure.tex:1204-1213`, measured | a reading of 1.0's disambiguator that reproduces its refusal without costing the 383 sites |
| DEV-13 | **The shipped 1.0 library violates the `comprises` rule the legacy enforces.** `Library/FortressLibrary.fsi:406` declares `trait QQ ... comprises { ... }` then `trait AnyIntegral extends { QQ }`, and `Library/IntMap.fsi` declares `trait IntMap ... comprises { ... }` then `NonEmptyIntMap extends IntMap` at :63 — byte for byte the shape the legacy refuses in `Compiled5.x.fsi`. **COUNT CORRECTED 2026-08-21: TWO confirmed sites, not the "six across three" first reported.** That number came from a REGEX that read `comprises`-clause members as `extends` targets, and it was wrong in BOTH directions — it invented Reflect.fsi's two and it MISSED IntMap.fsi. Re-taken by compiling every `.fsi` and reading the diagnostic. `CompilerLibrary/FortressLibrary.fsi:419` carries the same text as the confirmed :406 but blocks earlier on `Self`, so it is LATENT and not pre-patched. No `.test` checks the library, so the legacy never ran its own static rules over its own source | `traits.tex:236-241`, measured **with the compiler** | FIRED — patched in source at the two confirmed sites rather than by weakening the rule. See DEV-14 |
| DEV-14 | **`Library/` carries v1 source corrections, and it is a MEASUREMENT INPUT.** 126 of the 1956 corpus files live under `Library/`, so an edit there moves the corpus metric for a reason that is not the compiler. Every edit **under `Library/` or `CompilerLibrary/`** is marked inline, lands in its own commit with its own before/after sweep, and never shares a commit with a compiler change. THE RULE IS ABOUT CORPUS INPUTS AND NOTHING ELSE: `fortressc/runtime/shims.c` is compiler source, not corpus, so a shim change riding with its own tests is not a violation of this | this session | upstream ever being re-synced — the inline markers are what make that diff explain itself |
| DEV-10 | `ProjectFortress/tests/XXXimmutable0.fss` is accepted — an immutable binding may shadow a mutable one | M5 exposed it | folded into the 47-file review above; decide with `XXXtypeParamShadowing`, they are separate edits |
