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

### DEF-1 — String juxtaposition drops the space. `bcat` where the oracle records `b cat`

| | |
|---|---|
| **Owner** | Group 3 (correctness debt), with a one-line runtime change that touches Group 4's lowering |
| **Class** | **Silent wrong output at exit 0** — the worst class this project recognises |
| **Found by** | `tools/oracle-gate.sh`, the first execution of any corpus program against its recorded output in this project's history |
| **Status** | open, diagnosed to primary source, not fixed |

**Reproducer** (`ProjectFortress/other_compiler_tests/GenMet0.fss`, and it is
four lines):

```
trait a   m[\T extends String\](x:T) = println("a" x)  end
trait b extends a   m[\T extends String\](x:T) = println("b" x)  end
object o extends b end
run():() = o.m[\String\]("cat")
```

Observed `bcat`. Oracle records `b cat`. Exit 0 both ways.

**Scope.** Four cases fail today — GenMet0, GenMet1, GenMet2, GenMet3, all under
`ProjectFortress/other_compiler_tests/GenMet.test`. **Seven `.test` cases pin the
behaviour** and they agree with each other: GenMet5, GenMet6 and GenMet8 expect
`b cat\na 3\n`, so the space appears for `String ZZ32` juxtaposition as well, not
only `String String`. The other three are currently *blocked* rather than
failing, so fixing this without fixing them will move the count by four.

**Diagnosis — settled from the legacy source, not inferred.**

`ProjectFortress/LibraryBuiltin/CompilerBuiltin.fss:384` defines juxtaposition on
the **compiler** path, which is the path `other_compiler_tests` exercises:

```
opr juxtaposition(self, b:Object): String = self ||| b
:410  opr |||(self, b:Object): JavaString = jSmartConcatenate(self, b.asString.asJavaString)
```

and `jSmartConcatenate` is
`ProjectFortress/src/com/sun/fortress/nativeHelpers/simpleConcatenate.java:20`:

```java
public static String nativeSmartConcatenate(String s1, String s2) {
    if (s1.length() == 0) return s2;
    else if (s2.length() == 0) return s1;
    else return s1 + " " + s2;
}
```

**So String juxtaposition is `|||`: space-inserting concatenation, with an
exception for an empty operand on either side.** Our lowering
(`crates/codegen/src/lib.rs:1586`) calls `concat_string_string`
(`runtime/shims.c:558`), which is plain `s1 + s2` — that is `||`, not `|||`.

**THE TRAP, AND IT IS WHY THIS ENTRY IS LONG.** The two 1.0 libraries
**contradict each other** on this operator, and the one a reader will find first
is the wrong one. `Library/FortressLibrary.fss:4047-4050` — the **interpreter**
library — says:

```
(** Right now for backward compatibility juxtaposition works like %||% **)
opr juxtaposition(a:Any, self):String = (""||a) || self
opr juxtaposition(self, b:String):String = self || b
```

That is plain concatenation and it agrees with what we emit. **Do not "fix" the
expectation to match it.** This is the same class as the `trait ZZ32`
contradiction between `CompilerBuiltin.fsi:412` and `FortressLibrary.fsi:461`
that gap analysis §2.7 records: two shipped libraries, two answers, and the
compiler path is the one the `.test` oracle was produced from.

**Fix.**

1. Add a space-inserting concatenation shim next to `concat_string_string` in
   `runtime/shims.c`, mirroring `nativeSmartConcatenate` **including both
   empty-operand guards** — those are not an optimisation, they are observable
   (`"" juxt "x"` is `"x"`, not `" x"`).
2. Point the juxtaposition lowering at it. Leave `concat_string_string` where it
   is: it is `||`, which is a different operator and will need it when infix
   `||` lands (see `SPIKE-OPEXPR`).
3. Fixture with all four shapes: `String String`, `String ZZ32`, empty left,
   empty right.

**Acceptance.** `tools/oracle-gate.sh` moves pass 285 → 289 and fail 51 → 47,
with the four GenMet cases leaving the fail bucket. **Raise `PASS_FLOOR` to 289
in the same commit** or the ratchet stops guarding the win. Per the repo's own
recorded lesson, verify with an **IR diff over the compiling set** as well as the
exit-code count — a checker fix once moved 280 → 280 with zero list delta and
only the IR diff showed the four real changes.

**Do not generalise past the shim.** This is a builtin whose behaviour is
currently hardcoded in the compiler. Once §2.7's builtin decision lands and
`CompilerBuiltin.fss` supplies its own definition, this shim becomes the
implementation *behind* that declaration. Making the shim match
`nativeSmartConcatenate` byte for byte now is what makes that later handover a
no-op.

---

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
| DEV-10 | `ProjectFortress/tests/XXXimmutable0.fss` is accepted — an immutable binding may shadow a mutable one | M5 exposed it | folded into the 47-file review above; decide with `XXXtypeParamShadowing`, they are separate edits |
