# M01-MUSIC-DEVICE: M01's music state requests play through the audio device

Date: 2026-10-17. Task: `M01-MUSIC-DEVICE` (#1547), follow-up of #1463
(`M01-PLAY-MUSIC`) and #1539. Capabilities used: `retail`, `gpu`, `audio`.
Test prefix: `accept_m01_music_device_` (3 tests,
`crates/cs_app/tests/accept_m01_music_device.rs`). Evidence report:
`docs/findings/evidence/M01-MUSIC-DEVICE.json` (harness
`crates/cs_app/tests/evidence_report_m01_music_device.rs`).

Nothing here is `verified_original`: the original program never ran, and no
person listened (`human_review` stays the owner's).

## What was missing

#1463 delivered the request half: a `music_*_sg` cue reaches `MusicDirector`
under the owner-ruled track id. `AudioSession::request_music` returned the
verdict and nobody played it, and no decoded music track was in the device's
sample library (finding `2026-10-16-m01-play-music-state-requests.md` §6 item 3;
`2026-10-17-m01-window-hud.md` §3).

## What landed

* `cs_sim::audio_events::AudioMixer::mix_music` — `Started` and `Transition`
  (audible) stop the sounding track and start the new one as a **looping**
  voice on `AudioBus::Music` at `MUSIC_GAIN` (1.0); `Unchanged` and
  `SuppressedDuplicate` leave it; an inaudible verdict, a device refusal
  (including `sample_unavailable` for a track the library lacks) and a director
  refusal are named in `MusicMixReport::refusals`, never silence.
  `device_lost` stops the music voice (`MixerReport::music_stopped`).
* `AudioSession` keeps the director's verdicts (`drain_music`);
  `mix_session` drains them after the radio pass and records the result in
  `AudioMixReport::music`. A lost output reads the verdicts without reopening
  the device.
* `MissionRadio::read` decodes every track a prepared music state can select
  from the labelled container (`ZBD/soundsh.zbd`, else `soundsl.zbd`) into
  `DecodedMusicMember`s (id = `music_member_id`), and the existing
  `publish_prepared_voices` step hands them to the device library beside the
  voices (`publish_music_members`). The windowed M01 composition already
  installs that step, so a music request in the window now reaches the output.
* The sample decode now uses a fresh `ParseContext` per member. The 64 MiB
  allocation budget was shared across one archive, so the long
  `music_primaryobj*`/`music_secondaryobj*` tracks (1.8-2.1 MB of samples each)
  were refused `allocation_budget_exceeded` after the first tracks had used it.

## Measured here (retail, `audio`)

* M01's five music states select 19 distinct tracks (prebattle's image table
  repeats three names), all decode non-silent: 22 050 Hz stereo, 0.22-0.79 M
  frames.
* The probe (`music-playback.json`, private) drives M01's own cue chain
  (completing the waker blocks, as #1463's harness does, because the music cues
  are not in the opening), mixes the verdicts to the machine's real output via
  `open_audible_device`, and reads what the stream pulled from the sounding
  track: 88 478 sample values, peak 0.68, sounding track = the last requested
  (`music_secondaryobj1.wav`). This is the **source boundary**, not a speaker
  capture.
* The windowed `cs --mission M01` capture (`retail,gpu,audio`) still launches,
  draws and exits 0 with the music tracks prepared. Its capture run is a few
  seconds and M01 requests no music in the opening, so the windowed run itself
  starts no music voice; the audible claim rests on the probe above.

## Designed, not measured

* That a track **loops**, the **hard cut** between tracks (no fade or
  crossfade), unity gain, and the music bus choice.
* The slot ↔ track binding (unknown #1 of the #1463 finding) — the image
  table's spelled order, as the owner ruled.
* After a device loss the director keeps the cue current and
  `device_restored` returns it, but nothing re-issues it to the mixer yet; the
  music stays silent until a caller re-requests. **Resolved by #1552
  (`M01-MUSIC-RESTORE`, 2026-10-17):** `device_restored` now queues the
  director's `Started` verdict in the session, so the one music mix pass replays
  the current cue on the music bus; see
  `docs/findings/2026-10-17-m01-music-restore.md`. Restarting the cue from its
  beginning, with the same hard cut and unity gain, remains this build's
  designed choice.

## Unknown / open

* Original fades, crossfades, ducking under radio, and which container the
  original opens — unmeasured.
* **Prepare cost.** Decoding the 19 tracks adds roughly 30 s of CPU in a release
  build (debug builds are several times slower) to content preparation. A lazy
  or streaming decode would remove it; filed as a follow-up.
* Real listening: `human_review`.
