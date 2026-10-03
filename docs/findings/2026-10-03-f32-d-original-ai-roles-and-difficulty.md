# F32-D: the original's AI skill tiers and its three-step difficulty option, measured — and a probe that replays a mission at every one of them

Date: 2026-10-03. Task: #132, "Verify original AI roles and difficulty-sensitive
mission behavior" (`specs/F32-ai-combat-formations-aces-and-difficulty.md`,
section `### F32-D`, acceptance test **AC04**). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`; evidence contract:
`docs/contracts/CLI-EVIDENCE.md`.

Capabilities used: `retail` (read-only access to `$CS_GAME_DIR`) and ordinary
build/test. **Not** used and not claimed: any run of the original executable,
any GPU, any audio, any human play or review. Nothing here is evidence of how
the original *behaves*; it is evidence of what its shipped files *declare*.

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/ai.rs` (owner path): the **measured** half —
  `ORIGINAL_DIFFICULTY_OPTION_MACRO`/`_FIRST_ID`/`_BOUND_MACRO`/`_BOUND_ID`,
  the derived `ORIGINAL_DIFFICULTY_STEPS`, `ORIGINAL_DIFFICULTY_OPTION_IDS`,
  `ORIGINAL_GAME_OPTION_DIFFICULTY_TITLE`/`_DESC`,
  `ORIGINAL_IA_DIFFICULTY_LABEL`, `ORIGINAL_DIFFICULTY_RECORDED_PER_SCENARIO`,
  `DeclaredSkillTier`, `ORIGINAL_SKILL_LABEL_COUNT`,
  `ORIGINAL_ENEMY_GROUP_COUNT`, `ORIGINAL_SCENARIO_DESCRIPTOR_COUNT`,
  `ORIGINAL_ACE_STAT_SLOTS`, `ORIGINAL_ACE_STAT_MAX`,
  `DeclaredDifficultyOrigin`, and the production reader
  `original_ai_surface` with `OriginalAiSurface` / `ScenarioSurface` /
  `SkillLabelRow` / `SurfaceError`; `DifficultyTier::index` /
  `::measured_step` / `::is_designed_extension` / `::measured_tier_count`;
  and the `accept_f32_d_*` tests plus the evidence harness.
- `crates/cs_sim/src/ai/combat.rs` (owner path): the **runtime** half —
  `ORIGINAL_DIFFICULTY_STEPS` and the same four `DifficultyTier` queries;
  `DIFFICULTY_PROBE_DOMAIN`, `MAX_PROBE_RUNS_PER_TIER`, `MAX_PROBE_TICKS`,
  `PROBE_LATERAL_JITTER_M`, `PROBE_FORMATION`, `DifficultyProbeSpec`,
  `DifficultyProbeRun`, `TierProbeOutcome`, `ProbeDifference`,
  `TierComparison`, `DifficultyProbeReport` with its invariance and coverage
  queries, `CombatRuntime::probe_difficulties`, the authored
  `probe_geometry` scenario, the FNV-1a geometry/arsenal/profile digests, the
  population variance, the adjacent comparison, the four new `CombatError`
  variants, `CombatPlanner::profiles` (needed to rebuild a run's runtime), and
  `synthetic_difficulty_probe_spec`.
- `crates/cs_sim/tests/ai/accept_f32_d_combat.rs` and
  `crates/cs_sim/tests/ai/accept_f32_d_combat_failures.rs` (new, owner path):
  the 22 `accept_f32_d_*` tests.
- `crates/cs_sim/tests/ai/main.rs` (owner path, wiring only): the two new
  module declarations and the target's doc comment.
- `crates/cs_content/Cargo.toml` and the root `Cargo.lock` (wiring only): one
  **test-only** `cs_sim` dev-dependency, so the anti-drift test below can
  compare the two crates' copies of the measured count on the side that owns
  the measurement.
- This file and `docs/findings/evidence/F32-D.json`.

No protected path, no `cs_app` change, no original data, no binary file.

**One observable failure:** AC04 — "run repeated mission combat probes at
every discovered difficulty and compare outcomes statistically". Before this
change nothing in the workspace replayed a mission: `CombatRuntime::step` was
one actor's decision on one tick, and the difficulty tier was a *key* nothing
exercised more than once. A roster that resolved the same profile for all four
tiers — or a tier that moved the world, the clock or the AI's weapons instead
of its behavior — was indistinguishable from a correct one, because there was
no repeated measurement anywhere to tell them apart. That is the failure this
stage repairs.

## What was measured, and where each number comes from

Everything below was read out of the owner's installation through production
readers and is **re-measured on every run** of the retail tests, so a stale
committed constant fails rather than passing.

Installation fingerprint, from the production discovery every span is bound to:

| Fingerprint | Value |
| --- | --- |
| `install_sha256` | `b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978` |
| `content_sha256` | `a0223506e512b50c0e0445ba73204a0461e60197e28d58a7f7144632d262c12d` |

### 1. The campaign difficulty option is a **three**-step selector

Two independent witnesses, because a header gap alone is not a count.

**The bound (an upper bound).** `ASSETS/SCRIPTS/RESOURCE.H`, the engine's own
resource header inside `GOSDATA/ASSETS/crimson.rof`, declares
`IDS_DIFFICULTY 109`; the first macro it declares *after* that is
`IDS_VIEWCOCKPIT 112`. The header declares **612** ids in all, and neither
`110` nor `111` is one of them, so the option's name list can hold **at most
three** ids: `109..=111`.

**The occupancy (a lower bound).** The shipped UI string image
`GOSDATA/ASSETS/BINARIES/langui.dll` (language 1033) carries **exactly one
non-empty string at each of 109, 110 and 111**, so the option offers **at
least three** steps. Bound met by occupancy ⇒ **three**.

**Why a gap is not a count.** The default-view row is the case that shows it.
`IDS_VIEWCOCKPIT 112` and `IDS_VIEWCHASE 113` are *both* declared and *both*
are view entries, so the gap from 112 to 113 is **one** while the list really
holds **two** — the string image populates both `112` and `113`, and the next
list (`IDS_LIGHTINGLEVELS 114`) starts after them. Reading a header gap as an
option count is the mistake F27-D repaired in the ammunition blocks; here it
would have produced the right answer **by luck**, so the count is pinned from
both sides instead. The same bound re-measured on the two blocks F27-D/F28-D
already measured with it — the rocket name blocks fifteen ids apart
(`IDS_ROCKETLONGNAME 3380` → `IDS_ROCKETSHORTNAME 3395`) and the airframe name
blocks twenty apart (`IDS_AIRFRAMELONGNAME 3000` →
`IDS_AIRFRAMESHORTNAME 3020`) — so this constant uses the bound those
constants were read with.

**The step names are deliberately not here.** They live in `langui.dll`'s
localizable string table. Committing them would be original display content
(AGENTS rule 3), so the measurement is the ids, the count and the occupancy and
nothing else.

### 2. The difficulty option is **one row of one screen**, and nothing records it per mission

| what | measurement |
| --- | --- |
| `IDS_GO_DIFFICULTY_TITLE` | id **1084**, and `IDS_GO_VIEW_TITLE` is 1085 — immediately after, so the title is a **single** id |
| `IDS_GO_DIFFICULTY_DESC` | id **1087**, and `IDS_GO_VIEW_DESC` is 1088 — immediately after, so the description is a **single** id |
| `IDS_IA_DIFFICULTY` | id **3695**, and `IDS_IA_PLANES` is 3700 — a single id the instant-action story screens show as a *label* |

So the difficulty is a **selector with a title and a help line** on the
game-options screen, beside "default view" and "auto head turn". And the
complete root-key vocabulary of **all eight** instant-action scenario
descriptors contains **no** difficulty key: `ORIGINAL_DIFFICULTY_RECORDED_PER_SCENARIO`
is `false`, measured over every descriptor, not assumed. **Which step is in
force is the player's choice at the options screen, and nothing in the measured
data binds a step to a mission.**

That negative is the load-bearing result of this stage, and it is why
`cs_content::ai::DeclaredDifficultyOrigin` has two variants instead of one: a
declared tier may be a `SelectedOptionStep { step }` — bounded by the
measurement — or a `DesignedExtension`, and it may not be presented as the
original's per-mission difficulty, because no such record exists to lower.

### 3. The per-aircraft AI skill vocabulary is exactly **three** labels

Over **62** reader archives, exactly **8** carry an `IA1` scenario descriptor,
each declaring four enemy groups (`group1` … `group4`) and a named ace:

| label | declarations |
| --- | ---: |
| `novice` | 8 |
| `veteran` | 13 |
| `ace` | 19 |
| **total** | **40** = 32 group labels + 8 ace labels |

`cs_content::stunts` already spelled `enemy_skill`/`ace_skill` as three `&str`
constants; what was missing was the **closed vocabulary and the refusal**, so
`DeclaredSkillTier` adds it and `from_label` returns `None` for anything the
installation never spells (`"ACE"`, `"Ace"`, `"ace "`, `"elite"`,
`"recruit"`, `"hard"`, `""`). A test asserts the two sets are equal, so the
vocabulary and the `.zrd` reader cannot drift.

The measured mission types are exactly three: `dogfight_squadron` (2
descriptors), `stunt_flying` (4), `zeppelin_run` (2).

**This is a different thing from the difficulty option.** Both happen to be
three, and a reader will be tempted to map one onto the other. Nothing measured
says the game does, so nothing here does either: `enemy_skill` is a
*per-aircraft* tier authored by the scenario, the difficulty option is a
*global user preference*, and the two live in different files.

### 4. The declared ace is a **nine**-slot integer vector, saturated

`ace_stats` is a nine-element integer list in **all eight** descriptors, and
every one of the 72 slots is **9**. That is the whole measurement: a slot
count and an extent. **No measured file names what the nine slots are, their
order, or what a value below the maximum does**, and no measured key names a
damage or a health slot — both assertions are in the acceptance suite, because
"the ace is not inflated health" (F32's deliverable) is a statement about this
engine's *type*, and this measurement says the original's record is a
nine-number vector whose meaning is unknown. `ORIGINAL_ACE_STAT_SLOTS` and
`ORIGINAL_ACE_STAT_MAX` are therefore documented as an extent and never as a
meaning.

### 5. The project's declared vocabulary is **one step longer** than the original's

`DifficultyTier` has four steps (`Relaxed`, `Standard`, `Hard`, `Elite`) and
was authored when the count was unknown — F32-A and F32-C both recorded that
the count was F32-D's to measure. It is now measured at **three**.

The four-step vocabulary is **kept**, because non-negotiable 1 explicitly
allows "explicitly designed alternatives", and because removing a variant would
break `crates/cs_app/tests/accept_f49_a_preset_spawn.rs`, which is not this
task's owner path. What changed is that the extra step is now **declared**:
`DifficultyTier::measured_step()` maps the first three tiers positionally onto
the measured steps `0..3` and returns `None` for the fourth, and
`::is_designed_extension()` names it. `cs_content::ai::DifficultyTier` carries
the identical pair, and both crates' doc comments now say the original's option
has three steps instead of saying the count is unknown.

**The mapping is positional and says nothing about the names.** `ALL` runs most
forgiving to most demanding and the measured option runs the same way, so the
first three correspond in order — but the original's step *names* are not known
to this project, so no label in either crate claims anything about the
original's wording.

### 6. The anti-drift check between the two crates

`cs_sim` may not depend on `cs_content` (AGENTS rule 7), so `cs_sim` holds its
own `ORIGINAL_DIFFICULTY_STEPS`. Nothing else compared the two. `cs_content`
now carries a **test-only** `cs_sim` dev-dependency (mirroring the existing
`cs_net` one) and
`accept_f32_d_retail_the_runtime_tier_mirror_agrees_with_the_measured_option`
re-derives the count from the installation through `original_ai_surface` and
then reads `cs_sim`'s mirror and its tier mapping — the two cannot drift, and
the check happens on the side that owns the measurement.

## The difficulty probe (AC04)

`CombatRuntime::probe_difficulties` replays one mission-combat scenario
`runs_per_tier` times at **every** `DifficultyTier::ALL`, driving
`update_formation` and `step` on a fresh per-run clone of the runtime.

The scenario is a **three-actor escort engagement in a three-slot formation**:
the deciding escort leads the formation, a hostile attacks its charge on a
120-tick schedule, a nearer hostile attacks nobody, and the probe loses a
follower at tick 250 and the formation's assigned target at tick 400 so a real
recovery is applied rather than a steady state measured. Every number in it is
authored project design (see the limitation below).

**What is held constant, and why that is the measurement.** Run `n` replays the
same world at every tier: the positions, the threat stamps and the tick count
are a function of `(root_seed, run)` alone, under the documented
`SplitMix64::for_domain(root_seed ^ DOMAIN)` recipe with
`DIFFICULTY_PROBE_DOMAIN = 0x4633_3250_524F_4245`. A per-index 64-bit FNV-1a
digest over every coordinate and threat stamp is compared **across** tiers, and
one weapons snapshot digest is compared across tiers, so:

* a tier that moved a position, a threat stamp or the clock would change the
  geometry digest — **non-negotiable 1** checked mechanically, not by review;
* a tier that handed the AI a different gun or rack would change the arsenal
  digest — **non-negotiable 2**, which no verified original exception covers;
* and because the world is identical, any difference in outcome is attributable
  to the profile `resolve_profile` selected and to nothing else.

**The measured outcome** (24 runs × 600 ticks per tier, root seed `20260903`):

| tier | measured step | answered ticks / run | deferred threats / run | engagements | range refusals | recoveries | profile digest |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| relaxed | 0 | 196.83 | 184.00 | 14 400 | 3 060 | 24 | `6445903300892541448` |
| standard | 1 | 303.00 | 97.00 | 14 400 | 3 060 | 24 | `2014782459771529981` |
| hard | 2 | 351.00 | 49.00 | 14 400 | 3 060 | 24 | `202099323039344105` |
| elite | — | 375.00 | 25.00 | 14 400 | 3 060 | 24 | `7305471848971676703` |

Every adjacent pair: `protected_answers` **Higher**, `deferred_threats`
**Lower**, `engagements` **Same**. The engagements, the range refusals and the
applied recoveries are the **controls**: identical at every tier, which is the
statement that the tier changed the AI's *behavior* and nothing else.

**It is a distribution, not four traces.** The relaxed tier's per-run answer
count has a population variance of **568.81** (the seeded lateral jitter moves
the attacker's geometry enough to change *when* the slow reaction gate opens),
while the elite tier's is exactly **0** — it notices every attack the schedule
produces. Two different reasons for the same shape of number, and the suite
asserts both.

**The negative.** A roster that resolves one profile for all four tiers
measures `distinct_profile_count() == 1`, `outcomes_differ() == false` and every
adjacent comparison `Same`, while the three invariance queries still hold. That
is the outcome the probe exists to catch: "difficulty" as a comment.

## Semantics defined at this stage

- **A measured count is a bound met by an occupancy, never a block width read
  as a count.** The header bounds a name list; the shipped image says how much
  of it is usable. Both are measured and asserted to be equal.
- **Difficulty is a selection, not a per-mission record.** Measured: no
  descriptor records one. So `DeclaredDifficultyOrigin` distinguishes a
  `SelectedOptionStep` from a `DesignedExtension`, and no declared profile may
  be presented as the original's per-mission difficulty.
- **The declared vocabulary may be longer than the original's, and must say
  so.** `measured_step()` / `is_designed_extension()` make the difference a
  query rather than a claim.
- **A per-aircraft skill tier is not a difficulty step.** Both are three; the
  measurement keeps them apart because nothing measured maps one onto the
  other.
- **A measured extent is not a measured meaning.** `ace_stats` is nine slots
  saturated at nine; what a slot *does* stays unknown.
- **A repeated probe measures behavior, and proves the world did not move.**
  The per-index geometry digest, the arsenal digest and the clock check are
  what make the per-tier comparison attributable; without them the comparison
  would be an anecdote.

## Unknowns recorded (not guessed)

| claim id | unknown | resolving task |
| --- | --- | --- |
| `f32.d.limit.difficulty_effects` | The option has **three** steps; **nothing in any shipped file says what a step changes.** `SkillKnob` remains a designed vocabulary and the runtime's tier profiles remain designed alternatives under non-negotiable 1. | #566 `F32-DIFFICULTY-EFFECT` |
| `f32.d.limit.skill_tier_effects` | `novice`/`veteran`/`ace` are measured labels; what a tier *does* is unmeasured, so `DeclaredSkillTier` is a closed vocabulary with a refusal and **no production consumer yet**. | #567 `F32-SKILL-EFFECT` |
| `f32.d.limit.ace_stats` | Nine slots, maximum nine, **no meaning**, no order, no below-maximum behaviour, no damage or health slot. A future named nine-field struct would be guessing the original's nine meanings. | #568 `F32-ACE-STATS` |
| `f32.d.limit.ai_roles` | The original's AI **role** set is in no measured file. `DeclaredCombatRole`'s seven roles and `cs_sim`'s mirror stay designed vocabulary; only the skill labels and the squadron shape are measured. | #569 `F32-ROLES` |
| `f32.d.limit.difficulty_naming` | The three step names are in the shipped localizable image and are deliberately not reproduced, so `relaxed`/`standard`/`hard`/`elite` claim nothing about the original's wording and the tier-to-step mapping is positional only. | #551 `F32-LOWERING` |
| `f32.d.limit.per_mission_difficulty` | Measured: no record binds a step to a scenario or mission, so this engine has no way to read one from content. | #551 `F32-LOWERING` |
| `f32.d.limit.probe_geometry` | Every number in the probe's scenario is authored project design; no measured file describes an original AI encounter. The probe measures **this** engine's per-tier decision behavior and carries no machine-readable marker saying so. | #570 `F32-PROBE-GEOMETRY` |
| `f32.d.limit.fire_discipline` | `fire_discipline_ticks` and `aim_error_rad` are reported and not enforced across ticks, so **two tiers differing only in those two knobs measure as identical** and the probe's firing-tick count is a control that cannot discriminate. | #571 `F32-FIRE-DISCIPLINE` |

Every one of these is also in the machine-readable record
(`review.method` in `docs/findings/evidence/F32-D.json`), so no limitation is
removed from evidence to turn a validator green.

## Sensitivity probes (run and reverted; none committed)

Each probe was applied to the committed `crates/cs_sim/src/ai/combat.rs` or
`crates/cs_content/src/ai.rs`, the `accept_f32_d_` selection was re-run, and the
file was restored from a copy afterwards. The committed tree is the green one
(23 `accept_f32_d_*` tests: 10 in `cs_content`'s `f32_d` module and 13 in the
`ai` test target).

`crates/cs_sim/src/ai/combat.rs`:

| probe | result |
| --- | --- |
| `resolve_profile` resolves the *first declared tier's* profile for every tier, so difficulty selects nothing | **3 failed**: `…the_comparison_is_statistical_and_the_slowest_tier_spreads`, `…every_discovered_difficulty_is_probed_repeatedly_and_compared`, `…a_roster_whose_tiers_collapse_measures_nothing_to_compare` |
| `probe_once` shifts its candidates by `tier.index()`, so each tier sees a different world | **3 failed**: `…a_tier_changes_the_behaviour_and_never_the_world_or_the_clock`, `…the_probe_jitter_cannot_move_a_candidate_across_a_gate`, `…a_roster_whose_tiers_collapse_measures_nothing_to_compare` |
| the arsenal snapshot is rebuilt from a tier-varying source | **2 failed**: `…a_tier_never_hands_the_ai_different_weapons`, `…a_roster_whose_tiers_collapse_measures_nothing_to_compare` |
| the run length is divided by `tier.index() + 1` (the simulation-rate fake) | **5 failed**: `…a_tier_changes_the_behaviour_and_never_the_world_or_the_clock`, `…every_discovered_difficulty_is_probed_repeatedly_and_compared`, `…the_probe_jitter_cannot_move_a_candidate_across_a_gate`, `…the_probe_replays_a_mission_formation_loss_at_every_tier`, `…a_roster_whose_tiers_collapse_measures_nothing_to_compare` |
| `aggregate` reports a mean but a hard-coded zero variance | **1 failed**: `…the_comparison_is_statistical_and_the_slowest_tier_spreads` |
| `difference` returns `Same` for every adjacent pair | **1 failed**: `…every_discovered_difficulty_is_probed_repeatedly_and_compared` |
| `covers_every_measured_step` returns `true` unconditionally | **1 failed**: `…without_a_measurement_the_probe_still_refuses_to_claim_coverage_it_did_not_run` |
| `answers_protected_threat` reduced to `step.target().is_some()` | **3 failed**: `…an_answer_is_read_from_the_trace_and_not_from_the_scenarios_attacker`, `…every_discovered_difficulty_is_probed_repeatedly_and_compared`, `…the_comparison_is_statistical_and_the_slowest_tier_spreads` |
| the probe loop counts `protected_answers` from the scenario's attacker identity instead of the trace | **1 failed**: `…the_comparison_is_statistical_and_the_slowest_tier_spreads` |
| `DifficultyProbeSpec::try_new` clamps a zero-run probe to one run instead of refusing it | **1 failed**: `…a_probe_spec_that_would_measure_nothing_is_refused_by_name` |

`crates/cs_content/src/ai.rs`:

| probe | result |
| --- | --- |
| `ORIGINAL_DIFFICULTY_STEPS` derived one short of the measurement (2 instead of 3) | **3 failed**: `…retail_the_original_campaign_difficulty_option_is_a_three_step_selector`, `…retail_the_runtime_tier_mirror_agrees_with_the_measured_option`, `…the_declared_fixture_stands_for_no_measured_step` |
| `DeclaredSkillTier::from_label` trims, lowercases and then falls back to the first tier instead of refusing | **1 failed**: `…retail_the_original_declares_exactly_three_ai_skill_tiers` |
| `ORIGINAL_DIFFICULTY_RECORDED_PER_SCENARIO` flipped to `true` | **1 failed**: `…retail_no_scenario_records_a_difficulty_so_only_a_selection_is_measurable` |
| `ORIGINAL_ACE_STAT_SLOTS` set to 8 | **1 failed**: `…retail_every_scenario_declares_a_nine_slot_ace_stat_vector` |

### Two probes that were **not** caught, and what was done about it

Both were found by running them, and neither is papered over.

1. **`answers_protected_threat` counted from the scenario passed** on the first
   attempt. Inside the probe's authored geometry the two readings genuinely
   coincide — the attacker only out-scores the harmless hostile once its attack
   is noticed — so nothing in the probe could tell them apart. Fixed by making
   the counting rule a production predicate (`answers_protected_threat`) and by
   adding `…an_answer_is_read_from_the_trace_and_not_from_the_scenarios_attacker`,
   which builds an escort decision where the attacker is **also** the script
   objective and the nearer hostile, so it is selected with or without the
   protected-actor term; the test shows the term contributing `2.0` with the
   charge alive and `0.0` with it reported destroyed, while the selected target
   is the attacker in both. Both reductions (to `target().is_some()` and to the
   scenario's attacker identity) now fail.
2. **`original_ai_surface` reporting the bound as the occupancy was not
   detectable through the surface type**, and still is not: on this
   installation the shipped image populates *every* block the header bounds, so
   `difficulty_steps == populated_difficulty_ids` for all of them and no
   assertion over the surface can distinguish "measured" from "restated". The
   reader itself is pinned instead by the one block the header declares and the
   image leaves **empty** — `IDS_KEYUNMAPPED 135` — which
   `…retail_the_difficulty_count_is_a_bound_the_string_image_meets` asserts
   reads **0**, so `populated_difficulty_ids` cannot degenerate into the bound
   without that assertion failing. A reviewer who wants the *field* itself
   pinned should note that this installation cannot pin it, and should read the
   surface's `populated_difficulty_ids` as "whatever the reader measured",
   which is what its doc comment says.

## Checks run

- `cargo fmt --all -- --check` — clean.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — exit 0.
- `cargo test --workspace --locked` — exit 0 (305 suites, 0 failed).
- `cargo test --workspace --locked -- accept_f32_d_ --include-ignored` — exit 0;
  23 tests, all `accept_f32_d_*`, all passing (13 unignored so CI runs them, 10
  `#[ignore = "requires CS_GAME_DIR"]` run locally over `$CS_GAME_DIR`).
- `python3 tools/validate_evidence.py private/evidence/F32-D/acceptance.json
  --artifact-root private/evidence/F32-D --require-pass` — exit 0,
  `{"structurally_valid": true, "artifact_count": 3}`.

The evidence report is committed as `docs/findings/evidence/F32-D.json` and was
produced on the tree named inside it, by the harness the documented command
runs; the reviewer regenerates it on the rebased commit and compares.

## Not claimed

No original-data verification of AI *behavior*: `retail` here is read access to
original files, not evidence that the original executable ran, and the probe's
measured outcomes are this engine's. No ECS/Bevy wiring — `cs_app` is not among
this task's owner paths, and the declared→runtime lowering boundary (#551) is
still open. No claim that the four declared tiers, their labels, the seven
declared roles, the probe's geometry or anything the runtime does match the
original's. The task awards at most **checked** status.

## Notes for the owner and the reviewer

- The `cs_sim` → `cs_content` direction was deliberately inverted: the
  anti-drift test lives in `cs_content` (which owns the measurement) and takes a
  **test-only** dev-dependency on `cs_sim`, rather than the other way round, so
  `cs_sim`'s test suite does not acquire a dependency on the whole content
  layer. Neither crate links the other in a build.
- `CombatPlanner::profiles()` was added because `probe_once` rebuilds a runtime
  for its own session generation and must not carry a second copy of the role
  roster. Returning the planner's own map is what keeps the two from drifting.
- The probe registers **its own** formation (`PROBE_FORMATION = 90`) with the
  declared `RecoveryPolicySet` the spec carries, rather than reusing
  `FormationId(1)`: a probe that reused the fixture's formation would be
  measuring the fixture's membership, and a spec is where declared data belongs.
- `DifficultyProbeReport::geometry_is_tier_invariant` is `false` for an empty
  report on purpose: an empty report has measured nothing, and the suite pairs
  it with `covers_every_measured_step` (also `false`) so a caller cannot
  present a vacuous invariant as evidence.
- A reviewer should treat `f32.d.limit.fire_discipline` as the load-bearing
  open limitation: the runtime *reports* `fire_discipline_ticks` and
  `aim_error_rad` without enforcing them, so two tiers that differ only in those
  two knobs would measure as identical here. The probe's `firing_ticks` column
  is a control precisely because it cannot discriminate today; when the
  per-actor fire-discipline state gets a session owner, that column becomes a
  discriminator and this finding should be revisited.
- Reviewer identity for this session: implemented **and** self-checked by
  `bunny-2`, which is **not** independent review. The F32 sheet asks for a fresh
  reader on format and mission semantics, and the probe's statistics in
  particular deserve a second pair of eyes: a different agent instance should
  re-check that the aggregate, the variance and the per-index invariance really
  measure what the tables above claim.

## Sources used

- `specs/F32-ai-combat-formations-aces-and-difficulty.md` (stage `### F32-D`,
  AC04, non-negotiables 1–5) and `docs/contracts/IDENTITY-CONTENT.md`,
  `docs/contracts/CLI-EVIDENCE.md`, `docs/contracts/FLIGHT-PHYSICS.md`.
- `docs/findings/2026-10-01-f32-a-combat-roles-skill-knobs-and-decision-traces.md`,
  `docs/findings/2026-10-03-f32-b-maneuvers-priority-and-firing-solutions.md`
  and `docs/findings/2026-10-03-f32-c-formation-ace-and-difficulty-runtime.md`
  — the three stages this one completes, including their recorded unknowns.
- `docs/findings/2026-10-02-t465-ai-stunt-earning.md` and
  `docs/findings/2026-10-02-t463-stunt-encoding-and-gate-geometry.md` — the
  `.zrd` grammar and the first census of the scenario roster this stage
  extended.
- `docs/findings/2026-10-03-f27-d-original-ammunition-and-loadout-audit.md` and
  `docs/findings/2026-10-03-f28-d-original-ordnance-catalogue.md` — the
  "a block width is not a count" precedent this stage's bound/occupancy rule
  follows, and the resource-header measurements it corroborates.
- `docs/findings/2026-09-29-f12-d-langui-dll-routing.md` and
  `docs/findings/2026-09-29-f12-g-strings-dll-resources-and-header-id-correlation.md`
  — how a shipped PE string image is read as inert data through
  `cs_content::config::StringCatalog`.
- The owner's installation, read-only, over `$CS_GAME_DIR`: the resource header
  member of `GOSDATA/ASSETS/crimson.rof`, the `GOSDATA/ASSETS/BINARIES/langui.dll`
  image, and all **62** reader archives with their eight `ia.zrd` descriptors.
  Installation fingerprint above.
