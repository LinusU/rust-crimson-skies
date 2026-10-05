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

## Original-assets free flight (what you can test now)

```sh
cargo run --locked -p cs_app --bin cs -- --playtest --cs-path "$CS_GAME_DIR" --world c1c
```

Window title and banner: **ORIGINAL ASSETS / DEVELOPMENT FREE FLIGHT /
PROVISIONAL TUNING**. You fly the whole intact original `bloodhawk`
(`ZBD/planes.zbd`: 16 mesh bindings, 927 triangles: fuselage, wings, ailerons,
elevators, rudder, canopy, pilot and a static propeller) beside one original airship
of `ZBD/C1C/gamez.zbd` (subtree `piratezep`, 401 mesh records, 8 673
triangles). The controls below are the same. The airship's colliders are derived
from the triangles it draws: fly into it and the HUD counts an obstacle
contact and the aircraft stops. `R` resets the aircraft only (the area is
never respawned). `--aircraft bloodhawk` is the only documented aircraft and the
default; `c1c` is the only documented world. Any other id is refused.

An explicit `--cs-path` that is missing, is not an installation, or lacks
`ZBD/C1C/gamez.zbd` / `ZBD/planes.zbd` exits non-zero with the reason and
**never** falls back to the synthetic scene. Plain `--playtest` stays synthetic.
On start the command prints `playtest sources: {...}` (installation fingerprint,
the sha-256 of both containers, the aircraft's drawn/undrawn bindings, and a
`textures` object: the chosen archive, its sha-256, the claims, and per container
the textured and neutral material counts and every unresolved material); a smoke
run records the same object in
`report.json`.

What is provisional (all labelled, none original behaviour):

* Flight is the synthetic fixed-wing tuning, not the Bloodhawk's. It has no
  attitude stability and its velocity follows the nose only slowly.
* The aircraft's collider is **one box** measured from the composed extent of the
  drawn set (wingspan included), centred on the flight body; the parts do not
  collide individually.
* The propeller is **static** (`staticprop1` only, picked by its authored name;
  the other five propeller meshes, whose use is unmeasured, are not drawn), no
  control surface moves, and the engine-detail band `l12`, the shadow and the wreck
  pieces are not drawn. The drawn LOD band (`nearest`) is selected at a designed
  20 m viewer distance and never changes in flight. Every undrawn binding is listed
  with its reason in the `playtest sources` line and the smoke `report.json`
  (`aircraft_undrawn`), next to `aircraft_mesh_bindings`, `aircraft_triangles` and
  `aircraft_selection`.
* Spawn: 0.6 area-widths off the airship's port side, 55 % up its height,
  amidships, heading -Z (designed, `playtest_retail::spawn_pose`).
* Textures are the original ones, bound by a **designed** rule (task #666): the
  world group's highest-numbered `rtexture<N>.zbd` tier (`ZBD/C1C/rtexture10.zbd`,
  printed in the `playtest sources` line), a material's stored texture name read
  up to its first `.` in ASCII lower case (the exact-name rule of the lookup
  contract reaches almost nothing on retail data, so this is a development
  value, not the engine's rule), a plain lit material with sRGB, repeat
  addressing, keyed coverage as a 0.5 mask and no vertex colour. The aircraft is
  textured from the flown world's archive; every drawn binding of the airframe
  is cut into one textured piece per stored material group. A material whose texture does not
  resolve keeps the neutral colour and is listed in the report with its source id
  and mesh count. No lighting from the original, no sky or ground: the area has
  no floor, so you fall or fly on until `R`.
* World scale and handedness are a designed identity reading. No audio, no
  mission scripts, no M01 setup, no other sectors, worlds or airframes.

Tested at commit `66290057` (+ this document) on macOS 26, Apple M3 Pro GPU
(Metal). A real windowed scripted run (the `cs` binary, same input -> F24 flight
-> Avian -> chase camera route, **not** `human_play`): 120 simulated seconds,
7 200 rendered frames, 6 pause/focus-loss cycles, 12 resets, 54 contacts with
the airship, no panic, finite poses, 6 framebuffer PNGs. Private artifacts
(ignored by Git): `private/playtest-retail/` (`report.json`, `trace.jsonl`,
`frame_*.png`). Reproduce:

```sh
CS_PLAYTEST_RETAIL_OUT=$PWD/private/playtest-retail cargo test -p cs_app --locked \
  --test playtest_retail_launch -- --include-ignored windowed
cargo run --locked -p cs_app --bin cs -- --playtest --cs-path "$CS_GAME_DIR" \
  --smoke-seconds 20 --capture-dir private/playtest-retail
```

The smoke exits non-zero on a panic, a non-finite pose, a missing player,
camera or area, a missing container, an empty framebuffer, or a missing contact
or reset. Nobody has played this by hand yet; that is for you.

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
