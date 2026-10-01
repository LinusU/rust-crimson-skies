# F36-A: interaction state machines and transfer policy

Task #144. Runtime contract `cs_sim::interaction`; declared schema
`cs_content::interaction`; lowering boundary `cs_app::interaction`. Tests:
`crates/cs_sim/tests/accept_f36_a_interaction.rs`,
`crates/cs_content/tests/accept_f36_a_interaction_schema.rs`,
`crates/cs_app/tests/accept_f36_a_interaction_boundary.rs`.

All behavior is **designed**, synthetic and not original-verified. Awards at
most *checked*. No original docking, pickup, boarding or plane-swap data exists
on this machine and none was used: the fixture is `Origin::SyntheticFixture`,
subject `mission/synthetic.interaction`, every `Resolved` carrying designed
provenance.

## One observable failure

Fly the initiator past the hook too fast (75 m/s relative against a 10 m/s
envelope) or from the wrong direction (a −X pass against a +X docking axis),
and the interaction **must not latch**: `evaluate_eligibility` returns
`TooFast`/`WrongDirection` and `InteractionTransaction::latch` leaves the
stage `Eligible` and returns the refusal. A build in which either pass latched
would show this.

## Designed semantics

- **Explicit state machine, one owner.** `InteractionState` is the canonical
  chain `Available → Approaching → Eligible → Latching → Transferring →
  Released → Completed`, with `Aborted` reachable from any non-terminal stage.
  `InteractionTransaction::control_owner` reports exactly one
  `ControlOwner` (`Initiator` before latch, `LatchController` during latch and
  transfer, the policy's owner after release), so "exactly one system owns
  pose/control during latch and release" is a value, not a convention
  (non-negotiable behavior 2).
- **Swept eligibility, not a radius.** `evaluate_eligibility` samples the
  target anchor through the F34 `anchor_sample` (so renderer, collision and
  eligibility read the same pose), sweeps the initiator's constant-velocity
  motion *relative to the anchor* over an interval and checks the closest
  approach over that interval, the relative speed, the approach direction
  against the anchor's declared axis, and the closing speed. A test
  (`accept_f36_a_sweep_reaches_a_hook_a_radius_test_misses`) places the
  initiator 8 m from a 5 m-radius hook but inside the sweep; a radius-only
  implementation refuses it, the production path accepts it — which is
  non-negotiable behavior 1.
- **Authorization gates completion.** `InteractionAuthorization` names the
  objective and the kind, and `InteractionId` binds initiator, target, session
  and serial. A completed interaction publishes exactly one typed
  `InteractionCompletion` (`Docked`, `PassengersDelivered`, `Boarded`,
  `AircraftSwapped`), so the mission completes only on the objective's own
  event, never because "a zeppelin was approached" (non-negotiable behavior 3).
- **Clean aborts, no duplicates.** `complete` re-validates the objective and
  the target's liveness; a changed phase or a destroyed target moves the
  transaction to `Aborted` and returns the reason without publishing effects.
  `abort(TargetDestroyed | CinematicCancelled | Pause | Retry | Disconnect |
  Cancelled)` applies `effects() == None`, is idempotent and cannot resume, so
  a retry cannot leave a duplicate pilot or cargo (non-negotiable behavior 4).
- **Per-transition transfer policy.** `TransferPolicy` declares velocity,
  pilot, inventory, camera and the control owner after release; `aircraft_swap`
  moves the pilot, inventory and camera to the new actor while `docking` moves
  none. Effects are derived from the policy only at completion
  (non-negotiable behavior 5's data half; the runtime rebind is F36-B/C).
- **Unknown stays unknown.** The declared envelope and authorization are
  `Resolved`; the boundary carries nothing through by guessing: an unknown
  objective is `InteractionLowerError::UnknownAuthorization`, an unknown
  envelope field is `InteractionLowerError::UnknownEligibility`, because an
  interaction cannot latch against an invented capture radius or mission
  phase.

## Not done here (deliberately)

- The moving-frame eligibility runtime across an origin rebase and the atomic
  ECS transfer: F36-B.
- The producer/consumer wiring (script spawns, objective completion events,
  HUD/input/camera rebinding) and teardown/retry propagation: F36-C.
- Original interaction validation: F36-D (needs `retail`).
- No Avian body, no renderer, no file access: `cs_sim` depends only on
  `cs_types` and `cs_script`.

## Unknowns

- The original envelope values (capture radius, speed, approach angle), the
  transition set and the per-transition transfer policy are unrecovered. Every
  number here (5 m, 10 m/s, 35°) is authored design, not a measured rule.
- Whether the original models docking as a swept eligibility test at all, and
  whether its completion is a distinct mission event, are unverified. A retail
  lead is required before any of this can become a fidelity claim; the
  affected content is every docking hook, passenger pickup, boarding point and
  plane swap in every mission.

## Files

`crates/cs_sim/src/interaction/{mod,state,eligibility,transaction,synthetic}.rs`;
`crates/cs_content/src/interaction.rs`; `crates/cs_app/src/interaction.rs`; the
three test files above; module wiring and doc bullets in each crate's `lib.rs`.
No `Cargo.toml` change was needed: the interaction contract names
`cs_sim::damage::ActorId`, which `cs_app` reaches through its existing
`cs_sim` dependency.
