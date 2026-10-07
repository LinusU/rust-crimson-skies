# M01-LC-DIRECTIVE-LOWERING.03: lowering M01's control record into a `RawProgram`, and rows derived from that attempt

Task `M01-LC-DIRECTIVE-LOWERING.03` (Rally #726, stage of the parent task
`M01-LC-DIRECTIVE-LOWERING`, #717). Stage `.01` (#724) put the binding
vocabulary and multi-shape argument representation into `cs_script`, stage `.02`
(#725) put the side-effect-free block-condition lowering next to it; this stage
is the `cs_app` adapter that joins them to the record, and the point where
`ControlLowering`'s two rows stop being `unmet(...)` on principle and start
being **derived from what the attempt actually produced**.

## What this document is, and what it is not

A record of what production code did over the owner's installation, with the
figures re-derived there. It is **not** a statement that the original ran: no
original executable was started, no mission was played, no human looked at
anything. `retail` here means read access to the original files. Claim level is
`implemented`; a Rally merge awards `checked` and nothing more.

## The adapter

`crates/cs_app/src/control_lowering.rs`,
`cs_app::control_lowering::lower_control_record`, is the whole crossing:

1. **The walk.** It re-walks the record's own decoded `.zrd` document with the
   same asymmetric directive grammar the census measured — a site is a key plus
   the list beside it, a bare key is the measured no-argument spelling — and
   emits one `cs_script::conditions::RawBlock` per numbered `OBJECTIVE<N>`
   block.
2. **The conditions.** Each block goes through
   `cs_script::conditions::lower_record` (stage `.02`). A `Lowered` condition
   enters the `RawObjective` verbatim; a refusal, an unreadable block, or a
   block whose directive list the adapter could not represent faithfully
   (a scalar spelled beside its key) enters as `Condition::Unknown`, which
   `MissionProgram::validate` refuses rather than guesses, and the attempt
   reports the block's verdict as `ConditionOutcome::Refused` /
   `::Unreadable` with the field named.
3. **The calls.** Every directive site becomes one `RawCall`, its arguments
   carried field for field inside `Value::List` — nested lists stay nested,
   nothing is flattened. A site the IR cannot carry (an int beyond `i32`, a
   non-finite float, a list deeper or wider than the `Value` bounds, a scalar
   beside a key) produces no call and a `CallOutcome::Refused` naming the site,
   the block and the key.
4. **The registry.** Bindings are built from the record's own key
   dispositions: a `Measured` key registers one `BindingSpec` whose signatures
   are exactly the shapes its sites were measured to spell (a key whose sites
   disagree registers one signature per shape — none is chosen), a terminal
   key registers `Lowering::Finish`, and an `Unmeasured` key registers
   **nothing**, so its sites refuse `unknown host call` instead of binding to a
   convenient operation.
5. **The attempt.** The whole outcome comes back as a `LoweringAttempt` —
   plain data in `cs_content`, because that crate may not name `cs_script`'s
   types (`docs/01-ARCHITECTURE.md`) — carrying the mission id, the objective
   count, every condition verdict, every call verdict, the keys registration
   refused and `MissionProgram::validate`'s verdict.

`ControlLowering::measure(record, attempt)` and
`MeasuredControlRecord::is_complete(attempt)` read **only** that attempt for
what the lowering did. A row is met only while the attempt produced what the
row needs; a refused attempt unmet-lifts nothing, and every unmet row still
names the fields it lacks.

The mission id is not a member field: it comes from
`campaign_bindings::campaign_layout`, the same walk `SourceContext::read` and
the catalog baseline use, keyed by the reader archive's logical container key.
A reader the layout does not name produces `Err`, no `RawProgram`, and an
unmet `mission_identity` row carrying that reason — never an invented id.

## M01's attempt, as production measures it

| Figure | Value | Where it is asserted |
| --- | --- | --- |
| numbered blocks / `RawObjective`s | 58 / 58 | `accept_m01_lc_lowering_adapter_m01_lowers_into_a_validated_raw_program` |
| condition verdicts, all `Lowered` | 58 | same |
| directive sites / bound `RawCall`s | 353 / 353 | same |
| distinct keys / registry bindings | 43 / 43 | `..._every_key_is_bound_and_both_vocabulary_tables_agree` |
| measured keys → `Lowering::Directive` | 41 | same |
| terminal keys → `Lowering::Finish` | 2 (`INSTANTWIN`, `INSTANTLOSS`) | same |
| keys left unmeasured / dropped | 0 / 0 | same |
| `MissionProgram::validate` | passes (`Some([])`) | `..._m01_lowers_into_a_validated_raw_program` |
| lowered actions | 351 `Action::Directive` + 2 `Action::Finish` | same |
| sites spelled with a nested list argument | 14 | same |
| `RetailControlRow::is_complete()` for `zbd/c1c/m01` | **true** | `..._the_census_reports_complete_rows_and_keeps_the_gate` |

Every figure above is re-derived from the installation by the test run; none is
read out of this document.

### The two vocabulary tables, compared rather than trusted

Acceptance asked for a cross-crate check, and it lives in
`accept_m01_lc_lowering_adapter_every_key_is_bound_and_both_vocabulary_tables_agree`:
for each of M01's 43 keys the test takes the operation code `cs_content`'s
measured table publishes, looks it up in `cs_script`'s own
`DirectiveOperation::from_code`/`ALL` table, requires the codes to be the same
string, and requires the registered `BindingSpec` to carry that exact
operation. The record's key table and the registry are also compared for size
and membership (43 keys, 43 specs, every key present), so a key the adapter
dropped fails here rather than in a site total somewhere else. Neither table is
trusted on its own: a code only `cs_content` knew would refuse at registration
and be named in `unbound_keys`, and a name only the registry knew cannot appear
because registration is driven by the record.

## Fail-closed witnesses

Every refusal acceptance names still refuses, by name, with its row unmet
naming the field:

| Case | Test |
| --- | --- |
| a directive key with no measured effect (`SET_AI_`) | `..._an_unmeasured_key_refuses_its_sites_by_name` — the site refuses `unknown host call`, the `call_arguments` row names the key and `meaning_not_measured`, no program assembles |
| a block whose evaluator stage `.02` refused (two `DEDG` in one block) | `..._a_block_the_condition_lowering_refuses_stays_unmet` — `ConditionOutcome::Refused` naming `OBJECTIVE9`/`DEDG`, `objective_condition` unmet on that field |
| a scalar spelled beside its key (`not_a_list`) | `..._a_scalar_site_is_refused_and_damages_its_block` — the site refuses, the block's condition verdict is overridden to a refusal (a predicate over a partially represented directive list is not the record's predicate) |
| a block the walk cannot read | `..._an_unreadable_block_is_carried_as_a_refusal` — `RawBlock::Unreadable`, `Condition::Unknown`, validation refuses it |
| an empty record | `..._an_empty_record_stays_unmet` — three unmet rows, each naming what it lacks |
| a record whose mission id never resolved | `..._a_missing_mission_id_produces_no_program` — no `RawProgram`, `mission_identity`/`objective_identity`/`call_arguments` unmet with the reason |
| a record whose attempt never ran | `..._the_rows_report_the_attempt_not_the_vocabulary` — all four rows unmet, none lifted by vocabulary |

Corpus-wide the gate stays closed: `campaign_ready()` is `false`, because 13 of
the 53 mission-scoped readers declare no control program at all and because
every row whose directives include a key no finding covers stays incomplete
(the census test asserts `complete ⇒ unmeasured().is_empty()`, which is
exactly that gate). The five rows that do lower completely are `zbd/c1/m05`,
`zbd/c1c/m01`, `zbd/c3/m03`, `zbd/c4/m03` and `zbd/c5/m03` — reported by
`complete_missions()`, which is the corpus's positive list, not a claim that
the campaign is releasable.

## What "lowered" does not mean

* A bound call emits `Action::Directive { operation, args }`. Its whole effect
  is carrying the measured operation and the site's own arguments to the host
  (`docs/contracts/SCRIPT-MISSION.md`, "Host interface"). It is **not**
  evidence that a world-side handler exists: `DirectiveDisposition::is_implemented`
  stays `false` for every measured key, and only the two terminal spellings
  reach an engine operation the program itself performs.
* `is_complete()` for M01 means the record lowered into a validated
  `MissionProgram` with no refusals. It does not mean the mission is playable,
  that its evaluators have been watched evaluating, or that the directives do
  what the findings measured in a live run. Runtime behaviour remains for the
  mission runtime's host emission and the owner's human review.
* The measured residual unknowns (IDENTITY's third child, the in-play flag's
  writers, DEDG's member-field rewrites, TRAVELERS' polarity, the sound-group
  handles, the animation call's trailing arguments) are unchanged by this
  stage: they stay named on the dispositions and on the rows that carry them.
  They were not resolved here and are not implied to be harmless.

## The launch surfaces

`RetailControlRow::is_complete()` and `RetailControlRow::lowering()` — the two
surfaces a launch path reads — now answer from the row's own attempt:
`true`/all-met for M01, `false` with named fields for a row with an unmeasured
directive (asserted both ways in
`..._the_census_reports_complete_rows_and_keeps_the_gate`).

`crates/cs_app/src/mission_launch.rs` and the `--mission` CLI were **not**
touched: they belong to the blocked task #359, as
`docs/findings/2026-10-06-m01-lc-directive-meaning.md` already recorded. What
#359 will read is exactly these rows, and they now report the intended thing.

## Reconciling the suites that pinned today's "unmet" state

The suites that asserted `unmet` for `objective_condition`/`call_arguments`
were updated to the new measured truth, never weakened: the row expectations in
`accept_m01_lc_mission_program.rs`,
`accept_m01_lc_every_mission_is_measured_and_the_gate_reports_who_lowers`
(renamed from `..._and_none_is_campaign_ready`, now asserting the positive list
*and* that the gate stays closed),
`accept_m01_lc_every_directive_m01_spells_is_measured_and_only_outcomes_run`
(now asserting 41 measured + 2 terminal = 43 with M01 complete),
`accept_m01_lc_directive_e_the_lowering_rows_fail_closed` (the measured record's
four rows are met; the empty record's three are still unmet),
`accept_m01_lc_directive_e_m01_is_measured_but_never_supported` →
`..._m01_lowers_completely_while_measured_is_not_implemented` (the rows are met
and measured is still not implemented), the module docs of
`accept_m01_lc_directive_meaning.rs`, and the launch-gate assertion beside it.
The five `evidence_report_*` harnesses whose artifacts quote the old rows were
reconciled to the new API, and the report prose three of them carried —
"lower_program's four requirements are unmet for every measured archive … so no
mission is lowered", "the condition and the calls stay unmet … and no mission
reports complete", "the exact set of argument shapes the mission IR cannot
carry" — was rewritten during review to interpolate what the census measures
(`{complete_rows}` of `{measured_rows}` rows lower completely, nested argument
lists carried as nested values, one signature per disagreeing shape) instead of
asserting what this stage made false; all six artifacts were then regenerated
on the reviewed tree by their own harnesses, never edited by hand. No assertion
was deleted, no test was skipped and no `#[ignore]` was added to dodge the
reconciliation.

## How to re-derive every number here

```sh
cargo test --workspace --locked -- accept_m01_lc_lowering_adapter_ --include-ignored   # 12 tests, 3 retail

CS_EVIDENCE_DIR=private/evidence/M01-LC-DIRECTIVE-LOWERING \
CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_lc_lowering_adapter_ --include-ignored" \
CS_EVIDENCE_EXIT_CODE=0 CS_EVIDENCE_REVIEWER=<identity> \
  cargo test --locked -p cs_app --test evidence_report_m01_lc_directive_lowering -- --ignored

python3 tools/validate_evidence.py private/evidence/M01-LC-DIRECTIVE-LOWERING/acceptance.json \
  --artifact-root private/evidence/M01-LC-DIRECTIVE-LOWERING --require-pass
```

The report's second artifact (`m01-directive-lowering.json`) is a second
production observation over the installation: the corpus gate (readers,
measured rows, absent readers, the complete list, `campaign_ready`, the corpus's
unmet-row count), M01's whole attempt (mission id, objectives, condition
verdicts, call verdicts, bindings, validation), the binding registered for each
of the 43 keys beside the disposition it came from, the four requirement rows,
and which of the six findings documents this tree holds.

## Status and limits

* `implemented` only. This task awarded itself no `checked`, no
  `verified_original` and no `release_approved`.
* Affected content and what would still block a fidelity claim: M01's 353
  directive sites reach the host as `Action::Directive` emissions whose
  world-side behaviour nobody has observed; the 13 mission-scoped readers with
  no control program and the corpus keys no finding covers (`SET_AI_`,
  `WAKEUP_OBJECTIVE_WHEN_I_COMPLETE`, `Change`, `to`, `mobile`, `net`) keep
  their rows unmet. Resolving tasks: the parent `M01-LC-DIRECTIVE-LOWERING`
  (#717) for the integration step, #359 for wiring one original mission into
  the playable application, and the stage documents the findings cite for any
  residual unknown a later reading corrects.
* `docs/findings/2026-10-04-m01-lc-mission-program.md` (the four-row table this
  stage supersedes for M01) and `docs/findings/2026-10-06-m01-lc-directive-meaning.md`
  (the disposition chain and the two launch surfaces) are the prior record;
  their "why the record does not lower yet" sections are historical for M01 and
  were left in place rather than rewritten — the superseding text is here.
