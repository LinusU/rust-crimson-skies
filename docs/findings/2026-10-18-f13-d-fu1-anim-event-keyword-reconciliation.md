# #1415 (F13-D-FU1): the owner's anim-event keywords against F20's event-stream table

Date: 2026-10-18. Task: #1415. Follow-up of #640 (F13-D). Capability used: `retail`
(read-only reader archives of `$CS_GAME_DIR`). Nothing was run, rendered or decoded
from the executable here; the owner's keyword list is taken from the owner note on
#640 (2026-10-05, static analysis of `crimson.decrypted.exe`, sha256
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`, table
`0x63341c`–`0x633718`, matched in `0x51c47d`). Static evidence, address-plus-summary,
never `verified_original`. No decompiled code or executable bytes are committed.

**Affected content: all 242 391 events of the 61 retail carriers** (56 994 blocks).
**No model is claimed for any anim family anywhere in this report** until every
name is placed *and* the four unjoined ids are measured (#1416).

## Code and tests

- `crates/cs_app/src/animation/keywords.rs` (new, read-only measurement):
  `OWNER_KEYWORDS` (the 44 names of the note), `ReaderVocabulary` (statement kinds and
  field names counted from the reader text), `reconcile`. Wiring: `pub mod keywords;`.
- `crates/cs_app/tests/accept_f13_d_fu1_keyword_reconciliation.rs` (registered in
  `suite_b.rs`): three `accept_f13_d_fu1_` tests, two synthetic, one retail
  (`#[ignore = "requires CS_GAME_DIR"]`). They fail when `OWNER_KEYWORDS` or the
  placement rule is removed.

## Counts, corrected

The note says "41 event keywords" and enumerates **44 distinct names**. The task text's
"43 entries after removing 2 duplicates" does not reproduce: the enumeration holds no
duplicate (`ANIM_VERBOSE`/`GLOBAL_ANIM_VERBOSE` are two names). All 44 are placed; the
"41" is not reproduced and is not used. F20's side: 35 stored ids = 31 joined to a
declaration statement (30 distinct spellings; `OBJECT_MOTION_SI_SCRIPT` is stored as 12
and 47) + 4 unjoined (13, 17, 26, 28). `STORED_OPCODES` on main still holds exactly
those 35 rows. The "59→41" mapping of the owner's wording is not reproducible from main:
there are 31 spelled rows → 30 owner names.

## Placement rule

Each owner keyword is placed once, first match wins: (1) **compiled-event name** — the
spelling equals an F20 statement joined to a stored id; (2) **reader-text
record-vocabulary field** — a definition/record field name of the reader text;
(3) **reader statement without stored id** — a `SEQUENCE_DEFINITION` statement kind of
the reader text with no stored id; (4) **neither** — the installation's reader text never
writes it (all `zrdr.zbd` archives) and no id carries it.

## Result (retail, `accept_f13_d_fu1_retail_placement`)

| placement | count | owner keywords |
| --- | ---: | --- |
| compiled-event name | 30 | the 30 rows below |
| reader-text record-vocabulary field | 1 | `ACTIVATION` (1 350 as a field; also 1 914 as a statement kind, which F20 drops from the stored events) |
| reader statement, no stored id | 3 | `OBJECT_OPACITY_STATE` (23), `OBJECT_CYCLE_TEXTURE` (12), `RESET_ANIMATION` (2) |
| neither | 10 | `EFFECT`, `FOG_STATE`, `OBJECT_CONNECTOR`, `OBJECT_VERTEX_COLOR`, `CALL_OBJECT_CONNECTOR`, `CAMERA_FROM_TO`, `FBFX_CSINWAVE_FROM_TO`, `ANIM_VERBOSE`, `GLOBAL_ANIM_VERBOSE`, `ON_CALL` |
| total | 44 | |

### F20 spelling → owner keyword (every spelled row, one citation each)

Citation for every row: F20 table row in `docs/findings/2026-10-06-f20-event-grammar.md`
(census, #690) / `STORED_OPCODES`; owner side: the #640 note's keyword list. Spelling
equality is exact; no pair is guessed.

| id | F20 spelling | owner keyword | events |
| ---: | --- | --- | ---: |
| 1 | SOUND | SOUND | 6 398 |
| 2 | SOUND_NODE | SOUND_NODE | 1 244 |
| 4 | LIGHT_STATE | LIGHT_STATE | 1 468 |
| 5 | LIGHT_ANIMATION | LIGHT_ANIMATION | 535 |
| 6 | OBJECT_ACTIVE_STATE | OBJECT_ACTIVE_STATE | 68 109 |
| 7 | OBJECT_TRANSLATE_STATE | OBJECT_TRANSLATE_STATE | 1 143 |
| 8 | OBJECT_SCALE_STATE | OBJECT_SCALE_STATE | 2 209 |
| 9 | OBJECT_ROTATE_STATE | OBJECT_ROTATE_STATE | 6 610 |
| 10 | OBJECT_MOTION | OBJECT_MOTION | 7 458 |
| 11 | OBJECT_MOTION_FROM_TO | OBJECT_MOTION_FROM_TO | 9 421 |
| 12 | OBJECT_MOTION_SI_SCRIPT | OBJECT_MOTION_SI_SCRIPT | 845 |
| 47 | OBJECT_MOTION_SI_SCRIPT | OBJECT_MOTION_SI_SCRIPT | 22 |
| 14 | OBJECT_OPACITY_FROM_TO | OBJECT_OPACITY_FROM_TO | 9 917 |
| 15 | OBJECT_ADD_CHILD | OBJECT_ADD_CHILD | 1 163 |
| 16 | OBJECT_DELETE_CHILD | OBJECT_DELETE_CHILD | 263 |
| 20 | CAMERA_STATE | CAMERA_STATE | 10 |
| 22 | CALL_SEQUENCE | CALL_SEQUENCE | 22 391 |
| 23 | STOP_SEQUENCE | STOP_SEQUENCE | 1 143 |
| 24 | CALL_ANIMATION | CALL_ANIMATION | 56 750 |
| 25 | STOP_ANIMATION | STOP_ANIMATION | 7 928 |
| 27 | INVALIDATE_ANIMATION | INVALIDATE_ANIMATION | 6 618 |
| 30 | LOOP | LOOP | 3 475 |
| 31 | IF | IF | 5 468 |
| 32 | ELSE | ELSE | 4 680 |
| 33 | ELSEIF | ELSEIF | 5 727 |
| 34 | ENDIF | ENDIF | 5 468 |
| 35 | CALLBACK | CALLBACK | 736 |
| 36 | FBFX_COLOR_FROM_TO | FBFX_COLOR_FROM_TO | 155 |
| 41 | DETONATE_WEAPON | DETONATE_WEAPON | 3 |
| 42 | PUFFER_STATE | PUFFER_STATE | 4 535 |
| 46 | SOUND_ADJUST | SOUND_ADJUST | 3 |
| 13 | *(no spelling)* | *(none assigned)* | 340 |
| 17 | *(no spelling)* | *(none assigned)* | 144 |
| 26 | *(no spelling)* | *(none assigned)* | 11 |
| 28 | *(no spelling)* | *(none assigned)* | 1 |

31 spelled rows → 30 owner names; **no F20 spelling is missing from the owner list and
none is extra** (`extra_f20` is empty). The four unjoined ids are the 496 events whose
meaning is unknown.

### Extra names on the reader-text side

Statement kinds the reader text writes that are in neither list: `START_TIME` (13),
`generic` (1), `instead` (1), `is` (1) — free-text artefacts of the entries as read, not
keywords; recorded, not placed as event names. Reader field names that are not owner
keywords (`ANIMATION_ROOT_NAME`, `HEALTH`, `RESET_TIME`, …, 18 names) are definition
configuration, a different vocabulary, listed by `reader_field_extras`.

## The owner's cross-check does not hold as stated

The owner confirmed (and the F13-D finding predicted) that the owner keywords with no
compiled mention are exactly the four ids 13, 17, 26, 28. Measured: **14** owner keywords
have no F20 spelling (3 + 10 + `ACTIVATION`), not 4. The four-way match is **refuted as a
count equality**. What survives: the four ids are unjoined and no owner keyword is
assigned to them. Which of the 13 non-`ACTIVATION` candidates they are (if any; four of
them need not be stored at all) stays #1416's measurement. `ACTIVATION` is a reader
field and statement that F20's alignment rule drops, so it cannot be one of them.

## Not claimed

No model for any anim family; no `verified_original`; no id → keyword assignment for
13/17/26/28; the owner keywords placed "neither" are not said to be absent from the
engine, only from the installation's reader text and stored ids.
