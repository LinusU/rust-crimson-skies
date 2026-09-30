//! Lowering a declared audio catalog into runtime routing records (F41-A).
//!
//! Spec: `specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stage
//! `### F41-A`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! The declared record lives in [`cs_content::audio`], the runtime records in
//! [`cs_sim::audio_events`], and neither crate may see the other: this module
//! is the one place they meet.
//!
//! The translation is deliberately strict. A declared record may carry an
//! explicit unknown for its bus, level or playback mode — that is honest
//! content while original routing is unmeasured — but the runtime cannot mix
//! on an unknown bus or schedule an unknown level, so
//! [`lower_record`] **refuses** the record with the unknown's claim id and
//! reason instead of inventing a `Designed` default. An unmixed cue is a
//! visible gap; a guessed bus is a silent fidelity claim.

use std::collections::BTreeMap;
use std::fmt;

use cs_content::audio::{
    AudioAssetRecord, AudioBus as DeclaredBus, AudioCatalog, DecodedPcm,
    PlaybackMode as DeclaredMode,
};
use cs_sim::audio_events::{
    AudioAssetSpec, AudioBus, AudioEventError, PlaybackMode as RuntimeMode,
};
use cs_types::content::{ContentId, Resolved};
use cs_types::evidence::ClaimId;

/// Why a declared audio record could not be lowered.
#[derive(Clone, Debug, PartialEq)]
pub enum AudioLowerError {
    /// The record's bus is [`Resolved::Unknown`]; a cue cannot be mixed on an
    /// unmeasured bus.
    UnknownBus {
        /// The refused record.
        id: ContentId,
        /// The claim the unknown is recorded under.
        claim_id: ClaimId,
        /// Why the bus is unknown.
        reason: String,
    },
    /// The record's level is [`Resolved::Unknown`]; a cue cannot be scheduled
    /// at an unmeasured level.
    UnknownLevel {
        /// The refused record.
        id: ContentId,
        /// The claim the unknown is recorded under.
        claim_id: ClaimId,
        /// Why the level is unknown.
        reason: String,
    },
    /// The record's playback mode is [`Resolved::Unknown`]; a cue cannot be
    /// scheduled without knowing whether it loops.
    UnknownPlaybackMode {
        /// The refused record.
        id: ContentId,
        /// The claim the unknown is recorded under.
        claim_id: ClaimId,
        /// Why the mode is unknown.
        reason: String,
    },
    /// The runtime refused the assembled spec (an unreachable state while both
    /// crates apply the same asset-kind and gain rules; kept so the boundary
    /// stays honest if they ever diverge).
    Spec {
        /// The refused record.
        id: ContentId,
        /// Why the runtime refused it.
        source: AudioEventError,
    },
}

impl fmt::Display for AudioLowerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownBus {
                id,
                claim_id,
                reason,
            } => write!(
                f,
                "audio record {id} has an unknown bus ({}: {reason}) and cannot be lowered",
                claim_id.as_str()
            ),
            Self::UnknownLevel {
                id,
                claim_id,
                reason,
            } => write!(
                f,
                "audio record {id} has an unknown level ({}: {reason}) and cannot be lowered",
                claim_id.as_str()
            ),
            Self::UnknownPlaybackMode {
                id,
                claim_id,
                reason,
            } => write!(
                f,
                "audio record {id} has an unknown playback mode ({}: {reason}) and cannot be lowered",
                claim_id.as_str()
            ),
            Self::Spec { id, source } => {
                write!(f, "the runtime refused audio record {id}: {source}")
            }
        }
    }
}

impl std::error::Error for AudioLowerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spec { source, .. } => Some(source),
            Self::UnknownBus { .. }
            | Self::UnknownLevel { .. }
            | Self::UnknownPlaybackMode { .. } => None,
        }
    }
}

/// One lowered declared record: the runtime routing spec plus the decoded-asset
/// reference the catalog carried (an unknown decode stays unknown — F41-B does
/// the decoding).
#[derive(Clone, Debug, PartialEq)]
pub struct LoweredAudioAsset {
    /// The runtime routing record.
    pub spec: AudioAssetSpec,
    /// The decoded-asset shape, or an explicit unknown.
    pub decoded: Resolved<DecodedPcm>,
}

/// Lowers one declared audio record into its runtime routing record.
///
/// # Errors
///
/// [`AudioLowerError::UnknownBus`], [`AudioLowerError::UnknownLevel`] or
/// [`AudioLowerError::UnknownPlaybackMode`] when a mandatory field is unknown,
/// and [`AudioLowerError::Spec`] when the runtime refuses the assembled spec.
pub fn lower_record(record: &AudioAssetRecord) -> Result<LoweredAudioAsset, AudioLowerError> {
    let id = record.id().clone();
    let bus = match &record.playback().bus {
        Resolved::Known(known) => lower_bus(known.value),
        Resolved::Unknown { claim_id, reason } => {
            return Err(AudioLowerError::UnknownBus {
                id,
                claim_id: claim_id.clone(),
                reason: reason.clone(),
            });
        }
    };
    let gain = match &record.playback().level {
        Resolved::Known(known) => known.value.get(),
        Resolved::Unknown { claim_id, reason } => {
            return Err(AudioLowerError::UnknownLevel {
                id,
                claim_id: claim_id.clone(),
                reason: reason.clone(),
            });
        }
    };
    let mode = match &record.playback().mode {
        Resolved::Known(known) => lower_mode(known.value),
        Resolved::Unknown { claim_id, reason } => {
            return Err(AudioLowerError::UnknownPlaybackMode {
                id,
                claim_id: claim_id.clone(),
                reason: reason.clone(),
            });
        }
    };
    let spec = AudioAssetSpec::try_new(id.clone(), bus, gain, mode)
        .map_err(|source| AudioLowerError::Spec { id, source })?;
    Ok(LoweredAudioAsset {
        spec,
        decoded: record.decoded().clone(),
    })
}

/// Lowers a whole catalog into runtime routing records keyed by content id.
///
/// # Errors
///
/// The first [`AudioLowerError`] encountered, in id order, so a catalog with
/// one unmixable record is refused by that record rather than silently losing
/// it.
pub fn lower_catalog(
    catalog: &AudioCatalog,
) -> Result<BTreeMap<ContentId, LoweredAudioAsset>, AudioLowerError> {
    let mut lowered = BTreeMap::new();
    for record in catalog.iter() {
        lowered.insert(record.id().clone(), lower_record(record)?);
    }
    Ok(lowered)
}

/// Maps the declared bus onto the runtime bus, field for field.
#[must_use]
pub fn lower_bus(bus: DeclaredBus) -> AudioBus {
    match bus {
        DeclaredBus::Engine => AudioBus::Engine,
        DeclaredBus::Weapons => AudioBus::Weapons,
        DeclaredBus::Impacts => AudioBus::Impacts,
        DeclaredBus::Environment => AudioBus::Environment,
        DeclaredBus::Music => AudioBus::Music,
        DeclaredBus::Radio => AudioBus::Radio,
        DeclaredBus::Ui => AudioBus::Ui,
    }
}

/// Maps the declared playback mode onto the runtime mode.
#[must_use]
pub fn lower_mode(mode: DeclaredMode) -> RuntimeMode {
    match mode {
        DeclaredMode::OneShot => RuntimeMode::OneShot,
        DeclaredMode::Loop => RuntimeMode::Loop,
    }
}
