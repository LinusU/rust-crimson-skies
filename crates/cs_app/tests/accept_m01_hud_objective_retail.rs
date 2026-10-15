//! The retail half of task #1462 `M01-HUD-OBJECTIVE`: the four message ids
//! M01's `IDENTITY` declarations carry resolve to the **installation's own
//! text** through production readers, and the HUD read model shows the
//! objective M01's recovered lifecycle model marks current, following the
//! record's own wake chain.
//!
//! The pairing these tests use is measured, not assumed: `strings.dll`'s
//! `.data` table of `{name pointer, string id}` records is the only place the
//! installation pairs a symbolic message name with an `RT_STRING` id — both
//! shipped `#define` headers carry zero `MSG_*` defines, so
//! `ResourceHeader::define` answers nothing for these names
//! (`docs/findings/2026-10-15-m01-hud-objective-message-ids.md` records that
//! measurement and the ids below).
//!
//! Every number asserted here is structural: an id, a count, a language id, a
//! container path, an objective index or a length. No original string text is
//! written into this file — the texts are compared against what the same
//! production catalog resolves.
//!
//! One test rather than several, because discovering the installation
//! (fingerprinting it) is the expensive half and both halves below need the
//! same read.

use std::path::PathBuf;

use cs_app::objectives::{ObjectiveRecovery, recover_retail_objectives};
use cs_app::ui::hud::{MissionMessages, ObjectiveHud};
use cs_content::config::{MessageCatalog, MessageLookup};
use cs_content::objectives_engine::{EngineObjective, EngineObjectives};
use cs_formats::text::MessageIdLookup;

/// The four M01 objective message ids and the `RT_STRING` ids their records
/// pair them with, as measured in `strings.dll`: ids 16 100…16 103, four
/// consecutive rows of block 1007.
const OBJECTIVE_MESSAGES: [(&str, u32); 4] = [
    ("MSG_BRF_NWM1_OBJ1", 16_100),
    ("MSG_BRF_NWM1_OBJ2", 16_101),
    ("MSG_BRF_NWM1_OBJ3", 16_102),
    ("MSG_BRF_NWM1_OBJ4", 16_103),
];

/// The message names M01's five `IDENTITY` declarations carry, in objective
/// number order — four messages and the message-less secondary.
const M01_MESSAGES: [Option<&str>; 5] = [
    Some("MSG_BRF_NWM1_OBJ1"),
    Some("MSG_BRF_NWM1_OBJ2"),
    Some("MSG_BRF_NWM1_OBJ3"),
    Some("MSG_BRF_NWM1_OBJ4"),
    None,
];

fn retail_root() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"))
}

/// The installation's message catalog, read through the production path the
/// composed HUD reads it through: `strings.dll` beside the fingerprint of the
/// installation it was read from.
fn messages(root: &std::path::Path) -> MessageCatalog {
    let found = cs_assets::install::discover(root).expect("the installation discovers");
    let install = cs_assets::install::fingerprint(&found.manifest);
    let messages = MissionMessages::read(root, install);
    let reason = messages.missing();
    assert!(
        reason.is_none(),
        "the installation's own message catalog must read: {}",
        reason.unwrap_or_default()
    );
    messages
        .catalog()
        .expect("the catalog is present when no reason is recorded")
        .clone()
}

/// M01's recovery, read through the production reader.
fn m01(root: &std::path::Path) -> ObjectiveRecovery {
    recover_retail_objectives(root, "zbd/c1c/m01").expect("M01's objective record reads")
}

/// The recovered declarations as engine objectives, the loader's way: every
/// loaded objective is [`EngineObjective::loaded`] on its declaration — the
/// same construction the mission composition performs.
fn loaded_engine(recovery: &ObjectiveRecovery) -> EngineObjectives {
    let objectives = (1..=u32::try_from(recovery.engine_objective_count()).expect("a few dozen"))
        .map(|number| {
            recovery
                .engine_declarations()
                .get(&number)
                .cloned()
                .unwrap_or_default()
        })
        .map(EngineObjective::loaded)
        .collect();
    EngineObjectives::new(objectives).expect("every target is in range")
}

/// The text the catalog itself resolves for `name`, asserted beside what the
/// HUD draws — the production reader compared against the production reader,
/// so no original text has to be written into the test.
fn expected(catalog: &MessageCatalog, name: &str) -> String {
    match catalog.resolve(name, None) {
        MessageLookup::Found { row, .. } => row.text.clone().expect("the text decodes"),
        other => panic!("{name} must resolve to exactly one string row: {other:?}"),
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_hud_objective_retail_installation_text_and_the_current_objective() {
    let root = retail_root();

    // --- The text: the four message ids resolve to the installation's own ---
    let catalog = messages(&root);
    assert_eq!(
        catalog.messages().len(),
        1_023,
        "strings.dll's message table, with its stored repeat collapsed"
    );
    let mut texts: Vec<&str> = Vec::new();
    for (name, id) in OBJECTIVE_MESSAGES {
        assert_eq!(
            catalog.id(name),
            MessageIdLookup::Found(id),
            "{name} pairs with exactly one id"
        );
        let row = match catalog.resolve(name, None) {
            MessageLookup::Found { id: resolved, row } => {
                assert_eq!(resolved, id, "{name} resolves to its recorded id");
                row
            }
            other => panic!("{name} must resolve to exactly one string row: {other:?}"),
        };
        assert_eq!(row.id, id, "{name}'s row id");
        assert_eq!(row.language, 1_033, "{name}'s language is the image's own");
        assert_eq!(
            row.span.container_path(),
            "strings.dll",
            "{name}'s text lives in the installation's string image"
        );
        let text = row
            .text
            .as_deref()
            .unwrap_or_else(|| panic!("{name}'s code units do not decode"));
        assert!(!text.is_empty(), "{name} is not an empty row");
        texts.push(text);
    }
    assert_eq!(
        texts.iter().collect::<std::collections::HashSet<_>>().len(),
        4,
        "the four objectives have four different texts"
    );

    // --- The read model, over M01's own recovery ---
    let recovery = m01(&root);
    let mut engine = loaded_engine(&recovery);

    // The five identity rows, in objective number order: O9, O11, O15, O18
    // (the four primaries) and O21 (the secondary with no message element).
    let hud = ObjectiveHud::from_engine(&engine);
    let rows = hud.rows();
    assert_eq!(rows.len(), 5, "M01 declares five identities");
    assert_eq!(
        rows.iter()
            .map(|row| row.message.as_deref())
            .collect::<Vec<_>>(),
        M01_MESSAGES.to_vec()
    );
    assert_eq!(
        rows.iter().map(|row| row.objective).collect::<Vec<_>>(),
        vec![8, 10, 14, 17, 20],
        "rows are the zero-based indices of OBJECTIVE9/11/15/18/21"
    );
    // Every identity block loads dormant in M01, so nothing is current at
    // launch: the HUD draws no line rather than the first objective by
    // default.
    assert!(
        rows.iter().all(|row| !row.live),
        "all five identities load dormant"
    );
    assert!(hud.current().is_none());
    assert!(hud.current_text(Some(&catalog)).is_none());

    // OBJECTIVE1's measured two-second self-wake marks an objective awake,
    // but it carries no `IDENTITY`, so still no line.
    engine.advance(3.0);
    assert!(ObjectiveHud::from_engine(&engine).current().is_none());

    // The first identity block waking is what makes the line appear, and its
    // text is the installation's own.
    engine.wake(&[8]);
    let hud = ObjectiveHud::from_engine(&engine);
    let current = hud.current().expect("OBJECTIVE9 is awake with an identity");
    assert_eq!(current.objective, 8);
    assert_eq!((current.role_code, current.ordinal), (1, 1));
    let drawn = hud
        .current_text(Some(&catalog))
        .expect("the message resolves, so there is a line");
    assert_eq!(drawn, expected(&catalog, "MSG_BRF_NWM1_OBJ1"));
    assert!(!drawn.is_empty());

    // Completing OBJECTIVE9 runs the measured effects: it becomes completed
    // (so the HUD stops showing it) and naps OBJECTIVE10, the record's next
    // rung — which carries no `IDENTITY`, so the line hides rather than
    // showing a stale objective.
    engine.complete(8);
    let hud = ObjectiveHud::from_engine(&engine);
    assert!(hud.rows()[0].completed, "OBJECTIVE9 completed");
    assert!(
        !hud.rows()[0].live,
        "a completed objective is never current"
    );
    assert!(
        hud.current().is_none(),
        "the nap rung carries no identity, so nothing is current"
    );

    // The three pure latch hops of the record's own chain
    // (`OBJECTIVE10 ──WAKE──▶ OBJECTIVE49 ──NAP 5s──▶ OBJECTIVE50 ──WAKE──▶
    // OBJECTIVE11`) reach OBJECTIVE11, which becomes current and draws the
    // second message.
    engine.complete(9);
    engine.complete(48);
    engine.complete(49);
    let hud = ObjectiveHud::from_engine(&engine);
    let current = hud
        .current()
        .expect("OBJECTIVE11 woke through the record's latch hops");
    assert_eq!(current.objective, 10);
    assert_eq!((current.role_code, current.ordinal), (1, 2));
    let drawn = hud
        .current_text(Some(&catalog))
        .expect("the second objective's message resolves too");
    assert_eq!(drawn, expected(&catalog, "MSG_BRF_NWM1_OBJ2"));
    assert_ne!(
        drawn,
        expected(&catalog, "MSG_BRF_NWM1_OBJ1"),
        "the second objective draws the second text"
    );
    assert_ne!(
        drawn, "MSG_BRF_NWM1_OBJ2",
        "a resolved line is text, not the diagnostic key"
    );
}
