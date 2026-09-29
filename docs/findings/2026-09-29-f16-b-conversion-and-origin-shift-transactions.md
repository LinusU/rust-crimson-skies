# F16-B: conversion and origin-shift transactions

Date: 2026-09-29. Task: F16-B "Implement conversion and origin-shift
transactions"
(`specs/F16-coordinates-units-origin-management-and-clocks.md`, section
`### F16-B`). Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`. Required
capability: ordinary build/test. The machine also has `retail`, `gpu` and
`audio`; **none was used** — this stage reads no original data, renders
nothing and plays nothing, so no `private/evidence/` report is produced.

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/origin.rs` (extended): the F16-A names stay, and F16-B
  adds the transaction types
  - `SweptSegment` (`from_world`, `from_local`) — both endpoints of one
    record's last movement, in world (f64) and local (f32) form;
  - `SpatialAnchor` (`new`, `from_local`, `world`, `local`, `epoch`, `sweep`,
    `advance_local`, `move_to`, `teleport`, `rebase`, `rebased`) — one spatial
    record: invariant f64 world identity, f32 local pose in the epoch of the
    frame that produced it, and its swept segment;
  - `OriginShift` (`rebase`, `new`, `from`, `to`, `change`, `apply`) — one
    origin rebase applied to a whole set of records, atomically;
  - `OriginError::EpochMismatch { anchor, frame }` — a record or shift that
    names a frame it does not belong to is refused, never dragged.
  - four `accept_f16_b_*` unit tests.
- `crates/cs_app/tests/accept_f16_b_origin_shift.rs` (new): the AC02 minimum
  scenario — a rebase during a projectile flight *and* a docking approach —
  plus the swept-continuity test, over production code only.
- Wiring only (no logic): `crates/cs_app/src/lib.rs`'s `pub mod origin;` doc
  paragraph now names the F16-B transaction.

**One observable failure:** if `OriginShift::apply` did not re-anchor the f32
local cache into the new epoch (leaving the old local coordinates, or
converting with the old origin), `accept_f16_b_rebase_converts_every_local_cache_without_moving_world_identity`
fails: the world position read back from the stale local cache is off by the
origin offset (hundreds of metres). An implementation that instead *cleared*
the swept segment on a rebase (a teleport in disguise) fails
`accept_f16_b_teleport_invalidates_sweep_while_rebase_preserves_it` and
`accept_f16_b_rebase_preserves_swept_continuity_during_flight`. Mutation
probes (below) confirm both.

## Design decisions

- **"Conversion" is the frame change itself, done once per record.** F16-A
  declared the world → local (`WorldOrigin::local_of`) and local → world
  (`world_of`) maps and the per-format adapters. F16-B turns those maps into a
  *transaction* over a whole set of records: each record's invariant f64
  `WorldPosition` is kept, and only its f32 local cache is recomputed through
  the new frame. There is no second conversion path and no hidden state.
- **A record's local pose is bound to its epoch.** `SpatialAnchor` stores the
  `OriginEpoch` its local cache was produced in. `advance_local`, `move_to`
  and `teleport` refuse an origin of any other epoch with
  `OriginError::EpochMismatch`, so a stale local coordinate can never be
  silently combined with a frame that did not produce it.
- **Rebase and teleport are different operations, and both are typed.**
  `advance_local` / `move_to` record the previous endpoint as a
  `SweptSegment`; `teleport` discards it. `rebase` keeps the segment and
  converts *both* endpoints into the new frame — which is what "rebase
  preserves swept continuity and world identity" means operationally
  (non-negotiable behavior 5). A test asserts a rebase keeps the pre-existing
  segment (not one a later step recreated) and that a teleport clears it.
- **The transaction is atomic (plan then commit).** `OriginShift::apply`
  first checks every record is in the `from` epoch and converts it into a
  fully built replacement; only when *all* of them succeed does it write them
  back. A refused conversion (an f32 overflow in the new frame) therefore
  leaves every record — including the ones that would have converted — and the
  origin itself byte-for-byte unchanged, so a consumer can never observe a
  half-rebased set.
- **Epochs only move forwards, and never wrap.** `OriginShift::new` and
  `SpatialAnchor::rebased` require `to.epoch() > from.epoch()`; equal or older
  frames are `EpochMismatch`. `WorldOrigin::rebased` uses `checked_add`, so the
  epoch counter can never wrap and reuse an epoch for a different frame
  (`OriginError::EpochExhausted`).
- **The transaction stays Bevy-free.** `cs_app::origin` is plain Rust over
  `cs_types::space`; binding anchors to bodies, projectiles, triggers, AI
  paths, audio and camera histories is F16-C. This keeps the F16-B proof
  independent of the engine and cheap to test at fixed ticks.
- **The minimum scenario is a real fixed-tick integration, not a round trip.**
  The integration test integrates two anchors with `advance_local` every tick
  (a projectile and a docking ship), rebases mid-flight, and asserts the
  rebased world trajectory equals the un-rebased run tick for tick, that the
  docking outcome (arrival at the carrier) is unchanged, and that the rebase
  tick carries no velocity impulse. Every delta and offset is an exact dyadic
  fraction, so the f32 integration is exact and the declared tolerance
  (`1e-3 m`) only has to absorb the one rounding a frame change introduces — a
  rebase that forgot the frame change moves a body by more than 100 m.

## Test inventory (`accept_f16_b_`)

6 tests, all selecting production code: 4 unit tests inside `origin.rs` and 2
in the integration file.

| Test | Covers |
| --- | --- |
| `cs_app::origin::tests::accept_f16_b_rebase_converts_every_local_cache_without_moving_world_identity` | **minimum mechanism**: `apply` converts every local cache and both endpoints of a sweep, keeps world identity, advances the epoch once, and movement continues from the recorded endpoint afterwards |
| `cs_app::origin::tests::accept_f16_b_refused_conversion_leaves_every_record_unchanged` | atomicity: an f32 overflow in the new frame is refused with its field name, and *no* record (not even one that converted) nor epoch is mutated |
| `cs_app::origin::tests::accept_f16_b_stale_and_foreign_epochs_are_refused` | a foreign-epoch record is not dragged into a shift; movement uses the record's own frame; a shift/rebase must move forwards |
| `cs_app::origin::tests::accept_f16_b_teleport_invalidates_sweep_while_rebase_preserves_it` | non-negotiable 5 for a real record: rebase keeps the swept segment (both endpoints shift together), teleport discards it |
| `crates/cs_app/tests/accept_f16_b_origin_shift.rs::accept_f16_b_rebase_during_projectile_flight_and_docking_approach_matches_unrebased_run` | **AC02 minimum scenario**: rebase during a projectile flight and docking approach; every tick and the docking outcome equal the un-rebased run, with no velocity impulse at the rebase tick |
| `…accept_f16_b_rebase_preserves_swept_continuity_during_flight` | the segment that already existed before the rebase survives, its world endpoint is unchanged, its local endpoint is addressed in the new frame, and the next step continues from the previous tick |

## Mutation probes (implementation neutered → tests fail; all reverted and byte-compared)

Each probe was applied to `crates/cs_app/src/origin.rs`, the full
`accept_f16_b_` selection run with `--no-fail-fast`
(`cargo test --workspace --locked --no-fail-fast -- accept_f16_b_
--include-ignored`, exit 101 each time), then the file restored and its
sha256 checked. No `MUTATION PROBE` marker is left in the tree.

| # | Edit | Result (of the 6 tests) |
| --- | --- | --- |
| 1 | `OriginShift::apply` plans but never commits (`*anchor = rebased` removed) | 4 fail: both integration tests plus `…rebase_converts…` and `…teleport_invalidates…`; `…stale_and_foreign_epochs…` and `…refused_conversion…` still pass (they assert pre-commit refusals) |
| 2 | `SpatialAnchor::rebased` keeps the stale local (`local = self.local`) | 3 fail: `…rebase_converts…`, `…refused_conversion…` (the overflow is no longer detected) and the AC02 integration test |
| 3 | `SpatialAnchor::rebased` clears the sweep (`sweep = None`) | 3 fail: `…rebase_converts…`, `…teleport_invalidates…` and `…preserves_swept_continuity…`; the AC02 trajectory test still passes, which is exactly why the continuity test exists |

`sha256(crates/cs_app/src/origin.rs)` before and after every probe:
`ef62688e403dfca55dcd02ef8c215014a734d5f18ccae88e5c5df7e40153e6ab`.

## Recorded unknowns (recorded, not guessed)

- **Which subsystems need rebasing, and when, is F16-C's.** `OriginShift` is
  the primitive; binding anchors to bodies, projectiles, triggers, AI paths,
  audio and camera histories, choosing the rebase trigger and owning teardown
  is the F16-C integration. Nothing here asserts a rebase policy.
- **Original scale, handedness, axis order and angle units remain
  unmeasured.** F16-B inherits F16-A's position exactly: no original datum is
  read, no source convention is claimed, and the measurement with three
  independent landmarks is F16-D's. The `docs/research/FINDINGS.md` and
  `docs/findings/2026-09-28-f10-a-lossless-mesh-ir-and-strip-fixtures.md`
  open questions are unchanged.
- **The rebase threshold and the fixed tick rate are designed defaults.**
  240 ticks at 64 Hz and a 512 m origin offset are the test scenario's
  parameters, not original behavior. F16-C/F16-D can revise the policy
  through an approved design update; the transaction itself does not depend on
  them.

None of these is a new task — F16-C and F16-D already cover them — so
`create_tasks` was not used.

## Commands run

All commands from the repository root on branch
`rally/66-implement-conversion-and-origin-shift-tr`, Rust 1.98.1, based on
`origin/main` (`1002399`).

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_f16_b_ --include-ignored` | 0 (**6 tests**, all passing) |
| mutation probes 1–3 (see above) | 0 driver; each probe's selection exited 101; `origin.rs` restored byte-identical |

No command needed `CS_GAME_DIR`, and no `accept_f16_b_` test is `#[ignore]`d;
`CS_CAPABILITIES` (`retail,gpu,audio`) was not exercised by this stage.

## Wiring edits (outside owner paths, logic-free)

- `crates/cs_app/src/lib.rs`: the existing `pub mod origin;` doc paragraph now
  names the F16-B atomic transaction. No `Cargo.toml` change was needed:
  `origin.rs` uses only `cs_types` and `std`, and `cs_app` already depends on
  `cs_types`.

No protected path, original datum or binary file is involved.
