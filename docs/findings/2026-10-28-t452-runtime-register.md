# T452-RUNTIME-REGISTER (#2171): the measured original ordnance, registered

Date: 2026-10-28. Task: #2171, the integration step of #2156
(`T452-RUNTIME-REGISTER`) after its five measured subtasks T452-R.01 (#2159),
R.02 (#2160), R.03 (#2161), R.04 (#2169) and R.05 (#2170). Spec:
`specs/F28-rockets-special-ordnance-counter-effects-and-nitro.md` (stage
`### F28-D`). Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`, "Boost and
special models". Capabilities used: `retail` (read access to `$CS_GAME_DIR`) and
ordinary build/test. **Not** used and not claimed: any run of the original
executable, any GPU, audio or human observation.

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/ordnance.rs` (extend): `measured_ordnance_declaration`,
  `register_measured_ordnance`, `read_and_register_measured_ordnance`,
  `OriginalRegistrationBlocker`, `OriginalRegistrationOutcome`,
  `OriginalRegistrationReport`, `OriginalRegistrationError`,
  `original_kept_claim_context`, the measured-field claim ids and the four
  blocker claim ids with their resolving tasks.
- `crates/cs_content/src/ordnance.rs` (extend): `read_original_weapons_provenance`
  — the span and provenance of the `weapons.zrd` member the declared records
  come from, so a registered component is `Origin::Installation` rather than a
  fixture.
- `crates/cs_app/tests/accept_t452_register.rs` (**new**, 7 tests: 5 synthetic,
  2 retail) and `crates/cs_app/tests/evidence_report_t452_register.rs`
  (**new**, evidence harness, not an acceptance test).
- `crates/cs_app/tests/suite_a.rs` (wiring: two `mod` declarations).
- This file and `docs/findings/evidence/T452-RUNTIME-REGISTER.json`.

No protected path, no `Cargo.toml`/`Cargo.lock` change, no original data, no
binary file, no original display text and no script text.

**One observable failure, before the change:** after T452 and its five measured
subtasks, the project could say *what* every original hardpoint component's data
states, *where* its slot is and *what* its measured behaviour is — and still
nothing in it could take one of those records and make it fly. Every id the
installation declares existed only in the registry's *catalogue* map;
`OrdnanceRegistry::get` answered `None` for all sixteen, `OrdnanceSession`
refused to register any of them, and asking "can a measured original component be
launched?" had no answer a test could give. `t452.limit.runtime_lowering` was a
claim with no code behind it.

## What this stage adds: the lowering, and what it refuses

`measured_ordnance_declaration` maps one measured record into a
`cs_content::ordnance::DeclaredOrdnance`; `register_measured_ordnance` runs that
through the existing `lower_ordnance` boundary and inserts the result with
`OrdnanceRegistry::register`. Nothing new was invented in the runtime: the
boundary that refuses an unknown field is the same one F28-A wrote, and this
stage's job is to give it measured values.

Per field, the value and the measurement it came from:

| declared field | value | measurement |
| --- | --- | --- |
| `family` | the record's classified family | T452.01's key rules |
| `launch.launch_speed_mps` | the record's effective `VELOCITY`, m/s | T452-R.01 (`0x5ae0b8`, spawn `0x5af0e7`) |
| `launch.inheritance` | `Full` when `ACCELERATION` is zero or absent | T452-R.01 corrected by T452-R.04 §0 (`0x5af124`) |
| `launch.release_delay_ticks` | `0` | T452-R.04 §1: the airframe row's interval delays the **next** release, never the current one |
| `stack.capacity_units` | the record's `CLUSTER_SIZE`, per slot | T452-R.03-B (`0x4baa17`), cross-checked against the ammunition reader; one launch spends one unit (T452-R.02 §5) |
| `fuse` | the record's `DETONATION_DISTANCE` sphere for a seeker, or a declared `DETONATION_TIME` | T452-R.02 §3 (`0x5b0216` gates the proximity burst on `LOCK_ON`) |
| `guidance` | `Unguided`, or `Targeted { CoastThenDetonate }` | T452-R.02 §4 (`0x5af7f4`), T452-R.05 (the gate tests `LOCK_ON`, not `ROCKET`) |
| `lifetime_ticks` | the record's own flight rule at a stationary launcher, rounded up to the session tick | T452-R.04 §2 (`0x5b01ec`) |
| `area_effect` | `IMPACT_PROXIMITY` with a one-tick lifetime | T1318 (blast applies once, no persistent area), T452-R.02 §2; a field measured to persist (`wep_12`'s `TANGLER`) is blocked, not flattened into a blast |
| `armor_damage` / `internal_damage` | the record's `ARMOR_DAMAGE` / `HEALTH_DAMAGE` | the record's own keys; the record reader |
| `status` | `BEEPER` as `Marker` with its declared `TIME` | T452-R.02 §1 (`0x4b8d50` is the emitter, `0x4ba779` the marker) |
| `media` | the record's `FLYOUT` `MODEL` and `FIRE` `SOUND[0]` | T452-R.03-C (`0x5aecbf`, `0x5af3c0`) |
| `equipment_rules` | the measured empty set | T452-R.03-D |

### The two designed fields

The lowering has to fill two fields whose **vocabulary** is project design, not
original data, and it says so rather than pretending otherwise:

- the **mount key**. A `DamageNodeKey` is a project key. Its value is measured:
  the engine's mount id is the number in the record's name (`wep_05` → mount 5;
  T452-R.04 §1, `0x4b5a79`), so the key is `weapon_mount_05`.
- the **hardpoint kind**. No record and no screen states where on the airframe a
  slot sits — the measured layout is one eight-entry rocket array plus two box
  counts (T452.04) — and `DeclaredHardpointKind` is documented as designed
  vocabulary that F28-D maps onto. Every measured rocket component therefore gets
  the same kind (`Underslung`, the underside rack), inside a designed provenance,
  instead of an invented per-record position.

Both carry `t452_register.designed_launch_names` at `ClaimStatus::Designed`.

**Which records actually have a mount id.** The id is only realised through an
airframe's `weapons` row (T452-R.04 §1), and T452-R.04 measured that **no row
names `wep_24` and `wep_26`** — their `t452_04.limit.release_delay` claim stays
on the catalogue row for exactly that reason. Their runtime mount key therefore
names the id the measured name function would derive (24, 26), not a mount entry
any measured airframe builds; `wep_24` is registered as a measured *component*
and no airframe-based mount can reach it. `wep_26` is blocked for its own
reasons. T452-R.05 measured a second, different fact — the *loadout slot* reach:
`wep_04`, `wep_24`, `wep_25`, `wep_26` and `wep_27` are spawn-only, no plane
record's rocket slot selects them, and no retail script uses the `ammo` writer
that could install one (task #2173). That is a statement about the original's
loadout screen, not about the mount table: `wep_04`, `wep_25` and `wep_27` do
have airframe rows, so their mount ids are measured. Registering a spawn-only id
is a component definition, not a claim that a retail loadout can select it.

### Unit mass: how the measured 480 maps, and why no kilogram is asserted

T452-R.03 measured (`docs/findings/2026-10-28-t452-r03-hardpoint-price-and-gate-table.md`):
the airframe weight total adds `(record+0x34 + record+0x38) * 480` weight — a
flat **480 weight and 410 cost per enabled rocket box**, charged once per box,
reading **no weapon record field at all**; and the weight unit's conversion to
kilograms is unmeasured.

So the record has no per-type mass, and the runtime's `StackLoad` has only a
per-unit mass. The mapping stated here is:

- `unit_mass_kg` is `0.0` on every one of the sixteen, because the *measured*
  statement is the **absence of a per-type term**: a loaded type contributes
  nothing to any weight total. It is not a claim that a rocket weighs nothing.
- the flat **480 weight / 410 cost per enabled box** is kept as the named claim
  `t452_register.limit.per_box_weight`, whose content is "the flat
  per-enabled-box weight and cost this component's purchase charges" and whose
  resolving task is `T452-REG-PER-BOX-WEIGHT`. It is charged per *box*, not per
  round, in a weight unit whose kilogram conversion nothing measured, so it is
  not divided by the capacity, not spread over the rounds and not converted into
  kilograms here. No per-type table is invented, because none exists.

## What registers, and what stays blocked

The measurements express **ten** of the sixteen ids in the runtime vocabulary,
and the other **six** are blocked — each with one blocker per value, naming the
claim, the content it affects and the task that resolves it. Nothing is
defaulted to make an id register.

| id | outcome | why |
| --- | --- | --- |
| `wep_05`, `wep_06`, `wep_07`, `wep_08`, `wep_09`, `wep_10`, `wep_11`, `wep_14`, `wep_15`, `wep_24` | **registered** | full-inheritance launch, `DETONATION_DISTANCE` or declared timer fuse, measured stack, damage and media |
| `wep_04`, `wep_25`, `wep_27` | blocked | `ACCELERATION 150`: the measured lateral-remainder ramp (`t452_register.limit.inheritance_ramp`, `T452-REG-INHERITANCE-RAMP`) |
| `wep_12`, `wep_13` | blocked | the measured end of flight is a silent expiry at the end of `RANGE`, and the runtime fuse vocabulary only detonates (`t452_register.limit.silent_expiry`, `T452-REG-SILENT-EXPIRY`) |
| `wep_26` | blocked | the silent expiry (`t452_register.limit.silent_expiry`; the ramp is expressed since `T452-REG-INHERITANCE-RAMP`); its missing `FIRE` `SOUND` is a measured silence the media record expresses, not a blocker |

**Why these are blockers and not defaults.** `InheritanceRule` is
`Full`/`Fraction`/`None` and lives in `crates/cs_sim/src/weapons/guns.rs`, and
`LaunchGeometry` has no acceleration: the measured ramp — start at the launcher's
speed, keep the lateral remainder, accelerate along the launch direction — is not
expressible at all, and `Fraction { share: 1.0 }` would keep the along-direction
part and lose the ramp. `FuseRule` is `Impact`/`Proximity`/`Timed` and `Timed`
detonates; `ArmingRule::Disarmed` would express "never detonates" but overstates
a measurement that only covers the end of `RANGE`. `OrdnanceMedia` requires a
visual and carries the sound as optional, so a measured silent launch lowers as
no sound. A record without a `FLYOUT` `MODEL` would have no visual, but none
exists: all sixteen hardpoint records declare one (measured by
`accept_t452_register_retail_every_hardpoint_record_declares_a_flyout_model`),
so `t452_register.limit.no_flyout_model` was dropped and nothing defaults a
visual. Each remaining blocker is a vocabulary gap with a filed task, not
missing data: the measurement is done and is recorded in `cs_content`.

A registered id keeps the claims the runtime still cannot carry, named per
record by `original_kept_claim_context`: the `SONIC`/`FLASH` pilot
incapacitation (`t452_r02.limit.status_incapacitation`, follow-up
`T452-R05-STATUS-KIND` — a registered record simply carries no status for it),
the `BEEPER_SEEKER` retargeting (`t452_r02.limit.beeper_seeker_guidance`,
`T452-REG-BEEPER-SEEKER`), the tangler's effect
(`t452_r02.limit.tangler_flight_effect`, `T452-REG-TANGLER-EFFECT`) and the smoke
emitter's radius (`t452_r02.limit.smoke_radius`, `T452-REG-SMOKE-RADIUS`). The
catalogue layer from T452 is unchanged: its per-entry `t452.limit.runtime_lowering`
claim still describes the catalogue, and this report is the authority on which
ids are now registered as well.

## The acceptance criteria, answered

1. **Every id the installation declares is `register`ed with family, launch
   geometry, stack, arming, fuse, guidance, lifetime, area, damage, status and
   media, each traceable to a measurement.** Sixteen declared, **ten registered**
   with every one of those fields from the table above; **six blocked** with the
   measured value's claim, its content and its resolving task. The criterion is
   therefore **not met for six ids**, and cannot be met without inventing a value
   or extending the runtime vocabulary in a file this task does not own. The
   blockers are the honest record of that, and three tasks now exist to remove
   them.
2. **No claim is removed without the value behind it; every remaining claim
   names its content and resolving task.** The catalogue's claims are untouched,
   and every claim this stage records is either a blocker (content + task, by
   construction) or a kept claim with its context in `original_kept_claim_context`.
   The evidence report's `unknowns` are derived from that report, never typed.
3. **A test with prefix `accept_t452_register_` shows a measured component
   launching through the production session path, and the registry refusing an
   id with an unmeasured field.** Both exist:
   `accept_t452_register_a_registered_measured_component_launches_in_a_session`
   registers the lowered `wep_05` shape in an `OrdnanceSession` and launches it
   through `step_ordnance_session` (one item live, the measured 1200 m/s and one
   unit of the four-unit stack spent);
   `accept_t452_register_retail_a_registered_measured_component_launches` does
   the same with the record the production readers return from the owner's
   installation (the registry is filled by `register_measured_ordnance` over the
   read table, and the session launches that registered id); and
   `accept_t452_register_lowering_refuses_a_field_no_measurement_states` shows
   `lower_ordnance` refusing `stack.unit_mass_kg` by name plus the measured path
   refusing a record whose inheritance is unexpressible.
4. **The catalogue's evidence report is regenerated and validated.** Done for
   this stage's own report (below), not by re-running T452's: T452's harness
   measures T452's code, and re-running it here would report a tree it did not
   measure.

## Test sensitivity (measured, one mutation at a time)

Each mutation was applied to the committed production file and the
`accept_t452_register_` selection re-run with `--include-ignored`; the file was
restored byte-identically each time (`shasum 966841959e5e9de039b88ee8b545c7ea80185ed0`
of `crates/cs_app/src/ordnance.rs`):

| mutation | result | caught by |
| --- | --- | --- |
| the stack capacity becomes a defaulted `1` instead of the record's `CLUSTER_SIZE` | 4 failed, 3 passed (re-measured by the reviewer on the 7-test selection) | `accept_t452_register_a_measured_component_registers_with_its_measured_values`, `…_a_registered_measured_component_launches_in_a_session`, both retail tests |
| the ramping inheritance silently takes the `Full` path instead of blocking | 3 failed, 3 passed | `accept_t452_register_every_id_is_registered_or_names_its_blocker`, `…_lowering_refuses_a_field_no_measurement_states`, the retail registration test |

The retail test is the second half: it re-reads the owner's installation through
the production readers on every run, so a changed record, a changed stack, a
changed fuse radius or a changed outcome fails rather than passes.

## Evidence

`private/evidence/T452-RUNTIME-REGISTER/acceptance.json`, committed as
`docs/findings/evidence/T452-RUNTIME-REGISTER.json`, validates with
`python3 tools/validate_evidence.py private/evidence/T452-RUNTIME-REGISTER/acceptance.json
--artifact-root private/evidence/T452-RUNTIME-REGISTER` (structurally: its
`unknowns` carry the eight distinct limitations, so `--require-pass` is not used and no
limitation was removed to satisfy a validator). It records `capabilities:
["retail", "synthetic"]`, the `accept_t452_register_` selection — 7 tests, 7
passed, 0 failed, with `--include-ignored` and `CS_GAME_DIR` set — the
installation digests measured by `cs_assets::install` discovery, and two hashed
artifacts: the recorded `cargo-test.log` and `original-registration.json`, a
second production pass listing every declared id with its outcome, its runtime
family, its measured fields and every blocker with claim, content and task.
Identifiers, numbers and digests only. The report's `candidate_tree` is
`cbd88aeae2d2433d88ed38b0ab6deb0df7f7653b` (the code-and-test commit's tree);
this file and the committed copy are the documentation commit that follows it,
and nothing the report measured changed.

The report was regenerated by the reviewer on that tree after two fixes to the
harness below; both artifacts are valid JSON (the first version wrote the arming
label unquoted, which made `original-registration.json` unparseable) and every
assertion cites the artifacts that exist (`original-registration.json`, not the
catalogue report's name).

## Review

**Implementer:** `deepseek-1` (agent, model `deepseek-v4.1-flash`), session
2026-10-28 07:00–07:35, which handed the branch over without reviewing it.
**Reviewer:** `deepseek-1` (same agent name and model) in a **separate session
with a fresh context**, which read the diff, the parent task #2156's criteria,
the T452 closure and the R.01–R.05 findings, re-ran the checks and fixed what it
found. The reviewer is not an independent model; F28-D is mission-semantics and
fidelity territory where the project prefers a different agent instance or
model, and that preference was not available in this rotation. This is recorded
rather than glossed, and no agent review replaces the owner's human approval.

What the review checked and changed:

- **The evidence artifact was not JSON.** `original-registration.json` wrote the
  arming label unquoted (`"arming": after_ticks(0)`), which made the hashed
  artifact unparseable even though the validator only hashes it. Fixed in the
  harness (`jstr`), and the report regenerated.
- **The assertions cited an artifact that does not exist** —
  `original-catalogue.json` (T452's report name) instead of the
  `original-registration.json` this stage writes. Fixed and regenerated; both
  fixes are pinned by the regenerated report itself.
- **The harness's own reproduction command was wrong** (`--ignored` after the
  test filter instead of `-- --ignored`); corrected in its module doc.
- **The literal launch criterion needed the measured catalogue.** AC3 asks that
  a component *registered from the measured catalogue* launch through the
  production session path; the only launch test used a hand-built fixture shape.
  Added
  `accept_t452_register_retail_a_registered_measured_component_launches`, which
  reads `wep_05` through the production readers, registers it with
  `register_measured_ordnance` over the read table, and launches it with
  `step_ordnance_session` (1200 m/s, four-unit stack, one unit spent). It is
  listed in the harness's `RETAIL_TESTS`, so a report cannot be produced without
  it passing.
- **A mount statement was too broad.** The finding said the mount key's value is
  measured for every component; T452-R.04 measured that no airframe `weapons`
  row names `wep_24` or `wep_26`, so no mount entry is built for them, and
  T452-R.05 measured the five spawn-only records (`wep_04`, `wep_24`, `wep_25`,
  `wep_26`, `wep_27`). The finding now states both, separately, instead of
  folding them into one claim. No registered value changed: the mount key stays
  the designed `weapon_mount_NN` vocabulary carrying the measured name-derived
  id.
- **A blocker named the wrong claim and the wrong task.** `wep_12`'s area
  outlives its item because the measured `TANGLER` field lives on its own; the
  blocker attributed that to `t452_register.behavior` and to the silent-expiry
  task instead of to the measured tangler gap (#2179). It now names
  `t452_r02.limit.tangler_flight_effect` and `T452-REG-TANGLER-EFFECT` (the only
  effect `measured_area` gives a timed radius), so the evidence's `unknowns` are
  eight distinct claims, not nine.
- **Re-ran the capacity mutation** on the 7-test selection: defaulting the volley
  stack to `1` now fails 4 tests (the two synthetic ones and both retail tests);
  the production file was restored byte-identically
  (`shasum 5269e845170017b905814adaf66e072f356d557ad8ac847a7ecdfb43ee1fe436`).

The unmet part of the acceptance criterion is unchanged and is the point of the
branch: **six of sixteen ids are blocked, not registered**, because their
measured values have no runtime field (the ramping inheritance, the silent
range-end expiry and a measured silent launch), and guessing one is forbidden.
The reviewer confirmed each blocker names its claim, its content and its
resolving task, and that the follow-ups exist as #2174–#2180. "Every declared id
is registered" is therefore met for ten ids; that gap is recorded here and in
the evidence report rather than closed by a fabricated value.

## Not claimed

No original-run behavior: `retail` here is read access to the owner's files and
`static` analysis of the engine image, never evidence that the original
executable ran. No statement that a registered component flies as the original's
does — that its `VELOCITY` is the speed you would see, that its blast reaches the
actors the original's would, or that its marker lasts the same time on screen.
This awards at most **checked**, and no agent review replaces the owner's human
approval.
