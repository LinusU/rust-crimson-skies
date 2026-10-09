# M08-B: The Petrol Plot's mission-specific compatibility surface

Date: 2026-10-09. Task: M08-B "Implement and regress mission-specific
compatibility gaps" (#280, `missions/M08.md`, work order `M08-B`).
Shared contract: `docs/contracts/SCRIPT-MISSION.md`. Capabilities used:
`retail` (`$CS_GAME_DIR` read-only, never written) and `synthetic` (newly
authored `.zrd` values). Implementer: **bunny-alpha-2/bunny-alpha-2** (Rally
#280, session of 2026-10-09T02:22Z). Reviewers:
**bunny-alpha-1/bunny-alpha-1** (Rally review claims of 2026-10-09T05:38Z
and of 2026-10-09T08:22Z — the second round resolved the first landing
conflict) and **Devin SWE-2/swe2-max-1** (Rally review claim of
2026-10-09T09:52Z — the third round resolved the second landing conflict),
each a different agent instance with fresh context. Those reviews are
independent of the implementation, but they are agent reviews of the code
and the tests: they are not independent original-reference evidence and not
original-run evidence, and no agent review replaces the owner's human
approval. The evidence report's `review.identity` is read at run time from
`CS_EVIDENCE_REVIEWER`, so each agent writes its own.

The stage's minimum acceptance scenario is *"All discovered mission-specific
behavior uses production engine systems and regression tests."* It holds for
everything this note measures: M08's discovered mission-specific behavior is
its mission control program — the typed keyed list the installation ships as
`objectives.zrd` inside `ZBD/C2/M03/zrdr.zbd` — and this stage binds it
through production systems (`SourceContext::control_program`,
`cs_formats::script_raw::discover_container`, `cs_content::stunts::decode_zrd`,
`cs_content::mission_control`, `cs_app::mission_control`,
`cs_app::control_lowering`) and regresses it with seven `accept_m08_b_*`
tests. **One** compatibility gap remains and is pinned, not worked around;
it was already filed by its discoverer and is not duplicated here (see
below). M08's control record does **not** lower completely, so the mission
stays Unsupported and the campaign gate stays closed. The second gap this
stage was written against — the host-call bound — was closed on `main` by
#800 *while this branch was in flight*, so the pins were rewritten against
the landed behaviour and now assert both halves.

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
| Lowering attempt | mission `mission/ch2-m03`; 57 objectives; 209 call verdicts — **all 209 bound**, no key refuses registration; 57 condition verdicts — 49 lowered, **8 refused**; a `RawProgram` and a `MissionProgram` assemble, and `validate` reports exactly one unsupported instruction (the `OBJECTIVE17` danger-zones condition) | `cs_app::control_lowering::lower_control_record` through the census row |
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
  accounting, directive counts, the lowering's verdicts and the gap that
  remains). It is
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

## The lowering gap that remains, and the one that closed underneath this stage

M08's directive vocabulary is **fully measured**: 22 of 22 keys carry a
M01-LC disposition, none is Unmeasured, and no block is unreadable. When
this stage first measured the record, two mechanisms kept it from lowering
and both were pinned. **#800 (`M02-B-FU1`) landed on `main` while this
branch was in flight and closed the first one**, so the pins were rewritten
against the landed behaviour rather than left describing a gap that no
longer exists:

1. **Closed: `KILL_OBJECTIVE_WHEN_I_COMPLETE`, 19 sites.** Measured on this
   installation before #800 landed, the key's six shapes (1, 5, 6, 7, 9 and
   10 integers) ran against the registry's per-signature argument bound
   (`MAX_CALL_ARGS`, 8): the ten-argument lists of `OBJECTIVE28`…
   `OBJECTIVE33` (six sites) and the nine-argument list of `OBJECTIVE43`
   (one site) refused the whole spec, all 19 sites refused as
   `unknown host call` and `call_arguments` was unmet. #800's change (*carry
   objective-index directive lists as one list argument*) means every one of
   M08's 209 sites now binds, no key refuses registration and the program
   assembles. The retail test asserts that state — an empty
   `unbound_keys`, zero refused sites, the kill key registered with
   single-argument signatures, the ten-index shape still measured as the
   longest — so a regression in either direction fails it, and the synthetic
   test carries the mechanism (M08's six shapes, plus the over-wide refusal
   arm) into CI. Not duplicated: #800 owns the mechanism, M02-B-FU1's own
   suite pins it from M02's side.
2. **Open: `DANGER_ZONES_COMPLETED`, 8 refused completion conditions.**
   `OBJECTIVE17`…`OBJECTIVE24` — the whole danger-zone chain — are the eight
   blocks whose conditions this build refuses:
   *"the danger-zones flag evaluator is measured but this build lowers no
   condition for it, and offering one would be a guess at its predicate"*
   (`crates/cs_script/src/conditions.rs`). The directive itself is measured
   (`DirectiveOperation::DangerZoneFlags`), so this is a lowering gap, not an
   unknown directive; the other 49 conditions lower. The refusal surfaces
   twice in the accounting: as the `objective_condition` row itself, and as
   the single `MissionProgram::validate` error the unknown condition raises
   (which is what makes `call_arguments` unmet) — the test names both and
   proves no host call is refused. It is already filed as **#813
   (`M07-B-FU1`)** by the M07-B stage. Not duplicated.

In either state the census reports M08's row incomplete and
`campaign_ready()` stays false — the contract's honest reading. Nothing in
this stage loosens it, lowers an unmeasured directive to a no-op or clamps an
address.

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
| `accept_m08_b_the_danger_zones_condition_is_the_gap_that_keeps_m08_unlowered` (retail) | M08 does not lower completely; `objective_condition` and `call_arguments` are the two unmet rows and the test names what each one carries; no key refuses registration and all 209 sites bind, including the kill key's 19; the kill key's longest measured shape is the ten-index list (over `MAX_CALL_ARGS`, which is why #800's shaping is what binds it) and its signatures are single-argument; the program assembles and `validate` reports exactly one unsupported instruction, the `OBJECTIVE17` danger-zones condition; exactly 8 condition verdicts refuse, every one naming `DANGER_ZONES_COMPLETED` and `OBJECTIVE17`…`OBJECTIVE24`, with the other 49 lowered; M08 is not a complete census row and the campaign gate stays closed |
| `accept_m08_b_the_three_sheet_priorities_locate_in_the_measured_record` (retail) | the six team pairs and six net pairs of `OBJECTIVE3` and the 17 `DEDG` sites (logistics state); the eight-zone chain and its eight-target handover, the 190-second timed alternative and the two-way kill between the pair (alternative objective paths); the stoppoint record, the hook-point target and the single `TRAVELERS` operands (protected transfer); and the negative half — no key of the pinned vocabulary names an interaction, docking, pickup, boarding, transfer or authorisation directive |
| `accept_m08_b_m08s_kill_shapes_bind_as_one_list_argument_and_an_over_wide_one_refuses` (synthetic) | M08's six kill shapes (1, 5, 6, 7, 9, 10 indices) each register, arrive as one `Value::List` argument in order and complete their record; the over-wide list refuses the record and the site is dropped by name, never truncated |
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

## Review

Reviewed by **bunny-alpha-1/bunny-alpha-1** on 2026-10-09 in a fresh session
(a different agent instance from the implementer `bunny-alpha-2`), against
`missions/M08.md` (`M08-B`), `docs/contracts/SCRIPT-MISSION.md`,
`docs/contracts/CLI-EVIDENCE.md`, `AGENTS.md` and the diff. Review is
independent of the implementation; it is an agent review of the code and
tests, not independent original-reference evidence and not original-run
evidence.

- Rebased onto `origin/main` (`32b9d823`) with one conflict: `main`'s M04-B
  doc paragraph and this stage's paragraph in
  `crates/cs_app/tests/campaign/main.rs` were inserted at the same place.
  Both were kept, and this stage's paragraph — which still described the
  pre-#800 state ("**both** measured gaps", naming the host-call-bound
  `KILL_…` sites as open) — was corrected to what the tests assert now: all
  209 sites bind, and the eight danger-zones completion conditions (#813)
  are the one gap that keeps M08's record from lowering. That is a doc
  correction inside an owner path; no test or assertion changed.
- Main moved under the branch by M04-B, M10-B and the `CS_ENGINE_IMAGE`
  rename. Checked for interaction with this stage's pins:
  `crates/cs_script/src/conditions.rs` still refuses
  `DANGER_ZONES_COMPLETED` (so the gap pin holds), the branch changes no
  `Cargo.toml`/`Cargo.lock`, and apart from that one doc paragraph no file
  the branch touches is touched by those landings.
- Checks on that rebased tree (2026-10-09, `0ef3046a`): `cargo fmt --all --
  --check` (0), `cargo clippy --workspace --all-targets --all-features
  --locked -- -D warnings` (0), `cargo test --workspace --locked` (0: 474
  `test result: ok` lines, no failure), `cargo test --workspace --locked --
  accept_m08_b_ --include-ignored` (0: **7 discovered, 7 executed, 7 passed,
  0 failed, 0 ignored**, tee'd to `private/evidence/M08-B/cargo-test.log`).
- Two mutation probes, each applied alone, run and reverted, with the
  selection re-run green and the tree clean afterwards:

  | Mutation | Result |
  | --- | --- |
  | `MAX_CALL_ARGS` 8 → 16 (`cs_script::bindings`) | 1 of 7 fails: `accept_m08_b_the_danger_zones_condition_is_the_gap_that_keeps_m08_unlowered` (its `10 > MAX_CALL_ARGS` proof of #800's shaping) |
  | `control_member`'s measured rule → first member offered (`cs_content::mission_control`) | 5 of 7 fail (only the two synthetic tests pass) |

  So the retail pins are the tests that notice a broken derivation, and the
  CI-covered synthetic pair carries the two lowering mechanisms without
  original data.
- Evidence regenerated from that run with the recipe in the `evidence.rs`
  header (step 1 tee, step 2 harness with `CS_EVIDENCE_REVIEWER` naming this
  review), written to `private/evidence/M08-B/acceptance.json` and validated
  with `tools/validate_evidence.py … --require-pass` (exit 0,
  `structurally_valid: true`). The committed copy in
  `docs/findings/evidence/M08-B.json` is that file; against the implementer's
  copy only `candidate_tree` (the tree of the commit this review landed in;
  the report copy itself is the later delta the report's `review.method`
  declares), `created_at`, `command.cwd`, this run's `cargo-test.log`
  artifact hash, the assertion order and `review.identity` — which now
  names the reviewer instead of the hand-over note — differ;
  `m08-control-program.json` hashes identically, so the second production
  observation of M08's control program is byte-identical across the two runs.
  `python3 -m unittest discover -s tools/tests -p 'test_evidence_review_identity.py'`
  → 27 tests, OK (M08-B has no Rally review snapshot entry yet, so its
  implementer/reviewer pair stays an advisory note there, as the reader
  documents).
- No protected path, no original data and no binary file changed: the branch
  carries the two test files, the `mod m08_b;` + doc wiring in `main.rs`,
  this finding and the evidence copy.

### Landing conflict, round two (2026-10-09)

The approval at `8b34faf2` could not be fast-forwarded: the lander reported
*rebase conflict with main; a reviewer must rebase it by hand*, so the task
came back to review. The same independent reviewer (`bunny-alpha-1`,
a fresh session and claim, still a different agent instance from the
implementer `bunny-alpha-2`) rebased the branch onto the moved `origin/main`
by hand. Two owner files conflicted:

- `crates/cs_app/tests/campaign/main.rs` — this stage's doc paragraph and
  main's `record_objectives_sound.rs` paragraph had been inserted at the
  same place. Both were kept, and a stray conflict-marker `<` left in the
  first resolution attempt was removed before any check ran (`cargo fmt`
  caught it).
- `crates/cs_app/tests/campaign/evidence.rs` — this stage's M08-B constants
  and harness had been inserted where main had since added its M02-B-FU3
  constants, harness and `address_record`, its M02-B-FU2 harness and the
  RECORD-OBJECTIVES-SOUND constants and harness. Resolved as a **union**:
  main's blocks kept verbatim after `parse_m02_b_fu1_suite`, this stage's
  M08-B block kept verbatim beside them, so the merged file is `origin/main`'s
  `evidence.rs` plus this branch's additions only — `git diff --stat
  origin/main...HEAD` shows 350 insertions and no deletion in it — and every
  harness `evidence.rs` names (M02-B, M02-B-FU1, M02-B-FU2, M02-B-FU3,
  M04-B, RECORD-OBJECTIVES-SOUND, M08-B) is present exactly once.

Checks on the conflict-rebased tree: `cargo fmt --all -- --check` (0),
`cargo clippy --workspace --all-targets --all-features --locked -- -D
warnings` (0), `cargo test --workspace --locked` (0: 476 `test result: ok`
lines, no failure), `cargo test --workspace --locked -- accept_m08_b_
--include-ignored` (0: **7 discovered, 7 executed, 7 passed, 0 failed, 0
ignored**, tee'd to `private/evidence/M08-B/cargo-test.log`). The evidence
report was then regenerated from that run with the `evidence.rs` recipe and
validated again, and the branch was pushed for CI on the rebased head.

### Landing conflict, round three (2026-10-09)

The approval at `a94c48f4` could not be fast-forwarded either: the lander
reported *rebase conflict with main; a reviewer must rebase it by hand* a
second time, so the task came back to review again. A third reviewer —
**Devin SWE-2/swe2-max-1** (Rally review claim of 2026-10-09T09:52Z, a
different agent instance and model from both the implementer
`bunny-alpha-2` and the earlier reviewer `bunny-alpha-1`, in a fresh
session and claim) — rebased the branch by hand onto the moved
`origin/main` (`09b2618c`, which had landed M02-B-FU2, M07-B and F58-B
since `756b862e`). This time only one owner file conflicted:

- `crates/cs_app/tests/campaign/main.rs` — this stage's doc paragraph and
  main's new `m07_b.rs` paragraph had been inserted at the same place. Both
  were kept (M07-B's first, then M08-B's); nothing else in the file differs
  from `origin/main`.
- `crates/cs_app/tests/campaign/evidence.rs` auto-merged this round: the
  branch's M08-B block still sits between `parse_m02_b_fu1_suite` and
  main's M02-B-FU3 block, and the merged file remains `origin/main`'s
  `evidence.rs` plus this branch's additions only — `git diff --stat
  origin/main...HEAD` shows 350 insertions and no deletion, and the branch
  adds exactly one `evidence_report_*` function over main's 31
  (`evidence_report_m08_b_writes_the_acceptance_report`).

Checks on the third conflict-rebased tree (`635cf5bf`): `cargo fmt --all --
--check` (0), `cargo clippy --workspace --all-targets --all-features
--locked -- -D warnings` (0), `cargo test --workspace --locked` (0: 480
`test result: ok` lines, no failure), `cargo test --workspace --locked --
accept_m08_b_ --include-ignored` (0: **7 discovered, 7 executed, 7 passed,
0 failed, 0 ignored**, tee'd to `private/evidence/M08-B/cargo-test.log`).
The evidence report was regenerated from that run with the `evidence.rs`
recipe (`CS_EVIDENCE_REVIEWER` naming all three agents of record) and
validated again with `tools/validate_evidence.py … --require-pass` (exit 0,
`structurally_valid: true`). The `m08-control-program.json` artifact hashes
identically to the earlier rounds (`3134ddc3…`), so the second production
observation of M08's control program is byte-identical across three
rebases; only `candidate_tree`, `created_at`, `command.cwd`, the
`cargo-test.log` artifact hash and `review.identity` differ in the report.
The earlier reviews' mutation results still stand: no production file
changed in any of the three rebases, only the doc paragraph, the harness
placement and the findings/evidence copies moved.

### Landing conflict, round four (2026-10-09)

The approval at `27743251` could not be fast-forwarded either — the lander
reported *rebase conflict with main; a reviewer must rebase it by hand* a
third time — so the task came back to review a fourth time. This round's
reviewer is **bunny-alpha-2/bunny-alpha-2** (Rally review claim of
2026-10-09T10:50Z): the **same agent name as the implementer**, in a fresh
session and a fresh context, but therefore **not an independent review** —
it is recorded here and in the report's `review.identity` exactly that way.
The independent reviews remain rounds one and two (`bunny-alpha-1`, different
agent instance) and round three (`Devin SWE-2`, different agent instance *and*
model); none of them is original-run or original-reference evidence, and no
agent review replaces the owner's human approval.

The branch was rebased by hand onto the moved `origin/main` (`549ef7e4`,
which had landed M05-B since round three's `09b2618c`). One owner file
conflicted:

- `crates/cs_app/tests/campaign/main.rs` — this stage's doc paragraph and
  main's new `m05_b.rs` paragraph had been inserted at the same place. Both
  were kept (M05-B's first, then M08-B's); nothing else in the file differs
  from `origin/main`.
- `crates/cs_app/tests/campaign/evidence.rs` auto-merged again: verified to
  be `origin/main`'s file plus this branch's additions only — `git diff
  --stat origin/main...HEAD` shows 350 insertions and no deletion — and the
  branch still adds exactly one `evidence_report_*` function over main's
  (`evidence_report_m08_b_writes_the_acceptance_report`), each harness present
  exactly once.

Main's new M05-B landing touches only `crates/cs_app/tests/campaign/` and
`docs/findings/`; it changes no production file and no `Cargo.toml` /
`Cargo.lock`. The two pins this stage's retail test leans on were re-read on
that main and hold: `crates/cs_script/src/conditions.rs` still returns
`NoConditionFor` for `DANGER_ZONES_COMPLETED` ("this build lowers no
condition for it"), and `MAX_CALL_ARGS` is still `8`.

Checks on the fourth conflict-rebased tree (`1141b3de`): `cargo fmt --all --
--check` (0), `cargo clippy --workspace --all-targets --all-features
--locked -- -D warnings` (0), `cargo test --workspace --locked` (0: **480**
`test result: ok` lines, no failure), `cargo test --workspace --locked --
accept_m08_b_ --include-ignored` (0: **7 discovered, 7 executed, 7 passed,
0 failed, 0 ignored**, tee'd to `private/evidence/M08-B/cargo-test.log`).
One mutation probe was run in this round, on the rebased tree, alone and
then reverted (tree clean afterwards, selection re-run green):

  | Mutation | Result |
  | --- | --- |
  | `control_member`'s measured rule → first member offered (`crates/cs_content/src/mission_control.rs`) | 5 of 7 fail — the five retail tests; only the two synthetic tests pass |

  which reproduces round one's probe on today's main and proves the retail
  pins still notice a broken derivation.

The evidence report was regenerated from that run with the `evidence.rs`
recipe (`CS_EVIDENCE_REVIEWER` naming all four agents of record and marking
round four as *not* independent) and validated with
`tools/validate_evidence.py … --require-pass` (exit 0,
`structurally_valid: true`): `candidate_tree` `1141b3de…` (the rebased tree
the suite ran on), `created_at` 2026-10-09T11:26Z, `command.cwd` this
review's worktree, `cargo-test.log` hash `d33c24bb…`. The
`m08-control-program.json` artifact still hashes to `3134ddc3…`, so the
second production observation of M08's control program is byte-identical
across four runs and four rebases; only `candidate_tree`, `created_at`,
`command.cwd`, the log's hash, the assertion order and `review.identity`
differ.

### Round four, second push: main moved again (2026-10-09)

CI on `ee3db465` completed *success* (jobs `pack`, `rust lint`,
`rust test-rest`, `rust test-app` all green), but `origin/main` had moved
underneath it — M06-B landed as `55057ba3`, touching the same two owner
files — so landing it then would have run into the same conflict a fifth
time. The branch was rebased a second time in this round, by hand, onto
`55057ba3`. That rebase applied **without conflicts** (main's `m06_b.rs`
paragraph sits apart from this stage's) and the result was verified:
`git diff --stat origin/main...HEAD` still shows `evidence.rs` as 350
insertions and no deletion over main, exactly one `evidence_report_*`
function added over main's, each of the three doc paragraphs (`m06_b`,
`m05_b`, `m08_b`) present once, `mod m08_b;` present once, and no conflict
markers.

The commits this rebase brought in **do** touch files this branch changes
(`evidence.rs`, `main.rs`), so the owner's lighter rebase check does not
apply and the full four checks were re-run on the new tree (`f61eca5e`):
`cargo fmt --all -- --check` (0), `cargo clippy --workspace --all-targets
--all-features --locked -- -D warnings` (0), `cargo test --workspace
--locked` (0: **480** `test result: ok` lines, no failure),
`cargo test --workspace --locked -- accept_m08_b_ --include-ignored` (0:
**7 discovered, 7 executed, 7 passed, 0 failed, 0 ignored**, tee'd to
`private/evidence/M08-B/cargo-test.log`). M06-B's landing changed no
production file (tests and `docs/findings/` only), so this stage's two pins
are untouched by it.

The evidence report was regenerated from that run — `candidate_tree`
`f61eca5e…`, `created_at` 2026-10-09T12:07Z, `cargo-test.log` hash
`774a7997…` — and validated again (`structurally_valid: true`, exit 0); the
`m08-control-program.json` artifact still hashes to `3134ddc3…`, byte
identical across every run of this stage's harness.

## Recorded unknowns (not guessed)

- **The join remains an inference** (M08-A's standing unknown): this stage
  consumes it through `SourceContext::bind` and adds no second evidence for
  the mapping itself. The `Plot` / `Pit` spelling discrepancy of the two
  localized rows is likewise M08-A's unknown and is not resolved here.
- **No directive is implemented by a measured effect.** The 20 measured keys
  carry the M01-LC findings' static readings; a host operation for them is
  future work, and only the two outcome spellings are implemented (as
  `Lowering::Finish`).
- **M08's control record does not lower completely** — the danger-zones
  gap above. M08 stays `Unsupported`; the campaign gate stays closed; the
  fix is **#813** (danger-zones condition), already filed when this stage
  measured it, so no duplicate task was created. The other gap this stage
  pinned (the host-call bound) was closed by **#800** on `main` while this
  branch was in flight, and the pins now assert the landed behaviour.
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
Rally tasks #800 (`M02-B-FU1`, landed on `main` as
`f2c74585 Carry objective-index directive lists as one list argument`), #802
(`M02-B-FU3`), #807 (`M02-B-FU4`), #812 (`M10-B-FU1`), #813 (`M07-B-FU1`).
