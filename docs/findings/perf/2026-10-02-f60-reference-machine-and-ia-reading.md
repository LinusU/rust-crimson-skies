# F60 follow-up: the owner reference machine profile and the "IA" reading

Task #469. Complements
`2026-10-01-f60-a-benchmark-scenarios-and-budgets.md`. Nothing here is a
measurement of the original game or of any machine: the reference machine is
still unrecorded, and this file does not guess it.

## 1. Owner reference machine profile (required, not yet supplied)

F60's budgets are stated for a fixed hardware profile (sheet: "Primary
performance target is a playable original-content experience on the owners
machine"; "Set budgets after measuring a fixed baseline"). No agent can read
the owner's personal machine, and AGENTS rule 4 forbids guessing hardware
values, so `HardwareProfile::baseline_measured` stays `false` and the load,
peak-memory, cache and frame-p99 limits stay `BudgetLimit::Unset`.

### What the owner must supply

The owner supplies one record. Every field is free text; unknown fields must be
marked `unknown` rather than omitted.

| Field | Example shape | Why it is needed |
| --- | --- | --- |
| Reference machine name | a short stable id, e.g. the machine label the owner uses | the profile id in `HardwareProfile` and every budget row |
| CPU | vendor, model, core/performance-core counts | CPU-bound simulation and load budgets |
| GPU | vendor, model; integrated or discrete; VRAM / unified memory | frame p95/p99 and cache budgets |
| RAM | total GiB | peak-memory budget and soak trend plausibility |
| OS | name + version (macOS/Windows/Linux architecture) | `Platform`; supported-platform claims |
| Display | resolution(s) the 1080p target is judged at | `HardwareProfile::resolution` (currently designed 1920x1080) |
| Quality settings | the quality preset the baseline is taken at | `HardwareProfile::quality` (currently `Reference`) |

Once supplied, the values are recorded as data (not prose) and consumed by:
`HardwareProfile` (which today carries only id/platform/resolution/quality and
`baseline_measured`, and therefore needs the machine fields added when this
lands), then F60-B/F60-D replace the designed limits with `BudgetLimit::Measured`
and set `baseline_measured = true` only after a real baseline run on that
machine.

### Status

Unmet and owner-gated. This task cannot finish without the owner's record; a
blocked task request names the fields above. No value on this page is measured.

## 2. "IA" in F60 AC01 means Instant Action, not an AI engagement

F60 AC01 (and F60-A's minimum scenario) say "a 60-minute designed soak across
mission/IA/menu cycles". F60-A read `IA` as an AI-engagement phase and named the
soak's middle phase `SoakPhase::AiEngagement`.

The project's own vocabulary resolves it the other way:

- `specs/F49-instant-action-presets-and-custom-scenarios.md` is "Instant Action
  presets and custom scenarios" and abbreviates that feature `IA` throughout:
  "An IA mission is not just campaign launch with rewards disabled"
  (non-negotiable 1), "Complete/retry IA and verify campaign cash/progression
  unchanged" (AC03), "Build IA selection/customization/loadout UI" (F49-C).
  `docs/00-SCOPE.md` and `docs/research/FINDINGS.md` use the same abbreviation
  for the Instant Action catalogs.
- The cycle is the game's three top-level modes: campaign mission, Instant
  Action, menu. An "AI engagement" is not a mode; AI runs inside both mission
  phases, which is why `scenario::SystemKind::Ai` is required in every
  scenario and stays required here.

So the designed cycle is `MissionPlay` (3 min) -> `InstantAction` (2 min) ->
`MenuReturn` (1 min), ten times. `SoakPhase::AiEngagement` was renamed to
`SoakPhase::InstantAction` and the doc comments and error text updated; the
required-systems contract is unchanged. If the owner actually intended an AI
engagement phase, the owner should say so in the unblock note and this rename
is reverted.

## 3. Resident-memory source

The task defers the choice between process RSS and allocator-reported memory
for `SoakSample::resident_bytes` to F60-B instrumentation. F60-B must pick one,
record why, and use it consistently for every sample, because the least-squares
trend and the 16 MiB/h designed bound are only comparable across samples that
come from one counter.

## Tests

`accept_f60_a_soak_cycle_is_mission_instant_action_menu` pins the corrected
phase order and lengths and fails against the old reading; the rest of
`crates/cs_app/tests/accept_f60_a_benchmark_contracts.rs` is unchanged in
substance. No test was weakened.

## Sources

`specs/F60-performance-memory-stability-and-platforms.md`;
`specs/F49-instant-action-presets-and-custom-scenarios.md`;
`docs/00-SCOPE.md`; `docs/research/FINDINGS.md`;
`docs/findings/perf/2026-10-01-f60-a-benchmark-scenarios-and-budgets.md`;
`crates/cs_app/src/diagnostics/{scenario,soak}.rs`.
