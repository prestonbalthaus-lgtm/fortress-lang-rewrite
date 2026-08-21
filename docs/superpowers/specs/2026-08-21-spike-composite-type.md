# SPIKE-COMPOSITE-TYPE — priced, not landed

**Date:** 2026-08-21. **Tree:** master `f81f41ace`, in a scratch copy; the
repository was not written to. **Verdict: INTERN COMPOSITES. `Type` stays
`Copy`.** So tuples, arrow types, dimensions and the numeric tower are a feature
each, not a rewrite of every pass — which is the opposite of what the gap
analysis's own framing feared, and the reason it asked for the measurement
rather than the argument.

The gap analysis (§3.2) calls this *"the biggest unowned decision in the tree"*
and is right that it sits under four decision-4 items at once. It is not a bug:
`types.rs`'s doc comments say `Type` is `Copy` on purpose, and `Elem` is a
separate enum specifically so array-of-array is unrepresentable rather than
merely rejected.

---

## The prototype

A whole-repo scratch copy, one new variant and one new interner mirroring the
`intern()` that already exists at `types.rs:12-24`:

```rust
Tuple(&'static [Type]),
pub fn intern_types(types: &[Type]) -> &'static [Type]   // LazyLock<Mutex<HashSet<..>>>, Box::leak on miss
```

**`Copy` holds, and it is asserted by the compiler rather than claimed:**

```rust
const fn assert_copy<T: Copy>() {}
const _: () = assert_copy::<Type>();
```

A shared reference is `Copy` whatever it points at — that is the whole trick.
`Hash` has to be derived on `Elem` and `Type` (two derive edits) because the
intern table is a `HashSet`.

**I re-ran this myself** rather than taking it on report: the prototype rebuilds
and `types::spike_tuple::interning_dedups_and_type_stays_copy ... ok`, 103 of
103 types tests passing.

---

## What it costs

**Four arms. Two files. `+54/-6`.**

| what | count |
|---|---|
| exhaustive matches on `Type` the compiler forces | **4** — `types.rs` ×3 (`Elem::of`, `Type::name`, `Type::symbol`), `codegen/src/lib.rs` ×1 (`basic_type`) |
| exhaustive matches on `Elem` (measured separately with a dummy variant) | 5 — prices the nested-array follow-on, not the tuple work |
| files touched to make the workspace compile again | **2** |
| **`crates/types/src/lib.rs` (3854 lines), `mono.rs`, `registry.rs`, `error.rs`** | **ZERO changes** |
| new diagnostics needed | **none** — the refusal already exists at exactly one site, `registry.rs:101-107`, `TypeError::TypeNotImplemented { form: "a tuple type" }`. AST `TypeRef::Tuple` never becomes a `Type::Tuple`, so the variant is unconstructable by construction and construction is a single gate |

The census method matters: **ripgrep cannot classify these** — the compiler is
the census. Add the variant, `cargo check --workspace --all-targets`, harvest
`E0004`, patch, re-check. Three rounds, and round 3 was clean.

**The 20 sites that are the real homework.** Non-exhaustive matches the compiler
will never flag — `matches!(t, Type::Array(_))`, `let-else` on one variant, an
`if let` arm — where a real `Tuple` would be silently swallowed rather than
rejected: `crates/types/src/lib.rs` 11, `types.rs` 5, `registry.rs` 3,
`codegen/src/lib.rs` 1. **4 arms are forced; 20 sites have to be read.**

---

## `mono.rs` pays nothing, either way

This is the finding that most changes the shape of the decision. `rg -n '\bType\b'
crates/types/src/mono.rs` returns **one line, and it is a doc comment**.
Monomorphization's substitution is `BTreeMap<String, TypeRef>` — the *AST* type,
already `Vec`/`Box`/`String` and already non-`Copy`. Its 79 clone operations are
whole AST nodes (`template.decl.clone()`, `Member::Method(m.clone())`), and
`Subst` is passed by reference at 10 of 12 sites.

Measured, not reasoned: the non-`Copy` probe produced 162 located errors and
**zero** of them in `mono.rs`. Neither branch of this decision touches
monomorphization at all.

---

## Timing

Full 1956-file sweep, three runs each, same disk, same corpus:

```
BEFORE   files=1956 compiled=291 broken=0    1.07 / 1.08 / 1.08 s
AFTER    files=1956 compiled=291 broken=0    1.08 / 1.07 / 1.09 s
```

**All six TSVs are md5-identical** — per-file exit status *and* per-file IR
sha256 unchanged for all 1956. That is this project's own standard (an
exit-code count once hid a four-file defect); `diff before-1.tsv after-1.tsv` is
empty.

*On the absolute number:* 1.08 s does not reproduce the ~11 s recorded in
`04-state.md`. One `--emit-obj` on `arraysum.fss` is 10 ms, and 1956 × 10 ms ÷ 14
workers = 1.07 s, so it is internally consistent — the scratch copy is on tmpfs.
It does not affect the before/after comparison, which is all this turns on.

---

## What follows

* Tuples, arrow types, `RR32`/`NN32`/`NN64` and dimension exponents can each be
  their own milestone. **None of them is gated on a rewrite.**
* Arbitrary-precision `ZZ` stays a separate spike: it needs a heap
  representation and runtime shims, which interning does not provide.
* **Do the 20 non-exhaustive sites as part of whichever feature lands first**,
  not afterwards. They are where a composite gets swallowed in silence, and
  silence is the class this project ranks worst.
* Note for whoever picks it up: `SPIKE-CLOSURE-REPRESENTATION` landed branch (b)
  the same day, so **arrow types no longer need a `Type` variant at all** — an
  arrow is a generated trait. That removes one of the four items this decision
  was carrying.
