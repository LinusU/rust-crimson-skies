//! The per-mode multiplayer rules **measured** from the original by static
//! analysis (task #476, stage `### F56-A`).
//!
//! Evidence (static code evidence, `observed_tool`, **never**
//! `verified_original`): the owner note of 2026-10-05 on task #476 — static
//! analysis of the owner-supplied decrypted executable `crimson.decrypted.exe`
//! (Kuna v1.692), its lobby UI script `MULTIPLAYERLOBBY_MISSION.SCRIPT` and the
//! UI string ids — together with the `player.zrd` constants decoded from the
//! installation (`docs/findings/2026-10-03-f56-a-scoring-and-respawn-from-player-zrd.md`).
//! The analyzed image is [`EVIDENCE_IMAGE_SHA256`]; every policy names the
//! addresses it was read at in [`POLICY_ADDRESSES`]. Findings:
//! `docs/findings/2026-10-14-f476-measured-per-mode-rules.md`.
//!
//! What the original measures, and how this module encodes it:
//!
//! * The host **chooses** most rules on the Mission Options screen (the
//!   10-dword block at `0x642f8c`, broadcast as message `0x26`): environment,
//!   mission type, victory by **time** (minutes, UI default [`DEFAULT_TIME_MINUTES`])
//!   or **score** (points, UI default [`DEFAULT_SCORE_LIMIT`]), a team-count
//!   restriction, limited lives (else [`LIVES_UNLIMITED`]), auto respawn, custom
//!   planes and outlawed components. [`MissionOptions`] is that block and
//!   [`draft`] turns it into a [`RuleDraft`].
//! * The fields the original fixes for every mode are filled by [`draft`]:
//!   late join is refused (a session in progress rejects joins), friendly fire
//!   is on (a same-side kill is scored as a penalty), a leaver is struck from
//!   the scoreboard, and the limits/lives/respawn policies follow the host
//!   options. Each is cited in [`POLICY_ADDRESSES`].
//! * What static analysis did **not** measure stays unknown: [`draft`] leaves
//!   [`RESIDUAL_UNKNOWN`] empty, so a measured mode still refuses to resolve
//!   (`RuleDraft::resolve` → `RulesBlocked`) until those fields are measured
//!   too. In particular the original computes **no winner** at match end (no
//!   winner/draw computation was found; the result is the final scoreboard), so
//!   `RuleField::Victory` stays unknown here and `cs_sim`'s highest-score
//!   resolver remains a declared engine design choice.
//!
//! Nothing here runs the original executable and nothing here is a launch
//! default: a resolved `MatchRules` still has to come from a real lobby.

use super::{DisconnectPolicy, LateJoin, Limit, Lives, Respawn, RuleDraft, RuleField};
use crate::lobby::TeamMode;

/// The image the measured addresses were read from: the owner-supplied
/// decrypted `crimson.decrypted.exe`, kept outside the installation.
pub const EVIDENCE_IMAGE_SHA256: &str =
    "43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75";

/// Where each filled policy was measured, keyed by policy label: the
/// owner-note addresses (`0x…`, string ids), against [`EVIDENCE_IMAGE_SHA256`].
///
/// [`cs_content`]'s mode catalog mirrors these citations for the mode rows
/// (dev-dependency test, `accept_f476_`), so a row and the rules crate cite
/// the same measurement.
pub const POLICY_ADDRESSES: [(&str, &str); 9] = [
    (
        "host_team_option",
        "task #476 owner note 2026-10-05: 0x4136e0 (Dogfight = mode 1 without \
         teams, 2 with), restriction langui 10518/10519, up to 16 teams",
    ),
    (
        "two_teams",
        "task #476 owner note 2026-10-05: 0x4136e0 (CTF = 3, Zeppelin = 4, team \
         game = mode != 1), langui 10519",
    ),
    (
        "closed",
        "task #476 owner note 2026-10-05: 0x412ed2 (session state 3 in progress \
         -> langui 7083 \"game in progress\")",
    ),
    (
        "host_time_option",
        "task #476 owner note 2026-10-05: 0x4972c0 -> 0x46c510 (minutes x 60 s), \
         end reason 1 at 0x496d60, langui 10106 default \"10\"",
    ),
    (
        "host_score_option",
        "task #476 owner note 2026-10-05: 0x499270 (player >= limit), 0x46ea40 \
         (team total = bonus + member scores), end reason 2, langui default \"40\"",
    ),
    (
        "host_lives_option_else_10000",
        "task #476 owner note 2026-10-05: 0x4136e0 (count else 10000), 0x498c41 \
         (death decrements), langui 209/210/211",
    ),
    (
        "host_auto_respawn_option_else_fire_guns",
        "task #476 owner note 2026-10-05: 0x470a10 (auto respawns at once), \
         0x4709f0 (command 19 rebound while dead), player.zrd respawn_rad 1200 / \
         respawn_el 100",
    ),
    (
        "on_by_scoring",
        "task #476 owner note 2026-10-05: 0x46ecd0 (a same-side kill scores \
         score_suicide to the killer, so team damage occurs); the damage routine \
         itself was not traced",
    ),
    (
        "strike_score",
        "task #476 owner note 2026-10-05: 0x4995a0 -> 0x46e1f0 (leaver removed \
         from the scoreboard), 0x46ea40 (team totals recomputed), 0x49ab50 \
         (carried flag reset)",
    ),
];

/// The policy label for a fixed (non-option) rule, by field label.
pub const POLICY_LATE_JOIN: &str = "closed";
/// See [`POLICY_LATE_JOIN`].
pub const POLICY_FRIENDLY_FIRE: &str = "on_by_scoring";
/// See [`POLICY_LATE_JOIN`].
pub const POLICY_DISCONNECT: &str = "strike_score";
/// The policy label of the team rule when the host's type is Dogfight.
pub const POLICY_TEAMS_HOST_OPTION: &str = "host_team_option";
/// The policy label of the team rule when the host's type is CTF or Zeppelin.
pub const POLICY_TEAMS_TWO: &str = "two_teams";
/// The policy label of the time-limit rule (host option).
pub const POLICY_TIME_LIMIT: &str = "host_time_option";
/// The policy label of the score-limit rule (host option).
pub const POLICY_SCORE_LIMIT: &str = "host_score_option";
/// The policy label of the lives rule (host option, else [`LIVES_UNLIMITED`]).
pub const POLICY_LIVES: &str = "host_lives_option_else_10000";
/// The policy label of the respawn rule (host option).
pub const POLICY_RESPAWN: &str = "host_auto_respawn_option_else_fire_guns";

/// The lobby's Mission Type option (`0x642f8c+0x04`): the list the host picks
/// from, `0` Capture the Flag, `1` Dogfight, `2` Zeppelin (descriptions langui
/// 10124/10123/10125). The two deathmatch names of the mode catalog (7011
/// without Teams, 7012 with Teams) are both this type, split by the host's
/// team-count restriction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MissionType {
    /// Lobby value 0; the Capture the Flag mode (catalog name id 7013).
    CaptureTheFlag,
    /// Lobby value 1; both Deathmatch names (catalog name ids 7011 and 7012).
    Dogfight,
    /// Lobby value 2; Zeppelin vs. Zeppelin (catalog name id 7014).
    Zeppelin,
}

impl MissionType {
    /// Every type, in lobby-value order.
    pub const ALL: [MissionType; 3] = [Self::CaptureTheFlag, Self::Dogfight, Self::Zeppelin];

    /// The value the lobby block stores (`0x642f8c+0x04`).
    pub const fn lobby_value(self) -> u8 {
        match self {
            Self::CaptureTheFlag => 0,
            Self::Dogfight => 1,
            Self::Zeppelin => 2,
        }
    }

    /// The launch-time mode code (`0x4136e0` → `0x71c194`): CTF 3, Dogfight 1
    /// without teams or 2 with teams, Zeppelin 4.
    pub const fn launch_mode_code(self, teams: TeamMode) -> u32 {
        match (self, teams) {
            (Self::CaptureTheFlag, _) => 3,
            (Self::Dogfight, TeamMode::FreeForAll) => 1,
            (Self::Dogfight, TeamMode::Teams { .. }) => 2,
            (Self::Zeppelin, _) => 4,
        }
    }

    /// The multiplayer mission index the launch stores (1 Deathmatch, 2 CTF,
    /// 3 Zeppelin vs. Zeppelin).
    pub const fn mp_mission_index(self) -> u8 {
        match self {
            Self::Dogfight => 1,
            Self::CaptureTheFlag => 2,
            Self::Zeppelin => 3,
        }
    }

    /// The team count the type fixes, when it fixes one: CTF and Zeppelin are
    /// exactly two (`langui 10519`), Dogfight takes the grouping from the
    /// host's team-count restriction (up to [`TEAMS_DOGFIGHT_MAX`]).
    pub const fn fixed_teams(self) -> Option<u8> {
        match self {
            Self::Dogfight => None,
            Self::CaptureTheFlag | Self::Zeppelin => Some(2),
        }
    }

    /// The `(field label, policy label)` pairs [`draft`] fills for this type,
    /// in `RuleField::ALL` order (teams first). The fields not listed are
    /// exactly [`RESIDUAL_UNKNOWN`].
    pub const fn policies(self) -> [(&'static str, &'static str); 8] {
        let teams = match self {
            Self::Dogfight => POLICY_TEAMS_HOST_OPTION,
            Self::CaptureTheFlag | Self::Zeppelin => POLICY_TEAMS_TWO,
        };
        [
            ("teams", teams),
            ("late_join", POLICY_LATE_JOIN),
            ("time_limit", POLICY_TIME_LIMIT),
            ("score_limit", POLICY_SCORE_LIMIT),
            ("lives", POLICY_LIVES),
            ("respawn", POLICY_RESPAWN),
            ("friendly_fire", POLICY_FRIENDLY_FIRE),
            ("disconnect", POLICY_DISCONNECT),
        ]
    }
}

/// The rule fields static analysis did not measure. [`draft`] leaves them
/// empty, so they keep blocking the mode (`AGENTS.md` rule 5): unknown means
/// unknown.
///
/// * `spawn` — `respawn_rad`/`respawn_el` place a pilot near a point but do
///   not say which of the scenario's points is chosen.
/// * `victory` — no winner/draw computation was found at match end; the result
///   is the final scoreboard (`cs_sim`'s highest-score resolver stays a
///   declared design choice).
/// * `humans` — the launch player-count thresholds ("There are not enough
///   players in the game.", langui 10520) were not recovered.
/// * `human_scaling` — no scaling mechanism was found; absence of a mechanism
///   is not a measured "none".
/// * `custom_planes` — the lobby flag exists (`+0x25`) but its launch/match
///   enforcement was not traced.
/// * `component_limit` — the outlawed-components list (uidata 5044) and its
///   enforcement were not traced.
pub const RESIDUAL_UNKNOWN: [RuleField; 6] = [
    RuleField::Spawn,
    RuleField::Victory,
    RuleField::Humans,
    RuleField::HumanScaling,
    RuleField::CustomPlanes,
    RuleField::ComponentLimit,
];

/// The Victory Conditions option (`0x642f8c+0x08`): `0` = time, `1` = score,
/// then the value at `+0x0c`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VictoryBy {
    /// The match ends when the countdown expires; value in minutes.
    TimeMinutes(u16),
    /// The match ends when a player (free-for-all) or team total reaches the
    /// value; value in points.
    ScorePoints(u32),
}

/// The Mission Options time default (langui 10106 prints `"10"`).
pub const DEFAULT_TIME_MINUTES: u16 = 10;

/// The Mission Options score default (the score row prints `"40"`).
pub const DEFAULT_SCORE_LIMIT: u32 = 40;

/// The most whole minutes the time field holds (the UI accepts 2 digits).
pub const TIME_MINUTES_MAX: u16 = 99;

/// The most points the score field holds (the UI accepts 3 digits).
pub const SCORE_POINTS_MAX: u32 = 999;

/// The life count the launch code stores when Limited Lives is off
/// (`0x4136e0`): effectively unlimited, but a real stored count, which is why
/// [`Lives::Limited`] carries it rather than a separate "unlimited" original
/// value.
pub const LIVES_UNLIMITED: u32 = 10000;

/// The most teams the Dogfight restriction offers (the UI range; the engine's
/// own `MAX_TEAMS` bound may be smaller and is unchanged engine design).
pub const TEAMS_DOGFIGHT_MAX: u8 = 16;

/// The launch-time world each Mission Environment index selects
/// (`0x4136e0` → `0x71c1a8`), index 0 first.
pub const ENVIRONMENT_WORLDS: [&str; 7] = ["c1c", "c3", "c2", "c5", "c1", "c1b", "c4"];

/// The Mission Options block (`0x642f8c`, broadcast as message `0x26`) in the
/// shape a rule draft is derived from.
///
/// The team-count restriction is **not** carried here: in this engine the
/// lobby's own team grouping (`TeamMode`, passed to [`draft`]) is that choice,
/// written once, and [`MissionType::fixed_teams`] is the measured restriction
/// on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MissionOptions {
    /// Mission Environment, `0..=`[`ENVIRONMENT_WORLDS`]`len - 1`; the map,
    /// not a rule.
    pub environment: u8,
    /// Mission Type.
    pub mission_type: MissionType,
    /// Victory Conditions and their value.
    pub victory: VictoryBy,
    /// Limited Lives flag and count; `None` launches with [`LIVES_UNLIMITED`].
    pub limited_lives: Option<u32>,
    /// Auto Respawn checkbox.
    pub auto_respawn: bool,
    /// Allow Custom Planes checkbox (`+0x25`). Recorded as the host's choice;
    /// enforcement is [`RESIDUAL_UNKNOWN`].
    pub allow_custom_planes: bool,
    /// Outlaw Components checkbox (`+0x26`). Enforcement is
    /// [`RESIDUAL_UNKNOWN`].
    pub outlaw_components: bool,
}

/// Why a measured option set cannot become a draft.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissionError {
    /// An environment index outside `0..=7`.
    BadEnvironment(u8),
    /// A type that fixes two teams was paired with another grouping (langui
    /// 10519: "Each player must be on one of two teams to play.").
    TeamRestriction {
        /// The type the host selected.
        mission_type: MissionType,
        /// The grouping it was paired with.
        teams: TeamMode,
    },
    /// The host chose time victory by minutes whose second count does not fit
    /// the tick conversion.
    TimeOverflow,
}

impl std::fmt::Display for MissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadEnvironment(index) => {
                write!(f, "environment {index} selects no world")
            }
            Self::TeamRestriction {
                mission_type,
                teams,
            } => write!(
                f,
                "{mission_type:?} is a two-team type but the lobby groups pilots as {teams:?}"
            ),
            Self::TimeOverflow => write!(f, "the time limit does not fit the tick conversion"),
        }
    }
}

impl std::error::Error for MissionError {}

/// The measured rule draft for one host option set and lobby grouping.
///
/// Fills every field [`MissionType::policies`] lists with the value or host
/// option the original answers it with, and leaves [`RESIDUAL_UNKNOWN`] empty,
/// so [`RuleDraft::resolve`] still names what is unmeasured. The measured
/// values never default a mode into a launch.
///
/// # Errors
///
/// [`MissionError`] for an out-of-range environment, a violated two-team
/// restriction or a time limit that does not fit the tick conversion.
pub fn draft(
    options: &MissionOptions,
    teams: TeamMode,
    tick_hz: u32,
) -> Result<RuleDraft, MissionError> {
    if usize::from(options.environment) >= ENVIRONMENT_WORLDS.len() {
        return Err(MissionError::BadEnvironment(options.environment));
    }
    if let Some(fixed) = options.mission_type.fixed_teams() {
        match teams {
            TeamMode::Teams { teams } if teams == fixed => {}
            _ => {
                return Err(MissionError::TeamRestriction {
                    mission_type: options.mission_type,
                    teams,
                });
            }
        }
    }
    let (time_limit, score_limit) = match options.victory {
        VictoryBy::TimeMinutes(minutes) => {
            let ticks = u64::from(minutes) * 60 * u64::from(tick_hz);
            (
                Limit::At(u32::try_from(ticks).map_err(|_| MissionError::TimeOverflow)?),
                Limit::None,
            )
        }
        VictoryBy::ScorePoints(points) => (Limit::None, Limit::At(points)),
    };
    Ok(RuleDraft {
        teams: Some(teams),
        late_join: Some(LateJoin::Closed),
        time_limit: Some(time_limit),
        score_limit: Some(score_limit),
        lives: Some(Lives::Limited(
            options.limited_lives.unwrap_or(LIVES_UNLIMITED),
        )),
        respawn: Some(if options.auto_respawn {
            Respawn::Immediate
        } else {
            Respawn::OnCommand
        }),
        friendly_fire: Some(true),
        disconnect: Some(DisconnectPolicy::StrikeScore),
        ..RuleDraft::default()
    })
}

// ------------------------------------------------------------- scoring ---

/// How a scoring event changes a side's bonus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScoreApply {
    /// The points are added to the current bonus.
    Add,
    /// The points **replace** the team bonus: the original's event 9 (killed
    /// by a zeppelin) is a `MOV`, not an `ADD` (`0x46e1b0`). Recorded as the
    /// original quirk it is; a consumer must not smooth it into an add.
    Set,
}

/// One measured scoring event: its label, its points and how they apply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScoreEvent {
    /// The stable event label.
    pub label: &'static str,
    /// The points, from `player.zrd` where the key exists, else the code
    /// default (`0x4735b0`).
    pub points: i32,
    /// Add or set.
    pub apply: ScoreApply,
}

/// `player.zrd` `score_kill`: an enemy kill, to the killer.
pub const SCORE_KILL: i32 = 2;
/// `player.zrd` `score_suicide` (`4294967294` as two's-complement `i32`): a
/// same-side kill, charged to the killer.
pub const SCORE_SUICIDE: i32 = -2;
/// `player.zrd` `score_return_flag`: a returned flag (CTF).
pub const SCORE_RETURN_FLAG: i32 = 8;
/// `player.zrd` `score_enemy_flag`: a captured enemy flag (CTF).
pub const SCORE_ENEMY_FLAG: i32 = 10;
/// `player.zrd` `score_zep`: a destroyed enemy zeppelin, to every other team
/// (Zeppelin vs. Zeppelin).
pub const SCORE_ZEP: i32 = 10;
/// Code default `score_turret_kill` (no `player.zrd` key).
pub const SCORE_TURRET_KILL: i32 = 1;
/// Code default `score_gas_kill` (no `player.zrd` key).
pub const SCORE_GAS_KILL: i32 = 10;
/// Code default `score_my_gas_kill` (no `player.zrd` key): your own gas kills.
pub const SCORE_MY_GAS_KILL: i32 = -10;
/// Code default `score_zep_kill` (no `player.zrd` key); applied by **setting**
/// the team bonus ([`ScoreApply::Set`]).
pub const SCORE_ZEP_KILL: i32 = 1;

/// The values the `player.zrd` loader falls back to if a key were absent
/// (`0x4735b0`). They differ from the code defaults of the events above; both
/// are measured, and a consumer cites which one it used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoaderDefaults {
    /// `score_kill`.
    pub kill: i32,
    /// `score_suicide`.
    pub suicide: i32,
    /// `score_zep`.
    pub zeppelin: i32,
    /// `score_return_flag`.
    pub return_flag: i32,
    /// `score_enemy_flag`.
    pub enemy_flag: i32,
}

/// The loader's missing-key fallbacks (see [`LoaderDefaults`]).
pub const LOADER_DEFAULTS: LoaderDefaults = LoaderDefaults {
    kill: 1,
    suicide: -1,
    zeppelin: 100,
    return_flag: 1,
    enemy_flag: 5,
};

/// The scoring events one type scores (`0x46ecd0`; per mode `0x46ed40` DM,
/// `0x46ed90` CTF, `0x46ee20` ZvZ; points added by `0x46e1b0`). A death whose
/// cause is not one of these events carries no score.
pub const fn scoring(mission_type: MissionType) -> &'static [ScoreEvent] {
    const ENEMY_KILL: ScoreEvent = ScoreEvent {
        label: "enemy_kill",
        points: SCORE_KILL,
        apply: ScoreApply::Add,
    };
    const SAME_SIDE_KILL: ScoreEvent = ScoreEvent {
        label: "same_side_kill",
        points: SCORE_SUICIDE,
        apply: ScoreApply::Add,
    };
    const TURRET_KILL: ScoreEvent = ScoreEvent {
        label: "turret_kill",
        points: SCORE_TURRET_KILL,
        apply: ScoreApply::Add,
    };
    const FLAG_RETURN: ScoreEvent = ScoreEvent {
        label: "flag_return",
        points: SCORE_RETURN_FLAG,
        apply: ScoreApply::Add,
    };
    const ENEMY_FLAG_CAPTURE: ScoreEvent = ScoreEvent {
        label: "enemy_flag_capture",
        points: SCORE_ENEMY_FLAG,
        apply: ScoreApply::Add,
    };
    const ZEPPELIN_DESTROYED: ScoreEvent = ScoreEvent {
        label: "zeppelin_destroyed",
        points: SCORE_ZEP,
        apply: ScoreApply::Add,
    };
    const GAS_KILL: ScoreEvent = ScoreEvent {
        label: "gas_kill",
        points: SCORE_GAS_KILL,
        apply: ScoreApply::Add,
    };
    const OWN_GAS_KILL: ScoreEvent = ScoreEvent {
        label: "own_gas_kill",
        points: SCORE_MY_GAS_KILL,
        apply: ScoreApply::Add,
    };
    const KILLED_BY_ZEPPELIN: ScoreEvent = ScoreEvent {
        label: "killed_by_zeppelin",
        points: SCORE_ZEP_KILL,
        apply: ScoreApply::Set,
    };
    match mission_type {
        MissionType::Dogfight => &[ENEMY_KILL, SAME_SIDE_KILL, TURRET_KILL],
        MissionType::CaptureTheFlag => &[
            ENEMY_KILL,
            SAME_SIDE_KILL,
            TURRET_KILL,
            FLAG_RETURN,
            ENEMY_FLAG_CAPTURE,
        ],
        MissionType::Zeppelin => &[
            ENEMY_KILL,
            SAME_SIDE_KILL,
            TURRET_KILL,
            ZEPPELIN_DESTROYED,
            GAS_KILL,
            OWN_GAS_KILL,
            KILLED_BY_ZEPPELIN,
        ],
    }
}

// ------------------------------------------------------- end of match ---

/// One measured reason a match ends (`0x496d60` reason 1, `0x499270` reason
/// 2, `0x49afc7` reason 3, `0x4999f0` reason 4; message ids 7005/7006/7066/7067).
///
/// These end the *match*; which side then wins is not computed by the original
/// (see [`RESIDUAL_UNKNOWN`], `RuleField::Victory`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EndTrigger {
    /// The countdown expired (reason 1 "Time").
    TimeExpiry,
    /// A player score (free-for-all) or team total reached the score limit
    /// (reason 2 "Score"); disabled while the match runs on time.
    ScoreLimit,
    /// The zeppelin was destroyed (reason 3); Zeppelin vs. Zeppelin only.
    ZeppelinDestroyed,
    /// Fewer than two sides still hold a player with lives left, after any
    /// death and after any departure (reason 4 "No Enemies Left").
    NoEnemiesLeft,
}

/// The measured end triggers of one type, in reason order.
pub const fn end_triggers(mission_type: MissionType) -> &'static [EndTrigger] {
    const COMMON: [EndTrigger; 3] = [
        EndTrigger::TimeExpiry,
        EndTrigger::ScoreLimit,
        EndTrigger::NoEnemiesLeft,
    ];
    match mission_type {
        MissionType::Dogfight | MissionType::CaptureTheFlag => &COMMON,
        MissionType::Zeppelin => &[
            EndTrigger::TimeExpiry,
            EndTrigger::ScoreLimit,
            EndTrigger::ZeppelinDestroyed,
            EndTrigger::NoEnemiesLeft,
        ],
    }
}

/// The measured delay between the end message (broadcast as `0x17`) and the
/// Game Scores screen (`0x6036bc`, state 5).
pub const SCORES_SCREEN_DELAY_SECONDS: f32 = 5.0;

/// The measured grace period for players still loading at launch before the
/// host drops them (`0x60818c`, `0x4972c0`; "Waiting for %1", langui 7062).
pub const LOADING_PLAYER_DROP_SECONDS: u32 = 600;
