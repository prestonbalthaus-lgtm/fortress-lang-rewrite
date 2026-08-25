# Phase N — varargs (`x: T...`), design and measurements

Measured at `981f5cc20` (Phase L merge) with `target/debug/fortressc`, pinned to CPUs 2-7.
Written BEFORE any code, per this project's measure-the-ceiling-first rule.

## 0. Why this is not a gap but a silent wrong parse

`...` is completely inert today. Proven, instrument self-tested both ways:

    f(es: ZZ32...): ZZ32 = 7   --emit-ir  sha256 aa8244373b8e1562
    f(es: ZZ32):    ZZ32 = 7   --emit-ir  sha256 aa8244373b8e1562   <- IDENTICAL
    f(es: ZZ32):    ZZ32 = 8   --emit-ir  sha256 0fecf950137a7bec   <- control differs

So `f(es: ZZ32...)` is read as a plain scalar parameter, and **`f(1)` compiles clean**
today. `f(1,2,3)` reports ``f` takes 1 argument(s), found 3`, which is the diagnostic
recorded in `04-state.md`. The first of those is the defect: a program that should bind
a one-element collection compiles as a scalar bind.

## 1. THE PARSE HALF IS ALREADY BUILT. The gap is entirely downstream.

    f(es: ZZ32...)  lexed 37 tokens
    f(es: ZZ32)     lexed 34 tokens

Three extra tokens: `...` lexes as three `Dot`s. **And the parser already re-glues
them.** `at_ellipsis` (`crates/parser/src/lib.rs:464`) exists, with a doc comment
citing `Symbol.rats:212` and `Parameter.rats:88` and recording that the run need not be
glued to the type — `Any...` and `Any ...` are one declaration. It is called at three
sites: `:1028`, `:2387` (the elided-name branch) and `:2411` (the `name: Type` branch).

Already in place besides the glue:

    ast/nodes.rs:399          `pub varargs: bool` on `Param`
    parser/src/lib.rs:1330    `ObjectVarargsParameter`, refusing an object's varargs
                              by name -- `objects.tex:100` spells it `transient
                              Varargs` and `transient` is not a reserved word here
    driver/conform.rs:104     `same_params` compares `varargs`, so an api and a
                              component that disagree do not conform
    driver/conform.rs:125     `describe` renders the `...` back into the diagnostic
    types/mono.rs:676         the flag is propagated through substitution
    parser/src/lib.rs:5291    a lambda parameter is never varargs, recorded at the site
    parser/tests/parser.rs:1267,1280,1317   three assertions on the parsed flag

**So NO lexical reclassification and NO parser work is needed, and the
retroactive-invalidation scan a new `Ellipsis` token would have demanded does not
apply.** That is why the 152 non-varargs `...` sites in compiling files
(`import X.{...}`, `comprises { ... }`) are safe: they always were.

**THE FLAG IS PARSED, RECORDED, CONFORMANCE-CHECKED — AND READ BY NOTHING THAT
LOWERS.** That is exactly why the IR in §0 is byte-identical and why `f(1)` compiles
against `f(es: ZZ32...)`. This milestone is the checker, mono and codegen half only.
It is the same shape as the already-recorded `TypedValue.mutable`: written and read by
nothing, safe only until something needs it to be right.

## 2. The oracle

`Specification-1.0-frozen/appendices/grammars/rats/Parameter.rats:33-34`

    VarargsParam = BindId w colon w Type w ellipses
    Varargs = VarargsParam

`appendices/grammars/concrete-syntax.tex:696-708`

    Params ::= (Param,)* [Varargs,] Keyword(,Keyword)*
            |  (Param,)* Varargs
            |  Param(, Param)*

Keyword parameters are not in this subset, so the reachable rule is: **at most one
varargs, and it is LAST.**

`basic/overloading.tex:15` — the frozen spec's own note: *"Keyword and varargs
parameters are not yet supported."* The chapter still specifies them at :211-226.

`basic/overloading.tex:214-226` is the semantics, and it sanctions this compiler's
architecture directly:

> we can think of a functional declaration that has such parameters as though it were
> (possibly infinitely) many declarations, one for each set of arguments it may be
> called with. ... **In practice, we can bound that number by the maximum number of
> arguments that the functional is called with anywhere in the program.**
> ... A declaration with a varargs parameter is applicable to a call if any one of the
> expanded declarations is applicable.

Whole-program, demand-driven, finite. That is what monomorphization already does.
The worked example expands `f(x:ZZ, y:ZZ, z:ZZ...)` starting at `f(x:ZZ, y:ZZ)` —
**zero trailing arguments is legal.**

## 3. THE api/COMPONENT SPLIT IS THE WHOLE MILESTONE'S SAFETY

15 corpus files compile today **because** `...` is inert. Every one is a `.fsi`:

    CompilerLibrary/File.fsi   CompilerLibrary/List.fsi   CompilerLibrary/Set.fsi
    Library/File.fsi           Library/Format.fsi         Library/FortressLibrary.fsi
    Library/IntMap.fsi         Library/List.fsi           Library/PrefixSet.fsi
    Library/PureList.fsi       Library/Set.fsi            Library/Stream.fsi
    ProjectFortress/BirdyLib/{List,PureList,Set}.fsi

`Library/FortressLibrary.fsi` is the bootstrap root. Refusing a varargs declaration
outright would lose all 15 against a 583 baseline.

**RULE: a varargs parameter in a BODILESS api declaration is RECORDED AND NEVER
LOWERED.** It is checked and stamped only at a BODIED declaration or a CALL DEMAND.
That is exactly the split the tuple TYPE already has — an api may name one freely
because it is never lowered — and it is Link 5's rule 3 one level down: an api's
function declarations are OBLIGATIONS (`source-code.tex:313-320`), not names an
importer receives.

## 4. Lowering

For a call of arity `n` reaching `f(p1..pk, es: T...)`, stamp a concrete declaration
of arity `n`. The trailing `n-k` arguments are collected into a **rank-1 `T[n-k]`**
allocated at the call site through the existing `fortress_array_alloc`. No new shim,
one allocation path, no boxing.

`es` binds to that array, so `length(es)`, `es[i]` and `for x <- es` all work through
existing rank-1 machinery — which is also why the element type must be storable.

**Arity 0 is verified working, not assumed:** `a: ZZ32[0] = array(0)` compiles, runs,
and `length(a)` prints `0`.

**ARITY GOES IN THE MANGLE** — and the reason is subtler than "arity is not in the
mangle today". `mangle` (`types/src/lib.rs:315-324`) joins parameter symbols with `_`,
so for a FIXED parameter list arity is already implicit. It stops being implicit for a
varargs stamp precisely BECAUSE §4 collapses the trailing arguments into one `T[n-k]`
parameter: `f`@3 and `f`@4 both mangle to `f$zz32_array_zz32`. Two different stamps,
one symbol. Rank-above-one entering the mangle is the precedent for fixing it.

### 4a. Where the flag actually dies, and it is FOUR sites not one

`p.varargs` is dropped at `types/src/lib.rs:1416`, `build_signatures`'s parameter loop:
it reads `p.ty` and `p.name` and never `p.varargs`, producing
`Signature.params: Vec<Type>` — a fixed-arity type vector with **no way to say "this
position collects"**. Every downstream consumer (applicability, dispatch, mangling,
body checking) reads that vector, so from there on a varargs declaration is
indistinguishable from a scalar one. That is why none of the eight `ArityMismatch`
sites is the place to change: the flag never reaches them.

There are **FOUR `Signature` construction sites** and the stamp logic must not be built
into only some of them:

    lib.rs:1476   build_signatures            top-level functions
    lib.rs:1202   build_method_signatures     dotted methods -- DIFFERENT split
                                              mechanism: `abstract_` fed to `excusable`
                                              (:401-407), which excuses
                                              `TypeNotImplemented` and
                                              `UnsupportedElementType` and then SKIPS
                                              the whole signature. `Any...` at a stamp
                                              raises `UnsupportedElementType`, so this
                                              path already half-implements §3's split
                                              by accident -- verify, do not assume.
    lib.rs:1371   build_functional_signatures functional methods, e.g. Set.fsi:55
    lib.rs:1566   declare_constructors        MUST STAY UNREACHABLE. An object's value
                                              parameters are its FIELDS; a field has a
                                              layout and an offset and a collected
                                              varargs field has neither. The parser
                                              already refuses it
                                              (`ObjectVarargsParameter`, parser
                                              lib.rs:1330). Anchored here so nobody
                                              "completes" the milestone by wiring a
                                              fourth stamp path.

### 4b. `Type` IS NOT TOUCHED, and `TypedParam` is where a flag would go

No thirteenth `Type` variant. §4's lowering produces `Type::Array(Elem, 1)`, which
already exists, and the `const _: () = assert_copy::<Type>()` at `types.rs:232` must
keep compiling. **There must be no `Type::Varargs`** — varargs is a property of a
DECLARATION, not of a type.

`TypedParam` (`types.rs:742`) has three fields and no arity marker, so `Param.varargs`
stops at the AST today. If codegen's prologue, a diagnostic that wants to print
`es: ZZ32...`, or a future arity-flattening pass needs it, the field goes THERE.

### 4c. THREE ARITY-LOCKED `zip`s, and `zip` REPORTS NOTHING

`function` (:2503), `method` (:2585, with `skip(1)` for the receiver) and
`functional_method` (:2658, which finds `self` BY NAME rather than by position) each
`zip` the written `Param`s against `Signature.params` to bind the body's scope.
`zip` truncates silently on a length mismatch — so a stamp that gets the collected
slot wrong shifts or drops a binding and **nothing says so**. §4's one-slot collection
keeps the lengths equal, which is what makes the zips survive; any design that expands
the trailing arguments into k separate slots breaks all three at once, invisibly.

### 4d. The parser-side rules that are STILL MISSING

`params` (`parser/src/lib.rs:2294-2431`) records `varargs` at two sites (`:2387` the
elided-name branch, `:2411` the named branch) and contains **no check on varargs
POSITION or COUNT**. `f(a: ZZ32..., b: ZZ32...)` parses clean today with both flags set.

**THE CHECK CANNOT LIVE IN `params`.** The `opr` path assembles ONE parameter vector
from up to four separate `params()` calls (`:1984`, `:2023`, `:2056`, `:2138`), so a
per-call check would accept a varargs that is last within its own list and not last in
the assembled signature.

The precedent is `reject_elided_name` (`:2238`) — a free-standing associated fn taking
`&[Param]` and `Option<&Expr>`, whose first line is `if body.is_none() { return Ok(()) }`.
**That IS §3's api/component split, already written once in this file.** It is called
at exactly two sites, `:1677` and `:1892`; `opr_decl` (`:1911`) reads its body the same
way and is the gap.

**`signature_only` IS THE WRONG DISCRIMINATOR.** It is threaded from `decl` into
`fn_decl` and is in scope when `params` runs — but `Library/Random.fss:76` writes
`abstract perturbed(perturbvec:ZZ32...)`, a BODILESS member inside a `.fss`. The
discriminator is `body.is_none()` per declaration, which is what `reject_elided_name`
already uses.

### 4e. `at_ellipsis` MUST NOT CHANGE

It has TWO callers: parameter position (`:2387`/`:2411`) and **`:1028`, import-on-demand**.
Tightening its glue rule to "must be glued to the type" would silently change how
`import X.{...}` parses. Its doc comment already records that `Any...` and `Any ...`
are the same declaration, per `Parameter.rats:88`.

## 5. Ambiguity falls out; do NOT invent a rule

The spec says a varargs declaration is applicable if any expansion is, and is silent on
tie-breaking. This compiler already makes an ambiguous call a **compile error naming
the tuple and both declarations** (a signed-off deviation from 1.0's arbitrary winner).

So `f(a: ZZ32)` beside `f(es: ZZ32...)` at `f(1)` reports an ambiguity. That is
spec-faithful AND consistent with the house deviation. **Do not suppress the stamp at
arities where a fixed declaration exists** — suppressing is wrong the moment the fixed
declaration is not type-applicable (`f(s: String)` + `f(es: ZZ32...)` at `f(1)`).

## 6. Refused by name

- **`Any...` at a bodied site or a stamp demand.** 30 of 86 sites. A `ZZ32` in an
  `Any[]` slot needs boxing and this backend does not box; `occupies_a_trait_slot` is
  an allowlist admitting only `Type::Object` and `Type::Trait`. Accepted-and-inert
  api-side (see §3), refused where it would be lowered.
- **A tuple element type**, `xs: (Key,Val)...` — `Library/Map.fsi:100`,
  `IntMap.fsi:80`, `PrefixMap.fsi:105`, `BirdyLib/Map.fsi:96`. No tuple value
  representation. Same api-side/bodied split.
- **A varargs that is not last**, and **more than one varargs**.
- **A local varargs** (`LocalDecl.rats:142`) and **`transient` object varargs**
  (`TraitObject.rats`). Measure at zero corpus files, refuse by name, do not build.

## 7. Element-type distribution of the 86 sites

    Any    30   refused where lowered (boxing)
    ZZ32   16 |
    String 11 |  28 concrete monomorphic — buildable
    Boolean 1 |
    Foo     1 |
    E/F/T/List[\T\]/IntMap[\Val\]  20   static parameters — buildable via the stamp
    Type    6   Library/Reflect.fsi, reflection, out of subset

61 sites sit in a `.fss`, 25 in a `.fsi`.

## 8. PREDICTED CEILING, written before the sweep

**0-3 files gained, 0 lost.** The 15 files that compile today are all `.fsi` and stay
compiling by §3. A gain requires a BODIED varargs declaration that is actually called,
and most of those sit behind other walls.

The value is not the file count. It is killing a **silent-wrong-parse class across 86+
sites**, the same job H and I did — make the compiler say the true thing so the
milestone behind it can be priced.

**This does NOT unblock `Library/Set.fsi:55`.** Varargs is one of that route's THREE
blockers; the declaration is still BODILESS and an api's function declarations are
still OBLIGATIONS the resolver does not merge. The resolver stays untouched.

## 9. Not covered by this milestone

`assert takes 3 argument(s), found 5` (3 files: `arrayWithTrailingSpaces.fss`,
`objectCC_mutVar1.fss`, `objectCC_mutVar2.fss`). `assert` is a BUILTIN routed by name
at `types/src/lib.rs:5397`; parser varargs never reaches `fn assert` at `:6212`. Its
own deliverable.

## 10. Mutation rows to write with the code

1. non-final varargs accepted
2. two varargs accepted
3. `Any...` lowered at a bodied site
4. varargs beside a same-arity fixed declaration — ambiguity suppressed
5. arity dropped from the mangle
6. api-side varargs refused rather than recorded (must take all 15 `.fsi` down)

Each needs a fixture that can actually reach the guard; a row whose fixture cannot
reach it reports SURVIVED forever.

---

# SLICE 2 — the stamp. Mapped at `d01e1dc6a`, before any code.

Slice 1 (the two structural rules) is committed. This is the lowering.

## S2.1 THE HEADLINE: a plain call generates NO demand today

`mono.rs:985`, the `Expr::Call` arm, produces demand through EXACTLY ONE path —
`Expr::Instantiate`, i.e. WRITTEN STATIC ARGUMENTS. `f(1,2,3)` on a ground function
files no `Job`, no `MethodRequest`, nothing; the fall-through at `:1006-1010` is a pure
structural rebuild. Varargs arity is a property of the CALL'S ARGUMENT COUNT, which
this arm does not currently read.

**So varargs is the first demand kind in this compiler sourced from a call's shape
rather than from written type structure.** That is the whole difficulty, and it has
three consequences below (S2.5, S2.6, S2.7).

## S2.2 The precedent to copy, verbatim

`MethodRequest` (`mono.rs:85-91`) already is arity-keyed demand:

    struct MethodRequest { name, args, value_arity, mangled, span }

and its doc comment already states varargs' own justification: *"The receiver is
deliberately not part of it: this pass has no types, so demand is by name and arity
only."* A `VarargsRequest { name, value_arity, span }` is that struct minus `args`.

**Dedup with `.entry((name, value_arity)).or_insert_with(...)` on a `BTreeMap`**, the
idiom at `mono.rs:1349`. `f(1,2,3)` and `f(4,5,6)` are ONE arity-3 stamp; pushing into
a `Vec` instead files one job per call site and burns the instantiation budget.
`BTreeMap` and not `HashMap` for the determinism reason stated at `mono.rs:39`.

## S2.3 Where it runs

`lib.rs:73`, inside `mono::expand` — the only pass that runs to a fixpoint after the
AST is ground and before tags freeze. The whole-component syntactic pre-flight (refuse
`Any...`/tuple element at a bodied site, refuse a local varargs) goes beside
`check_template_headers` at `mono.rs:101`, not inside the walk.

**INPUT CONTRACT: `tuple::lower` (`lib.rs:72`) HAS ALREADY RUN**, and it destroys the
varargs flag on a tuple element type (`tuple.rs:278`). So the `xs: (Key,Val)...`
refusal cannot be written as "check the flag after expansion" — by then it is gone.

## S2.4 The one predicate change in the stamper

`stamp_methods`'s filter (`mono.rs:255`) is an EXACT arity match today:

    t.decl.params.len() == request.value_arity

A varargs template must instead match `value_arity >= fixed_prefix_len`. That is the
single change; everything else in that function is copied as-is, including the
reserved-before-walked memo at `:307`.

## S2.5 TERMINATION — the stamper's return value IS the fixpoint signal

`run`'s `loop` at `mono.rs:226-231` breaks on "nothing new was made", and
`stamp_methods` signals that with `Ok(true)`/`Ok(false)`. **A varargs stamper that
always returns `true` never terminates.** A third demand kind must participate in that
same `break` condition or the loop exits before arity stamps that were filed during the
last round are made.

## S2.6 MAX_INSTANTIATIONS — one ceiling, and varargs is the first thing that can blow
it from a program with no generics at all

`MAX_INSTANTIATIONS = 4096` (`mono.rs:31`), enforced at `:292-298` and `:352-358`.
`total()` (`:343`) must learn to count arity stamps, and there must NOT be a second
ceiling for them — the constant's own comment says two separate ceilings is the bug.

Because varargs demand is driven by call ARITY, `f(es: ZZ32...)` called at 4097
distinct arities trips the limit with not one generic in the program. That is correct
behaviour, and it is a new reachable path to that diagnostic.

## S2.7 DEV-16 DROPS `()` PARAMETERS, AND THAT CHANGES THE PREFIX LENGTH

`functional_params` (`mono.rs:684-693`) is `self.params(...)` followed by
`out.retain(|p| !is_void_parameter(p))`. A varargs stamp computes its trailing count as
`call_arity - fixed_prefix_len`; if the fixed prefix contains a `()` parameter, that
retain has already deleted it and the two arities are measured against DIFFERENT LISTS.
Whichever side computes the prefix must agree with this `retain`.

## S2.8 What is already right and must be routed through, not around

`mono::params` (`:666-683`) rebuilds every `Param` field by field and carries
`varargs: p.varargs` at `:676`. That is what lets the 20 generic-element sites (§7:
`E`, `F`, `T`, `List[\T\]`, `IntMap[\Val\]`) reach a stamp with the flag intact.

`is_signature_only` (`mono.rs:1641-1646`) IS §3's api/component rule already written,
and its doc comment is §3's argument. Reuse it rather than writing a second bodiless
test. Its own warning does fire, though: *"a bodiless declaration generates no demand"*
is a claim about STATIC arguments. Varargs adds demand from CALL ARITY — the claim
still holds, because a bodiless api declaration has no calls either, but it now holds
for a second reason and the comment should say so.

## S2.9 `check_uniformity` is ARITY-BLIND, in both directions

`mono.rs:1672-1743` keys by declaration NAME and compares ONLY `StaticParam` vectors
(`:1725-1731`) — it never looks at `Param`. It also runs at `mono.rs:160`, inside
`Expander::new`, on the component AS WRITTEN: pre-expansion, so it sees UNEXPANDED
forms.

Two consequences: arity stamps can never trip it, and **nothing existing pre-validates
value parameters at all** — which is exactly why slice 1's position rule had to be
written from scratch in the parser rather than found here.

## S2.10 The mangle collision, restated precisely

`mangle` (`lib.rs:315-324`) joins parameter symbols with `_`, so arity is implicit for
a FIXED list. It stops being implicit for a varargs stamp precisely because §4 collapses
the trailing arguments into ONE `T[n-k]` parameter: `f`@3 and `f`@4 both become
`f$zz32_array_zz32`. Two stamps, one symbol. Arity must enter the mangle.

## S2.11 Predicted ceiling for slice 2, written before the build

Still **0-3 files gained, 0 lost**, unchanged from §8. Slice 2 is what makes a varargs
call MEAN something; the file count does not move because the 41 bodied `.fss` sites
sit behind other walls, and the 15 api sites stay recorded-and-not-lowered.
