# Group 2 phase 2: api check mode, conformance, resolution on, the `nat` hook

Branch `feature/group2-phase2`, four commits on master `285f67ac1` (the
orchestrator's four-lane merge). Worktree
`/home/prestonalthaus/claude/fortress-wt-lexparse`. Not merged, not pushed.

## Read this before the commit messages

**The first three commits were written BEFORE the orchestrator's merge existed
and their numbers are superseded.** When this work started, `master` was still
`f81f41ace`: nothing was merged, no merge was in progress, and origin agreed. The
merge landed mid-session (step 1 semantics, step 2 codegen, step 3 frontend), and
this branch was rebased onto it. Every number below is measured on the rebased
branch with a sha256-pinned driver; the in-commit figures are the pre-rebase
ones and are kept only because rewriting history to fix a number is worse than a
note saying which number to trust.

Two other stale premises from the task, for the record: the "strict 285 compile
floor" is `fix/semantics-correctness`'s own floor measured on its own base, not a
merged number; and D7 was already **drafted** (`8d6381111`), not in flight.

## Where it lands

|  | master 285f67ac1 | this branch |
|---|---|---|
| corpus exit 0 | 307 / 1956 | **366** / 1956 |
| corpus exit 0, `--real` | 249 / 1588 | **306** / 1588 |
| **api census, exit 0** | 5 (+13 refused at the terminus) | **17** / 126 |
| `cargo test` | 380 | **384** |

And the split that matters, because the corpus metric now means two things:

    master       exit0  307  =  302 .fss (emit an object)  +   5 .fsi (checked)
    this branch  exit0  366  =  302 .fss (emit an object)  +  64 .fsi (checked)

**The `.fss` count is identical.** This branch adds zero object-emitting files
and 59 checked apis. No component regressed.

## The four tasks

### 1. `SPIKE-API-CHECK-MODE` — done, census is **17**

`Checker::run` refused `is_api` as its first statement, before
`discharge_bounds` and before anything was resolved. It is checked now: headers
resolved, bounds discharged, no bodies, nothing emitted, exit 0.

It turned out small because most of the work already happened: `Checker::new`
builds the registry, which resolves every `extends`/`comprises`/`excludes` name
and computes a resolved `Signature` for every declaration and member. That is
why an api could always fail with `unknown type` before reaching the refusal.

**Attribution, measured both ways:**

* API-check-mode alone: master's 18 terminus-or-better become **18 exit-0**.
  ZERO lost — every api that could be reached passes the signatures check.
* `--resolve-imports` on by default costs exactly one: `Library/Reader.fsi`, on
  `unknown type Generator`.
* Net **17**.

**The pre-merge caveat was right, and here is its size.** On the unmerged base
this measured 25. After the merge — with `SPIKE-TEMPLATE-WELLFORMEDNESS` and the
semantics lane's fixes — it is 17. The 25 was inflated by eight.

What still blocks the other 51 census `.fsi`, which is the phase-3 map:
`UncheckedException` and `Generator` (apis that do not parse yet), **8 on
`nat`** (task 4), `String` is not a trait (the builtin-seeding decision, gap
analysis §2.7), `ImmutableArray`, and one `Self`.

### 2. Component-satisfies-api — done, behind `--check-exports`

`Component::exports` had no readers at all. `Compiled0.p.fss` exports
`Executable` and declares `ran():()`; one letter, and nothing had ever looked.

**The blast radius is two files, not 1526.** The specification's `Executable` is
`run(args:String...)` and 1526 corpus files export it, so a strict check looked
capable of turning the corpus off. **All three `Executable.fsi` files in this
tree declare `run(): ()`.** The spec predicted otherwise; the files decide.

    with --check-exports:  366 -> 364
      Compiled0.p.fss   exports `Executable`, declares `ran()`
      Compiled2.a.fss   satisfies one member of a two-member overload set

Measuring first also caught the one false positive: `Function.rats:18`'s `FnSig`
lets an api declare a function as a NAME OF ARROW TYPE (`foo: String -> ()`),
which is the same declaration as `foo(s: String): ()`. Fifteen corpus apis write
it and the first cut flagged every one.

**Left off by default deliberately.** The comparison is `TypeRef`-level — same
spelling, same shape — and whether `List.List` and `List` denote one type is a
resolution question that is the rest of phase 3. Two files is small enough to
default on; doing it before the resolver means re-basing the metric twice.
**Recommendation: turn it on when phase 3's resolver lands.**

### 3. `--resolve-imports` on by default — done

Accuracy over inflation, as instructed. `--no-resolve-imports` is new and is not
a courtesy: the census methodology needs the with-and-without comparison to
attribute a change to the resolver, and one driver test's negative half measures
nothing without it.

### 4. The `nat` scaffold — done, and it changes nothing

D7 §3.4 requires the decision before the parser change and D7 says **drafted,
not adopted**, so this is diagnostics and a named landing place. All six kinds
still refuse; corpus 366 → 366; 48 files' diagnostics now name the right owner.
The six are three groups — `nat`/`int`/`bool` (D7 §3.1, the arm that goes when
D7 is adopted), `unit`/`dim` (sub-phase 4d), `opr` (D7 §4, stays refused when
the others open) — because "M3d is type parameters only" was telling three
different people the same wrong thing.

## Three things that are not mine to fix

### `tools/api-census.sh` IS BROKEN ON MASTER, and silently

The semantics lane's line:col renderer prints the message FIRST and then a
source excerpt. `api-census.sh:175` takes `lines[-1]`, so it reports the caret
rule as the diagnostic. Run on master it prints "85 distinct diagnostics over
110 blocked census files", most of them `^^^^^^`.

`tools/triage.sh` already has the fix (`:148` prefers a parsed header and falls
back); `api-census.sh` did not get it. One file, group 0's lane.

My own harness had the identical bug and I hit it here: it is why the numbers in
this document were re-taken. **Anybody with a script that reads `lines[-1]` of
this compiler's stderr is now reading a caret rule.**

### `tools/dispatch-gate.sh` is 34/1 ON MASTER

It asserts `is ambiguous for (OL, OR)`. `OL` and `OR` are operator words under
the merged lexical rule — `OR` is the disjunction operator — so
`tests/ambiguous.fss` was renamed to `OLeft`/`ORight` and the gate was not. The
merge carried the rename and not the expectation. One string, still in `tools/`.
Every other gate is green: apply 21/0, operator 25/0, generics 24/0, unit 15/0,
array 16/0, atomic 28/0, parallel 26/0, memory 17/0.

### The corpus metric now counts two different things

`apply-gate.sh` counts exit 0 under `--emit-obj` and calls it "compile end to
end". An api exits 0 and emits nothing. The split is in the table above and the
`.fss` half is what "compiles" should mean.

## Instruments

`.spike/` in the worktree, untracked. `measure.py sweep|census|ir|diff|irdiff`,
`FCFLAGS` for driver flags, `env.sh` pins `FORTRESSC` to this worktree's own
binary. The census classifier keys on exit 0 now that the terminus message is
gone.
