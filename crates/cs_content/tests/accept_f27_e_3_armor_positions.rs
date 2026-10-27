//! Acceptance tests for task F27-E.3 (#550): the original's armor position
//! vocabulary, measured from `ARMOR.SCRIPT`, `LAYOUT.CSV` and `RESOURCE.H`, is
//! queryable through `cs_content::damage` and resolves nothing about gun groups.
//!
//! The fast tests drive the production measurement over synthetic members built
//! here; the `#[ignore]`d one re-measures the retail installation with its own
//! independent scan, so a stale constant fails instead of passing.

use cs_content::damage::{
    ArmorMember, ArmorPurchaseColumn, ArmorVocabularyError, ORIGINAL_ARMOR_SCRIPT,
    ORIGINAL_LAYOUT_CSV, OriginalArmorPosition, measure_original_armor_vocabulary,
    read_original_armor_vocabulary,
};
use cs_content::weapons::{ORIGINAL_RESOURCE_HEADER, OriginalLimitClaim};
use cs_types::asset_id::SourceSpan;
use cs_types::content::Resolved;
use cs_types::evidence::{ClaimStatus, ContentHash};

use crate::f27_e_1_support as support;

fn member(name: &str, text: &str) -> ArmorMember {
    let sha = cs_assets::install::sha256(text.as_bytes());
    ArmorMember {
        bytes: text.as_bytes().to_vec(),
        span: SourceSpan::new(
            ContentHash::from_bytes([7; 32]),
            "fixture.rof",
            Some(name),
            0,
            text.len() as u64,
            Some(sha),
        )
        .expect("a valid span"),
    }
}

fn script(count: usize, titles: [&str; 4]) -> String {
    let mut text = String::from("main\r\n{\r\ngui_create\r\n{\r\nobject Q[3]\r\n");
    for title in titles {
        text.push_str(&format!(
            "T = @ctl@PE\r\nT.YC = \"{title}\"\r\ninitialize(T)\r\n"
        ));
    }
    text.push_str(&format!(
        "object X[{count}]\r\nfor (R = 0; R < {count}; R++)\r\n{{\r\n"
    ));
    text.push_str(
        "X[R].YC = \"ar_d_point\" conv$(R)\r\ncallback($$E$$, 2247, 0, (R), P[R])\r\n}\r\n",
    );
    text.push_str("callback($$E$$, 2220, 3, Y.BC)\r\nY.YC = \"ar_t_armorname\"\r\n");
    text.push_str("Z.YC = \"ar_s_armordesc\"\r\ncallback($$E$$, 2225, Z.BC)\r\n}\r\n}\r\n");
    text
}

const TITLES: [&str; 4] = [
    "ar_t_nosetitle",
    "ar_t_tailtitle",
    "ar_t_lefttitle",
    "ar_t_righttitle",
];

fn layout(point_ys: [i32; 4], nose_macro: &str, drop_purchase: bool) -> String {
    let mut text = String::from("; fixture\r\n[@Armor@]\r\n");
    for (key, name, y) in [
        ("AR_T_NOSETITLE", nose_macro, 110),
        ("AR_T_TAILTITLE", "IDS_AR_TAIL_TITLE", 159),
        ("AR_T_LEFTTITLE", "IDS_AR_LEFT_TITLE", 205),
        ("AR_T_RIGHTTITLE", "IDS_AR_RIGHT_TITLE", 252),
    ] {
        text.push_str(&format!("{key}\t=T,{name},<GX>,{y},0,0,0,0xFF000000,0\r\n"));
    }
    for (slot, y) in point_ys.iter().enumerate() {
        text.push_str(&format!(
            "AR_D_POINT{slot}  =D,<S>,<U>,<D>,<DU>,<DD>,<V2>,{y},{},<W>,<H>,13\r\n",
            100 - slot * 10
        ));
    }
    text.push_str("[@Purchase@]\r\n");
    text.push_str("PUR_T_ARMORLIST     =A,<L>,<LINE3>,0,220,<H>,<C>,0,1\r\n");
    text.push_str("PUR_T_ARMORWEIGHT   =A,<L>,<LINE3>,0,<W>,<H>,<C>,2,1\r\n");
    if !drop_purchase {
        text.push_str("PUR_T_ARMORCOST     =A,<L>,<LINE3>,0,<W>,<H>,<C>,2,1\r\n");
    }
    text
}

const HEADER: &str = "#define IDS_AR_NOSE_TITLE 1044\r\n#define IDS_AR_TAIL_TITLE 1045\r\n\
#define IDS_AR_LEFT_TITLE 1046\r\n#define IDS_AR_RIGHT_TITLE 1047\r\n\
#define IDS_PX_ARMORUNITS 1170\r\n";

fn measure(
    script_text: &str,
    layout_text: &str,
) -> Result<cs_content::damage::OriginalArmorVocabulary, ArmorVocabularyError> {
    measure_original_armor_vocabulary(
        &member(ORIGINAL_ARMOR_SCRIPT, script_text),
        &member(ORIGINAL_LAYOUT_CSV, layout_text),
        &member(ORIGINAL_RESOURCE_HEADER, HEADER),
    )
}

fn good_layout() -> String {
    layout([133, 180, 228, 275], "IDS_AR_NOSE_TITLE", false)
}

#[test]
fn accept_f27_e_3_measures_four_positions_and_four_points() {
    let vocabulary = measure(&script(4, TITLES), &good_layout()).expect("measures");
    let order: Vec<_> = vocabulary
        .positions()
        .iter()
        .map(|r| r.position())
        .collect();
    assert_eq!(order, OriginalArmorPosition::ALL);
    assert_eq!(vocabulary.point_count(), 4);
    let tail = vocabulary
        .position(OriginalArmorPosition::Tail)
        .expect("tail");
    assert_eq!(tail.point_slot(), 1);
    assert_eq!(tail.point_control_key(), "ar_d_point1");
    assert_eq!(tail.title_string_id(), 1045);
    assert_eq!((tail.title_row_y(), tail.point_row_y()), (159, 180));
    assert_eq!(vocabulary.units_string_id(), 1170);
    assert_eq!(vocabulary.point_callback().id(), 2247);
    assert_eq!(vocabulary.name_callback().id(), 2220);
    assert_eq!(vocabulary.description_callback().id(), 2225);
    let columns: Vec<_> = vocabulary
        .purchase_columns()
        .iter()
        .map(|c| c.column())
        .collect();
    assert_eq!(columns, ArmorPurchaseColumn::ALL);
}

#[test]
fn accept_f27_e_3_facts_carry_observed_spans_and_decoded_lines() {
    let script_text = script(4, TITLES);
    let vocabulary = measure(&script_text, &good_layout()).expect("measures");
    let nose = &vocabulary.positions()[0];
    let line = nose.script_title();
    assert_eq!(line.provenance().class, ClaimStatus::ObservedTool);
    let span = line.provenance().source.as_ref().expect("a span");
    assert_eq!(span.member_key(), Some(ORIGINAL_ARMOR_SCRIPT));
    let start = line.decoded_offset() as usize;
    let end = start + line.decoded_len() as usize;
    assert!(script_text[start..end].contains("ar_t_nosetitle"));
}

#[test]
fn accept_f27_e_3_refuses_what_the_members_do_not_state() {
    let mut swapped = TITLES;
    swapped.swap(2, 3);
    assert!(measure(&script(4, swapped), &good_layout()).is_err());
    assert!(measure(&script(3, TITLES), &good_layout()).is_err());
    assert!(
        measure(
            &script(4, TITLES),
            &layout([133, 180, 228, 275], "IDS_AR_TAIL_TITLE", false)
        )
        .is_err()
    );
    // A dropdown above its own title, and one past the next title.
    assert!(
        measure(
            &script(4, TITLES),
            &layout([100, 180, 228, 275], "IDS_AR_NOSE_TITLE", false)
        )
        .is_err()
    );
    assert!(
        measure(
            &script(4, TITLES),
            &layout([160, 180, 228, 275], "IDS_AR_NOSE_TITLE", false)
        )
        .is_err()
    );
    assert!(
        measure(
            &script(4, TITLES),
            &layout([133, 180, 228, 275], "IDS_AR_NOSE_TITLE", true)
        )
        .is_err()
    );
}

#[test]
fn accept_f27_e_3_gun_groups_stay_unknown_for_every_position() {
    let vocabulary = measure(&script(4, TITLES), &good_layout()).expect("measures");
    for position in OriginalArmorPosition::ALL {
        match vocabulary.gun_group_for(position) {
            Resolved::Unknown { claim_id, .. } => {
                assert_eq!(claim_id.as_str(), "f27.d.limit.gun_group_assignment");
            }
            Resolved::Known(_) => panic!("{position} must not resolve a gun group"),
        }
    }
    assert!(
        OriginalLimitClaim::GunGroupAssignment
            .measured_lead()
            .is_some()
    );
    assert!(OriginalLimitClaim::Convergence.measured_lead().is_none());
}

/// Counts non-overlapping occurrences of `needle` in `haystack`, ignoring case.
fn count_ci(haystack: &str, needle: &str) -> usize {
    haystack
        .to_ascii_lowercase()
        .matches(&needle.to_ascii_lowercase())
        .count()
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f27_e_3_retail_armor_vocabulary_is_four_positions_four_points() {
    let root = support::game_dir();
    let vocabulary = read_original_armor_vocabulary(&root).expect("the retail members measure");

    // Independent scan of the raw member text, not through the production parser.
    let raw = support::read_member(&root, ORIGINAL_ARMOR_SCRIPT);
    let text = String::from_utf8(raw).expect("text");
    assert_eq!(
        count_ci(&text, "title\""),
        5,
        "four positions plus ar_t_title"
    );
    assert_eq!(count_ci(&text, "object X[4]"), 1);
    assert_eq!(count_ci(&text, "\"ar_d_point\" conv$(R)"), 1);
    assert_eq!(vocabulary.point_count(), 4);
    assert_eq!(vocabulary.positions().len(), 4);
    assert_eq!(vocabulary.purchase_columns().len(), 3);
    for record in vocabulary.positions() {
        assert!(record.point_row_y() > record.title_row_y());
        let span = record
            .script_title()
            .provenance()
            .source
            .as_ref()
            .expect("span");
        assert_eq!(
            span.member_key().map(str::to_ascii_uppercase).as_deref(),
            Some(ORIGINAL_ARMOR_SCRIPT)
        );
    }
}
