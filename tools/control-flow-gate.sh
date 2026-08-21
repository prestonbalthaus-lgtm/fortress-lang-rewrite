#!/usr/bin/env bash
#
# The M6 control-flow gate: `case`, `typecase`, and `label`/`exit`.
#
# Six things cargo cannot check on its own: that a `typecase` lowers to a SWITCH
# on the object tag rather than a chain of comparisons, that a `case` subject is
# evaluated EXACTLY ONCE, that an unmatched `case` with no `else` HALTS with a
# diagnostic instead of falling through, that `label`/`exit` adds NO runtime
# call at all, and that the three refusals this feature owes -- an `exit`
# crossing an `atomic`, an `exit` leaving a `for` body, and a `label` body that
# runs off the bottom -- are refusals and not accidents.
#
# THE ATOMIC ONE IS THE OBLIGATION, NOT A NICETY. `atomic.tex:59-70`'s
# rollback-on-abrupt-completion was recorded as UNREACHABLE rather than
# violated, and its writes-RETAINED arm re-opens the moment `label`/`exit`
# lands -- which is now. A branch out of an `atomic` region skips the unlock and
# one process-wide recursive mutex stays held, which atomic-gate mutation 4
# already measured as a timeout. Refusing it by name is how that rule fails
# loudly instead of quietly.
#
#   ./tools/control-flow-gate.sh              run the gate
#   ./tools/control-flow-gate.sh --selftest   only prove the assertions can refuse
#   ./tools/control-flow-gate.sh --mutate     break the compiler five ways and
#                                             prove the gate refuses each one
#
# FORTRESSC pins the binary. KEEP THE PINNED COPY OUTSIDE fortressc/build/ --
# that directory is shared and seven gates `rm -rf` it. FORTRESSC and --mutate
# do not mix and --mutate refuses when it is set; see the guard below.
set -uo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
build=$repo/fortressc/build/controlflow
fortressc=${FORTRESSC:-$repo/fortressc/target/debug/fortressc}
export LLVM_SYS_221_PREFIX=${LLVM_SYS_221_PREFIX:-$HOME/.local/opt/llvm22-root/usr/lib64/llvm22}
export CPATH=${CPATH:-$HOME/.local/opt/gc-root/usr/include}
export LIBRARY_PATH=${LIBRARY_PATH:-$HOME/.local/opt/gc-root/usr/lib64}

# Measured on the merged codegen lane, not copied from a design note.
CONTROLFLOW_EXPECTED='circle 3
square 4
something else
12
25
0
one
two
many
11
-1
100
-1'

passed=0
failed=0
ok()  { passed=$((passed + 1)); printf 'ok    %s\n' "$1"; }
bad() { failed=$((failed + 1)); printf 'FAIL  %s\n' "$1"; [[ -n ${2:-} ]] && printf '      %s\n' "$2"; }

# ---------------------------------------------------------------- assertions

# A diagnostic is exit 1 and nothing else. 70 is EXIT_INTERNAL_ERROR, 101 is a
# Rust panic, 139 is SIGSEGV: all three mean the compiler broke rather than
# reported. 0 means it accepted a program it should have refused.
refused_cleanly() { [[ $1 -eq 1 ]]; }

# Counting is the whole assertion for `caseonce`, so it is its own function and
# it is self tested.
occurrences() { grep -c -F -- "$2" <<<"$1"; }

# A refusal that does not NAME its mechanism is the class this project already
# paid an hour for. Every negative below asserts the text as well as the code.
names_mechanism() { grep -q -F -- "$2" <<<"$1"; }

selftest() {
    printf '== gate self test ==\n'

    if refused_cleanly 1; then ok 'exit 1 is a clean refusal'
    else bad 'exit 1 is a clean refusal'; fi
    for status in 0 70 101 139; do
        if refused_cleanly "$status"; then
            bad "status $status is refused as a clean refusal" \
                'only exit 1 is a diagnostic; the rest are compiler bugs'
        else
            ok "status $status is not a clean refusal"
        fi
    done

    if [[ $(occurrences $'a\nb\na' 'a') -eq 2 ]]; then ok 'occurrences counts lines'
    else bad 'occurrences counts lines'; fi
    if [[ $(occurrences $'evaluated\nmany' 'evaluated') -eq 1 ]]; then
        ok 'occurrences finds exactly one'
    else bad 'occurrences finds exactly one'; fi
    if [[ $(occurrences $'many' 'evaluated') -eq 0 ]]; then
        ok 'occurrences returns 0 when absent'
    else bad 'occurrences returns 0 when absent'; fi
    # The one that matters: twice must not read as once.
    if [[ $(occurrences $'evaluated\nevaluated\nmany' 'evaluated') -eq 1 ]]; then
        bad 'occurrences reports 1 for two occurrences' 'the once-only assertion would be blind'
    else ok 'two occurrences do not read as one'; fi

    if names_mechanism 'x leaves an `atomic` region y' 'leaves an `atomic` region'; then
        ok 'names_mechanism finds its substring'
    else bad 'names_mechanism finds its substring'; fi
    if names_mechanism 'some other diagnostic' 'leaves an `atomic` region'; then
        bad 'names_mechanism matched an unrelated message'
    else ok 'names_mechanism refuses an unrelated message'; fi

    # The fixtures this gate is nothing without.
    for f in controlflow caseonce caseunmatched badexitatomic badexitloop \
             badlabelfall badtypecasedead; do
        if [[ -f $repo/fortressc/tests/$f.fss ]]; then ok "fixture $f.fss is present"
        else bad "fixture $f.fss is present" 'the gate asserts nothing without it'; fi
    done

    printf '\n%d passed, %d failed\n' "$passed" "$failed"
    [[ $failed -eq 0 ]]
}

# ------------------------------------------------------------------ the gate

compile_to() {   # <fixture> <out>  -> stderr on stdout, exit code preserved
    "$fortressc" "$repo/fortressc/tests/$1.fss" -o "$2" 2>&1 >/dev/null
}

emit_ir() {      # <fixture> -> the IR on stdout
    "$fortressc" "$repo/fortressc/tests/$1.fss" --emit-ir 2>/dev/null
}

run_gate() {
    mkdir -p "$build"
    printf '== control flow: case, typecase, label/exit ==\n'

    # -- 1. the positive fixture, end to end
    if err=$(compile_to controlflow "$build/controlflow"); then
        out=$("$build/controlflow" 2>&1); rc=$?
        if [[ $rc -eq 0 ]]; then ok 'controlflow.fss runs and exits 0'
        else bad 'controlflow.fss runs and exits 0' "exit $rc"; fi
        if [[ $out == "$CONTROLFLOW_EXPECTED" ]]; then
            ok 'controlflow.fss prints all thirteen expected lines'
        else
            bad 'controlflow.fss prints all thirteen expected lines' \
                "got: $(tr '\n' ' ' <<<"$out")"
        fi
    else
        bad 'controlflow.fss compiles' "$err"
    fi

    # -- 2. the case subject is evaluated EXACTLY ONCE
    if compile_to caseonce "$build/caseonce" >/dev/null; then
        out=$("$build/caseonce" 2>&1)
        n=$(occurrences "$out" 'evaluated')
        if [[ $n -eq 1 ]]; then ok 'a `case` subject is evaluated exactly once'
        else bad 'a `case` subject is evaluated exactly once' "evaluated $n time(s)"; fi
    else
        bad 'caseonce.fss compiles'
    fi

    # -- 3. an unmatched `case` with no `else` HALTS
    if compile_to caseunmatched "$build/caseunmatched" >/dev/null; then
        out=$("$build/caseunmatched" 2>&1); rc=$?
        if [[ $rc -ne 0 ]]; then ok 'an unmatched `case` halts with a non-zero exit'
        else bad 'an unmatched `case` halts with a non-zero exit' \
                 'it fell through and exited 0'; fi
        if names_mechanism "$out" 'no case arm matched and there is no `else`'; then
            ok 'the halt names its mechanism'
        else bad 'the halt names its mechanism' "got: $out"; fi
        if names_mechanism "$out" 'unreachable'; then
            bad 'the program continued past the unmatched case' \
                'the halt did not halt'
        else ok 'nothing after the unmatched case runs'; fi
    else
        bad 'caseunmatched.fss compiles'
    fi

    # -- 4. the three refusals this feature owes, plus the dead arm
    check_refusal badexitatomic   'leaves an `atomic` region' \
        'an `exit` crossing an `atomic` boundary'
    check_refusal badexitloop     'leaves a `for` body' \
        'an `exit` leaving a parallel `for` body'
    check_refusal badlabelfall    'may not also run off the bottom' \
        'a `label` body that falls off the bottom'
    check_refusal badtypecasedead 'can never run' \
        'a `typecase` arm an earlier arm already claims'

    # -- 5. the LOWERING, which is what an exit-code check cannot see
    ir=$(emit_ir controlflow)
    if grep -q 'switch i32 %tag' <<<"$ir"; then
        ok 'a `typecase` lowers to a switch on the object tag'
    else
        bad 'a `typecase` lowers to a switch on the object tag' \
            'a chain of comparisons is O(arms) and loses the M3c tag dispatch'
    fi
    if grep -q '^label.end' <<<"$ir" && grep -q 'phi i32' <<<"$ir"; then
        ok '`label`/`exit` lowers to blocks and a phi'
    else
        bad '`label`/`exit` lowers to blocks and a phi'
    fi
    # `fortress_case_failed` is the ONE runtime call this feature adds, and it
    # belongs only on the no-`else` path. Every `case` in controlflow.fss has an
    # `else`, so a call here means the halt is being emitted unconditionally.
    n=$(grep -c 'call void @fortress_case_failed' <<<"$ir")
    if [[ $n -eq 0 ]]; then
        ok 'control flow with an `else` everywhere adds no runtime call'
    else
        bad 'control flow with an `else` everywhere adds no runtime call' \
            "$n call(s) to fortress_case_failed"
    fi
    n=$(grep -c 'call void @fortress_case_failed' <<<"$(emit_ir caseunmatched)")
    if [[ $n -eq 1 ]]; then ok 'the no-`else` path emits exactly one halt'
    else bad 'the no-`else` path emits exactly one halt' "$n call(s)"; fi

    printf '\n%d passed, %d failed\n' "$passed" "$failed"
    [[ $failed -eq 0 ]]
}

check_refusal() {   # <fixture> <expected substring> <what it is>
    local err rc
    err=$("$fortressc" "$repo/fortressc/tests/$1.fss" --emit-obj -o /dev/null 2>&1 >/dev/null)
    rc=$?
    if refused_cleanly $rc; then ok "$3 is refused"
    else bad "$3 is refused" "exit $rc"; fi
    if names_mechanism "$err" "$2"; then ok "$3 -- the diagnostic names it"
    else bad "$3 -- the diagnostic names it" "got: $(head -1 <<<"$err")"; fi
}

# ------------------------------------------------------------------ mutations
#
# THE TABLE IS SPLIT ON IFS='|' so no field may contain a vertical line, and
# every `from` must match EXACTLY ONCE -- both checked below rather than
# assumed. Re-run this after any milestone AND after `cargo fmt`.
MUTATIONS=(
  "crates/types/src/lib.rs|if self.atomic_depth > atomic_depth {|if false {|the exit-crosses-atomic guard is removed|badexitatomic"
  "crates/types/src/lib.rs|if self.loop_ctx.len() > loop_depth {|if false {|the exit-leaves-a-loop-body guard is removed|badexitloop"
  "crates/types/src/lib.rs|return Err(TypeError::LabelFallsThrough {|let _m = (TypeError::LabelFallsThrough {|the label-falls-off-the-bottom guard is removed|badlabelfall"
  "crates/types/src/lib.rs|if tags.is_empty() {|if false {|the dead-typecase-arm guard is removed|badtypecasedead"
  "runtime/shims.c|    fortress_abnormal_exit();\n}\n\nchar *to_string_zz32|    return;\n}\n\nchar *to_string_zz32|the unmatched-case halt returns instead of halting|caseunmatched"
)

# FORTRESSC AND --mutate DO NOT MIX, and the failure is silent. Four of the five
# mutations rebuild fortressc/target/debug; if FORTRESSC points anywhere else the
# gate keeps reading the pinned binary, the mutation has no effect, the assertion
# holds, and the table reports a clean escape.
mutate_needs_the_built_compiler() {
    local built=$repo/fortressc/target/debug/fortressc
    if [[ $fortressc != "$built" ]]; then
        printf 'refusing --mutate: FORTRESSC is %s\n' "$fortressc" >&2
        printf 'but four of five mutations rebuild %s.\n' "$built" >&2
        printf 'A pinned binary makes each one a silent no-op. Unset FORTRESSC.\n' >&2
        exit 2
    fi
}

mutate() {
    mutate_needs_the_built_compiler
    # Against HEAD, not against the index: a gate that rewinds to the index will
    # faithfully put a DEFECT back if anything staged during the run.
    if ! git -C "$repo" diff --quiet HEAD -- fortressc; then
        printf 'refusing to mutate: fortressc/ differs from HEAD\n' >&2
        exit 2
    fi
    ( cd "$repo/fortressc" && cargo build --workspace ) || exit 2

    local entry file from to label fixture hits broken=0 survived=0 refused=0
    for entry in "${MUTATIONS[@]}"; do
        IFS='|' read -r file from to label fixture <<<"$entry"
        from=$(printf '%b' "$from"); to=$(printf '%b' "$to")
        printf '\n== mutation: %s ==\n' "$label"

        hits=$(python3 - "$repo/fortressc/$file" "$from" <<'PY'
import sys, pathlib
print(pathlib.Path(sys.argv[1]).read_text().count(sys.argv[2]))
PY
)
        if [[ $hits -ne 1 ]]; then
            printf 'BROKEN  the pattern matches %s time(s), not once\n' "$hits"
            broken=$((broken + 1)); continue
        fi
        python3 - "$repo/fortressc/$file" "$from" "$to" <<'PY'
import sys, pathlib
p = pathlib.Path(sys.argv[1])
p.write_text(p.read_text().replace(sys.argv[2], sys.argv[3], 1))
PY
        ( cd "$repo/fortressc" && cargo build --workspace >/dev/null 2>&1 )

        # The fixture must stop being refused, or stop halting.
        if [[ $fixture == caseunmatched ]]; then
            "$fortressc" "$repo/fortressc/tests/$fixture.fss" -o "$build/mut" >/dev/null 2>&1
            "$build/mut" >/dev/null 2>&1; local rc=$?
            if [[ $rc -eq 0 ]]; then
                printf 'refused  the gate would catch this: the halt exits %s\n' "$rc"
                refused=$((refused + 1))
            else
                printf 'ESCAPED  the halt still exits %s\n' "$rc"
                survived=$((survived + 1))
            fi
        else
            "$fortressc" "$repo/fortressc/tests/$fixture.fss" --emit-obj -o /dev/null >/dev/null 2>&1
            local rc=$?
            # CAUGHT MEANS "NO LONGER A CLEAN REFUSAL", NOT "NOW COMPILES", and
            # the first draft of this table had it wrong. Two of these guards
            # are load bearing for codegen as well as for the diagnostic, so
            # removing them yields exit 70 -- an internal error -- rather than
            # exit 0. The gate's own `refused_cleanly` accepts only exit 1, so
            # it goes red on 70 too; a mutate check stricter than the gate it
            # tests reports a catch as an escape.
            if [[ $rc -ne 1 ]]; then
                printf 'refused  the gate would catch this: %s.fss exits %s, not 1\n' \
                    "$fixture" "$rc"
                refused=$((refused + 1))
            else
                printf 'ESCAPED  %s.fss is still cleanly refused (exit 1)\n' "$fixture"
                survived=$((survived + 1))
            fi
        fi
        git -C "$repo" checkout HEAD -- "fortressc/$file"
    done
    ( cd "$repo/fortressc" && cargo build --workspace >/dev/null 2>&1 )

    printf '\n%d mutations, %d refused, %d survived, %d could not be applied\n' \
        "${#MUTATIONS[@]}" "$refused" "$survived" "$broken"
    git -C "$repo" diff --quiet HEAD -- fortressc || {
        printf 'TREE NOT RESTORED -- inspect git status before trusting this run\n' >&2
        exit 2
    }
    [[ $survived -eq 0 && $broken -eq 0 ]]
}

case ${1:-} in
    --selftest) selftest ;;
    --mutate)   mutate ;;
    '')         if [[ ! -x $fortressc ]]; then
                    printf 'no compiler at %s -- cargo build first\n' "$fortressc" >&2
                    exit 2
                fi
                run_gate ;;
    *)          printf 'unknown argument %s\n' "$1" >&2; exit 2 ;;
esac
