//! Task #2204 (`T452-REG-MANEUVER-CHOOSER`): the AI maneuver chooser and the
//! tangled-actor nitro gate. Task test prefix: `accept_t452_maneuver_`.
//!
//! The synthetic tests author their own catalog. The ignored retail test lowers
//! the installation's `maneuvers.zrd` and runs the same production chooser over
//! it; it reads `$CS_GAME_DIR` (never writes it) and fails loudly without it.
//! Nothing here is `verified_original`.

use cs_app::ai::maneuver::{
    CatalogError, ChooserActor, MANEUVER_NAMES, ManeuverCatalog, ManeuverGate, ManeuverRecord,
    candidates, choose, choose_named, gate,
};
use cs_formats::ParseContext;
use cs_formats::zbd::{
    ManeuverNode, ZbdProbe, dispatch, read_maneuver_nodes, read_reader_archive, read_reader_nodes,
    read_version_one_index,
};
use cs_types::install::RelativePath;

fn record(natural_touch: i32, nitro: bool, weight: f32) -> ManeuverRecord {
    ManeuverRecord {
        natural_touch,
        relative: false,
        nitro,
        autogyro_allowed: true,
        weight,
        step_rows: 1,
    }
}

/// Slot 0 and 2 plain, slot 1 and 3 `nitro`; all offered at skill 0.
fn catalog() -> ManeuverCatalog {
    ManeuverCatalog::from_records(vec![
        record(0, false, 0.0),
        record(0, true, 0.0),
        record(0, false, 0.0),
        record(0, true, 0.0),
    ])
}

fn rolls() -> impl Iterator<Item = u16> {
    (0..=32_767u16).step_by(97)
}

#[test]
fn accept_t452_maneuver_tangled_actor_is_never_offered_a_nitro_maneuver() {
    let catalog = catalog();
    let mut actor = ChooserActor::new(5);
    actor.has_injector = true;

    // Untangled with the injector: all four are offered, nitro included.
    let all: Vec<usize> = candidates(&catalog, &actor)
        .iter()
        .map(|c| c.slot)
        .collect();
    assert_eq!(all, [0, 1, 2, 3]);

    actor.tangled = true;
    let offered: Vec<usize> = candidates(&catalog, &actor)
        .iter()
        .map(|c| c.slot)
        .collect();
    assert_eq!(offered, [0, 2]);
    assert_eq!(
        gate(&catalog.record(1).unwrap().clone(), &actor),
        ManeuverGate::NitroWhileTangled
    );

    // Every possible draw lands on a plain maneuver, and never asks for the injector.
    for roll in rolls().chain([32_767]) {
        let mut tangled = actor.clone();
        let choice = choose(&catalog, &mut tangled, roll).expect("plain maneuvers remain");
        assert!(
            [0, 2].contains(&choice.slot),
            "roll {roll} chose {}",
            choice.slot
        );
        assert!(!choice.injector_enabled);
        assert_eq!(tangled.current, Some(choice.slot));
    }
}

#[test]
fn accept_t452_maneuver_nitro_is_reachable_when_not_tangled_and_asks_for_the_injector() {
    let catalog = catalog();
    let mut actor = ChooserActor::new(5);
    actor.has_injector = true;
    let mut chosen_nitro = false;
    for roll in rolls() {
        let mut fresh = actor.clone();
        let choice = choose(&catalog, &mut fresh, roll).unwrap();
        let nitro = catalog.record(choice.slot).unwrap().nitro;
        assert_eq!(choice.injector_enabled, nitro);
        chosen_nitro |= nitro;
    }
    assert!(
        chosen_nitro,
        "an untangled actor with the injector can draw nitro"
    );

    // Without the injector a nitro maneuver is gated out exactly like a tangle.
    actor.has_injector = false;
    assert_eq!(
        gate(catalog.record(1).unwrap(), &actor),
        ManeuverGate::NitroWithoutInjector
    );
    let offered: Vec<usize> = candidates(&catalog, &actor)
        .iter()
        .map(|c| c.slot)
        .collect();
    assert_eq!(offered, [0, 2]);
}

#[test]
fn accept_t452_maneuver_all_gated_out_keeps_the_current_maneuver() {
    let catalog = ManeuverCatalog::from_records(vec![record(0, true, 0.0), record(0, true, 0.0)]);
    let mut actor = ChooserActor::new(9);
    actor.has_injector = true;
    actor.tangled = true;
    actor.current = Some(1);
    actor.previous = Some(0);
    let before = actor.clone();
    assert_eq!(choose(&catalog, &mut actor, 12_345), None);
    assert_eq!(actor, before);
}

#[test]
fn accept_t452_maneuver_gates_run_autogyro_then_skill_then_nitro() {
    let mut actor = ChooserActor::new(3);
    actor.has_injector = true;
    actor.tangled = true;
    let mut weak = record(4, true, 0.0);
    weak.autogyro_allowed = false;
    // Everything fails; the first gate in the measured order is named.
    actor.autogyro_restricted = true;
    assert_eq!(gate(&weak, &actor), ManeuverGate::NotAutogyroAllowed);
    actor.autogyro_restricted = false;
    assert_eq!(gate(&weak, &actor), ManeuverGate::SkillTooLow);
    weak.natural_touch = 3; // equal to the skill level passes the skill gate
    assert_eq!(gate(&weak, &actor), ManeuverGate::NitroWhileTangled);
    // The skill level is clamped to 0..=9.
    assert_eq!(ChooserActor::new(40).skill_level(), 9);
    assert_eq!(ChooserActor::new(-2).skill_level(), 0);
}

#[test]
fn accept_t452_maneuver_weights_follow_the_measured_factors() {
    let catalog = ManeuverCatalog::from_records(vec![
        record(0, false, 1.0),
        record(0, false, 1.0),
        record(0, false, 1.0),
        record(0, false, 1.0),
        record(0, false, -5.0),
    ]);
    let mut actor = ChooserActor::new(0);
    actor.current = Some(0);
    actor.previous = Some(1);
    actor.preferred = vec![2];
    let weights: Vec<f32> = candidates(&catalog, &actor)
        .iter()
        .map(|c| c.weight)
        .collect();
    // (1.0 + 1.0) × 0.1, × 0.2, × 6.0, unchanged; a negative weight becomes 0.1.
    let expected = [0.2f32, 0.4, 12.0, 2.0, 0.1];
    for (got, want) in weights.iter().zip(expected) {
        assert!((got - want).abs() < 1e-5, "{got} vs {want}");
    }
}

#[test]
fn accept_t452_maneuver_named_path_bypasses_the_gates_but_not_the_injector_refusal() {
    let catalog = catalog();
    let mut actor = ChooserActor::new(0);
    actor.tangled = true;
    let choice = choose_named(&catalog, &mut actor, 1).expect("slot 1 exists");
    assert_eq!(choice.slot, 1);
    assert!(!choice.injector_enabled, "the setter refuses while tangled");
    assert_eq!(choose_named(&catalog, &mut actor, 4), None);
    assert_eq!((actor.current, actor.previous), (Some(1), None));
}

#[test]
fn accept_t452_maneuver_catalog_places_nodes_by_name_in_engine_order() {
    let node = |name| ManeuverNode {
        name,
        natural_touch: 0x63,
        has_natural_touch: false,
        bias: 0.5,
        relative: false,
        nitro: name == "nitro_evade",
        autogyro_allowed: false,
        steps: vec![vec![0.0; 4]],
    };
    // The file lists nitro_evade first; the engine's slot for it is 14.
    let mut nodes: Vec<ManeuverNode<'_>> = vec![node("nitro_evade")];
    nodes.extend(
        MANEUVER_NAMES
            .iter()
            .filter(|n| **n != "nitro_evade")
            .map(|n| node(n)),
    );
    let catalog = ManeuverCatalog::from_nodes(&nodes).expect("seventeen names");
    assert_eq!(catalog.len(), 17);
    assert!(catalog.record(14).unwrap().nitro);
    assert!(!catalog.record(0).unwrap().nitro);
    assert_eq!(catalog.record(0).unwrap().weight, 0.5);

    assert_eq!(
        ManeuverCatalog::from_nodes(&nodes[1..]),
        Err(CatalogError::Missing("nitro_evade"))
    );
    nodes.push(node("roll"));
    assert_eq!(
        ManeuverCatalog::from_nodes(&nodes),
        Err(CatalogError::Duplicate("roll".into()))
    );
    assert_eq!(
        ManeuverCatalog::from_nodes(&[node("hover")]),
        Err(CatalogError::Unknown("hover".into()))
    );
}

/// The installation's catalog through the same chooser: a tangled expert actor
/// is never given a `nitro` maneuver, an untangled one with the injector can be.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_maneuver_retail_catalog_never_offers_nitro_to_a_tangled_actor() {
    let root = std::env::var_os("CS_GAME_DIR")
        .expect("CS_GAME_DIR is not set: this test needs the original installation");
    let spelling = "ZBD/zrdr.zbd";
    let bytes = std::fs::read(std::path::Path::new(&root).join(spelling)).expect("zrdr.zbd");
    let relative = RelativePath::new(spelling).expect("path");
    let probe = &bytes[..bytes.len().min(64)];
    let decided = dispatch(ZbdProbe::new(spelling, &relative, probe)).expect("dispatches");
    let mut context = ParseContext::with_defaults(spelling);
    let index = read_version_one_index(&mut context, decided, &bytes).expect("index");
    let table = index.member_table();
    let archive = read_reader_archive(&mut context, &table, index.data()).expect("archive");
    let entry = archive
        .entries()
        .find(|entry| entry.name() == b"maneuvers.zrd")
        .expect("maneuvers.zrd");
    let nodes = read_reader_nodes(entry.content()).expect("decodes");
    let catalog = ManeuverCatalog::from_nodes(&read_maneuver_nodes(&nodes).expect("lowers"))
        .expect("the seventeen engine slots");
    let nitro_slots: Vec<usize> = (0..catalog.len())
        .filter(|slot| catalog.record(*slot).unwrap().nitro)
        .collect();
    assert!(!nitro_slots.is_empty());

    let mut actor = ChooserActor::new(9);
    actor.has_injector = true;
    let untangled: Vec<usize> = candidates(&catalog, &actor)
        .iter()
        .map(|c| c.slot)
        .collect();
    assert!(nitro_slots.iter().any(|slot| untangled.contains(slot)));
    actor.tangled = true;
    let tangled: Vec<usize> = candidates(&catalog, &actor)
        .iter()
        .map(|c| c.slot)
        .collect();
    assert!(!tangled.is_empty());
    assert!(nitro_slots.iter().all(|slot| !tangled.contains(slot)));
    for roll in rolls() {
        let mut fresh = actor.clone();
        let choice = choose(&catalog, &mut fresh, roll).expect("non-nitro remain");
        assert!(!nitro_slots.contains(&choice.slot));
    }
}
