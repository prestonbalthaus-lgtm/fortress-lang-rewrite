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
fortressc=${FORTRESSC:-$repo/fortressc/target/debug/fortressc}
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
    for name in generics genericdispatch genericoverload; do
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

# The ordering contract, which is what determinism rests on: instantiations are
# emitted at their template's position, sorted by mangled name. A build can be
# perfectly reproducible and still have the wrong tags, so this is checked
# separately from the two-build comparison.
ordering() {
    printf '== instantiations are emitted in source-then-name order ==\n'
    local ir order
    ir=$("$fortressc" "$repo/fortressc/tests/genericdispatch.fss" --emit-ir 2>/dev/null)
    order=$(printf '%s' "$ir" | grep -oE '^%"?[A-Za-z$0-9]+"? = type' | sed 's/ = type//; s/"//g; s/^%//' | tr '\n' ' ')
    # Box is declared before Dot, and Box's two instantiations sort by name.
    if [[ $order == 'Box$String$e Box$ZZ64$e Dot '* ]]; then
        ok "emission order is a pure function of the source: $order"
    else
        bad 'instantiations are emitted at their template position, sorted by name' \
            "got: $order"
    fi
}

# Tags are switch keys and switch arms follow tag order, so a nondeterministic
# instantiation order is a nondeterministic binary. This guards against genuine
# nondeterminism; `ordering` above guards against being deterministically wrong,
# and it is the one the mutation proves, because introducing real nondeterminism
# on demand is itself unreliable.
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
        "stampceiling|$LIMIT|a generic method that stamps itself larger stops there too" \
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

# A generic overload set has more than one member, and instantiating it must
# produce a ground overload set of the same size. The expander keyed its
# template map by name alone, so all but one member was dropped, and its
# emission loop matched instances by name alone, so the survivor was emitted
# once per source declaration -- which the checker reported as
# `tag$Red$e is defined twice`. Both halves are asserted: the program runs, and
# the IR carries one definition per member per static argument.
overload_set() {
    printf '== a generic overload set instantiates to a ground overload set ==\n'
    have "$build/genericoverload" 'a generic overload set keeps every member' || return

    local out want
    out=$("$build/genericoverload" 2>&1)
    want=$(printf '1\n2\n1\n')
    if [[ $out == "$want" ]]; then
        ok "each member keeps its own body: $(printf '%s' "$out" | tr '\n' ' ')"
    else
        bad 'a generic overload set keeps every member' \
            "want: $(printf '%s' "$want" | tr '\n' ' ') | got: $(printf '%s' "$out" | tr '\n' ' ')"
    fi

    local ir members
    if ! ir=$("$fortressc" "$repo/fortressc/tests/genericoverload.fss" --emit-ir 2>/dev/null); then
        bad 'the overload program emits IR'
        return
    fi
    # Two members at two static arguments is four definitions. Counting them is
    # the assertion: dropping a member and double-emitting one both move it.
    members=$(printf '%s' "$ir" | grep -c 'define .*@"tag[$]')
    if [[ $members -eq 4 ]]; then
        ok "two members at two static arguments emit $members definitions"
    else
        bad 'two members at two static arguments emit 4 definitions' "found $members"
    fi
}

# The legacy library's own source breaks the uniformity rule 1.0 states, so the
# rule is suspended FOR THAT SOURCE and for nothing else. Both directions are
# asserted here, because either alone passes with the scope wrong: the accept
# alone passes if the exemption is universal, and the refusal alone passes if
# the exemption does not exist.
uniformity_exemption() {
    printf '== the uniformity exemption is scoped to the legacy library ==\n'
    local fixture=$repo/fortressc/tests/copiedcond.fsi
    local outside=$build/copiedcond.fsi
    local inside=$build/Library/copiedcond.fsi
    mkdir -p "$build/Library"
    cp "$fixture" "$outside"
    cp "$fixture" "$inside"

    local err status
    err=$(timeout 300 "$fortressc" "$outside" --emit-obj -o /dev/null 2>&1 >/dev/null)
    status=$?
    if refused_cleanly "$status" && [[ $err == *'differ in their static parameters'* ]]; then
        ok 'the same declarations outside a Library directory are refused'
    else
        bad 'the same declarations outside a Library directory are refused' "status $status: $err"
    fi

    err=$(timeout 300 "$fortressc" "$inside" --emit-obj -o /dev/null 2>&1 >/dev/null)
    status=$?
    if [[ $status -eq 0 ]]; then
        ok 'inside a Library directory the rule is suspended'
    else
        bad 'inside a Library directory the rule is suspended' "status $status: $err"
    fi

    err=$(timeout 300 "$fortressc" "$inside" --no-legacy-library-uniformity \
            --emit-obj -o /dev/null 2>&1 >/dev/null)
    status=$?
    if refused_cleanly "$status" && [[ $err == *'differ in their static parameters'* ]]; then
        ok 'forcing the exemption off restores the refusal inside Library'
    else
        bad 'forcing the exemption off restores the refusal inside Library' "status $status: $err"
    fi
}

# --------------------------------------------------------------- mutations

MUTATIONS=(
  # THE GROWING-MEMBER CUT. Weaken the detector and every such member is walked
  # again -- `nestedInst.fss` goes back to being killed by the allocator rather
  # than refused, because each round DOUBLES the mangled spelling.
  # `grows = true;` is the target because it is the only line in the detector
  # with no vertical bar in it: this table splits on IFS='|' and the call site
  # carries a closure.
  'crates/types/src/mono.rs|grows = true;|grows = false;|never detect a member that demands its owner larger'
  # AND THE ARROW HALF, which the row above cannot reach. Inverting the guard
  # rewrites the signatures of GENERIC methods and leaves non-generic ones
  # alone -- the exact swap that reported `unknown type E` on a static
  # parameter.
  'crates/types/src/closure.rs|Member::Method(m) if !m.static_params.is_empty() => {}|Member::Method(m) if m.static_params.is_empty() => {}|rewrite generic method signatures and skip non-generic ones'
  'crates/types/src/mono.rs|for ((mangled, slot), instance) in &self.instances {|for ((mangled, slot), instance) in self.instances.iter().rev() {|emit instantiations in reverse name order'
  # RE-TARGETED when the legacy-library exemption landed: the call is now
  # inside `if uniformity == Uniformity::Enforced`, so the old pattern reported
  # COULD NOT BE APPLIED. Inverting the test is the one-token change that keeps
  # every binding used -- `if false` would leave `uniformity` unread.
  'crates/types/src/mono.rs|if uniformity == Uniformity::Enforced {|if uniformity != Uniformity::Enforced {|stop enforcing the uniformity rule'
  # THE EXEMPTION MUST NOT REACH OUTSIDE THE LEGACY LIBRARY. Without this row
  # the exemption is guarded by nothing: `badoverload.fss` is refused whether
  # the scope is a path test or `true`, because it is not in a Library
  # directory either way -- so only a fixture that IS outside and a mutation
  # that widens the scope can tell the two apart.
  'crates/driver/src/main.rs|if LEGACY_LIBRARY_DIRS.contains(&name) {|if true {|widen the legacy-library exemption to every path'
  # AND THE OTHER DIRECTION, which the row above cannot reach. Widening is
  # caught by `badoverload.fss` alone -- it is outside a Library directory, so
  # widening makes it compile. NARROWING is caught by nothing except the
  # fixture inside one, which is the whole reason that fixture exists.
  'crates/driver/src/main.rs|const LEGACY_LIBRARY_DIRS: [&str; 2] = ["Library", "CompilerLibrary"];|const LEGACY_LIBRARY_DIRS: [&str; 2] = ["NoSuchDirectory", "CompilerLibrary"];|narrow the legacy-library exemption so no path matches'
  # RE-TARGETED at the consolidation. `self.discharge_bounds(component)?;` has
  # TWO hits since api check mode landed -- `run` and `check_api` both call it --
  # so the row reported COULD NOT BE APPLIED. Mutating the LOOP INSIDE the
  # function is unique and kills both call sites at once, which is what the row
  # meant in the first place.
  'crates/types/src/lib.rs|        for obligation in &component.bounds {|        for obligation in &Vec::<fortress_ast::BoundObligation>::new() {|stop discharging bound obligations'
  'crates/types/src/mono.rs|if instance.origin == name && instance.member == member {|if instance.origin == name {|emit every instance once per source declaration of its name'
  'crates/types/src/mono.rs|for (member, template) in templates.iter().enumerate() {|for (member, template) in templates.iter().enumerate().take(1) {|instantiate only the first member of an overload set'
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
            overload_set
            ordering
            determinism
            refusals
            uniformity_exemption
            growing_member
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

growing_member() {
    printf '== a member that demands its owner larger is filed, not walked ==\n'
    local out err status exe=$build/growingmember

    # (a) THE ORDINARY MEMBER STILL WORKS. Walking the growing signature is
    #     what starts the chain; nothing else about the trait is affected.
    err=$(timeout 300 "$fortressc" "$repo/fortressc/tests/growingmember.fss" \
            -o "$exe" 2>&1 >/dev/null)
    status=$?
    out=$("$exe" 2>/dev/null)
    if [[ $status -eq 0 && $out == 7 ]]; then
        ok 'an ordinary member of a growing trait compiles, links and runs'
    else
        bad 'an ordinary member of a growing trait compiles, links and runs' \
            "status $status, out [$out]: $err"
    fi

    # (b) AND CALLING THE CUT MEMBER NAMES THE MECHANISM. `has no field` is
    #     what it said before Component::cuts was carried, and that names the
    #     wrong thing on a member the source plainly declares.
    err=$(timeout 300 "$fortressc" "$repo/fortressc/tests/growingmembercall.fss" \
            --emit-obj -o /dev/null 2>&1 >/dev/null)
    status=$?
    if refused_cleanly "$status" && [[ $err == *'properly contain its own'* ]] \
        && [[ $err != *'has no field'* ]]; then
        ok 'calling the filed member is refused, and the diagnostic names it'
    else
        bad 'calling the filed member is refused, and the diagnostic names it' \
            "status $status: $err"
    fi

    # (c) THE CORPUS WITNESS. It exists to test exactly this and says so.
    err=$(timeout 300 "$fortressc" "$repo/ProjectFortress/tests/nestedInst.fss" \
            -o "$build/nestedInst" 2>&1 >/dev/null)
    status=$?
    out=$("$build/nestedInst" 2>/dev/null | tr '\n' ' ')
    if [[ $status -eq 0 && $out == 'Starting instantiation 1 ' ]]; then
        ok 'nestedInst.fss compiles and prints the answer its source names'
    else
        bad 'nestedInst.fss compiles and prints the answer its source names' \
            "status $status, out [$out]: $err"
    fi

    # (d) A GENERIC METHOD'S ARROW MAY NAME ITS OWNER'S STATIC PARAMETER. A
    #     static parameter is a NAME, not a type; rewriting an unstamped
    #     signature reported `unknown type E` against a type never meant to
    #     exist. Pre-existing, and the growing-member cut is what reached it.
    err=$(timeout 300 "$fortressc" "$repo/fortressc/tests/arrowstaticparam.fss" \
            --emit-obj -o /dev/null 2>&1 >/dev/null)
    status=$?
    if [[ $status -eq 0 ]]; then
        ok 'body: E->R inside a generic method on a generic trait is well formed'
    else
        bad 'body: E->R inside a generic method on a generic trait is well formed' \
            "status $status: $err"
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
        stamping
        overload_set
        unboxing
        dispatching
        ordering
        determinism
        refusals
        uniformity_exemption
        growing_member
        ;;
esac

printf '\n%d/%d\n' "$passed" "$failed"
[[ $failed -eq 0 ]]
