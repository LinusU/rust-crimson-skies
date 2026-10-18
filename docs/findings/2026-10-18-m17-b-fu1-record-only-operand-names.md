# M17-B-FU1: what the original binds M17's record-only names to

Date: 2026-10-18. Task: M17-B-FU1 "Resolve M17's record-only operand names
against the engine image or a reference run" (Rally #1256, follow-up of M17-B
#307; `docs/findings/2026-10-10-m17-b-compatibility-gaps.md`). Capabilities
used: `retail` (`$CS_GAME_DIR`, read-only) and the owner-supplied decrypted
engine image (`$CS_ENGINE_IMAGE`, SHA-256 `43540fc9…c37d75`, read-only, nothing
of it committed). Implementer: **sonnet-2/sonnet-2** (Rally #1256, session of
2026-10-18). Provenance of every claim below is **static** (AGENTS.md rule 3's
owner exception: numbers and tables are facts; no disassembly or code is
committed, the pinned routines are cited by address, length and SHA-256). No
original executable was run and no mission was played, so nothing here is
`verified_original` (rule 8); the runtime halves stay with M17-C
(`human_play`).

M17-B left nine texts of M17's control member declared by no other `.zrd`
member. This note gives each its consumer in the engine and what that consumer
does with the name. Eight of the nine are resolved to a mechanism; one
sub-question (what else might register an actor or node name at run time) stays
unknown and is named below.

## Resolution table

| Text(s) | What it is | Consumer (image VA) | What the original does | Status |
| --- | --- | --- | --- | --- |
| `WARP_VEHICLE` | a directive **key** | the key table string at `0x626674`, the only occurrence in the image | The mission parser recognises it; no other archive spells it because no other mission uses it. | resolved (already measured by M01-LC-DIRECTIVE-C; this row records the absence) |
| `MSG_BRF_RMM2_OBJ1`, `MSG_BRF_RMM2_OBJ2` | **localized-string labels** (the `IDENTITY` operands) | `strings.dll` export `ZLocGetStringID` (file offset `0x1010`, 95 bytes): a linear walk of an eight-byte `{name pointer, id}` table at `0x10006030..0x10008028` (1 023 rows) comparing the label byte by byte; zero when absent. The engine reaches it through a function pointer at `0x9ad9bc` (the image holds no `MSG_BRF_` literal). | Row 679 and 680 of the table: ids **16 420** and **16 421**, consecutive, and both ids are rows of the same DLL's `RT_STRING` catalog (read by the production `StringCatalog`). | resolved. M17-B searched `.zrd` members only; `strings.dll` is the declaration site. M13's `MSG_BRF_HAM3_OBJ*` have the same shape (ids 16 340, 16 341). |
| `bhatbrigand_13`, `bhatbrigand_14` (and `bhatbrigand_4`, softer) | actor names, `SET_AI_NET` operands | consumer `0x46a000` (168 bytes, SHA-256 `8625eebc…319abd`): vehicle registry `0x71dab8` via the case-insensitive `_stricmp` walk at `0x4aff10`; on a miss the zeppelin registry `0x71df80` via `0x4bd3e0`; on a second miss it logs `SET_AI_NET: can't find vehicle or zeppelin %s` (`mission.cpp` line `0x11fa`) and returns. | A name that is in neither registry is **logged and skipped**: no write, no error, the remaining pairs still run. | mechanism resolved. **Which registry contents M17 has when block 20/30 fire is not decided statically** (see Unknowns). `bhatbrigand_4` is in sibling missions' tables only, so it takes the same path under the same condition. |
| `snd_c4-RM-m2_Zachary_13`, `_15`, `snd_c4-RM-m2_Blacke_14`, `_16` | queued-sound names, `STOP_QUEUED_SOUNDS` operands | `0x5920f0` (84 bytes, SHA-256 `5a3a8116…cbef`): the queue `[0x639eac]` is searched for an entry with the name through `0x5791a0`; a null answer returns. | A name that matches no queued sound is a **silent no-op**; a match is unlinked and rescheduled for removal 10.0 later (D's measured effect). So these are not declarations that must exist: they are compared with whatever sound was queued under that name. | resolved. Whether the original ever queues sounds under these four names (the two declared names are in `ZBD/zrdr.zbd#sounds.zrd`) is a runtime observation for M17-C. |
| `pp5` | **node name**: the fifth child of the last `WARP_VEHICLE` point of block 23 | `0x4940d0` (627 bytes, SHA-256 `4ea4afe1…b7a0`) calls the find-or-create lookup `0x4511b0` (644 bytes, `1bd669dc…2e28e`) on the node registry `0x6541b0` (`_stricmp` walk, creates an empty node when missing); the warp returns unless the node's position list (12-byte entries) holds at least two. | A name that no loaded data has put in the registry yields a fresh, empty node and **no warp**; the vehicle stays where it is. `pp5` is declared only by `ZBD/C2/M02/zrdr.zbd#aiv.zrd`, not by anything M17 loads, so a draw of that fourth point (one in four, `rand() % 4`) is expected to be a no-op unless something else fills the node. | mechanism resolved; the registry's feed is **not** traced (Unknowns). |

The three other points of the same warp site carry raw coordinates and no node
name, and take the other branch (`0x493fb0`, written in degrees), unchanged
from M01-LC-DIRECTIVE-C.

## Evidence

* **Image spans** (virtual address, length, SHA-256 — the digests are in the
  test constants and re-derived from `$CS_ENGINE_IMAGE` on every run):
  `0x46a000`/168, `0x4aff10`/76, `0x5920f0`/84, `0x4940d0`/627,
  `0x4511b0`/644; string `0x626f18`
  (`SET_AI_NET: can't find vehicle or zeppelin %s`), `0x626778`
  (`STOP_QUEUED_SOUNDS`), `0x626674` (`WARP_VEHICLE`).
* **Installation file**: `strings.dll` (131 072 bytes, SHA-256
  `7582feca…445c21`), the 95 bytes at file offset `0x1010`
  (`ZLocGetStringID`, SHA-256 `4853f7d6…2e55`); the code itself names the table
  base, the stride and the end, and the tests read the layout from those
  bytes. Section `.data` is mapped at its file offsets in this DLL.
* **Whole-installation scans** (raw bytes, not the `.zrd` index): the four
  `snd_c4-RM-m2_*` names, `bhatbrigand_13`, `bhatbrigand_14` and
  `WARP_VEHICLE` occur in exactly one file, `ZBD/C4/M02/zrdr.zbd`;
  `MSG_BRF_RMM2_OBJ1/2` occur in that file and in `strings.dll`; none of the
  names occurs in the engine image.
* **Correction to an earlier note**: M01-LC-DIRECTIVE-C wrote that the
  `SET_AI_NET` net-name search is case-sensitive. The *actor* lookup above is
  `_stricmp` (case-insensitive); the net-name walk was not re-read here.

## Tests (`accept_m17_b_fu1_*`, 6: 5 retail, 1 CI)

In `crates/cs_app/tests/campaign/m17_b_fu1.rs`; they reuse the M17-B helpers
(made `pub(super)` in `m17_b.rs`, no behaviour change), production discovery,
the production `.zrd` decoder, `cs_content::config::StringCatalog` and
`cs_content::coordinates::load_engine_image`.

* `…_the_msg_labels_resolve_through_the_string_dlls_name_table`
* `…_an_unregistered_actor_name_is_logged_and_skipped_by_set_ai_net`
* `…_unmatched_queued_sound_names_are_skipped_silently`
* `…_the_pp5_warp_node_is_a_registry_lookup_that_refuses_an_empty_node`
* `…_warp_vehicle_is_a_directive_key_spelled_once_in_the_image`
* `…_every_record_only_name_has_exactly_one_recorded_resolution` (runs in CI,
  no original data: a tenth record-only name, or a dropped one, fails here
  until this note is updated)

## Unknowns, kept unknown

1. **Does anything register `bhatbrigand_13` / `_14` as an actor at run time?**
   The registry `0x71dab8` is keyed by name; M17's own aircraft table
   (14 records, `bhatbrigand_1..3`, `5..12`) has neither. A static reading of
   the registry's insert sites was not done here. If nothing else registers
   them, blocks 20 and 30 net two fewer actors than they spell and the engine
   only logs. Resolver: **M17-C** (a reference run reading the log, or a
   follow-up that traces the inserts into `0x71dab8`).
2. **What feeds the node registry `0x6541b0`.** Its three `0x4511b0` callers
   are `0x45199b` (builds a `%.2s%.3s` name from a packed reference), `0x47c3c2`
   (an aiv record's field at `+0x11c`) and `0x494100` (the warp). No reader of
   `...\zrdr\nets\neindex.zrd` was found by a direct reference to its string,
   so whether `pp5` can ever be in the registry in M17 is not established; the
   no-warp conclusion above holds **only if** nothing in C4/M02 fills it.
   Resolver: **M17-C** or a follow-up tracing the registry's loader.
3. **Which of the four unmatched sounds the original ever queues.** Static
   reading shows only that a miss is harmless. Resolver: **M17-C**.
4. **The `IDENTITY` label's consumer.** The mission parser reads only the
   class and the HUD ordinal (M01-LC-DIRECTIVE-A/B); the label is resolved by
   something that holds the retained document. That the label *resolves* is
   shown; what reads it (briefing screen?) is not. Resolver: **M17-C** for the
   briefing's appearance.

No name was guessed: a name is listed as resolved only where an image span or
an installation file establishes the mechanism.

## Checks

See the submission summary for the exit codes of `cargo fmt`, `cargo clippy`,
the crate tests and the `accept_m17_b_fu1_` run.
