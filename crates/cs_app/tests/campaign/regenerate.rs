//! Shared regeneration harness for the committed mission binding records.
//!
//! **Not an acceptance test.** Rewrites `missions/bindings/<label>.json`
//! from what production code derives, so a record can be regenerated rather
//! than hand-edited whenever the derivation or a mission-scoped entry
//! changes (every `accept_mNN_a_the_committed_record_is_what_the_
//! installation_derives` byte-compares the two). Run by hand with
//! `CS_GAME_DIR` set and `CS_REGENERATE_BINDING=M06`; it leaves the file
//! byte-identical when the record is already current.

use std::path::PathBuf;
use std::sync::OnceLock;

use cs_content::campaign_bindings::{MissionLabel, SourceContext};

use crate::campaign::common::{load_inventory, repo_path};

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: record regeneration needs the retail capability; run with \
             `--include-ignored` and CS_GAME_DIR pointing at the read-only installation"
        )
    }))
}

#[test]
#[ignore = "regeneration harness: needs CS_GAME_DIR and CS_REGENERATE_BINDING=<label>"]
fn regenerate_binding_writes_the_committed_record_through_production() {
    let work_order = std::env::var("CS_REGENERATE_BINDING").unwrap_or_else(|_| {
        panic!(
            "set CS_REGENERATE_BINDING=<label> (e.g. M06) to rewrite its committed record from \
             the production derivation"
        )
    });
    let context: &'static SourceContext = {
        static CONTEXT: OnceLock<SourceContext> = OnceLock::new();
        CONTEXT.get_or_init(|| {
            SourceContext::read(&game_dir()).expect("the installation yields a source context")
        })
    };
    let title = load_inventory()
        .iter()
        .find(|(label, _)| label.as_str() == work_order)
        .map(|(_, title)| title.clone())
        .unwrap_or_else(|| panic!("the declared inventory has no {work_order} work order"));
    let binding = context
        .bind(
            MissionLabel::new(&work_order).unwrap_or_else(|error| panic!("{work_order}: {error}")),
            &title,
        )
        .unwrap_or_else(|error| panic!("{work_order} binds to the original data: {error}"));
    binding
        .validate()
        .expect("the derived record is internally consistent");
    assert!(
        binding.unresolved_critical().is_empty(),
        "refusing to write a record with unresolved critical dependencies"
    );
    let path = repo_path(&format!("missions/bindings/{work_order}.json"));
    std::fs::write(&path, binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
    println!("wrote {}", path.display());
}
