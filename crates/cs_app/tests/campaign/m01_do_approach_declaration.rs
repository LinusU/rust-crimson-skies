//! `ZBD/C3/M01`'s `do_approach1..6` landing records are declared (#2098).
//! Task test prefix: `accept_m01_do_approach_declaration_`.
//!
//! The six records carry no `ANIMATION_NAME`; their `ANIMATION_DEFINITION`s
//! name one object, `do_approachN`, and the compiled record stores that name as
//! both its `anim_name` and its `object_name`. See
//! `docs/findings/2026-10-24-m01-do-approach-declaration.md`.

use bevy::prelude::{Quat, Vec3};
use cs_app::animation::mission::{NamedAnimationResolution, bind_mission_animation};
use cs_app::animation::programs::{
    ANIMATION_DEFINITION_FIELD, ANIMATION_DEFINITIONS_RECORD, ANIMATION_LIST_FIELD,
    ANIMATION_NAME_FIELD, AnimationDefinitionSite, BindingResolution, NAME_FIELD,
    read_animation_definition_member, resolve_animation,
};
use cs_app::mission_animations::ActivationOutcome;
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{MissionHost, build_headless, stage_for, teardown};
use cs_script::runtime::ObjectiveLifecycle;
use cs_sim::mission::{BlockLifecycleTable, LifecycleDecl};
use cs_types::asset_id::SourceSpan;
use cs_types::evidence::ContentHash;

use crate::campaign::common::label;
use crate::campaign::m01_play_triggers_anims_land::{
    Body, composed_node_position, observed_state, pump,
};
use crate::campaign::vs_m01_rt_window::game_dir;

fn text(text: &str) -> Vec<u8> {
    let mut bytes = 3_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

fn list(children: Vec<Vec<u8>>) -> Vec<u8> {
    let mut bytes = 4_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&((children.len() as u32) + 1).to_le_bytes());
    children.into_iter().for_each(|child| bytes.extend(child));
    bytes
}

fn flat(entries: Vec<(&str, Vec<u8>)>) -> Vec<u8> {
    list(
        entries
            .into_iter()
            .flat_map(|(key, value)| [text(key), value])
            .collect(),
    )
}

/// One authored definition per `(animation_name, object names)` pair, read
/// through the production definition reader.
fn sites(definitions: &[(Option<&str>, &[&str])]) -> Vec<AnimationDefinitionSite> {
    let mut children = Vec::new();
    for (animation, objects) in definitions {
        let mut fields = Vec::new();
        if let Some(animation) = animation {
            fields.push((ANIMATION_NAME_FIELD, text(animation)));
        }
        fields.push((
            NAME_FIELD,
            list(objects.iter().map(|object| text(object)).collect()),
        ));
        children.push(text(ANIMATION_DEFINITION_FIELD));
        children.push(flat(fields));
    }
    let member = list(vec![flat(vec![(
        ANIMATION_DEFINITIONS_RECORD,
        flat(vec![(ANIMATION_LIST_FIELD, list(children))]),
    )])]);
    let read = read_animation_definition_member("synthetic.zrd", &member).expect("member reads");
    let span = SourceSpan::new(
        ContentHash::from_bytes([7; 32]),
        "zbd/synthetic/zrdr.zbd",
        Some("synthetic.zrd"),
        0,
        1,
        None,
    )
    .expect("span");
    read.definitions()
        .iter()
        .map(|definition| {
            AnimationDefinitionSite::new(
                "zbd/synthetic/zrdr.zbd",
                "synthetic.zrd",
                span.clone(),
                definition.clone(),
            )
        })
        .collect()
}

/// **A definition without `ANIMATION_NAME` that names one literal object
/// declares the animation spelled like that object, and only when nothing
/// states the name explicitly.**
#[test]
fn accept_m01_do_approach_declaration_an_unnamed_single_object_definition_declares_its_object() {
    let all = sites(&[
        (Some("enable"), &["node_a"]),
        (None, &["node_a"]),
        (None, &["pair_a", "pair_b"]),
        (None, &["wild_*"]),
    ]);
    // Only the unnamed single-object definition declares `node_a`.
    match resolve_animation("node_a", &all) {
        BindingResolution::Single(site) => assert_eq!(site.definition().index(), 1),
        other => panic!("{other:?}"),
    }
    // An explicit ANIMATION_NAME wins over the fallback.
    assert!(matches!(
        resolve_animation("enable", &all),
        BindingResolution::Single(site) if site.definition().index() == 0
    ));
    // Several objects and wildcards declare no single name.
    assert!(matches!(
        resolve_animation("pair_a", &all),
        BindingResolution::Unresolved
    ));
    assert!(matches!(
        resolve_animation("wild_*", &all),
        BindingResolution::Unresolved
    ));
    // Two unnamed definitions of one object stay ambiguous.
    let twice = sites(&[(None, &["node_a"]), (None, &["node_a"])]);
    assert!(matches!(
        resolve_animation("node_a", &twice),
        BindingResolution::Ambiguous(sites) if sites.len() == 2
    ));
}

/// **C3/M01: all six `do_approachN` names bind a declaring member with every
/// agreement holding and no refusal.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_do_approach_declaration_the_six_landing_records_bind_a_declaring_member() {
    let binding = bind_mission_animation(&game_dir(), "zbd/c3/m01").expect("C3/M01 binds");
    for n in 1..=6 {
        let name = format!("do_approach{n}");
        let NamedAnimationResolution::Bound(row) = binding.animation_by_name(&name) else {
            panic!("{name} does not bind a record");
        };
        let site = row
            .declaration()
            .resolution()
            .single()
            .unwrap_or_else(|| panic!("{name} has no single declaring site"));
        assert_eq!(site.archive(), "zbd/c3/m01/zrdr.zbd", "{name}");
        assert_eq!(
            site.animation_name(),
            None,
            "{name} states no ANIMATION_NAME"
        );
        assert!(row.refusals().is_empty(), "{name}: {:?}", row.refusals());
        assert!(row.bound_record().is_some(), "{name}");
    }
}

/// **A landing at `do_approach1`, through the production landing path, starts
/// its record, its `CALL_ANIMATION texdrop` runs, and OBJECTIVE21 completes.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_do_approach_declaration_a_landing_starts_the_record_and_texdrop_completes_objective21()
 {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure").expect("plans");
    let mut stage = stage_for(&root, &plan).expect("stage reads");
    // OBJECTIVE21 (block index 20): its wake effect is `enable_dropoff`.
    let mut table = BlockLifecycleTable::new();
    table
        .declare(20, LifecycleDecl::awake())
        .expect("declared once");
    stage.host.block_lifecycles = table;
    stage.host.engine_objectives = None;
    let mut app = build_headless(&stage).expect("M01 composes");

    pump(&mut app, 120, None);
    assert!(
        app.world()
            .resource::<MissionHost>()
            .landings()
            .activated()
            .is_empty(),
        "no landing runs nothing"
    );

    let position = composed_node_position(&app, "do_approach1");
    let landed = Body {
        position: Some(position),
        velocity: Vec3::new(100.0, 0.0, 0.0),
        rotation: Quat::IDENTITY,
    };
    pump(&mut app, 30, Some(&landed));
    {
        let host = app.world().resource::<MissionHost>();
        let activated = host.landings().activated();
        let row = activated
            .iter()
            .find(|activation| activation.node == "do_approach1")
            .unwrap_or_else(|| panic!("no do_approach1 activation: {activated:?}"));
        assert_eq!(row.anim, "do_approach1");
        assert_eq!(row.outcome, ActivationOutcome::Started, "{row:?}");
        assert!(
            host.call_animations()
                .activations()
                .iter()
                .any(|call| call.name() == "texdrop"
                    && call.caller() == "do_approach1"
                    && call.outcome() == ActivationOutcome::Started),
            "texdrop was called by the record: {:?} / {:?}",
            host.call_animations().activations(),
            host.call_animations().refused()
        );
    }

    let mut executed = false;
    for _ in 0..60 {
        pump(&mut app, 60, Some(&landed));
        if observed_state(&app, "texdrop") == Some(3) {
            executed = true;
            break;
        }
    }
    assert!(
        executed,
        "texdrop EXECUTED: {:?}",
        observed_state(&app, "texdrop")
    );
    pump(&mut app, 120, Some(&landed));
    assert_eq!(
        app.world().resource::<MissionHost>().lifecycles().state(20),
        Some(ObjectiveLifecycle::Done),
        "OBJECTIVE21 completes on texdrop EXECUTED"
    );
    teardown(&mut app);
}
