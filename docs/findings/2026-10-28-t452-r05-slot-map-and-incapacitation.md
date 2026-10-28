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
- `crates/cs_content/tests/accept_t452_r05_slot_and_status.rs` (new, 13 tests,
  a module of `tests/main.rs`).
- `crates/cs_content/tests/main.rs` (wiring: one `mod` declaration).
- `data/owner-static/engine-facts.toml`: nine `[[ordnance_slot_fact]]` and
  thirteen `[[exposure_fact]]` rows.
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

## What the production reader now states

| symbol | value |
| --- | --- |
| `ORIGINAL_LOADOUT_BINDER_VA` | `0x443de0` |
| `ORIGINAL_LOADOUT_PRESET_BINDER_VA` | `0x444180` |
| `ORIGINAL_GUN_SLOT_TABLE_VA` / `ORIGINAL_ROCKET_SLOT_TABLE_VA` / `ORIGINAL_SLOT_FLAG_TABLE_VA` | `0x62ae28` / `0x62ae38` / `0x62ae58` |
| `ORIGINAL_GUN_SLOT_COUNT` / `ORIGINAL_ROCKET_SLOT_COUNT` | 4 / 8 |
| `ORIGINAL_PLANE_RECORD_GUN_IDS_OFFSET` / `…_ROCKET_IDS_OFFSET` | `0x88` / `0xa8` |
| `ORIGINAL_SLOT_INSTALL_FUNCTION_VA` | `0x4b24d0` |
| `ORIGINAL_SCRIPT_AMMO_SLOT_WRITE_VA` / `…_ROCKET_FILL_VA` | `0x43e553` / `0x43e56a` |
| `T452_R05_SCRIPT_AMMO_USE` | `t452_r05.limit.script_ammo_use` |
| `T452_R05_STATUS_KIND_FOLLOW_UP` | `T452-R05-STATUS-KIND` |
| `OriginalSlotReach::RocketSlot` / `NoRocketSlot` | the ids that select the record / none, with the script writer named |
| `MeasuredStatusKind::PilotIncapacitation` | kind |
| `MeasuredStatusDuration` | `intensity * 5.0` s, 0.6/1.0 ratios, `FLASH` facing, the `IMPACT_PROXIMITY²` ratio |
| `MeasuredStatusVictims` | player screen/audio, no control loss; other actors mode 4 with six zeroed control words ending at the clock |

## Test sensitivity (measured)

Three mutations were applied to `crates/cs_content/src/ordnance.rs` one at a
time and the `accept_t452_r05_` selection re-run with `--include-ignored`
(13 tests; the file was restored byte-identically each time — shasum
`6696690efad87ac8fe64108d7518a7879c76b3a8`):

| mutation | result | caught by |
| --- | --- | --- |
| an empty rocket-id set is given a synthetic id, so `wep_25` becomes selectable | 3 failed, 10 passed | `accept_t452_r05_wep_25_has_no_plane_record_slot`, `accept_t452_r05_exactly_the_five_spawn_only_records_have_no_slot`, `accept_t452_r05_retail_the_slot_map_and_the_status_match_the_records` |
| the exposure's facing scale is set for both bits | 4 failed, 9 passed | `accept_t452_r05_flash_adds_the_facing_scale_and_sonic_does_not`, `accept_t452_r05_sonic_and_flash_are_a_pilot_incapacitation`, `accept_t452_r05_both_bits_measured_are_the_same_incapacitation`, the retail test |
| the exposure's `runtime_kind` becomes `Some(Marker)` and the claim is dropped | 3 failed, 10 passed | `accept_t452_r05_sonic_and_flash_are_a_pilot_incapacitation`, `accept_t452_r05_both_bits_measured_are_the_same_incapacitation`, the retail test |

The twenty-nine image pins are the second half: a changed VA, a changed
constant or a moved store fails `accept_t452_r05_the_image_pins_the_loadout_map`
or `…_the_image_pins_the_exposure` rather than passing, and
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
campaign (a mission's `ammo` command can install it by index — #2173 measures
that), that a SONIC/FLASH hit in the original hit anything at all, or that the
player's screen effect looks or sounds a particular way (the sound names resolve
to descriptors; no audio was played, no frame was rendered). What is verified is
what the image's **code** does at the cited windows and what the records
**declare**. This awards at most **checked**; no agent review replaces the
owner's human approval.
