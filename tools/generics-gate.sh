#!/usr/bin/env bash
#
# The M3d gate: generics by monomorphization.
#
# Five things cargo cannot check on its own: that an instantiation is stamped out
# unboxed rather than erased or boxed, that two builds of the same generic
# program are byte identical, that an instantiation reaching a trait gets its own
# dispatch arm, that polymorphic recursion is refused rather than hung on, and
# that the two rules holding the design up -- 1.0's uniformity rule and the bound
# obligations -- actually refuse.
#
#   ./tools/generics-gate.sh              run the gate
#   ./tools/generics-gate.sh --selftest   only prove the assertions can refuse
#   ./tools/generics-gate.sh --mutate     break the compiler three ways and
#                                         prove the gate refuses each one
set -uo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
build=$repo/fortressc/build
fortressc=$repo/fortressc/target/debug/fortressc
export LLVM_SYS_221_PREFIX=${LLVM_SYS_221_PREFIX:-$HOME/.local/opt/llvm22-root/usr/lib64/llvm22}
export CPATH=${CPATH:-$HOME/.local/opt/gc-root/usr/include}
export LIBRARY_PATH=${LIBRARY_PATH:-$HOME/.local/opt/gc-root/usr/lib64}

# The architect's ceiling. Named here so the gate and the compiler cannot drift.
LIMIT=4096

passed=0
failed=0
ok()  { passed=$((passed + 1)); printf 'ok    %s\n' "$1"; }
bad() { failed=$((failed + 1)); printf 'FAIL  %s\n' "$1"; [[ -n ${2:-} ]] && printf '      %s\n' "$2"; }

# ---------------------------------------------------------------- assertions

# The matrix the dispatch fixture must produce, derived here from the
# declarations rather than read out of the program: Dot has its own declaration,
# and each instantiation of Box has one, so the trait-typed fallback is never
# the winner.
expected_dispatch() { printf '2\n3\n4\n'; }

# A clean refusal: a diagnostic and exit 1. Anything else -- 0, or a signal --
# means the compiler did not decide, it fell over.
refused_cleanly() { [[ $1 -eq 1 ]]; }

# Monomorphization means the argument type survives into the layout. Erasure or
# boxing would put a pointer in both.
unboxed() { [[ $1 == *'i64, i32 }'* ]]; }

selftest() {
    printf '== gate self test ==\n'

    if [[ $(expected_dispatch) == $'2\n3\n4' ]]; then
        ok 'the expected matrix is three distinct winners'
    else
        bad 'the expected matrix is three distinct winners'
    fi

    if refused_cleanly 1; then
        ok 'exit 1 is a clean refusal'
    else
        bad 'exit 1 is a clean refusal'
    fi
    local status
    for status in 0 70 124 139; do
        if refused_cleanly "$status"; then
            bad "status $status is refused as a clean refusal" \
                '0 compiled it, 70 is a compiler bug, 124 hung, 139 faulted'
        else
            ok "status $status is refused as a clean refusal"
        fi
    done

    if unboxed '%"Cell$ZZ64$e" = type { i32, i32, i64, i32 }'; then
        ok 'an i64 field reads as unboxed'
    else
        bad 'an i64 field reads as unboxed'
    fi
    if unboxed '%"Cell$String$e" = type { i32, i32, ptr, i32 }'; then
        bad 'a pointer field is refused as unboxed' 'a ptr is not an i64'
    else
        ok 'a pointer field is refused as unboxed'
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
    local name
    for name in generics genericdispatch; do
        if "$fortressc" "$repo/fortressc/tests/$name.fss" -o "$build/$name" 2>"$build/$name.err"; then
            ok "$name.fss compiles and links"
        else
            bad "$name.fss compiles and links" "$(cat "$build/$name.err")"
        fi
    done
}

have() {
    if [[ -f $1 ]]; then
        return 0
    fi
    bad "$2" "no artifact at $1"
    return 1
}

stamping() {
    printf '== one copy per static argument ==\n'
    have "$build/generics" 'a generic is stamped out per static argument' || return

    local out want
    out=$("$build/generics" 2>&1)
    want=$(printf '7\nhi\n2\n3\nno\nsecond\n')
    if [[ $out == "$want" ]]; then
        ok "two instantiations of an object and three of a function: $(printf '%s' "$out" | tr '\n' ' ')"
    else
        bad 'a generic is stamped out per static argument' \
            "want: $(printf '%s' "$want" | tr '\n' ' ') | got: $(printf '%s' "$out" | tr '\n' ' ')"
    fi
}

unboxing() {
    printf '== monomorphization, not erasure ==\n'
    local ir
    if ! ir=$("$fortressc" "$repo/fortressc/tests/generics.fss" --emit-ir 2>/dev/null); then
        bad 'the generic program emits IR'
        return
    fi

    local zz64 str
    zz64=$(printf '%s' "$ir" | grep -F 'Cell$ZZ64$e" = type')
    str=$(printf '%s' "$ir" | grep -F 'Cell$String$e" = type')
    if unboxed "$zz64" && ! unboxed "$str"; then
        ok "ZZ64 is stored as an i64 and String as a pointer: ${zz64##*= }"
    else
        bad 'ZZ64 is stored unboxed' "zz64=$zz64 str=$str"
    fi

    if printf '%s' "$ir" | grep -qF 'define i64 @"pick$ZZ64$e"(i64'; then
        ok 'the instantiated function takes and returns raw i64'
    else
        bad 'the instantiated function takes and returns raw i64'
    fi
}

dispatching() {
    printf '== an instantiation reaches the dispatch table ==\n'
    have "$build/genericdispatch" 'each instantiation gets its own arm' || return

    local out want
    out=$("$build/genericdispatch" 2>&1)
    want=$(expected_dispatch)
    if [[ $out == "$want" ]]; then
        ok 'Dot and both instantiations of Box each reach their own declaration'
    else
        bad 'each instantiation gets its own arm' \
            "want: $(printf '%s' "$want" | tr '\n' ' ') | got: $(printf '%s' "$out" | tr '\n' ' ')"
    fi

    local ir
    ir=$("$fortressc" "$repo/fortressc/tests/genericdispatch.fss" --emit-ir 2>/dev/null)
    if printf '%s' "$ir" | grep -q 'switch i32 %tag' &&
       printf '%s' "$ir" | grep -qF 'area$Box$ZZ64$e' &&
       printf '%s' "$ir" | grep -qF 'area$Box$String$e'; then
        ok 'the switch has a leaf per instantiation'
    else
        bad 'the switch has a leaf per instantiation'
    fi
}

# Tags are switch keys and switch arms follow tag order, so a nondeterministic
# instantiation order is a nondeterministic binary.
determinism() {
    printf '== the build is reproducible ==\n'
    local a b
    "$fortressc" "$repo/fortressc/tests/genericdispatch.fss" --emit-obj -o "$build/det-a.o" 2>/dev/null
    "$fortressc" "$repo/fortressc/tests/genericdispatch.fss" --emit-obj -o "$build/det-b.o" 2>/dev/null
    if [[ ! -f $build/det-a.o || ! -f $build/det-b.o ]]; then
        bad 'two builds of a generic program are byte identical' 'no object emitted'
        return
    fi
    a=$(sha256sum "$build/det-a.o" | cut -d' ' -f1)
    b=$(sha256sum "$build/det-b.o" | cut -d' ' -f1)
    if [[ $a == "$b" ]]; then
        ok "two builds are byte identical (${a:0:16})"
    else
        bad 'two builds of a generic program are byte identical' "$a vs $b"
    fi
}

# Each of these is a program the compiler must REFUSE. A gate made only of
# programs that must compile cannot catch a rule that stopped firing.
refusals() {
    printf '== the refusals ==\n'
    local name pattern err status
    for entry in \
        "polyrec|$LIMIT|polymorphic recursion stops at the ceiling" \
        "badoverload|uniformly generic or uniformly ground|a mixed overload set is refused" \
        "badbound|does not satisfy|an unsatisfied bound is refused"; do
        IFS='|' read -r name pattern label <<<"$entry"
        err=$(timeout 300 "$fortressc" "$repo/fortressc/tests/$name.fss" --emit-ir 2>&1 >/dev/null)
        status=$?
        if refused_cleanly "$status" && [[ $err == *"$pattern"* ]]; then
            ok "$label (exit $status)"
        else
            bad "$label" "status $status: $err"
        fi
    done
}

# --------------------------------------------------------------- mutations

MUTATIONS=(
  'crates/types/src/mono.rs|type Instances = BTreeMap<String, Instance>;|type Instances = std::collections::HashMap<String, Instance>;|emit instantiations in hash order instead of sorted'
  'crates/types/src/mono.rs|check_uniformity(component)?;|let _ = check_uniformity(component);|stop enforcing the uniformity rule'
  'crates/types/src/lib.rs|self.discharge_bounds(component)?;|let _ = self.discharge_bounds(component);|stop discharging bound obligations'
)

mutate() {
    if ! git -C "$repo" diff --quiet -- fortressc/crates; then
        printf 'refusing to mutate: fortressc/crates has unstaged changes\n' >&2
        exit 2
    fi

    local entry file from to label hits status
    local survived=0 broken=0
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
            stamping >/dev/null 2>&1
            determinism
            refusals
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
        stamping
        unboxing
        dispatching
        determinism
        refusals
        ;;
esac

printf '\n%d/%d\n' "$passed" "$failed"
[[ $failed -eq 0 ]]
