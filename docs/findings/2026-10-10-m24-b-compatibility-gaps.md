# M24-B: Battle over Broadway's mission-specific compatibility surface

Date: 2026-10-10. Task: M24-B "Implement and regress mission-specific
compatibility gaps" (#328, `missions/M24.md`, work order `M24-B`). Shared
contract: `docs/contracts/SCRIPT-MISSION.md`. Capabilities used: `retail`
(`$CS_GAME_DIR` read-only, never written), `synthetic` (newly authored `.zrd`
records), and the owner-supplied engine image `$CS_ENGINE_IMAGE` (read-only,
never committed). Implementer: **bunny-alpha-1/bunny-alpha-1** (OpenCode Space
Bunny Free, `opencode-go/mimo-v2.6-flash`, Rally #328 implement claim of
2026-10-10). Nothing here is `verified_original`: no original executable was
run and no mission was played (AGENTS.md rule 8).

The stage's minimum acceptance scenario is *"All discovered mission-specific
behavior uses production engine systems and regression tests."* It holds for
everything this note measures: M24's discovered mission-specific behavior is
its mission control program — the typed keyed list the installation ships as
`objectives.zrd` inside `ZBD/C5/M04/zrdr.zbd` — and this stage runs it
through production systems (`cs_app::mission_control::
survey_mission_control_programs`, `SourceContext::control_program`,
`SourceContext::bind`, `cs_content::mission_control::
measure_control_record`, `cs_app::control_lowering::lower_control_record`,
`cs_app::world::triggers::survey_retail_trigger_volumes`,
`cs_content::coordinates::load_engine_image`,
`cs_formats::script_raw::discover_container`, `cs_content::stunts::
decode_zrd`) and regresses it with eleven `accept_m24_b_*` tests. Two
measured gaps are left **recorded, not worked around**: see *The measured
gaps* below.

## What changed

No production code and no change to `missions/bindings/M24.json`: this stage
measures, it does not correct the binding. Added:

* `crates/cs_app/tests/campaign/m24_b.rs` — eleven `accept_m24_b_*` tests
  (eight retail, one engine-image, two synthetic);
* `crates/cs_app/tests/campaign/evidence/m24_b.rs` — the M24-B evidence
  harness, plus its one `mod` line in `evidence.rs` and that file's stage
  list;
* `docs/findings/2026-10-10-m24-b-compatibility-gaps.md` (this note) and
  `docs/findings/evidence/M24-B.json` (the committed report copy).

Wiring only: `crates/cs_app/tests/campaign/main.rs` (`mod m24_b;`).

## What was read from the installation

| Fact | Value |
| --- | --- |
| `install_sha256` | `b4e780ab…c631978` (same installation as M01-A … M24-A) |
| Reader archive | `ZBD/C5/M04/zrdr.zbd`, 62 785 bytes, SHA-256 `28d0076b…cfef4` (M24-A's own program span) |
| Members | 14: `aiv.zrd` (0, 18 561), `dzones.zrd` (18 561, 826), `egen.zrd` (19 387, 621), `location.zrd` (20 008, 16), `map.zrd` (20 024, 651), `mis_anim.zrd` (20 675, 2 316), `net.zrd` (22 991, 16), `objectives.zrd` (23 007, 19 590, 63 blocks — the control member), `startanims.zrd` (42 597, 238), `targets.zrd` (42 835, 710), `weather.zrd` (43 545, 3 319), `zeppelins.zrd` (46 864, 7 535), `miles_drop.zrd` (54 399, 2 678), `glidebomb.zrd` (57 077, 3 628) |
| Control member | `objectives.zrd`, offset 23 007, 19 590 bytes, SHA-256 `b0430f95…12da8`. Exactly one member declares numbered blocks. For M24 the control member is *also* the archive's longest — length would coincide with the rule here, recorded as a fact, not as the rule (position still cannot be it: the member is the eighth of fourteen) |
| Control record | 63 numbered blocks, 267 directive sites, 40 distinct keys, no block refusal, no record key outside the measured vocabulary |
| Dispositions | 37 measured, 2 terminal (`INSTANTWIN` block 21, `INSTANTLOSS` block 39), **1 unmeasured** (`SET_AI_`, block 19), 0 refused by the grammar |
| Lowering | mission `mission/ch5-m04`, 63 `RawObjective`s, all 63 conditions lowered, 260 of 267 calls bound, 7 refused, unbound `["`STOP_QUEUED_SOUNDS`: binding `STOP_QUEUED_SOUNDS`: too many arguments"]`, no program assembled, `validation` never runs, unmet `["call_arguments"]`, census row **not complete** |
| Campaign | `zbd/c5/m04` is a measured census row under `call_arguments` only; `campaign_ready()` is false |
| Record-level fields | `MISSION_TIMER [0.0]`; `PLAYER_INIT [1, [-10582, 250, -12909], [0, -45, 0], 0.8, 180]`; the three animation lists are empty |
| Zones | M24's `MissionZoneDeclaration` is this archive's own `dzones.zrd` (offset 18 561, 826 bytes, same container digest); the five zones the record spells (`dzpath28…32`) all have nodes in `ZBD/C5/zrdr.zbd` and appear in the declaration; **no declaration gap** |
| Start structure | 12 always-awake non-empty blocks (2, 3, 4, 5, 8, 9, 10, 14, 24, 25, 53, 62); four timed wakes (OBJECTIVE1 @2 s, 16 @5 s, 26 @7 s, 45 @120 s); five empty stubs (40–44), no edge reaches a stub; 46 blocks spell `BEGIN_DORMANT` |
| Graph | 90 cross-objective addresses (24 wake, 25 nap, 41 kill), all in `2..=63`; nothing ever addresses OBJECTIVE1; `TICK_DEPENDS_ON_OBJ`, `SLEEP_…`, `WAKE_OBJECTIVE` and `HIDE_OBJ` are spelled nowhere |
| Engine image | the directive-key table's measured extent `0x226040..0x2268c0` carries its 87 strings; every M24 key that lives in the table is present; the truncated `SET_AI_` is not an entry and is not a standalone string anywhere in the image (`SET_AI_NET\0` and `SET_AI_TEAM\0` are) |

## The three regression priorities, as the record spells them

**Capital battle transition.** A three-zeppelin chain. OBJECTIVE1 (a
two-second clock) wakes `piratezep` and `dantezep` turret nodes; blocks 2
and 3 are the player's own 2000/1500 approach gates on `dantezep`. Blocks 14
and 15 are the transition proper: `TRAVELERS dantezep APPROACHING
{piratezep, blackswanzep} 2000 1` beside `COMPLETED_ZEPCANNONS
[[dantezep,1],[<zep>,1]]` — the measured +0xc cannon write on both
zeppelins. The friendly capital falls through five spelled thresholds: five
gasbag panels at counts 1 and 2 (blocks 4/5), fourteen chained engine nodes
(`reng11…leng42`, `healthy`) at counts 7 and 12 (blocks 8/9), then
OBJECTIVE10 — the record's first PRIMARY, the three-panel gasbag threshold
itself — which retires `dantezep`'s objective-target flag, plays the fall
sound and kills ten blocks; its half-second nap of OBJECTIVE11 wakes the
gasbag animation and `WAKEUP_GENERATOR dantezep 1`. The enemy
`piratezep`'s six gasbag panels are watched at thresholds 1/2/3 (blocks
24/25/62, all always awake); the two-panel completion points it at the
retreat net `M4PZRetreat`. The final `blackswanzep` is woken with its
turrets (block 6), reinforced by its three Furies (block 27), and its Furies
are moved onto `M4Miles` in block 46. The three zeppelin names resolve
against this archive's own `zeppelins.zrd`; the five nets (`M4Attack`,
`M4Defend`, `M4PZRetreat`, `M4Miles`, `M4MilesRun`) resolve against the
chapter container `ZBD/C5/zrdr.zbd`. What the +0xc byte drives, and who
wins a window in which success and failure are both armed, stays runtime
(M24-C).

**Target eligibility change.** Five flag writes, and the record never adds a
target. `REMOVE_OBJECTIVE_TARGET` (measured: clear the leaf's +0x4d flag)
appears twice: `dantezep` in block 10 (the fall) and the chained
`[piratezep, rock_zeppelin]` in block 20. `ADD_OTHER_TARGET` (measured: set
the leaf's +0x4c flag) appears three times: the same chain in block 20 —
beside its own `DEDG 1 0` gate, so the swap happens in one completion — and
`blackswanzep` in blocks 23 and 46. **No `ADD_OBJECTIVE_TARGET` site exists
anywhere in the 267 sites**, so the eligibility this record changes only
decreases; the initial target set is not this member's data (`targets.zrd`
is a candidate home, unmeasured here). Which icon or label the target-info
layer draws from the flag is the disposition's own recorded unknown.

**Campaign ending.** One win and one loss latch, both nap-armed, and a
measured mutual exclusion. `INSTANTWIN` lives in OBJECTIVE21 alone (the
record's `SECONDARY 11` identity), starts dormant with no timed wake, and
its only incoming edge is `NAP 21 45` from OBJECTIVE28; the chain back from
28 is 13 → 35 → 47 → 34 → 11 → 10, so the win arms 45 seconds after a chain
whose root is OBJECTIVE10 — the friendly capital's fall. `INSTANTLOSS`
lives in OBJECTIVE39 alone, is armed by two naps (`38` at 15 s, `62` at
20 s — 62 is always awake, its three-panel condition arms the loss with no
predecessor), and is **killed** by OBJECTIVE34's kill list — the
`stihellhound_5_eg0` detour gate, which OBJECTIVE38 and OBJECTIVE62 in turn
kill (`KILL 34 35`), a spelled exclusion between the detour and the failure
path. The win latch is killed nowhere. M24 is the campaign's final row, so
nothing follows the win in the record or the layout; the ending's continuity
(profile, records, successor step) is M24-C's `human_play` evidence.

Test combinations, not only each in isolation: the campaign-ending test
walks the arming chains *through* the capital-fall root and the detour
exclusion, and the capital-battle test pins the target-eligibility writes
beside the thresholds that gate them.

## The measured gaps

1. **`STOP_QUEUED_SOUNDS` does not register** — the same gap M16-B, M02-B,
   M03-B and M06-B record. M24 spells it six times with 1, 9, 7, 8, 7 and 8
   names (blocks 5, 10, 13, 38, 59, 62); the 9-name shape (OBJECTIVE10's
   fall cleanup) exceeds `MAX_CALL_ARGS` (8), so the whole spec is refused
   and all six sites report `unknown host call` — even the one-name site.
   The original reads at most ten names (the M01-LC-D finding), so the
   nine-name site is faithful data the original truncates; carrying the
   name list as one list argument is the follow-up **M16-B-FU1** (#1252)
   already tracks.
2. **`SET_AI_` is a truncated spelling.** One site, OBJECTIVE19, carries the
   five `{actor, net}` pairs `SET_AI_NET` spells (`devastator_1/2`,
   `wingman_1/2/3` onto `M4Defend`). The engine-image member shows the
   spelling absent from the directive-key table the original parser looks
   names up in (and absent as a standalone string), so under the measured
   "unlooked-up keys are ignored" rule the original ignores it and the
   unmeasured disposition is the faithful one — exactly where M16-B found
   its four comment words. No finding states what it was meant to do.

Because of these two gaps no M24 program assembles, `call_arguments` is
unmet, the census row is not complete and the campaign gate stays closed.

## What is not claimed

`verified` stays `false` and every checklist entry M24-A left unknown is
still listed in `missions/bindings/M24.json` — the control program being
measured does not answer difficulty branches, failure/success precedence,
media, rewards, actors' declarations or `closure_sha256`. No mission was
played, no original executable was run, no presentation, audio or visual
row was reviewed: those are M24-C, which requires `human_play` and the
owner. The wrong-actor / wrong-session / repeated-event halves of the
sheet's priorities are runtime observations. The campaign stays not ready
on this stage's evidence.

## Test inventory (`accept_m24_b_*`, 11 tests: 8 retail, 1 image, 2 synthetic)

Retail, all `#[ignore = "requires CS_GAME_DIR"]`:

1. `…_m24s_control_program_is_bound_to_the_same_identities_as_its_mission_binding`
   — mission binding, control binding and census agree on `mission/ch5-m04`,
   `script/c5-m04-zrdr`, the archive (length + digest re-derived from
   bytes), the one block-carrying member (span + digest re-derived), the
   campaign position 23, and an independent discovery walk sees all 63
   blocks.
2. `…_the_measured_vocabulary_partitions_and_refuses_no_m24_key` — 63/267/40
   with site counts summing, the exact sorted 40-key vocabulary, the two
   terminal outcomes, `SET_AI_` the only unmeasured key (with an argument
   list, unlike M16's comment words), the five record fields once each, no
   unclassified record key.
3. `…_the_block_graph_is_closed_under_the_records_own_numbering` — numbered
   1..=63, the three directed keys, all addresses in `2..=63` (nothing
   addresses OBJECTIVE1), the two nap-armed latches, the kill-site table
   (17 sites; OBJECTIVE59's twelve-block list; the loss latch killed only by
   34), the dormant-timer table (four timed wakes), the five empty stubs
   with no incoming edge.
4. `…_the_capital_battle_transition_is_the_zeppelin_chain_the_record_spells`
   — the turret wake, the player gates, the two approach/cannon sites, the
   six fall thresholds with their chained member operands, the retreat net,
   the Black Swan reinforcement, plus the declaration cross-checks
   (`zeppelins.zrd`, chapter-container nets, the dzones declaration and the
   five `dzpath` nodes through the production trigger survey, no gaps).
5. `…_the_target_eligibility_changes_are_five_writes_and_the_record_never_adds_a_target`
   — no `ADD_OBJECTIVE_TARGET` key exists; the two retirements and three
   other-target marks pinned with operands, blocks and measured flag
   semantics; the swap block's `DEDG` gate and the two marks' waking edges.
6. `…_the_campaign_ending_is_one_win_and_one_loss_latch_with_measured_arming_chains`
   — the two latches with identities and dormancy, the win's single 45-second
   nap, the loss's two naps and one kill, the three prerequisite closures,
   the detour↔failure mutual exclusion, the five identity sites, and the
   final-row fact (no successor).
7. `…_m24s_record_does_not_lower_and_its_calls_refuse_by_two_named_gaps` —
   the census attempt row by row (mission id, 63 objectives, 63 lowered
   conditions, 267 calls, exactly seven refusals pinned by flat site index
   and re-derived `(block, position)` location, the one unbound key, no
   program, no validation, unmet `["call_arguments"]`, row not complete) and
   the six sites' name counts (the 9-name poison beside the one-name victim).
8. `…_m24_stays_unready_while_its_sound_cleanup_and_truncated_key_refuse` —
   the census reports M24 under `call_arguments` and only that; the
   campaign gate is shut; the mission binding stays unverified.

Engine image, `#[ignore = "requires CS_ENGINE_IMAGE"]`:

9. `…_the_truncated_set_ai_key_is_not_in_the_measured_directive_table` — the
   table's 87 entries, every applicable M24 key present (including
   `SET_AI_TEAM`, which M16 did not spell), the nap key at its measured
   address, and the truncated `SET_AI_` absent as an entry and as a
   standalone string while `SET_AI_NET\0` / `SET_AI_TEAM\0` are present.

Synthetic, run in CI without original data:

10. `…_one_oversized_sound_site_poisons_the_whole_sound_key` — M24's own six
    site sizes authored: at 1/9/7/8/7/8 every site refuses and the key is
    gone; at 1/8/7/8/7/8 the key registers and every site binds.
11. `…_the_truncated_set_ai_site_refuses_but_the_measured_net_site_binds` —
    the truncated name refuses exactly its own site as an unknown call with
    no binding error (an unmeasured key registers nothing); the same pairs
    under `SET_AI_NET` bind, validate clean and arrive as the one spelled
    list argument (the `AssignNet` list shape, M03-B-FU2 #810).

## Checks

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_m24_b_ --include-ignored` | 0 (11 tests; log of record `private/evidence/M24-B/cargo-test.log`) |
| `python3 tools/validate_evidence.py private/evidence/M24-B/acceptance.json --artifact-root private/evidence/M24-B --require-pass` | 0 (`structurally_valid: true`) |

## Mutation checks run and reverted (both fail as required)

| Mutation | Result |
| --- | --- |
| `crates/cs_app/tests/campaign/m24_b.rs` capital test `dzpath32` → `dzpath31` | `accept_m24_b_the_capital_battle_transition_is_the_zeppelin_chain_the_record_spells` FAILED at the spelled-zones set, exit 101 |
| `crates/cs_script/src/bindings/mod.rs` `MAX_CALL_ARGS` `8` → `9` (production) | `accept_m24_b_m24s_record_does_not_lower_and_its_calls_refuse_by_two_named_gaps` FAILED at the refusal count (1 refused instead of 7 — the sound key registers again), exit 101 |

Both were reverted and the eleven-test selection was rerun green afterwards.

## Rebase

`main` advanced during this stage (to `02b8c0f1`, the M19-B merge). The
rebase conflicted in exactly one file, `crates/cs_app/tests/campaign/
evidence.rs`'s stage list, resolved by naming both M19-B and M24-B; because
the rebase did not apply cleanly, the owner's 2026-10-01 lighter check set
did not apply and all four checks above were re-run **in full** on the
rebased tree. The acceptance run and the evidence report belong to that
tree: the report's `candidate_tree` is the tree of the commit that carries
the suite (`11e16529`), and the only later delta is this findings note and
the report's own copy under `docs/findings/evidence/`, neither of which the
acceptance suite reads.

## Recorded unknowns (not guessed)

- **The fate of the `SET_AI_` site in the original** — the image shows the
  spelling unlooked-up (inert text under the measured ignore rule), but no
  observation states what OBJECTIVE19 was *meant* to do with its five
  `M4Defend` pairs. Resolving task: **M24-C** (original reference run).
- **The operand declarations not cross-checked here** — the aircraft names
  (`bhatwarhawk_5_1…6`, `stihellhound_5_1…8`, `stihellhound_5_eg0`,
  `bsfury_5_1…3`, `devastator_1/2`, `wingman_1/2/3`), the sound-group and
  queued-sound names (`snd_c5-MN-m4_*`, `snd_MN4*`, `snd_MN4*Dead/Swan/
  Dante/Blackout…`), `MSG_BRF_NYM4_OBJ1`, `MSG_OBJ_DESTROY`, the animations
  (`call_bombs_away`, `milesdrop`, `killdtzep`, `all_dtzep_gasbags`) and the
  leaf `rock_zeppelin` were not audited against the whole installation's
  declarations. This stage cross-checked only the zeppelin names, the five
  nets and the five zones. Resolving task: **M24-B-FU1** (#1271, filed with
  this stage).
- **`STOP_QUEUED_SOUNDS` as one list argument** — the six sound-cleanup
  sites stay refused; the mechanism and its fix are **M16-B-FU1** (#1252).
- **The runtime halves of all three priorities** — who wins a window in
  which OBJECTIVE21 and OBJECTIVE39 are both armed, whether the wrong actor
  or a repeated event can satisfy a threshold, what the cannon byte and the
  target flags drive on screen — runtime observations for **M24-C**
  (`human_play`, owner-gated).
- **The initial objective-target set** — this record only ever retires
  eligibility; which actors start as objective targets lives outside the
  control member (`targets.zrd` is a candidate) and is unmeasured here.
- **The M24-A findings-doc title row** —
  `docs/findings/2026-10-02-m24-a-source-binding.md` says the confirmed
  localized row reads "Death on the Docks", while the committed inventory,
  binding record and the passing M24-A suite all carry "Battle over
  Broadway"; the doc row looks copied from another stage's note. Filed as
  **M24-A-FU1** (#1272); nothing in this stage depends on it.
- The join remains M24-A's inference (`ClaimStatus::Inferred`); no campaign
  definition record or original run was observed.

## Review (bunny-alpha-1/bunny-alpha-1, 2026-10-10)

Reviewer: **bunny-alpha-1/bunny-alpha-1** (Rally #328 review claim of
2026-10-10T17:06Z, OpenCode, `opencode-go/mimo-v2.6-Flash`), a separate review
session with fresh context that began from the task history, `missions/M24.md`,
`docs/contracts/SCRIPT-MISSION.md`, `docs/contracts/CLI-EVIDENCE.md` and the
branch diff — not from the implementation's conversation. Implementer:
**bunny-alpha-1/bunny-alpha-1** (same Rally agent name and model, implement
claim submitted at `aae6d8e2`), so this review is **not independent evidence**
under the owner's 2026-09-28 audit directive: it is a fresh-context
re-derivation plus a full re-run of every check. A Rally merge awards
`checked` only, no agent review replaces the owner's human approval, and
nothing here is `verified_original` or `release_approved`.

What the reviewer verified on the branch:

* only owner paths changed — `crates/cs_app/tests/campaign/` (`m24_b.rs`,
  `evidence/m24_b.rs` and the two `mod` lines plus the `evidence.rs` stage
  list) and `docs/findings/`: six text files, no protected path, no production
  crate, no `missions/bindings/M24.json`, no binary file, no original data;
* rebased onto `3a293d6e` (the M18-B merge) — git auto-merged the two wiring
  files without a conflict, but the incoming commits touch
  `campaign/main.rs` and `evidence.rs`, the same two files this branch
  touches, and this branch added commits of its own, so the owner's lighter
  check set never applied and the full four checks re-ran on the rebased tree;
* the pinned retail record facts were re-derived by hand from the raw-record
  dump the implementer's own exploration printed (kept in the private working
  directory, never committed): the
  40-key vocabulary with the site counts summing to 267, the seven refused
  sites and their `(block, position)` locations, the six sound sites' name
  counts (1/9/7/8/7/8), the five identity sites, the two latches and their
  arming/kill edges (`28→21` nap 45 s; `38→39` nap 15 s, `62→39` nap 20 s,
  `34→39` kill), both closures (`[10,11,13,28,34,35,47]` and
  `[10,11,37,38]`), the five `dzpath` zones and the 8
  `INACTIVE_COMPLETION_COUNT` sites — five for the friendly capital (blocks
  4/5/8/9/10) and three for the pirate (24/25/62); every claim matched,
  and the two that did not were counts *about* the notes, not about the data
  (see below);
* every retail test drives production code — `SourceContext::bind`,
  `SourceContext::control_program`, `survey_mission_control_programs`,
  `lower_control_record`, `measure_control_record`,
  `survey_retail_trigger_volumes`, `discover_container` + `decode_zrd`,
  `load_engine_image` — and the two synthetic tests carry both refusal arms
  (the oversized sound signature, the truncated net key) into CI unignored;
* the prefix resolves to exactly eleven names in the one `campaign` binary
  and to nothing elsewhere in the workspace; each of the eleven passes when
  run alone with `--exact --include-ignored`; no test is skipped, weakened or
  `#[ignore]`d beyond the repository's `requires CS_GAME_DIR` /
  `requires CS_ENGINE_IMAGE` convention, and no lint was relaxed;
* the sheet's three regression priorities are each pinned against measured
  record data, while the claims the sheet says must not be assumed — wrong
  actor, wrong session, repeated event — are honestly left unmeasured for
  M24-C; every recorded unknown names its resolving task (`M16-B-FU1` #1252,
  `M24-B-FU1` #1271, `M24-A-FU1` #1272, `M24-C` #329); `claim` stays
  `implemented` and the campaign gate is asserted shut.

**Changes made during review:** two count corrections in this note and the
test module doc — the friendly capital spells **five** threshold sites (the
note said six, the module doc four) — and one stale wiring-doc entry: the
`evidence.rs` stage list never named M18-B after its merge, fixed the same
way the M19-B review named M17-B. No test or production code needed a fix.
The review also regenerated the evidence report on the reviewed tree with
`CS_EVIDENCE_REVIEWER` naming this session (the committed copy); the
implementer's committed copy and their `private/evidence/M24-B/acceptance.json`
were identical before regeneration.

Mutation checks the reviewer ran and reverted (both fail as required; the
selection was rerun green afterwards and `git status` was clean):

| Mutation | Result |
| --- | --- |
| `crates/cs_content/src/mission_control.rs`, `terminal_outcome_of`'s `INSTANTWIN` → `Failed` (production measurement) | `accept_m24_b_the_measured_vocabulary_partitions_and_refuses_no_m24_key` FAILED at `m24_b.rs:651` (the pinned terminal-outcome pair), exit 101 |
| `crates/cs_script/src/bindings/mod.rs`, `MAX_CALL_ARGS` `8` → `9` (production lowering) | two tests FAILED, exit 101: `accept_m24_b_m24s_record_does_not_lower_and_its_calls_refuse_by_two_named_gaps` (1 refusal instead of 7 — the sound key registers again) and the synthetic `accept_m24_b_one_oversized_sound_site_poisons_the_whole_sound_key` (no unbound key) |

Each mutation is in production code the suite does not own, so the failures
show the tests read the lowering and the measurement rather than their own
constants.

Checks run by the reviewer on the rebased tree, all green:

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_m24_b_ --include-ignored` | 0 (11 tests, log of record `private/evidence/M24-B/cargo-test.log`) |
| each of the eleven names alone with `--exact --include-ignored` | 0 (1 passed each) |
| `python3 tools/validate_evidence.py private/evidence/M24-B/acceptance.json --artifact-root private/evidence/M24-B --require-pass` | 0 (`structurally_valid: true`) |

The reviewer's regenerated report carries `candidate_tree` = the tree of
`1a60f115`, the commit that carries the suite plus these review fixes; the
only later delta is this findings section and the report's own copy under
`docs/findings/evidence/`, neither of which the acceptance suite reads.

## Sources

`$CS_GAME_DIR` read-only through `cs_assets::install::discover`,
`cs_formats::script_raw::discover_container`, `cs_content::stunts::decode_zrd`;
`$CS_ENGINE_IMAGE` read through `cs_content::coordinates::load_engine_image`;
`missions/M24.md`; `docs/contracts/SCRIPT-MISSION.md`,
`docs/contracts/CLI-EVIDENCE.md`;
`docs/findings/2026-10-02-m24-a-source-binding.md`,
`docs/findings/2026-10-10-m16-b-compatibility-gaps.md` (the stage this one
follows — same archive family, same sound-key gap),
`docs/findings/2026-10-10-m17-b-compatibility-gaps.md`,
`docs/findings/2026-10-06-m01-lc-directive-b-objective-lifecycle-target-semantics.md`
(target-flag semantics, `COMPLETED_ZEPCANNONS`, `INACTIVE*`),
`docs/findings/2026-10-06-m01-lc-directive-a-…` (the directive table's
measured extent).
