# Task #1135: the retail smoke's flight path, re-derived for the original law

Date: 2026-10-09. Task: #1135 "Re-derive the scripted retail smoke's flight path
for the original flight law" (key `FLIGHT-ORIGINAL-RETAIL-SMOKE-SCRIPT`,
priority 50, `allowProtectedChanges: false`, depends on #797). Test prefix:
`accept_playtest_smoke_original_`. Capabilities used: `retail` (read-only
`$CS_GAME_DIR`), `gpu` (the framing captures) and ordinary build/test.
Provenance stays what #797's flight law carries — `OWNER-STATIC-2026-10-08`,
static evidence, never `verified_original`; nothing here is calibrated against
an original run (#358).

## What changed

| file | what |
| --- | --- |
| `crates/cs_app/src/playtest/smoke.rs` | `SteerHold`, `RETAIL_STEER` (the re-derived maneuver) and `RESET_SECONDS`; `pass()` now flies `RETAIL_STEER` instead of #649's `D`-then-`E` pair; the script's doc table and the constant's measured note |
| `crates/cs_app/src/playtest_retail.rs` | `SPAWN_FRACTION_X` `−0.35` → **`−0.6`** (the designed value #797 had to retune), with the measured matrix in the doc comment |
| `crates/cs_app/tests/playtest_retail_launch.rs` | the two `accept_playtest_smoke_original_` tests (the production run and the candidate matrix), the shared `fly_candidate` harness, and `CS_SMOKE_ORIGINAL_OUT` so an evidence run can keep the smoke's own artifacts |
| `crates/cs_app/tests/evidence_report_retail_smoke_script.rs` | this task's evidence harness (`docs/contracts/CLI-EVIDENCE.md`) |
| `crates/cs_app/tests/evidence_report_flight_original_playtest.rs`, `docs/findings/evidence/T797.json` | the resolved `scripted_smoke_path` unknown is gone from #797's report **and** from the generator that writes it |
| `docs/PLAYTEST-RETAIL.md` | the `#1135` note, the spawn row (fraction and coordinates) and the two spawn-relative camera eyes |
| `docs/findings/evidence/T1135.json` | this task's evidence report |

## The question

#649's scripted smoke reached the original area by holding yaw-right and then
roll-right (`steer_into_area`). That was tuned on the **synthetic** fixed-wing,
where banking turns the aircraft. Under the recovered original law (#797) the
same keys do not, and with the scene's designed spawn — 0.6 of the area's width
off the port side — the aircraft slid along the hull and never touched it:
measured, zero obstacle contacts for a whole 60 s run. #797 kept the smoke green
by moving the scene's spawn fraction `−0.6 → −0.35`, a scene knob doing a
script's job, and left the script itself to #1135 (recorded as the
`scripted_smoke_path` unknown of `docs/findings/evidence/T797.json`).

## The re-derived maneuver

`RETAIL_STEER` = **hold `D` (yaw right) from 10.7 s to 16.0 s of every pass** —
0.2 s after the pass's reset until 5.5 s after it, rudder only, no roll.

The two measurements that decide it, both taken by the committed matrix test at
2.0 s (the end of #649's yaw hold) and 3.0 s (one second into its roll) of
every candidate — before the earliest contact any of them records:

* **The rudder is what builds heading.** From 2.0 s to 3.0 s the rows still
  holding `D` keep turning: **19.27° → 26.74°, +7.5°**, wings level throughout
  (bank −0.46° → −0.55°). The recovered law steers its velocity at the nose (`lift_target` in
  `cs_sim::flight::original`: while the nose is within `lift_aoa` of the
  velocity the target *is* the velocity), so a nose led sideways by the rudder
  is precisely the input that curves the path.
* **The roll buys no heading at all.** The rows holding `E` instead reach
  **85.3° of bank in that same second and their heading does not move**
  (19.45° → 19.37°, i.e. −0.08°). A bank feeds only the attitude's own
  `FALL_OFF`/`BANK_OFF` coupling terms; it is not what turns this law's flight
  path, so #649's roll phase was buying an attitude the maneuver cannot use —
  and the pair still failed to reach the hull from the designed spawn.

The window ends 1.57 s after the first contact from the farthest designed
start, so the key is still down when the aircraft arrives and is up before the
pass resets.

## The measured matrix

All four candidates flown on one retail scene, each from its own declared
initial condition (`spawn_pose`'s formula evaluated at that fraction of the
measured extent), keys injected through the production input path, 6 s of the
pass's steer window (the contact always lands in the first 4 s):

| steer | spawn `x` fraction | contacts | first contact | heading @2.0 s → @3.0 s | bank @2.0 s → @3.0 s |
| --- | --- | --- | --- | --- | --- |
| #649 `D` then `E` (`STEER_649`) | `−0.6` (designed) | **0** | — | 19.45° → 19.37° | 1.54° → 85.31° |
| #649 `D` then `E` | `−0.35` (#797) | 4 | 3.32 s | 19.27° → 19.19° | 1.59° → 85.33° |
| `RETAIL_STEER` (`D`) | `−0.6` (designed) | 3 | **3.93 s** | 19.27° → 26.74° | −0.46° → −0.55° |
| `RETAIL_STEER` (`D`) | `−0.35` (#797) | 2 | 3.25 s | 19.02° → 26.58° | 0.07° → 0.07° |

Read from the log lines `PLAYTEST-SMOKE-ORIGINAL-MATRIX …`, printed and
asserted by `accept_playtest_smoke_original_candidate_matrix_justifies_the_spawn_and_the_script`.

Row 1 is #797's miss reproduced exactly — with the old keys from the designed
spawn the aircraft records no contact at all, which is why #797 moved the
spawn and why this task re-derived the script instead of the scene. Rows 3 and
4 are the re-derivation: the rudder turn reaches the hull from **both**
fractions, from the farther (designed) start in 3.93 s.

The whole production path is asserted separately by
`accept_playtest_smoke_original_retail_script_reaches_the_original_area`: a
20 s headless retail smoke (one full pass — response checks, pause, focus loss,
reset, steer) reports `obstacle_contacts = 3`, first contact at trace frame 870
(14.50 s of the pass on the trace's 0.1 s sample grid; the exact frame is
14.43 s), and that contact lies inside `RETAIL_STEER`'s 10.7–16.0 s hold. The
assertion fails if the maneuver is changed back, if it arrives after the key is
up, or if the spawn moves so that the run no longer flies what the docs say it
flies.

## The spawn fraction: back to `−0.6`

Both fractions work with the re-derived maneuver (rows 3 and 4), so the
**designed** value wins: `SPAWN_FRACTION_X = −0.6` again, giving the spawn
`(−116.97, −8.11, 205.79)` of the measured extent
`106.3356 × 144.9831 × 571.5788` m (bounds min `(−53.1680, −87.8525, −222.8929)`).
The scene keeps its own placement and the script does the steering, instead of
the placement being moved to compensate for the script. #797's retune is now
recorded as what it was — a measured stopgap, superseded here — and the code
comments on `SPAWN_FRACTION_X`, on `RETAIL_STEER` and in `docs/PLAYTEST-RETAIL.md`
all carry the same matrix.

The spawn-relative camera eyes moved with it (measured from the production
capture run, `PLAYTEST … eye=…`): `chase` `(−136.08, −5.96, 212.16)` and
`quarter` `(−131.83, −4.43, 188.80)`; `overview` is area-relative and unchanged
at `(−44.66, 46.04, 634.48)`. Both GPU framing tests still pass from the
farther spawn: `accept_playtest_area_flat_shape_every_documented_view_still_frames_the_aircraft`
(chase 11 375 aircraft pixels, quarter 7 984, overview 24 — the aircraft is
still in the area-framing frame at 26.6 m further outboard) and
`accept_playtest_retail_retail_c1c_area_and_bloodhawk_mesh_spawn_and_capture`.

## Evidence

* `docs/findings/evidence/T797.json` **loses its `scripted_smoke_path` unknown**
  — its resolving task was this one, and the script is now re-derived and
  measured. The same entry is gone from #797's report generator
  (`evidence_report_flight_original_playtest.rs`), so a regeneration reproduces
  the committed copy. Nothing else in that report changed, and the remaining
  four unknowns are untouched: this removal is the resolution of a named issue,
  not a deletion to satisfy a validator (`--require-pass` still refuses on that
  report, for the four issues that remain).
* `docs/findings/evidence/T1135.json` is this task's report, written by
  `crates/cs_app/tests/evidence_report_retail_smoke_script.rs` from the
  acceptance log, production discovery of `$CS_GAME_DIR` and the smoke run's own
  `report.json`/`trace.jsonl`. It carries **one** unknown,
  `scripted_maneuver_provenance`: the key schedule is a designed development
  input — the original's own scripted inputs and keyboard scaling were never
  recovered (#796) — so nothing here claims the original ever flew this
  sequence. That is the still-valid half of the old gate, restated against this
  deliverable with its own resolving task (#358) instead of against a defect in
  our code; it gates any claim that the smoke's key sequence, or the path it
  flies, reproduces an original input sequence. `--require-pass` therefore
  refuses on this report too, by design, and no unknown was deleted to make it
  pass.

## Tests

Prefix `accept_playtest_smoke_original_`, two tests, both green locally with
`CS_GAME_DIR` (both `#[ignore = "requires CS_GAME_DIR"]`, run with
`--include-ignored`):

```
PLAYTEST-SMOKE-ORIGINAL spawn_x_fraction=-0.6 spawn=(-116.9694, -8.1118, 205.7912) bounds_min=(-53.1680, -87.8525, -222.8929) steer=KeyD 10.7..16 s contacts=3 first_contact_frame=870 first_contact_s=14.50
PLAYTEST-SMOKE-ORIGINAL-MATRIX steer=#649 D+E spawn_x_fraction=-0.6 ... contacts=0 first_contact_frame=None heading_2s=19.45 bank_2s=1.54 heading_3s=19.37 bank_3s=85.31
PLAYTEST-SMOKE-ORIGINAL-MATRIX steer=#649 D+E spawn_x_fraction=-0.35 ... contacts=4 first_contact_s=3.32 heading_2s=19.27 bank_2s=1.59 heading_3s=19.19 bank_3s=85.33
PLAYTEST-SMOKE-ORIGINAL-MATRIX steer=RETAIL_STEER spawn_x_fraction=-0.6 ... contacts=3 first_contact_s=3.93 heading_2s=19.27 bank_2s=-0.46 heading_3s=26.74 bank_3s=-0.55
PLAYTEST-SMOKE-ORIGINAL-MATRIX steer=RETAIL_STEER spawn_x_fraction=-0.35 ... contacts=2 first_contact_s=3.25 heading_2s=19.02 bank_2s=0.07 heading_3s=26.58 bank_3s=0.07
test result: ok. 2 passed; 0 failed
```

Also run green after the change, with `CS_GAME_DIR`: the launch binary's other
tests, including `accept_playtest_retail_launch_scripted_smoke_collides_with_the_original_area`
(60 s, three passes, contacts from the re-derived maneuver) and the two GPU
capture tests named above.
