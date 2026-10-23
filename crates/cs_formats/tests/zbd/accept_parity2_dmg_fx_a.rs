//! Acceptance for task #1805 (`PARITY2-DMG-FX-A`): the fire, smoke and flak
//! effect definitions of the reader archive, measured from the retail
//! installation (`docs/findings/2026-10-21-parity2-dmg-fx-a-effect-definitions.md`).
//!
//! The synthetic tests author every byte they read. The retail tests read
//! `$CS_GAME_DIR` (never write it) and fail loudly without it. Task test
//! prefix: `accept_parity2_dmg_fx_a_`.

use std::fs;

use cs_formats::ParseContext;
use cs_formats::zbd::{
    EffectAnimation, EffectDefError, EffectEvent, PufferState, ReaderNode, ReaderTokenError,
    ZbdProbe, dispatch, read_animation_definitions, read_reader_archive, read_reader_node,
    read_reader_nodes, read_vehicle_records, read_version_one_index,
};

use super::t340::{game_dir, path};

// --- Authored bytes -----------------------------------------------------------

fn int(value: i32) -> Vec<u8> {
    [1u32.to_le_bytes(), value.to_le_bytes()].concat()
}

fn float(value: f32) -> Vec<u8> {
    [2u32.to_le_bytes(), value.to_le_bytes()].concat()
}

fn text(value: &str) -> Vec<u8> {
    let mut out = 3u32.to_le_bytes().to_vec();
    out.extend((value.len() as u32).to_le_bytes());
    out.extend(value.as_bytes());
    out
}

/// A list of `children`, stored with the measured word: children plus one.
fn list(children: &[Vec<u8>]) -> Vec<u8> {
    let mut out = 4u32.to_le_bytes().to_vec();
    out.extend((children.len() as u32 + 1).to_le_bytes());
    for child in children {
        out.extend(child);
    }
    out
}

/// `keyword (values...)`, the flat sibling pair every record is written as.
fn field(keyword: &str, values: &[Vec<u8>]) -> Vec<Vec<u8>> {
    vec![text(keyword), list(values)]
}

fn fields(parts: &[Vec<Vec<u8>>]) -> Vec<u8> {
    list(&parts.concat())
}

fn authored_member(definition: Vec<u8>) -> Vec<u8> {
    let animation_list = list(&[text("ANIMATION_DEFINITION"), definition]);
    let definitions = list(&[text("ANIMATION_LIST"), animation_list]);
    list(&[list(&[text("ANIMATION_DEFINITIONS"), definitions])])
}

fn authored_puffer() -> Vec<u8> {
    fields(&[
        field("NAME", &[text("testpuffer")]),
        field("ACTIVE_STATE", &[text("ACTIVE")]),
        field("PRIORITY", &[int(6)]),
        field("AT_NODE", &[text("INPUT_NODE"), int(0), int(0), float(0.5)]),
        field("DISTANCE_INTERVAL", &[float(0.75)]),
        field(
            "MIN_RANDOM_VELOCITY",
            &[float(-1.5), float(-2.5), float(-3.5)],
        ),
        field("SIZE_RANGE", &[float(0.25), int(2)]),
        field("FADE_RANGE", &[int(700), int(800)]),
        field("TEXTURES", &[text("a01"), text("a02")]),
        field(
            "COLORS",
            &[list(&[int(0), int(255), int(128), int(64), float(0.5)])],
        ),
        field("START_TIME", &[text("EVENT_OFFSET"), float(0.3)]),
        field("MYSTERY", &[int(1)]),
    ])
}

fn authored_animation() -> Vec<u8> {
    let sequence = list(&[
        text("NAME"),
        list(&[text("seq")]),
        text("PUFFER_STATE"),
        authored_puffer(),
        text("CALL_ANIMATION"),
        fields(&[
            field("NAME", &[text("dense_firetrail")]),
            field("LOCAL_NAME", &[text("local_dft")]),
            field("WITH_NODE", &[text("prop1")]),
        ]),
        text("LOOP"),
        list(&[text("LOOP_COUNT"), list(&[int(-1)])]),
    ]);
    authored_member(fields(&[
        field("NAME", &[text("host")]),
        field("ANIMATION_NAME", &[text("test_anim")]),
        field("ACTIVATION", &[text("ON_CALL")]),
        field("RESET_TIME", &[int(-1)]),
        vec![text("SEQUENCE_DEFINITION"), sequence],
    ]))
}

// --- Synthetic ---------------------------------------------------------------

#[test]
fn accept_parity2_dmg_fx_a_the_token_grammar_is_int_float_string_and_counted_list() {
    let bytes = list(&[text("name"), int(-3), float(1.5), list(&[])]);
    let (node, used) = read_reader_node(&bytes).expect("decodes");
    assert_eq!(used, bytes.len(), "every byte is consumed");
    let ReaderNode::List(children) = node else {
        panic!("a list");
    };
    assert_eq!(children.len(), 4, "a list word of N holds N - 1 children");
    assert_eq!(children[0].as_str(), Some("name"));
    assert_eq!(children[1].as_int(), Some(-3));
    assert_eq!(children[2].as_number(), Some(1.5));
    assert_eq!(children[3].as_list().map(<[_]>::len), Some(0));
}

#[test]
fn accept_parity2_dmg_fx_a_the_token_reader_refuses_what_it_cannot_bound() {
    assert_eq!(
        read_reader_node(&99u32.to_le_bytes().repeat(2)),
        Err(ReaderTokenError::UnknownTag { offset: 0, tag: 99 })
    );
    let mut truncated = text("abcdef");
    truncated.truncate(truncated.len() - 2);
    assert_eq!(
        read_reader_node(&truncated),
        Err(ReaderTokenError::LengthOutOfBounds { offset: 0 })
    );
    let zero_word = [4u32.to_le_bytes(), 0u32.to_le_bytes()].concat();
    assert_eq!(
        read_reader_node(&zero_word),
        Err(ReaderTokenError::EmptyListWord { offset: 0 })
    );
    // A list claiming a billion children in eight bytes is refused before any
    // allocation.
    let hostile = [4u32.to_le_bytes(), 1_000_000_000u32.to_le_bytes()].concat();
    assert_eq!(
        read_reader_node(&hostile),
        Err(ReaderTokenError::LengthOutOfBounds { offset: 0 })
    );
    let mut nested = list(&[]);
    for _ in 0..64 {
        nested = list(&[nested]);
    }
    assert!(matches!(
        read_reader_node(&nested),
        Err(ReaderTokenError::TooDeep { .. })
    ));
    assert_eq!(
        read_reader_nodes(&[1, 0, 0, 0]),
        Err(ReaderTokenError::UnexpectedEnd { offset: 4 })
    );
}

#[test]
fn accept_parity2_dmg_fx_a_a_puffer_event_is_typed_field_by_field() {
    let bytes = authored_animation();
    let nodes = read_reader_nodes(&bytes).expect("decodes");
    let animations = read_animation_definitions(&nodes).expect("typed");
    let [animation] = animations.as_slice() else {
        panic!("one animation");
    };
    assert_eq!(animation.id(), "test_anim");
    assert_eq!(animation.name, "host");
    assert_eq!(animation.activation, Some("ON_CALL"));
    assert_eq!(animation.reset_time, [-1.0]);
    let puffer = animation.puffers().next().expect("one puffer");
    assert_eq!(puffer.name, "testpuffer");
    assert_eq!(puffer.active, Some(true));
    assert_eq!(puffer.priority, Some(6.0));
    let at = puffer.at_node.as_ref().expect("anchor");
    assert_eq!(
        (at.node, at.words.as_slice()),
        ("INPUT_NODE", [0.0, 0.0, 0.5].as_slice())
    );
    assert_eq!(puffer.distance_interval, Some(0.75));
    assert_eq!(puffer.min_random_velocity, Some([-1.5, -2.5, -3.5]));
    assert_eq!(
        puffer.size_range,
        Some([0.25, 2.0]),
        "an int reads as a number"
    );
    assert_eq!(puffer.fade_range, Some([700.0, 800.0]));
    assert_eq!(puffer.textures, ["a01", "a02"]);
    assert_eq!(puffer.colors, [[0.0, 255.0, 128.0, 64.0, 0.5]]);
    assert_eq!(puffer.start_time, Some(("EVENT_OFFSET", 0.3)));
    assert_eq!(
        puffer.other_fields,
        ["MYSTERY"],
        "a keyword no field reads is listed, not dropped"
    );
    let call = animation.calls().next().expect("one call");
    assert_eq!(
        (call.name, call.local_name, call.with_node),
        ("dense_firetrail", Some("local_dft"), Some("prop1"))
    );
    assert!(
        animation.sequences[0]
            .events
            .iter()
            .any(|event| matches!(event, EffectEvent::Other { keyword, .. } if *keyword == "LOOP"))
    );
}

#[test]
fn accept_parity2_dmg_fx_a_a_malformed_field_is_refused_with_its_keyword() {
    let broken = authored_member(fields(&[
        field("NAME", &[text("host")]),
        vec![
            text("SEQUENCE_DEFINITION"),
            list(&[
                text("PUFFER_STATE"),
                fields(&[field("SIZE_RANGE", &[float(1.0)])]),
            ]),
        ],
    ]));
    let nodes = read_reader_nodes(&broken).expect("decodes");
    assert_eq!(
        read_animation_definitions(&nodes),
        Err(EffectDefError::FieldShape {
            keyword: "SIZE_RANGE".to_owned(),
            expected: "two numbers"
        })
    );
    let not_bytes = text("x");
    let not_animation = read_reader_nodes(&not_bytes).expect("decodes");
    assert_eq!(
        read_animation_definitions(&not_animation),
        Err(EffectDefError::NotAnAnimationMember)
    );
}

#[test]
fn accept_parity2_dmg_fx_a_a_vehicle_record_yields_its_injure_thresholds() {
    let entry = |threshold: f32, name: &str, target: Option<&str>| {
        let mut parts = vec![float(threshold), text(name)];
        if let Some(target) = target {
            parts.push(text(target));
        }
        list(&parts)
    };
    let part = list(&[
        text("nose"),
        int(25),
        int(25),
        text("critical"),
        text("got_hit_anim"),
        list(&[text("nose_got_hit"), text("player_pfighter")]),
        text("injure_anims"),
        list(&[
            entry(0.99, "nose_damage_effects", Some("player_pfighter")),
            entry(0.2, "nose_damage_red", Some("player_pfighter")),
        ]),
    ]);
    let record = fields(&[
        field("nodename", &[text("player_pfighter")]),
        field(
            "injure_anims",
            &[
                entry(0.1, "player_smoketrail", None),
                entry(0.85, "player_fuelleak", None),
            ],
        ),
        field("destroyable_parts", &[part]),
    ]);
    let bytes = list(&[list(&[text("pdevastator"), record])]);
    let nodes = read_reader_nodes(&bytes).expect("decodes");
    let vehicles = read_vehicle_records(&nodes).expect("typed");
    let [vehicle] = vehicles.as_slice() else {
        panic!("one vehicle");
    };
    assert_eq!(vehicle.key, "pdevastator");
    assert_eq!(vehicle.node_name, Some("player_pfighter"));
    let levels: Vec<_> = vehicle
        .injure_anims
        .iter()
        .map(|entry| (entry.threshold, entry.animation))
        .collect();
    assert_eq!(
        levels,
        [(0.1, "player_smoketrail"), (0.85, "player_fuelleak")]
    );
    let nose = &vehicle.destroyable_parts[0];
    assert_eq!(nose.name, "nose");
    assert_eq!(nose.numbers, [25.0, 25.0]);
    assert_eq!(nose.flags, ["critical"]);
    assert_eq!(
        nose.got_hit_anim,
        Some(("nose_got_hit", Some("player_pfighter")))
    );
    assert_eq!(nose.injure_anims[1].threshold, 0.2);
    assert_eq!(nose.injure_anims[1].target, Some("player_pfighter"));
}

// --- Retail ------------------------------------------------------------------

const ZRDR: &str = "ZBD/zrdr.zbd";

/// Runs `f` over every member of the retail shared reader archive.
pub(super) fn with_zrdr<R>(f: impl FnOnce(&[(String, &[u8])]) -> R) -> R {
    let bytes = fs::read(game_dir().join(ZRDR)).expect("ZBD/zrdr.zbd is readable");
    let relative = path(ZRDR);
    let probe = &bytes[..bytes.len().min(64)];
    let decided = dispatch(ZbdProbe::new(ZRDR, &relative, probe)).expect("dispatches");
    let mut context = ParseContext::with_defaults(ZRDR);
    let index = read_version_one_index(&mut context, decided, &bytes).expect("index");
    let table = index.member_table();
    let archive = read_reader_archive(&mut context, &table, index.data()).expect("archive");
    let members: Vec<(String, &[u8])> = archive
        .entries()
        .map(|entry| {
            (
                String::from_utf8_lossy(entry.name()).into_owned(),
                entry.content(),
            )
        })
        .collect();
    f(&members)
}

/// Every animation named `id` (by `ANIMATION_NAME`, else `NAME`), with its member.
fn animations_named<'a>(
    decoded: &'a [(String, Vec<ReaderNode<'a>>)],
    id: &str,
) -> Vec<(&'a str, EffectAnimation<'a>)> {
    let mut out = Vec::new();
    for (member, nodes) in decoded {
        if let Ok(animations) = read_animation_definitions(nodes) {
            out.extend(
                animations
                    .into_iter()
                    .filter(|animation| animation.id() == id)
                    .map(|animation| (member.as_str(), animation)),
            );
        }
    }
    out
}

pub(super) fn decode_all<'a>(
    members: &'a [(String, &'a [u8])],
) -> Vec<(String, Vec<ReaderNode<'a>>)> {
    members
        .iter()
        .filter(|(name, _)| name.to_ascii_lowercase().ends_with(".zrd"))
        .map(|(name, bytes)| {
            let nodes = read_reader_nodes(bytes)
                .unwrap_or_else(|error| panic!("{name}: token stream refused: {error}"));
            (name.clone(), nodes)
        })
        .collect()
}

fn only<'a>(
    decoded: &'a [(String, Vec<ReaderNode<'a>>)],
    id: &str,
) -> (&'a str, EffectAnimation<'a>) {
    let mut found = animations_named(decoded, id);
    assert_eq!(found.len(), 1, "exactly one retail animation is named {id}");
    found.remove(0)
}

fn puffer<'a, 'b>(
    animation: &'b EffectAnimation<'a>,
    name: &str,
    active: bool,
) -> &'b PufferState<'a> {
    animation
        .puffers()
        .find(|puffer| puffer.name == name && puffer.active == Some(active))
        .unwrap_or_else(|| panic!("{}: no {name} puffer with active={active}", animation.id()))
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity2_dmg_fx_a_retail_every_reader_member_is_a_token_stream_with_no_byte_left() {
    with_zrdr(|members| {
        let decoded = decode_all(members);
        let mut animation_members = 0;
        let mut definitions = 0;
        for (name, nodes) in &decoded {
            assert!(!nodes.is_empty(), "{name}: empty");
            if let Ok(animations) = read_animation_definitions(nodes) {
                animation_members += 1;
                definitions += animations.len();
            }
        }
        assert_eq!(
            (decoded.len(), animation_members, definitions),
            (221, 162, 616),
            "members, animation members, animation definitions"
        );
    });
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity2_dmg_fx_a_retail_dense_firetrail_is_a_smoke_puffer_and_a_fire_puffer() {
    with_zrdr(|members| {
        let decoded = decode_all(members);
        let (member, animation) = only(&decoded, "dense_firetrail");
        assert_eq!(member, "pufftrails.zrd");
        assert_eq!(animation.activation, Some("ON_CALL"));
        assert_eq!(animation.reset_time, [-1.0]);
        let smoke = puffer(&animation, "smokepuffer", true);
        assert_eq!(smoke.at_node.as_ref().map(|at| at.node), Some("INPUT_NODE"));
        assert_eq!(smoke.distance_interval, Some(1.0));
        assert_eq!(smoke.min_random_velocity, Some([-0.1; 3]));
        assert_eq!(smoke.max_random_velocity, Some([-0.1; 3]));
        assert_eq!(smoke.friction, Some(1.0));
        assert_eq!(smoke.size_range, Some([0.4, 0.8]));
        assert_eq!(smoke.lifetime_range, Some([3.5, 4.5]));
        assert_eq!(smoke.growth_factor, Some(2.45));
        assert_eq!(smoke.deviation_distance, Some(0.001));
        assert_eq!(smoke.fade_range, Some([700.0, 800.0]));
        assert_eq!(smoke.textures, ["smoke101", "smoke102", "smoke103"]);
        assert_eq!(
            smoke.colors,
            [
                [0.0, 255.0, 164.0, 90.0, 1.0],
                [0.05, 5.0, 5.0, 5.0, 0.8],
                [0.2, 5.0, 5.0, 5.0, 0.8],
                [1.0, 0.0, 0.0, 0.0, 0.0],
            ]
        );
        let fire = puffer(&animation, "firepuffer", true);
        assert_eq!(fire.distance_interval, Some(0.25));
        assert_eq!(fire.size_range, Some([0.1, 0.4]));
        assert_eq!(fire.lifetime_range, Some([0.5, 0.7]));
        assert_eq!(fire.growth_factor, Some(2.25));
        let frames: Vec<_> = fire
            .texture_sequence
            .iter()
            .map(|(time, _)| *time)
            .collect();
        assert_eq!(frames, [0.0, 0.05, 0.15, 0.25, 0.3, 0.35]);
        assert_eq!(fire.texture_sequence[5].1, "fire_f06");
        // Both are switched off again when the node goes inactive.
        puffer(&animation, "smokepuffer", false);
        puffer(&animation, "firepuffer", false);
    });
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity2_dmg_fx_a_retail_the_other_trails_and_fireballs() {
    with_zrdr(|members| {
        let decoded = decode_all(members);

        let (_, fuel) = only(&decoded, "fuel_trail");
        let fuel_puffer = puffer(&fuel, "fuelpuffer", true);
        assert_eq!(fuel_puffer.distance_interval, Some(0.1));
        assert_eq!(fuel_puffer.size_range, Some([0.05, 0.1]));
        assert_eq!(fuel_puffer.lifetime_range, Some([0.5, 1.0]));
        assert_eq!(
            fuel_puffer.colors,
            [[0.0, 255.0, 255.0, 255.0, 0.6], [1.0, 0.0, 0.0, 0.0, 0.0]]
        );

        let (_, large) = only(&decoded, "large_firetrail");
        let lg = puffer(&large, "lgpuffer", true);
        assert_eq!(lg.priority, Some(6.0));
        assert_eq!(lg.size_range, Some([1.2, 2.6]));
        assert_eq!(lg.lifetime_range, Some([0.3, 2.5]));
        assert_eq!(lg.fade_range, Some([900.0, 1100.0]));
        assert_eq!(lg.texture_sequence.len(), 6);

        let (_, short) = only(&decoded, "short_firetrail");
        let stages: Vec<_> = short
            .puffers()
            .filter(|puffer| puffer.active == Some(true))
            .map(|puffer| {
                (
                    puffer.name,
                    puffer.distance_interval.or(puffer.time_interval),
                )
            })
            .collect();
        assert_eq!(
            stages,
            [
                ("shortpuffer1", Some(0.5)),
                ("shortpuffer2", Some(0.2)),
                ("shortpuffer3", Some(0.18)),
                ("shortpuffer4", Some(0.1)),
                ("black_smoke", Some(0.15)),
            ]
        );

        let (_, sputter) = only(&decoded, "sputter_firetrail");
        assert_eq!(
            puffer(&sputter, "firepuffer", true)
                .at_node
                .as_ref()
                .map(|a| a.words.clone()),
            Some(vec![0.0, 0.0, 0.2])
        );

        // Three variants share the host object `small_injure_fireball`.
        for id in [
            "small_fireball_follow",
            "short_fire_follow",
            "short_fireball_follow",
        ] {
            let (member, animation) = only(&decoded, id);
            assert_eq!(member, "pufftrails.zrd");
            assert_eq!(animation.name, "small_injure_fireball");
        }
        let (_, follow) = only(&decoded, "small_fireball_follow");
        let second = puffer(&follow, "lgpuffer2", true);
        assert_eq!(
            second.colors,
            [
                [0.0, 150.0, 100.0, 100.0, 0.6],
                [0.9, 150.0, 100.0, 100.0, 0.5]
            ]
        );

        // `injure_fireball` is a host node of C3/gamez.zbd, not an animation id.
        assert!(animations_named(&decoded, "injure_fireball").is_empty());
    });
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity2_dmg_fx_a_retail_partial_damage_flak_and_ground_fire() {
    with_zrdr(|members| {
        let decoded = decode_all(members);

        for (id, interval, number, size) in [
            ("sputter_black_smoke", 0.15, 5.0, [0.5, 1.5]),
            ("sputter_fire_smoke", 0.13, 3.0, [1.0, 2.0]),
        ] {
            let (member, animation) = only(&decoded, id);
            assert_eq!(member, "partial_damage.zrd");
            assert_eq!(animation.name, "partial_damage");
            let smoke = puffer(&animation, "black_smoke", true);
            assert_eq!(smoke.time_interval, Some(interval));
            assert_eq!(smoke.number, Some(number));
            assert_eq!(smoke.size_range, Some(size));
            assert_eq!(smoke.near_fade, Some([70.0, 30.0]));
        }
        only(&decoded, "sputter_fire");

        let (member, large) = only(&decoded, "large_30sec_fire");
        assert_eq!(member, "flame_ball.zrd");
        let fire = puffer(&large, "fire_n_smoke", true);
        assert_eq!(fire.time_interval, Some(0.1));
        assert_eq!(fire.lifetime_range, Some([3.5, 5.5]));
        assert_eq!(fire.world_acceleration, Some([0.0, -1.0, 0.0]));
        let (_, huge) = only(&decoded, "huge_30sec_fire");
        assert_eq!(
            puffer(&huge, "fire_n_smoke", true).size_range,
            Some([4.0, 6.0])
        );

        let (member, black) = only(&decoded, "large_black_smokeball");
        assert_eq!(member, "flame_ball.zrd");
        let ball = puffer(&black, "trailpuffer2", true);
        assert_eq!(ball.number, Some(5.0));
        assert_eq!(ball.size_range, Some([4.0, 7.0]));
        assert_eq!(ball.lifetime_range, Some([4.0, 5.0]));
        assert_eq!(
            ball.textures,
            ["thickblksmoke01", "thickblksmoke02", "thickblksmoke03"]
        );

        // The flak effect: one controller calling the trails and the smoke ball.
        let (member, control) = only(&decoded, "flak_effect");
        assert_eq!(member, "flak_control.zrd");
        let called: Vec<_> = control.calls().map(|call| call.name).collect();
        assert_eq!(called, ["call_flak_trails", "large_black_smokeball"]);
        let flash = control.lights().next().expect("the flash light");
        assert_eq!(flash.name, "flak_light1");
        assert_eq!(flash.range, Some([4.0, 10.0]));
        assert_eq!(flash.color, Some([0.95, 0.76, 0.24]));
        let (_, trail) = only(&decoded, "flak");
        assert_eq!(
            trail.puffers().next().map(|p| p.name),
            Some("trailpuffer_dark")
        );
    });
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity2_dmg_fx_a_retail_player_fighter_damage_levels_and_effect_chain() {
    with_zrdr(|members| {
        let decoded = decode_all(members);
        let (_, vehicles) = decoded
            .iter()
            .find(|(name, nodes)| name == "vehicle.zrd" && read_vehicle_records(nodes).is_ok())
            .expect("vehicle.zrd");
        let vehicles = read_vehicle_records(vehicles).expect("typed");
        let player: Vec<_> = vehicles
            .iter()
            .filter(|vehicle| vehicle.node_name == Some("player_pfighter"))
            .collect();
        // `player_airplane` is the base template that names the node and carries
        // no damage fields; `pdevastator` is the record that does.
        assert_eq!(
            player.iter().map(|vehicle| vehicle.key).collect::<Vec<_>>(),
            ["player_airplane", "pdevastator"]
        );
        assert!(player[0].injure_anims.is_empty() && player[0].destroyable_parts.is_empty());
        let player = player[1];
        let levels: Vec<_> = player
            .injure_anims
            .iter()
            .map(|entry| (entry.threshold, entry.animation))
            .collect();
        assert_eq!(
            levels,
            [(0.1, "player_smoketrail"), (0.85, "player_fuelleak")]
        );

        let part = |name: &str| {
            player
                .destroyable_parts
                .iter()
                .find(|part| part.name == name)
                .unwrap_or_else(|| panic!("part {name}"))
        };
        let ladder = |name: &str| -> Vec<(f32, &str)> {
            part(name)
                .injure_anims
                .iter()
                .map(|entry| (entry.threshold, entry.animation))
                .collect()
        };
        assert_eq!(
            ladder("leftwing"),
            [
                (0.99, "leftwing_damage_effects"),
                (0.72, "leftwing_damage_green"),
                (0.5, "pdpanel5"),
                (0.46, "leftwing_damage_yellow"),
                (0.3, "pdpanel4"),
                (0.2, "leftwing_damage_red"),
                (0.15, "pdpanel3"),
            ]
        );
        assert_eq!(
            ladder("rightwing"),
            [
                (0.99, "rightwing_damage_effects"),
                (0.72, "rightwing_damage_green"),
                (0.46, "rightwing_damage_yellow"),
                (0.4, "pdpanel6"),
                (0.3, "pdpanel1"),
                (0.2, "rightwing_damage_red"),
                (0.15, "pdpanel2"),
            ]
        );
        assert_eq!(ladder("nose").last(), Some(&(0.15, "pdpanel7")));
        assert_eq!(ladder("tail").last(), Some(&(0.15, "pdpanel8")));
        assert_eq!(part("tail").flags, ["critical", "engine"]);
        assert_eq!(part("leftwing").numbers, [25.0, 25.0]);
        assert_eq!(
            part("leftwing").got_hit_anim,
            Some(("leftwing_got_hit", Some("player_pfighter")))
        );

        // What the two vehicle-level animations are made of.
        let (member, smoke) = only(&decoded, "player_smoketrail");
        assert_eq!(member, "player.zrd");
        let call = smoke.calls().next().expect("the trail call");
        assert_eq!(
            (call.name, call.local_name, call.with_node, call.start_time),
            (
                "dense_firetrail",
                Some("local_dft"),
                Some("prop1"),
                Some(("EVENT_OFFSET", 0.1))
            )
        );
        let lights: Vec<_> = smoke.lights().collect();
        assert_eq!(lights.len(), 2);
        for light in &lights {
            assert_eq!(light.name, "fire_lt");
            assert_eq!(light.at_node.as_ref().map(|at| at.node), Some("prop1"));
            assert_eq!(light.saturated, Some(false));
        }
        assert_eq!(lights[0].range, Some([0.5, 2.25]));
        assert_eq!(lights[0].color, Some([0.88, 0.78, 0.36]));
        assert_eq!(lights[1].range, Some([0.9, 3.25]));
        assert_eq!(lights[1].color, Some([0.93, 0.78, 0.36]));

        let (_, leak) = only(&decoded, "player_fuelleak");
        let nodes: Vec<_> = leak
            .calls()
            .filter(|call| call.name == "fuel_trail")
            .map(|call| call.with_node)
            .collect();
        assert_eq!(nodes, [Some("pdp1"), Some("pdp2"), Some("pdp3")]);

        let (_, damage_trail) = only(&decoded, "player_damage_trail");
        assert_eq!(
            damage_trail.calls().next().map(|call| call.name),
            Some("short_firetrail")
        );
        let (_, firetrail) = only(&decoded, "player_firetrail");
        assert_eq!(
            firetrail.calls().next().map(|call| call.name),
            Some("dense_firetrail")
        );

        let (_, panel3) = only(&decoded, "pdpanel3");
        let calls: Vec<_> = panel3
            .calls()
            .map(|call| (call.name, call.with_node))
            .collect();
        assert!(calls.contains(&("small_fireball_follow", Some("pdp3"))));
        assert!(calls.contains(&("loop_short_firetrail", Some("pdp3"))));
    });
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity2_dmg_fx_a_retail_the_propeller_spin_and_stop_animations() {
    with_zrdr(|members| {
        let decoded = decode_all(members);
        let (member, spin) = only(&decoded, "spinprops");
        assert_eq!(member, "plane_props.zrd");
        let motion = |animation: &EffectAnimation<'_>, node: &str| -> Option<Vec<f32>> {
            animation
                .sequences
                .iter()
                .flat_map(|sequence| sequence.events.iter())
                .find_map(|event| match event {
                    EffectEvent::Other {
                        keyword: "OBJECT_MOTION",
                        value,
                    } if value
                        .field("NAME")
                        .and_then(|n| n.as_list()?.first()?.as_str())
                        == Some(node) =>
                    {
                        Some(
                            value
                                .field("XYZ_ROTATION")?
                                .as_list()?
                                .iter()
                                .filter_map(ReaderNode::as_number)
                                .collect(),
                        )
                    }
                    _ => None,
                })
        };
        assert_eq!(
            motion(&spin, "prop1"),
            Some(vec![0.0, 0.0, -220.0, 0.0, 0.0, 0.0])
        );
        assert_eq!(
            motion(&spin, "prop1b"),
            Some(vec![0.0, 0.0, 60.0, 0.0, 0.0, 0.0])
        );
        let (_, stop) = only(&decoded, "stopprops");
        assert!(stop.sequences.len() >= 10);
    });
}

/// The `ZBD/C3/cam_anim.zbd` record repeats the source floats in its event
/// stream: every numeric parameter of the two `dense_firetrail` puffers is
/// found, bit for bit, at the offsets below within its 596-byte PUFFER_STATE
/// event (event start = the name field's offset minus 12).
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity2_dmg_fx_a_retail_cam_anim_repeats_the_source_parameters_at_fixed_offsets() {
    use super::t1662::Container;
    let carrier = "ZBD/C3/cam_anim.zbd";
    let container = Container::open(carrier);
    let events = container.with(carrier, |_index, records| {
        let record = records
            .iter()
            .find(|record| record.anim_name() == b"dense_firetrail")
            .expect("the dense_firetrail record");
        assert_eq!(
            record.counts().puffers,
            3,
            "empty slot 0, then the two puffers"
        );
        let [sequence] = record.sequences() else {
            panic!("one ordinary sequence");
        };
        sequence.events().to_vec()
    });
    let word = |at: usize| f32::from_le_bytes(events[at..at + 4].try_into().expect("four bytes"));
    let event_of = |name: &[u8]| {
        let at = events
            .windows(name.len() + 1)
            .position(|window| window[..name.len()] == *name && window[name.len()] == 0)
            .expect("the puffer's name field");
        at - 12
    };
    with_zrdr(|members| {
        let decoded = decode_all(members);
        let (_, source) = only(&decoded, "dense_firetrail");
        for name in ["smokepuffer", "firepuffer"] {
            let at = event_of(name.as_bytes());
            let size = u32::from_le_bytes(events[at + 4..at + 8].try_into().expect("four"));
            assert_eq!(size, 596, "{name}: event size word");
            let source = puffer(&source, name, true);
            let [min_a, min_b, min_c] = source.min_random_velocity.expect("min");
            let [max_a, max_b, max_c] = source.max_random_velocity.expect("max");
            assert_eq!(
                [word(at + 0x60), word(at + 0x64), word(at + 0x68)],
                [min_a, min_b, min_c],
                "{name}: MIN_RANDOM_VELOCITY at +0x60"
            );
            assert_eq!(
                [word(at + 0x6c), word(at + 0x70), word(at + 0x74)],
                [max_a, max_b, max_c],
                "{name}: MAX_RANDOM_VELOCITY at +0x6c"
            );
            assert_eq!(
                word(at + 0x88),
                source.distance_interval.expect("d"),
                "{name}: DISTANCE_INTERVAL at +0x88"
            );
            let size_range = source.size_range.expect("size");
            assert_eq!(
                [word(at + 0x8c), word(at + 0x90)],
                size_range,
                "{name}: SIZE_RANGE at +0x8c"
            );
            let lifetime = source.lifetime_range.expect("lifetime");
            assert_eq!(
                [word(at + 0x94), word(at + 0x98)],
                lifetime,
                "{name}: LIFETIME_RANGE at +0x94"
            );
            assert_eq!(
                word(at + 0xa4),
                source.deviation_distance.expect("dev"),
                "{name}: DEVIATION_DISTANCE at +0xa4"
            );
            let fade = source.fade_range.expect("fade");
            assert_eq!(
                [word(at + 0xb0), word(at + 0xb4)],
                fade,
                "{name}: FADE_RANGE at +0xb0"
            );
            assert_eq!(
                word(at + 0xb8),
                source.friction.expect("friction"),
                "{name}: FRICTION at +0xb8"
            );
            assert_eq!(
                word(at + 0x230),
                source.growth_factor.expect("growth"),
                "{name}: GROWTH_FACTOR at +0x230"
            );
        }
    });
}
