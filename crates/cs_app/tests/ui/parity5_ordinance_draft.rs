//! Acceptance task #2072: the Instant Action draft's `PlaneRecord` in and out.
//! Task test prefix: `accept_parity5_ordinance_draft_`.
//!
//! The first tests run the production conversion over the measured airframe
//! templates (`cs_content::weapons`) and the committed engine facts; they run
//! in CI and prove the mapping as read, not retail behaviour. The image test
//! checks the gate readings against `$CS_ENGINE_IMAGE` and is `#[ignore]`d
//! because CI has no image.

use cs_app::engine_facts::engine_facts;
use cs_app::ui::front_end::ordinance_draft::{DraftRecordError, commit_record};
use cs_app::ui::front_end::ordinance_feed::{
    OrdinanceDraft, PlaneRecord, ProgressGate, ordinance_feed,
};
use cs_content::instant_action::{
    RosterSlot, ScenarioActorSpec, ScenarioSide, SlotOrdnance, synthetic_player_faction,
    synthetic_provenance,
};
use cs_content::pilots::DeclaredSurvivability;
use cs_content::weapons::original_airframe_loadout;
use cs_types::content::{ContentId, ContentKind, Known, Resolved};
use cs_types::evidence::ClaimId;

fn airframe(key: &str) -> Resolved<ContentId> {
    Resolved::Known(Known::new(
        ContentId::from_source(ContentKind::Airframe, key).expect("airframe id is valid"),
        synthetic_provenance(),
    ))
}

fn actor_with(airframe: Resolved<ContentId>) -> ScenarioActorSpec {
    ScenarioActorSpec::try_new_resolved(
        ScenarioSide::Player,
        RosterSlot(0),
        Resolved::Known(Known::new(
            synthetic_player_faction(),
            synthetic_provenance(),
        )),
        airframe,
        Resolved::unknown(
            ClaimId::new("p5.draft").expect("claim"),
            "ordnance is per-slot",
        )
        .expect("reason"),
        None,
        Resolved::Known(Known::new(
            DeclaredSurvivability::Mortal,
            synthetic_provenance(),
        )),
        synthetic_provenance(),
    )
    .expect("the actor is structurally valid")
}

fn text(id: u32) -> Option<String> {
    Some(format!("s{id}"))
}

#[test]
fn accept_parity5_ordinance_draft_a_record_takes_the_airframe_template() {
    let record = PlaneRecord::from_actor(&actor_with(airframe("player_brigand")))
        .expect("a measured airframe");
    let template = original_airframe_loadout("player_brigand").expect("template");
    assert_eq!(record.plane_id, 4);
    assert_eq!(&record.guns, template.guns());
    assert_eq!(&record.ammo, template.ammo());
    assert_eq!(&record.rockets, template.rockets());
    assert_eq!((record.low_boxes, record.high_boxes), (2, 2));
    // The airframe's boxes decide which rocket boxes the feed offers.
    let facts = engine_facts().expect("committed table");
    let feed = ordinance_feed(facts, &record, ProgressGate::default(), &text);
    let live: Vec<bool> = feed.rockets.iter().map(Option::is_some).collect();
    assert_eq!(live, [true, true, false, false, true, true, false, false]);
}

#[test]
fn accept_parity5_ordinance_draft_b_slot_ordnance_overrides_only_the_stores() {
    let picked = SlotOrdnance::try_new([0, 1, 4, 0], [11, 1, 11, 11, 1, 11, 11, 11]).expect("ok");
    let record =
        PlaneRecord::from_actor(&actor_with(airframe("player_brigand")).with_slot_ordnance(picked))
            .expect("record");
    assert_eq!(record.ammo, [0, 1, 4, 0]);
    assert_eq!(record.rockets, [11, 1, 11, 11, 1, 11, 11, 11]);
    let template = original_airframe_loadout("player_brigand").expect("template");
    assert_eq!(&record.guns, template.guns());
}

#[test]
fn accept_parity5_ordinance_draft_c_an_edit_round_trips_into_the_actor() {
    let facts = engine_facts().expect("committed table");
    let actor = actor_with(airframe("player_avenger"));
    let record = PlaneRecord::from_actor(&actor).expect("record");
    let mut draft = OrdinanceDraft::load(record);
    assert!(draft.choose_ammo(1, 3));
    let table = cs_app::ui::front_end::ordinance_feed::RocketTable::new(
        facts.ordinance(),
        ProgressGate {
            unlocked: true,
            progress: 0,
        },
    );
    assert!(draft.choose_rocket(&table, 0, 2));
    let committed = draft.commit().clone();
    let actor = commit_record(actor, &committed).expect("storable");
    let stored = actor
        .slot_ordnance()
        .expect("the actor now holds a selection");
    assert_eq!(stored.gun_ammo()[1], 3);
    assert_eq!(stored.rockets()[0], table.nth(2) as u32);
    // Reading it back gives the committed record.
    assert_eq!(PlaneRecord::from_actor(&actor).expect("record"), committed);
}

#[test]
fn accept_parity5_ordinance_draft_d_an_unmeasured_airframe_has_no_record() {
    let unknown = Resolved::unknown(ClaimId::new("p5.draft").expect("claim"), "not measured")
        .expect("reason");
    assert_eq!(
        PlaneRecord::from_actor(&actor_with(unknown)),
        Err(DraftRecordError::UnknownAirframe)
    );
    assert_eq!(
        PlaneRecord::from_actor(&actor_with(airframe("player_nothing"))),
        Err(DraftRecordError::NoTemplate("player_nothing".to_owned()))
    );
}

#[test]
fn accept_parity5_ordinance_draft_e_a_record_outside_the_domain_is_refused() {
    let actor = actor_with(airframe("player_avenger"));
    let mut record = PlaneRecord::from_actor(&actor).expect("record");
    record.ammo[0] = 99;
    assert!(matches!(
        commit_record(actor, &record),
        Err(DraftRecordError::Refused(_))
    ));
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_parity5_ordinance_draft_image_the_gate_readings_match_the_image() {
    use cs_content::coordinates::{load_engine_image, original_image_digest};
    let loaded = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    assert_eq!(loaded.digest, original_image_digest());
    let image = loaded.bytes.as_slice();
    let facts = engine_facts().expect("committed table");
    let ordinance = facts.ordinance();
    let at = |va: u64| {
        usize::try_from(facts.image_offset(va).expect("inside the image")).expect("an offset")
    };
    let dword = |va: u64| u32::from_le_bytes(image[at(va)..at(va) + 4].try_into().expect("4"));
    let name = |va: u64| {
        let bytes = &image[at(va)..];
        let end = bytes.iter().position(|&b| b == 0).expect("terminated");
        String::from_utf8(bytes[..end].to_vec()).expect("ascii")
    };
    // Each flag cell is pushed with its name for the registration call.
    for (flag, expected) in [
        ("flag_a", "fAllowAll"),
        ("flag_b", "fMultiPlayerConstruction"),
        ("flag_c", "fIAConstruction"),
    ] {
        assert_eq!(
            u64::from(dword(ordinance.value(&format!("{flag}_register_va")))),
            ordinance.value(&format!("{flag}_va"))
        );
        assert_eq!(name(ordinance.value(&format!("{flag}_name_va"))), expected);
    }
    // The three stores to the progress cell, and the count cell they read.
    for key in [
        "progress_max_store_va",
        "progress_count_store_va",
        "progress_reset_store_va",
    ] {
        assert_eq!(
            u64::from(dword(ordinance.value(key))),
            ordinance.value("progress_cell_va"),
            "{key}"
        );
    }
    assert_eq!(
        u64::from(dword(ordinance.value("progress_max_store_va") - 13)),
        ordinance.value("progress_count_va")
    );
}
