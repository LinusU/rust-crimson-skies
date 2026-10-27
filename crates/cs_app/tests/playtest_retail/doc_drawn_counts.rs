//! #769 (`PLAYTEST-DOC-DRAWN-COUNTS`): `docs/PLAYTEST.md`'s free-flight banner
//! named the airship's *stored* subtree measurement — "subtree `piratezep`, 401
//! mesh records, 8 673 triangles" — in a sentence that lists what the owner
//! flies, where it reads as the drawn set. Since #753 the documented selection
//! draws one intact variant of every part, and since #795 the two flat-colour
//! landing cards are hidden too: **293 of the 401 stored bindings, 8 108 of the
//! 8 673 stored triangles**, the same numbers
//! `accept_playtest_retail_retail_c1c_area_and_bloodhawk_mesh_spawn_and_capture`
//! re-measures over the retail installation (`area.stored_bindings`,
//! `area.mesh_records`, `area.triangles`). The doc must say which count is
//! which so it cannot silently drift back. Task test prefix:
//! `accept_playtest_doc_drawn_counts_`.
//!
//! This is a doc-content test in the style of
//! `area_flicker::accept_playtest_area_flicker_docs_describe_the_selection_as_provisional`:
//! it reads the committed documents and needs no installation.

use std::path::Path;

/// Whitespace-collapsed text, so a phrase the doc wraps over a line break still
/// matches.
fn flattened(file: &str) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::read_to_string(root.join(file))
        .expect("the doc exists")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// **`docs/PLAYTEST.md` states the stored count as stored and the drawn count
/// under the #753 selection.**
#[test]
fn accept_playtest_doc_drawn_counts_playtest_banner_names_stored_and_drawn() {
    let doc = flattened("docs/PLAYTEST.md");
    assert!(
        doc.contains("subtree `piratezep`: 401 stored mesh bindings"),
        "the banner must name the stored binding count as stored"
    );
    assert!(
        doc.contains("8 673 stored triangles"),
        "the banner must name the stored triangle count as stored"
    );
    assert!(
        doc.contains("#753") && doc.contains("draws 293 bindings / 8 108 triangles"),
        "the banner must state the drawn set under the #753 selection"
    );
}

/// **`docs/PLAYTEST-RETAIL.md` says the same thing consistently.**
#[test]
fn accept_playtest_doc_drawn_counts_retail_doc_states_the_same_counts() {
    let doc = flattened("docs/PLAYTEST-RETAIL.md");
    assert!(
        doc.contains("8 673 stored triangles"),
        "the stored triangle count must be labelled stored"
    );
    assert!(
        doc.contains("293 of the 401 stored bindings") && doc.contains("8 108"),
        "the retail doc must state the same drawn set"
    );
}
