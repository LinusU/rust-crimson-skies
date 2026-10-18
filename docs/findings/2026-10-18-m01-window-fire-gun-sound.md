# M01-WINDOW-FIRE-GUN-SOUND: which original sound a gun fires, and why the designed M01 gun stays silent

Task: Rally #1546. Date: 2026-10-18. Capabilities used: `retail`, `build`,
`test`; the decrypted engine image was read for two static addresses (no
disassembly is committed). Nothing here is `verified_original`: no original
executable ran and nothing was heard.

## Result

The sound a gun fires **is bound in the original data, per weapon record** —
the premise of #1546 ("weapons.zrd BALLISTICS records carry no sound field")
was wrong. What is **not** measured is which weapon record the player's guns
carry in M01, so the project's designed M01 gun cannot be bound to a member and
`m01-play-weapons.designed-shot` stays refused by name
(`WeaponEffectRefusal::UnknownAsset`). Nothing is bound by filename.

## (a) The pdevastator record chain has no gun sound field

Through the production reader (`OriginalDocuments::vehicle`), the top-level
keys of `pdevastator` that look like sound/gun/cannon/fire fields are exactly
`engine_sound` and `cockpit_engine_sound`; `player_airplane` (its parent) has
no `sound`/`snd` key. `player_airplane` carries a `weapons` list naming
`wep_00`..`wep_73` etc. — the weapons the airframe *may* carry, not the ones
equipped. Pinned by `accept_m01_window_fire_gun_sound_the_player_airframe_record_names_no_gun_sound`.

## (b) weapons.zrd names the sound group; sounds.zrd names the member

Each `BALLISTICS` record in `weapons.zrd` (member of `ZBD/zrdr.zbd`) carries a
`LOOPED_SOUND_NAME` field. Measured through `mission_session::gun_sound`:

| weapon record(s) | group | member (`sounds.zrd`) |
| --- | --- | --- |
| `wep_00`, `wep_30`..`wep_33`, `wep_130` | `snd_30cal` | `30cal_gun.wav` |
| `wep_01`, `wep_40`..`wep_43`, `wep_140` | `snd_40cal` | `40cal_gun.wav` |
| `wep_02`, `wep_50`..`wep_53`, `wep_150` | `snd_50cal` | `50cal_gun.wav` |
| `wep_03`, `wep_60`..`wep_63`, `wep_160` | `snd_60cal` | `60cal_gun.wav` |
| `wep_70`..`wep_73`, `wep_170` | `snd_70cal` | `70cal_gun.wav` |
| (rocket records `wep_04`, `wep_24`) | none declared | — |

Every one of those groups is flagged `3D LOOPED SFX` in `sounds.zrd`. So the
gun cue is a **loop held while firing**, not a one-shot: the task text's
`PlaybackMode::OneShot` assumption does not match the data. `chaingun.wav`
(`snd_chaingun`) and `turret_gun.wav` (`snd_turretgun`) are referenced by
turret/zeppelin records (`CANNON` blocks), not by these ballistics records.

## (c) The engine reads the field for every weapon record it loads

In the decrypted image (`image_va_base 0x400000`): the string
`LOOPED_SOUND_NAME` is at VA `0x62b380`, referenced once, at `0x4baa7c`, in the
weapon-record loader. After the lookup (`call 0x57a090` with the key) it copies
the value, truncated to `0x13` bytes, into the weapon structure at offset
`+0x24` (`0x4baa93`..`0x4baaa3`), beside the `FIRING_HEAT` read (string
`0x62b35c`, ref `0x4baa3b`). Provenance `static`. An unrelated `cannon_sound`
key string exists at `0x627f4c` (ref `0x47a7fc`); no vehicle record in
`vehicle.zrd` carries it, so its owner is unresolved.

## What is not measured

* **Which weapon records the player's M01 guns carry.** The reader archives
  with `wep_` names are `weapons.zrd`, `vehicle.zrd` (the allowed set) and
  `ai.zrd` (AI loadouts); `ZBD/C3/M01/zrdr.zbd` has none. The player's chosen
  loadout is built by the loadout screens/scripts (F27-D), outside the files
  read here. The designed M01 gun is a ".50 caliber" of the project's design;
  binding it to `wep_02`/`snd_50cal` through that label is the name guess
  F14-D.7 ruled out.
* Which of `soundsl`/`soundsh` the original opens (F14-D.7).
* Audibility: nothing was played (no `human_review`).

## What this changes

`mission_session::gun_sound::{measure_weapon_sound, weapon_sound_from_documents,
sound_like_keys}` is the production reader for the chain above. No catalog
record, load item or loadout id changed, so the windowed shot is still silent.
The follow-up is recorded as a Rally task: measure the player's M01 equipped
weapon records, then author a **looped** declared record for the measured
member and deliver it beside the launch member (`compose.rs`), after
#1304 schedules the presentation consumer.

Tests: `accept_m01_window_fire_gun_sound_*` in
`crates/cs_app/tests/campaign/m01_window_fire.rs` (two synthetic, two
`CS_GAME_DIR`).
