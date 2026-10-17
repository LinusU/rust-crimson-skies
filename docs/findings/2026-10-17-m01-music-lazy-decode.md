# M01-MUSIC-LAZY-DECODE: M01's music tracks decode when first requested

Date: 2026-10-17. Task: `M01-MUSIC-LAZY-DECODE` (#1551), follow-up of
`M01-MUSIC-DEVICE` (#1547, `2026-10-17-m01-music-device.md`). Capability used:
`retail`. Test prefix: `accept_m01_music_lazy_`
(`crates/cs_app/tests/accept_m01_music_lazy.rs`).

Nothing here is `verified_original`.

## What changed

* `MissionRadio::read` no longer decodes the music tracks. It names each
  selectable track (`MusicTrackSource`: music id, container, member) and keeps
  the installation path. The ids are the same `music_member_id` values, and the
  container choice (`soundsh` preferred, else `soundsl`) is unchanged.
* `MissionRadio::decode_music` decodes one named track on demand;
  `MissionRadio::music_loader` hands that to the world as `LazyMusicTracks`.
  `publish_prepared_voices` installs the loader instead of publishing tracks.
* `audio::publish_requested_music` runs between `advance_radio` and
  `mix_session`. For each pending `Started`/`Transition` verdict that is
  audible and whose track the device library lacks, it decodes that track and
  publishes it (`ensure_music_published`). Inaudible, `Unchanged` and duplicate
  verdicts decode nothing; a track already published is not decoded again.
* A track that will not load is listed in `AudioMixReport::music_unloaded`
  with the reason; the device still refuses it `sample_unavailable`, as before.
* A decode failure used to surface in `MissionRadio::failures()` at prepare
  time; it now surfaces at first request, in `music_unloaded`. A music track
  that is absent from both archives, or has no id, is still a prepare-time
  failure.

## Measured (retail, `MissionRadio::read` for M01, 19 music tracks)

| build   | before (eager) | after (lazy) |
| ------- | -------------- | ------------ |
| release | 137-140 ms     | 58 ms        |
| debug   | 730 ms         | not re-measured |

First request of one track (release): 19 ms (`music_battlesuccess1.wav`).

**Correction to #1547's finding.** That finding states the eager decode adds
"roughly 30 s of CPU" in release. It does not: `MissionRadio::read` took
~0.14 s in release and ~0.73 s in debug with all 19 tracks decoded. The ~34 s
(release) / ~275 s (debug) seen around it is `plan_mission_launch`, which runs
before the radio is read and is untouched here. The remaining cost of
content preparation for M01 is that planning step, not music.

Memory is the other gain: 19 stereo 22 kHz tracks (0.22-0.79 M frames each, as
f32) are no longer all resident from launch; only requested tracks are.

## Not measured

* Whether the first-request decode (tens of ms in release) is audible as a
  hitch in a real run; debug builds decode several times slower.
* No eviction: a track stays in the library once requested.
