//! Acceptance for task #1942 (`PARITY-M01-PLAYER-DAMAGE-GAUGE-A`): which
//! animation each damage level of the player airframe's parts plays, and what
//! the gauge-zone animations do to the cockpit's `<part>damage` nodes
//! (`docs/findings/2026-10-23-parity-m01-player-damage-gauge-mapping.md`).
//!
//! The synthetic test authors every byte it reads; the retail tests read
//! `$CS_GAME_DIR` (never write it) and fail loudly without it. Task test
//! prefix: `accept_parity_m01_player_damage_gauge_a_`.

use cs_formats::zbd::{
    EffectDefError, ReaderNode, read_animation_definitions, read_reader_nodes, read_vehicle_records,
};

use super::accept_parity2_dmg_fx_a::{decode_all, with_zrdr};
use super::t1662::Container;

fn int(value: i32) -> Vec<u8> {
    [1u32.to_le_bytes(), value.to_le_bytes()].concat()
}

fn text(value: &str) -> Vec<u8> {
    let mut out = 3u32.to_le_bytes().to_vec();
    out.extend((value.len() as u32).to_le_bytes());
    out.extend(value.as_bytes());
    out
}

fn list(children: &[Vec<u8>]) -> Vec<u8> {
    let mut out = 4u32.to_le_bytes().to_vec();
    out.extend((children.len() as u32 + 1).to_le_bytes());
    for child in children {
        out.extend(child);
    }
    out
}

fn cycle_event(node: Option<&str>, reset: Option<i32>) -> Vec<Vec<u8>> {
    let mut value = Vec::new();
    if let Some(node) = node {
        value.push(text("NAME"));
        value.push(list(&[text(node)]));
    }
    if let Some(reset) = reset {
        value.push(text("RESET"));
        value.push(list(&[int(reset)]));
    }
    vec![text("OBJECT_CYCLE_TEXTURE"), list(&value)]
}

/// One animation member holding a single sequence of `events`.
fn animation_with(events: &[Vec<Vec<u8>>]) -> Vec<u8> {
    let sequence = list(&events.concat());
    let definition = list(&[
        text("NAME"),
        list(&[text("nose")]),
        text("ANIMATION_NAME"),
        list(&[text("nose_damage_yellow")]),
        text("ACTIVATION"),
        list(&[text("ON_CALL")]),
        text("SEQUENCE_DEFINITION"),
        sequence,
    ]);
    let animation_list = list(&[text("ANIMATION_DEFINITION"), definition]);
    let definitions = list(&[text("ANIMATION_LIST"), animation_list]);
    list(&[list(&[text("ANIMATION_DEFINITIONS"), definitions])])
}

#[test]
fn accept_parity_m01_player_damage_gauge_a_a_cycle_texture_event_yields_node_and_reset() {
    let bytes = animation_with(&[
        cycle_event(Some("nosedamage"), Some(2)),
        cycle_event(Some("nosedamage"), None),
        cycle_event(None, Some(1)),
    ]);
    let nodes = read_reader_nodes(&bytes).expect("decodes");
    let animations = read_animation_definitions(&nodes).expect("an animation member");
    assert_eq!(animations.len(), 1);
    assert_eq!(
        animations[0].cycle_textures().collect::<Vec<_>>(),
        [("nosedamage", 2)],
        "an event missing its name or its RESET is skipped, not guessed"
    );
}

/// The five levels of a part's ladder, as `(threshold, animation)`.
fn ladder<'a>(nodes: &[ReaderNode<'a>], part: &str) -> Result<Vec<(f32, &'a str)>, EffectDefError> {
    let vehicles = read_vehicle_records(nodes)?;
    let player = vehicles
        .into_iter()
        .find(|vehicle| vehicle.key == "pdevastator")
        .expect("pdevastator");
    let part = player
        .destroyable_parts
        .into_iter()
        .find(|candidate| candidate.name == part)
        .expect("part");
    Ok(part
        .injure_anims
        .iter()
        .map(|entry| (entry.threshold, entry.animation))
        .collect())
}

/// Every part's `<part>_damage_<level>` animation cycles that part's gauge
/// node (`<part>damage`) to RESET 1 (green), 2 (yellow), 3 (red) — and the
/// `pdpanelN` animations never touch a texture.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_player_damage_gauge_a_retail_levels_cycle_the_zone_and_panels_do_not() {
    with_zrdr(|members| {
        let decoded = decode_all(members);
        let (_, vehicle) = decoded
            .iter()
            .find(|(name, nodes)| name == "vehicle.zrd" && read_vehicle_records(nodes).is_ok())
            .expect("vehicle.zrd");
        for (part, zone) in [
            ("nose", "nosedamage"),
            ("tail", "taildamage"),
            ("leftwing", "leftwingdamage"),
            ("rightwing", "rightwingdamage"),
        ] {
            let ladder = ladder(vehicle, part).expect("ladder");
            for (level, reset) in [("green", 1), ("yellow", 2), ("red", 3)] {
                let id = format!("{part}_damage_{level}");
                assert!(ladder.iter().any(|(_, anim)| *anim == id), "{id} in ladder");
                let mut cycles = Vec::new();
                for (_, nodes) in &decoded {
                    for animation in read_animation_definitions(nodes).unwrap_or_default() {
                        if animation.id() == id {
                            cycles.extend(animation.cycle_textures());
                        }
                    }
                }
                assert_eq!(cycles, [(zone, reset)], "{id}");
            }
        }
        for panel in 1..=8 {
            let id = format!("pdpanel{panel}");
            let mut found = 0;
            for (_, nodes) in &decoded {
                for animation in read_animation_definitions(nodes).unwrap_or_default() {
                    if animation.id() == id {
                        found += 1;
                        assert_eq!(animation.cycle_textures().count(), 0, "{id}");
                    }
                }
            }
            assert_eq!(found, 1, "{id}");
        }
    });
}

/// The per-part threshold ladder, and which `pdpanelN` sits at which level.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_player_damage_gauge_a_retail_pdpanel_per_part_ladder() {
    with_zrdr(|members| {
        let decoded = decode_all(members);
        let (_, vehicle) = decoded
            .iter()
            .find(|(name, nodes)| name == "vehicle.zrd" && read_vehicle_records(nodes).is_ok())
            .expect("vehicle.zrd");
        let panels = |part: &str| -> Vec<(f32, String)> {
            ladder(vehicle, part)
                .expect("ladder")
                .into_iter()
                .filter(|(_, anim)| anim.starts_with("pdpanel"))
                .map(|(threshold, anim)| (threshold, anim.to_owned()))
                .collect()
        };
        let pair = |threshold: f32, name: &str| (threshold, name.to_owned());
        assert_eq!(panels("nose"), [pair(0.15, "pdpanel7")]);
        assert_eq!(panels("tail"), [pair(0.15, "pdpanel8")]);
        assert_eq!(
            panels("leftwing"),
            [
                pair(0.5, "pdpanel5"),
                pair(0.3, "pdpanel4"),
                pair(0.15, "pdpanel3")
            ]
        );
        assert_eq!(
            panels("rightwing"),
            [
                pair(0.4, "pdpanel6"),
                pair(0.3, "pdpanel1"),
                pair(0.15, "pdpanel2")
            ]
        );
        for part in ["nose", "tail", "leftwing", "rightwing"] {
            let gauge: Vec<_> = ladder(vehicle, part)
                .expect("ladder")
                .into_iter()
                .filter(|(_, anim)| anim.contains("_damage_") && !anim.ends_with("_effects"))
                .map(|(threshold, anim)| (threshold, anim.to_owned()))
                .collect();
            assert_eq!(
                gauge,
                [
                    pair(0.72, &format!("{part}_damage_green")),
                    pair(0.46, &format!("{part}_damage_yellow")),
                    pair(0.2, &format!("{part}_damage_red")),
                ],
                "{part}"
            );
        }
    });
}

/// The compiled `ZBD/C3/cam_anim.zbd` record repeats the RESET value as the
/// 16-bit word at +0x12 of a 0x14-byte event with tag 0x111.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_player_damage_gauge_a_retail_cam_anim_repeats_the_reset_word() {
    let carrier = "ZBD/C3/cam_anim.zbd";
    let container = Container::open(carrier);
    container.with(carrier, |_index, records| {
        for part in ["nose", "tail", "leftwing", "rightwing"] {
            for (level, reset) in [("green", 1u16), ("yellow", 2), ("red", 3)] {
                let id = format!("{part}_damage_{level}");
                // The green record opens with a zero-valued cycle event of
                // its own; yellow and red carry one event.
                let expected: Vec<u16> = if level == "green" {
                    vec![0, reset]
                } else {
                    vec![reset]
                };
                let record = records
                    .iter()
                    .find(|record| record.anim_name() == id.as_bytes())
                    .unwrap_or_else(|| panic!("record {id}"));
                let cycle: Vec<u16> = record
                    .sequences()
                    .iter()
                    .filter_map(|sequence| {
                        let events = sequence.events();
                        (events.len() >= 0x14 && events[..4] == [0x11, 1, 0, 0])
                            .then(|| u16::from_le_bytes([events[0x12], events[0x13]]))
                    })
                    .collect();
                assert_eq!(cycle, expected, "{id}");
            }
        }
    });
}
