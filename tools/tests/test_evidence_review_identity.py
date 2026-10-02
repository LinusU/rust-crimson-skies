"""The campaign-binding evidence reports must name the implementer and the reviewer.

`docs/contracts/CLI-EVIDENCE.md` puts "reviewer identity/method" in the evidence
record minimum, and the owner ruling of 2026-09-28 requires that the actual
implementer/reviewer identities and whether the reviewer's context was fresh are
always recorded.  Every campaign-binding report written by
`crates/cs_app/tests/campaign/evidence.rs` must therefore name the agents that
really ran, and the report text and the harness that writes it must agree, so a
regeneration cannot silently put a placeholder back.

Rally itself is not reachable from an offline check, so the review facts read
out of the Rally activity log are committed in
`docs/findings/2026-10-02-m16-a-fu2-rally-review-snapshot.json` and this check
resolves the reports against that snapshot.  Found while reviewing M16-A (#303);
work item M16-A-FU2 (#479).

A new campaign-binding stage adds a harness and a report, so the task that adds
it extends the snapshot with that stage's Rally implementer and reviewer in the
same commit.  That is an advisory here, not a failure: a correct new report must
not turn an unrelated branch red while the campaign keeps landing stages.  A new
report that still says `reviewer: none yet` *is* a failure, because Rally only
merges a stage through `complete_review` or its landing queue and a committed
report has therefore always had a reviewer.

Run with:

    python3 -m unittest discover -s tools/tests -p 'test_evidence_review_identity.py' -v
"""
from __future__ import annotations

import json
from pathlib import Path
import re
import sys
import unittest

ROOT = Path(__file__).resolve().parents[2]
SNAPSHOT = ROOT / 'docs/findings/2026-10-02-m16-a-fu2-rally-review-snapshot.json'
HARNESS = ROOT / 'crates/cs_app/tests/campaign/evidence.rs'
REPORTS = ROOT / 'docs/findings/evidence'

# The placeholder wording the reports used while they claimed that no review had
# happened.  None of it survives once the real reviewer is named.  The last two
# are the wording M19-A, M21-A and M24-A shipped with; a stage that keeps
# "an independent review is pending" but drops the "none yet" must fail too.
PLACEHOLDERS = ('none yet', 'not yet assigned', 'not yet known', 'not yet independent',
                'to be recorded', 'self-check', 'the rally reviewer regenerates',
                'independent review is pending')

TASK_ID = re.compile(r'\\"task_id\\": \\"([^"\\]+)\\"')
CLAIM = re.compile(r'\\"claim\\": \\"([a-z_]+)\\"')
REVIEW_MARKER = r'\"review\": {{\"identity\": {}, \"method\": {}}}'
# The identity and the method are the only `jstr` arguments in these reports
# that are literals; every earlier one is `jstr(&some_variable)`.
IDENTITY = re.compile(r'jstr\(\s*"')
HARNESS_SPLIT = re.compile(r'^fn evidence_report_', re.M)


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


def read_harness(text=HARNESS.read_text()):
    """Return the task id, `review.identity` and `claim` literal of every harness.

    One entry per `evidence_report_*` test in `crates/cs_app/tests/campaign/evidence.rs`.
    """
    harnesses = {}
    starts = [match.start() for match in HARNESS_SPLIT.finditer(text)]
    for index, start in enumerate(starts):
        chunk = text[start:starts[index + 1] if index + 1 < len(starts) else len(text)]
        task_id = TASK_ID.search(chunk)
        literal = IDENTITY.search(chunk, chunk.index(REVIEW_MARKER))
        harnesses[task_id.group(1)] = {'identity': read_rust_string(chunk, literal.end() - 1),
                                       'claim': CLAIM.search(chunk).group(1)}
    return harnesses


def read_snapshot(path=SNAPSHOT):
    return json.loads(Path(path).read_text())


def read_reports(directory=REPORTS):
    """Return every committed report keyed by its `task_id`."""
    return {report['task_id']: report
            for report in (json.loads(path.read_text())
                           for path in sorted(Path(directory).glob('*.json')))}


def claim_actors(task, role):
    """Every agent that held a claim of `role` on this task, latest first.

    A stage can be reviewed more than once (M07-A was approved twice before the
    lander could apply it), so an earlier review claim is still a claim the
    report has to account for.
    """
    if role == 'implementer':
        return [task['implementer']['actor']]
    return [task['reviewer']['actor']] + [claim['actor']
                                          for claim in task['reviewer'].get('earlier_review_claims', [])]


def review_problems(snapshot, harnesses, reports):
    """Return `(problems, unrecorded)` for the campaign-binding reports.

    `problems` is what must fail: a placeholder where a reviewer belongs, a
    report that disagrees with the harness that writes it, a claim above
    `implemented`, or a snapshot-backed stage whose identity misses an agent the
    Rally log recorded.  `unrecorded` names stages that have a harness but no
    snapshot entry yet; they are an advisory, because the campaign lands stages
    continuously and a correct new report must not fail an unrelated branch.
    """
    problems, unrecorded = [], []
    tasks = snapshot['tasks']
    if sorted(task['task_key'] for task in tasks) != [task['task_key'] for task in tasks]:
        problems.append('snapshot tasks are not in Rally task order')
    known = {task['task_key']: task for task in tasks}

    for key in sorted(harnesses):
        harness = harnesses[key]
        where = f'docs/findings/evidence/{key}.json'
        report = reports.get(key)
        if report is None:
            problems.append(f'{where}: no committed report, but {key} has an evidence harness')
            continue
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
        if 'context' not in identity.lower():
            problems.append(f'{where}: `review.identity` says nothing about the reviewer\'s context')
        if harness['identity'] != identity:
            problems.append(f'crates/cs_app/tests/campaign/evidence.rs: the {key} harness writes a'
                            f' different `review.identity` than {where}, so regenerating the report'
                            f' would undo the recorded reviewer')
        for claim, source in ((report['claim'], where), (harness['claim'], 'the harness')):
            if claim != 'implemented':
                problems.append(f'{source}: claim {claim!r}; a merge awards `checked` only and'
                                f' nobody self-awards a level')
        if task is None:
            unrecorded.append(key)
            continue
        if not task['implementer']['actor'] or not task['reviewer']['actor']:
            problems.append(f'{key}: the snapshot records no implementer or no reviewer')
            continue
        if task['reviewer']['merge_event']['type'] not in snapshot['review_event_types']:
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
    return problems, unrecorded


class EvidenceReviewIdentityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.snapshot = read_snapshot()
        cls.harnesses = read_harness()
        cls.reports = read_reports()

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
        problems, unrecorded = review_problems(self.snapshot, self.harnesses, self.reports)
        self.assertEqual(problems, [])
        for key in unrecorded:
            print(f'note: {key} has no Rally review facts in {SNAPSHOT.name} yet', file=sys.stderr)

    def test_accept_m16_a_fu2_detects_drift(self):
        """A reviewer, an implementer or a harness that stops naming the real agents must fail."""
        key = 'M13-A'
        recorded = self.snapshot_by_key()[key]

        reverted = copy_reports(self.reports)
        reverted[key]['review']['identity'] = reverted[key]['review']['identity'].replace(
            recorded['reviewer']['actor'], 'none yet')
        self.assertTrue(review_problems(self.snapshot, self.harnesses, reverted)[0])

        renamed = copy_reports(self.reports)
        renamed[key]['review']['identity'] = renamed[key]['review']['identity'].replace(
            recorded['implementer']['actor'], 'claude-9')
        self.assertTrue(review_problems(self.snapshot, self.harnesses, renamed)[0])

        silent = copy_reports(self.reports)
        silent[key]['review']['identity'] = re.sub(r'(?i)not independent', 'independent',
                                                   silent[key]['review']['identity'])
        self.assertTrue(review_problems(self.snapshot, self.harnesses, silent)[0])

        drifted = dict(self.harnesses)
        drifted[key] = dict(drifted[key], identity='implementer: nobody; reviewer: nobody')
        self.assertTrue(review_problems(self.snapshot, drifted, self.reports)[0])

        awarded = copy_reports(self.reports)
        awarded[key]['claim'] = 'checked'
        self.assertTrue(review_problems(self.snapshot, self.harnesses, awarded)[0])

    def test_accept_m16_a_fu2_a_new_stage_is_an_advisory_until_it_gaps(self):
        """A stage the snapshot has not caught up with must not fail, unless it really gaps."""
        key = 'M99-A'
        identity = ('implementer: x/y (Rally #999); reviewer: a/b, a different agent instance with '
                    'fresh context, so not independent original-reference evidence')
        stage = self.harnesses['M13-A']
        harnesses = dict(self.harnesses, **{key: dict(stage, identity=identity)})
        clean = copy_reports(self.reports, **{key: report(key, identity)})
        problems, unrecorded = review_problems(self.snapshot, harnesses, clean)
        self.assertEqual(problems, [])
        self.assertIn(key, unrecorded)
        gapped = copy_reports(self.reports, **{key: report(key, 'implementer: x/y; reviewer: none yet')})
        problems, unrecorded = review_problems(self.snapshot, harnesses, gapped)
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
        problems, unrecorded = review_problems(self.snapshot, harnesses,
                                               copy_reports(self.reports, **{key: report(key, gapped)}))
        self.assertIn(key, unrecorded)
        self.assertTrue(any(f'docs/findings/evidence/{key}.json: `review.identity` still says '
                            "'independent review is pending'" in problem for problem in problems),
                        problems)

    def test_accept_m16_a_fu2_harness_reader_joins_rust_line_continuations(self):
        chunk = '''fn evidence_report_x99_a() {
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
    );
}
'''
        self.assertEqual(read_harness(chunk),
                         {'X99-A': {'identity': 'one two', 'claim': 'implemented'}})

    def snapshot_by_key(self):
        return {task['task_key']: task for task in self.snapshot['tasks']}


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
