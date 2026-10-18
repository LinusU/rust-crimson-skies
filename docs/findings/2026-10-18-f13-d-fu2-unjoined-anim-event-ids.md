# F13-D-FU2 (#1416): the four unjoined anim event ids (13, 17, 26, 28)

Date: 2026-10-18. Follow-up of F13-D (#640) and F20 (#690). Capabilities used:
`retail` (read `$CS_GAME_DIR`) and the owner-supplied decrypted engine image
(`$CS_ENGINE_IMAGE`, read only, never committed). No original run was observed:
every row below is provenance `static` (engine image) or `observed_tool`
(installation files), **nothing is `verified_original`**. Committed: a small
factual table (ids, virtual addresses, lengths, keyword spellings) in
`crates/cs_app/src/animation/event_keywords.rs`; no bytes, disassembly or
decompiled code (AGENTS.md rule 3, owner exception of 2026-10-14).

## Result

| id | keyword | reader-text twin | parameters (parser order) | timing | role |
| ---: | --- | --- | --- | --- | --- |
| 13 | `OBJECT_OPACITY_STATE` | aligned: 10 events over candidate records | `START_TIME`, `NAME`, `MAIN_ROOT_NODE`, `STATE` (`ON`/`OFF`) | `START_TIME` = payload word 0; no `RUN_TIME` keyword | sets a named node's opacity state ON/OFF; record 24 bytes |
| 17 | `OBJECT_CYCLE_TEXTURE` | aligned: 96 events | `NAME`, `START_TIME`, `RESET` | as above | cycles a named node's texture; `RESET` is bit 0 of the flag word at record offset 12; record 20 bytes |
| 26 | `RESET_ANIMATION` | aligned: 10 events | `NAME`, `START_TIME` | as above | names an animation (32-byte field at offset 12), resolved by the runtime to an instance; record 48 bytes |
| 28 | `FOG_STATE` | **none alignable** (see below) | `NAME`, `START_TIME`, `TYPE` (`OFF`/`LINEAR`/`EXPONENTIAL` = 0/1/2), `COLOR`, `ALTITUDE`, `RANGE` | as above | scene fog; record 80 bytes, presence bits for COLOR/ALTITUDE/RANGE in the flag word at offset 44 |

## Method and evidence

1. **Engine parser (static).** The image's animation reader parser
   (`zeff_anim_init.c`) tests each keyword by string compare (keyword table at
   VA `0x63341c`..`0x633740`; compare chain at `0x51c481`..`0x51d260`) and calls
   one parser per keyword. Each parser stores the event id as the first byte of
   a new record and the record length next to it. Writer sites: id 13 `0x511404`,
   17 `0x5127e2`, 26 `0x5161d9`, 28 `0x4ff31a`; all 35 stored ids are in the
   table with their site. The parsers' own string references give the parameter
   lists above.
2. **Runtime handler (static).** The animation runtime (`0x4ecbb0`..) dispatches
   an event through a table at VA `0x727de0` indexed by id (ids 1..0x2f), filled
   at startup at `0x4ee1a3`..`0x4ee370`; handlers: 13 `0x4ea960`, 17 `0x4eabd0`,
   26 `0x4ebe40`, 28 `0x4e8540`.
3. **Cross-check against F20's join.** For the 31 joined ids the engine
   keyword equals F20's declaration spelling in every case (test
   `accept_f13_d_fu2_table_has_a_row_for_every_stored_id`), so the engine
   mapping is independently confirmed on 31 ids before it names four more.
   The record length the parser stores equals the length word of **every one of
   the 242 391 events** of the retail corpus for 34 ids (id 47 appends the
   script it reads, so its events are longer than the fixed part; id 46's
   length is computed). Counts for the four ids reproduce F20's census:
   340, 144, 11, 1.
4. **Reader-text twin.** F20 required a *unique* declaration/record pair, which
   excluded the records carrying these ids. Aligning each declaration against
   **every** record of the same animation name (ACTIVATION dropped as F20 did)
   puts `OBJECT_OPACITY_STATE` only at id 13 (10 events), `OBJECT_CYCLE_TEXTURE`
   only at id 17 (96) and `RESET_ANIMATION` only at id 26 (10), with no other
   spelling at those ids. Refutation rules from F13-D (position joins to a
   resolved spelling; two spellings at one id) did not fire. Caveat: a handful
   of non-unique pairs misalign other ids (6, 14, 24), which is why F20's
   unique rule stays the table of record for the 31 joined ids; the four
   unjoined ids show no conflict.
5. **Id 28 has no alignable twin.** `FOG_STATE` occurs in reader text
   (`ZBD/C1/M04/zrdr.zbd`, `NAME drop_fog`, `TYPE LINEAR`, ...), but no
   declaration statement read by the production reader carries it, and the one
   id-28 event is not in an ordinary sequence block. Its name rests on the engine
   image alone (`TwinEvidence::EngineOnly`).
6. **`ACTIVATION` and the no-mention keywords** (the F13-D prediction): the
   engine keywords with no compiled mention are `ACTIVATION`, `ON_CALL`,
   `ANIM_VERBOSE`, `GLOBAL_ANIM_VERBOSE`. The parser stores ids 0x27/0x28 for
   the verbose pair, so they are event ids outside the 35 stored ones; the
   prediction that the four unjoined ids join the *no-mention* keywords is
   **refuted**: they join `OBJECT_OPACITY_STATE`, `OBJECT_CYCLE_TEXTURE`,
   `RESET_ANIMATION` and `FOG_STATE`, which are ordinary statements. The full
   vocabulary reconciliation stays #1415.

Id 12 vs 47: both are `OBJECT_MOTION_SI_SCRIPT` (F20); the engine writes 47 at
`0x510f15` (16-byte fixed part) and 12 at `0x50fd64` (76-byte record with
`SPLINE_INTERP`, `IGNORE_*`, `*_OFFSET`). Recorded, not interpreted.

## The 496 events stay refused

Ids 13, 17, 26 and 28 are now **named**; `decode_event_stream` and F20's
`PlaybackGap` still refuse the 496 events in 389 blocks / 347 records / 16
carriers. Reason (`UNJOINED_REFUSAL`): payload fields beyond `START_TIME` are
only partly measured (e.g. fog `COLOR`/`ALTITUDE`/`RANGE` offsets overlap in the
reading of the stores and are not assigned), no original run shows any
effect, and the F20 timing rule is value-matched per joined statement. They
become decodable with a consumer task that measures the payload fields and an
original run or capture.

## Tests (`accept_f13_d_fu2_`)

In `crates/cs_app/tests/accept_f13_d_fu2_anim_event_keywords.rs` (binary
`cs_app_tests_a`): the table covers every stored id and agrees with F20 on the
31 joined; the four ids stay unnamed in F20's table and refused by the
decoder (naming one silently, or dropping a row, fails); retail lengths,
counts and twin alignment (`CS_GAME_DIR`); engine-image drift guard re-reading
every writer site, length, handler registration and keyword string
(`CS_ENGINE_IMAGE`).

## Sources

`docs/findings/2026-10-14-f13-d-every-program-family-has-its-real-model.md`,
`docs/findings/2026-10-06-f20-event-grammar.md`,
`docs/findings/2026-10-14-owner-facts-table.md`, the engine image and the
installation read-only.
