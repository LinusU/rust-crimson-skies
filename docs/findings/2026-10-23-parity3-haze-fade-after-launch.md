# #1744 PARITY3-HAZE-FADE-AFTER-LAUNCH: the haze step is the cinematic hand-over, and it is a step, not a curve

Date: 2026-10-23. Basis: the owner's reference frames `c02`/`c03`/`c04` (reference-only, never
committed), the committed static trace `2026-10-23-m01-fog-update-static.md` (#1847) and a
re-read of the same sites in `private/fog-trace/image.asm` (`llvm-objdump` of
`$CS_ENGINE_IMAGE`, Intel syntax). No original executable ran: nothing here is
`verified_original`. Implementer: `swe2-max-1` (Devin SWE-2); no independent review yet.

## What the task saw in the owner frames

* `c03` ("+15 s after the launch checkpoint") is the first controllable frame: settled chase
  pose, HUD on. Mid-distance terrain pixels sit at the authored haze (about `(206,203,206)` —
  ZONE1's `FOG_COLOR 0.79` grey).
* `c04` ("+20 s") is the same view five seconds later: the same mountain columns read lower and
  greener (e.g. G > R where c03 was neutral grey) — a real drop in fog factor, not a repaint.
* Under `FOG_RANGES 1000 4500` a ~0.15–0.25 drop in the linear factor is what closing
  roughly 0.7–1.4 km on the terrain does. Five seconds of level flight at ~200 m/s is
  ~1 km, and the plane descends between the frames. **A constant fog plus the flight is
  sufficient to explain the observed clearing; a fog-parameter change is not needed.**

## What the image allows (#1847, confirmed by re-reading the sites)

* Nothing interpolates fog near/far, colour, altitude or tier scale over time. Fog changes
  only in discrete steps.
* The cinematic-mode pair is the step that matters: entry `0x450550` writes fog mode 0 (the
  renderer reads mode 0 at `0x551e3b` as "no fog test at all") via setter `0x4da800`
  (call `0x45067c`), multiplies the camera far clip by 4.0 (`0x4506a6`, row
  `cinematic_enter_far_clip_factor`) and sets `0x654120 = 1` (`0x450562`). Exit `0x450e30`
  writes mode 1 (`0x450ee1`), multiplies the clip by 0.25 (`0x450f0b`, row
  `cinematic_leave_far_clip_factor`, the exact inverse) and writes `0x654120 = 0`
  (`0x450f9b`). Fog parameters are never touched at entry or exit.
* **The pair's reach into the launch is an inference, not a trace** — but a tighter one than
  #1847 left it: a byte-pattern sweep of the image shows `0x654120` is stored by exactly
  those two instructions and nothing else, and the per-frame camera dispatch `0x489cec` steps
  the cinematic camera `0x450ad0` exactly while the flag is set (the player camera
  `0x493dc0` runs while it is clear). A driven intro camera therefore implies entry ran, and
  the return of the player camera at hand-over implies the exit ran: the flag cannot be
  cleared any other way.
* The alternatives are ruled out for M01: the launch hands over at ~150 m while `CLOUD_COVER`
  spans 10–11 km and `FOG_ALTITUDE` spans 9–10 km, so no altitude-band or zone boundary is
  crossed; the SW/HW zone-array selector `0x9be708` is written once at renderer init; M01's
  scripts spell no fog or clip verbs (`WorldSetFogState` at load only) and no `fogvol.zrd`,
  no precipitation block and no `zone2_cloud_floor` exist in its data.

## What was implemented

`sync_fog` in `crates/cs_app/src/mission_session/launch_cinematic.rs` (installed with the
phase by `install_launch_cinematic`): while `LaunchCinematic` plays, every `DistanceFog` is
parked on `FogFalloff::Linear { 1.0e9, 2.0e9 }` — the codebase's stand-in for "the renderer
does not test fog" (`apply_probe` uses the same), the observable half of mode 0 — with the
bound falloff kept per entity; on the first frame after the phase ends the kept falloffs are
written back unchanged, the mode-1 half of the step. The five-second clearing then needs no
code at all: it is the aircraft flying through the already-bound ZONE1 fog.

Not ported: the far-clip ×4/÷4. Our composed camera's far plane (40 km) already exceeds
everything drawn (dome ~24 km, sea ~20 km), so the factor would change no pixel; recording
that here rather than adding dead state.

For evidence the no-teleport smoke schedule gained two owner-capture seconds: `flight_1s`
(t+1 s) beside `flight_5s` (t+5 s) — the pair a few seconds apart that stands next to
`c03`/`c04` at the same simulated offset after control. (The no-teleport `first_control`
frame is asked for the very frame control returns — before the window's first flight
present in that mode — so it latches the flight-check screen; t+1 s is the earliest clean
haze frame there, and the scripted run's own `first_control` shows the hazy view because
its world was already rendering through the cinematic.) The scripted smoke's
`cinematic_end` (fog off) → `first_control`/`cinematic_handover` (fog on) frames show the
step itself.

## Kept unknowns

| unknown | evidence searched | content affected |
| --- | --- | --- |
| Which mission opcode activates the cinematic camera-mode object | callers of the pair's wrappers traced to the mode objects' activate methods and the end-cinematic callback `0x423cc0`; the dispatch site into M01's launch not traced | the "the launch runs `0x450e30`" premise stays an inference — strong, because `0x654120` has no other writer, but static only |
| Whether `0x71c2ac` and `0x71c0b8` hold the same class-14 object | `0x71c0b8` has no absolute store in the image (#1847 section 4) | none observed; both are the world object's fog state if they coincide |
| What `0x49dcde` gates with `near + 0.8·(far − near)` | operand source only | a draw-distance limit inside the fog range |
| The original's per-frame fog reach during the clearing | owner frames only; no runtime trace | the attribution to flight-through-constant-fog rests on the pixel change fitting the range factor, not on a measured original curve |

## Tests

`accept_parity3_haze_fade_after_launch_*` (`crates/cs_app/tests/accept_parity3_haze_fade_after_launch.rs`):
the parked-while-playing / restored-at-hand-over cycle through the production
`install_launch_cinematic` wiring, restoration of the bound value byte-for-byte (no
recompute), and a fog added mid-cinematic joining the same hand-over.
