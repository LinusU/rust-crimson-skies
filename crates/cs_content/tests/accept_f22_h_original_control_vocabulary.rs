//! Acceptance tests for F22-H (#411): measure the original 2000 PC game's
//! control vocabulary from the read-only installation (`$CS_GAME_DIR`,
//! capability `retail`).
//!
//! The original exposes its control vocabulary in three places, all measured
//! here through production readers where one exists:
//!
//! 1. `strings.dll`'s `RT_STRING` tree (via `cs_content::config::StringCatalog`)
//!    holds the command, device and key/button labels by id, and its PE `.data`
//!    section holds a `{name, id}` table that names all 65 command labels, the
//!    seven control categories, the two device-family modes and the 44
//!    key/button labels.
//! 2. `crimson.icd` — the packed original game executable — carries a plaintext
//!    pool of 120 `key_*` names.
//! 3. `GOSDATA/ASSETS/crimson.rof` holds the control UI scripts, decoded
//!    through the production `read_tree`/`read_member` container reader.
//!
//! Default bindings are **not** observable in any shipped file: the control
//! scripts read every command row from native callbacks (`2115`, `2120`,
//! `2139`, `2140`). They are recorded as unknown in
//! `docs/findings/2026-10-02-f22-h-original-control-vocabulary.md` and are
//! deliberately not asserted as a value here.
//!
//! No original display text is reproduced or asserted: a label is identified
//! by its id and measured code-unit length only.

#[path = "f22_h_support/mod.rs"]
mod support;

use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};
use cs_content::config::{StringCatalog, StringLookup};
use cs_formats::{ParseContext, RofLimits, read_member, read_tree};
use cs_types::asset_id::SourceSpan;
use cs_types::input::{FlightCommand, UiAction};
use support::*;

/// The production `StringCatalog` over the installation's `strings.dll`.
fn read_string_catalog(dir: &std::path::Path) -> (StringCatalog, Vec<u8>) {
    let bytes = read_installation_file(dir, "strings.dll");
    let install = fingerprint(&discover(dir).expect("the installation discovers").manifest);
    let span = SourceSpan::new(install, "strings.dll", None, 0, bytes.len() as u64, None)
        .expect("a valid source span");
    let mut context = ParseContext::with_defaults("strings.dll");
    let catalog = StringCatalog::read(&mut context, span, &bytes)
        .expect("the production reader must read strings.dll");
    (catalog, bytes)
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f22_h_retail_strings_dll_names_the_original_command_vocabulary() {
    let dir = game_dir();
    let found = discover(&dir).expect("the installation discovers");
    assert_eq!(
        fingerprint(&found.manifest).to_hex(),
        INSTALL_SHA256,
        "the installation fingerprint must match the measured one"
    );
    assert_eq!(
        content_fingerprint(&found.manifest).to_hex(),
        CONTENT_SHA256,
        "the canonical-content fingerprint must match the measured one"
    );

    let (catalog, bytes) = read_string_catalog(&dir);
    assert_eq!(
        sha256(&bytes).to_hex(),
        STRINGS_DLL_SHA256,
        "strings.dll must be the measured image"
    );
    assert_eq!(bytes.len() as u64, STRINGS_DLL_LEN);

    // The production reader's own accounting of the image.
    let accounting = catalog.accounting();
    assert_eq!(accounting.strings, STRINGS_STRING_UNITS);
    assert_eq!(accounting.other_leaves, STRINGS_OTHER_LEAVES);
    assert_eq!(accounting.undecodable, 0);
    assert_eq!(accounting.duplicate_ids, 0);
    assert_eq!(catalog.resources().strings().len(), STRINGS_STRING_BLOCKS);
    assert_eq!(catalog.languages(), vec![STRINGS_LANGUAGE]);

    // The `.data` `{name, id}` table names all 65 command labels.
    let table = parse_name_id_table(&bytes);
    assert_eq!(table.len(), NAME_ID_ENTRIES);
    assert_eq!(table.values().min().copied(), Some(NAME_ID_MIN));
    assert_eq!(table.values().max().copied(), Some(NAME_ID_MAX));
    for (name, id) in COMMAND_LABELS {
        assert_eq!(
            table.get(name).copied(),
            Some(id),
            "{name} must name id {id} in the .data table"
        );
        let row = match catalog.resolve(id, Some(STRINGS_LANGUAGE)) {
            StringLookup::Found(row) => row,
            other => panic!("{name} (id {id}) must resolve through StringCatalog, got {other:?}"),
        };
        assert!(
            row.text.is_some(),
            "{name} (id {id}) must decode in language {STRINGS_LANGUAGE}"
        );
        assert!(
            !row.code_units.is_empty(),
            "{name} (id {id}) must not be an empty command label"
        );
    }

    // Category headers, device families, device sources, button labels and
    // key labels.
    let named: Vec<(&str, u32)> = CONTROL_CATEGORIES
        .iter()
        .chain(DEVICE_FAMILIES.iter())
        .chain(DEVICE_SOURCES.iter())
        .chain(BUTTON_LABELS.iter())
        .chain(KEY_LABELS.iter())
        .copied()
        .collect();
    for (name, id) in named {
        assert_eq!(
            table.get(name).copied(),
            Some(id),
            "{name} must name id {id}"
        );
        let row = match catalog.resolve(id, Some(STRINGS_LANGUAGE)) {
            StringLookup::Found(row) => row,
            other => panic!("{name} (id {id}) must resolve, got {other:?}"),
        };
        assert!(
            !row.code_units.is_empty(),
            "{name} (id {id}) must not be an empty label"
        );
    }

    // The label ranges the control UI reads: the counts are measured, so a
    // lost or duplicated row is visible here even for labels with no symbol.
    // Every label in the command ranges is named, so the non-empty count must
    // equal the number of `COMMAND_LABELS` in the range.
    let non_empty = |low: u32, high: u32| {
        (low..=high)
            .filter(|id| match catalog.resolve(*id, Some(STRINGS_LANGUAGE)) {
                StringLookup::Found(row) => !row.code_units.is_empty(),
                StringLookup::Missing => false,
                StringLookup::Ambiguous(count) => panic!("id {id} is ambiguous ({count} rows)"),
            })
            .count()
    };
    for (low, high, expected) in COMMAND_RANGES {
        assert_eq!(non_empty(low, high), expected, "range {low}..={high}");
        let named_in_range = COMMAND_LABELS
            .iter()
            .filter(|(_, id)| (low..=high).contains(id))
            .count();
        assert_eq!(
            named_in_range, expected,
            "every label in {low}..={high} must be named"
        );
    }
    let (low, high, expected) = KEY_BUTTON_RANGE;
    assert_eq!(non_empty(low, high), expected, "range {low}..={high}");
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f22_h_retail_the_game_executable_and_control_scripts_expose_the_key_vocabulary() {
    let dir = game_dir();

    // The game executable carries the plaintext `key_*` pool.
    let icd = read_installation_file(&dir, "crimson.icd");
    assert_eq!(sha256(&icd).to_hex(), CRIMSON_ICD_SHA256);
    assert_eq!(icd.len() as u64, CRIMSON_ICD_LEN);
    let names = parse_icd_key_names(&icd);
    assert_eq!(names.len(), ICD_KEY_NAMES);
    assert_eq!(
        names.first().map(|(offset, name)| (*offset, name.as_str())),
        Some((ICD_KEY_POOL_START, "key_pause"))
    );
    assert_eq!(
        names.last().map(|(offset, name)| (*offset, name.as_str())),
        Some((ICD_KEY_POOL_END - "key_escape".len() as u64, "key_escape"))
    );
    for wanted in [
        "key_lshift",
        "key_rshift",
        "key_lcontrol",
        "key_rcontrol",
        "key_return",
        "key_escape",
        "key_numpad0",
        "key_f12",
        "key_space",
        "key_tab",
    ] {
        assert!(
            names.iter().any(|(_, name)| name == wanted),
            "the executable's key pool must contain {wanted}"
        );
    }

    // The control UI scripts, decoded through the production container reader.
    let rof = read_installation_file(&dir, CRIMSON_ROF);
    let mut context = ParseContext::with_defaults(CRIMSON_ROF);
    let tree = read_tree(&mut context, &rof).expect("the container's tree must walk");
    assert_eq!(tree.members().len(), CRIMSON_ROF_MEMBERS);

    let mut decoded: std::collections::BTreeMap<&str, Vec<u8>> = std::collections::BTreeMap::new();
    for (spelling, length, digest) in CONTROL_SCRIPTS {
        let wanted: Vec<&str> = spelling.split('/').collect();
        let member = tree
            .members()
            .iter()
            .find(|entry| {
                entry.path.len() == wanted.len()
                    && entry
                        .path
                        .iter()
                        .zip(&wanted)
                        .all(|(segment, name)| segment.eq_ignore_ascii_case(name.as_bytes()))
            })
            .unwrap_or_else(|| panic!("{spelling}: no such member in {CRIMSON_ROF}"));
        let read = read_member(&context, &rof, member, &RofLimits::default())
            .unwrap_or_else(|error| panic!("{spelling}: the member must read: {error}"));
        assert_eq!(
            read.trailing_len, 0,
            "{spelling}: no unconsumed stored bytes"
        );
        assert_eq!(read.data.len() as u64, length, "{spelling}: decoded length");
        assert_eq!(
            sha256(&read.data).to_hex(),
            digest,
            "{spelling}: decoded bytes must match the measured digest"
        );
        decoded.insert(spelling, read.data);
    }

    // The control widget handles ten special keys itself; the keyboard screen
    // is driven entirely by native callbacks, so it binds no key by hand.
    let ctl = &decoded["ASSETS/SCRIPTS/CTL.SCRIPT"];
    for constant in CTL_SCRIPT_KEY_CONSTANTS {
        assert!(
            contains(ctl, constant.as_bytes()),
            "CTL.SCRIPT must handle the special key {constant}"
        );
    }
    let keys = &decoded["ASSETS/SCRIPTS/KEYS.SCRIPT"];
    for callback in [b"2115", b"2139", b"2140"] {
        assert!(
            contains(keys, callback),
            "KEYS.SCRIPT must fetch its rows through native callback {}",
            std::str::from_utf8(callback).unwrap()
        );
    }
    let prefs = &decoded["ASSETS/SCRIPTS/CONTROLSPREFS.SCRIPT"];
    for callback in [b"2120"] {
        assert!(
            contains(prefs, callback),
            "CONTROLSPREFS.SCRIPT must fetch its rows through native callback {}",
            std::str::from_utf8(callback).unwrap()
        );
    }
}

/// A single production-path regression test that runs in CI: the comparison
/// table must classify every declared command and UI action exactly once, and
/// its evidence must partition the measured original command labels. A new
/// `FlightCommand` or `UiAction` fails here until it is classified against the
/// original installation.
#[test]
fn accept_f22_h_the_comparison_covers_every_declared_command() {
    assert_eq!(
        COMPARISON.len(),
        FlightCommand::ALL.len() + UiAction::ALL.len(),
        "the comparison must classify every declared command and UI action"
    );
    for command in FlightCommand::ALL {
        let (status, _) = comparison_entry(command.label()).unwrap_or_else(|| {
            panic!("{} is not classified against the original", command.label())
        });
        assert!(
            matches!(status, "observed" | "absent" | "runtime_only"),
            "{} has an unknown status {status}",
            command.label()
        );
    }
    for action in UiAction::ALL {
        let (status, _) = comparison_entry(action.label())
            .unwrap_or_else(|| panic!("{} is not classified against the original", action.label()));
        assert!(
            matches!(status, "observed" | "absent" | "runtime_only"),
            "{} has an unknown status {status}",
            action.label()
        );
    }
    // Every declared label appears once, so a copy/paste cannot double-count.
    let mut labels: Vec<&str> = COMPARISON.iter().map(|(label, _, _)| *label).collect();
    labels.sort_unstable();
    labels.dedup();
    assert_eq!(labels.len(), COMPARISON.len());

    // Evidence discipline: an `observed` command names at least one measured
    // original command label and no other status carries evidence.
    for (label, status, evidence) in COMPARISON {
        match status {
            "observed" => assert!(
                !evidence.is_empty(),
                "{label} is observed but names no original label"
            ),
            "absent" | "runtime_only" => assert!(
                evidence.is_empty(),
                "{label} is {status} but still names original labels"
            ),
            other => panic!("{label} has an unknown status {other}"),
        }
        for name in evidence {
            assert!(
                command_label_id(name).is_some(),
                "{label} cites {name}, which is not a measured command label"
            );
        }
    }

    // The 65 measured command labels partition into the ones the project also
    // declares (cited by an observed entry) and the original-only set, with no
    // overlap and no label lost.
    let mut cited: Vec<&str> = COMPARISON
        .iter()
        .flat_map(|(_, _, evidence)| evidence.iter().copied())
        .collect();
    cited.sort_unstable();
    cited.dedup();
    assert_eq!(
        cited.len(),
        32,
        "the project maps 32 original command labels"
    );
    assert_eq!(
        ORIGINAL_ONLY_COMMANDS.len(),
        33,
        "33 original command labels have no project counterpart"
    );
    for name in ORIGINAL_ONLY_COMMANDS {
        assert!(
            command_label_id(name).is_some(),
            "original-only command {name} is not a measured command label"
        );
        assert!(
            !cited.contains(&name),
            "original-only command {name} is also cited by an observed command"
        );
    }
    let mut all: Vec<&str> = COMMAND_LABELS.iter().map(|(name, _)| *name).collect();
    all.extend(cited.iter().copied());
    all.extend(ORIGINAL_ONLY_COMMANDS.iter().copied());
    all.sort_unstable();
    all.dedup();
    assert_eq!(
        all.len(),
        COMMAND_LABELS.len(),
        "the cited and original-only labels must cover the measured vocabulary exactly"
    );
    assert_eq!(
        cited.len() + ORIGINAL_ONLY_COMMANDS.len(),
        COMMAND_LABELS.len(),
        "the project-mapped and original-only labels must not overlap"
    );
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}
