# M3e Implementation Plan: the unit type `()`, with syntax for tuples and arrows

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `()` a writable type and a writable value resolving to the existing `Type::Void`, give tuple types, tuple expressions and arrow types a real AST shape with a clean diagnostic, and close the four positions where a Void that cannot be stored would otherwise reach codegen.

**Architecture:** `TypeRef` stops being a `{ name, args, span }` struct and becomes a four-variant enum. `Type` in the types crate gains nothing and stays `Copy` — tuples and arrows are parsed and refused, never resolved. `()` resolves to `Type::Void`, which already exists and which codegen already lowers to an LLVM void. `->` is recognised from glued `Minus`+`Gt` in type position; the lexer is not touched.

**Tech Stack:** Rust edition 2021+, `logos` lexer, hand-written recursive descent parser, `inkwell`/LLVM 22 codegen. Bash gates under `tools/`.

Design document: `docs/superpowers/specs/2026-08-19-m3e-unit-tuple-arrow-design.md`. Read it before Task 1.

## Global Constraints

- Build needs all three, every time, or `cargo build` fails at link:
  ```
  export LLVM_SYS_221_PREFIX=$HOME/.local/opt/llvm22-root/usr/lib64/llvm22
  export CPATH=$HOME/.local/opt/gc-root/usr/include
  export LIBRARY_PATH=$HOME/.local/opt/gc-root/usr/lib64
  ```
- Linker driver is `cc`. `lld` is not installed.
- Every task ends green on `cargo test`, `cargo fmt --all -- --check`, and `cargo clippy --all-targets -- -D warnings`. All three are enforced; clippy denies warnings workspace-wide.
- Compiler passes return `Result`. Never `unwrap()` or `panic!` on user-supplied source. A malformed program is a diagnostic and exit 1, never exit 70.
- `Type` in `crates/types/src/types.rs` stays `Copy`. No new variant. If a task seems to need one, stop — the design is being violated.
- No new allocation path. Nothing in this milestone touches `runtime/`.
- Never emit an implementation-specific symbol into LLVM IR.
- Comment only non-obvious logic. The house style is expressive names, not docstrings on everything.
- Work on a branch off `master` (`267373e1b`). Do not commit to `master`.
- Do not push, do not open a PR. The design document stays local and ships with the implementation.
- A gate is not trusted until it has refused. Task 9's mutations must be run and shown to fail.

## File Structure

| File | Responsibility | Task |
|---|---|---|
| `crates/ast/src/nodes.rs` | `TypeRef` enum, `span()`, `written()`; `Expr::Unit`, `Expr::Tuple` | 1, 7, 8 |
| `crates/parser/src/lib.rs` | `type_ref` / `type_atom` split; `primary`'s `LParen` arm | 1, 3, 4, 5, 6, 7, 8 |
| `crates/types/src/error.rs` | `TypeNotImplemented`, `VoidNotStorable` | 1, 2, 3 |
| `crates/types/src/registry.rs` | `resolve` over the four `TypeRef` forms | 1, 3, 5, 6 |
| `crates/types/src/mono.rs` | `ty` substitution, `mangle_static` / `mangle_type` | 1 |
| `crates/types/src/lib.rs` | `supertrait`, binding check, parameter/field guards, `Expr::Unit` / `Expr::Tuple` checking | 1, 2, 3, 7, 8 |
| `crates/codegen/src/lib.rs` | `TypedExprKind::Unit` | 7 |
| `crates/parser/tests/parser.rs` | grammar tests | 3–8 |
| `crates/types/tests/types.rs` | resolution and refusal tests | 2, 3, 5, 6, 7, 8 |
| `crates/driver/tests/end_to_end.rs` | `run():() = ()` compiles, links, runs | 7 |
| `crates/parser/tests/corpus.rs` | the parser ratchet | 10 |
| `tests/unitvoid.fss` | the positive fixture: a void function that runs | 7 |
| `tests/badvoidparam.fss`, `tests/badtupletype.fss`, `tests/badarrowtype.fss`, `tests/badtupleexpr.fss` | the four negative gate fixtures | 9 |
| `tools/unit-gate.sh` | the M3e gate, `--selftest` and `--mutate` | 9 |
| `ROADMAP.md`, `04-state.md`, the design doc | the record | 10 |

---

### Task 1: `TypeRef` becomes an enum

Pure refactor. The enum lands, every crate is updated to match it, and **only `Named` is reachable from the parser**. Nothing about what compiles changes. `TypeError::TypeNotImplemented` is introduced here so no arm has to panic.

**Files:**
- Modify: `crates/ast/src/nodes.rs:116-122`
- Modify: `crates/parser/src/lib.rs:565-582`
- Modify: `crates/types/src/error.rs`
- Modify: `crates/types/src/registry.rs:98-140`
- Modify: `crates/types/src/mono.rs:221-259`, `crates/types/src/mono.rs:519-530`
- Modify: `crates/types/src/lib.rs:199-207`
- Test: `crates/parser/tests/parser.rs`

**Interfaces:**
- Produces: `TypeRef::{Named{name,args,span}, Unit{span}, Tuple{elems,span}, Arrow{from,to,span}}`; `TypeRef::span(&self) -> Span`; `TypeRef::written(&self) -> String`; `TypeError::TypeNotImplemented{span, form: &'static str}`; `fortress_types::mangle_static(name: &str, args: &[TypeRef]) -> String` unchanged in signature.

- [ ] **Step 1: Write the failing test**

In `crates/parser/tests/parser.rs`, at the end of the file:

```rust
// ------------------------------------------------------------- type syntax

fn return_type(decl: &str) -> fortress_ast::TypeRef {
    let src = format!("component t\n{decl}\nend\n");
    match component(&src).decls.into_iter().next() {
        Some(Decl::Function(f)) => f.return_type.expect("a declared return type"),
        other => panic!("expected a function, got {other:?}"),
    }
}

#[test]
fn a_plain_name_is_a_named_type() {
    match return_type("f(): ZZ32 = 1") {
        fortress_ast::TypeRef::Named { name, args, .. } => {
            assert_eq!(name, "ZZ32");
            assert!(args.is_empty());
        }
        other => panic!("expected a named type, got {other:?}"),
    }
}

#[test]
fn a_named_type_renders_its_static_arguments() {
    let written = return_type("f(): Array[\\ZZ64\\] = 1").written();
    assert_eq!(written, "Array[\\ZZ64\\]");
}
```

`Decl` is already imported in this file. Add `fortress_ast::TypeRef` inline as written above rather than to the `use` list, so the test reads as a type-shape assertion.

- [ ] **Step 2: Run the test to verify it fails**

```bash
cd fortressc && cargo test -p fortress-parser --test parser a_plain_name_is_a_named_type
```

Expected: FAIL to compile — `TypeRef` is a struct, so `TypeRef::Named` does not exist and `written` is not a method.

- [ ] **Step 3: Replace the `TypeRef` struct with the enum**

In `crates/ast/src/nodes.rs`, replace the `TypeRef` struct (currently lines 116-122, keep the existing doc comment above it and extend it):

```rust
/// Types are bare names (`ZZ32`), a name applied to static arguments
/// (`Map[\ZZ64, List[\String\]\]`), the unit type `()`, a tuple of two or more,
/// or an arrow. Resolution happens in the types crate; the parser only records
/// what was written. After monomorphization no `TypeRef` in a component has
/// static arguments -- expansion rewrites every one to a ground name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeRef {
    Named {
        name: String,
        args: Vec<TypeRef>,
        span: Span,
    },
    /// `()`. The specification's special type, pronounced void; not a tuple.
    Unit { span: Span },
    /// Two or more, by construction. A one-element parenthesised list is
    /// unwrapped by the parser and can never arrive here.
    Tuple { elems: Vec<TypeRef>, span: Span },
    /// `A -> B`, right associative. Parsed, never resolved: this subset has no
    /// function values, so an arrow type is uninhabited.
    Arrow {
        from: Box<TypeRef>,
        to: Box<TypeRef>,
        span: Span,
    },
}

impl TypeRef {
    #[must_use]
    pub const fn span(&self) -> Span {
        match self {
            Self::Named { span, .. }
            | Self::Unit { span }
            | Self::Tuple { span, .. }
            | Self::Arrow { span, .. } => *span,
        }
    }

    /// The type as the user wrote it, for diagnostics.
    #[must_use]
    pub fn written(&self) -> String {
        match self {
            Self::Named { name, args, .. } if args.is_empty() => name.clone(),
            Self::Named { name, args, .. } => {
                let inner: Vec<String> = args.iter().map(Self::written).collect();
                format!("{name}[\\{}\\]", inner.join(", "))
            }
            Self::Unit { .. } => "()".to_owned(),
            Self::Tuple { elems, .. } => {
                let inner: Vec<String> = elems.iter().map(Self::written).collect();
                format!("({})", inner.join(", "))
            }
            Self::Arrow { from, to, .. } => format!("{} -> {}", from.written(), to.written()),
        }
    }
}
```

- [ ] **Step 4: Update the parser's two construction sites**

In `crates/parser/src/lib.rs`, `fn type_ref` (currently lines 565-582) — the body keeps its shape, only the constructor changes:

```rust
    fn type_ref(&mut self) -> Parsed<TypeRef> {
        let (name, span) = self.identifier("a type name")?;
        if !self.at(&Kind::LGeneric) {
            return Ok(TypeRef::Named {
                name,
                args: Vec::new(),
                span,
            });
        }
        self.pos += 1;
        let args = self.type_args()?;
        let close = self.expect(&Kind::RGeneric, "`\\]`")?.span;
        Ok(TypeRef::Named {
            name,
            args,
            span: Span::new(span.start, close.end),
        })
    }
```

- [ ] **Step 5: Add the new error variant**

In `crates/types/src/error.rs`, add to the `TypeError` enum, after `UnknownType`:

```rust
    /// A type form the parser accepts and this subset does not implement.
    /// `form` names it: "a tuple type", "an arrow type", "a tuple expression".
    TypeNotImplemented {
        span: Span,
        form: &'static str,
    },
```

Add `| Self::TypeNotImplemented { span, .. }` to the `span()` accessor's list of span-carrying variants (the chain beginning at line 237), and to the `Display` impl:

```rust
            Self::TypeNotImplemented { form, .. } => {
                write!(f, "{form} is not implemented in this subset")
            }
```

- [ ] **Step 6: Update `registry::resolve`**

In `crates/types/src/registry.rs`, replace the head of `resolve` (line 98 onward). Everything from `if name == "Array"` down is the existing body with `t.name` → `name`, `t.args` → `args`, `t.span` → `span`:

```rust
    pub(crate) fn resolve(&self, t: &TypeRef) -> Result<Type, TypeError> {
        let (name, args, span) = match t {
            TypeRef::Named { name, args, span } => (name, args, *span),
            TypeRef::Unit { span } => {
                return Err(TypeError::TypeNotImplemented {
                    span: *span,
                    form: "the unit type",
                })
            }
            TypeRef::Tuple { span, .. } => {
                return Err(TypeError::TypeNotImplemented {
                    span: *span,
                    form: "a tuple type",
                })
            }
            TypeRef::Arrow { span, .. } => {
                return Err(TypeError::TypeNotImplemented {
                    span: *span,
                    form: "an arrow type",
                })
            }
        };
        if name == "Array" {
            let [argument] = args.as_slice() else {
                return Err(TypeError::UnsupportedElementType {
                    span,
                    name: "Array".to_owned(),
                });
            };
            let inner = self.resolve(argument)?;
            return Elem::of(inner).map(Type::Array).ok_or_else(|| {
                TypeError::UnsupportedElementType {
                    span: argument.span(),
                    name: inner.name().to_owned(),
                }
            });
        }
        if !args.is_empty() {
            return Err(TypeError::UnknownType {
                span,
                name: name.clone(),
            });
        }
        match name.as_str() {
            "ZZ32" => Ok(Type::ZZ32),
            "ZZ64" => Ok(Type::ZZ64),
            "RR64" => Ok(Type::RR64),
            "Boolean" => Ok(Type::Boolean),
            "String" => Ok(Type::String),
            other => {
                if let Some((interned, _)) = self.traits.get_key_value(other) {
                    return Ok(Type::Trait(interned));
                }
                if let Some((interned, _)) = self.objects.get_key_value(other) {
                    return Ok(Type::Object(interned));
                }
                Err(TypeError::UnknownType {
                    span,
                    name: name.clone(),
                })
            }
        }
    }
```

The `Unit` arm is a placeholder. Task 3 replaces it with `Ok(Type::Void)`.

- [ ] **Step 7: Update `mono::ty` and the mangler**

In `crates/types/src/mono.rs`, replace `fn ty` (line 221 onward). Non-`Named` forms substitute structurally and are never an instantiation request:

```rust
    fn ty(&mut self, t: &TypeRef, subst: &Subst) -> Result<TypeRef, TypeError> {
        let (name, args, span) = match t {
            TypeRef::Named { name, args, span } => (name, args, *span),
            TypeRef::Unit { .. } => return Ok(t.clone()),
            TypeRef::Tuple { elems, span } => {
                let mut out = Vec::with_capacity(elems.len());
                for e in elems {
                    out.push(self.ty(e, subst)?);
                }
                return Ok(TypeRef::Tuple {
                    elems: out,
                    span: *span,
                });
            }
            TypeRef::Arrow { from, to, span } => {
                return Ok(TypeRef::Arrow {
                    from: Box::new(self.ty(from, subst)?),
                    to: Box::new(self.ty(to, subst)?),
                    span: *span,
                })
            }
        };

        if args.is_empty() {
            if let Some(replacement) = subst.get(name) {
                return Ok(replacement.clone());
            }
            if self.generics.contains_key(name) {
                return Err(TypeError::StaticArgumentsRequired {
                    span,
                    name: name.clone(),
                });
            }
            return Ok(t.clone());
        }

        let mut expanded = Vec::with_capacity(args.len());
        for a in args {
            expanded.push(self.ty(a, subst)?);
        }
        if BUILTIN_CONSTRUCTORS.contains(&name.as_str()) {
            return Ok(TypeRef::Named {
                name: name.clone(),
                args: expanded,
                span,
            });
        }
        if !self.generics.contains_key(name) {
            return Err(TypeError::UnknownType {
                span,
                name: name.clone(),
            });
        }
        let mangled = mangle_static(name, &expanded);
        self.request(name, expanded, &mangled, span);
        Ok(TypeRef::Named {
            name: mangled,
            args: Vec::new(),
            span,
        })
    }
```

Replace `mangle_static` (line 519 onward). It stays injective for the same reason it already was, and `$` cannot appear in a source identifier, so `$unit` cannot collide with a user name:

```rust
#[must_use]
pub fn mangle_static(name: &str, args: &[TypeRef]) -> String {
    if args.is_empty() {
        return name.to_owned();
    }
    let mut out = String::from(name);
    for a in args {
        out.push('$');
        out.push_str(&mangle_type(a));
    }
    out.push_str("$e");
    out
}

fn mangle_type(t: &TypeRef) -> String {
    match t {
        TypeRef::Named { name, args, .. } => mangle_static(name, args),
        TypeRef::Unit { .. } => "$unit".to_owned(),
        TypeRef::Tuple { elems, .. } => {
            let mut out = String::from("$tuple");
            for e in elems {
                out.push('$');
                out.push_str(&mangle_type(e));
            }
            out.push_str("$e");
            out
        }
        TypeRef::Arrow { from, to, .. } => {
            format!("$arrow${}${}$e", mangle_type(from), mangle_type(to))
        }
    }
}
```

- [ ] **Step 8: Update `supertrait`**

In `crates/types/src/lib.rs:199-207`:

```rust
    fn supertrait(&self, reference: &TypeRef) -> Checked<&'static str> {
        match self.registry.resolve(reference)? {
            Type::Trait(name) => Ok(name),
            _ => Err(TypeError::NotATrait {
                span: reference.span(),
                name: reference.written(),
            }),
        }
    }
```

- [ ] **Step 9: Fix every remaining compile error**

```bash
cd fortressc && cargo build --workspace 2>&1 | head -60
```

Every remaining error is a `t.name` / `t.args` / `t.span` field access on what is now an enum. Convert each to a `match` or to `.span()` / `.written()`. Do not add a `name()` accessor that returns `""` for non-`Named` forms — that hides the case a later task has to handle.

- [ ] **Step 10: Run the full suite**

```bash
cd fortressc && cargo test && cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings
```

Expected: 193 passed, 0 failed, plus the 2 new parser tests = 195. fmt and clippy silent.

- [ ] **Step 11: Confirm nothing about the corpus changed**

```bash
cd fortressc && cargo test -p fortress-parser --test corpus -- --nocapture 2>&1 | grep parsed
```

Expected: `parsed 168`. This task is a refactor; a different number means behaviour changed and something is wrong.

- [ ] **Step 12: Commit**

```bash
git checkout -b m3e/unit-tuple-arrow
git add -A fortressc/crates docs/superpowers
git commit -m "refactor(ast): TypeRef becomes an enum

Tuples and arrows are not a name applied to arguments. The enum lands with
only Named reachable; behaviour is unchanged and the corpus stays at 168."
```

---

### Task 2: a void-valued binding becomes a diagnostic

This fixes a defect that exists on `master` today, independent of everything else in the milestone. `x = println(\"hi\")` is accepted by the checker and codegen bails with `internal error: a void expression used as a value`, exit 70 — the driver's code for a compiler bug — on ordinary user source.

**Files:**
- Modify: `crates/types/src/error.rs`
- Modify: `crates/types/src/lib.rs` (the `BlockItem::Binding` arm, around line 1791)
- Test: `crates/types/tests/types.rs`

**Interfaces:**
- Consumes: `TypeError::TypeNotImplemented` from Task 1 (not used here, but the error enum's `span()` and `Display` arms are shared).
- Produces: `TypeError::VoidNotStorable { span, position: &'static str }`.

- [ ] **Step 1: Write the failing test**

In `crates/types/tests/types.rs`, at the end of the file:

```rust
// ------------------------------------------------------- void is not storable

#[test]
fn a_void_valued_binding_is_a_diagnostic_not_an_internal_error() {
    match body_error("f(): ZZ32 = do\n  x = println(\"hi\")\n  0\nend") {
        TypeError::VoidNotStorable { position, .. } => assert_eq!(position, "a binding"),
        other => panic!("expected VoidNotStorable, got {other:?}"),
    }
}

#[test]
fn a_binding_of_a_void_while_is_refused_too() {
    match body_error("f(): ZZ32 = do\n  y: ZZ32 := 0\n  x = while y < 0 do y := y + 1 end\n  0\nend") {
        TypeError::VoidNotStorable { .. } => {}
        other => panic!("expected VoidNotStorable, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cd fortressc && cargo test -p fortress-types --test types void_valued
```

Expected: FAIL to compile — `VoidNotStorable` does not exist.

- [ ] **Step 3: Add the error variant**

In `crates/types/src/error.rs`, after `TypeNotImplemented`:

```rust
    /// `Type::Void` has no representation -- `basic_type` maps it to `None` --
    /// so a position that has to store a value cannot hold one. Reaching
    /// codegen with one is malformed IR, which is exit 70 rather than a
    /// diagnostic, so it is refused here.
    VoidNotStorable {
        span: Span,
        position: &'static str,
    },
```

Add it to the `span()` chain and to `Display`:

```rust
            Self::VoidNotStorable { position, .. } => {
                write!(f, "`()` has no value, so it cannot be stored in {position}")
            }
```

- [ ] **Step 4: Guard the binding**

In `crates/types/src/lib.rs`, in the `BlockItem::Binding(b)` arm (around line 1791), after the value has been checked and before it is pushed into `typed`, add:

```rust
                    if value.ty == Type::Void {
                        return Err(TypeError::VoidNotStorable {
                            span: b.span,
                            position: "a binding",
                        });
                    }
```

Read the surrounding arm first: the local holding the checked right-hand side may not be called `value`. Use whatever it is called there, and place the guard after the declared-type check so an explicit annotation mismatch still wins.

- [ ] **Step 5: Run the tests**

```bash
cd fortressc && cargo test -p fortress-types --test types void_valued
```

Expected: PASS, both.

- [ ] **Step 6: Prove the defect is actually gone through the driver**

```bash
cd fortressc && cat > /tmp/v1.fss <<'EOF'
component v1
export Executable
run(): ZZ32 = do
  x = println("hi")
  0
end
end
EOF
./target/debug/fortressc /tmp/v1.fss -o /tmp/v1; echo "exit=$?"
```

Expected: a diagnostic naming `()` and **exit 1**. Before this task it was `internal error: a void expression used as a value` and exit 70. Record both numbers in the commit message.

- [ ] **Step 7: Full suite, then commit**

```bash
cd fortressc && cargo test && cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings
git add -A fortressc/crates
git commit -m "fix(types): a void-valued binding is a diagnostic, not exit 70

x = println(\"hi\") typechecked and then failed in codegen with
'a void expression used as a value', exit 70 -- the driver's code for a
compiler bug -- on ordinary source. Now VoidNotStorable, exit 1."
```

---

### Task 3: `()` in type position, and the three storage guards it opens

`()` becomes nameable. That immediately opens parameter, field and array-element positions, all of which would build a broken signature or layout, so the guards land in the same task.

**Files:**
- Modify: `crates/parser/src/lib.rs` (`type_ref`)
- Modify: `crates/types/src/registry.rs` (the `Unit` arm)
- Modify: `crates/types/src/lib.rs` (parameter and field checking)
- Test: `crates/parser/tests/parser.rs`, `crates/types/tests/types.rs`

**Interfaces:**
- Consumes: `TypeRef::Unit { span }` and `TypeError::VoidNotStorable { span, position }` from Tasks 1 and 2.
- Produces: `()` resolving to `fortress_types::Type::Void`.

- [ ] **Step 1: Write the failing tests**

In `crates/parser/tests/parser.rs`:

```rust
#[test]
fn empty_parentheses_are_the_unit_type() {
    match return_type("f(): () = println(\"hi\")") {
        fortress_ast::TypeRef::Unit { .. } => {}
        other => panic!("expected the unit type, got {other:?}"),
    }
}
```

In `crates/types/tests/types.rs`:

```rust
#[test]
fn the_unit_type_resolves_to_void() {
    let c = typed("component t\nf(): () = println(\"hi\")\nend\n");
    assert_eq!(c.functions[0].return_type, Type::Void);
}

#[test]
fn a_unit_parameter_is_refused() {
    match type_error("component t\nf(x: ()): ZZ32 = 1\nend\n") {
        TypeError::VoidNotStorable { position, .. } => assert_eq!(position, "a parameter"),
        other => panic!("expected VoidNotStorable, got {other:?}"),
    }
}

#[test]
fn a_unit_field_is_refused() {
    match type_error("component t\nobject O(x: ()) end\nf(): ZZ32 = 1\nend\n") {
        TypeError::VoidNotStorable { position, .. } => assert_eq!(position, "a field"),
        other => panic!("expected VoidNotStorable, got {other:?}"),
    }
}
```

An object's value parameters may resolve through the same code path as a
function's, in which case this reports `"a parameter"`. Both diagnostics are
correct. Match the assertion to whichever path actually fires; do not contort
the code to satisfy the string.

```rust

#[test]
fn a_unit_array_element_is_refused() {
    match type_error("component t\nf(): ZZ32 = do\n  a: Array[\\()\\] = array(1)\n  1\nend\nend\n") {
        TypeError::VoidNotStorable { position, .. } => assert_eq!(position, "an array element"),
        other => panic!("expected VoidNotStorable, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run them to verify they fail**

```bash
cd fortressc && cargo test -p fortress-parser --test parser empty_parentheses
cd fortressc && cargo test -p fortress-types --test types unit_
```

Expected: the parser test fails with a parse error `expected a type name, found LParen`; the types tests fail the same way.

- [ ] **Step 3: Parse `()`**

In `crates/parser/src/lib.rs`, at the top of `type_ref`, before the `identifier` call:

```rust
        if self.at(&Kind::LParen) {
            let start = self.expect(&Kind::LParen, "`(`")?.span.start;
            self.skip_newlines();
            let end = self.expect(&Kind::RParen, "`)`")?.span.end;
            return Ok(TypeRef::Unit {
                span: Span::new(start, end),
            });
        }
```

This is deliberately the narrow version: only `()`. Task 4 turns it into the full parenthesised form.

- [ ] **Step 4: Resolve it**

In `crates/types/src/registry.rs`, replace the placeholder `Unit` arm from Task 1:

```rust
            TypeRef::Unit { .. } => return Ok(Type::Void),
```

- [ ] **Step 5: Guard the parameter, field and element positions**

In `crates/types/src/lib.rs`, wherever a `Param`'s type is resolved into a `TypedParam` (search `TypedParam {`), and wherever a `FieldDecl`'s type is resolved into a `TypedField` (search `TypedField {`), add immediately after the `resolve`:

```rust
            if ty == Type::Void {
                return Err(TypeError::VoidNotStorable {
                    span: p.ty.span(),
                    position: "a parameter",
                });
            }
```

with `"a field"` and the field's span in the field case.

For the array element, `Elem::of(Type::Void)` already returns `None` and the existing code reports `UnsupportedElementType`, which names the wrong cause. In `crates/types/src/registry.rs`'s `Array` branch, before the `Elem::of` call:

```rust
            if inner == Type::Void {
                return Err(TypeError::VoidNotStorable {
                    span: argument.span(),
                    position: "an array element",
                });
            }
```

- [ ] **Step 6: Run the tests**

```bash
cd fortressc && cargo test -p fortress-parser --test parser && cargo test -p fortress-types --test types
```

Expected: all PASS.

- [ ] **Step 7: Measure the corpus**

```bash
cd fortressc && cargo test -p fortress-parser --test corpus -- --nocapture 2>&1 | grep parsed
```

Expected: roughly **298**. The spike measured 298 for `()` plus the parenthesised form; the narrow `()`-only version here will be at or just below that. Record the actual number in the commit message — do not round it to the prediction.

- [ ] **Step 8: Full suite, then commit**

```bash
cd fortressc && cargo test && cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings
git add -A fortressc/crates
git commit -m "feat(types): () is the unit type, resolving to Type::Void

And the three storage positions it opens are refused: a parameter, a field
and an array element. basic_type(Void) is None, so any of them would have
built a broken signature rather than reported anything."
```

---

### Task 4: `(A)` is `A`

**Files:**
- Modify: `crates/parser/src/lib.rs` (`type_ref`)
- Test: `crates/parser/tests/parser.rs`

**Interfaces:**
- Consumes: the `LParen` branch from Task 3.
- Produces: nothing new. A parenthesised single type returns the inner `TypeRef` with its span widened to the parentheses.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn a_parenthesised_type_is_the_type_itself() {
    match return_type("f(): (ZZ32) = 1") {
        fortress_ast::TypeRef::Named { name, .. } => assert_eq!(name, "ZZ32"),
        other => panic!("expected the inner named type, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run it to verify it fails**

```bash
cd fortressc && cargo test -p fortress-parser --test parser a_parenthesised_type
```

Expected: FAIL — Task 3's branch demands `)` immediately after `(`.

- [ ] **Step 3: Widen the branch**

Replace the `LParen` branch added in Task 3:

```rust
        if self.at(&Kind::LParen) {
            let start = self.expect(&Kind::LParen, "`(`")?.span.start;
            self.skip_newlines();
            if self.at(&Kind::RParen) {
                let end = self.expect(&Kind::RParen, "`)`")?.span.end;
                return Ok(TypeRef::Unit {
                    span: Span::new(start, end),
                });
            }
            let inner = self.type_ref()?;
            self.skip_newlines();
            let end = self.expect(&Kind::RParen, "`)`")?.span.end;
            return Ok(match inner {
                TypeRef::Named { name, args, .. } => TypeRef::Named {
                    name,
                    args,
                    span: Span::new(start, end),
                },
                TypeRef::Unit { .. } => TypeRef::Unit {
                    span: Span::new(start, end),
                },
                TypeRef::Tuple { elems, .. } => TypeRef::Tuple {
                    elems,
                    span: Span::new(start, end),
                },
                TypeRef::Arrow { from, to, .. } => TypeRef::Arrow {
                    from,
                    to,
                    span: Span::new(start, end),
                },
            });
        }
```

- [ ] **Step 4: Run the tests and measure**

```bash
cd fortressc && cargo test -p fortress-parser && cargo test -p fortress-parser --test corpus -- --nocapture 2>&1 | grep parsed
```

Expected: tests PASS, corpus **298**.

- [ ] **Step 5: Commit**

```bash
git add -A fortressc/crates
git commit -m "feat(parser): a parenthesised type is that type

Specification 1.0 basic/types-vals-vars.tex:260 -- a tuple is two or more.
Folding (A) into the tuple case would be a silent type error."
```

---

### Task 5: tuple types, parsed and refused

**Files:**
- Modify: `crates/parser/src/lib.rs` (`type_ref`)
- Test: `crates/parser/tests/parser.rs`, `crates/types/tests/types.rs`

**Interfaces:**
- Consumes: `TypeRef::Tuple { elems, span }` and `TypeError::TypeNotImplemented` from Task 1.
- Produces: a `Tuple` reachable from source for the first time.

- [ ] **Step 1: Write the failing tests**

In `crates/parser/tests/parser.rs`:

```rust
#[test]
fn two_or_more_types_in_parentheses_are_a_tuple() {
    match return_type("f(): (ZZ32, String) = 1") {
        fortress_ast::TypeRef::Tuple { elems, .. } => {
            assert_eq!(elems.len(), 2);
            assert_eq!(elems[0].written(), "ZZ32");
            assert_eq!(elems[1].written(), "String");
        }
        other => panic!("expected a tuple type, got {other:?}"),
    }
}
```

In `crates/types/tests/types.rs`:

```rust
#[test]
fn a_tuple_type_is_refused_with_a_diagnostic() {
    match type_error("component t\nf(): (ZZ32, String) = 1\nend\n") {
        TypeError::TypeNotImplemented { form, .. } => assert_eq!(form, "a tuple type"),
        other => panic!("expected TypeNotImplemented, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run them to verify they fail**

```bash
cd fortressc && cargo test -p fortress-parser --test parser two_or_more
cd fortressc && cargo test -p fortress-types --test types a_tuple_type_is_refused
```

Expected: both FAIL with a parse error at the comma.

- [ ] **Step 3: Parse the comma-separated form**

In the `LParen` branch, replace `let inner = self.type_ref()?;` and what follows it with:

```rust
            let mut elems = vec![self.type_ref()?];
            self.skip_newlines();
            while self.at(&Kind::Comma) {
                self.pos += 1;
                self.skip_newlines();
                elems.push(self.type_ref()?);
                self.skip_newlines();
            }
            let end = self.expect(&Kind::RParen, "`)`")?.span.end;
            if elems.len() == 1 {
                return Ok(match elems.remove(0) {
                    TypeRef::Named { name, args, .. } => TypeRef::Named {
                        name,
                        args,
                        span: Span::new(start, end),
                    },
                    TypeRef::Unit { .. } => TypeRef::Unit {
                        span: Span::new(start, end),
                    },
                    TypeRef::Tuple { elems, .. } => TypeRef::Tuple {
                        elems,
                        span: Span::new(start, end),
                    },
                    TypeRef::Arrow { from, to, .. } => TypeRef::Arrow {
                        from,
                        to,
                        span: Span::new(start, end),
                    },
                });
            }
            return Ok(TypeRef::Tuple {
                elems,
                span: Span::new(start, end),
            });
```

`elems` must be `let mut elems` for the `remove(0)`. The invariant that `Tuple`
holds two or more is enforced right here and nowhere else.

- [ ] **Step 4: Run the tests and measure**

```bash
cd fortressc && cargo test -p fortress-parser && cargo test -p fortress-types
cd fortressc && cargo test -p fortress-parser --test corpus -- --nocapture 2>&1 | grep parsed
```

Expected: tests PASS, corpus **303**.

- [ ] **Step 5: Commit**

```bash
git add -A fortressc/crates
git commit -m "feat(parser): tuple types parse, and the checker refuses them

Type stays Copy. A tuple is structurally recursive and making it a real
type means the subtype relation and M3c's dispatch matrix, which is a
milestone of its own."
```

---

### Task 6: arrow types, parsed and refused

`->` is not a token. It is `Minus` followed by `Gt`, and the parser accepts the pair only when they are glued, which is this parser's existing idiom for deciding meaning from adjacency (`crates/parser/src/lib.rs:9-11`). The lexer is not touched, so the 1780 lexer ratchet is not at risk.

**Files:**
- Modify: `crates/parser/src/lib.rs` (split `type_ref` into `type_ref` / `type_atom`)
- Test: `crates/parser/tests/parser.rs`, `crates/types/tests/types.rs`

**Interfaces:**
- Consumes: `TypeRef::Arrow { from, to, span }` from Task 1.
- Produces: `fn type_atom(&mut self) -> Parsed<TypeRef>` — everything Tasks 3–5 built moves into it; `type_ref` becomes the arrow layer above it and stays the entry point every existing caller uses.

- [ ] **Step 1: Write the failing tests**

In `crates/parser/tests/parser.rs`. `decl_error` lands here rather than with the
other helpers in Task 1, because its only caller is the last test below and an
unused function fails `clippy -D warnings`:

```rust
/// The parse error from a whole declaration, for grammar cases that cannot be
/// expressed as a bare expression.
fn decl_error(decl: &str) -> ParseError {
    let src = format!("component t\n{decl}\nend\n");
    let tokens = fortress_lexer::lex(&src).unwrap_or_else(|e| panic!("lex failed: {e}"));
    match parse(&tokens) {
        Ok(_) => panic!("expected {decl:?} to fail to parse"),
        Err(e) => e,
    }
}

#[test]
fn a_glued_minus_greater_is_an_arrow_type() {
    match return_type("f(): ZZ32 -> String = 1") {
        fortress_ast::TypeRef::Arrow { from, to, .. } => {
            assert_eq!(from.written(), "ZZ32");
            assert_eq!(to.written(), "String");
        }
        other => panic!("expected an arrow type, got {other:?}"),
    }
}

#[test]
fn arrow_types_are_right_associative() {
    assert_eq!(
        return_type("f(): ZZ32 -> String -> Boolean = 1").written(),
        "ZZ32 -> String -> Boolean"
    );
    match return_type("f(): ZZ32 -> String -> Boolean = 1") {
        fortress_ast::TypeRef::Arrow { to, .. } => match *to {
            fortress_ast::TypeRef::Arrow { .. } => {}
            other => panic!("expected the right side to be the nested arrow, got {other:?}"),
        },
        other => panic!("expected an arrow type, got {other:?}"),
    }
}

#[test]
fn a_spaced_minus_greater_is_not_an_arrow() {
    let e = decl_error("f(): ZZ32 - > String = 1");
    assert!(matches!(e, ParseError::UnexpectedToken { .. }), "got {e:?}");
}
```

In `crates/types/tests/types.rs`:

```rust
#[test]
fn an_arrow_type_is_refused_with_a_diagnostic() {
    match type_error("component t\nf(): ZZ32 -> String = 1\nend\n") {
        TypeError::TypeNotImplemented { form, .. } => assert_eq!(form, "an arrow type"),
        other => panic!("expected TypeNotImplemented, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run them to verify they fail**

```bash
cd fortressc && cargo test -p fortress-parser --test parser arrow
cd fortressc && cargo test -p fortress-types --test types an_arrow_type
```

Expected: FAIL — the current `type_ref` stops at `ZZ32` and the caller reports `expected ...`, found `Minus`.

- [ ] **Step 3: Split the function**

Rename the existing `type_ref` to `type_atom` — everything Tasks 3, 4 and 5 added stays inside it, and its recursive calls for tuple elements and the parenthesised form become `self.type_ref()` so an arrow can appear inside parentheses. Then add above it:

```rust
    /// `A -> B`, right associative. `->` is not a token: it is `Minus` glued to
    /// `Gt`, decided by span adjacency the same way operator fixity is.
    fn type_ref(&mut self) -> Parsed<TypeRef> {
        let from = self.type_atom()?;
        if !(self.at(&Kind::Minus)
            && self.glued_right(self.pos)
            && matches!(self.peek_ahead(1), Some(Kind::Gt)))
        {
            return Ok(from);
        }
        let start = from.span().start;
        self.pos += 2;
        self.skip_newlines();
        let to = self.type_ref()?;
        let end = to.span().end;
        Ok(TypeRef::Arrow {
            from: Box::new(from),
            to: Box::new(to),
            span: Span::new(start, end),
        })
    }
```

Every existing caller of `type_ref` keeps working unchanged, which is the point of putting the arrow layer on top rather than inside.

- [ ] **Step 4: Run the tests and measure**

```bash
cd fortressc && cargo test -p fortress-parser && cargo test -p fortress-types
cd fortressc && cargo test -p fortress-parser --test corpus -- --nocapture 2>&1 | grep parsed
cd fortressc && cargo test -p fortress-lexer --test corpus -- --nocapture 2>&1 | grep lex
```

Expected: tests PASS, parser corpus **314**, lexer corpus **1780 unchanged**. If the lexer number moved, something touched the lexer and it should not have.

- [ ] **Step 5: Commit**

```bash
git add -A fortressc/crates
git commit -m "feat(parser): arrow types parse, and the checker refuses them

Glued Minus+Gt in type position, no lexer change, so the 1780 lexer
ratchet is untouched. Arrows are uninhabited here -- there are no lambdas
and no function values -- so refusing them is the honest signal."
```

---

### Task 7: `()` as an expression, end to end

**Files:**
- Modify: `crates/ast/src/nodes.rs` (`Expr`)
- Modify: `crates/parser/src/lib.rs` (`primary`, the `Kind::LParen` arm around line 972)
- Modify: `crates/types/src/types.rs` (`TypedExprKind`)
- Modify: `crates/types/src/lib.rs` (the `Expr::Unit` arm)
- Modify: `crates/codegen/src/lib.rs`
- Create: `tests/unitvoid.fss`
- Test: `crates/parser/tests/parser.rs`, `crates/types/tests/types.rs`, `crates/driver/tests/end_to_end.rs`

**Interfaces:**
- Consumes: `Type::Void`, already resolvable as `()` since Task 3.
- Produces: `Expr::Unit { span }` in the AST; `TypedExprKind::Unit` in the typed AST.

- [ ] **Step 1: Write the failing tests**

In `crates/types/tests/types.rs`:

```rust
#[test]
fn the_unit_expression_has_type_void() {
    let b = body("f(): () = ()");
    assert_eq!(b.ty, Type::Void);
    assert!(matches!(b.kind, TypedExprKind::Unit), "got {:?}", b.kind);
}

#[test]
fn a_unit_binding_is_refused() {
    match body_error("f(): ZZ32 = do\n  x: () = ()\n  0\nend") {
        TypeError::VoidNotStorable { position, .. } => assert_eq!(position, "a binding"),
        other => panic!("expected VoidNotStorable, got {other:?}"),
    }
}
```

Create `tests/unitvoid.fss` (repository root `tests/`, alongside `fact.fss`):

```
component unitvoid
export Executable

greet(): () = println("hello from a void function")

run(): () = greet()

end
```

In `crates/driver/tests/end_to_end.rs`:

```rust
#[test]
fn a_void_function_compiles_links_and_runs() {
    let binary = compile_fixture("unitvoid.fss", "unitvoid");
    let out = run(&binary);
    assert!(out.status.success(), "exited {:?}", out.status);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "hello from a void function\n"
    );
}
```

- [ ] **Step 2: Run them to verify they fail**

```bash
cd fortressc && cargo test -p fortress-types --test types unit_expression
cd fortressc && cargo test -p fortress-driver --test end_to_end a_void_function
```

Expected: FAIL — `expected an expression, found RParen`.

- [ ] **Step 3: Add the AST node**

In `crates/ast/src/nodes.rs`, in the `Expr` enum:

```rust
    /// `()`. The one value of the unit type.
    Unit { span: Span },
```

Add it to `Expr::span()`'s match.

- [ ] **Step 4: Parse it**

In `crates/parser/src/lib.rs`, in `primary`'s `Kind::LParen` arm, immediately after `self.pos += 1; self.skip_newlines();`:

```rust
                if self.at(&Kind::RParen) {
                    let close = self.expect(&Kind::RParen, "`)`")?.span;
                    return Ok(Expr::Unit {
                        span: Span::new(span.start, close.end),
                    });
                }
```

`span` is already bound in that arm to the `(`'s span. Do not touch the glued-`(` application rule at `lib.rs:775-778` — `f()` is a call with zero arguments and reaches `args()`, never `primary`.

- [ ] **Step 5: Add the typed node and check it**

In `crates/types/src/types.rs`, in `TypedExprKind`:

```rust
    /// `()`. Typed as `Void`, and lowers to no value at all.
    Unit,
```

In `crates/types/src/lib.rs`, in the expression checker's match on `Expr`:

```rust
            Expr::Unit { span } => {
                self.require(Type::Void, expected, *span)?;
                Ok(TypedExpr {
                    kind: TypedExprKind::Unit,
                    ty: Type::Void,
                    span: *span,
                })
            }
```

Follow the shape of the existing `while` arm at `crates/types/src/lib.rs:1828-1836`, which does the same `require(Type::Void, ...)` before returning.

- [ ] **Step 6: Lower it**

In `crates/codegen/src/lib.rs`, in the match on `TypedExprKind`:

```rust
            TypedExprKind::Unit => Ok(None),
```

Match the arm's actual return type — the surrounding function returns `Result<Option<BasicValueEnum>, CodegenError>` or equivalent. A void call already yields `None` at `lib.rs:1019`; this is the same shape.

- [ ] **Step 7: Run the tests and measure**

```bash
cd fortressc && cargo test
cd fortressc && cargo test -p fortress-parser --test corpus -- --nocapture 2>&1 | grep parsed
```

Expected: all PASS, parser corpus **418**.

- [ ] **Step 8: Commit**

```bash
git add -A fortressc/crates tests/unitvoid.fss
git commit -m "feat: () is an expression, and run():() = () compiles and runs

The most common function shape in the corpus, and it had never compiled."
```

---

### Task 8: tuple expressions, parsed and refused

**Files:**
- Modify: `crates/ast/src/nodes.rs` (`Expr`)
- Modify: `crates/parser/src/lib.rs` (`primary`)
- Modify: `crates/types/src/lib.rs`
- Test: `crates/parser/tests/parser.rs`, `crates/types/tests/types.rs`

**Interfaces:**
- Consumes: `TypeError::TypeNotImplemented` from Task 1.
- Produces: `Expr::Tuple { items: Vec<Expr>, span: Span }`.

- [ ] **Step 1: Write the failing tests**

In `crates/parser/tests/parser.rs`:

```rust
#[test]
fn a_comma_separated_parenthesised_expression_is_a_tuple() {
    match expr("(1, 2)") {
        Expr::Tuple { items, .. } => assert_eq!(items.len(), 2),
        other => panic!("expected a tuple expression, got {other:?}"),
    }
}

#[test]
fn a_single_parenthesised_expression_is_not_a_tuple() {
    match expr("(1)") {
        Expr::Tuple { .. } => panic!("a one-element parenthesised expression is not a tuple"),
        _ => {}
    }
}
```

In `crates/types/tests/types.rs`:

```rust
#[test]
fn a_tuple_expression_is_refused_with_a_diagnostic() {
    match body_error("f(): ZZ32 = do\n  x = (1, 2)\n  0\nend") {
        TypeError::TypeNotImplemented { form, .. } => assert_eq!(form, "a tuple expression"),
        other => panic!("expected TypeNotImplemented, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run them to verify they fail**

```bash
cd fortressc && cargo test -p fortress-parser --test parser tuple
cd fortressc && cargo test -p fortress-types --test types a_tuple_expression
```

Expected: FAIL — `expected )`, found `Comma`.

- [ ] **Step 3: Add the AST node**

In `crates/ast/src/nodes.rs`:

```rust
    /// `(a, b)`. Two or more, by construction.
    Tuple { items: Vec<Expr>, span: Span },
```

Add it to `Expr::span()`.

- [ ] **Step 4: Parse it**

In `primary`'s `Kind::LParen` arm, after the existing `let inner = self.expr()?; self.skip_newlines();`:

```rust
                if self.at(&Kind::Comma) {
                    let mut items = vec![inner];
                    while self.at(&Kind::Comma) {
                        self.pos += 1;
                        self.skip_newlines();
                        items.push(self.expr()?);
                        self.skip_newlines();
                    }
                    let close = self.expect(&Kind::RParen, "`)`")?.span;
                    return Ok(Expr::Tuple {
                        items,
                        span: Span::new(span.start, close.end),
                    });
                }
```

- [ ] **Step 5: Refuse it**

In `crates/types/src/lib.rs`, in the expression checker:

```rust
            Expr::Tuple { span, .. } => Err(TypeError::TypeNotImplemented {
                span: *span,
                form: "a tuple expression",
            }),
```

- [ ] **Step 6: Run the tests and measure**

```bash
cd fortressc && cargo test
cd fortressc && cargo test -p fortress-parser --test corpus -- --nocapture 2>&1 | grep parsed
```

Expected: all PASS, parser corpus **428**.

- [ ] **Step 7: Commit**

```bash
git add -A fortressc/crates
git commit -m "feat(parser): tuple expressions parse, and the checker refuses them"
```

---

### Task 9: the gate

**Files:**
- Create: `tools/unit-gate.sh`
- Create: `tests/badvoidparam.fss`, `tests/badtupletype.fss`, `tests/badarrowtype.fss`, `tests/badtupleexpr.fss`

**Interfaces:**
- Consumes: everything Tasks 1–8 produced.
- Produces: `./tools/unit-gate.sh`, `./tools/unit-gate.sh --selftest`, `./tools/unit-gate.sh --mutate`.

- [ ] **Step 1: Create the negative fixtures**

`tests/badvoidparam.fss`:

```
component badvoidparam
export Executable

f(x: ()): ZZ32 = 1

run(): () = ()

end
```

`tests/badtupletype.fss`:

```
component badtupletype
export Executable

f(): (ZZ32, String) = 1

run(): () = ()

end
```

`tests/badarrowtype.fss`:

```
component badarrowtype
export Executable

f(): ZZ32 -> String = 1

run(): () = ()

end
```

`tests/badtupleexpr.fss`:

```
component badtupleexpr
export Executable

run(): () = do
  x = (1, 2)
  println("unreachable")
end

end
```

Four negative fixtures, because the design lists four refusals and a gate that
checks two of them is green on a compiler that lost the other two.

- [ ] **Step 2: Write the gate**

Create `tools/unit-gate.sh`, `chmod +x`. Model it on `tools/dispatch-gate.sh`, which is the one that already has `--mutate`. The header, the `repo`/`build`/`fortressc` variables, the three `export`s, and the `ok`/`bad` counters are copied from it verbatim.

Structure, in the order `main` runs them:

```bash
selftest()   # prove the assertions can refuse, before anything is compiled
preflight()  # cargo build --workspace, rm -rf $build, mkdir -p $build
compile()    # unitvoid.fss compiles and links
runs()       # unitvoid prints exactly "hello from a void function"
refusals()   # badvoidparam, badtupletype, badarrowtype, badtupleexpr:
             # each exits 1, and stderr carries the expected phrase
parens()     # (ZZ32) and ZZ32 resolve the same
```

`selftest` must prove each assertion can say no. At minimum:

```bash
refused_cleanly() { [[ $1 -eq 1 ]]; }

selftest() {
    printf '== gate self test ==\n'

    if refused_cleanly 1; then ok 'exit 1 is a clean refusal'
    else bad 'exit 1 is a clean refusal'; fi

    for status in 0 70 124 139; do
        if refused_cleanly "$status"; then
            bad "status $status is refused as a clean refusal" 'only exit 1 is a diagnostic'
        else
            ok "status $status is refused as a clean refusal"
        fi
    done
}
```

70 is the internal-error code and it matters here more than anywhere: the whole point of Task 2 and Task 3's guards is that these programs exit 1 and not 70. A gate that accepts any nonzero status would pass on the bug this milestone fixes.

`parens()` compares two compilations rather than trusting one. Compile a program declaring `f(): (ZZ32) = 1` and one declaring `f(): ZZ32 = 1` with `--emit-ir`, and assert the two IR files are identical apart from the module name. If `(A)` were folded into `Tuple`, the first would not compile at all.

- [ ] **Step 3: Run the gate's self test alone**

```bash
./tools/unit-gate.sh --selftest
```

Expected: every assertion `ok`, and the summary shows `N/0`.

- [ ] **Step 4: Run the gate**

```bash
./tools/unit-gate.sh
```

Expected: `N/0` and exit 0.

- [ ] **Step 5: Add the mutation suite**

Copy the `MUTATIONS` array and the `mutate()` function from `tools/dispatch-gate.sh:288-345`, changing only the array and the list of check functions it re-runs. The three mutations:

Each entry is `file|from|to|label`, split on `|`, so no field may contain a
pipe. Each `from` **must match exactly once** in its file or the gate prints
`the mutation pattern is not unique` and refuses to run.

The three targets, and what each one is meant to prove:

1. **Drop the void parameter guard.** Target the guard Task 3 added in
   `crates/types/src/lib.rs`, next to `position: "a parameter"`. Mutate its
   condition so it never fires. Proves `badvoidparam.fss` would otherwise reach
   codegen and produce a broken signature.
2. **Fold `(A)` into `Tuple`.** Target `if elems.len() == 1 {` in
   `crates/parser/src/lib.rs`, mutate to `if false {`. Proves `parens()` is
   really comparing two compilations and not just checking one succeeded.
3. **Accept a tuple type.** Target the `TypeRef::Tuple` arm in
   `crates/types/src/registry.rs`, mutate the `return Err(...)` to
   `return Ok(Type::ZZ32)` so a tuple type silently becomes something else.
   Proves `refusals()` checks the exit status rather than only that the driver
   said something.

Before writing each entry, confirm uniqueness:

```bash
grep -F -c -- 'if elems.len() == 1 {' fortressc/crates/parser/src/lib.rs
```

Expected: `1`. If a string appears more than once, extend it with surrounding
context until it does not. Then write the array, using the exact strings you
verified:

```bash
MUTATIONS=(
  'crates/parser/src/lib.rs|if elems.len() == 1 {|if false {|fold a one-element parenthesised type into Tuple'
  # ... plus the two above, with the exact `from` strings grep confirmed unique
)
```

- [ ] **Step 6: Run the mutation suite and record what happened**

```bash
./tools/unit-gate.sh --mutate
```

Expected: `mutations: 3 run, 0 survived, 0 could not be applied`, and each mutation shows `REFUSED` with at least one failed check. **Write down which check caught each one and what the driver's exit code was.** A mutation that survives means the gate does not test what it claims; fix the gate, not the mutation.

- [ ] **Step 7: Confirm the tree is clean and rebuild**

```bash
git status --porcelain fortressc/crates   # must be empty
cd fortressc && cargo build --workspace
```

- [ ] **Step 8: Commit**

```bash
git add -A tools tests
git commit -m "test(m3e): the unit gate, with a mutation suite

Three mutations, all refused: dropping the void parameter guard, folding
(A) into Tuple, and accepting a tuple type. The self test refuses exit 70
specifically -- the bug this milestone fixes exits 70."
```

---

### Task 10: ratchets and the record

**Files:**
- Modify: `crates/parser/tests/corpus.rs:106-114`
- Modify: `ROADMAP.md:52-56`
- Modify: `04-state.md`
- Modify: `docs/superpowers/specs/2026-08-19-m3e-unit-tuple-arrow-design.md` (status line)

**Interfaces:**
- Consumes: the measured corpus number from Task 8.

- [ ] **Step 1: Measure both corpora one final time**

```bash
cd fortressc && cargo test -p fortress-parser --test corpus -- --nocapture 2>&1 | grep -E "parsed|lex cleanly"
cd fortressc && cargo test -p fortress-lexer --test corpus -- --nocapture 2>&1 | grep "lex cleanly"
```

Record both numbers. Expected: parser 428, lexer 1780.

- [ ] **Step 2: Raise the parser ratchet**

In `crates/parser/tests/corpus.rs`, replace the comment and assertion at lines 106-114 with the measured number:

```rust
    // The same ratchet. The lexer pass took this from 84 to 154 by adding
    // `import`, the headerless-file production and the tokens above it; M3d's
    // static parameters took it to 168; M3e's `()` took it to 428, of which the
    // unit type alone was 232.
    assert!(
        parsed >= 428,
        "parser corpus regressed: {parsed} files parse, floor is 428"
    );
```

Use whatever Step 1 actually printed, not 428, if they differ.

- [ ] **Step 3: Verify the ratchet refuses**

```bash
cd fortressc && sed -i 's/parsed >= 428/parsed >= 9999/' crates/parser/tests/corpus.rs
cargo test -p fortress-parser --test corpus 2>&1 | tail -5
git checkout -- crates/parser/tests/corpus.rs
```

Expected: the test FAILS with `parser corpus regressed`. A ratchet that has never refused is not a ratchet. Then re-apply Step 2's edit.

- [ ] **Step 4: Update the roadmap**

In `ROADMAP.md`, the "What is in front now" paragraph at lines 52-56 currently says tuple and arrow types are the top parser blocker. Replace it with what was measured:

```markdown
**What is in front now**, in rough order of what the corpus is waiting on:
`getter`/`setter` (126 files) and `opr` declarations (79), then enclosing
operators, which need the precedence map that `<|`, `|>` and `|x|` were
tokenised without.

M3e landed the unit type and syntax for tuples and arrows, and it is the third
time the blocker histogram has pointed at the wrong thing. "Tuple and arrow
types" was billed as the top blocker at 536 files. 485 of those 536 were `()`,
the unit type. Measured by construct: unit +232, tuples +15, arrows +13, for a
parser total of 168 to 428. Details in
`specs/2026-08-19-m3e-unit-tuple-arrow-design.md`.
```

- [ ] **Step 5: Update the state file**

In `04-state.md`, under `fortress_compiler`: set `last_updated`, move M3e from "static-argument inference" to what shipped, rename the inference milestone to M3f, update the test count, the gate list (now six), and the corpus line to `lexer 1780 of 1956 (91.0%), parser 428`.

- [ ] **Step 6: Mark the design document landed**

Change its status line to `Status: **landed**` plus the branch name.

- [ ] **Step 7: Full verification, everything**

```bash
cd fortressc && cargo test && cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings
cd .. && for g in generics dispatch array memory mpi unit; do
  printf '%-10s ' "$g"; ./tools/$g-gate.sh >/dev/null 2>&1 && echo PASS || echo FAIL
done
```

Expected: tests green, fmt and clippy silent, all six gates PASS. The five pre-existing gates must be unchanged — if any of them moved, this milestone touched something it should not have.

- [ ] **Step 8: Sweep the whole corpus with the real driver**

The parser corpus test stops at the parser, so it cannot see what the 260
newly-parsing files do once they reach the checker. Run the driver over every
file and compare the exit-code profile against the baseline:

```bash
cd /home/prestonalthaus/claude/fortress-lang-rewrite
FC=$PWD/fortressc/target/debug/fortressc
find . -path ./fortressc -prune -o -path ./.git -prune -o \
  \( -name '*.fss' -o -name '*.fsi' \) -print | sort > /tmp/corpus.txt
: > /tmp/sweep.txt
while read -r f; do
  out=$("$FC" "$f" --emit-obj -o /dev/null 2>&1 >/dev/null); status=$?
  printf '%s\t%s\t%s\n' "$status" "$f" "${out//$'\n'/ }" >> /tmp/sweep.txt
done < /tmp/corpus.txt
echo "compile end to end:"; awk -F'\t' '$1==0' /tmp/sweep.txt | wc -l
echo "internal errors (exit 70):"; awk -F'\t' '$1==70' /tmp/sweep.txt | wc -l
awk -F'\t' '$1==70 {print $2": "$3}' /tmp/sweep.txt | head -20
```

Two things to record:

* **How many files compile end to end.** The design document promised this
  number alongside the parse number and expects it to be small.
* **The exit-70 count, which must not grow.** Exit 70 is a compiler bug on user
  source, which the house rules forbid. Task 2 removed one cause of it. If this
  sweep finds new ones, they are panics on paths that only became reachable
  because 260 more files now parse — fix them before finishing the milestone,
  and add a regression test for each. Do not record the number and move on.

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "docs(m3e): raise the parser ratchet to 428 and record the measurement

Third time the blocker histogram pointed at the wrong thing. 485 of the
536-file 'tuple and arrow types' blocker were the unit type. Driver sweep
over all 1956 files recorded, with the exit-70 count checked against
baseline -- 260 more files reach the checker than did before."
```
