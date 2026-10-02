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
family, found while reviewing M16-A, work item M16-A-FU2 / #479) and
`docs/findings/2026-10-02-m16-a-fu4-rally-review-snapshot.json` (the rest,
#483), and this check resolves the reports against both.

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
`reviewer: none yet` *is* a failure.

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
# A `jstr` at the start of an argument, with or without a module path
# (`cs_inspect`'s harnesses call `super::jstr`).
JSTR_CALL = re.compile(r'\A\s*(?:\w+::)*jstr\s*\(')
# The argument of a `jstr(...)`: either a literal, `&review_identity()`, or
# `&review`.
ARGUMENT = re.compile(r'\A\s*&?\s*(\w+)\b')
# `fn review_identity() -> String {` and `let review = env_var("CS_EVIDENCE_REVIEWER");`
FUNCTION = r'fn\s+{}(?:\s*<[^>]*>)?\s*\(\s*\)\s*->\s*String\b'
DECLARATION = r'(?:let\s+{}\b|fn\s+{}\b)[^;{{]{{0,200}}?(?:env::var|env_var)\(\s*"(?P<env>[A-Z0-9_]+)"'

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
    call = JSTR_CALL.match(raw)
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


def read_harness(text):
    """Return the task id, `review.identity` and `claim` of every harness in one file.

    A `"review"` marker the reader cannot resolve to a task id and a `claim` is
    a hole, so it is keyed by its own offset and reported by
    `review_problems` instead of being skipped: a harness that is quietly
    dropped is a harness that silently stops being cross-checked.
    """
    harnesses = {}
    for marker in re.finditer(re.escape(REVIEW_MARKER), text):
        ids = TASK_ID.findall(text[:marker.start()])
        claim = CLAIM.search(text, marker.end())
        if not ids or not claim:
            harnesses[UNRESOLVED.format(marker.start())] = {
                'claim': None, 'identity': None, 'shape': 'unknown',
                'via': 'no task id before the marker' if not ids else 'no claim after the marker'}
            continue
        harnesses[ids[-1]] = {'claim': claim.group(1), **read_identity(text, marker.end())}
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
        cls.snapshots = [cls.snapshot, cls.non_campaign]
        cls.harnesses = read_harnesses()
        cls.reports = read_reports()

    def snapshot_by_key(self):
        return {task['task_key']: task for task in self.snapshot['tasks']}

    def non_campaign_by_key(self):
        return {task['task_key']: task for task in self.non_campaign['tasks']}

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
        self.assertLessEqual(set(self.non_campaign_by_key()), set(self.reports))

    def test_accept_m16_a_fu4_the_reader_covers_the_whole_family(self):
        """The reader must open the real harness files, not a list somebody maintained."""
        expected = {
            'crates/cs_app/tests/campaign/evidence.rs': ('M01-A', 'literal'),
            'crates/cs_app/tests/render/paint.rs': ('F17-C-PAINT', 'literal'),
            'crates/cs_app/tests/text/evidence.rs': ('F51-D', 'function'),
            'crates/cs_app/tests/world/audit/evidence.rs': ('F18-D', 'function'),
            'crates/cs_assets/tests/accept_f04_d_member_collisions.rs': ('T342', 'runtime'),
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
        self.assertEqual({key for key, harness in self.harnesses.items() if harness['shape'] == 'runtime'},
                         {'F11-D', 'F14-D', 'F15-D', 'F56-A', 'T342', 'T345', 'T346'})
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
        """`CS_EVIDENCE_REVIEW` harnesses have no literal; the exemption is deliberate."""
        runtime = {key for key, harness in self.harnesses.items() if harness['shape'] == 'runtime'}
        self.assertEqual(runtime, {'F11-D', 'F14-D', 'F15-D', 'F56-A', 'T342', 'T345', 'T346'})
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


def copy_reports(reports, **extra):
    copied = json.loads(json.dumps(reports))
    copied.update(json.loads(json.dumps(extra)))
    return copied


def report(task_id, identity, claim='implemented'):
    """The shape of a committed report this check reads."""
    return {'task_id': task_id, 'claim': claim,
            'review': {'identity': identity, 'method': 'acceptance suite run locally with the retail '
                                                        'capability by the implementer'}}


if __name__ == '__main__':
    unittest.main()
