# F39-E3: the objective-record census extended to the shared and world-group readers

Date: 2026-10-03. Task: F39-E3 "Extend the objective-record census to the
shared and world-group readers" (Rally #597). Shared contract:
`docs/contracts/SCRIPT-MISSION.md`. Capability used: `retail` (read-only
`$CS_GAME_DIR`). Evidence report: `private/evidence/F39-E3/acceptance.json`,
committed as `docs/findings/evidence/F39-E3.json`.

## The question

F39-D's census measured only mission-scoped reader archives
(`zbd/<group>/<mission>/zrdr.zbd`). Its unknown #5 was the denominator: the
shared reader (`ZBD/zrdr.zbd`) and the world-group readers
(`ZBD/<group>/zrdr.zbd`) were outside it, so a mission might inherit objective
declarations the census did not see. This stage decides whether the
denominator is right — by measuring, not by arguing.

## What is measured

`survey_retail_objective_records` now walks every `zrdr.zbd` the mission-scope
rule does **not** claim (`non_mission_reader_scope` owns the partition:
`zbd/zrdr.zbd` → `zbd`, `zbd/<group>/zrdr.zbd` → `zbd/<group>`; anything else
fails the census rather than being silently skipped). Every member of each
archive is decoded with the production `.zrd` reader and measured through
`reader_member_measurement` — the same `objective_state_machine` the mission
rows use, so an `OBJECTIVE<N>` block counts identically wherever it sits. A
member that cannot be decoded, an index that cannot be fully read, or a member
with no name **fails the census** (`ObjectiveCensusError`), because an
unmeasured gap is exactly where a hidden block would live. Members carrying a
measured mission-control member name (`map`, `aiv`, `egen`, `net`, `ia`,
`objectives`, `targets` `.zrd` — F14-D.1/F13-B's per-mission member set) are
flagged `mission_control`; a `targets.zrd` member is additionally read through
the production target-record readers.

The result is published per reader as `RetailObjectiveReaderRow`: the archive's
span and SHA-256, **every** member (name, span, SHA-256, block count, in-block
key vocabulary, the mission-control flag, and for `targets.zrd` the record
count and record-key vocabulary), plus the archive's totals. The member list
is the searched set — the negative bound is published, not asserted.

## The measured answer

| measurement | value |
| --- | --- |
| non-mission readers measured | 9 (`zbd` + `zbd/{c1,c1b,c1c,c2,c2b,c3,c4,c5}`) |
| member index entries across them | 612 (221 + 70 + 29 + 28 + 70 + 25 + 56 + 57 + 56) |
| members that decoded | 612 of 612 (a refusal would fail the census) |
| `OBJECTIVE<N>` blocks declared by any member | **0** |
| in-block key occurrences | 0 (the per-reader vocabulary is empty) |
| members carrying a mission-control member name | **1**: `zbd/c1c`'s `targets.zrd` |
| objective target records in it | 5 |

The five `ZBD/C1C/zrdr.zbd::targets.zrd` records are objective **target**
records in the measured `targets.zrd` shape: keys `description` (5),
`nodes` (5), `help_label` (5), `category_label` (3), `objective` (1), with
labels `MSG_OBJ_KLONDIKE`, `MSG_OBJ_WVOYAGE`, `MSG_OBJ_WVOYAGEHOOK`,
`MSG_OBJ_DARKANGEL`, `MSG_OBJ_ZEPPELIN`, `MSG_OBJ_DEFEND`, `MSG_OBJ_DISABLE`,
`MSG_OBJ_DOCK` — shared world targets C1C's missions can inherit.

A total text-atom sweep (every text value in every decoded member, any depth)
was also run during development: the only `objective`-family spellings
anywhere outside mission scope are that `targets.zrd` member plus
`ZBD/zrdr.zbd::Briefing.zrd`'s `OBJECTIVESLIST`/`Objective` UI element names —
briefing-screen widget names, not declarations — and no member anywhere
carries the branching, optionality or outcome key vocabulary
(`WAKE_OBJECTIVE_WHEN_I_COMPLETE`, `NAP_*`, `KILL_*`,
`WAKEUP_OBJECTIVE_WHEN_DAMAGED`, `TICK_DEPENDS_ON_OBJ`, `INACTIVE*`,
`INSTANTWIN`, `INSTANTLOSS`).

## The verdict

**Widened census, stated denominator.** The census now measures all nine
readers and publishes them — but the `OBJECTIVE<N>` *block* denominator stays
the mission-scoped 53 readers / 1338 blocks, and that denominator is now
**proven complete**: zero blocks exist anywhere else in the installation's
readers. The one measured nuance: objective *target* records (the
`targets.zrd` shape — the things a state machine's conditions can name, not
state machines themselves) can be inherited from the world-group reader in
C1C; the census publishes them rather than pretending mission scope is
airtight for every declaration family.

F39-D's unknown #5 is resolved and annotated as such in its findings.

## Unknown / deferred (not guessed)

1. **What the five shared target records mean.** Their keys and labels are
   measured; whether a C1C mission's objectives actually *consume* them, and
   how an inherited record is matched to a mission's `objectives.zrd`, is
   unmeasured — the same consumer question F42-D's target decoding carries.
2. **Whether other declaration families can be inherited.** This bound covers
   the objective vocabulary (blocks, target records, the measured key
   spellings). AI directives (`aiv.zrd`), scene scripts, or other families are
   outside this census's vocabulary by construction — no member in the nine
   readers carries those mission-control names either, but a family not yet
   named would not be caught here.
3. **`Briefing.zrd`'s `OBJECTIVESLIST` atoms.** Measured as UI element names
   on the briefing screen; what the screen does with them is display
   behaviour, not an objective declaration, and is unmeasured.

## Test inventory (`accept_f39_e3_*`)

- `accept_f39_e3_the_non_mission_scope_rule_names_shared_and_world_group_readers`
  — the scope rule on authored paths.
- `accept_f39_e3_member_measurement_counts_objective_blocks_like_the_mission_rows`
  — an authored `objectives.zrd`-shaped document counts its blocks and
  publishes its in-block vocabulary through the census's member measurement.
- `accept_f39_e3_a_targets_member_is_measured_as_objective_target_records` —
  an authored `targets.zrd`-shaped document yields its record count and
  complete record-key vocabulary.
- `accept_f39_e3_retail_shared_and_world_group_readers_bound_the_denominator`
  — `#[ignore = "requires CS_GAME_DIR"]`: pins the 9 readers, the 612-member
  searched set, zero `OBJECTIVE<N>` blocks, the single inherited
  `zbd/c1c/targets.zrd` member and its 5-record vocabulary, and the unchanged
  53/1338 mission denominator.

## Commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_f39_e3_ --include-ignored
cargo test --locked -p cs_app --test evidence_report_f39_e3 -- --ignored
python3 tools/validate_evidence.py private/evidence/F39-E3/acceptance.json \
  --artifact-root private/evidence/F39-E3 --require-pass
```

## Sources

`specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`,
`docs/contracts/SCRIPT-MISSION.md`, `docs/contracts/CLI-EVIDENCE.md`,
`docs/findings/2026-10-03-f39-d-branching-optional-and-failure-validation.md`
(unknown #5), `docs/findings/2026-10-02-f14-d-1-reader-archive-directories.md`
(reader inventory), `crates/cs_app/src/objectives.rs`,
`crates/cs_formats/src/script_raw/discovery.rs` (`mission_scope`,
`reader_programs`, `MISSION_CONTROL_MEMBERS`),
`crates/cs_content/src/stunts.rs` (`objective_state_machine`,
`objective_record_count`, `objective_record_keys`, `decode_zrd`).
