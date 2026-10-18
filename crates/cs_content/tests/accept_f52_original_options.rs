//! F52-ORIGINAL-OPTIONS (#789): the names of the original's stored options,
//! measured in the owner-supplied decrypted engine image.
//!
//! The finding `docs/findings/2026-10-18-f52-original-options.md` cites each
//! name by its image offset (virtual address = offset + `0x400000`). This
//! test re-reads those bytes through the production engine-image loader so
//! the table cannot drift from the image, and checks the one negative the
//! finding relies on: the config-name block holds no subtitle, field-of-view,
//! shake or flash name. It records names only. Ranges, defaults and which
//! name is shown on which options screen are not measured here.

use cs_content::coordinates::load_engine_image;

/// First occurrence of each name in the config block, `(file offset, name)`.
const OPTION_NAMES: &[(usize, &str)] = &[
    (0x2234e8, "DEFAULT"),
    (0x2234fc, "HWCardFlag"),
    (0x223508, "HWCardDev"),
    (0x223514, "EffectsLevel_SW"),
    (0x223534, "Shadow"),
    (0x223544, "Clutter"),
    (0x223554, "ViewingRange"),
    (0x223574, "EffectsLevel_HW"),
    (0x223594, "GfxFlags_SW"),
    (0x2235a0, "Transparency"),
    (0x2235b0, "Lighting"),
    (0x2235bc, "Perspective"),
    (0x2235c8, "GlobalLight_SW"),
    (0x2235d8, "AllVideoBuffer"),
    (0x2235e8, "GfxFlags_HW"),
    (0x22361c, "GlobalLight_HW"),
    (0x22363c, "ObjectLOD_SW"),
    (0x22365c, "ObjectLOD_HW"),
    (0x22367c, "TextureMemory_SW"),
    (0x2236a4, "TextureMemory_HW"),
    (0x2236cc, "GameIntensity"),
    (0x2236dc, "AllowIME"),
    (0x2236e8, "FlyMode"),
    (0x2236f0, "MouseSens"),
    (0x2236fc, "AskForVDev"),
    (0x223708, "LoadOptionalSounds"),
    (0x223730, "VoiceOn"),
    (0x223738, "VoiceVolume"),
    (0x223744, "MusicOn"),
    (0x223754, "MusicVolume"),
    (0x223760, "SfxOn"),
    (0x223768, "SfxVolume"),
    (0x223774, "SoundLOD"),
    (0x22378c, "PlayerName"),
    (0x223798, "UIAssetPath"),
    (0x2237a4, "GOSData"),
    (0x2237ac, "D3DFlag"),
    (0x2237b4, "FullScreen"),
    (0x2237c0, "HUDFlag"),
    (0x2237c8, "HWAPI"),
    (0x2237d0, "JoystickNumAxes"),
    (0x2237e0, "JoystickNumButtons"),
    (0x2237f4, "Network"),
    (0x2237fc, "NetListen"),
    (0x223808, "Ammo"),
    (0x223810, "Fuel"),
    (0x223818, "DefaultView"),
    (0x223824, "EnemySkill"),
    (0x223830, "PlayerSpeed"),
    (0x22383c, "MPFuel"),
    (0x223844, "Render"),
    (0x22384c, "Display"),
    (0x223854, "Window"),
    (0x22385c, "Replicate"),
    (0x223868, "VMode"),
    (0x223878, "InGameVMode"),
    (0x223890, "VStride"),
    (0x223898, "sunlight"),
];

const BLOCK_START: usize = 0x2234e8;
const BLOCK_END: usize = 0x223898;

/// The stored-options registry key and the single value name measured beside it.
const REGISTRY_KEY: (usize, &str) = (
    0x223a40,
    "Software\\Microsoft\\Microsoft Games\\Crimson Skies\\1.0",
);

fn c_string_at(image: &[u8], offset: usize) -> &str {
    let tail = &image[offset..];
    let end = tail
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(tail.len());
    std::str::from_utf8(&tail[..end]).expect("option names are ASCII")
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_f52_original_options_names_are_measured_in_the_engine_image() {
    let image = load_engine_image()
        .unwrap_or_else(|error| panic!("the engine image must load: {error}"))
        .bytes;

    for (offset, name) in OPTION_NAMES {
        assert_eq!(
            c_string_at(&image, *offset),
            *name,
            "the image holds {name:?} at offset {offset:#x}"
        );
    }
    assert_eq!(c_string_at(&image, REGISTRY_KEY.0), REGISTRY_KEY.1);
    assert_eq!(c_string_at(&image, 0x223878), "InGameVMode");
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_f52_original_options_block_names_no_subtitle_fov_shake_or_flash() {
    let image = load_engine_image()
        .unwrap_or_else(|error| panic!("the engine image must load: {error}"))
        .bytes;

    let block = &image[BLOCK_START..BLOCK_END];
    let text = String::from_utf8_lossy(block).to_ascii_lowercase();
    for absent in [
        "subtitle",
        "caption",
        "fov",
        "fieldofview",
        "shake",
        "flash",
        "scale",
    ] {
        assert!(
            !text.contains(absent),
            "the config-name block must not name {absent:?}; if it does the finding is wrong"
        );
    }
}
