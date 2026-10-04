#!/usr/bin/env python3
"""Poll a cargo target dir's deps/ and log every disappearance of an executable.

A `cargo test` run prints `Running <unit> (<path>)` and then execs that path.
If the file is gone at exec time cargo prints

    could not execute process `<path> ...` (never executed)
    No such file or directory (os error 2)

and no test in that unit ran. This watcher records which executable files
appear and disappear, with timestamps, so a disappearance can be placed inside
or outside cargo's build phase. It is a poller (no root, no fs_usage); a gap
shorter than the poll interval can be missed, and a rename-over (atomic
replace) is invisible by construction.

usage: watch_deps.py <deps-dir> <interval-seconds> <out.jsonl>
"""

import json
import os
import sys
import time

# Suffixes cargo/rustc write beside the executables in deps/.
NON_EXEC_SUFFIXES = (
    ".d",
    ".rlib",
    ".rmeta",
    ".so",
    ".dylib",
    ".a",
    ".o",
    ".json",
    ".lock",
)


def snapshot(directory):
    """Map executable file name -> (size, mtime) for one poll."""
    seen = {}
    try:
        with os.scandir(directory) as entries:
            for entry in entries:
                try:
                    if not entry.is_file(follow_symlinks=False):
                        continue
                    if not os.access(entry.path, os.X_OK):
                        continue
                    if entry.name.endswith(NON_EXEC_SUFFIXES):
                        continue
                    stat = entry.stat(follow_symlinks=False)
                except OSError:
                    # The entry vanished inside this poll; that is itself the
                    # signal, and it is logged as an absence on the next pass.
                    continue
                seen[entry.name] = (stat.st_size, int(stat.st_mtime))
    except FileNotFoundError:
        return seen
    return seen


def main():
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    directory, interval, out_path = sys.argv[1], float(sys.argv[2]), sys.argv[3]

    start = time.time()
    previous = snapshot(directory)
    with open(out_path, "w", encoding="utf-8") as out:
        out.write(
            json.dumps({"event": "start", "t": 0.0, "present": len(previous)}) + "\n"
        )
        out.flush()
        while True:
            time.sleep(interval)
            current = snapshot(directory)
            now = round(time.time() - start, 3)
            for name in sorted(set(previous) - set(current)):
                size, mtime = previous[name]
                out.write(
                    json.dumps(
                        {
                            "event": "removed",
                            "t": now,
                            "name": name,
                            "last_size": size,
                            "last_mtime_age_s": round(time.time() - mtime, 1),
                        }
                    )
                    + "\n"
                )
            for name in sorted(set(current) - set(previous)):
                size, _ = current[name]
                out.write(
                    json.dumps(
                        {"event": "added", "t": now, "name": name, "size": size}
                    )
                    + "\n"
                )
            previous = current
            out.flush()


if __name__ == "__main__":
    main()
