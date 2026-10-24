//! Acceptance task PARITY5 (#2053): the Plane Construction hub's purchase panel
//! (Ready, `[@Purchase@]`, *CONSTRUCTION TOTALS*). Task test prefix:
//! `accept_parity5_cabin_plane_construction_purchase_`.
//!
//! * The synthetic tests run over layout text in the original's *format* (the
//!   real `[@Purchase@]` rows, invented strings and plane records). They prove
//!   the page's rules (the engine's weight and cost arithmetic, the problems
//!   line, the name check, the hangar's free record), the route from Ready to
//!   the purchase and the production `ConstructionScreen::commit`; they run in
//!   CI and prove nothing about retail data.
//! * The `retail` test reads `$CS_GAME_DIR`: the real rows and the
//!   installation's own strings. `#[ignore]`d because CI has no installation.

use cs_app::construction::{ConstructionContext, ConstructionScreen};
use cs_app::ui::front_end::construction_hangar::SavedPlane;
use cs_app::ui::front_end::construction_purchase::{
    DEFAULT_SEPARATOR, FREE_RESERVE, PURCHASE_BUTTON, PURCHASE_SECTION, Problem, PurchaseError,
    PurchaseFeed, PurchaseOutcome, PurchasePage, capacity, commit_purchase, engine_cost, free_slot,
    plane_value, plane_weight, problems,
};
use cs_app::ui::front_end::construction_stats::PlaneRecord;
use cs_app::ui::front_end::{HubOutcome, HubPage, OriginalLayout, READY_NOT_BUILT};
use cs_content::construction::declared_synthetic_blueprint;

use super::parity5_cabin_plane_construction_hangar::{GLOBALS, HANGAR_ROWS, hangar_names};
use super::parity5_cabin_plane_construction_hub::{LAYOUT, draft, ids, names};

/// The real `[@Purchase@]` rows.
const PURCHASE_ROWS: &str = "[@Purchase@]
V1=TEXTLEFT,412
V2=TEXTCENT,642
V3=TEXTRIGHT,705
V4=LINE0,84
V5=LINE1,131
V6=LINE2,165
V7=LINE3,194
V8=LINE4,274
V9=LINE5,354
V10=LINE6,388
V11=TCOLOR,0xFF354560
V12=TCOLOR1,0xFFffffff
V13=TCOLOR2,0xFF000000
V14=PWIDTH,50
V15=TEXTLEFTI,417
V16=LINE7,430
PUR_B_PURCHASE  =B,SB_B_PaperButton.png,510,465,0,IDS_PUR_B_PURCHASE,,,,,,,,,,<TCOLOR1>,<TCOLOR2>,<TCOLOR2>,<TCOLOR2>
PUR_T_TITLE			=T,IDS_PUR_TITLE,426,60,0,0,0,0xFF000000,0
PUR_T_ITEMTITLE     =T,IDS_PUR_ITEM_TITLE,<TEXTLEFT>,<LINE0>,0,225,<PX_TEXTH>,0xFF000000,0
PUR_T_WEIGHTTITLE   =T,IDS_PUR_WEIGHT_TITLE,<TEXTCENT>,<LINE0>,0,<PWIDTH>,<PX_TEXTH>,0xFF000000,2
PUR_T_COSTTITLE     =T,IDS_PUR_COST_TITLE,<TEXTRIGHT>,<LINE0>,0,<PWIDTH>,<PX_TEXTH>,0xFF000000,2
PUR_T_AIRFRAMETITLE =T,IDS_PX_B_AIRFRAME,<TEXTLEFT>,115,0,225,<PX_TEXTH>,0xFF000000,0
PUR_T_ENGINETITLE   =T,IDS_PX_B_ENGINE,<TEXTLEFT>,148,0,0,0,0xFF000000,0
PUR_T_ARMORTITLE    =T,IDS_PX_B_ARMOR,<TEXTLEFT>,181,0,225,<PX_TEXTH>,0xFF000000,0
PUR_T_GUNSTITLE     =T,IDS_PX_B_GUNS,<TEXTLEFT>,260,0,225,<PX_TEXTH>,0xFF000000,0
PUR_T_HARDPOINTTITLE=T,IDS_PX_B_HARDPOINTS,<TEXTLEFT>,340,0,225,<PX_TEXTH>,0xFF000000,0
PUR_T_TOTALSTITLE   =T,IDS_PUR_TOTALS_TITLE,<TEXTLEFT>,<LINE7>,0,225,<PX_TEXTH>,0xFF000000,0
PUR_T_AIRFRAME      =T,,<TEXTLEFTI>,<LINE1>,0,220,<PX_TEXTH>,<TCOLOR>,0
PUR_T_AIRFRAMEWEIGHT=T,,<TEXTCENT>,<LINE1>,0,<PWIDTH>,<PX_TEXTH>,<TCOLOR>,2
PUR_T_AIRFRAMECOST  =T,,<TEXTRIGHT>,<LINE1>,0,<PWIDTH>,<PX_TEXTH>,<TCOLOR>,2
PUR_T_ENGINE        =T,,<TEXTLEFTI>,<LINE2>,0,220,<PX_TEXTH>,<TCOLOR>,0
PUR_T_ENGINEWEIGHT  =T,,<TEXTCENT>,<LINE2>,0,<PWIDTH>,<PX_TEXTH>,<TCOLOR>,2
PUR_T_ENGINECOST    =T,,<TEXTRIGHT>,<LINE2>,0,<PWIDTH>,<PX_TEXTH>,<TCOLOR>,2
PUR_T_PROBLEMS      =T,,<TEXTLEFTI>,<LINE6>,0,334,36,0xFFFF0000,3
PUR_T_TOTALSWEIGHT  =T,,<TEXTCENT>,<LINE7>,0,<PWIDTH>,<PX_TEXTH>,0xFF000000,2
PUR_T_TOTALSCOST    =T,,<TEXTRIGHT>,<LINE7>,0,<PWIDTH>,<PX_TEXTH>,0xFF000000,2
PUR_T_ARMORLIST     =A,<TEXTLEFTI>,<LINE3>,0,220,<STDTEXTH>,<TCOLOR>,0,1
PUR_T_ARMORWEIGHT   =A,<TEXTCENT>,<LINE3>,0,<PWIDTH>,<STDTEXTH>,<TCOLOR>,2,1
PUR_T_ARMORCOST     =A,<TEXTRIGHT>,<LINE3>,0,<PWIDTH>,<STDTEXTH>,<TCOLOR>,2,1
PUR_T_GUNLIST       =A,<TEXTLEFTI>,<LINE4>,0,224,<STDTEXTH>,<TCOLOR>,0,1
PUR_T_GUNWEIGHT     =A,<TEXTCENT>,<LINE4>,0,<PWIDTH>,<STDTEXTH>,<TCOLOR>,2,1
PUR_T_GUNCOST       =A,<TEXTRIGHT>,<LINE4>,0,<PWIDTH>,<STDTEXTH>,<TCOLOR>,2,1
PUR_T_HARDPOINTLIST  =A,<TEXTLEFTI>,<LINE5>,0,220,<STDTEXTH>,<TCOLOR>,0,1
PUR_T_HARDPOINTWEIGHT=A,<TEXTCENT>,<LINE5>,0,<PWIDTH>,<STDTEXTH>,<TCOLOR>,2,1
PUR_T_HARDPOINTCOST  =A,<TEXTRIGHT>,<LINE5>,0,<PWIDTH>,<STDTEXTH>,<TCOLOR>,2,1
";

fn layout_text() -> String {
    let hub = LAYOUT.replace(
        "	[@Purchase@]
PU_P_BACKGROUND=P,bg.png,0,0,1,1,0,0,0
",
        "",
    );
    assert!(!hub.contains("PU_P_BACKGROUND"), "the fixture was edited");
    format!("{GLOBALS}{hub}\n{PURCHASE_ROWS}\n{HANGAR_ROWS}")
}

fn layout() -> OriginalLayout {
    OriginalLayout::parse(&layout_text()).expect("the fixture parses")
}

fn purchase_names(name: &str) -> Option<String> {
    Some(
        match name {
            "IDS_PUR_TITLE" => "[AB14I]CONSTRUCTION TOTALS",
            "IDS_PUR_ITEM_TITLE" => "[TREB10]Item",
            "IDS_PUR_WEIGHT_TITLE" => "[TREB10]Weight",
            "IDS_PUR_COST_TITLE" => "[TREB10]Cost",
            "IDS_PUR_TOTALS_TITLE" => "[TREB10]Totals",
            "IDS_PUR_B_PURCHASE" => "[TREB9B]Purchase Now",
            other => return hangar_names(other),
        }
        .to_owned(),
    )
}

/// The engine's strings in their real format, invented names.
fn purchase_ids(id: u32) -> Option<String> {
    let text = match id {
        203 => "[COUR9]You must enter a name for your new plane.".to_owned(),
        204 => "[COUR9]You have reached your hangar limit of planes.".to_owned(),
        506 => "(%1!d!) ".to_owned(),
        703 => "[COUR9]This %1!s! has been purchased and delivered to your hangar.".to_owned(),
        1171 => "No Engine Selected".to_owned(),
        1176 => "Left Wing: %1!d!".to_owned(),
        1177 => "Right Wing: %1!d!".to_owned(),
        1178 => "$%1!d!".to_owned(),
        1182 => "[COUR9]CAN'T PURCHASE: ".to_owned(),
        1183 => "[COUR9]NO ENGINE".to_owned(),
        1191 => "[COUR9]Nose: %1!d! units".to_owned(),
        1192 => "[COUR9]Tail: %1!d! units".to_owned(),
        1193 => "[COUR9]Left Wing: %1!d! units".to_owned(),
        1194 => "[COUR9]Right Wing: %1!d! units".to_owned(),
        1226 => "[COUR9]INSUFFICIENT FUNDS".to_owned(),
        1227 => "[COUR9]OVERWEIGHT".to_owned(),
        3000..=3010 => format!("[COUR9]Airframe {}", id - 3000),
        3020..=3030 => format!("Short {}", id - 3020),
        3100..=3165 => format!("Engine {}", id - 3100),
        3310..=3314 => format!("Gun {}", id - 3310),
        other => return ids(other),
    };
    Some(text)
}

fn page() -> PurchasePage {
    PurchasePage::from_layout(&layout(), &purchase_names, &purchase_ids)
        .expect("the fixture has the rows")
}

fn hub() -> HubPage {
    HubPage::from_layout(&layout(), &purchase_names, &purchase_ids)
        .expect("the fixture has the rows")
}

/// Airframe 1 (cost 4548, weight 3255, capacity 9525; its gun mask has slot 3
/// on the second column), engine 2, two guns (slot 0 paired), 20 units of
/// armor and four hardpoints.
fn record() -> PlaneRecord {
    PlaneRecord {
        airframe: 1,
        engine: 2,
        armor: [10, 0, 5, 5],
        hardpoints: [2, 2],
        guns: [0, 5, 5, 3],
        gun_pairs: 0b0001,
    }
}

fn row(name: &str, weight: &str, cost: &str) -> [String; 3] {
    [name.to_owned(), weight.to_owned(), cost.to_owned()]
}

fn feed(cash: i64) -> PurchaseFeed {
    PurchaseFeed {
        record: record(),
        cash,
    }
}

fn text<'a>(
    texts: &'a [cs_app::ui::front_end::construction_hub::HubText],
    id: &str,
) -> Vec<&'a str> {
    texts
        .iter()
        .filter(|text| text.id == id)
        .map(|text| text.text.as_str())
        .collect()
}

// ------------------------------------------------------- the synthetic half ---

/// `0x405550` and `0x405680` over the tables, worked by hand:
///
/// * weight: airframe 3255, engine (base 2000 + 500), guns (slot 0: kind 0,
///   first column 280, paired, so 560; slot 3: kind 3, second column 820),
///   plus `4 * (20 + 120 * 4)` = 2000;
/// * cost: airframe 4548, engine (base 1700 + 425), guns (240 * 2 + 700),
///   plus `4 * 20 + 410 * 4` = 1720.
#[test]
fn accept_parity5_cabin_plane_construction_purchase_a_the_weight_and_cost_follow_the_engines_arithmetic()
 {
    let record = record();
    assert_eq!(plane_weight(&record), Ok(3255 + 2500 + 560 + 820 + 2000));
    assert_eq!(plane_value(&record), Ok(4548 + 2125 + 480 + 700 + 1720));
    assert_eq!(engine_cost(&record), Ok(2125));
    assert_eq!(capacity(&record), Ok(9525));
    // Engine 6 is no engine: nothing for it, in the weight or the cost.
    let bare = PlaneRecord {
        engine: 6,
        ..record
    };
    assert_eq!(engine_cost(&bare), Ok(0));
    assert_eq!(plane_weight(&bare), Ok(3255 + 560 + 820 + 2000));
    // The six engines' offsets from the airframe's base (1700 for airframe 1).
    for (engine, offset) in [-425, 0, 425, 5, 430, 855].into_iter().enumerate() {
        let plane = PlaneRecord {
            engine: engine as u32,
            ..record
        };
        assert_eq!(engine_cost(&plane), Ok(1700 + offset), "engine {engine}");
    }
    // A record the tables do not cover is refused, never guessed.
    let wild = PlaneRecord {
        airframe: 11,
        ..record
    };
    assert_eq!(plane_value(&wild), Err(PurchaseError::Airframe(11)));
    let gun = PlaneRecord {
        guns: [9, 5, 5, 5],
        ..record
    };
    assert_eq!(plane_weight(&gun), Err(PurchaseError::Gun(9)));
}

/// `E 2264`: funds, then weight, then the engine; joined by the list
/// separator and a space; empty and "go" with no problem.
#[test]
fn accept_parity5_cabin_plane_construction_purchase_b_the_problems_line_lists_what_stops_the_purchase()
 {
    let mut page = page();
    let value = i64::from(plane_value(&record()).expect("covered"));
    page.set_feed(feed(value)).expect("covered");
    assert!(page.problems().is_empty());
    assert_eq!(page.problems_text(), "");
    assert!(page.button_enabled());

    page.set_feed(feed(value - 1)).expect("covered");
    assert_eq!(page.problems(), [Problem::InsufficientFunds]);
    assert_eq!(page.problems_text(), "CAN'T PURCHASE: INSUFFICIENT FUNDS");
    assert!(!page.button_enabled());

    // Airframe 0 holds 4160: armor alone is over it, with no engine either.
    let heavy = PlaneRecord {
        airframe: 0,
        engine: 6,
        armor: [300, 300, 300, 300],
        hardpoints: [0, 0],
        guns: [5, 5, 5, 5],
        gun_pairs: 0,
    };
    assert_eq!(
        problems(&heavy, 0),
        Ok(vec![
            Problem::InsufficientFunds,
            Problem::Overweight,
            Problem::NoEngine
        ])
    );
    page.set_feed(PurchaseFeed {
        record: heavy,
        cash: 0,
    })
    .expect("covered");
    assert_eq!(
        page.problems_text(),
        format!(
            "CAN'T PURCHASE: INSUFFICIENT FUNDS{s} OVERWEIGHT{s} NO ENGINE",
            s = DEFAULT_SEPARATOR
        )
    );
    page.set_separator(";");
    assert!(
        page.problems_text()
            .contains("FUNDS; OVERWEIGHT; NO ENGINE")
    );
    // Exactly at the capacity is not over it (`cmp capacity, weight; jge`).
    let at = PlaneRecord {
        armor: [0; 4],
        ..heavy
    };
    let weight = plane_weight(&at).expect("covered");
    assert!(weight < capacity(&at).expect("covered"));
}

#[test]
fn accept_parity5_cabin_plane_construction_purchase_c_the_rows_follow_the_record() {
    let mut page = page();
    assert!(page.airframe_line().is_none(), "no plane, no figures");
    page.set_feed(feed(100_000)).expect("covered");
    assert_eq!(
        page.airframe_line(),
        Some(row("Airframe 1", "3255", "4548"))
    );
    // Engine name: string 3100 + 6 * airframe + engine.
    assert_eq!(page.engine_line(), Some(row("Engine 8", "2500", "2125")));
    // Zones with units only, in zone order; weight and cost 4 per unit.
    assert_eq!(
        page.armor_lines(),
        [
            row("Nose: 10 units", "40", "40"),
            row("Left Wing: 5 units", "20", "20"),
            row("Right Wing: 5 units", "20", "20"),
        ]
    );
    assert_eq!(
        page.gun_lines(),
        [
            row("(2) Gun 0", "560", "480"),
            row("(1) Gun 3", "820", "700"),
        ]
    );
    // Weight 480 and cost 410 a hardpoint.
    assert_eq!(
        page.hardpoint_lines(),
        [
            row("Left Wing: 2", "960", "820"),
            row("Right Wing: 2", "960", "820"),
        ]
    );
    assert_eq!(
        page.totals_line(),
        Some([
            "{}".replace("{}", &plane_weight(&record()).unwrap().to_string()),
            "$9573".into()
        ])
    );
    // No engine: the name is string 1171, the figures empty, the name red.
    let bare = PlaneRecord {
        engine: 6,
        ..record()
    };
    page.set_feed(PurchaseFeed {
        record: bare,
        cash: 100_000,
    })
    .expect("covered");
    assert_eq!(
        page.engine_line(),
        Some(["No Engine Selected".into(), String::new(), String::new()])
    );
    let texts = page.texts();
    let red: Vec<u32> = texts
        .iter()
        .filter(|text| text.id == "PUR_T_ENGINE")
        .map(|text| text.color)
        .collect();
    assert_eq!(red, [0xFFFF_0000]);
    // `E 2259` counts the non-zero words and row 0 reads the left word: with
    // only the right word set the one row shows the left word, 0.
    let right_only = PlaneRecord {
        hardpoints: [0, 3],
        ..record()
    };
    page.set_feed(PurchaseFeed {
        record: right_only,
        cash: 100_000,
    })
    .expect("covered");
    assert_eq!(page.hardpoint_lines(), [row("Left Wing: 0", "0", "0")]);
}

#[test]
fn accept_parity5_cabin_plane_construction_purchase_d_the_texts_sit_on_the_layouts_rows() {
    let mut page = page();
    // Without a plane only the fixed strings are drawn.
    let texts = page.texts();
    assert_eq!(text(&texts, "PUR_T_TITLE"), ["CONSTRUCTION TOTALS"]);
    assert_eq!(text(&texts, "PUR_T_ITEMTITLE"), ["Item"]);
    assert_eq!(text(&texts, "PUR_T_TOTALSTITLE"), ["Totals"]);
    assert!(text(&texts, "PUR_T_AIRFRAME").is_empty());
    assert_eq!(
        page.button().map(|b| (b.text.as_str(), b.colors)),
        Some((
            "Purchase Now",
            [0xFFFF_FFFF, 0xFF00_0000, 0xFF00_0000, 0xFF00_0000]
        ))
    );
    page.set_feed(feed(0)).expect("covered");
    let texts = page.texts();
    let at = |id: &str| {
        let text = texts.iter().find(|text| text.id == id).expect(id);
        (text.rect.x, text.rect.y, text.justify)
    };
    // LINE1 131, TEXTLEFTI 417, TEXTCENT 642, TEXTRIGHT 705.
    assert_eq!(at("PUR_T_AIRFRAME"), (417, 131, 0));
    assert_eq!(at("PUR_T_AIRFRAMEWEIGHT"), (642, 131, 2));
    assert_eq!(at("PUR_T_AIRFRAMECOST"), (705, 131, 2));
    assert_eq!(at("PUR_T_ENGINE").1, 165);
    assert_eq!(at("PUR_T_TOTALSWEIGHT"), (642, 430, 2));
    // The list rows stack from the list's line (LINE3 194, LINE4 274).
    let ys = |id: &str| -> Vec<i32> {
        texts
            .iter()
            .filter(|text| text.id == id)
            .map(|text| text.rect.y)
            .collect()
    };
    assert_eq!(ys("PUR_T_ARMORLIST").first(), Some(&194));
    assert_eq!(ys("PUR_T_GUNLIST").first(), Some(&274));
    // The problems line sits at LINE6, red.
    let problems = texts
        .iter()
        .find(|text| text.id == "PUR_T_PROBLEMS")
        .expect("cash 0 cannot buy");
    assert_eq!((problems.rect.x, problems.rect.y), (417, 388));
    assert_eq!(problems.color, 0xFFFF_0000);
}

#[test]
fn accept_parity5_cabin_plane_construction_purchase_e_the_press_checks_the_hangar_then_the_name() {
    let mut page = page();
    let value = i64::from(plane_value(&record()).expect("covered"));
    // No plane: nothing.
    assert_eq!(
        page.press_purchase("Ace", Some(0)),
        PurchaseOutcome::Nothing
    );
    // A plane the cash cannot pay: the button is inactive.
    page.set_feed(feed(value - 1)).expect("covered");
    assert_eq!(
        page.press_purchase("Ace", Some(0)),
        PurchaseOutcome::Nothing
    );
    page.set_feed(feed(value)).expect("covered");
    // `E 2210` first: no free record is message box 204, even with no name.
    assert_eq!(
        page.press_purchase("", None),
        PurchaseOutcome::HangarFull {
            text: Some("You have reached your hangar limit of planes.".into())
        }
    );
    // Then the name (`mail 20001`): empty is message box 203.
    assert_eq!(
        page.press_purchase("", Some(3)),
        PurchaseOutcome::NameRequired {
            text: Some("You must enter a name for your new plane.".into())
        }
    );
    assert_eq!(
        page.press_purchase("Ace", Some(3)),
        PurchaseOutcome::Commit {
            slot: 3,
            name: "Ace".into(),
            value: value as i32,
        }
    );
    assert_eq!(
        page.purchased().as_deref(),
        Some("This Short 1 has been purchased and delivered to your hangar.")
    );
}

/// `0x4111f0`: the first empty record, answered only while more than five
/// records are empty or of kind 2.
#[test]
fn accept_parity5_cabin_plane_construction_purchase_f_the_free_record_needs_more_than_five_spare() {
    assert_eq!(FREE_RESERVE, 5);
    let mut kinds: Vec<Option<u32>> = vec![Some(1); 25];
    assert_eq!(free_slot(&kinds), None, "nothing free");
    // Six spare records: the first empty one is the answer.
    for slot in [4, 9, 10, 20, 21, 24] {
        kinds[slot] = None;
    }
    assert_eq!(free_slot(&kinds), Some(4));
    // Five empty and a kind 2 record count the same; the kind 2 record is
    // never the answer.
    kinds[24] = Some(2);
    assert_eq!(free_slot(&kinds), Some(4));
    kinds[21] = Some(1);
    assert_eq!(free_slot(&kinds), None, "only five spare");
    // Kind 2 records only add to the count.
    kinds[4] = Some(2);
    kinds[9] = Some(2);
    kinds[10] = Some(2);
    kinds[20] = Some(2);
    assert_eq!(free_slot(&kinds), None, "no empty record to answer");
}

fn saved(kind: u32) -> Option<SavedPlane> {
    Some(SavedPlane {
        kind,
        name: "Old".into(),
        airframe: 0,
        value: 100,
    })
}

/// Ready opens the panel, a tab closes it, and the button runs the checks.
#[test]
fn accept_parity5_cabin_plane_construction_purchase_g_ready_opens_the_panel_in_the_hub() {
    let mut hub = hub();
    assert!(hub.purchase().is_some());
    assert!(!hub.purchase_open());
    hub.open("");
    assert_eq!(hub.latched(), Some("PX_B_AIRFRAME"));
    // Ready is the panel, not the marked notice.
    assert_eq!(hub.press("PX_B_READY"), HubOutcome::Purchase);
    assert!(hub.purchase_open());
    assert_eq!(hub.panel_section(), Some(PURCHASE_SECTION));
    // No tab is held while the panel is up (`case 10017` releases Ready).
    assert_eq!(hub.latched(), None);
    assert!(hub.texts().iter().any(|text| text.id == "PUR_T_TITLE"));
    // A tab brings its panel back.
    assert_eq!(
        hub.press("PX_B_ENGINE"),
        HubOutcome::Panel(cs_app::ui::front_end::HubPanel::Engine)
    );
    assert!(!hub.purchase_open());
    assert_eq!(hub.latched(), Some("PX_B_ENGINE"));
    assert!(!hub.texts().iter().any(|text| text.id == "PUR_T_TITLE"));

    // The button: inactive without a plane.
    assert_eq!(hub.press("PX_B_READY"), HubOutcome::Purchase);
    assert!(!hub.purchase_enabled());
    assert_eq!(hub.press(PURCHASE_BUTTON), HubOutcome::Nothing);
    let value = i64::from(plane_value(&record()).expect("covered"));
    assert_eq!(hub.set_purchase_feed(feed(value)), Ok(true));
    assert!(hub.purchase_enabled());
    let spare: Vec<Option<SavedPlane>> = (0..25)
        .map(|n| (n < 18).then(|| saved(1)).flatten())
        .collect();
    hub.set_saved_planes(spare, 0, value as u64);
    assert!(hub.hangar().is_some(), "hangar rows read");
    // The name box is empty: message box 203, the screen stays.
    assert_eq!(hub.press(PURCHASE_BUTTON), HubOutcome::Message);
    assert_eq!(
        hub.message(),
        Some("You must enter a name for your new plane.")
    );
    // Nothing else is read while the box is up.
    hub.type_text("x");
    assert_eq!(hub.text(), "");
    assert_eq!(hub.press("PX_B_ENGINE"), HubOutcome::Nothing);
    assert_eq!(hub.dismiss_message(), HubOutcome::Stay);
    assert!(hub.message().is_none());
    // A name: the commit is the caller's, to the first empty record.
    hub.type_text("Ace");
    assert_eq!(
        hub.press(PURCHASE_BUTTON),
        HubOutcome::Commit {
            slot: 18,
            name: "Ace".into(),
            value: value as i32,
        }
    );
    // The box of the finished purchase leaves the screen when closed.
    assert_eq!(hub.finish_purchase(), HubOutcome::Message);
    assert_eq!(
        hub.message(),
        Some("This Short 1 has been purchased and delivered to your hangar.")
    );
    assert_eq!(hub.dismiss_message(), HubOutcome::Leave);
}

#[test]
fn accept_parity5_cabin_plane_construction_purchase_h_a_full_hangar_refuses_with_box_204() {
    let mut hub = hub();
    hub.open("Ace");
    assert_eq!(hub.press("PX_B_READY"), HubOutcome::Purchase);
    let value = i64::from(plane_value(&record()).expect("covered"));
    assert_eq!(hub.set_purchase_feed(feed(value)), Ok(true));
    // 21 saved planes leave four records spare: not more than five.
    let full: Vec<Option<SavedPlane>> = (0..25)
        .map(|n| (n < 21).then(|| saved(1)).flatten())
        .collect();
    hub.set_saved_planes(full, 0, value as u64);
    assert_eq!(hub.press(PURCHASE_BUTTON), HubOutcome::Message);
    assert_eq!(
        hub.message(),
        Some("You have reached your hangar limit of planes.")
    );
    assert_eq!(hub.dismiss_message(), HubOutcome::Stay);
}

/// A layout without the rows leaves Ready on the marked notice.
#[test]
fn accept_parity5_cabin_plane_construction_purchase_i_a_layout_without_the_rows_keeps_the_notice() {
    let bare = OriginalLayout::parse(LAYOUT).expect("the hub fixture parses");
    let mut hub = HubPage::from_layout(&bare, &names, &ids).expect("rows");
    assert!(hub.purchase().is_none());
    assert_eq!(hub.press("PX_B_READY"), HubOutcome::Ready);
    assert_eq!(READY_NOT_BUILT.button, "PX_B_READY");
    assert_eq!(hub.set_purchase_feed(feed(1)), Ok(false));
    assert!(PurchasePage::from_layout(&bare, &names, &ids).is_err());
}

/// The purchase is the production screen's commit: the cash leaves the
/// profile, a second press of the same draft is an empty draft.
#[test]
fn accept_parity5_cabin_plane_construction_purchase_j_the_commit_is_the_construction_screens() {
    let mut draft = draft();
    let ctx = ConstructionContext {
        rules: &draft.rules,
        policy: &draft.policy,
        book: &draft.book,
        graph: &draft.graph,
    };
    let mut screen = ConstructionScreen::open(&draft.state, declared_synthetic_blueprint(), vec![]);
    let before = draft.state.currency();
    let receipt = commit_purchase(&mut screen, &ctx, &mut draft.state).expect("the draft commits");
    assert_eq!(draft.state.currency(), before - receipt.charged);
    assert!(receipt.charged > 0);
    assert_eq!(screen.committed(), Some(&receipt));
    assert!(commit_purchase(&mut screen, &ctx, &mut draft.state).is_err());
}

// ---------------------------------------------------------- the retail half ---

/// The installation's rows and strings build the page, and a plane's lines
/// read in the installation's own words.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity5_cabin_plane_construction_purchase_retail_the_installations_rows_and_strings_build_the_page()
 {
    use cs_app::ui::front_end::FrontEndScreens;
    let Ok(game_dir) = std::env::var("CS_GAME_DIR") else {
        panic!("CS_GAME_DIR is unset: this run needs the original installation");
    };
    let root = std::path::PathBuf::from(game_dir);
    let screens = FrontEndScreens::open(&root).expect("the installation's screens open");
    let layout = screens.original_layout().expect("the real layout reads");
    let strings = cs_app::mission_session::FlightCheckStrings::read(&root, &screens)
        .expect("the string table reads");
    let mut page =
        PurchasePage::from_layout(&layout, &|n: &str| strings.resolve_tagged(n), &|id: u32| {
            strings.resolve_id_tagged(id)
        })
        .expect("the real Purchase rows read");
    let fixed = page.texts();
    assert_eq!(text(&fixed, "PUR_T_ITEMTITLE"), ["Item"]);
    assert_eq!(text(&fixed, "PUR_T_WEIGHTTITLE"), ["Weight"]);
    assert_eq!(text(&fixed, "PUR_T_COSTTITLE"), ["Cost"]);
    assert_eq!(text(&fixed, "PUR_T_TOTALSTITLE"), ["Totals"]);
    assert!(!text(&fixed, "PUR_T_TITLE").is_empty());
    assert_eq!(page.button().map(|b| b.text.as_str()), Some("Purchase Now"));
    // The real strings the rows are built from.
    for id in [
        203, 204, 506, 703, 1171, 1176, 1177, 1178, 1182, 1183, 1191, 1192, 1193, 1194, 1226, 1227,
        3000, 3010, 3020, 3030, 3100, 3165, 3310, 3314,
    ] {
        assert!(
            strings
                .resolve_id_tagged(id)
                .is_some_and(|raw| !raw.is_empty()),
            "string {id} is in the table"
        );
    }
    let value = i64::from(plane_value(&record()).expect("covered"));
    page.set_feed(feed(value)).expect("covered");
    let line = page.airframe_line().expect("airframe 1 has a name");
    assert!(!line[0].is_empty());
    assert_eq!((line[1].as_str(), line[2].as_str()), ("3255", "4548"));
    assert_eq!(page.armor_lines()[0][0], "Nose: 10 units");
    assert_eq!(
        page.gun_lines()[0][0].chars().take(4).collect::<String>(),
        "(2) "
    );
    assert_eq!(
        page.totals_line().map(|[_, cost]| cost),
        Some(format!("${value}"))
    );
    assert_eq!(page.problems_text(), "");
    page.set_feed(feed(0)).expect("covered");
    assert!(
        page.problems_text()
            .starts_with("CAN'T PURCHASE: INSUFFICIENT FUNDS")
    );
    let hub = HubPage::from_layout(&layout, &|n: &str| strings.resolve_tagged(n), &|id: u32| {
        strings.resolve_id_tagged(id)
    })
    .expect("the real hub rows read");
    assert!(hub.purchase().is_some(), "Ready has its panel");
}
