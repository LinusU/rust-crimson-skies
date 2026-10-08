//! The declared pilot, wingmate and neutral-traffic roster (F33-A).
//!
//! Spec: `specs/F33-wingmates-factions-neutral-traffic-and-pilot-identity.md`,
//! stage `### F33-A`. Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! This module is the **content half** of the pilot/aircraft/faction
//! separation: the per-mission, provenance-carrying roster a mission
//! importer produces. Its runtime counterpart is `cs_sim::allies` (the
//! per-session `AlliesRoster`); the conversion boundary is
//! `cs_app::roster`. The split mirrors every other F14+ stage: this crate
//! cannot depend on `cs_sim`, so the declared records keep their own
//! vocabulary and the boundary maps it field-wise.
//!
//! # Records
//!
//! A [`DeclaredRoster`] names its `subject` — the mission or Instant Action
//! scenario the roster belongs to — carries an [`Origin`] and the mission's
//! declared player faction, and holds three separate lists:
//!
//! * [`DeclaredPilot`] — a pilot's identity (`ContentKind::Pilot`) and the
//!   [`DeclaredPilot::voice`] it speaks through, each a catalog id. A voice
//!   is a [`Resolved`]: a random line never stands in for a mission
//!   dialogue, so an unmeasured voice is an explicit unknown that refuses to
//!   lower rather than a silent pick (F33 non-negotiable 5).
//! * [`DeclaredWingmate`] — one authored wingmate slot: the pilot that flies
//!   it, the `airframe` it launches in, the `loadout` it carries and the
//!   [`DeclaredSurvivability`] the mission grants it. Aircraft, loadout and
//!   pilot are three separate ids; nothing here bakes an enemy status into a
//!   mesh or a paint colour (non-negotiable 1).
//! * [`DeclaredNeutralTraffic`] — one authored neutral actor. Neutral
//!   traffic exists only where the mission (or an explicitly enabled modern
//!   sandbox) declares it; an omitted list is an empty list, never a global
//!   population default (non-negotiable and AC04's contract half).
//!
//! # Identity is separate from appearance
//!
//! Pilot, faction and aircraft are *different* catalog namespaces
//! ([`ContentKind::Pilot`], [`ContentKind::Faction`],
//! [`ContentKind::Airframe`]) and the type system keeps them apart: every
//! [`DeclaredRoster::try_new`] validates the namespace of each referenced id,
//! so a faction id can never be stored in a pilot field. Capture therefore
//! changes a faction and nothing else — the airframe id a vehicle is built
//! from is a separate field (F33 AC01).
//!
//! # Designed vocabulary, not original data
//!
//! The original game's pilot roster, its wingmate assignments, aircraft,
//! loadouts, voices and survivability rules, and where it stores them, are
//! **unmeasured** (F33 "Research boundary"; F13 locates mission programs but
//! recovers no roster). Every value, slot grammar, survivability label and
//! fixture in this module is newly authored project design carrying
//! [`Origin::SyntheticFixture`] or [`Origin::Designed`] provenance, recorded in
//! `docs/findings/2026-10-01-f33-a-pilot-aircraft-faction-separation.md`.
//!
//! The tail of this module is different in kind: task #574's *measured*
//! retail surface — the decoded `zeppelins.zrd` mission carrier and what it
//! can and cannot supply the declared schema. Measured data and designed
//! vocabulary stay in separate types; nothing in the measured section is a
//! roster value.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use cs_assets::install;
use cs_formats::script_raw::discovery::discover_container;
use cs_formats::zbd::zeppelins::{
    ZEPPELINS_MEMBER, ZeppelinKey, ZeppelinMember, ZeppelinRecord, ZeppelinsError,
    read_zeppelins_member,
};
use cs_types::content::{ContentId, ContentKind, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::install::RelativePath;

/// What a mission declares about one allied actor's survival (F33
/// non-negotiable 4).
///
/// The three labels are designed engine vocabulary: whether the original game
/// makes an escort unkillable, why, and for which missions is unmeasured
/// (F33-D's retail stage). The point of the type is that "an ally" is not one
/// behavior: [`Mortal`](Self::Mortal) allies die for real,
/// [`ProtectedNeutral`](Self::ProtectedNeutral) losses are a mission event
/// rather than a kill, and only an explicitly authored
/// [`ScriptedInvulnerable`](Self::ScriptedInvulnerable) escort is unkillable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DeclaredSurvivability {
    /// The actor can be destroyed; its death is a real loss.
    Mortal,
    /// The actor is a protected neutral: its loss reports a mission event
    /// and is never counted as a kill.
    ProtectedNeutral,
    /// The mission authors the actor as unable to be destroyed.
    ScriptedInvulnerable,
}

impl DeclaredSurvivability {
    /// Every label, in a stable order.
    pub const ALL: &'static [DeclaredSurvivability] = &[
        Self::Mortal,
        Self::ProtectedNeutral,
        Self::ScriptedInvulnerable,
    ];

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Mortal => "mortal",
            Self::ProtectedNeutral => "protected_neutral",
            Self::ScriptedInvulnerable => "scripted_invulnerable",
        }
    }
}

impl fmt::Display for DeclaredSurvivability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The identity of one authored wingmate slot within a subject.
///
/// A wingmate assignment is mission-authored, not a catalog asset, so it
/// takes the same `u32` newtype the F32 [`FormationId`] does: stable within
/// a subject, comparable, and never a filename guess. The runtime mirrors it
/// as `cs_sim::allies::WingmateSlot`.
///
/// [`FormationId`]: super::ai::FormationId
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WingmateSlot(pub u32);

impl WingmateSlot {
    /// The slot's index within its subject.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.0
    }
}

impl fmt::Display for WingmateSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "wingmate slot {}", self.0)
    }
}

/// A declared pilot: an identity and the voice it speaks through.
///
/// Both are opaque catalog ids ([`ContentKind::Pilot`] and
/// [`ContentKind::Voice`]); the original roster's pilot names and their
/// voices are unmeasured, so these are designed ids, never guessed names.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredPilot {
    id: ContentId,
    voice: Resolved<ContentId>,
    origin: Origin,
    provenance: Provenance,
}

impl DeclaredPilot {
    /// Assembles and validates a declared pilot.
    ///
    /// # Errors
    ///
    /// [`RosterSchemaError::PilotKindMismatch`] when the id is not in the
    /// `pilot` namespace, and [`RosterSchemaError::VoiceKindMismatch`] when a
    /// *known* voice is not in the `voice` namespace. An unknown voice is
    /// valid here — it is an explicit unknown the lowering boundary refuses.
    pub fn try_new(
        id: ContentId,
        voice: Resolved<ContentId>,
        origin: Origin,
        provenance: Provenance,
    ) -> Result<Self, RosterSchemaError> {
        if id.kind() != ContentKind::Pilot {
            return Err(RosterSchemaError::PilotKindMismatch { pilot: id });
        }
        if let Resolved::Known(known) = &voice
            && known.value.kind() != ContentKind::Voice
        {
            return Err(RosterSchemaError::VoiceKindMismatch {
                pilot: id,
                voice: known.value.clone(),
            });
        }
        Ok(Self {
            id,
            voice,
            origin,
            provenance,
        })
    }

    /// The pilot's catalog id.
    #[must_use]
    pub fn id(&self) -> &ContentId {
        &self.id
    }

    /// The voice the pilot speaks through, or an explicit unknown.
    #[must_use]
    pub const fn voice(&self) -> &Resolved<ContentId> {
        &self.voice
    }

    /// Where the record came from.
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

/// One declared wingmate slot: *who* flies it, *what* it flies and the
/// survival policy the mission grants it.
///
/// `pilot`, `airframe` and `loadout` are three separate catalog ids in three
/// separate namespaces; the pilot is not the aircraft and the aircraft is not
/// the loadout (F33 deliverable). The survivability is a [`Resolved`] so an
/// unmeasured policy stays an explicit unknown rather than a silent mortal.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredWingmate {
    slot: WingmateSlot,
    pilot: ContentId,
    airframe: ContentId,
    loadout: ContentId,
    survivability: Resolved<DeclaredSurvivability>,
    origin: Origin,
    provenance: Provenance,
}

impl DeclaredWingmate {
    /// Assembles and validates a declared wingmate.
    ///
    /// # Errors
    ///
    /// [`RosterSchemaError::WingmateAirframeKindMismatch`] when `airframe` is
    /// not in the `airframe` namespace and
    /// [`RosterSchemaError::WingmateLoadoutKindMismatch`] when `loadout` is
    /// not in the `loadout` namespace. The `pilot` namespace and the
    /// declaration of that pilot are checked by
    /// [`DeclaredRoster::try_new`], which owns the pilot list.
    pub fn try_new(
        slot: WingmateSlot,
        pilot: ContentId,
        airframe: ContentId,
        loadout: ContentId,
        survivability: Resolved<DeclaredSurvivability>,
        origin: Origin,
        provenance: Provenance,
    ) -> Result<Self, RosterSchemaError> {
        if airframe.kind() != ContentKind::Airframe {
            return Err(RosterSchemaError::WingmateAirframeKindMismatch { airframe });
        }
        if loadout.kind() != ContentKind::Loadout {
            return Err(RosterSchemaError::WingmateLoadoutKindMismatch { loadout });
        }
        Ok(Self {
            slot,
            pilot,
            airframe,
            loadout,
            survivability,
            origin,
            provenance,
        })
    }

    /// The slot this assignment occupies.
    #[must_use]
    pub const fn slot(&self) -> WingmateSlot {
        self.slot
    }

    /// The pilot that flies this slot.
    #[must_use]
    pub fn pilot(&self) -> &ContentId {
        &self.pilot
    }

    /// The airframe the slot launches in.
    #[must_use]
    pub fn airframe(&self) -> &ContentId {
        &self.airframe
    }

    /// The loadout the slot carries.
    #[must_use]
    pub fn loadout(&self) -> &ContentId {
        &self.loadout
    }

    /// The declared survival policy, or an explicit unknown.
    #[must_use]
    pub const fn survivability(&self) -> &Resolved<DeclaredSurvivability> {
        &self.survivability
    }

    /// Where the record came from.
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

/// One declared neutral actor.
///
/// Neutral traffic is authored, per mission. An omitted [`DeclaredRoster`]
/// list is an empty list: nothing here is a global "population" a session
/// falls back on (F33 AC04). The pilot, airframe and faction are separate
/// catalog ids and the faction is the side this actor starts on — a captured
/// actor changes it at runtime, not here.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredNeutralTraffic {
    traffic: u32,
    pilot: ContentId,
    airframe: ContentId,
    faction: ContentId,
    survivability: Resolved<DeclaredSurvivability>,
    origin: Origin,
    provenance: Provenance,
}

impl DeclaredNeutralTraffic {
    /// Assembles and validates one declared neutral actor.
    ///
    /// # Errors
    ///
    /// [`RosterSchemaError::NeutralPilotKindMismatch`],
    /// [`RosterSchemaError::NeutralAirframeKindMismatch`] or
    /// [`RosterSchemaError::NeutralFactionKindMismatch`] when a referenced id
    /// is not in its own namespace.
    pub fn try_new(
        traffic: u32,
        pilot: ContentId,
        airframe: ContentId,
        faction: ContentId,
        survivability: Resolved<DeclaredSurvivability>,
        origin: Origin,
        provenance: Provenance,
    ) -> Result<Self, RosterSchemaError> {
        if pilot.kind() != ContentKind::Pilot {
            return Err(RosterSchemaError::NeutralPilotKindMismatch { pilot });
        }
        if airframe.kind() != ContentKind::Airframe {
            return Err(RosterSchemaError::NeutralAirframeKindMismatch { airframe });
        }
        if faction.kind() != ContentKind::Faction {
            return Err(RosterSchemaError::NeutralFactionKindMismatch { faction });
        }
        Ok(Self {
            traffic,
            pilot,
            airframe,
            faction,
            survivability,
            origin,
            provenance,
        })
    }

    /// The mission-authored traffic index.
    #[must_use]
    pub const fn traffic(&self) -> u32 {
        self.traffic
    }

    /// The pilot that flies this actor.
    #[must_use]
    pub fn pilot(&self) -> &ContentId {
        &self.pilot
    }

    /// The airframe this actor flies.
    #[must_use]
    pub fn airframe(&self) -> &ContentId {
        &self.airframe
    }

    /// The faction this actor starts on.
    #[must_use]
    pub fn faction(&self) -> &ContentId {
        &self.faction
    }

    /// The declared survival policy, or an explicit unknown.
    #[must_use]
    pub const fn survivability(&self) -> &Resolved<DeclaredSurvivability> {
        &self.survivability
    }

    /// Where the record came from.
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

/// Why a declared roster record was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum RosterSchemaError {
    /// The roster's subject is neither a mission nor a launchable scenario.
    SubjectKind {
        /// The offending subject id.
        subject: ContentId,
    },
    /// The declared player faction is not in the `faction` namespace.
    PlayerFactionKindMismatch {
        /// The offending id.
        faction: ContentId,
    },
    /// A pilot entry's id is not in the `pilot` namespace.
    PilotKindMismatch {
        /// The offending id.
        pilot: ContentId,
    },
    /// Two pilot entries share one id.
    DuplicatePilot {
        /// The duplicated id.
        pilot: ContentId,
    },
    /// A known pilot voice is not in the `voice` namespace.
    VoiceKindMismatch {
        /// The pilot the voice belongs to.
        pilot: ContentId,
        /// The offending voice id.
        voice: ContentId,
    },
    /// A wingmate's airframe is not in the `airframe` namespace.
    WingmateAirframeKindMismatch {
        /// The offending id.
        airframe: ContentId,
    },
    /// A wingmate's loadout is not in the `loadout` namespace.
    WingmateLoadoutKindMismatch {
        /// The offending id.
        loadout: ContentId,
    },
    /// Two wingmate assignments share one slot.
    DuplicateWingmateSlot {
        /// The duplicated slot.
        slot: WingmateSlot,
    },
    /// A wingmate names a pilot the roster never declared.
    UndeclaredWingmatePilot {
        /// The assignment's slot.
        slot: WingmateSlot,
        /// The pilot that is not declared.
        pilot: ContentId,
    },
    /// The declared pilot id is not in the `pilot` namespace (checked while
    /// cross-referencing).
    WingmatePilotKindMismatch {
        /// The offending id.
        pilot: ContentId,
    },
    /// A neutral actor's pilot is not in the `pilot` namespace.
    NeutralPilotKindMismatch {
        /// The offending id.
        pilot: ContentId,
    },
    /// A neutral actor's airframe is not in the `airframe` namespace.
    NeutralAirframeKindMismatch {
        /// The offending id.
        airframe: ContentId,
    },
    /// A neutral actor's faction is not in the `faction` namespace.
    NeutralFactionKindMismatch {
        /// The offending id.
        faction: ContentId,
    },
    /// Two neutral actors share one traffic index.
    DuplicateNeutralTraffic {
        /// The duplicated index.
        traffic: u32,
    },
}

impl fmt::Display for RosterSchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SubjectKind { subject } => {
                write!(
                    f,
                    "roster subject {subject} is neither a mission nor a launchable"
                )
            }
            Self::PlayerFactionKindMismatch { faction } => write!(
                f,
                "player faction {faction} is not in the faction namespace"
            ),
            Self::PilotKindMismatch { pilot } => {
                write!(f, "pilot entry {pilot} is not in the pilot namespace")
            }
            Self::DuplicatePilot { pilot } => {
                write!(f, "pilot {pilot} is declared more than once")
            }
            Self::VoiceKindMismatch { pilot, voice } => {
                write!(f, "pilot {pilot} names voice {voice}, which is not a voice")
            }
            Self::WingmateAirframeKindMismatch { airframe } => write!(
                f,
                "wingmate airframe {airframe} is not in the airframe namespace"
            ),
            Self::WingmateLoadoutKindMismatch { loadout } => write!(
                f,
                "wingmate loadout {loadout} is not in the loadout namespace"
            ),
            Self::DuplicateWingmateSlot { slot } => {
                write!(f, "{slot} is declared more than once")
            }
            Self::UndeclaredWingmatePilot { slot, pilot } => {
                write!(
                    f,
                    "{slot} names pilot {pilot}, which the roster never declared"
                )
            }
            Self::WingmatePilotKindMismatch { pilot } => {
                write!(f, "wingmate pilot {pilot} is not in the pilot namespace")
            }
            Self::NeutralPilotKindMismatch { pilot } => {
                write!(
                    f,
                    "neutral traffic pilot {pilot} is not in the pilot namespace"
                )
            }
            Self::NeutralAirframeKindMismatch { airframe } => write!(
                f,
                "neutral traffic airframe {airframe} is not in the airframe namespace"
            ),
            Self::NeutralFactionKindMismatch { faction } => write!(
                f,
                "neutral traffic faction {faction} is not in the faction namespace"
            ),
            Self::DuplicateNeutralTraffic { traffic } => {
                write!(f, "neutral traffic #{traffic} is declared more than once")
            }
        }
    }
}

impl std::error::Error for RosterSchemaError {}

/// The declared pilot/wingmate/neutral-traffic roster of one catalog subject.
///
/// `subject` is the catalog id the roster belongs to — a `mission` id for a
/// campaign mission, an `ia_scenario` id for an Instant Action scenario — so
/// the record shares the catalog's identity discipline. An omitted list is an
/// empty list by construction: no field defaults to a population.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredRoster {
    subject: ContentId,
    origin: Origin,
    player_faction: ContentId,
    pilots: Vec<DeclaredPilot>,
    wingmates: Vec<DeclaredWingmate>,
    neutral_traffic: Vec<DeclaredNeutralTraffic>,
    provenance: Provenance,
}

impl DeclaredRoster {
    /// Assembles and validates a declared roster.
    ///
    /// # Errors
    ///
    /// [`RosterSchemaError::SubjectKind`] for a subject that is neither a
    /// mission nor a launchable scenario,
    /// [`RosterSchemaError::PlayerFactionKindMismatch`] for a non-faction
    /// player side, [`RosterSchemaError::DuplicatePilot`] /
    /// [`RosterSchemaError::DuplicateWingmateSlot`] /
    /// [`RosterSchemaError::DuplicateNeutralTraffic`] for a repeated id, and
    /// [`RosterSchemaError::UndeclaredWingmatePilot`] (or the matching
    /// namespace error) for a wingmate that names an undeclared or
    /// wrong-namespace pilot.
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        subject: ContentId,
        origin: Origin,
        player_faction: ContentId,
        pilots: Vec<DeclaredPilot>,
        wingmates: Vec<DeclaredWingmate>,
        neutral_traffic: Vec<DeclaredNeutralTraffic>,
        provenance: Provenance,
    ) -> Result<Self, RosterSchemaError> {
        if subject.kind() != ContentKind::Mission && subject.kind() != ContentKind::IaScenario {
            return Err(RosterSchemaError::SubjectKind { subject });
        }
        if player_faction.kind() != ContentKind::Faction {
            return Err(RosterSchemaError::PlayerFactionKindMismatch {
                faction: player_faction,
            });
        }

        let mut declared_pilots = BTreeSet::new();
        for pilot in &pilots {
            if !declared_pilots.insert(pilot.id.clone()) {
                return Err(RosterSchemaError::DuplicatePilot {
                    pilot: pilot.id.clone(),
                });
            }
        }

        let mut slots = BTreeSet::new();
        for wingmate in &wingmates {
            if wingmate.pilot.kind() != ContentKind::Pilot {
                return Err(RosterSchemaError::WingmatePilotKindMismatch {
                    pilot: wingmate.pilot.clone(),
                });
            }
            if !declared_pilots.contains(&wingmate.pilot) {
                return Err(RosterSchemaError::UndeclaredWingmatePilot {
                    slot: wingmate.slot,
                    pilot: wingmate.pilot.clone(),
                });
            }
            if !slots.insert(wingmate.slot) {
                return Err(RosterSchemaError::DuplicateWingmateSlot {
                    slot: wingmate.slot,
                });
            }
        }

        let mut traffic = BTreeSet::new();
        for neutral in &neutral_traffic {
            if !traffic.insert(neutral.traffic) {
                return Err(RosterSchemaError::DuplicateNeutralTraffic {
                    traffic: neutral.traffic,
                });
            }
        }

        Ok(Self {
            subject,
            origin,
            player_faction,
            pilots,
            wingmates,
            neutral_traffic,
            provenance,
        })
    }

    /// The catalog id the roster belongs to.
    #[must_use]
    pub fn subject(&self) -> &ContentId {
        &self.subject
    }

    /// Where the record came from.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The faction the mission's player and wingmates fly for.
    #[must_use]
    pub fn player_faction(&self) -> &ContentId {
        &self.player_faction
    }

    /// The declared pilots, in authored order.
    #[must_use]
    pub fn pilots(&self) -> &[DeclaredPilot] {
        &self.pilots
    }

    /// The declared wingmate assignments, in authored order.
    #[must_use]
    pub fn wingmates(&self) -> &[DeclaredWingmate] {
        &self.wingmates
    }

    /// The declared neutral actors, in authored order.
    #[must_use]
    pub fn neutral_traffic(&self) -> &[DeclaredNeutralTraffic] {
        &self.neutral_traffic
    }

    /// Where the record itself came from.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

// ----------------------------------------------------------- fixture ------

fn design_claim(key: &str) -> Provenance {
    Provenance::designed(ClaimId::new(key).expect("fixture claim id is valid"))
}

fn known<T>(value: T, key: &str) -> Resolved<T> {
    Resolved::Known(cs_types::content::Known::new(value, design_claim(key)))
}

/// The minimal synthetic pilot roster: two player pilots (an ace and a
/// wingman), two wingmate slots and one neutral trader.
///
/// Newly authored fixture content with designed provenance — never a
/// stand-in for the original game's roster, which is unmeasured.
#[must_use]
pub fn declared_synthetic_roster() -> DeclaredRoster {
    let subject = ContentId::from_source(ContentKind::IaScenario, "synthetic.roster")
        .expect("fixture subject id is valid");
    let player = ContentId::from_source(ContentKind::Faction, "synthetic.nathan")
        .expect("fixture faction id is valid");

    let nathan = DeclaredPilot::try_new(
        ContentId::from_source(ContentKind::Pilot, "synthetic.nathan").expect("pilot id"),
        known(
            ContentId::from_source(ContentKind::Voice, "synthetic.nathan").expect("voice id"),
            "f33a.synthetic.nathan.voice",
        ),
        Origin::SyntheticFixture,
        design_claim("f33a.synthetic.nathan"),
    )
    .expect("the fixture pilot is valid");

    let betty = DeclaredPilot::try_new(
        ContentId::from_source(ContentKind::Pilot, "synthetic.betty").expect("pilot id"),
        known(
            ContentId::from_source(ContentKind::Voice, "synthetic.betty").expect("voice id"),
            "f33a.synthetic.betty.voice",
        ),
        Origin::SyntheticFixture,
        design_claim("f33a.synthetic.betty"),
    )
    .expect("the fixture pilot is valid");

    let wingmates = vec![
        DeclaredWingmate::try_new(
            WingmateSlot(1),
            betty.id().clone(),
            ContentId::from_source(ContentKind::Airframe, "synthetic.devastator")
                .expect("airframe"),
            ContentId::from_source(ContentKind::Loadout, "synthetic.escort").expect("loadout"),
            known(
                DeclaredSurvivability::Mortal,
                "f33a.synthetic.betty.survivability",
            ),
            Origin::SyntheticFixture,
            design_claim("f33a.synthetic.wingmate.1"),
        )
        .expect("the fixture wingmate is valid"),
        DeclaredWingmate::try_new(
            WingmateSlot(2),
            nathan.id().clone(),
            ContentId::from_source(ContentKind::Airframe, "synthetic.fury").expect("airframe"),
            ContentId::from_source(ContentKind::Loadout, "synthetic.interceptor").expect("loadout"),
            known(
                DeclaredSurvivability::ScriptedInvulnerable,
                "f33a.synthetic.nathan.survivability",
            ),
            Origin::SyntheticFixture,
            design_claim("f33a.synthetic.wingmate.2"),
        )
        .expect("the fixture wingmate is valid"),
    ];

    let neutral_traffic = vec![
        DeclaredNeutralTraffic::try_new(
            1,
            ContentId::from_source(ContentKind::Pilot, "synthetic.trader").expect("pilot id"),
            ContentId::from_source(ContentKind::Airframe, "synthetic.freighter").expect("airframe"),
            ContentId::from_source(ContentKind::Faction, "synthetic.traders").expect("faction"),
            known(
                DeclaredSurvivability::ProtectedNeutral,
                "f33a.synthetic.trader.survivability",
            ),
            Origin::SyntheticFixture,
            design_claim("f33a.synthetic.traffic.1"),
        )
        .expect("the fixture neutral actor is valid"),
    ];

    DeclaredRoster::try_new(
        subject,
        Origin::SyntheticFixture,
        player,
        vec![nathan, betty],
        wingmates,
        neutral_traffic,
        design_claim("f33a.synthetic.roster"),
    )
    .expect("the fixture roster is valid")
}

// --------------------- measured `zeppelins.zrd` carrier (task #574) --------
//
// Everything above is designed vocabulary. Everything below is measurement:
// the decoded mission carrier, a census over the installation, and the one
// question this module must answer honestly — what a carrier record supplies
// toward [`DeclaredNeutralTraffic`].
//
// The measured facts (task #574, over the owner's retail installation):
//
// * 50 of 53 mission `zrdr.zbd` archives carry `zeppelins.zrd`; `c1/m02`,
//   `c2/m01` and `c5/mp2` are the three that do not. No installation-scope
//   archive carries it (F33-D's census established the same counts without
//   decoding the member).
// * All 50 members decode under the `.zrd` grammar: **58 records** in all —
//   12 members carry zero (the `c*/mp1` and `c*/mp2` multiplayer missions),
//   23 carry one, 11 carry two, three carry three and one carries four.
// * A record is one placed zeppelin: a world-`node` binding, a pose, motion
//   tuning, a `net` name, gasbag/engine/cannon bindings, `targets` that name
//   `player` or a sibling record's `node`, and — on 16 records — a `team`
//   spelling (`ally` 12, `enemy` 4).
//
// What is **not** established: what any of it means to the original, which
// records — if any — the original treats as neutral traffic, and how a
// record maps to the declared schema's catalog ids. The support evaluation
// below keeps that negative explicit.

/// One input a [`DeclaredNeutralTraffic`] record needs — what a decoded
/// carrier record would have to supply to lower into one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NeutralTrafficField {
    /// The mission-authored traffic index.
    Traffic,
    /// The pilot that flies the actor.
    Pilot,
    /// The airframe the actor flies.
    Airframe,
    /// The faction the actor starts on.
    Faction,
    /// The declared survival policy.
    Survivability,
}

impl NeutralTrafficField {
    /// Every input, in declared order.
    pub const ALL: [Self; 5] = [
        Self::Traffic,
        Self::Pilot,
        Self::Airframe,
        Self::Faction,
        Self::Survivability,
    ];
}

/// What one decoded `zeppelins.zrd` record supplies toward one
/// [`DeclaredNeutralTraffic`] field — measured over the retail corpus, never
/// guessed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CarrierFieldSupport {
    /// The record's encoding carries a field that supplies this input. No
    /// measured key supplies any declared input today; the variant exists so
    /// a future measurement names the key instead of widening the claim.
    Supplied {
        /// The measured key that carries it.
        key: ZeppelinKey,
    },
    /// The record carries authored data near the input — vocabulary a naive
    /// wiring would reach for, but not the input itself.
    Nearby {
        /// The measured key whose data sits nearest.
        key: ZeppelinKey,
        /// What the key actually carries, and why it is not the input.
        note: &'static str,
    },
    /// Nothing in the measured encoding supplies this input.
    Absent,
}

/// One decoded carrier record evaluated against the declared
/// neutral-traffic schema (task #574).
///
/// The evaluation is per-field, never a boolean guess: [`Self::can_lower`]
/// is `true` only when every [`NeutralTrafficField`] reports
/// [`CarrierFieldSupport::Supplied`], which no measured record achieves —
/// the member names placed zeppelins, not neutral-traffic declarations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NeutralTrafficSupport {
    support: [(NeutralTrafficField, CarrierFieldSupport); 5],
    node: String,
    team: Option<String>,
}

impl NeutralTrafficSupport {
    /// The record's `node` spelling, kept so a report can name which record
    /// was evaluated.
    pub fn node(&self) -> &str {
        &self.node
    }

    /// The record's `team` spelling, when it states one — mission
    /// vocabulary, reported verbatim, not a faction.
    pub fn team(&self) -> Option<&str> {
        self.team.as_deref()
    }

    /// The support one field reports.
    pub fn support_for(&self, field: NeutralTrafficField) -> CarrierFieldSupport {
        self.support
            .iter()
            .find(|(name, _)| *name == field)
            .map(|(_, support)| *support)
            .expect("the table covers every field")
    }

    /// The fields the record supplies outright.
    pub fn supplied(&self) -> impl Iterator<Item = NeutralTrafficField> + '_ {
        self.support
            .iter()
            .filter(|(_, support)| matches!(support, CarrierFieldSupport::Supplied { .. }))
            .map(|(field, _)| *field)
    }

    /// The fields nothing in the measured encoding supplies.
    pub fn missing(&self) -> impl Iterator<Item = NeutralTrafficField> + '_ {
        self.support
            .iter()
            .filter(|(_, support)| matches!(support, CarrierFieldSupport::Absent))
            .map(|(field, _)| *field)
    }

    /// Whether the record can lower into a [`DeclaredNeutralTraffic`] without
    /// inventing data. `false` for every measured record.
    pub fn can_lower(&self) -> bool {
        self.missing().next().is_none() && self.nearby().next().is_none()
    }

    /// The fields the record carries only nearby vocabulary for.
    pub fn nearby(&self) -> impl Iterator<Item = NeutralTrafficField> + '_ {
        self.support
            .iter()
            .filter(|(_, support)| matches!(support, CarrierFieldSupport::Nearby { .. }))
            .map(|(field, _)| *field)
    }
}

/// Evaluates one decoded `zeppelins.zrd` record against the declared
/// neutral-traffic schema.
///
/// The result is the measured negative, made explicit:
///
/// * `node` sits nearest [`NeutralTrafficField::Airframe`] — it is the
///   record's world-node binding (a placed instance's name), not an airframe
///   catalog id, and which airframe it names is unmeasured;
/// * `team`, when stated, sits nearest [`NeutralTrafficField::Faction`] —
///   it is mission vocabulary (`ally`/`enemy` measured), not a faction
///   catalog id;
/// * every other field is [`CarrierFieldSupport::Absent`]: the encoding
///   carries no traffic index, no pilot and no survival policy. A lowering
///   could invent an ordinal or a default, but that would be design — the
///   same class of data the declared fixture authors — not a measurement of
///   the original.
#[must_use]
pub fn neutral_traffic_support(record: &ZeppelinRecord) -> NeutralTrafficSupport {
    let support = [
        (NeutralTrafficField::Traffic, CarrierFieldSupport::Absent),
        (NeutralTrafficField::Pilot, CarrierFieldSupport::Absent),
        (
            NeutralTrafficField::Airframe,
            CarrierFieldSupport::Nearby {
                key: ZeppelinKey::Node,
                note: "the `node` spelling is the record's world-node binding (a placed \
                       instance's name), not an airframe catalog id; which airframe it \
                       names is unmeasured",
            },
        ),
        (
            NeutralTrafficField::Faction,
            match record.team() {
                Some(_) => CarrierFieldSupport::Nearby {
                    key: ZeppelinKey::Team,
                    note: "the `team` spelling is mission vocabulary (`ally`/`enemy` \
                           measured), not a faction catalog id",
                },
                None => CarrierFieldSupport::Absent,
            },
        ),
        (
            NeutralTrafficField::Survivability,
            CarrierFieldSupport::Absent,
        ),
    ];
    NeutralTrafficSupport {
        support,
        node: record.node().to_owned(),
        team: record.team().map(str::to_owned),
    }
}

/// Why the decoded-carrier census could not be produced.
#[derive(Clone, Debug, PartialEq)]
pub enum CarrierCensusError {
    /// The installation could not be discovered.
    Discovery(String),
    /// An archive the installation declares could not be read, or discovery
    /// inventoried no such archive.
    Read {
        /// The installation-relative archive.
        container: String,
        /// Why the read failed.
        reason: String,
    },
    /// A discovered archive spelling is not a usable relative path.
    Path {
        /// The installation-relative archive.
        container: String,
        /// Why the spelling was refused.
        reason: String,
    },
    /// The archive is not readable as a reader container: discovery reported
    /// findings and located no members, so "the member is absent" cannot be
    /// claimed.
    NotAReaderArchive {
        /// The installation-relative archive.
        container: String,
        /// Why discovery refused it.
        reason: String,
    },
    /// The mission's `zeppelins.zrd` member refused to decode — a named
    /// failure, never a skipped member.
    Decode {
        /// The installation-relative archive.
        container: String,
        /// The decoder's named refusal.
        error: ZeppelinsError,
    },
}

impl fmt::Display for CarrierCensusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discovery(reason) => write!(f, "the installation is undiscoverable: {reason}"),
            Self::Read { container, reason } => {
                write!(f, "archive {container} could not be read: {reason}")
            }
            Self::Path { container, reason } => {
                write!(
                    f,
                    "archive {container} is not a usable relative path: {reason}"
                )
            }
            Self::NotAReaderArchive { container, reason } => {
                write!(f, "archive {container} is not a reader archive: {reason}")
            }
            Self::Decode { container, error } => {
                write!(
                    f,
                    "{ZEPPELINS_MEMBER} in {container} refused to decode: {error}"
                )
            }
        }
    }
}

impl std::error::Error for CarrierCensusError {}

/// One mission's carrier row: whether its archive carries `zeppelins.zrd`,
/// and the decoded member when it does.
#[derive(Clone, Debug, PartialEq)]
pub struct RetailZeppelinCarrier {
    group: String,
    mission: String,
    archive: String,
    archive_sha256: String,
    member: Option<ZeppelinMember>,
}

impl RetailZeppelinCarrier {
    /// The world group directory (`c1`, `c2`, ...).
    pub fn group(&self) -> &str {
        &self.group
    }

    /// The mission directory (`m01`, `mp3`, ...).
    pub fn mission(&self) -> &str {
        &self.mission
    }

    /// The installation-relative archive the row reports.
    pub fn archive(&self) -> &str {
        &self.archive
    }

    /// The archive's SHA-256, so the row's number traces to bytes.
    pub fn archive_sha256(&self) -> &str {
        &self.archive_sha256
    }

    /// Whether the archive carries [`ZEPPELINS_MEMBER`].
    pub const fn carrier_present(&self) -> bool {
        self.member.is_some()
    }

    /// The decoded member, when the archive carries it.
    pub const fn member(&self) -> Option<&ZeppelinMember> {
        self.member.as_ref()
    }

    /// How many records the member carries (`0` when the member is absent or
    /// states an empty record list — distinguished by
    /// [`Self::carrier_present`]).
    pub fn record_count(&self) -> usize {
        self.member.as_ref().map_or(0, ZeppelinMember::len)
    }
}

/// The decoded `zeppelins.zrd` census over one installation: one row per
/// mission directory, plus whether any installation-scope archive carries
/// the member.
#[derive(Clone, Debug, PartialEq)]
pub struct RetailZeppelinCarrierCensus {
    install_sha256: String,
    content_sha256: String,
    carrier_member: &'static str,
    installation_scope_carrier: bool,
    missions: Vec<RetailZeppelinCarrier>,
    team_spellings: BTreeMap<String, u64>,
}

impl RetailZeppelinCarrierCensus {
    /// The installation fingerprint the census was measured over.
    pub fn install_sha256(&self) -> &str {
        &self.install_sha256
    }

    /// The canonical-content fingerprint.
    pub fn content_sha256(&self) -> &str {
        &self.content_sha256
    }

    /// The member name the census decodes.
    pub const fn carrier_member(&self) -> &'static str {
        self.carrier_member
    }

    /// Whether the root or any world-group `zrdr.zbd` carries the member:
    /// measured `false` over the retail installation.
    pub const fn installation_scope_carrier(&self) -> bool {
        self.installation_scope_carrier
    }

    /// The mission rows, in sorted `(group, mission)` order.
    pub fn missions(&self) -> &[RetailZeppelinCarrier] {
        &self.missions
    }

    /// The mission rows whose archive carries no member.
    pub fn missions_without_carrier(&self) -> impl Iterator<Item = &RetailZeppelinCarrier> {
        self.missions.iter().filter(|row| !row.carrier_present())
    }

    /// The mission rows whose archive carries a member.
    pub fn missions_with_carrier(&self) -> impl Iterator<Item = &RetailZeppelinCarrier> {
        self.missions.iter().filter(|row| row.carrier_present())
    }

    /// Records across every carrying member.
    pub fn record_count(&self) -> usize {
        self.missions
            .iter()
            .map(RetailZeppelinCarrier::record_count)
            .sum()
    }

    /// The `team` spellings the decoded records carry, with their counts —
    /// measured vocabulary, not an interpretation of it.
    pub fn team_spellings(&self) -> &BTreeMap<String, u64> {
        &self.team_spellings
    }

    /// Every decoded record across the carrying missions, with its mission.
    pub fn records(&self) -> impl Iterator<Item = (&RetailZeppelinCarrier, &ZeppelinRecord)> {
        self.missions
            .iter()
            .filter_map(|row| row.member().map(|member| (row, member)))
            .flat_map(|(row, member)| member.records().iter().map(move |record| (row, record)))
    }
}

/// One archive's member scan: whether the carrier is present and its decode.
struct MemberScan {
    sha256: String,
    member: Option<ZeppelinMember>,
}

/// Reads one installation archive through the production reader-archive
/// discovery and decodes its `zeppelins.zrd` member when present.
///
/// # Errors
///
/// [`CarrierCensusError`] when the installation declares no such archive, it
/// cannot be read or located, it is not a reader archive at all, or the
/// member refuses to decode. A failed read is a refusal, never an absent
/// member; a member that will not decode is a named failure, never a skipped
/// one.
fn scan_carrier(
    found: &install::Discovery,
    container: &str,
) -> Result<MemberScan, CarrierCensusError> {
    let record = found
        .manifest
        .files
        .iter()
        .find(|record| record.relative_spelling.logical_key() == container)
        .ok_or_else(|| CarrierCensusError::Read {
            container: container.to_owned(),
            reason: "production discovery inventoried no such archive".to_owned(),
        })?;
    let bytes = std::fs::read(
        found
            .manifest
            .host_root
            .join(record.relative_spelling.as_str()),
    )
    .map_err(|error| CarrierCensusError::Read {
        container: container.to_owned(),
        reason: error.to_string(),
    })?;
    let path = RelativePath::new(record.relative_spelling.as_str()).map_err(|error| {
        CarrierCensusError::Path {
            container: container.to_owned(),
            reason: error.to_string(),
        }
    })?;
    let discovery = discover_container(container, &path, &bytes);
    if discovery.programs().is_empty() && !discovery.findings().is_empty() {
        let reason = discovery
            .findings()
            .iter()
            .map(|finding| format!("{finding:?}"))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(CarrierCensusError::NotAReaderArchive {
            container: container.to_owned(),
            reason,
        });
    }
    let member = discovery
        .programs()
        .iter()
        .find(|program| program.locator().member() == Some(ZEPPELINS_MEMBER))
        .map(|program| {
            read_zeppelins_member(program.bytes()).map_err(|error| CarrierCensusError::Decode {
                container: container.to_owned(),
                error,
            })
        })
        .transpose()?;
    Ok(MemberScan {
        sha256: record.sha256.to_hex(),
        member,
    })
}

/// Decodes the original installation's per-mission `zeppelins.zrd` carrier
/// (task #574).
///
/// One production discovery, one production reader-archive discovery per
/// archive, and the production [`read_zeppelins_member`] decode per carrying
/// member. Each mission row carries its archive key, SHA-256 and decoded
/// member, so every count traces to bytes; the installation and canonical
/// content fingerprints cover the whole source.
///
/// # Errors
///
/// [`CarrierCensusError`] on an undiscoverable installation, an archive that
/// cannot be read or located, an archive that is not a reader container, or
/// a member that refuses to decode. The census does not turn a failed read
/// into a shorter list or a failed decode into a skipped member.
pub fn survey_retail_zeppelin_carrier(
    install_root: &Path,
) -> Result<RetailZeppelinCarrierCensus, CarrierCensusError> {
    let found = install::discover(install_root)
        .map_err(|error| CarrierCensusError::Discovery(error.to_string()))?;
    let install_sha256 = install::fingerprint(&found.manifest).to_hex();
    let content_sha256 = install::content_fingerprint(&found.manifest).to_hex();

    // Every `ZBD/<group>/<mission>` directory, from the production diagnosis
    // (the same set F33-D's census walked: directories are authoritative).
    let mut mission_dirs: Vec<(String, String)> = Vec::new();
    let mut groups: Vec<String> = Vec::new();
    for directory in &found.diagnosis.directories {
        let key = directory.logical_key();
        let parts: Vec<&str> = key.split('/').collect();
        match parts.as_slice() {
            ["zbd", group] => groups.push((*group).to_owned()),
            ["zbd", group, mission] if !group.is_empty() && !mission.is_empty() => {
                mission_dirs.push(((*group).to_owned(), (*mission).to_owned()));
            }
            _ => {}
        }
    }
    mission_dirs.sort();
    groups.sort();
    groups.dedup();

    // The installation-scope archives: the root and every world group. A
    // carrier here is decoded the same way — measured absent, but the check
    // is the same measurement, not an assumption.
    let mut scope_keys: Vec<String> = vec!["zbd/zrdr.zbd".to_owned()];
    for group in &groups {
        scope_keys.push(format!("zbd/{group}/zrdr.zbd"));
    }
    let mut installation_scope_carrier = false;
    for key in &scope_keys {
        if scan_carrier(&found, key)?.member.is_some() {
            installation_scope_carrier = true;
        }
    }

    let mut missions = Vec::with_capacity(mission_dirs.len());
    for (group, mission) in mission_dirs {
        let archive = format!("zbd/{group}/{mission}/zrdr.zbd");
        let scan = scan_carrier(&found, &archive)?;
        missions.push(RetailZeppelinCarrier {
            group,
            mission,
            archive,
            archive_sha256: scan.sha256,
            member: scan.member,
        });
    }

    let mut team_spellings: BTreeMap<String, u64> = BTreeMap::new();
    for row in &missions {
        if let Some(member) = row.member() {
            for record in member.records() {
                if let Some(team) = record.team() {
                    *team_spellings.entry(team.to_owned()).or_default() += 1;
                }
            }
        }
    }

    Ok(RetailZeppelinCarrierCensus {
        install_sha256,
        content_sha256,
        carrier_member: ZEPPELINS_MEMBER,
        installation_scope_carrier,
        missions,
        team_spellings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(kind: ContentKind, key: &str) -> ContentId {
        ContentId::from_source(kind, key).expect("test id is valid")
    }

    fn pilot(key: &str) -> DeclaredPilot {
        DeclaredPilot::try_new(
            id(ContentKind::Pilot, key),
            known(id(ContentKind::Voice, key), "f33a.test.pilot.voice"),
            Origin::SyntheticFixture,
            design_claim("f33a.test.pilot"),
        )
        .expect("test pilot is valid")
    }

    /// The fixture roster is structurally valid and keeps pilot, aircraft and
    /// faction in separate namespaces; a wrong-namespace id is refused at
    /// construction, so a faction can never be stored where a pilot belongs.
    #[test]
    fn accept_f33_a_declared_roster_keeps_identity_namespaces_apart() {
        let roster = declared_synthetic_roster();
        assert_eq!(roster.subject().as_str(), "ia_scenario/synthetic.roster");
        assert_eq!(roster.player_faction().as_str(), "faction/synthetic.nathan");
        assert_eq!(roster.pilots().len(), 2);
        assert_eq!(roster.wingmates().len(), 2);
        assert_eq!(roster.neutral_traffic().len(), 1);
        for pilot in roster.pilots() {
            assert_eq!(pilot.id().kind(), ContentKind::Pilot);
        }

        let bad = DeclaredPilot::try_new(
            id(ContentKind::Faction, "synthetic.nathan"),
            Resolved::unknown(
                ClaimId::new("f33a.test.unknown").expect("claim"),
                "unmeasured",
            )
            .expect("reason"),
            Origin::SyntheticFixture,
            design_claim("f33a.test.pilot"),
        );
        assert_eq!(
            bad,
            Err(RosterSchemaError::PilotKindMismatch {
                pilot: id(ContentKind::Faction, "synthetic.nathan"),
            }),
            "a faction id cannot be a pilot"
        );

        let bad_voice = DeclaredPilot::try_new(
            id(ContentKind::Pilot, "synthetic.pilot"),
            known(
                id(ContentKind::Faction, "synthetic.nathan"),
                "f33a.test.voice",
            ),
            Origin::SyntheticFixture,
            design_claim("f33a.test.pilot"),
        );
        assert_eq!(
            bad_voice,
            Err(RosterSchemaError::VoiceKindMismatch {
                pilot: id(ContentKind::Pilot, "synthetic.pilot"),
                voice: id(ContentKind::Faction, "synthetic.nathan"),
            }),
            "a faction id cannot be a voice"
        );
    }

    /// Slot and pilot uniqueness and the cross-reference are enforced: two
    /// wingmates cannot share a slot, and a wingmate may not name a pilot the
    /// roster never declared.
    #[test]
    fn accept_f33_a_declared_roster_refuses_duplicate_slots_and_undeclared_pilots() {
        let nathan = pilot("synthetic.nathan");
        let wingmate = |slot: u32| {
            DeclaredWingmate::try_new(
                WingmateSlot(slot),
                id(ContentKind::Pilot, "synthetic.nathan"),
                id(ContentKind::Airframe, "synthetic.fury"),
                id(ContentKind::Loadout, "synthetic.interceptor"),
                known(DeclaredSurvivability::Mortal, "f33a.test.survivability"),
                Origin::SyntheticFixture,
                design_claim("f33a.test.wingmate"),
            )
            .expect("test wingmate is valid")
        };

        let duplicate_slot = DeclaredRoster::try_new(
            id(ContentKind::IaScenario, "synthetic.test"),
            Origin::SyntheticFixture,
            id(ContentKind::Faction, "synthetic.nathan"),
            vec![nathan],
            vec![wingmate(1), wingmate(1)],
            Vec::new(),
            design_claim("f33a.test.roster"),
        );
        assert_eq!(
            duplicate_slot,
            Err(RosterSchemaError::DuplicateWingmateSlot {
                slot: WingmateSlot(1)
            })
        );

        let undeclared = DeclaredRoster::try_new(
            id(ContentKind::IaScenario, "synthetic.test"),
            Origin::SyntheticFixture,
            id(ContentKind::Faction, "synthetic.nathan"),
            vec![pilot("synthetic.other")],
            vec![wingmate(1)],
            Vec::new(),
            design_claim("f33a.test.roster"),
        );
        assert_eq!(
            undeclared,
            Err(RosterSchemaError::UndeclaredWingmatePilot {
                slot: WingmateSlot(1),
                pilot: id(ContentKind::Pilot, "synthetic.nathan"),
            })
        );

        let wrong_subject = DeclaredRoster::try_new(
            id(ContentKind::Airframe, "synthetic.fury"),
            Origin::SyntheticFixture,
            id(ContentKind::Faction, "synthetic.nathan"),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            design_claim("f33a.test.roster"),
        );
        assert_eq!(
            wrong_subject,
            Err(RosterSchemaError::SubjectKind {
                subject: id(ContentKind::Airframe, "synthetic.fury")
            }),
            "only a mission or a launchable scenario can be a roster subject"
        );
    }

    /// An unmeasured survivability is an explicit unknown, not a silent
    /// mortal: the record constructs, keeps the unknown, and the boundary is
    /// left to refuse it (never this schema inventing a default).
    #[test]
    fn accept_f33_a_unmeasured_survivability_stays_an_explicit_unknown() {
        let unknown = Resolved::<DeclaredSurvivability>::unknown(
            ClaimId::new("f33a.test.survivability").expect("claim"),
            "the original escort policy is unmeasured",
        )
        .expect("a reason is present");
        assert!(!unknown.is_known());

        let wingmate = DeclaredWingmate::try_new(
            WingmateSlot(1),
            id(ContentKind::Pilot, "synthetic.nathan"),
            id(ContentKind::Airframe, "synthetic.fury"),
            id(ContentKind::Loadout, "synthetic.interceptor"),
            unknown,
            Origin::SyntheticFixture,
            design_claim("f33a.test.wingmate"),
        )
        .expect("an unknown survivability is a valid declared record");
        assert!(!wingmate.survivability().is_known());

        let roster = DeclaredRoster::try_new(
            id(ContentKind::Mission, "m01"),
            Origin::SyntheticFixture,
            id(ContentKind::Faction, "synthetic.nathan"),
            vec![pilot("synthetic.nathan")],
            vec![wingmate],
            Vec::new(),
            design_claim("f33a.test.roster"),
        )
        .expect("the roster is valid");
        assert_eq!(roster.wingmates().len(), 1);
        assert!(roster.neutral_traffic().is_empty());
    }
}
