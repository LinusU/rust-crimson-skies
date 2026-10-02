# F20-A: Animation channels and event markers

Date: 2026-09-30. Task: F20-A "Define animation channels and event markers"
(`specs/F20-object-animation-and-authored-destruction-states.md`, section
`### F20-A`). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: ordinary build/test only (no `CS_GAME_DIR` read, no
evidence report required).

## Files and the one observable failure (listed before editing)

- `crates/cs_sim/src/animated_object.rs` (new): the runtime records
  (`AnimatedClip`, `NodeChannel`, `PoseSample`, `ClipMarker`,
  `MarkerEffect`, `LoopMode`, `Interpolation`, `PosePolicy`,
  `AttachmentOp`), the fixed-tick evaluator `AnimatedObject::advance_to`,
  the coherent `AnimatedNodeState` output, `AnimationEvent` /
  `AnimationEventId` / `BlockedMarker` / `TickOutcome`, `AnimationError`
  and the `synthetic_door_clip` / `synthetic_propeller_clip` fixtures.
- `crates/cs_content/src/animation.rs` (new): the declared,
  provenance-carrying IR (`AnimationClip` with `Origin`/`Provenance`, the
  four channel records bound to `SceneNodeId`, `EventMarker` with
  `Resolved<MarkerEffect>`) and `declared_synthetic_door_clip`.
- `crates/cs_app/src/animation/{mod,lower,presentation}.rs` (new):
  `lower_clip`, the declared→runtime conversion boundary that carries every
  `Resolved::Unknown` through verbatim; `interpolated_pose`, the
  presentation-only fractional-alpha sampler; and the `AnimatedNodeBinding`
  ECS record.
- `crates/cs_sim/src/lib.rs`, `crates/cs_content/src/lib.rs`,
  `crates/cs_app/src/lib.rs` (wiring only): module declarations and docs.
- `crates/cs_sim/tests/accept_f20_a_animated_object.rs`,
  `crates/cs_content/tests/accept_f20_a_animation_clip.rs`,
  `crates/cs_app/tests/accept_f20_a_animation_boundary.rs` (new): the
  `accept_f20_a_*` acceptance tests.
- This file.

**One observable failure:** without per-activation dedup keyed on marker
identity, the looping propeller emits `engine_started` once per pass —
`accept_f20_a_looping_propeller_never_repeats_one_shot_gameplay_event`
counts 4 gameplay events instead of 1. Without a single evaluated pose per
node, the door's mesh can open while its collider stays closed —
`accept_f20_a_door_opens_at_fixed_tick_changing_collider_and_mesh_coherently`
and `accept_f20_a_door_clip_lowers_and_opens_coherently` compare the two
accessors. Both tests call production code (the evaluator, the lowering,
the declared fixture), so removing any layer fails to compile.

## Semantics defined at this stage

- **Tick-indexed channels.** `transform`, `visibility`, `material` and
  `attachment` channels hold keys at integer clip ticks in `0..=duration`.
  `Step` holds the last reached key; `Linear` blends the surrounding keys
  *at integer ticks only* — fractional resampling is `cs_app`'s
  presentation concern and produces no events (F20 non-negotiable 1).
- **Event markers** carry a stable authored `key` (unique per clip, never
  an index) and a `Resolved<MarkerEffect>`. `Gameplay` effects fire at most
  once per activation, deduplicated by marker index even across loops
  (AC02). `Presentation` cues may fire once per pass. Dedup retention is
  bounded by the clip's marker count (`HashSet<usize>` /
  `BTreeMap<usize, u64>`), not by elapsed time (IDENTITY-CONTENT).
- **Unknown effects block.** A marker with `Resolved::Unknown` effect is
  not skipped or guessed: reaching it reports a `BlockedMarker` (claim id +
  reason) once per activation, so the gated gameplay transition visibly
  does not fire (non-negotiable 2).
- **Coherent state.** `AnimatedObject::states()` evaluates all channels at
  the current position into one `AnimatedNodeState` per node;
  `mesh_pose()`/`collider_pose()` return the same `PoseSample` (the F11
  `visual_transform`/`collision_transform` pattern), and a `Hidden` node
  reports `collider_enabled() == false`.
- **Forward only.** `advance_to` refuses `Regression` — a reversed
  cinematic can never re-offer a one-shot marker (non-negotiable 5's skip
  direction is forward: `advance_to` over a span fires each pending
  gameplay marker exactly once, the AC04 shape).
- **Attachment records** state the new parent (or detach) plus an explicit
  `PosePolicy::{KeepWorldPose, KeepLocalPose}` (non-negotiable 4). The
  inherited-velocity physics of a detach is F20-C's consumer; the record
  here carries the change faithfully.

## Designed vocabulary, not original data

Every record, channel kind, marker effect name, dedup rule and fixture
value here is **newly authored project design**. Unknown and not guessed:

- the original animation container layouts: F13 located `mis_anim.zbd` /
  `cam_anim.zbd` as script-family archives with a validated signature but
  **no decoded instruction meaning** — channel encoding, marker opcodes and
  loop flags are all unmeasured (FINDINGS.md: "Camera/mission animation
  layouts … F20/F40"). The spec forbids importing MechWarrior semantics.
- whether the original couples node visibility to collision. This engine's
  rule — hidden ⇒ no collider — is designed, not observed.
- the original tick rate any animation ran at, and whether marker firing
  ran before or after physics (schedule variants: F16-D/F23-D/F38).
- which original files produce which channels; the importer and its
  evidence are F20-B/D.

## Follow-ups that gate later stages

1. **Resolved by #397 (`T-IDENTITY-IDS`).** `cs_types::net` gained the shared
   `SessionId`/`ActorId`/`EventId` types in F54-A, and #397 migrated
   `AnimationEventId` onto them: it is now `pub type AnimationEventId =
   cs_types::net::EventId`, and `AnimatedObject`/`AnimationPlayback` carry a
   nonzero `SessionId`. The animation event id is the shared contract type, not
   a second struct (`docs/findings/2026-10-02-t397-shared-identity-ids.md`).
   (The damage and audio realizations of the same shape are still open; see
   #442 and the audio follow-up filed by #397.)
2. **TRS-only transform keys.** `PoseSample`/`TransformSample` hold
   rotation + translation + scale (negative scale mirrors). Authored shear
   or full-affine tracks — if the original formats carry them — have no
   representation yet; F20-B decides when it decodes real track data.
3. **Attachment is a record, not a re-posed body.** `AttachmentState`
   reports the parent change and pose policy; applying the world-pose
   recomputation and inherited velocity belongs to F20-C (AC03).
4. **No spawn wiring.** `AnimatedObject` is a plain evaluator; the session
   spawn/despawn, the mission-marker consumer and the Bevy schedule
   placement are F20-B/C. `AnimatedNodeBinding` is the binding record only.
5. **`advance_to` is the only clock face**: callers supply the session
   `Tick` stamped into event ids, so event timing is the session driver's
   responsibility (F16-C ordering).

## Evidence

Synthetic fixtures and designed contracts only. No original-data, visual,
audible or ordinary-play claim; this stage can award at most **checked**.

## Sources

- `specs/F20-object-animation-and-authored-destruction-states.md`
  (`### F20-A`), `docs/contracts/IDENTITY-CONTENT.md`.
- `docs/research/FINDINGS.md` (animation layouts row), F13-B
  classification of `mis_anim.zbd`/`cam_anim.zbd`
  (`crates/cs_formats/src/script_raw/discovery.rs`).
