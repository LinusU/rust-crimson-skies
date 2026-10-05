# #637: the CI doctest linker SIGBUS was runner disk exhaustion, and has not recurred since the disk fix

Date: 2026-10-05. Task: #637 "CI red on a doctest linker bus error, not on any test
assertion". Capabilities used: ordinary build/test and `gh` read access to CI logs only. No
`CS_GAME_DIR`, GPU, audio or network capability, so no `acceptance.json` is produced
(`docs/contracts/CLI-EVIDENCE.md`). This is about the project's CI gate, not the original game.

## Measured

Source: the last 200 runs of workflow CI (2026-10-03 20:48Z to 2026-10-05 04:37Z): 164 success,
26 failure, 9 cancelled, one in progress at the time of the query. Logs were read with
`gh run view <id> --log`.

* `ld terminated with signal 7 [Bus error]`: I read the logs of 13 of the 26 failures (the other
  13 were not examined) and 7 contain it (runs 37231163923, 37228844393, 37219056500,
  37218972571, 37215435982, 37202040934, 37201380587), all between 2026-10-04 12:13Z and 20:11Z.
  The doctest that dies is not always `cs_app`'s livery doctest: 37231163923 and 37215435982
  report several links dying in one run. In addition, every run from 2026-10-04 18:00Z on
  (74 runs) was scanned for `signal 7`.
* Commit 7a1018f2 ("Free runner disk space before the CI build", 2026-10-04 20:14Z) added the
  `Free runner disk space` step. **In the 66 runs started after 20:15:30Z there is no `signal 7`
  anywhere in the logs.** Run 37231375211 (20:15Z) is the first run that has the step.
* With the step, the runner starts with 41G used / 105G free of 145G, and finishes `cargo test`
  with 126G to 135G used. The test job therefore writes about 85 to 95G.
  Free space at the end of tests, for the five most recent runs that print it: 19G (20:15Z on
  10-04), 12G, 12G, 11G, 11G. It is shrinking by roughly 1G per few hours as the workspace grows.
* Failures without signal 7 (e.g. 37231768769 and 37224207272: `cargo test` step failed, no
  linker message) were not diagnosed here and are not part of this finding.
* **Same-commit reruns were not measured as a clean A/B.** Commit 1a0315be failed three times
  (18:22Z, 19:35Z, 20:21Z) with, in order: no signal 7, one signal 7, no signal 7. That shows the
  failure is intermittent per run but not that a rerun is green, because the other two failures
  were not linker failures. I did not trigger reruns: `.github/` is protected and spending
  runner minutes to re-measure a failure that has not recurred in 66 runs is not warranted.

## Cause

SIGBUS in `ld` while writing an output file is what a full filesystem does to an mmap-written
link output. The linker was killed mid-write, the tests themselves had passed. This is the
cause that 7a1018f2's own comment in `ci.yml` already names; the measurements above confirm it
(about 90G written per run on a runner that offers about 14G free without the cleanup, and
`CARGO_PROFILE_*_DEBUG=line-tables-only` plus the `rm -rf` of preinstalled toolchains
together restored 105G).

## Decision

1. **Do not add a retry for "signal 7 during doctest linking".** A retry would hide a runner
   that is out of disk, the next run starts from the same cache and fails the same way, and
   `AGENTS.md` asks that a green run stay green because the work is right.
2. **Do not split the doctests** so the largest link is alone. The failing link is simply the
   one that happened to be writing when the disk filled up; splitting moves the failure.
3. **No bigger runner image is needed now.** 11G of free space at the end of the job is
   enough, but the margin is shrinking.
4. Nothing in this task edits `.github/workflows/ci.yml`: the task has no
   `allowProtectedChanges`. The one worthwhile change there is a follow-up task, filed with
   this finding: make the `Disk after tests` step fail loudly when free space is low, so the
   next exhaustion is reported as "disk below N G" rather than as a linker signal on a random
   doctest.

## Relation to #610 and #617

Not the same failure. #617 and #610 are an external process deleting harness binaries under
concurrent cargo on a busy macOS machine; #637 is a Linux runner running out of disk. What they
share is only that the test command died for a reason outside the code under test. The
machine-level measure that would catch both is the same habit: record free disk at the start and
end of the run, which CI now does.
