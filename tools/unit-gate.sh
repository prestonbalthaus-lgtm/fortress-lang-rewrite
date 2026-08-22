#!/usr/bin/env bash
#
# The M3e gate: the unit type, and syntax for tuples and arrows.
#
# Four things cargo cannot check on its own: that a void function is a real ELF
# that runs, that `()` in a position which has to store a value is a diagnostic
# and not an internal error, that a parenthesised type really is the type it
# parenthesises rather than a one-element tuple, and that the three parsed but
# unimplemented forms halt with exit 1 rather than 70.
#
# 2026-08-22, `()` AS A STATIC ARGUMENT: ONE OF THOSE FOUR CHANGED SIDES.
# `basic/functions.tex:148-151` says no bindings, `()`, is EQUIVALENT to a
# single plain binding `(_: ())` -- so `f(x: ()): ZZ32` is not an error, it is
# `f(): ZZ32`, and `badvoidparam.fss` is now `unitparam.fss` and an ACCEPTANCE.
# Proved by comparing its module against the other spelling's, not by both
# compiling.
#
# WHAT DID NOT CHANGE SIDES IS THE VALUE. `types-vals-vars.tex:469-471` makes
# `Any` the only supertype of `()`, so the subtype test now says yes -- and a
# trait slot is a tagged pointer with nothing to put in it. `badvoidvalue.fss`
# is the program that found this by exiting 70, and it is why the refusal moved
# rather than being deleted.
#
# Exit 70 is what makes this gate specific. The driver reserves it for compiler
# bugs, and a gate that accepted any nonzero status would be green on exactly
# the defect this milestone fixed.
#
#   ./tools/unit-gate.sh              run the gate
#   ./tools/unit-gate.sh --selftest   only prove the assertions can refuse
#   ./tools/unit-gate.sh --mutate     break the compiler three ways and prove
#                                     the gate refuses each one
set -uo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
build=$repo/fortressc/build
fortressc=${FORTRESSC:-$repo/fortressc/target/debug/fortressc}
export LLVM_SYS_221_PREFIX=${LLVM_SYS_221_PREFIX:-$HOME/.local/opt/llvm22-root/usr/lib64/llvm22}
export CPATH=${CPATH:-$HOME/.local/opt/gc-root/usr/include}
export LIBRARY_PATH=${LIBRARY_PATH:-$HOME/.local/opt/gc-root/usr/lib64}

passed=0
failed=0
ok()  { passed=$((passed + 1)); printf 'ok    %s\n' "$1"; }
bad() { failed=$((failed + 1)); printf 'FAIL  %s\n' "$1"; [[ -n ${2:-} ]] && printf '      %s\n' "$2"; }

# ---------------------------------------------------------------- assertions

# A diagnostic is exit 1 and nothing else. 70 is EXIT_INTERNAL_ERROR, 124 is a
# timeout, 139 is SIGSEGV: all three mean the compiler broke rather than
# reported. 0 means it accepted a program it should have refused.
refused_cleanly() { [[ $1 -eq 1 ]]; }

# Two IR dumps agree when everything but the module identity matches. The name
# is the one thing that must differ, because it comes from the component.
same_ir() {
    diff <(grep -v 'ModuleID\|^source_filename' <<<"$1") \
         <(grep -v 'ModuleID\|^source_filename' <<<"$2") >/dev/null
}

selftest() {
    printf '== gate self test ==\n'

    if refused_cleanly 1; then
        ok 'exit 1 is a clean refusal'
    else
        bad 'exit 1 is a clean refusal'
    fi

    for status in 0 70 124 139; do
        if refused_cleanly "$status"; then
            bad "status $status is refused as a clean refusal" \
                'only exit 1 is a diagnostic; 70 is a compiler bug'
        else
            ok "status $status is refused as a clean refusal"
        fi
    done

    local a b
    a=$'; ModuleID =
define i32 @f(i32 %x) {
  ret i32 %x
}'
    b=$'; ModuleID =
define i32 @f(i32 %x) {
  ret i32 %x
}'
    if same_ir "$a" "$b"; then
        ok 'two identical modules compare equal'
    else
        bad 'two identical modules compare equal'
    fi

    b=$'; ModuleID =
define i32 @f(i32 %x) {
  ret i32 0
}'
    if same_ir "$a" "$b"; then
        bad 'two different modules are refused as equal' 'the comparison sees nothing'
    else
        ok 'two different modules are refused as equal'
    fi
}

# ------------------------------------------------------------------ the gate

preflight() {
    ( cd "$repo/fortressc" && cargo build --workspace ) || exit 2
    rm -rf "$build"
    mkdir -p "$build"
}

compile() {
    printf '== compile ==\n'
    if "$fortressc" "$repo/fortressc/tests/unitvoid.fss" -o "$build/unitvoid" \
        2>"$build/unitvoid.err"; then
        ok 'unitvoid.fss compiles and links'
    else
        bad 'unitvoid.fss compiles and links' "$(cat "$build/unitvoid.err")"
    fi
}

runs() {
    printf '== a void function runs ==\n'
    if [[ ! -f $build/unitvoid ]]; then
        bad 'run():() = () runs' "no artifact at $build/unitvoid"
        return
    fi

    local out status
    out=$("$build/unitvoid" 2>&1)
    status=$?
    if [[ $status -eq 0 && $out == 'hello from a void function' ]]; then
        ok 'run():() = () compiles, links and runs'
    else
        bad 'run():() = () compiles, links and runs' "status $status: $out"
    fi
}

# `(A)` is `A`, proved by two compilations rather than by one succeeding. If a
# one-element parenthesised type were folded into the tuple case, the first of
# these would not compile at all.
parens() {
    printf '== a parenthesised type is the type ==\n'
    local a b
    if ! a=$("$fortressc" "$repo/fortressc/tests/parenthesised.fss" --emit-ir 2>/dev/null); then
        bad '(ZZ32) compiles' "$("$fortressc" "$repo/fortressc/tests/parenthesised.fss" --emit-ir 2>&1 >/dev/null)"
        return
    fi
    ok '(ZZ32) compiles'
    if ! b=$("$fortressc" "$repo/fortressc/tests/plainnamed.fss" --emit-ir 2>/dev/null); then
        bad 'ZZ32 compiles'
        return
    fi
    if same_ir "$a" "$b"; then
        ok '(ZZ32) and ZZ32 generate the same module'
    else
        bad '(ZZ32) and ZZ32 generate the same module' \
            "$(diff <(printf '%s' "$a") <(printf '%s' "$b") | head -6 | tr '\n' ' ')"
    fi
}

# Four refusals, because the design lists four and a gate that checks two of
# them is green on a compiler that lost the other two.
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
badvoidvalue|cannot be stored in a slot of a wider type
badvoidfield|cannot be stored in a field
voidnotobject|() does not satisfy `T extends Object`
badtupletype|cannot be the result of a function with a body
badarrowtype|an arrow type is not implemented
badtupleexpr|a tuple expression is not implemented
CASES
}

# A `()` PARAMETER IS NO PARAMETER, and the two spellings must generate THE SAME
# MODULE. Both compiling proves nothing: a compiler that kept the parameter and
# passed an undef would compile both too. The component names are deliberately
# equal so even the module identity does not differ.
unit_parameter_is_no_parameter() {
    printf '== a `()` parameter is no parameter ==\n'
    local a b
    if ! a=$("$fortressc" "$repo/fortressc/tests/unitparam.fss" --emit-ir 2>/dev/null); then
        bad 'f(x: ()) compiles' \
            "$("$fortressc" "$repo/fortressc/tests/unitparam.fss" --emit-ir 2>&1 >/dev/null)"
        return
    fi
    ok 'f(x: ()) compiles'
    if ! b=$("$fortressc" "$repo/fortressc/tests/unitnoparam.fss" --emit-ir 2>/dev/null); then
        bad 'f() compiles'
        return
    fi
    if same_ir "$a" "$b"; then
        ok 'f(x: ()) and f() generate the same module'
    else
        bad 'f(x: ()) and f() generate the same module' \
            "$(diff <(printf '%s' "$a") <(printf '%s' "$b") | head -6 | tr '\n' ' ')"
    fi
}

# `Condition[\()\]` IN TEN LINES. The wall this milestone cleared is in a
# 754-line library file; reproduced here so a failure names the FEATURE. And
# then the library file itself, because a fixture that passes while the real
# thing does not is a gate measuring itself.
unit_as_a_static_argument() {
    printf '== `()` as a static argument ==\n'
    local err status
    err=$("$fortressc" "$repo/fortressc/tests/condunit.fsi" --emit-obj -o /dev/null 2>&1 >/dev/null)
    status=$?
    if [[ $status -eq 0 ]]; then
        ok 'a trait instantiated at `()` checks'
    else
        bad 'a trait instantiated at `()` checks' "status $status: $err"
    fi

    err=$("$fortressc" "$repo/ProjectFortress/LibraryBuiltin/CompilerBuiltin.fsi" \
            --emit-obj -o /dev/null 2>&1 >/dev/null)
    status=$?
    if [[ $status -eq 0 ]]; then
        ok 'CompilerBuiltin.fsi checks clean, all four `Condition[\()\]` intact'
    else
        bad 'CompilerBuiltin.fsi checks clean' "status $status: $err"
    fi

    if ! "$fortressc" "$repo/fortressc/tests/unitinstance.fss" -o "$build/unitinstance" \
            2>"$build/unitinstance.err"; then
        bad 'a generic instantiated at `()` compiles and links' \
            "$(cat "$build/unitinstance.err")"
        return
    fi
    local out
    out=$("$build/unitinstance" 2>&1)
    status=$?
    if [[ $status -eq 0 && $out == '7' ]]; then
        ok 'a generic instantiated at `()` compiles, links and RUNS'
    else
        bad 'a generic instantiated at `()` compiles, links and RUNS' "status $status: $out"
    fi
}

# ----------------------------------------------------------------- mutations
#
# Each entry is file|from|to|label. Every `from` must match exactly once in its
# file, and the tree has to be clean first. Restored either way.

MUTATIONS=(
  # RE-TARGETED 2026-08-22. It pointed at the void guard on a PARAMETER, and
  # that line no longer exists in that shape -- and the guard behind it is a
  # BACKSTOP now rather than a live path, because a functional's `()` parameter
  # is dropped before the checker sees one. The row that exercises the backstop
  # is "stop dropping a `()` parameter at the substitution", above. An object's
  # value parameters are its FIELDS and are still refused, which is where the
  # void guard is reachable from source.
  'crates/types/src/lib.rs|ty: self.storable(&p.ty, "a field")?,|ty: self.registry.resolve(&p.ty)?,|drop the void guard on an object field'
  # `()` AS A STATIC ARGUMENT, THREE AXES. Dropping the parameter, `()` being a
  # subtype of `Any`, and `()` still having no representation are three separate
  # decisions and no one row reaches two of them.
  'crates/types/src/mono.rs|matches!(p.ty, TypeRef::Unit { .. })|matches!(p.ty, TypeRef::Tuple { .. })|stop dropping a `()` parameter at the substitution'
  'crates/types/src/registry.rs|Type::Void => wanted == "Any",|Type::Void => false,|put `()` under nothing, so no bound it is written against discharges'
  'crates/types/src/registry.rs|Type::Void => wanted == "Any",|Type::Void => true,|put `()` under EVERY trait rather than under `Any` alone'
  'crates/types/src/lib.rs|found == Type::Void && want != Type::Void && self.registry.is_subtype(found, want)|found != Type::Void && want != Type::Void && self.registry.is_subtype(found, want)|let a `()` VALUE into a wider slot, which has nothing to put there'
  'crates/parser/src/lib.rs|if elems.len() == 1 {|if false {|fold a one-element parenthesised type into Tuple'
  'crates/parser/src/lib.rs|return Ok(TypeRef::Tuple { elems, span });|return Ok(TypeRef::Named { name: "ZZ32".to_owned(), args: Vec::new(), span });|make a tuple type silently become ZZ32'
)

# FORTRESSC AND --mutate DO NOT MIX, and the failure is silent. Every mutation
# below rebuilds fortressc/target/debug; if FORTRESSC points anywhere else the
# gate keeps reading the pinned binary, the mutation has no effect, the
# assertion holds, and the table reports a clean escape. Refuse instead.
mutate_needs_the_built_compiler() {
    local built=$repo/fortressc/target/debug/fortressc
    if [[ $fortressc != "$built" ]]; then
        printf 'refusing --mutate: FORTRESSC is %s\n' "$fortressc" >&2
        printf 'but every mutation rebuilds %s.\n' "$built" >&2
        printf 'A pinned binary makes each mutation a silent no-op. Unset FORTRESSC.\n' >&2
        exit 2
    fi
}

mutate() {
    mutate_needs_the_built_compiler
    # Against HEAD, not against the index, and the restore below matches. A
    # gate that rewinds to the index will faithfully put a DEFECT back if
    # anything staged during the run -- and the worktree and the index would
    # then agree with each other while both are wrong.
    if ! git -C "$repo" diff --quiet HEAD -- fortressc/crates; then
        printf 'refusing to mutate: fortressc/crates differs from HEAD\n' >&2
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
            compile >/dev/null 2>&1
            runs; parens; unit_parameter_is_no_parameter
            unit_as_a_static_argument; refusals
            if [[ $failed -gt 0 ]]; then
                printf 'REFUSED  %d check(s) failed, which is the point\n' "$failed"
            else
                printf 'SURVIVED %s -- the gate did not notice\n' "$label"
                survived=$((survived + 1))
            fi
        fi
        git -C "$repo" checkout HEAD -- "fortressc/$file"
    done

    ( cd "$repo/fortressc" && cargo build --workspace >/dev/null 2>&1 )
    printf '\nmutations: %d run, %d survived, %d could not be applied\n' \
        "${#MUTATIONS[@]}" "$survived" "$broken"
    [[ $survived -eq 0 && $broken -eq 0 ]]
}

# ----------------------------------------------------------------- main

case "${1:-}" in
    --selftest)
        selftest
        ;;
    --mutate)
        selftest
        preflight
        mutate
        exit $?
        ;;
    *)
        selftest
        preflight
        compile
        runs
        parens
        unit_parameter_is_no_parameter
        unit_as_a_static_argument
        refusals
        ;;
esac

printf '\n%d/%d\n' "$passed" "$failed"
[[ $failed -eq 0 ]]
