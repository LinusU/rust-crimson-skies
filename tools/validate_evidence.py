#!/usr/bin/env python3
"""Check report consistency and artifact hashes, never the truth of gameplay/visual claims."""
from __future__ import annotations
import argparse, datetime, hashlib, json, re, sys
from pathlib import Path

CAPS={'synthetic','retail','gpu','audio','network_local','network_real','human_play','human_review'}

def sha(value, nullable=False):
    if value is None and nullable:return
    if not isinstance(value,str) or not re.fullmatch('[0-9a-f]{64}',value):raise ValueError('Expected SHA-256')

def validate(doc:dict, artifact_root:Path, require_pass:bool=False) -> dict:
    required={'schema_version','task_id','candidate_tree','engine','created_at','command','source','seed','ticks',
              'overrides','capabilities','tests','assertions','artifacts','unknowns','review','claim'}
    if set(doc)!=required or doc['schema_version']!=1:raise ValueError('Evidence fields/version mismatch')
    if not isinstance(doc['task_id'],str) or not doc['task_id']:raise ValueError('Missing task identity')
    if not isinstance(doc['candidate_tree'],str) or not re.fullmatch('(?:[0-9a-f]{40}|[0-9a-f]{64})',doc['candidate_tree']):raise ValueError('Invalid Git tree hash')
    for k in ('rust','bevy','avian'):
        if not isinstance(doc['engine'].get(k),str) or not doc['engine'][k]:raise ValueError('Missing engine/toolchain version')
    when=datetime.datetime.fromisoformat(doc['created_at'].replace('Z','+00:00'))
    if when.tzinfo is None:raise ValueError('Timestamp needs timezone')
    cmd=doc['command'];argv=cmd.get('argv')
    if not isinstance(argv,list) or not argv or not all(isinstance(x,str) and '\0' not in x for x in argv):raise ValueError('Invalid command argv')
    if not isinstance(cmd.get('cwd'),str) or not cmd['cwd']:raise ValueError('Missing working directory')
    if type(cmd.get('exit_code')) is not int:raise ValueError('Invalid command exit code')
    for k in ('install_sha256','content_sha256'):sha(doc['source'].get(k),nullable=True)
    caps=doc['capabilities']
    if not isinstance(caps,list) or not caps or len(caps)!=len(set(caps)) or not set(caps)<=CAPS:raise ValueError('Invalid capabilities')
    for k in ('overrides','unknowns'):
        if not isinstance(doc[k],list) or not all(isinstance(x,str) for x in doc[k]):raise ValueError('Invalid '+k)
    if type(doc['seed']) is not int or doc['seed']<0:raise ValueError('Invalid seed')
    ticks=doc['ticks']
    if any(type(ticks.get(k)) is not int or ticks[k]<0 for k in ('start','end')) or ticks['end']<ticks['start']:raise ValueError('Invalid tick range')
    tests=doc['tests']
    for k in ('discovered','executed','passed','failed','ignored'):
        if type(tests.get(k)) is not int or tests[k]<0:raise ValueError('Invalid test count')
    if tests['executed']!=tests['passed']+tests['failed'] or tests['executed']+tests['ignored']>tests['discovered']:raise ValueError('Contradictory test counts')
    if not isinstance(doc['assertions'],list):raise ValueError('Invalid assertions')
    for a in doc['assertions']:
        if not isinstance(a,dict) or not a.get('id') or a.get('status') not in ('pass','fail','unknown') or not isinstance(a.get('evidence'),list):raise ValueError('Invalid assertion')
    for k in ('identity','method'):
        if not isinstance(doc['review'].get(k),str) or not doc['review'][k]:raise ValueError('Missing review identity/method')
    if doc['claim'] not in ('implemented','checked','verified_original','release_approved'):raise ValueError('Invalid claim')
    if doc['claim'] in ('verified_original','release_approved'):
        sha(doc['source']['install_sha256']);sha(doc['source']['content_sha256'])
        if 'retail' not in caps or doc['unknowns']:raise ValueError('Original claim lacks retail evidence or has unresolved issues')
    if doc['claim']=='release_approved' and 'human_review' not in caps:raise ValueError('Release requires owner review')
    paths=set();root=artifact_root.resolve()
    if not isinstance(doc['artifacts'],list):raise ValueError('Invalid artifacts')
    for a in doc['artifacts']:
        path=a.get('path');sha(a.get('sha256'))
        if not isinstance(path,str) or Path(path).is_absolute() or '..' in Path(path).parts or path in paths:raise ValueError('Unsafe/duplicate artifact path')
        paths.add(path);p=root/path
        if not a.get('kind') or p.is_symlink() or not p.is_file() or root not in p.resolve().parents:raise ValueError('Missing/unsafe artifact: '+path)
        if hashlib.sha256(p.read_bytes()).hexdigest()!=a['sha256']:raise ValueError('Artifact hash mismatch: '+path)
    if require_pass:
        if cmd['exit_code']!=0 or tests['executed']==0 or tests['failed'] or tests['passed']==0:raise ValueError('No successful nonempty execution')
        if not doc['assertions'] or any(a['status']!='pass' or not a['evidence'] for a in doc['assertions']):raise ValueError('Assertions incomplete')
        if doc['unknowns']:raise ValueError('Unresolved issues')
    return {'structurally_valid':True,'artifact_count':len(paths),'claims_semantically_verified':False}

def main():
    ap=argparse.ArgumentParser(description=__doc__);ap.add_argument('report',type=Path)
    ap.add_argument('--artifact-root',type=Path,required=True);ap.add_argument('--require-pass',action='store_true')
    a=ap.parse_args();print(json.dumps(validate(json.loads(a.report.read_text()),a.artifact_root,a.require_pass)));return 0

if __name__=='__main__':
    try:sys.exit(main())
    except (ValueError,KeyError,TypeError,OSError) as e:print('Invalid evidence:',e,file=sys.stderr);sys.exit(3)
