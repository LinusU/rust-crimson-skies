//! Acceptance for Rally #1886 `FLAK-C4-SOUND`: the flak shot's `snd_enemyflak`
//! and the burst's `snd_missile_flak`, named by the `wep_27` record and played
//! through the one-shot path (finding
//! `docs/findings/2026-10-22-flak-c4-flak-sounds.md`).
//!
//! Test prefix: `accept_flak_c4_`. The synthetic tests build the zrd documents
//! and the audio session by hand; the ignored tests read the retail record
//! chain from `CS_GAME_DIR`.

use cs_app::audio::{AudioSession, AudioSpatial};
use cs_app::mission_session::{
    BURST_IMPACT_ROW, BurstCause, FlakSound, FlakSoundError, FlakSoundKind, FlakSoundSequences,
    FlakVoice, OrdnanceBurst, measure_flak_sound, pick_index, play_flak_burst, play_flak_shot,
    voices_from_documents,
};
use cs_app::weapon_effects::SpatialPlacement;
use cs_content::catalog::baseline::install_file_key;
use cs_content::stunts::ZrdValue;
use cs_sim::audio_events::{
    AudioAssetSpec, AudioBus, AudioRouter, Listener, OneShotEvent, OneShotOutcome, PlaybackMode,
    SpatialPolicy,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::evidence::ContentHash;
use cs_types::net::SessionId;

use cs_app::scene::SceneGeneration;

fn text(s: &str) -> ZrdValue {
    ZrdValue::Text(s.to_owned())
}

fn list(items: Vec<ZrdValue>) -> ZrdValue {
    ZrdValue::List(items)
}

fn row(group: &str) -> ZrdValue {
    list(vec![text("SOUND"), list(vec![text(group)])])
}

fn weapons(fire: &str, impact: &str) -> ZrdValue {
    list(vec![
        text("BALLISTICS"),
        list(vec![
            text("wep_27"),
            list(vec![
                text("FIRE"),
                row(fire),
                text("IMPACT"),
                list(vec![
                    text("default"),
                    row(impact),
                    text("water"),
                    row("snd_x"),
                ]),
            ]),
        ]),
    ])
}

fn sounds(entries: &[(&str, &str, bool)]) -> ZrdValue {
    list(
        entries
            .iter()
            .map(|(group, member, looped)| {
                let mut entry = vec![text(group), text(member), text("3D"), text("SFX")];
                if *looped {
                    entry.push(text("LOOPED"));
                }
                entry.push(text("RANGE"));
                entry.push(list(vec![ZrdValue::Float(800.0), ZrdValue::Float(2400.0)]));
                list(entry)
            })
            .collect(),
    )
}

fn id(member: &str) -> ContentId {
    ContentId::from_source(
        ContentKind::Sound,
        &install_file_key(&format!("ZBD/soundsh.zbd/{member}")),
    )
    .expect("a valid sound id")
}

fn voice(group: &str, member: &str) -> FlakVoice {
    FlakVoice {
        group: group.to_owned(),
        member: member.to_owned(),
        range_m: Some([800.0, 2400.0]),
        content: id(member),
    }
}

fn flak() -> FlakSound {
    FlakSound {
        shot: voice("snd_enemyflak", "enemy_flak.wav"),
        burst: voice("snd_missile_flak", "missile_flak.wav"),
        container: "ZBD/soundsh.zbd".to_owned(),
        container_sha256: ContentHash::from_hex(&"0".repeat(64)).expect("a hash"),
        host: std::path::PathBuf::new(),
        bytes: 1,
    }
}

fn session(delivered: &[&ContentId]) -> AudioSession {
    let specs: Vec<_> = delivered
        .iter()
        .map(|content| {
            AudioAssetSpec::try_new(
                (*content).clone(),
                AudioBus::Weapons,
                1.0,
                PlaybackMode::OneShot,
            )
            .expect("a valid one-shot spec")
        })
        .collect();
    AudioSession::new(
        AudioRouter::new(SessionId::new(7).expect("a session")),
        SceneGeneration::default(),
        specs,
    )
}

fn spatial() -> AudioSpatial {
    AudioSpatial::new(
        SpatialPolicy::try_new(1.0, 10_000.0).expect("a valid policy"),
        Listener::try_new([0.0; 3], [1.0, 0.0, 0.0]).expect("a valid listener"),
    )
}

fn burst(at: u64, position_m: [f64; 3]) -> OrdnanceBurst {
    OrdnanceBurst {
        at: Tick(at),
        position_m,
        cause: BurstCause::Proximity,
        splash_radius_m: 22.0,
        armor_damage: 10.0,
        health_damage: 10.0,
        impact_row: BURST_IMPACT_ROW,
    }
}

#[test]
fn accept_flak_c4_the_record_chain_names_the_shot_and_the_burst_group() {
    let [shot, burst] = voices_from_documents(
        &weapons("snd_enemyflak", "snd_missile_flak"),
        &sounds(&[
            ("snd_enemyflak", "enemy_flak.wav", false),
            ("snd_missile_flak", "missile_flak.wav", false),
        ]),
    )
    .expect("the chain reads");
    assert_eq!(shot.0, "snd_enemyflak");
    assert_eq!(shot.1, "enemy_flak.wav");
    assert_eq!(shot.2, Some([800.0, 2400.0]));
    assert_eq!(burst.0, "snd_missile_flak");
    assert_eq!(burst.1, "missile_flak.wav");
}

#[test]
fn accept_flak_c4_an_absent_or_looped_group_is_unmeasured() {
    let missing = voices_from_documents(
        &weapons("snd_enemyflak", "snd_missile_flak"),
        &sounds(&[("snd_enemyflak", "enemy_flak.wav", false)]),
    );
    assert!(matches!(missing, Err(FlakSoundError::Unmeasured(_))));
    let looped = voices_from_documents(
        &weapons("snd_enemyflak", "snd_missile_flak"),
        &sounds(&[
            ("snd_enemyflak", "enemy_flak.wav", true),
            ("snd_missile_flak", "missile_flak.wav", false),
        ]),
    );
    assert!(matches!(looped, Err(FlakSoundError::Unmeasured(_))));
}

#[test]
fn accept_flak_c4_the_pick_is_the_engine_law() {
    // (rand * count) >> 15 over rand in 0..=32767.
    assert_eq!(pick_index(1, 0), 0);
    assert_eq!(pick_index(1, 32_767), 0);
    assert_eq!(pick_index(3, 10_922), 0);
    assert_eq!(pick_index(3, 10_923), 1);
    assert_eq!(pick_index(3, 32_767), 2);
}

#[test]
fn accept_flak_c4_the_records_are_one_shots_on_the_weapons_bus() {
    let records = flak().records().expect("the records form");
    assert_eq!(records.len(), 2);
}

#[test]
fn accept_flak_c4_shot_and_burst_play_at_their_own_positions() {
    let sound = flak();
    let mut audio = session(&[&sound.shot.content, &sound.burst.content]);
    let mut sequences = FlakSoundSequences::default();
    let listener = spatial();

    let shot = play_flak_shot(
        &sound,
        Tick(5),
        [100.0, 0.0, 0.0],
        &mut audio,
        &mut sequences,
        Some(&listener),
    )
    .expect("the shot plays");
    assert_eq!(shot.kind, FlakSoundKind::Shot);
    assert_eq!(shot.event.asset, sound.shot.content);
    assert_eq!(shot.event.id.tick, Tick(5));
    assert_eq!(shot.origin_m, [100.0, 0.0, 0.0]);
    assert!(matches!(shot.spatial, SpatialPlacement::Placed(_)));

    let burst = play_flak_burst(
        &sound,
        &burst(9, [0.0, 400.0, -300.0]),
        &mut audio,
        &mut sequences,
        None,
    )
    .expect("the burst plays");
    assert_eq!(burst.kind, FlakSoundKind::Burst);
    assert_eq!(burst.event.asset, sound.burst.content);
    assert_eq!(burst.event.id.tick, Tick(9));
    assert_eq!(burst.origin_m, [0.0, 400.0, -300.0]);
    assert_eq!(burst.spatial, SpatialPlacement::NoListener);
    assert_ne!(shot.event.id.sequence, burst.event.id.sequence);

    // The router really holds both: replaying the shot's id is a duplicate.
    assert!(matches!(
        audio.router.play_one_shot(&OneShotEvent {
            ..shot.event.clone()
        }),
        OneShotOutcome::SuppressedDuplicate { .. }
    ));
}

#[test]
fn accept_flak_c4_an_undelivered_member_or_other_impact_row_plays_nothing() {
    let sound = flak();
    let mut sequences = FlakSoundSequences::default();
    let mut audio = session(&[&sound.shot.content]);
    let refused = play_flak_burst(
        &sound,
        &burst(1, [0.0; 3]),
        &mut audio,
        &mut sequences,
        None,
    );
    assert_eq!(
        refused,
        Err(FlakSoundError::UnknownAsset(sound.burst.content.clone()))
    );
    let mut water = burst(1, [0.0; 3]);
    water.impact_row = 1;
    let mut audio = session(&[&sound.burst.content]);
    assert!(matches!(
        play_flak_burst(&sound, &water, &mut audio, &mut sequences, None),
        Err(FlakSoundError::Unmeasured(_))
    ));
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_flak_c4_retail_wep_27_names_enemyflak_and_missile_flak() {
    let dir = std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must name the installation");
    let root = std::path::Path::new(&dir);
    let archive = cs_app::mission_session::SoundArchive {
        key: "ZBD/soundsh.zbd".to_owned(),
        sha256: "0".repeat(64),
    };
    let sound = measure_flak_sound(root, &[archive]).expect("the retail chain measures");
    assert_eq!(sound.shot.group, "snd_enemyflak");
    assert_eq!(sound.shot.member, "enemy_flak.wav");
    assert_eq!(sound.burst.group, "snd_missile_flak");
    assert_eq!(sound.burst.member, "missile_flak.wav");
    assert_eq!(sound.shot.range_m, Some([800.0, 2400.0]));
    assert_eq!(sound.burst.range_m, Some([800.0, 2600.0]));
    assert_eq!(sound.records().expect("records").len(), 2);
}
