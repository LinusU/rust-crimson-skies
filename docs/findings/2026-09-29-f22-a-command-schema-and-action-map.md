# F22-A: Command schema and action map

Date: 2026-09-29. Task: F22-A "Define command schema and action map"
(`specs/F22-input-bindings-devices-and-control-ownership.md`, section
`### F22-A`). Shared contract: `docs/contracts/UI-NETWORK.md`. Capabilities
used: ordinary build/test only (no `CS_GAME_DIR` read, no evidence report
required).

## Files and the one observable failure (listed before editing)

- `crates/cs_types/src/input.rs` (new): the dependency-free command schema —
  `DeviceClass`, `DeviceIdentity`/`DeviceId`, the device vocabulary
  (`Key`, `MouseButton`, `MouseAxis`, `GamepadButton`, `GamepadAxis`),
  `BindingSource`, `FlightCommand`, `UiAction`, `Action`, `InputContext`,
  `AxisValue`/`InputFrame`, `Binding`/`BindingTarget`/`ActionMap` and the
  `ActionMap::designed_default` map.
- `crates/cs_sim/src/control.rs` (new): the consumer —
  `AxisState` (continuous, held across ticks), `ControlBuffer`
  (`apply_frame`, `begin_tick`, `pending_edges`), `ControlAuthority`,
  `LocalSeatId` and `ControlGate` (exactly one authority, context-gated
  `resolve`).
- `crates/cs_app/src/input/mod.rs` (new): the app boundary —
  `InputBindings` (map + context) and `InputCollector` (per-render-frame
  edges into one `InputFrame`).
- `crates/cs_types/src/lib.rs`, `crates/cs_sim/src/lib.rs`,
  `crates/cs_app/src/lib.rs` (wiring only): module declarations and module
  documentation.
- `crates/cs_sim/tests/accept_f22_a_one_frame_key_edge.rs` (new): the
  cross-crate acceptance scenario.
- This file.

**One observable failure:** the render frame observes a key press once, but
the fixed-step loop runs several physics substeps for that frame; without an
edge queue that consumes each press exactly once, the press is either
re-fired on every substep (space fires the guns N times per frame) or lost.
`accept_f22_a_one_frame_key_edge_produces_one_action_across_substeps` fails
under either mutation.

## Designed vocabulary, not original data

Every type, label and binding in this stage is newly authored project design.
The following are **unknown** and are not guessed here:

- which commands the original 2000 PC game exposes and how it labels them;
- which keys, mouse buttons/axes, gamepad or joystick/HOTAS controls it binds
  to them (including the original default mouse-flight mapping);
- how the original game distributes control between local seats, the server
  and scripts across its modes.

Affected content: the whole F22 input surface. Resolving tasks already in the
queue: **F22-B** (device adapters and axis calibration), **F22-C** (focus, UI,
replay and control ownership wiring), **F22-D** (all declared device families
and original command coverage). `ActionMap::designed_default` is therefore an
explicitly designed default that exercises every device class; it is not a
claim about the original bindings, and F22-B/D may replace it per profile.

`DeviceIdentity::EnumerationFallback` encodes non-negotiable behavior 1
literally: an index-only identity can exist, but it reports `is_stable() ==
false` so calibration is not persisted against it. Which platform identity
string is actually stable on the original platform is F22-B's measurement.

## Tests

Task-test prefix `accept_f22_a_`:

- `cs_types::input` unit tests: context gating (text entry/cinematic emit
  nothing), conflict and malformed-target refusal, stable-vs-fallback device
  identity, axis quantization and validation, vocabulary/label round-trip.
- `cs_sim::control` unit tests: the minimum scenario, separate axis/edge
  buffering, stale-frame refusal, single-authority ownership.
- `crates/cs_sim/tests/accept_f22_a_one_frame_key_edge.rs`: the minimum
  scenario and its failure cases through the public production API.
- `cs_app::input` unit test: one press → one frame edge, text-entry gating,
  continuous targets are not emitted as edges.

Review (2026-09-29, reviewer `deepseek-1`, fresh context) found and fixed one
ordering bug in `ActionMap::resolve`: it returned the first binding for a
source even when that binding belonged to a different context, so a source
legitimately bound in two contexts (the map's documented "fire in flight,
confirm in a menu" case) resolved to nothing whenever the wrong-context
binding was inserted first. `resolve` now selects the binding the active
context accepts, in either insertion order. The regression test
`accept_f22_a_multi_context_binding_resolves_in_both_orders` covers both
orders and fails against the old behavior.

## Checks

Run locally before hand-over (all exit 0):

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_f22_a_ --include-ignored
```

The task-prefix selection discovers and runs 13 tests: 5 in
`cs_types::input`, 4 in `cs_sim::control`, 3 in
`crates/cs_sim/tests/accept_f22_a_one_frame_key_edge.rs`, and 1 in
`cs_app::input`. No test needs original data, so none is `#[ignore]`d.

## Mutation probe (test sensitivity)

Making `ControlBuffer::begin_tick` deliver every queued edge without
consuming it — the re-fire bug the minimum scenario exists to catch — fails
`accept_f22_a_one_frame_key_edge_produces_one_action_across_substeps` and
`accept_f22_a_out_of_order_frames_are_refused_and_late_edges_wait` (and the
integration twin of the first). The mutation was reverted; the probe is
documented here only to show the tests exercise the production behavior.
