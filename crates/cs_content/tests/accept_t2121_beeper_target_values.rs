//! Task #2121 against the owner's engine image: what the two values the
//! `BEEPER_SEEKER` callback `0x441780` stores into projectile `+0x70`/`+0x74`
//! are, and how the steering routine `0x5af960` (through `0x538ca0`) consumes
//! them.
//!
//! Task test prefix: `accept_t2121_`. Findings:
//! `docs/findings/2026-10-25-t2121-beeper-target-values.md`.
//!
//! The pins are byte windows of the owner-supplied decrypted executable
//! (`$CS_ENGINE_IMAGE`, base `0x400000`): static-analysis evidence, never
//! original-run behaviour, and no code is printed or committed beyond the byte
//! strings below.

const IMAGE_BASE: u64 = 0x40_0000;

/// `(virtual address, bytes, what the window carries)`.
const IMAGE_PINS: [(u64, &[u8], &str); 13] = [
    (
        0x441786,
        &[
            0x8d, 0x46, 0x3c, 0x50, 0x8d, 0x4e, 0x48, 0x51, 0x8b, 0x0d, 0xc0, 0xdc, 0x71, 0x00,
            0xe8, 0xb7, 0x73, 0x07, 0x00,
        ],
        "search call: callback 0x441780 calls the beeper search 0x4b8b50 with (&projectile+0x48, &projectile+0x3c) and this=[0x71dcc0]",
    ),
    (
        0x4417a1,
        &[
            0x8b, 0x0f, 0x8b, 0x11, 0xff, 0x12, 0x89, 0x46, 0x70, 0x8b, 0x0f, 0x8b, 0x01, 0xff,
            0x50, 0x04, 0x5f, 0x89, 0x46, 0x74, 0x5e, 0xc3, 0x5f, 0x89, 0x46, 0x70, 0x89, 0x46,
            0x74, 0x5e,
        ],
        "callback: found beeper object -> [esi+0x70] = its virtual slot 0 value, [esi+0x74] = slot 1 value; none found -> both zero",
    ),
    (
        0x538ca0,
        &[
            0x55, 0x8b, 0xec, 0x53, 0x8b, 0x5d, 0x0c, 0x8b, 0x4d, 0x08, 0x8b, 0x55, 0x10, 0xd9,
            0x03, 0xd8, 0x21, 0xd9, 0x43, 0x04, 0xd8, 0x61, 0x04, 0xd9, 0x43, 0x08, 0xd8, 0x61,
            0x08, 0xd9, 0xca, 0xd9, 0x1a, 0xd9, 0x5a, 0x04, 0xd9, 0x5a, 0x08, 0x8b, 0x45, 0x10,
            0x50, 0xe8, 0xc0, 0x99, 0xee, 0xff, 0x83, 0xc4, 0x04, 0x5b, 0x5d, 0xc3,
        ],
        "0x538ca0(from, to, out): out = *to - *from, then normalized by 0x422690",
    ),
    (
        0x5af96e,
        &[
            0x8b, 0x4e, 0x70, 0x57, 0x8d, 0x45, 0xf4, 0x50, 0x51, 0x8d, 0x5e, 0x48, 0x53, 0xe8,
            0x20, 0x93, 0xf8, 0xff,
        ],
        "steering 0x5af960: 0x538ca0(&projectile+0x48, [projectile+0x70], &local)",
    ),
    (
        0x5af9aa,
        &[
            0x8b, 0x46, 0x74, 0x85, 0xc0, 0x74, 0x7a, 0x8b, 0x8e, 0x40, 0x06, 0x00, 0x00, 0x8d,
            0x55, 0xe0, 0x52, 0x50, 0x8b, 0x46, 0x70, 0x50, 0x51, 0x53, 0xe8, 0xa6, 0xeb, 0xf8,
            0xff,
        ],
        "steering: with [+0x74] non-zero calls the lead solver 0x53e56d(&projectile+0x48, [+0x640], [+0x70], [+0x74], &out)",
    ),
    (
        0x53e57d,
        &[
            0x8b, 0x4d, 0x10, 0x8b, 0x55, 0xfc, 0xd9, 0x03, 0xd8, 0x21, 0xd9, 0x43, 0x04, 0xd8,
            0x61, 0x04, 0xd9, 0x43, 0x08, 0xd8, 0x61, 0x08, 0xd9, 0xca, 0xd9, 0x1a, 0xd9, 0x5a,
            0x04, 0xd9, 0x5a, 0x08,
        ],
        "lead solver 0x53e56d: relative = arg1 - arg3 (position minus target position)",
    ),
    (
        0x53e5dc,
        &[
            0x8b, 0x4d, 0x14, 0x8b, 0x55, 0xfc, 0xd9, 0x01, 0xd8, 0x0a, 0xd9, 0x41, 0x04, 0xd8,
            0x4a, 0x04, 0xd9, 0x41, 0x08, 0xd8, 0x4a, 0x08, 0xd9, 0xc9, 0xde, 0xc2, 0xde, 0xc1,
            0xd9, 0x5d, 0x10,
        ],
        "lead solver: dot of arg4 (the second target value) with the relative vector",
    ),
    (
        0x53e6dc,
        &[
            0x8b, 0x5d, 0x14, 0x8b, 0x4d, 0x0c, 0x8b, 0x55, 0x18, 0xd9, 0x03, 0xd8, 0x21, 0xd9,
            0x43, 0x04, 0xd8, 0x61, 0x04, 0xd9, 0x43, 0x08, 0xd8, 0x61, 0x08, 0xd9, 0xca, 0xd9,
            0x1a, 0xd9, 0x5a, 0x04, 0xd9, 0x5a, 0x08, 0x8b, 0x55, 0x18, 0x52, 0xe8, 0x88, 0x3f,
            0xee, 0xff, 0xdd, 0xd8, 0x83, 0xc4, 0x04,
        ],
        "lead solver: out = normalize(arg4 - relative * k)",
    ),
    (
        0x4b8b88,
        &[0x8b, 0x0e, 0x8b, 0x11, 0xff, 0x12],
        "beeper search 0x4b8b50: a candidate's tracked object ([entry+0]) is asked for its slot 0 value, which is then passed to 0x538880 and subtracted from the query point as a pointer to three floats",
    ),
    (
        0x4b8b9b,
        &[0x8b, 0x0e, 0x8b, 0x11, 0x83, 0xc4, 0x08, 0xff, 0x12],
        "... and asked for it again, the result treated as the pointer to the position (fsub [ecx], [ecx+4], [ecx+8])",
    ),
    (
        0x4b8bb6,
        &[
            0xd9, 0x03, 0xd8, 0x21, 0xd9, 0x43, 0x04, 0xd8, 0x61, 0x04, 0xd9, 0x43, 0x08, 0xd8,
            0x61, 0x08,
        ],
        "... the query point minus [slot 0 value][0..3]: slot 0 is a pointer to a position",
    ),
    (
        0x4a36e0,
        &[0x8d, 0x41, 0x74, 0xc3],
        "vtable 0x608848 slot 0: lea eax, [ecx+0x74]; ret",
    ),
    (
        0x4a36f0,
        &[0x8d, 0x81, 0x80, 0x00, 0x00, 0x00, 0xc3],
        "vtable 0x608848 slot 1: lea eax, [ecx+0x80]; ret",
    ),
];

/// The three classes whose vtable slots 0 and 1 are `lea eax, [ecx+N]; ret`
/// (the only such vtables in the image): `(vtable, slot 0 function, slot 0
/// offset, slot 1 function, slot 1 offset)`.
const VTABLES: [(u64, u64, u32, u64, u32); 3] = [
    (0x60_8848, 0x4a_36e0, 0x74, 0x4a_36f0, 0x80),
    (0x60_8b48, 0x4a_cbb0, 0x220, 0x4a_cbc0, 0x22c),
    (0x60_8b88, 0x4b_4420, 0x240, 0x4b_4430, 0x924),
];

fn image() -> Vec<u8> {
    let path = std::env::var_os("CS_ENGINE_IMAGE").expect(
        "CS_ENGINE_IMAGE is not set: this measurement needs the owner-supplied decrypted \
         executable",
    );
    std::fs::read(&path).expect("the engine image reads")
}

fn at(image: &[u8], address: u64, len: usize) -> &[u8] {
    let offset = usize::try_from(address - IMAGE_BASE).expect("offset");
    &image[offset..offset + len]
}

fn word(image: &[u8], address: u64) -> u32 {
    let bytes = at(image, address, 4);
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_t2121_image_pins_the_beeper_target_values_and_their_consumers() {
    let image = image();
    // The first pin list entry carries the call to the search; the pin table
    // is exhaustive for the windows the finding cites.
    for (address, bytes, meaning) in IMAGE_PINS {
        assert_eq!(
            at(&image, address, bytes.len()),
            bytes,
            "0x{address:x} no longer carries: {meaning}"
        );
    }

    // The three classes that expose a position and a second vector through
    // virtual slots 0 and 1: each slot is `lea eax, [ecx + N]; ret`.
    for (vtable, slot0, offset0, slot1, offset1) in VTABLES {
        assert_eq!(
            u64::from(word(&image, vtable)),
            slot0,
            "vtable 0x{vtable:x}"
        );
        assert_eq!(
            u64::from(word(&image, vtable + 4)),
            slot1,
            "vtable 0x{vtable:x}"
        );
        for (function, offset) in [(slot0, offset0), (slot1, offset1)] {
            let code = at(&image, function, 7);
            assert_eq!(code[0], 0x8d, "0x{function:x} is not an lea");
            let (displacement, length) = match code[1] {
                0x41 => (u32::from(code[2]), 3),
                0x81 => (word(&image, function + 2), 6),
                other => panic!("0x{function:x}: unexpected modrm 0x{other:02x}"),
            };
            assert_eq!(
                code[1] & 0x38,
                0,
                "0x{function:x} does not load eax from [ecx+N]"
            );
            assert_eq!(displacement, offset, "0x{function:x}");
            assert_eq!(code[length], 0xc3, "0x{function:x} does not return");
        }
    }
}
