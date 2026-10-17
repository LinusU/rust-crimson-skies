# M01-MUSIC-RESTORE: the current music cue is re-issued after a device restore

Date: 2026-10-17. Task: `M01-MUSIC-RESTORE` (#1552), follow-up of #1547
(`M01-MUSIC-DEVICE`). Capabilities used: `retail` (regression runs of the
retail music suites only; the task's own acceptance suite is synthetic).
Test prefix: `accept_m01_music_restore_` (3 tests,
`crates/cs_app/tests/accept_m01_music_restore.rs`).

Nothing here is `verified_original`: the original program never ran and no
person listened (`human_review` stays the owner's).

## What was missing

#1547 played M01's music state requests through the device and taught the
`MusicDirector` to keep its cue current through a device loss
(`2026-10-17-m01-music-device.md`), but nothing re-issued that cue:
`audio::device_restored` re-opened the output and the session re-bound its
loops, while the music stayed silent until the mission requested another state.

## What landed

* `cs_sim::audio_events::MusicDirector::device_restored` now returns the verdict
  that restarts the cue — a
  `MusicOutcome::Started { to: <current cue>, audible: true }` — because the
  mixer's music voice was stopped by the loss, so from the mixer's side music
  begins with nothing before it.
* `cs_app::audio::loops::AudioSession::device_restored` queues that verdict in
  the session's music log, so the **one** music mix pass
  (`mix_session` → `mix_music`, after `publish_requested_music`) plays it,
  including the lazy decode a started verdict asks for. No second device path
  is added.
* The cue's identity is unchanged: `MusicDirector::current` still returns the
  same cue across the loss, and a second restore on an already-available output
  re-issues nothing (idempotent).

## Designed, not measured

* Restarting the current cue **from its beginning** after an output device
  returns, with the same hard cut and unity gain as a fresh state request. The
  original's device-restore behavior — restart, resume at an offset, crossfade
  back or nothing — is unmeasured; this build's choice keeps the authored cue
  playing (F41 non-negotiable behavior 4) rather than leaving the mission silent
  until another state request.
* The loop, fade and unity-gain rules of the music bus are #1547's designed
  choices and are unchanged here.

## Tests

`crates/cs_app/tests/accept_m01_music_restore.rs`: the real `AudioPlugin` in a
Bevy `App` over a `RecordingAudioDevice` — a request sounds, the output is lost
(the voice stops, the director keeps the cue), the restore makes the next mix
pass sound the **same** cue again on the music bus; a world with no current cue
re-issues nothing; a second restore is a no-op. The suite is sensitive:
removing the queued re-issue fails two of the three tests.

## The M01 owner-command smoke

The owner-command smoke (`docs/PLAYTEST.md`, "The owner-command smoke") drives
M01's ordinary play with no device loss, and its checks cover the world, sea,
actors, weapons, radio and engine audio — not a device restore and not music.
It therefore cannot observe this criterion; no framebuffer from it is offered
as evidence for the replay. Ordinary-play listening remains the owner's F41-D
gate.
