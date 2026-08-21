#!/usr/bin/env bash
#
# The library census: where every file of the census set stops, per file.
#
# It was a hand count in 04-state.md ("16 of 114 reach the api gate"), and both
# halves of that sentence were wrong. THERE IS NO API GATE -- tools/ holds no
# such script and never did. What the number means is that the file parsed,
# Checker::new registered its declarations, and the driver ended on
# `an api is a set of signatures with no bodies`. Call that the API TERMINUS.
# It is not a pass. It is the furthest an .fsi can currently get.
#
# THE DENOMINATOR IS SETTLED HERE. 114 is Library/ TOP LEVEL (104) plus
# CompilerLibrary/ (10), which is the census set below. 126 is Library/
# RECURSIVE -- the extra 22 are Library/incomplete/, and the directory name is
# the reason they are not in the census. Both numbers are real and they count
# different things; neither is a correction of the other.
#
# THE CENSUS SET IS NOT THE BOOTSTRAP SET, and this script prints both so the
# difference stops being invisible. The legacy source path is
# default_repository/configuration:44 --
#     ;.;${_fr}/LibraryBuiltin;${FORTRESS_AUTOHOME}/Library;${_fr}/test_library
# CompilerLibrary is NOT on it and ProjectFortress/LibraryBuiltin IS, which is
# where FortressLibrary.fss:13-14 gets NativeArray and NatReflect from.
#
#   ./tools/api-census.sh              the census, summary and per-file list
#   ./tools/api-census.sh --selftest   only prove the classifier can refuse
#   ./tools/api-census.sh --tsv        per-file TSV on stdout, nothing else
#   ./tools/api-census.sh --group NAME one group: census, incomplete,
#                                      libraryBuiltin, testLibrary, all
#   ./tools/api-census.sh --status S   list one status: compiles, terminus,
#                                      blocked
#   ./tools/api-census.sh --json       machine readable
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

fortressc=${FORTRESSC:-$repo/fortressc/target/debug/fortressc}
if [[ ! -x $fortressc ]]; then
    printf 'no compiler at %s -- cargo build first\n' "$fortressc" >&2
    exit 2
fi
mkdir -p "$repo/fortressc/build"

cd "$repo" && FORTRESSC=$fortressc \
    CENSUS_SHA=$(git rev-parse --short=9 HEAD 2>/dev/null || echo unknown) \
    CENSUS_CC=$(sha256sum "$fortressc" 2>/dev/null | cut -c1-12 || echo unknown) \
    CACHE=${CACHE:-$repo/fortressc/build/api-census.json} \
    python3 - "$@" <<'PY'
import json, os, re, subprocess, sys, collections
from concurrent.futures import ThreadPoolExecutor

FORTRESSC = os.environ['FORTRESSC']
CACHE     = os.environ['CACHE']
SHA       = os.environ['CENSUS_SHA']
# FORTRESSC pins the binary, so repo HEAD is NOT compiler identity. Both.
CCID      = os.environ.get('CENSUS_CC', 'unknown')

args, opt = sys.argv[1:], {}
i = 0
while i < len(args):
    a = args[i]
    if a in ('--group', '--status', '--jobs'):
        opt[a[2:]] = args[i + 1]; i += 2
    elif a in ('--selftest', '--tsv', '--json'):
        opt[a[2:]] = True; i += 1
    else:
        print(f'unknown argument {a}', file=sys.stderr); sys.exit(2)
jobs = int(opt.get('jobs', os.cpu_count() or 4))

# ------------------------------------------------------------- the groups
#
# `census` is the 114. The other three exist so the difference between the
# census set and the bootstrap set is a printed number rather than a footnote.
def top_level(d):
    if not os.path.isdir(d):
        return []
    return sorted(os.path.join(d, f) for f in os.listdir(d)
                  if f.endswith(('.fss', '.fsi')))

def recursive(d):
    out = []
    for base, _, fs in os.walk(d):
        out += [os.path.join(base, f) for f in fs if f.endswith(('.fss', '.fsi'))]
    return sorted(out)

census     = top_level('Library') + top_level('CompilerLibrary')
incomplete = [p for p in recursive('Library') if p not in set(census)]
builtin    = recursive('ProjectFortress/LibraryBuiltin')
testlib    = recursive('ProjectFortress/test_library')
GROUPS = {'census': census, 'incomplete': incomplete,
          'libraryBuiltin': builtin, 'testLibrary': testlib}
GROUPS['all'] = census + incomplete + builtin + testlib

# ---------------------------------------------------------- classification
#
# The driver prints `path: start..end: message`. Stripping the span is what
# turns a few dozen shapes back into a few dozen instead of one per file, and
# it is the same regex tools/triage.sh:130 uses.
SPAN = re.compile(r'^\S+?: \d+\.\.\d+: ')

# EXACT, not a substring search: the terminus is a status and not a keyword.
# types/src/lib.rs refuses `is_api` as the first statement of Checker::run, so
# this message means the file parsed and registered and stopped there.
TERMINUS = 'an `api` is a set of signatures with no bodies; there is nothing to compile'

def diagnostic(last):
    return SPAN.sub('', last) or '<no diagnostic>'

def classify(code, last):
    if code == 0:
        return 'compiles'
    if code == 1 and diagnostic(last) == TERMINUS:
        return 'terminus'
    return 'blocked'

if opt.get('selftest'):
    # An assertion is not trusted until it has refused. Each case below is a
    # near miss of the one above it.
    passed = failed = 0
    def check(name, got, want):
        global passed, failed
        if got == want:
            passed += 1; print(f'ok    {name}')
        else:
            failed += 1; print(f'FAIL  {name}\n      got {got!r}, want {want!r}')

    term_line = f'Library/Testable.fsi: 352..541: {TERMINUS}'
    print('== census classifier self test ==')
    check('exit 0 is compiles',           classify(0, ''), 'compiles')
    check('the terminus line is terminus', classify(1, term_line), 'terminus')
    check('exit 0 wins over a terminus line',
          classify(0, term_line), 'compiles')
    # The three that matter: a terminus message must not be recognised on the
    # wrong exit code, and a message that merely CONTAINS the terminus text or
    # is a prefix of it is a different diagnostic.
    check('exit 70 with the terminus line is blocked, not terminus',
          classify(70, term_line), 'blocked')
    check('a substring of the terminus message is blocked',
          classify(1, 'x.fsi: 1..2: an `api` is a set of signatures'), 'blocked')
    check('a superstring of the terminus message is blocked',
          classify(1, f'x.fsi: 1..2: {TERMINUS} yet'), 'blocked')
    check('an ordinary diagnostic is blocked',
          classify(1, 'x.fsi: 1..2: unknown type `ImmutableArray`'), 'blocked')
    check('an unspanned line still strips to itself',
          diagnostic('fortressc: no such file'), 'fortressc: no such file')
    check('the span strips', diagnostic(term_line), TERMINUS)
    check('an empty stderr is named, not empty',
          diagnostic(''), '<no diagnostic>')
    # The denominator, asserted rather than believed. This is the 114-vs-126
    # settlement and it is a fact about the tree, so it belongs in the gate.
    check('census set is 114 files', len(census), 114)
    check('Library top level is 104', len(top_level('Library')), 104)
    check('CompilerLibrary is 10', len(top_level('CompilerLibrary')), 10)
    check('Library recursive is 126', len(recursive('Library')), 126)
    check('the 126-114 gap is Library/incomplete', len(incomplete), 22)
    check('every file of the gap is under Library/incomplete',
          all(p.startswith('Library/incomplete/') for p in incomplete), True)
    print(f'\n{passed} passed, {failed} failed')
    sys.exit(1 if failed else 0)

# ------------------------------------------------------------- the sweep
def run_one(path):
    r = subprocess.run([FORTRESSC, path, '--emit-obj', '-o', '/dev/null'],
                       capture_output=True, text=True, stdin=subprocess.DEVNULL)
    lines = r.stderr.strip().splitlines()
    last  = lines[-1] if lines else ''
    return {'path': path, 'code': r.returncode, 'last': last,
            'status': classify(r.returncode, last),
            'diagnostic': diagnostic(last) if r.returncode else ''}

want_group = opt.get('group', 'all')
if want_group not in GROUPS:
    print(f'unknown group {want_group}; one of {", ".join(GROUPS)}', file=sys.stderr)
    sys.exit(2)

with ThreadPoolExecutor(max_workers=jobs) as pool:
    recs = list(pool.map(run_one, GROUPS['all']))
by_path = {r['path']: r for r in recs}
with open(CACHE, 'w') as f:
    json.dump({'sha': SHA, 'cc': CCID, 'records': recs}, f, indent=1)

sel = [by_path[p] for p in GROUPS[want_group]]
if 'status' in opt:
    sel = [r for r in sel if r['status'] == opt['status']]

if opt.get('tsv'):
    for r in sorted(sel, key=lambda r: r['path']):
        print(f"{r['status']}\t{r['code']}\t{r['path']}\t{r['diagnostic']}")
    sys.exit(0)

def tally(paths):
    c = collections.Counter(by_path[p]['status'] for p in paths)
    return c['compiles'], c['terminus'], c['blocked']

if opt.get('json'):
    print(json.dumps({
        'sha': SHA, 'compiler': CCID,
        'groups': {g: {'files': len(ps),
                       'compiles': tally(ps)[0], 'terminus': tally(ps)[1],
                       'blocked': tally(ps)[2]} for g, ps in GROUPS.items()},
        'records': sorted(sel, key=lambda r: r['path']),
    }, indent=2))
    sys.exit(0)

print(f'== library census at repo {SHA}, compiler {CCID} ==')
print('THE DENOMINATOR: 114 = Library/ top level 104 + CompilerLibrary/ 10.')
print('126 = Library/ RECURSIVE; the extra 22 are Library/incomplete/.')
print('The census set is NOT the bootstrap set -- the legacy source path')
print('(default_repository/configuration:44) carries LibraryBuiltin and NOT')
print('CompilerLibrary.\n')
print(f"{'files':>6} {'compile':>8} {'terminus':>9} {'blocked':>8}  group")
for g in ('census', 'incomplete', 'libraryBuiltin', 'testLibrary', 'all'):
    c, t, b = tally(GROUPS[g])
    print(f'{len(GROUPS[g]):>6} {c:>8} {t:>9} {b:>8}  {g}')

# The published hand count is over .fsi files only, so it is split out or the
# two numbers cannot be compared at all.
fsi = [p for p in census if p.endswith('.fsi')]
fss = [p for p in census if p.endswith('.fss')]
print(f'\n-- the census set by extension. The 04-state hand count is the .fsi row --')
print(f"{'files':>6} {'compile':>8} {'terminus':>9} {'blocked':>8}  kind")
for name, ps in (('.fsi', fsi), ('.fss', fss)):
    c, t, b = tally(ps)
    print(f'{len(ps):>6} {c:>8} {t:>9} {b:>8}  {name}')

print('\n-- census set: what compiles end to end --')
for p in sorted(p for p in census if by_path[p]['status'] == 'compiles'):
    print(f'   {p}')
print('\n-- census set: what reaches the api terminus --')
for p in sorted(p for p in census if by_path[p]['status'] == 'terminus'):
    print(f'   {p}')

print('\n-- census set: what blocks it, by diagnostic --')
blocked = collections.Counter(by_path[p]['diagnostic'] for p in census
                              if by_path[p]['status'] == 'blocked')
for msg, n in blocked.most_common():
    print(f'{n:>6}  {msg[:110]}')
print(f'\n{len(blocked)} distinct diagnostics over '
      f'{sum(blocked.values())} blocked census files.')
print(f'\nPer file: ./tools/api-census.sh --tsv   Cache: {CACHE}')
PY
