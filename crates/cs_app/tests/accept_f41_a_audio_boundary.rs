//! Acceptance scenarios F41-A at the application boundary: lowering a declared
//! audio catalog into runtime routing records and the ECS emitter binding.
//!
//! Spec: `specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stage
//! `### F41-A`. Task test prefix: `accept_f41_a_`.
//!
//! These tests drive production code only: [`cs_app::audio`]'s
//! [`lower_catalog`] / [`lower_bus`] and the [`AudioEmitterBinding`], plus the
//! `cs_sim::audio_events::AudioRouter` the lowered records feed — the AC01
//! scenario runs declared fixture → lowering → router, so a boundary that
//! invents a bus or drops the dedup fails them.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data.

use cs_app::audio::{AudioEmitterBinding, AudioLowerError, lower_bus, lower_catalog, lower_record};
use cs_app::scene::SceneGeneration;
use cs_content::audio::{
    AudioAssetRecord, AudioBus as DeclaredBus, AudioDraft, AudioPlayback, DecodedPcm,
    PlaybackMode as DeclaredMode, SYNTHETIC_WEAPON_SOUND_KEY, declared_synthetic_audio_catalog,
};
use cs_sim::audio_events::{
    AudioBus, AudioEmitterId, AudioEventId, AudioRouter, MAX_AUDIO_GAIN as RUNTIME_MAX_GAIN,
    OneShotOutcome, PlaybackMode,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

const SESSION: u64 = 21;

fn claim(id: &str) -> ClaimId {
    ClaimId::new(id).expect("valid claim id")
}

fn designed() -> Provenance {
    Provenance::designed(claim("f41a.boundary"))
}

/// Every declared bus maps onto the matching runtime bus, and the two
/// vocabularies cannot drift.
#[test]
fn accept_f41_a_every_declared_bus_lowers_to_its_runtime_twin() {
    assert_eq!(DeclaredBus::ALL.len(), AudioBus::ALL.len());
    for declared in DeclaredBus::ALL {
        let runtime = lower_bus(*declared);
        assert_eq!(runtime.label(), declared.label());
        assert_eq!(AudioBus::from_label(declared.label()), Some(runtime));
    }
}

/// Lowering preserves the declared asset, bus, level and playback form, and
/// carries the decoded reference through unchanged.
#[test]
fn accept_f41_a_lowering_preserves_the_declared_routing() {
    let catalog = declared_synthetic_audio_catalog();
    let lowered = lower_catalog(&catalog).expect("the fixture catalog lowers");
    assert_eq!(lowered.len(), catalog.len());

    let weapon_id =
        ContentId::from_source(ContentKind::Sound, SYNTHETIC_WEAPON_SOUND_KEY).expect("valid");
    let weapon = lowered.get(&weapon_id).expect("the weapon lowered");
    assert_eq!(weapon.spec.asset(), &weapon_id);
    assert_eq!(weapon.spec.bus(), AudioBus::Weapons);
    assert_eq!(weapon.spec.gain(), 1.0);
    assert_eq!(weapon.spec.mode(), PlaybackMode::OneShot);
    assert_eq!(
        weapon.decoded.clone().known(),
        Some(DecodedPcm::try_new(2205, 1, 22050).expect("valid"))
    );

    // The engine record is a loop on the engine bus.
    let engine = lowered
        .get(&ContentId::from_source(ContentKind::Sound, "synthetic.engine.loop").expect("valid"))
        .expect("the engine loop lowered");
    assert_eq!(engine.spec.bus(), AudioBus::Engine);
    assert_eq!(engine.spec.mode(), PlaybackMode::Loop);

    // The two designed gain ceilings must not drift: a declared level the
    // content side accepts must be one the runtime accepts.
    assert_eq!(
        cs_content::audio::MAX_AUDIO_GAIN,
        RUNTIME_MAX_GAIN,
        "content and runtime gain ceilings disagree"
    );
}

/// A mandatory playback field that is an explicit unknown is refused with its
/// claim, rather than lowered to a guessed bus, level or mode.
#[test]
fn accept_f41_a_lowering_refuses_unknown_playback_fields() {
    let id =
        ContentId::from_source(ContentKind::Dialogue, "synthetic.radio.unmeasured").expect("valid");
    let record_with = |playback: AudioPlayback| {
        AudioAssetRecord::try_new(AudioDraft {
            id: id.clone(),
            origin: Origin::Designed,
            playback,
            decoded: Resolved::unknown(claim("f41a.boundary.pcm"), "not decoded")
                .expect("a reason is present"),
            provenance: Provenance::unknown(claim("f41a.boundary")),
        })
        .expect("the record is valid")
    };
    let known_bus = Resolved::Known(Known::new(DeclaredBus::Radio, designed()));
    let known_level = Resolved::Known(Known::new(cs_content::audio::AudioLevel::UNITY, designed()));
    let known_mode = Resolved::Known(Known::new(DeclaredMode::OneShot, designed()));

    let unknown_bus = record_with(AudioPlayback {
        bus: Resolved::unknown(claim("f41a.boundary.bus"), "routing unmeasured")
            .expect("a reason is present"),
        level: known_level.clone(),
        mode: known_mode.clone(),
    });
    assert_eq!(
        lower_record(&unknown_bus),
        Err(AudioLowerError::UnknownBus {
            id: id.clone(),
            claim_id: claim("f41a.boundary.bus"),
            reason: "routing unmeasured".to_owned(),
        })
    );

    let unknown_level = record_with(AudioPlayback {
        bus: known_bus.clone(),
        level: Resolved::unknown(claim("f41a.boundary.level"), "level unmeasured")
            .expect("a reason is present"),
        mode: known_mode.clone(),
    });
    assert_eq!(
        lower_record(&unknown_level),
        Err(AudioLowerError::UnknownLevel {
            id: id.clone(),
            claim_id: claim("f41a.boundary.level"),
            reason: "level unmeasured".to_owned(),
        })
    );

    let unknown_mode = record_with(AudioPlayback {
        bus: known_bus,
        level: known_level,
        mode: Resolved::unknown(claim("f41a.boundary.mode"), "loop semantics unmeasured")
            .expect("a reason is present"),
    });
    assert_eq!(
        lower_record(&unknown_mode),
        Err(AudioLowerError::UnknownPlaybackMode {
            id,
            claim_id: claim("f41a.boundary.mode"),
            reason: "loop semantics unmeasured".to_owned(),
        })
    );

    // A whole catalog with one unmixable row is refused by that row.
    let mut catalog = declared_synthetic_audio_catalog();
    catalog
        .insert(unknown_bus)
        .expect("the unknown-bus row has a unique id");
    assert!(matches!(
        lower_catalog(&catalog),
        Err(AudioLowerError::UnknownBus { .. })
    ));
}

/// The AC01 scenario through the boundary: the declared fixture lowers, then
/// the weapon event replayed twice plays one accepted one-shot.
#[test]
fn accept_f41_a_lowered_weapon_catalog_plays_one_shot_end_to_end() {
    let catalog = declared_synthetic_audio_catalog();
    let lowered = lower_catalog(&catalog).expect("the fixture lowers");
    let weapon = lowered
        .get(
            &ContentId::from_source(ContentKind::Sound, SYNTHETIC_WEAPON_SOUND_KEY).expect("valid"),
        )
        .expect("the weapon lowered");

    let id = AudioEventId {
        session: SESSION,
        tick: Tick(7),
        producer: 4,
        sequence: 0,
    };
    let event = weapon.spec.one_shot_event(id).expect("a one-shot spec");

    let mut router = AudioRouter::new(SESSION);
    assert_eq!(
        router.play_one_shot(&event),
        OneShotOutcome::Accepted {
            id,
            asset: event.asset.clone(),
            bus: AudioBus::Weapons,
            gain: 1.0,
        }
    );
    assert_eq!(
        router.play_one_shot(&event),
        OneShotOutcome::SuppressedDuplicate { id }
    );

    // The engine loop uses the same boundary to bind an emitter.
    let engine = lowered
        .get(&ContentId::from_source(ContentKind::Sound, "synthetic.engine.loop").expect("valid"))
        .expect("the engine loop lowered");
    let emitter = AudioEmitterId {
        session: SESSION,
        serial: 2,
    };
    let binding = engine
        .spec
        .loop_binding(emitter, id)
        .expect("the engine spec is a loop");
    assert_eq!(binding.bus, AudioBus::Engine);
    assert!(matches!(
        router.start_loop(&binding),
        cs_sim::audio_events::LoopOutcome::Started { .. }
    ));
}

/// The binding record ties an entity's emitter, bus and asset to its scene
/// generation: a reload under a new generation is distinguishable.
#[test]
fn accept_f41_a_emitter_binding_is_generation_qualified() {
    let asset = ContentId::from_source(ContentKind::Sound, "synthetic.engine.loop").expect("valid");
    let binding = AudioEmitterBinding {
        emitter: AudioEmitterId {
            session: SESSION,
            serial: 1,
        },
        bus: AudioBus::Engine,
        asset: asset.clone(),
        generation: SceneGeneration(1),
    };
    let reloaded = AudioEmitterBinding {
        generation: SceneGeneration(1).next(),
        ..binding.clone()
    };
    assert_ne!(binding, reloaded, "a reload stamps a new generation");
    assert_eq!(binding.asset, asset);
    assert_eq!(binding.bus, AudioBus::Engine);
}
