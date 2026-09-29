//! Acceptance contracts F15-A: the canonical-to-Bevy conversion boundary.
//!
//! These tests exercise production code only — `cs_app::assets`'s
//! [`CanonicalPayload`], [`CanonicalAsset`] and [`ConvertedAsset`], plus a
//! real [`LoadTransaction`] to stamp the load identity. If
//! `ConvertedAsset` stopped recording which cache key it was produced
//! under, `verify_fresh` could never refuse a stale derivation and these
//! tests fail.

mod common;

use cs_app::assets::{CanonicalAsset, CanonicalPayload, ConversionError, ConvertedAsset};
use cs_app::loading::{Criticality, LoadItem, LoadRequest, LoadTarget, LoadTransaction};
use cs_assets::cache::{CacheKey, ConversionOptions, ConverterVersion, DecoderId, IrVersion};
use cs_assets::install::sha256;
use cs_assets::vfs::SessionBuilder;
use cs_types::content::ContentKind;

use common::{fixed_hash, synthetic_content, synthetic_context, synthetic_key, synthetic_span};

fn install() -> cs_types::evidence::ContentHash {
    fixed_hash(0x55)
}

fn key(options: ConversionOptions) -> CacheKey {
    CacheKey::new(
        install(),
        &[synthetic_span(
            install(),
            "zbd/c1/texture.zbd",
            "hull.bmp",
            0,
            fixed_hash(0xA1),
        )],
        ConverterVersion {
            decoder: DecoderId::new("zbd-texture").expect("valid decoder id"),
            decoder_version: 3,
            ir: IrVersion(1),
        },
        options,
    )
    .expect("valid key")
}

/// The payload records the production SHA-256 of its own bytes, and the
/// converted product is stamped with the cache key it was produced under:
/// a consumer asking for a different derivation is refused.
#[test]
fn accept_f15_a_converted_asset_tracks_the_cache_key_it_was_built_under() {
    let key_rgba = key(ConversionOptions::from_pairs(&[("format", "rgba8")]).unwrap());
    let key_rgb = key(ConversionOptions::from_pairs(&[("format", "rgb8")]).unwrap());
    assert_ne!(key_rgba.digest(), key_rgb.digest());

    let payload = CanonicalPayload::new(ContentKind::Image, b"fixture pixels".to_vec());
    assert_eq!(payload.sha256(), sha256(payload.bytes()));

    let input = CanonicalAsset {
        content: synthetic_content(ContentKind::Image, "c1.hull"),
        cache_key: key_rgba.clone(),
        payload,
    };
    assert_eq!(input.payload.kind(), ContentKind::Image);

    // The product is stamped with the producing load and key.
    let session = SessionBuilder::new(synthetic_context(install(), "zbd/c1")).open();
    let load = LoadTransaction::issue(LoadRequest {
        session: session.generation(),
        target: LoadTarget::shared(),
        items: vec![],
    });
    let converted: ConvertedAsset<u32> = ConvertedAsset::new(
        input.content.clone(),
        load.identity(),
        &input.cache_key,
        0xBEEF,
    );
    assert_eq!(converted.content(), &input.content);
    assert_eq!(converted.load(), load.identity());
    assert_eq!(converted.produced_under(), key_rgba.digest());

    // Fresh for the derivation it was built under; stale the moment the
    // option set (or any key facet) moves.
    converted
        .verify_fresh(&key_rgba)
        .expect("fresh under its own key");
    assert!(matches!(
        converted.verify_fresh(&key_rgb),
        Err(ConversionError::StaleCacheKey { .. })
    ));
}

/// A load item can name the cache key of its derived form, tying the
/// transaction record to the entry it will produce or consume.
#[test]
fn accept_f15_a_load_item_binds_its_derived_cache_key() {
    let derived = key(ConversionOptions::none());
    let item = LoadItem::new(
        synthetic_key("world", "texture/hull.bmp"),
        synthetic_content(ContentKind::Image, "c1.hull"),
        Criticality::GameplayCritical,
        64,
    )
    .expect("nonzero units")
    .with_derived(derived.clone());
    assert_eq!(
        item.derived.as_ref().map(CacheKey::digest),
        Some(derived.digest())
    );
}
