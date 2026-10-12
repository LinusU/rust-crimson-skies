# M01-B-OPC: text directive programs in the F13-C probe and coverage report

Date: 2026-10-12. Task: **M01-B-OPC** (#1356), re-scoped by the owner on
2026-10-12 from "measure the opcode signature table" to "classify reader-text
members as directive programs". Shared contract: `docs/contracts/SCRIPT-MISSION.md`.
Capabilities used: `retail` (read-only `$CS_GAME_DIR`) and ordinary build/test.

## The premise that was wrong

M01's binding said "F13-C probes every program against an empty signature
table". The probe walked every located program as binary words. A mission
reader archive's control member (`objectives.zrd`) is not binary words: it is a
decoded `.zrd` list of named directive keys. Walking it stopped on its first
text word with `unknown_opcode`, and a numeric `SignatureClaim` for that would
have described the probe's misreading of text bytes. No such claim is written.

## What changed

- `cs_formats::script_raw::probe`: `DirectiveProgramProbe`, `DirectiveKeyRow`,
  `DirectiveKeyStatus` and `ProbeSession::record_directive_program`. A session
  files a text directive program with the keys it reaches instead of walking
  it. It refuses an empty key, a key listed twice and an unknown key without a
  reason.
- `cs_inspect scripts`: for a reader container, the control member is chosen by
  `cs_content::mission_control::control_member` (the one decoded member that
  declares numbered `OBJECTIVE<N>` blocks, never by name). Its keys come from
  `measure_control_record`. Each key is `measured` (with the findings it cites,
  from `measured_directive`), `terminal_outcome` (`INSTANTWIN`/`INSTANTLOSS`, a
  reading of the spelling) or `unknown` with `meaning_not_measured` /
  `disagreeing_argument_shape`. A container with no unique control member, or a
  member that does not decode, is not classified and keeps the binary probe.
- Report version `cs-inspect-scripts/3`: `probe.directive_programs`,
  `probe.directive_keys_known`, `probe.directive_keys_unknown` and
  `probe.directive_program_rows`. A directive program counts as accounted for
  (not `unresolved`); a block that does not read as a directive list leaves
  `signatures_complete` false.

## Measured over the installation (`cs-inspect scripts`, no `--signatures`)

1452 located programs: 40 are directive programs (one per mission archive that
declares objective blocks), 1412 are still binary-walked and stop at
`unknown_opcode` (animation containers, INTERP bodies and the other reader
members, whose role is unestablished). Directive keys over the 40 programs: 773
measured or terminal, 6 unknown.

**M01 (`zbd/c1c/m01`)**: 58 blocks, 353 sites, 43 distinct keys, 0 refused
blocks. 41 keys are measured (cited to the M01-LC-DIRECTIVE-A..D findings), 2
are terminal outcomes, **0 are unknown**. M01's directive program is therefore
fully accounted for.

The 6 unknown keys are in other missions: `c4/m01` spells the stray words
`Change`, `to`, `mobile`, `net` (`meaning_not_measured`), `c5/m04` spells
`SET_AI_` (`meaning_not_measured`) and `c1b/m03` spells
`WAKEUP_OBJECTIVE_WHEN_I_COMPLETE` (`disagreeing_argument_shape`). They are
listed in the report with that reason and are not guessed. They block no M01
claim, so no per-family measurement task was split off for M01.

## Not covered

- Only the control member is a directive program. `aiv.zrd`, `targets.zrd` and
  the other `.zrd` members are still probed as binary words; their role is
  unestablished and this task did not measure it.
- M01's INTERP loading body was not examined, so no binary-opcode finding is
  made for it.
- `terminal_outcome` is a reading of the spelling, as `cs_content` already
  records; it is not an engine measurement.
