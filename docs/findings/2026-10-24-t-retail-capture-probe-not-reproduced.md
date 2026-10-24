# T-RETAIL-CAPTURE-PROBE (#1677): windowed capture probe failure not reproduced

Status: **not reproduced, not root-caused.** No code changed.

## Report

On origin/main at 804ee613f (and 7c1bc6038) on the retail+gpu+audio host, two
ignored tests failed because the `cs --mission M01` capture subprocess exited
nonzero with "the probe frame frame_00032_world.png holds only 0 of 3686400
pixels of the window's clear colour (0 per mille, less than the 20 required)":

- `campaign::m01_play_world_visible::accept_m01_play_world_windowed_mission_capture_draws_world_pixels`
- `campaign::vs_m01_rt_evidence_capture::accept_vs_m01_runtime_evidence_capture_writes_png_and_trace_from_the_launch`

## Re-run (2026-10-24, macOS/Metal, `CS_CAPABILITIES=retail,gpu,audio`)

On origin/main at 0cfdba9af:

```sh
cargo test --locked -p cs_app -- accept_m01_play_world_windowed_mission_capture_draws_world_pixels --include-ignored
cargo test --locked -p cs_app -- accept_vs_m01_runtime_evidence_capture_writes_png_and_trace_from_the_launch --include-ignored
```

Both passed (1 passed, 0 failed; about 235 s each).

## Reading

The probe passed on the same host between the failing commits and 0cfdba9af, so
the failure is either fixed by intervening commits or depends on the host's
window state (an occluded or unfocused window, or a display-sleep/compositor
state, can make the framebuffer probe read an empty frame). That second
explanation is a candidate only; no evidence was gathered for it. If the
failure returns, capture the failing run's `--capture-dir` and the window/display
state before closing it as flaky.
