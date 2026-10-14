//! Acceptance scenario task #476 (rules half): the per-mode rules the owner
//! measured by static analysis of the original executable replace their
//! unknowns with the measured value or host option, the unmeasured fields stay
//! explicit unknowns, and the evidence names the analyzed image. Every test
//! drives the production `cs_net::rules::measured` module; the evidence is the
//! owner note of 2026-10-05 on task #476 (static code evidence, never
//! `verified_original`).

use cs_net::lobby::{LateJoin, TeamMode};
use cs_net::rules::measured::{
    DEFAULT_SCORE_LIMIT, DEFAULT_TIME_MINUTES, ENVIRONMENT_WORLDS, EVIDENCE_IMAGE_SHA256,
    EndTrigger, LIVES_UNLIMITED, LOADER_DEFAULTS, LOADING_PLAYER_DROP_SECONDS, MissionError,
    MissionOptions, MissionType, POLICY_ADDRESSES, POLICY_TEAMS_HOST_OPTION, POLICY_TEAMS_TWO,
    RESIDUAL_UNKNOWN, SCORE_ENEMY_FLAG, SCORE_GAS_KILL, SCORE_KILL, SCORE_MY_GAS_KILL,
    SCORE_POINTS_MAX, SCORE_RETURN_FLAG, SCORE_SUICIDE, SCORE_TURRET_KILL, SCORE_ZEP,
    SCORE_ZEP_KILL, SCORES_SCREEN_DELAY_SECONDS, ScoreApply, ScoreEvent, TEAMS_DOGFIGHT_MAX,
    TIME_MINUTES_MAX, VictoryBy, draft, end_triggers, scoring,
};
use cs_net::rules::{DisconnectPolicy, Limit, Lives, ResolveError, Respawn, RuleField};

fn options(mission_type: MissionType, victory: VictoryBy) -> MissionOptions {
    MissionOptions {
        environment: 0,
        mission_type,
        victory,
        limited_lives: None,
        auto_respawn: true,
        allow_custom_planes: false,
        outlaw_components: false,
    }
}

const TICK_HZ: u32 = 64;

fn teams_for(mission_type: MissionType) -> TeamMode {
    match mission_type.fixed_teams() {
        Some(teams) => TeamMode::Teams { teams },
        None => TeamMode::FreeForAll,
    }
}

#[test]
fn accept_f476_measured_draft_fills_every_measured_field_with_the_measured_value() {
    for mission_type in MissionType::ALL {
        let teams = teams_for(mission_type);
        let draft = draft(
            &options(mission_type, VictoryBy::TimeMinutes(DEFAULT_TIME_MINUTES)),
            teams,
            TICK_HZ,
        )
        .expect("the measured options draft");
        assert_eq!(draft.teams, Some(teams));
        assert_eq!(
            draft.late_join,
            Some(LateJoin::Closed),
            "a session in progress refuses joins (0x412ed2)"
        );
        assert_eq!(
            draft.time_limit,
            Some(Limit::At(u32::from(DEFAULT_TIME_MINUTES) * 60 * TICK_HZ)),
            "time victory counts minutes x 60 s (0x4972c0)"
        );
        assert_eq!(draft.score_limit, Some(Limit::None));
        assert_eq!(
            draft.lives,
            Some(Lives::Limited(LIVES_UNLIMITED)),
            "unlimited is the stored count 10000 (0x4136e0)"
        );
        assert_eq!(draft.respawn, Some(Respawn::Immediate));
        assert_eq!(
            draft.friendly_fire,
            Some(true),
            "a same-side kill is scored as a penalty, so team damage occurs (0x46ecd0)"
        );
        assert_eq!(
            draft.disconnect,
            Some(DisconnectPolicy::StrikeScore),
            "the leaver is removed from the scoreboard (0x46e1f0)"
        );
    }
}

#[test]
fn accept_f476_measured_host_options_pick_limits_lives_and_respawn() {
    let score = draft(
        &options(
            MissionType::Dogfight,
            VictoryBy::ScorePoints(DEFAULT_SCORE_LIMIT),
        ),
        TeamMode::FreeForAll,
        TICK_HZ,
    )
    .expect("drafts");
    assert_eq!(
        score.time_limit,
        Some(Limit::None),
        "in time mode the score check is disabled, and the other way round"
    );
    assert_eq!(score.score_limit, Some(Limit::At(DEFAULT_SCORE_LIMIT)));

    let limited = draft(
        &MissionOptions {
            limited_lives: Some(3),
            auto_respawn: false,
            ..options(MissionType::CaptureTheFlag, VictoryBy::ScorePoints(40))
        },
        TeamMode::Teams { teams: 2 },
        TICK_HZ,
    )
    .expect("drafts");
    assert_eq!(limited.lives, Some(Lives::Limited(3)));
    assert_eq!(
        limited.respawn,
        Some(Respawn::OnCommand),
        "Auto Respawn off means the dead pilot respawns on the fire-weapons command (0x4709f0)"
    );
}

#[test]
fn accept_f476_measured_ui_defaults_and_ranges_are_pinned() {
    assert_eq!(DEFAULT_TIME_MINUTES, 10, "langui 10106 prints \"10\"");
    assert_eq!(DEFAULT_SCORE_LIMIT, 40, "the score row prints \"40\"");
    assert_eq!(TIME_MINUTES_MAX, 99, "the time field accepts 2 digits");
    assert_eq!(SCORE_POINTS_MAX, 999, "the score field accepts 3 digits");
    assert_eq!(
        TEAMS_DOGFIGHT_MAX, 16,
        "the Dogfight restriction offers up to 16 teams"
    );
    assert_eq!(LIVES_UNLIMITED, 10000);
    assert_eq!(
        ENVIRONMENT_WORLDS,
        ["c1c", "c3", "c2", "c5", "c1", "c1b", "c4"],
        "the launch-time world per environment index (0x4136e0)"
    );
}

#[test]
fn accept_f476_measured_two_team_types_refuse_another_grouping() {
    for mission_type in [MissionType::CaptureTheFlag, MissionType::Zeppelin] {
        let error = draft(
            &options(mission_type, VictoryBy::ScorePoints(40)),
            TeamMode::FreeForAll,
            TICK_HZ,
        )
        .expect_err("CTF and Zeppelin are exactly two teams");
        assert_eq!(
            error,
            MissionError::TeamRestriction {
                mission_type,
                teams: TeamMode::FreeForAll,
            }
        );
        let two_team = draft(
            &options(mission_type, VictoryBy::ScorePoints(40)),
            TeamMode::Teams { teams: 2 },
            TICK_HZ,
        )
        .expect("two teams are what the type fixes");
        assert_eq!(two_team.teams, Some(TeamMode::Teams { teams: 2 }));
        assert_eq!(
            mission_type.policies()[0],
            ("teams", POLICY_TEAMS_TWO),
            "the measured team policy of a two-team type"
        );
    }
    assert_eq!(
        MissionType::Dogfight.policies()[0],
        ("teams", POLICY_TEAMS_HOST_OPTION),
        "Dogfight takes its grouping from the host's restriction"
    );
    // Dogfight takes the grouping from the host's restriction.
    assert!(
        draft(
            &options(MissionType::Dogfight, VictoryBy::ScorePoints(40)),
            TeamMode::FreeForAll,
            TICK_HZ
        )
        .is_ok()
    );
    assert!(
        draft(
            &options(MissionType::Dogfight, VictoryBy::ScorePoints(40)),
            TeamMode::Teams { teams: 4 },
            TICK_HZ
        )
        .is_ok()
    );
}

#[test]
fn accept_f476_measured_launch_codes_and_team_rules_match_the_note() {
    assert_eq!(MissionType::CaptureTheFlag.lobby_value(), 0);
    assert_eq!(MissionType::Dogfight.lobby_value(), 1);
    assert_eq!(MissionType::Zeppelin.lobby_value(), 2);

    let ffa = TeamMode::FreeForAll;
    let teams3 = TeamMode::Teams { teams: 3 };
    assert_eq!(MissionType::CaptureTheFlag.launch_mode_code(ffa), 3);
    assert_eq!(MissionType::Dogfight.launch_mode_code(ffa), 1);
    assert_eq!(MissionType::Dogfight.launch_mode_code(teams3), 2);
    assert_eq!(MissionType::Zeppelin.launch_mode_code(ffa), 4);

    assert_eq!(MissionType::Dogfight.mp_mission_index(), 1);
    assert_eq!(MissionType::CaptureTheFlag.mp_mission_index(), 2);
    assert_eq!(MissionType::Zeppelin.mp_mission_index(), 3);

    assert_eq!(MissionType::Dogfight.fixed_teams(), None);
    assert_eq!(MissionType::CaptureTheFlag.fixed_teams(), Some(2));
    assert_eq!(MissionType::Zeppelin.fixed_teams(), Some(2));
}

#[test]
fn accept_f476_measured_bad_options_are_refused() {
    let bad_environment = MissionOptions {
        environment: 7,
        ..options(MissionType::Dogfight, VictoryBy::ScorePoints(40))
    };
    assert_eq!(
        draft(&bad_environment, TeamMode::FreeForAll, TICK_HZ),
        Err(MissionError::BadEnvironment(7))
    );
    let overflow = MissionOptions {
        victory: VictoryBy::TimeMinutes(TIME_MINUTES_MAX),
        ..options(MissionType::Dogfight, VictoryBy::TimeMinutes(1))
    };
    assert_eq!(
        draft(&overflow, TeamMode::FreeForAll, u32::MAX),
        Err(MissionError::TimeOverflow)
    );
}

#[test]
fn accept_f476_measured_draft_leaves_exactly_the_unmeasured_fields_unknown() {
    for mission_type in MissionType::ALL {
        let draft = draft(
            &options(mission_type, VictoryBy::TimeMinutes(DEFAULT_TIME_MINUTES)),
            teams_for(mission_type),
            TICK_HZ,
        )
        .expect("drafts");
        assert_eq!(
            draft.missing(),
            RESIDUAL_UNKNOWN.to_vec(),
            "the measured values fill their fields and nothing else ({mission_type:?})"
        );
    }
    // The measured policies and the residual unknowns partition the rule fields.
    let mut partition: Vec<&str> = MissionType::Dogfight
        .policies()
        .iter()
        .map(|(field, _)| *field)
        .chain(RESIDUAL_UNKNOWN.iter().map(|field| field.label()))
        .collect();
    partition.sort_unstable();
    let mut all: Vec<&str> = RuleField::ALL.iter().map(|field| field.label()).collect();
    all.sort_unstable();
    assert_eq!(partition, all);
}

#[test]
fn accept_f476_measured_a_measured_mode_still_refuses_to_launch() {
    let draft = draft(
        &options(MissionType::CaptureTheFlag, VictoryBy::ScorePoints(40)),
        TeamMode::Teams { teams: 2 },
        TICK_HZ,
    )
    .expect("drafts");
    match draft.resolve() {
        Err(ResolveError::Blocked(blocked)) => {
            assert_eq!(blocked.missing, RESIDUAL_UNKNOWN);
        }
        other => panic!("measured values must never resolve a mode: {other:?}"),
    }
}

#[test]
fn accept_f476_measured_scoring_events_pin_the_player_zrd_constants() {
    // The values `player.zrd` states (task #476's earlier finding) and the
    // code defaults for keys it does not.
    assert_eq!(SCORE_KILL, 2);
    assert_eq!(SCORE_SUICIDE, -2);
    assert_eq!(SCORE_RETURN_FLAG, 8);
    assert_eq!(SCORE_ENEMY_FLAG, 10);
    assert_eq!(SCORE_ZEP, 10);
    assert_eq!(SCORE_TURRET_KILL, 1);
    assert_eq!(SCORE_GAS_KILL, 10);
    assert_eq!(SCORE_MY_GAS_KILL, -10);
    assert_eq!(SCORE_ZEP_KILL, 1);
    assert_eq!(
        (LOADER_DEFAULTS.kill, LOADER_DEFAULTS.suicide),
        (1, -1),
        "the loader's missing-key fallbacks differ from the code defaults"
    );
    assert_eq!(
        (
            LOADER_DEFAULTS.zeppelin,
            LOADER_DEFAULTS.return_flag,
            LOADER_DEFAULTS.enemy_flag
        ),
        (100, 1, 5)
    );

    let labels = |mission_type: MissionType| -> Vec<(&str, i32, ScoreApply)> {
        scoring(mission_type)
            .iter()
            .map(
                |ScoreEvent {
                     label,
                     points,
                     apply,
                 }| (*label, *points, *apply),
            )
            .collect()
    };
    let add = ScoreApply::Add;
    assert_eq!(
        labels(MissionType::Dogfight),
        vec![
            ("enemy_kill", 2, add),
            ("same_side_kill", -2, add),
            ("turret_kill", 1, add),
        ],
        "both deathmatches print 2 / -2 and nothing else"
    );
    assert_eq!(
        labels(MissionType::CaptureTheFlag),
        vec![
            ("enemy_kill", 2, add),
            ("same_side_kill", -2, add),
            ("turret_kill", 1, add),
            ("flag_return", 8, add),
            ("enemy_flag_capture", 10, add),
        ],
        "the CTF briefing prints 10 / 8 / 2 / -2"
    );
    assert_eq!(
        labels(MissionType::Zeppelin),
        vec![
            ("enemy_kill", 2, add),
            ("same_side_kill", -2, add),
            ("turret_kill", 1, add),
            ("zeppelin_destroyed", 10, add),
            ("gas_kill", 10, add),
            ("own_gas_kill", -10, add),
            ("killed_by_zeppelin", 1, ScoreApply::Set),
        ],
        "the ZvZ briefing prints 10 / 2 / -2, and event 9 SETS the bonus (the MOV quirk)"
    );
}

#[test]
fn accept_f476_measured_end_triggers_and_match_timing() {
    let triggers = |mission_type: MissionType| end_triggers(mission_type).to_vec();
    let common = vec![
        EndTrigger::TimeExpiry,
        EndTrigger::ScoreLimit,
        EndTrigger::NoEnemiesLeft,
    ];
    assert_eq!(triggers(MissionType::Dogfight), common);
    assert_eq!(triggers(MissionType::CaptureTheFlag), common);
    assert_eq!(
        triggers(MissionType::Zeppelin),
        vec![
            EndTrigger::TimeExpiry,
            EndTrigger::ScoreLimit,
            EndTrigger::ZeppelinDestroyed,
            EndTrigger::NoEnemiesLeft,
        ],
        "a destroyed zeppelin ends a ZvZ match (reason 3)"
    );
    assert_eq!(
        SCORES_SCREEN_DELAY_SECONDS, 5.0,
        "the scores screen waits 5 s (0x6036bc)"
    );
    assert_eq!(
        LOADING_PLAYER_DROP_SECONDS, 600,
        "players still loading are dropped after 600 s (0x60818c)"
    );
}

#[test]
fn accept_f476_measured_evidence_names_the_analyzed_image_and_addresses() {
    assert_eq!(
        EVIDENCE_IMAGE_SHA256.len(),
        64,
        "the full sha256 of crimson.decrypted.exe"
    );
    assert!(EVIDENCE_IMAGE_SHA256.chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(
        EVIDENCE_IMAGE_SHA256,
        "43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75"
    );

    let mut cited = Vec::new();
    for mission_type in MissionType::ALL {
        for (field, policy) in mission_type.policies() {
            let (_, addresses) = POLICY_ADDRESSES
                .iter()
                .find(|(label, _)| *label == policy)
                .unwrap_or_else(|| panic!("{mission_type:?} {field} cites no measurement"));
            assert!(
                addresses.contains("task #476 owner note 2026-10-05"),
                "{policy}: {addresses}"
            );
            cited.push(policy);
        }
    }
    for (policy, addresses) in POLICY_ADDRESSES {
        assert!(
            cited.contains(&policy),
            "{policy} is cited but no mode uses it: {addresses}"
        );
    }
}
