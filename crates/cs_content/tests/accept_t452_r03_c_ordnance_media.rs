//! Task #2166 (T452-R.03-C): what the effect-class names `FIRE`/`FLYOUT`/
//! `IMPACT`/`DESTROY_ANIMATION` (and the absent `SOUND_NAME`) of
//! `weapons.zrd` map to — measured in the decrypted engine image — and what
//! a record's launch sound is when it declares no `SOUND_NAME`.
//!
//! Task test prefix: `accept_t452_r03_c_`. Finding:
//! `docs/findings/2026-10-28-t452-r03-c-ordnance-media-ids.md`.
//!
//! The synthetic tests drive [`cs_content::ordnance::weapon_media`] and
//! [`cs_content::ordnance::SpannedDocument`]'s decoder over a `.zrd` member
//! built in the test; the `#[ignore]`d halves re-measure the image
//! (`CS_ENGINE_IMAGE`, base `0x400000`) and the installation (`CS_GAME_DIR`)
//! and fail loudly without them.

use cs_assets::install::sha256;
use cs_content::ordnance::{
    ORIGINAL_EFFECT_BLOCK_READER_VA, ORIGINAL_MOUNT_LOOPED_SOUND_VA, ORIGINAL_SOUND_GATE_VA,
    ORIGINAL_WEAPON_DESTROY_ANIMATION_VA, ORIGINAL_WEAPON_DESTROY_USE_VA,
    ORIGINAL_WEAPON_FIRE_BLOCK_VA, ORIGINAL_WEAPON_FLYOUT_BLOCK_VA,
    ORIGINAL_WEAPON_FLYOUT_MODEL_ATTACH_VA, ORIGINAL_WEAPON_IMPACT_BLOCK_VA,
    ORIGINAL_WEAPON_IMPACT_SOUND_VA, ORIGINAL_WEAPON_LAUNCH_SLOT_VA,
    ORIGINAL_WEAPON_LAUNCH_SOUND_VA, weapon_media,
};
use cs_content::original_airframe::SpannedDocument;
use cs_types::content::{ContentKind, Resolved};
use cs_types::evidence::{ClaimId, ClaimStatus};

const IMAGE_BASE: u64 = 0x40_0000;

// ----------------------------------------------------------- synthetic zrd ---

enum N {
    Int(u32),
    Text(&'static str),
    Owned(String),
    List(Vec<N>),
}

fn encode(node: &N, out: &mut Vec<u8>) {
    let word = |out: &mut Vec<u8>, value: u32| out.extend_from_slice(&value.to_le_bytes());
    let text = |out: &mut Vec<u8>, text: &str| {
        word(out, 3);
        word(out, u32::try_from(text.len()).expect("short"));
        out.extend_from_slice(text.as_bytes());
    };
    match node {
        N::Int(value) => {
            word(out, 1);
            word(out, *value);
        }
        N::Text(value) => text(out, value),
        N::Owned(value) => text(out, value),
        N::List(children) => {
            word(out, 4);
            word(out, u32::try_from(children.len() + 1).expect("short"));
            for child in children {
                encode(child, out);
            }
        }
    }
}

fn texts(values: &[&'static str]) -> N {
    N::List(values.iter().map(|name| N::Text(name)).collect())
}

/// A `weapons.zrd` member with `wep_00` carrying every effect-class key and
/// `wep_01` carrying none — the two shapes the member mixes.
fn member() -> Vec<u8> {
    let full = N::Owned("wep_00".to_owned());
    let body = N::List(vec![
        N::Text("FIRE"),
        N::List(vec![
            N::Text("ANIMATION"),
            texts(&["muzzle_a", "muzzle_b"]),
            N::Text("EFFECT"),
            texts(&["muzzle_fx"]),
            N::Text("SOUND"),
            texts(&["snd_shot"]),
        ]),
        N::Text("FLYOUT"),
        N::List(vec![
            N::Text("MODEL"),
            texts(&["dart"]),
            N::Text("MODEL_ANIMATION"),
            texts(&["dart_spin"]),
            N::Text("SOUND"),
            texts(&["snd_dart_loop"]),
        ]),
        N::Text("IMPACT"),
        N::List(vec![
            N::Text("default"),
            N::List(vec![
                N::Text("ANIMATION"),
                texts(&["ground_burst"]),
                N::Text("SURFACE_ANIMATION"),
                texts(&["ground_mark"]),
                N::Text("ANIMATION_MIN_RANGE"),
                texts(&["near_burst"]),
                N::Text("SOUND"),
                texts(&["snd_hit_a", "snd_hit_b"]),
                N::Text("BOUNCE_SOUND"),
                texts(&["snd_bounce"]),
                N::Text("RANDOM_ROTATE"),
                N::List(vec![N::Int(1)]),
            ]),
            N::Text("water"),
            N::List(vec![
                N::Text("ANIMATION"),
                texts(&["splash"]),
                N::Text("SOUND"),
                texts(&["snd_splash"]),
            ]),
        ]),
        N::Text("DESTROY_ANIMATION"),
        texts(&["dart_destroy_effect"]),
        N::Text("LOOPED_SOUND_NAME"),
        texts(&["snd_gun_loop"]),
    ]);
    let bare = N::List(vec![N::Text("ROCKET"), N::List(vec![N::Int(1)])]);
    let mut out = Vec::new();
    encode(
        &N::List(vec![
            N::Text("VERSION"),
            N::List(vec![N::Int(2)]),
            full,
            body,
            N::Owned("wep_01".to_owned()),
            bare,
        ]),
        &mut out,
    );
    out
}

fn decoded(bytes: &[u8]) -> SpannedDocument {
    SpannedDocument::decode(sha256(b"install"), "weapons.zrd", bytes, 100, sha256(bytes))
        .expect("the synthetic member decodes")
}

/// The `wep_00` body of the synthetic member, as `weapon_media` sees it.
fn full_media() -> cs_content::ordnance::OriginalWeaponMedia {
    let bytes = member();
    let document = decoded(&bytes);
    let children = document.root().as_list().expect("root is a list");
    let index = children
        .iter()
        .position(|child| child.as_text() == Some("wep_00"))
        .expect("wep_00 is declared");
    weapon_media(
        "wep_00",
        children[index + 1].as_list().expect("a record body"),
    )
}

#[test]
fn accept_t452_r03_c_effect_blocks_read_their_declared_names() {
    let media = full_media();
    let fire = media.fire.as_ref().expect("wep_00 declares FIRE");
    assert_eq!(fire.animations, ["muzzle_a", "muzzle_b"]);
    assert_eq!(fire.effect.as_deref(), Some("muzzle_fx"));
    assert_eq!(fire.sounds, ["snd_shot"]);
    let flyout = media.flyout.as_ref().expect("wep_00 declares FLYOUT");
    assert_eq!(flyout.model.as_deref(), Some("dart"));
    assert_eq!(flyout.model_animation.as_deref(), Some("dart_spin"));
    assert_eq!(flyout.sounds, ["snd_dart_loop"]);
    assert_eq!(
        media.destroy_animation.as_deref(),
        Some("dart_destroy_effect")
    );
    assert_eq!(media.looped_sound_name.as_deref(), Some("snd_gun_loop"));
    assert!(media.keys.contains("FLYOUT"));
}

#[test]
fn accept_t452_r03_c_impact_rows_keep_their_surface_names() {
    let media = full_media();
    assert_eq!(media.impact_rows.len(), 2);
    let default = media.default_impact_row().expect("a default row");
    assert_eq!(default.block.animations, ["ground_burst"]);
    assert_eq!(
        default.block.surface_animation.as_deref(),
        Some("ground_mark")
    );
    assert_eq!(
        default.block.animation_min_range.as_deref(),
        Some("near_burst")
    );
    assert_eq!(default.block.sounds, ["snd_hit_a", "snd_hit_b"]);
    assert_eq!(default.block.bounce_sounds, ["snd_bounce"]);
    assert_eq!(default.block.random_rotate, Some(true));
    // An undeclared surface resolves to the default row, the way the engine
    // copies row 0 into every soil the record does not name.
    let water = media.impact_row("water").expect("water row");
    assert_eq!(water.block.animations, ["splash"]);
    assert_eq!(media.impact_row("lava").unwrap().surface, "default");
}

#[test]
fn accept_t452_r03_c_launch_sound_is_the_first_fire_sound() {
    let media = full_media();
    assert_eq!(media.launch_sound(), Some("snd_shot"));
    assert_eq!(media.launch_sounds(), &["snd_shot".to_owned()]);
    // The muzzle visual: EFFECT takes precedence over the ANIMATION slots
    // (0x5af3e6), which is what these two accessors name separately.
    assert_eq!(media.launch_effect(), Some("muzzle_fx"));
    assert_eq!(media.launch_animation(), Some("muzzle_a"));
}

#[test]
fn accept_t452_r03_c_a_record_without_fire_sound_is_silent() {
    let bytes = member();
    let document = decoded(&bytes);
    let children = document.root().as_list().expect("root is a list");
    let index = children
        .iter()
        .position(|child| child.as_text() == Some("wep_01"))
        .expect("wep_01 is declared");
    let media = weapon_media("wep_01", children[index + 1].as_list().expect("body"));
    assert_eq!(media.launch_sound(), None);
    assert!(media.fire.is_none());
    assert!(media.flyout.is_none());
    assert!(media.impact_rows.is_empty());
    assert_eq!(media.destroy_animation, None);
    // wep_01 declares no LOOPED_SOUND_NAME either; and when a record
    // declares one it is the mount's gun loop, not the launch one-shot —
    // launch_sound() stays None for it.
    assert_eq!(media.mount_looped_sound(), None);
}

#[test]
fn accept_t452_r03_c_measured_media_ids_carry_the_declared_names() {
    let media = full_media();
    let claim = ClaimId::new("t452_r03_c.test").expect("valid claim id");
    let declared = media.measured_ordnance_media(&claim);
    let Resolved::Known(visual) = &declared.visual else {
        panic!("a declared FLYOUT MODEL resolves")
    };
    assert_eq!(visual.value.kind(), ContentKind::HardpointEquipment);
    assert_eq!(visual.value.as_str(), "hardpoint_equipment/dart");
    assert_eq!(visual.provenance.class, ClaimStatus::ObservedTool);
    let Resolved::Known(sound) = &declared.sound else {
        panic!("a declared FIRE SOUND resolves")
    };
    assert_eq!(sound.value.kind(), ContentKind::Sound);
    assert_eq!(sound.value.as_str(), "sound/snd_shot");
    assert_eq!(sound.provenance.class, ClaimStatus::ObservedTool);
    assert!(declared.particles.is_none());
}

#[test]
fn accept_t452_r03_c_missing_names_stay_unknown_never_invented() {
    let media = cs_content::ordnance::OriginalWeaponMedia {
        record: "wep_99".to_owned(),
        ..Default::default()
    };
    let claim = ClaimId::new("t452_r03_c.test").expect("valid claim id");
    let declared = media.measured_ordnance_media(&claim);
    assert!(!declared.visual.is_known());
    assert!(!declared.sound.is_known());
}

// -------------------------------------------------------------- the image ---

fn image() -> Vec<u8> {
    let path = std::env::var_os("CS_ENGINE_IMAGE").expect(
        "CS_ENGINE_IMAGE is not set: this measurement needs the owner-supplied decrypted \
         executable",
    );
    std::fs::read(&path).expect("the engine image reads")
}

fn at(image: &[u8], va: u64, len: usize) -> &[u8] {
    let offset = usize::try_from(va - IMAGE_BASE).expect("offset");
    &image[offset..offset + len]
}

fn cstr(image: &[u8], va: u64, len: usize) -> &[u8] {
    at(image, va, len)
}

/// The weapon parser's three keyed call sites: `FIRE` -> block at
/// `weapon + 0x94`, `FLYOUT` -> `weapon + 0xf8`, `IMPACT` -> rows at
/// `weapon + 0x15c`; `DESTROY_ANIMATION` -> `weapon + 0x188`.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_t452_r03_c_image_pins_the_block_parse_sites() {
    let image = image();
    // push "FIRE"; lea edx,[ebx+0x94]; push edx; push esi; call 0x5ae990
    assert_eq!(
        at(&image, ORIGINAL_WEAPON_FIRE_BLOCK_VA, 0x14),
        [
            0x68, 0xd8, 0xcf, 0x63, 0x00, 0x8d, 0x93, 0x94, 0x00, 0x00, 0x00, 0x52, 0x56, 0xe8,
            0x82, 0x08, 0x00, 0x00, 0x68, 0xd0
        ]
    );
    // push "FLYOUT"; lea eax,[ebx+0xf8]; push eax; push esi; call 0x5ae990
    assert_eq!(
        at(&image, ORIGINAL_WEAPON_FLYOUT_BLOCK_VA, 0x0e),
        [
            0x68, 0xd0, 0xcf, 0x63, 0x00, 0x8d, 0x83, 0xf8, 0x00, 0x00, 0x00, 0x50, 0x56, 0xe8
        ]
    );
    // push "IMPACT"; push esi; call 0x57a090 (keyed lookup)
    assert_eq!(
        at(&image, ORIGINAL_WEAPON_IMPACT_BLOCK_VA, 0x0b),
        [
            0x68, 0x98, 0xcf, 0x63, 0x00, 0x56, 0xe8, 0xb6, 0xbe, 0xfc, 0xff
        ]
    );
    // rows: mov ecx,[ebx+0x15c] (row 0), then stride 0x64 over the soil table
    assert_eq!(
        at(&image, 0x5a_e1ef, 7),
        [0x8b, 0x8b, 0x5c, 0x01, 0x00, 0x00, 0x50]
    );
    assert_eq!(at(&image, 0x5a_e216, 5), [0xbe, 0x64, 0x00, 0x00, 0x00]);
    // push "DESTROY_ANIMATION"; lookup; resolve; store at weapon + 0x188
    assert_eq!(
        at(&image, ORIGINAL_WEAPON_DESTROY_ANIMATION_VA, 6),
        [0x68, 0xfc, 0xd0, 0x63, 0x00, 0x56]
    );
    assert_eq!(
        at(&image, 0x5a_dc54, 6),
        [0x89, 0x83, 0x88, 0x01, 0x00, 0x00]
    );
    // The key strings the sites push.
    assert_eq!(cstr(&image, 0x63_cfd8, 5), b"FIRE\x00");
    assert_eq!(cstr(&image, 0x63_cfd0, 7), b"FLYOUT\x00");
    assert_eq!(cstr(&image, 0x63_cf98, 7), b"IMPACT\x00");
    assert_eq!(cstr(&image, 0x63_d0fc, 18), b"DESTROY_ANIMATION\x00");
}

/// The shared block reader's key pushes, in the order it handles them, and
/// the string each names.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_t452_r03_c_image_pins_the_block_reader_keys() {
    let image = image();
    let keys: [(u64, u64, &[u8]); 10] = [
        (0x5a_e9bf, 0x63_369c, b"EFFECT\x00"),
        (0x5a_e9f1, 0x63_d370, b"MODEL\x00"),
        (0x5a_ea20, 0x63_d35c, b"ANIMATION_ATTACHED\x00"),
        (0x5a_ea49, 0x63_d348, b"ANIMATION_MIN_RANGE\x00"),
        (0x5a_ea72, 0x63_d338, b"MODEL_ANIMATION\x00"),
        (0x5a_ea9b, 0x63_d324, b"SURFACE_ANIMATION\x00"),
        (0x5a_eac4, 0x63_d318, b"ANIMATION\x00"),
        (0x5a_eb33, 0x63_d308, b"RANDOM_ROTATE\x00"),
        (0x5a_eb57, 0x63_3458, b"SOUND\x00"),
        (0x5a_eba8, 0x63_d2f8, b"BOUNCE_SOUND\x00"),
    ];
    for (site, string_va, string) in keys {
        let push = at(&image, site, 5);
        assert_eq!(push[0], 0x68, "site {site:#x}: push imm32");
        assert_eq!(
            u32::from_le_bytes(push[1..5].try_into().expect("4 bytes")) as u64,
            string_va,
            "site {site:#x} pushes its key string"
        );
        assert_eq!(cstr(&image, string_va, string.len()), string);
    }
    // The reader's entry: block +0x5c is zeroed before the keys are read.
    assert_eq!(
        at(&image, ORIGINAL_EFFECT_BLOCK_READER_VA + 0x12, 7),
        [0xc7, 0x45, 0x5c, 0x00, 0x00, 0x00, 0x00]
    );
}

/// The launch-sound path: `weapon + 0xe8` selects the `FIRE>SOUND` slot, the
/// caller's helper `0x5ac120` bounds it against the declared count, and the
/// playback gate returns early on a missing handle — silence, not a
/// fallback.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_t452_r03_c_image_pins_the_launch_sound_path() {
    let image = image();
    // mov eax,[esi+0xe8]; cmp eax,-1; je skip; mov eax,[esi+eax*4+0xc4]; ...
    // call 0x593b80
    assert_eq!(
        at(&image, ORIGINAL_WEAPON_LAUNCH_SOUND_VA, 0x26),
        [
            0x8b, 0x86, 0xe8, 0x00, 0x00, 0x00, 0x83, 0xf8, 0xff, 0x74, 0x1b, 0x8b, 0x55, 0x20,
            0x8b, 0x84, 0x86, 0xc4, 0x00, 0x00, 0x00, 0x6a, 0x00, 0x52, 0x68, 0x00, 0x00, 0x80,
            0x3f, 0x50, 0xe8, 0x9d, 0x47, 0xfe, 0xff, 0x83, 0xc4, 0x10
        ]
    );
    // FIRE>EFFECT at weapon + 0xb8 takes precedence over the ANIMATION slots.
    assert_eq!(
        at(&image, 0x5a_f3e6, 6),
        [0x8b, 0x86, 0xb8, 0x00, 0x00, 0x00]
    );
    // The slot selector: index < [weapon+0xc0] (the SOUND count) writes
    // weapon + 0xe8; a signed compare keeps -1 below every count.
    assert_eq!(
        at(&image, ORIGINAL_WEAPON_LAUNCH_SLOT_VA, 0x23),
        [
            0x8b, 0x4c, 0x24, 0x04, 0x33, 0xc0, 0x85, 0xc9, 0x75, 0x01, 0xc3, 0x8b, 0x54, 0x24,
            0x08, 0x3b, 0x91, 0xc0, 0x00, 0x00, 0x00, 0x7d, 0x0b, 0x89, 0x91, 0xe8, 0x00, 0x00,
            0x00, 0xb8, 0x01, 0x00, 0x00, 0x00, 0xc3
        ]
    );
    // The gate: the descriptor is tested and a null returns early.
    assert_eq!(
        at(&image, ORIGINAL_SOUND_GATE_VA + 0x24, 7),
        [0x8b, 0x74, 0x24, 0x0c, 0x85, 0xf6, 0x0f]
    );
}

/// The `IMPACT` consumers: the row's `EFFECT` at `+0x24` played via
/// `0x525d90`, and the per-row `SOUND` picked uniformly at `0x5ad100` — the
/// detonation presentation is per surface, not one media id.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_t452_r03_c_image_pins_the_impact_consumers() {
    let image = image();
    // row +0x1c SURFACE_ANIMATION, played surface-aligned
    assert_eq!(
        at(&image, 0x5a_c9e4, 0x0f),
        [
            0x8b, 0x86, 0x5c, 0x01, 0x00, 0x00, 0x51, 0x8b, 0x4c, 0x07, 0x1c, 0x52, 0x6a, 0x00,
            0x51
        ]
    );
    // row +0x24 EFFECT -> 0x525d90
    assert_eq!(
        at(&image, 0x5a_ca01, 0x0a),
        [0x8b, 0x86, 0x5c, 0x01, 0x00, 0x00, 0x8b, 0x44, 0x90, 0x24]
    );
    assert_eq!(at(&image, 0x5a_ca1b, 5), [0xe8, 0x70, 0x93, 0xf7, 0xff]);
    // The picker: row +0x2c count, rand()*count>>15 index into row +0x30,
    // played through 0x593b80.
    assert_eq!(
        at(&image, ORIGINAL_WEAPON_IMPACT_SOUND_VA + 0x19, 4),
        [0x8b, 0x54, 0x06, 0x2c]
    );
    assert_eq!(
        at(&image, ORIGINAL_WEAPON_IMPACT_SOUND_VA + 0x2a, 4),
        [0x0f, 0xaf, 0x44, 0x33]
    );
    assert_eq!(
        at(&image, ORIGINAL_WEAPON_IMPACT_SOUND_VA + 0x48, 4),
        [0x8b, 0x4c, 0x8b, 0x30]
    );
    assert_eq!(
        at(&image, ORIGINAL_WEAPON_IMPACT_SOUND_VA + 0x4e, 5),
        [0xe8, 0x2d, 0x6a, 0xfe, 0xff]
    );
}

/// `DESTROY_ANIMATION` is consumed twice: a round ending in flight plays
/// `weapon + 0x188` (`0x5af78b`), and the detonation path selects it when
/// the surface row's `ANIMATION_MIN_RANGE` field is absent (`0x5b06b8`).
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_t452_r03_c_image_pins_the_destroy_animation_consumers() {
    let image = image();
    assert_eq!(
        at(&image, ORIGINAL_WEAPON_DESTROY_USE_VA, 6),
        [0x8b, 0x87, 0x88, 0x01, 0x00, 0x00]
    );
    assert_eq!(
        at(&image, 0x5b_06b8, 6),
        [0x8b, 0x87, 0x88, 0x01, 0x00, 0x00]
    );
    // The fallback chain reads the impact row's +0x20 first.
    assert_eq!(
        at(&image, 0x5b_06a4, 0x10),
        [
            0x8b, 0x8f, 0x5c, 0x01, 0x00, 0x00, 0x8d, 0x04, 0x80, 0x8d, 0x04, 0x80, 0x8b, 0x44,
            0x81, 0x20
        ]
    );
}

/// The round allocator attaches `FLYOUT>MODEL` (`weapon + 0x120`) to the
/// spawned round's node, building the animated form first when
/// `weapon + 0x110` (`MODEL_ANIMATION`) is nonzero.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_t452_r03_c_image_pins_the_flyout_model_attach() {
    let image = image();
    assert_eq!(
        at(&image, ORIGINAL_WEAPON_FLYOUT_MODEL_ATTACH_VA, 0x10),
        [
            0x8b, 0x87, 0x20, 0x01, 0x00, 0x00, 0x3b, 0xc3, 0x74, 0x2d, 0x39, 0x9f, 0x10, 0x01,
            0x00, 0x00
        ]
    );
}

/// `SOUND_NAME` exists in the image only as the tail of
/// `LOOPED_SOUND_NAME` (`0x62b380`) — no literal `SOUND_NAME` key is
/// parsed; the launch one-shot is `FIRE>SOUND`, the flying loop
/// `FLYOUT>SOUND`, the mount loop `LOOPED_SOUND_NAME`.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_t452_r03_c_image_has_no_sound_name_key() {
    let image = image();
    assert_eq!(cstr(&image, 0x62_b380, 18), b"LOOPED_SOUND_NAME\x00");
    // Every occurrence of the bytes "SOUND_NAME" in the image is inside
    // this string or a longer *_SOUND_NAME sibling; none is a standalone
    // "SOUND_NAME\0" key.
    let needle = b"SOUND_NAME\x00";
    let mut found = Vec::new();
    for (index, window) in image.windows(needle.len()).enumerate() {
        if window == needle {
            found.push(index);
        }
    }
    for index in found {
        // A standalone key would start a string (preceded by NUL); here every
        // hit sits inside a longer identifier (LOOPED_, ...).
        assert!(
            index > 0 && image[index - 1] != 0,
            "a standalone SOUND_NAME string at file offset {index:#x}"
        );
    }
    // The parse site for LOOPED_SOUND_NAME: push 0x62b380 in the mount
    // descriptor filler, resolved by 0x59cd40(0x13, name).
    assert_eq!(
        at(&image, ORIGINAL_MOUNT_LOOPED_SOUND_VA, 0x27),
        [
            0x68, 0x80, 0xb3, 0x62, 0x00, 0x53, 0xe8, 0x0a, 0xf6, 0x0b, 0x00, 0x83, 0xc4, 0x08,
            0x3b, 0xc7, 0x74, 0x1f, 0x8b, 0x40, 0x04, 0x8b, 0x48, 0x0c, 0x6a, 0x13, 0x51, 0xe8,
            0xa5, 0x22, 0x0e, 0x00, 0x83, 0xc4, 0x04, 0x50, 0x83, 0xc6, 0x24
        ]
    );
}

// ----------------------------------------------------------------- retail ---

fn game_dir() -> std::path::PathBuf {
    let dir = std::env::var_os("CS_GAME_DIR").expect(
        "CS_GAME_DIR is not set: this measurement needs the original installation \
         (capability `retail`)",
    );
    std::path::PathBuf::from(dir)
}

fn retail_media() -> Vec<cs_content::ordnance::OriginalWeaponMedia> {
    cs_content::ordnance::read_original_weapon_media(&game_dir()).expect("retail reads")
}

fn retail(name: &str) -> cs_content::ordnance::OriginalWeaponMedia {
    retail_media()
        .into_iter()
        .find(|media| media.record == name)
        .unwrap_or_else(|| panic!("{name} is a retail record"))
}

/// `wep_14`, the task's lead: every effect-class field measured.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_r03_c_retail_wep_14_declares_the_torpedo_media() {
    let media = retail("wep_14");
    assert_eq!(media.launch_sound(), Some("snd_missile_bg"));
    assert_eq!(media.flyout_model(), Some("a_torpedo"));
    assert_eq!(media.flyout_model_animation(), Some("torpedo_trail"));
    assert_eq!(media.flyout_sounds(), &["snd_torpedo_loop".to_owned()]);
    assert_eq!(
        media.destroy_animation.as_deref(),
        Some("torpedo_destroy_effect")
    );
    let surfaces: Vec<_> = media
        .impact_rows
        .iter()
        .map(|row| row.surface.as_str())
        .collect();
    assert_eq!(surfaces, ["default", "water", "player", "buildings"]);
    let water = media.impact_row("water").expect("the water row");
    assert_eq!(
        water.block.surface_animation.as_deref(),
        Some("torpedo_water_effect")
    );
    assert_eq!(water.block.sounds, ["snd_bsplash"]);
    let player = media.impact_row("player").expect("the player row");
    assert_eq!(player.block.animations, ["torpedo_ground_effect"]);
    assert_eq!(player.block.sounds, ["snd_torpedo_hit"]);
    assert_eq!(
        media
            .default_impact_row()
            .unwrap()
            .block
            .surface_animation
            .as_deref(),
        Some("torpedo_ground_effect")
    );
}

/// Every retail record's launch sound is its own declared `FIRE>SOUND`
/// entry — shared names like `snd_missile_sm` are shared *declarations*,
/// and a record without one is silent rather than defaulted.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_r03_c_retail_launch_sounds_are_declared() {
    let media = retail_media();
    assert_eq!(media.len(), 48);
    // The rocket family shares two declared names; the torpedo and the
    // heavy rockets declare the big one.
    let named = |record: &str| -> String {
        media
            .iter()
            .find(|row| row.record == record)
            .and_then(|row| row.launch_sound().map(str::to_owned))
            .unwrap_or_default()
    };
    for (record, sound) in [
        ("wep_04", "snd_missile_sm"),
        ("wep_05", "snd_missile_sm"),
        ("wep_06", "snd_missile_bg"),
        ("wep_11", "snd_missile_bg"),
        ("wep_12", "snd_missile_bg"),
        ("wep_13", "snd_gotshot2"),
        ("wep_14", "snd_missile_bg"),
        ("wep_24", "snd_missile_bg"),
        ("wep_25", "snd_missile_sm"),
        ("wep_27", "snd_enemyflak"),
        ("wep_28", "snd_zepcannonfire"),
    ] {
        assert_eq!(named(record), sound, "{record}");
    }
    // Retail declares at most one FIRE>SOUND per record: the launch slot is
    // index 0 or silence.
    for row in &media {
        assert!(row.launch_sounds().len() <= 1, "{}", row.record);
    }
    // Guns declare no FIRE>SOUND: their audio is the mount's
    // LOOPED_SOUND_NAME — wep_00 snd_30cal — which launch_sound does not
    // pick up; the two keys are not conflated.
    let gun = media.iter().find(|row| row.record == "wep_00").unwrap();
    assert_eq!(gun.launch_sound(), None);
    assert_eq!(gun.mount_looped_sound(), Some("snd_30cal"));
    // wep_26 declares neither: a completely silent launch.
    let silent = media.iter().find(|row| row.record == "wep_26").unwrap();
    assert_eq!(silent.launch_sound(), None);
    assert_eq!(silent.mount_looped_sound(), None);
    assert!(silent.fire.is_none() || silent.fire.as_ref().unwrap().is_empty());
    assert!(silent.impact_rows.is_empty());
    assert_eq!(silent.flyout_model(), Some("incendiary"));
}

/// The runtime-facing projection: every ordnance record's declared names
/// become kind-checked content ids — `FLYOUT>MODEL` under
/// `hardpoint_equipment`, `FIRE>SOUND` under `sound` — and a record with no
/// declared name keeps the field unknown rather than inventing one.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_r03_c_retail_media_ids_resolve_the_declared_names() {
    let claim = ClaimId::new("t452_r03_c.ordnance_media").expect("valid claim id");
    for media in retail_media() {
        let declared = media.measured_ordnance_media(&claim);
        match (media.flyout_model(), media.launch_sound()) {
            (Some(model), Some(sound)) => {
                let Resolved::Known(visual) = &declared.visual else {
                    panic!("{}: declared FLYOUT MODEL resolves", media.record)
                };
                assert_eq!(visual.value.kind(), ContentKind::HardpointEquipment);
                assert_eq!(
                    visual.value.as_str(),
                    format!("hardpoint_equipment/{model}"),
                    "{}",
                    media.record
                );
                assert_eq!(visual.provenance.class, ClaimStatus::ObservedTool);
                let Resolved::Known(audio) = &declared.sound else {
                    panic!("{}: declared FIRE SOUND resolves", media.record)
                };
                assert_eq!(audio.value.kind(), ContentKind::Sound);
                assert_eq!(
                    audio.value.as_str(),
                    format!("sound/{sound}"),
                    "{}",
                    media.record
                );
            }
            (model, sound) => {
                if model.is_none() {
                    assert!(!declared.visual.is_known(), "{}", media.record);
                }
                if sound.is_none() {
                    assert!(!declared.sound.is_known(), "{}", media.record);
                }
            }
        }
    }
    // The task's lead, end to end.
    let media = retail("wep_14");
    let declared = media.measured_ordnance_media(&claim);
    let Resolved::Known(visual) = &declared.visual else {
        panic!("wep_14 declares a torpedo model")
    };
    assert_eq!(visual.value.as_str(), "hardpoint_equipment/a_torpedo");
    let Resolved::Known(sound) = &declared.sound else {
        panic!("wep_14 declares a launch sound")
    };
    assert_eq!(sound.value.as_str(), "sound/snd_missile_bg");
}
