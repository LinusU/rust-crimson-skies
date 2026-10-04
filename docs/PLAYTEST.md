# Development playtest

**DEVELOPMENT PLAYTEST / SYNTHETIC SCENE / UNCALIBRATED FLIGHT.** This is a
flyable development scene, not original Crimson Skies and not mission M01. It
needs no original installation. Nothing in it is `verified_original`.

## The command

```sh
cargo run --locked -p cs_app --bin cs -- --playtest
```

It opens a native window with a yellow-and-red aircraft in open sky, a flat
ground with reference lines and one large orange tower 450 m dead ahead.

## Controls (keyboard only)

| Key | Action |
| --- | --- |
| `W` / `S` | pitch down / up |
| `Q` / `E` | roll left / right |
| `A` / `D` | yaw left / right |
| `Left Shift` / `F` | throttle step up / down |
| `1` / `4` | throttle idle / full |
| `R` | reset to the known flyable state |
| `Esc` | pause / resume |
| `F10` | quit (closing the window also quits) |

A held flight key deflects the stick half way (designed: full deflection
pitched the uncalibrated airframe 80 degrees in two seconds). Mouse and
gamepad are bound by the F22 default map but are untested here.

The on-screen readout shows speed, altitude, heading, pitch, bank, throttle,
pause state and collisions, all read from the simulated aircraft.

## What is real

* Flight: the production F24 fixed-wing model through `spawn_flight_body` and
  `FlightForcesPlugin`, in the F23 Avian adapter at 120 Hz. Nothing writes the
  aircraft's `Transform`.
* Input: Bevy keyboard, F22 `BevyInputPlugin`, `InputSession` policy.
* Camera: the F21 chase `CameraRig`, resolved from the authoritative pose.
* Collision: Avian contacts with the tower and the ground.

## Limitations

* The airframe, its tuning, the spawn, the lighting, the scene dimensions and
  the key feel are newly authored. Flight is uncalibrated and has no attitude
  stability: the nose stays where you leave it.
* No original assets, missions, objectives, AI, weather, weapons or audio.
* A crash does not reset by itself; press `R`.
* Window focus loss pauses (F22 policy) and clears held keys; regaining focus
  resumes a pause the focus loss caused, while an `Esc` pause stands until `Esc`.

## Reset

`R` despawns the aircraft and spawns a fresh one through the same production
path: level, 250 m up, 55 m/s, throttle 75 %, chase camera re-seated. Repeated
resets leave one aircraft, one camera and the same fixed clock.

## Smoke run and artifacts

```sh
cargo run --locked -p cs_app --bin cs -- --playtest --smoke-seconds 20 --capture-dir private/playtest
```

A scripted key sequence goes through the same input and simulation path (every
frame is exactly 1/60 s of simulated time). It writes, into the capture
directory (`private/playtest` by default, ignored by Git): real window
framebuffer PNGs (`frame_*.png`), `trace.jsonl` (pose, attitude, command,
contacts) and `report.json`. It exits non-zero if a framebuffer is empty or
uniform, the aircraft did not respond to a key, the obstacle was not hit, the
resets or pause did not happen, or the clock ran while paused. The smoke ignores
the host window's real focus state (counted as `real_focus_overrides`). A scripted run is never
`human_play`.

The windowed binary test is ignored in CI:
`cargo test -p cs_app --locked --test playtest_fly -- --include-ignored`.
