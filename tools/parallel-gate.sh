#!/usr/bin/env bash
#
# The M4 gate: parallel for loops.
#
# Five things cargo cannot check on its own: that a million-element array filled
# in parallel is byte for byte what a serial fill produces, that the static
# partition covers every index exactly once, that an allocation-free body really
# gets faster on more cores, that an ALLOCATING body does too -- which is the
# collector rebuild's whole justification -- and that `seq(...)` still runs in
# order above the size at which everything else is distributed.
#
# The partition is COMPUTED here, in bash, from (lo, hi, workers) and nothing
# else, then compared against what the runtime says it did. A gate that asks the
# runtime what it split and then agrees with it is checking nothing.
#
#   ./tools/parallel-gate.sh              run the gate
#   ./tools/parallel-gate.sh --selftest   only prove the assertions can refuse
#   ./tools/parallel-gate.sh --mutate     break the compiler six ways and prove
#                                         the gate refuses each one
set -uo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
build=$repo/fortressc/build
fortressc=$repo/fortressc/target/debug/fortressc
export LLVM_SYS_221_PREFIX=${LLVM_SYS_221_PREFIX:-$HOME/.local/opt/llvm22-root/usr/lib64/llvm22}
export CPATH=${CPATH:-$HOME/.local/opt/gc-root/usr/include}
export LIBRARY_PATH=${LIBRARY_PATH:-$HOME/.local/opt/gc-root/usr/lib64}

# The speedup floors, as tenths, so bash can compare them without floating point.
FREE_SPEEDUP_TENTHS=20     # >2.0x on an allocation-free body
ALLOC_SPEEDUP_TENTHS=11    # >1.1x on an allocating body
WORKERS=8

passed=0
failed=0
ok()  { passed=$((passed + 1)); printf 'ok    %s\n' "$1"; }
bad() { failed=$((failed + 1)); printf 'FAIL  %s\n' "$1"; [[ -n ${2:-} ]] && printf '      %s\n' "$2"; }

refused_cleanly() { [[ $1 -eq 1 ]]; }

# Milliseconds of wall clock for one run, so a speedup is integer arithmetic.
millis() {
    local start end
    start=$(date +%s%N)
    "$@" >/dev/null 2>&1
    end=$(date +%s%N)
    echo $(( (end - start) / 1000000 ))
}

# The partition, computed independently of the runtime. Prints one
# "start end" line per worker.
expected_partition() {  # expected_partition LO HI WORKERS
    local lo=$1 hi=$2 workers=$3 total base extra begin count w
    total=$((hi - lo)); base=$((total / workers)); extra=$((total % workers))
    for (( w = 0; w < workers; w++ )); do
        if (( w < extra )); then begin=$((lo + base * w + w)); count=$((base + 1))
        else                     begin=$((lo + base * w + extra)); count=$base; fi
        echo "$begin $((begin + count))"
    done
}

selftest() {
    printf '== gate self test ==\n'

    if refused_cleanly 1; then ok 'exit 1 is a clean refusal'; else bad 'exit 1 is a clean refusal'; fi
    local status
    for status in 0 70 101 139; do
        if refused_cleanly "$status"; then
            bad "status $status is refused as a clean refusal" 'only exit 1 is a diagnostic'
        else
            ok "status $status is refused as a clean refusal"
        fi
    done

    # The partition arithmetic, checked against a hand-worked case: 10 over 4
    # workers is 3,3,2,2 and the boundaries must touch.
    local got want
    got=$(expected_partition 0 10 4 | tr '\n' '|')
    want='0 3|3 6|6 8|8 10|'
    if [[ $got == "$want" ]]; then
        ok "the gate's own partition arithmetic is right: $got"
    else
        bad "the gate's own partition arithmetic" "want $want got $got"
    fi

    # And that it notices a partition that drops an index.
    got=$(expected_partition 0 10 3 | awk '{s+=$2-$1} END {print s}')
    if [[ $got -eq 10 ]]; then
        ok 'a partition of 10 over 3 workers still covers 10 indices'
    else
        bad 'the coverage check can count' "got $got"
    fi

    # A speedup comparison that has to be able to fail.
    if [[ $((1000 * 10)) -ge $((100 * FREE_SPEEDUP_TENTHS)) ]]; then
        ok 'a 10x speedup passes the 2x floor'
    else
        bad 'a 10x speedup passes the 2x floor'
    fi
    if [[ $((100 * 10)) -ge $((100 * FREE_SPEEDUP_TENTHS)) ]]; then
        bad 'a 1x speedup passes the 2x floor' 'the floor cannot refuse anything'
    else
        ok 'a 1x speedup is refused by the 2x floor'
    fi
}

preflight() {
    printf '== preflight ==\n'
    if [[ -x $fortressc ]]; then ok 'the compiler is built'; else
        bad 'the compiler is built' "no binary at $fortressc"; exit 1; fi
    rm -rf "$build"; mkdir -p "$build"
}

compile() {
    printf '== compile ==\n'
    local name
    for name in parallelfill parallelcollatz parallelalloc parallelseq; do
        if "$fortressc" "$repo/fortressc/tests/$name.fss" -o "$build/$name" 2>"$build/$name.err"; then
            ok "$name.fss compiles and links"
        else
            bad "$name.fss compiles and links" "$(cat "$build/$name.err")"
        fi
    done
}

have() {
    if [[ -f $1 ]]; then return 0; fi
    bad "$2" "no artifact at $1"; return 1
}

# THE correctness assertion. Not a checksum and not a sample: the whole array,
# in index order, hashed. Any slot written by the wrong iteration, written
# twice, or not written at all changes the hash.
identical() {
    printf '== a million elements, filled in parallel ==\n'
    have "$build/parallelfill" 'the parallel fill matches the serial one' || return

    local serial parallel four
    serial=$(FORTRESS_WORKERS=1 "$build/parallelfill" | sha256sum | cut -d' ' -f1)
    parallel=$("$build/parallelfill" | sha256sum | cut -d' ' -f1)
    four=$(FORTRESS_WORKERS=4 "$build/parallelfill" | sha256sum | cut -d' ' -f1)

    if [[ $serial == "$parallel" && $serial == "$four" ]]; then
        ok "1 000 000 elements byte identical at 1, 4 and $(nproc) workers (${serial:0:16})"
    else
        bad 'the parallel fill matches the serial one' \
            "serial ${serial:0:16} parallel ${parallel:0:16} four ${four:0:16}"
    fi

    local lines
    lines=$(FORTRESS_WORKERS=1 "$build/parallelfill" | wc -l)
    if [[ $lines -eq 1000000 ]]; then
        ok 'every one of the million slots was written'
    else
        bad 'every slot was written' "got $lines lines"
    fi
}

partition() {
    printf '== the static partition ==\n'
    local runtime=$build/partition
    cat > "$build/partition.c" <<'C'
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
void fortress_parallel_chunk(int64_t, int64_t, int, int, int64_t *, int64_t *);
int main(int argc, char **argv) {
    int64_t lo = atoll(argv[1]), hi = atoll(argv[2]);
    int workers = atoi(argv[3]);
    for (int w = 0; w < workers; w++) {
        int64_t s, e;
        fortress_parallel_chunk(lo, hi, w, workers, &s, &e);
        printf("%lld %lld\n", (long long)s, (long long)e);
    }
    return 0;
}
C
    if ! cc "$build/partition.c" "$repo/fortressc/runtime/shims.c" \
         -I "$CPATH" -o "$runtime" -lgc -lm 2>"$build/partition.err"; then
        bad 'the partition probe builds' "$(cat "$build/partition.err")"
        return
    fi

    local case lo hi workers got want
    for case in "0 1000000 8" "0 10 4" "7 1000010 3" "0 5 8" "100 100000 14"; do
        read -r lo hi workers <<<"$case"
        got=$("$runtime" "$lo" "$hi" "$workers")
        want=$(expected_partition "$lo" "$hi" "$workers")
        if [[ $got == "$want" ]]; then
            ok "[$lo, $hi) over $workers workers splits as the gate computed"
        else
            bad "[$lo, $hi) over $workers workers" \
                "want $(tr '\n' '|' <<<"$want") got $(tr '\n' '|' <<<"$got")"
        fi
    done
}

speedup() {
    printf '== wall clock ==\n'
    local name floor label one many tenths
    while IFS='|' read -r name floor label; do
        have "$build/$name" "$label" || continue
        one=$(FORTRESS_WORKERS=1 millis "$build/$name")
        many=$(FORTRESS_WORKERS=$WORKERS millis "$build/$name")
        if [[ $many -le 0 ]]; then many=1; fi
        tenths=$((one * 10 / many))
        if [[ $tenths -ge $floor ]]; then
            ok "$label: ${one}ms -> ${many}ms on $WORKERS workers, $((tenths / 10)).$((tenths % 10))x"
        else
            bad "$label" "only $((tenths / 10)).$((tenths % 10))x (${one}ms -> ${many}ms), floor $((floor / 10)).$((floor % 10))x"
        fi
    done <<CASES
parallelcollatz|$FREE_SPEEDUP_TENTHS|an allocation-free body scales
parallelalloc|$ALLOC_SPEEDUP_TENTHS|an ALLOCATING body scales, so the collector is parallel
CASES
}

sequential() {
    printf '== seq(...) ==\n'
    have "$build/parallelseq" 'a sequential loop runs in order' || return
    local out
    out=$("$build/parallelseq")
    if [[ $(wc -l <<<"$out") -eq 5000 ]] && awk 'NR-1!=$1 {exit 1}' <<<"$out"; then
        ok 'seq(0#5000) runs in index order, above the inline threshold'
    else
        bad 'a sequential loop runs in order' 'the output is not 0..4999 in order'
    fi
}

refusals() {
    printf '== the scope boundary ==\n'
    local name phrase label err status
    while IFS='|' read -r name phrase label; do
        err=$("$fortressc" "$repo/fortressc/tests/$name.fss" --emit-obj -o /dev/null 2>&1 >/dev/null)
        status=$?
        if refused_cleanly "$status" && [[ $err == *"$phrase"* ]]; then
            ok "$label (exit $status)"
        else
            bad "$label" "status $status: $err"
        fi
    done <<'CASES'
badparallelescape|is declared outside this loop|assigning to an outer binding is refused
badparallelindex|the element its own iteration owns|assigning to a slot this iteration does not own is refused
CASES
}

# ----------------------------------------------------------------- mutations
#
# file|from|to|label. No `|` in any field: the table is split on IFS='|'.

MUTATIONS=(
  'runtime/shims.c|#define GC_THREADS|#define GC_THREADS_DISABLED_BY_MUTATION|stop registering worker threads with the collector'
  'runtime/shims.c|int64_t begin = lo + base * w + (w < extra ? w : extra);|int64_t begin = lo + base * w;|drop the remainder from the partition, so indices are skipped'
  'runtime/shims.c|if (requested == 1 |if (0 |hand a sequential loop to the pool'
  'runtime/shims.c|    fortress_run_chunk(&task, 0);|    (void)0;|let the calling thread skip its own chunk'
  'crates/types/src/lib.rs|matches!(self.depth_of(name), Some(depth) if depth < floor)|false|let a parallel body assign to a binding outside it'
  'crates/codegen/src/lib.rs|scope.insert(loop_.binder.to_owned(), Slot::Value(index));|scope.insert(loop_.binder.to_owned(), Slot::Value(self.context.i64_type().const_zero().into()));|give every iteration the same index'
)

mutate() {
    if ! git -C "$repo" diff --quiet HEAD -- fortressc/crates fortressc/runtime; then
        printf 'refusing to mutate: the tree differs from HEAD\n' >&2
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
            broken=$((broken + 1)); continue
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
            identical; partition; sequential; refusals
            if [[ $failed -gt 0 ]]; then
                printf 'REFUSED  %d check(s) failed, which is the point\n' "$failed"
            else
                printf 'SURVIVED  the gate did not notice\n'
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
    --selftest) selftest ;;
    --mutate)   selftest; preflight; mutate; exit $? ;;
    *)
        selftest
        preflight
        compile
        identical
        partition
        speedup
        sequential
        refusals
        ;;
esac

printf '\n%d/%d\n' "$passed" "$failed"
[[ $failed -eq 0 ]]
