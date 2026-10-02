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
same commit; the coverage test below fails until it does.

Run with:

    python3 -m unittest discover -s tools/tests -p 'test_evidence_review_identity.py' -v
"""
from __future__ import annotations

import json
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]
SNAPSHOT = ROOT / 'docs/findings/2026-10-02-m16-a-fu2-rally-review-snapshot.json'
HARNESS = ROOT / 'crates/cs_app/tests/campaign/evidence.rs'
REPORTS = ROOT / 'docs/findings/evidence'

# The placeholder wording the reports used while they claimed that no review had
# happened.  None of it survives once the real reviewer is named.
PLACEHOLDERS = ('none yet', 'not yet assigned', 'not yet known', 'not yet independent',
                'to be recorded', 'self-check', 'the rally reviewer regenerates')

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
    source = text
    harnesses = {}
    starts = [match.start() for match in HARNESS_SPLIT.finditer(source)]
    for index, start in enumerate(starts):
        chunk = source[start:starts[index + 1] if index + 1 < len(starts) else len(source)]
        task_id = TASK_ID.search(chunk)
        literal = IDENTITY.search(chunk, chunk.index(REVIEW_MARKER))
        identity = read_rust_string(chunk, literal.end() - 1)
        harnesses[task_id.group(1)] = {'identity': identity,
                                       'claim': CLAIM.search(chunk).group(1)}
    return harnesses


def read_snapshot(path=SNAPSHOT):
    return json.loads(Path(path).read_text())


def read_reports(directory=REPORTS):
    """Return every committed report keyed by its `task_id`."""
    return {report['task_id']: report
            for report in (json.loads(path.read_text())
                           for path in sorted(Path(directory).glob('*.json')))}


def review_problems(snapshot, harnesses, reports):
    """Everything wrong with the campaign-binding reports' reviewer records."""
    problems = []
    tasks = snapshot['tasks']
    if sorted(task['task_key'] for task in tasks) != [task['task_key'] for task in tasks]:
        problems.append('snapshot tasks are not in Rally task order')
    for task in tasks:
        key = task['task_key']
        if not task['implementer']['actor'] or not task['reviewer']['actor']:
            problems.append(f'{key}: the snapshot records no implementer or no reviewer')
            continue
        if task['reviewer']['merge_event']['type'] not in snapshot['review_event_types']:
            problems.append(f'{key}: the reviewer is closed by {task["reviewer"]["merge_event"]["type"]}'
                            f', which is not a review event')
        report = reports.get(key)
        if report is None:
            problems.append(f'{key}: no committed report at {task["evidence_report"]}')
            continue
        if Path(task['evidence_report']).name != f'{key}.json':
            problems.append(f'{key}: the snapshot points at {task["evidence_report"]}')
        harness = harnesses.get(key)
        if harness is None:
            problems.append(f'{key}: no evidence harness writes this report')
            continue
        identity = report['review']['identity']
        where = task['evidence_report']
        if not report['review']['method'].strip():
            problems.append(f'{where}: `review.method` is empty')
        for placeholder in PLACEHOLDERS:
            if placeholder in identity.lower():
                problems.append(f'{where}: `review.identity` still says {placeholder!r} while Rally'
                                f' records a review claim for {key} by {task["reviewer"]["actor"]}')
        for role in ('implementer', 'reviewer'):
            actor = task[role]['actor']
            if actor not in identity:
                problems.append(f'{where}: `review.identity` does not name the {role} {actor!r}')
        if task['implementer']['actor'] == task['reviewer']['actor'] \
                and 'not independent' not in identity.lower():
            problems.append(f'{where}: the reviewer is the implementer\'s own instance'
                            f' ({task["reviewer"]["actor"]}) but `review.identity` does not say the'
                            f' review is not independent')
        if 'context' not in identity.lower():
            problems.append(f'{where}: `review.identity` says nothing about the reviewer\'s context')
        if harness['identity'] != identity:
            problems.append(f'crates/cs_app/tests/campaign/evidence.rs: the {key} harness writes a'
                            f' different `review.identity` than {where}, so regenerating the report'
                            f' would undo the recorded reviewer')
        for report_claim, source in ((report['claim'], where), (harness['claim'], 'the harness')):
            if report_claim != 'implemented':
                problems.append(f'{source}: claim {report_claim!r}; a merge awards `checked` only and'
                                f' nobody self-awards a level')
    return problems


class EvidenceReviewIdentityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.snapshot = read_snapshot()
        cls.harnesses = read_harness()
        cls.reports = read_reports()

    def test_accept_m16_a_fu2_snapshot_covers_every_campaign_binding_stage(self):
        self.assertEqual(sorted(self.harnesses), sorted(t['task_key'] for t in self.snapshot['tasks']))
        self.assertEqual(len(self.snapshot['tasks']), 9)
        for task in self.snapshot['tasks']:
            self.assertIn(task['task_key'], self.reports, task)
            report = self.reports[task['task_key']]
            self.assertEqual(report['task_id'], task['task_key'])
            self.assertRegex(task['reviewer']['review_claim_started'], r'^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ$')
            self.assertLessEqual(task['implementer']['claim_started'], task['reviewer']['review_claim_started'])

    def test_accept_m16_a_fu2_reports_name_the_actual_implementer_and_reviewer(self):
        self.assertEqual(review_problems(self.snapshot, self.harnesses, self.reports), [])

    def test_accept_m16_a_fu2_detects_drift(self):
        """A reviewer, an implementer or a harness that stops naming the real agents must fail."""
        key = 'M13-A'
        reviewed = next(t for t in self.snapshot['tasks'] if t['task_key'] == key)

        reverted = json.loads(json.dumps(self.reports))
        reverted[key]['review']['identity'] = reverted[key]['review']['identity'].replace(
            reviewed['reviewer']['actor'], 'none yet')
        self.assertTrue(review_problems(self.snapshot, self.harnesses, reverted))

        renamed = json.loads(json.dumps(self.reports))
        renamed[key]['review']['identity'] = renamed[key]['review']['identity'].replace(
            reviewed['implementer']['actor'], 'claude-9')
        self.assertTrue(review_problems(self.snapshot, self.harnesses, renamed))

        silent = json.loads(json.dumps(self.reports))
        silent[key]['review']['identity'] = re.sub(r'(?i)not independent', 'independent',
                                                   silent[key]['review']['identity'])
        self.assertTrue(review_problems(self.snapshot, self.harnesses, silent))

        drifted = dict(self.harnesses)
        drifted[key] = dict(drifted[key], identity='implementer: nobody; reviewer: nobody')
        self.assertTrue(review_problems(self.snapshot, drifted, self.reports))

        awarded = json.loads(json.dumps(self.reports))
        awarded[key]['claim'] = 'checked'
        self.assertTrue(review_problems(self.snapshot, self.harnesses, awarded))

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


if __name__ == '__main__':
    unittest.main()
