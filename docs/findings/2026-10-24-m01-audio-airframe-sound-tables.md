# M01 audio: the M01 aircraft's sound keys after `kind_of` resolution

Task #1917 (PARITY6-M01-AUDIO-TIMELINE.03). Capabilities: `retail`. Provenance
`static` (retail `ZBD/zrdr.zbd` member `vehicle.zrd`, read by the production
`cs_content::original_airframe::resolve_vehicle_record`); nothing here is
`verified_original` and no audible match is claimed. Tests:
`accept_parity6_m01_audio_03_` in
`crates/cs_app/tests/accept_parity6_m01_audio_03_airframe_sound_tables.rs`
(retail, ignored without `CS_GAME_DIR`).

## Correction to `2026-10-23-m01-audio-residual-engine-rate.md`

That note says exactly one `vehicle.zrd` record carries `damaged_engine_sound`
and that `pdevastator` "has none", so its pitch scale is the fixed `1.0f`. The
first half is right (one record *writes* the key: `basic_airplane`), but the
records are not read flat: a `kind_of` naming an earlier record is copied, then
the record's own keys overlay it (`0x477b70`, `2026-10-08-flight-original-fixed-wing-law.md`).
`basic_airplane` is the root of every airplane chain, so **every M01 aircraft
inherits the table**:

| Record (M01 use) | Chain | `engine_sound` (from) | `cockpit_engine_sound` | `damaged_engine_sound` | `prop_sound` |
| --- | --- | --- | --- | --- | --- |
| `pdevastator` (player) | basic_airplane, player_airplane, pdevastator | `snd_devastatorengine` (pdevastator) | `snd_devastator_cp` | `(("snd_damagedengine" 0.0 1.0))` (basic_airplane) | none |
| `devastator` (`devastator_2/3`) | basic_airplane, devastator | `snd_devastatorengine` | `snd_devastator_cp` | same | none |
| `wingman` (`wingman_2/3`) | basic_airplane, devastator, wingman | `snd_devastatorengine` (devastator) | `snd_devastator_cp` | same | none |
| `rusdevastator` (5 records) | basic_airplane, devastator, rusdevastator | `snd_devastatorengine` (devastator) | `snd_devastator_cp` | same | none |
| `bsfury` (`bsfury_1/2`) | basic_airplane, fury, bsfury | `snd_furyengine` (fury) | `snd_fury_cp` | same | none |

So the sound table at `[record+0x78]` has one entry for every M01 aircraft:
`snd_damagedengine` with floats `(0.0, 1.0)`, and `engine.pitch_scale_roll`
(`0x4b1af2`) yields `0 + (1 - 0) * rand/32768`, a uniform value in `[0, 1)`, not
`1.0f`. `snd_damagedengine` is `engine_damaged.wav`, `LOOPED 3D RANGE`, 22,050 Hz
mono, 250,976 decoded bytes (`soundsh.zbd`).

**Caveat (rule 4).** This follows the loader's copy-then-overlay rule. The
loader at `0x47a1c0..0x47a74f` was not re-traced here to confirm that it reads
`damaged_engine_sound` from the copied record rather than from the record's own
keys only, and what `[aircraft+0x68]` multiplies in the normal (undamaged)
engine voice, and when the 3-5 s re-roll runs, are still the open questions of
`engine.reroll_timer`. If the roll applies to the normal engine loop, the
scale is a random factor in `[0, 1)` and the earlier "scale term is 1" premise
of #1955's `EngineRateLaw` is wrong for M01; if it applies only to the damaged
loop, that premise stands. This task does not decide between them.

## What the data says about the owner's 6.6-31 kHz and the 33,864 B loop

* **Prop voice: none.** The word `prop_sound` occurs in no `vehicle.zrd` record,
  so no M01 aircraft (indeed no retail airframe) has a prop-voice source; the
  `prop_sound.*` keys of `player.zrd` are tuning for a voice no record names.
  The 33,864 B loop is therefore **not** an aircraft prop voice.
* **Engine loops of the other aircraft.** The second 287,408 B loop of the
  capture is `devastator.wav` for `devastator`, `wingman`, `rusdevastator`
  (all `snd_devastatorengine`); `bsfury` is `fury.wav` (340,032 B, a different
  size), so a 287,408 B second loop is a devastator-chain aircraft.
* **Damaged engine loop is not 33,864 B** (250,976 B at 22,050 Hz, 126,250 B at
  11,025 Hz in `soundsl.zbd`), and no member of either sound archive decodes to
  33,864 B (test `..._damaged_engine_member_is_not_the_unresolved_loop`).
* **6.6-31 kHz.** With a random scale in `[0, 1)` on the 0.6..1.2 pitch law, a
  low end of 6.6 kHz is reachable (`22,050 * 0.6 * 0.5`), the 31 kHz top is not
  (the scale cannot exceed 1, so `22,050 * 1.2 = 26.5 kHz` before Doppler, which
  needs a factor of at least 1.17). The numbers are **not** explained to the Hz
  and which voice the owner's log rates belong to is still unestablished.

## Still unknown (rule 4)

| Unknown | Affected content | Resolving work |
| --- | --- | --- |
| Whether `[aircraft+0x68]` scales the normal engine loop or only the damaged-engine voice, and the copy semantics at `0x47a2bd` | engine pitch of every M01 aircraft; #1955's scale-1 premise | trace the readers of `[aircraft+0x68]` and the loader for inherited keys |
| What the 33,864 B loop is | capture voice at ~27 s | not a prop voice and not `engine_damaged.wav`; needs the owner log per voice or another starter |
| Which voice carries 6.6 / 31 kHz | rate comparison | owner log per voice |
