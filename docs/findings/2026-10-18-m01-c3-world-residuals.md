# #1578: `c3`'s 47 grid records that store no mesh index — the grouping nodes, and M01's launch

Date: 2026-10-18. Task: #1578 `M01-C3-WORLD-RESIDUALS` — "classify c3's 47 grid
records that store no mesh index so the world-geometry verdict is answered".
Feature sheet:
`specs/F18-world-geometry-terrain-water-and-traversable-interiors.md` (`### F18-B`,
`### F18-D`). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`. Capabilities
used: **`retail`** (read-only `$CS_GAME_DIR`) and **`gpu`** (the owner-command
smoke, which opens the real window), plus ordinary build/test. `audio` was
available and **not** used: nothing is played. No original executable was run as
a *measurement* beyond that smoke, so nothing here is `verified_original`.

## Why this task exists

Since #1565 bound M01 "The Lost Treasure" to `ZBD/C3/M01`, the owner's command
(`cs --cs-path $CS_GAME_DIR --mission M01`) refused before any window opened:

```text
M01 cannot launch: world_geometry: unknown ... (256 partition cells, 458 objects) ...
47 grid records that store no mesh index
```

Nothing in the suite caught it: `accept_vs_m01_geometry_verdict_retail_*` had
been re-pinned to the `M06` label, so no retail test planned the container M01 is
actually set in. The 47 records are the ones this task classifies.

## Provenance and method

Two independent things, then bound together:

1. static analysis of the owner-supplied decrypted image (`$CS_ENGINE_IMAGE`,
   sha256 `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`,
   image base `0x400000`, the same address convention as
   `docs/findings/2026-10-05-f04-d-original-lookup-order.md`), re-reading the two
   routines task #771 measured — `cls_zbd.c`'s node pass (`0x4e2a7c`–`0x4e2aa6`)
   and `cls_di.c`'s intersection walk (`0x4cb5eb`–`0x4cb674`) — as the rule the
   new arm is keyed on;
2. the production readers over the installation, for every count.

**This is code-derived evidence plus a byte census.** Nothing here is
`verified_original`; the evidence class of everything touched by this task is
`observed_tool` (plus the smoke's own `gpu` capture, which is a rendering
observation of *this* build, not of the original).

## A. What the 47 records store

Read through the production readers, one production discovery over the
installation and one import per container:

* `zbd/c3/gamez.zbd` names **439** records in its 16×16 (256-cell) partition
  grid; **374** of them bind a mesh, and the remaining **65** bind none
  (`mesh_index = -1`, the store's own "no mesh": the loader skips the
  mesh-table base adjustment for a negative index at `0x4e2aa9`).
* Those 65 split **exactly** in two, and the split is the record's own stored
  selector word `unk112` — the word `cls_zbd.c` rewrites into a pointer to one of
  the record's three boxes (`0` → `+0x74`, `1` → `+0x8c`, `2` → `+0xa4`,
  `0x4e2a7c`–`0x4e2aa6`, with the inverse at `0x4e19e6`):

| store state | `unk112` | boxes (116 / 140 / 164) | count in `c3` | resolved by |
| --- | --- | --- | --- | --- |
| no geometry at all | `0` | empty / empty / empty | 18 | #771, `GRID_RECORD_STORES_NO_GEOMETRY` |
| a box only in the third slot | `2` | empty / empty / **box** | **47** | this task, `GRID_GROUPING_NODE_NEVER_BLOCKS` |

No third state occurs: over all eight containers every mesh-less grid record
stores either nothing or exactly this `unk112 = 2` third-slot box, and none of
them stores the intersection narrow-phase bit (`0x40`). That is the store
partition this task answers.

The 47, measured in detail (counts and shape, not a data dump):

* all are `object3d` records with `unk196 = 160` and `children_count` between 1
  and 14 — **every one of them has at least one child**;
* 38 store flags `0x0308851c` with zone id 1, 9 store `0x0308051c` with zone id
  255 (`ZONE_DEFAULT`); all 47 keep the narrow-phase bit `0x40` **clear**;
* their `object3d` transform flags are `40` (identity) for 33 and `32`
  (transformed) for 14 — they are positioned records, not bare markers;
* their children are **not** in the partition grid and are not the world node's
  direct children: of the 186 child records the 47 name, 54 bind a mesh and 117
  are themselves mesh-less grouping records. So the meshes the box bounds are one
  level below it, and this flat world import does not own them (the same
  limitation #1553 recorded for model-instance part hierarchies).

The stored names of these records read as places and model instances rather than
as volumes — a representative nine: `town_1`, `tikicave`, `craterlake`,
`shipwreck`, `susp_bridge`, `rock_formation`, `treasure_approach`, `studebaker6`,
`trucker1`. That matches the class task #771's section A table already measured
and named: `unk112 = 2` is a **grouping node** whose `unk164` box is the union of
its children's extent (its `c1` node-2752 example). The names are stored display
names; no original asset is reproduced here.

## B. What the image does with such a record — the measured rule

`cls_di.c`'s candidate walk, re-read at instruction level (task #727 measured
that the grid is a broad-phase *candidate* index; task #771 measured its filter):

| step | instruction(s) | address |
| --- | --- | --- |
| record's flags word, bit `0x04` — first filter | `test byte [esi+0x24], 4` / `je` drop | `0x4cb5eb` |
| zone whitelist over the record's byte at `0x30` | `mov cl,[esi+0x30]` / `call 0x56c430` | `0x4cb5f7` |
| optional name filter | `mov edx,[0x71e890]` / null skips it | `0x4cb60b`–`0x4cb62f` |
| **narrow-phase bit** | `mov eax,[esi+0x24]` / `test al,0x40` / `jne` narrow phase | `0x4cb635`–`0x4cb63a` |
| otherwise: recurse into children, or **drop** | `cmp word [esi+0x56], bx` / `jle` drop | `0x4cb63c`–`0x4cb642` |
| narrow phase copies `[node+0x70]`'s 24 bytes | `test ah,1` / `push` / `call 0x4cd960` | `0x4cb665`–`0x4cb674` |

So a grid-named record reaches the only branch that copies its stored box
**only with bit `0x40` set**. With that bit clear the walk descends into the
record's children — and the 47 all have children — so the box this record
stores is **never copied and never tested**. Nothing measured therefore reports a
contact for the record itself, which is exactly the leap tasks #716, #727 and
#771 already made for the six grid-named `fvol*` volumes (those have no
children, so the same branch *drops* them; the branch is the same, only the
`jle` goes the other way).

## C. The binding

`crates/cs_content/src/world.rs`, `import_world_container`'s object loop gains a
third measured exception, between #771's two and the index's default:

```text
1. indexed && name carries `fvol`            → role None / shape unknown  (FOG_VOLUME_RECORD_NEVER_BLOCKS, #716/#771)
2. indexed && no mesh && all three boxes empty→ role None / shape unknown  (GRID_RECORD_STORES_NO_GEOMETRY, #771)
3. indexed && no mesh && grouping box        → role None / shape unknown  (GRID_GROUPING_NODE_NEVER_BLOCKS, #1578)
   (the same store state with bit 0x40 set    → role+shape unknown        (GRID_GROUPING_NODE_ROLE_UNMEASURED, #1578))
4. indexed otherwise                         → Solid / FromMesh           (INDEXED_RECORD_IS_STATIC)
5. the unindexed arms (#677/#716/#1553), unchanged
```

New claim ids:

* `GRID_GROUPING_NODE_NEVER_BLOCKS`
  (`f18-world.grid-grouping-node-never-blocks`) — the resolution, with the
  measured filter in its reason;
* `GRID_GROUPING_NODE_ROLE_UNMEASURED`
  (`f18-world.grid-grouping-node-role-unmeasured`) — for a record of the same
  store class that **does** store `0x40`, whose box the walk would test; it
  keeps the container's silence rather than inheriting `Solid`. No record on this
  installation carries it, which is what keeps the rule a measurement keyed on
  the store rather than a blanket rule about mesh-less grid records;
* `GROUPING_BOX_SELECTOR` (`2`) — the measured stored word, exported so a test
  can re-derive the class from the bytes.

New report accessor: `WorldImportReport::partition_records_grouping_node()` — a
**store census**, like `partition_records_fog_volume()`: it counts every grid
record in that store state, resolved or not.

`crates/cs_app/src/mission_launch.rs::geometry_verdict` gains the term the
verdict needs, so the mesh-less question is answered for c3 exactly when both
measured classes are subtracted:

```text
open roles = report.objects_unresolved_collision()
open mesh  = report.partition_records()
           - report.partition_records_with_mesh()
           - report.partition_records_stores_no_geometry()
           - report.partition_records_grouping_node()
```

and the partition identity every retail test now pins is

```text
partition_records == objects_solid + partition_records_fog_volume
                        + partition_records_stores_no_geometry
                        + partition_records_grouping_node
```

Measured per container for the new counter: `c1` 48, `c1b` 8, `c1c` **0**,
`c2` 49, `c2b` 0, `c3` **47**, `c4` 43, `c5` 24 — 219 records, and together with
#771's 148 empty records they are **exactly**
`partition_records − partition_records_with_mesh` in every container.

## D. What this changes, and what it does not

For `c3` (the M01 container): `objects_solid` 421 → **374**;
`partition_records_grouping_node` 0 → **47**; the spawn's `unknown_mesh` skips
47 → **0**; the spawn's non-colliding count 37 → **84**; `objects_unresolved_collision`
stays **0** (the 47 already had a *known* role — `Solid` — which is why the open
role count never showed the gap) and the collider count stays **374**. The
`world_geometry` verdict goes `Unknown` → **`Satisfied`**, and
`plan_mission_launch(label = M01)` is `launchable()`.

For the other containers the same arm fires wherever the class occurs, so their
pinned counts move the same way (`objects_solid`, the grouping counter, the
spawn's `unknown_mesh` → 0 and its non-colliding count). `c1c` and `c2b` have no
such record and do not move at all. Every test that pinned a number this changed
was **re-measured** through the production path rather than loosened — the
`solid == partition − fog − empty` identities became four-term sums, the spawn's
`unknown_mesh` expectation became the class's own count (now zero), and
#727's retail member now re-derives the grouping class from the containers' own
bytes and checks each one resolves under the new claim. No assertion lost the
property it existed for.

**Not changed:** the sector index, the mesh references, the stored transforms,
the unindexed split (#677/#716/#1553), the fog overlap counter, and the 18 empty
records of `c3` (#771's claim, which this task's class is disjoint from).

## E. `c3`'s `horizon` subtree (#1537 asked)

**Yes.** `c3`'s world node owns a direct child named `horizon` — node slot 3234,
`mesh_index = -1`, two children — and the import owns its **5** descendants under
`HORIZON_CONTAINER_IS_THE_SKY_HIERARCHY` / `HORIZON_SUBTREE_RECORD_NEVER_BLOCKS`
(`report.environment_records() == 5`), exactly as it does `c1c`'s 7. Task #1553's
rule therefore already covers M01's container: the far terrain and sky are
imported and resolve role `None`. A retail member asserts the count, the child's
identity and that every environment record is a descendant of that `horizon`
child.

## F. The owner-command smoke

`docs/PLAYTEST.md`'s owner-command smoke ran on this branch's commit, after the
release build, with `CS_CAPABILITIES=retail,gpu,audio`:

```sh
target/release/cs --cs-path "$CS_GAME_DIR" --mission M01 --smoke-seconds 45 \
  --capture-dir private/evidence/M01-C3-WORLD-RESIDUALS/smoke
```

**It gets past launch planning and opens the real window on `zbd/c3/m01`** — the
acceptance this task was filed for. The run composed `ORIGINAL MISSION M01 /
ORIGINAL WORLD`, bound **377 meshes / 643 drawn parts** from `world/c3`, placed
the player at `(-1426, 150, -1813)`, spawned 3 aircraft and 2 zeppelins, and
wrote **12 real framebuffer captures** (`frame_*_world|sea_visible|sea_absent|
actor_0..5|hud|guns|rocket.png`) plus `report.json`. It then exited **non-zero**,
failing 2 of its 12 checks, both of them other tasks' subjects:

| check | result | what it measures |
| --- | --- | --- |
| `world_terrain_drawn` | **pass** | 3 686 400 of 3 686 400 pixels off the clear colour |
| `world_terrain_textured` | **pass** | 22 per mille local detail (floor 20), 28 731 distinct colours |
| `world_terrain_lit` | **fail** | near field spans luminance 42..47 (spread 5, floor 24) — #1537's subject |
| `world_sea_visible` | *skip* | the world's records declare no known water surface |
| `hud_objective_current` | **pass** | the model marks a current objective |
| `hud_objective_matches` | **fail** | the drawn HUD line is absent — the HUD objective widget gap (#1537's window bullet / the HUD surface) |
| `actors_project_and_cover` | **pass** | all 6 spawned actors project and cover ≥ 8 pixels |
| `weapon_guns_fire` / `weapon_rounds_drawn` | **pass** | 90 rounds, 66 live, all 66 drawn |
| `weapon_rocket_launches` | **pass** | a rocket launched beside a declared-hostile actor |
| `audio_radio_queued` / `audio_engine_queued` | **pass** | 1 radio line, 1 engine loop |
| `smoke_shots_delivered` | **pass** | every requested capture arrived |

So the world this task answered **is drawn**: terrain pixels, textures, actors,
projectiles. The two failures are the unlit/near-field gap #1537 owns and the
absent HUD objective line, neither of which this task's surface (`world_geometry`)
covers, and both are recorded here rather than hidden behind a green line.

## Unknowns and limitations (recorded, not guessed)

* **The children of these 47 records are not imported.** This flat
  `WorldDefinition` owns the records the world node names; a grouping node's
  children are one level below it and carry the meshes. Role `None` is what the
  *record* states — its own box is never tested and it draws nothing — and it is
  **not** a claim that the place it groups is passable. Importing those
  part/instance hierarchies needs parent-transform composition and a `lod`
  policy the container does not state (#1553's recorded limitation), and it is
  not this task's scope. **Affected content:** the collision and *presentation*
  of everything a `town_*`/`studebaker*`/`trucker*`/`bont*`/`tikicave`/… record
  groups inside M01's world. **Resolving task:** none filed here; it is the same
  design question #1553 recorded for every other model instance in every
  container, and the owner's immediate milestone (#1537's window) is served by
  the terrain, sea, actors and their own meshes.
* **What the 2000 renderer drew for a mesh-less grouping node** was not
  observed. This task answers what the container states. **Affected content:**
  the presentation of the 219 grouping records. **Resolving task:** #358 (an
  owner-supplied original run).
* **The rule keys on stored bytes.** An installation whose records were renamed
  or re-authored would need the counts re-run; the rule itself is keyed on the
  store's own `mesh_index`, `unk112` and box words, not on any name.
* **Which gameplay query consumes the walk** stays open under
  `f18-world.intersection-query-gameplay-consumer-unmeasured` (#771's claim;
  #358 lifts it). Nothing in this conversion depends on the answer.
* No image bytes, no disassembly listing and no decompiled code are committed;
  the strings quoted are `cls_zbd.c`/`cls_di.c`, the flag/selector words and a
  representative handful of stored display names.

## Test inventory

| test | covers | fails when |
| --- | --- | --- |
| `accept_m01_c3_world_residuals_a_grouping_grid_record_resolves_and_a_kept_one_does_not` (`crates/cs_app/tests/world/grouping_roles.rs`, synthetic, CI) | a grouping record resolving role `None` + the grouping shape claim + `OBJECT_STORES_NO_MESH`, the partition identity's new term, and a same-store-class record that stores `0x40` keeping `GRID_GROUPING_NODE_ROLE_UNMEASURED` with `objects_unresolved_collision == 1` and the verdict open; then the spawn presenting the answered one and reporting no `unknown_mesh` | the new arm is removed (the answered record inherits `Solid`+`FromMesh`, the grouping counter drops to 0, and the spawn skips it as `unknown_mesh`), the flags test becomes unconditional (the kept record resolves `None`), or a counter/skip list moves |
| `accept_m01_c3_world_residuals_c3_leaves_no_grouping_record_open` (retail, `#[ignore]`) | one production read of `c3`: the 47 counter, the four-term partition identity, the 47 records re-derived from the container's own bytes (`unk112 = 2`, box in 164, children > 0, bit `0x40` clear) each resolving `None` + the grouping claim + `OBJECT_STORES_NO_MESH`, `objects_unresolved_collision == 0`, the verdict `Satisfied`, and the spawn's colliders/skips/non-colliding balance with no `unknown_mesh` at all | any count moves, a record stays open or inherits the index's role, or one skip reappears |
| `accept_m01_c3_world_residuals_m01_s_plan_is_launchable` (retail, `#[ignore]`) | the CLI's own read: `plan_mission_launch(label M01, "The Lost Treasure")` resolves `zbd/c3` + `zbd/c3/m01`, `world_geometry` is `Satisfied`, the gap list is empty and `launchable()` is true | the container stops being launchable, or the binding resolves another group |
| `accept_m01_c3_world_residuals_c3_has_a_horizon_environment_subtree` (retail, `#[ignore]`) | `c3`'s `horizon` child and its 5 descendants, each resolving `None`, the count equal to `environment_records()` | the subtree stops being imported or a descendant resolves another role |

Re-pinned siblings, each keeping its property: `world_units.rs` (a
`grid_grouping` column per container, the four-term solid identity, the spawn's
`unknown_mesh` expectation — now the class's own count, zero — and the
non-colliding sum), `residual_roles.rs` (the four-term identity in both halves,
and the fixture's grouping counter pinned at 0), `grid_collision_origin.rs` (the
same identity, plus the grouping class re-derived from the bytes and each such
record asserted to resolve under the new claim while every *other* grid-named
record still resolves `Solid`), `fvol_roles.rs` (the four-term solid identity in
the synthetic and the census members) and `vs_m01_geometry_verdict.rs` (its
fixture now authors a grouping record, so the verdict's mesh-less term is
discriminated on the class this task added; the M06 retail member is unchanged
and stays).

## Commands run

```sh
cargo fmt --all -- --check                                            # exit 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings   # exit 0
cargo test --workspace --locked                                        # CI runs this
cargo test -p cs_app --locked -- accept_m01_c3_world_residuals_ --include-ignored
cargo test -p cs_app --locked --test cs_app_tests_c -- --include-ignored     # the world suites
cargo test -p cs_app --locked --test cs_app_tests_d -- --include-ignored     # the campaign suites
# evidence (docs/contracts/CLI-EVIDENCE.md):
cargo test --locked -p cs_app --test cs_app_tests_c -- \
  evidence_m01_c3_world_residuals --ignored
python3 tools/validate_evidence.py private/evidence/M01-C3-WORLD-RESIDUALS/acceptance.json \
  --artifact-root private/evidence/M01-C3-WORLD-RESIDUALS --require-pass
# the owner-command smoke (docs/PLAYTEST.md, "The owner-command smoke"):
cargo build --release --locked -p cs_app --bin cs
CS_CAPABILITIES=retail,gpu,audio target/release/cs --cs-path "$CS_GAME_DIR" --mission M01 \
  --smoke-seconds 45 --capture-dir private/evidence/M01-C3-WORLD-RESIDUALS/smoke
```

## Sources used

* `$CS_ENGINE_IMAGE` (sha256 above): `cls_zbd.c`'s node pass
  (`0x4e2a7c`–`0x4e2aa6`) and the inverse mapping (`0x4e19e6`–`0x4e1a1a`);
  `cls_di.c`'s candidate walk (`0x4cb5eb`–`0x4cb674`); `fcn.004cd960`.
* The production readers over the installation: `read_world_containers`,
  `RetailWorldContainer::{partition_grid, nodes, definition, uploaded_meshes}`,
  `cs_formats::gamez::read_gamez_nodes` — every count comes from them, and the
  new retail member re-derives the grouping predicate from the record bytes.
* `docs/findings/2026-10-08-m01-lc-world-residual-roles.md` (#771: the
  `[node+0x70]` rewrite, the walk's filters, the empty grid record),
  `docs/findings/2026-10-07-f18-grid-collision-origin.md` (#727: the candidate
  index), `docs/findings/2026-10-05-m01-lc-world-unit-roles.md` (#677: the
  store-state rule), `docs/findings/2026-10-17-m01-window-farimport.md` (#1553:
  the `horizon` subtree and the part-hierarchy limitation),
  `docs/findings/2026-10-02-gamez-node-array-layout.md` (the record's offsets).
* `crates/cs_content/src/world.rs` (`import_world_container`,
  `WorldImportReport`), `crates/cs_app/src/mission_launch.rs`
  (`geometry_verdict`, `plan_mission_launch`), `crates/cs_app/src/world/spawn.rs`
  — read; `spawn.rs` not modified.

## Evidence

`docs/findings/evidence/M01-C3-WORLD-RESIDUALS.json` — schema
`schemas/evidence.schema.json`, written by
`crates/cs_app/tests/evidence_report_m01_c3_world_residuals.rs` and validated
with `tools/validate_evidence.py … --require-pass`. Capabilities `retail`,
`synthetic` and `gpu`; the candidate tree recorded in the JSON is the tree of the
commit the acceptance run tested; the acceptance tests discovered, executed and
passed with the retail capability; claim `implemented`. Its second artifact is a
**second production observation** — one discovery over the installation, one
import per container, recording per group the grouping-class count, the
four-term partition identity's terms and the open-role count — and the smoke's
own `frame_*.png` captures are referenced as the `gpu` artifact. No original byte
is in either file: ids, digests, counts and claim labels only.
