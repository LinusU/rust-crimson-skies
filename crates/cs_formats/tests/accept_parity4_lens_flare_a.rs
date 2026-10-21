//! PARITY4-LENS-FLARE-A (#1858): what makes the sun a lens-flare source in M01
//! (finding `docs/findings/2026-10-22-parity4-lens-flare-a-sun-source-and-blend.md`).
//!
//! Two members, both needing the owner's data and both failing loudly without it:
//!
//! * with the installation named, the production GameZ readers show that the
//!   only mesh lights of `ZBD/C3/gamez.zbd` (M01's archive) that carry the
//!   lens-flare word are the one on the `sun` node's mesh and one on each of
//!   two `lens_flash` nodes, with the stored fade and flare numbers the
//!   finding states;
//! * with the engine image named, the instruction bytes the finding's claims
//!   rest on are re-read: the four projection loops, which of them hand the
//!   record queue a light object, the single caller of the flare draw and the
//!   mode-word gate in front of it.

use cs_formats::gamez::{read_gamez_meshes, read_gamez_nodes};
use cs_formats::io::ParseContext;
use std::path::Path;

fn game_dir() -> String {
    std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: this retail test needs the original installation; run it \
             with `--include-ignored`"
        )
    })
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity4_lens_flare_a_c3_gamez_sun_mesh_light_carries_the_flare_word() {
    let key = "ZBD/C3/gamez.zbd";
    let bytes = std::fs::read(Path::new(&game_dir()).join(key)).expect("the archive reads");
    let nodes = read_gamez_nodes(&mut ParseContext::with_defaults(key), &bytes)
        .expect("the node array reads");
    let meshes = read_gamez_meshes(&mut ParseContext::with_defaults(key), key, &bytes)
        .expect("the mesh section reads");

    let owners = |mesh: u32| -> Vec<&str> {
        nodes
            .nodes
            .iter()
            .filter(|node| node.mesh_index() == mesh as i32)
            .map(|node| node.name.as_str())
            .collect()
    };

    let mut total = 0;
    let mut flagged = Vec::new();
    for mesh in meshes.present() {
        total += mesh.lights.len();
        for light in &mesh.lights {
            if light.header.unk60 != 0 {
                flagged.push((mesh.index, light.clone(), mesh.info));
            }
        }
    }
    assert_eq!(total, 26, "mesh lights stored in the archive");
    let indices: Vec<u32> = flagged.iter().map(|(index, _, _)| *index).collect();
    assert_eq!(
        indices,
        [128, 130, 491],
        "meshes whose light carries the flare word"
    );

    for (index, light, info) in &flagged {
        let header = &light.header;
        assert_eq!(header.unk60, 1, "the flare word is a boolean");
        assert_eq!(header.extra_count, 1);
        assert_eq!(light.extra.len(), 1);
        assert_eq!((header.unk00, header.unk04), (0, 0), "point light, mode 0");
        assert_eq!(header.color, [255.0, 255.0, 255.0]);
        assert_eq!(
            (header.unk48, header.unk52),
            (0.0, 0.0),
            "no bulb fade stored"
        );
        assert_eq!(header.unk56, 0.17_f32);
        match *index {
            491 => {
                assert_eq!(owners(*index), ["sun"]);
                assert_eq!(
                    (header.unk64, header.unk68, header.unk72),
                    (5000.0, 5.0, 10000.0),
                    "sun flare: size word, fade min, fade max"
                );
                assert_eq!(header.flags, 0x3df);
                assert_eq!((info.vertex_count, info.polygon_count), (4, 1));
                assert_eq!(info.unk04, 1, "a geometry mesh, not a light-only mesh");
                let at = light.extra[0];
                assert_eq!(
                    [at[0], at[1], at[2].round()],
                    [6.18489, 18.294209, 0.0],
                    "the bulb sits on the quad, in the node's own space"
                );
            }
            _ => {
                assert_eq!(owners(*index), ["lens_flash"]);
                assert_eq!(
                    (header.unk64, header.unk68, header.unk72),
                    (30.0, 4000.0, 6000.0)
                );
                assert_eq!(header.flags, 0x211);
                assert_eq!((info.vertex_count, info.polygon_count), (0, 0));
                assert_eq!(info.unk04, 0, "a light-only mesh");
            }
        }
    }
}

/// The bytes of one virtual address range of the image (`.text`/`.rdata` map
/// at `va - 0x400000` in this image, which the guard below checks).
fn at(image: &[u8], va: usize, len: usize) -> &[u8] {
    &image[va - 0x40_0000..va - 0x40_0000 + len]
}

fn call_target(image: &[u8], site: usize) -> usize {
    let bytes = at(image, site, 5);
    assert_eq!(bytes[0], 0xe8, "a near call at {site:#x}");
    let rel = i32::from_le_bytes(bytes[1..5].try_into().expect("four bytes"));
    (site as i64 + 5 + i64::from(rel)) as usize
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_parity4_lens_flare_a_image_bytes_match_the_finding() {
    let path = std::env::var("CS_ENGINE_IMAGE").unwrap_or_else(|_| {
        panic!("CS_ENGINE_IMAGE is not set: this test needs the owner-supplied engine image")
    });
    let image = std::fs::read(path).expect("the engine image reads");
    assert_eq!(&image[..2], b"MZ");
    // `.text` starts at file offset 0x1000 for VA 0x401000: the push of the
    // function prologue of the record filler is the first guard.
    assert_eq!(
        at(&image, 0x57c230, 5),
        [0xb9, 0x3c, 0xe8, 0xa0, 0x00],
        "mov ecx, 0xa0e83c"
    );

    // The four projection loops all call the record filler 0x57c230.
    for site in [0x55_2815, 0x55_39e8, 0x55_8d41, 0x55_8f65] {
        assert_eq!(call_target(&image, site), 0x57_c230, "site {site:#x}");
    }
    // Sites 1 and 2 hand it a null light object (`push 0` before the call's
    // other pushes); sites 3 and 4 hand it `entry + 0x30` (`add edx, 0x30`).
    assert_eq!(at(&image, 0x55_280a, 2), [0x6a, 0x00]);
    assert_eq!(at(&image, 0x55_39dd, 2), [0x6a, 0x00]);
    assert_eq!(at(&image, 0x55_8d37, 3), [0x83, 0xc2, 0x30]);
    assert_eq!(at(&image, 0x55_8f53, 3), [0x83, 0xc2, 0x30]);

    // The queue builder takes only records with a light object whose word at
    // +0xc is non-zero (`mov edi, [eax+0xc]; test edi, edi`).
    assert_eq!(at(&image, 0x57_c445, 5), [0x8b, 0x78, 0x0c, 0x85, 0xff]);
    // The lens flag the sun's stored light sets is entry +0x3c; the sprite
    // loop tests it before filling a record (`mov eax, [ebx+0x3c]`).
    assert_eq!(at(&image, 0x55_8cb7, 3), [0x8b, 0x43, 0x3c]);

    // The flare draw `0x4d6a10` has exactly two callers, and the one in the
    // frame function sits behind the mode word 0x9be708 (`je` over it).
    let mut callers = Vec::new();
    for offset in 0..image.len().saturating_sub(5).min(0x20_14bd) {
        let va = 0x40_1000 + offset;
        if image[0x1000 + offset] == 0xe8 {
            let rel =
                i32::from_le_bytes(image[0x1001 + offset..0x1005 + offset].try_into().unwrap());
            if (va as i64 + 5 + i64::from(rel)) as usize == 0x4d_6a10 {
                callers.push(va);
            }
        }
    }
    assert_eq!(callers, [0x42_e708, 0x49_efb9]);
    assert_eq!(
        at(&image, 0x42_e6d0, 6),
        [0xa1, 0x08, 0xe7, 0x9b, 0x00, 0x85],
        "mov eax, [0x9be708]; test"
    );
    // The flare queue draw is reached from one place: the loop in 0x4d6a10.
    assert_eq!(call_target(&image, 0x4d_6a94), 0x57_cc00);

    // The sprite routine's fourth argument: `push 0x3f000000` (0.5) at the
    // first quad, and the opaque/translucent split in the device draw
    // compares the third argument with 1.0 (`fcomp [0x6032dc]`).
    assert_eq!(at(&image, 0x57_caff, 5), [0x68, 0x00, 0x00, 0x00, 0x3f]);
    assert_eq!(
        at(&image, 0x5a_45b0, 6),
        [0xd8, 0x1d, 0xdc, 0x32, 0x60, 0x00]
    );
    let float_at = |va: usize| f32::from_le_bytes(at(&image, va, 4).try_into().unwrap());
    assert_eq!(float_at(0x60_32dc), 1.0);
    assert_eq!(float_at(0x60_32e0), 0.5);
    assert_eq!(float_at(0x60_414c), 255.0);
}
