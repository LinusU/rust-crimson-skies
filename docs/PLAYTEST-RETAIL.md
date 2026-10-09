# PLAYTEST-RETAIL: one original world area and one original aircraft

**Stage:** `PLAYTEST-RETAIL-SCENE` (task #648), the render lane of the free-flight
experiment. It prepares **original art** for a standalone playtest. It is not a
mission, it is not `M01`, and it does not block the synthetic flight loop
(`PLAYTEST-FLY-NOW`).

**Capabilities used:** `retail` (read-only access to `$CS_GAME_DIR`) and `gpu`
(one real renderer, `Apple M3 Pro` on Metal). No original run happened: `retail`
is file access, not evidence of how the 2000 engine behaved. **Nothing in this
document is `verified_original`.**

## The label every surface carries

```
ORIGINAL ASSETS / DEVELOPMENT FREE FLIGHT / PROVISIONAL TUNING / ORIGINAL FLIGHT LAW (OWNER-STATIC-2026-10-08, UNCALIBRATED AGAINST AN ORIGINAL RUN #358)
```

`crates/cs_app/src/playtest_retail.rs` exports it as `PLAYTEST_LABEL` and a
test asserts it verbatim and asserts that it never contains `M01`, `faithful`,
`campaign` or `verified_original`.

## The flight law (#797)

The scene stage spawns a pose; the loop that flies it is `cs --playtest
--cs-path …` (`docs/PLAYTEST.md`). Over this content that loop is **not** the
designed fixed-wing: it is the original 2000 PC game's own fixed-wing law,
recovered by static analysis of the owner's decrypted image under
**`OWNER-STATIC-2026-10-08`** (#796) and flown with the parameters the
production importer reads from `ZBD/zrdr.zbd`:

| input | record | value |
| --- | --- | --- |
| airframe | `vehicle.zrd` `pbloodhawk`, chain `basic_airplane -> player_airplane -> pbloodhawk` | engine 11, pitch/roll/rudder 3.3 / 7.5 / 2.0, return 3.0, damp 5.0, recI 1.18 / 1.0 / 1.1, `fd_speed` 135, drag 0.37, `W` 1900, `S` 330 |
| engine | `engines.zrd` id 11 `Bloodhawk Lvl-2` | `0.62` |
| globals | `player.zrd` first entry | `nom_gravity 20` and the lift/authority fades |

The body's mass is `W / 9.82` — the law's own force scale — global gravity
stays zero because the step already contains gravity and drag, and the law owns
attitude while the kind is active (it integrates `2 * |omega| * dt`, which no
torque-driven body reproduces), so the body integrates no torque at all.

It is **static evidence, not a measurement**: no original executable ran and the
law is **still uncalibrated against an original run** (#358). The **cruise**
those parameters decide is the imported `fd_speed` (135 m/s; full-throttle
level flight settles at 134 m/s, measured by the acceptance test). The **start**
speed is the playtest's own declared 55 m/s (`RETAIL_START_SPEED_M_S`), because
the original's player spawn speed was never recovered — no start speed here can
claim to be the original's. The original's Level-Off assist (Shift+L, command
47) is in the law but has no input-layer slot, so it stays off (follow-up
**#1134**). #649's scripted smoke path was steered onto the hull by #797's
spawn retune (see the spawn row below); re-deriving the script itself for this
law is follow-up **#1135**.

## What is rendered

| | chosen value | why this one |
| --- | --- | --- |
| world container | `ZBD/C1C/gamez.zbd` | the group #629 imported, so this lane reuses its measured reader and its exact per-container mesh naming |
| area | the node subtree of stored node slot **517**, authored name `piratezep` | a record of the world node's own stored child list; measured 793 nodes, 401 of them binding a mesh, **8 673 stored triangles**, composed extent 106.3 × 217.7 × 839.5 canonical m |
| aircraft container | `ZBD/planes.zbd` | the shared aircraft archive |
| aircraft airframe | the root named **`bloodhawk`**, found by name through the production `SceneGraph::root` | F11's identity rule: an airframe references a root in PLANES.ZBD, never a mesh-array position |
| aircraft meshes | the **whole intact airframe** (#665): the subtree of node slot **2296** `healthy`, one LOD band (`nearest`, slot 2299) selected by the F11-B rule, plus the propeller node slot **2541** `staticprop1`, drawn and spun about its measured hub (#710) | measured 16 mesh bindings, 927 triangles, composed extent about 11.6 × 2.7 × 10.6 units; 24 of the airframe's 40 bindings are listed as undrawn |

The **same** area cannot be had from the partition-grid import, and that is the
gap this stage closes. Measured over `ZBD/C1C/gamez.zbd`:

* the container holds **5 644 nodes** and 2 250 mesh slots (21 779 triangles in
  total);
* the world record's partition grid plus its stored child list — what #629 imports
  — names **346** of those nodes, and those records draw **1 786 triangles**: 144
  flat 1 024-unit quads at `y = 960`, 20 volumetric fog boxes, a 17 km horizon
  dome and ~45 vegetation instances;
* the pinned subtree holds **793** nodes and **8 673** triangles: the airship's
  hull, its gas bags, its rigging, its interior, its panels, its turrets and its
  engine cowls, reachable only by descending the hierarchy.

So a mission could load an original world and see almost nothing in it. This
stage renders the part that is visible.

The aircraft was one mesh (`fuse03`) in #648; #665 replaced it with the whole
intact set, so the wings, tail, canopy and propeller are drawn. See
`docs/findings/2026-10-05-t665-full-aircraft.md` for the selection rule and the
bindings that are still not drawn.

## The designed values (PROVISIONAL TUNING)

Every one is a declared development value with its own claim id, and every one is
a constant in `playtest_retail.rs`.

| decision | value | claim id |
| --- | --- | --- |
| stored unit | **1 stored unit = 1 canonical metre** | `playtest-retail.stored-unit-is-one-metre` |
| axis map / handedness | identity, right-handed, `+Y` up — the workspace's declared *canonical* source, at `Unknown` calibration | same |
| area selection | the pinned node subtree 517 | `playtest-retail.area-selection-is-designed` |
| collision classification | every area record `Solid` + `FromMesh`, so a collider is derived from the mesh the record draws | `playtest-retail.collision-is-derived-from-the-drawn-mesh` |
| object identity | `playtest.node-<stored slot>` (the slot, for #629's reason: the container stores 34 records named `g27816` in one group) | — |
| gameplay surface | an explicit **unknown** carrying `cs_content::world::WORLD_SURFACE_UNMEASURED` | #629's claim |
| world boundary | an explicit **unknown**; no invisible wall | #629's claim |
| spawn | `min + (−0.35·width, 0.55·height, 0.75·depth)` of the measured extent → `(−90.40, −8.11, 205.80)`; the `x` fraction was `−0.6` until #797 retuned it (measured: at `−0.6` the scripted smoke's steer-into-area reaches the hull only where it has tapered away, and never collides) | `playtest-retail.aircraft-pose-is-designed` |
| aircraft nose | `nose_mapping`'s yaw landing the **measured** stored nose (`−Z`, `STORED_AIRCRAFT_NOSE_AXIS`: the container's tail surfaces compose aft of the cockpit in all eleven scene airframes) onto the runtime's forward axis (`−Z`) — the identity, so nothing is turned. Measured axis, designed mapping (#709) | same |
| propeller spin rate | 1 rev/s at idle rising linearly to 6 rev/s at full throttle, read from the flight model's **engine spool**; `0` while the engine is stopped, frozen while paused; only the propeller child's own local `Transform` is written, never the flight body's pose (#710) | `playtest-retail.propeller-spin-rate-is-designed` |
| propeller spin sense | the measured disc normal oriented **aft** (away from the measured `−Z` nose) with the right-hand rule about it. The axis and pivot themselves are **measured** from the disc's own 16 triangles, not chosen: see "The propeller spin" below (#710) | `playtest-retail.propeller-spin-sense-is-designed` |
| camera views | three, derived from the measured bounds and the aircraft's own extent (see below) | `playtest-retail.camera-views-are-designed` |
| lighting | key `3.2` lux, fill `0.45 ×` the key, both aimed at the view's target | `playtest-retail.camera-views-are-designed` |
| clear sky | opaque `(0.36, 0.52, 0.72)`, declared, not a claim about the original's sky | same |
| material | **one** neutral development material per subject — world `(0.74, 0.70, 0.62)`, aircraft `(0.86, 0.34, 0.30)` — reported once on `PlaytestMaterial` with the container keys and mesh counts it covers | `playtest-retail.neutral-development-material` |
| capture frame | 640 × 480, 45° vertical FOV, near `0.01 ×` and far `8 ×` the eye-to-target distance | — |
| gravity | **zero** — this stage owns no flight loop, so nothing here may fall | — |

### The three camera views

Distances are declared as multiples of the **aircraft's own measured extent**, not
as metres, so the framing holds for a different aircraft:

| view | eye, as an offset from the spawn | target | why |
| --- | --- | --- | --- |
| `chase` | `(−1.8·length, +0.7·height, +0.6·length)` = `(−109.51, −5.96, 212.16)` | the spawn | **abeam, outboard.** Behind the aircraft it would show a 2 m cross-section (measured: 807 aircraft pixels); outboard, because an inboard eye puts the *camera* between the aircraft and the area and frames only sky (measured: refused as `NoEnvironment`) |
| `quarter` | `(−1.4·length, +1.2·height, −1.6·length)` = `(−105.26, −4.43, 188.80)` | the spawn | the same placement from higher and further forward, so the second aircraft frame is a different angle rather than the same one twice |
| `overview` | `centre + (−0.42·span_x, +0.36·span_y + 3·height, +1.0·span_z)` = `(−44.66, 46.04, 634.48)` | the area's centre | frames the **area**: the whole airship with the aircraft a measured speck in front of it. The z offset is a full span (#795's review), so the eye stands half a span **beyond** the extent's aft face: at the earlier `+0.5·span` it sat exactly on that plane, which once the landing cards no longer inflated the extent left the eye inside the hull's silhouette and the outboard spawn occluded (measured: 0 aircraft pixels) |

### The propeller spin (task #710, `PLAYTEST-PROP-SPIN`)

The one drawn disc (`staticprop1`, slot 2541) turns with the engine. Its **hub
is measured, not chosen**: `measure_propeller_hub` takes the area-weighted
normal of the disc's own 16 triangles — sign-aligned to the largest triangle's
winding first, because a stored mesh's winding is a rendering convention — and
the area-weighted centroid of them, orients the normal aft, and refuses a
surface thicker than it is wide rather than inventing a plane for it. Measured on
the pinned pair:

| quantity | value |
| --- | --- |
| axis (mesh frame, unit, aft) | `[1.43e-9, −3.91e-10, 1.0]`, i.e. the body's `Z` once the node composes it |
| pivot (mesh frame) | `[2.23e-8, −0.019357, 4.681585]` canonical metres, composed unchanged by the node |
| radius / thickness | `1.2529 m` / `0.1680 m` |
| triangles | 16 |

The **rate** is designed (`playtest-retail.propeller-spin-rate-is-designed`):
1 rev/s at idle rising linearly to 6 rev/s at full throttle, read from the
flight model's engine spool, `0` while the engine is stopped, frozen while
paused, and exactly one entity survives an `R` reset. Which stored propeller
state the original showed at which speed is unmeasured, so **no** blur-disc
swap rule is adopted: only `staticprop1` is drawn and spun, while `prop1`,
`prop1b`, `prop2`, `prop2b` and `nitroprop1` stay listed in `aircraft_undrawn`.
The startup `playtest sources` line and the smoke `report.json` carry the whole
rule, both claim ids, the shown mesh and the measured hub under
`propeller_spin`.

```sh
CS_GAME_DIR="$CS_GAME_DIR" cargo test -p cs_app --test playtest_retail -- \
  accept_playtest_prop_spin_ --include-ignored
```

## The captures

`private/evidence/PLAYTEST-RETAIL-SCENE/<test>/` (Git-ignored; no original bytes and
no screenshots are committed; each test writes its own subdirectory because the
harness runs the GPU tests in parallel). Each view is rendered **twice** on the real GPU —
once with the aircraft presented, once with it hidden — and the difference
between the two frames is the aircraft's own contribution. The hidden frame is
the environment's own contribution. That is what makes "the aircraft and the
environment are both in frame" a measurement instead of a claim.

Measured on the pinned pair (640 × 480, `Apple M3 Pro`, Metal):

| view | non-sky pixels | permille | aircraft pixels | environment pixels | PNG bytes |
| --- | --- | --- | --- | --- | --- |
| `chase` | 168 376 | 548 | **9 357** | 168 361 | 154 957 |
| `quarter` | 130 166 | 423 | **5 089** | 130 166 | 121 371 |
| `overview` | 28 113 | 91 | **10** | 28 103 | 38 574 |

Those numbers predate #795: they were measured with the landing cards still
drawn, so the area extent and the designed start were larger. Re-measured after
#795 (same machine and renderer, `playtest_retail` binary): aircraft pixels
`chase` 11 359, `quarter` 7 968, `overview` 24, with covered permille 621 / 226 /
31 — the same invariant holds and the tests assert it on every run.

The digests of those exact files, so a later reader can tell a rerun's frame from
this one instead of trusting a byte count (they are reproducible on the pinned
pair and not across drivers):

```
6bd0c0e87ced6928e3b32766bedc7ba1a8bc9ad0460dc4099166e0a2f08bc76b  playtest-retail-c1c-chase.png
50c3e2bf3596b5681d33153e4334107b28587c918b622c2bca8af3822722b8c9  playtest-retail-c1c-overview.png
694a0a9510e0ed85c1e46d372ee31df24943aba6fa75cb05efe70e0cbc7b4535  playtest-retail-c1c-quarter.png
```

The refusal thresholds are named and every refusal **deletes the PNG**, so a file
that exists on disk is a frame that was measured: a frame with a single luminance
level is `UniformFrame`, less than 20 permille of non-sky pixels is
`NoEnvironment`, and zero aircraft pixels is `NoAircraft`.

## Commands and commit

```sh
# the whole selection, retail half included
CS_GAME_DIR="$CS_GAME_DIR" cargo test -p cs_app --test playtest_retail -- \
  accept_playtest_retail_ --include-ignored
```

11 tests: 8 run and pass without the installation, 3 are `#[ignore]`d
(`requires CS_GAME_DIR`) and run and pass with it. Measured on the two review
runs (`Apple M3 Pro`): the retail half of the selection finished in **48 s** and
in **60 s**, with the installation's pages already in the page cache. A cold run
is dominated by the one production discovery pass that fingerprints every file in
the installation, so budget minutes rather than seconds for the first run of a
session.

The exact commit every number in this document was produced on is
`0717f3bd9297225ee8f3c85678a961af6aff1996`. The review that followed changed the
mesh-identity rule to have one owner instead of two copies, made the teardown
report the size of the asset release it performs (and the retail test assert that
every uploaded engine mesh is gone afterwards), and gave the non-retail scratch
directory a per-process name; **no rendered geometry, camera, spawn or threshold
changed**, and the review re-ran the selection and got the same three frames with
the digests above.

## What this is **not**

* **Not a mission and not `M01`.** No campaign, no objective, no actor, no script.
* **Not a flight loop.** `spawn_playtest_scene` produces a scene and a pose; a
  consumer drives it. This module owns no window, no input and no fixed-step
  integration, so `PLAYTEST-FLY-NOW` keeps the generic loop.
* **Not textured *by this stage*.** This stage flat-shaded every surface. Task
  #666 (`PLAYTEST-TEXTURES`) now draws the stored textures; see "Textures" below.
  The neutral material remains the fallback for a material whose texture does not
  resolve.
* **Not `verified_original`.** The counts and extents are measured from the
  original bytes by the production readers, which is `ObservedTool`. The designed
  values are `Designed`. No original run happened.
* **Not an area census.** The counts are one documented subtree's, not a world's
  and not the installation's. #639 still covers all eight world containers.

## Known unknowns, carried forward

| unknown | effect | resolving task |
| --- | --- | --- |
| the original's world-vertex unit and coordinate handedness | every metre in this document is "stored units × a declared factor" | #436 (blocked) |
| the original's collision classification | the area's records are declared solid; whether the original collided a panel the same way is unmeasured | — |
| how the 2000 engine oriented an airframe | which stored axis is the nose is now measured from the container's own authored names (`−Z`: the tail surfaces compose aft of the cockpit in all eleven scene airframes, #709 and `docs/findings/2026-10-06-t709-airframe-nose-mapping.md`), and the mapping onto the runtime's forward axis is a designed choice; that the original engine read its own airframes the same way is still unmeasured | an original run |
| how fast and which way the 2000 engine turned a propeller | the hub **axis and pivot** are measured from the disc's own triangles (#710), but the rate curve and the spin sense are designed claims (`playtest-retail.propeller-spin-rate-is-designed`, `playtest-retail.propeller-spin-sense-is-designed`) | an original run |
| which stored propeller state the original showed at which speed | only `staticprop1` is drawn and spun; `prop1`, `prop1b`, `prop2`, `prop2b` and `nitroprop1` are listed undrawn rather than swapped in at a guessed throttle (#710) | an original run |
| the container-wide `c1c` hierarchy is inconsistent (node 642 names parent slot 0, which the world record's child list does not name back), so the scene graph is built over the documented subtree through the **same** production validator | one production rule, a narrower node set | the `scene_node` id blocker in `docs/findings/2026-10-04-m01-lc-world-import.md` |
| the mesh identity is per-container (`c1c.mesh-<n>`, `planes.mesh-<n>`) | two containers can hold the same mesh under different ids | #638 |
| the original's lighting, sky, weather and audio | this stage declares its own | — |

## The integration-ready seam for `PLAYTEST-RETAIL-HANDOFF`

`crates/cs_app/src/playtest_retail.rs` is self-contained and exposes exactly what
a one-command free flight needs:

```rust
let config  = PlaytestConfig::documented();               // or a variant of it
let sources = read_playtest_sources(&install_root, &config.world_group)?;
let mut app = playtest_app();                             // physics + renderer, no window
let scene   = spawn_playtest_scene(&mut app, &sources, &config)?;

// scene.spawn()          the aircraft's start position, canonical metres
// scene.rotation()       the declared nose rotation
// scene.aircraft_entity() the entity a flight loop would fly
// scene.definition()     the area's records, if a mission wanted to place actors
// scene.spawned()        every collider, with its own triangle count
settle_playtest_colliders(&mut app, &scene);              // derived colliders exist
// ... drive the scene (the consumer owns the loop) ...
teardown_playtest_scene(&mut app, &scene);                // despawns exactly what it created
```

`playtest_app()` is Bevy's own defaults with the window plugin disabled plus
Avian's physics plugins and this crate's passes, finished and cleaned up before
anything is spawned. A consumer that wants a windowed loop adds its own window
plugin and camera on top; the renderer, the asset stack and the physics stack are
already there.

## Textures (task #666, `PLAYTEST-TEXTURES`)

`crates/cs_app/src/playtest_textures.rs` joins F10-C.02's material records and
texture-name table, F08-C's `TextureCatalog` and F17-B's RGB565 expansion for this
scene. Each mesh is cut back into one drawn part per stored material group (the
merged upload keeps its whole-mesh `Mesh3d`, for the area's collider and as the
aircraft binding's `AircraftPart` entity, but carries no material of its own), and
each part gets the material its stored index resolves to. An aircraft part's
pieces are children of its binding entity, so they move, hide and despawn with it.

Three **designed** choices, each under its own claim id and none an original
claim:

| decision | value | claim |
| --- | --- | --- |
| archive | the highest-numbered `rtexture<N>.zbd` tier of the world group (`ZBD/C1C/rtexture10.zbd`), else `texture.zbd`; the aircraft uses the flown world's archive because F10-C.04 measured the airframe's archive as a runtime fact | `playtest-textures.archive-selection-is-designed` |
| name reading | stored name up to its first `.`, ASCII lower case (`TextureNameRule::FirstDotCaseFolded`) | `playtest-textures.name-reading-is-designed` |
| presentation | lit `StandardMaterial`, sRGB, repeat, keyed coverage as `AlphaMode::Mask(0.5)`, no vertex colour, both sides drawn | `playtest-textures.presentation-is-provisional` |

The exact-name rule resolves 10 of 4 478 retail material rows
(`docs/findings/2026-09-29-f10-c-04-gamez-texture-archive-binding.md`), which is
why the name reading is a development value here. It is scoped to this playtest.
A texture of another world group's archive is refused (`check_group`); there is
no aliasing across groups. Whether the stored UV origin matches Bevy's is
unmeasured (`UvOrigin`), so textures are drawn with the stored UVs unflipped.

Measured on the pinned pair (`C1C` + `bloodhawk`, `rtexture10.zbd`, 640 × 480,
Apple M5 Max / Metal), with the whole intact airframe drawn (PLAYTEST-FULL-AIRCRAFT):
**51 of 52** area materials and **16 of 16** aircraft materials resolve (the one
other area material is a flat-colour record, which names no texture); 365 of 367
area parts and 37 of 37 aircraft pieces (over every drawn binding) are textured;
no material is unresolved. The area binds 51 distinct images and the aircraft 16.
Distinct colours over the pixels the aircraft / the area contribute, neutral
baseline against textured:

| view | aircraft colours | area colours |
| --- | --- | --- |
| `chase` | 203 → 1 457 | 105 → 2 767 |
| `quarter` | 175 → 1 292 | 137 → 1 641 |
| `overview` | 10 → 20 | 123 → 1 609 |

The frames are under `private/evidence/PLAYTEST-TEXTURES/{neutral,textured}/`
(Git-ignored; nothing original is committed). Reproduce:

```sh
CS_GAME_DIR="$CS_GAME_DIR" cargo test -p cs_app --test playtest_retail -- \
  accept_playtest_textures_ --include-ignored
```

`PlaytestConfig::textured = false` renders the neutral baseline the textured
frames are compared against. Teardown releases every material, image and mesh the
scene created (tested over three spawn/teardown rounds). The `playtest sources`
line and the smoke `report.json` carry the chosen archive and the textured and
neutral counts.

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

Hiding the cards also shrank the area's measured extent, which moved two
designed values that derive from it. The start length fraction was retuned
(see above); and the review caught that the `overview` camera's eye, placed at
`centre + 0.5·span_z`, then sat exactly **on** the extent's aft plane — at the
airship's own aft tip, inside the hull's silhouette — so the outboard spawn was
occluded and the overview frame carried **zero** aircraft pixels (failing the
`c1c`, nose and textures capture tests). The overview eye now stands a full
span from the centre, half a span beyond the aft face, so "from outside the
area" holds by construction; re-measured, the overview carries 24 aircraft
pixels again.

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
