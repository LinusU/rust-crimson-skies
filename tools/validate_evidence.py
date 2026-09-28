#!/usr/bin/env python3
"""Check report consistency and artifact hashes, never the truth of gameplay/visual claims."""
from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import re
import sys
from pathlib import Path

CAPS = {
    'synthetic', 'retail', 'gpu', 'audio', 'network_local',
    'network_real', 'human_play', 'human_review',
}
FIELDS = {
    'schema_version', 'task_id', 'candidate_tree', 'engine', 'created_at',
    'command', 'source', 'seed', 'ticks', 'overrides', 'capabilities', 'tests',
    'assertions', 'artifacts', 'unknowns', 'review', 'claim',
}


def record(value, fields, label):
    """Validate a v1 object before accessing it, including nested objects."""
    if not isinstance(value, dict) or set(value) != set(fields):
        raise ValueError(f'Invalid {label} fields')
    return value


def text(value, label):
    if not isinstance(value, str) or not value.strip() or '\0' in value:
        raise ValueError(f'Invalid {label}')
    return value


def strings(value, label):
    if not isinstance(value, list) or not all(isinstance(x, str) for x in value):
        raise ValueError(f'Invalid {label}')
    return value


def sha(value, nullable=False):
    if value is None and nullable:
        return
    if not isinstance(value, str) or not re.fullmatch('[0-9a-f]{64}', value):
        raise ValueError('Expected SHA-256')


def artifact_path(value):
    """Use a single portable relative spelling for declarations and references."""
    text(value, 'artifact path')
    if (any(ord(c) < 32 for c in value) or '\\' in value or ':' in value
            or any(part in ('', '.', '..') for part in value.split('/'))):
        raise ValueError('Unsafe/noncanonical artifact path: ' + value)
    return value


def validate(doc: dict, artifact_root: Path, require_pass: bool = False) -> dict:
    record(doc, FIELDS, 'evidence')
    if type(doc['schema_version']) is not int or doc['schema_version'] != 1:
        raise ValueError('Evidence fields/version mismatch')
    text(doc['task_id'], 'task identity')
    if not isinstance(doc['candidate_tree'], str) or not re.fullmatch(
            '(?:[0-9a-f]{40}|[0-9a-f]{64})', doc['candidate_tree']):
        raise ValueError('Invalid Git tree hash')
    engine = record(doc['engine'], ('rust', 'bevy', 'avian'), 'engine')
    for value in engine.values():
        text(value, 'engine/toolchain version')
    when = datetime.datetime.fromisoformat(text(doc['created_at'], 'timestamp').replace('Z', '+00:00'))
    if when.tzinfo is None:
        raise ValueError('Timestamp needs timezone')
    cmd = record(doc['command'], ('argv', 'cwd', 'exit_code'), 'command')
    argv = strings(cmd['argv'], 'command argv')
    if not argv or any('\0' in x for x in argv):
        raise ValueError('Invalid command argv')
    text(cmd['cwd'], 'working directory')
    if type(cmd['exit_code']) is not int:
        raise ValueError('Invalid command exit code')
    source = record(doc['source'], ('install_sha256', 'content_sha256'), 'source')
    for value in source.values():
        sha(value, nullable=True)
    caps = strings(doc['capabilities'], 'capabilities')
    if not caps or len(caps) != len(set(caps)) or not set(caps) <= CAPS:
        raise ValueError('Invalid capabilities')
    for key in ('overrides', 'unknowns'):
        strings(doc[key], key)
    if type(doc['seed']) is not int or doc['seed'] < 0:
        raise ValueError('Invalid seed')
    ticks = record(doc['ticks'], ('start', 'end'), 'ticks')
    if any(type(v) is not int or v < 0 for v in ticks.values()) or ticks['end'] < ticks['start']:
        raise ValueError('Invalid tick range')
    tests = record(doc['tests'], ('discovered', 'executed', 'passed', 'failed', 'ignored'), 'tests')
    if any(type(v) is not int or v < 0 for v in tests.values()):
        raise ValueError('Invalid test count')
    # Counts describe the declared task selection, not unrelated filtered tests.
    if (tests['executed'] != tests['passed'] + tests['failed']
            or tests['discovered'] != tests['executed'] + tests['ignored']):
        raise ValueError('Contradictory or incomplete test counts')
    if not isinstance(doc['assertions'], list):
        raise ValueError('Invalid assertions')
    assertion_ids = set()
    referenced_paths = set()
    for assertion in doc['assertions']:
        record(assertion, ('id', 'status', 'evidence'), 'assertion')
        identity = text(assertion['id'], 'assertion id')
        if identity in assertion_ids:
            raise ValueError('Duplicate assertion id: ' + identity)
        assertion_ids.add(identity)
        if assertion['status'] not in ('pass', 'fail', 'unknown'):
            raise ValueError('Invalid assertion status')
        references = strings(assertion['evidence'], 'assertion evidence')
        for reference in references:
            referenced_paths.add(artifact_path(reference))
    review = record(doc['review'], ('identity', 'method'), 'review')
    for value in review.values():
        text(value, 'review identity/method')
    if doc['claim'] not in ('implemented', 'checked', 'verified_original', 'release_approved'):
        raise ValueError('Invalid claim')
    if doc['claim'] in ('verified_original', 'release_approved'):
        sha(source['install_sha256'])
        sha(source['content_sha256'])
        if 'retail' not in caps or doc['unknowns']:
            raise ValueError('Original claim lacks retail evidence or has unresolved issues')
    if doc['claim'] == 'release_approved' and 'human_review' not in caps:
        raise ValueError('Release requires owner review')
    paths = set()
    root = artifact_root.resolve()
    if not isinstance(doc['artifacts'], list):
        raise ValueError('Invalid artifacts')
    for artifact in doc['artifacts']:
        record(artifact, ('path', 'sha256', 'kind'), 'artifact')
        path = artifact_path(artifact['path'])
        sha(artifact['sha256'])
        text(artifact['kind'], 'artifact kind')
        if path in paths:
            raise ValueError('Duplicate artifact path: ' + path)
        paths.add(path)
        p = root
        for component in path.split('/'):
            p = p / component
            if p.is_symlink():
                raise ValueError('Symlink in artifact path: ' + path)
        if not p.is_file() or root not in p.resolve().parents:
            raise ValueError('Missing/unsafe artifact: ' + path)
        digest = hashlib.sha256()
        with p.open('rb') as stream:
            for chunk in iter(lambda: stream.read(1024 * 1024), b''):
                digest.update(chunk)
        if digest.hexdigest() != artifact['sha256']:
            raise ValueError('Artifact hash mismatch: ' + path)
    if not referenced_paths <= paths:
        raise ValueError('Undeclared evidence artifact(s): ' + ', '.join(sorted(referenced_paths - paths)))
    if require_pass:
        if (cmd['exit_code'] != 0 or tests['executed'] == 0 or tests['failed']
                or tests['passed'] == 0 or tests['ignored']):
            raise ValueError('No successful complete nonempty execution')
        if not doc['assertions'] or any(
                a['status'] != 'pass' or not a['evidence'] for a in doc['assertions']):
            raise ValueError('Assertions incomplete')
        # Task #353 does not change v1 unknown/claim semantics. A separate
        # versioned model will distinguish task success from product readiness.
        if doc['unknowns']:
            raise ValueError('Unresolved issues')
    return {'structurally_valid': True, 'artifact_count': len(paths), 'claims_semantically_verified': False}


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('report', type=Path)
    ap.add_argument('--artifact-root', type=Path, required=True)
    ap.add_argument('--require-pass', action='store_true')
    args = ap.parse_args()
    print(json.dumps(validate(json.loads(args.report.read_text()), args.artifact_root, args.require_pass)))
    return 0


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (ValueError, KeyError, TypeError, OSError) as error:
        print('Invalid evidence:', error, file=sys.stderr)
        sys.exit(3)
