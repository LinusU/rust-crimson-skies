# FLAK-C4-SOUND (#1886): `snd_enemyflak` at the shot, `snd_missile_flak` at the burst

Date: 2026-10-22. Follow-up of `2026-10-22-flak-c1-burst-presentation-and-descriptor-flag.md`.
Capabilities: `retail` (read of `ZBD/zrdr.zbd`, `ZBD/soundsh.zbd`) and a static
disassembly of `0x5ad100` / `0x5ad160` in `$CS_ENGINE_IMAGE`. No original run;
every claim is `static`, none `verified_original`. Nothing of the image is
committed.

## What is bound

| Cue | Source | Group | Member | `RANGE` |
| --- | --- | --- | --- | --- |
| shot | `wep_27` `FIRE` row `SOUND` | `snd_enemyflak` | `enemy_flak.wav` | 800 .. 2400 |
| burst | `wep_27` `IMPACT` row `default` (index 0, FLAK-C1) `SOUND` | `snd_missile_flak` | `missile_flak.wav` | 800 .. 2600 |

Both groups are `3D SFX` in `sounds.zrd` with no `LOOPED`, so both are one-shots
(`PlaybackMode::OneShot`, claim `flak-c4.one-shot-mode`). Both members exist in
`ZBD/soundsh.zbd`, the container the launch sound is fed from (the same labelled
choice as `LAUNCH_SOUND_CONTAINER`; which archive the original opens stays
unmeasured, F14-D.7). The chain is read by `measure_flak_sound`; the cue is
played by `play_flak_shot` (at the muzzle) and `play_flak_burst` (at the burst
point, only for the `default` row) through the session router with producer
serial `FLAK_SOUND_PRODUCER` (3). The members are declared in the mission's
audio catalog and delivered by the load beside the gun sound
(`MissionStage::flak_sound`, M01's retail stage only).

## The pick

`0x5ad100(weapon, record, row, 1.0)` (and its twin `0x5ad160`, which reads the
list one slot further, count at `+0x40`, handles at `+0x44`) computes, with the
row at `weapon+0x15c + row*100`:

    count = [row+0x2c]; if count == 0 return
    index = (rand() * count) >> 15        // rand through the import at 0xa20350
    play(handle = [row+0x30 + index*4], position = arg+0xc, ...)   // 0x593b80

`pick_index(count, rand)` is that law. Both flak groups map to one member, so
the index is 0 for every `rand` in `0..=32767`; no random state is needed.

## Unresolved

| Unknown | Evidence | Affected content | Resolves in |
| --- | --- | --- | --- |
| Whether the row's own `SOUND` handle (`row+0x24`, played at `0x5aca01` through `0x525d90`) and the `0x5ad100` list pick are the same sound, so the burst might sound twice | both paths were read, the parser that fills `row+0x24` and `row+0x2c/+0x30` was not | the loudness/doubling of the burst | a trace of the weapon-section parser's `SOUND` handling, or an owner capture |
| Which fire path plays the `FIRE` row's sound (`0x5ad100` / `0x5ad160` / the row handle) and with which row index | not traced | the shot cue is played once at the muzzle | trace of the weapon fire `0x5aef40` |
| What the `RANGE` pair (800, 2400) does | the world's designed attenuation places the cue; the pair is kept on `FlakVoice::range_m`, unapplied | distance falloff of both cues | F41 spatial work |
| The bus and level | designed (`Weapons`, 1.0), like the gun sound | mix | F41 mixing |
| The `player` and `buildings` `IMPACT` rows, which also name `snd_missile_flak` | impacts that pass a surface, not an air burst | impacts on surfaces | later flak/impact work |
| Nothing fires a woken turret's `wep_27` in the live session yet | FLAK-B / C5 | audibility in the M01 smoke | #1887 (FLAK-C5) |
