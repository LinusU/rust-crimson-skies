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
use std::sync::{Arc, Mutex};

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

    /// The active loops, in emitter order.
    pub fn active_loops(&self) -> impl Iterator<Item = &LoopBinding> {
        self.loops.values()
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

// -------------------------------------------------- engine voice smoothing --

/// Why a designed engine smoothing law was refused.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EngineAudioError {
    /// A field was NaN or infinite.
    NonFinite {
        /// The offending field.
        field: &'static str,
    },
    /// A rate was not strictly positive.
    NonPositiveRate {
        /// The offending field.
        field: &'static str,
        /// The rejected value.
        value: f64,
    },
    /// A gain was negative or above [`MAX_AUDIO_GAIN`].
    BadGain {
        /// The offending field.
        field: &'static str,
        /// The rejected value.
        value: f64,
    },
    /// A pitch ratio was not strictly positive.
    NonPositivePitch {
        /// The offending field.
        field: &'static str,
        /// The rejected value.
        value: f64,
    },
}

impl fmt::Display for EngineAudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite { field } => write!(f, "engine smoothing {field} must be finite"),
            Self::NonPositiveRate { field, value } => {
                write!(
                    f,
                    "engine smoothing {field} must be greater than zero, got {value}"
                )
            }
            Self::BadGain { field, value } => {
                write!(
                    f,
                    "engine smoothing {field} must be within 0..={MAX_AUDIO_GAIN}, got {value}"
                )
            }
            Self::NonPositivePitch { field, value } => {
                write!(
                    f,
                    "engine smoothing {field} must be greater than zero, got {value}"
                )
            }
        }
    }
}

impl std::error::Error for EngineAudioError {}

/// One continuous voice's smoothed level: a linear gain and a pitch ratio
/// applied to the sample rate a loop plays at.
///
/// `pitch` is a *ratio*, not a rate: `1.0` plays the recorded sample rate, a
/// smaller ratio slows the loop down. Both fields carry the same validated
/// domain as the rest of the module, so a mixer never forwards a NaN.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoiceLevel {
    /// Linear gain in `0.0..=MAX_AUDIO_GAIN`.
    pub gain: f64,
    /// Playback ratio in `(0.0, inf)`.
    pub pitch: f64,
}

impl VoiceLevel {
    /// Full volume at the recorded sample rate: what a voice that is not
    /// driven by any simulation state plays at.
    pub const UNITY: Self = Self {
        gain: 1.0,
        pitch: 1.0,
    };

    /// Assembles a level without validating it; [`Self::validate`] refuses a
    /// corrupt one.
    #[must_use]
    pub const fn new(gain: f64, pitch: f64) -> Self {
        Self { gain, pitch }
    }

    /// Applies the same gain rules as [`validate_gain`] and requires a
    /// strictly positive pitch ratio.
    ///
    /// # Errors
    ///
    /// [`EngineAudioError::BadGain`] or [`EngineAudioError::NonPositivePitch`].
    pub fn validate(&self) -> Result<(), EngineAudioError> {
        if !self.gain.is_finite() {
            return Err(EngineAudioError::NonFinite {
                field: "voice gain",
            });
        }
        if self.gain < 0.0 || self.gain > MAX_AUDIO_GAIN {
            return Err(EngineAudioError::BadGain {
                field: "voice gain",
                value: self.gain,
            });
        }
        if !self.pitch.is_finite() {
            return Err(EngineAudioError::NonFinite {
                field: "voice pitch",
            });
        }
        if self.pitch <= 0.0 {
            return Err(EngineAudioError::NonPositivePitch {
                field: "voice pitch",
                value: self.pitch,
            });
        }
        Ok(())
    }
}

/// The designed smoothing law of one continuous engine voice.
///
/// F41 non-negotiable behavior 1 requires engine pitch and volume to depend on
/// *measured* throttle/engine state with stable smoothing "not render FPS".
/// The state it depends on is the caller-supplied [`EngineState`] spool
/// (below); the law itself is **designed, not measured**: the original engine
/// audio's attack, release and pitch range are unmeasured (see
/// `docs/findings/2026-10-01-f41-b-loops-and-spatial-emitters.md` and the F41
/// research boundary), so every value here is a project choice that a later
/// evidence stage may replace, and nothing in this file claims the original's
/// numbers.
///
/// The step law is a bounded linear ramp: one tick moves the gain toward its
/// target by `rate * dt`, never past it, and never leaves the gain untouched on
/// a zero or corrupt `dt`. Because the step is clamped at the target, the same
/// elapsed time lands on the same value however it was divided into steps —
/// that is what makes the smoothing stable across frame rates, and it is
/// checked directly by `accept_f41_b_smoothing_is_step_invariant`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EngineSmoothing {
    gain_attack_per_s: f64,
    gain_release_per_s: f64,
    pitch_rate_per_s: f64,
    idle_gain: f64,
    full_gain: f64,
    idle_pitch: f64,
    full_pitch: f64,
}

impl EngineSmoothing {
    /// The designed law a voice starts from unless the content declares
    /// another: the gain opens at 2.0/s and closes at 1.5/s, the pitch ratio
    /// sweeps at 3.0/s, and the spool spans gain `0.25 .. 1.0` and pitch ratio
    /// `0.7 .. 1.6`.
    ///
    /// These are project defaults for the *shape* of the response. They are not
    /// measurements of the original engine's audio and must not be read as
    /// such.
    pub const DESIGNED_DEFAULT: Self = Self {
        gain_attack_per_s: 2.0,
        gain_release_per_s: 1.5,
        pitch_rate_per_s: 3.0,
        idle_gain: 0.25,
        full_gain: 1.0,
        idle_pitch: 0.7,
        full_pitch: 1.6,
    };

    /// Validates a law: finite, strictly positive rates, gains inside the
    /// module's ceiling and strictly positive pitch ratios.
    ///
    /// # Errors
    ///
    /// [`EngineAudioError::NonFinite`], [`EngineAudioError::NonPositiveRate`],
    /// [`EngineAudioError::BadGain`] or [`EngineAudioError::NonPositivePitch`],
    /// naming the first offending field.
    pub fn try_new(
        gain_attack_per_s: f64,
        gain_release_per_s: f64,
        pitch_rate_per_s: f64,
        idle_gain: f64,
        full_gain: f64,
        idle_pitch: f64,
        full_pitch: f64,
    ) -> Result<Self, EngineAudioError> {
        let law = Self {
            gain_attack_per_s,
            gain_release_per_s,
            pitch_rate_per_s,
            idle_gain,
            full_gain,
            idle_pitch,
            full_pitch,
        };
        law.validate()?;
        Ok(law)
    }

    /// Validates the law, applying nothing.
    ///
    /// # Errors
    ///
    /// As [`Self::try_new`].
    pub fn validate(&self) -> Result<(), EngineAudioError> {
        for (field, value) in [
            ("gain_attack_per_s", self.gain_attack_per_s),
            ("gain_release_per_s", self.gain_release_per_s),
            ("pitch_rate_per_s", self.pitch_rate_per_s),
        ] {
            if !value.is_finite() {
                return Err(EngineAudioError::NonFinite { field });
            }
            if value <= 0.0 {
                return Err(EngineAudioError::NonPositiveRate { field, value });
            }
        }
        for (field, value) in [("idle_gain", self.idle_gain), ("full_gain", self.full_gain)] {
            if !value.is_finite() {
                return Err(EngineAudioError::NonFinite { field });
            }
            if value < 0.0 || value > MAX_AUDIO_GAIN {
                return Err(EngineAudioError::BadGain { field, value });
            }
        }
        for (field, value) in [
            ("idle_pitch", self.idle_pitch),
            ("full_pitch", self.full_pitch),
        ] {
            if !value.is_finite() {
                return Err(EngineAudioError::NonFinite { field });
            }
            if value <= 0.0 {
                return Err(EngineAudioError::NonPositivePitch { field, value });
            }
        }
        Ok(())
    }

    /// How fast the gain opens, in gains per second.
    #[must_use]
    pub const fn gain_attack_per_s(self) -> f64 {
        self.gain_attack_per_s
    }

    /// How fast the gain closes, in gains per second.
    #[must_use]
    pub const fn gain_release_per_s(self) -> f64 {
        self.gain_release_per_s
    }

    /// How fast the pitch ratio sweeps, in ratios per second.
    #[must_use]
    pub const fn pitch_rate_per_s(self) -> f64 {
        self.pitch_rate_per_s
    }

    /// The gain a running engine holds at zero throttle.
    #[must_use]
    pub const fn idle_gain(self) -> f64 {
        self.idle_gain
    }

    /// The gain a full throttle reaches.
    #[must_use]
    pub const fn full_gain(self) -> f64 {
        self.full_gain
    }

    /// The pitch ratio a running engine holds at zero throttle.
    #[must_use]
    pub const fn idle_pitch(self) -> f64 {
        self.idle_pitch
    }

    /// The pitch ratio a full throttle reaches.
    #[must_use]
    pub const fn full_pitch(self) -> f64 {
        self.full_pitch
    }

    /// The level a throttle `spool` in `[0, 1]` asks for: the gain and pitch
    /// span interpolated along the spool, and silence for an engine that is
    /// not running.
    ///
    /// A spool outside `[0, 1]` or a non-finite one is clamped into the span
    /// rather than propagated: this reads simulation state, and the caller
    /// that produced it is already responsible for its own invariants
    /// (`cs_sim::flight::EngineState` clamps at construction).
    #[must_use]
    pub fn target(self, running: bool, spool: f64) -> VoiceLevel {
        if !running {
            return VoiceLevel::new(0.0, self.idle_pitch);
        }
        let spool = if spool.is_finite() {
            spool.clamp(0.0, 1.0)
        } else {
            0.0
        };
        VoiceLevel::new(
            self.idle_gain + (self.full_gain - self.idle_gain) * spool,
            self.idle_pitch + (self.full_pitch - self.idle_pitch) * spool,
        )
    }
}

/// One engine voice's smoothed state: where the gain and pitch ratio are right
/// now, and the single place they move.
///
/// This is state, not a force law, exactly like
/// [`cs_sim::flight::EngineState`](crate::flight::EngineState): the caller reads
/// [`Self::level`] and never writes the fields, so no smoothing can be skipped
/// by a path that forgets to advance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EngineVoice {
    gain: f64,
    pitch: f64,
}

impl EngineVoice {
    /// A voice that starts where a running engine at zero throttle belongs,
    /// so the first tick of a mission is a valid mix rather than a jump from
    /// silence.
    #[must_use]
    pub fn at_idle(smoothing: &EngineSmoothing) -> Self {
        let level = smoothing.target(true, 0.0);
        Self {
            gain: level.gain,
            pitch: level.pitch,
        }
    }

    /// The smoothed level.
    #[must_use]
    pub const fn level(&self) -> VoiceLevel {
        VoiceLevel {
            gain: self.gain,
            pitch: self.pitch,
        }
    }

    /// The smoothed linear gain.
    #[must_use]
    pub const fn gain(&self) -> f64 {
        self.gain
    }

    /// The smoothed playback ratio.
    #[must_use]
    pub const fn pitch(&self) -> f64 {
        self.pitch
    }

    /// Moves one step of `dt_s` toward `target` under `smoothing`.
    ///
    /// The gain opens at the law's attack rate and closes at its release rate;
    /// the pitch ratio sweeps at one rate in both directions. Each value moves
    /// at most `rate * dt_s`, so it can neither overshoot the target nor leave
    /// its valid domain, and the same elapsed time reaches the same value
    /// however many steps it was split into. A non-finite or non-positive
    /// `dt_s` changes nothing — a corrupt clock cannot move a voice.
    pub fn advance(&mut self, smoothing: &EngineSmoothing, target: VoiceLevel, dt_s: f64) {
        if !dt_s.is_finite() || dt_s <= 0.0 {
            return;
        }
        let gain_rate = if target.gain >= self.gain {
            smoothing.gain_attack_per_s
        } else {
            smoothing.gain_release_per_s
        };
        self.gain += (target.gain - self.gain).clamp(-gain_rate * dt_s, gain_rate * dt_s);
        self.pitch += (target.pitch - self.pitch).clamp(
            -smoothing.pitch_rate_per_s * dt_s,
            smoothing.pitch_rate_per_s * dt_s,
        );
    }
}

// ------------------------------------------------------------------ device --

/// A voice id one device assigned to one started loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceVoiceId(pub u64);

/// What a mixer asks a device to start.
#[derive(Clone, Debug, PartialEq)]
pub struct VoiceStart {
    /// The audio asset the voice loops.
    pub asset: ContentId,
    /// The bus the voice mixes on.
    pub bus: AudioBus,
    /// The loop's declared linear gain, before engine and spatial terms.
    pub gain: f64,
    /// Initial stereo placement in `-1.0 ..= 1.0`.
    pub pan: f64,
    /// Initial playback ratio.
    pub pitch: f64,
}

/// What a mixer asks a device to change on a voice already sounding.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoiceUpdate {
    /// The final linear gain: loop gain × engine gain × spatial gain.
    pub gain: f64,
    /// The stereo placement.
    pub pan: f64,
    /// The playback ratio.
    pub pitch: f64,
}

/// Why a device refused a command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceError {
    /// A stable machine-readable code, for reports and evidence.
    pub code: &'static str,
    /// A human-readable detail, naming what the device was doing.
    pub detail: String,
}

impl DeviceError {
    /// Assembles a device error.
    #[must_use]
    pub fn new(code: &'static str, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for DeviceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "audio device {}: {}", self.code, self.detail)
    }
}

impl std::error::Error for DeviceError {}

/// Why a voice stopped, as the device records it: the router's own
/// [`EmitterStopReason`], or `None` when the mixer stopped a voice whose loop
/// no longer exists and has therefore no reason to report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoiceStop {
    /// The voice the device owned.
    pub voice: DeviceVoiceId,
    /// The reason, when the mixer has one.
    pub reason: Option<EmitterStopReason>,
}

/// The output device the mixer drives.
///
/// This is the whole of the audio boundary a device has to implement: five
/// commands and no game state. Nothing above it — the router, the session, the
/// radio queue's timing, mission progression — may consult the device, which is
/// what keeps F41 non-negotiable behavior 2 true: losing an output must not be
/// able to stall a mission.
///
/// **No implementation in this workspace opens hardware yet.** Reaching an
/// original audible review needs the `audio` capability and is F41-D
/// (`specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stage
/// `### F41-D`); what exists here is the boundary and
/// [`RecordingAudioDevice`], which records what a real device would have been
/// asked to do.
pub trait AudioDevice: fmt::Debug + Send + Sync {
    /// Whether the device currently accepts commands.
    fn is_open(&self) -> bool;

    /// Opens the device.
    ///
    /// # Errors
    ///
    /// [`DeviceError`] when the device cannot be opened.
    fn open(&mut self) -> Result<(), DeviceError>;

    /// Closes the device, silencing every voice it holds.
    fn close(&mut self);

    /// Starts one looping voice and returns the id the device gave it.
    ///
    /// # Errors
    ///
    /// [`DeviceError`] when the device refuses the voice.
    fn start_voice(&mut self, start: VoiceStart) -> Result<DeviceVoiceId, DeviceError>;

    /// Stops one voice.
    ///
    /// # Errors
    ///
    /// [`DeviceError`] when the device refuses the stop.
    fn stop_voice(&mut self, stop: VoiceStop) -> Result<(), DeviceError>;

    /// Changes the gain, pan or pitch of a voice already sounding.
    ///
    /// # Errors
    ///
    /// [`DeviceError`] when the device refuses the change.
    fn update_voice(
        &mut self,
        voice: DeviceVoiceId,
        update: VoiceUpdate,
    ) -> Result<(), DeviceError>;
}

/// One command a [`RecordingAudioDevice`] received.
#[derive(Clone, Debug, PartialEq)]
pub enum DeviceCommand {
    /// The device was opened.
    Opened,
    /// The device was closed.
    Closed,
    /// A voice started.
    Started {
        /// The id the device assigned.
        voice: DeviceVoiceId,
        /// What it was asked to play.
        start: VoiceStart,
    },
    /// A voice's mix changed.
    Updated {
        /// The voice.
        voice: DeviceVoiceId,
        /// The change.
        update: VoiceUpdate,
    },
    /// A voice stopped.
    Stopped {
        /// The voice.
        voice: DeviceVoiceId,
        /// Why, when the mixer had a reason.
        reason: Option<EmitterStopReason>,
    },
}

impl fmt::Display for DeviceCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Opened => f.write_str("device opened"),
            Self::Closed => f.write_str("device closed"),
            Self::Started { voice, start } => write!(
                f,
                "voice {} started {} on {} at gain {} pan {} pitch {}",
                voice.0, start.asset, start.bus, start.gain, start.pan, start.pitch
            ),
            Self::Updated { voice, update } => write!(
                f,
                "voice {} set to gain {} pan {} pitch {}",
                voice.0, update.gain, update.pan, update.pitch
            ),
            Self::Stopped { voice, reason } => match reason {
                Some(reason) => write!(f, "voice {} stopped: {reason}", voice.0),
                None => write!(f, "voice {} stopped: no loop remains", voice.0),
            },
        }
    }
}

/// A device that records every command and makes no sound.
///
/// It is the production stand-in while no hardware backend exists — the mixer
/// is a real consumer either way — and the probe an evidence stage reads to
/// learn what the simulation *asked* for. It is explicitly **not** proof that
/// a user heard anything (F41 non-negotiable behavior 5): it holds no samples
/// and opens no hardware.
///
/// The log is shared: [`Self::handle`] is a second handle onto the *same*
/// device, which is how a caller hands the device to a world and still reads
/// back what the world asked of it.
#[derive(Clone, Debug, Default)]
pub struct RecordingAudioDevice {
    log: Arc<Mutex<RecordingLog>>,
}

#[derive(Debug, Default)]
struct RecordingLog {
    open: bool,
    voices: BTreeMap<DeviceVoiceId, VoiceStart>,
    next_voice: u64,
    commands: Vec<DeviceCommand>,
}

impl RecordingAudioDevice {
    /// A closed device with an empty log.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A second handle onto this same device.
    #[must_use]
    pub fn handle(&self) -> Self {
        Self {
            log: Arc::clone(&self.log),
        }
    }

    /// Every command since the last [`Self::clear`], in arrival order.
    #[must_use]
    pub fn commands(&self) -> Vec<DeviceCommand> {
        self.read().commands.clone()
    }

    /// Empties the command log; the sounding voices are untouched.
    pub fn clear(&self) {
        self.write().commands.clear();
    }

    /// The voices this device is sounding, in id order.
    #[must_use]
    pub fn voices(&self) -> Vec<(DeviceVoiceId, VoiceStart)> {
        self.read()
            .voices
            .iter()
            .map(|(voice, start)| (*voice, start.clone()))
            .collect()
    }

    /// Whether `voice` is still sounding.
    #[must_use]
    pub fn is_sounding(&self, voice: DeviceVoiceId) -> bool {
        self.read().voices.contains_key(&voice)
    }

    /// How many voices are sounding.
    #[must_use]
    pub fn sounding(&self) -> usize {
        self.read().voices.len()
    }

    /// The last command the device received, if any.
    #[must_use]
    pub fn last_command(&self) -> Option<DeviceCommand> {
        self.read().commands.last().cloned()
    }

    fn read(&self) -> std::sync::MutexGuard<'_, RecordingLog> {
        self.log
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn write(&self) -> std::sync::MutexGuard<'_, RecordingLog> {
        self.read()
    }
}

impl AudioDevice for RecordingAudioDevice {
    fn is_open(&self) -> bool {
        self.read().open
    }

    fn open(&mut self) -> Result<(), DeviceError> {
        let mut log = self.write();
        log.open = true;
        log.commands.push(DeviceCommand::Opened);
        Ok(())
    }

    fn close(&mut self) {
        let mut log = self.write();
        log.open = false;
        log.voices.clear();
        log.commands.push(DeviceCommand::Closed);
    }

    fn start_voice(&mut self, start: VoiceStart) -> Result<DeviceVoiceId, DeviceError> {
        let mut log = self.write();
        if !log.open {
            return Err(DeviceError::new("closed", "the device is not open"));
        }
        let voice = DeviceVoiceId(log.next_voice);
        log.next_voice += 1;
        log.voices.insert(voice, start.clone());
        log.commands.push(DeviceCommand::Started { voice, start });
        Ok(voice)
    }

    fn stop_voice(&mut self, stop: VoiceStop) -> Result<(), DeviceError> {
        let mut log = self.write();
        if !log.open {
            return Err(DeviceError::new("closed", "the device is not open"));
        }
        log.voices.remove(&stop.voice);
        log.commands.push(DeviceCommand::Stopped {
            voice: stop.voice,
            reason: stop.reason,
        });
        Ok(())
    }

    fn update_voice(
        &mut self,
        voice: DeviceVoiceId,
        update: VoiceUpdate,
    ) -> Result<(), DeviceError> {
        let mut log = self.write();
        if !log.open {
            return Err(DeviceError::new("closed", "the device is not open"));
        }
        log.commands.push(DeviceCommand::Updated { voice, update });
        Ok(())
    }
}

// ------------------------------------------------------------------- mixer --

/// One emitter's continuous state for one mix pass: where it is and what level
/// its simulation state asks for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EmitterMix {
    /// The emitter being mixed.
    pub emitter: AudioEmitterId,
    /// Its position, in the listener's own frame, in meters.
    pub position_m: [f64; 3],
    /// The level its simulation state asks for — [`VoiceLevel::UNITY`] for an
    /// emitter nothing drives.
    pub level: VoiceLevel,
}

/// Why the mixer could not carry out what an outcome asked.
#[derive(Clone, Debug, PartialEq)]
pub enum MixerRefusal {
    /// The outcome named an emitter of another session generation.
    ForeignSession {
        /// The refused emitter.
        emitter: AudioEmitterId,
        /// The session the mixer serves.
        session: u64,
    },
    /// A start outcome had no live loop binding to play.
    NoBinding {
        /// The emitter.
        emitter: AudioEmitterId,
    },
    /// A stop outcome named an emitter the mixer never started.
    NoActiveVoice {
        /// The emitter.
        emitter: AudioEmitterId,
        /// Why the stop was requested.
        reason: EmitterStopReason,
    },
    /// The emitter's position could not be placed.
    NotPlaced {
        /// The emitter.
        emitter: AudioEmitterId,
        /// Why.
        error: SpatialError,
    },
    /// A voice was still sounding although no loop remains for its emitter, so
    /// the mixer stopped it without a reason to report.
    OrphanedVoice {
        /// The emitter.
        emitter: AudioEmitterId,
    },
    /// The device refused a command, so the voice is not sounding as asked.
    Device {
        /// The emitter.
        emitter: AudioEmitterId,
        /// The device's code.
        code: &'static str,
    },
}

impl fmt::Display for MixerRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignSession { emitter, session } => write!(
                f,
                "{emitter} belongs to another audio session than the mixer's {session}"
            ),
            Self::NoBinding { emitter } => {
                write!(f, "{emitter} was asked to start a loop it no longer holds")
            }
            Self::NoActiveVoice { emitter, reason } => {
                write!(
                    f,
                    "{emitter} was asked to stop ({reason}) but was not sounding"
                )
            }
            Self::NotPlaced { emitter, error } => {
                write!(f, "{emitter} could not be placed: {error}")
            }
            Self::OrphanedVoice { emitter } => write!(
                f,
                "{emitter} was still sounding although no loop remains, so it was stopped"
            ),
            Self::Device { emitter, code } => {
                write!(f, "the device refused a command for {emitter}: {code}")
            }
        }
    }
}

/// What one mix pass did, in emitter order per phase.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MixerReport {
    /// Voices that started from nothing.
    pub started: Vec<AudioEmitterId>,
    /// Voices whose loop was replaced.
    pub swapped: Vec<AudioEmitterId>,
    /// Voices that stopped.
    pub stopped: Vec<AudioEmitterId>,
    /// Voices whose mix was updated.
    pub updated: Vec<AudioEmitterId>,
    /// Everything the mixer could not do, in arrival order.
    pub refusals: Vec<MixerRefusal>,
}

/// One voice the mixer owns on the device.
#[derive(Clone, Debug, PartialEq)]
struct MixerVoice {
    device: DeviceVoiceId,
    gain: f64,
}

/// The device-independent mixer: the consumer of the session's loop outcomes
/// and of the spatial law.
///
/// It holds no game state and consults no device for a decision — the device is
/// only *told* what to play — so a mission can be simulated end to end with no
/// output at all, and a lost device cannot change what the simulation decided
/// (F41 non-negotiable behaviors 1 and 2).
///
/// One pass is two phases, in this order:
///
/// 1. **Lifecycle.** Every [`LoopOutcome`] the session recorded is carried out:
///    a start opens a device voice from the loop the router still holds, a swap
///    closes the old voice and opens the new one, a stop closes the voice. A
///    voice whose loop has gone — because the session was replaced under the
///    mixer — is closed as [`MixerRefusal::OrphanedVoice`], so a reload cannot
///    leave the previous load audible.
/// 2. **Placement.** Every voice still sounding is given the mix its
///    [`EmitterMix`] and the listener imply: `loop gain × engine gain ×
///    spatial gain`, the spatial pan, and the engine's pitch ratio.
#[derive(Clone, Debug)]
pub struct AudioMixer {
    session: u64,
    voices: BTreeMap<AudioEmitterId, MixerVoice>,
}

impl AudioMixer {
    /// A mixer for `session`, with nothing sounding.
    #[must_use]
    pub fn new(session: u64) -> Self {
        Self {
            session,
            voices: BTreeMap::new(),
        }
    }

    /// The session generation this mixer serves.
    #[must_use]
    pub const fn session(&self) -> u64 {
        self.session
    }

    /// The voices the mixer owns, in emitter order.
    pub fn emitters(&self) -> impl Iterator<Item = &AudioEmitterId> {
        self.voices.keys()
    }

    /// How many voices the mixer owns.
    #[must_use]
    pub fn voice_count(&self) -> usize {
        self.voices.len()
    }

    /// The device voice one emitter owns, if any.
    #[must_use]
    pub fn device_voice(&self, emitter: &AudioEmitterId) -> Option<DeviceVoiceId> {
        self.voices.get(emitter).map(|voice| voice.device)
    }

    /// One mix pass: lifecycle first, then placement.
    ///
    /// A closed device is **opened** first — the output becomes available when
    /// there is something to play, not at plugin-build time. A device that
    /// refuses to open makes every outcome a [`MixerRefusal::Device`] rather
    /// than a silent success, and the caller reports it.
    ///
    /// An emitter with no [`EmitterMix`] this pass keeps the mix it last
    /// received: an emitter whose pose the caller does not know is not a reason
    /// to move a sounding voice.
    pub fn mix(
        &mut self,
        router: &AudioRouter,
        outcomes: &[LoopOutcome],
        states: &[EmitterMix],
        listener: &Listener,
        policy: &SpatialPolicy,
        device: &mut dyn AudioDevice,
    ) -> MixerReport {
        let mut report = MixerReport::default();
        if !device.is_open()
            && let Err(error) = device.open()
        {
            report.refusals = outcomes
                .iter()
                .map(outcome_emitter)
                .map(|emitter| MixerRefusal::Device {
                    emitter,
                    code: error.code,
                })
                .collect();
            return report;
        }
        for outcome in outcomes {
            match outcome {
                LoopOutcome::Started { emitter } => {
                    if let Some(refusal) = self.foreign(*emitter, &mut report) {
                        report.refusals.push(refusal);
                        continue;
                    }
                    if self.start(*emitter, router, device, &mut report) {
                        report.started.push(*emitter);
                    }
                }
                LoopOutcome::Swapped { emitter, .. } => {
                    if let Some(refusal) = self.foreign(*emitter, &mut report) {
                        report.refusals.push(refusal);
                        continue;
                    }
                    self.close(
                        *emitter,
                        Some(EmitterStopReason::EmitterSwapped),
                        device,
                        &mut report,
                    );
                    if self.start(*emitter, router, device, &mut report) {
                        report.swapped.push(*emitter);
                    }
                }
                LoopOutcome::Stopped { emitter, reason } => {
                    if let Some(refusal) = self.foreign(*emitter, &mut report) {
                        report.refusals.push(refusal);
                        continue;
                    }
                    if self.voices.contains_key(emitter) {
                        self.close(*emitter, Some(*reason), device, &mut report);
                        report.stopped.push(*emitter);
                    } else {
                        report.refusals.push(MixerRefusal::NoActiveVoice {
                            emitter: *emitter,
                            reason: *reason,
                        });
                    }
                }
                LoopOutcome::NotActive { emitter, reason } => {
                    if let Some(refusal) = self.foreign(*emitter, &mut report) {
                        report.refusals.push(refusal);
                        continue;
                    }
                    report.refusals.push(MixerRefusal::NoActiveVoice {
                        emitter: *emitter,
                        reason: *reason,
                    });
                }
                LoopOutcome::RefusedForeignSession { emitter, session } => {
                    report.refusals.push(MixerRefusal::ForeignSession {
                        emitter: *emitter,
                        session: *session,
                    });
                }
            }
        }
        // A voice whose loop has gone — a replaced session, a stop that reached
        // the router without passing through this mixer — must not stay audible.
        let orphans: Vec<AudioEmitterId> = self
            .voices
            .keys()
            .copied()
            .filter(|emitter| router.active_loop(emitter).is_none())
            .collect();
        for emitter in orphans {
            self.close(emitter, None, device, &mut report);
            report.stopped.push(emitter);
            report
                .refusals
                .push(MixerRefusal::OrphanedVoice { emitter });
        }

        for state in states {
            let Some(voice) = self.voices.get(&state.emitter) else {
                continue;
            };
            let spatial = match spatialize(policy, listener, state.position_m) {
                Ok(spatial) => spatial,
                Err(error) => {
                    report.refusals.push(MixerRefusal::NotPlaced {
                        emitter: state.emitter,
                        error,
                    });
                    continue;
                }
            };
            let update = VoiceUpdate {
                gain: voice.gain * state.level.gain * spatial.gain,
                pan: spatial.pan,
                pitch: state.level.pitch,
            };
            match device.update_voice(voice.device, update) {
                Ok(()) => report.updated.push(state.emitter),
                Err(error) => report.refusals.push(MixerRefusal::Device {
                    emitter: state.emitter,
                    code: error.code,
                }),
            }
        }
        report
    }

    /// The session this mixer served is gone: every voice stops with no reason to
    /// report, and the mixer keeps none.
    ///
    /// This is the reload path: the session and its mixer are replaced together
    /// when a new load is delivered, and the previous load's voices must not
    /// survive into it. Nothing is remembered for retry — the *new* session's
    /// own loops are what sound after a reload, so keeping a copy of the old
    /// ones here would be a second source of truth about what is playing.
    pub fn release(&mut self, device: &mut dyn AudioDevice) -> MixerReport {
        let mut report = MixerReport::default();
        let live: Vec<AudioEmitterId> = self.voices.keys().copied().collect();
        for emitter in live {
            self.close(emitter, None, device, &mut report);
            report.stopped.push(emitter);
            report
                .refusals
                .push(MixerRefusal::OrphanedVoice { emitter });
        }
        report
    }

    /// The output device is gone: every voice stops as
    /// [`EmitterStopReason::DeviceLost`] and the device is closed.
    ///
    /// The mixer keeps no voice to restore — the session holds the loops it
    /// stopped and re-issues them when the device returns, so what sounds
    /// afterwards comes from the session's own state rather than from a second
    /// copy of it here.
    pub fn device_lost(&mut self, device: &mut dyn AudioDevice) -> MixerReport {
        let mut report = MixerReport::default();
        let lost: Vec<AudioEmitterId> = self.voices.keys().copied().collect();
        for emitter in lost {
            self.close(
                emitter,
                Some(EmitterStopReason::DeviceLost),
                device,
                &mut report,
            );
            report.stopped.push(emitter);
        }
        device.close();
        report
    }

    /// Re-opens a closed device. Every loop the session re-binds afterwards
    /// reaches the device through [`Self::mix`].
    ///
    /// # Errors
    ///
    /// [`DeviceError`] when the device cannot be re-opened.
    pub fn device_restored(&mut self, device: &mut dyn AudioDevice) -> Result<(), DeviceError> {
        device.open()
    }

    fn foreign(&self, emitter: AudioEmitterId, _report: &mut MixerReport) -> Option<MixerRefusal> {
        (emitter.session != self.session).then_some(MixerRefusal::ForeignSession {
            emitter,
            session: self.session,
        })
    }

    fn start(
        &mut self,
        emitter: AudioEmitterId,
        router: &AudioRouter,
        device: &mut dyn AudioDevice,
        report: &mut MixerReport,
    ) -> bool {
        let Some(binding) = router.active_loop(&emitter) else {
            report.refusals.push(MixerRefusal::NoBinding { emitter });
            return false;
        };
        let start = VoiceStart {
            asset: binding.asset.clone(),
            bus: binding.bus,
            gain: binding.gain,
            pan: 0.0,
            pitch: 1.0,
        };
        let gain = binding.gain;
        match device.start_voice(start) {
            Ok(voice) => {
                self.voices.insert(
                    emitter,
                    MixerVoice {
                        device: voice,
                        gain,
                    },
                );
                true
            }
            Err(error) => {
                report.refusals.push(MixerRefusal::Device {
                    emitter,
                    code: error.code,
                });
                false
            }
        }
    }

    fn close(
        &mut self,
        emitter: AudioEmitterId,
        reason: Option<EmitterStopReason>,
        device: &mut dyn AudioDevice,
        report: &mut MixerReport,
    ) {
        let Some(voice) = self.voices.remove(&emitter) else {
            return;
        };
        if let Err(error) = device.stop_voice(VoiceStop {
            voice: voice.device,
            reason,
        }) {
            report.refusals.push(MixerRefusal::Device {
                emitter,
                code: error.code,
            });
        }
    }
}

/// The emitter every loop outcome names.
fn outcome_emitter(outcome: &LoopOutcome) -> AudioEmitterId {
    match outcome {
        LoopOutcome::Started { emitter }
        | LoopOutcome::Swapped { emitter, .. }
        | LoopOutcome::Stopped { emitter, .. }
        | LoopOutcome::NotActive { emitter, .. }
        | LoopOutcome::RefusedForeignSession { emitter, .. } => *emitter,
    }
}

// ---------------------------------------------------------- radio queue ----

/// Maximum lines waiting behind the active radio line; a designed bound.
pub const MAX_PENDING_RADIO_LINES: usize = 32;

/// One radio dialogue line: speaker, priority, interruptibility, subtitle and
/// the simulation-tick length that defines when it completes.
///
/// Completion is measured in simulation ticks, never by a device callback, so
/// mission progression cannot depend on an audio device (F41 non-negotiable
/// behavior 2). The caller derives `duration_ticks` from the decoded clip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RadioLine {
    /// The line's identity and dedup key.
    pub id: AudioEventId,
    /// Who speaks.
    pub speaker: String,
    /// The dialogue asset.
    pub asset: ContentId,
    /// Higher preempts lower.
    pub priority: u8,
    /// Whether a strictly higher priority line may cut this one off.
    pub interruptible: bool,
    /// The subtitle text shown while the line is active, if any.
    pub subtitle: Option<String>,
    /// How many simulation ticks the line lasts; nonzero.
    pub duration_ticks: u64,
}

/// Why a radio line was refused at construction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RadioLineError {
    /// The asset is not a dialogue id.
    NotDialogue {
        /// The kind it names.
        kind: ContentKind,
    },
    /// The speaker name is empty.
    EmptySpeaker,
    /// The duration is zero.
    ZeroDuration,
}

impl fmt::Display for RadioLineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotDialogue { kind } => {
                write!(f, "radio line asset names a {kind}, not dialogue")
            }
            Self::EmptySpeaker => f.write_str("radio line has no speaker"),
            Self::ZeroDuration => f.write_str("radio line has zero duration"),
        }
    }
}

impl std::error::Error for RadioLineError {}

impl RadioLine {
    /// Validates and builds a radio line.
    ///
    /// # Errors
    ///
    /// [`RadioLineError`] for a non-dialogue asset, an empty speaker or a zero
    /// duration.
    pub fn try_new(
        id: AudioEventId,
        speaker: &str,
        asset: ContentId,
        priority: u8,
        interruptible: bool,
        subtitle: Option<&str>,
        duration_ticks: u64,
    ) -> Result<Self, RadioLineError> {
        if asset.kind() != ContentKind::Dialogue {
            return Err(RadioLineError::NotDialogue { kind: asset.kind() });
        }
        if speaker.is_empty() {
            return Err(RadioLineError::EmptySpeaker);
        }
        if duration_ticks == 0 {
            return Err(RadioLineError::ZeroDuration);
        }
        Ok(Self {
            id,
            speaker: speaker.to_owned(),
            asset,
            priority,
            interruptible,
            subtitle: subtitle.map(str::to_owned),
            duration_ticks,
        })
    }
}

/// What the radio queue reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RadioEvent {
    /// A line became active; `audible` is whether a device was available.
    Started {
        /// The line.
        id: AudioEventId,
        /// The speaker.
        speaker: String,
        /// The dialogue asset.
        asset: ContentId,
        /// The subtitle to show.
        subtitle: Option<String>,
        /// Whether the line is being voiced.
        audible: bool,
    },
    /// A line reached its full duration. `voiced` is false when the device was
    /// absent or lost at any point; completion is reported either way.
    Completed {
        /// The line.
        id: AudioEventId,
        /// Whether it was voiced throughout.
        voiced: bool,
    },
    /// A higher priority line cut the active one off.
    Interrupted {
        /// The cut-off line.
        id: AudioEventId,
        /// The line that replaced it.
        by: AudioEventId,
    },
    /// The device was lost under the active line; it keeps its timing.
    PlaybackLost {
        /// The line.
        id: AudioEventId,
    },
    /// The line was already accepted under this `(producer, sequence)`.
    SuppressedDuplicate {
        /// The line.
        id: AudioEventId,
    },
    /// The line belongs to another session generation.
    RefusedForeignSession {
        /// The line.
        id: AudioEventId,
    },
    /// The pending queue is at [`MAX_PENDING_RADIO_LINES`].
    RefusedQueueFull {
        /// The line.
        id: AudioEventId,
    },
}

#[derive(Clone, Debug)]
struct ActiveLine {
    line: RadioLine,
    started: Tick,
    voiced: bool,
}

/// The per-session radio queue: priority ordering, interruption, subtitles and
/// tick-based completion that is independent of any audio device.
#[derive(Clone, Debug)]
pub struct RadioQueue {
    session: u64,
    highest_sequence: BTreeMap<u32, u32>,
    pending: Vec<RadioLine>,
    active: Option<ActiveLine>,
    device_available: bool,
}

impl RadioQueue {
    /// An empty queue for `session` with a working device.
    #[must_use]
    pub fn new(session: u64) -> Self {
        Self {
            session,
            highest_sequence: BTreeMap::new(),
            pending: Vec::new(),
            active: None,
            device_available: true,
        }
    }

    /// The active line, if any.
    #[must_use]
    pub fn active(&self) -> Option<&RadioLine> {
        self.active.as_ref().map(|a| &a.line)
    }

    /// The subtitle of the active line; shown whether or not it is voiced.
    #[must_use]
    pub fn subtitle(&self) -> Option<&str> {
        self.active()?.subtitle.as_deref()
    }

    /// How many lines wait behind the active one.
    #[must_use]
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// Offers a line at simulation tick `now`.
    ///
    /// It starts at once when idle, preempts the active line when strictly
    /// higher priority and that line is interruptible, and otherwise waits
    /// (highest priority first, then identity order).
    pub fn enqueue(&mut self, line: RadioLine, now: Tick) -> Vec<RadioEvent> {
        let id = line.id;
        if id.session != self.session {
            return vec![RadioEvent::RefusedForeignSession { id }];
        }
        if self
            .highest_sequence
            .get(&id.producer)
            .is_some_and(|seen| id.sequence <= *seen)
        {
            return vec![RadioEvent::SuppressedDuplicate { id }];
        }
        let preempts = self
            .active
            .as_ref()
            .is_some_and(|a| a.line.interruptible && line.priority > a.line.priority);
        if self.active.is_some() && !preempts && self.pending.len() >= MAX_PENDING_RADIO_LINES {
            return vec![RadioEvent::RefusedQueueFull { id }];
        }
        self.highest_sequence.insert(id.producer, id.sequence);
        let mut events = Vec::new();
        if preempts {
            if let Some(cut) = self.active.take() {
                events.push(RadioEvent::Interrupted {
                    id: cut.line.id,
                    by: id,
                });
            }
            self.start(line, now, &mut events);
        } else if self.active.is_none() {
            self.start(line, now, &mut events);
        } else {
            let at = self.pending.partition_point(|p| {
                (std::cmp::Reverse(p.priority), p.id) <= (std::cmp::Reverse(line.priority), id)
            });
            self.pending.insert(at, line);
        }
        events
    }

    /// Advances to tick `now`: completes the active line when its duration has
    /// elapsed and starts the next pending one. Total: no device is consulted.
    pub fn advance(&mut self, now: Tick) -> Vec<RadioEvent> {
        let mut events = Vec::new();
        let done = self
            .active
            .as_ref()
            .is_some_and(|a| now.0.saturating_sub(a.started.0) >= a.line.duration_ticks);
        if done && let Some(a) = self.active.take() {
            events.push(RadioEvent::Completed {
                id: a.line.id,
                voiced: a.voiced,
            });
        }
        if self.active.is_none() && !self.pending.is_empty() {
            let next = self.pending.remove(0);
            self.start(next, now, &mut events);
        }
        events
    }

    /// The physical device is gone: the active line keeps its tick timing and
    /// subtitle but is no longer voiced; later lines start unvoiced.
    pub fn device_lost(&mut self) -> Vec<RadioEvent> {
        self.device_available = false;
        match self.active.as_mut() {
            Some(a) if a.voiced => {
                a.voiced = false;
                vec![RadioEvent::PlaybackLost { id: a.line.id }]
            }
            _ => Vec::new(),
        }
    }

    /// The device is back: lines that start from now on are voiced. A line
    /// already running is not re-voiced mid-sentence.
    pub fn device_restored(&mut self) {
        self.device_available = true;
    }

    fn start(&mut self, line: RadioLine, now: Tick, events: &mut Vec<RadioEvent>) {
        events.push(RadioEvent::Started {
            id: line.id,
            speaker: line.speaker.clone(),
            asset: line.asset.clone(),
            subtitle: line.subtitle.clone(),
            audible: self.device_available,
        });
        self.active = Some(ActiveLine {
            line,
            started: now,
            voiced: self.device_available,
        });
    }
}

// ---------------------------------------------------------------- music ----

/// One authored music cue request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MusicCue {
    /// The request's identity and dedup key.
    pub id: AudioEventId,
    /// The music asset.
    pub asset: ContentId,
}

/// What a music request produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MusicOutcome {
    /// Music began with nothing before it.
    Started {
        /// The new asset.
        to: ContentId,
        /// Whether a device is playing it.
        audible: bool,
    },
    /// The authored transition from one cue to the next.
    Transition {
        /// The cue left.
        from: ContentId,
        /// The cue entered.
        to: ContentId,
        /// Whether a device is playing it.
        audible: bool,
    },
    /// The requested cue is already current.
    Unchanged,
    /// The request was already accepted.
    SuppressedDuplicate {
        /// The request.
        id: AudioEventId,
    },
    /// The request belongs to another session generation.
    RefusedForeignSession {
        /// The request.
        id: AudioEventId,
    },
    /// The asset is not a music id.
    RefusedNotMusic {
        /// The kind it names.
        kind: ContentKind,
    },
}

/// The per-session music director: authored cue transitions only, no random
/// substitution (F41 non-negotiable behavior 4), surviving device loss as
/// state so it can be re-issued on retry.
#[derive(Clone, Debug)]
pub struct MusicDirector {
    session: u64,
    highest_sequence: BTreeMap<u32, u32>,
    current: Option<MusicCue>,
    device_available: bool,
}

impl MusicDirector {
    /// A director with no cue, over a working device.
    #[must_use]
    pub fn new(session: u64) -> Self {
        Self {
            session,
            highest_sequence: BTreeMap::new(),
            current: None,
            device_available: true,
        }
    }

    /// The current authored cue (kept through device loss).
    #[must_use]
    pub fn current(&self) -> Option<&MusicCue> {
        self.current.as_ref()
    }

    /// Requests a cue; the transition is the caller's authored decision.
    pub fn request(&mut self, cue: MusicCue) -> MusicOutcome {
        if cue.id.session != self.session {
            return MusicOutcome::RefusedForeignSession { id: cue.id };
        }
        if cue.asset.kind() != ContentKind::Music {
            return MusicOutcome::RefusedNotMusic {
                kind: cue.asset.kind(),
            };
        }
        if self
            .highest_sequence
            .get(&cue.id.producer)
            .is_some_and(|seen| cue.id.sequence <= *seen)
        {
            return MusicOutcome::SuppressedDuplicate { id: cue.id };
        }
        self.highest_sequence
            .insert(cue.id.producer, cue.id.sequence);
        let audible = self.device_available;
        let to = cue.asset.clone();
        match self.current.replace(cue) {
            Some(prev) if prev.asset == to => MusicOutcome::Unchanged,
            Some(prev) => MusicOutcome::Transition {
                from: prev.asset,
                to,
                audible,
            },
            None => MusicOutcome::Started { to, audible },
        }
    }

    /// Device lost: the cue stays current but is not playing.
    pub fn device_lost(&mut self) {
        self.device_available = false;
    }

    /// Device back: returns the current cue to restart, if any.
    pub fn device_restored(&mut self) -> Option<&MusicCue> {
        self.device_available = true;
        self.current.as_ref()
    }
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

    /// The designed default law is a valid law, so a voice built from it can
    /// never refuse its own smoothing.
    #[test]
    fn accept_f41_b_designed_smoothing_law_validates() {
        let law = EngineSmoothing::DESIGNED_DEFAULT;
        assert!(law.validate().is_ok(), "the designed law is valid");
        assert!(matches!(
            EngineSmoothing::try_new(f64::NAN, 1.5, 3.0, 0.25, 1.0, 0.7, 1.6),
            Err(EngineAudioError::NonFinite {
                field: "gain_attack_per_s"
            })
        ));
        assert!(matches!(
            EngineSmoothing::try_new(2.0, 0.0, 3.0, 0.25, 1.0, 0.7, 1.6),
            Err(EngineAudioError::NonPositiveRate {
                field: "gain_release_per_s",
                ..
            })
        ));
        assert!(matches!(
            EngineSmoothing::try_new(2.0, 1.5, 3.0, 0.25, 9.0, 0.7, 1.6),
            Err(EngineAudioError::BadGain {
                field: "full_gain",
                ..
            })
        ));
        assert!(matches!(
            EngineSmoothing::try_new(2.0, 1.5, 3.0, 0.25, 1.0, 0.0, 1.6),
            Err(EngineAudioError::NonPositivePitch {
                field: "idle_pitch",
                ..
            })
        ));
    }

    /// A stopped engine asks for silence; a running one interpolates the
    /// spool along the designed span, and a corrupt spool reads as idle.
    #[test]
    fn accept_f41_b_stopped_engine_asks_for_silence() {
        let law = EngineSmoothing::DESIGNED_DEFAULT;
        assert_eq!(
            law.target(false, 1.0),
            VoiceLevel::new(0.0, law.idle_pitch())
        );
        assert_eq!(
            law.target(true, 0.0),
            VoiceLevel::new(law.idle_gain(), law.idle_pitch())
        );
        assert_eq!(
            law.target(true, 1.0),
            VoiceLevel::new(law.full_gain(), law.full_pitch())
        );
        let half = law.target(true, 0.5);
        assert!((half.gain - (law.idle_gain() + law.full_gain()) / 2.0).abs() < 1e-12);
        assert!((half.pitch - (law.idle_pitch() + law.full_pitch()) / 2.0).abs() < 1e-12);
        assert_eq!(law.target(true, f64::NAN), law.target(true, 0.0));
        assert_eq!(law.target(true, 4.0), law.target(true, 1.0));
    }

    /// A corrupt clock cannot move a voice, and a voice never leaves its
    /// validated domain.
    #[test]
    fn accept_f41_b_corrupt_step_changes_no_voice() {
        let law = EngineSmoothing::DESIGNED_DEFAULT;
        let mut voice = EngineVoice::at_idle(&law);
        let before = voice.level();
        for dt in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            voice.advance(&law, VoiceLevel::new(1.0, 1.6), dt);
            assert_eq!(voice.level(), before, "dt {dt} must change nothing");
        }
        voice.advance(&law, VoiceLevel::new(1.0, 1.6), 10.0);
        assert!(voice.level().validate().is_ok());
        assert!(voice.gain() <= law.full_gain() && voice.pitch() <= law.full_pitch());
    }
}
