"""Every committed evidence report must name the implementer and the reviewer.

`docs/contracts/CLI-EVIDENCE.md` puts "reviewer identity/method" in the evidence
record minimum, and the owner ruling of 2026-09-28 requires that the actual
implementer/reviewer identities and whether the reviewer's context was fresh are
always recorded.  Every report committed under `docs/findings/evidence/` must
therefore name the agents that really ran, and the report text and the harness
that writes it must agree, so a regeneration cannot silently put a placeholder
back.

Rally itself is not reachable from an offline check, so the review facts read
out of the Rally activity log are committed in
`docs/findings/2026-10-02-m16-a-fu2-rally-review-snapshot.json` (the campaign
family, found while reviewing M16-A, work item M16-A-FU2 / #479),
`docs/findings/2026-10-02-m16-a-fu4-rally-review-snapshot.json` (the rest,
#483) and `docs/findings/2026-10-02-m16-a-fu5-rally-review-snapshot.json` (the
three reports that named only one of the two agents, M16-A-FU5 / #484), and
this check resolves the reports against all three.

The reader discovers the harnesses instead of listing them.  A hand-maintained
list of harness files is a list that rots: a new stage adds a file and a report,
and a reader that only opens the files it was told about silently stops
covering the new one.  #479 covered one harness file and so let the same defect
through in every other family.

The check works in two levels, on purpose.  A hand-over placeholder in any
committed report is always a failure: Rally only merges a stage through
`complete_review` or its landing queue, so a committed report has always had a
reviewer.  A stage the snapshots have not caught up with is an advisory, because
stages land continuously and a correct new report must not turn an unrelated
branch red.  The rules that need Rally's review facts - the actors a report must
name, whether the review is independent, and whether the report says anything
about the reviewer's context - are therefore snapshot-backed rules, while the
placeholder, `claim`, `review.method` and harness-agreement rules are
unconditional.

A new stage adds a harness and a report, so the task that adds it extends the
matching snapshot with that stage's Rally implementer and reviewer in the same
commit.  That is an advisory, not a failure.  A new report that still says
`reviewer: none yet` *is* a failure.  An entry can also be added later, once the
stage's merge event really exists: #578 added F14-D.7's entry to the FU4 snapshot
after #490 had landed, because an entry written before the merge can only guess
at the event that closes the review.

The reader has to be told what a harness does only where the harness itself
is ambiguous, and each such reading is a decision somebody can review:

* **an encoder is not a shape.**  `jstr(...)`, `json(...)` and `m01lc_json(...)`
  all quote a value into the report, so which one a harness happens to call
  says nothing about where the identity comes from.  F04-D-order-archives
  encodes with `json(&reviewer)` and reads `CS_EVIDENCE_REVIEWER`, which makes
  it a runtime-identity harness like every `jstr(&reviewer)` one; refusing it
  for its name (M16-A-FU6 / #523) turned a legitimate harness into an
  unreadable one and put five tests red behind it.  The shape comes from the
  argument, never from the encoder's name.
* **a `task_id` the format string does not spell is still one the file
  spells** - through the constant its placeholder argument names, or through
  the report table (`M01lcReport { task_id, review }`) the caller chose.  Both
  are read literally, neither is guessed, a table entry is its own harness, and
  a marker that resolves to neither is still reported as unresolvable (this is
  #492 / T388-a's case, which this file now covers).

The run-time exemption itself stays unlisted: `runtime_identity_expectation`
derives it from the committed reports, so a new runtime harness has to be a
decision the reader and that derivation agree on rather than a line somebody
remembered to edit.  The two harnesses this task (#523) asks about are the
case: `F22-H` reads `CS_EVIDENCE_REVIEWER` and `F11-D2` reads
`CS_EVIDENCE_REVIEW`, neither spells a `review.identity` literal, so both are
legitimately runtime harnesses - which is the decision recorded here, and the
derivation above is what fails if one of them ever stops being one.

Run with:

    python3 -m unittest discover -s tools/tests -p 'test_evidence_review_identity.py' -v
"""
from __future__ import annotations

import json
from pathlib import Path
import re
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
CAMPAIGN_SNAPSHOT = ROOT / 'docs/findings/2026-10-02-m16-a-fu2-rally-review-snapshot.json'
NON_CAMPAIGN_SNAPSHOT = ROOT / 'docs/findings/2026-10-02-m16-a-fu4-rally-review-snapshot.json'
FU5_SNAPSHOT = ROOT / 'docs/findings/2026-10-02-m16-a-fu5-rally-review-snapshot.json'
REPORTS = ROOT / 'docs/findings/evidence'
# Every evidence report is written by a Rust file, most of them by one under a
# `tests/` directory.  A few harnesses live in a production `src/` file
# (F06-D, F07-D, F08-D, F09-D, F13-B, F13-C), so the reader walks both
# roots rather than only the test directories.
HARNESS_ROOTS = ('crates', 'tools')
# Matched against the path *relative to the checkout*, never against the
# absolute one: this repository is developed in a directory called `private/`
# on some machines, and a reader that excluded every harness because of the
# name of an ancestor above the checkout would report nothing and look green.
IGNORED = frozenset({'target', '.git', 'private'})
# The key a `"review"` marker gets when the reader cannot resolve it to a task
# id, so an unresolvable harness is reported rather than skipped.
UNRESOLVED = 'unresolved review marker at offset {}'

# The placeholder wording the reports used while they claimed that no review had
# happened, or handed the job to a reviewer that had not run yet.  None of it
# survives once the real reviewer is named.  `independent review is pending` is
# the wording M19-A, M21-A and M24-A shipped with, `not yet independent` is
# F12-K's, and the last three are the instructions to a future reviewer that
# F10-D and F51-D shipped with - the same gap written as an order rather than as
# a claim, which is why a rule that only looked for `none yet` missed them.
PLACEHOLDERS = ('none yet', 'not yet assigned', 'not yet known', 'not yet independent',
                'to be recorded', 'self-check', 'the rally reviewer regenerates',
                'independent review is pending', 'must re-run',
                'must record its own identity', 'are recorded at review time')

TASK_ID = re.compile(r'\\"task_id\\": \\"([^"\\]+)\\"')
CLAIM = re.compile(r'\\"claim\\": \\"([a-z_]+)\\"')
REVIEW_MARKER = r'\"review\": {{\"identity\": {}, \"method\": {}}}'
# The value-encoding call at the start of an argument, with or without a module
# path (`cs_inspect`'s harnesses call `super::jstr`).  Every harness encodes a
# string into the JSON report through a helper, and the helpers are not all
# spelled `jstr`: F04-D-order-archives encodes with `json(&reviewer)` and the
# m01lc family with `m01lc_json(...)`.  Within this family the helper's *name*
# decides nothing here - the argument does - so the shape never comes from the
# name of the encoder, only from what its argument resolves to.  A helper this
# list does not spell out matches nothing and reads as `unknown`, which
# `review_problems` reports as a harness the reader cannot parse; it is never
# guessed at and never trusted for its name either.
ENCODING_CALL = re.compile(r'\A\s*(?:\w+::)*(?:jstr|(?:\w+_)?json)\s*\(')
# The argument of such a call: either a literal, `&review_identity()`, or
# `&review`.
ARGUMENT = re.compile(r'\A\s*&?\s*(\w+)\b')
# `fn review_identity() -> String {` and `let review = env_var("CS_EVIDENCE_REVIEWER");`
FUNCTION = r'fn\s+{}(?:\s*<[^>]*>)?\s*\(\s*\)\s*->\s*String\b'
DECLARATION = r'(?:let\s+{}\b|fn\s+{}\b)[^;{{]{{0,200}}?(?:env::var|env_var)\(\s*"(?P<env>[A-Z0-9_]+)"'

# The task id of a report the harness fills in at run time rather than spelling
# into the format string: `"task_id": {},` with the value passed as the format
# call's first placeholder argument.  #492's case.
RUNTIME_TASK_ID = re.compile(r'\\"task_id\\": \{\}')
# The m01lc harness writes three reports through one format call, choosing the
# task id from a `M01lcReport { task_id: "...", ..., review: NAME }` table
# entry, so its task ids and identities live in the table rather than at the
# marker.
TABLE_TASK_ID = re.compile(r'\btask_id:\s*"([^"\\]+)"')
TABLE_REVIEW = re.compile(r'\breview:\s*("(?:[^"\\]|\\.)*"|\w+)\s*,')

# A whole evidence harness, small enough to read: a `format!` whose `review`
# object is filled from two `jstr` arguments, with a `\`-continued identity
# literal because that is how every real harness writes one.
MINIMAL_HARNESS = '''fn evidence_report_x99_a() {
    let report = format!(
        "{{\\n\\
         \\x20\\"task_id\\": \\"X99-A\\",\\n\\
         \\x20\\"review\\": {{\\"identity\\": {}, \\"method\\": {}}},\\n\\
         \\x20\\"claim\\": \\"implemented\\"\\n\\
         }}\\n",
        jstr(
            "one \\
             two"
        ),
        jstr("three"),
    );
}
'''

# A whole evidence harness that encodes its run-time identity with `json(...)`
# instead of `jstr(...)`, the way F04-D-order-archives writes its report.
JSON_HARNESS = '''fn evidence_report_x99_a() {
    let reviewer = env_var("CS_EVIDENCE_REVIEWER");
    let report = format!(
        "{{\\n\\
         \\x20\\"task_id\\": \\"X99-A\\",\\n\\
         \\x20\\"review\\": {{\\"identity\\": {}, \\"method\\": {}}},\\n\\
         \\x20\\"claim\\": \\"implemented\\"\\n\\
         }}\\n",
        json(&reviewer),
        json(METHOD),
    );
}
'''

# The same run-time identity, with the `task_id` filled in at run time from a
# constant rather than spelled into the format string (#492's case).
RUNTIME_TASK_HARNESS = '''const TASK: &str = "X98-B";

fn evidence_report_x98_b() {
    let reviewer = env_var("CS_EVIDENCE_REVIEWER");
    let report = format!(
        "{{\\n\\
         \\x20\\"task_id\\": {},\\n\\
         \\x20\\"review\\": {{\\"identity\\": {}, \\"method\\": {}}},\\n\\
         \\x20\\"claim\\": \\"implemented\\"\\n\\
         }}\\n",
        jstr(TASK),
        json(&reviewer),
        json(METHOD),
    );
}
'''

# One format call, two report table entries: the shape `accept_f20_d_validation.rs`
# uses to write three reports through `m01lc_write_report`.
TABLE_HARNESS = '''const REVIEW_A: &str = "implementer: x/y; reviewer: a/b, fresh context, not independent";

const REVIEW_B: &str = "implementer: x/y; reviewer: x/y, fresh context, not independent";

struct Spec {
    task_id: &'static str,
    review: &'static str,
}

const SPEC_A: Spec = Spec {
    task_id: "X97-C",
    review: REVIEW_A,
};

const SPEC_B: Spec = Spec {
    task_id: "X96-D",
    review: REVIEW_B,
};

fn evidence_report(spec: &Spec) {
    let report = format!(
        "{{\\n\\
         \\x20\\"task_id\\": {},\\n\\
         \\x20\\"review\\": {{\\"identity\\": {}, \\"method\\": {}}},\\n\\
         \\x20\\"claim\\": \\"implemented\\"\\n\\
         }}\\n",
        m01lc_json(spec.task_id),
        m01lc_json(spec.review),
        m01lc_json(METHOD),
    );
}
'''


def read_rust_string(text, quote):
    r"""Return the Rust string literal that starts at `text[quote] == '"'`.

    Line continuations (a backslash before a newline) drop the leading
    whitespace of the next line, exactly like the `\` continuations in
    `evidence.rs` are joined by the compiler.
    """
    assert text[quote] == '"', text[quote - 20:quote + 20]
    out, i = [], quote + 1
    while True:
        char = text[i]
        if char == '\\':
            following = text[i + 1]
            if following == '\n':
                i += 2
                while text[i] in ' \t':
                    i += 1
                continue
            out.append({'n': '\n', 't': '\t', '"': '"', '\\': '\\'}[following])
            i += 2
            continue
        if char == '"':
            return ''.join(out)
        out.append(char)
        i += 1


def string_literals(text):
    """Every Rust string literal value in `text`, continuations joined.

    The identity a harness commits usually spans many lines with
    backslash-newline continuations, so the raw bytes of the file do not hold
    the report's `review.identity` and a plain substring search would call even
    a literal harness `runtime`.  This walks the literals the way the compiler
    joins them: a backslash before a newline drops the next line's leading
    whitespace, and the escapes the harness texts use are decoded.  A char
    literal that contains a quote (`'"'`) is stepped over, and an escape this
    reader does not know is kept as its own character, so a format string
    elsewhere in the file cannot stop the scan.
    """
    values, i, size = [], 0, len(text)
    while i < size:
        if text[i] == "'" and i + 2 < size:
            if text[i + 1] == '\\' and i + 3 < size and text[i + 3] == "'":
                i += 4
                continue
            if text[i + 2] == "'":
                i += 3
                continue
        if text[i] != '"':
            i += 1
            continue
        value = []
        i += 1
        while i < size:
            char = text[i]
            if char == '\\':
                following = text[i + 1] if i + 1 < size else ''
                if following == '\n':
                    i += 2
                    while i < size and text[i] in ' \t':
                        i += 1
                    continue
                value.append({'n': '\n', 't': '\t', '"': '"', '\\': '\\',
                              'r': '\r', '0': '\0'}.get(following, following))
                i += 2
                continue
            if char == '"':
                i += 1
                break
            value.append(char)
            i += 1
        values.append(''.join(value))
    return values


def format_arguments(text, marker_start):
    """The top-level arguments of the `format!` call that writes the report.

    `review.identity` and `review.method` are the last two arguments of that
    call in every harness in this repository, and the report's `review` object
    has exactly those two keys, so the mapping from argument to field is fixed
    by the schema.  Counting the `{}` placeholders instead would mean parsing
    the format string's escapes; taking the last two arguments does not.  Each
    argument comes back with the offset it starts at, because a Rust string
    literal can only be read at an absolute position in the file.
    """
    call = text.rfind('format!(', 0, marker_start)
    if call < 0:
        return []
    arguments, index, depth, start, in_string = [], call + len('format!('), 1, call + len('format!('), False
    while depth:
        char = text[index]
        if in_string:
            if char == '\\':
                index += 2
                continue
            if char == '"':
                in_string = False
        elif char == '"':
            in_string = True
        elif char in '([{':
            depth += 1
        elif char in ')]}':
            depth -= 1
            if depth == 0:
                break
        elif char == ',' and depth == 1:
            arguments.append((text[start:index], start))
            start = index + 1
        index += 1
    tail = text[start:index]
    if tail.strip():
        arguments.append((tail, start))
    return arguments


def read_identity(text, start):
    """The `review.identity` the harness fills in after the marker, and its shape.

    `shape` is how the identity reaches the report, so a shape this reader
    cannot parse is a hole that has to be reported rather than quietly skipped:

    * `literal` - a `jstr` string literal in the file, and the value is the text
      of that literal.  This is the shape a report can be compared to.
    * `function` - the return value of a `fn ...() -> String` in the same file.
    * `runtime` - read from the environment when the harness runs, so there is
      no committed literal to compare and the reviewer supplies the text.
    * `unknown` - anything else.  A failure: an unreadable harness is not a
      checked one.
    """
    arguments = format_arguments(text, start)
    if len(arguments) < 3:
        return {'identity': None, 'shape': 'unknown', 'via': None}
    raw, offset = arguments[-2]
    call = ENCODING_CALL.match(raw)
    if not call:
        return {'identity': None, 'shape': 'unknown', 'via': raw.strip()[:60]}
    argument = raw[call.end():]
    if argument.lstrip().startswith('"'):
        return {'identity': read_rust_string(text, offset + raw.index('"', call.end())),
                'shape': 'literal', 'via': None}
    callee = ARGUMENT.match(argument)
    if not callee:
        return {'identity': None, 'shape': 'unknown', 'via': argument.strip()[:60]}
    name = callee.group(1)
    for function in re.finditer(FUNCTION.format(re.escape(name)), text):
        body = text.find('"', function.end())
        if body > 0:
            return {'identity': read_rust_string(text, body),
                    'shape': 'function', 'via': name}
    for declaration in re.finditer(DECLARATION.format(re.escape(name), re.escape(name)), text):
        return {'identity': None, 'shape': 'runtime', 'via': declaration.group('env')}
    return {'identity': None, 'shape': 'unknown', 'via': name}


def constant_string(text, name):
    r"""The literal a `const NAME: &str = "..."` of this file holds, or `None`.

    A harness that fills the report's `task_id` at run time names it through a
    constant (`const TASK: &str = "M01-LC-AUDIO-DEVICE";`).  The reader follows
    the name to the literal the file spells, the same way it follows an
    identity that comes from a `fn`: it reads what is written and returns
    `None` when nothing is, never inventing the value.
    """
    for declaration in re.finditer(r'\b(?:const|static)\s+' + re.escape(name)
                                   + r'\b[^;{]*=', text):
        quote = text.find('"', declaration.end())
        end = text.find(';', declaration.end())
        if quote > 0 and (end < 0 or quote < end):
            return read_rust_string(text, quote)
    return None


def argument_literal(text, raw, offset):
    """The literal an argument of the report's `format!` call plainly is, or `None`.

    `raw` is the argument and `offset` where it starts in the file, so a literal
    inside it can be read at an absolute position.  A name is resolved to the
    constant it refers to; anything else - a field of the table the caller
    chooses from - is `None`, because this reader reads Rust, it does not
    evaluate it.
    """
    call = ENCODING_CALL.match(raw)
    argument = raw[call.end():] if call else raw
    argument = argument.lstrip()
    while argument.startswith('&'):
        argument = argument[1:].lstrip()
    if argument.startswith('"'):
        return read_rust_string(text, offset + raw.index('"'))
    name = ARGUMENT.match(argument)
    if name:
        return constant_string(text, name.group(1))
    return None


def table_reports(text):
    """Every `(task id, identity)` the report tables of this file define.

    The m01lc harness writes three reports through one `format!` call, taking
    the `task_id` and the `review` identity from the `M01lcReport { task_id:
    "...", ..., review: NAME }` entry its caller passed.  Neither is at the
    marker, so the reader reads the table: one entry per `task_id`, each with
    the identity its own `review` field names.  An entry whose identity the
    file does not spell comes back as `None` so the marker is reported rather
    than that one report silently escaping the cross-check.
    """
    matches = list(TABLE_TASK_ID.finditer(text))
    reports = []
    for index, match in enumerate(matches):
        stop = matches[index + 1].start() if index + 1 < len(matches) else len(text)
        window = text[match.end():min(stop, match.end() + 2000)]
        review = TABLE_REVIEW.search(window)
        if review is None:
            reports.append((match.group(1), None))
            continue
        value = review.group(1)
        if value.startswith('"'):
            identity = read_rust_string(text, match.end() + review.start(1))
        else:
            identity = constant_string(text, value)
        reports.append((match.group(1), identity))
    return reports


def harness_reports(text, start, before):
    """Every `(task id, identity record)` one `review` marker writes, or `[]`.

    A harness that spells the `task_id` into the format string carries it
    before the marker.  One that fills it in at run time spells it elsewhere
    instead - in the constant the first placeholder argument names, or in the
    report table that argument indexes - and the reader follows it there,
    because a task id it cannot find is a harness it cannot cross-check.  `[]`
    means the marker resolves to nothing at all, which `read_harness` reports
    as an unresolvable marker rather than dropping quietly.
    """
    ids = TASK_ID.findall(text[before:start])
    if ids:
        return [(ids[-1], read_identity(text, start))]
    arguments = format_arguments(text, start)
    if len(arguments) > 1 and RUNTIME_TASK_ID.search(arguments[0][0]):
        task_id = argument_literal(text, arguments[1][0], arguments[1][1])
        if task_id is not None:
            return [(task_id, read_identity(text, start))]
        table = table_reports(text)
        if table:
            return [(name, {'identity': identity, 'shape': 'literal', 'via': None}
                     if identity is not None
                     else {'identity': None, 'shape': 'unknown',
                           'via': 'a report table entry with no readable identity'})
                    for name, identity in table]
    return []


def read_harness(text):
    """Return the task id, `review.identity` and `claim` of every harness in one file.

    A `"review"` marker the reader cannot resolve to a task id and a `claim` is
    a hole, so it is keyed by its own offset and reported by
    `review_problems` instead of being skipped: a harness that is quietly
    dropped is a harness that silently stops being cross-checked.  Each marker is
    read inside the window between it and the next one, so a harness can never
    borrow the task id or the `claim` of its neighbour in a file that writes
    several reports - and a marker that writes several reports through one
    format call contributes one harness per report it can write (see
    `harness_reports`).
    """
    starts = [match.start() for match in re.finditer(re.escape(REVIEW_MARKER), text)]
    harnesses = {}
    for index, start in enumerate(starts):
        before = starts[index - 1] if index else 0
        after = starts[index + 1] if index + 1 < len(starts) else len(text)
        claim = CLAIM.search(text, start, after)
        reports = harness_reports(text, start, before)
        if not reports or not claim:
            harnesses[UNRESOLVED.format(start)] = {
                'claim': None, 'identity': None, 'shape': 'unknown',
                'via': 'no task id before the marker' if not reports
                else 'no claim after the marker'}
            continue
        for task_id, identity in reports:
            harnesses[task_id] = {'claim': claim.group(1), **identity}
    return harnesses


def harness_files(root=ROOT):
    """Every Rust file that writes an evidence report, discovered rather than listed."""
    files = []
    for base in HARNESS_ROOTS:
        for path in sorted((root / base).rglob('*.rs')):
            if any(part in IGNORED for part in path.relative_to(root).parts):
                continue
            if REVIEW_MARKER in path.read_text():
                files.append(path)
    return files


def read_harnesses_from(files):
    """Read the harnesses of `{relative path: text}`, keyed by the `task_id` each writes."""
    harnesses = {}
    for source, text in files.items():
        for key, harness in read_harness(text).items():
            harnesses[key] = dict(harness, source=source)
    return harnesses


def read_harnesses(root=ROOT):
    """Every committed evidence harness, keyed by the `task_id` it writes."""
    return read_harnesses_from({str(path.relative_to(root)): path.read_text()
                                for path in harness_files(root)})


def read_snapshot(path=CAMPAIGN_SNAPSHOT):
    return json.loads(Path(path).read_text())


def read_reports(directory=REPORTS):
    """Return every committed report keyed by its `task_id`."""
    return {report['task_id']: report
            for report in (json.loads(path.read_text())
                           for path in sorted(Path(directory).glob('*.json')))}


def committed_identity_literals(harnesses):
    """Every string literal value in each harness source, keyed by relative path."""
    literals = {}
    for harness in harnesses.values():
        source = harness['source']
        if source not in literals:
            literals[source] = set(string_literals((ROOT / source).read_text()))
    return literals


def runtime_identity_expectation(harnesses, reports):
    """The harnesses whose committed report no literal can be compared against.

    A harness that writes `review.identity` from a committed literal can be
    compared byte-for-byte with its report; a harness that reads the identity
    from the environment (`CS_EVIDENCE_REVIEW`/`CS_EVIDENCE_REVIEWER`) cannot,
    and that exemption is what this set records.  It is derived from the reports
    rather than listed, so a stage that adds a runtime harness needs no edit
    here: when no string literal in the harness holds the report's identity, the
    harness must be the one that supplies it at run time.  A list would rot -
    this check pinned seven keys, and by 2026-10-03 eleven more committed
    harnesses took the same deliberate exemption - while an equality against
    this derivation still fails when the reader classifies a harness one way and
    the report's own text says the other.
    """
    literals = committed_identity_literals(harnesses)
    return {key for key, harness in harnesses.items()
            if key in reports
            and reports[key]['review']['identity'] not in literals[harness['source']]}


def states_the_reviewer_context(identity):
    """Does the identity say whether the reviewer's context was fresh?

    `context` and `fresh` both answer the owner ruling's question — "whether
    the reviewer's context was fresh" — in the words a report actually uses.
    Matching one token rather than the other turned a correct report (M16-A-FU1
    wrote "same agent and model, fresh session") into a failure, which is how a
    check teaches its readers to distrust it.
    """
    said = identity.lower()
    return 'context' in said or 'fresh' in said


def claim_actors(task, role):
    """Every agent that held a claim of `role` on this task, latest first.

    A stage can be implemented or reviewed more than once (F02-C was started by
    one agent and submitted by another after a lease expired; F02-B was reviewed
    twice), so an earlier claim is still a claim the report has to account for.
    """
    if role == 'implementer':
        actors = [task['implementer']['actor']]
        actors += [claim['actor']
                   for claim in task['implementer'].get('earlier_implement_claims', [])]
        return actors
    return [task['reviewer']['actor']] + [claim['actor']
                                          for claim in task['reviewer'].get('earlier_review_claims', [])]


def review_problems(snapshots, harnesses, reports):
    """Return `(problems, unrecorded)` for the committed evidence reports.

    `snapshots` is one snapshot or a list of them.  `problems` is what must
    fail: a placeholder where a reviewer belongs, a report that disagrees with
    the harness that writes it, a claim above `implemented`, a harness this
    reader cannot read, and a snapshot-backed stage whose identity misses a fact
    the Rally log recorded.  `unrecorded` names the reports the snapshots have no
    Rally facts for yet; they are an advisory, because stages land continuously
    and a correct new report must not fail an unrelated branch.
    """
    snapshots = list(snapshots) if isinstance(snapshots, (list, tuple)) else [snapshots]
    problems, unrecorded = [], []
    known, event_types = {}, set()
    for snapshot in snapshots:
        tasks = snapshot['tasks']
        numbers = [task['rally_task'] for task in tasks]
        if numbers != sorted(numbers):
            problems.append(f'{snapshot["task"]}: snapshot tasks are not in Rally task order')
        for task in tasks:
            if task['task_key'] in known:
                problems.append(f'{task["task_key"]}: recorded in more than one snapshot')
            known[task['task_key']] = task
        event_types |= set(snapshot['review_event_types'])

    for key in sorted(reports):
        report = reports[key]
        where = f'docs/findings/evidence/{key}.json'
        identity = report['review']['identity']
        task = known.get(key)
        if not report['review']['method'].strip():
            problems.append(f'{where}: `review.method` is empty')
        for placeholder in PLACEHOLDERS:
            if placeholder in identity.lower():
                if task is None:
                    problems.append(f'{where}: `review.identity` still says {placeholder!r}; every'
                                    f' merged stage has had a reviewer, so add {key}\'s Rally'
                                    f' implementer and reviewer to the snapshot and name them here')
                else:
                    problems.append(f'{where}: `review.identity` still says {placeholder!r} while'
                                    f' Rally records a review claim for {key} by'
                                    f' {task["reviewer"]["actor"]}')
        if report['claim'] != 'implemented':
            problems.append(f'{where}: claim {report["claim"]!r}; a merge awards `checked` only and'
                            f' nobody self-awards a level')
        harness = harnesses.get(key)
        if harness is not None:
            if harness['claim'] != 'implemented':
                problems.append(f'{harness["source"]}: claim {harness["claim"]!r}; a merge awards'
                                f' `checked` only and nobody self-awards a level')
            if harness['shape'] == 'unknown':
                problems.append(f'{harness["source"]}: the reader cannot work out how the {key}'
                                f' harness fills in `review.identity` (argument {harness["via"]!r}),'
                                f' so the report and the harness are not cross-checked')
            elif harness['shape'] != 'runtime' and harness['identity'] != identity:
                problems.append(f'{harness["source"]}: the {key} harness writes a'
                                f' different `review.identity` than {where}, so regenerating the report'
                                f' would undo the recorded reviewer')
        if task is None:
            unrecorded.append(key)
            continue
        if not states_the_reviewer_context(identity):
            problems.append(f'{where}: `review.identity` says nothing about whether the'
                            f' reviewer\'s context was fresh')
        if not task['implementer']['actor'] or not task['reviewer']['actor']:
            problems.append(f'{key}: the snapshot records no implementer or no reviewer')
            continue
        if task['reviewer']['merge_event']['type'] not in event_types:
            problems.append(f'{key}: the reviewer is closed by {task["reviewer"]["merge_event"]["type"]}'
                            f', which is not a review event')
        if Path(task['evidence_report']).name != f'{key}.json':
            problems.append(f'{key}: the snapshot points at {task["evidence_report"]}')
        for role in ('implementer', 'reviewer'):
            for actor in claim_actors(task, role):
                if actor not in identity:
                    problems.append(f'{where}: `review.identity` does not name the {role} {actor!r}')
        if task['implementer']['actor'] == task['reviewer']['actor'] \
                and 'not independent' not in identity.lower():
            problems.append(f'{where}: the reviewer is the implementer\'s own instance'
                            f' ({task["reviewer"]["actor"]}) but `review.identity` does not say the'
                            f' review is not independent')
    for key in sorted(harnesses):
        if key in reports:
            continue
        if key.startswith('unresolved review marker'):
            problems.append(f'{harnesses[key]["source"]}: a `review` block at {key} cannot be'
                            f' resolved ({harnesses[key]["via"]}), so this harness is not'
                            f' cross-checked against a report')
            continue
        problems.append(f'docs/findings/evidence/{key}.json: no committed report, but {key} has'
                        f' an evidence harness')
    return problems, unrecorded


class EvidenceReviewIdentityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.snapshot = read_snapshot(CAMPAIGN_SNAPSHOT)
        cls.non_campaign = read_snapshot(NON_CAMPAIGN_SNAPSHOT)
        cls.single_agent = read_snapshot(FU5_SNAPSHOT)
        cls.snapshots = [cls.snapshot, cls.non_campaign, cls.single_agent]
        cls.harnesses = read_harnesses()
        cls.reports = read_reports()

    def snapshot_by_key(self):
        return {task['task_key']: task for task in self.snapshot['tasks']}

    def non_campaign_by_key(self):
        return {task['task_key']: task for task in self.non_campaign['tasks']}

    def single_agent_by_key(self):
        return {task['task_key']: task for task in self.single_agent['tasks']}

    # -- M16-A-FU2 (#479): the campaign-binding family ------------------------

    def test_accept_m16_a_fu2_snapshot_resolves_every_recorded_stage(self):
        ids = [task['rally_task'] for task in self.snapshot['tasks']]
        self.assertEqual(len(set(ids)), len(ids), 'two snapshot entries share a Rally task')
        self.assertEqual(ids, sorted(ids), 'snapshot tasks are not in Rally task order')
        self.assertLessEqual(set(self.snapshot_by_key()), set(self.harnesses),
                             'the snapshot names a stage no harness writes')
        for key, task in self.snapshot_by_key().items():
            self.assertIn(key, self.reports, task)
            self.assertEqual(self.reports[key]['task_id'], key)
            self.assertRegex(task['reviewer']['review_claim_started'], r'^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ$')
            self.assertLessEqual(task['implementer']['claim_started'],
                                 task['reviewer']['review_claim_started'])
            # The commit on main that carries the stage. Only its shape is checked
            # here: resolving it would need the git history, which a shallow CI
            # checkout does not have.  Every entry in all three snapshots carries the
            # field, so the check is the same in each of them.
            self.assertRegex(task['merged_sha'], r'^[0-9a-f]{40}\Z')

    def test_accept_m16_a_fu2_reports_name_the_actual_implementer_and_reviewer(self):
        problems, unrecorded = review_problems(self.snapshots, self.harnesses, self.reports)
        self.assertEqual(problems, [])
        for key in unrecorded:
            print(f'note: {key} has no Rally review facts in the snapshots yet', file=sys.stderr)

    def test_accept_m16_a_fu2_detects_drift(self):
        """A reviewer, an implementer or a harness that stops naming the real agents must fail."""
        key = 'M13-A'
        recorded = self.snapshot_by_key()[key]

        reverted = copy_reports(self.reports)
        reverted[key]['review']['identity'] = reverted[key]['review']['identity'].replace(
            recorded['reviewer']['actor'], 'none yet')
        self.assertTrue(review_problems(self.snapshots, self.harnesses, reverted)[0])

        renamed = copy_reports(self.reports)
        renamed[key]['review']['identity'] = renamed[key]['review']['identity'].replace(
            recorded['implementer']['actor'], 'claude-9')
        self.assertTrue(review_problems(self.snapshots, self.harnesses, renamed)[0])

        silent = copy_reports(self.reports)
        silent[key]['review']['identity'] = re.sub(r'(?i)not independent', 'independent',
                                                   silent[key]['review']['identity'])
        self.assertTrue(review_problems(self.snapshots, self.harnesses, silent)[0])

        drifted = dict(self.harnesses)
        drifted[key] = dict(drifted[key], identity='implementer: nobody; reviewer: nobody')
        self.assertTrue(review_problems(self.snapshots, drifted, self.reports)[0])

        awarded = copy_reports(self.reports)
        awarded[key]['claim'] = 'checked'
        self.assertTrue(review_problems(self.snapshots, self.harnesses, awarded)[0])

        quiet = copy_reports(self.reports)
        quiet[key]['review']['identity'] = re.sub(r'(?i)(no |not |none is claimed|was |is )?'
                                                 r'(a )?(fresh|new) (session|context)|context',
                                                 'that review', quiet[key]['review']['identity'])
        self.assertNotIn('context', quiet[key]['review']['identity'].lower())
        self.assertNotIn('fresh', quiet[key]['review']['identity'].lower())
        self.assertTrue(any('whether the reviewer\'s context was fresh' in problem
                            for problem in review_problems(self.snapshots, self.harnesses, quiet)[0]),
                        quiet[key]['review']['identity'])

    def test_accept_m16_a_fu2_the_context_rule_reads_either_word(self):
        """"fresh session" answers the ruling; matching one token must not fail a correct report."""
        for identity, expected in (
                ('implementer: x/y; reviewer: x/y again, the same instance, not independent',
                 False),
                ('implementer: x/y; reviewer: x/y again, fresh context, not independent', True),
                ('implementer: x/y; reviewer: x/y again, same agent and model, fresh session, '
                 'not independent', True),
                ('implementer: x/y; reviewer: y/z, a different instance, no fresh context claimed',
                 True)):
            self.assertIs(states_the_reviewer_context(identity), expected, identity)

    def test_accept_m16_a_fu2_a_new_stage_is_an_advisory_until_it_gaps(self):
        """A stage the snapshot has not caught up with must not fail, unless it really gaps."""
        key = 'M99-A'
        identity = ('implementer: x/y (Rally #999); reviewer: a/b, a different agent instance with '
                    'fresh context, so not independent original-reference evidence')
        stage = self.harnesses['M13-A']
        harnesses = dict(self.harnesses, **{key: dict(stage, identity=identity)})
        clean = copy_reports(self.reports, **{key: report(key, identity)})
        problems, unrecorded = review_problems(self.snapshots, harnesses, clean)
        self.assertEqual(problems, [])
        self.assertIn(key, unrecorded)
        gapped = copy_reports(self.reports, **{key: report(key, 'implementer: x/y; reviewer: none yet')})
        problems, unrecorded = review_problems(self.snapshots, harnesses, gapped)
        self.assertIn(key, unrecorded)
        self.assertIn(f'docs/findings/evidence/{key}.json: `review.identity` still says '
                      f"'none yet'; every merged stage has had a reviewer, so add {key}'s Rally "
                      'implementer and reviewer to the snapshot and name them here', problems)

    def test_accept_m16_a_fu2_the_pending_review_template_is_a_gap_too(self):
        """`… an independent review is pending` is the same gap, with or without `none yet`."""
        key = 'M98-A'
        gapped = ('implementer: x/y (Rally #998); reviewer: x/y again, the same agent instance, so '
                  'this is not independent review; an independent review is pending')
        harnesses = dict(self.harnesses, **{key: dict(self.harnesses['M13-A'],
                                                      identity=gapped)})
        problems, unrecorded = review_problems(self.snapshots, harnesses,
                                               copy_reports(self.reports, **{key: report(key, gapped)}))
        self.assertIn(key, unrecorded)
        self.assertTrue(any(f'docs/findings/evidence/{key}.json: `review.identity` still says '
                            "'independent review is pending'" in problem for problem in problems),
                        problems)

    def test_accept_m16_a_fu2_harness_reader_joins_rust_line_continuations(self):
        self.assertEqual(read_harness(MINIMAL_HARNESS),
                         {'X99-A': {'claim': 'implemented', 'identity': 'one two',
                                    'shape': 'literal', 'via': None}})

    # -- M16-A-FU4 (#483): the same rule over every evidence harness ----------

    def test_accept_m16_a_fu4_the_non_campaign_snapshot_resolves_every_recorded_stage(self):
        ids = [task['rally_task'] for task in self.non_campaign['tasks']]
        self.assertEqual(len(set(ids)), len(ids), 'two snapshot entries share a Rally task')
        self.assertEqual(ids, sorted(ids), 'snapshot tasks are not in Rally task order')
        self.assertFalse(set(self.non_campaign_by_key()) & set(self.snapshot_by_key()),
                         'a stage is recorded in both snapshots')
        for key, task in self.non_campaign_by_key().items():
            self.assertIn(key, self.reports, task)
            self.assertEqual(self.reports[key]['task_id'], key)
            self.assertRegex(task['implementer']['claim_started'], r'^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ$')
            self.assertRegex(task['reviewer']['review_claim_started'], r'^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ$')
            self.assertLessEqual(task['implementer']['claim_started'],
                                 task['reviewer']['review_claim_started'])
            self.assertIn(task['reviewer']['merge_event']['type'],
                          self.non_campaign['review_event_types'])
            # The commit on main that carries the stage. Only its shape is checked
            # here: resolving it would need the git history, which a shallow CI
            # checkout does not have.
            self.assertRegex(task['merged_sha'], r'^[0-9a-f]{40}\Z')
        self.assertLessEqual(set(self.non_campaign_by_key()), set(self.reports))

    def test_accept_m16_a_fu4_the_reader_covers_the_whole_family(self):
        """The reader must open the real harness files, not a list somebody maintained."""
        expected = {
            'crates/cs_app/tests/campaign/evidence.rs': ('M01-A', 'literal'),
            # A report table entry: one format call, one harness per `task_id`.
            'crates/cs_app/tests/accept_f20_d_validation.rs': ('M01-LC-ANIM-RECORDS', 'literal'),
            'crates/cs_app/tests/evidence_report_m01_lc_audio_device.rs':
                ('M01-LC-AUDIO-DEVICE', 'runtime'),
            'crates/cs_app/tests/render/paint.rs': ('F17-C-PAINT', 'literal'),
            'crates/cs_app/tests/text/evidence.rs': ('F51-D', 'function'),
            'crates/cs_app/tests/world/audit/evidence.rs': ('F18-D', 'function'),
            'crates/cs_assets/tests/accept_f04_d_member_collisions.rs': ('T342', 'runtime'),
            'crates/cs_assets/tests/accept_f04_d_order_archives.rs': ('F04-D-order-archives',
                                                                     'runtime'),
            'crates/cs_assets/tests/evidence_report_f02_b.rs': ('F02-B', 'literal'),
            'crates/cs_assets/tests/evidence_report_f04_d.rs': ('F04-D', 'literal'),
            'crates/cs_content/tests/evidence_report_f10_c_02.rs': ('F10-C.02', 'literal'),
            'crates/cs_content/tests/evidence_report_f56_a.rs': ('F56-A', 'runtime'),
            'crates/cs_formats/tests/gamez/evidence.rs': ('F10-D', 'literal'),
            'crates/cs_formats/tests/zbd/t340.rs': ('T340', 'literal'),
            'crates/cs_formats/tests/zbd/t343.rs': ('T343', 'literal'),
            'crates/cs_formats/tests/zbd/t344.rs': ('T344', 'literal'),
            'tools/cs_inspect/src/interp.rs': ('F07-D', 'literal'),
            'tools/cs_inspect/src/script_discovery.rs': ('F13-B', 'literal'),
            'tools/cs_inspect/src/textures.rs': ('F08-D', 'literal'),
            'tools/cs_inspect/src/zbd.rs': ('F06-D', 'literal'),
            'tools/cs_inspect/tests/evidence_report_f02_c.rs': ('F02-C', 'literal'),
            'tools/cs_inspect/tests/evidence_report_f02_d.rs': ('F02-D', 'literal'),
            'tools/cs_inspect/tests/evidence_report_f31_d.rs': ('F31-D', 'literal'),
        }
        for source, (key, shape) in expected.items():
            self.assertEqual(self.harnesses[key]['source'], source, key)
            self.assertEqual(self.harnesses[key]['shape'], shape, key)
        self.assertGreaterEqual(len(self.harnesses), len(expected),
                                'the reader stopped finding evidence harnesses')
        self.assertNotIn('unknown', {harness['shape'] for harness in self.harnesses.values()})
        runtime = {key for key, harness in self.harnesses.items() if harness['shape'] == 'runtime'}
        self.assertLessEqual(runtime, set(self.reports),
                             'a runtime-identity harness has no committed report to compare against')
        self.assertEqual(runtime, runtime_identity_expectation(self.harnesses, self.reports),
                         'the harnesses that read `review.identity` at run time no longer match the'
                         ' committed reports no literal can produce')
        for key, harness in self.harnesses.items():
            if harness['shape'] == 'runtime':
                self.assertIn(harness['via'], ('CS_EVIDENCE_REVIEW', 'CS_EVIDENCE_REVIEWER'), key)

    def test_accept_m16_a_fu4_detects_drift_in_a_non_campaign_report(self):
        """A non-campaign report put back to `self-check`, or renamed, must be reported."""
        for key, reviewer in (('F02-B', 'Jakob - Devin SWE-2/devin-1'),
                              ('F04-D', 'claude-1/claude-1'),
                              ('F12-K', 'Jakob - Devin SWE-2/devin-1'),
                              ('T344', 'claude-1/claude-1')):
            reverted = copy_reports(self.reports)
            reverted[key]['review']['identity'] = reverted[key]['review']['identity'].replace(
                reviewer, 'self-check')
            problems = review_problems(self.snapshots, self.harnesses, reverted)[0]
            self.assertTrue(any("still says 'self-check'" in problem for problem in problems),
                            f'{key} kept its reviewer and still passed: {problems}')

            renamed = copy_reports(self.reports)
            renamed[key]['review']['identity'] = renamed[key]['review']['identity'].replace(
                reviewer, 'agent-9')
            self.assertTrue(review_problems(self.snapshots, self.harnesses, renamed)[0], key)

            drifted = copy_reports(self.reports)
            harnesses = dict(self.harnesses)
            harnesses[key] = dict(harnesses.get(key, self.harnesses['F02-B']),
                                  identity='implementer: nobody; reviewer: nobody')
            self.assertTrue(review_problems(self.snapshots, harnesses, drifted)[0], key)

            awarded = copy_reports(self.reports)
            awarded[key]['claim'] = 'checked'
            self.assertTrue(review_problems(self.snapshots, self.harnesses, awarded)[0], key)

            quiet = copy_reports(self.reports)
            quiet[key]['review']['identity'] = re.sub(r'(?i)context|fresh', 'that review',
                                                      quiet[key]['review']['identity'])
            self.assertNotIn('context', quiet[key]['review']['identity'].lower())
            self.assertNotIn('fresh', quiet[key]['review']['identity'].lower())
            self.assertTrue(any('whether the reviewer\'s context was fresh' in problem
                                for problem in review_problems(self.snapshots, self.harnesses, quiet)[0]),
                            key)

    def test_accept_m16_a_fu4_the_placeholder_list_covers_every_shipped_wording(self):
        """Each wording a committed report has actually shipped with must be caught."""
        shipped = ('self-check', 'none yet', 'not yet assigned at hand-over',
                   'not yet independently reviewed', 'the Rally reviewer regenerates this report',
                   'an independent review is pending', 'must re-run the four steps',
                   'must record its own identity', 'are recorded at review time')
        for phrase in shipped:
            self.assertTrue(any(placeholder in phrase.lower() for placeholder in PLACEHOLDERS),
                            f'{phrase!r} is not covered by PLACEHOLDERS')
        key = 'X01-A'
        for phrase in PLACEHOLDERS:
            identity = (f'implementer: x/y (Rally #9001); reviewer: {phrase}; fresh context, '
                        'not independent')
            harnesses = dict(self.harnesses, **{key: dict(self.harnesses['F02-B'],
                                                          identity=identity)})
            problems = review_problems(self.snapshots, harnesses,
                                       copy_reports(self.reports, **{key: report(key, identity)}))[0]
            self.assertTrue(any(phrase in problem for problem in problems),
                            f'{phrase!r} passed: {problems}')

    def test_accept_m16_a_fu4_a_runtime_identity_harness_is_exempt_and_pinned(self):
        """`CS_EVIDENCE_REVIEW` harnesses have no literal; the exemption is derived, not listed."""
        runtime = {key for key, harness in self.harnesses.items() if harness['shape'] == 'runtime'}
        self.assertLessEqual(runtime, set(self.reports),
                             'a runtime-identity harness has no committed report to compare against')
        self.assertEqual(runtime, runtime_identity_expectation(self.harnesses, self.reports),
                         'the harnesses that read `review.identity` at run time no longer match the'
                         ' committed reports no literal can produce')
        problems = review_problems(self.snapshots, self.harnesses, self.reports)[0]
        for key in runtime:
            self.assertIn(self.harnesses[key]['via'],
                          ('CS_EVIDENCE_REVIEW', 'CS_EVIDENCE_REVIEWER'), key)
            self.assertFalse([problem for problem in problems
                              if self.harnesses[key]['source'] in problem], key)
        # A shape the reader cannot parse is a failure, not a silent exemption.
        harnesses = dict(self.harnesses, **{'F02-B': dict(self.harnesses['F02-B'],
                                                          shape='unknown', identity=None,
                                                          via='review')})
        self.assertTrue(any('cannot work out how the F02-B harness' in problem
                            for problem in review_problems(self.snapshots, harnesses, self.reports)[0]))

    def test_accept_m16_a_fu4_the_runtime_expectation_comes_from_the_report(self):
        """The exemption is derived from the report, so it must follow the report.

        A harness is expected to read `review.identity` at run time exactly when
        no string literal in its source holds the report's identity.  A report
        whose text is committed leaves its harness pinned no matter what shape
        the reader reports, and a report no literal holds is expected to be
        runtime even when its harness committed one: that is what makes the
        equality a check rather than a list.
        """
        # A real literal harness, so the first case holds the actual committed
        # text rather than a string that happens to appear in the source (the
        # F14-D env-var name is one such coincidence).
        literal = {'F02-B': self.harnesses['F02-B']}
        identity = self.harnesses['F02-B']['identity']
        self.assertEqual(runtime_identity_expectation(literal, {'F02-B': report('F02-B', identity)}),
                         set(),
                         'a report whose identity is a committed literal is not an exemption')
        self.assertEqual(runtime_identity_expectation(
            literal, {'F02-B': report('F02-B', f'{identity} reviewed by nobody committed')}),
            {'F02-B'}, 'a report no committed literal holds must be a runtime exemption')
        # A real runtime harness: the report text no literal holds is the one the
        # environment supplies.
        self.assertEqual(runtime_identity_expectation({'F14-D': self.harnesses['F14-D']},
                                                      {'F14-D': self.reports['F14-D']}),
                         {'F14-D'}, 'a report only the run-time harness supplies is an exemption')

    def test_accept_m16_a_fu4_string_literals_join_continuations_and_skip_char_literals(self):
        """The derivation's lexer must join `\\`-continuations and ignore char literals.

        Every identity literal is `\\`-continued, so a raw search for the report
        text never finds it.  A char literal such as `'"'` must not be read as
        the start of a string, and an escape this lexer does not decode must not
        stop the scan, or a real literal harness would look like a runtime one.
        """
        text = ('let a = "one \\\n           two";\n'
                'let quote = \'"\';\n'
                'let b = "three";\n'
                'let c = "\\u{1f600} tail";\n')
        self.assertEqual(string_literals(text), ['one two', 'three', 'u{1f600} tail'])

    def test_accept_m16_a_fu4_a_harness_the_reader_cannot_resolve_is_reported(self):
        """A `review` block with no task id, or no `claim`, is a hole, not a harness to skip."""
        def without(needle):
            return '\n'.join(line for line in MINIMAL_HARNESS.splitlines() if needle not in line)

        for needle, reason in (('task_id', 'no task id before the marker'),
                               ('claim', 'no claim after the marker')):
            harnesses = read_harnesses_from({'crates/demo/tests/evidence.rs': without(needle)})
            key = next(iter(harnesses))
            self.assertTrue(key.startswith('unresolved review marker'), key)
            self.assertEqual(harnesses[key]['shape'], 'unknown')
            self.assertEqual(harnesses[key]['via'], reason)
            self.assertTrue(any('cannot be resolved' in problem
                                and 'crates/demo/tests/evidence.rs' in problem
                                for problem in review_problems(self.snapshots, harnesses,
                                                               self.reports)[0]),
                            harnesses)

    def test_accept_m16_a_fu4_each_harness_reads_only_its_own_window(self):
        """A harness must not borrow the task id or the `claim` of a neighbour in the same file."""
        second = MINIMAL_HARNESS.replace('X99-A', 'X98-A').replace('implemented', 'checked')
        self.assertEqual({key: harness['claim']
                          for key, harness in read_harness(MINIMAL_HARNESS + second).items()},
                         {'X99-A': 'implemented', 'X98-A': 'checked'})
        # Drop the first harness's own `claim`: the second one's must not stand
        # in for it, or a self-awarded `checked` would read as `implemented`.
        silent = '\n'.join(line for line in MINIMAL_HARNESS.splitlines() if 'claim' not in line)
        unresolved = read_harness(silent + second)
        self.assertEqual([harness['claim'] for key, harness in unresolved.items()
                          if not key.startswith('unresolved review marker')], ['checked'])
        unresolved_keys = [key for key in unresolved if key.startswith('unresolved review marker')]
        self.assertEqual(len(unresolved_keys), 1)
        self.assertEqual(unresolved[unresolved_keys[0]]['via'], 'no claim after the marker')

    def test_accept_m16_a_fu4_discovery_ignores_only_paths_inside_the_checkout(self):
        """A checkout that lives under a directory named `private` is still read."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / 'private' / 'checkout'
            harness = root / 'crates/demo/tests/evidence.rs'
            harness.parent.mkdir(parents=True)
            harness.write_text(MINIMAL_HARNESS)
            (root / 'crates/demo/target').mkdir()
            (root / 'crates/demo/target/generated.rs').write_text(MINIMAL_HARNESS)
            self.assertEqual([path.relative_to(root).as_posix() for path in harness_files(root)],
                             ['crates/demo/tests/evidence.rs'])
            self.assertEqual(read_harnesses(root)['X99-A']['shape'], 'literal')
            self.assertEqual(self.harnesses['M01-A']['source'],
                             'crates/cs_app/tests/campaign/evidence.rs',
                             'the reader found nothing in this checkout either')

    # -- M16-A-FU5 (#484): the three reports that named only one agent --------

    def test_accept_m16_a_fu5_snapshot_resolves_every_recorded_stage(self):
        ids = [task['rally_task'] for task in self.single_agent['tasks']]
        self.assertEqual(len(set(ids)), len(ids), 'two snapshot entries share a Rally task')
        self.assertEqual(ids, sorted(ids), 'snapshot tasks are not in Rally task order')
        self.assertFalse(set(self.single_agent_by_key())
                         & (set(self.snapshot_by_key()) | set(self.non_campaign_by_key())),
                         'a stage is recorded in more than one snapshot')
        for key, task in self.single_agent_by_key().items():
            self.assertIn(key, self.reports, task)
            self.assertEqual(self.reports[key]['task_id'], key)
            self.assertRegex(task['implementer']['claim_started'], r'^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ$')
            self.assertRegex(task['reviewer']['review_claim_started'], r'^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ$')
            self.assertLessEqual(task['implementer']['claim_started'],
                                 task['reviewer']['review_claim_started'])
            self.assertIn(task['reviewer']['merge_event']['type'],
                          self.single_agent['review_event_types'])
            # Shape only, as in the other two snapshots.
            self.assertRegex(task['merged_sha'], r'^[0-9a-f]{40}\Z')
        self.assertLessEqual(set(self.single_agent_by_key()), set(self.reports))

    def test_accept_m16_a_fu5_the_three_stages_are_no_longer_advisories(self):
        """Naming both agents must clear the three reports out of the advisory list."""
        problems, unrecorded = review_problems(self.snapshots, self.harnesses, self.reports)
        self.assertEqual(problems, [])
        for key in ('F05-D', 'F07-D', 'F31-D'):
            self.assertIn(key, self.reports)
            self.assertNotIn(key, unrecorded, f'{key} is still an advisory')
            self.assertIn(self.single_agent_by_key()[key]['implementer']['actor'],
                          self.reports[key]['review']['identity'])
            self.assertIn(self.single_agent_by_key()[key]['reviewer']['actor'],
                          self.reports[key]['review']['identity'])

    def test_accept_m16_a_fu5_detects_drift_in_a_single_agent_report(self):
        """Each of the three must fail if the agent, the context or the independence claim goes."""
        for key, actor in (('F05-D', 'bunny-1/bunny-1'),
                           ('F07-D', 'glm-1/deepseek-1'),
                           ('F31-D', 'deepseek-1/deepseek-1')):
            renamed = copy_reports(self.reports)
            renamed[key]['review']['identity'] = renamed[key]['review']['identity'].replace(
                actor, 'agent-9')
            self.assertTrue(review_problems(self.snapshots, self.harnesses, renamed)[0], key)

            reverted = copy_reports(self.reports)
            reverted[key]['review']['identity'] = reverted[key]['review']['identity'].replace(
                actor, 'none yet')
            self.assertTrue(any("still says 'none yet'" in problem
                                for problem in review_problems(self.snapshots, self.harnesses,
                                                               reverted)[0]), key)

            silent = copy_reports(self.reports)
            silent[key]['review']['identity'] = re.sub(r'(?i)not independent', 'independent',
                                                       silent[key]['review']['identity'])
            self.assertTrue(review_problems(self.snapshots, self.harnesses, silent)[0], key)

            quiet = copy_reports(self.reports)
            quiet[key]['review']['identity'] = re.sub(r'(?i)context|fresh', 'that review',
                                                      quiet[key]['review']['identity'])
            self.assertNotIn('context', quiet[key]['review']['identity'].lower())
            self.assertNotIn('fresh', quiet[key]['review']['identity'].lower())
            self.assertTrue(any("whether the reviewer's context was fresh" in problem
                                for problem in review_problems(self.snapshots, self.harnesses,
                                                               quiet)[0]), key)

            drifted = copy_reports(self.reports)
            harnesses = dict(self.harnesses)
            harnesses[key] = dict(harnesses.get(key, self.harnesses['F02-B']),
                                  identity='implementer: nobody; reviewer: nobody')
            self.assertTrue(review_problems(self.snapshots, harnesses, drifted)[0], key)

    # -- F14-D.7-SNAPSHOT (#578): a review recorded after its merge event -----

    def test_accept_f14_d_7_is_no_longer_an_advisory(self):
        """The snapshot-backed rules must cover F14-D.7, whose report already named both agents.

        The report itself was honest from the start; what was missing was the Rally
        review facts, and an entry that is missing cannot fail anything, so this pins
        the stage as resolved instead of relying on an advisory note having gone away.
        """
        key = 'F14-D.7'
        problems, unrecorded = review_problems(self.snapshots, self.harnesses, self.reports)
        self.assertEqual(problems, [])
        self.assertIn(key, self.reports)
        self.assertNotIn(key, unrecorded, f'{key} is still an advisory')
        recorded = self.non_campaign_by_key()[key]
        self.assertEqual(recorded['rally_task'], 490)
        self.assertEqual(recorded['evidence_report'], f'docs/findings/evidence/{key}.json')
        identity = self.reports[key]['review']['identity']
        for role in ('implementer', 'reviewer'):
            # Named at all: `assertIn` would call an empty actor a name.
            self.assertTrue(recorded[role]['actor'], role)
            self.assertIn(recorded[role]['actor'], identity, role)
        # One instance did both, so the report has to say the review is not independent;
        # the snapshot records the spelling the report uses next to Rally's own actor
        # string, which is the same instance.
        self.assertEqual(recorded['implementer']['actor'], recorded['reviewer']['actor'])
        self.assertEqual(recorded['implementer']['rally_actor'], 'bunny-alpha-2/bunny-alpha-2')
        self.assertEqual(recorded['reviewer']['rally_actor'], recorded['implementer']['rally_actor'])
        self.assertIn('not independent', identity.lower())
        # A landing-queue merge is a system event; the reviewer approved it.
        reviewer = recorded['reviewer']
        approval, merge = reviewer['approval_event'], reviewer['merge_event']
        self.assertEqual(merge['type'], 'task.merged')
        self.assertEqual(merge['actor'], 'rally')
        self.assertEqual(approval['type'], 'task.approved')
        self.assertEqual(approval['actor'], reviewer['rally_actor'])
        self.assertLessEqual(approval['at'], merge['at'])
        # `report_written_by` is read off the report's own `created_at` against these
        # claim windows (`report_written_by_meaning` in the snapshot), so it is checked
        # against the report it describes rather than taken on trust: a copy written
        # inside the review claim is the reviewer's, whatever the entry claims.
        created = self.reports[key]['created_at']
        self.assertRegex(created, r'^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ\Z')
        self.assertLessEqual(reviewer['review_claim_started'], created)
        self.assertLessEqual(created, approval['at'])
        self.assertEqual(recorded['report_written_by'], 'reviewer')
        self.assertIs(recorded['report_regenerated_on_reviewed_commit'], True)

    def test_accept_f14_d_7_detects_drift(self):
        """Dropping the entry, the reviewer's name or the independence claim must fail."""
        key = 'F14-D.7'
        recorded = self.non_campaign_by_key()[key]

        # The entry removed from the snapshot: the stage falls back to an advisory, which
        # `test_accept_f14_d_7_is_no_longer_an_advisory` reports, and nothing else moves.
        problems, unrecorded = review_problems(drop_task(self.snapshots, key),
                                               self.harnesses, self.reports)
        self.assertIn(key, unrecorded, 'removing the entry did not even downgrade the stage')
        self.assertEqual(problems, [])

        # The entry kept but its reviewer unnamed: the snapshot stops naming one.
        nameless = with_task(self.non_campaign, key, 'reviewer', {**recorded['reviewer'], 'actor': ''})
        problems = review_problems(nameless, self.harnesses, self.reports)[0]
        self.assertTrue(any('records no implementer or no reviewer' in problem for problem in problems),
                        problems)

        # The report stops naming the reviewer.
        renamed = copy_reports(self.reports)
        renamed[key]['review']['identity'] = renamed[key]['review']['identity'].replace(
            recorded['reviewer']['actor'], 'agent-9')
        problems = review_problems(self.snapshots, self.harnesses, renamed)[0]
        self.assertTrue(any('does not name the reviewer' in problem for problem in problems), problems)

        # The same instance on both sides, so the independence claim is load-bearing.
        silent = copy_reports(self.reports)
        silent[key]['review']['identity'] = re.sub(r'(?i)not independent', 'independent',
                                                   silent[key]['review']['identity'])
        self.assertTrue(any('does not say the review is not independent' in problem
                            for problem in review_problems(self.snapshots, self.harnesses, silent)[0]),
                        'a report that no longer says the review is not independent passed')

        reverted = copy_reports(self.reports)
        reverted[key]['review']['identity'] = reverted[key]['review']['identity'].replace(
            recorded['reviewer']['actor'], 'none yet')
        self.assertTrue(any("still says 'none yet'" in problem
                            for problem in review_problems(self.snapshots, self.harnesses, reverted)[0]))

        quiet = copy_reports(self.reports)
        quiet[key]['review']['identity'] = re.sub(r'(?i)context|fresh', 'that review',
                                                  quiet[key]['review']['identity'])
        self.assertNotIn('context', quiet[key]['review']['identity'].lower())
        self.assertNotIn('fresh', quiet[key]['review']['identity'].lower())
        self.assertTrue(any("whether the reviewer's context was fresh" in problem
                            for problem in review_problems(self.snapshots, self.harnesses, quiet)[0]))

        drifted = dict(self.harnesses)
        drifted[key] = dict(drifted[key], identity='implementer: nobody; reviewer: nobody')
        self.assertTrue(any('writes a different `review.identity`' in problem
                            for problem in review_problems(self.snapshots, drifted, self.reports)[0]))

        awarded = copy_reports(self.reports)
        awarded[key]['claim'] = 'checked'
        self.assertTrue(any('nobody self-awards a level' in problem
                            for problem in review_problems(self.snapshots, self.harnesses, awarded)[0]))

    def test_accept_f14_d_7_the_merge_event_is_not_invented(self):
        """The entry may only name a merge event this file already calls a review event.

        An entry written before its merge event exists can only guess at one, so the
        recorded event has to be one of this file's declared review events, timestamped
        after the review claim it closes and after the implement claim it follows.
        """
        key = 'F14-D.7'
        task = self.non_campaign_by_key()[key]
        merge = task['reviewer']['merge_event']
        self.assertIn(merge['type'], self.non_campaign['review_event_types'])
        self.assertRegex(merge['at'], r'^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ$')
        self.assertGreaterEqual(merge['at'], task['reviewer']['review_claim_started'])
        self.assertLessEqual(task['implementer']['claim_started'], task['reviewer']['review_claim_started'])
        self.assertLessEqual(task['reviewer']['review_claim_started'], merge['at'])

    # -- M16-A-FU6 (#523): harnesses whose identity or task id is filled in at run time ---

    def test_accept_m16_a_fu6_a_json_encoder_reads_as_a_jstr_encoder(self):
        """`json(&reviewer)` is the run-time identity `jstr(&reviewer)` writes.

        The helper only quotes the value, so refusing it for its name turned a
        legitimate run-time harness (F04-D-order-archives) into an unreadable
        one: its shape became `unknown`, the runtime exemption stopped matching
        the derivation from the reports, and five tests that ask for no
        problems at all went red behind it.  The shape must come from the
        argument - here the `env_var` declaration - and never from the name of
        the encoder.
        """
        self.assertEqual(read_harness(JSON_HARNESS),
                         {'X99-A': {'claim': 'implemented', 'identity': None,
                                    'shape': 'runtime',
                                    'via': 'CS_EVIDENCE_REVIEWER'}})

    def test_accept_m16_a_fu6_a_run_time_task_id_resolves_through_its_constant(self):
        """A `task_id` passed as a format argument is read from the constant it names.

        A literal the format string does not spell is still a literal the file
        spells, so the harness can be keyed and cross-checked like any other.
        """
        self.assertEqual(read_harness(RUNTIME_TASK_HARNESS),
                         {'X98-B': {'claim': 'implemented', 'identity': None,
                                    'shape': 'runtime',
                                    'via': 'CS_EVIDENCE_REVIEWER'}})

    def test_accept_m16_a_fu6_one_format_call_over_a_table_writes_one_harness_per_entry(self):
        """A file that writes several reports through one format call cross-checks all of them.

        Each `M01lcReport`-style entry carries its own `task_id` and its own
        `review` identity, so each is a harness of its own; resolving only the
        one the marker happens to sit nearest would leave the other reports
        without the harness that wrote them.
        """
        harnesses = read_harness(TABLE_HARNESS)
        self.assertEqual({key: harness['shape'] for key, harness in harnesses.items()},
                         {'X97-C': 'literal', 'X96-D': 'literal'})
        self.assertEqual(harnesses['X97-C']['identity'],
                         'implementer: x/y; reviewer: a/b, fresh context, not independent')
        self.assertEqual(harnesses['X96-D']['identity'],
                         'implementer: x/y; reviewer: x/y, fresh context, not independent')
        self.assertEqual({harness['claim'] for harness in harnesses.values()}, {'implemented'})

    def test_accept_m16_a_fu6_a_task_id_that_resolves_to_nothing_is_still_reported(self):
        """A run-time `task_id` the reader cannot pin down is a hole, not a silent skip."""
        orphan = RUNTIME_TASK_HARNESS.replace('const TASK: &str = "X98-B";\n\n', '') \
                                     .replace('jstr(TASK)', 'jstr(spec.task_id)')
        keys = list(read_harness(orphan))
        self.assertEqual(len(keys), 1, keys)
        self.assertTrue(keys[0].startswith('unresolved review marker'), keys)
        harnesses = read_harnesses_from({'crates/demo/tests/evidence.rs': orphan})
        self.assertTrue(any('cannot be resolved' in problem
                            and 'crates/demo/tests/evidence.rs' in problem
                            for problem in review_problems(self.snapshots, harnesses,
                                                           self.reports)[0]), harnesses)


def copy_reports(reports, **extra):
    copied = json.loads(json.dumps(reports))
    copied.update(json.loads(json.dumps(extra)))
    return copied


def with_task(snapshot, key, role, value):
    """One snapshot with `key`'s `role` record replaced, the other snapshots dropped.

    Only what `review_problems` reads is passed back, so a mutation of one entry
    cannot be masked by the rest of the file.
    """
    tasks = [{**task, role: value} if task['task_key'] == key else task
             for task in snapshot['tasks']]
    return [{'task': snapshot['task'], 'review_event_types': snapshot['review_event_types'],
             'tasks': tasks}]


def drop_task(snapshots, key):
    """The same snapshots with `key`'s entry removed from whichever one recorded it."""
    return [{'task': snapshot['task'],
             'review_event_types': snapshot['review_event_types'],
             'tasks': [task for task in snapshot['tasks'] if task['task_key'] != key]}
            for snapshot in snapshots]


def report(task_id, identity, claim='implemented'):
    """The shape of a committed report this check reads."""
    return {'task_id': task_id, 'claim': claim,
            'review': {'identity': identity, 'method': 'acceptance suite run locally with the retail '
                                                        'capability by the implementer'}}


if __name__ == '__main__':
    unittest.main()
