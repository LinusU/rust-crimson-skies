//! M01-WINGMAN-GOAL-Y-CEILING (#2300): the follower's goal-`y` clamp —
//! `G.y = clamp(G.y, [0x71c3f0], vehicle+0x314)` at `0x41b57e`..`0x41b5f1`
//! of `0x41b560` — its byte windows, its data source and its M01 value.
//!
//! Task test prefix: `accept_m01_wingman_goal_y_`. The decode is in
//! `docs/findings/2026-11-02-m01-wingman-gains.md` §13:
//!
//! * `vehicle+0x314` is the vehicle definition's `flight_ceiling` key:
//!   the `0x479240` parser looks it up (`push 0x627e14` at `0x479c5b`) and
//!   stores it at `def+0x30` (`0x479c73`), whose constructor default is
//!   `0x447a0000` (1000.0, `0x478a4d`/`0x478a52`), and the spawn copy at
//!   `0x475a3f`/`0x475a42` carries `def+0x30` into `vehicle+0x314`;
//! * the floor `[0x71c3f0]` is the global the world setup writes
//!   `0x41a00000` (20.0) to at `0x47415b` — the same word the escape
//!   latch reads as its own floor (`0x41f829`, `ESCAPE_WORLD_FLOOR_M`);
//! * the `0x4216e0` caller disables the clamp around its own `0x41b560`
//!   call by writing `±FLT_MAX` into both bounds (`0x4216f5`/`0x4216ff`)
//!   and restoring them after (`0x421775`/`0x42177b`).

use cs_app::mission_ai::{
    ESCAPE_GOAL_UP_M, GOAL_Y_FLOOR_M, M01_WINGMAN_GOAL_Y_CEILING_M, PHASE0_LEADER_CLIMB_M,
    goal_y_clamped,
};
use cs_content::original_airframe::{OriginalDocuments, resolve_vehicle_record};
use cs_content::stunts::ZrdValue;

use crate::campaign::m01_wingman_drift::fly_trace;
use crate::campaign::m01_wingman_gains::{
    f32_at, member_document, record_field, text_at, vehicle_records,
};
use crate::campaign::vs_m01_rt_window::game_dir;

/// The image windows §13.2 rests on: the clamp itself, the floor's writer,
/// the `flight_ceiling` key's parse site, the constructor default, the
/// spawn copy into `vehicle+0x314`, and the one caller that disables the
/// clamp around its own `0x41b560` call.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_wingman_goal_y_clamp_windows_match_the_image() {
    use cs_app::engine_facts::engine_facts;
    use cs_content::coordinates::load_engine_image;
    let loaded = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    let facts = engine_facts().expect("the committed table parses");
    let at = |va: u64| usize::try_from(facts.image_offset(va).expect("va in image")).expect("off");
    let bytes = &loaded.bytes;
    let window = |va: u64, want: &[u8]| {
        let i = at(va);
        assert_eq!(&bytes[i..i + want.len()], want, "window at {va:#x}");
    };

    // 1. The clamp inside `0x41b560`: compare the goal copy's `y` against
    //    `vehicle+0x314` (`fcomp`), take the ceiling branch on `test ah,0x41`
    //    not firing, store `+0x314` into the goal copy, and zero the second
    //    argument's `y` when it was positive.
    window(0x41b57e, &[0xd9, 0x45, 0xd0]); // fld [ebp-0x30] = G.y
    window(0x41b583, &[0xd8, 0x9e, 0x14, 0x03, 0x00, 0x00]); // fcomp [esi+0x314]
    window(0x41b59a, &[0xf6, 0xc4, 0x41]); // test ah, 0x41
    window(0x41b5a0, &[0x75, 0x20]); // jne over the ceiling store
    window(0x41b5a5, &[0x8b, 0x96, 0x14, 0x03, 0x00, 0x00]); // mov edx, [esi+0x314]
    window(0x41b5b1, &[0x89, 0x55, 0xd0]); // mov [ebp-0x30], edx  (G.y = ceiling)
    window(0x41b5bb, &[0xc7, 0x45, 0xdc, 0x00, 0x00, 0x00, 0x00]); // arg2.y = 0

    // 2. The floor: `fcomp [0x71c3f0]`, take the branch on `test ah,1` not
    //    firing, then `mov eax, [0x71c3f0]` into the goal copy; arg2.y is
    //    zeroed when it was negative.
    window(0x41b5c5, &[0xd8, 0x1d, 0xf0, 0xc3, 0x71, 0x00]); // fcomp [0x71c3f0]
    window(0x41b5d5, &[0xa1, 0xf0, 0xc3, 0x71, 0x00]); // mov eax, [0x71c3f0]

    // 3. The floor's writer: the world setup stores `0x41a00000` (20.0) —
    //    the same global the escape latch reads (`0x41f829`), matching the
    //    already-measured `ESCAPE_WORLD_FLOOR_M`.
    window(
        0x47415b,
        &[0xc7, 0x05, 0xf0, 0xc3, 0x71, 0x00, 0x00, 0x00, 0xa0, 0x41],
    );
    window(
        0x474151,
        &[0xc7, 0x05, 0xf4, 0xc3, 0x71, 0x00, 0x00, 0x00, 0xfa, 0x45],
    );
    assert_eq!(
        f32_at(bytes, 0x474161),
        GOAL_Y_FLOOR_M,
        "the floor immediate"
    );
    assert_eq!(
        f32_at(bytes, 0x474157),
        8000.0,
        "the ceiling global beside it"
    );

    // 4. The data source: the `flight_ceiling` key text, the `0x479240`
    //    parser's lookup of it (`push 0x627e14`), and the store to
    //    `def+0x30`.
    assert_eq!(text_at(bytes, 0x627e14), "flight_ceiling");
    window(0x479c5b, &[0x68, 0x14, 0x7e, 0x62, 0x00]); // push 0x627e14
    window(0x479c73, &[0x89, 0x4f, 0x30]); // mov [edi+0x30], ecx

    // 5. The definition constructor's default for `def+0x30`:
    //    `0x447a0000` = 1000.0.
    window(0x478a4d, &[0xb8, 0x00, 0x00, 0x7a, 0x44]); // mov eax, 0x447a0000
    window(0x478a52, &[0x89, 0x45, 0x30]); // mov [ebp+0x30], eax

    // 6. The spawn copy `def+0x30` → `vehicle+0x314` (`0x475820`), plus the
    //    second reader `0x47d1d9` that mirrors a non-negative `def+0x30`
    //    into `vehicle+0x980`.
    window(0x475a3f, &[0x8b, 0x4d, 0x30]); // mov ecx, [ebp+0x30]
    window(0x475a42, &[0x89, 0x8b, 0x14, 0x03, 0x00, 0x00]); // mov [ebx+0x314], ecx
    window(0x47d1d9, &[0x8b, 0x43, 0x30]); // mov eax, [ebx+0x30]
    window(0x47d1e3, &[0x89, 0x81, 0x80, 0x09, 0x00, 0x00]); // mov [ecx+0x980], eax

    // 7. The `0x4216e0` caller disables the clamp: `±FLT_MAX` into both
    //    bounds before its own `0x41b560` call (`0x42176a`), restored after.
    window(
        0x4216f5,
        &[0xc7, 0x05, 0xf0, 0xc3, 0x71, 0x00, 0xff, 0xff, 0x7f, 0xff],
    );
    window(
        0x4216ff,
        &[0xc7, 0x86, 0x14, 0x03, 0x00, 0x00, 0xff, 0xff, 0x7f, 0x7f],
    );
    window(0x421775, &[0x89, 0x15, 0xf0, 0xc3, 0x71, 0x00]); // restore floor
    window(0x42177b, &[0x89, 0x86, 0x14, 0x03, 0x00, 0x00]); // restore +0x314
}

/// **The value M01's wingman actually gets.** The retail `wingman` record
/// resolves through `kind_of` to `basic_airplane`, which declares
/// `flight_ceiling` = 2500.0 — inherited unchanged through `devastator`,
/// overriding the constructor's 1000.0. The resolved value is
/// [`M01_WINGMAN_GOAL_Y_CEILING_M`].
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_wingman_goal_y_retail_wingman_ceiling() {
    let root = game_dir();
    let documents = OriginalDocuments::read(&root).expect("the installation reads");
    let resolved = resolve_vehicle_record(documents.vehicle(), "wingman")
        .expect("the retail registry resolves `wingman`");
    let ceiling = resolved
        .value("flight_ceiling")
        .and_then(|node| node.as_scalar_f64())
        .expect("the `wingman` chain carries a numeric flight_ceiling");
    assert!(
        (ceiling as f32 - M01_WINGMAN_GOAL_Y_CEILING_M).abs() < 1.0e-6,
        "the resolved flight_ceiling {ceiling} is the pinned word \
         {M01_WINGMAN_GOAL_Y_CEILING_M} (chain {:?}, source {:?})",
        resolved.chain,
        resolved.source_of("flight_ceiling"),
    );
    assert_eq!(
        resolved.source_of("flight_ceiling"),
        Some("basic_airplane"),
        "the value inherits from `basic_airplane` (chain {:?})",
        resolved.chain
    );
    // The M01 spawn document's wingman_1 record carries no override of its
    // own: `aiv.zrd` is a positional document, and its optional fields end
    // at the `-1.0` pilot-input slots (finding §3), none of which reach
    // `def+0x30`.
    let (aiv, _span) = member_document(&root, "zbd/c3/m01/zrdr.zbd", "aiv.zrd");
    let wingman = {
        let top = aiv.as_list().expect("the aiv document is a list");
        let mut found = None;
        for node in top {
            if let Some(fields) = node.as_list()
                && fields
                    .first()
                    .and_then(ZrdValue::as_text)
                    .is_some_and(|name| name.eq_ignore_ascii_case("wingman_1"))
            {
                found = Some(fields);
            }
        }
        found.expect("the M01 aiv document carries wingman_1")
    };
    assert!(
        !wingman.iter().any(|node| node
            .as_text()
            .is_some_and(|key| key.eq_ignore_ascii_case("flight_ceiling"))),
        "wingman_1's aiv record has no flight_ceiling key"
    );
    // `wingman` itself declares no `flight_ceiling`: the resolved 2500.0 is
    // `basic_airplane`'s, inherited through `devastator`.
    let (vehicles, _span) = member_document(&root, "zbd/zrdr.zbd", "vehicle.zrd");
    let records = vehicle_records(&vehicles);
    let find = |name: &str| {
        records
            .iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
            .map(|(_, record)| *record)
            .unwrap_or_else(|| panic!("the retail registry declares `{name}`"))
    };
    let wingman_def = find("wingman");
    let devastator_def = find("devastator");
    let basic_def = find("basic_airplane");
    assert!(
        record_field(wingman_def, "flight_ceiling").is_none(),
        "`wingman` declares flight_ceiling: {:?}",
        record_field(wingman_def, "flight_ceiling")
    );
    assert!(
        record_field(devastator_def, "flight_ceiling").is_none(),
        "`devastator` declares flight_ceiling: {:?}",
        record_field(devastator_def, "flight_ceiling")
    );
    assert_eq!(
        record_field(basic_def, "flight_ceiling"),
        Some(&ZrdValue::List(vec![ZrdValue::Float(
            M01_WINGMAN_GOAL_Y_CEILING_M
        )])),
        "`basic_airplane` is the declaring record"
    );
}

/// **The clamp never binds on the hillside trace.** The goal point the law
/// sees each row is the station slot (`leader_alt + slot_dy`), the phase-0
/// goal `leader + 200` before the engage latch, or the escape goal
/// `betty + 1000` while latched — all inside
/// `[GOAL_Y_FLOOR_M, M01_WINGMAN_GOAL_Y_CEILING_M]` on every row, so the
/// ported clamp is inert here and §10's 45 s numbers stand.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_wingman_goal_y_never_binds_on_the_hillside_trace() {
    let rows = fly_trace(true);
    assert_eq!(rows.len(), 90);
    for r in &rows {
        let goal_y = if r.climbing {
            r.betty_alt + ESCAPE_GOAL_UP_M
        } else if !r.engaged {
            r.leader_alt + PHASE0_LEADER_CLIMB_M
        } else {
            r.leader_alt + r.slot_dy
        };
        assert!(
            (GOAL_Y_FLOOR_M..=M01_WINGMAN_GOAL_Y_CEILING_M).contains(&goal_y),
            "t={} goal y {goal_y} leaves the band [20, 2500] \
             (climbing={}, engaged={}, slot dy={})",
            r.t,
            r.climbing,
            r.engaged,
            r.slot_dy
        );
        // And the port's own helper is the identity on this row's delta.
        let error = bevy::prelude::Vec3::new(0.0, r.vert, 0.0);
        let here_y = r.leader_alt + r.slot_dy - r.vert;
        let clamped = goal_y_clamped(
            bevy::prelude::Vec3::new(0.0, here_y, 0.0),
            error,
            M01_WINGMAN_GOAL_Y_CEILING_M,
        );
        assert_eq!(clamped, error, "t={} the clamp moves the delta", r.t);
    }
    // The re-flown 45 s row still reads the §10 numbers: the ported clamp
    // never fires, so nothing about the hillside drift changes.
    let last = rows.last().copied().expect("the 45 s row");
    assert!((last.along - 354.5).abs() < 2.0, "along {}", last.along);
    assert!((last.cross - 368.8).abs() < 2.0, "cross {}", last.cross);
    assert!((last.vert - -295.3).abs() < 2.0, "vert {}", last.vert);
    assert!((last.gap - 576.7).abs() < 2.0, "gap {}", last.gap);
}
