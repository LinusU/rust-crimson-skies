//! Mode rules the host must have resolved before a match starts (F56-A).
//!
//! Spec: `specs/F56-original-multiplayer-scenarios-and-mode-rules.md`, stage
//! `### F56-A` ("Each mode defines spawn/respawn, lives, time/score limits,
//! teams, friendly fire, victory/draw conditions and disconnect policy.
//! Unknown values remain blocked"). Shared contract:
//! `docs/contracts/UI-NETWORK.md`.
//!
//! A [`RuleDraft`] holds one slot per [`RuleField`]; a slot is `None` while
//! its value is unknown. [`RuleDraft::resolve`] refuses a draft with any empty
//! slot and names every one ([`RulesBlocked`]), so an unknown rule can neither
//! default to something convenient nor reach a launch. A resolved
//! [`MatchRules`] then checks the lobby it is about to start against the
//! ranges it carries ([`MatchRules::validate_start`]): human count, team
//! count, custom planes and per-plane component limit.
//!
//! The vocabulary (what a respawn policy or a disconnect policy *can be*) is
//! engine design. Which value the original game uses for each mode is unknown
//! (`docs/findings/2026-10-02-f56-a-multiplayer-catalog.md`); nothing here
//! supplies one.

use std::fmt;

use crate::bounds::MAX_SESSION_PEERS;
use crate::lobby::{LateJoin, MAX_TEAMS, TeamMode};

/// One rule a mode must define.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RuleField {
    /// Free-for-all or teams.
    Teams,
    /// Whether a pilot may join after launch.
    LateJoin,
    /// The time limit, or that there is none.
    TimeLimit,
    /// The score limit, or that there is none.
    ScoreLimit,
    /// Lives per pilot, or unlimited.
    Lives,
    /// What happens after a pilot is shot down.
    Respawn,
    /// Whether hits on teammates damage them.
    FriendlyFire,
    /// What happens to a participant who disconnects mid-match.
    Disconnect,
    /// The smallest and largest human count the mode supports.
    Humans,
    /// How the scenario scales with the human count.
    HumanScaling,
    /// Whether custom planes may be brought in.
    CustomPlanes,
    /// The most components one plane may carry.
    ComponentLimit,
}

impl RuleField {
    /// Every field, in the canonical order [`RulesBlocked`] reports.
    pub const ALL: [RuleField; 12] = [
        Self::Teams,
        Self::LateJoin,
        Self::TimeLimit,
        Self::ScoreLimit,
        Self::Lives,
        Self::Respawn,
        Self::FriendlyFire,
        Self::Disconnect,
        Self::Humans,
        Self::HumanScaling,
        Self::CustomPlanes,
        Self::ComponentLimit,
    ];

    /// The stable label used in reports and findings.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Teams => "teams",
            Self::LateJoin => "late_join",
            Self::TimeLimit => "time_limit",
            Self::ScoreLimit => "score_limit",
            Self::Lives => "lives",
            Self::Respawn => "respawn",
            Self::FriendlyFire => "friendly_fire",
            Self::Disconnect => "disconnect",
            Self::Humans => "humans",
            Self::HumanScaling => "human_scaling",
            Self::CustomPlanes => "custom_planes",
            Self::ComponentLimit => "component_limit",
        }
    }
}

/// A limit that is either absent on purpose or set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Limit {
    /// The mode has no such limit.
    None,
    /// The limit's value (ticks for time, points for score); never zero.
    At(u32),
}

/// Lives per pilot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lives {
    /// A pilot is never out.
    Unlimited,
    /// A pilot is out after this many losses; never zero.
    Limited(u8),
}

/// What follows a shoot-down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Respawn {
    /// Back in at once.
    Immediate,
    /// Back in after this many ticks; never zero.
    AfterTicks(u32),
    /// Out for the rest of the match.
    Never,
}

/// What happens to a participant who disconnects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisconnectPolicy {
    /// Removed; their score stays on the board.
    KeepScore,
    /// Removed; their score is struck from the board.
    StrikeScore,
    /// The match ends.
    EndMatch,
}

/// The supported human counts, both bounds included.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HumanRange {
    /// The fewest humans a match may start with.
    pub min: u8,
    /// The most humans it admits.
    pub max: u8,
}

/// How the scenario scales with the number of humans.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HumanScaling {
    /// The scenario does not scale.
    None,
    /// One factor in thousandths for every human count, `min` first.
    PerHuman(Vec<u16>),
}

/// Whether custom planes may be brought in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CustomPlanes {
    /// Only the stock planes.
    Forbidden,
    /// Custom blueprints are admitted (still budget-validated by F44).
    Allowed,
}

/// The rules one mode needs, each slot empty while unknown.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuleDraft {
    /// See [`RuleField::Teams`].
    pub teams: Option<TeamMode>,
    /// See [`RuleField::LateJoin`].
    pub late_join: Option<LateJoin>,
    /// See [`RuleField::TimeLimit`].
    pub time_limit: Option<Limit>,
    /// See [`RuleField::ScoreLimit`].
    pub score_limit: Option<Limit>,
    /// See [`RuleField::Lives`].
    pub lives: Option<Lives>,
    /// See [`RuleField::Respawn`].
    pub respawn: Option<Respawn>,
    /// See [`RuleField::FriendlyFire`].
    pub friendly_fire: Option<bool>,
    /// See [`RuleField::Disconnect`].
    pub disconnect: Option<DisconnectPolicy>,
    /// See [`RuleField::Humans`].
    pub humans: Option<HumanRange>,
    /// See [`RuleField::HumanScaling`].
    pub human_scaling: Option<HumanScaling>,
    /// See [`RuleField::CustomPlanes`].
    pub custom_planes: Option<CustomPlanes>,
    /// See [`RuleField::ComponentLimit`]; `Limit::None` means unrestricted.
    pub component_limit: Option<Limit>,
}

/// A draft with unknown fields: the mode is blocked, not defaulted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RulesBlocked {
    /// Every unknown field, in [`RuleField::ALL`] order.
    pub missing: Vec<RuleField>,
}

impl fmt::Display for RulesBlocked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "mode rules are unknown for:")?;
        for field in &self.missing {
            write!(f, " {}", field.label())?;
        }
        Ok(())
    }
}

impl std::error::Error for RulesBlocked {}

/// Why a fully specified rule set is itself invalid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RulesError {
    /// A zero limit, life count, respawn delay or component limit.
    ZeroValue(RuleField),
    /// Neither a time limit nor a score limit: the match could never end.
    NoLimit,
    /// `min` is zero, above `max`, or `max` exceeds the session's peer bound.
    BadHumanRange,
    /// Teams outside `2..=`[`MAX_TEAMS`], or more teams than the minimum
    /// human count can fill.
    BadTeams,
    /// The scaling table does not hold exactly one factor per human count.
    BadScaling,
}

impl fmt::Display for RulesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroValue(field) => write!(f, "{} must not be zero", field.label()),
            Self::NoLimit => write!(f, "a mode needs a time limit or a score limit"),
            Self::BadHumanRange => write!(f, "the human range is empty or exceeds the session"),
            Self::BadTeams => write!(f, "the team count does not fit the human range"),
            Self::BadScaling => write!(f, "the scaling table must cover every human count"),
        }
    }
}

impl std::error::Error for RulesError {}

/// Why [`RuleDraft::resolve`] failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolveError {
    /// Fields are unknown.
    Blocked(RulesBlocked),
    /// All fields are known but contradict each other or a bound.
    Invalid(RulesError),
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Blocked(blocked) => blocked.fmt(f),
            Self::Invalid(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for ResolveError {}

/// A fully known, internally consistent rule set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchRules {
    teams: TeamMode,
    late_join: LateJoin,
    time_limit: Limit,
    score_limit: Limit,
    lives: Lives,
    respawn: Respawn,
    friendly_fire: bool,
    disconnect: DisconnectPolicy,
    humans: HumanRange,
    human_scaling: HumanScaling,
    custom_planes: CustomPlanes,
    component_limit: Limit,
}

impl RuleDraft {
    /// The fields still unknown, in [`RuleField::ALL`] order.
    pub fn missing(&self) -> Vec<RuleField> {
        let known = [
            self.teams.is_some(),
            self.late_join.is_some(),
            self.time_limit.is_some(),
            self.score_limit.is_some(),
            self.lives.is_some(),
            self.respawn.is_some(),
            self.friendly_fire.is_some(),
            self.disconnect.is_some(),
            self.humans.is_some(),
            self.human_scaling.is_some(),
            self.custom_planes.is_some(),
            self.component_limit.is_some(),
        ];
        RuleField::ALL
            .into_iter()
            .zip(known)
            .filter_map(|(field, known)| (!known).then_some(field))
            .collect()
    }

    /// Resolves the draft.
    ///
    /// # Errors
    ///
    /// [`ResolveError::Blocked`] naming every unknown field, otherwise
    /// [`ResolveError::Invalid`] for a contradiction.
    pub fn resolve(&self) -> Result<MatchRules, ResolveError> {
        let missing = self.missing();
        if !missing.is_empty() {
            return Err(ResolveError::Blocked(RulesBlocked { missing }));
        }
        let (
            Some(teams),
            Some(late_join),
            Some(time_limit),
            Some(score_limit),
            Some(lives),
            Some(respawn),
            Some(friendly_fire),
            Some(disconnect),
            Some(humans),
            Some(human_scaling),
            Some(custom_planes),
            Some(component_limit),
        ) = (
            self.teams,
            self.late_join,
            self.time_limit,
            self.score_limit,
            self.lives,
            self.respawn,
            self.friendly_fire,
            self.disconnect,
            self.humans,
            self.human_scaling.clone(),
            self.custom_planes,
            self.component_limit,
        )
        else {
            unreachable!("missing() reported every field known");
        };
        let rules = MatchRules {
            teams,
            late_join,
            time_limit,
            score_limit,
            lives,
            respawn,
            friendly_fire,
            disconnect,
            humans,
            human_scaling,
            custom_planes,
            component_limit,
        };
        rules.check().map_err(ResolveError::Invalid)?;
        Ok(rules)
    }
}

impl MatchRules {
    fn check(&self) -> Result<(), RulesError> {
        for (field, limit) in [
            (RuleField::TimeLimit, self.time_limit),
            (RuleField::ScoreLimit, self.score_limit),
            (RuleField::ComponentLimit, self.component_limit),
        ] {
            if limit == Limit::At(0) {
                return Err(RulesError::ZeroValue(field));
            }
        }
        if self.lives == Lives::Limited(0) {
            return Err(RulesError::ZeroValue(RuleField::Lives));
        }
        if self.respawn == Respawn::AfterTicks(0) {
            return Err(RulesError::ZeroValue(RuleField::Respawn));
        }
        if self.time_limit == Limit::None && self.score_limit == Limit::None {
            return Err(RulesError::NoLimit);
        }
        let HumanRange { min, max } = self.humans;
        if min == 0 || min > max || usize::from(max) > MAX_SESSION_PEERS {
            return Err(RulesError::BadHumanRange);
        }
        if let TeamMode::Teams { teams } = self.teams
            && (!(2..=MAX_TEAMS).contains(&teams) || teams > min)
        {
            return Err(RulesError::BadTeams);
        }
        if let HumanScaling::PerHuman(factors) = &self.human_scaling
            && (factors.len() != usize::from(max - min) + 1 || factors.contains(&0))
        {
            return Err(RulesError::BadScaling);
        }
        Ok(())
    }

    /// The team grouping.
    pub fn teams(&self) -> TeamMode {
        self.teams
    }

    /// The respawn policy.
    pub fn respawn(&self) -> Respawn {
        self.respawn
    }

    /// The disconnect policy.
    pub fn disconnect(&self) -> DisconnectPolicy {
        self.disconnect
    }

    /// The scaling factor in thousandths for `humans`, or `None` when the
    /// scenario does not scale or the count is outside the range.
    pub fn scaling_permille(&self, humans: u8) -> Option<u16> {
        let HumanScaling::PerHuman(factors) = &self.human_scaling else {
            return None;
        };
        let index = humans.checked_sub(self.humans.min)?;
        factors.get(usize::from(index)).copied()
    }

    /// Checks a lobby about to launch against these rules.
    ///
    /// # Errors
    ///
    /// The first [`StartError`] found; nothing launches on error.
    pub fn validate_start(&self, request: &StartRequest) -> Result<(), StartError> {
        let HumanRange { min, max } = self.humans;
        if request.humans < min {
            return Err(StartError::TooFewHumans {
                got: request.humans,
                min,
            });
        }
        if request.humans > max {
            return Err(StartError::TooManyHumans {
                got: request.humans,
                max,
            });
        }
        if request.custom_planes > 0 && self.custom_planes == CustomPlanes::Forbidden {
            return Err(StartError::CustomPlanesForbidden);
        }
        if let Limit::At(limit) = self.component_limit
            && u32::from(request.largest_loadout) > limit
        {
            return Err(StartError::ComponentLimitExceeded {
                got: request.largest_loadout,
                limit,
            });
        }
        Ok(())
    }
}

/// What the host knows about the lobby it is about to launch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartRequest {
    /// Humans in the lobby.
    pub humans: u8,
    /// Planes that are custom blueprints.
    pub custom_planes: u8,
    /// Components on the largest plane.
    pub largest_loadout: u16,
}

/// Why a launch was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartError {
    /// Fewer humans than the mode supports.
    TooFewHumans {
        /// The lobby's human count.
        got: u8,
        /// The mode's minimum.
        min: u8,
    },
    /// More humans than the mode supports.
    TooManyHumans {
        /// The lobby's human count.
        got: u8,
        /// The mode's maximum.
        max: u8,
    },
    /// A custom plane was brought to a stock-only mode.
    CustomPlanesForbidden,
    /// A plane carries more components than the mode allows.
    ComponentLimitExceeded {
        /// Components on the largest plane.
        got: u16,
        /// The mode's limit.
        limit: u32,
    },
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooFewHumans { got, min } => write!(f, "{got} humans, the mode needs {min}"),
            Self::TooManyHumans { got, max } => write!(f, "{got} humans, the mode admits {max}"),
            Self::CustomPlanesForbidden => write!(f, "the mode allows only stock planes"),
            Self::ComponentLimitExceeded { got, limit } => {
                write!(f, "a plane carries {got} components, the limit is {limit}")
            }
        }
    }
}

impl std::error::Error for StartError {}
