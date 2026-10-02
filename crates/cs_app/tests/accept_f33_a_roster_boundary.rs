//! Acceptance scenario F33-A: the declared → runtime roster boundary and the
//! capture contract.
//!
//! Spec: `specs/F33-wingmates-factions-neutral-traffic-and-pilot-identity.md`,
//! stage `### F33-A`. Task test prefix: `accept_f33_a_`.
//!
//! These tests drive production code only: [`cs_app::roster`]'s
//! [`lower_roster`] and [`RosterBinding`], the
//! `cs_content::pilots::DeclaredRoster` fixture that feeds it, and the
//! `cs_sim::allies::AlliesRoster` it registers actors and assignments with.
//! The AC01 scenario — a captured vehicle changes faction without changing its
//! geometry id — runs end-to-end through the lowered roster, so removing the
//! conversion, the capture transaction or the identity separation fails them.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data.

use cs_app::roster::{RosterBinding, RosterLowerError, lower_roster};
use cs_app::scene::SceneGeneration;
use cs_content::pilots::{
    DeclaredPilot, DeclaredRoster, DeclaredWingmate, WingmateSlot as DeclaredSlot,
    declared_synthetic_roster,
};
use cs_sim::allies::{
    AlliesError, AlliesRoster, SurvivabilityPolicy, synthetic_ally_roster, synthetic_faction,
};
use cs_sim::damage::ActorId;
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::net::SessionId;

const SESSION: u64 = 11;

fn session(value: u64) -> SessionId {
    SessionId::new(value).expect("a nonzero session generation")
}

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: session(SESSION),
        serial,
    }
}

fn id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("test id is valid")
}

fn claim(key: &str) -> ClaimId {
    ClaimId::new(key).expect("test claim id is valid")
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, Provenance::designed(claim("f33a.test"))))
}

/// AC01 end to end: the declared roster lowers to a runtime player faction,
/// the fixture actors register, and a captured raider changes faction while
/// its geometry id — and every wingmate's aircraft geometry — stays exactly
/// as it was.
#[test]
fn accept_f33_a_captured_vehicle_changes_faction_without_changing_geometry() {
    let lowered = lower_roster(&declared_synthetic_roster()).expect("the fixture lowers");
    let mut roster = AlliesRoster::new(SESSION);
    for record in synthetic_ally_roster(SESSION) {
        roster.register(record).expect("fixture records register");
    }
    roster
        .assign_wingmates(lowered.wingmates.clone())
        .expect("the lowered wingmates assign");

    let target = actor(9);
    let geometry_before = roster.geometry_of(&target).expect("registered").clone();
    let wingmate_aircraft_before = roster.wingmates()[0].aircraft.clone();
    assert_ne!(
        roster.faction_of(&target).expect("registered"),
        &lowered.player_faction,
        "the raider starts on another side"
    );

    let capture = roster
        .capture(target, lowered.player_faction.clone())
        .expect("the raider is registered");

    assert_eq!(capture.geometry, geometry_before);
    assert_eq!(
        roster.geometry_of(&target).expect("registered"),
        &geometry_before,
        "the captured vehicle keeps its geometry id"
    );
    assert_eq!(
        roster.faction_of(&target).expect("registered"),
        &lowered.player_faction,
        "the captured vehicle changed faction"
    );
    assert_eq!(
        roster.wingmates()[0].aircraft,
        wingmate_aircraft_before,
        "a capture does not reassign a wingmate's aircraft"
    );
}

/// The lowering preserves pilot, aircraft, loadout, voice and survivability as
/// separate fields, and lowers the authored neutral actor verbatim.
#[test]
fn accept_f33_a_lowered_wingmates_and_neutral_traffic_keep_their_identity() {
    let lowered = lower_roster(&declared_synthetic_roster()).expect("the fixture lowers");

    assert_eq!(
        lowered.player_faction.as_content().as_str(),
        "faction/synthetic.nathan"
    );
    assert_eq!(lowered.pilots.len(), 2);
    assert_eq!(lowered.wingmates.len(), 2);
    assert_eq!(lowered.wingmates[0].slot.index(), 1);
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
    assert_eq!(lowered.wingmates[0].voice.as_str(), "voice/synthetic.betty");
    assert_eq!(
        lowered.neutral_traffic.len(),
        1,
        "only the authored neutral actor is present"
    );
    assert_eq!(
        lowered.neutral_traffic[0].faction.as_content().as_str(),
        "faction/synthetic.traders"
    );
    assert_eq!(
        lowered.neutral_traffic[0].survivability,
        SurvivabilityPolicy::ProtectedNeutral
    );
}

/// A capture refuses an actor this session does not own and a foreign
/// generation's actor, and leaves the record and the wingmate set untouched.
#[test]
fn accept_f33_a_capture_refuses_unknown_and_foreign_actors() {
    let lowered = lower_roster(&declared_synthetic_roster()).expect("the fixture lowers");
    let mut roster = AlliesRoster::new(SESSION);
    for record in synthetic_ally_roster(SESSION) {
        roster.register(record).expect("fixture records register");
    }
    let geometry_before = roster.geometry_of(&actor(9)).expect("registered").clone();

    assert_eq!(
        roster.capture(actor(99), lowered.player_faction.clone()),
        Err(AlliesError::UnknownActor { actor: actor(99) })
    );
    assert_eq!(
        roster.capture(
            ActorId {
                session: session(SESSION + 1),
                serial: 9
            },
            lowered.player_faction.clone(),
        ),
        Err(AlliesError::ForeignSession {
            expected: SESSION,
            found: SESSION + 1,
        })
    );
    assert_eq!(
        roster.geometry_of(&actor(9)).expect("registered"),
        &geometry_before,
        "a refused capture leaves the vehicle untouched"
    );
    assert_eq!(
        roster.faction_of(&actor(9)).expect("registered"),
        &synthetic_faction("synthetic.raiders")
    );
}

/// An unmeasured pilot voice refuses to lower — a random line never replaces
/// a missing mission dialogue — and an unmeasured survivability refuses too,
/// so no actor silently becomes killable.
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

/// The binding record ties an entity to its session actor and roster subject
/// under the scene generation that spawned it.
#[test]
fn accept_f33_a_roster_binding_is_generation_stamped() {
    let declared = declared_synthetic_roster();
    let binding = RosterBinding {
        actor: actor(9),
        roster: declared.subject().clone(),
        generation: SceneGeneration(6),
    };
    assert_eq!(binding.actor.session, session(SESSION));
    assert_eq!(binding.roster.as_str(), "ia_scenario/synthetic.roster");
    assert_eq!(binding.generation, SceneGeneration(6));

    let stale = RosterBinding {
        generation: SceneGeneration(5),
        ..binding.clone()
    };
    assert_ne!(binding, stale, "a reload cannot alias a stale binding");
}
