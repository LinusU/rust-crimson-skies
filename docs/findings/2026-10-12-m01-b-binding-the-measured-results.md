# M01-B: the measured compatibility results now live in M01's binding record

Date: 2026-10-12. Task: **M01-B** (Rally #259) "Implement and regress
mission-specific compatibility gaps", the parent stage over #1358
(`M01-B-OBJ-1`), #1359 (`M01-B-OBJ-2`), #1360 (`M01-B-OBJ-3`), #1361
(`M01-B-OBJ-4`), #1355 (`M01-B-OBJ`), #1356 (`M01-B-OPC`) and #1357
(`M01-B-WAKE`). Test prefix `accept_m01_b_`. Capabilities used: `retail`
(read-only `$CS_GAME_DIR`) plus ordinary build/test. **No original
executable was run**; nothing here is `verified_original`. Owner paths used:
`crates/cs_content/src/campaign_bindings.rs`, `missions/bindings/` (the
`M01.json` record and the `README.md` bullet), `crates/cs_app/tests/campaign/`
(the `m01_b.rs` suite, its `main.rs` / `evidence.rs` module lines and
`evidence/m01_b.rs`), `docs/findings/`. Implementer: **opencode-go/
mimo-v2.6-flash** (Rally #259 implement claim of 2026-10-12, agent
bunny-alpha-1).

## Verdict

The three compatibility subtasks measured M01's gaps inside other
subsystems' owner paths; this parent stage binds their results into
`missions/bindings/M01.json` itself, in the record's own voice. The three
shared checklist entries M01's measurements settled — **the objective
graph**, **script/native coverage** and **the exact failure/success
precedence** — no longer read "not bound / unmeasured" on M01's record: each
is replaced by the measured statement, naming the census it rests on and
exactly what it did not settle. Every sibling mission's record and the
shared checklist are untouched, `verified` stays `false`, and the campaign
stays unready: binding a measured result is not awarding readiness
(AGENTS.md rule 8).

## What changed

* **Production** (`crates/cs_content/src/campaign_bindings.rs`): a new
  mission-scoped **checklist override** table
  `MISSION_SCOPED_CHECKLIST_OVERRIDES`, keyed by work order, with a public
  accessor `mission_scoped_checklist_overrides(&MissionLabel)` and a
  `source_binding_checklist()` accessor for the shared entries.
  `SourceContext::bind` now replaces a shared entry with the work order's
  measured statement when the table names it verbatim — the opposite job, at
  the same site, of the append-side `MISSION_SCOPED_UNKNOWNS` table
  (M06-B-FU4), which is untouched. The module documentation records both
  directions of the same rule: a limitation measured for one mission reaches
  only its record, and a question measured for one mission is settled on its
  record alone.
* **Record** (`missions/bindings/M01.json`): regenerated through production
  code by
  `regenerate_m01_b_writes_the_committed_record_through_production`
  (env-gated, `#[ignore]`d, deliberately not named with the acceptance
  prefix). The diff is exactly the three `unknowns` entries above;
  `accept_m01_a_the_committed_record_is_what_the_installation_derives`
  (8 of 8 green) byte-compares the committed file with the derivation as
  before, and its checklist needles ("objective graph", "script/native
  coverage", …) still find their entries because each replacement keeps the
  checklist prefix. `missions/bindings/README.md`'s M01 bullet records the
  regeneration.
* **Tests** (`crates/cs_app/tests/campaign/m01_b.rs`, two tests under the
  `accept_m01_b_` prefix, plus the regeneration harness):
  * `…_m01s_record_binds_the_measured_results_and_m02s_does_not` (retail):
    re-measures every number the entries claim through production code
    before pinning it — `recover_retail_objectives` on `zbd/c1c/m01` for the
    objective census (58 blocks, 358 fields, 308 recovered, the unrecovered
    45 `ControlDirective` + 5 `RecordDirective`, no `Unmeasured` family, the
    two outcome blocks and the `-1` sentinel both load behind), and
    `survey_mission_control_programs` for the directive census (58 blocks,
    353 sites, 43 keys, 41 measured + 2 terminal + 0 unknown dispositions).
    It then pins M01's derived **and** committed records carry the three
    statements verbatim, and that M02's derived and committed records — and
    the committed `M03`, `M07`, `M24` records — keep the shared text and
    name none of the M01-B verdicts.
  * `…_a_settled_checklist_entry_reaches_only_the_work_order_it_was_settled_for`
    (synthetic, runs in CI): over the whole declared inventory, M01's three
    rows are the only overrides; each names a shared checklist entry
    verbatim, keeps its prefix and cites its measurements; the other 23 work
    orders receive none; the append-side mechanism is untouched (M06 keeps
    its passenger-identity entry). Moving a replacement into the shared
    checklist fails this test.
* **Evidence harness** (`evidence/m01_b.rs`): the report cites the recorded
  `accept_m01_b_` acceptance log plus the derived **M01** and **M02**
  bindings (`m01-binding.json`, `m02-binding.json`), asserting before it
  writes that M01's derivation carries the three measured statements and
  M02's keeps the shared checklist.
* No `Cargo.toml` or `Cargo.lock` change; no protected path touched.

## Why a replacement table, not an appended entry

`M01.json` is generated: `SourceContext::read` + `SourceContext::bind`
derive it, and the M01-A suite byte-compares it with the committed file, so
the record can only say what production code derives. The shared
`SOURCE_BINDING_UNKNOWNS` table speaks for every mission, and after the
subtasks it stated the *opposite* of the measurement on M01's own record
("objective graph: not bound …" while M01's graph is measured). Editing the
shared table would assert the settled question for all 24 missions, which
is false for the siblings whose data was never measured; appending a second
entry would leave the record contradicting itself. The replacement table
does the only honest thing: on the one record whose data was measured, the
question is answered with its residuals named; on every other record the
question stays open with the shared text.

## What each entry binds, and what it stays unknown for

1. **objective graph** — bound: 58 `OBJECTIVE` blocks, 358 fields, 308
   recovered into the engine lifecycle model, 50 unrecovered and named
   (M01-B-OBJ); both terminals (`OBJECTIVE24` `INSTANTWIN`,
   `OBJECTIVE40` `INSTANTLOSS`) dormant behind the `-1` sentinel, woken only
   by the measured effect graph and observed world facts, each reached by a
   driven run of the production host (M01-B-WAKE). Residual (named in the
   entry): the in-play-bit writers, `DEDG`'s member-field rewrites,
   `TRAVELERS`' producers and the `TICK_DEPENDS_ON_OBJ` roots stay untraced
   off the terminal paths; `ObjectiveRecovery::program()` still refuses
   this record by the declared-schema gap; no original ordinary-play run
   exists.
2. **script/native coverage** — bound: the control member is a text
   directive program, so the old "empty signature table" premise did not
   describe it; the F13-C directive probe classifies it as 58 blocks, 353
   directive sites, 43 distinct keys, 41 measured + 2 terminal outcomes + 0
   unknown (M01-B-OPC). Residual: the other reader members are still
   binary-probed with unestablished role, M01's INTERP loading body was not
   examined, and no numeric binary signature claim was written.
3. **exact failure/success precedence** — bound: the record's only outcomes
   are the two terminal blocks, each reached through its own measured wake
   chain under the host's measured per-tick order, and each driven run
   settles on the program's own `Action::Finish` (M01-B-WAKE). Residual: no
   run reports both terminals' world facts, no original executable was run,
   and ordinary-play precedence stays with M01-C.

## What this does *not* change

* Nothing is `verified_original` or `ready`: `verified` stays false, every
  residual stays in the record, the campaign's readiness and progression
  are untouched, and M01-C remains the stage for ordinary-play success,
  failure and continuity with the owner's human evidence.
* VS-M01-CONTROLLED-RUNS (#360) consumes these fixes for automated
  controlled runs; automated input is not `human_play`.
* Every other mission's record and the shared 13-entry checklist are
  byte-identical to what M01-A … M24-A derive.

## Follow-up resolution statements

* Each replaced entry stays until it is settled: whoever re-measures the
  residuals (the untraced world-fact producers, the declared-schema carriers
  for `ObjectiveRecovery::program()`, the binary-probed members) edits the
  override table and regenerates the record with
  `CS_REGENERATE_M01_BINDING=1`; the ordinary-play residuals settle under
  M01-C with the owner's evidence.
* Nothing here supersedes the subtasks' own findings; the numbers are
  theirs, re-measured fresh by the acceptance test rather than copied.

## Checks

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 (535 suites, no failures) |
| `cargo test --workspace --locked -- accept_m01_b_ --include-ignored` | 0 (62 tests: the whole `accept_m01_b_` family — OBJ-1/2/3/4, OBJ integration, WAKE and the new `m01_b` suite) |
| `cargo test -p cs_app --test campaign --locked -- accept_m01_a_ --include-ignored` | 0 (8 tests, incl. the record-equality pin) |

## Evidence

This task needs the `retail` capability, so it follows
`docs/contracts/CLI-EVIDENCE.md`: the acceptance run is tee'd into
`private/evidence/M01-B/cargo-test.log`,
`evidence_report_m01_b_writes_the_acceptance_report` writes
`private/evidence/M01-B/acceptance.json` from that log, the candidate tree,
the toolchain, production discovery of `$CS_GAME_DIR` and the derived M01
and M02 bindings (nothing typed by hand), `tools/validate_evidence.py
… --require-pass` validates it, and the report is committed as
`docs/findings/evidence/M01-B.json`.

## Review (2026-10-13)

Reviewed by **opencode-go/mimo-v2.6-flash** (agent bunny-alpha-1, Rally #259
review claim of 2026-10-13) — the same agent name and model as the
implementer, but a separate session whose context was fresh: it started from
the task history, `missions/M01.md`, `docs/contracts/SCRIPT-MISSION.md` and
the branch diff. This is **not** independent review per AGENTS.md, and the
format and mission-semantics claims here still deserve a different agent
instance or model; a merge awards `checked` at most. What the review did:

* re-read the whole diff against the task description, the owner's 2026-10-12
  directive and the subtask findings, and checked every number the three
  replacement entries quote (58 blocks / 358 fields / 308 recovered / 45 +
  5 unrecovered; 353 sites / 43 keys / 41 + 2 + 0; `OBJECTIVE24`
  `INSTANTWIN`, `OBJECTIVE40` `INSTANTLOSS`, the `-1` sentinel) against
  `2026-10-12-m01-b-obj-integration-family-verdict.md`,
  `docs/findings/scripts/2026-10-12-m01-b-opc-directive-programs-in-the-f13-c-probe.md`
  and `2026-10-12-m01-b-wake-world-facts-that-wake-m01-finish-blocks.md` —
  all match;
* rebased onto `origin/main` `1be927d6` (conflict-free) and re-ran the full
  four checks on the rebased tree: `cargo fmt --check` = 0, `cargo clippy …
  -D warnings` = 0, `cargo test --workspace --locked` = 0, the
  `accept_m01_b_` selection with `CS_GAME_DIR` = 0 (62 tests, 0 failed), the
  `accept_m01_a_` pin = 0 (8 tests);
* applied two mutation probes, each reverted afterwards and each failing the
  test that pins it: keying `MISSION_SCOPED_CHECKLIST_OVERRIDES` at `M02`
  instead of `M01` (the synthetic inventory test fails), and dropping the
  replacement in `SourceContext::bind` (the retail binding-pin test fails);
* regenerated this task's evidence report on the reviewed tree through the
  producing harness, now carrying both identities, and re-validated it with
  `tools/validate_evidence.py --require-pass` (= 0).

No defect was found in the implementation; nothing was changed besides this
section and the regenerated `docs/findings/evidence/M01-B.json`.

## Sources

`$CS_GAME_DIR` read-only through `cs_assets::install::discover`;
`cs_content::campaign_bindings::{SourceContext, mission_scoped_unknowns,
mission_scoped_checklist_overrides, source_binding_checklist}`;
`cs_app::objectives::recover_retail_objectives`;
`cs_app::mission_control::survey_mission_control_programs`;
`missions/M01.md`; `missions/bindings/M01.json`;
`missions/bindings/campaign-inventory.tsv`;
`docs/contracts/SCRIPT-MISSION.md`;
`docs/findings/2026-10-12-m01-b-obj-integration-family-verdict.md`;
`docs/findings/scripts/2026-10-12-m01-b-opc-directive-programs-in-the-f13-c-probe.md`;
`docs/findings/2026-10-12-m01-b-wake-world-facts-that-wake-m01-finish-blocks.md`;
`docs/findings/2026-10-10-m06-b-fu4-mission-scoped-binding-unknowns.md`;
`crates/cs_app/tests/campaign/m01_a.rs`;
`crates/cs_app/tests/campaign/m06_b_fu4.rs`.
