//! Acceptance task #2069: the `OrdinanceFeed` built from a plane record. Task
//! test prefix: `accept_parity5_ordinance_feed_`.
//!
//! * The first tests run the production feed over the committed engine facts
//!   (`[ordinance_feed]`, `[flight_check_default]`) and invented string text;
//!   they run in CI and prove the engine's arithmetic as read, not retail
//!   behaviour.
//! * The `retail` test reads the strings out of `$CS_GAME_DIR`; the image test
//!   checks the committed table against `$CS_ENGINE_IMAGE`. Both are
//!   `#[ignore]`d because CI has neither.

use cs_app::engine_facts::{FlightCheckPlaneSlot, engine_facts};
use cs_app::ui::front_end::ordinance_feed::{
    OrdinanceDraft, PlaneRecord, ProgressGate, RocketTable, ordinance_feed,
};
use cs_app::ui::front_end::ordinance_layout::{GunPosition, NO_GUN_KIND};

const UNLOCKED: ProgressGate = ProgressGate {
    unlocked: true,
    progress: 0,
};

fn locked(progress: i32) -> ProgressGate {
    ProgressGate {
        unlocked: false,
        progress,
    }
}

fn text(id: u32) -> Option<String> {
    Some(format!("s{id}"))
}

fn pilot() -> PlaneRecord {
    PlaneRecord::fresh_campaign(
        engine_facts().expect("committed table"),
        FlightCheckPlaneSlot::Pilot,
    )
}

#[test]
fn accept_parity5_ordinance_feed_a_guns_come_from_the_record() {
    let facts = engine_facts().expect("committed table");
    let feed = ordinance_feed(facts, &pilot(), UNLOCKED, &text);
    // Guns 2, 1, 0 and the no-gun id, ammunition 0, 0, 0 and 4: strings
    // 3320 + gun, the held row from +0x98.
    assert_eq!(
        feed.guns,
        vec![
            GunPosition {
                kind: 2,
                name: "s3322".into(),
                choice: 0
            },
            GunPosition {
                kind: 1,
                name: "s3321".into(),
                choice: 0
            },
            GunPosition {
                kind: 0,
                name: "s3320".into(),
                choice: 0
            },
            GunPosition {
                kind: NO_GUN_KIND,
                name: "s3325".into(),
                choice: 4
            },
        ]
    );
    assert_eq!(
        feed.ammo_rows,
        ["s3360", "s3361", "s3362", "s3363", "s3364"]
    );
}

#[test]
fn accept_parity5_ordinance_feed_b_rocket_rows_follow_the_gate() {
    let facts = engine_facts().expect("committed table");
    let ordinance = facts.ordinance();
    let all = RocketTable::new(ordinance, UNLOCKED);
    assert_eq!(all.count(), 12);
    assert_eq!(all.nth(11), 11);
    assert_eq!(
        all.nth(12),
        12,
        "past the last row the loop ends at the table length"
    );
    // Progress 0: only the records whose first dword is at most 1 pass.
    let start = RocketTable::new(ordinance, locked(0));
    assert_eq!(
        (0..start.count()).map(|n| start.nth(n)).collect::<Vec<_>>(),
        [0, 1, 11]
    );
    // Progress 7: first <= 8.
    let mid = RocketTable::new(ordinance, locked(7));
    assert_eq!(
        (0..mid.count()).map(|n| mid.nth(n)).collect::<Vec<_>>(),
        [0, 1, 2, 3, 4, 6, 7, 11]
    );
    assert_eq!(mid.row_of(7), 6);
    assert_eq!(mid.row_of(11), 7);
    assert_eq!(mid.row_of(-1), -1);
    // Row of a record that is itself hidden: the passing rows before it.
    assert_eq!(start.row_of(5), 1);
    assert_eq!(start.row_of(0), 0);

    let feed = ordinance_feed(facts, &pilot(), locked(0), &text);
    assert_eq!(feed.rocket_rows, ["s3395", "s3396", "s3406"]);
    let feed = ordinance_feed(facts, &pilot(), UNLOCKED, &text);
    assert_eq!(feed.rocket_rows.len(), 12);
    assert_eq!(feed.rocket_rows[11], "s3406");
}

#[test]
fn accept_parity5_ordinance_feed_c_rocket_boxes_follow_the_plane() {
    let facts = engine_facts().expect("committed table");
    // The fresh campaign plane has two low and two high boxes: boxes 0, 1, 4
    // and 5 hold rocket 1; 2, 3, 6 and 7 do not exist.
    let feed = ordinance_feed(facts, &pilot(), UNLOCKED, &text);
    assert_eq!(
        feed.rockets,
        [Some(1), Some(1), None, None, Some(1), Some(1), None, None]
    );
    // A third low box exists once the count says so, and holds rocket 11
    // (none), the last row of the table.
    let mut record = pilot();
    record.low_boxes = 3;
    let feed = ordinance_feed(facts, &record, UNLOCKED, &text);
    assert_eq!(feed.rockets[2], Some(11));
    // The high boxes are counted from four plus the dword.
    record.high_boxes = 1;
    let feed = ordinance_feed(facts, &record, UNLOCKED, &text);
    assert_eq!(feed.rockets[4..], [Some(1), None, None, None]);
}

#[test]
fn accept_parity5_ordinance_feed_d_the_stores_edit_a_working_copy() {
    let facts = engine_facts().expect("committed table");
    let table = RocketTable::new(facts.ordinance(), locked(7));
    let mut draft = OrdinanceDraft::load(pilot());
    assert!(draft.choose_ammo(1, 3));
    assert!(draft.choose_rocket(&table, 0, 5));
    assert!(!draft.choose_ammo(4, 0));
    assert!(!draft.choose_rocket(&table, 8, 0));
    // Row 5 of the progress-7 table is record 6.
    assert_eq!(draft.working().ammo[1], 3);
    assert_eq!(draft.working().rockets[0], 6);
    assert_eq!(draft.live(), &pilot(), "nothing is live before the commit");
    let committed = draft.commit().clone();
    assert_eq!(committed.ammo[1], 3);
    assert_eq!(draft.live(), draft.working());
    // The rocket row the screen then shows is the one that was chosen.
    let feed = ordinance_feed(facts, &committed, locked(7), &text);
    assert_eq!(feed.rockets[0], Some(5));
    assert_eq!(feed.guns[1].choice, 3);
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity5_ordinance_feed_retail_the_installations_strings_fill_the_feed() {
    use cs_app::ui::front_end::FrontEndScreens;
    let Ok(game_dir) = std::env::var("CS_GAME_DIR") else {
        panic!("CS_GAME_DIR is unset: this run needs the original installation");
    };
    let root = std::path::PathBuf::from(game_dir);
    let screens = FrontEndScreens::open(&root).expect("the installation's screens open");
    let strings = cs_app::mission_session::FlightCheckStrings::read(&root, &screens)
        .expect("the string table reads");
    let facts = engine_facts().expect("committed table");
    let lookup = |id: u32| strings.resolve_id_tagged(id);
    let feed = ordinance_feed(facts, &pilot(), UNLOCKED, &lookup);
    assert_eq!(feed.guns.len(), 4);
    assert!(feed.guns.iter().all(|gun| !gun.name.is_empty()));
    assert_eq!(feed.ammo_rows.len(), 5);
    assert!(feed.ammo_rows.iter().all(|row| !row.is_empty()));
    assert_eq!(feed.rocket_rows.len(), 12);
    assert!(feed.rocket_rows.iter().all(|row| !row.is_empty()));
    // The screen takes the feed.
    let layout = screens.original_layout().expect("the real layout reads");
    let name = |name: &str| strings.resolve_tagged(name);
    let mut screen = cs_app::ui::front_end::ordinance_layout::OrdinanceLayoutScreen::from_layout(
        &layout,
        cs_app::ui::front_end::ordinance_layout::OrdinanceSide::Pilot,
        0,
        &name,
    )
    .expect("the real OrdinanceLayout rows read");
    screen.init(feed);
    assert!(screen.ammo_active(0));
    assert!(!screen.ammo_active(3));
    assert!(screen.rocket_active(0));
    assert!(!screen.rocket_active(2));
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_parity5_ordinance_feed_image_the_rocket_table_matches_the_image() {
    use cs_content::coordinates::{load_engine_image, original_image_digest};
    let loaded = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    assert_eq!(loaded.digest, original_image_digest());
    let image = loaded.bytes.as_slice();
    let facts = engine_facts().expect("committed table");
    let dword = |va: u64| -> i32 {
        let at = usize::try_from(facts.image_offset(va).expect("the va is inside the image"))
            .expect("an offset");
        i32::from_le_bytes(image[at..at + 4].try_into().expect("four bytes"))
    };
    let entries = facts.ordinance().rocket_entries();
    assert_eq!(
        entries.len() as u64,
        facts.ordinance().value("table_entries")
    );
    for (index, entry) in entries.iter().enumerate() {
        assert_eq!(
            entry.va,
            facts.ordinance().value("table_va") + 8 * index as u64
        );
        assert_eq!(
            (dword(entry.va), dword(entry.va + 4)),
            (entry.first, entry.aux)
        );
    }
    // The fresh-campaign record's box counts (`+0x34`, `+0x38`).
    let ordinance = facts.ordinance();
    for slot in [FlightCheckPlaneSlot::Pilot, FlightCheckPlaneSlot::Wingman] {
        let va = facts.flight_check().default_for(slot).va;
        assert_eq!(
            dword(va + ordinance.value("box_low_count_offset")) as u64,
            ordinance.value("default_box_low_count")
        );
        assert_eq!(
            dword(va + ordinance.value("box_high_count_offset")) as u64,
            ordinance.value("default_box_high_count")
        );
    }
}
