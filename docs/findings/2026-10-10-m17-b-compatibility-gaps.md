# M17-B: The Pirate's Duel's mission-specific compatibility surface

Date: 2026-10-10. Task: M17-B "Implement and regress mission-specific
compatibility gaps" (#307, `missions/M17.md`, work order `M17-B`). Shared
contract: `docs/contracts/SCRIPT-MISSION.md`. Capabilities used: `retail`
(`$CS_GAME_DIR` read-only, never written), `synthetic` (newly authored `.zrd`
records). Implementer: **bunny-2/bunny-2** (Rally #307, session of
2026-10-10). Nothing here is `verified_original`: no original executable was run
and no mission was played (AGENTS.md rule 8).

The stage's minimum acceptance scenario is *"All discovered mission-specific
behavior uses production engine systems and regression tests."* It holds for
everything this note measures: M17's discovered mission-specific behavior is its
mission control program — the typed keyed list the installation ships as
`objectives.zrd` inside `ZBD/C4/M02/zrdr.zbd` — and this stage runs it through
production systems (`cs_app::mission_control::survey_mission_control_programs`,
`read_control_member`, `SourceContext::control_program`, `SourceContext::bind`,
`cs_content::mission_control::measure_control_record`,
`cs_app::control_lowering::lower_control_record`,
`cs_app::world::triggers::survey_retail_trigger_volumes`,
`cs_app::mission_start::recover_retail_start_configuration`,
`cs_formats::script_raw::discover_container`,
`cs_content::stunts::decode_zrd`) and regresses it with eleven
`accept_m17_b_*` tests. Three measured gaps are left **recorded, not worked
around**: see *The measured gaps* below.

## What changed

No production code and no change to `missions/bindings/M17.json`: M17's record
lowers completely, so there was no lowering gap to close and no binding
statement to correct. Added:

* `crates/cs_app/tests/campaign/m17_b.rs` — eleven `accept_m17_b_*` tests
  (nine retail, two synthetic);
* `crates/cs_app/tests/campaign/evidence/m17_b.rs` — the M17-B evidence
  harness, plus its one `mod` line in `evidence.rs`;
* `docs/findings/2026-10-10-m17-b-compatibility-gaps.md` (this note) and
  `docs/findings/evidence/M17-B.json` (the committed report copy).

Wiring only: `crates/cs_app/tests/campaign/main.rs` (`mod m17_b;`).

## What was read from the installation

| Fact | Value |
| --- | --- |
| `install_sha256` | `b4e780ab…c631978` (same installation as M01-A … M17-A) |
| Reader archive | `ZBD/C4/M02/zrdr.zbd`, 31 647 bytes, SHA-256 `f21d5b6a…4786d` (M17-A's own program span) |
| Members | 12; exactly one declares numbered blocks — `objectives.zrd`, the 8th member, offset 13 070, 9 552 bytes, SHA-256 `a343b470…05e41`. The one member longer than it is `aiv.zrd` (10 843 bytes), which declares none |
| Detection-zone member | `dzones.zrd`, offset 10 843, 529 bytes, SHA-256 `517a6b6f…e99f6` — the span the census **and** the trigger survey both report |
| Control record | 38 numbered blocks, 135 directive sites, 22 distinct keys, no block refusal, no record key outside the measured vocabulary |
| Dispositions | 21 measured, 1 terminal (`INSTANTWIN`, block 14), **0 unmeasured, 0 refused** |
| Lowering | mission `mission/ch4-m02`, 38 `RawObjective`s, all 38 conditions lowered, all 135 calls bound, `MissionProgram::validate` clean, 0 unmet requirements, census row **complete** |
| Campaign | `zbd/c4/m02` is one of the census's complete rows; `campaign_ready()` is still false (other rows carry their own gaps) |
| Record-level fields | `MISSION_TIMER [0.0]`; `PLAYER_INIT [1, [-5629, 595, -6860], [0, 85, 0], 0.8, 180]` — the same position and heading the archive's own `aiv.zrd` spells for `player`; the three animation lists are empty; no record-level sound key |
| Aircraft table | 14 records: `player` (field 0 = the none value `0xFFFFFFFF`), `bhatgyro_1`, `bswingman_1`, `bhatbrigand_1..3` and `bhatbrigand_5..12`; no `wingman_<n>` record, no `ia.zrd` member |
| Start configuration | `recover_retail_start_configuration("zbd/c4/m02")` → stored pose `[-5629, 595, -6860]`, heading `85`; airframe `airframe/player_pfighter` (campaign chain row 5, measured engine state) |
| Zone survey | 80 numbered zones over the world containers, 15 of them in `c4` (`dzpath1…15`); M17 declares all 15 names, **no declaration gap** |
| Whole installation | one `.zrd` text index (16 040 distinct texts) plus a raw-byte scan used for the undeclared-name claims |

## The three regression priorities, as the record spells them

**Forced-airframe control.** The record writes **nothing** on the player. The
walk that proves it is recursive, so a nested operand could not hide: exactly
four sites spell `player`, all four are block 31–34's `TRAVELERS` subject
(`player`, `APPROACHING`, `bhatgyro_1`, 500, 1), and no record-level field
spells any text at all. The measured operation set of all 21 non-terminal keys
is pinned as a set of 21 variants; the only one that moves an object is
`WarpVehicle`, whose single site names `bhatgyro_1` and four warp points (the
last labelled `pp5`). The airframe the mission launches in is *not* in this
mission's data: `cs_app::mission_start` reads M17's own `aiv.zrd`, whose player
record carries the none value in field 0 and no airframe field, finds no
`ia.zrd` whose `player_plane` could decide it instead, and binds
`airframe/player_pfighter` from the measured campaign chain — not the
autogyro the sheet's discovery cue names. The cue stays a research label: no
measured record assigns an airframe to M17's player, and what the original does
at a forced-airframe transition is runtime behaviour (M17-C).

**Search triggers.** Exactly four blocks start awake — 2, 3, 4 and 6, the four
with no `BEGIN_DORMANT` (34 blocks carry one; two of those arm a clock, at 2 s
for block 1 and 5 s for block 23, the other 32 are woken by edges). Each of the
four spells `DANGER_ZONES_COMPLETION_COUNT 1` beside `DANGER_ZONES_COMPLETED`
over its own zones (`dzpath7/8/9`, `dzpath3/4`, `dzpath11`, `dzpath13`),
retires one objective target (`dz8`, `dz3`, `dz11`, `dz13`), wakes two more
blocks (18+31, 19+32, 5+33, 29+34) and naps the matching marker (35–38) for two
seconds. The second half is the four identical `TRAVELERS` proximity sites,
radius 500 about `bhatgyro_1`, each waking block 24 and killing its own marker.
The zone names are cross-checked by the **production** trigger-volume survey:
M17's `MissionZoneDeclaration` is this archive's own `dzones.zrd` (the same
offset, length and whole-archive digest the census reports), states its three
keys in stored order (`disable` `dzpath15`, `nosnapshot` `dzpath12`, then 13
`objective_numbers` pairs `dzpath1→18` … `dzpath14→30`), and every name it
states has a node in `ZBD/C4/zrdr.zbd`, which carries exactly the fifteen
`dzpath` nodes. The pairs' **meaning** stays unmeasured (`cs_content::world`
says so too): nothing here claims which objective an integer indexes.

**Ace encounter lifecycle.** `bhatgyro_1` is netted at block 15
(`{bhatgyro_1, M2Blacke}`), woken at 17 (which also wakes 8 and 25 and naps the
net site five seconds later), warped at 23 — a five-second clock and one of four
spelled points chosen at random — and approached at 31–34. The six `SET_AI_NET`
sites, the wake, the warp and the approach are pinned site by site. The node
`M2Blacke` resolves to exactly one declaration: `ZBD/C4/zrdr.zbd`,
`neindex.zrd` — the chapter world's node index, the same shape as M13's
`M3BritAce`. The named warp label `pp5` resolves to exactly one declaration
too, and it is another mission's archive (`ZBD/C2/M02/zrdr.zbd`, `aiv.zrd`).

## Full-coverage claim behind the gaps

The control document spells **128** distinct text nodes (the whole document,
keys and operands alike). Every one of them is declared by some `.zrd` member
outside M17's control member **except nine**, and the suite pins the exception
set exactly:

| Text | Kind |
| --- | --- |
| `WARP_VEHICLE` | a **directive key** no other archive in the installation spells (raw scan: one file, this one) |
| `bhatbrigand_13`, `bhatbrigand_14` | `SET_AI_NET` operands of blocks 20 and 30; **no** `.zrd` member anywhere, and a raw-byte scan finds each in exactly one file — the directive site that spells it |
| `MSG_BRF_RMM2_OBJ1`, `MSG_BRF_RMM2_OBJ2` | the `IDENTITY` operands of blocks 24 and 8; no other `.zrd` member spells them (M13-B measured the same shape for `MSG_BRF_HAM3_OBJ*`) |
| `snd_c4-RM-m2_Zachary_13`, `snd_c4-RM-m2_Zachary_15`, `snd_c4-RM-m2_Blacke_14`, `snd_c4-RM-m2_Blacke_16` | four of the six `STOP_QUEUED_SOUNDS` operands; the other two (`Zachary_11`, `Blacke_12`) are declared by `ZBD/zrdr.zbd`, `sounds.zrd` |

The counter-example the suite keeps beside that claim: `piratezep` occurs in
114 files of the installation, so the raw scan distinguishes a declared name
from an undeclared one rather than always answering "absent".

A softer class than the nine, recorded so it is not mistaken for one of them:
two operand names are declared **outside M17's archive only** — `pzhomebase`
(14 members elsewhere, including `ZBD/zrdr.zbd#pzep_hookup.zrd`) and
`hooked_to_klondike` (23 members elsewhere, including the chapter's own
`ZBD/C4/zrdr.zbd#landings.zrd`) — and `bhatbrigand_4`, the fourth `SET_AI_NET`
operand of block 16, is declared by four sibling missions' aircraft tables but
by **not** this mission's.

## The terminal structure

There is exactly one terminal outcome in the record: `INSTANTWIN` at block 14.
`INSTANTLOSS` is a key the original knows (`terminal_outcome_of` answers it) but
M17's record spells it nowhere, so **the control program contains no failure
cause at all**: the failure half of `M17-FAILURE` is outside this member and
stays unknown here rather than being invented from a walkthrough. All 48
spelled addresses (21 wake, 12 kill, 14 nap, 1 dependency gate) lie in `1..=38`
under M02-B-FU3's measured one-based convention; the success latch's
prerequisite closure is the 17 blocks `2, 3, 4, 6, 8, 9, 10, 11, 12, 13, 14,
17, 24, 31, 32, 33, 34`, 21 blocks lie outside it, and killing a block is not a
path into it — block 2 has no wake or nap predecessor at all.

## What is not claimed

`verified` stays `false` and every checklist entry M17-A left unknown is still
listed in `missions/bindings/M17.json` — the control program being measured does
not answer difficulty branches, failure/success precedence, media, rewards or
`closure_sha256`. No mission was played, no original executable was run, no
presentation, audio or visual row was reviewed: those are M17-C, which requires
`human_play` and the owner. The wrong-actor / wrong-session / repeated-event
halves of the sheet's priorities are runtime observations. The campaign stays
not ready on this stage's evidence.

## Test inventory (`accept_m17_b_*`, 11 tests: 9 retail, 2 synthetic)

Retail, all `#[ignore = "requires CS_GAME_DIR"]`:

1. `…_the_control_program_is_the_member_that_declares_the_blocks` — census,
   independent walk, production control binding and M17-A's mission binding all
   name one archive, one member, three spans and four digests re-derived from
   bytes (whole archive, control member, detection-zone member).
2. `…_every_directive_m17_spells_has_a_disposition_and_none_is_refused` — the
   exact 22-key vocabulary with its site counts summing to 135, 21 measured +
   1 terminal, nothing unmeasured or refused, the five record-level fields and
   the empty record-sound list.
3. `…_no_directive_writes_the_player_and_the_airframe_is_the_campaign_chains` —
   the recursive `player` walk (four sites), the record fields spelling no
   text, the one warp site, the pinned operation set, the absent `ia.zrd`, the
   production start configuration (player record, pose, airframe) and
   `PLAYER_INIT` re-derived from it.
4. `…_the_search_triggers_are_the_four_always_awake_danger_zone_blocks` — the
   four always-awake blocks and their sites, the two clocks, the four proximity
   sites and the four markers, then the production declaration, its stored key
   order, its 15 names, the empty gap list and the chapter container's 15
   nodes.
5. `…_the_ace_lifecycle_is_record_data_and_two_actors_are_declared_nowhere` —
   the six net sites, the node, the wake, the warp (four points, `pp5`), the
   approach and the primary, then the 17-row declaration table and the raw-byte
   scan for the two undeclared actors.
6. `…_every_text_the_record_spells_is_declared_outside_it_or_recorded_as_a_gap`
   — the 128-text count and the exact nine-text exception set, with both halves
   of "record-only" checked.
7. `…_the_single_terminal_latch_is_gated_and_every_address_is_in_range` — one
   `INSTANTWIN`, no `INSTANTLOSS`, the 48-address budget per key, the range, the
   gate, the closure and the three `IDENTITY` sites.
8. `…_every_call_binds_every_condition_lowers_and_m17s_record_completes` —
   mission id, 38 objectives, 38 lowered conditions, 135 bound calls, no
   unbound key, clean validation, zero unmet rows, complete census row.
9. `…_m17_is_complete_and_the_campaign_stays_unready` — the complete row, the
   measured row, an incomplete sibling and the shut campaign gate.

Synthetic, run in CI without original data:

1. `…_a_warp_site_binds_at_its_measured_shape_and_refuses_a_value_the_ir_cannot_carry`
   — the retail warp shape measured and bound into a validating program; the
   same site with a non-finite coordinate refused by name, its block's condition
   damaged to a refusal and `MissionProgram::validate` carrying an error.
   (`WARP_VEHICLE` is spelled by no other archive, so this member is where its
   arms run in CI. A *shorter* site is not a refusal arm: the registry's
   signatures come from the record's own measured shapes, so a truncated record
   simply measures a shorter shape — measured while writing this stage.)
2. `…_the_zone_evaluator_pairs_with_its_threshold_and_an_unknown_key_refuses`
   — the paired zone evaluator lowers and binds; the threshold alone still
   stands because the count key is itself measured; an unknown key refuses
   exactly its own site by name, leaves the three measured sites bound and
   produces no program.

## Checks

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_m17_b_ --include-ignored` | 0 (11 tests) |
| `python3 tools/validate_evidence.py private/evidence/M17-B/acceptance.json --artifact-root private/evidence/M17-B --require-pass` | 0 |

## Recorded unknowns (not guessed)

- **`bhatbrigand_13` and `bhatbrigand_14`** — `SET_AI_NET` operands of blocks
  20 and 30 that no file in the installation declares (decoded index and raw
  byte scan, above); what the original resolves them to is unknown. Together
  with the four undeclared `snd_c4-RM-m2_*` queued-sound names, the two
  record-only `MSG_BRF_RMM2_*` ids and the cross-archive `pp5` label, this is
  the stage's resolution backlog. Resolving task: **M17-B-FU1** (static reading
  in the owner's decrypted image and/or an original reference run under M17-C).
- **No failure cause in the control program.** `INSTANTLOSS` is spelled
  nowhere, so every discovered `M17-FAILURE` cause (player destruction, wingman
  loss, timer) is outside this member and unmeasured here; the sheet's
  `M17-FAILURE` row needs an ordinary-play observation. Resolving task:
  **M17-C** (`human_play`, owner-gated).
- **The runtime halves of all three priorities** — what happens at the
  zone-flag boundary, whether the wrong actor or a repeated event can satisfy a
  block, whether the warp point is chosen as measured — are runtime
  observations for **M17-C**.
- **The `objective_numbers` pairs' meaning** — which objective the integers 18
  …30 index — is unmeasured in `cs_content::world` and stays unmeasured here.
- **The `forced autogyro` discovery cue** — no measured M17 record assigns an
  airframe; the production chain gives `airframe/player_pfighter`. Whether some
  undecoded program or original run forces a different airframe is unknown.
- The join remains M17-A's inference (`ClaimStatus::Inferred`); no campaign
  definition record or original run was observed.

## Sources

`$CS_GAME_DIR` read-only through `cs_assets::install::discover`,
`cs_formats::script_raw::discover_container`, `cs_content::stunts::decode_zrd`;
`$CS_ENGINE_IMAGE` read through `cs_app::mission_start` for the campaign
airframe row; `missions/M17.md`; `docs/contracts/SCRIPT-MISSION.md`,
`docs/contracts/CLI-EVIDENCE.md`;
`docs/findings/2026-10-10-m13-b-compatibility-gaps.md` (the stage this one
follows),
`docs/findings/2026-10-09-m07-b-compatibility-gaps.md`,
`docs/findings/2026-10-10-m12-b-compatibility-gaps.md`,
`docs/findings/2026-10-06-m01-lc-directive-c-ai-world-and-animation-directives.md`
(`WARP_VEHICLE`, `SET_AI_NET`, `TRAVELERS`).
