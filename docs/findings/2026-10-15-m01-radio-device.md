# M01-RADIO-DEVICE: the queued radio lines play through the device

Task: #1461 (`M01-RADIO-DEVICE`), follow-up of #1438 (`M01-PLAY-RADIO`).
Date: 2026-10-15. Capabilities used: ordinary build/test (plus `retail` for
re-reading the finding this cites; no retail run and no audible device).
Spec: `specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stages
`### F41-B`/`### F41-C`; shared contract `docs/contracts/IDENTITY-CONTENT.md`.

## What was missing

#1438 offers M01's sound-group cues to `AudioSession`'s radio queue, and
`docs/findings/2026-10-01-f41-c-radio-queue-music-and-device-loss.md` recorded
the consequence: **nothing played them**.

* `AudioMixer::mix` consumed only `LoopOutcome`s, so a `RadioEvent::Started`
  never became a device voice.
* `AudibleDevice` (and its `LoopingVoice`) started only **infinite** loops, so a
  one-shot dialogue line would have repeated at the member's end instead of
  ending.
* `AudioSession::tick` had no production writer, so `advance_radio` advanced to a
  clock that never moved: a queued line never completed and the line behind it
  never started.

## What this task adds

### The session clock is the world's fixed-tick ledger

`cs_app::audio::loops::sync_audio_tick` copies `PhysicsTickLedger::ticks` into
`AudioSession::tick`. `AudioPlugin` registers it in `FixedPostUpdate`, after
`PhysicsSystems::StepSimulation` and **before** `mission_host_tick` — the same
system the host stamps ids from — so a cue the host offers in a tick is stamped
with the tick the host itself read. `advance_radio` syncs the same clock again on
its way to advancing the queue, so an `Update`-only world (a unit scenario that
has no ledger) keeps whatever tick it was given and the F41-C scenarios that
stamp the clock by hand are unaffected.

### The mixer has a radio path

`AudioMixer::mix_radio(&[RadioEvent], Option<&mut dyn AudioDevice>)`:

* `RadioEvent::Started { audible: true, .. }` opens a **non-looping** voice for
  the line's dialogue asset on `AudioBus::Radio`; `Completed`, `Interrupted` and
  `PlaybackLost` stop it.
* `RadioEvent::Started { audible: false, .. }` (the queue's own record that the
  output was gone) starts nothing and is named `RadioMixRefusal::Unvoiced`.
* A completion the queue reported as **voiced** with no voice behind it is named
  `RadioMixRefusal::NoVoice`, so a refused start cannot read as a line that
  played.
* A device refusal is named with the device's own stable code
  (`RadioMixRefusal::Device`, `sample_unavailable` for a member the library does
  not hold).
* The device is opened on demand like `mix`; a world the session declared lost
  passes `None`, so the pass reads and names every event **without** re-opening
  the output the session just declared gone. `PlaybackLost` is recorded in
  `RadioMixReport::lost` so the fact survives the drain.

`cs_app::audio::mix_session` drains the radio events after the loop pass and
records the result in `AudioMixReport::radio`. The loop pass is unchanged;
`AudioMixReport::passes` still counts only the passes that ran a loop mix.

Nothing in the radio path moves a clock or a queue: a line's length is
`RadioQueue`'s tick arithmetic, so a refused or lost device cannot change when
the line ends or when the next one starts (F41 non-negotiable behavior 2).

### The device has a one-shot voice

`AudioDevice` gained `start_one_shot` (default: `start_voice`, so a device that
cannot tell the two apart still plays the line and the queue still stops it).
`RecordingAudioDevice` logs `DeviceCommand::OneShotStarted` for it.
`AudibleDevice::start_one_shot` builds `LoopingVoice::one_shot`: the same pan
law, mono spread, channel interleave and sample tap as a loop, but the source
**ends** at the member's own end instead of repeating into it. `stop_voice` is
unchanged, so the queue can still cut a line short.

## Designed, not measured

* **Radio gain is unity** (`RADIO_GAIN = 1.0`). `RadioLine` carries no gain and
  F41-C records the original's radio mix (faders, limiter, bus levels) as
  unmeasured; applying a guessed fader would be a fidelity claim this workspace
  cannot make.
* **The radio bus is `AudioBus::Radio`**, the vocabulary's own name for radio
  dialogue and transmissions. No measurement states which bus the original mixed
  a line on.
* The pan law, the mono upmix and the one-shot end at the member's own end are
  project choices, as recorded in
  `docs/findings/2026-10-05-m01-lc-audible-audio-device.md`.
* Every priority, interrupt and queue-depth rule is #1438's and remains designed
  (F41-C records the original's as unmeasured).
* No original executable ran here, and **nothing here is `verified_original`**.

## What is still open (owner decision, by name)

The task text defers this explicitly: *"the decoded member must reach the sample
library … decide the measured identity rule with the owner rather than minting
ids ad hoc."* This task therefore **did not** invent that rule and did **not**
touch the loader. The state is:

* **The measured facts.** The M01 voice lines are members of
  `ZBD/soundsh.zbd`/`ZBD/soundsl.zbd` named `VO_<mission>_<speaker>_<index>.wav`
  (measured by #1438). F14-D.7 records that **no container family states a cue
  class**, so no dialogue or music row exists in the declared catalog
  (`docs/findings/2026-10-03-f14-d-7-sound-cue-collection.md`,
  `docs/findings/2026-10-06-m01-lc-audio-load.md`).
* **What #1438 does.** It derives each voice member's id as
  `ContentId::from_source(ContentKind::Dialogue, install_file_key("<container>/<member>"))`
  and names that id on the `RadioLine`.
* **What the loader does.** `cs_app::audio::samples::populate` decodes a delivered
  member only when its id is **declared by `AudioCatalog`** and its container
  index yields a member under the **same** derived id — and it derives member ids
  with `ContentKind::Sound`. A `Dialogue` id over the same key is therefore
  refused `sample_member_absent` today.
* **What the two ends never meet through.** #1438 reads the group table and
  decodes durations straight from the installation at content-preparation time;
  it does not go through the F15 closure or the `SampleLibrary`. The loader fills
  the library from the **delivered closure**, and no production closure builder
  exists on main.

So the rule the owner has to decide is: **which content kind names a spoken
voice member, and by which route those members enter the device's
`SampleLibrary`.** `ContentKind::Dialogue` (what #1438 mints) and
`ContentKind::Voice` (also in the vocabulary) are both plausible; both are
project conventions, since the installation states no cue class. The candidate
routes are (a) the loader registering a decoded member under the dialogue id over
the same `install_file_key` when the closure/catalog declares that id, (b) the
radio prepare step publishing the members it decoded into the library, or
(c) something the owner names instead.

**Until that is ruled, M01's lines are voiced only if something puts the dialogue
id's samples in the library.** The device then reports `sample_unavailable` by
name — which is the refusal this task added, not silence — but the owner still
cannot *hear* M01 until the identity rule and the population route are settled
and #1438 lands. This task is **not** evidence that a person hears M01's radio.

Affected content: every spoken line of every mission that plays through the radio
queue. Resolving task: the follow-up filed with this handover
(`M01-RADIO-SAMPLE-IDENTITY`); it needs the owner's rule first, and it gates any
audible M01 claim.

## Tests

`crates/cs_app/tests/accept_m01_radio_device.rs`, prefix
`accept_m01_radio_device_`, three scenarios. They drive production code only: the
real `AudioPlugin` in a real Bevy `App`, the real `RadioQueue`, the real
`AudioMixer`, the real device. The session and mixer are installed by hand
because the loading handoff that normally installs them is the #1438 cue path
that lands separately; the API driven (`AudioSession::enqueue_radio`) is the one
`offer_cues` calls.

1. `accept_m01_radio_device_a_queued_line_is_voiced_then_ends_and_the_next_begins`
   — a queued line becomes one `OneShotStarted` on `AudioBus::Radio`, is still
   sounding one tick short of its measured length, stops **at** that length, and
   the line behind it is voiced in the same frame; nothing is refused.
2. `accept_m01_radio_device_b_a_lost_device_keeps_the_queue_timing` — the output
   is lost mid-line through the production `device_lost` path; no voice survives,
   the line keeps its place and its subtitle, the loss is named against it, it is
   still running one tick short of its length, it completes **on the same tick**
   the voiced run does, the next line starts on that tick and is named
   `Unvoiced`, nothing plays while lost, and a line after `device_restored` is
   voiced again while the interrupted one is not re-voiced mid-sentence.
3. `accept_m01_radio_device_c_a_missing_sample_is_refused_by_name` — a real
   `AudibleDevice` whose library lacks the member refuses the line with the
   device's own `sample_unavailable`; the queue still completes the line on
   schedule; the voiced completion with no voice behind it is named `NoVoice`;
   and `LoopingVoice::one_shot` emits each mono frame's two values exactly once.

**Sensitivity, measured on this branch (remove, run, restore):**

| Change removed | Result |
| --- | --- |
| `mix_session` no longer calling `mix_radio` | 3 of 3 fail |
| `advance_radio` no longer reading the ledger | 3 of 3 fail |
| the radio path starting a loop (`start_voice`) instead of a one-shot | A and B fail (no `OneShotStarted` logged) |

## Checks

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_m01_radio_device_ --include-ignored` | 0 (3 run, 3 pass) |

## Identity of this work

Implementer: `bunny-alpha-2/bunny-alpha-2` (Rally #1461, implementation claim of
2026-10-15T04:06:11Z). This pass is `checked`, not `recreated`: no original
executable ran and no person heard anything. Review must be an independent pass
(fresh context, different agent instance or model); same-agent review is not
independent evidence and no agent review replaces the owner's approval.
