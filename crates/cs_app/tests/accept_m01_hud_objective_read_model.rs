//! The HUD read model of task #1462 `M01-HUD-OBJECTIVE`: which objective the
//! lifecycle model marks current, and what the HUD draws for it.
//!
//! These tests are headless and need no installation: they drive the
//! production [`ObjectiveHud::from_engine`] projection over engine models
//! built through the production declarations
//! ([`EngineObjective::loaded`], [`EngineIdentity::from_record`],
//! [`TargetList::from_record`]) shaped the way M01's own record is shaped
//! (four dormant `PRIMARY` identities with messages, one `SECONDARY` without,
//! and an identity-less objective that self-wakes first —
//! `docs/findings/2026-10-12-m01-b-obj-2-objective-lifecycle-count-identity-cues.md`).
//!
//! The retail sibling, `accept_m01_hud_objective_retail.rs`, runs the same
//! projection over M01's real recovery and resolves the text through the
//! installation's own `strings.dll`.

use cs_app::ui::hud::{MissionMessages, ObjectiveHud};
use cs_content::objectives_engine::{
    DormantTimers, EngineDeclaration, EngineIdentity, EngineObjective, EngineObjectives, TargetList,
};
use cs_types::evidence::ContentHash;

/// The message names this projection is expected to carry, in objective
/// order, as M01 spells them (the `MSG_BRF_NWM1_OBJ*` elements).
const MESSAGES: [Option<&str>; 5] = [
    Some("MSG_A"),
    Some("MSG_B"),
    Some("MSG_C"),
    Some("MSG_D"),
    None,
];

/// One dormant objective with an optional `IDENTITY` and an optional `WAKE`
/// list, built exactly the way the loader builds it: `begin_dormant` present
/// means the objective loads [`EngineState::Dormant`], and the `-1` wake slot
/// is M01's own `BEGIN_DORMANT [-1]` spelling — the sentinel that never
/// self-wakes (the default `0.0` slot would wake on the first pass).
fn dormant(role: Option<(&str, u32, Option<&str>)>, wake: &[u32]) -> EngineDeclaration {
    EngineDeclaration {
        identity: role.map(|(role, ordinal, message)| {
            EngineIdentity::from_record(role, ordinal, message.map(str::to_owned))
        }),
        begin_dormant: Some(DormantTimers {
            wake_at: -1.0,
            ..DormantTimers::DEFAULT
        }),
        wake: TargetList::from_record(wake).expect("the wake list fits the engine's window"),
        ..EngineDeclaration::default()
    }
}

/// Twenty-one objectives laid out like M01's identity blocks: an
/// identity-less block that self-wakes at two seconds (OBJECTIVE1), then the
/// four primaries (OBJECTIVE9/11/15/18) and the message-less secondary
/// (OBJECTIVE21), all dormant, with empty dormant blocks between them.
fn m01_shaped() -> EngineObjectives {
    let mut declarations = vec![EngineDeclaration::default(); 21];
    declarations[0] = EngineDeclaration {
        begin_dormant: Some(DormantTimers {
            wake_at: 2.0,
            ..DormantTimers::DEFAULT
        }),
        ..EngineDeclaration::default()
    };
    // OBJECTIVE9 wakes OBJECTIVE11 when it completes — the first step of the
    // chain the M01 wake finding records; the rest of the chain (the nap of
    // OBJECTIVE10 and its own wakes) is the record's, not this fixture's.
    declarations[8] = dormant(Some(("PRIMARY", 1, Some("MSG_A"))), &[11]);
    declarations[10] = dormant(Some(("PRIMARY", 2, Some("MSG_B"))), &[]);
    declarations[14] = dormant(Some(("PRIMARY", 3, Some("MSG_C"))), &[]);
    declarations[17] = dormant(Some(("PRIMARY", 4, Some("MSG_D"))), &[]);
    declarations[20] = dormant(Some(("SECONDARY", 11, None)), &[]);
    let objectives = declarations
        .into_iter()
        .map(EngineObjective::loaded)
        .collect();
    EngineObjectives::new(objectives).expect("every target is in range")
}

#[test]
fn accept_m01_hud_objective_read_model_lists_every_identity_in_objective_order() {
    let engine = m01_shaped();
    let hud = ObjectiveHud::from_engine(&engine);
    assert!(!hud.is_empty());
    let rows = hud.rows();
    assert_eq!(
        rows.len(),
        5,
        "five identity rows: four primaries, one secondary"
    );
    let expected = [
        (8usize, 1u32, 1u32),
        (10, 1, 2),
        (14, 1, 3),
        (17, 1, 4),
        (20, 2, 11),
    ];
    for (row, (objective, role_code, ordinal)) in rows.iter().zip(expected) {
        assert_eq!(row.objective, objective, "row order is objective order");
        assert_eq!(
            row.role_code,
            role_code,
            "OBJECTIVE{} role code",
            objective + 1
        );
        assert_eq!(row.ordinal, ordinal, "OBJECTIVE{} ordinal", objective + 1);
        assert!(!row.live, "every identity loads dormant in this fixture");
        assert!(!row.completed);
    }
    assert_eq!(
        rows.iter()
            .map(|row| row.message.as_deref())
            .collect::<Vec<_>>(),
        MESSAGES.to_vec(),
        "the message element travels verbatim, and the message-less secondary keeps none"
    );
    // Nothing is awake, so the model marks nothing current — the HUD shows
    // no line rather than a guess.
    assert!(hud.current().is_none());
    assert!(hud.current_text(None).is_none());
}

#[test]
fn accept_m01_hud_objective_read_model_follows_the_lifecycle_model() {
    let mut engine = m01_shaped();

    // The identity-less first block wakes at two seconds: the model marks
    // *something* awake, but it declares no identity, so no line appears.
    engine.advance(3.0);
    let hud = ObjectiveHud::from_engine(&engine);
    assert!(hud.current().is_none());

    // OBJECTIVE9 wakes: it carries `PRIMARY 1` and its message, so it is the
    // current objective now.
    engine.wake(&[8]);
    let hud = ObjectiveHud::from_engine(&engine);
    let current = hud.current().expect("OBJECTIVE9 is awake and uncompleted");
    assert_eq!(current.objective, 8);
    assert_eq!((current.role_code, current.ordinal), (1, 1));
    assert_eq!(current.message.as_deref(), Some("MSG_A"));
    assert!(current.live);
    // No catalog was supplied, so the line is the *key* — the diagnostic key
    // spec F12 asks for — never an invented caption.
    assert_eq!(hud.current_text(None).as_deref(), Some("MSG_A"));

    // A lower-ranked secondary waking does not displace the primary: the rank
    // is (role code, ordinal, index), measured from the identity the record
    // declares.
    engine.wake(&[20]);
    let hud = ObjectiveHud::from_engine(&engine);
    assert_eq!(hud.current().map(|row| row.objective), Some(8));

    // Completing OBJECTIVE9 runs its measured effects: it becomes completed
    // (so the HUD stops showing it) and its `WAKE` list wakes OBJECTIVE11,
    // which becomes the current objective.
    engine.complete(8);
    let hud = ObjectiveHud::from_engine(&engine);
    assert!(!hud.rows()[0].live);
    assert!(hud.rows()[0].completed);
    let current = hud
        .current()
        .expect("OBJECTIVE11 woke on OBJECTIVE9's completion");
    assert_eq!(current.objective, 10);
    assert_eq!((current.role_code, current.ordinal), (1, 2));
    assert_eq!(current.message.as_deref(), Some("MSG_B"));

    // Complete everything with an identity and the model marks nothing
    // current again — the line disappears instead of showing a stale one.
    engine.complete(10);
    engine.wake(&[14]);
    engine.complete(14);
    engine.wake(&[17]);
    engine.complete(17);
    engine.wake(&[20]);
    engine.complete(20);
    let hud = ObjectiveHud::from_engine(&engine);
    assert!(hud.current().is_none());
    assert!(hud.current_text(None).is_none());
    assert_eq!(
        hud.rows().iter().filter(|row| row.completed).count(),
        5,
        "every identity row is still reported; only the line hides"
    );
}

#[test]
fn accept_m01_hud_objective_read_model_of_a_record_without_identity_is_empty() {
    let engine = EngineObjectives::new(vec![
        EngineObjective::loaded(EngineDeclaration {
            begin_dormant: Some(DormantTimers::DEFAULT),
            ..EngineDeclaration::default()
        });
        3
    ])
    .expect("every target is in range");
    let hud = ObjectiveHud::from_engine(&engine);
    // No identity is not the same as "nothing current": the rows are absent,
    // and a consumer can tell the two apart.
    assert!(hud.is_empty());
    assert!(hud.rows().is_empty());
    assert!(hud.current().is_none());
    assert!(hud.current_text(None).is_none());
}

#[test]
fn accept_m01_hud_objective_messages_absent_keeps_why_it_is_absent() {
    // A root with no `strings.dll`: the resource reports the absence with its
    // reason instead of dropping it, which is what lets a test — and a
    // reader of the drawn key — tell "not loaded" from "loaded and empty".
    let root = std::env::temp_dir().join("cs-m01-hud-objective-no-installation");
    let messages = MissionMessages::read(&root, ContentHash::from_bytes([0; 32]));
    assert!(messages.catalog().is_none());
    let reason = messages.missing().expect("the reason is kept");
    assert!(
        reason.contains("strings.dll"),
        "the reason names the file it could not read: {reason}"
    );
}
