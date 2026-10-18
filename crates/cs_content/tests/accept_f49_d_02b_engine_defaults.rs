//! F49-D.02b (#1573): what `ZBD/c2b/IA1`'s scenario uses for the player's
//! airframe and the wingman count when its `ia.zrd` declares neither.
//!
//! The executable's scenario setup (`0x459390`) overwrites `player_plane` and
//! `num_wingmen` only when the key is present; the record it writes into is
//! reset by the initialiser `0x458ff0` first. The retail test pins the data
//! side (the key really is absent, through the production survey) and
//! re-reads the initialiser's and the reader's instruction bytes from the
//! owner-supplied decrypted image, independently of the constants the
//! production code carries.

use std::fs;
use std::path::PathBuf;

use super::accept_f49_d_01_ia_denominator::{add_directory, campaign_tree, ia_document};
use cs_content::instant_action::{
    IA_ENGINE_DEFAULT_PLAYER_PLANE_INDEX, IA_ENGINE_DEFAULT_WINGMEN, IaPlayerPlane, IaWingmen,
    survey_retail_instant_action,
};

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

#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_f49_d_02b_retail_c2b_runs_on_the_executables_record_defaults() {
    let game_dir = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));
    let survey = survey_retail_instant_action(&game_dir).expect("the survey reads");

    // Data side: exactly one directory leaves the keys out, and it is c2b.
    let mut absent = Vec::new();
    for directory in survey.directories() {
        let plane_absent = directory.aircraft().player_plane().is_none();
        let wingmen_absent = directory.aircraft().wingmen().is_none();
        if plane_absent || wingmen_absent {
            absent.push(directory.container().to_owned());
        }
        if directory.world_group() == "c2b" {
            assert!(plane_absent && wingmen_absent);
            assert_eq!(
                directory.player_plane(),
                IaPlayerPlane::EngineDefault { index: 5 }
            );
            assert_eq!(directory.wingmen(), IaWingmen::EngineDefault(0));
            assert_eq!(directory.wingmen().count(), 0);
        } else {
            assert!(matches!(
                directory.player_plane(),
                IaPlayerPlane::Declared(_)
            ));
            assert_eq!(directory.wingmen(), IaWingmen::Declared(3));
        }
    }
    assert_eq!(absent, ["zbd/c2b/ia1/zrdr.zbd"]);

    // Engine side, from the image: the initialiser writes 5 and 0 ...
    let image_path = std::env::var("CS_ENGINE_IMAGE").expect("CS_ENGINE_IMAGE must be set");
    let image = fs::read(&image_path).expect("the engine image reads");
    assert_eq!(
        image_bytes(&image, 0x0045_8ffa, 24),
        [
            0xc7, 0x45, 0x00, 0, 0, 0, 0, // mov [ebp], 0          mission_type
            0xbe, 0x05, 0, 0, 0, //          mov esi, 5
            0x33, 0xc0, //                   xor eax, eax
            0x89, 0x75, 0x04, //             mov [ebp+4], esi       player_plane
            0xc7, 0x45, 0x08, 0, 0, 0, 0, // mov [ebp+8], 0         num_wingmen
        ],
        "the record initialiser"
    );
    assert_eq!(IA_ENGINE_DEFAULT_PLAYER_PLANE_INDEX, 5);
    assert_eq!(IA_ENGINE_DEFAULT_WINGMEN, 0);
    // ... and the reader skips the store when the key lookup finds nothing.
    assert_eq!(
        image_bytes(&image, 0x0045_93f3, 4),
        [0x85, 0xc0, 0x74, 0x1a],
        "player_plane: test eax, eax; je past the store"
    );
    assert_eq!(
        image_bytes(&image, 0x0045_941f, 4),
        [0x85, 0xc0, 0x74, 0x1c],
        "num_wingmen: test eax, eax; je past the store"
    );
    // The initialiser runs right before each `ia.zrd` read: `0x45a150` calls
    // `0x45a120` (which loops `0x459e90` and then calls `0x458ff0`).
    assert_eq!(
        image_bytes(&image, 0x0045_a152, 5),
        [0xe8, 0xc9, 0xff, 0xff, 0xff]
    );
    assert_eq!(
        image_bytes(&image, 0x0045_a13d, 5),
        [0xe8, 0xae, 0xee, 0xff, 0xff]
    );
}

#[test]
fn accept_f49_d_02b_synthetic_absent_keys_fall_back_and_present_keys_win() {
    let temp = campaign_tree("d02b");
    add_directory(
        &temp,
        "C2B",
        "IA1",
        ia_document(&[("zeppelin_run", 4)], None, "zeppelin_run", None),
    );
    add_directory(
        &temp,
        "C1",
        "IA1",
        ia_document(&[("zeppelin_run", 4)], None, "zeppelin_run", Some("Fury")),
    );
    let survey = survey_retail_instant_action(&temp.0).expect("the survey reads");
    let by_group = |group: &str| {
        survey
            .directories()
            .iter()
            .find(|directory| directory.world_group() == group)
            .expect("the directory is surveyed")
    };
    let absent = by_group("c2b");
    assert_eq!(
        absent.player_plane(),
        IaPlayerPlane::EngineDefault {
            index: IA_ENGINE_DEFAULT_PLAYER_PLANE_INDEX
        }
    );
    assert_eq!(
        absent.wingmen(),
        IaWingmen::EngineDefault(IA_ENGINE_DEFAULT_WINGMEN)
    );
    let declared = by_group("c1");
    assert_eq!(
        declared.player_plane(),
        IaPlayerPlane::Declared("Fury".into())
    );
    assert_eq!(declared.wingmen(), IaWingmen::Declared(3));
}
