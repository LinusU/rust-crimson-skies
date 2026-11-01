# T452-R.05 (#2170): the record-to-slot loadout map and the SONIC/FLASH incapacitation

Split subtask of #2156 (`T452-RUNTIME-REGISTER`). It resolves
`t452_04.limit.rocket_flag_missing` (which slot holds `wep_25`, the `LOCK_ON`
record with no `ROCKET` flag) and closes the runtime question behind
`t452_r02.limit.status_incapacitation` (what `SONIC`/`FLASH` do to the actor
they strike, and which runtime status kind expresses it).

Spec: `specs/F28-rockets-special-ordnance-counter-effects-and-nitro.md`
(non-negotiables 1, 3, 4; stage `### F28-D`). Shared contract:
`docs/contracts/FLIGHT-PHYSICS.md`, "Boost and special models". Predecessors
cited, not repeated: `2026-10-25-t452-01-weapon-census.md`,
`2026-10-25-t452-04-component-hardpoints-launch-speeds-and-lifetimes.md`,
`2026-10-28-t452-r01-ordnance-launch-geometry.md`,
`2026-10-28-t452-r02-ordnance-effects-fuses-and-swarm.md`,
`2026-10-28-t452-r03-rocket-type-map-and-purchase-totals.md`,
`2026-10-19-t1601-status-bit-consumers.md`.

Capabilities used: `retail` (read access to `$CS_GAME_DIR`) and the
owner-supplied decrypted engine image `$CS_ENGINE_IMAGE` (image base `0x400000`,
sha-256 `43540fc97347210d…`). **Not** used and not claimed: any run of the
original executable, any GPU, audio or human observation. Every statement below
is what the image's code *does*; nothing is `verified_original`. The byte
windows and tables are committed only as the owner exception of AGENTS rule 3
(`data/owner-static/engine-facts.toml`); no disassembly, no script text and no
original display text is reproduced.

## Files touched

- `crates/cs_content/src/ordnance.rs` (extend): the loadout-map constants,
  `OriginalSlotReach`, `OriginalSlotMeasurement`, `measure_loadout_slot`,
  `measure_loadout_slots`, `original_weapon_record_index`,
  `original_rocket_ids_for_weapon_index`; the status vocabulary
  `MeasuredStatusKind`, `MeasuredStatusDuration`, `MeasuredPlayerExposure`,
  `MeasuredActorIncapacitation`, `MeasuredStatusVictims`,
  `OriginalStatusMeasurement`, `measure_status_effects`, and the claim ids.
- `crates/cs_content/tests/accept_t452_r05_slot_and_status.rs` (new, 15 tests,
  a module of `tests/main.rs`).
- `crates/cs_content/tests/main.rs` (wiring: one `mod` declaration).
- `data/owner-static/engine-facts.toml`: ten `[[ordnance_slot_fact]]` and
  fourteen `[[exposure_fact]]` rows.
- This file and `docs/findings/evidence/T452-R05.json`.

No protected path, no `Cargo.toml`/`Cargo.lock` change, no original data, no
binary file.

**One observable failure, before the change:** the T452.04 table could say that
`wep_25`'s mount was unknown and the T452-R.02 layer could say that the
SONIC/FLASH bit was "an exposure with `runtime_kind: None`", but nothing could
answer **what a rocket slot actually holds**, **whether any airframe can select
`wep_25` at all**, **what the exposure's ratio is measured over**, **who the
status affects and for how long the mode lasts**, or **whether any runtime
status kind can express it**. `t452_04.limit.rocket_flag_missing` and
`t452_r02.limit.status_incapacitation` were named claims with no measured value
or decision behind them.

## Part 1 — the record-to-slot loadout map

### The binder: a slot holds a rocket *type id*, never a record

The loadout binder `0x443de0` takes the local plane record (the `0x198`-byte
airframe record the loadout screen edits) and fills one twelve-entry table:

| table | address | entries | source |
| --- | --- | --- | --- |
| gun slots | `0x62ae28` | 4 | plane record `+0x88` through the gun mapper `0x443d70` |
| rocket slots | `0x62ae38` | 8 | plane record `+0xa8` through the rocket id mapper `0x4440f0` |
| gun enable flags | `0x62ae58` | 4 bytes | plane record flags |

The rocket loop is `0x443f08`..`0x443f2c` (`es=0x62ae38`, `lea edi,[edx+0xa8]`,
`mov eax,[edi]; push eax; call 0x4440f0; mov [esi],eax`, bound
`cmp esi,0x62ae58`) and the installer is called at `0x443f35`. **Each of the
eight slots holds the *record index* the mapper returned for the plane record's
rocket type id; the plane record itself holds type ids.**

`0x4440f0` is the map T452-R.03-A already pinned: `cmp ecx,0xa; ja 0x444144`
answers `-1` for any id above ten (the "none" id 11 included) and its jump table
at `0x444148` answers one of `5, 6, 7, 8, 9, 15, 13, 12, 10, 11, 14`
(`wep_05`..`wep_15`). So **the reachable slot contents are exactly the record
indices 5..=15, or nothing.**

### An index is the record's own name

The slot's installer turns an index into a record through `0x4bad90`, which
formats the engine's own `wep_%02d` string (`0x62b394`, pushed at `0x4bada7`,
fact `slot.record_lookup`) with the index and answers the record from the global
record list (`0x5abfd0`, `0x4badb8`). **The index a slot holds is therefore
exactly the number in the record's `wep_` name** — `25` is `wep_25`, `5` is
`wep_05` — so "no id maps to index 25" and "no slot can hold `wep_25`" are the
same statement. The acceptance test
`accept_t452_r05_a_slot_names_its_record_by_its_index` checks the round trip for
every classified record, and the ignored image test reads both the lookup head
and the format string.

### The other writers do not widen it

| writer | site | what it writes | does it widen the set? |
| --- | --- | --- | --- |
| preset binder (`0x444180`) | `0x444269`..`0x444284` | the preset's eight rocket ids from the global array `0x643078`, each through `0x4440f0` | no — same mapper |
| two literal loadout templates | `0x46776c`, `0x47e7d4` | indices `5`, `5`, `6`, `6` / `6`, `6` and `-1` for the rest | no — inside 5..=15 |
| saved-loadout restore | `0x472887` | the saved slot table's own indices, replayed through `0x4b2550` | no — it can only replay what a slot held |
| the mission command at `0x43e530` | `0x43e553`, `0x43e56a` | a raw record index from the script argument reader `0x5b7980` (no range check) into one slot or into all eight rocket slots | **yes** — see below |

The installer `0x4b24d0` walks the whole table: slots `0..=3` are the gun slots,
`4..=11` the eight rocket slots (`0x4b250b`: `cmp esi,0xc`; `0x4b2571`:
`cmp esi,4`). It installs each entry through `0x4b2550(entity, slot, index, -1,
flag)` and clears the table entry afterwards.

### The one writer that can name an arbitrary record: the mission command

The command handler `0x43e530` compares a five-byte command name at `0x622ee0`
and then either (a) `ammo <slot> <index>`: `0x43e553` stores the raw index into
`slot_table[slot]`, or (b) `ammo <index>`: `0x43e56a` fills all eight rocket
slots with the raw index (`mov ecx,8; mov edi,0x62ae38; rep stosd`). Both then
call the installer. The argument reader `0x5b7980` applies no range check, so a
mission script can name **any** record index, `wep_25` included.

That is not a slot assignment: it installs a record at a slot *the mission
names*, for that mission. Whether any retail mission issues the command with one
of the five indices no airframe can select is **script content**, not record
content, and no script was read for this task. It stays a named claim,
`t452_r05.limit.script_ammo_use`, with follow-up task `T452-R05-AMMO-SCRIPT`
(#2173).

**That follow-up has since landed and answered it** (its finding is committed on
`main` as `2026-10-28-t452-r05-ammo-command-census.md`, task #2173): it decodes
`ZBD/interp.zbd` (98 scripts, 5083 lines) and finds **zero** `ammo` commands, so
no retail script installs any record index. The claim stays named here because
*this* task measured records and code, not scripts; the value behind it is "no
retail script uses the writer", which the census owns and evidences.

### `wep_25`'s slot: none

`wep_25` is record index 25. No rocket type id maps to index 25, so **no plane
record's rocket slot can hold `wep_25`, on any airframe, through any loadout
path**. The five classified records in that position are exactly `wep_04`,
`wep_24`, `wep_25`, `wep_26` and `wep_27` — the set T452-R.03-A called
"spawn-only"; this task measures *why*: the slot holds a type id, and the type
table has eleven entries.

`T452_R05` therefore resolves `t452_04.limit.rocket_flag_missing` with the value
"no slot", not "unknown". Consistent with T452-R.02's pattern, the T452.04 row
(`OriginalComponentLaunch::mount`) keeps its pre-existing `Unknown` entry — its
own acceptance test pins that — and the new measured layer records the
resolution (`OriginalSlotMeasurement::resolved`) for the runtime-registration
task (#2156) to consume.

## Part 2 — what `SONIC`/`FLASH` do to the target

### The path

| step | site | measured |
| --- | --- | --- |
| parse | `0x4ba734`, `0x4ba74c` | `SONIC` sets flag `0x200`, `FLASH` flag `0x800`, on the weapon's `0x38`-byte effect object (`weapon + 0x210`) |
| per-hit test | `0x4b9d84` | `test ch, 0x0a` — **SONIC or FLASH** — jumps to `0x4ba564` |
| exposure entry | `0x42e84a` | returns `false` at once for a victim whose `+0x91d` byte is set |
| ratio | `0x42e85b` | `arg[0x18] / arg[0x1c]` |
| falloff | `0x42e87e` | `intensity = clamp(1 - 2.5 * (ratio - 0.6), 0, 1)`; full for `ratio <= 0.6` (`0x60411c` = 0.6, `0x6040a8` = 2.5) |
| FLASH facing | `0x42e8ed`..`0x42e966` | `FLASH` only: intensity `*= clamp(2 * dot(victim_forward, victim_pos - source_pos), 0, 1)`; a hit from behind does nothing, past half the facing dot is full |
| duration | `0x42e9b1` | out-duration = `intensity * 5.0` s (`0x6036bc`); the second out-value is the raw `0..1` intensity |
| consumption | `0x4ba632` | the shot is consumed: both result floats are zeroed |

**The ratio is measured over the record's own blast radius, and the numerator is
a squared distance.** The caller loads `weapon + 0x3c`, squares it (`0x4ba590`:
`fld [edi+0x3c]; fmul st(1)`), and passes it as the denominator
(`0x4ba5a6`). `+0x3c` is `IMPACT_PROXIMITY`, parsed at `0x5add6a` with its square
at `+0x40`. The numerator is the per-hit measure two callers build with
`0x538880` (the sum of squared component differences): the tangler tick compares
it with the field's `RADIUS²` and passes it on (`0x4b95db`..`0x4b9606`), and the
second hit path `0x4b9770` stores it in `[ebp-0x18]` and hands it to the per-hit
routine (`0x4b97ac`). So the ratio is `(d / radius)²`: full intensity out to
`d ≈ 0.775·radius`, falling to zero at `d = radius`. For the three records that
declare an effect bit: `wep_08` 35 m, `wep_09` 450 m, `wep_15` 500 m.

### Who is affected

| victim | branch | measured behavior |
| --- | --- | --- |
| the player (`0x71c298`) | `0x4ba5bf` → `0x4ba5c7` | a **screen/audio exposure only**: the intensity and duration go to `0x42e9d0`, `SONIC` selects the `(1,0,0)` parameters and its sound name, `FLASH` the `(1,1,1)` parameters and its own sound name. The player's controls are **not** zeroed (`0x4ba625` rejoins the common tail) |
| any other actor | `0x4ba627` → `0x4200d0` | **pilot incapacitation**: mode word `+0x358 = 4`, six control words zeroed (`+0x100`, `+0x108`, `+0x10c`, `+0x114`, `+0x11c`, `+0x120`), and `clock + duration` stored at `+0xc0` |

The other-actor path is gated at `0x4200da`..`0x42010f`: the victim must not be
the player, `+0x91d` must be clear, `+0xf8` must be clear, and `+0x67c` must be
`0` or `4`. The effect ends at the measured recovery site `0x489acd`: while
`+0x358 == 4` the actor update loads the shared clock `0x71c470`, compares it
with `+0xc0` (`0x489adc`) and clears mode `4` once the clock has reached it
(`0x489ae9`).

So the kind is **pilot incapacitation**: not damage (the records declare 0
damage and no damage channel is reached), not engine-output loss (a choke), not
a flight-model stall, and not a marker. The duration is `intensity * 5.0` s,
bounded below by the ratio and (for `FLASH`) by the victim's facing.

### The runtime decision

`cs_sim::weapons::ordnance::StatusEffectKind` has exactly four variants —
`Damage`, `Choke`, `Stall`, `Marker` — and none of them is pilot
incapacitation. No variant is forced: the claim `t452_r02.limit.status_incapacitation`
is kept on the measured row and the missing variant plus its application path is
filed as follow-up **`T452-R05-STATUS-KIND` (#2172)**, which depends on #2156.
The duration side stays resolved: `t452_04.limit.status_effect_duration` is
resolved for every SONIC/FLASH row (the formula above).

## Addendum (#2252): the `FLASH` facing offset is normalised, and the exposure has a caller

Task #2252 (follow-up of #2172 and #2222) closed the one question #2172 left
open and wired the measured exposure into the application. Static analysis
only, same image and same caveats; the original executable was not run.

### The facing term reads a **unit** victim-minus-source offset

| step | site | measured |
| --- | --- | --- |
| the offset | `0x42e8d3`..`0x42e8ea` | `victim.position - source.position`, three floats at `[ebp-0x18]` |
| normalisation | `0x42e8ed`..`0x42e8f6` | `lea eax,[ebp-0x18]; push eax; call 0x422690; fstp st(0)` |
| the helper | `0x422690` | computes `sqrt(x²+y²+z²)` (`0x4226b0`), stores it at `[ebp-4]`, tests its mantissa (`0x4226b5`) and, unless it is zero, divides the vector in place: `fld1; fdivrp st(1)` (`0x4226be`) then three `fmul`/`fstp` pairs (`0x4226c2`..`0x4226d9`). It returns the length |
| the dot | `0x42e8f8`..`0x42e93a` | the victim's own forward axis (`[esi+0x198]`) dotted with the (now unit) offset; `fstp [ebp+8]` keeps the scalar |
| the clamp | `0x42e94e` | `fcom [0x6032e0]` = `0.5`; at or above it the intensity is unchanged, below it `intensity *= 2 * dot` (`0x42e95e`..`0x42e966`) |
| behind | `0x42e940`..`0x42e94c` | `dot < 0` returns `false` before anything is written: a hit from behind does nothing at all |

**So the offset is normalised.** `0x422690` divides the vector by its length in
place and the caller discards the returned length; the dot is therefore
`clamp(dot(unit(victim_pos - source_pos), victim_forward), 0, 1)` scaled by two —
which is the `clamp(2 * facing_dot, 0, 1)` #2172 stated, with `facing_dot` over
the **unit** offset, not the raw one. A zero-length offset is the one case the
helper leaves unnormalised (the mantissa test at `0x4226b5` skips the division),
and its dot is then `0.0`, which the routine reads as "no effect".

#2172's `ExposureGeometry::facing_dot` now documents that meaning and
`ExposureGeometry::from_hit` is the one builder that applies it
(`crates/cs_sim/src/weapons/incapacitation.rs`).

### The facing flag is the `FLASH` bit, so a record declaring both is faced

The per-hit caller loads the weapon's `0x38`-byte effect object
(`weapon+0x210`), its flags dword, shifts right by eleven and masks one
(`0x4ba576`: `shr ecx,0xb`, `0x4ba579`: `and cl,1`) and passes the result as
the routine's first argument. `0x800 >> 11 == 1` is `FLASH`; `SONIC`'s
`0x200 >> 11 == 0`. The exposure runs **once** per hit, not once per declared
bit, so a record that declares both bits — `wep_09` does — is faced.

### What now consumes it

| piece | consumer |
| --- | --- |
| `measured_exposure_bit(row)` | the production producer: a measured row's `SONIC`/`FLASH` effect keys become one `ExposureBit`; `wep_08` is `Sonic`, `wep_09` and `wep_15` are `Flash`, and no other classified record carries one |
| `OrdnanceSession::register_exposure` | the session declaration, the same producer shape `register_tangler_field` has: the lowered component deliberately declares no fixed-duration status for an exposure record, so the bit is registered beside the loadout |
| `step_ordnance_session`'s blast path | for a triggered (or guidance-detonation) item with a registered bit, every supplied `BlastRecipient` is measured with `ExposureGeometry::from_hit` from the burst point, the record's `IMPACT_PROXIMITY` radius and the victim's forward axis, and applied with `apply_exposure` |
| `OrdnanceSession::with_player` | which recipient is `0x71c298`; the ledger never receives an incapacitation for it |
| `playtest::scene::overlay_incapacitation` | the flight input: `V+0x358` is `4` and the six control words are zeroed through `effective_controls` while the mode is live, so the ground-blow gate (`0x48c22b`) and the controls are both suppressed until the whole-tick deadline |

A refusal is named, never guessed: `BlastExposureRefusal::MissingRecipients`,
`MissingRadius` (a record with no `IMPACT_PROXIMITY` has no ratio to measure
over) and `MissingFacing` (a `FLASH` victim with no forward axis).

### What is still not done (affected content, resolving task)

* **The player's screen and sound.** The measured player branch (`0x4ba5bf` ->
  `0x42e9d0`, with `SONIC` selecting the `(1,0,0)` parameters and its own sound
  name and `FLASH` the `(1,1,1)` ones) has no presentation consumer yet: the
  step reports `ExposureOutcome::PlayerScreenAudio` with the measured intensity
  and duration, and nothing draws or plays it. No retail mission can reach it
  either, because the records that carry the bit are not bound to a loadout yet
  (F28-D); when they are, the consumer is the follow-up.
* **The victim gate's other words.** `0x4200d0` also tests `+0x91d`, `+0xf8`
  and `+0x67c`; this runtime models none of the three, so the exposure applies
  with "not exempt, not busy, state word zero" and the gate is stated that way
  in `apply_blast_exposure`. Nothing in the session sets those words, and a
  future feature that does must extend `IncapacitationVictim` rather than
  leave this silently wrong.
* **Production recipients.** `cs_app`'s mission ordnance step still passes
  `blast_recipients: None` and the M01 rocket is a designed record with no
  exposure bit, so no mission fires an exposure yet. The path is complete and
  exercised through `step_ordnance_session` with producer-supplied recipients;
  the recipient producer for a retail loadout belongs with the F28-D binding
  that gives a mission a record carrying the bit.

### Test sensitivity (measured)

Three mutations were applied to the tree one at a time and the
`accept_incap_hitpath_` selection re-run (5 unignored tests; the file was
restored byte-identically each time):

| mutation | result | caught by |
| --- | --- | --- |
| `from_hit` dots the raw offset instead of the unit one | 1 failed | `accept_incap_hitpath_flash_scales_by_the_normalised_facing` (intensity 1.0 where the measured `0.5` is) |
| the status word is left at `0` instead of the ledger's mode | 1 failed | `accept_incap_hitpath_a_sonic_hit_incapacitates_an_ai_until_the_measured_deadline` |
| the blast path applies no exposure at all | 5 failed | every unignored test in the selection |

The two ignored tests are the second half: the retail one re-reads the owner's
`weapons.zrd` through the production table reader and fails if `wep_08` is not
`Sonic` or `wep_09`/`wep_15` are not `Flash`, and the image one fails if any of
the six byte windows or the three constants above moves.

### Files

- `crates/cs_sim/src/weapons/incapacitation.rs`: the normalisation statement,
  and `ExposureGeometry::from_hit` (the one builder of the geometry).
- `crates/cs_sim/src/weapons/ordnance.rs`: `BlastRecipient::with_facing`,
  `OrdnanceRuntime::apply_exposure`.
- `crates/cs_app/src/ordnance.rs`: `measured_exposure_bit`,
  `OrdnanceSession::register_exposure` / `exposure_for` / `exposure_ids` /
  `with_player`, `apply_blast_exposure` on the trigger and guidance-loss paths,
  and the `BlastExposure` / `BlastExposureRefusal` reports.
- `crates/cs_app/src/playtest/scene.rs`: `overlay_incapacitation`;
  `crates/cs_app/src/playtest/mod.rs` calls it from the fixed-tick driver.
- `crates/cs_app/src/mission_session/weapons.rs`: names the player on the
  session.
- `crates/cs_app/tests/accept_incap_hitpath.rs` (new, 7 tests, a module of
  `suite_a.rs`).
- No protected path, no `Cargo.toml`/`Cargo.lock` change, no original data, no
  binary file.

## What the production reader now states

| symbol | value |
| --- | --- |
| `ORIGINAL_LOADOUT_BINDER_VA` | `0x443de0` |
| `ORIGINAL_LOADOUT_PRESET_BINDER_VA` | `0x444180` |
| `ORIGINAL_GUN_SLOT_TABLE_VA` / `ORIGINAL_ROCKET_SLOT_TABLE_VA` / `ORIGINAL_SLOT_FLAG_TABLE_VA` | `0x62ae28` / `0x62ae38` / `0x62ae58` |
| `ORIGINAL_GUN_SLOT_COUNT` / `ORIGINAL_ROCKET_SLOT_COUNT` | 4 / 8 |
| `ORIGINAL_PLANE_RECORD_GUN_IDS_OFFSET` / `…_ROCKET_IDS_OFFSET` | `0x88` / `0xa8` |
| `ORIGINAL_SLOT_INSTALL_FUNCTION_VA` | `0x4b24d0` |
| `ORIGINAL_SLOT_RECORD_LOOKUP_VA` / `ORIGINAL_SLOT_RECORD_NAME_FORMAT_VA` | `0x4bad90` / `0x62b394` (`wep_%02d`, so an index is the record's name number) |
| `ORIGINAL_SCRIPT_AMMO_SLOT_WRITE_VA` / `…_ROCKET_FILL_VA` | `0x43e553` / `0x43e56a` |
| `T452_R05_SCRIPT_AMMO_USE` | `t452_r05.limit.script_ammo_use` |
| `T452_R05_STATUS_KIND_FOLLOW_UP` | `T452-R05-STATUS-KIND` |
| `OriginalSlotReach::RocketSlot` / `NoRocketSlot` | the ids that select the record / none, with the script writer named |
| `MeasuredStatusKind::PilotIncapacitation` | kind |
| `MeasuredStatusDuration` | `intensity * 5.0` s, 0.6/1.0 ratios, `FLASH` facing, the `IMPACT_PROXIMITY²` ratio |
| `MeasuredStatusVictims` | player screen/audio, no control loss; other actors mode 4 with six zeroed control words ending at the clock |

## Test sensitivity (measured)

Four mutations were applied to `crates/cs_content/src/ordnance.rs` one at a
time and the `accept_t452_r05_` selection re-run with `--include-ignored`
(15 tests; the file was restored byte-identically each time — shasum
`973db6a73322d7ff8b0328e4d1d931f175a71a2b4551d388ba3f155220b2f10c`). All four
were re-run on the rebased, review-fixed tree with the same counts as tabled.

| mutation | result | caught by |
| --- | --- | --- |
| an empty rocket-id set is given a synthetic id, so `wep_25` becomes selectable | 5 failed, 10 passed | `accept_t452_r05_a_slot_holds_a_rocket_type_not_a_record`, `…_a_slot_names_its_record_by_its_index`, `…_exactly_the_five_spawn_only_records_have_no_slot`, `…_wep_25_has_no_plane_record_slot`, the retail test |
| the exposure's facing scale is set for both bits | 4 failed, 11 passed | `accept_t452_r05_flash_adds_the_facing_scale_and_sonic_does_not`, `…_sonic_and_flash_are_a_pilot_incapacitation`, `…_both_bits_measured_are_the_same_incapacitation`, the retail test |
| the exposure's `runtime_kind` becomes `Some(Marker)` and the claim is dropped | 3 failed, 12 passed | `accept_t452_r05_sonic_and_flash_are_a_pilot_incapacitation`, `…_both_bits_measured_are_the_same_incapacitation`, the retail test |
| the record index is read as the last digit of the name (`wep_25` -> 5) | 6 failed, 9 passed | `accept_t452_r05_the_record_index_is_the_number_in_its_name`, `…_a_slot_names_its_record_by_its_index`, `…_a_selectable_record_names_the_ids_that_carry_it`, `…_exactly_the_five_spawn_only_records_have_no_slot`, `…_wep_25_has_no_plane_record_slot`, the retail test |

The image pins are the second half: the two pin tables carry thirty-three byte
windows (fourteen loadout, nineteen exposure), and the two image tests read the
id mapper's head, the slot lookup's head, the `wep_%02d` format string, five
constants and the recovery site besides, so a changed VA, a changed constant or
a moved store fails `accept_t452_r05_the_image_pins_the_loadout_map` or
`…_the_image_pins_the_exposure` rather than passing, and
`accept_t452_r05_the_committed_rows_match_the_engine_image` re-reads every
committed row from the image. The retail test re-reads the owner's
`weapons.zrd` through the production table reader, so a record that gains an
effect bit, loses `IMPACT_PROXIMITY` or changes family fails the measured
expectations.

## Evidence

`private/evidence/T452-R05/acceptance.json`, committed as
`docs/findings/evidence/T452-R05.json`, validates with
`python3 tools/validate_evidence.py private/evidence/T452-R05/acceptance.json
--artifact-root private/evidence/T452-R05 --require-pass`. It records
`capabilities: ["retail", "synthetic"]`, the recorded acceptance log, the image
digest, the installation digests measured by `cs_assets::install` discovery and
this task's claim.

## Not claimed

No original-run behavior; `retail` here is read access to original files and
static analysis of the owner's decrypted image, never evidence that the original
executable ran. No statement that `wep_25` never appears in the original
campaign from record content alone — a mission's `ammo` command can install it by
index, and the landed #2173 census found no retail script issues that command, so
no retail mission installs any of the five by index either — that a SONIC/FLASH
hit in the original hit anything at all, or that the player's screen effect looks
or sounds a particular way (the sound names resolve to descriptors; no audio was
played, no frame was rendered). What is verified is what the image's **code**
does at the cited windows and what the records **declare**. This awards at most
**checked**; no agent review replaces the owner's human approval.
