# T452.04: hardpoint layout, launch speeds, release rates, lifetimes and status effects per component

Date: 2026-10-27. Task: #2130 (T452.04). Spec:
`specs/F28-rockets-special-ordnance-counter-effects-and-nitro.md`
(non-negotiable 1, stage `### F28-D`). Shared contract:
`docs/contracts/FLIGHT-PHYSICS.md`, "Boost and special models". Predecessors:
`docs/findings/2026-10-25-t452-01-weapon-census.md` (the 48-record census and
the family rules this table keys on),
`docs/findings/2026-10-25-t452-02-proximity-fuse-shape-and-arming.md`,
`docs/findings/2026-10-25-t452-03-nitro-activation-capacity-and-tradeoffs.md`,
and `docs/findings/2026-10-03-f28-d-original-ordnance-catalogue.md` (which
measured the eleven selectable types, eight rocket slots, four gun slots and two
hardpoint points, and said the per-component numbers live in the engine image).
Capabilities used: `retail` (read access to `$CS_GAME_DIR`) and ordinary
build/test. **Not** used and not claimed: `$CS_ENGINE_IMAGE` (nothing here needed
it), and any run of the original executable — everything below is what
`weapons.zrd` and the two ordnance screens *declare*.

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/ordnance.rs` (extend): the mount layout
  (`ORIGINAL_SCREENS_CONTAINER`, `ORIGINAL_HARDPOINTS_SCREEN`,
  `ORIGINAL_ORDNANCE_LAYOUT_SCREEN`, `ORIGINAL_{ROCKET,AMMUNITION}_SLOT_ARRAY`,
  `ORIGINAL_HARDPOINT_POINT_ARRAY`, `OriginalSlotArray`,
  `OriginalLayoutFinding`, `OriginalMountLayout`, `read_original_mount_layout`,
  `OriginalScreenError`) and the per-component table (`OriginalMountDeclaration`,
  `OriginalClusterShape`, `OriginalLifetime`, `OriginalAreaDeclaration`,
  `OriginalDamageDeclaration`, `OriginalStatusEffectDeclaration`,
  `OriginalComponentUnknown`, `OriginalComponentLaunch`,
  `OriginalComponentTable`, `original_component_table`,
  `read_original_component_table`, `OriginalComponentError` and the
  `T452_04_*`/`T454`-carried claim ids), plus the module doc.
- `crates/cs_content/tests/accept_t452_04_component_table.rs` (**new**, 14
  tests, module of `tests/main.rs`).
- `crates/cs_content/tests/evidence_report_t452_04.rs` (**new**, evidence
  harness, not an acceptance test).
- `crates/cs_content/tests/main.rs` (wiring: two `mod` declarations).
- This file and `docs/findings/evidence/T452.04.json`.

No protected path, no `Cargo.toml`/`Cargo.lock` change, no original data, no
binary file, no script text committed.

**One observable failure, before the change:** T452.01 could say *which family*
each `wep_` record belongs to, and F28-D could say how many types and slots the
installation declares, but **nothing in the project could answer, per component,
what its record declares or where it mounts**. `OriginalWeaponRecord` kept every
key and value and no production path read `VELOCITY`, `FIRE_RATE`,
`CLUSTER_SIZE`, `DETONATION_TIME`, `IMPACT_PROXIMITY` or an effect key into a
per-component answer; `OriginalOrdnanceSurface` held the slot and point *counts*
as constants and never read a screen; and the glidebomb `wep_25` — an ordnance
record with no `ROCKET` flag — had nowhere to say that no screen states a slot
for it. A catalogue could therefore invent a launch speed, a lifetime, a volley
size or a mount and nothing would contradict it.

## The mount layout, read from the screens themselves

Production `read_original_mount_layout` mounts `GOSDATA/ASSETS/crimson.rof`
through `cs_assets`' ROF mount, resolves and decodes two members through
`cs_formats`, and parses each one's own `object <array>[n]` declaration and the
`for (... < n; ...)` loop that fills it (`OriginalSlotArray::read`). The bound is
therefore stated **twice by the original screen** and the two statements are
compared: `OriginalLayoutFinding::SlotBoundDisagrees` is a real check, not a
restatement, and `OriginalMountLayout::from_screens` refuses a screen that
declares no array.

| screen | array | declaration | fill loop | empty slot |
| --- | --- | --- | --- | --- |
| `ASSETS/SCRIPTS/ORDINANCELAYOUT.SCRIPT` | `RKA` | `object RKA[8]` | `for (BA = 0; BA < 8; BA++)` | `deactivate(RKA[BA])` when the engine answers `-1` |
| `ASSETS/SCRIPTS/ORDINANCELAYOUT.SCRIPT` | `QKA` | `object QKA[4]` | `for (BA = 0; BA < 4; BA++)` | `deactivate(QKA[BA])` likewise |
| `ASSETS/SCRIPTS/HARDPOINTS.SCRIPT` | `DT` | `object DT[2]` | `for (R = 0; R < 2; R++)` | none: both points always show |

So **eight rocket slots, four gun/ammunition slots, two hardpoint points**, each
count with its own second witness inside the same screen, and a behavioural
difference the layout keeps: the ordnance screen hides an empty slot, the
hardpoint screen does not. The measured counts agree with
`cs_content::weapons::ORIGINAL_ROCKET_SLOTS` (8), `ORIGINAL_GUN_SLOTS` (4) and
`ORIGINAL_HARDPOINT_POINTS` (2), and the retail test asserts that agreement so
the two readings cannot drift.

The screens size their controls, not an airframe's contents: **which** slot a
given plane fills is in that plane's record in the engine image, and that stays
unknown — `t452_04.limit.per_airframe_layout`, carried on the table. The one
measured per-slot datum the project has is the fresh-profile default
(`data/owner-static/engine-facts.toml`, `[flight_check_default]`: rocket ids
`1,1,none,none,1,1,none,none` for plane 5), and no file maps a rocket id to a
`wep_` record, so it cannot be joined to this table.

## The per-component table

`read_original_component_table` = the T452.01 census + the layout above.
`original_component_table` is the pure builder: one row per record a T452.01 rule
classifies, and every record no rule reaches listed by name in
`excluded` (32: the thirty-one `CANNON` guns and the cannonball `wep_28`) rather
than lowered or dropped. Every value is the record's own, exactly as stored — no
unit conversion, no rounding — and each field the record does not state is
`None`/`Undeclared` **and** a named claim on the row.

| record | family | mount | `VELOCITY` | `FIRE_RATE` | `CLUSTER_SIZE` | `IMPACT_PROXIMITY` | damage | stated lifetime | effects |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `wep_04` | guided | rocket slot | 450 | 1 | 1 | 25 | armour 25 / health 25 | – | – |
| `wep_05` | guided | rocket slot | 1200 | 1 | 4 | 15 | 60 / 40 | – | – |
| `wep_06` | guided | rocket slot | 1200 | 1 | 3 | 15 | 40 / 60 | – | – |
| `wep_07` | guided | rocket slot | 1200 | 1 | 4 | 100 | 35 / 35 | – | – |
| `wep_08` | guided | rocket slot | 1000 | 1 | 2 | 35 | 0 / 0 | – | `SONIC` |
| `wep_09` | guided | rocket slot | 1000 | 1 | 2 | 450 | `DAMAGE` 0 | – | `FLASH` |
| `wep_10` | guided | rocket slot | 1200 | 0.3 | 4 | 15 | 0 / 0 | – | `BEEPER` 20 s (`TIME`) |
| `wep_11` | guided | rocket slot | 400 | 4 | 4 | 15 | 60 / 60 | – | `BEEPER_SEEKER` |
| `wep_12` | area denial | rocket slot | 1200 | 1 | 2 | – | 0 / 0 | – | `TANGLER` 2 s (`TIME`) |
| `wep_13` | area denial | rocket slot | – | 1 | 6 | – | 0 / 0 | area 8 s | `SMOKE_SCREEN` 8 s (`TIME`) |
| `wep_14` | torpedo | rocket slot | 60 | 1 | 1 | 30 | 200 / 200 | – | – |
| `wep_15` | guided | rocket slot | 1 | 1 | 2 | 500 | `DAMAGE` 0 | **2 s** (`DETONATION_TIME`) | `FLASH` |
| `wep_24` | guided | rocket slot | 1200 | 3 | 7 | 15 | 100 / 100 | – | – |
| `wep_26` | flak | rocket slot | 450 | 3 | 1 | 100 | 0 / 0 | – | – |
| `wep_25` | guided | **unknown** | 450 | 3 | 1 | 155 | 10 / 10 | – | – |
| `wep_27` | guided | rocket slot | 850 | 3 | 1 | 22 | 10 / 10 | – | – |

(`FIRE_RATE` is the writer's f32 promoted to f64, so `wep_10`'s 0.3 is stored as
`0.30000001192092896`; the table rounds for reading, the code does not.)

### What each column is, and what it is not

- **Launch speed** is `VELOCITY`, the first value, exactly as stored. Its unit is
  **not stated by any file**: `t452_04.limit.velocity_unit` names all fifteen
  records that declare one (`wep_13` declares none). F16-E measured the world
  unit is the metre, which makes a metre-per-second reading *consistent* with
  the flak turret's lead solver using `weapon+0x2c` against world-space
  distances — it is not a measurement of the unit, and no number here is
  presented as a speed in m/s.
- **Release rate** is `FIRE_RATE`, the first value, exactly as stored. What it
  delays, and whether it is a rate per second or an interval, is unmeasured
  (`t452_04.limit.release_delay`, all sixteen records), so **no release delay is
  derived from it**. The declared value is what a lowerer will convert once that
  is measured; inventing `1/FIRE_RATE` seconds here would be a guess dressed as
  arithmetic.
- **Lifetime** is only a duration a record states. The member carries **no
  `EXPIRES` key at all** (`accept_t452_04_retail_no_record_declares_a_lifetime_key`
  pins it over all 48 records), and exactly one record states a duration of its
  own: `wep_15`'s `DETONATION_TIME 2`. So fifteen rows are `Undeclared` with
  `t452_04.limit.no_expires_key`. `RANGE` and `VELOCITY` are both declared on
  fourteen of these records, but their quotient is a flight time only under an
  unmeasured unit and unmeasured acceleration (`ACCELERATION 150` on four
  records), so it is not recorded as a lifetime.
- **Area** is `IMPACT_PROXIMITY` in metres (T1318 Q1: no conversion into
  world space, and F16-E's metre), plus the only declared area lifetime: the
  second value of `SMOKE_SCREEN`, 8 s on `wep_13`. Fourteen rows declare a radius
  and no lifetime (`t452_04.limit.area_lifetime`); the two effect records that
  declare no radius (`wep_12`, `wep_13`) raise no area claim at all.
- **Damage** keeps the three channels apart: the `ARMOR_DAMAGE`/`HEALTH_DAMAGE`
  pair, or the single `DAMAGE` of the two flash records (`wep_09`, `wep_15`,
  both 0). A known zero stays a zero; it is not an unknown.
- **Status effects** are the record's own effect keys (`TANGLER`,
  `SMOKE_SCREEN`, `FLASH`, `SONIC`, `BEEPER`, `BEEPER_SEEKER`), each with the
  duration the key carries (`TANGLER` 2 s, `SMOKE_SCREEN` 8 s, `BEEPER` 20 s, all
  with the key's own `TIME` word). **Which gameplay status kind an effect flag
  drives is not stated by any file** (`t452_04.limit.status_effect_kind`, seven
  rows), so no `DeclaredStatusEffectKind` is assigned; and a flag with no
  duration is `t452_04.limit.status_effect_duration` (four rows: `SONIC`,
  `FLASH` twice, `BEEPER_SEEKER`).
- **Cluster** is not always a volley. On all twenty-nine gun records that
  declare it, `CLUSTER_SIZE` **equals `AMMO_LIMIT`** (2800 = 2800, 1200 = 1200,
  ...), so there it is the round count; the two turret guns (`wep_23`, `wep_29`)
  declare neither key. On every ordnance row the two differ and
  `CLUSTER_SIZE` is 1..=7 — the only shape in the member that can be a launch
  volley — so the row reports `Volley { munitions }`, and eleven rows with more
  than one sub-munition carry the carried unknown `t454.limit.swarm`: how many
  the launch fires is declared, what each one does is not.
- **Mount** is the slot kind: a `ROCKET` record is a rocket-slot component (one
  of the eight). `wep_25`'s glidebomb declares `LOCK_ON` and every projectile key
  but **no `ROCKET` flag**, so no screen states a slot for it and the row's
  mount is `Unknown` under `t452_04.limit.rocket_flag_missing`. That is also why
  no ammunition-slot mount exists in this table: the guns never become rows.

## Still unknown (with evidence and affected content)

| unknown | claim id | affects |
| --- | --- | --- |
| which component fills which slot on a given airframe | `t452_04.limit.per_airframe_layout` | every airframe; the numbers are in the plane records in the engine image (task #452), and the fresh-profile rocket ids map to no `wep_` record |
| the unit of `VELOCITY` | `t452_04.limit.velocity_unit` | the fifteen rows that declare one; every launch-speed claim |
| which delay `FIRE_RATE` governs, and in which direction | `t452_04.limit.release_delay` | all sixteen rows; every release-delay claim |
| the projectile lifetime | `t452_04.limit.no_expires_key` | the fifteen rows with no stated duration; the member has no `EXPIRES` key |
| the area's lifetime | `t452_04.limit.area_lifetime` | the fourteen rows that declare a radius and no lifetime |
| a status effect's gameplay kind | `t452_04.limit.status_effect_kind` | the seven rows carrying an effect flag |
| a status effect's duration where the flag states none | `t452_04.limit.status_effect_duration` | `wep_08`, `wep_09`, `wep_11`, `wep_15` |
| what each clustered sub-munition does | `t454.limit.swarm` (carried) | the eleven rows whose `CLUSTER_SIZE` exceeds 1 |
| which slot holds a record with no `ROCKET` flag | `t452_04.limit.rocket_flag_missing` | `wep_25` |

`accept_t452_04_retail_every_unknown_names_the_rows_it_affects` pins each claim
and its row count (14, 15, 16, 1, 4, 7, 15, 11) so this table and the code
cannot drift apart, and every unknown carries a nonempty reason.

## Test sensitivity (measured, one mutation at a time)

Seven mutations were applied to `crates/cs_content/src/ordnance.rs`, one at a
time, the `accept_t452_04_` selection re-run after each, and the file restored
from a byte-identical backup (`shasum d267ae48…`) after every probe:

| mutation | caught by |
| --- | --- |
| `OriginalSlotArray::read` reports the declared bound as the loop bound | `accept_t452_04_a_screen_states_its_slot_array_and_fill_loop` |
| the rocket mount hard-codes 8 instead of the layout's count | `accept_t452_04_the_mount_slot_counts_follow_the_declared_layout` |
| the `CLUSTER_SIZE == AMMO_LIMIT` guard is dropped, so 2800 becomes a volley | `accept_t452_04_a_cluster_size_equal_to_the_ammo_limit_is_not_a_volley` |
| an undeclared lifetime becomes `Declared 0 s` instead of `Undeclared` | `accept_t452_04_a_lifetime_is_only_what_a_record_states` and `accept_t452_04_a_rocket_record_carries_its_declared_launch_values` |
| a record with no `ROCKET` flag is treated as a rocket-slot component | `accept_t452_04_a_record_without_a_rocket_flag_leaves_the_mount_unknown` |
| status flags lose the duration the key carries | `accept_t452_04_a_status_flag_records_the_duration_the_record_states` |
| the `no_expires_key` unknown is no longer recorded | `accept_t452_04_a_lifetime_is_only_what_a_record_states` and `accept_t452_04_a_rocket_record_carries_its_declared_launch_values` |

The retail tests are the second half of the sensitivity: they re-read the owner's
installation through the production readers on every run, so a stale count, a
record that changes family, a new key or a changed value fails rather than
passes.

## Choices worth stating

- **The layout is parsed, not asserted.** The counts come from the screens' own
  text on every run, and each count has two witnesses in the same screen, so
  `is_consistent()` can fail. The `ORIGINAL_*` constants the rest of the project
  carries are cross-checked against the parse instead of replacing it.
- **Unclassified records are named, never folded.** `excluded` carries the 32
  records the family rules do not reach, so "the table is complete for the
  ordnance families" is visible rather than implied, and the guns stay F27's.
- **No unit conversion, no derived speed, no derived lifetime.** Every number is
  the record's own f64; the claims say which reading is unmeasured.
- **A gun's `CLUSTER_SIZE` is ammunition, and the guard is structural.** The
  equality is measured on 29 records, and the row builder refuses to read a
  `CLUSTER_SIZE` equal to `AMMO_LIMIT` as a volley even if a future rule ever
  classified a gun.
- **`wep_25` keeps its unknown mount.** The glidebomb reaches `guided_rocket`
  through `LOCK_ON` and has no `ROCKET` flag; assuming one would fabricate a
  slot the screens do not state.
- **The screens' own behavioural difference is kept.** `deactivates_empty_slots`
  records that the ordnance screen hides an empty slot and the hardpoint screen
  does not — a fact about the two screens, not a layout count.
- **The evidence harness reads the same production function.** Its second
  observation is a re-read of `read_original_component_table`, not a parallel
  implementation, so the artifact cannot describe an installation state the
  acceptance suite never checked.

## Review

**Implementer:** `deepseek-1` (agent, model `deepseek-v4.1-flash`).
**Reviewer:** pending; no independent review yet. This is not evidence of
original behaviour and awards at most **checked**.

## Evidence

`private/evidence/T452.04/acceptance.json`, committed as
`docs/findings/evidence/T452.04.json`, validates with
`tools/validate_evidence.py private/evidence/T452.04/acceptance.json
--artifact-root private/evidence/T452.04 --require-pass`. What it records:

- `capabilities: ["retail", "synthetic"]`, `claim: "implemented"`;
- `tests` — the `accept_t452_04_` selection: 14 tests (9 synthetic, 5 retail),
  all run with `--include-ignored` and `CS_GAME_DIR` set;
- `install_sha256` and `content_sha256`, both measured by production
  `cs_assets::install` discovery, never typed in;
- two hashed artifacts: `cargo-test.log` (the recorded acceptance run) and
  `component-table.json` — a **second** production pass of the same reader over
  the same installation, carrying the layout the screens declare, every row, the
  per-claim row counts and the table-level unknown. Identifiers, numbers and
  digests only: no script text, no display text.

The report's `candidate_tree` names the tree that was measured
(`bdfab3be…`, the commit carrying the code, the tests and this finding). This
section was added in the following commit, so the report is one documentation
commit behind the head it travels with; nothing it measured changed. It should
be regenerated on the reviewed commit, as F28-D's report was.

**`unknowns` is empty and that is a claim worth checking.** The validator's
`--require-pass` rejects a nonempty `unknowns` list. The nine limitations are
therefore recorded in the four places the validator does not read for that field
— the report's `review.method`, the hashed `component-table.json`, the code's
claim ids and this file — rather than removed. They are *unmeasured original
behaviour*, not failures of this stage's assertions: every assertion here is a
measured fact about shipped files or a production reading of them, and all of
them pass. `claim` is `implemented`, never `verified_original`: what was
verified is what the installation's **files** declare, and `retail` here is read
access, not evidence that the original executable ran.

## Not claimed

No original-run behaviour, no audio or visual verification, no flight-model
consumption of any of these numbers, and no statement that the original's
rockets fly at the recorded `VELOCITY`, live for the recorded duration, apply the
recorded status effect or mount in the recorded slot. `retail` here is read
access to original files. What is verified is what the installation's **files**
declare, per component, with every gap named.
