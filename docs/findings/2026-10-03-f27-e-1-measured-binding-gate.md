# F27-E.1: the damage amounts and the gun mounts are still not measurable, and the gate that will bind them when they are

Date: 2026-10-03. Task: #547 "Bind the original ammunition damage amounts and
per-airframe gun mounts once they are measurable" (follow-up to #545 / F27-E,
which is itself the follow-up to #120 / F27-D). Sheet:
`specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`, F27 non-negotiable
1 ("no unverified multiplier table is hardcoded") and 2 ("mount transforms come
from the live aircraft hierarchy/damage state, not a fixed center-screen origin;
convergence and inherited velocity are explicit verified rules"), F27-D's AC04
closure target. Shared contract: `docs/contracts/CLI-EVIDENCE.md`.
Predecessors: `docs/findings/2026-10-03-f27-d-original-ammunition-and-loadout-audit.md`
and `docs/findings/2026-10-03-f27-e-original-ammunition-and-gun-names-imported.md`.

Capabilities used: `retail` (read-only access to `$CS_GAME_DIR`) and ordinary
build/test. **Not** used and not claimed: any run of the original executable, so
nothing here is evidence of how the original *behaves* — only of what its files
declare.

## The short version

The task asked for two numbers to be bound. **Neither is measurable from the
files this installation ships**, and this stage measured that rather than
repeating the argument: four places a table like that would have to be in were
read through production readers and each fails a falsifiable test
(`accept_f27_e_1_retail_*`, six tests). So no number was invented. What the stage
delivers instead is the thing that was missing on `main` and that F27-E
explicitly did not build: **the gate the measurements come through**, plus the
machine-readable accounting that turns "we could not measure it" into a claim id
with a destination.

F27-E's note that no `DeclaredGunDefinition` could be built for the original's
five guns is a statement about a **schema hole**, and that is what this stage
fixes. `DeclaredGunDefinition::try_new` requires a concrete `DamageNodeKey` and
a concrete `DeclaredGunMountKind` — they are the only load-bearing fields in
either declared record that are **not** `Resolved` — so an importer that has not
measured them has no way to say so except to design one. The gate turns "no way
to say so" into "a refusal that names the field".

## Files (listed before editing)

- `crates/cs_content/src/weapons.rs` (extend): the gate
  (`is_observed_evidence`, `GunMountField`, `UnmeasuredCause`,
  `UnobservedValue`, `MeasuredGunMount`, `GunMountRefusal`, `bind_gun_mount`,
  `MeasuredAmmunitionDamage`, `DamageBindingRefusal`, `bind_ammunition_damage`,
  `unmeasured_ammunition_types`, `unmeasured_gun_mounts`) and the accounting
  (`OriginalLimitClaim`, `LimitEvidence`, `LimitOutcome`, `LimitClaimRow`,
  `LimitReportError`, `OriginalLimitReport`), plus the module doc.
- `crates/cs_content/tests/accept_f27_e_1_measured_binding.rs` (**new**, 13 fast
  tests over the gate).
- `crates/cs_content/tests/f27_e_1_support/mod.rs` (**new**, the shared retail
  read, included by the two targets below).
- `crates/cs_content/tests/accept_f27_e_1_retail_measurability.rs` (**new**, 6
  `#[ignore = "requires CS_GAME_DIR"]` tests).
- `crates/cs_content/tests/evidence_report_f27_e_1.rs` (**new**, evidence
  harness, not an acceptance test).
- This file and `docs/findings/evidence/F27-E.1.json`.

No protected path, no `Cargo.toml`/`Cargo.lock` change, no original data, no
binary file. `crates/cs_sim/src/weapons/guns.rs` and
`crates/cs_app/src/weapons.rs` are untouched — see "Why no runtime change".

## The one observable failure, before the change

Given a declared ammunition record whose two damage channels are
`Resolved::Unknown`, and given a declared gun record, there was **no production
function anywhere** that could turn a measurement into either of them. The
declared schema's answer to "we do not know this amount" was a `Resolved`, and
the lowering boundary's answer was to refuse the record — but nothing could
*improve* a record, so there was no path by which a future capture could reach
the catalogue at all. Concretely, a stage handed a per-type damage table would
have had to rebuild `DeclaredAmmunition::try_new` field by field inside its own
code, and `bind` a mount would have had to invent a `DamageNodeKey` and a
`DeclaredGunMountKind` before it could call `DeclaredGunDefinition::try_new` —
which is F27 non-negotiable 2's exact prohibition wearing a valid id.

And in the other direction: the five `f27.d.limit.*` claims F27-D recorded as
fidelity limitations existed only as **prose in a findings file and as a
`resolving_task` string in F27-D's hashed artifact**. Nothing in the production
code could answer "is this claim still open, and where did it go?" — so a later
stage could neither notice that a claim had quietly been dropped nor prove that
it had not.

## What was measured, and what it says

Six retail tests, six falsifiable readings. Every number below is re-measured on
every run; a stale constant fails instead of passing.

### 1. The original's own ammunition rows state no number at all

`GOSDATA/ASSETS/BINARIES/langui.dll` (282 624 bytes) read through
`cs_formats::pe_resources`. The four rows of the block `RESOURCE.H` declares as
`IDS_AMMODESCRIPTION 3370` are each non-empty, each carry the shipped `[COUR9]`
markup code, each are longer than 40 UTF-16 code units — and, **after the markup
code is split off, not one of them contains an ASCII digit**. The same holds for
the three ammunition name blocks at `3350`, `3360` and `3365`. This is the
falsifiable form of "the shipped text holds no damage table": a `12.5` or a
`30 %` in any of those twelve rows fails the test.

What the four descriptions do say is *relative* — read, measured for length and
digit-freedom, and **not committed**: that standard lead bullets damage armor and
internal components equally; that a dum-dum round's split head flattens on impact
and spreads damage over a broader area; that an armor-piercing round shreds armor
and tends to punch clean through unarmored surfaces inflicting very little
damage; that an explosive round's shaped charge detonates on any hard surface.
That is a description of four *behaviors*, and no quantity anywhere in it.

### 2. The five gun rows are caliber labels, five distinct bores and nothing else

The block `IDS_GUNSHORTNAME 3320` holds five rows; each carries exactly two ASCII
digits, and the five bores are `3`, `4`, `5`, `6`, `7`, each followed by `0`.
The five gun *long* names at `IDS_GUNLONGNAME 3310` carry **no** markup code —
the one run that does not — and state exactly their own caliber's two digits and
no other number. So the installation's own gun vocabulary is "which bore", never
"how much damage".

### 3. Neither generated header declares a damage or ballistic constant

`ASSETS/SCRIPTS/RESOURCE.H` and `ASSETS/SCRIPTS/RESRC1.H`, read through
`cs_assets`' ROF mount and `cs_formats::text::read_resource_header`. Across both
members **not one `#define` value spells a decimal point or a comma** — a
multiplier table would need both. And of the defines whose *name* mentions a
gun, an ammunition or armor, **not one is named after damage, caliber, calibre
spelling, penetration, ricochet or a bullet**: they are all string ids and UI
constants. The twenty gun-group ids `3061..=3080` and the six row-block ids are
re-measured from these headers on every run and pinned against
`ORIGINAL_GUN_GROUPS` and `ORIGINAL_AMMO_NAME_BLOCKS`.

### 4. The image that would hold the table carries no plaintext

`crimson.exe` read as inert PE data through `cs_formats::pe_resources`. Seven
sections, named `.txt`, `.text`, `.txt2`, `.rdata`, `.data`, `.rsrc`, `.reloc`.
The first code section `.txt` is at a Shannon entropy of **7.997 bits per byte**
— packed or encrypted. `.rsrc` is 146 944 raw bytes of which **145 945 are zero**,
and the `IMAGE_DIRECTORY_ENTRY_RESOURCE` directory describes **1 962** of them,
under 5 % of the section: there is no room in it for a table. The directory's
depth-one resource types are exactly **3, 14 and 16** — icon, version and group
icon — so the executable carries **no `RT_STRING` at all**. `crimson.icd`
(2 580 578 bytes) is a second `MZ` image at whole-length entropy **7.811**, and
across eleven words a damage table would be spelled with (`caliber`, `calibre`,
`ammo`, `armor`, `armour`, `piercing`, `incend`, `damage`, `rocket`, `shell`,
`heat`) its bytes carry **three occurrences in total**.

The entropy figures above are computed in the test, not by a production reader:
no production reader measures entropy. The *bytes* are production-read, which is
what the measurement is about.

### 5. The shared aircraft geometry names gun meshes, never a declared gun group

`ZBD/planes.zbd` read through `cs_formats::gamez::read_gamez_nodes`: **3 317**
nodes. More than ten distinct node names mention a gun or a turret, and **none
of them is any of the twenty declared gun-group identifiers**. That is why the
eleven wing-station groups (`INNERWINGGUNS` and its siblings, which name a
station and in most cases omit the side) cannot be put on a side from the mesh
data, why `DeclaredGunMountKind::original_groups` still maps only the nine the
original's own labels determine, and why `bind_gun_mount` refuses.

### 6. The accounting: five claims, all deferred, all re-filed

`OriginalLimitReport` is the production answer to "is this claim still open?".
For this installation all five claims are recorded `Unmeasurable` and each is
re-filed, so `unaccounted()` is empty and `is_complete()` is **true** — while
`bound()` is **empty**. That pairing is the honest result and the report's
`is_complete` doc says so in as many words: *accounting is not resolution*.

## The gate

`bind_gun_mount(record, measured)` is the only production path from a measurement
to a `DeclaredGunDefinition`, and it holds one rule: **the record's mount, mount
kind and scene binding are the measurement's**, never a default and never a
neighbouring record's. Everything else is carried across untouched, so a field
nobody measured stays `Resolved::Unknown` and still refuses to lower. A
measurement is usable when its field is `Known` **and** its provenance is an
observation — `VerifiedOriginal` or `ObservedTool`. A `Known` value carrying
`Designed`, `Documented`, `Inferred`, `Unknown` or `Contradicted` provenance is
reported as unmeasured, which is what keeps the project's own designed fixture
mount out of an original gun record.

`bind_ammunition_damage(record, measured)` copies the measured amounts across
**verbatim**, channel by channel: a channel the measurement leaves open keeps the
record's own `Resolved::Unknown`, with its own claim id and reason. Nothing is
interpolated, scaled or copied from another type, so a five-by-four multiplier
table cannot be expressed through this gate even if someone wrote one. A profile
with no usable amount on either channel binds nothing, and a profile measured for
another type is refused.

The two closure helpers exist because a claim is about *every* type or *every*
gun: `unmeasured_ammunition_types` and `unmeasured_gun_mounts` list what a
partial measurement leaves open, and a mount measurement with a hole counts as no
coverage at all — so three measured types out of four cannot report themselves as
measured.

## Why no runtime change

`crates/cs_sim/src/weapons/guns.rs` and `crates/cs_app/src/weapons.rs` are
untouched. `lower_gun` needs a known caliber, rate, muzzle velocity, lifetime,
spread, damage, inheritance, effect and sound, and none of those is measurable
either, so even a perfectly bound mount could not produce a runtime gun today:
adding a runtime path now would be a code path with no reachable input, which is
the speculative infrastructure F27 non-negotiable 4 warns against. The gate's
consumer on the declared half is real, though: `AmmunitionAudit` is what waits
for the binding, and
`accept_f27_e_1_a_bound_type_stops_the_audit_reporting_no_damage_consumer` shows
the production `no_damage_consumer` finding appear for an unmeasured type and
disappear for the same type once a measurement is bound through the gate.

## Choices worth stating

- **The gate refuses; it never repairs.** There is no "if the mount is missing,
  use `Nose`" branch anywhere, and no place where a missing channel picks up a
  neighbour's amount. That is the whole point, and each of those defaults would
  have been a mutation the tests catch (see below).
- **`is_observed_evidence` names two classes out of seven.** The five it rejects
  are rejected for stated reasons, and `Documented` is the important one: F27's
  research boundary says the public manual establishes no ammunition or ballistic
  table, so a cited-document amount is not a measurement either. If the owner
  later rules that a specific document *is* authoritative, that is one match arm.
- **The unmeasured mount fields are not made `Resolved` on the schema.** That
  would be the deeper fix, but it changes `DeclaredGunDefinition`,
  `lower_gun`, every fixture and every caller, which is a different slice with a
  different blast radius. The gate makes the hole reachable without it: the
  refusal names the field, so the next stage knows exactly what it owes.
- **The re-filing targets live in the test's table, not in production.** They name
  tasks (#358, #357, F29) that exist in the tracker, not in the code; hardcoding
  them in `cs_content` would make the library depend on the project's queue. The
  *rule* — every deferred claim must name a destination or it is unaccounted — is
  production.
- **The retail test asserts four places, not a byte-scan of all 846 container
  members.** Decoding one member of `crimson.rof` through the production reader
  costs about half a second and re-mounts the container each time, so a
  74-member scan inside a test is a slow test, not a better one. The four places
  chosen are the ones a table of this shape would have to be in: the rows the
  loadout screens ask the engine for, the build's own generated headers, the
  executable, and the mesh node names. As **out-of-band diligence** every one of
  the **74 non-graphics members** of `crimson.rof` (64 scripts, `LAYOUT.CSV`,
  `SCRAPBOOK.CSV` and 8 WAVs; 3.0 MB decoded) was exported through
  `cs-inspect rof --member` during this session and searched for
  `damage`, `piercing`, `ricochet`, `penetrat`, `caliber`, `calibre`, `bullet`
  and `shrapnel`: **zero occurrences in any member.** (`armor` is deliberately
  *not* in that list — armor is a shipped concept and belongs to F29; the only
  matches for it are `IDS_*` identifiers and the plane-construction armor list's
  control keys.) That scan was done with a scratch script, is **not** committed
  and **not** a claim the suite makes; it is recorded here because it is the
  cheapest way for a later stage to see that nothing else in the container is
  worth re-reading for a damage table.

## Test sensitivity

The gate's whole value is that it cannot be talked into inventing a value, so
every decision it makes was removed one at a time — production **or** test
source patched in place, restored after each run — and the whole
`accept_f27_e_1_` selection re-run with `--include-ignored`. **21 mutations, 21
caught.** The "caught by" column is what the run reported, not an estimate, and
the harness that applied them was a scratch script under `private/` (not
committed): it rewrites source in place and restores it, which is a one-off
measurement rather than a test.

| mutation | caught by |
| --- | --- |
| `unmeasured()` skips the mount kind | `a_mount_missing_any_one_field_is_refused_by_name`, `the_closure_helpers_count_incomplete_mounts_as_unmeasured` |
| `unmeasured()` skips the scene binding | the same two |
| `unmeasured_of` ignores the provenance class | `a_damage_measurement_with_no_usable_amount_binds_nothing`, `a_known_field_that_is_not_an_observation_is_refused` |
| `is_observed_evidence` accepts every class | the same two plus `the_five_claims_carry_f27_ds_own_ids` |
| `bind_gun_mount` drops the gun-mismatch check | `a_mount_measured_for_another_gun_is_refused` |
| `bind_gun_mount` drops the measurement's scene binding | `a_measured_mount_replaces_the_placeholder_and_keeps_the_rest_unknown` |
| `bind_ammunition_damage` copies the record's damage instead of the measurement's | `a_bound_type_stops_the_audit_reporting_no_damage_consumer`, `a_measured_damage_binds_only_the_measured_channel` |
| `bind_ammunition_damage` fills an unmeasured channel from the measured one | `a_measured_damage_binds_only_the_measured_channel` |
| `bind_ammunition_damage` accepts a profile with no usable amount | `a_damage_measurement_with_no_usable_amount_binds_nothing` |
| `bind_ammunition_damage` drops the type-mismatch check | `damage_measured_for_another_type_is_refused` |
| `measured_channels` counts a measured zero as nothing | `the_five_claims_carry_f27_ds_own_ids` |
| `unmeasured_gun_mounts` counts an incomplete measurement as coverage | `the_closure_helpers_count_incomplete_mounts_as_unmeasured` |
| `record()` maps `PartlyMeasured` to `Bound` | `a_partial_measurement_cannot_resolve_a_claim` |
| `unaccounted` ignores a deferral with no destination | `a_partial_measurement_cannot_resolve_a_claim`, `every_f27_d_limit_claim_is_accounted_or_the_report_is_incomplete` |
| `refile` accepts a bound claim | `a_refiling_refuses_a_bound_claim_and_an_empty_target` |
| `refile` accepts an empty target | the same test |
| a `f27.d.limit.*` claim id is renamed | `the_five_claims_carry_f27_ds_own_ids` |
| the support module stops splitting the markup code off | the three retail row tests |
| the support module counts digits with the markup still attached | the same three |
| `declared_id` returns a constant | the same three plus `retail_the_two_generated_headers_declare_no_damage_constant` |
| `read_image` stops measuring a section's entropy | `retail_the_image_that_would_hold_the_table_carries_no_plaintext`, `retail_every_f27_d_limit_claim_is_deferred_and_re_filed` |
| `read_node_census` claims every declared group is a node name | `retail_no_plane_node_names_a_declared_gun_group`, `retail_every_f27_d_limit_claim_is_deferred_and_re_filed` |

Two rows report more than one test because the mutation was in shared code — a
predicate several tests exercise — and both were listed from the run. Three
mutations' first patches did not compile because they left the surrounding match
arm dangling; they were re-measured with a complete patch before the table above
was written, and **no row above rests on a patch that failed to build**: the
harness records an anchor miss separately and the two rows that had one were
re-measured, not counted.

## Unknowns recorded (not guessed)

All five are `f27.d.limit.*` claims, all measured above to be unmeasurable in
this installation, all **re-filed** by this stage in
`crates/cs_content/tests/f27_e_1_support/mod.rs`'s `REFILED` table, in the
evidence report's `review.method`, in the hashed artifact
`binding-measurability.json`, and in follow-up tasks filed with Rally:

- **`f27.d.limit.ammo_names_damage`** — the per-type damage amounts. Affected
  content: every damage amount F27 simulates. Resolving task: the owner-supplied
  original-run capture (#358 `REF-OWNER-FIRST-CAPTURE`, protocol #357), **or**
  follow-up **#549 (F27-E.2)**, the one agent-reachable route to the same
  numbers: recovering the tables from the image itself, or recording precisely
  which static routes were tried and why each fails.
- **`f27.d.limit.gun_group_assignment`** — which side each of the eleven
  wing-station groups is on, and which airframe mounts which group. Affected
  content: `DeclaredGunMountKind`'s vocabulary and every mount transform.
  Resolving task: the same capture, plus F29's armor-zone work — the original's
  own armor screen (`ASSETS/SCRIPTS/ARMOR.SCRIPT`) names four positions
  (`ar_t_nosetitle`, `ar_t_tailtitle`, `ar_t_lefttitle`, `ar_t_righttitle`) and
  four hardpoint points, which is a *measured* lead about the original's position
  vocabulary and **not** a resolution: nothing maps a gun group onto one of those
  four positions. Filed as follow-up **#550 (F27-E.3)**.
- **`f27.d.limit.convergence`** — whether and where paired wing guns' barrels
  meet. `cs_sim::weapons::MountTransform::forward` still carries the resolved
  direction and this stage invents no convergence geometry.
- **`f27.d.limit.inheritance`** — the inherited-velocity rule, still a declared
  `Resolved` option with no measured value.
- **`f27.d.limit.interaction_rules`** — penetration, ricochet and in-flight
  ammunition switching. Still declared, still read by no production path, still
  deferred by `cs_content::weapons::InteractionRules::deferred`. **This stage
  does not re-point that deferral at itself**: F27-E.1 resolves none of the three,
  and re-filing a deferral to the stage that could not resolve it would make the
  machine-readable contract less true.

## Evidence

`private/evidence/F27-E.1/acceptance.json`, committed as
`docs/findings/evidence/F27-E.1.json`, validates with
`tools/validate_evidence.py --require-pass`. It records
`capabilities: ["retail", "synthetic"]`, `claim: "implemented"`, the
`accept_f27_e_1_` test counts (19 tests, 6 of them retail, all run with
`--include-ignored`), the installation and content digests measured by production
`cs_assets::install` discovery, and two hashed artifacts: `cargo-test.log` and
`binding-measurability.json` — a **second** production pass over the same
installation carrying each member's decoded length and SHA-256, the declared
ids, the ammunition-description and caliber row properties, the ballistic-word
defines, `crimson.exe`'s sections and entropy, `crimson.icd`'s keyword census, the
`ZBD/planes.zbd` node census, and the five claims with this stage's verdict and
re-filing target.

**`unknowns` is empty and that is a claim worth checking.** The validator's
`--require-pass` rejects a nonempty `unknowns` list. The five limitations are
recorded in the four places above that the validator does not read for that field,
rather than removed. They are *unmeasured original behavior*, not failures of
this stage's assertions: every assertion here is a measured fact about shipped
files or a production refusal the suite exercised, and all of them pass. `claim`
is `implemented`, never `verified_original`: what was verified is what the
installation's **files** declare, and `retail` here is read access, not evidence
that the original executable ran.

The reviewer should regenerate the report on the rebased commit and compare it.
The tree hash is in the report and is checked against `HEAD^{tree}` by the harness
itself, so a report from another commit cannot be reused.

## Not claimed

No original-data *verification* of gameplay behavior. This stage measured what the
installation's files declare, built the gate those measurements will come through,
and recorded that five `f27.d.limit.*` claims remain open and where each now lives.
It does **not** bind a single damage amount or a single original mount, because
none is measurable here, and it awards at most **checked**. `human_play`,
`human_review` and `network_real` were not available and are not claimed.