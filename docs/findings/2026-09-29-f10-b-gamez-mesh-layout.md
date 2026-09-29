# F10-B: the CS GameZ mesh/polygon layout and its reader

Date: 2026-09-29. Task: Rally **#363** `F10-B-gamez-layout`, "Establish and
parse the CS GameZ mesh/polygon binary layout into `RawMesh`" (a follow-up to
**#42** `F10-B`, which delivered validated n-gon triangulation but parsed no
bytes). Sheet: `specs/F10-gamez-mesh-topology-and-material-records.md`
(non-negotiables 1–5, AC04). Contract: `docs/contracts/IDENTITY-CONTENT.md`.
Read first: `docs/findings/2026-09-28-f10-a-lossless-mesh-ir-and-strip-fixtures.md`
and `docs/findings/2026-09-29-f10-b-validated-ngon-triangulation.md`.
Capabilities used: **`retail`** (read-only, `$CS_GAME_DIR`) plus ordinary
build/test. Test prefix: `accept_f10_b_gamez_`. Evidence:
`docs/findings/evidence/F10-B-gamez-layout.json`.

## Sources

- **Pinned reference** — mech3ax **v0.6.0**, commit
  `d3521a9721be731d365504568ddcd78e3f9846bb` ([S02], [S06], [S17] in
  `docs/research/SOURCES.md`; the commit S17 resolved for tag v0.6.0). Cloned
  into a private research directory outside this repository and read only: the
  licence is **EUPL-1.2** and **no code was copied**. Files read:
  - `crates/mech3ax-gamez/src/gamez/common.rs` — `SIGNATURE`, `VERSION_CS`,
    `MeshesInfoC`, `MeshIndexIter`, `read_meshes_info_nonseq`;
  - `crates/mech3ax-gamez/src/gamez/cs/mod.rs` — `HeaderCsC`, `read_gamez`, the
    section-offset assertions;
  - `crates/mech3ax-gamez/src/gamez/cs/meshes.rs` — `read_meshes`, the
    non-sequential mesh index and the sequential data walk;
  - `crates/mech3ax-gamez/src/gamez/cs/fixup.rs` — `Fixup`, the two measured
    remap tables, and the `HeaderCsC` comment block with every measured CS
    header;
  - `crates/mech3ax-gamez/src/gamez/cs/nodes.rs` — the node array, which is *not*
    read here but fixes the mesh index a node refers to;
  - `crates/mech3ax-gamez/src/mesh/ng.rs` — `MeshNgC`, `PolygonNgC`,
    `PolygonBitFlags`, `read_polygons`, `read_mesh_material_infos`,
    `read_mesh_data`;
  - `crates/mech3ax-gamez/src/mesh/common.rs` — `LightC`, `read_lights`,
    `read_vec3s`, `read_uvs`, `read_colors`, `read_u32s`;
  - `crates/mech3ax-api-types/src/gamez/mesh/ng.rs` — `MeshMaterialInfo`,
    `PolygonMaterialNg`, `PolygonNg`, `MeshNg`;
  - `crates/mech3ax-nodes/src/cs/node.rs` — `NodeCsC`, whose `mesh_index` at
    offset 60 is the node-to-mesh association.
- **Original installation** — the read-only installation at `$CS_GAME_DIR`.
  All **nine** GameZ archives were read. Fingerprints (SHA-256 of the whole
  file) are in the per-archive table below and in the evidence artifact
  `gamez-corpus.json` (private, referenced by digest).

## What was established, and how

Two independent steps, in this order, with no feedback from one into the other:

1. **Read the layout from the pinned source** and write it down as a worksheet
   (§ "Field worksheet").
2. **Check it against the installation** by walking all nine archives with a
   throwaway probe written from the worksheet alone, and comparing the
   reference's *own* recorded numbers — the `HeaderCsC` comment block in
   `fixup.rs` — against the bytes.

The result of step 2 is stronger than "it parsed": for every archive the mesh
data walk **ends exactly on the `nodes_offset` the reference recorded**, the
present-record count equals the index's declared `count`, every absent record's
stored index equals the reference's expected index under the archive's fixup
table, and the stored `last_index` equals the remapped value. The walk landing
on an independently recorded offset is the discriminating check: a reader with
any record length wrong would end somewhere else.

| Archive | Size | SHA-256 | `unk08` | `array_size` | present | `last_index` | polygons | decoded | invalid | unsupported |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `ZBD/planes.zbd` | 6 083 868 | `45da54a8…8fbc21b` | 967 277 477 (`Planes`) | 2 250 | 1 766 | 1 313 | 16 200 | 16 200 | 0 | 0 |
| `ZBD/C1/gamez.zbd` | 6 875 076 | `2a61482d…ae31a8` | 967 277 730 | 2 250 | 2 237 | 2 237 | 18 277 | 18 277 | 0 | 0 |
| `ZBD/C1B/gamez.zbd` | 3 953 304 | `38ea1c08…28f099` | 967 278 018 | 1 500 | 1 305 | 1 305 | 8 323 | 8 323 | 0 | 0 |
| `ZBD/C1C/gamez.zbd` | 4 008 612 | `6ad39cd4…5a7794` | 967 278 208 | 2 250 | 1 518 | 1 518 | 8 040 | 8 040 | 0 | 0 |
| `ZBD/C2/gamez.zbd` | 4 908 288 | `2b2cf09b…4c924b6` | 967 278 462 | 2 250 | 1 765 | 1 765 | 12 645 | 12 645 | 0 | 0 |
| `ZBD/C2B/gamez.zbd` | 3 435 348 | `8b64f30b…f2ab` | 967 278 721 | 1 500 | 1 365 | 1 365 | 7 008 | 7 008 | 0 | 0 |
| `ZBD/C3/gamez.zbd` | 5 633 696 | `30318f7b…fa81d73` | 967 278 943 | 2 250 | 1 901 | 1 901 | 16 087 | 16 087 | 0 | 0 |
| `ZBD/C4/gamez.zbd` | 8 102 080 | `91cdf00c…c62d458` | 967 279 328 (`C4`) | 3 000 | 2 431 | 2 283 | 19 661 | 19 660 | 0 | 1 |
| `ZBD/C5/gamez.zbd` | 9 385 808 | `4e7a6690…0469823d` | 967 279 700 | 3 000 | 2 851 | 2 851 | 22 493 | 22 485 | 0 | 8 |

The **unsupported** faces are n-gon outlines the ear-clipping triangulator
refuses (F10-B's `NgonIssue`), and they are **counted, not dropped**: the totals
in the `decoded` column plus the `unsupported` column equal the stored polygon
count for every archive. That is the AC04 report shape F10-D will aggregate over
the whole corpus; this task establishes the reader that produces it and the
per-airframe/world counts for the mesh section of every GameZ archive.

**Claim class.** The layout is `ObservedTool` — documented in the pinned
reference *and* measured against the installation. It is **not**
`verified_original`: no original run happened, and `retail` file access is not
evidence of runtime behaviour. `GameZMeshes::layout_evidence()` returns
`ObservedTool` and no code path can return a better class.

## Field worksheet

All words are little-endian. "Source" is the pinned file, with the line range of
the declaration. Offsets are relative to the start of the record. Every field the
reference names `unk` is kept raw here too, with a matching name.

### Container header — 40 bytes, `crates/mech3ax-gamez/src/gamez/cs/mod.rs:22-34`

| Offset | Size | Field | Meaning | Source | Retail check (9/9) |
| --- | --- | --- | --- | --- | --- |
| 0 | 4 | `signature` | `0x02971222` (`SIGNATURE`), shared by every game the source supports | `gamez/common.rs:8` | 9/9 |
| 4 | 4 | `version` | `42` for Crimson Skies (`VERSION_CS`) | `gamez/common.rs:13` | 9/9 |
| 8 | 4 | `unk08` | selects the mesh-index fixup; no other meaning established | `cs/mod.rs:25` | 9/9, seven distinct values |
| 12 | 4 | `texture_count` | entries of the texture-name table (not parsed here) | `cs/mod.rs:26` | 279–654 |
| 16 | 4 | `textures_offset` | start of the texture table; **equals 40** | `cs/mod.rs:27` | 9/9 = 40 |
| 20 | 4 | `materials_offset` | start of the material records (F10-C.02) | `cs/mod.rs:28` | 12 580–28 816 |
| 24 | 4 | `meshes_offset` | start of the mesh index; the section read here | `cs/mod.rs:29` | 56 372–103 388 |
| 28 | 4 | `node_array_size` | entries in the node array (F11-A) | `cs/mod.rs:30` | 3 317–11 438 |
| 32 | 4 | `light_index` | node index of the container's light node | `cs/mod.rs:31` | 2 338–9 979 |
| 36 | 4 | `nodes_offset` | start of the node array; the **exclusive end of the mesh section** | `cs/mod.rs:32` | 9/9, and the walk lands on it exactly |

The section chain the source asserts is
`textures_offset < materials_offset < meshes_offset < nodes_offset`
(`cs/mod.rs:95-132`), plus `read.offset == textures_offset` immediately after the
header read, which is what makes `textures_offset == 40` an equality.

### Mesh index — 12 bytes, `crates/mech3ax-gamez/src/gamez/common.rs:23-29`

| Offset | Size | Field | Meaning | Retail check |
| --- | --- | --- | --- | --- |
| 0 | 4 | `array_size` (`i32`) | slots in the mesh record array, present or not; `1..=i32::MAX-1` | 1 500–3 000 |
| 4 | 4 | `count` (`i32`) | present records; cross-checked against `parent_count > 0` | equals the present count in 9/9 |
| 8 | 4 | `last_index` (`i32`) | expected index after the **last present** mesh, after the fixup | matches in 9/9 |

The index is **non-sequential**. An *absent* record stores the index the next
present one is expected to carry, and that expectation is
`slot + 1`, or `-1` at the end of the array (`MeshIndexIter::next`,
`common.rs:36-47`) — except in two archives, where a measured table rewrites it
(`Fixup`, `cs/fixup.rs:176-272`): twelve pairs for `unk08 = 967 277 477`
(`planes.zbd`) and fifty-four for `unk08 = 967 279 328` (`C4/gamez.zbd`), plus
`last_index` 1779 → 1 313 and 2 490 → 2 283. Both tables were checked against
the bytes: every absent record of both archives stores exactly the remapped
value, and the other seven archives store exactly the sequential value.

### Mesh record — 100 bytes, `crates/mech3ax-gamez/src/mesh/ng.rs:15-42`

| Offset | Size | Field | Meaning | Retail |
| --- | --- | --- | --- | --- |
| 0 | 4 | `file_ptr` | `0` or `1` (`bool_c!`) | asserted, not enforced by this reader |
| 4 | 4 | `unk04` | `0`, `1` or `2` | asserted, not enforced |
| 8 | 4 | `unk08` | not interpreted | 7, 258, 263, 0 in the corpus |
| 12 | 4 | `parent_count` | **non-zero marks a present mesh**; zero means an all-zero stub | 0 or 1 |
| 16 | 4 | `polygon_count` | stored polygon records in this mesh | 0–2 290 |
| 20 | 4 | `vertex_count` | stored positions | 0–2 190 |
| 24 | 4 | `normal_count` | stored normals | 0–1 800 |
| 28 | 4 | `morph_count` | stored morph vectors | **0 in all nine archives** |
| 32 | 4 | `light_count` | stored mesh light records | 0–319 |
| 36 | 4 | `unk36` | asserted zero | — |
| 40 | 4 | `unk40` (`f32`) | not interpreted | 0.0 in the corpus |
| 44 | 4 | `unk44` (`f32`) | not interpreted | 0.0 in the corpus |
| 48 | 4 | `unk48` | asserted zero | — |
| 52 | 4 | `polygons_ptr` | the source's `Ptr`; only compared against zero. **Never used for reading** | 0 when `polygon_count == 0` |
| 56 | 4 | `vertices_ptr` | idem | — |
| 60 | 4 | `normals_ptr` | idem | — |
| 64 | 4 | `lights_ptr` | idem | — |
| 68 | 4 | `morphs_ptr` | idem | — |
| 72 | 4 | `unk72` (`f32`) | not interpreted | 0.0–2.13 |
| 76 | 4 | `unk76` (`f32`) | not interpreted | −0.0013–0.13 |
| 80 | 4 | `unk80` (`f32`) | not interpreted | −0.67–1.43 |
| 84 | 4 | `unk84` (`f32`) | not interpreted | 0.39–4.91 |
| 88 | 4 | `unk88` | asserted zero | — |
| 92 | 4 | `material_count` | stored mesh material references | 0–151 |
| 96 | 4 | `materials_ptr` | idem | — |

Followed by **one 4-byte word**: the mesh data offset for a present record, or
the expected index for a stub. A stub record is asserted to be entirely zero
(`assert_mesh_info_zero`, `ng.rs:673-704`).

**The pointers are not followed.** The reader reaches every array by walking
forward from the record's counts, exactly as `read_mesh_data` does, and requires
each present record's stored data offset to equal the offset the walk has
reached. That is the source's own `assert_that!("mesh offset", read.offset ==
mesh_offset, ...)`, and it is what makes the section walkable without trusting a
pointer whose meaning is not established.

### Mesh data, in stored order

`read_mesh_data` (`ng.rs:450-495`) and `read_polygons` (`ng.rs:370-448`). The
order below is the order, and it is the whole layout's load-bearing detail:

1. `vertex_count` × 3 × `f32` — positions;
2. `normal_count` × 3 × `f32` — normals;
3. `morph_count` × 3 × `f32` — morph vectors (empty in the corpus);
4. `light_count` × **76-byte light header**;
5. for each light in order, `extra_count` × 3 × `f32`;
6. `polygon_count` × **40-byte polygon record**;
7. for each polygon in order:
   1. `corners` × `u32` — position indices;
   2. `corners` × `u32` — normal indices, **only if the `NORMALS` bit is set**;
   3. `mat_count` × `u32` — material indices, one per group;
   4. `mat_count × corners` × 2 × `f32` — UVs, group by group;
   5. `corners` × 3 × `f32` — corner colours;
8. `material_count` × **12-byte mesh material reference**.

Steps 4/5 and 6/7 are **two-pass**: every header of a kind is stored before any
of that kind's variable-length data. A reader that interleaved them would
desynchronise at the second light and never reach the polygons. The synthetic
fixtures cover exactly that (`accept_f10_b_gamez_light_records_are_read_two_pass`,
`..._polygon` records probe).

### Polygon record — 40 bytes, `crates/mech3ax-gamez/src/mesh/ng.rs:77-89`

| Offset | Size | Field | Meaning | Retail |
| --- | --- | --- | --- | --- |
| 0 | 4 | `vertex_info` | packed: **corners = `& 0x1FF`** (nine bits, `0x1FF` max), **flags = `(& 0xFE00) >> 8`** (seven bits, `0xFE` max, low bit always zero) | corners 3–124, flag bytes `0x00`, `0x04`, `0x08`, `0x10`, `0x14`, `0x18`, `0x30`, `0x38`, `0x40`, `0x44` |
| 4 | 4 | `unk04` (`i32`) | asserted `-50..=50` | −49…49, inside the range |
| 8 | 4 | `vertices_ptr` | `Ptr`, not followed | — |
| 12 | 4 | `normals_ptr` | `Ptr`, non-zero exactly iff `NORMALS` | — |
| 16 | 4 | `mat_count` | stored material groups; asserted `> 0` | 1 in 127 728 polygons, **2 in 1 000, 3 in 6**; `planes.zbd` is all 1s, the world archives carry the rest |
| 20 | 4 | `uvs_ptr` | `Ptr`, not followed | — |
| 24 | 4 | `colors_ptr` | `Ptr`, not followed | — |
| 28 | 4 | `unk28` | asserted non-zero | — |
| 32 | 4 | `unk32` | asserted non-zero | — |
| 36 | 4 | `unk36` | asserted `<= 0xFFFF` | — |

**The flag field** (`PolygonBitFlags`, `ng.rs:91-100`) — only two bits are given
a meaning here, because only two are established:

| Bit (of the flag field) | Name | Meaning | Retail occurrence |
| --- | --- | --- | --- |
| 2 | `UNK2` | not interpreted | 52 621 polygons |
| 3 | `UNK3` | not interpreted ("not in mechlib") | 1 036 |
| 4 | `NORMALS` | one normal index per corner follows the position indices | 54 100 |
| 5 | `TRI_STRIP` | the corners are a triangle strip, not one outline | 22 124 |
| 6 | `UNK6` | not interpreted ("not in mechlib") | 240 |

(Out of 128 734 stored polygons across the nine archives.)

`TRI_STRIP` is **the only** thing that selects `PrimitiveKind::TriangleStrip`
versus `PrimitiveKind::Polygon`; that is the selector F10-A recorded as unknown.
`NORMALS` is the only thing that says a normal index array follows. No flag bit
outside these five occurs in the corpus, and a stored one that does is read,
decoded and reported as a `ParseFinding::UnknownPolygonFlagBits` rather than
silently accepted or refused.

### Mesh material reference — 12 bytes, `crates/mech3ax-api-types/src/gamez/mesh/ng.rs:22-27`

| Offset | Size | Field | Meaning |
| --- | --- | --- | --- |
| 0 | 4 | `material_index` | index into the container's material records |
| 4 | 4 | `polygon_usage_count` | read by the source, not otherwise used |
| 8 | 4 | `unk_ptr` | not interpreted |

The source asserts `material_index < material_count`, where `material_count`
comes from the material section it read. **This task does not parse that
section**, so the index stays a raw reference, is never range-checked, and
`GameZMeshes::unchecked_material_references` counts them: 19 810 in
`planes.zbd` (3 610 mesh-level plus 16 200 per-polygon), 32 904 mesh-level
across the nine archives. F10-C.02 owns the check. This is a deliberate gap,
stated in the report and in the API, not an oversight.

### Mesh light record — 76-byte header, `crates/mech3ax-gamez/src/mesh/common.rs:86-106`

| Offset | Size | Field | Source's own constraint | Retail |
| --- | --- | --- | --- | --- |
| 0 | 4 | `unk00` | `0` or `1` | 0 only |
| 4 | 4 | `unk04` | `0`, `1` or `2` | 0, 1 |
| 8 | 4 | `unk08` (`f32`) | `0.0..=5.0` | 0.0, 1.0, 2.0, 5.0 |
| 12 | 4 | `extra_count` | `1..=10` | **1 only** |
| 16 | 4 | `unk16` | zero | 0 |
| 20 | 4 | `unk20` | zero | 0 |
| 24 | 4 | `unk24` | **assertion commented out in the source**; several values occur | ten distinct values, 0/1/2 and `0xBF800000`-style bit patterns |
| 28 | 12 | `color` | 3 × `f32` in `0.0..=255.0` | 34 distinct; 218.7 grey, 255/212/42, 255/255/255, 0/0/0 |
| 40 | 2 | `pad40` | zero | 0 |
| 42 | 2 | `flags` | `LightFlags` bitfield, not interpreted here | 51 distinct values, 0x01FF/0x01E7/0x0200… |
| 44 | 4 | `ptr` | non-zero | e.g. `0x03BB7D30` |
| 48 | 4 | `unk48` (`f32`) | `0.0..=2000.0` | 0.0, 1000.0 |
| 52 | 4 | `unk52` (`f32`) | `0.0..=3500.0` | 0.0, 1500.0, 2000.0, 2500.0 |
| 56 | 4 | `unk56` (`f32`) | `0.0..=5.1` | 0.0, 0.102, 0.17, 0.255 |
| 60 | 4 | `unk60` | `0` or `1` | 0, 1 |
| 64 | 4 | `unk64` (`f32`) | `0.0..=5000.0` | 0.0, 5.0, 30.0, 4000.0 |
| 68 | 4 | `unk68` (`f32`) | `0.0..=4000.0` | 0.0, 5.0, 4000.0, 6000.0 |
| 72 | 4 | `unk72` (`f32`) | `0.0..=10000.0` | 0.0, 10000.0, 4000.0, 6000.0 |

Followed by `extra_count` × 3 × `f32`. The reference's own comment for `unk64`,
`unk68` and `unk72` records measured values outside the range it asserts, and
the corpus agrees: the assertions in that file are the source's *guesses*, not
established facts, and this reader enforces none of them. The **only** light
field whose meaning the layout needs is `extra_count`, because it is what makes
the record's length knowable.

### Node → mesh association (recorded, not read)

`NodeCsC` (`crates/mech3ax-nodes/src/cs/node.rs:86-117`) is a 208-byte record
whose **offset 60 is `mesh_index` (`i32`)** — the index into the very mesh array
this task reads. `read_nodes` (`gamez/cs/nodes.rs:91-101`) asserts a non-negative
`mesh_index` is inside the array *and* that the slot holds a present mesh. Node
records themselves are **F11-A's** and are not parsed here. What this task
provides for that binding is the addressing: `GameZMeshes` has one entry per
array slot in stored order, and `GameZMeshes::get(mesh_index)` is the lookup a
node performs, with `GameZMesh::index` naming the slot.

## Files and the one observable failure (listed before editing)

- `crates/cs_formats/src/gamez/reader.rs` (new, ~1 800 lines): the layout
  constants (`GAMEZ_HEADER_BYTES`, `MESH_INFO_BYTES`, `POLYGON_INFO_BYTES`,
  `MESH_MATERIAL_INFO_BYTES`, `MESH_LIGHT_HEADER_BYTES`, `MESH_INDEX_BYTES`,
  `MESH_INFO_TRAILER_BYTES`, `VEC3_BYTES`, `CORNER_COUNT_MASK`, `FLAG_MASK`,
  `FLAG_SHIFT`, `MAX_POLYGON_CORNERS`, `MAX_POLYGON_FLAGS`, `KNOWN_POLYGON_FLAGS`,
  `FLAG_NORMALS`, `FLAG_TRIANGLE_STRIP`, `FLAG_UNK2/3/6`, `UNK08_PLANES`,
  `UNK08_C4`), `Fixup` with both measured tables, `GameZHeader`, `MeshIndex`,
  `RawMeshInfo`, `RawPolygonInfo` (+ `corners`/`flags`/`has_normals`/
  `is_triangle_strip`/`kind`/`unknown_flag_bits`), `RawMeshMaterialInfo`,
  `RawMeshLightHeader`, `RawMeshLight`, `GameZMesh`, `GameZMeshes` (+ `present`,
  `present_count`, `get`, `topologies`, `layout_evidence`),
  `unchecked_material_references`, `ParseFinding`, `GameZError`,
  `MESHES_ENTRYPOINT` and `read_gamez_meshes`.
- `crates/cs_formats/src/gamez/mesh.rs`: `RawPolygon` gains
  `materials: Vec<RawMaterialGroup>` (new type `RawMaterialGroup { material,
  uvs }`), and `RawPolygon::material` / `RawCorner::uv` are documented as the
  **first** group mirrored onto the single-valued IR fields. `RawMesh` itself is
  unchanged.
- `crates/cs_formats/src/gamez/mod.rs` (wiring): `pub mod reader;`, re-exports,
  module doc.
- `crates/cs_formats/src/lib.rs` (wiring): one module-doc paragraph.
- `crates/cs_formats/tests/gamez/reader.rs` (new): the fixture writer and
  sixteen `accept_f10_b_gamez_*` tests, two of them retail.
- `crates/cs_formats/tests/gamez/main.rs`, `ngon.rs`: the two F10-A/F10-B
  fixtures now state their (single) material group explicitly. No assertion
  changed.
- `docs/findings/2026-09-29-f10-b-gamez-mesh-layout.md` (this file),
  `docs/findings/evidence/F10-B-gamez-layout.json`,
  `crates/cs_formats/tests/evidence_report_f10_b_gamez.rs` (evidence harness).

**One observable failure:** a reader that reads a polygon's normal array
unconditionally walks 12 bytes past the stored arrays for every polygon without
`NORMALS` (4 914 such polygons in `planes.zbd` alone), so the second polygon's
record is read from the middle of the first one's colours and the mesh data walk
ends far from `nodes_offset`. `accept_f10_b_gamez_flags_select_strip_and_normals`
fails on a three-byte fixture with one normal polygon and one without, and
`accept_f10_b_gamez_retail_every_archive_lands_on_the_reference_offset` fails on
`planes.zbd` with the walk's endpoint in place of the reference's
`nodes_offset`.

## Design decisions

- **Walk, do not follow pointers.** Every array is reached by stepping forward
  from the record's counts. The stored `*_ptr` fields are kept raw on
  [`RawMeshInfo`] and [`RawPolygonInfo`] and are never read. This is the
  source's own algorithm, and it is what lets the reader *prove* it read the
  whole section by landing on `nodes_offset`.
- **The section boundary is the correctness check.** `data_end` must equal
  `nodes_offset` exactly, or the read fails with both numbers. There is no
  "best effort" mode: a mesh whose records do not tile the section is a failure
  with the mesh index, not a partial mesh.
- **Assertions the source makes about values are recorded, not enforced** —
  except where violating them would make the byte stream unreadable. The
  source's `file_ptr ∈ {0,1}`, `unk04 ∈ {0,1,2}`, `unk36 == 0`, light range
  checks and `polygon.unk04 ∈ [-50,50]` are *value* assertions: a value outside
  them changes no offset, so enforcing them would refuse faces the layout reads
  exactly. The checks that are enforced are the ones the offset arithmetic
  depends on: `array_size` in range, the record array inside the section,
  stub records entirely zero, the present count, the remapped `last_index`, the
  non-decreasing and walk-matching data offsets, and the section chain.
- **A stored record the reference would refuse is read, decoded and reported.**
  An unmapped flag bit, a strip without normals, a polygon with no material
  group, a polygon with fewer than three corners: each becomes a
  [`ParseFinding`], and the face still reaches `RawMesh::topology` so its count
  is exact. `GameZMeshes::findings` is empty for all nine measured archives,
  which is the strongest statement this reader makes about them.
- **Every material group is kept.** The IR's single `material` field and
  per-corner `uv` mirror group 0, so a one-material consumer is never wrong;
  `RawPolygon::materials` holds every group, so a multi-material consumer is not
  either. The measured corpus stores 1 006 multi-group polygons (1 000 with two
  groups, 6 with three), all of them in the world archives — `planes.zbd` is
  entirely single-group — so the multi-group path is exercised by retail as well
  as by the synthetic fixtures. Some stored groups repeat another's
  coordinates; the reader keeps them apart either way.
- **`unchecked_material_references` is a count, not a finding.** It holds for
  every mesh at once; one finding per mesh would have produced 1 744 entries in
  `planes.zbd` and drowned the per-face findings.
- **`unk08` selects the fixup on its own.** The source compares the whole header
  struct, but `unk08` alone separates the two tables across the nine archives,
  and the other nine fields are validated separately. The reader exposes both
  measured tables and the identity case.
- **Bounded by the F03 budget.** Every buffer a parse hands out is reserved
  before it is created, the record array's byte extent is checked against
  `nodes_offset` by arithmetic before a record is read, and a failed attempt
  leaves the ledger as it was so the same context can retry.

## Test inventory (`accept_f10_b_gamez_*`)

All in `crates/cs_formats/tests/gamez/reader.rs`; every one calls
`read_gamez_meshes` or a type it returns. The synthetic fixtures are authored in
the test file by a writer that shares no code with the reader, composes each
mesh's data from its own parts, and **refuses** a container whose parts
contradict its own stored counts — so a fixture cannot accidentally parse.

| Test | Covers |
| --- | --- |
| `header_gates_the_section` | signature, version, `textures_offset == 40`, the section order, the section bounds, each with its own variant, offset and numbers |
| `every_truncation_fails_loudly` | **every** byte length from 0 to `len-1` fails; only the whole container parses |
| `arrays_come_from_counts_not_pointers` | the `*_ptr` fields point nowhere and are kept raw; positions, normals, polygon record, UVs still read correctly, in order |
| `flags_select_strip_and_normals` | `TRI_STRIP` is the only topology selector, `NORMALS` the only one that adds a normal array, an unmapped bit is a finding and the face still decodes |
| `keeps_every_material_group` | two groups, both material indices and both UV sets survive, group 0 mirrored onto the single-valued fields |
| `shared_position_keeps_both_corners` | one position index used twice keeps two corners with different UVs and colours, visible through the source-corner map (F10 #3, AC03) |
| `light_records_are_read_two_pass` | two lights with one and two trailing vectors; all 19 header fields survive; the second light's vectors would be misread if the passes were interleaved |
| `nonsequential_index_uses_the_fixup` | both measured remap tables at their measured values, unlisted indices pass through, `last_index` remaps, a bad stub word is refused with both numbers, and the **measured planes and C4 cases on containers large enough to reach them** — refused without the table, accepted with it |
| `index_contradictions_are_named` | wrong `count`, wrong `last_index`, a stub differing in any of four different fields, a zero and a negative count, and the largest `array_size`, each its own variant |
| `face_counts_are_exact` | one status per stored polygon; an out-of-range position index is **invalid**, an out-of-range normal index a **different** issue, and the polygon after a rejected one still decodes |
| `hostile_counts_are_refused_and_retryable` | `0xFFFF_FFFF` positions and three oversized `array_size` values refused before allocating; the ledger is unchanged afterwards and the same context then reads a good container; the record-table reservation is load-bearing at a budget one byte short |
| `data_section_ends_at_nodes_offset` | the walk lands on `nodes_offset`; two meshes abut exactly; a container whose data does not fill its section is refused with both numbers, and a claimed section past the end is refused first |
| `vertex_info_splits_into_nine_bit_fields` | the count is **nine** bits and the flags the top **seven**; both fields' full width, and a 260-corner stored record no eight-bit mask could describe |
| `a_meshes_declared_offset_must_match_the_walk` | a mesh whose declared data offset the walk cannot reach is refused **naming that mesh and both offsets** |
| `retail_every_archive_lands_on_the_reference_offset` (retail) | all nine archives: the walk lands on the reference's own recorded `nodes_offset`, the header matches the reference's recorded words, the index is self-consistent, both remapped archives select their table and store the measured `last_index`, every face is counted exactly once, no invalid face, no findings |
| `retail_flags_groups_and_seams_over_the_whole_corpus` (retail) | all nine archives: 128 734 polygons, strips only where `TRI_STRIP` is set, both normal cases, raw flag bytes include the bits the source does not name and are all one byte, **every stored group holds one coordinate per corner** and the corpus has two- and three-group polygons, corners share positions and shared ones keep their own attributes, 783 lights read, **no morph vectors anywhere**, and the unchecked-material count matches at both levels |

## Mutation probes

Applied to `crates/cs_formats/src/gamez/reader.rs`, one at a time, file restored
after each, `cargo test --locked -p cs_formats --test gamez -- accept_f10_b_gamez_
--include-ignored` run with `CS_GAME_DIR` set. Counts are failing task tests out
of sixteen.

| Mutation | Failing |
| --- | --- |
| corner count masked with 8 bits instead of 9 | 1 |
| flag field shifted by 1 instead of 8 | 16 |
| `TRI_STRIP` no longer selects the corner topology | 3 |
| normal array read unconditionally | 3 |
| only the first material group kept | 3 |
| per-corner UV taken from the last group | 1 |
| the IR's mirrored `material` taken from the last group | 1 |
| the stored material groups dropped from the mesh | 3 |
| per-corner UV dropped | 1 |
| per-corner normal dropped | 1 |
| per-corner colour dropped | 1 |
| polygon records and polygon arrays interleaved (structural) | 16 |
| light headers and light extras interleaved (structural) | 3 |
| stub index expectation not run through the fixup table | 3 |
| fixup always `Fixup::None` | 3 |
| present count not cross-checked | 1 |
| `last_index` not cross-checked | 1 |
| stub check removed entirely | 1 |
| stub check reduced to `unk08` only | 1 |
| stub check reduced to `unk44` only | 1 |
| end-of-section check removed | 1 |
| section order not checked | 1 |
| section bounds not checked | 2 |
| sequential-offset check removed | 1 |
| record array not bounded by `nodes_offset` | 1 |
| record-table allocation reservation removed | 1 |
| signature not checked | 1 |
| version not checked | 1 |
| unknown polygon flag bit silently accepted | 1 |

**Two probes survive, and both are equivalent mutations rather than holes:**

- *Removing the per-corner UV allocation reservation.* The corner count is nine
  bits, so the UV request is at most `0x1FF × 8 = 4 088` bytes — below any budget
  that lets the surrounding arrays through (the colours next reserve
  `0x1FF × 12 = 6 132`). No legal container can make that reservation
  load-bearing. It is kept because F03 asks the ledger to describe every buffer a
  parse hands out, not only the ones that could be large.
- *Replacing the `checked_mul` on the record array's byte count with a plain
  multiplication.* `array_size` is at most `i32::MAX - 1`, and
  `(i32::MAX - 1) × 104` is about 2.2 × 10¹¹, which cannot overflow `u64` on
  any platform. The check is kept for the same reason, and the *observable* part
  of that bound — the extent against `nodes_offset` — is covered by
  `..._hostile_counts_are_refused_and_retryable`.

## Recorded unknowns

- **The meaning of every `unk` field.** `unk08` on the header and on a mesh
  record, `mesh.unk04`, `unk40/44/72/76/80/84`, `polygon.unk04/28/32/36`, every
  `LightC` field, and all five `*_ptr` fields are stored raw and are not
  interpreted. In particular nothing is read as specularity, soil, or a display
  colour (F10 non-negotiable #5) — and the material record itself is not parsed
  at all, so the field the source calls `specular` is not in this task's reach.
- **Material references are unvalidated.** `RawMeshMaterialInfo::material_index`
  and `RawPolygon::materials[i].material` are raw references; see above. F10-C.02
  resolves them.
- **Front-face winding and handedness.** Still unknown (F10-A). The reader stores
  the corners in stored order and lets `RawMesh::topology` keep the stored
  winding; nothing here says which winding the original engine treated as front.
- **How the original renderer handled n-gons.** Nine of the 129 933 stored
  polygons in the corpus are outlines this triangulator refuses (1 in C4, 8 in
  C5, 0 elsewhere), and the reader counts them as unsupported rather than
  fanning them. What the original engine drew for them is F10-D's question on the
  private corpus.
- **Non-planar outlines.** The ear-clipping triangulator projects onto the plane
  its Newell normal is most aligned with; whether retail n-gons are planar is
  unmeasured. F10-B (the triangulation stage) recorded this; it survives here
  because the reader feeds it those same outlines.
- **The `*_ptr` fields.** Read and kept, never used. What they point at in the
  original engine, and whether they are addresses, indices or something else, is
  not established by anything read here.
- **Light records.** Parsed because their length is needed to reach the polygons,
  and kept raw. Their role in the original renderer (F18's light inventory) is
  not established.
- **The texture-name table and the material records** are not parsed. Their
  offsets are only used to bound the mesh section, and the header's
  `texture_count` is carried but unchecked against anything.
- **Morph vectors.** The layout and the reader handle them; `morph_count` is zero
  in all nine measured archives, so nothing is known about their content.
- **The node records** are F11-A's. What this task establishes is the addressing
  (`GameZMeshes::get(mesh_index)`) and the one field the source ties to it
  (`NodeCsC.mesh_index` at offset 60).

## Deferred scope, its resolving task and what it gates

The acceptance report's `unknowns` array is **empty**. That is a statement, not
an omission: everything this task could not resolve is a named boundary with a
resolving task, and it is written down here — the durable, versioned record. None
of them is an unresolved issue with the claim the report makes (the layout is
established from the pinned reference, checked against all nine retail archives,
and every one of them is parsed to the reference's own recorded
`nodes_offset`). Each item below names the content it affects, the task that
resolves it, and the claim it gates, so none of them can be forgotten when a
parent is marked done.

| # | Deferred item | Affected content | Resolving task | Gates |
| --- | --- | --- | --- | --- |
| 1 | Material records are not parsed, so **no material index is range-checked**. `GameZMeshes::unchecked_material_references` names the count: 19 810 in `planes.zbd` (3 610 mesh-level, 16 200 per-polygon), 32 904 mesh-level across the nine archives | every material reference of every mesh in all nine archives | **F10-C.02** (#365) | any claim that a polygon's material *resolves*; the F10-C upload path's material binding |
| 2 | The texture-name table is not parsed; `texture_count` is carried and checked against nothing | all texture names of all nine containers | F10-C.02 | any texture-dependency claim |
| 3 | Node records are not parsed. What is provided is the addressing: one entry per mesh-array slot and `GameZMeshes::get(mesh_index)`, the field the source ties to it being `NodeCsC.mesh_index` at offset 60 of the 208-byte record | every scene node of every container | **F11-A** | any world-assembly or collision claim (F18) |
| 4 | Every `unk` field is stored raw and uninterpreted: the header's `unk08` (beyond fixup selection) and `light_index`, the mesh record's `unk04/unk08/unk40/unk44/unk72/unk76/unk80/unk84`, the polygon's `unk04/unk28/unk32/unk36`, and every `LightC` field but `extra_count` | all nine archives | F10-D (the private-corpus pass) and F11-A/F18 where the field turns out to be needed | any claim that a stored value has a *meaning*; nothing may be read as specularity or soil (F10 non-negotiable #5) |
| 5 | The `*_ptr` fields are read and kept but **never followed**, and their runtime meaning is not established | `MeshNgC.polygons_ptr/vertices_ptr/normals_ptr/lights_ptr/morphs_ptr/materials_ptr`, `PolygonNgC.vertices_ptr/normals_ptr/uvs_ptr/colors_ptr/unk28/unk32` | not scheduled; F10-D may need it if a pointer is the only way to reach a section | nothing currently — the walk from the counts is what makes the section verifiable |
| 6 | Front-face winding and handedness | every polygon of every mesh | **F10-D** (#46, retail) | any lighting, culling or normal claim |
| 7 | What the original renderer did with the **nine outlines this triangulator refuses** (1 in C4, 8 in C5); and whether retail n-gons are planar | 9 of 128 734 stored polygons | **F10-D** (#46, retail) | AC04's "exact missing/invalid face counts for every private world and airframe" |
| 8 | Light records are parsed only because their length is needed to reach the polygons; their role in the original renderer is not established | 783 light records across the nine archives | F18 (light inventory) | any lighting claim |
| 9 | Morph vectors: the layout and the reader handle them, `morph_count` is zero in all nine archives | none in the measured corpus | not scheduled; no content is affected | nothing currently |
| 10 | Original-run behaviour of any kind: how the engine consumed this section, what it did with an ngon, what a light did | the whole mesh section | the owner (`human_play` / `human_review`) | every `verified_original` and `release_approved` claim; this report's claim is `implemented` and its layout class is `ObservedTool` |

## What F10-C.02 and F10-C.03 need from this

- **F10-C.02** (material records and their texture dependencies) needs
  `materials_offset..meshes_offset` parsed and the `material_index` range check
  that this task deliberately leaves open;
  `GameZMeshes::unchecked_material_references` is the number it must drive to
  zero.
- **F10-C.03** (upload path) now has a real producer: `read_gamez_meshes` over a
  container's bytes yields `GameZMeshes`, `GameZMeshes::topologies()` yields the
  exact per-mesh decoded/invalid/unsupported counts AC04 asks for, and
  `RawMesh::topology` is the shared gate (`is_complete`) before upload.
  `crates/cs_content/src/mesh.rs` (vertex splitting, AC03's other half) and
  `crates/cs_app/src/mesh.rs` are still unbuilt and remain that stage's work.
