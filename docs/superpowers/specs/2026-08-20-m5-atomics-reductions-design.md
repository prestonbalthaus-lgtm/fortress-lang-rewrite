# Fortress M5: atomics and reduction variables — architecture

Date: 2026-08-20
Status: **design for review. No compiler code written.**

Every `.tex` citation is against `Specification-1.0-frozen/`. Every number is
measured on this laptop, 14 cores, and the scratch programs that produced them
are named where they matter.

## The whole design, in one page

| question | answer | why |
|---|---|---|
| syntax | `atomic AtomicBack`, one production, both corpus shapes | `atomic.tex:18-24` already writes it; `atomic` is already a reserved token |
| AST | `Expr::Atomic { body }` — type-transparent | `atomic.tex:42-43`: the value and type are the body's |
| reduction syntax | **none.** Recognised from `count: ZZ32 := 0` + `count += e` | `reduction.tex:78-88` writes one with no annotation of any kind |
| new lexing | none. `+=` is `Plus` glued to `Eq` by span adjacency | same trick as `<-` and `->`; fifth milestone with no lexer change |
| atomic engine | ONE recursive `pthread_mutex_t`, two shims | `atomic.tex:89-90` leaves the mechanism to the implementation; a plain mutex self-deadlocks on nested atomic, measured |
| not STM | the legacy STM's commit protocol was a global lock anyway, and it taxed every program that never said `atomic` | `Transaction.java:25`, `VarCodeGen.java:369-436` |
| not `atomicrmw` | a per-iteration atomic is 2.4x slower than SERIAL at 14 workers; a per-iteration mutex is 13.7x slower | measured; and `MAX`, `fadd`, `fmax` are cmpxchg loops anyway |
| allocation inside atomic | ALLOWED | Boehm suspends by signal, preemptively; 1.6M lock-held allocations over 2770 collections, 3/3 clean |
| reduction engine | per-worker cacheline-padded accumulators, merged on the calling thread in worker order | `reduction.tex:60-77` names the OpenMP strategy outright; 8.2x on 14 workers |
| ABI | one argument APPENDED: `body(index, env, chunk)`; all five serial paths pass 0 | appending leaves `get_nth_param(0..1)` alone |
| race rule | M4's `escapes_loop` gains ONE carve-out: escaping assignment allowed if inside `atomic`, or if it is the recognised reduction shape | still one syntactic comparison, still no dataflow |
| the easy-to-miss half | M4 captures BY VALUE, so the lock path also needs **capture by reference** (`Slot::Cell`) or the update is lost with the lock held | `codegen/src/lib.rs:913`, `:1001`, `:146-149`; needed for §0.5b, not for the payoff |
| step 0, before any of this | M4's race rule does not survive a function call, and a `seq` loop assigning to an outer scalar is an exit-70 crash. Both measured on master | §0.5 |
| two ways the lock hangs | top-level `atomic` around a parallel loop deadlocks; a halt while the lock is held hangs in `atexit`. Both fixed in two lines each, measured | §2 |
| what the payoff actually exercises | all 7 write `+=` and take the REDUCTION path; the lock path has zero customers in the unlock set | §2 |
| corpus payoff | **7 files**, 266 → 273, and only with `var` and `+=` alongside. `atomic` alone unlocks ZERO, and only ONE of the 7 ever starts a thread | measured by disabling `escapes_loop` and rebuilding, §0 |

---

## 0. The corpus number, measured instead of counted

The premise was "M5 unlocks the final 102 legacy corpus files." 102 is the
SUBSTRING count — it counts `tryatomic`, `TransactionalArrayShakedown` and the
word inside comments. Word-boundary it is **92 `atomic` + 7 `tryatomic`, 95 in
the union** (4 use both), and 8 of the 92 say it only in a comment or a string.

Counting first blockers has been wrong four milestones running, so this was run
as an experiment — and the FIRST version of the experiment was still a
first-blocker count in disguise, so it got the wrong answer too. Both are below,
because the difference between them is the lesson.

**Weak version.** Elide every code-position `atomic` token, rewrite
`var x: T = e` → `x: T := e`, desugar `x op= e` → `x := x op (e)`, run the real
driver: 1 file compiles and 11 more stop on M4's escape rule. Conclusion "12".

**Strong version.** Do all of that AND patch `escapes_loop` to `false`
(`types/src/lib.rs:1805-1807`), rebuild the compiler, and see what is actually
left behind that rule:

| build | exit 0 | exit 70 | exit 1 |
|---|---|---|---|
| as-is | **0** | 0 | 95 |
| `atomic` elided | **0** | 0 | 95 |
| plus `var` rewritten | **0** | 0 | 95 |
| plus every `op=` desugared | **1** | 0 | 94 |
| plus `escapes_loop` disabled | **1** | **6** | 88 |

**The ceiling is 7, not 12.** The 6 land on exit 70 — ``{name}` was assigned to
but has no storage`, which is precisely the by-reference capture gap §2 fills, so
they compile end to end once M5 exists. Five of the weak version's 11 have
another blocker behind the escape rule: `XXXloopError.fss` is a
must-never-compile negative test, `nestedTransactions1/2/4` die on
`unknown name recordTime`, `tests/atomic1.fss` on `expected (), found ZZ32`.

The 7, by name:

```
SpecData/examples/preliminaries/Overview.Expression.atomicE.fss   (already exit 0)
ProjectFortress/other_compiler_tests/atomic5.fss  atomic6.fss
ProjectFortress/tests/atomic2.fss  atomic3.fss  atomic4.fss  atomicTest.fss
```

**So: 266 → 273, and "final" is not the word.** 88 of the 95 stay blocked on
`spawn`, `also`, `value`, `try`, `label`, `opr`, `at`, `||` (which does not parse
at all today), tuples, local functions, non-ASCII and radix numerals — and ~1683
non-atomic files are untouched.

**One more number, and it is the honest one.** Trip counts across the 7 are 500,
300, 500, 300, 300, **30000** and none. `FORTRESS_PARALLEL_MIN` is 4096
(`shims.c:81`, enforced at `:249`), so **six of the seven run inline on one
thread.** `tests/atomicTest.fss` is the only corpus file M5 unlocks that ever
starts a worker. Whatever M5 is worth, it is not worth it because of the corpus.

Two more corpus facts that shape the design:

* `+=` **does not parse today.** `s += i` gives *expected an expression, found
  Eq* — it lexes fine as `Plus` then `Eq`, it just means nothing to the parser.
  The fix is span adjacency, exactly as `<-` and `->` already are. **No lexer
  change** — the last one was M3h (`b816dd610`), so M3i, M3j, M3k and M4 all
  landed without touching it and M5 makes five.
* **The reduction type set is ZZ32, and that is not a detail.** Declared types of
  compound-assignment targets corpus-wide: **ZZ32 144**, String 9, `List[\Read\]`
  7, RR64 6, ZZ64 6. All seven unlockable files declare `var count: ZZ32`. A
  design that recognised reductions on ZZ64 and RR64 only would ship a milestone
  with **zero** customers. Operator traffic is just as skewed: `+=` 239 hits,
  `\|\|=` 37, `-=` 17, then `UNIONCAT=` 9, `UPLUS=` 9, `TIMES=` 7 and a tail of
  ones and twos. `+=` is 76% of every compound assignment in the tree.

M5 is still judged the way M4 was — **by a gate, not by `COMPILE_FLOOR`** — but
the floor does move, by 7, and the 7 are named so the claim is falsifiable.

---

## 0.5 What M4 actually guarantees, measured — two things M5 must fix first

M5 relaxes M4's race rule. Before relaxing a rule it is worth knowing the rule
does not hold today. Two defects, both reproduced on `master` (`cd2458cc0`).

### (a) `escapes_loop` does not survive a function call. Silent wrong answers.

`Checker::assign` consults `self.parallel_loop()` (`types/src/lib.rs:1813`), and
`loop_ctx` is a property of the checker walking ONE function body. A top-level
function is checked with an empty `loop_ctx`, so the guard is simply not entered.
Arrays are captured by pointer. So a loop body can hand a shared array to a
callee and the callee's `a[j] := v` is never checked against anything:

```fortress
bump(a: Array[\ZZ64\]): () = do  a[0] := a[0] + 1  end
run(): () = do
   a: Array[\ZZ64\] = array(1)
   a[0] := 0
   for i <- 0#4000000 do  bump(a)  end
   println(a[0])
end
```

Compiles clean, exit 0. Five runs: **567137, 775186, 895320, 849373, 457075**.
`FORTRESS_WORKERS=1` gives 4000000. No diagnostic, ever.

M4's claim is not "a parallel body cannot race", it is "a parallel body cannot
race *in an assignment written lexically inside it*". The gap is the call. That
is the honest statement, and it should replace the M4 wording in the docs.

**This is M5's real step 0**, ahead of everything else in this document, because
M5's whole contribution is to make the loop-body rules WEAKER. The cheap fix is
one check at the call site — refuse passing a loop-captured `Array` to a call
inside a parallel body. The right fix is a whole-program reachability pass, and
the compiler is already whole-program for M3c dispatch, so the machinery exists.

### (b) A `seq` loop that assigns to an outer scalar is an exit-70 internal error

And the compiler's own diagnostic walks the user into it:

```
for i <- 0#1000 do total := total + i end
  -> `total` is declared outside this loop ... Write `for ... <- seq(...)`
     for a sequential loop                                          (exit 1)

for i <- seq(0#1000) do total := total + i end
  -> fortressc: internal error: `total` was assigned to but has no storage (exit 70)
```

`assign` in codegen requires a `Slot::Cell` (`codegen/src/lib.rs:1539-1547`), an
outlined body binds every capture as `Slot::Value` (`:1001`), and a `seq` loop is
outlined through the same path. Following the remedy text at
`types/src/error.rs:579-584` produces an internal error on **valid** source,
which breaches the standing rule that malformed input is a diagnostic and never
a crash — and this input is not even malformed.

Latent rather than absent: M4's full-driver sweep reported zero exit 70 because
no corpus file happens to write a `seq` loop that assigns to an outer scalar.
**M5 fixes it as a side effect** — the capture-by-reference in §2 is the whole
repair — and the gate must carry it as its own case or it goes latent again.

---

## 1. Syntax and AST

**The grammar is already written and it needs one production.**
`basic/expressions/atomic.tex:18-24`:

```
FlowExpr    ::= atomic AtomicBack | tryatomic AtomicBack
AtomicBack  ::= AssignExpr | OpExpr | DelimitedExpr
```

Both corpus shapes are that one production. Comment-stripped and
string-masked, the 183 code occurrences of `atomic` split into **102
`atomic do ... end`** (in 61 files) — that is `atomic DelimitedExpr`, and
`do ... end` is already a delimited expression in our parser — and **69
`atomic <expr>`** one-liners, which is `atomic AssignExpr`. There is no block
form and no expression form to keep separately correct: one node, one body.

`atomic` is already `Kind::Reserved("atomic")` (`lexer/src/token.rs:146`) and
already refuses by name. New AST node, and it is deliberately thin:

```rust
Expr::Atomic { body: Box<Expr>, span: Span }
TypedExprKind::Atomic { body: Box<TypedExpr> }
```

`atomic.tex:42-43` — *"The value and type of an `atomic` expression are the
value and type of its body expression."* So it is an expression, it is
type-transparent, and the checker just forwards `expected` through it.

Inside that 102 sits the loop-header form,
`DoFront ::= [at Expr] [atomic] do [BlockElems]` (`for.tex:21`,
`DelimitedExpr.rats:155-157`) — **6 uses in 3 files** written as `for i <- g atomic do`. It
desugars in the parser to `Expr::Atomic` wrapping the body, so nothing
downstream knows the form existed. A further **11** are `also atomic do`, where
one `end` closes the whole chain and each clause carries its own flag; `also` is
not in the subset, so those refuse on `also` regardless, and 10 of the 11 are
one file (`tests/AlsoDo.fss`).

**Reduction variables get NO syntax at all, and that is the finding.**
`reduction.tex:78-88` writes the canonical reduction as:

```
sum: ZZ64 := 0
for i <- a.indices do
    sum += a[i]
end
sum
```

There is no declaration, no annotation, no `reduce` keyword. A reduction
variable is *recognised*, not declared, and `reduction.tex:28-39` gives the
recogniser as three conditions that are all syntactic:

1. every assignment to `l` in the thread group has the form `l ⊕= e`, with
   exactly one operator `⊕` or its group inverse `⊖`;
2. `l` is not otherwise READ within the thread group;
3. `l` is not a free variable of a functional, including a field of a receiver.

Not one of those needs dataflow. They are the same class of check as M4's
`escapes_loop`, which is the house rule for this whole area.

Refused at the parser with a named diagnostic: `tryatomic` (it throws
`TryAtomicFailure`, and we have no exceptions), and `atomic` as a modifier on a
functional — which is not a dodge, because `atomic.tex:15` says 1.0 did not
support it either.

---

## 2. The atomic engine: one global recursive mutex

Not STM. Not cmpxchg-first. **One recursive `pthread_mutex_t` in `shims.c`**,
and two shims:

```c
void fortress_atomic_enter(void);
void fortress_atomic_leave(void);
```

Generated code brackets the body and calls nothing else.

One implementation detail that is not free to get wrong:
`PTHREAD_RECURSIVE_MUTEX_INITIALIZER_NP` **does not compile without
`_GNU_SOURCE`** — checked, `error: ... undeclared here` — and `shims.c` defines
no feature-test macro today. Adding one to a file whose entire job is
portability is the wrong trade, so the mutex is built by `pthread_mutexattr_settype`
under a `pthread_once` inside `fortress_atomic_enter`. A program with no
`atomic` in it never executes that line.

Here is why each of the alternatives loses.

### Conformant, and the reference implementation was a global lock too

`atomic.tex:89-90`: *"The exact mechanism by which this occurs will vary; the
necessary serialization is provided by the implementation."* Serialization is
required; retry is named as something that *may* happen, never as something
that must.

And before calling a mutex a shortcut: the legacy STM's commit protocol WAS a
global lock. `Transaction.java:25` is `private static AtomicInteger global_lock`,
one counter for the whole process; `TXCommit:238` CASes it before any write and
sits ABOVE the top-level branch at `:244`, so even a nested commit serializes
the process. Zero write concurrency, by construction, at every depth — the same
serialization a mutex gives — paid for with a deferred-update read set,
O(read-set) revalidation, and an unbounded retry whose backoff is a no-op by a
bug (`BaseTask.java:194-197` measures the elapsed time before the delay).

**And it taxed every program that never says `atomic`.**
`VarCodeGen.java:369-398` and `:408-436` emit an `inATransaction()` call, an
unchecked `Thread.currentThread()` downcast, two field loads and a branch at
EVERY read and EVERY write of EVERY mutable variable, plus a heap-allocated
`MutableFValue` box per mutable local (`:351-359`). That is the rule M5
inherits: **a program containing no `atomic` must emit byte-identical IR to
today's**, and §5 checks it.

(`CodeGen.java` also has `forAtomicBlock` and no `forAtomicExpr`, so the legacy
native backend compiled `atomic do ... end` and threw `sayWhat(x)` on
`atomic <expr>`. Our one node covers both corpus shapes; theirs never did.)

### Nesting: measured, not assumed

`atomic.tex:72-75` permits atomic expressions to nest arbitrarily. With a plain
mutex an `atomic` that calls a function containing another `atomic`
self-deadlocks — **measured, 5s timeout, hangs; the same program on
`PTHREAD_MUTEX_RECURSIVE` completes.** So the mutex is recursive, and nesting
flattens: with one lock there is no evaluation that is dynamically outside the
inner and inside the outer, so the clause is satisfied trivially.

### The GC question, which could have killed the design

If Boehm's stop-the-world needed threads to reach a cooperative safepoint, a
thread suspended holding the atomic lock would deadlock the collector and
allocation inside `atomic` would be forbidden. It does not: suspension is
**signal-based and preemptive** — `pthread_stop_world.c:980-993` sends
`GC_sig_suspend` with `pthread_kill`, the handler acks with `sem_post` (`:392`)
and then `sigsuspend`s (`:406`). A thread blocked in `pthread_mutex_lock`, or
holding it, is suspended where it stands and still acks, and the collector never
acquires the application lock, so the lock order cannot cycle.

**Shown, not asserted:** 8 threads, a global mutex, `GC_malloc` INSIDE the
critical section, 1.6M lock-held allocations, **2767–2774 collections, 13
parallel markers, 3 runs out of 3 clean.** Allocation inside `atomic` is
allowed, and M5 needs no allocation-free-body rule the way M4's loops did.

### What the lock costs, and it is brutal

20M iterations, 20 ops per iteration, best of 3, 14-core laptop. Times in
seconds.

| update per iteration | 1 | 2 | 4 | 8 | 14 |
|---|---|---|---|---|---|
| private accumulator, merged at the end | 0.1164 | 0.0700 | 0.0399 | 0.0224 | **0.0142** |
| `lock xadd` (`atomicrmw add`) | 0.1294 | 0.2812 | 0.2682 | 0.2632 | 0.2789 |
| global mutex | 0.1622 | 1.1810 | 1.3434 | 1.5938 | 1.5638 |

The serial loop is 0.1164. **A mutex on every iteration is 13.7x SLOWER than
serial at 8 workers. An atomic add on every iteration is 2.4x slower at 14.**
Neither mechanism scales; contention on one cache line is the wall, and which
instruction hits it barely matters.

At what contention rate does it stop mattering? Same loop, 8 workers, the
update taken 1 iteration in K:

| 1-in-K | private | atomic add | mutex |
|---|---|---|---|
| 1 | 0.0224 | 0.2740 | 1.6621 |
| 4 | 0.0218 | 0.1769 | 0.7356 |
| 16 | 0.0226 | 0.0425 | 0.2312 |
| 64 | 0.0216 | 0.0228 | 0.0493 |
| 256 | 0.0216 | 0.0221 | 0.0228 |
| 1024 | 0.0215 | 0.0218 | 0.0219 |

**The lock breaks even against the SERIAL loop between 1-in-16 and 1-in-64**,
and is free by 1-in-256. So `atomic` is correct for the rare update — the
`SkipList.fss` insert, the `FileSupport.fss` consume flag — and catastrophic
for the per-iteration counter. That is the whole justification for §3.

### Why not lower `atomic x += e` straight to `atomicrmw`

Because it is not actually cheap, and it is not even one instruction outside the
integer `+` case. `llc 22.1.8`, `-O0`, `-mcpu=x86-64-v3`:

| IR | x86-64 at -O0 |
|---|---|
| `atomicrmw add i64 monotonic` | `lock xaddq` — one instruction |
| `atomicrmw add i64 seq_cst`, result unused | `lock addq` — seq_cst is free on x86 for RMW |
| `atomicrmw or i64` | `lock orq` |
| `atomicrmw max i64` | **`atomicrmw.start` cmpxchg loop** |
| `atomicrmw fadd double` | **cmpxchg loop** |
| `atomicrmw fmax double` | **cmpxchg loop** |

All of it holds at `-O0` and none of it emits a libcall, so `AtomicExpandPass`
is the only pass involved and the result does not depend on optimisation. But
`MAX` costs a CAS loop even on integers, and RR64 — which is what
`Fortify/example/buffons.fss:36` actually writes — costs one too:

| RR64 update every iteration | 1 | 2 | 8 | 14 |
|---|---|---|---|---|
| `fadd` via cmpxchg loop | 0.1875 | 0.5966 | 0.9171 | 0.9640 |
| `fadd` via mutex | 0.1768 | 0.9822 | 1.6837 | 1.6281 |

A CAS loop under contention retries, so the float path is the worst case in the
whole milestone: **5x slower than serial at 14 workers.**

And the API does not even reach the case that matters. `inkwell` 0.10.0 is
pinned in `codegen/Cargo.toml:12`, and `build_atomicrmw` takes `value: IntValue`
— integers only, with a TODO at `builder.rs:3751` to *"add support for fadd,
fsub and xchg on floating point types"*. An `RR64` reduction has no `atomicrmw`
path through this inkwell at all; it would be a hand-rolled `cmpxchg` loop on
the bit pattern, which is what `AtomicExpandPass` writes anyway.

Conclusion: `atomicrmw` buys one instruction in one case and loses everywhere
else. **One lock, one lowering, one thing to prove.** The performance answer is
not a faster atomic — it is not taking the atomic at all, which is §3.

(For the record: codegen emits **zero** atomic instructions and zero fences
today, verified over the emitted IR of the loop fixtures. All synchronisation in
a Fortress binary is C-side. M5 keeps it that way.)

### Two ways one global lock hangs, both measured, both two lines to fix

Neither is theoretical. Both were reproduced against the REAL `runtime/shims.c`
with a recursive mutex bolted on exactly where M5 would put one.

**(i) `atomic` at top level around a parallel loop deadlocks.**
`fortress_in_parallel` is a `__thread` flag (`shims.c:118`) set only inside a
worker and on the calling thread *during* a loop. So an `atomic` written INSIDE a
loop body is safe by accident — the nested loop runs inline. An `atomic` written
OUTSIDE any loop is not: the inner `for` really does distribute, the pool workers
block on the mutex the main thread holds, and the main thread parks at the join
(`shims.c:283-287`). **A recursive mutex does not help — recursion rescues
re-entry by the SAME thread, and the workers are different threads.**

```
main: atomic do  for i <- 0#1000000 do atomic ... end  end
  ->  timeout, 3 runs out of 3
```

The fix is not a checker refusal, because the checker cannot see a loop reached
through a call (§0.5a). It is two lines in the runtime: **`fortress_atomic_enter`
sets `fortress_in_parallel = 1` and `fortress_atomic_leave` restores it**, so any
loop started inside an atomic region runs inline — the same mechanism M4 already
uses for a nested loop, and exactly what `atomic.tex:77-81` asks for when it says
implicit threads created inside an atomic must complete before it does. Measured
with the fix applied: **3 runs out of 3, `counter=1000000` exactly.**

**(ii) A halt while the lock is held hangs the process forever.**
`fortress_pool_stop` is an `atexit` handler (`shims.c:226`) whose body is
`pthread_join` over the pool (`:160-169`). A worker parked in
`fortress_atomic_enter` on a mutex the exiting thread still holds can never be
joined. There are three `exit(1)` sites — `fortress_halt:331-333` (out-of-bounds
subscript, negative integer exponent, bad array length), `fortress_assert_failed:405`,
`fortress_dispatch_failed:529` — so five ordinary source constructs reach it.

```
worker 0: out-of-bounds subscript inside atomic
fortress: array index out of bounds (99, 4)
  ->  timeout, 3 runs out of 3.  The diagnostic prints, then nothing.
```

Under `srun` that is a job burning its entire wall-clock allocation on a dead
process. **M4 alone does not have this** — the same program without the atomic
mutex exits 1 cleanly, checked — so it is a defect M5 would INTRODUCE, and it has
to be fixed in the same milestone. The fix is `fflush(NULL); _exit(1)` at those
three sites: an abnormal halt has no business running `atexit` handlers.
Measured with the fix applied: **3 runs out of 3, exit 1, diagnostic intact.**

### What the lock writes to, which is the half that is easy to miss

**M4 captures by VALUE.** `parallel_for` stores `load_name(&capture.name)` into
the environment (`codegen/src/lib.rs:913`) and `define_loop_body` binds each
capture as `Slot::Value` (`:1001`). There is no store target, so an assignment
to a captured name in an outlined body has nothing to write to — and a naive
build that kept by-value capture would be **silently wrong even with the lock
held**: every worker would increment its own loop-entry copy, and 8 workers ×
1M increments would produce entry+1 per worker instead of 8M. Lock acquired,
update still lost. That is the failure the whole milestone would ship with.

So the atomic path needs **capture by reference**, and the machinery is already
there: `Slot::Cell { pointer, ty }` exists (`:146-149`) and `load_name` already
handles it. For a scalar that is assigned, or read-modify-written, inside an
`atomic` in a loop body, the env slot holds the ADDRESS of the caller's
`alloca` rather than the value, and the body binds it as `Slot::Cell` — so the
read and the write inside the lock both hit live storage. The checker already
knows which captures need it: mutable, and assigned under an enclosing
`Atomic` node.

**The lifetime is safe by construction.** `fortress_parallel_for` blocks on the
done-wait before it returns (`shims.c:283-287`), so the caller's stack slot
outlives every worker's use of it. No heap box, no escape analysis.

Two corollaries:

* **Arrays need none of this.** The captured value is the array pointer, and a
  write through it already reaches live storage. That is why `SkipList.fss`'s
  `destinations[index] := ...` pattern works against today's outliner unchanged.
* **The Index arm of the carve-out is separate and has to be named.** Inside an
  `atomic`, `ParallelIndexNotBinder` is relaxed too — any index into a shared
  array is allowed, because the lock is what makes two iterations writing the
  same slot safe. §3's carve-out sentence covers `Var` targets; this covers
  `Index` ones, and `SkipList.fss` is the reason both exist.

**And here is the uncomfortable part: the lock path has no customer in the
unlock set.** All 7 files in §0 write `+=` and nothing else —
`atomicTest.fss:19` is `atomic do count+= 1 end`, `atomic5.fss:22` is
`atomic do count1+= 1; count2+=2; count3+=3; count4+=4 end` (four reduction
variables in one block), `atomic4.fss` nests two. **Every one of them goes down
the reduction path, where nothing is captured at all.**

The `:=`-inside-`atomic` shape does exist —
`other_compiler_tests/atomic3.fss:19-21` is
`for i <- 1#1000 do atomic do count := count + 1 end end` with
`assert(count = 1000)`, which is exactly the by-reference case — but that file
is blocked on `||`, and the lock path's other real customers (`SkipList.fss`,
`FileSupport.fss`, `HeapShakedown.fss`) are blocked on features far beyond M5.

Capture-by-reference is still required, for two reasons that have nothing to do
with the unlock set: it is the entire fix for the exit-70 crash in §0.5b, and
without it the lock path is a lock around a private copy. It just should not be
sold as corpus payoff.

### The rest of the engine, so nothing is implicit

| case | answer |
|---|---|
| the five serial paths — `seq`, range under 4096, nested, pool failed, no pool at all (`shims.c:236`, `:118`) | one mutex covers all of them; an uncontended `pthread_mutex_lock` is a couple of ns and needs no special case |
| a `for` INSIDE an `atomic` body | allowed, and it has to be: `tests/atomic4.fss` is `atomic do for i <- 1#300 do atomic do count += 1 end end end`, one of the 7. A static refusal would cost 14% of the whole payoff. The serial-region fix above is what makes it safe |
| implicit threads inside `atomic` (`atomic.tex:77-81`) must finish first | M4 already demotes a nested parallel loop to sequential — no new code, but the demotion stops being an optimisation and becomes load bearing, so the gate says so |
| `io` inside `atomic` is a static error in 1.0 (`atomic.tex:55-57`) | we have no `io` modifier; `println` inside `atomic` compiles and is serialised. A named deviation, not a silent one |
| rollback on abrupt completion (`atomic.tex:59-70`) | our subset has no exceptions and no `label`/`exit`, both reserved-and-refused, so **no M5 atomic body can complete abruptly**. Unreachable, not violated — and it re-opens the day exceptions land |

---

## 3. The reduction pipeline

**The spec names the implementation.** `reduction.tex:60-77` blesses *"the one
used in OpenMP: A reduction variable `l` is assigned `Identity[⊕]` at the
beginning of each iteration... When all iterations are complete, the initial
value of the reduction variable and values of the variable at the end of each
implicit thread are reduced and the result is assigned to the reduction
variable."* Private accumulator per worker, merged at the end — the 8.2x column
in §2, and the only one that scales.

### The licence to skip the lock

Can `atomic sum += a[i]` legally become a private accumulator, when `atomic`
promises the update is atomic with respect to threads dynamically outside? Yes,
and one sentence is the whole authority — `reduction.tex:40-42`: *"Other threads
which simultaneously reference a reduction variable while a loop is running may
see an arbitrary value for that variable. Any updates performed by those threads
may be lost."* The reduction rules deliberately override atomic's visibility
guarantee for a recognised reduction variable, so if the recogniser is wrong
about condition 2 the licence does not apply and the answer is wrong.

`:43-46` gives the other half — *"The association of terms in the reduction is
arbitrary"* — and `defining-generators.tex:57-62` goes further: a generator may
*"insert an arbitrary number of empty elements"*, observable on signed zero.
Reassociation is conformant, bitwise equality with the serial fold is promised
nowhere, and `reproduc` appears zero times in the whole specification.

### The ABI change: one argument

The outlined body is `void (*)(int64_t index, void *env)` (`shims.c:83`) and a
worker knows nothing about itself. Reductions need per-worker storage, so:

```c
typedef void (*fortress_loop_body)(int64_t index, void *env, int64_t chunk);

void fortress_parallel_for(int64_t lo, int64_t hi, fortress_loop_body body,
                           void *env, int64_t requested);
```

`chunk` is the worker index `w` that `fortress_chunk` already computes, and
`fortress_run_chunk` (`shims.c:120`) already has it in hand — it is one
argument forwarded, not new plumbing. **All five serial paths pass 0**, so one
lowering serves the parallel and the sequential case and there is no second code
path to keep correct — the same argument that made M4 lower `seq` and parallel
identically.

The new parameter goes LAST on purpose. Putting the worker second would
renumber `env` from `get_nth_param(1)` to `(2)` in `define_loop_body`
(`codegen/src/lib.rs:982`), and `get_nth_param` returns an `Option` — a wrong
index is an internal error at run time, not a compile error. Appending changes
no existing index.

Nothing else about the engine moves. The split stays a pure function of
`(lo, hi, workers)`, `fortress_parallel_chunk` (`shims.c:292`) stays exported,
and **`parallel-gate.sh`'s independent partition check keeps working unchanged**,
which is the property that makes the gate worth anything.

### Storage and merge

* The accumulators are **one scanned block of `workers × reductions`
  cacheline-padded slots** — a loop may reduce more than one variable, and
  `buffons.fss:22-23` reduces two (`hits` and `n`) — allocated once beside the
  environment struct, before the loop. Padding is not
  hygiene: 20M bare updates, padded versus a plain `int64_t[16]`, best of 3 —
  0.0055 vs 0.0078 at 8 workers and **0.0036 vs 0.0093 at 14, 2.6x**, and the
  unpadded version gets *worse* from 8 to 14 workers.
* Each slot is initialised to `Identity[⊕]` — `0` for `+` and `-` on ZZ32,
  ZZ64 and RR64 — before the loop.
* Codegen rewrites `l ⊕= e` inside the body into `slot[chunk] ⊕= e`. The
  captured `l` is not in the environment at all, so there is nothing shared to
  race on.
* The merge runs **on the calling thread, after the done-wait, in worker order
  0..P-1**, starting from `l`'s value at loop entry. `reduction.tex:70-73` wants
  the initial value included, and folding in a fixed order makes the result
  **deterministic for a fixed worker count** — byte-identical run to run.
* `-=` (17 corpus uses) is `reduction.tex:56-62`'s group inverse: accumulate
  `Identity ⊖ e` and merge with `⊕`. No second accumulator kind.

**RR64 is deterministic per worker count and NOT across worker counts.** That is
inherent to reassociation, the spec permits it, and the gate must pin
`FORTRESS_WORKERS` and print the spread rather than assert an equality that is
not true. ZZ32 and ZZ64 are unaffected: two's-complement addition is associative whatever
the grouping, so the merged sum is bit-identical to the serial fold including
on overflow — which is why (2) in §5 can assert exact equality and (3) cannot.

An empty range needs no special case: `fortress_parallel_for` returns early on
`hi <= lo` (`shims.c:241`), the slots still hold `Identity`, and the merge —
which is emitted in the CALLER, after the call — yields `l`'s entry value.

### The recogniser, and the one carve-out in `escapes_loop`

M4's whole race-freedom argument is one comparison (`types/src/lib.rs:1805`):
a parallel body may not assign to a name that resolves below the loop's floor.
A reduction variable is exactly that — an outer binding being assigned. So the
rule gains one carve-out and stays one rule:

> Assignment to an escaping name is refused **unless** the body is inside an
> `atomic` (serialised by the lock, and captured by reference — §2), **or** the
> assignment is the recognised reduction shape (private accumulator, no lock).

The same carve-out covers `ParallelIndexNotBinder`: inside an `atomic`, any
index into a shared array is allowed.

The recogniser is `reduction.tex:28-39` verbatim, checked over the loop body
before it is lowered:

| condition | how it is checked |
|---|---|
| every assignment to `l` is `l ⊕= e` | walk the body's assignments; any `l := e` disqualifies |
| exactly one operator across them | collect the set; size must be 1, or `{⊕, ⊖}` for a group |
| `l` is not otherwise read | `lookup_capturing` (`types/src/lib.rs:1337`) already records every crossing read — if `l` is in `captures`, it is read, and it is not a reduction |
| `l` is not free in a functional | our subset has no closures; a field of `self` disqualifies by name |

Condition 3 falls out of machinery M4 already built. That is the reason this
design fits: the scope stack already knows.

**WHEN it runs is load bearing, and getting it wrong is a silent race.**
`lookup_capturing` inserts into `ctx.captures` AS THE WALK PROCEEDS
(`types/src/lib.rs:1337-1351`, single call site at `:1460`), so `captures` is
only complete when the body walk finishes. Evaluate condition 2 at the assign
site and this passes:

```fortress
for i <- 0#N do
   atomic sum += a[i]     (* captures does not contain `sum` YET -> "reduction" *)
   println(sum)           (* ...and now it does *)
end
```

— a private accumulator AND a captured read of the same name. So the recogniser
runs **after** the body walk, against the final `ctx.captures`, in `for_expr`
where the loop context is popped (`types/src/lib.rs:1719`) and `captures` is built (`:1735-1739`), not inside `assign`.

The order of the three decisions is fixed and must be stated, because two of the
orderings are races:

1. walk the body; `assign` records escaping assignments and their operators, and
   raises `ParallelEscape` only when neither carve-out applies;
2. **then** run the recogniser against the completed `captures`;
3. **then** compute capture mode. A recognised reduction name is captured
   **not at all** — neither by value nor by reference — and its store is
   redirected to `partial[chunk]`. Only a name that stays on the LOCK path is
   captured by reference.

Do it in the other order and a reduction name lands in the environment as a
shared pointer with the lock erased, which is an unsynchronised load-add-store
from up to 16 threads — the `bump.fss` failure of §0.5a, in the headline case of
the whole milestone.

**THE TRAP: `+=` must NOT be desugared to `l := l + e` before the recogniser
runs.** `assign` reads its target with `lookup` (`types/src/lib.rs:1849`), which
records nothing, but it checks the VALUE with `self.expr`, and a `Var` there
goes through `lookup_capturing` (`:1460`). The moment `l += e` becomes
`l := l + e`, `l` lands in `ctx.captures`, condition 2 says it was read, and
**every reduction in the program disqualifies itself.** `+=` stays its own node,
`CompoundAssign { target, op, value }`, through the checker; only codegen splits it.

This is exactly where the reference implementation lost the feature:
`Operators.scala:521-529` desugars `x += e` to `x := x + e` in the typechecker,
`AssignmentAndSubscriptDesugarer.scala:82-115` did it earlier with temporaries,
and `CodeGen.java:1682-1688` throws on anything compound that survives. By the
time a recogniser could have run the shape was gone, which is why the legacy
implementation has reduction variables in no layer at all.

A failing recogniser is not an error. It falls through to the `atomic` rule: if
the body wrapped it in `atomic`, it takes the lock and is correct-but-slow; if
it did not, `ParallelEscape` fires exactly as today — and its diagnostic can now
name both fixes.

---

## 4. Scope boundary

**In:**

* `atomic <expr>` and `atomic do ... end`, anywhere an expression is legal
* `atomic` in a `for` header (`DoFront`), desugared in the parser
* `+=` and `-=`, as glued `Plus`/`Minus` + `Eq`
* reduction recognition for `+=` and `-=` on **`ZZ32` first**, then `ZZ64` and `RR64` — ZZ32 is 144 of the corpus's compound-assignment targets and all 7 unlockable files use it
* allocation, calls and `println` inside `atomic` — the GC measurement says so
* nested `atomic`, flattened by the recursive lock

**Out, each with a diagnostic that names the construct:**

| refused | why |
|---|---|
| `tryatomic` | throws `TryAtomicFailure`; we have no exceptions |
| `atomic`/`io` as a functional modifier | `atomic.tex:15` — 1.0 did not support it either |
| a reduction variable READ inside the loop | `reduction.tex:35`; it falls back to `atomic`, or is refused |
| two different operators assigning to one `l` | `reduction.tex:32-34` |
| `\|\|=` (37 uses), `UNIONCAT=`, `UPLUS=`, `TIMES=`, `MIN=`, `MAX=`, `CUP=` | each needs `Monoid[\T,⊙\]` and a user-declared identity; `\|\|=` is the biggest single thing M5 leaves on the table |
| a reduction on an object field or a receiver field | `reduction.tex:36-38` |
| `also do` parallel blocks | `reduction.tex:89+`'s second example; there is no `also` in the subset |
| `BIG` operators, comprehensions | separate milestone; `BIG` is still reserved-and-refused |
| `at`, `spawn` | unchanged from M4 |
| passing a loop-captured `Array` to a call inside a parallel body | §0.5a — this is not an M5 restriction, it is M4's missing check, and M5 cannot weaken the loop rules while it is open |

Every one of those is a syntactic check on the typed tree. No dataflow analysis
enters the compiler in M5, which is the same claim M4 made and the reason its
gate could prove anything.

And to be clear that the two paths are not one mechanism dressed twice:
`demos/HeapShakedown.fss:93` writes `dup = atomic do d = flags[v]; flags[v] := true; d end`
— a read-modify-write that RETURNS a value. There is no operator, no monoid and
no identity, so it is not a reduction at any scope, and it never will be. It is
the lock path, and it is why the lock path exists.

---

## 5. The gate

`tools/atomic-gate.sh`, built from `parallel-gate.sh`'s skeleton — the same
`--selftest` / `--mutate` split, the same `refused_cleanly` (only exit 1), the
same `file|from|to|label` mutation table split on `IFS='|'` (so no `|` in any
field), and the same link line, which gains nothing:
`cc probe.c runtime/shims.c -I "$CPATH" -o out -lgc -lm`.

What it asserts:

1. **The lost update.** 8 workers × 1,000,000 increments of one shared counter
   under `atomic` must total exactly 8,000,000, 20 runs out of 20. Engineered so
   the mutation cannot get lucky: unpadded counter, no work between updates.
2. **Reduction equals serial, exactly, for ZZ32 and ZZ64** — same input,
   `FORTRESS_WORKERS` 1, 2, 4, 8 and 14, all bit-identical to the serial fold.
   ZZ32 first: it is the type every unlockable corpus file actually uses.
3. **Reduction is deterministic for RR64 at a fixed worker count** — 20 runs
   byte-identical at `FORTRESS_WORKERS=8`, and the spread ACROSS worker counts is
   PRINTED, not asserted. Asserting a false thing is worse than asserting
   nothing.
4. **The reduction beats the lock**, on the same source, by more than 4x on 8
   workers. §2's tables predict ~70x; 4x is a floor that survives a noisy laptop.
5. **No deadlock and no hang**, every case under `timeout`, because all three of
   these were measured to hang before the fix: nested `atomic`; `atomic` at TOP
   LEVEL wrapping a parallel loop whose body also takes `atomic`; a halt
   (out-of-bounds subscript, failed `assert`) raised while the lock is held;
   plus `atomic` containing a `GC_malloc`-heavy body, `atomic` inside a `seq`
   loop, and `atomic` inside a loop below the 4096 threshold.
   Also the exit-70 case from §0.5b: `for i <- seq(...) do total := total + i end`
   must exit 0. **The gate treats any status but 0 or 1 as a hard failure and
   never as a refusal** — `refused_cleanly` already encodes that.
6. **The partition is unchanged** — `parallel-gate.sh`'s existing check is
   re-run after the ABI change, because a third argument to the body is exactly
   the kind of edit that silently breaks the split.
7. **No tax on programs that never say `atomic`.** Every fixture in
   `fortressc/tests/` is compiled with `--emit-ir` before and after the
   milestone and the IR must be byte-identical, except the loop fixtures, whose
   only permitted difference is the third argument to the body call. This is
   the assertion the legacy implementation could not have made, and it is the
   one that stops the mechanism leaking into the storage.

Mutations that must be refused — a gate is not trusted until it has refused, and
each of these is a REAL defect, not a rewrite:

| mutation | what it breaks |
|---|---|
| `fortress_atomic_enter` returns without locking | lost updates in (1) |
| the mutex is `PTHREAD_MUTEX_DEFAULT` instead of recursive | nested atomic hangs in (5); measured to hang |
| accumulator slots not padded | (4) still passes, so (4) needs the padding number printed — this mutation is why |
| the merge starts from `Identity` instead of `l`'s entry value | (2) fails whenever `sum` starts non-zero, so the fixture MUST start non-zero |
| the merge folds only workers `1..P` | (2) fails |
| the outlined body writes `slot[0]` instead of `slot[chunk]` | every worker races one slot; (2) fails on lost updates, and it is the mutation that proves the ABI argument is actually used |
| an atomic-assigned scalar is captured by VALUE instead of by reference | (1) fails with exactly `workers` counted instead of 8,000,000 — the lock is held and the update is still lost |
| revert the `seq` capture to `Slot::Value` | the `seq`-loop fixture returns to exit 70 |
| `fortress_atomic_enter` stops setting `fortress_in_parallel` | the top-level-atomic case times out; measured to hang 3 of 3 without it |
| `fortress_halt` goes back to `exit(1)` from `_exit(1)` | the halt-under-lock case times out; measured to hang 3 of 3 |
| the recogniser drops condition 2 (`l` is not otherwise read) | a body that reads `sum` mid-loop silently reads a partial |

The padding row is the honest one: it is a performance mutation, not a
correctness one, and **a mutation that is not a defect cannot be refused** —
so it is listed as a number the gate prints rather than a refusal it claims.

---

## Open questions for you

**1. Reduction variables are a deliberate step BEYOND 1.0.** `reduction.tex:15`,
`for.tex:15`, `also.tex:15` and `parallelism.tex:15` all say *"Reduction
variables are not yet supported."* M4 used that same note as the reason to leave
them out. Implementing them now is the M3c precedent — a named, signed-off
deviation — but it should be your signature, not mine.

**2. Does the recogniser fire without `atomic`?** The spec writes the reduction
both ways: `reduction.tex:78-88`'s `arraySum` has a bare `sum += a[i]`, and
`SpecData/examples/basic/Expr.Atomic.fss:22` — which is the atomic chapter's own
example — writes `atomic sum += a[i]` for the identical shape. My
recommendation is **both**: the recogniser matches on shape, and `atomic` is
neither required nor an obstacle. It is one rule instead of two, and it makes
the atomic chapter's example fast for free. The alternative — requiring
`atomic` as an explicit marker — is more conservative and would refuse the
spec's own `arraySum`.

**3. Split the milestone, and if so which half first?** The measurement argues
for the reverse of the obvious order. All 7 unlockable files write `+=` and go
down the **reduction** path; the lock path has zero customers among them. So
reductions-first delivers the entire corpus payoff, and `atomic`-first delivers
none of it — while `atomic`-first is what proves the GC interaction and fixes
the two hangs in §2. My recommendation is still to ship them together, because
§0.5's two defects and §2's two hangs have to be fixed either way and they
straddle both halves. But if you want a smaller first merge, take the
**reduction half plus §0.5b's capture fix** and leave the lock for M5b.
