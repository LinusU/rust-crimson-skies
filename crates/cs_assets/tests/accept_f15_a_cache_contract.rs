//! Acceptance contracts F15-A: the cache-key identity, the stored-entry
//! integrity gate and the private/bounded location contract.
//!
//! These tests exercise production code only — `cs_assets::cache`. If the
//! digest stopped covering one of its facets (say conversion options were
//! dropped from the encoding), or if `verify_entry` served an uncommitted
//! or corrupted entry, or if `CacheDirectory` allowed a root inside the
//! installation, these tests fail.

mod common;

use cs_assets::cache::{
    BudgetError, BudgetExceeded, CacheBudget, CacheDirectory, CacheKey, CacheLocationError,
    ConversionOptions, ConverterVersion, DecoderId, EntryHeader, IntegrityError, IrVersion,
    SourceSpanHash, verify_entry,
};
use cs_assets::install::sha256;
use cs_types::evidence::ContentHash;

use common::{TempTree, fixed_hash, synthetic_span};

fn install() -> ContentHash {
    fixed_hash(0x11)
}

fn converter() -> ConverterVersion {
    ConverterVersion {
        decoder: DecoderId::new("zbd-texture").expect("valid decoder id"),
        decoder_version: 3,
        ir: IrVersion(1),
    }
}

fn span_a() -> cs_types::asset_id::SourceSpan {
    synthetic_span(
        install(),
        "zbd/c1/texture.zbd",
        "hull_a.bmp",
        0,
        fixed_hash(0xA1),
    )
}

fn span_b() -> cs_types::asset_id::SourceSpan {
    synthetic_span(
        install(),
        "zbd/c1/texture.zbd",
        "mask_b.bmp",
        4096,
        fixed_hash(0xB2),
    )
}

/// The key digest is deterministic and covers every facet: installation,
/// inputs, decoder id and version, IR version and options.
#[test]
fn accept_f15_a_cache_key_digest_covers_every_identity_facet() {
    let base = CacheKey::new(
        install(),
        &[span_a()],
        converter(),
        ConversionOptions::none(),
    )
    .expect("valid key");
    // Same construction, same digest — warm/cold determinism.
    let same = CacheKey::new(
        install(),
        &[span_a()],
        converter(),
        ConversionOptions::none(),
    )
    .expect("valid key");
    assert_eq!(base, same);
    assert_eq!(base.digest(), same.digest());

    let cases: Vec<(&str, CacheKey)> = vec![
        (
            "a different installation",
            CacheKey::new(
                fixed_hash(0x22),
                &[span_a()],
                converter(),
                ConversionOptions::none(),
            )
            .unwrap(),
        ),
        (
            "a different source span",
            CacheKey::new(
                install(),
                &[span_b()],
                converter(),
                ConversionOptions::none(),
            )
            .unwrap(),
        ),
        (
            "a different decoder",
            CacheKey::new(
                install(),
                &[span_a()],
                ConverterVersion {
                    decoder: DecoderId::new("gamez-mesh").unwrap(),
                    ..converter()
                },
                ConversionOptions::none(),
            )
            .unwrap(),
        ),
        (
            "a different decoder version",
            CacheKey::new(
                install(),
                &[span_a()],
                ConverterVersion {
                    decoder_version: 4,
                    ..converter()
                },
                ConversionOptions::none(),
            )
            .unwrap(),
        ),
        (
            "a different IR version",
            CacheKey::new(
                install(),
                &[span_a()],
                ConverterVersion {
                    ir: IrVersion(2),
                    ..converter()
                },
                ConversionOptions::none(),
            )
            .unwrap(),
        ),
        (
            "different conversion options",
            CacheKey::new(
                install(),
                &[span_a()],
                converter(),
                ConversionOptions::from_pairs(&[("mipmaps", "full")]).unwrap(),
            )
            .unwrap(),
        ),
    ];
    for (what, key) in &cases {
        assert_ne!(
            base.digest(),
            key.digest(),
            "the digest must change when {what} changes"
        );
    }
}

/// Inputs are sorted and deduplicated: traversal order is not part of the
/// identity, and listing one source twice does not fork the key.
#[test]
fn accept_f15_a_cache_key_inputs_are_canonical() {
    let forward = CacheKey::new(
        install(),
        &[span_a(), span_b()],
        converter(),
        ConversionOptions::none(),
    )
    .unwrap();
    let reversed = CacheKey::new(
        install(),
        &[span_b(), span_a()],
        converter(),
        ConversionOptions::none(),
    )
    .unwrap();
    assert_eq!(forward.digest(), reversed.digest());
    let duplicated = CacheKey::new(
        install(),
        &[span_a(), span_a()],
        converter(),
        ConversionOptions::none(),
    )
    .unwrap();
    assert_eq!(duplicated.inputs().len(), 1);

    // A key with no inputs is refused: nothing derived from nothing can be
    // invalidated precisely.
    assert!(matches!(
        CacheKey::new(install(), &[], converter(), ConversionOptions::none()),
        Err(cs_assets::cache::CacheKeyError::NoInputs)
    ));
}

/// Spec F15 AC03's granularity at contract level: editing one source span
/// changes only the keys that list that span.
#[test]
fn accept_f15_a_changing_one_source_invalidates_only_dependent_entries() {
    let texture = CacheKey::new(
        install(),
        &[span_a()],
        converter(),
        ConversionOptions::none(),
    )
    .unwrap();
    let livery = CacheKey::new(
        install(),
        &[span_a(), span_b()],
        converter(),
        ConversionOptions::from_pairs(&[("faction", "brighton")]).unwrap(),
    )
    .unwrap();

    // The livery's second source is edited (new bytes -> new member digest).
    let edited_b = synthetic_span(
        install(),
        "zbd/c1/texture.zbd",
        "mask_b.bmp",
        4096,
        fixed_hash(0xB9),
    );
    assert_ne!(SourceSpanHash::of(&span_b()), SourceSpanHash::of(&edited_b));
    assert!(livery.depends_on(SourceSpanHash::of(&span_b())));
    assert!(!texture.depends_on(SourceSpanHash::of(&span_b())));

    let livery_rebuilt = CacheKey::new(
        install(),
        &[span_a(), edited_b],
        converter(),
        ConversionOptions::from_pairs(&[("faction", "brighton")]).unwrap(),
    )
    .unwrap();
    let texture_rebuilt = CacheKey::new(
        install(),
        &[span_a()],
        converter(),
        ConversionOptions::none(),
    )
    .unwrap();

    assert_ne!(
        livery.digest(),
        livery_rebuilt.digest(),
        "the derived livery must be invalidated by its changed input"
    );
    assert_eq!(
        texture.digest(),
        texture_rebuilt.digest(),
        "an entry that does not list the changed source keeps its key"
    );
}

/// Spec F15 non-negotiable behavior 3: a partially written or corrupted
/// entry fails integrity validation and is rebuilt — never served.
#[test]
fn accept_f15_a_entry_integrity_gate_refuses_partial_and_corrupt() {
    let key = CacheKey::new(
        install(),
        &[span_a()],
        converter(),
        ConversionOptions::none(),
    )
    .unwrap();
    let payload = b"derived bytes of the fixture".as_slice();

    // A write in progress is a partial entry by contract, whatever bytes
    // sit behind it.
    let writing = EntryHeader::writing(key.clone());
    assert!(matches!(
        verify_entry(&key, &writing, payload),
        Err(IntegrityError::Uncommitted)
    ));

    // A committed entry verifies and yields the payload only through the
    // verified wrapper.
    let committed = EntryHeader::committed(key.clone(), payload);
    let verified = verify_entry(&key, &committed, payload).expect("a committed entry verifies");
    assert_eq!(verified.payload(), payload);
    assert_eq!(verified.key(), &key);

    // Truncated payload.
    assert!(matches!(
        verify_entry(&key, &committed, &payload[..payload.len() - 4]),
        Err(IntegrityError::LengthMismatch { .. })
    ));

    // Corrupted payload.
    let mut corrupt = payload.to_vec();
    corrupt[0] ^= 0xFF;
    assert!(matches!(
        verify_entry(&key, &committed, &corrupt),
        Err(IntegrityError::DigestMismatch { .. })
    ));

    // An entry stored under a different derivation is not this key's
    // answer.
    let other = CacheKey::new(
        install(),
        &[span_b()],
        converter(),
        ConversionOptions::none(),
    )
    .unwrap();
    let foreign = EntryHeader::committed(other, payload);
    assert!(matches!(
        verify_entry(&key, &foreign, payload),
        Err(IntegrityError::WrongKey { .. })
    ));
}

/// Non-negotiable behavior 1's private-location half: a cache root inside
/// the source installation is refused; a bound of zero is not a bound.
#[test]
fn accept_f15_a_cache_location_is_private_and_budget_is_bounded() {
    let tree = TempTree::new("f15-a-location");
    let install_root = tree.root().join("install");
    let inside = install_root.join("cache");
    let outside = tree.root().join("cache");
    for dir in [&install_root, &inside, &outside] {
        std::fs::create_dir_all(dir).expect("fixture directories are created");
    }

    assert!(matches!(
        CacheDirectory::open(&inside, &install_root),
        Err(CacheLocationError::InsideInstall { .. })
    ));
    // The installation directory itself is inside it.
    assert!(matches!(
        CacheDirectory::open(&install_root, &install_root),
        Err(CacheLocationError::InsideInstall { .. })
    ));
    let directory = CacheDirectory::open(&outside, &install_root).expect("a private root opens");
    assert_eq!(
        directory.root(),
        std::fs::canonicalize(&outside).unwrap().as_path()
    );

    assert_eq!(
        CacheBudget::new(0, 1024),
        Err(BudgetError::Zero {
            field: "max_entries"
        })
    );
    assert_eq!(
        CacheBudget::new(8, 0),
        Err(BudgetError::Zero { field: "max_bytes" })
    );
    let budget = CacheBudget::new(2, 100).expect("a nonzero budget");
    budget.check(2, 100).expect("the budget admits its bound");
    assert_eq!(
        budget.check(3, 50),
        Err(BudgetExceeded::Entries {
            limit: 2,
            requested: 3
        })
    );
    assert_eq!(
        budget.check(1, 101),
        Err(BudgetExceeded::Bytes {
            limit: 100,
            requested: 101
        })
    );
}

/// A committed entry's digest is the production SHA-256 of its payload, so
/// verification cannot be satisfied by a header that merely claims a
/// length.
#[test]
fn accept_f15_a_committed_header_records_real_payload_digest() {
    let key = CacheKey::new(
        install(),
        &[span_a()],
        converter(),
        ConversionOptions::none(),
    )
    .unwrap();
    let payload = b"warm bytes";
    let header = EntryHeader::committed(key.clone(), payload);
    let cs_assets::cache::EntryState::Committed {
        payload_len,
        payload_sha256,
    } = header.state()
    else {
        panic!("a committed header is committed");
    };
    assert_eq!(payload_len, payload.len() as u64);
    assert_eq!(payload_sha256, sha256(payload));
}
