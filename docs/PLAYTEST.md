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
* The original's **Level-Off** assist (Shift+L, command 47) is wired (#1134):
  `FlightCommand::LevelOff` carries the toggle into the law's
  `OriginalState.level_off`, bound to **`L`** in the playtest (the original's
  chord Shift+L is not reproduced: `Left Shift` is already the throttle
  step-up here and the action map has no chord sources). The assist levels the
  wings only while it is on **and** the stick is hands-off; it starts off and
  `R` reset returns to off. The binding is a designed slot for the original
  command, not a measurement of the original's chord.
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
| `L` | Level-Off assist toggle (original law only) |
| `Space` | fire guns (mission M01) |
| `Left Ctrl` | fire rockets (mission M01) |
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

## Original mission M01

```sh
cargo build --release --locked -p cs_app --bin cs
CS_CAPABILITIES=retail,gpu,audio \
target/release/cs --cs-path /path/to/retail --mission M01
```

* `--cs-path` is the read-only retail installation. Never put anything in it:
  an extra file there changes the installation fingerprint.
* `CS_ENGINE_IMAGE` is **optional**. The player's airframe and start pose come
  from the committed owner-static table `data/owner-static/engine-facts.toml`
  (#1432, citations and `static` provenance, never `verified_original`).
  Setting `CS_ENGINE_IMAGE=/path/to/crimson.decrypted.exe` reads the same two
  facts from the owner's decrypted executable instead; it lives **outside** the
  installation and is never searched for inside it. A value that is set must
  name an existing file. If neither source can supply the mission's airframe
  and pose, the closure measurement refuses and names the gap.
* `CS_CAPABILITIES` must list `audio` (and `gpu` for the window; `retail` for
  the data). The mission plays its launch sound and refuses to run mute.
  Without `audio` it refuses with `add audio to CS_CAPABILITIES` and exit
  code 4. This is checked before the dependency closure is measured, so it is
  reported at once. Startup then takes about a minute while the closure is
  measured and the world loads; the window opens after.

What the window shows: the original M01 world with its mission records,
labelled `ORIGINAL MISSION M01 / ORIGINAL WORLD / PLAYER AIRFRAME MEASURED /
PROP+LOD PICKS DESIGNED`, and the player airframe `pdevastator`. M01 now
shows:

* the world and the sky, with the authored weather;
* its actors, flying under the mission AI;
* weapons: `Space` fires the guns, `Left Ctrl` a rocket (see "Firing" below);
* the radio: mission dialogue and music (#1438, #1461, #1463);
* the objective HUD;
* the mission's triggers.

The terminal prints the flight law's provenance (`mission flight: record ...
provenance ... verified_original=false`): the statically recovered fixed-wing
law, still uncalibrated against an original run (#358). Nothing here is
`verified_original`.

Controls are the keyboard table above (`W`/`S`, `Q`/`E`, `A`/`D`, throttle,
`Esc`, `F10`). `R` restarts the whole mission: it reloads the world and
respawns the player from the start recipe, instead of only resetting the
aircraft.

### Placing the player and waiting for a block (#1486)

An evidence capture (`CS_MISSION_CAPTURE=1`, `CS_EVIDENCE_DIR`, capabilities
`gpu,audio`) can be given a scenario. M01 starts 5 311 m from
`workersvoyagezep` against the proximity trigger's 700 m radius and its
authored heading never passes inside about 1 400 m, so the trigger cannot fire
from the measured start:

| Variable | Meaning |
| --- | --- |
| `CS_MISSION_START_M=x,y,z` | replaces the measured start position (metres) |
| `CS_MISSION_START_HEADING_RAD=<r>` | replaces the measured heading (radians) |
| `CS_MISSION_WATCH_BLOCK=<n>` | zero-based block index the run waits for |
| `CS_MISSION_WATCH_MAX_FRAMES=<n>` | rendered frames the wait may last |

* The start pair and the watch pair are each all-or-nothing, and a malformed
  or non-finite value is refused. Setting any of them without
  `CS_MISSION_CAPTURE=1` is refused too; it is never ignored.
* A watched run does not run the scripted pitch-up. The player is the
  production body under the production flight law; the run ends once the block
  completes (writing a `trigger` trace record with the observed member
  positions and the record's nap/wake/kill states, and a
  `frame_<n>_trigger.png`), or fails if the budget is spent first.
* The trace header is followed by a `scenario` record naming the placement.
  An overridden start is a test placement, not the measured start, and is
  never original evidence of where the mission begins.

### Firing (#1437)

The player can shoot in M01:

| Key | Command | Effect |
| --- | --- | --- |
| `Space` | `FirePrimary` | fire the guns |
| `Left Ctrl` | `FireSecondary` | fire a rocket |

Both are the F22 designed default bindings, delivered as one press edge each
and published to the mission's weapon session on the earliest committed fixed
tick of the frame (a press fires once, not once per fixed tick). **Holding
`Space` keeps firing** for as long as it is held (#1536): the input boundary
delivers one edge per hold, so the mission's publisher mints the repeat itself,
one shot per the loadout's own declared gun rate (`MISSION_TICKS_BETWEEN_SHOTS`,
designed) — the same rate the F27 step's per-mount cooldown enforces. Letting
go stops the stream; a key held through a pause or a focus loss is released by
the input session and stops firing too.

* **Guns** go through the F27 weapon path: three designed guns (a nose cannon
  and one per wing) fire the one selected bank along their live mount poses,
  the round's sweep reaches the F29 damage authority, and a destroyed part
  spawns its authored debris and scores the kill.
* **Rockets** go through the F28 ordnance path: one designed proximity-fused
  flak rocket launches from its designed hardpoint, detonates when its flight
  path comes within the declared trigger radius of a live damage-bound actor
  the targeting store declares hostile, and the blast routes into the same
  F29 damage authority — so a rocket kill also wrecks and scores. Firing a
  rocket with no such target is refused by name (`no declared-hostile damage
  target is live`), and the refusal is shown on the HUD for two seconds
  (`mission_session::weapon_present`, designed layout) instead of the mission
  silently doing nothing.

**What firing draws (#1536).** A round's ECS mirror now carries a drawn child:
a bright unlit streak spanning the segment the round actually flew since the
last drawn frame, so a held trigger reads as a stream of tracers. A launched
rocket's mirror carries a drawn body oriented along its own flight path. Both
are **designed** visuals (`m01-window-fire.designed-visuals`): the original's
sprites, sizes and colours were not recovered, and the mirror carries no
measured orientation. The pose is not invented — every streak comes from the
authoritative runtime position, and a mirror the schedule despawns takes its
drawing with it.

**Firing is still silent.** A shot's sound id (`m01-play-weapons.designed-shot`)
has no declared record and no measured binding to any sound member, so the
presentation consumer refuses it by name rather than playing a guessed clip
(`WeaponEffectRefusal::UnknownAsset`); the consumer itself is still not driven
from the mission's fixed loop (F27-C-FX-SCHED, #1304) and the one-shot device
pass that would audibly play a cue is F41's follow-up. The installation does
contain gun-named members (`30cal_gun.wav`, `40cal_gun.wav`, `chaingun.wav`)
but nothing in the data binds one to the player's guns — naming one would be a
guess, so the gun sound stays unbound until it is measured
(`M01-WINDOW-FIRE-GUN-SOUND`).

**The loadout is designed, not measured.** There is no production reader that
returns M01's Devastator guns, ammunition or rocket loadout from the original
installation (the launch closure binds the airframe and the start pose and
nothing else), so every gun, ammunition and rocket record is
`Origin::Designed` with the `m01-play-weapons.designed-loadout` claim. The
original's numbers replace these when F27-E/F28-D measure them; nothing here
is `verified_original`.

**The wind is the mission's, or designed calm air.** The F27 step refuses to
run without an authoritative wind. M01's authored weather declares no wind, so
the mission's publisher falls back to a **designed calm air** carrying
`m01-play-weapons.designed-calm-air` whenever the environment's own wind is an
explicit unknown; the mission's weapon report says which one a tick resolved
under (`designed_calm_air`). F19-C owns the permanent policy.

**What must be present to hit anything.** Nothing in this slice spawns a
victim. A target is an actor the spawn host binds (`DamageActorBinding`, a
`PartSweptBox`, a `PartDebrisBinding`) and registers with the session's shared
damage authority and its score ledger. M01's own actors are another task's
(M01-PLAY-ACTORS, #1435; the score admission is VS-M01-SCORE-ADMIT, #1426);
with none bound, the guns fire, the rockets refuse by name and nothing
resolves — the honest state, reported, not papered over.
