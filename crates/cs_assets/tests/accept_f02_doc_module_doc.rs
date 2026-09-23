//! Acceptance scenario F02-DOC: the `cs_assets` crate-level module doc must
//! describe the F02-B discovery and hashing path as shipped production
//! code, not as work that still "arrives".
//!
//! The doc itself is this task's deliverable, so the test embeds the
//! shipped `src/lib.rs` (`include_str!`) and asserts on it. Reverting the
//! doc fix — restoring the stale "arrive with F02-B" wording or deleting
//! the discovery/hashing description — fails the test.

/// The shipped crate root, embedded at compile time so the test reads the
/// real doc comment rather than a copy.
const LIB_RS: &str = include_str!("../src/lib.rs");

/// The module doc describes walking a real installation and hashing bytes
/// as present capability and names its shipped entry points.
#[test]
fn accept_f02_doc_module_doc_describes_shipped_discovery_and_hashing() {
    assert!(
        !LIB_RS.contains("arrive with F02-B"),
        "walking a real installation and hashing bytes shipped with F02-B; \
         the module doc must not describe them as future work"
    );
    for needle in ["walk", "hash"] {
        assert!(
            LIB_RS.contains(needle),
            "the module doc must describe the F02-B path that walks a real \
             installation and hashes bytes (missing {needle:?})"
        );
    }
    for symbol in ["discover", "fingerprint", "AnalysisCache"] {
        assert!(
            LIB_RS.contains(symbol),
            "the module doc must name the shipped `{symbol}` entry point"
        );
    }
}
