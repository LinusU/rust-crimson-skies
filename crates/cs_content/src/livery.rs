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
//!
//! The **F09-PAINTSHOP** section at the end of this module is the paint shop
//! itself: [`PaintShopCatalog`] reads the one readable member that declares the
//! paint-shop option space — the `[@Paint@]` section of `ASSETS/LAYOUT.CSV` —
//! and records, per dropdown, how many entries the original control displays and
//! that **no field of it carries a value**. The swatch palette, the shade table
//! and the pattern display names are therefore engine-internal, and
//! [`PaintShopValue`] has no variant that could carry one.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fmt;

use cs_assets::install::sha256;
use cs_assets::rof::{RofReadError, RofSource};
use cs_assets::vfs::{ContentSession, ResolveError};
use cs_assets::zbd::{ZbdContainer, ZbdError};
use cs_formats::bm::{BM_COMPOSITION_VERSION, BmComposite, BmError, BmFile, BmPlane, PaintColor};
use cs_formats::io::AllocationBudget;
use cs_formats::{ParseContext, read_bm};
use cs_types::asset_id::{AssetKey, SourceSpan, SourceSpanError};
use cs_types::evidence::ContentHash;

use crate::config::{
    ConfigDocument, ConfigEntry, ConfigError, FieldSpelling, Lookup, RawValue, RecordSchema,
    RecordView,
};
use cs_formats::text::RecordKind;

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

// ------------------------------------------------------------ F09-D ---

/// The directory the pinned livery helper reads stock liveries from:
/// `ASSETS/GRAPHICS/<FACTION>/`.
///
/// Claim class *observed tool* ([S10]). Whether the original renderer looks
/// anywhere else is not established, so [`StockLiveryCatalog::discover`]
/// records a `.bm` member outside this layout instead of ignoring it.
pub const LIVERY_GRAPHICS_DIRECTORY: &str = "graphics";

/// One BM member name in the observed stock-livery layout:
/// `.../GRAPHICS/<FACTION>/<PREFIX>_<PART>.BM`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveryName {
    /// The faction directory component.
    pub faction: String,
    /// The airframe prefix before the first `_`.
    pub prefix: String,
    /// The remaining part of the file stem.
    pub part: String,
}

/// Parses a member spelling as a stock livery name, or returns `None` when it
/// is not one.
///
/// The layout is the pinned helper's: a `.bm` file (extension compared
/// case-insensitively) inside `GRAPHICS/<FACTION>/`, named
/// `<PREFIX>_<PART>`. The prefixes are the original names' own — which
/// airframe a prefix belongs to comes from original data, not from a table
/// here (spec non-negotiable #4).
pub fn parse_livery_spelling(spelling: &str) -> Option<LiveryName> {
    let components: Vec<&str> = spelling.split('/').collect();
    if components.len() < 3 {
        return None;
    }
    if !components[components.len() - 3].eq_ignore_ascii_case(LIVERY_GRAPHICS_DIRECTORY) {
        return None;
    }
    let faction = components[components.len() - 2];
    let file = components[components.len() - 1];
    if faction.is_empty() {
        return None;
    }
    let stem = file.strip_suffix(".bm").or_else(|| {
        let (stem, extension) = file.rsplit_once('.')?;
        extension.eq_ignore_ascii_case("bm").then_some(stem)
    })?;
    let (prefix, part) = stem.split_once('_')?;
    if prefix.is_empty() || part.is_empty() {
        return None;
    }
    Some(LiveryName {
        faction: faction.to_owned(),
        prefix: prefix.to_owned(),
        part: part.to_owned(),
    })
}

/// One stock livery BM discovered in an installation: which faction and
/// airframe it belongs to and what its header declares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StockLivery {
    spelling: String,
    faction: String,
    prefix: String,
    part: String,
    width: u32,
    height: u32,
    covered_len: u64,
    tail_bytes: u64,
    fingerprint: ContentHash,
}

impl StockLivery {
    fn from_file(spelling: &str, name: LiveryName, file: &BmFile<'_>) -> Self {
        Self {
            spelling: spelling.to_owned(),
            faction: name.faction,
            prefix: name.prefix,
            part: name.part,
            width: file.width(),
            height: file.height(),
            covered_len: file.covered_len(),
            tail_bytes: file.tail().map_or(0, |tail| tail.bytes.len() as u64),
            fingerprint: source_fingerprint(file),
        }
    }

    /// The member spelling inside its container.
    pub fn spelling(&self) -> &str {
        &self.spelling
    }

    /// The faction directory.
    pub fn faction(&self) -> &str {
        &self.faction
    }

    /// The airframe prefix before the part.
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// The part name after the prefix.
    pub fn part(&self) -> &str {
        &self.part
    }

    /// Declared columns.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Declared rows.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The covered `4 + 10*w*h` length.
    pub fn covered_len(&self) -> u64 {
        self.covered_len
    }

    /// Bytes after the covered length, `0` when the member is exactly the
    /// observed subset.
    pub fn tail_bytes(&self) -> u64 {
        self.tail_bytes
    }

    /// The [`source_fingerprint`] of the parsed member.
    pub fn fingerprint(&self) -> &ContentHash {
        &self.fingerprint
    }
}

/// A `.bm` member that is not a valid stock livery, kept rather than
/// dropped so an unsupported variant is visible (spec non-negotiable #1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveryFinding {
    spelling: String,
    code: &'static str,
    detail: String,
}

impl LiveryFinding {
    /// A `.bm` member whose spelling is not the observed livery layout.
    pub fn not_a_stock_livery(spelling: &str) -> Self {
        Self {
            spelling: spelling.to_owned(),
            code: "not_a_stock_livery",
            detail: format!("`{spelling}` is not GRAPHICS/<FACTION>/<PREFIX>_<PART>.bm"),
        }
    }

    /// A `.bm` member that could not be read from its container.
    pub fn read_failed(spelling: &str, error: &RofReadError) -> Self {
        Self {
            spelling: spelling.to_owned(),
            code: "member_read_failed",
            detail: format!("{error}"),
        }
    }

    /// A member that is not a BM image in the observed subset. `code` is the
    /// underlying parse failure's own stable code (`empty_image`,
    /// `unexpected_eof`, ...).
    pub fn unsupported(spelling: &str, error: &BmError) -> Self {
        Self {
            spelling: spelling.to_owned(),
            code: error.code(),
            detail: format!("{error}"),
        }
    }

    /// The member spelling the finding is about.
    pub fn spelling(&self) -> &str {
        &self.spelling
    }

    /// Stable lowercase identifier.
    pub fn code(&self) -> &'static str {
        self.code
    }

    /// Human-readable detail.
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

/// The valid stock combinations of one airframe prefix: every faction whose
/// directory stores at least one `<PREFIX>_*.bm`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveryCombination {
    prefix: String,
    factions: Vec<String>,
    assets: usize,
}

impl LiveryCombination {
    /// The airframe prefix.
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// The factions with at least one asset for this prefix, sorted.
    pub fn factions(&self) -> &[String] {
        &self.factions
    }

    /// How many BM assets this prefix has, over all its factions.
    pub fn assets(&self) -> usize {
        self.assets
    }
}

/// The stock liveries discovered in a container, their valid combinations
/// and every member that could not be verified, all in spelling order.
#[derive(Debug, Default)]
pub struct StockLiveryCatalog {
    assets: Vec<StockLivery>,
    findings: Vec<LiveryFinding>,
}

impl StockLiveryCatalog {
    /// An empty catalog.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one member from its spelling and its **decoded** bytes.
    ///
    /// A member that is not a `.bm` file is ignored; a `.bm` member that is
    /// not the observed livery layout or does not parse becomes a
    /// [`LiveryFinding`] instead of an asset.
    pub fn add(&mut self, spelling: &str, bytes: &[u8]) {
        if !spelling.to_ascii_lowercase().ends_with(".bm") {
            return;
        }
        let Some(name) = parse_livery_spelling(spelling) else {
            self.findings
                .push(LiveryFinding::not_a_stock_livery(spelling));
            return;
        };
        let mut context = ParseContext::with_defaults(spelling);
        match read_bm(&mut context, bytes) {
            Ok(file) => self
                .assets
                .push(StockLivery::from_file(spelling, name, &file)),
            Err(error) => self
                .findings
                .push(LiveryFinding::unsupported(spelling, &error)),
        }
        // Spelling order makes the catalog deterministic however the members
        // were enumerated.
        self.assets.sort_by(|a, b| a.spelling.cmp(&b.spelling));
        self.findings.sort_by(|a, b| a.spelling.cmp(&b.spelling));
    }

    /// Discovers every `.bm` member of `source` through the production ROF
    /// reader and verifies each one.
    ///
    /// A member that cannot be read or parsed is recorded in
    /// [`Self::findings`]; nothing is skipped silently.
    pub fn discover(source: &RofSource) -> Self {
        let mut catalog = Self::new();
        for member in source.members() {
            if !member.spelling.to_ascii_lowercase().ends_with(".bm") {
                continue;
            }
            let Ok(key) =
                AssetKey::from_spelling(source.namespace().as_str(), &member.spelling, "default")
            else {
                catalog
                    .findings
                    .push(LiveryFinding::not_a_stock_livery(&member.spelling));
                continue;
            };
            match source.read(&key) {
                Ok(read) => catalog.add(&member.spelling, &read.data),
                Err(error) => catalog
                    .findings
                    .push(LiveryFinding::read_failed(&member.spelling, &error)),
            }
        }
        catalog.assets.sort_by(|a, b| a.spelling.cmp(&b.spelling));
        catalog.findings.sort_by(|a, b| a.spelling.cmp(&b.spelling));
        catalog
    }

    /// Every verified stock livery, in spelling order.
    pub fn assets(&self) -> &[StockLivery] {
        &self.assets
    }

    /// Every `.bm` member that could not be verified, in spelling order.
    pub fn findings(&self) -> &[LiveryFinding] {
        &self.findings
    }

    /// The distinct factions, sorted.
    pub fn factions(&self) -> Vec<String> {
        let mut factions: Vec<String> = self
            .assets
            .iter()
            .map(|asset| asset.faction.clone())
            .collect();
        factions.sort();
        factions.dedup();
        factions
    }

    /// The distinct airframe prefixes, sorted.
    pub fn prefixes(&self) -> Vec<String> {
        let mut prefixes: Vec<String> = self
            .assets
            .iter()
            .map(|asset| asset.prefix.clone())
            .collect();
        prefixes.sort();
        prefixes.dedup();
        prefixes
    }

    /// The valid combinations: for every prefix, the factions that store at
    /// least one asset, with the asset count. Sorted by prefix.
    pub fn combinations(&self) -> Vec<LiveryCombination> {
        let mut table: BTreeMap<&str, BTreeMap<&str, usize>> = BTreeMap::new();
        for asset in &self.assets {
            *table
                .entry(asset.prefix.as_str())
                .or_default()
                .entry(asset.faction.as_str())
                .or_default() += 1;
        }
        table
            .into_iter()
            .map(|(prefix, factions)| LiveryCombination {
                prefix: prefix.to_owned(),
                assets: factions.values().sum(),
                factions: factions.into_keys().map(str::to_owned).collect(),
            })
            .collect()
    }
}

// -------------------------------------------------------- F09-PALETTE ---

/// The installation-relative path of the shared reader archive that stores the
/// original vehicle definitions, [`PALETTE_MEMBER`] among them
/// (`docs/findings/2026-10-02-f09-palette-original-faction-palettes.md`).
pub const PALETTE_CONTAINER: &str = "ZBD/zrdr.zbd";

/// The member of [`PALETTE_CONTAINER`] whose records carry the paint fields:
/// the faction paint pattern, the three mask colors and the three decal
/// indices of every vehicle.
pub const PALETTE_MEMBER: &str = "vehicle.zrd";

/// How many mask colors and decals a complete paint triple has.
const PALETTE_SLOTS: usize = 3;

/// `.zrd` node tags: `1` int, `2` float, `3` string, `4` list.
const ZRD_TAG_INT: u32 = 1;
const ZRD_TAG_FLOAT: u32 = 2;
const ZRD_TAG_TEXT: u32 = 3;
const ZRD_TAG_LIST: u32 = 4;

/// The smallest encoded `.zrd` node: a `u32` tag plus a `u32` payload word.
const MIN_ZRD_NODE_BYTES: usize = 8;

/// The deepest a `.zrd` document may nest before it is refused, so a hostile
/// member cannot exhaust the stack.
const MAX_ZRD_DEPTH: u32 = 64;

/// A `.zrd` decode failure: its stable code and the byte offset it was found
/// at, relative to the member. Never a symptom of a guessed layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ZrdFailure {
    code: &'static str,
    offset: u64,
    detail: &'static str,
}

/// The decoded payload of one `.zrd` node.
#[derive(Clone, Debug, PartialEq)]
enum ZrdKind {
    Int(u32),
    /// A float node is recognized so a document containing one still parses;
    /// no paint field is a float, so its value is not read.
    Float,
    Text(String),
    List(Vec<ZrdNode>),
}

/// One decoded `.zrd` node and the byte range it occupied in the member.
#[derive(Clone, Debug, PartialEq)]
struct ZrdNode {
    kind: ZrdKind,
    /// Offset of the node's first byte, relative to the member.
    offset: u64,
    /// Encoded length of the node, tag included.
    length: u64,
}

impl ZrdNode {
    fn as_list(&self) -> Option<&[ZrdNode]> {
        match &self.kind {
            ZrdKind::List(children) => Some(children),
            _ => None,
        }
    }

    fn as_text(&self) -> Option<&str> {
        match &self.kind {
            ZrdKind::Text(text) => Some(text),
            _ => None,
        }
    }

    fn as_int(&self) -> Option<u32> {
        match self.kind {
            ZrdKind::Int(value) => Some(value),
            _ => None,
        }
    }
}

/// The observed `.zrd` grammar: `<u32 tag>`, where tag `1` is a `u32`, `2` a
/// `f32`, `3` a `u32` length followed by that many bytes, and `4` a `u32`
/// count followed by **`count - 1`** child nodes. Measured against every
/// `.zrd` member of `ZBD/zrdr.zbd`, each of which consumes the member exactly
/// (`docs/findings/2026-10-02-f09-palette-original-faction-palettes.md`).
struct ZrdReader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> ZrdReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }

    fn read_u32(&mut self) -> Result<u32, ZrdFailure> {
        let offset = self.position as u64;
        let end = self.position.checked_add(4).ok_or(ZrdFailure {
            code: "truncated",
            offset,
            detail: "a u32 does not fit the member",
        })?;
        let word = self.bytes.get(self.position..end).ok_or(ZrdFailure {
            code: "truncated",
            offset,
            detail: "a u32 does not fit the member",
        })?;
        self.position = end;
        Ok(u32::from_le_bytes(word.try_into().expect("four bytes")))
    }

    fn read_body(&mut self, length: usize, what: &'static str) -> Result<&'a [u8], ZrdFailure> {
        let offset = self.position as u64;
        let end = self.position.checked_add(length).ok_or(ZrdFailure {
            code: "truncated",
            offset,
            detail: what,
        })?;
        let body = self.bytes.get(self.position..end).ok_or(ZrdFailure {
            code: "truncated",
            offset,
            detail: what,
        })?;
        self.position = end;
        Ok(body)
    }

    fn parse_node(&mut self, depth: u32) -> Result<ZrdNode, ZrdFailure> {
        let start = self.position as u64;
        if depth > MAX_ZRD_DEPTH {
            return Err(ZrdFailure {
                code: "depth_exceeded",
                offset: start,
                detail: "the document nests too deeply",
            });
        }
        let tag = self.read_u32()?;
        let kind = match tag {
            ZRD_TAG_INT => ZrdKind::Int(self.read_u32()?),
            // A float is one payload word; no paint field is a float.
            ZRD_TAG_FLOAT => {
                self.read_u32()?;
                ZrdKind::Float
            }
            ZRD_TAG_TEXT => {
                let length = self.read_u32()? as usize;
                let body = self.read_body(length, "a text body")?;
                let text = std::str::from_utf8(body).map_err(|_| ZrdFailure {
                    code: "invalid_text",
                    offset: start,
                    detail: "a text node is not valid UTF-8",
                })?;
                ZrdKind::Text(text.to_owned())
            }
            ZRD_TAG_LIST => {
                // A list of `N` holds `N - 1` children (measured, not guessed).
                let count = self.read_u32()?;
                let children = count.saturating_sub(1) as usize;
                // A child costs at least eight bytes, so a count larger than
                // the remaining bytes cannot be honest; refusing here also
                // bounds the allocation below.
                if children > self.remaining() / MIN_ZRD_NODE_BYTES + 1 {
                    return Err(ZrdFailure {
                        code: "count_exceeds_bytes",
                        offset: start,
                        detail: "a list declares more children than the member can hold",
                    });
                }
                let mut kids = Vec::with_capacity(children);
                for _ in 0..children {
                    kids.push(self.parse_node(depth + 1)?);
                }
                ZrdKind::List(kids)
            }
            _ => {
                return Err(ZrdFailure {
                    code: "unknown_tag",
                    offset: start,
                    detail: "a node tag is not one of the four documented kinds",
                });
            }
        };
        Ok(ZrdNode {
            kind,
            offset: start,
            length: self.position as u64 - start,
        })
    }
}

/// Decodes one `.zrd` member, refusing a tail that no node accounts for.
fn parse_zrd(bytes: &[u8]) -> Result<ZrdNode, ZrdFailure> {
    let mut reader = ZrdReader::new(bytes);
    let root = reader.parse_node(0)?;
    if reader.remaining() != 0 {
        return Err(ZrdFailure {
            code: "trailing_bytes",
            offset: reader.position as u64,
            detail: "the nodes do not account for every byte of the member",
        });
    }
    Ok(root)
}

/// The `(key, value)` field pairs of a `.zrd` list node: a text key is
/// followed by its value node, and any node without a preceding key is kept
/// with a `None` key rather than dropped.
fn zrd_field_pairs(node: &ZrdNode) -> Vec<(Option<&str>, &ZrdNode)> {
    let mut pairs = Vec::new();
    let Some(children) = node.as_list() else {
        return pairs;
    };
    let mut index = 0;
    while index < children.len() {
        let key = children[index].as_text();
        if key.is_some() && index + 1 < children.len() {
            pairs.push((key, &children[index + 1]));
            index += 2;
        } else {
            pairs.push((None, &children[index]));
            index += 1;
        }
    }
    pairs
}

/// The named records of the observed `vehicle.zrd` shape: the root's only
/// child is a list of alternating record-name strings and record lists.
fn vehicle_record_nodes(root: &ZrdNode) -> Result<Vec<(&str, &ZrdNode)>, ZrdFailure> {
    let top = root
        .as_list()
        .and_then(|children| children.first())
        .and_then(ZrdNode::as_list)
        .ok_or(ZrdFailure {
            code: "shape",
            offset: root.offset,
            detail: "the root does not hold the record list",
        })?;
    let mut records = Vec::new();
    let mut index = 0;
    while index < top.len() {
        let Some(name) = top[index].as_text() else {
            return Err(ZrdFailure {
                code: "shape",
                offset: top[index].offset,
                detail: "the record list does not alternate a name and a record",
            });
        };
        let Some(record) = top.get(index + 1) else {
            return Err(ZrdFailure {
                code: "shape",
                offset: top[index].offset,
                detail: "a record name has no record",
            });
        };
        records.push((name, record));
        index += 2;
    }
    Ok(records)
}

/// Where one `.zrd` member lives, so every extracted field gets a
/// container-absolute [`SourceSpan`].
struct PaletteProvenance<'a> {
    install_sha256: ContentHash,
    container_path: &'a str,
    member_sha256: ContentHash,
    member_offset: u64,
}

impl PaletteProvenance<'_> {
    fn span(&self, node: &ZrdNode) -> Result<SourceSpan, PaletteError> {
        SourceSpan::new(
            self.install_sha256,
            self.container_path,
            Some(PALETTE_MEMBER),
            self.member_offset + node.offset,
            node.length,
            Some(self.member_sha256),
        )
        .map_err(PaletteError::Span)
    }
}

/// One extracted RGB color and the exact bytes it was decoded from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteColor {
    rgb: [u8; 3],
    span: SourceSpan,
}

impl PaletteColor {
    /// The three channels, in stored order (red, green, blue).
    pub fn rgb(&self) -> [u8; 3] {
        self.rgb
    }

    /// The red channel.
    pub fn red(&self) -> u8 {
        self.rgb[0]
    }

    /// The green channel.
    pub fn green(&self) -> u8 {
        self.rgb[1]
    }

    /// The blue channel.
    pub fn blue(&self) -> u8 {
        self.rgb[2]
    }

    /// The bytes this color was decoded from, inside [`PALETTE_MEMBER`].
    pub fn span(&self) -> &SourceSpan {
        &self.span
    }
}

/// One extracted decal selection: the stored index and the bytes it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteDecal {
    index: u32,
    span: SourceSpan,
}

impl PaletteDecal {
    /// The stored decal index, verbatim. Its meaning (a decal-sheet cell or an
    /// engine id) is not established here.
    pub fn index(&self) -> u32 {
        self.index
    }

    /// The bytes this decal was decoded from, inside [`PALETTE_MEMBER`].
    pub fn span(&self) -> &SourceSpan {
        &self.span
    }
}

/// The paint fields of one vehicle record. `colors` and `decals` are both
/// empty (a record that names a pattern but stores no palette) or both
/// complete; a partial triple is refused during extraction, never padded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteRecord {
    name: String,
    pattern: String,
    pattern_span: SourceSpan,
    colors: Vec<PaletteColor>,
    decals: Vec<PaletteDecal>,
    span: SourceSpan,
}

impl PaletteRecord {
    /// The record's name as `vehicle.zrd` spells it.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The paint pattern the record names, used as the faction.
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// The bytes `paint_pattern` was decoded from.
    pub fn pattern_span(&self) -> &SourceSpan {
        &self.pattern_span
    }

    /// The whole record's range inside [`PALETTE_MEMBER`].
    pub fn span(&self) -> &SourceSpan {
        &self.span
    }

    /// The stored colors, in mask order; empty when the record names a
    /// pattern but stores no color triple.
    pub fn colors(&self) -> &[PaletteColor] {
        &self.colors
    }

    /// The stored decals, in mask order; empty when the record stores none.
    pub fn decals(&self) -> &[PaletteDecal] {
        &self.decals
    }

    /// Whether the record stores a complete color triple.
    pub fn has_colors(&self) -> bool {
        self.colors.len() == PALETTE_SLOTS
    }

    /// The color at `slot` (0..3), or a [`PaletteRefusal`] naming this record.
    pub fn color(&self, slot: usize) -> Result<&PaletteColor, PaletteRefusal> {
        self.colors
            .get(slot)
            .ok_or_else(|| PaletteRefusal::UnknownColorSlot {
                owner: self.name.clone(),
                slot,
                available: self.colors.len(),
            })
    }

    /// The decal at `slot` (0..3), or a [`PaletteRefusal`] naming this record.
    pub fn decal(&self, slot: usize) -> Result<&PaletteDecal, PaletteRefusal> {
        self.decals
            .get(slot)
            .ok_or_else(|| PaletteRefusal::UnknownDecalSlot {
                owner: self.name.clone(),
                slot,
                available: self.decals.len(),
            })
    }
}

/// Why a requested faction or slot is not part of the extracted palette.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaletteRefusal {
    /// No extracted faction has this name.
    UnknownFaction {
        /// The faction asked for.
        faction: String,
    },
    /// The faction or record stores no color at this slot.
    UnknownColorSlot {
        /// The faction or record the slot was asked of.
        owner: String,
        /// The slot asked for.
        slot: usize,
        /// How many slots the owner actually stores.
        available: usize,
    },
    /// The faction or record stores no decal at this slot.
    UnknownDecalSlot {
        /// The faction or record the slot was asked of.
        owner: String,
        /// The slot asked for.
        slot: usize,
        /// How many slots the owner actually stores.
        available: usize,
    },
}

impl fmt::Display for PaletteRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownFaction { faction } => {
                write!(f, "no extracted faction is named {faction:?}")
            }
            Self::UnknownColorSlot {
                owner,
                slot,
                available,
            } => write!(
                f,
                "{owner:?} stores no color at slot {slot} (it stores {available})"
            ),
            Self::UnknownDecalSlot {
                owner,
                slot,
                available,
            } => write!(
                f,
                "{owner:?} stores no decal at slot {slot} (it stores {available})"
            ),
        }
    }
}

impl std::error::Error for PaletteRefusal {}

/// One faction's palette: the color/decal triple its paint-bearing records
/// agree on, and the records that name it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FactionPalette {
    faction: String,
    colors: Vec<PaletteColor>,
    decals: Vec<PaletteDecal>,
    records: Vec<String>,
}

impl FactionPalette {
    /// The faction (`paint_pattern`) name.
    pub fn faction(&self) -> &str {
        &self.faction
    }

    /// The stored colors, in mask order.
    pub fn colors(&self) -> &[PaletteColor] {
        &self.colors
    }

    /// The stored decals, in mask order.
    pub fn decals(&self) -> &[PaletteDecal] {
        &self.decals
    }

    /// The `vehicle.zrd` records that name this faction.
    pub fn records(&self) -> &[String] {
        &self.records
    }

    /// The color at `slot` (0..3), or a [`PaletteRefusal`].
    pub fn color(&self, slot: usize) -> Result<&PaletteColor, PaletteRefusal> {
        self.colors
            .get(slot)
            .ok_or_else(|| PaletteRefusal::UnknownColorSlot {
                owner: self.faction.clone(),
                slot,
                available: self.colors.len(),
            })
    }

    /// The decal at `slot` (0..3), or a [`PaletteRefusal`].
    pub fn decal(&self, slot: usize) -> Result<&PaletteDecal, PaletteRefusal> {
        self.decals
            .get(slot)
            .ok_or_else(|| PaletteRefusal::UnknownDecalSlot {
                owner: self.faction.clone(),
                slot,
                available: self.decals.len(),
            })
    }
}

/// Content a paint record carries that this stage cannot turn into a palette,
/// kept rather than dropped (spec F09 non-negotiable #4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteFinding {
    code: &'static str,
    record: String,
    detail: String,
}

impl PaletteFinding {
    /// Stable lowercase identifier.
    pub fn code(&self) -> &'static str {
        self.code
    }

    /// The record the finding is about.
    pub fn record(&self) -> &str {
        &self.record
    }

    /// Human-readable detail.
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

/// Why the faction palette could not be extracted.
#[derive(Debug)]
pub enum PaletteError {
    /// The container did not resolve, route, index or list as a reader
    /// archive.
    Container(ZbdError),
    /// The reader archive does not declare the palette member.
    MissingMember {
        /// The member that was looked for.
        member: String,
    },
    /// The palette member is not the observed `.zrd` layout.
    Member {
        /// The member decoder's stable code.
        code: &'static str,
        /// Where in the member the failure was found.
        offset: u64,
        /// What was expected there.
        detail: &'static str,
    },
    /// A [`SourceSpan`] for an extracted field was refused.
    Span(SourceSpanError),
}

impl PaletteError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Container(error) => error.code(),
            Self::MissingMember { .. } => "missing_member",
            Self::Member { code, .. } => code,
            Self::Span(_) => "invalid_span",
        }
    }
}

impl fmt::Display for PaletteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Container(error) => write!(f, "{error}"),
            Self::MissingMember { member } => {
                write!(f, "the reader archive declares no {member:?} member")
            }
            Self::Member {
                code,
                offset,
                detail,
            } => write!(
                f,
                "the palette member is not the observed layout: {detail} (at {offset}, code {code})"
            ),
            Self::Span(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for PaletteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Container(error) => Some(error),
            Self::Span(error) => Some(error),
            Self::MissingMember { .. } | Self::Member { .. } => None,
        }
    }
}

impl From<ZrdFailure> for PaletteError {
    fn from(failure: ZrdFailure) -> Self {
        Self::Member {
            code: failure.code,
            offset: failure.offset,
            detail: failure.detail,
        }
    }
}

/// The original faction paint palette, extracted from the paint records of
/// [`PALETTE_MEMBER`] in [`PALETTE_CONTAINER`].
///
/// Every color, decal and pattern carries the container-absolute [`SourceSpan`]
/// it was decoded from, so a reviewer can re-read the exact bytes; nothing here
/// is a hardcoded table (spec F09 non-negotiable #4). [`Self::factions`] holds
/// only patterns with a complete color triple; a record that names a pattern
/// without colors is in [`Self::records`] and named by a [`PaletteFinding`].
#[derive(Debug)]
pub struct FactionPaletteCatalog {
    install_sha256: ContentHash,
    container_path: String,
    member_sha256: ContentHash,
    member_span: SourceSpan,
    records: Vec<PaletteRecord>,
    factions: Vec<FactionPalette>,
    findings: Vec<PaletteFinding>,
}

impl FactionPaletteCatalog {
    /// Opens `key` in `session`, reads its reader archive, and extracts the
    /// paint palette from [`PALETTE_MEMBER`].
    ///
    /// # Errors
    ///
    /// [`PaletteError::Container`] when the key does not open or is not a
    /// reader archive, [`PaletteError::MissingMember`] when the member is not
    /// declared, [`PaletteError::Member`] when its bytes are not the observed
    /// `.zrd` layout, and [`PaletteError::Span`] when a field span is refused.
    pub fn discover(session: &ContentSession, key: &AssetKey) -> Result<Self, PaletteError> {
        let container = ZbdContainer::open(session, key).map_err(PaletteError::Container)?;
        let mut context = ParseContext::with_defaults(container.label());
        let index = container
            .index(&mut context)
            .map_err(PaletteError::Container)?;
        let table = index.member_table();
        let archive = container
            .reader_archive(&mut context, &index, &table)
            .map_err(PaletteError::Container)?;
        let entry = archive
            .entries()
            .find(|entry| entry.name() == PALETTE_MEMBER.as_bytes())
            .ok_or_else(|| PaletteError::MissingMember {
                member: PALETTE_MEMBER.to_owned(),
            })?;

        let install_sha256 = container.span().install_sha256();
        let container_path = container.path().as_str().to_owned();
        let member_sha256 = sha256(entry.content());
        let member_span = SourceSpan::new(
            install_sha256,
            &container_path,
            Some(PALETTE_MEMBER),
            entry.span().offset,
            entry.span().length,
            Some(member_sha256),
        )
        .map_err(PaletteError::Span)?;

        let root = parse_zrd(entry.content())?;
        let records = vehicle_record_nodes(&root)?;
        let provenance = PaletteProvenance {
            install_sha256,
            container_path: &container_path,
            member_sha256,
            member_offset: entry.span().offset,
        };

        let mut extracted = Vec::new();
        for (name, node) in records {
            if let Some(record) = extract_palette_record(name, node, &provenance)? {
                extracted.push(record);
            }
        }

        // Group paint-bearing records by the pattern they name, in sorted
        // (deterministic) order. A pattern with no complete triple is not a
        // faction palette: it becomes a finding instead.
        let mut by_pattern: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        let mut findings = Vec::new();
        for (position, record) in extracted.iter().enumerate() {
            if !record.has_colors() {
                findings.push(PaletteFinding {
                    code: "pattern_without_colors",
                    record: record.name.clone(),
                    detail: format!(
                        "`{}` names paint pattern `{}` but stores no color triple",
                        record.name, record.pattern
                    ),
                });
                continue;
            }
            by_pattern
                .entry(record.pattern.as_str())
                .or_default()
                .push(position);
        }

        let mut factions = Vec::with_capacity(by_pattern.len());
        for (pattern, members) in by_pattern {
            let first = &extracted[members[0]];
            for other in &members[1..] {
                let other = &extracted[*other];
                if !same_palette_values(first, other) {
                    findings.push(PaletteFinding {
                        code: "inconsistent_pattern_palette",
                        record: other.name.clone(),
                        detail: format!(
                            "`{}` and `{}` name paint pattern `{pattern}` but store different \
                             palettes",
                            first.name, other.name
                        ),
                    });
                }
            }
            factions.push(FactionPalette {
                faction: pattern.to_owned(),
                colors: first.colors.clone(),
                decals: first.decals.clone(),
                records: members
                    .iter()
                    .map(|index| extracted[*index].name.clone())
                    .collect(),
            });
        }

        Ok(Self {
            install_sha256,
            container_path,
            member_sha256,
            member_span,
            records: extracted,
            factions,
            findings,
        })
    }

    /// The installation fingerprint every span was built with.
    pub fn install_sha256(&self) -> ContentHash {
        self.install_sha256
    }

    /// The installation-relative path of the container the member was read
    /// from (`ZBD/zrdr.zbd`).
    pub fn container_path(&self) -> &str {
        &self.container_path
    }

    /// The member the palette was read from.
    pub fn member(&self) -> &str {
        PALETTE_MEMBER
    }

    /// The digest of the whole member bytes.
    pub fn member_sha256(&self) -> ContentHash {
        self.member_sha256
    }

    /// The member's range inside the container.
    pub fn member_span(&self) -> &SourceSpan {
        &self.member_span
    }

    /// Every paint-bearing record, in file order.
    pub fn records(&self) -> &[PaletteRecord] {
        &self.records
    }

    /// The record named `name`, if it carries a paint field.
    pub fn record(&self, name: &str) -> Option<&PaletteRecord> {
        self.records.iter().find(|record| record.name == name)
    }

    /// Every faction with a complete palette, sorted by name.
    pub fn factions(&self) -> &[FactionPalette] {
        &self.factions
    }

    /// The faction names, sorted.
    pub fn faction_names(&self) -> Vec<&str> {
        self.factions
            .iter()
            .map(|faction| faction.faction.as_str())
            .collect()
    }

    /// The palette of `faction`, or [`PaletteRefusal::UnknownFaction`].
    pub fn palette(&self, faction: &str) -> Result<&FactionPalette, PaletteRefusal> {
        self.factions
            .iter()
            .find(|palette| palette.faction == faction)
            .ok_or_else(|| PaletteRefusal::UnknownFaction {
                faction: faction.to_owned(),
            })
    }

    /// Content a paint record carries that no palette uses.
    pub fn findings(&self) -> &[PaletteFinding] {
        &self.findings
    }
}

/// Whether two paint records store the same palette values: the same color
/// channels and the same decal indices. A span is provenance, not identity, so
/// two records that store the same colors at different member offsets still
/// agree; only a value disagreement is an inconsistency.
fn same_palette_values(left: &PaletteRecord, right: &PaletteRecord) -> bool {
    left.colors.len() == right.colors.len()
        && left
            .colors
            .iter()
            .zip(&right.colors)
            .all(|(first, other)| first.rgb == other.rgb)
        && left.decals.len() == right.decals.len()
        && left
            .decals
            .iter()
            .zip(&right.decals)
            .all(|(first, other)| first.index == other.index)
}

/// Builds one [`PaletteRecord`] from a record node, or `None` when the record
/// carries no paint field at all.
fn extract_palette_record(
    name: &str,
    node: &ZrdNode,
    provenance: &PaletteProvenance<'_>,
) -> Result<Option<PaletteRecord>, PaletteError> {
    let fields = zrd_field_pairs(node);
    let field = |key: &str| {
        fields
            .iter()
            .find(|(key_of, _)| *key_of == Some(key))
            .map(|(_, value)| *value)
    };
    let pattern_node = field("paint_pattern");
    let color_nodes: [Option<&ZrdNode>; PALETTE_SLOTS] = [
        field("paint_color1"),
        field("paint_color2"),
        field("paint_color3"),
    ];
    let decal_nodes: [Option<&ZrdNode>; PALETTE_SLOTS] = [
        field("paint_decal1"),
        field("paint_decal2"),
        field("paint_decal3"),
    ];
    let carries_paint = pattern_node.is_some()
        || color_nodes.iter().any(Option::is_some)
        || decal_nodes.iter().any(Option::is_some);
    if !carries_paint {
        return Ok(None);
    }

    let pattern_node = pattern_node.ok_or(PaletteError::Member {
        code: "shape",
        offset: node.offset,
        detail: "a paint record does not name its paint pattern",
    })?;
    let pattern = pattern_node
        .as_list()
        .and_then(|children| children.first())
        .and_then(ZrdNode::as_text)
        .ok_or(PaletteError::Member {
            code: "shape",
            offset: pattern_node.offset,
            detail: "paint_pattern is not a one-text list",
        })?
        .to_owned();
    let pattern_span = provenance.span(pattern_node)?;

    let present = |nodes: &[Option<&ZrdNode>; PALETTE_SLOTS]| {
        nodes.iter().filter(|node| node.is_some()).count()
    };
    let colors = match present(&color_nodes) {
        0 => Vec::new(),
        count if count == PALETTE_SLOTS => {
            let mut colors = Vec::with_capacity(PALETTE_SLOTS);
            for color_node in color_nodes {
                colors.push(palette_color(
                    color_node.expect("all three present"),
                    provenance,
                )?);
            }
            colors
        }
        _ => {
            return Err(PaletteError::Member {
                code: "shape",
                offset: node.offset,
                detail: "a paint record has an incomplete color triple",
            });
        }
    };
    let decals = match present(&decal_nodes) {
        0 => Vec::new(),
        count if count == PALETTE_SLOTS => {
            let mut decals = Vec::with_capacity(PALETTE_SLOTS);
            for decal_node in decal_nodes {
                decals.push(palette_decal(
                    decal_node.expect("all three present"),
                    provenance,
                )?);
            }
            decals
        }
        _ => {
            return Err(PaletteError::Member {
                code: "shape",
                offset: node.offset,
                detail: "a paint record has an incomplete decal triple",
            });
        }
    };

    Ok(Some(PaletteRecord {
        name: name.to_owned(),
        pattern,
        pattern_span,
        colors,
        decals,
        span: provenance.span(node)?,
    }))
}

/// Decodes one `paint_colorN` value node: a three-int list with 8-bit channels.
fn palette_color(
    node: &ZrdNode,
    provenance: &PaletteProvenance<'_>,
) -> Result<PaletteColor, PaletteError> {
    let channels = node
        .as_list()
        .filter(|children| children.len() == PALETTE_SLOTS)
        .ok_or(PaletteError::Member {
            code: "shape",
            offset: node.offset,
            detail: "paint_color is not a three-int list",
        })?;
    let mut rgb = [0u8; 3];
    for (slot, channel) in channels.iter().enumerate() {
        let value = channel.as_int().ok_or(PaletteError::Member {
            code: "shape",
            offset: channel.offset,
            detail: "a paint color channel is not an int",
        })?;
        rgb[slot] = u8::try_from(value).map_err(|_| PaletteError::Member {
            code: "shape",
            offset: channel.offset,
            detail: "a paint color channel is outside 0..=255",
        })?;
    }
    Ok(PaletteColor {
        rgb,
        span: provenance.span(node)?,
    })
}

/// Decodes one `paint_decalN` value node: a one-int list.
fn palette_decal(
    node: &ZrdNode,
    provenance: &PaletteProvenance<'_>,
) -> Result<PaletteDecal, PaletteError> {
    let index = node
        .as_list()
        .and_then(|children| children.first())
        .and_then(ZrdNode::as_int)
        .ok_or(PaletteError::Member {
            code: "shape",
            offset: node.offset,
            detail: "paint_decal is not a one-int list",
        })?;
    Ok(PaletteDecal {
        index,
        span: provenance.span(node)?,
    })
}

// ------------------------------------------------------ F09-PAINTSHOP ---

/// The ROF container the paint shop's readable original data lives in.
pub const PAINT_SHOP_CONTAINER: &str = "GOSDATA/ASSETS/crimson.rof";

/// The configuration member that declares the paint-shop controls. It is the
/// only readable member that names the paint shop's option space: the
/// `[@Paint@]` section declares every paint-shop control, and none of those
/// records carries a colour, a shade or a decal value.
pub const PAINT_SHOP_LAYOUT: &str = "ASSETS/LAYOUT.CSV";

/// The `LAYOUT.CSV` section the paint shop owns, named the way the production
/// keyed-list reader reports a section header: the bytes between the brackets,
/// verbatim, so `[@Paint@]` in the member is `@Paint@` here.
pub const PAINT_SHOP_SECTION: &[u8] = b"@Paint@";

/// The same section as the member spells it, for messages and findings.
const PAINT_SHOP_SECTION_SPELLED: &str = "[@Paint@]";

/// The pane record that declares the paint shop's decal sheet.
pub const PAINT_SHOP_DECAL_PANE: &str = "PT_P_DECALS";

/// How many paint slots (mask planes) the shop offers a dropdown for.
pub const PAINT_SHOP_SLOTS: u32 = 3;

/// Field positions inside a `D` (dropdown) `LAYOUT.CSV` record, numbered as
/// [`RecordSchema::Dropdown`] numbers them, `0` being the record letter.
const DROPDOWN_TOTAL_DISPLAYED: usize = 11;

/// How many fields a `D` record has in the shipped data.
const DROPDOWN_FIELDS: usize = 12;

/// Field positions inside a `P` (pane) record.
const PANE_ART: usize = 1;
const PANE_NUM_FRAMES: usize = 5;

/// How many fields a `P` record has in the shipped data.
const PANE_FIELDS: usize = 9;

/// The gap identifier of the swatch palette.
const SWATCH_GAP: &str = "swatch_palette_engine_internal";
/// The gap identifier of the shade table.
const SHADE_GAP: &str = "shade_table_engine_internal";
/// The gap identifier of the paint-pattern display names.
const PATTERN_NAME_GAP: &str = "pattern_names_engine_internal";
/// The gap identifier of the decal values.
const DECAL_GAP: &str = "decal_value_engine_internal";

/// Which paint-shop dropdown a control is.
///
/// The four roles are the original record-name stems themselves
/// ([`PAINT_SHOP_STEMS`]); nothing here invents a classification, and a stem the
/// original data does not spell has no role at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PaintShopRole {
    /// The paint pattern dropdown (`PT_D_PATTERN`).
    Pattern,
    /// The colour dropdown of one paint slot (`PT_D_COLORS0..2`).
    Color,
    /// The shade dropdown of one paint slot (`PT_D_SHADES0..2`).
    Shade,
    /// The decal dropdown of one paint slot (`PT_D_DECALS0..2`).
    Decal,
}

impl PaintShopRole {
    /// Stable lowercase identifier.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Pattern => "pattern",
            Self::Color => "color",
            Self::Shade => "shade",
            Self::Decal => "decal",
        }
    }

    /// The role a `[@Paint@]` record-name stem declares, compared the way the
    /// layout keys are compared (ASCII case-insensitively), or `None` for a
    /// stem the paint shop does not own.
    pub fn from_layout_stem(stem: &str) -> Option<Self> {
        PAINT_SHOP_STEMS
            .iter()
            .find(|(_, name)| name.eq_ignore_ascii_case(stem))
            .map(|(role, _)| *role)
    }

    /// How many slots this role offers a dropdown for: none for the pattern
    /// list, `PAINT_SHOP_SLOTS` for the three per-slot lists.
    pub fn slots(self) -> Option<u32> {
        match self {
            Self::Pattern => None,
            Self::Color | Self::Shade | Self::Decal => Some(PAINT_SHOP_SLOTS),
        }
    }
}

/// The `[@Paint@]` record-name stem each role is declared by, as the original
/// data spells it. A per-slot control appends its slot as the last character of
/// the record key (`PT_D_COLORS0`), which is how [`PaintShopCatalog::control`]
/// finds the three colour, shade and decal lists.
pub const PAINT_SHOP_STEMS: [(PaintShopRole, &str); 4] = [
    (PaintShopRole::Pattern, "PT_D_PATTERN"),
    (PaintShopRole::Color, "PT_D_COLORS"),
    (PaintShopRole::Shade, "PT_D_SHADES"),
    (PaintShopRole::Decal, "PT_D_DECALS"),
];

/// How the fields of one paint-shop record spell themselves.
///
/// This is the measurable part of the engine-internal claim. A dropdown that
/// stored its entries would spell them: a colour or a shade as an eight-hex-digit
/// `0x…` literal, a display name as text. The shipped paint shop spells **none**
/// of the three — its controls spell the record letter, `<NAME>` references to
/// other records and whole numbers, which is why [`PaintShopValue`] has no
/// variant that could carry a value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PaintShopFieldCensus {
    /// Fields with no bytes.
    pub empty: u32,
    /// `<NAME>` references to another record or a global name.
    pub placeholder: u32,
    /// Decimal whole numbers, signed or not.
    pub integer: u32,
    /// `0x…` values that are not an eight-hex-digit colour.
    pub hex: u32,
    /// Eight-hex-digit `0x…` colour literals.
    pub colour: u32,
    /// Anything else, including the one-letter record kind.
    pub text: u32,
}

impl PaintShopFieldCensus {
    fn count(&mut self, spelling: FieldSpelling) {
        match spelling {
            FieldSpelling::Empty => self.empty += 1,
            FieldSpelling::Placeholder => self.placeholder += 1,
            FieldSpelling::Integer => self.integer += 1,
            FieldSpelling::Hex => self.hex += 1,
            FieldSpelling::Color => self.colour += 1,
            FieldSpelling::Text => self.text += 1,
        }
    }

    /// How many fields the record has.
    pub fn total(&self) -> u32 {
        self.empty + self.placeholder + self.integer + self.hex + self.colour + self.text
    }

    /// How many fields carry a literal value rather than a reference or a
    /// whole number: the colour literals plus everything spelled as text. The
    /// record kind letter is one of the text fields, so this is at least one
    /// for every record the production reader recognizes.
    pub fn literals(&self) -> u32 {
        self.colour + self.text
    }
}

/// One paint-shop dropdown as the layout declares it: which record declared it,
/// which paint slot it belongs to, how many entries it displays, and — the point
/// of this stage — how its fields spell themselves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaintShopControl {
    key: String,
    role: PaintShopRole,
    slot: Option<u32>,
    displayed: u32,
    census: PaintShopFieldCensus,
    line: u64,
    span: SourceSpan,
}

impl PaintShopControl {
    /// The `LAYOUT.CSV` record key, as the original spells it
    /// (`PT_D_COLORS0`).
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Which paint-shop list this control is.
    pub fn role(&self) -> PaintShopRole {
        self.role
    }

    /// The paint slot, or `None` for the pattern list.
    pub fn slot(&self) -> Option<u32> {
        self.slot
    }

    /// How many entries the control declares it displays — the count the shop
    /// offers, read from the record's `TotalDisplayed` field. It is a count:
    /// the record names no entry.
    pub fn displayed_entries(&self) -> u32 {
        self.displayed
    }

    /// How the record's fields spell themselves. The shipped paint shop
    /// declares `colour: 0` and one text field (the record letter) for each of
    /// its ten controls.
    pub fn field_census(&self) -> PaintShopFieldCensus {
        self.census
    }

    /// The 1-based line the record sits on inside [`PAINT_SHOP_LAYOUT`].
    pub fn line(&self) -> u64 {
        self.line
    }

    /// The member's own container-absolute span. The layout member is stored
    /// compressed, so a line inside it has no container-absolute byte range;
    /// the line number and this member span are the provenance, and neither is
    /// invented.
    pub fn span(&self) -> &SourceSpan {
        &self.span
    }
}

/// The decal sheet the paint shop draws its decals from, as the layout declares
/// it: the art member and how many frames of it the pane record holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaintShopDecalSheet {
    key: String,
    art: String,
    frames: u32,
    line: u64,
    span: SourceSpan,
}

impl PaintShopDecalSheet {
    /// The pane record key (`PT_P_DECALS`).
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The art member the record names, exactly as it spells it.
    pub fn art(&self) -> &str {
        &self.art
    }

    /// The frame count the pane record declares.
    pub fn frames(&self) -> u32 {
        self.frames
    }

    /// The 1-based line the record sits on inside [`PAINT_SHOP_LAYOUT`].
    pub fn line(&self) -> u64 {
        self.line
    }

    /// The member's own container-absolute span.
    pub fn span(&self) -> &SourceSpan {
        &self.span
    }
}

/// A paint-shop value the readable original data does not store, with the count
/// and field census that establishes the absence and the content it affects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaintShopGap {
    code: &'static str,
    quantity: &'static str,
    displayed: u32,
    census: PaintShopFieldCensus,
    detail: &'static str,
    affected: &'static str,
}

impl PaintShopGap {
    /// Stable lowercase identifier.
    pub fn code(&self) -> &'static str {
        self.code
    }

    /// Which quantity is missing, in the original data's own terms.
    pub fn quantity(&self) -> &'static str {
        self.quantity
    }

    /// How many entries the control declares, as the evidence for the count.
    pub fn displayed_entries(&self) -> u32 {
        self.displayed
    }

    /// How the declaring record's fields spell themselves, as the evidence that
    /// it stores no value.
    pub fn field_census(&self) -> PaintShopFieldCensus {
        self.census
    }

    /// Why the value is not readable.
    pub fn detail(&self) -> &'static str {
        self.detail
    }

    /// The original content the gap affects.
    pub fn affected(&self) -> &'static str {
        self.affected
    }
}

/// A paint-shop value, as the readable original data can describe it.
///
/// There is deliberately **no variant that carries a colour, a shade, a decal or
/// a display name**: none of the members the paint shop reads stores one, so
/// this type cannot be given such a value at all. A missing value is
/// [`PaintShopValue::EngineInternal`], never a fabricated RGB triple and never a
/// silent zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaintShopValue {
    /// The control declares `displayed` entries and stores no value for any of
    /// them, so the entries are produced inside the engine image. `code` is the
    /// [`PaintShopGap`] this value belongs to.
    EngineInternal {
        /// The gap's stable identifier.
        code: &'static str,
        /// How many entries the control declares.
        displayed: u32,
        /// How the declaring record's fields spell themselves.
        census: PaintShopFieldCensus,
    },
    /// The paint shop offers no such entry: the control declares `displayed`
    /// entries and `index` is at or past them.
    NotOffered {
        /// How many entries the control declares.
        displayed: u32,
        /// The entry that was asked for.
        index: u32,
    },
}

/// Why a paint-shop query could not be answered at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaintShopRefusal {
    /// The layout declares no control for this role and slot.
    NoControl {
        /// The role that was asked for.
        role: PaintShopRole,
        /// The slot that was asked for.
        slot: Option<u32>,
    },
}

impl fmt::Display for PaintShopRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoControl { role, slot } => write!(
                f,
                "the paint shop declares no {:?} control for slot {slot:?}",
                role.code()
            ),
        }
    }
}

impl std::error::Error for PaintShopRefusal {}

/// Why the paint-shop option space could not be read.
#[derive(Debug)]
pub enum PaintShopError {
    /// The layout member did not resolve in the session.
    Resolve(ResolveError),
    /// The archive could not hand the member's bytes over.
    Archive(RofReadError),
    /// The member is not the keyed list the dialect inventory routes it as, or
    /// its bytes were refused.
    Config(ConfigError),
    /// A [`SourceSpan`] was refused.
    Span(SourceSpanError),
    /// The member name is not a usable asset key.
    UnknownMember {
        /// The member that was asked for.
        member: String,
    },
    /// The session resolved the member out of a container the archive is not.
    ForeignArchive {
        /// The container the session resolved through.
        resolved: String,
        /// The archive the bytes would have come from.
        archive: String,
    },
    /// The member declares no `[@Paint@]` section.
    MissingSection,
    /// A control's record has fewer fields than its record kind declares.
    ShortRecord {
        /// The record key.
        key: String,
        /// The line it sits on.
        line: u64,
        /// How many fields it has.
        fields: usize,
        /// How many its record kind declares.
        expected: usize,
    },
    /// A control's record carries a trailing digit that is not a paint slot.
    UnknownSlot {
        /// The record key.
        key: String,
        /// The line it sits on.
        line: u64,
    },
    /// A control's entry-count field is not a plain decimal whole number, so
    /// the count is unknown rather than defaulted.
    EntriesUnreadable {
        /// The record key.
        key: String,
        /// The line it sits on.
        line: u64,
    },
    /// A control's record is not of the record kind its role requires.
    WrongKind {
        /// The record key.
        key: String,
        /// The line it sits on.
        line: u64,
        /// The role that asked for it.
        role: PaintShopRole,
    },
    /// The decal sheet's frame count is not a plain decimal whole number.
    FramesUnreadable {
        /// The record key.
        key: String,
        /// The line it sits on.
        line: u64,
    },
}

impl PaintShopError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Resolve(error) => match error {
                ResolveError::NotFound { .. } => "not_found",
                ResolveError::Ambiguous { .. } => "ambiguous",
                ResolveError::UnmeasuredOrder { .. } => "unmeasured_order",
            },
            Self::Archive(error) => error.code(),
            Self::Config(error) => error.code(),
            Self::Span(_) => "invalid_span",
            Self::UnknownMember { .. } => "unknown_member",
            Self::ForeignArchive { .. } => "foreign_archive",
            Self::MissingSection => "missing_section",
            Self::ShortRecord { .. } => "short_record",
            Self::UnknownSlot { .. } => "unknown_slot",
            Self::EntriesUnreadable { .. } => "entries_unreadable",
            Self::WrongKind { .. } => "wrong_kind",
            Self::FramesUnreadable { .. } => "frames_unreadable",
        }
    }
}

impl fmt::Display for PaintShopError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Resolve(error) => write!(f, "{error}"),
            Self::Archive(error) => write!(f, "{error}"),
            Self::Config(error) => write!(f, "{error}"),
            Self::Span(error) => write!(f, "{error}"),
            Self::UnknownMember { member } => {
                write!(f, "{member:?} is not a usable member key")
            }
            Self::ForeignArchive { resolved, archive } => write!(
                f,
                "the member resolves through {resolved:?} but the archive is {archive:?}"
            ),
            Self::MissingSection => write!(
                f,
                "{} declares no {} section",
                PAINT_SHOP_LAYOUT, PAINT_SHOP_SECTION_SPELLED
            ),
            Self::ShortRecord {
                key,
                line,
                fields,
                expected,
            } => write!(
                f,
                "`{key}` (line {line}) has {fields} fields; its record kind declares {expected}"
            ),
            Self::UnknownSlot { key, line } => write!(
                f,
                "`{key}` (line {line}) does not end in a paint slot below {PAINT_SHOP_SLOTS}"
            ),
            Self::EntriesUnreadable { key, line } => write!(
                f,
                "`{key}` (line {line}) does not spell its entry count as a whole number"
            ),
            Self::WrongKind { key, line, role } => write!(
                f,
                "`{key}` (line {line}) is not the {:?} control's record kind",
                role.code()
            ),
            Self::FramesUnreadable { key, line } => write!(
                f,
                "`{key}` (line {line}) does not spell its frame count as a whole number"
            ),
        }
    }
}

impl std::error::Error for PaintShopError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Resolve(error) => Some(error),
            Self::Archive(error) => Some(error),
            Self::Config(error) => Some(error),
            Self::Span(error) => Some(error),
            Self::UnknownMember { .. }
            | Self::ForeignArchive { .. }
            | Self::MissingSection
            | Self::ShortRecord { .. }
            | Self::UnknownSlot { .. }
            | Self::EntriesUnreadable { .. }
            | Self::WrongKind { .. }
            | Self::FramesUnreadable { .. } => None,
        }
    }
}

impl From<ResolveError> for PaintShopError {
    fn from(error: ResolveError) -> Self {
        Self::Resolve(error)
    }
}

impl From<RofReadError> for PaintShopError {
    fn from(error: RofReadError) -> Self {
        Self::Archive(error)
    }
}

impl From<ConfigError> for PaintShopError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<SourceSpanError> for PaintShopError {
    fn from(error: SourceSpanError) -> Self {
        Self::Span(error)
    }
}

/// The paint shop's option space, as the original layout declares it, and every
/// value it does not declare.
///
/// [`Self::discover`] reads `ASSETS/LAYOUT.CSV`'s `[@Paint@]` section through
/// the production keyed-list reader. What it yields:
///
/// * [`Self::controls`]: the ten paint-shop dropdowns — one pattern list and
///   three lists each for colours, shades and decals — with the entry count each
///   declares and the number of colour-valued fields its record has (measured
///   `0` for all ten);
/// * [`Self::decal_sheet`]: the art member and frame count the decal pane
///   declares;
/// * [`Self::gaps`]: the values no readable member stores, each with the count
///   and the colour-field census that establishes the absence.
///
/// The swatch palette, the shade table and the pattern display names are
/// **engine-internal**: they are produced by native callbacks inside the engine
/// image, and the section that declares the shop's controls has no field at all
/// in which a value could be written. No colour is ever synthesised here
/// (spec F09 non-negotiable #4).
#[derive(Debug)]
pub struct PaintShopCatalog {
    install_sha256: ContentHash,
    container_path: String,
    member: String,
    layout: SourceSpan,
    decoded: SourceSpan,
    stored_len: u64,
    trailing_bytes: u64,
    controls: Vec<PaintShopControl>,
    decal_sheet: Option<PaintShopDecalSheet>,
    gaps: Vec<PaintShopGap>,
}

impl PaintShopCatalog {
    /// Reads the paint-shop section of `member` out of the archive `source`.
    ///
    /// `session` is what resolves the member, so the span the catalog carries is
    /// the production one — installation fingerprint, container, member key,
    /// stored extent and digest — while the bytes come from the archive reader
    /// that owns them (a mounted archive declares its members but has no host
    /// bytes behind it, so the session cannot serve them). A member the session
    /// resolves out of a **different** container than `source` is refused rather
    /// than read from the wrong archive.
    ///
    /// # Errors
    ///
    /// [`PaintShopError::Resolve`] when the member does not resolve,
    /// [`PaintShopError::ForeignArchive`] when the resolution and the archive
    /// name different containers, [`PaintShopError::Archive`] when the archive
    /// cannot hand the member's bytes over, [`PaintShopError::Config`] when it
    /// is not the keyed list the dialect inventory routes it as,
    /// [`PaintShopError::MissingSection`] when it declares no `[@Paint@]`
    /// section, [`PaintShopError::WrongKind`] /
    /// [`PaintShopError::ShortRecord`] / [`PaintShopError::UnknownSlot`] /
    /// [`PaintShopError::EntriesUnreadable`] when a paint-shop control's record
    /// is not the shape the role requires, and [`PaintShopError::Span`] when a
    /// span is refused.
    pub fn discover(
        session: &ContentSession,
        source: &RofSource,
        member: &str,
    ) -> Result<Self, PaintShopError> {
        let key = AssetKey::from_spelling(source.namespace().as_str(), member, "default").map_err(
            |_| PaintShopError::UnknownMember {
                member: member.to_owned(),
            },
        )?;
        let asset = session.resolve(&key)?;
        let layout = asset.resolved().span.clone();
        if !layout
            .container_path()
            .eq_ignore_ascii_case(source.container())
        {
            return Err(PaintShopError::ForeignArchive {
                resolved: layout.container_path().to_owned(),
                archive: source.container().to_owned(),
            });
        }
        let info = source
            .member(&key)
            .ok_or_else(|| PaintShopError::UnknownMember {
                member: member.to_owned(),
            })?
            .clone();
        let read = source.read(&key)?;
        let bytes = read.data.as_slice();
        // A compressed member's stored and decoded extents differ, and a span
        // describes one of them. The document is therefore read against the
        // **decoded** extent's own span, whose digest is the digest of exactly
        // the decoded bytes, while [`Self::layout_span`] keeps the resolution's
        // span: the member's stored extent, the bytes the container physically
        // holds. Both are recorded; neither is derived from the other.
        let decoded = SourceSpan::new(
            layout.install_sha256(),
            layout.container_path(),
            layout.member_key(),
            info.offset,
            info.declared_decoded_len,
            Some(sha256(bytes)),
        )?;
        let mut context = ParseContext::with_defaults(layout.container_path());
        let mut document = ConfigDocument::read(&mut context, decoded.clone(), bytes)?;
        if !document
            .entries()
            .any(|entry| entry.section.as_deref() == Some(PAINT_SHOP_SECTION))
        {
            return Err(PaintShopError::MissingSection);
        }

        let mut controls = Vec::new();
        for entry in document.entries() {
            let Some((role, stem)) = paint_shop_stem(entry) else {
                continue;
            };
            controls.push(paint_shop_control(entry, role, stem, decoded.clone())?);
        }
        // Sorted by role then slot, so the catalog's order does not depend on
        // how the member's lines happen to be ordered.
        controls.sort_by(|left, right| {
            (left.role, left.slot)
                .cmp(&(right.role, right.slot))
                .then_with(|| left.key.cmp(&right.key))
        });

        let decal_sheet =
            match document.lookup(Some(PAINT_SHOP_SECTION), PAINT_SHOP_DECAL_PANE.as_bytes()) {
                Lookup::Found(entry) => Some(paint_shop_decal_sheet(entry, decoded.clone())?),
                Lookup::Missing | Lookup::Ambiguous(_) => None,
            };

        let gaps = paint_shop_gaps(&controls, decal_sheet.as_ref());

        Ok(Self {
            install_sha256: layout.install_sha256(),
            container_path: layout.container_path().to_owned(),
            member: layout.member_key().unwrap_or(PAINT_SHOP_LAYOUT).to_owned(),
            layout,
            decoded,
            stored_len: read.stored_len,
            trailing_bytes: read.trailing_len,
            controls,
            decal_sheet,
            gaps,
        })
    }

    /// The installation fingerprint every span was built with.
    pub fn install_sha256(&self) -> ContentHash {
        self.install_sha256
    }

    /// The container the layout member was read from.
    pub fn container_path(&self) -> &str {
        &self.container_path
    }

    /// The member the paint-shop section was read from.
    pub fn member(&self) -> &str {
        &self.member
    }

    /// The layout member's container-absolute span as the session resolved it:
    /// the member's **stored** extent, the bytes the container physically holds,
    /// with their digest.
    pub fn layout_span(&self) -> &SourceSpan {
        &self.layout
    }

    /// The container-absolute span of the member's **decoded** extent — the bytes
    /// the keyed-list document was read from, with the digest of exactly those
    /// bytes. It differs from [`Self::layout_span`] for a compressed member and
    /// is equal to it for an uncompressed one.
    pub fn decoded_span(&self) -> &SourceSpan {
        &self.decoded
    }

    /// How many bytes of the member's stored extent the decoder did not
    /// consume. Zero for every member of the original installation; a nonzero
    /// count is reported, never skipped.
    pub fn trailing_bytes(&self) -> u64 {
        self.trailing_bytes
    }

    /// How many bytes the member's stored extent occupies in the container.
    pub fn stored_len(&self) -> u64 {
        self.stored_len
    }

    /// Every paint-shop control, sorted by role then slot.
    pub fn controls(&self) -> &[PaintShopControl] {
        &self.controls
    }

    /// The control for `role` and `slot`, or `None` when the layout declares
    /// none. [`PaintShopRole::slots`] is the authority on which slots a role may
    /// be asked for.
    pub fn control(&self, role: PaintShopRole, slot: Option<u32>) -> Option<&PaintShopControl> {
        self.controls
            .iter()
            .find(|control| control.role == role && control.slot == slot)
    }

    /// The decal sheet the pane record declares, or `None` when the layout
    /// declares no such record.
    pub fn decal_sheet(&self) -> Option<&PaintShopDecalSheet> {
        self.decal_sheet.as_ref()
    }

    /// Every paint-shop value no readable original member stores.
    pub fn gaps(&self) -> &[PaintShopGap] {
        &self.gaps
    }

    /// The gap the quantity `role`/`slot` belongs to, when the gap was recorded.
    pub fn gap(&self, code: &str) -> Option<&PaintShopGap> {
        self.gaps.iter().find(|gap| gap.code == code)
    }

    /// The colour swatch `index` of paint slot `slot`.
    ///
    /// # Errors
    ///
    /// [`PaintShopRefusal::NoControl`] when the layout declares no colour
    /// control for `slot`.
    pub fn color(&self, slot: u32, index: u32) -> Result<PaintShopValue, PaintShopRefusal> {
        self.value(PaintShopRole::Color, Some(slot), index, SWATCH_GAP)
    }

    /// The shade swatch `index` of paint slot `slot`.
    ///
    /// # Errors
    ///
    /// [`PaintShopRefusal::NoControl`] when the layout declares no shade control
    /// for `slot`.
    pub fn shade(&self, slot: u32, index: u32) -> Result<PaintShopValue, PaintShopRefusal> {
        self.value(PaintShopRole::Shade, Some(slot), index, SHADE_GAP)
    }

    /// The display name of paint pattern `index`.
    ///
    /// # Errors
    ///
    /// [`PaintShopRefusal::NoControl`] when the layout declares no pattern
    /// control at all.
    pub fn pattern_name(&self, index: u32) -> Result<PaintShopValue, PaintShopRefusal> {
        self.value(PaintShopRole::Pattern, None, index, PATTERN_NAME_GAP)
    }

    /// The decal `index` of paint slot `slot`.
    ///
    /// # Errors
    ///
    /// [`PaintShopRefusal::NoControl`] when the layout declares no decal control
    /// for `slot`.
    pub fn decal(&self, slot: u32, index: u32) -> Result<PaintShopValue, PaintShopRefusal> {
        self.value(PaintShopRole::Decal, Some(slot), index, DECAL_GAP)
    }

    fn value(
        &self,
        role: PaintShopRole,
        slot: Option<u32>,
        index: u32,
        gap: &'static str,
    ) -> Result<PaintShopValue, PaintShopRefusal> {
        let control = self
            .control(role, slot)
            .ok_or(PaintShopRefusal::NoControl { role, slot })?;
        if index >= control.displayed_entries() {
            return Ok(PaintShopValue::NotOffered {
                displayed: control.displayed_entries(),
                index,
            });
        }
        Ok(PaintShopValue::EngineInternal {
            code: gap,
            displayed: control.displayed_entries(),
            census: control.field_census(),
        })
    }

    /// What the shop's declared option space and the vehicle records' stored
    /// palettes say about each other.
    ///
    /// Every disagreement is a finding, never a repaired value: a pattern count
    /// that does not match, a stored paint pattern the shop does not offer, a
    /// pattern whose records store no palette, a stored decal index outside the
    /// declared sheet, a control that does carry a value field after all, and a
    /// paint shop that declares no decal sheet at all.
    pub fn cross_check(&self, palette: &FactionPaletteCatalog) -> Vec<PaintShopFinding> {
        let mut findings = Vec::new();

        let stored: BTreeMap<&str, usize> = palette.records().iter().fold(
            BTreeMap::new(),
            |mut table: BTreeMap<&str, usize>, record| {
                *table.entry(record.pattern()).or_default() += 1;
                table
            },
        );
        match self.control(PaintShopRole::Pattern, None) {
            None => findings.push(PaintShopFinding::no_pattern_control()),
            Some(control) if control.displayed_entries() as usize != stored.len() => {
                findings.push(PaintShopFinding::pattern_count_mismatch(
                    control.key(),
                    control.displayed_entries(),
                    stored.len(),
                ));
            }
            Some(_) => {}
        }

        // A pattern the vehicle records name but no record colours: the player's
        // own paint is chosen in the shop, so it stays a gap, never a palette.
        for pattern in stored.keys() {
            if !palette
                .records()
                .iter()
                .any(|record| record.pattern() == *pattern && record.has_colors())
            {
                findings.push(PaintShopFinding::pattern_without_palette(pattern));
            }
        }

        // A stored decal index outside the declared sheet cannot be drawn from
        // the sheet the layout names.
        let frames = self.decal_sheet.as_ref().map(PaintShopDecalSheet::frames);
        for record in palette.records() {
            for decal in record.decals() {
                if let Some(frames) = frames
                    && decal.index() >= frames
                {
                    findings.push(PaintShopFinding::decal_outside_sheet(
                        record.name(),
                        decal.index(),
                        frames,
                    ));
                }
            }
        }
        if self.decal_sheet.is_none() {
            findings.push(PaintShopFinding::no_decal_sheet());
        }

        for control in &self.controls {
            if control.field_census().colour > 0 {
                findings.push(PaintShopFinding::control_carries_value(
                    control.key(),
                    control.field_census().colour,
                ));
            }
        }

        findings
    }
}

/// A disagreement between the paint shop's declared option space and the vehicle
/// records' stored palettes, kept rather than repaired.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaintShopFinding {
    code: &'static str,
    detail: String,
}

impl PaintShopFinding {
    fn no_pattern_control() -> Self {
        Self {
            code: "no_pattern_control",
            detail: format!(
                "the layout declares no {} record, so no paint pattern is offered",
                PAINT_SHOP_STEMS
                    .iter()
                    .find(|(role, _)| *role == PaintShopRole::Pattern)
                    .map(|(_, stem)| *stem)
                    .unwrap_or("the pattern control")
            ),
        }
    }

    fn pattern_count_mismatch(key: &str, declared: u32, stored: usize) -> Self {
        Self {
            code: "pattern_count_mismatch",
            detail: format!(
                "`{key}` declares {declared} entries and the vehicle records name {stored} paint \
                 patterns"
            ),
        }
    }

    fn pattern_without_palette(pattern: &str) -> Self {
        Self {
            code: "pattern_without_palette",
            detail: format!(
                "paint pattern `{pattern}` is named by vehicle records that store no colour or \
                 decal triple"
            ),
        }
    }

    fn decal_outside_sheet(record: &str, index: u32, frames: u32) -> Self {
        Self {
            code: "decal_outside_sheet",
            detail: format!(
                "vehicle record `{record}` stores decal {index}, outside the {frames} frames the \
                 decal pane declares"
            ),
        }
    }

    fn no_decal_sheet() -> Self {
        Self {
            code: "no_decal_sheet",
            detail: format!(
                "the layout declares no `{PAINT_SHOP_DECAL_PANE}` record, so the decal sheet is \
                 unknown"
            ),
        }
    }

    fn control_carries_value(key: &str, colour_fields: u32) -> Self {
        Self {
            code: "control_carries_value",
            detail: format!(
                "`{key}` has {colour_fields} colour-valued fields, so the shop's option space is \
                 not engine-internal after all and the recorded gaps must be re-derived"
            ),
        }
    }

    /// Stable lowercase identifier.
    pub fn code(&self) -> &'static str {
        self.code
    }

    /// Human-readable detail naming the records and counts involved.
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

/// The role and record-name stem a `[@Paint@]` entry declares, or `None` for an
/// entry of another section or another control family.
fn paint_shop_stem(entry: &ConfigEntry) -> Option<(PaintShopRole, &str)> {
    if entry.section.as_deref() != Some(PAINT_SHOP_SECTION) {
        return None;
    }
    let key = std::str::from_utf8(&entry.key).ok()?;
    let stem = key.trim_end_matches(|character: char| character.is_ascii_digit());
    let role = PaintShopRole::from_layout_stem(stem)?;
    Some((role, stem))
}

/// The slot digit a per-slot control key ends in, or `None` when the key names
/// the un-slot list.
fn paint_shop_slot(key: &str) -> Option<u32> {
    let digit = key
        .len()
        .checked_sub(1)
        .and_then(|tail| key.get(tail..))
        .and_then(|tail| tail.parse::<u32>().ok())?;
    (digit < PAINT_SHOP_SLOTS).then_some(digit)
}

/// Reads one paint-shop dropdown record.
fn paint_shop_control(
    entry: &ConfigEntry,
    role: PaintShopRole,
    stem: &str,
    span: SourceSpan,
) -> Result<PaintShopControl, PaintShopError> {
    let key = String::from_utf8_lossy(&entry.key).into_owned();
    let fields = match &entry.value {
        RawValue::Fields(fields) => fields,
        RawValue::Unsplit { .. } => {
            return Err(PaintShopError::ShortRecord {
                key,
                line: entry.line,
                fields: 0,
                expected: DROPDOWN_FIELDS,
            });
        }
    };
    if fields.len() != DROPDOWN_FIELDS {
        return Err(PaintShopError::ShortRecord {
            key,
            line: entry.line,
            fields: fields.len(),
            expected: DROPDOWN_FIELDS,
        });
    }
    let view = RecordView::for_entry(entry).ok_or_else(|| PaintShopError::WrongKind {
        key: key.clone(),
        line: entry.line,
        role,
    })?;
    // The record must really be a `D` record, or its entry-count field is not
    // the field this stage reads.
    if view.schema() != RecordSchema::Layout(RecordKind::Dropdown) {
        return Err(PaintShopError::WrongKind {
            key,
            line: entry.line,
            role,
        });
    }
    let displayed = whole_number(fields[DROPDOWN_TOTAL_DISPLAYED].value()).ok_or_else(|| {
        PaintShopError::EntriesUnreadable {
            key: key.clone(),
            line: entry.line,
        }
    })?;
    let slot = match role.slots() {
        Some(_) => Some(
            paint_shop_slot(key.strip_prefix(stem).unwrap_or_default()).ok_or_else(|| {
                PaintShopError::UnknownSlot {
                    key: key.clone(),
                    line: entry.line,
                }
            })?,
        ),
        None => None,
    };
    let mut census = PaintShopFieldCensus::default();
    for field in view.fields() {
        census.count(field.spelling);
    }
    Ok(PaintShopControl {
        key,
        role,
        slot,
        displayed,
        census,
        line: entry.line,
        span,
    })
}

/// Reads the paint shop's decal pane record.
fn paint_shop_decal_sheet(
    entry: &ConfigEntry,
    span: SourceSpan,
) -> Result<PaintShopDecalSheet, PaintShopError> {
    let key = String::from_utf8_lossy(&entry.key).into_owned();
    let fields = match &entry.value {
        RawValue::Fields(fields) => fields,
        RawValue::Unsplit { .. } => {
            return Err(PaintShopError::ShortRecord {
                key,
                line: entry.line,
                fields: 0,
                expected: PANE_FIELDS,
            });
        }
    };
    if fields.len() != PANE_FIELDS {
        return Err(PaintShopError::ShortRecord {
            key,
            line: entry.line,
            fields: fields.len(),
            expected: PANE_FIELDS,
        });
    }
    let art = std::str::from_utf8(fields[PANE_ART].value())
        .map_err(|_| PaintShopError::ShortRecord {
            key: key.clone(),
            line: entry.line,
            fields: fields.len(),
            expected: PANE_FIELDS,
        })?
        .to_owned();
    let frames = whole_number(fields[PANE_NUM_FRAMES].value()).ok_or_else(|| {
        PaintShopError::FramesUnreadable {
            key: key.clone(),
            line: entry.line,
        }
    })?;
    Ok(PaintShopDecalSheet {
        key,
        art,
        frames,
        line: entry.line,
        span,
    })
}

/// The plain decimal whole number `bytes` spells, or `None` for anything else:
/// no sign, no `0x` prefix, no blank bytes and at least one digit. A field that
/// does not spell a count this way is unknown, never defaulted to zero.
fn whole_number(bytes: &[u8]) -> Option<u32> {
    let text = std::str::from_utf8(bytes).ok()?;
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// Builds the recorded gaps from what the layout actually declares: a gap is
/// emitted for a value the layout's own controls store no value for, and a
/// control that does carry a colour literal stops its gap (the cross-check
/// reports that control instead).
fn paint_shop_gaps(
    controls: &[PaintShopControl],
    _decal_sheet: Option<&PaintShopDecalSheet>,
) -> Vec<PaintShopGap> {
    let mut gaps = Vec::new();
    let mut gap = |code, quantity, role, slot, detail: &'static str, affected: &'static str| {
        let Some(control) = controls
            .iter()
            .find(|control| control.role == role && control.slot == slot)
        else {
            return;
        };
        let census = control.field_census();
        // The record letter is the one text field every `D` record carries;
        // anything beyond it is a value the shop would be storing.
        if census.colour > 0 || census.text > 1 || census.hex > 0 {
            return;
        }
        gaps.push(PaintShopGap {
            code,
            quantity,
            displayed: control.displayed_entries(),
            census,
            detail,
            affected,
        });
    };
    gap(
        SWATCH_GAP,
        "the colour swatch palette of one paint slot",
        PaintShopRole::Color,
        Some(0),
        "the layout declares how many colour swatches a control displays and no field of that \
         record carries a colour; the swatch values are produced by a native callback inside the \
         engine image, which no readable member stores",
        "every colour the paint shop offers, on every paint slot",
    );
    gap(
        SHADE_GAP,
        "the shade table of one paint slot",
        PaintShopRole::Shade,
        Some(0),
        "the layout declares how many shades a control displays and no field of that record \
         carries a shade; the shaded values are produced by a native callback inside the engine \
         image, which no readable member stores",
        "every shaded paint variant",
    );
    gap(
        PATTERN_NAME_GAP,
        "the display name of a paint pattern",
        PaintShopRole::Pattern,
        None,
        "the layout declares how many patterns a control displays and no field of that record \
         carries a name; the pattern names are produced by a native callback inside the engine \
         image, which no readable member stores",
        "every faction and player paint name in the shop",
    );
    gap(
        DECAL_GAP,
        "the decal a paint slot selects",
        PaintShopRole::Decal,
        Some(0),
        "the layout declares how many decals a control displays and no field of that record \
         carries a decal id; the selected decal is produced by a native callback inside the \
         engine image",
        "every decal selection",
    );
    gaps
}

/// Acceptance stage F09-B. Every fixture is newly authored synthetic bytes
/// built here; nothing is derived from original game data. These tests call
/// the production [`compose_livery`] / [`source_fingerprint`] path.
#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use cs_assets::install;
    use cs_assets::rof::{RofSource, mount_rof_into};
    use cs_assets::vfs::{ContentSession, INSTALL_NAMESPACE, MountBuilder, SessionBuilder};
    use cs_formats::zbd::{
        INDEX_ENTRY_BYTES, INDEX_NAME_BYTES, INDEX_UNEXPLAINED_BYTES, TRAILER_VERSION_ONE,
    };
    use cs_formats::{ParseContext, read_bm};
    use cs_types::asset_id::{MountId, MountNamespace, PrecedenceClass, ResolveContext};

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

    // ------------------------------------------------------------ F09-D ---

    /// A synthetic BM with explicit planes in stored order and an optional
    /// uncovered tail.
    fn livery_bytes(
        height: u16,
        width: u16,
        base: &[u8],
        masks: [&[u8]; 3],
        overlay: &[u8],
        tail: &[u8],
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&height.to_le_bytes());
        bytes.extend_from_slice(&width.to_le_bytes());
        bytes.extend_from_slice(base);
        for mask in masks {
            bytes.extend_from_slice(mask);
        }
        bytes.extend_from_slice(overlay);
        bytes.extend_from_slice(tail);
        bytes
    }

    /// One valid 1x1 BM: RGB base, three one-byte masks, RGBA overlay.
    fn one_by_one() -> Vec<u8> {
        livery_bytes(
            1,
            1,
            &[9, 8, 7],
            [&[10], &[20], &[30]],
            &[1, 2, 3, 200],
            &[],
        )
    }

    /// AC (part): a member spelling in the observed stock-livery layout names
    /// its faction, airframe prefix and part, and anything else is refused.
    #[test]
    fn accept_f09_d_spelling_names_faction_prefix_and_part() {
        let name = parse_livery_spelling("ASSETS/GRAPHICS/BLACKHAT/AGYRO_FUSALAGE1.BM")
            .expect("a livery spelling");
        assert_eq!(name.faction, "BLACKHAT");
        assert_eq!(name.prefix, "AGYRO");
        assert_eq!(name.part, "FUSALAGE1");

        // The extension is compared case-insensitively; the rest is preserved.
        let lower =
            parse_livery_spelling("assets/graphics/blackhat/agyro_fusalage1.bm").expect("a livery");
        assert_eq!(lower.faction, "blackhat");
        assert_eq!(lower.prefix, "agyro");

        for rejected in [
            "ASSETS/TEXTURES/BLACKHAT/AGYRO_FUSALAGE1.BM",
            "ASSETS/GRAPHICS/BLACKHAT/AGYRO.BM",
            "ASSETS/GRAPHICS/BLACKHAT/AGYRO_.BM",
            "ASSETS/GRAPHICS/BLACKHAT/_FUSALAGE1.BM",
            "ASSETS/GRAPHICS/AGYRO_FUSALAGE1.BM",
            "AGYRO_FUSALAGE1.BM",
            "ASSETS/GRAPHICS/BLACKHAT/AGYRO_FUSALAGE1.PNG",
        ] {
            assert!(parse_livery_spelling(rejected).is_none(), "{rejected}");
        }
    }

    /// The catalog groups one source's members into the valid
    /// `<PREFIX>_<PART>` x faction combinations and keeps every member it
    /// cannot verify as a finding instead of dropping it.
    #[test]
    fn accept_f09_d_catalog_discovers_combinations_and_keeps_findings() {
        let bm = one_by_one();
        let mut catalog = StockLiveryCatalog::new();
        catalog.add("ASSETS/GRAPHICS/RED/AAA_B.BM", &bm);
        catalog.add("ASSETS/GRAPHICS/BLUE/AAA_C.BM", &bm);
        catalog.add("ASSETS/GRAPHICS/RED/BBB_D.BM", &bm);
        // A `.bm` outside the layout, one that is not a BM plane set at all,
        // and a non-BM member: the first two are findings, the last is not a
        // livery candidate.
        catalog.add("ASSETS/GRAPHICS/RED/NOPREFIX.BM", &bm);
        catalog.add("ASSETS/TEXTURES/RED/AAA_B.BM", &bm);
        catalog.add("ASSETS/GRAPHICS/RED/CCC_B.BM", &bm[..10]);
        catalog.add("ASSETS/GRAPHICS/RED/AAA_B.PNG", &bm);

        assert_eq!(catalog.assets().len(), 3);
        assert_eq!(
            catalog
                .findings()
                .iter()
                .map(|finding| finding.code())
                .collect::<Vec<_>>(),
            ["unexpected_eof", "not_a_stock_livery", "not_a_stock_livery"],
        );
        assert_eq!(catalog.factions(), ["BLUE", "RED"]);
        assert_eq!(catalog.prefixes(), ["AAA", "BBB"]);

        let combinations = catalog.combinations();
        assert_eq!(combinations.len(), 2);
        assert_eq!(combinations[0].prefix(), "AAA");
        assert_eq!(
            combinations[0]
                .factions()
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["BLUE", "RED"],
        );
        assert_eq!(combinations[0].assets(), 2);
        assert_eq!(combinations[1].prefix(), "BBB");
        assert_eq!(
            combinations[1]
                .factions()
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["RED"],
        );
        assert_eq!(combinations[1].assets(), 1);
    }

    /// The catalog records the dimensions, the exact covered length and any
    /// uncovered tail, and reports a zero-sized header as unsupported.
    #[test]
    fn accept_f09_d_catalog_records_dimensions_tail_and_refusals() {
        // 3 wide, 2 high, with a 5-byte uncovered tail.
        let mut bytes = livery_bytes(
            2,
            3,
            &[1u8; 18],
            [&[2u8; 6], &[3u8; 6], &[4u8; 6]],
            &[5u8; 24],
            &[0xaa; 5],
        );
        let mut catalog = StockLiveryCatalog::new();
        catalog.add("ASSETS/GRAPHICS/RED/AAA_B.BM", &bytes);
        let asset = &catalog.assets()[0];
        assert_eq!((asset.width(), asset.height()), (3, 2));
        assert_eq!(asset.covered_len(), 4 + 10 * 6);
        assert_eq!(asset.tail_bytes(), 5);
        assert_eq!(asset.faction(), "RED");
        assert_eq!(asset.prefix(), "AAA");
        assert_eq!(asset.part(), "B");

        // An empty image is outside the observed subset: a finding, not a
        // silently accepted 4-byte file.
        bytes = livery_bytes(0, 0, &[], [&[], &[], &[]], &[], &[]);
        catalog.add("ASSETS/GRAPHICS/RED/DDD_E.BM", &bytes);
        assert_eq!(catalog.assets().len(), 1);
        assert_eq!(catalog.findings().len(), 1);
        assert_eq!(catalog.findings()[0].code(), "empty_image");
        assert_eq!(
            catalog.findings()[0].spelling(),
            "ASSETS/GRAPHICS/RED/DDD_E.BM"
        );
    }

    /// The catalog is deterministic: the same members in any enumeration
    /// order give the same asset list and the same combination table.
    #[test]
    fn accept_f09_d_catalog_is_deterministic_in_spelling_order() {
        let bm = one_by_one();
        let mut first = StockLiveryCatalog::new();
        for spelling in [
            "ASSETS/GRAPHICS/ZED/BBB_B.BM",
            "ASSETS/GRAPHICS/ALPHA/BBB_A.BM",
            "ASSETS/GRAPHICS/ALPHA/AAA_A.BM",
        ] {
            first.add(spelling, &bm);
        }
        let mut second = StockLiveryCatalog::new();
        for spelling in [
            "ASSETS/GRAPHICS/ALPHA/AAA_A.BM",
            "ASSETS/GRAPHICS/ZED/BBB_B.BM",
            "ASSETS/GRAPHICS/ALPHA/BBB_A.BM",
        ] {
            second.add(spelling, &bm);
        }
        let spellings = |catalog: &StockLiveryCatalog| {
            catalog
                .assets()
                .iter()
                .map(|asset| asset.spelling().to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(spellings(&first), spellings(&second));
        assert_eq!(
            spellings(&first),
            [
                "ASSETS/GRAPHICS/ALPHA/AAA_A.BM",
                "ASSETS/GRAPHICS/ALPHA/BBB_A.BM",
                "ASSETS/GRAPHICS/ZED/BBB_B.BM",
            ],
        );
        assert_eq!(first.combinations(), second.combinations());
    }

    // -------------------------------------------------- retail (F09-D) ---

    /// The shared airframe library, spelled as the installation does.
    const RETAIL_CONTAINER: &str = "GOSDATA/ASSETS/crimson.rof";

    /// The 14 faction directories of the original airframe library.
    const RETAIL_FACTIONS: [&str; 14] = [
        "BLACKHAT", "BLAKE", "BLCKSWAN", "BRITISH", "BROADWAY", "CCCP", "FORTUNE", "GERMAN",
        "HOLLYWD", "HUGHES", "ITSTAXI", "MEDUSAS", "SACTRUST", "STUDIO",
    ];

    /// The 11 airframe prefixes the library stores.
    const RETAIL_PREFIXES: [&str; 11] = [
        "AGYRO", "BAL", "BLO", "BRI", "DEV", "FIR", "FUR", "HEL", "KES", "PEA", "WAR",
    ];

    /// The measured valid combinations: every prefix and, per faction, how
    /// many `<PREFIX>_*` assets that faction stores. Measured from the
    /// installation and pinned here so a reader or catalog change that
    /// discovers fewer cannot pass.
    const RETAIL_COMBINATIONS: [(&str, &[(&str, usize)]); 11] = [
        (
            "AGYRO",
            &[
                ("BLACKHAT", 4),
                ("FORTUNE", 4),
                ("ITSTAXI", 4),
                ("STUDIO", 4),
            ],
        ),
        ("BAL", &[("BRITISH", 5), ("FORTUNE", 5)]),
        ("BLO", &[("BLAKE", 6), ("FORTUNE", 6), ("HUGHES", 6)]),
        ("BRI", &[("BLACKHAT", 9), ("FORTUNE", 9), ("MEDUSAS", 9)]),
        ("DEV", &[("CCCP", 6), ("FORTUNE", 6)]),
        ("FIR", &[("FORTUNE", 6), ("HOLLYWD", 6)]),
        (
            "FUR",
            &[
                ("BLCKSWAN", 5),
                ("FORTUNE", 5),
                ("HUGHES", 5),
                ("STUDIO", 5),
            ],
        ),
        ("HEL", &[("FORTUNE", 4), ("GERMAN", 4), ("SACTRUST", 4)]),
        ("KES", &[("FORTUNE", 6), ("HUGHES", 6), ("MEDUSAS", 6)]),
        (
            "PEA",
            &[
                ("BLAKE", 6),
                ("BRITISH", 6),
                ("BROADWAY", 6),
                ("FORTUNE", 6),
            ],
        ),
        ("WAR", &[("BLACKHAT", 5), ("FORTUNE", 5), ("SACTRUST", 5)]),
    ];

    fn game_dir() -> PathBuf {
        PathBuf::from(
            std::env::var_os("CS_GAME_DIR")
                .expect("CS_GAME_DIR must name the original installation for this retail test"),
        )
    }

    /// Mounts `GOSDATA/ASSETS/crimson.rof` through the production ROF mount.
    fn retail_crimson() -> RofSource {
        let root = game_dir();
        assert!(
            root.is_dir(),
            "CS_GAME_DIR {} is not a directory",
            root.display()
        );
        let found = install::discover(&root).expect("the installation is discovered");
        let context = ResolveContext::new(install::fingerprint(&found.manifest));
        let mut builder = SessionBuilder::new(context);
        let mount = MountBuilder::new(
            MountId::new("rof-gosdata-assets-crimson-rof").expect("a valid mount id"),
            MountNamespace::new(INSTALL_NAMESPACE).expect("a valid namespace"),
            PrecedenceClass::Shared,
            RETAIL_CONTAINER,
        )
        .retail();
        mount_rof_into(&mut builder, mount, &root.join(RETAIL_CONTAINER))
            .expect("the airframe library mounts")
    }

    fn member_key(source: &RofSource, spelling: &str) -> AssetKey {
        AssetKey::from_spelling(source.namespace().as_str(), spelling, "default")
            .expect("a valid member key")
    }

    /// The measured per-faction asset counts, keyed by prefix.
    fn measured_combinations(
        catalog: &StockLiveryCatalog,
    ) -> BTreeMap<String, BTreeMap<String, usize>> {
        let mut table: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
        for asset in catalog.assets() {
            *table
                .entry(asset.prefix().to_owned())
                .or_default()
                .entry(asset.faction().to_owned())
                .or_default() += 1;
        }
        table
    }

    /// Endpoint coverage of a plane corpus: how many mask bytes and overlay
    /// alpha values are at the `0` and `255` endpoints and in between.
    #[derive(Default)]
    struct EndpointTally {
        mask_zero: u64,
        mask_full: u64,
        mask_partial: u64,
        alpha_zero: u64,
        alpha_full: u64,
        alpha_partial: u64,
    }

    fn tally_endpoints(file: &BmFile<'_>, tally: &mut EndpointTally) {
        for y in 0..file.height() {
            for x in 0..file.width() {
                for plane in [BmPlane::Mask1, BmPlane::Mask2, BmPlane::Mask3] {
                    match file.mask(plane, x, y).expect("inside the image") {
                        0 => tally.mask_zero += 1,
                        255 => tally.mask_full += 1,
                        _ => tally.mask_partial += 1,
                    }
                }
                match file.overlay(x, y).expect("inside the image")[3] {
                    0 => tally.alpha_zero += 1,
                    255 => tally.alpha_full += 1,
                    _ => tally.alpha_partial += 1,
                }
            }
        }
    }

    /// AC: every known stock livery of the original airframe library is in
    /// the observed BM subset and every discovered valid combination is
    /// enumerated, with the measured counts pinned.
    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_f09_d_retail_stock_liveries_and_combinations() {
        let source = retail_crimson();
        let catalog = StockLiveryCatalog::discover(&source);

        assert!(
            catalog.findings().is_empty(),
            "an airframe-library member is unsupported: {:?}",
            catalog
                .findings()
                .iter()
                .map(|finding| (finding.spelling().to_owned(), finding.code()))
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            catalog.assets().len(),
            184,
            "the original airframe library holds 184 stock livery members"
        );
        assert_eq!(
            catalog
                .factions()
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            RETAIL_FACTIONS,
        );
        assert_eq!(
            catalog
                .prefixes()
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            RETAIL_PREFIXES,
        );

        // The combination table, both through the accessor and as the
        // per-faction asset counts.
        let measured = measured_combinations(&catalog);
        let expected: BTreeMap<String, BTreeMap<String, usize>> = RETAIL_COMBINATIONS
            .iter()
            .map(|(prefix, factions)| {
                (
                    (*prefix).to_owned(),
                    factions
                        .iter()
                        .map(|(faction, count)| ((*faction).to_owned(), *count))
                        .collect(),
                )
            })
            .collect();
        assert_eq!(measured, expected, "the measured combination table");
        assert_eq!(catalog.combinations().len(), 11);

        // Every member is exactly the covered length, no tail, nonzero.
        let mut tally = EndpointTally::default();
        for asset in catalog.assets() {
            assert_eq!(
                asset.tail_bytes(),
                0,
                "{} has bytes outside the observed subset",
                asset.spelling()
            );
            assert!(asset.width() > 0 && asset.height() > 0);
            assert_eq!(
                asset.covered_len(),
                4 + 10 * u64::from(asset.width()) * u64::from(asset.height()),
                "{}",
                asset.spelling()
            );
            let read = source
                .read(&member_key(&source, asset.spelling()))
                .expect("the member reads back");
            let mut context = ParseContext::with_defaults(asset.spelling());
            let file = read_bm(&mut context, &read.data).expect("the member parses");
            tally_endpoints(&file, &mut tally);
        }

        // The corpus really exercises both endpoints and the interior of the
        // masks and the overlay alpha; otherwise the composition tests would
        // not discriminate them.
        assert!(tally.mask_zero > 0 && tally.mask_full > 0 && tally.mask_partial > 0);
        assert!(tally.alpha_zero > 0 && tally.alpha_full > 0 && tally.alpha_partial > 0);
    }

    /// The pinned reference of composed stock and private paints, one entry
    /// per stock livery.
    struct ReferenceLivery {
        spelling: String,
        stock: [PaintColor; 3],
        custom: [PaintColor; 3],
        stock_path: PathBuf,
        custom_path: PathBuf,
    }

    /// The private pinned reference directory, overridable for a reviewer.
    fn reference_dir() -> PathBuf {
        if let Some(dir) = std::env::var_os("CS_F09_D_REFERENCE") {
            return PathBuf::from(dir);
        }
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crates/")
            .parent()
            .expect("workspace root")
            .join("private/f09d-reference")
    }

    /// One `r g b` x 3 triple.
    fn paint_of(fields: &[&str]) -> [PaintColor; 3] {
        assert_eq!(fields.len(), 9, "{fields:?}");
        let channel = |index: usize| fields[index].parse::<u8>().expect("a color channel");
        [
            PaintColor::new(channel(0), channel(1), channel(2)),
            PaintColor::new(channel(3), channel(4), channel(5)),
            PaintColor::new(channel(6), channel(7), channel(8)),
        ]
    }

    fn read_reference(dir: &Path) -> Vec<ReferenceLivery> {
        let path = dir.join("manifest.txt");
        let manifest = fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!(
                "cannot read {}: {error} (generate the pinned reference with the script in \
                 docs/findings/2026-09-29-f09-d-stock-liveries-and-combinations.md)",
                path.display()
            )
        });
        let mut colors: BTreeMap<String, [PaintColor; 3]> = BTreeMap::new();
        let mut custom: Option<[PaintColor; 3]> = None;
        let mut rows: Vec<(String, String, String, String)> = Vec::new();
        for line in manifest.lines() {
            let fields: Vec<&str> = line.split_whitespace().collect();
            match fields.first().copied() {
                Some("faction") => {
                    assert_eq!(fields.len(), 11, "{line}");
                    colors.insert(fields[1].to_owned(), paint_of(&fields[2..]));
                }
                Some("custom") => {
                    assert_eq!(fields.len(), 10, "{line}");
                    custom = Some(paint_of(&fields[1..]));
                }
                Some("livery") => {
                    assert_eq!(fields.len(), 5, "{line}");
                    rows.push((
                        fields[1].to_owned(),
                        fields[2].to_owned(),
                        fields[3].to_owned(),
                        fields[4].to_owned(),
                    ));
                }
                _ => {}
            }
        }
        let custom = custom.expect("the manifest names the private paint");
        rows.into_iter()
            .map(|(spelling, faction, stock, custom_path)| ReferenceLivery {
                stock: *colors
                    .get(&faction)
                    .unwrap_or_else(|| panic!("the manifest has no colors for {faction}")),
                spelling,
                custom,
                stock_path: dir.join(stock),
                custom_path: dir.join(custom_path),
            })
            .collect()
    }

    /// Reads a reference image: `u32` width, `u32` height, then RGB8.
    fn read_reference_rgb(path: &Path) -> (u32, u32, Vec<u8>) {
        let bytes = fs::read(path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        assert!(bytes.len() >= 8, "{}", path.display());
        let width = u32::from_le_bytes(bytes[0..4].try_into().expect("four bytes"));
        let height = u32::from_le_bytes(bytes[4..8].try_into().expect("four bytes"));
        let rgb = bytes[8..].to_vec();
        assert_eq!(
            rgb.len(),
            width as usize * height as usize * 3,
            "{}",
            path.display()
        );
        (width, height, rgb)
    }

    /// Compares every texel and also counts how many would differ if the
    /// reference were vertically flipped, so an orientation error is named.
    fn compare(reference: &[u8], composed: &[u8], width: u32, height: u32) -> (usize, usize) {
        let mut differences = 0usize;
        let mut flipped = 0usize;
        for y in 0..height {
            for x in 0..width {
                let at = ((y * width + x) as usize) * 3;
                if reference[at..at + 3] != composed[at..at + 3] {
                    differences += 1;
                }
                let mirrored = (((height - 1 - y) * width + x) as usize) * 3;
                if reference[mirrored..mirrored + 3] != composed[at..at + 3] {
                    flipped += 1;
                }
            }
        }
        (differences, flipped)
    }

    /// AC: a stock livery and a private (custom) paint composed from the
    /// original planes match the pinned reference, including the decal
    /// overlay alpha, at every texel.
    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_f09_d_retail_composed_liveries_match_the_pinned_reference() {
        let source = retail_crimson();
        let catalog = StockLiveryCatalog::discover(&source);
        let directory = reference_dir();
        let reference = read_reference(&directory);
        assert_eq!(reference.len(), 184, "{}", directory.display());

        // The catalog and the reference describe exactly the same members.
        let discovered: Vec<&str> = catalog.assets().iter().map(StockLivery::spelling).collect();
        let referenced: Vec<&str> = reference
            .iter()
            .map(|entry| entry.spelling.as_str())
            .collect();
        assert_eq!(discovered, referenced);

        let mut compared_texels = 0u64;
        let mut tally = EndpointTally::default();
        for entry in &reference {
            let read = source
                .read(&member_key(&source, &entry.spelling))
                .unwrap_or_else(|error| panic!("{}: {error}", entry.spelling));
            let mut context = ParseContext::with_defaults(&entry.spelling);
            let file = read_bm(&mut context, &read.data)
                .unwrap_or_else(|error| panic!("{}: {error}", entry.spelling));
            tally_endpoints(&file, &mut tally);

            for (paint, path) in [
                (LiveryPaint::new(entry.stock), &entry.stock_path),
                (LiveryPaint::new(entry.custom), &entry.custom_path),
            ] {
                let composed = compose_livery(
                    &file,
                    &paint,
                    &mut AllocationBudget::with_defaults(&entry.spelling),
                )
                .unwrap_or_else(|error| panic!("{}: {error}", entry.spelling));
                let (width, height, expected) = read_reference_rgb(path);
                assert_eq!(
                    (width, height),
                    (composed.image().width(), composed.image().height()),
                    "{}",
                    path.display()
                );
                let (differences, flipped) = compare(&expected, composed.rgb(), width, height);
                assert_eq!(
                    differences,
                    0,
                    "{}: {differences} of {} texels differ from the pinned reference \
                     (a vertical flip would differ in {flipped})",
                    path.display(),
                    u64::from(width) * u64::from(height),
                );
                compared_texels += u64::from(width) * u64::from(height);
            }
        }
        assert!(compared_texels > 0);
        assert!(tally.mask_full > 0 && tally.mask_partial > 0);
        assert!(tally.alpha_zero > 0 && tally.alpha_full > 0 && tally.alpha_partial > 0);
    }

    // ---------------------------------------------- palette (F09-PALETTE) ---

    static NEXT_PALETTE_TREE: AtomicU64 = AtomicU64::new(0);

    /// A disposable installation tree for the palette fixtures.
    struct PaletteTree(PathBuf);

    impl PaletteTree {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "cs-f09-palette-{}-{}",
                std::process::id(),
                NEXT_PALETTE_TREE.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).expect("the fixture root is created");
            Self(root)
        }

        fn write(&self, spelling: &str, bytes: &[u8]) {
            let path = self.0.join(spelling);
            fs::create_dir_all(path.parent().expect("a parent")).expect("fixture dirs");
            fs::write(path, bytes).expect("fixture bytes are written");
        }

        /// A session of this tree mounted as the production install root,
        /// exactly as `$CS_GAME_DIR` is mounted, so the palette extraction
        /// resolves `ZBD/zrdr.zbd` through the production VFS path.
        fn session(&self) -> ContentSession {
            let found = install::discover(&self.0).expect("the fixture installation is discovered");
            let context = ResolveContext::new(install::fingerprint(&found.manifest));
            let mut builder = SessionBuilder::new(context);
            let mount = MountBuilder::new(
                MountId::new("install").expect("a valid mount id"),
                MountNamespace::new(INSTALL_NAMESPACE).expect("a valid namespace"),
                PrecedenceClass::Shared,
                ".",
            )
            .retail();
            builder
                .mount_directory(mount, &self.0)
                .expect("the fixture installation mounts");
            builder.open()
        }
    }

    impl Drop for PaletteTree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// The key the palette extraction addresses in an install-root mount.
    fn palette_key() -> AssetKey {
        AssetKey::from_spelling(INSTALL_NAMESPACE, PALETTE_CONTAINER, "default")
            .expect("a valid palette key")
    }

    /// One `T1` int node.
    fn zrd_int(value: u32) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(8);
        bytes.extend_from_slice(&ZRD_TAG_INT.to_le_bytes());
        bytes.extend_from_slice(&value.to_le_bytes());
        bytes
    }

    /// One `T3` text node.
    fn zrd_text(text: &str) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(8 + text.len());
        bytes.extend_from_slice(&ZRD_TAG_TEXT.to_le_bytes());
        bytes.extend_from_slice(&u32::try_from(text.len()).expect("fits").to_le_bytes());
        bytes.extend_from_slice(text.as_bytes());
        bytes
    }

    /// One `T4` list node holding `children`. Its count word is `len + 1`
    /// because a list of `N` holds `N - 1` children.
    fn zrd_list(children: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&ZRD_TAG_LIST.to_le_bytes());
        bytes.extend_from_slice(
            &u32::try_from(children.len() + 1)
                .expect("fits")
                .to_le_bytes(),
        );
        for child in children {
            bytes.extend_from_slice(child);
        }
        bytes
    }

    /// One `vehicle.zrd` record carrying a pattern, a color triple and a decal
    /// triple, spelled as the original stores them.
    fn zrd_paint_record(pattern: &str, colors: [[u8; 3]; 3], decals: [u32; 3]) -> Vec<u8> {
        let mut children = Vec::new();
        children.push(zrd_text("paint_pattern"));
        children.push(zrd_list(&[zrd_text(pattern)]));
        for (slot, color) in colors.iter().enumerate() {
            children.push(zrd_text(&format!("paint_color{}", slot + 1)));
            children.push(zrd_list(&[
                zrd_int(u32::from(color[0])),
                zrd_int(u32::from(color[1])),
                zrd_int(u32::from(color[2])),
            ]));
        }
        for (slot, decal) in decals.iter().enumerate() {
            children.push(zrd_text(&format!("paint_decal{}", slot + 1)));
            children.push(zrd_list(&[zrd_int(*decal)]));
        }
        zrd_list(&children)
    }

    /// One `vehicle.zrd` record carrying a pattern but no colors or decals.
    fn zrd_pattern_only_record(pattern: &str) -> Vec<u8> {
        zrd_list(&[zrd_text("paint_pattern"), zrd_list(&[zrd_text(pattern)])])
    }

    /// A whole `vehicle.zrd` in the observed shape: a root list whose only
    /// child alternates record-name text and record list.
    fn vehicle_zrd(records: &[(&str, &[u8])]) -> Vec<u8> {
        let mut top = Vec::new();
        for (name, record) in records {
            top.push(zrd_text(name));
            top.push(record.to_vec());
        }
        zrd_list(&[zrd_list(&top)])
    }

    /// A version-one reader archive holding `members`, written exactly as the
    /// pinned reader expects: member data, 148-byte index entries, trailer.
    fn palette_reader_archive(members: &[(&str, &[u8])]) -> Vec<u8> {
        let mut data = Vec::new();
        let mut entries = Vec::new();
        for (name, bytes) in members {
            entries.extend_from_slice(&u32::try_from(data.len()).expect("fits").to_le_bytes());
            entries.extend_from_slice(&u32::try_from(bytes.len()).expect("fits").to_le_bytes());
            let mut field = vec![0u8; INDEX_NAME_BYTES];
            field[..name.len()].copy_from_slice(name.as_bytes());
            entries.extend_from_slice(&field);
            entries.extend_from_slice(&[0u8; INDEX_UNEXPLAINED_BYTES]);
            data.extend_from_slice(bytes);
        }
        assert_eq!(entries.len(), members.len() * INDEX_ENTRY_BYTES as usize);
        data.extend_from_slice(&entries);
        data.extend_from_slice(&TRAILER_VERSION_ONE.to_le_bytes());
        data.extend_from_slice(&u32::try_from(members.len()).expect("fits").to_le_bytes());
        data
    }

    /// AC: the production extractor reads the faction palette out of the
    /// original-shaped member, with a container-absolute span per color,
    /// decal and pattern.
    #[test]
    fn accept_f09_palette_extracts_the_faction_palette_and_spans() {
        let medusas = zrd_paint_record(
            "medusas",
            [[95, 125, 143], [41, 14, 21], [141, 137, 93]],
            [21, 14, 14],
        );
        let british = zrd_paint_record(
            "british",
            [[177, 130, 66], [48, 47, 39], [255, 255, 255]],
            [21, 4, 4],
        );
        let zrd = vehicle_zrd(&[("medkestrel", &medusas), ("britpeace", &british)]);
        let archive = palette_reader_archive(&[(PALETTE_MEMBER, &zrd)]);

        let tree = PaletteTree::new();
        tree.write(PALETTE_CONTAINER, &archive);
        let catalog = FactionPaletteCatalog::discover(&tree.session(), &palette_key())
            .expect("the palette extracts");

        assert_eq!(catalog.container_path(), PALETTE_CONTAINER);
        assert_eq!(catalog.member(), PALETTE_MEMBER);
        assert_eq!(catalog.member_sha256(), sha256(&zrd));
        assert_eq!(catalog.member_span().offset(), 0);
        assert_eq!(catalog.member_span().length(), zrd.len() as u64);
        assert_eq!(catalog.member_span().member_key(), Some(PALETTE_MEMBER));
        assert_eq!(catalog.records().len(), 2);
        assert_eq!(catalog.faction_names(), ["british", "medusas"]);
        assert!(catalog.findings().is_empty());

        let record = catalog
            .record("medkestrel")
            .expect("the record is extracted");
        assert_eq!(record.pattern(), "medusas");
        assert!(record.has_colors());
        assert_eq!(record.color(0).expect("slot 0").rgb(), [95, 125, 143]);
        assert_eq!(record.color(2).expect("slot 2").blue(), 93);
        assert_eq!(record.decal(0).expect("decal 0").index(), 21);
        assert_eq!(record.decal(1).expect("decal 1").index(), 14);
        assert_eq!(record.decal(2).expect("decal 2").index(), 14);

        // The span locates the exact encoded color list: `T4` tag, count 4,
        // then three `T1` ints.
        let span = record.color(0).expect("slot 0").span();
        assert_eq!(span.container_path(), PALETTE_CONTAINER);
        assert_eq!(span.member_key(), Some(PALETTE_MEMBER));
        let start = span.offset() as usize;
        let end = start + span.length() as usize;
        let expected: [u8; 32] = [
            4, 0, 0, 0, 4, 0, 0, 0, 1, 0, 0, 0, 95, 0, 0, 0, 1, 0, 0, 0, 125, 0, 0, 0, 1, 0, 0, 0,
            143, 0, 0, 0,
        ];
        assert_eq!(&archive[start..end], &expected[..]);
        assert_eq!(record.pattern_span().member_key(), Some(PALETTE_MEMBER));
        assert!(record.pattern_span().length() > 0);
        assert!(record.span().length() > 0);

        // The faction facade returns the same palette.
        let palette = catalog.palette("medusas").expect("the faction exists");
        assert_eq!(palette.color(1).expect("slot 1").rgb(), [41, 14, 21]);
        assert_eq!(
            palette
                .records()
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["medkestrel"],
        );
    }

    /// AC: an unknown faction and an out-of-range color or decal slot are
    /// refused, never padded with a zero.
    #[test]
    fn accept_f09_palette_refuses_unknown_factions_and_slots() {
        let medusas = zrd_paint_record("medusas", [[1, 2, 3], [4, 5, 6], [7, 8, 9]], [21, 14, 14]);
        let zrd = vehicle_zrd(&[("medkestrel", &medusas)]);
        let archive = palette_reader_archive(&[(PALETTE_MEMBER, &zrd)]);
        let tree = PaletteTree::new();
        tree.write(PALETTE_CONTAINER, &archive);
        let catalog = FactionPaletteCatalog::discover(&tree.session(), &palette_key())
            .expect("the palette extracts");

        assert_eq!(
            catalog.palette("no_such_faction").unwrap_err(),
            PaletteRefusal::UnknownFaction {
                faction: "no_such_faction".to_owned(),
            },
        );
        let palette = catalog.palette("medusas").expect("the faction exists");
        assert_eq!(
            palette.color(3).unwrap_err(),
            PaletteRefusal::UnknownColorSlot {
                owner: "medusas".to_owned(),
                slot: 3,
                available: 3,
            },
        );
        assert_eq!(
            palette.decal(3).unwrap_err(),
            PaletteRefusal::UnknownDecalSlot {
                owner: "medusas".to_owned(),
                slot: 3,
                available: 3,
            },
        );
    }

    /// AC: a member that is not the observed `.zrd` layout, or one the archive
    /// does not declare, is refused with the decoder's own code instead of
    /// being read as an empty palette.
    #[test]
    fn accept_f09_palette_refuses_a_member_that_is_not_the_observed_layout() {
        // A list whose only child carries an unknown node tag.
        let broken = zrd_list(&[vec![9, 0, 0, 0]]);
        let archive = palette_reader_archive(&[(PALETTE_MEMBER, &broken)]);
        let tree = PaletteTree::new();
        tree.write(PALETTE_CONTAINER, &archive);
        let error = FactionPaletteCatalog::discover(&tree.session(), &palette_key())
            .expect_err("a malformed member is refused");
        assert_eq!(error.code(), "unknown_tag");
        assert!(error.to_string().contains("observed layout"), "{error}");

        let other = palette_reader_archive(&[("other.zrd", &broken)]);
        let tree = PaletteTree::new();
        tree.write(PALETTE_CONTAINER, &other);
        let error = FactionPaletteCatalog::discover(&tree.session(), &palette_key())
            .expect_err("a missing member is refused");
        assert_eq!(error.code(), "missing_member");
    }

    /// AC: a record that names a pattern but stores no colors is kept in the
    /// records and named by a finding, and never becomes a faction palette.
    #[test]
    fn accept_f09_palette_records_a_pattern_without_colors_as_a_finding() {
        let devastator = zrd_pattern_only_record("player_fortune");
        let medusas = zrd_paint_record("medusas", [[1, 2, 3], [4, 5, 6], [7, 8, 9]], [21, 14, 14]);
        let zrd = vehicle_zrd(&[("devastator", &devastator), ("medkestrel", &medusas)]);
        let archive = palette_reader_archive(&[(PALETTE_MEMBER, &zrd)]);
        let tree = PaletteTree::new();
        tree.write(PALETTE_CONTAINER, &archive);
        let catalog = FactionPaletteCatalog::discover(&tree.session(), &palette_key())
            .expect("the palette extracts");

        let record = catalog
            .record("devastator")
            .expect("the record is extracted");
        assert_eq!(record.pattern(), "player_fortune");
        assert!(!record.has_colors());
        assert_eq!(
            record.color(0).unwrap_err(),
            PaletteRefusal::UnknownColorSlot {
                owner: "devastator".to_owned(),
                slot: 0,
                available: 0,
            },
        );
        assert!(catalog.palette("player_fortune").is_err());
        assert_eq!(catalog.faction_names(), ["medusas"]);
        assert_eq!(catalog.findings().len(), 1);
        assert_eq!(catalog.findings()[0].code(), "pattern_without_colors");
        assert_eq!(catalog.findings()[0].record(), "devastator");
    }

    /// AC: records that name one pattern and store the same colors and decals
    /// are one faction even though each carries its own span (a span is
    /// provenance, not identity); a genuine value disagreement is still a
    /// finding. This is the bug the retail evidence run caught: comparing the
    /// whole [`PaletteColor`] compares offsets too, so every same-pattern record
    /// looked inconsistent.
    #[test]
    fn accept_f09_palette_groups_agreeing_records_and_flags_value_disagreement() {
        let first = zrd_paint_record("medusas", [[1, 2, 3], [4, 5, 6], [7, 8, 9]], [21, 14, 14]);
        let same = zrd_paint_record("medusas", [[1, 2, 3], [4, 5, 6], [7, 8, 9]], [21, 14, 14]);
        let zrd = vehicle_zrd(&[("medkestrel", &first), ("medbrigand", &same)]);
        let archive = palette_reader_archive(&[(PALETTE_MEMBER, &zrd)]);
        let tree = PaletteTree::new();
        tree.write(PALETTE_CONTAINER, &archive);
        let catalog = FactionPaletteCatalog::discover(&tree.session(), &palette_key())
            .expect("the palette extracts");

        assert_eq!(catalog.faction_names(), ["medusas"]);
        assert!(
            catalog.findings().is_empty(),
            "spans are provenance, not identity: {:?}",
            catalog.findings()
        );
        let palette = catalog.palette("medusas").expect("the faction exists");
        assert_eq!(
            palette
                .records()
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["medkestrel", "medbrigand"],
        );
        // The two records really do carry different spans.
        assert_ne!(
            catalog
                .record("medkestrel")
                .expect("record")
                .color(0)
                .expect("color")
                .span(),
            catalog
                .record("medbrigand")
                .expect("record")
                .color(0)
                .expect("color")
                .span(),
        );

        // A real value disagreement is a finding, not silently collapsed.
        let different =
            zrd_paint_record("medusas", [[9, 9, 9], [4, 5, 6], [7, 8, 9]], [21, 14, 14]);
        let zrd = vehicle_zrd(&[("medkestrel", &first), ("medbrigand", &different)]);
        let archive = palette_reader_archive(&[(PALETTE_MEMBER, &zrd)]);
        let tree = PaletteTree::new();
        tree.write(PALETTE_CONTAINER, &archive);
        let catalog = FactionPaletteCatalog::discover(&tree.session(), &palette_key())
            .expect("the palette extracts");
        assert_eq!(catalog.findings().len(), 1);
        assert_eq!(catalog.findings()[0].code(), "inconsistent_pattern_palette");
        assert_eq!(catalog.findings()[0].record(), "medbrigand");
    }

    /// AC: the original installation's faction palettes, extracted through the
    /// production reader and VFS, carry the measured colors, decals and
    /// container-absolute spans.
    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_f09_palette_retail_faction_palettes_and_combinations() {
        let root = game_dir();
        let found = install::discover(&root).expect("the installation is discovered");
        let context = ResolveContext::new(install::fingerprint(&found.manifest));
        let mut builder = SessionBuilder::new(context);
        builder
            .mount_installation(&root, &found.diagnosis)
            .expect("the installation mounts");
        let session = builder.open();
        let catalog = FactionPaletteCatalog::discover(&session, &palette_key())
            .expect("the original palette extracts");

        assert_eq!(
            catalog.install_sha256(),
            install::fingerprint(&found.manifest)
        );
        assert_eq!(catalog.container_path(), PALETTE_CONTAINER);
        assert_eq!(
            catalog.faction_names(),
            RETAIL_PALETTE_NAMES,
            "the 11 original faction paint patterns"
        );
        assert_eq!(
            catalog.records().len(),
            28,
            "26 painted records and 2 pattern-only"
        );

        for (faction, colors, decal2, decal3) in RETAIL_PALETTES {
            let palette = catalog
                .palette(faction)
                .unwrap_or_else(|error| panic!("{faction}: {error}"));
            for (slot, expected) in colors.iter().enumerate() {
                let color = palette.color(slot).expect("a stored color");
                assert_eq!(color.rgb(), *expected, "{faction} color {slot}");
                assert_eq!(color.span().container_path(), PALETTE_CONTAINER);
                assert_eq!(color.span().member_key(), Some(PALETTE_MEMBER));
                assert_eq!(color.span().install_sha256(), catalog.install_sha256());
                assert!(color.span().length() > 0);
            }
            assert_eq!(palette.decal(0).expect("decal 1").index(), 21);
            assert_eq!(palette.decal(1).expect("decal 2").index(), decal2);
            assert_eq!(palette.decal(2).expect("decal 3").index(), decal3);
            assert!(!palette.records().is_empty(), "{faction} names no record");
        }

        // The member's own provenance is pinned: offset 1397861, length 97917
        // of `ZBD/zrdr.zbd`, digest of exactly those bytes.
        assert_eq!(catalog.member(), PALETTE_MEMBER);
        assert_eq!(catalog.member_span().offset(), 1_397_861);
        assert_eq!(catalog.member_span().length(), 97_917);
        assert_eq!(
            catalog.member_sha256().to_hex(),
            "d22cabb0038c6bde6481a38992729657a4de60b3d6dbf668c71d29545d671baf"
        );

        // `devastator` and `wingman` name the player pattern but store no
        // colors; the gap is reported, not invented. Those two are the only
        // findings: every same-pattern record must store one palette.
        for name in ["devastator", "wingman"] {
            let record = catalog
                .record(name)
                .expect("the pattern-only record is extracted");
            assert_eq!(record.pattern(), "player_fortune");
            assert!(record.colors().is_empty());
        }
        assert!(catalog.palette("player_fortune").is_err());
        assert_eq!(
            catalog.findings().len(),
            2,
            "the two player gaps are the only findings: {:?}",
            catalog
                .findings()
                .iter()
                .map(|finding| (finding.code(), finding.record()))
                .collect::<Vec<_>>(),
        );
        assert!(
            catalog
                .findings()
                .iter()
                .all(|finding| finding.code() == "pattern_without_colors")
        );
    }

    /// The names of the 11 faction paint patterns, sorted.
    const RETAIL_PALETTE_NAMES: [&str; 11] = [
        "blackhat", "blake", "blckswan", "british", "cccp", "german", "hollywd", "hughes",
        "medusas", "sactrust", "studio",
    ];

    /// The extracted original faction palettes: `(faction, [color1, color2,
    /// color3], decal2, decal3)`. `paint_decal1` is 21 for every record, so it is
    /// asserted once in the test rather than repeated. Measured from
    /// `ZBD/zrdr.zbd [vehicle.zrd]` and pinned so an extraction change that drops
    /// or alters a palette cannot pass.
    const RETAIL_PALETTES: [(&str, [[u8; 3]; 3], u32, u32); 11] = [
        (
            "blackhat",
            [[177, 130, 66], [119, 74, 43], [66, 39, 15]],
            2,
            2,
        ),
        (
            "blake",
            [[149, 163, 195], [89, 114, 159], [233, 228, 240]],
            3,
            3,
        ),
        (
            "blckswan",
            [[23, 23, 21], [48, 47, 39], [196, 193, 186]],
            5,
            5,
        ),
        (
            "british",
            [[177, 130, 66], [48, 47, 39], [255, 255, 255]],
            4,
            4,
        ),
        ("cccp", [[57, 64, 68], [223, 0, 41], [245, 211, 0]], 6, 6),
        ("german", [[96, 115, 126], [0, 0, 0], [48, 47, 39]], 13, 13),
        (
            "hollywd",
            [[108, 102, 169], [67, 36, 121], [212, 202, 225]],
            10,
            9,
        ),
        (
            "hughes",
            [[243, 194, 0], [0, 0, 0], [255, 255, 255]],
            11,
            11,
        ),
        (
            "medusas",
            [[95, 125, 143], [41, 14, 21], [141, 137, 93]],
            14,
            14,
        ),
        (
            "sactrust",
            [[52, 38, 107], [243, 194, 0], [23, 23, 21]],
            15,
            15,
        ),
        (
            "studio",
            [[32, 90, 167], [255, 255, 255], [0, 0, 0]],
            11,
            11,
        ),
    ];
    // ------------------------------------------- paint shop (F09-PAINTSHOP) ---

    /// A ROF directory block: header, 24-byte records (`start`, `raw_length` =
    /// decoded count, `raw_length_on_disk` = stored count, `flags`,
    /// `name_length`, `id`) and the NUL-separated name table, in record order.
    fn rof_block(records: &[[u32; 6]], names: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(records.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(names.len() as u32).to_le_bytes());
        for record in records {
            for word in record {
                bytes.extend_from_slice(&word.to_le_bytes());
            }
        }
        bytes.extend_from_slice(names);
        bytes
    }

    /// The NUL-separated name table of `names`, in record order.
    fn rof_names(names: &[&str]) -> Vec<u8> {
        let mut table = Vec::new();
        for name in names {
            table.extend_from_slice(name.as_bytes());
            table.push(0);
        }
        table
    }

    /// A ROF container holding `members` under the `ASSETS` directory, the shape
    /// `GOSDATA/ASSETS/crimson.rof` has: one root directory record and one
    /// payload per member, stored uncompressed so every member's stored extent
    /// is its decoded extent.
    fn paintshop_rof(members: &[(&str, &[u8])]) -> Vec<u8> {
        let root_names = rof_names(&["ASSETS"]);
        let root = rof_block(
            &[[
                (8 + 24 + root_names.len()) as u32,
                0,
                0,
                1, // directory
                "ASSETS".len() as u32 + 1,
                1,
            ]],
            &root_names,
        );
        let member_names: Vec<&str> = members.iter().map(|(name, _)| *name).collect();
        let sub_names = rof_names(&member_names);
        let mut cursor = (root.len() + 8 + 24 * members.len() + sub_names.len()) as u32;
        let mut records = Vec::new();
        for (index, (name, bytes)) in members.iter().enumerate() {
            let length = u32::try_from(bytes.len()).expect("a fixture payload fits");
            records.push([
                cursor,
                length,
                length,
                0,
                name.len() as u32 + 1,
                10 + u32::try_from(index).expect("a fixture index fits"),
            ]);
            cursor += length;
        }
        let mut container = root;
        container.extend_from_slice(&rof_block(&records, &sub_names));
        for (_, bytes) in members {
            container.extend_from_slice(bytes);
        }
        assert_eq!(
            container.len(),
            cursor as usize,
            "every payload placed once"
        );
        container
    }

    /// One paint-shop dropdown record, spelled the way the original spells it:
    /// the record letter, the two slider arrows, the two drop arrows, then `x`,
    /// `y`, `z`, the item width, the item height and the entry count.
    fn paint_shop_dropdown(stem: &str, slot: Option<u32>, displayed: u32) -> String {
        let key = match slot {
            Some(slot) => format!("{stem}{slot}"),
            None => stem.to_owned(),
        };
        format!(
            "    {key}=D,<PX_SLIDER>,<PX_UP>,<PX_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,<V2>,120,0,\
             <PX_ITEMW>,<STDITEMH>,{displayed}\r\n"
        )
    }

    /// The paint-shop section of a `LAYOUT.CSV` member: the decal pane (when the
    /// caller declares one), the pattern list with `patterns` entries and the
    /// nine per-slot lists, in the member's own order.
    fn paint_shop_layout(decal_pane: Option<&str>, patterns: u32) -> String {
        let mut layout = String::from(";paint shop fixture\r\n[@Paint@]\r\n");
        if let Some(pane) = decal_pane {
            layout.push_str(pane);
        }
        layout.push_str(&paint_shop_dropdown("PT_D_PATTERN", None, patterns));
        for slot in 0..3 {
            layout.push_str(&paint_shop_dropdown("PT_D_COLORS", Some(slot), 18));
        }
        for slot in 0..3 {
            layout.push_str(&paint_shop_dropdown("PT_D_SHADES", Some(slot), 10));
        }
        for slot in 0..3 {
            layout.push_str(&paint_shop_dropdown("PT_D_DECALS", Some(slot), 2));
        }
        layout
    }

    /// The decal pane record the original spells, with the given frame-count
    /// field: `P`, the art member, `x`, `y`, `z`, the frames and three flags.
    fn paint_shop_decal_pane(frames: &str) -> String {
        format!("    PT_P_DECALS=P,PX_P_Decals.tga,0,0,0,{frames},0,2,1\r\n")
    }

    /// A session over `tree` with its `GOSDATA/ASSETS/crimson.rof` mounted
    /// through the production ROF reader, so the layout member resolves under the
    /// container label the dialect inventory routes it by.
    fn paint_shop_session(tree: &PaletteTree) -> (ContentSession, RofSource) {
        let found = install::discover(&tree.0).expect("the fixture installation is discovered");
        let context = ResolveContext::new(install::fingerprint(&found.manifest));
        let mut builder = SessionBuilder::new(context);
        let directory = MountBuilder::new(
            MountId::new("install").expect("a valid mount id"),
            MountNamespace::new(INSTALL_NAMESPACE).expect("a valid namespace"),
            PrecedenceClass::Shared,
            ".",
        )
        .retail();
        builder
            .mount_directory(directory, &tree.0)
            .expect("the fixture installation mounts");
        let archive = MountBuilder::new(
            MountId::new("rof-paint-shop").expect("a valid mount id"),
            MountNamespace::new(INSTALL_NAMESPACE).expect("a valid namespace"),
            PrecedenceClass::Shared,
            PAINT_SHOP_CONTAINER,
        )
        .retail();
        let source = mount_rof_into(&mut builder, archive, &tree.0.join(PAINT_SHOP_CONTAINER))
            .expect("the fixture archive mounts");
        (builder.open(), source)
    }

    /// Builds the fixture tree for one `[@Paint@]` member and returns the
    /// production catalog over it.
    fn paint_shop_catalog(layout: &str) -> PaintShopCatalog {
        let tree = PaletteTree::new();
        tree.write(
            PAINT_SHOP_CONTAINER,
            &paintshop_rof(&[("LAYOUT.CSV", layout.as_bytes())]),
        );
        let (session, source) = paint_shop_session(&tree);
        PaintShopCatalog::discover(&session, &source, PAINT_SHOP_LAYOUT)
            .expect("the fixture layout extracts")
    }

    /// The census every shipped paint-shop control spells: the record letter,
    /// eight `<NAME>` references (the slider, its two arrows, the two drop
    /// arrows, `x`, the item width and the item height) and three whole numbers
    /// (`y`, `z` and the entry count).
    fn plain_control_census() -> PaintShopFieldCensus {
        PaintShopFieldCensus {
            empty: 0,
            placeholder: 8,
            integer: 3,
            hex: 0,
            colour: 0,
            text: 1,
        }
    }

    /// AC: the paint shop's declared option space is read from the layout with
    /// provenance, every value it does not declare is refused as
    /// engine-internal, and an entry past the declared count is refused as not
    /// offered.
    #[test]
    fn accept_f09_paintshop_reads_the_declared_option_space_from_the_layout() {
        let layout = paint_shop_layout(Some(&paint_shop_decal_pane("50")), 12);
        let catalog = paint_shop_catalog(&layout);

        assert_eq!(catalog.container_path(), PAINT_SHOP_CONTAINER);
        assert_eq!(catalog.member(), PAINT_SHOP_LAYOUT);
        assert_eq!(catalog.layout_span().member_key(), Some(PAINT_SHOP_LAYOUT));
        assert_eq!(catalog.layout_span().container_path(), PAINT_SHOP_CONTAINER);
        assert!(
            catalog.layout_span().length() > 0,
            "the member's stored extent is recorded"
        );
        // An uncompressed fixture member's stored and decoded extents are the
        // same bytes, so the two spans agree; a compressed member's do not.
        assert_eq!(catalog.decoded_span(), catalog.layout_span());
        assert_eq!(catalog.stored_len(), layout.len() as u64);
        assert_eq!(catalog.trailing_bytes(), 0);

        // Ten controls: one pattern list and three lists each for colours,
        // shades and decals, with the entry counts the original declares.
        assert_eq!(catalog.controls().len(), 10);
        let counts: Vec<(PaintShopRole, Option<u32>, u32)> = catalog
            .controls()
            .iter()
            .map(|control| (control.role(), control.slot(), control.displayed_entries()))
            .collect();
        assert_eq!(
            counts,
            vec![
                (PaintShopRole::Pattern, None, 12),
                (PaintShopRole::Color, Some(0), 18),
                (PaintShopRole::Color, Some(1), 18),
                (PaintShopRole::Color, Some(2), 18),
                (PaintShopRole::Shade, Some(0), 10),
                (PaintShopRole::Shade, Some(1), 10),
                (PaintShopRole::Shade, Some(2), 10),
                (PaintShopRole::Decal, Some(0), 2),
                (PaintShopRole::Decal, Some(1), 2),
                (PaintShopRole::Decal, Some(2), 2),
            ]
        );
        assert_eq!(
            catalog
                .control(PaintShopRole::Color, Some(1))
                .expect("slot 1 is declared")
                .key(),
            "PT_D_COLORS1"
        );
        assert!(catalog.control(PaintShopRole::Color, Some(3)).is_none());

        // Provenance: every control names its own line in the member, and the
        // member's span is the one the session resolved.
        let lines: Vec<&str> = layout.split("\r\n").collect();
        for control in catalog.controls() {
            assert!(control.line() > 1, "{} has no line", control.key());
            assert_eq!(control.span(), catalog.decoded_span());
            assert!(
                lines
                    .get((control.line() - 1) as usize)
                    .expect("the line is inside the member")
                    .contains(control.key()),
                "{} does not sit on line {}",
                control.key(),
                control.line()
            );
        }

        // The measurable engine-internal claim: the ten records spell the record
        // letter, `<NAME>` references and whole numbers, and nothing else.
        let mut census = PaintShopFieldCensus::default();
        for control in catalog.controls() {
            let own = control.field_census();
            assert_eq!(own.total(), 12, "{} has 12 fields", control.key());
            census.empty += own.empty;
            census.placeholder += own.placeholder;
            census.integer += own.integer;
            census.hex += own.hex;
            census.colour += own.colour;
            census.text += own.text;
        }
        assert_eq!(
            census,
            PaintShopFieldCensus {
                empty: 0,
                placeholder: 80,
                integer: 30,
                hex: 0,
                colour: 0,
                text: 10,
            },
            "ten dropdown records of 12 fields: 8 references, 3 whole numbers and the record \
             letter apiece, and no colour, hex or name value"
        );

        // The decal sheet is declared, with its art member and frame count.
        let sheet = catalog
            .decal_sheet()
            .expect("the decal pane is declared")
            .clone();
        assert_eq!(sheet.key(), "PT_P_DECALS");
        assert_eq!(sheet.art(), "PX_P_Decals.tga");
        assert_eq!(sheet.frames(), 50);
        assert_eq!(sheet.span(), catalog.decoded_span());
        assert!(sheet.line() > 1);

        // The four gaps are recorded, each with the count and the census that
        // establishes the absence.
        assert_eq!(
            catalog
                .gaps()
                .iter()
                .map(|gap| gap.code())
                .collect::<Vec<_>>(),
            vec![SWATCH_GAP, SHADE_GAP, PATTERN_NAME_GAP, DECAL_GAP]
        );
        for gap in catalog.gaps() {
            assert_eq!(gap.field_census().colour, 0, "{}", gap.code());
            assert_eq!(gap.field_census().literals(), 1, "{}", gap.code());
            assert!(!gap.detail().is_empty());
            assert!(!gap.affected().is_empty());
            assert!(gap.displayed_entries() > 0, "{}", gap.code());
        }
        assert_eq!(
            catalog
                .gap(SWATCH_GAP)
                .expect("the swatch gap")
                .displayed_entries(),
            18
        );
        assert_eq!(
            catalog
                .gap(SHADE_GAP)
                .expect("the shade gap")
                .displayed_entries(),
            10
        );
        assert_eq!(
            catalog
                .gap(PATTERN_NAME_GAP)
                .expect("the name gap")
                .displayed_entries(),
            12
        );
        assert_eq!(
            catalog
                .gap(DECAL_GAP)
                .expect("the decal gap")
                .displayed_entries(),
            2
        );

        // A swatch in range is engine-internal, never a colour; past the
        // declared count the shop offers no such entry.
        assert_eq!(
            catalog.color(0, 0).expect("slot 0 is declared"),
            PaintShopValue::EngineInternal {
                code: SWATCH_GAP,
                displayed: 18,
                census: plain_control_census(),
            }
        );
        assert_eq!(
            catalog.color(2, 17).expect("slot 2 is declared"),
            PaintShopValue::EngineInternal {
                code: SWATCH_GAP,
                displayed: 18,
                census: plain_control_census(),
            }
        );
        assert_eq!(
            catalog.color(0, 18).expect("slot 0 is declared"),
            PaintShopValue::NotOffered {
                displayed: 18,
                index: 18
            }
        );
        assert_eq!(
            catalog.shade(1, 9).expect("slot 1 is declared"),
            PaintShopValue::EngineInternal {
                code: SHADE_GAP,
                displayed: 10,
                census: plain_control_census(),
            }
        );
        assert_eq!(
            catalog.decal(2, 1).expect("slot 2 is declared"),
            PaintShopValue::EngineInternal {
                code: DECAL_GAP,
                displayed: 2,
                census: plain_control_census(),
            }
        );
        assert_eq!(
            catalog
                .pattern_name(11)
                .expect("the pattern list is declared"),
            PaintShopValue::EngineInternal {
                code: PATTERN_NAME_GAP,
                displayed: 12,
                census: plain_control_census(),
            }
        );
        // A control the layout does not declare is a refusal, not a zero.
        assert_eq!(
            catalog.color(3, 0).expect_err("slot 3 is not declared"),
            PaintShopRefusal::NoControl {
                role: PaintShopRole::Color,
                slot: Some(3)
            }
        );
        assert_eq!(
            catalog
                .pattern_name(12)
                .expect("the pattern list is declared"),
            PaintShopValue::NotOffered {
                displayed: 12,
                index: 12
            }
        );
    }

    /// AC: a layout without the paint-shop section, a control that is not the
    /// record kind its role requires, a control with a truncated record, a
    /// control whose entry count is not a whole number, a per-slot key whose
    /// digit is not a paint slot and an unreadable decal frame count are each
    /// refused with their own code.
    #[test]
    fn accept_f09_paintshop_refuses_a_layout_or_control_it_cannot_read() {
        // No `[@Paint@]` section at all.
        let tree = PaletteTree::new();
        tree.write(
            PAINT_SHOP_CONTAINER,
            &paintshop_rof(&[("LAYOUT.CSV", b"[@Hangar@]\r\n    PT_D_COLORS0=D,x\r\n")]),
        );
        let (session, source) = paint_shop_session(&tree);
        let error = PaintShopCatalog::discover(&session, &source, PAINT_SHOP_LAYOUT)
            .expect_err("a layout without the paint-shop section is refused");
        assert_eq!(error.code(), "missing_section");
        assert!(error.to_string().contains("[@Paint@]"));

        // A control spelled as a pane instead of a dropdown.
        let layout = paint_shop_layout(None, 12).replace(
            "    PT_D_PATTERN=D,",
            "    PT_D_PATTERN=P,PX_P_Decals.tga,0,0,0,50,0,2,1,0,0,0\r\n    PT_X_PATTERN=D,",
        );
        assert!(
            layout.contains("PT_X_PATTERN=D,"),
            "the fixture replaced the record"
        );
        let (session, source) = paint_shop_session(&paint_shop_tree(&layout));
        let error = PaintShopCatalog::discover(&session, &source, PAINT_SHOP_LAYOUT)
            .expect_err("a pane under a paint-shop stem is refused");
        assert_eq!(error.code(), "wrong_kind", "{error}");
        assert!(error.to_string().contains("PT_D_PATTERN"));

        // A control whose record is one field short.
        let layout = paint_shop_layout(None, 12)
            .replace(",<PX_ITEMW>,<STDITEMH>,12\r\n", ",<PX_ITEMW>,12\r\n");
        let (session, source) = paint_shop_session(&paint_shop_tree(&layout));
        let error = PaintShopCatalog::discover(&session, &source, PAINT_SHOP_LAYOUT)
            .expect_err("a truncated control is refused");
        assert_eq!(error.code(), "short_record");
        assert!(error.to_string().contains("11 fields"));

        // An entry count that is not a whole number stays unknown.
        let layout =
            paint_shop_layout(None, 12).replace("<STDITEMH>,12\r\n", "<STDITEMH>,<NINE>\r\n");
        let (session, source) = paint_shop_session(&paint_shop_tree(&layout));
        let error = PaintShopCatalog::discover(&session, &source, PAINT_SHOP_LAYOUT)
            .expect_err("a placeholder entry count is refused");
        assert_eq!(error.code(), "entries_unreadable");
        assert!(error.to_string().contains("PT_D_PATTERN"));

        // A per-slot key whose digit is not a paint slot.
        let layout = paint_shop_layout(None, 12).replace("PT_D_DECALS2=", "PT_D_DECALS7=");
        let (session, source) = paint_shop_session(&paint_shop_tree(&layout));
        let error = PaintShopCatalog::discover(&session, &source, PAINT_SHOP_LAYOUT)
            .expect_err("a slot digit outside the shop's slots is refused");
        assert_eq!(error.code(), "unknown_slot");
        assert!(error.to_string().contains("PT_D_DECALS7"));

        // A decal pane whose frame count is not a whole number.
        let layout = paint_shop_layout(Some(&paint_shop_decal_pane("<FRAMES>")), 12);
        let (session, source) = paint_shop_session(&paint_shop_tree(&layout));
        let error = PaintShopCatalog::discover(&session, &source, PAINT_SHOP_LAYOUT)
            .expect_err("an unreadable frame count is refused");
        assert_eq!(error.code(), "frames_unreadable");
        assert!(error.to_string().contains("PT_P_DECALS"));
    }

    /// A fixture tree holding one `[@Paint@]` member with the given layout.
    fn paint_shop_tree(layout: &str) -> PaletteTree {
        let tree = PaletteTree::new();
        tree.write(
            PAINT_SHOP_CONTAINER,
            &paintshop_rof(&[("LAYOUT.CSV", layout.as_bytes())]),
        );
        tree
    }

    /// AC: a control that does spell a colour literal is measured, its gap is
    /// **not** recorded, and the cross-check reports the control instead. This is
    /// what keeps the engine-internal claim derived from the data rather than
    /// asserted.
    #[test]
    fn accept_f09_paintshop_reports_a_control_that_stores_a_value() {
        let layout = paint_shop_layout(Some(&paint_shop_decal_pane("50")), 12).replace(
            "    PT_D_COLORS0=D,<PX_SLIDER>,",
            "    PT_D_COLORS0=D,0xff102030,",
        );
        let (session, source) = paint_shop_session(&paint_shop_tree(&layout));
        let catalog = PaintShopCatalog::discover(&session, &source, PAINT_SHOP_LAYOUT)
            .expect("the fixture layout extracts");

        // The colour literal is measured, and the ten controls are still ten.
        assert_eq!(catalog.controls().len(), 10);
        let control = catalog
            .control(PaintShopRole::Color, Some(0))
            .expect("slot 0 is declared");
        assert_eq!(
            control.field_census(),
            PaintShopFieldCensus {
                empty: 0,
                placeholder: 7,
                integer: 3,
                hex: 0,
                colour: 1,
                text: 1,
            },
            "the colour literal is measured, not assumed"
        );
        // The value is now readable, so the swatch gap must not be recorded: a
        // gap that survived a control that stores its values would be a lie.
        assert!(
            catalog.gap(SWATCH_GAP).is_none(),
            "a control that stores a colour literal is not engine-internal"
        );
        assert!(catalog.gap(SHADE_GAP).is_some());
    }

    /// AC: the cross-check closes the shop's pattern list against the vehicle
    /// records' stored patterns, keeps the player's uncoloured pattern a
    /// finding, and bounds every stored decal index by the declared sheet.
    #[test]
    fn accept_f09_paintshop_cross_check_closes_patterns_and_bounds_decals() {
        // One faction pattern with a palette and the player's pattern without
        // one, matching the original's shape.
        let medusas = zrd_paint_record(
            "medusas",
            [[95, 125, 143], [41, 14, 21], [141, 137, 93]],
            [21, 14, 14],
        );
        let player = zrd_pattern_only_record("player_fortune");
        let zrd = vehicle_zrd(&[("medkestrel", &medusas), ("devastator", &player)]);
        let tree = paint_shop_tree(&paint_shop_layout(Some(&paint_shop_decal_pane("50")), 2));
        tree.write(
            PALETTE_CONTAINER,
            &palette_reader_archive(&[(PALETTE_MEMBER, &zrd)]),
        );
        let (session, source) = paint_shop_session(&tree);
        let palette = FactionPaletteCatalog::discover(&session, &palette_key())
            .expect("the palette extracts");
        let catalog = PaintShopCatalog::discover(&session, &source, PAINT_SHOP_LAYOUT)
            .expect("the shop reads");

        // A pattern list with one entry per stored pattern agrees, and only the
        // pattern no record colours is reported.
        let findings = catalog.cross_check(&palette);
        assert_eq!(
            findings
                .iter()
                .map(|finding| finding.code())
                .collect::<Vec<_>>(),
            vec!["pattern_without_palette"],
            "the list declares 2 entries and the records name 2 patterns, so they agree"
        );
        assert!(findings[0].detail().contains("player_fortune"));

        // A declared list that does not match the stored pattern count is a
        // finding, not a silently accepted agreement.
        let layout = paint_shop_layout(Some(&paint_shop_decal_pane("50")), 3);
        let tree = paint_shop_tree(&layout);
        tree.write(
            PALETTE_CONTAINER,
            &palette_reader_archive(&[(PALETTE_MEMBER, &zrd)]),
        );
        let (session, source) = paint_shop_session(&tree);
        let catalog = PaintShopCatalog::discover(&session, &source, PAINT_SHOP_LAYOUT)
            .expect("the shop reads");
        let findings = catalog.cross_check(&palette);
        assert!(
            findings.iter().any(|finding| {
                finding.code() == "pattern_count_mismatch"
                    && finding.detail().contains("3")
                    && finding.detail().contains("2")
            }),
            "{:?}",
            findings
                .iter()
                .map(|finding| (finding.code(), finding.detail().to_owned()))
                .collect::<Vec<_>>()
        );

        // Every stored decal index is inside the declared sheet; a decal past the
        // declared frame count is a finding.
        let tree = paint_shop_tree(&paint_shop_layout(Some(&paint_shop_decal_pane("50")), 2));
        tree.write(
            PALETTE_CONTAINER,
            &palette_reader_archive(&[(PALETTE_MEMBER, &zrd)]),
        );
        let (session, source) = paint_shop_session(&tree);
        let catalog = PaintShopCatalog::discover(&session, &source, PAINT_SHOP_LAYOUT)
            .expect("the shop reads");
        assert!(
            !catalog
                .cross_check(&palette)
                .iter()
                .any(|finding| finding.code() == "decal_outside_sheet"),
            "decal 21 is inside the 50 declared frames"
        );
        let tree = paint_shop_tree(&paint_shop_layout(Some(&paint_shop_decal_pane("20")), 12));
        tree.write(
            PALETTE_CONTAINER,
            &palette_reader_archive(&[(PALETTE_MEMBER, &zrd)]),
        );
        let (session, source) = paint_shop_session(&tree);
        let catalog = PaintShopCatalog::discover(&session, &source, PAINT_SHOP_LAYOUT)
            .expect("the shop reads");
        let findings = catalog.cross_check(&palette);
        let outside: Vec<&str> = findings
            .iter()
            .filter(|finding| finding.code() == "decal_outside_sheet")
            .map(PaintShopFinding::detail)
            .collect();
        assert_eq!(outside.len(), 1, "{outside:?}");
        assert!(
            outside[0].contains("21") && outside[0].contains("20 frames"),
            "{outside:?}"
        );

        // A layout that declares no decal pane leaves the sheet unknown.
        let tree = paint_shop_tree(&paint_shop_layout(None, 12));
        tree.write(
            PALETTE_CONTAINER,
            &palette_reader_archive(&[(PALETTE_MEMBER, &zrd)]),
        );
        let (session, source) = paint_shop_session(&tree);
        let catalog = PaintShopCatalog::discover(&session, &source, PAINT_SHOP_LAYOUT)
            .expect("the shop reads");
        assert!(catalog.decal_sheet().is_none());
        assert!(
            catalog
                .cross_check(&palette)
                .iter()
                .any(|finding| finding.code() == "no_decal_sheet")
        );
    }

    /// The retail option space and the four engine-internal gaps, measured
    /// through the production readers against the original installation.
    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_f09_paintshop_retail_option_space_and_recorded_gaps() {
        let root = game_dir();
        let found = install::discover(&root).expect("the installation is discovered");
        let context = ResolveContext::new(install::fingerprint(&found.manifest));
        let mut builder = SessionBuilder::new(context);
        builder
            .mount_installation(&root, &found.diagnosis)
            .expect("the installation mounts");
        let archive = MountBuilder::new(
            MountId::new("rof-gosdata-assets-crimson-rof").expect("a valid mount id"),
            MountNamespace::new(INSTALL_NAMESPACE).expect("a valid namespace"),
            PrecedenceClass::Shared,
            PAINT_SHOP_CONTAINER,
        )
        .retail();
        let source = mount_rof_into(&mut builder, archive, &root.join(PAINT_SHOP_CONTAINER))
            .expect("the airframe library mounts");
        let session = builder.open();

        let catalog = PaintShopCatalog::discover(&session, &source, PAINT_SHOP_LAYOUT)
            .expect("the original paint shop extracts");
        assert_eq!(catalog.container_path(), PAINT_SHOP_CONTAINER);
        assert_eq!(catalog.member(), PAINT_SHOP_LAYOUT);
        assert_eq!(
            catalog.install_sha256(),
            install::fingerprint(&found.manifest)
        );
        // The layout member is stored compressed, so its stored and decoded
        // extents differ and both spans are recorded; the decoder consumed the
        // stored extent exactly.
        assert_eq!(catalog.stored_len(), 15_078);
        assert_eq!(catalog.layout_span().length(), 15_078);
        assert_eq!(catalog.decoded_span().length(), 56_148);
        assert_ne!(
            catalog.decoded_span().member_sha256(),
            catalog.layout_span().member_sha256(),
            "a stored extent and a decoded extent do not hash the same"
        );
        assert_eq!(catalog.trailing_bytes(), 0);
        assert_eq!(catalog.layout_span().offset(), 37_359);
        assert_eq!(catalog.decoded_span().offset(), 37_359);

        // The ten controls and their declared entry counts, as the original
        // layout spells them.
        /// One retail control row: the record key, its paint slot, the entry
        /// count it declares, the line it sits on and how many of its twelve
        /// fields are `<NAME>` references and whole numbers.
        type RetailControlRow = (String, Option<u32>, u32, u64, u32, u32);

        // `(key, slot, displayed entries, line, placeholders, whole numbers)`
        let counts: Vec<RetailControlRow> = catalog
            .controls()
            .iter()
            .map(|control| {
                let census = control.field_census();
                (
                    control.key().to_owned(),
                    control.slot(),
                    control.displayed_entries(),
                    control.line(),
                    census.placeholder,
                    census.integer,
                )
            })
            .collect();
        assert_eq!(
            counts,
            vec![
                ("PT_D_PATTERN".to_owned(), None, 12, 826, 8, 3),
                ("PT_D_COLORS0".to_owned(), Some(0), 18, 827, 7, 4),
                ("PT_D_COLORS1".to_owned(), Some(1), 18, 828, 7, 4),
                ("PT_D_COLORS2".to_owned(), Some(2), 18, 829, 7, 4),
                ("PT_D_SHADES0".to_owned(), Some(0), 10, 830, 7, 4),
                ("PT_D_SHADES1".to_owned(), Some(1), 10, 831, 7, 4),
                ("PT_D_SHADES2".to_owned(), Some(2), 10, 832, 7, 4),
                ("PT_D_DECALS0".to_owned(), Some(0), 2, 833, 5, 6),
                ("PT_D_DECALS1".to_owned(), Some(1), 2, 834, 5, 6),
                ("PT_D_DECALS2".to_owned(), Some(2), 2, 835, 5, 6),
            ],
            "the paint shop's ten dropdowns, the entry counts the original declares and how \
             each record spells its fields"
        );

        // The engine-internal claim, measured: the ten records spell the record
        // letter, `<NAME>` references and whole numbers — no colour, no hex and
        // no name value anywhere in the shop's option space.
        let mut census = PaintShopFieldCensus::default();
        for control in catalog.controls() {
            let own = control.field_census();
            assert_eq!(own.total(), 12, "{}", control.key());
            census.empty += own.empty;
            census.placeholder += own.placeholder;
            census.integer += own.integer;
            census.hex += own.hex;
            census.colour += own.colour;
            census.text += own.text;
        }
        assert_eq!(
            census,
            PaintShopFieldCensus {
                empty: 0,
                placeholder: 65,
                integer: 45,
                hex: 0,
                colour: 0,
                text: 10,
            },
            "the pattern list spells two positions as <NAME> references and the nine per-slot \
             lists spell three or none, but no record spells a colour, a hex value or a name"
        );
        assert_eq!(
            catalog
                .gaps()
                .iter()
                .map(|gap| gap.code())
                .collect::<Vec<_>>(),
            vec![SWATCH_GAP, SHADE_GAP, PATTERN_NAME_GAP, DECAL_GAP]
        );

        // The decal sheet the layout declares.
        let sheet = catalog.decal_sheet().expect("the decal pane is declared");
        assert_eq!(sheet.art(), "PX_P_Decals.tga");
        assert_eq!(sheet.frames(), 50);
        assert_eq!(sheet.line(), 802);
        assert_eq!(
            sheet.span().member_key(),
            Some(PAINT_SHOP_LAYOUT),
            "the decal sheet's art member is provenance, not a decoded image"
        );

        // A stored decal index is inside the declared sheet, and the shop's own
        // decal list offers two entries per slot.
        let palette = FactionPaletteCatalog::discover(&session, &palette_key())
            .expect("the palette extracts");
        assert!(
            !catalog
                .cross_check(&palette)
                .iter()
                .any(|finding| finding.code() == "decal_outside_sheet"),
            "every stored decal index (2..=21) is inside the 50 declared frames"
        );
        assert_eq!(
            catalog.decal(0, 1).expect("slot 0 is declared"),
            PaintShopValue::EngineInternal {
                code: DECAL_GAP,
                displayed: 2,
                // The retail decal record spells `x`, `y`, `z` and the item
                // geometry as whole numbers, so its census differs from the
                // fixture's; the colour count is `0` either way.
                census: PaintShopFieldCensus {
                    empty: 0,
                    placeholder: 5,
                    integer: 6,
                    hex: 0,
                    colour: 0,
                    text: 1,
                },
            }
        );
        assert_eq!(
            catalog.decal(0, 2).expect("slot 0 is declared"),
            PaintShopValue::NotOffered {
                displayed: 2,
                index: 2
            }
        );

        // The player's own paint stays a recorded gap: the shop offers exactly
        // as many patterns as the vehicle records name, and the pattern those
        // records leave uncoloured is the one finding.
        let findings = catalog.cross_check(&palette);
        assert_eq!(
            findings
                .iter()
                .map(|finding| finding.code())
                .collect::<Vec<_>>(),
            vec!["pattern_without_palette"],
            "{:?}",
            findings
                .iter()
                .map(|finding| (finding.code(), finding.detail().to_owned()))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            catalog
                .control(PaintShopRole::Pattern, None)
                .expect("the pattern list is declared")
                .displayed_entries() as usize,
            palette
                .records()
                .iter()
                .map(|record| record.pattern())
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            "the shop's pattern list has one entry per paint pattern the vehicle records name"
        );
    }
}
/// Evidence-report harnesses for tasks F09-D and F09-PALETTE
/// (`docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`).
///
/// The F09-D harness below is deliberately **not** named `accept_f09_d_*`: it is
/// not part of the acceptance suite, it fails loudly when its inputs are missing
/// instead of passing vacuously, and the `accept_f09_d_` selection must never
/// pick it up. The F09-PALETTE harness that follows the same rule is described
/// at its own definition. Both live in an owner path; F09-D and F09-PALETTE own
/// no `crates/cs_content/tests/` file, so they live here, beside the production
/// code they measure.
///
/// Run from the workspace root, after the acceptance suite, exactly as:
///
/// 1. ```sh
///    cargo test --workspace --locked -- accept_f09_d_ --include-ignored \
///      > private/evidence/F09-D/cargo-test.log 2>&1
///    ```
///    (record that command's exit status — it is passed to this harness as
///    `CS_EVIDENCE_EXIT_CODE`.)
/// 2. ```sh
///    CS_EVIDENCE_DIR=private/evidence/F09-D \
///    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
///    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f09_d_ --include-ignored" \
///    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
///      cargo test --locked -p cs_content --lib evidence_report_f09_d -- --ignored
///    ```
/// 3. ```sh
///    python3 tools/validate_evidence.py \
///      private/evidence/F09-D/acceptance.json \
///      --artifact-root private/evidence/F09-D --require-pass
///    ```
/// 4. Commit a copy of `acceptance.json` as
///    `docs/findings/evidence/F09-D.json`.
///
/// Every field is derived from real inputs: the recorded test log, the
/// environment, `rustc --version` and `Cargo.lock`, the production installation
/// discovery and fingerprint of `$CS_GAME_DIR`, and the production
/// [`StockLiveryCatalog::discover`] run over the airframe library. The
/// `unknowns` array is empty because the report makes no unresolved issue: the
/// inventory, the faction and prefix sets and every combination are measured
/// from the original library. The scope boundaries this stage does not resolve
/// are **not** dropped to satisfy that gate: each is named in the report's
/// `review.method` (see [`DEFERRED_BOUNDARIES`]), with its affected content and
/// its resolving Rally task, tracked by its own task and written down in
/// `docs/findings/2026-09-29-f09-d-stock-liveries-and-combinations.md`, so they
/// outlive this task. A failing acceptance run writes an honestly failing
/// report the validator rejects.
#[cfg(test)]
mod evidence {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    use cs_assets::install;
    use cs_assets::rof::{RofSource, mount_rof_into};
    use cs_assets::vfs::{INSTALL_NAMESPACE, MountBuilder, SessionBuilder};
    use cs_formats::bm::{BmFile, BmPlane};
    use cs_formats::{ParseContext, read_bm};
    use cs_types::asset_id::{AssetKey, MountId, MountNamespace, PrecedenceClass, ResolveContext};

    use super::{FactionPaletteCatalog, PALETTE_CONTAINER, PALETTE_MEMBER, StockLiveryCatalog};

    /// The shared airframe library, spelled as the installation does.
    const CONTAINER: &str = "GOSDATA/ASSETS/crimson.rof";

    /// The two retail acceptance tests, which together are this task's `retail`
    /// capability. Both must be in the recorded log and both must have passed.
    const RETAIL_TESTS: [&str; 2] = [
        "accept_f09_d_retail_stock_liveries_and_combinations",
        "accept_f09_d_retail_composed_liveries_match_the_pinned_reference",
    ];

    /// The measured corpus the report asserts against the production catalog, so
    /// a catalog change that discovers fewer members cannot silently pass.
    const EXPECTED_ASSETS: usize = 184;
    const EXPECTED_FACTIONS: usize = 14;
    const EXPECTED_PREFIXES: usize = 11;
    const EXPECTED_COMBINATIONS: usize = 33;

    /// Every scope boundary this stage does not resolve, each naming the
    /// affected content and the resolving Rally task, and stating the fidelity
    /// claim it gates. These are follow-up limitations, not unresolved issues
    /// with this report's `implemented` claim: the inventory and every
    /// combination are measured from the original library, and each boundary is
    /// tracked by its own Rally task so it survives this task being done. They
    /// are recorded in the report's `review.method` (the `unknowns` gate is
    /// reserved for unresolved issues with the claim, of which the report has
    /// none) and in the committed finding.
    const DEFERRED_BOUNDARIES: [&str; 4] = [
        "retail_composition_agreement: the original renderer's exact mask \
         weights, rounding and overlay alpha are not established; the production \
         composition matches the pinned S09/S10 tool reference at every texel of \
         184 members x (stock + one private paint), which is agreement between two \
         readers, not the original game. Affected content: every composed livery. \
         Resolving task: F17-D (needs the F17-B GPU consumer and an owner-run \
         capture). Gates: any verified_original or release claim about livery \
         appearance.",
        "retail_faction_palette: the pinned reference paints with S10's \
         FACTION_COLORS research lead; no original-data palette has been \
         extracted, so the real faction colors are unknown (F09 non-negotiable \
         #4). Affected content: every faction's base/mask colors on every \
         composed livery. Resolving task: F09-PALETTE (Rally #385). Gates: any \
         palette or faction-color fidelity claim.",
        "prefix_airframe_mapping: the 11 livery prefixes are the original names' \
         own, but which airframe each prefix names is not established from \
         original data (F09 non-negotiable #3). Affected content: the airframe \
         identity of every stock livery. Resolving task: F09-PREFIX (Rally \
         #386).",
        "on_screen_several_angles: AC04's literal comparison of a private painted \
         aircraft from several angles against the original is not performed; \
         there is no GPU consumer for a composed ComposedLivery and no owner \
         capture (human_play/human_review). Affected content: any visual fidelity \
         claim about a painted aircraft. The texel-level comparison against the \
         pinned tool reference is the only livery composition evidence this \
         stage has. Resolving task: F17-D (consumer F17-B). Gates: F63-D and any \
         release or visual-fidelity approval.",
    ];

    /// The retail acceptance test that is this task's `retail` capability. It
    /// must be in the recorded log and must have passed.
    const PALETTE_RETAIL_TEST: &str = "accept_f09_palette_retail_faction_palettes_and_combinations";

    /// The measured palette corpus the report asserts against the production
    /// extraction, so a change that discovers fewer factions cannot pass.
    const PALETTE_EXPECTED_FACTIONS: usize = 11;
    const PALETTE_EXPECTED_RECORDS: usize = 28;

    /// Every limitation this stage records machine-readably, each naming the
    /// content it affects and the Rally task that resolves it. The `unknowns`
    /// gate in `tools/validate_evidence.py` rejects a nonempty array under
    /// `--require-pass`; deleting these to turn the flag green would be exactly
    /// what the contract forbids (the F12-G and F12-H reports give the same
    /// reason), so this report is validated **without** `--require-pass` and the
    /// expected rejection is documented in the committed finding.
    const PALETTE_UNKNOWNS: [&str; 5] = [
        "player_palette: `player_fortune` — the player's own paint — has no stored \
         palette: the `devastator` and `wingman` vehicle records name the pattern but \
         store no color or decal triple. Affected content: the player paint on every \
         airframe. Resolving task: F09-PAINTSHOP (Rally #485), which owns the \
         paint-shop swatch source (PAINT.SCRIPT native callbacks 2236/2237/2229/2239 \
         and LAYOUT.CSV [@Paint@]).",
        "missing_faction_palettes: the factions BROADWAY and ITSTAXI have stock BM \
         livery directories (the F09-D inventory) but no paint-bearing `vehicle.zrd` \
         record, so their base/mask colors are not in this source. Affected content: \
         those two factions' colors on every composed livery. Resolving task: \
         F09-PAINTSHOP (Rally #485).",
        "decal_index_semantics: the meaning of the `paint_decal` indices (values 2..21; \
         `paint_decal1` is 21 for every record) is unmeasured — whether an index names \
         a cell of the PX_P_DECALS sheet or an engine decal id is unknown. Affected \
         content: every decal selection on every composed livery. Resolving task: \
         F09-PAINTSHOP (Rally #485).",
        "shade_multipliers: the shade multipliers the paint shop applies to palette \
         colors are unknown; LAYOUT.CSV's [@Paint@] shade dropdowns give counts (10 \
         entries), not values, and the values come from native callback 2236. Affected \
         content: every shaded paint variant. Resolving task: F09-PAINTSHOP (Rally \
         #485).",
        "slot_to_mask_mapping: the three stored `paint_color` slots are exposed in \
         stored order; that slot 1/2/3 corresponds to the BM Mask1/Mask2/Mask3 planes \
         is inferred from order, not established from an original render. Affected \
         content: the mask assignment of every composed livery. Resolving task: \
         F17-D (composition agreement; F09-D DEFERRED_BOUNDARIES), with the \
         paint-shop semantics from F09-PAINTSHOP (Rally #485).",
    ];

    /// The mask/overlay-alpha endpoint coverage of the whole library, measured
    /// through the production `BmFile` accessors. Pinned so a corpus whose
    /// endpoints are never exercised cannot pass silently.
    const MASK_ZERO: u64 = 6_092_775;
    const MASK_FULL: u64 = 2_786_275;
    const MASK_PARTIAL: u64 = 318_518;
    const ALPHA_ZERO: u64 = 1_805_690;
    const ALPHA_FULL: u64 = 18_328;
    const ALPHA_PARTIAL: u64 = 1_241_838;

    /// How many mask bytes and overlay-alpha values are at the `0` and `255`
    /// endpoints, and in between, over a corpus.
    #[derive(Clone, Copy, Debug, Default)]
    struct EndpointTally {
        mask_zero: u64,
        mask_full: u64,
        mask_partial: u64,
        alpha_zero: u64,
        alpha_full: u64,
        alpha_partial: u64,
    }

    /// Adds one parsed member's masks and overlay alpha to `tally`.
    fn tally_endpoints(file: &BmFile<'_>, tally: &mut EndpointTally) {
        for y in 0..file.height() {
            for x in 0..file.width() {
                for plane in [BmPlane::Mask1, BmPlane::Mask2, BmPlane::Mask3] {
                    match file.mask(plane, x, y).expect("inside the image") {
                        0 => tally.mask_zero += 1,
                        255 => tally.mask_full += 1,
                        _ => tally.mask_partial += 1,
                    }
                }
                match file.overlay(x, y).expect("inside the image")[3] {
                    0 => tally.alpha_zero += 1,
                    255 => tally.alpha_full += 1,
                    _ => tally.alpha_partial += 1,
                }
            }
        }
    }

    /// The key of one member spelling in `source`.
    fn member_key(source: &RofSource, spelling: &str) -> AssetKey {
        AssetKey::from_spelling(source.namespace().as_str(), spelling, "default")
            .expect("a valid member key")
    }

    #[test]
    #[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
    fn evidence_report_f09_d_writes_the_acceptance_report() {
        let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
        let candidate_tree = env_var("CS_CANDIDATE_TREE");
        let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        assert!(
            !argv.is_empty(),
            "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
        );
        let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
            .parse()
            .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
        let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

        // The candidate tree must be the tree that was actually tested: a stale
        // report from another commit is exactly what this check refuses.
        let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
        assert_eq!(
            candidate_tree, head_tree,
            "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
             old reports cannot be reused for new code"
        );

        // The acceptance suite is the evidence: parse its recorded output.
        let log_path = evidence_dir.join("cargo-test.log");
        let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
            panic!(
                "cannot read the acceptance log {}: {error} (step 1 must write its output there)",
                log_path.display()
            )
        });
        let suite = parse_suite(&log);
        assert!(
            suite.passed > 0 && !suite.assertions.is_empty(),
            "no `accept_f09_d_` tests were recorded in {}",
            log_path.display()
        );

        // Capability coverage is checked, never assumed: `retail` is declared
        // only because both retail acceptance tests are in this log and passed.
        for retail_test in RETAIL_TESTS {
            let status = suite
                .assertions
                .iter()
                .find(|(name, _)| name == retail_test)
                .map(|(_, status)| *status)
                .unwrap_or_else(|| {
                    panic!(
                        "{retail_test} did not run: F09-D requires capability `retail`, run step \
                         1 with `--include-ignored` and CS_GAME_DIR set"
                    )
                });
            assert_eq!(
                status, "pass",
                "{retail_test} must pass; got status {status}"
            );
        }
        assert!(
            suite
                .assertions
                .iter()
                .any(|(name, _)| name.starts_with("accept_f09_d_") && !name.contains("_retail_")),
            "synthetic task tests must be present alongside the retail ones"
        );

        // The installation and content hashes come from the **production**
        // discovery and fingerprint code (F02), not from a hash this harness
        // computes, so they are comparable with every earlier task's record.
        let found = install::discover(&game_dir).expect(
            "production discovery must read the original installation for the evidence record",
        );
        let install_sha256 = install::fingerprint(&found.manifest).to_hex();
        let content_sha256 = install::content_fingerprint(&found.manifest).to_hex();

        // The substantive measurement: the production catalog over the whole
        // airframe library, asserted against the pinned corpus and written beside
        // the report as a digest-and-count artifact.
        let source = retail_source(&game_dir, &found);
        let catalog = StockLiveryCatalog::discover(&source);
        assert!(
            catalog.findings().is_empty(),
            "an airframe-library member is unsupported: {:?}",
            catalog
                .findings()
                .iter()
                .map(|finding| (finding.spelling().to_owned(), finding.code()))
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            catalog.assets().len(),
            EXPECTED_ASSETS,
            "the original airframe library holds {EXPECTED_ASSETS} stock livery members"
        );
        assert_eq!(catalog.factions().len(), EXPECTED_FACTIONS);
        assert_eq!(catalog.prefixes().len(), EXPECTED_PREFIXES);
        let combinations: usize = catalog
            .combinations()
            .iter()
            .map(|combination| combination.factions().len())
            .sum();
        assert_eq!(
            combinations, EXPECTED_COMBINATIONS,
            "every prefix x faction combination the library stores"
        );

        // Every member is exactly the covered length with no tail, and the
        // corpus really exercises both endpoints and the interior of the masks
        // and the overlay alpha; the pinned tallies are asserted, not quoted.
        let mut tally = EndpointTally::default();
        for asset in catalog.assets() {
            assert_eq!(
                asset.tail_bytes(),
                0,
                "{} has bytes outside the observed subset",
                asset.spelling()
            );
            assert_eq!(
                asset.covered_len(),
                4 + 10 * u64::from(asset.width()) * u64::from(asset.height()),
                "{}",
                asset.spelling()
            );
            let read = source
                .read(&member_key(&source, asset.spelling()))
                .unwrap_or_else(|error| panic!("{}: {error}", asset.spelling()));
            let mut context = ParseContext::with_defaults(asset.spelling());
            let file = read_bm(&mut context, &read.data)
                .unwrap_or_else(|error| panic!("{}: {error}", asset.spelling()));
            tally_endpoints(&file, &mut tally);
        }
        assert_eq!(tally.mask_zero, MASK_ZERO);
        assert_eq!(tally.mask_full, MASK_FULL);
        assert_eq!(tally.mask_partial, MASK_PARTIAL);
        assert_eq!(tally.alpha_zero, ALPHA_ZERO);
        assert_eq!(tally.alpha_full, ALPHA_FULL);
        assert_eq!(tally.alpha_partial, ALPHA_PARTIAL);

        let catalog_path = evidence_dir.join("livery-catalog.json");
        fs::write(
            &catalog_path,
            catalog_json(&candidate_tree, &catalog, tally),
        )
        .unwrap_or_else(|error| panic!("write {}: {error}", catalog_path.display()));
        let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
        artifacts.push(artifact(&catalog_path, "json", &evidence_dir));

        let engine = format!(
            "{{\"rust\": {}, \"bevy\": {}, \"avian\": {}}}",
            jstr(&rustc_version()),
            jstr(&locked_version("bevy")),
            jstr(&locked_version("avian3d")),
        );

        // `unknowns` is empty because the report has no unresolved issue with
        // its `implemented` claim: the stock-livery inventory, the faction and
        // prefix sets and every prefix x faction combination are measured from
        // the original library itself, and all 184 members parse inside the
        // observed subset with zero findings. The scope boundaries this stage
        // cannot verify (visual comparison, palette, prefix->airframe) are
        // named in `review.method` ([`DEFERRED_BOUNDARIES`]) with their
        // resolving Rally tasks; they are never hidden to satisfy the gate.
        let report = format!(
            "{{\n\
             \x20\"schema_version\": 1,\n\
             \x20\"task_id\": \"F09-D\",\n\
             \x20\"candidate_tree\": {},\n\
             \x20\"engine\": {},\n\
             \x20\"created_at\": {},\n\
             \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
             \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
             \x20\"seed\": 0,\n\
             \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
             \x20\"overrides\": [],\n\
             \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
             \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
             \x20\"assertions\": [{}],\n\
             \x20\"artifacts\": [{}],\n\
             \x20\"unknowns\": [],\n\
             \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
             \x20\"claim\": \"implemented\"\n\
             }}\n",
            jstr(&candidate_tree),
            engine,
            jstr(&iso_utc_now()),
            str_array(&argv),
            jstr(&git(&["rev-parse", "--show-toplevel"])),
            exit_code,
            jstr(&install_sha256),
            jstr(&content_sha256),
            suite.discovered,
            suite.executed,
            suite.passed,
            suite.failed,
            suite.ignored,
            assertion_array(&suite.assertions),
            artifact_array(&artifacts),
            jstr(
                "implemented by deepseek-1 (Rally #40); reviewed and regenerated on the rebased \
                 commit by deepseek-1 in a fresh session. The same agent identity implemented and \
                 reviewed this task, so this is not independent evidence in the owner directive's \
                 sense, and no agent review replaces the owner's human approval."
            ),
            jstr(&format!(
                "acceptance suite run locally with the retail capability; this harness derives \
                 every field from the recorded log, production discovery and fingerprint of \
                 $CS_GAME_DIR, the production StockLiveryCatalog::discover over \
                 GOSDATA/ASSETS/crimson.rof (184 members, 14 factions, 11 prefixes, 33 \
                 prefix x faction combinations, zero findings; per-member names, dimensions and \
                 plane digests in livery-catalog.json), rustc and Cargo.lock. The reference \
                 comparison in the suite is against a pinned private Pillow reference that runs \
                 the S09/S10 tool algorithm, not the original renderer, so the claim is \
                 `implemented` only, and the `unknowns` array is empty because this report has no \
                 unresolved issue with that claim. Deferred follow-up boundaries (tracked by \
                 their own Rally tasks so they survive this task; also recorded in \
                 docs/findings/2026-09-29-f09-d-stock-liveries-and-combinations.md): {}. \
                 Validated with tools/validate_evidence.py --require-pass.",
                DEFERRED_BOUNDARIES.join(" | "),
            )),
        );

        let out = evidence_dir.join("acceptance.json");
        fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

        // A cheap self-check without a JSON dependency: the validator runs next,
        // but a structurally empty write must fail here first.
        let written = fs::read_to_string(&out).expect("the report reads back");
        for needle in [
            "\"schema_version\": 1",
            "\"task_id\": \"F09-D\"",
            "\"claim\": \"implemented\"",
            "\"install_sha256\"",
            "\"content_sha256\"",
            "\"assertions\": [",
            "\"artifacts\": [",
            "\"unknowns\": []",
            "retail_composition_agreement",
            "retail_faction_palette",
            "prefix_airframe_mapping",
            "on_screen_several_angles",
        ] {
            assert!(
                written.contains(needle),
                "the written report is missing {needle:?}:\n{written}"
            );
        }
        assert!(
            suite.failed == 0 && exit_code == 0,
            "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
             honestly and must NOT validate; fix the tests first",
            suite.failed
        );
        println!("wrote {}", out.display());
    }

    /// Evidence-report harness for task F09-PALETTE
    /// (`docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`).
    ///
    /// Like the F09-D harness above, it is deliberately **not** named
    /// `accept_f09_palette_*`: it is not part of the acceptance suite, it fails
    /// loudly when its inputs are missing instead of passing vacuously, and the
    /// `accept_f09_palette_` selection must never pick it up.
    ///
    /// Run from the workspace root, after the acceptance suite, exactly as:
    ///
    /// 1. ```sh
    ///    cargo test --workspace --locked -- accept_f09_palette_ --include-ignored \
    ///      > private/evidence/F09-PALETTE/cargo-test.log 2>&1
    ///    ```
    ///    (record that command's exit status — it is passed to this harness as
    ///    `CS_EVIDENCE_EXIT_CODE`.)
    /// 2. ```sh
    ///    CS_EVIDENCE_DIR=private/evidence/F09-PALETTE \
    ///    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
    ///    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f09_palette_ --include-ignored" \
    ///    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
    ///      cargo test --locked -p cs_content --lib evidence_report_f09_palette -- --ignored
    ///    ```
    /// 3. ```sh
    ///    python3 tools/validate_evidence.py \
    ///      private/evidence/F09-PALETTE/acceptance.json \
    ///      --artifact-root private/evidence/F09-PALETTE
    ///    ```
    ///    (the `--require-pass` flag rejects a report with a nonempty `unknowns`
    ///    array, and this task's deliverable is that the gaps stay recorded
    ///    machine-readably; see `PALETTE_UNKNOWNS` and the committed finding for
    ///    why deleting them to turn the flag green is the forbidden thing.)
    /// 4. Commit a copy of `acceptance.json` as
    ///    `docs/findings/evidence/F09-PALETTE.json`.
    ///
    /// Every field is derived from real inputs: the recorded test log, the
    /// environment, `rustc --version` and `Cargo.lock`, the production
    /// installation discovery and fingerprint of `$CS_GAME_DIR`, and the
    /// production [`FactionPaletteCatalog::discover`] over `ZBD/zrdr.zbd`
    /// member `vehicle.zrd`.
    #[test]
    #[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
    fn evidence_report_f09_palette_writes_the_acceptance_report() {
        let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
        let candidate_tree = env_var("CS_CANDIDATE_TREE");
        let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        assert!(
            !argv.is_empty(),
            "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
        );
        let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
            .parse()
            .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
        let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

        // The candidate tree must be the tree that was actually tested: a stale
        // report from another commit is exactly what this check refuses.
        let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
        assert_eq!(
            candidate_tree, head_tree,
            "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
             old reports cannot be reused for new code"
        );

        // The acceptance suite is the evidence: parse its recorded output.
        let log_path = evidence_dir.join("cargo-test.log");
        let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
            panic!(
                "cannot read the acceptance log {}: {error} (step 1 must write its output there)",
                log_path.display()
            )
        });
        let suite = parse_suite_for(&log, "accept_f09_palette_");
        assert!(
            suite.passed > 0 && !suite.assertions.is_empty(),
            "no `accept_f09_palette_` tests were recorded in {}",
            log_path.display()
        );

        // Capability coverage is checked, never assumed: `retail` is declared
        // only because the retail acceptance test is in this log and passed.
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == PALETTE_RETAIL_TEST)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{PALETTE_RETAIL_TEST} did not run: F09-PALETTE requires capability `retail`, \
                     run step 1 with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{PALETTE_RETAIL_TEST} must pass; got status {status}"
        );
        assert!(
            suite
                .assertions
                .iter()
                .any(|(name, _)| name.starts_with("accept_f09_palette_")
                    && !name.contains("_retail_")),
            "synthetic task tests must be present alongside the retail one"
        );

        // The installation and content hashes come from the **production**
        // discovery and fingerprint code (F02), not from a hash this harness
        // computes, so they are comparable with every earlier task's record.
        let found = install::discover(&game_dir).expect(
            "production discovery must read the original installation for the evidence record",
        );
        let install_sha256 = install::fingerprint(&found.manifest).to_hex();
        let content_sha256 = install::content_fingerprint(&found.manifest).to_hex();

        // The substantive measurement: the production palette extraction over
        // the original installation, asserted against the pinned corpus.
        let context = ResolveContext::new(install::fingerprint(&found.manifest));
        let mut builder = SessionBuilder::new(context);
        builder
            .mount_installation(&game_dir, &found.diagnosis)
            .expect("the installation mounts");
        let session = builder.open();
        let catalog = FactionPaletteCatalog::discover(&session, &palette_evidence_key())
            .expect("the original faction palette extracts");

        assert_eq!(
            catalog.install_sha256(),
            install::fingerprint(&found.manifest)
        );
        assert_eq!(catalog.container_path(), PALETTE_CONTAINER);
        assert_eq!(catalog.member(), PALETTE_MEMBER);
        assert_eq!(catalog.factions().len(), PALETTE_EXPECTED_FACTIONS);
        assert_eq!(catalog.records().len(), PALETTE_EXPECTED_RECORDS);
        assert_eq!(
            catalog.member_sha256().to_hex(),
            "d22cabb0038c6bde6481a38992729657a4de60b3d6dbf668c71d29545d671baf"
        );
        assert_eq!(catalog.member_span().offset(), 1_397_861);
        assert_eq!(catalog.member_span().length(), 97_917);
        assert!(
            catalog.palette("player_fortune").is_err(),
            "the player pattern must stay a recorded gap, never a fabricated palette"
        );
        assert_eq!(
            catalog.findings().len(),
            2,
            "the two player gaps are the only findings: {:?}",
            catalog
                .findings()
                .iter()
                .map(|finding| (finding.code(), finding.record()))
                .collect::<Vec<_>>(),
        );
        assert!(
            catalog
                .findings()
                .iter()
                .all(|finding| finding.code() == "pattern_without_colors")
        );
        // Every exposed combination is a real three-color, three-decal triple
        // with a container-absolute span per color.
        for faction in catalog.factions() {
            assert_eq!(faction.colors().len(), 3, "{}", faction.faction());
            assert_eq!(faction.decals().len(), 3, "{}", faction.faction());
            for color in faction.colors() {
                assert_eq!(color.span().install_sha256(), catalog.install_sha256());
                assert_eq!(color.span().container_path(), PALETTE_CONTAINER);
                assert_eq!(color.span().member_key(), Some(PALETTE_MEMBER));
                assert!(color.span().length() > 0);
            }
        }

        let catalog_path = evidence_dir.join("palette-catalog.json");
        fs::write(
            &catalog_path,
            palette_catalog_json(&candidate_tree, &catalog),
        )
        .unwrap_or_else(|error| panic!("write {}: {error}", catalog_path.display()));
        let artifacts = vec![
            artifact(&log_path, "log", &evidence_dir),
            artifact(&catalog_path, "json", &evidence_dir),
        ];

        let engine = format!(
            "{{\"rust\": {}, \"bevy\": {}, \"avian\": {}}}",
            jstr(&rustc_version()),
            jstr(&locked_version("bevy")),
            jstr(&locked_version("avian3d")),
        );

        let unknowns: Vec<String> = PALETTE_UNKNOWNS
            .iter()
            .map(|item| (*item).to_owned())
            .collect();
        let report = format!(
            "{{\n\
             \x20\"schema_version\": 1,\n\
             \x20\"task_id\": \"F09-PALETTE\",\n\
             \x20\"candidate_tree\": {},\n\
             \x20\"engine\": {},\n\
             \x20\"created_at\": {},\n\
             \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
             \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
             \x20\"seed\": 0,\n\
             \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
             \x20\"overrides\": [],\n\
             \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
             \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
             \x20\"assertions\": [{}],\n\
             \x20\"artifacts\": [{}],\n\
             \x20\"unknowns\": {},\n\
             \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
             \x20\"claim\": \"implemented\"\n\
             }}\n",
            jstr(&candidate_tree),
            engine,
            jstr(&iso_utc_now()),
            str_array(&argv),
            jstr(&git(&["rev-parse", "--show-toplevel"])),
            exit_code,
            jstr(&install_sha256),
            jstr(&content_sha256),
            suite.discovered,
            suite.executed,
            suite.passed,
            suite.failed,
            suite.ignored,
            assertion_array(&suite.assertions),
            artifact_array(&artifacts),
            str_array(&unknowns),
            jstr(
                "implemented by deepseek-1 (Rally #385); reviewed and regenerated on the rebased \
                 commit by deepseek-1 in a fresh session. The same agent identity implemented and \
                 reviewed this task, so this is not independent evidence in the owner directive's \
                 sense, and no agent review replaces the owner's human approval."
            ),
            jstr(
                "acceptance suite run locally with the retail capability; this harness derives \
                 every field from the recorded log, production discovery and fingerprint of \
                 $CS_GAME_DIR, the production FactionPaletteCatalog::discover over ZBD/zrdr.zbd \
                 member vehicle.zrd (28 paint-bearing records, 11 complete faction palettes, two \
                 recorded pattern_without_colors findings; per-record names, color/decal values and \
                 container-absolute spans in palette-catalog.json), rustc and Cargo.lock. The \
                 palette values are read from the original member with a container-absolute \
                 SourceSpan each, so the claim is `implemented` only. The `unknowns` array is \
                 populated because this task's deliverable includes the gaps that stay unresolved \
                 (player_fortune, BROADWAY/ITSTAXI, decal index semantics, shade multipliers, \
                 slot->mask mapping), each naming its affected content and resolving task; the \
                 report is therefore validated with tools/validate_evidence.py WITHOUT \
                 --require-pass, whose failure is expected and is recorded in the committed \
                 finding. See docs/findings/2026-10-02-f09-palette-original-faction-palettes.md."
            ),
        );

        let out = evidence_dir.join("acceptance.json");
        fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

        // A cheap self-check without a JSON dependency: the validator runs next,
        // but a structurally empty/wrong write must fail here first.
        let written = fs::read_to_string(&out).expect("the report reads back");
        for needle in [
            "\"schema_version\": 1",
            "\"task_id\": \"F09-PALETTE\"",
            "\"claim\": \"implemented\"",
            "\"install_sha256\"",
            "\"content_sha256\"",
            "\"assertions\": [",
            "\"artifacts\": [",
            "\"unknowns\": [",
            "player_palette",
            "missing_faction_palettes",
            "decal_index_semantics",
            "shade_multipliers",
            "slot_to_mask_mapping",
        ] {
            assert!(
                written.contains(needle),
                "the written report is missing {needle:?}:\n{written}"
            );
        }
        assert!(
            suite.failed == 0 && exit_code == 0,
            "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
             honestly and must NOT validate; fix the tests first",
            suite.failed
        );
        println!("wrote {}", out.display());
    }

    /// The install-root key the palette extraction addresses.
    fn palette_evidence_key() -> AssetKey {
        AssetKey::from_spelling(INSTALL_NAMESPACE, PALETTE_CONTAINER, "default")
            .expect("a valid palette key")
    }

    // ------------------------------------------------------------- inputs ---

    /// Mounts the shared airframe library through the production ROF mount of an
    /// already-discovered installation.
    fn retail_source(root: &Path, found: &install::Discovery) -> RofSource {
        let context = ResolveContext::new(install::fingerprint(&found.manifest));
        let mut builder = SessionBuilder::new(context);
        let mount = MountBuilder::new(
            MountId::new("rof-gosdata-assets-crimson-rof").expect("a valid mount id"),
            MountNamespace::new(INSTALL_NAMESPACE).expect("a valid namespace"),
            PrecedenceClass::Shared,
            CONTAINER,
        )
        .retail();
        mount_rof_into(&mut builder, mount, &root.join(CONTAINER))
            .expect("the airframe library mounts")
    }

    fn env_var(name: &str) -> String {
        std::env::var(name).unwrap_or_else(|_| {
            panic!(
                "{name} is not set: this harness only runs through the sequence in its module doc \
                 (crates/cs_content/src/livery.rs)"
            )
        })
    }

    /// Cargo runs a test binary with its working directory set to the *package*
    /// root, so a path written relative to the workspace root must be re-anchored.
    fn workspace_path(as_described: &str) -> PathBuf {
        let path = PathBuf::from(as_described);
        if path.is_absolute() {
            return path;
        }
        Path::new(&git(&["rev-parse", "--show-toplevel"])).join(path)
    }

    fn git(args: &[&str]) -> String {
        let output = Command::new("git").args(args).output().expect("git runs");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn rustc_version() -> String {
        let output = Command::new("rustc")
            .arg("--version")
            .output()
            .expect("rustc runs");
        assert!(output.status.success(), "rustc --version failed");
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// The locked version of one `Cargo.lock` package: read, never asserted from
    /// memory.
    fn locked_version(package: &str) -> String {
        let lock_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crates/")
            .parent()
            .expect("workspace root")
            .join("Cargo.lock");
        let lock = fs::read_to_string(&lock_path)
            .unwrap_or_else(|error| panic!("read {}: {error}", lock_path.display()));
        let mut wanted = false;
        for line in lock.lines() {
            let line = line.trim();
            if line == "[[package]]" {
                wanted = false;
            } else if let Some(name) = line.strip_prefix("name = \"") {
                wanted = name.trim_end_matches('"') == package;
            } else if let Some(version) = line.strip_prefix("version = \"")
                && wanted
            {
                return version.trim_end_matches('"').to_owned();
            }
        }
        panic!("package {package:?} is not in {}", lock_path.display());
    }

    // ---------------------------------------------------------- log parsing ---

    /// What the recorded `cargo test` output says actually happened.
    #[derive(Debug, Default)]
    struct Suite {
        discovered: u64,
        executed: u64,
        passed: u64,
        failed: u64,
        ignored: u64,
        /// `(test name, "pass" | "fail")`, in log order, deduplicated.
        assertions: Vec<(String, &'static str)>,
    }

    /// Extracts the libtest summaries and the per-test results of the
    /// `accept_f09_d_` tests from a recorded `cargo test` output.
    fn parse_suite(log: &str) -> Suite {
        parse_suite_for(log, "accept_f09_d_")
    }

    /// Extracts the libtest summaries and the per-test results of the tests
    /// whose leaf name carries `prefix` from a recorded `cargo test` output.
    fn parse_suite_for(log: &str, prefix: &str) -> Suite {
        let mut suite = Suite::default();
        let mut pending: Vec<String> = Vec::new();
        for line in log.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("test result:") {
                for (count, kind) in summary_fields(trimmed) {
                    match kind {
                        "passed" => suite.passed += count,
                        "failed" => suite.failed += count,
                        "ignored" => suite.ignored += count,
                        _ => {}
                    }
                }
                continue;
            }
            // A status on its own line completes the earliest test that was
            // started on an earlier line without an inline status.
            if !pending.is_empty() {
                if trimmed == "ok" {
                    let name = pending.remove(0);
                    record(&mut suite, name, "pass");
                    continue;
                }
                if trimmed == "FAILED" {
                    let name = pending.remove(0);
                    record(&mut suite, name, "fail");
                    continue;
                }
            }
            // `test <name> ... <status>`, possibly several per interleaved line.
            // Test names carry their module path (`livery::tests::accept_f09_d_…`);
            // the report records the leaf, which is what the prefix selects.
            let mut cursor = trimmed;
            while let Some(position) = cursor.find("test ") {
                let after = &cursor[position + 5..];
                let Some(separator) = after.find(" ... ") else {
                    break;
                };
                let full = &after[..separator];
                if !full.contains(prefix) {
                    cursor = &after[separator + 5..];
                    continue;
                }
                let name = full.rsplit("::").next().expect("a name").to_owned();
                let tail = &after[separator + 5..];
                cursor = tail;
                match tail.split_whitespace().next() {
                    Some("ok") => record(&mut suite, name, "pass"),
                    Some("FAILED") => record(&mut suite, name, "fail"),
                    _ => pending.push(name),
                }
            }
        }
        suite.assertions.dedup_by(|left, right| left.0 == right.0);
        suite.executed = suite.passed + suite.failed;
        suite.discovered = suite.passed + suite.failed + suite.ignored;
        suite
    }

    /// `(count, kind)` pairs of one `test result:` summary line.
    fn summary_fields(line: &str) -> Vec<(u64, &str)> {
        let mut fields = Vec::new();
        for segment in line["test result:".len()..].split(';') {
            let words: Vec<&str> = segment.split_whitespace().collect();
            for pair in words.windows(2) {
                if let Ok(count) = pair[0].parse::<u64>()
                    && matches!(pair[1], "passed" | "failed" | "ignored")
                {
                    fields.push((count, pair[1]));
                    break;
                }
            }
        }
        fields
    }

    fn record(suite: &mut Suite, name: String, status: &'static str) {
        if suite.assertions.iter().any(|(seen, _)| *seen == name) {
            return;
        }
        suite.assertions.push((name, status));
    }

    // ------------------------------------------------------------- catalog ---

    /// The measured catalog as a JSON artifact: member spellings, dimensions,
    /// per-member plane digests, every prefix x faction count and the corpus
    /// endpoint coverage. No pixels, no file bytes; the artifact itself stays
    /// in `private/`.
    fn catalog_json(
        candidate_tree: &str,
        catalog: &StockLiveryCatalog,
        tally: EndpointTally,
    ) -> String {
        let mut by_prefix: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
        for asset in catalog.assets() {
            *by_prefix
                .entry(asset.prefix().to_owned())
                .or_default()
                .entry(asset.faction().to_owned())
                .or_default() += 1;
        }
        let combinations: Vec<String> = by_prefix
            .iter()
            .map(|(prefix, factions)| {
                let items: Vec<String> = factions
                    .iter()
                    .map(|(faction, count)| {
                        format!("{{\"faction\": {}, \"assets\": {count}}}", jstr(faction))
                    })
                    .collect();
                let assets: usize = factions.values().sum();
                format!(
                    "{{\"prefix\": {}, \"assets\": {assets}, \"factions\": [{}]}}",
                    jstr(prefix),
                    items.join(", "),
                )
            })
            .collect();

        let assets: Vec<String> = catalog
            .assets()
            .iter()
            .map(|asset| {
                format!(
                    "{{\"spelling\": {}, \"faction\": {}, \"prefix\": {}, \"part\": {}, \
                     \"width\": {}, \"height\": {}, \"covered_len\": {}, \"tail_bytes\": {}, \
                     \"fingerprint\": {}}}",
                    jstr(asset.spelling()),
                    jstr(asset.faction()),
                    jstr(asset.prefix()),
                    jstr(asset.part()),
                    asset.width(),
                    asset.height(),
                    asset.covered_len(),
                    asset.tail_bytes(),
                    jstr(&asset.fingerprint().to_hex()),
                )
            })
            .collect();

        // One digest over every member's spelling and plane fingerprint, so the
        // committed finding can carry a single comparable value.
        let mut material = Vec::new();
        for asset in catalog.assets() {
            material.extend_from_slice(asset.spelling().as_bytes());
            material.push(0);
            material.extend_from_slice(asset.fingerprint().as_bytes());
        }
        let catalog_fingerprint = install::sha256(&material).to_hex();

        let factions: Vec<String> = catalog.factions().iter().map(|f| jstr(f)).collect();
        let prefixes: Vec<String> = catalog.prefixes().iter().map(|p| jstr(p)).collect();

        format!(
            "{{\n\
             \x20\"task_id\": \"F09-D\",\n\
             \x20\"candidate_tree\": {},\n\
             \x20\"created_at\": {},\n\
             \x20\"reader\": \"cs_content::livery::StockLiveryCatalog::discover\",\n\
             \x20\"layout_source\": \"S09 extract_bm.py blob \
             ec196de05f532cc3c286ccf8fead363bf76e5c63; S10 set_paintjob.py blob \
             ccf7c4ea065c17a44d354e8d703561cdc94dd518\",\n\
             \x20\"claim\": \"implemented\",\n\
             \x20\"evidence_class\": \"observed_tool\",\n\
             \x20\"note\": \"member spellings, dimensions, per-member plane digests and every \
             prefix x faction count; no pixels, no file bytes\",\n\
             \x20\"container\": {},\n\
             \x20\"catalog_fingerprint\": {},\n\
             \x20\"totals\": {{\"assets\": {}, \"factions\": {}, \"prefixes\": {}, \
             \"combinations\": {}, \"findings\": {}}},\n\
             \x20\"endpoint_coverage\": {{\"mask\": {{\"zero\": {}, \"full\": {}, \
             \"partial\": {}}}, \"overlay_alpha\": {{\"zero\": {}, \"full\": {}, \
             \"partial\": {}}}}},\n\
             \x20\"factions\": [{}],\n\
             \x20\"prefixes\": [{}],\n\
             \x20\"combinations\": [\n  {}\n ],\n\
             \x20\"assets\": [\n  {}\n ]\n\
             }}\n",
            jstr(candidate_tree),
            jstr(&iso_utc_now()),
            jstr(CONTAINER),
            jstr(&catalog_fingerprint),
            catalog.assets().len(),
            catalog.factions().len(),
            catalog.prefixes().len(),
            by_prefix.values().map(BTreeMap::len).sum::<usize>(),
            catalog.findings().len(),
            tally.mask_zero,
            tally.mask_full,
            tally.mask_partial,
            tally.alpha_zero,
            tally.alpha_full,
            tally.alpha_partial,
            factions.join(", "),
            prefixes.join(", "),
            combinations.join(",\n  "),
            assets.join(",\n  "),
        )
    }

    /// The extracted palette as a JSON artifact: the member provenance, every
    /// faction's colors, decals and container-absolute spans, every
    /// paint-bearing record and its recorded findings. Values and spans only;
    /// no pixels, no file bytes. The artifact itself stays in `private/`.
    fn palette_catalog_json(candidate_tree: &str, catalog: &FactionPaletteCatalog) -> String {
        let factions: Vec<String> = catalog
            .factions()
            .iter()
            .map(|palette| {
                let colors: Vec<String> = palette
                    .colors()
                    .iter()
                    .map(|color| {
                        format!(
                            "{{\"rgb\": [{}, {}, {}], \"offset\": {}, \"length\": {}}}",
                            color.red(),
                            color.green(),
                            color.blue(),
                            color.span().offset(),
                            color.span().length(),
                        )
                    })
                    .collect();
                let decals: Vec<String> = palette
                    .decals()
                    .iter()
                    .map(|decal| {
                        format!(
                            "{{\"index\": {}, \"offset\": {}, \"length\": {}}}",
                            decal.index(),
                            decal.span().offset(),
                            decal.span().length(),
                        )
                    })
                    .collect();
                let records: Vec<String> =
                    palette.records().iter().map(|name| jstr(name)).collect();
                format!(
                    "{{\"faction\": {}, \"colors\": [{}], \"decals\": [{}], \"records\": [{}]}}",
                    jstr(palette.faction()),
                    colors.join(", "),
                    decals.join(", "),
                    records.join(", "),
                )
            })
            .collect();

        let records: Vec<String> = catalog
            .records()
            .iter()
            .map(|record| {
                format!(
                    "{{\"name\": {}, \"pattern\": {}, \"has_colors\": {}, \
                     \"pattern_offset\": {}, \"pattern_length\": {}, \"offset\": {}, \
                     \"length\": {}}}",
                    jstr(record.name()),
                    jstr(record.pattern()),
                    record.has_colors(),
                    record.pattern_span().offset(),
                    record.pattern_span().length(),
                    record.span().offset(),
                    record.span().length(),
                )
            })
            .collect();

        let findings: Vec<String> = catalog
            .findings()
            .iter()
            .map(|finding| {
                format!(
                    "{{\"code\": {}, \"record\": {}, \"detail\": {}}}",
                    jstr(finding.code()),
                    jstr(finding.record()),
                    jstr(finding.detail()),
                )
            })
            .collect();

        // One digest over every faction's name, colors and decals, so the
        // committed finding can carry a single comparable value.
        let mut material = Vec::new();
        for palette in catalog.factions() {
            material.extend_from_slice(palette.faction().as_bytes());
            material.push(0);
            for color in palette.colors() {
                material.extend_from_slice(&color.rgb());
            }
            for decal in palette.decals() {
                material.extend_from_slice(&decal.index().to_le_bytes());
            }
            material.push(0xff);
        }
        let palette_fingerprint = install::sha256(&material).to_hex();

        format!(
            "{{\n\
             \x20\"task_id\": \"F09-PALETTE\",\n\
             \x20\"candidate_tree\": {},\n\
             \x20\"created_at\": {},\n\
             \x20\"reader\": \"cs_content::livery::FactionPaletteCatalog::discover\",\n\
             \x20\"grammar\": \"zrd: u32 tag 1=int, 2=float, 3=u32 len + bytes, 4=u32 count + (count-1) children; every zrd member of ZBD/zrdr.zbd consumes exactly\",\n\
             \x20\"claim\": \"implemented\",\n\
             \x20\"evidence_class\": \"observed_original_data\",\n\
             \x20\"note\": \"faction names, color/decal values and container-absolute byte spans; no pixels, no file bytes\",\n\
             \x20\"container\": {},\n\
             \x20\"member\": {},\n\
             \x20\"member_sha256\": {},\n\
             \x20\"member_offset\": {},\n\
             \x20\"member_length\": {},\n\
             \x20\"palette_fingerprint\": {},\n\
             \x20\"totals\": {{\"factions\": {}, \"records\": {}, \"findings\": {}}},\n\
             \x20\"factions\": [\n  {}\n ],\n\
             \x20\"records\": [\n  {}\n ],\n\
             \x20\"findings\": [\n  {}\n ]\n\
             }}\n",
            jstr(candidate_tree),
            jstr(&iso_utc_now()),
            jstr(catalog.container_path()),
            jstr(catalog.member()),
            jstr(&catalog.member_sha256().to_hex()),
            catalog.member_span().offset(),
            catalog.member_span().length(),
            jstr(&palette_fingerprint),
            catalog.factions().len(),
            catalog.records().len(),
            catalog.findings().len(),
            factions.join(",\n  "),
            records.join(",\n  "),
            findings.join(",\n  "),
        )
    }

    // ------------------------------------------------------------ artifacts ---

    /// One referenced artifact, hashed with the **production** SHA-256. The
    /// validator re-hashes it with `hashlib` independently, so a wrong digest
    /// here fails validation rather than passing quietly.
    fn artifact(source: &Path, kind: &str, evidence_dir: &Path) -> (String, String, String) {
        let name = source
            .file_name()
            .expect("artifact has a file name")
            .to_string_lossy()
            .into_owned();
        let target = evidence_dir.join(&name);
        if source != target {
            fs::copy(source, &target).unwrap_or_else(|error| {
                panic!("copy {} -> {}: {error}", source.display(), target.display())
            });
        }
        let bytes =
            fs::read(&target).unwrap_or_else(|error| panic!("read {}: {error}", target.display()));
        (name, install::sha256(&bytes).to_hex(), kind.to_owned())
    }

    // ------------------------------------------------------------- rendering ---

    fn assertion_array(assertions: &[(String, &'static str)]) -> String {
        let items: Vec<String> = assertions
            .iter()
            .map(|(name, status)| {
                format!(
                    "{{\"id\": {}, \"status\": {status:?}, \"evidence\": [\"cargo-test.log\"]}}",
                    jstr(name)
                )
            })
            .collect();
        items.join(", ")
    }

    fn artifact_array(artifacts: &[(String, String, String)]) -> String {
        let items: Vec<String> = artifacts
            .iter()
            .map(|(name, digest, kind)| {
                format!(
                    "{{\"path\": {}, \"sha256\": {digest:?}, \"kind\": {kind:?}}}",
                    jstr(name)
                )
            })
            .collect();
        items.join(", ")
    }

    fn str_array(items: &[String]) -> String {
        let quoted: Vec<String> = items.iter().map(|item| jstr(item)).collect();
        format!("[{}]", quoted.join(", "))
    }

    /// A JSON string literal: quoted and escaped, so no report field can break
    /// out of its string.
    fn jstr(value: &str) -> String {
        let mut out = String::with_capacity(value.len() + 2);
        out.push('"');
        for character in value.chars() {
            match character {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                control if (control as u32) < 0x20 => {
                    out.push_str(&format!("\\u{:04x}", control as u32));
                }
                other => out.push(other),
            }
        }
        out.push('"');
        out
    }

    /// RFC 3339 with whole seconds and `Z`, which `datetime.fromisoformat`
    /// accepts after the validator's `Z` -> `+00:00` replacement.
    fn iso_utc_now() -> String {
        let epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the system clock is after 1970")
            .as_secs() as i64;
        let (year, month, day, hour, minute, second) = civil_from_unix(epoch);
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
    }

    /// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to a UTC
    /// calendar date, because `std` has no date formatting.
    fn civil_from_unix(seconds: i64) -> (i64, u32, u32, u32, u32, u32) {
        let days = seconds.div_euclid(86_400);
        let rest = seconds.rem_euclid(86_400);
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let day_of_era = z - era * 146_097;
        let year_of_era =
            (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let year_of_day = year_of_era + era * 400;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let month_prime = (5 * day_of_year + 2) / 153;
        let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
        let month = (if month_prime < 10 {
            month_prime + 3
        } else {
            month_prime - 9
        }) as u32;
        let year = if month <= 2 {
            year_of_day + 1
        } else {
            year_of_day
        };
        (
            year,
            month,
            day,
            (rest / 3_600) as u32,
            ((rest % 3_600) / 60) as u32,
            (rest % 60) as u32,
        )
    }
}
