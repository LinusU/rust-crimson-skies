#!/usr/bin/env python3
"""Sample the host process table and record every short-lived process.

usage: sample_procs.py <interval-seconds> <out.jsonl>

Runs `ps -Ao pid,ppid,args` on a tight interval and records each pid the first
time it is seen, with its parent and full command line, plus a periodic full
table snapshot. A prune pass is short-lived, so it shows up here as a pid that
appears and then disappears; its ppid chain can be resolved against the
long-lived processes recorded at the start.

This is the no-root attribution method available on this host: `fs_usage` and
`dtrace` both require root, and FSEvents reports no pid.
"""

import json
import subprocess
import sys
import time


def table():
    out = subprocess.run(
        ["ps", "-Ao", "pid,ppid,args"], capture_output=True, text=True, check=False
    ).stdout
    rows = {}
    for line in out.splitlines()[1:]:
        parts = line.strip().split(None, 2)
        if len(parts) < 2:
            continue
        try:
            pid, ppid = int(parts[0]), int(parts[1])
        except ValueError:
            continue
        rows[pid] = (ppid, parts[2] if len(parts) > 2 else "")
    return rows


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    interval, out_path = float(sys.argv[1]), sys.argv[2]

    start = time.time()
    seen = set()
    last_full = 0.0
    with open(out_path, "w", encoding="utf-8") as out:
        while True:
            now = round(time.time() - start, 3)
            rows = table()
            for pid, (ppid, args) in rows.items():
                if pid not in seen:
                    seen.add(pid)
                    out.write(
                        json.dumps(
                            {"event": "new", "t": now, "pid": pid, "ppid": ppid, "args": args}
                        )
                        + "\n"
                    )
            if now - last_full >= 30.0:
                last_full = now
                out.write(
                    json.dumps(
                        {
                            "event": "table",
                            "t": now,
                            "processes": [
                                {"pid": pid, "ppid": ppid, "args": args}
                                for pid, (ppid, args) in sorted(rows.items())
                            ],
                        }
                    )
                    + "\n"
                )
            out.flush()
            time.sleep(interval)


if __name__ == "__main__":
    main()
