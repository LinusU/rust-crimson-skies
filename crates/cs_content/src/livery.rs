//! Paint composition on a content session: the three mask colors of a BM
//! livery, the deterministic key of a composed variant and the composed
//! image itself (`specs/F09-bm-multilayer-liveries-and-paint-composition.md`,
//! stage `### F09-B`; contract `docs/contracts/IDENTITY-CONTENT.md`).
//!
//! Stage F09-A read the BM planes ([`cs_formats::bm`]); F09-B composes them
//! there. This module is the content-layer production path over that
//! composition:
//!
//! * [`LiveryPaint`] names the three paint colors applied through the mask
//!   planes. The colors are caller input: faction palettes and decals come
//!   from original data (F09-D), and the Blender helper's colors are research
//!   leads, so none is baked in here (spec non-negotiable #4).
//! * [`source_fingerprint`] fingerprints the whole parsed source: the
//!   dimensions, every stored plane — the RGB base, the three masks and the
//!   RGBA overlay, which is the observed decal/overlay layer — and any
//!   unsupported tail. Two files that differ only in a mask or the overlay
//!   therefore fingerprint differently.
//! * [`LiveryVariantKey`] is the cache key the spec requires
//!   (non-negotiable #5): the source fingerprint, all three colors and the
//!   composition algorithm version ([`BM_COMPOSITION_VERSION`]) are its
//!   inputs. Switching colors yields a new key and new bytes; an existing
//!   composition is never mutated in place.
//! * [`compose_livery`] returns the key together with the composed RGB8
//!   image.
//!
//! Stage **F09-C** adds the production cache map this stage left open
//! ([`LiveryVariantStore`]): composed variants keyed by every input that
//! distinguishes them, with [`variant_key`] exposing the key of a
//! source/paint pair before anything is composed. Requesting a different
//! paint adds a new entry and never mutates the stored bytes, so two model
//! instances that share a source and choose different faction colors cannot
//! contaminate each other (spec non-negotiable #5). The store produces and
//! owns composed content; mapping instances onto entries, the construction
//! preview and teardown are the app layer's (`cs_app::livery`).
//!
//! Composition is deterministic: the same source and paint always give the
//! same key and the same bytes. Findings and the recorded unknowns:
//! `docs/findings/2026-09-28-f09-b-deterministic-layered-composition.md` and
//! `docs/findings/2026-09-29-f09-c-model-instances-and-construction-preview.md`.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use cs_assets::install::sha256;
use cs_formats::bm::{BM_COMPOSITION_VERSION, BmComposite, BmError, BmFile, BmPlane, PaintColor};
use cs_formats::io::AllocationBudget;
use cs_types::evidence::ContentHash;

/// The three paint colors a livery applies through a BM's mask planes, in
/// plane order (mask 1, mask 2, mask 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LiveryPaint {
    colors: [PaintColor; 3],
}

impl LiveryPaint {
    /// A paint scheme from its three colors.
    pub const fn new(colors: [PaintColor; 3]) -> Self {
        Self { colors }
    }

    /// The colors, in mask-plane order.
    pub const fn colors(&self) -> [PaintColor; 3] {
        self.colors
    }
}

/// A deterministic key for one composed variant.
///
/// Every input the spec names for a cache key is here: the source planes
/// (masks, base, overlay/decals) through [`Self::source`], the three colors
/// through [`Self::colors`] and the composition algorithm through
/// [`Self::algorithm`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LiveryVariantKey {
    source: ContentHash,
    colors: [PaintColor; 3],
    algorithm: u32,
}

impl LiveryVariantKey {
    /// The fingerprint of the parsed source image.
    pub fn source(&self) -> &ContentHash {
        &self.source
    }

    /// The colors, in mask-plane order.
    pub fn colors(&self) -> [PaintColor; 3] {
        self.colors
    }

    /// The composition algorithm version.
    pub fn algorithm(&self) -> u32 {
        self.algorithm
    }

    /// A canonical digest over every field, usable as a map key or as a
    /// catalog `fingerprint`.
    pub fn digest(&self) -> ContentHash {
        let mut material = Vec::with_capacity(32 + 9 + 4);
        material.extend_from_slice(self.source.as_bytes());
        for color in self.colors {
            material.extend_from_slice(&color.channels());
        }
        material.extend_from_slice(&self.algorithm.to_le_bytes());
        sha256(&material)
    }
}

/// One composed livery: the variant key it was produced under and the RGB8
/// image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComposedLivery {
    key: LiveryVariantKey,
    image: BmComposite,
}

impl ComposedLivery {
    /// The key this composition was produced under.
    pub fn key(&self) -> &LiveryVariantKey {
        &self.key
    }

    /// The composed image.
    pub fn image(&self) -> &BmComposite {
        &self.image
    }

    /// Every composed texel, row-major from the top-left, three bytes each.
    pub fn rgb(&self) -> &[u8] {
        self.image.rgb()
    }

    /// Composed RGB at canonical texel `(x, y)`.
    pub fn texel(&self, x: u32, y: u32) -> Option<[u8; 3]> {
        self.image.texel(x, y)
    }
}

/// Fingerprints the parsed source image so a composed variant can be keyed on
/// it.
///
/// The dimensions, each stored plane and any unsupported tail are folded in
/// through their own SHA-256 digests, so the whole covered image is covered
/// without copying it. This is a new-engine key construction, not an original
/// file field.
pub fn source_fingerprint(file: &BmFile<'_>) -> ContentHash {
    let mut material = Vec::with_capacity(4 + BmPlane::ALL.len() * 32 + 32);
    material.extend_from_slice(&file.header().height.to_le_bytes());
    material.extend_from_slice(&file.header().width.to_le_bytes());
    for plane in BmPlane::ALL {
        material.extend_from_slice(sha256(file.stored_plane(plane)).as_bytes());
    }
    if let Some(tail) = file.tail() {
        material.extend_from_slice(sha256(tail.bytes).as_bytes());
    }
    sha256(&material)
}

/// The deterministic key of the variant that composes `file` with `paint`.
///
/// This is exactly the key [`compose_livery`] stamps onto its result and the
/// key [`LiveryVariantStore`] indexes by, exposed so a caller can ask whether
/// a variant already exists — or tell two variants apart — without composing
/// anything.
pub fn variant_key(file: &BmFile<'_>, paint: &LiveryPaint) -> LiveryVariantKey {
    LiveryVariantKey {
        source: source_fingerprint(file),
        colors: paint.colors,
        algorithm: BM_COMPOSITION_VERSION,
    }
}

/// Composes `file` with `paint` and returns the deterministic variant key
/// together with the composed image.
///
/// # Errors
///
/// As [`BmFile::compose`]: a composed buffer beyond `budget` is refused
/// without allocating.
pub fn compose_livery(
    file: &BmFile<'_>,
    paint: &LiveryPaint,
    budget: &mut AllocationBudget,
) -> Result<ComposedLivery, BmError> {
    let image = file.compose(paint.colors, budget)?;
    Ok(ComposedLivery {
        key: variant_key(file, paint),
        image,
    })
}

/// The composed variants a runtime has produced, keyed by every input that
/// distinguishes them: the source fingerprint, the three colors and the
/// algorithm version.
///
/// The store is the production cache map F09-B deferred. It never mutates a
/// stored variant in place: composing a source with a different paint inserts
/// a second entry and leaves the first variant's bytes and key untouched
/// (spec non-negotiable #5). So two model instances that share one source
/// image but choose different faction colors resolve to independent entries,
/// and switching one instance's paint cannot cross-contaminate the other.
///
/// Entries are released explicitly — [`Self::remove`], [`Self::retain`] or
/// [`Self::clear`] — never silently evicted as a side effect of a lookup.
#[derive(Debug, Default)]
pub struct LiveryVariantStore {
    variants: HashMap<LiveryVariantKey, ComposedLivery>,
}

impl LiveryVariantStore {
    /// An empty store.
    pub fn new() -> Self {
        Self {
            variants: HashMap::new(),
        }
    }

    /// How many composed variants are stored.
    pub fn len(&self) -> usize {
        self.variants.len()
    }

    /// Whether no variant is stored.
    pub fn is_empty(&self) -> bool {
        self.variants.is_empty()
    }

    /// The key of `file` composed with `paint`, without composing it.
    pub fn key_for(file: &BmFile<'_>, paint: &LiveryPaint) -> LiveryVariantKey {
        variant_key(file, paint)
    }

    /// Whether the variant for `file` and `paint` is already stored.
    pub fn contains(&self, file: &BmFile<'_>, paint: &LiveryPaint) -> bool {
        self.variants.contains_key(&variant_key(file, paint))
    }

    /// Returns the stored variant for `file` and `paint`, composing and
    /// storing it on the first request.
    ///
    /// A hit returns the existing bytes unchanged; a miss composes once and
    /// inserts. A failed composition stores nothing and leaves every existing
    /// variant untouched, so a retry can be attempted with a larger budget.
    ///
    /// # Errors
    ///
    /// As [`compose_livery`].
    pub fn compose(
        &mut self,
        file: &BmFile<'_>,
        paint: &LiveryPaint,
        budget: &mut AllocationBudget,
    ) -> Result<&ComposedLivery, BmError> {
        let key = variant_key(file, paint);
        if let Entry::Vacant(entry) = self.variants.entry(key) {
            entry.insert(compose_livery(file, paint, budget)?);
        }
        Ok(self
            .variants
            .get(&key)
            .expect("the variant was either already present or just inserted"))
    }

    /// The stored variant under `key`, if any.
    pub fn get(&self, key: &LiveryVariantKey) -> Option<&ComposedLivery> {
        self.variants.get(key)
    }

    /// Removes and returns the variant under `key`; every other variant is
    /// untouched.
    pub fn remove(&mut self, key: &LiveryVariantKey) -> Option<ComposedLivery> {
        self.variants.remove(key)
    }

    /// Keeps only the variants whose keys `is_referenced` accepts and returns
    /// how many were dropped.
    pub fn retain(&mut self, mut is_referenced: impl FnMut(&LiveryVariantKey) -> bool) -> usize {
        let before = self.variants.len();
        self.variants.retain(|key, _| is_referenced(key));
        before - self.variants.len()
    }

    /// Removes every variant and returns how many were dropped.
    pub fn clear(&mut self) -> usize {
        let dropped = self.variants.len();
        self.variants.clear();
        dropped
    }
}

/// Acceptance stage F09-B. Every fixture is newly authored synthetic bytes
/// built here; nothing is derived from original game data. These tests call
/// the production [`compose_livery`] / [`source_fingerprint`] path.
#[cfg(test)]
mod tests {
    use cs_formats::{ParseContext, read_bm};

    use super::*;

    const RED: PaintColor = PaintColor::new(255, 0, 0);
    const GREEN: PaintColor = PaintColor::new(0, 255, 0);
    const BLUE: PaintColor = PaintColor::new(0, 0, 255);
    const PAINT_X: PaintColor = PaintColor::new(200, 100, 50);

    const BASE: [[u8; 3]; 4] = [[10, 20, 30], [40, 50, 60], [70, 80, 90], [100, 110, 120]];

    /// A 2x2 BM with the given masks and overlay, header height then width,
    /// planes in stored order. The stored rows are bottom (`BASE[0..2]`) then
    /// top (`BASE[2..4]`).
    fn build(masks: [u8; 3], overlay_alpha: u8) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&2u16.to_le_bytes()); // height
        bytes.extend_from_slice(&2u16.to_le_bytes()); // width
        for texel in BASE {
            bytes.extend_from_slice(&texel);
        }
        bytes.extend_from_slice(&[masks[0]; 4]);
        bytes.extend_from_slice(&[masks[1]; 4]);
        bytes.extend_from_slice(&[masks[2]; 4]);
        for texel in 0..4u8 {
            bytes.extend_from_slice(&[texel * 3, texel * 3 + 1, texel * 3 + 2, overlay_alpha]);
        }
        bytes
    }

    /// Parses the synthetic bytes; the returned buffer must outlive the file.
    fn parse(bytes: &[u8]) -> Result<BmFile<'_>, BmError> {
        let mut context = ParseContext::with_defaults("synthetic/f09-b.bm");
        read_bm(&mut context, bytes)
    }

    fn budget() -> AllocationBudget {
        AllocationBudget::with_defaults("synthetic/f09-b.bm")
    }

    /// Canonical (top-down) order of `BASE`: top stored row first.
    const CANONICAL: [[u8; 3]; 4] = [BASE[2], BASE[3], BASE[0], BASE[1]];

    /// The zero-mask endpoint: no paint color is applied and a transparent
    /// overlay leaves the base untouched, through the content path.
    #[test]
    fn accept_f09_b_content_all_zero_masks_compose_to_the_base() {
        let bytes = build([0, 0, 0], 0);
        let file = parse(&bytes).expect("the synthetic image parses");
        let paint = LiveryPaint::new([RED, GREEN, BLUE]);
        let composed = compose_livery(&file, &paint, &mut budget()).expect("composition fits");

        assert_eq!(composed.rgb(), CANONICAL.concat().as_slice());
        assert_eq!(composed.texel(0, 0), Some(CANONICAL[0]));
        assert_eq!(composed.texel(1, 1), Some(CANONICAL[3]));
        assert_eq!(composed.texel(2, 0), None);
        assert_eq!(composed.key().colors(), [RED, GREEN, BLUE]);
        assert_eq!(composed.key().algorithm(), BM_COMPOSITION_VERSION);
        assert_eq!(composed.key().source(), &source_fingerprint(&file));
    }

    /// The full-mask endpoint: white placeholder colors leave the base alone,
    /// a colored third plane scales it (floored), and a second composition
    /// with other colors leaves the first result untouched.
    #[test]
    fn accept_f09_b_content_paint_selects_and_does_not_mutate_other_variants() {
        let bytes = build([255, 255, 255], 0);
        let file = parse(&bytes).expect("the synthetic image parses");
        let first = LiveryPaint::new([PaintColor::WHITE, PaintColor::WHITE, PAINT_X]);
        let second = LiveryPaint::new([PAINT_X, PaintColor::WHITE, PaintColor::WHITE]);

        let a = compose_livery(&file, &first, &mut budget()).expect("composition fits");
        let snapshot = a.rgb().to_vec();
        let key = *a.key();
        // floor(base * (200,100,50) / 255) per channel.
        assert_eq!(
            a.rgb(),
            [[54, 31, 17], [78, 43, 23], [7, 7, 5], [31, 19, 11]]
                .concat()
                .as_slice()
        );

        // The same source with a different paint is a different variant, and
        // composing it must not have changed the first one.
        let b = compose_livery(&file, &second, &mut budget()).expect("composition fits");
        assert_eq!(b.rgb(), a.rgb(), "white planes are order-independent");
        assert_ne!(*b.key(), key, "different colors, different key");
        assert_ne!(b.key().digest(), key.digest());
        assert_eq!(a.rgb(), snapshot.as_slice(), "the first variant is intact");
        assert_eq!(*a.key(), key);
    }

    /// The variant key covers the source planes: a different mask or a
    /// different overlay is a different source, hence a different key.
    #[test]
    fn accept_f09_b_content_key_covers_masks_and_overlay() {
        let zero = build([0, 0, 0], 0);
        let mask = build([255, 0, 0], 0);
        let overlay = build([0, 0, 0], 255);
        let files: Vec<BmFile<'_>> = [&zero, &mask, &overlay]
            .into_iter()
            .map(|bytes| parse(bytes).expect("the synthetic image parses"))
            .collect();
        let paint = LiveryPaint::new([RED, GREEN, BLUE]);

        let fingerprints: Vec<ContentHash> = files.iter().map(source_fingerprint).collect();
        assert_ne!(fingerprints[0], fingerprints[1], "masks are in the source");
        assert_ne!(fingerprints[0], fingerprints[2], "overlay is in the source");
        assert_ne!(fingerprints[1], fingerprints[2]);

        let keys: Vec<LiveryVariantKey> = files
            .iter()
            .map(|file| {
                *compose_livery(file, &paint, &mut budget())
                    .expect("composition fits")
                    .key()
            })
            .collect();
        assert!(keys.iter().any(|key| *key != keys[0]));
        assert_eq!(
            keys.iter()
                .map(LiveryVariantKey::source)
                .collect::<Vec<_>>(),
            fingerprints.iter().collect::<Vec<_>>()
        );
    }

    /// The composed values are deterministic and a too-small budget is
    /// refused without allocating.
    #[test]
    fn accept_f09_b_content_is_deterministic_and_bounded() {
        let bytes = build([255, 255, 255], 128);
        let file = parse(&bytes).expect("the synthetic image parses");
        let paint = LiveryPaint::new([RED, PAINT_X, BLUE]);

        let first = compose_livery(&file, &paint, &mut budget()).expect("composition fits");
        let second = compose_livery(&file, &paint, &mut budget()).expect("composition fits");
        assert_eq!(first, second, "same inputs, same result");
        assert_eq!(first.key(), second.key());

        // 2x2 RGB8 needs 12 bytes; a 5-byte budget is refused.
        let mut tiny = AllocationBudget::new("synthetic/f09-b.bm", 5);
        let error = compose_livery(&file, &paint, &mut tiny).expect_err("too small");
        assert_eq!(error.code(), "allocation_budget_exceeded");
        assert_eq!(tiny.used(), 0, "nothing is charged when the reserve fails");
    }

    /// A larger source so the content-level F09-C tests can pick paints that
    /// really change the bytes. 2x2, full mask on every plane and a
    /// transparent overlay, same layout as [`build`].
    fn two_by_two(masks: [u8; 3], overlay_alpha: u8) -> Vec<u8> {
        build(masks, overlay_alpha)
    }

    const FACTIONS: [[PaintColor; 3]; 2] = [
        [RED, PAINT_X, PaintColor::WHITE],
        [PaintColor::WHITE, PAINT_X, BLUE],
    ];

    /// Two variants of one source, one store: composing the second paint must
    /// not change the first variant's bytes or key, and both stay stored.
    #[test]
    fn accept_f09_c_store_keeps_two_paints_of_one_source_distinct() {
        let bytes = two_by_two([255, 255, 255], 0);
        let file = parse(&bytes).expect("the synthetic image parses");
        let first = LiveryPaint::new(FACTIONS[0]);
        let second = LiveryPaint::new(FACTIONS[1]);
        let key_first = variant_key(&file, &first);
        let key_second = variant_key(&file, &second);
        assert_ne!(
            key_first, key_second,
            "the paint colors are part of the key"
        );

        let mut store = LiveryVariantStore::new();
        assert!(store.is_empty());
        let a = store
            .compose(&file, &first, &mut budget())
            .expect("composition fits");
        let a_rgb = a.rgb().to_vec();
        let a_key = *a.key();
        assert_eq!(a_key, key_first, "key_for agrees with the composed key");
        assert_eq!(store.len(), 1);

        let b = store
            .compose(&file, &second, &mut budget())
            .expect("composition fits");
        assert_eq!(*b.key(), key_second);
        assert_ne!(b.rgb(), a_rgb.as_slice(), "the two factions differ");

        assert_eq!(store.len(), 2, "a second paint adds a second variant");
        assert!(store.contains(&file, &first));
        assert!(store.contains(&file, &second));
        assert_eq!(
            store.get(&a_key).expect("the first variant stays").rgb(),
            a_rgb.as_slice(),
            "composing the second paint must not mutate the first"
        );
        assert_eq!(*store.get(&a_key).expect("stored").key(), a_key);
    }

    /// A repeated request is the same one variant, and entries are released
    /// only explicitly: remove drops one, retain drops only unreferenced.
    #[test]
    fn accept_f09_c_store_reuses_and_releases_variants_explicitly() {
        let bytes = two_by_two([255, 255, 255], 0);
        let file = parse(&bytes).expect("the synthetic image parses");
        let first = LiveryPaint::new(FACTIONS[0]);
        let second = LiveryPaint::new(FACTIONS[1]);
        let key_first = variant_key(&file, &first);
        let key_second = variant_key(&file, &second);

        let mut store = LiveryVariantStore::new();
        let one = store
            .compose(&file, &first, &mut budget())
            .expect("composition fits")
            .rgb()
            .to_vec();
        let again = store
            .compose(&file, &first, &mut budget())
            .expect("composition fits")
            .rgb()
            .to_vec();
        assert_eq!(one, again, "the same request keeps the same bytes");
        assert_eq!(store.len(), 1, "a repeated request is one variant");
        store
            .compose(&file, &second, &mut budget())
            .expect("composition fits");
        assert_eq!(store.len(), 2);

        // Evict every variant except the first; the second alone is dropped.
        let evicted = store.retain(|key| *key == key_first);
        assert_eq!(evicted, 1);
        assert_eq!(store.len(), 1);
        assert!(store.get(&key_first).is_some());
        assert!(store.get(&key_second).is_none());

        let removed = store.remove(&key_first).expect("the first is stored");
        assert_eq!(*removed.key(), key_first);
        assert!(store.is_empty());
        assert_eq!(store.remove(&key_first), None, "removing twice is a no-op");

        store
            .compose(&file, &first, &mut budget())
            .expect("composition fits");
        store
            .compose(&file, &second, &mut budget())
            .expect("composition fits");
        assert_eq!(store.clear(), 2);
        assert!(store.is_empty());
    }

    /// A refused composition stores nothing and leaves existing variants
    /// intact, so the caller can retry with a budget that fits.
    #[test]
    fn accept_f09_c_store_refuses_over_budget_without_partial_state() {
        let bytes = two_by_two([255, 255, 255], 0);
        let file = parse(&bytes).expect("the synthetic image parses");
        let first = LiveryPaint::new(FACTIONS[0]);
        let second = LiveryPaint::new(FACTIONS[1]);

        let mut store = LiveryVariantStore::new();
        store
            .compose(&file, &first, &mut budget())
            .expect("composition fits");

        // 2x2 RGB8 needs 12 bytes; 5 is refused.
        let mut tiny = AllocationBudget::new("synthetic/f09-c.bm", 5);
        let error = store
            .compose(&file, &second, &mut tiny)
            .expect_err("the second variant does not fit");
        assert_eq!(error.code(), "allocation_budget_exceeded");
        assert_eq!(tiny.used(), 0, "a refused reservation allocates nothing");
        assert_eq!(store.len(), 1, "the failed variant stored nothing");
        assert!(!store.contains(&file, &second));
        assert!(
            store.contains(&file, &first),
            "the existing variant is untouched"
        );

        // Retry with a sufficient budget succeeds and leaves both variants.
        store
            .compose(&file, &second, &mut budget())
            .expect("the retry fits");
        assert_eq!(store.len(), 2);
    }
}
