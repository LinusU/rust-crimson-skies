# M06-B-FU1: M06's three `COMPLETION_COUNT` `ANIM_STATE` sites, lowered and pinned

Date: 2026-10-09. Task: `M06-B-FU1` (#817), a follow-up of M06-B (#274,
`missions/M06.md`, `docs/findings/2026-10-09-m06-b-compatibility-gaps.md`).
Shared contract: `docs/contracts/SCRIPT-MISSION.md`; evidence per
`docs/contracts/CLI-EVIDENCE.md`. Capabilities used: `retail` (`$CS_GAME_DIR`
read-only, never written) and `synthetic` (authored `.zrd` records).
Implementer: **Devin SWE-2/swe2-max-1** (Rally #817, session of 2026-10-09).
No reviewer yet; an implementer's own run is not independent review and no
agent review replaces the owner's human approval.

## What this task is

M06-B measured the gap and pinned it: of M06's eight `ANIM_STATE` sites,
three — blocks 9, 11 and 41 of `zbd/c2/m01`'s control member — spell six
operands (`COMPLETION_COUNT [1]` plus two `ANIM` descriptors) where the
then-current lowering accepted only a single-pair site of two. The fix
direction the M06-B findings recorded was landed first as **M04-B-FU1**
(#806), the same mechanism M04-scoped, and M06-B's suite was re-pinned on
that landing. This task is the M06-scoped follow-up: it re-measures the
three sites' own spelled contents, pins them under the task's own
`accept_m06_b_fu1_` prefix, and writes the task's evidence report. **No
production code changes**: the shared mechanism already lowers M06; what
this change adds is the M06 pins and the evidence.

## The sites, re-measured from the retail record

Read again from `ZBD/C2/M01/zrdr.zbd` member `objectives.zrd` through
`read_control_member` + `zrd_flat_fields` (never from the earlier note's
prose): each of the three sites is its block's **first** directive and
spells, verbatim:

```
ANIM_STATE [ COMPLETION_COUNT [1],
             ANIM [NAME [pathN_continue],   STATE [RUNNING]],
             ANIM [NAME [pathN_accelerate], STATE [RUNNING]] ]
```

with `pathN` = `path3` (block 9), `path4` (block 11), `path5` (block 41).
That is exactly what the measured helper walk (`0x4691d0`, re-confirmed in
the decrypted image by #806,
`docs/findings/2026-10-09-m04-b-fu1-anim-state-operand-list.md`) describes:
every `ANIM`/spec pair appends, `required` counts the appended pairs, and
the `COMPLETION_COUNT` found inside the same operand list overwrites it —
so each site lowers to `AnimationStates { required: 1, animations:
[(pathN_continue, Running), (pathN_accelerate, Running)] }`. The operand
list is the call's one `Value::List` argument, so a six-operand site binds
as a list, never as an arity.

## What changed

* `crates/cs_app/tests/campaign/m06_b.rs` — three `accept_m06_b_fu1_*`
  tests (below), the `anim_descriptor`/`m06_anim_state_operands` helpers
  and the module doc; no other test moved or weakened.
* `crates/cs_app/tests/campaign/evidence/m06_b.rs` — the M06-B report's
  required test lists now include the three FU1 members, since the
  `accept_m06_b_` selection picks them up (the same fold-in M04-B's lists
  took when #806 landed).
* `crates/cs_app/tests/campaign/evidence/m06_b_fu1.rs` — new harness
  `evidence_report_m06_b_fu1_writes_the_acceptance_report` and this task's
  test lists; one `mod m06_b_fu1;` line in `evidence.rs` (wiring).
* `crates/cs_app/tests/campaign/main.rs` — the `m06_b` paragraph names the
  FU1 members (wiring-only doc edit).
* `docs/findings/evidence/M06-B-FU1.json` — the committed acceptance
  report (artifacts stay in `private/evidence/M06-B-FU1/`).

## Test inventory (`accept_m06_b_fu1_`, 3 tests)

| Test | What it pins |
| --- | --- |
| `accept_m06_b_fu1_the_completion_count_sites_lower_and_m06s_record_completes` (retail) | `ANIM_STATE` is 8 sites in shapes `[(2,5),(6,3)]`; blocks 9/11/41 each carry it as the first directive spelling exactly the measured operand list above; all 265 calls bind and `unbound_keys` is empty; each of the three lowered conditions is the two-pair evaluator with `required = 1` under the site's own names; each call and each bound `AnimationStates` action carries the six-item operand list whole; `validation` is `Some([])`; every lowering requirement is met and the row is complete |
| `accept_m06_b_fu1_m06_is_complete_and_the_campaign_stays_unready` (retail) | M06's completeness is the census's own verdict (`is_measured`, zero unmet requirements, `lowering.complete()`, `is_complete()`), the row is in `complete_missions()`, and `campaign_ready()` stays false on other missions' gaps |
| `accept_m06_b_fu1_m06s_spelling_lowers_all_three_sites` (synthetic) | the three sites' verbatim spelling authored on a three-block record lowers the same way — per-site own-name pairs under `required = 1`, six-item `Value::List` call arguments, program validates — into CI, where there is no original data |

Every test calls production code (`survey_mission_control_programs`,
`read_control_member`, `measure_control_record`, `lower_control_record`);
the retail pin re-derives the spelled operand lists from `$CS_GAME_DIR`
through an independent walk rather than trusting the lowered output. The
M06-B gap pin (`accept_m06_b_every_call_binds_every_condition_lowers_and_m06s_record_completes`)
was already updated when #806's re-pin landed and is untouched here — not
deleted to get green — and the two synthetic refusal arms
(`accept_m06_b_a_completion_count_site_lowers_with_its_override`,
`accept_m06_b_a_wide_kill_list_binds_and_a_wide_non_index_key_still_refuses`)
still pass unchanged.

## Checks

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_m06_b_fu1_ --include-ignored` | 0 (3 tests) |
| `cargo test --workspace --locked -- accept_m06_b_ --include-ignored` | 0 (11 tests) |

The acceptance run the evidence report records is the `accept_m06_b_fu1_`
selection, tee'd into `private/evidence/M06-B-FU1/cargo-test.log`; the
report is validated with `tools/validate_evidence.py --require-pass` and
copied to `docs/findings/evidence/M06-B-FU1.json`.

## Not claimed

Nothing here is `verified_original` and no mission was played: the runtime
half of `AnimationStates` — which world writes put `pathN_continue` /
`pathN_accelerate` into `RUNNING`, and whether one of the pair matching is
enough on a real run — belongs to M06-C and stays open, exactly as finding
C and the M06-B note left it. `campaign_ready()` remains false on other
missions' gaps. M06-B-FU2 (#818, passenger identity) and M06-B-FU3 (#819,
the sibling suite's address reading) are untouched.

## Sources

`$CS_GAME_DIR` read-only through `read_control_member` /
`survey_mission_control_programs` / `decode_zrd`;
`docs/findings/2026-10-09-m06-b-compatibility-gaps.md`;
`docs/findings/2026-10-09-m04-b-fu1-anim-state-operand-list.md`;
`docs/findings/2026-10-06-m01-lc-directive-c-ai-world-and-animation-directives.md`;
`crates/cs_script/src/conditions.rs`;
`crates/cs_app/src/control_lowering.rs`.
