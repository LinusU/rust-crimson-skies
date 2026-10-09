# M08-B: The Petrol Plot's mission-specific compatibility surface

Date: 2026-10-09. Task: M08-B "Implement and regress mission-specific
compatibility gaps" (#280, `missions/M08.md`, work order `M08-B`).
Shared contract: `docs/contracts/SCRIPT-MISSION.md`. Capabilities used:
`retail` (`$CS_GAME_DIR` read-only, never written) and `synthetic` (newly
authored `.zrd` values). Implementer: **bunny-alpha-2** (Rally #280, session
of 2026-10-09T02:22Z). Reviewer: whoever reviews the branch — the evidence
report's `review.identity` is read at run time from `CS_EVIDENCE_REVIEWER`,
so each agent writes its own; no agent review replaces the owner's human
approval.

The stage's minimum acceptance scenario is *"All discovered mission-specific
behavior uses production engine systems and regression tests."* It holds for
everything this note measures: M08's discovered mission-specific behavior is
its mission control program — the typed keyed list the installation ships as
`objectives.zrd` inside `ZBD/C2/M03/zrdr.zbd` — and this stage binds it
through production systems (`SourceContext::control_program`,
`cs_formats::script_raw::discover_container`, `cs_content::stunts::decode_zrd`,
`cs_content::mission_control`, `cs_app::mission_control`,
`cs_app::control_lowering`) and regresses it with seven `accept_m08_b_*`
tests. **Two** compatibility gaps are measured and pinned, not worked
around; both are already filed by their discoverers, and neither is
duplicated here (see below). M08's control record does **not** lower, so the
mission stays Unsupported and the campaign gate stays closed.

Nothing here is `verified_original` (AGENTS.md rule 8): the
work-order ↔ mission join remains M08-A's inference, directive *effects* are
the M01-LC findings' static readings of the original code, and no original
executable has been run.

## What was read from the installation

`$CS_GAME_DIR` opened read-only. Measured on this installation, all of it
re-derived by the acceptance tests on each run:

| Fact | Value | Where it comes from |
| --- | --- | --- |
| `install_sha256` | `b4e780ab…c631978` | production discovery, re-measured by the test |
| Reader archive | `ZBD/C2/M03/zrdr.zbd`, 41 769 bytes, SHA-256 `a72c66b1…3295886` | the campaign layout entry at M08-A's campaign position 7 — the same bytes and digest `missions/bindings/M08.json` cites |
| Members offered | 13, every one decoding as a `.zrd` document | `discover_container`, the same enumeration the F13-B script census uses |
| Control member | `objectives.zrd`, offset 17 212, length 13 980, own SHA-256 `b0db161f…30531e` | `cs_content::mission_control::control_member` — the **rule** (the only member whose decoded record declares numbered `OBJECTIVE<N>` blocks), never a filename |
| Record | 57 numbered blocks (`OBJECTIVE1`…`OBJECTIVE57`), 209 directive sites, 22 distinct keys | `measure_control_record` over the decoded member |
| Vocabulary partition | 2 implemented keys (`INSTANTWIN` ×1 site, `INSTANTLOSS` ×1 site, both bare), 20 measured keys, **0 unmeasured**, 0 block refusals | `MeasuredControlRecord::{implemented, measured, unmeasured}` |
| Record-level fields | `MISSION_TIMER`, `PLAYER_INIT` and the three empty animation lists — each once | `record_fields()` |
| Unclassified record keys | *none* — unlike M02's five sound keys, every record-level key M08 spells is in `CONTROL_RECORD_KEY_VOCABULARY` | `unclassified_record_keys()` |
| Lowering attempt | mission `mission/ch2-m03`; 57 objectives; 209 call verdicts — 190 bound, **19 refused**; 57 condition verdicts — 49 lowered, **8 refused**; `validation` `None`; no program | `cs_app::control_lowering::lower_control_record` through the census row |
| Census context | 40 measured rows of the mission-scoped readers; M08 among the rows that are **not** complete; `campaign_ready()` false | `survey_mission_control_programs` |

The 13 members, in archive order, each with the block count that qualified or
excluded it under the rule:

| member | offset | length | blocks |
| --- | --- | --- | --- |
| `aiv.zrd` | 0 | 14 280 | 0 |
| `dzones.zrd` | 14 280 | 458 | 0 |
| `egen.zrd` | 14 738 | 16 | 0 |
| `location.zrd` | 14 754 | 468 | 0 |
| `map.zrd` | 15 222 | 651 | 0 |
| `mis_anim.zrd` | 15 873 | 1 003 | 0 |
| `net.zrd` | 16 876 | 336 | 0 |
| `objectives.zrd` | 17 212 | 13 980 | **57** |
| `startanims.zrd` | 31 192 | 167 | 0 |
| `targets.zrd` | 31 359 | 2 238 | 0 |
| `weather.zrd` | 33 597 | 3 218 | 0 |
| `zeppelins.zrd` | 36 815 | 2 559 | 0 |
| `zepstate.zrd` | 39 374 | 463 | 0 |

Size and position are measurably **not** the rule, as at M02: `aiv.zrd` is
longer than the control member (14 280 > 13 980 bytes) and the control member
is the eighth of thirteen. The identities test asserts both facts, so a future
"longest member is the program" or "first member is the program" regression
fails a test.

## What M08-B adds, in owner paths

- `crates/cs_app/tests/campaign/m08_b.rs` (owner path): the seven
  `accept_m08_b_*` tests (five retail, two synthetic).
- `crates/cs_app/tests/campaign/evidence.rs` (owner path): the
  `evidence_report_m08_b_writes_the_acceptance_report` harness plus this
  task's test lists, and a second production observation written beside the
  report (`m08-control-program.json`: identities, spans, digests, member
  accounting, directive counts, the lowering's verdicts and both gaps). It is
  deliberately not prefixed `accept_m08_b_`, so a task selection never picks
  it up as an acceptance test.
- Wiring only (AGENTS rule 1): `crates/cs_app/tests/campaign/main.rs` (one
  `mod m08_b;` and a doc paragraph).
- **No production code.** The machinery this stage runs is M01-LC's (the
  census, the directive dispositions, the record → `RawProgram` adapter) and
  M02-B's (`SourceContext::control_program`); M08 adds no new operation, no
  new rule and no new binding. A gap that needs production code is filed, not
  patched here.
- No `Cargo.toml` or `Cargo.lock` change: this stage adds no dependency and
  no crate edge.
- `missions/bindings/` is unchanged: this stage adds no new file there.
  `M08.json` keeps its empty `objective_ids` and its "objective graph: not
  bound" unknown — the blocks this stage measures are not objective *content
  ids*, and the product-incompleteness state stays where `AUDIT-PLAN-SYNC`
  puts it (the binding record and `docs/findings/`), never narrowed to pass a
  validator.

## Where the three M08 sheet priorities live, and what stays unmeasured

M08-A left all three unbound ("they remain for M08-B once the mission program
is decoded"). The sheet's priorities are designs; their predicates must come
from the source program and reference runs. This stage locates each one's
**data** in the measured record and assigns no timing, count or coordinate
the record does not spell:

- **Logistics state.** The record's only actor-ownership writes are one
  `SET_AI_TEAM` site and one `SET_AI_NET` site, both in `OBJECTIVE3`: six
  named pairs each, `hafury_1`…`hafury_6` to teams `2,1,2,2,1,2` and to the
  one node-list entry `M3Aces`. The world state the mission reads back is the
  `DEDG` family (17 group-depletion sites) and the two `WAKEUP_ENEMIES`
  lists. *Whether* the wrong actor, the wrong session or a repeated event can
  satisfy any of these writes is runtime behaviour and stays **unmeasured**;
  nothing here simulates it.
- **Alternative objective paths.** The danger-zone chain the record spells is
  `dzpath1, dzpath2, dzpath3, dzpath5, dzpath4, dzpath6, dzpath7, dzpath9`
  across `OBJECTIVE17`…`OBJECTIVE24` — eight `dzpath` names with 4 and 5
  interleaved, and no `dzpath8` anywhere in the record (whether such a zone
  exists in `dzones.zrd` is not checked here), with each block retiring the target its
  predecessor added (`sghangar → dz2 → dz3 → dz5 → dz4 → dz6 → dz7 → dz9`).
  That ordering is record data, and the reordering question it raises is
  answered by the record rather than by a walkthrough. The timed alternative
  is the pair `OBJECTIVE42` / `OBJECTIVE43`: `OBJECTIVE43` wakes itself at
  190 mission-clock seconds and retires the eight chain targets and blocks
  `OBJECTIVE17`…`OBJECTIVE24` plus `OBJECTIVE42`; the chain's last block
  wakes `OBJECTIVE42`, which retires `OBJECTIVE43`. Exactly one of the pair
  survives each outcome, so an alternative-order test has a real boundary to
  cross (M08-ORDER, whose *runtime* observation is M08-C's).
- **Protected transfer.** `OBJECTIVE7` carries the one `COMPLETED_STOPPOINT`
  site (`{M3Piratezep, 1, 0}`), the `pzhookpoint` objective target and the
  `{piratezep, rock_zeppelin}` other-target record; `OBJECTIVE48` carries the
  record's single `TRAVELERS` site
  (`player`, `APPROACHING`, `piratezep`, `1500.0`, `1`) — the subject, the
  polarity token (the only spelling the findings measured) and the numbers
  the record spells, with no radius or count semantics claimed here. The
  **authorisation** half of the contract's interaction family (authorize a
  docking/pickup/boarding, begin, complete, abort a transfer) is *absent from
  this member*: the vocabulary test pins M08's whole 22-key list, and not one
  key names a docking, pickup, boarding, transfer or authorisation directive.
  Where that half lives — `targets.zrd`, `zeppelins.zrd`, the members this
  stage does not decode, or another program — is **unbound**; the interaction
  family's owner is F36-D (and F35-D for capital ships).

The two terminal latches are gated in the record and pinned by the graph
test: one bare `INSTANTWIN` in `OBJECTIVE8`, woken by exactly one other
block (`OBJECTIVE7`); one bare `INSTANTLOSS` in `OBJECTIVE49`, which no block
wakes and which the six group-depletion announcer blocks `OBJECTIVE28`…`OBJECTIVE33`
nap (the nap re-wakes its target after the spelled seconds). Nothing here
claims what those edges *do* at runtime: the wrong-actor, wrong-session and
repeated-event halves of all three priorities need ordinary play (M08-C).

## The two measured compatibility gaps (already filed, not duplicated)

M08's directive vocabulary is **fully measured**: 22 of 22 keys carry a
M01-LC disposition, none is Unmeasured, and no block is unreadable. Yet the
record does not lower, and `objective_condition` and `call_arguments` are
both unmet:

1. **`KILL_OBJECTIVE_WHEN_I_COMPLETE`, 19 refused sites.** The key's sites
   spell six shapes (1, 5, 6, 7, 9 and 10 integers); the lowering adapter
   registers one `BindingSpec` signature per measured shape and
   `cs_script::bindings::HostBindingRegistry::register` refuses any
   signature longer than `MAX_CALL_ARGS` (8), so the whole key fails
   registration and all 19 sites refuse as `unknown host call`. The
   over-bound shapes are the ten-argument lists of `OBJECTIVE28`…
   `OBJECTIVE33` (six sites) and the nine-argument list of `OBJECTIVE43`
   (one site) — seven sites in total. This is the shared gap already filed as
   **#800 (`M02-B-FU1`)**; this stage pins M08's 19 sites so the fix cannot
   land without updating this test. Not duplicated.
2. **`DANGER_ZONES_COMPLETED`, 8 refused completion conditions.**
   `OBJECTIVE17`…`OBJECTIVE24` — the whole danger-zone chain — are the eight
   blocks whose conditions this build refuses:
   *"the danger-zones flag evaluator is measured but this build lowers no
   condition for it, and offering one would be a guess at its predicate"*
   (`crates/cs_script/src/conditions.rs`). The directive itself is measured
   (`DirectiveOperation::DangerZoneFlags`), so this is a lowering gap, not an
   unknown directive; the other 49 conditions lower. It is already filed as
   **#813 (`M07-B-FU1`)** by the M07-B stage. Not duplicated.

In both cases the census reports M08's row incomplete and `campaign_ready()`
stays false — the contract's honest reading. Nothing in this stage loosens
it, lowers an unmeasured directive to a no-op or clamps an address.

The synthetic tests carry both arms into CI, where there is no original data:
an authored kill site at the bound of 8 arguments binds while one over it
refuses the key's registration and its record, and an authored
`DANGER_ZONES_COMPLETED` block refuses its condition while the same record
with `DEDG` in its place lowers and completes. The follow-ups that change
either mechanism (#800, #813) must update these pins in their own change —
never delete them to get green.

## The cross-objective address rule, and this stage's arithmetic

M08 spells 151 cross-objective addresses (43 wake, 96 kill, 12 nap; 55
sites). Every one lies in `1..=57`: none is zero, none is negative and none
is past the record's last block — the largest value M08 spells *is* the block
count, so the boundary is live without being crossed. Under the rule
M02-B-FU3 measured from the original (a spelled address names the block it
decrements to; the parse stores `address - 1`, and only an address past the
count is refused — Rally #802), every address resolves to a block this record
declares, so **M08 carries no out-of-range address**.

This stage does **not** re-measure that rule: it is #802's measurement, cited
here because the arithmetic above is stated in its terms. When #802's
`cs_sim::objectives::address::resolve_objective_address` reaches `main`, the
`1..=57` arithmetic in `accept_m08_b_the_block_graph_is_closed_under_the_records_own_numbering`
can be replaced by a call to that resolver; the assertions stay.

## Files

- `crates/cs_app/tests/campaign/m08_b.rs` — the seven acceptance tests.
- `crates/cs_app/tests/campaign/evidence.rs` — the evidence harness and the
  task's test lists.
- `crates/cs_app/tests/campaign/main.rs` — wiring only.
- `docs/findings/evidence/M08-B.json` — the committed copy of the acceptance
  report (the artifacts it hashes stay in `private/evidence/M08-B/`).

## Test inventory (`accept_m08_b_*`, 7 tests: 5 retail, 2 synthetic)

| Test | What it pins |
| --- | --- |
| `accept_m08_b_m08s_control_program_is_bound_to_the_same_identities_as_its_mission_binding` (retail) | the control binding and M08-A's mission binding name one mission (`mission/ch2-m03`, campaign position 7) and one program (`script/c2-m03-zrdr`); the archive's length and digest re-derive from disk; exactly one of the 13 members declares numbered blocks and it is the member the rule named (`objectives.zrd`); the member's span lies inside the archive and its digest re-derives from the member's own bytes; a longer member exists and the control member is not the first, so size and position are not the rule; the census — a third derivation through its own discovery path — names the same container, the same digest, the same member and the same record; a document re-read through discovery carries the same 57 blocks |
| `accept_m08_b_the_measured_vocabulary_partitions_and_refuses_no_m08_key` (retail) | 57 blocks / 209 sites / 22 keys; sites sum to the record total; the partition is exactly 2 implemented + 20 measured + 0 unmeasured; every measured key names operation, effect and evidence; the outcome keys are the only bare spellings; the five measured record fields each occur once; no unclassified record key; and the whole sorted 22-key vocabulary, so an added or dropped key fails here |
| `accept_m08_b_the_block_graph_is_closed_under_the_records_own_numbering` (retail) | the independent walk sees 57 blocks numbered 1…57 with no gaps; the three cross-objective keys are the only ones spelled; the walk's 151 addresses equal the measurement's; every address lies in `1..=57` (min 2, max 57), so every edge resolves to a declared block; one success latch (`OBJECTIVE8`) woken by exactly one block (`OBJECTIVE7`); one failure latch (`OBJECTIVE49`) with no wake edge and six nap sources (`OBJECTIVE28`…`OBJECTIVE33`); the census and the binding agree on one measurement |
| `accept_m08_b_both_lowering_gaps_are_named_and_the_campaign_gate_stays_closed` (retail) | M08 does not lower; exactly `objective_condition` and `call_arguments` are unmet; one key refuses registration and the refusal names `KILL_OBJECTIVE_WHEN_I_COMPLETE` and "too many arguments"; exactly 19 sites refuse, all naming the key, and the other 190 bind; the kill key's longest shape exceeds `MAX_CALL_ARGS` and exactly 7 sites spell an over-bound one; exactly 8 condition verdicts refuse, every one naming `DANGER_ZONES_COMPLETED` and `OBJECTIVE17`…`OBJECTIVE24`, with the other 49 lowered; no program stood to validate; M08 is not a complete census row and the campaign gate stays closed |
| `accept_m08_b_the_three_sheet_priorities_locate_in_the_measured_record` (retail) | the six team pairs and six net pairs of `OBJECTIVE3` and the 17 `DEDG` sites (logistics state); the eight-zone chain and its eight-target handover, the 190-second timed alternative and the two-way kill between the pair (alternative objective paths); the stoppoint record, the hook-point target and the single `TRAVELERS` operands (protected transfer); and the negative half — no key of the pinned vocabulary names an interaction, docking, pickup, boarding, transfer or authorisation directive |
| `accept_m08_b_a_kill_site_over_the_host_call_bound_refuses_and_one_at_the_bound_binds` (synthetic) | the refusal mechanism at the bound: an 8-argument kill site binds and completes; a 9-argument one refuses the key's registration, refuses its record and leaves the key out of the registry |
| `accept_m08_b_the_danger_zones_condition_refuses_while_a_measured_condition_lowers` (synthetic) | the condition refusal: an authored `DANGER_ZONES_COMPLETED` block refuses under `objective_condition` (naming the evaluator and "lowers no condition for it") with all its calls bound, while the same record with `DEDG` in its place lowers and completes |

Every test calls production code (`SourceContext::control_program`,
`SourceContext::bind`, `discover_container`, `decode_zrd`,
`objective_blocks_of`, `measure_control_record`, `lower_control_record`,
`survey_mission_control_programs`). No test repeats an expected value read
from the record it checks: the retail assertions re-derive from `$CS_GAME_DIR`
through a second production walk, or compare two independent derivations
against each other. The retail tests are `#[ignore = "requires CS_GAME_DIR"]`
(CI skips them); the synthetic ones run in CI.

## Checks

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_m08_b_ --include-ignored` | 0 (7 tests) |
| `python3 tools/validate_evidence.py private/evidence/M08-B/acceptance.json --artifact-root private/evidence/M08-B --require-pass` | 0 (`structurally_valid: true`) |

## Recorded unknowns (not guessed)

- **The join remains an inference** (M08-A's standing unknown): this stage
  consumes it through `SourceContext::bind` and adds no second evidence for
  the mapping itself. The `Plot` / `Pit` spelling discrepancy of the two
  localized rows is likewise M08-A's unknown and is not resolved here.
- **No directive is implemented by a measured effect.** The 20 measured keys
  carry the M01-LC findings' static readings; a host operation for them is
  future work, and only the two outcome spellings are implemented (as
  `Lowering::Finish`).
- **M08's control record does not lower** — the two measured gaps above.
  M08 stays `Unsupported`; the campaign gate stays closed; the fixes are
  **#800** (host-call bound) and **#813** (danger-zones condition). Both were
  already filed when this stage measured them, so no duplicate task was
  created.
- **The runtime predicates of all three priorities are unobserved.** No
  original executable was run; the wrong-actor / wrong-session /
  repeated-event halves of logistics state, alternative objective paths and
  protected transfer need ordinary-play observation (M08-C, which is blocked
  on `human_play`) and stay open. No reference capture exists for M08
  (REF-OWNER-FIRST-CAPTURE is blocked on the owner).
- **The authorisation half of protected transfer is not in the control
  member** (pinned vocabulary), and which other reader members carry gameplay
  this stage does not decode — `aiv.zrd`, `dzones.zrd`, `targets.zrd`,
  `zeppelins.zrd`, `zepstate.zrd`, `net.zrd`, `location.zrd`, `map.zrd`,
  `weather.zrd`, `mis_anim.zrd`, `egen.zrd`, `startanims.zrd` — is unmeasured
  here: the control program is one member. The interaction family's owner is
  **F36-D** (F35-D for capital ships); nothing here assumes where it lives.
- **The cross-objective address rule is cited, not re-measured** — it is
  #802's measurement (see above), and this stage's arithmetic should become a
  call to `resolve_objective_address` once that task lands.
- **M08 has no runtime consumer yet** (`cs_sim::mission::MissionCountdown`
  applies only the timer operations today, per #807), so no test in this
  stage can observe a transition: everything here is binding and lowering
  evidence, not play evidence. The consumer is #807's
  (`M02-B-FU4`).
- **Difficulty, presentation and media rows (M08-DIFFICULTY,
  M08-PRESENTATION, M08-CONTINUITY)** are untouched: headless traces cannot
  pass them and they need the owner (M08-C).

## Sources

`$CS_GAME_DIR` read-only through `cs_assets::install::discover`,
`SourceContext::read`, `cs_formats::script_raw::discover_container`,
`cs_content::stunts::decode_zrd`; `missions/M08.md`;
`docs/contracts/SCRIPT-MISSION.md`; `docs/contracts/CLI-EVIDENCE.md`;
`docs/findings/2026-10-02-m08-a-source-binding.md`;
`docs/findings/2026-10-06-m01-lc-directive-{b,c,d}-*.md`;
`docs/findings/2026-10-08-m02-b-compatibility-gaps.md`;
`docs/findings/2026-10-08-m03-b-control-program-gaps.md`;
`crates/cs_app/src/mission_control.rs`;
`crates/cs_app/src/control_lowering.rs`;
`crates/cs_script/src/conditions.rs`;
Rally tasks #800 (`M02-B-FU1`), #802 (`M02-B-FU3`), #807 (`M02-B-FU4`),
#812 (`M10-B-FU1`), #813 (`M07-B-FU1`).
