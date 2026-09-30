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
    (teardown wiring is F20-C.02's);
  - `creates_cycle`, `spin_term`, `reference_point`: the two guards that
    refuse a hierarchy cycle before anything is written, and that measure the
    `ω × r` term only where there is a spin to measure it with (both were
    added in review; see the two review sections below).
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
     point to the node's, and `ω_inherited = ω_spin`;
   - the `ω × r` term is measured **only where there is a spin**: with
     `ω = 0` it is exactly zero for every `r`, so no reference point is
     consulted and the linear source is inherited whole. A real spin needs
     both ends of `r` and reports which one is missing
     (`NoReferencePoint` for the source, `NoNodeReferencePoint` for the
     detaching node).
   The values are written **only onto components the entity already
   carries** — a decorative node never gains a velocity component it was
   never simulated with, which is not a missing inheritance and so is not
   reported. Nothing is invented: a chain with no velocity component, an
   unmeasurable `r` or a detach from a node that was already a root
   reparents and preserves the pose anyway and publishes one
   `VelocityNotInherited` record saying why; a chain that carries only an
   angular source contributes the rotation alone and says so
   (`NoLinearSource`), because `ω × r` without a linear reference point
   would be a guess. The node's own angular velocity is left alone when no
   ancestor spins: overwriting it with a zero would *be* an invention.
6. **Release before despawn.** `release_attachments_before_despawn` detaches
   the children whose animated attachment this consumer applied (world pose
   preserved by construction — the world affine is simply not touched, so a
   released child needs no composed pose of its own), updates their applied
   record, and inherits the parent's velocity by the same rule as an authored
   detach. An entity the animation never touched keeps its authored `ChildOf`
   and stays part of the parent's subtree. The Bevy behavior this rule exists
   for was **measured**, not assumed — see "The Bevy despawn measurement"
   below.

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
| `accept_f20_c_01_release_reaches_an_animated_attachment_below_an_unmanaged_child` (added in review) | the release walks the whole subtree the recursive despawn reaches: an animated attachment at depth two is released (and inherits the velocity of the parent it was linked to) while the unmanaged child between it and the doomed root still dies with the root — measured in the test itself |
| `accept_f20_c_01_a_parent_inside_the_nodes_own_subtree_is_refused` (added in review) | a parent id resolving to a node inside the animated node's own subtree is refused (`CyclicParent`), writes no `ChildOf`, leaves the existing hierarchy untouched and publishes exactly one refusal |
| `accept_f20_c_01_an_unmeasurable_spin_term_still_inherits_the_linear_source` (added in review) | a chain that does not spin inherits its linear source whole however unmeasurable `r` is, and the node's own spin is never overwritten with a zero; a spinning chain whose detaching node has no location of its own inherits nothing, publishes one `NoNodeReferencePoint` saying which end of the offset is missing, and still survives the despawn without a composed pose |

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
| the `creates_cycle` check disabled in `apply_one` (review mutation probe) | `..._a_parent_inside_the_nodes_own_subtree_is_refused`: the test process **aborts with a stack overflow** (`recompose_descendants` recursing over the cycle that was just inserted) rather than merely failing — 1 |
| the release walk limited to the direct children of `parent` (review mutation probe) | `..._release_reaches_an_animated_attachment_below_an_unmanaged_child` (`left: [], right: [cargo]`) — 1 |
| the `ω × r` term measured even when the chain does not spin (review mutation probe) | `..._an_unmeasurable_spin_term_still_inherits_the_linear_source` — 1 |
| the unmeasurable-spin-term refusal swallowed instead of published (review mutation probe) | `..._an_unmeasurable_spin_term_still_inherits_the_linear_source` — 1 |
| the node's own spin overwritten with zero when no ancestor spins (review mutation probe) | `..._an_unmeasurable_spin_term_still_inherits_the_linear_source` — 1 |

## Review fixes (2026-09-30, review pass)

Two problems found in review of the original submission, fixed in the owner
paths and pinned by the two tests marked "added in review" above. Both are
**designed** decisions like the rest of this stage (still no original
animation data, F13/F20-D unchanged).

1. **A parent inside the node's own subtree was applied.** The consumer
   resolved a parent id and inserted `ChildOf` without checking whether the
   parent was the node itself or one of its descendants. That is exactly
   what `docs/contracts/IDENTITY-CONTENT.md` declares invalid (*"cycles in
   ownership/parent hierarchies are invalid"*), and it is not a cosmetic
   violation: measured by disabling the new check, the resulting cycle makes
   `recompose_descendants` recurse until the process dies with
   `fatal runtime error: stack overflow`. Fix: `creates_cycle` walks the
   parent's ancestor chain (with a visited set, so it also terminates on a
   chain that already loops) before anything is written and publishes
   `AttachmentRefusalReason::CyclicParent` — no `ChildOf`, no half-reparent,
   one refusal per state.
2. **The release before a despawn only covered one hierarchy level.** The
   same measurement that motivates the rule (`despawn` is recursive over
   `Children`) means an animated attachment below a child the animation
   never touched was *still* despawned with its grandparent. The rule the
   stage exists to implement therefore did not hold past depth 1. Fix:
   `release_attachments_before_despawn` walks the whole subtree breadth
   first, releasing every managed attachment it reaches and stopping the
   descent at a node it released (a released child survives together with
   its own subtree, so unparenting below it would sever a link that was
   never in danger); the unmanaged nodes between the doomed root and a
   released attachment still die with the root. The velocity a deep release
   inherits comes from the parent the released node was actually linked to,
   read before the link goes away.

Neither fix changes an `accept_f20_a_*` / `accept_f20_b_*` assertion, and
neither touches `crates/cs_app/src/scene.rs` or a protected path.

## Review fixes, second pass (2026-09-30)

One further defect in the inherited-velocity rule, found in review of the
first review pass and fixed here. It is a **designed** decision like the rest
of this stage (no original animation data, F13/F20-D unchanged).

3. **A detach could refuse an inheritance it was able to compute exactly,
   and a release could pass in silence.** `detached_velocity` asked for the
   two reference points of `r` *before* it knew whether `r` was needed: with
   `ω = 0` the term `ω × r` is exactly `0` for every `r`, so a chain that does
   not spin was refused (`NoReferencePoint`) instead of inheriting its linear
   source, which is the one value that *was* exactly known. Two consequences,
   both measured before the fix:

   - an authored detach under a source with no reference point of its own
     (a hull that is not a scene node) and no spin anywhere kept
     `LinearVelocity(Vec3::ZERO)` and published `NoReferencePoint`, although
     `v_inherited = v_source` was exactly computable;
   - `release_attachments_before_despawn` branched on the released child's
     composed world pose. A managed child without one was unparented and
     marked released, inherited **nothing**, and published **nothing at all**
     — a hierarchy change plus a missing inheritance, silently, which is
     exactly what the module's own rule ("report every case where nothing
     could be inherited") and the task's error-propagation requirement
     forbid.

   Fix: the `ω × r` term is computed by `spin_term`, which returns exactly
   zero for a chain that does not spin — no reference point is consulted and
   nothing is reported — and otherwise requires both ends of `r` and reports
   which one is missing (`NoReferencePoint` for the source,
   `NoNodeReferencePoint` for the detaching node, a new variant). The node's
   own reference point is read inside `detached_velocity` (`Position`, else
   its composed pose) instead of being passed in, so the release no longer
   needs a composed pose at all: the branch that released silently is gone,
   and a pose-less managed child now either inherits everything measurable or
   publishes why. The release is also one code path now, not two.
   Pinned by `accept_f20_c_01_an_unmeasurable_spin_term_still_inherits_the_linear_source`
   (three probes, above; the third one also pins that the node's own spin is
   never zeroed, which nothing asserted before).

   Also recorded here, because the cycle guard of the first review pass raised
   it: the two remaining hierarchy walks cannot loop on a pre-existing cycle.
   Both walk *down* from a node (`recompose_descendants` and the release walk
   follow `ChildOf` children), and a `ChildOf` cycle is closed — every node in
   it has its parent inside it — so no downward walk from outside can reach
   one. The `creates_cycle` guard is therefore the only place a cycle can be
   created, and it refuses before anything is written. This was verified
   against the pinned Bevy 0.19.1 rather than assumed: re-inserting the same
   `ChildOf` does not duplicate the entry in the parent's `Children`
   (the `on_discard` hook removes the source before `on_insert` re-adds it),
   so a same-parent attach is not a second hierarchy link.

## Checks run

- `cargo fmt --all -- --check` — exit 0.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  — exit 0.
- `cargo test --workspace --locked` — exit 0 (167 test binaries/suites ok,
  1546 tests passed, 0 failed, 104 retail tests skipped as they need
  `CS_GAME_DIR`; the 7 `accept_f20_b_*` and the `accept_f20_a_*` tests are
  unchanged and still pass — no F20-B assertion was touched).
- `cargo test --workspace --locked -- accept_f20_c_01_ --include-ignored`
  — exit 0, **8 tests matched** in
  `crates/cs_app/tests/accept_f20_c_01_attachment_hierarchy.rs`, all
  passing. (The implementer's run matched 5; the first review pass raised it
  to 7 with the two tests marked "added in review", the second to 8 with the
  one the velocity fix added.)

Both review passes re-ran the four commands above on the tree they pushed;
the last run is the one above (after the second pass's fix). The rebase onto
the then-current `origin/main`, the commit SHA and the CI run are recorded in
the task's handover notes.

## Unknowns and follow-ups

- **The consumer is not on a schedule yet.** `apply_attachment_transitions`
  runs at the end of `advance_animation`, and nothing calls
  `advance_animation` from a Bevy schedule in this crate, so no in-game tick
  reaches the attachment consumer until F20-C.02 places the advance
  (`#418`). That is the stage's own split, not a gap in the consumer, but it
  does mean nothing in this slice is reachable from a running session yet.
- **A release is only safe in the same step as the despawn.** A release marks
  the applied record, so the clip's own detach finds it and does not inherit
  twice. An *attach* record, though, is not marked by a release: if a
  released child still carries an `Attach` record naming a parent that is
  still alive and the advance runs again before the despawn, the consumer
  re-attaches it and the despawn then takes it with the parent. F20-C.02's
  teardown must therefore release and despawn in one step (or stop the
  instance first); the release cannot make that safe by itself.
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
  above is the one this slice hands to that teardown, together with the two
  ordering requirements just listed.
- `AnimatedObject` publishes its node states in a `BTreeMap` keyed by node id,
  so the set of driven nodes is itself ordered; which order a
  `Query` iteration yields the *entities* in is Bevy's, not ours. When one
  tick both a parent and its child transition, which of the two poses the
  child's `ω × r` is measured against therefore depends on that iteration
  order. Nothing observable depends on it today (no fixture authors a nested
  attachment, and the fixed-tick pass is single-threaded and deterministic
  for one world state), so it is recorded here rather than fixed; a fixed
  parent-before-child order belongs to whoever owns the schedule.

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
