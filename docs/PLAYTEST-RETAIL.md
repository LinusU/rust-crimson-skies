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
ORIGINAL ASSETS / DEVELOPMENT FREE FLIGHT / PROVISIONAL TUNING
```

`crates/cs_app/src/playtest_retail.rs` exports it as `PLAYTEST_LABEL` and a
test asserts it verbatim and asserts that it never contains `M01`, `faithful`,
`campaign` or `verified_original`.

## What is rendered

| | chosen value | why this one |
| --- | --- | --- |
| world container | `ZBD/C1C/gamez.zbd` | the group #629 imported, so this lane reuses its measured reader and its exact per-container mesh naming |
| area | the node subtree of stored node slot **517**, authored name `piratezep` | a record of the world node's own stored child list; measured 793 nodes, 401 of them binding a mesh, **8 673 stored triangles**, composed extent 106.3 × 217.7 × 839.5 canonical m |
| aircraft container | `ZBD/planes.zbd` | the shared aircraft archive |
| aircraft airframe | the root named **`bloodhawk`**, found by name through the production `SceneGraph::root` | F11's identity rule: an airframe references a root in PLANES.ZBD, never a mesh-array position |
| aircraft meshes | the **whole intact airframe** (#665): the subtree of node slot **2296** `healthy`, one LOD band (`nearest`, slot 2299) selected by the F11-B rule, plus the static propeller node slot **2541** `staticprop1` | measured 16 mesh bindings, 927 triangles, composed extent about 11.6 × 2.7 × 10.6 units; 24 of the airframe's 40 bindings are listed as undrawn |

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
| spawn | `min + (−0.6·width, 0.55·height, 0.5·depth)` of the measured extent → `(−116.97, −40.85, 196.86)` | `playtest-retail.aircraft-pose-is-designed` |
| aircraft nose | one half turn about `+Y`, mapping the **measured** stored nose (`+Z`) onto the runtime's forward axis (`−Z`) | same |
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
| `chase` | `(−1.8·length, +0.7·height, +0.6·length)` = `(−135.39, −39.80, 203.00)` | the spawn | **abeam, outboard.** Behind the aircraft it would show a 2 m cross-section (measured: 807 aircraft pixels); outboard, because an inboard eye puts the *camera* between the aircraft and the area and frames only sky (measured: refused as `NoEnvironment`) |
| `quarter` | `(−1.4·length, +1.2·height, −1.6·length)` = `(−131.30, −39.06, 180.48)` | the spawn | the same placement from higher and further forward, so the second aircraft frame is a different angle rather than the same one twice |
| `overview` | `centre + (−0.42·span_x, +0.36·span_y + 3·height, +0.5·span_z)` = `(−44.66, 31.13, 616.61)` | the area's centre | frames the **area**: the whole airship with the aircraft a measured speck in front of it |

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
* **Not textured.** The per-material texture binding (F10-C.02's dependency audit
  through F17's image upload) is **not** built here, so every surface is flat-shaded
  and no material identity is claimed. Drawing a stored texture is the next step,
  and `PLAYTEST-RETAIL-HANDOFF` should take it rather than mistake this stage's
  flat shading for a property of the original.
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
| which stored axis is an airframe's nose | the half turn follows a measured hint (the propeller disc's plane), and the mapping is a designed choice | an original run |
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
