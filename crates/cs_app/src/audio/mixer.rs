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
//!
//! The mixer never asks the device anything. That asymmetry is the whole
//! contract: the device may refuse every command, and the session, the radio
//! queue's tick timing and mission progression behave exactly as before.

use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Res, ResMut};
use bevy::ecs::world::World;
use bevy::prelude::{GlobalTransform, Query};
use cs_sim::audio_events::{
    AudioDevice, AudioMixer, DeviceError, EmitterMix, Listener, MixerReport, MusicCue,
    RecordingAudioDevice, SpatialPolicy,
};

use super::AudioEmitterBinding;
use super::engine::EngineVoices;
use super::loops::AudioSession;

/// Resource: the one device this world mixes to.
///
/// `Default` is a closed [`RecordingAudioDevice`]: the mixer is wired either
/// way, and the log it keeps is readable by a test or a probe. A caller with a
/// real backend installs its own through
/// [`AudioPlugin::with_device`](super::AudioPlugin::with_device).
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
    pub fn new(session: u64) -> Self {
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
}

/// One mix pass: the session's outcomes become device commands.
///
/// Runs after [`super::sync_emitter_loops`] so the loops of this frame are the
/// ones the router holds. A world with no session — nothing has been loaded
/// into it — or with no mixer, no device or no spatial configuration mixes
/// nothing and records nothing: audio that was never loaded is silence, not an
/// error.
pub fn mix_session(
    mut mixing: Option<ResMut<AudioMixing>>,
    mut output: Option<ResMut<AudioOutput>>,
    mut session: Option<ResMut<AudioSession>>,
    spatial: Option<Res<AudioSpatial>>,
    voices: Res<EngineVoices>,
    emitters: Query<(&AudioEmitterBinding, Option<&GlobalTransform>)>,
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
    let mut placed = Vec::with_capacity(emitters.iter().len());
    let mut unplaced = 0;
    for (binding, transform) in &emitters {
        let Some(transform) = transform else {
            unplaced += 1;
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
    report.passes += 1;
    report.unplaced = unplaced;
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

/// The device is back: re-open it and let the session re-bind every loop it
/// remembered, returning the music cue to restart if one was current.
///
/// The returned cue is the caller's to re-issue: the director keeps it current
/// through a device loss, so a caller that cannot start music for a reason of
/// its own can say so instead of having the audio stack guess.
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
