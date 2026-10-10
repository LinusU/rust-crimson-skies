# M19-B: Rescue the Black Swan's mission-specific compatibility surface

Date: 2026-10-10. Task: M19-B "Implement and regress mission-specific
compatibility gaps" (#313, `missions/M19.md`, work order `M19-B`; depends on
F38-C, F50-C and M19-A). Shared contract:
`docs/contracts/SCRIPT-MISSION.md`. Capabilities used: `retail`
(`$CS_GAME_DIR` read-only, never written), `synthetic` (newly authored `.zrd`
records). Implementer: **swe2-max-1/swe2-max-1** (Rally #313, session of
2026-10-10). Review is Rally's next step after submission; a merged task is
`checked` only, no agent review replaces the owner's human approval, and
nothing here is `verified_original` (AGENTS.md rule 8).

The stage's minimum acceptance scenario is *"All discovered mission-specific
behavior uses production engine systems and regression tests."* It holds for
everything this note measures: M19's discovered mission-specific behavior is
its mission control program — the typed keyed list the installation ships as
`objectives.zrd` inside `ZBD/C4/M04/zrdr.zbd` — and this stage runs it
through production systems (`SourceContext::control_program`,
`SourceContext::bind`, `cs_formats::script_raw::discover_container`,
`cs_content::stunts::decode_zrd`, `cs_content::mission_control`,
`cs_app::mission_control`, `cs_app::control_lowering`,
`cs_sim::objectives::address`'s convention as measured by
M02-B-FU3/M06-B-FU3) and regresses it with fourteen `accept_m19_b_*` tests.
M19's record lowers completely, so — as with M05-B, M08-B and M13-B — this
stage had no generic lowering gap to close and could spend its whole budget
on what is *different* at M19.

## What changed

No production code and no change to `missions/bindings/M19.json`: the record
lowers completely, so there was no lowering gap to close and no binding
statement to correct. Added:

* `crates/cs_app/tests/campaign/m19_b.rs` — fourteen `accept_m19_b_*` tests
  (eleven retail, three synthetic);
* `crates/cs_app/tests/campaign/evidence/m19_b.rs` — the M19-B evidence
  harness, which also writes `m19-control-program.json` (the binding, the
  lowering accounting and the measured latch/closure graph) beside the
  report;
* `docs/findings/2026-10-10-m19-b-compatibility-gaps.md` (this note) and
  `docs/findings/evidence/M19-B.json` (the committed report copy).

Wiring only: `crates/cs_app/tests/campaign/main.rs` (`mod m19_b;`),
`crates/cs_app/tests/campaign/evidence.rs` (`mod m19_b;` plus the task list
in its module doc, which also gains the omitted M13-B).

## What was read from the installation

| Fact | Value |
| --- | --- |
| `install_sha256` | `b4e780ab…c631978` (same installation as M01-A … M19-A) |
| Reader archive | `ZBD/C4/M04/zrdr.zbd`, 61 137 bytes, SHA-256 `c59692a9…cbd6` (M19-A's own program span) |
| Members | 13; exactly one declares numbered blocks — `objectives.zrd`, the 8th member, offset 19 740, 28 323 bytes, SHA-256 `907df449…8c96` |
| Control record | 108 numbered blocks, 440 directive sites, 36 distinct keys — **the largest measured record in the census**, on blocks and on sites — no block refusal, no record key outside the measured vocabulary |
| Dispositions | 34 measured, 2 terminal (`INSTANTWIN` block 30, `INSTANTLOSS` block 13), **0 unmeasured, 0 refused** |
| Lowering | mission `mission/ch4-m04`, 108 `RawObjective`s, all 440 calls bound, all 108 conditions lowered, `MissionProgram::validate` clean, 0 unmet requirements, census row **complete** |
| Campaign | `zbd/c4/m04` joins the census's complete rows; `campaign_ready()` is still false (other rows carry their own gaps) |
| Record-level fields | `MISSION_TIMER [0]`; `PLAYER_INIT [1, [-6290, 1350, -10393], [0, -125, 0], 0.8, 180]`; the three animation lists `RESTORE/EXECUTE/INVALIDATE_ANIMS` |
| Whole installation | every `.zbd` container decoded member by member for the name-resolution table and the 111-text coverage claim |

## The three regression priorities, as the record spells them

**World unlock conditions.** Ten blocks carry no `BEGIN_DORMANT` and watch
from the mission's first tick — `{2, 5, 9, 14, 18, 20, 21, 25, 39, 60}`: the
first-hit detector 2, the primary turret count 5, both damage-ladder starts 9
and 14, the fortress turret count 18, the support watchers 20 and 21, the
zeppelin's in-play bit 25, the helium tank 39 and the half-turret sound 60.
Block 1 is the record's **only** armed clock (`BEGIN_DORMANT 2.0`: the
zeppelin-turret wake and the start sound). The three `TICK_DEPENDS_ON_OBJ`
gates — `23 → 24`, `65 → 106`, `91 → 103` — lower as the dependency's own
`ObjectiveAwake` conjunct: the payback block 23 evaluates only while the
`piratezep`-inactive watcher 24 is awake, and the warhawk (65) and brigand
(91) launch waves are gated on their own zeppelin watchers 106 and 103. The
fortress's targets move stage by stage through `ADD_/REMOVE_OBJECTIVE_TARGET`
and `ADD_/REMOVE_OTHER_TARGET`: block 18 adds the hook pair
`[player_bmhook, bm_hook]`, block 5 removes the one `OTHER` target, and the
rail watchers 27/46 and dock block 26 remove targets as the stages close.
What the engine *does* at each boundary — wrong actor, wrong session,
repeated event — is a runtime observation and stays unmeasured here (M19-C).

**Player-aircraft transfer.** M19's 36-key vocabulary contains **no**
directive that transfers an aircraft, re-seats a pilot or changes an
allegiance — the suite asserts that against the exact key list, not against a
guess at spellings. The transfer is spelled as *animation state and target
flags*: block 18 (undormant, counting eight fortress-turret chains) completes
into `ADD_OBJECTIVE_TARGET [player_bmhook, bm_hook]` and wakes block 19 and
the rail watcher 46; block 19 (`PRIMARY` slot 3, `MSG_BRF_RMM4_OBJ3`) arms
`activate_bmhookup_node` through `WAKE_ANIM` and completes only when
`ANIM_STATE` reports `bm_hookup_player` `EXECUTED`, then removes the hook
targets, naps the dock response 22, wakes the rail watcher 27 and the
secondary marker 78, and kills `{20, 21, 61, 62, 63, 64}` — closing the
support watchers and the first launch stage it supersedes. Both animation
operands resolve outside the control member: `bm_hookup_player` and
`activate_bmhookup_node` in the chapter world's
`ZBD/C4/zrdr.zbd/bhmhookup.zrd`, and the hook targets in this archive's own
`targets.zrd`. The synthetic suite carries the two mechanisms into CI: the
measured parse takes the **first** `ANIM_STATE` text of a block (a repeated
site is never reached — M19's authored answer to a repeated event), and a
`STATE` token outside `RUNNING/EXECUTED/INVALID` drops its pair rather than
arming a fourth state.

**Ally extraction.** The win predicate *is* the extraction: block 30
(`PRIMARY` slot 5, `MSG_BRF_RMM4_OBJ5`) carries the record's one
`INSTANTWIN`, wakes `pzhomebase` on entry and completes only when `ANIM_STATE
hooked_to_klondike` reports `EXECUTED` — both names declared by the shared
`ZBD/zrdr.zbd/pzep_hookup.zrd`. Its one completion edge is block 49's nap,
behind the chain `19 → 22 → 23 → 28 → 29 → 49` (hookup → dock response →
payback under the zeppelin-watcher gate → mansion primary → escort depletion
`DEDG [2,0]` → the cargo/hand-off stage whose `SET_HELP_LABEL` and
`COMPLETED_SOUND_GROUP` sites are spelled there). The success latch is itself
**evaluated** — unlike M13's wake-only latches — and `klondike`, the rescued
Black Swan, is named by no `.zrd` text anywhere in the installation: the ally
exists to the objective program only inside the animation name
`hooked_to_klondike`.

**The failure side is measured and state-dependent.** Four chains watch the
rescue's fragile states, each spelling its own member lists: the gasbag-panel
ladder `9 → 10 → 11 → 12` over the zeppelin's six
`piratezep/gasbag<n>/panels` chains at `INACTIVE_COMPLETION_COUNT` **1, 2, 3,
4**; the engine ladder `14 → 15 → 16` over the twelve `reng/leng …/healthy`
chains at **3, 5, 7**; the hangar supports 20 (sounds at 1 of 4 — a warning)
and 21 (spells no count, so the measured default is its own four members);
and the helium tank 39. The worst rung of each — 12, 16, 21, 39 — naps the
one `INSTANTLOSS` latch 13 after the spelled 20 s and kills the same nine
primary blocks `{5, 6, 7, 17, 18, 19, 28, 30, 38}` (39 also kills the dock
target 48), so a failure ends the primaries rather than leaving them
dangling. And the goods primary 17 kills the helium-tank watcher 39 while
waking block 26, a second watcher on the **same**
`[bhf_heliumtank1, tank1_healthy]` chain that removes the `bhf_dock` target:
one world event is a mission failure before the goods stage and the dock
release after it, spelled as a kill plus a re-watch, never a flag.

**Latches, closures and addresses.** Both terminal blocks are dormant
(`BEGIN_DORMANT -1`), bare and one site each: block 13's only completion-edge
predecessors are the four failure rungs' naps, block 30's is block 49's nap —
asserted with kill edges deliberately excluded (every failure rung also
*kills* block 30; killing it does not enter it). Over wake/nap edges plus the
three gates the success closure is thirteen blocks `{5, 6, 7, 17, 18, 19, 22,
23, 24, 28, 29, 30, 49}` — the dock chain `5 → 6 → 7 → 17` whose last nap
re-arms watcher 24 feeding the win path — the failure closure ten `{9, 10,
11, 12, 13, 14, 15, 16, 21, 39}`, the two disjoint, and the remaining 85
blocks named by neither latch. Whether any of the 85 is an *optional reward
or stunt branch* is **not** measured — M19-A binds no stunt or reward ids.
The condition split: 64 blocks lower to a bare `ObjectiveAwake` (wake-only),
44 spell an evaluator or a gate. All 147 spelled block addresses (41 wake, 47
kill, 56 nap, 3 gates) lie in `1..=108`, none is `0`, and block 25's nap
spells `108` — the block count itself, the value a zero-based reading would
report out of range. The convention is M02-B-FU3 (#802) and M06-B-FU3
(#819)'s measurement, re-derived here on M19's own record, not re-decided.

**Names.** The record's sites spell 111 distinct texts; the suite asserts
every one is declared by some `.zrd` member outside M19's control member
except a measured set of five — the `MSG_BRF_RMM4_OBJ*` briefing operands of
the `IDENTITY` sites (the same record-only status M13's `MSG_BRF_HAM3_OBJ*`
operands hold). Three resolution scopes cover the rest: this archive's own
members (`piratezep`, `M4Piratezep`, `zepgetcargo`, the hook and dock
targets), the chapter-4 world (`bhmhookup.zrd`, `bhm_warhawks.zrd`,
`bhf_dockdoors.zrd`, `bhf_docktankboom.zrd`, `bhf_hangarboom.zrd`,
`neindex.zrd`), and the shared `ZBD/zrdr.zbd` (`pzep_hookup.zrd`,
`pzep_cargo.zrd`). Two footnotes are measured: `b_turret1`/`b_turret2` — the
balloon turrets the `INACTIVE` chains spell literally — are declared by no
member literally; the declaration anywhere is the `b_turret*` wildcard (this
archive's `aiv.zrd`, the shared turret records), the one-digit-consuming
wildcard of finding C. And M19 adds a two-star spelling the earlier suites
did not see: `WAKEUP_TURRETS` carries `aagun**`, `8igun**` and `t_truck**` as
spelled data — the wildcard match is a runtime rule, and the lowering passes
the spellings through.

## What is not claimed

`verified` stays `false` and every checklist entry M19-A left unknown is
still listed in `missions/bindings/M19.json` — the control program being
measured does not answer difficulty branches, outcome precedence at runtime,
media, rewards or `closure_sha256`. No mission was played, no original
executable was run, no presentation, audio or visual row was reviewed: those
are M19-C, which requires `human_play` and the owner. The wrong-actor /
wrong-session / repeated-event halves of the sheet's priorities are runtime
observations. The campaign stays not ready on this stage's evidence.

## Test inventory (`accept_m19_b_*`, 14 tests: 11 retail, 3 synthetic)

Retail, all `#[ignore = "requires CS_GAME_DIR"]`:

1. `…_the_control_program_is_the_member_that_declares_the_blocks` — census,
   independent walk, production control binding and M19-A's mission binding
   all name one archive, one member, one span and two digests re-derived from
   bytes; the block declaration, not the coincidental length, selects the
   member.
2. `…_every_directive_m19_spells_has_a_disposition_and_none_is_refused` — the
   exact 36-key vocabulary, 34 measured + 2 terminal, sites summing to 440.
3. `…_the_sheet_priorities_resolve_to_measured_operations_and_none_transfers`
   — operations for the three priorities plus the absence of any transfer or
   allegiance key.
4. `…_the_hookup_transfer_and_the_extraction_are_record_data` — blocks
   18/19/27/46/30 as spelled, the hook target pair's add and removes, both
   `ANIM_STATE` predicates lowered, every animation and target name resolved
   to its declaring member.
5. `…_the_unlock_gates_and_the_watchers_are_record_data` — the ten undormant
   watchers, the one armed clock, the three gates lowered as `ObjectiveAwake`
   conjuncts and their piratezep-chain watchers.
6. `…_the_failure_watchers_share_one_latch_and_the_kill_lists_match` — both
   ladders with chains and thresholds, the support default, the four 20 s
   latch naps, the identical kill lists, and the kill-plus-re-watch that
   moves the helium tank from failure guard to dock release.
7. `…_every_actor_the_record_names_resolves_in_the_shipped_data` — the
   declaration table across the three scopes; then, because a table is a
   sample, the one-pass installation index asserting all **111** spelled
   texts resolve outside the control member except the five measured
   `MSG_BRF_RMM4_OBJ*` operands, the wildcard footnote and `klondike`
   resolving nowhere.
8. `…_the_terminal_blocks_are_gated_and_every_address_is_in_range` —
   latches, 147 addresses, the discriminating `108`, the latch edge sets.
9. `…_the_two_outcomes_have_disjoint_prerequisites_and_the_rest_is_not_mandatory`
   — prerequisite closures, kill-is-not-a-prerequisite, the 64/44 condition
   split.
10. `…_every_call_binds_every_condition_lowers_and_m19s_record_is_the_largest_complete`
    — bindings, validation, unmet rows, the census-max claim and the retail
    `INACTIVE_COMPLETION_COUNT` defaults.
11. `…_m19_is_complete_and_the_campaign_stays_unready`.

Synthetic, run in CI without original data:

1. `…_the_hookup_anim_state_lowers_and_a_second_site_is_inert` — M19's own
   `ANIM_STATE` spelling lowers to the one-pair predicate; a second site is
   never reached; an out-of-vocabulary state drops its pair.
2. `…_a_zero_dependency_arms_no_gate_and_the_wildcards_arrive_as_data` — a
   spelled `0` lands on the no-dependency sentinel; a spelled `1` gates on
   the first block (`child0 − 1`); the one- and two-star turret wildcards
   bind as spelled data.
3. `…_an_ungrounded_transfer_directive_is_refused_rather_than_honoured` — an
   authored `TRANSFER_PLAYER_TO_HOOK` (a key M19 never spells) stays
   unmeasured, refuses its site by name as an unknown host call, stands no
   program and leaves the record incomplete.

## Checks

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_m19_b_ --include-ignored` | 0 (14 tests) |
| `python3 tools/validate_evidence.py private/evidence/M19-B/acceptance.json --artifact-root private/evidence/M19-B --require-pass` | 0 |

## Recorded unknowns (not guessed)

- **The runtime halves of all three priorities** — what the engine does at
  each unlock boundary, whether the wrong actor or a repeated event can
  satisfy a block, whether a hookup staged twice re-arms, and which of the 85
  outside blocks is an optional branch — are runtime observations for
  **M19-C** (`human_play`, owner-gated).
- **`klondike`'s runtime referent** — no `.zrd` member names it; whatever the
  engine attaches the `hooked_to_klondike` animation to (a node, a spawned
  actor, the zeppelin's own hook state) is unmeasured here.
- **`b_turret1`/`b_turret2` under the wildcard** — the `b_turret*`
  declaration is measured; which world entities the runtime match covers, and
  whether the one-digit-consuming wildcard of finding C also covers
  `b_turret10+` spellings, is a runtime question.
- The join remains M19-A's inference; no campaign definition record or
  original run was observed.

## Sources

`$CS_GAME_DIR` read-only through `cs_assets::install::discover`,
`cs_formats::script_raw::discover_container`,
`cs_content::stunts::decode_zrd`; `missions/M19.md`;
`docs/contracts/SCRIPT-MISSION.md`, `docs/contracts/CLI-EVIDENCE.md`;
`docs/findings/2026-10-02-m19-a-source-binding.md`,
`docs/findings/2026-10-10-m13-b-compatibility-gaps.md`,
`docs/findings/2026-10-10-m16-b-compatibility-gaps.md`,
`docs/findings/2026-10-10-m06-b-fu3-objective-address-convention.md`.
