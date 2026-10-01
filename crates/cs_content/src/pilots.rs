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

use std::collections::BTreeSet;
use std::fmt;

use cs_types::content::{ContentId, ContentKind, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

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
