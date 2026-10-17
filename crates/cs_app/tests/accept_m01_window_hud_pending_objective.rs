//! Task #1539 `M01-WINDOW-HUD`: the windowed M01 draws an objective from the
//! first frame. Every M01 identity block loads dormant, so the live-only
//! `current()` left the window without a line (owner playtest 2026-10-17); the
//! widget now draws [`ObjectiveHud::displayed`], the live row or else the
//! pending one with the smallest rank (a designed fallback, labelled so).
//!
//! No original text is written here: the drawn text is compared with what the
//! production catalog resolves for the message name.

use std::path::PathBuf;

use cs_app::objectives::recover_retail_objectives;
use cs_app::ui::hud::{MissionMessages, ObjectiveHud};
use cs_content::config::{MessageCatalog, MessageLookup};
use cs_content::objectives_engine::{EngineObjective, EngineObjectives};

fn expected(catalog: &MessageCatalog, name: &str) -> String {
    match catalog.resolve(name, None) {
        MessageLookup::Found { row, .. } => row.text.clone().expect("the text decodes"),
        other => panic!("{name} must resolve to exactly one string row: {other:?}"),
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_window_hud_pending_objective_shows_from_launch_and_advances() {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));
    let found = cs_assets::install::discover(&root).expect("the installation discovers");
    let install = cs_assets::install::fingerprint(&found.manifest);
    let messages = MissionMessages::read(&root, install);
    let catalog = messages.catalog().expect("strings.dll reads");

    let recovery = recover_retail_objectives(&root, "zbd/c1c/m01").expect("M01 reads");
    let objectives = (1..=u32::try_from(recovery.engine_objective_count()).expect("small"))
        .map(|number| {
            recovery
                .engine_declarations()
                .get(&number)
                .cloned()
                .unwrap_or_default()
        })
        .map(EngineObjective::loaded)
        .collect();
    let mut engine = EngineObjectives::new(objectives).expect("targets in range");

    // Launch: nothing is live, the first objective is drawn.
    let hud = ObjectiveHud::from_engine(&engine);
    assert!(hud.current().is_none());
    assert_eq!(
        hud.displayed_text(Some(catalog)).expect("a line at launch"),
        expected(catalog, "MSG_BRF_NWM1_OBJ1")
    );

    // It stays through the mission clock's own wakes.
    engine.advance(3.0);
    let hud = ObjectiveHud::from_engine(&engine);
    assert_eq!(
        hud.displayed().map(|row| row.objective),
        Some(8),
        "OBJECTIVE9 is the first pending identity"
    );

    // Completing it advances the line to the second objective at once, even
    // while the latch hops that wake OBJECTIVE11 are still running.
    engine.wake(&[8]);
    engine.complete(8);
    let hud = ObjectiveHud::from_engine(&engine);
    assert_eq!(
        hud.displayed_text(Some(catalog)).expect("a second line"),
        expected(catalog, "MSG_BRF_NWM1_OBJ2")
    );
}
