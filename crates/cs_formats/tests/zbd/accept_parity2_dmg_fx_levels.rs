//! Acceptance for task #1979 (`PARITY2-DMG-FX-LEVELS`): which `injure_anims`
//! entry plays when, how the puffer runtime applies `GROWTH_FACTOR` /
//! `FADE_RANGE`, and what starts the propeller discs
//! (`docs/findings/2026-10-23-parity2-dmg-fx-levels.md`).
//!
//! * The data tests read `$CS_GAME_DIR` through the production readers
//!   (`read_vehicle_records`, `read_animation_definitions`) and are ignored
//!   without it.
//! * The image tests re-read the cited virtual addresses of
//!   `$CS_ENGINE_IMAGE` (never committed) and are ignored without it.
//! * The committed rows (`[[puffer_runtime]]` of
//!   `data/owner-static/engine-facts.toml`) are checked on every run.
//!
//! Task test prefix: `accept_parity2_dmg_fx_levels_`.

use std::collections::BTreeSet;

use cs_formats::zbd::{
    EffectEvent, InjureAnim, ReaderNode, read_animation_definitions, read_vehicle_records,
};

use super::accept_parity2_dmg_fx_a::{decode_all, with_zrdr};

const TABLE_TEXT: &str = include_str!("../../../../data/owner-static/engine-facts.toml");

// --- Committed rows -----------------------------------------------------------

struct Row {
    name: String,
    va: u64,
    kind: String,
    value: String,
}

fn rows() -> Vec<Row> {
    let mut rows = Vec::new();
    let mut current: Option<Vec<(String, String)>> = None;
    let finish = |entries: Option<Vec<(String, String)>>, rows: &mut Vec<Row>| {
        let Some(entries) = entries else { return };
        let get = |key: &str| -> String {
            entries
                .iter()
                .find(|(k, _)| k == key)
                .unwrap_or_else(|| panic!("a [[puffer_runtime]] row lacks `{key}`"))
                .1
                .clone()
        };
        assert_eq!(get("provenance"), "\"static\"");
        assert_eq!(get("claim"), "\"OWNER-STATIC-2026-10-23\"");
        let va = get("va");
        rows.push(Row {
            name: get("name").trim_matches('"').to_owned(),
            va: u64::from_str_radix(va.trim_start_matches("0x"), 16).expect("a hex va"),
            kind: get("kind").trim_matches('"').to_owned(),
            value: get("value"),
        });
    };
    for line in TABLE_TEXT.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            finish(current.take(), &mut rows);
            if line == "[[puffer_runtime]]" {
                current = Some(Vec::new());
            }
        } else if let Some(entries) = current.as_mut()
            && let Some((key, value)) = line.split_once(" = ")
        {
            entries.push((key.to_owned(), value.to_owned()));
        }
    }
    finish(current.take(), &mut rows);
    rows
}

/// The committed rows: `(name, va, kind, value)`.
const EXPECTED_ROWS: &[(&str, u64, &str, &str)] = &[
    ("puffer_age_factor_default", 0x637a90, "f32", "1.0"),
    ("puffer_fade_factor_default", 0x637a94, "f32", "1.0"),
    ("puffer_jump_guard_distance", 0x6035c4, "f32", "200.0"),
    ("puffer_alpha_scale", 0x60414c, "f32", "255.0"),
    ("puffer_priority_depth_bias_a", 0x54db72, "f32", "0.02"),
    ("puffer_priority_depth_bias_b", 0x54db88, "f32", "0.01"),
    ("puffer_growth_start_scale", 0x4fafb7, "f32", "1.0"),
    (
        "puffer_far_fade_default_bits",
        0x550263,
        "u32",
        "0x7f7fffff",
    ),
    ("puffer_friction_lookup_scale", 0x609128, "f32", "-51.0"),
    (
        "puffer_friction_table_step",
        0x60912c,
        "f32",
        "-0.019607844",
    ),
    ("puffer_friction_cutoff", 0x6036bc, "f32", "5.0"),
];

#[test]
fn accept_parity2_dmg_fx_levels_committed_rows_are_exactly_the_finding_s_table() {
    let rows = rows();
    assert_eq!(rows.len(), EXPECTED_ROWS.len());
    for (row, (name, va, kind, value)) in rows.iter().zip(EXPECTED_ROWS) {
        assert_eq!(
            (
                row.name.as_str(),
                row.va,
                row.kind.as_str(),
                row.value.as_str()
            ),
            (*name, *va, *kind, *value)
        );
    }
}

// --- Retail data --------------------------------------------------------------

fn pairs<'a>(entries: &[InjureAnim<'a>]) -> Vec<(f32, &'a str)> {
    entries
        .iter()
        .map(|entry| (entry.threshold, entry.animation))
        .collect()
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity2_dmg_fx_levels_retail_vehicle_ladders() {
    with_zrdr(|members| {
        let decoded = decode_all(members);
        let (_, nodes) = decoded
            .iter()
            .find(|(name, nodes)| name == "vehicle.zrd" && read_vehicle_records(nodes).is_ok())
            .expect("vehicle.zrd");
        let vehicles = read_vehicle_records(nodes).expect("typed");

        // The eleven player airframes (a `player_*` node, with parts) all
        // carry the same vehicle-level list.
        let player_level = [(0.1, "player_smoketrail"), (0.85, "player_fuelleak")];
        let players: Vec<_> = vehicles
            .iter()
            .filter(|vehicle| {
                vehicle.node_name.is_some_and(|n| n.starts_with("player_"))
                    && !vehicle.destroyable_parts.is_empty()
            })
            .collect();
        assert_eq!(players.len(), 11);
        for vehicle in &players {
            assert_eq!(
                pairs(&vehicle.injure_anims),
                player_level,
                "{}",
                vehicle.key
            );
            let parts: Vec<_> = vehicle.destroyable_parts.iter().map(|p| p.name).collect();
            assert_eq!(parts, ["nose", "tail", "leftwing", "rightwing"]);
            // Every part ladder ends on a `pdpanel` fire at 0.2 or lower
            // (0.1 to 0.16 in practice).
            for part in &vehicle.destroyable_parts {
                let last = part.injure_anims.last().expect("a ladder");
                assert!(last.animation.starts_with("pdpanel"), "{}", vehicle.key);
                assert!(last.threshold <= 0.2, "{}", vehicle.key);
            }
        }
        // `pdevastator`, the M01 player: the measured ladder.
        let devastator = players
            .iter()
            .find(|vehicle| vehicle.key == "pdevastator")
            .expect("pdevastator");
        let left = devastator
            .destroyable_parts
            .iter()
            .find(|part| part.name == "leftwing")
            .expect("leftwing");
        assert_eq!(
            pairs(&left.injure_anims),
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

        // Non-player airframes: no parts with ladders, a vehicle-level ladder
        // of `random_remote_damage` steps and one trail at 0.40.
        let ai = vehicles
            .iter()
            .find(|vehicle| vehicle.key == "devastator")
            .expect("devastator");
        assert_eq!(
            pairs(&ai.injure_anims),
            [
                (0.95, "random_remote_damage"),
                (0.8, "random_remote_damage"),
                (0.65, "random_remote_damage"),
                (0.5, "random_remote_damage"),
                (0.45, "random_remote_damage"),
                (0.4, "pfsmoketrail"),
                (0.25, "random_remote_damage"),
            ]
        );
        let rdevastator = vehicles
            .iter()
            .find(|vehicle| vehicle.key == "rdevastator")
            .expect("rdevastator");
        assert!(
            rdevastator
                .destroyable_parts
                .iter()
                .all(|part| part.injure_anims.is_empty())
        );
    });
}

fn field_name(value: &ReaderNode<'_>, field: &str) -> Option<String> {
    let fields = value.fields()?;
    let (_, node) = fields.into_iter().find(|(key, _)| *key == Some(field))?;
    node.as_list()?.first()?.as_str().map(str::to_owned)
}

/// Every event keyword, the nodes an `OBJECT_MOTION` moves and the
/// `(node, STATE)` pairs an `OBJECT_ACTIVE_STATE` sets, over the call closure
/// of `roots`.
struct Closure {
    animations: BTreeSet<String>,
    keywords: BTreeSet<String>,
    moved: BTreeSet<String>,
    active_states: BTreeSet<(String, String)>,
}

fn closure(decoded: &[(String, Vec<ReaderNode<'_>>)], roots: &[&str]) -> Closure {
    let mut out = Closure {
        animations: BTreeSet::new(),
        keywords: BTreeSet::new(),
        moved: BTreeSet::new(),
        active_states: BTreeSet::new(),
    };
    let mut todo: Vec<String> = roots.iter().map(|root| (*root).to_owned()).collect();
    while let Some(id) = todo.pop() {
        if !out.animations.insert(id.clone()) {
            continue;
        }
        for (_, nodes) in decoded {
            let Ok(animations) = read_animation_definitions(nodes) else {
                continue;
            };
            for animation in animations.iter().filter(|animation| animation.id() == id) {
                for event in animation.sequences.iter().flat_map(|s| &s.events) {
                    match event {
                        EffectEvent::Other { keyword, value } => {
                            out.keywords.insert((*keyword).to_owned());
                            let name = field_name(value, "NAME").unwrap_or_default();
                            match *keyword {
                                "OBJECT_MOTION" => {
                                    out.moved.insert(name);
                                }
                                "OBJECT_ACTIVE_STATE" => {
                                    let state = field_name(value, "STATE").unwrap_or_default();
                                    out.active_states.insert((name, state));
                                }
                                _ => {}
                            }
                        }
                        EffectEvent::CallAnimation(call) => todo.push(call.name.to_owned()),
                        EffectEvent::Puffer(_) => {
                            out.keywords.insert("PUFFER_STATE".to_owned());
                        }
                        EffectEvent::Light(_) => {
                            out.keywords.insert("LIGHT_STATE".to_owned());
                        }
                    }
                }
            }
        }
    }
    out
}

fn strings(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity2_dmg_fx_levels_retail_injure_animations_move_only_debris_nodes() {
    with_zrdr(|members| {
        let decoded = decode_all(members);
        let (_, nodes) = decoded
            .iter()
            .find(|(name, nodes)| name == "vehicle.zrd" && read_vehicle_records(nodes).is_ok())
            .expect("vehicle.zrd");
        let vehicles = read_vehicle_records(nodes).expect("typed");
        let devastator = vehicles
            .iter()
            .find(|vehicle| vehicle.key == "pdevastator")
            .expect("pdevastator");
        let mut roots: Vec<&str> = devastator
            .injure_anims
            .iter()
            .map(|entry| entry.animation)
            .collect();
        for part in &devastator.destroyable_parts {
            roots.extend(part.injure_anims.iter().map(|entry| entry.animation));
        }
        let closure = closure(&decoded, &roots);

        // Thirty-eight animations make up the player's damage effects.
        assert_eq!(closure.animations.len(), 38);
        // No event moves the airframe, a wing, a panel or a prop: only the
        // thrown debris (`flake1..7`, `gimmeflakes`) and the gun-hit sparks
        // (`bit1..3`, `chunk`, `3040slug_gunhit` at the fuel-leak nodes) move.
        assert_eq!(
            closure.moved,
            strings(&[
                "bit1", "bit2", "bit3", "chunk", "flake1", "flake2", "flake3", "flake4", "flake5",
                "flake6", "flake7"
            ])
        );
        // Node visibility is what the rest of them change.
        assert_eq!(
            closure.active_states,
            [
                ("fire_lt", "INACTIVE"),
                ("flame_ball_02", "ACTIVE"),
                ("flame_ball_02", "INACTIVE"),
                ("gunhit_lt", "INACTIVE"),
                ("pcdp4", "ACTIVE"),
                ("pcdp6", "ACTIVE"),
                ("pdp1", "ACTIVE"),
                ("pdp2", "ACTIVE"),
                ("pdp3", "ACTIVE"),
                ("pdp4", "ACTIVE"),
                ("pdp5", "ACTIVE"),
                ("pdp6", "ACTIVE"),
                ("pdp7", "ACTIVE"),
                ("pdp8", "ACTIVE"),
                ("wing_flare2", "INACTIVE"),
                ("winglight2", "INACTIVE"),
                ("yellow_spark_02", "INACTIVE"),
            ]
            .iter()
            .map(|(node, state)| ((*node).to_owned(), (*state).to_owned()))
            .collect()
        );
        // The wing_flare1 / winglight1 pair is not touched by any of them.
        assert!(
            closure
                .active_states
                .iter()
                .all(|(node, _)| node != "wing_flare1" && node != "winglight1")
        );
        // The event vocabulary of the whole closure.
        assert_eq!(
            closure.keywords,
            strings(&[
                "ACTIVATION",
                "CALL_SEQUENCE",
                "ELSE",
                "ELSEIF",
                "ENDIF",
                "IF",
                "LOOP",
                "OBJECT_ACTIVE_STATE",
                "OBJECT_CYCLE_TEXTURE",
                "OBJECT_MOTION",
                "SOUND",
                "STOP_SEQUENCE",
                "PUFFER_STATE",
                "LIGHT_STATE",
            ])
        );
    });
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity2_dmg_fx_levels_retail_spinning_props_carry_no_opacity_event() {
    with_zrdr(|members| {
        let decoded = decode_all(members);
        let spin = closure(&decoded, &["spinprops"]);
        let stop = closure(&decoded, &["stopprops"]);
        // Spinning: both discs of each prop pair are made active together
        // (`prop1` with `prop1b`), the nitro and static discs are not, and
        // nothing in the animation sets an opacity: whatever the discs draw
        // with while spinning is the node's own opacity and texture alpha.
        for node in ["prop1", "prop1b", "prop2", "prop2b", "prop3", "prop3b"] {
            assert!(
                spin.active_states
                    .contains(&(node.to_owned(), "ACTIVE".to_owned())),
                "{node}"
            );
        }
        for node in ["nitroprop1", "staticprop1"] {
            assert!(
                spin.active_states
                    .contains(&(node.to_owned(), "INACTIVE".to_owned())),
                "{node}"
            );
        }
        assert!(spin.keywords.iter().all(|k| !k.contains("OPACITY")));
        assert!(spin.moved.contains("prop1") && spin.moved.contains("prop1b"));
        // Stopping is where the opacity fades live, and they are timed, not
        // driven by a speed.
        assert!(stop.keywords.contains("OBJECT_OPACITY_FROM_TO"));
        assert!(stop.keywords.iter().all(|k| k != "PUFFER_STATE"));
    });
}

// --- Engine image -------------------------------------------------------------

fn load_image() -> Vec<u8> {
    let path = std::env::var("CS_ENGINE_IMAGE").unwrap_or_else(|_| {
        panic!("CS_ENGINE_IMAGE is not set: this test needs the owner-supplied engine image")
    });
    let image = std::fs::read(path).expect("the engine image reads");
    assert_eq!(&image[..2], b"MZ");
    image
}

/// The bytes of a virtual address range (the image maps `va - 0x400000`).
fn at(image: &[u8], va: u64, len: usize) -> &[u8] {
    let offset = usize::try_from(va - 0x40_0000).expect("an offset");
    &image[offset..offset + len]
}

fn call_target(image: &[u8], site: u64) -> u64 {
    let bytes = at(image, site, 5);
    assert_eq!(bytes[0], 0xe8, "a near call at {site:#x}");
    let rel = i32::from_le_bytes(bytes[1..5].try_into().expect("four bytes"));
    (site as i64 + 5 + i64::from(rel)) as u64
}

fn assert_bytes(image: &[u8], va: u64, want: &[u8], what: &str) {
    assert_eq!(at(image, va, want.len()), want, "{what} at {va:#x}");
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_parity2_dmg_fx_levels_image_rows_match_the_image() {
    let image = load_image();
    for row in rows() {
        let bytes: [u8; 4] = at(&image, row.va, 4).try_into().expect("four bytes");
        match row.kind.as_str() {
            "f32" => assert_eq!(
                f32::from_le_bytes(bytes),
                row.value.parse::<f32>().expect("an f32"),
                "{} at {:#x}",
                row.name,
                row.va
            ),
            "u32" => assert_eq!(
                u32::from_le_bytes(bytes),
                u32::from_str_radix(row.value.trim_start_matches("0x"), 16).expect("hex"),
                "{} at {:#x}",
                row.name,
                row.va
            ),
            other => panic!("unknown kind {other}"),
        }
    }
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_parity2_dmg_fx_levels_image_injure_anims_selector() {
    let image = load_image();

    // The vehicle-level list and the part list are both parsed from the
    // keyword `injure_anims` (the string's two xrefs).
    assert_bytes(&image, 0x627dc4, b"injure_anims\0", "string");
    assert_bytes(&image, 0x627eb4, b"injure_anims\0", "string");
    assert_bytes(
        &image,
        0x4796a5,
        &[0x68, 0xc4, 0x7d, 0x62, 0x00],
        "part parse push",
    );
    assert_bytes(
        &image,
        0x479f2d,
        &[0x68, 0xb4, 0x7e, 0x62, 0x00],
        "vehicle parse push",
    );

    // Vehicle level `0x4b3800`: the fraction is [+0x2d0] / [+0x2cc] ...
    assert_bytes(
        &image,
        0x4b3806,
        &[0xd9, 0x86, 0xd0, 0x02, 0x00, 0x00],
        "fld [esi+0x2d0]",
    );
    assert_bytes(
        &image,
        0x4b380e,
        &[0xd8, 0xb6, 0xcc, 0x02, 0x00, 0x00],
        "fdiv [esi+0x2cc]",
    );
    // ... an entry is 0x1c bytes ...
    assert_bytes(&image, 0x4b388d, &[0x83, 0xc5, 0x1c], "add ebp, 0x1c");
    // ... a running entry (handle non-null) is stopped when the fraction is
    // above its threshold: `fcomp thr; test ah,0x41; jne skip` ...
    assert_bytes(
        &image,
        0x4b386b,
        &[0xf6, 0xc4, 0x41, 0x0f, 0x85],
        "running: skip if x <= thr",
    );
    assert_eq!(call_target(&image, 0x4b3877), 0x4e_d480, "stop");
    // ... a stopped entry is started when it is at or below: `je skip`.
    assert_bytes(
        &image,
        0x4b38a3,
        &[0xf6, 0xc4, 0x41, 0x74],
        "idle: skip if x > thr",
    );
    assert_eq!(call_target(&image, 0x4b38e9), 0x4e_dda0, "start");

    // Part level `0x4b3d70`: the same loop over the part's own fraction.
    assert_bytes(&image, 0x4b3d76, &[0xd9, 0x46, 0x30], "fld [esi+0x30]");
    assert_bytes(&image, 0x4b3d7a, &[0xd8, 0x76, 0x2c], "fdiv [esi+0x2c]");
    assert_bytes(
        &image,
        0x4b3dc2,
        &[0xf6, 0xc4, 0x41, 0x75],
        "running: skip if x <= thr",
    );
    assert_eq!(call_target(&image, 0x4b3dca), 0x4e_d480, "stop");
    assert_bytes(
        &image,
        0x4b3dee,
        &[0xf6, 0xc4, 0x41, 0x74],
        "idle: skip if x > thr",
    );
    assert_eq!(call_target(&image, 0x4b3e03), 0x4e_dda0, "start");

    // The vehicle totals are sums over the 0x58-byte parts: part +0x2c and
    // +0x30 into +0x2cc and +0x2d0, part +0x24 and +0x28 into +0x2c4, +0x2c8.
    assert_bytes(&image, 0x47bf0e, &[0xd9, 0x41, 0x2c], "part +0x2c");
    assert_bytes(&image, 0x47bef9, &[0xd9, 0x41, 0x30], "part +0x30");
    assert_bytes(
        &image,
        0x47bf44,
        &[0xd9, 0x9e, 0xd0, 0x02, 0x00, 0x00],
        "store +0x2d0",
    );
    assert_bytes(
        &image,
        0x47bf4a,
        &[0xd9, 0x9e, 0xcc, 0x02, 0x00, 0x00],
        "store +0x2cc",
    );

    // Who re-evaluates: every call site of the two selectors.
    for site in [
        0x47_bf57, 0x47_c0e2, 0x47_c1fe, 0x47_eba5, 0x47_f127, 0x48_0653, 0x48_d7a7, 0x49_86a0,
        0x4b_433e, 0x4b_81ec, 0x4b_a327,
    ] {
        assert_eq!(
            call_target(&image, site),
            0x4b_3800,
            "vehicle selector {site:#x}"
        );
    }
    for site in [
        0x47_bf50, 0x47_c0db, 0x47_c1f7, 0x48_06bb, 0x4b_3c34, 0x4b_431d, 0x4b_8280,
    ] {
        assert_eq!(
            call_target(&image, site),
            0x4b_3d70,
            "part selector {site:#x}"
        );
    }
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_parity2_dmg_fx_levels_image_puffer_runtime() {
    let image = load_image();

    // The compiled event is turned into the runtime emitter by one applier
    // (0x4e7e60..0x4e8290); its calls, in order, are the setters.
    let applier: &[(u64, u64)] = &[
        (0x4e7f69, 0x550440), // DISTANCE_INTERVAL / TIME_INTERVAL mode
        (0x4e7f79, 0x550460), // the interval (emitter +0x40, 1/interval at +0x44)
        (0x4e7f94, 0x550490), // SIZE_RANGE min
        (0x4e7fa4, 0x5504a0), // SIZE_RANGE max
        (0x4e7fbf, 0x5504b0), // LIFETIME_RANGE min
        (0x4e7fcf, 0x5504c0), // LIFETIME_RANGE max
        (0x4e7fea, 0x5504d0), // START_AGE_RANGE min
        (0x4e7ffa, 0x5504e0), // START_AGE_RANGE max
        (0x4e8015, 0x5504f0), // DEVIATION_DISTANCE
        (0x4e8038, 0x550500), // NEAR_FADE
        (0x4e805a, 0x550550), // FADE_RANGE / FAR_FADE
        (0x4e8076, 0x5505a0), // FRICTION
        (0x4e8092, 0x5505b0), // WIND_FACTOR
        (0x4e80ae, 0x5505c0), // PRIORITY
        (0x4e80ca, 0x5505d0), // NUMBER
        (0x4e81b3, 0x550660), // clear the scale sequence
        (0x4e81dd, 0x550670), // add one (age, scale) pair
    ];
    for (site, target) in applier {
        assert_eq!(
            call_target(&image, *site),
            *target,
            "applier call {site:#x}"
        );
    }
    // FADE_RANGE and FAR_FADE are two spellings of one field: one lookup each
    // in a row, the first found wins, both set the 0x1000 flag.
    assert_bytes(
        &image,
        0x4fcf29,
        &[0x68, 0xbc, 0x01, 0x63, 0x00],
        "push FADE_RANGE",
    );
    assert_bytes(
        &image,
        0x4fcf3d,
        &[0x68, 0xb0, 0x01, 0x63, 0x00],
        "push FAR_FADE",
    );
    assert_bytes(&image, 0x630_1bc, b"FADE_RANGE\0", "string");
    assert_bytes(&image, 0x630_1b0, b"FAR_FADE\0", "string");
    // The far-fade setter stores start at +0xdc, end at +0xe0 and
    // 1/(end-start) at +0xe4; the near-fade setter does +0xd0, +0xd4, +0xd8.
    assert_bytes(
        &image,
        0x55055c,
        &[0xd9, 0x99, 0xdc, 0x00, 0x00, 0x00],
        "far start",
    );
    assert_bytes(
        &image,
        0x550566,
        &[0x89, 0x81, 0xe0, 0x00, 0x00, 0x00],
        "far end",
    );
    assert_bytes(
        &image,
        0x550576,
        &[0xd9, 0x91, 0xe4, 0x00, 0x00, 0x00],
        "far inverse",
    );
    assert_bytes(
        &image,
        0x55050c,
        &[0xd9, 0x99, 0xd0, 0x00, 0x00, 0x00],
        "near start",
    );
    assert_bytes(
        &image,
        0x550516,
        &[0x89, 0x81, 0xd4, 0x00, 0x00, 0x00],
        "near end",
    );
    assert_bytes(
        &image,
        0x550526,
        &[0xd9, 0x91, 0xd8, 0x00, 0x00, 0x00],
        "near inverse",
    );

    // GROWTH_FACTOR: the parser stores the number at event +0x230 and, instead
    // of a growth field, writes a two-key scale sequence: count 2, key 0.0 ->
    // scale 1.0, key 1.0 -> scale G (the same +0x220..+0x230 words the
    // `SCALE_SEQUENCE` keyword fills), and sets the shared 0x2000 flag.
    assert_bytes(
        &image,
        0x4fadb7,
        &[0x89, 0x8f, 0x30, 0x02, 0x00, 0x00],
        "G -> +0x230",
    );
    assert_bytes(
        &image,
        0x4fafbb,
        &[0xc7, 0x87, 0x20, 0x02, 0x00, 0x00, 0x02, 0, 0, 0],
        "count 2",
    );
    assert_bytes(
        &image,
        0x4fafc5,
        &[0xc7, 0x87, 0x24, 0x02, 0x00, 0x00, 0, 0, 0, 0],
        "key 0.0",
    );
    assert_bytes(
        &image,
        0x4fafcf,
        &[0x89, 0x87, 0x28, 0x02, 0x00, 0x00],
        "scale 1.0",
    );
    assert_bytes(
        &image,
        0x4fafd5,
        &[0x89, 0x87, 0x2c, 0x02, 0x00, 0x00],
        "key 1.0",
    );
    assert_bytes(
        &image,
        0x4fad98,
        &[0x81, 0x4f, 0x30, 0x00, 0x20, 0x00, 0x00],
        "or +0x30, 0x2000",
    );

    // The draw (0x54e6e0): the depth is scaled by [0x9fd5c0] and, for the far
    // test, by the global fade factor [0x637a94]; far end culls, far start
    // begins a linear ramp to 0 (x [+0x48]); the near pair culls at or below
    // its first number and is opaque at or above its second; between them the
    // ramp subtracts +0x3c (the far start), which is not the near start.
    assert_bytes(
        &image,
        0x54e732,
        &[0xd8, 0x0d, 0x94, 0x7a, 0x63, 0x00],
        "fmul fade factor",
    );
    assert_bytes(&image, 0x54e73c, &[0xd8, 0x5e, 0x40], "fcomp far end");
    assert_bytes(&image, 0x54e74e, &[0xd8, 0x5e, 0x3c], "fcomp far start");
    assert_bytes(&image, 0x54e75f, &[0xd8, 0x4e, 0x48], "fmul far inverse");
    assert_bytes(&image, 0x54e795, &[0xd8, 0x5e, 0x34], "fcomp near start");
    assert_bytes(&image, 0x54e7a7, &[0xd8, 0x5e, 0x38], "fcomp near end");
    assert_bytes(
        &image,
        0x54e7b5,
        &[0xd8, 0x66, 0x3c],
        "fsub +0x3c in the near ramp",
    );
    assert_bytes(&image, 0x54e7b8, &[0xd8, 0x4e, 0x44], "fmul near inverse");
    // The normalised age (age x 1/lifetime) keys the scale, colour and
    // texture sequences alike.
    assert_bytes(
        &image,
        0x54e7c7,
        &[0xd9, 0x46, 0x30, 0xd8, 0x4e, 0x28],
        "[+0x30] * [+0x28]",
    );
    // The per-frame update adds [0x637a90] x dt to the age.
    assert_bytes(
        &image,
        0x54efc1,
        &[0xd9, 0x05, 0x90, 0x7a, 0x63, 0x00],
        "age factor",
    );
    assert_bytes(&image, 0x54efca, &[0xd8, 0x46, 0x28], "fadd age");
    // PRIORITY biases the sprite depth by (1 + [0xa06fb0] x priority).
    assert_bytes(
        &image,
        0x54e8e9,
        &[0xd9, 0x05, 0xb0, 0x6f, 0xa0, 0x00],
        "fld bias",
    );
    assert_bytes(&image, 0x54e8ef, &[0xd8, 0x4e, 0x54], "fmul priority");
    // Friction: v' = wind + (v - wind) * g(dt * FRICTION) with g = exp(-x)
    // tabulated at 1/51 steps, 0 above 5 and 1 below 0.
    assert_eq!(call_target(&image, 0x54f030), 0x53_e2e0, "friction factor");
    assert_bytes(&image, 0x54f03e, &[0xd8, 0x4e, 0x50], "wind x WIND_FACTOR");
    assert_bytes(
        &image,
        0x53e335,
        &[0xd8, 0x1d, 0xbc, 0x36, 0x60, 0x00],
        "fcomp 5.0",
    );
    // A distance emitter accumulates the anchor's travel and ignores a jump
    // of 200 or more.
    assert_bytes(
        &image,
        0x54fa4c,
        &[0xd8, 0x1d, 0xc4, 0x35, 0x60, 0x00],
        "fcomp 200",
    );
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_parity2_dmg_fx_levels_image_prop_discs_follow_one_engine_flag() {
    let image = load_image();
    assert_bytes(&image, 0x6280dc, b"spin_props_anim\0", "string");
    assert_bytes(&image, 0x6280ec, b"stop_props_anim\0", "string");
    // The two record fields the strings fill: +0x18c spin, +0x190 stop.
    assert_bytes(
        &image,
        0x47b13c,
        &[0x89, 0x87, 0x8c, 0x01, 0x00, 0x00],
        "spin index",
    );
    assert_bytes(
        &image,
        0x47b163,
        &[0x89, 0x87, 0x90, 0x01, 0x00, 0x00],
        "stop index",
    );
    // 0x4b1630 starts the spin animation (reads +0x18c), 0x4b15c0 the stop
    // animation (reads +0x190).
    assert_bytes(
        &image,
        0x4b165d,
        &[0x8b, 0x80, 0x8c, 0x01, 0x00, 0x00],
        "spin read",
    );
    assert_bytes(
        &image,
        0x4b15ed,
        &[0x8b, 0x80, 0x90, 0x01, 0x00, 0x00],
        "stop read",
    );
    assert_eq!(call_target(&image, 0x4b1675), 0x4e_dda0);
    assert_eq!(call_target(&image, 0x4b1605), 0x4e_dda0);
    // The flag-setter edge: bit 0x2 of [+0x2dc] going off calls the stop
    // function, going on calls the spin function. No speed or RPM is read.
    assert_bytes(
        &image,
        0x4b1718,
        &[0xf6, 0x86, 0xdc, 0x02, 0x00, 0x00, 0x02],
        "test [esi+0x2dc], 2",
    );
    assert_eq!(call_target(&image, 0x4b1723), 0x4b_15c0);
    assert_eq!(call_target(&image, 0x4b1739), 0x4b_1630);
    // Spawn starts the spin; death stops it.
    assert_eq!(call_target(&image, 0x47_b9ac), 0x4b_1630);
    assert_eq!(call_target(&image, 0x4b_8440), 0x4b_15c0);
}
