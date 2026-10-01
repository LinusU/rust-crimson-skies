# F41-C: radio queue, music transitions, subtitles, device loss

Date: 2026-10-01. Task F41-C. Capabilities: ordinary build/test only.

## Done

- `cs_sim::audio_events`: `RadioLine`/`RadioQueue`/`RadioEvent` (priority,
  interruptibility, subtitle, tick-based completion, bounded pending queue,
  per-producer sequence dedup) and `MusicCue`/`MusicDirector`/`MusicOutcome`
  (authored transitions only, dedup, music-kind check). `AudioRouter::active_loops`.
- `cs_app::audio::AudioSession`: owns the queue and director;
  `device_lost`/`device_restored` stop and remember loops (retry rebinds live
  emitters only; a despawn during loss is not resurrected), keep radio timing
  and subtitles, and return the music cue to restart. `advance_radio` system.
- Tests: `cs_app/tests/accept_f41_c_radio_device_loss.rs`. Probe: removing the
  `voiced = false` in `RadioQueue::device_lost` fails the minimum scenario.

## Unknown / not done (designed, not original)

- Original radio priority, interrupt and queue-depth rules, speaker set and
  music transition shapes (crossfade lengths, cue triggers) are unmeasured;
  every rule here is a designed option, not a fidelity claim.
- No decoded clip yet supplies `duration_ticks` (ADPCM decode is #444); no
  mixer/device consumes the events; `AudioSession` is not yet inserted by the
  loading handoff. Subtitle text sources (string resources) are F51's.
- A line running when the device returns stays unvoiced to its end.
