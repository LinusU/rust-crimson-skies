//! Acceptance scenario F33-B: briefing-selected wingmate equipment, the
//! allegiance commitment and the retry reset.
//!
//! Spec: `specs/F33-wingmates-factions-neutral-traffic-and-pilot-identity.md`,
//! stage `### F33-B`. Task test prefix: `accept_f33_b_`.
//!
//! These tests drive production code only: the declared fixture roster in
//! `cs_content::pilots`, the lowering and session boundary
//! [`cs_app::roster::open_roster`], and the runtime rules in
//! `cs_sim::allies` (`briefed_wingmates`, `AlliesRoster::reset_wingmates`,
//! `register_wingmate` and the capture transaction). The AC02 scenario — the
//! wingmate loadout matches the briefing selection and a retry restores it —
//! runs end to end, so removing the rule, the reset or the allegiance
//! commitment fails these tests.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data.

use cs_app::roster::{lower_roster, open_roster};
use cs_content::pilots::declared_synthetic_roster;
use cs_sim::allies::{AlliesError, BriefingError, BriefingPlan, WingmateSlot, synthetic_faction};
use cs_sim::damage::ActorId;
use cs_types::content::{ContentId, ContentKind};
use cs_types::net::SessionId;

const SESSION: u64 = 21;

fn session(value: u64) -> SessionId {
    SessionId::new(value).expect("a nonzero session generation")
}

fn actor(session_value: u64, serial: u64) -> ActorId {
    ActorId {
        session: session(session_value),
        serial,
    }
}

fn loadout(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::Loadout, key).expect("test loadout id is valid")
}

fn airframe(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::Airframe, key).expect("test airframe id is valid")
}

/// AC02's happy path: the wingmate's loadout and airframe match the briefing
/// selection, and an unselected slot keeps its authored equipment.
#[test]
fn accept_f33_b_wingmate_loadout_matches_the_briefing_selection() {
    let lowered = lower_roster(&declared_synthetic_roster()).expect("the fixture lowers");
    let mut plan = BriefingPlan::new();
    plan.select(
        WingmateSlot(1),
        cs_sim::allies::GeometryId::try_new(airframe("synthetic.select")).expect("airframe"),
        loadout("synthetic.select"),
    );

    let roster = open_roster(SESSION, &lowered, &plan).expect("slot 1 is authored");

    assert_eq!(
        roster.player_faction(),
        Some(&lowered.player_faction),
        "the session commits the mission's player faction"
    );
    let first = roster.wingmate(WingmateSlot(1)).expect("slot 1 assigned");
    assert_eq!(
        first.loadout,
        loadout("synthetic.select"),
        "the wingmate carries the briefing loadout"
    );
    assert_eq!(
        first.aircraft.as_content().as_str(),
        "airframe/synthetic.select",
        "the wingmate flies the briefing airframe"
    );

    let second = roster.wingmate(WingmateSlot(2)).expect("slot 2 assigned");
    assert_eq!(
        second.loadout,
        loadout("synthetic.interceptor"),
        "an unselected slot keeps its authored loadout"
    );
    assert_eq!(
        second.aircraft.as_content().as_str(),
        "airframe/synthetic.fury",
        "an unselected slot keeps its authored airframe"
    );
}

/// AC02's reset half: after an in-session rearm and a capture, a retry is a
/// new session generation opened from the authored roster and the same
/// briefing, so the loadout returns to the briefing choice and the wingmate is
/// back on the player faction rather than the failed world's rearm/capture.
#[test]
fn accept_f33_b_a_retry_restores_the_briefing_loadout_and_allegiance() {
    let lowered = lower_roster(&declared_synthetic_roster()).expect("the fixture lowers");
    let mut plan = BriefingPlan::new();
    plan.select(
        WingmateSlot(1),
        cs_sim::allies::GeometryId::try_new(airframe("synthetic.select")).expect("airframe"),
        loadout("synthetic.select"),
    );
    let briefing_loadout = loadout("synthetic.select");
    let rearm = loadout("synthetic.rearm");

    // The first attempt.
    let mut first = open_roster(SESSION, &lowered, &plan).expect("slot 1 is authored");
    let wingman = actor(SESSION, 2);
    let record = first
        .register_wingmate(wingman, WingmateSlot(1))
        .expect("the slot is assigned and the faction is set");
    assert_eq!(
        record.faction, lowered.player_faction,
        "the wingmate flies for the mission's player faction"
    );
    assert_eq!(
        record.geometry.as_content().as_str(),
        "airframe/synthetic.select",
        "the wingmate geometry is the briefing airframe, its own field"
    );

    // The failed world's mutations: a rearm and a capture.
    first
        .set_wingmate_loadout(WingmateSlot(1), rearm.clone())
        .expect("the slot is assigned");
    first
        .capture(wingman, synthetic_faction("synthetic.raiders"))
        .expect("the wingman is registered");
    assert_eq!(
        first.wingmate(WingmateSlot(1)).expect("assigned").loadout,
        rearm,
        "the in-session rearm is visible before the retry"
    );
    assert_eq!(
        first.faction_of(&wingman),
        Some(&synthetic_faction("synthetic.raiders")),
        "the capture is visible before the retry"
    );

    // The retry: a fresh session generation opened the same way.
    let mut retry = open_roster(SESSION + 1, &lowered, &plan).expect("slot 1 is authored");
    assert_ne!(
        retry.session(),
        first.session(),
        "a retry is a new generation"
    );
    assert_eq!(
        retry.wingmate(WingmateSlot(1)).expect("assigned").loadout,
        briefing_loadout,
        "the retry restores the briefing loadout, not the failed rearm"
    );
    let wingman = actor(SESSION + 1, 2);
    let record = retry
        .register_wingmate(wingman, WingmateSlot(1))
        .expect("the retry slot is assigned");
    assert_eq!(
        record.faction, lowered.player_faction,
        "the retry wingmate is on the player faction, not the captured one"
    );
}

/// A briefing selection for a slot the mission never assigned is refused, so
/// no wingmate is invented at open time.
#[test]
fn accept_f33_b_briefing_refuses_an_unauthored_slot() {
    let lowered = lower_roster(&declared_synthetic_roster()).expect("the fixture lowers");
    let mut plan = BriefingPlan::new();
    plan.select(
        WingmateSlot(9),
        cs_sim::allies::GeometryId::try_new(airframe("synthetic.fury")).expect("airframe"),
        loadout("synthetic.interceptor"),
    );

    let result = open_roster(SESSION, &lowered, &plan);
    assert_eq!(
        result.err(),
        Some(BriefingError::UnknownWingmateSlot {
            slot: WingmateSlot(9)
        })
    );
}

/// A duplicate actor registration and an unassigned slot are distinct
/// refusals, and neither leaves a partial record behind.
#[test]
fn accept_f33_b_wingmate_registration_refuses_duplicates_and_unknown_slots() {
    let lowered = lower_roster(&declared_synthetic_roster()).expect("the fixture lowers");
    let mut roster =
        open_roster(SESSION, &lowered, &BriefingPlan::new()).expect("the authored set assigns");
    let wingman = actor(SESSION, 2);
    roster
        .register_wingmate(wingman, WingmateSlot(1))
        .expect("the slot is assigned");

    assert_eq!(
        roster.register_wingmate(wingman, WingmateSlot(1)),
        Err(BriefingError::Assign(AlliesError::DuplicateActor {
            actor: wingman
        }))
    );
    assert_eq!(
        roster.register_wingmate(actor(SESSION, 3), WingmateSlot(9)),
        Err(BriefingError::UnknownWingmateSlot {
            slot: WingmateSlot(9)
        })
    );
    assert!(
        !roster.is_registered(&actor(SESSION, 3)),
        "a refused registration registers nothing"
    );
}
