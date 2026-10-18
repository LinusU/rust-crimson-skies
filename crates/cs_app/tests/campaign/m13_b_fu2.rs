//! Acceptance stage M13-B-FU2 (Rally #1244): where each of M13's nine startup
//! animation names resolves, measured through production code.
//!
//! M13-B read the reader archive `ZBD/C3/M03/zrdr.zbd` and found a
//! `startanims.zrd` naming nine animations (eight under `NEW_GAME_START`, one
//! under `LOAD_GAME_START`) beside a `mis_anim.zrd` member that declares no
//! definition at all. It did not claim whether production binds the names. This
//! stage runs `bind_mission_animation` (the M01-LC machinery) over the scope
//! `zbd/c3/m03` and records one row per name: the declaring member(s), the
//! carrier record, and the refusals the production join spells.
//!
//! The synthetic member authors a startup table and definition members in the
//! measured `.zrd` shapes and runs them through the production readers and
//! `WorldActorProgramBinding::new`, so CI proves the three outcomes (single,
//! ambiguous, unresolved) are kept apart. The retail members are
//! `#[ignore = "requires CS_GAME_DIR"]`. Nothing here is `verified_original`.
//! The findings note is
//! `docs/findings/2026-10-18-m13-b-fu2-startup-animation-binding.md`.

use std::path::PathBuf;

use cs_app::animation::mission::bind_mission_animation;
use cs_app::animation::programs::{
    ANIMATION_DEFINITION_FIELD, ANIMATION_DEFINITIONS_RECORD, ANIMATION_LIST_FIELD,
    ANIMATION_NAME_FIELD, AnimationDefinitionSite, LOAD_GAME_START, NAME_FIELD, NEW_GAME_START,
    SEQUENCE_FIELD, SEQUENCE_NAME_FIELD, WorldActorProgramBinding,
    read_animation_definition_member, read_startup_animations,
};
use cs_app::animation::survey::CarrierKind;
use cs_content::campaign_bindings::{MissionLabel, mission_scoped_unknowns};
use cs_formats::script_raw::discover_container;
use cs_types::asset_id::SourceSpan;
use cs_types::evidence::ContentHash;
use cs_types::install::RelativePath;

/// The nine startup names, in stored order, with their event.
const NAMES: [(&str, &str); 9] = [
    (NEW_GAME_START, "place_tntboxes"),
    (NEW_GAME_START, "pzep_todrydock"),
    (NEW_GAME_START, "cgzepstate"),
    (NEW_GAME_START, "set_cggasbagstate"),
    (NEW_GAME_START, "cg1zep_engines_stop"),
    (NEW_GAME_START, "pzep_engines_start"),
    (NEW_GAME_START, "calldestroy_the_cargozep"),
    (NEW_GAME_START, "flag_state_pirate"),
    (LOAD_GAME_START, "player_setup"),
];

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: M13-B-FU2 needs the retail capability; run this suite with \
             `--include-ignored` and CS_GAME_DIR pointing at the read-only installation"
        )
    }))
}

// Synthetic `.zrd` authoring in the measured grammar (tag `3` text, tag `4`
// list, a record's root a one-element list holding a flat `KEY, value` body).

fn zrd_text(text: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

fn zrd_list(children: Vec<Vec<u8>>) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&4_u32.to_le_bytes());
    bytes.extend_from_slice(&((children.len() as u32) + 1).to_le_bytes());
    for child in children {
        bytes.extend_from_slice(&child);
    }
    bytes
}

fn zrd_flat(entries: Vec<(&str, Vec<u8>)>) -> Vec<u8> {
    let mut children = Vec::new();
    for (key, value) in entries {
        children.push(zrd_text(key));
        children.push(value);
    }
    zrd_list(children)
}

fn zrd_record(body: Vec<u8>) -> Vec<u8> {
    zrd_list(vec![body])
}

fn startup_member(events: &[(&str, &[&str])]) -> Vec<u8> {
    zrd_record(zrd_flat(
        events
            .iter()
            .map(|(event, names)| {
                (
                    *event,
                    zrd_list(names.iter().map(|name| zrd_text(name)).collect()),
                )
            })
            .collect(),
    ))
}

fn definition_member(names: &[&str]) -> Vec<u8> {
    let mut list = Vec::new();
    for name in names {
        list.push(zrd_text(ANIMATION_DEFINITION_FIELD));
        list.push(zrd_flat(vec![
            (ANIMATION_NAME_FIELD, zrd_text(name)),
            (NAME_FIELD, zrd_list(vec![zrd_text("actor")])),
            (
                SEQUENCE_FIELD,
                zrd_flat(vec![(SEQUENCE_NAME_FIELD, zrd_text("PLAY"))]),
            ),
        ]));
    }
    zrd_record(zrd_flat(vec![(
        ANIMATION_DEFINITIONS_RECORD,
        zrd_flat(vec![(ANIMATION_LIST_FIELD, zrd_list(list))]),
    )]))
}

fn sites(member: &str, names: &[&str]) -> Vec<AnimationDefinitionSite> {
    let read = read_animation_definition_member(member, &definition_member(names))
        .expect("the authored member reads");
    read.definitions()
        .iter()
        .map(|definition| {
            let span = SourceSpan::new(
                ContentHash::from_bytes([7; 32]),
                "zbd/synthetic/zrdr.zbd",
                Some(member),
                0,
                16,
                None,
            )
            .expect("the synthetic span is valid");
            AnimationDefinitionSite::new("zbd/synthetic/zrdr.zbd", member, span, definition.clone())
        })
        .collect()
}

/// **The production binding keeps a single declaration, an ambiguous one and
/// an undeclared name apart, per startup row.**
#[test]
fn accept_m13_b_fu2_synthetic_binding_keeps_single_ambiguous_and_unresolved_apart() {
    let table = read_startup_animations(&startup_member(&[
        (NEW_GAME_START, &["only_here", "declared_twice", "nowhere"]),
        (LOAD_GAME_START, &["only_here"]),
    ]))
    .expect("the authored startup table reads");
    let mut declarations = sites("a.zrd", &["only_here", "declared_twice"]);
    declarations.extend(sites("b.zrd", &["declared_twice"]));
    let binding = WorldActorProgramBinding::new(
        "zbd/synthetic/m00",
        vec!["zbd/synthetic/zrdr.zbd".to_owned()],
        vec!["startanims.zrd".to_owned()],
        table,
        &declarations,
    );
    let rows = binding.startup();
    assert_eq!(rows.len(), 4);
    assert_eq!(
        rows[0].resolution().single().map(|s| s.member()),
        Some("a.zrd")
    );
    let twice = rows[1].resolution().ambiguous().expect("two declare it");
    assert_eq!(
        twice.iter().map(|s| s.member()).collect::<Vec<_>>(),
        ["a.zrd", "b.zrd"]
    );
    assert!(rows[2].resolution().is_unresolved());
    assert_eq!(rows[3].event(), LOAD_GAME_START);
    assert!(rows[3].resolution().is_resolved());
}

/// **Against the owner's installation: the archive's startup table is the nine
/// names, and its `mis_anim.zrd` declares none of them.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m13_b_fu2_retail_startup_table_and_empty_mis_anim_member() {
    let container = "ZBD/C3/M03/zrdr.zbd";
    let bytes = std::fs::read(game_dir().join(container)).expect("the archive reads");
    let relative = RelativePath::new(&container.to_lowercase()).expect("relative");
    let discovery = discover_container(&relative.logical_key(), &relative, &bytes);
    let member = |name: &str| {
        discovery
            .programs()
            .iter()
            .find(|program| program.locator().member() == Some(name))
            .unwrap_or_else(|| panic!("the archive has {name}"))
            .bytes()
            .to_vec()
    };
    let table = read_startup_animations(&member("startanims.zrd")).expect("startanims reads");
    let stored: Vec<(String, String)> = table
        .events()
        .iter()
        .flat_map(|event| {
            event
                .animation_names()
                .iter()
                .map(|name| (event.event().to_owned(), name.clone()))
        })
        .collect();
    let expected: Vec<(String, String)> = NAMES
        .iter()
        .map(|(event, name)| ((*event).to_owned(), (*name).to_owned()))
        .collect();
    assert_eq!(stored, expected);

    let mis_anim = read_animation_definition_member("mis_anim.zrd", &member("mis_anim.zrd"))
        .expect("mis_anim.zrd reads");
    assert!(
        mis_anim.definitions().is_empty(),
        "M13's own mis_anim.zrd declares no definition"
    );
}

/// **Against the owner's installation: production binds eight of the nine names
/// to one declaring member and one carrier record each, and refuses the ninth
/// by name.**
///
/// The declarations are not in the archive's `mis_anim.zrd`; they are in
/// members of the mission archive, the `c3` group archive and the shared root
/// archive. `cg1zep_engines_stop` is declared by two root members, and no
/// measured rule picks one: production refuses it as ambiguous.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m13_b_fu2_retail_each_startup_name_resolves_or_is_refused_by_name() {
    let binding = bind_mission_animation(&game_dir(), "zbd/c3/m03")
        .expect("M13's animation closure reads through the production readers");
    // (name, declaring member, carrier, record index)
    let measured = [
        (
            "place_tntboxes",
            "zbd/c3/m03/zrdr.zbd:destroy_cargozep.zrd",
            CarrierKind::Mission,
            359,
        ),
        (
            "pzep_todrydock",
            "zbd/c3/m03/zrdr.zbd:destroy_cargozep.zrd",
            CarrierKind::Mission,
            358,
        ),
        (
            "cgzepstate",
            "zbd/c3/m03/zrdr.zbd:zepstate.zrd",
            CarrierKind::Mission,
            215,
        ),
        (
            "set_cggasbagstate",
            "zbd/zrdr.zbd:cg1_hawaii_set.zrd",
            CarrierKind::Mission,
            342,
        ),
        (
            "pzep_engines_start",
            "zbd/zrdr.zbd:pirate_zep_nacelles.zrd",
            CarrierKind::Mission,
            49,
        ),
        (
            "calldestroy_the_cargozep",
            "zbd/c3/m03/zrdr.zbd:destroy_cargozep.zrd",
            CarrierKind::Mission,
            361,
        ),
        (
            "flag_state_pirate",
            "zbd/c3/zrdr.zbd:flag_pirate.zrd",
            CarrierKind::Mission,
            1,
        ),
        (
            "player_setup",
            "zbd/zrdr.zbd:player_setup.zrd",
            CarrierKind::Camera,
            126,
        ),
    ];
    assert_eq!(binding.startup().len(), NAMES.len());
    for (name, member, carrier, index) in measured {
        let row = binding
            .startup()
            .iter()
            .find(|row| row.identity() == name)
            .unwrap_or_else(|| panic!("{name} is a startup row"));
        let site = row
            .declaration()
            .resolution()
            .single()
            .unwrap_or_else(|| panic!("{name} has exactly one declaring member"));
        assert_eq!(
            format!("{}:{}", site.archive(), site.member()),
            member,
            "{name}"
        );
        let record = row
            .bound_record()
            .unwrap_or_else(|| panic!("{name} binds a record: {:?}", row.record()));
        assert_eq!(
            (record.carrier(), record.index()),
            (carrier, index),
            "{name}"
        );
        assert!(row.is_playable(), "{name}: {:?}", row.refusals());
    }

    // The ninth: two members declare it, so production refuses to pick.
    let row = binding
        .startup()
        .iter()
        .find(|row| row.identity() == "cg1zep_engines_stop")
        .expect("cg1zep_engines_stop is a startup row");
    let sites = row
        .declaration()
        .resolution()
        .ambiguous()
        .expect("two members declare cg1zep_engines_stop");
    let members: Vec<&str> = sites.iter().map(|site| site.member()).collect();
    assert_eq!(members, ["cg1_zep_nacelles.zrd", "dante_zep_nacelles.zrd"]);
    assert!(sites.iter().all(|site| site.archive() == "zbd/zrdr.zbd"));
    let record = row.bound_record().expect("the carrier record is unique");
    assert_eq!(
        (record.carrier(), record.index()),
        (CarrierKind::Mission, 245)
    );
    assert!(!row.is_playable());
    assert_eq!(row.refusals().len(), 1, "{:?}", row.refusals());
    assert!(
        matches!(
            row.refusals()[0],
            cs_app::animation::mission::PlayRefusal::AmbiguousDeclaration { sites: 2, .. }
        ),
        "{:?}",
        row.refusals()
    );
}

/// **The surviving limitation reaches M13's record alone, and the committed
/// record carries it.**
#[test]
fn accept_m13_b_fu2_the_ambiguous_startup_name_is_an_unknown_of_m13_alone() {
    let m13 = mission_scoped_unknowns(&MissionLabel::new("M13").expect("label"));
    assert_eq!(m13.len(), 1);
    for needle in [
        "cg1zep_engines_stop",
        "cg1_zep_nacelles.zrd",
        "dante_zep_nacelles.zrd",
        "M13-C",
    ] {
        assert!(
            m13[0].contains(needle),
            "the unknown no longer names {needle:?}"
        );
    }
    assert!(
        mission_scoped_unknowns(&MissionLabel::new("M01").expect("label")).is_empty(),
        "a limitation measured for M13 must not reach a sibling"
    );
    let committed = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../missions/bindings/M13.json"),
    )
    .expect("missions/bindings/M13.json exists");
    assert!(
        committed.contains("startup animation `cg1zep_engines_stop`"),
        "the committed M13 record dropped the ambiguous-startup limitation"
    );
}
