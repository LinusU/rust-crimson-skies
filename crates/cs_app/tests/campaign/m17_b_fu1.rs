//! Acceptance stage M17-B-FU1: what the original binds M17's record-only
//! names to (Rally #1256, follow-up of M17-B #307).
//!
//! M17-B measured that nine of the 128 texts M17's control member spells are
//! declared by no other `.zrd` member of the installation, and recorded them as
//! a backlog. This stage resolves them by **static reading** of the owner's
//! decrypted engine image (`$CS_ENGINE_IMAGE`) and of the installation's own
//! `strings.dll`; no original run was made (that is M17-C, `human_play`), so
//! nothing here is `verified_original`.
//!
//! The nine fall into four classes, and each class has one consumer:
//!
//! * **`WARP_VEHICLE`** is a directive *key*, spelled once in the image (the
//!   key table), so its absence from every other archive is expected;
//! * **`MSG_BRF_RMM2_OBJ1` / `MSG_BRF_RMM2_OBJ2`** are localized-string labels:
//!   `strings.dll` exports `ZLocGetStringID`, whose code walks an eight-byte
//!   `{name, id}` table of 1 023 rows and which holds both names, at the
//!   consecutive ids 16 420 and 16 421; both ids are rows of the same image's
//!   string catalog. The earlier "declared by no `.zrd` member" was a search of
//!   the wrong declaration site;
//! * **`bhatbrigand_13` / `bhatbrigand_14`** (and the softer `bhatbrigand_4`)
//!   are `SET_AI_NET` actor names. The consumer looks the name up in the
//!   vehicle registry and then the zeppelin registry; a miss logs
//!   `SET_AI_NET: can't find vehicle or zeppelin %s` and does nothing else.
//!   Whether anything *else* registers the two names is not decided by static
//!   reading and stays recorded for M17-C;
//! * **the four `snd_c4-RM-m2_*` names** are `STOP_QUEUED_SOUNDS` operands; the
//!   consumer walks the queued-sound list for entries with that name and
//!   returns when there is none, so a name that matches nothing is a silent
//!   no-op;
//! * **`pp5`** is the fifth child of a `WARP_VEHICLE` point — a *node name*
//!   looked up (created empty when missing) in the node registry, and refused
//!   unless the node holds at least two positions.
//!
//! The retail tests are `#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]`;
//! the guard test at the end runs in CI.

use std::collections::BTreeSet;

use cs_assets::install::{discover, fingerprint, sha256};
use cs_content::config::{StringCatalog, StringLookup};
use cs_content::coordinates::{load_engine_image, original_image_digest};
use cs_content::stunts::ZrdValue;
use cs_formats::ParseContext;
use cs_types::asset_id::SourceSpan;

use super::m17_b::{
    CONTAINER, RECORD_ONLY_TEXTS, blocks_of, control_document, declarations_of, game_dir,
    raw_files_containing, sites, spelled, texts,
};

/// The image offset of a virtual address: the decrypted image is flat from
/// `0x400000` (`data/owner-static/engine-facts.toml`, `image_va_base`).
const IMAGE_VA_BASE: usize = 0x40_0000;

/// SHA-256 of the 168 bytes of the `SET_AI_NET` consumer at `0x46a000`.
const SET_AI_NET_CONSUMER_SHA256: &str =
    "8625eebc0ea12311b6ddf12045569284f814e3d439ea3b448c997f663f319abd";
/// SHA-256 of the 76-byte case-insensitive vehicle-registry lookup at `0x4aff10`.
const VEHICLE_LOOKUP_SHA256: &str =
    "20788d196a38aac2717c1d487b2c116f0d461c1854e7eea28298003f6fd6fb58";
/// SHA-256 of the 84-byte queued-sound sweep `STOP_QUEUED_SOUNDS` calls at `0x5920f0`.
const QUEUED_SOUND_STOP_SHA256: &str =
    "5a3a81162eb17f4f380d666fc600ab8153e3ad7c06d93fcd40b0a6a1bfc6cbef";
/// SHA-256 of the 627-byte node warp at `0x4940d0`.
const NODE_WARP_SHA256: &str = "4ea4afe1c50614f451f3081ba71bb9f4d76126ceadd285c76d12c36aabb3b7a0";
/// SHA-256 of the 644-byte find-or-create node lookup at `0x4511b0`.
const NODE_LOOKUP_SHA256: &str = "1bd669dc82f00624e266eb541655303c8343fc8ee6566276d89d550f9e72e28e";
/// SHA-256 of the installation's `strings.dll`.
const STRINGS_DLL_SHA256: &str = "7582fecaca42d21dd44eb95f896dcb415af790c7057ad81c8e1600ac0b445c21";
/// SHA-256 of the 95 bytes of `ZLocGetStringID` in `strings.dll`.
const GET_STRING_ID_SHA256: &str =
    "4853f7d6c74bc3fec7a509d5db4388502bfda47eba2a51de57bb0b28718f2e55";

fn image() -> Vec<u8> {
    let loaded = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    assert_eq!(loaded.digest, original_image_digest());
    loaded.bytes
}

/// `len` bytes of the image at virtual address `va`.
fn at(image: &[u8], va: usize, len: usize) -> &[u8] {
    &image[va - IMAGE_VA_BASE..va - IMAGE_VA_BASE + len]
}

fn digest_hex(bytes: &[u8]) -> String {
    sha256(bytes).to_hex()
}

fn count(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

/// The C string at `va`.
fn cstr(image: &[u8], va: usize) -> String {
    let tail = &image[va - IMAGE_VA_BASE..];
    let end = tail.iter().position(|b| *b == 0).expect("a terminator");
    String::from_utf8(tail[..end].to_vec()).expect("ascii")
}

fn u32_le(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("four bytes"))
}

/// The actor operands of every `SET_AI_NET` pair in the record.
fn net_actors() -> BTreeSet<String> {
    let blocks = blocks_of(&control_document());
    spelled(&blocks, "SET_AI_NET")
        .into_iter()
        .flatten()
        .filter_map(|pair| match pair {
            ZrdValue::List(children) => {
                children.first().and_then(|c| c.as_text()).map(String::from)
            }
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// MSG_BRF_RMM2_OBJ1 / _OBJ2: localized-string labels
// ---------------------------------------------------------------------------

/// **The two `IDENTITY` message labels are rows of `strings.dll`'s name table.**
///
/// `ZLocGetStringID` (the DLL's only export) is pinned by digest; its own
/// bytes name the table (`0x10006030`), the stride (eight) and the end
/// (`0x10008028`), so the layout is read from the code rather than assumed.
/// Both names are found at consecutive ids and each id is a row of the
/// production string catalog read from the same DLL.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m17_b_fu1_the_msg_labels_resolve_through_the_string_dlls_name_table() {
    // The record spells exactly these two `MSG_` labels, both as IDENTITY operands.
    let blocks = blocks_of(&control_document());
    let labels: BTreeSet<&str> = blocks
        .iter()
        .flat_map(|(_, directives)| directives.iter())
        .filter(|directive| directive.key == "IDENTITY")
        .flat_map(texts)
        .filter(|text| text.starts_with("MSG_"))
        .collect();
    assert_eq!(
        labels,
        BTreeSet::from(["MSG_BRF_RMM2_OBJ1", "MSG_BRF_RMM2_OBJ2"]),
        "the record's IDENTITY labels changed"
    );

    let dll_path = game_dir().join("strings.dll");
    let dll = std::fs::read(&dll_path).expect("strings.dll is in the installation");
    assert_eq!(digest_hex(&dll), STRINGS_DLL_SHA256);

    // ZLocGetStringID: file offset 0x1010 (virtual 0x10001010) for 95 bytes.
    let code = &dll[0x1010..0x1010 + 95];
    assert_eq!(digest_hex(code), GET_STRING_ID_SHA256);
    // `mov edi, <table>` at +0x0a, `add edi, 8` at +0x40, `cmp edi, <end>` at +0x44.
    assert_eq!(code[0x0a], 0xbf, "mov edi, imm32");
    assert_eq!(code[0x40..0x43], [0x83, 0xc7, 0x08], "add edi, 8");
    assert_eq!(code[0x44..0x46], [0x81, 0xff], "cmp edi, imm32");
    let table = u32_le(code, 0x0b) as usize;
    let end = u32_le(code, 0x46) as usize;
    assert_eq!((table, end), (0x1000_6030, 0x1000_8028));
    // `.text`, `.rdata` and `.data` are mapped at their file offsets here.
    let rows = (end - table) / 8;
    assert_eq!(rows, 1023);

    let name_table: Vec<(String, u32)> = (0..rows)
        .map(|row| {
            let at = table - 0x1000_0000 + row * 8;
            let name_va = u32_le(&dll, at) as usize - 0x1000_0000;
            let id = u32_le(&dll, at + 4);
            let tail = &dll[name_va..];
            let stop = tail.iter().position(|b| *b == 0).expect("a terminator");
            (String::from_utf8(tail[..stop].to_vec()).expect("ascii"), id)
        })
        .collect();
    let id_of = |name: &str| -> u32 {
        let matches: Vec<u32> = name_table
            .iter()
            .filter(|(candidate, _)| candidate == name)
            .map(|(_, id)| *id)
            .collect();
        assert_eq!(matches.len(), 1, "{name} must be one row of the name table");
        matches[0]
    };
    assert_eq!(id_of("MSG_BRF_RMM2_OBJ1"), 16_420);
    assert_eq!(id_of("MSG_BRF_RMM2_OBJ2"), 16_421);

    // The same ids are rows of the production string catalog of the same image.
    let found = discover(&game_dir()).expect("production discovery reads the installation");
    let source = SourceSpan::new(
        fingerprint(&found.manifest),
        "strings.dll",
        None,
        0,
        dll.len() as u64,
        None,
    )
    .expect("a loose file is its own source");
    let mut context = ParseContext::with_defaults("strings.dll");
    let catalog = StringCatalog::read(&mut context, source, &dll).expect("strings.dll reads");
    for id in [16_420, 16_421] {
        match catalog.resolve(id, None) {
            StringLookup::Found(row) => assert!(
                row.text.as_deref().is_some_and(|text| !text.is_empty()),
                "string {id} decodes to a non-empty text"
            ),
            other => panic!("string {id} is not one row of strings.dll: {other:?}"),
        }
    }
    // The engine reaches the table through a function pointer, so it holds no
    // copy of the label in its own image.
    let image = image();
    assert_eq!(count(&image, b"MSG_BRF_"), 0);
}

// ---------------------------------------------------------------------------
// bhatbrigand_*: SET_AI_NET actor names
// ---------------------------------------------------------------------------

/// **A `SET_AI_NET` actor that no registry holds is logged and skipped.**
///
/// The consumer at `0x46a000` looks the name up in the vehicle registry
/// (`0x71dab8`, then the case-insensitive walk at `0x4aff10`), then in the
/// zeppelin registry (`0x71df80`), and otherwise logs and returns. The two
/// undeclared names, and the softer `bhatbrigand_4`, are not in M17's own
/// aircraft table, so unless something else registers them they take the miss
/// path.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m17_b_fu1_an_unregistered_actor_name_is_logged_and_skipped_by_set_ai_net() {
    let image = image();

    let consumer = at(&image, 0x46_a000, 168);
    assert_eq!(digest_hex(consumer), SET_AI_NET_CONSUMER_SHA256);
    // Two registry probes, in order: vehicles, then zeppelins.
    assert_eq!(
        consumer[0x08..0x0d],
        [0xb9, 0xb8, 0xda, 0x71, 0x00],
        "mov ecx, 0x71dab8"
    );
    assert_eq!(
        consumer[0x49..0x4e],
        [0xb9, 0x80, 0xdf, 0x71, 0x00],
        "mov ecx, 0x71df80"
    );
    // Each probe's miss branches (`test eax, eax; je`) to the next, and the last
    // one reaches the log-and-return tail at +0x87 with no write in between.
    assert_eq!(consumer[0x12..0x16], [0x85, 0xc0, 0x74, 0x30]);
    assert_eq!(consumer[0x53..0x57], [0x85, 0xc0, 0x74, 0x30]);
    assert_eq!(consumer[0x87..0x8b], [0x8b, 0x16, 0x52, 0x68]);
    let log = u32_le(consumer, 0x8b) as usize;
    assert_eq!(
        cstr(&image, log),
        "SET_AI_NET: can't find vehicle or zeppelin %s"
    );
    // `add esp, 0x14; pop esi; ret`: the tail returns with nothing stored.
    assert_eq!(consumer[0xa3..0xa8], [0x83, 0xc4, 0x14, 0x5e, 0xc3]);

    // The vehicle lookup is a `_stricmp` walk that returns zero when exhausted.
    let lookup = at(&image, 0x4a_ff10, 76);
    assert_eq!(digest_hex(lookup), VEHICLE_LOOKUP_SHA256);
    assert_eq!(
        lookup[0x16..0x1c],
        [0x8b, 0x2d, 0x14, 0x02, 0xa2, 0x00],
        "mov ebp, [_stricmp]"
    );
    assert_eq!(
        lookup[0x2b..0x2d],
        [0x74, 0x15],
        "equal -> return the entry"
    );
    assert_eq!(lookup[0x3b..0x3d], [0x33, 0xc0], "xor eax, eax: no entry");

    // The names are data, not engine constants.
    for name in ["bhatbrigand_13", "bhatbrigand_14", "bhatbrigand_4"] {
        assert_eq!(
            count(&image, name.as_bytes()),
            0,
            "{name} is not in the image"
        );
    }

    // The record spells the three names at SET_AI_NET sites, and M17's own
    // archive is the only file of the installation that spells 13 and 14.
    let actors = net_actors();
    for name in ["bhatbrigand_13", "bhatbrigand_14", "bhatbrigand_4"] {
        assert!(actors.contains(name), "{name} is a SET_AI_NET actor");
    }
    for name in ["bhatbrigand_13", "bhatbrigand_14"] {
        assert!(
            declarations_of(name).is_empty(),
            "{name} is declared somewhere"
        );
        let files = raw_files_containing(name);
        assert_eq!(files.len(), 1, "{name}: {files:?}");
        assert!(files[0].eq_ignore_ascii_case(CONTAINER), "{files:?}");
    }
    // `bhatbrigand_4` is declared by sibling missions' tables only, never by
    // M17's own archive.
    let hits = declarations_of("bhatbrigand_4");
    assert!(!hits.is_empty());
    assert!(
        hits.iter()
            .all(|(container, _)| !container.eq_ignore_ascii_case(CONTAINER)),
        "{hits:?}"
    );
}

// ---------------------------------------------------------------------------
// snd_c4-RM-m2_*: queued sounds
// ---------------------------------------------------------------------------

/// **A `STOP_QUEUED_SOUNDS` name that matches no queued sound does nothing.**
///
/// The sweep at `0x5920f0` asks the queue (`[0x639eac]`) for an entry with
/// the name through `0x5791a0` and `return`s when the answer is null; only a
/// match is taken off the list and flagged. The names are therefore not
/// declarations that must exist, but strings compared with whatever was
/// queued under them.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m17_b_fu1_unmatched_queued_sound_names_are_skipped_silently() {
    let image = image();

    let sweep = at(&image, 0x59_20f0, 84);
    assert_eq!(digest_hex(sweep), QUEUED_SOUND_STOP_SHA256);
    // Empty queue -> return; a null lookup result -> return; a match is unlinked
    // (`0x5790a0`) and rescheduled (`0x592010`) and the walk repeats.
    assert_eq!(sweep[0..7], [0xa1, 0xac, 0x9e, 0x63, 0x00, 0x85, 0xc0]);
    assert_eq!(sweep[0x09..0x0b], [0x74, 0x46], "empty queue returns");
    assert_eq!(sweep[0x20..0x22], [0x85, 0xf6], "test esi, esi");
    assert_eq!(sweep[0x22..0x24], [0x74, 0x2d], "no match returns");
    assert_eq!(
        sweep[0x51..0x54],
        [0x5f, 0x5e, 0xc3],
        "pop edi; pop esi; ret"
    );

    // The key's own string and its place in the image.
    assert_eq!(cstr(&image, 0x62_6778), "STOP_QUEUED_SOUNDS");

    let blocks = blocks_of(&control_document());
    let operands: BTreeSet<String> = spelled(&blocks, "STOP_QUEUED_SOUNDS")
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_text().map(String::from))
        .collect();
    assert_eq!(operands.len(), 6, "the six STOP_QUEUED_SOUNDS operands");
    let undeclared: BTreeSet<&str> = operands
        .iter()
        .map(String::as_str)
        .filter(|name| declarations_of(name).is_empty())
        .collect();
    assert_eq!(
        undeclared,
        BTreeSet::from([
            "snd_c4-RM-m2_Blacke_14",
            "snd_c4-RM-m2_Blacke_16",
            "snd_c4-RM-m2_Zachary_13",
            "snd_c4-RM-m2_Zachary_15",
        ])
    );
    for name in &undeclared {
        assert_eq!(
            count(&image, name.as_bytes()),
            0,
            "{name} is not in the image"
        );
        let files = raw_files_containing(name);
        assert_eq!(files.len(), 1, "{name}: {files:?}");
        assert!(files[0].eq_ignore_ascii_case(CONTAINER), "{files:?}");
    }
}

// ---------------------------------------------------------------------------
// pp5 and WARP_VEHICLE
// ---------------------------------------------------------------------------

/// **`pp5` is a node name, and a node with fewer than two positions is not a
/// warp target.**
///
/// `WARP_VEHICLE`'s fifth point child is looked up at `0x4940d0` through
/// `0x4511b0` over the node registry `0x6541b0`; that lookup creates an empty
/// node when the name is missing, and the warp returns unless the node's
/// position list (12-byte entries) holds at least two. The label is declared
/// only by `ZBD/C2/M02`'s table, not by anything M17 loads, so a draw of that
/// point is expected to leave the vehicle where it is. What fills the registry
/// is **not** traced here (see the finding).
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m17_b_fu1_the_pp5_warp_node_is_a_registry_lookup_that_refuses_an_empty_node() {
    let image = image();

    let warp = at(&image, 0x49_40d0, 627);
    assert_eq!(digest_hex(warp), NODE_WARP_SHA256);
    // `push 1; push name; push 0; mov ecx, 0x6541b0; call 0x4511b0`.
    assert_eq!(
        warp[0x26..0x30],
        [0x6a, 0x01, 0x50, 0x6a, 0x00, 0xb9, 0xb0, 0x41, 0x65, 0x00]
    );
    // A null node, a null position list, and fewer than two 12-byte positions
    // (`cmp edx, 2; jl`) all jump to the same return at +0x26a (every `je`/`jl` target is 0x4940d0 + 0x26a).
    assert_eq!(
        warp[0x37..0x3d],
        [0x85, 0xdb, 0x0f, 0x84, 0x2b, 0x02],
        "null node"
    );
    assert_eq!(
        warp[0x42..0x48],
        [0x85, 0xc0, 0x0f, 0x84, 0x20, 0x02],
        "null position list"
    );
    assert_eq!(
        warp[0x5f..0x66],
        [0x83, 0xfa, 0x02, 0x0f, 0x8c, 0x02, 0x02],
        "fewer than two"
    );
    assert_eq!(
        warp[0x26a..0x273],
        [0x5d, 0x5b, 0x81, 0xc4, 0xac, 0x00, 0x00, 0x00, 0xc3],
        "the shared return"
    );

    let lookup = at(&image, 0x45_11b0, 644);
    assert_eq!(digest_hex(lookup), NODE_LOOKUP_SHA256);

    // The named point is the fifth child of the last of four points, in M17's
    // single WARP_VEHICLE site.
    let blocks = blocks_of(&control_document());
    let warp_sites = sites(&blocks, 23, "WARP_VEHICLE");
    assert_eq!(warp_sites.len(), 1);
    let site = &warp_sites[0];
    assert_eq!(site[0].as_text(), Some("bhatgyro_1"));
    let points = &site[1..];
    assert_eq!(points.len(), 4);
    let labels: Vec<Option<&str>> = points
        .iter()
        .map(|point| match point {
            ZrdValue::List(children) => children.get(4).and_then(ZrdValue::as_text),
            _ => panic!("a warp point is a list"),
        })
        .collect();
    assert_eq!(labels, [None, None, None, Some("pp5")]);

    // Declared by another mission's table only.
    assert_eq!(
        declarations_of("pp5"),
        [("ZBD/C2/M02/zrdr.zbd".to_owned(), "aiv.zrd".to_owned())]
    );
    assert_eq!(count(&image, b"pp5"), 0);
}

/// **`WARP_VEHICLE` is a key of the executable.**
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m17_b_fu1_warp_vehicle_is_a_directive_key_spelled_once_in_the_image() {
    let image = image();
    assert_eq!(count(&image, b"WARP_VEHICLE\0"), 1);
    assert_eq!(cstr(&image, 0x62_6674), "WARP_VEHICLE");
    // The installation spells it only in M17's own archive.
    let files = raw_files_containing("WARP_VEHICLE");
    assert_eq!(files.len(), 1, "{files:?}");
    assert!(files[0].eq_ignore_ascii_case(CONTAINER), "{files:?}");
}

// ---------------------------------------------------------------------------
// The accounting
// ---------------------------------------------------------------------------

/// How the original treats one of M17-B's nine record-only names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Resolution {
    /// A directive key of the executable.
    ExecutableKey,
    /// A label of `strings.dll`'s `ZLocGetStringID` name table.
    StringLabel,
    /// An actor name: a registry miss is logged and skipped.
    ActorMissLogged,
    /// A queued-sound name: no match is a no-op.
    QueuedSoundNoMatch,
}

/// The resolution recorded for each of the nine, by name.
fn resolution_of(name: &str) -> Option<Resolution> {
    match name {
        "WARP_VEHICLE" => Some(Resolution::ExecutableKey),
        "MSG_BRF_RMM2_OBJ1" | "MSG_BRF_RMM2_OBJ2" => Some(Resolution::StringLabel),
        "bhatbrigand_13" | "bhatbrigand_14" => Some(Resolution::ActorMissLogged),
        "snd_c4-RM-m2_Blacke_14"
        | "snd_c4-RM-m2_Blacke_16"
        | "snd_c4-RM-m2_Zachary_13"
        | "snd_c4-RM-m2_Zachary_15" => Some(Resolution::QueuedSoundNoMatch),
        _ => None,
    }
}

/// **Every one of M17-B's nine names has exactly one recorded resolution.**
///
/// Runs in CI: it reads only M17-B's committed set, so a tenth name, or a
/// name dropped from the set, fails here until the finding is updated.
#[test]
fn accept_m17_b_fu1_every_record_only_name_has_exactly_one_recorded_resolution() {
    let mut by_class = [0usize; 4];
    for name in RECORD_ONLY_TEXTS {
        let resolution = resolution_of(name)
            .unwrap_or_else(|| panic!("{name} has no recorded resolution (task #1256)"));
        by_class[resolution as usize] += 1;
    }
    // 1 key, 2 labels, 2 actors, 4 queued sounds.
    assert_eq!(by_class, [1, 2, 2, 4]);
}
