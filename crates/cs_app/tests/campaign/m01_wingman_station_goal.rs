//! M01-WINGMAN-STATION-GOAL-AND-ALTITUDE (#2277): how the follower routine
//! `0x41e760` builds its leader-relative station goal `[esi+0xe0]`, and what
//! moves the original follower's altitude when the stick law's pitch word is
//! scaled by `0.0`.
//!
//! Task test prefix: `accept_m01_wingman_station_goal_`. The answers, with
//! the byte windows these tests pin, are in
//! `docs/findings/2026-11-02-m01-wingman-gains.md` §7. Every test is
//! ignored without `CS_ENGINE_IMAGE`; the claims are `static`.

use cs_content::coordinates::load_engine_image;

use crate::campaign::m01_wingman_gains::bytes_at;

fn f32_at(bytes: &[u8], va: u64) -> f32 {
    f32::from_le_bytes(bytes_at(bytes, va, 4).try_into().expect("four bytes"))
}

/// **Which slot, in which frame, per phase.** The two parameter tables, the
/// player/AI selection, the full-matrix transform of the slot in the phase
/// 1 and 4 bodies, the phase 0 fallback (leader position, 200 m up) and the
/// AI-leader phase force.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_wingman_station_goal_slot_is_the_leader_matrix_times_the_table_vector() {
    let image = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    let bytes = &image.bytes;

    // The slot vectors: player leader (6, 0, 18), any other leader (8, -2, -8).
    let table = |va: u64| [0, 4, 8].map(|d| f32_at(bytes, va + d));
    assert_eq!(table(0x61fb88), [6.0, 0.0, 18.0], "player-leader table");
    assert_eq!(table(0x61fb98), [8.0, -2.0, -8.0], "AI-leader table");

    // `0x41e78a`: `cmp edi, [0x71c298]` (leader vs the player vehicle);
    // equal -> `ebx = 0x61fb88`, else `[ebp+8] = 0x61fb98` and the sub-state
    // `[esi+0xd8]` is forced to 2 (`ecx = 2`) with `[esi+0xdd] = 1`.
    assert_eq!(
        bytes_at(bytes, 0x41e78a, 24),
        &[
            0x3b, 0x3d, 0x98, 0xc2, 0x71, 0x00, 0x75, 0x0f, 0xbb, 0x88, 0xfb, 0x61, 0x00, 0x89,
            0x5d, 0x08, 0xb9, 0x02, 0x00, 0x00, 0x00, 0xeb, 0x1c, 0xb9
        ],
        "cmp edi,[0x71c298] / jne / mov ebx,0x61fb88 / … / jmp 0x41e7bd"
    );
    assert_eq!(
        bytes_at(bytes, 0x41e7a1, 22),
        &[
            0xb9, 0x02, 0x00, 0x00, 0x00, 0xc7, 0x45, 0x08, 0x98, 0xfb, 0x61, 0x00, 0x8b, 0x5d,
            0x08, 0x89, 0x8e, 0xd8, 0x00, 0x00, 0x00, 0xc6
        ],
        "mov ecx,2 / mov [ebp+8],0x61fb98 / mov ebx,[ebp+8] / mov [esi+0xd8],ecx"
    );

    // Phases 1 (`0x41ebc9`) and 4 (`0x41ed35`): the table vector is copied to
    // `[esi+0xe0]`, then rotated in place by the leader's nine floats at
    // `leader+0x180` (`edi+0x180`, rows of 3) and translated by the triple
    // at `leader+0x1a4` (`ebx+0x24..+0x2c`): full orientation, not heading.
    assert_eq!(bytes_at(bytes, 0x41ebc9, 2), &[0x8b, 0x0b], "mov ecx,[ebx]");
    assert_eq!(
        bytes_at(bytes, 0x41ed35, 3),
        &[0x8b, 0x13, 0x8d],
        "mov edx,[ebx]"
    );
    for site in [0x41ebe1_u64, 0x41ed4d] {
        let lea = bytes_at(bytes, site, 6);
        assert_eq!(lea[0], 0x8d, "lea at {site:#x}");
        assert_eq!(
            u32::from_le_bytes(lea[2..6].try_into().expect("disp")),
            0x180,
            "lea reg,[edi+0x180] at {site:#x}"
        );
    }
    // Row 0 times x (`[ebx+0x00,+0x04,+0x08]`), row 1 times y (`+0x0c..`),
    // row 2 times z (`+0x18..`), then `+0x24`, `+0x28`, `+0x2c`.
    assert_eq!(
        bytes_at(bytes, 0x41ebf0, 9),
        &[0x8b, 0x5d, 0x08, 0xd9, 0x00, 0xd8, 0x0b, 0xd9, 0x00],
        "mov ebx,[ebp+8] / fld [eax] / fmul [ebx] / fld [eax]"
    );
    assert_eq!(
        bytes_at(bytes, 0x41ec37, 3),
        &[0xd8, 0x43, 0x24],
        "fadd [ebx+0x24]"
    );
    assert_eq!(
        bytes_at(bytes, 0x41ec3c, 3),
        &[0xd8, 0x43, 0x28],
        "fadd [ebx+0x28]"
    );
    assert_eq!(
        bytes_at(bytes, 0x41ec41, 3),
        &[0xd8, 0x43, 0x2c],
        "fadd [ebx+0x2c]"
    );
    // Then `call [edi->vtable+4]` and its result is the reference velocity.
    assert_eq!(
        bytes_at(bytes, 0x41ec52, 3),
        &[0xff, 0x50, 0x04],
        "call [eax+4]"
    );

    // Phase 0 without a route object (`0x41eb96`): the goal is the leader's
    // position (`edi+0x204`) with `+200.0` on the y word (`0x6035c4`).
    assert_eq!(
        bytes_at(bytes, 0x41eb96, 6),
        &[0x8d, 0x8f, 0x04, 0x02, 0x00, 0x00]
    );
    assert_eq!(
        bytes_at(bytes, 0x41ebb2, 12),
        &[
            0xd9, 0x86, 0xe4, 0x00, 0x00, 0x00, 0xd8, 0x05, 0xc4, 0x35, 0x60, 0x00
        ],
        "fld [esi+0xe4] / fadd [0x6035c4]"
    );
    assert_eq!(f32_at(bytes, 0x6035c4), 200.0);
}

/// **The 80 m push reads an uninitialised local.** `Δ` (own minus leader
/// position) is stored at `[ebp-0x4c]`, but the vector scaled and added to
/// the goal is the one at `[ebp-0x40]`; no store to it exists in the routine.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_wingman_station_goal_push_scales_a_local_the_routine_never_writes() {
    let image = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    let bytes = &image.bytes;

    assert_eq!(f32_at(bytes, 0x6032c8), 0.0, "lower bound of d²");
    assert_eq!(f32_at(bytes, 0x6035c0), 6400.0, "upper bound of d²: 80 m");
    assert_eq!(f32_at(bytes, 0x6035bc), 80.0, "push length");
    // `lea ecx,[ebp-0x4c]` (Δ written there) …
    assert_eq!(bytes_at(bytes, 0x41edde, 3), &[0x8d, 0x4d, 0xb4]);
    // … but `lea ecx,[ebp-0x40]` is the vector rescaled and added.
    assert_eq!(bytes_at(bytes, 0x41ee74, 3), &[0x8d, 0x4d, 0xc0]);
    assert_eq!(
        bytes_at(bytes, 0x41ee65, 8),
        &[0xd9, 0x45, 0xec, 0xd8, 0x3d, 0xbc, 0x35, 0x60],
        "fld [ebp-0x14] / fdivr [0x6035bc]"
    );
}

/// **Nothing in the follower or the law gives the pitch word authority.**
/// The routine stores no stick or throttle word itself, the law's
/// pitch store is scaled by `vehicle+0x8c8`, and the routine returns right
/// after the shared call when it has no route object.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_wingman_station_goal_follower_writes_no_stick_word_itself() {
    let image = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    let bytes = &image.bytes;

    // No disp32 naming +0x108/+0x10c/+0x114/+0x11c/+0x120/+0x124/+0x128
    // anywhere in `0x41e760..0x41f00a`; the two `+0x100` byte matches are
    // the immediates of `mov dword [esi+0xd8], 1`.
    let routine = bytes_at(bytes, 0x41e760, 0x41f00a - 0x41e760);
    for disp in [0x108_u32, 0x10c, 0x114, 0x11c, 0x120, 0x124, 0x128] {
        let pattern = disp.to_le_bytes();
        assert!(
            routine.windows(4).all(|window| window != pattern),
            "no [reg+{disp:#x}] access in the follower routine"
        );
    }
    let hits: Vec<u64> = routine
        .windows(4)
        .enumerate()
        .filter(|(_, window)| *window == 0x100_u32.to_le_bytes())
        .map(|(i, _)| 0x41e760 + i as u64)
        .collect();
    assert_eq!(hits, [0x41e8d6, 0x41ea67]);
    for site in [0x41e8d1_u64, 0x41ea62] {
        assert_eq!(
            bytes_at(bytes, site, 7),
            &[0xc7, 0x86, 0xd8, 0x00, 0x00, 0x00, 0x01],
            "mov dword [esi+0xd8], 1 at {site:#x}"
        );
    }

    // After the shared call: `cmp [esi+0x948], 0 / je 0x41eff9` (the
    // epilogue) — no route object means nothing else runs.
    assert_eq!(
        bytes_at(bytes, 0x41eed8, 14),
        &[
            0x8b, 0x86, 0x48, 0x09, 0x00, 0x00, 0x85, 0xc0, 0x0f, 0x84, 0x13, 0x01, 0x00, 0x00
        ]
    );

    // The law: the low-speed recovery stores ±1.0 into `+0x108`, but step 7
    // multiplies it by the pitch scale (`+0x8c8`, or `+0x8e0` for `bl = 1`).
    assert_eq!(
        bytes_at(bytes, 0x41be28, 10),
        &[0xc7, 0x86, 0x08, 0x01, 0x00, 0x00, 0x00, 0x00, 0x80, 0x3f]
    );
    assert_eq!(
        bytes_at(bytes, 0x41be34, 10),
        &[0xc7, 0x86, 0x08, 0x01, 0x00, 0x00, 0x00, 0x00, 0x80, 0xbf]
    );
    assert_eq!(
        bytes_at(bytes, 0x41be78, 6),
        &[0xd9, 0x86, 0xe0, 0x08, 0x00, 0x00]
    );
    assert_eq!(
        bytes_at(bytes, 0x41be80, 6),
        &[0xd9, 0x86, 0xc8, 0x08, 0x00, 0x00]
    );
    assert_eq!(
        bytes_at(bytes, 0x41be97, 6),
        &[0xd8, 0x8e, 0x08, 0x01, 0x00, 0x00],
        "fmul [esi+0x108]"
    );
}

/// **The terrain look-ahead that latches the escape climb.** `0x41f810` is
/// called every tick from the vehicle update (`0x489abc`) for AI modes 0, 1
/// and 4; below the world floor it latches `+0x358 = 3` outright, else it
/// probes 4.5 s of travel ahead with `0x4c8f70` and latches on a hit. The
/// escape climb then flies the emergency pitch pair.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_wingman_station_goal_escape_climb_is_the_pitch_authority() {
    let image = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    let bytes = &image.bytes;

    // The caller `0x489a65..0x489abc`: AI flag `[esi+0x944]`, `+0x358 != 5`
    // (`0x489a79`), `+0x91f == 0`, mode 0/4/1 (`0x489a90`..), and the call
    // only while `+0x358 <= 3` (`cmp ecx,3 / jg`).
    assert_eq!(
        bytes_at(bytes, 0x489a65, 6),
        &[0x8a, 0x86, 0x44, 0x09, 0x00, 0x00]
    );
    assert_eq!(bytes_at(bytes, 0x489a79, 3), &[0x83, 0xf9, 0x05]);
    assert_eq!(bytes_at(bytes, 0x489ab6, 3), &[0x83, 0xf9, 0x03]);
    let call = bytes_at(bytes, 0x489abc, 5);
    assert_eq!(call[0], 0xe8);
    let rel = i32::from_le_bytes(call[1..5].try_into().expect("rel32"));
    assert_eq!(0x489ac1_u64.wrapping_add(rel as i64 as u64), 0x41f810);

    // `0x41f810`: `y < [0x71c3f0]` latches 3 (`jne 0x41f98e`); `y >=
    // [0x71c3f4]` clears; else the probe from the position plus velocity ×
    // 4.5 (`0x603644`) through `0x4c8f70`, latch at `0x41f98e`, clear at
    // `0x41f9a6` when the latch was 3.
    assert_eq!(
        bytes_at(bytes, 0x41f827, 6),
        &[0xd8, 0x1d, 0xf0, 0xc3, 0x71, 0x00]
    );
    assert_eq!(
        bytes_at(bytes, 0x41f83e, 6),
        &[0xd8, 0x1d, 0xf4, 0xc3, 0x71, 0x00]
    );
    assert_eq!(
        bytes_at(bytes, 0x41f8b5, 6),
        &[0xd8, 0x0d, 0x44, 0x36, 0x60, 0x00]
    );
    assert_eq!(f32_at(bytes, 0x603644), 4.5);
    let probe = bytes_at(bytes, 0x41f94a, 5);
    assert_eq!(probe[0], 0xe8);
    let rel = i32::from_le_bytes(probe[1..5].try_into().expect("rel32"));
    assert_eq!(0x41f94f_u64.wrapping_add(rel as i64 as u64), 0x4c8f70);
    assert_eq!(
        bytes_at(bytes, 0x41f98e, 10),
        &[0xc7, 0x86, 0x58, 0x03, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00]
    );
    assert_eq!(
        bytes_at(bytes, 0x41f9a6, 10),
        &[0xc7, 0x86, 0x58, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]
    );

    // The emergency pitch scale `+0x8e0` ← `def+0x280` and limit `+0x8ec` ←
    // `def+0x28c` (spawn copy), the `ai_emerg_input_*` words.
    assert_eq!(
        bytes_at(bytes, 0x47d355, 6),
        &[0x8b, 0x92, 0x80, 0x02, 0x00, 0x00]
    );
    assert_eq!(bytes_at(bytes, 0x47d35b, 6)[..2], [0x89, 0x90]);
    assert_eq!(
        bytes_at(bytes, 0x47d3ca, 6),
        &[0x8b, 0x92, 0x8c, 0x02, 0x00, 0x00]
    );
    assert_eq!(
        bytes_at(bytes, 0x47d3d0, 6),
        &[0x89, 0x90, 0xec, 0x08, 0x00, 0x00]
    );
}
