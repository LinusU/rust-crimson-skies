# M01's owner-command smoke, and what it measured on the owner's machine

Task: Rally #1535 (`M01-OWNER-SMOKE`), 2026-10-17.
Implementer: bunny-alpha-1 (Rally agent, `opencode-go/mimo-v2.6-flash`).

## Why this task exists

The owner played the game on 2026-10-17 with the launch this repository
documents —

```sh
CS_CAPABILITIES=retail,gpu,audio CS_ENGINE_IMAGE=... \
  target/release/cs --cs-path <retail> --mission M01
```

on main `9b75d132`, and reported that most of the mission was not visible:
untextured, unlit terrain ending at a square edge; no sea; no objective text on
screen; no other aircraft or zeppelins ever seen; `Space` and `Left Ctrl`
firing nothing. Only the voice radio worked. Every task behind those surfaces
(#1433–#1440 and their follow-ups) was green in its headless acceptance tests —
which proves the records and the systems, not the **window**.

This task therefore adds the measurement that was missing: a scripted smoke on
*the owner's own command*, which fails when the player cannot see, hear or
shoot the mission.

## The command

```sh
cargo build --release --locked -p cs_app --bin cs
CS_CAPABILITIES=retail,gpu,audio \
CS_ENGINE_IMAGE=<the owner's crimson.decrypted.exe> \
target/release/cs --cs-path <retail> --mission M01 \
  --smoke-seconds 45 --capture-dir private/evidence/M01-OWNER-SMOKE
```

`--smoke-seconds`/`--capture-dir` now belong to `--mission` as well as
`--playtest` (`crates/cs_app/src/cli.rs`); the run is the same windowed
composition the owner plays, driven with a manual 1/60 s clock, keys injected
through Bevy's keyboard state, framebuffer PNGs and a `report.json`, and it
**exits non-zero when a check fails**. A scripted run is never `human_play`
and nothing in it is `verified_original`.

The implementation is `crates/cs_app/src/mission_session/smoke.rs`; the run
sequence, the probes and every floor are documented there and in
`docs/PLAYTEST.md` ("The owner-command smoke").

## The run of record

`private/evidence/M01-OWNER-SMOKE/` (ignored by Git) holds this run's 19
framebuffer PNGs and `report.json`, produced on this machine (Apple M3 Pro,
Metal) on 2026-10-17 by the acceptance test itself (the release binary,
the same windowed launch the owner plays). The PNGs this finding names:

| PNG | what it is |
| --- | --- |
| `frame_00033_world.png` | the terrain probe: horizon, weather, precipitation, player, every actor and the UI hidden |
| `frame_00040_sea_visible.png` / `frame_00044_sea_absent.png` | the differential sea probe (session paused between them) |
| `frame_00063_actor_0.png` … `frame_00351_actor_12.png` | one probe per spawned actor, player placed 400 m abeam it |
| `frame_01464_hud.png` | the objective widget's frame, after the approach |
| `frame_01525_guns.png` | one second into the `Space` hold |
| `frame_01616_rocket.png` | after `Left Ctrl` beside the hostile zeppelin |

The console of the run (13 checks, **3 failed**):

| check | verdict | measured |
| --- | --- | --- |
| `world_terrain_drawn` | **pass** | 1 971 178 of 3 686 400 probe pixels off the clear colour (534 per mille) |
| `world_terrain_textured` | **FAIL** | **1 per mille** of the world's pixels differ from a neighbour (floor 20); 699 distinct colours, which the horizon fog alone produces |
| `world_terrain_lit` | **FAIL** | the near field spans luminance **82..83 — spread 1** (floor 24); the whole frame's 120 is the fog band, not shading |
| `world_sea_visible` | skip | c1c's object records declare no known `Water` surface — recorded by name, never counted as a pass |
| `hud_objective_current` | **FAIL** | after 1 081 frames flying the measured #1486 approach into the proximity trigger, the lifecycle model still marks no current objective |
| `hud_objective_matches` | pass | nothing current, nothing drawn — the widget agrees with the model |
| `actors_project_and_cover` | **pass** | all 13 drew: player 176 579 px, the nine `aiv.zrd` aircraft 287–512 px each from their 400 m placements, the three zeppelins 616 k–1 045 k px |
| `weapon_guns_fire` | **pass** | 90 rounds through the mission's weapon session in the held second (66 live at once) |
| `weapon_rounds_drawn` | **pass** | all 66 live round entities carry a mesh |
| `weapon_rocket_launches` | **pass** | a live rocket after `Left Ctrl` beside `workersvoyagezep` |
| `audio_radio_queued` | **pass** | 1 radio line reached the mixer |
| `audio_engine_queued` | **pass** | 9 engine bindings, 9 loops running |
| `smoke_shots_delivered` | **pass** | all 19 requested frames arrived |

**The metric discriminates.** The same probe measures the player's own
textured airframe (`frame_00063_actor_0.png`) at **233 per mille** local
detail and 15 374 colours, against the terrain's **1 per mille** — 200× apart,
with the floor of 20 in between. That is what makes "untextured" a failure
here rather than a colour count the horizon fog can pass.

**What this run says about the owner's report.** Three of the owner's five
defects were closed between their playtest of `9b75d132` and this run — the
other aircraft and zeppelins are now drawn (`M01-WINDOW-ACTORS` #1538: the
aircraft's `planes.zbd` models and each zeppelin's world-node visual) and
shots now draw (`M01-WINDOW-FIRE`): both checks pass with the pixels above.
Two remain and are red: the terrain is still flat and unlit (1 per mille
detail, near-field spread 1 — visible in `frame_00033_world.png`), and the
objective flow still wakes nothing from the measured trigger approach, so no
objective line is ever current for the widget to draw. The radio and the
engine — the two things the owner *did* hear — pass.

## What changed

* `crates/cs_app/src/mission_session/smoke.rs` (new) — the smoke: the phase
  script, the hide-everything-but probes, the player placements, the check
  judge (`evaluate`, pure) and the `report.json` writer.
* `crates/cs_app/src/mission_session/compose.rs` — `run_windowed` takes the
  optional smoke request, installs the plugin, pins the clock and turns a
  failing check into a non-zero exit.
* `crates/cs_app/src/mission_session/evidence.rs` — the pixel helpers
  (`count_world_pixels`, `clear_colour_encodings`, `same_colour`) became
  `pub(super)` so both probes count against the same declared clear colour.
* `crates/cs_app/src/mission_session/mod.rs`, `cli.rs`, `mission_launch.rs`,
  `playtest/mod.rs` — re-exports, the CLI flags and help, the plumbing, and
  `PlaytestCamera::reseat`.
* `crates/cs_app/tests/accept_m01_owner_smoke.rs` (new) — the task's
  acceptance tests: the judge against a healthy mission and against the
  owner's own sighting (every defect fails its own named check), the CLI
  request and its refusals, and the ignored windowed run.
* `docs/PLAYTEST.md` — the new mode, documented.
* `AGENTS.md` (owner-authorized protected change, per the task description) —
  rule 6 gained the M01 bullet: a task whose acceptance includes anything the
  player sees or hears in M01 is only done when this smoke passes on the
  reviewed commit, with its screenshots cited in the review notes.

## Limits (each names the affected content and the task that resolves it)

* **The actor probes depend on #1538's parenting**: each spawned actor's
  drawn model hangs off its own entity (the aircraft's `planes.zbd` parts, the
  zeppelin's world-node visual), and the probe keeps exactly that entity while
  hiding every other actor body. If that parenting ever moves elsewhere, the
  probe's keep/hide handle must follow (`CensusEntry::drawn_entity`); a member
  with no drawn entity is recorded by name and counted as zero, never assumed
  visible.
* **No sea measurement exists for c1c**: its records declare no water surface,
  so `world_sea_visible` is skipped by name. The drawn sea plane is `#1434`'s
  designed one and its appearance is unmeasured (F19-C / #636 own the
  binding).
* **The objective wake is measured, not explained**: `hud_objective_current`
  failing after the #1486 approach is a fact about this run; which record
  refuses to wake from that placement is the objective work's to find
  (M01-HUD-OBJECTIVE #1462 and the trigger flow it reads).
* **The smoke never ran an original executable** and is never `human_play`;
  nothing here is `verified_original`, and every number above is this
  machine's own frames at the commit this finding was committed with.
* The floors — 20 per mille local detail and 24 distinct colours for
  *textured*, 24 near-field luminance levels for *lit*, 8 actor pixels, 64
  objective-text pixels, 100 per mille sea — are **designed** and documented
  beside their constants with the measured basis: this run's own flat,
  untextured, unlit terrain measures **1 per mille** detail and **1**
  near-field spread, while the player's textured airframe in the same run
  measures **233 per mille**. They separate the state the owner reported from
  a drawn one; they are not measurements of the original renderer (F17/F18 own
  that).

## Reproduce

```sh
cargo build --release --locked -p cs_app --bin cs         # the windowed half runs it
cargo test -p cs_app --locked --test accept_m01_owner_smoke   # CI-safe half
CS_M01_OWNER_SMOKE_OUT=private/evidence/M01-OWNER-SMOKE \
cargo test -p cs_app --locked --test accept_m01_owner_smoke -- --include-ignored
```

The ignored test runs the windowed binary exactly as the owner does (`CS_MISSION_BIN`
overrides which one; it prefers `target/release/cs`, because debug Bevy steps a
2 700-frame script far too slowly) and asserts the harness contract: every
check id judged, the PNGs delivered, and a non-zero exit exactly when a check
failed.

The task's evidence report is `docs/findings/evidence/M01-OWNER-SMOKE.json`,
written by `crates/cs_app/tests/evidence_report_m01_owner_smoke.rs` from the
recorded acceptance log, `report.json` and the hashes of all 21 artifacts, and
validated with `tools/validate_evidence.py --require-pass`. Its `review.method`
names the three checks the run currently fails, so the machine-readable record
carries them too.
