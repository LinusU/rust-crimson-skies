# F16-C: frame clock bound to every spatial subsystem

Date: 2026-09-29. Task: F16-C "Integrate clocks and origin with every spatial
subsystem"
(`specs/F16-coordinates-units-origin-management-and-clocks.md`, section
`### F16-C`). Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`. Required
capability: ordinary build/test. The machine also reports `retail`, `gpu` and
`audio`; **none was used** — this stage reads no original data, renders nothing
and plays nothing, so no `private/evidence/` report is produced.

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/origin.rs` (extended): F16-A/F16-B names stay, and F16-C
  adds the frame→tick→world integration
  - `SpatialSubsystem` (`ALL`, `label`) — the six consumers a rebase must move:
    body, projectile, trigger, ai-path, audio, camera-history;
  - `SpatialId` — a stable, never-reused record id;
  - `SpatialError` (`Origin`, `NonFiniteDisplacement`, `InvalidRebaseLimit`,
    `RecordIdExhausted`, `UnknownRecord`) and its `Display`/`Error` impls;
  - `SpatialRecord` (`id`, `subsystem`, `world`, `local`, `epoch`, `sweep`,
    `per_tick_local`) — one subsystem's anchor plus the local displacement one
    fixed tick applies (a per-tick local delta, never a variable dt);
  - `SpatialWorld` (`new`, `origin`, `epoch`, `records`, `len`, `is_empty`,
    `record`, `world_position`, `spawn`, `advance`, `teleport`, `despawn`,
    `teardown`, `rebase`, `rebase_if_needed`) — every record of one session in
    one frame;
  - `RebasePolicy` (`DEFAULT_LIMIT_M = 512.0`, `at_limit`, `disabled`,
    `limit_m`, `trigger`, `Default`);
  - `DriverError` (`Time`, `Spatial`) and its `Display`/`Error`/`From` impls;
  - `FixedTickDriver` (`new`, `clock`, `world`, `world_mut`, `tick`, `rate`,
    `rebase_policy`, `rebase_count`, `first_rebase_tick`, `last_rebase_tick`,
    `set_paused`, `advance_frame`, `rebase`, `teleport`, `despawn`, `retry`);
  - three `accept_f16_c_*` unit tests.
- `crates/cs_app/tests/accept_f16_c_frame_rate_integration.rs` (new): the AC03
  minimum scenario — the same 2 s of wall time split into 30, 60 and 144 render
  frames, a rebase mid-run, pause/resume, and a refused rebase followed by
  `retry` — over production code only.
- Wiring only (no logic): `crates/cs_app/src/lib.rs`'s `pub mod origin;` doc
  paragraph now names the F16-C binding; `crates/cs_app/Cargo.toml` adds
  `cs_sim.workspace = true` and the root `Cargo.lock` gains that edge; nothing
  else moved.

**One observable failure:** if `FixedTickDriver::advance_frame` stepped the
world once per render frame instead of once per whole fixed tick (or let a
frame's wall delta become a variable dt), the 30, 60 and 144 FPS runs would
disagree — a 30 FPS frame carries more wall time than a 144 FPS one — so
`accept_f16_c_equal_input_at_30_60_144_fps_agrees_on_ticks_and_state` and
`accept_f16_c_rebase_lands_on_the_same_tick_at_every_frame_rate` fail (mutation
probe 1 below: 2 passed, 2 failed). If a refused rebase committed the origin
before the transaction, `accept_f16_c_refused_spatial_rebase_changes_nothing`
observes a half-rebased world (probe 3). If `spawn` stopped validating its
per-tick displacement, `accept_f16_c_spawn_validation_despawn_and_teardown`
fails (probe 4).

## Design decisions

- **One owner sees both halves of the integration.** The `cs_sim` clock and the
  F16 `SpatialAnchor`/`OriginShift` types previously lived in crates that could
  not see each other. `crates/cs_app/src/origin.rs` is the one owner path that
  can name both, so the driver lives there. `cs_app` already sits above every
  content crate in `docs/01-ARCHITECTURE.md`, so the new `cs_app → cs_sim` edge
  is downward and acyclic.
- **A render frame only ever yields whole fixed ticks.** `advance_frame`
  delegates to `SimClock::advance`, which accumulates fractional wall time and
  returns the number of *whole* ticks to commit. The driver then runs that many
  identical fixed steps. There is no variable-dt path: at 30, 60 and 144 FPS
  the same total wall time produces the same tick count and the same state
  (AC03).
- **The rebase policy is evaluated once per fixed tick, never per frame.** The
  loop calls `SpatialWorld::rebase_if_needed` immediately before each one-tick
  step, so a rebase lands on the same tick number at every frame rate. This is
  the property that makes frame-rate agreement hold *while* the origin moves,
  not just when it does not.
- **Per-tick displacement is local and frame-rate free.** Each record stores a
  per-tick local (`f32`) delta; `advance` applies it in the current frame. A
  real subsystem will replace it with an integrated velocity; F16-D calibrates
  that behaviour. Keeping it local means `advance` never reads a render delta.
- **Rebases reuse the F16-B atomic transaction.** `SpatialWorld::rebase` builds
  an `OriginShift`, converts every record into a candidate set, and only writes
  them back — and adopts the new origin — when *all* of them succeed. A refused
  conversion leaves every record and the origin byte-for-byte unchanged, so no
  consumer can observe a half-rebased world (unit test
  `accept_f16_c_refused_spatial_rebase_changes_nothing`).
- **Ids are never reused and teardown is explicit.** `despawn` removes one
  record; `teardown` clears the rest; `retry` installs a brand-new
  `SpatialWorld` at tick 0, so a fresh generation cannot inherit a stale
  trigger, origin epoch or camera history.
- **An error is terminal for the session.** `advance_frame` returns a typed
  `DriverError` instead of logging; the failed session is not resumed, and
  `retry(origin)` starts a fresh one. `SpatialWorld::advance` aborts the
  caller's tick loop rather than swallowing a refused step.
- **The rebase threshold is a designed default.** `DEFAULT_LIMIT_M = 512.0` and
  the 64 Hz fixture rate are development values, not original behaviour; the
  policy is injectable (`RebasePolicy::at_limit`, `disabled`) so F16-D can
  calibrate them without touching the driver.

## Test inventory (`accept_f16_c_`)

7 tests, all selecting production code: 3 unit tests inside `origin.rs` and 4
in the integration file. All are ordinary tests (none `#[ignore]`d) and none
needs `CS_GAME_DIR`.

| Test | Covers |
| --- | --- |
| `cs_app::origin::tests::accept_f16_c_spatial_world_rebases_every_subsystem_atomically` | **minimum mechanism**: all six subsystems live in one `SpatialWorld`; one `rebase` converts every record into the new epoch, keeps its world identity, re-anchors its local cache, and keeps a swept segment's world endpoint |
| `cs_app::origin::tests::accept_f16_c_refused_spatial_rebase_changes_nothing` | atomicity: an f32 overflow in the new frame is refused by field name and *no* record nor the origin is mutated |
| `cs_app::origin::tests::accept_f16_c_spawn_validation_despawn_and_teardown` | policy validation (zero/NaN limit), non-finite displacement refused before a record exists, unique ids, `despawn` not reusing an id, `teardown` |
| `crates/cs_app/tests/accept_f16_c_frame_rate_integration.rs::accept_f16_c_equal_input_at_30_60_144_fps_agrees_on_ticks_and_state` | **AC03 minimum scenario**: 2 s at 64 Hz = 128 ticks at 30, 60 and 144 FPS; frame splits differ; ticks, origin, rebase count/tick and every record's world+local state agree, and each local cache still converts back to its world position within `local_round_trip_tolerance_m` |
| `…accept_f16_c_rebase_lands_on_the_same_tick_at_every_frame_rate` | the rebase is part of the fixed-tick schedule: same first/last tick (mid-run) and same epoch at every frame rate |
| `…accept_f16_c_paused_session_advances_no_ticks_and_no_spatial_state` | pause integration: a frozen clock produces zero ticks and no movement, and resume banks no paused time |
| `…accept_f16_c_refused_rebase_is_propagated_and_retry_starts_fresh` | error propagation: a refused rebase surfaces as `DriverError::Spatial`, leaves the session untouched, and `retry` restarts a clean generation at tick 0 |

The fixture: 64 Hz, 2 s = 128 ticks, origin `[-240, 0, 0]` at epoch 0, a
`RebasePolicy::at_limit(256.0)`, and one record per `SpatialSubsystem::ALL`
with dyadic starts/deltas so the f32 integration is reproducible bit for bit.
It rebases twice (ticks 47 and 108 in the 128-tick run; final epoch 2),
identically at every frame rate. Cross-run agreement is asserted exactly (the
operations are deterministic); only the local↔world round trip uses the
declared `local_round_trip_tolerance_m`.

## Mutation probes (implementation neutered → tests fail; all reverted and byte-compared)

Each probe was applied to `crates/cs_app/src/origin.rs` by a rerunnable driver,
the full `accept_f16_c_` selection run with `--include-ignored`
(`cargo test --workspace --locked -- accept_f16_c_ --include-ignored`, exit 101
each time), then the file restored and its sha256 checked. No
`MUTATION PROBE` marker is left in the tree.

| # | Edit | Result of the 7 tests |
| --- | --- | --- |
| 1 | `advance_frame` runs one world step per frame (`for offset in 0..ticks.min(1)`) | 2 fail: both `accept_f16_c_equal_input_at_30_60_144_fps_agrees_on_ticks_and_state` and `accept_f16_c_rebase_lands_on_the_same_tick_at_every_frame_rate` |
| 3 | `SpatialWorld::rebase` commits the origin *before* the transaction (`self.origin = shift.to();` early) | 1 fail: `accept_f16_c_refused_spatial_rebase_changes_nothing` |
| 4 | `SpatialWorld::spawn` stops validating the per-tick displacement | 1 fail: `accept_f16_c_spawn_validation_despawn_and_teardown` |

`sha256(crates/cs_app/src/origin.rs)` before and after every probe:
`9bcc02ba81f0c20beb01446d388372d99166938664e06613e06f37158e8e08b7`.

## Recorded unknowns (recorded, not guessed)

- **Which real velocity integration each subsystem uses is F16-D's.**
  `SpatialRecord::per_tick_local` is a designed constant per-tick displacement,
  not a measured speed. F16-D calibrates a real fixed-step integration and the
  minimum scenario.
- **The rebase threshold, the fixed rate and the unit scale are designed
  defaults, not original behaviour.** 512 m, 64 Hz and metres are development
  choices; F16-A's note that original scale, handedness, axis order and angle
  units remain unmeasured is inherited. F16-D's three-landmark measurement is
  the calibration path.
- **This stage is not wired into a Bevy schedule.** The driver is plain Rust
  over `cs_sim`/`cs_types`; binding it to an app `FixedUpdate` schedule and
  real components is later fixed-step work (F13+). No claim is made that an
  original executable behaves this way.

None of these is a new task — F16-D and the fixed-step work already cover them
— so `create_tasks` was not used.

## Commands run

All commands from the repository root on branch
`rally/67-integrate-clocks-and-origin-with-every-s`, Rust 1.98.1, based on
`origin/main` (`dacd029`).

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_f16_c_ --include-ignored` | 0 (**7 tests**: 3 unit + 4 integration, all passing) |
| mutation probes 1, 3, 4 (above) | 0 driver; each probe's selection exited 101; `origin.rs` restored byte-identical |

No command needed `CS_GAME_DIR`, and no `accept_f16_c_` test is `#[ignore]`d;
`CS_CAPABILITIES` (`retail,gpu,audio`) was not exercised by this stage.

## Wiring edits (outside owner paths, logic-free)

- `crates/cs_app/src/lib.rs`: the existing `pub mod origin;` doc paragraph now
  names the F16-C frame-clock binding.
- `crates/cs_app/Cargo.toml`: `cs_sim.workspace = true` added (the driver uses
  `cs_sim::time`), and the root `Cargo.lock` records the new `cs_app → cs_sim`
  edge. No other dependency changed.

No protected path, original datum or binary file is involved.
