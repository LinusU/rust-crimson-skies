# F20-C.01: attachment transitions in the ECS hierarchy (AC03)

Date: 2026-09-30. Task: F20-C.01 "Apply attachment transitions to the ECS
hierarchy with inherited detach velocity (AC03)"
(`specs/F20-object-animation-and-authored-destruction-states.md`, section
`### F20-C`, non-negotiable behavior 4 and acceptance case **AC03**). Shared
contract: `docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: ordinary
build/test only (no `CS_GAME_DIR` read, no evidence report required).

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/animation/attachment.rs` (new): the consumer
  `apply_attachment_transitions(world)` that turns the
  `NodeAnimatedAttachment` record F20-B publishes into an actual parent
  change —
  - `AppliedAttachment` / `RefusedAttachment`: the per-entity bookkeeping
    that makes one evaluated state apply exactly once and one refusal
    publish exactly once;
  - `AttachmentRecord` (`Refused`, `VelocityNotInherited`), appended to
    `playback::AnimationLog`;
  - `release_attachments_before_despawn(world, parent)`: the release rule
    non-negotiable behavior 4 asks for, for whoever despawns a parent
    (teardown wiring is F20-C.02's).
- `crates/cs_app/src/animation/playback.rs` (extended): the
  `AnimationLog::attachments` collection plus `push_attachments`, the
  `AnimationPlayback::drives` query the consumer verifies with, and the one
  line at the end of `advance_animation` that runs the consumer — so the
  fixed-tick entry applies the transition the same tick it is evaluated.
- `crates/cs_app/src/animation/mod.rs` (wiring only): the `attachment`
  module declaration and its re-exports.
- `crates/cs_app/src/lib.rs` (wiring only): the `### F20-C` doc paragraph.
- `crates/cs_app/tests/accept_f20_c_01_attachment_hierarchy.rs` (new): the
  `accept_f20_c_01_*` acceptance tests, including the Bevy `despawn`
  measurement below.
- This file.

**One observable failure:** cargo bound to the bay node of the playing
`declared_synthetic_cargo_clip` never leaves the bay: at
`SYNTHETIC_CARGO_DETACH_TICK` the `ChildOf` link is still there and the
cargo's `LinearVelocity` stays `Vec3::ZERO` instead of becoming the parent's
`v + ω × r`, and advancing further ticks would add the inheritance again
(the double-add failure).
`accept_f20_c_01_detaching_cargo_from_a_moving_parent_inherits_the_parent_velocity`
fails on the `ChildOf`, the velocity and the "unchanged on later ticks"
assertions when the consumer (or the idempotence bookkeeping, or the
velocity write) is removed.

## Designed decisions (this feature is designed, not original-verified)

The original `mis_anim.zbd` / `cam_anim.zbd` layouts are still undecoded
(F13): every rule below is **designed**, and no original behavior is
claimed. F20-D keeps the original-family validation gate.

1. **The consumer runs inside `advance_animation`.** The transition is
   evaluated once per committed tick, in the same fixed-tick entry that
   published the record, so a test that drives `play_animation` →
   `advance_animation` drives the whole production path. F20-C.02 places
   that entry on the schedule; nothing else has to call the consumer.
2. **Verification, in order, before anything is attempted.**
   - No `AnimationPlayback`, or the clip is not playing → the entity does
     not verify: nothing is attempted and nothing is reported (a stopped
     instance leaves its state to its owner, F20-B's rule).
   - The clip does not drive the node → not verified, silent.
   - The entity carries no `SceneNodeBinding`, or that binding names
     another node id → it is not the scene node this clip drives, so there
     is no hierarchy link for this consumer to own: silent.
   - `SceneNodeBinding::generation` ≠ `AnimatedNodeBinding::generation`,
     or the live instance serves another generation than the binding →
     **stale**: nothing is applied and one `AttachmentRecord::Refused` is
     published (`StaleScene` / `StaleBinding`).
   - A parent id that resolves to no live `SceneNodeBinding` **of the
     binding's generation** (or to more than one) → nothing is applied, one
     `Refused` (`UnknownParent` / `AmbiguousParent`); a half-reparent never
     happens.
   - The node or the parent it needs has no `NodeVisualTransform` → the
     authored pose policy cannot be honored, so nothing is applied, one
     `Refused` (`NodeWorldPoseMissing` / `ParentWorldPoseMissing`).
   A refusal is published **once per (state, reason)**, kept in
   `RefusedAttachment`, while the lookup is retried every tick — so a
   parent that appears one tick later is still attached, and a log never
   grows one entry per frame (the `AirframeSceneLog` "report the gap, not
   the frame" rule).
3. **Idempotence.** `AppliedAttachment` records the transition that was
   applied; the consumer only acts while the evaluated state differs from
   it. The record is re-published every tick, so re-running the same advance
   performs no second transition, and the inherited velocity is written
   **exactly once per detach**.
4. **Pose policy.** `NodeVisualTransform` is the one pose owner (the node's
   composed world affine); the parent-relative pose is derived from it, so
   both policies are computed at the transition instead of being stored:
   - `KeepWorldPose`: the world affine is written only if it must change
     (it does not for a detach), and the new local pose is
     `new_parent⁻¹ · world`;
   - `KeepLocalPose`: the local pose is kept — `old_parent⁻¹ · world`
     before the change (the world pose itself when the node was a root) —
     and the world pose becomes `new_parent · local`. For a **detach** the
     new parent is nothing, so the kept local numbers become the world
     pose; that literal reading is authored data, not a guess, and the
     fixtures only author `KeepWorldPose` for their detach.
   Afterwards the `NodeVisualTransform` of every descendant is recomposed
   from the node's new world affine and each descendant's old local pose,
   so a parent change never leaves a child with a stale world affine
   (F20-B boundary 1). The walk only runs when the world affine actually
   changed, so a `KeepWorldPose` transition writes no floats at all.
5. **Inherited detach velocity — the source is named, not assumed.** At the
   detach tick the consumer walks from the parent being left up the `ChildOf`
   chain:
   - the **linear source** is the nearest ancestor carrying avian
     `LinearVelocity`;
   - the **spin source** is the nearest ancestor carrying avian
     `AngularVelocity`;
   - the reference point of a body is its avian `Position` when it has one
     (physics' authority for a body's location) and otherwise its
     `NodeVisualTransform` translation;
   - `v_inherited = v_source + ω × r`, with `r` from the source's reference
     point to the node's, and `ω_inherited = ω_spin`.
   The values are written **only onto components the entity already
   carries** — a decorative node never gains a velocity component it was
   never simulated with. Nothing is invented: a chain with no velocity
   component, an unknown reference point or a detach from a node that was
   already a root reparents and preserves the pose anyway and publishes one
   `VelocityNotInherited` record saying why; a chain that carries only an
   angular source contributes the rotation alone and says so
   (`NoLinearSource`), because `ω × r` without a linear reference point
   would be a guess. The node's own angular velocity is left alone when no
   ancestor spins: overwriting it with a zero would *be* an invention.
6. **Release before despawn.** `release_attachments_before_despawn` detaches
   the children whose animated attachment this consumer applied (world pose
   preserved by construction — the world affine is simply not touched),
   updates their applied record, and inherits the parent's velocity by the
   same rule as an authored detach. An entity the animation never touched
   keeps its authored `ChildOf` and stays part of the parent's subtree. The
   Bevy behavior this rule exists for was **measured**, not assumed — see
   "The Bevy despawn measurement" below.

## The Bevy despawn measurement (rule 6)

Measured on the pinned Bevy (**0.19.1**, `Cargo.lock`), both from the source
and by running it, not assumed:

* `bevy_ecs::world::World::despawn` and `EntityWorldMut::despawn` both
  document: *"This will also despawn the entities in any
  `RelationshipTarget` that is configured to despawn descendants. For
  example, this will recursively despawn `Children`."*
* `accept_f20_c_01_attachments_are_released_before_a_parent_is_despawned`
  asserts the same thing by running it: a child linked with `ChildOf` is
  **gone** after `world.entity_mut(parent).despawn()`, while a child
  released by `release_attachments_before_despawn` first is still alive
  afterwards.

So an attachment that is still linked when its parent is despawned does not
survive with a dangling link — it disappears with the parent, silently
taking the cargo (or the dropped part) with it. That is what the release
rule prevents, and why F20-C.02's teardown must call it *before* the
despawn.

## Tests and mutation probes

| test | what it pins |
| --- | --- |
| `accept_f20_c_01_detaching_cargo_from_a_moving_parent_inherits_the_parent_velocity` (minimum, AC03) | the declared cargo clip drives a moving parent (v and ω both nonzero): the cargo is parented to the bay before `SYNTHETIC_CARGO_DETACH_TICK`, the `ChildOf` link is gone at it, the composed world pose is bit-identical across the tick (`KeepWorldPose`), the linear velocity is `v + ω × r` exactly (tolerance documented in the test) and the angular velocity is the parent's, and advancing further ticks changes neither value at all (the double-add failure) |
| `accept_f20_c_01_attaching_with_keep_local_pose_keeps_the_local_pose_and_moves_the_world_pose` | `Attach` with `KeepLocalPose` keeps the local pose of the node *and* of its child (both derived, never stored twice) while their world poses move with the new parent — the descendant recomposition of F20-B boundary 1 |
| `accept_f20_c_01_unresolved_parent_and_stale_binding_reparent_nothing_and_report_once` | a parent id that names no live entity of the binding's generation applies nothing and reports once (not once per tick); a binding whose generation the live instance does not serve applies nothing and reports once; both keep the `ChildOf` they had |
| `accept_f20_c_01_the_same_advance_never_writes_the_transition_twice` | running the advance over the same tick again leaves the hierarchy and the inherited velocity untouched (the applied record, not a frame counter, decides) |
| `accept_f20_c_01_attachments_are_released_before_a_parent_is_despawned` | the measured Bevy `despawn` behavior, plus `release_attachments_before_despawn`: released children keep their world pose and survive the parent's despawn |

Mutation probes run locally (all reverted afterwards, verified by
`grep -rn "MUTATION PROBE" crates/` returning nothing and the suite being
green again):

| probe | tests that fail |
| --- | --- |
| the `apply_attachment_transitions` call removed from `advance_animation` (the consumer never runs) | **all 5** `accept_f20_c_01_*` |
| the `AppliedAttachment` idempotence gate disabled (the state is re-applied every tick) | `..._detaching_cargo_from_a_moving_parent_inherits_the_parent_velocity` (4 stray `NoParent` publications), `..._the_same_advance_never_writes_the_transition_twice`, `..._attachments_are_released_before_a_parent_is_despawned` — 3 |
| the `ChildOf` removal on detach skipped (velocity written, link kept) | `..._detaching_cargo_...` (the `ChildOf` assertion), `..._the_same_advance_never_writes_...`, `..._attaching_with_keep_local_pose_...` — 3 |
| the inherited `LinearVelocity` computed but never written | `..._detaching_cargo_...` (`got [0, 0, 0], expected [10, 8, 0]`), `..._attachments_are_released_...` — 2 |
| `recompose_descendants` skipped | `..._attaching_with_keep_local_pose_...` (`the descendant's composed world pose is recomposed behind the change`) — 1 |
| the refusal publication swallowed in `refuse()` | `..._unresolved_parent_and_stale_binding_reparent_nothing_and_report_once` — 1 |

## Checks run

- `cargo fmt --all -- --check` — exit 0.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  — exit 0.
- `cargo test --workspace --locked` — exit 0 (141 test binaries/suites ok,
  no failures; the 7 `accept_f20_b_*` and the `accept_f20_a_*` tests are
  unchanged and still pass — no F20-B assertion was touched).
- `cargo test --workspace --locked -- accept_f20_c_01_ --include-ignored`
  — exit 0, **5 tests matched** in
  `crates/cs_app/tests/accept_f20_c_01_attachment_hierarchy.rs`, all
  passing.

## Unknowns and follow-ups

- The original animation container layouts (`mis_anim.zbd`, `cam_anim.zbd`)
  are still undecoded (F13), and this stage reads no original data: every
  record, fixture value and rule above is **designed**, and no original
  behavior is claimed. F20-D keeps the original-family validation gate.
- Scene-node poses and Avian body poses are not synchronized anywhere in the
  crate today; this slice reads both only to name a velocity reference point
  (rule 5) and never writes `Position`. Whether the two ever disagree, and
  who owns that sync, is not decided here.
- Instance identity on `AnimatedNodeBinding`, schedule placement and
  teardown of the applied components are F20-C.02 (`#418`); the release rule
  above is the one this slice hands to that teardown.

## Evidence

Synthetic fixtures and designed contracts only. No original-data, visual,
audible or ordinary-play claim; this stage can award at most **checked**.

## Sources

- `specs/F20-object-animation-and-authored-destruction-states.md`
  (`### F20-C`, non-negotiable behavior 4, AC03),
  `docs/contracts/IDENTITY-CONTENT.md`.
- `docs/findings/2026-09-30-f20-a-animation-channels-and-event-markers.md`,
  `docs/findings/2026-09-30-f20-b-transform-material-attachment-tracks.md`
  (boundary 1, handed to F20-C).
