//! Acceptance for Rally #1865 `PARITY6-M01-AUDIO-TIMELINE.05`: the player's
//! bullet-hit ricochets. A gun round routed onto the player plays one pick of
//! `bullet_hit_sg`, drawn with the engine's group selector (finding
//! `docs/findings/2026-10-23-m01-audio-sfx-group-pick.md`).
//!
//! The synthetic tests build the zrd document and the audio session by hand;
//! the ignored test reads the retail `sounds.zrd` from `CS_GAME_DIR`.
//! Task test prefix: `accept_parity6_m01_audio_05_`.

use std::collections::BTreeSet;

use cs_app::audio::AudioSession;
use cs_app::mission_session::{
    BULLET_HIT_GROUP, CRand, GroupPick, HitSound, HitSoundError, HitSoundState, measure_hit_sound,
    play_hit_cue, voices_from_document,
};
use cs_app::scene::SceneGeneration;
use cs_content::catalog::baseline::install_file_key;
use cs_content::stunts::ZrdValue;
use cs_sim::audio_events::{AudioAssetSpec, AudioBus, AudioRouter, PlaybackMode};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::evidence::ContentHash;
use cs_types::net::SessionId;

use super::accept_parity6_m01_audio_01_sample_identity::game_dir;

const FILES: [(&str, &str); 4] = [
    ("snd_ricochet1", "ricco4.wav"),
    ("snd_ricochet2", "ricco1.wav"),
    ("snd_ricochet3", "ricco3.wav"),
    ("snd_ricochet4", "ricco2.wav"),
];

fn text(s: &str) -> ZrdValue {
    ZrdValue::Text(s.to_owned())
}

fn list(items: Vec<ZrdValue>) -> ZrdValue {
    ZrdValue::List(items)
}

/// A `sounds.zrd` shaped like the retail one: the group record naming plain
/// member lists, then the single-sound records.
fn document(extra_member_keyword: bool) -> ZrdValue {
    let mut group = vec![
        text(BULLET_HIT_GROUP),
        text("DYNAMIC_WEIGHTS"),
        ZrdValue::Float(0.5),
    ];
    for (name, _) in FILES {
        let mut member = vec![text(name)];
        if extra_member_keyword {
            member.push(text("WEIGHT"));
            member.push(ZrdValue::Float(10.0));
        }
        group.push(list(member));
    }
    let mut all = vec![list(group)];
    for (name, file) in FILES {
        all.push(list(vec![text(name), text(file), text("3D"), text("SFX")]));
    }
    list(all)
}

fn id(member: &str) -> ContentId {
    ContentId::from_source(
        ContentKind::Sound,
        &install_file_key(&format!("ZBD/soundsh.zbd/{member}")),
    )
    .expect("a valid sound id")
}

fn hit_sound() -> HitSound {
    let (rows, factor) = voices_from_document(&document(false)).expect("the group reads");
    HitSound {
        voices: rows
            .into_iter()
            .map(|(name, member)| cs_app::mission_session::HitVoice {
                content: id(&member),
                name,
                member,
            })
            .collect(),
        factor,
        container: "ZBD/soundsh.zbd".to_owned(),
        container_sha256: ContentHash::from_hex(&"0".repeat(64)).expect("a hash"),
        host: std::path::PathBuf::new(),
        bytes: 1,
    }
}

fn session(sound: &HitSound, delivered: usize) -> AudioSession {
    let specs: Vec<_> = sound
        .voices
        .iter()
        .take(delivered)
        .map(|voice| {
            AudioAssetSpec::try_new(
                voice.content.clone(),
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

#[test]
fn accept_parity6_m01_audio_05_the_c_runtime_stream_is_the_msvcrt_generator() {
    let mut rand = CRand::new(1);
    let first: Vec<u32> = (0..5).map(|_| rand.next()).collect();
    assert_eq!(first, [41, 18467, 6334, 26500, 19169]);
}

#[test]
fn accept_parity6_m01_audio_05_the_selector_halves_the_pick_and_renormalises() {
    let names: Vec<String> = FILES.iter().map(|(n, _)| (*n).to_owned()).collect();
    let mut group = GroupPick::new(names, 0.5).expect("a group");
    for member in group.members() {
        assert!((member.weight - 25.0).abs() < 1e-4);
    }
    let mut rand = CRand::new(1);
    let picked = group.pick(&mut rand).expect("a member");
    // 4 equal members: the pick drops to 14.29, the others rise to 28.57.
    for (index, member) in group.members().iter().enumerate() {
        let want = if index == picked { 14.2857 } else { 28.5714 };
        assert!(
            (member.weight - want).abs() < 1e-2,
            "{index}: {} vs {want}",
            member.weight
        );
    }
    let sum: f32 = group.members().iter().map(|m| m.weight).sum();
    assert!((sum - 100.0).abs() < 1e-3);
}

#[test]
fn accept_parity6_m01_audio_05_every_member_is_reachable_and_repeats_are_rarer() {
    let names: Vec<String> = FILES.iter().map(|(n, _)| (*n).to_owned()).collect();
    let mut group = GroupPick::new(names, 0.5).expect("a group");
    let mut rand = CRand::new(7);
    let (mut seen, mut repeats, mut previous) = (BTreeSet::new(), 0u32, None);
    let plays = 4000u32;
    for _ in 0..plays {
        let picked = group.pick(&mut rand).expect("a member");
        seen.insert(picked);
        if previous == Some(picked) {
            repeats += 1;
        }
        previous = Some(picked);
    }
    assert_eq!(seen.len(), 4, "all four ricochets are reachable");
    // Repeat odds are 1/7 against 1/4 for an equal draw.
    let rate = f64::from(repeats) / f64::from(plays);
    assert!((0.10..0.20).contains(&rate), "repeat rate {rate}");
}

#[test]
fn accept_parity6_m01_audio_05_a_single_member_group_plays_without_a_draw() {
    let mut group = GroupPick::new(vec!["snd_only".to_owned()], 0.5).expect("a group");
    let mut rand = CRand::new(1);
    let before = rand.clone();
    assert_eq!(group.pick(&mut rand), Some(0));
    assert_eq!(rand, before, "no rand() call for a one-member group");
}

#[test]
fn accept_parity6_m01_audio_05_the_document_reader_refuses_a_keyword_it_does_not_model() {
    let (rows, factor) = voices_from_document(&document(false)).expect("plain members read");
    assert_eq!(
        rows,
        FILES
            .iter()
            .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
            .collect::<Vec<_>>()
    );
    assert!((factor - 0.5).abs() < f32::EPSILON);
    let error = voices_from_document(&document(true)).expect_err("a WEIGHT keyword is refused");
    assert!(matches!(error, HitSoundError::Unmeasured(_)), "{error:?}");
}

#[test]
fn accept_parity6_m01_audio_05_a_hit_plays_one_delivered_member_as_a_one_shot() {
    let sound = hit_sound();
    let mut audio = session(&sound, 4);
    let mut state = HitSoundState::default();
    let mut played = BTreeSet::new();
    for tick in 0..40u64 {
        let index = play_hit_cue(&sound, &mut state, Tick(tick), &mut audio).expect("plays");
        played.insert(sound.voices[index].name.clone());
    }
    assert_eq!(state.played.len(), 40, "one voice per hit");
    assert!(played.len() > 1, "the group varies: {played:?}");
    assert!(played.iter().all(|n| n.starts_with("snd_ricochet")));
}

#[test]
fn accept_parity6_m01_audio_05_an_undelivered_member_is_a_named_refusal() {
    let sound = hit_sound();
    let mut audio = session(&sound, 0);
    let mut state = HitSoundState::default();
    let error = play_hit_cue(&sound, &mut state, Tick(0), &mut audio).expect_err("refused");
    assert!(matches!(error, HitSoundError::UnknownAsset(_)), "{error:?}");
    assert!(state.played.is_empty());
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity6_m01_audio_05_retail_group_is_the_four_ricochets() {
    let root = game_dir();
    let archives = [cs_app::mission_session::SoundArchive {
        key: "ZBD/soundsh.zbd".to_owned(),
        sha256: "0".repeat(64),
    }];
    let sound = measure_hit_sound(&root, &archives).expect("the retail group measures");
    let got: Vec<(&str, &str)> = sound
        .voices
        .iter()
        .map(|v| (v.name.as_str(), v.member.as_str()))
        .collect();
    assert_eq!(got, FILES);
    assert!((sound.factor - 0.5).abs() < f32::EPSILON);
    let records = sound.records().expect("records build");
    assert_eq!(records.len(), 4);
}
