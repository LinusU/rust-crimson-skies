# M01-WINDOW-FIRE-GUN-LOADOUT: which weapon records the player's M01 guns carry, and the looped sound that answers

Task: Rally #1628 (depends on #1546 M01-WINDOW-FIRE-GUN-SOUND, which measured
the `LOOPED_SOUND_NAME` record chain). Date: 2026-10-19. Capabilities used:
`retail`, `build`, `test`; the owner-supplied decrypted engine image
(`$CS_ENGINE_IMAGE`, SHA-256
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`, image VA
base `0x400000`) was read for static analysis only — no disassembly or
decompiled code is committed. Nothing here is `verified_original`: no original
executable ran and nothing was heard. The committed facts sit in
`data/owner-static/engine-facts.toml` under claim `OWNER-STATIC-2026-10-19`,
provenance `static`.

## Result

The player's M01 guns are **`wep_50`, `wep_40` and `wep_30`** — one .50, one
.40 and one .30 caliber record on the fresh-campaign Devastator, fourth slot
empty. The engine's own `(gun, ammunition) -> weapon index` map answers them
from the plane-record fields #1670 already measured; the selector keeps the
first slot as the object's current gun, so the member the mission declares
and delivers is `wep_50`'s `LOOPED_SOUND_NAME` chain: `snd_50cal` →
`50cal_gun.wav`, flagged `3D LOOPED SFX`. The declared record rides the
`Weapons` bus in loop mode — the bus and level are designed, the loop is
measured (`ObservedTool`), and the record's provenance says so. The
designed M01 loadout's guns now name that delivered member's content id; the
retired designed spelling `m01-play-weapons.designed-shot` is gone.

## (a) The player's gun and ammunition ids were already measured

`[[flight_check_default]]` (Rally #1670, finding
`2026-10-19-m01-lc-fc-b-flight-check-gun-and-rocket-lists.md`) recorded the
fresh-campaign roster records the Flight Check copies into the live
`0x64b78c` record array — stride `0xcc`, pilot cell `0x64b67c`, wingman cell
`0x64b680`. Both records carry:

```
pair_mask = 7          gun = [2, 1, 0, 5]      ammo = [0, 0, 0, 4]
```

gun id `5` is the no-gun sentinel (`gun_none`), ammunition kind `4` the
none kind. The pair mask sets the paired display bits, not the binder's
mount bits (below).

## (b) The engine's (gun, ammunition) -> `wep_NN` map

Three sites in the image carry the whole chain (all VAs in the `0x400000`
image, claim `OWNER-STATIC-2026-10-19`, provenance `static`):

* **`0x443de0`** (committed `bind_va`) reads a live plane record's
  pair-mask byte (`+0x84`), gun ids (`+0x88`), ammunition kinds (`+0x98`)
  and rocket ids (`+0xa8`) — the same field offsets
  `[flight_check_layout]` names — and walks the four gun slots, calling
  the map below per `(gun, ammo)` pair. When the mode cell `0x71bb80`
  reads 1 the record it reads is the array row the selection cell points
  at — `0x64b78c + cell * 0xcc` — so the campaign's live pilot record is
  the very record (a) lists.
* **`0x443d70`** (committed `map_va`) is the map: a jump table
  (`0x443dc4`) on `gun` for ids 0..4 answers the bases **30, 40, 50, 60,
  70**; `gun` above 4 or `ammo` equal to 4 answers the no-record value;
  ammunition kinds 1..=3 add their value to the base, every other kind
  adds 0. So `index = {30,40,50,60,70}[gun] + ammo`.
* **`0x4aab00`** (committed `name_va`) formats the index as `wep_%2d`
  (format string `0x629d70`) and resolves the record through `0x5abfd0`
  onto a mount emitter when bit `(4 + slot)` of the pair mask is set —
  which the fresh-campaign mask `7` never sets, so every slot's index
  stays in the object's gun list (`0x62ae28`) instead.
* **`0x4b2350`** (committed `select_va`) fills a `wep_??` template
  (`0x62ae5c`, tens digit at `+4`, units at `+5`) per slot, asks
  `0x5abfd0` whether the record resolves, and stores the first slot that
  answers as the object's current gun.

Applied to the measured record:

| slot | gun | ammo | index | record | group | member |
| --- | --- | --- | --- | --- | --- | --- |
| 0 | 2 | 0 | 50 | `wep_50` | `snd_50cal` | `50cal_gun.wav` |
| 1 | 1 | 0 | 40 | `wep_40` | `snd_40cal` | `40cal_gun.wav` |
| 2 | 0 | 0 | 30 | `wep_30` | `snd_30cal` | `30cal_gun.wav` |
| 3 | 5 | 4 | none | — | — | — |

The selector keeps slot 0, so the player's first pull fires `wep_50` and
the delivered member is **`50cal_gun.wav`**. The other two members are
measured too (`gun_sound::GunSound::weapons` carries all three records in
slot order); a gun-switch model is a later task.

## (c) What the implementation does with it

* `mission_session::gun_sound::measure_gun_sound` walks the pilot default
  record's slots through the committed map and measures each record's
  `LOOPED_SOUND_NAME` chain through the production readers — never a
  filename guess. It refuses a non-`LOOPED` selected group and a selected
  member that is not `50cal_gun.wav`, verifies the member is really inside
  the launch sound's container through the sound-archive reader, and takes
  the container's digest from the mission's declared sound archives.
* `mission_session::compose` carries it as `MissionStage::gun_sound`,
  announces it as a second `LoadItem` on the same staged `ZBD/soundsh.zbd`
  (one read per item, one container open per decode pass), and declares
  its record beside the launch record in the one installed catalog
  (`install_launch_audio`'s `extra` slot).
* `GunSound::record()` declares the member on `AudioBus::Weapons` with
  `PlaybackMode::Loop`. The loop mode's provenance is
  `ClaimStatus::ObservedTool` (the `sounds.zrd` `LOOPED` flag is measured);
  the bus, the `1.0` level and the record provenance are
  `Provenance::designed` (`m01-window-fire-gun-loadout.gun-loop-mix`) —
  which archive feeds the member is the same unmeasured choice #1313
  labelled.
* `weapons.rs`'s designed guns name `measured_gun_sound_id()` — the
  member-derived id — as their shot sound, so the session's fire effect
  and the delivered member are one binding. The loadout itself (mounts,
  caliber text, rates, ammunition, ordnance) stays `Origin::Designed`.

## Scope of the measurement

* The measured records are the **fresh-campaign** defaults the Flight Check
  copies into the live record array when the mode cell `0x71bb80` reads 1 —
  the pilot's and the wingman's records alike. A saved campaign's persisted
  loadout was not read; nothing about saved games is claimed.
* `wep_50`'s member is delivered because the selector keeps slot 0; whether
  the player can select slots 1–2 in mission (and which member then plays)
  is engine behavior this task does not model.
* Which of `soundsl.zbd`/`soundsh.zbd` the original opens stays unmeasured
  (F14-D.7); `LAUNCH_SOUND_CONTAINER` is the labelled choice.
* Audibility is not claimed: nothing was played (`human_review` is the
  owner's).
* Ammunition, ordnance, mounts and the playable rig's numbers are still
  designed — only the *record identities* and the *delivered member* are
  measured.

## Evidence

* `data/owner-static/engine-facts.toml` `[gun_loadout]` — the committed
  table (claim `OWNER-STATIC-2026-10-19`, provenance `static`).
* `docs/findings/evidence/M01-WINDOW-FIRE-GUN-LOADOUT.json` — the validated
  acceptance report.
* Tests: `accept_m01_window_fire_gun_loadout_*` in
  `crates/cs_app/tests/campaign/m01_window_fire.rs` — the committed-map
  resolution (both slots), the loadout's shot id naming the delivered
  member, the declared record's bus/mode/provenance, and the retail run
  through `stage_for` + `build_headless` asserting the member's binding,
  decode, catalog record and lowered `Weapons`/`Loop` spec.
