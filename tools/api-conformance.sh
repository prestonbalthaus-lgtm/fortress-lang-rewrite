#!/usr/bin/env bash
#
# Component-satisfies-api conformance: the Group 2 baseline, and the ratchet
# that arms itself when the api check mode lands.
#
# `Specification/basic/components/source-code.tex:313-320`: a component must
# provide a declaration, or a SET of declarations, that SATISFIES every
# top-level declaration in any api it exports. `private` declarations do not
# participate, and a component may carry declarations that satisfy nothing.
#
# NOTHING IN THIS REPOSITORY COMPARES A `.fss` TO A `.fsi`. The driver takes one
# file, `exports` has no readers, and `Checker::run` refuses `is_api` as its
# first statement. So this script cannot measure conformance today -- what it
# measures is HOW FAR EACH PAIR HAS GOT, on a ladder, so the number moves as the
# api check mode and the conformance check come online instead of being written
# from scratch afterwards.
#
#   L0  unpaired      no `.fsi` for this `.fss`, or no `.fss` for this `.fsi`
#   L1  blocked       one side or both stops before it registers its declarations
#   L2  pairable      the api reaches the api terminus AND the component compiles
#   L3  checked       an api check mode exists and the `.fsi` passes it
#   L4  conformant    the driver compares the two and agrees
#
# L3 AND L4 ARE UNREACHABLE UNTIL SOMETHING LANDS, AND THAT IS THE POINT. The
# script PROBES for an api check mode rather than assuming its spelling, so the
# day `SPIKE-API-CHECK-MODE` ships, this number moves without anyone editing
# this file. Add the flag to API_MODES below if it is spelled differently.
#
#   ./tools/api-conformance.sh              the ladder, and the per-pair table
#   ./tools/api-conformance.sh --selftest   only prove the assertions can refuse
#   ./tools/api-conformance.sh --tsv        per pair, machine readable
#   ./tools/api-conformance.sh --gaps       api declarations with no same-name
#                                           component declaration
#   ./tools/api-conformance.sh --json
#
# FORTRESSC pins the binary. Set it when other work is rebuilding the tree, or
# the sweep silently mixes two compilers. KEEP THE PINNED COPY OUTSIDE
# fortressc/build/ -- that directory is shared and seven gates `rm -rf` it.
set -uo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
export LLVM_SYS_221_PREFIX=${LLVM_SYS_221_PREFIX:-$HOME/.local/opt/llvm22-root/usr/lib64/llvm22}
export CPATH=${CPATH:-$HOME/.local/opt/gc-root/usr/include}
export LIBRARY_PATH=${LIBRARY_PATH:-$HOME/.local/opt/gc-root/usr/lib64}

fortressc=${FORTRESSC:-$repo/fortressc/target/debug/fortressc}
if [[ ! -x $fortressc ]]; then
    printf 'no compiler at %s -- cargo build first\n' "$fortressc" >&2
    exit 2
fi
mkdir -p "$repo/fortressc/build"

cd "$repo" && FORTRESSC=$fortressc \
    CONF_SHA=$(git rev-parse --short=9 HEAD 2>/dev/null || echo unknown) \
    CONF_CC=$(sha256sum "$fortressc" 2>/dev/null | cut -c1-12 || echo unknown) \
    CACHE=${CACHE:-$repo/fortressc/build/api-conformance.json} \
    python3 - "$@" <<'PY'
import json, os, re, subprocess, sys, collections
from concurrent.futures import ThreadPoolExecutor

FORTRESSC = os.environ['FORTRESSC']
CACHE     = os.environ['CACHE']
SHA       = os.environ['CONF_SHA']
CCID      = os.environ['CONF_CC']

args, opt = sys.argv[1:], {}
i = 0
while i < len(args):
    a = args[i]
    if a in ('--jobs',):
        opt[a[2:]] = args[i + 1]; i += 2
    elif a in ('--selftest', '--tsv', '--json', '--gaps'):
        opt[a[2:]] = True; i += 1
    else:
        print(f'unknown argument {a}', file=sys.stderr); sys.exit(2)
JOBS = int(opt.get('jobs', os.cpu_count() or 4))

# THE SOURCE PATH, from default_repository/configuration:44 --
#   .;${_fr}/LibraryBuiltin;${FORTRESS_AUTOHOME}/Library;${_fr}/test_library
# CompilerLibrary is NOT on it and LibraryBuiltin IS, which is where
# FortressLibrary.fss:13-14 gets NativeArray and NatReflect. CompilerLibrary is
# swept anyway because its ten apis are part of the census set; they are all
# api-only and that is itself a finding, not an omission.
ROOTS = ['ProjectFortress/LibraryBuiltin', 'Library', 'CompilerLibrary',
         'ProjectFortress/test_library']


# ------------------------------------------------------- the declaration set
#
# LEXICAL AND APPROXIMATE, AND THE REPORT SAYS SO. A top-level declaration sits
# at column 0 inside the `api`/`component` block; members are indented. This is
# a CEILING on conformance in the same sense triage's `alone` is a ceiling: a
# name present on both sides is NECESSARY for satisfaction and nowhere near
# SUFFICIENT, because source-code.tex:322-360 makes satisfaction a relation over
# TYPES and over SETS of declarations, with subtyping on the return.
def strip(text):
    """Mirrors lexer/src/raw.rs:51-142 -- `(*)` is a LINE comment and is inert
    inside a block comment. tools/triage.sh carries the same code and the
    comment there records what a stripper without this ate."""
    out, i, n = [], 0, len(text)
    while i < n:
        c = text[i]
        if c == '"':
            i += 1
            while i < n and text[i] != '"':
                i += 2 if text[i] == '\\' else 1
            i += 1; out.append(' '); continue
        if text.startswith('(*)', i):
            i = _line_comment(text, i + 3); continue
        if text.startswith('(*', i):
            i = _block_comment(text, i + 2, out); continue
        out.append(c); i += 1
    return ''.join(out)


def _line_comment(text, i):
    depth, n = 0, len(text)
    while i < n:
        if text.startswith('(*)', i): i += 3; continue
        if text.startswith('(*', i):  depth += 1; i += 2; continue
        if text.startswith('*)', i):
            if depth == 0: return i
            depth -= 1; i += 2; continue
        if text[i] == '\n': return i
        i += 1
    return i


def _block_comment(text, i, out):
    depth, n = 1, len(text)
    while i < n:
        if text.startswith('(*)', i): i += 3; continue
        if text.startswith('(*', i):  depth += 1; i += 2; continue
        if text.startswith('*)', i):
            depth -= 1; i += 2
            if depth == 0: return i
            continue
        if text[i] == '\n': out.append('\n')
        i += 1
    return i


SKIP = re.compile(r'^(api|component|import|export|end)\b')
MODS = (r'(?:(?:private|abstract|value|native|test|getter|setter|coerce'
        r'|widens|io|atomic|pure|override|hidden|settable)\s+)*')
DECL = re.compile(r'^' + MODS +
                  r'(?:(?:trait|object)\s+([A-Za-z_]\w*)'
                  r'|opr\s+(\S+)'
                  r'|([A-Za-z_]\w*)\s*(?:\[\\|\(|:))')


def declarations(path):
    try:
        text = strip(open(path, encoding='utf-8', errors='replace').read())
    except OSError:
        return set()
    out = set()
    for line in text.splitlines():
        if not line or line[0] in ' \t':
            continue
        if SKIP.match(line):
            continue
        m = DECL.match(line)
        if m:
            out.add(m.group(1) or m.group(2) or m.group(3))
    return out


# --------------------------------------------------------------- the ladder
TERMINUS = ('an `api` is a set of signatures with no bodies; '
            'there is nothing to compile')
# THE DRIVER RENDERS A SOURCE EXCERPT UNDER EACH DIAGNOSTIC since the semantics
# lane's line:col work, and for some variants `note:` lines with excerpts of
# their own. SO THE LAST STDERR LINE IS A CARET, NOT A MESSAGE. Take the first
# HEADER line instead -- the one that carries a position and is not a note --
# which is exactly what tools/triage.sh:140-148 settled on, kept identical here
# so the two instruments cannot disagree about what a file's diagnostic was.
HEADER = re.compile(r'^\S+?:\d+:\d+: (?!note: )')

SPAN = re.compile(r'^\S+?:(?: \d+\.\.\d+|\d+:\d+): ')


def diagnostic(last):
    return SPAN.sub('', last) or '<no diagnostic>'


# Candidate spellings for an api check mode. NONE OF THESE EXISTS TODAY; the
# probe is here so the day SPIKE-API-CHECK-MODE lands, this script measures it
# without being rewritten. Add a spelling rather than changing the ladder.
API_MODES = ['--check-api', '--api', '--api-check', '--check']


def run(argv, timeout=60):
    try:
        r = subprocess.run(argv, capture_output=True, text=True, timeout=timeout,
                           stdin=subprocess.DEVNULL, errors='replace')
        return r.returncode, r.stderr
    except (subprocess.TimeoutExpired, OSError):
        return 'error', ''


def probe_api_mode():
    """Returns the first flag the driver accepts on an .fsi, or None."""
    sample = 'Library/Testable.fsi'
    if not os.path.isfile(sample):
        return None
    for flag in API_MODES:
        code, _ = run([FORTRESSC, flag, sample, '-o', '/dev/null'])
        if code == 0:
            return flag
    return None


def level(api_code, api_last, comp_code, api_mode_ok, conformant):
    """The ladder. Ordered, and each level is a strictly stronger claim."""
    if api_code is None or comp_code is None:
        return 'L0-unpaired'
    api_ok = api_code == 1 and diagnostic(api_last) == TERMINUS
    if not (api_ok and comp_code == 0):
        return 'L1-blocked'
    if not api_mode_ok:
        return 'L2-pairable'
    if not conformant:
        return 'L3-checked'
    return 'L4-conformant'


# ------------------------------------------------------------------ pairing
def pairs():
    out = []
    for d in ROOTS:
        if not os.path.isdir(d):
            continue
        fsi = {f[:-4] for f in os.listdir(d) if f.endswith('.fsi')}
        fss = {f[:-4] for f in os.listdir(d) if f.endswith('.fss')}
        for n in sorted(fsi | fss):
            out.append({
                'dir': d, 'name': n,
                'api':  os.path.join(d, n + '.fsi') if n in fsi else None,
                'comp': os.path.join(d, n + '.fss') if n in fss else None,
            })
    return out


if opt.get('selftest'):
    ok = bad = 0

    def check(name, got, want):
        global ok, bad
        if got == want:
            ok += 1; print(f'ok    {name}')
        else:
            bad += 1; print(f'FAIL  {name}\n      got {got!r}, want {want!r}')

    print('== api conformance self test ==')
    T = f'x.fsi: 1..2: {TERMINUS}'

    # The ladder, and every level shown to refuse the one above it.
    check('an unpaired api is L0',   level(1, T, None, True, True), 'L0-unpaired')
    check('an unpaired component is L0', level(None, '', 0, True, True), 'L0-unpaired')
    check('an api that does not reach the terminus is L1',
          level(1, 'x.fsi: 1..2: unknown type `Q`', 0, False, False), 'L1-blocked')
    check('a component that does not compile is L1',
          level(1, T, 1, False, False), 'L1-blocked')
    check('a component crash is L1, not a pass',
          level(1, T, 70, False, False), 'L1-blocked')
    check('both halves clean is L2 while there is no api mode',
          level(1, T, 0, False, False), 'L2-pairable')
    check('L3 needs the api mode to have passed',
          level(1, T, 0, True, False), 'L3-checked')
    check('L4 needs the comparison to agree',
          level(1, T, 0, True, True), 'L4-conformant')
    check('the terminus is matched EXACTLY, not as a substring',
          level(1, 'x.fsi: 1..2: an `api` is a set of signatures', 0, False, False),
          'L1-blocked')

    # The extractor. Each case is a shape the census set actually writes.
    d = declarations
    import tempfile
    def probe(src):
        with tempfile.NamedTemporaryFile('w', suffix='.fsi', delete=False) as f:
            f.write(src); p = f.name
        try:    return d(p)
        finally: os.unlink(p)

    check('a trait is a top-level declaration',
          probe('api X\ntrait AnyList excludes { Number }\nend\n'), {'AnyList'})
    check('an object is one',   probe('api X\nobject Foo(a: ZZ32)\nend\n'), {'Foo'})
    check('a function is one',  probe('api X\nemptyList[\\E\\](): List[\\E\\]\nend\n'),
          {'emptyList'})
    check('a variable is one',  probe('api X\nGlobal: Region\nend\n'), {'Global'})
    check('a modifier does not hide the name',
          probe('api X\nprivate value object Foo()\nend\n'), {'Foo'})
    check('an INDENTED member is NOT a top-level declaration',
          probe('api X\ntrait T\n    addLeft(e:Any): T\nend\n'), {'T'})
    check('the api header is not a declaration', probe('api X\nend\n'), set())
    check('import and export are not declarations',
          probe('api X\nimport List.{...}\nexport Foo\nend\n'), set())
    check('a `(*)` line comment does not eat the rest of the file',
          probe('api X\n(*) trait Hidden\ntrait Real\nend\n'), {'Real'})
    check('a block comment hides what is inside it',
          probe('api X\n(* trait Hidden *)\ntrait Real\nend\n'), {'Real'})
    check('a declaration inside a string is not a declaration',
          probe('api X\nf(): String = "trait Nope"\nend\n'), {'f'})

    # The pairing, asserted against the tree rather than believed.
    ps = pairs()
    both = [p for p in ps if p['api'] and p['comp']]
    # Decomposed per directory, so a change tells you WHICH source-path entry
    # moved rather than only that the total did.
    per = collections.Counter(p['dir'] for p in both)
    check('67 api/component pairs exist to track', len(both), 67)
    check('  51 of them are Library/',        per['Library'], 51)
    check('  10 are test_library/',           per['ProjectFortress/test_library'], 10)
    check('  6 are LibraryBuiltin/',          per['ProjectFortress/LibraryBuiltin'], 6)
    check('CompilerLibrary is entirely api-only, which is itself the finding',
          [p['name'] for p in ps
           if p['dir'] == 'CompilerLibrary' and p['comp']], [])
    check('  and it contributes 10 unpaired apis',
          len([p for p in ps if p['dir'] == 'CompilerLibrary']), 10)
    print(f'\n{ok} passed, {bad} failed')
    sys.exit(1 if bad else 0)

# ------------------------------------------------------------------- report
API_MODE = probe_api_mode()
PS = pairs()


def measure(p):
    api_code = api_last = comp_code = None
    if p['api']:
        api_code, err = run([FORTRESSC, p['api'], '--emit-obj', '-o', '/dev/null'])
        lines = err.strip().splitlines()
        header = next((l for l in lines if HEADER.match(l)), None)
        api_last = header if header is not None else (lines[-1] if lines else '')
    comp_last = ''
    if p['comp']:
        comp_code, cerr = run([FORTRESSC, p['comp'], '--emit-obj', '-o', '/dev/null'])
        clines = cerr.strip().splitlines()
        ch = next((l for l in clines if HEADER.match(l)), None)
        comp_last = ch if ch is not None else (clines[-1] if clines else '')
    api_mode_ok = False
    if API_MODE and p['api']:
        code, _ = run([FORTRESSC, API_MODE, p['api'], '-o', '/dev/null'])
        api_mode_ok = code == 0
    lvl = level(api_code, api_last or '', comp_code, api_mode_ok, False)
    a = declarations(p['api']) if p['api'] else set()
    c = declarations(p['comp']) if p['comp'] else set()
    return {**p, 'level': lvl, 'apiDecls': len(a), 'compDecls': len(c),
            'named': len(a & c), 'missing': sorted(a - c),
            'why': _why(lvl, api_code, api_last or '', comp_code, comp_last)}


def _why(lvl, api_code, api_last, comp_code, comp_last):
    """WHICH HALF blocked it, and its diagnostic. The first draft said only
    `the component does not compile` for eleven pairs, which is the shape of a
    bucket that names no mechanism -- the class this project has already paid an
    hour for twice."""
    if lvl != 'L1-blocked':
        return ''
    api_ok = api_code == 1 and diagnostic(api_last) == TERMINUS
    if api_code is not None and not api_ok:
        return 'api: ' + diagnostic(api_last)
    if comp_code not in (None, 0):
        return 'component: ' + diagnostic(comp_last)
    return 'component: <no diagnostic>'


with ThreadPoolExecutor(max_workers=JOBS) as pool:
    rows = list(pool.map(measure, PS))
with open(CACHE, 'w') as f:
    json.dump({'sha': SHA, 'cc': CCID, 'apiMode': API_MODE, 'rows': rows}, f, indent=1)

if opt.get('tsv'):
    for r in rows:
        print(f"{r['level']}\t{r['dir']}/{r['name']}\t{r['apiDecls']}\t"
              f"{r['compDecls']}\t{r['named']}\t{r['why']}")
    sys.exit(0)

if opt.get('gaps'):
    for r in rows:
        if r['missing']:
            print(f"{r['dir']}/{r['name']}  {len(r['missing'])} api declaration(s) "
                  f"with no same-name component declaration")
            for m in r['missing']:
                print(f'    {m}')
    sys.exit(0)

if opt.get('json'):
    print(json.dumps({'sha': SHA, 'compiler': CCID, 'apiMode': API_MODE,
                      'levels': dict(collections.Counter(r['level'] for r in rows)),
                      'rows': rows}, indent=2))
    sys.exit(0)

print(f'== component-satisfies-api at repo {SHA}, compiler {CCID} ==\n')
print('NOTHING COMPARES A .fss TO A .fsi YET. This is the ladder each pair is on,')
print('so the number moves as the api check mode and the conformance check land.\n')
counts = collections.Counter(r['level'] for r in rows)
print(f"{'pairs':>6}  level")
for lvl in ('L4-conformant', 'L3-checked', 'L2-pairable', 'L1-blocked', 'L0-unpaired'):
    print(f'{counts.get(lvl, 0):>6}  {lvl}')
print(f'{len(rows):>6}  total, over {len(ROOTS)} source-path directories')
print()
if API_MODE:
    print(f'   an api check mode IS available: `{API_MODE}`. L3 is live.')
else:
    print('   NO api check mode yet. None of ' + ', '.join(API_MODES))
    print('   is accepted, so L3 and L4 are unreachable BY CONSTRUCTION and L2 is')
    print('   the ceiling. That is the')
    print('   baseline this file exists to move; add a spelling to API_MODES if it')
    print('   ships under a different flag.')

both = [r for r in rows if r['api'] and r['comp']]
api_total = sum(r['apiDecls'] for r in both)
named = sum(r['named'] for r in both)
print(f'\n-- the name ceiling, and read the caveat before quoting it --')
print(f'{len(both)} pairs, {api_total} api top-level declarations, {named} have a')
print(f'same-name component declaration ({100 * named / max(api_total, 1):.1f}%).')
print('THIS IS SATURATED AND THEREFORE USELESS AS A PROGRESS METRIC. It is a')
print('CEILING: a matching name is necessary for satisfaction and nowhere near')
print('sufficient, because source-code.tex:322-360 makes satisfaction a relation')
print('over TYPES and over SETS of declarations with subtyping on the return.')
print('It is reported so nobody mistakes it for conformance later. The LADDER is')
print('the metric.')

print('\n-- L1, by what blocks it --')
for msg, n in collections.Counter(
        r['why'] for r in rows if r['level'] == 'L1-blocked').most_common(12):
    print(f'{n:>6}  {msg[:96]}')

print(f'\nPer pair: ./tools/api-conformance.sh --tsv   Cache: {CACHE}')
PY
