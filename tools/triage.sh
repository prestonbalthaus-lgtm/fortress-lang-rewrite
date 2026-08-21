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
#   ./tools/triage.sh --real                 drop must-fail and known-broken
#   ./tools/triage.sh --conformance          the 1846 denominator: v1 minus
#                                            syntax abstraction (decision 1)
#   ./tools/triage.sh --reuse                skip the compile, read the cache
#   ./tools/triage.sh --selftest             only prove the instrument can refuse
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

cd "$repo" && FORTRESSC=$fortressc CACHE=${CACHE:-$repo/fortressc/build/triage.json} \
    TRIAGE_SHA=$(git rev-parse --short=9 HEAD 2>/dev/null || echo unknown) \
    TRIAGE_CC=$(sha256sum "$fortressc" 2>/dev/null | cut -c1-12 || echo unknown) \
    python3 - "$@" <<'PY'
import json, os, re, subprocess, sys, collections
from concurrent.futures import ThreadPoolExecutor

FORTRESSC = os.environ['FORTRESSC']
CACHE     = os.environ['CACHE']
# WHAT PRODUCED THE CACHE, stamped into it. Three agents share this tree and
# FORTRESSC pins the binary, so repo HEAD and compiler identity DIVERGE by
# design -- a report saying only "at <HEAD>" is not self describing.
SHA       = os.environ.get('TRIAGE_SHA', 'unknown')
CCID      = os.environ.get('TRIAGE_CC', 'unknown')

# ---------------------------------------------------------------- arguments
args, opt = sys.argv[1:], {}
i = 0
while i < len(args):
    a = args[i]
    if a in ('--root', '--top', '--category', '--bundle', '--jobs'):
        opt[a[2:]] = args[i + 1]; i += 2
    elif a in ('--raw', '--json', '--reuse', '--real', '--conformance',
               '--selftest'):
        opt[a[2:]] = True; i += 1
    else:
        print(f'unknown argument {a}', file=sys.stderr); sys.exit(2)
root  = opt.get('root', '.')

# Directories the upstream project itself marks as not-supposed-to-work, plus
# the XXX/SXX/DXX filename prefix the corpus uses for a must-FAIL test. 282 of
# the 1676 failures live here, so leaving them in overstates every category by
# about a sixth. --real drops them.
#
# THE LAST FIVE ARE NEW and they are why --real's denominator moved. Each is a
# path this repository already treats as out of scope somewhere else and the
# filter never knew about: `obsolete_interpreter_tests` is upstream's own word,
# `Fortify` is named in ROADMAP's out-of-scope list, and Sandbox, Documentation
# and CommunityMetrics are not corpus at all.
NEGATIVE = re.compile(
    r'not_passing_yet|not_working|long_term_not_working|staticError'
    r'|NeedBetterErrorMessages|compiler_regressions|parser_tests'
    r'|obsolete_interpreter_tests|(^|/)Fortify/|(^|/)Sandbox/'
    r'|(^|/)Documentation/|(^|/)CommunityMetrics/'
    r'|(^|/)(XXX|SXX|DXX)')

# ROADMAP decision 1 cuts syntax abstraction from v1, which makes the
# CONFORMANCE denominator 1846 and not 1956. The cut is 110 files and it is
# exactly one path -- measured, then asserted in --selftest, because a
# denominator nobody can reproduce is how 114-vs-126 happened.
#
# ONE FILE IS DELIBERATELY LEFT IN: Library/FortressSyntax.fsi first-blocks on
# the reserved word `grammar` and is not a syntax-abstraction TEST, it is a
# library api. Cutting by feature rather than by path would take it out and
# 1846 would stop reproducing.
SYNTAX_ABSTRACTION = re.compile(r'(^|/)syntax_abstraction_tests/')
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

def compile_one(path):
    r = subprocess.run([FORTRESSC, path, '--emit-obj', '-o', '/dev/null'],
                       capture_output=True, text=True)
    lines = r.stderr.strip().splitlines()
    return {'path': path, 'code': r.returncode, 'last': lines[-1] if lines else ''}

def _cache_load():
    with open(CACHE) as f:
        blob = json.load(f)
    # The cache was a bare list before it was stamped. Both shapes are read so
    # an existing cache does not have to be thrown away to gain a stamp.
    if isinstance(blob, list):
        return blob, 'unstamped', 'unstamped'
    return blob['records'], blob.get('sha', '?'), blob.get('cc', '?')


def results(base):
    global SHA, CCID
    if opt.get('reuse') and os.path.exists(CACHE):
        cached, SHA, CCID = _cache_load()
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
            json.dump({'sha': SHA, 'cc': CCID, 'records': out}, f)
    return out

# `file: 123..456: the message` -> `the message`. The span is what makes 340
# messages out of what is really a few dozen shapes.
SPAN = re.compile(r'^\S+?: \d+\.\.\d+: ')
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
 # Split out of `literal-forms` so each has a marker of its own. Both are
 # named v1 features (radix numerals by decision 4) and neither could be
 # priced while they shared a bucket with curly quotes and tab characters.
 ('radix-numerals',          'lex',   r'radix numerals'),
 ('character-literals',      'lex',   r'character literals'),
 ('literal-forms',           'lex',   r'curly-quote|string literal may not span|tab characters|integer literal does not fit'),

 ('operator-declarations',   'parse', r'reserved word `opr`|`opr` static parameters'),
 ('api-signature-files',     'n/a',   r'an `api` is a set of signatures'),
 ('comprehensions-and-big',  'parse', r'reserved word `BIG`'),

 # Traits and objects in their full 1.0 form. `trait aa(c:ZZ32, d:ZZ32)` --
 # value parameters on a trait -- is what `found LParen` is.
 ('traits-objects',          'parse', r'found KwExtends|found KwComprises|found KwExcludes|found KwObject|found KwSelf|found KwTrait|reserved word `(abstract|value|Self|object)`|unknown type `(Object|Any)`|is not a trait, so nothing can extend it|expected a field or method name, found LParen'),

 ('var-without-initialiser', 'parse', r'found KwVar'),

 # `export { a, b }` reported `found LBrace` and landed in aggregate-literals,
 # which is imports and not aggregates. 9 files. It has to be matched BEFORE
 # the brace rule below or the order decides it.
 ('imports-and-exports',     'parse', r'expected an export name'),

 # THE BAR FAMILY WAS ONE BUCKET AND IT IS THREE FEATURES. Decomposed by the
 # token the parser stopped on, measured at f81f41ace: BarBar 60, Bar 37,
 # LeftBar 26, LBrace 29 (20 after the export rule above), RightBar 2. Infix
 # `||` is the single largest first-blocker FEATURE in the corpus and it was
 # filed under aggregate literals, where nothing could see it.
 ('bars',                    'parse', r'found BarBar'),
 ('enclosing-operators',     'parse', r'found LeftBar|found RightBar|found Bar\b'),
 ('aggregate-literals',      'parse', r'found LBrace'),

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
    """Comments and string literals out, line structure kept.

    THIS MIRRORS lexer/src/raw.rs:51-142 AND IT DID NOT USED TO. The old
    version had no `(*)` case, so a `(*)` LINE comment read as an unclosed
    nested BLOCK comment and swallowed the rest of the file. Measured:
    ProjectFortress/library_tests/Integer1.fss stripped to 40 bytes of 3241,
    losing every marker past line 15. It is the same defect 04-state records
    against the SYNTAX_GUIDE counter -- fixed there, never carried here, and
    the marker table has been reading truncated source ever since.

    The three rules, from raw.rs: `(*)` comments to end of line and does NOT
    consume the terminator; `(* ... *)` nests; and `(*)` inside either form is
    INERT. Newlines are preserved because `var-without-initialiser` and
    `imports-and-exports` anchor on them with re.M.
    """
    out, i, n = [], 0, len(text)
    while i < n:
        c = text[i]
        if c == '"':
            i += 1
            while i < n and text[i] != '"':
                i += 2 if text[i] == '\\' else 1
            i += 1
            out.append(' ')
            continue
        if text.startswith('(*)', i):
            i = skip_line_comment(text, i + 3, out)
            continue
        if text.startswith('(*', i):
            i = skip_block_comment(text, i + 2, out)
            continue
        out.append(c)
        i += 1
    return ''.join(out)


def skip_line_comment(text, i, out):
    """`(*)` already consumed. Stops BEFORE the terminator, never on it."""
    depth, n = 0, len(text)
    while i < n:
        if text.startswith('(*)', i):
            i += 3; continue
        if text.startswith('(*', i):
            depth += 1; i += 2; continue
        if text.startswith('*)', i):
            if depth == 0:
                return i
            depth -= 1; i += 2; continue
        if text[i] == '\n':
            return i
        i += 1
    return i


def skip_block_comment(text, i, out):
    """`(*` already consumed. Nests; `(*)` inside is inert; keeps newlines."""
    depth, n = 1, len(text)
    while i < n:
        if text.startswith('(*)', i):
            i += 3; continue
        if text.startswith('(*', i):
            depth += 1; i += 2; continue
        if text.startswith('*)', i):
            depth -= 1; i += 2
            if depth == 0:
                return i
            continue
        if text[i] == '\n':
            out.append('\n')
        i += 1
    return i

MARKERS = {
 'operator-declarations':   re.compile(r'(^|\s)opr\s'),
 'operator-characters':     re.compile(r'\|\\|/\||(^|\s)opr\s+\S*[!?~$@]'),
 'unicode-identifiers':     re.compile(r'[^\x00-\x7F]'),
 'traits-objects':          re.compile(r'(^|\s)(trait|object|extends|comprises|excludes|abstract)(\s|\()'),
 'var-without-initialiser': re.compile(r'(^|\s)var\s+\w+\s*:[^=\n]*$', re.M),

 # A RUN OF TWO OR MORE VERTICAL LINES. This row did not exist, and the
 # `aggregate-literals` regex it was folded into -- `<\||\|>|\{\s*\w` --
 # could not match a bare `||` at all, so the largest single first-blocker
 # FEATURE in the corpus (60 files) had no ceiling recorded in the instrument
 # the project says to plan from. Covers infix `||`, `|||` and `||=`.
 'bars':                    re.compile(r'\|\|'),

 # `|x|` application, and the `<|...|>` and `|/ \|` bracket forms. Kept apart
 # from `bars` because the spec's own decomposition does: BarBar is an infix
 # operator, these are enclosing operators, and they are different grammar.
 'enclosing-operators':     re.compile(r'<\||\|>|\|\\|/\||\|[^|\n]{1,60}\|'),

 # Braces only now that the bar family has left. Still the weakest row here:
 # a brace list in an `import` matches too. Read it next to `first`.
 'aggregate-literals':      re.compile(r'\{\s*\w'),

 # A tuple TYPE, a tuple BINDER, or an arrow returning one. Deliberately not
 # `(a, b)` on its own -- that is indistinguishable from a two-argument call,
 # and a marker that matches every call site is worth nothing.
 'tuples':                  re.compile(r':\s*\(\s*\w[\w\[\]\\]*\s*,'
                                       r'|^\s*\(\s*\w+\s*,[^)]*\)\s*(:=|=)\s'
                                       r'|->\s*\(\s*\w+\s*,', re.M),

 # `1100_16`, `0006'0000_16`, and the letter-leading `FF_16` that lexes as an
 # identifier and reports `unknown name` instead (gap analysis 6.1).
 'radix-numerals':          re.compile(r"\b[0-9][0-9']*_[0-9A-Za-z]+\b"
                                       r"|\b[0-9A-F][0-9A-F']*_[0-9]+\b"),

 # `'a'`, `'\t'`. The lookarounds are load bearing: `'` is also the digit
 # separator inside a numeral, so `0006'0000` must not read as a literal.
 'character-literals':      re.compile(r"(?<![0-9A-Za-z_'])'(\\.|[^'\\\n])'"
                                       r"(?![0-9A-Za-z_'])"),

 'exceptions':              re.compile(r'(^|\s)(try|catch|throw|throws|finally)(\s|$)'),
 # WORD BOUNDARY, not whitespace. `spawn(makeWork(10))` and `(spawn ())` and
 # `case`/`label` after punctuation all escaped the old `(^|\s)...\s` form:
 # 6 files, 4 of them the spawn spellings.
 'control-flow-extras':     re.compile(r'(^|\W)(typecase|case|label|exit|spawn|also|at|goto)\b'),
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


def builtin_marker(records):
    """The `library-builtins` row, derived from the run rather than hand written.

    A file is marked if its source names something the compiler has nowhere to
    get. The name set is not a guess: it is every X the driver itself reported
    as `unknown name X` / `unknown type X` / `X takes no static arguments`
    across this run -- the third spelling included because 6.3 of the gap
    analysis shows it is what an unknown GENERIC name reports, which is most of
    the legacy library.

    It is a CEILING like every other marker and it is the loosest of them: a
    file that declares its own `List` is marked too. Read it next to `first`.
    """
    NAME = re.compile(r'unknown (?:name|type) `([^`]+)`'
                      r'|`([^`]+)` takes no static arguments')
    names = set()
    for r in records:
        if r['code'] == 0:
            continue
        for a, b in NAME.findall(diagnostic(r)):
            n = a or b
            if re.fullmatch(r'[A-Za-z_]\w*', n):
                names.add(n)
    if not names:
        return re.compile(r'(?!)'), 0
    pat = r'\b(' + '|'.join(sorted(names, key=len, reverse=True)) + r')\b'
    return re.compile(pat), len(names)


def markers_of(path):
    try:
        text = strip(open(path, encoding='utf-8', errors='replace').read())
    except OSError:
        return set()
    return {name for name, pat in MARKERS.items() if pat.search(text)}

# ---------------------------------------------------------------- selftest
#
# The instrument is not trusted until its own parts have refused. `strip` gets
# most of the cases because `strip` is where the defect was: it had no `(*)`
# case, so a LINE comment read as an unclosed BLOCK comment and swallowed the
# rest of the file. 192 corpus files contain `(*)` and all 192 were being
# truncated -- 833,063 non-newline bytes invisible to the marker table.
if opt.get('selftest'):
    ok = bad = 0
    def check(name, got, want):
        global ok, bad
        if got == want:
            ok += 1; print(f'ok    {name}')
        else:
            bad += 1; print(f'FAIL  {name}\n      got {got!r}, want {want!r}')

    print('== triage self test ==')
    check('a block comment goes',        strip('a (* b *) c'), 'a  c')
    check('block comments nest',         strip('a (* b (* c *) d *) e'), 'a  e')
    check('a `(*)` line comment stops at the newline',
                                         strip('a (*) b\nc'), 'a \nc')
    check('a `(*)` inside a block comment is INERT',
                                         strip('a (* x (*) y *) b'), 'a  b')
    check('a `(*)` does not open anything the rest of the file must close',
                                         strip('a (*) (* z *)\nq'), 'a \nq')
    check('a block comment keeps its newlines, so re.M anchors survive',
                                         strip('a (*\nb\n*) c'), 'a \n\n c')
    check('a string literal goes',       strip('a "b" c'), 'a   c')
    check('`(*` inside a string opens nothing',
                                         strip('a "(*" b'), 'a   b')
    # The regression itself, on the file that found it.
    _p = 'ProjectFortress/library_tests/Integer1.fss'
    if os.path.exists(_p):
        _t = strip(open(_p, encoding='utf-8', errors='replace').read())
        check('Integer1.fss keeps the source past its `(*)` line',
              '1100_16' in _t, True)
        check('Integer1.fss still loses its copyright header',
              'Oracle' in _t, False)

    # Every rule must be reachable: an earlier rule that already claims a
    # message makes a later one dead, which is how the split of `literal-forms`
    # nearly went in wrong.
    for cat, msg in (('radix-numerals',      'radix numerals are not in the M1 subset'),
                     ('character-literals',  'character literals are not in the M1 subset'),
                     ('bars',                'expected a newline or `;`, found BarBar'),
                     ('enclosing-operators', 'expected an expression, found Bar'),
                     ('aggregate-literals',  'expected an expression, found LBrace'),
                     ('imports-and-exports', 'expected an export name, found LBrace')):
        check(f'`{msg[:34]}...` classifies as {cat}', classify(msg)[0], cat)

    # The two denominators, asserted rather than believed.
    _c = corpus('.')
    check('the corpus is 1956 files', len(_c), 1956)
    check('the syntax-abstraction cut is 110 files',
          sum(1 for f in _c if SYNTAX_ABSTRACTION.search(f)), 110)
    check('the conformance denominator is 1846',
          sum(1 for f in _c if not SYNTAX_ABSTRACTION.search(f)), 1846)
    check('every marker regex compiles and is distinct',
          len(MARKERS), len({p.pattern for p in MARKERS.values()}))
    print(f'\n{ok} passed, {bad} failed')
    sys.exit(1 if bad else 0)

# ------------------------------------------------------------------ report
res    = results(root)
if opt.get('conformance'):
    res = [r for r in res if not SYNTAX_ABSTRACTION.search(r['path'])]
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
        'root': root, 'sha': SHA, 'compiler': CCID,
        'total': len(res), 'compiled': passed, 'failed': len(failed),
        'categories': [{'category': k, 'stage': stages[k], 'count': len(v),
                        'example': v[0]['path'], 'message': diagnostic(v[0])}
                       for k, v in ranked],
    }, indent=2))
    sys.exit(0)

crash = [r for r in failed if r['code'] not in (0, 1)]
label = ' (must-fail and known-broken paths dropped)' if opt.get('real') else ''
if opt.get('conformance'):
    label = (' (syntax abstraction cut by ROADMAP decision 1)' +
             (', must-fail and known-broken dropped' if opt.get('real') else ''))
print(f'== {root}: {len(res)} files, {passed} compile, {len(failed)} do not{label} ==')
print(f'   repo {SHA}, compiler {CCID}')
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
MARKERS['library-builtins'], _builtin_names = builtin_marker(res)
marked = {r['path']: markers_of(r['path']) for r in failed}
print('\n-- source markers. Plan from `alone`; it is the CEILING on one feature --')
print(f'   library-builtins is derived from this run: {_builtin_names} names the')
print('   driver itself reported as unknown. It is the loosest row here.')
print('   first   = files whose FIRST blocker is this. What the map above counts.')
print('   appears = files whose SOURCE uses it at all.')
print('   alone   = of those, files using NO OTHER marked feature.')
print('   alone*  = the same, holding `library-builtins` constant. Read THIS one.')
print('   first >> alone means the category is a WALL other work hides behind.\n')
print(f"{'first':>7} {'appears':>8} {'alone':>7} {'alone*':>7}  category")
LIB = 'library-builtins'
rows = []
for name in MARKERS:
    appears = [p for p, m in marked.items() if name in m]
    alone   = [p for p in appears if marked[p] == {name}]
    # alone* ignores `library-builtins`, which appears in almost every failing
    # file and would otherwise take every other row to zero. It is not a
    # feature anyone implements on its own -- it is the library bootstrap,
    # which is group 2's entire job -- so holding it constant is what lets the
    # rest of the column order anything.
    astar   = [p for p in appears if marked[p] - {LIB} == {name}]
    rows.append((len(astar), len(alone), len(appears),
                 len(buckets.get(name, [])), name))
for astar, alone, appears, first, name in sorted(rows, reverse=True):
    star = '-' if name == LIB else str(astar)
    print(f'{first:>7} {appears:>8} {alone:>7} {star:>7}  {name}')

nomarker = [p for p, m in marked.items() if not m]
print(f"\n{len(nomarker)} failing file(s) carry NO marked feature at all -- those are blocked")
print('on something this table cannot see, and are where a spike is the only answer.')
print('That number was 221 under --real before the `strip()` fix in this commit,')
print('and it was an artifact: the markers were reading truncated source.')

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
