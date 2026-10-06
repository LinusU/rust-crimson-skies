# F10-C.02: the GameZ material records, and how a material names its texture

Date: 2026-09-29. Task: Rally **#365** `F10-C.02`, "Audit GameZ material
records and their texture dependencies" (a slice of **#43** `F10-C`, the "audit
material/texture dependencies" half of stage F10-C's non-negotiable #5). Sheet:
`specs/F10-gamez-mesh-topology-and-material-records.md`, section `### F10-C`.
Contract: `docs/contracts/IDENTITY-CONTENT.md`. Read first: **#363**'s
`docs/findings/2026-09-29-f10-b-gamez-mesh-layout.md` (the container header, the
mesh section, and the two deferred items this task closes) and
`docs/findings/2026-09-28-f08-c-texture-catalog-and-upload-boundary.md` (the
`TextureCatalog`/`TextureRef` API this task resolves through). Capabilities used:
**`retail`** (read-only, `$CS_GAME_DIR`) plus ordinary build/test. Test prefix:
`accept_f10_c_02_`. Evidence: `docs/findings/evidence/F10-C.02.json`.

## Sources

- **Pinned reference** — mech3ax **v0.6.0**, commit
  `d3521a9721be731d365504568ddcd78e3f9846bb` ([S02], [S06], [S17] in
  `docs/research/SOURCES.md`). Cloned into a private research directory outside
  this repository and read only: the licence is **EUPL-1.2** and **no code was
  copied**. Files read:
  - `crates/mech3ax-gamez/src/gamez/cs/mod.rs` — `HeaderCsC`, `read_gamez`, the
    section chain, and the two lines that fix the order of the two sections read
    here (`read.offset == textures_offset` and `read.offset == materials_offset`);
  - `crates/mech3ax-gamez/src/textures/ng.rs` — `TextureInfoNgC`,
    `read_texture_infos`, the 44-byte record and the `used` word's two values;
  - `crates/mech3ax-gamez/src/materials/mod.rs` — `MaterialInfoC` (16 bytes),
    `MaterialC` (40 bytes), `CycleInfoC` (28 bytes), `MaterialFlags` (five bits),
    and `MatType::Ng`'s `size_i32`/`size_i16`/`size_u32` of **1000**;
  - `crates/mech3ax-gamez/src/materials/read_multi.rs` — `read_materials`,
    `assert_material_info`, `read_materials_zero`: the two-pass order and the two
    link-word rules;
  - `crates/mech3ax-gamez/src/materials/read_single.rs` — `read_material`,
    `read_material_zero`, `read_cycle`, and the `material.index` to
    `textures[index]` binding;
  - `crates/mech3ax-common/src/string/mod.rs` — `str_from_c_suffix` and
    `from_ascii`, the name encoding;
  - `crates/mech3ax-common/src/texture/…` — not read; the texture *archive* side
    is F08's, and is only used here through `TextureCatalog`.
- **Original installation** — the read-only installation at `$CS_GAME_DIR`. All
  **nine** GameZ archives were read, and the world's texture archives were read
  to compare their stored names. Fingerprints (SHA-256 of the whole file) are in
  the per-archive table below and in the evidence artifact `material-corpus.json`
  (private, referenced by digest).

## What was established, and how

Two independent steps, in this order, with no feedback from one into the other:

1. **Read the layout from the pinned source** and write it down as a worksheet
   (§ "Field worksheet"), from the section chain down to the byte order of one
   material record.
2. **Check it against the installation** with a throwaway probe written from the
   worksheet alone — a Python reader that shares no code with the production
   Rust reader and whose expected values are the worksheet's, not the Rust
   reader's output.

The result of step 2 is stronger than "it parsed": for **all nine archives** the
material walk ends **exactly** on the `meshes_offset` the header declares, and
`materials_offset == textures_offset + 44 × texture_count` in all nine, to the
byte. The section's length depends on the stored `count` *and* on which records
set the cycled flag, so a reader with any record length, any slot count or any
cycle-record size wrong ends somewhere else. That equality is the discriminating
check.

| Archive | Bytes | SHA-256 | `texture_count` | materials | textured | untextured | cycled | `UNKNOWN` flag | zero slot | duplicate names |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `ZBD/planes.zbd` | 6 083 868 | `45da54a8…8fbc21b` | 298 | 954 | 936 | 18 | 608 | 134 | 46 | 36 |
| `ZBD/C1/gamez.zbd` | 6 875 076 | `2a61482d…ae31a8` | 565 | 570 | 552 | 18 | 3 | 0 | 430 | 0 |
| `ZBD/C1B/gamez.zbd` | 3 953 304 | `38ea1c08…28f099` | 359 | 320 | 310 | 10 | 5 | 0 | 680 | 0 |
| `ZBD/C1C/gamez.zbd` | 4 008 612 | `6ad39cd4…5a7794` | 279 | 284 | 276 | 8 | 1 | 0 | 716 | 0 |
| `ZBD/C2/gamez.zbd` | 4 908 288 | `2b2cf09b…4c924b6` | 498 | 492 | 480 | 12 | 2 | 0 | 508 | 0 |
| `ZBD/C2B/gamez.zbd` | 3 435 348 | `8b64f30b…f2ab` | 285 | 274 | 267 | 7 | 2 | 0 | 726 | 0 |
| `ZBD/C3/gamez.zbd` | 5 633 696 | `30318f7b…fa81d73` | 465 | 483 | 456 | 27 | 2 | 21 | 517 | 0 |
| `ZBD/C4/gamez.zbd` | 8 102 080 | `91cdf00c…c62d458` | 654 | 685 | 662 | 23 | 2 | 38 | 315 | 0 |
| `ZBD/C5/gamez.zbd` | 9 385 808 | `4e7a6690…0469823d` | 582 | 607 | 583 | 24 | 2 | 22 | 393 | 0 |

Totals: **3 985** texture-name entries, **4 669** present material records, 4 522
textured and 147 untextured, 627 cycled, 215 with the `UNKNOWN` flag set. Every one of the 3 985 stored texture
indices is inside its container's own table, and the production reader reports
**zero** findings over all nine archives.

**Claim class.** The layout is `ObservedTool` — documented in the pinned
reference *and* measured against the installation. It is **not**
`verified_original`: no original run happened, and `retail` file access is not
evidence of runtime behaviour. `GameZMaterials::layout_evidence()` returns
`ObservedTool` and no code path can return a better class.

## Field worksheet

All words are little-endian. "Source" is the pinned file at the commit above,
with the line range of the declaration. Offsets are relative to the start of the
record. Every field the reference names `unk` is kept raw here too, with a
matching name.

### Where the two sections sit

The container header is F10-B's `GameZHeader` and is not repeated here. This
task's reader re-reads and re-validates it (so either entrypoint can be used
alone and both reject the same bytes for the same reason) and then reads the two
sections the header points at:

| Field | Meaning | Source | Retail check (9/9) |
| --- | --- | --- | --- |
| `textures_offset` | start of the texture-name table; **equals 40**, because the table is read straight after the header | `cs/mod.rs:111-115` | 9/9 = 40 |
| `texture_count` | entries of the texture-name table; the reference asserts `< 4096` | `cs/mod.rs:95` | 279–654 |
| `materials_offset` | start of the material records; the reference asserts the texture table ends here | `cs/mod.rs:120-124` | 12 316–28 816 |
| `meshes_offset` | start of the mesh index; the reference asserts the material records end here | `cs/mod.rs:127-131` | 56 372–103 388 |

`materials_offset − textures_offset` equals `44 × texture_count` **exactly** in
all nine archives, which is the table's byte extent stated by the container
itself.

### Texture-name record — 44 bytes, `crates/mech3ax-gamez/src/textures/ng.rs:14-23`

| Offset | Size | Field | Meaning | Source | Retail check (9/9) |
| --- | --- | --- | --- | --- | --- |
| 0 | 4 | `unk00` | the reference's `Ptr`: null when `used == 2`, non-null when `used == 1`. **Never followed.** | `ng.rs:50-59` | 175 null, 3 810 non-null |
| 4 | 4 | `zero04` | asserted zero | `ng.rs:42` | 0 everywhere |
| 8 | 4 | `zero08` | asserted zero | `ng.rs:43` | 0 everywhere |
| 12 | 20 | `texture` | `Ascii<20>`, the stored name (§ "The name encoding") | `ng.rs:44-46` | every entry decodes |
| 32 | 4 | `used` | `2` in use, `1` being processed | `ng.rs:49` | both values occur |
| 36 | 4 | `index` | asserted zero | `ng.rs:61` | 0 everywhere |
| 40 | 4 | `unk40` | asserted `-1` | `ng.rs:62` | −1 everywhere |

`texture_count` and this table are the whole of F10-B's deferred item 2.

### The name encoding

`str_from_c_suffix` (`crates/mech3ax-common/src/string/mod.rs:137-165`) reads the
20-byte field as **two NUL-separated runs** and replaces the first NUL with a
`.`, which is the layout's way of storing a name and its extension in one fixed
field. Three shapes occur in the measured corpus and all three are ordinary:

| Stored bytes (hex, 20) | Shape | Stored name | Count in `planes.zbd` |
| --- | --- | --- | --- |
| `…\x00tif\x00…` | name, NUL, extension, NUL, padding | `horizonindicator.tif` | 5 |
| `lightmap\x00\x00…` | name, NUL, NUL, padding — **no extension** | `lightmap` | — (C2 has 5) |
| `bldhwk_cowling\x00.tif\x00` | name, NUL, extension that itself starts with `.` | `bldhwk_cowling..tif` | 36 |
| `cpit_bullethole1\x00tif` | **completely full**: one NUL, no second terminator | `cpit_bullethole1.tif` | 5 of the 95 in the corpus |

Over the whole corpus the three shapes are **3 872** with an extension, **95**
completely full and **18** with no extension at all. Two consequences that the
corpus forces and that a reader must not "fix":

- A name that fills the field is **truncated by the layout, not by the reader**.
  `blo_fusalagebottom\x00ti` decodes to `blo_fusalagebottom.ti` — twenty bytes
  cannot hold `blo_fusalagebottom.tif`. Whether the original engine matched such
  a name by prefix, or the content is simply absent, is not established (§
  "Recorded unknowns", item 2).
- A name with a dot inside it decodes to a **double dot**
  (`bldhwk_cowling..tif`), and 36 entries of `planes.zbd` store exactly that
  name. The pinned reference renames repeats on **write**
  (`bldhwk_cowling..1.tif`, …) so that a round trip is unambiguous; that is an
  extractor convention and this reader does **not** reproduce it, so the
  duplicate stays visible (`GameZMaterials::duplicate_names`).

The extension's case is stored, not folded: over the 1 476 distinct names the
nine containers hold, the stored extension is `tif` 1 410 times, `TIF` 42, a
truncated `t` 8, `ti` 5, an empty run 3, no extension at all 6, `jpg` once and
`TI` once (`sacredtrust_logo1.TI`). `from_ascii` refuses only a byte with the
high bit set; no stored name in the corpus has one, and control bytes below 0x80
are accepted by the reference, so this reader accepts them too and stores the
name as `str_from_c_sized` would return it.

### Material section header — 16 bytes, `materials/mod.rs:48-54`

| Offset | Size | Field | Source's constraint | Retail check (9/9) |
| --- | --- | --- | --- | --- |
| 0 | 4 | `array_size` (`i32`) | `0 <= array_size <= 1000` (`MatType::Ng::size_i32`) | equals `count` in 9/9 |
| 4 | 4 | `count` (`i32`) | `0 <= count <= array_size` | 274–954 |
| 8 | 4 | `index_max` (`i32`) | `== count` | 9/9 |
| 12 | 4 | `index_last` (`i32`) | `== count - 1` | 9/9 |

**`array_size` is not the length of the array.** The reference asserts only that
it is at most 1000 and then always *reads* and *writes*
`(MaterialC::SIZE + 2 + 2) × 1000` slots (`materials/mod.rs:112`). In the
measured corpus `array_size == count` in all nine archives, so the corpus cannot
distinguish the two readings; the physical array is 1000 slots in all of them and
that is what the reader walks.

### Material record — 40 bytes, `materials/mod.rs:58-70`

| Offset | Size | Field | Source's constraint | Retail (textured / untextured / zero slot) |
| --- | --- | --- | --- | --- |
| 0 | 1 | `alpha` | `0xFF` when textured; **no assertion** otherwise | 255 / any / 0 |
| 1 | 1 | `flags` | `MaterialFlags`, five bits (§ below) | — |
| 2 | 2 | `rgb` | `0x7FFF` when textured, `0x0000` otherwise | 32767 / 0 / 0 |
| 4 | 12 | `color` | 3 × `f32`; `WHITE_FULL` (255,255,255) when textured, `BLACK` for a zero slot, **unconstrained** otherwise | — |
| 16 | 4 | `index` | **texture index in a GameZ**, `0` otherwise. The source's own field comment: "ptr in mechlib, texture index in gamez" | 0–654 / 0 / 0 |
| 20 | 4 | `zero20` (`f32`) | `0.0` | 0.0 everywhere |
| 24 | 4 | `half24` (`f32`) | `0.5` for a present record, `0.0` for a zero slot | — |
| 28 | 4 | `half28` (`f32`) | as above | — |
| 32 | 4 | `specular` (`f32`) | **not constrained by the source** | 1–6 distinct values per archive |
| 36 | 4 | `cycle_ptr` | `0` unless the cycled flag is set. Raw, never followed. | 0 / 0 / 0 |

**Offset 32 is the field spec F10 non-negotiable #5 is about.** The pinned
reference calls it `specular`; newer classification of the same field calls it
soil, and the two readings contradict each other. Nothing read here decides
between them: the field is `RawMaterialRecord::field32`, a raw `f32` whose bits
are untouched, and the audit never looks at it. It is **not** read as
specularity, and it is **not** read as soil.

### Material flags — one byte, `materials/mod.rs:101-108`

| Bit | Name | Source's use | Retail |
| --- | --- | --- | --- |
| 0 | `TEXTURED` | all colour comes from the texture, and the record names one | 4 447 materials |
| 1 | `UNKNOWN` | handed on as `TexturedMaterial::flag`; the source names no meaning | 215 materials |
| 2 | `CYCLED` | **cycle data is stored after the whole array** — the one bit that changes a length | 627 materials |
| 3 | — | not named by the source | never occurs |
| 4 | `ALWAYS` | asserted **set** for every present CS material, **clear** for every Recoil one | set on all 4 669 |
| 5 | `FREE` | asserted clear on a present material; a zero slot's record has **only** this bit | clear on all 4 669; set on 5 331 zero slots |

Bit 3 is the one bit inside the byte that the source's `MaterialFlags` does not
name, and it never occurs: `bits & 0x08` is zero in all 4 669 present records and
in all 5 331 zero slots. Bit 6 and 7 are outside the byte.

### The two-pass material walk, and the two link-word rules

`read_materials` (`materials/read_multi.rs:10-59`):

1. the 16-byte header;
2. `count` present slots, each a 40-byte record followed by **two `i16` link
   words**;
3. slots `count .. 1000`, each also a 40-byte record plus two link words, every
   byte of the record zero except the free flag;
4. then, **after the whole array**, one `CycleInfoC` plus `count1` `u32` frames
   for every material whose cycled flag is set, in material order.

The two link-word rules are **not the same**, and the corpus confirms both:

| Region | first link word | second link word | Source |
| --- | --- | --- | --- |
| a present slot at `i` | `i + 1`, or `-1` at the last one | `i - 1`, or `-1` at `i == 0` | `read_multi.rs:31-43` |
| a zero slot at `i` (from `count`) | `i - 1`, or `-1` at the first one | `i + 1`, or `-1` at the last one | `read_multi.rs:86-98` |

The reference is the only source for this asymmetry and the corpus confirms it:
every present slot and every zero slot of all nine archives stores exactly the
pair its own rule gives. Nothing in the bytes says what the words *mean*; they
are kept raw as `RawMaterial::link1`/`link2`.

### Cycle record — 28-byte header plus frames, `materials/mod.rs:74-83`

| Offset | Size | Field | Source's constraint | Retail |
| --- | --- | --- | --- | --- |
| 0 | 4 | `unk00` | non-zero | non-zero in 627/627 |
| 4 | 4 | `unk04` | not interpreted | 2 and 7 occur |
| 8 | 4 | `zero08` | zero | 0 in 627/627 |
| 12 | 4 | `unk12` (`f32`) | `0.0 <= unk12 <= 16.0` for CS ("in MW: 2.0..=16.0") | inside |
| 16 | 4 | `count1` | frames stored; asserted `== count2` | 2–4 |
| 20 | 4 | `count2` | asserted `== count1` | equal |
| 24 | 4 | `data_ptr` | non-zero. Raw, never followed. | non-zero in 627/627 |

Each frame is a `u32` **texture index** into the container's texture-name table
(`read_single.rs:158-165`), exactly like a material's offset-16 word. All 1 000+
frames of the corpus name a table entry the container has.

### How a material names its texture

This is the whole point of the task, and it is one sentence in the source's own
code: `read_cycle` takes `mat.pointer` — the value at record offset 16 — and
indexes `textures` with it (`materials/read_single.rs:124-126`), after asserting
`texture_index < textures.len()`. So:

> **A GameZ material record stores no name. It stores a `u32` at offset 16, which
> is an index into the container's texture-name table at `textures_offset`.**

`GameZMaterials::texture_of` is exactly that lookup, and the reference's
`material_index < material_count` assertion is `GameZMaterials::material`
returning `None` — offered to a caller rather than enforced, so an out-of-range
index is *reported* (F10-B deferred item 1) instead of aborting a read.

## Files and the one observable failure (listed before editing)

- `crates/cs_formats/src/gamez/materials.rs` (new, ~1 100 lines): the layout
  constants (`TEXTURE_INFO_BYTES`, `TEXTURE_NAME_BYTES`, `MATERIAL_HEADER_BYTES`,
  `MATERIAL_RECORD_BYTES`, `MATERIAL_LINK_BYTES`, `MATERIAL_SLOT_BYTES`,
  `CYCLE_HEADER_BYTES`, `CYCLE_FRAME_BYTES`, `NG_MATERIAL_SLOTS`, the five
  `MATERIAL_FLAG_*`, `KNOWN_MATERIAL_FLAGS`), `TextureNameEncoding`,
  `GameZTextureName`, `MaterialKind`, `RawMaterialRecord`, `RawMaterial`,
  `RawCycle`, `MaterialInfo`, `MaterialFinding`, `GameZMaterials`,
  `GameZMaterialError`, `MATERIALS_ENTRYPOINT` and `read_gamez_materials`.
- `crates/cs_formats/src/gamez/reader.rs`: **one line of wiring** — the private
  `read_header` becomes `pub(crate) read_container_header`, so both entrypoints
  read and validate the header through the same code instead of two copies of
  it.
- `crates/cs_formats/src/gamez/mod.rs` (wiring): `pub mod materials;`,
  re-exports, three module-doc paragraphs.
- `crates/cs_formats/tests/gamez/materials.rs` (new): the fixture writer and ten
  `accept_f10_c_02_` tests, one of them retail.
- `crates/cs_formats/tests/gamez/main.rs` (wiring): one `mod materials;` and two
  doc sentences.
- `crates/cs_content/src/mesh.rs`: `MaterialState`, `MaterialRow`,
  `MaterialReference`, `MaterialUse`, `DependencyReadiness`, `DependencyContext`,
  `MeshDependencyAudit`, `MATERIAL_KIND`, `MATERIAL_CONSUMER`, the nine
  `accept_f10_c_02_` tests (one retail), and the module doc.
- `docs/findings/2026-09-29-f10-c-02-gamez-material-records.md` (this file),
  `docs/findings/evidence/F10-C.02.json`,
  `crates/cs_formats/tests/evidence_report_f10_c_02.rs` (evidence harness).

**One observable failure:** a reader that reads the texture-name record's words
in the order the *struct literal* suggests rather than the order the bytes are in
— `field00, field04, field08, used, index, unk40` with the name read last, which
is exactly what this task's first draft did — reads `Sky1.tif` as `pock1.tif`,
because the name field starts at offset 12 and not after `unk40`. Every entry
then fails to decode and **no** container parses.
`accept_f10_c_02_a_material_names_its_texture_by_table_index` fails on the first
fixture, and `..._retail_every_archive_lands_on_its_meshes_offset` fails on
`planes.zbd` with `texture 0 name byte 16 is 0xFF, not ASCII`. The other
observable failure is the one the layout is most exposed to: a reader that sizes
the material array from `array_size` or from `count` instead of walking 1 000
slots ends 44 bytes per missing slot away from `meshes_offset` and is refused
with both numbers — on a synthetic fixture that is one material versus two.

## Design decisions

- **Two entrypoints, one header.** `read_gamez_materials` re-reads the header
  rather than taking one from the mesh reader. A caller that wants both sections
  reads the container twice; in exchange, each reader proves **its own** section
  boundary on its own, and neither can be used with a header the other did not
  validate.
- **Walk the zero slots; do not skip them.** The zero region's length is what
  makes the cycle data's offset knowable, so it is read, every one of its bytes
  is checked against the all-zero record, and its link words are checked against
  their own (opposite-direction) rule. A non-zero slot there is a
  `MaterialFinding`, not a refusal: it changes no length, so the section boundary
  still decides whether the container was really read.
- **Value assertions become findings, not errors.** Every assertion the reference
  makes about a *value* — `alpha`, `rgb`, `color`, `zero20`, `half24`, `half28`,
  `ALWAYS`, `FREE`, the zero slot's every byte, the cycle record's fields — is
  checked and **reported**, and the record is still read, because such a value
  changes no stored length and refusing it would throw away a dependency the
  audit has to be able to see. `GameZMaterials::findings` is empty for all nine
  measured archives, which is the strongest statement this reader makes about
  them. The one exception is the name decoding, which the reference *refuses*:
  a field with no terminator, a byte with the high bit set, or junk where the
  padding must be, is an **error**, because the decoded name is what the rest of
  the system consumes.
- **The assertion list is the reference's, not a tidy one.** The reference asserts
  nothing about `alpha` or `color` for an *untextured* record, and the corpus
  stores `0xFF` there. An earlier revision of this task asserted `alpha == 0` for
  untextured records and produced 18 spurious findings on `planes.zbd` alone; the
  assertion was deleted, not the finding threshold. The retail test found it.
- **The zero-slot link rule is the reference's, measured.** It runs the opposite
  way from the present slots'. That looks like a bug in the source and is not:
  all 5 331 zero slots of all nine archives store exactly the pair its own rule
  gives. What the words *mean* is not established and is not claimed.
- **A material's texture index is a lookup, not a resolution.** `texture_of`
  returns the table entry or `None`; it does not decide what the name *is*. The
  audit is the only place a name becomes an origin, and it does that through the
  caller's archive with no stripping, no alias and no fallback. The one
  relaxation that later turned out to be real is the **ASCII case fold** of the
  request, measured at `0x531930` and adopted by task #689: see
  `docs/findings/2026-10-06-t689-texture-name-case-fold.md`. Every count in
  this file was measured **before** that fold and is unchanged by it, which is
  itself measured there: the fold newly resolves 0 of the 3 521 distinct names
  the world's textured materials use, because each name that differs in case
  also carries an extension the archive does not store.
- **`array_size` is carried, not obeyed.** The reference asserts `array_size <=
  1000` and then always reads 1 000 slots; this reader does the same and says so
  in the constant's documentation. In the corpus the two are equal, so the corpus
  cannot decide the question, and the reader does not pretend it can.
- **Bounded by the F03 budget.** The texture table's stored extent, the vector it
  becomes and the name bytes it owns are three separate reservations; the
  material array is one reservation for all 1 000 slots so a hostile `count`
  cannot turn the walk into a per-slot allocation; the cycle frames are reserved
  before they are collected; and a failed attempt leaves the ledger as it was, so
  the same context can retry the bytes honestly.

## The dependency audit, and what it found on the installation

`MeshDependencyAudit` (`crates/cs_content/src/mesh.rs`) resolves, for every
distinct stored material index a container's meshes reference at **both** levels
(the mesh record's own 12-byte reference list and every stored polygon material
group):

1. the index to a material record, or `material_index_out_of_range` — reported,
   never clamped to the last record and never wrapped;
2. the record to its texture, or `untextured` (a flat colour, which is
   **complete**, not blocked), `unknown_field` (an unmapped flag bit, so even the
   textured/untextured distinction is not established) or
   `texture_index_out_of_range` (the container does not store that texture);
3. that name to exactly one stored texture in **the caller's** archive, or
   `missing_texture` with the exact stored name and the exact archive,
   `duplicate_texture` with both entry indices, or `archive_unavailable`.

Every row is kept, failures included, and each carries the contract's
`id`, `kind`, `origin`, `dependencies`, `parse_state`, `normalize_state`,
`runtime_consumers`, `readiness`, `unsupported_reasons` and `fingerprint` — the
last a SHA-256 over exactly the 44 stored bytes of the slot, so two materials
that differ in one word have different fingerprints.

**F10-B's deferred item 1 is closed.** Every stored material index of every mesh
in all nine archives is inside its container's own material table: the
production mesh reader's *unchecked* references — 19 810 in `planes.zbd`,
142 841 across the eight world archives, 162 651 in all nine — are now checked
references, and both retail tests assert
`audit.out_of_range().count() == 0` and
`audit.references == meshes.unchecked_material_references` for every archive.
**What the audit reports on the installation, per archive**, from
`material-corpus.json` (the evidence artifact, private copy):

| Archive | stored material references | audit rows | `resolved` | `missing_texture` | `untextured` | out of range |
| --- | --- | --- | --- | --- | --- | --- |
| `ZBD/C1` | 22 678 | 561 | 0 | 552 | 9 | 0 |
| `ZBD/C1B` | 11 169 | 314 | 0 | 310 | 4 | 0 |
| `ZBD/C1C` | 10 173 | 279 | 0 | 275 | 4 | 0 |
| `ZBD/C2` | 16 202 | 488 | 5 | 475 | 8 | 0 |
| `ZBD/C2B` | 9 336 | 271 | 0 | 267 | 4 | 0 |
| `ZBD/C3` | 20 506 | 441 | 5 | 429 | 7 | 0 |
| `ZBD/C4` | 24 381 | 627 | 0 | 618 | 9 | 0 |
| `ZBD/C5` | 28 396 | 562 | 0 | 555 | 7 | 0 |
| **total** | **142 841** | **3 543** | **10** | **3 481** | **62** | **0** |

`planes.zbd` is counted but **not** audited: it is the shared airframe library,
has no world of its own, and which archive its materials resolve against is not
established (deferred item 3 below). Its 19 810 stored references are inside its own 954-record material table,
like every other archive's, and it contributes 954 materials and 298 texture
names to the corpus totals above.

### The name rule resolves 10 of 3 543 rows, and that is the finding

The discriminating acceptance case is the rule working as written, and on the
installation it produces a large number of `missing_texture` rows. The reason is
measurable and is **not** a defect in the audit. (This section was written when
the rule was byte equality; task #689 replaced byte equality with the measured
ASCII case fold of the request, which leaves **every number below unchanged** —
measured, not assumed: the fold newly resolves none of the 3 521 distinct names
the world's textured materials use.)

- a GameZ container spells a texture with an extension and in mixed case —
  `Sky1.tif`, `SPACE.tif`, `A.tif`…`Z.tif`, `pass_Sparks.tif`,
  `sacredtrust_logo1.TI`;
- the world's own ZBD texture archive stores the same texture **without an
  extension and in lower case** — `sky1`, `space`, `a`…`z`, `pass_sparks`,
  `ilsafas`. The ZBD package's name field is a plain NUL-padded 32-byte ASCII
  name with no suffix run at all (F08-B.02's `str_from_c_padded`).

Comparing the two name sets over the whole corpus, for every name a textured
material of a world archive uses:

| World archive | distinct names used | exact matches | match after dropping the extension | match after dropping it **and** folding case | no match at all |
| --- | --- | --- | --- | --- | --- |
| `ZBD/C1` | 551 | 0 | 514 | 549 | 2 |
| `ZBD/C1B` | 309 | 0 | 270 | 307 | 2 |
| `ZBD/C1C` | 275 | 0 | 237 | 272 | 3 |
| `ZBD/C2` | 479 | 5 | 436 | 472 | 2 |
| `ZBD/C2B` | 266 | 0 | 228 | 264 | 2 |
| `ZBD/C3` | 443 | 5 | 399 | 434 | 4 |
| `ZBD/C4` | 635 | 0 | 598 | 632 | 3 |
| `ZBD/C5` | 563 | 0 | 526 | 560 | 3 |
| **sum** | **3 521** | **10** | **3 208** | **3 426** | **21** |

The two middle columns count names that do **not** match exactly and match only
under that relaxation, so each excludes the exact column. The three counting
columns are therefore a partition of the 3 521 per-world distinct names once the
exact column is added back: **10** match exactly, **3 208** match after the
extension is dropped, a further **282** match only after the case is folded as
well (3 208 + 282 + 10 = 3 500), and **21** match nothing. Over the union of the
eight worlds' name sets the same corpus has **1 328** distinct names, of which
**1 316** match under the relaxed rule and **5** match exactly.

The five exact matches in `C2` and `C3` are the handful of names the container
stores with no extension at all (`lflare1`, `lightmap`, …), which are already
lower case. The handful that match **nothing** are `pir_spinner.tif` and
`snow16x16.tif` in every world, plus `c1c.jpg`, `cloud1.tif`, `cloud2.tif`,
`canopycorner.tif` and `barngrill.tif` in one world each — absent from that
world's texture archives under any spelling this audit is allowed to try.

So: **a case-insensitive, extension-insensitive match would resolve 549 of 551
of `C1`'s names, and the name rule the task specifies resolves none of
`C1`'s.** Over the eight world archives the audit produces **3 543** rows and
**10** of them resolve — the ten are `C2`'s and `C3`'s five extension-less
lower-case names each. The other **3 481** are `missing_texture`. Both the
per-world and the union figures are given above so that neither can be quoted
without the other.

**What the ASCII fold added to this, measured in #689: nothing.** Case folding
alone is not enough to reach any of these names, because the container's name
carries an extension and the archive stores a bare stem: `Sky1.tif` folds to
`sky1.tif`, not `sky1`. Resolving these rows needs the extension stripped,
which is the separate relaxation the table above counts and the audit does not
perform. So the 10-of-3 543 figure is the same figure under the exact rule and
under the measured fold.

The task's rule is not relaxed here to make that number smaller, and no alias
record is invented. The retail acceptance test asserts the rule on real data:
`Sky1.tif` is `missing_texture` with the archive named, *and* the world's own
archive is shown to store `sky1` and to resolve it. That pair of assertions is
what distinguishes "a naming difference" from "a missing archive", and it is the
honest report either way. `material-corpus.json` carries four such
`naming_examples` per archive — the container's spelling beside the archive's —
so the claim is checkable from the artifact without the artifact holding a
single pixel.

## Test inventory (`accept_f10_c_02_*`)

`crates/cs_formats/tests/gamez/materials.rs` (10) and
`crates/cs_content/src/mesh.rs` (9). Every one calls the production reader, the
production audit or the production `TextureCatalog`; the synthetic fixtures are
authored in the test files, and the fixture writers share no code with the
readers they feed (the texture-archive writer spells the 24/40/16-byte layout
constants out rather than importing the reader's).

| Test | Covers |
| --- | --- |
| `..._a_material_names_its_texture_by_table_index` | the binding itself: three name encodings decode to the stored names, and each material's stored **index** picks its name; an untextured record names nothing and is not a failure |
| `..._record_words_survive_in_their_own_slots` | all ten material words and all six texture-record words in their own slots, two materials differing in every field, the `specular`/soil word kept raw, the extension's case stored, and the **exact** finding list for a record deliberately outside the profile |
| `..._the_array_is_a_thousand_slots_and_the_walk_ends_on_meshes_offset` | the array is 1 000 slots however few are present, so one more present material does **not** move the boundary; the section is 16 + 44 × 1 000 exactly |
| `..._cycle_data_is_stored_after_the_whole_array` | the cycle records come after the whole array, in material order, one per cycled material, and a container with two still lands on `meshes_offset` |
| `..._link_words_run_in_opposite_directions_in_the_two_halves` | the present slots' forwards/backwards pairs and the zero region's opposite pairs, with `-1` terminating each run |
| `..._out_of_profile_values_are_findings_not_failures` | an unmapped flag bit, five wrong value fields and both link words wrong: every finding is raised, each names its slot and field, the record is still read with its raw words, the section still ends on `meshes_offset`, and a non-zero **zero slot** is a finding too |
| `..._duplicate_container_names_are_reported_with_every_index` | the 36-fold `bldhwk_cowling..tif` case: the name is reported as the field holds it and every table index with it, and no material's name is rewritten |
| `..._contradictions_are_named_with_both_numbers` | `index_max`/`index_last` against `count` (both numbers), `count > array_size`, a name field with no terminator, with a high bit, with junk padding, `texture_count` at the reference's bound, and a non-container — seven variants |
| `..._the_section_boundary_fails_loudly_and_is_retryable` | a material naming a texture the container does not have, four truncations, and the allocation ledger left unchanged so the same context then reads the good bytes |
| `..._retail_every_archive_lands_on_its_meshes_offset` (retail) | all nine archives: the walk ends on `meshes_offset`, `materials_offset == textures_offset + 44 × texture_count`, the recorded texture and material counts (3 985 and 4 669 in total), every stored texture index inside the container's own table, **zero** findings, every stored stem a prefix of its stored name, and the evidence class |
| `..._audit_resolves_a_material_to_exactly_one_stored_texture` | the resolved row in full: the `TextureId` with its archive, entry and name, the row's ten catalog fields, and both stored references named at their own levels |
| `..._audit_reports_a_missing_texture_with_its_exact_name_and_archive` | **the discriminating case**, with a *second archive of the same world* holding `sky1.tif` — the spelling `Sky1.tif` folds to, so a fallback would succeed and must not be taken — plus `Sky1.tif` verbatim and `sky1`: the row is `missing_texture` naming the exact name and archive, the dependency list has one entry, every spelling of the request resolves to the stored `sky1.tif` rather than the stored `Sky1.tif`, and the world's own archive is shown to store none of the three |
| `..._audit_neither_folds_case_nor_strips_an_extension` | `Sky1.tif` is missing, `sky1` and `ground` resolve, in the same audit. Since #689 the request *is* case-folded, and this is the case that says the fold does not rescue it: `Sky1.tif` folds to `sky1.tif` and the archive stores the bare stem `sky1` |
| `..._audit_reports_a_material_index_past_the_table_and_never_clamps` | index 7 of 3 records is reported with `fingerprint: None` and no record, the last real record is **not** what it resolved to, and its three references are named |
| `..._audit_keeps_untextured_and_unknown_field_rows` | `untextured` is **complete** (ready, parsed, no reasons) with its flat colour uninterpreted, and `unknown_field` is blocked |
| `..._audit_reports_dangling_archive_and_duplicate_dependencies` | `texture_index_out_of_range`, `duplicate_texture` with both entry indices, and `archive_unavailable` for an archive the catalog does not hold — and a container-side defect still reported as itself |
| `..._audit_reports_a_container_duplicate_as_a_reason` | the container's own duplicate name is a **reason** on the row, not a second state, and the material's stored name is unchanged |
| `..._audit_keeps_the_raw_record_and_hashes_exactly_its_stored_bytes` | the raw record on the row, one word different ⇒ one fingerprint different, and the audit is a function of the bytes |
| `..._audit_retail_world_resolves_no_name_by_substitution` (retail) | `ZBD/C1/gamez.zbd` and `ZBD/C1/texture.zbd` through the production readers: both section boundaries proved, zero out-of-range material indices, `references == unchecked_material_references`, every resolved row names the world's own archive and the container's own stored name, `Sky1.tif` is `missing_texture` while the archive is shown to store `sky1`, and no row resolved to a name its archive does not store |

## Mutation probes

Every mutation below was **applied, run and restored**; the counts are what the
runs reported, not what should have happened. Two selections, both with
`--include-ignored` and `CS_GAME_DIR` set: the ten `cs_formats` task tests
(`cargo test -p cs_formats --test gamez -- accept_f10_c_02_ --include-ignored`)
and the nine `cs_content` task tests
(`cargo test -p cs_content --lib -- accept_f10_c_02_ --include-ignored`). Each
mutation is a single textual change to a production file; the mutation script is
not committed, and the file is restored from git after every run.

### The reader, `crates/cs_formats/src/gamez/materials.rs`

| Mutation | Failing |
| --- | --- |
| the name field read after `unk40` instead of at offset 12 (the first draft's bug) | 10 — every test, retail included |
| the stored `.` of a full name field is not restored | 1 |
| the full-field name shape is read as a bare stem | 8 |
| the material array is sized from `count` instead of a thousand slots | 9 |
| the zero slots use the present slots' link rule | 4 |
| the name field's non-ASCII check is narrowed | 10 |
| the word the reference calls `specular` and newer source calls soil is dropped | 9 |
| the material's texture index is read as a constant | 9 |
| `index_max`/`index_last` are not cross-checked against `count` | 1 |
| the `texture_count` bound is not checked | 1 |
| the name field's padding check is removed | 1 |
| the duplicate-name groups keep every name | 1 |
| the cycle header's `count1` and `count2` reads are swapped | **0 — equivalent** |
| the material's texture index read under the cycle pointer's label | **0 — equivalent** |
| the texture table's vector capacity zeroed | **0 — equivalent** |

### The audit, `crates/cs_content/src/mesh.rs`

| Mutation | Failing |
| --- | --- |
| a material index past the table is clamped to the last record | 1 |
| the container's spelling is normalised (case folded, extension dropped) before the lookup | 3, retail included |
| the mesh-level material reference list is not counted | 2, retail included |
| a row that did not resolve is dropped from the audit | 7, retail included |
| untextured is treated as a blocked row | 1 |
| the container's duplicate name is not reported as a reason | 1 |
| an unknown flag bit does not block the row | 1 |
| an untextured material is treated as naming a texture | 1 (in `cs_formats`' selection, where the binding itself is pinned) |

### The three survivors are equivalent mutations, not holes

Recorded so that a later reader does not re-run them:

- **Swapping the `count1` and `count2` reads.** The reader asserts
  `count1 == count2` — the reference's own assertion — and reads the frames with
  `count1`, so which of the two equal words is called "count1" is unobservable by
  construction. The check that `count1 == count2` is pinned instead, by the
  reader raising `cycle_field` for a disagreeing pair.
- **Reading the material's texture index under the cycle pointer's label.** The
  reads are positional: a struct literal's field order *is* the read order, so
  changing the label string of one positional read changes only the error
  message. (The F10-B reviewer found the same class of no-op in the mesh reader.)
  What the mutation *did* reveal is that nothing asserted the material's
  `cycle_ptr` value; the cycle test now pins `0x1111_1111` and `0x2222_2222` on
  the two cycled fixtures and `0` on the uncycled one, and a **separate**
  mutation that reads the texture index from the wrong *offset* is killed by that
  same assertion.
- **Zeroing the texture table's vector capacity.** The loop is driven by
  `header.texture_count`, not by the reserved capacity, so the capacity is an
  allocation hint and nothing observable. It is kept because F03 asks the ledger
  to describe every buffer a parse hands out, not only the ones that could be
  large — the same reasoning F10-B recorded for its two equivalent survivors.

### Two fixture lessons, of the kind F10-B's reviewer found

- **The first version of the `cs_content` fixture wrote the per-texture info
  block as six `u32` words** instead of `u32,u16,u16,u32,u16,u16`, so every
  audit test failed with "the fixture archive opens: 1 failure" rather than
  testing the audit at all. The layout's numbers are now spelled out in the test
  file so the writer cannot borrow the reader's constants.
- **`array_size == count` in the whole measured corpus**, so **no retail test can
  distinguish a reader that walks 1 000 slots from one that walks `array_size`
  slots**. Only a fixture can, which is why
  `..._the_array_is_a_thousand_slots_and_the_walk_ends_on_meshes_offset` asserts
  the *absence* of a boundary change when a second material is added, and why
  the `count`-sized mutation is killed by a synthetic test alone. This is the
  same class of gap F10-B's reviewer closed for the mesh reader's two adjacent
  `Vec3` arrays, and the lesson is the same: **a field the retail corpus never
  varies must be covered by a fixture or not covered at all.**

## Recorded unknowns

- **What the field at material offset 32 is.** The pinned reference calls it
  `specular`; newer classification calls it soil. It is stored raw as
  `RawMaterialRecord::field32` and nothing here reads it as either (§ "Material
  record").
- **Whether the original engine matched a texture name with or without its
  extension, and with or without case.** The corpus shows the container's spelling
  and the archive's spelling differ for 549 of 551 of `C1`'s names, and that an
  extension- and case-insensitive match would resolve nearly all of them. Which
  rule the original engine used is **not** established by anything read here, and
  no alias record is invented to make the audit's numbers look better (§ "The
  exact-name rule").
- **The seven names that match nothing.** `pir_spinner.tif` and
  `snow16x16.tif` in every world, plus `c1c.jpg`, `cloud1.tif`, `cloud2.tif`,
  `canopycorner.tif` and `barngrill.tif`: absent from that world's texture
  archives under every spelling this audit may try. Whether the original engine
  found them elsewhere — a different archive, the `rimage.zbd` UI set, a
  built-in resource — is not established.
- **Which archive a container's materials resolve against.** `planes.zbd` is the
  shared airframe library and has no world of its own: its 221 distinct texture
  names match 0 of `ZBD/rimage.zbd`'s 254 (which is the **UI/HUD** set: `arrow`,
  `escape_button1`, `ha-m1map`, …) and 260 of `ZBD/C1/texture.zbd`'s 881 after
  dropping the extension and folding case. The audit takes the archive from its
  caller and never picks one, which is why this is a question for the consumer
  that knows the world (F08-C records the same question).
- **Every other `unk` field.** `unk00` on a texture record, `TextureInfoNgC`'s
  `zero04`/`zero08`/`index`/`unk40`, the material record's `alpha` for an
  untextured record, its `rgb`, `color`, `zero20`, `half24`, `half28` and
  `cycle_ptr`, every cycle field but `count1`'s role, and both link words, are
  stored raw and uninterpreted. What the link words *are* (a doubly-linked list
  of material slots is a guess, not a measurement) is unknown.
- **`MaterialFlags::UNKNOWN` (bit 1).** 215 present materials set it and the
  source hands it to the consumer as `TexturedMaterial::flag` with no meaning.
  It is kept raw.
- **The cycled frame list's meaning.** 627 materials carry 2–4 frames; the corpus
  stores the frames, the reference reads them, and neither says when the original
  engine advanced a frame or what `unk12` scales.
- **The two `Ptr`-like words.** `TextureInfoNgC.unk00` and `MaterialC.cycle_ptr`
  are read, kept and never followed, exactly as F10-B treated its own.
- **Original-run behaviour of any kind.** No original game was run for this task.
  `retail` file access is not evidence of runtime behaviour, so every layout
  claim here is `ObservedTool` and no `verified_original` or `release_approved`
  claim is made or supported.

## Deferred scope, its resolving task and what it gates

The acceptance report's `unknowns` array is **empty**, and that is a statement,
not an omission: everything this task could not resolve is a named boundary with a
resolving task, written down here so it survives a parent being marked done.
None of them is an unresolved issue with the claim the report makes — the layout
is established from the pinned reference and the material section ends exactly on
`meshes_offset` in all nine retail archives.

| # | Deferred item | Affected content | Resolving task | Gates |
| --- | --- | --- | --- | --- |
| 1 | **The name-matching rule between a GameZ container and a texture archive.** No case folding, no extension stripping, no alias, no second archive, as the task specifies. On the installation that resolves **10 of the 3 543** audited material rows — 10 of the 3 521 distinct names counted once per world, or 5 of the 1 328 distinct names over the union of the eight worlds' name sets, because `C2`'s five and `C3`'s five are the same five names. An extension- and case-insensitive match would resolve **3 500 of the 3 521**, i.e. every one but the 21 that are absent under any spelling | every textured material of all eight world archives, and the 221 `planes.zbd` names | **F10-C.03** (#366) for the upload path's binding; the **owner** for the rule itself, which is a claim about what the original engine did | any claim that a rendered surface shows the *right* texture; the F10-C upload path may not substitute |
| 2 | **Names truncated by the 20-byte field** (`blo_fusalagebottom.ti`, `tracer_armorpierce.t`, `buildingspotlighted.`) and the seven names absent from every world archive | 3 truncated names in every world, plus `pir_spinner.tif`, `snow16x16.tif`, `c1c.jpg`, `cloud1.tif`, `cloud2.tif`, `canopycorner.tif`, `barngrill.tif` | **F10-D** (#46, retail) for the private corpus; the **owner** for whether the original engine matched by prefix | any claim that the audited texture set is complete |
| 3 | **Which archive a container resolves against.** `planes.zbd` is a shared airframe library with no world; its names are absent from `rimage.zbd` (the UI set) and mostly present in a world's own `texture.zbd` | all 954 materials of `planes.zbd`, i.e. every airframe | **F10-C.03** (#366) and the consumer that knows the world | any claim about airframe materials; the audit's `archive` is the caller's for exactly this reason |
| 4 | **Which of `texture.zbd`, `rtexture2/4/6/8/11/12/14/15.zbd` and `rimage.zbd` a mission uses, and when the resolution tier is chosen.** All the tiers of one world store the same 881 names, so the choice is invisible to a name lookup | every world, every tier | not scheduled; F08-C records the same open question | any claim that a mission's texture set is the right one |
| 5 | **The meaning of the field at material offset 32** (`specular` in the pinned source, soil in newer classification) and of `MaterialFlags::UNKNOWN` (215 materials) | 4 669 present material records | not scheduled; F10-D or F18 where the field turns out to be needed | any claim that a material's appearance is reproduced; nothing may read it as specularity or as soil (F10 non-negotiable #5) |
| 6 | **The link words, the `unk` fields, the never-followed pointers and the cycle frames' meaning** | every material and cycle record of all nine archives | not scheduled | nothing currently — they are stored raw and no claim rests on them |
| 7 | **Original-run behaviour of any kind** | the whole material and texture dependency chain | the owner (`human_play` / `human_review`) | every `verified_original` and `release_approved` claim; this report's layout class is `ObservedTool` |

## Review note (bunny-2, review of #365)

The reviewer re-measured the naming table of § "The exact-name rule" from the
installation with a throwaway Python probe written against the worksheet, sharing
no code with the Rust reader, and reproduced **all eight rows and all five
columns** of it. Two numbers that the probe could not reproduce were corrected:

- the first draft of deferred item 1 claimed a relaxed match "would resolve
  2 510 of the 2 271 distinct world-archive names", which is impossible as
  written (more resolved than total). Measured: **3 500 of 3 521** distinct
  names summed per world, or **1 316 of 1 328** over the union of the eight
  worlds' name sets;
- the committed evidence report's `review.method` said the exact rule "resolves
  5 of 2271 distinct world-archive material names". The `5` is right only over
  the union of the eight worlds' name sets; per world it is 10, and the
  denominator is 3 521, not 2 271. Both readings are now stated side by side so
  neither can be quoted alone.

The per-world exact-match total of 10 and the `1 328`/`5` union total are both
real and measure different things, which is why the two figures survived the
original writing. A separate code comment in
`crates/cs_formats/tests/gamez/materials.rs` claimed 41 completely full name
fields in `planes.zbd`; the probe counts **5** there and 95 over the nine
archives, which is the figure the finding and the reader's own doc already gave.
That comment is now correct.

Three names in § "The exact-name rule" were also near-misses of real stored
names rather than measurements, and are corrected: the container stores
`SPACE.tif`, not `Space.tif`; `pass_Sparks.tif`, not `Pass_Sparks.tif`; and
`sacredtrust_logo1.TI`, not `sacredtruss_logo1.TI` (the archive side is
`sacredtrust_logo1`, with one `s` in *sacredtrust*). The point each name was
making still holds — the corpus does store an extension in mixed case, and it
does contain exactly one `TI` — but a reader checking the list would have found
three of six entries absent from all nine containers. That paragraph now also
carries the measured extension histogram over the nine containers' 1 476
distinct names in place of the vaguer "the corpus contains `tif`, `TIF`, `jpg`
and a truncated `TI`". The archive-side name `ilsafas` in the same list is
correct and was left alone; the container spells it `IlsaFas.tif`.

No layout claim changed: the material walk still lands on `meshes_offset` in all
nine archives, `materials_offset == textures_offset + 44 × texture_count` in all
nine, and the production reader still reports zero findings over all nine.

## What F10-C.03 and F10-D need from this
- **F10-C.03** (the upload path) has a real producer for both halves of the
  binding: `read_gamez_meshes` gives the mesh material references at both levels,
  `read_gamez_materials` gives the material records and the texture names, and
  `MeshDependencyAudit` turns one archive key into a list of rows each with an
  exact origin or a named reason. A mesh that binds its materials at upload time
  should consume `MaterialRow::state` and refuse anything that is not
  `is_complete()`, so a `missing_texture` row can never become a default
  material.
- **F10-D** has the per-airframe and per-world numbers: the material counts, the
  textured/untextured split, the cycled split, the finding count (zero over the
  public corpus) and the texture-name counts of § "The dependency audit" are the
  shape its AC04 report aggregates, and the naming-rule question in deferred
  items 1 and 2 is the largest open item it will meet on the private corpus.
