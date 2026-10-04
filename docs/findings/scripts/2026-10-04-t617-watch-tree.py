#!/usr/bin/env python3
"""Watch a directory tree for unlinks of executables, with full stat attributes.

usage: watch_tree.py <root> <interval-seconds> <out.jsonl>

Records every executable file that disappears under <root> (recursively), with
the attributes it had when it was last seen: size, inode, link count and the
ages of mtime/ctime/atime at the moment of removal. That is what separates the
candidate rules for *which* files get removed (age window, size, link count,
name shape, plan membership) — the removed set alone cannot.

A poller, so a gap shorter than the interval is invisible and an atomic
rename-over is invisible by construction.
"""

import json
import os
import sys
import time

SKIP_DIRS = {".fingerprint", "incremental", "build", ".rustc_info.json"}
NON_EXEC_SUFFIXES = (".d", ".rlib", ".rmeta", ".so", ".dylib", ".a", ".o", ".json", ".lock")


def walk_executables(root):
    seen = {}
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if d not in SKIP_DIRS]
        for name in filenames:
            if name.endswith(NON_EXEC_SUFFIXES):
                continue
            path = os.path.join(dirpath, name)
            try:
                if not os.path.isfile(path) or not os.access(path, os.X_OK):
                    continue
                stat = os.stat(path, follow_symlinks=False)
            except OSError:
                continue
            seen[path] = (
                stat.st_size,
                stat.st_ino,
                stat.st_nlink,
                stat.st_mtime,
                stat.st_ctime,
                stat.st_atime,
            )
    return seen


def main():
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    root, interval, out_path = sys.argv[1], float(sys.argv[2]), sys.argv[3]

    start = time.time()
    previous = walk_executables(root)
    with open(out_path, "w", encoding="utf-8") as out:
        out.write(json.dumps({"event": "start", "t": 0.0, "present": len(previous)}) + "\n")
        out.flush()
        while True:
            time.sleep(interval)
            current = walk_executables(root)
            now = round(time.time() - start, 3)
            for path in sorted(set(previous) - set(current)):
                size, inode, nlink, mtime, ctime, atime = previous[path]
                out.write(
                    json.dumps(
                        {
                            "event": "removed",
                            "t": now,
                            "path": path,
                            "size": size,
                            "inode": inode,
                            "nlink": nlink,
                            "age_mtime_s": round(now + start - mtime, 1),
                            "age_ctime_s": round(now + start - ctime, 1),
                            "age_atime_s": round(now + start - atime, 1),
                        }
                    )
                    + "\n"
                )
            previous = current
            out.flush()


if __name__ == "__main__":
    main()
