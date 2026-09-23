#!/usr/bin/env python3
"""Write newly authored format fixtures; no original game data is used."""
from __future__ import annotations
import argparse, hashlib, json, struct
from pathlib import Path


def make(out: Path) -> dict:
    out.mkdir(parents=True, exist_ok=True)
    # Two by three makes accidental width/height swaps and vertical flips observable.
    width, height = 2, 3
    base = bytes([255,0,0, 0,255,0, 0,0,255, 255,255,0, 0,255,255, 255,0,255])
    masks = [bytes([0,51,102,153,204,255]), bytes([255,204,153,102,51,0]), bytes([0,255,0,255,0,255])]
    overlay = bytes([10,20,30,0, 40,50,60,51, 70,80,90,102, 100,110,120,153, 130,140,150,204, 160,170,180,255])
    bm = struct.pack('<HH', height, width) + base + b''.join(masks) + overlay
    (out/'rectangular.bm').write_bytes(bm)
    # Single root, two uncompressed files. Both length fields deliberately equal:
    # this fixture does NOT resolve the real compressed-length ambiguity.
    names = b'HELLO.TXT\0EMPTY.DAT\0'
    body = b'Newly authored synthetic archive.\n'
    start = 8 + 2*24 + len(names)
    rof = struct.pack('<II',2,len(names))
    rof += struct.pack('<IIIIII',start,len(body),len(body),0,len(b'HELLO.TXT')+1,101)
    rof += struct.pack('<IIIIII',start+len(body),0,0,0,len(b'EMPTY.DAT')+1,102)
    rof += names + body
    (out/'flat-uncompressed.rof').write_bytes(rof)
    # This is a candidate INTERP envelope, not proof of Crimson Skies mission semantics.
    commands = [b'SYNTHETIC\0alpha\0beta\0', b'VALUE\0' + b'42\0']
    header = struct.pack('<III',0x08971119,7,1)
    entry = b'synthetic_probe'.ljust(120,b'\0') + struct.pack('<II',0,12+128)
    interp = header + entry + b''.join(struct.pack('<II',len(c),c.count(b'\0'))+c for c in commands) + struct.pack('<I',0)
    (out/'synthetic.interp').write_bytes(interp)
    # Deliberately malformed data used to verify rejection, not canonical examples.
    (out/'truncated.bm').write_bytes(bm[:-1])
    (out/'bad-version.interp').write_bytes(header[:4]+struct.pack('<I',999)+interp[8:])
    expected = {
        'provenance':'All bytes newly authored by tools/make_synthetic_fixtures.py; no retail content.',
        'bm':{'width':width,'height':height,'bytes':len(bm),'rgb_first':[255,0,0],
              'rgb_last':[255,0,255],'mask_first':[0,255,0], 'rgba_last':[160,170,180,255]},
        'rof':{'names':['HELLO.TXT','EMPTY.DAT'],'record_ids':[101,102], 'payload_utf8':body.decode(),
               'compressed_length_semantics':'not tested or established'},
        'interp':{'signature':0x08971119,'version':7,'names':['synthetic_probe'],
                  'raw_tokens':[[x.decode() for x in c[:-1].split(b'\0')] for c in commands],
                  'mission_language_semantics':'not tested or established'},
        'sha256':{p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(out.iterdir()) if p.suffix in ['.bm','.rof','.interp']}
    }
    (out/'expected.json').write_text(json.dumps(expected,indent=2)+'\n')
    return expected

if __name__=='__main__':
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--out',type=Path,default=Path(__file__).resolve().parents[1]/'fixtures/synthetic')
    args=ap.parse_args();make(args.out)
    print('Created newly authored synthetic examples in',args.out)
