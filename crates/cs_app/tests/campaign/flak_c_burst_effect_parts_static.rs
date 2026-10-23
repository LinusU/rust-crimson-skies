//! FLAK-C-BURST-EFFECT-PARTS (#1879): the static trace of what starts the
//! `flak_effect` at a flak burst, the rocket range end and the turret `AMMO`
//! key (finding `docs/findings/2026-10-23-flak-c-burst-effect-parts-static.md`).
//!
//! The finding commits no table rows; this file holds the cited sites. The
//! first test checks the citations are well formed under the production
//! loader's address arithmetic; the ignored one re-reads every site from the
//! owner's image and the facts derived from them.

use cs_app::engine_facts::engine_facts;
use cs_content::coordinates::{load_engine_image, original_image_digest};

/// One cited instruction sequence: its VA, its bytes and the claim it carries.
struct Site {
    va: u64,
    bytes: &'static [u8],
    what: &'static str,
}

const SITES: &[Site] = &[
    // 0x5ac7a0: the arguments' stack offsets.
    Site {
        va: 0x5ac7a0,
        bytes: &[0x83, 0xec, 0x2c, 0x53, 0x55],
        what: "impact: sub esp,0x2c; push ebx; push ebp",
    },
    Site {
        va: 0x5ac7b0,
        bytes: &[0x56, 0x57],
        what: "impact: push esi; push edi",
    },
    Site {
        va: 0x5ac7c3,
        bytes: &[0x8b, 0x74, 0x24, 0x40],
        what: "impact: esi = [esp+0x40] (weapon)",
    },
    Site {
        va: 0x5ac7cf,
        bytes: &[0x8b, 0x7c, 0x24, 0x48],
        what: "impact: edi = [esp+0x48] (rocket)",
    },
    // The hit handler call and its result slot.
    Site {
        va: 0x5ac80f,
        bytes: &[0xe8, 0xdc, 0xf4, 0xff, 0xff, 0x89, 0x44, 0x24, 0x5c],
        what: "impact: call 0x5abcf0; mov [esp+0x5c], eax",
    },
    Site {
        va: 0x5ac81b,
        bytes: &[0x83, 0xc4, 0x18],
        what: "impact: add esp,0x18 after six pushed words",
    },
    Site {
        va: 0x5ac933,
        bytes: &[0xf6, 0x44, 0x24, 0x10, 0x04, 0x75, 0x4d],
        what: "impact: hook flag 4 skips the animation block",
    },
    Site {
        va: 0x5ac93a,
        bytes: &[0x8b, 0x44, 0x24, 0x44, 0x85, 0xc0, 0x75, 0x45],
        what: "impact: guard tests [esp+0x44]",
    },
    Site {
        va: 0x5ac942,
        bytes: &[
            0x8b, 0x96, 0x5c, 0x01, 0x00, 0x00, 0x8d, 0x04, 0x9b, 0x8d, 0x0c, 0x80, 0x8b, 0x44,
            0x8a, 0x04, 0x85, 0xc0, 0x74, 0x31, 0x8b, 0x4c, 0x24, 0x4c, 0x85, 0xc9, 0x74, 0x02,
            0x8b, 0xc1, 0x85, 0xc0, 0x74, 0x23,
        ],
        what: "impact: row ANIMATION handle [rows + row*100 + 4], override [esp+0x4c]",
    },
    Site {
        va: 0x5ac97f,
        bytes: &[0xe8, 0x8c, 0x12, 0xf4, 0xff],
        what: "impact: call 0x4edc10 (animation start)",
    },
    Site {
        va: 0x5ac996,
        bytes: &[0x8b, 0x44, 0x17, 0x1c, 0x85, 0xc0, 0x74, 0x5d],
        what: "impact: row+0x1c (SURFACE_ANIMATION) handle gate",
    },
    Site {
        va: 0x5aca07,
        bytes: &[0x8b, 0x44, 0x90, 0x24, 0x85, 0xc0, 0x74, 0x14],
        what: "impact: row+0x24 (EFFECT) gate",
    },
    Site {
        va: 0x5aca1b,
        bytes: &[0xe8, 0x70, 0x93, 0xf7, 0xff],
        what: "impact: call 0x525d90 (effect at the position)",
    },
    // The crater and effect gates.
    Site {
        va: 0x5ac820,
        bytes: &[
            0x74, 0x2e, 0xf6, 0x44, 0x24, 0x10, 0x02, 0x75, 0x27, 0xa9, 0x00, 0x00, 0x80, 0x00,
            0x75, 0x08, 0xc7, 0x44, 0x24, 0x14, 0x01, 0x00, 0x00, 0x00,
        ],
        what: "impact: CRATER gate, skip flag [esp+0x14] = 1 unless weapon flag 0x800000",
    },
    Site {
        va: 0x5ac850,
        bytes: &[
            0xa9, 0x00, 0x00, 0x04, 0x00, 0x74, 0x33, 0xf6, 0x44, 0x24, 0x10, 0x02, 0x75, 0x2c,
        ],
        what: "impact: weapon flag 0x40000 gate in front of 0x5ac580",
    },
    Site {
        va: 0x5ac58a,
        bytes: &[0x0f, 0x85, 0xf1, 0x00, 0x00, 0x00],
        what: "0x5ac580: non-zero third argument returns at once",
    },
    Site {
        va: 0x5ac597,
        bytes: &[0xf7, 0x40, 0x24, 0x00, 0x00, 0x01, 0x00],
        what: "0x5ac580: node mask 0x10000 test",
    },
    Site {
        va: 0x5ac69e,
        bytes: &[0x33, 0xc0, 0xf7, 0xc2, 0x00, 0x00, 0x01, 0x00],
        what: "0x5ac690: result 0 unless node mask 0x10000",
    },
    Site {
        va: 0x5ac78f,
        bytes: &[0xf7, 0xd8, 0x1b, 0xc0, 0x40],
        what: "0x5ac690: result 1 only when 0x4e4a10 returned 0",
    },
    // The hit handler.
    Site {
        va: 0x5abd1c,
        bytes: &[
            0x8b, 0x46, 0x24, 0x3b, 0xc3, 0x0f, 0x84, 0x45, 0x02, 0x00, 0x00, 0x8b, 0xb8, 0xbc,
            0x00, 0x00, 0x00, 0x3b, 0xfb, 0x0f, 0x84, 0x37, 0x02, 0x00, 0x00,
        ],
        what: "0x5abcf0: no node or no handler at node+0xbc returns",
    },
    Site {
        va: 0x5abd38,
        bytes: &[0x89, 0x5c, 0x24, 0x20],
        what: "0x5abcf0: result local starts 0",
    },
    Site {
        va: 0x5abe68,
        bytes: &[0xff, 0x57, 0x04, 0x89, 0x44, 0x24, 0x30],
        what: "0x5abcf0: result = call [handler+4]",
    },
    Site {
        va: 0x5abf63,
        bytes: &[0x8b, 0x44, 0x24, 0x20],
        what: "0x5abcf0: returns the local",
    },
    Site {
        va: 0x5abf6c,
        bytes: &[0x5f, 0x5e, 0x5d, 0x33, 0xc0, 0x5b, 0xc3],
        what: "0x5abcf0: early return 0",
    },
    Site {
        va: 0x5abf07,
        bytes: &[
            0xf6, 0x45, 0x74, 0x02, 0x74, 0x44, 0xa1, 0xdc, 0xd7, 0xa1, 0x00, 0x85, 0xc0, 0x74,
            0x3b,
        ],
        what: "0x5abcf0: weapon flag 2 and [0xa1d7dc] gate of the handler-side row animation",
    },
    // The only write of node+0xbc in the launch code.
    Site {
        va: 0x5af664,
        bytes: &[0x8b, 0x46, 0x74, 0xa8, 0x08, 0x74, 0x62],
        what: "launch: weapon flag 8 branch",
    },
    Site {
        va: 0x5af6a9,
        bytes: &[0xc7, 0x80, 0xbc, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00],
        what: "launch: mov [node+0xbc], 1",
    },
    // The range end.
    Site {
        va: 0x5b01e6,
        bytes: &[
            0xd9, 0x86, 0x64, 0x06, 0x00, 0x00, 0xd8, 0x5f, 0x1c, 0xdf, 0xe0, 0xf6, 0xc4, 0x01,
            0x8b, 0x47, 0x74, 0x75, 0x1d, 0xf6, 0xc4, 0x80, 0x74, 0x08, 0xa8, 0x80, 0x0f, 0x84,
            0xb4, 0x00, 0x00, 0x00, 0xa9, 0x00, 0x00, 0x00, 0x02, 0x0f, 0x84, 0x07, 0x01, 0x00,
            0x00,
        ],
        what: "range end: travelled vs RANGE; LOCK_ON 0x8000, EXPIRES 0x80, DETONATE_AT_RANGE 0x2000000",
    },
    Site {
        va: 0x5b030e,
        bytes: &[
            0x56, 0x57, 0xe8, 0x8b, 0xc0, 0xff, 0xff, 0x83, 0xc4, 0x08, 0xc7, 0x46, 0x20, 0x00,
            0x00, 0x00, 0x00,
        ],
        what: "range end: burst call 0x5ac3a0 falls into [rocket+0x20] = 0",
    },
    // AMMO.
    Site {
        va: 0x629c98,
        bytes: b"AMMO\0",
        what: "the AMMO key string",
    },
    Site {
        va: 0x4aa2de,
        bytes: &[0x68, 0x98, 0x9c, 0x62, 0x00],
        what: "turret parser: push \"AMMO\"",
    },
    Site {
        va: 0x4aa2f6,
        bytes: &[0x89, 0x8b, 0x64, 0x01, 0x00, 0x00],
        what: "turret parser: mov [entry+0x164], ecx",
    },
    Site {
        va: 0x4a9bed,
        bytes: &[0xc7, 0x86, 0x64, 0x01, 0x00, 0x00, 0x32, 0x00, 0x00, 0x00],
        what: "turret constructor: AMMO defaults to 50",
    },
    Site {
        va: 0x4ac693,
        bytes: &[0x8b, 0x86, 0x64, 0x01, 0x00, 0x00],
        what: "save copy: read [entry+0x164]",
    },
    Site {
        va: 0x4ac6f7,
        bytes: &[0x89, 0x91, 0x64, 0x01, 0x00, 0x00],
        what: "save copy: write [entry+0x164]",
    },
    Site {
        va: 0x441830,
        bytes: &[0x6a, 0xff, 0x68, 0x40, 0xe1, 0x5f, 0x00],
        what: "0x441830 prologue",
    },
];

/// The disassembly's displacement bytes for `+0x164`.
const AMMO_DISPLACEMENT: [u8; 4] = [0x64, 0x01, 0x00, 0x00];

fn site(va: u64) -> &'static Site {
    SITES
        .iter()
        .find(|site| site.va == va)
        .unwrap_or_else(|| panic!("no cited site at {va:#x}"))
}

/// The target of the `call rel32` that starts the site's bytes at `from`.
fn call_target(site: &Site, from: usize) -> u64 {
    assert_eq!(site.bytes[from], 0xe8, "{}: not a call", site.what);
    let rel = i32::from_le_bytes(site.bytes[from + 1..from + 5].try_into().expect("rel32"));
    (site.va + from as u64 + 5).wrapping_add_signed(i64::from(rel))
}

#[test]
fn accept_flak_c_burst_effect_parts_citations_are_well_formed() {
    let facts = engine_facts().expect("the committed table parses");
    let mut vas: Vec<u64> = SITES.iter().map(|site| site.va).collect();
    vas.sort_unstable();
    vas.dedup();
    assert_eq!(vas.len(), SITES.len(), "cited VAs are unique");
    for site in SITES {
        assert!(!site.bytes.is_empty(), "{} cites no bytes", site.what);
        let offset = facts
            .image_offset(site.va)
            .unwrap_or_else(|error| panic!("{}: {error}", site.what));
        assert!(offset < 0x30_0000, "{} is past the image", site.what);
    }

    // The guard reads the slot the handler result was stored into: the store
    // displacement minus the six pushed words is the guard's displacement.
    let store = site(0x5ac80f).bytes[8];
    let cleanup = site(0x5ac81b).bytes[2];
    let guard = site(0x5ac93a).bytes[3];
    assert_eq!(store - cleanup, guard, "result slot == guard slot");
    // That slot is the second argument: frame (0x2c) + four pushes + the
    // return address + one argument word before it.
    let frame = site(0x5ac7a0).bytes[2];
    assert_eq!(guard, frame + 4 * 4 + 4 + 4, "the guard slot is argument 2");
    // The weapon and rocket are arguments 1 and 3 of the same frame.
    assert_eq!(site(0x5ac7c3).bytes[3], guard - 4);
    assert_eq!(site(0x5ac7cf).bytes[3], guard + 4);

    // The calls of the burst path.
    assert_eq!(call_target(site(0x5ac80f), 0), 0x5abcf0);
    assert_eq!(call_target(site(0x5ac97f), 0), 0x4edc10);
    assert_eq!(call_target(site(0x5aca1b), 0), 0x525d90);
    assert_eq!(call_target(site(0x5b030e), 2), 0x5ac3a0);

    // The range-end masks.
    let range_end = site(0x5b01e6).bytes;
    assert_eq!(
        &range_end[19..22],
        [0xf6, 0xc4, 0x80],
        "LOCK_ON 0x8000 (ah)"
    );
    assert_eq!(&range_end[24..26], [0xa8, 0x80], "EXPIRES 0x80 (al)");
    assert_eq!(&range_end[32..37], [0xa9, 0, 0, 0, 0x02], "0x2000000");

    // The one pointer-sized reference to AMMO is the parser's push.
    assert_eq!(&site(0x4aa2de).bytes[1..], &0x629c98u32.to_le_bytes());
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_flak_c_burst_effect_parts_cited_sites_match_the_image() {
    let loaded = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    assert_eq!(loaded.digest, original_image_digest());
    let facts = engine_facts().expect("the committed table parses");
    let image = loaded.bytes.as_slice();
    let at = |va: u64| -> usize {
        usize::try_from(facts.image_offset(va).expect("va inside the image")).expect("an offset")
    };

    for site in SITES {
        let offset = at(site.va);
        assert_eq!(
            &image[offset..offset + site.bytes.len()],
            site.bytes,
            "{} at {:#x}",
            site.what,
            site.va
        );
    }

    // The AMMO key string is referenced exactly once, by the parser's push.
    let pointer = 0x629c98u32.to_le_bytes();
    let references: Vec<usize> = image
        .windows(4)
        .enumerate()
        .filter(|(_, window)| *window == pointer)
        .map(|(offset, _)| offset)
        .collect();
    assert_eq!(
        references,
        [at(0x4aa2de) + 1],
        "the AMMO string's address appears only in the parser's push"
    );

    // Neither launch routine carries the AMMO field's displacement.
    for (name, start, end) in [
        ("0x5aef40 launch", 0x5aef40, 0x5af715),
        ("0x441830 post-launch", 0x441830, 0x441a1a),
    ] {
        let body = &image[at(start)..at(end)];
        assert!(
            !body.windows(4).any(|window| window == AMMO_DISPLACEMENT),
            "{name} contains the +0x164 displacement"
        );
    }

    // Inside the turret module the displacement appears only in the sites
    // the finding lists: instruction displacements at the parser store, the
    // constructor default and the save copy.
    let module = &image[at(0x4a9000)..at(0x4ad000)];
    let expected = [0x4a9bed + 2, 0x4aa2f6 + 2, 0x4ac693 + 2, 0x4ac6f7 + 2];
    for (offset, window) in module.windows(4).enumerate() {
        if window == AMMO_DISPLACEMENT {
            let va = 0x4a9000 + offset;
            assert!(
                expected.contains(&va),
                "unlisted +0x164 displacement bytes at {va:#x}"
            );
        }
    }
}
