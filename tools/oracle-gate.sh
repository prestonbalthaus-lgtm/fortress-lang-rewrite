#!/usr/bin/env bash
#
# The oracle gate: what the legacy implementation asserted, checked against
# what this compiler does. THIS IS THE PHASE-0 ARTIFACT.
#
# ROADMAP phase 0's exit is "the legacy interpreter runs ProjectFortress/tests/
# and the pass/fail set is recorded in the repo". No such set exists -- the JVM
# path was cancelled as a side effect of the no-JVM decision and the ROADMAP was
# never amended. But the pass/fail set was never only in the JVM: 373 `.test`
# files record it on disk, in java.util.Properties format, and 264 of them carry
# the exact compile error the legacy implementation produced. That is an oracle,
# it needs no JVM, and unlike a one-off count it grows with the compiler.
#
# THREE PARTS, AND THE THIRD IS INDEPENDENT OF THE FIRST TWO.
#
#   A. THE CASES. Every name in every `tests=`, resolved to its source, run
#      against the directives and expectations of its `.test`. Reported as
#      three numbers that mean three different things:
#        pass     we reached a verdict and AGREED with the oracle
#        fail     we reached a verdict and DISAGREED -- a wrong answer
#        blocked  we could not reach a verdict, because a feature is missing
#      A missing feature is not a wrong answer, and collapsing the two is how a
#      compile metric ends up counting programs that print the wrong thing.
#
#   B. THE MUST-FAIL RATCHET. A case with a non-empty `compile_err_equals` is a
#      program the legacy implementation REFUSED. We accept 47 of them today.
#      The ratchet is a LIST and not a count: tools/oracle-accepted-must-fail.txt
#      names all 47, a new acceptance outside the list is red, and a file that
#      starts being refused must be removed from the list in the same commit.
#
#   C. THE SIGNAL SWEEP. Every corpus file that compiles is linked and RUN. Exit
#      must be 0 or 1 and never a signal. Nothing in this repository had ever
#      executed a corpus program before this gate: the compile metric only ever
#      checked that the driver exited 0.
#
#   ./tools/oracle-gate.sh              run the gate
#   ./tools/oracle-gate.sh --selftest   only prove the assertions can refuse
#   ./tools/oracle-gate.sh --mutate     break the oracle six ways and prove the
#                                       gate refuses each one
#   ./tools/oracle-gate.sh --cases      per-case verdicts, TSV
#   ./tools/oracle-gate.sh --skip-run   parts A and B only, no linking
#   ./tools/oracle-gate.sh --json
#
# FORTRESSC pins the binary. Set it when other work is rebuilding the tree, or
# the sweep silently mixes two compilers. KEEP THE PINNED COPY OUTSIDE
# fortressc/build/ -- that directory is shared, and another gate wiped a pin
# out of it mid-session. Every report stamps the repo SHA AND the binary's
# sha256, because with a pin those two are different facts.
set -uo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
export LLVM_SYS_221_PREFIX=${LLVM_SYS_221_PREFIX:-$HOME/.local/opt/llvm22-root/usr/lib64/llvm22}
export CPATH=${CPATH:-$HOME/.local/opt/gc-root/usr/include}
export LIBRARY_PATH=${LIBRARY_PATH:-$HOME/.local/opt/gc-root/usr/lib64}

# RR64 reassociation is deterministic per worker count and not across them, so
# an output comparison without this is flaky by design, not by accident.
export FORTRESS_WORKERS=${FORTRESS_WORKERS:-1}

# ------------------------------------------------------------------ mutations
#
# A GATE IS NOT TRUSTED UNTIL IT HAS REFUSED. Six mutations, each one a real
# weakening of something this gate asserts, each restored FROM HEAD and not
# from the index -- restoring from the index faithfully restores a defect if
# anything stages mid-run. Written as functions rather than as an IFS table on
# purpose: this gate's subject matter is full of vertical lines and a table
# split on `|` cannot carry them.
if [[ ${1:-} == --mutate ]]; then
    fortressc=${FORTRESSC:-$repo/fortressc/target/debug/fortressc}
    cd "$repo" || exit 2

    # WHY THIS GATE'S VERSION OF THE GUARD READS DIFFERENTLY FROM EVERY OTHER
    # ONE. The other eleven mutate `fortressc/crates` and rebuild, so theirs
    # says "a pinned binary makes each mutation a silent no-op". NOT TRUE HERE:
    # none of the six rows below touches a crate and nothing is rebuilt, so a
    # pinned binary would apply every mutation faithfully. The reason the pin
    # is still refused is the EXPECTED NUMBERS. Rows 5 and 6 assert 495 cases
    # and a position relative to `passFloor`, and the comment on row 6 says it
    # outright -- the pass count is a property of TODAY'S COMPILER, and every
    # refusal added anywhere moves it. Point FORTRESSC at yesterday's binary
    # and the rows fail against a baseline that was never theirs.
    mutate_needs_the_built_compiler() {
        local built=$repo/fortressc/target/debug/fortressc
        if [[ $fortressc != "$built" ]]; then
            printf 'refusing --mutate: FORTRESSC is %s\n' "$fortressc" >&2
            printf 'but this table baselines its counts against %s.\n' "$built" >&2
            printf 'A pinned binary fails rows against numbers that are not its own. Unset FORTRESSC.\n' >&2
            exit 2
        fi
    }
    mutate_needs_the_built_compiler

    # AGAINST HEAD, NOT AGAINST THE INDEX, and `restore` below matches. This
    # said `git diff --quiet --` while the comment above claimed HEAD: staged
    # work passed the guard and was then restored away as "clean", and the
    # worktree and the index would agree with each other while both were wrong.
    if ! git diff --quiet HEAD -- tools/ ProjectFortress/compiler_tests/; then
        printf 'the tree differs from HEAD under tools/ or the test dirs;\n'
        printf 'commit or stash before mutating, or a restore will lose work\n' >&2
        exit 2
    fi

    gate() { FORTRESSC=$fortressc ./tools/oracle-gate.sh --skip-run --json 2>/dev/null; }
    gate_full() { FORTRESSC=$fortressc ./tools/oracle-gate.sh --json 2>/dev/null; }
    field() { python3 -c 'import json,sys; d=json.load(sys.stdin); print(eval(sys.argv[1],{},{"d":d}))' "$1"; }
    restore() { git checkout HEAD -- "$@"; }

    mut_pass=0; mut_fail=0; mut_doc=0

    # TRAP 3, AND IT BIT THIS TABLE ON ITS FIRST RUN. A sed whose pattern does
    # not match is a mutation that never happened, and it reports as a clean
    # escape -- mutation 4 below targeted an 8-space-indented line with a
    # 4-space pattern and silently did nothing. So every mutation goes through
    # here, and applying nothing is a hard error rather than a result.
    apply() {
        local file=$1 expr=$2 before after
        before=$(md5sum "$file")
        sed -i "$expr" "$file"
        after=$(md5sum "$file")
        if [[ ${before%% *} == "${after%% *}" ]]; then
            printf 'BROKEN   the mutation did not apply: %s\n         %s\n' \
                "$file" "$expr" >&2
            restore "$file"
            return 1
        fi
    }

    report() {
        if [[ $2 == "$3" ]]; then
            mut_pass=$((mut_pass + 1)); printf 'refused  %s\n         %s\n' "$1" "$4"
        else
            mut_fail=$((mut_fail + 1))
            printf 'ESCAPED  %s\n         expected %s, got %s\n' "$1" "$3" "$2"
        fi
    }
    documented() {
        mut_doc=$((mut_doc + 1))
        printf 'ESCAPED  %s\n         DOCUMENTED: %s\n' "$1" "$2"
    }

    printf '== oracle gate mutation table ==\n'

    # 1. Take a file off the accepted-must-fail list. A program that must fail
    #    and does not is then unaccounted for, and that is the whole ratchet.
    # AGAINST TODAY'S BASELINE AND NOT AGAINST 1. This row asserted 1 outright
    # and escaped at the three-lane merge reporting 8 -- seven acceptances the
    # merge introduced and nobody has signed off were already unaccounted for,
    # so the mutation's own file was the eighth. The mutation was working; the
    # expectation was absolute where the quantity is a DELTA.
    # BY NAME AND NOT BY LINE NUMBER. `5d` was Compiled0.p.fss, which the
    # consolidation's export checking now REFUSES -- so dropping its line
    # changed nothing and the row read as an escape. A row pinned to a position
    # in a file the same commit is allowed to edit is a row that stops testing
    # without saying so.
    base=$(gate | field 'len(d["newAcceptances"])')
    apply tools/oracle-accepted-must-fail.txt '/Compiled9.z.fss/d' || exit 2
    got=$(gate | field 'len(d["newAcceptances"])')
    restore tools/oracle-accepted-must-fail.txt
    report 'a file dropped from the accepted-must-fail list' "$got" "$((base + 1))" \
        "reported as one more than the $base already unaccounted for; gate red"

    # 2. Same for the signal list. This one needs part C, so it is the slow one.
    apply tools/oracle-known-signals.txt '3d' || exit 2
    got=$(gate_full | field 'len(d["newSignals"])')
    restore tools/oracle-known-signals.txt
    report 'a binary dropped from the known-signal list' "$got" 1 \
        'reported as 1 new bad exit; gate red'

    # 3. Corrupt an expectation the compiler currently SATISFIES. Neither list
    #    can see this; only the pass floor can. The target has to be a case
    #    that passes today -- the first draft corrupted Funmet.test, whose case
    #    is BLOCKED on `import java`, and nothing moved. A mutation applied to
    #    a case the gate never reaches is not a mutation.
    # THE ASSERTION IS THE MECHANISM AND NOT THE NUMBER, and this row learned
    # it the same way rows 5 and 6 did. It carried `330` and ESCAPED reporting
    # 335: `pass` is a property of TODAY'S COMPILER and every file that starts
    # compiling moves it, so an absolute figure here stops testing without
    # saying so. What the row is for is that ONE case leaves `pass`, so that is
    # what it says, against a baseline the gate computes.
    base=$(gate | field 'd["outcomes"]["pass"]')
    apply ProjectFortress/compiler_tests/Compiled17.test \
        's/^run_out_equals=pass/run_out_equals=nonesuch/' || exit 2
    got=$(gate | field 'd["outcomes"]["pass"]')
    restore ProjectFortress/compiler_tests/Compiled17.test
    report 'a satisfied expectation corrupted' "$got" "$((base - 1))" \
        "pass fell $base -> $((base - 1)); one case left the passing set"

    # 4. Make `matches` a search rather than a full match, which is what Java
    #    String.matches is NOT.
    # BASELINED RATHER THAN WRITTEN, for the reason row 3 above now carries:
    # this said `331/38` and escaped reporting `336/39` the day more of the
    # corpus started compiling.
    base=$(gate | field 'str(d["outcomes"]["pass"]) + "/" + str(d["outcomes"]["fail"])')
    apply tools/oracle-gate.sh \
        's/^        return re.fullmatch(pattern, text, re.S) is not None$/        return re.search(pattern, text, re.S) is not None/' || exit 2
    got=$(gate | field 'str(d["outcomes"]["pass"]) + "/" + str(d["outcomes"]["fail"])')
    restore tools/oracle-gate.sh
    if [[ $got == "$base" ]]; then
        documented 'matches weakened from fullmatch to search' \
            "nothing moved ($base either way). 36 cases carry a _matches or
         _WImatches expectation and this compiler reaches only 5 of them; all
         5 are satisfied by both readings, so no assertion the suite can make
         separates them today.
         The mutation is kept because that stops being true the moment a
         prefix-matching case is reached, and it is 8 lines from a false
         green if the comparator is ever rewritten"
    else
        report 'matches weakened from fullmatch to search' "$got" "$base" \
            'a case changed verdict'
    fi

    # 5. Read exit 0 as a refusal. Every must-fail program would then pass.
    # THE ASSERTION IS THE MECHANISM AND NOT THE NUMBER, for the same reason
    # row 6 below carries that note. This row carried `39` and ESCAPED the day
    # the list went to 37 -- and it went to 37 because the gate itself had
    # reported two entries as newly refused and its own header says a refused
    # file comes out in the same commit. So a row asserting the list's SIZE is
    # a row that breaks every time the ratchet does its job. What the row is
    # for is that EVERY listed file stops being refused at once, so that is
    # what it says, against a count the gate computes rather than one written
    # here.
    apply tools/oracle-gate.sh 's/^        if code == 1:$/        if code in (0, 1):/' || exit 2
    got=$(gate | field 'str(len(d["nowRefused"])) + "/" + str(d["knownAccepted"])')
    restore tools/oracle-gate.sh
    want=$(gate | field 'str(d["knownAccepted"]) + "/" + str(d["knownAccepted"])')
    report 'exit 0 read as a clean refusal' "$got" "$want" \
        'every listed file reported as no longer refused'

    # 6. Break the Properties continuation so a wrapped `tests=` truncates.
    # THE ASSERTION IS THE MECHANISM AND NOT THE NUMBER. This row carried
    # `495/93` and escaped at the consolidation reporting `495/83`: the case
    # count is a property of THIS READER and holds, but the pass count that
    # survives a truncated read is a property of TODAY'S COMPILER, and every
    # refusal added anywhere moves it. What the row is for is that cases
    # collapse and the floor breaks, so that is what it says.
    apply tools/oracle-gate.sh 's/^    return n % 2 == 1$/    return False/' || exit 2
    got=$(gate | field 'str(d["cases"]) + "/" + ("below-floor" if d["outcomes"]["pass"] < d["passFloor"] else "at-or-above-floor")')
    restore tools/oracle-gate.sh
    report 'line continuation disabled in the .test reader' "$got" 495/below-floor \
        'cases fell 609 -> 495 and pass collapsed far below the floor; gate red'

    printf '\n%d mutations, %d refused, %d documented escape(s), %d unexplained\n' \
        "$((mut_pass + mut_fail + mut_doc))" "$mut_pass" "$mut_doc" "$mut_fail"
    git diff --quiet HEAD -- tools/ ProjectFortress/compiler_tests/ || {
        printf 'TREE NOT RESTORED -- inspect git status before trusting this run\n' >&2
        exit 2
    }
    exit $(( mut_fail > 0 ? 1 : 0 ))
fi

fortressc=${FORTRESSC:-$repo/fortressc/target/debug/fortressc}
if [[ ! -x $fortressc ]]; then
    printf 'no compiler at %s -- cargo build first\n' "$fortressc" >&2
    exit 2
fi
mkdir -p "$repo/fortressc/build/oracle"

cd "$repo" && FORTRESSC=$fortressc \
    ORACLE_SHA=$(git rev-parse --short=9 HEAD 2>/dev/null || echo unknown) \
    ORACLE_CC=$(sha256sum "$fortressc" 2>/dev/null | cut -c1-12 || echo unknown) \
    REFUSE_LIST=$repo/tools/oracle-accepted-must-fail.txt \
    SIGNAL_LIST=$repo/tools/oracle-known-signals.txt \
    DIVERGE_LIST=$repo/tools/oracle-known-divergences.txt \
    BUILD=$repo/fortressc/build/oracle \
    python3 - "$@" <<'PY'
import json, os, re, subprocess, sys, collections, glob, shutil
from concurrent.futures import ThreadPoolExecutor

FORTRESSC   = os.environ['FORTRESSC']
SHA         = os.environ['ORACLE_SHA']
# FORTRESSC pins the binary, so repo HEAD is NOT compiler identity. Both.
CCID        = os.environ.get('ORACLE_CC', 'unknown')
REFUSE_LIST = os.environ['REFUSE_LIST']
SIGNAL_LIST = os.environ['SIGNAL_LIST']
# Cases where THIS COMPILER IS RIGHT AND THE RECORDED EXPECTATION IS NOT. A
# third list and not a fourth bucket of `fail`, because the difference between
# "we produce the wrong answer" and "we produce a different answer on purpose"
# is the whole reason this gate reports three numbers instead of one.
DIVERGE_LIST = os.environ['DIVERGE_LIST']
BUILD       = os.environ['BUILD']

COMPILE_TIMEOUT = 60
RUN_TIMEOUT     = 20

# Measured at the three-lane merge (semantics + codegen + frontend) with
# FORTRESS_WORKERS=1; it was 285 at f81f41ace and the merge took passes to 291.
# The floor is the third
# ratchet and it is the one that catches a case moving pass -> fail, which
# neither list can see: the acceptance list only knows about must-fail
# programs and the signal list only about binaries. Raise it when passes are
# won; never lower it to make a red run green.
# 2026-08-21: 311 -> 310, AND A FLOOR GOING DOWN NEEDS ITS REASON WRITTEN.
# `XXXPreparser.c.fss` left the pass set, and the refusal it lost WAS NEVER
# OURS: until the resolver read the import list, that file merged the WHOLE of
# `List` and died on `unknown type LexicographicOrder`, a name it never
# mentions. Fixing the resolver removed the accident and revealed we accept a
# program 1.0's PREPARSER refuses for unmatched delimiters. The precedent is the
# semantics lane's 291 -> 285, where every lost case was a must-fail being
# wrongly accepted; this is the same shape one step further out -- a PASS being
# wrongly earned.
PASS_FLOOR = 337

args, opt = sys.argv[1:], {}
i = 0
while i < len(args):
    a = args[i]
    if a in ('--jobs',):
        opt[a[2:]] = args[i + 1]; i += 2
    elif a in ('--selftest', '--mutate', '--cases', '--json', '--skip-run',
               '--refresh-lists'):
        opt[a[2:].replace('-', '_')] = True; i += 1
    else:
        print(f'unknown argument {a}', file=sys.stderr); sys.exit(2)
JOBS = int(opt.get('jobs', os.cpu_count() or 4))


# ============================================================ the .test format
#
# `StringMap.FromFileProps` is a java.util.Properties load (FileTests.java:919),
# so this is Properties and not an ad-hoc key=value reader. Getting that wrong
# is not cosmetic: `\ ` is what preserves a leading space in an expected error,
# `\n` is a real newline inside a value, and a natural line continues only on an
# ODD number of trailing backslashes. A hand-rolled splitter silently produced
# five junk keys out of three corpus files, which is how the format was
# identified in the first place.
def load_properties(text):
    out, i, lines = {}, 0, text.split('\n')
    while i < len(lines):
        line = lines[i].lstrip(' \t\f')
        i += 1
        if not line or line[0] in '#!':
            continue
        while _odd_backslashes(line):
            line = line[:-1] + (lines[i].lstrip(' \t\f') if i < len(lines) else '')
            i += 1
        key, val = _split_key(line)
        out[_unescape(key)] = _unescape(val)
    return out


def _odd_backslashes(s):
    n = 0
    while n < len(s) and s[len(s) - 1 - n] == '\\':
        n += 1
    return n % 2 == 1


def _split_key(line):
    i, n = 0, len(line)
    while i < n:
        if line[i] == '\\':
            i += 2; continue
        if line[i] in ' \t\f=:':
            break
        i += 1
    key = line[:i]
    while i < n and line[i] in ' \t\f':
        i += 1
    if i < n and line[i] in '=:':
        i += 1
        while i < n and line[i] in ' \t\f':
            i += 1
    return key, line[i:]


_ESC = {'n': '\n', 't': '\t', 'r': '\r', 'f': '\f'}


def _unescape(s):
    out, i, n = [], 0, len(s)
    while i < n:
        c = s[i]
        if c != '\\':
            out.append(c); i += 1; continue
        i += 1
        if i >= n:
            break
        c = s[i]; i += 1
        if c == 'u' and i + 4 <= n:
            out.append(chr(int(s[i:i + 4], 16))); i += 4
        else:
            out.append(_ESC.get(c, c))
    return ''.join(out)


# =============================================================== the comparators
#
# FileTests.java:141-268, transcribed. Java's String.matches is a FULL match, so
# these are fullmatch and not search. `_equals` collapses runs of spaces and
# tabs on BOTH sides and normalises line endings before comparing; the whole
# whitespace-diagnosis ladder below it in the Java only decides the failure
# MESSAGE, so it is not reproduced.
def _wi(s):
    return re.sub(r'\s+', ' ', s).strip()


def _equals(got, want):
    n = lambda s: re.sub(r'\r\n|\r', '\n', re.sub(r'[ \t]+', ' ', s))
    return n(got) == n(want)


def _fullmatch(text, pattern):
    try:
        return re.fullmatch(pattern, text, re.S) is not None
    except re.error:
        return None            # a pattern Python cannot compile is not a verdict


COMPARATORS = {
    'contains':         lambda got, want: want in got,
    'does_not_contain': lambda got, want: want not in got,
    'matches':          lambda got, want: _fullmatch(got, want),
    'WImatches':        lambda got, want: _fullmatch(_wi(got), want),
    'WCIequals':        lambda got, want: _wi(got).lower() == _wi(want).lower(),
    'equals':           _equals,
    # NOT READ BY THE LEGACY HARNESS. `grep -rn WIcontains ProjectFortress/src`
    # returns nothing, so upstream's 37 cases fell through to the default
    # pass/PASS rule instead. Implemented here because the intent is
    # unambiguous, and counted in the report so the divergence is a printed
    # number rather than a silent one.
    'WIcontains':       lambda got, want: _wi(want) in _wi(got),
}

CHECK = re.compile(r'^(compile|link|run)_(out|err|exception)_(\w+)$')

# ============================================================== the case model
DIRECTIVES = {'compile', 'link', 'run', 'typecheck', 'parse', 'disambiguate',
              'api', 'build'}
# `typecheck` is honoured because this driver type-checks on the way to an
# object file; it cannot STOP there, so a pass on one of these is a stronger
# claim than the directive asked for and the report says so. The other four
# name phases this compiler does not expose at all.
MODELLED   = {'compile', 'link', 'run', 'typecheck'}


def cases():
    out, notests, junk = [], [], collections.Counter()
    for t in sorted(glob.glob('ProjectFortress/*/*.test')):
        p = load_properties(open(t, encoding='utf-8', errors='replace').read())
        for k in p:
            if k not in DIRECTIVES and not CHECK.match(k) and \
               k not in ('tests', 'STATIC_TESTS_DIR', 'PREPARSER_TESTS_DIR', 'arg1'):
                # `arg1` IS NOT MODELLED. Two .test files pass a command-line
                # argument to the binary and this gate runs every binary with
                # none. No case reaches it today -- the 51 fails are 47
                # acceptances plus 4 wrong outputs, all accounted for -- but
                # the day one compiles it will report `the default check
                # run_out_contains=PASS did not hold`, which names the wrong
                # mechanism. Wire the argument in before then.
                junk[f'{t}: {k[:40]}'] += 1
        names = p.get('tests', '').split()
        if not names:
            notests.append(t)
        d = os.path.dirname(t)
        for n in names:
            src = None
            # THE THIRD CANDIDATE IS NOT DECORATION. 24 of the 264 must-fail
            # names already carry their extension (`Compiled5.q.fss`,
            # `Compiled3.w.fsi`), and a resolver that only tries `name + .fss`
            # silently drops all 24. That is exactly the 264-vs-240 and the
            # 47-vs-45 in the gap analysis.
            for c in (os.path.join(d, n + '.fss'), os.path.join(d, n + '.fsi'),
                      os.path.join(d, n)):
                if os.path.isfile(c):
                    src = os.path.normpath(c); break
            out.append({'test': t, 'name': n, 'src': src, 'props': p})
    return out, notests, junk


def expectations(props, phase, stream):
    """The checks declared for one phase and stream, in Java's own order."""
    found = []
    for key, val in props.items():
        m = CHECK.match(key)
        if not m or m.group(1) != phase or m.group(2) != stream:
            continue
        if not val.strip() or m.group(3) not in COMPARATORS:
            continue
        found.append((m.group(3), _expand(val, props)))
    return found


VAR = re.compile(r'\$\{(\w+)\}')


def _expand(text, props):
    """`${STATIC_TESTS_DIR}` and friends. FORTRESS_AUTOHOME is this repo."""
    env = {'FORTRESS_AUTOHOME': os.getcwd(), 'FORTRESS_HOME': os.getcwd()}
    for _ in range(4):
        new = VAR.sub(lambda m: props.get(m.group(1), env.get(m.group(1), m.group(0))),
                      text)
        if new == text:
            break
        text = new
    return text


# ================================================================== running one
def _run(argv, timeout, cwd=None):
    try:
        r = subprocess.run(argv, capture_output=True, text=True, timeout=timeout,
                           stdin=subprocess.DEVNULL, cwd=cwd, errors='replace')
        return r.returncode, r.stdout, r.stderr
    except subprocess.TimeoutExpired:
        return 'timeout', '', ''
    except OSError as e:
        return 'oserror', '', str(e)


def verdict(case, idx):
    """One case -> (outcome, detail). Outcomes are the three the report prints,
    plus `unmodelled` for a directive this gate does not claim to run."""
    p, src = case['props'], case['src']
    if src is None:
        return 'blocked', 'no source file for this name'
    active = set(p) & DIRECTIVES
    if not (active & MODELLED):
        return 'unmodelled', ','.join(sorted(active)) or 'no directive'

    must_fail = bool(p.get('compile_err_equals', '').strip() or
                     p.get('compile_err_contains', '').strip())
    out = os.path.join(BUILD, f'c{idx}')
    wants_binary = 'run' in p and not must_fail

    argv = [FORTRESSC, src] + ([] if wants_binary else ['--emit-obj'])
    argv += ['-o', out if wants_binary else '/dev/null']
    code, _, cerr = _run(argv, COMPILE_TIMEOUT)

    if code == 'timeout':
        return 'fail', f'the compiler timed out after {COMPILE_TIMEOUT}s'
    if code not in (0, 1):
        return 'fail', f'the compiler exited {code}, which is not a diagnostic'

    if must_fail:
        # The oracle says this program does not compile. Refusing it is the
        # whole verdict; the legacy error TEXT is not reproducible here and is
        # not compared. `compile_err_contains` is, because it is a substring.
        if code == 1:
            for kind, want in expectations(p, 'compile', 'err'):
                if kind == 'equals':
                    continue
                ok = COMPARATORS[kind](cerr, want)
                if ok is False:
                    return 'blocked', f'refused, but compile_err_{kind} did not hold'
            return 'pass', 'refused, as the oracle requires'
        return 'fail', 'ACCEPTED a program the legacy implementation refused'

    if code == 1:
        return 'blocked', _first_line(cerr)

    if 'run' not in p:
        return 'pass', 'compiled, and the oracle asked for nothing more'

    if not os.path.isfile(out):
        return 'fail', 'the driver exited 0 and produced no binary'
    rcode, rout, rerr = _run([out], RUN_TIMEOUT)
    try:
        os.unlink(out)
    except OSError:
        pass
    if rcode == 'timeout':
        return 'fail', f'the binary did not finish in {RUN_TIMEOUT}s'
    if isinstance(rcode, int) and rcode < 0:
        return 'fail', f'the binary died on signal {-rcode}'
    if isinstance(rcode, int) and rcode not in (0, 1):
        return 'fail', f'the binary exited {rcode}'

    checked = False
    for stream, text in (('out', rout), ('err', rerr)):
        for kind, want in expectations(p, 'run', stream):
            ok = COMPARATORS[kind](text, want)
            checked = True
            if ok is None:
                return 'blocked', f'run_{stream}_{kind} is not a Python regex'
            if not ok:
                return 'fail', f'run_{stream}_{kind} did not hold'
    if not checked:
        # FileTests.java:266-272, the default when run_out carries no check.
        if 'pass' not in rout and 'PASS' not in rout:
            return 'fail', 'the default check run_out_contains=PASS did not hold'
    return 'pass', 'ran and matched the oracle'


# THE LAST STDERR LINE IS A CARET, NOT A MESSAGE. The driver renders a source
# excerpt under every diagnostic since the semantics lane's line:col work, and
# `note:` lines carry excerpts of their own. This is the THIRD instrument in the
# family -- tools/triage.sh:140-148 was fixed first and tools/api-census.sh
# second -- and the symptom here was the blocked-reason histogram reporting
# `|        ^^^^` 28 times as its most common cause. The regex is copied from
# triage deliberately, so the three cannot disagree about what a file's
# diagnostic was.
HEADER = re.compile(r'^\S+?:\d+:\d+: (?!note: )')
SPAN = re.compile(r'^\S+?:(?: \d+\.\.\d+|\d+:\d+): ')


def _first_line(err):
    lines = err.strip().splitlines()
    header = next((l for l in lines if HEADER.match(l)), None)
    line = header if header is not None else (lines[-1] if lines else '')
    return SPAN.sub('', line)[:96]


# ======================================================= C. the signal sweep
def corpus():
    out = []
    for d, ds, fs in os.walk('.'):
        ds[:] = [x for x in ds if x not in ('.git', 'target', 'fortressc', '.claude')]
        if os.path.normpath(d) == '.':
            ds[:] = [x for x in ds if x != 'examples']
        out += [os.path.normpath(os.path.join(d, f)) for f in fs
                if f.endswith(('.fss', '.fsi'))]
    return sorted(out)


def build_and_run(item):
    idx, path = item
    out = os.path.join(BUILD, f's{idx}')
    code, _, _ = _run([FORTRESSC, path, '-o', out], COMPILE_TIMEOUT)
    if code != 0 or not os.path.isfile(out):
        return None                      # does not compile; part C is not about that
    rcode, rout, rerr = _run([out], RUN_TIMEOUT)
    try:
        os.unlink(out)
    except OSError:
        pass
    if rcode == 'timeout':
        return {'path': path, 'why': f'no exit within {RUN_TIMEOUT}s', 'kind': 'timeout'}
    if isinstance(rcode, int) and rcode < 0:
        return {'path': path, 'why': f'signal {-rcode}', 'kind': 'signal'}
    if isinstance(rcode, int) and rcode not in (0, 1):
        return {'path': path, 'why': f'exit {rcode}', 'kind': 'exit'}
    return {'path': path, 'why': '', 'kind': 'clean'}


def read_list(path):
    if not os.path.exists(path):
        return []
    return [l.split('#')[0].strip() for l in open(path)
            if l.split('#')[0].strip()]


# ==================================================================== selftest
def selftest():
    ok = bad = 0

    def check(name, got, want):
        nonlocal ok, bad
        if got == want:
            ok += 1; print(f'ok    {name}')
        else:
            bad += 1; print(f'FAIL  {name}\n      got {got!r}, want {want!r}')

    print('== oracle gate self test ==')

    # -- the Properties reader. Every case below is a real shape from the corpus.
    P = load_properties
    check('a bare directive is a key with an empty value',
          P('compile\n'), {'compile': ''})
    check('an odd trailing backslash continues the line',
          P('tests=a \\\n   b c\n')['tests'], 'a b c')
    check('an EVEN number of trailing backslashes does not continue',
          P('k=a\\\\\nj=b\n'), {'k': 'a\\', 'j': 'b'})
    check('`\\n` in a value is a real newline',
          P('k=a\\nb\n')['k'], 'a\nb')
    check('`\\ ` preserves a leading space, which is what the error texts need',
          P('k=x\\n\\\n\\ y\\n\n')['k'], 'x\n y\n')
    check('a `#` line is a comment', P('#k=v\nj=w\n'), {'j': 'w'})
    check('a key may be terminated by whitespace instead of `=`',
          P('STATIC_TESTS_DIR ${X}/y\n'), {'STATIC_TESTS_DIR': '${X}/y'})
    check('`${...}` is NOT expanded at load time',
          '${' in P('k=${A}/b\n')['k'], True)

    # -- the comparators, each shown to REFUSE its own near miss.
    C = COMPARATORS
    check('contains holds',            C['contains']('abcd', 'bc'), True)
    check('contains refuses',          C['contains']('abcd', 'xy'), False)
    check('does_not_contain refuses a hit', C['does_not_contain']('abcd', 'bc'), False)
    check('matches is a FULL match, as Java String.matches is',
          C['matches']('abcd', 'b.'), False)
    check('matches holds on a full pattern', C['matches']('abcd', 'a.*d'), True)
    check('matches spans newlines',     C['matches']('a\nb', 'a.b'), True)
    check('WImatches collapses whitespace first',
          C['WImatches']('  2   3 \n', '2 3'), True)
    check('WImatches still refuses a different string',
          C['WImatches']('  2   4 \n', '2 3'), False)
    check('WCIequals ignores case and whitespace',
          C['WCIequals']('  Ok!  ', 'ok!'), True)
    check('WCIequals refuses different text',
          C['WCIequals']('Ok?', 'ok!'), False)
    check('equals collapses runs of spaces and tabs',
          C['equals']('a  \tb\n', 'a b\n'), True)
    check('equals does NOT collapse newlines',
          C['equals']('a\n\nb', 'a\nb'), False)
    check('equals normalises CRLF',      C['equals']('a\r\nb', 'a\nb'), True)
    check('WIcontains collapses whitespace',
          C['WIcontains']('x  1   2 y', '1 2'), True)
    check('an uncompilable pattern is not a verdict',
          C['matches']('x', '('), None)

    # -- the check-key parser
    check('run_out_equals parses as a check',
          bool(CHECK.match('run_out_equals')), True)
    check('`tests` is not a check', bool(CHECK.match('tests')), False)
    check('an unknown comparator is dropped, not guessed',
          expectations({'run_out_nonesuch': 'x'}, 'run', 'out'), [])

    # -- expansion
    check('${STATIC_TESTS_DIR} expands from the file itself',
          _expand('${STATIC_TESTS_DIR}/a.fss', {'STATIC_TESTS_DIR': '/t'}), '/t/a.fss')
    check('an unknown variable is left alone rather than emptied',
          _expand('${NOPE}/a', {}), '${NOPE}/a')

    # -- the corpus facts this gate is built on, asserted rather than believed
    cs, notests, _junk = cases()
    check('there are 373 .test files', len(glob.glob('ProjectFortress/*/*.test')), 373)
    check('they yield 609 cases', len(cs), 609)
    check('every case resolves to a source',
          sum(1 for c in cs if c['src'] is None), 0)
    check('2 .test files carry no tests= at all', len(notests), 2)
    mf = [c for c in cs if c['props'].get('compile_err_equals', '').strip()]
    check('264 cases carry a non-empty compile_err_equals', len(mf), 264)
    # The 47-vs-45 reconciliation, asserted so it cannot drift back.
    bare = [c for c in mf if c['name'].endswith(('.fss', '.fsi'))]
    check('240 of the 264 resolve as `name` + `.fss`', len(mf) - len(bare), 240)
    check('24 resolve by a name that ALREADY carries its extension', len(bare), 24)
    check('a `name.fss`-only resolver would drop every one of the 24',
          [c for c in bare
           if os.path.isfile(os.path.join(os.path.dirname(c['test']),
                                          c['name'] + '.fss'))], [])

    print(f'\n{ok} passed, {bad} failed')
    return 1 if bad else 0


if opt.get('selftest'):
    sys.exit(selftest())


# ====================================================================== report
CS, NOTESTS, JUNK = cases()
with ThreadPoolExecutor(max_workers=JOBS) as pool:
    verdicts = list(pool.map(lambda t: verdict(t[1], t[0]), enumerate(CS)))
for c, (o, d) in zip(CS, verdicts):
    c['outcome'], c['detail'] = o, d

known_diverge = set(read_list(DIVERGE_LIST))
for c in CS:
    if c['outcome'] == 'fail' and c['src'] in known_diverge \
            and 'ACCEPTED a program' not in c['detail']:
        c['outcome'] = 'divergence'
buckets = collections.Counter(c['outcome'] for c in CS)
stale_diverge = sorted(known_diverge - {c['src'] for c in CS
                                        if c['outcome'] == 'divergence'})
must_fail = [c for c in CS if c['props'].get('compile_err_equals', '').strip() or
             c['props'].get('compile_err_contains', '').strip()]
accepted = sorted(c['src'] for c in must_fail if c['outcome'] == 'fail')

if opt.get('cases'):
    for c in sorted(CS, key=lambda c: (c['outcome'], c['src'] or '')):
        print(f"{c['outcome']}\t{c['src']}\t{c['test']}\t{c['detail']}")
    sys.exit(0)

# -- C
signal_rows, ran = [], 0
if not opt.get('skip_run'):
    files = list(enumerate(corpus()))
    with ThreadPoolExecutor(max_workers=JOBS) as pool:
        rows = [r for r in pool.map(build_and_run, files) if r]
    ran = len(rows)
    signal_rows = [r for r in rows if r['kind'] != 'clean']

def rewrite_list(path, wanted, key=lambda l: l):
    """Rewrite one ratchet list IN PLACE, keeping every comment.

    THESE FILES CARRY THEIR REASONS AND THE REASONS ARE THE POINT. Writing the
    header plus `'\\n'.join(...)` -- which is what this did -- deleted every
    block explaining why a line was tolerated: 62 lines of written argument for
    Compiled1.al and XXXPreparser.c went in one run, and nothing said so.

    A line is dropped only when the entry it names is no longer wanted, and a
    comment is never dropped.

    NOTHING IS EVER ADDED, which is what all three headers already say: "only
    to REMOVE a line, never to add one". An addition is a REGRESSION and needs
    a written reason beside it, so it is REPORTED here and left for a human to
    write in. A refresh that quietly appended would launder a new wrong answer
    into a tolerated one.
    """
    old_lines = open(path).read().splitlines() if os.path.exists(path) else []
    wanted = list(wanted)
    index = {key(w): w for w in wanted}
    out, seen = [], set()
    for line in old_lines:
        bare = line.split('#')[0].strip()
        if not bare:
            out.append(line)
            continue
        name = key(bare)
        if name in index:
            out.append(index[name])
            seen.add(name)
    open(path, 'w').write('\n'.join(out) + '\n')
    missing = [w for w in wanted if key(w) not in seen]
    for w in missing:
        print(f'   NOT ADDED to {os.path.basename(path)}: {w}')
        print('   -- a new entry is a regression; write its reason in beside it')
    return sum(1 for l in out if l.split('#')[0].strip())


if opt.get('refresh_lists'):
    # A DIVERGENCE THAT IS ALREADY KNOWN HAS OUTCOME `divergence`, NOT `fail` --
    # it was reclassified above, by this same script, from the list being
    # rewritten. Reading only `fail` here meant every regeneration emptied the
    # divergence list of everything already in it: four documented GenMet
    # entries went to zero in one run, and the gate then silently stopped
    # tolerating what it had been told to tolerate.
    diverged = sorted(c['src'] for c in CS
                      if c['outcome'] in ('fail', 'divergence')
                      and 'ACCEPTED a program' not in c['detail'])
    n_div = rewrite_list(DIVERGE_LIST, diverged)
    n_acc = rewrite_list(REFUSE_LIST, accepted)
    n_sig = rewrite_list(SIGNAL_LIST,
                         [f"{r['path']}\t{r['why']}" for r in
                          sorted(signal_rows, key=lambda r: r['path'])],
                         key=lambda l: l.split('\t')[0])
    print(f'wrote {n_div} to {DIVERGE_LIST}')
    print(f'wrote {n_acc} to {REFUSE_LIST}')
    print(f'wrote {n_sig} to {SIGNAL_LIST}')
    sys.exit(0)

known_accept = set(read_list(REFUSE_LIST))
known_signal = {l.split('\t')[0] for l in read_list(SIGNAL_LIST)}
new_accept = sorted(set(accepted) - known_accept)
now_refused = sorted(known_accept - set(accepted))
new_signal = sorted({r['path'] for r in signal_rows} - known_signal)

if opt.get('json'):
    print(json.dumps({
        'sha': SHA, 'compiler': CCID, 'cases': len(CS), 'outcomes': dict(buckets),
        'passFloor': PASS_FLOOR,
        'mustFail': len(must_fail), 'accepted': len(accepted),
        'knownAccepted': len(known_accept),
        'staleDivergences': stale_diverge,
        'newAcceptances': new_accept, 'nowRefused': now_refused,
        'ranBinaries': ran, 'badExits': signal_rows, 'newSignals': new_signal,
    }, indent=2))
    sys.exit(0)

print(f'== oracle gate at repo {SHA}, compiler {CCID} ==\n')
print('-- A. the cases. Every name in every `tests=`, one bucket each --')
print(f"{'cases':>7}  bucket")
order = ['pass', 'fail', 'divergence', 'blocked', 'unmodelled']
for k in order:
    print(f'{buckets.get(k, 0):>7}  {k}')
print(f'{sum(buckets.values()):>7}  total, over '
      f"{len(glob.glob('ProjectFortress/*/*.test'))} .test files")
print(f'   the pass floor is {PASS_FLOOR}')
print('   pass       a verdict was reached and it AGREED with the oracle')
print('   fail       a verdict was reached and it DISAGREED -- a wrong answer')
print('   divergence a verdict was reached, it DISAGREED, and the disagreement is')
print(f'              signed off in {os.path.relpath(DIVERGE_LIST)}')
print('   blocked    no verdict: a feature is missing. NOT a wrong answer')
print('   unmodelled the directive names a phase this driver does not expose')
print(f'   {len(NOTESTS)} .test file(s) carry no `tests=` and yield no case: '
      f"{', '.join(os.path.basename(t) for t in NOTESTS)}")
if JUNK:
    print(f'   {len(JUNK)} key(s) in the .test files are neither a directive nor a')
    print('   check. They are upstream typos and the legacy harness drops them too:')
    for k in sorted(JUNK):
        print(f'      {k}')
wic = sum(1 for c in CS if any(k.endswith('WIcontains') and v.strip()
                               for k, v in c['props'].items()))
print(f'   {wic} case(s) use `run_out_WIcontains`, which the legacy harness never')
print('   read -- this gate is STRICTER than the oracle was on those.')

print('\n-- A, continued: why the blocked cases are blocked --')
blocked = collections.Counter(c['detail'] for c in CS if c['outcome'] == 'blocked')
for msg, n in blocked.most_common(12):
    print(f'{n:>7}  {msg}')

fails = [c for c in CS if c['outcome'] == 'fail']
print(f'\n-- A, continued: the {len(fails)} disagreements, by kind --')
for msg, n in collections.Counter(c['detail'] for c in fails).most_common(12):
    print(f'{n:>7}  {msg}')

print('\n-- B. the must-fail ratchet --')
print(f'{len(must_fail):>7}  cases record a compile error from the legacy implementation')
print(f'{len(must_fail) - len(accepted):>7}  we refuse, as required')
print(f'{len(accepted):>7}  we ACCEPT -- programs that must fail and do not')
print(f'{len(known_accept):>7}  are named in {os.path.relpath(REFUSE_LIST)}')
if new_accept:
    print(f'\n   !! {len(new_accept)} NEW acceptance(s) -- a program that must fail')
    print('      started compiling and is not in the list:')
    for p in new_accept:
        print(f'      {p}')
if now_refused:
    print(f'\n   {len(now_refused)} listed file(s) are now REFUSED. Good -- delete them')
    print(f'   from {os.path.relpath(REFUSE_LIST)} in the same commit:')
    for p in now_refused:
        print(f'      {p}')

if stale_diverge:
    print(f'\n   {len(stale_diverge)} listed divergence(s) no longer disagree. Good -- delete')
    print(f'   them from {os.path.relpath(DIVERGE_LIST)} in the same commit:')
    for p in stale_diverge:
        print(f'      {p}')

print('\n-- C. every corpus file that compiles, linked and RUN --')
if opt.get('skip_run'):
    print('       skipped by --skip-run')
else:
    print(f'{ran:>7}  binaries built and executed')
    print(f'{ran - len(signal_rows):>7}  exited 0 or 1')
    print(f'{len(signal_rows):>7}  did not:')
    for r in sorted(signal_rows, key=lambda r: r['path']):
        mark = ' !! NEW' if r['path'] in new_signal else ''
        print(f"          {r['why']:<22} {r['path']}{mark}")
    print(f'{len(known_signal):>7}  are named in {os.path.relpath(SIGNAL_LIST)}')

red = []
if buckets.get('pass', 0) < PASS_FLOOR:
    red.append(f"pass is {buckets.get('pass', 0)}, below the floor of {PASS_FLOOR}")
if new_accept:
    red.append(f'{len(new_accept)} new must-fail acceptance(s)')
if new_signal:
    red.append(f'{len(new_signal)} new bad exit(s) from a compiled binary')
print()
if red:
    print('GATE RED: ' + '; '.join(red))
    sys.exit(1)
print('GATE GREEN')
PY
