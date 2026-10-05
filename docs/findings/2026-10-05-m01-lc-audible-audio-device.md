# M01-LC-AUDIO-DEVICE: the audible backend, and what it does not prove

Date: 2026-10-05. Task: #635 (`M01-LC-AUDIO-DEVICE`), "Implement an audible
`AudioDevice` backend". Spec:
`specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stage `### F41-D`'s
playback half. Capabilities used: `retail`, `audio` (both declared in
`$CS_CAPABILITIES` on the machine this was implemented on).

## The gap this closes

`cs_app::audio` had a complete mixing stack and no output:

| Piece | Where | State before |
| --- | --- | --- |
| Router, loop lifecycle, one-shot ledger | `cs_sim::audio_events` | present |
| Session, engine smoothing, radio queue, music director | `cs_app::audio::{loops,engine,lower}` | present |
| Mixer, spatial law, device-loss path | `cs_app::audio::mixer` | present (#445) |
| Plugin, schedule, loading handoff | `cs_app::audio::plugin`,`handoff` | present (#445) |
| **`AudioDevice` that opens hardware** | — | **missing** |

`AudioOutput::default()` was a `RecordingAudioDevice`: it recorded commands and
made no sound. Every command the mixer issued was therefore unverifiable at the
output, no `audio`-capable run could produce an audible artifact, and VS-M01-RUNTIME
(#359) was blocked on this. The boundary was complete and unreachable.

## What was added

`crates/cs_app/src/audio/device.rs` (new, owner path) plus the wiring in
`plugin.rs` and `mod.rs`:

* **`PcmAudio`** — one decoded asset in the device's domain: interleaved `f32`
  normalized onto `-1.0 ..= 1.0` from the width the member declared, with the
  member's own channel count and rate unchanged.
* **`sound_member_pcm`** — the runtime consumer of the F06-C decode. It takes a
  `cs_assets::zbd::SoundAsset` and produces `PcmAudio` under that member's own
  WAVE header, and every failure carries the **refusal's own stable code**
  (`SampleError::code()` / `PcmError::code()`), not a generic one.
* **`SampleLibrary` / `InMemorySamples`** — where a load puts the samples a
  `VoiceStart` names. The device never decodes and never opens a file; an asset
  the library does not hold is refused by name.
* **`AudibleDevice`** — the `AudioDevice`, over rodio's
  `DeviceSinkBuilder::open_default_sink()` + `Player`. One looping voice per
  started loop; gain and pitch are `Player::set_volume`/`set_speed`, pan is a
  constant-power stereo gain applied inside the source and shared with the mixer
  through an atomic cell (a rodio source is moved into the audio thread's mixer
  and cannot be borrowed afterwards).
* **`open_audible_device`** — the capability gate. **Capability first**: a machine
  that does not declare `audio` is refused with `audio_capability_absent` *before
  any hardware call*. **Hardware second**: a declared machine with no stream gets
  `no_output_device` and rodio's own explanation. `audibility_exit_code` maps the
  first to exit code 4 and the second to 1, per `docs/contracts/CLI-EVIDENCE.md`.
* **`RefusingAudioDevice`** — what a world installs when the gate refuses. Every
  command fails with the same named code, so the mixer's report names the reason.
  This is the difference from falling back to the recording device: a recording
  device reports success while nothing is audible, which makes a capability
  failure indistinguishable from a mission with nothing to say.
* **`AudioBackendLog`** — a world resource saying which of the three backends
  (`Audible` / `Refused` / `StandIn`) the world has, what the machine declared, and
  the refusal if any. A running frame is identical on all three, so this resource
  is the only honest answer to "is this world audible?".
* **`SampleProbe`** — a tap on the **source** boundary: it counts the frames the
  output stream pulled, their peak and their energy. It makes "the member played"
  checkable on an `audio`-capable machine.
* **`AudioPlugin::audible(library)`** — the production device wiring, and
  `AudioPlugin::with_capabilities(declaration)` so a caller (or a test) states a
  machine's capabilities instead of the environment being read.

## Verification

`crates/cs_app/tests/accept_m01_lc_audio_device.rs`, prefix
`accept_m01_lc_audio_device_`. 18 tests, 17 synthetic + 1 retail+audio.

The minimum scenario
(`a_retail_sound_member_decodes_and_plays_through_the_real_device`,
`#[ignore]`) ran locally against `$CS_GAME_DIR` on a machine declaring
`retail,gpu,audio`, and passed. What it actually did, measured:

1. `cs_assets::install::discover` read the installation (fingerprint over the
   whole tree).
2. The VFS mounted it; `ZbdContainer::open` routed `ZBD/soundsl.zbd` to the sound
   family and read its own version-one trailer.
3. `sound_assets` listed **2520** members; the first that both decoded and
   declared 1 or 2 channels was `c2-NW-m1_briefing.wav`.
4. `sound_member_pcm` decoded it under its own header: **1137765 frames,
   11025 Hz, 1 channel** (IMA ADPCM — the shape task #344 measured most often).
5. The device opened the default output stream, took the voice, and
   `played_position` advanced past zero, i.e. **the output stream consumed frames
   from that voice**.
6. The probe recorded `pulled > 0`, `peak > 0.0` and `energy > 0.0`, so the
   values the stream consumed were real samples of the decoded member and not
   silence.
7. `update_voice` (gain 0.5, pan -1.0, pitch 1.5) succeeded, `stop_voice`
   succeeded, `close` left `is_open() == false`.

**Cost:** this scenario takes ~15 minutes, almost all of it `discover` (~350 s)
and the sound listing (~525 s) over a 190 MB retail tree. That is the same shape
as `accept_f14_d_7_retail_sound_cues_are_rows` (~25 min). It is a local
implementer/reviewer cost, not a CI cost — CI has no original data and skips it.

## Limits of what was proven

Everything below is **unknown**, and each limits a fidelity claim. None of it is
guessed anywhere in the code.

* **A person did not hear anything.** This is the load-bearing limit. The probe
  measures the source boundary of a running output stream, which is stronger than
  a nonempty WAV and weaker than `human_review`. `human_review` and
  `human_play` were never available to an agent and remain the owner's gate. F41
  non-negotiable behavior 5 keeps decoded-PCM evidence and audible device
  verification apart, and this finding keeps them apart too.
* **Only one member, one container.** One `soundsl` member was decoded and
  played. The complete media census F41-D asks for (every container, every member,
  every loop seam) is **not** done: `soundsh` was never opened, no member's
  readiness distribution was re-measured here, and no claim is made that the other
  2519 members play. That census is F41-D's own scope, not this task's.
* **Bus faders are not applied.** A voice plays at `loop gain × engine gain ×
  spatial gain`, exactly as `cs_sim::audio_events::AudioMixer` computes it,
  because no original bus fader is known. Seven buses exist in the runtime
  vocabulary and all seven currently mix at whatever the record's declared level
  says.
* **There is no limiter.** Two voices summing past `1.0` clip at the device. This
  is recorded as a known limitation rather than papered over with a limiter whose
  threshold would itself be an invented tuning value.
* **The pan law is designed.** Constant-power (`cos`/`sin` of the pan angle), and
  a mono member is upmixed to stereo inside the voice source so pan is a real
  left/right placement. The original engine's spatialization is unmeasured (F41
  research boundary); nothing here claims it matches.
* **Doppler is absent.** `VoiceUpdate` carries gain, pan and pitch; nothing
  carries a frequency shift from relative velocity. F41 non-negotiable behavior 1
  requires doppler to be "evidence-backed or designed options", and **it is
  neither here** — it is missing, not designed.
* **`pitch` is rodio's playback-rate ratio**, not a resampling design of ours, and
  its quality at 2.0 (which the engine smoothing can reach) was not measured.
* **Loop seams are not measured.** No retail member declares loop points
  (F06-C's finding), so every loop in this backend repeats an asset end-to-end,
  seam and all. Whether that is what the original did is **unknown**.
* **Only mono and stereo are placeable.** Three channels or more is refused with
  `pcm_unsupported_channels`. Retail declares at most two (task #344), so no
  observed member is refused, but a wider member would be rather than downmixed.
* **The gate is a declaration, not a measurement.** `$CS_CAPABILITIES` is parsed
  and trusted as exactly that, reusing `cs_content::replay::DeclaredCapabilities`.
  A machine that declares `audio` and then has no stream gets
  `no_output_device`; a machine that declares nothing gets
  `audio_capability_absent`. A declaration is not proof a speaker works.
* **The audio capability is `audio`, not `human_review`.** A run on this machine
  exercised the `audio` class only. `human_play` and `human_review` were never
  available and gate any "the player hears it" claim.

## The gate, and why `RefusingAudioDevice` exists

The task required that an absent `audio` capability be a *named nonzero failure*
and never a silent mute. The implementation splits the two failures apart because
they are not the same event:

| Machine | `open_audible_device` | Installed device | `AudioBackendLog` |
| --- | --- | --- | --- |
| declares `audio`, stream opens | `Ok(AudibleDevice)` | the real device | `Audible` |
| declares `audio`, no stream | `Err(no_output_device)` | `RefusingAudioDevice` | `Refused { Open }` |
| declares no `audio` | `Err(audio_capability_absent)` | `RefusingAudioDevice` | `Refused { CapabilityAbsent }` |
| `$CS_CAPABILITIES` unparsable | `Err(audio_capability_absent)` | `RefusingAudioDevice` | `Refused { CapabilityUnparsable }` |
| never asked for audible | not called | `RecordingAudioDevice` | `StandIn` |

The last row is why the fourth exists: a `$CS_CAPABILITIES` list with a typo
(`retail,teleport`) must **grant nothing and say why**, because a list that
silently dropped the unknown element would read as "this machine declared `audio`
but the device is broken", which is a different claim with a different exit code.
`classify_refusal` reads that distinction off the declaration rather than out of
the error's wording, so a diagnostic does not depend on prose.

## Test inventory (`accept_m01_lc_audio_device_`, 18)

| Test | Needs | What fails if the implementation goes |
| --- | --- | --- |
| `an_absent_capability_is_a_named_nonzero_failure` | synthetic | the gate opens hardware anyway, or reports success |
| `retail_access_alone_does_not_authorize_playback` | synthetic | file access is mistaken for an output device |
| `a_malformed_capability_list_refuses_rather_than_widening` | synthetic | a typo silently grants capabilities |
| `a_refused_world_reports_the_refusal_instead_of_going_silent` | synthetic | the fallback is the recording device, so nothing names the failure |
| `the_voice_that_starts_is_the_asset_the_mixer_named` | synthetic | the asset lookup is dropped and a *different* asset plays |
| `an_unregistered_asset_is_named_before_the_device_state` | synthetic | the two refusal codes are conflated |
| `a_closed_device_refuses_an_update_and_a_stop_by_name` | synthetic | an unknown voice and a closed device report the same code |
| `a_corrupt_mix_value_is_refused_rather_than_clamped` | synthetic | a NaN gain is clamped, so the mixer reports success at the wrong volume |
| `each_declared_width_normalizes_onto_the_device_domain` | synthetic | 8-bit PCM's silence (128) is not shifted, so silence becomes full-scale DC |
| `an_unplaceable_shape_is_refused_by_name` | synthetic | a shape the device cannot place is downmixed by a guess |
| `a_foreign_session_loop_never_reaches_the_device` | synthetic | a stale generation's loop reaches this generation's device |
| `device_loss_closes_the_audible_device_and_simulation_continues` | synthetic | the loss path moves simulation state, or the retry is silent |
| `the_mix_the_device_is_asked_for_carries_the_engine_level` | synthetic | gain/pitch are dropped, so the loop plays at a fixed rate |
| `the_two_open_failures_carry_different_codes` | synthetic | a missing capability and a missing device are indistinguishable |
| `a_stand_in_world_is_not_audible_and_says_so` | synthetic | "audible" is inferred from a running frame |
| `the_probe_measures_the_sample_path_and_nothing_else` | synthetic | a refused start is reported as playback |
| `close_is_idempotent_and_leaves_no_voice` | synthetic | teardown strands a voice |
| `a_retail_sound_member_decodes_and_plays_through_the_real_device` *(retail + audio)* | hardware | nothing plays, or the decoded member is not what reached the stream |

All 18 call production code. The 17 synthetic tests write RIFF/WAVE members byte
by byte and reach the conversion through the production header reader and
decoder, so the values asserted on are the ones a real member would carry; no
fabricated `DecodedSound` is used. The retail test re-derives every figure from
`$CS_GAME_DIR` on each run.

## Where the deviation risk is

Three places a reviewer should look hardest:

1. **`normalize()`** — the width-to-domain table. Getting 8-bit PCM wrong turns
   silence into DC offset, which is audible as a hum and would still "play". The
   test pins the three stored extremes of each width.
2. **The gate's ordering** — capability before hardware. Reversed, a headless
   machine would attempt an open and report `no_output_device` for what is really
   a missing capability, which is a false statement about the machine.
3. **`LoopingVoice`'s cursor and pan** — the cursor wraps the interleaved buffer
   and indexes `is_right` off `cursor % 2` *before* incrementing. Off-by-one there
   swaps the channels of every frame, which sounds plausible and is wrong.

## Not done here

* **F41-D's media audit** (every container, every member, complete media
  coverage, loop seams, radio ordering). Task #635 was the missing device; the
  audit is F41-D's own scope and this finding names what a single-member run
  leaves open.
* **Bus faders, limiter, doppler, loop-seam handling.** Recorded as unknown above.
  Each needs either measured original data or an explicitly designed option, and
  neither is something this task could supply.
* **Wiring a delivered load's decoded audio into a `SampleLibrary`.**
  `sound_member_pcm` is the per-member consumer and the plugin takes a library,
  but the F15 load path does not yet populate one from the closure it delivered.
  Until it does, `AudioPlugin::audible` has to be handed a library by a caller; the
  mixer, the plugin and the device are all real and tested, and the last hop is
  the loader's.

## Checks

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_m01_lc_audio_device_ --include-ignored` | 0 (18: 17 synthetic, 1 retail+audio) |