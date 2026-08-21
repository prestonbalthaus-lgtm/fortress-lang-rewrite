# D9. The oracle, and the licence

Two paperwork decisions from the 2026-08-21 gap analysis, section 3.1. Both are
cheap, both block nothing technical, and both are currently blocking honesty.
Written against master `f81f41ace`, after the Group 0 instruments landed, so
every number below is one the gate produces rather than one this document
asserts.

Status: **drafted, not adopted.** The oracle half is a recommendation with the
evidence attached and can be taken by whoever owns the ROADMAP. The licence half
ends in a choice that is Preston's to make and nobody else's.

---

## Part 1 — The oracle

### The problem, in one paragraph

ROADMAP phase 0's exit is *"legacy interpreter runs `ProjectFortress/tests/` and
the pass/fail set is recorded in the repo"* (ROADMAP.md:121-124). **No such
recorded set exists** — `find . -iname '*baseline*'` returns nothing — and the
legacy interpreter **has never been built or run** in this project. README.md:290
says so outright: *"The legacy interpreter builds with Ant against Java 6 era
code. It has not been verified to still work."* The JVM path was cancelled as a
side effect of the no-JVM decision, in a design document, and the ROADMAP was
never amended. Phase 0 is therefore recorded as the foundation of a plan that
skipped it.

### Five live commitments name an oracle that has never run

| Where | What it says |
|---|---|
| `ROADMAP.md:8` | the corpus is *"run against the legacy interpreter for a differential baseline"* |
| `ROADMAP.md:123` | phase 0 exit: the interpreter runs the tests and the set is recorded |
| `ROADMAP.md:144-145` | phase 4 exit: *"disagreements with the legacy interpreter documented"* |
| `ROADMAP.md:149` | phase 5 exit: *"produces the same output as the interpreter"* |
| `ROADMAP.md:260-261` | *"The legacy interpreter stays only as a differential oracle and gets deleted once phase 7 passes."* |

**The last one is not hypothetical any more.** Phase 7's exit criterion was
measured and met on 2026-08-21 — 10^9-iteration reduction 0.80 s at one worker
to 0.09 s at fourteen, and a 3,000,000,000-element `Array[\Boolean\]` indexed at
2,999,999,999. So by the ROADMAP's own words the interpreter is now due for
deletion, having never once served as the oracle that was its only remaining
job. And the deletion is not a small tidy: the interpreter is
`ProjectFortress/src/`, and `ProjectFortress/` is also where all 373 `.test`
files, the entire compiler test corpus, and `LibraryBuiltin/` live. Executing
that line as written would delete the measuring stick.

### What actually exists, and it was on disk the whole time

**373 `.test` files** record the legacy implementation's behaviour, in
`java.util.Properties` format, read by `StringMap.FromFileProps`
(`FileTests.java:919`). They carry:

- **264 `compile_err_equals` values** — the *exact* compile error the legacy
  implementation produced, byte for byte, for 264 corpus programs. A program
  with one of these is a program the legacy implementation **refused**.
- **227 `run` cases with an expected output**, in six comparator flavours
  transcribed from `FileTests.java:141-272`, plus the default rule that a `run`
  with no explicit check must print `pass` or `PASS`.

That is a pass/fail set, it is recorded in the repo, and it needs no JVM. It is
what phase 0 asked for. `tools/oracle-gate.sh` reads it.

### What the gate produces, at `f81f41ace`

```
  cases  bucket
    285  pass        a verdict was reached and it AGREED with the oracle
     51  fail        a verdict was reached and it DISAGREED
    267  blocked     no verdict: a feature is missing. NOT a wrong answer
      6  unmodelled  a directive this driver does not expose
    609  total, over 373 .test files
```

Plus two things nothing in this repository had ever looked at:

- **47 programs the legacy implementation refused, this compiler accepts.**
  Listed by path in `tools/oracle-accepted-must-fail.txt`.
- **291 corpus binaries built and executed. 288 exit 0 or 1; three die on
  SIGSEGV.** No corpus program had ever been *run* before — the compile metric
  only ever checked that the driver exited 0. Four more (`GenMet0`-`GenMet3`)
  run to completion at exit 0 and print the wrong answer.

### The recommendation

**Adopt the `.test` files as the oracle of record, and amend the five lines.**

The alternative — build the Java 6 interpreter, get it running under a modern
JDK, and run it over the corpus — buys a *live* oracle that can answer questions
the recorded one cannot (what does the legacy do on a program with no `.test`
file?). It costs an Ant/Java-6 revival on a tree upstream abandoned, and it
reintroduces a JVM into a project whose forbidden list names the JVM first. The
recorded oracle covers 609 of the cases anyone has ever asked about and grows
with the compiler instead of being a one-off count. That trade is not close.

Concretely:

1. **ROADMAP.md:121-124, phase 0.** Replace the exit with:
   *"the pass/fail set the legacy implementation recorded in the 373 `.test`
   files is read by a gate in `tools/`, and its three numbers are recorded in
   the repository at a named commit."* Mark it **met** at `f81f41ace` with the
   four numbers above. Add one sentence saying the legacy interpreter was never
   built and that this is deliberate, not outstanding.

2. **ROADMAP.md:144-145, phase 4.** Replace *"disagreements with the legacy
   interpreter documented rather than silently matched"* with *"disagreements
   with the legacy implementation's recorded behaviour documented rather than
   silently matched"*, and name `tools/oracle-accepted-must-fail.txt` as that
   document. It already holds 47 of them.

3. **ROADMAP.md:149, phase 5.** Replace *"produces the same output as the
   interpreter"* with *"produces the output recorded in the `.test` files"*.
   Note that the compile half is met and the output half is now measured for the
   first time: 285 agreements and 4 disagreements.

4. **ROADMAP.md:8.** Replace *"run against the legacy interpreter for a
   differential baseline"* with a pointer to the recorded set.

5. **ROADMAP.md:260-261.** This is the one that needs a real edit and not a
   substitution. Replace the deletion trigger with:
   *"The legacy tree stays. `ProjectFortress/src/` is the reference
   implementation and is read, not run; `ProjectFortress/` also holds the corpus,
   the 373 `.test` files and `LibraryBuiltin/`, all of which are load-bearing.
   Nothing here is deleted at any phase."*
   Phase 7 has passed, so leaving this line as written leaves a standing
   instruction to delete the measuring stick.

**One thing the amendment must not claim.** The recorded oracle is a *sample*,
not the legacy implementation. 609 cases over 373 files is a fraction of the 1956
corpus files, and a program with no `.test` file has no recorded answer. Phase 4
and phase 5 exits should say "recorded behaviour" and never "the legacy
implementation", because those are different sets and the difference will matter
the first time someone wants an answer the corpus does not carry.

### One divergence to record while amending

Before treating any of the 47 as a bug, read its expected message against
decisions 1 and 4. **A legacy static error for a feature v1 scopes differently
is a documented divergence, not a defect** — and phase 4's exit criterion asks
for exactly that document. The shapes are already known from the gap analysis
(§5.1): `Invalid comprises clause` 7, `X is undefined` 5, `Unmatched delimiter
"end"` 3, and so on. The list is the place to write the verdict per file; a
line that turns out to be a divergence stays in the file with a comment, because
removing it would make the gate red the next time somebody looks.

---

## Part 2 — The licence

### The contradiction, three ways

| Source | Says |
|---|---|
| `fortressc/Cargo.toml:16` | `license = "Apache-2.0"`, inherited by all six crates via `license.workspace = true` |
| `README.md:295-296` | *"New code is unlicensed so far, pick something before the first release."* |
| `LICENSE` (root, 406 lines) | the legacy compound file: **BSD 3-clause, Sun Microsystems 2007** for the reference interpreter, plus separate sections for bundled Ant/BCEL (Apache-2.0), the Unicode character data, Doug Lea's public-domain code, and DSTM2 (BSD, Sun 2006) |

Three points worth being precise about, because two of them are easy to get
backwards:

- **The Apache-2.0 text in `LICENSE` is not about Fortress.** It sits under the
  heading *"Ant and its libraries, and BCEL are released under this license"*.
  The Cargo declaration matching a string that appears in `LICENSE` is a
  coincidence, not a basis.
- **The root licence for the legacy tree is BSD 3-clause, not Apache-2.0.**
  `LICENSE:1-2` — *"Unless otherwise noted below, the Fortress reference
  interpreter is released under this BSD license"*.
- **The repository is public and is a fork.** `gh` records it as `isFork=true`
  with parent `stokito/fortress-lang`; origin is
  `prestonbalthaus-lgtm/fortress-lang-rewrite`. The published crate metadata
  already asserts Apache-2.0 to anyone who reads it, and has since the workspace
  was created.

### What is and is not at stake

The new compiler under `fortressc/` is original work. It is a **rewrite**, not a
port of the Java: it shares no code with `ProjectFortress/src/`. What it does
share, and what the whole repository ships, is the legacy tree — the corpus, the
specification sources, the `.rats` grammars, `LibraryBuiltin/`, the 373 `.test`
files. Those are Sun/Oracle BSD 3-clause and stay that way whatever is chosen;
BSD 3-clause permits redistribution with the notice retained, which the root
`LICENSE` does.

So the question is narrow: **what licence covers the new code under
`fortressc/`, and is the Cargo declaration currently telling the truth?**
Today it is not, because README says the opposite in the same repository.

### The three real options

1. **Apache-2.0.** Ratify what `Cargo.toml` already declares. Add
   `fortressc/LICENSE` with the Apache-2.0 text, change `README.md`'s License
   section to say which licence covers which tree, and stop there. Compatible
   with the BSD 3-clause legacy tree in the same repository. Carries an express
   patent grant, which is the usual reason to prefer it over MIT for a compiler.
   **No file has to change its licence, and nothing already published becomes
   retroactively wrong.**
2. **MIT, or BSD 3-clause to match the legacy tree.** Simpler, no patent clause.
   Requires changing `Cargo.toml` and every `license.workspace` inheritor, and
   means the crate metadata was wrong for the whole history rather than merely
   unratified.
3. **Dual MIT OR Apache-2.0.** The Rust ecosystem's default and what most
   downstream Rust consumers expect. Costs one extra file and a
   `license = "MIT OR Apache-2.0"` line.

### The recommendation, and it is not the decision

**Option 1, Apache-2.0.** It is the only one of the three under which nothing
already asserted becomes retroactively false, the patent grant is the right
default for a compiler, and it composes with the BSD 3-clause legacy tree
without any per-file work. Option 3 is the better answer if `fortressc` is ever
meant to be published to crates.io, and that question has not been asked.

**This is Preston's call and nobody else's.** A licence choice on a public
repository is the owner's, it is not reversible for anything already
distributed, and no agent should make it. What is drafted here is the argument;
the pick is a signature.

### What to do the moment it is picked, whichever way it goes

1. Add the licence text at `fortressc/LICENSE` (do **not** overwrite the root
   `LICENSE` — it is the legacy tree's notice and BSD 3-clause requires it to be
   retained).
2. Set `fortressc/Cargo.toml:16` to the chosen SPDX identifier and add
   `license-file` if the identifier is not a standard one.
3. Rewrite `README.md:293-296` to name both trees explicitly:
   which licence covers `fortressc/` and that everything else is under the
   Sun/Oracle terms in the root `LICENSE`.
4. Add a short `NOTICE` naming Sun Microsystems and Oracle as the origin of the
   legacy tree, and stokito/fortress-lang as the fork parent.

Steps 1-3 are the release blocker. Step 4 is courtesy.
