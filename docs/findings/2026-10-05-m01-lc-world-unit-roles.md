# #677: the GameZ stored unit is the metre, and the unindexed world records are two measured classes

Date: 2026-10-05. Task: #677 "Measure the world-vertex unit and the collision
roles of the 53 unindexed c1c records" (`M01-LC-WORLD-UNIT-ROLES`), the
`world_geometry` measurement `VS-M01-RUNTIME` (#359) was blocked on. Feature
sheet: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
stages `### F18-B` and `### F18-D`; the unit half also serves #436's blocked
measurement. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: **`retail`** (read-only access to `$CS_GAME_DIR`) and
ordinary build/test. `gpu` and `audio` were available and **not used**: nothing
is rendered or played and no original run happened, so nothing here is
`verified_original`.

## Files

- `crates/cs_content/src/coordinates.rs` (extended): `CoordinateSource`
  carries a landmark record now (`record_landmark`, and `calibration()`
  replays it — the same `UnitCalibration` rule a free-standing record
  applies, so `retail_gamez`'s gaps list is a real F16-D record and not an
  annotation). `CoordinateSource::retail_gamez` is the first measured source:
  the GameZ convention with **one quantity measured** — the scale, pinned to
  the metre at `observed_tool` by the landmark census below — and the rest
  honestly declared. `UnitCalibration::quantity_status` is the per-quantity
  claim accessor the import reports through; `claim_status` stays the
  whole-convention answer, so a source whose axis map was never observed can
  still have its measured scale reported without the report pretending the
  convention is calibrated. `GAMEZ_VERTEX_UNIT_IS_THE_METRE` is the claim id.
- `crates/cs_content/src/world.rs` (extended): the unindexed branch of
  `import_world_container` is now the measured **three-way split** — indexed ⇒
  `Solid` + `FromMesh` (unchanged, `INDEXED_RECORD_IS_STATIC`); unindexed with
  no mesh index and an all-zero `unk140` box ⇒ `WorldCollisionRole::None`
  under the new `UNINDEXED_RECORD_STORES_NO_GEOMETRY`; unindexed with a mesh
  or an extent ⇒ the explicit unknown under `UNINDEXED_ROLE_UNMEASURED`,
  unchanged. `WorldImportReport` counts the two new classes
  (`objects_unindexed_none`, `objects_unindexed_unresolved`) and reports the
  **scale quantity's** evidence class as `unit_class`, so an import under the
  measured source reports `observed_tool` rather than `unknown`.
- `crates/cs_app/src/world/retail.rs` (doc only): the module and
  `definition` docs no longer say the unit is unmeasured — they name
  `retail_gamez` as the measured choice and say which quantities are still
  open.
- `crates/cs_app/src/playtest_retail.rs` (doc only): `playtest_adapter`
  stays a **designed** identity map — its scale happens to agree with the
  measured metre; the doc now says so instead of claiming the unit is
  unmeasured.
- `crates/cs_app/tests/world/import_retail.rs` (extended): the fixture grew a
  second unindexed record — a mesh-bearing, non-zero-extent `volume` beside
  the `marker` anchor — so all three arms of the new rule are exercised and a
  one-size-fits-all implementation fails. The retail assertions now pin the
  c1c census: 36 `None` anchors, 17 unresolved `fvol` volumes, spawn report
  `unknown_collision_role: 17` + `unknown_mesh: 1` + 36 non-colliding.
- `crates/cs_app/tests/world/world_units.rs` (extended): the measured table
  carries the per-group split; the imports run under `retail_gamez`; the spawn
  assertions count the deliberate non-colliding population separately from the
  skips.
- `crates/cs_app/tests/accept_m01_lc_world_unit_roles.rs` (new): the task's
  acceptance tests — the measured source's own contract (scale pinned, other
  quantities gapped, `observed_tool` not `verified_original`), the declared
  source still reporting `unknown`, the all-eight census, the c1c spawn
  split, and (added in review) every scale landmark re-observed over the
  installation through the production readers.
- `crates/cs_app/tests/evidence_report_m01_lc_world_unit_roles.rs` (new): the
  evidence harness, writing `world-unit-roles-census.json` (a second
  production run over all eight containers) beside `acceptance.json`.
- `docs/findings/evidence/M01-LC-WORLD-UNIT-ROLES.json`: the committed report.
- This file.

## What this task found

### One: the stored unit is the metre, at `observed_tool`

Five independent landmarks, recorded on `CoordinateSource::retail_gamez`'s own
calibration (F16-D's three-landmark rule, with behaviours among them), all from
tool runs over the owner's installation:

| landmark | what was measured | reads at 1 unit = 1 m | reads at 1 unit = 1 ft |
| --- | --- | --- | --- |
| animation `GRAVITY` word | all **61** anim containers store `0xC11CCCCD` = **-9.8** | Earth's g in m/s² | meaningless |
| pilot figure (`cpilot`, `pickup_cpilot`) | composed subtree extent ≈ **0.7 × 1.9 × 0.5** units | a standing human | a 58 cm figure |
| airframe subtrees | 11 roster roots span **8.8–27.2** composed units (`bloodhawk` 11.6 × 3.6 × 11.3; `warhawk` 21.9 × 6.0 × 16.4) | fighter-class aircraft | the largest "fighter" is 8.3 m |
| aircraft LOD switch ranges | stored bands run **50–3000** units | tens of metres to ~3 km view distances | implausible draw distances |
| world bounds + partition grid | published sector bounds run **-16384..256** units; grid cells **1024** units | a 12–16 km archipelago with kilometre cells | a 3.7–5 km map with ~300 m cells |

The first two are the pin: a stored `-9.8` in a field the format family calls
`GRAVITY` is an SI number in the bytes, and a human-scale mesh measuring
human-scale is a measured distance against a known size. The rest corroborate.

The pilot figure and the airframes are **composed** extents: each member's
stored `unk140` box corners carried through the member's composed `SceneGraph`
transform. A subtree's stored boxes are stated in each record's own frame, so
the union of the raw boxes is a different (and for `cpilot`, much smaller)
number than the figure below — see the review corrections at the end.
The class is `observed_tool` and stays there: every record is a tool probe over
file bytes, none carries an original-run fingerprint, and `claim_status` never
reaches `verified_original` — a byte census is not a run.

**Only the scale is measured.** The GameZ container family stores no axis map,
handedness or angle-unit declaration anyone has tied to an original behaviour,
so `retail_gamez` declares the same identity convention the world import
already applied and reports `Handedness`, `AxisOrder` and `AngleUnit` as gaps.
`UnitCalibration::claim_status` stays `Unknown`; `quantity_status(Scale)` is
`ObservedTool`. The world import's `unit_class` reports the scale's own class —
the convention and the factor are different claims and stay different.

### Two: the unindexed records are exactly two classes, in all eight containers

Census of every record the world node's stored child list owns and the
partition grid does **not** name, taken through the production readers:

| container | unindexed | no geometry (⇒ `None`) | geometry-bearing (⇒ unknown) | odd |
| --- | ---: | ---: | ---: | ---: |
| c1 | 66 | 56 | 10 | 0 |
| c1b | 78 | 78 | 0 | 0 |
| c1c | 53 | 36 | 17 | 0 |
| c2 | 24 | 24 | 0 | 0 |
| c2b | 48 | 39 | 9 | 0 |
| c3 | 14 | 14 | 0 | 0 |
| c4 | 51 | 38 | 13 | 0 |
| c5 | 105 | 20 | 85 | 0 |

The split is **exact**: every unindexed record stores either a mesh index and a
non-zero bounding box, or neither. There is no record in the corpus carrying
only one of the two, so `mesh_index < 0 && stored_extent == None` is a measured
partition, not a heuristic.

**The no-geometry half** is the `horizon`, the `g27816` transform groups, the
`*zep` zeppelin anchors and the vehicle/scatter dummies — transform carriers
whose own record bounds nothing. The store gives such a record nothing a
collider could be built from: `FromMesh` has no mesh to derive from and a
`Cuboid` has no extent to fill. Its role resolves to `None` — *presented,
never blocking* — under `UNINDEXED_RECORD_STORES_NO_GEOMETRY`, a designed
resolution over a measured fact. `spawn_world` already honours `None` as a
deliberate non-collider, so these objects stopped reporting
`unknown_collision_role` without touching the spawn path.

**The geometry-bearing half** is the `fvol*` volumes (c1c's 17, named
`fvol5..fvol23` with gaps in the numbering, all flag `0x0308011c`-class) and
c5's 85 mesh carriers. The container binds geometry to them and the partition
grid does not index them, but **nothing in the container says whether the
original engine collided with them** — they keep `UNINDEXED_ROLE_UNMEASURED`
on both role and shape, and the spawn reports each as
`unknown_collision_role` rather than handing them a substitute.

**A prior hint was measured and rejected.** The record flags word looked like
a discriminator — every indexed record carries bit `0x8000`, no unindexed one
does — until c2/c3 were read: their *indexed* `0x0308051c` records are
themselves mesh-less group anchors (`studebaker*`, `police*`, `security*`), so
the flag does not mark geometry and the split is defined on the two stored
geometry fields alone.

**Does M01's data consume them?** The mission's own reader binds the zeppelin
anchors heavily (all five `*zep` records are animation-driven movers — the
animation-binding census confirms it), and names **no** `fvol*`, `g27816`,
`horizon` or `dzpath` record: the mission layer does not disambiguate the
volumes' role either, which is why they stay unknown rather than becoming
sensors or solids by inference.

## Unknowns and limitations (recorded, not guessed)

- **Whether the `fvol*` volumes collided, sensed or decorated is UNMEASURED.**
  The container stores their mesh and extent and nothing about their role;
  the mission data does not name them. They are 17 records of c1c (134 across
  the corpus) that stay `Resolved::Unknown` — **affected content:** collision
  and trigger behaviour over those volumes. **Resolving task:** a follow-up
  measurement of the volume-class records' runtime consumption, if the
  mission semantics stage (#675/#676/#678 family) cannot already answer it.
  *(Resolved by #716 — `docs/findings/2026-10-07-m01-lc-fvol-roles-and-axis-convention.md`:
  the image's only name-keyed consumer of the `fvol` prefix is its fog
  routine, so the 59 world-owned `fvol*` records resolve to `None` and the
  75 records no measured prefix names keep this claim.)*
- **The axis map, handedness, angle unit, rotation sense and front-face rule
  are DECLARED, not measured.** `retail_gamez` carries the identity
  convention because that is what the import already applied; the calibration
  gap list names every unmeasured quantity. **Affected content:** any claim
  that a stored transform's *orientation* is the original's rather than the
  declared convention's. **Resolving task:** #436's wider convention
  measurement. *(Partly resolved by #716: the axis map, orientation
  preservation, angle unit and rotation sense are now measured by static
  analysis of the decrypted image (#436's owner note) and reported per import
  by `WorldImportReport::axis_class`; this source's own calibration still
  carries no landmark for them, which is #390/F16-E's work.)*
- **`observed_tool` is the ceiling here.** No original executable ran; the
  metre claim is a byte census. It could still be wrong if the format's
  `GRAVITY` word is not the Earth's, or if the aircraft were authored at a
  non-1:1 scale — both limitations are recorded on the landmarks' own
  `EvidenceRecord`s.
- **`None` is about the record's own store only.** The airship a `*zep`
  anchor parents is another record with its own geometry and role; resolving
  the anchor does not resolve the child. The actor/animation surfaces
  (#632-family) measure those.
- **Spawn semantics for `None` were already designed behaviour**, not a
  measurement of the 2000 engine: `spawn_world` presents the object and
  builds no collider. Nothing here claims the original engine skipped or
  collided the anchors.
- **Nothing derived from the original bytes is committed.** The numbers above
  are counts, dimensions and relations; no name list beyond the class
  prefixes, no mesh and no screenshot is in the repository.

## Sources used

- `crates/cs_formats` GameZ node-array reader (`GameZNodes`, `RawNode`,
  `mesh_index`, `info.unk140`), the ZBD-anim payload reader (the `gravity`
  field census over all 61 anim containers) and the ZRD member reader (the
  mission-data name scan), all through production code.
- `crates/cs_content/src/coordinates.rs` (`UnitCalibration`, `Landmark`,
  `EvidenceRecord`/`ObservationLocator` in `cs_types`) and
  `crates/cs_content/src/world.rs` (`import_world_container`,
  `WorldImportReport`, `WorldCollisionRole`, `stored_extent`,
  `WorldPartitionGrid`).
- `crates/cs_app/src/world/spawn.rs` (`spawn_world`, `SpawnedWorld`,
  `SkipReason`, `non_colliding`) — read, **not** modified; it already honoured
  `WorldCollisionRole::None` as designed.
- `docs/findings/2026-10-04-m01-lc-world-import.md` (#629: the c1c import and
  the "53 unindexed" blocker this task resolves),
  `docs/findings/2026-10-05-f18-world-units-containers.md` (#639: the
  all-eight census this task extends), and the #632-family anim-records
  finding for the animation-binding census.

## Commands run

```sh
cargo fmt --all -- --check                                       # clean
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings   # clean
cargo test --workspace --locked                                  # exit 0
cargo test --workspace --locked -- accept_m01_lc_world_unit_roles --include-ignored
#   5 tests discovered under this prefix, all passing: the 2 calibration-contract
#   tests run everywhere; the 3 retail tests run locally under CS_GAME_DIR.
```

(The reconnaissance harness used for the census was removed before commit; the
acceptance and evidence tests re-run the same production readers.)

## Review corrections (bunny-alpha-2, 2026-10-06)

The review re-measured every landmark in the table above through the production
readers rather than accepting the prose, and corrected one wrong number plus one
gap the prose left open.

- **The world-bounds landmark claimed "grid cells ~300 units" and "300-m dogfight
  cells".** That is wrong. The partition grids step in **1024** stored units —
  1329 of the 1364 sectors the import publishes are exactly 1024 units across,
  and the pitch between neighbouring cells' own stored header floats is 1024 in
  every container. At the metre the cells are kilometre cells; at the foot they
  are ~300 m. The landmark and the table now say that, and the reading was
  correct in substance (a kilometre cell only makes sense as a distance) while
  wrong in its number. Corrected in `coordinates.rs` and in the table above.
- **Nothing re-observed the landmarks.** The five landmarks were prose in a
  source file, and the finding said the census harness was deleted, so no test
  could tell a measured number from a plausible one. The review added
  `accept_m01_lc_world_unit_roles_every_scale_landmark_is_re_observed_over_the_installation`,
  which re-measures all five through the production readers — the ZBD-anim
  payload reader for the 61 gravity words, the GameZ node reader plus
  `SceneGraph` for the pilot figure, the eleven roster airframes and the LOD
  bands, and the partition grid plus the imported sectors for the theatre and
  the cell pitch — and asserts that no landmark still claims a cell size the
  grid does not state. It failed on the `300-unit` claim before the correction.
- **The LOD band measurement needed a stated rule.** One LOD record stores
  `10 000 000` as its far bound; that is an unbounded band, not a
  ten-thousand-kilometre view distance, and the test reads the finite bounds and
  says so.
- **The pilot figure and the airframes are composed extents, not stored boxes.**
  A subtree's stored `unk140` boxes are stated in each record's own frame, so
  the census measured each member's box corners through its composed
  `SceneGraph` transform. The raw stored-box union of the same `cpilot` subtree
  is 0.43 × 0.77 × 0.43, which is **not** the figure the landmark reports and
  would not read as a human at either scale. The landmark's number is the
  composed one and is the right measurement; the finding now says which is
  which, so the two cannot be confused later.

Reviewed and re-measured independently with the `retail` capability. Nothing
here raises the claim's class: the metre stays `observed_tool`, no landmark
reaches `verified_original`, and the axis map, handedness and angle unit remain
declared rather than measured.
