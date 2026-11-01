//! #2255 (`GROUNDBLOW-GRACE-PLAYER-PAIR`): which vehicle pairs reach the two
//! `V+0xac` writes of the contact response (`0x48d2c0`, window
//! `0x48d31e`..`0x48d395`).
//!
//! Task test prefix: `accept_groundblow_grace_pair_`. The test pins the byte
//! windows the answer in `docs/findings/2026-11-01-groundblow-gates.md`
//! ("Settled by #2255") rests on. Static claims only, never `verified_original`.

const IMAGE_BASE: usize = 0x40_0000;

fn hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex"))
        .collect()
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_groundblow_grace_pair_byte_windows_match_the_engine_image() {
    let path = std::env::var("CS_ENGINE_IMAGE").expect("CS_ENGINE_IMAGE must name the image");
    let image = std::fs::read(path).expect("read the engine image");
    let windows: [(usize, &str, &str); 7] = [
        // `this` (esi) against the player pointer; the `je` at 0x48d2fb skips
        // the whole window when they are equal: the player's own step writes
        // no grace.
        (0x48d2ed, "3b3598c27100", "cmp esi,[0x71c298]"),
        (0x48d2fb, "0f84a9000000", "je 0x48d3aa (this is the player)"),
        // the hit object's flag test and the registry lookup of the other
        // vehicle (call 0x4afee0 on table 0x71dab8) -- no player test on it.
        (0x48d31e, "f7432800000040", "test [ebx+0x28],0x40000000"),
        (
            0x48d360,
            "8b887c06000085c9740583f90475",
            "other class [eax+0x67c] must be 0 or 4",
        ),
        // the two writes: this (esi) and the other (eax), both now + 1.0.
        (
            0x48d36f,
            "d90570c47100c645f201d805dc326000c645f300d99eac000000d90570c47100d805dc326000d998ac000000",
            "V+0xac = [0x71c470] + [0x6032dc] on esi and on eax",
        ),
        // the player test that follows the window reads only the flags.
        (0x48d3a2, "3b3598c27100", "cmp esi,[0x71c298]"),
        // the registry lookup: first node whose [node+8]->[+0xc] is the key.
        (
            0x4afee0,
            "8b51048b0a3bca5674128b7424088b410839700c74088b093bca75f233c05ec20400",
            "registry lookup by pointer",
        ),
    ];
    for (va, bytes, what) in windows {
        let expected = hex(bytes);
        let offset = va - IMAGE_BASE;
        assert_eq!(
            &image[offset..offset + expected.len()],
            expected.as_slice(),
            "{what} at {va:#x}"
        );
    }
}

/// The caller's `this` is the stepping vehicle and the argument is its own
/// sweep result, so each vehicle's step runs the response for its own hits.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_groundblow_grace_pair_callers_pass_the_stepping_vehicle() {
    let path = std::env::var("CS_ENGINE_IMAGE").expect("CS_ENGINE_IMAGE must name the image");
    let image = std::fs::read(path).expect("read the engine image");
    let near = |va: usize| &image[va - IMAGE_BASE..va - IMAGE_BASE + 5];
    // call rel32 to 0x48d2c0 at 0x48ed8b and 0x490b08, preceded by mov ecx,edi.
    for call in [0x48ed8b_usize, 0x490b08] {
        let site = near(call);
        assert_eq!(site[0], 0xe8, "call at {call:#x}");
        let rel = i32::from_le_bytes(site[1..5].try_into().unwrap());
        assert_eq!((call as i64 + 5 + i64::from(rel)) as usize, 0x48d2c0);
        let before = &image[call - IMAGE_BASE - 2..call - IMAGE_BASE];
        assert_eq!(before, [0x8b, 0xcf], "mov ecx,edi before {call:#x}");
    }
}

/// #2256 (`GROUNDBLOW-GRACE-SWEEP`): the hit search the sweep (`0x48d7f0`)
/// calls is the same code for every seat. The sweep's own player test is one
/// `cmp esi,[0x71c298]` after the search, and the search routines
/// (`0x4c8f70`..`0x4cca30`, containing `0x4ca320`) never name the player
/// pointer, so nothing in the bytes makes the AI's sweep blind to the player.
/// Whether the player's scene node is *flagged* into the search is data, not
/// code, and stays open (see the finding).
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_groundblow_grace_sweep_search_is_seat_agnostic() {
    let path = std::env::var("CS_ENGINE_IMAGE").expect("CS_ENGINE_IMAGE must name the image");
    let image = std::fs::read(path).expect("read the engine image");
    let at = |va: usize, len: usize| &image[va - IMAGE_BASE..va - IMAGE_BASE + len];
    let rel_target = |call: usize| {
        let rel = i32::from_le_bytes(at(call + 1, 4).try_into().unwrap());
        assert_eq!(image[call - IMAGE_BASE], 0xe8, "call at {call:#x}");
        (call as i64 + 5 + i64::from(rel)) as usize
    };
    // The sweep: the stepping vehicle's own `[V+0xac]` against the clock is
    // the first thing it does (a vehicle inside its grace sweeps nothing).
    assert_eq!(
        at(0x48d7fd, 0x1e),
        hex("d90570c4710053568bf1d89eac0000005789b578ffffffdfe0f6c401740f")
    );
    // Its two searches, both with the world `[0x71c2ac]` and the vehicle's
    // own box: no vehicle or seat argument.
    assert_eq!(rel_target(0x48da06), 0x4c8f70);
    assert_eq!(rel_target(0x48da25), 0x4ca320);
    // The one player test after the searches (`esi` is the stepping vehicle).
    assert_eq!(at(0x48da3e, 0x10), hex("a198c2710083c4083bf00f84bb010000"));
    // The caller of the sweep hands back the stepping vehicle's own `H`.
    assert_eq!(rel_target(0x48ea17), 0x48d7f0);
    // The sweep names the player pointer exactly once...
    let needle = hex("98c27100");
    let count = |from: usize, to: usize| {
        image[from - IMAGE_BASE..to - IMAGE_BASE]
            .windows(4)
            .filter(|w| *w == needle.as_slice())
            .count()
    };
    assert_eq!(count(0x48d7f0, 0x48e540), 1, "player pointer in the sweep");
    // ...and the search routines not at all.
    assert_eq!(count(0x4c8f70, 0x4cca30), 0, "player pointer in the search");
}
