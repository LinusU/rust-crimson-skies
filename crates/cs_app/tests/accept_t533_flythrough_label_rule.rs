//! Task #533 acceptance tests: the fly-through selector's label rule, and the
//! campaign stunt records re-measured under it.
//!
//! Spec: `specs/F42-stunts-fame-photos-and-optional-achievement-events.md`,
//! stage `### F42-D`. Required capabilities: retail. Task test prefix:
//! `accept_t533_`.
//!
//! Task #463's selector required **both** halves of the measured label pair —
//! its `?` on `category_label` refused a record that carried only the help
//! label. Over the eight instant-action scenarios that rule and the union of
//! the two labels read the same 54 rows (every instant-action record carries
//! both), so #463 could not see the difference; over the whole installation
//! they disagree on exactly three campaign-mission records. #533 decides the
//! rule with evidence and this file pins it:
//!
//! * the chosen rule — **either** `category_label = MSG_OBJ_DZ` **or**
//!   `help_label = MSG_OBJ_FLYTHROUGH` selects a record — is asserted on the
//!   two authored shapes the task names: a record with **only** the help label
//!   (the shape of the three campaign records) and a record with **both**
//!   labels (the shape of every instant-action record). Inverting the rule to
//!   require the category label drops the first and fails the test.
//! * the retail re-measurement states the whole corpus under the production
//!   survey: 67 fly-through records by either label, of which the three named
//!   campaign readers carry a help-only record each, and the instant-action
//!   corpus is unchanged at 54 with both labels on every record.
//!
//! The unignored tests author every byte (the `.zrd` grammar and a reader
//! archive) and run the production decoder, the production discovery and the
//! production survey, so the rule is falsifiable in CI. The `#[ignore]`d test
//! reads the owner's installation through the same production entry points.
//!
//! Nothing here is `verified_original`: no original run happened, and reading
//! the installation's files is not evidence of how the game behaves.

use std::fs;
use std::path::{Path, PathBuf};

use cs_app::stunts::survey_retail_stunt_authority;
use cs_content::stunts::{
    FLY_THROUGH_CATEGORY_LABEL, FLY_THROUGH_HELP_LABEL, SCENARIO_MEMBER, SCENARIO_TARGETS_MEMBER,
    decode_zrd, is_fly_through_labelled, scenario_fly_through_targets,
};

// ------------------------------------------------------------- .zrd writer ---

/// A `.zrd` text node: tag `3`, the byte length and the bytes.
fn zrd_text(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8 + text.len());
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

/// A `.zrd` list node: tag `4`, then **`children.len() + 1`** as the count, then
/// the children (the measured `count - 1` grammar, F09/#463).
fn zrd_list(children: Vec<Vec<u8>>) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8 + children.iter().map(Vec::len).sum::<usize>());
    bytes.extend_from_slice(&4_u32.to_le_bytes());
    bytes.extend_from_slice(&((children.len() as u32) + 1).to_le_bytes());
    for child in children {
        bytes.extend_from_slice(&child);
    }
    bytes
}

/// One authored `targets.zrd` objective from explicit `[key, value]` pairs, so
/// a record can **omit** a key entirely — which is what makes the three
/// campaign records help-only.
fn target_pairs(pairs: Vec<(&str, Vec<u8>)>) -> Vec<u8> {
    zrd_list(
        pairs
            .into_iter()
            .map(|(key, value)| zrd_list(vec![zrd_text(key), value]))
            .collect(),
    )
}

/// A one-element list of a text node, the shape the original writes a scalar in
/// (`nodes`, `mission_type`, …).
fn zrd_text1(text: &str) -> Vec<u8> {
    zrd_list(vec![zrd_text(text)])
}

// ------------------------------------------------------- reader-archive writer ---

/// A version-one reader archive holding `members` in order, as task #463's test
/// authored it.
fn reader_archive(members: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut entries = Vec::with_capacity(members.len());
    for (name, member) in members {
        let start = bytes.len() as u32;
        bytes.extend_from_slice(member);
        entries.push((start, member.len() as u32, *name));
    }
    for (start, length, name) in &entries {
        bytes.extend_from_slice(&start.to_le_bytes());
        bytes.extend_from_slice(&length.to_le_bytes());
        let name = name.as_bytes();
        assert!(name.len() < 64, "a fixture member name fits its field");
        let mut field = [0_u8; 64];
        field[..name.len()].copy_from_slice(name);
        bytes.extend_from_slice(&field);
        bytes.extend_from_slice(&[0_u8; 76]);
    }
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&(members.len() as u32).to_le_bytes());
    bytes
}

/// A throwaway installation tree.
struct TempInstallation {
    root: PathBuf,
}

impl TempInstallation {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the system clock is after the Unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "crimson-t533-{label}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("the fixture root is created");
        Self { root }
    }

    fn write(&self, spelling: &str, bytes: &[u8]) {
        let path = self.root.join(spelling);
        fs::create_dir_all(path.parent().expect("a fixture spelling has a parent"))
            .expect("the fixture directories are created");
        fs::write(&path, bytes).expect("the fixture bytes are written");
    }
}

impl Drop for TempInstallation {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

// --------------------------------------------- the rule, on authored bytes ---

/// The chosen label rule, on the two authored shapes the task names.
///
/// A record with **only** the help label is a fly-through target; a record with
/// **both** labels is a fly-through target; a record with **neither** is not.
/// Inverting the rule — going back to #463's requirement of a `category_label`
/// — drops the first record and fails this test.
#[test]
fn accept_t533_flythrough_label_rule_either_label_selects_a_record() {
    // Help-only: description, nodes and `help_label`, and **no**
    // `category_label` key at all — the shape of `ZBD/C1/M02`'s
    // `MSG_OBJ_ZEPHANGER`, `ZBD/C4/M03`'s `MSG_TRGT_DEVILSHORN` and
    // `ZBD/C5/M02`'s `MSG_TRGT_PHQ`.
    let help_only = target_pairs(vec![
        ("description", zrd_text("MSG_OBJ_ZEPHANGER")),
        ("nodes", zrd_text1("h3_marker")),
        ("help_label", zrd_text(FLY_THROUGH_HELP_LABEL)),
    ]);
    // Both labels: the shape of every instant-action fly-through record.
    let both = target_pairs(vec![
        ("description", zrd_text("MSG_OBJ_DESCRIPTION_1")),
        ("nodes", zrd_text1("dz1")),
        ("category_label", zrd_text(FLY_THROUGH_CATEGORY_LABEL)),
        ("help_label", zrd_text(FLY_THROUGH_HELP_LABEL)),
    ]);
    // Neither label: an ordinary objective, never a gate.
    let neither = target_pairs(vec![
        ("description", zrd_text("MSG_TRGT_FLAGBASE")),
        ("nodes", zrd_text1("ctf_1")),
        ("objective", zrd_list(vec![])),
        ("help_label", zrd_text("MSG_OBJ_TEAM_1")),
    ]);
    let targets = decode_zrd(&zrd_list(vec![
        help_only.clone(),
        both.clone(),
        neither.clone(),
    ]))
    .expect("the authored targets decode");

    // The label rule itself, record by record: `is_fly_through_labelled` is the
    // predicate `scenario_fly_through_targets` applies.
    let records = targets.as_list().expect("the root is a list");
    assert!(
        is_fly_through_labelled(&records[0]),
        "a record carrying only the fly-through help label is labelled a fly-through target"
    );
    assert!(
        is_fly_through_labelled(&records[1]),
        "a record carrying both labels is labelled a fly-through target"
    );
    assert!(
        !is_fly_through_labelled(&records[2]),
        "a record carrying neither label is not"
    );

    // The selector, which additionally requires the record to name a node.
    let selected = scenario_fly_through_targets(&targets);
    assert_eq!(
        selected.len(),
        2,
        "the help-only record and the both-label record are selected; the record with \
         neither label is not"
    );
    assert_eq!(selected[0].zone_label, "h3_marker");
    assert_eq!(
        selected[0].category_label, None,
        "the help-only record carries no category label, and the rule keeps it"
    );
    assert_eq!(
        selected[0].help_label.as_deref(),
        Some(FLY_THROUGH_HELP_LABEL)
    );
    assert_eq!(selected[0].description, "MSG_OBJ_ZEPHANGER");
    assert_eq!(selected[1].zone_label, "dz1");
    assert_eq!(
        selected[1].category_label.as_deref(),
        Some(FLY_THROUGH_CATEGORY_LABEL),
        "the both-label record keeps both halves"
    );
    assert_eq!(
        selected[1].help_label.as_deref(),
        Some(FLY_THROUGH_HELP_LABEL)
    );

    // A labelled record that names **no** node is still labelled, but the
    // selector skips it rather than inventing an empty zone name — so the two
    // readings differ exactly there, and nowhere else.
    let nodeless = target_pairs(vec![
        ("description", zrd_text("MSG_TRGT_NODELESS")),
        ("help_label", zrd_text(FLY_THROUGH_HELP_LABEL)),
    ]);
    let targets = decode_zrd(&zrd_list(vec![nodeless])).expect("the nodeless record decodes");
    assert_eq!(
        scenario_fly_through_targets(&targets).len(),
        0,
        "a labelled record with no node is skipped by the selector"
    );
    assert_eq!(
        cs_content::stunts::fly_through_labelled_objectives(&targets),
        1,
        "…but counted by the label-only reading, which is why both are measured"
    );
}

// ------------------------------------------------- the survey over a file ----

/// The production survey counts a help-only record exactly like a both-label
/// record: the rule is the selector's own, not a parallel reimplementation.
#[test]
fn accept_t533_flythrough_label_rule_the_survey_counts_help_only_records() {
    let install = TempInstallation::new("surface");
    // A world group the discovery recognizes.
    install.write("ZBD/C5/gamez.zbd", b"synthetic world container");
    // A campaign-style reader: no `ia.zrd`, one help-only fly-through record.
    install.write(
        "ZBD/C5/M02/zrdr.zbd",
        &reader_archive(&[(
            SCENARIO_TARGETS_MEMBER,
            zrd_list(vec![target_pairs(vec![
                ("description", zrd_text("MSG_TRGT_PHQ")),
                ("nodes", zrd_text1("dz1")),
                ("help_label", zrd_text(FLY_THROUGH_HELP_LABEL)),
            ])]),
        )]),
    );
    // An instant-action-style reader: a both-label record and an ordinary
    // objective.
    install.write(
        "ZBD/C5/IA1/zrdr.zbd",
        &reader_archive(&[
            (SCENARIO_MEMBER, zrd_list(vec![])),
            (
                SCENARIO_TARGETS_MEMBER,
                zrd_list(vec![
                    target_pairs(vec![
                        ("description", zrd_text("MSG_TRGT_NYPD")),
                        ("nodes", zrd_text1("dz2")),
                        ("category_label", zrd_text(FLY_THROUGH_CATEGORY_LABEL)),
                        ("help_label", zrd_text(FLY_THROUGH_HELP_LABEL)),
                    ]),
                    target_pairs(vec![
                        ("description", zrd_text("MSG_TRGT_ZEP_ENEMY")),
                        ("nodes", zrd_text1("multiplayer1zep")),
                        ("category_label", zrd_text("MSG_OBJ_ZEPPELIN")),
                        ("help_label", zrd_text("MSG_OBJ_DISABLEENG")),
                    ]),
                ]),
            ),
        ]),
    );

    let survey = survey_retail_stunt_authority(&install.root)
        .unwrap_or_else(|error| panic!("the fixture installation surveys: {error}"));
    assert_eq!(
        survey.fly_through_objectives(),
        2,
        "the help-only campaign record and the both-label instant-action record"
    );
    assert_eq!(
        survey.fly_through_labelled_objectives(),
        2,
        "the label-only reading agrees over this corpus"
    );
    let campaign = survey
        .rows()
        .iter()
        .find(|row| row.container() == "zbd/c5/m02/zrdr.zbd")
        .expect("the campaign reader is a row");
    assert_eq!(
        campaign
            .objectives()
            .expect("it declares objectives")
            .fly_through(),
        1,
        "the campaign reader's single record carries only the help label and counts"
    );
}

// ------------------------------------------------------------------ retail ---

/// The retail root, or a loud failure. The tests that call this are `#[ignore]`d;
/// a test that skipped itself here would report a pass it never earned.
fn retail_root() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR is set for a retail test"))
}

/// The installation fingerprint of the owner's installation, recorded in
/// `docs/findings/2026-10-02-t465-ai-stunt-earning.md`.
const RETAIL_INSTALL_SHA256: &str =
    "b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978";

/// The whole corpus re-measured under the chosen rule, through the production
/// survey: 67 fly-through records, the three help-only campaign records named,
/// and the instant-action corpus untouched.
///
/// Measured over the owner's installation: **67** objective records carry
/// either measured label, and every one of them names a node, so the selector
/// and the label-only reading agree at 67. **64** carry both labels; the
/// **3** that carry only `help_label = MSG_OBJ_FLYTHROUGH` are campaign
/// missions — `ZBD/C1/M02` (`MSG_OBJ_ZEPHANGER` at `h3_marker`), `ZBD/C4/M03`
/// (`MSG_TRGT_DEVILSHORN` at `dz2`) and `ZBD/C5/M02` (`MSG_TRGT_PHQ` at `dz1`)
/// — each wired into its mission's objective machine like every both-label
/// record. The instant-action corpus is unchanged: **54** labelled records
/// across the six world groups that author any, all with both labels.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t533_flythrough_label_rule_retail_the_whole_corpus_under_the_chosen_rule() {
    let root = retail_root();
    let survey = survey_retail_stunt_authority(&root).expect("the retail corpus surveys");
    assert_eq!(
        survey.install_sha256(),
        RETAIL_INSTALL_SHA256,
        "the installation fingerprint the measurement was taken over"
    );
    assert_eq!(
        survey.fly_through_objectives(),
        67,
        "either label selects: the three help-only campaign records are included"
    );
    assert_eq!(
        survey.fly_through_labelled_objectives(),
        67,
        "the two readings agree: every labelled record names a node"
    );

    // The three help-only records, named: their readers each carry exactly one
    // labelled record, and the chosen rule counts it.
    for (container, spelling, description, node) in [
        (
            "zbd/c1/m02/zrdr.zbd",
            "ZBD/C1/M02/zrdr.zbd",
            "MSG_OBJ_ZEPHANGER",
            "h3_marker",
        ),
        (
            "zbd/c4/m03/zrdr.zbd",
            "ZBD/C4/M03/zrdr.zbd",
            "MSG_TRGT_DEVILSHORN",
            "dz2",
        ),
        (
            "zbd/c5/m02/zrdr.zbd",
            "ZBD/C5/M02/zrdr.zbd",
            "MSG_TRGT_PHQ",
            "dz1",
        ),
    ] {
        let row = survey
            .rows()
            .iter()
            .find(|row| row.container() == container)
            .unwrap_or_else(|| panic!("{container} is a measured row"));
        let corpus = row.objectives().expect("the reader declares objectives");
        assert_eq!(
            (corpus.fly_through(), corpus.fly_through_labelled()),
            (1, 1),
            "{container}'s single labelled record is help-only and the rule counts it"
        );
        // The record itself, through the production decoder and selector.
        let targets = decode_reader_targets(&root, spelling);
        let selected = scenario_fly_through_targets(&targets);
        assert_eq!(selected.len(), 1, "{container} selects exactly one gate");
        assert_eq!(selected[0].description, description);
        assert_eq!(selected[0].zone_label, node);
        assert_eq!(
            selected[0].category_label, None,
            "{container}'s record carries no category label"
        );
        assert_eq!(
            selected[0].help_label.as_deref(),
            Some(FLY_THROUGH_HELP_LABEL)
        );
    }

    // The instant-action corpus is untouched by the rule change: every one of
    // its labelled records carries both labels, so the stricter rule read the
    // same rows. A row has a scenario descriptor exactly when its reader is an
    // instant-action scenario (`ia.zrd`), which is 8 readers over retail.
    let mut instant_action = 0_u32;
    let mut instant_action_both = 0_u32;
    let mut instant_action_readers = 0_u32;
    for row in survey.rows() {
        if row.scenario().is_none() {
            continue;
        }
        instant_action_readers += 1;
        let corpus = row.objectives().expect("a scenario declares objectives");
        instant_action += corpus.fly_through();
        instant_action_both += corpus.fly_through_labelled();
    }
    assert_eq!(
        instant_action_readers, 8,
        "the eight instant-action scenario readers"
    );
    assert_eq!(
        (instant_action, instant_action_both),
        (54, 54),
        "the eight instant-action scenarios author 54 labelled records, all with both \
         labels — the rule change moves nothing there"
    );
}

/// One reader archive's `targets.zrd` decoded through the production container
/// discovery and `.zrd` decoder — the same path the survey takes.
fn decode_reader_targets(game_dir: &Path, spelling: &str) -> cs_content::stunts::ZrdValue {
    let path = game_dir.join(spelling);
    let bytes = fs::read(&path).unwrap_or_else(|error| panic!("{} reads: {error}", path.display()));
    let relative =
        cs_types::install::RelativePath::new(spelling).expect("a manifest path is a relative path");
    let discovery = cs_formats::script_raw::discover_container(spelling, &relative, &bytes);
    let targets = discovery
        .programs()
        .iter()
        .find(|program| program.locator().member() == Some(SCENARIO_TARGETS_MEMBER))
        .unwrap_or_else(|| panic!("{spelling} carries a targets member"));
    decode_zrd(targets.bytes()).expect("the member decodes as .zrd")
}
