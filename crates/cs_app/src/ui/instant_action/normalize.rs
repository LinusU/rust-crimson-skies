//! Scenario normalization and isolated outcomes (F49-B).
//!
//! Spec: `specs/F49-instant-action-presets-and-custom-scenarios.md`, stage
//! `### F49-B`. Task test prefix: `accept_f49_b_`.
//!
//! * [`diff_scenarios`] compares two [`LoweredScenario`]s and reports exactly
//!   what differs, per actor and per scenario dimension. It is how AC02
//!   ("change one custom roster slot and confirm only the intended actor
//!   changes") is observed on the production lowering path, and how a screen
//!   can show what a customization changed (F49 non-negotiable 5).
//! * [`ScenarioSnapshot`] and [`evaluate_outcome`] decide whether a lowered
//!   scenario has ended and with what [`ScenarioOutcome`]. The outcome carries
//!   the scenario's own subject, seed and tick and **nothing from a campaign**:
//!   there is no cash, node, unlock or run field on it, so settling an Instant
//!   Action cannot move campaign progression (non-negotiable 3) because it has
//!   no way to name it.
//!
//! # Designed, not measured
//!
//! How the original decides an Instant Action has ended, and what it records,
//! is unmeasured. The end conditions evaluated here are the designed
//! vocabulary of `cs_content::instant_action::VictoryCondition`. See
//! `docs/findings/2026-10-08-f49-b-scenario-normalization-and-outcomes.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cs_content::instant_action::{
    RosterSlot, ScenarioSeed, ScenarioSide, TieOutcome, VictoryCondition,
};
use cs_types::content::ContentId;

use super::{LoweredScenario, LoweredScenarioActor};

/// One field of an actor that can differ between two scenarios.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ActorField {
    /// The faction it flies for.
    Faction,
    /// The geometry it is built from.
    Geometry,
    /// The loadout it carries.
    Loadout,
    /// The pilot that flies it.
    Pilot,
    /// Its survivability policy.
    Survivability,
}

/// One difference between two lowered scenarios.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScenarioChange {
    /// The `ia_scenario` identity differs.
    Subject,
    /// The number of human seats differs.
    Players,
    /// The world differs.
    World,
    /// The environment differs.
    Environment,
    /// The victory condition, respawn budget, deadline or tie outcome differs.
    Rules,
    /// The difficulty tier differs.
    Difficulty,
    /// The seed differs.
    Seed,
    /// An actor present in both scenarios differs in these fields.
    ActorChanged {
        /// Its side.
        side: ScenarioSide,
        /// Its slot.
        slot: RosterSlot,
        /// The fields that differ, in field order.
        fields: Vec<ActorField>,
    },
    /// An actor only the second scenario has.
    ActorAdded {
        /// Its side.
        side: ScenarioSide,
        /// Its slot.
        slot: RosterSlot,
    },
    /// An actor only the first scenario has.
    ActorRemoved {
        /// Its side.
        side: ScenarioSide,
        /// Its slot.
        slot: RosterSlot,
    },
}

fn differing_fields(a: &LoweredScenarioActor, b: &LoweredScenarioActor) -> Vec<ActorField> {
    let mut fields = Vec::new();
    if a.faction != b.faction {
        fields.push(ActorField::Faction);
    }
    if a.geometry != b.geometry {
        fields.push(ActorField::Geometry);
    }
    if a.loadout != b.loadout {
        fields.push(ActorField::Loadout);
    }
    if a.pilot != b.pilot {
        fields.push(ActorField::Pilot);
    }
    if a.survivability != b.survivability {
        fields.push(ActorField::Survivability);
    }
    fields
}

/// Reports every difference between two lowered scenarios.
///
/// Actors are matched by `(side, slot)`, never by position, so inserting or
/// removing an actor cannot make its neighbours look changed. Scenario-level
/// differences come first, then actors in `(side, slot)` order. Two equal
/// scenarios report nothing.
#[must_use]
pub fn diff_scenarios(before: &LoweredScenario, after: &LoweredScenario) -> Vec<ScenarioChange> {
    let mut changes = Vec::new();
    if before.subject() != after.subject() {
        changes.push(ScenarioChange::Subject);
    }
    if before.players() != after.players() {
        changes.push(ScenarioChange::Players);
    }
    if before.world().world != after.world().world {
        changes.push(ScenarioChange::World);
    }
    if before.world().environment != after.world().environment {
        changes.push(ScenarioChange::Environment);
    }
    if before.condition() != after.condition()
        || before.respawns() != after.respawns()
        || before.deadline_ticks() != after.deadline_ticks()
        || before.tie_outcome() != after.tie_outcome()
    {
        changes.push(ScenarioChange::Rules);
    }
    if before.difficulty() != after.difficulty() {
        changes.push(ScenarioChange::Difficulty);
    }
    if before.seed() != after.seed() {
        changes.push(ScenarioChange::Seed);
    }

    let keys: BTreeSet<(ScenarioSide, RosterSlot)> = before
        .actors()
        .iter()
        .chain(after.actors())
        .map(|actor| (actor.side, actor.slot))
        .collect();
    for (side, slot) in keys {
        let find = |scenario: &'_ LoweredScenario| {
            scenario
                .actors()
                .iter()
                .find(|actor| actor.side == side && actor.slot == slot)
                .cloned()
        };
        match (find(before), find(after)) {
            (Some(a), Some(b)) => {
                let fields = differing_fields(&a, &b);
                if !fields.is_empty() {
                    changes.push(ScenarioChange::ActorChanged { side, slot, fields });
                }
            }
            (None, Some(_)) => changes.push(ScenarioChange::ActorAdded { side, slot }),
            (Some(_), None) => changes.push(ScenarioChange::ActorRemoved { side, slot }),
            (None, None) => {}
        }
    }
    changes
}

/// Which actors are still flying at a tick of a running scenario.
///
/// The snapshot names actors by `(side, slot)`, the identity the lowering
/// gives them, so it cannot refer to an actor by a spawn-order index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioSnapshot {
    tick: u64,
    alive: BTreeSet<(ScenarioSide, RosterSlot)>,
    respawns_used: BTreeMap<ScenarioSide, u32>,
}

impl ScenarioSnapshot {
    /// A snapshot at `tick` with these actors alive and no replacements used.
    #[must_use]
    pub fn new(tick: u64, alive: impl IntoIterator<Item = (ScenarioSide, RosterSlot)>) -> Self {
        Self {
            tick,
            alive: alive.into_iter().collect(),
            respawns_used: BTreeMap::new(),
        }
    }

    /// Records that `side` has spent `used` of **its own** replacement budget.
    ///
    /// The count is per side because the declared budget is
    /// [`RespawnBudget::PerSide`](cs_content::instant_action::RespawnBudget::PerSide)
    /// — "each side may replace up to `per_side` destroyed actors" — and the
    /// two coalitions do not share one: replacements the opposition spent say
    /// nothing about whether the player's side can still replace, or the other
    /// way round. Chains, so a snapshot of both sides' spending reads
    /// `.with_respawns_used(ScenarioSide::Player, 1).with_respawns_used(ScenarioSide::Enemy, 2)`.
    #[must_use]
    pub fn with_respawns_used(mut self, side: ScenarioSide, used: u32) -> Self {
        self.respawns_used.insert(side, used);
        self
    }

    fn alive_on(&self, sides: &[ScenarioSide]) -> usize {
        self.alive
            .iter()
            .filter(|(side, _)| sides.contains(side))
            .count()
    }

    /// The replacements `side` has spent of its own budget. A side the
    /// snapshot does not mention has spent nothing.
    fn respawns_spent(&self, side: ScenarioSide) -> u32 {
        self.respawns_used.get(&side).copied().unwrap_or_default()
    }
}

/// How a scenario ended for the player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScenarioResult {
    /// The player's side won.
    Victory,
    /// The player's side lost.
    Defeat,
    /// Neither side won.
    Draw,
}

impl ScenarioResult {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Victory => "victory",
            Self::Defeat => "defeat",
            Self::Draw => "draw",
        }
    }
}

impl fmt::Display for ScenarioResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The terminal state of one Instant Action session.
///
/// Deliberately has no campaign field of any kind (cash, node, unlock, run,
/// profile): the outcome identifies the scenario, its seed and its tick, and
/// that is all a consumer can learn from it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioOutcome {
    subject: ContentId,
    seed: ScenarioSeed,
    result: ScenarioResult,
    ended_tick: u64,
}

impl ScenarioOutcome {
    /// The `ia_scenario` the session ran.
    #[must_use]
    pub const fn subject(&self) -> &ContentId {
        &self.subject
    }

    /// The seed the session ran under, so a replay can reproduce it.
    #[must_use]
    pub const fn seed(&self) -> ScenarioSeed {
        self.seed
    }

    /// How it ended.
    #[must_use]
    pub const fn result(&self) -> ScenarioResult {
        self.result
    }

    /// The tick it ended on.
    #[must_use]
    pub const fn ended_tick(&self) -> u64 {
        self.ended_tick
    }
}

/// Decides whether `scenario` has ended in `snapshot`, and how.
///
/// Returns `None` while the scenario is still running. The player's side is
/// the player and ally actors; the opposition is the enemy actors. Neutral
/// traffic never decides an outcome. A side with replacements left in the
/// declared [`RespawnBudget`](cs_content::instant_action::RespawnBudget) is
/// not yet out, so a lone destroyed actor does not end a scenario that would
/// replace it. That budget is counted **per side**, as it is declared: a side
/// is out only when nothing of it is flying and no side of its coalition that
/// flies in this scenario has a replacement left, so replacements the
/// opposition spent never keep the player's side in — or push it out.
///
/// When both coalitions are out at once under
/// [`VictoryCondition::EliminateEnemies`] or
/// [`VictoryCondition::LastSideStanding`] (mutual destruction on one tick),
/// neither condition has a winner and the scenario's declared
/// [`TieOutcome`] decides, rather than a hard-coded precedence.
#[must_use]
pub fn evaluate_outcome(
    scenario: &LoweredScenario,
    snapshot: &ScenarioSnapshot,
) -> Option<ScenarioOutcome> {
    const FRIENDLY: &[ScenarioSide] = &[ScenarioSide::Player, ScenarioSide::Ally];
    const HOSTILE: &[ScenarioSide] = &[ScenarioSide::Enemy];
    let budget = scenario.respawns().per_side();
    let flies = |side: ScenarioSide| scenario.actors().iter().any(|actor| actor.side == side);
    // A coalition can still field an actor when one of the sides it flies
    // here has budget left. A side the roster does not hold has no actor to
    // replace, so its untouched budget is not a reserve — otherwise a roster
    // without allies would keep the player's side "not out" on an ally budget
    // no actor of this scenario could ever spend.
    let can_replace = |sides: &[ScenarioSide]| {
        sides
            .iter()
            .any(|side| flies(*side) && snapshot.respawns_spent(*side) < budget)
    };
    let friendly = snapshot.alive_on(FRIENDLY);
    let hostile = snapshot.alive_on(HOSTILE);
    let friendly_out = friendly == 0 && !can_replace(FRIENDLY);
    let hostile_out = hostile == 0 && !can_replace(HOSTILE);
    let result = match scenario.condition() {
        // Both conditions end on the same two facts — which coalition is still
        // flying — and neither order of checking may swallow the case where
        // neither is: that is a tie, and the rules declare what a tie resolves
        // to. Treating a mutual wipe as a plain loss would throw the declared
        // `tie_outcome` away for the one case that needs it most.
        VictoryCondition::EliminateEnemies | VictoryCondition::LastSideStanding => {
            match (friendly_out, hostile_out) {
                (true, true) => tie(scenario.tie_outcome()),
                (true, false) => ScenarioResult::Defeat,
                (false, true) => ScenarioResult::Victory,
                (false, false) => return None,
            }
        }
        VictoryCondition::SurviveToDeadline => {
            if friendly_out {
                ScenarioResult::Defeat
            } else if scenario
                .deadline_ticks()
                .is_some_and(|deadline| snapshot.tick >= deadline)
            {
                ScenarioResult::Victory
            } else {
                return None;
            }
        }
    };
    Some(ScenarioOutcome {
        subject: scenario.subject().clone(),
        seed: scenario.seed(),
        result,
        ended_tick: snapshot.tick,
    })
}

const fn tie(outcome: TieOutcome) -> ScenarioResult {
    match outcome {
        TieOutcome::Draw => ScenarioResult::Draw,
        TieOutcome::PlayerFavour => ScenarioResult::Victory,
        TieOutcome::OppositionFavour => ScenarioResult::Defeat,
    }
}
