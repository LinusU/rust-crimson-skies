# F32-PROBE-GEOMETRY: the difficulty probe's scenario is sealed as a designed fixture — in code, not in prose

Date: 2026-10-12. Task: #570 `F32-PROBE-GEOMETRY`, resolving
`f32.d.limit.probe_geometry` from
`docs/findings/2026-10-03-f32-d-original-ai-roles-and-difficulty.md`.
Capabilities: ordinary build/test only. No `retail`, no original run — the
task's "either" half (binding the scenario to measured content) was not taken,
because no measured file describes an original AI encounter to bind to; the
"or" half, sealing the fixture with a machine-readable marker, is what this
change does.

## The gap

`cs_sim::ai::combat::probe_difficulties` (F32-D) replays one authored
mission-combat scenario 24 times at each of the four declared tiers and
compares the outcome distributions. Every number in that scenario — positions,
velocities, the 120-tick attack period, the 250-tick follower loss, the
400-tick assigned-target loss, the 120 m jitter bound — is newly authored
project design. The module docs and the recorded limitation said so, but
nothing *stopped* a later stage from quoting the per-tier outcome table as
evidence about the original's difficulty: `DifficultyProbeReport` carried no
marker saying what its outcome is a measurement *of*.

## What changed

`cs_sim::ai::combat`:

* `ProbeProvenance`, a new enum naming where a probe scenario's geometry comes
  from. It has exactly one variant, `DesignedFixture` — deliberately: while no
  measured encounter content exists, no caller can construct a spec whose
  provenance claims more than the code replays. `ProbeProvenance::ALL`,
  `label()` (`"designed_fixture"`) and `is_measured()` (`false`) are pinned by
  the acceptance tests, so a future `Measured` variant arrives only together
  with the geometry it binds — an authored mission's declared spawn points or
  an `ia.zrd` roster's aircraft and groups — and a carrying record.
* `DifficultyProbeSpec::provenance`, a required field: `try_new` takes the
  provenance as an argument, so a spec cannot be assembled unmarked. The field
  is also part of the public struct, so a struct literal declares it too.
* `DifficultyProbeReport::provenance()` carries the spec's declaration into
  every report, and `DifficultyProbeReport::tiers_as_original_evidence()`
  refuses the one read a designed fixture may not have — quoting the outcome
  table *as the original's difficulty* — with the new
  `CombatError::ProbeOutcomeIsDesigned`, matching the refusal conventions of
  `DeclaredSkillTier::knob_overrides` and `AceStatSlots::effect_of`.
* `synthetic_difficulty_probe_spec` declares `DesignedFixture`, and the
  existing `accept_f32_d_*` spec call sites declare it too.

`cs_content::ai` (the side that owns the measurement): the
`f32.d.limit.probe_geometry` entries in `UNRESOLVED` and in the F32-D evidence
harness's `REVIEW_METHOD` now describe the marker; the emitted
`difficulty-probe.json` artifact carries `scenario_provenance`,
`scenario_is_measured` and `original_evidence_readable` (all `false` /
`designed_fixture`), so a committed evidence report cannot quote a designed
outcome without the marker.

## Tests

`accept_f32_pg_*` — five in `crates/cs_sim/tests/ai/accept_f32_pg_combat.rs`
(the marker on the spec, the report and `ProbeProvenance::ALL`; the
construction-site declaration; the refused as-original read and its legible
error; the tier order as a designed-fixture claim; a collapsed roster still
reporting designed provenance) plus
`accept_f32_pg_the_runtime_probe_is_marked_designed_and_refuses_the_original_read`
in `cs_content::ai`'s test module, the cross-crate check. Removing the marker,
defaulting the provenance, or letting the as-original read succeed fails the
test that names it.

## What stays a claim

The measured tier *order* — a more demanding tier answers more authoritative
attacks (`protected_answers` rises at every adjacent pair) — is a property of
the designed fixture's profiles and the designed scenario. It is a **project
claim** about this engine's per-tier decision behavior, and the marker now
keeps it that way: the outcome table can be quoted as what the probe measured,
never as what the original does. Binding the geometry itself to measured
content remains open — it needs an original AI encounter description that no
measured file currently provides.
