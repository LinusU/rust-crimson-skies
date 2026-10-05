# #648: the visible-content hop, and one original area plus one original aircraft

Date: 2026-10-05. Task: #648 "Render one original world area and aircraft for
free-flight playtest" (`PLAYTEST-RETAIL-SCENE`), the render lane of the
free-flight experiment and the input to #649 `PLAYTEST-RETAIL-HANDOFF`. Feature
sheets: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`
(`### F18-A`, `### F18-B`, `### F18-D`) and
`specs/F10-gamez-mesh-topology-and-material-records.md` (`### F10-C.02`,
`### F10-C.03`). Shared contracts: `docs/contracts/IDENTITY-CONTENT.md`,
`docs/contracts/CLI-EVIDENCE.md`. Capabilities used: **`retail`** (read-only
`$CS_GAME_DIR`) and **`gpu`** (one real renderer, `Apple M3 Pro` on Metal).

**This is render evidence, not `verified_original`.** No original run happened;
`retail` is file access. Every count below is measured from the original bytes by
the production readers (`ObservedTool`), and every tuning value is `Designed`.
Nothing derived from the original bytes is committed: the frames live under
`private/evidence/PLAYTEST-RETAIL-SCENE/`, which Git ignores.

The user-facing document is `docs/PLAYTEST-RETAIL.md`. This file is the design,
the measurement and the limitation record.

## Files

- `crates/cs_app/src/playtest_retail.rs` (new, the whole stage):
  `read_playtest_sources`, `area_graph`, `aircraft_graph`, `playtest_adapter`,
  `spawn_playtest_scene`, `settle_playtest_colliders`, `teardown_playtest_scene`,
  `playtest_app`, `capture_playtest_views`, `spawn_pose`, `camera_poses`, the
  reports, the errors and every pinned constant.
- `crates/cs_app/tests/playtest_retail.rs` (new): the eleven
  `accept_playtest_retail_` tests, three of them retail.
- `crates/cs_app/src/world/retail.rs` (minimal adapter change, an F18 owner path):
  `render_mesh` → **`pub fn stored_render_mesh`** and
  `presentation_unknowns` → **`pub fn stored_presentation_unknowns`**. The playtest
  scene reads one world container's stored meshes and must upload them through
  **this** builder and declare **this** presentation-unknown list, or the project
  would have two of each (AGENTS rule 7).
- `crates/cs_app/src/lib.rs` (wiring only): `pub mod playtest_retail;`.
- `docs/PLAYTEST-RETAIL.md` (new): the label, the chosen world/aircraft/start
  transform/scale/material overrides and the exact command.
- This file.

No reader refusal was weakened: `git diff` touches no line of `cs_formats`,
`cs_types` or `cs_assets`, and no line of any `cs_content` module.

## The measurement: what #629's import leaves on the floor

`docs/findings/2026-10-04-m01-lc-world-import.md` measured that `ZBD/C1C/gamez.zbd`
imports 346 records from the world record's partition grid and stored child list.
This stage measured what is **left**, over the same container through the same
production readers:

| container fact | value |
| --- | --- |
| nodes | **5 644** |
| mesh-array slots | 2 250 |
| triangles in the **whole** mesh array | **21 779** |
| nodes named by the partition grid + the stored child list (#629's import) | **346** |
| triangles those 346 records draw | **1 786** |
| the pinned subtree's nodes / mesh bindings / triangles | **793 / 401 / 8 673** |
| largest single mesh in the container | 317 triangles |

So the partition-grid import is **6.1 % of the container's nodes** and **8.2 % of
its triangles**. What the 346 records draw is the world's *spatial index*: 144
flat 1 024-unit quads at `y = 960` (2 triangles each), 20 volumetric fog boxes
(`fvol1`…`fvol20`, 6–106 triangles each, `y ∈ [970.7, 1524.5]`), a 17 km horizon
dome (5 meshes, 118 triangles, `y ∈ [−5568.8, 2374.7]`) and ~45 vegetation
instances (`g27816`, 34–37 tiny meshes each). The world's **visible content** —
the airships' hulls, gas bags, rigging, interiors, panels, turrets and engine
cowls — is in the other 5 298 nodes and is reachable only by descending the
hierarchy.

That is the gap: a mission could load an original world and see almost nothing in
it. This stage closes it for one documented subtree.

### Why `piratezep` and not something else

The world record's stored child list holds 53 records. Measured over their
subtrees, the five airships are the only pieces of real content:
`piratezep` 793 nodes / 401 meshes / 8 673 triangles; `workersvoyagezep` 822 / 417 /
7 471; `blackswanzep` 681 / 315 / 6 025; `multiplayer1zep` and `multiplayer2zep`
622 / 296 / 5 803 each. `piratezep` is the densest, so it is pinned. Its composed
extent is **106.3 × 217.7 × 839.5 m** — an 840 m airship, which is room to fly.

### The aircraft

`ZBD/planes.zbd` holds 3 317 nodes, 2 250 mesh slots and **28** parentless roots,
all `object3d`. Measured over the roots: 12 are `player_*` or effect roots with
one or two children, and the vehicle airframes (`bloodhawk`, `warhawk`, `fury`,
`avenger`, `balmoral`, `kestrel`, `firebrand`, `autogyro`, `peacemaker`,
`brigand`, `piratefighter`) have five children each. **No root binds a mesh of
its own** (`mesh_index = −1` on all 28), which is exactly why the mesh identity
has to come from a named root and then a slot inside its subtree.

The pinned aircraft is `bloodhawk`'s `fuse03` node — stored slot **2 525**,
mesh-array slot **1 436**:

| fact | value |
| --- | --- |
| stored triangles (over the production F10-E builder) | **140** |
| stored material groups | **3** |
| stored extent | **2.111 × 1.493 × 10.233** units |
| composed transform | the **identity** — the mesh is already in its airframe's own frame |
| the airframe's other mesh bindings | 40 more, **not claimed** |

**The measured nose hint.** `prop1` (node 2 542, mesh 1 449) is a disc of radius
1.20 in the stored `x`/`y` plane at `z = 0`, and its node's composed translation is
`z = +4.80` — the fuselage mesh's own maximum. A propeller disc whose normal is
`±z`, placed at the end of the body, says the stored nose is `+Z`. The scene's
half turn about `+Y` maps that onto the runtime's forward axis (`−Z`). **The hint
is measured; the mapping is designed** (`playtest-retail.aircraft-pose-is-designed`),
because which stored axis the original engine called "forward" is unmeasured and an
original run has not happened.

## Design decisions

- **The area's scene graph is built over a subtree, through the same validator.**
  `cs_content::scene::SceneGraph::build` refuses the **container-wide** `c1c`
  hierarchy: measured, node 642 names parent slot 0 while the world record's child
  list does not name it back, and the validator refuses that as
  `SceneError::InconsistentParentage`. #629 recorded the same blocker from the id
  side. The alternative — composing the transforms here — would have been a second
  implementation of one canonical rule (AGENTS rule 7), so `area_graph` narrows the
  node set instead: the world record plus the documented subtree, with the world
  record's child list narrowed to the subtree root. Every production check still
  runs (unique slots, in-range links, parent↔child agreement on both sides,
  acyclicity, derived ids, finite transforms), and the composed transforms are the
  container's own because the only ancestor above the subtree root is the world
  record, whose kind (`NodeKind::World`) carries no transform and therefore
  converts to the identity. **Unresolved:** the `scene_node` id / hierarchy blocker
  itself, which is a `cs_content::scene` change and outside this task's owner paths.
- **One discovery pass for both containers.** `read_playtest_sources` calls
  production `install::discover` once and reads both containers out of the one
  manifest. Two passes would have cost a second full fingerprint of a 500 MB
  installation (~3.5 min) for nothing.
- **The mesh naming rule is #629's.** `c1c.mesh-<n>` / `planes.mesh-<n>`, produced by
  the same `<group>.mesh-<index>` rule `RetailWorldContainer::mesh_key` writes, so a
  mesh the area draws and a mesh the world import draws are one catalog element.
  Still a **per-container** name rather than a shared-catalog element: #638's seam,
  named rather than papered over.
- **Object identity is the stored node slot**, `playtest.node-<slot>`, for #629's
  reason: a container stores many records under one authored name (34 records named
  `g27816` in one group), so the slot is the only value the key grammar always
  accepts.
- **Collision is the drawn mesh, declared.** Every area record is `Solid` +
  `FromMesh`, so Avian derives a `TrimeshFromMesh` collider from the very mesh the
  record draws and the two cannot diverge (F18 non-negotiable behavior 1). The
  world spawn presents geometry **without** a material, so the scene attaches its
  declared neutral material to every drawn node afterwards; the aircraft gets its
  own colour so a reader can tell the two apart in a frame.
- **A record the spawn refuses is reported, not fatal.** One unplaceable record
  must not take a 401-record area down, so refusals land on
  `PlaytestAreaReport::refused` and a missing upload on `.gaps`. On the pinned
  installation both are empty, and the retail test asserts that.
- **The scene owns its entities.** `PlaytestScene::entities` is the complete list,
  and `teardown_playtest_scene` despawns exactly it and drops the loader's owning
  `WorldMeshAssets` resource. A spawn/teardown/spawn cycle returns to the same live
  entity count and a zero asset count, and the reload's aircraft is a **new**
  entity with the **same** upload fingerprint.
- **`playtest_app` is its own composition.** `crate::world::fixture::world_app` is
  headless and has no renderer, and adding render plugins to it would mean two asset
  stacks (Bevy refuses the duplicate). So this module states its own: Bevy's
  defaults with the window plugin disabled — which is where `AssetPlugin`,
  `MeshPlugin`, `ImagePlugin` and `WorldSerializationPlugin` come from — plus
  Avian's physics plugins and this crate's passes, with `TimeUpdateStrategy`
  manual, one substep, **zero gravity** (this stage owns no flight loop, so nothing
  here may fall), then `finish()` and `cleanup()` **before** the caller spawns.
  Measured: driving an app that was left in its building state panicked a Bevy
  system on the compute task pool with "Parameter failed validation: Resource does
  not exist", because the plugins' `finish`/`cleanup` hooks that register the render
  extract systems never ran.
- **Two GPU tests write two artifact directories.** The test harness runs the
  binary's tests in parallel, and each builds its own Bevy app and its own
  renderer. Sharing one output path let one test's refusal delete the other test's
  PNG and failed the run on `the artifact the capture reports exists on disk`, so
  each test writes `private/evidence/PLAYTEST-RETAIL-SCENE/<test>/`.
- **Each view is rendered twice.** With the aircraft presented, and with it hidden.
  The pixel difference is the aircraft's own contribution and the hidden frame is
  the environment's own, both **measured**. Only the first is written to disk, so
  one documented view is one PNG. A view that drew nothing, framed only sky or
  showed no aircraft is refused **and its file is removed**, so a file that exists
  is a frame that was measured.
- **The lighting is derived, not chosen by eye.** Bevy's Lambert term is
  `albedo · E / π`, so an illuminance near π renders a surface at approximately its
  own albedo. Measured: at the first value tried (40 000 lux, which is what
  `gpu_capture` uses) every albedo above ~0.08 clipped to white and the aircraft
  was indistinguishable from the area. At 3.2 lux the aircraft's declared red reads
  as red.

## Test inventory

| `accept_playtest_retail_` test | Runs without retail | Covers | Fails when |
| --- | --- | --- | --- |
| `a_missing_installation_is_refused_by_name` | yes | the production loader's first step is discovery, so a wrong `CS_GAME_DIR` is refused by name instead of producing an empty scene | the loader stops refusing, or refuses without naming anything |
| `a_directory_without_the_containers_is_refused_by_key` | yes | a readable directory holding neither container is `Absent`, naming `zbd/c1c/gamez.zbd` | a missing container is drawn as an empty area |
| `every_designed_value_is_recorded_under_its_own_claim` | yes | six claim ids, valid and distinct; the label verbatim and free of `M01`/`faithful`/`campaign`/`verified_original`; the documented configuration | a claim id is malformed or shared, or the label inflates |
| `the_declared_unit_is_canonical_and_uncalibrated` | yes | the scene renders under the workspace's declared **canonical** source, whose calibration class is `Unknown` and whose origin is not an installation | a bespoke adapter is used, or the unit is claimed as measured |
| `the_spawn_is_outside_the_measured_area_and_facing_forward` | yes | against the **measured** composed extent: off the area's own side, inside its height, amidships, a quarter of its width of clear air ahead; and the declared rotation maps the measured stored `+Z` onto `-Z` about the vertical axis | the spawn lands inside the geometry, or the nose mapping changes |
| `a_degenerate_area_is_refused_by_name` | yes | a zero-extent area is `DegenerateArea` carrying the extent it saw | a degenerate area produces a pose or a frame |
| `the_three_camera_views_are_finite_and_frame_both_subjects` | yes | three named views; every pose finite; every eye outside the aircraft's bounding sphere; the two aircraft views at the spawn and within the declared 1–8-aircraft-length band; the overview at the area's centre and somewhere neither other view stands | a view goes non-finite, ends up inside the subject, stops framing the aircraft or duplicates another |
| `a_non_finite_pose_input_is_refused_by_name` | yes | a `NaN`/infinite spawn or aircraft extent is `NonFinite` naming the component | the framing function clamps a non-finite input away (it did: `f64::max` swallows a `NaN`, so the check had to move **before** the one-metre floor) |
| `retail_c1c_area_and_bloodhawk_mesh_spawn_and_capture` (retail) | no | the whole criterion set: the measured counts (793 / 401 / 8 673), no refusal and no gap, every record's shape/provenance/mesh-table resolution, the aircraft's slot/triangles/groups/extent/identity transform, finiteness, the material decision, **every derived collider a triangle mesh carrying exactly its record's triangles** on the same entity that presents it, one asset per distinct mesh, three real captures with their measured pixel facts, and a teardown/reload cycle | any count moves, a collider stops being the drawn mesh, a frame loses the aircraft or the area, or a teardown leaves an entity or an asset behind |
| `retail_a_pinned_slot_holding_another_record_is_refused_by_name` (retail) | no | an absent area slot, a slot holding a different record, an absent airframe root and a mesh slot outside the airframe are each refused **by name**, and none leaves an entity behind | a pinned choice is silently substituted, or a refusal half-applies |
| `retail_a_capture_that_shows_no_aircraft_leaves_no_file` (retail) | no | with the aircraft moved out of every frustum the capture is `NoAircraft` **and** no PNG for any view exists on disk | an aircraft-less frame is accepted, or a refused view leaves a file |

**Sensitivity.** Five mutations were applied and the non-retail selection re-run;
all five are killed by a test CI can run (the two retail tests were not needed for
any of them):

| mutation | killed by |
| --- | --- |
| the six claim ids collapse to one | the claim test |
| `spawn_pose` puts the spawn at the area's centre | the spawn test |
| the camera views' aircraft target becomes the area's centre | the view test |
| `camera_poses` floors the aircraft extent **before** checking it is finite | the non-finite test (the real defect this stage found in itself) |
| the label loses its `PROVISIONAL TUNING` half | the claim test's verbatim comparison |

## Unknowns and limitations (recorded, not guessed)

- **The original's world-vertex unit and coordinate handedness are UNMEASURED**
  (task #436, blocked). Every metre here is "stored units × a declared factor",
  and the factor is 1.0 under the workspace's canonical source at `Unknown`
  calibration. **Affected content:** every extent, every position and therefore any
  gameplay-distance claim over retail geometry. **Resolving task:** #436.
- **The original's collision classification is UNMEASURED.** The area's records are
  declared solid and collided from their drawn mesh. Whether the original collided a
  panel, an engine cowl or a cloud the same way is not measured, and #629's own rule
  ("an indexed record is a collider") is a *different* rule over a *different*
  record set.
- **The original's gameplay surfaces are UNMEASURED.** Every record carries an
  explicit unknown surface (the container states none), so a contact inherits no
  ground or water rule.
- **The original's lighting, sky, weather and audio are not built here.** The sky is
  a declared colour and the key light a derived illuminance; both are stated as
  development values.
- **No stored texture is drawn.** The per-material texture binding (F10-C.02's
  dependency audit through F17's image upload) is not built, so the material path
  is the one place this stage is deliberately short: one neutral development
  material, reported once on `PlaytestMaterial` with the container keys and mesh
  counts it covers. **Affected content:** the appearance of every surface.
  **Resolving task:** #649 `PLAYTEST-RETAIL-HANDOFF` should take it.
- **The container-wide `c1c` hierarchy is refused by the production validator**
  (node 642 ↔ parent slot 0), so the scene graph is built over the documented
  subtree. **Affected content:** any consumer that wants the whole container's
  scene graph. **Resolving task:** the `scene_node` id blocker filed in
  `docs/findings/2026-10-04-m01-lc-world-import.md`.
- **One mesh is one mesh.** The airframe's other 40 mesh bindings are not drawn, so
  the frame shows a fuselage rather than a complete aircraft. Deliberate: a precise
  subset beats a collection of half-read records.
- **The mesh identity is per-container.** `c1c.mesh-<n>` and `planes.mesh-<n>` are
  this module's and #629's naming, not the shared catalog's, so two containers can
  hold the same mesh under different ids. **Resolving task:** #638.
- **Evidence class.** The facts are measured from the original bytes by the
  production readers, which makes the layout and the counts `ObservedTool` +
  measurement. Every **rule** this stage adds — the area selection, the unit
  reading, the collision classification, the pose, the views, the material — is a
  designed engine contract carrying its own claim id. No original run happened:
  `retail` is file access, not evidence of runtime behaviour. **A fresh-context
  reviewer should read this format work**, and no agent review replaces the owner's
  approval.
- **Nothing derived from the original bytes is committed.** The frames are under
  `private/evidence/PLAYTEST-RETAIL-SCENE/<test>/`, which `.gitignore` excludes
  (`/private/` is anchored at the workspace root, which is why the capture
  directory is resolved from `CARGO_MANIFEST_DIR` rather than from the test
  process's working directory); the numbers above are counts, dimensions and
  relations.

## Follow-ups filed

- **#649 `PLAYTEST-RETAIL-HANDOFF`** — the one-command free flight over this scene
  source, and the natural place to add the stored-texture material path.
- **#639** — all eight world containers; this stage measured eight of them but
  renders one subtree of one of them.
- **#638** — the shared render-mesh catalog as both containers' mesh source.
- **#436** — the original's world-vertex unit and handedness.
- **#645** — one rule for a stored `object3d` transform. Measured again here and
  still latent: `cs_content::scene::canonical_local` takes the stored matrix only
  when it disagrees with the euler triple, while `cs_content::world`'s importer
  always takes it, and this stage goes through the **scene** rule. Every record in
  the pinned subtree renders the same way under both (the disagreement count for
  world-owned records is 0 in `c1c`), so nothing observable differs today — but two
  rules for one conversion remain a canonical-contract defect.

## Sources used

- `crates/cs_app/src/world/retail.rs` (`read_world_container`,
  `RetailWorldContainer::mesh_key`), `spawn.rs` (`spawn_object`, `SpawnedWorld`,
  `WorldMeshAssets`, `SkipReason`, `MESH_SETTLE_UPDATES`), `meshes.rs`
  (`WorldMeshes::insert_render_mesh`), `fixture.rs` (`MESH_SETTLE_UPDATES`, the
  headless composition whose resources and `finish`/`cleanup` order this stage
  mirrors), `gpu_capture.rs` (the offscreen capture pattern: an image render
  target, an explicit clear colour, no back-face culling, warmup updates, a
  uniform-frame refusal, and deleting the PNG on every refusal).
- `crates/cs_content/src/scene.rs` (`SceneGraph::build`, `parsed_nodes_from_gamez`,
  `scene_graph_from_gamez`, `MeshBinding`, `MeshSlot`), `mesh.rs`
  (`RenderMesh::from_stored_groups`, `MeshPresentationUnknown`),
  `world.rs` (`WorldObjectInstance::resident`, `WorldDefinition::try_new`,
  `Aabb`, `WORLD_SURFACE_UNMEASURED`), `coordinates.rs` (`SourceAdapter::declared`).
- `crates/cs_formats/src/gamez` (`read_gamez_nodes`, `read_gamez_meshes`,
  `GameZMeshes::get`, `GameZMesh::groups`, `groups_are_complete`, `NodeKind`).
- `crates/cs_assets/src/install.rs` (`discover`, `fingerprint`, `sha256`).
- `docs/findings/2026-10-04-m01-lc-world-import.md` (the import this stage reuses
  and the hierarchy blocker it works around),
  `docs/findings/2026-10-04-m01-lc-world-scene-ids.md`,
  `docs/findings/2026-09-29-f10-c-02-gamez-material-records.md` and
  `docs/findings/2026-09-30-f18-d-world-group-audit-and-gpu-capture.md` (the
  material-audit and capture precedents).
- `docs/contracts/IDENTITY-CONTENT.md` (stable ids, `Resolved<T>`, evidence
  classes) and `docs/contracts/CLI-EVIDENCE.md` (what an artifact has to be before
  it is evidence).

## Commands run

See the handover summary. The four checks plus:

```sh
CS_GAME_DIR="$CS_GAME_DIR" cargo test -p cs_app --test playtest_retail -- \
  accept_playtest_retail_ --include-ignored
#   11 tests: 8 run and pass, 3 retail run and pass
```
