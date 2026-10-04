#!/usr/bin/env python3
"""Prefix each line of a command's output with elapsed seconds and a timestamp.

usage: timestamper.py <out-path>   (reads stdin, writes stdout + out-path)
"""

import sys
import time

if len(sys.argv) != 2:
    sys.exit(__doc__)

path = sys.argv[1]
start = time.time()
with open(path, "w", encoding="utf-8") as out:
    for line in sys.stdin:
        stamp = time.time()
        marked = f"{stamp - start:8.2f} {time.strftime('%H:%M:%S', time.localtime(stamp))} {line}"
        out.write(marked)
        out.flush()
        sys.stdout.write(marked)
        sys.stdout.flush()
