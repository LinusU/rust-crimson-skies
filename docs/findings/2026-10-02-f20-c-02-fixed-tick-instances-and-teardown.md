# F20-C.02: fixed-tick placement, instance identity, instance teardown

Date: 2026-10-02. Task: F20-C.02 "Place the animation advance on the
fixed-tick schedule, give bindings instance identity, tear down instances"
(#418). Spec:
`specs/F20-object-animation-and-authored-destruction-states.md`, section
`### F20-C`, non-negotiable behaviors 1 and 3. Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: ordinary build/test only — no `CS_GAME_DIR` read, no
render, no audio, so no `private/evidence/` report is produced.

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/animation/schedule.rs` (**new**): the producer wiring —
  `CommittedSessionTick` (the stamp the session driver writes),
  `advance_animation_on_session_tick` (the fixed-tick entry, also directly
  callable), `AnimationSchedulePlugin` (`FixedPostUpdate`, after
  `PhysicsSystems::StepSimulation`) and `release_superseded_instances` (the
  teardown rule for a scene generation the load path superseded).
- `crates/cs_app/src/animation/playback.rs` (extended): `AnimationInstance`
  identity in the live map, `InstanceKey`, the instance-qualified queries,
  the `advanced_through`/`advances` schedule bookkeeping, the instance
  parameter on `play_animation` / `stop_animation`, and `teardown_instance`
  (release this instance's applied values, for its entities only).
- `crates/cs_app/src/animation/attachment.rs` (extended):
  `release_animated_attachment` — the one-entity release the teardown and the
  subtree release share.
- `crates/cs_app/src/animation/mod.rs` (wiring only): the `schedule` module
  declaration, its re-exports, `AnimationInstance` and the new
  `AnimatedNodeBinding::instance` field.
- `crates/cs_app/tests/accept_f20_c_02_fixed_tick_instances_and_teardown.rs`
  (**new**): the `accept_f20_c_02_*` acceptance tests.
- `crates/cs_app/tests/accept_f20_b_animation_playback.rs`,
  `crates/cs_app/tests/accept_f20_c_01_attachment_hierarchy.rs`,
  `crates/cs_app/tests/accept_f20_a_animation_boundary.rs` (call sites only:
  the new instance argument / field; see "Expectations this slice changed").
- `crates/cs_app/src/lib.rs` (wiring only): the `### F20-C` doc paragraph.
- This file.

**One observable failure:** nothing in the crate called
`advance_animation`, so the whole playback was unreachable from a running
session: F20-B's boundary 3 and F20-C.01's follow-up both recorded "no in-game
tick reaches the animation path". A world stepped by the real
`FixedPostUpdate` loop (the `SyntheticScene` harness) publishes no marker, and
applies no `NodeAnimatedPose`, however many ticks it runs — measured by
`accept_f20_c_02_the_fixed_tick_advance_runs_once_per_committed_session_tick`,
which fails on a playback whose `advances()` stays `0`.

The second failure is identity: `AnimationPlayback` was keyed by
`animation_track` alone, so two aircraft could not both spin a propeller track
— the second `play_animation` was refused and there was no way to name a
second instance. The third is teardown: a stopped instance left its
`NodeAnimatedPose` / `NodeAnimatedMaterial` / `NodeAnimatedAttachment` /
`AnimatedNodeBinding` on the entities forever, with no caller able to name the
state that instance owned.

## Designed decisions (this feature is designed, not original-verified)

The original `mis_anim.zbd` / `cam_anim.zbd` layouts are still undecoded (F13):
every rule below is **designed**, and no original behavior is claimed. F20-D
keeps the original-family validation gate.

1. **One clock, one stamp.** `CommittedSessionTick(Tick)` is a *stamp*, not a
   clock: nothing in this module ever advances it, and its only writer is the
   session driver that commits a fixed tick (F20-A finding 5: "`advance_to` is
   the only clock face … the caller supplies the session `Tick`"). A second
   clock authority is not introduced: `Time<Fixed>` stays the physics
   adapter's and the session's, and this module never reads it. A world with
   no `CommittedSessionTick` advances nothing at all — the same "no session, no
   animation" rule `play_animation` already enforces.
2. **Advance after the physics step.** The system is registered in
   `FixedPostUpdate` `.after(PhysicsSystems::StepSimulation)` — the F23-A
   adapter's measured hook, and the same place the contact reporter and the
   spawn-tick crossing delivery sit, so a marker fires on the tick whose
   physics has already produced the poses it refers to. **The original order
   is unmeasured** (F20-A's unknowns: "whether marker firing ran before or
   after physics"); this is a designed choice, and the F20-D probe is where it
   gets measured.
3. **One advance per committed tick change, and what a repeat does.**
   `advance_animation_on_session_tick` forwards the stamp to
   `advance_animation` only when it **differs** from the tick the playback was
   last advanced to (`AnimationPlayback::advanced_through`, recorded by
   `advance_animation` itself, so a direct call and the schedule agree). A
   repeated tick is not forwarded at all: no pass runs, nothing is published
   and no component is written. A tick that goes **backwards** *is* forwarded,
   so `AnimationRefusal::Held` can be published — the existing F20-B rule
   still holds the head where it is, changes nothing and reports once per
   occurrence; the forwarded tick becomes the new `advanced_through`, and the
   next repeated tick is therefore a repeat and is not forwarded again.
4. **`AnimationInstance` is the identity a binding names.** A validated
   nonzero `u32` (mirroring `SessionId`/`PeerId` in
   `docs/contracts/IDENTITY-CONTENT.md`: zero never names a live thing). The
   spawn wiring assigns one per animated node it spawns — one per aircraft
   propeller, not one per track. The live map is keyed by
   `InstanceKey { clip, instance }`, so:
   - two bound entities of one track each receive their **own** evaluated
     state, and each instance's event ids carry its own producer serial, so no
     two events of one tick collide;
   - `play_animation` refuses a **second instance of the same identity** with
     `AnimationPlayError::AlreadyPlaying` (now naming the instance), and never
     replaces a live instance; a *different* identity is a different live
     instance and starts normally.
   Query methods that used to name a track now name an instance
   (`is_playing`, `time`, `generation`, `drives`, `producer`, `is_finished`);
   `len()` counts live **instances** across all tracks and `playing()` yields
   `(track, instance)` keys in stable order.
5. **Teardown is per instance, and it releases before it clears.**
   `stop_animation(world, clip, instance)` ends that instance *and* releases
   what it applied, for the entities whose `AnimatedNodeBinding` names exactly
   that `(clip, instance)`: the animation-managed `ChildOf` link first (through
   F20-C.01's release rule, so a detach inherits the departing parent's
   velocity exactly as an authored detach does), then `NodeAnimatedPose`,
   `NodeAnimatedMaterial`, `NodeAnimatedAttachment`, the consumer's
   `AppliedAttachment` / `RefusedAttachment` bookkeeping and the
   `AnimatedNodeBinding` itself. Nothing else is touched: another instance of
   the same track, an entity bound to a different track, and an entity that
   carries an animated component with no binding all keep their state. The
   instance is removed from the map **before** the release, so the next
   advance cannot re-attach what the release unparented (F20-C.01's recorded
   requirement that a release and the despawn happen in one step).
6. **A superseded scene load releases the same way.**
   `release_superseded_instances(world)` stops every live instance whose
   `SceneGeneration` is not `SceneGenerations::latest()` (the scene load
   path's own monotone counter — generations only count up, so "older" is
   "not the latest") and releases what each of them applied. It is a
   directly callable entry, **not** a system: the load path must call it in the
   same step in which it despawns the superseded scene, together with
   `release_attachments_before_despawn` (non-negotiable behavior 4), and a
   fixed-tick system could not be ordered against a despawn that happens in
   `Update`. A world with **no** `SceneGenerations` resource releases nothing:
   without the counter there is no evidence that any generation was
   superseded, and tearing every instance down on absent evidence would be a
   guess.
7. **Retry works because the teardown is complete.** Playing `(clip,
   instance)` again after a stop starts a fresh evaluator with a fresh
   producer serial, so its one-shot gameplay marker fires again exactly once
   and its event ids cannot collide with the earlier activation's.
8. **A refusal names the instance, not just the track.**
   `AnimationRefusal::Held` gained an `instance` field. One track played by
   two instances reports two holds, and a record that named only the track
   could not tell them apart — the same instance-identity requirement the live
   map key and `AlreadyPlaying` follow. The F20-B hold test's expectation grew
   the field; the assertion it makes (one hold, from 10 to 3, re-reported
   never) is unchanged.
9. **A teardown propagates its release's refusals.** Everything
   `release_animated_attachment` could not measure is appended to the
   `AnimationLog` by the teardown, one `AttachmentRecord` per transition.
   Dropping them would repeat the silent-branch defect F20-C.01's second
   review pass removed from the release walk: a hierarchy change plus a
   missing inheritance with nothing on record.

## What is still not wired, and who owns it

`AnimationSchedulePlugin` and `release_superseded_instances` have **no
production caller in this crate**: the composition seam
(`PhysicsSession::configure` in `crates/cs_app/src/physics/session.rs`) is
read-only for this task, so the plugin is registered by the acceptance tests
against the real Avian `App` that `cs_app::synthetic::SyntheticScene` builds.
The session driver that writes `CommittedSessionTick` does not exist yet
either. Both are the F20-C spawn/despawn wiring (`#75`), and are recorded as a
note on that task.

## Expectations this slice changed in the earlier tests

Call-site only, plus exactly one expectation that encoded the *old* rule:

- `accept_f20_b_animation_playback.rs` and
  `accept_f20_c_01_attachment_hierarchy.rs` pass an
  `AnimationInstance` to `play_animation` / `stop_animation`, put it in every
  `AnimatedNodeBinding` they build, and pass it to the instance-qualified
  playback queries. No discriminating assertion changed: the one-shot marker
  counts, the verified-binding rejections, the hold/refusal behavior and every
  hierarchy/velocity assertion are byte-identical.
- `accept_f20_b_play_requires_a_session_and_refuses_a_second_instance` last
  asserted that *"the applied state stays with its entity: teardown is the
  owner's work (F20-C)"*. That is the rule this task replaces, so it now
  asserts the opposite (the applied state and the binding are gone after a
  stop), which is what `accept_f20_c_02_stopping_one_instance_releases_only_its_own_entities`
  pins in the multi-instance case. No other F20-B expectation encodes it.
- `accept_f20_a_animation_boundary.rs` fills the new binding field only.
- `accept_f20_b_holding_the_head_never_replays_a_one_shot_marker`'s expected
  `AnimationRefusal::Held` literal grew the new `instance` field (decision 8).
  The assertion it makes is unchanged.

## Tests

| test | what it pins |
| --- | --- |
| `accept_f20_c_02_the_fixed_tick_advance_runs_once_per_committed_session_tick` (minimum) | the plugin's system in the **real** Avian `FixedPostUpdate` loop: one advance per fixed tick, a repeated stamp advances nothing (`advances()` unchanged, log unchanged), a changed stamp advances exactly once, and a world with no `CommittedSessionTick` never advances at all |
| `accept_f20_c_02_the_schedule_advances_nothing_without_a_committed_tick` | the same loop with no driver: the plugin alone advances nothing, applies nothing and publishes nothing, however many fixed ticks run |
| `accept_f20_c_02_a_repeated_or_reversed_stamp_publishes_nothing_new` | the repeat rule and the hold rule on the schedule path: a repeat publishes nothing, a backwards stamp is forwarded and publishes exactly one `AnimationRefusal::Held` while the applied pose stays, a second repeat publishes nothing, and a forward stamp resumes without re-offering the marker |
| `accept_f20_c_02_two_instances_of_one_track_each_drive_their_own_entity` | two entities bound to one playing `animation_track` as two instances: both receive their own pose on every tick, each fires `engine_started` once, and the two instances' event ids differ (distinct producer serials, same session and tick) |
| `accept_f20_c_02_stopping_one_instance_releases_only_its_own_entities` | the teardown: the stopped instance's entity loses its applied values, its released `ChildOf` (with the parent velocity it inherits and its unchanged composed world pose) and its binding; the other instance keeps playing, keeps its hierarchy and keeps swapping its material; a hand-written state on an entity bound to another track survives; the clip's own later detach does not inherit a second time |
| `accept_f20_c_02_a_second_play_of_the_same_identity_is_refused_and_a_new_one_starts_after_a_stop` | `AlreadyPlaying` naming the identity, a different identity playing beside it, and a replay after the stop with a fresh producer serial whose one-shot marker fires once under an id the first activation could not have used |
| `accept_f20_c_02_a_superseded_scene_generation_releases_what_its_instances_applied` | `release_superseded_instances` against `SceneGenerations::latest()`: exactly the older instance is released, the live generation's instance keeps its state, and a second call is a no-op |
| `accept_f20_c_02_the_teardown_reports_a_release_it_could_not_inherit_a_velocity_for` | error propagation through the teardown: a cargo released from a bay that carries no velocity publishes exactly one `AttachmentRecord::VelocityNotInherited { NoVelocitySource }` for the instance it tore down, the surviving instance publishes nothing, and the link is released either way |
| `accept_f20_c_02_the_propeller_fixture_drives_the_production_lowering` | the pose the multi-instance scenarios assert is the one `lower_clip` really produces: the declared fixture lowers to the `cs_sim` runtime twin (same id, duration, channels and markers) rather than to a test-authored clip |

## Mutation probes

Run locally with a rerunnable driver: each probe edited one production file,
ran the full selection `cargo test --workspace --locked -- accept_f20_c_02_
--include-ignored` (exit 101 each time), then restored the file byte for byte.
`grep -rn "MUTATION PROBE" crates/` returns nothing, `git status` shows no
probe edit, and the selection is green again.

| probe | edit | tests that fail (of 9) |
| --- | --- | --- |
| P1 repeat rule off | `if already && false` in `advance_animation_on_session_tick` | `..._the_fixed_tick_advance_runs_once_per_committed_session_tick` (1) |
| P2 the stamp is not required | the missing-`CommittedSessionTick` early return replaced by `unwrap_or_default()` | `..._the_schedule_advances_nothing_without_a_committed_tick` (1) |
| P3 the live map keyed by track alone | `InstanceKey::new` drops its `instance` argument and always keys by instance 1 | `..._two_instances_of_one_track_each_drive_their_own_entity`, `..._stopping_one_instance_releases_only_its_own_entities`, `..._a_second_play_of_the_same_identity_is_refused_...`, `..._a_superseded_scene_generation_releases_what_its_instances_applied` (4) |
| P4 the teardown clears every animated entity | `release_instance` drops its `(clip, instance)` filter | `..._stopping_one_instance_releases_only_its_own_entities`, `..._a_superseded_scene_generation_releases_what_its_instances_applied` (2) |
| P5 the teardown leaves the binding | `release_instance` stops removing `AnimatedNodeBinding` | `..._stopping_one_instance_releases_only_its_own_entities`, `..._a_second_play_of_the_same_identity_is_refused_...`, `..._a_superseded_scene_generation_releases_what_its_instances_applied` (3) |
| P6 the teardown drops the release's records | `release_instance` calls `release_animated_attachment` for its effect and throws the returned records away | `..._the_teardown_reports_a_release_it_could_not_inherit_a_velocity_for` (1) |

## Checks run

- `cargo fmt --all -- --check` — exit 0.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  — exit 0.
- `cargo test --workspace --locked` — exit 0 (246 test binaries/suites ok, 0
  failed; the retail tests stay `#[ignore]`d and are run below with
  `--include-ignored`). The 7 `accept_f20_b_*`, the 8 `accept_f20_c_01_*` and
  the `accept_f20_a_*` tests are unchanged except at the call sites listed
  above, and still pass.
- `cargo test --workspace --locked -- accept_f20_c_02_ --include-ignored` —
  exit 0, **9 tests matched** in
  `crates/cs_app/tests/accept_f20_c_02_fixed_tick_instances_and_teardown.rs`,
  all passing.
- the six mutation probes above — each probe's selection exited 101 and the
  files were restored.

No command needed `CS_GAME_DIR`, and no `accept_f20_c_02_` test is
`#[ignore]`d; `CS_CAPABILITIES` (`retail,gpu,audio`) was not exercised.

## Evidence

Synthetic fixtures and designed contracts only. No original-data, visual,
audible or ordinary-play claim; this stage can award at most **checked**.

## Sources

- `specs/F20-object-animation-and-authored-destruction-states.md`
  (`### F20-C`, behaviors 1, 3 and 4), `docs/contracts/IDENTITY-CONTENT.md`.
- `docs/findings/2026-09-30-f20-a-animation-channels-and-event-markers.md`
  (finding 5: the caller supplies the session tick),
  `docs/findings/2026-09-30-f20-b-transform-material-attachment-tracks.md`
  (boundaries 3 and 4, handed to F20-C),
  `docs/findings/2026-09-30-f20-c-01-attachment-hierarchy-and-detach-velocity.md`
  (the release-before-despawn rule and the release/despawn ordering
  requirement this teardown honours),
  `docs/findings/2026-09-29-f16-c-frame-clock-and-origin-integration.md` (the
  fixed-tick order; the app's measured hook is the F23-A adapter's).
