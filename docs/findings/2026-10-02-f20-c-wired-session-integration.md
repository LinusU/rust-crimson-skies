# F20-C: the session producers — spawn-side binding and the committed-tick driver

Date: 2026-10-02. Task: F20-C "Wire stateful animated props and destruction
transitions" (#75). Spec:
`specs/F20-object-animation-and-authored-destruction-states.md`, section
`### F20-C`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: ordinary build/test only — no `CS_GAME_DIR` read, no render,
no audio, so no `private/evidence/` report is produced.

This is the **integration** step of F20-C. Its subtasks built the consumers
(`.01` attachment hierarchy and detach velocity, `.02` fixed-tick instances and
teardown, `.03` visibility/LOD/damage ownership); this slice adds the two
**producers** the stage was still missing and composes them with the real
`PhysicsSession`.

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/animation/binding.rs` (**new**): the spawn-side producer —
  `bind_animated_node` and `AnimatedNodeBindError`. The one entry a scene spawn
  path calls per animated node it creates: it validates the entity, validates
  that the clip drives the node, starts the `(track, instance)` through the same
  `play_animation` boundary, verifies the live instance serves the binding's
  scene generation, and only then writes the generation-stamped
  `AnimatedNodeBinding`.
- `crates/cs_app/src/animation/schedule.rs` (extended): `commit_session_tick`
  (the session driver's writer — copies the F23-A `PhysicsTickLedger`'s
  committed fixed-tick count into `CommittedSessionTick`) and `AnimationPlugin`
  (the one-stop production composition: driver plus the existing
  `AnimationSchedulePlugin`).
- `crates/cs_app/src/animation/mod.rs` (wiring only): the `binding` module
  declaration, the re-exports (`AnimatedNodeBindError`, `bind_animated_node`,
  `AnimationPlugin`, `commit_session_tick`) and one doc paragraph.
- `crates/cs_app/src/lib.rs` (wiring only): one doc paragraph.
- `crates/cs_app/tests/accept_f20_c_wired_session.rs` (**new**): the seven
  `accept_f20_c_*` integration tests below.
- This file.

**One observable failure (before editing):** after F20-C.01/.02/.03 a real
session could still play an instance and drive **no entity at all**, and its
fixed loop could advance **no animation at all**:

- nothing in the crate produced an `AnimatedNodeBinding` — F20-C.02 recorded
  that "the spawn/despawn wiring is F20-C's", so the *verified-binding* path
  had no producer;
- nothing wrote `CommittedSessionTick`, so `AnimationSchedulePlugin` installed
  in a real `PhysicsSession` advanced nothing: `AnimationPlayback::advances()`
  stayed `0` however many fixed ticks ran (F20-C.02 recorded "the session driver
  that writes `CommittedSessionTick` does not exist yet").

Both are shown by
`accept_f20_c_the_wired_plugin_and_binder_drive_a_bound_node`: it fails on a
playback whose `advances()` never moves, and on a bound door whose
`NodeAnimatedPose` is never written.

## Designed decisions (this feature is designed, not original-verified)

The original `mis_anim.zbd` / `cam_anim.zbd` layouts are still undecoded (F13):
every rule below is **designed**, and no original behavior is claimed. F20-D
keeps the original-family validation gate.

1. **`bind_animated_node` is the spawn-side producer, and it validates in the
   order the spawn path can act on.** The scene loader — not this module —
   knows the node id, the generation and the instance it assigned, so the entry
   is a plain function, not a system: it takes those three plus the declared
   clip and performs one atomic step. It refuses, in order:
   - `UnknownEntity` — the entity is not part of this world, so there is
     nothing to bind;
   - `NodeNotDriven` — the clip has no channel on the node. A binding to a node
     the clip does not drive would drive nothing forever, so it is refused
     instead of written (the same "do not bind to nothing" rule the verified
     application uses);
   - `Play(_)` — the `play_animation` boundary refused the start (no
     `AnimationPlayback` in the world, a clip that does not lower, an exhausted
     producer serial);
   - `StaleInstance` — the identity is live but serves another scene
     generation, i.e. it is a superseded instance the load path should have
     released first. Refusing here surfaces the stale state at the producer
     instead of letting the next advance silently ignore the binding.
   `InstanceMissing` is unreachable after a successful start and is kept so a
   future drift between the start and the lookup is refused rather than binding
   to an instance that is not there.
2. **`AlreadyPlaying` for the same identity is the ordinary multi-node case,
   not an error.** One clip can drive several nodes (a door frame and its
   panel); the spawn path calls the entry node by node, and the second call
   starts nothing because the instance already plays the very identity it is
   binding. Every *other* play error propagates. A binding for a *different*
   identity is a different live instance and starts normally (F20-C.02's
   per-instance identity).
3. **The binding is written only after both the start and the generation check
   hold**, so a refused binding leaves the entity exactly as it was, and the
   live instance it would have named is not left half-bound.
4. **The driver reads the F23-A physics ledger, and introduces no second
   clock.** `PhysicsTickLedger.ticks` (in the read-only
   `crates/cs_app/src/physics/adapter.rs`) is the world's authoritative count
   of fixed steps a session has run — `PhysicsSession` pumps the world and the
   ledger counts every step. `commit_session_tick` copies that counter into
   `CommittedSessionTick`; it never reads `Time<Fixed>`, never advances time and
   never invents a tick. With no ledger (no physics session) it commits
   nothing, and the schedule then advances nothing — the existing "no session,
   no animation" rule.
5. **`AnimationPlugin` is one-stop and is added once.** It installs
   `AnimationSchedulePlugin` and registers `commit_session_tick` in
   `FixedPostUpdate`, `.after(PhysicsSystems::StepSimulation)` and
   `.before(advance_animation_on_session_tick)`, so the advance in the same
   fixed tick reads the tick the physics step just committed. Adding it twice,
   or adding `AnimationSchedulePlugin` beside it, would double-install the
   advance; the module doc says so. A caller that wants only the schedule keeps
   `AnimationSchedulePlugin` — and then commits no tick and advances nothing,
   which is the honest state of a world with no session driver.
6. **The placement after physics stays the designed F20-C.02 placement.**
   **The original order is unmeasured** (F20-A's unknowns: whether marker
   firing ran before or after physics). This slice keeps the F20-C.02 choice
   and does not claim original order; F20-D's probe is where it gets measured.
7. **The mission-marker consumer is still absent, and is not stubbed.**
   `AnimationLog`'s gameplay markers (`event.effect.is_gameplay()`) now reach
   the log from a real session, but the layer that turns them into gameplay is
   the mission/objective layer (F37/F39), which does not exist. Inventing a
   parallel "marker registry" here would be a guess about a layer this task
   does not own. The seam is `AnimationLog::drain`, and it is filed as #507.

## What is still not wired, and who owns it

Three call sites remain outside this task's owner paths and are filed rather
than edited (the same pattern F20-C.02 used for its own missing caller):

- **the scene spawn path** (`crates/cs_app/src/scene.rs`) does not call
  `bind_animated_node`. The scene loader owns node creation and instance
  assignment and that file is outside the owner paths; the entry is provided
  and exercised against the **real** `PhysicsSession` via its `configure`
  seam instead.
- **the scene load/despawn path** (`crates/cs_app/src/scene.rs`) does not call
  `release_superseded_instances` / `release_attachments_before_despawn` in the
  step that despawns a superseded scene — non-negotiable behavior 4's
  release-before-despawn ordering. Filed as #508.
- **the production app composition** that adds `AnimationPlugin` to the session
  (`PhysicsSessionBuilder::configure` call site) does not exist. Filed as
  #509.
- **the mission/objective marker consumer** of `AnimationLog` (boundary 5).
  Filed as #507.

## Tests

`crates/cs_app/tests/accept_f20_c_wired_session.rs`, all prefixed
`accept_f20_c_`. Every test calls production code (`bind_animated_node`,
`play_animation`, `advance_animation`,
`AnimationPlayback`/`AnimationLog`, and, in the first, the real
`PhysicsSession` + `AnimationPlugin`).

| test | what it pins |
| --- | --- |
| `accept_f20_c_the_wired_plugin_and_binder_drive_a_bound_node` (minimum + AC01) | `AnimationPlugin` inside the **real** `PhysicsSession` (installed through `configure`): the driver commits the ledger's first fixed tick, the schedule advances through it, and the production spawn entry binds the door. The door's `NodeAnimatedPose` answers both the mesh and the collider question with one stored value and reaches the authored open pose at the open tick; its gameplay marker fires exactly once; later ticks change nothing |
| `accept_f20_c_a_looping_propeller_fires_its_one_shot_marker_once` (AC02, behavior 1) | a looping rotor bound by the production spawn entry advances once per fixed tick across several loop passes: clip time is monotone, the current pose is presented, `engine_started` fires exactly once, and `blade_pass` fires once per completed loop pass |
| `accept_f20_c_a_skipped_animation_reaches_its_final_state_exactly_once` (AC04) | a skip is one advance past the marker: the final authored pose is reached, `door_opened` fires exactly once, arriving at the same final state again publishes nothing, and a one-shot clip reports itself finished |
| `accept_f20_c_detaching_cargo_from_a_moving_parent_inherits_its_velocity` (AC03, minimum) | cargo bound by the production entry, attached to a moving bay and detached at the authored tick through the production consumer: the `ChildOf` link is gone, linear velocity is `v + ω × r`, spin is inherited, and the velocity is written exactly once (a later write is not overwritten) |
| `accept_f20_c_the_binder_refuses_a_node_the_clip_does_not_drive` | a binding to a node the clip does not drive is refused with `NodeNotDriven`, writes no binding and starts no instance |
| `accept_f20_c_the_binder_propagates_missing_entity_session_and_stale_instance` | the producer propagates every failure instead of binding: `Play(NoSession)` with no playback, `UnknownEntity` for a despawned entity, and `StaleInstance` for a live identity serving another generation (with no binding written) |
| `accept_f20_c_stopping_an_instance_releases_it_and_a_rebind_plays_again` | teardown/retry: stopping releases the applied pose and the binding and removes the instance; a rebind of the same identity starts a fresh evaluator with a fresh producer serial whose one-shot marker fires once more |

## Mutation probes

Each probe edited one production file, ran the selection
`cargo test -p cs_app --locked --test accept_f20_c_wired_session -- <test>`
(exit 101 each time), then restored the file byte for byte (`cmp` clean
against the backups). The selection was green again afterwards.

| probe | edit | test that fails (of 7) |
| --- | --- | --- |
| A the producer binds to anything | remove the `NodeNotDriven` guard from `bind_animated_node` | `..._the_binder_refuses_a_node_the_clip_does_not_drive` |
| B the producer ignores the generation | remove the `StaleInstance`/`InstanceMissing` lookup from `bind_animated_node` | `..._the_binder_propagates_missing_entity_session_and_stale_instance` |
| C the driver commits nothing | `commit_session_tick` stops writing `CommittedSessionTick` | `..._the_wired_plugin_and_binder_drive_a_bound_node` |
| D the composition installs no schedule | `AnimationPlugin` drops `AnimationSchedulePlugin` | `..._the_wired_plugin_and_binder_drive_a_bound_node` |

## Checks run

- `cargo fmt --all -- --check` — exit 0.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  — exit 0.
- `cargo test --workspace --locked` — exit 0 (251 test suites ok, 0 failed).
- `cargo test --workspace --locked -- accept_f20_c_ --include-ignored` —
  exit 0, **32 tests matched** across the four `accept_f20_c_*` files (8 `.01` +
  9 `.02` + 8 `.03` + 7 this slice), all passing.
- the four mutation probes above — each exited 101 and the files were restored.

No command needed `CS_GAME_DIR`; no `accept_f20_c_*` test is `#[ignore]`d;
`CS_CAPABILITIES` (`retail,gpu,audio`) was not exercised.

## Evidence

Synthetic fixtures and designed contracts only. No original-data, visual,
audible or ordinary-play claim: this stage can award at most **checked**, and
the reviewer must inspect the runtime wiring, the error paths, the stale-state
handling and the test sensitivity. The mission-marker consumer does not exist
(boundary 5 is filed as #507), and the scene spawn/despawn callers and the
production app composition are outside the owner paths (#508, #509).

## Sources

- `specs/F20-object-animation-and-authored-destruction-states.md`
  (`### F20-C`; behaviors 1, 3, 4 and 5; AC01–AC04),
  `docs/contracts/IDENTITY-CONTENT.md` (scene/session generations).
- `docs/findings/2026-10-02-f20-c-02-fixed-tick-instances-and-teardown.md`
  (the `CommittedSessionTick` stamp, the schedule placement and the missing
  session driver / spawn wiring this slice fills),
  `docs/findings/2026-09-30-f20-c-01-attachment-hierarchy-and-detach-velocity.md`
  (the release/despawn ordering and the detach-velocity rule the teardown and
  the tests use),
  `docs/findings/2026-10-02-f20-c-03-visibility-lod-damage-ownership.md`
  (the consumer half this slice composes with),
  `docs/findings/2026-09-30-f20-b-transform-material-attachment-tracks.md`
  (boundaries 3 and 4),
  `docs/findings/2026-09-30-f20-a-animation-channels-and-event-markers.md`
  (finding 5: the caller supplies the session tick; the unmeasured placement
  order).
- `crates/cs_app/src/physics/session.rs` (`PhysicsSessionBuilder::configure`)
  and `crates/cs_app/src/physics/adapter.rs` (`PhysicsTickLedger`) — read-only
  for this task.
