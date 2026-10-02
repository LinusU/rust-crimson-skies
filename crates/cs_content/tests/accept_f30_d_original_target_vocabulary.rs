//! F30-D at the content layer: what the original installation actually *names*
//! about targeting, and what it does not name at all.
//!
//! Spec: `specs/F30-targeting-classification-aim-assistance-and-threat-cues.md`,
//! stage `### F30-D`. Task test prefix: `accept_f30_d_`. Shared contract:
//! `docs/contracts/CLI-EVIDENCE.md`.
//!
//! These two tests need the original installation (`capability: retail`) and
//! are `#[ignore]`d so CI (which has none) skips them; they must be run with
//! `--include-ignored`, and they fail loudly when `CS_GAME_DIR` is absent rather
//! than passing on nothing.
//!
//! They measure, read-only, through **production** readers wherever one owns
//! the format:
//!
//! * `cs_assets::install::{discover, fingerprint, content_fingerprint}` over the
//!   whole installation, and `cs_assets::install::sha256` over `strings.dll`;
//! * `cs_content::config::StringCatalog` over `strings.dll`'s `RT_STRING`
//!   tree — every id the test-local `{name, id}` table returns is resolved
//!   through it, so a wrong parse fails here;
//! * `cs_content::target_rules::DeclaredAction` — the project's own declared
//!   action vocabulary, which the classification table must cover exactly.
//!
//! What is measured: the original's **target-action vocabulary** (eleven
//! `MSG_CMD_TARGET_*` command labels plus the `MSG_TARGETING_CONTROLS`
//! category), the **padlock** command family (twelve labels), four
//! target-display text fonts, and two *absences* measured over a table in which
//! every entry is named: no reveal/visibility concept and no lead-indicator or
//! aim-assistance option appear in the shipped string image.
//!
//! What is **not** measured, and not asserted anywhere: the original's target
//! **order**, its **reveal rules** and its **assistance behavior**. The
//! rebinding UI fetches each command row and its binding from native callbacks
//! in the packed executable (F22-H, Observation 3), and a label name says which
//! commands exist, never what they do. `absent` therefore means "absent from the
//! shipped observation", never "the original cannot do it", and nothing here is
//! a `verified_original` claim.

#[path = "f30_d_support/mod.rs"]
mod support;

use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};
use cs_content::config::{StringCatalog, StringLookup};
use cs_content::target_rules::DeclaredAction;
use cs_formats::ParseContext;
use cs_types::asset_id::SourceSpan;
use cs_types::evidence::ContentHash;
use support::*;

/// `strings.dll` read through the production `StringCatalog`, plus the
/// test-local `{name, id}` table of the same image.
struct Measurement {
    table: std::collections::BTreeMap<String, u32>,
    catalog: StringCatalog,
    install: ContentHash,
    strings_sha256: String,
    strings_len: usize,
}

fn measure() -> Measurement {
    let dir = game_dir();
    let found = discover(&dir).expect("production discovery reads the installation");
    let install = fingerprint(&found.manifest);
    assert_eq!(
        install.to_hex(),
        INSTALL_SHA256,
        "the installation fingerprint this measurement was taken against"
    );
    assert_eq!(
        content_fingerprint(&found.manifest).to_hex(),
        CONTENT_SHA256,
        "the canonical-content fingerprint this measurement was taken against"
    );

    let bytes = std::fs::read(dir.join("strings.dll")).expect("strings.dll is in the installation");
    assert_eq!(bytes.len() as u64, STRINGS_DLL_LEN, "image length");
    let strings_sha256 = sha256(&bytes).to_hex();
    assert_eq!(strings_sha256, STRINGS_DLL_SHA256, "image digest");

    let span = SourceSpan::new(install, "strings.dll", None, 0, bytes.len() as u64, None)
        .expect("a valid span over the whole image");
    let mut context = ParseContext::with_defaults("strings.dll");
    let catalog = StringCatalog::read(&mut context, span, &bytes).expect("strings.dll reads");
    Measurement {
        table: parse_name_id_table(&bytes),
        catalog,
        install,
        strings_sha256,
        strings_len: bytes.len(),
    }
}

/// Resolves one symbolic name through the production catalog and returns its
/// measured code-unit length. The label's *text* never leaves this function.
fn label_units(measurement: &Measurement, name: &str, id: u32) -> usize {
    assert_eq!(
        measurement.table.get(name).copied(),
        Some(id),
        "{name} must be a named {id} in the .data table"
    );
    match measurement.catalog.resolve(id, Some(STRINGS_LANGUAGE)) {
        StringLookup::Found(row) => {
            assert!(
                !row.code_units.is_empty(),
                "{name} ({id}) is an empty RT_STRING label, so the name does not \
                 label anything the original shows"
            );
            row.code_units.len()
        }
        other => panic!("{name} ({id}) must resolve, got {other:?}"),
    }
}

/// The original's target vocabulary, measured: every target command label, the
/// targeting control category, the padlock family and the target fonts resolve
/// to non-empty labels under their measured ids, and the whole table is the
/// size and id range this measurement recorded.
///
/// This is the part of F30-D that is an observation rather than a design
/// statement: the original *names* eleven target commands — a clear, an
/// under-reticule pick and a next/previous/nearest triple for each of three
/// named classes (enemy, ally, ground). It names an assist family too, the
/// twelve padlock commands.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f30_d_retail_names_the_original_target_action_vocabulary() {
    let measurement = measure();

    // The label tree itself is sane: the accounting this test resolves ids
    // through, measured on this installation.
    let accounting = measurement.catalog.accounting();
    assert_eq!(accounting.strings, STRINGS_STRING_UNITS, "RT_STRING units");
    assert_eq!(
        measurement.catalog.resources().strings().len(),
        STRINGS_STRING_BLOCKS,
        "RT_STRING blocks"
    );
    assert_eq!(accounting.other_leaves, STRINGS_OTHER_LEAVES);
    assert_eq!(accounting.undecodable, 0, "every label decodes");
    assert_eq!(accounting.duplicate_ids, 0, "an id identifies one label");

    // The `{name, id}` table: 1 023 named entries, ids 100…17 142, every one
    // of the measured target names among them.
    assert_eq!(
        measurement.table.len(),
        NAME_ID_ENTRIES,
        "the measured name/id table this stage walked"
    );
    let ids = measurement.table.values().copied();
    assert_eq!(
        ids.clone().min(),
        Some(NAME_ID_MIN),
        "the lowest RT_STRING id the table carries"
    );
    assert_eq!(ids.max(), Some(NAME_ID_MAX), "the highest");

    let (category, category_id) = TARGETING_CATEGORY;
    let category_units = label_units(&measurement, category, category_id);
    assert!(category_units > 0);

    let mut target_units = Vec::new();
    for (name, id) in TARGET_COMMAND_LABELS {
        target_units.push((name, id, label_units(&measurement, name, id)));
    }
    assert_eq!(
        target_units.len(),
        11,
        "eleven target command labels, and the table above is the whole measured set"
    );

    let mut padlock_units = Vec::new();
    for (name, id) in PADLOCK_COMMAND_LABELS {
        padlock_units.push((name, id, label_units(&measurement, name, id)));
    }
    assert_eq!(
        padlock_units.len(),
        12,
        "the measured padlock family: three mode commands and nine directions"
    );

    let mut font_units = Vec::new();
    for (name, id) in TARGET_FONTS {
        font_units.push((name, id, label_units(&measurement, name, id)));
    }
    assert_eq!(
        font_units.len(),
        4,
        "an aim-point font and three target-display text fonts are named"
    );

    // The names are distinct ids and distinct labels: no two measured names
    // alias, so a "which command is this" question is answerable.
    let mut seen = std::collections::BTreeSet::new();
    for (name, id, _) in target_units.iter().chain(&padlock_units).chain(&font_units) {
        assert!(seen.insert(*id), "{name} aliases an id already measured");
    }
    assert_eq!(measurement.strings_len as u64, STRINGS_DLL_LEN);
    assert_eq!(measurement.strings_sha256, STRINGS_DLL_SHA256);
    assert_eq!(measurement.install.to_hex(), INSTALL_SHA256);
}

/// The absences, measured over a table in which **every** entry is named: the
/// shipped string image names no reveal/visibility concept and no
/// lead-indicator or aim-assistance option, and the project's declared actions
/// classify completely against what it does name.
///
/// The classification is the load-bearing part: it is asserted to cover
/// `DeclaredAction::ALL` exactly once, every `observed` row to cite a measured
/// label, and every `absent` row to cite none — so a new declared action cannot
/// be added without classifying it, and an `absent` row cannot quietly acquire
/// evidence.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f30_d_retail_names_no_reveal_or_assistance_option() {
    let measurement = measure();

    // The absence measurement. Every name in the table is measured, so an empty
    // match set is a statement about the whole shipped string vocabulary.
    for fragment in ABSENT_NAME_FRAGMENTS {
        let hits: Vec<&String> = measurement
            .table
            .keys()
            .filter(|name| name.contains(fragment))
            .collect();
        assert!(
            hits.is_empty(),
            "{fragment} matched {hits:?}: the shipped string image does name one, \
             so this measurement and that table cannot both be right"
        );
    }

    // The classification covers the project's declared actions exactly once.
    assert_eq!(
        ACTION_CLASSIFICATION.len(),
        DeclaredAction::ALL.len(),
        "one classification row per declared action"
    );
    let measured_targets: Vec<&str> = TARGET_COMMAND_LABELS
        .iter()
        .map(|(name, _)| *name)
        .collect();
    for (label, status, evidence) in ACTION_CLASSIFICATION {
        let action = DeclaredAction::from_label(label).unwrap_or_else(|| {
            panic!("{label} is not a declared action label this project can lower")
        });
        assert_eq!(
            action.label(),
            label,
            "the row names the action it classifies"
        );
        match status {
            "observed" => {
                assert!(
                    !evidence.is_empty(),
                    "{label} claims observed evidence and cites none"
                );
                for name in evidence {
                    assert!(
                        measured_targets.contains(name),
                        "{label} cites {name}, which is not a measured target command label"
                    );
                    let id = measurement
                        .table
                        .get(*name)
                        .copied()
                        .unwrap_or_else(|| panic!("{name} must be a named label"));
                    label_units(&measurement, name, id);
                }
            }
            "absent" => assert!(
                evidence.is_empty(),
                "{label} is absent from the observation and cites evidence anyway"
            ),
            other => panic!("{label} has unknown status {other:?}"),
        }
    }
    // Every declared action is classified: the rows above and `DeclaredAction::ALL`
    // are the same set of labels, which is what stops an unclassified action.
    for action in DeclaredAction::ALL {
        assert!(
            ACTION_CLASSIFICATION
                .iter()
                .any(|(label, _, _)| *label == action.label()),
            "{} is declared but not classified against the measured vocabulary",
            action.label()
        );
    }
    // And the two rows the original does not name are exactly the two this
    // project added for its own deliverable: the nearest-attacker and
    // nearest-objective picks.
    let absent: Vec<&str> = ACTION_CLASSIFICATION
        .iter()
        .filter(|(_, status, _)| *status == "absent")
        .map(|(label, _, _)| *label)
        .collect();
    assert_eq!(
        absent,
        vec!["nearest_objective", "nearest_attacker"],
        "these two declared actions have no name in the shipped string vocabulary"
    );
    // The original's third named class is `GROUND`; the project spells that
    // axis `nearest_non_aircraft`. The name is the evidence, so the spelling
    // difference must stay visible rather than be papered over.
    assert!(
        !absent.contains(&"nearest_non_aircraft"),
        "the non-aircraft axis is named by MSG_CMD_TARGET_NEAREST_GROUND"
    );
}
