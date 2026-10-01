//! Declared Instant Action presets and custom-scenario schemas (F49-A).
//!
//! Spec: `specs/F49-instant-action-presets-and-custom-scenarios.md`, stage
//! `### F49-A`. Shared contract: `docs/contracts/UI-NETWORK.md` ("Discover
//! original **PC** mode/scenario ids and full option tables. Do not infer this
//! list from High Road to Revenge, a demo or a generic deathmatch library.").
//!
//! This module is the **declared, provenance-carrying input side** of Instant
//! Action. It owns:
//!
//! * [`InstantActionCatalog`] — the presets the original installation offers
//!   plus [`ScenarioOptions`], the closed set of dimensions a custom scenario
//!   may select. "Supported dimension" is a declared list, not "whatever the
//!   loader happened to find".
//! * [`InstantActionPreset`] — one authored preset's identity and its full
//!   parameter set (world, environment, roster, difficulty, victory rules,
//!   seed). A preset is a scenario in its own right, never "a campaign launch
//!   with rewards switched off" (F49 non-negotiable 1).
//! * [`CustomScenarioRequest`] and [`CustomScenarioDraft`] — the same
//!   parameter set addressed by the player's choices, plus
//!   [`InstantActionCatalog::validate_custom`], which refuses impossible
//!   factions, unknown planes, unsupported ordnance and invalid player counts
//!   with **every** problem reported, so a screen can show what to fix instead
//!   of starting a session that can never end (non-negotiable 2, AC04).
//! * [`ScenarioSeed`] — the scenario's explicit root seed and its documented
//!   domain-separated streams. A scenario never draws an implicit seed, so
//!   nothing is randomized behind the evidence record (non-negotiable 4).
//!
//! # Designed vocabulary, not original data
//!
//! Every preset, option table, side label, victory condition and bound in this
//! module is **newly authored synthetic fixture or design**. What the original
//! 2000 PC game actually offers is only partly observed: the F31-D retail route
//! coverage audit counts eight `IA#` mission directories in the installation
//! (`docs/findings/2026-10-01-f31-d-route-coverage-in-every-mission-type.md`),
//! but which of them are presets, what each preset's parameters are, which
//! worlds/environments/airframes/loadouts a custom scenario can pick, the
//! player-count limits, the victory vocabulary and the original preset names
//! are all **unmeasured**. Nothing here may be read as an original preset
//! identity, name, option table or rule. See
//! `docs/findings/2026-10-01-f49-a-instant-action-scenario-schemas.md`.
//!
//! What *is* bound by the earlier stages this module reuses, rather than
//! redefines: [`crate::pilots::DeclaredSurvivability`] (F33-A),
//! [`crate::ai::DifficultyProfile`] (F32-A), [`crate::weapons::DeclaredLoadout`]
//! (F27-A) and the shared `ContentId` identity discipline (F14-A). Custom
//! scenarios go through these same declared validators, which is what the
//! sheet's "the same validators and mission runtime used by campaign" requires
//! at the input side.

use std::collections::BTreeMap;
use std::fmt;

use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::random::SplitMix64;

use crate::ai::{DifficultyProfile, DifficultyTier};
use crate::environment::EnvironmentId;
use crate::pilots::DeclaredSurvivability;
use crate::target_rules::{DeclaredAllegiance, DeclaredRelation};
use crate::world::WorldId;

/// The domain separator of the Instant Action scenario seed stream.
///
/// `cs_types::random::SplitMix64::for_domain` guarantees that a new consumer
/// never shifts a value another consumer already observed, so this constant is
/// owned by this module and may not be reused by another stream.
pub const SCENARIO_SEED_DOMAIN: u64 = 0x4941_5343_454E_4152; // "IASCENAR"

/// Largest accepted number of actors one scenario may declare, across every
/// side. A bound keeps a malformed request from becoming an unbounded spawn
/// request and keeps the lowered plan's ids inside a documented width.
pub const MAX_SCENARIO_ACTORS: usize = 64;

/// Largest accepted number of actors on a single side.
pub const MAX_SCENARIO_ACTORS_PER_SIDE: usize = 16;

/// Largest accepted number of players (human seats) in one scenario.
///
/// Instant Action is authored here as a single-player scenario with wingmates
/// and enemies; the field exists because the sheet's validation rule names
/// "invalid player count" explicitly, and because a multiplayer scenario table
/// (F49's later sibling work) must not be smuggled in through a field that
/// never had a bound. The original game's player-count limits are unmeasured.
pub const MAX_SCENARIO_PLAYERS: u8 = 8;

// ------------------------------------------------------------------ sides ---

/// Which side of the scenario an actor is spawned on.
///
/// The four sides are designed engine vocabulary. They are deliberately
/// **not** factions: a faction is a `ContentId` and allegiance is a directed
/// relation (`crate::target_rules`), so relabeling a side can never relabel a
/// faction and a hostile faction can still be spawned as an ally (which
/// validation then refuses, see [`ScenarioProblem::ImpossibleFaction`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ScenarioSide {
    /// The local player's aircraft.
    Player,
    /// An aircraft flying the player's side.
    Ally,
    /// An aircraft spawned against the player.
    Enemy,
    /// Unaligned traffic.
    Neutral,
}

impl ScenarioSide {
    /// Every side, in a stable order.
    pub const ALL: &'static [ScenarioSide] =
        &[Self::Player, Self::Ally, Self::Enemy, Self::Neutral];

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Player => "player",
            Self::Ally => "ally",
            Self::Enemy => "enemy",
            Self::Neutral => "neutral",
        }
    }

    /// Whether actors on this side fight actors on `other`.
    ///
    /// A side that fights nothing (there is none) is not representable, so a
    /// scenario always has at least one opposing pair of populated sides.
    #[must_use]
    pub const fn is_hostile_to(self, other: Self) -> bool {
        matches!(
            (self, other),
            (Self::Player | Self::Ally, Self::Enemy) | (Self::Enemy, Self::Player | Self::Ally)
        )
    }
}

impl fmt::Display for ScenarioSide {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// One authored slot index within a side.
///
/// A slot is mission-authored, not a catalog asset, so it takes the same `u32`
/// newtype `crate::pilots::WingmateSlot` does: stable within a scenario,
/// comparable, and never a list row number a UI could renumber.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RosterSlot(pub u32);

impl RosterSlot {
    /// The slot's index within its side.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.0
    }
}

impl fmt::Display for RosterSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "roster slot {}", self.0)
    }
}

// ------------------------------------------------------------------ roster ---

/// One authored actor of a scenario: who flies what, on which side.
///
/// Every content reference is a `Resolved` field, so an unmeasured plane or
/// loadout refuses at the boundary instead of lowering to a silent default
/// (`RosterLowerError` in the application boundary repeats the refusal).
#[derive(Clone, Debug, PartialEq)]
pub struct ScenarioActorSpec {
    side: ScenarioSide,
    slot: RosterSlot,
    faction: ContentId,
    airframe: Resolved<ContentId>,
    loadout: Resolved<ContentId>,
    pilot: Option<ContentId>,
    survivability: Resolved<DeclaredSurvivability>,
    provenance: Provenance,
}

impl ScenarioActorSpec {
    /// Assembles and validates one authored actor.
    ///
    /// # Errors
    ///
    /// [`ScenarioSchemaError::FactionKindMismatch`] when `faction` is not in
    /// the `faction` namespace, [`ScenarioSchemaError::AirframeKindMismatch`]
    /// or [`ScenarioSchemaError::LoadoutKindMismatch`] when a *known* id is in
    /// the wrong namespace (an `Resolved::Unknown` passes here and refuses in
    /// validation, which reports every problem at once), and
    /// [`ScenarioSchemaError::PilotKindMismatch`] when `pilot` is set and is
    /// not a `pilot` id.
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        side: ScenarioSide,
        slot: RosterSlot,
        faction: ContentId,
        airframe: Resolved<ContentId>,
        loadout: Resolved<ContentId>,
        pilot: Option<ContentId>,
        survivability: Resolved<DeclaredSurvivability>,
        provenance: Provenance,
    ) -> Result<Self, ScenarioSchemaError> {
        if faction.kind() != ContentKind::Faction {
            return Err(ScenarioSchemaError::FactionKindMismatch { id: faction });
        }
        if let Resolved::Known(Known { value, .. }) = &airframe
            && value.kind() != ContentKind::Airframe
        {
            return Err(ScenarioSchemaError::AirframeKindMismatch { id: value.clone() });
        }
        if let Resolved::Known(Known { value, .. }) = &loadout
            && value.kind() != ContentKind::Loadout
        {
            return Err(ScenarioSchemaError::LoadoutKindMismatch { id: value.clone() });
        }
        if let Some(pilot) = &pilot
            && pilot.kind() != ContentKind::Pilot
        {
            return Err(ScenarioSchemaError::PilotKindMismatch { id: pilot.clone() });
        }
        Ok(Self {
            side,
            slot,
            faction,
            airframe,
            loadout,
            pilot,
            survivability,
            provenance,
        })
    }

    /// The side this actor is spawned on.
    #[must_use]
    pub const fn side(&self) -> ScenarioSide {
        self.side
    }

    /// This actor's slot index within its side.
    #[must_use]
    pub const fn slot(&self) -> RosterSlot {
        self.slot
    }

    /// The faction this actor flies for.
    #[must_use]
    pub const fn faction(&self) -> &ContentId {
        &self.faction
    }

    /// The airframe this actor is built from.
    #[must_use]
    pub const fn airframe(&self) -> &Resolved<ContentId> {
        &self.airframe
    }

    /// The loadout this actor carries.
    #[must_use]
    pub const fn loadout(&self) -> &Resolved<ContentId> {
        &self.loadout
    }

    /// The pilot that flies this actor, when one is authored.
    #[must_use]
    pub const fn pilot(&self) -> Option<&ContentId> {
        self.pilot.as_ref()
    }

    /// The declared survivability of this actor.
    #[must_use]
    pub const fn survivability(&self) -> &Resolved<DeclaredSurvivability> {
        &self.survivability
    }

    /// Where this record came from.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// The full declared roster of one scenario: every side's actors.
///
/// The roster is ordered by `(side, slot)`, so two requests built from the
/// same choices in a different order are the same roster and produce the same
/// lowered plan — non-negotiable 5 ("every visible customization option
/// affects the actual spawned scenario") depends on there being exactly one
/// lowering of a given choice set.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScenarioRoster {
    actors: Vec<ScenarioActorSpec>,
}

impl ScenarioRoster {
    /// Assembles and validates a declared roster.
    ///
    /// # Errors
    ///
    /// [`ScenarioSchemaError::EmptyRoster`], [`ScenarioSchemaError::DuplicateSlot`]
    /// for a `(side, slot)` pair held twice,
    /// [`ScenarioSchemaError::TooManyActors`] above [`MAX_SCENARIO_ACTORS`],
    /// [`ScenarioSchemaError::TooManyActorsOnSide`] above
    /// [`MAX_SCENARIO_ACTORS_PER_SIDE`], and
    /// [`ScenarioSchemaError::MultiplePlayers`] when more than one
    /// [`ScenarioSide::Player`] slot is declared.
    pub fn try_new(mut actors: Vec<ScenarioActorSpec>) -> Result<Self, ScenarioSchemaError> {
        if actors.is_empty() {
            return Err(ScenarioSchemaError::EmptyRoster);
        }
        if actors.len() > MAX_SCENARIO_ACTORS {
            return Err(ScenarioSchemaError::TooManyActors {
                count: actors.len(),
                max: MAX_SCENARIO_ACTORS,
            });
        }
        actors.sort_by_key(|actor| (actor.side, actor.slot));
        for pair in actors.windows(2) {
            if (pair[0].side, pair[0].slot) == (pair[1].side, pair[1].slot) {
                return Err(ScenarioSchemaError::DuplicateSlot {
                    side: pair[0].side,
                    slot: pair[0].slot,
                });
            }
        }
        for side in ScenarioSide::ALL {
            let count = actors.iter().filter(|a| a.side == *side).count();
            if count > MAX_SCENARIO_ACTORS_PER_SIDE {
                return Err(ScenarioSchemaError::TooManyActorsOnSide {
                    side: *side,
                    count,
                    max: MAX_SCENARIO_ACTORS_PER_SIDE,
                });
            }
        }
        if actors
            .iter()
            .filter(|a| a.side == ScenarioSide::Player)
            .count()
            > 1
        {
            return Err(ScenarioSchemaError::MultiplePlayers {
                max: MAX_SCENARIO_PLAYERS,
            });
        }
        Ok(Self { actors })
    }

    /// The declared actors, in `(side, slot)` order.
    #[must_use]
    pub fn actors(&self) -> &[ScenarioActorSpec] {
        &self.actors
    }

    /// The declared actors on one side, in slot order.
    #[must_use]
    pub fn actors_on(&self, side: ScenarioSide) -> Vec<&ScenarioActorSpec> {
        self.actors.iter().filter(|a| a.side == side).collect()
    }

    /// The number of declared actors.
    #[must_use]
    pub fn len(&self) -> usize {
        self.actors.len()
    }

    /// Whether no actor is declared. A roster can never be empty once built.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.actors.is_empty()
    }

    /// The declared actors on `side` that fly for `faction`.
    #[must_use]
    pub fn actors_of_faction(
        &self,
        side: ScenarioSide,
        faction: &ContentId,
    ) -> Vec<&ScenarioActorSpec> {
        self.actors
            .iter()
            .filter(|a| a.side == side && &a.faction == faction)
            .collect()
    }
}

// ------------------------------------------------------------------ rules ---

/// How a scenario ends in the player's favour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VictoryCondition {
    /// The player's side destroys every enemy actor.
    EliminateEnemies,
    /// The side that first loses every actor loses; a declared tie outcome
    /// applies if both sides are emptied on the same tick.
    LastSideStanding,
    /// The player's side is still intact at the declared deadline.
    SurviveToDeadline,
}

impl VictoryCondition {
    /// Every condition, in a stable order.
    pub const ALL: &'static [VictoryCondition] = &[
        Self::EliminateEnemies,
        Self::LastSideStanding,
        Self::SurviveToDeadline,
    ];

    /// The stable label used in reports and as the scenario fingerprint's
    /// condition component.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::EliminateEnemies => "eliminate_enemies",
            Self::LastSideStanding => "last_side_standing",
            Self::SurviveToDeadline => "survive_to_deadline",
        }
    }

    /// Whether this condition needs a declared deadline.
    #[must_use]
    pub const fn needs_deadline(self) -> bool {
        matches!(self, Self::SurviveToDeadline)
    }

    /// Whether this condition needs at least one enemy actor to be meaningful.
    #[must_use]
    pub const fn needs_enemies(self) -> bool {
        matches!(self, Self::EliminateEnemies | Self::LastSideStanding)
    }
}

impl fmt::Display for VictoryCondition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// How many times a side may replace a destroyed actor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RespawnBudget {
    /// A destroyed actor stays destroyed.
    None,
    /// Each side may replace up to `per_side` destroyed actors.
    PerSide {
        /// The replacements allowed on each side.
        per_side: u32,
    },
}

impl RespawnBudget {
    /// The replacements this budget allows on one side.
    #[must_use]
    pub const fn per_side(self) -> u32 {
        match self {
            Self::None => 0,
            Self::PerSide { per_side } => per_side,
        }
    }
}

/// The declared end conditions and replacement budget of one scenario.
///
/// A deadline is required by [`VictoryCondition::SurviveToDeadline`] and
/// refused for the other two: a declared limit that the condition cannot end
/// on would be an option that affects no actor, which non-negotiable 5
/// forbids.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VictoryRules {
    condition: VictoryCondition,
    respawns: RespawnBudget,
    deadline_ticks: Option<u64>,
    tie_outcome: TieOutcome,
}

impl VictoryRules {
    /// Assembles and validates victory rules.
    ///
    /// # Errors
    ///
    /// [`ScenarioSchemaError::DeadlineRequired`] when the condition needs a
    /// deadline and none is declared, and
    /// [`ScenarioSchemaError::DeadlineNotApplicable`] when one is declared for
    /// a condition that cannot end on it.
    pub fn try_new(
        condition: VictoryCondition,
        respawns: RespawnBudget,
        deadline_ticks: Option<u64>,
        tie_outcome: TieOutcome,
    ) -> Result<Self, ScenarioSchemaError> {
        match (condition.needs_deadline(), deadline_ticks) {
            (true, None) => return Err(ScenarioSchemaError::DeadlineRequired { condition }),
            (false, Some(_)) => {
                return Err(ScenarioSchemaError::DeadlineNotApplicable { condition });
            }
            (true, Some(0)) => {
                return Err(ScenarioSchemaError::ZeroDeadline { condition });
            }
            _ => {}
        }
        Ok(Self {
            condition,
            respawns,
            deadline_ticks,
            tie_outcome,
        })
    }

    /// The declared end condition.
    #[must_use]
    pub const fn condition(&self) -> VictoryCondition {
        self.condition
    }

    /// The declared replacement budget.
    #[must_use]
    pub const fn respawns(&self) -> RespawnBudget {
        self.respawns
    }

    /// The declared deadline in whole simulation ticks, when the condition
    /// takes one.
    #[must_use]
    pub const fn deadline_ticks(&self) -> Option<u64> {
        self.deadline_ticks
    }

    /// The declared outcome for a tie.
    #[must_use]
    pub const fn tie_outcome(&self) -> TieOutcome {
        self.tie_outcome
    }
}

/// What a tie resolves to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TieOutcome {
    /// The scenario is recorded as drawn.
    Draw,
    /// The player's side wins the tie.
    PlayerFavour,
    /// The side opposed to the player wins the tie.
    OppositionFavour,
}

impl TieOutcome {
    /// Every outcome, in a stable order.
    pub const ALL: &'static [TieOutcome] =
        &[Self::Draw, Self::PlayerFavour, Self::OppositionFavour];

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Draw => "draw",
            Self::PlayerFavour => "player_favour",
            Self::OppositionFavour => "opposition_favour",
        }
    }
}

impl fmt::Display for TieOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

// ------------------------------------------------------------------- seed ---

/// A scenario's explicit root seed and its derived streams.
///
/// The root seed is a required field of every preset and every custom
/// request: nothing in the Instant Action path may draw an implicit seed
/// (non-negotiable 4, "no randomization hidden from evidence"). A consumer
/// derives its own stream through [`ScenarioSeed::stream`], so two consumers
/// of the same scenario never shift each other's values and a new consumer can
/// be added later without moving an observed one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScenarioSeed {
    root: u64,
}

impl ScenarioSeed {
    /// The root seed of one scenario.
    #[must_use]
    pub const fn new(root: u64) -> Self {
        Self { root }
    }

    /// The root seed's value, which a developer tool displays and a replay
    /// records.
    #[must_use]
    pub const fn root(self) -> u64 {
        self.root
    }

    /// The seed of one consumer's stream within this scenario.
    ///
    /// `domain` is the consumer's own domain constant; using a different one
    /// yields an independent stream from the same scenario seed.
    #[must_use]
    pub fn stream(self, domain: u64) -> u64 {
        SplitMix64::for_domain(self.root, domain).next_u64()
    }

    /// The generator one consumer draws from within this scenario.
    #[must_use]
    pub fn generator(self, domain: u64) -> SplitMix64 {
        SplitMix64::for_domain(self.root, domain)
    }
}

// --------------------------------------------------------------- scenarios ---

/// The scenario parameters a preset and a custom request share.
///
/// One struct rather than two, because non-negotiable 1 requires that an
/// Instant Action mission *is* its authored scenario: a preset and a custom
/// request must be describable by the same parameter set, or "custom" would
/// silently be a different, lesser mission type.
#[derive(Clone, Debug, PartialEq)]
pub struct ScenarioParameters {
    world: Resolved<WorldId>,
    environment: Resolved<EnvironmentId>,
    roster: ScenarioRoster,
    difficulty: DifficultyProfile,
    rules: VictoryRules,
    seed: ScenarioSeed,
}

impl ScenarioParameters {
    /// Assembles the shared parameter set.
    #[must_use]
    pub const fn new(
        world: Resolved<WorldId>,
        environment: Resolved<EnvironmentId>,
        roster: ScenarioRoster,
        difficulty: DifficultyProfile,
        rules: VictoryRules,
        seed: ScenarioSeed,
    ) -> Self {
        Self {
            world,
            environment,
            roster,
            difficulty,
            rules,
            seed,
        }
    }

    /// The world this scenario loads.
    #[must_use]
    pub const fn world(&self) -> &Resolved<WorldId> {
        &self.world
    }

    /// The environment this scenario runs under.
    #[must_use]
    pub const fn environment(&self) -> &Resolved<EnvironmentId> {
        &self.environment
    }

    /// The declared roster.
    #[must_use]
    pub const fn roster(&self) -> &ScenarioRoster {
        &self.roster
    }

    /// The declared difficulty profile.
    #[must_use]
    pub const fn difficulty(&self) -> &DifficultyProfile {
        &self.difficulty
    }

    /// The declared victory rules.
    #[must_use]
    pub const fn rules(&self) -> &VictoryRules {
        &self.rules
    }

    /// The scenario's explicit root seed.
    #[must_use]
    pub const fn seed(&self) -> ScenarioSeed {
        self.seed
    }
}

/// One authored Instant Action preset.
///
/// A preset keeps its own `ia_preset` **and** `ia_scenario` identity: the
/// scenario id is the `IA#` scenario the preset launches, and the preset id is
/// the entry the player selects. Collapsing them would make it impossible to
/// say which authored scenario a selection produced, which is what
/// non-negotiable 1 asks to preserve.
#[derive(Clone, Debug, PartialEq)]
pub struct InstantActionPreset {
    id: ContentId,
    scenario: ContentId,
    title: Resolved<String>,
    parameters: ScenarioParameters,
    origin: Origin,
    provenance: Provenance,
}

impl InstantActionPreset {
    /// Assembles and validates a preset.
    ///
    /// # Errors
    ///
    /// [`ScenarioSchemaError::PresetKindMismatch`] when `id` is not an
    /// `ia_preset` id, [`ScenarioSchemaError::ScenarioKindMismatch`] when
    /// `scenario` is not an `ia_scenario` id, and
    /// [`ScenarioSchemaError::TitleNotMeasured`] when the title is known but
    /// longer than [`MAX_PRESET_TITLE_BYTES`] or empty.
    pub fn try_new(
        id: ContentId,
        scenario: ContentId,
        title: Resolved<String>,
        parameters: ScenarioParameters,
        origin: Origin,
        provenance: Provenance,
    ) -> Result<Self, ScenarioSchemaError> {
        if id.kind() != ContentKind::IaPreset {
            return Err(ScenarioSchemaError::PresetKindMismatch { id });
        }
        if scenario.kind() != ContentKind::IaScenario {
            return Err(ScenarioSchemaError::ScenarioKindMismatch { id: scenario });
        }
        if let Resolved::Known(Known { value, .. }) = &title {
            if value.is_empty() {
                return Err(ScenarioSchemaError::TitleNotMeasured {
                    reason: "the preset title is empty".to_owned(),
                });
            }
            if value.len() > MAX_PRESET_TITLE_BYTES {
                return Err(ScenarioSchemaError::TitleTooLong {
                    len: value.len(),
                    max: MAX_PRESET_TITLE_BYTES,
                });
            }
        }
        Ok(Self {
            id,
            scenario,
            title,
            parameters,
            origin,
            provenance,
        })
    }

    /// The `ia_preset` id this preset is selected by.
    #[must_use]
    pub const fn id(&self) -> &ContentId {
        &self.id
    }

    /// The `ia_scenario` id this preset launches.
    #[must_use]
    pub const fn scenario(&self) -> &ContentId {
        &self.scenario
    }

    /// The original preset's display name, when it has been measured.
    ///
    /// An original title is **unmeasured**, so this stays
    /// [`Resolved::Unknown`] until an evidence stage reads it; the catalog is
    /// usable without it and the UI reports "unknown" instead of inventing a
    /// name.
    #[must_use]
    pub const fn title(&self) -> &Resolved<String> {
        &self.title
    }

    /// The scenario parameters this preset launches.
    #[must_use]
    pub const fn parameters(&self) -> &ScenarioParameters {
        &self.parameters
    }

    /// Where the preset came from.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// Where the record itself came from.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// Largest accepted original preset title, in bytes.
pub const MAX_PRESET_TITLE_BYTES: usize = 128;

/// The player's selected set of custom-scenario dimensions.
///
/// Every field is optional **only** so a partially filled draft can be shown
/// with its problems; [`CustomScenarioDraft::resolve`] refuses an incomplete
/// draft. There is no default for any dimension: an unset world is an error,
/// never "the first world in the catalog".
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CustomScenarioDraft {
    subject: Option<ContentId>,
    world: Option<Resolved<WorldId>>,
    environment: Option<Resolved<EnvironmentId>>,
    roster: Option<Vec<ScenarioActorSpec>>,
    difficulty: Option<DifficultyProfile>,
    rules: Option<VictoryRules>,
    seed: Option<ScenarioSeed>,
    players: Option<u8>,
    provenance: Option<Provenance>,
}

impl CustomScenarioDraft {
    /// An empty draft: every dimension unset.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the `ia_scenario` identity the custom scenario will be saved as.
    #[must_use]
    pub fn with_subject(mut self, subject: ContentId) -> Self {
        self.subject = Some(subject);
        self
    }

    /// Sets the selected world.
    #[must_use]
    pub fn with_world(mut self, world: Resolved<WorldId>) -> Self {
        self.world = Some(world);
        self
    }

    /// Sets the selected environment.
    #[must_use]
    pub fn with_environment(mut self, environment: Resolved<EnvironmentId>) -> Self {
        self.environment = Some(environment);
        self
    }

    /// Sets the selected roster actors.
    #[must_use]
    pub fn with_roster(mut self, roster: Vec<ScenarioActorSpec>) -> Self {
        self.roster = Some(roster);
        self
    }

    /// Sets the selected difficulty profile.
    #[must_use]
    pub fn with_difficulty(mut self, difficulty: DifficultyProfile) -> Self {
        self.difficulty = Some(difficulty);
        self
    }

    /// Sets the selected victory rules.
    #[must_use]
    pub fn with_rules(mut self, rules: VictoryRules) -> Self {
        self.rules = Some(rules);
        self
    }

    /// Sets the scenario's explicit root seed.
    #[must_use]
    pub fn with_seed(mut self, seed: ScenarioSeed) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Sets the number of human seats.
    #[must_use]
    pub fn with_players(mut self, players: u8) -> Self {
        self.players = Some(players);
        self
    }

    /// Sets the provenance the resolved request will carry.
    #[must_use]
    pub fn with_provenance(mut self, provenance: Provenance) -> Self {
        self.provenance = Some(provenance);
        self
    }

    /// Which dimensions are still unset, as stable field names a screen can
    /// highlight.
    ///
    /// The order is the declaration order of the sheet's dimension list —
    /// environment, roster, loadouts, skill, rules, seed — preceded by the
    /// identity, so the report reads like the form.
    #[must_use]
    pub fn unset_dimensions(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        if self.subject.is_none() {
            missing.push("subject");
        }
        if self.world.is_none() {
            missing.push("world");
        }
        if self.environment.is_none() {
            missing.push("environment");
        }
        if self.roster.is_none() {
            missing.push("roster");
        }
        if self.difficulty.is_none() {
            missing.push("skill");
        }
        if self.rules.is_none() {
            missing.push("rules");
        }
        if self.seed.is_none() {
            missing.push("seed");
        }
        if self.players.is_none() {
            missing.push("players");
        }
        missing
    }

    /// Whether every dimension is set.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.unset_dimensions().is_empty()
    }

    /// Resolves the draft into a validated request.
    ///
    /// The roster is the only dimension whose *value* can still be invalid, so
    /// it is the only one that returns a [`ScenarioSchemaError`] here; the
    /// remaining rules (unknown planes, unsupported ordnance, impossible
    /// factions, invalid player counts) belong to
    /// [`InstantActionCatalog::validate_custom`], which reports all of them
    /// together.
    ///
    /// # Errors
    ///
    /// [`ScenarioSchemaError::IncompleteDraft`] naming every unset dimension,
    /// [`ScenarioSchemaError::MissingProvenance`] when no provenance was set,
    /// [`ScenarioSchemaError::SubjectKindMismatch`] when the subject is not an
    /// `ia_scenario` id, [`ScenarioSchemaError::InvalidPlayerCount`] when the
    /// declared seat count is zero or above [`MAX_SCENARIO_PLAYERS`], and
    /// whatever [`ScenarioRoster::try_new`] refuses.
    pub fn resolve(self) -> Result<CustomScenarioRequest, ScenarioSchemaError> {
        let missing = self.unset_dimensions();
        if !missing.is_empty() {
            return Err(ScenarioSchemaError::IncompleteDraft { missing });
        }
        let provenance = self
            .provenance
            .ok_or(ScenarioSchemaError::MissingProvenance)?;
        let subject = self.subject.expect("checked above");
        if subject.kind() != ContentKind::IaScenario {
            return Err(ScenarioSchemaError::SubjectKindMismatch { id: subject });
        }
        let players = self.players.expect("checked above");
        if players == 0 || players > MAX_SCENARIO_PLAYERS {
            return Err(ScenarioSchemaError::InvalidPlayerCount {
                players,
                max: MAX_SCENARIO_PLAYERS,
            });
        }
        let roster = ScenarioRoster::try_new(self.roster.expect("checked above"))?;
        let parameters = ScenarioParameters::new(
            self.world.expect("checked above"),
            self.environment.expect("checked above"),
            roster,
            self.difficulty.expect("checked above"),
            self.rules.expect("checked above"),
            self.seed.expect("checked above"),
        );
        Ok(CustomScenarioRequest {
            subject,
            parameters,
            players,
            provenance,
        })
    }
}

/// A player's fully selected custom scenario, validated but not yet checked
/// against the catalog's supported option table.
///
/// Splitting construction from [`InstantActionCatalog::validate_custom`] is
/// what makes AC04 possible: a complete request can exist and still be
/// refused, so "invalid" is a state a request can be in rather than something
/// that stops it from being represented.
#[derive(Clone, Debug, PartialEq)]
pub struct CustomScenarioRequest {
    subject: ContentId,
    parameters: ScenarioParameters,
    players: u8,
    provenance: Provenance,
}

impl CustomScenarioRequest {
    /// The `ia_scenario` identity this custom scenario runs under.
    #[must_use]
    pub const fn subject(&self) -> &ContentId {
        &self.subject
    }

    /// The selected parameters.
    #[must_use]
    pub const fn parameters(&self) -> &ScenarioParameters {
        &self.parameters
    }

    /// The number of human seats the scenario declares.
    #[must_use]
    pub const fn players(&self) -> u8 {
        self.players
    }

    /// Where the record came from.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

// -------------------------------------------------------------- problems ---

/// The stable code of one custom-scenario validation problem.
///
/// Codes are stable strings so a screen, a test and a report all name a
/// problem the same way without depending on the `Display` text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ScenarioProblemCode {
    /// The selected world is not in the supported option table.
    UnsupportedWorld,
    /// The selected environment is not in the supported option table.
    UnsupportedEnvironment,
    /// An actor's airframe is not in the supported option table.
    UnsupportedAirframe,
    /// An actor's loadout is not in the supported option table.
    UnsupportedLoadout,
    /// An actor's airframe or loadout is `Resolved::Unknown`.
    UnmeasuredActorField,
    /// An actor's survivability is `Resolved::Unknown`.
    UnmeasuredSurvivability,
    /// A faction id is not in the `faction` namespace.
    FactionKindMismatch,
    /// The roster declares no actor on a side the scenario needs.
    MissingSide,
    /// The declared relations make the roster impossible to fight.
    ImpossibleFaction,
    /// The declared seat count does not match the declared player slots.
    InvalidPlayerCount,
    /// The victory condition needs enemies and the roster declares none.
    ConditionUnsatisfiable,
    /// The selected difficulty tier is not in the supported option table.
    UnsupportedDifficulty,
    /// The selected victory condition is not in the supported option table.
    UnsupportedVictoryCondition,
}

impl ScenarioProblemCode {
    /// The stable code string.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::UnsupportedWorld => "unsupported_world",
            Self::UnsupportedEnvironment => "unsupported_environment",
            Self::UnsupportedAirframe => "unsupported_airframe",
            Self::UnsupportedLoadout => "unsupported_loadout",
            Self::UnmeasuredActorField => "unmeasured_actor_field",
            Self::UnmeasuredSurvivability => "unmeasured_survivability",
            Self::FactionKindMismatch => "faction_kind_mismatch",
            Self::MissingSide => "missing_side",
            Self::ImpossibleFaction => "impossible_faction",
            Self::InvalidPlayerCount => "invalid_player_count",
            Self::ConditionUnsatisfiable => "condition_unsatisfiable",
            Self::UnsupportedDifficulty => "unsupported_difficulty",
            Self::UnsupportedVictoryCondition => "unsupported_victory_condition",
        }
    }

    /// The short form name of the dimension this problem is about, matching
    /// [`CustomScenarioDraft::unset_dimensions`].
    #[must_use]
    pub const fn dimension(self) -> &'static str {
        match self {
            Self::UnsupportedWorld | Self::UnmeasuredActorField => "world",
            Self::UnsupportedEnvironment => "environment",
            Self::UnsupportedAirframe | Self::UnsupportedLoadout => "roster",
            Self::UnmeasuredSurvivability => "roster",
            Self::FactionKindMismatch | Self::ImpossibleFaction => "roster",
            Self::MissingSide => "roster",
            Self::InvalidPlayerCount => "players",
            Self::ConditionUnsatisfiable | Self::UnsupportedVictoryCondition => "rules",
            Self::UnsupportedDifficulty => "skill",
        }
    }
}

impl fmt::Display for ScenarioProblemCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// One actionable custom-scenario validation problem.
///
/// Every field is present because a screen has to act on the problem: which
/// dimension to highlight, which id to replace, and what a developer tool
/// prints. `unknown(claim_id, reason)` is kept verbatim so an unmeasured value
/// reaches the reader as the recorded unknown it is, not as a default.
#[derive(Clone, Debug, PartialEq)]
pub struct ScenarioProblem {
    code: ScenarioProblemCode,
    dimension: &'static str,
    offender: Option<String>,
    claim_id: Option<ClaimId>,
    detail: String,
}

impl ScenarioProblem {
    /// A problem about a named dimension, with no id attached.
    #[must_use]
    pub fn new(code: ScenarioProblemCode, detail: String) -> Self {
        Self {
            code,
            dimension: code.dimension(),
            offender: None,
            claim_id: None,
            detail,
        }
    }

    /// A problem about a specific selection in the code's dimension.
    ///
    /// The offender is stored as text because a dimension's selection is not
    /// always a `ContentId`: a world is a `WorldId` and an environment an
    /// `EnvironmentId`, and a report must name what the player picked rather
    /// than force it into a namespace it does not belong to.
    #[must_use]
    pub fn about(code: ScenarioProblemCode, offender: impl fmt::Display, detail: String) -> Self {
        Self {
            code,
            dimension: code.dimension(),
            offender: Some(offender.to_string()),
            claim_id: None,
            detail,
        }
    }

    /// A problem about an `Resolved::Unknown`, carrying its claim and reason.
    #[must_use]
    pub fn unknown(
        code: ScenarioProblemCode,
        dimension: &'static str,
        claim_id: ClaimId,
        reason: &str,
    ) -> Self {
        Self {
            code,
            dimension,
            offender: None,
            claim_id: Some(claim_id),
            detail: reason.to_owned(),
        }
    }

    /// The stable problem code.
    #[must_use]
    pub const fn code(&self) -> ScenarioProblemCode {
        self.code
    }

    /// The scenario dimension this problem is about.
    #[must_use]
    pub const fn dimension(&self) -> &'static str {
        self.dimension
    }

    /// The selection that must be replaced, when the problem names one.
    #[must_use]
    pub fn offender(&self) -> Option<&str> {
        self.offender.as_deref()
    }

    /// The claim an unmeasured value is recorded under.
    #[must_use]
    pub const fn claim_id(&self) -> Option<&ClaimId> {
        self.claim_id.as_ref()
    }

    /// The human-readable detail a screen shows.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl fmt::Display for ScenarioProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({}): ", self.code.code(), self.dimension)?;
        if let Some(offender) = &self.offender {
            write!(f, "{offender} ")?;
        }
        if let Some(claim) = &self.claim_id {
            write!(f, "under {claim} ")?;
        }
        f.write_str(&self.detail)
    }
}

/// Every problem a custom scenario has, sorted by `(code, dimension, detail)`
/// so a report and a test see one stable order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScenarioProblems {
    problems: Vec<ScenarioProblem>,
}

impl ScenarioProblems {
    /// Sorts and wraps a list of problems.
    #[must_use]
    pub fn new(mut problems: Vec<ScenarioProblem>) -> Self {
        problems.sort_by(|a, b| {
            (a.code, a.dimension, &a.detail).cmp(&(b.code, b.dimension, &b.detail))
        });
        Self { problems }
    }

    /// The problems, in stable order.
    #[must_use]
    pub fn problems(&self) -> &[ScenarioProblem] {
        &self.problems
    }

    /// Whether there is no problem.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.problems.is_empty()
    }

    /// The number of problems.
    #[must_use]
    pub fn len(&self) -> usize {
        self.problems.len()
    }

    /// Every problem with the given code.
    #[must_use]
    pub fn with_code(&self, code: ScenarioProblemCode) -> Vec<&ScenarioProblem> {
        self.problems.iter().filter(|p| p.code == code).collect()
    }

    /// Every problem about the named dimension.
    #[must_use]
    pub fn on_dimension(&self, dimension: &str) -> Vec<&ScenarioProblem> {
        self.problems
            .iter()
            .filter(|p| p.dimension == dimension)
            .collect()
    }
}

impl fmt::Display for ScenarioProblems {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, problem) in self.problems.iter().enumerate() {
            if index > 0 {
                f.write_str("; ")?;
            }
            write!(f, "{problem}")?;
        }
        Ok(())
    }
}

// --------------------------------------------------------------- options ---

/// The closed set of dimensions a custom scenario may select.
///
/// This is the sheet's "all supported custom-scenario dimensions discovered
/// from data/UI" as a **declared list**. A selection outside it is refused with
/// a [`ScenarioProblem`], which is what makes "every visible customization
/// option affects the actual spawned scenario" checkable: an option the
/// catalog does not declare cannot be displayed and cannot be lowered.
#[derive(Clone, Debug, PartialEq)]
pub struct ScenarioOptions {
    worlds: Vec<WorldId>,
    environments: Vec<EnvironmentId>,
    factions: Vec<ContentId>,
    airframes: Vec<ContentId>,
    loadouts: Vec<ContentId>,
    difficulty_tiers: Vec<DifficultyTier>,
    victory_conditions: Vec<VictoryCondition>,
    relations: Vec<DeclaredRelation>,
    max_players: u8,
}

impl ScenarioOptions {
    /// Assembles and validates the option table.
    ///
    /// # Errors
    ///
    /// [`ScenarioSchemaError::EmptyOptionTable`] naming every option list that
    /// is empty, [`ScenarioSchemaError::OptionKindMismatch`] for an id in the
    /// wrong namespace, [`ScenarioSchemaError::DuplicateOption`] for an id held
    /// twice, [`ScenarioSchemaError::InvalidPlayerLimit`] when `max_players` is
    /// zero or above [`MAX_SCENARIO_PLAYERS`], and
    /// [`ScenarioSchemaError::UnknownDifficultyTier`] /
    /// [`ScenarioSchemaError::UnknownVictoryCondition`] for a tier or condition
    /// outside the closed vocabulary.
    // Each argument is one declared option list; grouping them into a struct
    // would hide which list is empty or mis-namespaced at the call site.
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        mut worlds: Vec<WorldId>,
        mut environments: Vec<EnvironmentId>,
        mut factions: Vec<ContentId>,
        mut airframes: Vec<ContentId>,
        mut loadouts: Vec<ContentId>,
        mut difficulty_tiers: Vec<DifficultyTier>,
        mut victory_conditions: Vec<VictoryCondition>,
        relations: Vec<DeclaredRelation>,
        max_players: u8,
    ) -> Result<Self, ScenarioSchemaError> {
        check_unique_list("worlds", &worlds)?;
        check_unique_list("environments", &environments)?;
        check_option_list("factions", &factions, ContentKind::Faction)?;
        check_option_list("airframes", &airframes, ContentKind::Airframe)?;
        check_option_list("loadouts", &loadouts, ContentKind::Loadout)?;
        if difficulty_tiers.is_empty() || victory_conditions.is_empty() {
            return Err(ScenarioSchemaError::EmptyOptionTable {
                what: "difficulty tiers or victory conditions",
            });
        }
        if max_players == 0 || max_players > MAX_SCENARIO_PLAYERS {
            return Err(ScenarioSchemaError::InvalidPlayerLimit {
                max: max_players,
                ceiling: MAX_SCENARIO_PLAYERS,
            });
        }
        for tier in &difficulty_tiers {
            if !DifficultyTier::ALL.contains(tier) {
                return Err(ScenarioSchemaError::UnknownDifficultyTier { tier: *tier });
            }
        }
        for condition in &victory_conditions {
            if !VictoryCondition::ALL.contains(condition) {
                return Err(ScenarioSchemaError::UnknownVictoryCondition {
                    condition: *condition,
                });
            }
        }
        worlds.sort();
        environments.sort();
        factions.sort();
        airframes.sort();
        loadouts.sort();
        difficulty_tiers.sort();
        victory_conditions.sort();
        Ok(Self {
            worlds,
            environments,
            factions,
            airframes,
            loadouts,
            difficulty_tiers,
            victory_conditions,
            relations,
            max_players,
        })
    }

    /// The supported worlds, in canonical id order.
    #[must_use]
    pub fn worlds(&self) -> &[WorldId] {
        &self.worlds
    }

    /// The supported environments, in canonical order.
    #[must_use]
    pub fn environments(&self) -> &[EnvironmentId] {
        &self.environments
    }

    /// The selectable factions, in canonical id order.
    #[must_use]
    pub fn factions(&self) -> &[ContentId] {
        &self.factions
    }

    /// The selectable airframes, in canonical id order.
    #[must_use]
    pub fn airframes(&self) -> &[ContentId] {
        &self.airframes
    }

    /// The selectable loadouts, in canonical id order.
    #[must_use]
    pub fn loadouts(&self) -> &[ContentId] {
        &self.loadouts
    }

    /// The selectable difficulty tiers, in ascending order.
    #[must_use]
    pub fn difficulty_tiers(&self) -> &[DifficultyTier] {
        &self.difficulty_tiers
    }

    /// The selectable victory conditions, in canonical order.
    #[must_use]
    pub fn victory_conditions(&self) -> &[VictoryCondition] {
        &self.victory_conditions
    }

    /// The declared cross-faction relations the roster is judged against.
    #[must_use]
    pub fn relations(&self) -> &[DeclaredRelation] {
        &self.relations
    }

    /// The largest seat count a custom scenario may declare.
    #[must_use]
    pub const fn max_players(&self) -> u8 {
        self.max_players
    }

    /// The declared relation of `from` toward `to`.
    ///
    /// A self-pair is friendly by the relation table's own invariant, and an
    /// undeclared pair is `None` — never assumed hostile.
    #[must_use]
    pub fn relation(&self, from: &ContentId, to: &ContentId) -> Option<&DeclaredRelation> {
        if from == to {
            return None;
        }
        self.relations
            .iter()
            .find(|relation| &relation.from == from && &relation.to == to)
    }
}

/// Refuses an empty typed option list and a duplicate entry.
///
/// The worlds and environments are already-validated identity newtypes
/// ([`WorldId`], [`EnvironmentId`]), so they need the emptiness and uniqueness
/// checks but no namespace check: the grammar is enforced in their own
/// constructors, which this table never bypasses.
fn check_unique_list<T: Ord + Clone + fmt::Display>(
    what: &'static str,
    ids: &[T],
) -> Result<(), ScenarioSchemaError> {
    if ids.is_empty() {
        return Err(ScenarioSchemaError::EmptyOptionTable { what });
    }
    let mut sorted: Vec<&T> = ids.iter().collect();
    sorted.sort();
    for pair in sorted.windows(2) {
        if pair[0] == pair[1] {
            return Err(ScenarioSchemaError::DuplicateOption {
                what,
                entry: pair[0].to_string(),
            });
        }
    }
    Ok(())
}

/// Refuses an empty option list, an id in the wrong namespace and a duplicate.
fn check_option_list(
    what: &'static str,
    ids: &[ContentId],
    kind: ContentKind,
) -> Result<(), ScenarioSchemaError> {
    if ids.is_empty() {
        return Err(ScenarioSchemaError::EmptyOptionTable { what });
    }
    let mut seen: Vec<&ContentId> = Vec::with_capacity(ids.len());
    for id in ids {
        if id.kind() != kind {
            return Err(ScenarioSchemaError::OptionKindMismatch {
                what,
                id: id.clone(),
                expected: kind,
            });
        }
        if seen.contains(&id) {
            return Err(ScenarioSchemaError::DuplicateOption {
                what,
                entry: id.as_str().to_owned(),
            });
        }
        seen.push(id);
    }
    Ok(())
}

// --------------------------------------------------------------- catalog ---

/// Every Instant Action preset plus the supported custom-scenario dimensions.
///
/// The catalog is the single place a selection is resolved against, so a
/// preset and a custom request are validated by the *same* option table — the
/// sheet's "the same validators ... used by campaign" requirement at the
/// catalog level.
#[derive(Clone, Debug, PartialEq)]
pub struct InstantActionCatalog {
    presets: BTreeMap<ContentId, InstantActionPreset>,
    options: ScenarioOptions,
    provenance: Provenance,
}

impl InstantActionCatalog {
    /// Assembles and validates a catalog.
    ///
    /// # Errors
    ///
    /// [`ScenarioSchemaError::DuplicatePreset`] when two presets share one
    /// `ia_preset` id, [`ScenarioSchemaError::DuplicateScenario`] when two
    /// presets launch the same `ia_scenario` id, and
    /// [`ScenarioSchemaError::EmptyOptionTable`] when the option table is
    /// empty.
    pub fn try_new(
        presets: Vec<InstantActionPreset>,
        options: ScenarioOptions,
        provenance: Provenance,
    ) -> Result<Self, ScenarioSchemaError> {
        if presets.is_empty() {
            return Err(ScenarioSchemaError::NoPresets);
        }
        let mut by_id: BTreeMap<ContentId, InstantActionPreset> = BTreeMap::new();
        let mut scenarios: BTreeMap<ContentId, ContentId> = BTreeMap::new();
        for preset in presets {
            if by_id.insert(preset.id().clone(), preset.clone()).is_some() {
                return Err(ScenarioSchemaError::DuplicatePreset {
                    id: preset.id().clone(),
                });
            }
            if let Some(first) = scenarios.insert(preset.scenario().clone(), preset.id().clone())
                && first != *preset.id()
            {
                return Err(ScenarioSchemaError::DuplicateScenario(Box::new(
                    DuplicateScenarioClaim {
                        scenario: preset.scenario().clone(),
                        first: first.clone(),
                        second: preset.id().clone(),
                    },
                )));
            }
        }
        Ok(Self {
            presets: by_id,
            options,
            provenance,
        })
    }

    /// Every preset id, in canonical id order.
    #[must_use]
    pub fn preset_ids(&self) -> Vec<&ContentId> {
        self.presets.keys().collect()
    }

    /// Every preset, in canonical id order.
    #[must_use]
    pub fn presets(&self) -> Vec<&InstantActionPreset> {
        self.presets.values().collect()
    }

    /// One preset by its `ia_preset` id.
    #[must_use]
    pub fn preset(&self, id: &ContentId) -> Option<&InstantActionPreset> {
        self.presets.get(id)
    }

    /// The preset that launches a given `ia_scenario` id.
    #[must_use]
    pub fn preset_for_scenario(&self, scenario: &ContentId) -> Option<&InstantActionPreset> {
        self.presets
            .values()
            .find(|preset| preset.scenario() == scenario)
    }

    /// The supported custom-scenario dimensions.
    #[must_use]
    pub const fn options(&self) -> &ScenarioOptions {
        &self.options
    }

    /// Where the catalog came from.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// Whether every preset is backed by original installation data.
    ///
    /// The F49-D retail catalog verification turns this true; until then it is
    /// false and a caller must treat the catalog as synthetic.
    #[must_use]
    pub fn is_original_complete(&self) -> bool {
        !self.presets.is_empty() && self.presets.values().all(|p| p.origin().is_original())
    }

    /// Every problem a custom scenario has against this catalog's option table.
    ///
    /// Reports **all** problems, in stable order, so AC04's "actionable
    /// validation errors" can be a list a screen renders rather than a single
    /// error a user retries into forever.
    #[must_use]
    pub fn validate_custom(&self, request: &CustomScenarioRequest) -> ScenarioProblems {
        let mut found = Vec::new();
        let parameters = request.parameters();

        match parameters.world() {
            Resolved::Unknown { claim_id, reason } => found.push(ScenarioProblem::unknown(
                ScenarioProblemCode::UnsupportedWorld,
                "world",
                claim_id.clone(),
                reason,
            )),
            Resolved::Known(Known { value, .. }) => {
                if !self.options.worlds().contains(value) {
                    found.push(ScenarioProblem::about(
                        ScenarioProblemCode::UnsupportedWorld,
                        value.clone(),
                        "is not one of the world's a custom scenario may load".to_owned(),
                    ));
                }
            }
        }
        match parameters.environment() {
            Resolved::Unknown { claim_id, reason } => found.push(ScenarioProblem::unknown(
                ScenarioProblemCode::UnsupportedEnvironment,
                "environment",
                claim_id.clone(),
                reason,
            )),
            Resolved::Known(Known { value, .. }) => {
                if !self.options.environments().contains(value) {
                    found.push(ScenarioProblem::about(
                        ScenarioProblemCode::UnsupportedEnvironment,
                        value.clone(),
                        "is not one of the environments a custom scenario may run under".to_owned(),
                    ));
                }
            }
        }
        if !self
            .options
            .difficulty_tiers()
            .contains(&parameters.difficulty().tier())
        {
            found.push(ScenarioProblem::new(
                ScenarioProblemCode::UnsupportedDifficulty,
                format!(
                    "difficulty tier {} is not offered by this catalog",
                    parameters.difficulty().tier()
                ),
            ));
        }
        if !self
            .options
            .victory_conditions()
            .contains(&parameters.rules().condition())
        {
            found.push(ScenarioProblem::new(
                ScenarioProblemCode::UnsupportedVictoryCondition,
                format!(
                    "victory condition {} is not offered by this catalog",
                    parameters.rules().condition()
                ),
            ));
        }
        if request.players() == 0 || request.players() > self.options.max_players() {
            found.push(ScenarioProblem::new(
                ScenarioProblemCode::InvalidPlayerCount,
                format!(
                    "{} seats is outside the supported 1..={} range",
                    request.players(),
                    self.options.max_players()
                ),
            ));
        }
        found.extend(self.check_roster(parameters.roster()));
        found.extend(self.check_victory_feasibility(parameters));
        ScenarioProblems::new(found)
    }

    /// Refuses a custom scenario that has at least one problem.
    ///
    /// # Errors
    ///
    /// The full [`ScenarioProblems`] list, never just the first entry.
    pub fn require_valid_custom(
        &self,
        request: &CustomScenarioRequest,
    ) -> Result<(), ScenarioProblems> {
        let problems = self.validate_custom(request);
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems)
        }
    }

    /// The roster problems: namespace, option-table, unknown-value, missing
    /// side and impossible-faction refusals.
    fn check_roster(&self, roster: &ScenarioRoster) -> Vec<ScenarioProblem> {
        let mut found = Vec::new();
        for actor in roster.actors() {
            match actor.airframe() {
                Resolved::Unknown { claim_id, reason } => found.push(ScenarioProblem::unknown(
                    ScenarioProblemCode::UnmeasuredActorField,
                    "roster",
                    claim_id.clone(),
                    reason,
                )),
                Resolved::Known(Known { value, .. }) => {
                    if !self.options.airframes().contains(value) {
                        found.push(ScenarioProblem::about(
                            ScenarioProblemCode::UnsupportedAirframe,
                            value.clone(),
                            format!(
                                "{} slot {} is not a plane this catalog offers",
                                actor.side(),
                                actor.slot()
                            ),
                        ));
                    }
                }
            }
            match actor.loadout() {
                Resolved::Unknown { claim_id, reason } => found.push(ScenarioProblem::unknown(
                    ScenarioProblemCode::UnmeasuredActorField,
                    "roster",
                    claim_id.clone(),
                    reason,
                )),
                Resolved::Known(Known { value, .. }) => {
                    if !self.options.loadouts().contains(value) {
                        found.push(ScenarioProblem::about(
                            ScenarioProblemCode::UnsupportedLoadout,
                            value.clone(),
                            format!(
                                "{} slot {} carries ordnance this catalog does not support",
                                actor.side(),
                                actor.slot()
                            ),
                        ));
                    }
                }
            }
            if let Resolved::Unknown { claim_id, reason } = actor.survivability() {
                found.push(ScenarioProblem::unknown(
                    ScenarioProblemCode::UnmeasuredSurvivability,
                    "roster",
                    claim_id.clone(),
                    reason,
                ));
            }
            if actor.faction().kind() != ContentKind::Faction {
                found.push(ScenarioProblem::about(
                    ScenarioProblemCode::FactionKindMismatch,
                    actor.faction().clone(),
                    format!(
                        "{} slot {} does not name a faction",
                        actor.side(),
                        actor.slot()
                    ),
                ));
            } else if !self.options.factions().contains(actor.faction()) {
                found.push(ScenarioProblem::about(
                    ScenarioProblemCode::ImpossibleFaction,
                    actor.faction().clone(),
                    format!(
                        "{} slot {} names a faction a custom scenario cannot spawn",
                        actor.side(),
                        actor.slot()
                    ),
                ));
            }
        }

        for side in [ScenarioSide::Player, ScenarioSide::Enemy] {
            if roster.actors_on(side).is_empty() {
                found.push(ScenarioProblem::new(
                    ScenarioProblemCode::MissingSide,
                    format!("the roster declares no {side} actor"),
                ));
            }
        }
        found.extend(self.check_hostility(roster));
        found
    }

    /// Refuses a roster whose declared relations make it impossible to fight:
    /// a player-side actor whose faction is declared friendly (or neutral)
    /// toward an enemy-side faction has nothing to shoot at, and two actors of
    /// the same faction on opposing sides cannot fight each other.
    ///
    /// A relation whose allegiance is `Resolved::Unknown` produces no problem
    /// here: it cannot prove impossibility, and guessing it would be a
    /// fabricated faction table. It refuses at the lowering boundary instead
    /// ([`ScenarioSchemaError::UnresolvedValue`]).
    fn check_hostility(&self, roster: &ScenarioRoster) -> Vec<ScenarioProblem> {
        let mut found = Vec::new();
        let player_side = if roster.actors_on(ScenarioSide::Player).is_empty() {
            ScenarioSide::Ally
        } else {
            ScenarioSide::Player
        };
        for ours in roster.actors_on(player_side) {
            for theirs in roster.actors_on(ScenarioSide::Enemy) {
                if ours.faction() == theirs.faction() {
                    found.push(ScenarioProblem::about(
                        ScenarioProblemCode::ImpossibleFaction,
                        theirs.faction().clone(),
                        format!(
                            "{} slot {} fights its own faction at {} slot {}",
                            ours.side(),
                            ours.slot(),
                            theirs.side(),
                            theirs.slot()
                        ),
                    ));
                    continue;
                }
                match self.options.relation(ours.faction(), theirs.faction()) {
                    Some(DeclaredRelation {
                        allegiance:
                            Resolved::Known(Known {
                                value: DeclaredAllegiance::Friendly,
                                ..
                            }),
                        ..
                    }) => found.push(ScenarioProblem::about(
                        ScenarioProblemCode::ImpossibleFaction,
                        theirs.faction().clone(),
                        format!(
                            "{} is declared friendly toward {}, so the scenario cannot be fought",
                            ours.faction(),
                            theirs.faction()
                        ),
                    )),
                    Some(DeclaredRelation {
                        allegiance:
                            Resolved::Known(Known {
                                value: DeclaredAllegiance::Neutral,
                                ..
                            }),
                        ..
                    }) => found.push(ScenarioProblem::about(
                        ScenarioProblemCode::ImpossibleFaction,
                        theirs.faction().clone(),
                        format!(
                            "{} and {} are declared neutral, so no side has a target",
                            ours.faction(),
                            theirs.faction()
                        ),
                    )),
                    _ => {}
                }
            }
        }
        found
    }

    /// Refuses a victory condition the declared roster can never satisfy.
    fn check_victory_feasibility(&self, parameters: &ScenarioParameters) -> Vec<ScenarioProblem> {
        let rules = parameters.rules();
        if !self
            .options
            .victory_conditions()
            .contains(&rules.condition())
        {
            return Vec::new();
        }
        let mut found = Vec::new();
        if rules.condition().needs_enemies()
            && parameters
                .roster()
                .actors_on(ScenarioSide::Enemy)
                .is_empty()
        {
            found.push(ScenarioProblem::new(
                ScenarioProblemCode::ConditionUnsatisfiable,
                format!(
                    "victory condition {} can never complete without an enemy actor",
                    rules.condition()
                ),
            ));
        }
        if rules.condition() == VictoryCondition::LastSideStanding
            && parameters.roster().actors_on(ScenarioSide::Ally).is_empty()
        {
            found.push(ScenarioProblem::new(
                ScenarioProblemCode::ConditionUnsatisfiable,
                format!(
                    "victory condition {} has only one side to stand on",
                    rules.condition()
                ),
            ));
        }
        found
    }
}

// ------------------------------------------------------------------ errors ---

/// The two preset ids that claimed one scenario id.
#[derive(Clone, Debug, PartialEq)]
pub struct DuplicateScenarioClaim {
    /// The shared scenario id.
    pub scenario: ContentId,
    /// The first preset that claimed it.
    pub first: ContentId,
    /// The second preset that claimed it.
    pub second: ContentId,
}

/// Why a declared Instant Action record was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum ScenarioSchemaError {
    /// The preset id is not in the `ia_preset` namespace.
    PresetKindMismatch {
        /// The offending id.
        id: ContentId,
    },
    /// The scenario id is not in the `ia_scenario` namespace.
    ScenarioKindMismatch {
        /// The offending id.
        id: ContentId,
    },
    /// The custom scenario's subject is not in the `ia_scenario` namespace.
    SubjectKindMismatch {
        /// The offending id.
        id: ContentId,
    },
    /// The roster was empty.
    EmptyRoster,
    /// Two actors share one `(side, slot)` pair.
    DuplicateSlot {
        /// The side of the duplicate.
        side: ScenarioSide,
        /// The duplicated slot.
        slot: RosterSlot,
    },
    /// The roster declared more than [`MAX_SCENARIO_ACTORS`] actors.
    TooManyActors {
        /// The declared count.
        count: usize,
        /// The accepted maximum.
        max: usize,
    },
    /// One side declared more than [`MAX_SCENARIO_ACTORS_PER_SIDE`] actors.
    TooManyActorsOnSide {
        /// The offending side.
        side: ScenarioSide,
        /// The declared count.
        count: usize,
        /// The accepted maximum.
        max: usize,
    },
    /// More than one `Player` slot was declared.
    MultiplePlayers {
        /// The accepted maximum.
        max: u8,
    },
    /// A faction reference is not in the `faction` namespace.
    FactionKindMismatch {
        /// The offending id.
        id: ContentId,
    },
    /// An airframe reference is not in the `airframe` namespace.
    AirframeKindMismatch {
        /// The offending id.
        id: ContentId,
    },
    /// A loadout reference is not in the `loadout` namespace.
    LoadoutKindMismatch {
        /// The offending id.
        id: ContentId,
    },
    /// A pilot reference is not in the `pilot` namespace.
    PilotKindMismatch {
        /// The offending id.
        id: ContentId,
    },
    /// The victory condition needs a deadline and none was declared.
    DeadlineRequired {
        /// The condition.
        condition: VictoryCondition,
    },
    /// A deadline was declared for a condition that cannot end on it.
    DeadlineNotApplicable {
        /// The condition.
        condition: VictoryCondition,
    },
    /// A `SurviveToDeadline` condition declared a zero deadline.
    ZeroDeadline {
        /// The condition.
        condition: VictoryCondition,
    },
    /// The preset title is not a usable name.
    TitleNotMeasured {
        /// Why the title was refused.
        reason: String,
    },
    /// The preset title is longer than [`MAX_PRESET_TITLE_BYTES`].
    TitleTooLong {
        /// Its length in bytes.
        len: usize,
        /// The accepted maximum.
        max: usize,
    },
    /// A draft was resolved with dimensions still unset.
    IncompleteDraft {
        /// The unset dimension names, in form order.
        missing: Vec<&'static str>,
    },
    /// A draft was resolved without provenance.
    MissingProvenance,
    /// The declared seat count is zero or above [`MAX_SCENARIO_PLAYERS`].
    InvalidPlayerCount {
        /// The declared count.
        players: u8,
        /// The accepted maximum.
        max: u8,
    },
    /// An option list was empty.
    EmptyOptionTable {
        /// Which list.
        what: &'static str,
    },
    /// An option id was in the wrong namespace.
    OptionKindMismatch {
        /// Which list.
        what: &'static str,
        /// The offending id.
        id: ContentId,
        /// The namespace the list requires.
        expected: ContentKind,
    },
    /// An option entry was held twice.
    DuplicateOption {
        /// Which list.
        what: &'static str,
        /// The duplicated entry, in the list's own identity form.
        entry: String,
    },
    /// A difficulty tier outside the closed vocabulary was declared.
    UnknownDifficultyTier {
        /// The offending tier.
        tier: DifficultyTier,
    },
    /// A victory condition outside the closed vocabulary was declared.
    UnknownVictoryCondition {
        /// The offending condition.
        condition: VictoryCondition,
    },
    /// The catalog declared no preset at all.
    NoPresets,
    /// Two presets share one `ia_preset` id.
    DuplicatePreset {
        /// The duplicated id.
        id: ContentId,
    },
    /// Two presets launch the same `ia_scenario` id.
    ///
    /// Boxed because it carries three ids and is otherwise the largest variant
    /// by a wide margin; an unboxed one would make every `Result` in this
    /// module carry its size (clippy's `result_large_err`).
    DuplicateScenario(Box<DuplicateScenarioClaim>),
    /// The catalog's seat limit is zero or above [`MAX_SCENARIO_PLAYERS`].
    InvalidPlayerLimit {
        /// The declared limit.
        max: u8,
        /// The accepted ceiling.
        ceiling: u8,
    },
    /// A `Resolved::Unknown` was assembled where a known value is required.
    UnresolvedValue {
        /// Which field.
        field: &'static str,
        /// The claim the unknown is recorded under.
        claim_id: ClaimId,
        /// Why it is unknown.
        reason: String,
    },
}

impl fmt::Display for ScenarioSchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PresetKindMismatch { id } => {
                write!(f, "preset id {id} is not in the ia_preset namespace")
            }
            Self::ScenarioKindMismatch { id } => {
                write!(f, "scenario id {id} is not in the ia_scenario namespace")
            }
            Self::SubjectKindMismatch { id } => {
                write!(
                    f,
                    "custom scenario subject {id} is not in the ia_scenario namespace"
                )
            }
            Self::EmptyRoster => f.write_str("a scenario roster declares no actor"),
            Self::DuplicateSlot { side, slot } => {
                write!(f, "{side} {slot} is declared more than once")
            }
            Self::TooManyActors { count, max } => {
                write!(
                    f,
                    "the roster declares {count} actors, above the {max} accepted"
                )
            }
            Self::TooManyActorsOnSide { side, count, max } => write!(
                f,
                "{side} declares {count} actors, above the {max} accepted on one side"
            ),
            Self::MultiplePlayers { max } => {
                write!(
                    f,
                    "a scenario declares more player slots than the {max} accepted"
                )
            }
            Self::FactionKindMismatch { id } => {
                write!(f, "roster faction {id} is not in the faction namespace")
            }
            Self::AirframeKindMismatch { id } => {
                write!(f, "roster airframe {id} is not in the airframe namespace")
            }
            Self::LoadoutKindMismatch { id } => {
                write!(f, "roster loadout {id} is not in the loadout namespace")
            }
            Self::PilotKindMismatch { id } => {
                write!(f, "roster pilot {id} is not in the pilot namespace")
            }
            Self::DeadlineRequired { condition } => {
                write!(f, "victory condition {condition} needs a deadline in ticks")
            }
            Self::DeadlineNotApplicable { condition } => write!(
                f,
                "victory condition {condition} cannot end on a deadline, so one was refused"
            ),
            Self::ZeroDeadline { condition } => {
                write!(f, "victory condition {condition} was given a zero deadline")
            }
            Self::TitleNotMeasured { reason } => {
                write!(f, "the preset title is unusable: {reason}")
            }
            Self::TitleTooLong { len, max } => {
                write!(
                    f,
                    "the preset title is {len} bytes, above the {max} accepted"
                )
            }
            Self::IncompleteDraft { missing } => write!(
                f,
                "the custom scenario draft leaves {} unset",
                missing.join(", ")
            ),
            Self::MissingProvenance => {
                f.write_str("a custom scenario request must name its provenance")
            }
            Self::InvalidPlayerCount { players, max } => write!(
                f,
                "{players} seats is outside the supported 1..={max} range"
            ),
            Self::EmptyOptionTable { what } => {
                write!(f, "the option table declares no {what}")
            }
            Self::OptionKindMismatch { what, id, expected } => write!(
                f,
                "option {id} in {what} is not in the {} namespace",
                expected.label()
            ),
            Self::DuplicateOption { what, entry } => {
                write!(f, "option {entry} is declared twice in {what}")
            }
            Self::UnknownDifficultyTier { tier } => {
                write!(f, "difficulty tier {tier} is outside the closed vocabulary")
            }
            Self::UnknownVictoryCondition { condition } => {
                write!(
                    f,
                    "victory condition {condition} is outside the closed vocabulary"
                )
            }
            Self::NoPresets => f.write_str("the catalog declares no Instant Action preset"),
            Self::DuplicatePreset { id } => {
                write!(f, "preset {id} is declared more than once")
            }
            Self::DuplicateScenario(claim) => write!(
                f,
                "scenario {} is launched by both preset {} and preset {}",
                claim.scenario, claim.first, claim.second
            ),
            Self::InvalidPlayerLimit { max, ceiling } => {
                write!(f, "the catalog's seat limit {max} is outside 1..={ceiling}")
            }
            Self::UnresolvedValue {
                field,
                claim_id,
                reason,
            } => write!(f, "{field} is unknown under {claim_id}: {reason}"),
        }
    }
}

impl std::error::Error for ScenarioSchemaError {}

/// Refuses a required known value, preserving the unknown's own claim.
///
/// This is the single place an `Resolved::Unknown` becomes a refusal with the
/// field that needs it, so the application boundary reports every unknown once
/// and with the same wording.
pub fn require_known<T: Clone>(
    field: &'static str,
    value: &Resolved<T>,
) -> Result<T, ScenarioSchemaError> {
    match value {
        Resolved::Known(Known { value, .. }) => Ok(value.clone()),
        Resolved::Unknown { claim_id, reason } => Err(ScenarioSchemaError::UnresolvedValue {
            field,
            claim_id: claim_id.clone(),
            reason: reason.clone(),
        }),
    }
}

/// Whether this record was authored for the original installation.
///
/// A convenience for the readiness reports: only
/// [`Origin::Installation`](cs_types::content::Origin::Installation) counts.
#[must_use]
pub fn is_original(origin: &Origin) -> bool {
    origin.is_original()
}

// ------------------------------------------------------- synthetic fixture ---

/// The claim id every synthetic fixture in this module is recorded under.
pub const SYNTHETIC_INSTANT_ACTION_CLAIM: &str = "f49a.synthetic-instant-action";

/// Synthetic world keys the fixture catalog offers.
pub const SYNTHETIC_IA_WORLD_COAST: &str = "synthetic.fixture_ia_coastal";
/// A second synthetic world, so "the world is part of the preset identity" is
/// testable rather than vacuous.
pub const SYNTHETIC_IA_WORLD_CITY: &str = "synthetic.fixture_ia_city";
/// Synthetic environment keys the fixture catalog offers.
pub const SYNTHETIC_IA_ENV_DAY_CLEAR: &str = "synthetic.fixture_ia_day_clear";
/// A second synthetic environment, with a distinct gameplay visibility.
pub const SYNTHETIC_IA_ENV_DUSK_HAZE: &str = "synthetic.fixture_ia_dusk_haze";

/// Synthetic airframe keys the fixture catalog offers.
pub const SYNTHETIC_IA_AIRFRAME_INTERCEPTOR: &str = "synthetic.fixture_ia_interceptor";
/// A heavy synthetic airframe, so a preset can select a different plane.
pub const SYNTHETIC_IA_AIRFRAME_HEAVY: &str = "synthetic.fixture_ia_heavy";

/// Synthetic loadout keys the fixture catalog offers.
pub const SYNTHETIC_IA_LOADOUT_LIGHT: &str = "synthetic.fixture_ia_light_guns";
/// A second synthetic loadout, so "the loadout affects the scenario" is
/// testable.
pub const SYNTHETIC_IA_LOADOUT_HEAVY: &str = "synthetic.fixture_ia_heavy_guns";
/// A synthetic loadout the fixture catalog deliberately does **not** offer, so
/// "unsupported ordnance" has something to refuse.
pub const SYNTHETIC_IA_LOADOUT_UNSUPPORTED: &str = "synthetic.fixture_ia_unlisted";

/// Synthetic faction keys the fixture catalog offers.
pub const SYNTHETIC_IA_FACTION_PLAYER_SIDE: &str = "synthetic.fixture_ia_player_side";
/// The fixture's opposing faction.
pub const SYNTHETIC_IA_FACTION_OPPOSITION: &str = "synthetic.fixture_ia_opposition";
/// A synthetic faction deliberately **not** offered by the fixture catalog.
pub const SYNTHETIC_IA_FACTION_UNLISTED: &str = "synthetic.fixture_ia_unlisted";

/// The root seed of the first synthetic preset.
pub const SYNTHETIC_IA_SEED_DOGFIGHT: u64 = 0x00D0_0F1A_0000_0001;
/// The root seed of the second synthetic preset.
pub const SYNTHETIC_IA_SEED_BOMBER_RUN: u64 = 0x00B0_0BE2_0000_0002;

/// The number of players the fixture catalog allows a custom scenario to seat.
pub const SYNTHETIC_IA_MAX_PLAYERS: u8 = 4;

/// A fixture [`Provenance`] for a synthetic Instant Action record.
#[must_use]
pub fn synthetic_provenance() -> Provenance {
    Provenance::designed(
        ClaimId::new(SYNTHETIC_INSTANT_ACTION_CLAIM).expect("the fixture claim id is valid"),
    )
}

/// A `Resolved::Known` value carrying the synthetic fixture provenance.
fn fixture_known<T: Clone>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, synthetic_provenance()))
}

/// A `Resolved::Unknown` recorded as the still-open measurement it is.
fn fixture_unknown<T>(reason: &str) -> Resolved<T> {
    match Resolved::<T>::unknown(
        ClaimId::new(SYNTHETIC_INSTANT_ACTION_CLAIM).expect("the fixture claim id is valid"),
        reason,
    ) {
        Ok(value) => value,
        // Only an empty reason can fail, and `reason` is a fixture literal.
        Err(_) => unreachable!("the fixture unknown carries a reason"),
    }
}

/// A synthetic `ContentId` in the given namespace.
fn fixture_id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("the fixture content id is valid")
}

/// A synthetic world id.
fn fixture_world(key: &str) -> WorldId {
    WorldId::from_key(key).expect("the fixture world key is valid")
}

/// A synthetic environment id.
fn fixture_environment(key: &str) -> EnvironmentId {
    EnvironmentId::new(key).expect("the fixture environment key is valid")
}

/// A synthetic declared relation between two fixture factions.
fn fixture_relation(
    from: &ContentId,
    to: &ContentId,
    allegiance: DeclaredAllegiance,
) -> DeclaredRelation {
    DeclaredRelation {
        from: from.clone(),
        to: to.clone(),
        allegiance: fixture_known(allegiance),
    }
}

/// A synthetic faction id.
fn fixture_faction(key: &str) -> ContentId {
    fixture_id(ContentKind::Faction, key)
}

/// The player-side synthetic faction.
pub fn synthetic_player_faction() -> ContentId {
    fixture_faction(SYNTHETIC_IA_FACTION_PLAYER_SIDE)
}

/// The opposing synthetic faction.
pub fn synthetic_opposition_faction() -> ContentId {
    fixture_faction(SYNTHETIC_IA_FACTION_OPPOSITION)
}

/// A synthetic [`ScenarioActorSpec`] on the given side.
///
/// The pilot is always the fixture pilot and the survivability always mortal:
/// an actor's *pilot identity* is F33-A's field and this fixture does not claim
/// an original pilot assignment.
pub fn synthetic_actor(
    side: ScenarioSide,
    slot: u32,
    airframe_key: &str,
    loadout_key: &str,
) -> ScenarioActorSpec {
    ScenarioActorSpec::try_new(
        side,
        RosterSlot(slot),
        if side == ScenarioSide::Enemy {
            synthetic_opposition_faction()
        } else {
            synthetic_player_faction()
        },
        fixture_known(fixture_id(ContentKind::Airframe, airframe_key)),
        fixture_known(fixture_id(ContentKind::Loadout, loadout_key)),
        Some(fixture_id(ContentKind::Pilot, "synthetic.fixture_pilot")),
        fixture_known(DeclaredSurvivability::Mortal),
        synthetic_provenance(),
    )
    .expect("the synthetic actor fixture is valid")
}

/// The synthetic supported-dimension table of the fixture catalog.
///
/// Three presets' worth of worlds, environments, planes and loadouts, plus one
/// unlisted loadout and one unlisted faction so the "unsupported selection"
/// refusals have something real to refuse.
#[must_use]
pub fn synthetic_scenario_options() -> ScenarioOptions {
    let player_side = synthetic_player_faction();
    let opposition = synthetic_opposition_faction();
    let traders = fixture_faction("synthetic.fixture_ia_traders");
    ScenarioOptions::try_new(
        vec![
            fixture_world(SYNTHETIC_IA_WORLD_COAST),
            fixture_world(SYNTHETIC_IA_WORLD_CITY),
        ],
        vec![
            fixture_environment(SYNTHETIC_IA_ENV_DAY_CLEAR),
            fixture_environment(SYNTHETIC_IA_ENV_DUSK_HAZE),
        ],
        vec![player_side.clone(), opposition.clone(), traders.clone()],
        vec![
            fixture_id(ContentKind::Airframe, SYNTHETIC_IA_AIRFRAME_INTERCEPTOR),
            fixture_id(ContentKind::Airframe, SYNTHETIC_IA_AIRFRAME_HEAVY),
        ],
        vec![
            fixture_id(ContentKind::Loadout, SYNTHETIC_IA_LOADOUT_LIGHT),
            fixture_id(ContentKind::Loadout, SYNTHETIC_IA_LOADOUT_HEAVY),
        ],
        vec![DifficultyTier::Standard, DifficultyTier::Hard],
        vec![
            VictoryCondition::EliminateEnemies,
            VictoryCondition::LastSideStanding,
            VictoryCondition::SurviveToDeadline,
        ],
        vec![
            fixture_relation(&player_side, &opposition, DeclaredAllegiance::Hostile),
            fixture_relation(&opposition, &player_side, DeclaredAllegiance::Hostile),
            fixture_relation(&player_side, &traders, DeclaredAllegiance::Neutral),
            fixture_relation(&traders, &player_side, DeclaredAllegiance::Neutral),
            fixture_relation(&opposition, &traders, DeclaredAllegiance::Hostile),
            fixture_relation(&traders, &opposition, DeclaredAllegiance::Hostile),
        ],
        SYNTHETIC_IA_MAX_PLAYERS,
    )
    .expect("the synthetic scenario option table is valid")
}

/// A synthetic [`DifficultyProfile`] at the given tier.
pub fn synthetic_difficulty(tier: DifficultyTier) -> DifficultyProfile {
    DifficultyProfile::try_new(tier, Vec::new(), synthetic_provenance())
        .expect("a synthetic difficulty profile with no overrides is valid")
}

/// The synthetic dogfight preset: one player and one ally against two
/// enemies, coastal world, clear day, eliminate-enemies.
#[must_use]
pub fn synthetic_dogfight_preset() -> InstantActionPreset {
    synthetic_preset(
        "synthetic.fixture_ia_dogfight",
        "synthetic.fixture_ia_scenario_dogfight",
        Some("synthetic fixture dogfight"),
        SYNTHETIC_IA_WORLD_COAST,
        SYNTHETIC_IA_ENV_DAY_CLEAR,
        vec![
            synthetic_actor(
                ScenarioSide::Player,
                0,
                SYNTHETIC_IA_AIRFRAME_INTERCEPTOR,
                SYNTHETIC_IA_LOADOUT_LIGHT,
            ),
            synthetic_actor(
                ScenarioSide::Ally,
                0,
                SYNTHETIC_IA_AIRFRAME_INTERCEPTOR,
                SYNTHETIC_IA_LOADOUT_LIGHT,
            ),
            synthetic_actor(
                ScenarioSide::Enemy,
                0,
                SYNTHETIC_IA_AIRFRAME_INTERCEPTOR,
                SYNTHETIC_IA_LOADOUT_LIGHT,
            ),
            synthetic_actor(
                ScenarioSide::Enemy,
                1,
                SYNTHETIC_IA_AIRFRAME_INTERCEPTOR,
                SYNTHETIC_IA_LOADOUT_LIGHT,
            ),
        ],
        DifficultyTier::Standard,
        VictoryCondition::EliminateEnemies,
        RespawnBudget::None,
        TieOutcome::Draw,
        SYNTHETIC_IA_SEED_DOGFIGHT,
    )
}

/// The synthetic bomber-run preset: one heavy player aircraft against three
/// heavy enemies, city world, dusk haze, survive-to-deadline with a deadline.
#[must_use]
pub fn synthetic_bomber_run_preset() -> InstantActionPreset {
    synthetic_preset(
        "synthetic.fixture_ia_bomber_run",
        "synthetic.fixture_ia_scenario_bomber_run",
        None,
        SYNTHETIC_IA_WORLD_CITY,
        SYNTHETIC_IA_ENV_DUSK_HAZE,
        vec![
            synthetic_actor(
                ScenarioSide::Player,
                0,
                SYNTHETIC_IA_AIRFRAME_HEAVY,
                SYNTHETIC_IA_LOADOUT_HEAVY,
            ),
            synthetic_actor(
                ScenarioSide::Enemy,
                0,
                SYNTHETIC_IA_AIRFRAME_HEAVY,
                SYNTHETIC_IA_LOADOUT_HEAVY,
            ),
            synthetic_actor(
                ScenarioSide::Enemy,
                1,
                SYNTHETIC_IA_AIRFRAME_HEAVY,
                SYNTHETIC_IA_LOADOUT_HEAVY,
            ),
            synthetic_actor(
                ScenarioSide::Enemy,
                2,
                SYNTHETIC_IA_AIRFRAME_HEAVY,
                SYNTHETIC_IA_LOADOUT_HEAVY,
            ),
        ],
        DifficultyTier::Hard,
        VictoryCondition::SurviveToDeadline,
        RespawnBudget::PerSide { per_side: 1 },
        TieOutcome::PlayerFavour,
        SYNTHETIC_IA_SEED_BOMBER_RUN,
    )
}

/// Assembles a synthetic preset from its parts.
#[allow(clippy::too_many_arguments)]
fn synthetic_preset(
    preset_key: &str,
    scenario_key: &str,
    title: Option<&str>,
    world_key: &str,
    environment_key: &str,
    roster: Vec<ScenarioActorSpec>,
    tier: DifficultyTier,
    condition: VictoryCondition,
    respawns: RespawnBudget,
    tie: TieOutcome,
    seed: u64,
) -> InstantActionPreset {
    let deadline = if condition.needs_deadline() {
        Some(3_600)
    } else {
        None
    };
    let rules = VictoryRules::try_new(condition, respawns, deadline, tie)
        .expect("the synthetic preset's rules are valid");
    let parameters = ScenarioParameters::new(
        fixture_known(fixture_world(world_key)),
        fixture_known(fixture_environment(environment_key)),
        ScenarioRoster::try_new(roster).expect("the synthetic preset's roster is valid"),
        synthetic_difficulty(tier),
        rules,
        ScenarioSeed::new(seed),
    );
    InstantActionPreset::try_new(
        fixture_id(ContentKind::IaPreset, preset_key),
        fixture_id(ContentKind::IaScenario, scenario_key),
        match title {
            Some(text) => fixture_known(text.to_owned()),
            // A second preset deliberately carries no measured title, so
            // "an unmeasured original name stays unknown" is exercised.
            None => fixture_unknown("the original preset's display name is unmeasured"),
        },
        parameters,
        Origin::SyntheticFixture,
        synthetic_provenance(),
    )
    .expect("the synthetic preset fixture is valid")
}

/// The synthetic [`InstantActionCatalog`]: the two presets above plus the
/// supported option table.
///
/// **Every row is synthetic.** The original 2000 PC installation's Instant
/// Action preset list, their parameters and the supported custom-scenario
/// dimensions are unmeasured; F49-D verifies the real catalog. This fixture
/// exists so the schema, the validators and the lowering boundary have a
/// production-path input, not so a catalog claim can be made.
#[must_use]
pub fn synthetic_instant_action_catalog() -> InstantActionCatalog {
    InstantActionCatalog::try_new(
        vec![synthetic_dogfight_preset(), synthetic_bomber_run_preset()],
        synthetic_scenario_options(),
        synthetic_provenance(),
    )
    .expect("the synthetic Instant Action catalog fixture is valid")
}

/// A complete, valid synthetic custom scenario built from the fixture
/// dimensions.
///
/// This is the request AC02's "change one roster slot" starts from: it is
/// valid, so a later stage's normalization has a baseline that passes
/// validation unchanged.
#[must_use]
pub fn synthetic_custom_request() -> CustomScenarioRequest {
    let rules = VictoryRules::try_new(
        VictoryCondition::LastSideStanding,
        RespawnBudget::None,
        None,
        TieOutcome::Draw,
    )
    .expect("the synthetic custom request's rules are valid");
    CustomScenarioDraft::new()
        .with_subject(fixture_id(
            ContentKind::IaScenario,
            "synthetic.fixture_ia_scenario_custom",
        ))
        .with_world(fixture_known(fixture_world(SYNTHETIC_IA_WORLD_CITY)))
        .with_environment(fixture_known(fixture_environment(
            SYNTHETIC_IA_ENV_DAY_CLEAR,
        )))
        .with_roster(vec![
            synthetic_actor(
                ScenarioSide::Player,
                0,
                SYNTHETIC_IA_AIRFRAME_INTERCEPTOR,
                SYNTHETIC_IA_LOADOUT_LIGHT,
            ),
            synthetic_actor(
                ScenarioSide::Enemy,
                0,
                SYNTHETIC_IA_AIRFRAME_INTERCEPTOR,
                SYNTHETIC_IA_LOADOUT_LIGHT,
            ),
            synthetic_actor(
                ScenarioSide::Ally,
                0,
                SYNTHETIC_IA_AIRFRAME_HEAVY,
                SYNTHETIC_IA_LOADOUT_HEAVY,
            ),
        ])
        .with_difficulty(synthetic_difficulty(DifficultyTier::Standard))
        .with_rules(rules)
        .with_seed(ScenarioSeed::new(SYNTHETIC_IA_SEED_DOGFIGHT ^ 0x55))
        .with_players(1)
        .with_provenance(synthetic_provenance())
        .resolve()
        .expect("the synthetic custom request fixture is valid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pilots::DeclaredSurvivability;

    fn id(kind: ContentKind, key: &str) -> ContentId {
        ContentId::from_source(kind, key).expect("fixture id is valid")
    }

    fn known<T: Clone>(value: T) -> Resolved<T> {
        Resolved::Known(Known::new(
            value,
            Provenance::designed(ClaimId::new("f49a.fixture").expect("fixture claim is valid")),
        ))
    }

    fn provenance() -> Provenance {
        Provenance::designed(ClaimId::new("f49a.fixture").expect("fixture claim is valid"))
    }

    /// A structurally valid custom request over an arbitrary roster, so a test
    /// can vary one dimension and keep everything else fixed.
    fn custom_request_with_roster(roster: &[ScenarioActorSpec]) -> CustomScenarioRequest {
        let rules = VictoryRules::try_new(
            VictoryCondition::LastSideStanding,
            RespawnBudget::None,
            None,
            TieOutcome::Draw,
        )
        .expect("the fixture rules are valid");
        CustomScenarioDraft::new()
            .with_subject(id(
                ContentKind::IaScenario,
                "synthetic.fixture_ia_scenario_probe",
            ))
            .with_world(fixture_known(fixture_world(SYNTHETIC_IA_WORLD_COAST)))
            .with_environment(fixture_known(fixture_environment(
                SYNTHETIC_IA_ENV_DAY_CLEAR,
            )))
            .with_roster(roster.to_vec())
            .with_difficulty(synthetic_difficulty(DifficultyTier::Standard))
            .with_rules(rules)
            .with_seed(ScenarioSeed::new(11))
            .with_players(1)
            .with_provenance(synthetic_provenance())
            .resolve()
            .expect("the probe request is structurally valid")
    }

    fn actor_with_unmeasured_airframe(
        side: ScenarioSide,
        slot: u32,
        reason: &str,
    ) -> ScenarioActorSpec {
        ScenarioActorSpec::try_new(
            side,
            RosterSlot(slot),
            synthetic_player_faction(),
            fixture_unknown(reason),
            fixture_known(fixture_id(ContentKind::Loadout, SYNTHETIC_IA_LOADOUT_LIGHT)),
            None,
            fixture_known(DeclaredSurvivability::Mortal),
            synthetic_provenance(),
        )
        .expect("an unmeasured airframe is carried, not refused at construction")
    }

    fn actor(side: ScenarioSide, slot: u32, airframe: &str, loadout: &str) -> ScenarioActorSpec {
        ScenarioActorSpec::try_new(
            side,
            RosterSlot(slot),
            id(
                ContentKind::Faction,
                if side == ScenarioSide::Enemy {
                    "fixture.opposition"
                } else {
                    "fixture.player_side"
                },
            ),
            known(id(ContentKind::Airframe, airframe)),
            known(id(ContentKind::Loadout, loadout)),
            Some(id(ContentKind::Pilot, "fixture.pilot")),
            known(DeclaredSurvivability::Mortal),
            provenance(),
        )
        .expect("fixture actor is valid")
    }

    #[test]
    fn accept_f49_a_sides_are_a_closed_vocabulary_with_one_hostile_pair() {
        assert_eq!(ScenarioSide::ALL.len(), 4);
        assert!(ScenarioSide::Player.is_hostile_to(ScenarioSide::Enemy));
        assert!(ScenarioSide::Ally.is_hostile_to(ScenarioSide::Enemy));
        assert!(!ScenarioSide::Player.is_hostile_to(ScenarioSide::Ally));
        assert!(!ScenarioSide::Neutral.is_hostile_to(ScenarioSide::Enemy));
        // Every side is its own label: no two sides collapse into one label.
        let mut labels: Vec<&str> = ScenarioSide::ALL.iter().map(|s| s.label()).collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), ScenarioSide::ALL.len());
    }

    #[test]
    fn accept_f49_a_roster_is_ordered_by_side_and_slot_and_refuses_duplicates() {
        let roster = ScenarioRoster::try_new(vec![
            actor(ScenarioSide::Enemy, 1, "fixture.enemy_b", "fixture.loadout"),
            actor(ScenarioSide::Player, 0, "fixture.player", "fixture.loadout"),
            actor(ScenarioSide::Enemy, 0, "fixture.enemy_a", "fixture.loadout"),
        ])
        .expect("the roster builds");
        let order: Vec<(ScenarioSide, u32)> = roster
            .actors()
            .iter()
            .map(|a| (a.side(), a.slot().index()))
            .collect();
        assert_eq!(
            order,
            vec![
                (ScenarioSide::Player, 0),
                (ScenarioSide::Enemy, 0),
                (ScenarioSide::Enemy, 1),
            ],
            "a roster must have exactly one ordering regardless of input order"
        );

        let duplicate = ScenarioRoster::try_new(vec![
            actor(ScenarioSide::Player, 0, "fixture.player", "fixture.loadout"),
            actor(ScenarioSide::Player, 0, "fixture.player", "fixture.loadout"),
        ])
        .expect_err("a duplicated (side, slot) is refused");
        assert_eq!(
            duplicate,
            ScenarioSchemaError::DuplicateSlot {
                side: ScenarioSide::Player,
                slot: RosterSlot(0),
            }
        );
    }

    #[test]
    fn accept_f49_a_an_empty_roster_is_refused_rather_than_defaulted() {
        let error = ScenarioRoster::try_new(Vec::new()).expect_err("an empty roster is refused");
        assert_eq!(error, ScenarioSchemaError::EmptyRoster);
    }

    #[test]
    fn accept_f49_a_a_deadline_is_required_only_by_the_condition_that_uses_it() {
        assert_eq!(
            VictoryRules::try_new(
                VictoryCondition::SurviveToDeadline,
                RespawnBudget::None,
                None,
                TieOutcome::Draw
            )
            .expect_err("survive-to-deadline needs a deadline"),
            ScenarioSchemaError::DeadlineRequired {
                condition: VictoryCondition::SurviveToDeadline
            }
        );
        assert_eq!(
            VictoryRules::try_new(
                VictoryCondition::EliminateEnemies,
                RespawnBudget::None,
                Some(100),
                TieOutcome::Draw
            )
            .expect_err("eliminate-enemies cannot end on a deadline"),
            ScenarioSchemaError::DeadlineNotApplicable {
                condition: VictoryCondition::EliminateEnemies
            }
        );
        assert!(
            VictoryRules::try_new(
                VictoryCondition::SurviveToDeadline,
                RespawnBudget::None,
                Some(1),
                TieOutcome::Draw
            )
            .is_ok()
        );
    }

    #[test]
    fn accept_f49_a_the_scenario_seed_is_explicit_and_domain_separated() {
        let seed = ScenarioSeed::new(0x0123_4567_89AB_CDEF);
        assert_eq!(seed.root(), 0x0123_4567_89AB_CDEF);
        assert_eq!(
            seed.stream(SCENARIO_SEED_DOMAIN),
            seed.stream(SCENARIO_SEED_DOMAIN)
        );
        assert_ne!(
            seed.stream(SCENARIO_SEED_DOMAIN),
            seed.stream(SCENARIO_SEED_DOMAIN ^ 1),
            "two domains must produce independent streams"
        );
    }

    /// AC01 minimum scenario at the schema level: every synthetic preset
    /// carries a distinct world, roster, rule set and seed, and the catalog
    /// resolves a preset by both its preset id and its scenario id.
    ///
    /// This is the input half of the F49-A minimum scenario. The spawn half
    /// lives in the acceptance test, which lowers each preset through the
    /// application boundary and inspects the actors, world and rules the
    /// simulation received.
    #[test]
    fn accept_f49_a_every_preset_declares_its_own_world_roster_rules_and_seed() {
        let catalog = synthetic_instant_action_catalog();
        let mut seen_worlds: Vec<String> = Vec::new();
        let mut seen_seeds: Vec<u64> = Vec::new();

        for preset in catalog.presets() {
            let parameters = preset.parameters();

            let world = require_known("preset world", parameters.world())
                .expect("a synthetic preset's world is known");
            assert!(
                catalog.options().worlds().contains(&world),
                "preset {} names a world the catalog does not offer",
                preset.id()
            );
            let environment = require_known("preset environment", parameters.environment())
                .expect("a synthetic preset's environment is known");
            assert!(catalog.options().environments().contains(&environment));

            // Every actor's plane and loadout must be selectable, or the
            // preset could not have come from this catalog.
            for actor in parameters.roster().actors() {
                let airframe = require_known("actor airframe", actor.airframe())
                    .expect("a synthetic actor's airframe is known");
                assert!(catalog.options().airframes().contains(&airframe));
                let loadout = require_known("actor loadout", actor.loadout())
                    .expect("a synthetic actor's loadout is known");
                assert!(catalog.options().loadouts().contains(&loadout));
            }

            // A condition that needs enemies has them. The converse is not
            // required: a survive-to-deadline scenario may still have enemies.
            assert!(
                !parameters.rules().condition().needs_enemies()
                    || !parameters
                        .roster()
                        .actors_on(ScenarioSide::Enemy)
                        .is_empty(),
                "preset {} declares a condition its roster cannot satisfy",
                preset.id()
            );
            // A deadline is present exactly when the condition ends on one.
            assert_eq!(
                parameters.rules().condition().needs_deadline(),
                parameters.rules().deadline_ticks().is_some()
            );

            // Preset identity resolves both ways.
            assert_eq!(catalog.preset(preset.id()), Some(preset));
            assert_eq!(catalog.preset_for_scenario(preset.scenario()), Some(preset));

            seen_worlds.push(world.to_string());
            seen_seeds.push(parameters.seed().root());
        }

        assert_eq!(seen_worlds.len(), catalog.presets().len());
        seen_worlds.sort();
        seen_worlds.dedup();
        assert_eq!(
            seen_worlds.len(),
            catalog.presets().len(),
            "two presets sharing a world would make the world unobservable"
        );
        seen_seeds.sort_unstable();
        seen_seeds.dedup();
        assert_eq!(
            seen_seeds.len(),
            catalog.presets().len(),
            "two presets sharing a seed would make the seed unobservable"
        );
    }

    /// An unmeasured original preset name stays an explicit unknown instead of
    /// becoming a label the UI would show as original.
    #[test]
    fn accept_f49_a_an_unmeasured_preset_name_stays_unknown() {
        let named = synthetic_dogfight_preset();
        let unnamed = synthetic_bomber_run_preset();
        assert!(
            named.title().is_known(),
            "the fixture names the first preset"
        );
        assert!(
            !unnamed.title().is_known(),
            "the second preset deliberately has no measured name"
        );
        // Neither is claimed to be original data.
        assert!(!named.origin().is_original());
        assert!(!synthetic_instant_action_catalog().is_original_complete());
    }

    /// The catalog refuses a duplicate preset identity and a duplicate scenario
    /// identity: two presets launching the same scenario would make
    /// "which authored scenario did I launch" unanswerable.
    #[test]
    fn accept_f49_a_the_catalog_refuses_duplicate_preset_and_scenario_identity() {
        let duplicate_preset = InstantActionCatalog::try_new(
            vec![synthetic_dogfight_preset(), synthetic_dogfight_preset()],
            synthetic_scenario_options(),
            synthetic_provenance(),
        )
        .expect_err("the same preset id twice is refused");
        assert_eq!(
            duplicate_preset,
            ScenarioSchemaError::DuplicatePreset {
                id: fixture_id(ContentKind::IaPreset, "synthetic.fixture_ia_dogfight"),
            }
        );

        let second = synthetic_preset(
            "synthetic.fixture_ia_dogfight_alias",
            "synthetic.fixture_ia_scenario_dogfight",
            None,
            SYNTHETIC_IA_WORLD_COAST,
            SYNTHETIC_IA_ENV_DAY_CLEAR,
            vec![
                synthetic_actor(
                    ScenarioSide::Player,
                    0,
                    SYNTHETIC_IA_AIRFRAME_INTERCEPTOR,
                    SYNTHETIC_IA_LOADOUT_LIGHT,
                ),
                synthetic_actor(
                    ScenarioSide::Enemy,
                    0,
                    SYNTHETIC_IA_AIRFRAME_INTERCEPTOR,
                    SYNTHETIC_IA_LOADOUT_LIGHT,
                ),
            ],
            DifficultyTier::Standard,
            VictoryCondition::EliminateEnemies,
            RespawnBudget::None,
            TieOutcome::Draw,
            SYNTHETIC_IA_SEED_DOGFIGHT ^ 0xFF,
        );
        let duplicate_scenario = InstantActionCatalog::try_new(
            vec![synthetic_dogfight_preset(), second],
            synthetic_scenario_options(),
            synthetic_provenance(),
        )
        .expect_err("two presets launching one scenario is refused");
        assert_eq!(
            duplicate_scenario,
            ScenarioSchemaError::DuplicateScenario(Box::new(DuplicateScenarioClaim {
                scenario: fixture_id(
                    ContentKind::IaScenario,
                    "synthetic.fixture_ia_scenario_dogfight"
                ),
                first: fixture_id(ContentKind::IaPreset, "synthetic.fixture_ia_dogfight"),
                second: fixture_id(ContentKind::IaPreset, "synthetic.fixture_ia_dogfight_alias"),
            }))
        );
    }

    /// The option table is a closed list: an empty list, a wrong-namespace id
    /// and a duplicate are all refused, and the seat limit is bounded.
    #[test]
    fn accept_f49_a_the_option_table_is_a_closed_bounded_list() {
        let factions = vec![synthetic_player_faction()];
        let airframes = vec![fixture_id(
            ContentKind::Airframe,
            SYNTHETIC_IA_AIRFRAME_INTERCEPTOR,
        )];
        let loadouts = vec![fixture_id(ContentKind::Loadout, SYNTHETIC_IA_LOADOUT_LIGHT)];
        let worlds = vec![fixture_world(SYNTHETIC_IA_WORLD_COAST)];
        let environments = vec![fixture_environment(SYNTHETIC_IA_ENV_DAY_CLEAR)];

        let table = |worlds: Vec<WorldId>, max_players: u8, factions: Vec<ContentId>| {
            ScenarioOptions::try_new(
                worlds,
                environments.clone(),
                factions,
                airframes.clone(),
                loadouts.clone(),
                vec![DifficultyTier::Standard],
                vec![VictoryCondition::EliminateEnemies],
                Vec::new(),
                max_players,
            )
        };

        assert_eq!(
            table(Vec::new(), 1, factions.clone()).expect_err("no worlds is refused"),
            ScenarioSchemaError::EmptyOptionTable { what: "worlds" }
        );
        assert_eq!(
            table(worlds.clone(), 0, factions.clone()).expect_err("a zero seat limit is refused"),
            ScenarioSchemaError::InvalidPlayerLimit {
                max: 0,
                ceiling: MAX_SCENARIO_PLAYERS,
            }
        );
        assert_eq!(
            table(worlds.clone(), MAX_SCENARIO_PLAYERS + 1, factions.clone())
                .expect_err("an unbounded seat limit is refused"),
            ScenarioSchemaError::InvalidPlayerLimit {
                max: MAX_SCENARIO_PLAYERS + 1,
                ceiling: MAX_SCENARIO_PLAYERS,
            }
        );
        assert_eq!(
            table(
                worlds.clone(),
                1,
                vec![synthetic_player_faction(), synthetic_player_faction()]
            )
            .expect_err("a duplicated faction is refused"),
            ScenarioSchemaError::DuplicateOption {
                what: "factions",
                entry: synthetic_player_faction().as_str().to_owned(),
            }
        );
        assert!(matches!(
            table(
                worlds,
                1,
                vec![fixture_id(
                    ContentKind::Airframe,
                    "synthetic.wrong_namespace"
                )]
            )
            .expect_err("a faction list holding a plane is refused"),
            ScenarioSchemaError::OptionKindMismatch {
                what: "factions",
                ..
            }
        ));
    }

    /// AC04: every refusal is reported at once, with its dimension and the
    /// offending selection, so a screen can render what to fix. An empty
    /// roster is refused rather than launched.
    #[test]
    fn accept_f49_a_an_invalid_custom_scenario_reports_every_problem_with_its_dimension() {
        let catalog = synthetic_instant_action_catalog();

        let rules = VictoryRules::try_new(
            VictoryCondition::LastSideStanding,
            RespawnBudget::None,
            None,
            TieOutcome::Draw,
        )
        .expect("the fixture rules are valid");

        let request = CustomScenarioDraft::new()
            .with_subject(fixture_id(
                ContentKind::IaScenario,
                "synthetic.fixture_ia_scenario_broken",
            ))
            // A world the catalog does not offer.
            .with_world(fixture_known(fixture_world(
                "synthetic.fixture_ia_absent_world",
            )))
            // An environment the catalog does not offer.
            .with_environment(fixture_known(fixture_environment(
                "synthetic.fixture_ia_absent_env",
            )))
            .with_roster(vec![
                // An unknown plane.
                actor_with_unmeasured_airframe(
                    ScenarioSide::Player,
                    0,
                    "the original roster assignment is unmeasured",
                ),
                // A plane the catalog does not offer, a loadout it does not
                // offer, and a faction it cannot spawn.
                ScenarioActorSpec::try_new(
                    ScenarioSide::Enemy,
                    RosterSlot(0),
                    fixture_faction(SYNTHETIC_IA_FACTION_UNLISTED),
                    fixture_known(fixture_id(
                        ContentKind::Airframe,
                        SYNTHETIC_IA_AIRFRAME_INTERCEPTOR,
                    )),
                    fixture_known(fixture_id(
                        ContentKind::Loadout,
                        SYNTHETIC_IA_LOADOUT_UNSUPPORTED,
                    )),
                    None,
                    fixture_known(DeclaredSurvivability::Mortal),
                    synthetic_provenance(),
                )
                .expect("the fixture actor is structurally valid"),
            ])
            .with_difficulty(synthetic_difficulty(DifficultyTier::Elite))
            .with_rules(rules)
            .with_seed(ScenarioSeed::new(7))
            .with_players(catalog.options().max_players() + 1)
            .with_provenance(synthetic_provenance())
            .resolve()
            .expect("a structurally complete draft resolves");

        let problems = catalog.validate_custom(&request);
        assert!(
            !problems.is_empty(),
            "an invalid custom scenario must be refused"
        );

        let codes: Vec<&str> = problems
            .problems()
            .iter()
            .map(|problem| problem.code.code())
            .collect();
        for expected in [
            "unsupported_world",
            "unsupported_environment",
            "unsupported_difficulty",
            "invalid_player_count",
            "unmeasured_actor_field",
            "unsupported_loadout",
            "impossible_faction",
            "condition_unsatisfiable",
        ] {
            assert!(
                codes.contains(&expected),
                "expected problem {expected}, got {problems}"
            );
        }

        // Every problem is actionable: it names its dimension and states what
        // to change. A problem about a *selection* names the selection to
        // replace; a problem about a *count or condition* states the accepted
        // range or the missing thing, which is what the player has to change.
        for problem in problems.problems() {
            assert!(
                !problem.dimension().is_empty(),
                "{problem} has no dimension"
            );
            assert!(!problem.detail().is_empty(), "{problem} has no detail");
            assert!(
                problem.offender().is_some()
                    || problem.claim_id().is_some()
                    || problem.detail().contains("range")
                    || problem.detail().contains("is not offered")
                    || problem.detail().contains("never complete")
                    || problem.detail().contains("only one side"),
                "{problem} must name what to change or which claim is open"
            );
        }
        // Every selection-shaped problem does name its selection.
        for problem in problems.problems().iter().filter(|problem| {
            matches!(
                problem.code(),
                ScenarioProblemCode::UnsupportedWorld
                    | ScenarioProblemCode::UnsupportedEnvironment
                    | ScenarioProblemCode::UnsupportedAirframe
                    | ScenarioProblemCode::UnsupportedLoadout
                    | ScenarioProblemCode::ImpossibleFaction
            )
        }) {
            assert!(
                problem.offender().is_some(),
                "{problem} must name the selection to replace"
            );
        }

        // An unmeasured value keeps its claim and reason verbatim.
        let unmeasured = problems.with_code(ScenarioProblemCode::UnmeasuredActorField);
        assert_eq!(unmeasured.len(), 1);
        assert_eq!(
            unmeasured[0].detail(),
            "the original roster assignment is unmeasured"
        );
        assert!(unmeasured[0].claim_id().is_some());

        // LastSideStanding with no ally has only one side to stand on.
        assert!(
            problems
                .with_code(ScenarioProblemCode::ConditionUnsatisfiable)
                .iter()
                .any(|problem| problem.detail().contains("only one side"))
        );

        // A roster with no enemy actor at all names the missing side, and the
        // roster-level refusal is independent of which side is absent.
        let no_enemies = custom_request_with_roster(&[synthetic_actor(
            ScenarioSide::Player,
            0,
            SYNTHETIC_IA_AIRFRAME_INTERCEPTOR,
            SYNTHETIC_IA_LOADOUT_LIGHT,
        )]);
        let enemy_problems = catalog.validate_custom(&no_enemies);
        assert!(
            enemy_problems
                .with_code(ScenarioProblemCode::MissingSide)
                .iter()
                .any(|problem| problem.detail().contains("no enemy actor")),
            "an enemy-free roster must name the missing side: {enemy_problems}"
        );
        assert!(
            enemy_problems
                .with_code(ScenarioProblemCode::ConditionUnsatisfiable)
                .iter()
                .any(|problem| problem.detail().contains("without an enemy actor")),
            "an enemy-free roster must make an enemy condition unsatisfiable: {enemy_problems}"
        );

        assert!(catalog.require_valid_custom(&request).is_err());
    }

    /// The valid fixture request produces no problem, so AC02's baseline is a
    /// request that passes validation before anything is changed.
    #[test]
    fn accept_f49_a_the_valid_synthetic_custom_request_has_no_problems() {
        let catalog = synthetic_instant_action_catalog();
        let request = synthetic_custom_request();
        let problems = catalog.validate_custom(&request);
        assert!(
            problems.is_empty(),
            "the fixture request is valid: {problems}"
        );
        assert_eq!(request.players(), 1);
        assert_eq!(catalog.options().max_players(), SYNTHETIC_IA_MAX_PLAYERS);
    }

    /// An incomplete draft reports every unset dimension instead of defaulting
    /// one, and a zero seat count is refused at construction.
    #[test]
    fn accept_f49_a_an_incomplete_draft_names_every_unset_dimension() {
        let error = CustomScenarioDraft::new()
            .resolve()
            .expect_err("an empty draft is refused");
        let ScenarioSchemaError::IncompleteDraft { missing } = &error else {
            panic!("expected an incomplete draft, got {error}");
        };
        assert_eq!(
            missing,
            &vec![
                "subject",
                "world",
                "environment",
                "roster",
                "skill",
                "rules",
                "seed",
                "players",
            ],
            "every dimension must be named, so no dimension can silently default"
        );

        let rules = VictoryRules::try_new(
            VictoryCondition::EliminateEnemies,
            RespawnBudget::None,
            None,
            TieOutcome::Draw,
        )
        .expect("the fixture rules are valid");
        let zero_seats = CustomScenarioDraft::new()
            .with_subject(fixture_id(
                ContentKind::IaScenario,
                "synthetic.fixture_ia_scenario_zero_seats",
            ))
            .with_world(fixture_known(fixture_world(SYNTHETIC_IA_WORLD_COAST)))
            .with_environment(fixture_known(fixture_environment(
                SYNTHETIC_IA_ENV_DAY_CLEAR,
            )))
            .with_roster(vec![
                synthetic_actor(
                    ScenarioSide::Player,
                    0,
                    SYNTHETIC_IA_AIRFRAME_INTERCEPTOR,
                    SYNTHETIC_IA_LOADOUT_LIGHT,
                ),
                synthetic_actor(
                    ScenarioSide::Enemy,
                    0,
                    SYNTHETIC_IA_AIRFRAME_INTERCEPTOR,
                    SYNTHETIC_IA_LOADOUT_LIGHT,
                ),
            ])
            .with_difficulty(synthetic_difficulty(DifficultyTier::Standard))
            .with_rules(rules)
            .with_seed(ScenarioSeed::new(1))
            .with_players(0)
            .with_provenance(synthetic_provenance())
            .resolve()
            .expect_err("zero seats is refused");
        assert_eq!(
            zero_seats,
            ScenarioSchemaError::InvalidPlayerCount {
                players: 0,
                max: MAX_SCENARIO_PLAYERS,
            }
        );
    }

    /// A scenario's roster keeps its bounds: more than one player slot, and
    /// more than the accepted per-side count, are refused.
    #[test]
    fn accept_f49_a_the_roster_bounds_players_and_per_side_counts() {
        let two_players = ScenarioRoster::try_new(vec![
            synthetic_actor(
                ScenarioSide::Player,
                0,
                SYNTHETIC_IA_AIRFRAME_INTERCEPTOR,
                SYNTHETIC_IA_LOADOUT_LIGHT,
            ),
            synthetic_actor(
                ScenarioSide::Player,
                1,
                SYNTHETIC_IA_AIRFRAME_INTERCEPTOR,
                SYNTHETIC_IA_LOADOUT_LIGHT,
            ),
        ])
        .expect_err("two player slots are refused");
        assert_eq!(
            two_players,
            ScenarioSchemaError::MultiplePlayers {
                max: MAX_SCENARIO_PLAYERS
            }
        );

        let too_many: Vec<ScenarioActorSpec> = (0..=MAX_SCENARIO_ACTORS_PER_SIDE as u32)
            .map(|slot| {
                synthetic_actor(
                    ScenarioSide::Enemy,
                    slot,
                    SYNTHETIC_IA_AIRFRAME_INTERCEPTOR,
                    SYNTHETIC_IA_LOADOUT_LIGHT,
                )
            })
            .collect();
        assert_eq!(
            ScenarioRoster::try_new(too_many).expect_err("the per-side bound is enforced"),
            ScenarioSchemaError::TooManyActorsOnSide {
                side: ScenarioSide::Enemy,
                count: MAX_SCENARIO_ACTORS_PER_SIDE + 1,
                max: MAX_SCENARIO_ACTORS_PER_SIDE,
            }
        );
    }

    /// A roster field in the wrong namespace is refused at construction, while
    /// an unmeasured value is carried and reported by validation. An unknown is
    /// never collapsed into a wrong-namespace id.
    #[test]
    fn accept_f49_a_namespace_errors_are_refused_but_unknowns_are_carried() {
        let wrong_faction = ScenarioActorSpec::try_new(
            ScenarioSide::Player,
            RosterSlot(0),
            fixture_id(ContentKind::Airframe, "synthetic.not_a_faction"),
            fixture_known(fixture_id(
                ContentKind::Airframe,
                SYNTHETIC_IA_AIRFRAME_INTERCEPTOR,
            )),
            fixture_known(fixture_id(ContentKind::Loadout, SYNTHETIC_IA_LOADOUT_LIGHT)),
            None,
            fixture_known(DeclaredSurvivability::Mortal),
            synthetic_provenance(),
        )
        .expect_err("a plane is not a faction");
        assert!(matches!(
            wrong_faction,
            ScenarioSchemaError::FactionKindMismatch { .. }
        ));

        let wrong_loadout = ScenarioActorSpec::try_new(
            ScenarioSide::Player,
            RosterSlot(0),
            synthetic_player_faction(),
            fixture_known(fixture_id(
                ContentKind::Airframe,
                SYNTHETIC_IA_AIRFRAME_INTERCEPTOR,
            )),
            fixture_known(fixture_id(ContentKind::Weapon, "synthetic.not_a_loadout")),
            None,
            fixture_known(DeclaredSurvivability::Mortal),
            synthetic_provenance(),
        )
        .expect_err("a weapon is not a loadout");
        assert!(matches!(
            wrong_loadout,
            ScenarioSchemaError::LoadoutKindMismatch { .. }
        ));

        let unmeasured = ScenarioActorSpec::try_new(
            ScenarioSide::Player,
            RosterSlot(0),
            synthetic_player_faction(),
            fixture_unknown("the original plane for this slot is unmeasured"),
            fixture_known(fixture_id(ContentKind::Loadout, SYNTHETIC_IA_LOADOUT_LIGHT)),
            None,
            fixture_known(DeclaredSurvivability::Mortal),
            synthetic_provenance(),
        )
        .expect("an unknown plane is carried, not refused at construction");
        assert!(!unmeasured.airframe().is_known());
    }
}
