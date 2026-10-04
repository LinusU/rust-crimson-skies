# M01-LC-ANIM-CARRIERS: what `mis_anim.zbd` / `cam_anim.zbd` actually carry

Date: 2026-10-04. Task: #633 (`M01-LC-ANIM-CARRIERS`). Capability used:
**`retail`** (read-only access to `$CS_GAME_DIR`) and ordinary build/test. No
original run happened; nothing here is `verified_original`, and nothing here
says what the 2000 engine did with any of it.

Follows `2026-10-02-f20-d-animation-family-validation.md` (task #468), which
walked the two mission-critical animation families, validated all 61 carriers
through the production dispatch and fingerprinted them — and read **no payload**.
`VS-M01-RUNTIME` (#359) needs that payload, so this task measures what is in it
and what binds to it.

## Files

- `crates/cs_formats/src/zbd/anim.rs` (new): the family's **own** front index —
  `read_animation_index`, `AnimationIndex`, `AnimationRow`,
  `AnimationPayloadHeader`, `AnimationPayload`, `AnimationIndexError`,
  `AnimationRowAnomaly`. Wiring: `crates/cs_formats/src/zbd/mod.rs`
  (`pub mod anim;`, the re-exports, one doc paragraph).
- `crates/cs_app/src/animation/carrier.rs` (new): the binding —
  `bind_animation_carrier`, `survey_animation_bindings`, `bind_installation`,
  `CarrierBinding`, `CarrierMember`, `AnimationDocument`, `AnimationReference`,
  `UnresolvedReference`, `StartupIdentities`, `BindingBlocker`. Every row keeps
  the format reader's `AnimationRowAnomaly` list in `CarrierMember::anomalies`,
  so the lossy path decode cannot drop the reader's refusals. Wiring:
  `crates/cs_app/src/animation/mod.rs`.
- `crates/cs_app/tests/accept_f20_d_validation.rs`: the `accept_m01_lc_anim_carriers_`
  section (9 tests: 8 synthetic, 1 retail). It lives in F20-D's existing test
  binary rather than a new one: every CI run that added a test binary at this
  crate's tests root died on the runner's disk (task #637), and `specs/F20-*`
  already owns this file.

## These are not reader archives

`cs_formats::zbd::trailer` reads a version-one member table out of the **last
eight bytes** of a sound or reader archive. None of the 61 animation containers
has one, so `read_version_one_index` refuses all of them, and this task's
`read_animation_index` refuses a reader archive by name. Both directions are
tested (`accept_m01_lc_anim_carriers_an_animation_container_is_not_a_reader_archive`),
not asserted in a comment.

The pinned mech3ax v0.6.0 source (`crates/mech3ax-anim/src/parse.rs`, commit
`d3521a9721be731d365504568ddcd78e3f9846bb`, `docs/research/SOURCES.md` S02/S06)
reads `signature`, `version`, `count`, then `AnimNameC { name: Ascii<80>,
unknown: u32 }` — the 84-byte member row below, field order included. It
documents **no** Crimson Skies version and reads **no** member of this family
for Crimson Skies, so everything below the header is this repository's own
measurement, labelled `ClaimStatus::ObservedTool` in the code.

## The measured layout

```text
0x00  u32   signature 0x08170616                      (61 of 61)
0x04  u32   version 53                                 (61 of 61)
0x08  u32   external count = 2                        (61 of 61)
0x0C  u32   member count       (2 .. 170; 2595 over 61 containers)
0x10  2 x { u8 path[128]; u32 stamp }  = 132 bytes each
      N x { u8 path[80];  u32 stamp }  =  84 bytes each
      ... the animation payload ...
```

* **Externals** are the two sibling containers the data refers to, in measured
  form always the world's own `zbd\<group>\gamez.zbd` and the shared
  `zbd\planes.zbd`, in that order, in all 61 containers. They are listed, never
  opened.
* **Members** are the animation-definition sources whose records the payload
  carries: `.zrd` records and `.zan` sequences, named by the same
  Windows-style relative path the scope's paired document uses. A path repeats
  (`..\data\c1c\m01\zrdr\zeps\climbladder.zan` is 14 consecutive rows of M01's
  carrier, `descendladder.zan` and `fbdescendladder.zan` 14 each and
  `halfdescendladder.zan` 13; c1c's camera carrier holds 134 rows over 106
  distinct paths), so rows stay separate and are addressed by index. Measured:
  **the first member row of all 61 containers is the scope's own paired
  document** (`..\data\<group>\<mission>\zrdr\mis_anim.zrd` in a mission scope,
  and the camera container's own `cam_anim.zrd` with doubled separators in the
  8 camera carriers), and no paired document's `ANIMATION_LIST` names its own
  row — so member 0 is unreferenced in every one of the 61.
* **Stamps** are build timestamps by measurement, and the two tables differ, so
  they are counted separately: the 2595 member rows carry 39 distinct values,
  all inside 2000-08-26 08:00:56 .. 08:06:58 UTC, the window a single build
  session would produce, and every carrier's member rows carry more than one of
  them; the 122 external rows carry 9 further values, all **later**
  (08:11:17 .. 08:48:20 UTC) — one per world group, plus a single
  `zbd\planes.zbd` stamp repeated in all 61 containers. What the engine *does*
  with them is not measured, so the code reads them as numbers and says so.

Two measured quirks of the path fields, both kept verbatim and neither decoded:

* **Non-zero padding after the NUL** in 1115 of the 2595 member rows, spread over
  41 of the 61 containers (49 of M01's 91 rows, 57 of c1c's camera 134). The
  bytes carry floats, text and — around M01's row 51 onwards — whole fragments of
  other files (`1.803563 16.190489\nObject: cp_rc\nScaling…`). That is what the
  80-byte field holds behind the path, so `AnimationRow::padding()` hands it out
  and `AnimationRowAnomaly::NonZeroPathPadding` names it. **It is not read as a
  second name**: doing so would invent animation content out of a padding byte.
* **A doubled separator in exactly 8 rows corpus-wide**: the **first member row of
  all 8 camera carriers**, which spells its own paired record
  `..\\data\\<group>\\zrdr\cam_anim.zrd` — every separator doubled except the last
  one. No mission carrier does this, and no measured document names that row
  (every camera document names `..\data\common\zrdr\anim.zrd` instead, which is
  member row 1), so no binding breaks today — but a document that did would not
  match verbatim, which is the point of comparing verbatim.

Every member path field is NUL-terminated and ASCII in all 2595 rows (0
unterminated, 0 non-ASCII), so the reader's two refusal arms are unexercised by
the original installation and are exercised by the synthetic tests instead.

## The payload header, and what is behind it

The fixed 68-byte block at the front of every payload, with this repository's
measured field values (`AnimationPayloadHeader`):

| offset | width | measured | evidence |
| --- | --- | --- | --- |
| `+0 +4 +8` | u32 u32 u16 | zero in 61 of 61 | observed |
| `+10` | u16 | 2 .. 610; never below the member count; 15 024 in total | observed; the *counting records* reading is an inference |
| `+12 +14 +16 +20 +32 +34` | u16 u16 u32 u32 u16 u16 | vary per container (`+16`/`+20` are zero in the same 30) | observed, **meaning not measured** |
| `+24 +28 +44 +48 +52 +56 +64` | u32 | zero in 61 of 61 | observed |
| `+36` | f32 | `-9.8` in 61 of 61 | **documented** — the pinned source asserts the same constant for its own 68-byte anim info block |
| `+40` | u32 | `1` in 48, `0` in 13 | observed, meaning not measured |
| `+60` | u32 | `1` in 61 of 61 | observed |

The 68 bytes are followed by **40 zero bytes** (61 of 61) and then a 32-byte
name field holding a NUL-terminated ASCII string (61 of 61). In c1c's camera
carrier that name is `reserved_anim_0`; in M01's mission carrier it is
`reserved_anim_0` too. `AnimationPayload::first_record_name` reports that one
name, and the module is explicit that **it is one name, not a table**.

### The records behind it are not decoded

`RECORDS_NOT_DECODED_REASON` states why, and this is the evidence:

* The record area begins at a fixed offset (`payload + 108`, measured) and its
  first records sit on a 272-byte stride — but only for the first six of c1c's
  camera carrier, because record length then depends on inline sub-tables.
* What a record holds is measurable but not *interpretable* yet: an animation
  name and an object name as NUL-separated pairs inside 32-byte fields
  (`reserved_anim_0`, `open_map`/`map`, `callback_sequence`, `close_map`,
  `large_camshake`/`player`, `huge_splash`/`model`/`huge_splash_model`,
  `splash_polys`, `sp_1`, `lt_node_name`, `snd_bsplash`, and the `RESET_SEQUENCE`
  the pinned source also expects), floats and `-1` sentinels in the fixed part,
  and word pairs that read as small counts.
* The record-local pointers do **not** resolve inside the container. Values like
  `0x04358690` (≈70.6 MB) and `0x043a9eb0` appear in records of a 1.2 MB file, so
  they are offsets or addresses into whatever image the original engine built,
  and no rule in this tree maps them back to a member row or a record.
* The member rows and the payload's record groups do line up in **order** — M01's
  first records after `reserved_anim_0` are `placepiratezep`/`piratezep`
  (member 1, `placezeps.zrd`), then `placeworkersvoyagezep`,
  `placeblackswanzep` (also `placezeps.zrd`), then `zepskinfire_1`,
  `zep_skin_fire1`, `fire_at_zepskin1` (member 2, `zepskinfire.zrd`) — but
  *ordering* is not a length, and nothing can address record *n* without one.

A guessed walk would produce names no measured rule supports, so this task stops
at the header. The follow-up that finishes the job is the record walk, with the
sub-table counts and the pointer semantics as its two unknowns.

## The binding

`bind_animation_carrier` is the join, and it is where the "what a mission
actually plays" question becomes answerable at file granularity:

1. the carrier's own index, through `read_animation_index`;
2. the scope's sibling `zrdr.zbd`, through the production dispatch, trailer and
   listing readers;
3. the paired record (`mis_anim.zrd` in a mission scope, `cam_anim.zrd` in a
   world-group scope), through the production `.zrd` grammar reader
   `cs_content::stunts::decode_zrd`;
4. its `ANIMATION_DEFINITION_FILE` references joined to the member rows by
   **exact path**: a reference that carries a separator is compared as it stands,
   a bare name is joined to each `ANIMATION_PATH` root in order, and the first
   root that names a member wins. Nothing is normalized, lower-cased or matched
   by basename.

The paired records' measured shape (all 61, decoded by production code):

```text
root -> [ ANIMATION_DEFINITIONS, { GRAVITY: -9.8,
                                   ANIMATION_PATH: [root; ...],   (50 of 61)
                                   ANIMATION_LIST: [ ANIMATION_DEFINITION_FILE, [path] ... ] } ]
```

50 records carry all three keys, 11 have no `ANIMATION_PATH` at all and one more
has an empty one — 12 records with no usable root. `ANIMATION_LIST`'s own count
word is `2n + 1` for `n` references in every record, and `GRAVITY` is `-9.8` in
every one, the same value the carrier's payload header states.

### Measured counts

| population | carriers | member rows | references | bound | unresolved | unreferenced rows |
| --- | --- | --- | --- | --- | --- | --- |
| whole installation | 61 | 2 595 | 739 | 731 | 8 | 1 865 |
| `zbd/c1c` group | 5 | 250 | 46 | 45 | 1 | 205 |
| `zbd/c1c/m01` | 1 | 91 | 22 | 21 | 1 | 70 |

c1c row by row: camera `134` members / `2` references / `2` bound / `132`
unreferenced; `m01` `91` / `22` / `21` / `70`; `ia1` `9` / `8` / `8` / `1`;
`mp1` `2` / `1` / `1` / `1`; `mp3` `14` / `13` / `13` / `1`.

M01's payload: 2 019 493 bytes, payload at offset 7 924, 2 015 569 payload
bytes, declared record count **573**, gravity `-9.8`, first record
`reserved_anim_0`. c1c's camera carrier: 1 217 815 bytes, payload at 11 536,
1 206 279 payload bytes, declared record count **307**.

The 731 bound references reach **730 distinct member rows**, and the difference
is measured rather than rounded away: exactly one carrier,
`zbd/c1b/m03`, binds two of its 29 references to one row. Reference 12 is the
bare name `pzep_getcargo.zrd`, whose first root (`...\vessels`) names no member,
so it resolves at the second root to row 18; reference 22 is the full
`..\data\c1b\m03\zrdr\zeps\pzep_getcargo.zrd`, the same row. Every other carrier
has one distinct row per bound reference.

### The 8 unresolved references, named

Every one of them is reported with the spelling it was compared against; none is
matched by basename.

| carrier | reference | the container's own spelling |
| --- | --- | --- |
| `zbd/c1c/m01` | `..\data\common\zrdr\zeps\wv_tailhook.zrd` | **has** `..\data\c1c\m01\zrdr\zeps\wv_tailhook.zrd` — same basename, mission root |
| `zbd/c5/m02` | `..\data\common\zrdr\zeps\wv_tailhook.zrd` | no member of that basename |
| `zbd/c1/m04` | `..\data\common\zrdr\zeps\pzep_hangerlights.zrd` | none |
| `zbd/c1/m05` | `..\data\common\zrdr\zeps\no_rock.zrd` | none |
| `zbd/c1/cam_anim` | `..\data\c1\zrdr\envmodels\hotelstart.zrd` | none |
| `zbd/c2/cam_anim` | `..\data\c2\zrdr\envmodels\tarzan_huts.zrd` | none |
| `zbd/c5/cam_anim` | `..\data\c5\zrdr\envmodels\manned_aa_gun.zrd` | none |
| `zbd/c5/cam_anim` | `..\data\c5\zrdr\envmodels\generic_signs.zrd` | none |

The M01 row is the interesting one: the document names the **common** spelling
while the carrier stores the **mission-scoped** one. Whether the original engine
resolved that at load time, and how, is **unmeasured** — a basename rule would
make this task's numbers prettier and would be a guess, so it is not applied.
The other seven name a member the container does not carry at all; whether the
original fell back to a common file, or ignored them, is equally unmeasured.

## `startanims.zrd`: read, counted, deliberately not bound

All 53 mission-scoped readers carry `startanims.zrd`; no world-group reader and
not the content-root reader does. Every one of the 53 decodes to the same
**two-key** table — `NEW_GAME_START` and `LOAD_GAME_START`, in that order — each
key holding a list of animation identities, 195 identities in total (1 .. 18 per
scope). M01's is 6 + 1 = 7: `generic_intro`, `wv_hookup_state`,
`pzep_engines_start`, `wvzep_engines_start`, `bszep_engines_start`,
`call_add_jack` under `NEW_GAME_START`, and `player_setup` under
`LOAD_GAME_START`. `zbd/c1c/ia1`'s `LOAD_GAME_START` names none, which is
measured content rather than a failure.

An identity names an **animation record inside the payload**, and the payload's
records are not decoded. So `StartupIdentities::reason` carries
`UNRESOLVED_REASON_NO_RECORD_NAMES` on the value, and the identities are listed
rather than matched by a byte search dressed as a binding. A spot check of the
raw bytes shows why that matters, and is *not* evidence for any rule: of M01's 9
names (2 keys + 7 identities), 4 occur only in its **mission** carrier
(`wv_hookup_state`, `wvzep_engines_start`, `bszep_engines_start`,
`pzep_engines_start`), 3 only in its **camera** carrier (`generic_intro`,
`call_add_jack`, `player_setup`), 1 in both (`pzep_engines_start`) and 2 in
neither (`NEW_GAME_START`, `LOAD_GAME_START`). Which carrier a startup animation
lives in is therefore *not* a rule this tree can state yet — it is the first
thing the record walk has to settle.

## Evidence classes

| claim | class | why |
| --- | --- | --- |
| signature / version offsets, the `AnimNameC` row shape | `Documented` | the pinned mech3ax v0.6.0 source [S02/S06] |
| gravity `-9.8` at `+36` | `Documented` | the same source asserts that constant |
| two count words, the 128-byte and 80-byte path fields, the 68-byte payload header, the 40 zero bytes, the first record name, the stamp range, the non-zero padding, the doubled separator | `ObservedTool` | read out of the 61 retail containers by this repository; no source states them |
| the container is "the animation family" (role rule) | `Documented` | the mech3ax README's family list, task #340 findings |
| `+10` counts animation records | **inference** | the pinned source reads a record count at the same offset of *its* layout; nothing here confirms it, so the field is reported as a declared count and no consumer indexes a record by it |
| `+12 +14 +16 +20 +32 +34 +40` meaning | **unknown** | measured values only |
| record layout, sub-table sizes, record-local pointers | **unknown** | this is the task's open item |
| whether the original resolves the 8 references, and how | **unknown** | no original run |

## Limits, and what stays open

* **The animation records are not decoded.** A member is bound to a definition
  file; its animation records, their names, their node and object references and
  their sequence events are not read. Every mission-facing animation claim
  depends on that walk.
* **`startanims.zrd` identities are unbound** for the same reason, and the
  carrier split above (mission vs camera) is unmeasured.
* **The `ANIMATION_PATH` roots are used as spelled**, including one record that
  stores two roots in a single `;`-joined string (19 of 61 do). Whether the
  original split on `;` or stored a list is an inference from the corpus; the
  join is unchanged either way, because both roots are offered.
* **Nothing is `verified_original`.** No original executable was run; `gpu` and
  `audio` were available and unused.
* The c1c retail test is one production discovery pass plus the whole 61-carrier
  census (~38 s on the implementer's machine), because the corpus totals are the
  evidence that 8 is the whole population and not a c1c artefact.

## Review (2026-10-04, `bunny-alpha-1`)

Reviewed in a fresh session with no memory of the implementation, by the same
agent identity that wrote it — so it is a second pass over the code, **not**
independent evidence, and the format claims above still want a different
reviewer (a task asks for one).

**Re-derived independently.** Every number this finding pins was recomputed
from `$CS_GAME_DIR` with a throwaway reader written against the measured
layout alone (the trailer's 148-byte index entries and the `.zrd` grammar, both
re-implemented from this repository's readers) rather than through the code
under review. All of it reproduces: 61 carriers / 2595 member rows / 739
references / 731 bound / 8 unresolved / 1865 unreferenced, the c1c row-by-row
table, the payload offsets, lengths and declared counts, the 19 `;`-joined
`ANIMATION_PATH` records (11 without, 1 empty), the 53 `startanims.zrd` records
with 195 identities and M01's exact seven, the 1115 padded rows over 41
containers, the 272-byte stride of the first six records of c1c's camera
carrier, and the 8 unresolved spellings.

**Corrected here** (the findings were mine to fix, so they are fixed rather
than filed): `climbladder.zan` is **14** consecutive M01 rows, not 13 (`13` is
`halfdescendladder.zan`); the stamp counts are per table (39 member, 9
external, 48 over all rows), not 39 "corpus-wide"; the doubled separator is in
the first row of the **8 camera carriers** only, and its last separator is not
doubled.

**Fixed in the code.**

* `AnimationIndex::payload()` used to report an **empty** first-record name
  when the payload held the 68-byte header but stopped inside the name field.
  It now refuses with `AnimationIndexError::FirstRecordNameTruncated` (needed
  32, available what it had), covered by the synthetic refusal test.
* `bind_animation_carrier` used to push a `carrier_refused` blocker carrying an
  invented `unvalidated_header` code and then **read the index anyway** when
  dispatch answered `HeaderStatus::Unvalidated`. It now refuses with
  `BindingBlocker::UnvalidatedHeader`, matching `survey::CarrierBlocker` in the
  sibling F20-D module. Unreachable for this family, which has a documented
  signature rule — so the fix is consistency and fail-closed, not a live bug.
* `CarrierMember` now carries the reader's `AnimationRowAnomaly` list. The
  lossy path decode made an anomaly invisible to every consumer of the binding;
  the retail test now asserts the measured census (1115 padded rows over 41
  carriers, **zero** unterminated and zero non-ASCII rows) and that member 0 is
  the scope's own paired document in all 61 carriers and unreferenced in all 61,
  instead of leaving both claims as prose.

**Not changed, and still the honest gap.** The animation records behind the
payload header are not decoded. The task's acceptance sentence names "decoded"
among the members that M01's documents reference; this stage decodes the
carrier index, the payload header and the first record's name, and stops there
because no measured rule fixes a record's length (see above). A follow-up task
carries the record walk.