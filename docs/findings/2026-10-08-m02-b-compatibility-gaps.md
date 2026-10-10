# M02-B: The Bomber Heist's mission-specific compatibility surface

Date: 2026-10-08. Task: M02-B "Implement and regress mission-specific
compatibility gaps" (#262, `missions/M02.md`, work order `M02-B`).
Shared contract: `docs/contracts/SCRIPT-MISSION.md`. Capabilities used:
`retail` (`$CS_GAME_DIR` read-only, never written) and `synthetic` (newly
authored `.zrd` bytes). Implementer: **bunny-2** (Rally #262, session of
2026-10-08T20:03Z). Reviewer: whoever reviews the branch — the evidence
report's `review.identity` is read at run time from `CS_EVIDENCE_REVIEWER`,
so each agent writes its own; no agent review replaces the owner's human
approval.

The stage's minimum acceptance scenario is *"All discovered mission-specific
behavior uses production engine systems and regression tests."* It holds for
everything this note measures: M02's discovered mission-specific behavior is
its mission control program — the typed keyed list the installation ships as
`objectives.zrd` inside `ZBD/C1/M02/zrdr.zbd` — and this stage binds it
through production systems (`SourceContext::control_program`,
`cs_formats::script_raw::discover_container`, `cs_content::stunts::decode_zrd`,
`cs_content::mission_control`, `cs_app::mission_control`,
`cs_app::control_lowering`) and regresses it with eight `accept_m02_b_*` tests.
One compatibility gap is **measured and pinned, not worked around**: eight of
M02's directive sites refuse to bind against the host-call registry's
per-signature argument bound, so M02's control record does not lower and the
mission stays Unsupported. That fix belongs to the lowering subsystem and is
filed as #800 (`M02-B-FU1`); this stage names it exactly. *Closed by #800 on
2026-10-09 — the gap section below carries the dated note.*

Nothing here is `verified_original` (AGENTS.md rule 8): the
work-order ↔ mission join remains M02-A's inference, directive *effects* are
the stage A–D findings' static readings of the original code, and no original
executable has been run.

## What was read from the installation

`$CS_GAME_DIR` opened read-only. Measured on this installation, all of it
re-derived by the acceptance tests on each run:

| Fact | Value | Where it comes from |
| --- | --- | --- |
| `install_sha256` | `b4e780ab…c631978` | production discovery, re-measured by the test |
| Reader archive | `ZBD/C1/M02/zrdr.zbd`, 117 252 bytes, SHA-256 `89f27219…5423ad9` | the campaign layout entry at M02-A's campaign position 1 — the same bytes and digest `missions/bindings/M02.json` cites |
| Members offered | 20, every one decoding as a `.zrd` document | `discover_container`, the same enumeration the F13-B script census uses |
| Control member | `objectives.zrd`, offset 14 413, length 12 410, own SHA-256 `c2fb540b…` | `cs_content::mission_control::control_member` — the **rule** (the only member whose decoded record declares numbered `OBJECTIVE<N>` blocks), never a filename |
| Record | 50 numbered blocks (`OBJECTIVE1`…`OBJECTIVE50`), 190 directive sites, 36 distinct keys | `measure_control_record` over the decoded member |
| Vocabulary partition | 2 implemented keys (`INSTANTWIN` ×1 site, `INSTANTLOSS` ×2 sites, both bare), 34 measured keys, **0 unmeasured**, 0 refusals | `MeasuredControlRecord::{implemented, measured, unmeasured}` |
| Record-level fields | `MISSION_TIMER`, `PLAYER_INIT` and the three empty animation lists — each once | `record_fields()` |
| Unclassified record keys | `MISSION_WON_SOUND`, `MISSION_LOST_SOUND`, `PRIMARY_COMPLETE_SOUND`, `SECONDARY_COMPLETE_SOUND`, `TERTIARY_COMPLETE_SOUND` | five record-level keys **outside** `CONTROL_RECORD_KEY_VOCABULARY`; counted and named, never interpreted (filed as #801, `M02-B-FU2`) |
| Lowering attempt | 182 of 190 sites bind; 8 refuse; all 50 completion conditions lower; no program assembles | `cs_app::control_lowering::lower_control_record` through the census row |
| Census context | 53 mission-scoped readers, 40 with control programs; M02 among the rows that are **not** complete; `campaign_ready()` false | `survey_mission_control_programs` |

The 20 members, in archive order, each with the block count that qualified or
excluded it under the rule:

| member | offset | length | blocks |
| --- | --- | --- | --- |
| `aiv.zrd` | 0 | 10 364 | 0 |
| `dzones.zrd` | 10 364 | 218 | 0 |
| `egen.zrd` | 10 582 | 730 | 0 |
| `location.zrd` | 11 312 | 378 | 0 |
| `map.zrd` | 11 690 | 651 | 0 |
| `mis_anim.zrd` | 12 341 | 1 736 | 0 |
| `net.zrd` | 14 077 | 336 | 0 |
| `objectives.zrd` | 14 413 | 12 410 | **50** |
| `pickups.zrd` | 26 823 | 60 | 0 |
| `startanims.zrd` | 26 883 | 277 | 0 |
| `targets.zrd` | 27 160 | 898 | 0 |
| `weather.zrd` | 28 058 | 3 218 | 0 |
| `copilot_pkup.zrd` | 31 276 | 20 094 | 0 |
| `hangar_drop.zrd` | 51 370 | 13 810 | 0 |
| `objcomplete.zrd` | 65 180 | 1 231 | 0 |
| `bmanrun.zrd` | 66 411 | 27 542 | 0 |
| `bmanwalk.zrd` | 93 953 | 7 447 | 0 |
| `hangar_panic.zrd` | 101 400 | 3 114 | 0 |
| `ladder.zrd` | 104 514 | 8 003 | 0 |
| `zepstate.zrd` | 112 517 | 1 767 | 0 |

Size and position are measurably **not** the rule: three members are longer
than the control member (`bmanrun.zrd` is 2.2x its length), and it is the
eighth member, not the first. The binding asserts both facts, so a future
"longest member is the program" or "first member is the program" regression
fails a test.

## What M02-B adds, in owner paths

- `crates/cs_content/src/campaign_bindings.rs` (owner path) adds
  `MissionControlBinding`, `ControlMemberRow`, `ControlProgramError` and
  `SourceContext::control_program`. The method resolves the work order
  exactly like `SourceContext::bind` — same title confirmation, same join,
  same campaign position — so this binding cannot name a different mission
  than `missions/bindings/M02.json` names; then it reads the archive the
  layout declares, locates and decodes every member through the production
  discovery the F13-B census uses, applies the control-member rule to the
  whole member set, measures the record, and cites the chosen member's own
  span and digest. A member that fails to decode refuses the whole binding
  (`ControlProgramError::Decode`), never a narrowed one: the rule judges the
  archive's complete member set, and skipping an unreadable member could
  hide the second block-carrying member the rule exists to refuse.
- `crates/cs_app/tests/campaign/m02_b.rs` (owner path): the eight
  `accept_m02_b_*` tests. `crates/cs_app/tests/campaign/evidence.rs` (owner
  path): the `evidence_report_m02_b_writes_the_acceptance_report` harness
  plus this task's test lists. It is deliberately not prefixed
  `accept_m02_b_`, so a task selection never picks it up as an acceptance
  test.
- Wiring only (AGENTS rule 1): `crates/cs_app/tests/campaign/main.rs` (one
  `mod m02_b;` and a doc paragraph, noting that the `accept_m02_b_` prefix is
  shared with `m02_t3.rs` from Rally #450).
- No `Cargo.toml` or `Cargo.lock` change: this stage adds no dependency and
  no crate edge.
- `missions/bindings/` is unchanged: this stage adds no new file there. The
  `M02.json` record keeps its empty `objective_ids` and its "objective
  graph: not bound" unknown — the blocks this stage measures are not
  objective *content ids*, and the product-incompleteness state stays where
  `AUDIT-PLAN-SYNC` puts it (the binding record and `docs/findings/`), never
  narrowed to pass a validator.

## Where the M02 sheet's regression priorities live, and what stays unmeasured

The sheet's priorities are designs; the predicates must come from the source
program and reference runs. This stage locates each one in the measured
record so a later runtime stage knows exactly what to predicate, and assigns
no timing, count or coordinate the record does not spell:

- **Success versus destruction.** Exactly one block spells the bare
  `INSTANTWIN` (the success latch) and it is woken by exactly one other
  block — measured in-degree 1, so the latch cannot fire before that block
  completes. Two blocks spell bare `INSTANTLOSS` (the failure causes), one
  reachable through a remaining-target block's completion edge. Whether the
  *wrong actor*, the *wrong session* or a *repeated event* can satisfy these
  latches is runtime behaviour and stays **unmeasured**; nothing here
  simulates it.
- **Player-aircraft transfer / capture versus destruction.** The record
  spells both sides of the target-flag vocabulary (`ADD`/`REMOVE`
  `_OBJECTIVE`/`_OTHER` `_TARGET`) and the inactive-member evaluator family
  (`INACTIVE1`…`INACTIVE15`, `INACTIVE_COMPLETION_COUNT`), which is the data
  those transitions are built from. The chain lookup a member name performs
  and the in-play bit it reads are the stage-B findings' measured effects
  with their own residual unknowns; the *transfer* of the player's aircraft
  is not in this member at all (it belongs to the mission program members
  this stage does not decode) and remains unbound.
- **Remaining-target failure.** Five blocks carry `INACTIVE_COMPLETION_COUNT`
  thresholds; the independent re-walk asserts each threshold is a positive
  count no larger than the member lists its own block spells — "destroy N of
  M" is record data, not a guess.
- **The block graph is closed; every spelled address is in range.**
  M02's block `OBJECTIVE13` spells `WAKE_OBJECTIVE_WHEN_I_COMPLETE [14, 50]`
  and the record declares 50 blocks numbered 1…50: under the measured
  convention the spelled integer is the *one-based block number*, so `50`
  names `OBJECTIVE50`, the last block, and no address points past the
  record's own end. What the original does with a genuinely out-of-range
  address — none exists in M02 — is **unmeasured**; the engine-side rule
  (`cs_sim::objectives::address`) refuses it by name and the test never
  silently clamps.

  > **Corrected by M02-B-FU3 (#802) and M06-B-FU3 (#819), 2026-10-10.** This
  > bullet previously read the spelled integer as a *zero-based record
  > index*, called `50` dangling and left "what the original does with it"
  > open as #802. #802 measured the parse's `dec` at `0x468c40`/`0x468cf0`/
  > `0x4679fc` — the spelled value is the block number, decremented at
  > storage — so the address was never dangling, and #819 reconciled the
  > test to the convention. See
  > `2026-10-09-m02-b-fu3-out-of-range-wake-address.md` for the measurement
  > and `2026-10-10-m06-b-fu3-objective-address-convention.md` for the
  > reconciliation. The same misreading also under-counted the success
  > latch's edge: block 15 is named by `OBJECTIVE14`'s
  > `NAP_OBJECTIVE_WHEN_I_COMPLETE [15, 22.0]` — a nap, not a wake.

## The measured compatibility gap, and why it is not papered over

> **Closed by #800 (`M02-B-FU1`), 2026-10-09.** The follow-up measured the
> same evidence the row below records and decided the question this note
> left open: the list spelled beside a list-taking objective directive
> (`WAKE_OBJECTIVE`, `WAKE_OBJECTIVE_WHEN_I_COMPLETE`,
> `SLEEP_OBJECTIVE_WHEN_I_COMPLETE`, `KILL_OBJECTIVE_WHEN_I_COMPLETE`,
> `WAKE_OBJECTIVE_WHEN_I_SLEEP` — the measured operations `WakeObjectives`,
> `SleepObjectives`, `KillObjectives`, `WakeObjectivesOnTransition`) is
> carried as **one** `Value::List` argument, so its length is the list's and
> not an arity; `MAX_CALL_ARGS` stays 8. M02's record now lowers completely
> and its census row is complete through the census's own verdict. The gap
> pin below became `accept_m02_b_fu1_the_kill_sites_lower_through_one_list_argument_and_m02_lowers`
> in that change, and the positional over-bound refusal arm it used to carry
> stayed in `accept_m02_b_a_site_over_the_host_call_bound_refuses_and_one_at_the_bound_binds`
> on the positional key `IDENTITY`. The rest of this section is the record of
> the gap as this stage measured it.

M02's directive vocabulary is **fully measured**: 36 of 36 keys carry a
stage A–D disposition, none is Unmeasured. Yet the record does not lower:

1. `KILL_OBJECTIVE_WHEN_I_COMPLETE` appears at 8 sites whose argument lists
   disagree in shape — inner lists of 9, 3, 3, 3, 2, 1, 1 and 1 integers.
2. The lowering adapter registers one `BindingSpec` signature per measured
   shape, and `cs_script::bindings::HostBindingRegistry::register` refuses
   any signature longer than `MAX_CALL_ARGS` (8) — one over-bound signature
   refuses the **whole** spec rather than narrowing what the name accepts.
3. The key therefore never enters the registry; all 8 sites refuse as
   `unknown host call`; `call_arguments` is the only unmet requirement (the
   mission identity, the 50 objective identities and all 50 completion
   conditions lower cleanly); no `MissionProgram` assembles and
   `MissionProgram::validate` is never reached.
4. The census reports M02's row incomplete and `campaign_ready()` stays
   false. That is the contract's honest reading — an undecodable/unlowerable
   program is `Unsupported`, never a guessed one — so nothing in this stage
   loosens it.

The synthetic test
`accept_m02_b_a_site_over_the_host_call_bound_refuses_and_one_at_the_bound_binds`
carries the same mechanism on authored records: a kill site **at** the bound
of 8 arguments binds and the record completes; one integer over it refuses
the key's registration and the record with it. So CI covers the refusal arm
without original data, and the follow-up that changes the bound (#800) must
update both pins in its own change — never delete them to get green.

The measured operation (`kill_objectives`: "kill each listed index") semantically
takes *one list* of indices, so the likely correct fix is in how the adapter
shapes list-taking directives, not in raising the cap that protects the
registry from untrusted programs. That judgement is #800's to measure and
make; this stage records the evidence and does not pre-decide it.

## Files

- `crates/cs_content/src/campaign_bindings.rs` — the production binding
  (types, errors, `SourceContext::control_program`).
- `crates/cs_app/tests/campaign/m02_b.rs` — the eight acceptance tests (nine
  after #800 renamed the gap pin and added its own synthetic arm).
- `crates/cs_app/tests/campaign/evidence.rs` — the evidence harness and the
  task's test lists.
- `crates/cs_app/tests/campaign/main.rs` — wiring only.
- `docs/findings/evidence/M02-B.json` — the committed copy of the acceptance
  report (the artifacts it hashes stay in `private/evidence/M02-B/`).

## Test inventory (`accept_m02_b_*`, 8 tests as landed; 9 after #800)

| Test | What it pins |
| --- | --- |
| `accept_m02_b_m02s_control_program_is_bound_to_the_same_identities_as_its_mission_binding` (retail) | the control binding and M02-A's mission binding name one mission (`mission/ch1-m02`) and one program (`script/c1-m02-zrdr`); the archive's length and digest re-derive from disk; exactly one member declares numbered blocks and it is the member the binding names; the member's span lies inside the archive and its digest re-derives from the member's own bytes; a longer member exists and the control member is not the first, so size and position are not the rule; the census read through its own discovery path names the same container, the same member and the same record |
| `accept_m02_b_the_measured_vocabulary_partitions_and_refuses_no_m02_key` (retail) | 50 blocks / 190 sites / 36 keys; sites sum to the record total; the partition is exactly 2 implemented + 34 measured + 0 unmeasured; every measured key names operation, effect and evidence; the outcome keys are bare and answer only for their own names; the five measured record fields each occur once; the five record-level sound keys are named and stay outside the vocabulary |
| `accept_m02_b_the_objective_graph_the_sheet_priorities_need_is_measured_not_invented` (retail) | an independent production re-walk of the control member sees every block; every cross-objective address is in range under the measured one-based rule (`OBJECTIVE13` → 50 names `OBJECTIVE50`; reconciled from the zero-based misreading by #819); one success latch napped by exactly one block (OBJECTIVE14); two failure latches (OBJECTIVE24 named by block 3's kill and block 19's nap, OBJECTIVE36 by block 7's nap); five remaining-target thresholds each no larger than their block's member lists; the target-flag and inactive-member vocabularies are spelled; the census and the binding agree on one measurement |
| `accept_m02_b_fu1_the_kill_sites_lower_through_one_list_argument_and_m02_lowers` (retail; renamed from `accept_m02_b_the_lowering_gap_is_named_and_the_campaign_gate_stays_closed` by #800) | M02's control record lowers completely: no unbound key, no unmet row, every site bound through a measured signature, every completion condition lowered, the bound program validating; the kill key still disagrees in shape and still spells a shape over `MAX_CALL_ARGS`, but registers one single-list signature per measured shape and every one of its eight sites carries its index list as one `Value::List` argument; the census row is complete and M02 joins the complete rows only through the census's own verdict |
| `accept_m02_b_the_control_rule_refuses_an_archive_without_or_with_two_control_members` (synthetic) | the rule refuses an archive with no block-carrying member (naming the container and the member count) and one with two (naming both, sorted), and chooses the single carrier from its own record |
| `accept_m02_b_the_vocabulary_partition_is_exact_on_an_authored_record` (synthetic) | on an authored record: bare outcome implements, covered key measures, unknown key stays unmeasured, sites sum, an unclassified record key is named not read, a measured record field is counted |
| `accept_m02_b_a_site_over_the_host_call_bound_refuses_and_one_at_the_bound_binds` (synthetic) | the refusal mechanism at the bound, on the positional key `IDENTITY` since #800: 8-argument site binds and completes; 9-argument site refuses the key's registration, refuses its record and leaves the key out of the registry — the bound is not raised by the follow-up |
| `accept_m02_b_fu1_a_long_index_list_binds_as_one_list_argument` (synthetic, added by #800) | a nine-index `KILL_OBJECTIVE_WHEN_I_COMPLETE` site lowers where nine positional arguments would refuse: one single-list signature for the one measured shape, the indices arriving as one list in order; a list wider than `MAX_VALUE_ITEMS` still refuses by site, never truncating |
| `accept_m02_b_a_disagreeing_key_keeps_every_shape_and_a_text_follower_is_the_next_key` (synthetic) | a key whose sites disagree keeps both shapes with their site counts (`[text,int]`, `[text,int,text]`); a text follower is the next directive's key, so the outcome site is bare; three authored sites are counted exactly |

Every test calls production code
(`SourceContext::control_program`, `SourceContext::bind`,
`discover_container`, `decode_zrd`, `control_member`,
`measure_control_record`, `lower_control_record`,
`survey_mission_control_programs`). No test repeats an expected value read
from the record it checks: the retail assertions re-derive from
`$CS_GAME_DIR` through a second production walk, or compare two independent
derivations against each other. The retail tests are
`#[ignore = "requires CS_GAME_DIR"]` (CI skips them); the synthetic ones run
in CI.

## Mutation probes

Five mutations were applied one at a time, each reverted before the next;
the tree carried none of them afterwards (`git status --porcelain` clean but
for this note). Every mutation was observed on the full `accept_m02_b_`
selection:

| Mutation | Observed result |
| --- | --- |
| `control_program` picks the archive's first member instead of the measured rule | **4 of 8 fail**: the identities, vocabulary, graph and gap tests — the binding names a member that declares no blocks, so every retail test that re-derives through it breaks |
| the control member's digest is the whole archive's digest | **1 fails**: the identities test, at the member-digest re-derivation |
| `measure_control_record` reports no unclassified record keys | **2 of 8 fail**: the retail vocabulary test and the synthetic partition test — the five M02 sound keys vanish without a refusal anywhere |
| `cs_script::bindings::MAX_CALL_ARGS` raised 8 → 16 (the follow-up's direction) | **1 fails**: the gap pin — no key refuses registration, so the retail refusal assertions break; the synthetic bound test adapts to the new constant and still passes, which is exactly the pair #800 must update together |
| the control-member rule accepts every member (`> 0` → true) | **5 of 8 fail**: the synthetic rule-refusal test plus all four retail tests that touch the binding — every archive becomes ambiguous |

## Checks

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_m02_b_ --include-ignored` | 0 (14 tests: this stage's 8 plus `m02_t3.rs`'s 6, which share the prefix from Rally #450) |
| `python3 tools/validate_evidence.py private/evidence/M02-B/acceptance.json --artifact-root private/evidence/M02-B --require-pass` | 0 (`structurally_valid: true`) |

The five mutation probes above were run between the workspace checks, each
reverted before the next; `git status --porcelain` after the last one showed
only this note.

## Recorded unknowns (not guessed)

- **The join remains an inference** (M02-A's standing unknown): this stage
  consumes it through `SourceContext::bind` and adds no second evidence for
  the mapping itself.
- **No directive is implemented by a measured effect.** The 34 measured keys
  carry the stage A–D findings' static readings; a host operation for them
  is future work, and only the two outcome spellings are implemented
  (as `Lowering::Finish`).
- **M02's control record does not lower** — the measured gap above, **closed
  by #800 (`M02-B-FU1`) on 2026-10-09**: the kill sites' index lists are
  carried as one list argument and M02's row is complete. Lowering evidence
  only — no directive effect is implemented, so M02 is still not ready; the
  campaign gate stays closed on the rows that remain incomplete (M03's
  refused sites among them).
- **The five record-level sound keys are uninterpreted** — their consumer
  in the original is unmeasured; filed as #801.
- **The out-of-range wake address's behaviour is unmeasured** — filed as
  #802.
- **Runtime predicates are unobserved.** No original executable was run; the
  wrong-actor / wrong-session / repeated-event halves of the sheet's
  priorities need ordinary-play observation (M02-C) and stay open. No
  reference capture exists for M02 (REF-OWNER-FIRST-CAPTURE is blocked on
  the owner).
- **Which other reader members carry gameplay this stage does not decode**
  (`aiv.zrd`, `bmanrun.zrd`, `zepstate.zrd`, …) is unmeasured here: the
  control program is one member; the mission program members the player-aircraft
  transfer likely lives in are future measurement, not assumption.
- **Player-aircraft transfer has no engine consumer yet**, and M02 is not
  declared launchable (`cs_app::mission_launch` declares M01 only). Both
  belong to the runtime stages, not to this binding.

## Sources

`$CS_GAME_DIR` read-only through `cs_assets::install::discover`,
`SourceContext::read`, `cs_formats::script_raw::discover_container`,
`cs_content::stunts::decode_zrd`; `missions/M02.md`;
`docs/contracts/SCRIPT-MISSION.md`; `docs/contracts/CLI-EVIDENCE.md`;
`docs/findings/2026-10-01-m02-a-source-binding.md`;
`docs/findings/2026-10-04-m01-lc-mission-program.md`;
`docs/findings/2026-10-06-m01-lc-directive-{b,c,d}-*.md`;
`crates/cs_app/tests/accept_m01_lc_mission_program.rs`;
`crates/cs_app/src/mission_control.rs`;
`crates/cs_app/src/control_lowering.rs`.
