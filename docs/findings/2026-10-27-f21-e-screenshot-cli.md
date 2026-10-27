# F21-E: the deterministic capture flags reach `cli::parse`

Date: 2026-10-27. Task: F21-E-SCREENSHOT-CLI (Rally #536) "Parse the
deterministic capture flags into a cs_app::camera::CaptureRequest".
Spec: `specs/F21-cameras-cockpit-views-and-spyglass.md`, non-negotiable
behavior 5. Shared contract: `docs/contracts/UI-NETWORK.md` and
`docs/contracts/CLI-EVIDENCE.md`. Capabilities used: ordinary build/test only —
no `CS_GAME_DIR` read, no GPU, no evidence report.

## Files

* `crates/cs_app/src/cli.rs`: the new flag arms (`--tick`, `--pose`,
  `--aspect`, `--view`, `--settings`), the capture context that validates them
  under `--synthetic --headless`, the `CliRequest::Capture` variant, the
  `capture_flag` exclusions on the `--joystick-probe`, `--resume-start-flow`
  and `--front-end` gates, `HELP_TEXT`, and the `Eq` derive dropped from
  `CliRequest` (`CaptureRequest` carries `f64` fields).
* `crates/cs_app/src/run.rs`: `CaptureRunRequest`, `CaptureRunReport`,
  `run_capture`, five new `RunError` variants, and the shared
  `step_and_trace` helper the two run modes use.
* `crates/cs_app/src/main.rs` (wiring only): dispatch of
  `CliRequest::Capture` and `print_capture_report`; no logic.
* `crates/cs_app/tests/camera/capture_cli.rs` (new) plus its `mod` entry and
  the doc paragraphs in `tests/camera/mod.rs`: the 10 `accept_f21_e_*` tests.
* This file.

Nothing under `crates/cs_app/src/camera/` changed: F21-C's records and
session are consumed as they stand.

## The grammar and the run mode

```text
cs --synthetic --headless --mission <id> --tick <n> --pose <x,y,z>
   [--aspect <w:h>] [--view cockpit|chase|spyglass] [--settings fixed]
   [--seed <n>] [--trace <file.jsonl>]
```

* Required: `--mission`, `--tick`, `--pose`. A bare mission key (`m01`)
  becomes `mission/m01` through `ContentId::from_source`; a namespaced value
  goes through `ContentId::parse`, and `CaptureRequest::build` still refuses
  a non-mission kind, mapped back to a `--mission` diagnostic.
* `--pose` is position only; the pinned `CameraPose` carries
  `Quaternion::IDENTITY` because the flags name no orientation.
* `--settings fixed` maps to `ComparisonSettings::comparison()` — the only
  value accepted, because an unpinned set cannot reproduce a frame.
* The request is pure: `cli::parse` inspects the argument vector only, reads
  no `CS_GAME_DIR` and no installation, exactly like `--ticks`/`--seed`.
* `--tick` is the run's own endpoint, so it cannot combine with `--ticks`;
  `--seed` and `--trace` keep their plain-run meaning.
* `run::run_capture` builds the same `SyntheticBodySpec` fixture, steps it to
  the capture's tick, drives one `CameraSession` frame over
  `declared_synthetic_camera_modes()` and returns the applied
  `CaptureReport`. The binary prints every `CaptureOverride` plus each
  `Narrowing` of the pinned `f64 -> f32` projection. **It draws nothing and
  produces no image** — the contract's rendered `--screenshot`/`--cam` path
  remains unimplemented and `HELP_TEXT` says so.

## Boundary decisions

* **Capture flags outside `--synthetic` are invalid naming the flag that
  needs it** — the same rule `--seed`, `--ticks` and `--trace` already
  follow, enforced before any mode dispatch so no other branch can swallow a
  capture flag (`--front-end --cs-path x --tick 5` previously would have
  parsed `--tick` as unknown; now every mode gate names the combination).
* **`--mission` under `--synthetic` selects the capture run**, so the two
  synthetic run modes stay disjoint (`--mission` + `--ticks` is refused
  because the capture's length is its tick); `--mission` without
  `--synthetic` stays the original-mission launch, unchanged.
* **Missing values for recognized flags keep the existing convention**:
  `CliRequest::Unsupported`, while malformed values are
  `CliRequest::Invalid` naming the flag and the bad value.
* **The headless viewport's fallback aspect is `AspectRatio::FOUR_THREE`** —
  the era's reference shape — used only when `--aspect` pins nothing; the
  report names whichever the frame used.
* **The synthetic body enters the frame with `OriginChange::Teleport` and a
  one-tick `elapsed`** — a first frame has no continuous prior pose, and the
  fixture integrates no angular velocity, so its rotation is the identity;
  the pinned pose overrides whatever the rig derives anyway.

## Test sensitivity

Four mutations, applied and reverted against production code, each run as a
single test in `cs_app_tests_c`:

1. The `--tick` arm accepts any string as tick 0 → **fails**:
   `accept_f21_e_a_non_numeric_tick_is_refused_naming_tick` (a capture of
   `"abc"` was silently built).
2. The `capture_flag && !synthetic` gate disabled → **fails**:
   `accept_f21_e_a_missing_required_flag_is_refused` case 4 —
   `--mission m01 --tick 1 --pose 0,0,0` fell through to the mission launch
   and refused `--cs-path`, so no capture flag was ever named.
3. `run_capture` steps to `tick - 1` → **fails**: the frame is produced at
   the wrong tick, the installed capture never applies and the run reports
   `RunError::CaptureMissed` — caught by
   `accept_f21_e_the_run_produces_the_applied_capture_report`.
4. `print_capture_report` stops printing the `overrides` list → **fails**:
   `accept_f21_e_the_cs_binary_prints_the_applied_report` — the real
   binary's stdout lost every `override` line and the test names the first
   missing field.

All were reverted; the tree is back to the implementation under review.

## Commands run (exit codes)

```text
cargo fmt --all -- --check                                        → 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings → 0
cargo test --locked -p cs_app                                     → 0
cargo test --locked -p cs_app -- accept_f21_e_ --include-ignored  → 0 (10 tests, all passed)
```

`cargo test --workspace` is left to CI per `AGENTS.md` (local full-workspace
runs cost the macOS scan per binary); `cs_app` is the only crate changed.

## Designed vs measured

Everything here is designed engine contract; nothing is original-verified:

| unknown | evidence | resolves in |
| --- | --- | --- |
| What the original's screenshot CLI was named or accepted | `--screenshot`/`--cam` appear in the CLI-EVIDENCE contract as future work; no original flag grammar was read or reproduced | the rendered-screenshot stage (`gpu` + `retail`) |
| Whether the original could freeze a pose for a screenshot at all | the seam pins a pose because the spec requires a reproducible frame, not because the original was observed doing so | F21-D |
| The mission id the capture names | the synthetic scene is not mission m01's world; the id travels with the request so the report is self-describing | a stage that maps missions to runnable worlds |
| What a real rendered capture writes (PNG, framebuffer) | this run prints the typed report only | F17/F21 capture-output stage |
