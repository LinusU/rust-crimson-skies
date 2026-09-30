//! The declared targeting schema: provenance-carrying faction relations
//! and targeting rules (F30-A).
//!
//! Spec: `specs/F30-targeting-classification-aim-assistance-and-threat-cues.md`,
//! stage `### F30-A`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
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
//! # Designed vocabulary, not original data
//!
//! The original game's faction matrix, target-cycle order, reveal rules,
//! crosshair cone and assistance behavior are unmeasured (F30 "Research
//! boundary"; F30-D's retail stage). Every value in the synthetic fixture
//! is newly authored project design carrying `Origin::SyntheticFixture`
//! and designed provenance, recorded in
//! `docs/findings/2026-09-30-f30-a-target-queries-and-allegiance-contracts.md`.

use std::collections::BTreeSet;
use std::fmt;

use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
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
