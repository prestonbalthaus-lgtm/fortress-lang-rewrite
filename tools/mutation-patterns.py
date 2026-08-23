"""Every MUTATIONS row's `from` must match EXACTLY ONCE in its file.

A row matching ZERO times silently does nothing -- the table reports it as
COULD NOT BE APPLIED at best, and older harnesses reported nothing at all. A row
matching TWICE is worse: `atomic-gate`'s `Slot::Cell { pointer, ty }` started
matching twice when the spawn outliner landed and quietly disabled that whole
table, which is recorded in 04-state.md as the reason to re-check.

Running every `--mutate` after a milestone takes the better part of an hour.
CHECKING THAT EVERY PATTERN STILL MATCHES ONCE TAKES SECONDS AND IS MOST OF WHAT
THAT BUYS, so it is worth having on its own:

    python3 tools/mutation-patterns.py     # exit 1 if any row is stale

Escapes are interpreted the way the gates themselves do -- `printf '%b'` --
because `control-flow-gate` writes a multi-line pattern with `\n` in it.
"""

import codecs
import pathlib
import re
import sys

repo = pathlib.Path(__file__).resolve().parent.parent
bad = 0
for gate in sorted((repo / 'tools').glob('*-gate.sh')):
    text = gate.read_text()
    m = re.search(r'^MUTATIONS=\(\n(.*?)^\)', text, re.S | re.M)
    if not m:
        continue
    rows = []
    for line in m.group(1).splitlines():
        line = line.strip()
        if not line or line.startswith('#'):
            continue
        if line[0] in "'\"" and line[-1] == line[0]:
            rows.append(line[1:-1])
    # `IFS='|' read -r file from to label <<<"$entry"` -- the one that reads
    # THIS table, which is the one naming a variable called `from`.
    reader = re.search(r"IFS='\|' read -r ((?:\w+ )*from(?: \w+)*) <<<", text)
    want = len(reader.group(1).split()) if reader else 4
    problems = []
    for row in rows:
        parts = row.split('|')
        if len(parts) < 2:
            problems.append(f'      UNPARSEABLE: {row[:60]}')
            bad = 1
            continue
        # A ROW HAS EXACTLY AS MANY FIELDS AS THE GATE'S OWN `read` NAMES.
        # `IFS='|' read -r file from to label` splits on EVERY bar, so a `||`
        # inside a pattern silently shifts the replacement and the label into
        # the wrong variables -- and the uniqueness check below would still
        # pass, because the TRUNCATED pattern happens to match once. Recorded
        # in 04-state.md as a gate-authoring trap; this is the instrument for
        # it, and the field count is READ FROM THE GATE rather than assumed,
        # because `control-flow-gate` carries a fifth column of its own.
        if len(parts) != want:
            problems.append(f'      {len(parts)} FIELDS, want {want}: {row[:70]!r}')
            bad = 1
            continue
        f, frm = parts[0], parts[1]
        frm = codecs.decode(frm, 'unicode_escape')
        target = repo / 'fortressc' / f
        if not target.is_file():
            target = repo / f
        if not target.is_file():
            problems.append(f'      MISSING FILE {f}')
            bad = 1
            continue
        hits = target.read_text().count(frm)
        if hits != 1:
            problems.append(f'      {hits} hits: {f} :: {frm[:70]!r}')
            bad = 1
    print(f'{gate.name:<28} {len(rows):>2} rows' + ('\n' + '\n'.join(problems) if problems else ''))
sys.exit(bad)
