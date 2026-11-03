//! M01-WINGMAN-4209B0-ROLE (#2297): the second AI stick routine `0x4209b0`
//! is the mode-0 scripted attitude-program driver. Its two call sites are
//! the `+0x358` = 1 slot of a `+0x67c` = 0 switch, so the mode-4 follower —
//! M01's player-led wingman — can never reach it in any phase.
//!
//! Task test prefix: `accept_m01_wingman_4209b0_`. The decode is
//! `docs/findings/2026-11-02-m01-wingman-gains.md` §11. Every image pin
//! uses `cs_content::coordinates::load_engine_image` — the production
//! loader — and is `#[ignore]`d without `CS_ENGINE_IMAGE`. All claims are
//! `OWNER-STATIC-2026-10-29` static reads; nothing is `verified_original`.

use cs_app::mission_ai::{M01_WINGMAN_PILOT_GAINS, StickWords};

use crate::campaign::m01_wingman_gains::{bytes_at, f32_at};

/// The rel32 target of the `e8` call at `va`.
fn call_target(bytes: &[u8], va: u64) -> u64 {
    let window = bytes_at(bytes, va, 5);
    assert_eq!(window[0], 0xe8, "a rel32 call at {va:#x}");
    let rel = i32::from_le_bytes(window[1..5].try_into().expect("rel32")) as i64;
    (va as i64 + 5 + rel) as u64
}

/// The 32-bit immediate of the `jmp [4*reg + table]` at `va` (`ff 24 <sib>`).
fn jump_table_base(bytes: &[u8], va: u64) -> u64 {
    let window = bytes_at(bytes, va, 7);
    assert_eq!(
        &window[..2],
        &[0xff, 0x24],
        "an indirect jmp at {va:#x}: {window:02x?}"
    );
    u32::from_le_bytes(window[3..7].try_into().expect("disp32")) as u64
}

/// Every `e8` rel32 call site in the image whose target is `target`, in
/// image order — the byte-scan census §11's "the only callers" claims use.
fn e8_sites_to(bytes: &[u8], target: u64) -> Vec<u64> {
    (0..bytes.len().saturating_sub(5))
        .filter(|&i| bytes[i] == 0xe8)
        .map(|i| i as u64 + 0x400000)
        .filter(|&va| {
            let rel =
                i32::from_le_bytes(bytes_at(bytes, va, 5)[1..5].try_into().expect("rel32")) as i64;
            va as i64 + 5 + rel == target as i64
        })
        .collect()
}

/// The `mov [reg+0x358], …` census of §11.1: `c7` immediate stores as
/// `(site, value)` pairs and `89` register stores as a site list (their
/// values ride the register). The `modrm` low-three-bits = 4 case is a
/// SIB encoding whose disp32 sits one byte later; none exists here.
fn substate_writers(bytes: &[u8]) -> (Vec<(u64, u32)>, Vec<u64>) {
    let mut immediates = Vec::new();
    let mut stores = Vec::new();
    for i in 0..bytes.len().saturating_sub(10) {
        let (op, modrm) = (bytes[i], bytes[i + 1]);
        if !(0x80..0xc0).contains(&modrm) || modrm & 0x07 == 0x04 {
            continue;
        }
        let va = i as u64 + 0x400000;
        if bytes_at(bytes, va + 2, 4) != [0x58, 0x03, 0x00, 0x00] {
            continue;
        }
        match op {
            0xc7 => immediates.push((
                va,
                u32::from_le_bytes(bytes_at(bytes, va + 6, 4).try_into().expect("imm32")),
            )),
            0x89 => stores.push(va),
            _ => {}
        }
    }
    (immediates, stores)
}

/// **The call sites.** Both are the `+0x358` = 1 slot of a `+0x67c` = 0
/// sub-state switch inside the mode-0 laws `0x41d1f0` (maneuver, `+0x2f0`
/// ∈ {0,2}) and `0x41d9f0` (pursuit, `+0x2f0` = 1) — and the scan result:
/// those are the *only* `e8` call sites reaching `0x4209b0`.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_wingman_4209b0_call_sites_are_substate1_of_mode0_switches() {
    let image =
        cs_content::coordinates::load_engine_image().expect("CS_ENGINE_IMAGE names the image");
    let bytes = &image.bytes;

    // The two call instructions reach `0x4209b0` — and they are the only
    // `e8` call sites in the image that do (§11.1's byte scan).
    assert_eq!(call_target(bytes, 0x41d4e9), 0x4209b0, "caller 0x41d4e9");
    assert_eq!(call_target(bytes, 0x41da69), 0x4209b0, "caller 0x41da69");
    assert_eq!(
        e8_sites_to(bytes, 0x4209b0),
        &[0x41d4e9, 0x41da69],
        "the e8 census reaches 0x4209b0 from exactly these two sites"
    );

    // `0x41d1f0`'s mode gate: `+0x67c` != 0 jumps to `0x41d671`, not the
    // switch. `8b 96 7c 06 00 00 85 d2 0f 85 bd 03 00 00`.
    assert_eq!(
        bytes_at(bytes, 0x41d2a6, 14),
        &[
            0x8b, 0x96, 0x7c, 0x06, 0x00, 0x00, 0x85, 0xd2, 0x0f, 0x85, 0xbd, 0x03, 0x00, 0x00
        ],
        "mov edx,[esi+0x67c]; test edx,edx; jne 0x41d671"
    );
    // `+0x358` > 3 falls out (`ja 0x41d4fd`); the switch indexes `0x41d8d4`.
    assert_eq!(
        bytes_at(bytes, 0x41d2b4, 8),
        &[0x8b, 0x96, 0x58, 0x03, 0x00, 0x00, 0x83, 0xfa],
        "mov edx,[esi+0x358]; cmp edx,3"
    );
    assert_eq!(
        bytes_at(bytes, 0x41d2bd, 6),
        &[0x0f, 0x87, 0x3a, 0x02, 0x00, 0x00],
        "ja 0x41d4fd: +0x358 > 3 leaves the switch"
    );
    assert_eq!(jump_table_base(bytes, 0x41d2c3), 0x41d8d4);
    let table = bytes_at(bytes, 0x41d8d4, 16);
    let entry = |i: usize| u32::from_le_bytes(table[4 * i..4 * i + 4].try_into().expect("entry"));
    assert_eq!(entry(0), 0x41d30b, "sub-state 0: the maneuver leg");
    assert_eq!(entry(1), 0x41d4e7, "sub-state 1: the 0x4209b0 call site");
    assert_eq!(entry(2), 0x41d4f0, "sub-state 2: the 0x4216e0 call site");
    assert_eq!(entry(3), 0x41d2ca, "sub-state 3: the climb call");
    assert_eq!(
        call_target(bytes, 0x41d4f2),
        0x4216e0,
        "sub-state 2 is the route driver"
    );

    // `0x41d9f0`'s mode gate: `+0x67c` != 0 jumps to `0x41e2f5`.
    assert_eq!(
        bytes_at(bytes, 0x41da43, 14),
        &[
            0x8b, 0x86, 0x7c, 0x06, 0x00, 0x00, 0x85, 0xc0, 0x0f, 0x85, 0xa4, 0x08, 0x00, 0x00
        ],
        "mov eax,[esi+0x67c]; test eax,eax; jne 0x41e2f5"
    );
    assert_eq!(
        bytes_at(bytes, 0x41da51, 8),
        &[0x8b, 0x86, 0x58, 0x03, 0x00, 0x00, 0x83, 0xf8],
        "mov eax,[esi+0x358]; cmp eax,3"
    );
    assert_eq!(
        bytes_at(bytes, 0x41da5a, 6),
        &[0x0f, 0x87, 0x34, 0x0b, 0x00, 0x00],
        "ja 0x41e594: +0x358 > 3 leaves the switch"
    );
    assert_eq!(jump_table_base(bytes, 0x41da60), 0x41e744);
    let table = bytes_at(bytes, 0x41e744, 16);
    let entry = |i: usize| u32::from_le_bytes(table[4 * i..4 * i + 4].try_into().expect("entry"));
    assert_eq!(entry(0), 0x41dac1, "sub-state 0: the pursuit leg");
    assert_eq!(entry(1), 0x41da67, "sub-state 1: the 0x4209b0 call site");
    assert_eq!(entry(2), 0x41da73, "sub-state 2: the 0x4216e0 call site");
    assert_eq!(entry(3), 0x41da80, "sub-state 3: the climb call");
    assert_eq!(
        call_target(bytes, 0x41da75),
        0x4216e0,
        "sub-state 2 is the route driver"
    );
}

/// **The mode dispatch.** `+0x67c` = 4 goes to `0x41e760`, the follower;
/// only `+0x67c` ≠ 4 reaches the `+0x2f0` split, so a mode-4 wingman never
/// sees either `0x4209b0` caller — in any `[ai+0xd8]` phase.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_wingman_4209b0_mode4_dispatches_to_the_follower() {
    let image =
        cs_content::coordinates::load_engine_image().expect("CS_ENGINE_IMAGE names the image");
    let bytes = &image.bytes;

    // `cmp dword [esi+0x67c], 4; je 0x41c358` then `push esi; call 0x41e760`.
    assert_eq!(
        bytes_at(bytes, 0x41c2dd, 8),
        &[0x83, 0xbe, 0x7c, 0x06, 0x00, 0x00, 0x04, 0x74],
        "cmp [esi+0x67c],4 / je"
    );
    assert_eq!(bytes_at(bytes, 0x41c358, 1), &[0x56], "push esi");
    assert_eq!(
        call_target(bytes, 0x41c359),
        0x41e760,
        "mode 4 calls the follower 0x41e760"
    );
    assert_eq!(
        e8_sites_to(bytes, 0x41e760),
        &[0x41c359],
        "the e8 census calls the follower only from the mode-4 dispatch"
    );

    // The `+0x2f0` dispatch on `+0x67c` != 4: {0,2} -> `0x41d1f0`,
    // {1} -> `0x41d9f0`, anything else -> ret.
    assert_eq!(
        bytes_at(bytes, 0x41c2e6, 6),
        &[0x8b, 0x86, 0xf0, 0x02, 0x00, 0x00],
        "mov eax,[esi+0x2f0]"
    );
    assert_eq!(
        bytes_at(bytes, 0x41c2ec, 11),
        &[
            0x83, 0xe8, 0x00, 0x74, 0x13, 0x48, 0x74, 0x05, 0x48, 0x74, 0x0d
        ],
        "sub eax,0 / je case0 / dec / je case1 / dec / je case2->0x41c304"
    );
    assert_eq!(
        bytes_at(bytes, 0x41c2f7, 2),
        &[0x5e, 0xc3],
        "other +0x2f0 values return"
    );
    assert_eq!(
        call_target(bytes, 0x41c2fa),
        0x41d9f0,
        "+0x2f0 = 1 calls the pursuit law"
    );
    assert_eq!(
        call_target(bytes, 0x41c305),
        0x41d1f0,
        "+0x2f0 in {{0,2}} calls the maneuver law"
    );

    // The vehicle update's own gates on `+0x358` before `0x41c270` runs:
    // `+0x358` = 5 skips the step (`0x489a73`..`0x489a82`), a non-null
    // `+0x9bc` raises `+0x358` to 2 — a pending route outranks the program.
    assert_eq!(
        bytes_at(bytes, 0x489a73, 6),
        &[0x8b, 0x8e, 0x58, 0x03, 0x00, 0x00],
        "mov ecx,[esi+0x358]"
    );
    assert_eq!(
        bytes_at(bytes, 0x489a79, 9),
        &[0x83, 0xf9, 0x05, 0x0f, 0x84, 0xa0, 0x00, 0x00, 0x00],
        "cmp ecx,5; je 0x489b22 — +0x358 = 5 jumps past the AI-step call"
    );
    assert_eq!(
        bytes_at(bytes, 0x489b03, 5),
        &[0xb8, 0x02, 0x00, 0x00, 0x00],
        "mov eax,2"
    );
    assert_eq!(
        bytes_at(bytes, 0x489b0c, 6),
        &[0x89, 0x86, 0x58, 0x03, 0x00, 0x00],
        "mov [esi+0x358],eax with eax = 2 (route arm)"
    );
    assert_eq!(
        call_target(bytes, 0x489b14),
        0x41c270,
        "the vehicle update calls the AI step"
    );
}

/// **The program walker and the throttle law.** The vector of 32-byte
/// entries `+0x9a4`..`+0x9a8` at index `+0x9b0`; per leg `+0x89c` =
/// `clock + dwell`; the throttle `+0x124` slews `dt·0.35` toward
/// 80.467 m/s clamped to [0.8, 1.1]; legs advance on a small residual
/// angle plus the dwell, exhaustion clears `+0x358` to 0.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_wingman_4209b0_program_walker_throttle_and_leg_advance() {
    let image =
        cs_content::coordinates::load_engine_image().expect("CS_ENGINE_IMAGE names the image");
    let bytes = &image.bytes;

    // `+0x9b0` = -1 reinitialises to 0 and arms the first dwell.
    assert_eq!(
        bytes_at(bytes, 0x4209b9, 10),
        &[0x8b, 0x86, 0xb0, 0x09, 0x00, 0x00, 0x57, 0x33, 0xff, 0x83],
        "mov eax,[esi+0x9b0]; push edi; xor edi,edi; cmp eax,-1"
    );
    assert_eq!(
        bytes_at(bytes, 0x4209c7, 6),
        &[0x89, 0xbe, 0xb0, 0x09, 0x00, 0x00],
        "mov [esi+0x9b0],edi (0)"
    );
    assert_eq!(
        bytes_at(bytes, 0x4209cd, 6),
        &[0x8b, 0x8e, 0xa4, 0x09, 0x00, 0x00],
        "mov ecx,[esi+0x9a4] (vector begin)"
    );
    assert_eq!(
        bytes_at(bytes, 0x4209dd, 11),
        &[
            0x8b, 0x96, 0xa8, 0x09, 0x00, 0x00, 0x2b, 0xd1, 0xc1, 0xfa, 0x05
        ],
        "mov edx,[esi+0x9a8]; sub edx,ecx; sar edx,5 (32-byte entries)"
    );
    assert_eq!(
        bytes_at(bytes, 0x4209f0, 14),
        &[
            0xc1, 0xe0, 0x05, 0xd9, 0x44, 0x08, 0x10, 0xd8, 0x05, 0x70, 0xc4, 0x71, 0x00, 0xd9
        ],
        "shl eax,5; fld [eax+ecx+0x10] (dwell); fadd [0x71c470] (clock); fstp [esi+0x89c]"
    );

    // The throttle slew: own speed `+0x934` vs the 80.467 m/s target.
    assert_eq!(
        bytes_at(bytes, 0x420a03, 6),
        &[0xd9, 0x86, 0x34, 0x09, 0x00, 0x00],
        "fld [esi+0x934] (own speed)"
    );
    assert_eq!(
        bytes_at(bytes, 0x420a09, 6),
        &[0xd8, 0x1d, 0xb8, 0x36, 0x60, 0x00],
        "fcomp [0x6036b8]"
    );
    assert_eq!(f32_at(bytes, 0x6036b8), f32::from_bits(0x42a0_ef35));
    assert_eq!(
        bytes_at(bytes, 0x420a0f, 12),
        &[
            0xd9, 0x05, 0x44, 0xd7, 0x9a, 0x00, 0xd8, 0x0d, 0x08, 0x35, 0x60, 0x00
        ],
        "fld [0x9ad744] (dt); fmul [0x603508] (0.35/s)"
    );
    assert_eq!(f32_at(bytes, 0x603508), f32::from_bits(0x3eb3_3333));
    assert_eq!(
        bytes_at(bytes, 0x420a22, 6),
        &[0xd8, 0x86, 0x24, 0x01, 0x00, 0x00],
        "fadd [esi+0x124] (throttle up)"
    );
    assert_eq!(
        bytes_at(bytes, 0x420a2a, 6),
        &[0xd8, 0xae, 0x24, 0x01, 0x00, 0x00],
        "fsubr [esi+0x124] (throttle down)"
    );
    assert_eq!(
        bytes_at(bytes, 0x420a3c, 6),
        &[0xd9, 0x05, 0x6c, 0xfb, 0x61, 0x00],
        "fld [0x61fb6c] (1.1 clamp)"
    );
    assert_eq!(f32_at(bytes, 0x61fb6c), f32::from_bits(0x3f8c_cccd));
    assert_eq!(
        bytes_at(bytes, 0x420a5f, 6),
        &[0xd8, 0x1d, 0x68, 0xfb, 0x61, 0x00],
        "fcomp [0x61fb68] (0.8 clamp)"
    );
    assert_eq!(f32_at(bytes, 0x61fb68), f32::from_bits(0x3f4c_cccd));

    // Leg advance: `0x53fca0` residual, magnitude < 0.15 and `+0x89c`
    // elapsed -> `+0x9b0`++; exhausted -> `+0x358` = 0, `+0x89c` = clock+3.5.
    assert_eq!(
        call_target(bytes, 0x421023),
        0x53fca0,
        "the residual extractor"
    );
    assert_eq!(
        bytes_at(bytes, 0x421052, 6),
        &[0xd8, 0x1d, 0xa8, 0x36, 0x60, 0x00],
        "fcomp [0x6036a8] (0.15)"
    );
    assert_eq!(f32_at(bytes, 0x6036a8), f32::from_bits(0x3e19_999a));
    assert_eq!(
        bytes_at(bytes, 0x42105f, 12),
        &[
            0xd9, 0x05, 0x70, 0xc4, 0x71, 0x00, 0xd8, 0x9e, 0x9c, 0x08, 0x00, 0x00
        ],
        "fld [0x71c470] (clock); fcomp [esi+0x89c] (the dwell deadline)"
    );
    assert_eq!(
        bytes_at(bytes, 0x421072, 13),
        &[
            0x8b, 0x86, 0xb0, 0x09, 0x00, 0x00, 0x40, 0x89, 0x86, 0xb0, 0x09, 0x00, 0x00
        ],
        "mov eax,[esi+0x9b0]; inc eax; mov [esi+0x9b0],eax"
    );
    assert_eq!(
        bytes_at(bytes, 0x421089, 11),
        &[
            0x8b, 0x96, 0xa8, 0x09, 0x00, 0x00, 0x2b, 0xd1, 0xc1, 0xfa, 0x05
        ],
        "the bounds check's sar edx,5: entry count = (end-begin)>>5"
    );
    assert_eq!(
        bytes_at(bytes, 0x421098, 6),
        &[0x89, 0xbe, 0x58, 0x03, 0x00, 0x00],
        "mov [esi+0x358],edi (0): program exhausted"
    );
    assert_eq!(
        bytes_at(bytes, 0x42109e, 12),
        &[
            0xd9, 0x05, 0x70, 0xc4, 0x71, 0x00, 0xd8, 0x05, 0xa4, 0x36, 0x60, 0x00
        ],
        "fld [0x71c470]; fadd [0x6036a4] (3.5 s re-arm cooldown)"
    );
    assert_eq!(f32_at(bytes, 0x6036a4), 3.5_f32);
}

/// **The attitude leg.** `Δ = conj(+0x150) ⊗ entry`: `0x53f8d0` keeps
/// dword 0 and sign-flips dwords 1..3 (conjugate), `0x53f920` multiplies
/// w-first quaternions; `0x53fb40` rotates the `(0,0,-1)` forward constant
/// by `Δ` into the desired nose direction; the small band is an `f64` 0.05
/// on `|Δ.x|` and `|Δ.z|`. A non-zero stick triple is a literal script.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_wingman_4209b0_attitude_error_path() {
    let image =
        cs_content::coordinates::load_engine_image().expect("CS_ENGINE_IMAGE names the image");
    let bytes = &image.bytes;

    // The entry's stick triple decides script vs attitude leg.
    assert_eq!(
        bytes_at(bytes, 0x420a86, 4),
        &[0xd9, 0x44, 0x01, 0x14],
        "fld [ecx+eax+0x14] (entry pitch script)"
    );
    assert_eq!(
        bytes_at(bytes, 0x420a9d, 3),
        &[0xd9, 0x41, 0x18],
        "fld [ecx+0x18]"
    );
    assert_eq!(
        bytes_at(bytes, 0x420ab1, 3),
        &[0xd9, 0x41, 0x1c],
        "fld [ecx+0x1c]"
    );
    assert_eq!(f32_at(bytes, 0x6032c8), 0.0_f32, "the zero sentinel");

    // `+0x150` supplies one operand of `0x53f8d0`/`0x53f920`.
    assert_eq!(
        bytes_at(bytes, 0x420ad9, 6),
        &[0x8d, 0x8e, 0x50, 0x01, 0x00, 0x00],
        "lea ecx,[esi+0x150] (the AI's orientation quaternion)"
    );
    assert_eq!(call_target(bytes, 0x420b01), 0x53f8d0, "conjugate");
    assert_eq!(call_target(bytes, 0x420b12), 0x53f920, "quaternion product");

    // `0x53f8d0`: dword 0 copied, dwords 1..3 XOR 0x80000000 (sign flips).
    assert_eq!(
        bytes_at(bytes, 0x53f8d9, 4),
        &[0x8b, 0x10, 0x89, 0x11],
        "mov edx,[eax]; mov [ecx],edx (w copied)"
    );
    assert_eq!(
        bytes_at(bytes, 0x53f8f8, 5),
        &[0x35, 0x00, 0x00, 0x00, 0x80],
        "xor eax,0x80000000"
    );
    assert_eq!(
        bytes_at(bytes, 0x53f8fd, 6),
        &[0x81, 0xf2, 0x00, 0x00, 0x00, 0x80],
        "xor edx,0x80000000"
    );
    assert_eq!(
        bytes_at(bytes, 0x53f903, 6),
        &[0x81, 0xf3, 0x00, 0x00, 0x00, 0x80],
        "xor ebx,0x80000000"
    );

    // The fine band compares |Δ.x| (0x420b1f),|Δ.z| (0x420b31) against an
    // f64 0.05; inside it `+0x100` = 0 and `+0x10c` = Δ.y·sign(Δ.w).
    assert_eq!(
        bytes_at(bytes, 0x420b1f, 6),
        &[0xdc, 0x1d, 0xb0, 0x36, 0x60, 0x00],
        "fcomp qword [0x6036b0] (|Δ.x|)"
    );
    assert_eq!(
        bytes_at(bytes, 0x420b31, 6),
        &[0xdc, 0x1d, 0xb0, 0x36, 0x60, 0x00],
        "fcomp qword [0x6036b0] (|Δ.z|)"
    );
    assert_eq!(
        f64::from_le_bytes(bytes_at(bytes, 0x6036b0, 8).try_into().expect("f64")),
        f64::from_bits(0x3fa9_9999_a000_0000),
        "the f64 band is the f32 0.05 widened"
    );

    // The pitch word in the attitude leg: `+0x108` = the desired-forward
    // y while |x′| < 0.06 (edx loaded from [ebp-0x10]), else 0 (edi) —
    // and the fine band lands on the same zeroing store.
    assert_eq!(
        bytes_at(bytes, 0x420c4d, 9),
        &[0x8b, 0x55, 0xf0, 0x89, 0x96, 0x08, 0x01, 0x00, 0x00],
        "mov edx,[ebp-0x10]; mov [esi+0x108],edx (+0x108 = y′)"
    );
    assert_eq!(
        bytes_at(bytes, 0x420c97, 6),
        &[0x89, 0xbe, 0x08, 0x01, 0x00, 0x00],
        "mov [esi+0x108],edi (+0x108 = 0; edi is xor'd at 0x4209c0)"
    );

    // `0x53fb40(out, Δ, 0x6034c8)` — the forward axis rotated by the error.
    assert_eq!(
        bytes_at(bytes, 0x420b6b, 5),
        &[0x68, 0xc8, 0x34, 0x60, 0x00],
        "push 0x6034c8"
    );
    assert_eq!(
        call_target(bytes, 0x420b71),
        0x53fb40,
        "rotate vector by quaternion"
    );
    let forward: Vec<f32> = (0..4).map(|i| f32_at(bytes, 0x6034c8 + 4 * i)).collect();
    assert_eq!(
        forward,
        vec![0.0, 0.0, -1.0, 37.5],
        "the forward-axis constant"
    );

    // `0x53fca0` extracts the residual's axis·angle for the leg advance.
    assert_eq!(
        bytes_at(bytes, 0x53fca8, 7),
        &[0x8d, 0x56, 0x04, 0x89, 0x55, 0xfc, 0x8b],
        "lea edx,[esi+4] (the vector part)"
    );
    assert_eq!(
        bytes_at(bytes, 0x53fcf5, 7),
        &[0xd1, 0xf8, 0x05, 0x00, 0x00, 0xc0, 0x1f],
        "sar eax; add eax,0x1fc00000 (magnitude hack)"
    );
}

/// **The ∓0.05 floor and the gain finish.** Six literal stores snap the
/// dominant channel to exactly ∓0.05; then the words scale by
/// `+0x8c4`/`+0x8c8`/`+0x8cc`, clamp to `+0x8d0`/`+0x8d4`/`+0x8d8`, take the
/// `+0x974` skill, and land on `+0x114`/`+0x11c`/`+0x120`.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_wingman_4209b0_deadband_floor_and_gain_finish() {
    let image =
        cs_content::coordinates::load_engine_image().expect("CS_ENGINE_IMAGE names the image");
    let bytes = &image.bytes;

    // The dominant-channel floor: six literal `mov [esi+word], ∓0.05`.
    for (site, offset, value) in [
        (0x420cf9_u64, 0x10c_u64, 0xbd4c_cccd_u32),
        (0x420d36, 0x10c, 0x3d4c_cccd),
        (0x420d93, 0x108, 0xbd4c_cccd),
        (0x420dd0, 0x108, 0x3d4c_cccd),
        (0x420e22, 0x100, 0xbd4c_cccd),
        (0x420e54, 0x100, 0x3d4c_cccd),
    ] {
        let window = bytes_at(bytes, site, 10);
        assert_eq!(
            &window[..6],
            &[
                0xc7,
                0x86,
                (offset & 0xff) as u8,
                (offset >> 8) as u8,
                0x00,
                0x00
            ],
            "a dword store to vehicle+{offset:#x} at {site:#x}"
        );
        assert_eq!(
            u32::from_le_bytes(window[6..10].try_into().expect("imm")),
            value,
            "the floor immediate at {site:#x}"
        );
    }
    assert_eq!(f32_at(bytes, 0x6036ac), f32::from_bits(0xbd4c_cccd));
    assert_eq!(f32_at(bytes, 0x6034d8), f32::from_bits(0x3d4c_cccd));
    assert_eq!(f32_at(bytes, 0x61fb70), f32::from_bits(0x3d75_c28f));
    assert_eq!(f32_at(bytes, 0x61fb74), f32::from_bits(0x3d75_c28f));
    assert_eq!(f32_at(bytes, 0x61fb80), f32::from_bits(0x3eb3_3333));

    // The literal-stick script path writes `+0x14/+0x18/+0x1c` and forces
    // an identity residual so the leg advances on its deadline alone.
    assert_eq!(
        bytes_at(bytes, 0x420e60, 9),
        &[0xd9, 0x41, 0x14, 0xd9, 0x9e, 0x08, 0x01, 0x00, 0x00],
        "fld [ecx+0x14]; fstp [esi+0x108]"
    );
    assert_eq!(
        bytes_at(bytes, 0x420e69, 9),
        &[0xd9, 0x41, 0x18, 0xd9, 0x9e, 0x0c, 0x01, 0x00, 0x00],
        "fld [ecx+0x18]; fstp [esi+0x10c]"
    );
    assert_eq!(
        bytes_at(bytes, 0x420e72, 13),
        &[
            0xd9, 0x41, 0x1c, 0xc7, 0x45, 0xd0, 0x00, 0x00, 0x80, 0x3f, 0xd9, 0x9e, 0x00
        ],
        "fld [ecx+0x1c]; mov [ebp-0x30],1.0 (identity residual); fstp [esi+0x100]"
    );
    assert_eq!(
        bytes_at(bytes, 0x420e82, 21),
        &[
            0xc7, 0x45, 0xd4, 0x00, 0x00, 0x00, 0x00, //
            0xc7, 0x45, 0xd8, 0x00, 0x00, 0x00, 0x00, //
            0xc7, 0x45, 0xdc, 0x00, 0x00, 0x00, 0x00
        ],
        "[ebp-0x2c/-0x28/-0x24] = 0: the residual is the full identity (1,0,0,0)"
    );

    // The gain finish: scales, limits, skill, outputs — same offsets as
    // `0x41b560`'s step 7.
    for (site, offset) in [
        (0x420e9d_u64, 0x8c4_u64),
        (0x420eb2, 0x8c8),
        (0x420ec7, 0x8cc),
        (0x420edc, 0x8d0),
        (0x420f19, 0x8d4),
        (0x420f55, 0x8d8),
        (0x420f91, 0x974),
    ] {
        let window = bytes_at(bytes, site, 6);
        assert_eq!(window[0], 0xd9, "an f32 load at {site:#x}");
        assert_eq!(
            u32::from_le_bytes(window[2..6].try_into().expect("disp")),
            u32::try_from(offset).expect("offset"),
            "the load at {site:#x} names vehicle+{offset:#x}"
        );
    }
    assert_eq!(
        bytes_at(bytes, 0x420ea3, 6),
        &[0xd8, 0x8e, 0x00, 0x01, 0x00, 0x00],
        "fmul [esi+0x100] (roll word scaled)"
    );
    assert_eq!(
        bytes_at(bytes, 0x420eb8, 6),
        &[0xd8, 0x8e, 0x08, 0x01, 0x00, 0x00],
        "fmul [esi+0x108] (pitch word scaled — dead at +0x8c8 = 0)"
    );
    assert_eq!(
        bytes_at(bytes, 0x420ecd, 6),
        &[0xd8, 0x8e, 0x0c, 0x01, 0x00, 0x00],
        "fmul [esi+0x10c] (yaw word scaled)"
    );
    for (site, offset) in [
        (0x420fe3_u64, 0x114_u64),
        (0x420ff9, 0x11c),
        (0x421015, 0x120),
    ] {
        let window = bytes_at(bytes, site, 6);
        assert_eq!(&window[..2], &[0xd9, 0x9e], "an f32 store at {site:#x}");
        assert_eq!(
            u32::from_le_bytes(window[2..6].try_into().expect("disp")),
            u32::try_from(offset).expect("offset"),
            "the store at {site:#x} names vehicle+{offset:#x}"
        );
    }
    assert_eq!(call_target(bytes, 0x420fd8), 0x460890, "the output stub");
    assert_eq!(
        bytes_at(bytes, 0x460890, 5),
        &[0xd9, 0x44, 0x24, 0x04, 0xc3],
        "0x460890: fld [esp+4]; ret (identity)"
    );
}

/// **The `+0x358` = 1 arm.** Only `0x4201a0` writes `+0x358` = 1 (with
/// `+0x9b0` = −1) — called from the mode-0 pursuit branch `0x41db9d`, a
/// directive `0x43e10e` and an assignment `0x4b9ff8`. The descriptor table
/// is `.bss` `0x71b210`.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_wingman_4209b0_substate1_is_armed_by_program_start() {
    let image =
        cs_content::coordinates::load_engine_image().expect("CS_ENGINE_IMAGE names the image");
    let bytes = &image.bytes;

    assert_eq!(
        bytes_at(bytes, 0x4208ed, 10),
        &[0xc7, 0x87, 0xb0, 0x09, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff],
        "mov [edi+0x9b0],0xffffffff (start index)"
    );
    assert_eq!(
        bytes_at(bytes, 0x4208f7, 10),
        &[0xc7, 0x87, 0x58, 0x03, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00],
        "mov [edi+0x358],1 (the program arm)"
    );
    assert_eq!(
        call_target(bytes, 0x41db9d),
        0x4201a0,
        "the mode-0 pursuit branch arms programs"
    );
    assert_eq!(call_target(bytes, 0x43e10e), 0x4201a0, "the directive path");
    assert_eq!(
        call_target(bytes, 0x4b9ff8),
        0x4201a0,
        "the assignment path"
    );
    assert_eq!(
        e8_sites_to(bytes, 0x4201a0),
        &[0x41db9d, 0x43e10e, 0x4b9ff8],
        "the e8 census arms programs only through these three paths"
    );
    assert_eq!(
        bytes_at(bytes, 0x420236, 7),
        &[0x8b, 0x0c, 0x8d, 0x10, 0xb2, 0x71, 0x00],
        "mov ecx,[4*ecx+0x71b210] (the .bss descriptor table)"
    );

    // The stun-recovery clear and the +0x358 = 4 check bound the
    // sub-state vocabulary the two switches see.
    assert_eq!(
        bytes_at(bytes, 0x489ae9, 10),
        &[0xc7, 0x86, 0x58, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        "mov [esi+0x358],0 (stun recovery)"
    );
    assert_eq!(
        bytes_at(bytes, 0x489acd, 7),
        &[0x83, 0xbe, 0x58, 0x03, 0x00, 0x00, 0x04],
        "cmp dword [esi+0x358],4"
    );

    // The route-assign path arms `+0x358` = 2 alongside the route handle
    // `+0x9bc` — the second `+0x358` = 2 writer of the §11.1 census.
    assert_eq!(
        bytes_at(bytes, 0x421670, 6),
        &[0x89, 0xb5, 0xbc, 0x09, 0x00, 0x00],
        "mov [ebp+0x9bc],esi (the route handle)"
    );
    assert_eq!(
        bytes_at(bytes, 0x42167e, 10),
        &[0xc7, 0x85, 0x58, 0x03, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00],
        "mov [ebp+0x358],2 (route assign)"
    );

    // The full `mov [reg+0x358], …` census: no SIB-encoded or push/pop
    // forms exist; this is every writer §11.1 enumerates.
    let (immediates, stores) = substate_writers(bytes);
    assert_eq!(
        immediates,
        &[
            (0x41f98e, 3), // escape latch
            (0x41f9a6, 0), // escape end
            (0x420127, 4), // stun
            (0x4208f7, 1), // program arm
            (0x4214d4, 2), // route arm
            (0x42167e, 2), // route assign (above)
            (0x42180f, 5), // route in progress
            (0x489ae9, 0), // stun recovery
        ],
        "the c7 +0x358 writer census"
    );
    assert_eq!(
        stores,
        &[0x421098, 0x489b0c, 0x490d09, 0x4b08bf],
        "the 89 +0x358 writer census (program exhaust, route raise, clears)"
    );
}

/// **Even if it ran, the pitch stays dead.** `0x4209b0`'s `+0x108` words —
/// the ∓0.05 floor included — are multiplied by the vehicle's `+0x8c8`,
/// which for M01's wingman is `mode_alt` = 0.0. The production scale law
/// proves the floor cannot supply pitch authority through the merged gains.
#[test]
fn accept_m01_wingman_4209b0_pitch_floor_would_still_be_dead() {
    assert_eq!(M01_WINGMAN_PILOT_GAINS.scale, [0.6, 0.0, 0.15]);

    // A word at the floor: pitch ±0.05 with a small roll and yaw.
    let floored = StickWords {
        a100: -0.05,
        a108: 0.05,
        a10c: 0.05,
        recover_throttle: false,
    };
    let scaled = floored.scaled(M01_WINGMAN_PILOT_GAINS);
    assert_eq!(scaled.a108, 0.0, "+0.05 * mode_alt 0.0 stays 0");
    assert!((scaled.a100 + 0.03).abs() < 1e-6, "roll still scales");
    assert!((scaled.a10c - 0.0075).abs() < 1e-6, "yaw still scales");

    // And the default follower configuration is untouched by this task:
    // `original_stick` stays opt-in and the merged words are unchanged.
    let default_words = StickWords {
        a100: 1.0,
        a108: 1.0,
        a10c: 1.0,
        recover_throttle: false,
    }
    .scaled(M01_WINGMAN_PILOT_GAINS);
    assert_eq!(default_words.a100, 0.6);
    assert_eq!(default_words.a108, 0.0);
    assert_eq!(default_words.a10c, 0.15);
}
