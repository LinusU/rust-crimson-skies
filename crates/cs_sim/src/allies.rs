//! Pilot, aircraft and faction identity with capture (F33-A).
//!
//! Spec: `specs/F33-wingmates-factions-neutral-traffic-and-pilot-identity.md`,
//! stage `### F33-A`. Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! This module is the **runtime half** of the pilot/aircraft/faction
//! separation: the per-session [`AlliesRoster`] that owns who each actor is —
//! the pilot flying it, the faction it belongs to, the geometry it is built
//! from, the voice it speaks through and the survivability the mission granted
//! it. Its declared, provenance-carrying counterpart is
//! `cs_content::pilots`; the conversion boundary is `cs_app::roster`.
//!
//! # Identity separation
//!
//! [`PilotId`], [`FactionId`] and [`GeometryId`] are three distinct types and
//! an [`ActorId`] is a fourth: the same value can never be mistaken for
//! another role, because each constructor validates its catalog namespace and
//! the type system keeps the fields apart. In particular the geometry id — the
//! airframe or mesh a vehicle is built from — is its **own** field, so a
//! faction change cannot rename or repaint a vehicle's shape (F33
//! non-negotiable 1, AC01): [`AllyRecord`] has no setter for it at all, and
//! [`AlliesRoster::capture`] only ever moves `faction`.
//!
//! # Capture is the ownership transaction
//!
//! [`AlliesRoster::capture`] records an ownership change: it takes the new
//! [`FactionId`], returns a [`Capture`] naming the previous and new faction
//! *and the unchanged geometry*, and leaves pilot, geometry, voice and
//! survivability exactly as they were. Destruction and bailout are not this
//! transaction — the F29 [`LifecycleKind`] vocabulary keeps those separate —
//! and the A→B swap that carries a pilot into a new airframe is F33-C's
//! interaction transaction; this stage defines the record it moves.
//!
//! # Designed vocabulary, not original data
//!
//! Which pilots, factions, aircraft, loadouts and voices the original game
//! has, how it assigns wingmates, which allies it makes unkillable and where
//! any of that is stored are **unmeasured** (F33 "Research boundary"; the
//! retail stage is F33-D). Every type and fixture value here is newly authored
//! engine design, recorded in
//! `docs/findings/2026-10-01-f33-a-pilot-aircraft-faction-separation.md`.
//!
//! `cs_sim` may depend only on [`cs_types`] and [`cs_script`]
//! (`docs/01-ARCHITECTURE.md`): no Bevy, no renderer, no file access.
//!
//! [`cs_types`]: cs_types
//! [`cs_script`]: cs_script
//! [`LifecycleKind`]: crate::damage::LifecycleKind

use std::collections::BTreeMap;
use std::fmt;

use cs_types::content::{ContentId, ContentKind};
use cs_types::net::SessionId;

use crate::damage::ActorId;

/// Why an identity value was rejected.
///
/// Each variant names the namespace a value had to be in, so a cross-wired
/// content record is refused at construction rather than carried into a
/// session (F33 non-negotiable 1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdentityError {
    /// A pilot id was not in the `pilot` namespace.
    NotAPilot {
        /// The offending id.
        id: ContentId,
    },
    /// A faction id was not in the `faction` namespace.
    NotAFaction {
        /// The offending id.
        id: ContentId,
    },
    /// A geometry id was neither an `airframe` nor a `mesh`.
    NotAGeometry {
        /// The offending id.
        id: ContentId,
    },
}

impl fmt::Display for IdentityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAPilot { id } => write!(f, "{id} is not a pilot id"),
            Self::NotAFaction { id } => write!(f, "{id} is not a faction id"),
            Self::NotAGeometry { id } => write!(f, "{id} is not an airframe or mesh id"),
        }
    }
}

impl std::error::Error for IdentityError {}

/// A pilot's identity: the person, distinct from the aircraft and the faction.
///
/// Wraps a `ContentKind::Pilot` catalog id; the wrapper is what keeps a pilot
/// from being stored where an aircraft or a faction belongs.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PilotId(ContentId);

impl PilotId {
    /// Wraps a pilot catalog id.
    ///
    /// # Errors
    ///
    /// [`IdentityError::NotAPilot`] when `id` is not in the `pilot` namespace.
    pub fn try_new(id: ContentId) -> Result<Self, IdentityError> {
        if id.kind() != ContentKind::Pilot {
            return Err(IdentityError::NotAPilot { id });
        }
        Ok(Self(id))
    }

    /// The underlying catalog id.
    #[must_use]
    pub const fn as_content(&self) -> &ContentId {
        &self.0
    }
}

impl fmt::Display for PilotId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "pilot {}", self.0)
    }
}

/// A faction's identity: the side an actor belongs to.
///
/// Wraps a `ContentKind::Faction` catalog id. Faction relations themselves
/// are `cs_sim::targeting`'s directed [`AllegianceTable`]; this is only the
/// identity an actor is currently on, which capture moves.
///
/// [`AllegianceTable`]: crate::targeting::AllegianceTable
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FactionId(ContentId);

impl FactionId {
    /// Wraps a faction catalog id.
    ///
    /// # Errors
    ///
    /// [`IdentityError::NotAFaction`] when `id` is not in the `faction`
    /// namespace.
    pub fn try_new(id: ContentId) -> Result<Self, IdentityError> {
        if id.kind() != ContentKind::Faction {
            return Err(IdentityError::NotAFaction { id });
        }
        Ok(Self(id))
    }

    /// The underlying catalog id.
    #[must_use]
    pub const fn as_content(&self) -> &ContentId {
        &self.0
    }
}

impl fmt::Display for FactionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "faction {}", self.0)
    }
}

/// The geometry a vehicle is built from: an airframe or a mesh catalog id.
///
/// This is deliberately its own type and its own field on [`AllyRecord`]: a
/// capture changes *who owns* a vehicle and can never change what it is built
/// from, so the enemy status is never baked into a mesh name or a paint
/// colour (F33 non-negotiable 1, AC01).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GeometryId(ContentId);

impl GeometryId {
    /// Wraps an airframe or mesh catalog id.
    ///
    /// # Errors
    ///
    /// [`IdentityError::NotAGeometry`] when `id` is neither an `airframe` nor
    /// a `mesh`.
    pub fn try_new(id: ContentId) -> Result<Self, IdentityError> {
        if !matches!(id.kind(), ContentKind::Airframe | ContentKind::Mesh) {
            return Err(IdentityError::NotAGeometry { id });
        }
        Ok(Self(id))
    }

    /// The underlying catalog id.
    #[must_use]
    pub const fn as_content(&self) -> &ContentId {
        &self.0
    }
}

impl fmt::Display for GeometryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "geometry {}", self.0)
    }
}

/// The runtime survivability of one actor, lowered from
/// `cs_content::pilots::DeclaredSurvivability`.
///
/// The three labels keep "an ally" from collapsing into one behavior (F33
/// non-negotiable 4): a [`Mortal`](Self::Mortal) ally's death is a real loss,
/// a [`ProtectedNeutral`](Self::ProtectedNeutral) loss is a mission event
/// rather than a kill, and only a [`ScriptedInvulnerable`](Self::ScriptedInvulnerable)
/// actor was explicitly authored as unable to be destroyed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SurvivabilityPolicy {
    /// The actor can be destroyed; its death is a real loss.
    Mortal,
    /// The actor is a protected neutral: its loss reports a mission event and
    /// is never counted as a kill.
    ProtectedNeutral,
    /// The mission authors the actor as unable to be destroyed.
    ScriptedInvulnerable,
}

impl SurvivabilityPolicy {
    /// Every policy, in a stable order.
    pub const ALL: &'static [SurvivabilityPolicy] = &[
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

impl fmt::Display for SurvivabilityPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The runtime identity of one actor: five separate fields with five separate
/// meanings.
///
/// Every field is public because there is no hidden state — the mutations are
/// the explicit typed transactions on [`AlliesRoster`] (`set_pilot`,
/// `set_faction`, `capture`), never a field write behind its back. In
/// particular there is no operation that changes `geometry`, which is exactly
/// the AC01 contract.
#[derive(Clone, Debug, PartialEq)]
pub struct AllyRecord {
    /// The actor's session-qualified identity.
    pub actor: ActorId,
    /// The pilot flying it.
    pub pilot: PilotId,
    /// The faction it currently belongs to; moved only by
    /// [`AlliesRoster::set_faction`] or [`AlliesRoster::capture`].
    pub faction: FactionId,
    /// The geometry it is built from; **never** changed by a faction change.
    pub geometry: GeometryId,
    /// The voice it speaks through, when the mission authored one. `None`
    /// means no voice was authored, never a random substitute (F33
    /// non-negotiable 5).
    pub voice: Option<ContentId>,
    /// The mission-authored survivability.
    pub survivability: SurvivabilityPolicy,
}

/// The identity of one wingmate slot within a session.
///
/// Mirrors `cs_content::pilots::WingmateSlot`; the boundary lowers one onto
/// the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WingmateSlot(pub u32);

impl WingmateSlot {
    /// The slot's index.
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

/// One lowered wingmate assignment: who flies the slot, what it flies and the
/// survival policy it runs.
///
/// The assignment is data the roster holds; the *rules* that decide which
/// pilot, airframe and loadout a briefing selection produces (and how a retry
/// resets them) are F33-B.
#[derive(Clone, Debug, PartialEq)]
pub struct WingmateAssignment {
    /// The slot this assignment occupies.
    pub slot: WingmateSlot,
    /// The pilot that flies it.
    pub pilot: PilotId,
    /// The airframe it launches in (its geometry id).
    pub aircraft: GeometryId,
    /// The loadout it carries.
    pub loadout: ContentId,
    /// The voice it speaks through.
    pub voice: ContentId,
    /// The declared survival policy.
    pub survivability: SurvivabilityPolicy,
}

/// Why an [`AlliesRoster`] operation was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AlliesError {
    /// An input carried a session generation other than the roster's — state
    /// never leaks across generations.
    ForeignSession {
        /// The roster's session.
        expected: u64,
        /// The session the input carried.
        found: u64,
    },
    /// The actor is already registered.
    DuplicateActor {
        /// The actor that exists already.
        actor: ActorId,
    },
    /// The named actor is not registered with this roster.
    UnknownActor {
        /// The actor that was named.
        actor: ActorId,
    },
    /// Two assignments share one wingmate slot.
    DuplicateWingmateSlot {
        /// The repeated slot.
        slot: WingmateSlot,
    },
}

impl fmt::Display for AlliesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignSession { expected, found } => write!(
                f,
                "input belongs to session {found}, but this roster owns session {expected}"
            ),
            Self::DuplicateActor { actor } => write!(f, "{actor} is already registered"),
            Self::UnknownActor { actor } => write!(f, "{actor} is not registered"),
            Self::DuplicateWingmateSlot { slot } => {
                write!(f, "{slot} is assigned more than once")
            }
        }
    }
}

impl std::error::Error for AlliesError {}

/// The outcome of an ownership capture: the actor, the faction it left, the
/// faction it joined and the geometry that did **not** change.
///
/// Publishing this as its own record is what lets mission logic react to a
/// capture separately from a destruction (F33 non-negotiable 4); wiring it to
/// the damage lifecycle and the mission callbacks is F33-C.
#[derive(Clone, Debug, PartialEq)]
pub struct Capture {
    /// The captured actor.
    pub actor: ActorId,
    /// The faction the actor belonged to.
    pub previous_faction: FactionId,
    /// The faction the actor now belongs to.
    pub new_faction: FactionId,
    /// The actor's geometry id — identical before and after, by contract.
    pub geometry: GeometryId,
}

/// The per-session identity and wingmate roster. See the module docs.
///
/// The roster owns one [`AllyRecord`] per actor and the session's wingmate
/// assignments for one session generation. A restart or an aircraft swap is a
/// *new* roster: records, assignments and captures never carry into the next
/// generation (STATE-TRANSACTIONS session generations).
#[derive(Clone, Debug)]
pub struct AlliesRoster {
    session: u64,
    records: BTreeMap<ActorId, AllyRecord>,
    wingmates: BTreeMap<WingmateSlot, WingmateAssignment>,
}

impl AlliesRoster {
    /// An empty roster for session `session`.
    #[must_use]
    pub fn new(session: u64) -> Self {
        Self {
            session,
            records: BTreeMap::new(),
            wingmates: BTreeMap::new(),
        }
    }

    /// The session generation this roster owns.
    #[must_use]
    pub const fn session(&self) -> u64 {
        self.session
    }

    /// Registers an actor's identity record.
    ///
    /// # Errors
    ///
    /// [`AlliesError::ForeignSession`] when the actor belongs to another
    /// session generation, [`AlliesError::DuplicateActor`] when it is already
    /// registered.
    pub fn register(&mut self, record: AllyRecord) -> Result<(), AlliesError> {
        if record.actor.session.get() != self.session {
            return Err(AlliesError::ForeignSession {
                expected: self.session,
                found: record.actor.session.get(),
            });
        }
        if self.records.contains_key(&record.actor) {
            return Err(AlliesError::DuplicateActor {
                actor: record.actor,
            });
        }
        self.records.insert(record.actor, record);
        Ok(())
    }

    /// Whether the actor is registered.
    #[must_use]
    pub fn is_registered(&self, actor: &ActorId) -> bool {
        self.records.contains_key(actor)
    }

    /// One registered actor's record; `None` when unregistered.
    #[must_use]
    pub fn record(&self, actor: &ActorId) -> Option<&AllyRecord> {
        self.records.get(actor)
    }

    /// The faction the actor currently belongs to.
    #[must_use]
    pub fn faction_of(&self, actor: &ActorId) -> Option<&FactionId> {
        self.records.get(actor).map(|record| &record.faction)
    }

    /// The geometry the actor is built from.
    #[must_use]
    pub fn geometry_of(&self, actor: &ActorId) -> Option<&GeometryId> {
        self.records.get(actor).map(|record| &record.geometry)
    }

    /// The pilot flying the actor.
    #[must_use]
    pub fn pilot_of(&self, actor: &ActorId) -> Option<&PilotId> {
        self.records.get(actor).map(|record| &record.pilot)
    }

    /// Records an ownership/faction change — the capture transaction.
    ///
    /// Only `faction` moves. The returned [`Capture`] names the unchanged
    /// geometry, so mission logic can publish a capture without inventing a
    /// new vehicle (F33 non-negotiable 1, AC01). Capturing to the faction the
    /// actor already belongs to is an idempotent no-op capture, not an error.
    ///
    /// # Errors
    ///
    /// [`AlliesError::ForeignSession`] or [`AlliesError::UnknownActor`].
    pub fn capture(
        &mut self,
        actor: ActorId,
        new_faction: FactionId,
    ) -> Result<Capture, AlliesError> {
        let record = self.entry_mut(actor)?;
        let previous_faction = std::mem::replace(&mut record.faction, new_faction.clone());
        Ok(Capture {
            actor,
            previous_faction,
            new_faction,
            geometry: record.geometry.clone(),
        })
    }

    /// Recursively: sets the actor's faction without producing a capture
    /// record — the scripted relation/alliance change that is not an
    /// ownership transfer.
    ///
    /// # Errors
    ///
    /// [`AlliesError::ForeignSession`] or [`AlliesError::UnknownActor`].
    pub fn set_faction(&mut self, actor: ActorId, faction: FactionId) -> Result<(), AlliesError> {
        self.entry_mut(actor)?.faction = faction;
        Ok(())
    }

    /// Rebinds the pilot flying an actor — the destination half of an
    /// authored aircraft swap.
    ///
    /// The pilot identity is carried over from the source actor; the
    /// destination's geometry is the destination aircraft's own, so aircraft
    /// damage does not transfer with the pilot (F33 non-negotiable 3). The
    /// full atomic swap (create the destination actor, release the source) is
    /// F33-C.
    ///
    /// # Errors
    ///
    /// [`AlliesError::ForeignSession`] or [`AlliesError::UnknownActor`].
    pub fn set_pilot(&mut self, actor: ActorId, pilot: PilotId) -> Result<(), AlliesError> {
        self.entry_mut(actor)?.pilot = pilot;
        Ok(())
    }

    /// Replaces the session's wingmate assignments.
    ///
    /// The whole set is validated before any of it is applied: two
    /// assignments sharing a slot refuse the caller and leave the previous
    /// set untouched. Assignment *rules* (which pilot and loadout a briefing
    /// produces, and how a retry resets them) are F33-B; this is the typed
    /// store they write through.
    ///
    /// # Errors
    ///
    /// [`AlliesError::DuplicateWingmateSlot`].
    pub fn assign_wingmates(
        &mut self,
        assignments: Vec<WingmateAssignment>,
    ) -> Result<(), AlliesError> {
        let mut staged: BTreeMap<WingmateSlot, WingmateAssignment> = BTreeMap::new();
        for assignment in assignments {
            let slot = assignment.slot;
            if staged.contains_key(&slot) {
                return Err(AlliesError::DuplicateWingmateSlot { slot });
            }
            staged.insert(slot, assignment);
        }
        self.wingmates = staged;
        Ok(())
    }

    /// The session's wingmate assignments, in slot order.
    #[must_use]
    pub fn wingmates(&self) -> Vec<&WingmateAssignment> {
        self.wingmates.values().collect()
    }

    /// One wingmate assignment by slot; `None` when unassigned.
    #[must_use]
    pub fn wingmate(&self, slot: WingmateSlot) -> Option<&WingmateAssignment> {
        self.wingmates.get(&slot)
    }

    fn entry_mut(&mut self, actor: ActorId) -> Result<&mut AllyRecord, AlliesError> {
        if actor.session.get() != self.session {
            return Err(AlliesError::ForeignSession {
                expected: self.session,
                found: actor.session.get(),
            });
        }
        self.records
            .get_mut(&actor)
            .ok_or(AlliesError::UnknownActor { actor })
    }
}

// ----------------------------------------------------------- fixture ------

fn content(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("fixture id is valid")
}

/// A fixture pilot id.
#[must_use]
pub fn synthetic_pilot(key: &str) -> PilotId {
    PilotId::try_new(content(ContentKind::Pilot, key)).expect("fixture pilot id is valid")
}

/// A fixture faction id.
#[must_use]
pub fn synthetic_faction(key: &str) -> FactionId {
    FactionId::try_new(content(ContentKind::Faction, key)).expect("fixture faction id is valid")
}

/// A fixture geometry id (an airframe).
#[must_use]
pub fn synthetic_geometry(key: &str) -> GeometryId {
    GeometryId::try_new(content(ContentKind::Airframe, key)).expect("fixture geometry id is valid")
}

fn ally(
    session: u64,
    serial: u64,
    pilot: &str,
    faction: &str,
    geometry: &str,
    voice: Option<&str>,
    survivability: SurvivabilityPolicy,
) -> AllyRecord {
    AllyRecord {
        actor: ActorId {
            session: SessionId::new(session).expect("a nonzero session generation"),
            serial,
        },
        pilot: synthetic_pilot(pilot),
        faction: synthetic_faction(faction),
        geometry: synthetic_geometry(geometry),
        voice: voice.map(|key| content(ContentKind::Voice, key)),
        survivability,
    }
}

/// The minimal synthetic ally roster for `session`: the player ace (serial 1),
/// a mortal wingman (serial 2), a raider (serial 9) and a protected neutral
/// trader (serial 4).
///
/// Serials are chosen to differ from the coordinate order the targeting
/// fixture uses, so a test that relies on ordering cannot pass by accident.
#[must_use]
pub fn synthetic_ally_roster(session: u64) -> Vec<AllyRecord> {
    vec![
        ally(
            session,
            1,
            "synthetic.nathan",
            "synthetic.nathan",
            "synthetic.fury",
            Some("synthetic.nathan"),
            SurvivabilityPolicy::ScriptedInvulnerable,
        ),
        ally(
            session,
            2,
            "synthetic.betty",
            "synthetic.nathan",
            "synthetic.devastator",
            Some("synthetic.betty"),
            SurvivabilityPolicy::Mortal,
        ),
        ally(
            session,
            9,
            "synthetic.raider",
            "synthetic.raiders",
            "synthetic.raider",
            None,
            SurvivabilityPolicy::Mortal,
        ),
        ally(
            session,
            4,
            "synthetic.trader",
            "synthetic.traders",
            "synthetic.freighter",
            None,
            SurvivabilityPolicy::ProtectedNeutral,
        ),
    ]
}

/// The synthetic wingmate assignments matching
/// `cs_content::pilots::declared_synthetic_roster`.
#[must_use]
pub fn synthetic_wingmates() -> Vec<WingmateAssignment> {
    vec![
        WingmateAssignment {
            slot: WingmateSlot(1),
            pilot: synthetic_pilot("synthetic.betty"),
            aircraft: synthetic_geometry("synthetic.devastator"),
            loadout: content(ContentKind::Loadout, "synthetic.escort"),
            voice: content(ContentKind::Voice, "synthetic.betty"),
            survivability: SurvivabilityPolicy::Mortal,
        },
        WingmateAssignment {
            slot: WingmateSlot(2),
            pilot: synthetic_pilot("synthetic.nathan"),
            aircraft: synthetic_geometry("synthetic.fury"),
            loadout: content(ContentKind::Loadout, "synthetic.interceptor"),
            voice: content(ContentKind::Voice, "synthetic.nathan"),
            survivability: SurvivabilityPolicy::ScriptedInvulnerable,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION: u64 = 7;

    fn actor(serial: u64) -> ActorId {
        ActorId {
            session: SessionId::new(SESSION).expect("a nonzero session generation"),
            serial,
        }
    }

    fn roster() -> AlliesRoster {
        let mut roster = AlliesRoster::new(SESSION);
        for record in synthetic_ally_roster(SESSION) {
            roster.register(record).expect("fixture records register");
        }
        roster
    }

    /// AC01: a captured vehicle changes faction and nothing else — its
    /// geometry id before and after is identical, so the enemy status is
    /// never baked into the mesh.
    #[test]
    fn accept_f33_a_capture_changes_faction_without_changing_geometry() {
        let mut roster = roster();
        let target = actor(9);
        let geometry_before = roster.geometry_of(&target).expect("registered").clone();
        let pilot_before = roster.pilot_of(&target).expect("registered").clone();
        assert_eq!(
            roster.faction_of(&target).expect("registered").clone(),
            synthetic_faction("synthetic.raiders")
        );

        let capture = roster
            .capture(target, synthetic_faction("synthetic.nathan"))
            .expect("the raider is registered");

        assert_eq!(
            capture.previous_faction,
            synthetic_faction("synthetic.raiders")
        );
        assert_eq!(capture.new_faction, synthetic_faction("synthetic.nathan"));
        assert_eq!(
            capture.geometry, geometry_before,
            "the capture names the unchanged geometry"
        );
        assert_eq!(
            roster.geometry_of(&target).expect("registered").clone(),
            geometry_before,
            "a capture never changes the geometry id"
        );
        assert_eq!(
            roster.faction_of(&target).expect("registered").clone(),
            synthetic_faction("synthetic.nathan"),
            "a capture does change the faction"
        );
        assert_eq!(
            roster.pilot_of(&target).expect("registered").clone(),
            pilot_before,
            "a capture does not replace the pilot"
        );

        // Capturing to the faction it already belongs to is an idempotent
        // no-op capture, not an error.
        let again = roster
            .capture(target, synthetic_faction("synthetic.nathan"))
            .expect("a repeat capture is allowed");
        assert_eq!(again.previous_faction, again.new_faction);
        assert_eq!(again.geometry, geometry_before);
    }

    /// A capture refuses an actor this roster does not own and one from
    /// another session generation; neither touches anything.
    #[test]
    fn accept_f33_a_capture_refuses_unknown_and_foreign_actors() {
        let mut roster = roster();
        let geometry_before = roster.geometry_of(&actor(9)).expect("registered").clone();

        assert_eq!(
            roster.capture(actor(99), synthetic_faction("synthetic.nathan")),
            Err(AlliesError::UnknownActor { actor: actor(99) })
        );
        assert_eq!(
            roster.capture(
                ActorId {
                    session: SessionId::new(SESSION + 1).expect("a nonzero session generation"),
                    serial: 9
                },
                synthetic_faction("synthetic.nathan"),
            ),
            Err(AlliesError::ForeignSession {
                expected: SESSION,
                found: SESSION + 1,
            })
        );
        assert_eq!(
            roster.geometry_of(&actor(9)).expect("registered").clone(),
            geometry_before,
            "a refused capture leaves the record untouched"
        );
        assert_eq!(
            roster.faction_of(&actor(9)).expect("registered").clone(),
            synthetic_faction("synthetic.raiders")
        );
    }

    /// The identity types refuse a value from another namespace, so a faction
    /// can never be stored where a pilot or a geometry belongs.
    #[test]
    fn accept_f33_a_identity_ids_refuse_the_wrong_namespace() {
        assert_eq!(
            PilotId::try_new(content(ContentKind::Faction, "synthetic.nathan")),
            Err(IdentityError::NotAPilot {
                id: content(ContentKind::Faction, "synthetic.nathan")
            })
        );
        assert_eq!(
            FactionId::try_new(content(ContentKind::Pilot, "synthetic.nathan")),
            Err(IdentityError::NotAFaction {
                id: content(ContentKind::Pilot, "synthetic.nathan")
            })
        );
        assert_eq!(
            GeometryId::try_new(content(ContentKind::Pilot, "synthetic.nathan")),
            Err(IdentityError::NotAGeometry {
                id: content(ContentKind::Pilot, "synthetic.nathan")
            })
        );
        // A mesh is geometry too; an airframe is geometry.
        assert!(GeometryId::try_new(content(ContentKind::Mesh, "synthetic.fury")).is_ok());
        assert!(GeometryId::try_new(content(ContentKind::Airframe, "synthetic.fury")).is_ok());
    }

    /// A pilot identity survives an aircraft swap: the destination actor
    /// keeps the pilot, its own geometry and its own faction. Aircraft
    /// identity is not the pilot's.
    #[test]
    fn accept_f33_a_pilot_identity_survives_an_aircraft_swap() {
        let mut roster = roster();
        let source = actor(2);
        let destination = actor(3);
        let pilot = roster.pilot_of(&source).expect("registered").clone();

        roster
            .register(AllyRecord {
                actor: destination,
                pilot: synthetic_pilot("synthetic.unassigned"),
                faction: synthetic_faction("synthetic.nathan"),
                geometry: synthetic_geometry("synthetic.fury"),
                voice: None,
                survivability: SurvivabilityPolicy::Mortal,
            })
            .expect("the destination registers");

        roster
            .set_pilot(destination, pilot.clone())
            .expect("the destination is registered");

        assert_eq!(
            roster.pilot_of(&destination).expect("registered").clone(),
            pilot,
            "the pilot identity carried to the destination"
        );
        assert_eq!(
            roster
                .geometry_of(&destination)
                .expect("registered")
                .clone(),
            synthetic_geometry("synthetic.fury"),
            "the destination keeps its own geometry"
        );
        assert_ne!(
            roster.geometry_of(&source).expect("registered"),
            roster.geometry_of(&destination).expect("registered"),
            "the two aircraft are different geometry ids"
        );
    }

    /// The wingmate store refuses a duplicated slot and keeps the previous
    /// set untouched, so a retry cannot half-apply an assignment.
    #[test]
    fn accept_f33_a_wingmates_refuse_duplicate_slots() {
        let mut roster = roster();
        roster
            .assign_wingmates(synthetic_wingmates())
            .expect("the fixture assigns");
        assert_eq!(roster.wingmates().len(), 2);
        assert_eq!(roster.wingmates()[0].slot, WingmateSlot(1));

        let mut duplicated = synthetic_wingmates();
        duplicated[1].slot = WingmateSlot(1);
        assert_eq!(
            roster.assign_wingmates(duplicated),
            Err(AlliesError::DuplicateWingmateSlot {
                slot: WingmateSlot(1)
            })
        );
        assert_eq!(
            roster.wingmates().len(),
            2,
            "a refused assignment leaves the previous set in place"
        );
    }
}
