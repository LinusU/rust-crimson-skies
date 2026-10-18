//! Acceptance stage M13-B-FU1: what the original does with the `SET_AI_NET`
//! actor `britkestrel_1` (Rally #1243, follow-up of M13-B #295).
//!
//! M13-B measured that the actor operand of the control member's one
//! `SET_AI_NET` site is declared by no archive of the installation. That
//! measurement was taken on `ZBD/C3/M03/zrdr.zbd`; the corrected campaign
//! layout (M01-ARCHIVE-DECISION) binds that archive to work order **M03**, and
//! M13 to `ZBD/C2/M03`, whose record spells no such name. So the limitation
//! belongs to M03's record, and this stage records it there.
//!
//! The mechanism is read from the owner's decrypted engine image
//! (`$CS_ENGINE_IMAGE`, read-only, nothing of it committed) by static reading:
//! the `SET_AI_NET` consumer at `0x46a000` looks the actor up by name
//! (`_stricmp`) in the vehicle registry `0x71dab8`, then in the zeppelin
//! registry `0x71df80`, and on a second miss logs
//! `SET_AI_NET: can't find vehicle or zeppelin %s` and returns. The vehicle
//! registry's insert in the record-driven constructor is pinned beside it.
//! What static reading does **not** settle — whether any other path registers
//! the name at run time — stays an unknown of M03's record, resolved by M03-C
//! (an original reference run) or a trace of the registry inserts.
//!
//! The retail tests are `#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]`;
//! the guard test at the end runs in CI.

use std::sync::OnceLock;

use cs_assets::install::sha256;
use cs_content::campaign_bindings::{MissionLabel, SourceContext, mission_scoped_unknowns};
use cs_content::coordinates::{load_engine_image, original_image_digest};

use super::m17_b::{game_dir, raw_files_containing};
use crate::campaign::common::{load_inventory, repo_path};

const IMAGE_VA_BASE: usize = 0x40_0000;
const ACTOR: &str = "britkestrel_1";
const NODE: &str = "M3BritAce";
const ARCHIVE: &str = "ZBD/C3/M03/zrdr.zbd";

/// SHA-256 of the 168 bytes of the `SET_AI_NET` consumer at `0x46a000`.
const SET_AI_NET_CONSUMER_SHA256: &str =
    "8625eebc0ea12311b6ddf12045569284f814e3d439ea3b448c997f663f319abd";
/// SHA-256 of the 76-byte case-insensitive vehicle-registry lookup at `0x4aff10`.
const VEHICLE_LOOKUP_SHA256: &str =
    "20788d196a38aac2717c1d487b2c116f0d461c1854e7eea28298003f6fd6fb58";
/// SHA-256 of the 46 bytes at `0x47c52d` that insert a constructed vehicle
/// into the registry `0x71dab8` (list insert `0x5b3330`).
const VEHICLE_INSERT_SHA256: &str =
    "64593949d90b4fa47925079ec22e45ea9f7bf47ca8f1c62806da1896aef8c85b";

fn image() -> Vec<u8> {
    let loaded = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    assert_eq!(loaded.digest, original_image_digest());
    loaded.bytes
}

fn at(image: &[u8], va: usize, len: usize) -> &[u8] {
    &image[va - IMAGE_VA_BASE..va - IMAGE_VA_BASE + len]
}

fn digest_hex(bytes: &[u8]) -> String {
    sha256(bytes).to_hex()
}

fn count_ci(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|window| window.eq_ignore_ascii_case(needle))
        .count()
}

fn context() -> &'static SourceContext {
    static CONTEXT: OnceLock<SourceContext> = OnceLock::new();
    CONTEXT.get_or_init(|| {
        SourceContext::read(&game_dir()).expect("the installation yields a source context")
    })
}

fn binding_of(work_order: &str) -> cs_content::campaign_bindings::SourceBinding {
    let title = load_inventory()
        .iter()
        .find(|(label, _)| label.as_str() == work_order)
        .map(|(_, title)| title.clone())
        .unwrap_or_else(|| panic!("the declared inventory has no {work_order} work order"));
    context()
        .bind(
            MissionLabel::new(work_order).unwrap_or_else(|error| panic!("{work_order}: {error}")),
            &title,
        )
        .unwrap_or_else(|error| panic!("{work_order} binds to the original data: {error}"))
}

fn label(work_order: &str) -> MissionLabel {
    MissionLabel::new(work_order).expect("label")
}

/// **A name no file or literal declares is a miss in the engine's consumer.**
///
/// The actor is spelled in exactly one installation file (M03's reader
/// archive) and appears nowhere in the image; its node operand is also spelled
/// in the chapter world's node index. The consumer and the registry lookup are
/// pinned by digest, the consumer's miss string is present once, and the
/// vehicle registry's insert is the record-driven constructor's.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m13_b_fu1_an_undeclared_actor_name_misses_in_the_set_ai_net_consumer() {
    assert_eq!(
        raw_files_containing(ACTOR),
        vec![ARCHIVE.to_owned()],
        "{ACTOR} is spelled in exactly M03's reader archive"
    );
    assert!(
        raw_files_containing(NODE).contains(&"ZBD/C3/zrdr.zbd".to_owned()),
        "the node operand resolves in the chapter world's index"
    );

    let image = image();
    assert_eq!(count_ci(&image, b"britkestrel"), 0, "no image literal");
    assert_eq!(count_ci(&image, ACTOR.as_bytes()), 0);
    assert_eq!(
        digest_hex(at(&image, 0x46a000, 168)),
        SET_AI_NET_CONSUMER_SHA256
    );
    assert_eq!(digest_hex(at(&image, 0x4aff10, 76)), VEHICLE_LOOKUP_SHA256);
    assert_eq!(digest_hex(at(&image, 0x47c52d, 46)), VEHICLE_INSERT_SHA256);

    // The consumer reaches both registries and then the miss message.
    let consumer = at(&image, 0x46a000, 168);
    let has = |needle: &[u8]| consumer.windows(needle.len()).any(|w| w == needle);
    assert!(has(&0x71dab8u32.to_le_bytes()), "vehicle registry");
    assert!(has(&0x71df80u32.to_le_bytes()), "zeppelin registry");
    assert!(has(&0x626f18u32.to_le_bytes()), "miss message");
    let miss = &image[0x626f18 - IMAGE_VA_BASE..];
    assert!(miss.starts_with(b"SET_AI_NET: can't find vehicle or zeppelin %s\0"));
    assert_eq!(count_ci(&image, b"SET_AI_NET: can't find"), 1);
}

/// **The limitation is M03's, and the committed record carries it.**
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m13_b_fu1_the_derived_m03_record_carries_the_actor_limitation() {
    let m03 = binding_of("M03");
    let derived = m03.to_json();
    assert!(derived.contains("SET_AI_NET actor `britkestrel_1`"));
    assert!(!m03.is_verified());
    let committed = std::fs::read_to_string(repo_path("missions/bindings/M03.json"))
        .expect("missions/bindings/M03.json exists");
    assert_eq!(
        committed, derived,
        "missions/bindings/M03.json was not regenerated through production code"
    );
    // M13's own archive carries no such name.
    assert!(
        !binding_of("M13").to_json().contains(ACTOR),
        "M13's record must not carry M03's actor"
    );
}

/// **Regeneration harness, not an acceptance test.** Rewrites
/// `missions/bindings/M03.json` from the production derivation; run by hand
/// with `CS_GAME_DIR` set and `CS_REGENERATE_M03_BINDING=1`.
#[test]
#[ignore = "regeneration harness: needs CS_GAME_DIR and CS_REGENERATE_M03_BINDING=1"]
fn regenerate_m13_b_fu1_writes_the_committed_m03_record_through_production() {
    assert_eq!(
        std::env::var("CS_REGENERATE_M03_BINDING").as_deref(),
        Ok("1"),
        "set CS_REGENERATE_M03_BINDING=1 to rewrite missions/bindings/M03.json"
    );
    let binding = binding_of("M03");
    binding
        .validate()
        .expect("the derived record is consistent");
    let path = repo_path("missions/bindings/M03.json");
    std::fs::write(&path, binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
}

/// **CI guard: the actor limitation reaches M03 alone.** No original data.
#[test]
fn accept_m13_b_fu1_the_actor_limitation_is_an_unknown_of_m03_alone() {
    let m03 = mission_scoped_unknowns(&label("M03"));
    let entry = m03
        .iter()
        .find(|entry| entry.contains(ACTOR))
        .expect("M03 carries the britkestrel_1 limitation");
    for needle in [NODE, "0x46a000", "can't find vehicle or zeppelin", "M03-C"] {
        assert!(
            entry.contains(needle),
            "the unknown no longer names {needle:?}"
        );
    }
    for (work_order, _) in load_inventory().iter() {
        if work_order.as_str() == "M03" {
            continue;
        }
        assert!(
            mission_scoped_unknowns(work_order)
                .iter()
                .all(|entry| !entry.contains(ACTOR)),
            "the limitation leaked into {}'s record",
            work_order.as_str()
        );
    }
}
