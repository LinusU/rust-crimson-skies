# M13-B: The Nefarious Trap's mission-specific compatibility surface

Date: 2026-10-10. Task: M13-B "Implement and regress mission-specific
compatibility gaps" (#295, `missions/M13.md`, work order `M13-B`). Shared
contract: `docs/contracts/SCRIPT-MISSION.md`. Capabilities used: `retail`
(`$CS_GAME_DIR` read-only, never written), `synthetic` (newly authored `.zrd`
records). Implementer: **bunny-2/bunny-2** (Rally #295, session of
2026-10-10). The review round of 2026-10-10 is recorded at the end of this
note: it was run by the same agent name and model in a separate, fresh-context
session, so it is not independent review, an agent review never replaces the
owner's human approval, and nothing here is `verified_original`
(AGENTS.md rule 8).

The stage's minimum acceptance scenario is *"All discovered mission-specific
behavior uses production engine systems and regression tests."* It holds for
everything this note measures: M13's discovered mission-specific behavior is its
mission control program — the typed keyed list the installation ships as
`objectives.zrd` inside `ZBD/C3/M03/zrdr.zbd` — and this stage runs it through
production systems (`SourceContext::control_program`, `SourceContext::bind`,
`cs_formats::script_raw::discover_container`, `cs_content::stunts::decode_zrd`,
`cs_content::mission_control`, `cs_app::mission_control`,
`cs_app::control_lowering`, `cs_sim::objectives::address`'s convention as
measured by M02-B-FU3/M06-B-FU3) and regresses it with thirteen
`accept_m13_b_*` tests. One compatibility gap is left **measured and recorded,
not worked around**: see *The one measured gap* below.

## What changed

No production code and no change to `missions/bindings/M13.json`: M13's record
lowers completely, so there was no lowering gap to close and no binding
statement to correct. Added:

* `crates/cs_app/tests/campaign/m13_b.rs` — thirteen `accept_m13_b_*` tests
  (ten retail, three synthetic);
* `crates/cs_app/tests/campaign/evidence/m13_b.rs` — the M13-B evidence
  harness, plus its one `mod` line in `evidence.rs`;
* `docs/findings/2026-10-10-m13-b-compatibility-gaps.md` (this note) and
  `docs/findings/evidence/M13-B.json` (the committed report copy).

Wiring only: `crates/cs_app/tests/campaign/main.rs` (`mod m13_b;`).

## What was read from the installation

| Fact | Value |
| --- | --- |
| `install_sha256` | `b4e780ab…c631978` (same installation as M01-A … M13-A) |
| Reader archive | `ZBD/C3/M03/zrdr.zbd`, 118 635 bytes, SHA-256 `37a227cc…d6efe` (M13-A's own program span) |
| Members | 16; exactly one declares numbered blocks — `objectives.zrd`, the 8th member, offset 12 552, 11 773 bytes, SHA-256 `4a691f0c…ee400` |
| Control record | 38 numbered blocks, 178 directive sites, 36 distinct keys, no block refusal, no record key outside the measured vocabulary |
| Dispositions | 34 measured, 2 terminal (`INSTANTWIN` block 16, `INSTANTLOSS` block 7), **0 unmeasured, 0 refused** |
| Lowering | mission `mission/ch3-m03`, 38 `RawObjective`s, all 178 calls bound, all 38 conditions lowered, `MissionProgram::validate` clean, 0 unmet requirements, census row **complete** |
| Campaign | `zbd/c3/m03` is one of the census's complete rows; `campaign_ready()` is still false (other rows carry their own gaps) |
| Record-level fields | `MISSION_TIMER [0]`; `PLAYER_INIT [1, [-13129, 400, -9180], [0, 330, 0], 0.8, 180]` — the same position and heading the archive's own `aiv.zrd` spells for `player`; the three animation lists are empty |
| Whole installation | 184 `.zbd` containers decoded member by member for the two names the one `SET_AI_NET` site spells |

## The three regression priorities, as the record spells them

**Ordered traversal.** Blocks 3–6 are a four-stage damage ladder over the
*same* twelve `piratezep` engine chains (`reng11`…`reng32`, `leng11`…`leng32`,
each `piratezep/<engine>/healthy`), under rising `INACTIVE_COMPLETION_COUNT`
thresholds **3, 5, 7, 10**. Block 3 is the only block of the record with no
dormant marker, so the ladder starts with the mission; blocks 4, 5 and 6 spell
`BEGIN_DORMANT -1` and are entered only by the previous stage's nap, on the
delay the record spells (15, 15, 10 s), and block 6's nap names block 7 — the
`INSTANTLOSS` latch — after 25 s. Only blocks 1 and 2 arm a clock (2 s and
13.5 s). The two dependency gates (`TICK_DEPENDS_ON_OBJ 19` at block 15, `29` at
block 21) lower into the program as an extra `ObjectiveAwake` conjunct carrying
the dependency's own zero-based index, so order is a gate on evaluation rather
than a timing guess. What the engine *does* at each boundary — wrong actor,
wrong session, repeated event — is a runtime observation and stays unmeasured
here (M13-C).

**Allegiance transition.** M13's control member spells **no** directive that
changes an owner, a faction, a team or an attitude: the assertion walks the
exact 36-key vocabulary rather than looking for one spelling, and
`SET_AI_TEAM` (which M10 spells) is absent. What the record does spell on that
theme is `SET_AI_NET` — one site, block 25, `["britkestrel_1", "M3BritAce"]`,
`DirectiveOperation::AssignNet` — and `WAKEUP_ENEMIES` at block 26
(`britpeace_1..4`), plus three `DEDG` group-depletion sites (groups 2, 3, 1 at
blocks 13, 14, 21). The stage therefore did the sheet's first step instead:
*identify the real actor bindings*, by walking every member of the archive and
asking where each directive name is declared (see the table in
`accept_m13_b_every_actor_the_record_names_resolves_in_the_shipped_data_but_one`).
Fifteen of the seventeen checked names resolve in a member of the archive —
`piratezep` in `aiv`/`targets`/`zeppelins`/`destroy_cargozep`, `barracuda` in
`egen`/`sub_movement`/`submarine`/`targets`, the turret names in `targets`, the
engine names in `zeppelins`, `M3PirateZep` in `zeppelins`, `runway_door` in
`submarine`, `movebridge1` in `destroy_cargozep` — and `player` in four. Two do
not, and they are the two halves of the one `SET_AI_NET` site.

**Optional versus mandatory outcome.** Exactly two terminal outcome keys exist,
one bare site each, both dormant with no timed self-wake, each named by exactly
one completion edge: block 6 naps block 7 (`INSTANTLOSS`) and block 15 naps
block 16 (`INSTANTWIN`), both with a 25-second delay. Over wake/nap edges plus
the two gates, the success latch's prerequisite closure is 13 blocks
(`2, 8, 9, 12, 13, 14, 15, 16, 19, 20, 21, 26, 29`) and the failure latch's is
the five-block ladder (`3, 4, 5, 6, 7`); the two sets are disjoint and the
remaining 20 blocks are named by neither. Kill edges are deliberately not
prerequisites — killing block 9 does not enter it — and the suite pins that by
asserting block 9's only predecessor is block 8's nap. Condition shapes split
the record the same way: 23 blocks (both latches included) lower to a bare
`ObjectiveAwake` and complete on wake alone, 15 spell an evaluator or a gate.
Whether any of the 20 outside blocks is an *optional reward or stunt branch* is
**not** measured — M13-A binds no stunt and reward ids — so nothing here calls
one optional in that sense.

Addressing is not assumed: all 41 spelled integers (20 wake, 4 kill, 15 nap, 2
gates) lie in `1..=38`, none is `0`, and block 9 spells `38`, the block count
itself — the value a zero-based reading would report out of range. The
convention is M02-B-FU3 (#802) and M06-B-FU3 (#819)'s measurement, re-derived
here on M13's own record, not re-decided.

## The one measured gap

**`britkestrel_1`, the actor M13's single `SET_AI_NET` site points a node at, is
declared by no archive in the installation.** Measured by decoding every `.zbd`
container (184 of them) member by member through production discovery and the
production `.zrd` decoder, and reading every text node: the name occurs exactly
once, in `ZBD/C3/M03/zrdr.zbd`, member `objectives.zrd` — the directive site
that spells it. A raw byte scan of the whole installation agrees. Its node
operand `M3BritAce` resolves: it occurs in that same site and in
`ZBD/C3/zrdr.zbd`, member `neindex.zrd` — the chapter world's node index — so
the second half of the site has a declaration and the first does not.

Nothing here says what the original does with a name no shipped file declares
(runtime spawn, a naming path this build has not measured, or a site whose
actor is simply never resolved); that is recorded as unknown, not guessed, and
filed as a follow-up below. Note the contrast with M06-B-FU2's *absence* result:
there, the mission's data simply did not mention the mechanic; here the data
mentions the actor and no file declares it.

**Coverage behind that claim (review round).** The same whole-installation
walk was extended from the seventeen-name table to *every* text the record
spells: 63 distinct texts across the 178 sites. All but three are declared
outside M13's control member. The three are `britkestrel_1` and — as message
operands, not actors — `MSG_BRF_HAM3_OBJ1` and `MSG_BRF_HAM3_OBJ2`, the `MSG_…`
operands of the `IDENTITY` sites at blocks 15 and 16, which no other `.zrd`
member of the installation spells (`IDENTITY`'s other operand, `PRIMARY`, is
declared by 24 records elsewhere, so the key itself is not what makes them
record-only). This stage draws no conclusion about resources that are not
`.zrd` data: the walk decodes `.zrd` members only, and
`2026-10-06-m01-lc-directive-meaning.md` measures the `IDENTITY` `MSG_…`
operand as never read by the measured parse. The count and the exception set
are pinned by `accept_m13_b_every_actor_the_record_names_resolves_in_the_shipped_data_but_one`,
so a name that stops resolving anywhere fails the suite.

## What is not claimed

`verified` stays `false` and every checklist entry M13-A left unknown is still
listed in `missions/bindings/M13.json` — the control program being measured does
not answer difficulty branches, failure/success precedence, media, rewards or
`closure_sha256`. No mission was played, no original executable was run, no
presentation, audio or visual row was reviewed: those are M13-C, which requires
`human_play` and the owner. The wrong-actor / wrong-session / repeated-event
halves of the sheet's priorities are runtime observations. The campaign stays
not ready on this stage's evidence.

## Test inventory (`accept_m13_b_*`, 13 tests: 10 retail, 3 synthetic)

Retail, all `#[ignore = "requires CS_GAME_DIR"]`:

1. `…_the_control_program_is_the_member_that_declares_the_blocks` — census,
   independent walk, production control binding and M13-A's mission binding all
   name one archive, one member, one span and two digests re-derived from bytes.
2. `…_every_directive_m13_spells_has_a_disposition_and_none_is_refused` — the
   exact 36-key vocabulary, 34 measured + 2 terminal, sites summing to 178.
3. `…_the_sheet_priorities_resolve_to_measured_operations_and_none_changes_allegiance`
   — operations for the three priorities plus the absence of any
   allegiance-changing key.
4. `…_the_damage_ladder_and_the_dependency_gates_are_record_data` — thresholds,
   engine chains, the four nap delays, dormancy, both gates lowered, ladder
   conditions and the failure latch's wake-only condition.
5. `…_every_actor_the_record_names_resolves_in_the_shipped_data_but_one` — the
   actor/node declaration table, and the record really spells every name in it;
   then, because a table is a sample, a one-pass index of every `.zrd` text of
   the installation that asserts all **63** distinct texts the record spells
   are declared outside the control member except three measured ones
   (`MSG_BRF_HAM3_OBJ1`, `MSG_BRF_HAM3_OBJ2`, `britkestrel_1`).
6. `…_the_ai_net_actor_is_declared_by_no_archive_in_the_installation` — the
   whole-installation decode scan, both names.
7. `…_the_terminal_blocks_are_gated_and_every_address_is_in_range` — latches,
   41 addresses, the discriminating `38`, the two latch edges.
8. `…_the_two_outcomes_have_disjoint_prerequisites_and_the_rest_is_not_mandatory`
   — prerequisite closures, kill-is-not-a-prerequisite, the 23/15 condition
   split, M13's `TRAVELERS` condition.
9. `…_every_call_binds_every_condition_lowers_and_m13s_record_completes` —
   bindings, validation, unmet rows, and the two retail blocks that spell no
   completion count.
10. `…_m13_is_complete_and_the_campaign_stays_unready`.

Synthetic, run in CI without original data:

1. `…_m13s_travelers_site_lowers_and_a_counting_mode_refuses` — M13's own
   predicate at both subjects: named lowers and validates, numeric refuses with
   the counting mode.
2. `…_an_ungrounded_allegiance_directive_is_refused_rather_than_honoured` — an
   authored `SET_FACTION` (a key M13 never spells) stays unmeasured, refuses its
   site by name as an unknown host call, stands no program and leaves the record
   incomplete.
3. `…_a_nap_delay_is_data_and_an_inactive_threshold_defaults_to_its_list` — a
   nap delay past the block count is data; the threshold defaults to the block's
   own member count, a spelled count wins, and a count of 99 against three
   members is carried as spelled rather than clamped.

## Checks

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_m13_b_ --include-ignored` | 0 (13 tests) |
| `python3 tools/validate_evidence.py private/evidence/M13-B/acceptance.json --artifact-root private/evidence/M13-B --require-pass` | 0 |

## Recorded unknowns (not guessed)

- **`britkestrel_1`** — declared nowhere in the installation (measured above);
  what the original resolves it to is unknown. Resolving task:
  **M13-B-FU1** (static reading in the owner's decrypted image and/or an
  original reference run under M13-C).
- **The runtime halves of all three priorities** — what happens at the
  threshold boundary, whether the wrong actor or a repeated event can satisfy a
  block, and which of the 20 outside blocks is an optional branch — are runtime
  observations for **M13-C** (`human_play`, owner-gated).
- **M13's startup animations** — `startanims.zrd` declares eight
  `NEW_GAME_START` names (`place_tntboxes`, `pzep_todrydock`, `cgzepstate`,
  `set_cggasbagstate`, `cg1zep_engines_stop`, `pzep_engines_start`,
  `calldestroy_the_cargozep`, `flag_state_pirate`) and `player_setup` on
  `LOAD_GAME_START`, read through `cs_app::animation::programs`; the archive's
  own `mis_anim.zrd` declares zero definitions, while the same eight names also
  occur in the separate container `ZBD/C3/M03/mis_anim.zbd`. Whether production
  binds M13's startup animations against that container is **not asserted by
  this stage** (the M01-LC machinery that would do it was measured on M01's
  data). Resolving task: **M13-B-FU2**.
- The join remains M13-A's inference (`ClaimStatus::Inferred`); no campaign
  definition record or original run was observed.

## Review (bunny-2/bunny-2, 2026-10-10)

Reviewer: **bunny-2/bunny-2** (Rally #295 review claim of 2026-10-10, OpenCode
Space Bunny Free, `opencode-go/mimo-v2.6-Flash`), a separate session that began
from the task history, the submitted summary, `missions/M13.md`, the contract
and the branch diff — not from the implementation. Implementer: **bunny-2/bunny-2**
(same Rally agent name and model, implement claim submitted at `fe2f62ad`), so
this review is **not independent evidence** in the AGENTS.md sense: it is a
fresh-context re-derivation plus a full re-run of every check. A Rally merge
awards `checked` only, no agent review replaces the owner's human approval, and
nothing here is `verified_original` or `release_approved`.

What the reviewer verified on the branch:

* only owner paths changed — `crates/cs_app/tests/campaign/` (test, evidence
  harness, two `mod` lines) and `docs/findings/`; no protected path, no
  `missions/bindings/M13.json`, no `campaign_bindings.rs`, no production crate,
  no binary file;
* every retail test drives production code (`survey_mission_control_programs`,
  `read_control_member`, `SourceContext::control_program`, `SourceContext::bind`,
  `discover` + `discover_container` + `decode_zrd`, `measure_control_record`,
  `lower_control_record`, `MissionProgram::validate`) and the three synthetic
  tests carry the refusal arms (counting-mode `TRAVELERS`, unknown
  `SET_FACTION`, nap-delay-is-not-an-address) into CI unignored;
* no test was skipped, weakened or `#[ignore]`d beyond the repository's
  `requires CS_GAME_DIR` convention, and no lint was relaxed;
* the sheet's three priorities are each pinned against measured record data,
  and the two claims the sheet says must not be assumed — that the wrong actor
  cannot satisfy a block and that a repeated event cannot — are honestly left
  unmeasured for M13-C rather than asserted.

**Change made during review.** The actor-resolution test asserted a
seventeen-name table while its name said *every* actor the record names. The
record spells **63** distinct texts, so the test now builds one installation
index (one pass over the same `.zrd` walk `declarations_of` does per name) and
asserts that all 63 are declared outside M13's control member except the three
measured record-only ones. No production code changed; the test name, the
evidence harness list and the suite's prefix are unchanged.

Mutation checks the reviewer ran and reverted (both fail as required):

| Mutation | Result |
| --- | --- |
| `m13_b.rs` ladder thresholds `[3, 5, 7, 10]` → `[3, 5, 7, 11]` | `accept_m13_b_the_damage_ladder_and_the_dependency_gates_are_record_data` FAILED, exit 101 |
| `m13_b.rs` record-only list drops `MSG_BRF_HAM3_OBJ1` | `accept_m13_b_every_actor_the_record_names_resolves_in_the_shipped_data_but_one` FAILED at the new full-coverage assertion, exit 101 |

Checks run by the reviewer after the change, all green: `cargo fmt --all --
--check`, `cargo clippy --workspace --all-targets --all-features --locked --
-D warnings`, `cargo test --workspace --locked`,
`cargo test --workspace --locked -- accept_m13_b_ --include-ignored` (13
discovered, 13 executed, 13 passed, 0 failed, exit 0), and
`tools/validate_evidence.py … --require-pass` on the reviewer's regenerated
report.

**Rebase rounds.** `main` advanced twice while this review ran — first to
`852d1dd0` (M12-B), then to `390796d7` (the M01 objective-source amendment).
Both rebases applied without conflicts, but both incoming sets touch
`crates/cs_app/tests/campaign/main.rs`, which this branch changes too, so the
owner's 2026-10-01 lighter check set did not apply: all four checks were
re-run in full after each rebase, and the selection still runs exactly the
thirteen `accept_m13_b_` tests while filtering the new stages' tests in the
same binary. The run recorded above and the evidence report belong to the
final rebased tree — the report's `candidate_tree` is the tree of the last
content commit, since the only later commit is the report copy itself.

## Sources

`$CS_GAME_DIR` read-only through `cs_assets::install::discover`,
`cs_formats::script_raw::discover_container`, `cs_content::stunts::decode_zrd`;
`missions/M13.md`; `docs/contracts/SCRIPT-MISSION.md`,
`docs/contracts/CLI-EVIDENCE.md`;
`docs/findings/2026-10-02-m13-a-source-binding.md`,
`docs/findings/2026-10-09-m05-b-control-program.md`,
`docs/findings/2026-10-09-m06-b-compatibility-gaps.md`,
`docs/findings/2026-10-09-m08-b-compatibility-gaps.md`,
`docs/findings/2026-10-09-m10-b-control-program-gaps.md`,
`docs/findings/2026-10-10-m06-b-fu3-objective-address-convention.md`.
