# M3f Application and Chaining Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `println "Hello"` becomes a function call, and `a < b < c` becomes a chain that evaluates `b` once.

**Architecture:** Two independent rules from Specification 1.0, landed in two different phases. Juxtaposition-as-application is a **checker** rule, because it asks whether a name is a local and only the checker has scopes; it delegates to the existing `Checker::call`, which buys MPI builtins, `println`, object construction and the whole overload machinery unchanged. Chained comparison is a **parser** rewrite to an ordinary block of temporaries plus a nested `if`, so the checker never learns that chaining exists. Neither adds a type, a runtime shim, an AST variant, or a change to `Type`.

**Tech Stack:** Rust 2021, `logos` lexer, hand-written recursive-descent parser, `inkwell` / LLVM 22, Boehm GC. Gates are bash under `tools/`.

## Global Constraints

- Design document: `docs/superpowers/specs/2026-08-19-m3f-application-chaining-design.md`. Every scope decision is settled there; this plan implements it and does not re-open it.
- Build environment, all three exports needed before any `cargo build` or link:
  - `export LLVM_SYS_221_PREFIX=$HOME/.local/opt/llvm22-root/usr/lib64/llvm22`
  - `export CPATH=$HOME/.local/opt/gc-root/usr/include`
  - `export LIBRARY_PATH=$HOME/.local/opt/gc-root/usr/lib64`
- Linker driver is `cc`. `lld` is not installed.
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` must pass at every commit. The workspace denies `unwrap_used`, `expect_used`, `panic` and `indexing_slicing` outside tests: use `.first()`, `.get()`, `split_first()`, never `xs[0]` or `xs[1..]`.
- No compiler pass may `panic!` or `unwrap()` on user source. Malformed input is a `Result` diagnostic, exit 1, never exit 70.
- Fixtures live in `fortressc/tests/`. `end_to_end.rs`'s `fixture()` resolves `CARGO_MANIFEST_DIR/../../tests`, which from `crates/driver` is `fortressc/tests`. Not the repository root.
- `Type` stays `Copy`. Nothing in this milestone touches it.
- Branch is `m3f/application-chaining`, already created, already carrying the design commit `4a759fc0e`.

## Three deviations from the approved design, each with the measurement behind it

These were found while reading the code to write this plan. All three are defect-prevention inside the approved change's blast radius, not new scope.

1. **Singleton objects are excluded from the function-element set.** The design says "present in `Checker::functions` or an `MpiOp`, or it is `println`". Object constructors belong there too — `Foo x` is construction — but `crates/types/src/lib.rs:636` shows a *singleton* object name is a **value**, the only type name that is. So the test is `!info.singleton`, not `is_object`. Without this, `Marker 2` on a singleton would become `Marker(2)`.

2. **`f ()` is the zero-argument call.** In Fortress a nullary function's argument is `()`. Parsed, `f ()` is `Juxt[Var(f), Unit]`, and a naive delegation calls `f` with one argument of type `Void` and reports an arity mismatch. One arm fixes it. Its file delta is measured in Task 2 and reported.

3. **A local function declaration in a block is refused with its own diagnostic.** Adding `=` to the comparison operators turns `f(x) = 3` in block position from a parse *error* into a silently accepted equality expression. Measured: **236 corpus files carry 572 indented `name(...) = ` lines**. The guard is a token-level speculative parse, not a match on the parsed tree, because a local function whose body is itself an equality (`isZero(x) = x = 0`) collects into a three-operand chain and desugars into a `Block` before any tree match could see it.

---

### Task 1: Baselines, measured not inherited

No production code. Every number this milestone reports is a delta against a baseline taken today, on this tree, by this engineer.

**Files:**
- Create: `/tmp/claude-1000/-home-prestonalthaus-claude/92371b9f-ac5f-46f7-a11e-41d63047ecdc/scratchpad/m3f-baseline-parse.txt`
- Create: `/tmp/claude-1000/-home-prestonalthaus-claude/92371b9f-ac5f-46f7-a11e-41d63047ecdc/scratchpad/m3f-baseline-compile.txt`

**Interfaces:**
- Produces: two sorted file-path lists, one per metric, for the set-diffs in Tasks 2 and 3. Nothing in the repository depends on them.

- [ ] **Step 1: Export the build environment and build the workspace clean**

```bash
cd /home/prestonalthaus/claude/fortress-lang-rewrite/fortressc
export LLVM_SYS_221_PREFIX=$HOME/.local/opt/llvm22-root/usr/lib64/llvm22
export CPATH=$HOME/.local/opt/gc-root/usr/include
export LIBRARY_PATH=$HOME/.local/opt/gc-root/usr/lib64
cargo build --workspace
```

Expected: builds with no warnings.

- [ ] **Step 2: Record the parser corpus baseline**

```bash
cd /home/prestonalthaus/claude/fortress-lang-rewrite/fortressc
cargo test -p fortress-parser --test corpus -- --nocapture 2>&1 | grep -E 'parsed|corpus:'
```

Expected: `parsed 428 (24.0% of those that lex)`. If it is not 428, stop: the ratchet in `crates/parser/tests/corpus.rs` and `04-state.md` disagree with the tree, and that has to be understood before anything is built on top of it.

- [ ] **Step 3: Record the set of files that parse**

```bash
cd /home/prestonalthaus/claude/fortress-lang-rewrite
SCRATCH=/tmp/claude-1000/-home-prestonalthaus-claude/92371b9f-ac5f-46f7-a11e-41d63047ecdc/scratchpad
python3 - > "$SCRATCH/m3f-baseline-parse.txt" <<'PY'
import os, subprocess
files = []
for d, ds, fs in os.walk('.'):
    ds[:] = [x for x in ds if x not in ('.git', 'target', 'fortressc')]
    files += [os.path.join(d, f) for f in fs if f.endswith(('.fss', '.fsi'))]
files.sort()
for p in files:
    r = subprocess.run(['fortressc/target/debug/fortressc', p, '--emit-obj', '-o', '/dev/null'],
                       capture_output=True, text=True)
    # A parse failure is the only diagnostic whose text has no `..` span prefix
    # from the types crate and comes from ParseError's Display. Cheaper and
    # exact: re-run the parser directly is not exposed, so classify on stderr.
    err = r.stderr
    parsed = ('expected ' not in err and 'unexpected end of input' not in err
              and 'reserved word' not in err and 'static parameters' not in err
              and 'postfix operator' not in err)
    if parsed:
        print(p)
PY
wc -l "$SCRATCH/m3f-baseline-parse.txt"
```

Expected: a count near 428. This heuristic classifies on stderr text; it is used only for the *set diff*, never for the headline number, which comes from the corpus test in Step 2.

- [ ] **Step 4: Record the full driver sweep baseline**

```bash
cd /home/prestonalthaus/claude/fortress-lang-rewrite
export LLVM_SYS_221_PREFIX=$HOME/.local/opt/llvm22-root/usr/lib64/llvm22
export CPATH=$HOME/.local/opt/gc-root/usr/include
export LIBRARY_PATH=$HOME/.local/opt/gc-root/usr/lib64
SCRATCH=/tmp/claude-1000/-home-prestonalthaus-claude/92371b9f-ac5f-46f7-a11e-41d63047ecdc/scratchpad
python3 - <<'PY' | tee "$SCRATCH/m3f-baseline-compile.txt"
import os, subprocess, collections, sys
files = []
for d, ds, fs in os.walk('.'):
    ds[:] = [x for x in ds if x not in ('.git', 'target', 'fortressc')]
    files += [os.path.join(d, f) for f in fs if f.endswith(('.fss', '.fsi'))]
files.sort()
c = collections.Counter()
ok = []
for p in files:
    r = subprocess.run(['fortressc/target/debug/fortressc', p, '--emit-obj', '-o', '/dev/null'],
                       capture_output=True, text=True)
    c[r.returncode] += 1
    if r.returncode == 0:
        ok.append(p)
    elif r.returncode == 70:
        print('EXIT70', p, r.stderr.strip()[:200], file=sys.stderr)
print('\n'.join(ok))
print('# exit profile', dict(sorted(c.items())), file=sys.stderr)
PY
```

Expected on stderr: `# exit profile {0: 151, 1: 1805}`, and no `EXIT70` lines. Measured on this tree on 2026-08-19; the whole sweep takes about 9 seconds.

- [ ] **Step 5: Record the three numbers in the working notes**

No commit. Carry forward: **parse 428, compile 151, exit-70 zero.** Every later measurement is stated against these.

---

### Task 2: Juxtaposition as function application

**Files:**
- Modify: `fortressc/crates/types/src/error.rs` — add `JuxtapositionNotBinary`, its `span()` arm, its `Display` arm
- Modify: `fortressc/crates/types/src/lib.rs` — add `Checker::is_function_element`, add the application check at the top of `Checker::juxtaposition`
- Create: `fortressc/tests/juxtapply.fss`
- Create: `fortressc/tests/juxtshadow.fss`
- Create: `fortressc/tests/juxtnary.fss`
- Create: `fortressc/tests/juxtsingleton.fss`
- Create: `fortressc/tests/juxtnullary.fss`
- Test: `fortressc/crates/driver/tests/end_to_end.rs`

**Interfaces:**
- Consumes: `Checker::call(&mut self, callee: &Expr, args: &[Expr], span: Span, expected: Option<Type>) -> Checked<TypedExpr>` (`crates/types/src/lib.rs:1187`); `Checker::lookup(&self, name: &str) -> Option<Local>` (`:544`); `self.functions: HashMap<String, Vec<Signature>>`; `self.registry.objects: <name -> ObjectInfo { singleton: bool, .. }>`; `MpiOp::from_name(name) -> Option<MpiOp>`.
- Produces: `TypeError::JuxtapositionNotBinary { span: Span, found: usize }`, used by Task 5's gate.

- [ ] **Step 1: Write the failing end-to-end test**

Append to `fortressc/crates/driver/tests/end_to_end.rs`:

```rust
/// M3f: an identifier with no local binding, juxtaposed with one operand, is a
/// function application. This is the whole reason `println "Hello"` is 48 files
/// of the missing-name histogram.
#[test]
fn juxtaposition_of_a_function_is_application() {
    let binary = compile_fixture("juxtapply.fss", "juxtapply");
    let out = run(&binary);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "Hello\n42\n");
    assert_eq!(out.status.code(), Some(0));
    let _ = std::fs::remove_file(&binary);
}

/// The guard. A parameter that shadows a function name is a value, so `f y` is
/// multiplication and not a call. Dropping the `lookup` test silently changes
/// what this program computes, which is why it is a test and not a comment.
#[test]
fn a_shadowed_function_name_is_not_application() {
    let binary = compile_fixture("juxtshadow.fss", "juxtshadow");
    let out = run(&binary);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "12\n");
    let _ = std::fs::remove_file(&binary);
}
```

- [ ] **Step 2: Write the two fixtures the test needs**

`fortressc/tests/juxtapply.fss`:

```
component juxtapply
export Executable

double(x: ZZ64): ZZ64 = x 2

run(): () = do
    println "Hello"
    println(double 21)
  end

end
```

`fortressc/tests/juxtshadow.fss`:

```
component juxtshadow
export Executable

f(x: ZZ64): ZZ64 = x + 1

apply(f: ZZ64, y: ZZ64): ZZ64 = f y

run(): () = println(apply(3, 4))

end
```

`double`'s own body is `x 2`, where `x` is a parameter, so it stays multiplication — the same rule, exercised from the inside. `apply` takes a parameter literally named `f` while a function `f` exists; `f y` must be 3 times 4.

- [ ] **Step 3: Run the tests and watch them fail**

```bash
cd /home/prestonalthaus/claude/fortress-lang-rewrite/fortressc
cargo test -p fortress-driver --test end_to_end juxta 2>&1 | tail -30
```

Expected: `juxtaposition_of_a_function_is_application` fails, because `fortressc` exits 1 with `unknown name \`println\``. `a_shadowed_function_name_is_not_application` PASSES already — it is the regression guard, and it is supposed to be green before and after.

- [ ] **Step 4: Add the diagnostic**

In `fortressc/crates/types/src/error.rs`, add to the `TypeError` enum, immediately after the `UnresolvableJuxtaposition` variant:

```rust
    /// A juxtaposition led by a function element with more than two elements.
    /// The specification's reassociation rules (`juxtameaning.tex:70-111`) are
    /// not implemented, and were measured at zero corpus files, so this refuses
    /// rather than guesses.
    JuxtapositionNotBinary {
        span: Span,
        found: usize,
    },
```

Add to the `span()` match, in the same alternation chain, after `| Self::UnresolvableJuxtaposition { span, .. }`:

```rust
            | Self::JuxtapositionNotBinary { span, .. }
```

Add to the `Display` match, after the `UnresolvableJuxtaposition` arm:

```rust
            Self::JuxtapositionNotBinary { found, .. } => write!(
                f,
                "a juxtaposition of {found} elements led by a function is not implemented; \
                 parenthesise the application"
            ),
```

- [ ] **Step 5: Add the function-element test**

In `fortressc/crates/types/src/lib.rs`, in `impl Checker`, immediately above `fn juxtaposition`:

```rust
    /// Specification rule (c), `juxtameaning.tex:44-46`: an identifier with no
    /// visible declaration is a function element. `lookup` is what "visible
    /// declaration" means here, and it is the whole guard -- a local or a
    /// parameter that shares a name with a function is a value, so `f y` stays
    /// multiplication. A singleton object is a value too (`Self::variable`), so
    /// only a constructible object counts.
    fn is_function_element(&self, name: &str) -> bool {
        if self.lookup(name).is_some() {
            return false;
        }
        MpiOp::from_name(name).is_some()
            || matches!(name, "widen" | "println" | "array" | "length")
            || self
                .registry
                .objects
                .get(name)
                .is_some_and(|info| !info.singleton)
            || self.functions.contains_key(name)
    }
```

- [ ] **Step 6: Apply it at the top of the fold**

In `fortressc/crates/types/src/lib.rs`, at the very start of `fn juxtaposition`, before the comment `// Literals cannot supply a type`:

```rust
        // Application first, and only on a leading function element: every
        // juxtaposition that resolved before this milestone still takes the
        // same path. The probe loop below is what reports `unknown name
        // println`, so this has to run ahead of it.
        if let Some((callee, args)) = items.split_first() {
            if let Expr::Var { name, .. } = callee {
                if self.is_function_element(name) {
                    // `f ()` is the nullary call: in Fortress a zero-argument
                    // function's argument is the unit value.
                    if let [Expr::Unit { .. }] = args {
                        return self.call(callee, &[], span, expected);
                    }
                    if args.len() != 1 {
                        return Err(TypeError::JuxtapositionNotBinary {
                            span,
                            found: items.len(),
                        });
                    }
                    return self.call(callee, args, span, expected);
                }
            }
        }
```

- [ ] **Step 7: Run the tests and watch them pass**

```bash
cd /home/prestonalthaus/claude/fortress-lang-rewrite/fortressc
cargo test -p fortress-driver --test end_to_end juxta 2>&1 | tail -20
```

Expected: both PASS.

- [ ] **Step 8: Run the whole suite, clippy and fmt**

```bash
cd /home/prestonalthaus/claude/fortress-lang-rewrite/fortressc
cargo test --workspace 2>&1 | tail -25
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

Expected: all green. The suite was 217 tests before this milestone; it is now 219.

- [ ] **Step 9: Write the three refusal fixtures**

`fortressc/tests/juxtnary.fss`:

```
component juxtnary
export Executable

g(x: ZZ64): ZZ64 = x

run(): () = println(g 1 2)

end
```

`fortressc/tests/juxtsingleton.fss`:

```
component juxtsingleton
export Executable

object Marker end

run(): () = println(Marker 2)

end
```

`fortressc/tests/juxtnullary.fss`:

```
component juxtnullary
export Executable

answer(): ZZ64 = 42

run(): () = println(answer ())

end
```

- [ ] **Step 10: Check each refusal fixture by hand and record its exact message**

```bash
cd /home/prestonalthaus/claude/fortress-lang-rewrite
export LLVM_SYS_221_PREFIX=$HOME/.local/opt/llvm22-root/usr/lib64/llvm22
export CPATH=$HOME/.local/opt/gc-root/usr/include
export LIBRARY_PATH=$HOME/.local/opt/gc-root/usr/lib64
for f in juxtnary juxtsingleton juxtnullary; do
  fortressc/target/debug/fortressc "fortressc/tests/$f.fss" --emit-obj -o /dev/null 2>&1 >/dev/null | tail -1
  echo "  ^ $f exit ${PIPESTATUS[0]}"
done
```

Expected:
- `juxtnary` exit 1, message contains `a juxtaposition of 3 elements led by a function is not implemented`.
- `juxtsingleton` exit 1, message contains `neither multiplication nor concatenation`. This is the singleton exclusion: if `is_function_element` used `is_object` instead of `!info.singleton`, the message would be `is a singleton and has no constructor` instead. **The message is the assertion, not the exit code** — both are exit 1.
- `juxtnullary` exit 0, and running it prints `42`. If it exits 1 with an arity mismatch, deviation 2 is not wired.

- [ ] **Step 11: Measure the compile delta and the exit-70 count**

```bash
cd /home/prestonalthaus/claude/fortress-lang-rewrite
SCRATCH=/tmp/claude-1000/-home-prestonalthaus-claude/92371b9f-ac5f-46f7-a11e-41d63047ecdc/scratchpad
# same script as Task 1 Step 4, writing to $SCRATCH/m3f-after-juxt-compile.txt
```

Re-run Task 1 Step 4's script verbatim, redirecting to `$SCRATCH/m3f-after-juxt-compile.txt`.

Expected: exit profile `{0: 181, 1: 1775}`, **zero** `EXIT70` lines. 181 is the number the design predicts from the scouting spike. If the count differs, report the measured number: the measurement wins, and a difference is worth understanding before moving on. If any `EXIT70` line appears, stop and fix it — that is the M3e latent-crash class and it is exactly what this sweep exists to catch.

- [ ] **Step 12: Measure the nullary rule on its own**

```bash
cd /home/prestonalthaus/claude/fortress-lang-rewrite/fortressc
# temporarily comment out the `if let [Expr::Unit { .. }] = args` arm from Step 6
cargo build --workspace 2>&1 | tail -3
# re-run the sweep, note the count, then restore the arm and rebuild
```

Report the delta the nullary arm is worth as its own number. It is a deviation from the approved design and it does not get to hide inside an aggregate.

- [ ] **Step 13: Commit**

```bash
cd /home/prestonalthaus/claude/fortress-lang-rewrite
git add fortressc/crates/types/src/error.rs fortressc/crates/types/src/lib.rs \
        fortressc/crates/driver/tests/end_to_end.rs \
        fortressc/tests/juxtapply.fss fortressc/tests/juxtshadow.fss \
        fortressc/tests/juxtnary.fss fortressc/tests/juxtsingleton.fss \
        fortressc/tests/juxtnullary.fss
git commit -m "feat(types): juxtaposition of a function element is application

Specification rule (c), juxtameaning.tex:44-46. Binary only: the n-ary
reassociation was spiked and measured at zero corpus files, so it is a
diagnostic instead. A singleton object name is a value, not a constructor, so
only a constructible object is a function element. \`f ()\` is the nullary call."
```

---

### Task 3: `=` is an equality operator, and a local function declaration is refused

Adding `Kind::Eq` to the comparison operators is what makes chaining reachable at all, and it is also the one change in this milestone that can silently change what a program means. Both halves land together.

**Files:**
- Modify: `fortressc/crates/parser/src/lib.rs` — `comparison_op`, `block_item`
- Modify: `fortressc/crates/parser/src/error.rs` — add `LocalFunctionDeclarationUnsupported`, its `span()` arm, its `Display` arm
- Modify: `fortressc/crates/parser/tests/corpus.rs` — the exhaustive blocker match gains an arm
- Create: `fortressc/tests/localfn.fss`
- Test: `fortressc/crates/parser/tests/parser.rs`

**Interfaces:**
- Consumes: `Parser::postfix(&mut self) -> Parsed<Expr>`; `Parser::glued_left(&self, index: usize) -> bool`; `Parser::peek_ahead(&self, n: usize) -> Option<&Kind>`; `Parser::at(&self, kind: &Kind) -> bool`.
- Produces: `ParseError::LocalFunctionDeclarationUnsupported { span: Span }`; `Kind::Eq` mapping to `BinOp::Eq`, which Task 4 chains over.

- [ ] **Step 1: Write the failing parser tests**

Append to `fortressc/crates/parser/tests/parser.rs`:

```rust
#[test]
fn a_bare_equals_in_expression_position_is_equality() {
    let source = "component c\nrun(): () = if 1 = 1 then println(\"y\") else println(\"n\") end\nend\n";
    let tokens = fortress_lexer::lex(source).unwrap();
    assert!(fortress_parser::parse(&tokens).is_ok());
}

/// `f(x) = e` in block position is a local function declaration, which this
/// subset does not implement. Without this it would parse as a discarded
/// equality: 236 corpus files carry 572 such lines.
#[test]
fn a_local_function_declaration_is_refused() {
    let source = "component c\nrun(): () = do\n  isZero(x) = x = 0\n  println(\"unreachable\")\nend\nend\n";
    let tokens = fortress_lexer::lex(source).unwrap();
    let err = fortress_parser::parse(&tokens).unwrap_err();
    assert!(
        matches!(err, fortress_parser::ParseError::LocalFunctionDeclarationUnsupported { .. }),
        "expected a local function declaration diagnostic, got {err:?}"
    );
}

/// Pinned deliberately: `try_binding` takes the first `=`, so this is a binding
/// whose value is a comparison. It was a parse error before `=` became an
/// operator, and the new reading is the one the design argues for.
#[test]
fn a_binding_of_a_comparison_binds_the_comparison() {
    let source = "component c\nrun(): () = do\n  b = 3 = 4\n  println(\"done\")\nend\nend\n";
    let tokens = fortress_lexer::lex(source).unwrap();
    assert!(fortress_parser::parse(&tokens).is_ok());
}
```

- [ ] **Step 2: Run them and watch them fail**

```bash
cd /home/prestonalthaus/claude/fortress-lang-rewrite/fortressc
cargo test -p fortress-parser --test parser 2>&1 | tail -25
```

Expected: all three fail. The first two on the missing variant (a compile error is a failure here), the third on a parse error at `=`.

- [ ] **Step 3: Add the parse error variant**

In `fortressc/crates/parser/src/error.rs`, add to the enum after `StaticParameterKindUnsupported`:

```rust
    /// `f(x) = e` in block position. `=` is an equality operator in expression
    /// position, so without this the declaration would parse as a discarded
    /// comparison rather than fail.
    LocalFunctionDeclarationUnsupported {
        span: Span,
    },
```

Add to the `span()` alternation, after `| Self::StaticParameterKindUnsupported { span, .. }`:

```rust
            | Self::LocalFunctionDeclarationUnsupported { span }
```

Add to `Display`, after the `StaticParameterKindUnsupported` arm:

```rust
            Self::LocalFunctionDeclarationUnsupported { span } => write!(
                f,
                "{}..{}: a local function declaration is not implemented; \
                 declare it at component level",
                span.start, span.end
            ),
```

- [ ] **Step 4: Make `=` a comparison operator**

In `fortressc/crates/parser/src/lib.rs`, in `comparison_op`, add above the `EqEqEq` arm:

```rust
        Kind::Eq => Some(BinOp::Eq),
```

The comment above the function should record why this is safe. Replace the existing doc comment on `comparison_op` with:

```rust
/// `=` is here because every definition site consumes its own `=` first:
/// `member` takes a field's or a function's via `optional_definition`, and
/// `try_binding` takes a binding's. An `=` that reaches this point is equality.
/// The one shape that slips through, `f(x) = e` in block position, is refused
/// by `block_item`.
```

- [ ] **Step 5: Add the block-item guard**

In `fortressc/crates/parser/src/lib.rs`, in `fn block_item`, immediately after `self.pos = save;` and before `let target = self.expr()?;`:

```rust
        // `f(x) = e`: a local function declaration, not a discarded equality.
        // Guarded on tokens rather than on the parsed tree, because a body that
        // is itself an equality (`isZero(x) = x = 0`) collects into a chain and
        // desugars into a block before any tree match could see it.
        if matches!(self.peek_kind(), Some(Kind::Ident(_)))
            && matches!(self.peek_ahead(1), Some(Kind::LParen))
            && self.glued_left(self.pos + 1)
        {
            let probe = self.pos;
            if let Ok(Expr::Call { callee, span, .. }) = self.postfix() {
                if matches!(*callee, Expr::Var { .. }) && self.at(&Kind::Eq) {
                    return Err(ParseError::LocalFunctionDeclarationUnsupported { span });
                }
            }
            self.pos = probe;
        }
```

- [ ] **Step 6: Add the corpus blocker arm**

In `fortressc/crates/parser/tests/corpus.rs`, in the `match &e` that labels blockers, add before the closing brace:

```rust
                    fortress_parser::ParseError::LocalFunctionDeclarationUnsupported {
                        ..
                    } => "local function declaration".to_owned(),
```

- [ ] **Step 7: Run the parser tests and watch them pass**

```bash
cd /home/prestonalthaus/claude/fortress-lang-rewrite/fortressc
cargo test -p fortress-parser 2>&1 | tail -25
```

Expected: all PASS. The corpus test's ratchet still asserts 428 and must not regress; the count should now be **higher**, which the ratchet permits.

- [ ] **Step 8: Measure the parse delta and the leakage**

```bash
cd /home/prestonalthaus/claude/fortress-lang-rewrite/fortressc
cargo test -p fortress-parser --test corpus -- --nocapture 2>&1 | grep -A12 'what blocks'
```

Record the new `parsed N` number and the new top-10 blocker table. Then, from the repository root, re-run Task 1 Step 3's script into `$SCRATCH/m3f-after-eq-parse.txt` and diff the sets:

```bash
cd /home/prestonalthaus/claude/fortress-lang-rewrite
SCRATCH=/tmp/claude-1000/-home-prestonalthaus-claude/92371b9f-ac5f-46f7-a11e-41d63047ecdc/scratchpad
comm -13 "$SCRATCH/m3f-baseline-parse.txt" "$SCRATCH/m3f-after-eq-parse.txt" > "$SCRATCH/m3f-newly-parsing.txt"
wc -l "$SCRATCH/m3f-newly-parsing.txt"
# leakage check: did any of these newly-parsing files get there by misreading a
# local function declaration as a comparison?
while read -r p; do
  grep -Hn -E '^[[:space:]]+[A-Za-z_][A-Za-z0-9_]*[[:space:]]*\([^()]*\)[[:space:]]*=[^=>/]' "$p" | head -2
done < "$SCRATCH/m3f-newly-parsing.txt" | head -40
```

Expected: **no output** from the leakage loop. Any hit is a file that now parses because a local function declaration was misread, and the guard has a hole. Report the count either way.

- [ ] **Step 9: Write the refusal fixture**

`fortressc/tests/localfn.fss`:

```
component localfn
export Executable

run(): () = do
    isZero(x) = x = 0
    println("unreachable")
  end

end
```

- [ ] **Step 10: Check it by hand**

```bash
cd /home/prestonalthaus/claude/fortress-lang-rewrite
fortressc/target/debug/fortressc fortressc/tests/localfn.fss --emit-obj -o /dev/null; echo "exit $?"
```

Expected: exit 1, message contains `a local function declaration is not implemented`.

- [ ] **Step 11: Clippy, fmt and the whole suite**

```bash
cd /home/prestonalthaus/claude/fortress-lang-rewrite/fortressc
cargo test --workspace 2>&1 | tail -25
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

- [ ] **Step 12: Commit**

```bash
cd /home/prestonalthaus/claude/fortress-lang-rewrite
git add fortressc/crates/parser/src/lib.rs fortressc/crates/parser/src/error.rs \
        fortressc/crates/parser/tests/corpus.rs fortressc/crates/parser/tests/parser.rs \
        fortressc/tests/localfn.fss
git commit -m "feat(parser): \`=\` is equality in expression position

Every definition site consumes its own \`=\` first, so an \`=\` reaching the
expression grammar is equality. The one shape that slips through -- \`f(x) = e\`
in block position, 572 lines across 236 corpus files -- is refused with its own
diagnostic rather than silently read as a discarded comparison. The guard is on
tokens, because a body that is itself an equality desugars before a tree match
could see it."
```

---

### Task 4: Chained comparison

**Files:**
- Modify: `fortressc/crates/parser/src/lib.rs` — `Parser` struct gains `chain_temps`, `comparison` collects and desugars, add `chain_sense` and `desugar_chain`
- Modify: `fortressc/crates/parser/src/error.rs` — add `ChainedOperatorsDiffer`, its `span()` arm, its `Display` arm
- Modify: `fortressc/crates/parser/tests/corpus.rs` — one more blocker arm
- Create: `fortressc/tests/chainonce.fss`
- Create: `fortressc/tests/chainmixed.fss`
- Create: `fortressc/tests/badchainsense.fss`
- Test: `fortressc/crates/parser/tests/parser.rs`, `fortressc/crates/driver/tests/end_to_end.rs`

**Interfaces:**
- Consumes: `Task 3`'s `Kind::Eq => Some(BinOp::Eq)` in `comparison_op`; `infix(op: BinOp, fixity: Fixity, lhs: Expr, rhs: Expr) -> Expr`; `Expr::Block`, `BlockItem::Binding`, `Binding`, `Expr::If`, `Expr::BoolLit`, `Expr::Var`.
- Produces: `ParseError::ChainedOperatorsDiffer { span: Span, first: &'static str, second: &'static str }`, used by Task 5's gate.

- [ ] **Step 1: Write the failing tests**

Append to `fortressc/crates/parser/tests/parser.rs`:

```rust
#[test]
fn a_two_operator_chain_desugars_to_a_block() {
    let source = "component c\nrun(): () = if 0 < 1 < 2 then println(\"y\") else println(\"n\") end\nend\n";
    let tokens = fortress_lexer::lex(source).unwrap();
    let parsed = fortress_parser::parse(&tokens).unwrap();
    let body = parsed.decls.first().map(|d| match d {
        fortress_ast::Decl::Function(f) => f.body.clone(),
        _ => None,
    });
    let Some(Some(fortress_ast::Expr::If { cond, .. })) = body else {
        panic!("expected an if at the top of run");
    };
    let fortress_ast::Expr::Block { items, .. } = *cond else {
        panic!("a chain must desugar to a block, got {cond:?}");
    };
    // three temporaries and the nested if
    assert_eq!(items.len(), 4);
}

/// A single comparison is untouched: no block, no temporaries, byte-identical
/// to what every earlier milestone produced.
#[test]
fn a_single_comparison_is_not_a_chain() {
    let source = "component c\nrun(): () = if 0 < 1 then println(\"y\") else println(\"n\") end\nend\n";
    let tokens = fortress_lexer::lex(source).unwrap();
    let parsed = fortress_parser::parse(&tokens).unwrap();
    let body = parsed.decls.first().map(|d| match d {
        fortress_ast::Decl::Function(f) => f.body.clone(),
        _ => None,
    });
    let Some(Some(fortress_ast::Expr::If { cond, .. })) = body else {
        panic!("expected an if at the top of run");
    };
    assert!(
        matches!(*cond, fortress_ast::Expr::Infix { .. }),
        "a single comparison must stay a bare Infix, got {cond:?}"
    );
}

#[test]
fn a_chain_may_mix_equivalence_with_one_ordering_sense() {
    let source = "component c\nrun(): () = if 0 <= 0 < 1 = 1 then println(\"y\") else println(\"n\") end\nend\n";
    let tokens = fortress_lexer::lex(source).unwrap();
    assert!(fortress_parser::parse(&tokens).is_ok());
}

#[test]
fn a_chain_may_not_mix_two_ordering_senses() {
    let source = "component c\nrun(): () = if 1 <= 2 > 0 then println(\"y\") else println(\"n\") end\nend\n";
    let tokens = fortress_lexer::lex(source).unwrap();
    let err = fortress_parser::parse(&tokens).unwrap_err();
    let fortress_parser::ParseError::ChainedOperatorsDiffer { first, second, .. } = err else {
        panic!("expected a mixed-sense diagnostic, got {err:?}");
    };
    assert_eq!((first, second), ("<=", ">"));
}
```

- [ ] **Step 2: Run them and watch them fail**

```bash
cd /home/prestonalthaus/claude/fortress-lang-rewrite/fortressc
cargo test -p fortress-parser --test parser 2>&1 | tail -30
```

Expected: `a_two_operator_chain_desugars_to_a_block` fails on the block assertion, `a_chain_may_not_mix_two_ordering_senses` fails to compile on the missing variant, the other two pass.

- [ ] **Step 3: Add the parse error variant**

In `fortressc/crates/parser/src/error.rs`, add to the enum:

```rust
    /// `a <= b > c`. `chained-multifix.tex:16-34` restricts a chain to a
    /// mixture of equivalence operators and ordering operators of one sense.
    ChainedOperatorsDiffer {
        span: Span,
        first: &'static str,
        second: &'static str,
    },
```

Add to the `span()` alternation:

```rust
            | Self::ChainedOperatorsDiffer { span, .. }
```

Add to `Display`:

```rust
            Self::ChainedOperatorsDiffer {
                span,
                first,
                second,
            } => write!(
                f,
                "{}..{}: a chain mixes `{first}` with `{second}`; \
                 chained ordering operators must have the same sense",
                span.start, span.end
            ),
```

- [ ] **Step 4: Add the sense classifier**

In `fortressc/crates/parser/src/lib.rs`, next to `comparison_op`:

```rust
/// A chain's ordering sense. Equivalence operators carry none and mix freely;
/// two ordering operators must agree. `chained-multifix.tex:16-34`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sense {
    Increasing,
    Decreasing,
}

const fn chain_sense(op: BinOp) -> Option<Sense> {
    match op {
        BinOp::Lt | BinOp::Le => Some(Sense::Increasing),
        BinOp::Gt | BinOp::Ge => Some(Sense::Decreasing),
        _ => None,
    }
}

const fn op_text(op: BinOp) -> &'static str {
    match op {
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::Eq => "=",
        BinOp::Ne => "=/=",
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
    }
}
```

- [ ] **Step 5: Give the parser a temporary counter**

Find the `struct Parser` definition in `fortressc/crates/parser/src/lib.rs` and add a field:

```rust
    /// Monotonic and never reset, so nested chains cannot collide. `$` cannot
    /// appear in a source identifier -- the property `mangle_type` already
    /// relies on -- so a temporary cannot shadow anything the user wrote.
    chain_temps: usize,
```

Initialise it to `0` wherever the struct is constructed.

- [ ] **Step 6: Rewrite `comparison` to collect**

Replace `fn comparison` in `fortressc/crates/parser/src/lib.rs` with:

```rust
    /// Comparison operators chain. One operator is left exactly as it was: no
    /// block, no temporaries, and nothing about existing generated code moves.
    fn comparison(&mut self) -> Parsed<Expr> {
        let first = self.additive()?;
        let mut operands = vec![first];
        let mut ops: Vec<(BinOp, Fixity, Span)> = Vec::new();
        let mut sense: Option<(Sense, BinOp)> = None;

        while let Some(op) = self.peek_kind().and_then(comparison_op) {
            let index = self.pos;
            let Some(fixity) = self.infix_fixity(index)? else {
                break;
            };
            let op_span = self.span_here();
            if let Some(this) = chain_sense(op) {
                match sense {
                    Some((seen, earlier)) if seen != this => {
                        return Err(ParseError::ChainedOperatorsDiffer {
                            span: op_span,
                            first: op_text(earlier),
                            second: op_text(op),
                        });
                    }
                    Some(_) => {}
                    None => sense = Some((this, op)),
                }
            }
            self.pos += 1;
            self.skip_newlines(); // a newline may follow an infix operator
            operands.push(self.additive()?);
            ops.push((op, fixity, op_span));
        }

        if ops.is_empty() {
            return operands.pop().ok_or(ParseError::UnexpectedEndOfInput {
                expected: "an operand",
            });
        }
        if ops.len() == 1 {
            let (op, fixity, _) = ops.remove(0);
            let rhs = operands.pop().ok_or(ParseError::UnexpectedEndOfInput {
                expected: "an operand",
            })?;
            let lhs = operands.pop().ok_or(ParseError::UnexpectedEndOfInput {
                expected: "an operand",
            })?;
            return Ok(infix(op, fixity, lhs, rhs));
        }
        self.desugar_chain(operands, &ops)
    }
```

- [ ] **Step 7: Write the desugar**

Immediately below `comparison` in `fortressc/crates/parser/src/lib.rs`:

```rust
    /// `a < b < c` becomes a block of one binding per operand and a nested
    /// `if`. The bindings are what the specification's "evaluated only once"
    /// requires, and the nested `if` is the conjunction: this subset has no
    /// `AND`, and does not gain one here.
    fn desugar_chain(&mut self, operands: Vec<Expr>, ops: &[(BinOp, Fixity, Span)]) -> Parsed<Expr> {
        let start = operands.first().map_or(0, |e| e.span().start);
        let end = operands.last().map_or(0, |e| e.span().end);
        let span = Span::new(start, end);

        let mut items = Vec::with_capacity(operands.len() + 1);
        let mut names = Vec::with_capacity(operands.len());
        for operand in operands {
            let name = format!("$chain{}", self.chain_temps);
            self.chain_temps += 1;
            let operand_span = operand.span();
            items.push(BlockItem::Binding(Binding {
                name: name.clone(),
                ty: None,
                value: operand,
                mutable: false,
                span: operand_span,
            }));
            names.push((name, operand_span));
        }

        let comparison_at = |index: usize| -> Parsed<Expr> {
            let (op, fixity, _) = ops.get(index).copied().ok_or(
                ParseError::UnexpectedEndOfInput {
                    expected: "a chained operand",
                },
            )?;
            let (lhs_name, lhs_span) = names.get(index).cloned().ok_or(
                ParseError::UnexpectedEndOfInput {
                    expected: "a chained operand",
                },
            )?;
            let (rhs_name, rhs_span) = names.get(index + 1).cloned().ok_or(
                ParseError::UnexpectedEndOfInput {
                    expected: "a chained operand",
                },
            )?;
            Ok(infix(
                op,
                fixity,
                Expr::Var {
                    name: lhs_name,
                    span: lhs_span,
                },
                Expr::Var {
                    name: rhs_name,
                    span: rhs_span,
                },
            ))
        };

        let mut tail = comparison_at(ops.len() - 1)?;
        for index in (0..ops.len() - 1).rev() {
            tail = Expr::If {
                cond: Box::new(comparison_at(index)?),
                then_branch: Box::new(tail),
                else_branch: Some(Box::new(Expr::BoolLit { value: false, span })),
                span,
            };
        }
        items.push(BlockItem::Expr(tail));
        Ok(Expr::Block { items, span })
    }
```

- [ ] **Step 8: Add the corpus blocker arm**

In `fortressc/crates/parser/tests/corpus.rs`, add to the `match &e`:

```rust
                    fortress_parser::ParseError::ChainedOperatorsDiffer { .. } => {
                        "chain mixes ordering senses".to_owned()
                    }
```

- [ ] **Step 9: Run the parser tests and watch them pass**

```bash
cd /home/prestonalthaus/claude/fortress-lang-rewrite/fortressc
cargo test -p fortress-parser 2>&1 | tail -25
```

Expected: all PASS.

- [ ] **Step 10: Write the end-to-end fixtures**

`fortressc/tests/chainonce.fss`:

```
component chainonce
export Executable

mid(x: ZZ64): ZZ64 = do
    println("MID")
    x
  end

run(): () = do
    if 0 < mid(1) < 2 then println("YES") else println("NO") end
  end

end
```

`fortressc/tests/chainmixed.fss`:

```
component chainmixed
export Executable

run(): () = do
    if 0 <= 0 < 1 = 1 < 2 <= 2 then println("YES") else println("NO") end
  end

end
```

`fortressc/tests/badchainsense.fss`:

```
component badchainsense
export Executable

run(): () = if 1 <= 2 > 0 then println("y") else println("n") end

end
```

- [ ] **Step 11: Write the end-to-end tests**

Append to `fortressc/crates/driver/tests/end_to_end.rs`:

```rust
/// The one property of chaining that is observable from inside the language:
/// the middle operand is evaluated exactly once. This subset has no mutable
/// global and no closure, so the counter is a print.
#[test]
fn a_chain_evaluates_its_middle_operand_once() {
    let binary = compile_fixture("chainonce.fss", "chainonce");
    let out = run(&binary);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        stdout.matches("MID").count(),
        1,
        "the middle operand ran more than once: {stdout}"
    );
    assert!(stdout.contains("YES"), "the chain was false: {stdout}");
    let _ = std::fs::remove_file(&binary);
}

#[test]
fn a_chain_mixing_equivalence_with_one_sense_is_true() {
    let binary = compile_fixture("chainmixed.fss", "chainmixed");
    let out = run(&binary);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "YES\n");
    let _ = std::fs::remove_file(&binary);
}
```

- [ ] **Step 12: Run them**

```bash
cd /home/prestonalthaus/claude/fortress-lang-rewrite/fortressc
cargo test -p fortress-driver --test end_to_end chain 2>&1 | tail -20
```

Expected: both PASS. If `chainonce` fails because `mid`'s body block cannot hold a void statement before its value, check `Checker::block` — a non-final block item takes `expected = None` and any type is allowed, so it should compile.

- [ ] **Step 13: Check the refusal fixture by hand**

```bash
cd /home/prestonalthaus/claude/fortress-lang-rewrite
fortressc/target/debug/fortressc fortressc/tests/badchainsense.fss --emit-obj -o /dev/null; echo "exit $?"
```

Expected: exit 1, message contains ``a chain mixes `<=` with `>` ``.

- [ ] **Step 14: Clippy, fmt and the whole suite**

```bash
cd /home/prestonalthaus/claude/fortress-lang-rewrite/fortressc
cargo test --workspace 2>&1 | tail -25
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

- [ ] **Step 15: Measure both metrics again, and the exit-70 count**

Re-run Task 1 Step 2 (parser corpus), Task 1 Step 3 (parse set, into `$SCRATCH/m3f-final-parse.txt`) and Task 1 Step 4 (driver sweep, into `$SCRATCH/m3f-final-compile.txt`).

Expected: parse around 477 (the spike measured `+49` for `=` alone; the desugar may move it), compile at least 181, **zero** exit-70. Report the measured numbers; the design says chained comparison's compile delta is not predicted, so whatever it is, that is the number.

Then diff the compile set and read what is new:

```bash
cd /home/prestonalthaus/claude/fortress-lang-rewrite
SCRATCH=/tmp/claude-1000/-home-prestonalthaus-claude/92371b9f-ac5f-46f7-a11e-41d63047ecdc/scratchpad
comm -13 "$SCRATCH/m3f-baseline-compile.txt" "$SCRATCH/m3f-final-compile.txt" | head -40
```

- [ ] **Step 16: Commit**

```bash
cd /home/prestonalthaus/claude/fortress-lang-rewrite
git add fortressc/crates/parser/src/lib.rs fortressc/crates/parser/src/error.rs \
        fortressc/crates/parser/tests/corpus.rs fortressc/crates/parser/tests/parser.rs \
        fortressc/crates/driver/tests/end_to_end.rs \
        fortressc/tests/chainonce.fss fortressc/tests/chainmixed.fss \
        fortressc/tests/badchainsense.fss
git commit -m "feat(parser): chained comparison

chained-multifix.tex:16-34. A chain of two or more comparison operators becomes
a block of one binding per operand -- which is what \"evaluated only once\"
means -- and a nested \`if\`, which is the conjunction without adding an \`AND\`
this subset does not have. Mixing two ordering senses is refused, naming both.
One comparison operator is left byte-identical to what it was."
```

---

### Task 5: The gate

**Files:**
- Create: `tools/apply-gate.sh`

**Interfaces:**
- Consumes: every fixture from Tasks 2, 3 and 4; `fortressc/target/debug/fortressc`.
- Produces: nothing the compiler uses. `./tools/apply-gate.sh`, `--selftest`, `--mutate`.

- [ ] **Step 1: Write the gate**

Create `tools/apply-gate.sh`, modelled on `tools/unit-gate.sh`:

```bash
#!/usr/bin/env bash
#
# The M3f gate: juxtaposition as function application, and chained comparison.
#
# Six things cargo cannot check on its own: that `println "Hello"` becomes a
# real ELF that prints the right bytes, that a parameter shadowing a function
# name is NOT application, that a singleton object is a value and not a
# constructor, that a three-element juxtaposition halts with exit 1 rather than
# 70, that a chain evaluates its middle operand exactly once, and that a chain
# mixing two ordering senses is refused by name.
#
# It also carries this milestone's headline number. The parser corpus test stops
# at the parser and cannot see the compile metric at all, so the gate sweeps all
# 1956 corpus files with the real driver and fails if the count drops or if any
# file exits 70.
#
#   ./tools/apply-gate.sh              run the gate
#   ./tools/apply-gate.sh --selftest   only prove the assertions can refuse
#   ./tools/apply-gate.sh --mutate     break the compiler four ways and prove
#                                      the gate refuses each one
set -uo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
build=$repo/fortressc/build
fortressc=$repo/fortressc/target/debug/fortressc
export LLVM_SYS_221_PREFIX=${LLVM_SYS_221_PREFIX:-$HOME/.local/opt/llvm22-root/usr/lib64/llvm22}
export CPATH=${CPATH:-$HOME/.local/opt/gc-root/usr/include}
export LIBRARY_PATH=${LIBRARY_PATH:-$HOME/.local/opt/gc-root/usr/lib64}

# The floors. Set from the measurement at the end of Task 4, not from the
# design document.
COMPILE_FLOOR=181

passed=0
failed=0
ok()  { passed=$((passed + 1)); printf 'ok    %s\n' "$1"; }
bad() { failed=$((failed + 1)); printf 'FAIL  %s\n' "$1"; [[ -n ${2:-} ]] && printf '      %s\n' "$2"; }

# ---------------------------------------------------------------- assertions

# A diagnostic is exit 1 and nothing else. 70 is EXIT_INTERNAL_ERROR.
refused_cleanly() { [[ $1 -eq 1 ]]; }

# The middle operand of a chain must run exactly once. Counting is the whole
# assertion, so it is its own function and it is self-tested.
occurrences() { grep -c -F -- "$2" <<<"$1"; }

selftest() {
    printf '== gate self test ==\n'

    if refused_cleanly 1; then ok 'exit 1 is a clean refusal'; else bad 'exit 1 is a clean refusal'; fi
    for status in 0 70 124 139; do
        if refused_cleanly "$status"; then
            bad "status $status is refused as a clean refusal" 'only exit 1 is a diagnostic'
        else
            ok "status $status is refused as a clean refusal"
        fi
    done

    local sample
    sample=$'MID\nYES'
    if [[ $(occurrences "$sample" MID) -eq 1 ]]; then
        ok 'one MID counts as one'
    else
        bad 'one MID counts as one'
    fi
    sample=$'MID\nMID\nYES'
    if [[ $(occurrences "$sample" MID) -eq 2 ]]; then
        ok 'two MIDs count as two'
    else
        bad 'two MIDs count as two' 'the counter cannot see a duplicated operand'
    fi
}

# ------------------------------------------------------------------ the gate

preflight() {
    ( cd "$repo/fortressc" && cargo build --workspace ) || exit 2
    rm -rf "$build"
    mkdir -p "$build"
}

# name|expected stdout|label
runs_and_prints() {
    printf '== programs that run ==\n'
    local name want label out status
    while IFS='|' read -r name want label; do
        [[ -z $name ]] && continue
        if ! "$fortressc" "$repo/fortressc/tests/$name.fss" -o "$build/$name" 2>"$build/$name.err"; then
            bad "$label" "$(cat "$build/$name.err")"
            continue
        fi
        out=$("$build/$name" 2>&1)
        status=$?
        if [[ $status -eq 0 && $out == "$(printf '%b' "$want")" ]]; then
            ok "$label"
        else
            bad "$label" "status $status: $out"
        fi
    done <<'CASES'
juxtapply|Hello\n42|`println "Hello"` and `double 21` are applications
juxtshadow|12|a parameter shadowing a function name stays multiplication
juxtnullary|42|`answer ()` is the zero-argument call
chainmixed|YES|a chain mixes equivalence with one ordering sense
CASES
}

evaluated_once() {
    printf '== a chain evaluates its middle operand once ==\n'
    if ! "$fortressc" "$repo/fortressc/tests/chainonce.fss" -o "$build/chainonce" \
        2>"$build/chainonce.err"; then
        bad 'chainonce.fss compiles' "$(cat "$build/chainonce.err")"
        return
    fi
    local out count
    out=$("$build/chainonce" 2>&1)
    count=$(occurrences "$out" MID)
    if [[ $count -eq 1 ]]; then
        ok "the middle operand ran once"
    else
        bad "the middle operand ran once" "it ran $count times: $out"
    fi
    if [[ $out == *YES* ]]; then
        ok 'the chain is true'
    else
        bad 'the chain is true' "$out"
    fi
}

# Four refusals. The PHRASE is the assertion, not the exit code: every one of
# these is exit 1 both with and without the code under test, and only the
# message distinguishes them.
refusals() {
    printf '== the refusals ==\n'
    local name phrase err status
    while IFS='|' read -r name phrase; do
        [[ -z $name ]] && continue
        err=$("$fortressc" "$repo/fortressc/tests/$name.fss" --emit-obj -o /dev/null 2>&1 >/dev/null)
        status=$?
        if refused_cleanly "$status" && [[ $err == *"$phrase"* ]]; then
            ok "$name.fss is refused (exit $status)"
        else
            bad "$name.fss is refused" "status $status: $err"
        fi
    done <<'CASES'
juxtnary|a juxtaposition of 3 elements led by a function is not implemented
juxtsingleton|neither multiplication nor concatenation
localfn|a local function declaration is not implemented
badchainsense|chained ordering operators must have the same sense
CASES
}

# The milestone's headline number, and the first time it is guarded. The parser
# corpus test stops at the parser and cannot see this.
compile_metric() {
    printf '== the compile metric ==\n'
    local report compiled internal
    report=$(cd "$repo" && python3 - <<'PY'
import os, subprocess, collections
files = []
for d, ds, fs in os.walk('.'):
    ds[:] = [x for x in ds if x not in ('.git', 'target', 'fortressc')]
    files += [os.path.join(d, f) for f in fs if f.endswith(('.fss', '.fsi'))]
files.sort()
c = collections.Counter()
for p in files:
    r = subprocess.run(['fortressc/target/debug/fortressc', p, '--emit-obj', '-o', '/dev/null'],
                       capture_output=True, text=True)
    c[r.returncode] += 1
print(c[0], c[70], len(files))
PY
)
    read -r compiled internal _total <<<"$report"
    if [[ $compiled -ge $COMPILE_FLOOR ]]; then
        ok "$compiled files compile end to end (floor $COMPILE_FLOOR)"
    else
        bad "$compiled files compile end to end" "floor is $COMPILE_FLOOR"
    fi
    if [[ $internal -eq 0 ]]; then
        ok 'no corpus file makes the compiler exit 70'
    else
        bad 'no corpus file makes the compiler exit 70' "$internal file(s) did"
    fi
}

# ----------------------------------------------------------------- mutations

MUTATIONS=(
  'crates/types/src/lib.rs|if self.lookup(name).is_some() {|if false {|drop the shadowing guard on a function element'
  'crates/parser/src/lib.rs|let name = format!("$chain{}", self.chain_temps);|let name = format!("$chain{}", self.chain_temps); let _ = &name;|placeholder, replaced in step 3'
  'crates/parser/src/lib.rs|Some((seen, earlier)) if seen != this => {|Some((seen, earlier)) if false && seen != this && earlier == op => {|drop the chain sense check'
  'crates/parser/src/lib.rs|&& self.glued_left(self.pos + 1)|&& false|drop the local function declaration guard'
)

mutate() {
    if ! git -C "$repo" diff --quiet -- fortressc/crates; then
        printf 'refusing to mutate: fortressc/crates has unstaged changes\n' >&2
        exit 2
    fi

    local entry file from to label hits status
    local broken=0 survived=0
    for entry in "${MUTATIONS[@]}"; do
        IFS='|' read -r file from to label <<<"$entry"
        printf '\n== mutation: %s ==\n' "$label"

        hits=$(grep -F -c -- "$from" "$repo/fortressc/$file")
        if [[ $hits -ne 1 ]]; then
            printf 'FAIL  the mutation pattern is not unique (%s hits in %s)\n' "$hits" "$file"
            broken=$((broken + 1))
            continue
        fi

        python3 - "$repo/fortressc/$file" "$from" "$to" <<'PY'
import sys, pathlib
path, old, new = sys.argv[1], sys.argv[2], sys.argv[3]
p = pathlib.Path(path)
p.write_text(p.read_text().replace(old, new, 1))
PY
        ( cd "$repo/fortressc" && cargo build --workspace >/dev/null 2>&1 )
        status=$?
        if [[ $status -ne 0 ]]; then
            printf 'FAIL  the mutated compiler does not build\n'
            broken=$((broken + 1))
        else
            rm -rf "$build"; mkdir -p "$build"
            passed=0; failed=0
            runs_and_prints; evaluated_once; refusals
            if [[ $failed -gt 0 ]]; then
                printf 'REFUSED  %d check(s) failed, which is the point\n' "$failed"
            else
                printf 'SURVIVED %s -- the gate did not notice\n' "$label"
                survived=$((survived + 1))
            fi
        fi
        git -C "$repo" checkout -- "fortressc/$file"
    done

    ( cd "$repo/fortressc" && cargo build --workspace >/dev/null 2>&1 )
    printf '\nmutations: %d run, %d survived, %d could not be applied\n' \
        "${#MUTATIONS[@]}" "$survived" "$broken"
    [[ $survived -eq 0 && $broken -eq 0 ]]
}

# ----------------------------------------------------------------- main

case "${1:-}" in
    --selftest) selftest ;;
    --mutate)   selftest; preflight; mutate; exit $? ;;
    *)          selftest; preflight; runs_and_prints; evaluated_once; refusals; compile_metric ;;
esac

printf '\n%d/%d\n' "$passed" "$failed"
[[ $failed -eq 0 ]]
```

- [ ] **Step 2: Make it executable and run the self test**

```bash
cd /home/prestonalthaus/claude/fortress-lang-rewrite
chmod +x tools/apply-gate.sh
./tools/apply-gate.sh --selftest
```

Expected: 7/0. Every assertion has been shown to accept a right answer and refuse a wrong one before it is used on anything.

- [ ] **Step 3: Replace the placeholder mutation with a real evaluate-once mutation**

Mutation 2 must make the desugar duplicate the operand instead of binding it, so the middle operand runs twice. Look at what `desugar_chain` compiled to and pick a single grep-unique line that does it. The intended form: in `desugar_chain`, replace the `Expr::Var` on the right-hand side of each comparison with a clone of the original operand expression. Because `desugar_chain` consumes `operands` into the bindings, the simplest grep-unique mutation that reproduces double evaluation is to make `comparison_at` build both sides from a re-parse-free clone. If no single-line mutation is available, restructure `desugar_chain` so it is: keep a `Vec<Expr>` of the original operands alongside `names`, and have `comparison_at` read from `names`; the mutation then flips one `names.get(...)` to the original-operand vector. Adjust the code so the mutation is one line, then record the exact `from` and `to` strings in `MUTATIONS`.

Verify the mutation actually doubles the output before trusting it:

```bash
cd /home/prestonalthaus/claude/fortress-lang-rewrite
./tools/apply-gate.sh --mutate 2>&1 | tail -30
```

- [ ] **Step 4: Run the mutation suite**

```bash
cd /home/prestonalthaus/claude/fortress-lang-rewrite
./tools/apply-gate.sh --mutate 2>&1 | tail -40
```

Expected: `mutations: 4 run, 0 survived, 0 could not be applied`. State each mutation and the numbers it produced when reporting. A gate is not trusted until it has refused.

- [ ] **Step 5: Set `COMPILE_FLOOR` from the measurement and run the gate**

Set `COMPILE_FLOOR` to the number Task 4 Step 15 measured, not to 181 if the measurement disagrees.

```bash
cd /home/prestonalthaus/claude/fortress-lang-rewrite
./tools/apply-gate.sh 2>&1 | tail -30
```

Expected: all green, and the last line is `N/0`.

- [ ] **Step 6: Re-run the other six gates**

```bash
cd /home/prestonalthaus/claude/fortress-lang-rewrite
for g in generics dispatch array memory mpi unit; do
  printf '\n### %s\n' "$g"
  ./tools/$g-gate.sh 2>&1 | tail -3
done
```

Expected: generics 20/0, dispatch 19/0, array 16/0, memory 17/0, MPI 17/0, unit 15/0. Nothing in this milestone should move any of them; if one moves, understand it before continuing.

- [ ] **Step 7: Commit**

```bash
cd /home/prestonalthaus/claude/fortress-lang-rewrite
git add tools/apply-gate.sh
git commit -m "test(m3f): the apply gate, with the compile metric's first floor

Six checks cargo cannot make, four mutations, and the milestone's headline
number. The parser corpus test stops at the parser, so nothing has ever guarded
the count of files that compile end to end; the gate sweeps all 1956 with the
real driver and fails on a drop or on any exit 70."
```

---

### Task 6: Ratchets, documentation and state

**Files:**
- Modify: `fortressc/crates/parser/tests/corpus.rs` — the ratchet floor and its comment
- Modify: `ROADMAP.md`
- Modify: `docs/superpowers/specs/2026-08-19-m3f-application-chaining-design.md` — status line and §6/§7 numbers
- Modify: `/home/prestonalthaus/claude/04-state.md`
- Modify: `/home/prestonalthaus/.claude/projects/-home-prestonalthaus-claude/memory/MEMORY.md` and one memory file

**Interfaces:**
- Consumes: every measurement from Tasks 2, 3, 4 and 5.
- Produces: nothing code depends on.

- [ ] **Step 1: Move the parser ratchet**

In `fortressc/crates/parser/tests/corpus.rs`, replace the floor and its comment with the measured number:

```rust
    // The same ratchet. The lexer pass took this from 84 to 154; M3d's static
    // parameters took it to 168; M3e's `()` took it to 428, of which the unit
    // type alone was 232; M3f's `=` as an equality operator took it to <N>.
    assert!(
        parsed >= <N>,
        "parser corpus regressed: {parsed} files parse, floor is <N>"
    );
```

Replace `<N>` with the number Task 4 Step 15 measured. Run `cargo test -p fortress-parser --test corpus` and confirm it passes on the nose.

- [ ] **Step 2: Update the design document's status and measured numbers**

In `docs/superpowers/specs/2026-08-19-m3f-application-chaining-design.md`:
- Change the status line to `Status: **landed**` plus the commit range.
- In §6, replace "whatever the implementation measures" and "about 477" with the measured parse number, and "Measured target is 181" with the measured compile number.
- In §7, replace "Not measured: what chained comparison does to the compile metric" with the number, now that it is measured.
- Add a short subsection recording the three deviations listed at the top of the implementation plan, each with its measurement.

- [ ] **Step 3: Update `ROADMAP.md`**

Tick M3f, record the two metrics, and name the next milestone. The M3f candidate table in `04-state.md` still holds the measured deltas for the nine constructs that were not built; do not delete it, it is the input to M3g.

- [ ] **Step 4: Update `04-state.md`**

Set `last_updated` to today. Under `fortress_compiler`, rewrite `active_focus`, `recent_wins`, `known_bugs` and `next_steps`:
- `active_focus`: the branch and its state, the seven gates and their numbers, the corpus numbers.
- `recent_wins`: what M3f delivered, the three deviations with their measurements, and the fact that the compile metric now has a floor for the first time.
- `known_bugs`: carry forward the deliberate-do-not-fix list, and add: n-ary juxtaposition refused (measured at zero files), local function declarations refused, multifix operators out, no `AND`/`OR`.
- `next_steps`: the remaining M3f scouting table, unchanged, minus the two constructs this milestone consumed.

- [ ] **Step 5: Update memory**

Both existing memories are still right and both gained evidence. Append to `measure-by-experiment-not-by-counting.md`: the blocker histogram was wrong a **fourth** time — `var` 105 blockers to +6 files, `opr` 97 to +5, chained `=` 51 to +49. Append to `full-driver-sweep-finds-latent-crashes.md`: the sweep is now a gate with a floor, not just a one-off measurement, and it runs in about 9 seconds over 1956 files.

- [ ] **Step 6: Final verification, everything, measured**

```bash
cd /home/prestonalthaus/claude/fortress-lang-rewrite/fortressc
export LLVM_SYS_221_PREFIX=$HOME/.local/opt/llvm22-root/usr/lib64/llvm22
export CPATH=$HOME/.local/opt/gc-root/usr/include
export LIBRARY_PATH=$HOME/.local/opt/gc-root/usr/lib64
cargo test --workspace 2>&1 | tail -20
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
cd /home/prestonalthaus/claude/fortress-lang-rewrite
for g in generics dispatch array memory mpi unit apply; do
  printf '\n### %s\n' "$g"; ./tools/$g-gate.sh 2>&1 | tail -2
done
```

- [ ] **Step 7: Commit**

```bash
cd /home/prestonalthaus/claude/fortress-lang-rewrite
git add fortressc/crates/parser/tests/corpus.rs ROADMAP.md \
        docs/superpowers/specs/2026-08-19-m3f-application-chaining-design.md
git commit -m "docs(m3f): ratchets and results

Parser floor moves to the measured number. The design document's predicted
figures are replaced by measured ones, and the three deviations found while
planning are recorded with the measurements behind them."
```

The `04-state.md` and memory files live outside this repository and are not committed here.

---

## Self-review

**Spec coverage.** §1.1 classification → Task 2 Steps 5-6. §1.2 binary only → Task 2 Step 6 and fixture `juxtnary`. §1.3 placement ahead of the probe → Task 2 Step 6. §2.1 which operators chain → Task 4 Step 4. §2.2 `=` in `comparison_op` → Task 3 Step 4. §2.3 the desugar → Task 4 Step 7. §2.4 multifix out → not implemented, recorded in Task 6 Step 4. §3 scope boundary → the three refusal fixtures plus `localfn`. §4 diagnostics → Task 2 Step 4, Task 4 Step 3 (and `LocalFunctionDeclarationUnsupported`, a deviation, Task 3 Step 3). §5 gate → Task 5. §6 ratchets → Task 5 Step 5 and Task 6 Step 1. §7 measured versus not → Task 4 Step 15 and Task 6 Step 2.

**Type consistency.** `is_function_element(&self, name: &str) -> bool` is defined in Task 2 Step 5 and called in Task 2 Step 6. `chain_sense(op: BinOp) -> Option<Sense>` and `op_text(op: BinOp) -> &'static str` are defined in Task 4 Step 4 and called in Task 4 Step 6. `desugar_chain(&mut self, operands: Vec<Expr>, ops: &[(BinOp, Fixity, Span)])` is defined in Task 4 Step 7 and called in Task 4 Step 6 with exactly those types. `chain_temps: usize` is added in Task 4 Step 5 and used in Task 4 Step 7. `ParseError::LocalFunctionDeclarationUnsupported { span }` is one field, used identically in Task 3 Steps 3 and 5.

**Known soft spot, stated rather than hidden.** Task 5 Step 3 asks the engineer to restructure `desugar_chain` if no single-line mutation reproduces double evaluation. That is the one step in this plan that does not hand over finished code, because the exact grep-unique string depends on how `desugar_chain` reads after `cargo fmt` has reflowed it. The requirement is precise — one grep-unique line, mutation makes `chainonce` print `MID` twice — and the shape of the fix is given.

**One limitation, deliberate and not worth code.** `(f) x` unwraps to `Expr::Var` in `primary`, so a parenthesised identifier is indistinguishable from a bare one and would also be treated as application. The specification distinguishes them. Tracking parenthesisation would mean a new AST field for a form the corpus does not contain; it is recorded here instead.
