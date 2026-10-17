# #1553: the world import owns the `horizon` environment subtree

Date: 2026-10-17. Task: #1553 `M01-WINDOW-FARIMPORT` — "import the world node's
descendant subtree so its far-terrain/horizon zones become world objects", owner
#1537 bullet 1. Feature sheets: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`
(the world import and its collision roles) and `specs/F19-sky-atmosphere-weather-and-visibility.md`
(the sky the `horizon` container carries). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: **`retail`** (read-only
`$CS_GAME_DIR`) and ordinary build/test. `gpu` and `audio` were available and
**not used**: nothing is rendered or played and no original run happened, so
nothing here is `verified_original`.

## What the owner saw, and what the import dropped

The owner's windowed M01 draw was only the water/cloud tiles and the `fvol*`
boxes: the island the mission is set on was absent. The split of #1537
(`rally/1537-m01-terrain-is-untextured-and-unlit-ends`, head `b6bbe113`) measured
the cause: `cs_content::world::import_world_container` imported exactly
`partition grid ∪ the world node's own stored child list`, and the island hangs
one level below the world node, under the record `horizon`:

```
world1#0
  └─ horizon#789              (direct child; mesh_index = -1)
       ├─ zone1#795 ─ g1164#796     mesh 156  far terrain
       └─ zone2#790
            ├─ g1155#792           mesh 153  far terrain
            ├─ moon#791            mesh 152
            ├─ stars#793           mesh 154
            └─ h_zone2scroll#794   mesh 155
```

## The measurement: the literal "descendant subtree" is not importable

The task's rule as written ("every record reachable from the world node's
reconciled children") was measured before it was implemented, and it cannot be
implemented faithfully. The world node's direct children are **not only** the
horizon: they are the world's spatially indexed geometry, its fog volumes, its
vegetation instances and its **model instances** (zeppelins, ships, vehicles,
trains, hangars, refineries), each of which parents a whole part hierarchy. Taken
literally, the descendant closure is:

| container | direct children | descendant closure | of which `object3d` | of which `lod` |
| --- | --- | --- | --- | --- |
| `zbd/c1/gamez.zbd`  | 412 | 6 262 | 5 581 | 681 |
| `zbd/c1b/gamez.zbd` | 233 | 4 903 | 4 483 | 420 |
| **`zbd/c1c/gamez.zbd`** | **346** | **5 004** | **4 560** | **444** |
| `zbd/c2/gamez.zbd`  | 282 | 3 925 | 3 546 | 379 |
| `zbd/c2b/gamez.zbd` | 338 | 4 285 | 3 903 | 382 |
| `zbd/c3/gamez.zbd`  | 453 | 4 419 | 4 050 | 369 |
| `zbd/c4/gamez.zbd`  | 401 | 7 162 | 6 498 | 664 |
| `zbd/c5/gamez.zbd`  | 576 | 9 380 | 8 271 | 1 109 |

Three measured facts make the literal reading wrong rather than merely large:

1. **The import refuses non-`object3d` records.** Every `lod` record above would
   reach [`WorldImportError::WorldOwnedNotAnObject`] — the loop's own contract —
   so the literal rule does not even run on the corpus. `moon`/`stars` in the
   horizon subtree are `object3d`; the LOD selectors that hang under a zeppelin
   or a vegetation instance are not.
2. **The closure is dominated by model-instance parts**, not world geometry. A
   zeppelin's turrets, panels and gasbags are children of the zeppelin root
   (`piratezep` 792 records, `blackswanzep` 680, `workersvoyagezep` …), and each
   vegetation `g27816` instance is 37 records including one LOD. They are placed
   by their parent, and a `WorldObjectInstance` carries **one world-space
   transform and no parent link**, so importing them standalone would place every
   part at its parent-relative stored transform. The horizon subtree is the one
   case where this is not so: its meshes store world-space vertices and identity
   transforms (`zone1`/`zone2` are transform groups).
3. **There is no stored marker** that separates the two: a zeppelin part and a
   horizon-zone record have the same kind, the same flag word and the same
   `unk140` box shape. The only thing that distinguishes them is the name of the
   ancestor they hang under.

## The adopted rule

The import owns the world node's direct children (unchanged) **plus the
descendants of its one `horizon` child**, under the same parent-slot rule the
hierarchy conversion uses. Two designed claims carry the reasoning:

* [`HORIZON_CONTAINER_IS_THE_SKY_HIERARCHY`] (`f18-world.horizon-container-is-the-sky-hierarchy`):
  the world node's child named `horizon` is the sky/far-terrain container.
  Measured over all eight containers: exactly one such child each, it hangs
  directly off `world1`, and its children are the `zone*`/`g*` records. Task
  #541 measured the original binding the node by that name
  (`CameraSetHorizon horizon` through the node registry,
  `docs/findings/2026-10-11-t541-horizon-commands-anchor-scenery-to-camera.md`),
  so the import and the production camera path key the same record.
* [`HORIZON_SUBTREE_RECORD_NEVER_BLOCKS`] (`f18-world.horizon-subtree-record-never-blocks`):
  every record in that subtree is environment presentation and resolves
  [`WorldCollisionRole::None`], with its shape an explicit unknown under the same
  claim. The far terrain (`g1155`, `g1164`) and the sky zones (`moon`, `stars`,
  `h_zone2scroll`, the `zone*` groups) are **one subtree**, so one rule covers
  them — the question #1553 asked, answered by the parent-slot walk.

The role rule is what keeps the closure intact: the five mesh-bearing horizon
records are neither indexed nor `fvol*`, so without it they would resolve
[`UNINDEXED_ROLE_UNMEASURED`], `objects_unresolved_collision` would rise by five,
and `mission_launch::geometry_verdict` would return `Unknown` and M01 would
refuse to launch. With it, the subtree resolves `None` and the launch verdict
stays `Satisfied`.

## What moved

Per container, the environment subtree adds `8 / 6 / 7 / 5 / 5 / 5 / 7 / 4`
objects (measured above), every one resident and non-blocking. For `c1c`:

| quantity | before | after |
| --- | --- | --- |
| imported objects | 346 | **353** |
| objects with a mesh | 309 | **314** |
| resident | 53 | **60** |
| mesh-binding records elsewhere | 3 045 | **3 040** |
| `objects_unresolved_collision` | 0 | **0** |

The pinned tables in `crates/cs_app/tests/world/world_units.rs` (all eight
containers), `crates/cs_app/tests/world/import_retail.rs` (`c1c`),
`crates/cs_app/tests/world/residual_roles.rs` and
`crates/cs_app/tests/accept_m01_lc_world_unit_roles.rs` were **re-measured**
through the production readers, not loosened. The relation
`values + child_list + environment == objects` replaces the old
`values + child_list == objects`; the unindexed split
`anchors + fog + unresolved == stored_child_list` is unchanged because the
environment records are a class of their own.

## Tests

* `crates/cs_app/tests/campaign/m01_window_world_import.rs` —
  `accept_m01_window_world_imports_the_horizon_subtree_and_stays_launchable`
  (`#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]`). Drives
  `read_world_container` → `definition` → `import_world_container`, asserts
  M01's definition owns node slots 790–796 with their catalog meshes, that each
  resolves `None` under the environment claim, and that `geometry_verdict` and
  `plan_mission_launch` stay `Satisfied`/`launchable`.
* **Sensitivity**, run against the owner's installation and reverted:
  (a) disabling the environment arm of the role match failed the member at
  `node-790's collision role is answered by the environment rule`;
  (b) removing the subtree import entirely failed it at
  `the import counted exactly the horizon child's descendants` (`0 != 7`).

## Unknowns and limitations (recorded, not guessed)

* **The general descendant closure is not imported.** The model instances'
  part hierarchies (zeppelins, vegetation, vehicles, trains, hangars, aircraft)
  stay `mesh_binding_records_elsewhere`; they are other subsystems' content
  (F20/F33/F34/F35) and this flat `WorldDefinition` cannot represent them.
  Importing a part hierarchy standalone needs parent-transform composition and a
  `lod` policy the container does not state — a separate design question, not
  a silent default. **Affected content:** any claim that M01's world definition
  contains a zeppelin/vehicle's drawn parts.
* **The `horizon` scope is a designed rule over a measured name.** The engine's
  own use of the name is measured (#541); that *this import* should own exactly
  that subtree is this project's rule, stated with its own claim id, not a
  measurement of how the 2000 engine streamed the world.
* **No contact was measured for an environment record**, and no original run
  happened: `None` is this project's designed non-blocking role, at
  `observed_tool`, never `verified_original`.
* **Nothing derived from the original bytes is committed.** The tables above are
  counts and dimensions; no name list, no mesh and no screenshot.

## Sources used

* `crates/cs_content/src/world.rs` — the import, its claim ids, and the
  hierarchy rule the environment walk follows.
* `crates/cs_formats/src/gamez/nodes.rs` — the decoded records
  (`RawNode::parent`/`children`, `RawObject3dData`, the three stored boxes).
* `crates/cs_app/src/camera/horizon.rs` and
  `docs/findings/2026-10-11-t541-horizon-commands-anchor-scenery-to-camera.md` —
  the measured `horizon` node identity (#541).
* `docs/findings/2026-10-04-m01-lc-world-import.md` and
  `docs/findings/2026-10-03-f18-world-hierarchy-authority.md` — the import and
  the parent-slot rule this extends.
* `docs/contracts/IDENTITY-CONTENT.md` — `Resolved<T>`, stable ids and evidence
  classes.
