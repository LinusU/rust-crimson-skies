//! Acceptance scenarios F41-A for the declared audio catalog: buses,
//! playback metadata and decoded-asset references.
//!
//! Spec: `specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stage
//! `### F41-A`. Task test prefix: `accept_f41_a_`.
//!
//! These tests drive the production public API of `cs_content::audio` from
//! outside the crate: the validating record constructor, the catalog's
//! duplicate refusal and the declared synthetic fixture. Removing the
//! duplicate check, the kind check or the level validation fails them.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data.

use cs_content::audio::{
    AudioAssetRecord, AudioBus, AudioCatalog, AudioCatalogError, AudioDraft, AudioError,
    AudioLevel, AudioLevelError, AudioPlayback, DecodedPcm, DecodedPcmError, PlaybackMode,
    SYNTHETIC_WEAPON_SOUND_KEY, declared_synthetic_audio_catalog, is_audio_kind,
};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

fn claim(id: &str) -> ClaimId {
    ClaimId::new(id).expect("valid claim id")
}

fn designed() -> Provenance {
    Provenance::designed(claim("f41a.integration"))
}

fn playback(bus: AudioBus, mode: PlaybackMode) -> AudioPlayback {
    AudioPlayback {
        bus: Resolved::Known(Known::new(bus, designed())),
        level: Resolved::Known(Known::new(AudioLevel::UNITY, designed())),
        mode: Resolved::Known(Known::new(mode, designed())),
    }
}

/// A declared record binds one audio id to its playback metadata and decoded
/// reference; the catalog holds it under that id.
#[test]
fn accept_f41_a_declared_record_binds_id_playback_and_decoded_reference() {
    let id =
        ContentId::from_source(ContentKind::Sound, "synthetic.integration.one").expect("valid id");
    let record = AudioAssetRecord::try_new(AudioDraft {
        id: id.clone(),
        origin: Origin::SyntheticFixture,
        playback: playback(AudioBus::Weapons, PlaybackMode::OneShot),
        decoded: Resolved::Known(Known::new(
            DecodedPcm::try_new(2205, 1, 22050).expect("valid pcm shape"),
            designed(),
        )),
        provenance: designed(),
    })
    .expect("the record is valid");
    assert_eq!(record.id(), &id);
    assert_eq!(record.origin(), &Origin::SyntheticFixture);
    assert_eq!(
        record.playback().bus.clone().known(),
        Some(AudioBus::Weapons)
    );
    assert_eq!(
        record.playback().level.clone().known().map(AudioLevel::get),
        Some(1.0)
    );
    assert_eq!(
        record.playback().mode.clone().known(),
        Some(PlaybackMode::OneShot)
    );
    assert_eq!(
        record.decoded().clone().known(),
        Some(DecodedPcm::try_new(2205, 1, 22050).expect("valid"))
    );

    let mut catalog = AudioCatalog::new();
    catalog.insert(record).expect("first insert");
    assert!(catalog.contains(&id));
    assert_eq!(catalog.get(&id).map(AudioAssetRecord::id), Some(&id));
}

/// The catalog refuses two records with the same identity and keeps id order,
/// and it refuses a record whose id is not an audio namespace.
#[test]
fn accept_f41_a_catalog_refuses_duplicate_ids_and_foreign_kinds() {
    let mut catalog = AudioCatalog::new();
    catalog
        .insert(
            AudioAssetRecord::try_new(AudioDraft {
                id: ContentId::from_source(ContentKind::Sound, "synthetic.dupe").expect("valid"),
                origin: Origin::Designed,
                playback: playback(AudioBus::Impacts, PlaybackMode::OneShot),
                decoded: Resolved::unknown(claim("f41a.integration.pcm"), "not decoded yet")
                    .expect("a reason is present"),
                provenance: designed(),
            })
            .expect("valid"),
        )
        .expect("first insert");

    assert_eq!(
        catalog.insert(
            AudioAssetRecord::try_new(AudioDraft {
                id: ContentId::from_source(ContentKind::Sound, "synthetic.dupe").expect("valid"),
                origin: Origin::Designed,
                playback: playback(AudioBus::Music, PlaybackMode::Loop),
                decoded: Resolved::unknown(claim("f41a.integration.pcm"), "not decoded yet")
                    .expect("a reason is present"),
                provenance: designed(),
            })
            .expect("valid"),
        ),
        Err(AudioCatalogError::DuplicateId {
            id: ContentId::from_source(ContentKind::Sound, "synthetic.dupe").expect("valid")
        })
    );

    // A non-audio namespace is refused by the record constructor.
    assert_eq!(
        AudioAssetRecord::try_new(AudioDraft {
            id: ContentId::from_source(ContentKind::Mesh, "synthetic.mesh").expect("valid"),
            origin: Origin::Designed,
            playback: playback(AudioBus::Environment, PlaybackMode::Loop),
            decoded: Resolved::unknown(claim("f41a.integration.pcm"), "not decoded yet")
                .expect("a reason is present"),
            provenance: designed(),
        }),
        Err(AudioError::NotAnAudioAsset {
            kind: ContentKind::Mesh
        })
    );
    assert!(!is_audio_kind(ContentKind::World));
}

/// An unmeasured bus, level, mode or decode is carried as an explicit unknown,
/// never defaulted.
#[test]
fn accept_f41_a_unmeasured_playback_is_an_explicit_unknown() {
    let record = AudioAssetRecord::try_new(AudioDraft {
        id: ContentId::from_source(ContentKind::Dialogue, "synthetic.radio.unknown")
            .expect("valid"),
        origin: Origin::Designed,
        playback: AudioPlayback {
            bus: Resolved::unknown(claim("f41a.integration.bus"), "original routing unmeasured")
                .expect("a reason is present"),
            level: Resolved::unknown(claim("f41a.integration.level"), "original level unmeasured")
                .expect("a reason is present"),
            mode: Resolved::unknown(claim("f41a.integration.mode"), "loop semantics unmeasured")
                .expect("a reason is present"),
        },
        decoded: Resolved::unknown(claim("f41a.integration.pcm"), "ADPCM not decoded")
            .expect("a reason is present"),
        provenance: Provenance::unknown(claim("f41a.integration")),
    })
    .expect("unknown metadata is content, not an authoring error");
    assert!(!record.playback().bus.is_known());
    assert!(!record.playback().level.is_known());
    assert!(!record.playback().mode.is_known());
    assert!(!record.decoded().is_known());

    // A catalog holding only this record knows no bus: an unmeasured routing is
    // not evidence that a bus exists.
    let mut catalog = AudioCatalog::new();
    catalog.insert(record).expect("insert");
    assert!(catalog.buses().is_empty());
    assert_eq!(catalog.missing_buses().len(), AudioBus::ALL.len());
}

/// Levels are finite, non-negative and bounded; decoded shapes need a channel
/// and a rate.
#[test]
fn accept_f41_a_levels_and_decoded_shapes_are_validated() {
    assert_eq!(
        AudioLevel::try_new(f64::INFINITY),
        Err(AudioLevelError::NonFinite {
            value: f64::INFINITY
        })
    );
    assert_eq!(
        AudioLevel::try_new(-0.25),
        Err(AudioLevelError::Negative { value: -0.25 })
    );
    assert_eq!(
        AudioLevel::try_new(100.0),
        Err(AudioLevelError::AboveMaximum { value: 100.0 })
    );
    assert_eq!(AudioLevel::UNITY.get(), 1.0);

    assert_eq!(
        DecodedPcm::try_new(0, 0, 22050),
        Err(DecodedPcmError::ZeroChannels)
    );
    assert_eq!(
        DecodedPcm::try_new(10, 1, 0),
        Err(DecodedPcmError::ZeroRate { rate_hz: 0 })
    );
    // An empty payload is a whole number of zero frames, not an error.
    let empty = DecodedPcm::try_new(0, 1, 22050).expect("zero frames is a valid empty asset");
    assert_eq!(empty.frames(), 0);
    assert_eq!(empty.channels(), 1);
    assert_eq!(empty.rate_hz(), 22050);
}

/// The declared fixture covers all seven buses with synthetic provenance and
/// places the weapon one-shot on the weapons bus.
#[test]
fn accept_f41_a_declared_fixture_covers_every_bus() {
    let catalog = declared_synthetic_audio_catalog();
    assert_eq!(catalog.len(), AudioBus::ALL.len());
    assert_eq!(catalog.buses().len(), AudioBus::ALL.len());
    assert!(catalog.missing_buses().is_empty());
    for row in catalog.iter() {
        assert_eq!(row.origin(), &Origin::SyntheticFixture);
        assert!(!row.origin().is_original());
        assert!(row.decoded().is_known());
    }

    let weapon = catalog
        .get(
            &ContentId::from_source(ContentKind::Sound, SYNTHETIC_WEAPON_SOUND_KEY).expect("valid"),
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

    // The vocabulary round-trips and is closed.
    let labels: Vec<&str> = AudioBus::ALL.iter().map(|bus| bus.label()).collect();
    let mut unique = labels.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), labels.len(), "bus labels are unique");
    for bus in AudioBus::ALL {
        assert_eq!(AudioBus::from_label(bus.label()), Some(*bus));
    }
    assert_eq!(AudioBus::from_label("nonsense"), None);
    assert_eq!(PlaybackMode::from_label("loop"), Some(PlaybackMode::Loop));
    assert_eq!(PlaybackMode::from_label("nonsense"), None);
}
