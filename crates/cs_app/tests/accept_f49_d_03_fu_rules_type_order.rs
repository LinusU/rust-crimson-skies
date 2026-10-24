//! F49-D.03-FU-RULES-TYPE-ORDER (#2037): whether the Instant Action menu's
//! mission type survives the load's `ia.zrd` re-read.
//!
//! The measurement (`docs/findings/2026-10-24-f49-d-03-fu-rules-type-order.md`)
//! is the menu handler's stores to the live type `[0x718cd8]`
//! (`0x417554`..`0x417578`), the load's `0x45a150` (called once, from
//! `0x466bd5`) which resets the record and re-reads the descriptor's own
//! `mission_type`, and the exhaustive list of the image's references to
//! `[0x718cd8]`. The engine-image tests re-read the bytes at the named virtual
//! addresses independently of the production code; the retail test binds the
//! consequence to the production survey of the installation's eight `ia.zrd`.
//!
//! Required capabilities: **retail** (`$CS_GAME_DIR`) and `$CS_ENGINE_IMAGE`,
//! read-only. Nothing is run, so the claim stays `static`, never
//! `verified_original`.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use cs_content::instant_action::{IA_MISSION_TYPE_VOCABULARY, survey_retail_instant_action};

const FINDING: &str = "docs/findings/2026-10-24-f49-d-03-fu-rules-type-order.md";

/// The live mission type, `[0x718cd8]`, and the record's first dword.
const LIVE_TYPE: u32 = 0x0071_8cd8;

// ------------------------------------------------------- engine image --------

/// The image's bytes at virtual address `va`, through its PE section table.
fn image_bytes(image: &[u8], va: u32, len: usize) -> &[u8] {
    let u16_at = |at: usize| u16::from_le_bytes(image[at..at + 2].try_into().expect("2 bytes"));
    let u32_at = |at: usize| u32::from_le_bytes(image[at..at + 4].try_into().expect("4 bytes"));
    let pe = u32_at(0x3c) as usize;
    assert_eq!(&image[pe..pe + 4], b"PE\0\0", "a PE image");
    let sections = usize::from(u16_at(pe + 6));
    let optional = usize::from(u16_at(pe + 20));
    let image_base = u32_at(pe + 24 + 28);
    let table = pe + 24 + optional;
    for index in 0..sections {
        let entry = table + index * 40;
        let virtual_size = u32_at(entry + 8);
        let section_va = image_base + u32_at(entry + 12);
        let raw_size = u32_at(entry + 16);
        let raw = u32_at(entry + 20) as usize;
        if va >= section_va && va - section_va + len as u32 <= virtual_size.min(raw_size) {
            let start = raw + (va - section_va) as usize;
            return &image[start..start + len];
        }
    }
    panic!("virtual address {va:#x} is in no section");
}

fn image_u32(image: &[u8], va: u32) -> u32 {
    u32::from_le_bytes(
        image_bytes(image, va, 4)
            .try_into()
            .expect("a dword is 4 bytes"),
    )
}

fn engine_image() -> Vec<u8> {
    let path = std::env::var("CS_ENGINE_IMAGE").expect("CS_ENGINE_IMAGE must be set");
    fs::read(path).expect("the engine image reads")
}

fn assert_bytes(image: &[u8], va: u32, want: &[u8], what: &str) {
    assert_eq!(
        image_bytes(image, va, want.len()),
        want,
        "{what} at {va:#x}"
    );
}

/// The target of the `call rel32` / `jmp rel32` (`e8`/`e9`) at `va`.
fn rel32_target(image: &[u8], va: u32) -> u32 {
    assert!(
        matches!(image_bytes(image, va, 1)[0], 0xe8 | 0xe9),
        "a rel32 branch at {va:#x}"
    );
    va.wrapping_add(5).wrapping_add(image_u32(image, va + 1))
}

/// The `.text` section's `(start, length)` virtual range.
fn text_range(image: &[u8]) -> (u32, usize) {
    let u32_at = |at: usize| u32::from_le_bytes(image[at..at + 4].try_into().expect("4 bytes"));
    let pe = u32_at(0x3c) as usize;
    let optional = usize::from(u16::from_le_bytes([image[pe + 20], image[pe + 21]]));
    let image_base = u32_at(pe + 24 + 28);
    let first = pe + 24 + optional;
    assert_eq!(&image[first..first + 5], b".text", "section 0 is .text");
    let length = u32_at(first + 8).min(u32_at(first + 16));
    (image_base + u32_at(first + 12), length as usize)
}

/// Every virtual address in `.text` that holds `needle` as a little-endian
/// dword.
fn dword_sites(image: &[u8], needle: u32) -> Vec<u32> {
    let (start, length) = text_range(image);
    let text = image_bytes(image, start, length);
    let pattern = needle.to_le_bytes();
    (0..length.saturating_sub(3))
        .filter(|&at| text[at..at + 4] == pattern)
        .map(|at| start + at as u32)
        .collect()
}

/// Every `call`/`jmp rel32` in `.text` whose target is `target`.
fn branch_sites_to(image: &[u8], target: u32) -> Vec<u32> {
    let (start, length) = text_range(image);
    let text = image_bytes(image, start, length);
    (0..length.saturating_sub(4))
        .filter(|&at| matches!(text[at], 0xe8 | 0xe9))
        .filter(|&at| {
            let rel = u32::from_le_bytes(text[at + 1..at + 5].try_into().expect("4 bytes"));
            (start + at as u32).wrapping_add(5).wrapping_add(rel) == target
        })
        .map(|at| start + at as u32)
        .collect()
}

/// The menu handler stores the type (`0, 1, 4, 2` by the row `[0x64aba0]`)
/// and then, on every path, jumps to the call that stores the mission
/// parameters; the type is not an argument of that call.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_f49_d_03_fu_rules_type_order_image_the_menu_stores_the_type_before_the_parameters() {
    let image = engine_image();

    assert_bytes(
        &image,
        0x0041_7543,
        &[0xa1, 0xa0, 0xab, 0x64, 0x00, 0x83, 0xf8, 0x03, 0x77],
        "mov eax, [0x64aba0]; cmp eax, 3; ja: the menu row selects the store",
    );
    assert_eq!(image_u32(&image, 0x0041_754d + 3), 0x0041_75d8);
    let table: Vec<u32> = (0..4)
        .map(|row| image_u32(&image, 0x0041_75d8 + 4 * row))
        .collect();
    assert_eq!(table, [0x0041_7554, 0x0041_7560, 0x0041_756c, 0x0041_7578]);

    // The four stores, rows 0..3 → types 0, 1, 4, 2 (`mov dword [0x718cd8], imm32`).
    for (store, ty) in [
        (0x0041_7554_u32, 0_u8),
        (0x0041_7560, 1),
        (0x0041_756c, 4),
        (0x0041_7578, 2),
    ] {
        assert_bytes(
            &image,
            store,
            &[0xc7, 0x05, 0xd8, 0x8c, 0x71, 0x00, ty, 0, 0, 0],
            "mov dword [0x718cd8], type",
        );
    }

    // Every row ends at 0x417582 (the first three by a short jmp), which
    // pushes the arguments of 0x4638f0 and calls it. Its arguments are the
    // handler's `esi` (the directory) and 1: the type is not passed.
    for jmp in [0x0041_755e_u32, 0x0041_756a, 0x0041_7576] {
        assert_bytes(&image, jmp, &[0xeb], "short jmp");
        let rel = image_bytes(&image, jmp + 1, 1)[0] as i8;
        assert_eq!(jmp + 2 + rel as u32, 0x0041_7582, "row exit at {jmp:#x}");
    }
    assert_bytes(
        &image,
        0x0041_7582,
        &[0x6a, 0x01, 0x56, 0xb9, 0x80, 0xb4, 0x71, 0x00],
        "push 1; push esi; mov ecx, 0x71b480",
    );
    assert_eq!(rel32_target(&image, 0x0041_7594), 0x0046_38f0);
    // 0x4638f0 reads only its two stack arguments (`[esp+4]`, `[esp+8]`) into
    // `[this+0xc1c]` and `[this+0xc20]`; it touches no `[0x718cd8]`.
    assert_bytes(
        &image,
        0x0046_38f0,
        &[
            0x8b, 0x44, 0x24, 0x04, 0x56, 0x8b, 0xf1, 0x8b, 0x4c, 0x24, 0x0c,
        ],
        "mov eax,[esp+4]; push esi; mov esi,ecx; mov ecx,[esp+0xc]",
    );
    assert_bytes(
        &image,
        0x0046_3907,
        &[
            0x89, 0x86, 0x1c, 0x0c, 0x00, 0x00, 0x89, 0x8e, 0x20, 0x0c, 0x00, 0x00,
        ],
        "mov [esi+0xc1c], eax; mov [esi+0xc20], ecx",
    );
    assert!(
        !(0x0046_38f0..0x0046_3954).any(|va| dword_sites(&image, LIVE_TYPE).contains(&va)),
        "0x4638f0 does not reference the live type"
    );
}

/// The load's `0x45a150` (one caller) resets the record to type 0 and then
/// re-reads the descriptor's `mission_type` into it, so the menu's store does
/// not reach the spawn routine unless the descriptor declares the same type.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_f49_d_03_fu_rules_type_order_image_the_load_resets_then_rereads_the_descriptor_type() {
    let image = engine_image();

    // One caller, in the mission load, behind the state==3 test (0x4639b0).
    assert_eq!(branch_sites_to(&image, 0x0045_a150), [0x0046_6bd5]);
    assert_bytes(&image, 0x0046_6bcc, &[0xe8], "call 0x4639b0 (state == 3)");
    assert_eq!(rel32_target(&image, 0x0046_6bcc), 0x0046_39b0);
    assert_bytes(
        &image,
        0x0046_6bd1,
        &[0x84, 0xc0, 0x74, 0x12, 0xe8],
        "test al, al; je 0x466be7; call 0x45a150",
    );
    assert_bytes(
        &image,
        0x0046_39b0,
        &[
            0x8b, 0x91, 0x00, 0x07, 0x00, 0x00, 0x33, 0xc0, 0x83, 0xfa, 0x03, 0x0f, 0x94, 0xc0,
        ],
        "state == 3",
    );

    // 0x45a150 starts with the reset 0x45a120 → the initialiser 0x458ff0 on
    // the record, whose first store is `[ecx] = 0`.
    assert_eq!(rel32_target(&image, 0x0045_a152), 0x0045_a120);
    assert_bytes(
        &image,
        0x0045_a138,
        &[0xb9, 0xd8, 0x8c, 0x71, 0x00, 0xe8],
        "mov ecx, 0x718cd8; call",
    );
    assert_eq!(rel32_target(&image, 0x0045_a13d), 0x0045_8ff0);
    assert_bytes(
        &image,
        0x0045_8ff3,
        &[0x8b, 0xe9],
        "mov ebp, ecx: the record",
    );
    assert_bytes(
        &image,
        0x0045_8ffa,
        &[0xc7, 0x45, 0x00, 0, 0, 0, 0],
        "mov dword [ebp], 0: the type becomes dogfight_ace",
    );

    // Then the descriptor is parsed into the record: `0x459390` with
    // ecx = 0x718cd8, esi = the parsed `ia.zrd`.
    assert_bytes(
        &image,
        0x0045_a222,
        &[0xb9, 0xd8, 0x8c, 0x71, 0x00, 0xe8],
        "mov ecx, 0x718cd8; call 0x459390",
    );
    assert_eq!(rel32_target(&image, 0x0045_a227), 0x0045_9390);
    assert_eq!(
        branch_sites_to(&image, 0x0045_9390),
        [0x0045_a227],
        "the descriptor reader has this one caller"
    );

    // The reader: key `mission_type` (0x62555c) → name → index (0x458c20) →
    // `[ebx] = index` unless it is the sentinel 5; an absent key or an
    // unrecognised name leaves the reset 0.
    assert_bytes(
        &image,
        0x0045_93b5,
        &[0x68, 0x5c, 0x55, 0x62, 0x00],
        "push \"mission_type\"",
    );
    assert_bytes(
        &image,
        0x0045_93c7,
        &[0x85, 0xc0, 0xbf, 5, 0, 0, 0, 0x74, 0x15],
        "test eax, eax; mov edi, 5; je 0x4593e5: an absent key",
    );
    assert_eq!(rel32_target(&image, 0x0045_93d7), 0x0045_8c20);
    assert_bytes(
        &image,
        0x0045_93df,
        &[0x3b, 0xc7, 0x74, 0x02, 0x89, 0x03],
        "cmp eax, edi; je; mov [ebx], eax: the type store, skipped for the sentinel",
    );
    assert_eq!(
        image_bytes(&image, 0x0062_555c, 13),
        b"mission_type\0",
        "the key text"
    );
    assert_eq!(image_bytes(&image, 0x0062_56a4, 7), b"ia.zrd\0");

    // The menu's stores precede this in time (menu handler → 0x4638f0 now,
    // the load later) and nothing between re-applies them: the whole list of
    // references to the live type, classified by the opcode bytes before the
    // address.
    let sites = dword_sites(&image, LIVE_TYPE);
    let mut stores = Vec::new();
    let mut record_pointer = Vec::new();
    let mut reads = Vec::new();
    for &site in &sites {
        match image_bytes(&image, site - 2, 2) {
            [0xc7, 0x05] => stores.push(site),
            [_, 0xb9] => record_pointer.push(site),
            // mov r32,[m]; mov eax,[m] (a1); cmp [m],imm8 (83 3d); cmp [m],r32 (39 2d).
            [_, 0xa1] | [0x83, 0x3d] | [0x39, 0x2d] | [0x8b, 0x0d | 0x1d] => reads.push(site),
            other => panic!("unclassified reference to the live type at {site:#x}: {other:x?}"),
        }
    }
    assert_eq!(
        stores,
        [0x0041_7556, 0x0041_7562, 0x0041_756e, 0x0041_757a],
        "the only direct stores are the menu's"
    );
    assert_eq!(
        record_pointer,
        [0x0045_8bd1, 0x0045_8bf1, 0x0045_a139, 0x0045_a223],
        "the record's address as `this`: the static constructor and destructor, \
         the reset 0x45a120 and the reader 0x45a150 calls"
    );
    assert_eq!(reads.len(), sites.len() - 8);
    assert_eq!(reads.len(), 14, "everything else only reads the type");
    // The `this` users are the four already named. The constructor/destructor
    // run at start-up/exit (`0x458bd0` static init via `0x5f6e40` atexit), so
    // no path writes the type between the load's reset and the spawn routine.
    assert_eq!(
        branch_sites_to(&image, 0x0045_8ff0),
        [0x0045_a13d],
        "the initialiser is called only by the load's reset"
    );
}

// ------------------------------------------------------------- retail --------

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"))
}

/// Whatever type the menu offers, the type that runs is the directory's own
/// default: every retail descriptor declares one, none declares
/// `dogfight_ace`, so the ace is never seated and the `num_wingmen` zeroing
/// at `0x459433` never fires.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f49_d_03_fu_rules_type_order_retail_each_directory_runs_its_descriptor_default() {
    let survey = survey_retail_instant_action(&game_dir()).expect("the survey reads");
    assert_eq!(survey.directories().len(), 8);

    let mut running = Vec::new();
    let mut offered_total = 0;
    for directory in survey.directories() {
        let default = directory
            .default_mission_type()
            .unwrap_or_else(|| panic!("{} declares no mission_type", directory.container()));
        assert!(
            IA_MISSION_TYPE_VOCABULARY.contains(&default),
            "{}: {default} is a recognised name (not the reader's sentinel 5)",
            directory.container()
        );
        assert_ne!(default, "dogfight_ace", "{}", directory.container());
        assert!(
            directory
                .offered()
                .iter()
                .any(|o| o.mission_type() == default),
            "{}: the default is one of the offered types, with spawn points",
            directory.container()
        );
        offered_total += directory.offered().len();
        running.push((directory.container().to_owned(), default.to_owned()));
    }
    assert_eq!(offered_total, 30, "30 menu (directory, type) rows");
    assert_eq!(
        running.len(),
        8,
        "8 launches actually distinct by type order"
    );

    let reachable: BTreeSet<&str> = running.iter().map(|(_, ty)| ty.as_str()).collect();
    assert_eq!(
        reachable,
        BTreeSet::from(["dogfight_squadron", "stunt_flying", "zeppelin_run"]),
        "dogfight_ace and ground_target are never what runs"
    );
    let by_type = |name: &str| -> Vec<&str> {
        running
            .iter()
            .filter(|(_, ty)| ty == name)
            .map(|(dir, _)| dir.as_str())
            .collect()
    };
    assert_eq!(
        by_type("dogfight_squadron"),
        ["zbd/c1/ia1/zrdr.zbd", "zbd/c3/ia1/zrdr.zbd"]
    );
    assert_eq!(
        by_type("stunt_flying"),
        [
            "zbd/c1b/ia1/zrdr.zbd",
            "zbd/c2/ia1/zrdr.zbd",
            "zbd/c4/ia1/zrdr.zbd",
            "zbd/c5/ia1/zrdr.zbd"
        ]
    );
    assert_eq!(
        by_type("zeppelin_run"),
        ["zbd/c1c/ia1/zrdr.zbd", "zbd/c2b/ia1/zrdr.zbd"]
    );
}

// ------------------------------------------------------------- finding --------

#[test]
fn accept_f49_d_03_fu_rules_type_order_the_finding_records_the_measurement() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let finding = fs::read_to_string(root.join(FINDING)).expect("the finding is committed");
    for needle in [
        "f49.d.03.ia-type-order",
        "0x417554",
        "0x4638f0",
        "0x45a150",
        "0x45a120",
        "0x458ff0",
        "0x459390",
        "0x4593d7",
        "0x466bd5",
        "0x459433",
        "dogfight_ace",
        "dogfight_squadron",
        "zeppelin_run",
        "stunt_flying",
        "static",
        "unmeasured",
    ] {
        assert!(finding.contains(needle), "the finding mentions {needle}");
    }
}
