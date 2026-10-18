# PARITY-M01-PLAYER-VIEW-A (#1666): the M01 player chase camera preset

Task test prefix: `accept_parity_m01_player_view_a_`. Code:
`crates/cs_app/src/camera/chase_preset.rs`, wired in
`mission_session/compose.rs` (`PlaytestCamera::capture_fitted`). Tests:
`crates/cs_app/tests/camera/chase_preset.rs`.

## What is and is not known

| Quantity | Status | Evidence |
| --- | --- | --- |
| Chase distance (astern) | **unknown** in the data | No readable file and no `engine-facts.toml` row carries it; the player model's `cockpit_camera` marker is the cockpit eye, and the `bullet1..5` "chase cams" nodes are projectile cameras (`2026-10-10-vs-m01-rt-player-airframe-visual.md`). The per-frame camera function `0x42ee60` was read: it places the viewer from a node position and applies altitude/shake terms, not a body offset. A bounded static analysis of the original's chase placement is **not done**. |
| Chase height | **unknown** | as above |
| Field of view | **unknown** | `CAMERA H_FOV`/`V_FOV` are script commands; the chase view's value is not recovered. The fixture's designed 55° vertical stays. |
| Follow smoothing | **unknown** | A still capture cannot show lag; `CameraRig::DEFAULT_RESPONSE_PER_S` (designed, 12/s) stays. |
| Horizon-level up axis (plane banks against a level camera) | **unknown / not implemented** | Owner's description of the original; the rig keeps the aircraft's own axes. Same open question as #541 (`CameraSetHorizon`). Affects every banked turn of the player view. |

## The visual target

Owner capture of the original (retail 1.02 under Drydock) at +25 s, 1024×768,
downloaded to the gitignored `private/ref-drydock/m01/orig25.png`: lower wing
705 px (0.918 of the image height), spinner centre at 540 px (0.203 of the
height below the view axis, which is the horizon at 0.495), plane seen from
directly behind, level.

## The preset

Designed, fitted to the capture (provenance stays `designed`): **10.1 m astern,
2.1 m above** the body origin, fixture projection. The fixture's 12 m / 3 m
rendered the lower wing at 0.83 of the image height; the first fit (10.9 / 2.5)
at 0.85.

Owner-command smoke on the committed code (`--smoke-no-teleport`, frame
`frame_01801_flight.png`, 2560×1440, kept in
`private/evidence/PARITY-M01-PLAYER-VIEW-A/`): lower wing 1295 px = **0.90**
of the image height, spinner centre at 0.698 of the height. Against the
capture (0.918 / 0.703) that is within **0.02 of the image height**, the
stated tolerance. Fractions of the *height* are compared because the framing is
`PreserveVertical`; the capture is 4:3 and the window 16:9, and the original's
horizontal framing is not known. Measured by eye from the screenshot, not by a
tool.

## The gun lines

The yellow lines in the old frame (`frame_00361_guns.png`) are the designed
tracers (`weapon_present.rs`, unlit `Color::srgb(1.0, 0.88, 0.25)`), drawn only
while the guns fire: the smoke holds `Space` for that frame. No overlay is
drawn in the default view (`frame_01801_flight.png` has none). The original
data was not found to show any such lines.

## Follow-up

Horizon-level chase up axis and the original's placement values remain open;
see #541 and the unknowns above. This slice does not claim them.
