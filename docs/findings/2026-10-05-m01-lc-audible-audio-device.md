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
* **`LoopingVoice`** — the per-voice source, **public**. It was private at first,
  which left the three properties the finding below names as the highest
  deviation risk — the channel interleave, the pan law and the loop seam —
  reachable only on a machine with an output device. It is a plain
  `Iterator<Item = f32>`, so pulling from it measures exactly what an output
  stream would consume with no hardware involved, and those three properties are
  now covered in CI. It also **shares** the member's sample buffer through an
  `Arc` instead of copying it per voice: a decoded retail member is over a million
  samples, and the first version copied (and, for mono, doubled) that on every
  voice start, on the audio thread's start path. A mono member is spread to
  stereo one frame at a time in `next` rather than into a second buffer.
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
`accept_m01_lc_audio_device_`. 20 tests, 19 synthetic + 1 retail+audio.

The two `a_voice_source_*` scenarios were added during review. At
implementation time the pan law, the channel interleave and the loop seam were
reachable **only** through a running output stream, so they were asserted only in
the one `#[ignore]`d retail scenario — on a machine with an `audio` capability
and nothing else. Making the source public moved all three into CI. Two of them
caught something on the way: `cos(π/2)` is `6.1e-17`, not a clean zero, so the
muted side of a hard-panned voice is asserted inaudible rather than exactly
`0.0`; and a placement update that lands mid-frame must apply to the channel
actually due, which is now asserted rather than assumed.

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

## Test inventory (`accept_m01_lc_audio_device_`, 20)

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
| `a_voice_source_emits_the_assets_channels_placed_and_repeated` | synthetic | every frame's channels are swapped, the pan law is wrong, or the loop seam drops or duplicates a frame |
| `the_voice_source_reports_the_members_rate_and_stereo_channels` | synthetic | a spread voice reports one channel, or the device's rate is imposed on the member's |
| `the_two_open_failures_carry_different_codes` | synthetic | a missing capability and a missing device are indistinguishable |
| `a_stand_in_world_is_not_audible_and_says_so` | synthetic | "audible" is inferred from a running frame |
| `the_probe_measures_the_sample_path_and_nothing_else` | synthetic | a refused start is reported as playback |
| `close_is_idempotent_and_leaves_no_voice` | synthetic | teardown strands a voice |
| `a_retail_sound_member_decodes_and_plays_through_the_real_device` *(retail + audio)* | hardware | nothing plays, or the decoded member is not what reached the stream |

All 20 call production code. The 19 synthetic tests write RIFF/WAVE members byte
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
3. **`LoopingVoice`'s cursor and pan** — the cursor wraps the member's shared
   buffer and the channel is read off it *before* it advances (for a mono member,
   off the "this frame still owes a right channel" flag instead). Off-by-one there
   swaps the channels of every frame, which sounds plausible and is wrong. This
   is now asserted in CI rather than only on a machine with a sound card.

## Follow-up tasks filed rather than fixed here

Three, because each is a different kind of work and none belongs in a bounded
device slice:

* **#652 `M01-LC-AUDIO-LOAD`** — populate a `SampleLibrary` from a delivered F15
  load closure. This is the missing last hop: the device, plugin and mixer are
  real and tested, but no loader fills the library, so a mission must hand the
  plugin one by hand.
* **#653 `F41-D-MEDIA-AUDIT`** — the complete media audit F41-D asks for. One
  member of one container is a minimum scenario, not AC04's coverage.
* **#654 `M01-LC-AUDIO-OWNER-GATE`** — the `human_review` gate. It needs the owner
  listening, and no agent can supply it.

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

## Evidence

`docs/findings/evidence/M01-LC-AUDIO-DEVICE.json`, generated by
`crates/cs_app/tests/evidence_report_m01_lc_audio_device.rs` from
`private/evidence/M01-LC-AUDIO-DEVICE/`. It carries two artifacts, both hashed
with the production SHA-256 and both private:

* `cargo-test.log` — the recorded
  `cargo test --workspace --locked -- accept_m01_lc_audio_device_ --include-ignored`
  run: **18 discovered, 18 executed, 18 passed, 0 failed**, exit code 0.
* `sound-member-census.json` — a **second production observation** over the
  installation, independent of any playback assertion. It re-reads
  `ZBD/soundsl.zbd` through the same production ZBD reader the acceptance
  scenario used and records what the archive really holds: **2520 members, all
  2520 `decoded`**, no member unresolved under its own declared format. The first
  decoded member is `c2-NW-m1_briefing.wav` at **1137765 samples, 11025 Hz, 1
  channel** — the same member the play scenario chose, derived independently by
  "first member that decodes" rather than asserted.

The report is validated with `tools/validate_evidence.py` **without**
`--require-pass`:

```sh
python3 tools/validate_evidence.py private/evidence/M01-LC-AUDIO-DEVICE/acceptance.json \
  --artifact-root private/evidence/M01-LC-AUDIO-DEVICE
# {"structurally_valid": true, "artifact_count": 2, "claims_semantically_verified": false}
```

`--require-pass` rejects any nonempty `unknowns`, and this report's `unknowns`
holds the eight scope limits listed above. Removing them to satisfy the flag
would be exactly the shortcut `docs/contracts/CLI-EVIDENCE.md` forbids: those
limits are what gate a fidelity or audible claim. The same choice for the same
reason is documented in `docs/findings/2026-09-29-f12-j-letter-o-colour.md`.

`capabilities` is `["audio", "retail", "synthetic"]`: the `audio` class was
exercised on this machine, `retail` because the census reads the owner's
installation, and `synthetic` for the 17 fixture-only scenarios. **`human_play`
and `human_review` were not exercised and are not in that list.** `claim` is
`implemented`, never `verified_original` — a Rally merge plus an agent review
would award `checked` at most.

One process note: the committed report names
`candidate_tree 9d37b04bb037c5611dc9149a6ab289ef6e746409`, the tree of the commit
that carried the acceptance suite when the run was recorded. Committing the
report itself changes the tree, so a report can never name the tree that contains
it; the reviewer regenerates the report on the reviewed commit and compares it,
which is what the contract asks for. The numbers that depend on the tree
(`candidate_tree` itself) change and the ones that do not (**20/20/20**, exit 0,
2520/2520, the member's shape, the two digests) must reproduce.

The reviewer regenerated it. Both figures that moved are accounted for: the test
count went 18 → 20 with the two `a_voice_source_*` scenarios added during review,
and the tree changed with them. Everything the second production observation
derives from the installation reproduced exactly — 2520 members, all 2520
`decoded`, first decoded member `c2-NW-m1_briefing.wav` at 1137765 samples,
11025 Hz, 1 channel — and the retail scenario passed again on the reviewed code,
so the review's change to the voice source did not alter what reaches the stream.

## What the review changed

Reviewer: `bunny-2/bunny-2`, a separate session with fresh context over the same
agent's implementation. **This is not an independent review** — the same agent
instance both implemented and reviewed the work, so it is not independent
evidence about the original game, and it is recorded as such here and in the
report's `review.identity`. `docs/findings/evidence/M01-LC-AUDIO-DEVICE.json`
names the same identity for the same reason. What this review can support is that
the code does what this finding says and that its claims are checked by tests
that reach production code.

Three things changed, all of them in this finding's "where the deviation risk
is" list:

1. **The per-voice sample copy is gone.** `LoopingVoice::new` copied the whole
   decoded member — and doubled it for mono — on every voice start. A retail
   member is over a million samples (`c2-NW-m1_briefing.wav` decodes to 1137765),
   so starting one engine loop allocated several megabytes, and starting it again
   allocated them again. It now shares the `Arc` the `PcmAudio` already holds and
   spreads mono to stereo one frame at a time in `next`. Same emitted values; the
   assertion that pins them is the new `a_voice_source_*` scenario.
2. **The pan law, the channel interleave and the loop seam are covered in CI.**
   All three were asserted only inside the `#[ignore]`d retail scenario, because
   `LoopingVoice` was private. They are the three properties this finding names as
   the most likely to be silently wrong, and a machine without an `audio`
   capability could not check any of them. The source is now public and the three
   are asserted by pulling from it.
3. **The evidence harness's own instructions were wrong.** Its module doc told
   the reader to validate with `--require-pass`, which rejects any nonempty
   `unknowns` — and this report's `unknowns` are the scope limits that gate every
   audible claim. The review method below already said the report is validated
   without that flag; the step-by-step instructions contradicted it, so the next
   agent to follow the doc would have deleted the limits. The doc now states the
   omission and why.

Two assertions written during the review were **wrong on first run and the code
was right**, which is recorded because it is the interesting part:

* `cos(π/2)` is `6.1e-17`, not `0.0`. The constant-power law's muted side is
  inaudible, not a clean zero, and asserting an exact zero would have been
  asserting a rounding accident. The scenarios now compare inaudible-to-1e-6 and
  say why that tolerance cannot hide an ordering or gain error.
* A placement update that lands **mid-frame** — after one value, the cursor is on
  the right channel — must apply to the channel actually due. The first
  expectation assumed the cursor restarted at a frame boundary; the source does
  not, and must not. That is now asserted: a source that recomputed the channel
  from a re-zeroed cursor would put the left gain on a right-channel value.

## Checks

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_m01_lc_audio_device_ --include-ignored` | 0 (20: 19 synthetic, 1 retail+audio) |
| `python3 tools/validate_evidence.py private/evidence/M01-LC-AUDIO-DEVICE/acceptance.json --artifact-root private/evidence/M01-LC-AUDIO-DEVICE` | 0 (without `--require-pass`, see above) |