#!/usr/bin/env bash
#
# The M2a gate. Everything cargo cannot reach: a real link against a real
# OpenMPI, and real ranks.
#
# The compiler runs on this host. The link and the launch run inside
# apptainer/fortress-mpi.sif, because the binary is linked against Rocky's C
# library and MPI, not Fedora's. Nothing produced here is ever executed on the
# host; that would fail for reasons that look like gate bugs.
#
#   ./tools/mpi-gate.sh              run the gate
#   ./tools/mpi-gate.sh --selftest   only prove the assertions can refuse
set -uo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
sif=${FORTRESS_MPI_SIF:-$repo/apptainer/fortress-mpi.sif}
build=$repo/fortressc/build
fortressc=${FORTRESSC:-$repo/fortressc/target/debug/fortressc}
export LLVM_SYS_221_PREFIX=${LLVM_SYS_221_PREFIX:-$HOME/.local/opt/llvm22-root/usr/lib64/llvm22}
# runtime/shims.c includes <gc.h> and every link pulls in -lgc, so the host
# link needs the collector's headers the same way it needs LLVM's.
export CPATH=${CPATH:-$HOME/.local/opt/gc-root/usr/include}
export LIBRARY_PATH=${LIBRARY_PATH:-$HOME/.local/opt/gc-root/usr/lib64}
export FORTRESS_MPI_SIF=$sif

passed=0
failed=0
ok()  { passed=$((passed + 1)); printf 'ok    %s\n' "$1"; }
bad() { failed=$((failed + 1)); printf 'FAIL  %s\n' "$1"; [[ -n ${2:-} ]] && printf '      %s\n' "$2"; }

# ---------------------------------------------------------------- assertions

# Every rank prints one line and mpirun interleaves them, so order proves
# nothing. Compare the multiset instead: a missing rank, a duplicated rank and
# a wrong world size all fail it.
rank_set_matches() {
    local want_np=$1 output=$2 expected actual
    expected=$(for ((r = 0; r < want_np; r++)); do printf 'rank %d of %d\n' "$r" "$want_np"; done | sort)
    actual=$(printf '%s\n' "$output" | grep -E '^rank [0-9]+ of [0-9]+$' | sort)
    [[ $actual == "$expected" ]]
}

# ------------------------------------------------------------ the self test
# The assertion is only worth anything if it can refuse. Both directions, so a
# function that always returns 0 and one that always returns 1 both fail here.
selftest() {
    printf '== gate self test ==\n'
    local two
    two=$(printf 'rank 0 of 2\nrank 1 of 2\n')

    if rank_set_matches 2 "$two"; then
        ok 'the assertion accepts a correct two rank set'
    else
        bad 'the assertion accepts a correct two rank set'
    fi

    if rank_set_matches 3 "$two"; then
        bad 'the assertion refuses two ranks presented as three' 'it accepted a wrong world size'
    else
        ok 'the assertion refuses two ranks presented as three'
    fi

    if rank_set_matches 2 "$(printf 'rank 0 of 2\nrank 0 of 2\n')"; then
        bad 'the assertion refuses a duplicated rank' 'it accepted rank 0 twice'
    else
        ok 'the assertion refuses a duplicated rank'
    fi

    if rank_set_matches 2 "$(printf 'rank 0 of 2\n')"; then
        bad 'the assertion refuses a missing rank' 'it accepted a short set'
    else
        ok 'the assertion refuses a missing rank'
    fi
}

# ------------------------------------------------------------------ the gate

preflight() {
    if [[ ! -f $sif ]]; then
        printf 'no image at %s\n' "$sif" >&2
        printf 'build it: apptainer build --fakeroot %s %s\n' \
            "$repo/apptainer/fortress-mpi.sif" "$repo/apptainer/fortress-mpi.def" >&2
        exit 2
    fi
    ( cd "$repo/fortressc" && cargo build --workspace ) || exit 2
    # Wiped, not reused. A compile that fails leaves the previous run's binary
    # behind, and every check after it would then be measuring a stale artifact
    # and reporting it as a live result.
    rm -rf "$build"
    mkdir -p "$build"
}

compile_and_link() {
    printf '== compile and link ==\n'

    if "$fortressc" "$repo/fortressc/tests/mpi_hello.fss" \
        -o "$build/mpi_hello" --cc "$repo/tools/mpicc-in-image.sh" 2>"$build/mpi_hello.err"; then
        ok 'mpi_hello.fss links through the image mpicc'
    else
        bad 'mpi_hello.fss links through the image mpicc' "$(cat "$build/mpi_hello.err")"
    fi

    if "$fortressc" "$repo/fortressc/tests/fact.fss" -o "$build/fact" 2>"$build/fact.err"; then
        ok 'the M1 program still links with the host cc'
    else
        bad 'the M1 program still links with the host cc' "$(cat "$build/fact.err")"
    fi

    if "$fortressc" "$repo/fortressc/tests/mpi_hello.fss" \
        --emit-obj -o "$build/mpi_hello.o" 2>"$build/obj.err"; then
        ok '--emit-obj writes an object without linking'
    else
        bad '--emit-obj writes an object without linking' "$(cat "$build/obj.err")"
    fi
}

# Anything downstream of a failed compile would be measuring nothing, or worse,
# something left over.
have() {
    if [[ -f $1 ]]; then
        return 0
    fi
    bad "$2" "no artifact at $1"
    return 1
}

symbols() {
    printf '== symbols ==\n'

    have "$build/mpi_hello.o" 'the object defers to the fortress_mpi_ shim' || return
    have "$build/mpi_hello" 'the linked MPI program pulls in libmpi' || return
    have "$build/fact" 'a program that never calls MPI carries no MPI symbol' || return

    local undefined
    undefined=$(nm -u "$build/mpi_hello.o" 2>/dev/null | awk '{print $NF}' | sort)
    if printf '%s\n' "$undefined" | grep -qx 'fortress_mpi_comm_rank'; then
        ok 'the object defers to the fortress_mpi_ shim'
    else
        bad 'the object defers to the fortress_mpi_ shim' "undefined: $(printf '%s' "$undefined" | tr '\n' ' ')"
    fi

    # The whole reason mpi_shims.c exists. Generated code must name no MPI
    # symbol of its own, whatever the local MPI calls things.
    if printf '%s\n' "$undefined" | grep -qE '^(MPI_|ompi_|PMPI_)'; then
        bad 'generated code names no MPI symbol directly' \
            "$(printf '%s\n' "$undefined" | grep -E '^(MPI_|ompi_|PMPI_)' | tr '\n' ' ')"
    else
        ok 'generated code names no MPI symbol directly'
    fi

    if apptainer exec "$sif" ldd "$build/mpi_hello" | grep -q 'libmpi'; then
        ok 'the linked MPI program pulls in libmpi'
    else
        bad 'the linked MPI program pulls in libmpi'
    fi

    if nm "$build/fact" 2>/dev/null | grep -qiE 'mpi'; then
        bad 'a program that never calls MPI carries no MPI symbol' \
            "$(nm "$build/fact" | grep -i mpi | tr '\n' ' ')"
    else
        ok 'a program that never calls MPI carries no MPI symbol'
    fi

    if ldd "$build/fact" | grep -qiE 'jvm|libjava|libmpi'; then
        bad 'the M1 no-JVM guarantee holds' "$(ldd "$build/fact" | tr '\n' ' ')"
    else
        ok 'the M1 no-JVM guarantee holds'
    fi
}

warnings() {
    printf '== the shims compile clean ==\n'
    local out
    out=$(apptainer exec "$sif" mpicc -c -Wall -Wextra -Wpedantic -std=c11 \
        -o /dev/null "$repo/fortressc/runtime/mpi_shims.c" 2>&1)
    if [[ -z $out ]]; then
        ok 'mpi_shims.c is clean under -Wall -Wextra -Wpedantic'
    else
        bad 'mpi_shims.c is clean under -Wall -Wextra -Wpedantic' "$out"
    fi
}

ranks() {
    printf '== ranks ==\n'

    have "$build/mpi_hello" 'the ranks run at all' || return

    # The singleton control: no launcher at all. MPI_Init succeeds on its own
    # and reports a world of one, which is what makes the -np results below
    # mean something rather than being a constant the program could have
    # printed without MPI.
    local singleton
    singleton=$(apptainer exec "$sif" "$build/mpi_hello" 2>&1)
    if rank_set_matches 1 "$singleton"; then
        ok 'singleton control: no mpirun, one rank'
    else
        bad 'singleton control: no mpirun, one rank' "$singleton"
    fi

    local np output
    for np in 1 2 4; do
        output=$(apptainer exec "$sif" mpirun -np "$np" --oversubscribe "$build/mpi_hello" 2>&1)
        if rank_set_matches "$np" "$output"; then
            ok "mpirun -np $np reports ranks 0..$((np - 1)) of $np"
        else
            bad "mpirun -np $np reports ranks 0..$((np - 1)) of $np" "$output"
        fi
    done
}

# ----------------------------------------------------------------- main

if [[ ${1:-} == --selftest ]]; then
    selftest
else
    selftest
    preflight
    compile_and_link
    symbols
    warnings
    ranks
fi

printf '\n%d/%d\n' "$passed" "$failed"
[[ $failed -eq 0 ]]
