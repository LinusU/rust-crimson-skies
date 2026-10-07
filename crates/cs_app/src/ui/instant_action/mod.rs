//! The Instant Action selection and scenario-lowering boundary (F49-A).
//!
//! Spec: `specs/F49-instant-action-presets-and-custom-scenarios.md`, stage
//! `### F49-A`. Shared contract: `docs/contracts/UI-NETWORK.md` ("A UI action
//! requests a domain transaction; it does not directly edit campaign cash,
//! ownership or objective fields." and "Dropdown selection is a content id,
//! never a transient row number").
//!
//! This module is the boundary between three things that cannot see each
//! other: the **declared** preset and custom-scenario schema
//! ([`cs_content::instant_action`]), the **runtime** actor store
//! ([`cs_sim::allies`]), and a **selection**. It owns:
//!
//! * [`ScenarioSelection`] — what the player chose: one preset id, or one
//!   custom draft. A selection is content ids and content values only. It has
//!   no row number, no index into a list a catalog reorder would move, and no
//!   campaign field of any kind, which is what UI-NETWORK's dropdown rule asks
//!   for and what F49 non-negotiable 3 ("IA never modifies campaign
//!   progression or money") is enforced by construction here: there is no
//!   field on this type that could carry cash, ownership or an objective.
//! * [`LoweredScenario`] — the actors, world, rules and seed one selection
//!   produces, with every id resolved through the production
//!   [`cs_sim::allies`] identity constructors. An `Resolved::Unknown` **refuses**:
//!   an unmeasured plane never lowers to a default aircraft (F33's rule, which
//!   F49 inherits by using the same store).
//! * [`LowerError`] — why a selection could not be lowered, carrying the whole
//!   [`ScenarioProblems`] list so a screen shows every problem at once
//!   (AC04) rather than one per retry.
//!
//! What this stage deliberately does **not** own: the running scenario.
//! `ScenarioSelection` and [`LoweredScenario`] are records; spawning actors,
//! ticking a mission and reporting an outcome are F49-B's
//! ("scenario normalization and isolated outcomes") and F49-C's. The screen
//! that drives this boundary is F45's front-end state table plus F49-C's IA
//! screens.
//!
//! # Designed, synthetic
//!
//! Every id, name and dimension below comes from the synthetic fixture catalog
//! (`cs_content::instant_action::synthetic_instant_action_catalog`). The
//! original 2000 PC Instant Action preset list and option table are
//! unmeasured; see
//! `docs/findings/2026-10-01-f49-a-instant-action-scenario-schemas.md`.

mod normalize;

use std::fmt;

use cs_content::instant_action::{
    CustomScenarioDraft, CustomScenarioRequest, InstantActionCatalog, RosterSlot,
    ScenarioActorSpec, ScenarioParameters, ScenarioProblem, ScenarioProblems, ScenarioSeed,
    ScenarioSide, require_known,
};
use cs_content::pilots::DeclaredSurvivability;
use cs_sim::allies::{
    FactionId, GeometryId, IdentityError, PilotId, SurvivabilityPolicy, WingmateSlot,
};
use cs_types::content::{ContentId, Resolved};
use cs_types::evidence::ClaimId;

pub use normalize::{
    ActorField, ScenarioChange, ScenarioOutcome, ScenarioResult, ScenarioSnapshot, diff_scenarios,
    evaluate_outcome,
};

pub use cs_content::instant_action::{RespawnBudget, TieOutcome, VictoryCondition, VictoryRules};

/// What the player selected: one preset, or one custom scenario.
#[derive(Clone, Debug, PartialEq)]
pub enum ScenarioSelection {
    /// One authored preset, by its `ia_preset` content id.
    Preset(ContentId),
    /// A player's custom scenario, as a draft whose dimensions may still be
    /// incomplete.
    ///
    /// Boxed because a draft carries every dimension of the form, and an
    /// unboxed variant would make a one-preset selection as large as a filled
    /// custom draft (clippy's `large_enum_variant`). The box is only unwrapped
    /// at [`resolve_custom`]/[`lower_custom`], which consume the draft anyway.
    Custom(Box<CustomScenarioDraft>),
}

impl ScenarioSelection {
    /// The preset id this selection names, when it names one.
    ///
    /// A custom selection has no preset id: it has its own `ia_scenario`
    /// subject, which is a different namespace entry, not a second name for
    /// the same preset.
    #[must_use]
    pub fn preset_id(&self) -> Option<&ContentId> {
        match self {
            Self::Preset(id) => Some(id),
            Self::Custom(_) => None,
        }
    }

    /// The custom draft this selection holds, when it is a custom selection.
    #[must_use]
    pub fn draft(&self) -> Option<&CustomScenarioDraft> {
        match self {
            Self::Preset(_) => None,
            Self::Custom(draft) => Some(draft.as_ref()),
        }
    }
}

/// One lowered actor: the runtime identity records a session registers from.
#[derive(Clone, Debug, PartialEq)]
pub struct LoweredScenarioActor {
    /// The side this actor is spawned on.
    pub side: ScenarioSide,
    /// The authored slot index within its side.
    pub slot: RosterSlot,
    /// The faction it flies for.
    pub faction: FactionId,
    /// The geometry it is built from.
    pub geometry: GeometryId,
    /// The loadout it carries.
    pub loadout: ContentId,
    /// The pilot that flies it, when the scenario authors one.
    pub pilot: Option<PilotId>,
    /// The lowered survivability.
    pub survivability: SurvivabilityPolicy,
}

/// The world one selection loads.
#[derive(Clone, Debug, PartialEq)]
pub struct LoweredWorld {
    /// The world group or variant the scenario loads.
    pub world: ContentId,
    /// The environment the scenario runs under.
    pub environment: String,
}

/// One selection's lowered scenario: what a session would spawn and run.
///
/// The record is total and ordered: the actors are in the declared
/// `(side, slot)` order, so two lowerings of the same selection are the same
/// record and a diff of two selections' plans shows exactly which actor moved
/// (AC02's baseline; F49-B performs the change and reports it).
#[derive(Clone, Debug, PartialEq)]
pub struct LoweredScenario {
    subject: ContentId,
    players: u8,
    world: LoweredWorld,
    actors: Vec<LoweredScenarioActor>,
    condition: VictoryCondition,
    respawns: RespawnBudget,
    deadline_ticks: Option<u64>,
    tie_outcome: TieOutcome,
    difficulty: cs_content::ai::DifficultyTier,
    seed: ScenarioSeed,
}

impl LoweredScenario {
    /// The `ia_scenario` identity this scenario runs under: the preset's
    /// scenario id, or the custom draft's subject.
    #[must_use]
    pub const fn subject(&self) -> &ContentId {
        &self.subject
    }

    /// The number of human seats.
    #[must_use]
    pub const fn players(&self) -> u8 {
        self.players
    }

    /// The world the session loads.
    #[must_use]
    pub const fn world(&self) -> &LoweredWorld {
        &self.world
    }

    /// The actors, in declared `(side, slot)` order.
    #[must_use]
    pub fn actors(&self) -> &[LoweredScenarioActor] {
        &self.actors
    }

    /// The actors on one side, in slot order.
    #[must_use]
    pub fn actors_on(&self, side: ScenarioSide) -> Vec<&LoweredScenarioActor> {
        self.actors
            .iter()
            .filter(|actor| actor.side == side)
            .collect()
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
    /// ends on one.
    #[must_use]
    pub const fn deadline_ticks(&self) -> Option<u64> {
        self.deadline_ticks
    }

    /// The declared outcome for a tie.
    #[must_use]
    pub const fn tie_outcome(&self) -> TieOutcome {
        self.tie_outcome
    }

    /// The declared difficulty tier.
    #[must_use]
    pub const fn difficulty(&self) -> cs_content::ai::DifficultyTier {
        self.difficulty
    }

    /// The scenario's explicit root seed, which a developer tool displays and
    /// a replay records (F49 non-negotiable 4).
    #[must_use]
    pub const fn seed(&self) -> ScenarioSeed {
        self.seed
    }
}

/// Why a selection could not be lowered.
#[derive(Clone, Debug, PartialEq)]
pub enum LowerError {
    /// The selection named a preset the catalog does not hold.
    UnknownPreset {
        /// The preset id that was selected.
        preset: ContentId,
    },
    /// A custom draft could not become a complete request.
    IncompleteDraft {
        /// The unset dimension names, in form order.
        missing: Vec<&'static str>,
        /// The schema refusal, when the draft was complete but invalid.
        schema: Option<cs_content::instant_action::ScenarioSchemaError>,
    },
    /// The selection is structurally valid but the catalog refuses it.
    Invalid {
        /// Every problem, sorted, so a screen renders all of them at once.
        problems: ScenarioProblems,
    },
    /// A value the lowering needs is `Resolved::Unknown`.
    ///
    /// An unknown is refused rather than defaulted: an unmeasured plane never
    /// becomes some other plane, and an unmeasured survivability never becomes
    /// a silent mortal.
    UnknownValue {
        /// Which field, e.g. `"enemy slot 0 airframe"`.
        field: String,
        /// The claim the unknown is recorded under.
        claim_id: ClaimId,
        /// Why it is unknown.
        reason: String,
    },
    /// A referenced id was not in the namespace its runtime record requires.
    Identity {
        /// Which field the id belongs to.
        field: &'static str,
        /// The namespace error.
        error: IdentityError,
    },
}

impl fmt::Display for LowerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownPreset { preset } => {
                write!(f, "the catalog holds no preset {preset}")
            }
            Self::IncompleteDraft { missing, schema } => {
                write!(f, "the custom scenario is incomplete: ")?;
                if missing.is_empty() {
                    // The draft named every dimension, so the refusal is about
                    // the roster or the rules rather than an unset field. It is
                    // rendered through `Display` like every other message here:
                    // `Debug` would print a variant name at a player, which is
                    // the opposite of AC04's "actionable validation errors".
                    match schema {
                        Some(schema) => write!(f, "{schema}"),
                        None => f.write_str("the scenario is not complete"),
                    }
                } else {
                    write!(f, "{} unset", missing.join(", "))
                }
            }
            Self::Invalid { problems } => write!(f, "the scenario is invalid: {problems}"),
            Self::UnknownValue {
                field,
                claim_id,
                reason,
            } => write!(f, "{field} is unknown ({}: {reason})", claim_id.as_str()),
            Self::Identity { field, error } => write!(f, "{field} is not a valid id: {error}"),
        }
    }
}

impl std::error::Error for LowerError {}

/// The catalog's presets as selectable rows, in canonical id order.
///
/// The rows carry **content ids**, never a row index: a UI that renumbered its
/// list on a catalog reorder would then be selecting a different preset, which
/// UI-NETWORK's dropdown rule forbids.
#[must_use]
pub fn preset_rows(catalog: &InstantActionCatalog) -> Vec<InstantActionPresetRow> {
    catalog
        .presets()
        .iter()
        .map(|preset| InstantActionPresetRow {
            preset_id: preset.id().clone(),
            scenario_id: preset.scenario().clone(),
            title: match preset.title() {
                Resolved::Known(known) => Some(known.value.clone()),
                Resolved::Unknown { .. } => None,
            },
        })
        .collect()
}

/// One selectable preset row.
#[derive(Clone, Debug, PartialEq)]
pub struct InstantActionPresetRow {
    /// The `ia_preset` id this row selects.
    pub preset_id: ContentId,
    /// The `ia_scenario` id the selection will run under, so a row shows which
    /// authored scenario it launches.
    pub scenario_id: ContentId,
    /// The measured display name, when one has been read. `None` means
    /// unmeasured, and the UI says so instead of inventing a name.
    pub title: Option<String>,
}

/// Lowers a preset selection through the production lowering path.
///
/// # Errors
///
/// [`LowerError::UnknownPreset`] when the catalog holds no such preset, and
/// [`LowerError::UnknownValue`] / [`LowerError::Identity`] when a declared
/// value cannot be lowered. Every other preset is valid by construction: the
/// catalog's own presets were accepted into it, and a preset is not re-validated
/// against the custom option table (a preset is an authored scenario, not a
/// player's selection — F49 non-negotiable 1).
pub fn lower_preset(
    catalog: &InstantActionCatalog,
    preset_id: &ContentId,
) -> Result<LoweredScenario, LowerError> {
    let preset = catalog
        .preset(preset_id)
        .ok_or_else(|| LowerError::UnknownPreset {
            preset: preset_id.clone(),
        })?;
    lower_parameters(
        preset.scenario().clone(),
        preset.parameters(),
        // A preset is authored single-player until evidence says otherwise.
        1,
    )
}

/// Lowers a custom-scenario draft through the production lowering path.
///
/// The draft is resolved first (naming every unset dimension), then validated
/// against the catalog's option table (naming every invalid selection), and
/// only then lowered — so a selection is refused *before* any actor identity
/// is constructed and never half-spawns.
///
/// # Errors
///
/// [`LowerError::IncompleteDraft`] when a dimension is unset or the resolved
/// roster is invalid, [`LowerError::Invalid`] with the full
/// [`ScenarioProblems`] when the catalog refuses the request, and
/// [`LowerError::UnknownValue`] / [`LowerError::Identity`] when a declared
/// value cannot be lowered.
pub fn lower_custom(
    catalog: &InstantActionCatalog,
    draft: CustomScenarioDraft,
) -> Result<LoweredScenario, LowerError> {
    let request = resolve_custom(catalog, draft)?;
    lower_parameters(
        request.subject().clone(),
        request.parameters(),
        request.players(),
    )
}

/// Resolves and validates a custom draft, returning the request a lowering
/// consumes.
///
/// Split out so a screen can show the validation result without lowering: AC04
/// is about *reporting* problems, and reporting must not require spawning
/// anything.
///
/// # Errors
///
/// [`LowerError::IncompleteDraft`] and [`LowerError::Invalid`] exactly as
/// [`lower_custom`].
pub fn resolve_custom(
    catalog: &InstantActionCatalog,
    draft: CustomScenarioDraft,
) -> Result<CustomScenarioRequest, LowerError> {
    let missing = draft.unset_dimensions();
    let request = draft.resolve().map_err(|schema| {
        if missing.is_empty() {
            LowerError::IncompleteDraft {
                missing: Vec::new(),
                schema: Some(schema),
            }
        } else {
            LowerError::IncompleteDraft {
                missing,
                schema: None,
            }
        }
    })?;
    catalog
        .require_valid_custom(&request)
        .map_err(|problems| LowerError::Invalid { problems })?;
    Ok(request)
}

/// Lowers one resolved parameter set into the runtime records.
fn lower_parameters(
    subject: ContentId,
    parameters: &ScenarioParameters,
    players: u8,
) -> Result<LoweredScenario, LowerError> {
    let world = LoweredWorld {
        world: require_known("scenario world", parameters.world())
            .map_err(unknown_as)?
            .content_id()
            .clone(),
        environment: require_known("scenario environment", parameters.environment())
            .map_err(unknown_as)?
            .to_string(),
    };
    let rules = parameters.rules();
    let mut actors = Vec::with_capacity(parameters.roster().len());
    for actor in parameters.roster().actors() {
        actors.push(lower_actor(actor)?);
    }
    Ok(LoweredScenario {
        subject,
        players,
        world,
        actors,
        condition: rules.condition(),
        respawns: rules.respawns(),
        deadline_ticks: rules.deadline_ticks(),
        tie_outcome: rules.tie_outcome(),
        difficulty: parameters.difficulty().tier(),
        seed: parameters.seed(),
    })
}

/// Requires a known value, naming the field an unknown belongs to.
///
/// This is [`require_known`] with a **per-actor** field name, which the
/// content-side helper cannot take because its error type holds a `&'static`
/// field name. The unknown's own claim and reason are preserved unchanged, so
/// the refusal reads the same as one raised by the catalog.
fn require_known_at<T: Clone>(field: &str, value: &Resolved<T>) -> Result<T, LowerError> {
    match value {
        Resolved::Known(known) => Ok(known.value.clone()),
        Resolved::Unknown { claim_id, reason } => Err(LowerError::UnknownValue {
            field: field.to_owned(),
            claim_id: claim_id.clone(),
            reason: reason.clone(),
        }),
    }
}

/// Maps a `require_known` refusal onto the boundary's error, naming the field
/// the unknown belongs to.
fn unknown_as(error: cs_content::instant_action::ScenarioSchemaError) -> LowerError {
    match error {
        cs_content::instant_action::ScenarioSchemaError::UnresolvedValue {
            field,
            claim_id,
            reason,
        } => LowerError::UnknownValue {
            field: field.to_owned(),
            claim_id,
            reason,
        },
        // `require_known` only ever produces `UnresolvedValue`; anything else
        // would be a bug in the mapping, so it is named rather than ignored.
        other => LowerError::UnknownValue {
            field: "scenario value".to_owned(),
            claim_id: synthetic_boundary_claim(),
            reason: other.to_string(),
        },
    }
}

/// The claim a mapping bug would be recorded under. It is never an original
/// claim and must never be presented as evidence.
fn synthetic_boundary_claim() -> ClaimId {
    ClaimId::new("f49a.boundary-mapping").expect("the boundary claim id is valid")
}

/// Lowers one declared actor into its runtime identity records.
fn lower_actor(actor: &ScenarioActorSpec) -> Result<LoweredScenarioActor, LowerError> {
    let label = format!("{} slot {} airframe", actor.side(), actor.slot().index());

    let faction =
        FactionId::try_new(actor.faction().clone()).map_err(|error| LowerError::Identity {
            field: "scenario faction",
            error,
        })?;
    let geometry =
        GeometryId::try_new(require_known_at(&label, actor.airframe())?).map_err(|error| {
            LowerError::Identity {
                field: "scenario airframe",
                error,
            }
        })?;
    let loadout = require_known_at(
        &format!("{} slot {} loadout", actor.side(), actor.slot().index()),
        actor.loadout(),
    )?;
    let pilot = match actor.pilot() {
        None => None,
        Some(pilot) => {
            Some(
                PilotId::try_new(pilot.clone()).map_err(|error| LowerError::Identity {
                    field: "scenario pilot",
                    error,
                })?,
            )
        }
    };
    let survivability = match actor.survivability() {
        Resolved::Known(known) => match known.value {
            DeclaredSurvivability::Mortal => SurvivabilityPolicy::Mortal,
            DeclaredSurvivability::ProtectedNeutral => SurvivabilityPolicy::ProtectedNeutral,
            DeclaredSurvivability::ScriptedInvulnerable => {
                SurvivabilityPolicy::ScriptedInvulnerable
            }
        },
        Resolved::Unknown { claim_id, reason } => {
            return Err(LowerError::UnknownValue {
                field: format!(
                    "{} slot {} survivability",
                    actor.side(),
                    actor.slot().index()
                ),
                claim_id: claim_id.clone(),
                reason: reason.clone(),
            });
        }
    };
    Ok(LoweredScenarioActor {
        side: actor.side(),
        slot: actor.slot(),
        faction,
        geometry,
        loadout,
        pilot,
        survivability,
    })
}

/// The wingmate slot an ally actor occupies in the runtime roster.
///
/// Only an [`ScenarioSide::Ally`] actor occupies one: the player and the
/// enemies are not wingmates, so they report `None` rather than colliding with
/// the ally numbering. The slot index is the declared one, so this reports
/// whether an ally can be placed at all instead of silently dropping it —
/// building the [`WingmateAssignment`] itself is F49-B's spawn work.
#[must_use]
pub fn wingmate_slot(actor: &LoweredScenarioActor) -> Option<WingmateSlot> {
    (actor.side == ScenarioSide::Ally).then(|| WingmateSlot(actor.slot.index()))
}

/// Every dimension the catalog offers a custom scenario, as the names a UI
/// groups its controls under.
///
/// The list is derived from the catalog rather than hard-coded, so a dimension
/// the catalog does not offer cannot be displayed (F49 non-negotiable 5).
#[must_use]
pub fn custom_dimensions(catalog: &InstantActionCatalog) -> Vec<CustomDimension> {
    let options = catalog.options();
    vec![
        CustomDimension::World(options.worlds().to_vec()),
        CustomDimension::Environment(options.environments().to_vec()),
        CustomDimension::Airframe(options.airframes().to_vec()),
        CustomDimension::Loadout(options.loadouts().to_vec()),
        CustomDimension::Difficulty(options.difficulty_tiers().to_vec()),
        CustomDimension::Victory(options.victory_conditions().to_vec()),
    ]
}

/// One selectable dimension and the values the catalog offers for it.
#[derive(Clone, Debug, PartialEq)]
pub enum CustomDimension {
    /// Selectable worlds.
    World(Vec<cs_content::world::WorldId>),
    /// Selectable environments.
    Environment(Vec<cs_content::environment::EnvironmentId>),
    /// Selectable airframes.
    Airframe(Vec<ContentId>),
    /// Selectable loadouts.
    Loadout(Vec<ContentId>),
    /// Selectable difficulty tiers.
    Difficulty(Vec<cs_content::ai::DifficultyTier>),
    /// Selectable victory conditions.
    Victory(Vec<VictoryCondition>),
}

impl CustomDimension {
    /// The stable dimension name, matching
    /// [`ScenarioProblemCode::dimension`].
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::World(_) => "world",
            Self::Environment(_) => "environment",
            Self::Airframe(_) | Self::Loadout(_) => "roster",
            Self::Difficulty(_) => "skill",
            Self::Victory(_) => "rules",
        }
    }

    /// How many values this dimension offers.
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::World(values) => values.len(),
            Self::Environment(values) => values.len(),
            Self::Airframe(values) => values.len(),
            Self::Loadout(values) => values.len(),
            Self::Difficulty(values) => values.len(),
            Self::Victory(values) => values.len(),
        }
    }

    /// Whether this dimension offers no value, which a UI must not render as a
    /// selectable group.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Re-exported so a caller can name the problem codes without importing
/// `cs_content` directly.
pub use cs_content::instant_action::ScenarioProblemCode as ProblemCode;

/// The whole problem list a selection produced, for a screen that only reports.
#[must_use]
pub fn report_problems(error: &LowerError) -> Vec<&ScenarioProblem> {
    match error {
        LowerError::Invalid { problems } => problems.problems().iter().collect(),
        _ => Vec::new(),
    }
}
