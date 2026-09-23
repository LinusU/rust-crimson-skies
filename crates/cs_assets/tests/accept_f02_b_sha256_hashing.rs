//! F02-B hashing: the production SHA-256 is the published FIPS 180-4
//! algorithm, checked against the NIST example vectors, and its streaming
//! path agrees with its one-shot path.
//!
//! These tests exercise `cs_assets::install::Sha256`/`sha256` — the code
//! discovery hashes every installation file with. A broken compression
//! function, padding rule or block buffer makes them fail.

use cs_assets::install::{Sha256, sha256};

/// NIST/FIPS 180-4 example digests, stored in this project's canonical
/// lowercase hexadecimal form (IDENTITY-CONTENT: "Hash strings must be
/// canonical lowercase hexadecimal").
const EMPTY_MESSAGE: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const THREE_BYTE_MESSAGE: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const FIFTY_SIX_BYTE_MESSAGE: &str =
    "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1";
const MILLION_A_MESSAGE: &str = "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0";

/// The 56-byte NIST input: its padding spills into a second block.
const FIFTY_SIX_BYTE_INPUT: &str = "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";

#[test]
fn accept_f02_b_sha256_matches_published_vectors() {
    assert_eq!(
        sha256(b"").to_hex(),
        EMPTY_MESSAGE,
        "the empty-message vector covers the padding of a zero-length input"
    );
    assert_eq!(
        sha256(b"abc").to_hex(),
        THREE_BYTE_MESSAGE,
        "the FIPS 180-4 `abc` vector"
    );
    assert_eq!(
        sha256(FIFTY_SIX_BYTE_INPUT.as_bytes()).to_hex(),
        FIFTY_SIX_BYTE_MESSAGE,
        "the 56-byte vector forces the length suffix into a second block"
    );
    let million = vec![b'a'; 1_000_000];
    assert_eq!(
        sha256(&million).to_hex(),
        MILLION_A_MESSAGE,
        "the 1,000,000-`a` vector exercises long multi-block streaming"
    );
}

#[test]
fn accept_f02_b_sha256_streaming_matches_one_shot() {
    // 4096 bytes in an awkward residue pattern: every update below splits
    // the message at different offsets across 64-byte block boundaries.
    let data: Vec<u8> = (0..4096u32).map(|index| (index % 251) as u8).collect();
    let one_shot = sha256(&data);
    for chunk in [1usize, 3, 63, 64, 65, 127, 1000, 4096] {
        let mut hasher = Sha256::new();
        for piece in data.chunks(chunk) {
            hasher.update(piece);
        }
        assert_eq!(
            hasher.finalize(),
            one_shot,
            "streaming in chunks of {chunk} bytes must hash identically to one shot"
        );
    }
}
