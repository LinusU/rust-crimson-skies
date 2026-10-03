# F27-D: the original ammunition/loadout surface, measured; and the audit that says what it cannot map

Date: 2026-10-03. Task: #120 "Verify every original gun/ammunition combination
and convergence rule" (`specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`,
section `### F27-D`, AC04). Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`,
sections "Collision and ballistic tests" and "Inputs and outputs".
Predecessors recorded in
`docs/findings/2026-10-01-f27-a-weapon-ammo-schemas-and-fire-events.md`,
`docs/findings/2026-10-02-f27-b-gun-cadence-mounts-and-swept-ballistics.md`,
`docs/findings/2026-10-02-f27-c-candidate-filtering-and-hit-damage-routing.md`
and `docs/findings/2026-10-02-f27-c-weapon-session-wiring.md`.
Capabilities used: `retail` (read access to `$CS_GAME_DIR`) and ordinary
build/test. **Not** used and not claimed: any run of the original executable, so
no statement here is evidence of how the original *behaves* — only of what its
files declare.

## Files and the one observable failure (listed before editing)

- `crates/cs_sim/src/weapons/guns.rs` (extend): the measured original gun
  vocabulary (`ORIGINAL_GUN_GROUPS`, `ORIGINAL_AMMO_NAME_BLOCKS`,
  `ORIGINAL_AMMUNITION_TYPES`, `ORIGINAL_SELECTABLE_GUNS`,
  `ORIGINAL_GUN_SLOTS`, `ORIGINAL_ROCKET_SLOTS`, `ORIGINAL_HARDPOINT_POINTS`,
  `GunGroupName`, `covers_group`, `original_gun_group_ids`,
  `original_groups_for`, `uncovered_original_gun_groups`), the damage-consumer
  half (`DAMAGE_CONSUMED_BY_ROUTER`, `AmmunitionDamageConsumer`,
  `AmmunitionRegistry`, `AmmunitionRefusal`, `AmmunitionDeclaration`,
  `DivergentAmmunition`, `Divergence`) and `FireResolver::shooters` /
  `::definitions`.
- `crates/cs_sim/src/weapons/mod.rs` (wiring only): the re-exports and the module
  docs.
- `crates/cs_content/src/weapons.rs` (extend): the declared mirror
  (`DeclaredGunGroup`, `ORIGINAL_GUN_GROUPS`, `original_groups`,
  `covers_group`, `uncovered_original_gun_groups`), `OriginalLoadoutCounts`,
  `OriginalGunLoadout`, `OriginalLoadoutError`, `AmmoBehavior`,
  `AmmoDamageConsumer`, `DAMAGE_CONSUMED_BY_ROUTER`, `AmmoAuditRow`,
  `AmmoAuditFinding`, `AmmunitionAuditReport`, `AmmunitionAudit`.
- `crates/cs_app/src/weapons.rs` (extend): `SessionAmmunitionRow`,
  `SessionAmmunitionAudit`, `session_ammunition_audit`.
- `crates/cs_sim/tests/accept_f27_d_ammunition_registry.rs` (**new**),
  `crates/cs_content/tests/accept_f27_d_ammo_audit.rs` (**new**),
  `crates/cs_content/tests/accept_f27_d_retail_ammo_catalogue.rs` (**new**,
  `#[ignore]`), `crates/cs_app/tests/accept_f27_d_session_ammo_audit.rs`
  (**new**), `crates/cs_content/tests/f27_d_support/mod.rs` (**new**, the shared
  retail read, included by the two `cs_content` targets above and by the
  evidence harness so all three describe one installation),
  `crates/cs_content/tests/evidence_report_f27_d.rs` (**new**, evidence harness,
  not an acceptance test).
- This file and `docs/findings/evidence/F27-D.json`.

No protected path, no `Cargo.toml`/`Cargo.lock` change, no original data, no
binary file.

**One observable failure, before the change:** F27-A's schema said in prose that
"the original set is enumerated by F27-D from the installation", and both halves
of the module said the same, and **nothing did it**. Given the declared
ammunition records, guns and loadouts of an installation there was no production
query at all that produced a row per ammunition type, so nothing could say what
a type's behavior is or which damage consumer applies it. And nothing could say
the *opposite*: there was no number anywhere in the project that "every type"
could be checked against, so a catalogue holding one type — the synthetic
fixture's — would have satisfied any check there was. The audit was absent in
both directions, which is the failure AC04 names.

## What this stage measures, and where each number comes from

Everything below was read out of two retail members of
`GOSDATA/ASSETS/crimson.rof`, through the production readers (`cs_assets`' ROF
mount and `cs_formats`' bounded member decoder), and is re-measured by
`accept_f27_d_retail_ammo_catalogue.rs` on every run, so a stale constant fails
rather than passing.

### `ASSETS/SCRIPTS/RESOURCE.H` — the engine's own resource header

This member is a C include the original build generated, shipped inside the
asset container. It declares the game's identifier vocabulary, and it is where
the gun and ammunition identity blocks live.

| what | measurement |
| --- | --- |
| gun groups (hardpoints) | twenty contiguous string ids, `3061..=3080` (`IDS_INNERWINGGUNS` … `IDS_NOSETURRET`); `3060` is `IDS_AIRFRAMEGUNGROUPNAMES`, the table's header, and names no group |
| ammunition name blocks | four: `IDS_AMMOLONGNAME 3350`, `IDS_AMMOSHORTNAME 3360`, `IDS_AMMOABBRNAME 3365`, `IDS_AMMODESCRIPTION 3370`. They are **not** equally wide: the next block the header declares after them is `IDS_ROCKETLONGNAME 3380`, so the gaps between the four bases are `10`, `5` and `5` |
| ammunition types | four — the multiplayer ammunition screen iterates `selection` over `1..=4` and indexes the description block as `3370 + selection - 1`, so the descriptions occupy `3370..=3373`. The count is **not** read from the block gaps, which would give 10 or 5 |
| guns | five — `IDS_NUMGUNS 506` names the string, not the count; the count is five (below) |
| gun identity blocks | `IDS_GUNLONGNAME 3310`, `IDS_GUNSHORTNAME 3320`, `IDS_GUNDESCRIPTION 3330`. Their spacing does **not** encode the gun count, so the count of five comes from the screens below, not from here |

### The loadout screens — the counts, read a second independent way

- `ASSETS/SCRIPTS/MULTIPLAYER_AMMOG.SCRIPT` iterates four gun slots
  (`for (LHA=0; LHA < 4; LHA++)`), builds five dropdown rows per hardpoint
  (`for(OHA = 0; OHA < 4 + 1; OHA++)` — index 0 is a header row whose label is
  supplied by `callback($$NB$$,10139,THA)` and is therefore not readable from any
  file, and 1..=4 are the four types), and asks the engine for `string VHA[4]`
  (the four ammunition names) and `string UHA[5]` (the five gun names).
- `ASSETS/SCRIPTS/ORDINANCELAYOUT.SCRIPT` builds `object PKA[4]` /
  `object QKA[4]` (four guns, four gun-ammunition dropdowns) and
  `object RKA[8]` (eight rocket-ammunition dropdowns) — so **four gun slots and
  eight rocket slots** on one airframe.
- `ASSETS/SCRIPTS/GUNS.SCRIPT` offers four gun slots in plane construction;
  `ASSETS/SCRIPTS/HARDPOINTS.SCRIPT` offers **two hardpoint points**.
- `ASSETS/LAYOUT.CSV` declares `OL_D_AMMO0..OL_D_AMMO3` and a group
  `V6=GUNS,5`. The group is five entries wide, but the file does not say what
  the group holds, so it is **not** the decisive evidence for the gun count; the
  two five-element gun-name arrays above are.

So **four ammunition types, five selectable guns, four gun slots, eight rocket
slots, two hardpoint points** — each confirmed by at least two independent
places, and the five-guns count by two five-element gun-name arrays
(`string UHA[5]` in the multiplayer screen and `object ZAA[5]` /
`for(int R=0; R < 5; R++)` in the outlaw gun screen) that ask the engine for
gun names rather than inferring one.

### `ASSETS/SCRIPTS/DEBUGINFO.TXT` — the engine's variable dictionary

This member names the loadout identifiers in the original's own words, which is
what lets the audit speak the original's vocabulary rather than this project's:
`ngunslot` = LHA (the gun slot index), `ngunid` = MHA, `ngammoid` = NHA (the
ammunition id), `ngammoitem` = OHA, `argunname` = UHA, `argammoname` = VHA,
`nhardpoint` = YHA, and a separate rocket space (`nroc` = VIA, `nrammoid`,
`arrocketnames` = YIA). Guns and rockets are **separate id spaces**, and gun
ammunition (`gammo`) is again separate from rocket ammunition (`ramm`).

## What is measured, and what is not

**Measured (file content, ids and counts):** everything in the two tables above.

**Not measured, and not guessed:** the *names* of the four ammunition types, their
calibers, their per-type damage amounts, the convergence rule and the
penetration/ricochet/ammo-switching behaviors. All of them live in the
executable's own tables and its runtime string catalog:

- the ammunition screen never names a type — it asks the engine for the array
  (`callback($$E$$,5054,VHA[0])` fills `string VHA[4]`), and the shared item list
  template `@shareditems@BUA` takes its label from
  `parent.BN.IN.BC[parent.QG]`, i.e. from the same engine-supplied list;
- `crimson.exe` carries **no** `RT_STRING` resource at all (its resource
  directory holds only types 3, 14 and 16), and `strings.dll`'s `RT_STRING`
  blocks run `7..=1072`, which under the measured `(block - 1) * 16 + index`
  numbering covers ids `96..=17151` and **does not contain block 211**, the block
  that would hold id `3370`. So the id the scripts use is not a Win32 string-table
  id in either numbering, and the catalog behind `callback($$E$$,5067,10585,…)`
  is a different one entirely.

Reading those would require running the original executable, which no agent can
do. **The audit therefore reports them as unknown by name instead of filling them
in** — which is the whole point of the deliverable, and the reason this stage
produces a *complete-looking* failure rather than a passing catalogue.

## The audit

`cs_content::weapons::AmmunitionAudit` walks the declared ammunition records, the
declared guns and the declared loadouts and produces one
`AmmoAuditRow` per distinct type: its caliber, its `AmmoBehavior` (which options
a production path applies, which are deferred and why, which values are
unmeasured), its `AmmoDamageConsumer` (which channels carry a known amount and
which production path applies them), and which guns and loadouts pair with it.

It then compares that against a **measured** `OriginalGunLoadout` — the closure
target AC04 needs. Without a number the installation itself declares, "every type"
is unfalsifiable, so `AmmunitionAuditReport::is_complete()` requires:

1. at least as many declared types as the installation declares
   (`UndeclaredAmmunitionType`);
2. every gun group the installation names covered by a declared mount kind
   (`UncoveredGunGroup`);
3. every declared mount kind *in use* covering at least one group *this*
   installation names (`UnobservedMountKind`);
4. per type: a known caliber (`UnmeasuredCaliber`), no unknown interaction option
   (`UnmeasuredRule`), at least one known damage amount (`NoDamageConsumer`), and
   at least one gun pairing it (`Unpaired`);
5. per loadout: no gun and no ammunition type nothing describes
   (`UndescribedGun`, `UndescribedAmmunition`).

Anything else is a named finding, so a partial audit reports itself as partial.

`cs_app::weapons::session_ammunition_audit` is the runtime half: it walks a real
`WeaponSession`'s registered guns through `cs_sim::weapons::AmmunitionRegistry`
and reports, per ammunition type the session can fire, the mounts firing it and
the channels the router would emit damage on (`DAMAGE_CONSUMED_BY_ROUTER` =
`cs_sim::weapons::GunHitRouter::route`). A type whose amounts are zero on every
channel is reported as **consumed by nothing** — a round of it costs a round,
sounds, emits an effect and lands for nothing — and two mounts that contradict
each other about one type are refused by name (`DivergentAmmunition`), with the
first declaration left standing rather than overwritten.

## The stage's substantive finding: the mount vocabulary does not fit

`GunMountKind` is a **designed** five-kind set: `Nose`, `WingLeft`, `WingRight`,
`Tail`, `Gondola`. The original declares **twenty** gun groups. Nine of them are
placed by the original's own labels and are covered:

| designed kind | measured groups |
| --- | --- |
| `Nose` | `LOWERNOSEGUNS 3063`, `UPPERNOSEGUNS 3064`, `NOSEGUNS 3071`, `NOSEGUNS2 3072`, `NOSETURRET 3080` |
| `WingLeft` | `LEFTWINGGUNS 3068` |
| `WingRight` | `RIGHTWINGGUNS 3067` |
| `Tail` | `REARTURRET 3073` |
| `Gondola` | `RIGHTFUSELAGEGUNS 3066` |

The other **eleven** are `INNERWINGGUNS 3061`, `OUTERWINGGUNS 3062`,
`CENTERGUNS 3065`, `OUTERWINGGUNS2 3069`, `INNERWINGGUNS2 3070`,
`LOWINNERWINGGUNS 3074`, `LOWOUTERWINGGUNS 3075`, `UPPERINNERWINGGUNS 3076`,
`UPPEROUTERWINGGUNS 3077`, `CENTERGUNS2 3078` and `MIDDLEWINGGUNS 3079`. These
name a **wing station** (inner, outer, middle, centre; upper, lower) and in most
cases **omit the side**. Which side a given airframe's inner-wing group is on is
in the executable's per-airframe gun tables, which no agent can read.

`covers_group` therefore maps **only** the nine the labels determine, and the
eleven stay uncovered and are reported by name. Assigning them a side would be a
guess presented as a mount rule — exactly what F27 non-negotiable 2 and 4
forbid — and it would also make the audit pass on a fact nobody measured. The
regression is reported, and the follow-up task below asks for the evidence that
would resolve it.

Note what the coverage table *gained*: the original does have a rear mount
(`REARTURRET`), which the designed `Tail` kind covers, and a nose turret
(`NOSETURRET`), which the designed `Nose` kind covers. The designed set was not
missing those positions; it was missing the wing-station and side distinctions.

## Choices worth stating

- **The audit reports, it never repairs.** No invented fourth ammunition type, no
  assigned side, no defaulted damage amount. A deferral is a *schema* fact and is
  reported on the row (`AmmoBehavior::deferred`), not as a finding — otherwise
  `is_complete()` would be unreachable for every record ever written, including
  the original's.
- **A known amount of zero is a measurement, not a gap.** The original's data can
  declare a channel it does not damage; `AmmoDamageConsumer::is_consumed` counts a
  known zero as consumed, and the *runtime* registry (`AmmunitionDamageConsumer`)
  is where "delivers nothing" is decided, because that is a question about the
  runtime rather than the record.
- **Two records for one type are one type.** The rows are keyed by type id, so a
  duplicated record cannot inflate the type count past the closure check. The
  disagreement between the two records is *not* averaged or resolved — the first
  in insertion order is reported, and resolving it is the importer's job.
- **A dangling loadout reference is reported once, not once per pairing.**
  `DeclaredLoadout::pairings()` crosses every gun with every type, so the
  dangling-reference checks walk `guns()` and `ammunition()` directly; otherwise
  one undeclared type would be reported `guns` times and a caller counting
  findings could not tell a real second gap from an echo. (This was a defect found
  by `accept_f27_d_a_dangling_loadout_reference_is_reported_both_ways` and fixed
  during this stage.)
- **`UnobservedMountKind` is about the surface, not the kind in the abstract.**
  `WingLeft` covers a real measured group, so on a surface that names only a nose
  group it is the *wing* kind that is unobserved there. (Also a defect found and
  fixed by `accept_f27_d_a_mount_kind_the_installation_never_names_is_reported`.)
- **The refusal is boxed.** `AmmunitionRefusal::Divergent(Box<DivergentAmmunition>)`
  keeps the error small for a `Result` this crate returns from hot paths while
  carrying both declarations in full; `Divergence` says which field conflicts so a
  caller can read amounts or mounts without matching on two variants.
- **Two `AmmunitionId` types, as everywhere else.** `cs_content::weapons::
  AmmunitionId` is the declared record's and `cs_sim::weapons::AmmunitionId` the
  runtime's; the lowering boundary (`cs_app::weapons::lower_ammunition`) is the one
  conversion. Neither crate may depend on the other.
- **The retail test reads bytes through `RofSource`, not the session.** A ROF
  member lives inside its container and may be compressed, so the VFS's directory
  read reports `no backing` for it **by design**; the production member reader on
  the `RofSource` is the byte path. The session is still used, to prove the member
  resolves through the mount.
- **Installation discovery is done once.** It hashes every file, so the manifest is
  cached in a `OnceLock` and every span in the test is bound to the same
  installation digest.

## Test sensitivity

The audit's whole value is that it can fail, so each of its decisions has a test
that removes exactly that decision. **Nineteen** mutations were applied to the
three production files, one at a time, restoring the source after each, and each
selected test was re-run **alone with `--exact`**. Every mutation was caught. The
"caught by" column is what the run reported, not an estimate.

| mutation | caught by |
| --- | --- |
| the closure check against the measured surface is dropped | 3/3: `accept_f27_d_a_catalogue_that_under_counts_the_installation_is_incomplete`, `accept_f27_d_an_empty_catalogue_is_incomplete`, `accept_f27_d_retail_the_ammo_audit_reports_every_type_it_cannot_map` |
| `is_complete` ignores the findings | 3/4 — the three above. `accept_f27_d_a_fully_declared_catalogue_is_complete` survives, correctly: with no findings it *is* complete |
| a type with no known amount is never reported as unconsumed | 1/1: `accept_f27_d_a_type_with_no_measured_damage_has_no_damage_consumer` |
| `AmmoDamageConsumer::is_consumed` counts an unknown amount as known | 1/2 — `..._has_no_damage_consumer`. `accept_f27_d_an_empty_catalogue_is_incomplete` is unaffected, correctly |
| `AmmoDamageConsumer::consumer` names a path even for an unmeasured type | 1/2 — `..._has_no_damage_consumer` |
| an unmeasured interaction option is never reported | 1/1: `accept_f27_d_an_unmeasured_interaction_rule_is_named_by_option` |
| an unmeasured caliber is never reported | 1/1: `accept_f27_d_an_unmeasured_caliber_is_reported` |
| an unpaired type is never reported | 1/1: `accept_f27_d_an_unpaired_type_is_reported` |
| `AmmoBehavior` reports every option as applied | 1/1: `accept_f27_d_a_fully_declared_catalogue_is_complete` |
| the audit's rows are not deduplicated by type id | 1/1: `accept_f27_d_a_repeated_type_record_is_audited_once` |
| `covers_group` is widened to claim every measured group | 3/4: `accept_f27_d_every_uncovered_gun_group_is_reported_once_by_name`, `accept_f27_d_a_mount_kind_the_installation_never_names_is_reported`, `accept_f27_d_retail_the_ammo_audit_reports_every_type_it_cannot_map` |
| `covers_group` is narrowed to claim nothing | 4/5 — those three plus `accept_f27_d_a_fully_declared_catalogue_is_complete`, whose synthetic surface is built from the covered groups and so then has none |
| the dangling-reference checks walk `pairings()` instead of `guns()`/`ammunition()` | 1/1: `accept_f27_d_a_dangling_loadout_reference_is_reported_both_ways` |
| the registry overwrites the profile on a divergent registration | 2/2: `accept_f27_d_two_mounts_disagreeing_about_one_type_are_refused_by_name`, `accept_f27_d_two_mounts_disagreeing_about_one_type_are_reported` |
| the registry folds a refused mount in anyway | 4/4: the same two plus `accept_f27_d_two_mounts_agreeing_about_one_type_are_one_row` and `accept_f27_d_two_mounts_of_one_type_audit_to_one_row` |
| `AmmunitionDamageConsumer::amount` returns `Some(0.0)` for a zero channel | 1/2: `accept_f27_d_a_type_with_no_delivering_channel_is_consumed_by_nothing` |
| `AmmunitionDamageConsumer::delivering` counts zero channels | 2/2: `..._consumed_by_nothing` and `accept_f27_d_a_one_channel_profile_consumes_only_that_channel` |
| `session_ammunition_audit` audits only the first registered actor | 1/1: `accept_f27_d_the_session_audit_spans_every_registered_actor` |
| `session_ammunition_audit` ignores a closed session | 1/1: `accept_f27_d_an_empty_or_closed_session_audits_to_nothing` |

Three rows report fewer catches than selected tests, and each is a test that
*should* survive because it does not exercise the removed decision — a complete
catalogue with no findings, an empty catalogue with no rows, and a synthetic
surface built from the covered groups. That is the correct reading of those
mutations, not a gap.

The harness that applied them was a scratch script under the session's temporary
directory, not a committed tool: it rewrites production source in place and
restores it after each run, which is a one-off measurement rather than a test.
Two of its first patches had an anchor that no longer matched after `cargo fmt`
reflowed the source; those two were re-measured with corrected anchors before the
table above was written, and no row above rests on an unmeasured patch.

## Unknowns recorded (not guessed)

- **The names of the four ammunition types.** Four string-id blocks are measured
  and the four descriptions are `3370..=3373`, but the text lives in the engine's
  runtime catalog, reachable only through the executable's callbacks
  (`2027`, `2030`–`2037`, `5053`/`5054`, `5017`/`5018`). Task filed below.
- **Per-type damage amounts and calibers.** Same source. Nothing in the public
  manual establishes an ammunition table either (F27 "Research boundary"), so
  there is no documented fallback to consult.
- **The convergence rule.** Whether the original's paired wing guns' barrels
  meet at all, and where, is unmeasured. `cs_sim::weapons::MountTransform::forward`
  still carries the *resolved* direction and this stage invents no convergence
  geometry, exactly as F27-B left it.
- **The inherited-velocity rule.** `InheritanceRule` is still a declared `Resolved`
  option with no measured value.
- **Which side each of the eleven uncovered gun groups is on, and which airframe
  uses which group.** The per-airframe gun tables are in the executable.
- **`penetration`, `ricochet` and `ammo_switching`.** Still declared, still read by
  no production path, still deferred by
  `cs_content::weapons::InteractionRules::deferred`. **This stage does not resolve
  the deferral**: the original's behavior is unmeasured, so any model would be a
  guess, and F27 non-negotiable 4 forbids a simulator feature game content does not
  support. What this stage *does* change is that the deferral now names F27-D as
  its stage and the audit reports it per type, so it can no longer be forgotten —
  see the follow-up task.
- **The gun count is five; the *set* of five guns is not enumerated.** Same
  runtime-catalog reason.
- **Everything else in F27** stays where F27-A/B/C left it: no original-run
  behavior, no audio or visual verification of the emitted effects, no Avian
  projectile body, no cockpit input device, and `crates/cs_app/src/run.rs` still
  does not schedule `step_weapon_session` (not an owner path for this task).

## Evidence

`private/evidence/F27-D/acceptance.json`, committed as
`docs/findings/evidence/F27-D.json`, validates with
`tools/validate_evidence.py --require-pass`. What it records:

- `capabilities: ["retail", "synthetic"]`, `claim: "implemented"`;
- `tests: {discovered: 41, executed: 41, passed: 41, failed: 0, ignored: 0}` — the 41
  `accept_f27_d_` tests, six of them retail and all run with `--include-ignored`;
- `install_sha256 b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978` and
  `content_sha256 a0223506e512b50c0e0445ba73204a0461e60197e28d58a7f7144632d262c12d`, both
  measured by production `cs_assets::install` discovery, never typed in;
- two hashed artifacts: `cargo-test.log` (the recorded acceptance run) and
  `ammunition-surface.json` — a **second** production pass of the same reader over
  the same installation, carrying each member's decoded length and SHA-256, the
  resource header's `#define` count, the measured gun-group and ammunition-block
  identifier lists, the engine dictionary's identifier names, the screen loop
  bound each count is read from with what it states, and the six fidelity
  limitations under their `f27.d.limit.*` claim ids.

**`unknowns` is empty and that is a claim worth checking.** The validator's
`--require-pass` rejects a nonempty `unknowns` list. The six limitations are
therefore recorded in the four places above the validator does not read for that
field — the report's `review.method`, the hashed artifact, this file, and
follow-up task #545 (F27-E) — rather than removed. They are *unmeasured original
behavior*, not failures of this stage's assertions: every assertion here is a
measured fact about shipped files or a production behavior the suite exercised,
and all of them pass. `claim` is `implemented`, never `verified_original`: what
was verified is what the installation's **files** declare, and `retail` here is
read access, not evidence that the original executable ran.

The reviewer should regenerate the report on the rebased commit and compare it.
The tree hash is in the report and is checked against `HEAD^{tree}` by the
harness itself, so a report from another commit cannot be reused.

## Not claimed

No original-data *verification* of gameplay behavior. This stage measured what
the installation's **files** declare and built the audit that reports everything
else as unknown. It does **not** establish the original's ammunition names,
damage, calibers, convergence or interaction rules, and it does not claim the
audit's declared half has been run against an imported catalogue — it has not,
because there is none yet. `retail` here is read access to original files, not
evidence that the original executable ran. This stage awards at most
**checked**.