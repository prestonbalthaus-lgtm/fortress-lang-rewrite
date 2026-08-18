# Fortress M1: native arithmetic, no JVM

Date: 2026-08-18
Status: approved

## Why this exists

The goal is feasibility data off real hardware. Can Fortress be compiled natively
while staying Fortress, or does a native implementation force so many semantic
compromises that the result is a new language wearing Fortress syntax?

That question cannot be answered by reading. It needs a program that compiles to
an ELF binary and runs on a compute node.

The JVM is excluded from the toolchain and from the output. That decision also
removes the legacy interpreter as a differential oracle, since it only runs on a
JVM. Correctness for M1 comes from the 1.0 specification and from hand verified
expected output.

## Milestones

**M1. Arithmetic on one node.** Source to ELF for a small subset of Fortress.
Proves the pipeline exists and that the awkward parts of the language survive it.

**M2. MPI hello across two nodes.** A Fortress program links against OpenMPI,
runs under `sbatch`, and reports its rank count. Proves the C ABI, which is the
boundary the JVM implementation could never cross.

Everything past M2 is in `ROADMAP.md`.

## Architecture

One Cargo workspace, six crates. Each is testable on its own.

```
fortressc/
  crates/
    lexer/     logos tokenizer plus the newline and layout layer
    ast/       type definitions only, no logic
    parser/    tokens to AST, recursive descent
    types/     numeric tower and static overload resolution
    codegen/   typed AST to LLVM IR via inkwell
    driver/    CLI, invokes lld, emits the ELF
  tests/       end to end: .fss in, run the binary, compare stdout
```

Interfaces:

| Crate | Signature |
|-------|-----------|
| `lexer` | `&str -> Result<Vec<Token>, LexError>`, tokens carry spans |
| `parser` | `&[Token] -> Result<Component, ParseError>` |
| `types` | `Component -> Result<TypedComponent, TypeError>` |
| `codegen` | `&TypedComponent -> Module` |

`ast` is a separate crate so `parser` and `codegen` never depend on each other.

### The TypedComponent firewall

`TypedComponent` has every operator and every call already resolved to one
concrete target. Codegen never asks which `+` this is, because the answer is
already in the tree.

This is the seam that keeps M1 from becoming a rewrite. Runtime multiple dispatch,
if it is ever added, lands in `types` and codegen barely changes.

## M1 language subset

In scope:

* `component` and `export Executable` and `run`
* function declarations with explicit parameter and return types
* `ZZ32`, `ZZ64`, `RR64`, and their literals
* `widen`
* `+`, `-`, `*`, `/`
* juxtaposition
* comparisons returning `Boolean`
* `if ... then ... else ... end`
* `do ... end`
* local bindings, both inferred and explicitly typed
* recursion
* `println`

Out of scope for M1: traits, objects, generics, arrays, `for`, generators,
parallelism, `atomic`, user defined overloads, Unicode aliases, and syntax
abstraction (which is cut from v1 entirely, see `ROADMAP.md` decision 1).

### Acceptance program

Adapted from `ProjectFortress/demos/fact64.fss`:

```
component fact
export Executable

f(x:ZZ64):ZZ64 = if x < 2 then 1 else x f(x-1) end

run() = do
   j:ZZ64 = widen(20)
   println("fact(20) = " f(j))
end
end
```

This program was chosen for one line. In `println("fact(20) = " f(j))` the
juxtaposition is string concatenation. In `x f(x-1)` the identical syntax is
multiplication. Same construct, different meaning, decided entirely by operand
types and resolved statically in one pass.

A subset that avoided this would be a fake milestone.

## Type rules

**Dispatch is static.** Every overload is resolved at compile time. Runtime
multiple dispatch is not in M1.

**No implicit widening of values.** A `ZZ32` variable used where `ZZ64` is
required is a hard `TypeError`. This matches the reference implementation: the
comment in `fact64.fss` notes that `widen` is required because the interpreter
"otherwise cheerfully multiplies two 32-bit integers giving a 32-bit result, in a
way that would not surprise Kernighan and Ritchie in the least." Refusing implicit
conversion is faithful to Fortress, not a shortcut.

**Literals are unfixed until context pins them.** In `if x < 2 then 1 else ...`
where `x: ZZ64`, the literals `2` and `1` become `ZZ64` literals because that is
what context requires. They are not `ZZ32` values being widened.

These two rules are distinct and must not be conflated. Conflating them either
rejects the acceptance program or reintroduces the implicit casting that is
banned. The negative tests below exist to hold the line.

**Juxtaposition** resolves by operand type: numeric operands multiply, string
operands concatenate. Any other combination is a `TypeError`.

## Memory

M1 has no GC and no ARC. Formatting a `ZZ64` into a string for concatenation
allocates, and that allocation is never freed. This leak is accepted for M1.

It is contained rather than scattered. All concatenation allocation goes through
one centralized allocator function rather than through `malloc` calls emitted
across codegen. When Boehm or ARC lands it is one place to change, and codegen is
untouched.

## Error handling

Three kinds of failure, kept apart.

**User errors** carry spans and render as file, line, column, the source line, and
a caret. One error enum per crate: `LexError`, `ParseError`, `TypeError`. The
driver renders them. No diagnostic crate for M1; `ariadne` or
`codespan-reporting` can wait until error variety justifies the dependency.

M1 fails fast on the first user error. Parser error recovery is real work and buys
nothing while the subset is this small. This is scoped to M1, not permanent.

**Internal errors** are separate and get a distinct exit code, so a test can tell
"your program is wrong" from "the compiler is broken." Bad IR, lld failures, and
inkwell rejecting a module are compiler bugs.

`Module::verify()` runs on every M1 build. It catches malformed IR where it is
generated instead of as a cryptic lld failure several steps later. Gate it behind
a flag once compile time starts to matter.

No `unwrap()` or `panic!` on anything derived from user source. Malformed input is
a diagnostic. This is an architectural rule, not a style preference, and it
matches `03-guidelines.md`.

## Testing

**Unit, per crate.** Lexer token streams, parser AST shapes, type resolution
outcomes. `insta` snapshots for AST and for generated IR, because eyeballing IR
diffs by hand is how regressions get through.

**End to end.** `tests/` holds `.fss` files with expected stdout. Compile, run the
binary, diff. `fact.fss` is the first entry.

**Negative tests that enforce the type rules rather than trusting them.**

1. A `ZZ32` variable passed where `ZZ64` is required must fail with a specific
   `TypeError`. This is what makes "no implicit widening" real.
2. A literal in the same position must succeed, pinning to `ZZ64`. This is what
   proves the two rules stayed distinct.
3. A juxtaposition whose operands resolve to neither multiplication nor
   concatenation must fail cleanly.

**Corpus smoke test.** M1's subset cannot parse the 1846 corpus files, but the
lexer can be pointed at all of them from day one and asserted not to panic. That
is the roadmap's phase 1 exit criterion arriving early for free.

**Leak detection does not run in M1 CI.** This is deliberate, per the memory
section. It is recorded here so it does not silently become policy.

**No JVM in CI.**

## Exit criteria

M1 is done when the acceptance program compiles to an ELF, runs on a compute
node, prints `fact(20) = 2432902008176640000`, and `ldd` on the binary shows libc
and nothing else.

## What M1 does not answer

M1 deliberately avoids the three things most likely to force a redesign. None of
them are resolved by passing M1.

**Symmetric multiple dispatch.** Fortress dispatches on the runtime types of all
arguments. It constrains the object model, vtable layout, and calling convention
together. M1 sidesteps it with static resolution.

**`atomic`.** The legacy implementation used DSTM2, a Java software transactional
memory library. There is no native equivalent to link against. This is the single
most likely feature to be cut or redefined.

**Dimensions and units.** In scope for v1 per `ROADMAP.md` decision 4, and unit
algebra has to survive inference rather than be checked after it.

## Open items

* Roadmap decision 2, the newline aware lexer layer, is work rather than a
  decision, but its shape is settled during M1's lexer.
* No differential oracle exists. Every expected output is verified by a human.
  That holds only while the test programs stay trivially checkable, and it is a
  standing argument for keeping them that way.
