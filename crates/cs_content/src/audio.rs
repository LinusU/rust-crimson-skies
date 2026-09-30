//! The declared audio catalog: buses, playback metadata and decoded-asset
//! references with provenance (F41-A).
//!
//! Spec: `specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stage
//! `### F41-A`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! This module is the **content half** of the audio contract — the normalized,
//! provenance-carrying record an audio importer produces and the catalog
//! consumes. Its runtime counterpart is `cs_sim::audio_events` (the event
//! identity, the one-shot dedup ledger and the loop-emitter lifecycle) and the
//! conversion boundary is `cs_app::audio`. The split mirrors
//! `damage` ↔ `cs_sim::damage`: this crate cannot depend on `cs_sim`, so the
//! declared record keeps its own typed fields.
//!
//! # Buses and playback metadata
//!
//! F41's deliverable names seven mix buses — engine, weapons, impacts,
//! environment, music, radio and UI — so accessible mixing can change what a
//! listener hears without touching game state. [`AudioBus`] is that closed,
//! validated vocabulary. One [`AudioAssetRecord`] binds an audio content id
//! (`sound`, `music` or `dialogue`) to its [`AudioPlayback`] metadata (bus,
//! linear level, one-shot/loop mode) and to a [`DecodedPcm`] reference: the
//! shape of the immutable decoded asset F06-C's decoder will produce in
//! F41-B. The samples themselves stay out of the catalog.
//!
//! # Known or explicitly unknown, never defaulted
//!
//! Every playback field travels as a [`Resolved`], exactly like the rest of the
//! catalog schema: a bus, level or loop mode that has not been measured is an
//! explicit unknown carrying its claim id and reason, never a `Designed`
//! default silently standing in for a measurement (F41 "Research boundary").
//! The lowering boundary (`cs_app::audio`) is where an unresolved mandatory
//! field refuses rather than plays a guessed cue.
//!
//! # What is designed and what is unknown
//!
//! The original audio pipeline is **not decoded**: F41-B owns decoding, and
//! the F06 findings record that retail sound members are mostly IMA/MS ADPCM,
//! that no member declares loop points and that the original engine's mixing,
//! doppler and attenuation are unmeasured (`docs/findings/
//! 2026-09-28-f06-c-vfs-members-and-audio-assets.md`,
//! `docs/findings/2026-09-28-t344-zbd-sound-member-wave-headers.md`). Every
//! value here is newly authored project design carrying `Designed` or
//! `SyntheticFixture` provenance; nothing in this module is a measurement of
//! the original game.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cs_types::content::{ContentId, ContentKind, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

/// Maximum validated linear gain.
///
/// A designed ceiling, not an original measurement: it keeps a corrupt
/// producer from requesting an unbounded amplification while still allowing
/// deliberate boost. Unity (`1.0`) is the neutral level.
pub const MAX_AUDIO_GAIN: f64 = 8.0;

// ----------------------------------------------------------------- buses ---

/// One of the seven named mix buses F41 requires.
///
/// The set is closed and validated: two records on the same bus are mixed
/// together, and a bus cannot be invented by a misspelling. Which fader,
/// effect chain or voice limit a bus owns is a runtime concern (F41-B);
/// this stage only fixes the vocabulary the catalog and the runtime agree on.
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
    /// Every bus, in a stable order. [`AudioBus::from_label`] scans this table,
    /// so `label` and `from_label` cannot disagree about one bus.
    pub const ALL: &'static [AudioBus] = &[
        Self::Engine,
        Self::Weapons,
        Self::Impacts,
        Self::Environment,
        Self::Music,
        Self::Radio,
        Self::Ui,
    ];

    /// The stable label used in reports and mix configuration.
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

// ----------------------------------------------------------------- level ---

/// Why an [`AudioLevel`] was refused.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AudioLevelError {
    /// The level was NaN or infinite.
    NonFinite {
        /// The rejected value.
        value: f64,
    },
    /// The level was negative; a negative linear gain inverts a signal.
    Negative {
        /// The rejected value.
        value: f64,
    },
    /// The level exceeded [`MAX_AUDIO_GAIN`].
    AboveMaximum {
        /// The rejected value.
        value: f64,
    },
}

impl fmt::Display for AudioLevelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite { value } => write!(f, "audio level {value} is not finite"),
            Self::Negative { value } => write!(f, "audio level {value} is negative"),
            Self::AboveMaximum { value } => {
                write!(
                    f,
                    "audio level {value} is above the maximum {MAX_AUDIO_GAIN}"
                )
            }
        }
    }
}

impl std::error::Error for AudioLevelError {}

/// A validated linear gain.
///
/// Finite, non-negative and at most [`MAX_AUDIO_GAIN`] by construction, so a
/// corrupt authoring document cannot reach the mixer as a `NaN` or an
/// unbounded amplification.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct AudioLevel(f64);

impl AudioLevel {
    /// The neutral level, `1.0`.
    pub const UNITY: Self = Self(1.0);

    /// Validates and wraps a linear gain.
    ///
    /// # Errors
    ///
    /// [`AudioLevelError::NonFinite`], [`AudioLevelError::Negative`] or
    /// [`AudioLevelError::AboveMaximum`].
    pub fn try_new(value: f64) -> Result<Self, AudioLevelError> {
        if !value.is_finite() {
            return Err(AudioLevelError::NonFinite { value });
        }
        if value < 0.0 {
            return Err(AudioLevelError::Negative { value });
        }
        if value > MAX_AUDIO_GAIN {
            return Err(AudioLevelError::AboveMaximum { value });
        }
        Ok(Self(value))
    }

    /// The validated linear gain.
    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }
}

impl fmt::Display for AudioLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ------------------------------------------------------------ playback -----

/// Whether a cue plays once or loops until it is stopped.
///
/// This is the declared mode the runtime routes on: a one-shot is deduplicated
/// per event id (F41 non-negotiable behavior 3) and a loop is bound to an
/// emitter that stops on despawn, swap, pause policy or device loss.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlaybackMode {
    /// Plays once per accepted event.
    OneShot,
    /// Loops until its emitter stops it.
    Loop,
}

impl PlaybackMode {
    /// Every mode, in a stable order.
    pub const ALL: &'static [PlaybackMode] = &[Self::OneShot, Self::Loop];

    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::OneShot => "one_shot",
            Self::Loop => "loop",
        }
    }

    /// Looks a mode up by its label; `None` for an unknown mode.
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|mode| mode.label() == label)
    }
}

impl fmt::Display for PlaybackMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Why a [`DecodedPcm`] shape was refused.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DecodedPcmError {
    /// A recording with no channels has no signal.
    ZeroChannels,
    /// A recording with no sample rate cannot be scheduled.
    ZeroRate {
        /// The rejected rate.
        rate_hz: u32,
    },
}

impl fmt::Display for DecodedPcmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroChannels => write!(f, "a decoded PCM asset must have at least one channel"),
            Self::ZeroRate { rate_hz } => {
                write!(
                    f,
                    "a decoded PCM asset rate {rate_hz} Hz is not a valid rate"
                )
            }
        }
    }
}

impl std::error::Error for DecodedPcmError {}

/// The shape of one decoded, immutable PCM asset.
///
/// This is a *reference* to what F06-C's decoder produced, not the samples:
/// frames, channels and rate are enough to schedule playback, and the payload
/// itself is owned by the audio asset store (F41-B). Which widths a decoder
/// supports and how ADPCM blocks are expanded is F06-C's and F41-B's business,
/// not this record's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodedPcm {
    frames: u64,
    channels: u16,
    rate_hz: u32,
}

impl DecodedPcm {
    /// Validates a decoded-asset shape.
    ///
    /// An empty recording (zero frames) is allowed — F06-C treats an empty
    /// `data` chunk as a whole number of zero frames — but a recording with no
    /// channel or no rate is not a schedulable asset.
    ///
    /// # Errors
    ///
    /// [`DecodedPcmError::ZeroChannels`] or [`DecodedPcmError::ZeroRate`].
    pub fn try_new(frames: u64, channels: u16, rate_hz: u32) -> Result<Self, DecodedPcmError> {
        if channels == 0 {
            return Err(DecodedPcmError::ZeroChannels);
        }
        if rate_hz == 0 {
            return Err(DecodedPcmError::ZeroRate { rate_hz });
        }
        Ok(Self {
            frames,
            channels,
            rate_hz,
        })
    }

    /// The number of whole frames.
    #[must_use]
    pub const fn frames(self) -> u64 {
        self.frames
    }

    /// The channel count.
    #[must_use]
    pub const fn channels(self) -> u16 {
        self.channels
    }

    /// The sample rate, in hertz.
    #[must_use]
    pub const fn rate_hz(self) -> u32 {
        self.rate_hz
    }
}

// -------------------------------------------------------------- record -----

/// The declared playback metadata of one audio asset.
///
/// Each field is resolved independently, so a known bus with an unmeasured
/// level stays playable-by-bus while its level is visibly unknown, instead of
/// the whole record being discarded.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioPlayback {
    /// The bus this asset is mixed on, or an explicit unknown.
    pub bus: Resolved<AudioBus>,
    /// The linear level, or an explicit unknown.
    pub level: Resolved<AudioLevel>,
    /// Whether the cue is a one-shot or a loop, or an explicit unknown.
    pub mode: Resolved<PlaybackMode>,
}

/// The raw parts of an [`AudioAssetRecord`], so the validating constructor
/// takes one record instead of a long positional argument list.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioDraft {
    /// The `sound`, `music` or `dialogue` content id.
    pub id: ContentId,
    /// Where the record came from.
    pub origin: Origin,
    /// The declared playback metadata.
    pub playback: AudioPlayback,
    /// The decoded-asset shape, or an explicit unknown while it is undecoded.
    pub decoded: Resolved<DecodedPcm>,
    /// The provenance of the record itself.
    pub provenance: Provenance,
}

/// Why an [`AudioAssetRecord`] was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AudioError {
    /// The id is not in the `sound`, `music` or `dialogue` namespace.
    NotAnAudioAsset {
        /// The kind it actually names.
        kind: ContentKind,
    },
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnAudioAsset { kind } => {
                write!(
                    f,
                    "audio asset id names a {kind}, not a sound, music or dialogue"
                )
            }
        }
    }
}

impl std::error::Error for AudioError {}

/// `true` for the three content kinds an audio catalog may hold.
#[must_use]
pub fn is_audio_kind(kind: ContentKind) -> bool {
    matches!(
        kind,
        ContentKind::Sound | ContentKind::Music | ContentKind::Dialogue
    )
}

/// One catalog row: an audio id bound to its playback metadata, decoded-asset
/// reference and provenance.
///
/// The record is immutable once validated; the fields are read through
/// accessors so a consumer cannot rename an asset or move it to another bus
/// after insertion.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioAssetRecord {
    id: ContentId,
    origin: Origin,
    playback: AudioPlayback,
    decoded: Resolved<DecodedPcm>,
    provenance: Provenance,
}

impl AudioAssetRecord {
    /// Validates and assembles a declared audio asset.
    ///
    /// # Errors
    ///
    /// [`AudioError::NotAnAudioAsset`] when the id is not a `sound`, `music`
    /// or `dialogue` id. Unknown playback metadata is content, not an
    /// authoring error — it is refused later, at the lowering boundary.
    pub fn try_new(draft: AudioDraft) -> Result<Self, AudioError> {
        let AudioDraft {
            id,
            origin,
            playback,
            decoded,
            provenance,
        } = draft;
        if !is_audio_kind(id.kind()) {
            return Err(AudioError::NotAnAudioAsset { kind: id.kind() });
        }
        Ok(Self {
            id,
            origin,
            playback,
            decoded,
            provenance,
        })
    }

    /// The asset's content id.
    #[must_use]
    pub fn id(&self) -> &ContentId {
        &self.id
    }

    /// Where the record's bytes came from.
    #[must_use]
    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The declared playback metadata.
    #[must_use]
    pub fn playback(&self) -> &AudioPlayback {
        &self.playback
    }

    /// The decoded-asset shape, or an explicit unknown.
    #[must_use]
    pub fn decoded(&self) -> &Resolved<DecodedPcm> {
        &self.decoded
    }

    /// The provenance of the record itself.
    #[must_use]
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

// ------------------------------------------------------------- catalog -----

/// Why the catalog refused a record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AudioCatalogError {
    /// Two records share one id; one identity must name one asset.
    DuplicateId {
        /// The duplicated id.
        id: ContentId,
    },
}

impl fmt::Display for AudioCatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateId { id } => {
                write!(f, "audio asset id {id} is inserted more than once")
            }
        }
    }
}

impl std::error::Error for AudioCatalogError {}

/// The audio catalog: every declared audio asset, keyed by stable id.
///
/// Records are kept in id order, so iteration is deterministic and does not
/// depend on insertion order. Two records may not share an id: a second insert
/// of the same id is refused by name rather than silently replacing the first
/// (`IDENTITY-CONTENT`: stable content ids).
#[derive(Clone, Debug, Default)]
pub struct AudioCatalog {
    records: BTreeMap<ContentId, AudioAssetRecord>,
}

impl AudioCatalog {
    /// An empty catalog.
    #[must_use]
    pub fn new() -> Self {
        Self {
            records: BTreeMap::new(),
        }
    }

    /// Inserts a record, refusing an id that is already present.
    ///
    /// # Errors
    ///
    /// [`AudioCatalogError::DuplicateId`].
    pub fn insert(&mut self, record: AudioAssetRecord) -> Result<(), AudioCatalogError> {
        let id = record.id.clone();
        if self.records.contains_key(&id) {
            return Err(AudioCatalogError::DuplicateId { id });
        }
        self.records.insert(id, record);
        Ok(())
    }

    /// The record with `id`, if the catalog holds one.
    #[must_use]
    pub fn get(&self, id: &ContentId) -> Option<&AudioAssetRecord> {
        self.records.get(id)
    }

    /// Whether `id` is in the catalog.
    #[must_use]
    pub fn contains(&self, id: &ContentId) -> bool {
        self.records.contains_key(id)
    }

    /// How many records the catalog holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether the catalog holds no records.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// The records, in id order.
    pub fn iter(&self) -> impl Iterator<Item = &AudioAssetRecord> {
        self.records.values()
    }

    /// The buses that carry at least one *known* record.
    ///
    /// A record whose bus is [`Resolved::Unknown`] contributes no bus: an
    /// unmeasured routing is not evidence that a bus exists.
    #[must_use]
    pub fn buses(&self) -> BTreeSet<AudioBus> {
        self.records
            .values()
            .filter_map(|record| record.playback.bus.clone().known())
            .collect()
    }

    /// The required buses with no known record, in [`AudioBus::ALL`] order.
    ///
    /// The catalog is expected to cover all seven buses F41 names; this reports
    /// which are still unpopulated instead of assuming coverage.
    #[must_use]
    pub fn missing_buses(&self) -> Vec<AudioBus> {
        let present = self.buses();
        AudioBus::ALL
            .iter()
            .copied()
            .filter(|bus| !present.contains(bus))
            .collect()
    }
}

// ------------------------------------------------------------ fixtures -----

/// The key of the synthetic weapon one-shot used by F41-A's minimum scenario.
pub const SYNTHETIC_WEAPON_SOUND_KEY: &str = "synthetic.weapon.machinegun";

/// Builds the minimal declared synthetic audio catalog.
///
/// One asset per bus so the fixture exercises the whole bus vocabulary, with a
/// one-shot weapon cue ([`SYNTHETIC_WEAPON_SOUND_KEY`]) at the centre of the
/// F41-A minimum scenario. Every id lives under the `synthetic` key, every
/// record carries [`Origin::SyntheticFixture`] and a designed provenance, and
/// every value is newly authored development content — it can never be mistaken
/// for an original audio asset and cannot stand in for one.
#[must_use]
pub fn declared_synthetic_audio_catalog() -> AudioCatalog {
    let mut catalog = AudioCatalog::new();
    let designed =
        || Provenance::designed(ClaimId::new("f41a.synthetic-audio-catalog").expect("valid"));
    let playback = |bus: AudioBus, level: f64, mode: PlaybackMode| AudioPlayback {
        bus: Resolved::Known(cs_types::content::Known::new(bus, designed())),
        level: Resolved::Known(cs_types::content::Known::new(
            AudioLevel::try_new(level).expect("fixture level is valid"),
            designed(),
        )),
        mode: Resolved::Known(cs_types::content::Known::new(mode, designed())),
    };
    let record = |kind: ContentKind,
                  key: &str,
                  bus: AudioBus,
                  level: f64,
                  mode: PlaybackMode,
                  pcm_shape: DecodedPcm| {
        AudioAssetRecord::try_new(AudioDraft {
            id: ContentId::from_source(kind, key).expect("fixture id is valid"),
            origin: Origin::SyntheticFixture,
            playback: playback(bus, level, mode),
            decoded: Resolved::Known(cs_types::content::Known::new(pcm_shape, designed())),
            provenance: designed(),
        })
        .expect("the fixture record is valid")
    };

    let entries = [
        (
            ContentKind::Sound,
            SYNTHETIC_WEAPON_SOUND_KEY,
            AudioBus::Weapons,
            PlaybackMode::OneShot,
            DecodedPcm::try_new(2205, 1, 22050).expect("valid"),
        ),
        (
            ContentKind::Sound,
            "synthetic.engine.loop",
            AudioBus::Engine,
            PlaybackMode::Loop,
            DecodedPcm::try_new(11025, 1, 11025).expect("valid"),
        ),
        (
            ContentKind::Sound,
            "synthetic.impact.hit",
            AudioBus::Impacts,
            PlaybackMode::OneShot,
            DecodedPcm::try_new(1102, 1, 22050).expect("valid"),
        ),
        (
            ContentKind::Sound,
            "synthetic.environment.wind",
            AudioBus::Environment,
            PlaybackMode::Loop,
            DecodedPcm::try_new(22050, 1, 22050).expect("valid"),
        ),
        (
            ContentKind::Music,
            "synthetic.music.title",
            AudioBus::Music,
            PlaybackMode::Loop,
            DecodedPcm::try_new(44100, 2, 44100).expect("valid"),
        ),
        (
            ContentKind::Dialogue,
            "synthetic.radio.wingman",
            AudioBus::Radio,
            PlaybackMode::OneShot,
            DecodedPcm::try_new(33075, 1, 22050).expect("valid"),
        ),
        (
            ContentKind::Sound,
            "synthetic.ui.click",
            AudioBus::Ui,
            PlaybackMode::OneShot,
            DecodedPcm::try_new(441, 1, 22050).expect("valid"),
        ),
    ];
    for (kind, key, bus, mode, shape) in entries {
        catalog
            .insert(record(kind, key, bus, 1.0, mode, shape))
            .expect("fixture ids are unique");
    }
    catalog
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claim(id: &str) -> ClaimId {
        ClaimId::new(id).expect("valid claim id")
    }

    fn designed() -> Provenance {
        Provenance::designed(claim("f41a.unit"))
    }

    fn record(kind: ContentKind, key: &str) -> AudioAssetRecord {
        AudioAssetRecord::try_new(AudioDraft {
            id: ContentId::from_source(kind, key).expect("valid id"),
            origin: Origin::SyntheticFixture,
            playback: AudioPlayback {
                bus: Resolved::Known(cs_types::content::Known::new(AudioBus::Weapons, designed())),
                level: Resolved::Known(cs_types::content::Known::new(
                    AudioLevel::UNITY,
                    designed(),
                )),
                mode: Resolved::Known(cs_types::content::Known::new(
                    PlaybackMode::OneShot,
                    designed(),
                )),
            },
            decoded: Resolved::unknown(claim("f41a.unit.undecoded"), "not decoded yet")
                .expect("a reason is present"),
            provenance: designed(),
        })
        .expect("the record is valid")
    }

    /// A non-audio id is refused; the three audio kinds are accepted.
    #[test]
    fn accept_f41_a_records_refuse_non_audio_ids() {
        assert_eq!(
            AudioAssetRecord::try_new(AudioDraft {
                id: ContentId::from_source(ContentKind::Mesh, "synthetic.mesh").expect("valid"),
                origin: Origin::Designed,
                playback: AudioPlayback {
                    bus: Resolved::unknown(claim("f41a.unit.bus"), "unmeasured")
                        .expect("a reason is present"),
                    level: Resolved::unknown(claim("f41a.unit.level"), "unmeasured")
                        .expect("a reason is present"),
                    mode: Resolved::unknown(claim("f41a.unit.mode"), "unmeasured")
                        .expect("a reason is present"),
                },
                decoded: Resolved::unknown(claim("f41a.unit.pcm"), "undecoded")
                    .expect("a reason is present"),
                provenance: designed(),
            }),
            Err(AudioError::NotAnAudioAsset {
                kind: ContentKind::Mesh
            })
        );
        for kind in [
            ContentKind::Sound,
            ContentKind::Music,
            ContentKind::Dialogue,
        ] {
            assert!(is_audio_kind(kind));
        }
    }

    /// A level outside the validated range is an authoring error.
    #[test]
    fn accept_f41_a_levels_are_finite_non_negative_and_bounded() {
        assert_eq!(AudioLevel::UNITY.get(), 1.0);
        assert!(matches!(
            AudioLevel::try_new(f64::NAN),
            Err(AudioLevelError::NonFinite { value }) if value.is_nan()
        ));
        assert_eq!(
            AudioLevel::try_new(-0.5),
            Err(AudioLevelError::Negative { value: -0.5 })
        );
        assert_eq!(
            AudioLevel::try_new(MAX_AUDIO_GAIN + 1.0),
            Err(AudioLevelError::AboveMaximum {
                value: MAX_AUDIO_GAIN + 1.0
            })
        );
    }

    /// The catalog keeps ids unique and iterates in id order.
    #[test]
    fn accept_f41_a_catalog_refuses_duplicate_ids() {
        let mut catalog = AudioCatalog::new();
        catalog
            .insert(record(ContentKind::Sound, "synthetic.one"))
            .expect("first insert");
        assert_eq!(catalog.len(), 1);
        assert_eq!(
            catalog.insert(record(ContentKind::Sound, "synthetic.one")),
            Err(AudioCatalogError::DuplicateId {
                id: ContentId::from_source(ContentKind::Sound, "synthetic.one").expect("valid")
            })
        );
        catalog
            .insert(record(ContentKind::Sound, "synthetic.zero"))
            .expect("second distinct insert");
        let order: Vec<&str> = catalog.iter().map(|row| row.id().as_str()).collect();
        assert_eq!(order, vec!["sound/synthetic.one", "sound/synthetic.zero"]);
    }

    /// The declared fixture covers every bus and is synthetic throughout.
    #[test]
    fn accept_f41_a_declared_fixture_covers_every_bus() {
        let catalog = declared_synthetic_audio_catalog();
        assert_eq!(catalog.len(), 7);
        assert_eq!(catalog.buses().len(), AudioBus::ALL.len());
        assert!(catalog.missing_buses().is_empty());
        for row in catalog.iter() {
            assert_eq!(row.origin(), &Origin::SyntheticFixture);
            assert!(!row.origin().is_original());
        }
        let weapon = catalog
            .get(
                &ContentId::from_source(ContentKind::Sound, SYNTHETIC_WEAPON_SOUND_KEY)
                    .expect("valid"),
            )
            .expect("the weapon one-shot is in the fixture");
        assert_eq!(
            weapon.playback().bus.clone().known(),
            Some(AudioBus::Weapons)
        );
        assert_eq!(
            weapon.playback().mode.clone().known(),
            Some(PlaybackMode::OneShot)
        );
    }
}
