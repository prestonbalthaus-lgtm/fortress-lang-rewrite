# Integer division by zero and integer overflow: what this compiler does, and why

Two questions were open on the arithmetic lowering. They look like one question
and they are not, and the evidence separates them cleanly.

Everything below was measured or read on disk today, against master `f81f41ace`.

---

## What 1.0 actually says

Both spec trees are the same tree. `diff -rq Specification Specification-1.0-frozen`
returns exactly one differing file and the delta is two LaTeX debug lines in
`basic-lib/objects.tex`. Citations are given against the frozen tree and hold
identically in the other.

### Division by zero — thrown, and named three different ways

`Specification-1.0-frozen/basic/operators/opr-overview.tex:164-170`, the
normative typology:

> The handling of division by zero depends on the type of the number produced.
> For integer results, division by zero throws a `DivisionByZero`.
> For rational results, division by zero produces `1/0`.
> For floating-point results, division by zero produces a `NaN` value according
> to the rules of IEEE 754.

`basic/evaluation/completion.tex:29-31` repeats it as the worked example of a
library-thrown exception. `basic-lib/basic-integers.tex:240-244` and `:459`
spell the same thing `IntegerDivisionByZero` on the `ZZ` operator signatures;
`Library/incomplete/basic/Fortress.Number.fsi:127-131` spells it a third way,
`IntegerDivideByZeroException`. **Only one of the three is declared anywhere:**
`Library/FortressLibrary.fss:1459`, `object DivisionByZero extends UncheckedException`.
The other two are spec-and-header vocabulary with no definition in the tree.

Unchecked matters: no `throws` clause is required at a call site, which is why
`CompilerBuiltin.fsi:201-226` declares `opr +(self, other:ZZ32): ZZ32` with no
`throws` at all.

### Integer overflow — ALSO thrown, and the gap analysis was wrong about this

`04-state.md` and the v1 gap analysis both record that "the spec position on
wrapping is not pinned by anything found on disk." It is pinned.

`Specification-1.0-frozen/basic/operators/opr-overview.tex:195-200`:

> The handling of overflow depends on the type of the number produced.
> For integer results, overflow throws an `IntegerOverflow`.
> Rational computations do not overflow.
> For floating-point results, overflow produces `+∞` or `-∞` according to the
> rules of IEEE 754.

The identical paragraph appears at `:154-159` for multiplication and division.
It is unqualified by width, so ZZ32 addition overflow throws. `IntegerOverflow`
is declared at `Library/FortressLibrary.fss:1516`, also an `UncheckedException`.

Two corroborations that this is deliberate rather than loose prose:

1. **1.0 provides the non-throwing forms as separate operators.**
   `opr-overview.tex:204-209` gives wrapping and saturating addition and
   subtraction their own symbols, and `:171-176` does the same for
   multiplication. A language only needs an opt-in wrapping `+` if the plain one
   throws.
2. **The substrate has the hook designed in.** `advanced-lib/binary.tex:1302-1311`
   gives `BinaryWord` — which `binary.tex:12-18` says ZZ32 and ZZ64 are *built
   out of* — a `signedAdd(other: T, overflowAction: () -> T): T`. The thunk is
   the mechanism the throwing `+` is built from.

Stated honestly: the substrate semantics themselves are unwritten. Every one of
those `BinaryWord` method groups is followed literally by
`[Description to be supplied.]` (`binary.tex:1313, :1331, :1345, :1479`). The
pin rests on the two `opr-overview.tex` paragraphs and on nothing else.

*Also found, and worth recording before somebody trips over it:* the spec
contradicts itself on which symbol is which. `opr-overview.tex:205-208` says
`DOTPLUS` is wrapping and `BOXPLUS` is saturating. `basic-integers.tex:369`,
`ProjectFortress/LibraryBuiltin/CompilerBuiltin.fss:625-626` and the executable
assertions at `ProjectFortress/library_tests/Integer3.fss:99,101` all say the
opposite. Three to one; `opr-overview` loses.

---

## DECISION 1 — division by zero HALTS with a diagnostic. Landed.

Integer division by zero, and integer division of the minimum value by -1, halt
with a diagnostic and exit 1. RR64 division is untouched: `1.0/0.0` is `inf` and
that is what the spec asks for.

**Why a halt rather than the wrapping-free status quo.** The status quo was not
a value at all. A bare `sdiv` **traps** on x86-64 for both of those operand
pairs and raises SIGFPE — rc 136, a core dump, no diagnostic, and `_exit` never
runs so **stdio's buffer goes with it**. `divzero.fss` printed nothing whatever,
including the successful quotient computed on the line above the failing one.
That is not "undefined behaviour" in the abstract; it is a program losing output
it had already produced.

`02-stack.md` already commits to the shape for the array case — a bad subscript
"halts with a diagnostic and exit 1 rather than faulting" — and `shims.c` says
of the dead dispatch arm that "'unreachable' should mean a clean halt with a
diagnostic rather than undefined behaviour." Division simply had neither.

**The named deviation:** 1.0 throws `DivisionByZero`, an `UncheckedException`.
This subset has no exceptions, so it halts instead. It re-opens the day
`SPIKE-EXCEPTIONS-MECHANISM` lands, and at that point `fortress_div_zz64` is the
one place that has to change.

**Why the literal case is a compile error and not the same halt.** The run-time
guard can never see `a/0`: LLVM's own constant folder turns the division into
`poison` while the module is being built, and `println(a/0)` printed `0`. So a
literal zero divisor is refused in the checker, before codegen exists.

Gated by `tools/arith-gate.sh` — 20 checks, 7 mutations, 0 survived.

---

## DECISION 2 — ZZ32 and ZZ64 arithmetic WRAPS. Deviation, recorded, not fixed.

`2147483647 + 1` is `-2147483648`. Two's complement, at both widths, for `+`,
`-` and `*`. 1.0 says this should throw `IntegerOverflow`. **We deviate, and
the deviation is deliberate.**

The reasons, strongest first.

### 1. The project's own phase-7 exit criterion is an overflowed computation

`fortressc/tests/reductionhuge.fss` sums `i i` over `0#1000000000`. The true sum
is 333,333,332,833,333,333,500,000,000 — about **3.3 × 10^26**, some thirty
million times past 2^63. The accepted answer, the one measured today at every
worker count and checked against the closed form `(n-1)n(2n-1)/6`, is
**3338615082255021824**, which *is* that number reduced mod 2^64.

A throwing `+` halts that program somewhere around iteration 2.4 million. The
headline number this rewrite exists to produce is only expressible because
addition wraps. `reductionbig.fss` at 20,000,000 overflows too (~2.7 × 10^21).

### 2. A trapping add would make parallel reduction non-deterministic in its FAILURE

M5's correctness argument for reductions is that two's complement addition is
associative under **any** regrouping, overflow included — which is exactly why
`tools/atomic-gate.sh` can assert a bit-identical answer at 1, 2, 4, 8 and 16
workers for ZZ32 and ZZ64 while pinning `FORTRESS_WORKERS` for RR64.

A trapping add is not associative under regrouping. Sixteen workers accumulate
sixteen partial sums each a sixteenth the size, so a computation that traps at
one worker may not trap at fourteen. The program's *success* would depend on the
worker count. That is a worse failure mode than a wrong-but-deterministic
answer, and it would take the reduction gate's central assertion with it.

### 3. It is on the hot path, and division is not

The guard for division costs a branch on an operation the corpus performs
**zero** times. A guard on `+` costs a branch on the most frequent operation in
the language, in every loop body, at -O0, on a compiler whose only performance
claim is a 10^9-iteration reduction.

### 4. The two failures are not the same class

Division **faults**: the machine kills the process and the diagnostic is a shell
message. Overflow produces a defined, deterministic, documented value. Fixing
the first is closing an undefined-behaviour hole; fixing the second is changing
a defined answer to an exception the language cannot yet throw.

### What re-opens it

`SPIKE-EXCEPTIONS-MECHANISM`. When `IntegerOverflow` has somewhere to go, the
right shape is 1.0's own: `+` throws, and `BOXPLUS`/`DOTPLUS` are how a
reduction opts out. `reductionhuge.fss` would then have to be rewritten in terms
of the wrapping operator, and that rewrite is the acceptance test for the change.
Until then the deviation is recorded here and in `02-stack.md`, not silent.

**Not decided here, and it should be:** whether `ZZ32`'s declared range belongs
in the compiler at all. `Library/FortressLibrary.fss:645-646` writes the minimum
as `-2147483647 - 1` because the literal `-2147483648` is not writable — the
lexer has no negative literals and `int_literal` refuses `2147483648` in a ZZ32
slot. That is a second, smaller thing the corpus will trip over
(`fortressc/tests/divoverflow32.fss` has to build the value the same way), and
it is a literal-and-lexer question, not an arithmetic one.
