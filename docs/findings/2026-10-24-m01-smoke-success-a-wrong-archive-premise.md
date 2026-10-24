# `--smoke-success` A (#2081): the task's premise comes from the wrong archive

The task text (and `tests/campaign/m01_play_triggers_route.rs`, which it cites
as the reference) place the player inside `OBJECTIVE3`'s 700 m radius of
`workersvoyagezep` and shoot that zeppelin's eighteen `reng<n>`/`leng<n>`
engine parts to complete blocks 2, 5, 7, 8.

That route is measured on `ZBD/C1C/M01` (campaign position ch2-m01, work order
M06 "The Red Menace"): the test plans `label("M06")`, "The Red Menace".
`--mission M01` is "The Lost Treasure", `ZBD/C3/M01`
(`2026-10-17-m01-archive-decision.md`).

## Evidence (retail run, release build, `--smoke-success`, this branch)

`cs --cs-path <retail> --mission M01 --smoke-seconds 240 --smoke-success`:

- `report.json` `actors` lists `player`, `medkestrel_1..3`, `wingman_1`,
  `...world1.piratezep` and `...world1.cargozep1`. There is **no**
  `workersvoyagezep`.
- The composition's `MissionPartReport` is present with `parts: 0` and
  `refusals: []`, so no engine part-state chain exists in the windowed M01
  at all.
- The stage waited 600 frames after control, found no engine part, stopped
  and recorded `parts_never_found: true`; 0 gun presses, 0 completed blocks
  seen.
- `2026-10-24-m01-convoy-destruction.md` states that in this M01 OBJECTIVE6
  wakes the convoy trucks' turrets and OBJECTIVE7 clears their target flags,
  not an engine ladder. The block numbers 2/5/7/8 name different objectives
  here.

So the stage as specified can never pass on M01. It is not a defect of the
smoke; the premise is.

## What is on the branch

Everything but the M01 route is generic and works: the `--smoke-success`
flag and its mode exclusions, `MissionSmokeMode::Success`, the
completed-block accumulator on the host's per-tick report
(`record_success_completions`), the checks and `report.json` section. The
stage's target (anchor name, engine-part chains, block numbers) are constants
at the top of `smoke/success.rs`.

## What is needed

The owner or planner must say which M01 (`ZBD/C3/M01`) objective blocks the
success stage drives, taken from that archive's control program. The first
step is listing its blocks and the part/actor chains they read (the same
census that was done for C1C in the F39/M01-LC findings). The spec of
#2079/#2081/#2082 should then be rewritten against that list.
