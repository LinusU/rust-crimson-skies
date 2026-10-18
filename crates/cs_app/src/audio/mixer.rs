//! The mixer and device consumer: what actually carries the session's loop
//! outcomes and spatial results to an output device (F41-B follow-up #445).
//!
//! Spec: `specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stage
//! `### F41-B`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! [`cs_sim::audio_events::AudioRouter`] answers *what* should be playing;
//! something has to answer *how*. This module is that something, in the ECS:
//!
//! * [`AudioOutput`] holds the one device this world mixes to. Until a
//!   hardware backend exists it is a
//!   [`cs_sim::audio_events::RecordingAudioDevice`], which records the commands
//!   and makes no sound — the mixer is a real consumer either way, and the log
//!   is what an evidence stage reads. It is **not** proof a user heard
//!   anything; F41 non-negotiable behavior 5 keeps those apart.
//! * [`AudioSpatial`] holds the listener pose and the attenuation policy the
//!   mix pass places emitters against.
//! * [`mix_session`] is the one pass: it drains the session's
//!   [`cs_sim::audio_events::LoopOutcome`]s into
//!   [`cs_sim::audio_events::AudioMixer`], builds one
//!   [`cs_sim::audio_events::EmitterMix`] per placed emitter from its pose and
//!   its [`EngineVoices`] level, and hands the result to the device.
//! * [`device_lost`] / [`device_restored`] are the device-failure path for the
//!   whole audio stack at once: the session forgets its loops, the mixer stops
//!   its voices, the device closes — and no simulation state moves, so a
//!   mission cannot stall on a missing output (F41 non-negotiable behavior 2).
//!   A restore re-issues the cue that was current through the loss to the
//!   mixer's next pass, so the music returns with the output (Rally #1552).
//!
//! The mixer never asks the device anything. That asymmetry is the whole
//! contract: the device may refuse every command, and the session, the radio
//! queue's tick timing and mission progression behave exactly as before.

use bevy::ecs::query::Has;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Res, ResMut};
use bevy::ecs::world::World;
use bevy::prelude::{GlobalTransform, Query};
use cs_sim::audio_events::{
    AudioDevice, AudioMixer, DeviceError, EmitterMix, Listener, MixerReport, MusicCue,
    MusicMixReport, RadioMixReport, RecordingAudioDevice, SpatialPolicy,
};
use cs_types::content::ContentId;
use cs_types::net::SessionId;

use super::AudioEmitterBinding;
use super::device::DeviceSampleLibrary;
use super::engine::EngineVoices;
use super::loops::AudioSession;
use super::music::{LazyMusicTracks, ensure_music_published};
use super::spawn::NonPositionalEmitter;

/// Resource: the one device this world mixes to.
///
/// `Default` is a closed [`RecordingAudioDevice`]: the mixer is wired either
/// way, and the log it keeps is readable by a test or a probe. A caller with a
/// real backend installs its own through
/// [`AudioPlugin::with_device`](super::AudioPlugin::with_device).
///
/// The device is opened by the first [`mix_session`] that has something to
/// play, and is closed again by [`device_lost`] until a caller calls
/// [`device_restored`].
#[derive(Resource, Debug)]
pub struct AudioOutput(Box<dyn AudioDevice>);

impl Default for AudioOutput {
    fn default() -> Self {
        Self(Box::new(RecordingAudioDevice::new()))
    }
}

impl AudioOutput {
    /// Wraps `device` as this world's output.
    #[must_use]
    pub fn new(device: Box<dyn AudioDevice>) -> Self {
        Self(device)
    }

    /// Read-only access, for diagnostics and probes.
    #[must_use]
    pub fn device(&self) -> &dyn AudioDevice {
        self.0.as_ref()
    }

    /// Mutable access, for the mixer and for the device-failure path.
    pub fn device_mut(&mut self) -> &mut dyn AudioDevice {
        self.0.as_mut()
    }
}

/// Resource: the mixer's own state.
///
/// [`AudioMixer`] is `cs_sim`'s, and `cs_sim` carries no Bevy dependency, so
/// the world holds it inside this resource. The pairing with [`AudioSession`]
/// is what the handoff keeps in step: both are installed together for one
/// delivered load and replaced together on a reload.
#[derive(Resource, Clone, Debug)]
pub struct AudioMixing(pub AudioMixer);

impl AudioMixing {
    /// An empty mixer for `session`.
    #[must_use]
    pub fn new(session: SessionId) -> Self {
        Self(AudioMixer::new(session))
    }

    /// The mixer.
    #[must_use]
    pub const fn mixer(&self) -> &AudioMixer {
        &self.0
    }

    /// The mixer, mutably.
    pub const fn mixer_mut(&mut self) -> &mut AudioMixer {
        &mut self.0
    }
}

/// Resource: the listener pose and the attenuation policy the mix pass places
/// emitters against.
///
/// The listener moves with the player's aircraft (or the camera), so a caller
/// overwrites it as the aircraft flies; the policy is the mission's and stays
/// put. Both are validated at construction ([`SpatialPolicy::try_new`],
/// [`Listener::try_new`]), so no mix pass ever sees a corrupt one.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct AudioSpatial {
    /// The designed attenuation law.
    pub policy: SpatialPolicy,
    /// Where the listener is and which way is right.
    pub listener: Listener,
}

impl AudioSpatial {
    /// Assembles the spatial configuration from two validated halves.
    #[must_use]
    pub const fn new(policy: SpatialPolicy, listener: Listener) -> Self {
        Self { policy, listener }
    }

    /// Moves the listener, keeping the policy.
    pub const fn at(&mut self, listener: Listener) {
        self.listener = listener;
    }
}

/// Resource: what the last mix pass did.
///
/// Kept in the world rather than returned, because the device is only reachable
/// through systems: a test asserts on this instead of on a channel the mixer
/// never opens.
#[derive(Resource, Clone, Debug, Default)]
pub struct AudioMixReport {
    /// The last pass the mixer ran.
    pub last: MixerReport,
    /// The last radio pass the mixer ran.
    ///
    /// Kept apart from [`Self::last`] because radio lines are not emitters: a
    /// line's start, stop and refusal are keyed by the line's own event id, and
    /// a report that folded them into the emitter lists could not say which
    /// dialogue line was silent.
    pub radio: RadioMixReport,
    /// The last music pass the mixer ran: the track voices the session's
    /// music verdicts started and stopped, and every request it could not
    /// voice, by name.
    pub music: MusicMixReport,
    /// The music tracks a verdict selected that would not load, with the
    /// loader's reason (Rally #1551): the device refuses each of them
    /// `sample_unavailable`, and this says why.
    pub music_unloaded: Vec<(ContentId, String)>,
    /// How many passes have run since this world was built.
    pub passes: u64,
    /// How many emitter bindings carried no pose in the last pass, and so kept
    /// whatever mix they last had.
    ///
    /// A non-zero count is not a failure — it says the world has an audio
    /// emitter nothing has placed — but it is exactly what a silently mixing
    /// engine looks like from the outside, so it is reported rather than
    /// hidden. An emitter with no `GlobalTransform` is *not* treated as being
    /// at the listener's own position: that would be a guessed placement.
    pub unplaced: usize,
    /// How many pose-less emitter bindings are **declared non-positional**
    /// ([`NonPositionalEmitter`], F41-B3): the world's ambient loop. They are
    /// not placed and not counted in [`Self::unplaced`]; they keep the mix
    /// their voice started with by design.
    pub non_positional: usize,
}

/// One mix pass: the session's outcomes become device commands.
///
/// Runs after [`super::sync_emitter_loops`] so the loops of this frame are the
/// ones the router holds. A world with no session — nothing has been loaded
/// into it — or with no mixer, no device or no spatial configuration mixes
/// nothing and records nothing: audio that was never loaded is silence, not an
/// error.
///
/// A world whose device is **lost** mixes no loop, even though it still has a
/// session, a mixer and an output. [`AudioMixer::mix`] opens a closed device on
/// demand, so without this gate the frame after [`device_lost`] would re-open
/// the very device that was just declared gone and the loss would be invisible
/// to every observer of the device log. The session is the authority on whether
/// an output exists — it is what [`device_lost`] clears and [`device_restored`]
/// restores — so the gate lives here, above the mixer, and the device stays
/// closed until a caller restores it.
///
/// A lost output still gets its **radio** events read: the queue keeps its tick
/// timing through a loss (F41 non-negotiable behavior 2), so a line that started
/// unvoiced or completed unvoiced is named in [`AudioMixReport::radio`] rather
/// than left to pile up in the session until the device returns.
pub fn mix_session(
    mut mixing: Option<ResMut<AudioMixing>>,
    mut output: Option<ResMut<AudioOutput>>,
    mut session: Option<ResMut<AudioSession>>,
    spatial: Option<Res<AudioSpatial>>,
    voices: Res<EngineVoices>,
    emitters: Query<(
        &AudioEmitterBinding,
        Option<&GlobalTransform>,
        Has<NonPositionalEmitter>,
    )>,
    mut report: ResMut<AudioMixReport>,
) {
    let (Some(mixing), Some(output), Some(session), Some(spatial)) = (
        mixing.as_mut(),
        output.as_mut(),
        session.as_mut(),
        spatial.as_deref(),
    ) else {
        return;
    };
    if !session.device_available() {
        // The output is gone: the session's own device-lost path stopped the
        // voices, but the radio events still have to be read so a line that
        // stayed silent is named rather than left sitting in the queue. No
        // device is handed to the radio pass, so this cannot re-open the output
        // the session just declared lost. No loop pass runs, so `passes` does
        // not move either — the world with no usable output records a mix only
        // where one happened.
        let lines = session.drain_radio();
        report.radio = mixing.mixer_mut().mix_radio(&lines, None);
        let music = session.drain_music();
        report.music = mixing.mixer_mut().mix_music(&music, None);
        return;
    }
    let mut placed = Vec::with_capacity(emitters.iter().len());
    let mut unplaced = 0;
    let mut non_positional = 0;
    for (binding, transform, declared_non_positional) in &emitters {
        let Some(transform) = transform else {
            if declared_non_positional {
                non_positional += 1;
            } else {
                unplaced += 1;
            }
            continue;
        };
        placed.push(EmitterMix {
            emitter: binding.emitter,
            position_m: transform.translation().as_dvec3().to_array(),
            level: voices.level(&binding.emitter),
        });
    }
    // The drain comes first so the outcomes are taken before the router is
    // borrowed for the pass: the mixer reads the router the *end* of this frame
    // holds, which is the loops `sync_emitter_loops` just bound.
    let outcomes = session.drain_outcomes();
    report.last = mixing.mixer_mut().mix(
        &session.router,
        &outcomes,
        &placed,
        &spatial.listener,
        &spatial.policy,
        output.device_mut(),
    );
    // Radio follows the loop pass: `mix` opens the device if it was closed, so
    // a line started this frame reaches an open output, and a device that
    // refused to open refuses the line too and names it. The events are taken
    // after the loop pass for the same reason the outcomes are: they are what
    // `advance_radio` produced this frame.
    let lines = session.drain_radio();
    report.radio = mixing
        .mixer_mut()
        .mix_radio(&lines, Some(output.device_mut()));
    let music = session.drain_music();
    report.music = mixing
        .mixer_mut()
        .mix_music(&music, Some(output.device_mut()));
    report.passes += 1;
    report.unplaced = unplaced;
    report.non_positional = non_positional;
}

/// Decodes the music track a pending verdict starts, the first time it is
/// selected, and hands it to the device's library (Rally #1551).
///
/// Runs between the radio pass and [`mix_session`], which drains the verdicts
/// this reads. A world with no session, library or lazy loader does nothing.
/// A track that will not load is named in
/// [`AudioMixReport::music_unloaded`]; the device then refuses it
/// `sample_unavailable`.
pub fn publish_requested_music(
    session: Option<Res<AudioSession>>,
    library: Option<Res<DeviceSampleLibrary>>,
    lazy_music: Option<Res<LazyMusicTracks>>,
    mut report: ResMut<AudioMixReport>,
) {
    let (Some(session), Some(library), Some(lazy_music)) = (session, library, lazy_music) else {
        return;
    };
    let pending = session.pending_music();
    if pending.is_empty() {
        return;
    }
    report.music_unloaded =
        ensure_music_published(library.library().as_ref(), &lazy_music, pending);
}

/// The output device is lost: the session stops and **remembers** every loop,
/// the mixer stops every voice as
/// [`cs_sim::audio_events::EmitterStopReason::DeviceLost`] and the device
/// closes.
///
/// Nothing else moves. The radio queue keeps its tick timing, the mission keeps
/// its state, and no simulation system consults the device — which is F41
/// non-negotiable behavior 2 as a property of the code rather than as a
/// promise.
///
/// Total and idempotent: losing a device that is already gone changes nothing.
pub fn device_lost(world: &mut World) {
    // The two halves are independent, and each is applied even if the other is
    // absent: a session with no mixer (nothing loaded) must still forget its
    // loops, and a mixer with no session must still stop what it is sounding.
    let session = world.remove_resource::<AudioSession>();
    if let Some(mut session) = session {
        session.device_lost();
        world.insert_resource(session);
    }
    let mut mixing = world.remove_resource::<AudioMixing>();
    let mut output = world.remove_resource::<AudioOutput>();
    let lost = match (mixing.as_mut(), output.as_mut()) {
        (Some(mixing), Some(output)) => Some(mixing.mixer_mut().device_lost(output.device_mut())),
        // No mixer to stop through, but the output is still there and still
        // open: the device is the thing that was lost, so it is closed either
        // way.
        (_, Some(output)) => {
            output.device_mut().close();
            None
        }
        _ => None,
    };
    if let Some(mixing) = mixing {
        world.insert_resource(mixing);
    }
    if let Some(output) = output {
        world.insert_resource(output);
    }
    if let Some(lost) = lost
        && let Some(mut report) = world.remove_resource::<AudioMixReport>()
    {
        report.last = lost;
        world.insert_resource(report);
    }
}

/// The device is back: re-open it, let the session re-bind every loop it
/// remembered, and re-issue the current music cue to the mixer's next pass,
/// returning that cue if one was current.
///
/// The music re-issue is the session's
/// ([`AudioSession::device_restored`], Rally #1552): it queues the director's
/// replay verdict, so the **one** music mix pass both decodes the track if it
/// is not yet published and starts it on the music bus — the same path a
/// mission's own state request takes. The returned cue is that same cue, for a
/// caller that wants to name it; there is nothing left to re-issue here.
///
/// **Designed, not measured:** restarting the current cue after an output
/// device returns is this build's choice; the original's behavior is
/// unmeasured (see
/// [`MusicDirector::device_restored`](cs_sim::audio_events::MusicDirector::device_restored)).
///
/// # Errors
///
/// [`DeviceError`] when the device cannot be re-opened. The session's loops
/// stay remembered and its radio timing untouched, so the caller may retry.
pub fn device_restored(world: &mut World) -> Result<Option<MusicCue>, DeviceError> {
    let mut mixing = world.remove_resource::<AudioMixing>();
    let mut output = world.remove_resource::<AudioOutput>();
    let opened = match (mixing.as_mut(), output.as_mut()) {
        (Some(mixing), Some(output)) => mixing.mixer_mut().device_restored(output.device_mut()),
        // No mixer to reopen through, but an output that is still there is
        // opened directly: a restore that only worked when a mixer happened to
        // exist would leave the device closed in every other world.
        (_, Some(output)) => output.device_mut().open(),
        _ => Ok(()),
    };
    if let Some(mixing) = mixing {
        world.insert_resource(mixing);
    }
    if let Some(output) = output {
        world.insert_resource(output);
    }
    opened?;
    let Some(mut session) = world.remove_resource::<AudioSession>() else {
        return Ok(None);
    };
    let cue = session.device_restored();
    world.insert_resource(session);
    Ok(cue)
}
