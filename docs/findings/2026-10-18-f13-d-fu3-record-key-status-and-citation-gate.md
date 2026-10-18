# F13-D-FU3: machine-readable record-key status and the citation gate

Date: 2026-10-18. Task: **F13-D-FU3** (#1417), follow-up of F13-D (#640).
Capabilities used: `retail` (read-only `$CS_GAME_DIR`) and ordinary build/test.

## What was wrong

`cs-inspect scripts` rendered a directive program's record-level keys as a
`support` label plus a free-text `reason`/`detail` pair. Nothing in a row
distinguished a key **measured** by a cited finding from a key merely **outside
the two measured vocabularies**, and the reason string `unmeasured`-style
wording read as "not measured" when the honest state is "outside the
vocabulary".

## What changed

- `cs_formats::script_raw::RecordKeyStatus` is now `Measured { support,
  citations }` or `OutsideVocabulary { code, vocabulary, resolving_task }`
  (labels `measured` / `outside_vocabulary`). The free-text `detail` is gone;
  `code` stays the stable `outside_both_record_vocabularies`.
  `RecordKeyStatus::gate_violation` returns a stable code for a measured key
  with no citation and for an outside-vocabulary key that does not name its
  vocabulary or its resolving task.
- `cs-inspect scripts` (report `cs-inspect-scripts/6`): each `record_keys` row
  keeps `support`, `sites` and `shapes` and gains `citations` (measured) or
  `reason`/`vocabulary`/`resolving_task` (outside vocabulary). The probe section
  gains `record_key_gate: {passes, violations, dropped}`.
- The gate, `ScriptsSummary::record_key_gate_passes`, is part of the
  `--coverage` check **independently of `--signatures`** (it asks about
  provenance, not the opcode table): a violation or a census key missing from
  the listing exits 3. *Every measured key carries a citation; every unmeasured
  key names the vocabulary it is outside and the task that would resolve it.*
  (Wording per the F13-D proposal; see the provenance note on #1417.)

## Citations

- `MISSION_TIMER`, `PLAYER_INIT`, `RESTORE/EXECUTE/INVALIDATE_ANIMS`
  (`shape_measured`): `2026-10-04-m01-lc-mission-program` and
  `2026-10-06-m01-lc-directive-meaning`.
- Record-level sound keys (`effect_measured`): the `evidence` of
  `record_sound_disposition` (findings B, D and M02-B-FU2). A sound key whose
  disposition were refused would carry none and fail the gate, not pass.
- Resolving task for an outside-vocabulary key: **#1623** (F13-D-FU3-RESOLVE),
  a standing task created for this purpose. Retail has no such key
  (F13-C-RECKEYS: 205 classified, 0 unclassified), so it is not expected to
  be claimed until one appears.

## Retail

M01 (`zbd/c1c/m01`) keeps its five record fields with their `support` and
shapes; they now also carry the two citations. The retail F13-C/F13-D suites
pass locally with `--include-ignored`.

## Pre-existing fix

`accept_f13_c_retail_cli_probes_every_program_and_reaches_every_record` was
failing on `main` (1732 vs 320): F13-D's modelled program rows also spell
`"evidence": "…"`. The assertion now expects records plus modelled programs.
No assertion was weakened.

## Tests (`accept_f13_d_fu3_*`, `tools/cs_inspect/src/script_discovery.rs`)

`gate_flags_uncited_measured_and_unnamed_outside_keys`,
`summary_gate_fails_on_violation_or_dropped_key`,
`production_rows_are_cited_and_complete`; the CLI tests
`accept_f13_c_reckeys_*` assert the new row fields and `record_key_gate`.

## Not covered

No end-to-end fixture drives the CLI to exit 3 on a violation: production rows
are always cited and named, so the failing inputs are exercised at the status
and summary level.
