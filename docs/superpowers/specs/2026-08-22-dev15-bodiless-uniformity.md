# DEV-15: the uniformity rule is relaxed for a pair of bodiless declarations

**Date:** 2026-08-22. **Authorized by Preston**, in the brief that scheduled it:
"Relax the uniformity restriction only for bodiless declarations. This
surgically bypasses the `__cond` errors in the APIs while preserving the safety
of user implementations."

**Answers:** link 4 of `2026-08-22-library-bootstrap-measured.md`, and the
recommendation in `2026-08-22-uniformity-relaxation-measured.md` §5 that the
narrow version is the one to price first.

---

## The rule and what breaks it

`Specification-1.0-frozen/basic/overloading.tex:100-108`:

> it is an error for their static parameters to differ (up to
> alpha-equivalence), or for one declaration to have static parameters and
> another to not have them.

`Library/FortressLibrary.fsi:757-758` breaks it:

    __cond[\E,R\](c:Condition[\E\], t:E->R, e:()->R): R
    __cond[\E\](c:Condition[\E\], t:E->()): ()

`BIG //`, `#` and `:` are three more pairs in the same file, and
`CompilerLibrary/FortressLibrary.fsi:764-765` repeats the first.
**The shipped library is not conformant with the shipped specification**, and no
amount of compiler work makes it so. The `\note{This restriction will be
relaxed.}` nearby was checked and annotates the OPERATOR restriction above it,
not this rule.

## What landed

`check_uniformity` skips the comparison when **both** declarations of the
collision are bodiless functions. Everything else is unchanged.

    fn is_signature_only(decl: &Decl) -> bool {
        let Decl::Function(f) = decl else {
            return NOTHING_ELSE_IS_A_SIGNATURE;
        };
        f.body.is_none()
    }

**BOTH SIDES, AND THAT IS THE WHOLE SCOPE.** One bodiless declaration beside a
bodied one is still refused: the bodied one can be CALLED, and a call is what
needs an overload set whose members agree on how many static arguments they
take.

### Why bodiless is the safe boundary

Two facts, and the deviation rests on both:

1. **A bodiless declaration generates no demand.** Static arguments are WRITTEN,
   never inferred, and there is no body to write one in. A relaxed function set
   cannot reach `expand_types`.
2. **The import resolver never merges `Decl::Function` into an importer.** An
   api's function declarations are signatures the component must SATISFY
   (`source-code.tex:313-320`), so a relaxed set cannot leave the file it is
   written in.

If either stops being true, `is_signature_only` is what has to be re-argued, and
it says so at the site.

### Why a trait is never a signature

A trait or object writes no body because it *cannot*, not because it is a
promise somebody else keeps -- and its name is written in TYPE position, which
IS demand. `Condition[\()\]` is exactly that. So `trait Holder[\T\]` beside
`trait Holder` reaches expansion with members that disagree on static arity,
inside an api as much as inside a component, and stays refused. A value is not a
signature either: it takes no static parameters, so exempting it could only ever
weaken the comparison against a generic function of the same name.

## Measured

Full corpus sweep, 1956 files, against `778214e76`:

| | baseline | DEV-15 |
|---|---|---|
| `.fss` compiling and emitting an object | 383 | 383 |
| `.fsi` checking | 65 | **66** |
| total | 448 | **449** |
| crashes (exit not 0 or 1) | 0 | 0 |

**GAINED: `ProjectFortress/BirdyLib/Tuple.fsi`.** It declares
`first[\T1,T2\]`, `first[\T1,T2,T3\]` and `first[\T1,T2,T3,T4\]` -- three
bodiless signatures at three static arities, which is the deviation's subject
written out.

**LOST: none.** One other file moved forward without compiling:
`ProjectFortress/BirdyLib/Maybe.fsi` was on the `__cond` wall and is now on
`unknown type CheckedException`.

**THE MUST-FAIL THAT SEPARATES THIS FROM THE BLANKET VERSION IS STILL REFUSED.**
`ProjectFortress/compiler_tests/Compiled6.ak.fss` writes `f(x: ZZ32) = ()`
beside `f[\T extends Any\](x: T) = ()` -- two BODIES -- and its `.test` expects
two errors. The blanket relaxation accepted it (see
`2026-08-22-uniformity-relaxation-measured.md` §1); DEV-15 does not.

## DEV-14 IS DEAD, MEASURED

`Uniformity::ExemptLegacyLibrary` suspended the rule for any file under a
directory named `Library` or `CompilerLibrary` -- a PATH test, deliberately not
earnable by writing anything. DEV-15 pays for every file it was paying for.

Swept over all **136** `.fss` and `.fsi` files under `Library/` and
`CompilerLibrary/`, each compiled twice, with and without
`--no-legacy-library-uniformity`:

    checked=136 differing=0

Neither the exit status nor one byte of any diagnostic changes. The path
exemption is a mechanism that now does nothing, and a dead mechanism that
suspends a safety rule is worse than none: it is strictly WIDER than DEV-15 and
would blanket-exempt a BODIED violation under `Library/`, which is the one thing
DEV-15 is careful not to do. It is retired in the commit after this one, so the
two are separately revertable.

## Not fixed here, found here

`XXXComprisesHidden.fss` reports its open-`comprises` defect against `T` or `S`
**depending on the run**, on the SAME binary -- `comprises.rs:95` builds a
`HashMap<&str, Row>` and reports out of it. Both readings are a correct refusal
of the same file, so nothing is wrong with the answer, but a nondeterministic
diagnostic is a flaky gate waiting to happen on a project whose gates assert
messages. Pre-existing; reproduced on the baseline binary three runs in a row.
