# F20-B: verified transform, material and attachment tracks

Date: 2026-09-30. Task: F20-B "Implement verified transform/material/attachment
tracks"
(`specs/F20-object-animation-and-authored-destruction-states.md`, section
`### F20-B`). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: ordinary build/test only (no `CS_GAME_DIR` read, no
evidence report required).

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/animation.rs` (extended): the declared-side fixtures
  this stage's production path is driven with —
  `declared_synthetic_propeller_clip` (the declared twin of
  `cs_sim::animated_object::synthetic_propeller_clip`: a looping transform
  track plus one gameplay and one presentation marker) and
  `declared_synthetic_cargo_clip` (a material track and an attachment track on
  one node), with their tick/marker constants.
- `crates/cs_app/src/animation/playback.rs` (new): the playback that closes
  the path F20-A left open —
  - `AnimationPlayback` (resource: session id, next producer serial, the live
    instance per `animation_track`),
  - `play_animation` / `stop_animation` / `advance_animation` (the fixed-tick
    entry that advances every instance and applies its tracks),
  - the ECS track components `NodeAnimatedPose`, `NodeAnimatedMaterial`,
    `NodeAnimatedAttachment`,
  - the published record: `AnimationLog` (events, blocked markers, blocked
    tracks, refusals) with `drain`, `BlockedTrack`, `TrackKind`,
    `AnimationRefusal` and `AnimationPlayError`.
- `crates/cs_app/src/animation/mod.rs`, `crates/cs_app/src/lib.rs` (wiring
  only): the module declaration, the re-exports and the doc paragraph that
  names the F20-B binding.
- `crates/cs_app/tests/accept_f20_b_animation_playback.rs` (new) and
  `crates/cs_content/tests/accept_f20_b_declared_track_fixtures.rs` (new): the
  `accept_f20_b_*` acceptance tests.
- This file.

**One observable failure:** if the playback did not keep one evaluator per
playing instance across ticks (for example if it rebuilt the evaluator from
the clip on every tick, losing the per-activation dedup state), the looping
propeller would emit `engine_started` once per loop pass instead of once per
activation — `accept_f20_b_looping_propeller_never_repeats_one_shot_gameplay_event`
counts 4 gameplay events instead of 1. If track application keyed entities by
node id alone, the stale-generation entity of
`accept_f20_b_transform_track_applies_to_verified_bindings_only` would be
driven, and if it trusted a `Resolved::Unknown` reference,
`accept_f20_b_material_and_attachment_tracks_apply_and_block_unknown_references`
would find a material or a parent that nobody resolved.

## Semantics defined at this stage

- **One live instance per track, in a session.** `AnimationPlayback` (a
  resource) holds the session id, the next producer serial and the live
  instance of every playing `animation_track`. `play_animation` lowers the
  declared clip through `lower::lower_clip` (which re-runs
  `AnimatedClip::try_new`) and refuses `NoSession`, `AlreadyPlaying`,
  `Lower` and `ProducerExhausted` instead of guessing: an animation never
  plays in no session, and a second instance never silently replaces the
  first. Event ids carry the playback's session, the tick the advance
  committed to, a per-instance producer serial and the object's own sequence
  (`IDENTITY-CONTENT`: session generations, unique event ids).
- **Clip time is the ticks since the instance started** (`at -
  started_at`), so the evaluator's head is monotone by construction. An
  instance whose own start tick has not arrived is passed over entirely: it
  offers no marker (not one authored at clip tick 0), applies no state and
  reports no hold, so a clip scheduled to start at tick `N` is silent at
  `N - 1`. A session tick that goes backwards holds the instance where it is,
  publishes `AnimationRefusal::Held { clip, from, to }` **once per
  occurrence** and changes nothing — combined with the evaluator's
  per-activation dedup, a reversed or restarted session can never re-offer a
  one-shot marker (non-negotiable behavior 5).
- **Verified binding.** A track value reaches an entity only when its
  `AnimatedNodeBinding` names a clip that is *playing*, is stamped with the
  *scene generation* that instance serves, and names a node the clip drives
  with an aspect that has a reached key. Every other entity keeps its state:
  a stale generation, an unplayed track or an undriven node is never written.
- **Idempotent application.** The evaluated state is derived from the clip
  position every tick and a component is written only when its value changed,
  so replaying a span or looping a clip back over a pose writes the same
  value and re-fires nothing (non-negotiable behavior 3).
- **Unknowns block one track, never the node.** A `Resolved::Unknown`
  material or attachment parent never becomes a component: the previously
  applied value stays, the refusal is published as a `BlockedTrack` carrying
  the claim id and reason **once per (clip, node, track)**, and the node's
  other tracks plus every marker keep working. The publication is derived
  from the clip's evaluated state alone, so a gap in the clip's own content
  is visible whether or not an entity happens to be bound to that node (the
  same rule a blocked marker follows); the binding decides only whether a
  *value* reaches the world. An unknown therefore stays visible in the log
  instead of per-frame-repeating or being repaired (non-negotiable behavior
  2).
- **One pose component.** `NodeAnimatedPose` stores the single applied pose;
  `mesh()` and `collider()` are two names for it, so the transform track
  cannot reach a render consumer on one tick and a collision consumer on
  another (the F20-A rule carried into the ECS).
- **The log is the consumer seam.** `AnimationLog` appends events, blocked
  markers, blocked tracks and refusals, and `drain()` hands a consumer its
  batch — it grows with publications, never with frames.

## Boundaries this stage does not cross

1. **Hierarchy and physics.** The transform track is published as a pose
   component; recomposing descendants' world poses from it, reparenting the
   ECS `ChildOf` links, and the inherited velocity of a detach are F20-C
   (AC03), as F20-A's findings already recorded for `AttachmentState`.
2. **Visibility.** The visibility channel is evaluated by F20-A's evaluator
   but not applied here: `NodePresentation` is written every pass by F11-C's
   `select_lod_presentation`, and animation writing it would need an
   ordering/ownership decision (destroyed vs. culled vs. hidden) that this
   stage must not guess. Applying it belongs to F20-C's destruction wiring.
3. **Clock and schedule placement.** `advance_animation(&mut World, at)` is
   the fixed-tick entry the session driver calls once per committed tick
   (F16-C order); which resource carries that tick into `FixedPostUpdate` is
   F20-C's wiring — F20-A's finding 5 ("the caller supplies the session
   `Tick`") still stands.
4. **Instance identity.** One live instance per `animation_track` id. Two
   aircraft playing the same propeller track need an instance id on
   `AnimatedNodeBinding`; that changes F20-A's binding record, so it is
   recorded here as F20-C's spawn-wiring decision instead of being invented
   by this stage.
5. **Consumers of `AnimationLog`.** The mission/objective layer that
   consumes gameplay markers is F20-C's; this stage publishes them.

## Tests and mutation probes

| test | what it pins |
| --- | --- |
| `accept_f20_b_looping_propeller_never_repeats_one_shot_gameplay_event` (minimum, AC02) | the declared propeller lowers to *exactly* the F20-A runtime fixture, runs four passes through the playback, fires `engine_started` once (4 loop passes), fires `blade_pass` once per pass at ticks 0/4/8/12/16, stamps unique ids with session + producer, and wraps the rotor pose on the bound entity every tick |
| `accept_f20_b_transform_track_applies_to_verified_bindings_only` | the door pose reaches the verified binding only: a superseded generation, an undriven node and an unplayed track stay untouched, and `mesh()`/`collider()` are the same stored value at the open tick |
| `accept_f20_b_material_and_attachment_tracks_apply_and_block_unknown_references` | known material/parent apply with their authored values and pose policies (and no pose is invented where the clip has no transform channel); unknown material and parent produce no component, two `BlockedTrack` reports with claim + reason, and nothing on the following tick |
| `accept_f20_b_holding_the_head_never_replays_a_one_shot_marker` | a backwards session tick holds the head, re-offers no marker, keeps the reached pose and reports the hold once |
| `accept_f20_b_play_requires_a_session_and_refuses_a_second_instance` | `NoSession`, `AlreadyPlaying`, advance/stop with nothing to act on, and a stopped instance leaving its applied state to its owner |
| `accept_f20_b_a_clip_never_plays_before_its_start_tick` | an instance started at tick 10 offers nothing at tick 9 — no marker, no applied state, no refusal — and first fires its clip-tick-0 marker at tick 10, with tick 10 in the event id; its gameplay marker follows at tick 11 |
| `accept_f20_b_an_unknown_reference_is_reported_without_a_bound_entity` | both unknown tracks of a playing clip are reported with no `AnimatedNodeBinding` anywhere in the world, the unbound world keeps no animated component, and the gap is still reported once |
| `accept_f20_b_propeller_fixture_is_a_looping_track_with_two_markers`, `accept_f20_b_cargo_fixture_carries_material_and_attachment_tracks`, `accept_f20_b_a_material_key_naming_another_kind_is_refused` (content) | the fixtures' identity, provenance, loop mode, every authored key/marker/tick, and the content-boundary refusal of a material key naming another kind |

Mutation probes run locally (all reverted afterwards):

1. Removing the evaluator's gameplay dedup (`fired_gameplay.insert(index)`
   forced fresh) → `accept_f20_b_looping_propeller_never_repeats_one_shot_gameplay_event`
   counts **8** gameplay events instead of 1.
2. Removing the generation check in `advance_animation` →
   `accept_f20_b_transform_track_applies_to_verified_bindings_only` fails at
   the stale-generation assertion.
3. Suppressing the `BlockedTrack` publication for an unknown material →
   `accept_f20_b_material_and_attachment_tracks_apply_and_block_unknown_references`
   reports 1 block instead of 2 and fails.

## Review fixes (2026-09-30, reviewer mimo-1)

Two problems found while reviewing the submitted branch were fixed in place:

1. **A clip could play before its own start tick.** `advance_animation`
   computed `at - started_at` with a saturating subtraction, so for an
   instance started at a future tick the clip time was `0` instead of "not
   yet": the first advance fired the clip-tick-0 markers (stamped with that
   pre-start tick) and applied the position-0 state one tick early. The
   marker loop and the state snapshot now pass over an instance whose own
   start tick has not arrived, so a clip scheduled to start at tick `N` stays
   silent at `N - 1` and its markers keep firing at their authored tick
   (non-negotiable behavior 1).
2. **A `BlockedTrack` was published only when an entity was bound to the
   node.** The refusal lived inside the binding loop, so a playing clip whose
   unknown material/parent had no bound entity published nothing — unlike a
   blocked marker, which the evaluator reports without consulting the world.
   The publication now derives from the evaluated state of the playing
   instance; a binding decides only whether a *value* is written.

Both are covered by `accept_f20_b_a_clip_never_plays_before_its_start_tick`
and `accept_f20_b_an_unknown_reference_is_reported_without_a_bound_entity`.
Probe: with `crates/cs_app/src/animation/playback.rs` restored to the
submitted (pre-fix) revision, both new tests fail (a marker published before
the start tick; `0` blocks instead of `2`) while the five earlier `cs_app`
tests still pass; with the fix restored they pass again.

## Checks run

- `cargo fmt --all -- --check` — exit 0.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  — exit 0.
- `cargo test --workspace --locked` — exit 0 (134 test binaries/suites ok).
- `cargo test --workspace --locked -- accept_f20_b_ --include-ignored` — 10
  tests matched (7 `cs_app`, 3 `cs_content`), all passing.

## Unknowns and follow-ups

- The original animation container layouts (`mis_anim.zbd`, `cam_anim.zbd`)
  are still undecoded (F13), and this stage reads no original data: every
  record, fixture value and rule above is **designed**, and no original
  behavior is claimed. F20-D keeps the original-family validation gate.
- `cs_types::net` now carries the shared `SessionId`/`EventId`/`ActorId`
  types, and #397 migrated `AnimationEventId` onto `EventId` and the playback
  onto `SessionId` (F20-A follow-up 1 resolved;
  `docs/findings/2026-10-02-t397-shared-identity-ids.md`). The damage and audio
  realizations of the same shape (#442 and the audio follow-up) are still open.
- The four boundaries above are F20-C's; they are handed over in a note on
  task #75 (F20-C) as well as here, so they survive this stage being done.

## Evidence

Synthetic fixtures and designed contracts only. No original-data, visual,
audible or ordinary-play claim; this stage can award at most **checked**.

## Sources

- `specs/F20-object-animation-and-authored-destruction-states.md`
  (`### F20-B`), `docs/contracts/IDENTITY-CONTENT.md`.
- `docs/findings/2026-09-30-f20-a-animation-channels-and-event-markers.md`
  (the stage this one plugs in, and its recorded follow-ups).
