#!/usr/bin/env python3
"""Write and check the F15-F evidence report (task #403).

`docs/contracts/CLI-EVIDENCE.md` requires a task that uses a capability other
than plain build/test to produce `acceptance.json` beneath `CS_EVIDENCE_DIR`.
F15-F uses `retail` (read-only access to the original installation and to the
owner-supplied decrypted engine image), so this harness:

1. runs the committed acceptance selection
   (`tools/tests/test_engine_file_io_probe.py`) exactly as the finding
   documents, capturing its log;
2. runs the read-only probe and stores its facts as `probe.json`;
3. reads the installation and canonical-content hashes out of a production
   `cs-inspect inventory` report, never typing them in;
4. writes `acceptance.json` with the per-test assertions, artifacts and their
   hashes, and the task's one explicit unknown.

It writes only inside `--evidence-dir` (a private, Git-ignored directory) and
refuses to run outside a clean checkout, because `candidate_tree` must name the
commit the run really tested.

Usage:

    CS_ENGINE_IMAGE=/path/crimson.decrypted.exe \\
    CS_GAME_DIR=/path/retail \\
        python3 tools/engine_file_io_evidence.py \\
            --evidence-dir private/evidence/F15-F \\
            --inventory private/f15f/inventory.json

Then:

    python3 tools/validate_evidence.py \\
        private/evidence/F15-F/acceptance.json \\
        --artifact-root private/evidence/F15-F

`--require-pass` is deliberately not used: this task's deliverable includes an
explicit unknown (see the finding's "The unknown, and what would settle it"),
and the validator treats a nonempty `unknowns` list as "not a pass" by design.
The structural check still hashes every artifact and checks every count.
"""

from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import os
import re
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
TESTS_DIR = REPO_ROOT / "tools" / "tests"
TEST_PATTERN = "test_engine_file_io_probe.py"
TASK_ID = "F15-F"

UNKNOWN = (
    "Whether the original engine wrote any persistent derived artefact during a "
    "normal mission load, and what it keyed it on, is unmeasured. The static "
    "audit found no such writer, path, import or vocabulary, but a negative over "
    "static spans cannot exclude a run-time-assembled path, a DLL writing on the "
    "engine's behalf, a script-invoked GameZ write that is later read back, or an "
    "unenumerated code path. What would settle it is an owner-run file-system "
    "trace of the original executable during one mission load (Process Monitor "
    "file events, with the executable digest and the written-path list). See "
    "docs/findings/2026-10-27-f15-f-original-cache-and-keys.md, "
    "'The unknown, and what would settle it'."
)


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(argv: list[str], cwd: Path, env: dict[str, str]) -> subprocess.CompletedProcess:
    return subprocess.run(argv, cwd=cwd, env=env, capture_output=True, text=True)


def toolchain_versions() -> dict[str, str]:
    rust = run(["rustc", "--version"], REPO_ROOT, os.environ.copy())
    if rust.returncode != 0:
        raise SystemExit("rustc --version failed; the report needs the toolchain")
    versions = {"rust": rust.stdout.strip()}
    lock = (REPO_ROOT / "Cargo.lock").read_text()
    for crate, key in (("bevy", "bevy"), ("avian3d", "avian")):
        match = re.search(
            rf'\[\[package\]\]\nname = "{re.escape(crate)}"\nversion = "([^"]+)"',
            lock,
        )
        if match is None:
            raise SystemExit(f"Cargo.lock does not pin {crate}")
        versions[key] = match.group(1)
    return versions


TEST_LINE = re.compile(
    r"^(?P<name>test_[A-Za-z0-9_]+) \((?P<module>[^)]+)\)"
    r"(?: \.\.\. )?(?P<outcome>ok|skipped.*|FAIL|ERROR)"
)


def parse_tests(log: str) -> list[dict[str, str]]:
    results = []
    seen = set()
    for line in log.splitlines():
        match = TEST_LINE.match(line.strip())
        if match is None:
            continue
        name = match.group("name")
        if name in seen:
            continue
        seen.add(name)
        outcome = match.group("outcome")
        status = "pass" if outcome == "ok" else ("skip" if outcome.startswith("skipped") else "fail")
        results.append({"id": name, "status": status})
    return results


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--evidence-dir", type=Path, required=True)
    parser.add_argument(
        "--inventory",
        type=Path,
        required=True,
        help="a cs-inspect inventory report for the same installation",
    )
    args = parser.parse_args(argv)

    image = os.environ.get("CS_ENGINE_IMAGE")
    game_dir = os.environ.get("CS_GAME_DIR")
    if not image or not game_dir:
        print(
            "CS_ENGINE_IMAGE and CS_GAME_DIR must both name the owner's inputs: "
            "this report cannot be produced without them",
            file=sys.stderr,
        )
        return 2

    status = run(["git", "status", "--porcelain"], REPO_ROOT, os.environ.copy())
    if status.returncode != 0 or status.stdout.strip():
        print(
            "the working tree is not clean: commit the change first, then "
            "regenerate, so candidate_tree names the tested commit",
            file=sys.stderr,
        )
        print(status.stdout, file=sys.stderr)
        return 2
    head = run(["git", "rev-parse", "HEAD^{tree}"], REPO_ROOT, os.environ.copy())
    candidate_tree = head.stdout.strip()
    if head.returncode != 0 or not re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", candidate_tree):
        print("cannot read HEAD^{tree}", file=sys.stderr)
        return 2

    evidence_dir = args.evidence_dir.resolve()
    evidence_dir.mkdir(parents=True, exist_ok=True)

    # 1. The acceptance selection.
    test_argv = [
        sys.executable,
        "-m",
        "unittest",
        "discover",
        "-s",
        str(TESTS_DIR),
        "-p",
        TEST_PATTERN,
        "-v",
    ]
    test_run = run(test_argv, REPO_ROOT, os.environ.copy())
    tests_log = evidence_dir / "tests.log"
    tests_log.write_text(
        "$ " + " ".join(test_argv) + "\n\n" + test_run.stdout + test_run.stderr
    )
    results = parse_tests(test_run.stdout + test_run.stderr)
    if not results:
        print("the acceptance selection produced no test results", file=sys.stderr)
        return 3

    # 2. The probe's own facts.
    sys.path.insert(0, str(REPO_ROOT / "tools"))
    import engine_file_io_probe as probe  # noqa: PLC0415

    facts = probe.analysis_data(probe.load_image(image), game_dir)
    probe_path = evidence_dir / "probe.json"
    probe_path.write_text(json.dumps(facts, indent=2, sort_keys=True) + "\n")

    # 3. The installation fingerprints, from production code.
    inventory = json.loads(args.inventory.read_text())
    fingerprints = inventory.get("fingerprints", {})
    install_sha = fingerprints.get("install_sha256")
    content_sha = fingerprints.get("content_sha256")
    if not install_sha or not content_sha:
        print(
            f"{args.inventory} carries no install/content fingerprints",
            file=sys.stderr,
        )
        return 3

    passed = sum(1 for r in results if r["status"] == "pass")
    failed = sum(1 for r in results if r["status"] == "fail")
    skipped = sum(1 for r in results if r["status"] == "skip")
    report = {
        "schema_version": 1,
        "task_id": TASK_ID,
        "candidate_tree": candidate_tree,
        "engine": toolchain_versions(),
        "created_at": datetime.datetime.now(datetime.timezone.utc)
        .replace(microsecond=0)
        .isoformat()
        .replace("+00:00", "Z"),
        "command": {
            "argv": test_argv,
            "cwd": str(REPO_ROOT),
            "exit_code": test_run.returncode,
        },
        "source": {
            "install_sha256": install_sha,
            "content_sha256": content_sha,
        },
        "seed": 0,
        "ticks": {"start": 0, "end": 0},
        "overrides": [],
        "capabilities": ["retail"],
        "tests": {
            "discovered": len(results),
            "executed": len(results) - skipped,
            "passed": passed,
            "failed": failed,
            "ignored": skipped,
        },
        "assertions": [
            {
                "id": r["id"],
                "status": "pass" if r["status"] == "pass" else ("fail" if r["status"] == "fail" else "unknown"),
                "evidence": ["tests.log", "probe.json"],
            }
            for r in results
        ],
        "artifacts": [
            {"path": "tests.log", "sha256": sha256(tests_log), "kind": "log"},
            {"path": "probe.json", "sha256": sha256(probe_path), "kind": "json"},
        ],
        "unknowns": [UNKNOWN],
        "review": {
            "identity": "deepseek-1 (implementer); no reviewer has run yet",
            "method": (
                "Every field is derived from the recorded run: candidate_tree from "
                "HEAD^{tree} on a clean checkout, the counts and assertion statuses "
                "from tests.log (the same selection the finding documents), the "
                "probe artifacts from the read-only probe over $CS_ENGINE_IMAGE, and "
                "install_sha256/content_sha256 from a production cs-inspect "
                "inventory of the same $CS_GAME_DIR in the same session. Nothing "
                "is typed in. SCOPE: this is static analysis of the original image "
                "plus retail data; no original executable ran, so the claim is "
                "'implemented', never 'verified_original', and the artifact-level "
                "result is an explicit unknown about original run-time behaviour, "
                "not a negative claim about it. tools/validate_evidence.py is run "
                "without --require-pass on purpose: the deliverable is itself an "
                "explicit unknown, so the structural check (every artifact hash, "
                "every count) is the applicable one, and dropping the unknown from "
                "machine-readable evidence to make --require-pass green is exactly "
                "what the owner directive forbids."
            ),
        },
        "claim": "implemented",
    }
    report_path = evidence_dir / "acceptance.json"
    report_path.write_text(json.dumps(report, indent=2) + "\n")
    print(f"wrote {report_path}")
    print(
        "next: python3 tools/validate_evidence.py "
        f"{report_path} --artifact-root {evidence_dir}"
    )
    return 0 if test_run.returncode == 0 else 3


if __name__ == "__main__":
    sys.exit(main())
