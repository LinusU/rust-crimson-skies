# VS-M01-RUNTIME: M01's launch closure, measured

Date: 2026-10-04. Task: #359 / `VS-M01-RUNTIME` "Wire one original mission
into the playable application" on
`rally/359-wire-one-original-mission-into-the-playa`. Shared contract:
`docs/contracts/SCRIPT-MISSION.md` and `docs/contracts/CLI-EVIDENCE.md`.
Capability used: **`retail`** — the original installation at `$CS_GAME_DIR`
was read through production code. `gpu` and `audio` were available and not
used: nothing is rendered or played, and no original run happened, so
nothing here is `verified_original`.

## The verdict first

**M01 cannot launch, and the closure says exactly why.** The task's own rule
applies: "the recorded per-mission dependency closure is mandatory … file
the exact missing work and block integration until it is implemented." The
measurement below is that recording: of the eleven surfaces the launch
enumerates, two are satisfied (`world_textures`, `shared_aircraft`), six are
unsupported on named missing mechanisms, and three are unknown.

## What was added

- `crates/cs_app/src/mission_launch.rs` (new, an owner path): the launch
  plan — `plan_mission_launch(install_root, label, discovery_title)` binds
  the work order through `SourceContext::bind` exactly as M01-A's binding
  does, then walks eleven `LaunchSurface`s and produces one
  `SurfaceReport` per surface: every archive examined (logical key +
  SHA-256 + dispatched family), every reader member's per-member verdict
  (`decode_zrd` document, opaque, or refused verbatim), and a
  `SurfaceVerdict` — `Satisfied { consumer }`, `Unsupported { mechanism,
  detail }` or `Unknown { detail }`. `MissionLaunchPlan::launchable` is the
  gate the `--mission` runner will read; `gaps()` lists the blocking
  surfaces in report order.
- `crates/cs_app/tests/campaign/vs_m01_runtime.rs` (new): three
  `accept_vs_m01_runtime_*` tests — the undiscoverable-installation refusal,
  the gate's gap reporting, and the ignored retail census that produced the
  table below.
- `crates/cs_app/src/lib.rs` (wiring only): the module declaration and its
  doc paragraph.

## The measured closure (install `b4e780ab…`, `zbd/c1c/m01`)

`plan_mission_launch` resolves `mission/ch1-m01`, `world/c1c`,
`script/c1c-m01-zrdr` from the campaign binding — never the discovery title
— and reports:

| surface | verdict |
| --- | --- |
| `world_geometry` | **unsupported** — `world-container scene conversion`: `node 553 cannot form a scene id: content id key is 135 bytes, max is 128` (the hierarchy itself reconciles: 293 of 5 644 records name a parent that does not list them, `PartialChildIndex { omitted: 293, parents: 1, child_slots: 5202 }`) |
| `world_textures` | **satisfied** — six archives (`texture.zbd`, `rtexture{2,4,6,8,10}.zbd`); F08 decode + F17-B upload adapters |
| `shared_aircraft` | **satisfied** — `zbd/planes.zbd` converts through `scene_graph_from_gamez` (3 317 nodes) |
| `player_configuration` | **unknown** — initial player/wingmate assignment is not bound from original data (a recorded M01-A unknown) |
| `mission_program` | **unsupported** — `mission-language semantics` |
| `mission_objectives` | **unsupported** — `objective-record recovery` |
| `world_actors` | **unsupported** — `world-actor program semantics` |
| `mission_animations` | **unsupported** — `mission-animation payload semantics` (`mis_anim.zbd` dispatches `Animation`) |
| `camera_animations` | **unsupported** — `camera-animation payload semantics` (`cam_anim.zbd` dispatches `Animation`) |
| `mission_audio` | **unsupported** — `audible output device` (`zbd/soundsh.zbd`, `zbd/soundsl.zbd` dispatch `Sound`) |
| `mission_environment` | **unknown** — `weather.zrd` decodes; which fields bind an `EnvironmentSession` is unmeasured |

### The mission reader archive, member by member

`zbd/c1c/m01/zrdr.zbd` (91 171 B, the archive M01-A fingerprinted) lists
**12 members; every one is a `.zrd` document and all 12 decode cleanly
through `cs_content::stunts::decode_zrd`**:

| member | bytes | note |
| --- | --- | --- |
| `aiv.zrd` | 9 869 | a `MISSION_CONTROL_MEMBERS` control member |
| `egen.zrd` | 1 300 | |
| `location.zrd` | 378 | |
| `map.zrd` | 652 | |
| `mis_anim.zrd` | 2 173 | mission-animation references |
| `net.zrd` | 336 | |
| `objectives.zrd` | 24 012 | the authored objective records (a control member) |
| `startanims.zrd` | 298 | startup-animation references |
| `weather.zrd` | 3 429 | |
| `zeppelins.zrd` | 6 766 | mission-specific: zeppelin declarations |
| `placezeps.zrd` | 1 535 | mission-specific: zeppelin placement |
| `wv_tailhook.zrd` | 38 639 | mission-specific: the largest member |

Notably `targets.zrd` is **absent** — consistent with F39-E4's census. The
mission's zeppelin/docking content is plain in the names but no field-level
semantics are claimed here.

The world-actor closure lists `zbd/c1c/zrdr.zbd` (28 members, all documents)
and the shared `zbd/zrdr.zbd` (**221 members, all documents** — the shared
aircraft/zeppelin/prop animation programs: `player.zrd`, `wingman.zrd`, the
`beowulf`/`bhat`/`dante`/`gem`/`vostok` zeppelin destruct chains, `aa_gun`,
`balloon_*`, `muzzle_burst`, …). Every member of all three archives decodes;
none of them has a runtime consumer.

## What the closure says is missing (the filed work)

Each `Unsupported` mechanism is a mechanism the launch literally requires:

1. **World-container scene conversion.** `SceneGraph::build` refuses the
   c1c container at node 553 — a stored name-path that escapes into a
   `scene_node` key 135 bytes long, over the 128-byte key bound — and the
   F18-A0/F11-E1 findings record the second refusal class behind it:
   duplicate name-paths the store actually carries (1 106 measured for c1c).
   Both verdict classes need a measured rule before any world becomes a
   `SceneGraph`.
2. **Original world import.** Even with a convertible graph, nothing turns
   an original world container into a `WorldDefinition` — the sector
   partition, object population and per-object collision roles are
   unmeasured (F18-B left them unmeasured by design).
3. **Mission-language semantics.** No production adapter emits a
   `RawProgram`/`MissionProgram` from an original reader member; F13-C
   measured 0 of 1 452 programs resolved and the signature table ships
   empty. Until the control members' field→effect semantics are measured,
   `objectives.zrd`, `wv_tailhook.zrd` and friends are documents, not a
   program — `docs/contracts/SCRIPT-MISSION.md` keeps this `Unsupported`.
4. **Objective-record recovery.** `lower_program` refuses
   `DeclaredSupport::Original`; no measured recovery maps `objectives.zrd`
   records onto a `DeclaredObjectiveProgram` (F39-E* has measured
   vocabulary, not semantics).
5. **World-actor program semantics.** `DeclaredWorldActorProgram` is
   declared-designed; which original `.zrd` member drives which actor —
   including the shared zeppelin/destruct chains M01's content plainly
   references — is unmeasured (the F34 census measured coverage, not
   semantics).
6. **Animation-carrier payload semantics.** `mis_anim.zbd` and
   `cam_anim.zbd` dispatch as `Animation` archives; no production consumer
   plays their members.
7. **Player configuration binding.** Which airframe M01 assigns the player
   (and wingmate) lives in the mission program — unbound until the program
   decodes; `planes.zbd` itself converts.
8. **Audible output device.** The sound archives dispatch and decode, and
   the session machinery exists (`AudioPlugin`, `RadioQueue`,
   `MusicDirector`), but the only production `AudioDevice` is a recorder —
   nothing makes sound.
9. **Environment/weather binding.** `weather.zrd` decodes; which fields
   bind an `EnvironmentSession` is unmeasured.

Separately, the launch's own application-side gaps (not content mechanisms):
there is **no windowed run path** anywhere in the codebase — every `App` is
`MinimalPlugins` headless or an offscreen capture — and no Bevy-side
producers feed `DeviceEvent`s into `InputSession`, project
`CameraSession::frame` onto a `Camera3d`, or bind an `EnvironmentSession` to
lights/sky. These stay inside this task's scope rather than being filed.

## Why the launch is refused rather than reduced

The task's hard acceptance — "original-data launch reaches a controllable
visible scene … authored objectives and success/failure" — needs the
actual c1c world placed and the mission program's own spawn/objective
statements. `SceneGraph::build` refusing the world container and the
program surface being `Unsupported` are exactly the task's
"unknown reachable instructions / missing gameplay assets" failure
conditions. A launch that drew the airframe over nothing, or ran a
handwritten objective stand-in, would be the placeholder the contract
forbids. So `plan_mission_launch` reports the gaps and this task is blocked
on the nine filed mechanisms rather than merged with a demo.

## Recorded unknowns

- `SceneGraph::build` stops at the first refusal; whether deeper verdicts
  beyond the name-path/duplicate classes wait behind it is unmeasured.
- `mis_anim.zbd`/`cam_anim.zbd` member listings were not walked — the
  `Animation` family is not reader-indexed through the index this census
  uses; their member semantics are unmeasured in either direction.
- Sound member contents were not listed (the census fingerprints and
  dispatches the archives; 128 MiB reads were avoided by a bounded
  64 KiB header probe).
- `wv_tailhook.zrd` is presumed the mission's driving program by name
  shape only; nothing classifies it further.
- Whether `zbd/interp.zbd` and `zbd/rimage.zbd` belong to the launch scope
  is unmeasured; they are not in the surfaces above.

## Commands run

All on `rally/359-wire-one-original-mission-into-the-playa`, Rust 1.98.1,
from the repository root.

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo test --locked -p cs_app --test campaign accept_vs_m01_runtime_ -- --include-ignored` | 0 (3 tests: 2 synthetic + 1 retail) |
| `cargo test --locked -p cs_app --test world accept_f18_a_retail_ -- --ignored` | 0 (re-verified the world-container refusal stands) |

## Sources

`missions/M01.md`, `missions/bindings/M01.json`,
`docs/contracts/SCRIPT-MISSION.md`, `docs/contracts/CLI-EVIDENCE.md`,
`docs/findings/2026-09-29-m01-a-source-binding.md`,
`docs/findings/2026-10-03-f18-world-hierarchy-authority.md`,
`docs/findings/2026-10-03-f11-e1-scene-node-id-escaping.md`,
`docs/findings/2026-09-29-f13-b-locate-and-classify-programs.md`,
`docs/findings/2026-09-29-f13-c-signature-probes-and-reachability.md`,
`docs/findings/2026-09-30-f18-b-world-import-and-static-collision.md`,
`docs/findings/2026-09-30-f18-d-world-group-audit-and-gpu-capture.md`,
and the production readers this census drives
(`cs_formats::zbd::{dispatch, read_version_one_index, read_reader_archive}`,
`cs_formats::gamez::read_gamez_nodes`,
`cs_content::stunts::decode_zrd`,
`cs_content::world::world_scene_graph_from_gamez`,
`cs_content::scene::scene_graph_from_gamez`,
`cs_content::campaign_bindings::SourceContext`).

## Re-measurement after #628–#636 (2026-10-05)

`plan_mission_launch` now judges each surface with the consumer that landed
for it instead of a fixed verdict. Measured on the retail installation
(`cargo test -p cs_app --test campaign accept_vs_m01_runtime_ -- --include-ignored`):

| surface | verdict | why |
| --- | --- | --- |
| `world_geometry` | unknown | `c1c/gamez.zbd` converts and imports to a `WorldDefinition`; the world-vertex unit and the collision role of every unindexed record are unmeasured (`2026-10-04-m01-lc-world-import.md`) |
| `world_textures`, `shared_aircraft` | satisfied | unchanged |
| `mission_environment` | satisfied | `read_mission_weather` binds `weather.zrd` |
| `mission_audio` | satisfied | the audible device landed (#635) |
| `player_configuration` | unknown | the `player` and `wingman_*` records of `aiv.zrd` resolve; the airframe and the start pose are recorded unknowns (`2026-10-05-m01-lc-player-config.md`) |
| `mission_program`, `mission_objectives` | unsupported | the control census leaves lowering requirements unmet: 30 of M01's 43 directive keys are `meaning_not_measured` (`2026-10-04-m01-lc-mission-program.md`) |
| `world_actors`, `mission_animations`, `camera_animations` | unsupported | the members and records are walked and bound as far as measured; no consumer plays them for a mission |

`cs --cs-path <dir> --mission M01` (`mission_launch::launch_mission`) runs the
plan and, because it is not launchable, exits 1 listing every unsatisfied
surface with its mechanism. Nothing is spawned, drawn or played, and nothing
falls back to synthetic content. The windowed composition (reusing the
`--playtest` window, input and chase camera) is deliberately not built: it
would have nothing original to run until the unmeasured player airframe, start
pose and directive meanings exist (AGENTS.md rules 4 and 5).

## Re-measured on main after #675-#678 and #690 (2026-10-06)

`cs --cs-path "$CS_GAME_DIR" --mission M01` (capability `retail`; nothing here
is `verified_original`) exits 1 with 7 of 11 surfaces unsatisfied:

| Surface | Verdict | What is still missing |
| --- | --- | --- |
| world_geometry | unknown | the collision role of the 17 `fvol*` volume records of c1c (134 corpus-wide) and the axis convention are unmeasured; the vertex unit is measured (#677) |
| player_configuration | unknown | the player's airframe is stored nowhere in the mission data read (field 0 is `0xFFFFFFFF` in all 53 missions); the start pose has no measured unit/heading zero (#676) |
| mission_program, mission_objectives | unsupported | the lowering requirements `objective_condition` and `call_arguments` are unmet; 11 directive keys still have no single call signature or a value `cs_script::ir::Value` cannot carry (`ANIM_STATE`, `COMPLETED_STOPPOINT`); no `Lowering` variant carries the measured directive operations |
| world_actors | unsupported | `DeclaredWorldActorProgram` has no original encoding |
| mission_animations, camera_animations | unsupported | the F20 grammar is measured (#690) but no production consumer plays the members for a mission |

Satisfied: world_textures, shared_aircraft, mission_weather, mission_audio.

The windowed mission composition on the `--playtest` window is therefore not
built: there is no original start pose or airframe to put in it, and no
runnable mission program to drive it. No synthetic fallback is offered.

## Re-measured on main after #715–#718 and #727 (2026-10-08)

The owner's 2026-10-08 note asked for a fresh run of
`cs --cs-path "$CS_GAME_DIR" --mission M01` on current main. Two of the
eleven verdicts were **stale wording** and one whole class of verdict was
**restated instead of measured**; this branch fixes all three in
`crates/cs_app/src/mission_launch.rs` (the owner path) and re-measures:

| surface | verdict before | verdict now | why |
| --- | --- | --- | --- |
| `world_geometry` | unknown — "the collision role of the unindexed `fvol*` records and the axis convention are unmeasured" | **unknown**, new question | the unit, the axis map and the fog roles are measured now, so the detail read off `WorldImportReport` names only what is still open |
| `world_textures`, `shared_aircraft` | satisfied | satisfied | unchanged |
| `player_configuration` | unknown (#677 wording) | **unknown**, #715 wording | #715 measured that no campaign mission data, no installation file and no measured directive names the airframe, and that heading zero, handedness and the frame relation were never measured |
| `mission_program`, `mission_objectives` | unsupported | **satisfied** | #717's lowering is complete for M01: `mission_control::survey_mission_control_programs(...).lowering().complete()` |
| `world_actors` | unsupported — "which member drives which actor is unmeasured" | **unsupported**, corrected | #632 measured member → actor and #718 consumes it (this plan reads `bind_mission_animation` and counts 38 world names over 7 startup rows and 3 placements); what is missing is the placed actor's spawn/route — `UnmeasuredFieldFamily::PlacementRecords`, resolving #574 |
| `mission_animations`, `camera_animations` | unsupported — "no production consumer plays its members for a mission" | **satisfied** | #718's `MissionAnimationPlayer` is the consumer; the plan now starts the rows whose record lives in that carrier, through `bind_mission_animation`, and reports how many started |
| `mission_audio`, `mission_environment` | satisfied | satisfied | unchanged |

`cs --cs-path "$CS_GAME_DIR" --mission M01` (capability `retail`, Rust 1.98.1,
`rally/359-wire-one-original-mission-into-the-playa`) now exits 1 with **3 of
11 surfaces unsatisfied**:

1. **`world_geometry` — unknown.** The container imports to a
   `WorldDefinition` (144 partition cells, 346 objects) with the `identity`
   axis map applied as `ObservedTool` under
   `f18-world.world-axis-convention-measured`, and every `fvol*` record is
   classified as fog (#716). What no stage answered: the collision role of
   the **4 grid-named `fvol*` records** of c1c
   (`f18-world.grid-named-fog-volume-role-unmeasured`, #727) and what the
   **1 grid record that stores no mesh index** drew. The verdict is derived
   from the report's own counters, so a world that leaves nothing unanswered
   reports `Satisfied` instead.
2. **`player_configuration` — unknown.** `aiv.zrd` records are read and the
   stored position binds to metres, but the campaign airframe and the pose's
   heading zero, handedness and frame relation are unmeasured
   (`docs/findings/2026-10-06-m01-lc-player-airframe-source.md`, #715).
3. **`world_actors` — unsupported**, mechanism `world-actor program
   semantics`: the join is measured and consumed, the placed actor's spawn
   and route are not (`f20-anim.placement-member-fields-undecoded` on this
   scope's three `placezeps.zrd` placements; resolving tasks #574 and #632's
   field families).

Nothing falls back to synthetic content, nothing is spawned or drawn, and no
windowed composition is built while these three stand (AGENTS.md rules 4 and
5). The residual unknowns are recorded with their claim ids rather than
smoothed into a `Satisfied`, per the owner's 2026-10-04 instruction to keep
this task's fidelity gate strict.

Capabilities: `retail` (the installation read through production code) and
ordinary build/test. `gpu` and `audio` were available and **not used** —
nothing is rendered or played here — so nothing in this re-measurement is
`verified_original`, and no original executable ran (#358).
