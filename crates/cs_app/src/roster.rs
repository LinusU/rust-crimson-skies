//! The pilot-roster application boundary (F33-A, F33-B, F33-C).
//!
//! Spec: `specs/F33-wingmates-factions-neutral-traffic-and-pilot-identity.md`,
//! stages `### F33-A`, `### F33-B` and `### F33-C`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! This module sits between the declared roster schema
//! ([`cs_content::pilots`]) and the per-session identity store
//! ([`cs_sim::allies`]), which cannot see each other — `cs_sim` must not
//! depend on `cs_content` (`docs/01-ARCHITECTURE.md`). It contains:
//!
//! * [`lower_roster`] — the conversion boundary: a validated
//!   [`cs_content::pilots::DeclaredRoster`] becomes the
//!   [`LoweredRoster`] a session registers actors and wingmate assignments
//!   from. Every `Resolved::Unknown` **refuses** rather than guessing: an
//!   unmeasured pilot voice never becomes a random line (F33 non-negotiable
//!   5) and an unmeasured survivability never becomes a silent mortal.
//! * [`open_roster`] — the F33-B session entry: it opens an
//!   [`AlliesRoster`] from a lowered roster and the player's [`BriefingPlan`],
//!   committing the player faction and deriving the wingmate assignments from
//!   the authored records every time, so a retry rebuilds rather than
//!   carrying the failed world's state.
//! * [`apply_roster_lifecycle`] / [`apply_ally_lifecycle`] — the F33-C
//!   consumer seam: the authoritative F29 [`cs_sim::damage::LifecycleKind`]
//!   ledger is fed into the identity record, a mission callback
//!   ([`cs_sim::allies::AllyEvent`]) and an authored-voice [`DialogueCue`] are
//!   produced, and an ally that may no longer act is taken out of the weapon
//!   firing gate ([`FireResolver`]) so a dead actor cannot later fire
//!   (AC03). Every refusal is named in the returned [`AllyConsumerOutcome`].
//! * [`RosterBinding`] — the ECS record tying an entity to its
//!   session-qualified [`cs_sim::damage::ActorId`] and the declared roster
//!   subject it was spawned under, generation-stamped like
//!   [`crate::scene::SceneNodeBinding`] so a reload can never leave a stale
//!   binding looking live.
//!
//! Nothing here owns identity state: the pilot, faction, geometry, voice and
//! survivability records are the `AlliesRoster`'s; these are the conversion
//! and binding records the ECS wiring consumes (F33-B/C).

use bevy::ecs::component::Component;
use cs_content::pilots::{DeclaredPilot, DeclaredRoster, DeclaredSurvivability, DeclaredWingmate};
use cs_sim::allies::{
    AlliesError, AlliesRoster, AllyEvent, AllyEventKind, BriefingError, BriefingPlan, FactionId,
    GeometryId, IdentityError, PilotId, SurvivabilityPolicy, WingmateAssignment, WingmateSlot,
};
use cs_sim::damage::{ActorId, DamageNodeKey, DamageResolver, LifecycleKind};
use cs_sim::weapons::FireResolver;
use cs_types::content::{ContentId, Resolved};
use cs_types::evidence::ClaimId;

use crate::scene::SceneGeneration;

/// One lowered pilot: its identity and the voice it speaks through.
///
/// The voice is a catalog id, resolved from the declared record's
/// `Resolved<ContentId>` — an unknown voice refuses at [`lower_roster`], so
/// the lowered record never carries a random substitute (F33 non-negotiable
/// 5).
#[derive(Clone, Debug, PartialEq)]
pub struct LoweredPilot {
    /// The pilot's identity.
    pub id: PilotId,
    /// The voice catalog id the pilot speaks through.
    pub voice: ContentId,
}

/// One lowered neutral actor.
#[derive(Clone, Debug, PartialEq)]
pub struct NeutralTraffic {
    /// The mission-authored traffic index.
    pub traffic: u32,
    /// The pilot that flies it.
    pub pilot: PilotId,
    /// The geometry it is built from.
    pub geometry: GeometryId,
    /// The faction it starts on.
    pub faction: FactionId,
    /// The declared survivability.
    pub survivability: SurvivabilityPolicy,
}

/// What [`lower_roster`] produces: the session's player faction, its lowered
/// pilots, its wingmate assignments and its authored neutral traffic.
///
/// An omitted declared list lowers to an **empty** list, never a global
/// population default (F33 AC04's contract half): the session can only spawn
/// the neutral actors the mission authored.
#[derive(Clone, Debug, PartialEq)]
pub struct LoweredRoster {
    /// The faction the mission's player and wingmates fly for.
    pub player_faction: FactionId,
    /// The lowered pilots, in authored order.
    pub pilots: Vec<LoweredPilot>,
    /// The lowered wingmate assignments, in authored order.
    pub wingmates: Vec<WingmateAssignment>,
    /// The lowered neutral actors, in authored order.
    pub neutral_traffic: Vec<NeutralTraffic>,
}

/// Why a declared roster could not be lowered to the runtime records.
#[derive(Clone, Debug, PartialEq)]
pub enum RosterLowerError {
    /// A pilot's voice is `Resolved::Unknown`: refused, because a random
    /// line must never stand in for a missing mission dialogue (F33
    /// non-negotiable 5).
    UnknownVoice {
        /// The pilot whose voice is unknown.
        pilot: ContentId,
        /// The claim the unknown is recorded under.
        claim_id: ClaimId,
        /// Why the voice is unknown.
        reason: String,
    },
    /// A survivability is `Resolved::Unknown`: refused, because "unknown" is
    /// not "mortal" and an actor must not silently become killable.
    UnknownSurvivability {
        /// Which record list the value belongs to: `"wingmate"` or
        /// `"neutral_traffic"`.
        record: &'static str,
        /// The value's index in that list.
        index: usize,
        /// The claim the unknown is recorded under.
        claim_id: ClaimId,
        /// Why the survivability is unknown.
        reason: String,
    },
    /// A referenced id was not in the namespace its field requires.
    Identity {
        /// Which record the id belongs to.
        record: &'static str,
        /// The value's index in that list.
        index: usize,
        /// The namespace error.
        error: IdentityError,
    },
    /// A wingmate names a pilot the lowered pilot list does not contain.
    /// Unreachable through `DeclaredRoster::try_new`, which refuses it
    /// earlier, but reported rather than silently skipped.
    UndeclaredPilot {
        /// Which record names the pilot.
        record: &'static str,
        /// The value's index in that list.
        index: usize,
        /// The undeclared pilot.
        pilot: ContentId,
    },
}

impl std::fmt::Display for RosterLowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownVoice {
                pilot,
                claim_id,
                reason,
            } => write!(
                f,
                "pilot {pilot} has no evidence-backed voice ({}: {reason})",
                claim_id.as_str()
            ),
            Self::UnknownSurvivability {
                record,
                index,
                claim_id,
                reason,
            } => write!(
                f,
                "{record} #{index} has an unknown survivability ({}: {reason})",
                claim_id.as_str()
            ),
            Self::Identity {
                record,
                index,
                error,
            } => write!(f, "{record} #{index} is invalid: {error}"),
            Self::UndeclaredPilot {
                record,
                index,
                pilot,
            } => write!(f, "{record} #{index} names undeclared pilot {pilot}"),
        }
    }
}

impl std::error::Error for RosterLowerError {}

fn lower_survivability(
    record: &'static str,
    index: usize,
    value: &Resolved<DeclaredSurvivability>,
) -> Result<SurvivabilityPolicy, RosterLowerError> {
    match value {
        Resolved::Known(known) => Ok(match known.value {
            DeclaredSurvivability::Mortal => SurvivabilityPolicy::Mortal,
            DeclaredSurvivability::ProtectedNeutral => SurvivabilityPolicy::ProtectedNeutral,
            DeclaredSurvivability::ScriptedInvulnerable => {
                SurvivabilityPolicy::ScriptedInvulnerable
            }
        }),
        Resolved::Unknown { claim_id, reason } => Err(RosterLowerError::UnknownSurvivability {
            record,
            index,
            claim_id: claim_id.clone(),
            reason: reason.clone(),
        }),
    }
}

fn lower_pilot(declared: &DeclaredPilot, index: usize) -> Result<LoweredPilot, RosterLowerError> {
    let id =
        PilotId::try_new(declared.id().clone()).map_err(|error| RosterLowerError::Identity {
            record: "pilot",
            index,
            error,
        })?;
    let voice = match declared.voice() {
        Resolved::Known(known) => known.value.clone(),
        Resolved::Unknown { claim_id, reason } => {
            return Err(RosterLowerError::UnknownVoice {
                pilot: declared.id().clone(),
                claim_id: claim_id.clone(),
                reason: reason.clone(),
            });
        }
    };
    Ok(LoweredPilot { id, voice })
}

fn lower_wingmate(
    declared: &DeclaredWingmate,
    index: usize,
    pilots: &[LoweredPilot],
) -> Result<WingmateAssignment, RosterLowerError> {
    let pilot =
        PilotId::try_new(declared.pilot().clone()).map_err(|error| RosterLowerError::Identity {
            record: "wingmate",
            index,
            error,
        })?;
    let aircraft = GeometryId::try_new(declared.airframe().clone()).map_err(|error| {
        RosterLowerError::Identity {
            record: "wingmate",
            index,
            error,
        }
    })?;
    let voice = pilots
        .iter()
        .find(|lowered| lowered.id.as_content() == declared.pilot())
        .map(|lowered| lowered.voice.clone())
        .ok_or_else(|| RosterLowerError::UndeclaredPilot {
            record: "wingmate",
            index,
            pilot: declared.pilot().clone(),
        })?;
    Ok(WingmateAssignment {
        slot: WingmateSlot(declared.slot().index()),
        pilot,
        aircraft,
        loadout: declared.loadout().clone(),
        voice,
        survivability: lower_survivability("wingmate", index, declared.survivability())?,
    })
}

/// Lowers declared roster records into the runtime records a session
/// registers actors and wingmate assignments from.
///
/// Every `Resolved::Unknown` refuses naming the field and the claim it is
/// recorded under; no value is defaulted, guessed or repaired here. The
/// neutral-traffic list lowers verbatim, so an omitted list becomes an empty
/// list rather than a population default.
///
/// # Errors
///
/// [`RosterLowerError`] on any unresolved declared value or a referenced id
/// outside its namespace.
pub fn lower_roster(declared: &DeclaredRoster) -> Result<LoweredRoster, RosterLowerError> {
    let player_faction =
        FactionId::try_new(declared.player_faction().clone()).map_err(|error| {
            RosterLowerError::Identity {
                record: "player_faction",
                index: 0,
                error,
            }
        })?;

    let mut pilots = Vec::with_capacity(declared.pilots().len());
    for (index, pilot) in declared.pilots().iter().enumerate() {
        pilots.push(lower_pilot(pilot, index)?);
    }

    let mut wingmates = Vec::with_capacity(declared.wingmates().len());
    for (index, wingmate) in declared.wingmates().iter().enumerate() {
        wingmates.push(lower_wingmate(wingmate, index, &pilots)?);
    }

    let mut neutral_traffic = Vec::with_capacity(declared.neutral_traffic().len());
    for (index, neutral) in declared.neutral_traffic().iter().enumerate() {
        let pilot = PilotId::try_new(neutral.pilot().clone()).map_err(|error| {
            RosterLowerError::Identity {
                record: "neutral_traffic",
                index,
                error,
            }
        })?;
        let geometry = GeometryId::try_new(neutral.airframe().clone()).map_err(|error| {
            RosterLowerError::Identity {
                record: "neutral_traffic",
                index,
                error,
            }
        })?;
        let faction = FactionId::try_new(neutral.faction().clone()).map_err(|error| {
            RosterLowerError::Identity {
                record: "neutral_traffic",
                index,
                error,
            }
        })?;
        neutral_traffic.push(NeutralTraffic {
            traffic: neutral.traffic(),
            pilot,
            geometry,
            faction,
            survivability: lower_survivability("neutral_traffic", index, neutral.survivability())?,
        });
    }

    Ok(LoweredRoster {
        player_faction,
        pilots,
        wingmates,
        neutral_traffic,
    })
}

/// Opens a session's ally roster from a lowered mission roster and the
/// player's briefing selection.
///
/// The roster is built from the **authored** lowered records every time: the
/// player faction is committed once, and the wingmate assignments are derived
/// from the authored assignments plus the briefing plan
/// ([`AlliesRoster::reset_wingmates`]). A retry calls this again with a new
/// `session` and the same lowered roster, so the session starts from the
/// authored state and the briefing choice rather than the failed world's
/// rearm or captures (`docs/contracts/STATE-TRANSACTIONS.md`: "Retry restores
/// the authored initial state, not a mutated copy of the just-failed world").
/// The briefing selection is a session input, so it is re-applied identically.
///
/// # Errors
///
/// [`BriefingError`] when the plan selects a slot the mission never assigned.
pub fn open_roster(
    session: u64,
    lowered: &LoweredRoster,
    plan: &BriefingPlan,
) -> Result<AlliesRoster, BriefingError> {
    let mut roster = AlliesRoster::new(session);
    roster.set_player_faction(lowered.player_faction.clone());
    roster.reset_wingmates(&lowered.wingmates, plan)?;
    Ok(roster)
}

// ------------------------------------- the ally lifecycle consumer seam (F33-C) ---
//
// F33-C. The F29 [`DamageResolver`] is the *producer*: it owns the
// authoritative five-way lifecycle ledger and records destruction, bailout,
// capture, despawn and mission removal exactly once. The *consumers* this seam
// drives are the identity record itself (the mission callback and its authored
// dialogue voice) and the weapon firing gate, which must stop firing the
// moment an actor may no longer act (AC03: "an ally killed during a cutscene
// cannot later fire from a stale actor").
//
// The pass is **state-driven**, not event-replayed, exactly like
// `cs_app::damage::apply_damage_state`: it reads the resolver's lifecycle set
// and records only the kinds the roster has not seen, so running it during a
// paused cutscene, after the scene resumes, or twice in a row is convergent
// and a death that happened while the simulation was paused cannot be missed.
// A capture or a bailout is recorded but does **not** close the firing gate —
// only destruction, despawn and mission removal do, mirroring
// `cs_sim::targeting`'s `ends_targeting` split. Every refusal is named in the
// returned [`AllyConsumerLog`] rather than swallowed.

/// One dialogue line a mission callback resolves to.
///
/// The voice is the pilot's authored catalog id, carried from the
/// [`AlliesRoster`] through the [`AllyEvent`]; a pilot the mission authored no
/// voice for produces **no** cue rather than a random substitute (F33
/// non-negotiable 5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DialogueCue {
    /// The actor whose transition the line answers.
    pub actor: ActorId,
    /// What the transition means to mission logic.
    pub kind: AllyEventKind,
    /// The authored voice catalog id the line resolves through.
    pub voice: ContentId,
}

/// Why the ally lifecycle pass could not update a consumer.
///
/// A refusal is a returned record, not a dropped update: the pass says
/// exactly which actor it could not reconcile and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AllyConsumerRefusal {
    /// The actor's session is not the roster's. A restarted or swapped actor
    /// is a new generation; nothing from the old one is recorded.
    ForeignSession {
        /// The roster's session generation.
        expected: u64,
        /// The session the named actor carried.
        found: u64,
    },
    /// The actor is not registered with the roster, so it has no identity to
    /// reconcile.
    UnknownActor {
        /// The unknown actor.
        actor: ActorId,
    },
    /// The roster itself refused the transition; see [`AlliesError`].
    Lifecycle(AlliesError),
}

/// One update the ally lifecycle pass applied, or one refusal.
#[derive(Clone, Debug, PartialEq)]
pub enum AllyConsumerEvent {
    /// The actor's identity recorded a lifecycle transition — the mission
    /// callback. A wingmate loss, a protected-neutral loss, a capture and a
    /// bailout are different events, never collapsed into one.
    Callback(AllyEvent),
    /// A mission callback resolved to the pilot's authored voice. Emitted
    /// only when the mission authored a voice.
    Dialogue(DialogueCue),
    /// A mount was disabled because the actor may no longer act, so a stale
    /// actor cannot fire from a wreck.
    MountDisabled {
        /// The actor.
        actor: ActorId,
        /// The disabled mount.
        mount: DamageNodeKey,
    },
    /// The update could not be applied; see [`AllyConsumerRefusal`].
    Refused(AllyConsumerRefusal),
}

/// The append-only record of what the ally lifecycle pass changed, oldest
/// first.
///
/// A log entry is written only for a real transition, a real disable or a
/// refusal — never for a consumer that already agreed with the state, so a
/// converged pass logs nothing more.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AllyConsumerLog {
    events: Vec<AllyConsumerEvent>,
}

impl AllyConsumerLog {
    /// An empty log.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends one event.
    pub fn push(&mut self, event: AllyConsumerEvent) {
        self.events.push(event);
    }

    /// Every event, oldest first.
    #[must_use]
    pub fn events(&self) -> &[AllyConsumerEvent] {
        &self.events
    }

    /// The most recent event.
    #[must_use]
    pub fn last(&self) -> Option<&AllyConsumerEvent> {
        self.events.last()
    }

    /// How many events the log holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether the log is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    fn absorb(&mut self, other: Self) {
        self.events.extend(other.events);
    }
}

/// What one ally lifecycle pass changed.
///
/// The counters are how a caller observes convergence: the same state applied
/// twice reports zero the second time, and a pass that only sees already-known
/// transitions disables nothing new.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AllyConsumerReport {
    /// Lifecycle transitions newly recorded in the identity record.
    pub recorded: u32,
    /// Mounts newly disabled on actors that may no longer act.
    pub mounts_disabled: u32,
    /// Authored-voice dialogue cues emitted.
    pub cues: u32,
    /// Updates that could not be applied.
    pub refused: u32,
}

impl AllyConsumerReport {
    /// Whether the pass changed nothing and refused nothing.
    #[must_use]
    pub const fn is_noop(&self) -> bool {
        self.recorded == 0 && self.mounts_disabled == 0 && self.cues == 0 && self.refused == 0
    }
}

/// One ally lifecycle pass's report and its log.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AllyConsumerOutcome {
    /// What the pass changed.
    pub report: AllyConsumerReport,
    /// Every change and refusal, oldest first.
    pub log: AllyConsumerLog,
}

impl AllyConsumerOutcome {
    fn absorb(&mut self, other: Self) {
        self.report.recorded += other.report.recorded;
        self.report.mounts_disabled += other.report.mounts_disabled;
        self.report.cues += other.report.cues;
        self.report.refused += other.report.refused;
        self.log.absorb(other.log);
    }
}

/// Reconciles one actor's identity record with the authoritative F29 damage
/// lifecycle and closes the firing gate when the actor may no longer act.
///
/// The damage resolver's lifecycle set is the producer: every transition the
/// roster has not yet recorded becomes an [`AllyEvent`] mission callback and,
/// when the pilot has an authored voice, a [`DialogueCue`]. Only destruction,
/// despawn and mission removal end the actor's ability to act
/// ([`cs_sim::allies::AllyStatus::ends_action`]); a capture or a bailout is
/// recorded but leaves the guns alone.
///
/// # Errors
///
/// Never returns `Err`: a foreign or unknown actor and a transition the roster
/// itself refuses are reported through [`AllyConsumerRefusal`] in the returned
/// outcome, so a caller can keep reconciling the rest of the roster.
#[must_use]
pub fn apply_ally_lifecycle(
    roster: &mut AlliesRoster,
    actor: ActorId,
    damage: &DamageResolver,
    fire: &mut FireResolver,
) -> AllyConsumerOutcome {
    let mut outcome = AllyConsumerOutcome::default();

    if actor.session.get() != roster.session() {
        refusals::foreign_session(&mut outcome, roster.session(), actor.session.get());
        return outcome;
    }
    if !roster.is_registered(&actor) {
        refusals::unknown_actor(&mut outcome, actor);
        return outcome;
    }

    // The mission callbacks and their dialogue come from the authoritative
    // lifecycle ledger. Only the kinds the roster has not seen are recorded,
    // so a replayed event is never reported twice.
    let kinds: Vec<LifecycleKind> = damage
        .lifecycle(&actor)
        .map(|seen| seen.iter().copied().collect())
        .unwrap_or_default();
    for kind in kinds {
        if roster
            .lifecycle(&actor)
            .is_some_and(|seen| seen.contains(&kind))
        {
            continue;
        }
        match roster.record_lifecycle(actor, kind) {
            Ok(event) => {
                outcome.report.recorded += 1;
                outcome.log.push(AllyConsumerEvent::Callback(event.clone()));
                if let Some(voice) = event.voice.clone() {
                    outcome.report.cues += 1;
                    outcome.log.push(AllyConsumerEvent::Dialogue(DialogueCue {
                        actor,
                        kind: event.kind,
                        voice,
                    }));
                }
            }
            Err(error) => refusals::lifecycle(&mut outcome, error),
        }
    }

    // The firing gate: an actor that may no longer act fires nothing. This is
    // the AC03 link — the aircraft's guns are taken out even when the lethal
    // hit landed on a node that is not itself a weapon mount, which the F29-C
    // mount-to-part pass does not cover.
    if !roster.is_acting(&actor) {
        let mounts: Vec<DamageNodeKey> = fire
            .definitions(&actor)
            .iter()
            .map(|definition| definition.mount().clone())
            .collect();
        for mount in mounts {
            let Some(state) = fire.state_mut(&actor) else {
                break;
            };
            if !state.is_disabled(&mount) {
                state.disable(&mount);
                outcome.report.mounts_disabled += 1;
                outcome
                    .log
                    .push(AllyConsumerEvent::MountDisabled { actor, mount });
            }
        }
    }

    outcome
}

/// Reconciles every actor the roster holds against the authoritative damage
/// lifecycle: the batch form of [`apply_ally_lifecycle`].
///
/// This is what a host runs after a cutscene resumes, so an ally killed while
/// the simulation was paused is reconciled before the next tick can fire.
/// The per-actor refusals are merged into one outcome.
#[must_use]
pub fn apply_roster_lifecycle(
    roster: &mut AlliesRoster,
    damage: &DamageResolver,
    fire: &mut FireResolver,
) -> AllyConsumerOutcome {
    let actors: Vec<ActorId> = roster.actors().collect();
    let mut total = AllyConsumerOutcome::default();
    for actor in actors {
        total.absorb(apply_ally_lifecycle(roster, actor, damage, fire));
    }
    total
}

/// The refusal constructors, so [`apply_ally_lifecycle`] stays readable.
mod refusals {
    use super::{AlliesError, AllyConsumerEvent, AllyConsumerOutcome, AllyConsumerRefusal};

    /// The actor belongs to another session generation.
    pub(super) fn foreign_session(outcome: &mut AllyConsumerOutcome, expected: u64, found: u64) {
        outcome.report.refused += 1;
        outcome.log.push(AllyConsumerEvent::Refused(
            AllyConsumerRefusal::ForeignSession { expected, found },
        ));
    }

    /// The actor is not registered with the roster.
    pub(super) fn unknown_actor(outcome: &mut AllyConsumerOutcome, actor: cs_sim::damage::ActorId) {
        outcome.report.refused += 1;
        outcome.log.push(AllyConsumerEvent::Refused(
            AllyConsumerRefusal::UnknownActor { actor },
        ));
    }

    /// The roster refused the lifecycle transition.
    pub(super) fn lifecycle(outcome: &mut AllyConsumerOutcome, error: AlliesError) {
        outcome.report.refused += 1;
        outcome
            .log
            .push(AllyConsumerEvent::Refused(AllyConsumerRefusal::Lifecycle(
                error,
            )));
    }
}

/// Component: ties an entity to one session-qualified actor and the declared
/// roster subject it was spawned under.
///
/// `actor` is the session-qualified [`ActorId`] the `AlliesRoster` registered
/// (its `session` is the session generation), `roster` the catalog subject of
/// the [`DeclaredRoster`] the session opened with, and `generation` the scene
/// generation the binding was spawned under — so a reload stamps new bindings
/// and stale ones are identified by mismatch, never by surviving pointers (the
/// `STATE-TRANSACTIONS` session-generation discipline; the same rule
/// [`crate::scene::SceneNodeBinding`] follows).
#[derive(Component, Clone, Debug, PartialEq)]
pub struct RosterBinding {
    /// The actor this entity presents.
    pub actor: ActorId,
    /// The catalog id of the declared roster the session runs under.
    pub roster: ContentId,
    /// The scene generation that spawned the binding.
    pub generation: SceneGeneration,
}

#[cfg(test)]
mod tests {
    use super::*;
    use cs_content::pilots::{
        DeclaredPilot, DeclaredRoster, DeclaredWingmate, WingmateSlot as DeclaredSlot,
    };
    use cs_types::content::{ContentKind, Known, Origin, Provenance};
    use cs_types::evidence::ClaimId;
    use cs_types::net::SessionId;

    fn id(kind: ContentKind, key: &str) -> ContentId {
        ContentId::from_source(kind, key).expect("test id is valid")
    }

    fn claim(key: &str) -> ClaimId {
        ClaimId::new(key).expect("test claim id is valid")
    }

    fn known<T>(value: T) -> Resolved<T> {
        Resolved::Known(Known::new(value, Provenance::designed(claim("f33a.test"))))
    }

    /// The declared synthetic roster lowers field-wise: the player faction,
    /// the two pilots with their voices, the two wingmate slots and the one
    /// neutral actor all arrive unchanged.
    #[test]
    fn accept_f33_a_lower_roster_maps_the_declared_roster() {
        let lowered = lower_roster(&cs_content::pilots::declared_synthetic_roster())
            .expect("the fixture lowers");

        assert_eq!(
            lowered.player_faction.as_content().as_str(),
            "faction/synthetic.nathan"
        );
        assert_eq!(lowered.pilots.len(), 2);
        assert_eq!(
            lowered.pilots[0].id.as_content().as_str(),
            "pilot/synthetic.nathan"
        );
        assert_eq!(lowered.pilots[0].voice.as_str(), "voice/synthetic.nathan");

        assert_eq!(lowered.wingmates.len(), 2);
        assert_eq!(lowered.wingmates[0].slot, WingmateSlot(1));
        assert_eq!(
            lowered.wingmates[0].pilot.as_content().as_str(),
            "pilot/synthetic.betty"
        );
        assert_eq!(
            lowered.wingmates[0].aircraft.as_content().as_str(),
            "airframe/synthetic.devastator"
        );
        assert_eq!(
            lowered.wingmates[0].loadout.as_str(),
            "loadout/synthetic.escort"
        );
        assert_eq!(
            lowered.wingmates[0].voice.as_str(),
            "voice/synthetic.betty",
            "the wingmate speaks through its pilot's voice"
        );
        assert_eq!(
            lowered.wingmates[0].survivability,
            SurvivabilityPolicy::Mortal
        );
        assert_eq!(
            lowered.wingmates[1].survivability,
            SurvivabilityPolicy::ScriptedInvulnerable
        );

        assert_eq!(lowered.neutral_traffic.len(), 1);
        assert_eq!(lowered.neutral_traffic[0].traffic, 1);
        assert_eq!(
            lowered.neutral_traffic[0].faction.as_content().as_str(),
            "faction/synthetic.traders"
        );
        assert_eq!(
            lowered.neutral_traffic[0].survivability,
            SurvivabilityPolicy::ProtectedNeutral
        );
    }

    /// An unknown voice refuses to lower (never a random line), and an
    /// unknown survivability refuses too (never a silent mortal).
    #[test]
    fn accept_f33_a_unknowns_refuse_to_lower() {
        let unknown_voice = DeclaredPilot::try_new(
            id(ContentKind::Pilot, "synthetic.nathan"),
            Resolved::unknown(claim("f33a.test.voice"), "no observed voice").expect("reason"),
            Origin::SyntheticFixture,
            Provenance::designed(claim("f33a.test")),
        )
        .expect("an unknown voice is a valid declared record");
        let declared = DeclaredRoster::try_new(
            id(ContentKind::Mission, "m01"),
            Origin::SyntheticFixture,
            id(ContentKind::Faction, "synthetic.nathan"),
            vec![unknown_voice],
            Vec::new(),
            Vec::new(),
            Provenance::designed(claim("f33a.test")),
        )
        .expect("the roster is valid");
        assert_eq!(
            lower_roster(&declared),
            Err(RosterLowerError::UnknownVoice {
                pilot: id(ContentKind::Pilot, "synthetic.nathan"),
                claim_id: claim("f33a.test.voice"),
                reason: "no observed voice".to_owned(),
            })
        );

        let pilot = DeclaredPilot::try_new(
            id(ContentKind::Pilot, "synthetic.nathan"),
            known(id(ContentKind::Voice, "synthetic.nathan")),
            Origin::SyntheticFixture,
            Provenance::designed(claim("f33a.test")),
        )
        .expect("the pilot is valid");
        let wingmate = DeclaredWingmate::try_new(
            DeclaredSlot(1),
            id(ContentKind::Pilot, "synthetic.nathan"),
            id(ContentKind::Airframe, "synthetic.fury"),
            id(ContentKind::Loadout, "synthetic.interceptor"),
            Resolved::unknown(claim("f33a.test.survivability"), "unmeasured escort policy")
                .expect("reason"),
            Origin::SyntheticFixture,
            Provenance::designed(claim("f33a.test")),
        )
        .expect("an unknown survivability is a valid declared record");
        let declared = DeclaredRoster::try_new(
            id(ContentKind::Mission, "m01"),
            Origin::SyntheticFixture,
            id(ContentKind::Faction, "synthetic.nathan"),
            vec![pilot],
            vec![wingmate],
            Vec::new(),
            Provenance::designed(claim("f33a.test")),
        )
        .expect("the roster is valid");
        assert_eq!(
            lower_roster(&declared),
            Err(RosterLowerError::UnknownSurvivability {
                record: "wingmate",
                index: 0,
                claim_id: claim("f33a.test.survivability"),
                reason: "unmeasured escort policy".to_owned(),
            })
        );
    }

    /// A roster that authors no neutral traffic lowers to an empty list, never
    /// a population default (F33 AC04's contract half).
    #[test]
    fn accept_f33_a_omitted_neutral_traffic_lowers_to_nothing() {
        let pilot = DeclaredPilot::try_new(
            id(ContentKind::Pilot, "synthetic.nathan"),
            known(id(ContentKind::Voice, "synthetic.nathan")),
            Origin::SyntheticFixture,
            Provenance::designed(claim("f33a.test")),
        )
        .expect("the pilot is valid");
        let declared = DeclaredRoster::try_new(
            id(ContentKind::Mission, "m01"),
            Origin::SyntheticFixture,
            id(ContentKind::Faction, "synthetic.nathan"),
            vec![pilot],
            Vec::new(),
            Vec::new(),
            Provenance::designed(claim("f33a.test")),
        )
        .expect("the roster is valid");
        let lowered = lower_roster(&declared).expect("the roster lowers");
        assert!(
            lowered.neutral_traffic.is_empty(),
            "an omitted list stays empty"
        );
    }

    /// The binding ties an entity to its session actor and roster subject
    /// under the scene generation that spawned it.
    #[test]
    fn accept_f33_a_roster_binding_is_generation_stamped() {
        let binding = RosterBinding {
            actor: ActorId {
                session: SessionId::new(3).expect("a nonzero session generation"),
                serial: 2,
            },
            roster: id(ContentKind::IaScenario, "synthetic.roster"),
            generation: SceneGeneration(4),
        };
        assert_eq!(binding.actor.session.get(), 3);
        assert_eq!(binding.roster.as_str(), "ia_scenario/synthetic.roster");
        assert_eq!(binding.generation, SceneGeneration(4));

        let stale = RosterBinding {
            generation: SceneGeneration(3),
            ..binding.clone()
        };
        assert_ne!(binding, stale, "a reload cannot alias a stale binding");
    }
}
