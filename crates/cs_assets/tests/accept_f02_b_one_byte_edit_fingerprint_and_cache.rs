//! The F02-B minimum acceptance scenario (spec F02 AC02): a one-byte edit
//! changes the fingerprint and invalidates cache entries.
//!
//! These tests exercise production code only — `cs_assets::install::
//! discover`, `discover_with_cache`, `fingerprint`, `content_fingerprint`
//! and `AnalysisCache` over `cs_types::install` records. Removing or
//! neutering that implementation (a fingerprint that ignores the digest
//! column, a cache lookup that ignores the fingerprint or the per-file
//! digest, discovery that does not hash the real bytes) makes them fail.
//! The fixture trees are newly authored bytes under the system temporary
//! directory; the original installation is never written to.

mod common;

use common::TempTree;
use cs_assets::install::{
    AnalysisCache, CacheError, CachedAnalysis, Discovery, discover, discover_with_cache,
    fingerprint,
};
use cs_types::install::{FileFamily, FileRole, ParseState};

/// Authored fixture payloads, distinct per file so no two rows share a
/// digest.
const PLANES_PAYLOAD: &[u8] = b"authored fixture planes payload";
const ROF_PAYLOAD: &[u8] = b"authored fixture rof payload";
const README_PAYLOAD: &[u8] = b"authored fixture readme payload";

const PLANES: &str = "ZBD/planes.zbd";
const ROF: &str = "GOSDATA/ASSETS/crimson.rof";
const README: &str = "Readme.rtf";

const PLANES_KEY: &str = "zbd/planes.zbd";
const ROF_KEY: &str = "gosdata/assets/crimson.rof";

/// The one fixture row used for the cache, by logical key.
fn row<'a>(discovery: &'a Discovery, key: &str) -> &'a cs_types::install::InstallFileRecord {
    discovery
        .manifest
        .files
        .iter()
        .find(|row| row.relative_spelling.logical_key() == key)
        .expect("the fixture row is inventoried")
}

/// `(logical key, size, digest)` per row, for comparing everything except
/// the data columns between two runs.
fn shape(discovery: &Discovery) -> Vec<(String, u64, cs_types::evidence::ContentHash)> {
    let mut rows: Vec<(String, u64, cs_types::evidence::ContentHash)> = discovery
        .manifest
        .files
        .iter()
        .map(|row| {
            (
                row.relative_spelling.logical_key(),
                row.size_bytes,
                row.sha256,
            )
        })
        .collect();
    rows.sort_by(|left, right| {
        (left.0.as_str(), left.1).cmp(&(right.0.as_str(), right.1))
    });
    rows
}

#[test]
fn accept_f02_b_one_byte_edit_changes_fingerprint_and_invalidates_cache() {
    let tree = TempTree::new("ac02");
    tree.write(PLANES, PLANES_PAYLOAD);
    tree.write(ROF, ROF_PAYLOAD);
    tree.write(README, README_PAYLOAD);

    // Discovery hashes the real bytes; fresh rows carry the explicit
    // unknown analysis (this stage never guesses a family or a role).
    let before = discover(tree.root()).expect("the fixture tree discovers");
    assert_eq!(before.cached_rows, 0, "a plain discovery applies no cache");
    assert_eq!(
        before.diagnosis.file_count, 3,
        "every fixture file inventories"
    );
    for file in &before.manifest.files {
        assert!(
            matches!(file.role, FileRole::Unknown),
            "fresh rows are unclassified, never assumed unused"
        );
        assert!(file.family.is_none(), "no family is detected yet");
        assert!(
            matches!(file.parse_state, ParseState::Unparsed),
            "fresh rows are unparsed"
        );
    }
    let first_fingerprint = fingerprint(&before.manifest);
    let first_digest = row(&before, PLANES_KEY).sha256;
    let untouched_digest = row(&before, ROF_KEY).sha256;

    // Record analysis for exactly one row of exactly this installation
    // state, then watch it be reused.
    let mut cache = AnalysisCache::for_manifest(&before.manifest);
    assert!(cache.is_empty(), "the fresh cache holds no entries");
    assert_eq!(
        cache.fingerprint(),
        first_fingerprint,
        "the cache binds to the manifest's fingerprint"
    );
    cache
        .record(
            &before.manifest,
            PLANES_KEY,
            CachedAnalysis {
                family: Some(FileFamily::new("synthetic-fixture").expect("label is valid")),
                role: FileRole::Consumed,
                parse_state: ParseState::Parsed,
            },
        )
        .expect("analysis records against the manifest it was measured on");
    assert!(
        cache
            .entry(&first_fingerprint, PLANES_KEY, first_digest)
            .is_some(),
        "positive control: a matching entry under the matching fingerprint is reusable"
    );

    let with_cache = discover_with_cache(tree.root(), &cache).expect("cached discovery succeeds");
    assert_eq!(
        with_cache.cached_rows, 1,
        "exactly the recorded row takes its analysis from the cache"
    );
    assert!(
        matches!(row(&with_cache, PLANES_KEY).role, FileRole::Consumed),
        "the cached analysis reaches the row"
    );
    assert!(
        matches!(row(&with_cache, PLANES_KEY).parse_state, ParseState::Parsed),
        "the cached parse state reaches the row"
    );
    assert!(
        row(&with_cache, ROF_KEY).family.is_none(),
        "unrecorded rows keep the unknown analysis"
    );
    assert_eq!(
        fingerprint(&with_cache.manifest),
        first_fingerprint,
        "applying analysis must not change the fingerprint: analysis is not data"
    );

    // --- The acceptance scenario: one byte changes, file length does not. ---
    tree.edit_byte(PLANES, 0, 0x01);
    let after = discover(tree.root()).expect("re-discovery succeeds");
    let second_fingerprint = fingerprint(&after.manifest);
    assert_ne!(
        second_fingerprint, first_fingerprint,
        "a one-byte edit must change the installation fingerprint"
    );
    assert_ne!(
        row(&after, PLANES_KEY).sha256,
        first_digest,
        "the edited file's content digest changed"
    );
    assert_eq!(
        row(&after, ROF_KEY).sha256,
        untouched_digest,
        "unrelated rows keep their digests"
    );
    let before_shape: Vec<(String, u64)> = shape(&before)
        .into_iter()
        .map(|(key, size, _)| (key, size))
        .collect();
    let after_shape: Vec<(String, u64)> = shape(&after)
        .into_iter()
        .map(|(key, size, _)| (key, size))
        .collect();
    assert_eq!(
        before_shape, after_shape,
        "keys and sizes are unchanged, so only the digest column can have \
         driven the fingerprint change"
    );

    // ... and with it, every cache entry is invalidated.
    assert!(
        !cache.is_valid_for(&second_fingerprint),
        "the cache must be invalid for the new fingerprint"
    );
    assert!(
        cache
            .entry(
                &second_fingerprint,
                PLANES_KEY,
                row(&after, PLANES_KEY).sha256
            )
            .is_none(),
        "an entry recorded against the old fingerprint is not reusable"
    );
    assert!(
        cache
            .entry(&second_fingerprint, PLANES_KEY, first_digest)
            .is_none(),
        "neither is one checked against the stale digest"
    );
    let stale = discover_with_cache(tree.root(), &cache)
        .expect("discovery with a stale cache still succeeds");
    assert_eq!(
        stale.cached_rows, 0,
        "stale entries must not be reused after the edit"
    );
    assert!(
        matches!(row(&stale, PLANES_KEY).role, FileRole::Unknown),
        "the invalidated row falls back to the explicit unknown analysis"
    );

    // Recording against the changed installation is refused by name, so a
    // stale cache can never absorb post-edit data silently.
    let mut refused = cache.clone();
    let refused_result = refused.record(
        &after.manifest,
        PLANES_KEY,
        CachedAnalysis {
            family: None,
            role: FileRole::Unknown,
            parse_state: ParseState::Unparsed,
        },
    );
    assert!(
        matches!(&refused_result, Err(CacheError::FingerprintMismatch { .. })),
        "recording against a changed manifest must be refused, got {refused_result:?}"
    );
}

/// The cache lookup is two-sided: even while the installation fingerprint
/// still matches, an entry whose recorded per-file digest disagrees with
/// the row it is asked about is not reusable. And an entry under the wrong
/// key is not either.
#[test]
fn accept_f02_b_cache_entries_match_key_and_digest() {
    let tree = TempTree::new("ac02-entries");
    tree.write(PLANES, PLANES_PAYLOAD);
    tree.write(ROF, ROF_PAYLOAD);

    let manifest = discover(tree.root())
        .expect("the fixture tree discovers")
        .manifest;
    let current = fingerprint(&manifest);
    let planes_digest = {
        let found = manifest
            .files
            .iter()
            .find(|row| row.relative_spelling.logical_key() == PLANES_KEY)
            .expect("planes inventories");
        found.sha256
    };
    let other_digest = {
        let found = manifest
            .files
            .iter()
            .find(|row| row.relative_spelling.logical_key() == ROF_KEY)
            .expect("rof inventories");
        found.sha256
    };

    let mut cache = AnalysisCache::for_manifest(&manifest);
    cache
        .record(
            &manifest,
            PLANES_KEY,
            CachedAnalysis {
                family: None,
                role: FileRole::OptionalMedia,
                parse_state: ParseState::Unparsed,
            },
        )
        .expect("analysis records");

    assert!(
        cache.entry(&current, PLANES_KEY, planes_digest).is_some(),
        "key and digest both match"
    );
    assert!(
        cache.entry(&current, PLANES_KEY, other_digest).is_none(),
        "a digest the entry was not recorded against must not match"
    );
    assert!(
        cache.entry(&current, ROF_KEY, planes_digest).is_none(),
        "an entry recorded under another key must not match"
    );
    assert!(
        cache
            .entry(&other_digest, PLANES_KEY, planes_digest)
            .is_none(),
        "a foreign fingerprint must not unlock the entry"
    );

    // An unknown logical key is refused when recording.
    assert!(
        matches!(
            cache.record(
                &manifest,
                "zbd/does-not-exist.zbd",
                CachedAnalysis {
                    family: None,
                    role: FileRole::Unknown,
                    parse_state: ParseState::Unparsed,
                },
            ),
            Err(CacheError::UnknownKey { .. })
        ),
        "recording against a row that does not exist must be refused by name"
    );
}
