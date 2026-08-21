#!/usr/bin/env bash
#
# The phase 7 gate: the two claims the whole rewrite exists to make.
#
# Both were met and NEITHER was asserted by anything. Until they were measured
# by hand on 2026-08-21 no fixture in the tree had ever exercised either one:
# the largest reduction was reductionbig.fss at 20,000,000, fifty times short,
# and grepping tools/ for 1000000000 or 2147483 returned two comment lines in
# crates/types/src/lib.rs.
#
#   (i)  a 10^9-iteration reduction scales against a REAL `seq(...)` build, and
#        prints the same answer at every worker count
#   (ii) an Array[\Boolean\] of 3,000,000,000 elements is written and read at
#        index 2,999,999,999 -- past 2^31, which is the JVM ceiling the rewrite
#        exists to break
#
# THE FLOOR IS A REAL `seq(...)` BUILD AND THE GATE PROVES IT IS ONE.
# FORTRESS_WORKERS=1 is NOT a sequential build: it reaches the pool, takes the
# inline branch, and still carries the private-accumulator addressing and the
# 16-row merge around it. The two differ by about 1% in wall clock, so a
# stopwatch cannot tell them apart -- which is why the difference is asserted in
# the IR instead. A seq build has ZERO `fortress_reduction_alloc` CALL sites and
# passes `i64 1` to fortress_parallel_for; the parallel build has exactly one
# and passes `i64 0`. Mutation M2 exists to make that an assertion rather than
# a comment.
#
# THE ANSWER IS COMPUTED HERE, from the closed form (n-1)n(2n-1)/6 mod 2^64,
# and never read back out of the subject.
#
# The array half's WALL TIME is deliberately NOT gated: nine tenths of its 1.0 s
# is the kernel faulting and zeroing 3 GB of anonymous pages, which is a
# property of the host, not of the compiler. Exit status, the three printed
# values and an RSS band are what mean something.
#
# Every number here is an -O0 number, like every performance number in this
# project: the driver links with no -O flag at all, so the runtime's own hot
# loop is at cc's default too.
#
#   ./tools/phase7-gate.sh              run the gate
#   ./tools/phase7-gate.sh --selftest   only prove the assertions can refuse
#   ./tools/phase7-gate.sh --mutate     break the compiler six ways and prove
#                                       the gate refuses each one
#
# Do NOT run this concurrently with the other gates: the array fixture is a
# single 3 GB scanned Boehm block.
set -uo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
build=$repo/fortressc/build
fortressc=${FORTRESSC:-$repo/fortressc/target/debug/fortressc}
export LLVM_SYS_221_PREFIX=${LLVM_SYS_221_PREFIX:-$HOME/.local/opt/llvm22-root/usr/lib64/llvm22}
export CPATH=${CPATH:-$HOME/.local/opt/gc-root/usr/include}
export LIBRARY_PATH=${LIBRARY_PATH:-$HOME/.local/opt/gc-root/usr/lib64}

# n for the reduction, and the worker counts swept. FORTRESS_WORKERS outside
# [1,16] is silently ignored by the runtime, and it is read ONCE per process,
# so the sweep is over separate runs and stays inside the range.
N=1000000000
WORKERS="1 2 4 8 14"

# Tenths, so the comparison is integer arithmetic -- the idiom parallel-gate
# uses. Measured 8.8x on an idle box and 6.9x under load; 4.0 has real headroom
# and still refuses mutation M1, which collapses the ratio to about 1.0.
SPEEDUP_TENTHS=40

# One byte per Boolean element, so 3e9 elements is ~2.93e6 KB. Measured
# 2,938,112 and 2,938,484. The band survives ordinary variation and still
# refuses mutation M6, which doubles the element size to ~5.86e6.
ELEMENTS=3000000000
RSS_MIN_KB=2900000
RSS_MAX_KB=3200000

# A hang guard, never a measurement.
HANG_TIMEOUT=300

passed=0
failed=0
ok()  { passed=$((passed + 1)); printf 'ok    %s\n' "$1"; }
bad() { failed=$((failed + 1)); printf 'FAIL  %s\n' "$1"; [[ -n ${2:-} ]] && printf '      %s\n' "$2"; }

# ---------------------------------------------------------------- assertions

# The oracle, from the closed form and from nothing the program produced.
# python3 rather than bash because the intermediate (n-1)n(2n-1) is about
# 2 * 10^27 and bash arithmetic is a signed 64 bit int.
expected_sum() { python3 -c "n=$1; print(((n - 1) * n * (2 * n - 1) // 6) % (2**64))"; }

at_least_ratio() {  # at_least_ratio SLOW_MS FAST_MS TENTHS
    local slow=$1 fast=$2 tenths=$3
    (( fast > 0 )) || return 1
    (( slow * 10 >= fast * tenths ))
}

within_band() { (( $1 >= $2 && $1 <= $3 )); }

# Wall clock AND exit status AND stdout, together. Timing a command whose exit
# status is thrown away lets a mutant that halts instantly be recorded as very
# fast and pass a speedup floor.
declare -g LAST_MS LAST_OUT LAST_STATUS
timed() {
    local start end
    start=$(date +%s%N)
    LAST_OUT=$(timeout "$HANG_TIMEOUT" "$@" 2>&1)
    LAST_STATUS=$?
    end=$(date +%s%N)
    LAST_MS=$(( (end - start) / 1000000 ))
}

selftest() {
    printf '== gate self test ==\n'

    # The oracle against an independently known value, so neither can drift
    # alone: 10^9 is the phase 7 number, and 10 is small enough to check by hand
    # (0+1+4+9+16+25+36+49+81 = 285).
    if [[ $(expected_sum "$N") == 3338615082255021824 ]]; then
        ok 'the closed form reproduces the phase 7 answer'
    else
        bad 'the closed form reproduces the phase 7 answer' "$(expected_sum "$N")"
    fi
    if [[ $(expected_sum 10) == 285 ]]; then
        ok 'the closed form is right at a size that can be checked by hand'
    else
        bad 'the closed form is right at a size that can be checked by hand'
    fi

    if at_least_ratio 880 100 "$SPEEDUP_TENTHS"; then
        ok 'a 8.8x speedup clears the floor'
    else
        bad 'a 8.8x speedup clears the floor'
    fi
    if at_least_ratio 880 800 "$SPEEDUP_TENTHS"; then
        bad 'a 1.1x speedup is refused' 'the ratio comparator cannot see a collapsed speedup'
    else
        ok 'a 1.1x speedup is refused'
    fi
    if at_least_ratio 880 0 "$SPEEDUP_TENTHS"; then
        bad 'a zero-millisecond run is refused' 'it would divide by zero into a pass'
    else
        ok 'a zero-millisecond run is refused'
    fi

    if within_band 2938112 "$RSS_MIN_KB" "$RSS_MAX_KB"; then
        ok 'the measured RSS is inside the band'
    else
        bad 'the measured RSS is inside the band'
    fi
    if within_band 5859383 "$RSS_MIN_KB" "$RSS_MAX_KB"; then
        bad 'a doubled element size is refused' 'the band cannot see a 2x allocation'
    else
        ok 'a doubled element size is refused'
    fi

    timed true
    if [[ $LAST_STATUS -eq 0 ]]; then
        ok 'the timer reports a clean exit'
    else
        bad 'the timer reports a clean exit'
    fi
    timed false
    if [[ $LAST_STATUS -eq 0 ]]; then
        bad 'the timer reports a failed exit' 'a crash would be timed as very fast and pass'
    else
        ok 'the timer reports a failed exit'
    fi
}

# ------------------------------------------------------------------ the gate

preflight() {
    ( cd "$repo/fortressc" && cargo build --workspace ) || exit 2
    rm -rf "$build"
    mkdir -p "$build"
    local name
    for name in seqreductionhuge reductionhuge arrayhuge; do
        if ! "$fortressc" "$repo/fortressc/tests/$name.fss" -o "$build/$name" \
            2>"$build/$name.err"; then
            printf 'FAIL  %s.fss does not compile\n      %s\n' "$name" "$(cat "$build/$name.err")"
            failed=$((failed + 1))
        fi
    done
}

# The claim that the floor is a real sequential build, asserted where it is
# actually visible. Captured into a variable and matched with `==`, never piped
# into `grep -q`: under `set -o pipefail` the producer takes SIGPIPE and the
# pipeline reports failure when the assertion succeeded.
seq_is_really_sequential() {
    printf '== the floor is a real seq(...) build ==\n'
    local ir
    ir=$("$fortressc" "$repo/fortressc/tests/seqreductionhuge.fss" --emit-ir 2>/dev/null)
    if [[ $ir == *'call ptr @fortress_reduction_alloc'* ]]; then
        bad 'the seq build allocates no accumulator block' \
            'it took the private-accumulator path, so it is not a serial floor'
    else
        ok 'the seq build allocates no accumulator block'
    fi
    if [[ $ir == *"@fortress_parallel_for(i64 0, i64 $N, ptr @\"\$loop1\", ptr %fortress_env_alloc, i64 1)"* ]]; then
        ok 'the seq build asks the runtime for the sequential path'
    else
        bad 'the seq build asks the runtime for the sequential path' 'the trailing flag is not 1'
    fi

    ir=$("$fortressc" "$repo/fortressc/tests/reductionhuge.fss" --emit-ir 2>/dev/null)
    if [[ $ir == *'call ptr @fortress_reduction_alloc(i64 1, i64 64)'* ]]; then
        ok 'the parallel build allocates one accumulator block'
    else
        bad 'the parallel build allocates one accumulator block'
    fi
    if [[ $ir == *"@fortress_parallel_for(i64 0, i64 $N, ptr @\"\$loop1\", ptr %fortress_env_alloc, i64 0)"* ]]; then
        ok 'the parallel build asks the runtime to distribute'
    else
        bad 'the parallel build asks the runtime to distribute' 'the trailing flag is not 0'
    fi
}

# Correctness first, at every worker count, because a partition that loses or
# double-counts indices is CORRECT AT ONE WORKER and wrong at more.
reduction_answer() {
    printf '== 10^9 iterations, same answer at every worker count ==\n'
    local want w
    want=$(expected_sum "$N")

    timed "$build/seqreductionhuge"
    if [[ $LAST_STATUS -eq 0 && $LAST_OUT == "$want" ]]; then
        ok "the seq build prints $want"
    else
        bad 'the seq build prints the closed form' "status $LAST_STATUS: $LAST_OUT"
    fi

    for w in $WORKERS; do
        FORTRESS_WORKERS=$w timed "$build/reductionhuge"
        if [[ $LAST_STATUS -eq 0 && $LAST_OUT == "$want" ]]; then
            ok "$w worker(s): $want"
        else
            bad "$w worker(s) print the closed form" "status $LAST_STATUS: $LAST_OUT"
        fi
    done
}

speedup() {
    printf '== and it is faster than the sequential build ==\n'
    local slow fast want
    want=$(expected_sum "$N")

    timed "$build/seqreductionhuge"
    if [[ $LAST_STATUS -ne 0 || $LAST_OUT != "$want" ]]; then
        bad 'the timed seq run is the right computation' "status $LAST_STATUS: $LAST_OUT"
        return
    fi
    slow=$LAST_MS

    FORTRESS_WORKERS=14 timed "$build/reductionhuge"
    if [[ $LAST_STATUS -ne 0 || $LAST_OUT != "$want" ]]; then
        bad 'the timed parallel run is the right computation' "status $LAST_STATUS: $LAST_OUT"
        return
    fi
    fast=$LAST_MS

    if at_least_ratio "$slow" "$fast" "$SPEEDUP_TENTHS"; then
        ok "seq ${slow}ms -> 14 workers ${fast}ms (floor ${SPEEDUP_TENTHS}/10 x)"
    else
        bad 'the parallel build beats the sequential one' \
            "seq ${slow}ms, 14 workers ${fast}ms, floor ${SPEEDUP_TENTHS}/10 x"
    fi
}

# The JVM ceiling, which is 2^31 indexing and is the reason this rewrite exists.
past_two_to_the_thirty_one() {
    printf '== a 3e9-element array, written and read past 2^31 ==\n'
    timed "$build/arrayhuge"
    if [[ $LAST_STATUS -ne 0 ]]; then
        bad 'arrayhuge runs to completion' "status $LAST_STATUS: $LAST_OUT"
        return
    fi
    local want
    want=$(printf '%s\ntrue\nfalse' "$ELEMENTS")
    if [[ $LAST_OUT == "$want" ]]; then
        ok "length $ELEMENTS, index $((ELEMENTS - 1)) reads back true, index 0 reads back false"
    else
        bad 'the array reports its length and both elements' "$LAST_OUT"
    fi

    # peak-rss.py suppresses stdout only, and refuses a non-zero exit itself.
    local rss
    rss=$(python3 "$repo/tools/peak-rss.py" "$build/arrayhuge" 2>&1)
    if [[ ! $rss =~ ^[0-9]+$ ]]; then
        bad 'peak RSS is measurable' "$rss"
        return
    fi
    if within_band "$rss" "$RSS_MIN_KB" "$RSS_MAX_KB"; then
        ok "peak RSS ${rss} KB is one byte per element (band ${RSS_MIN_KB}..${RSS_MAX_KB})"
    else
        bad 'peak RSS is one byte per element' \
            "${rss} KB is outside ${RSS_MIN_KB}..${RSS_MAX_KB}"
    fi
}

# ----------------------------------------------------------------- mutations
#
# file|from|to|label. No `|` in any field: the table is split on IFS='|'.
# A mutant that does not build counts as BROKEN and fails the table, which is
# parallel-gate's convention rather than atomic-gate's -- the two disagree, and
# a mutation that cannot be applied tells you nothing about the gate.

MUTATIONS=(
  'runtime/shims.c|#define FORTRESS_PARALLEL_MIN 4096|#define FORTRESS_PARALLEL_MIN 4000000000|never distribute the loop, so the answer stays right and only the speedup dies'
  'crates/types/src/lib.rs|let recognised = !sequential|let recognised = true|give the seq build a private accumulator, so the floor is no longer a serial build'
  'runtime/shims.c|        task->body(i, task->env, w);|        task->body(i, task->env, 0);|every worker accumulates into row 0'
  'runtime/shims.c|void *fortress_array_slot(void *array, long long index) {|void *fortress_array_slot(void *array, int index) {|put the 2^31 indexing ceiling back by hand'
  'runtime/shims.c|    return ((const FortressArray *)array)->length;|    return (int)((const FortressArray *)array)->length;|truncate the length to 32 bits'
  'crates/types/src/types.rs|            Self::Boolean => 1,|            Self::Boolean => 2,|double the size of a Boolean element'
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
            passed=0; failed=0
            preflight
            seq_is_really_sequential; reduction_answer; speedup; past_two_to_the_thirty_one
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
        seq_is_really_sequential
        reduction_answer
        speedup
        past_two_to_the_thirty_one
        ;;
esac

printf '\n%d/%d\n' "$passed" "$failed"
[[ $failed -eq 0 ]]
