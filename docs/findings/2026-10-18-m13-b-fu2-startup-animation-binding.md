# M13-B-FU2: where M13's nine startup animation names resolve

Date: 2026-10-18. Task: M13-B-FU2 (Rally #1244), the follow-up of M13-B (#295,
`missions/M13.md`). Owner paths used: `crates/cs_app/tests/campaign/`,
`crates/cs_content/src/campaign_bindings.rs`, `missions/bindings/M13.json` and
`docs/findings/`. Capability: `retail` (`$CS_GAME_DIR`, read-only). Implementer:
sonnet-2. Nothing here is `verified_original`; no original executable ran.

## Verdict

Production binds **eight of the nine** startup names to exactly one declaring
member and one carrier record, and refuses the ninth by name as ambiguous.
The empty `mis_anim.zrd` of `ZBD/C3/M03/zrdr.zbd` is not where the declarations
live: `bind_mission_animation(.., "zbd/c3/m03")` searches the mission archive,
the `c3` group archive and the shared root archive, and every payload record is
in the separate container `ZBD/C3/M03/mis_anim.zbd` (368 records) or, for
`player_setup`, `ZBD/C3/cam_anim.zbd` (398 records).

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

## Surviving limitation

Recorded where binding consumers read it: a `MISSION_SCOPED_UNKNOWNS` entry for
`M13` in `crates/cs_content/src/campaign_bindings.rs`, and the regenerated
`missions/bindings/M13.json` (via `CS_REGENERATE_BINDING=M13`). It names the
affected content (M13's `NEW_GAME_START` row `cg1zep_engines_stop`, the cargo
zeppelin's engine stop) and the resolving task (M13-C, an original reference
run, which would show which declaration the original selects). The report in
`docs/findings/evidence/M13-B-FU2.json` carries it in `unknowns`.

## Tests

`accept_m13_b_fu2_*` in `crates/cs_app/tests/campaign/m13_b_fu2.rs`: one
synthetic member (CI) pinning single / ambiguous / unresolved through
`read_startup_animations` and `WorldActorProgramBinding::new`; one (CI) pinning
that the limitation reaches M13's record alone and the committed record carries
it; two `#[ignore = "requires CS_GAME_DIR"]` retail members pinning the table
above and the empty `mis_anim.zrd`.
