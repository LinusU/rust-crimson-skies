# T452-R.03-C: the ordnance media ids — FIRE/FLYOUT/IMPACT/DESTROY_ANIMATION, and the launch sound

Date: 2026-10-28. Task #2166 (parent #2161). Capabilities used: `retail` (read
access to the installation) and static reading of `$CS_ENGINE_IMAGE` (image
base `0x400000`). Provenance of every engine claim: `static`, never
`verified_original`; the executable was never run, and nothing here asserts
original-run behavior. Offsets are virtual addresses; byte windows are in
`data/owner-static/engine-facts.toml` under `[[ordnance_media_fact]]`, and
the ignored `accept_t452_r03_c_image_*` tests re-measure them.

## 1. The effect blocks are one struct, three call sites

The weapon parser fills every media field through **one shared reader at
`0x5ae990`**, writing a `0x64`-byte block; `wep_` records stride `0x214`
(table iteration `0x5ac108`).

| key | parse site | destination | the block answers |
| --- | --- | --- | --- |
| `FIRE` | `0x5ae0fc` | `weapon + 0x94` | the muzzle presentation |
| `FLYOUT` | `0x5ae10e` | `weapon + 0xf8` | the flying projectile's presentation |
| `IMPACT` | `0x5ae1cf` | rows at `weapon + 0x15c`, stride `0x64` | the detonation presentation, per surface |

`IMPACT` is a keyed list of surface rows, not one block: the parser reads the
`default` child into row 0, then walks the registered soil-name table at
`0x637b10` (six built-in names: `default`, `water`, `seafloor`, `quicksand`,
`lava`, `fire`; missions append soils — retail records declare `enemy`,
`player` and `buildings`). An undeclared soil's row is a verbatim copy of row
0, and a declared row without `ANIMATION_MIN_RANGE` inherits row 0's.

The reader's key-to-offset map (`0x5ae9bf..0x5aebb8` push the key strings;
all names resolved to engine handles):

| offset | key | resolution |
| --- | --- | --- |
| `+0x00` bit0 / bit1 | `RANDOM_ROTATE` / an `int` child of `ANIMATION` | flags |
| `+0x04`, `+0x08` | `ANIMATION` | two name handles via `0x523820`, further entries dropped |
| `+0x14` | `ANIMATION_ATTACHED` | `0x523820` |
| `+0x18` | `MODEL_ANIMATION` | `0x523820` |
| `+0x1c` | `SURFACE_ANIMATION` | `0x523820` |
| `+0x20` | `ANIMATION_MIN_RANGE` | `0x523820` |
| `+0x24` | `EFFECT` | the effect resolver `0x526160` |
| `+0x28` | `MODEL` | the model-table lookup `0x5aec00` |
| `+0x2c`, `+0x30..` | `SOUND` | count, then names via `0x596120` |
| `+0x40`, `+0x44..` | `BOUNCE_SOUND` | count, then names via `0x596120` |

`weapons.zrd` declares no other key inside these blocks.

## 2. What each class maps to at spawn and at impact

- **Spawn (the flying item):** the round allocator `0x5aeca0` attaches
  `FLYOUT>MODEL` (`weapon + 0x120` = block `+0x28`) to the round's scene node
  at `0x5aecbf`, building the animated form first when `MODEL_ANIMATION`
  (`+0x110`) is nonzero. `wep_14`: `a_torpedo` + `torpedo_trail`.
- **Muzzle (one shot):** the fire routine `0x5aef40` reads the launch-sound
  slot `weapon + 0xe8` at `0x5af3c0`; `-1` plays nothing, otherwise entry
  `slot` of `FIRE>SOUND` (`weapon + 0xc4 + 4*slot`) plays through the gate
  `0x593b80` at `0x5af3de`. Then `FIRE>EFFECT` (`+0xb8`) wins over the
  `ANIMATION` slots (`0x5af3e6`); with no effect, slot `weapon + 0xec`
  selects among `FIRE>ANIMATION` handles.
- **Impact (the burst):** per-surface row — `EFFECT` at row `+0x24` played
  via `0x525d90` (`0x5aca01`), `SURFACE_ANIMATION` (`+0x1c`) played
  surface-aligned (`0x5ac9eb`), `ANIMATION` slots (`+0x04/+0x08`) at
  `0x5ac942`, and `0x5ad100` picks one `SOUND` entry uniformly
  (`rand * count >> 15` into `row + 0x30..`) through the same `0x593b80`
  gate. There is **no** single detonation media id — it is always the
  row for the surface hit.
- **End of life:** `DESTROY_ANIMATION` (`0x5adc33` → `weapon + 0x188`) is
  played when a round ends in flight (`0x5af78b`) and is the detonation
  animation fallback when the surface row's `ANIMATION_MIN_RANGE` is absent
  (`0x5b06b8`). `wep_14` → `torpedo_destroy_effect`; no other retail record
  declares it.

## 3. The launch sound, and `SOUND_NAME`

**There is no `SOUND_NAME` key.** The only image occurrence of the string is
the tail of `LOOPED_SOUND_NAME` (`0x62b380`, parsed at `0x4baa7b` into the
mount descriptor's `+0x24` — the gun loop, registered through
`0x59cd40(0x13, name)`). The actual keys are: `FIRE>SOUND` (the launch
one-shot), `FLYOUT>SOUND` (the declared flight loop), `LOOPED_SOUND_NAME`
(the mount's continuous loop).

The launch-sound slot is caller-selected: `0x5ac120(weapon, index)` writes
`weapon + 0xe8` when `index` is signed-below the `FIRE>SOUND` count
(`weapon + 0xc0`); the observed callers pass `0` (first declared entry) or
`-1` (silence). An index at or above the count writes nothing.

So for a record with no `SOUND_NAME` — which is every record, since the key
does not exist — the answer is:

- **the record's own declared `FIRE>SOUND` entry**, which happens to be
  *shared declarations* across the rocket family: `snd_missile_sm`
  (wep_04/05/07/08/09/10/15/25) and `snd_missile_bg`
  (wep_06/11/12/14/24), plus per-record names (`snd_gotshot2`,
  `snd_enemyflak`, `snd_zepcannonfire`). It is shared by declaration, not by
  fallback;
- **not a mount sound**: `LOOPED_SOUND_NAME` belongs to the mount
  descriptor (the gun loop — `snd_30cal` on wep_00 and its turret siblings),
  a different mechanism from the launch one-shot;
- **not a shared default**: records that declare no `FIRE>SOUND` play
  nothing — the gate `0x593b80` early-outs on a null descriptor
  (`0x593ba4`), and the slot selector proves the caller picks a declared
  slot or explicit `-1` silence. The gun records (wep_00..wep_03,
  wep_23/29/30..73) declare only `FIRE>ANIMATION` (`muzzle_burst_*`) plus
  their `LOOPED_SOUND_NAME`; `wep_26` declares neither — a completely silent
  launch.

Retail `weapons.zrd` declares at most one `FIRE>SOUND` per record, so the
slot is index 0 or silence in practice.

`FLYOUT>SOUND` is parsed (wep_14 `snd_torpedo_loop`, wep_25
`snd_bombwhitle`) into `+0x124`/`+0x128`; a definitive consumer in the
weapon module was **not** located statically (a `0x5c2890`-region function
reads those offsets on a record whose class was not proven). Recorded, not
claimed.

## 4. The reader surface and the runtime ids

`cs_content::ordnance` now carries the measurement:
`OriginalEffectBlock` (one block's declared names), `OriginalImpactRow`
(`surface`, `block`), `OriginalWeaponMedia` (`fire`, `flyout`,
`impact_rows`, `destroy_animation`, `looped_sound_name`), the pure parser
`weapon_media(name, body)`, the retail reader `read_original_weapon_media`,
and the pinned VAs (`ORIGINAL_EFFECT_BLOCK_READER_VA`,
`ORIGINAL_WEAPON_*_VA`, `ORIGINAL_SOUND_GATE_VA`,
`ORIGINAL_MOUNT_LOOPED_SOUND_VA`).

For the runtime `OrdnanceMedia` contract —
`cs_app::ordnance::lower_media` lowers `visual` +
`sound` and refuses unknowns — `OriginalWeaponMedia::measured_ordnance_media`
projects the declared names into `DeclaredOrdnanceMedia`:

- `visual` = `hardpoint_equipment/<FLYOUT>MODEL>` — the projectile's own
  declared visual (attached to the round at spawn);
- `sound` = `sound/<FIRE>SOUND[0]>` — the declared launch one-shot;
- `particles` = `None` — no such field exists in the member;
- every name carries `ClaimStatus::ObservedTool` provenance on the caller's
  claim id; a record that declares no `FLYOUT>MODEL` or no `FIRE>SOUND`
  keeps the field `Unknown` with the reason, because the measured behavior
  is silence, not a substitute.

## 5. Limits

- No original run, no audible/visual confirmation: `retail` is file access
  only. Names are measured; that the original played them the way the
  disassembly reads is `static` evidence, not `verified_original`.
- `FLYOUT>SOUND`'s runtime consumer is unproven (above).
- `ANIMATION`'s `int`-child flag (block `+0x00` bit1) is parsed but its
  meaning is unmeasured; no retail record uses it.
- The sound names (`snd_*`, `*_sg`) are logical names in the `sounds.zrd`
  registry (`SETS`/`SOUND_GROUPS`), which this slice does not resolve to
  wave members — a follow-up owns the registry mapping.
- The `IMPACT` surface list is read as declared; the soil-table row index a
  dynamic name lands on depends on mission registration order.
