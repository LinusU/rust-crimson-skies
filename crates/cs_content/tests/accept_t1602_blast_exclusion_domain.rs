//! Task #1602 against the owner-supplied engine image: the writers and the
//! domain of the object `+0xbc` key the ordnance blast's apply loop uses to
//! skip recipients (follow-up to #1318, claim `t1318.limit.exclusion_domain`).
//!
//! Task test prefix: `accept_t1602_`. Findings:
//! `docs/findings/2026-10-19-t1602-blast-exclusion-domain.md`.
//!
//! Static analysis only: no agent can run the original, and `retail` is read
//! access. Each window is a short byte pin of the decrypted executable
//! (`$CS_ENGINE_IMAGE`, base `0x400000`), not an extract.

/// The image base the executable's virtual addresses are relative to.
const IMAGE_BASE: u64 = 0x40_0000;

/// `(virtual address, bytes, what the window carries)`.
const IMAGE_PINS: [(u64, &[u8], &str); 19] = [
    (
        0x004c_c552,
        &[0xb9, 0x34, 0x00, 0x00, 0x00, 0x8b, 0xfe, 0xf3, 0xab],
        "node pool allocator: a fresh node is cleared with 0x34 dwords (0xd0 bytes), +0xbc included",
    ),
    (
        0x004c_c5aa,
        &[0x33, 0xc0, 0x66, 0x89],
        "node allocator: eax = 0 after the clear",
    ),
    (
        0x004c_c5e8,
        &[
            0x8d, 0x56, 0x74, 0x5f, 0x89, 0x86, 0xbc, 0x00, 0x00, 0x00, 0x89, 0x56, 0x70,
        ],
        "writer 1, node allocator: [node + 0xbc] = 0 (no callback record) on creation",
    ),
    (
        0x005a_bad0,
        &[
            0x55, 0x8b, 0x6c, 0x24, 0x10, 0x57, 0x8b, 0x7c, 0x24, 0x0c, 0x39, 0xaf, 0xbc, 0x00,
            0x00, 0x00,
        ],
        "subtree rewrite: only a node whose +0xbc equals the old key is rewritten",
    ),
    (
        0x005a_bb01,
        &[0xe8, 0xca, 0xff, 0xff, 0xff, 0x0f, 0xbf, 0x57, 0x56],
        "subtree rewrite: recurses over the child array (count +0x56, array +0x5c)",
    ),
    (
        0x005a_bb12,
        &[0x5e, 0x89, 0x9f, 0xbc, 0x00, 0x00, 0x00, 0x5b],
        "writer 2, subtree rewrite: [node + 0xbc] = the new key, after its children",
    ),
    (
        0x005a_bcbb,
        &[
            0x8b, 0x47, 0x5c, 0x8b, 0x0c, 0xb0, 0x53, 0x51, 0xe8, 0xd8, 0xff, 0xff,
        ],
        "subtree clear: recurses over the child array",
    ),
    (
        0x005a_bcd4,
        &[
            0x39, 0x9f, 0xbc, 0x00, 0x00, 0x00, 0x75, 0x0a, 0xc7, 0x87, 0xbc, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x5f, 0x5e, 0x5b, 0xc3,
        ],
        "writer 3, subtree clear: [node + 0xbc] = 0 only where it equals the key, after the children",
    ),
    (
        0x005a_ee28,
        &[
            0x8b, 0x46, 0x0c, 0x83, 0xc4, 0x30, 0xc7, 0x80, 0xbc, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x5e, 0xc3,
        ],
        "writer 4, projectile teardown: [projectile node + 0xbc] = 0",
    ),
    (
        0x005a_f6a6,
        &[
            0x8b, 0x47, 0x0c, 0xc7, 0x80, 0xbc, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x8b,
            0x4f, 0x0c,
        ],
        "writer 5, projectile spawn: [projectile node + 0xbc] = 1 (the projectile key)",
    ),
    (
        0x005a_bd27,
        &[
            0x8b, 0xb8, 0xbc, 0x00, 0x00, 0x00, 0x3b, 0xfb, 0x0f, 0x84, 0x37, 0x02, 0x00, 0x00,
            0x83, 0xff,
        ],
        "hit dispatcher: key = [node + 0xbc]; 0 returns without a callback",
    ),
    (
        0x005a_bd35,
        &[0x83, 0xff, 0x01],
        "hit dispatcher: key 1 (a projectile) takes an inline branch with no callback",
    ),
    (
        0x005a_be62,
        &[0x8b, 0x07, 0x53, 0x56, 0x55, 0x50, 0xff, 0x57, 0x04, 0x89],
        "hit dispatcher: any other key is a record; its callback [record + 4] is called with its context [record]",
    ),
    (
        0x004c_0613,
        &[0x68, 0xf0, 0x05, 0x4c, 0x00, 0x6a, 0x00, 0x50, 0x51, 0xe8],
        "registration site A: callback 0x4c05f0, slot 0",
    ),
    (
        0x004c_0857,
        &[0x68, 0x30, 0x08, 0x4c, 0x00, 0x6a, 0x01, 0x50, 0x51, 0xe8],
        "registration site B: callback 0x4c0830, slot 1",
    ),
    (
        0x004d_ef04,
        &[0x68, 0x20, 0xf4, 0x4d, 0x00, 0x6a, 0x01, 0x50, 0x6a, 0x00],
        "registration site C: callback 0x4df420, slot 1",
    ),
    (
        0x0052_32ac,
        &[0x68, 0x20, 0x72, 0x4e, 0x00, 0x83, 0xe0, 0x01, 0x50, 0x51],
        "registration site D: callback 0x4e7220, slot from bit 22 of [actor + 0x9c], node [actor + 0x6c]",
    ),
    (
        0x0052_346e,
        &[0x8b, 0x4e, 0x6c, 0x51, 0xe8, 0xd9, 0x87, 0x08, 0x00],
        "unregistration site: the actor's node [actor + 0x6c] released through 0x5abc50",
    ),
    (
        0x005a_cb0f,
        &[
            0x39, 0x82, 0xbc, 0x00, 0x00, 0x00, 0x0f, 0x84, 0x1c, 0x01, 0x00, 0x00,
        ],
        "reader (from #1318): the blast apply loop skips a recipient whose +0xbc equals the key",
    ),
];

/// Every writer of the node `+0xbc` field and the registry that produces its
/// non-trivial values are present in the image.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_t1602_image_pins_the_exclusion_key_writers() {
    let path = std::env::var_os("CS_ENGINE_IMAGE").expect(
        "CS_ENGINE_IMAGE is not set: this measurement needs the owner-supplied decrypted \
         executable",
    );
    let image = std::fs::read(&path).expect("the engine image reads");
    for (address, bytes, meaning) in IMAGE_PINS {
        let offset = usize::try_from(address - IMAGE_BASE).expect("offset");
        assert_eq!(
            &image[offset..offset + bytes.len()],
            bytes,
            "0x{address:x} no longer carries: {meaning}"
        );
    }
}
