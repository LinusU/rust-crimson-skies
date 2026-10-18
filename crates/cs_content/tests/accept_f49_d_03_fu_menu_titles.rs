//! Task #1584 `F49-D.03-FU-MENU`: where the original Instant Action menu's
//! titles live in the installation, measured through the production message
//! catalog (`MessageCatalog` over `strings.dll`).
//!
//! Only structure is asserted: message names, `RT_STRING` ids, counts and
//! "resolves to one non-empty row". No original text is written here.

use cs_content::config::{MessageCatalog, MessageLookup};
use cs_types::asset_id::SourceSpan;

/// `(mission type, message-name stem, first id)`: each stem owns `HEAD1`,
/// `HEAD2`, `OBJ1`, `OBJ2` at `id`, `id + 1`, `id + 2`, `id + 3`.
const TYPE_BRIEFINGS: [(&str, &str, u32); 4] = [
    ("dogfight_ace", "MSG_BRF_IAS", 16_680),
    ("dogfight_squadron", "MSG_BRF_IAT", 16_700),
    ("stunt_flying", "MSG_BRF_IASF", 16_720),
    ("zeppelin_run", "MSG_BRF_IAZ", 16_740),
];

fn catalog() -> MessageCatalog {
    let root =
        std::path::PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));
    let found = cs_assets::install::discover(&root).expect("the installation discovers");
    let install = cs_assets::install::fingerprint(&found.manifest);
    let bytes = std::fs::read(root.join("strings.dll")).expect("strings.dll reads");
    let span = SourceSpan::new(install, "strings.dll", None, 0, bytes.len() as u64, None)
        .expect("the span validates");
    let mut context = cs_formats::ParseContext::with_defaults("install/strings.dll".to_string());
    MessageCatalog::read(&mut context, span, &bytes).expect("strings.dll's catalog reads")
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f49_d_03_fu_menu_titles_each_offered_type_has_one_heading_and_no_scenario_has_one() {
    let catalog = catalog();

    // Each offered mission type owns exactly one four-message briefing block.
    for (mission_type, stem, first) in TYPE_BRIEFINGS {
        for (offset, part) in ["HEAD1", "HEAD2", "OBJ1", "OBJ2"].into_iter().enumerate() {
            let name = format!("{stem}_{part}");
            match catalog.resolve(&name, None) {
                MessageLookup::Found { id, row } => {
                    assert_eq!(id, first + offset as u32, "{mission_type}: {name}");
                    assert!(row.text.as_deref().is_some_and(|t| !t.is_empty()), "{name}");
                }
                other => panic!("{mission_type}: {name} must resolve to one row: {other:?}"),
            }
        }
    }

    // No message names a scenario (c1..c5) or the unoffered `ground_target`
    // type: the catalog holds no per-scenario title.
    let scenario_or_ground = catalog.messages().records().iter().filter(|record| {
        let name = record.name.to_ascii_uppercase();
        let after_ia_c = name
            .strip_prefix("MSG_IA_C")
            .and_then(|rest| rest.chars().next());
        after_ia_c.is_some_and(|c| c.is_ascii_digit()) || name.starts_with("MSG_BRF_IAG")
    });
    assert_eq!(scenario_or_ground.count(), 0);

    // The 17 `MSG_IA_*` rows (4000..=4016) are customize-screen labels.
    let labels = catalog
        .messages()
        .records()
        .iter()
        .filter(|record| record.name.starts_with("MSG_IA_"))
        .count();
    assert_eq!(labels, 17);
}
