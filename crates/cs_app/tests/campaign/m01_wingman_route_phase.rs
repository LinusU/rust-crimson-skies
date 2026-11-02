//! M01-WINGMAN-ROUTE-OBJECT-AND-PHASE (#2289): what the follower routine's
//! "route object" `[esi+0x948]` really is, where it comes from, and which
//! follower phase `[esi+0xd8]` an M01 wingman flies.
//!
//! Task test prefix: `accept_m01_wingman_route_phase_`. The answers, with the
//! byte windows these tests pin, are in
//! `docs/findings/2026-11-02-m01-wingman-gains.md` §8. Image tests are ignored
//! without `CS_ENGINE_IMAGE`, retail tests without `CS_GAME_DIR`; the claims
//! are `static` (`OWNER-STATIC-2026-10-29`), nothing is `verified_original`.

use cs_content::coordinates::load_engine_image;
use cs_content::stunts::ZrdValue;

use crate::campaign::m01_wingman_gains::{
    bytes_at, f32_at, member_document, record_field, text_at, vehicle_records,
};
use crate::campaign::vs_m01_rt_window::game_dir;

fn u32_at(bytes: &[u8], va: u64) -> u32 {
    u32::from_le_bytes(bytes_at(bytes, va, 4).try_into().expect("four bytes"))
}

/// `mov dword ptr [reg + disp32], imm32/reg` windows: the instruction's
/// displacement bytes sit at `va + 2..+6`.
fn disp32_at(bytes: &[u8], va: u64) -> u32 {
    u32::from_le_bytes(bytes_at(bytes, va + 2, 4).try_into().expect("disp"))
}

/// **Where the target handle `[esi+0x948]` comes from.** It is the AI's
/// current *target*, not a route: the constructor nulls it, the upkeep
/// `0x41fe10` (called unconditionally from the AI step `0x41c281`) rebuilds
/// it through the best-hostile-candidate search `0x41f9c0`, and `0x4b0f20`
/// re-arms its refresh timer `+0x94c` by 20 s.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_wingman_route_phase_target_handle_is_nulled_then_rebuilt_by_the_upkeep() {
    let image = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    let bytes = &image.bytes;

    // The vehicle constructor zeroes the handle (`ebx` is zero, `xor ebx,
    // ebx` at 0x4affaf), its timer and the phase word, and the sub-state.
    assert_eq!(bytes_at(bytes, 0x4affaf, 2), &[0x33, 0xdb], "xor ebx, ebx");
    for (site, disp) in [
        (0x4b039f_u64, 0x948_u32),
        (0x4b03b6, 0x94c),
        (0x4b05a3, 0xd8),
        (0x4b08bf, 0x358),
    ] {
        assert_eq!(
            bytes_at(bytes, site, 2),
            &[0x89, 0x9e],
            "mov [esi+d32], ebx"
        );
        assert_eq!(disp32_at(bytes, site), disp, "ctor store at {site:#x}");
    }
    // The ctor also nulls the leader `+0x2fc` (`0x4b00e8`) and the phase's
    // flag bytes `+0xdc`/`+0xdd` (`0x4b05a9`, `0x4b05af`).
    assert_eq!(disp32_at(bytes, 0x4b00e8), 0x2fc);
    assert_eq!(bytes_at(bytes, 0x4b05a9, 2), &[0x88, 0x9e]);
    assert_eq!(disp32_at(bytes, 0x4b05a9), 0xdc);
    assert_eq!(bytes_at(bytes, 0x4b05af, 2), &[0x88, 0x9e]);
    assert_eq!(disp32_at(bytes, 0x4b05af), 0xdd);

    // The AI step: `cmp byte [esi+0xcc], 0` / `jne` then `call 0x41fe10`
    // before the mode dispatch at `0x41c2dd`.
    assert_eq!(
        bytes_at(bytes, 0x41c273, 15),
        &[
            0x8a, 0x86, 0xcc, 0x00, 0x00, 0x00, 0x84, 0xc0, 0x0f, 0x85, 0xe0, 0x00, 0x00, 0x00,
            0xe8
        ],
        "mov al,[esi+0xcc] / test al,al / jne 0x41c361 / call …"
    );
    assert_eq!(
        0x41c281_u32
            .wrapping_add(5)
            .wrapping_add(u32_at(bytes, 0x41c282)),
        0x41fe10,
        "the call at 0x41c281 reaches the upkeep 0x41fe10"
    );
    // It is the only caller (`e8` to 0x41fe10 appears once in `.text`).
    let text = bytes_at(bytes, 0x401000, 0x2014bd - 5);
    let callers = (0..text.len())
        .filter(|&i| {
            text[i] == 0xe8 && {
                let rel = i32::from_le_bytes(text[i + 1..i + 5].try_into().expect("rel"));
                0x401000_i64 + i as i64 + 5 + i64::from(rel) == 0x41fe10
            }
        })
        .count();
    assert_eq!(callers, 1, "one direct caller of 0x41fe10");

    // The upkeep's gate: global byte `[0x64f66e]` set returns at once
    // (`0x41fe1e` load, `0x41fe35` `jne 0x420057`).
    assert_eq!(
        bytes_at(bytes, 0x41fe1e, 5),
        &[0xa0, 0x6e, 0xf6, 0x64, 0x00]
    );
    assert_eq!(bytes_at(bytes, 0x41fe35, 2), &[0x0f, 0x85]);

    // Mode 0 and 4 (the follower is 4) use the scorer with vtable
    // `0x603544`, every other mode `0x60354c` (`0x41fe84`..`0x41fe8f`).
    assert_eq!(u32_at(bytes, 0x603544), 0x421ad0);
    assert_eq!(u32_at(bytes, 0x60354c), 0x421950);
    // Mode 4 skips the leader branch (`cmp eax,4 / je 0x41ff8e` at 0x41feb0).
    assert_eq!(bytes_at(bytes, 0x41feb0, 8)[..3], [0x83, 0xf8, 0x04]);
    assert_eq!(bytes_at(bytes, 0x41feb3, 2), &[0x0f, 0x84]);
    // At `0x41ff8e` a null handle (`je 0x420002`) or an expired timer
    // `[esi+0x94c] <= [0x71c470]` (`0x41ff98`..`0x41ffa9`) rebuilds it with
    // `0x41f9c0(scorer, [esi+0x344], 0x420070(esi, 1))` (`0x420014`).
    assert_eq!(
        bytes_at(bytes, 0x41ff8e, 6),
        &[0x8b, 0x8e, 0x48, 0x09, 0x00, 0x00]
    );
    assert_eq!(
        bytes_at(bytes, 0x41ff98, 6),
        &[0xd9, 0x86, 0x4c, 0x09, 0x00, 0x00]
    );
    assert_eq!(bytes_at(bytes, 0x420014, 1), &[0xe8]);
    assert_eq!(
        0x420014_u32
            .wrapping_add(5)
            .wrapping_add(u32_at(bytes, 0x420015)),
        0x41f9c0
    );
    assert_eq!(
        bytes_at(bytes, 0x420031, 2),
        &[0x89, 0xbe],
        "mov [esi+0x948], edi"
    );
    assert_eq!(disp32_at(bytes, 0x420031), 0x948);
    // The leader-driven store (modes other than 4): `0x41ff5a`.
    assert_eq!(disp32_at(bytes, 0x41ff5a), 0x948);

    // `0x4b0f20`: `+0x94c = [0x71c470] + [0x6035e4]` and `ret`.
    assert_eq!(
        bytes_at(bytes, 0x4b0f20, 13),
        &[
            0xd9, 0x05, 0x70, 0xc4, 0x71, 0x00, 0xd8, 0x05, 0xe4, 0x35, 0x60, 0x00, 0xd9
        ]
    );
    assert_eq!(f32_at(bytes, 0x6035e4), 20.0, "the re-arm interval");
    // Acceptance threshold for a candidate and the "invalid" score.
    assert_eq!(f32_at(bytes, 0x6034c0), 1.0e20);
    assert_eq!(f32_at(bytes, 0x6034bc), 1.0e21);
}

/// **What the search looks at.** The scorer is built from the AI's team
/// (`lea eax,[esi+8]` at `0x41fe3f`, copied to scorer `+0x10` by `0x422850`),
/// and `0x422890` asks each candidate's virtual `+0x34` with that team: a
/// *hostility* filter. The scorer's range gate reads the AI's `+0x328`
/// (squared horizontal range) and `+0x32c`/`+0x330` (vertical band), which
/// the definition loader fills from the `attack` key, and the biases
/// `+0x340`/`+0x344` come from `target_bias`/`struct_bias`.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_wingman_route_phase_search_is_hostile_candidates_inside_the_attack_range() {
    let image = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    let bytes = &image.bytes;

    // `0x41fe3f..0x41fe4a`: `lea eax,[esi+8]` / `push eax` / `call [edx]` /
    // `push eax` / `lea ecx,[esp+0x1c]` / `call 0x422850`.
    assert_eq!(bytes_at(bytes, 0x41fe3f, 3), &[0x8d, 0x46, 0x08]);
    assert_eq!(
        0x41fe4a_u32
            .wrapping_add(5)
            .wrapping_add(u32_at(bytes, 0x41fe4b)),
        0x422850
    );
    // `0x422850` stores the team word at scorer `+0x10` and the flag at
    // `+0x14`; `0x422890` calls the candidate's virtual `+0x34` with
    // `&scorer.team` (`lea ecx,[esi+0x10]` / `call [eax+0x34]`).
    assert_eq!(bytes_at(bytes, 0x422874, 3), &[0x89, 0x50, 0x10]);
    assert_eq!(bytes_at(bytes, 0x422877, 3), &[0x88, 0x48, 0x14]);
    assert_eq!(
        bytes_at(bytes, 0x42289a, 9),
        &[0x8d, 0x4e, 0x10, 0x51, 0x8b, 0xcf, 0xff, 0x50, 0x34]
    );
    // The scorer `0x421ad0` range gate: horizontal squared distance against
    // the AI's `+0x328`, y against `+0x32c` and `+0x330`.
    assert_eq!(disp32_at(bytes, 0x421b47), 0x328);
    assert_eq!(bytes_at(bytes, 0x421b47, 2), &[0xd8, 0x99]);
    assert_eq!(bytes_at(bytes, 0x421b54, 2), &[0xd9, 0x81]);
    assert_eq!(disp32_at(bytes, 0x421b54), 0x32c);
    assert_eq!(bytes_at(bytes, 0x421b67, 2), &[0xd8, 0x99]);
    assert_eq!(disp32_at(bytes, 0x421b67), 0x330);

    // The loader (`0x475a5f..0x475c8e`, `ebp` = the definition) copies
    // `def+0xa4` -> mode `+0x67c`, `def+0x44/0x48/0x4c` -> `+0x328/32c/330`,
    // `def+0x138/0x13c` -> `+0x340/+0x344`.
    for (site, dest) in [
        (0x475a62_u64, 0x328_u32),
        (0x475a6b, 0x32c),
        (0x475a74, 0x330),
        (0x475abf, 0x67c),
        (0x475c82, 0x340),
        (0x475c8e, 0x344),
    ] {
        assert_eq!(disp32_at(bytes, site), dest, "loader store at {site:#x}");
    }
    // …and the definition parser writes those fields from the keys
    // `attack` (`0x627e30`), `target_bias` (`0x627e7c`) and `struct_bias`
    // (`0x627e88`); the constructor defaults are 160000/-400/400 and 0/0.
    assert_eq!(text_at(bytes, 0x627e30), "attack");
    assert_eq!(text_at(bytes, 0x627e38), "pursuit_range");
    assert_eq!(text_at(bytes, 0x627e7c), "target_bias");
    assert_eq!(text_at(bytes, 0x627e88), "struct_bias");
    // `fstp [edi+0x44]` (attack²), `mov [edi+0x4c], ecx`, `fstp [edi+0x48]`.
    assert_eq!(bytes_at(bytes, 0x479d11, 3), &[0xd9, 0x5f, 0x44]);
    assert_eq!(bytes_at(bytes, 0x479d27, 3), &[0xd9, 0x5f, 0x48]);
    assert_eq!(
        u32_at(bytes, 0x478a69 + 3),
        0x481c_4000,
        "default +0x44: 160000"
    );
    assert_eq!(
        u32_at(bytes, 0x478a70 + 3),
        0xc3c8_0000,
        "default +0x48: -400"
    );
    assert_eq!(
        u32_at(bytes, 0x478a77 + 3),
        0x43c8_0000,
        "default +0x4c: 400"
    );
}

/// **Every writer of the phase word `[esi+0xd8]`.** The constructor zeroes it
/// (phase 0 at spawn); inside the follower routine the only stores are the AI
/// leader's forced 2 (`0x41e7b0`), 2 -> 1 without a target (`0x41e8d1`),
/// 3/4 -> 2 with a target (`0x41e9f6`/`0x41ea37`, `ecx` = 2 from `0x41e79a`/
/// `0x41e7a1`), 3 -> 4 near the goal (`0x41ea21`) and the 0/2/4 -> 1 joins
/// (`0x41ea62`). Phase 1 has no exit in the first table (`0x41f010` is the
/// join `0x41ea6c`).
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_wingman_route_phase_phase_word_writers_and_transitions() {
    let image = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    let bytes = &image.bytes;

    // The phase-word stores inside `0x41e760..0x41f00a` and the constructor.
    // `mov [esi+0xd8], ecx` = 89 8e d8 00 00 00; `…, imm32` = c7 86 d8 …
    let ecx_store = [0x89, 0x8e, 0xd8, 0x00, 0x00, 0x00];
    let imm = |value: u8| [0xc7, 0x86, 0xd8, 0x00, 0x00, 0x00, value, 0x00, 0x00, 0x00];
    assert_eq!(
        bytes_at(bytes, 0x41e7b0, 6),
        &ecx_store,
        "AI leader: = ecx (2)"
    );
    assert_eq!(bytes_at(bytes, 0x41e8d1, 10), &imm(1), "2 w/o target -> 1");
    assert_eq!(
        bytes_at(bytes, 0x41e9f6, 6),
        &ecx_store,
        "3 with target -> ecx (2)"
    );
    assert_eq!(bytes_at(bytes, 0x41ea21, 10), &imm(4), "3 near goal -> 4");
    assert_eq!(
        bytes_at(bytes, 0x41ea37, 6),
        &ecx_store,
        "4 with target -> ecx (2)"
    );
    assert_eq!(bytes_at(bytes, 0x41ea62, 10), &imm(1), "join -> 1");
    // `ecx` is 2 on every path into those stores: `mov ecx, 2` at 0x41e79a
    // (player leader) and 0x41e7a1 (any other leader); nothing between
    // 0x41e7bd and the dispatch at 0x41e82d writes `ecx`.
    assert_eq!(
        bytes_at(bytes, 0x41e79a, 5),
        &[0xb9, 0x02, 0x00, 0x00, 0x00]
    );
    assert_eq!(
        bytes_at(bytes, 0x41e7a1, 5),
        &[0xb9, 0x02, 0x00, 0x00, 0x00]
    );

    // The first table (`0x41f00c`): phase 0 -> 0x41e834, 1 -> the join
    // 0x41ea6c, 2 -> 0x41e8bb, 3 -> 0x41e9ec, 4 -> 0x41ea2d.
    let first = [0x41e834, 0x41ea6c, 0x41e8bb, 0x41e9ec, 0x41ea2d];
    for (index, target) in first.into_iter().enumerate() {
        assert_eq!(u32_at(bytes, 0x41f00c + 4 * index as u64), target);
    }
    // The second table (`0x41f020`): the bodies of §7.1.
    let second = [0x41ea9c, 0x41ebc9, 0x41ec5a, 0x41edd2, 0x41ed35];
    for (index, target) in second.into_iter().enumerate() {
        assert_eq!(u32_at(bytes, 0x41f020 + 4 * index as u64), target);
    }

    // Phase 0 -> 1 needs: the leader awake (`[edi+0x91d] == 0`,
    // `0x41e834`), the squared gap under `0x6035e0` (490000 = 700²),
    // own speed `[esi+0x934]` above `0x603594` (20.576 m/s).
    assert_eq!(
        bytes_at(bytes, 0x41e834, 6),
        &[0x8a, 0x87, 0x1d, 0x09, 0x00, 0x00]
    );
    assert_eq!(f32_at(bytes, 0x6035e0), 490_000.0);
    assert_eq!(
        bytes_at(bytes, 0x41e869, 6),
        &[0xd9, 0x86, 0x34, 0x09, 0x00, 0x00]
    );
    assert_eq!(f32_at(bytes, 0x603594), 20.576);
    // Phase 0's body falls back to leader + 200 m on y without a target
    // (`0x6035c4`), and uses the target within 1800² with one (`0x6035c8`).
    assert_eq!(f32_at(bytes, 0x6035c4), 200.0);
    assert_eq!(cs_app::mission_ai::PHASE0_LEADER_CLIMB_M, 200.0);
    assert_eq!(
        u32_at(bytes, 0x6035c8),
        0x4a45_c0ff,
        "3239999.75, 1800² in f32"
    );

    // No other store writes `[esi+0xd8]` in the follower/AI code
    // (`0x41c270..0x4222ff`): scan the window for `89 xx d8 00 00 00` /
    // `c7 86 d8 00 00 00` with a `[esi+disp32]` ModRM.
    let window = bytes_at(bytes, 0x41c270, 0x4222ff - 0x41c270);
    let mut stores = Vec::new();
    for i in 0..window.len().saturating_sub(6) {
        let modrm_esi = matches!(
            window[i + 1],
            0x86 | 0x8e | 0x96 | 0x9e | 0xa6 | 0xae | 0xb6 | 0xbe
        );
        let store =
            (window[i] == 0x89 && modrm_esi) || (window[i] == 0xc7 && window[i + 1] == 0x86);
        if store && window[i + 2..i + 6] == [0xd8, 0x00, 0x00, 0x00] {
            stores.push(0x41c270 + i as u64);
        }
    }
    assert_eq!(
        stores,
        vec![0x41e7b0, 0x41e8d1, 0x41e9f6, 0x41ea21, 0x41ea37, 0x41ea62],
        "the only phase-word stores in the AI code"
    );
}

/// **Retail inputs.** The wingman's `attack` (and so `+0x328..+0x330`),
/// `target_bias`, `struct_bias` and `pursuit_range` come from the
/// `wingman` -> `devastator` -> `basic_airplane` chain of `vehicle.zrd`; M01's
/// `wingman_1` record carries no key of its own for them (they are definition
/// words, not instance slots), and its primary target resolves to the player
/// vehicle, the leader.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_wingman_route_phase_retail_definition_keys() {
    let root = game_dir();
    let (vehicles, _span) = member_document(&root, "zbd/zrdr.zbd", "vehicle.zrd");
    let records = vehicle_records(&vehicles);
    let find = |wanted: &str| {
        records
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(wanted))
            .map(|(_, record)| *record)
            .unwrap_or_else(|| panic!("the registry declares {wanted}"))
    };
    let value = |name: &str, key: &str| match record_field(find(name), key) {
        Some(ZrdValue::List(items)) => items.clone(),
        other => panic!("{name}.{key}: {other:?}"),
    };
    // The chain: wingman -> devastator -> basic_airplane.
    assert_eq!(
        value("wingman", "kind_of"),
        vec![ZrdValue::Text("devastator".into())]
    );
    assert_eq!(
        value("devastator", "kind_of"),
        vec![ZrdValue::Text("basic_airplane".into())]
    );
    assert!(record_field(find("basic_airplane"), "kind_of").is_none());
    // `attack` (`def+0x44/0x48/0x4c`): only `basic_airplane` declares it,
    // 2000 m, so the merged wingman has attack² 4,000,000 and a band of
    // -2000..2000 (single component: `0x479d11`..`0x479d27`).
    assert_eq!(
        value("basic_airplane", "attack"),
        vec![ZrdValue::Float(2000.0)]
    );
    for name in ["wingman", "devastator"] {
        assert!(
            record_field(find(name), "attack").is_none(),
            "{name}.attack"
        );
    }
    // The biases are the wingman's own: negative, i.e. preferred.
    assert_eq!(
        value("wingman", "target_bias"),
        vec![ZrdValue::Float(-100.0)]
    );
    assert_eq!(
        value("wingman", "struct_bias"),
        vec![ZrdValue::Float(-200.0)]
    );
    // `pursuit_range` is declared nowhere in the chain.
    for name in ["wingman", "devastator", "basic_airplane"] {
        assert!(
            record_field(find(name), "pursuit_range").is_none(),
            "{name}.pursuit_range"
        );
    }
}

/// **The port's phase 0 is short but real.** In the real M01 Betty's engage
/// latch (`follow_engaged`, the port's phase 0 -> 1) fires after the first
/// few decisions (measured: tick 58, decision 60, i.e. once her speed passes
/// 20.576 m/s), so the leader + 200 m goal of phase 0 is flown for about half
/// a second and for no longer than the first second.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_wingman_route_phase_betty_leaves_phase_zero_inside_the_first_second() {
    use bevy::prelude::{App, Entity};
    use bevy::time::{Time, TimeUpdateStrategy};
    use cs_app::mission_actors::ScriptedAircraftActor;
    use cs_app::mission_ai::MissionAircraftAi;
    use cs_app::mission_launch::plan_mission_launch;
    use cs_app::mission_session::{build_headless, stage_for, teardown};

    use crate::campaign::common::label;

    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure").expect("plans");
    let stage = stage_for(&root, &plan).expect("stage reads");
    let mut app: App = build_headless(&stage).expect("composes");
    app.update();
    let betty: Entity = {
        let world = app.world_mut();
        world
            .query::<(Entity, &ScriptedAircraftActor)>()
            .iter(world)
            .find(|(_, actor)| actor.record.name == "wingman_1")
            .map(|(entity, _)| entity)
            .expect("wingman_1")
    };
    let timestep = app.world().resource::<Time<bevy::time::Fixed>>().timestep();
    let mut first_engaged = None;
    for tick in 1..=240_u64 {
        *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
            TimeUpdateStrategy::ManualDuration(timestep);
        app.update();
        let ai = app
            .world()
            .get::<MissionAircraftAi>(betty)
            .expect("Betty's AI");
        if ai.follow_engaged && first_engaged.is_none() {
            first_engaged = Some((tick, ai.decisions));
        }
    }
    teardown(&mut app);
    let (tick, decisions) = first_engaged.expect("Betty engages within 2 s");
    assert!(
        (2..=120).contains(&tick) && decisions > 1,
        "engaged at tick {tick}, decision {decisions}"
    );
}
