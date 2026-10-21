# M01 audio timeline: which retail member each Drydock buffer is

Task #1861 (PARITY6-M01-AUDIO-TIMELINE.01). Capability: `retail`. Provenance of
every row below: `static` (read from the installation's own files by production
readers); nothing here is `verified_original`.

## Rule

The Drydock DirectSound buffer lengths are the **decoded 16-bit PCM byte
length** of the member, not the stored `data` length. The production helper
`cs_app::audio::pcm_size::decoded_pcm_size` computes it from the member's own
`fmt ` declaration (no sample is decoded):

- MS ADPCM (tag 2): `ceil(data / blockAlign)` blocks of
  `(blockAlign - 7*ch) * 2 / ch + 2` samples per channel;
- IMA ADPCM (tag 17): the same block count of
  `(blockAlign - 4*ch) * 2 / ch + 1` samples per channel;
- PCM: 16 bit `data`, 8 bit twice `data`;
- bytes = blocks * samples/block * 2 * ch. A trailing **partial block is
  counted whole** (needed for `waterfall.wav`: data 8,704 B = 17 blocks of 512).

The measured sizes match members of `ZBD/soundsh.zbd` (the high-rate archive;
the original plays it for these buffers). `soundsl.zbd` decodes none of them to
the same length except the 11,025 Hz `introscene3.wav`, identical in both.

## Candidate table (`soundsh.zbd`, MS ADPCM, 22,050 Hz mono unless noted)

`L` = the member's `sounds.zrd` group carries `LOOPED`. Group names are from
`sounds.zrd` (`ZBD/zrdr.zbd`), decoded by `stunts::decode_zrd`.

| Decoded bytes | Member (group) |
| --- | --- |
| 287,408 (stereo, except torpedo) | `devastator.wav` (`snd_devastatorengine`, L), `devastator_cp.wav` (`snd_devastator_cp`, L), `hellhound.wav` (`snd_hellhoundengine`, L), `kestrel.wav` (`snd_kestrelengine`, L), `missile_torpedo_hit.wav` (mono, `snd_torpedo_hit`) |
| 225,000 (11,025 Hz, both archives) | `introscene3.wav` (`snd_scene3`), `VO_c5-MN-m4_Miles_47.wav` |
| 89,056 | `introscene4.wav` (`snd_scene4`), `Metalbend3.wav`, `VO_c3-HW-M5_Zachary_14.wav` |
| 119,416 | `nujetwind.wav` (`snd_jetwind1`, L), `plane_shake.wav` (`snd_planeshake`, L), `punching.wav`, `VO_c1-HA-m3_Zachary_17.wav`, `VO_c4-RM-m1_Zachary_54.wav`, `VO_c4-RM-m3_Zachary_39.wav` |
| 34,408 | `waterfall.wav` (`snd_waterfall`, L), `hook_release.wav`, `VO_c4-RM-m2_Zachary_20.wav` |
| 267,168 | `train_derail.wav` (L), `explosion_7.wav`, `VO_c1-HA-M5_Zachary_23-02.wav`, `VO_c2-NW-M5_Zachary_19-5.wav` |
| 22,264 | `ricco4.wav`, `tny_exp_2.wav`, `warning_shot2.wav` |
| 78,936 | `exp_water_b.wav`, `VO_id1_GL-AllyDwn-B.wav`, `VO_c5-MN-m1_Zachary_4.wav`, `VO_c5-MN-m1_Zachary_21.wav` |
| 16,192 | `warning_seeker.wav` (`snd_seeker`, L), `ricco3.wav` |
| 18,216 | `ricco2.wav`, `warning_shot3.wav` |
| 220,616 | `enginelooped.wav` (`snd_enginelooped`, L), `exp_ground_a.wav`, `exp_ground_b.wav`, `exp_plane_1.wav`, `exp_plane_3.wav`, 4 `VO_*` |

## What the data narrows

Only one `LOOPED` candidate remains for the loops the capture shows, once the
loop flag is applied (a looping Drydock buffer needs a looped group):

- **287,408 B engine loops**: the player voice is `devastator.wav`
  (`pdevastator.engine_sound` = `snd_devastatorengine`). The other plane's loop
  is any of `devastator`, `hellhound` or `kestrel`: the airframe records
  `avenger`/`stihellhound*`/`germanhellhound` use `snd_hellhoundengine`, the
  `kestrel`-chain records use `snd_kestrelengine`. **Which airframe the second
  loop belongs to stays unknown** until the other M01 aircraft's airframe record
  chain is resolved (M01 aircraft records are `devastator_2/3`,
  `rusdevastator_*`, `bsfury_*`; `fury.wav` decodes to 340,032 B and is not
  this size).
- **119,416 B wind-like loop (-10 dB, pan 0)**: `nujetwind.wav` or
  `plane_shake.wav`. `punching.wav` and the VO lines are not looped groups.
  Between these two the data does not decide; `nujetwind.wav` (3D, FREQUENCY)
  is the likelier by name, `plane_shake.wav` is an airframe-damage loop.
- **34,408 B pan-57 loop at t=47.5**: `waterfall.wav` is the only looped
  member. `hook_release.wav` and the VO line are one-shot groups. The owner's
  "fire crackle" guess has no member at this size.
- **225,000 B launch one-shot**: `introscene3.wav` (11,025 Hz), also
  `VO_c5-MN-m4_Miles_47.wav` (a queued dialogue group).
- **89,056 B**: `introscene4.wav` (`snd_scene4`) by name against the cinematic
  scene group; `Metalbend3.wav` is the other non-dialogue member.
- Explosion/ricochet/warning sizes (267,168 B, 22,264 B, 78,936 B, 16,192 B,
  18,216 B, 220,616 B): sets listed above; no further narrowing from the
  groups, which are one-shot SFX with identical flags.

## Unresolved: 33,864 B (11,025 Hz loop, rate later 22,050 Hz)

**No member of either archive decodes to 33,864 B**, and no RIFF/WAVE blob in
any file of the installation does. Evidence (test
`accept_parity6_m01_audio_01_33864_matches_nothing_in_the_installation`): a raw
byte scan of every file under the install root finds WAVE blobs only in
`ZBD/soundsh.zbd` (2,521), `ZBD/soundsl.zbd` (2,520), `GOSDATA/ASSETS/crimson.rof`
(8) and `SETUPENU.DLL` (3); none decodes to 33,864 B, nor to 33,862, 33,866 or
16,932. The mission and campaign `.zbd` files (`ZBD/C*/..`) hold no WAVE.
Formats tried include 8-bit PCM (half length) and partial last blocks.
33,864 is not a whole number of 512-byte (2,024 B) or 1,024-byte (4,072 B) MS
ADPCM blocks, so it is probably a cut or a different decode path, not a plain
member. The affected content is the M01 timeline's 11,025 Hz loop; it stays
**unknown** (rule 4). Possibilities not yet tested: a sample truncated by a
`cue` chunk, MPEG-embedded audio (`GOSDATA/ASSETS/GRAPHICS/MPG`, no WAVE
header), or a decoder in the engine that yields a different length.
