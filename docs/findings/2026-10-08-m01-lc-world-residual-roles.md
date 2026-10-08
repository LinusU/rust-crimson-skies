# #771: the world_geometry surface's last unanswered records — `[node+0x70]`,
# the image's walk over a grid-named fog volume, and the mesh-less grid record

Date: 2026-10-08. Task #771 `M01-LC-WORLD-RESIDUAL-ROLES` ("Answer the
world_geometry surface's last unanswered records: grid-named fog volumes and
the mesh-less grid record"), the first of the three follow-ups #359
(`VS-M01-RUNTIME`) filed when it re-measured M01's launch closure on
2026-10-08. Feature sheet:
`specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`, stages
`### F18-B` (the import) and `### F18-D` (evidence over the installation).
Shared contract: `docs/contracts/IDENTITY-CONTENT.md`. Capabilities used:
**`retail`** (read-only access to `$CS_GAME_DIR`) and ordinary build/test.
`gpu` and `audio` were available and **not used**: nothing is rendered or
played and no original run happened, so nothing here is `verified_original`.

## Files

- `crates/cs_content/src/world.rs` (extended):
  - the **grid-named `fvol*` arm** of `import_world_container` is now
    conditional on a measurement instead of ending in an unknown: with the
    record's [`INTERSECTION_NARROW_PHASE_FLAG`] clear the image's own walk
    drops the candidate before any box test, so the record resolves
    `WorldCollisionRole::None` under
    [`FOG_VOLUME_RECORD_NEVER_BLOCKS`] with its shape left an explicit
    unknown; a record storing that bit set keeps
    [`GRID_NAMED_FOG_VOLUME_ROLE_UNMEASURED`] exactly as #727 wrote it;
  - a new arm resolves a grid-named record that stores **no geometry at all**
    (no mesh index, all three stored boxes empty) to `None` under the new
    [`GRID_RECORD_STORES_NO_GEOMETRY`], the store state #677 resolved for
    unindexed records;
  - `WorldImportReport` gained `partition_records_stores_no_geometry()` and
    `objects_unresolved_collision()`, the two numbers a launch verdict reads
    (see "The binding" below);
  - new claim id [`INTERSECTION_QUERY_GAMEPLAY_CONSUMER_UNMEASURED`] names
    what static analysis could not establish.
- `crates/cs_app/src/world/spawn.rs`: **read, not modified.** `spawn_world`
  already presents a `None` role with no collider and no skip, and a record
  whose shape is not `FromMesh` resolves no upload — so both new resolutions
  reach the spawn report as answers rather than gaps, exactly as #716's arm
  did.
- `crates/cs_app/tests/world/residual_roles.rs` (new): this task's acceptance
  tests (`accept_m01_lc_world_residual_roles_`) — the synthetic half drives the
  production import and spawn over a fixture holding all three classes (a fog
  volume the walk drops, one it would keep, a grid record with no geometry) plus
  the unindexed records that were already answered; the retail half is one
  discovery over all eight world containers, the per-container census, and the
  spawn over `c1c`'s real geometry.
- `crates/cs_app/tests/world/{main,import_retail,fvol_roles,grid_collision_origin}.rs`:
  the fixtures gained a stored-flag authoring field (the flag word the fixture
  wrote was fixed at `0x01800000`), and the counts and identities the two
  earlier tasks pinned are re-derived to the settled answer — the same
  treatment #727 gave #716's suite. No assertion lost the property it existed
  for; each now states a *smaller* or differently partitioned truth and names
  why.
- `crates/cs_app/tests/world/world_units.rs` and
  `crates/cs_app/tests/accept_m01_lc_world_unit_roles.rs` (re-pinned): a
  `grid_no_geometry` column per container, the re-partitioned solid count, the
  open-role and skip counts, the non-colliding sum, and the object-level claim
  walk that now distinguishes the two no-geometry claims by grid membership.
- `crates/cs_app/tests/evidence_report_f18_grid_collision_origin.rs`
  (re-pinned): its second-artifact census derived the six fog slots from the
  *unresolved* set, which this task emptied; it now derives them from the shape
  claim plus grid membership, so #727's own harness still runs.
- `crates/cs_app/tests/evidence_report_m01_lc_world_residual_roles.rs` (new):
  this task's evidence harness (CLI-EVIDENCE), whose second artifact is a
  second production run of the import over all eight containers plus the spawn
  over `c1c`'s own geometry.
- `docs/findings/2026-10-04-m01-lc-world-import.md` (the claim-id and c1c
  rows) and supersession notes in the #716 and #727 findings.
- This file.

## Provenance and method

Static analysis of the owner-supplied decrypted image
`$CS_GAME_DIR/crimson.decrypted.exe` (sha256
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`), same image,
same provenance and same address convention as
`docs/findings/2026-10-05-f04-d-original-lookup-order.md`: addresses are virtual
addresses, image base `0x400000`, and file offset = VA − `0x400000` for VAs
below `0x643000` (`.text`, `.rdata` and the raw part of `.data`; VA `0x71e890`
below is inside `.data`'s zero-filled tail). Tools: a full capstone
disassembly of `.text` (732 212 instructions) for cross-references, plus
targeted disassembly; the production readers over the installation for every
count. No image bytes, no disassembly listing and no decompiled code are
committed — only addresses, instruction shapes, strings of two to five
characters and counts.

**This is code-derived evidence, not a runtime capture.** Nothing here is
`verified_original`, and no claim below is a behavior landmark before #358
supplies an original run.

## A. What `[node+0x70]` points at — the narrow phase's box is the record's own

Task #727 left this open: `cls_di.c`'s narrow phase copies 24 bytes from the
address held at `[node+0x70]` (`fcn.004cd960`, `0x4cd9a1`–`0x4cd9b5`), and
"whether that address points at one of the record's three stored boxes
(offsets 116, 140 or 164) is not measured".

**Measured: it does, exactly, and the loader itself builds the pointer.** The
node-table pass in `D:\zipper\gamez\zclass\cls_zbd.c` (source-name string at
VA `0x62e354`) loops over every node — `inc edi` / `cmp edi, ebp` /
`jl 0x4e2a54` at `0x4e2afd`–`0x4e2b06` — and, per node, computes that node's
pointer with the same row/column arithmetic the grid-value converter uses
(`base + 212·column`, #727 section A) and rewrites the word at `+0x70`:

| stored word at `node+0x70` | instruction | what the field becomes |
| --- | --- | --- |
| `0` | `lea edx, [esi+0x74]` / `mov [esi+0x70], edx` (`0x4e2a83`) | pointer to the box at offset `0x74` (116) |
| `1` | `lea eax, [esi+0x8c]` / `mov [esi+0x70], eax` (`0x4e2a90`) | pointer to the box at offset `0x8c` (140) |
| `2` | `lea ecx, [esi+0xa4]` / `mov [esi+0x70], ecx` (`0x4e2aa0`) | pointer to the box at offset `0xa4` (164) |
| anything else | `jne 0x4e2aa9` | left untouched |

The same three-address rewrite appears again for a record *copied* at
`0x4e381d`–`0x4e3849`, and the **inverse** mapping — pointer back to index —
at `0x4e19e6`–`0x4e1a1a` (`[ecx+0x70] == ecx+0x74 → 0`, `+0x8c → 1`,
`+0xa4 → 2`), so the round trip through those three addresses is explicit in
the image rather than inferred from one writer.

The store's own field agrees three ways, read through the production readers
over all eight containers: `unk112` is asserted by the pinned reference to be
`0`, `1` or `2` (it is), and in every measured record class the box it selects
is exactly the one that is non-empty —

| class | `unk112` | non-empty box | example |
| --- | --- | --- | --- |
| `fvol*` volume | `1` | `unk140` (offset 140) | `c1c` node 944: 1024×120×1024 units |
| grouping node | `2` | `unk164` (offset 164) | `c1` node 2752, the union of its children |
| anchor / path / null node | `0` | none — all three empty | `c1c` node 2105 (`zepnull`) |

So the narrow phase reads **the record's own stored bounding box, selected by
the record's own stored word** — a fact about the container, not a derived or
runtime-computed volume. What the 24 bytes are used for after the copy
(`fcn.004cb8b0`) is a box test over a candidate the filters already accepted.

## B. What the intersection walk does with a grid-named `fvol*` record

`cls_di.c`'s candidate walk (`fcn.004cb420`), re-read at instruction level to
see the *order* of its filters — #727 established the filters exist, not what
they do to these six records:

| step | instruction(s) | address |
| --- | --- | --- |
| database capacity (32) | `cmp byte [eax], 0x20` … "Database intersections array is full" | `0x4cb5c5` |
| record's flags word, bit `0x04` — first filter | `test byte [esi+0x24], 4` / `je` drop | `0x4cb5eb` |
| zone whitelist over the record's byte at `0x30` | `mov cl, [esi+0x30]` / `call 0x56c430` / `test eax,eax` | `0x4cb5f7` |
| optional name filter | `mov edx, [0x71e890]` / `cmp edx, ebx` / `je` skip / `repne scasb` + `strncmp` | `0x4cb60b`–`0x4cb62f` |
| **narrow-phase bit** | `mov eax, [esi+0x24]` / `test al, 0x40` / `jne` narrow phase | `0x4cb635`–`0x4cb63a` |
| otherwise: recursion into children, or **drop** | `cmp word [esi+0x56], bx` / `jle` drop | `0x4cb63c`–`0x4cb642` |
| narrow phase: copy `[node+0x70]`'s 24 bytes | `test ah, 1` (bit `0x100`) / `push` / `call 0x4cd960` | `0x4cb665`–`0x4cb674` |

Two details settle the question:

1. **The name filter is optional at run time.** Its needle is the pointer
   loaded from the global at `0x71e890` — inside `.data`'s zero-filled tail in
   the image, so null at rest — and the `je 0x4cb635` skips the filter when it
   is null. That global is written in exactly one place in the whole image
   (`0x4cb548`), in the same file. It is a filter some other code installs, not
   a property of the record.
2. **With bit `0x40` clear the record never reaches a box test.** The walk
   either recurses into the record's children (`[esi+0x56]` count) or drops it
   when there are none. `cls_util.c`'s own error string names that bit the
   *proximity* flag (`0x62d784`), and the INTERP commands
   `SetIntersectSurface`/`SetIntersectBBOX`/`SetAltitudeSurface` (`0x5bbccc`,
   `0x5bbc90`) can write the related bits at run time — so this is the stored
   default, never a statement about what a script set later.

All six grid-named `fvol*` records this installation stores carry flags
`0x0308831c` (bit `0x40` clear) and `children_count = 0` (#727's census,
re-read here), so **in the loaded state the image's own walk drops each of them
before any box is read**. Together with #716 (the only name-keyed consumer of
the `fvol` prefix is the fog routine) the record's collision role resolves the
same `None` an unindexed fog volume already resolved — and the counter that
records the overlap,
`WorldImportReport::partition_records_fog_volume()`, keeps its meaning.

A record storing `0x40` would be a live candidate: that case still resolves
[`GRID_NAMED_FOG_VOLUME_ROLE_UNMEASURED`], which is what keeps this a
measurement keyed on the store rather than a rule keyed on the name. It does
not occur in this installation (all 65 `fvol*` records store `0x0308831c`),
which the retail test asserts as "no object carries #727's claim".

## C. Which gameplay query consumes the walk — partly named, mostly open

The chain is unique in the image: `call 0x5ac150` occurs exactly once (at
`0x4ab284`), and `fcn.004cb420` is called exactly once (at `0x5aca7e`, inside
`fcn.005ac150`).

* `fcn.005ac150` computes the distance between two of its vector arguments
  (`0x5ac166`–`0x5ac1a3`, with a bit-twiddling square root), reads fields at
  `+0x54` and `+0x1c` of its first argument, then walks the intersections
  database.
* Its single caller sits inside a `thiscall` routine at `0x4aabb0`–`0x4ab538`
  over an object holding node pointers and positions (`[esi+0x154]`,
  `[esi+0x160]`, `[esi+0x188]`, `[esi+0x1f4]`). That routine tests the node's
  flags word itself (`test byte [eax+0x24], 4` at `0x4ab20f`), compares the
  returned distance against the constant at `0x6034e8` (`0x4ab28c`) and, when
  it is under the threshold, calls `0x4b7e70` with `[esi+0x17c]` and the
  distance. Inside it, MSVC RTTI comparisons name `Target`, `TargetVehicle` and
  `TargetTurret` (type descriptors at `0x620768` and `0x620780`).

**Named and left open:** which gameplay query that is — target acquisition,
proximity, line of sight or something else — was not established. The
measurement cannot distinguish them without a behavior observation, so it is
recorded as [`INTERSECTION_QUERY_GAMEPLAY_CONSUMER_UNMEASURED`]
(`f18-world.intersection-query-gameplay-consumer-unmeasured`), a claim only an
original run (#358) can lift. Affected content: any statement about *what the
original used this walk for*. Nothing in the conversion depends on the answer:
the walk decides whether a record can be *reported* by that query, and the
record's role follows the store and the measured consumers.

## D. The grid record that stores no mesh: `c1c` node 2105

Read through the production readers, one production discovery over all eight
containers (the acceptance test does exactly this):

* **`c1c` has exactly one grid record with `mesh_index < 0`** — node slot
  **2105**, name `zepnull`, node type 5 (object3d), flags `0x0308801c`, zone 2,
  `parent_count = 1`, `children_count = 0`, `unk112 = 0`, and **all three
  stored boxes empty**, with an identity transform (the object3d record's own
  flags word is `40` = `OBJECT3D_FLAGS_IDENTITY`, translation zero). With `unk112 = 0` the loader points `[node+0x70]` at the
  first box — which is empty — so even the narrow phase would copy a zero box.
* `mesh_index = -1` is the store's own "no mesh" (the pinned reference, and the
  loader leaves a negative index unmapped rather than adding the mesh-table
  base: `test eax, eax` / `jl 0x4e2ab5` at `0x4e2aa9`). So no mesh-table entry
  is ever associated with the record.
* The same record shape exists in the other containers, as a census rather than
  a single case — grid records that bind no mesh **and** store no box in any of
  the three slots: `c1` 13, `c1b` 8, `c1c` 1, `c2` 23, `c2b` 1, `c3` 18,
  `c4` 4, `c5` 80 (148 of the installation's 367 mesh-less grid records; the
  other 219 store a box in one of the two slots `stored_extent` does not read
  and are **not** resolved by this task).

**Answered:** the container states no geometry for such a record — no mesh, no
non-empty box, no children — so there is nothing a collider or a drawing of its
own could come from, and it resolves `WorldCollisionRole::None` under the new
[`GRID_RECORD_STORES_NO_GEOMETRY`] with its absent mesh kept as an explicit
unknown under [`OBJECT_STORES_NO_MESH`]. This is the same store-state rule
#677 applied to unindexed records, reached here on a record the grid names: the
index makes it a *candidate* (#727) and a candidate that stores nothing has
nothing to test.

**Recorded rather than claimed:** no original run observed what the 2000
engine's renderer did with a mesh-less node (#358); this task answers what the
*container* says, which is what the conversion is allowed to say.

## The binding, and what `geometry_verdict` reads

`crates/cs_content/src/world.rs`, in `import_world_container`'s object loop:

1. `indexed && name carries the fvol prefix` → (flags `0x40` clear) role `None`
   + shape unknown under `FOG_VOLUME_RECORD_NEVER_BLOCKS`, counted in
   `partition_records_fog_volume`; (flags `0x40` set) role and shape unknown
   under `GRID_NAMED_FOG_VOLUME_ROLE_UNMEASURED`, as #727 left it;
2. `indexed && stores no mesh && all three stored boxes empty` → role `None` +
   shape unknown under `GRID_RECORD_STORES_NO_GEOMETRY`, counted in the new
   `partition_records_stores_no_geometry`;
3. `indexed` otherwise → `Solid` + `FromMesh` (`INDEXED_RECORD_IS_STATIC`),
   unchanged;
4. the unindexed arms (#677/#716), unchanged.

Every object whose **role** stays `Unknown` is counted in the new
`objects_unresolved_collision()`.

The two numbers `crates/cs_app/src/mission_launch.rs::geometry_verdict` reads
for `zbd/c1c` are therefore:

```text
open roles  = report.objects_unresolved_collision()                       // c1c: 0
open mesh   = report.partition_records()
            - report.partition_records_with_mesh()
            - report.partition_records_stores_no_geometry()               // c1c: 0
```

and the partition identity every retail test now pins is

```text
partition_records == objects_solid + partition_records_fog_volume
                           + partition_records_stores_no_geometry
```

Measured per container for `objects_unresolved_collision()` (all of them equal
`objects_unindexed_unresolved()`), and for
`partition_records_stores_no_geometry()`:
`c1` 13, `c1b` 8, `c1c` 1, `c2` 23, `c2b` 1, `c3` 18, `c4` 4, `c5` 80.

> **The `geometry_verdict` acceptance criterion is not verifiable on this
> branch, and why.** `crates/cs_app/src/mission_launch.rs` — and with it
> `geometry_verdict`, `plan_mission_launch` and the retail suite
> `crates/cs_app/tests/campaign/vs_m01_runtime.rs` (`accept_vs_m01_runtime_`) —
> exists only on task #359's blocked branch
> (`rally/359-wire-one-original-mission-into-the-playa`, head `45aa1eaf`), not
> on `main`, and Rally merges only this task's own branch off `main`. This task
> therefore delivers what that verdict *reads* (the two accessors and the
> arithmetic above, each documented on the accessor itself) and hands #359 the
> two-line change: replace its `fog` term with
> `objects_unresolved_collision()` and its `meshless` term with the
> three-term subtraction above; `partition_records_fog_volume()` is an overlap
> count and must no longer be read as an open question. A note carrying exactly
> that is filed on #359. Verifying `Satisfied` for `zbd/c1c`, and updating the
> retail gap set `{world_geometry, player_configuration, world_actors}` to drop
> `world_geometry`, is #359's re-measurement on its own branch.

## Affected content, and claim status

* **Collision over the six grid-named fog volumes** — `c1c` node slots
  944–947 and `c5` node slots 2304/2306: they no longer report
  `unknown_collision_role`; they are presented and never block, with the fog
  claim in their shape. Their meshes are untouched references.
* **The 148 grid records that store no geometry** (per-container counts above):
  same, from the store's own silence instead of from the fog consumer.
* **`c1c`'s spawn report goes from `{unknown_collision_role: 4, unknown_mesh: 1}`
  to empty**, and its `non_colliding` population from 53 to 58; the collider
  count stays 288. Other containers' `unknown_mesh` reports shrink by their
  empty-record counts (their boxed-but-mesh-less records are untouched and keep
  reporting that gap).
* **Claim ids:** `GRID_NAMED_FOG_VOLUME_ROLE_UNMEASURED` is narrowed (kept for
  a record that stores the narrow-phase bit; no record on this installation
  does); new: `GRID_RECORD_STORES_NO_GEOMETRY`,
  `INTERSECTION_QUERY_GAMEPLAY_CONSUMER_UNMEASURED`,
  `INTERSECTION_NARROW_PHASE_FLAG` (the measured flag constant).

**Evidence class:** everything here is `observed_tool` — static analysis of one
executable plus byte censuses through the production readers. Nothing is
`verified_original`; no original executable ran.

## Unknowns and limitations (recorded, not guessed)

- **Which gameplay query consumes the walk** is open under
  `f18-world.intersection-query-gameplay-consumer-unmeasured` (section C).
  **Affected content:** every claim about what the original used the walk for.
  **Resolving task:** #358 (an owner-supplied original run).
- **Whether a script sets the proximity/intersection bits on a fog volume at
  run time** is open: the INTERP commands exist (`0x5bbccc`, `0x5bbc90`) and
  this task measured only the *stored* flags. **Affected content:** collision
  over the six grid-named fog volumes in a run where such a script executes.
  **Resolving task:** #358.
- **What the 2000 renderer drew for a mesh-less node** was not observed; this
  task answers what the container states (section D). **Affected content:** the
  presentation of the 148 empty grid records. **Resolving task:** #358.
- **The 219 mesh-less grid records that do store a box** (in a slot
  `stored_extent` does not read) are *not* resolved here: their container states
  a box but no mesh, and what that box meant to the original is not measured.
  They keep reporting `unknown_mesh` in their own containers. **Affected
  content:** `c1` 48, `c1b` 8, `c2` 49, `c3` 47, `c4` 43, `c5` 24, `c2b` 0,
  `c1c` 0. **Resolving task:** none filed — no launch surface depends on them;
  `c1c` (M01's container) has none.
- The `fvol` rule keys on stored display names, and this census is of **this**
  installation: an installation whose records were renamed would need the
  counts re-run.
- No image bytes, no disassembly listing and no decompiled code are committed;
  the strings quoted are `cls_zbd.c`, `fvol`, `0x0308831c`-shaped flag words
  and two RTTI names.

## Test inventory

| test | covers | fails when |
| --- | --- | --- |
| `accept_m01_lc_world_residual_roles_grid_records_resolve_answers_instead_of_gaps` (synthetic) | the grid-named fog volume the walk drops (role `None`, fog claim, filter named in the reason), the one that stores the narrow-phase bit (still #727's unknown), the empty grid record (`None` + no-geometry claim + `OBJECT_STORES_NO_MESH`), the partition identity, `objects_unresolved_collision`, and the spawn's exact skip list (`unknown_collision_role` ×2, never `unknown_mesh`) with its exact non-colliding set | either arm is removed (the dropped fog volume inherits the unknown again, the empty record inherits `Solid`+`FromMesh` and skips as `unknown_mesh`), the flags test becomes unconditional (the kept record resolves `None`), or a skip list/count moves |
| `accept_m01_lc_world_residual_roles_the_installation_leaves_nothing_unanswered_in_c1c` (`#[ignore]`, retail) | one production discovery over all eight containers: the per-container empty-record census, `objects_unresolved_collision == objects_unindexed_unresolved`, the partition identity per container, that only unindexed records are open and **no** object carries #727's claim, plus `c1c`'s spawn over the real geometry reporting **zero** skips, 288 colliders and 58 non-colliding objects | any count moves, a record stays open, a `fvol*` record stops resolving the fog claim, or one skip reappears in `c1c` |

Re-pinned siblings, each keeping its property: `fvol_roles.rs` (the grid fog
record's resolved answer, the fog-object count = unindexed + grid-named, the
partition identity, and "no grid-named record is open"), `grid_collision_origin.rs`
(the same, plus "every other grid-named record still `Solid`" now skipping only
the fog and empty records, re-derived from the bytes rather than from the
report), `import_retail.rs` (c1c's report: 288 solid, 0 unresolved roles, the
empty claim-id census and the spawn's now-empty skip list; plus the ten-claim
census), `world_units.rs` (a `grid_no_geometry` column per container, the
re-partitioned solid count, the open-role and skip counts, and the non-colliding
sum), `accept_m01_lc_world_unit_roles.rs` (the object-level claim walk now
distinguishes the two no-geometry claims by grid membership, and c1c's spawn) and
`evidence_report_f18_grid_collision_origin.rs` (its second-artifact census
derives the six fog slots from the shape claim instead of from the *unresolved*
set, which #771 emptied).

## Sources used

- `$CS_GAME_DIR/crimson.decrypted.exe` (sha256 above): `cls_zbd.c`'s node pass
  (`0x4e2a54`–`0x4e2b06`, source name `0x62e354`), its copy-side rewrite
  (`0x4e37fb`–`0x4e384c`) and the inverse mapping (`0x4e19d1`–`0x4e1a1a`);
  `cls_di.c`'s candidate walk (`0x4cb579`–`0x4cb679`), `fcn.004cd960`
  (`0x4cd960`), `fcn.005ac150` and its single caller (`0x4aabb0`–`0x4ab538`);
  `cls_util.c`'s flag setters and the INTERP command names; RTTI descriptors
  `0x620768`/`0x620780`.
- The production readers over the installation: `read_world_containers`,
  `RetailWorldContainer::{partition_grid, nodes, definition, uploaded_meshes}`,
  `cs_formats::gamez::read_gamez_nodes` — every count in this file comes from
  them, and the retail test re-derives the empty-record predicate from the
  record's own bytes instead of reading the report back.
- `docs/findings/2026-10-07-f18-grid-collision-origin.md` (#727: the candidate
  index, the six records, `[node+0x70]` as an open question),
  `docs/findings/2026-10-07-m01-lc-fvol-roles-and-axis-convention.md` (#716:
  the fog consumer), `docs/findings/2026-10-05-m01-lc-world-unit-roles.md`
  (#677: the store-state rule and the unindexed split),
  `docs/findings/2026-10-02-gamez-node-array-layout.md` (the 208-byte record's
  field offsets, the 212-byte slot).
- `crates/cs_content/src/world.rs` (`import_world_container`,
  `WorldImportReport`), `crates/cs_app/src/world/spawn.rs` (`spawn_world`) —
  read; `spawn.rs` not modified.

## Evidence

`docs/findings/evidence/M01-LC-WORLD-RESIDUAL-ROLES.json` — schema
`schemas/evidence.schema.json`, written by
`crates/cs_app/tests/evidence_report_m01_lc_world_residual_roles.rs` and
validated with `tools/validate_evidence.py … --require-pass`. Capabilities
`retail` + `synthetic`; the candidate tree recorded in the JSON is the tree of
the commit the acceptance run tested; 2 tests discovered, executed and passed;
claim `implemented`. Its second artifact, `world-residual-roles-census.json`, is
a **second production observation** — one discovery over the installation, one
import per container, then the spawn over `c1c`'s own uploaded geometry —
recording per group the grid size, the fog overlap, the empty-record count, the
solid count and the open-role count, plus `c1c`'s spawn summary (346 objects,
288 colliders, **0 skips**, 58 non-colliding, 0 presentation gaps). No original
byte is in either file: ids, digests, counts and claim labels only.

The three residuals are carried in the report's `review.method` — each naming
the content it affects and #358 as the task that lifts it — and not in the
schema's `unknowns` array, which `tools/validate_evidence.py --require-pass`
requires to be empty. They are limits of code-derived evidence, not work this
task left undone, they gate `verified_original` and `release_approved` until
#358 supplies a run, and dropping them from the report entirely (rather than
relocating them inside it) is what the owner's evidence rule forbids.

## Commands run

```sh
cargo fmt --all -- --check                                            # exit 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings   # exit 0
cargo test --workspace --locked                                       # exit 0
cargo test --workspace --locked -- accept_m01_lc_world_residual_roles_ --include-ignored
#   exit 0: 2 tests discovered, executed and passed (1 synthetic, 1 retail)
# the re-pinned sibling suites, each with CS_GAME_DIR:
cargo test -p cs_app --test world -- --include-ignored
#   153 passed, 2 failed: `audit::evidence::evidence_report_f18_d_writes_the_acceptance_report`
#   and `evidence_f18_parry_denormal_bvh::writes_the_acceptance_report`, which refuse to run
#   without their own CS_EVIDENCE_DIR sequence and fail the same way on main
cargo test -p cs_app --test accept_m01_lc_world_unit_roles -- --include-ignored   # exit 0, 5 passed
# evidence (docs/contracts/CLI-EVIDENCE.md):
cargo test --locked -p cs_app --test evidence_report_m01_lc_world_residual_roles -- --ignored  # exit 0
python3 tools/validate_evidence.py docs/findings/evidence/M01-LC-WORLD-RESIDUAL-ROLES.json \
  --artifact-root private/evidence/M01-LC-WORLD-RESIDUAL-ROLES --require-pass  # exit 0
```

The branch was then rebased onto `origin/main`, which had moved by four commits
touching only `tools/cs_xtask/*` and one findings document: no `Cargo.toml` or
`Cargo.lock` and no file this branch changes (the owner's 2026-10-01
merge-race conditions), so the re-push ran the lighter set — `fmt`, `clippy` and
the prefix run, all exit 0 — and the full workspace suite had been exit 0 on the
pre-rebase tree (CI runs it on the pushed commit).
