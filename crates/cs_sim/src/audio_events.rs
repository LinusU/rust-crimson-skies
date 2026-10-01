//! Audio event identity, one-shot dedup and loop-emitter lifecycle (F41-A).
//!
//! Spec: `specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stage
//! `### F41-A`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! This module is the **runtime half** of the audio contract — the typed
//! records the simulation produces and the small, bounded router that turns
//! them into playback commands. The provenance-carrying declared form (buses,
//! playback metadata, decoded-asset references) lives in `cs_content::audio`,
//! which `cs_sim` cannot see; [`AudioAssetSpec`] is the runtime routing record
//! the conversion boundary (`cs_app::audio`) lowers a declared record into.
//!
//! # Event identity
//!
//! [`AudioEventId`] is the audio-scoped realization of the contract's
//! `EventId(session, tick, producer, sequence)` shape. `cs_types` does not
//! implement a shared `SessionId`/`EventId` type yet (the same gap
//! `cs_sim::damage` and `cs_sim::animated_object` record), so this module
//! carries the four fields itself rather than guessing a shared one. Session
//! qualification is load-bearing: an event from a previous session generation
//! can never alias a live one ([`AudioRouter`] refuses it by name).
//!
//! # One-shot dedup (F41 non-negotiable behavior 3)
//!
//! "Repeated simulation/network events cannot duplicate one-shot audio." A
//! producer stamps a strictly increasing `sequence` on its own events, so the
//! router keeps only the highest accepted sequence per `(session, producer)` —
//! a bounded `O(producers)` ledger, not a set that grows with mission length.
//! Re-delivering any event at or below that mark is a
//! [`OneShotOutcome::SuppressedDuplicate`]; the same event replayed twice
//! therefore produces exactly one [`OneShotOutcome::Accepted`].
//!
//! # Loop emitters (F41 non-negotiable behavior 3)
//!
//! A loop is bound to an [`AudioEmitterId`] and stops when the emitter is
//! [`EmitterStopReason::Despawned`], when a new loop is bound to the same
//! emitter ([`EmitterStopReason::EmitterSwapped`]), when the declared
//! [`PausePolicy`] suspends playback, or when the physical device is lost. The
//! registry holds at most one loop per emitter, so a swap can never leak the
//! previous loop.
//!
//! # What is designed and what is unknown
//!
//! Every vocabulary value, bound and rule here is newly authored project
//! design. The original engine's mixing, doppler, attenuation, loop semantics
//! and priority/interrupt handling are unmeasured (see the F06 and F41-A
//! findings files); this stage claims no original behavior.

use std::collections::BTreeMap;
use std::fmt;

use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};

/// Maximum validated linear gain; the same designed ceiling as
/// `cs_content::audio::MAX_AUDIO_GAIN`.
pub const MAX_AUDIO_GAIN: f64 = 8.0;

// ----------------------------------------------------------------- buses ---

/// One of the seven runtime mix buses.
///
/// This is the runtime vocabulary, parallel to
/// `cs_content::audio::AudioBus` in the same way `animated_object::Visibility`
/// parallels `cs_content::scene::NodeVisibility`: `cs_sim` cannot depend on
/// `cs_content`, so the conversion boundary maps one onto the other field for
/// field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AudioBus {
    /// Engine and propeller loops.
    Engine,
    /// Gun and weapon fire.
    Weapons,
    /// Impact and collision sounds.
    Impacts,
    /// Ambient wind, weather and environment.
    Environment,
    /// Authored music and transitions.
    Music,
    /// Radio dialogue and transmissions.
    Radio,
    /// User-interface feedback.
    Ui,
}

impl AudioBus {
    /// Every bus, in a stable order.
    pub const ALL: &'static [AudioBus] = &[
        Self::Engine,
        Self::Weapons,
        Self::Impacts,
        Self::Environment,
        Self::Music,
        Self::Radio,
        Self::Ui,
    ];

    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Engine => "engine",
            Self::Weapons => "weapons",
            Self::Impacts => "impacts",
            Self::Environment => "environment",
            Self::Music => "music",
            Self::Radio => "radio",
            Self::Ui => "ui",
        }
    }

    /// Looks a bus up by its label; `None` for an unknown bus.
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|bus| bus.label() == label)
    }
}

impl fmt::Display for AudioBus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Whether a cue plays once or loops, in runtime form.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlaybackMode {
    /// Plays once per accepted event.
    OneShot,
    /// Loops until its emitter stops it.
    Loop,
}

impl fmt::Display for PlaybackMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::OneShot => "one_shot",
            Self::Loop => "loop",
        })
    }
}

// -------------------------------------------------------------- identity ---

/// One producer's event identity: `EventId(session, tick, producer, sequence)`.
///
/// `producer` is the serial of the system that emitted the cue (a weapon
/// mount, an engine, the radio queue) and `sequence` orders that producer's own
/// events. Ordering by the full id is the declared deterministic order the
/// router applies within a tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AudioEventId {
    /// The session generation the event was produced in.
    pub session: u64,
    /// The simulation tick the event belongs to.
    pub tick: Tick,
    /// The producing system's serial.
    pub producer: u32,
    /// The event's sequence within its producer.
    pub sequence: u32,
}

impl fmt::Display for AudioEventId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "audio event {}:{}:{}:{}",
            self.session, self.tick.0, self.producer, self.sequence
        )
    }
}

/// One loop emitter inside one session generation; the contract's
/// `ActorId { session, serial }` shape applied to audio.
///
/// `cs_sim::damage::ActorId` is the same pair, but the audio contract must not
/// depend on the damage module's vocabulary: F41-B binds an actor to an emitter
/// explicitly, so the two identities stay independently testable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AudioEmitterId {
    /// The session generation the emitter belongs to.
    pub session: u64,
    /// The emitter's serial within that session.
    pub serial: u64,
}

impl fmt::Display for AudioEmitterId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "audio emitter {}:{}", self.session, self.serial)
    }
}

// ---------------------------------------------------------------- inputs ---

/// Why a runtime audio input was refused.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AudioEventError {
    /// The gain was NaN or infinite.
    NonFiniteGain {
        /// The rejected value.
        value: f64,
    },
    /// The gain was negative.
    NegativeGain {
        /// The rejected value.
        value: f64,
    },
    /// The gain exceeded [`MAX_AUDIO_GAIN`].
    GainAboveMaximum {
        /// The rejected value.
        value: f64,
    },
    /// The asset id is not a `sound`, `music` or `dialogue` id.
    NotAnAudioAsset {
        /// The kind it actually names.
        kind: ContentKind,
    },
    /// A helper was asked for the wrong playback form.
    WrongPlaybackMode {
        /// The form the caller asked for.
        expected: PlaybackMode,
        /// The form the spec declares.
        found: PlaybackMode,
    },
}

impl fmt::Display for AudioEventError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFiniteGain { value } => write!(f, "audio gain {value} is not finite"),
            Self::NegativeGain { value } => write!(f, "audio gain {value} is negative"),
            Self::GainAboveMaximum { value } => {
                write!(
                    f,
                    "audio gain {value} is above the maximum {MAX_AUDIO_GAIN}"
                )
            }
            Self::NotAnAudioAsset { kind } => {
                write!(
                    f,
                    "audio asset id names a {kind}, not a sound, music or dialogue"
                )
            }
            Self::WrongPlaybackMode { expected, found } => {
                write!(f, "expected a {expected} cue but the spec declares {found}")
            }
        }
    }
}

impl std::error::Error for AudioEventError {}

fn validate_gain(gain: f64) -> Result<(), AudioEventError> {
    if !gain.is_finite() {
        return Err(AudioEventError::NonFiniteGain { value: gain });
    }
    if gain < 0.0 {
        return Err(AudioEventError::NegativeGain { value: gain });
    }
    if gain > MAX_AUDIO_GAIN {
        return Err(AudioEventError::GainAboveMaximum { value: gain });
    }
    Ok(())
}

fn validate_asset(asset: &ContentId) -> Result<(), AudioEventError> {
    if matches!(
        asset.kind(),
        ContentKind::Sound | ContentKind::Music | ContentKind::Dialogue
    ) {
        Ok(())
    } else {
        Err(AudioEventError::NotAnAudioAsset { kind: asset.kind() })
    }
}

/// One immutable one-shot cue: the typed input a weapon, impact or dialogue
/// producer hands [`AudioRouter::play_one_shot`].
#[derive(Clone, Debug, PartialEq)]
pub struct OneShotEvent {
    /// The cue's stable identity and dedup key.
    pub id: AudioEventId,
    /// The audio asset to play.
    pub asset: ContentId,
    /// The bus it is mixed on.
    pub bus: AudioBus,
    /// The validated linear gain.
    pub gain: f64,
}

impl OneShotEvent {
    /// Builds a one-shot cue, refusing a non-audio asset or a corrupt gain.
    ///
    /// # Errors
    ///
    /// [`AudioEventError::NotAnAudioAsset`], [`AudioEventError::NonFiniteGain`],
    /// [`AudioEventError::NegativeGain`] or
    /// [`AudioEventError::GainAboveMaximum`].
    pub fn try_new(
        id: AudioEventId,
        asset: ContentId,
        bus: AudioBus,
        gain: f64,
    ) -> Result<Self, AudioEventError> {
        validate_asset(&asset)?;
        validate_gain(gain)?;
        Ok(Self {
            id,
            asset,
            bus,
            gain,
        })
    }
}

/// One loop bound to an emitter: the typed input a continuous producer (an
/// engine, a wind layer) hands [`AudioRouter::start_loop`].
#[derive(Clone, Debug, PartialEq)]
pub struct LoopBinding {
    /// The emitter the loop plays on.
    pub emitter: AudioEmitterId,
    /// The bind event's identity; when a later binding replaces this loop the
    /// router reports this id as the stopped loop's id.
    pub id: AudioEventId,
    /// The audio asset to loop.
    pub asset: ContentId,
    /// The bus it is mixed on.
    pub bus: AudioBus,
    /// The validated linear gain.
    pub gain: f64,
}

impl LoopBinding {
    /// Builds a loop binding, refusing a non-audio asset or a corrupt gain.
    ///
    /// # Errors
    ///
    /// [`AudioEventError::NotAnAudioAsset`], [`AudioEventError::NonFiniteGain`],
    /// [`AudioEventError::NegativeGain`] or
    /// [`AudioEventError::GainAboveMaximum`].
    pub fn try_new(
        emitter: AudioEmitterId,
        id: AudioEventId,
        asset: ContentId,
        bus: AudioBus,
        gain: f64,
    ) -> Result<Self, AudioEventError> {
        validate_asset(&asset)?;
        validate_gain(gain)?;
        Ok(Self {
            emitter,
            id,
            asset,
            bus,
            gain,
        })
    }
}

/// The runtime routing record a declared `cs_content::audio::AudioAssetRecord`
/// is lowered into.
///
/// It carries exactly what the router needs — asset, bus, gain and playback
/// form — with the validated invariants preserved across the boundary.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioAssetSpec {
    asset: ContentId,
    bus: AudioBus,
    gain: f64,
    mode: PlaybackMode,
}

impl AudioAssetSpec {
    /// Validates and assembles a runtime audio asset spec.
    ///
    /// # Errors
    ///
    /// [`AudioEventError::NotAnAudioAsset`] for a non-audio id and the gain
    /// errors of [`validate_gain`].
    pub fn try_new(
        asset: ContentId,
        bus: AudioBus,
        gain: f64,
        mode: PlaybackMode,
    ) -> Result<Self, AudioEventError> {
        validate_asset(&asset)?;
        validate_gain(gain)?;
        Ok(Self {
            asset,
            bus,
            gain,
            mode,
        })
    }

    /// The audio asset id.
    #[must_use]
    pub fn asset(&self) -> &ContentId {
        &self.asset
    }

    /// The bus.
    #[must_use]
    pub const fn bus(&self) -> AudioBus {
        self.bus
    }

    /// The validated linear gain.
    #[must_use]
    pub const fn gain(&self) -> f64 {
        self.gain
    }

    /// The playback form.
    #[must_use]
    pub const fn mode(&self) -> PlaybackMode {
        self.mode
    }

    /// Builds a one-shot cue from this spec.
    ///
    /// # Errors
    ///
    /// [`AudioEventError::WrongPlaybackMode`] when the spec is not a one-shot.
    pub fn one_shot_event(&self, id: AudioEventId) -> Result<OneShotEvent, AudioEventError> {
        if self.mode != PlaybackMode::OneShot {
            return Err(AudioEventError::WrongPlaybackMode {
                expected: PlaybackMode::OneShot,
                found: self.mode,
            });
        }
        OneShotEvent::try_new(id, self.asset.clone(), self.bus, self.gain)
    }

    /// Builds a loop binding from this spec.
    ///
    /// # Errors
    ///
    /// [`AudioEventError::WrongPlaybackMode`] when the spec is not a loop.
    pub fn loop_binding(
        &self,
        emitter: AudioEmitterId,
        id: AudioEventId,
    ) -> Result<LoopBinding, AudioEventError> {
        if self.mode != PlaybackMode::Loop {
            return Err(AudioEventError::WrongPlaybackMode {
                expected: PlaybackMode::Loop,
                found: self.mode,
            });
        }
        LoopBinding::try_new(emitter, id, self.asset.clone(), self.bus, self.gain)
    }
}

// -------------------------------------------------------------- outcomes ---

/// Why an active loop stopped.
///
/// The four reasons are distinct because the *consumer* differs: a despawn is
/// entity teardown, a swap is a new bound loop, a pause is gameplay state and a
/// device loss is an output failure. Collapsing them would make "the engine
/// went quiet" indistinguishable from "the player paused".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EmitterStopReason {
    /// The emitter's entity left the world.
    Despawned,
    /// A new loop was bound to the same emitter.
    EmitterSwapped,
    /// The declared pause policy suspends playback.
    Paused,
    /// The physical audio device was lost.
    DeviceLost,
}

impl EmitterStopReason {
    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Despawned => "despawned",
            Self::EmitterSwapped => "emitter_swapped",
            Self::Paused => "paused",
            Self::DeviceLost => "device_lost",
        }
    }
}

impl fmt::Display for EmitterStopReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// What a session does with its loops while paused.
///
/// A designed policy, not an observed one: the original engine's pause
/// behavior is unmeasured, so the caller declares whether a paused session
/// suspends its loops (freezing the mix) or keeps them running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PausePolicy {
    /// Stop every active loop with [`EmitterStopReason::Paused`].
    Suspend,
    /// Leave every active loop running.
    Continue,
}

/// What one one-shot cue produced.
#[derive(Clone, Debug, PartialEq)]
pub enum OneShotOutcome {
    /// The cue is accepted and must be played once.
    Accepted {
        /// The accepted event.
        id: AudioEventId,
        /// The asset to play.
        asset: ContentId,
        /// The bus to play it on.
        bus: AudioBus,
        /// The validated linear gain.
        gain: f64,
    },
    /// The event was already accepted under this id; nothing may play.
    SuppressedDuplicate {
        /// The duplicate event.
        id: AudioEventId,
    },
    /// The event belongs to another session generation.
    RefusedForeignSession {
        /// The refused event.
        id: AudioEventId,
        /// The router's session.
        session: u64,
    },
}

/// What one loop action produced.
#[derive(Clone, Debug, PartialEq)]
pub enum LoopOutcome {
    /// A loop was bound to an idle emitter.
    Started {
        /// The emitter.
        emitter: AudioEmitterId,
    },
    /// A new loop replaced the emitter's active loop, which stopped as
    /// [`EmitterStopReason::EmitterSwapped`].
    Swapped {
        /// The emitter.
        emitter: AudioEmitterId,
        /// The id of the loop that was stopped.
        stopped: AudioEventId,
    },
    /// An active loop stopped for `reason`.
    Stopped {
        /// The emitter.
        emitter: AudioEmitterId,
        /// Why it stopped.
        reason: EmitterStopReason,
    },
    /// A stop was requested for an emitter with no active loop.
    NotActive {
        /// The emitter.
        emitter: AudioEmitterId,
        /// Why the stop was requested.
        reason: EmitterStopReason,
    },
    /// The emitter belongs to another session generation.
    RefusedForeignSession {
        /// The refused emitter.
        emitter: AudioEmitterId,
        /// The router's session.
        session: u64,
    },
}

// --------------------------------------------------------------- router ----

/// A per-session audio router: the bounded one-shot dedup ledger and the
/// loop-emitter registry.
///
/// The router is bound to one session generation at construction. It owns no
/// game state and no device; it answers what to play and what to stop, which
/// is the typed input F41-B's actual mixer consumes.
#[derive(Clone, Debug)]
pub struct AudioRouter {
    session: u64,
    /// Highest accepted one-shot sequence per producer.
    highest_sequence: BTreeMap<u32, u32>,
    /// At most one active loop per emitter.
    loops: BTreeMap<AudioEmitterId, LoopBinding>,
}

impl AudioRouter {
    /// A router for `session`, with no cues seen.
    #[must_use]
    pub fn new(session: u64) -> Self {
        Self {
            session,
            highest_sequence: BTreeMap::new(),
            loops: BTreeMap::new(),
        }
    }

    /// The session generation this router serves.
    #[must_use]
    pub const fn session(&self) -> u64 {
        self.session
    }

    /// Accepts a one-shot cue at most once per event id.
    ///
    /// An event from another session is refused; an event whose
    /// `(producer, sequence)` has already been accepted is suppressed. The
    /// ledger keeps one `u32` per producer, so it does not grow with the
    /// number of cues played.
    pub fn play_one_shot(&mut self, event: &OneShotEvent) -> OneShotOutcome {
        if event.id.session != self.session {
            return OneShotOutcome::RefusedForeignSession {
                id: event.id,
                session: self.session,
            };
        }
        if self
            .highest_sequence
            .get(&event.id.producer)
            .is_some_and(|accepted| event.id.sequence <= *accepted)
        {
            return OneShotOutcome::SuppressedDuplicate { id: event.id };
        }
        self.highest_sequence
            .insert(event.id.producer, event.id.sequence);
        OneShotOutcome::Accepted {
            id: event.id,
            asset: event.asset.clone(),
            bus: event.bus,
            gain: event.gain,
        }
    }

    /// Binds a loop to its emitter, replacing any loop already bound there.
    ///
    /// A replacement reports [`LoopOutcome::Swapped`] carrying the stopped
    /// loop's id, so the consumer can pair the two halves of the swap.
    pub fn start_loop(&mut self, binding: &LoopBinding) -> LoopOutcome {
        if binding.emitter.session != self.session || binding.id.session != self.session {
            return LoopOutcome::RefusedForeignSession {
                emitter: binding.emitter,
                session: self.session,
            };
        }
        match self.loops.insert(binding.emitter, binding.clone()) {
            Some(previous) => LoopOutcome::Swapped {
                emitter: binding.emitter,
                stopped: previous.id,
            },
            None => LoopOutcome::Started {
                emitter: binding.emitter,
            },
        }
    }

    /// Stops the loop bound to `emitter` for `reason`.
    ///
    /// A stop for an emitter with no active loop is reported as
    /// [`LoopOutcome::NotActive`] rather than silently ignored, so a
    /// double-despawn is visible.
    pub fn stop_loop(
        &mut self,
        emitter: &AudioEmitterId,
        reason: EmitterStopReason,
    ) -> LoopOutcome {
        if emitter.session != self.session {
            return LoopOutcome::RefusedForeignSession {
                emitter: *emitter,
                session: self.session,
            };
        }
        match self.loops.remove(emitter) {
            Some(_) => LoopOutcome::Stopped {
                emitter: *emitter,
                reason,
            },
            None => LoopOutcome::NotActive {
                emitter: *emitter,
                reason,
            },
        }
    }

    /// The loop bound to `emitter`, if any.
    #[must_use]
    pub fn active_loop(&self, emitter: &AudioEmitterId) -> Option<&LoopBinding> {
        self.loops.get(emitter)
    }

    /// How many loops are active.
    #[must_use]
    pub fn active_loop_count(&self) -> usize {
        self.loops.len()
    }

    /// Applies the pause policy: [`PausePolicy::Suspend`] stops every active
    /// loop as [`EmitterStopReason::Paused`] and returns the outcomes in
    /// emitter order; [`PausePolicy::Continue`] changes nothing and returns an
    /// empty list.
    pub fn apply_pause(&mut self, policy: PausePolicy) -> Vec<LoopOutcome> {
        match policy {
            PausePolicy::Continue => Vec::new(),
            PausePolicy::Suspend => self.stop_all(EmitterStopReason::Paused),
        }
    }

    /// Device loss: stop every active loop as
    /// [`EmitterStopReason::DeviceLost`], in emitter order.
    ///
    /// The call is total — losing the device twice simply returns nothing the
    /// second time — so mission progression never depends on a device
    /// (F41 non-negotiable behavior 2).
    pub fn device_lost(&mut self) -> Vec<LoopOutcome> {
        self.stop_all(EmitterStopReason::DeviceLost)
    }

    fn stop_all(&mut self, reason: EmitterStopReason) -> Vec<LoopOutcome> {
        let emitters: Vec<AudioEmitterId> = self.loops.keys().copied().collect();
        emitters
            .into_iter()
            .map(|emitter| {
                self.loops.remove(&emitter);
                LoopOutcome::Stopped { emitter, reason }
            })
            .collect()
    }
}

// -------------------------------------------------------------- spatial ----

/// Why a spatial input was refused.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SpatialError {
    /// A position or axis component was NaN or infinite.
    NonFinite,
    /// The reference distance was not a positive finite number.
    BadReferenceDistance {
        /// The rejected value.
        value: f64,
    },
    /// The cutoff distance was not finite and strictly beyond the reference.
    BadMaxDistance {
        /// The rejected value.
        value: f64,
    },
    /// The listener's right axis was not a unit vector.
    NonUnitRightAxis,
}

impl fmt::Display for SpatialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite => f.write_str("a spatial input was not finite"),
            Self::BadReferenceDistance { value } => {
                write!(f, "reference distance {value} must be positive and finite")
            }
            Self::BadMaxDistance { value } => {
                write!(
                    f,
                    "cutoff distance {value} must exceed the reference distance"
                )
            }
            Self::NonUnitRightAxis => f.write_str("the listener right axis is not a unit vector"),
        }
    }
}

impl std::error::Error for SpatialError {}

/// The designed distance-attenuation law of one emitter bus.
///
/// Designed, not measured: the original attenuation curve is unmeasured (F41
/// "Research boundary"), so this is an inverse-distance law a later evidence
/// stage may replace. Full gain inside `reference`, `reference / distance`
/// beyond it, silence at and beyond `cutoff`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpatialPolicy {
    reference: f64,
    cutoff: f64,
}

impl SpatialPolicy {
    /// Validates a policy.
    ///
    /// # Errors
    ///
    /// [`SpatialError::BadReferenceDistance`] or [`SpatialError::BadMaxDistance`].
    pub fn try_new(reference: f64, cutoff: f64) -> Result<Self, SpatialError> {
        if !reference.is_finite() || reference <= 0.0 {
            return Err(SpatialError::BadReferenceDistance { value: reference });
        }
        if !cutoff.is_finite() || cutoff <= reference {
            return Err(SpatialError::BadMaxDistance { value: cutoff });
        }
        Ok(Self { reference, cutoff })
    }
}

/// The listener's pose, in the same origin-rebased frame as the emitters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Listener {
    position: [f64; 3],
    right: [f64; 3],
}

impl Listener {
    /// Validates a listener: finite position and a unit right axis.
    ///
    /// # Errors
    ///
    /// [`SpatialError::NonFinite`] or [`SpatialError::NonUnitRightAxis`].
    pub fn try_new(position: [f64; 3], right: [f64; 3]) -> Result<Self, SpatialError> {
        if !position.iter().chain(right.iter()).all(|c| c.is_finite()) {
            return Err(SpatialError::NonFinite);
        }
        let length = right.iter().map(|c| c * c).sum::<f64>().sqrt();
        if (length - 1.0).abs() > 1e-6 {
            return Err(SpatialError::NonUnitRightAxis);
        }
        Ok(Self { position, right })
    }
}

/// The spatial result for one emitter: a distance gain and a stereo pan.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpatialMix {
    /// Linear distance gain in `0.0..=1.0`.
    pub gain: f64,
    /// Stereo placement in `-1.0` (full left) `..=1.0` (full right).
    pub pan: f64,
}

/// Places one emitter relative to the listener.
///
/// Pan is the emitter direction's component along the listener's right axis,
/// so an emitter on the right pans positive; an emitter at the listener's own
/// position is centred.
///
/// # Errors
///
/// [`SpatialError::NonFinite`] for a non-finite emitter position.
pub fn spatialize(
    policy: &SpatialPolicy,
    listener: &Listener,
    emitter: [f64; 3],
) -> Result<SpatialMix, SpatialError> {
    if !emitter.iter().all(|c| c.is_finite()) {
        return Err(SpatialError::NonFinite);
    }
    let rel = [
        emitter[0] - listener.position[0],
        emitter[1] - listener.position[1],
        emitter[2] - listener.position[2],
    ];
    let distance = rel.iter().map(|c| c * c).sum::<f64>().sqrt();
    let gain = if distance >= policy.cutoff {
        0.0
    } else if distance <= policy.reference {
        1.0
    } else {
        policy.reference / distance
    };
    let pan = if distance <= f64::EPSILON {
        0.0
    } else {
        let along: f64 = rel.iter().zip(listener.right).map(|(r, a)| r * a).sum();
        (along / distance).clamp(-1.0, 1.0)
    };
    Ok(SpatialMix { gain, pan })
}

// ------------------------------------------------------------ fixtures -----

/// The key of the synthetic weapon one-shot used by F41-A's minimum scenario.
pub const SYNTHETIC_WEAPON_ASSET_KEY: &str = "synthetic.weapon.machinegun";

/// Builds the synthetic weapon one-shot cue F41-A's minimum scenario replays.
///
/// Newly authored development content on the [`AudioBus::Weapons`] bus, at
/// unity gain — never an original asset.
#[must_use]
pub fn synthetic_weapon_one_shot(id: AudioEventId) -> OneShotEvent {
    OneShotEvent::try_new(
        id,
        ContentId::from_source(ContentKind::Sound, SYNTHETIC_WEAPON_ASSET_KEY)
            .expect("fixture id is valid"),
        AudioBus::Weapons,
        1.0,
    )
    .expect("the synthetic weapon one-shot is valid")
}

/// Builds the synthetic engine loop F41-A's emitter tests bind and swap.
///
/// Newly authored development content on the [`AudioBus::Engine`] bus.
#[must_use]
pub fn synthetic_engine_loop(emitter: AudioEmitterId, id: AudioEventId) -> LoopBinding {
    LoopBinding::try_new(
        emitter,
        id,
        ContentId::from_source(ContentKind::Sound, "synthetic.engine.loop")
            .expect("fixture id is valid"),
        AudioBus::Engine,
        1.0,
    )
    .expect("the synthetic engine loop is valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event_id(sequence: u32) -> AudioEventId {
        AudioEventId {
            session: 7,
            tick: Tick(3),
            producer: 1,
            sequence,
        }
    }

    /// The runtime bus vocabulary covers all seven labels and round-trips them.
    #[test]
    fn accept_f41_a_bus_labels_are_unique_and_round_trip() {
        let labels: Vec<&str> = AudioBus::ALL.iter().map(|bus| bus.label()).collect();
        let mut unique = labels.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), labels.len(), "bus labels are unique");
        assert_eq!(labels.len(), 7, "F41 names seven buses");
        for bus in AudioBus::ALL {
            assert_eq!(AudioBus::from_label(bus.label()), Some(*bus));
        }
        assert_eq!(AudioBus::from_label("nonsense"), None);
    }

    /// A corrupt gain or a non-audio asset is refused at the input boundary.
    #[test]
    fn accept_f41_a_inputs_refuse_corrupt_gain_and_foreign_kinds() {
        let asset = ContentId::from_source(ContentKind::Sound, "synthetic.weapon").expect("valid");
        assert!(matches!(
            OneShotEvent::try_new(event_id(0), asset.clone(), AudioBus::Weapons, f64::NAN),
            Err(AudioEventError::NonFiniteGain { value }) if value.is_nan()
        ));
        assert_eq!(
            OneShotEvent::try_new(event_id(0), asset.clone(), AudioBus::Weapons, -1.0),
            Err(AudioEventError::NegativeGain { value: -1.0 })
        );
        assert_eq!(
            OneShotEvent::try_new(
                event_id(0),
                ContentId::from_source(ContentKind::Mesh, "synthetic.mesh").expect("valid"),
                AudioBus::Weapons,
                1.0
            ),
            Err(AudioEventError::NotAnAudioAsset {
                kind: ContentKind::Mesh
            })
        );
    }

    /// A one-shot is accepted once and every replay of the same event is
    /// suppressed.
    #[test]
    fn accept_f41_a_router_accepts_one_shot_once() {
        let mut router = AudioRouter::new(7);
        let event = synthetic_weapon_one_shot(event_id(0));
        assert!(matches!(
            router.play_one_shot(&event),
            OneShotOutcome::Accepted { .. }
        ));
        assert_eq!(
            router.play_one_shot(&event),
            OneShotOutcome::SuppressedDuplicate { id: event.id }
        );
        // A genuinely new sequence is a new cue.
        assert!(matches!(
            router.play_one_shot(&synthetic_weapon_one_shot(event_id(1))),
            OneShotOutcome::Accepted { .. }
        ));
    }

    /// An event from another session is refused rather than deduplicated.
    #[test]
    fn accept_f41_a_router_refuses_foreign_sessions() {
        let mut router = AudioRouter::new(7);
        let mut foreign = synthetic_weapon_one_shot(event_id(0));
        foreign.id.session = 8;
        assert_eq!(
            router.play_one_shot(&foreign),
            OneShotOutcome::RefusedForeignSession {
                id: foreign.id,
                session: 7
            }
        );
    }

    /// Loops start once per emitter, swap cleanly and stop by reason.
    #[test]
    fn accept_f41_a_loops_swap_and_stop_by_reason() {
        let emitter = AudioEmitterId {
            session: 7,
            serial: 4,
        };
        let mut router = AudioRouter::new(7);
        assert_eq!(
            router.start_loop(&synthetic_engine_loop(emitter, event_id(0))),
            LoopOutcome::Started { emitter }
        );
        assert_eq!(router.active_loop_count(), 1);
        assert_eq!(
            router.start_loop(&synthetic_engine_loop(emitter, event_id(1))),
            LoopOutcome::Swapped {
                emitter,
                stopped: event_id(0)
            }
        );
        assert_eq!(router.active_loop_count(), 1);
        assert_eq!(
            router.stop_loop(&emitter, EmitterStopReason::Despawned),
            LoopOutcome::Stopped {
                emitter,
                reason: EmitterStopReason::Despawned
            }
        );
        assert_eq!(router.active_loop_count(), 0);
        assert_eq!(
            router.stop_loop(&emitter, EmitterStopReason::Despawned),
            LoopOutcome::NotActive {
                emitter,
                reason: EmitterStopReason::Despawned
            }
        );
    }

    /// Pausing suspends every loop; a device loss stops every loop.
    #[test]
    fn accept_f41_a_pause_and_device_loss_stop_loops() {
        let emitter = |serial| AudioEmitterId { session: 7, serial };
        let mut router = AudioRouter::new(7);
        router.start_loop(&synthetic_engine_loop(emitter(1), event_id(0)));
        router.start_loop(&synthetic_engine_loop(emitter(2), event_id(1)));
        assert!(router.apply_pause(PausePolicy::Continue).is_empty());
        assert_eq!(router.active_loop_count(), 2);
        let stopped = router.apply_pause(PausePolicy::Suspend);
        assert_eq!(stopped.len(), 2);
        assert!(stopped.iter().all(|outcome| matches!(
            outcome,
            LoopOutcome::Stopped {
                reason: EmitterStopReason::Paused,
                ..
            }
        )));
        assert_eq!(router.active_loop_count(), 0);

        router.start_loop(&synthetic_engine_loop(emitter(1), event_id(2)));
        let lost = router.device_lost();
        assert_eq!(
            lost,
            vec![LoopOutcome::Stopped {
                emitter: emitter(1),
                reason: EmitterStopReason::DeviceLost
            }]
        );
        assert_eq!(router.active_loop_count(), 0);
        assert!(
            router.device_lost().is_empty(),
            "losing the device twice is total"
        );
    }

    /// The asset spec preserves the declared form and refuses the wrong form.
    #[test]
    fn accept_f41_a_asset_spec_routes_only_the_declared_form() {
        let spec = AudioAssetSpec::try_new(
            ContentId::from_source(ContentKind::Sound, SYNTHETIC_WEAPON_ASSET_KEY).expect("valid"),
            AudioBus::Weapons,
            1.0,
            PlaybackMode::OneShot,
        )
        .expect("valid spec");
        assert!(spec.one_shot_event(event_id(0)).is_ok());
        assert_eq!(
            spec.loop_binding(
                AudioEmitterId {
                    session: 7,
                    serial: 1
                },
                event_id(0)
            ),
            Err(AudioEventError::WrongPlaybackMode {
                expected: PlaybackMode::Loop,
                found: PlaybackMode::OneShot
            })
        );
    }
}
