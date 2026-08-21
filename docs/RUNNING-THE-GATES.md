# Running the gates

Fourteen shell scripts in `tools/` plus `peak-rss.py`. None runs automatically — there is no CI, no
git hook, no Makefile and no cargo alias — so every ratchet in this project fires
only when a person runs it. This document is what a person or a CI job has to do.

Written 2026-08-21 against master. If a count here disagrees with `tools/`,
`tools/` is right and this file is stale.

---

## 0. The two rules that are not optional

### RULE 1 — PIN THE COMPILER. `export FORTRESSC=<path>` BEFORE ANY SWEEP.

Several people or agents work this tree at once, and `cargo build` rewrites
`fortressc/target/debug/fortressc` in place. A sweep that takes minutes and reads
that path **silently mixes two compilers** and reports one number for both. There
is no error, no warning, and nothing in the output says it happened.

```bash
export FORTRESSC=$HOME/pins/fortressc-$(git rev-parse --short=9 HEAD)
cp fortressc/target/debug/fortressc "$FORTRESSC"
sha256sum "$FORTRESSC"          # record it; the reports print the first 12 chars
```

**Every script in `tools/` honours `FORTRESSC`** except `mpicc-in-image.sh`,
which is a `cc` wrapper and never invokes the compiler. That rollout is done.

**But `FORTRESSC` and `--mutate` do not mix, and the gates now refuse the
combination.** Every mutation rebuilds `fortressc/target/debug` with
`cargo build --workspace`; if `FORTRESSC` points at a pinned binary, the
mutation rebuilds one compiler and the gate reads a different one, so the
mutation has no effect, the assertion holds, and the table reports a **clean
escape**. The nine gates whose mutations rebuild carry
`mutate_needs_the_built_compiler` and exit 2 with an explanation. Unset
`FORTRESSC` before `--mutate`; pin it for everything else.

### RULE 2 — KEEP THE PIN OUTSIDE `fortressc/build/`.

**`fortressc/build/` is shared scratch and seven gates `rm -rf` it**:
`apply-gate.sh:105`, `atomic-gate.sh:93`, `generics-gate.sh:89,319`,
`memory-gate.sh:77`, `mpi-gate.sh:89`, `operator-gate.sh:105,303`,
`parallel-gate.sh:111,290`. Thirteen of the fourteen write into it -- `mpicc-in-image.sh` is a `cc`
wrapper and is the exception.

This is not hypothetical. A pin was put at `fortressc/build/pinned/` on
2026-08-21, another agent ran `parallel-gate.sh`, and the pin was gone — the
sweep after it failed with `no compiler at .../fortressc/build/pinned/...`. It
failed loudly, which was luck; had the gate merely *recreated* the directory the
run would have silently fallen back to `target/debug`.

Put pins in `~/pins/`, `/tmp`, or anywhere that is not this repository's build
directory. `fortressc/build/` is also gitignored, so nothing there is recoverable.

### Corollary — every report is stamped with two facts, and they differ

Because the pin is deliberate, **repo HEAD is not compiler identity**. The three
instruments print both:

```
== oracle gate at repo 0e6323e86, compiler 7e103205cb54 ==
```

Quote both when you quote a number. A number attributed only to a commit is not
reproducible when the compiler was pinned four commits back.

---

## 1. Before anything else

```bash
export LLVM_SYS_221_PREFIX=$HOME/.local/opt/llvm22-root/usr/lib64/llvm22
export CPATH=$HOME/.local/opt/gc-root/usr/include
export LIBRARY_PATH=$HOME/.local/opt/gc-root/usr/lib64
```

All three are needed before `cargo build` and before any link. `fortressc/setup-llvm.sh`
and `fortressc/setup-gc.sh` build those prefixes without root. Every link takes
`-lgc` **and** `-lm`. The linker driver is `cc`; `lld` is not installed.

Then:

```bash
cargo build --workspace
cargo test  --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
```

`cargo fmt` needs `--all`. Clippy forbids panicking indexing and `unsafe`.

---

## 2. The order to run them in

**Self tests first.** Every gate takes `--selftest`, which proves its assertions
can refuse and needs nothing built. If a self test fails, no result from that
gate means anything.

```bash
for g in tools/*-gate.sh tools/triage.sh tools/api-census.sh; do
    printf '%-24s ' "$(basename "$g")"; "$g" --selftest >/dev/null && echo ok || echo FAIL
done
```

**Then the measurement instruments** (all three honour `FORTRESSC`):

| Script | What it is | Ratchet |
|---|---|---|
| `triage.sh` | corpus sweep and the root-cause map. `--real` 1570, `--conformance` 1846, `--reuse` reads the cache | the compile count |
| `api-census.sh` | where every census-set file stops, per file | none — it is the measuring stick |
| `oracle-gate.sh` | the `.test` files as the oracle of record; links and runs every compiling corpus file | `PASS_FLOOR`, `oracle-accepted-must-fail.txt`, `oracle-known-signals.txt`, `oracle-known-divergences.txt` |
| `api-conformance.sh` | the component-satisfies-api **ladder** (L0 unpaired → L4 conformant); it probes for an api check mode so it arms itself when one lands | none yet — it is the Group 2 baseline |

**Then the feature gates**, which need a linking C compiler:

```
apply-gate      juxtaposition as application, chained comparison, COMPILE_FLOOR
arith-gate      integer division, and what it refuses
control-flow-gate  case, typecase, label/exit, and the atomic-rollback obligation
phase7-gate     the parallel reduction and ZZ64 past 2^31
tuple-gate      the tuple REFUSAL. Tuples are NOT implemented; this gate pins
                that they are refused cleanly at every position, and it is MEANT
                to go red the day they land
array-gate      arrays, bounds, the loop, and what the collector sees
atomic-gate     atomic blocks and parallel reductions
dispatch-gate   symmetric whole-program dispatch
generics-gate   monomorphization
memory-gate     the collector, and the leak it replaced
operator-gate   operators and builtins
parallel-gate   the parallel loop and the runtime
unit-gate       units
mpi-gate        the MPI link and four real ranks -- needs the Apptainer image
```

**`mpi-gate.sh` is the one that needs more than this machine**: build
`apptainer/fortress-mpi.def` first, and rebuild it with
`apptainer build --fakeroot --force` after any collector change.

---

## 3. Mutation tables

Ten gates take `--mutate`: `apply`, `arith`, `atomic`, `control-flow`,
`dispatch`, `generics`, `operator`, `parallel`, `phase7`, `oracle`. **A gate is not trusted until it has refused**,
so a green result is evidence only when the matching mutation table has run and
its numbers are stated.

```bash
git status --porcelain          # MUST be clean; see trap 1
tools/oracle-gate.sh --mutate
```

**Re-run every `--mutate` after a milestone AND after `cargo fmt`.** A mutation
pattern must match exactly once, and formatting moves lines.

---

## 4. The traps, all paid for

1. **A mutation script restores from HEAD, not from the index.** Restoring from
   the index faithfully restores a defect if anything stages mid-run, and the
   worktree and the index then agree with each other while both are wrong.
   Commit before mutating. **Every table enforces this**: each opens with a
   `git diff --quiet HEAD -- <paths>` guard and refuses on a dirty tree, and each
   restores with `git checkout HEAD -- <file>`.
2. **A mutation whose pattern does not match is a mutation that never happened,
   and it reports as a clean escape.** Every table guards against it, in two
   different ways because they mutate in two different ways. The literal
   `from`/`to` tables count occurrences of `from` and refuse unless there is
   **exactly one** (`apply-gate.sh:248`, and the same shape elsewhere).
   `oracle-gate.sh` mutates with `sed` expressions, where there is no `from`
   string to count, so its `apply()` md5s the file before and after and treats a
   no-op as a hard error. Two of oracle-gate's six escaped on their first run
   before that existed, and both were the author's bug rather than the gate's.
3. **A mutation aimed at a case the gate never reaches is not a mutation.** Pick
   a target that currently **passes**.
4. **A mutation table split on `IFS='|'` cannot carry a `|`.** Anything about
   bars, enclosers or `||` has to use a different separator, or functions.
   `control-flow-gate.sh` keeps the `IFS='|'` shape and every one of its five
   mutations is pipe-free by construction; check that before adding a sixth.
4b. **"Caught" means the assertion stops holding — NOT that the fixture now
   compiles.** Two of `control-flow-gate`'s guards are load bearing for codegen
   as well as for the diagnostic, so removing them yields **exit 70**, an
   internal error, rather than exit 0. The gate's `refused_cleanly` accepts only
   exit 1, so it goes red on 70 too. A mutate check written as `rc -eq 0`
   reported both as escapes; write it `rc -ne 1`.
5. **A gate that compiles `runtime/shims.c` has its own link line**, and every
   link takes `-lgc` **and** `-lm`.
6. **`nm ... | grep -q` under `set -o pipefail` reports failure when it
   succeeds.**
7. **A runtime feature-test macro is part of the source, not the build.**
   `memory-gate.sh` compiles `shims.c` under `-std=c11` and asserts zero warning
   output, so anything XSI has to ask for itself in the file.
8. **Agent worktrees under `.claude/` are full repo copies.** The corpus walk
   prunes `.claude`, `.git`, `target`, `fortressc` and root `examples` by path.
9. **`nohup ... &` inside a backgrounded wrapper is orphaned and killed.** Run
   the script *as* the background command. Same family as `pkill -f`
   self-matching.
10. **A build-system error reads as a passing gate** if a non-zero exit counts as
    a refusal. Make sure the target is reachable before trusting a refusal.

---

## 5. What a CI job would have to do, when there is one

Recorded because "add a YAML file" understates it by an order of magnitude:

- the image needs **LLVM 22** and a Boehm prefix, built by `setup-llvm.sh` and
  `setup-gc.sh` into `$HOME/.local` — Fedora splits both across a runtime and a
  `-devel` half and the `-devel` half needs root;
- the gates need a **linking C compiler**;
- `mpi-gate.sh` needs an **Apptainer image**, which is 160 MB and is gitignored;
- the job must **pin the compiler per rule 1** and stamp the sha256 into its
  output, or parallel jobs sharing a cache will report each other's numbers.

The three ratchets a CI job exists to enforce are `COMPILE_FLOOR` (290,
`apply-gate.sh:39`), the parser and lexer corpus floors (637 and 1780), and the
oracle gate's three. Today all of them fire only when someone types the command.
