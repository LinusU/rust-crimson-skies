"""Synthetic, production-path regressions for Rally AUDIT-EVIDENCE-INTEGRITY (#353)."""
from __future__ import annotations

import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

VALIDATOR = Path(__file__).resolve().parents[1] / 'validate_evidence.py'
spec = importlib.util.spec_from_file_location('validate_evidence', VALIDATOR)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class EvidenceIntegrityTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        payload = b'synthetic assertion output\n'
        (self.root / 'result.log').write_bytes(payload)
        self.doc = {
            'schema_version': 1, 'task_id': 'AUDIT-EVIDENCE-INTEGRITY',
            'candidate_tree': 'a' * 40,
            'engine': {'rust': 'synthetic-test', 'bevy': 'synthetic-test', 'avian': 'synthetic-test'},
            'created_at': '2026-09-28T20:00:00Z',
            'command': {'argv': ['synthetic-probe'], 'cwd': '.', 'exit_code': 0},
            'source': {'install_sha256': None, 'content_sha256': None},
            'seed': 0, 'ticks': {'start': 0, 'end': 1}, 'overrides': [],
            'capabilities': ['synthetic'],
            'tests': {'discovered': 1, 'executed': 1, 'passed': 1, 'failed': 0, 'ignored': 0},
            'assertions': [{'id': 'AC01', 'status': 'pass', 'evidence': ['result.log']}],
            'artifacts': [{'path': 'result.log', 'sha256': hashlib.sha256(payload).hexdigest(), 'kind': 'test-log'}],
            'unknowns': [], 'review': {'identity': 'synthetic-test', 'method': 'unit test'},
            'claim': 'implemented',
        }

    def check(self, doc=None, require_pass=True):
        return module.validate(self.doc if doc is None else doc, self.root, require_pass)

    def reject(self, doc):
        with self.assertRaises(ValueError):
            self.check(doc)

    def cli(self, doc):
        report = self.root / 'report.json'
        report.write_text(json.dumps(doc))
        return subprocess.run(
            [sys.executable, str(VALIDATOR), str(report), '--artifact-root', str(self.root), '--require-pass'],
            capture_output=True, text=True, check=False,
        )

    def test_accept_audit_evidence_integrity_positive_and_cli(self):
        self.assertEqual(self.check(), {
            'structurally_valid': True, 'artifact_count': 1, 'claims_semantically_verified': False,
        })
        result = self.cli(self.doc)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(json.loads(result.stdout)['claims_semantically_verified'])

    def test_accept_audit_evidence_integrity_dangling_reference(self):
        self.doc['assertions'][0]['evidence'] = ['missing.log']
        self.doc['artifacts'] = []
        self.reject(self.doc)
        with self.assertRaises(ValueError):
            self.check(require_pass=False)

    def test_accept_audit_evidence_integrity_reference_to_undeclared_existing_file(self):
        (self.root / 'other.log').write_text('exists but is not declared or hashed')
        self.doc['assertions'][0]['evidence'] = ['other.log']
        self.reject(self.doc)

    def test_accept_audit_evidence_integrity_unaccounted_tests(self):
        self.doc['tests']['discovered'] = 10
        self.reject(self.doc)

    def test_accept_audit_evidence_integrity_ignored_tests_not_a_pass(self):
        self.doc['tests'].update(discovered=2, ignored=1)
        self.assertTrue(self.check(require_pass=False)['structurally_valid'])
        self.reject(self.doc)

    def test_accept_audit_evidence_integrity_empty_failed_or_contradictory_counts(self):
        cases = [
            {'discovered': 0, 'executed': 0, 'passed': 0, 'failed': 0, 'ignored': 0},
            {'discovered': 2, 'executed': 2, 'passed': 1, 'failed': 1, 'ignored': 0},
            {'discovered': 2, 'executed': 1, 'passed': 2, 'failed': 0, 'ignored': 1},
        ]
        for counts in cases:
            with self.subTest(counts=counts):
                doc = copy.deepcopy(self.doc)
                doc['tests'] = counts
                self.reject(doc)

    def test_accept_audit_evidence_integrity_duplicate_assertions(self):
        self.doc['assertions'].append(copy.deepcopy(self.doc['assertions'][0]))
        self.reject(self.doc)

    def test_accept_audit_evidence_integrity_malformed_shapes(self):
        for field in ('engine', 'command', 'source', 'ticks', 'tests', 'review'):
            for value in (None, [], 'not an object'):
                with self.subTest(field=field, value=value):
                    doc = copy.deepcopy(self.doc)
                    doc[field] = value
                    self.reject(doc)
        for field, value in (
            ('assertions', [None]), ('artifacts', [None]), ('capabilities', [[]]),
            ('claim', {}), ('unknowns', {}), ('created_at', []), ('schema_version', True),
        ):
            with self.subTest(field=field):
                doc = copy.deepcopy(self.doc)
                doc[field] = value
                self.reject(doc)
        self.reject([])

    def test_accept_audit_evidence_integrity_malformed_assertions(self):
        for field, values in (
            ('id', ['', ' ', None, []]),
            ('evidence', [None, 'result.log', [None], [42], [{}]]),
            ('status', [None, [], 'success']),
        ):
            for value in values:
                with self.subTest(field=field, value=value):
                    doc = copy.deepcopy(self.doc)
                    doc['assertions'][0][field] = value
                    self.reject(doc)

    def test_accept_audit_evidence_integrity_noncanonical_paths(self):
        for path in ('', '.', '..', '../result.log', '/result.log', 'C:/result.log',
                     'C:result.log', '\\\\server\\share\\result.log', 'a\\result.log',
                     './result.log', 'a/../result.log', 'a//result.log', 'result.log/', 'a\0b'):
            for location in ('declaration', 'reference'):
                with self.subTest(path=path, location=location):
                    doc = copy.deepcopy(self.doc)
                    if location == 'declaration':
                        doc['artifacts'][0]['path'] = path
                    else:
                        doc['assertions'][0]['evidence'] = [path]
                    self.reject(doc)

    def test_accept_audit_evidence_integrity_missing_corrupt_duplicate_artifact(self):
        doc = copy.deepcopy(self.doc)
        doc['artifacts'].append(copy.deepcopy(doc['artifacts'][0]))
        self.reject(doc)
        (self.root / 'result.log').write_text('changed')
        self.reject(self.doc)
        (self.root / 'result.log').unlink()
        self.reject(self.doc)

    def test_accept_audit_evidence_integrity_nested_artifact(self):
        (self.root / 'nested').mkdir()
        (self.root / 'result.log').rename(self.root / 'nested' / 'result.log')
        self.doc['artifacts'][0]['path'] = 'nested/result.log'
        self.doc['assertions'][0]['evidence'] = ['nested/result.log']
        self.assertTrue(self.check()['structurally_valid'])

    @unittest.skipIf(os.name == 'nt', 'Symlink creation requires host privileges; exercised on Linux CI')
    def test_accept_audit_evidence_integrity_symlink_file_and_directory(self):
        (self.root / 'link.log').symlink_to(self.root / 'result.log')
        doc = copy.deepcopy(self.doc)
        doc['artifacts'][0]['path'] = 'link.log'
        doc['assertions'][0]['evidence'] = ['link.log']
        self.reject(doc)
        (self.root / 'real').mkdir()
        (self.root / 'result.log').rename(self.root / 'real' / 'result.log')
        (self.root / 'alias').symlink_to(self.root / 'real', target_is_directory=True)
        self.doc['artifacts'][0]['path'] = 'alias/result.log'
        self.doc['assertions'][0]['evidence'] = ['alias/result.log']
        self.reject(self.doc)

    def test_accept_audit_evidence_integrity_claim_gates_preserved(self):
        for change in (
            {'claim': 'verified_original'}, {'claim': 'release_approved'},
            {'unknowns': ['product behavior still unknown']},
        ):
            with self.subTest(change=change):
                doc = copy.deepcopy(self.doc)
                doc.update(change)
                self.reject(doc)
        self.doc.update(claim='release_approved', capabilities=['retail'])
        self.doc['source'] = {'install_sha256': 'a' * 64, 'content_sha256': 'b' * 64}
        self.reject(self.doc)
        self.doc['capabilities'].append('human_review')
        self.assertFalse(self.check()['claims_semantically_verified'])

    def test_accept_audit_evidence_integrity_incomplete_assertions_or_command(self):
        for change in ({'assertions': []}, {'command': {'argv': ['probe'], 'cwd': '.', 'exit_code': 3}}):
            doc = copy.deepcopy(self.doc)
            doc.update(change)
            self.reject(doc)
        for status in ('fail', 'unknown'):
            doc = copy.deepcopy(self.doc)
            doc['assertions'][0]['status'] = status
            self.reject(doc)
        self.doc['assertions'][0]['evidence'] = []
        self.reject(self.doc)

    def test_accept_audit_evidence_integrity_cli_invalid_reports_exit_three(self):
        for doc in ([], dict(self.doc, engine=None), dict(self.doc, artifacts=[])):
            with self.subTest(doc=doc):
                result = self.cli(doc)
                self.assertEqual(result.returncode, 3, result.stderr)
                self.assertIn('Invalid evidence:', result.stderr)
                self.assertNotIn('Traceback', result.stderr)


if __name__ == '__main__':
    unittest.main()
