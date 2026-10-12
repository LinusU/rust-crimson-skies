//! M01-B-OBJ-2: what `BEGIN_DORMANT`, `WAKE_OBJECTIVE_WHEN_I_SLEEP`,
//! `INACTIVE_COMPLETION_COUNT`, `IDENTITY` and the two sound-group keys do, as
//! measured from the engine image, and their recovery in `ObjectiveRecovery`.
//!
//! The behavior is `cs_content::objectives_engine`; these tests drive it
//! through the production recovery of an `objectives.zrd` document.

use std::path::PathBuf;

use cs_app::objectives::{ObjectiveRecovery, RecordFieldFamily, recover_retail_objectives};
use cs_content::objectives_engine::{CueKind, EngineObjective, EngineObjectives, EngineState};
use cs_content::stunts::ZrdValue;

fn text(value: &str) -> ZrdValue {
    ZrdValue::Text(value.to_owned())
}

fn ints(numbers: &[u32]) -> ZrdValue {
    ZrdValue::List(
        numbers
            .iter()
            .map(|number| ZrdValue::Int(*number))
            .collect(),
    )
}

/// A text argument as the record really carries it: inside the argument list,
/// never bare — a bare text follower is the directive grammar's bare-key case.
fn texts(values: &[&str]) -> ZrdValue {
    ZrdValue::List(values.iter().map(|value| text(value)).collect())
}

fn block(fields: Vec<(&str, ZrdValue)>) -> ZrdValue {
    ZrdValue::List(
        fields
            .into_iter()
            .flat_map(|(key, value)| [text(key), value])
            .collect(),
    )
}

fn record(blocks: Vec<(&str, ZrdValue)>) -> ZrdValue {
    ZrdValue::List(vec![block(blocks)])
}

fn recovery(blocks: Vec<(&str, ZrdValue)>) -> ObjectiveRecovery {
    ObjectiveRecovery::read("zbd/test/m00", &record(blocks))
}

/// The recovered declarations as engine objectives, the loader's way: every
/// loaded objective is [`EngineObjective::loaded`] on its declaration.
fn loaded_engine(recovery: &ObjectiveRecovery) -> EngineObjectives {
    let objectives = (1..=u32::try_from(recovery.engine_objective_count()).expect("small"))
        .map(|number| {
            let declaration = recovery
                .engine_declarations()
                .get(&number)
                .cloned()
                .unwrap_or_default();
            EngineObjective::loaded(declaration)
        })
        .collect();
    EngineObjectives::new(objectives).expect("every target is in range")
}

#[test]
fn accept_m01_b_obj_2_recovers_the_measured_keys() {
    let recovery = recovery(vec![(
        "OBJECTIVE1",
        block(vec![
            ("BEGIN_DORMANT", ints(&[2])),
            ("WAKE_OBJECTIVE_WHEN_I_SLEEP", ints(&[1])),
            ("INACTIVE1", texts(&["cond"])),
            ("INACTIVE2", texts(&["cond"])),
            ("INACTIVE_COMPLETION_COUNT", ZrdValue::Int(1)),
            (
                "IDENTITY",
                ZrdValue::List(vec![text("PRIMARY"), ZrdValue::Int(4), text("MSG_A")]),
            ),
            ("WAKEUP_SOUND_GROUP", texts(&["snd_a"])),
            ("COMPLETED_SOUND_GROUP", texts(&["snd_b"])),
        ]),
    )]);
    assert_eq!(recovery.fields_read(), 8);
    // The two stage conditions are M01-B-OBJ-3's family, recovered beside ours.
    assert_eq!(recovery.fields_recovered(), 8);
    let declaration = &recovery.engine_declarations()[&1];
    let timers = declaration.begin_dormant.expect("dormant");
    assert_eq!(timers.wake_at, 2.0);
    assert_eq!(timers.nap_after, -1.0);
    assert_eq!(timers.rewake_after, -1.0);
    assert_eq!(timers.sleep_at, -1.0);
    assert_eq!(declaration.wake_when_i_sleep.targets(), &[0]);
    assert_eq!(declaration.inactive_completion_count, Some(1));
    assert_eq!(declaration.declared_inactive_stages, 2);
    let identity = declaration.identity.as_ref().expect("identity");
    assert_eq!(identity.role_code, 1);
    assert_eq!(identity.ordinal, 4);
    assert_eq!(identity.message.as_deref(), Some("MSG_A"));
    assert_eq!(declaration.wakeup_sound_group.as_deref(), Some("snd_a"));
    assert_eq!(declaration.completed_sound_group.as_deref(), Some("snd_b"));
}

#[test]
fn accept_m01_b_obj_2_dormant_presence_not_value_picks_the_load_state() {
    let recovery = recovery(vec![
        (
            "OBJECTIVE1",
            block(vec![("BEGIN_DORMANT", ZrdValue::Int(u32::MAX))]),
        ),
        ("OBJECTIVE2", block(vec![])),
    ]);
    let set = loaded_engine(&recovery);
    let dormant = set.get(0).expect("first");
    assert_eq!(dormant.state, EngineState::Dormant);
    assert!(!dormant.active);
    assert_eq!(
        dormant.timers.wake_at, -1.0,
        "the u32 sentinel reads signed"
    );
    let awake = set.get(1).expect("second");
    assert_eq!(awake.state, EngineState::Awake);
    assert!(awake.active);
}

#[test]
fn accept_m01_b_obj_2_an_identity_needs_its_role_and_ordinal() {
    let recovery = recovery(vec![(
        "OBJECTIVE1",
        block(vec![
            // OBJECTIVE21's measured shape: two elements, no message element.
            (
                "IDENTITY",
                ZrdValue::List(vec![text("SECONDARY"), ZrdValue::Int(11)]),
            ),
            // The loader's first-occurrence rule applies here too.
            (
                "IDENTITY",
                ZrdValue::List(vec![text("TERTIARY"), ZrdValue::Int(1), text("x")]),
            ),
        ]),
    )]);
    assert_eq!(recovery.fields_recovered(), 1);
    let identity = recovery.engine_declarations()[&1]
        .identity
        .as_ref()
        .expect("identity");
    assert_eq!(identity.role_code, 2);
    assert_eq!(identity.ordinal, 11);
    assert_eq!(identity.message, None);
}

#[test]
fn accept_m01_b_obj_2_unmeasured_shapes_stay_unrecovered() {
    let recovery = recovery(vec![(
        "OBJECTIVE1",
        block(vec![
            // A string among the timer elements is the loader's error path.
            (
                "BEGIN_DORMANT",
                ZrdValue::List(vec![ZrdValue::Int(2), text("x")]),
            ),
            // No integer to store verbatim.
            ("INACTIVE_COMPLETION_COUNT", texts(&["five"])),
            // A non-text role element.
            (
                "IDENTITY",
                ZrdValue::List(vec![ZrdValue::Int(1), ZrdValue::Int(1)]),
            ),
            // A group name the loader's empty-name gate rejects.
            ("WAKEUP_SOUND_GROUP", texts(&[""])),
        ]),
    )]);
    assert_eq!(recovery.fields_recovered(), 0);
    let families = recovery.unrecovered_by_family();
    assert_eq!(families[&RecordFieldFamily::Dormancy], 1);
    assert_eq!(families[&RecordFieldFamily::CompletionCount], 1);
    assert_eq!(families[&RecordFieldFamily::Identity], 1);
    assert_eq!(families[&RecordFieldFamily::SoundCue], 1);
    assert!(recovery.engine_declarations().is_empty());
}

#[test]
fn accept_m01_b_obj_2_the_dormant_cycle_runs_the_measured_slots() {
    // OBJECTIVE1's measured shape: wake at mission clock 2, no other timer,
    // wake a second dormant objective when it sleeps — driven here through the
    // recovery's declarations rather than hand-built ones.
    let recovery = recovery(vec![
        (
            "OBJECTIVE1",
            block(vec![
                ("BEGIN_DORMANT", ints(&[2])),
                ("WAKEUP_SOUND_GROUP", texts(&["snd_wake"])),
            ]),
        ),
        (
            "OBJECTIVE2",
            block(vec![("BEGIN_DORMANT", ZrdValue::Int(u32::MAX))]),
        ),
    ]);
    let mut set = loaded_engine(&recovery);
    assert!(set.advance(1.0).woken.is_empty(), "clock 1 < slot 2");
    let outcome = set.advance(1.0);
    assert_eq!(outcome.woken, vec![0], "clock 2 reached the wake slot");
    assert_eq!(set.get(0).expect("first").state, EngineState::Awake);
    assert_eq!(
        set.cues(),
        &[cs_content::objectives_engine::CueEmission {
            objective: 0,
            kind: CueKind::Wakeup,
            group: Some("snd_wake".to_owned()),
        }]
    );
    // The -1 sentinel never self-wakes; a wake list still wakes it.
    assert_eq!(set.get(1).expect("second").state, EngineState::Dormant);
    set.wake(&[1]);
    assert_eq!(set.get(1).expect("second").state, EngineState::Awake);
}

#[test]
fn accept_m01_b_obj_2_completion_emits_the_cue_the_jingle_and_the_notice() {
    let recovery = recovery(vec![(
        "OBJECTIVE1",
        block(vec![
            ("COMPLETED_SOUND_GROUP", texts(&["snd_done"])),
            (
                "IDENTITY",
                ZrdValue::List(vec![text("secondary"), ZrdValue::Int(3), text("MSG")]),
            ),
        ]),
    )]);
    let mut set = loaded_engine(&recovery);
    set.complete(0);
    assert_eq!(set.cues().len(), 2);
    assert_eq!(set.cues()[0].kind, CueKind::Completed);
    assert_eq!(set.cues()[0].group.as_deref(), Some("snd_done"));
    assert_eq!(set.cues()[1].kind, CueKind::RoleJingle, "role 2 jingles");
    let notices = set.hud_notifications();
    assert_eq!(notices.len(), 1);
    assert_eq!(
        notices[0].role_code, 2,
        "role text resolves case-insensitively"
    );
    assert_eq!(notices[0].ordinal, 3);
}

#[test]
fn accept_m01_b_obj_2_the_count_defaults_to_the_declared_stage_count() {
    let recovery = recovery(vec![
        (
            "OBJECTIVE1",
            block(vec![
                ("INACTIVE1", text("a")),
                ("INACTIVE2", text("b")),
                ("INACTIVE3", text("c")),
            ]),
        ),
        (
            "OBJECTIVE2",
            block(vec![
                ("INACTIVE1", text("a")),
                ("INACTIVE2", text("b")),
                ("INACTIVE_COMPLETION_COUNT", ZrdValue::Int(1)),
            ]),
        ),
    ]);
    let set = loaded_engine(&recovery);
    // Absent, the engine's default is the stage count — here the declared
    // count, the resolved count's upper bound.
    assert_eq!(set.inactive_count_threshold(0), Some(3));
    assert_eq!(set.inactive_count_threshold(1), Some(1));
    assert!(set.count_threshold_reachable(0) == Some(true));
    assert!(!set.inactive_count_met(0, 2));
    assert!(set.inactive_count_met(0, 3));
}

fn retail_root() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"))
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_b_obj_2_retail_m01_recovers_every_lifecycle_field() {
    let recovery = recover_retail_objectives(&retail_root(), "zbd/c1c/m01").expect("M01 reads");
    assert_eq!(recovery.blocks().len(), 58);
    assert_eq!(recovery.engine_objective_count(), 58);

    let by_family = |family| {
        recovery
            .recovered()
            .iter()
            .filter(|field| field.family == family)
            .count()
    };
    // The task's own census: 52 BEGIN_DORMANT-family fields, 9 counts,
    // 5 identities, 41 sound groups.
    assert_eq!(by_family(RecordFieldFamily::Dormancy), 52);
    assert_eq!(by_family(RecordFieldFamily::CompletionCount), 9);
    assert_eq!(by_family(RecordFieldFamily::Identity), 5);
    assert_eq!(by_family(RecordFieldFamily::SoundCue), 41);
    let leftover = recovery.unrecovered_by_family();
    assert!(!leftover.contains_key(&RecordFieldFamily::Dormancy));
    assert!(!leftover.contains_key(&RecordFieldFamily::CompletionCount));
    assert!(!leftover.contains_key(&RecordFieldFamily::Identity));
    assert!(!leftover.contains_key(&RecordFieldFamily::SoundCue));
    assert_eq!(
        recovery.fields_recovered() + recovery.unrecovered().len(),
        recovery.fields_read()
    );

    // The declaration census the measurement predicts.
    let declarations = recovery.engine_declarations();
    let dormant_count = (1..=58u32)
        .filter(|number| {
            declarations
                .get(number)
                .is_some_and(|declaration| declaration.begin_dormant.is_some())
        })
        .count();
    assert_eq!(dormant_count, 52);
    // OBJECTIVE1 is the only non-sentinel value: [2] on slot 0.
    assert_eq!(
        declarations[&1].begin_dormant.expect("dormant").wake_at,
        2.0
    );
    for number in 2..=58u32 {
        if let Some(timers) = declarations
            .get(&number)
            .and_then(|declaration| declaration.begin_dormant)
        {
            assert_eq!(timers.wake_at, -1.0, "OBJECTIVE{number} keeps the sentinel");
        }
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_b_obj_2_retail_m01_count_thresholds_and_defaults_match() {
    let recovery = recover_retail_objectives(&retail_root(), "zbd/c1c/m01").expect("M01 reads");
    let declarations = recovery.engine_declarations();
    // The nine raw counts (one block declares the key twice; the engine reads
    // the first): (objective, stored, declared stages).
    let counted: [(usize, u32, u32); 9] = [
        (6, 1, 18),
        (7, 1, 9),
        (8, 5, 18),
        (9, 9, 18),
        (36, 2, 12),
        (37, 6, 12),
        (38, 10, 12),
        (39, 12, 12),
        (55, 2, 18),
    ];
    let set = loaded_engine(&recovery);
    for (number, stored, stages) in counted {
        let declaration = &declarations[&(number as u32)];
        assert_eq!(
            declaration.inactive_completion_count,
            Some(stored),
            "OBJECTIVE{number} count"
        );
        assert_eq!(
            declaration.declared_inactive_stages, stages,
            "OBJECTIVE{number} stage count"
        );
        assert_eq!(
            set.inactive_count_threshold(number - 1),
            Some(stored),
            "OBJECTIVE{number} threshold"
        );
        assert_eq!(
            set.count_threshold_reachable(number - 1),
            Some(true),
            "OBJECTIVE{number} is structurally reachable"
        );
    }
    // The three stage-carrying blocks without a count default to their stage
    // count — the engine's resolved count, for which the declared count is
    // the measured upper bound.
    for (number, stages) in [(28usize, 9u32), (54, 1), (56, 1)] {
        let declaration = &declarations[&(number as u32)];
        assert_eq!(declaration.inactive_completion_count, None);
        assert_eq!(declaration.declared_inactive_stages, stages);
        assert_eq!(set.inactive_count_threshold(number - 1), Some(stages));
    }
    // A stage-less objective's checker never fires (O1 has no INACTIVE<n>).
    assert!(!set.inactive_count_met(0, 58));
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_b_obj_2_retail_m01_identities_and_sound_groups_match() {
    let recovery = recover_retail_objectives(&retail_root(), "zbd/c1c/m01").expect("M01 reads");
    let declarations = recovery.engine_declarations();
    // (objective, role code, ordinal, message)
    let identities = [
        (9u32, 1u32, 1u32, Some("MSG_BRF_NWM1_OBJ1")),
        (11, 1, 2, Some("MSG_BRF_NWM1_OBJ2")),
        (15, 1, 3, Some("MSG_BRF_NWM1_OBJ3")),
        (18, 1, 4, Some("MSG_BRF_NWM1_OBJ4")),
        (21, 2, 11, None),
    ];
    for (number, role_code, ordinal, message) in identities {
        let identity = declarations[&number].identity.as_ref().expect("identity");
        assert_eq!(identity.role_code, role_code, "OBJECTIVE{number} role code");
        assert_eq!(identity.ordinal, ordinal, "OBJECTIVE{number} ordinal");
        assert_eq!(
            identity.message.as_deref(),
            message,
            "OBJECTIVE{number} message element"
        );
    }
    // The cue names are recovered verbatim for every declaring block.
    assert_eq!(
        declarations[&1].wakeup_sound_group.as_deref(),
        Some("snd_NW1Start")
    );
    assert_eq!(
        declarations[&3].completed_sound_group.as_deref(),
        Some("snd_NW1Prim2")
    );
    assert_eq!(
        declarations[&26].wakeup_sound_group.as_deref(),
        Some("music_primaryobj_sg")
    );
    let wake_groups = declarations
        .values()
        .filter(|declaration| declaration.wakeup_sound_group.is_some())
        .count();
    let done_groups = declarations
        .values()
        .filter(|declaration| declaration.completed_sound_group.is_some())
        .count();
    assert_eq!(wake_groups + done_groups, 41);
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_b_obj_2_retail_m01_objective1_wakes_at_two_seconds_with_its_cue() {
    let recovery = recover_retail_objectives(&retail_root(), "zbd/c1c/m01").expect("M01 reads");
    let mut set = loaded_engine(&recovery);
    // Every BEGIN_DORMANT [-1] objective loads dormant and stays dormant:
    // only OBJECTIVE1's [2] self-wakes, at mission clock 2.
    let outcome = set.advance(3.0);
    assert_eq!(outcome.woken, vec![0]);
    assert_eq!(set.get(0).expect("OBJECTIVE1").state, EngineState::Awake);
    assert_eq!(set.mission_clock(), 3.0);
    assert_eq!(set.cues().len(), 1);
    assert_eq!(set.cues()[0].kind, CueKind::Wakeup);
    assert_eq!(set.cues()[0].group.as_deref(), Some("snd_NW1Start"));
    for index in 1..58 {
        let objective = set.get(index).expect("objective");
        if objective.declaration.begin_dormant.is_some() {
            assert_eq!(
                objective.state,
                EngineState::Dormant,
                "OBJECTIVE{} keeps the -1 sentinel",
                index + 1
            );
        }
    }
}
