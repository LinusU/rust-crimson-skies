# M13-B-FU2: where M13's startup animation names resolve

Date: 2026-10-18. Task: M13-B-FU2 (Rally #1244), the follow-up of M13-B (#295,
`missions/M13.md`). Owner paths used: `crates/cs_app/tests/campaign/`,
`crates/cs_content/src/campaign_bindings.rs`, `missions/bindings/M13.json`,
`missions/bindings/M03.json` and `docs/findings/`. Capability: `retail`
(`$CS_GAME_DIR`, read-only). Implementers: **sonnet-2** (Rally #1244) and
**bunny-alpha-2** (the re-pin, Rally #1600). Nothing here is
`verified_original`; no original executable ran.

## 2026-10-18 re-pin on the corrected campaign layout (#1600)

The nine-name table below was measured on `ZBD/C3/M03/zrdr.zbd` under the M13
label. The corrected campaign layout
(`docs/findings/2026-10-17-m01-archive-decision.md`) binds that archive to work
order **M03**, and M13 to `ZBD/C2/M03/zrdr.zbd`; so the nine-name table and its
ambiguous `cg1zep_engines_stop` describe **M03's** data, not M13's. Re-derived
through `bind_mission_animation` on M13's own archive:

M13's `startanims.zrd` declares exactly **three** startup rows across two
events (`player_setup` and `pzep_engines_start` on `NEW_GAME_START`,
`player_setup` again on `LOAD_GAME_START`), every row binds one declaring member
and one carrier record, and none is refused:

| Event | Name | Declaring member (archive) | Carrier record | Playable |
| --- | --- | --- | --- | --- |
| NEW_GAME_START | `player_setup` | `player_setup.zrd` (`zbd`) | camera #116 | yes |
| NEW_GAME_START | `pzep_engines_start` | `pirate_zep_nacelles.zrd` (`zbd`) | mission #33 | yes |
| LOAD_GAME_START | `player_setup` | `player_setup.zrd` (`zbd`) | camera #116 | yes |

The archive's own `mis_anim.zrd` declares no definition either: it names
authoring files through `ANIMATION_DEFINITION_FILE` (the same shape M03's
member has), which the measured reader records as definition-file paths rather
than definitions.

The surviving limitation follows the archive it was measured on
(M01-ARCHIVE-DECISION): the ambiguous `cg1zep_engines_stop` entry moved out of
M13's `MISSION_SCOPED_UNKNOWNS` into **M03's**, and `missions/bindings/M03.json`
now carries it, keeping the M13-B-FU2 citation as provenance. M13's own record
(`missions/bindings/M13.json`) carries no mission-scoped unknown, and the
M13-B-FU2 report records none — its `accept_m13_b_fu2_` tests assert that the
limitation reaches M03's record and not M13's.

## Original measurement (`ZBD/C3/M03` — M03's archive)

The text below is the historical record of the #1244 measurement, kept
unchanged except for this attribution note.

Production binds **eight of the nine** startup names of `ZBD/C3/M03/zrdr.zbd`
to exactly one declaring member and one carrier record, and refuses the ninth
by name as ambiguous. The empty `mis_anim.zrd` of that archive is not where the
declarations live: `bind_mission_animation(.., "zbd/c3/m03")` searches the
mission archive, the `c3` group archive and the shared root archive, and every
payload record is in the separate container `ZBD/C3/M03/mis_anim.zbd` (368
records) or, for `player_setup`, `ZBD/C3/cam_anim.zbd` (398 records).

| Event | Name | Declaring member (archive) | Carrier record | Playable |
| --- | --- | --- | --- | --- |
| NEW_GAME_START | `place_tntboxes` | `destroy_cargozep.zrd` (`zbd/c3/m03`) | mission #359 | yes |
| NEW_GAME_START | `pzep_todrydock` | `destroy_cargozep.zrd` (`zbd/c3/m03`) | mission #358 | yes |
| NEW_GAME_START | `cgzepstate` | `zepstate.zrd` (`zbd/c3/m03`) | mission #215 | yes |
| NEW_GAME_START | `set_cggasbagstate` | `cg1_hawaii_set.zrd` (`zbd`) | mission #342 | yes |
| NEW_GAME_START | `cg1zep_engines_stop` | **ambiguous**: `cg1_zep_nacelles.zrd` and `dante_zep_nacelles.zrd` (`zbd`), both definition 4 | mission #245 (unique) | **no** |
| NEW_GAME_START | `pzep_engines_start` | `pirate_zep_nacelles.zrd` (`zbd`) | mission #49 | yes |
| NEW_GAME_START | `calldestroy_the_cargozep` | `destroy_cargozep.zrd` (`zbd/c3/m03`) | mission #361 | yes |
| NEW_GAME_START | `flag_state_pirate` | `flag_pirate.zrd` (`zbd/c3`) | mission #1 | yes |
| LOAD_GAME_START | `player_setup` | `player_setup.zrd` (`zbd`) | camera #126 | yes |

`cg1zep_engines_stop` is refused with production's own
`PlayRefusal::AmbiguousDeclaration` ("more than one member of the mission
closure declares this animation name, and no measured rule picks one of the
declaring sites", 2 sites). No nearest match, no preference for either member.

## Tests

`accept_m13_b_fu2_*` in `crates/cs_app/tests/campaign/m13_b_fu2.rs`: one
synthetic member (CI) pinning single / ambiguous / unresolved through
`read_startup_animations` and `WorldActorProgramBinding::new`; one (CI) pinning
that the limitation reaches the archive it was measured on (M03's record alone,
not M13's) and that the committed record carries it; two
`#[ignore = "requires CS_GAME_DIR"]` retail members pinning the two-name table
above and the definition-free `mis_anim.zrd`.
