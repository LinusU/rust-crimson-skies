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
PROVISIONAL TUNING / ORIGINAL FLIGHT LAW (OWNER-STATIC-2026-10-08,
UNCALIBRATED AGAINST AN ORIGINAL RUN #358)**. You fly the whole intact original
`bloodhawk`
(`ZBD/planes.zbd`: 16 mesh bindings, 927 triangles: fuselage, wings, ailerons,
elevators, rudder, canopy, pilot and the propeller disc, which spins with the
engine) beside one original airship
of `ZBD/C1C/gamez.zbd` (subtree `piratezep`, 401 mesh records, 8 673
triangles). The controls below are the same. The airship's colliders are derived
from the triangles it draws: fly into it and the HUD counts an obstacle
contact and the aircraft stops. `R` resets the aircraft only (the area is
never respawned). `--aircraft bloodhawk` is the only documented aircraft and the
default; `c1c` is the only documented world. Any other id is refused.

The aircraft is flown by the **original 2000 PC game's own fixed-wing law**,
recovered by static analysis of the owner's decrypted image under
**`OWNER-STATIC-2026-10-08`** (#796) and driven with the parameters imported
from `ZBD/zrdr.zbd`: `pbloodhawk` in `vehicle.zrd` (`kind_of` chain
`basic_airplane -> player_airplane -> pbloodhawk`), engine id 11 (`0.62`) in
`engines.zrd`, `nom_gravity 20` in `player.zrd`. It is **still uncalibrated
against an original run** (#358), so nothing it does is `verified_original`.
The cruise those parameters decide is the imported `fd_speed` (135 m/s; full
throttle level flight settles at 134 m/s, measured by the acceptance test). The
**start** speed is the playtest's own declared 55 m/s, because the original's
player spawn speed was never recovered — no start speed here can claim to be
the original's. The aircraft starts level below the 2000 m density ceiling, at
cruise throttle.

An explicit `--cs-path` that is missing, is not an installation, or lacks
`ZBD/C1C/gamez.zbd` / `ZBD/planes.zbd` / `ZBD/zrdr.zbd` exits non-zero with the
reason and **never** falls back to the synthetic scene. Plain `--playtest` stays
synthetic. On start the command prints `playtest sources: {...}` (installation
fingerprint, the sha-256 of both containers, the aircraft's drawn/undrawn
bindings, and a `textures` object: the chosen archive, its sha-256, the claims,
and per container the textured and neutral material counts and every unresolved
material) and `playtest flight: {...}` (the imported record, its chain, the
engine row, `fd_speed`, `veh_weight`, `ref_area`, `gravity`, the start speed and
the provenance label with `verified_original: false`); a smoke run records the
`playtest sources` object in `report.json`.

What is provisional (all labelled, none original behaviour):

* **Flight over original content is the recovered original law with the
  imported Bloodhawk parameters, and it is uncalibrated** (#797): the equations
  are static analysis of the decrypted image and the numbers are read from the
  installation, but no original executable has ever been run against them
  (#358). Over original content the playtest starts and resets at its declared
  55 m/s start and cruises at the record's imported `fd_speed` (134 m/s
  measured under full throttle); without original content the scene is the
  synthetic fixed-wing tuning below, which has no attitude stability and whose
  velocity follows the nose only slowly.
* The original's **Level-Off** assist (Shift+L, command 47) is in the law but
  is **not bound**: the input layer has no slot for it, so the toggle stays off.
  Filed as follow-up **#1134**.
* The aircraft's collider is **one box** measured from the composed extent of the
  drawn set (wingspan included), centred on the flight body; the parts do not
  collide individually.
* The propeller **spins with the engine** (#710, still provisional). The one
  drawn disc is `staticprop1` (picked by its authored name; the other five
  propeller meshes, whose use is unmeasured, are not drawn), and its hub is
  **measured from its own 16 triangles**: the area-weighted normal and centroid
  of the disc, with the normal's sign oriented aft. Measured: axis `[0, 0, 1]`,
  pivot `x ≈ 2.2e-8, y ≈ −0.0194, z ≈ +4.682`, radius `1.253 m`, thickness
  `0.168 m` — so it turns about the body's own `Z` axis at that rear pivot, and
  it sits under `propeller_spin.hub` in the `playtest sources` line and the
  smoke `report.json`. The rate is a **designed** curve, not a measurement
  (`playtest-retail.propeller-spin-rate-is-designed`): 1 rev/s at idle rising
  linearly to 6 rev/s at full throttle, driven by the flight model's engine
  **spool**, so it spools up and down instead of jumping. A stopped engine stops
  it, a pause freezes it, `R` reset leaves exactly one of them, and only the
  propeller child's own local `Transform` is written — the flight body's pose is
  never touched. Provisional in both directions: which mesh the original shows at
  which speed is unmeasured (no blur-disc swap rule is adopted — only
  `staticprop1` is drawn and spun), and which way it turns is a designed
  convention too (`playtest-retail.propeller-spin-sense-is-designed`).
* No control surface moves, and the engine-detail band `l12`, the shadow and the
  wreck pieces are not drawn. The drawn LOD band (`nearest`) is selected at a
  designed 20 m viewer distance and never changes in flight. Every undrawn binding
  is listed with its reason in the `playtest sources` line and the smoke
  `report.json` (`aircraft_undrawn`), next to `aircraft_mesh_bindings`,
  `aircraft_triangles` and `aircraft_selection`. Measured (#709): that propeller
  is at the **tail** — its `dontmove` siblings `prop1`/`prop1b` compose at
  `z = +4.80` and `prop2`/`prop2b`/`nitroprop1` at `z = +4.56`, behind the rudder
  at `z = +3.03`, and the drawn disc's own 16 triangles sit at `z ≈ +4.5` — which
  is why the spin axis is measured from the disc rather than assumed to be a
  nose-mounted hub.
* Nose direction (#709): the drawn airframe leads with its nose. The stored nose
  is the **measured** `−Z` — the container's own tail surfaces compose aft of
  the cockpit node in all eleven scene airframes — and
  `playtest_retail::nose_mapping` is the yaw that lands it on the body's forward
  axis, which for `−Z` is the identity, so nothing is turned. The earlier reading
  took the propeller's position for the nose (its disc composes behind the rudder
  here) and drew the aircraft tail first; see
  `docs/findings/2026-10-06-t709-airframe-nose-mapping.md`.
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
* Propeller: presentation only — `playtest::propeller` reads the same F24
  engine state the forces read and writes the disc child's own local
  `Transform` about its measured hub. It never writes the flight body's pose.
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

## The airship draws one intact variant of each part (#753)

Provisional. The `piratezep` subtree stores 401 mesh bindings, and #648 drew all
of them, so the scorched `burnpanels` (a `burn…` copy of every `panel…` mesh)
were drawn over the same hull plane as the light `panels`, and the `…d` damaged
halves beside the `…h` healthy ones: the dark jagged patches that flickered. The
area now uses the airframe's selection machinery (`choose_lod_band`, the F11-B
`select_lod_variant`, at a **designed** 300 m viewer distance,
`PLAYTEST_AREA_LOD_DISTANCE_M`): one LOD band per sibling group, `burnpanels`
hidden beside `panels`, `<stem>d` hidden beside `<stem>h`, and the running
`spin`/`counterspin` hidden beside `propstill`. These are **name reads**; the
original's damage-state rule is unmeasured. Every hidden binding is listed with
its reason in the `playtest sources` line and the smoke `report.json`
(`area_undrawn`, `area_selection`, `area_stored_bindings`), and colliders follow
the drawn set. No depth bias is applied: after the selection the measured
flank views are stable (see `docs/findings/2026-10-07-playtest-area-flicker.md`).

## The flat grey landing cards are hidden (#795)

Provisional. The owner's 2026-10-08 playtest screenshot showed a large flat
grey shape sticking out of the airship's underside. Measured over the drawn
bindings, exactly two carry a class the hull skin does not: `sphere` (node slot
3063, mesh 786 — one planar triangle, 500 × 125 stored units) under
`pz_auto_land`, and `half_cone` (slot 3066, mesh 787 — one 96 × 32 triangle)
under `pz_manual_land`. Each stores one polygon whose every material group
resolves to **flat-colour material 84** — a record whose textured flag is
clear, so it names no texture and the binder draws it in the neutral colour,
the uniform mid-grey of the screenshot. What the original did with these
landing cards is unmeasured (additive, translucent, or gated by landing
behaviour this free-flight playtest never runs), so the **flat-colour card**
rule (`flat_card_reason`, `PlaytestConfig::hide_flat_colour_cards`) hides both
from drawing **and** from collision, listed with their reason in `area_undrawn`.
The drawn set is
293 of the 401 stored bindings — two fewer than #753's 295 — and colliders
follow the drawn set. The before/after GPU frames of that end of the airship
are measured in `docs/findings/2026-10-08-playtest-area-flat-shape.md` (hashes
only; the PNGs stay under `private/`).

Hiding the cards also shrank the area's measured extent, which moved the
designed values that derive from it (the start length fraction retuned, and the
`overview` camera eye moved to stand a full span from the center — half a span
beyond the extent's aft face — after the review caught the old eye sitting
exactly on that plane, inside the hull's silhouette, occluding the outboard
spawn with zero aircraft pixels in the overview frame). Details in the finding
and in `docs/PLAYTEST-RETAIL.md`.

## Coplanar decal layers draw 1 cm off their base (#794)

Provisional, and a designed rule — nothing in the stored records marks a
decal. The skull emblem on the `piratezep` hull (a second material group of
the hull quads, `fhunter_logo2.tif`/`fhunter_logo4.tif` over `piratezepskin2.tif`)
and the Bloodhawk's wing insignia (own quads ~1 mm off the wing,
`blo_winglogo.tif`) are coplanar layers that z-fight their base. The shared
rule: a part whose bound texture carries keyed coverage is a decal layer, and
its vertices move `DECAL_OFFSET_M` (0.01 m) along their normals before upload,
so the coplanar base can never win the depth test. `StandardMaterial::depth_bias`
was measured inert on this path — `Depth32Float` scales a constant bias by the
smallest representable increment, ≈ 0 — so the offset is expressed in geometry.
`PlaytestConfig::decal_offset = false` keeps the coplanar baseline the
acceptance suite measures against. Every decal layer is listed with mesh,
material group, stored material and texture in the `playtest sources` line and
the smoke `report.json` (`decals`). See
`docs/findings/2026-10-08-playtest-decal-zfight.md`.
