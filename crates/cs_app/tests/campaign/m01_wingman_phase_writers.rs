//! M01-WINGMAN-PHASE-WRITERS-REST (#2294): the two `[+0xd8]` stores #2289 left
//! unclassified (`0x4679cf`, `0x468e1d`) and the `[0x64f66e]` gate.
//!
//! Task test prefix: `accept_m01_wingman_phase_writers_`. The answers, with the
//! byte windows these tests pin, are in
//! `docs/findings/2026-11-02-m01-wingman-gains.md` §9. Image tests are ignored
//! without `CS_ENGINE_IMAGE`, retail tests without `CS_GAME_DIR`; the claims are
//! `static` (`OWNER-STATIC-2026-10-29`), nothing is `verified_original`.
//!
//! The two stores are **not** follower-phase writers: both live in the
//! `objectives.zrd` loader `0x466b70`, write `[ebp+0xd8]` where `ebp` is one
//! `0x5e4`-byte objective record, and hold that record's `hide_obj` slot
//! (default `-1`, the `HIDE_OBJ` value minus one, or `0` when the key is
//! absent). `[0x64f66e]` is the engine's debug `suspend` latch: four writers,
//! 28 readers, and none of them in the follower routine `0x41e760`, so the
//! phase 0 -> 1 transition is not gated by it.

use cs_content::coordinates::load_engine_image;
use cs_content::stunts::{ZrdValue, objective_state_machine};

use crate::campaign::m01_wingman_gains::{bytes_at, f32_at, member_document, text_at};
use crate::campaign::vs_m01_rt_window::game_dir;

fn u32_at(bytes: &[u8], va: u64) -> u32 {
    u32::from_le_bytes(bytes_at(bytes, va, 4).try_into().expect("four bytes"))
}

/// The direct `call rel32` at `va` reaches `target`, and returns the target.
/// The displacement is signed: a call backwards (as most of these are) must
/// not be read as a huge unsigned offset.
fn call_target(bytes: &[u8], va: u64) -> u64 {
    assert_eq!(bytes_at(bytes, va, 1), &[0xe8], "a direct call at {va:#x}");
    let rel = i32::from_le_bytes(
        bytes_at(bytes, va + 1, 4)
            .try_into()
            .expect("a rel32 displacement"),
    );
    u64::try_from(i64::try_from(va).expect("an address") + 5 + i64::from(rel))
        .expect("a non-negative target")
}

/// Every `mov dword ptr [reg+0xd8], r32` store in `from..to` (no index
/// register): `89 /r` with mod = 10 and no SIB, or `c7 86 d8 …` (a `[esi+disp32]`
/// immediate). Sites are instruction addresses.
fn d8_stores(bytes: &[u8], from: u64, to: u64) -> Vec<u64> {
    let window = bytes_at(
        bytes,
        from,
        usize::try_from(to - from).expect("a bounded window"),
    );
    let mut sites = Vec::new();
    for i in 0..window.len().saturating_sub(6) {
        // ModRM mod = 10 keeps the top two bits; the low three name the base.
        // rm = 100 is a SIB byte, so it is not a plain `[reg+disp32]`.
        let plain = window[i] == 0x89
            && matches!(
                window[i + 1] & 0xc7,
                0x80 | 0x81 | 0x82 | 0x83 | 0x85 | 0x86 | 0x87
            );
        let immediate = window[i] == 0xc7 && window[i + 1] == 0x86;
        if (plain || immediate) && window[i + 2..i + 6] == [0xd8, 0x00, 0x00, 0x00] {
            sites.push(from + i as u64);
        }
    }
    sites
}

/// **The two unclassified `[+0xd8]` writers are the objectives loader's
/// `hide_obj`.** The enclosing function `0x466b70` is `mission.cpp`'s
/// `objectives.zrd` loader: an SEH-framed member (`ecx` = the mission),
/// opening the document by name and parsing every key an objective block can
/// carry. Each numbered block is one `0x5e4`-byte record allocated from
/// `[mission+0xc4c]`, zeroed with `rep stosd` of `0x179` dwords, and `ebp` is
/// the record base — so `0x4679cf` (`hide_obj = -1` default) and `0x468e1d`
/// (`HIDE_OBJ`'s value minus one) write an objective record, not the AI's
/// phase word `[esi+0xd8]`.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_wingman_phase_writers_are_the_objectives_loader_not_the_ai_phase_word() {
    let image = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    let bytes = &image.bytes;

    // The function's SEH prologue and `mov ebx, ecx` (this), then the record
    // array it frees and reallocates: `mov eax, [ebx+0xc4c]`.
    assert_eq!(
        bytes_at(bytes, 0x466b70, 32),
        &[
            0x6a, 0xff, 0x68, 0x9f, 0xf4, 0x5f, 0x00, 0x64, 0xa1, 0x00, 0x00, 0x00, 0x00, 0x50,
            0x64, 0x89, 0x25, 0x00, 0x00, 0x00, 0x00, 0x81, 0xec, 0xec, 0x00, 0x00, 0x00, 0x53,
            0x55, 0x56, 0x8b, 0xd9
        ],
        "SEH frame, sub esp, 0xec, push ebx/ebp/esi, mov ebx, ecx"
    );
    assert_eq!(
        bytes_at(bytes, 0x466b90, 6),
        &[0x8b, 0x83, 0x4c, 0x0c, 0x00, 0x00],
        "mov eax, [ebx+0xc4c] — the block record array"
    );
    // The document is opened by the stack argument (`mov esi,[esp+0x10c]`),
    // not built from a key.
    assert_eq!(
        bytes_at(bytes, 0x466bee, 10),
        &[0x8b, 0xb4, 0x24, 0x0c, 0x01, 0x00, 0x00, 0x57, 0x57, 0x56]
    );
    assert_eq!(
        call_target(bytes, 0x466bf8),
        0x579c60,
        "the document open by name"
    );
    // Its failure paths name the file: "Failed to read %s" and mission.cpp.
    assert_eq!(text_at(bytes, 0x625ee4), "Failed to read %s");
    assert_eq!(text_at(bytes, 0x626128), "D:\\zipper\\Crimson\\mission.cpp");
    // Both callers pass "objectives.zrd": the mission load `0x4647ba` and the
    // mission (re)start `0x46557b`.
    assert_eq!(text_at(bytes, 0x625cf8), "objectives.zrd");
    assert_eq!(
        bytes_at(bytes, 0x4647ba, 5),
        &[0x68, 0xf8, 0x5c, 0x62, 0x00]
    );
    assert_eq!(call_target(bytes, 0x4647c4), 0x466b70);
    // The first caller's enclosing function is the mission-data loader: it logs
    // "Load Mission Sounds" / "Finish LoadMissionData", and the function that
    // logs "Begin CZMission::Load" calls it at `0x4653b0`.
    assert_eq!(text_at(bytes, 0x625ca0), "Load Mission Sounds");
    assert_eq!(
        bytes_at(bytes, 0x4646f3, 5),
        &[0x68, 0xa0, 0x5c, 0x62, 0x00]
    );
    assert_eq!(text_at(bytes, 0x625d94), "Finish LoadMissionData");
    assert_eq!(
        bytes_at(bytes, 0x464951, 5),
        &[0x68, 0x94, 0x5d, 0x62, 0x00]
    );
    assert_eq!(call_target(bytes, 0x4653b0), 0x463f40);
    assert_eq!(text_at(bytes, 0x625e14), "Begin CZMission::Load");
    assert_eq!(
        bytes_at(bytes, 0x465395, 5),
        &[0x68, 0x14, 0x5e, 0x62, 0x00]
    );
    assert_eq!(text_at(bytes, 0x625e2c), "objectives.zrd");
    assert_eq!(
        bytes_at(bytes, 0x46557b, 5),
        &[0x68, 0x2c, 0x5e, 0x62, 0x00]
    );
    assert_eq!(call_target(bytes, 0x465582), 0x466b70);
    // The second caller is the (re)start, which also pushes NEW_GAME_START.
    assert_eq!(text_at(bytes, 0x625e50), "NEW_GAME_START");
    assert_eq!(
        bytes_at(bytes, 0x46564d, 5),
        &[0x68, 0x50, 0x5e, 0x62, 0x00]
    );

    // One `0x5e4`-byte record per numbered block:
    //   edx = n*3; edx <<= 4 (= 48n); edx -= n (= 47n); eax = n + 8*edx
    //   (= 377n); eax <<= 2 (= 1508n).
    assert_eq!(
        bytes_at(bytes, 0x46795c, 14),
        &[
            0x8d, 0x14, 0x40, 0xc1, 0xe2, 0x04, 0x2b, 0xd0, 0x8d, 0x04, 0xd0, 0xc1, 0xe0, 0x02
        ],
        "the 0x5e4 byte stride"
    );
    for index in 1_u64..=3 {
        let scaled = index * 377 * 4;
        assert_eq!(scaled, index * 0x5e4, "record {index}");
    }
    // `realloc([edi+0xc4c], 0x5e4n)` through the MSVC realloc import slot, the
    // new base stored back into the mission, then the record zeroed:
    // `mov ecx, 0x179` (377 dwords = 1508 bytes) / `mov edi, ebp` / `rep stosd`.
    assert_eq!(
        bytes_at(bytes, 0x46796c, 12),
        &[
            0xff, 0x15, 0x38, 0x03, 0xa2, 0x00, 0x89, 0x87, 0x4c, 0x0c, 0x00, 0x00
        ],
        "call [realloc] / mov [edi+0xc4c], eax"
    );
    assert_eq!(
        bytes_at(bytes, 0x46798e, 8),
        &[0xb9, 0x79, 0x01, 0x00, 0x00, 0x8b, 0xfd, 0xf3],
        "mov ecx, 0x179; mov edi, ebp; rep stosd"
    );
    // `lea ebp, [eax + 4*ecx]` with `ecx = 377*index`: ebp is the record base.
    assert_eq!(bytes_at(bytes, 0x467983, 3), &[0x8d, 0x2c, 0x88]);

    // The default: `or ebx, -1`, then `mov [ebp+0xd8], ebx` at 0x4679cf in the
    // initialiser that also sets `+0x558 = 1` and `+0x580 = -1`.
    assert_eq!(
        bytes_at(bytes, 0x467953, 3),
        &[0x83, 0xcb, 0xff],
        "or ebx, -1"
    );
    assert_eq!(
        bytes_at(bytes, 0x4679cf, 6),
        &[0x89, 0x9d, 0xd8, 0x00, 0x00, 0x00],
        "mov dword ptr [ebp+0xd8], ebx"
    );
    assert_eq!(
        bytes_at(bytes, 0x467997, 12),
        &[
            0xb8, 0x01, 0x00, 0x00, 0x00, 0x89, 0x85, 0x58, 0x05, 0x00, 0x00, 0x89
        ],
        "the same initialiser: +0x558 = 1"
    );

    // The `HIDE_OBJ` handler: the key text, the neighbours it is parsed next
    // to, and the two stores.
    assert_eq!(text_at(bytes, 0x6269c0), "WAKE_OBJECTIVE_WHEN_I_SLEEP");
    assert_eq!(text_at(bytes, 0x6269dc), "HIDE_OBJ");
    assert_eq!(text_at(bytes, 0x6269e8), "TIMER_ADJUST");
    assert_eq!(
        bytes_at(bytes, 0x468dfd, 6),
        &[0x68, 0xdc, 0x69, 0x62, 0x00, 0x53],
        "push HIDE_OBJ; push ebx"
    );
    // The `+0xdc` list terminator: `mov [ebp + 4*esi + 0xd8], edi` — a SIB
    // store whose address is `+0xdc + 4*(esi-1)`, never `+0xd8`.
    assert_eq!(
        bytes_at(bytes, 0x468e03, 7),
        &[0x89, 0xbc, 0xb5, 0xd8, 0x00, 0x00, 0x00],
        "mov [ebp+esi*4+0xd8], edi"
    );
    // `mov eax, [eax+0xc]` / `dec eax` / `mov [ebp+0xd8], eax` (value - 1),
    // and the absent branch's `mov [ebp+0xd8], edi` (0).
    assert_eq!(
        bytes_at(bytes, 0x468e16, 11),
        &[
            0x8b, 0x50, 0x04, 0x8b, 0x42, 0x0c, 0x48, 0x89, 0x85, 0xd8, 0x00
        ],
        "mov edx,[eax+4]; mov eax,[edx+0xc]; dec eax; mov [ebp+0xd8]…"
    );
    assert_eq!(
        bytes_at(bytes, 0x468e1d, 6),
        &[0x89, 0x85, 0xd8, 0x00, 0x00, 0x00],
        "mov dword ptr [ebp+0xd8], eax"
    );
    assert_eq!(
        bytes_at(bytes, 0x468e25, 6),
        &[0x89, 0xbd, 0xd8, 0x00, 0x00, 0x00],
        "mov dword ptr [ebp+0xd8], edi (absent HIDE_OBJ)"
    );
    // The block count lands in `+0xc48` after the loop.
    assert_eq!(
        bytes_at(bytes, 0x469043, 6),
        &[0x89, 0x90, 0x48, 0x0c, 0x00, 0x00],
        "mov [eax+0xc48], edx"
    );

    // Completeness. Inside the loader the only `[reg+0xd8]` stores are these
    // three; the whole image has 28, and the AI window's are exactly #2289's
    // six `[esi+0xd8]` phase-word stores.
    assert_eq!(
        d8_stores(bytes, 0x466b70, 0x469063),
        vec![0x4679cf, 0x468e1d, 0x468e25],
        "the objectives loader's +0xd8 stores"
    );
    assert_eq!(
        d8_stores(bytes, 0x401000, 0x6024bd),
        vec![
            0x402b72, 0x41e7b0, 0x41e8d1, 0x41e9f6, 0x41ea21, 0x41ea37, 0x41ea62, 0x425415,
            0x436e7e, 0x437468, 0x437d5d, 0x437dad, 0x439053, 0x4578bd, 0x4679cf, 0x468e1d,
            0x468e25, 0x4b05a3, 0x4c0b09, 0x4c0c12, 0x4d21e1, 0x4d2acc, 0x55025c, 0x59be3a,
            0x5b58f4, 0x5b5970, 0x5f3212, 0x5f325d
        ],
        "every +0xd8 store in the image"
    );
    assert_eq!(
        d8_stores(bytes, 0x41c270, 0x4222ff),
        vec![0x41e7b0, 0x41e8d1, 0x41e9f6, 0x41ea21, 0x41ea37, 0x41ea62],
        "the AI code's phase-word stores (#2289)"
    );
    // The AI object is bigger than a record — it holds +0x948/+0x94c (the
    // target handle and its timer) and +0x934 (own speed) — while a block
    // record is 0x5e4 bytes, so the record cannot be the object the phase
    // machine reads. (The stores above are [ebp+0xd8]; the phase machine's are
    // [esi+0xd8].)
}

/// **`[0x64f66e]` is the engine's `suspend` latch.** Four writers — the debug
/// console's `suspend` command, the automatic `-suspend` deadline, and the two
/// mission-lifecycle clears — and 28 readers, six of them in the AI code, where
/// each skips a timer-driven transition. None lies in the follower routine
/// `0x41e760..0x41f00a`, so the phase 0 -> 1 transition is not gated.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_wingman_phase_writers_suspend_latch_has_four_writers_and_no_phase_reader() {
    let image = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    let bytes = &image.bytes;

    // Every reference to the byte in `.text`: the four writers and 28 readers.
    // `mov al,[abs]` / `mov [abs],al` keep the imm at +1; `c6 05` and `38 1d`
    // keep it at +2.
    let sites: [(u64, u64); 4] = [(0x43db4d, 1), (0x464693, 2), (0x4654fa, 2), (0x4a0252, 2)];
    // Every reader is `mov al, byte ptr [0x64f66e]` (`a0`, imm at +1) except
    // `0x48a12f`, which compares: `cmp byte ptr [0x64f66e], bl` (`38 1d`).
    let readers: [(u64, u64); 28] = [
        (0x41d9f9, 1),
        (0x41db73, 1),
        (0x41dec4, 1),
        (0x41e674, 1),
        (0x41f060, 1),
        (0x41fe1e, 1),
        (0x452684, 1),
        (0x489ac4, 1),
        (0x48a12f, 2),
        (0x48b4ae, 1),
        (0x48b530, 1),
        (0x48b5b4, 1),
        (0x48c3fa, 1),
        (0x48d2db, 1),
        (0x48e632, 1),
        (0x48e6df, 1),
        (0x48e7e3, 1),
        (0x48ea74, 1),
        (0x4aad40, 1),
        (0x4b6bb0, 1),
        (0x4b72b5, 1),
        (0x4b9bff, 1),
        (0x4bf3fa, 1),
        (0x4bf421, 1),
        (0x4bf444, 1),
        (0x4bf5e4, 1),
        (0x4bf6d4, 1),
        (0x4bf716, 1),
    ];
    let mut expected: Vec<u64> = readers.iter().map(|(site, shift)| site + shift).collect();
    expected.extend(sites.iter().map(|(site, shift)| site + shift));
    expected.sort_unstable();
    let text = bytes_at(bytes, 0x401000, 0x2014bd);
    let mut found = Vec::new();
    for i in 0..text.len() - 4 {
        if text[i..i + 4] == [0x6e, 0xf6, 0x64, 0x00] {
            found.push(0x401000 + i as u64);
        }
    }
    assert_eq!(found, expected, "every [0x64f66e] reference in .text");
    assert_eq!(found.len(), 32, "4 writers + 28 readers");

    // The console command's store: the dispatcher `0x43d640` compares the
    // 8-byte key at 0x622c7c, reads the boolean with `0x5b7910`, stores it,
    // and on false resets the deadline to the 1e9 "never" word.
    assert_eq!(text_at(bytes, 0x622c7c), "suspend");
    assert_eq!(text_at(bytes, 0x622c84), "gbc");
    assert_eq!(text_at(bytes, 0x622c88), "exit");
    assert_eq!(text_at(bytes, 0x622c90), "debug");
    assert_eq!(text_at(bytes, 0x622cf8), "gimme");
    assert_eq!(
        bytes_at(bytes, 0x43db2e, 10),
        &[0xbf, 0x7c, 0x2c, 0x62, 0x00, 0xb9, 0x08, 0x00, 0x00, 0x00],
        "mov edi, \"suspend\"; mov ecx, 8"
    );
    assert_eq!(
        bytes_at(bytes, 0x43db43, 21),
        &[
            0x52, 0x8b, 0xcb, 0xe8, 0xc5, 0x9d, 0x17, 0x00, 0x84, 0xc0, 0xa2, 0x6e, 0xf6, 0x64,
            0x00, 0x0f, 0x85, 0x51, 0x08, 0x00, 0x00
        ],
        "push 0; mov ecx,ebx; call get_bool; test al,al; mov [0x64f66e],al; jne"
    );
    assert_eq!(call_target(bytes, 0x43db46), 0x5b7910);
    assert_eq!(
        bytes_at(bytes, 0x43db58, 15),
        &[
            0x5f, 0x5e, 0xc7, 0x05, 0xb8, 0x2b, 0x62, 0x00, 0x00, 0xca, 0x9a, 0x3b, 0x5b, 0x8b,
            0x4d
        ],
        "off: pop edi/esi; mov [0x622bb8], 1e9; pop ebx; mov ecx,[ebp-0xc]"
    );
    // The dispatcher is reached from the console's `execute(text)` wrapper.
    assert_eq!(
        bytes_at(bytes, 0x45cdf0, 6),
        &[0x8b, 0x44, 0x24, 0x04, 0x50, 0xe8]
    );
    assert_eq!(call_target(bytes, 0x45cdf5), 0x43d640);

    // The automatic writer: the per-frame routine `0x4a0220` sets the byte when
    // the counter `[0x9be6f8]` has reached the deadline `[0x622bb8]`.
    assert_eq!(
        bytes_at(bytes, 0x4a0220, 10),
        &[0x55, 0x8b, 0xec, 0x6a, 0xff, 0x68, 0x2a, 0x03, 0x60, 0x00],
        "the automatic writer's routine prologue"
    );
    assert_eq!(
        bytes_at(bytes, 0x4a023e, 21),
        &[
            0xa1, 0xf8, 0xe6, 0x9b, 0x00, 0x53, 0x56, 0x8b, 0xf1, 0x3b, 0x05, 0xb8, 0x2b, 0x62,
            0x00, 0x89, 0x75, 0x90, 0x72, 0x07, 0xc6
        ],
        "mov eax,[0x9be6f8]; … cmp eax,[0x622bb8]; … jb; mov byte [0x64f66e], 1"
    );
    assert_eq!(
        bytes_at(bytes, 0x4a0252, 7),
        &[0xc6, 0x05, 0x6e, 0xf6, 0x64, 0x00, 0x01]
    );
    // The counter is a per-call increment, not the float clock.
    assert_eq!(
        bytes_at(bytes, 0x5a857b, 6),
        &[0x40, 0xa3, 0xf8, 0xe6, 0x9b, 0x00],
        "inc eax; mov [0x9be6f8], eax"
    );
    // The command line's `-suspend <n>` supplies the delay `[0x622bb4]`; both
    // globals start at 1e9, so an ordinary launch never expires.
    assert_eq!(text_at(bytes, 0x62991c), "-suspend");
    assert_eq!(
        bytes_at(bytes, 0x4a76c3, 9),
        &[0xbf, 0x1c, 0x99, 0x62, 0x00, 0x8b, 0xf0, 0xb9, 0x09]
    );
    assert_eq!(
        bytes_at(bytes, 0x4a76e4, 15),
        &[
            0x50, 0xff, 0x15, 0x48, 0x02, 0xa2, 0x00, 0x83, 0xc4, 0x04, 0xa3, 0xb4, 0x2b, 0x62,
            0x00
        ],
        "push; call strtol-ish; mov [0x622bb4], eax"
    );
    assert_eq!(
        u32_at(bytes, 0x622bb4),
        1_000_000_000,
        "the -suspend default"
    );
    assert_eq!(
        u32_at(bytes, 0x622bb8),
        1_000_000_000,
        "the deadline default"
    );

    // The two mission-lifecycle clears: re-arm the deadline at
    // `[0x622bb4] + [0x9be6f8]`, clear the byte. The (re)start path also
    // resets the sim clock the AI timers read.
    assert_eq!(
        bytes_at(bytes, 0x464680, 29),
        &[
            0xa1, 0xb4, 0x2b, 0x62, 0x00, 0x83, 0xec, 0x0c, 0x56, 0x8b, 0xf1, 0x8b, 0x0d, 0xf8,
            0xe6, 0x9b, 0x00, 0x03, 0xc1, 0xc6, 0x05, 0x6e, 0xf6, 0x64, 0x00, 0x00, 0xa3, 0xb8,
            0x2b
        ],
        "the mission load's clear and re-arm"
    );
    assert_eq!(
        bytes_at(bytes, 0x4654e0, 35),
        &[
            0xa1, 0xf8, 0xe6, 0x9b, 0x00, 0x56, 0x8b, 0xf1, 0x8b, 0x0d, 0xb4, 0x2b, 0x62, 0x00,
            0x03, 0xc8, 0xc7, 0x05, 0x70, 0xc4, 0x71, 0x00, 0x00, 0x00, 0x00, 0x00, 0xc6, 0x05,
            0x6e, 0xf6, 0x64, 0x00, 0x00, 0x89, 0x0d
        ],
        "the (re)start's clear, clock reset and re-arm"
    );
    // The clear's enclosing function is `0x464680`, invoked from the function
    // that logs "Begin CZMission::Load" (pinned in the loader test).
    assert_eq!(call_target(bytes, 0x4653be), 0x464680);

    // No reader lies in the follower routine: the phase tables, the 0/2/4 -> 1
    // join and its three gates are not gated by the latch.
    for site in &expected {
        assert!(
            !(0x41e760..=0x41f00a).contains(site),
            "{site:#x} is inside the follower routine"
        );
    }
    assert_eq!(
        bytes_at(bytes, 0x41e834, 6),
        &[0x8a, 0x87, 0x1d, 0x09, 0x00, 0x00],
        "the phase 0 -> 1 gate's leader-awake read is 0x91d, not the latch"
    );
    assert_eq!(f32_at(bytes, 0x6035e0), 490_000.0);
    assert_eq!(f32_at(bytes, 0x603594), 20.576);

    // What the latch does gate: the whole upkeep (so the target handle is not
    // rebuilt) and the vehicle update's stun-recovery clear.
    assert_eq!(
        bytes_at(bytes, 0x41fe1e, 5),
        &[0xa0, 0x6e, 0xf6, 0x64, 0x00],
        "mov al, [0x64f66e] at the upkeep"
    );
    assert_eq!(
        bytes_at(bytes, 0x41fe35, 6),
        &[0x0f, 0x85, 0x1c, 0x02, 0x00, 0x00],
        "jne past the scorer and the handle rebuild"
    );
    assert_eq!(
        bytes_at(bytes, 0x489ac4, 5),
        &[0xa0, 0x6e, 0xf6, 0x64, 0x00],
        "the vehicle update reads the latch"
    );
    assert_eq!(
        bytes_at(bytes, 0x489acb, 9),
        &[0x75, 0x26, 0x83, 0xbe, 0x58, 0x03, 0x00, 0x00, 0x04],
        "jne over the +0x358 stun-recovery clear"
    );
}

/// **M01's own `objectives.zrd` never reaches either store's non-default
/// branch.** `ZBD/C3/M01/zrdr.zbd` declares 39 `OBJECTIVE<n>` blocks (the same
/// count the lowered win route uses), and none of them declares `HIDE_OBJ` or
/// `WAKE_OBJECTIVE_WHEN_I_SLEEP` — the two keys whose handlers write `+0xd8`
/// and `+0xdc`. So every M01 block takes the absent branch: `hide_obj = 0` and
/// an empty sleep-wake list. The loader runs before the first tick, on
/// objective records only; `[0x64f66e]` is not writable by mission data at all.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_wingman_phase_writers_m01_objectives_declare_no_hide_or_sleep_keys() {
    let root = game_dir();
    let (document, _) = member_document(&root, "zbd/c3/m01/zrdr.zbd", "objectives.zrd");

    // The production census counts the numbered blocks.
    assert_eq!(
        objective_state_machine(&document).blocks(),
        39,
        "C3/M01's OBJECTIVE blocks"
    );

    // Every key in the document, at any depth.
    fn keys(value: &ZrdValue, out: &mut Vec<String>) {
        if let Some(items) = value.as_list() {
            let mut index = 0;
            while index + 1 < items.len() {
                if let Some(key) = items[index].as_text() {
                    out.push(key.to_owned());
                }
                index += 2;
            }
            for item in items {
                keys(item, out);
            }
        }
    }
    let mut all = Vec::new();
    keys(&document, &mut all);
    // A positive control: the neighbouring keys are present, so an empty scan
    // would be a broken walk, not a missing key.
    assert_eq!(
        all.iter()
            .filter(|key| key.as_str() == "TICK_DEPENDS_ON_OBJ")
            .count(),
        2,
        "the block's neighbour key is present"
    );
    assert_eq!(
        all.iter()
            .filter(|key| key.as_str() == "NAP_OBJECTIVE_WHEN_I_COMPLETE")
            .count(),
        12,
        "the neighbouring nap key is present (the count §9.3 records)"
    );
    assert_eq!(
        all.iter().filter(|key| key.as_str() == "HIDE_OBJ").count(),
        0,
        "no block declares HIDE_OBJ"
    );
    assert_eq!(
        all.iter()
            .filter(|key| key.as_str() == "WAKE_OBJECTIVE_WHEN_I_SLEEP")
            .count(),
        0,
        "no block declares WAKE_OBJECTIVE_WHEN_I_SLEEP"
    );
}

/// **No production change.** The two stores write objective records and the
/// latch is off in an ordinary mission, so the M01 follower's measured set and
/// its `original_stick` default are exactly what #2289 left.
#[test]
fn accept_m01_wingman_phase_writers_default_m01_follower_is_unchanged() {
    use cs_app::mission_ai::{M01_WINGMAN_PILOT_GAINS, PHASE0_LEADER_CLIMB_M, RUDDER_TOL};

    assert_eq!(M01_WINGMAN_PILOT_GAINS.scale, [0.6, 0.0, 0.15]);
    assert_eq!(M01_WINGMAN_PILOT_GAINS.limit, [100.0, 100.0, 0.25]);
    assert_eq!(RUDDER_TOL, 0.2);
    assert_eq!(PHASE0_LEADER_CLIMB_M, 200.0);
}
