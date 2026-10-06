# #718: a mission's animation records and its animation log, consumed

Date: 2026-10-07. Task: #718 (`M01-LC-ACTOR-ANIM-CONSUMERS`), a step
`VS-M01-RUNTIME` (#359) waits on. Feature sheet:
`specs/F20-object-animation-and-authored-destruction-states.md`, stage
`### F20-D` (non-negotiable behaviors 1, 2 and 5). Shared contracts:
`docs/contracts/IDENTITY-CONTENT.md` (session generations) and
`docs/contracts/SCRIPT-MISSION.md` ("Objective event ordering"). Capabilities
used: **`retail`** (read-only access to `$CS_GAME_DIR`, by the one `#[ignore]`d
acceptance test) and ordinary build/test. `gpu` and `audio` were available and
**not used**: nothing is rendered or played and no original run happened, so
nothing here is `verified_original`.

## Files

- `crates/cs_app/src/mission_animations.rs` (new): the mission session's
  consumer — `MissionAnimationPlayer`, `RunningRecord`, `RefusedRecord`,
  `FinishedRecord`, `RecordStatement`, `StartupReport`, `TickReport`,
  `TickAdvanceRefusal`, `PlayerError`, `PlayerTeardown`, and the composed
  `step_mission_animations` with `MissionAnimationStep`,
  `MissionAnimationStepRefusal` and `MissionTickRefusal`.
- `crates/cs_app/src/lib.rs` (wiring only): `pub mod mission_animations;` and
  the module-documentation paragraph beside `mission_markers`.
- `crates/cs_app/tests/accept_t718_mission_animations.rs` (new): the seven
  `accept_t718_*` tests — six synthetic, one `#[ignore = "requires CS_GAME_DIR"]`
  retail.
- `crates/cs_app/tests/evidence_report_m01_lc_actor_anim_consumers.rs` (new):
  the `CLI-EVIDENCE` harness (see **Evidence** below); not named with the
  acceptance prefix, so it never pads the task selection.
- This file.

**No reader refusal was weakened.** The diff touches no line of
`crates/cs_formats`, `crates/cs_types`, `crates/cs_assets`, `cs_content`,
`cs_sim` or of `cs_app::animation`: the join, the event grammar and the marker
consumer are consumed exactly as #632, #678, #690 and #507 left them, and every
refusal this module can make is one of those modules' own refusals carried
forward (`PlayRefusal`, `PlaybackGap`, `TickAdvanceRefusal`'s rule copied from
`AnimationPlayback::advanced_through`).

## The gap this closes

Three measured halves had no consumer:

| half | measured by | what was still missing |
| --- | --- | --- |
| which `zrdr` member drives which actor (the member → actor binding) | #632 (`programs`), #678 (`mission::WorldActorPlacement`) | nothing consumed the binding at runtime |
| one mission scope's `mis_anim.zbd` / `cam_anim.zbd` records, joined and refused | #678 (`bind_mission_animation`) | nothing started, advanced or finished a record |
| each record's event grammar, duration and per-tick statements | #690 (`events`, `mission::RecordPlayback`) | a timeline to run them on |
| the animation log's markers reaching the mission runtime | #507 (`mission_markers`) | a composed tick a host could call once |

This stage is that consumer. `MissionAnimationPlayer::start` takes the rows of
one startup event, keeps the ones the join refused with their refusals and
starts the rest; `advance` runs them **once per committed tick** on the caller's
timeline and publishes each statement the first time its measured `START_TIME`
is reached; `step_mission_animations` is the one call that does that *and*
drains the `AnimationLog` into `ObjectiveSession` for the same tick.

## What "plays" means here

A record's event stream stores **statements** — an opcode, its authored target
fields and its timing — and no keyframes, measured over all 56 994 retail
blocks by #690. So a running record publishes `RecordStatement`s: the
installation's own spelling (`OBJECT_MOTION_FROM_TO`), this project's class,
the `[start_time, end_time]` span in the original's stored time unit, the
sequence block it belongs to, **the archive and member that declared the
animation**, and the session generation. That is the member → actor binding in
motion — the same numbers #632 and #678 reported, now readable per tick from a
running session.

It is **not** a transform. No `PoseSample`, no node pose and no world position
is produced from an event; the claim that stands in for one is
`f20-anim.event-pose-transform-not-decoded` (#690's AC3 owner ruling), and a
record's statements are not converted into `MarkerEffect`s either: a cue label
nobody authored would be an invented mission symbol, which `mission_markers`
refuses even for the designed vocabulary.

## The composed step and its two refusals

`step_mission_animations(world, player, markers, objectives, facts)` advances
the record player first and then calls `step_mission_with_markers`. The order
is the retry contract, and each refusal says which half ran:

| refusal | half that ran | what the host does next |
| --- | --- | --- |
| `Records(TickAdvanceRefusal::NotAfter { .. })` | neither | nothing was applied — the player was already through the tick; the log was **not** drained (asserted) |
| `Mission(Box<MissionTickRefusal>)` | the record half only | retry the **mission** half with the delivery the inner refusal carries (`step_mission_with_markers` documents this); do not offer this tick to the player again — the record report is carried in the box so the tick's statements are not lost |

A repeated committed tick publishes nothing at all (the same rule as
`AnimationPlayback::advanced_through`), and a statement can only be published
once per activation, so a retry cannot duplicate a mission event (F20
non-negotiable behavior 5).

## Design decisions

- **Statements, not transforms, and a refusal for the rest.** The one tempting
  wrong turn was to lower a record into an `AnimationClip` and hand the
  renderer a pose nobody measured. `MissionAnimationPlayer` has no such path:
  it reports timing and spelling, and the transform claim stays on
  `POSE_TRANSFORM_NOT_DECODED_CLAIM`.
- **The caller states `ticks_per_second`.** The original's animation tick rate
  is unmeasured (`f20-anim.tick-rate-unmeasured`), so the player maps stored
  time onto the **host's** timeline and never claims the two are one clock —
  the rule `RecordPlayback::poses` already states. A rate of zero is refused
  at construction rather than producing a player that silently publishes
  nothing.
- **One activation per live identity, a new one after a finish.** Starting an
  identity that is already running is reported as `already_running` and does
  not touch the live timeline; a record that finished may be started again as
  a fresh activation (a fresh timeline, the finished row still on record).
  `retry(served)` refuses the generation already served and leaves the live
  timeline untouched, exactly like `MissionMarkerConsumer::retry`.
- **A refused row keeps every refusal and its span.** `RefusedRecord` carries
  the `PlayRefusal` list verbatim, the record's `SourceSpan` when a record was
  bound, and the claim ids the refusals carry — a content mismatch carries
  none, and that absence is part of the answer.
- **The player never spawns.** The mission's `placezeps.zrd` placements are
  reported by `bind_mission_animation` with their identity measured and their
  placement refused under `f20-anim.placement-member-fields-undecoded`; this
  module reads no position, heading or motion limit out of them.
- **Plain struct, host-owned, like `mission_markers`.** Not a Bevy resource:
  the session it drives (`ObjectiveSession`, the marker consumer) is
  host-owned, and the one `World` the composed step touches is the log's.

## Test inventory

| `accept_t718_` test | Covers | Fails when |
| --- | --- | --- |
| `a_playable_record_starts_advances_once_and_finishes` | **the observable failure**: a played record's declaring archive/member/carrier/index/span, the measured 2.5 duration, statements at stored times 0.5 and 1.5 on the exact ticks that reach them, one publication per statement, a repeated tick refused with nothing published, finishing **after** its last statement, a restart after a finish, a repeated start not becoming a second activation | a record starts without its binding, a statement is published twice or on the wrong tick, a record never finishes (or finishes before its statements), a repeated start silently replaces a live timeline |
| `a_row_the_join_refused_is_never_started` | both refusal shapes — an undecoded event stream with `f20-anim.sequence-event-stream-not-decoded` and its source span, and an object-name disagreement with **no** claim id — kept in stored order, and 64 advancing ticks publishing nothing for them | a refused row is started, the refusals or their claim ids are dropped, or a refused row's bytes reach the timeline |
| `a_zero_tick_rate_and_a_repeated_generation_are_refused` | `PlayerError::ZeroTickRate`, `PlayerError::SameSession` leaving the live timeline untouched, `PlayerTeardown`'s counts and the new generation's timeline starting over | a zero rate builds a player, or a retry re-arms the generation it already serves |
| `the_composed_step_raises_the_animation_log_and_advances_the_records` | one call doing both halves: the fired door marker drained once into a real `ObjectiveSession` (the fixture program's reveal rule fires), the log empty afterwards, and the record's statement arriving on the tick that reaches stored time 0.5 | either half stops running in the composed step, or the log is drained twice |
| `a_refused_mission_tick_carries_the_record_report` | the **order**: the mission half refuses a tick it already stepped, and `advanced_through == Some(Tick(1))` proves the record half ran first; the carried report and the already-drained log | the halves are reordered, or the record report is lost when the mission refuses |
| `a_player_ahead_of_the_mission_refuses_before_the_mission_runs` | the other refusal: `Records(NotAfter)` before the mission runs, the log still holding its marker and no marker admitted | a refused tick still drains the log |
| `retail_m01_startup_animations_play_in_the_mission_session` (retail, `#[ignore]`) | M01's six `NEW_GAME_START` rows all start and all finish; the declaring archive/member of `pzep_engines_start` (`zbd/zrdr.zbd` :: `pirate_zep_nacelles.zrd`) and of `wv_hookup_state` (`zbd/c1c/m01/zrdr.zbd` :: `wv_tailhook.zrd`); both carriers represented; every statement stamped with the session and a member; every finished duration equal to the record's own measured duration; `LOAD_GAME_START`'s single `player_setup` row; and the three `placezeps.zrd` placements still refused to place under their claim | a member moves, a row stops decoding, a duration is read from somewhere else, or a placement starts being spawned |

## Sensitivity

Four mutations were applied to `mission_animations.rs` and the synthetic
selection (`-- --skip retail`) re-run with the source restored afterwards. All
four are killed by a test CI can run, so none relies on the `#[ignore]`d case:

| mutation | killed by |
| --- | --- |
| the per-activation guard is removed, so a statement is published on every tick that reaches it | `a_playable_record_starts_advances_once_and_finishes` (the 196-tick advance reports two statements instead of one) |
| `if !row.is_playable()` is never taken, so a refused row is started | `a_row_the_join_refused_is_never_started` (a started row and a missing refusal) |
| the completion test becomes strict (`time > duration`), so a record never finishes | `a_playable_record_starts_advances_once_and_finishes` (nothing is finished) |
| the composed step runs the mission half first | `a_refused_mission_tick_carries_the_record_report` (the player never advanced) **and** `a_player_ahead_of_the_mission_refuses_before_the_mission_runs` (the log is drained by a refused tick) |

The behaviours only the retail case checks are M01's own numbers: the six
`NEW_GAME_START` rows, the two declaring archive/member pairs, both carriers,
every finished duration equal to the record's measured duration, and the three
`placezeps.zrd` placements under their claim; every **rule** above is covered
without the installation.

## Unknowns and limitations (recorded, not guessed)

- **No transform, no node motion, no world position.** The statements are the
  record's measured activity; `f20-anim.event-pose-transform-not-decoded` still
  stands. **Affected content:** every claim that M01's startup animations move
  anything on screen. **Resolving task:** the pose-transform half of #690's
  AC3, which needs an original run or the original's own source.
- **`DeclaredWorldActorProgram` still has no original encoding.** The member →
  actor *binding* is measured (#632, #678) and this stage consumes it, but the
  declared world-actor schema still has no measured source for motion, socket,
  pickup, tick rate or faction, and the placement member's
  `node`/`position`/`yaw`/`pitch`/`max_speed`/`max_accel` fields stay
  `UnmeasuredFieldFamily::PlacementRecords`
  (`f20-anim.placement-member-fields-undecoded`). **Affected content:** the
  `world_actors` launch surface of `MissionLaunchPlan`, and every placed
  actor's pose and route in M01. **Resolving tasks:** #574 (decode the
  mission-scoped `zeppelins.zrd` placed-traffic carrier) and the seventeen
  field families #632 named.
- **The original's tick rate and the stored time unit stay unmeasured**
  (`f20-anim.tick-rate-unmeasured`, and #436's stored unit), so the player
  reports durations in the original's own unit and converts them with a rate
  the **host** stated. **Affected content:** any claim that a statement
  happened "at 3.2 seconds".
- **No statement became a mission signal.** The original marker encoding is
  undecoded; `MarkerEffect` stays a designed vocabulary and no cue label is
  invented here. **Affected content:** gameplay transitions triggered by an
  original animation.
- **Only M01 is measured by the retail case**, the same scope #678 settled.
  **Affected content:** the other 52 mission scopes. **Resolving task:** F50.
- **No production mission host owns this yet.** The composed step is a function
  the mission host calls; `VS-M01-RUNTIME` (#359) owns the windowed mission
  session (`crates/cs_app/src/mission_session/`) that will call it, and the
  `--mission` runner stays gated on `MissionLaunchPlan::launchable`.
- **No original run.** Every number above is read out of the original bytes by
  the production readers: `ClaimStatus::ObservedTool`, never
  `verified_original`. `retail` is file access, not evidence of runtime
  behaviour.
- **Nothing derived from the original bytes is committed** beyond counts,
  member names and relations a reviewer needs to check the binding; no member
  payload and no screenshot is in the repository.

## Evidence

`crates/cs_app/tests/evidence_report_m01_lc_actor_anim_consumers.rs` is the
`CLI-EVIDENCE` harness: it reads the recorded acceptance log, takes the
install and content fingerprints from production discovery, and performs a
**second production observation** — it binds M01 through
`bind_mission_animation`, starts every startup row in `MissionAnimationPlayer`
and advances the timeline to its measured end, recording per row the declaring
archive and member, the carrier and record index, the measured duration, the
statements published and the finished tick, plus the three placements with
their claim (`private/evidence/M01-LC-ACTOR-ANIM-CONSUMERS/actor-anim-consumers.json`).
The report is `private/evidence/M01-LC-ACTOR-ANIM-CONSUMERS/acceptance.json`,
validated with `tools/validate_evidence.py --require-pass`, and its committed
copy is `docs/findings/evidence/M01-LC-ACTOR-ANIM-CONSUMERS.json`. The claim
is `implemented`: nothing here is `verified_original`.

## Commands run

```sh
cargo fmt --all -- --check                                   # exit 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
                                                             # exit 0
cargo test --workspace --locked                              # exit 0 (413 test binaries)
#   The first run of this suite exited 101 on exactly one test,
#   accept_t696_the_workspaces_test_binaries_are_measured_not_guessed: the
#   evidence harness was added to the package while that run's build phase was
#   already complete, so the plan held a test source with no binary in
#   target/debug/deps ("a workspace-wide build must measure every target in the
#   plan", left: 1, right: 0). Nothing in the suite was weakened: the rerun
#   after the target existed is green, and that rerun is the run of record.
cargo test --workspace --locked -- accept_t718_ --include-ignored
#   exit 0 => 7 tests discovered and executed, 7 passed, 0 failed, 0 ignored
#   (6 synthetic + 1 retail, about 75 s; the retail case does production
#   discovery passes over the installation)
python3 tools/validate_evidence.py \
  private/evidence/M01-LC-ACTOR-ANIM-CONSUMERS/acceptance.json \
  --artifact-root private/evidence/M01-LC-ACTOR-ANIM-CONSUMERS --require-pass
#   => structurally_valid: true, artifact_count: 2
```

## Sources used

- `crates/cs_app/src/animation/mission.rs` (#678's join, `PlayRefusal`,
  `RecordPlayback`, `WorldActorPlacement`), `crates/cs_app/src/animation/events.rs`
  (#690's grammar and opcode table) and `crates/cs_app/src/animation/programs.rs`
  (#632's declaration reader).
- `crates/cs_app/src/mission_markers.rs` (#507's log consumer) and
  `crates/cs_app/src/objectives.rs` (`ObjectiveSession`), which the composed
  step reuses rather than parallel.
- `docs/findings/2026-10-04-m01-lc-world-actors.md` (#632),
  `docs/findings/2026-10-05-m01-lc-actor-anim-playback.md` (#678) and
  `docs/findings/2026-10-06-f20-event-grammar.md` (#690).
- `docs/contracts/IDENTITY-CONTENT.md`, `docs/contracts/SCRIPT-MISSION.md`.
