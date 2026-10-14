//! Acceptance scenario task #476 (content half): in the mode catalog, every
//! per-mode rule the owner measured by static analysis of the original
//! executable replaces its `UnknownRule` with a `MeasuredRule` (policy,
//! citation, analyzed image; class `observed_tool`, never `verified_original`),
//! and every rule nothing measured answers stays an explicit unknown with its
//! reason (`specs/F56-original-multiplayer-scenarios-and-mode-rules.md`,
//! non-negotiable behavior 1). The tests drive the production
//! `cs_content::multiplayer::discover_modes`; the fixture rows are authored
//! strings, and the measured table is pinned to the rules crate's own
//! measurement through the `cs_net` dev-dependency.

use cs_content::config::StringRow;
use cs_content::multiplayer::{
    BRIEFING_FIRST_ID, BRIEFING_STRIDE, EVIDENCE_IMAGE_SHA256, MEASURED_RULE_POLICIES,
    RESIDUAL_UNKNOWN_RULES, RULE_LABELS, discover_modes,
};
use cs_types::asset_id::SourceSpan;
use cs_types::content::Resolved;
use cs_types::evidence::{ClaimStatus, ContentHash};

const LANGUAGE: u32 = 1033;

fn hash(byte: u8) -> ContentHash {
    ContentHash::from_bytes([byte; 32])
}

fn span() -> SourceSpan {
    SourceSpan::new(hash(1), "strings.dll", None, 0, 100, None).expect("a valid span")
}

fn row(id: u32, text: &str) -> StringRow {
    StringRow {
        id,
        language: LANGUAGE,
        code_page: 1252,
        code_units: text.encode_utf16().collect(),
        text: Some(text.to_owned()),
        span: span(),
    }
}

/// The four mode names and four briefing blocks, shaped like the retail table
/// (authored fixture strings, not original text), followed by another family.
fn fixture_rows() -> Vec<StringRow> {
    let mut rows = vec![
        row(7010, "score"),
        row(7011, "Deathmatch without Teams"),
        row(7012, "Deathmatch with Teams"),
        row(7013, "Capture the Flag"),
        row(7014, "Zeppelin vs. Zeppelin"),
        row(7015, "TCP/IP"),
    ];
    let blocks: [(&str, &[&str], &[&str]); 4] = [
        (
            "CAPTURE THE FLAG",
            &["take it", "bring it back", "your team scores"],
            &["10", "8", "2", "-2"],
        ),
        (
            "ZEPPELIN vs. ZEPPELIN",
            &["sink theirs", "protect your team"],
            &["10", "2", "-2"],
        ),
        ("DEATHMATCH", &["shoot them", "do not crash"], &["2", "-2"]),
        (
            "TEAM DEATHMATCH",
            &["shoot them", "not your team"],
            &["2", "-2"],
        ),
    ];
    for (index, (title, instructions, points)) in blocks.iter().enumerate() {
        let base = BRIEFING_FIRST_ID + index as u32 * BRIEFING_STRIDE;
        rows.push(row(base, title));
        rows.push(row(base + 1, "a tagline"));
        rows.push(row(base + 2, "POINTS"));
        rows.push(row(base + 3, "INSTRUCTIONS"));
        let mut next = base + 4;
        for text in *instructions {
            rows.push(row(next, text));
            next += 1;
        }
        for text in *points {
            rows.push(row(next, text));
            next += 1;
        }
    }
    let other = BRIEFING_FIRST_ID + 4 * BRIEFING_STRIDE;
    rows.push(row(other, "INSTANT ACTION"));
    rows.push(row(other + 1, "DOGFIGHT AN ACE."));
    rows.push(row(other + 2, "a different family"));
    rows
}

fn fixture_catalog() -> cs_content::multiplayer::ModeCatalog {
    discover_modes(&fixture_rows(), LANGUAGE).expect("the fixture table reads")
}

#[test]
fn accept_f476_measured_rules_replace_their_unknowns_for_every_mode() {
    let catalog = fixture_catalog();
    assert_eq!(catalog.modes.len(), 4);
    let expected: Vec<(&str, &str, &str)> = MEASURED_RULE_POLICIES
        .iter()
        .map(|(field, policy, citation)| (*field, *policy, *citation))
        .collect();
    for mode in &catalog.modes {
        let name_id = mode
            .id
            .as_str()
            .rsplit('-')
            .next()
            .expect("mode.name-<id>")
            .to_owned();
        let measured: Vec<(&str, &str, &str, ClaimStatus, &str)> = mode
            .measured_rules
            .iter()
            .map(|rule| {
                assert_eq!(
                    rule.claim.as_str(),
                    format!("f56.mode.name-{name_id}.{}", rule.field),
                    "the measured rule keeps the claim its unknown carried"
                );
                (
                    rule.field,
                    rule.policy,
                    rule.citation,
                    rule.class,
                    rule.image_sha256,
                )
            })
            .collect();
        for (field, policy, citation, class, image) in measured {
            assert_eq!(
                (field, policy, citation),
                expected
                    .iter()
                    .find(|(f, ..)| *f == field)
                    .copied()
                    .unwrap_or_else(|| panic!("{field} is not a measured field")),
                "{} {field}",
                mode.name.text
            );
            assert_eq!(
                class,
                ClaimStatus::ObservedTool,
                "{field}: static code evidence, never verified_original"
            );
            assert_eq!(image, EVIDENCE_IMAGE_SHA256);
        }
        let fields: Vec<&str> = mode.measured_rules.iter().map(|r| r.field).collect();
        let want: Vec<&str> = MEASURED_RULE_POLICIES.iter().map(|(f, ..)| *f).collect();
        assert_eq!(fields, want, "{}", mode.name.text);
    }
}

#[test]
fn accept_f476_residual_unknown_rules_stay_unknown_with_their_reasons() {
    let catalog = fixture_catalog();
    let want: Vec<&str> = RESIDUAL_UNKNOWN_RULES
        .iter()
        .map(|(label, _)| *label)
        .collect();
    for mode in &catalog.modes {
        let labels: Vec<&str> = mode.unknown_rules.iter().map(|rule| rule.field).collect();
        assert_eq!(
            labels, want,
            "{}: only the unmeasured fields stay unknown",
            mode.name.text
        );
        for rule in &mode.unknown_rules {
            assert!(
                !rule.reason.trim().is_empty(),
                "{} {} must say what is unknown",
                mode.name.text,
                rule.field
            );
            assert!(
                rule.reason.contains("task #476"),
                "{} {}: {}",
                mode.name.text,
                rule.field,
                rule.reason
            );
            let (_, want_reason) = RESIDUAL_UNKNOWN_RULES
                .iter()
                .find(|(label, _)| *label == rule.field)
                .expect("a residual label");
            assert_eq!(&rule.reason, want_reason);
        }
    }
}

#[test]
fn accept_f476_a_mode_whose_name_states_no_team_play_keeps_teams_unknown() {
    // A paired mode whose name and briefing say nothing about teams: the
    // teams rule stays an explicit unknown beside the residual ones, with its
    // own claim; the measured rules still replace the fields static analysis
    // answered.
    let mut rows = fixture_rows();
    for text in ["Deathmatch with Teams", "TEAM DEATHMATCH", "not your team"] {
        for candidate in rows.iter_mut() {
            if candidate.text.as_deref() == Some(text) {
                candidate.text = Some(
                    match text {
                        "Deathmatch with Teams" => "Melee",
                        "TEAM DEATHMATCH" => "MELEE",
                        _ => "stay alive",
                    }
                    .to_owned(),
                );
                candidate.code_units = candidate
                    .text
                    .as_deref()
                    .expect("replaced")
                    .encode_utf16()
                    .collect();
            }
        }
    }
    let catalog = discover_modes(&rows, LANGUAGE).expect("the table reads");
    let melee = catalog
        .modes
        .iter()
        .find(|mode| mode.name.text == "Melee")
        .expect("the renamed mode paired");
    assert!(matches!(melee.team_play, Resolved::Unknown { .. }));
    let labels: Vec<&str> = melee.unknown_rules.iter().map(|rule| rule.field).collect();
    let mut want = vec!["teams"];
    want.extend(RESIDUAL_UNKNOWN_RULES.iter().map(|(label, _)| *label));
    assert_eq!(labels, want);
    assert!(
        melee.unknown_rules[0]
            .reason
            .contains("neither the name nor the briefing"),
        "{}",
        melee.unknown_rules[0].reason
    );
    assert_eq!(
        melee.measured_rules.len(),
        MEASURED_RULE_POLICIES.len(),
        "the measured rules do not depend on the fixture text"
    );
}

#[test]
fn accept_f476_the_catalog_labels_mirror_the_rules_crate_measurement() {
    // The catalog may not depend on `cs_net`, so the mirror is labels and
    // citations; this dev-dependency test pins the two measurements together.
    assert_eq!(
        EVIDENCE_IMAGE_SHA256,
        cs_net::rules::measured::EVIDENCE_IMAGE_SHA256,
        "both crates cite the same analyzed image"
    );
    for (field, policy, citation) in MEASURED_RULE_POLICIES {
        let (_, addresses) = cs_net::rules::measured::POLICY_ADDRESSES
            .iter()
            .find(|(label, _)| *label == policy)
            .unwrap_or_else(|| panic!("{field} cites no rules-crate measurement"));
        assert_eq!(
            citation, *addresses,
            "{field}: the catalog and the rules crate cite the same addresses"
        );
        assert!(
            cs_net::rules::measured::MissionType::ALL
                .iter()
                .any(|mission_type| mission_type
                    .policies()
                    .iter()
                    .any(|(f, p)| *f == field && *p == policy)),
            "{field}={policy} is not a policy the rules crate fills"
        );
    }
    // The measured fields, the residual unknowns and (conditionally) teams
    // partition the rule vocabulary.
    let mut partition: Vec<&str> = MEASURED_RULE_POLICIES
        .iter()
        .map(|(field, _, _)| *field)
        .chain(RESIDUAL_UNKNOWN_RULES.iter().map(|(label, _)| *label))
        .chain(std::iter::once("teams"))
        .collect();
    partition.sort_unstable();
    let mut all: Vec<&str> = RULE_LABELS.into_iter().collect();
    all.sort_unstable();
    assert_eq!(partition, all);
}
