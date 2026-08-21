#!/usr/bin/env bash
#
# Corpus triage: why the corpus does not compile, grouped by ROOT CAUSE.
#
# Runs the driver over every corpus file, takes the diagnostic each failure
# ends on, and folds ~340 distinct messages into a frequency map of language
# features. Output is Count -> Category, most expensive first.
#
# READ THIS BEFORE PLANNING A MILESTONE OFF THE FREQUENCY MAP.
#
# The map is FIRST-BLOCKER data. It says what the compiler hit first, which is
# not what a feature is WORTH: a file blocked on `opr` may be blocked on four
# other things behind it, and fixing `opr` moves it from one bucket to another
# without compiling anything. First-blocker counting has been wrong on this
# project four milestones running -- M5's own headline was "12 files" by that
# method and 7 by measurement.
#
# So the second table is the one to plan from. For every category with a
# reliable source marker it reports:
#
#   appears   files whose SOURCE uses the feature at all, comments and string
#             literals stripped first
#   alone     of those, how many use NO OTHER marked feature -- the ceiling on
#             what implementing it by itself could unlock
#
# `alone` is an upper bound and not a promise: a file can still be blocked on
# something with no marker. The only number that settles it is a spike behind
# an env switch and a re-run, which is what --bundle exists to approximate.
#
#   ./tools/triage.sh                        the whole corpus
#   ./tools/triage.sh --root SpecData        one subtree
#   ./tools/triage.sh --top 15               trim the frequency map
#   ./tools/triage.sh --category traits      list the files in one category
#   ./tools/triage.sh --bundle traits,opr    what a milestone of those unlocks
#   ./tools/triage.sh --raw                  ungrouped messages, to re-ground
#                                            the rules table when it goes stale
#   ./tools/triage.sh --json                 machine readable
#   ./tools/triage.sh --reuse                skip the compile, read the cache
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

cd "$repo" && FORTRESSC=$fortressc CACHE=${CACHE:-$repo/fortressc/build/triage.json} \
    python3 - "$@" <<'PY'
import json, os, re, subprocess, sys, collections
from concurrent.futures import ThreadPoolExecutor

FORTRESSC = os.environ['FORTRESSC']
CACHE     = os.environ['CACHE']

# ---------------------------------------------------------------- arguments
args, opt = sys.argv[1:], {}
i = 0
while i < len(args):
    a = args[i]
    if a in ('--root', '--top', '--category', '--bundle', '--jobs'):
        opt[a[2:]] = args[i + 1]; i += 2
    elif a in ('--raw', '--json', '--reuse', '--real'):
        opt[a[2:]] = True; i += 1
    else:
        print(f'unknown argument {a}', file=sys.stderr); sys.exit(2)
root  = opt.get('root', '.')

# Directories the upstream project itself marks as not-supposed-to-work, plus
# the XXX/SXX/DXX filename prefix the corpus uses for a must-FAIL test. 282 of
# the 1676 failures live here, so leaving them in overstates every category by
# about a sixth. --real drops them.
NEGATIVE = re.compile(
    r'not_passing_yet|not_working|long_term_not_working|staticError'
    r'|NeedBetterErrorMessages|compiler_regressions|parser_tests'
    r'|(^|/)(XXX|SXX|DXX)')
top   = int(opt.get('top', 0))
jobs  = int(opt.get('jobs', os.cpu_count() or 4))

# ------------------------------------------------------------- the corpus
# The same walk apply-gate.sh uses, and the two exclusions are both scars.
# `.claude` holds agent worktrees, which are FULL REPO COPIES. `examples/` at
# the ROOT is hand-written demo code -- pruned BY PATH, because SpecData/examples
# IS corpus and pruning the name once took 137 legacy files out of the metric.
def corpus(base):
    out = []
    for d, ds, fs in os.walk(base):
        ds[:] = [x for x in ds if x not in ('.git', 'target', 'fortressc', '.claude')]
        if os.path.normpath(d) == '.':
            ds[:] = [x for x in ds if x != 'examples']
        out += [os.path.join(d, f) for f in fs if f.endswith(('.fss', '.fsi'))]
    return sorted(out)

# A rendered diagnostic is `path:LINE:COL: message` followed by a source
# excerpt and, for two variants, `note:` lines with excerpts of their own. So
# the last stderr line is a CARET, not a message. Take the first HEADER line
# instead -- the one that carries a position and is not a note.
HEADER = re.compile(r'^\S+?:\d+:\d+: (?!note: )')

def compile_one(path):
    r = subprocess.run([FORTRESSC, path, '--emit-obj', '-o', '/dev/null'],
                       capture_output=True, text=True)
    lines = r.stderr.strip().splitlines()
    header = next((l for l in lines if HEADER.match(l)), None)
    return {'path': path, 'code': r.returncode,
            'last': header if header is not None else (lines[-1] if lines else '')}

def results(base):
    if opt.get('reuse') and os.path.exists(CACHE):
        with open(CACHE) as f:
            cached = json.load(f)
        if base in ('.', './'):
            return cached
        want = os.path.normpath(base)
        return [r for r in cached
                if os.path.normpath(r['path']).startswith(want + os.sep)]
    files = corpus(base)
    with ThreadPoolExecutor(max_workers=jobs) as pool:
        out = list(pool.map(compile_one, files))
    if base in ('.', './'):
        with open(CACHE, 'w') as f:
            json.dump(out, f)
    return out

# `file:12:7: the message` -> `the message`. The position is what makes 340
# messages out of what is really a few dozen shapes, so stripping it is what
# makes the fold work at all -- leaving it in gives one bucket per FILE and
# fails green, with no exception and a garbage map.
#
# The old `file: 123..456: ` form is still matched, because the cache at
# fortressc/build/triage.json carries whatever format wrote it and `--reuse`
# re-parses it with no version check.
SPAN = re.compile(r'^\S+?:(?: \d+\.\.\d+|\d+:\d+): ')
def diagnostic(rec):
    return SPAN.sub('', rec['last']) or '<no diagnostic>'

# -------------------------------------------------------------- the rules
#
# ORDERED. First match wins, so the specific feature rules come before the
# generic "some parse error" catch-alls at the bottom. Every pattern here was
# read off a real run -- regenerate with --raw when it drifts.
#
# (category, stage, regex)
RULES = [
 # Lexical. `unrecognized character` is NOT about Unicode -- measured, the
 # characters are \\ ! ? ~ $ @ and a backtick, and they are OPERATOR characters:
 # 28 of them are `opr |\\self/|`, the floor bracket. Same milestone as `opr`.
 ('operator-characters',     'lex',   r'unrecognized character|`=` is followed by an operator character'),
 ('unicode-identifiers',     'lex',   r'non-ASCII characters'),
 ('literal-forms',           'lex',   r'character literals|radix numerals|curly-quote|string literal may not span|tab characters|integer literal does not fit'),

 ('operator-declarations',   'parse', r'reserved word `opr`|`opr` static parameters'),
 ('api-signature-files',     'n/a',   r'an `api` is a set of signatures'),
 ('comprehensions-and-big',  'parse', r'reserved word `BIG`'),

 # Traits and objects in their full 1.0 form. `trait aa(c:ZZ32, d:ZZ32)` --
 # value parameters on a trait -- is what `found LParen` is.
 ('traits-objects',          'parse', r'found KwExtends|found KwComprises|found KwExcludes|found KwObject|found KwSelf|found KwTrait|reserved word `(abstract|value|Self|object)`|unknown type `(Object|Any)`|is not a trait, so nothing can extend it|expected a field or method name, found LParen'),

 ('var-without-initialiser', 'parse', r'found KwVar'),
 ('aggregate-literals',      'parse', r'found LeftBar|found RightBar|found LBrace|found Bar\b|found BarBar'),

 # `if (k,v,_) <- extractMinimum() then` -- a generator binding in a condition,
 # with a tuple binder and a wildcard. Three features, one line.
 ('generator-bindings',      'parse', r'expected `then`, found Lt|expected `then`, found LParen'),
 ('tuples',                  'type',  r'a tuple (type|expression) is not implemented'),

 # `a:ZZ32[5]` and `Array1[\\ZZ32,0,5\\]` -- sized array types, and numbers as
 # static arguments. The two travel together in this corpus.
 ('array-and-matrix-types',  'parse', r'expected a type name, found IntLit|expected a newline or `;`, found Colon'),
 ('non-type-static-params',  'type',  r'`(nat|int|bool|unit|dim)` static parameters are not implemented'),

 ('function-types',          'parse', r'reserved word `fn`|an arrow type is not implemented'),
 # `f(v) = 2` and `object O(x)` -- a parameter with no type annotation.
 ('untyped-parameters',      'parse', r'expected `:`, found RParen'),
 ('local-functions',         'parse', r'a local function declaration|expected `\)`, found Colon'),

 ('exceptions',              'parse', r'reserved word `(try|catch|throw|throws|finally)`|unknown type `\w*Exception`'),
 ('control-flow-extras',     'parse', r'reserved word `(typecase|case|label|exit|spawn|also|at|goto)`'),
 ('syntax-abstraction',      'parse', r'reserved word `(grammar|syntax)`'),
 ('declaration-modifiers',   'parse', r'reserved word `(private|public|native|test|covariant|contravariant|pure|io|override|hidden|settable|widens|coerce|coerces|absorbs|of|most|forbid|default|asif|typed|invariant|requires|ensures|provided|or)`|found Reserved\('),

 # `import java com.sun...{...}` and `import { a, b } from M` -- dotted module
 # paths and brace-delimited name lists.
 ('imports-and-exports',     'parse', r'found Dot|expected an export name|expected a newline or `;`, found Ident'),

 ('component-level-values',  'type',  r'a component-level value declaration|expected `:` or `\(`, found Eq'),
 ('generics-remaining',      'type',  r'is generic; write its static arguments|generic functional method|differ in their static parameters|instantiations in one component|takes no static arguments'),
 ('accessors',               'type',  r'is a getter or setter'),
 ('missing-library',         'type',  r'unknown (name|type) `'),
 ('reserved-word-other',     'parse', r'reserved word `'),
]
COMPILED = [(name, stage, re.compile(pat)) for name, stage, pat in RULES]

TOKEN = re.compile(r'found (\w+)')

def classify(message):
    for name, stage, pat in COMPILED:
        if pat.search(message):
            return name, stage
    # No invented feature name for the residue. A parse error that no rule
    # claims is reported by the TOKEN it stopped on, which is a fact, and a
    # type error by itself. Both are the queue for the next --raw pass.
    if message.startswith(('expected', 'unexpected')):
        found = TOKEN.search(message)
        token = found.group(1) if found else 'other'
        # `expected ZZ32, found ZZ64` names a TYPE, not a token. The parser
        # never says that, so the stage is inferred from the shape.
        stage = 'type' if re.match(r'^expected [A-Z(]', message) else 'parse'
        return f'{stage}-residue:{token}', stage
    return 'type-residue', 'type'

# ------------------------------------------------- source markers, for ROI
#
# Comments and string literals are stripped FIRST. Without that the Unicode
# marker fires on every copyright header in the corpus, and `opr` fires on
# prose. Fortress comments are `(* ... *)` and they NEST.
def strip(text):
    out, depth, i, n = [], 0, 0, len(text)
    while i < n:
        two = text[i:i + 2]
        if two == '(*':
            depth += 1; i += 2; continue
        if two == '*)' and depth:
            depth -= 1; i += 2; continue
        if depth:
            i += 1; continue
        if text[i] == '"':
            i += 1
            while i < n and text[i] != '"':
                i += 2 if text[i] == '\\' else 1
            i += 1; out.append(' '); continue
        out.append(text[i]); i += 1
    return ''.join(out)

MARKERS = {
 'operator-declarations':   re.compile(r'(^|\s)opr\s'),
 'operator-characters':     re.compile(r'\|\\|/\||(^|\s)opr\s+\S*[!?~$@]'),
 'unicode-identifiers':     re.compile(r'[^\x00-\x7F]'),
 'traits-objects':          re.compile(r'(^|\s)(trait|object|extends|comprises|excludes|abstract)(\s|\()'),
 'var-without-initialiser': re.compile(r'(^|\s)var\s+\w+\s*:[^=\n]*$', re.M),
 # Loose on purpose and it is the weakest row here: a brace list in an
 # `import` matches too. Read it next to `first`, not on its own.
 'aggregate-literals':      re.compile(r'<\||\|>|\{\s*\w'),
 'exceptions':              re.compile(r'(^|\s)(try|catch|throw|throws|finally)(\s|$)'),
 'control-flow-extras':     re.compile(r'(^|\s)(typecase|case|label|exit|spawn|also|at)\s'),
 'syntax-abstraction':      re.compile(r'(^|\s)(grammar|syntax)\s'),
 'function-types':          re.compile(r'(^|\s)fn(\s|\()|->'),
 'non-type-static-params':  re.compile(r'\[\\[^\]]*\b(nat|int|bool|unit|dim)\s'),
 'declaration-modifiers':   re.compile(r'(^|\s)(private|test|native|covariant|contravariant|hidden|settable|coerce|requires|ensures|invariant)\s'),
 'api-signature-files':     re.compile(r'(^|\s)api\s'),
 'imports-and-exports':     re.compile(r'^\s*(import|export)\s.*[{.]', re.M),
 'array-and-matrix-types':  re.compile(r':\s*\w+\[\d|Array\d\[\\'),
 'generator-bindings':      re.compile(r'\bif\s*\(.*\)\s*<-'),
 'comprehensions-and-big':  re.compile(r'(^|\s)BIG\s'),
}


def markers_of(path):
    try:
        text = strip(open(path, encoding='utf-8', errors='replace').read())
    except OSError:
        return set()
    return {name for name, pat in MARKERS.items() if pat.search(text)}

# ------------------------------------------------------------------ report
res    = results(root)
if opt.get('real'):
    res = [r for r in res if not NEGATIVE.search(r['path'])]
failed = [r for r in res if r['code'] != 0]
passed = len(res) - len(failed)

if opt.get('raw'):
    for m, n in collections.Counter(diagnostic(r) for r in failed).most_common():
        print(f'{n}\t{m}')
    sys.exit(0)

buckets = collections.defaultdict(list)
stages  = {}
for r in failed:
    name, stage = classify(diagnostic(r))
    buckets[name].append(r)
    stages[name] = stage

if 'category' in opt:
    want = opt['category']
    for r in sorted(buckets.get(want, []), key=lambda r: r['path']):
        print(f"{r['path']}\n    {diagnostic(r)}")
    print(f"\n{len(buckets.get(want, []))} file(s) in `{want}`")
    sys.exit(0)

ranked = sorted(buckets.items(), key=lambda kv: -len(kv[1]))
if top:
    ranked = ranked[:top]

if opt.get('json'):
    print(json.dumps({
        'root': root, 'total': len(res), 'compiled': passed, 'failed': len(failed),
        'categories': [{'category': k, 'stage': stages[k], 'count': len(v),
                        'example': v[0]['path'], 'message': diagnostic(v[0])}
                       for k, v in ranked],
    }, indent=2))
    sys.exit(0)

crash = [r for r in failed if r['code'] not in (0, 1)]
label = ' (must-fail and known-broken paths dropped)' if opt.get('real') else ''
print(f'== {root}: {len(res)} files, {passed} compile, {len(failed)} do not{label} ==')
if not opt.get('real'):
    hidden = sum(1 for r in failed if NEGATIVE.search(r['path']))
    print(f'   {hidden} of them live in must-fail or known-broken paths. '
          f'Re-run with --real to drop them.')
if crash:
    print(f'   !! {len(crash)} exited with a status other than 0 or 1 -- '
          f'that is a compiler crash, not a diagnostic')
    for r in crash[:5]:
        print(f"      {r['path']} (exit {r['code']})")

print('\n-- first blocker, by root cause. NOT a milestone plan; see the table below --')
print(f"{'count':>6}  {'%':>5}  {'stage':<5}  category")
for name, rs in ranked:
    print(f'{len(rs):>6}  {100 * len(rs) / max(len(failed), 1):>4.1f}%  {stages[name]:<5}  {name}')
    print(f"{'':>6}  {'':>5}  {'':<5}  e.g. {rs[0]['path']}")
    print(f"{'':>6}  {'':>5}  {'':<5}      {diagnostic(rs[0])[:96]}")

# ------------------------------------------------------- the ROI estimate
marked = {r['path']: markers_of(r['path']) for r in failed}
print('\n-- source markers. Plan from `alone`; it is the CEILING on one feature --')
print('   first   = files whose FIRST blocker is this. What the map above counts.')
print('   appears = files whose SOURCE uses it at all.')
print('   alone   = of those, files using NO OTHER marked feature.')
print('   first >> alone means the category is a WALL other work hides behind.\n')
print(f"{'first':>7} {'appears':>8} {'alone':>7}  category")
rows = []
for name in MARKERS:
    appears = [p for p, m in marked.items() if name in m]
    alone   = [p for p in appears if marked[p] == {name}]
    rows.append((len(alone), len(appears), len(buckets.get(name, [])), name))
for alone, appears, first, name in sorted(rows, reverse=True):
    print(f'{first:>7} {appears:>8} {alone:>7}  {name}')

nomarker = [p for p, m in marked.items() if not m]
print(f"\n{len(nomarker)} failing file(s) carry NO marked feature at all -- those are blocked")
print('on something this table cannot see, and are where a spike is the only answer.')

if 'bundle' in opt:
    want = set(opt['bundle'].split(','))
    unknown = want - set(MARKERS)
    if unknown:
        print(f'\nno marker for: {", ".join(sorted(unknown))}', file=sys.stderr)
    unlocked = [p for p, m in marked.items() if m and m <= want]
    print(f'\n-- bundle {sorted(want)} --')
    print(f'{len(unlocked)} failing file(s) use nothing outside the bundle.')
    print('That is a CEILING and not a forecast: it counts only what the markers see.')
    for p in sorted(unlocked)[:20]:
        print(f'   {p}')
    if len(unlocked) > 20:
        print(f'   ... and {len(unlocked) - 20} more')
PY
