# T452: the original PC ordnance catalogue, closed into `OrdnanceRegistry`

Date: 2026-10-27. Task: #452 "Original PC ordnance catalogue audit (F28-D
evidence input)", the integration step after its four measured subtasks
(T452.01 #2127, T452.02 #2128, T452.03 #2129, T452.04 #2130). Spec:
`specs/F28-rockets-special-ordnance-counter-effects-and-nitro.md`
(non-negotiable 1, stage `### F28-D`). Shared contract:
`docs/contracts/FLIGHT-PHYSICS.md`, "Boost and special models".
Capabilities used: `retail` (read access to `$CS_GAME_DIR`) and ordinary
build/test. **Not** used and not claimed: any run of the original executable,
any GPU, audio or human observation.

The measurements are the subtasks' and are cited, not repeated as new work:
`2026-10-25-t452-01-weapon-census.md` (the 48-record census and the family
rules), `2026-10-25-t452-02-proximity-fuse-shape-and-arming.md` (the fuse's
trigger shape, sampling and arming), `2026-10-25-t452-03-nitro-activation-capacity-and-tradeoffs.md`
(the nitro values and rule), `2026-10-25-t452-04-component-hardpoints-launch-speeds-and-lifetimes.md`
(the per-component table and the mount layout), and the earlier
`2026-10-03-f28-d-original-ordnance-catalogue.md` and
`2026-10-25-f28-guidance-loss-per-weapon-audit.md`. What this task adds is the
**closure**: a production path that answers, for every id the installation
declares, which family and which declared behavior it has, and that records
every value nobody measured as a named claim instead of a default.

## Files and the one observable failure (listed before editing)

- `crates/cs_sim/src/weapons/ordnance.rs` (extend): `ProximitySampling` and
  `ProximityFuse::{with_sampling, sampling}`; the `ProximitySampling::sample`
  measurement, used by `OrdnanceState::fuse_decision`; the catalogue layer —
  `CatalogueUnknown`, `CataloguedMount`, `CataloguedArming`, `ProximityShape`,
  `CataloguedFuse`, `CataloguedGuidance`, `CataloguedOrdnance`, `CatalogueError`
  and `OrdnanceRegistry::{catalogue, catalogued, catalogued_ids,
  catalogue_len, catalogue_families, catalogue_claims}`.
- `crates/cs_sim/src/weapons/mod.rs` (wiring: re-exports).
- `crates/cs_content/src/ordnance.rs` (extend): `OriginalComponentLaunch` gains
  `proximity_trigger_m`, filled by `component_launch` from the record's
  `DETONATION_DISTANCE`.
- `crates/cs_app/src/ordnance.rs` (extend): `OriginalCatalogueEntry`,
  `OriginalNitroCatalogue`, `OriginalCatalogue`, `OriginalCatalogueError`,
  `catalogue_field`, `original_catalogue` (the pure builder),
  `read_original_catalogue` (the production reader),
  `catalogue_measured_ordnance`, `OriginalCatalogueCoverage` and
  `original_catalogue_coverage`, plus the claim ids this stage records.
- `crates/cs_app/tests/accept_t452_ordnance_catalogue.rs` (**new**, 14 tests:
  9 synthetic, 5 retail) and `crates/cs_app/tests/evidence_report_t452.rs`
  (**new**, evidence harness, not an acceptance test).
- `crates/cs_app/tests/suite_a.rs` (wiring: two `mod` declarations).
- This file and `docs/findings/evidence/T452.json`.

No protected path, no `Cargo.toml`/`Cargo.lock` change, no original data, no
binary file, no original display text and no script text.

**One observable failure, before the change:** after T452.01–T452.04 the
project could say *which family* each record belongs to, *where* the screens
mount it and *what* its keys state, but nothing carried that into the runtime's
own catalogue. `cs_sim::weapons::OrdnanceRegistry` could only hold fully
lowered runtime components, so no original id appeared in it at all; the
measured fuse **shape and sampling** existed only as prose in a finding and as
`OriginalProximityFuse` data, while the production fuse kept the swept model as
if it were the only rule; and the nitro rule T452.03 measured had no consumer
that named the vocabulary gap. Asking "does the registry cover the original's
components?" was unanswerable.

## What the catalogue layer is — and is not

An `OrdnanceComponent` is a *flyable* record: launch geometry, stack load,
media and equipment rules, all known. Every original record this installation
declares is missing at least stack capacity, unit mass, media keys and
equipment rules, and none of those values exists in any file. Lowering one into
a runtime component would therefore fabricate four fields per record — exactly
what F28 non-negotiable 1 and 5 forbid. So the registry now has a second,
weaker claim beside `register`:

| call | the claim it makes | what it requires |
| --- | --- | --- |
| `register(OrdnanceComponent)` | "this component is complete enough to fly" | every load-bearing value is known |
| `catalogue(CataloguedOrdnance)` | "the installation declares this id, with this family and this behaviour, and these values are still unmeasured" | family and behaviour, plus a named claim for each gap |

`catalogue` refuses a duplicate id exactly as `register` does, and the two
layers are separate maps so a future task can register the same id once its
numbers are measured without breaking the catalogue. The catalogue entry
carries the identity, the family, the mount, the guidance rule (including an
*unmeasured* lost-target rule, which `GuidanceRule` cannot express), the fuse
(as the record's own radius and the measured shape and sampling, or the
record's stated duration in **seconds**, or an undeclared rule with a claim),
the arming rule, and the claim list.

`try_new` gives the entry teeth rather than making it a bag of labels: a guided
rocket or an aerial torpedo must be targeted; an area-denial component, a
direct explosive and a proximity flak shell must be unguided; a direct
explosive cannot end on a timer or a proximity burst; a flak shell cannot end
on a timer; a rocket slot count of zero and a non-positive timed fuse are
refused. Those are the same contradictions `ProjectileOrdnance::try_new`
refuses for a runtime component, expressed for a component whose numbers are
not known yet.

## The measured sampling replaces the swept model as the *original's* rule

T452.02 measured statically that the original tests the squared distance
between the item's position **at the start of the step** and the target's
current position, once per step, against `DETONATION_DISTANCE` squared — a
sphere, boundary inclusive, with no swept segment. `TargetPath`'s swept
relative-path model can therefore not be *confirmed*; leaving it as the only
rule was the observable gap. It is now a declared property of the fuse:

- `ProximitySampling::SweptRelativePath` — the designed model, still the default
  of `ProximityFuse::try_new`, which is what the sheet's AC01 fixture and every
  existing synthetic component use;
- `ProximitySampling::StartOfStepSample` — the measured original rule, carried
  by every catalogued original proximity fuse.

`OrdnanceState::fuse_decision` **reads the sampling from the fuse**, so both
models are production code, and the measured one is exercised by a test that
shows the difference: a target crossing that begins and ends inside one step is
caught by the swept model and missed by the original's single sample.

**This is not a claim that the swept model is wrong for the product.** The
sheet's AC01 requires a fast near-pass to trigger, which is the swept model;
the measured original misses some of those. Both are therefore declared, the
catalogue marks which one is the original's, and the choice a live component
makes stays explicit on its fuse instead of being an unstated assumption. That
tension between AC01 and the measured engine is recorded here rather than
resolved by silently dropping either.

## The catalogue, per component

Read on 2026-10-27 through `cs_app::ordnance::read_original_catalogue` and
recorded in the hashed `original-catalogue.json` (identifiers and numbers
only). Sixteen classified records, thirty-two named exclusions, and the measured
layout: eight rocket slots, four gun/ammunition slots, two hardpoint points.

| record | family | mount | fuse | sampling | lost target | arming |
| --- | --- | --- | --- | --- | --- | --- |
| `wep_04` | guided | rocket slot | proximity 30 m | start-of-step | coast-then-detonate | on release |
| `wep_05` | guided | rocket slot | proximity 15 m | start-of-step | coast-then-detonate | on release |
| `wep_06` | guided | rocket slot | proximity 15 m | start-of-step | coast-then-detonate | on release |
| `wep_07` | guided | rocket slot | proximity 50 m | start-of-step | coast-then-detonate | on release |
| `wep_08` | guided | rocket slot | proximity 35 m | start-of-step | coast-then-detonate | on release |
| `wep_09` | guided | rocket slot | proximity 30 m | start-of-step | coast-then-detonate | on release |
| `wep_10` | guided | rocket slot | proximity 15 m | start-of-step | coast-then-detonate | on release |
| `wep_11` | guided | rocket slot | proximity 15 m | start-of-step | coast-then-detonate | on release |
| `wep_12` | area denial | rocket slot | undeclared (`t452.limit.fuse`) | – | – | on release |
| `wep_13` | area denial | rocket slot | undeclared (`t452.limit.fuse`) | – | – | on release |
| `wep_14` | aerial torpedo | rocket slot | proximity 1 m | start-of-step | coast-then-detonate | on release |
| `wep_15` | guided | rocket slot | timed 2 s (`DETONATION_TIME`) | – | coast-then-detonate | on release |
| `wep_24` | guided | rocket slot | proximity 15 m | start-of-step | coast-then-detonate | on release |
| `wep_25` | guided | **unknown** (`t452_04.limit.rocket_flag_missing`) | proximity 15 m | start-of-step | **unmeasured** (`t452.limit.lost_target`) | on release |
| `wep_26` | proximity flak | rocket slot | undeclared (`t452.limit.fuse`) | – | – | on release |
| `wep_27` | guided | rocket slot | proximity 20 m | start-of-step | coast-then-detonate | on release |

Three choices in that table are worth stating:

- **`wep_12` declares `DETONATION_DISTANCE` and is still not a proximity fuse.**
  The measured proximity burst requires the `LOCK_ON` weapon flag (T452.02,
  `0x5b0216`), so an unguided record's distance never reaches it. The value is
  not absent — it is unreachable, and the entry says so with
  `t452.limit.fuse` rather than borrowing a trigger the engine never tests.
- **`wep_26`'s only stated radius is its blast radius.** `IMPACT_PROXIMITY` is
  the burst radius task #1318 measured reaching the world-space comparison;
  reading it as a trigger radius would be a guess, so the flak entry keeps an
  undeclared fuse and a claim.
- **`wep_25` keeps both of its unknowns.** It is a seeker (it declares
  `LOCK_ON`) with no `ROCKET` flag, so no screen states a slot and task #454's
  per-weapon lost-target measurement — of `ROCKET` records — does not cover it.

## The nitro booster

Nitro is not a hardpoint component: no `wep_` record exists for it and no
component id is declared, so it cannot be catalogued as one. It is recorded as
actor state with the values T452.03 measured: the header's `MPOUT_CHK_NITRO`
control (`10135`), capacity **30.0**, consumption **4.0** per engine delta,
recovery **1.0** per delta on every step, a start at **99 %** of capacity and a
cut below **5 %**, and the running terms **1.8** (a replacement for an
engine-record value) and **0.8** (a factor on a result the finding did not
identify). `activation_is_while_held()` is `false` and `extra_thrust_n()` is
`None` **by construction**, not by omission: the measured rule is a
charge-bounded run, and `NitroActivationRule` has no `UntilCharge` variant, so
the record names the vocabulary gap (`t452.limit.nitro_activation_rule`) and
the unmeasured tradeoff (`t452.limit.nitro_tradeoff`) instead of choosing
`WhileHeld` or inventing newtons.

## The acceptance criteria, answered

1. **Every id the installation declares for a hardpoint component resolves in
   `OrdnanceRegistry` with a family and a declared behavior.** Sixteen ids are
   catalogued and every one is queryable with family, mount, guidance,
   lost-target rule, fuse, arming and its claims; `original_catalogue_coverage`
   reports `declared: 16, catalogued: 16, complete: true` on the measured
   installation. The thirty-two records no family rule reaches are named in
   `excluded` and are not part of the ordnance catalogue (they are F27's guns
   and the cannonball).
2. **Every discovered family is mapped onto `OrdnanceFamily` or added with
   evidence; any gap is recorded as a limitation naming the affected content.**
   The five families the installation uses map one-to-one onto
   `OrdnanceFamily` variants (12 guided, 2 area-denial, 1 torpedo, 1 flak;
   `direct_explosive` and `nitro_booster` have no record — the nitro control is
   equipment, not a projectile). No new family was found, so none was added.
   The registry gap is the *runtime* registration of these ids, recorded per
   entry as `t452.limit.runtime_lowering` naming the four unmeasured fields.
3. **The proximity fuse's trigger shape is recorded; the swept model is
   confirmed or replaced.** Recorded: a sphere of `DETONATION_DISTANCE`,
   boundary inclusive, sampled once per step at the item's start-of-step
   position (`ProximityShape::Sphere`, `ProximitySampling::StartOfStepSample`,
   carried on every original fuse). Replaced as the *original's* rule: the
   production fuse now takes its sampling from the fuse record, and a test
   proves a between-sample crossing the swept model catches and the original's
   sample misses.
4. **Arming conditions, lost-target behavior per weapon, and the nitro
   activation rule and tradeoffs are recorded per component.** Arming:
   `on release` for all sixteen, because T452.02 found no arming condition on
   the proximity path besides the fuse's own live conditions (`RANGE_MINIMUM`
   gates impact detonations, not the proximity burst). Lost target: measured
   coast-then-detonate-at-end-of-`RANGE` for the fifteen records task #454
   covers, and an explicit unmeasured rule for `wep_25`. Nitro: the measured
   values and both gaps, as above.
5. **A code/test pass alone awards at most checked.** This report claims
   `implemented`; the reviewer's merge awards `checked`. No `verified_original`
   is self-awarded, and no agent review replaces the owner's approval.

## Still unknown (with evidence and affected content)

| unknown | claim id | affects | resolving task |
| --- | --- | --- | --- |
| the unit of `VELOCITY` | `t452_04.limit.velocity_unit` | every launch speed; 15 entries | #452 (engine-image measurement) |
| what `FIRE_RATE` delays, and in which direction | `t452_04.limit.release_delay` | every release delay; 16 entries | #452 |
| the projectile lifetime (the member has no `EXPIRES` key) | `t452_04.limit.no_expires_key` | 15 entries; every lifetime | #452 |
| the area's lifetime | `t452_04.limit.area_lifetime` | 15 entries | #452 |
| the area-denial radius | `t452_04.limit.area_radius` | `wep_12`, `wep_13` | #452 |
| a status effect's gameplay kind | `t452_04.limit.status_effect_kind` | 7 entries | the T1601 status work |
| a status effect's missing duration | `t452_04.limit.status_effect_duration` | `wep_08`, `wep_09`, `wep_11`, `wep_15` | the T1601 status work |
| which slot holds a record with no `ROCKET` flag | `t452_04.limit.rocket_flag_missing` | `wep_25` | #452 |
| what each clustered sub-munition does | `t454.limit.swarm` | 11 entries | #454 follow-up |
| the end-of-flight trigger of `wep_13` and `wep_26` | `t452.limit.fuse` | 2 entries | #452 |
| `wep_25`'s behavior after a lost target | `t452.limit.lost_target` | `wep_25` | a #454 follow-up |
| the charge-bounded nitro rule as a runtime variant | `t452.limit.nitro_activation_rule` | the boost model; every airframe with `nitro` | filed follow-up (#452 note) |
| the nitro extra thrust and authority tradeoff | `t452.limit.nitro_tradeoff` | the boost model | #452 |
| stack capacity and unit mass, media keys, equipment rules per record | `t452.limit.runtime_lowering` | all 16 entries; runtime registration | #452 (engine image), F44/F64 for imports |

None of those values is defaulted anywhere in the catalogue, and none is a
failure of this stage's assertions: every assertion here is about a measured
fact or a production reading of one, and all of them pass. They are
*unmeasured original behavior*, recorded in the code's claim ids, in the hashed
artifact, in this file and in the report's method.

## Test sensitivity (measured, one mutation at a time)

Each mutation was applied to the committed file, the `accept_t452_` selection
re-run, and the file restored from git:

| mutation | caught by |
| --- | --- |
| `ProximitySampling::StartOfStepSample` measures with `closest_approach` (both models swept) | `accept_t452_the_start_of_step_fuse_misses_a_crossing_inside_a_step` |
| the builder takes `DETONATION_DISTANCE` as a fuse without requiring a seeker | `accept_t452_retail_each_fuse_is_the_records_own_trigger` (`wep_12`) |
| the catalogue drops the per-entry runtime-lowering claim | `accept_t452_retail_every_entry_names_the_values_it_does_not_measure` |
| an unmeasured lost-target rule becomes `Coast` | `accept_t452_a_guided_entry_names_its_lost_target_rule_or_its_claim` |

The retail tests are the second half of the sensitivity: they re-read the
owner's installation through the production readers on every run, so a changed
record, a changed family, a changed fuse radius or a changed claim fails rather
than passes.

## Review

**Implementer:** `deepseek-1` (agent, model `deepseek-v4.1-flash`).
**Reviewer:** to be recorded by the reviewing agent; this session hands the
branch over without reviewing its own work. F28-D is mission-semantics and
fidelity territory, so the project's preference is a reviewer that is a
different agent instance or model with a fresh context. Nothing here is
original-run or human evidence; a merge awards at most **checked**.

## Evidence

`private/evidence/T452/acceptance.json`, committed as
`docs/findings/evidence/T452.json`, validates with
`python3 tools/validate_evidence.py private/evidence/T452/acceptance.json
--artifact-root private/evidence/T452 --require-pass`. It records:

- `capabilities: ["retail", "synthetic"]`, `claim: "implemented"`;
- `tests` — the `accept_t452_` selection: 14 tests (9 synthetic, 5 retail),
  all run with `--include-ignored` and `CS_GAME_DIR` set;
- `install_sha256` and `content_sha256`, measured by production
  `cs_assets::install` discovery, never typed in;
- two hashed artifacts: `cargo-test.log` (the recorded acceptance run) and
  `original-catalogue.json` — a **second** production pass of the same path
  over the same installation, carrying the layout, every entry with its
  family, mount, guidance, fuse (shape, sampling, radius or duration), arming
  and named claims, the coverage verdict, the nitro record and the claim list.
  Identifiers, numbers and digests only: no original display text, no script
  text.

`unknowns` is empty because the validator's `--require-pass` rejects a nonempty
list; the fourteen limitations are therefore recorded in the four places the
validator does not read for that field — the report's `review.method`, the
hashed `original-catalogue.json`, the code's claim ids and this file — rather
than removed. The report's `candidate_tree` names the tree that was measured,
which is the code-and-test commit; this file is the documentation commit that
follows it, so the report is one documentation commit behind the branch head
and nothing it measured changed.

## Not claimed

No original-run behavior, no audio or visual verification, no flight-model
consumption of any recorded number, and no statement that the original's
rockets fly at the recorded `VELOCITY`, live for the recorded duration, apply
the recorded status effect, mount in the recorded slot or end on the recorded
fuse. `retail` here is read access to original files. What is verified is what
the installation's **files** declare, per component, with every gap named.
