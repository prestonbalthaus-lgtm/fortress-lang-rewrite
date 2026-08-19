#!/usr/bin/env bash
#
# The M3c gate: traits, objects and symmetric multiple dispatch.
#
# Four things cargo cannot check on its own: that every cell of the dispatch
# matrix reaches the declaration the specificity rules say it should, that the
# run-time type decides rather than the static one, that a symmetrically
# ambiguous call is refused at compile time naming both declarations, and that a
# switch arm nothing can reach halts cleanly instead of running off the end.
#
# The matrix is COMPUTED here, from the subtype relation and the declaration
# list, not read out of the program. A gate that takes the answer from the thing
# it is testing is only checking self-consistency.
#
#   ./tools/dispatch-gate.sh              run the gate
#   ./tools/dispatch-gate.sh --selftest   only prove the assertions can refuse
#   ./tools/dispatch-gate.sh --mutate     break the compiler three ways and
#                                         prove the gate refuses each one
set -uo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
build=$repo/fortressc/build
fortressc=$repo/fortressc/target/debug/fortressc
export LLVM_SYS_221_PREFIX=${LLVM_SYS_221_PREFIX:-$HOME/.local/opt/llvm22-root/usr/lib64/llvm22}
export CPATH=${CPATH:-$HOME/.local/opt/gc-root/usr/include}
export LIBRARY_PATH=${LIBRARY_PATH:-$HOME/.local/opt/gc-root/usr/lib64}

passed=0
failed=0
ok()  { passed=$((passed + 1)); printf 'ok    %s\n' "$1"; }
bad() { failed=$((failed + 1)); printf 'FAIL  %s\n' "$1"; [[ -n ${2:-} ]] && printf '      %s\n' "$2"; }

# ---------------------------------------------------------------- the model
#
# tests/dispatch.fss, restated. Everything below is derived from these two
# declarations of fact and nothing else.

# The hierarchy: "concrete trait" pairs.
EXTENDS=("Solid Ink" "Dotted Ink" "Round Face" "Square Face")

# The overload set: "param1 param2 result".
DECLS=("Ink Face 1000" "Solid Face 2000" "Solid Round 3000" "Dotted Square 4000")

INKS=(Solid Dotted)
FACES=(Round Square)

subtype() { # subtype SUB SUP
    [[ $1 == "$2" ]] && return 0
    local pair
    for pair in "${EXTENDS[@]}"; do
        [[ "$1 $2" == "$pair" ]] && return 0
    done
    return 1
}

applies() { # applies INDEX A B
    local -a p
    read -r -a p <<<"${DECLS[$1]}"
    subtype "$2" "${p[0]}" && subtype "$3" "${p[1]}"
}

# Pointwise subtyping, strict in at least one position. This is the whole
# specificity order, and it is the thing mutation 1 inverts in the compiler.
strictly_below() { # strictly_below INDEX INDEX
    local -a a b
    read -r -a a <<<"${DECLS[$1]}"
    read -r -a b <<<"${DECLS[$2]}"
    [[ "${a[0]} ${a[1]}" == "${b[0]} ${b[1]}" ]] && return 1
    subtype "${a[0]}" "${b[0]}" && subtype "${a[1]}" "${b[1]}"
}

# The single most specific applicable declaration, or nothing if the cell ties.
winner() { # winner A B
    local i j maximal=() other
    for i in "${!DECLS[@]}"; do
        applies "$i" "$1" "$2" || continue
        local beaten=0
        for j in "${!DECLS[@]}"; do
            applies "$j" "$1" "$2" || continue
            if strictly_below "$j" "$i"; then beaten=1; break; fi
        done
        [[ $beaten -eq 0 ]] && maximal+=("$i")
    done
    [[ ${#maximal[@]} -eq 1 ]] || return 1
    other=$(read -r -a p <<<"${DECLS[${maximal[0]}]}"; printf '%s' "${p[2]}")
    printf '%s' "$other"
}

expected_matrix() {
    local ink face
    for ink in "${INKS[@]}"; do
        for face in "${FACES[@]}"; do
            winner "$ink" "$face" || return 1
            printf '\n'
        done
    done
}

# A clean halt: a diagnostic and a nonzero status. 139 is SIGSEGV and 134 is
# SIGABRT, and both mean the fail arm was not reached.
halted_cleanly() { [[ $1 -ne 0 && $1 -ne 139 && $1 -ne 134 ]]; }

# ---------------------------------------------------------------- self test

selftest() {
    printf '== gate self test ==\n'

    if subtype Solid Ink && subtype Solid Solid && ! subtype Ink Solid && ! subtype Solid Face; then
        ok 'the subtype relation is reflexive, directed, and not universal'
    else
        bad 'the subtype relation is reflexive, directed, and not universal'
    fi

    if strictly_below 2 0 && ! strictly_below 0 2 && ! strictly_below 0 0; then
        ok 'specificity is strict and antisymmetric'
    else
        bad 'specificity is strict and antisymmetric'
    fi

    # (Solid, Round) has three applicable declarations and exactly one maximal.
    if [[ $(winner Solid Round) == 3000 ]]; then
        ok '(Solid, Round) resolves to the most specific of its three candidates'
    else
        bad '(Solid, Round) resolves to the most specific of its three candidates' \
            "got: $(winner Solid Round)"
    fi

    # And the row is not constant, which is what makes the matrix a test.
    if [[ $(winner Solid Square) == 2000 && $(winner Dotted Round) == 1000 &&
          $(winner Dotted Square) == 4000 ]]; then
        ok 'all four cells differ, so a collapsed table cannot pass'
    else
        bad 'all four cells differ, so a collapsed table cannot pass'
    fi

    # A symmetric tie has no winner. `f(Solid, Face)` against a hypothetical
    # `f(Ink, Round)`: neither is below the other.
    local saved=("${DECLS[@]}")
    DECLS=("Solid Face 1" "Ink Round 2")
    if winner Solid Round >/dev/null 2>&1; then
        bad 'a symmetric tie is refused rather than resolved'
    else
        ok 'a symmetric tie is refused rather than resolved'
    fi
    DECLS=("${saved[@]}")

    if halted_cleanly 1; then
        ok 'a clean halt is accepted'
    else
        bad 'a clean halt is accepted'
    fi
    local signal
    for signal in 139 134 0; do
        if halted_cleanly "$signal"; then
            bad "status $signal is refused as a clean halt" 'a fault is not a diagnostic'
        else
            ok "status $signal is refused as a clean halt"
        fi
    done
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
    for name in dispatch specificity dottedmethod; do
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

matrix() {
    printf '== the matrix ==\n'
    have "$build/dispatch" 'every cell reaches its own declaration' || return

    local want out
    want=$(expected_matrix) || { bad 'the gate can compute the matrix'; return; }
    # The four cells, then the statically concrete call, then the fields.
    want=$(printf '%s\n%s\n5\nsq\n9\n' "$want" "$(winner Solid Round)")
    out=$("$build/dispatch" 2>&1)
    if [[ $out == "$want" ]]; then
        ok "the 2x2 matrix, a static call and three field reads: $(printf '%s' "$want" | tr '\n' ' ')"
    else
        bad 'every cell reaches its own declaration' \
            "want: $(printf '%s' "$want" | tr '\n' ' ') | got: $(printf '%s' "$out" | tr '\n' ' ')"
    fi
}

runtime_type_wins() {
    printf '== the run-time type decides ==\n'
    have "$build/specificity" 'the run-time type decides, not the static one' || return

    local out
    out=$("$build/specificity" 2>&1)
    # Binding the call to the one declaration applicable at the STATIC type
    # would print 2 2 1. The cell (Solid) has a more specific winner.
    if [[ $out == $'1\n2\n1' ]]; then
        ok 'a Solid behind an Ink still reaches name(Solid)'
    else
        bad 'a Solid behind an Ink still reaches name(Solid)' \
            "want: 1 2 1 | got: $(printf '%s' "$out" | tr '\n' ' ')"
    fi
}

ambiguity() {
    printf '== ambiguity is a compile error ==\n'
    local err status
    err=$("$fortressc" "$repo/fortressc/tests/ambiguous.fss" --emit-ir 2>&1 >/dev/null)
    status=$?
    if [[ $status -ne 1 ]]; then
        bad 'a symmetrically ambiguous call is refused' "status $status: $err"
        return
    fi
    if [[ $err == *"is ambiguous for (OL, OR)"* ]]; then
        ok 'a symmetrically ambiguous call is refused, naming the tuple'
    else
        bad 'a symmetrically ambiguous call is refused, naming the tuple' "$err"
    fi
    if [[ $err =~ declarations\ at\ [0-9]+\.\.[0-9]+\ and\ [0-9]+\.\.[0-9]+ ]]; then
        ok 'the diagnostic names both declarations'
    else
        bad 'the diagnostic names both declarations' "$err"
    fi
}

shape() {
    printf '== the emitted shape ==\n'
    local ir
    if ! ir=$("$fortressc" "$repo/fortressc/tests/dispatch.fss" --emit-ir 2>/dev/null); then
        bad 'the dispatch program emits IR'
        return
    fi

    if [[ $ir == *"switch i32 %tag"* && $ir == *"@fortress_dispatch_failed"* ]]; then
        ok 'dispatch is a tag load and a switch, with a fail arm per switch'
    else
        bad 'dispatch is a tag load and a switch, with a fail arm per switch'
    fi

    # One fail arm per switch, not one per function: a shared arm would use a
    # tag that is not defined on every path into it.
    local switches fails
    switches=$(printf '%s' "$ir" | grep -c 'switch i32 %tag')
    fails=$(printf '%s' "$ir" | grep -c 'call void @fortress_dispatch_failed')
    if [[ $switches -eq $fails && $switches -eq 3 ]]; then
        ok "three switches, three fail arms"
    else
        bad 'one fail arm per switch' "switches=$switches fails=$fails"
    fi

    if [[ $ir != *indirectbr* && $ir == *'call i32 @"draw$Solid_Round"'* ]]; then
        ok 'every leaf is a direct call, so the callees stay inlinable'
    else
        bad 'every leaf is a direct call, so the callees stay inlinable'
    fi

    if [[ $ir == *"call ptr @fortress_object_alloc"* && $ir != *GC_malloc* && $ir != *@malloc* ]]; then
        ok 'objects allocate through the shim and generated code names no allocator'
    else
        bad 'objects allocate through the shim and generated code names no allocator'
    fi
}

# --------------------------------------------------------------- mutations
#
# A gate is not trusted until it has refused. Each mutation is applied to the
# real source, the compiler is rebuilt, the gate is run, and the run is required
# to FAIL. The tree has to be clean first, and it is restored either way.


# A dotted method lifts to a function whose parameter 0 is the receiver, so
# single dispatch is M3c's symmetric dispatch with nothing added. Three facts
# fall out of that and are asserted here rather than assumed: an override beats
# an inherited default because Object <: Trait; an unimplemented abstract method
# has no winner for its tag; and a method never collides with a function of the
# same name, because the two namespaces are separate.
methods() {
    printf '== dotted methods ==\n'
    have "$build/dottedmethod" 'a dotted method dispatches on its receiver' || return

    local out want
    out=$("$build/dottedmethod" 2>&1)
    want=$(printf '1\n0\n7\n14\n')
    if [[ $out == "$want" ]]; then
        ok "override beats default and a method reads its fields: $(printf '%s' "$out" | tr '\n' ' ')"
    else
        bad 'a dotted method dispatches on its receiver' \
            "want: $(printf '%s' "$want" | tr '\n' ' ') | got: $(printf '%s' "$out" | tr '\n' ' ')"
    fi

    local err status
    err=$("$fortressc" "$repo/fortressc/tests/badabstract.fss" --emit-obj -o /dev/null 2>&1 >/dev/null)
    status=$?
    if [[ $status -eq 1 && $err == *"no declaration of \`noise\` applies"* ]]; then
        ok 'an unimplemented abstract method has no winner for its tag (exit 1)'
    else
        bad 'an unimplemented abstract method is refused' "status $status: $err"
    fi

    err=$("$fortressc" "$repo/fortressc/tests/baddiamond.fss" --emit-obj -o /dev/null 2>&1 >/dev/null)
    status=$?
    if [[ $status -eq 1 && $err == *"is ambiguous for (Cat)"* ]]; then
        ok 'two defaulted methods with no most specific one are refused by name (exit 1)'
    else
        bad 'an ambiguous diamond is refused' "status $status: $err"
    fi
}

MUTATIONS=(
  'crates/types/src/lib.rs|strictly_below(&a.params, &b.params, registry)|strictly_below(&b.params, &a.params, registry)|invert the specificity comparison'
  'crates/codegen/src/lib.rs|.build_switch(tag, fail, &cases)|.build_switch(tag, fail, &cases[..cases.len().saturating_sub(1)])|drop the last case from every switch'
  'crates/types/src/lib.rs|if maximal.len() != 1 {|if false {|accept a tie instead of reporting it'
  'crates/types/src/lib.rs|concrete: m.body.is_some(),|concrete: true,|let a bodiless declaration be a dispatch target'
  'crates/types/src/lib.rs|let ty = field.ty;|let ty = Type::Void;|stop giving a receiver field its real type in a method body'
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
            compile >/dev/null 2>&1
            matrix; runtime_type_wins; ambiguity; shape; methods
            if [[ $failed -gt 0 ]]; then
                printf 'REFUSED  %d check(s) failed, which is the point\n' "$failed"
                if [[ $label == drop* ]]; then
                    halt_is_clean
                fi
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

# Under the dropped-case mutation the program reaches an arm no tag can match.
# It has to halt with a diagnostic, not fault.
halt_is_clean() {
    local err status
    err=$("$build/dispatch" 2>&1 >/dev/null)
    status=$?
    if halted_cleanly "$status" && [[ $err == *"no declaration of draw"* ]]; then
        printf '         and the unreachable arm halted cleanly (status %s): %s\n' "$status" "$err"
    else
        printf '         BUT the unreachable arm did not halt cleanly (status %s): %s\n' \
            "$status" "$err"
    fi
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
        matrix
        runtime_type_wins
        ambiguity
        shape
        methods
        ;;
esac

printf '\n%d/%d\n' "$passed" "$failed"
[[ $failed -eq 0 ]]
