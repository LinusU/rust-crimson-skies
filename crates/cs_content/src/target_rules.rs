//! The declared targeting schema: provenance-carrying faction relations,
//! targeting rules and selection actions (F30-A, F30-B).
//!
//! Spec: `specs/F30-targeting-classification-aim-assistance-and-threat-cues.md`,
//! stages `### F30-A` and `### F30-B`. Shared contract:
//! `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! This module is the **content half** of the targeting contract — the
//! normalized record a mission/rules importer produces. Its runtime
//! counterpart is `cs_sim::targeting` (the `TargetStore` a session
//! runs); the conversion boundary between them is `cs_app::targeting`.
//! The split mirrors `damage` ↔ `cs_sim::damage`: this crate cannot
//! depend on `cs_sim`, so the declared record keeps its own
//! [`DeclaredAllegiance`]
//! vocabulary and the boundary maps it field-wise.
//!
//! # Records
//!
//! A [`DeclaredTargetRules`] names its `subject` — the catalog id of the
//! mission or rules record the relations belong to — carries an
//! [`Origin`], the declared faction set, the directed
//! [`DeclaredRelation`]s and the [`TargetRuleSet`] policy knobs.
//!
//! Every load-bearing value is a [`Resolved`]: a relation's allegiance,
//! the threat window, the crosshair cone and the two assistance options
//! are each either known with [`Provenance`] or an explicit unknown with
//! its claim id and reason — never a silent default (F14 non-negotiable
//! behavior 3). Relations are *directed*: "raiders hostile to the player
//! faction" does not assert the reverse, and an undeclared pair lowers to
//! no relation — which the runtime reports as `None`, never a guessed
//! hostility (F30 non-negotiable 1).
//!
//! Faction identity is the catalog's own [`ContentId`] in the
//! [`ContentKind::Faction`] namespace — the same identity discipline the
//! rest of the content uses.
//!
//! # Lead indicator and aim assistance
//!
//! `TargetRuleSet` keeps [`TargetRuleSet::lead_indicator`] and
//! [`TargetRuleSet::aim_assistance`] as *separate* `Resolved<bool>`
//! options (F30 non-negotiable 3): they are distinct features, each with
//! its own evidence, and an unknown status stays unknown rather than
//! defaulting on or off. No automatic hit correction is declared anywhere;
//! aim assistance is a declared option with an evidence class, not a
//! behavior this schema smuggles in.
//!
//! # Selection actions (F30-B)
//!
//! [`DeclaredSelectionActions`] is the declared action table: which typed
//! command edge (`cs_types::input::FlightCommand`, the engine's once-per-
//! press edges) an IA preset or control scheme binds to which selection
//! action. Each action is a [`Resolved`], so an action the importer could
//! not evidence stays unknown and refuses to lower rather than binding an
//! edge to a guess; the table is validated so one command binds at most one
//! action and no continuous axis is ever bound to a target action.
//!
//! # The assistance options reach the F30-C guidance consumer (F30-C)
//!
//! The F30-C guidance consumer is the last reader of
//! [`TargetRuleSet::lead_indicator`] and [`TargetRuleSet::aim_assistance`],
//! and it reads them as two separate [`Resolved<bool>`] values, each with its
//! own [`Provenance`] — which is exactly the separation and the evidence
//! classification non-negotiable 3 asks for. `cs_app::targeting::lower_rules`
//! carries each provenance across the lowering boundary and
//! `cs_app::targeting::AssistanceOption::presentable` reads it, so a consumer
//! that draws an aid can tell a measured option from a designed default.
//!
//! F30-C deliberately adds **no** declared record to this module: the guidance
//! consumer's declared inputs are the two options this record already carries.
//! In particular there is no declared aid magnitude, no lead point and no aim
//! correction anywhere in the schema, because a lead solution needs the
//! target's velocity and the projectile's measured ballistics — neither of which
//! this schema has evidence for, and neither of which is measured (F30-D).
//!
//! # Designed vocabulary, not original data
//!
//! The original game's faction matrix, target-cycle order, selection
//! actions and their key bindings, reveal rules, crosshair cone and
//! assistance behavior are unmeasured (F30 "Research boundary"; F30-D's
//! retail stage). Every value in the synthetic fixtures is newly authored
//! project design carrying `Origin::SyntheticFixture` and designed
//! provenance, recorded in
//! `docs/findings/2026-09-30-f30-a-target-queries-and-allegiance-contracts.md`,
//! `docs/findings/2026-10-02-f30-b-selection-actions-and-threat-state.md` and,
//! for the F30-C consumer contract, in
//! `docs/findings/2026-10-02-f30-c-hud-spyglass-and-weapon-guidance.md`.
//!
//! # What F30-D measured about the original's action vocabulary
//!
//! F30-D read the original's shipped string image (`strings.dll`: its
//! `RT_STRING` label tree plus the `{name, id}` symbol table in its PE
//! `.data` section) through the production readers, and recorded the result
//! in `docs/findings/2026-10-02-f30-d-target-order-reveal-and-assistance.md`.
//! The measurement is about **names**, because a label names a command and
//! never says what it does:
//!
//! * The original names **eleven** target commands: a clear
//!   (`MSG_CMD_TARGET_NOTHING`, 10 008), an under-reticule pick
//!   (`MSG_CMD_TARGET_UNDER_RETICULE`, 10 009) and a next / previous /
//!   nearest triple for each of three named classes — enemy, ally and
//!   *ground*. So [`DeclaredAction::Clear`], [`DeclaredAction::UnderCrosshair`]
//!   and the cycle/nearest actions have a name in the original; this schema's
//!   spelling of the third class as `nearest_non_aircraft` is this project's
//!   name for what the original calls `GROUND`, and the difference stays
//!   visible rather than being papered over.
//! * The original names **no** command for [`DeclaredAction::NearestAttacker`]
//!   or [`DeclaredAction::NearestObjective`]. Both are in this project's
//!   deliverable, so both stay; each carries designed provenance, and the
//!   F30-C consumer gate (`cs_app::targeting::AssistanceOffer`) keeps a designed
//!   value from ever being presented as original behavior. `absent` means
//!   "absent from the shipped observation", never "the original cannot do it".
//! * The assist-shaped vocabulary the original *does* name is the **padlock**
//!   family: three mode commands (`MSG_CMD_PADLOCK_SNAP`, `_WATCH`, `_STICK`)
//!   and nine directions. What a mode or a direction does is native code, so
//!   this is evidence that an assist family exists and nothing more. The two
//!   separate `Resolved<bool>` options above stay separate (F30
//!   non-negotiable 3) and neither is given a padlock semantics.
//! * The shipped string image names **no** reveal or visibility concept and
//!   **no** lead-indicator or aim-assistance option: over all 1 023 named
//!   entries, no name contains `REVEAL`, `VISIB`, `SENSOR`, `DETECT`,
//!   `HIDDEN`, `STEALTH`, `LEAD` or `ASSIST`. The reveal rule this schema
//!   carries is therefore project design, and its absence from the string
//!   vocabulary is an absence measurement, not a licence to guess the
//!   original's.
//!
//! None of this is a `verified_original` claim: the target **order** the
//! original's cycle walks, its **reveal rules** and its **assistance
//! behavior** remain unmeasured and are recorded as fidelity limitations in
//! the F30-D finding.

use std::collections::BTreeSet;
use std::fmt;

use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::input::FlightCommand;
use cs_types::space::Radians;

/// The declared relation of one faction toward another.
///
/// Mirrors `cs_sim::targeting::Allegiance`; the boundary lowers it
/// field-wise. An undeclared pair has *no* record — the absence is the
/// unknown — and a record whose allegiance is [`Resolved::Unknown`]
/// refuses to lower rather than collapsing to "no relation", because the
/// two are not the same statement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeclaredAllegiance {
    /// The factions are enemies.
    Hostile,
    /// The factions are neither enemies nor allies.
    Neutral,
    /// The factions are allies.
    Friendly,
}

impl DeclaredAllegiance {
    /// Every allegiance, in a stable order.
    pub const ALL: &'static [DeclaredAllegiance] = &[Self::Hostile, Self::Neutral, Self::Friendly];

    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Hostile => "hostile",
            Self::Neutral => "neutral",
            Self::Friendly => "friendly",
        }
    }
}

impl fmt::Display for DeclaredAllegiance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// One declared directed relation: `from` regards `to` with `allegiance`.
///
/// The allegiance is a [`Resolved`]: a measured or authored relation is
/// `Known` with its provenance, and a pair known to matter but not yet
/// measured is `Unknown` with its claim id and reason — which the
/// lowering boundary refuses rather than guessing (F30 non-negotiable 1).
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredRelation {
    /// The observing faction (`ContentKind::Faction`).
    pub from: ContentId,
    /// The observed faction (`ContentKind::Faction`).
    pub to: ContentId,
    /// The declared relation, or an explicit unknown.
    pub allegiance: Resolved<DeclaredAllegiance>,
}

/// The declared targeting policy knobs a session runs under.
///
/// Each field is a [`Resolved`]: a value the importer could not evidence
/// stays an explicit unknown and refuses to lower, so no session runs
/// targeting under a guessed window, cone or assistance flag.
#[derive(Clone, Debug, PartialEq)]
pub struct TargetRuleSet {
    /// How many ticks an authoritative attack keeps its attacker a live
    /// threat cue for the victim.
    pub threat_window: Resolved<u64>,
    /// The default acceptance half-angle of an under-crosshair selection,
    /// in radians within `[0, π]` when known.
    pub crosshair_cone: Resolved<Radians>,
    /// Whether a lead indicator is offered. Separate from
    /// `aim_assistance` (F30 non-negotiable 3): the indicator is a
    /// display aid, assistance is aim correction — and neither is
    /// declared without its own evidence.
    pub lead_indicator: Resolved<bool>,
    /// Whether aim assistance is offered. Separate from
    /// `lead_indicator`; it is an option with an evidence class, never an
    /// automatic hit correction claiming original behavior
    /// (non-negotiable 3).
    pub aim_assistance: Resolved<bool>,
}

/// Why a [`DeclaredTargetRules`] was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum TargetRulesError {
    /// A faction entry's id is not in the `faction` namespace.
    FactionKindMismatch {
        /// The offending id.
        faction: ContentId,
    },
    /// Two faction entries share one id.
    DuplicateFaction {
        /// The duplicated id.
        faction: ContentId,
    },
    /// A relation endpoint's id is not in the `faction` namespace.
    RelationEndpointKind {
        /// The offending endpoint id.
        endpoint: ContentId,
    },
    /// A relation names a faction that was never declared.
    UndeclaredFaction {
        /// The relation's index in the authored list.
        relation: usize,
        /// The endpoint missing from the faction set.
        missing: ContentId,
    },
    /// A relation relates a faction to itself; self-relations are the
    /// runtime contract's own invariant, not data.
    SelfRelation {
        /// The faction related to itself.
        faction: ContentId,
    },
    /// The directed pair `(from, to)` is declared twice.
    DuplicateRelation {
        /// The observing end.
        from: ContentId,
        /// The observed end.
        to: ContentId,
    },
    /// A known `crosshair_cone` was NaN or infinite.
    NonFiniteCone,
    /// A known `crosshair_cone` fell outside `[0, π]`.
    ConeOutOfRange {
        /// The rejected angle.
        radians: f64,
    },
}

impl fmt::Display for TargetRulesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FactionKindMismatch { faction } => {
                write!(f, "faction entry {faction} is not in the faction namespace")
            }
            Self::DuplicateFaction { faction } => {
                write!(f, "faction {faction} is declared more than once")
            }
            Self::RelationEndpointKind { endpoint } => write!(
                f,
                "relation endpoint {endpoint} is not in the faction namespace"
            ),
            Self::UndeclaredFaction { relation, missing } => write!(
                f,
                "relation #{relation} names {missing}, which is not a declared faction"
            ),
            Self::SelfRelation { faction } => {
                write!(
                    f,
                    "a faction cannot declare a relation to itself ({faction})"
                )
            }
            Self::DuplicateRelation { from, to } => {
                write!(f, "relation {from} -> {to} is declared more than once")
            }
            Self::NonFiniteCone => write!(f, "the crosshair cone must be finite"),
            Self::ConeOutOfRange { radians } => {
                write!(f, "the crosshair cone {radians} rad is outside [0, π]")
            }
        }
    }
}

impl std::error::Error for TargetRulesError {}

/// The declared targeting rules of one catalog subject.
///
/// `subject` is the catalog id the rules belong to — a `mission` id for
/// per-mission rules, another launchable id for scenario rules — so the
/// record shares the catalog's identity discipline. `provenance` records
/// where the record itself came from.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredTargetRules {
    subject: ContentId,
    origin: Origin,
    factions: Vec<ContentId>,
    relations: Vec<DeclaredRelation>,
    rules: TargetRuleSet,
    provenance: Provenance,
}

impl DeclaredTargetRules {
    /// Assembles and validates a declared rules record.
    ///
    /// # Errors
    ///
    /// [`TargetRulesError`] on a non-faction or duplicated faction id, a
    /// relation endpoint outside the `faction` namespace, a relation
    /// naming an undeclared faction, a self-relation, a duplicated
    /// directed pair or a corrupt known cone.
    pub fn try_new(
        subject: ContentId,
        origin: Origin,
        factions: Vec<ContentId>,
        relations: Vec<DeclaredRelation>,
        rules: TargetRuleSet,
        provenance: Provenance,
    ) -> Result<Self, TargetRulesError> {
        validate(&factions, &relations, &rules)?;
        Ok(Self {
            subject,
            origin,
            factions,
            relations,
            rules,
            provenance,
        })
    }

    /// The catalog id the rules belong to.
    #[must_use]
    pub fn subject(&self) -> &ContentId {
        &self.subject
    }

    /// Where the record came from.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The declared factions, in authored order.
    #[must_use]
    pub fn factions(&self) -> &[ContentId] {
        &self.factions
    }

    /// The declared directed relations, in authored order.
    #[must_use]
    pub fn relations(&self) -> &[DeclaredRelation] {
        &self.relations
    }

    /// The declared policy knobs.
    #[must_use]
    pub const fn rules(&self) -> &TargetRuleSet {
        &self.rules
    }

    /// Where the record itself came from.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// The structural validation [`DeclaredTargetRules::try_new`] applies:
/// unique `faction`-namespace faction ids, directed relations between
/// distinct declared factions with no duplicated pair, and a finite
/// in-range known cone.
fn validate(
    factions: &[ContentId],
    relations: &[DeclaredRelation],
    rules: &TargetRuleSet,
) -> Result<(), TargetRulesError> {
    let mut declared = BTreeSet::new();
    for faction in factions {
        if faction.kind() != ContentKind::Faction {
            return Err(TargetRulesError::FactionKindMismatch {
                faction: faction.clone(),
            });
        }
        if !declared.insert(faction.clone()) {
            return Err(TargetRulesError::DuplicateFaction {
                faction: faction.clone(),
            });
        }
    }

    let mut pairs = BTreeSet::new();
    for (index, relation) in relations.iter().enumerate() {
        for endpoint in [&relation.from, &relation.to] {
            if endpoint.kind() != ContentKind::Faction {
                return Err(TargetRulesError::RelationEndpointKind {
                    endpoint: endpoint.clone(),
                });
            }
        }
        if relation.from == relation.to {
            return Err(TargetRulesError::SelfRelation {
                faction: relation.from.clone(),
            });
        }
        for endpoint in [&relation.from, &relation.to] {
            if !declared.contains(endpoint) {
                return Err(TargetRulesError::UndeclaredFaction {
                    relation: index,
                    missing: endpoint.clone(),
                });
            }
        }
        if !pairs.insert((relation.from.clone(), relation.to.clone())) {
            return Err(TargetRulesError::DuplicateRelation {
                from: relation.from.clone(),
                to: relation.to.clone(),
            });
        }
    }

    if let Resolved::Known(known) = &rules.crosshair_cone {
        if !known.value.0.is_finite() {
            return Err(TargetRulesError::NonFiniteCone);
        }
        if !(0.0..=std::f64::consts::PI).contains(&known.value.0) {
            return Err(TargetRulesError::ConeOutOfRange {
                radians: known.value.0,
            });
        }
    }
    Ok(())
}

/// The declared selection action vocabulary (F30-B).
///
/// Mirrors `cs_sim::targeting::SelectionAction`; the boundary lowers it
/// onto the runtime table that binds command edges. The set is the
/// deliverable's action list — the enemy/objective, ally, non-aircraft,
/// nearest-attacker, under-crosshair and clear actions, plus the two cycle
/// directions. Which of them the original game binds, and to which keys, is
/// unmeasured: F30-D measured which of them the original *names*
/// ([`DeclaredAction::NearestAttacker`] and
/// [`DeclaredAction::NearestObjective`] have no name in the shipped
/// string image — see the module docs), and the binding of any of them to a
/// key stays native data in the packed executable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DeclaredAction {
    /// Walk the declared-hostile cycle away from the observer.
    NextHostile,
    /// Walk it toward the observer.
    PreviousHostile,
    /// The nearest declared hostile.
    NearestHostile,
    /// The nearest actor mission rules flagged as an objective.
    NearestObjective,
    /// The nearest declared ally.
    NearestAlly,
    /// The nearest actor that is not an aircraft.
    NearestNonAircraft,
    /// The nearest actor with a live threat cue against the observer.
    NearestAttacker,
    /// The eligible, unoccluded actor under the crosshair.
    UnderCrosshair,
    /// Drop the selection.
    Clear,
}

impl DeclaredAction {
    /// Every action, in a stable order.
    pub const ALL: &'static [DeclaredAction] = &[
        Self::NextHostile,
        Self::PreviousHostile,
        Self::NearestHostile,
        Self::NearestObjective,
        Self::NearestAlly,
        Self::NearestNonAircraft,
        Self::NearestAttacker,
        Self::UnderCrosshair,
        Self::Clear,
    ];

    /// The stable label used in reports and persisted bindings.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::NextHostile => "next_hostile",
            Self::PreviousHostile => "previous_hostile",
            Self::NearestHostile => "nearest_hostile",
            Self::NearestObjective => "nearest_objective",
            Self::NearestAlly => "nearest_ally",
            Self::NearestNonAircraft => "nearest_non_aircraft",
            Self::NearestAttacker => "nearest_attacker",
            Self::UnderCrosshair => "under_crosshair",
            Self::Clear => "clear",
        }
    }

    /// Looks an action up by its label; `None` for an unknown label.
    #[must_use]
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|action| action.label() == label)
    }
}

impl fmt::Display for DeclaredAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// One declared command binding: the typed command edge an IA preset or
/// control scheme binds, and the action it runs.
///
/// `action` is a [`Resolved`]: a binding the importer could not evidence
/// refuses to lower rather than binding the edge to a guess — an unknown
/// action is not the same statement as "this command is not a target
/// command" (F30 non-negotiable 1, AGENTS "unknown means unknown").
/// `evidence` records where the binding itself came from, so a
/// `verified_original` key binding and a `designed` default are never the
/// same record.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredSelectionAction {
    /// The command edge this binding fires on.
    pub command: FlightCommand,
    /// The action it runs, or an explicit unknown.
    pub action: Resolved<DeclaredAction>,
    /// Where the binding came from.
    pub evidence: Provenance,
}

/// Why a [`DeclaredSelectionActions`] was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum SelectionActionsError {
    /// A binding names a continuous axis command. A target action is a
    /// once-per-press edge; binding it to an axis would fire it on every
    /// frame the axis moved.
    ContinuousCommand {
        /// The offending command.
        command: FlightCommand,
    },
    /// The same command is bound twice; one edge runs one action.
    DuplicateCommand {
        /// The command bound more than once.
        command: FlightCommand,
    },
}

impl fmt::Display for SelectionActionsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ContinuousCommand { command } => write!(
                f,
                "{command} is a continuous axis; a selection action must bind a once-per-press command"
            ),
            Self::DuplicateCommand { command } => {
                write!(f, "command {command} is bound to more than one action")
            }
        }
    }
}

impl std::error::Error for SelectionActionsError {}

/// The declared selection-action table of one catalog subject.
///
/// `subject` is the catalog id the table belongs to — the same discipline
/// [`DeclaredTargetRules`] follows, so an IA preset's actions and a
/// mission's relations are separately addressable records that can still
/// name each other.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredSelectionActions {
    subject: ContentId,
    origin: Origin,
    bindings: Vec<DeclaredSelectionAction>,
    provenance: Provenance,
}

impl DeclaredSelectionActions {
    /// Assembles and validates a declared action table.
    ///
    /// # Errors
    ///
    /// [`SelectionActionsError`] when a binding names a continuous axis
    /// command or the same command is bound twice.
    pub fn try_new(
        subject: ContentId,
        origin: Origin,
        bindings: Vec<DeclaredSelectionAction>,
        provenance: Provenance,
    ) -> Result<Self, SelectionActionsError> {
        let mut commands = BTreeSet::new();
        for binding in &bindings {
            if binding.command.is_continuous() {
                return Err(SelectionActionsError::ContinuousCommand {
                    command: binding.command,
                });
            }
            if !commands.insert(binding.command) {
                return Err(SelectionActionsError::DuplicateCommand {
                    command: binding.command,
                });
            }
        }
        Ok(Self {
            subject,
            origin,
            bindings,
            provenance,
        })
    }

    /// The catalog id the table belongs to.
    #[must_use]
    pub fn subject(&self) -> &ContentId {
        &self.subject
    }

    /// Where the table came from.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The declared bindings, in authored order.
    #[must_use]
    pub fn bindings(&self) -> &[DeclaredSelectionAction] {
        &self.bindings
    }

    /// The binding for one command edge, if the table declares it.
    #[must_use]
    pub fn binding(&self, command: FlightCommand) -> Option<&DeclaredSelectionAction> {
        self.bindings
            .iter()
            .find(|binding| binding.command == command)
    }

    /// Where the table itself came from.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

// ----------------------------------------------------------- fixture ------

fn faction(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::Faction, key).expect("fixture faction id is valid")
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(
        value,
        Provenance::designed(ClaimId::new("f30a.synthetic-target-range").expect("valid claim id")),
    ))
}

fn relation(from: &ContentId, to: &ContentId, allegiance: DeclaredAllegiance) -> DeclaredRelation {
    DeclaredRelation {
        from: from.clone(),
        to: to.clone(),
        allegiance: known(allegiance),
    }
}

/// The minimal synthetic fixture in declared form: the same
/// player/raider/trader triangle `cs_sim::targeting`'s synthetic
/// allegiance table defines, with declared provenance.
///
/// The subject is `ia_scenario/synthetic.target-range` (a launchable
/// synthetic scenario the rules belong to); the record carries
/// [`Origin::SyntheticFixture`] and designed provenance — it can never be
/// mistaken for retail content and cannot stand in for it.
#[must_use]
pub fn declared_synthetic_target_rules() -> DeclaredTargetRules {
    let player = faction("synthetic.nathan");
    let raiders = faction("synthetic.raiders");
    let traders = faction("synthetic.traders");

    DeclaredTargetRules::try_new(
        ContentId::from_source(ContentKind::IaScenario, "synthetic.target-range")
            .expect("fixture subject id is valid"),
        Origin::SyntheticFixture,
        vec![player.clone(), raiders.clone(), traders.clone()],
        vec![
            relation(&player, &raiders, DeclaredAllegiance::Hostile),
            relation(&raiders, &player, DeclaredAllegiance::Hostile),
            relation(&player, &traders, DeclaredAllegiance::Neutral),
            relation(&traders, &player, DeclaredAllegiance::Neutral),
            relation(&raiders, &traders, DeclaredAllegiance::Hostile),
            relation(&traders, &raiders, DeclaredAllegiance::Hostile),
        ],
        TargetRuleSet {
            threat_window: known(120),
            crosshair_cone: known(Radians(std::f64::consts::PI / 18.0)),
            // Neither assistance option has original evidence; both are
            // designed defaults recorded as such.
            lead_indicator: known(true),
            aim_assistance: known(false),
        },
        Provenance::designed(ClaimId::new("f30a.synthetic-target-range").expect("valid claim id")),
    )
    .expect("the declared synthetic target rules fixture is valid")
}

fn action_binding(command: FlightCommand, action: DeclaredAction) -> DeclaredSelectionAction {
    // The action table carries its own claim, distinct from the F30-A rules
    // record: a preset's key binding and a mission's faction relations are
    // separately evidenced statements.
    let claim = ClaimId::new("f30b.synthetic-target-bindings").expect("valid claim id");
    DeclaredSelectionAction {
        command,
        action: Resolved::Known(Known::new(action, Provenance::designed(claim.clone()))),
        evidence: Provenance::designed(claim),
    }
}

/// The minimal synthetic action table in declared form: the two cycle edges
/// over the declared hostiles plus one nearest-attacker binding, the same
/// shape the runtime `SelectionBinding` fixture defines.
///
/// Newly authored project design with designed provenance. It says nothing
/// about which keys the original game bound — that is F30-D's retail
/// measurement.
#[must_use]
pub fn declared_synthetic_selection_actions() -> DeclaredSelectionActions {
    DeclaredSelectionActions::try_new(
        ContentId::from_source(ContentKind::IaScenario, "synthetic.target-range")
            .expect("fixture subject id is valid"),
        Origin::SyntheticFixture,
        vec![
            action_binding(FlightCommand::TargetNext, DeclaredAction::NextHostile),
            action_binding(FlightCommand::TargetPrev, DeclaredAction::PreviousHostile),
            action_binding(FlightCommand::CycleWeapon, DeclaredAction::NearestAttacker),
        ],
        Provenance::designed(
            ClaimId::new("f30b.synthetic-target-bindings").expect("valid claim id"),
        ),
    )
    .expect("the declared synthetic selection-action fixture is valid")
}
