# F20-C.03: the visibility channel, LOD/damage ownership, authored destruction

Date: 2026-10-02. Task: F20-C.03 "Apply the visibility channel with
LOD/damage ownership and authored destruction transitions" (#419). Spec:
`specs/F20-object-animation-and-authored-destruction-states.md`, section
`### F20-C`, non-negotiable behavior 3. Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: ordinary build/test only — no `CS_GAME_DIR` read, no
render, no audio, so no `private/evidence/` report is produced.

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/animation.rs` (extended): the declared fixture this
  slice's production path is driven with,
  `declared_synthetic_breakable_clip` — the one **declared** clip with a
  visibility channel (the three existing declared fixtures carry transform,
  material and attachment channels only, and the declared door fixture's
  identity is pinned by the F20-A/B tests, so a fourth fixture is added rather
  than one of them changed), with
  `SYNTHETIC_BREAKABLE_{NODE, DURATION, HIDDEN_TICK, SHOWN_TICK, BREAK_TICK,
  MARKER}`.
- `crates/cs_app/src/animation/visibility.rs` (**new**): the consumer half of
  the channel —
  - `NodeAnimatedVisibility` — the clip's evaluated visibility applied to a
    node (`visibility()`, `collider_enabled()`), written by
    `advance_animation` on the same verified-binding path as the other three
    channels;
  - `DrawVerdict`, `ColliderVerdict`, `VisibilityVerdict` — the single
    **composed** verdict, and `VisibilityVerdict::compose` plus
    `composed_visibility_verdict(world, entity)`, the one read entry a
    presentation or collision consumer calls.
- `crates/cs_app/src/animation/playback.rs` (extended): the visibility write
  (`NodeWrite::Visibility`), the `insert_changed` application, the release in
  `release_instance`, and the module docs that said "three track kinds".
- `crates/cs_app/src/animation/mod.rs`, `crates/cs_app/src/lib.rs` (wiring
  only): the module declaration, the re-exports and the doc paragraph.
- `crates/cs_app/tests/accept_f20_c_03_visibility_lod_ownership.rs` (**new**):
  the `accept_f20_c_03_*` acceptance tests.
- This file.

**One observable failure:** the visibility channel cannot leave the playback
at all. `advance_animation` writes `NodeAnimatedPose`,
`NodeAnimatedMaterial` and `NodeAnimatedAttachment` and never the visibility,
and nothing in the crate composes a visibility verdict at all, so a declared
clip that hides a node at its authored tick leaves the node **drawn and
colliding forever**:
`accept_f20_c_03_a_hidden_node_stays_hidden_across_a_lod_selection_pass` finds
no applied visibility on the bound entity and reads a drawn verdict at the hide
tick.

**The second, structural failure** is the one the task exists to settle. If the
animation wrote the public `NodePresentation` — the single field
`select_lod_presentation` rewrites on every pass — the hide would be silently
lost on the next distance change, and the reverse defect is worse: an
animation pass that wrote `Drawn` over `Disabled` would **re-draw a destroyed
node** (F20 non-negotiable behavior 3). The acceptance test drives the real
`select_lod_presentation` after the hide, so a design with one shared field and
a schedule constraint fails it.

## Designed decisions (this feature is designed, not original-verified)

The original `mis_anim.zbd` / `cam_anim.zbd` layouts are still undecoded (F13):
every rule below is **designed**, and no original behavior is claimed. F20-D
keeps the original-family validation gate.

1. **Two records, one composed verdict; no second writer.** The clip's fact is
   the new component `NodeAnimatedVisibility`; LOD/damage's fact is F11-C's
   existing `NodePresentation`; the answer a consumer reads is
   `VisibilityVerdict`, composed **at read time** by
   `composed_visibility_verdict`. The composition is *not* stored as a
   component, and the animation never writes `NodePresentation`. That is the
   whole ordering decision, and it is why it needs no schedule constraint:
   because the verdict is computed from the two records as they are at the
   moment of the read, the LOD pass cannot lose the animation's verdict (it
   never touches it) and the animation cannot override LOD or damage (it never
   touches their field), whatever order the two run in. A stored verdict would
   reintroduce exactly the race this stage exists to settle: it would have to
   be ordered after `select_lod_presentation` in a schedule, and
   `crates/cs_app/src/scene.rs` — which registers the LOD system — is outside
   this task's owner paths, so no honest constraint could be placed there.
2. **The priority, combination by combination** (the draw half):

   | `NodePresentation` (LOD/damage) | clip visibility | composed `draw` |
   | --- | --- | --- |
   | `Disabled` (self or ancestor `NodeDisabled`) | `Visible` or `Hidden` | `Disabled` |
   | `LodCulled` | `Visible` or `Hidden` | `LodCulled` |
   | `Drawn` | `Hidden` | `HiddenByAnimation` |
   | `Drawn` | `Visible` or no channel | `Drawn` |
   | no presentation record | `Hidden` | `HiddenByAnimation` |
   | no presentation record | `Visible` or no channel | `Drawn` |

   - **Damage wins over everything.** It is F11-C's own rule one level up
     ("`Disabled` wins over `LodCulled` at any depth, so a destroyed wing stays
     destroyed whichever band its parent group selects"), and the ancestor fold
     is already in that record, so the composition needs no hierarchy walk. A
     looping clip that re-shows the node on every pass and a distance change
     therefore both leave a destroyed node not drawn — non-negotiable
     behavior 3, asserted tick by tick.
   - **LOD's reason outranks the clip's**, but the clip's *fact* is not lost:
     a culled variant is reported culled, because at that distance it is not
     the band the group chose and blaming the clip for a distance decision
     would be a lie. The clip's own record still reads `Hidden` and still
     reaches collision (decision 3).
   - **The clip decides only against `Drawn`.** Nothing in LOD/damage opposes
     the node, so the clip's own verdict is the answer.
   - **No presentation record is not a cull.** An entity the LOD pass has
     never written for carries no evidence of a distance decision, so the
     composition reports what the clip says; it does not invent a cull.
3. **Collision is composed on the clip's own record, not on the draw
   reason.** `ColliderVerdict::NoCollider` exactly when a playing clip hides
   the node — F20-A's designed rule, carried from
   `AnimatedNodeState::collider_enabled()` — and `Undecided` otherwise, because
   nothing in this composition decides collision for a drawn node (that is the
   authored `CollisionRole`, F11-C/F29) or for a culled one (F11-C: *"LOD is
   presentation state only … collision, weapon origins and damage identity live
   on the bound node regardless of which variant is active"*). A disabled node
   is `Undecided` for collision as well, for the same reason: `NodeDisabled` is
   a presentation marker, and F11-C states that collision and damage identity
   read their own records, never `NodePresentation`. **A collider verdict is
   never guessed.** Because the collision half is not gated on the draw
   reason, a node the clip hides carries no collider whether or not LOD culls
   it — a render consumer and a collision consumer cannot disagree about
   whether the clip hid it.
4. **The channel has no blocked-track case, and that is stated, not
   forgotten.** A visibility key carries a `NodeVisibility`, not a
   `Resolved<_>`, so there is nothing to be unknown: `TrackKind` has no
   `Visibility` variant and no `BlockedTrack` is ever published for it. The
   blocking rules that do apply are the ones all four channels share: a channel
   with **no reached key writes nothing** (the previous record stays, which is
   the base state the spawned object has), and an entity whose
   `AnimatedNodeBinding` does not verify is never written, so an animation that
   is not playing cannot hide anything.
5. **The applied record is the clip's, the verdict is the world's.** A hidden
   node keeps `NodeAnimatedVisibility(Hidden)` on a damaged node, because the
   component records what the clip evaluates and the verdict records what the
   world does. That is why the destruction rule lives in the composition and
   not in the write: the write is idempotent, verified-binding-gated and
   teardown-released like the other three channels, and none of that needs to
   know about damage.
6. **The teardown releases the visibility with the rest.** `release_instance`
   (F20-C.02) removes `NodeAnimatedVisibility` for the entities of the instance
   it tore down, so a stopped instance cannot leave a node hidden forever, and
   a node that is **also** damaged stays `Disabled` after the teardown — which
   is the non-negotiable-3 assertion that distinguishes the two.
7. **The hidden verdict is per node; no subtree fold is designed.** The clip
   names one node. Whether the original's visibility swap hides the node's whole
   subtree — the way `NodeDisabled` and a culled band both propagate — is
   unmeasured, so nothing here folds ancestors for the animation half. A
   consumer that needs subtree visibility must fold it explicitly and record
   that decision; this stage does not guess one.
8. **`BlockedTrack`-style reporting is unchanged** by this stage: the three
   `Resolved` channels keep publishing once per `(clip, node, track)`, and the
   visibility write is the fourth channel of the same verified path, so a gap in
   the clip's content is reported exactly as before.

## Boundaries this stage does not cross

1. **No consumer is invented.** `NodePresentation` is read by no system in the
   crate today (F17's render sync is the named consumer and does not read it
   yet), and there is **no** collision-enable component in the owner paths —
   `grep -rn CollisionEnabled crates/` returns nothing, so writing Avian's
   component from here would be inventing a consumer for a physics subsystem
   another stage owns, on a coupling whose original semantics are unmeasured.
   What this stage wires is the applied record plus the one read entry
   (`composed_visibility_verdict`); the two consumers that must exist are filed
   as follow-up tasks instead.
2. **`crates/cs_app/src/scene.rs` is untouched** (read-only for this task),
   and so is every protected path. The composition reads F11-C's record and
   writes nothing in F11-C's files.
3. **The gameplay-marker consumer** is still F20-C's later slice: this stage
   publishes nothing new into `AnimationLog`; it applies one more channel.

## Expectations this slice changed in the earlier tests

None. No `accept_f20_a_*`, `accept_f20_b_*` or `accept_f20_c_0[12]_*`
assertion changes: the visibility channel is additive (no existing fixture has
one, so no existing entity ever gains the component), and the teardown
extension removes a component that no earlier test's entity carries.

## Tests

| test | what it pins |
| --- | --- |
| `accept_f20_c_03_the_breakable_fixture_drives_the_production_lowering` | the fixture's declared shape (id, origin, loop mode, node, every visibility key and tick, the one gameplay marker and that it is a gameplay cue) and that `lower_clip` produces a runtime clip with that visibility channel, whose evaluated state at the break tick is `Hidden` with `collider_enabled() == false` — the ECS path is driven by the evaluator, not by a test-authored value |
| `accept_f20_c_03_a_hidden_node_stays_hidden_across_a_lod_selection_pass` (minimum) | the wired fixed-tick entry at the hide tick applies `Hidden` and the composed verdict is not drawn with `NoCollider`, the gameplay cue fired once; then the **real** `select_lod_presentation` rewrites `NodePresentation` (asserted: `LodCulled` at a far distance, `Drawn` again at a near one) and the verdict stays not drawn with `NoCollider` both times, reporting LOD's own reason at the far distance |
| `accept_f20_c_03_showing_the_node_again_is_the_symmetric_verdict` | at the show tick the record is `Visible`, the verdict is `Drawn`/`Undecided`, the loop's second pass re-fires nothing, and a far-distance LOD pass culls it with `Undecided` again: the clip stops deciding collision the moment it stops hiding |
| `accept_f20_c_03_a_destroyed_node_is_never_restored_by_a_loop_pass_or_an_lod_pass` | damage's `NodeDisabled` marker outranks the clip: over four loop passes — every re-show tick included, and the count of those re-shows is asserted so the test cannot pass vacuously — the verdict is `Disabled` and never drawn, the marker is never touched, the one-shot cue still fires once, a mesh under the destroyed part is not drawn, two distances change nothing, and the teardown does not either |
| `accept_f20_c_03_a_released_instance_hands_a_live_node_back_to_lod` | the mirror: the teardown releases the record, an undamaged node returns to LOD's `Drawn`/`Undecided`, and a later advance of the same track drives nothing |
| `accept_f20_c_03_the_visibility_verdict_is_written_only_when_it_changes` | idempotence observed through a real `On<Insert, NodeAnimatedVisibility>` counter: the break tick inserts once, a second advance of the same tick inserts nothing, the real LOD pass inserts nothing, and the show tick inserts again (2) — a value comparison could not have told "written again" from "not written" |
| `accept_f20_c_03_an_unbound_or_stale_generation_entity_is_never_written` | of four entities, only the verified binding receives the record: a superseded generation, an entity with no binding and a binding to a node the clip does not drive keep `None` and stay drawn |

## Mutation probes

Run locally with a rerunnable driver: each probe edited one production file,
ran the selection `cargo test -p cs_app --locked --test accept_f20_c_03_visibility_lod_ownership`
(exit 101 each time), then restored the file with `git checkout --`. `grep -rn "MUTATION PROBE"
crates/` returns nothing, `git status` shows no probe edit, and the selection is green again.

| probe | edit | tests that fail (of 7) |
| --- | --- | --- |
| P1 the visibility write removed | the `if let Some(visibility) = state.visibility()` block in `advance_animation` reads into `_visibility` and writes nothing | 6 (everything but the fixture test) |
| P2 the composition ignores damage and LOD | both `Some(PresentationState::…)` arms in `VisibilityVerdict::compose` fall through to `DrawVerdict::Drawn`, so only the clip decides | 4: `..._a_hidden_node_stays_hidden_across_a_lod_selection_pass`, `..._showing_the_node_again_is_the_symmetric_verdict`, `..._a_released_instance_hands_a_live_node_back_to_lod`, `..._a_destroyed_node_is_never_restored_...` |
| P3 the composition ignores the clip | `let hidden = false` in `VisibilityVerdict::compose` | 4: `..._a_hidden_node_stays_hidden_across_a_lod_selection_pass`, `..._showing_the_node_again_is_the_symmetric_verdict`, `..._the_visibility_verdict_is_written_only_when_it_changes`, `..._a_released_instance_hands_a_live_node_back_to_lod` |
| P4 the teardown keeps the record | `release_instance` no longer removes `NodeAnimatedVisibility` | 2: `..._a_released_instance_hands_a_live_node_back_to_lod`, `..._a_destroyed_node_is_never_restored_...` |
| P5 the collider half ignores the hide | `VisibilityVerdict::compose` always answers `ColliderVerdict::Undecided` | 2: `..._a_hidden_node_stays_hidden_across_a_lod_selection_pass`, `..._the_visibility_verdict_is_written_only_when_it_changes` |
| P6 the write is not idempotent | `apply_write` inserts the visibility record unconditionally instead of through `insert_changed` | 1: `..._the_visibility_verdict_is_written_only_when_it_changes` |

## Checks run

- `cargo fmt --all -- --check` — exit 0.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  — exit 0.
- `cargo test --workspace --locked` — exit 0, no failing suite (248 suites ok, 0
  failed). The 7 `accept_f20_a_*`, 7 `accept_f20_b_*`, 8 `accept_f20_c_01_*`
  and 9 `accept_f20_c_02_*` tests are unchanged and still pass; the whole
  `accept_f20_*` selection is 47 tests, 0 failed.
- `cargo test --workspace --locked -- accept_f20_c_03_ --include-ignored` —
  exit 0, **7 tests matched** in
  `crates/cs_app/tests/accept_f20_c_03_visibility_lod_ownership.rs`, all
  passing; none of them is `#[ignore]`d.
- the six mutation probes above — each probe's selection exited 101 and the
  files were restored.

No command needed `CS_GAME_DIR`, and `CS_CAPABILITIES`
(`retail,gpu,audio`) was not exercised: this stage reads no original data.

## Unknowns

- **The original's coupling of visibility to collision is unknown** and stays
  recorded as unknown (F20-A finding): F20-A's `hidden ⇒ no collider` is this
  engine's designed rule, adopted unchanged here, with no original evidence.
  F20-D measures it or nothing may claim it.
- Whether an original visibility swap hides the node's **subtree** (decision
  7) and whether the original ever paired a destruction transition with a
  gameplay marker at the same tick are unmeasured with the container layouts
  (F13).
- The original tick rate an animation ran at, and whether its markers ran
  before or after physics, stay F20-C.02's unmeasured placement.
- The mission-marker consumer, the F17 render consumer of
  `NodePresentation` and the physics consumer of a collider verdict are all
  absent from the crate; they are filed as follow-up tasks (see "Follow-ups").

## Follow-ups filed

* **#503 `F20-C-visibility-draw-consumer`** — the render-side draw consumer:
  nothing reads `NodePresentation` today, so the composed verdict's
  `drawn()` has no implementation. It must read the composed verdict rather
  than re-derive the priority.
* **#504 `F20-C-visibility-collider-consumer`** — the collision side:
  `ColliderVerdict::NoCollider` has no reader because no collision-enable
  record exists in the engine (`grep -rn CollisionEnabled crates/` is empty).
  The task carries the decision of which record a hidden node's collision
  state belongs to, and the unknown original coupling with it.
* The spawn wiring and the `CommittedSessionTick` driver stay F20-C's (recorded
  as a note on task #75, as F20-C.02 already recorded them).

## Evidence

Synthetic fixtures and designed contracts only. No original-data, visual,
audible or ordinary-play claim; this stage can award at most **checked**.

## Sources

- `specs/F20-object-animation-and-authored-destruction-states.md`
  (`### F20-C`, behavior 3), `docs/contracts/IDENTITY-CONTENT.md`.
- `docs/findings/2026-09-30-f20-a-animation-channels-and-event-markers.md`
  (the designed `hidden ⇒ no collider` rule and the unknown original
  coupling),
- `docs/findings/2026-09-30-f20-b-transform-material-attachment-tracks.md`
  (boundary 2: the visibility ownership decision this stage makes),
- `docs/findings/2026-09-30-f20-c-01-attachment-hierarchy-and-detach-velocity.md`
  (the consumer-half pattern, the once-per-change bookkeeping),
- `docs/findings/2026-10-02-f20-c-02-fixed-tick-instances-and-teardown.md`
  (the fixed-tick entry and the instance teardown this stage writes into),
- `crates/cs_app/src/scene.rs` (`select_lod_presentation`, `NodePresentation`,
  `NodeDisabled`, `LodDistance`, `NodeLodVariant` — read-only here) and
  `crates/cs_content/src/scene.rs` (`select_lod_variant`, `LodInfo`).
