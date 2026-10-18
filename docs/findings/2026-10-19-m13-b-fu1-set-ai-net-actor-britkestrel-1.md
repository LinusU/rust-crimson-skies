# M13-B-FU1: what the original does with the `SET_AI_NET` actor `britkestrel_1`

Date: 2026-10-19. Task: M13-B-FU1 "Resolve the actor M13's SET_AI_NET site
names" (Rally #1243, follow-up of M13-B #295). Capabilities used: `retail`
(`$CS_GAME_DIR`, read-only) and the owner-supplied decrypted engine image
(`$CS_ENGINE_IMAGE`, SHA-256 `43540fc9…c37d75`, read-only, nothing of it
committed). Implementer: **sonnet-2/sonnet-2**. Provenance is **static**
(AGENTS.md rule 3's owner exception: facts and addresses, no code). No original
executable ran, so nothing here is `verified_original`.

## Verdict

1. **The premise moved.** `britkestrel_1` was measured on
   `ZBD/C3/M03/zrdr.zbd`. The corrected campaign layout
   (`2026-10-17-m01-archive-decision.md`) binds that archive to work order
   **M03**; M13 is `ZBD/C2/M03`, whose record spells no such name
   (`2026-10-10-m13-b-compatibility-gaps.md`, re-pin table). The limitation is
   therefore recorded on **M03's** record, not M13's.
2. **Mechanism (resolved, static).** The `SET_AI_NET` consumer at `0x46a000`
   (168 bytes, SHA-256 `8625eebc…319abd`; first read by M17-B-FU1) looks the
   actor name up case-insensitively in the vehicle registry `0x71dab8`
   (`0x4aff10`, 76 bytes, `20788d19…fb58`), then in the zeppelin registry
   `0x71df80`, and on a second miss logs `SET_AI_NET: can't find vehicle or
   zeppelin %s` (string `0x626f18`, the only occurrence) and returns. So a name
   no registry holds is **logged and skipped**: no spawn, no faction, no error.
3. **The name is declared nowhere.** It occurs in exactly one installation file
   (`ZBD/C3/M03/zrdr.zbd`, the directive site itself, raw byte scan) and not in
   the image: the image holds no `britkestrel` literal. The only `kestrel`
   strings are an aircraft-type name table at `0x620d50` (`Kestrel`,
   `player_kestrel`, `pkestrel`, `rkestrel`, `wkestrel`, `kestrel`), none of
   which is `britkestrel_1`. The vehicle registry's insert in the record-driven
   vehicle constructor (46 bytes at `0x47c52d`, `64593949…c85b`) takes its name
   from a data record.
4. **Not settled (kept unknown).** Whether some other path registers
   `britkestrel_1` in either registry at run time. The registry has ~40
   referencing sites; only the record-driven insert was read, and the
   aircraft-type table's consumers were not traced. Until then the expectation
   "the pair is skipped and the node stays unassigned" is a static reading, not
   an observation. Resolver: **M03-C** (a reference run reading the log or
   observing the node) or a follow-up tracing every insert into `0x71dab8` /
   `0x71df80`. No implementation may invent an actor, spawn or faction.

The node operand `M3BritAce` resolves (chapter world's `neindex.zrd`).

## Recorded where consumers read it

`MISSION_SCOPED_UNKNOWNS["M03"]` in `crates/cs_content/src/campaign_bindings.rs`
gained a second entry and `missions/bindings/M03.json` was regenerated through
production (`regenerate_m13_b_fu1_…`, `CS_REGENERATE_M03_BINDING=1`). No other
work order carries it.

## Tests (`accept_m13_b_fu1_*`, in `crates/cs_app/tests/campaign/m13_b_fu1.rs`)

* `…_an_undeclared_actor_name_misses_in_the_set_ai_net_consumer` (retail+image)
* `…_the_derived_m03_record_carries_the_actor_limitation` (retail; byte-compares
  the committed `M03.json` with the derivation, and checks M13 lacks the name)
* `…_the_actor_limitation_is_an_unknown_of_m03_alone` (CI)

`m13_b_fu2.rs`'s M03 count assertion moved from one to two entries.
Evidence: `docs/findings/evidence/M13-B-FU1.json`.
