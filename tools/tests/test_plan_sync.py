"""Plan-consistency regressions for Rally AUDIT-PLAN-SYNC (#356).

The feature sheets, mission sheets and the owner-ruling section of
specs/README.md are checked against the Rally dependency snapshot committed in
docs/findings/. Run with:

    python3 -m unittest discover -s tools/tests -p 'test_plan_sync.py' -v
"""
from __future__ import annotations

import copy
import json
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]
SNAPSHOT = ROOT / 'docs/findings/2026-09-28-audit-plan-sync-rally-snapshot.json'
RULING_HEADING = '## Owner ruling 2026-09-28: first-playable sequencing (AUDIT-PLAN-SYNC, #356)'

STAGE = re.compile(r'^### (F\d\d-[A-D]):.*?^Dependencies: (.*?)\. Required capabilities: (.*?)\.$', re.M | re.S)
MISSION_STAGE = re.compile(
    r'^\*\*(M\d\d-[A-C]) - .*?\*\*\n\nDependencies: (.*?)\. Test prefix: .*?Required capabilities: (.*?)\.', re.M)
RULING_ROW = re.compile(r'^\| (\S+) \| #(\d+) \| ((?:#\d+(?:, )?)+) \| .* \|$', re.M)
NON_SHEET_TASK = re.compile(r'\*\*(\S+) \(#(\d+)\)\*\*, depends on (.*?): ')

# Stages whose sheet dependencies the owner resequenced. The removed entries
# must still be required before the full-campaign or full-release stages.
REMOVED_FROM_F50_A = [f'F{n}-A' for n in (18, 19, 20, 24, 25, 27, 28, 29, 32, 33, 34, 35, 36,
                                            38, 39, 40, 41, 42, 43, 44, 45, 46, 47)]
REMOVED_FROM_F63_A = [f'F{n}-A' for n in (26, 47, 49, 52, 53, 56, 58, 60, 61, 62)]
RELEASE_VERIFICATION = [333, 339, 341, 342, 351, 352, 353, 354, 355, 356, 358, 360, 361]
HUMAN_GATED = {
    'M01-C': {'retail', 'gpu', 'audio', 'human_play'},
    'F50-D': {'retail', 'gpu', 'audio', 'human_play'},
    'F63-C': {'retail', 'gpu', 'audio', 'network_real', 'human_play', 'human_review'},
    'F63-D': {'retail', 'gpu', 'audio', 'network_real', 'human_play', 'human_review'},
}


def load_snapshot(path=SNAPSHOT):
    tasks = json.loads(path.read_text())['tasks']
    return {t['id']: t for t in tasks}


def read_plan(root=ROOT):
    """Return the texts the checks read, keyed by repository-relative path."""
    names = sorted(root.glob('specs/*.md')) + sorted(root.glob('missions/M*.md'))
    return {str(p.relative_to(root)): p.read_text() for p in names}


def split_list(text):
    return [part.strip() for part in re.split(r',| and ', text) if part.strip()]


def github_slug(heading):
    title = heading.lstrip('#').strip().lower()
    return re.sub(r'[^a-z0-9 -]', '', title).replace(' ', '-')


class Plan:
    """Parsed sheet stages and owner ruling, resolved against a snapshot."""

    def __init__(self, texts, tasks):
        self.texts = texts
        self.tasks = tasks
        self.by_key = {t['key']: t for t in tasks.values() if t['key']}
        self.problems = []
        self.stages = {}
        for path, text in texts.items():
            pattern = MISSION_STAGE if path.startswith('missions/') else STAGE
            for key, deps, caps in pattern.findall(text):
                if key in self.stages:
                    self.problems.append(f'{key}: stage defined twice')
                names = [] if deps.strip().lower() == 'none' else split_list(deps)
                self.stages[key] = {
                    'deps': {self.resolve(name, key) for name in names},
                    'caps': set(split_list(caps)),
                    'path': path,
                }
        readme = texts.get('specs/README.md', '')
        ruling = readme.split(RULING_HEADING, 1)[1] if RULING_HEADING in readme else ''
        if not ruling:
            self.problems.append('specs/README.md has no owner-ruling section')
        self.ruling = ruling
        self.additions = {}
        for key, ident, deps in RULING_ROW.findall(ruling):
            if key in self.additions:
                self.problems.append(f'{key}: ruling row repeated')
            if self.by_key.get(key, {}).get('id') != int(ident):
                self.problems.append(f'{key}: ruling names #{ident}, Rally has another id')
            self.additions[key] = {int(n) for n in re.findall(r'#(\d+)', deps)}
        self.non_sheet = {}
        for key, ident, deps in NON_SHEET_TASK.findall(ruling):
            if self.by_key.get(key, {}).get('id') != int(ident):
                self.problems.append(f'{key}: ruling names #{ident}, Rally has another id')
            names = [] if deps == 'no task' else split_list(deps)
            self.non_sheet[int(ident)] = {self.resolve(name, key) for name in names}

    def resolve(self, name, owner):
        match = re.fullmatch(r'#(\d+)', name)
        if match and int(match.group(1)) in self.tasks:
            return int(match.group(1))
        if name in self.by_key:
            return self.by_key[name]['id']
        self.problems.append(f'{owner}: unknown dependency {name!r}')
        return None

    def rally_mismatches(self):
        problems = []
        for key, stage in sorted(self.stages.items()):
            task = self.by_key.get(key)
            if task is None:
                problems.append(f'{key}: no Rally task')
                continue
            rally = set(task['depends_on'])
            extra = self.additions.get(key, set())
            if extra & stage['deps']:
                problems.append(f'{key}: ruling repeats sheet dependencies {sorted(extra & stage["deps"])}')
            split_children = {t['id'] for t in self.tasks.values()
                              if t['key'] and t['key'].startswith(key + '.')}
            expected = stage['deps'] | extra
            if expected != rally - split_children:
                problems.append(f'{key} (#{task["id"]}): sheet+ruling {sorted(expected)} '
                                f'!= Rally {sorted(rally)}')
        for key in self.additions:
            if key not in self.stages:
                problems.append(f'{key}: ruling row without a sheet stage')
        for ident, deps in sorted(self.non_sheet.items()):
            if deps != set(self.tasks[ident]['depends_on']):
                problems.append(f'#{ident}: ruling lists {sorted(deps)}, '
                                f'Rally has {sorted(self.tasks[ident]["depends_on"])}')
        return self.problems + problems

    def closure(self, ident):
        seen, todo = set(), [ident]
        while todo:
            for dep in self.tasks[todo.pop()]['depends_on']:
                if dep not in seen:
                    seen.add(dep)
                    todo.append(dep)
        return seen

    def cycles(self):
        state, found = {}, []

        def visit(ident, stack):
            state[ident] = 'open'
            for dep in self.tasks[ident]['depends_on']:
                if state.get(dep) == 'open':
                    found.append(stack[stack.index(dep):] + [dep] if dep in stack else [ident, dep])
                elif dep not in state:
                    visit(dep, stack + [dep])
            state[ident] = 'done'

        for ident in sorted(self.tasks):
            if ident not in state:
                visit(ident, [ident])
        return found

    def id(self, key):
        return self.by_key[key]['id']


class PlanSyncTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tasks = load_snapshot()
        cls.texts = read_plan()
        cls.plan = Plan(cls.texts, cls.tasks)

    def test_accept_audit_plan_sync_snapshot_is_complete(self):
        ids = set(self.tasks)
        self.assertEqual(ids, set(range(1, max(ids) + 1)), 'snapshot must list every Rally task')
        for task in self.tasks.values():
            self.assertLessEqual(set(task['depends_on']), ids, task)
        self.assertEqual(self.plan.cycles(), [])

    def test_accept_audit_plan_sync_sheets_and_ruling_match_rally(self):
        self.assertGreaterEqual(len(self.plan.stages), 65 * 4 + 24 * 3)
        self.assertEqual(self.plan.rally_mismatches(), [])
        for key in ('F50-A', 'F63-A', 'M01-B', 'F63-C', 'F24-D', 'F17-D', 'F18-B'):
            self.assertIn(key, self.plan.additions)
        for ident in (353, 354, 355, 357, 358, 359, 360, 361):
            self.assertIn(ident, self.plan.non_sheet)

    def test_accept_audit_plan_sync_ruling_links_resolve(self):
        slug = github_slug(RULING_HEADING)
        links = [(path, anchor) for path, text in self.texts.items()
                 for anchor in re.findall(r'README\.md#([a-z0-9-]+)', text)]
        self.assertGreaterEqual(len(links), 10)
        for path, anchor in links:
            self.assertEqual(anchor, slug, path)
        for path in ('AGENTS.md', 'docs/00-SCOPE.md', 'docs/TASK-SPLITTING.md'):
            self.assertIn('specs/README.md' if path != 'AGENTS.md' else 'allowProtectedChanges',
                          (ROOT / path).read_text(), path)

    def test_accept_audit_plan_sync_first_mission_path_skips_full_campaign(self):
        plan = self.plan
        m01c = plan.id('M01-C')
        path = [plan.id(k) for k in ('M01-A', 'VS-M01-RUNTIME', 'M01-B', 'VS-M01-CONTROLLED-RUNS')]
        closure = plan.closure(m01c)
        for earlier, later in zip(path, path[1:] + [m01c]):
            self.assertIn(earlier, plan.closure(later))
            self.assertNotIn(later, plan.closure(earlier))
        self.assertIn(plan.id('REF-OWNER-FIRST-CAPTURE'), closure)
        for key in ('F50-B', 'F50-C', 'F50-D', 'F63-B', 'F63-C', 'F63-D'):
            self.assertNotIn(plan.id(key), closure, key)
        self.assertNotIn(plan.id('F50-C'), plan.stages['M01-B']['deps'])
        self.assertIn(plan.id('VS-M01-RUNTIME'), plan.stages['M01-B']['deps'])
        self.assertIn(plan.id('VS-M01-CONTROLLED-RUNS'), plan.stages['M01-C']['deps'])

    def test_accept_audit_plan_sync_full_release_requirements_remain(self):
        plan = self.plan
        f50b, f63b = plan.closure(plan.id('F50-B')), plan.closure(plan.id('F63-B'))
        for key in REMOVED_FROM_F50_A:
            self.assertIn(plan.id(key), f50b, key)
        for key in REMOVED_FROM_F63_A:
            self.assertIn(plan.id(key), f63b, key)
        f50c = plan.id('F50-C')
        for n in range(2, 25):
            self.assertIn(f50c, plan.stages[f'M{n:02d}-B']['deps'])
        f50d = plan.stages['F50-D']['deps']
        f63d = plan.stages['F63-D']['deps']
        for n in range(1, 25):
            self.assertIn(plan.id(f'M{n:02d}-C'), f50d)
            self.assertIn(plan.id(f'M{n:02d}-C'), f63d)
        self.assertIn(f50c, f50d)
        for key in [f'F{n:02d}-D' for n in range(65) if n != 63] + ['F63-C']:
            self.assertIn(plan.id(key), f63d, key)
        self.assertIn(plan.id('F50-D'), set(plan.tasks[plan.id('F63-D')]['depends_on']))
        self.assertLessEqual(set(RELEASE_VERIFICATION), set(plan.tasks[plan.id('F63-C')]['depends_on']))
        self.assertLessEqual(set(RELEASE_VERIFICATION), plan.closure(plan.id('F63-D')))
        for key, caps in HUMAN_GATED.items():
            self.assertEqual(plan.stages[key]['caps'], caps, key)

    def test_accept_audit_plan_sync_detects_drift(self):
        def mutated(path, old, new):
            texts = dict(self.texts)
            self.assertIn(old, texts[path])
            texts[path] = texts[path].replace(old, new, 1)
            return Plan(texts, self.tasks).rally_mismatches()

        readme = 'specs/README.md'
        self.assertTrue(mutated(readme, '| F18-B | #86 | #333, #356 |', '| F18-B | #86 | #356 |'))
        self.assertTrue(mutated('missions/M01.md', 'Dependencies: M01-A, F38-C, VS-M01-RUNTIME.',
                                'Dependencies: M01-A, F50-C, F38-C.'))
        f50 = next(p for p in self.texts if p.startswith('specs/F50-'))
        self.assertTrue(mutated(f50, 'Dependencies: F14-A, F13-A, F16-A.',
                                'Dependencies: ' + ', '.join(REMOVED_FROM_F50_A) + '.'))
        self.assertTrue(mutated(readme, 'depends on F59-B, M01-B, #358 and #359:',
                                'depends on F59-B, M01-B and #359:'))
        tasks = copy.deepcopy(self.tasks)
        tasks[self.plan.id('M01-B')]['depends_on'].append(self.plan.id('F50-C'))
        self.assertTrue(Plan(self.texts, tasks).rally_mismatches())


if __name__ == '__main__':
    unittest.main()
