//! The declared localization contract: locale identity and fallback, the
//! control-markup grammar, and font provenance (F51-A).
//!
//! Spec: `specs/F51-localization-fonts-text-layout-and-original-media-ids.md`,
//! stage `### F51-A`. Shared contract: `docs/contracts/UI-NETWORK.md`.
//!
//! This module is the **content half** of F51. It answers three questions
//! with typed records, and answers nothing else:
//!
//! * **Which locale answers a string?** [`LocaleId`] is a bounded, validated
//!   label and [`LocaleChain`] is the *explicit* ordered fallback a caller
//!   supplies. There is no default chain and no language enum: the original
//!   supported locale list is **unmeasured** (see the stage finding
//!   `docs/findings/2026-10-01-f51-a-locale-fallback-markup-and-font-provenance.md`),
//!   so a guessed list would be a fabricated compatibility claim.
//! * **What does a string's markup mean?** [`parse_markup`] interprets a
//!   resource string only against a [`MarkupGrammar`] the caller declares.
//!   Anything the grammar does not admit stays literal text and produces a
//!   [`MarkupIssue`] — an arbitrary resource string is never executable markup
//!   (F51 non-negotiable behavior 2).
//! * **Where did a font come from?** [`FontFace`] can only be built from a
//!   [`FontProvenance`]: an original font inside the owner's installation
//!   (never redistributed) or a separately licensed fallback with verified
//!   permission. Naming an operating-system font or a proprietary game font is
//!   possible only as a [`FontSource`] that [`FontFace::try_new`] **refuses**
//!   (F51 non-negotiable behavior 1).
//!
//! # Known or explicitly unknown, never defaulted
//!
//! Every row keeps an [`Origin`] and a [`Provenance`], and
//! [`TextResolution::Missing`] names every locale that was tried instead of
//! substituting a placeholder string. A font's glyph coverage is a declared
//! [`GlyphCoverage`] set: characters outside it are *counted* by
//! [`GlyphCoverage::missing_in`] rather than silently dropped (F51
//! non-negotiable behavior 3).
//!
//! # Decoding the F12 resource rows (F51-B)
//!
//! [`ResourceDecode`] bridges the F12 PE string catalog
//! ([`crate::config::StringRow`], `(id, language, text)`) into a
//! [`TextCatalog`]. The mapping from a resource language id to a [`LocaleId`]
//! is the caller's [`LanguageMap`] — never a built-in language table — because
//! the original supported-locale list is unmeasured; a row whose language is
//! not declared is reported, not guessed. A row whose code units did not decode
//! (`StringRow::text == None`) and a duplicate `(id, locale)` pair are likewise
//! reported instead of being replaced or silently chosen between.
//!
//! # Declaring the locale set from measurement (F51-LOCALE-SET)
//!
//! [`MeasuredLocales`] replaces the caller's guess with what the original files
//! actually carry: [`ResourceLanguageTable`] is the set of third-level PE
//! resource language ids read out of the installation's string images, each
//! with the [`SourceSpan`] that proves it, and the declaration is *derived* from
//! that table ([`MeasuredLocales::from_table`]) instead of being typed in. The
//! derived [`LocaleId`] label spells the measured id (`resource-1033`), never a
//! language name, because the release's own naming of its locales is unmeasured
//! and Win32's naming convention is not evidence about the 2000 release.
//!
//! [`IdNumbering`] answers F12 AC04 — *a localized installation preserves
//! stable ids while changing display text* — by comparing the ids two locales
//! answer, and it is what a second, localized installation will be measured
//! with. One installation carries one language id, so that comparison is
//! measurable only once the owner supplies a second installation; the type
//! exists so the answer is a measurement rather than an assumption.
//!
//! # What this stage does not do
//!
//! No font is parsed and no glyph is rasterized. The real control-markup
//! delimiters and the original font format are still unmeasured, so the grammar
//! stays caller-declared and the font metrics stay a caller-supplied input;
//! those measurements belong to the retail-capable F51-D audit. This module
//! stays Bevy-free and never opens an original file: it turns already-read F12
//! rows into typed localization rows.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentId, ContentIdError, ContentKind, Origin, Provenance};
use cs_types::profile::{ProfileDocument, SettingApply, SettingEntry};

use crate::config::StringRow;
use crate::save::settings::{SettingRule, ValueRule};

// --------------------------------------------------------------- locale ---

/// Maximum byte length of a [`LocaleId`].
///
/// Matches `cs_types::install::MAX_LOCALE_LABEL_LEN`: a locale label is a short
/// bounded token in every layer, so the same label can be carried by an
/// installation profile, a mount scope and a catalog row.
pub const MAX_LOCALE_ID_LEN: usize = 32;

/// Why a [`LocaleId`] was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocaleIdError {
    /// The label was empty or all whitespace.
    Empty,
    /// The label exceeded [`MAX_LOCALE_ID_LEN`] bytes.
    TooLong {
        /// Its length in bytes.
        len: usize,
    },
    /// The label carried a character outside the grammar.
    ///
    /// The grammar is ASCII alphanumerics plus `-` and `_` — the shape the
    /// Windows resource language ids and the installation's own labels
    /// already use — so a label can never smuggle a path separator, a quote or
    /// a control character into a report or a mount scope.
    BadCharacter {
        /// The offending character.
        ch: char,
    },
}

/// One locale identity: an opaque, bounded, validated label.
///
/// The label is **opaque on purpose**. No original supported-locale list was
/// observed at this stage, so this type is not an enum of languages and does
/// not pretend to know which locales the 2000 PC release shipped. A caller
/// resolves a label from its own evidence; this crate only guarantees that the
/// label is a single safe token.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocaleId(String);

impl LocaleId {
    /// Validates and wraps a locale label.
    ///
    /// # Errors
    ///
    /// [`LocaleIdError::Empty`], [`LocaleIdError::TooLong`] or
    /// [`LocaleIdError::BadCharacter`].
    pub fn new(label: &str) -> Result<Self, LocaleIdError> {
        let trimmed = label.trim();
        if trimmed.is_empty() {
            return Err(LocaleIdError::Empty);
        }
        if trimmed.len() > MAX_LOCALE_ID_LEN {
            return Err(LocaleIdError::TooLong { len: trimmed.len() });
        }
        for ch in trimmed.chars() {
            if !ch.is_ascii_alphanumeric() && !matches!(ch, '-' | '_') {
                return Err(LocaleIdError::BadCharacter { ch });
            }
        }
        Ok(Self(trimmed.to_owned()))
    }

    /// The label as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for LocaleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Maximum number of locales one [`LocaleChain`] may hold.
///
/// A designed bound, not an original measurement: a fallback chain longer than
/// this is a content authoring error (an accidental cycle spelled out) and is
/// refused rather than resolved.
pub const MAX_LOCALE_CHAIN_LEN: usize = 8;

/// Why a [`LocaleChain`] was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocaleChainError {
    /// The chain held no locale at all, so nothing could ever answer.
    Empty,
    /// The chain held more than [`MAX_LOCALE_CHAIN_LEN`] locales.
    TooLong {
        /// How many locales were supplied.
        len: usize,
    },
    /// One locale appeared twice: the second occurrence could never be
    /// reached, so the chain was hiding a cycle.
    Duplicate {
        /// The repeated locale.
        locale: LocaleId,
    },
}

impl fmt::Display for LocaleChainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("a locale fallback chain must hold at least one locale"),
            Self::TooLong { len } => write!(
                f,
                "locale chain holds {len} locales, max is {MAX_LOCALE_CHAIN_LEN}"
            ),
            Self::Duplicate { locale } => {
                write!(f, "locale {locale} appears more than once in the chain")
            }
        }
    }
}

impl std::error::Error for LocaleChainError {}

/// The explicit, ordered locale fallback a resolution walks.
///
/// The first entry is the selected locale; each later entry is the next locale
/// to try when the previous one has no row for the requested id. The chain is
/// supplied by the caller and is never defaulted: an empty chain is refused
/// (F51's deliverable names "an explicit fallback chain"), a repeated locale is
/// refused (it would hide a cycle), and an over-long chain is refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocaleChain {
    locales: Vec<LocaleId>,
}

impl LocaleChain {
    /// Validates and assembles a chain from the selected locale first.
    ///
    /// # Errors
    ///
    /// [`LocaleChainError::Empty`], [`LocaleChainError::TooLong`] or
    /// [`LocaleChainError::Duplicate`].
    pub fn new(
        selected: LocaleId,
        fallbacks: impl IntoIterator<Item = LocaleId>,
    ) -> Result<Self, LocaleChainError> {
        let mut locales = vec![selected];
        locales.extend(fallbacks);
        if locales.is_empty() {
            return Err(LocaleChainError::Empty);
        }
        if locales.len() > MAX_LOCALE_CHAIN_LEN {
            return Err(LocaleChainError::TooLong { len: locales.len() });
        }
        let mut seen = BTreeSet::new();
        for locale in &locales {
            if !seen.insert(locale.clone()) {
                return Err(LocaleChainError::Duplicate {
                    locale: locale.clone(),
                });
            }
        }
        Ok(Self { locales })
    }

    /// A chain with a single locale and no fallback.
    ///
    /// # Errors
    ///
    /// [`LocaleChainError::Duplicate`] is unreachable for one locale, so this
    /// only fails if the chain bound is ever lowered below one.
    pub fn exact(locale: LocaleId) -> Result<Self, LocaleChainError> {
        Self::new(locale, [])
    }

    /// The selected locale — the first entry, the one a resolution tries
    /// first.
    #[must_use]
    pub fn selected(&self) -> &LocaleId {
        &self.locales[0]
    }

    /// The whole chain, in try order.
    #[must_use]
    pub fn locales(&self) -> &[LocaleId] {
        &self.locales
    }

    /// How many locales the chain holds.
    #[must_use]
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.locales.len()
    }

    /// Whether the chain holds no locale. Always `false` for a chain built by
    /// [`LocaleChain::new`], which refuses the empty chain; present so a
    /// `Default`-built value cannot be mistaken for a usable chain.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.locales.is_empty()
    }

    /// The depth of `locale` in this chain, or `None` when it is not in it.
    #[must_use]
    pub fn depth_of(&self, locale: &LocaleId) -> Option<usize> {
        self.locales.iter().position(|entry| entry == locale)
    }
}

// --------------------------------------------------------- language ids ---

/// Maximum number of language-id mappings one [`LanguageMap`] may hold.
///
/// A designed bound, not an original measurement: a map larger than this is a
/// content-authoring or reading error (a corrupted language table) and is
/// refused rather than silently truncated.
pub const MAX_LANGUAGE_MAP_LEN: usize = 64;

/// Why a [`LanguageMap`] was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LanguageMapError {
    /// The map declared no language at all, so no resource row could ever be
    /// decoded.
    Empty,
    /// The map held more than [`MAX_LANGUAGE_MAP_LEN`] mappings.
    TooLong {
        /// How many mappings were supplied.
        len: usize,
    },
    /// One resource language id was mapped twice, so a row's locale would
    /// depend on iteration order.
    DuplicateLanguage {
        /// The repeated resource language id.
        language: u32,
    },
}

impl fmt::Display for LanguageMapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => {
                f.write_str("a language map must declare at least one resource language")
            }
            Self::TooLong { len } => write!(
                f,
                "language map holds {len} languages, max is {MAX_LANGUAGE_MAP_LEN}"
            ),
            Self::DuplicateLanguage { language } => {
                write!(f, "resource language {language} is mapped more than once")
            }
        }
    }
}

impl std::error::Error for LanguageMapError {}

/// The caller-declared map from an original resource language id to a
/// [`LocaleId`].
///
/// F12 yields `StringRow.language` as the raw third-level language id the image
/// stored (§ the PE resource tree); turning that number into a locale is a
/// *content* decision this crate must not guess, because the original
/// supported-locale list is unmeasured. The map is therefore supplied by the
/// caller — the same discipline as the caller-declared [`MarkupGrammar`] — and a
/// resource row whose language id is absent is reported by
/// [`ResourceDecode::unmapped_languages`] rather than dropped silently.
///
/// Several language ids may map to one locale (a locale shipped under more than
/// one id), but one id may map to only one locale.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanguageMap {
    locales: BTreeMap<u32, LocaleId>,
}

impl LanguageMap {
    /// Validates and assembles a map from resource language ids to locales.
    ///
    /// # Errors
    ///
    /// [`LanguageMapError::Empty`], [`LanguageMapError::TooLong`] or
    /// [`LanguageMapError::DuplicateLanguage`].
    pub fn new(
        entries: impl IntoIterator<Item = (u32, LocaleId)>,
    ) -> Result<Self, LanguageMapError> {
        let mut locales: BTreeMap<u32, LocaleId> = BTreeMap::new();
        for (language, locale) in entries {
            if locales.contains_key(&language) {
                return Err(LanguageMapError::DuplicateLanguage { language });
            }
            locales.insert(language, locale);
        }
        if locales.is_empty() {
            return Err(LanguageMapError::Empty);
        }
        if locales.len() > MAX_LANGUAGE_MAP_LEN {
            return Err(LanguageMapError::TooLong { len: locales.len() });
        }
        Ok(Self { locales })
    }

    /// The locale a resource language id maps to, if the caller declared it.
    #[must_use]
    pub fn locale(&self, language: u32) -> Option<&LocaleId> {
        self.locales.get(&language)
    }

    /// The declared resource language ids, in ascending order.
    pub fn languages(&self) -> impl Iterator<Item = u32> + '_ {
        self.locales.keys().copied()
    }

    /// How many language ids the map declares.
    #[must_use]
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.locales.len()
    }

    /// Whether the map declares no language. Always `false` for a map built by
    /// [`LanguageMap::new`], which refuses the empty map; present so a
    /// `Default`-built value cannot be mistaken for a usable map.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.locales.is_empty()
    }
}

// ------------------------------------------------------------- string id ---

/// Why a [`TextId`] was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextIdError {
    /// The content id was rejected by the shared id grammar.
    Content(ContentIdError),
    /// The id named a content kind other than `string_resource`.
    ///
    /// A localizable string keeps a `string_resource` identity so it can never
    /// be spelled like a mission, a save or a network value: changing the
    /// locale must not be able to change mission identity or a protocol value
    /// (F51 non-negotiable behavior 5).
    NotAStringResource {
        /// The kind the id actually named.
        kind: ContentKind,
    },
}

impl fmt::Display for TextIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Content(error) => write!(f, "{error}"),
            Self::NotAStringResource { kind } => write!(
                f,
                "localizable text id names a {kind}, not a string_resource"
            ),
        }
    }
}

impl std::error::Error for TextIdError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Content(error) => Some(error),
            Self::NotAStringResource { .. } => None,
        }
    }
}

impl From<ContentIdError> for TextIdError {
    fn from(error: ContentIdError) -> Self {
        Self::Content(error)
    }
}

/// The stable identity of one localizable string.
///
/// The identity is a `string_resource` [`ContentId`] and nothing else, so it is
/// independent of the locale that answers it: the same id under two locales is
/// the same string, which is what makes a translation a different *text* for
/// one *id* rather than a different content element.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TextId(ContentId);

impl TextId {
    /// Builds a text id from a semantic source key.
    ///
    /// # Errors
    ///
    /// [`TextIdError::Content`] for an id the shared grammar rejects, and
    /// [`TextIdError::NotAStringResource`] if the namespace is not
    /// `string_resource`.
    pub fn new(source_key: &str) -> Result<Self, TextIdError> {
        let id = ContentId::from_source(ContentKind::StringResource, source_key)?;
        Self::try_from_content(id)
    }

    /// Re-wraps a [`ContentId`], refusing any other namespace.
    ///
    /// # Errors
    ///
    /// [`TextIdError::NotAStringResource`].
    pub fn try_from_content(id: ContentId) -> Result<Self, TextIdError> {
        if id.kind() != ContentKind::StringResource {
            return Err(TextIdError::NotAStringResource { kind: id.kind() });
        }
        Ok(Self(id))
    }

    /// The id of a PE string-table unit, from the numeric id F12 read.
    ///
    /// `StringRow::id` is `(block - 1) * 16 + index`
    /// (`cs_formats::string_id`) and is the identity a localized installation
    /// keeps while its display text changes (spec F12 AC04), so it is exactly
    /// the semantic key a [`TextId`] needs. The key is spelled in decimal
    /// (`string_resource/<id>`): stable, locale-free and impossible to confuse
    /// with a mission or a network value.
    ///
    /// This cannot fail: a decimal `u32` is always a valid id key.
    #[must_use]
    pub fn from_resource_id(id: u32) -> Self {
        Self::new(&id.to_string()).expect("a decimal resource id is a valid string key")
    }

    /// The underlying content id.
    #[must_use]
    pub fn as_content_id(&self) -> &ContentId {
        &self.0
    }
}

impl fmt::Display for TextId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ------------------------------------------------------------- text rows ---

/// One localized string row: a [`TextId`] bound to one locale's text, with
/// where it came from.
///
/// The row keeps the text **verbatim**. Nothing here normalizes whitespace,
/// folds case or substitutes a missing translation: a row is what the content
/// declared, and a locale that has no row is a
/// [`TextResolution::Missing`], not an empty string.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalizedText {
    id: TextId,
    locale: LocaleId,
    text: String,
    origin: Origin,
    provenance: Provenance,
}

impl LocalizedText {
    /// Assembles a row from already-validated parts.
    #[must_use]
    pub fn new(
        id: TextId,
        locale: LocaleId,
        text: impl Into<String>,
        origin: Origin,
        provenance: Provenance,
    ) -> Self {
        Self {
            id,
            locale,
            text: text.into(),
            origin,
            provenance,
        }
    }

    /// The string's stable identity, independent of the locale.
    #[must_use]
    pub fn id(&self) -> &TextId {
        &self.id
    }

    /// The locale this row's text is written in.
    #[must_use]
    pub fn locale(&self) -> &LocaleId {
        &self.locale
    }

    /// The text, verbatim.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Where the row's bytes came from.
    #[must_use]
    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The provenance of the row.
    #[must_use]
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// Why a [`TextCatalog`] refused a row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextCatalogError {
    /// Two rows share one `(id, locale)` pair.
    ///
    /// Choosing between them would hide a contradiction, exactly as
    /// `cs_content::config::StringCatalog::resolve` refuses an ambiguous
    /// `(id, language)` pair.
    DuplicateEntry {
        /// The string id.
        id: TextId,
        /// The locale.
        locale: LocaleId,
    },
}

impl fmt::Display for TextCatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateEntry { id, locale } => {
                write!(
                    f,
                    "string {id} is inserted more than once for locale {locale}"
                )
            }
        }
    }
}

impl std::error::Error for TextCatalogError {}

/// Every localized string row, keyed by `(id, locale)`.
///
/// Rows are kept in `(id, locale)` order, so iteration and audit output are
/// deterministic and do not depend on insertion order. A missing locale is
/// never a fabricated row.
#[derive(Clone, Debug, Default)]
pub struct TextCatalog {
    rows: BTreeMap<(TextId, LocaleId), LocalizedText>,
}

impl TextCatalog {
    /// An empty catalog.
    #[must_use]
    pub fn new() -> Self {
        Self {
            rows: BTreeMap::new(),
        }
    }

    /// Inserts a row, refusing a `(id, locale)` pair that is already present.
    ///
    /// # Errors
    ///
    /// [`TextCatalogError::DuplicateEntry`].
    pub fn insert(&mut self, row: LocalizedText) -> Result<(), TextCatalogError> {
        let key = (row.id().clone(), row.locale().clone());
        if self.rows.contains_key(&key) {
            return Err(TextCatalogError::DuplicateEntry {
                id: key.0,
                locale: key.1,
            });
        }
        self.rows.insert(key, row);
        Ok(())
    }

    /// The row for `id` in `locale`, if the catalog holds one.
    #[must_use]
    pub fn get(&self, id: &TextId, locale: &LocaleId) -> Option<&LocalizedText> {
        self.rows.get(&(id.clone(), locale.clone()))
    }

    /// How many rows the catalog holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the catalog holds no rows.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Every row, in `(id, locale)` order.
    pub fn rows(&self) -> impl Iterator<Item = &LocalizedText> {
        self.rows.values()
    }

    /// The distinct string ids the catalog holds, in id order.
    ///
    /// The rows are keyed by `(id, locale)` in that order, so the equal ids of
    /// one string are adjacent and the deduplication is exact: an id that is
    /// translated into three locales is reported **once**, so a coverage
    /// denominator counts strings and not translations.
    #[must_use]
    pub fn ids(&self) -> Vec<TextId> {
        let mut ids: Vec<TextId> = self.rows.keys().map(|(id, _)| id.clone()).collect();
        ids.dedup();
        ids
    }

    /// The locales the catalog holds at least one row for, deduplicated and
    /// sorted.
    #[must_use]
    pub fn locales(&self) -> Vec<LocaleId> {
        let mut locales: Vec<LocaleId> =
            self.rows.keys().map(|(_, locale)| locale.clone()).collect();
        locales.sort();
        locales.dedup();
        locales
    }

    /// Resolves `id` through `chain`, reporting which locale answered and how
    /// far down the chain it was.
    ///
    /// A chain is walked in order and the first locale with a row wins. The
    /// reported `depth` is `0` for the selected locale, so a caller can tell a
    /// real translation from a fallback without comparing labels, and
    /// [`TextResolution::Missing`] names every locale that was tried — the
    /// string is never invented.
    #[must_use]
    pub fn resolve(&self, id: &TextId, chain: &LocaleChain) -> TextResolution<'_> {
        for (depth, locale) in chain.locales().iter().enumerate() {
            if let Some(row) = self.get(id, locale) {
                return TextResolution::Resolved {
                    row,
                    locale: locale.clone(),
                    depth,
                    used_fallback: depth > 0,
                };
            }
        }
        TextResolution::Missing {
            id: id.clone(),
            tried: chain.locales().to_vec(),
        }
    }

    /// Audits every distinct id in the catalog against `chain`.
    ///
    /// This is the machine-readable shape of F51's AC04 ("audit all strings and
    /// media for each declared supported original locale"): the coverage
    /// denominator is the catalog's own **distinct** ids
    /// ([`TextCatalog::ids`], not its row count), so a string translated into
    /// three locales counts once however many rows it has, and an id with no
    /// row anywhere in the chain stays counted as missing instead of being
    /// dropped.
    #[must_use]
    pub fn audit(&self, chain: &LocaleChain) -> LocaleAudit {
        let mut audit = LocaleAudit {
            chain: chain.locales().to_vec(),
            ..LocaleAudit::default()
        };
        for id in self.ids() {
            audit.ids += 1;
            match self.resolve(&id, chain) {
                TextResolution::Resolved { depth, .. } => {
                    audit.resolved += 1;
                    if depth > 0 {
                        audit.served_by_fallback += 1;
                    }
                }
                TextResolution::Missing { .. } => {
                    audit.missing.push(id);
                }
            }
        }
        audit
    }
}

/// The answer to one [`TextCatalog::resolve`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextResolution<'a> {
    /// One locale in the chain had a row for the id.
    Resolved {
        /// The row that answered.
        row: &'a LocalizedText,
        /// The locale whose row answered.
        locale: LocaleId,
        /// How far down the chain the answering locale was (`0` is the
        /// selected locale).
        depth: usize,
        /// Whether the answer came from a fallback locale rather than the
        /// selected one.
        used_fallback: bool,
    },
    /// No locale in the chain had a row for the id.
    Missing {
        /// The requested id.
        id: TextId,
        /// Every locale that was tried, in chain order.
        tried: Vec<LocaleId>,
    },
}

impl<'a> TextResolution<'a> {
    /// The answering row, or `None` when the id is missing.
    #[must_use]
    pub fn row(&self) -> Option<&'a LocalizedText> {
        match self {
            Self::Resolved { row, .. } => Some(row),
            Self::Missing { .. } => None,
        }
    }

    /// The answering locale, or `None` when the id is missing.
    #[must_use]
    pub fn locale(&self) -> Option<&LocaleId> {
        match self {
            Self::Resolved { locale, .. } => Some(locale),
            Self::Missing { .. } => None,
        }
    }

    /// The chain depth that answered, or `None` when the id is missing.
    #[must_use]
    pub fn depth(&self) -> Option<usize> {
        match self {
            Self::Resolved { depth, .. } => Some(*depth),
            Self::Missing { .. } => None,
        }
    }

    /// Whether the answer came from a fallback locale. `false` when missing:
    /// nothing answered at all.
    #[must_use]
    pub fn used_fallback(&self) -> bool {
        matches!(
            self,
            Self::Resolved {
                used_fallback: true,
                ..
            }
        )
    }

    /// The text to display, or `None` when the id is missing. A missing id
    /// never becomes an empty or placeholder string here.
    #[must_use]
    pub fn text(&self) -> Option<&'a str> {
        self.row().map(LocalizedText::text)
    }
}

/// The coverage of one catalog against one locale chain.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LocaleAudit {
    /// The chain that was audited, in order.
    pub chain: Vec<LocaleId>,
    /// How many distinct string ids the catalog holds — the coverage
    /// denominator, never the row count.
    pub ids: usize,
    /// How many ids some locale in the chain answered.
    pub resolved: usize,
    /// How many ids were answered by a fallback locale rather than the
    /// selected one.
    pub served_by_fallback: usize,
    /// Ids no locale in the chain answered, in id order.
    pub missing: Vec<TextId>,
}

impl LocaleAudit {
    /// Whether every id resolved somewhere in the chain.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.missing.is_empty()
    }

    /// The share of the catalog's distinct ids some locale in the chain
    /// answered, in `0.0..=1.0`.
    ///
    /// The denominator is [`LocaleAudit::ids`] — distinct strings, not rows — so
    /// a string translated into several locales is not counted several times.
    /// A catalog with no ids at all reports `1.0`: there is nothing missing.
    #[must_use]
    pub fn coverage(&self) -> f32 {
        if self.ids == 0 {
            return 1.0;
        }
        self.resolved as f32 / self.ids as f32
    }

    /// The same coverage expressed as whole percent, for a report a human or a
    /// packager reads.
    #[must_use]
    pub fn coverage_percent(&self) -> u32 {
        (self.coverage() * 100.0).round() as u32
    }
}

// ------------------------------------------------------- resource decode ---

/// The typed result of decoding F12's PE string rows into a [`TextCatalog`],
/// with everything that could not become a row named instead of hidden.
///
/// F12 ([`crate::config::StringCatalog`]) answers "which strings does this
/// image carry, in which language, with which code units". F51 answers "which
/// string answers this id in this locale". This type is the bridge: each row
/// whose language the caller's [`LanguageMap`] declares **and** whose code
/// units decoded becomes a [`LocalizedText`] under the id
/// [`TextId::from_resource_id`] builds. Nothing else is invented:
///
/// * a row in a language the map does not declare is left out and its language
///   recorded in [`ResourceDecode::unmapped_languages`];
/// * a row whose units did not decode (`StringRow::text` is `None`) is left out
///   and its id recorded in [`ResourceDecode::undecodable_ids`];
/// * a duplicate `(id, locale)` pair is a contradiction, so **neither** copy is
///   kept and the pair is recorded in [`ResourceDecode::duplicates`] — matching
///   F12's own refusal to choose between two strings that share an id.
#[derive(Clone, Debug, Default)]
pub struct ResourceDecode {
    catalog: TextCatalog,
    rows: usize,
    unmapped_languages: Vec<u32>,
    undecodable_ids: Vec<u32>,
    duplicates: Vec<(u32, LocaleId)>,
}

impl ResourceDecode {
    /// Decodes `rows` into a catalog using the caller's language map.
    ///
    /// `origin` and `provenance` describe where the rows came from and are
    /// attached to every decoded row; a caller reading the owner's installation
    /// passes an [`Origin::Installation`] span and an evidence provenance, and a
    /// synthetic test passes [`Origin::SyntheticFixture`] and a designed one.
    /// This function copies what it is given and asserts no evidence itself.
    #[must_use]
    pub fn decode(
        rows: &[StringRow],
        locales: &LanguageMap,
        origin: Origin,
        provenance: Provenance,
    ) -> Self {
        // Pass one: map each declared-language row to its `(id, locale)` key and
        // count collisions so a duplicated key can be excluded as a pair rather
        // than resolved by whichever copy was seen first. An undecodable row
        // still takes part in the collision count: F12 counts every row that
        // shares an `(id, language)` regardless of whether its units decoded, so
        // a pair where one copy is undecodable is still a contradiction, not a
        // licence to keep the other copy.
        let mut keys: Vec<Option<(TextId, LocaleId)>> = Vec::with_capacity(rows.len());
        let mut counts: BTreeMap<(TextId, LocaleId), usize> = BTreeMap::new();
        let mut unmapped: BTreeSet<u32> = BTreeSet::new();
        let mut undecodable: BTreeSet<u32> = BTreeSet::new();
        for row in rows {
            let Some(locale) = locales.locale(row.language) else {
                unmapped.insert(row.language);
                keys.push(None);
                continue;
            };
            if row.text.is_none() {
                undecodable.insert(row.id);
            }
            let key = (TextId::from_resource_id(row.id), locale.clone());
            *counts.entry(key.clone()).or_insert(0) += 1;
            keys.push(Some(key));
        }
        let duplicate_keys: BTreeSet<(TextId, LocaleId)> = counts
            .iter()
            .filter(|(_, count)| **count > 1)
            .map(|(key, _)| key.clone())
            .collect();

        // Pass two: keep exactly the rows with a unique key and decodable units.
        let mut catalog = TextCatalog::new();
        for (row, key) in rows.iter().zip(&keys) {
            let Some(key) = key else {
                continue;
            };
            if duplicate_keys.contains(key) || row.text.is_none() {
                continue;
            }
            let text = row
                .text
                .clone()
                .expect("only a row with text keeps a key into pass two");
            catalog
                .insert(LocalizedText::new(
                    key.0.clone(),
                    key.1.clone(),
                    text,
                    origin.clone(),
                    provenance.clone(),
                ))
                .expect("a unique (id, locale) key is inserted exactly once");
        }

        let mut duplicates: Vec<(u32, LocaleId)> = duplicate_keys
            .iter()
            .map(|(id, locale)| {
                let resource_id = id
                    .as_content_id()
                    .key()
                    .parse::<u32>()
                    .expect("a decoded id is a decimal resource id");
                (resource_id, locale.clone())
            })
            .collect();
        duplicates.sort();

        Self {
            catalog,
            rows: rows.len(),
            unmapped_languages: unmapped.into_iter().collect(),
            undecodable_ids: undecodable.into_iter().collect(),
            duplicates,
        }
    }

    /// The catalog the decoded rows form.
    #[must_use]
    pub fn catalog(&self) -> &TextCatalog {
        &self.catalog
    }

    /// Consumes the decode and returns the catalog.
    #[must_use]
    pub fn into_catalog(self) -> TextCatalog {
        self.catalog
    }

    /// How many resource rows were handed in.
    #[must_use]
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// How many rows became localized rows.
    #[must_use]
    pub fn decoded(&self) -> usize {
        self.catalog.len()
    }

    /// The distinct resource language ids no [`LanguageMap`] entry declared,
    /// in ascending order.
    #[must_use]
    pub fn unmapped_languages(&self) -> &[u32] {
        &self.unmapped_languages
    }

    /// The distinct resource ids whose code units did not decode, in ascending
    /// order.
    #[must_use]
    pub fn undecodable_ids(&self) -> &[u32] {
        &self.undecodable_ids
    }

    /// The duplicated `(resource id, locale)` pairs, one entry per pair, in
    /// ascending order. Every row that shared the pair — decodable or not — was
    /// left out.
    #[must_use]
    pub fn duplicates(&self) -> &[(u32, LocaleId)] {
        &self.duplicates
    }

    /// Whether every handed-in row became a unique localized row.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.unmapped_languages.is_empty()
            && self.undecodable_ids.is_empty()
            && self.duplicates.is_empty()
    }
}

// ----------------------------------------------------------------- markup --

/// The four delimiter characters a [`MarkupGrammar`] declares.
///
/// The delimiter spelling is **caller-declared**, not assumed: the original
/// control-markup syntax is unmeasured at this stage (see the stage finding),
/// so the token model is designed here and the concrete delimiters are supplied
/// by whoever measures them. The defaults are this project's own choice for the
/// synthetic fixtures and are marked as such.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarkupDelimiters {
    /// Opens a control or a substitution.
    pub control_open: char,
    /// Closes a control.
    pub control_close: char,
    /// Opens a substitution.
    pub substitution_open: char,
    /// Closes a substitution.
    pub substitution_close: char,
}

impl MarkupDelimiters {
    /// The project's declared spelling: `[tag]`, `[/tag]`, `{id}`.
    ///
    /// This is a **designed** grammar for the synthetic fixture, not a
    /// measurement of the original's control markup.
    pub const DECLARED: Self = Self {
        control_open: '[',
        control_close: ']',
        substitution_open: '{',
        substitution_close: '}',
    };

    /// Validates that the four delimiters are four distinct characters and
    /// that none of them is alphanumeric, so plain prose can never be mistaken
    /// for markup.
    ///
    /// # Errors
    ///
    /// [`MarkupDelimiterError::Duplicate`] or
    /// [`MarkupDelimiterError::Alphanumeric`].
    pub fn validate(self) -> Result<Self, MarkupDelimiterError> {
        let all = [
            self.control_open,
            self.control_close,
            self.substitution_open,
            self.substitution_close,
        ];
        for (index, ch) in all.iter().enumerate() {
            if ch.is_alphanumeric() {
                return Err(MarkupDelimiterError::Alphanumeric { ch: *ch });
            }
            for other in &all[index + 1..] {
                if other == ch {
                    return Err(MarkupDelimiterError::Duplicate { ch: *ch });
                }
            }
        }
        Ok(self)
    }
}

/// Why a [`MarkupDelimiters`] spelling was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkupDelimiterError {
    /// The same character was used for two roles, so a token could not be
    /// parsed unambiguously.
    Duplicate {
        /// The repeated character.
        ch: char,
    },
    /// An alphanumeric was used as a delimiter, so ordinary words in a
    /// translation would be read as markup.
    Alphanumeric {
        /// The offending character.
        ch: char,
    },
}

impl fmt::Display for MarkupDelimiterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Duplicate { ch } => {
                write!(f, "markup delimiter {ch:?} is used for two different roles")
            }
            Self::Alphanumeric { ch } => write!(
                f,
                "markup delimiter {ch:?} is alphanumeric and would parse plain words as markup"
            ),
        }
    }
}

impl std::error::Error for MarkupDelimiterError {}

/// Maximum byte length of a control tag or substitution id.
pub const MAX_MARKUP_TOKEN_LEN: usize = 32;

/// One control tag the grammar admits.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ControlTag {
    name: String,
    takes_argument: bool,
}

impl ControlTag {
    /// Declares a tag that carries no argument.
    ///
    /// # Errors
    ///
    /// [`MarkupGrammarError::BadTagName`] when the name is empty, over-long or
    /// outside the token grammar.
    pub fn simple(name: &str) -> Result<Self, MarkupGrammarError> {
        Ok(Self {
            name: validate_token(name)?.to_owned(),
            takes_argument: false,
        })
    }

    /// Declares a tag that carries one `=argument` value.
    ///
    /// # Errors
    ///
    /// [`MarkupGrammarError::BadTagName`] as [`ControlTag::simple`].
    pub fn with_argument(name: &str) -> Result<Self, MarkupGrammarError> {
        Ok(Self {
            name: validate_token(name)?.to_owned(),
            takes_argument: true,
        })
    }

    /// The tag's name as it is written between the control delimiters.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether the tag may carry an argument.
    #[must_use]
    pub fn takes_argument(&self) -> bool {
        self.takes_argument
    }
}

impl fmt::Display for ControlTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}

/// Why a [`MarkupGrammar`] was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkupGrammarError {
    /// A tag name was empty, over [`MAX_MARKUP_TOKEN_LEN`] bytes, or carried a
    /// character outside `[A-Za-z0-9_-]`.
    BadTagName {
        /// The rejected name.
        name: String,
    },
    /// The same tag name was declared twice, so its argument rule was
    /// ambiguous.
    DuplicateTag {
        /// The repeated name.
        name: String,
    },
    /// The grammar declared no tags at all, so every control in a resource
    /// string would be an issue.
    NoTags,
}

impl fmt::Display for MarkupGrammarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadTagName { name } => write!(
                f,
                "markup tag name {name:?} must be 1..={MAX_MARKUP_TOKEN_LEN} bytes of [A-Za-z0-9_-]"
            ),
            Self::DuplicateTag { name } => {
                write!(f, "markup tag {name:?} is declared more than once")
            }
            Self::NoTags => f.write_str("a markup grammar must declare at least one control tag"),
        }
    }
}

impl std::error::Error for MarkupGrammarError {}

/// The declared control-markup grammar a resource string is validated against.
///
/// This is the whole of "grammar validation" at this stage: a caller declares
/// the delimiter spelling and the admitted tags, and [`parse_markup`] refuses
/// everything else. A grammar is never inferred from the string being parsed,
/// so a resource string can never widen its own grammar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkupGrammar {
    delimiters: MarkupDelimiters,
    tags: BTreeMap<String, ControlTag>,
}

impl MarkupGrammar {
    /// Validates and assembles a grammar.
    ///
    /// # Errors
    ///
    /// [`MarkupDelimiterError`] for a bad delimiter spelling and
    /// [`MarkupGrammarError`] for a bad, duplicated or empty tag set.
    pub fn new(
        delimiters: MarkupDelimiters,
        tags: impl IntoIterator<Item = ControlTag>,
    ) -> Result<Self, MarkupGrammarErrorOrDelimiters> {
        let delimiters = delimiters
            .validate()
            .map_err(MarkupGrammarErrorOrDelimiters::Delimiters)?;
        let mut map: BTreeMap<String, ControlTag> = BTreeMap::new();
        for tag in tags {
            let name = tag.name().to_owned();
            if map.contains_key(&name) {
                return Err(MarkupGrammarError::DuplicateTag { name }.into());
            }
            map.insert(name, tag);
        }
        if map.is_empty() {
            return Err(MarkupGrammarError::NoTags.into());
        }
        Ok(Self {
            delimiters,
            tags: map,
        })
    }

    /// The delimiter spelling this grammar validates against.
    #[must_use]
    pub fn delimiters(&self) -> MarkupDelimiters {
        self.delimiters
    }

    /// The admitted tags, in name order.
    pub fn tags(&self) -> impl Iterator<Item = &ControlTag> {
        self.tags.values()
    }

    /// The admitted tag called `name`, if any.
    #[must_use]
    pub fn tag(&self, name: &str) -> Option<&ControlTag> {
        self.tags.get(name)
    }
}

/// Why a [`MarkupGrammar`] could not be built: either the tag set or the
/// delimiter spelling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkupGrammarErrorOrDelimiters {
    /// The tag set was refused.
    Grammar(MarkupGrammarError),
    /// The delimiter spelling was refused.
    Delimiters(MarkupDelimiterError),
}

impl From<MarkupGrammarError> for MarkupGrammarErrorOrDelimiters {
    fn from(error: MarkupGrammarError) -> Self {
        Self::Grammar(error)
    }
}

impl From<MarkupDelimiterError> for MarkupGrammarErrorOrDelimiters {
    fn from(error: MarkupDelimiterError) -> Self {
        Self::Delimiters(error)
    }
}

impl fmt::Display for MarkupGrammarErrorOrDelimiters {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Grammar(error) => write!(f, "{error}"),
            Self::Delimiters(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for MarkupGrammarErrorOrDelimiters {}

/// One validated piece of a localized string.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkupToken {
    /// Literal text, kept verbatim. Includes any markup the grammar refused:
    /// an uninterpreted control is *text*, never dropped and never executed.
    Text(String),
    /// A hard line break the resource declared (`\n`).
    LineBreak,
    /// An admitted control tag, with its argument when the grammar allows one.
    ///
    /// A closing control is the *same* tag, flagged by [`MarkupToken::is_closing`]:
    /// a renderer pushes the style an opening control names and pops it at the
    /// matching close, and it can only do that if the token stream says which of
    /// the two a token is.
    Control {
        /// The tag's name.
        name: String,
        /// The `=argument` value, when the tag takes one. A closing control
        /// never carries one.
        argument: Option<String>,
        /// Whether this control closes the tag it names rather than opening it.
        closing: bool,
    },
    /// An admitted substitution: an id whose value the layout supplies.
    Substitution {
        /// The substitution id, as written between the substitution
        /// delimiters.
        id: String,
    },
}

impl MarkupToken {
    /// The tag name of a [`MarkupToken::Control`], and nothing for any other
    /// token.
    #[must_use]
    pub fn control_name(&self) -> Option<&str> {
        match self {
            Self::Control { name, .. } => Some(name),
            Self::Text(_) | Self::LineBreak | Self::Substitution { .. } => None,
        }
    }

    /// Whether this token is a control that **closes** the tag it names.
    ///
    /// A renderer needs the distinction: an opening control pushes a style and
    /// the matching close pops it, and the grammar's balance check already
    /// guarantees the two pair up in order.
    #[must_use]
    pub fn is_closing(&self) -> bool {
        matches!(self, Self::Control { closing: true, .. })
    }

    /// Whether this token is a control that **opens** the tag it names.
    #[must_use]
    pub fn is_opening(&self) -> bool {
        matches!(self, Self::Control { closing: false, .. })
    }
}

/// What kind of markup problem was found, and where.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkupIssueKind {
    /// A control delimiter was opened and never closed.
    UnterminatedControl {
        /// The byte offset the control was opened at.
        offset: usize,
    },
    /// A closing control named a tag that is not open.
    UnbalancedControl {
        /// The tag name that was closed.
        name: String,
        /// The byte offset the closing control was at.
        offset: usize,
    },
    /// A control was left open when the string ended.
    UnclosedControl {
        /// The tag name that stayed open.
        name: String,
    },
    /// A control named a tag the grammar does not admit, so it stays literal
    /// text.
    UnknownTag {
        /// The unrecognized tag name.
        name: String,
        /// The byte offset the control was at.
        offset: usize,
    },
    /// A control carried an argument although its tag admits none, so the whole
    /// control stays literal text.
    UnexpectedArgument {
        /// The tag name.
        name: String,
        /// The byte offset the control was at.
        offset: usize,
    },
    /// A control had no name at all.
    EmptyTag {
        /// The byte offset the control was at.
        offset: usize,
    },
    /// A substitution delimiter was opened and never closed.
    UnterminatedSubstitution {
        /// The byte offset the substitution was opened at.
        offset: usize,
    },
    /// A substitution id was empty.
    EmptySubstitution {
        /// The byte offset the substitution was at.
        offset: usize,
    },
    /// A substitution id was over [`MAX_MARKUP_TOKEN_LEN`] bytes.
    SubstitutionIdTooLong {
        /// Its length in bytes.
        len: usize,
    },
    /// A substitution id was non-empty but outside the `[A-Za-z0-9_-]` token
    /// grammar, so it stays literal text.
    ///
    /// This is a different problem from [`MarkupIssueKind::EmptySubstitution`]:
    /// the author wrote an id, and the diagnostic has to name it so the
    /// resource can be fixed, rather than claiming there was no id at all.
    BadSubstitutionId {
        /// The rejected id, as written between the substitution delimiters.
        id: String,
        /// The byte offset the substitution was at.
        offset: usize,
    },
}

impl MarkupIssueKind {
    /// A stable, machine-matchable label for reports and evidence files.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnterminatedControl { .. } => "unterminated_control",
            Self::UnbalancedControl { .. } => "unbalanced_control",
            Self::UnclosedControl { .. } => "unclosed_control",
            Self::UnknownTag { .. } => "unknown_tag",
            Self::UnexpectedArgument { .. } => "unexpected_argument",
            Self::EmptyTag { .. } => "empty_tag",
            Self::UnterminatedSubstitution { .. } => "unterminated_substitution",
            Self::EmptySubstitution { .. } => "empty_substitution",
            Self::SubstitutionIdTooLong { .. } => "substitution_id_too_long",
            Self::BadSubstitutionId { .. } => "bad_substitution_id",
        }
    }
}

impl fmt::Display for MarkupIssueKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnterminatedControl { .. } => f.write_str("a control delimiter was never closed"),
            Self::UnbalancedControl { name, .. } => {
                write!(f, "closing control {name:?} does not match an open control")
            }
            Self::UnclosedControl { name } => {
                write!(f, "control {name:?} was left open at the end of the string")
            }
            Self::UnknownTag { name, .. } => write!(
                f,
                "control tag {name:?} is not declared by the grammar and stays literal text"
            ),
            Self::UnexpectedArgument { name, .. } => write!(
                f,
                "control tag {name:?} admits no argument, so the control stays literal text"
            ),
            Self::EmptyTag { .. } => f.write_str("a control had no tag name"),
            Self::UnterminatedSubstitution { .. } => {
                f.write_str("a substitution delimiter was never closed")
            }
            Self::EmptySubstitution { .. } => f.write_str("a substitution had no id"),
            Self::SubstitutionIdTooLong { len } => write!(
                f,
                "a substitution id is {len} bytes, max is {MAX_MARKUP_TOKEN_LEN}"
            ),
            Self::BadSubstitutionId { id, .. } => write!(
                f,
                "substitution id {id:?} is outside the [A-Za-z0-9_-] grammar and stays literal text"
            ),
        }
    }
}

/// One markup problem, with where in the string it was found.
///
/// The byte offset is into the **original** string, so a diagnostic points at
/// the exact place in the resource that has to be fixed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkupIssue {
    /// What kind of problem this is.
    pub kind: MarkupIssueKind,
    /// A human-readable rendering of [`MarkupIssue::kind`].
    pub detail: String,
}

impl MarkupIssue {
    /// Builds an issue with its rendered detail.
    #[must_use]
    pub fn new(kind: MarkupIssueKind) -> Self {
        let detail = kind.to_string();
        Self { kind, detail }
    }

    /// The stable label of the problem kind.
    #[must_use]
    pub fn code(&self) -> &'static str {
        self.kind.code()
    }

    /// The byte offset the problem was found at, when the kind carries one.
    #[must_use]
    pub fn offset(&self) -> Option<usize> {
        match self.kind {
            MarkupIssueKind::UnterminatedControl { offset }
            | MarkupIssueKind::UnbalancedControl { offset, .. }
            | MarkupIssueKind::UnknownTag { offset, .. }
            | MarkupIssueKind::UnexpectedArgument { offset, .. }
            | MarkupIssueKind::EmptyTag { offset }
            | MarkupIssueKind::UnterminatedSubstitution { offset }
            | MarkupIssueKind::BadSubstitutionId { offset, .. }
            | MarkupIssueKind::EmptySubstitution { offset } => Some(offset),
            MarkupIssueKind::UnclosedControl { .. }
            | MarkupIssueKind::SubstitutionIdTooLong { .. } => None,
        }
    }
}

impl fmt::Display for MarkupIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.offset() {
            Some(offset) => write!(f, "{} at byte {offset}", self.detail),
            None => f.write_str(&self.detail),
        }
    }
}

/// The validated token stream of one localized string, plus every problem the
/// grammar found.
///
/// A document with issues is still a document: the refused controls are
/// [`MarkupToken::Text`], so a caller can display the string verbatim *and*
/// report the problem. Nothing is silently dropped and nothing unvalidated is
/// interpreted.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MarkupDocument {
    tokens: Vec<MarkupToken>,
    issues: Vec<MarkupIssue>,
}

impl MarkupDocument {
    /// The token stream.
    #[must_use]
    pub fn tokens(&self) -> &[MarkupToken] {
        &self.tokens
    }

    /// Every problem the grammar found, in the order it was found.
    #[must_use]
    pub fn issues(&self) -> &[MarkupIssue] {
        &self.issues
    }

    /// Whether the grammar accepted the whole string.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.issues.is_empty()
    }

    /// The paragraphs the tokens form, split on the hard line breaks the
    /// resource declared, with substitutions replaced by `substitutions`.
    ///
    /// An id with no supplied value becomes the visible
    /// [`UNRESOLVED_SUBSTITUTION`] marker rather than an empty gap, and the
    /// name is appended to `unresolved`, in first-appearance order and without
    /// repeats. Use [`MarkupDocument::paragraph_substitutions`] when the
    /// position of each marker matters.
    #[must_use]
    pub fn paragraphs(
        &self,
        substitutions: &SubstitutionTable,
    ) -> (Vec<String>, Vec<SubstitutionId>) {
        let (paragraphs, per_paragraph) = self.paragraph_substitutions(substitutions);
        let mut seen: BTreeSet<SubstitutionId> = BTreeSet::new();
        let unresolved: Vec<SubstitutionId> = per_paragraph
            .into_iter()
            .flatten()
            .filter(|id| seen.insert(id.clone()))
            .collect();
        (paragraphs, unresolved)
    }

    /// The paragraphs the tokens form together with, **per paragraph**, the
    /// substitution ids that paragraph could not resolve — in the order they
    /// appear and **with repeats**.
    ///
    /// This is the positional form of [`MarkupDocument::paragraphs`]: a layout
    /// needs to know *which* paragraph each unresolved id is in, and in which
    /// order the markers appear, to name the line a diagnostic belongs to. A
    /// deduplicated global list cannot say that, so it is not used for that.
    #[must_use]
    pub fn paragraph_substitutions(
        &self,
        substitutions: &SubstitutionTable,
    ) -> (Vec<String>, Vec<Vec<SubstitutionId>>) {
        let mut paragraphs = vec![String::new()];
        // One entry per paragraph, so each id keeps the paragraph its marker
        // renders in.
        let mut unresolved: Vec<Vec<SubstitutionId>> = vec![Vec::new()];
        for token in &self.tokens {
            match token {
                MarkupToken::Text(text) => {
                    paragraphs
                        .last_mut()
                        .expect("one paragraph exists")
                        .push_str(text);
                }
                MarkupToken::LineBreak => {
                    paragraphs.push(String::new());
                    unresolved.push(Vec::new());
                }
                MarkupToken::Control { .. } => {}
                MarkupToken::Substitution { id } => match substitutions.get(id) {
                    Some(value) => paragraphs
                        .last_mut()
                        .expect("one paragraph exists")
                        .push_str(value),
                    None => {
                        paragraphs
                            .last_mut()
                            .expect("one paragraph exists")
                            .push_str(UNRESOLVED_SUBSTITUTION);
                        unresolved
                            .last_mut()
                            .expect("one paragraph exists")
                            .push(id.clone());
                    }
                },
            }
        }
        (paragraphs, unresolved)
    }
}

/// The visible marker a substitution with no supplied value renders as.
///
/// It is deliberately not empty: an unresolved substitution must be *seen* in
/// the layout rather than leave a hole that reads as correct output.
pub const UNRESOLVED_SUBSTITUTION: &str = "\u{fffd}";

/// A validated substitution id: the `id` in `{id}`.
pub type SubstitutionId = String;

/// Why a [`SubstitutionTable`] refused a value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubstitutionValueError {
    /// The id was empty or outside the `[A-Za-z0-9_-]` token grammar, so it
    /// could never be written as `{id}` in a resource string.
    BadId {
        /// The rejected id.
        id: String,
    },
    /// The value contained [`UNRESOLVED_SUBSTITUTION`].
    ///
    /// That marker is how an *unresolved* substitution is made visible, so a
    /// value carrying it would be indistinguishable from one: a layout counts
    /// the markers in a paragraph to name the line each unresolved id landed on,
    /// and a marker a caller supplied would be counted as somebody else's
    /// diagnostic. The marker is the replacement character, so a value
    /// containing it is already damaged text and is refused rather than
    /// silently made ambiguous.
    MarkerInValue {
        /// The id whose value was refused.
        id: String,
    },
}

impl fmt::Display for SubstitutionValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadId { id } => write!(
                f,
                "substitution id {id:?} must be 1..={MAX_MARKUP_TOKEN_LEN} bytes of [A-Za-z0-9_-]"
            ),
            Self::MarkerInValue { id } => write!(
                f,
                "the value for substitution {id:?} contains the unresolved-substitution marker"
            ),
        }
    }
}

impl std::error::Error for SubstitutionValueError {}

/// The substitution values a layout supplies for a document.
///
/// The table is the *caller's* data — a pilot name, a count, a key binding —
/// so a localized string stays a template and the values that vary stay
/// outside it. A value the caller does not supply is
/// [`UNRESOLVED_SUBSTITUTION`], never an empty string.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SubstitutionTable {
    values: BTreeMap<SubstitutionId, String>,
}

impl SubstitutionTable {
    /// An empty table.
    #[must_use]
    pub fn new() -> Self {
        Self {
            values: BTreeMap::new(),
        }
    }

    /// Supplies a value for a substitution id.
    ///
    /// The id must be writable as `{id}` in a resource string, and the value
    /// must not contain [`UNRESOLVED_SUBSTITUTION`], so that a marker in the
    /// rendered text always means *this* substitution was unresolved.
    ///
    /// # Errors
    ///
    /// [`SubstitutionValueError::BadId`] for an id the token grammar rejects
    /// and [`SubstitutionValueError::MarkerInValue`] for a value that would be
    /// indistinguishable from an unresolved substitution.
    pub fn insert(
        &mut self,
        id: impl Into<SubstitutionId>,
        value: impl Into<String>,
    ) -> Result<(), SubstitutionValueError> {
        let id = id.into();
        validate_token(&id).map_err(|_| SubstitutionValueError::BadId { id: id.clone() })?;
        let value = value.into();
        if is_marker(&value) {
            return Err(SubstitutionValueError::MarkerInValue { id });
        }
        self.values.insert(id, value);
        Ok(())
    }

    /// The value supplied for `id`, if any.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&str> {
        self.values.get(id).map(String::as_str)
    }

    /// Whether `id` has a supplied value.
    #[must_use]
    pub fn contains(&self, id: &str) -> bool {
        self.values.contains_key(id)
    }

    /// How many values the table holds.
    #[must_use]
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether the table holds no values.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// Whether `text` holds the visible unresolved-substitution marker.
///
/// The marker is [`UNRESOLVED_SUBSTITUTION`], one character, and it is what a
/// layout counts to name the line each unresolved id landed on — so a
/// *supplied* value may not contain it either.
fn is_marker(text: &str) -> bool {
    text.contains(UNRESOLVED_SUBSTITUTION)
}

/// Validates a control tag or substitution id against the token grammar.
fn validate_token(token: &str) -> Result<&str, MarkupGrammarError> {
    let bad = |name: &str| {
        Err(MarkupGrammarError::BadTagName {
            name: name.to_owned(),
        })
    };
    if token.is_empty() || token.len() > MAX_MARKUP_TOKEN_LEN {
        return bad(token);
    }
    for ch in token.chars() {
        if !ch.is_ascii_alphanumeric() && !matches!(ch, '-' | '_') {
            return bad(token);
        }
    }
    Ok(token)
}

/// Validates a localized string against a [`MarkupGrammar`].
///
/// This is the only place a resource string becomes tokens, and it interprets
/// **nothing** the grammar does not admit:
///
/// * a `\n` is a hard [`MarkupToken::LineBreak`] the resource declared;
/// * a substitution is a token only when its id validates;
/// * a control is a token only when the grammar declares the tag *and* the
///   tag's argument rule matches;
/// * everything else — an unknown tag, an argument where none is admitted, an
///   unbalanced or unterminated control — is recorded as a [`MarkupIssue`] and
///   stays [`MarkupToken::Text`], verbatim.
///
/// Nesting is not inferred: a control is a flat token, because whether the
/// original markup nests is unmeasured. A `[/tag]` is checked against the
/// innermost open tag so an unbalanced close is reported instead of silently
/// balancing.
#[must_use]
pub fn parse_markup(text: &str, grammar: &MarkupGrammar) -> MarkupDocument {
    let delimiters = grammar.delimiters();
    let mut document = MarkupDocument::default();
    let mut literal = String::new();
    let mut open: Vec<String> = Vec::new();
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut index = 0usize;

    while index < chars.len() {
        let (offset, ch) = chars[index];
        if ch == '\n' {
            flush(&mut document, &mut literal);
            document.tokens.push(MarkupToken::LineBreak);
            index += 1;
            continue;
        }
        if ch == delimiters.substitution_open {
            match find_close(text, offset, delimiters) {
                Some(closed) => {
                    parse_substitution(&mut document, &mut literal, delimiters, &closed, offset);
                    index = chars.partition_point(|(at, _)| *at < closed.end);
                }
                None => {
                    document.issues.push(MarkupIssue::new(
                        MarkupIssueKind::UnterminatedSubstitution { offset },
                    ));
                    literal.push(ch);
                    index += 1;
                }
            }
            continue;
        }
        if ch == delimiters.control_open {
            match find_close(text, offset, delimiters) {
                Some(closed) => {
                    parse_control(
                        &mut document,
                        &mut literal,
                        grammar,
                        delimiters,
                        &closed,
                        offset,
                        &mut open,
                    );
                    index = chars.partition_point(|(at, _)| *at < closed.end);
                }
                None => {
                    document
                        .issues
                        .push(MarkupIssue::new(MarkupIssueKind::UnterminatedControl {
                            offset,
                        }));
                    literal.push(ch);
                    index += 1;
                }
            }
            continue;
        }
        literal.push(ch);
        index += 1;
    }

    flush(&mut document, &mut literal);
    for name in open.into_iter().rev() {
        document
            .issues
            .push(MarkupIssue::new(MarkupIssueKind::UnclosedControl { name }));
    }
    document
}

/// Moves a pending literal run into the token stream.
fn flush(document: &mut MarkupDocument, literal: &mut String) {
    if !literal.is_empty() {
        document
            .tokens
            .push(MarkupToken::Text(std::mem::take(literal)));
    }
}

/// A matched delimiter pair inside the string being parsed.
struct Closed<'a> {
    /// The byte offset just past the closing delimiter.
    end: usize,
    /// The text between the delimiters.
    body: &'a str,
    /// Which role the pair has, so a refused token can be rebuilt with the
    /// exact delimiters the string used.
    role: DelimiterRole,
}

/// Finds the closing delimiter for the pair that opened at `from`.
///
/// The caller has already checked which of the two opening delimiters sits at
/// `from`, so the matching closing character is decided here from the same
/// [`MarkupDelimiters`]. No delimiter is hard-coded: a grammar that declares a
/// different spelling is honoured end to end.
fn find_close(text: &str, from: usize, delimiters: MarkupDelimiters) -> Option<Closed<'_>> {
    let opening = text.get(from..)?;
    let opening_char = opening.chars().next()?;
    let (close, role) = if opening_char == delimiters.substitution_open {
        (delimiters.substitution_close, DelimiterRole::Substitution)
    } else {
        (delimiters.control_close, DelimiterRole::Control)
    };
    let body_start = from + opening_char.len_utf8();
    let found = text.get(body_start..)?.find(close)?;
    let body_end = body_start + found;
    Some(Closed {
        end: body_end + close.len_utf8(),
        body: text.get(body_start..body_end)?,
        role,
    })
}

/// Which role a matched pair has, so a refused token can be rebuilt with the
/// exact delimiters the string used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DelimiterRole {
    /// A control, delimited by the control pair.
    Control,
    /// A substitution, delimited by the substitution pair.
    Substitution,
}

/// Validates one substitution body, either admitting it or leaving it literal.
fn parse_substitution(
    document: &mut MarkupDocument,
    literal: &mut String,
    delimiters: MarkupDelimiters,
    closed: &Closed<'_>,
    offset: usize,
) {
    let body = closed.body;
    if body.is_empty() || body.len() > MAX_MARKUP_TOKEN_LEN {
        keep_literal(document, literal, delimiters, closed);
        let kind = if body.is_empty() {
            MarkupIssueKind::EmptySubstitution { offset }
        } else {
            MarkupIssueKind::SubstitutionIdTooLong { len: body.len() }
        };
        document.issues.push(MarkupIssue::new(kind));
        return;
    }
    match validate_token(body) {
        Ok(id) => {
            flush(document, literal);
            document
                .tokens
                .push(MarkupToken::Substitution { id: id.to_owned() });
        }
        Err(_) => {
            // The id is present but outside the token grammar, so it is named
            // rather than reported as an empty substitution.
            keep_literal(document, literal, delimiters, closed);
            document
                .issues
                .push(MarkupIssue::new(MarkupIssueKind::BadSubstitutionId {
                    id: body.to_owned(),
                    offset,
                }));
        }
    }
}

/// Validates one control body, either admitting it or leaving it literal.
fn parse_control(
    document: &mut MarkupDocument,
    literal: &mut String,
    grammar: &MarkupGrammar,
    delimiters: MarkupDelimiters,
    closed: &Closed<'_>,
    offset: usize,
    open: &mut Vec<String>,
) {
    let body = closed.body;
    if let Some(name) = body.strip_prefix('/') {
        // A closing control: it must name the innermost open tag, otherwise the
        // string's own nesting is broken and we report it instead of balancing
        // it by guesswork.
        if name.is_empty() {
            keep_literal(document, literal, delimiters, closed);
            document
                .issues
                .push(MarkupIssue::new(MarkupIssueKind::EmptyTag { offset }));
            return;
        }
        if open.last().is_some_and(|expected| expected == name) {
            open.pop();
            flush(document, literal);
            // A closing control is a token too: a renderer has to be able to
            // pop the style the matching opening control pushed.
            document.tokens.push(MarkupToken::Control {
                name: name.to_owned(),
                argument: None,
                closing: true,
            });
        } else {
            keep_literal(document, literal, delimiters, closed);
            document
                .issues
                .push(MarkupIssue::new(MarkupIssueKind::UnbalancedControl {
                    name: name.to_owned(),
                    offset,
                }));
        }
        return;
    }

    let (name, argument) = match body.split_once('=') {
        Some((name, argument)) => (name, Some(argument)),
        None => (body, None),
    };
    if name.is_empty() {
        keep_literal(document, literal, delimiters, closed);
        document
            .issues
            .push(MarkupIssue::new(MarkupIssueKind::EmptyTag { offset }));
        return;
    }
    let Some(tag) = grammar.tag(name) else {
        keep_literal(document, literal, delimiters, closed);
        document
            .issues
            .push(MarkupIssue::new(MarkupIssueKind::UnknownTag {
                name: name.to_owned(),
                offset,
            }));
        return;
    };
    if argument.is_some() && !tag.takes_argument() {
        keep_literal(document, literal, delimiters, closed);
        document
            .issues
            .push(MarkupIssue::new(MarkupIssueKind::UnexpectedArgument {
                name: name.to_owned(),
                offset,
            }));
        return;
    }
    flush(document, literal);
    document.tokens.push(MarkupToken::Control {
        name: tag.name().to_owned(),
        argument: argument.map(str::to_owned),
        closing: false,
    });
    open.push(tag.name().to_owned());
}

/// Keeps a refused control or substitution in the literal text, delimiters
/// included, so the displayed string stays the resource string the caller
/// wrote and nothing is silently dropped.
fn keep_literal(
    document: &mut MarkupDocument,
    literal: &mut String,
    delimiters: MarkupDelimiters,
    closed: &Closed<'_>,
) {
    let (open, close) = match closed.role {
        DelimiterRole::Control => (delimiters.control_open, delimiters.control_close),
        DelimiterRole::Substitution => {
            (delimiters.substitution_open, delimiters.substitution_close)
        }
    };
    literal.push(open);
    literal.push_str(closed.body);
    literal.push(close);
    flush(document, literal);
}
// ------------------------------------------------------------------ font ---

/// The set of characters a font can render.
///
/// Coverage is **declared**, not measured: no font file is parsed at this
/// stage. What the set buys is the counting rule of F51 non-negotiable
/// behavior 3 — a character outside the set is reported by
/// [`GlyphCoverage::missing_in`] instead of being dropped, so a translation
/// that needs a glyph the font lacks is *visible* as a coverage gap.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GlyphCoverage {
    covered: BTreeSet<char>,
}

impl GlyphCoverage {
    /// The empty coverage: a font that can render nothing. Every character is
    /// then a missing glyph, which is the honest state for a font whose
    /// coverage has not been read yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds coverage from an explicit character set.
    #[must_use]
    pub fn from_chars(chars: impl IntoIterator<Item = char>) -> Self {
        Self {
            covered: chars.into_iter().collect(),
        }
    }

    /// Declares one more character as covered.
    pub fn insert(&mut self, ch: char) {
        self.covered.insert(ch);
    }

    /// Whether the font can render `ch`.
    #[must_use]
    pub fn covers(&self, ch: char) -> bool {
        self.covered.contains(&ch)
    }

    /// The characters of `text` the font cannot render, in first-appearance
    /// order, with how often each occurs.
    ///
    /// Whitespace is not a glyph and is never reported missing.
    #[must_use]
    pub fn missing_in(&self, text: &str) -> MissingGlyphReport {
        let mut order: Vec<char> = Vec::new();
        let mut counts: BTreeMap<char, usize> = BTreeMap::new();
        for ch in text.chars() {
            if ch.is_whitespace() || self.covers(ch) {
                continue;
            }
            let entry = counts.entry(ch).or_insert_with(|| {
                order.push(ch);
                0
            });
            *entry += 1;
        }
        MissingGlyphReport {
            missing: order,
            counts,
        }
    }

    /// How many distinct characters the font declares.
    #[must_use]
    pub fn len(&self) -> usize {
        self.covered.len()
    }

    /// Whether the font declares no character at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.covered.is_empty()
    }
}

/// The characters a font cannot render, counted rather than hidden.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MissingGlyphReport {
    missing: Vec<char>,
    counts: BTreeMap<char, usize>,
}

impl MissingGlyphReport {
    /// Whether every character was covered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.missing.is_empty()
    }

    /// The distinct missing characters, in first-appearance order.
    #[must_use]
    pub fn missing(&self) -> &[char] {
        &self.missing
    }

    /// How often `ch` occurs in the measured text, if it is missing.
    #[must_use]
    pub fn count(&self, ch: char) -> usize {
        self.counts.get(&ch).copied().unwrap_or(0)
    }

    /// How many character occurrences are missing in total.
    #[must_use]
    pub fn total(&self) -> usize {
        self.counts.values().sum()
    }

    /// Records `occurrences` further occurrences of an already-known missing
    /// character, appending it to the first-appearance order the first time.
    pub fn add(&mut self, ch: char, occurrences: usize) {
        if occurrences == 0 {
            return;
        }
        let entry = self.counts.entry(ch).or_insert_with(|| {
            self.missing.push(ch);
            0
        });
        *entry += occurrences;
    }
}

/// Whether a license's redistribution permission was verified.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LicensePermission {
    /// The permission was checked against a named license and its evidence.
    Verified {
        /// The license the permission was verified against.
        license: String,
    },
    /// The permission has not been verified, so the font cannot ship.
    Unverified {
        /// Why it is not verified.
        reason: String,
    },
}

impl LicensePermission {
    /// Whether this permission allows the font to be distributed.
    #[must_use]
    pub fn is_verified(&self) -> bool {
        matches!(self, Self::Verified { .. })
    }
}

/// A separately licensed font, with the permission that was actually verified.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontLicense {
    /// The license name.
    pub name: String,
    /// The permission state.
    pub permission: LicensePermission,
}

/// Where a font file would come from.
///
/// The two refused variants exist **so they can be refused**: F51
/// non-negotiable behavior 1 forbids bundling operating-system fonts and
/// proprietary game fonts, and a type that could not even name those sources
/// would leave the rule untested.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FontSource {
    /// A font inside the owner's original installation, loaded privately and
    /// never redistributed.
    OriginalInstallation {
        /// Where the font's bytes live in the installation.
        span: Box<SourceSpan>,
    },
    /// A separately licensed font shipped with the release.
    LicensedFallback {
        /// The license and its verified permission.
        license: FontLicense,
    },
    /// An operating-system font. **Never acceptable**: it is not ours to ship
    /// and its metrics differ per machine.
    OperatingSystem {
        /// The family name the requester asked for.
        family: String,
    },
    /// A proprietary font from the original game. **Never acceptable to
    /// redistribute**: it may be loaded privately from the owner's own
    /// installation, never committed or shipped.
    ProprietaryGame {
        /// The family name the requester asked for.
        family: String,
    },
}

/// The provenance a [`FontFace`] may carry: only two answers are shippable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FontProvenance {
    /// An original font read from the owner's installation. It stays private:
    /// the release never redistributes it, and the span is what makes the read
    /// auditable.
    OriginalPrivate {
        /// Where the font's bytes live.
        span: Box<SourceSpan>,
    },
    /// A separately licensed fallback font, distributed only with verified
    /// permission.
    LicensedFallback {
        /// The license and its verified permission.
        license: FontLicense,
    },
}

impl FontProvenance {
    /// Whether the font may be distributed with the release. An original font
    /// never may; a licensed fallback does when its permission is verified.
    #[must_use]
    pub fn is_distributable(&self) -> bool {
        match self {
            Self::OriginalPrivate { .. } => false,
            Self::LicensedFallback { license } => license.permission.is_verified(),
        }
    }
}

/// Why a [`FontFace`] was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FontRefusal {
    /// An operating-system font was requested. F51 non-negotiable behavior 1:
    /// operating-system fonts are never bundled.
    OperatingSystem {
        /// The family that was asked for.
        family: String,
    },
    /// A proprietary game font was requested for redistribution. The original
    /// font may be loaded privately from the owner's installation, never
    /// shipped.
    ProprietaryGame {
        /// The family that was asked for.
        family: String,
    },
    /// A licensed fallback's permission was never verified, so it cannot ship.
    UnverifiedLicense {
        /// The license name.
        name: String,
        /// Why the permission is not verified.
        reason: String,
    },
}

impl fmt::Display for FontRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OperatingSystem { family } => write!(
                f,
                "operating-system font {family:?} cannot be bundled; use an original or a licensed fallback"
            ),
            Self::ProprietaryGame { family } => write!(
                f,
                "proprietary game font {family:?} cannot be redistributed; load it from the owner's installation"
            ),
            Self::UnverifiedLicense { name, reason } => write!(
                f,
                "fallback font license {name:?} is not verified: {reason}"
            ),
        }
    }
}

impl std::error::Error for FontRefusal {}

/// The raw parts of a [`FontFace`], so the validating constructor takes one
/// record instead of a long positional argument list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontFaceDraft {
    /// The `font` content id.
    pub id: ContentId,
    /// The family name the content declares.
    pub family: String,
    /// Where the font file would come from.
    pub source: FontSource,
    /// The declared glyph coverage, or the empty set while it is unmeasured.
    pub coverage: GlyphCoverage,
    /// Where the record itself came from.
    pub origin: Origin,
    /// The provenance of the record.
    pub provenance: Provenance,
}

/// One font the content declares: an id, a family, a provenance and a declared
/// glyph coverage.
///
/// A `FontFace` cannot exist for an operating-system or a proprietary
/// redistributed font: [`FontFace::try_new`] turns those sources into a
/// [`FontRefusal`] instead of quietly accepting them (F51 non-negotiable
/// behavior 1). The provenance is a closed enum with only the two acceptable
/// answers, so no consumer can branch around the rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontFace {
    id: ContentId,
    family: String,
    provenance: FontProvenance,
    coverage: GlyphCoverage,
    origin: Origin,
    provenance_record: Provenance,
}

impl FontFace {
    /// Validates and assembles a declared font.
    ///
    /// # Errors
    ///
    /// [`FontRefusal::OperatingSystem`] or [`FontRefusal::ProprietaryGame`] for
    /// a source that must never be bundled, and
    /// [`FontRefusal::UnverifiedLicense`] for a licensed fallback whose
    /// permission was not verified.
    pub fn try_new(draft: FontFaceDraft) -> Result<Self, FontRefusal> {
        let FontFaceDraft {
            id,
            family,
            source,
            coverage,
            origin,
            provenance,
        } = draft;
        let provenance_value = match source {
            FontSource::OriginalInstallation { span } => FontProvenance::OriginalPrivate { span },
            FontSource::LicensedFallback { license } => {
                if !license.permission.is_verified() {
                    let name = license.name;
                    let reason = match license.permission {
                        LicensePermission::Unverified { reason } => reason,
                        LicensePermission::Verified { .. } => {
                            unreachable!("an unverified permission was just rejected")
                        }
                    };
                    return Err(FontRefusal::UnverifiedLicense { name, reason });
                }
                FontProvenance::LicensedFallback { license }
            }
            FontSource::OperatingSystem { family } => {
                return Err(FontRefusal::OperatingSystem { family });
            }
            FontSource::ProprietaryGame { family } => {
                return Err(FontRefusal::ProprietaryGame { family });
            }
        };
        Ok(Self {
            id,
            family,
            provenance: provenance_value,
            coverage,
            origin,
            provenance_record: provenance,
        })
    }

    /// The font's content id.
    #[must_use]
    pub fn id(&self) -> &ContentId {
        &self.id
    }

    /// The declared family name.
    #[must_use]
    pub fn family(&self) -> &str {
        &self.family
    }

    /// Where the font came from: the two acceptable provenances only.
    #[must_use]
    pub fn provenance(&self) -> &FontProvenance {
        &self.provenance
    }

    /// The declared glyph coverage.
    #[must_use]
    pub fn coverage(&self) -> &GlyphCoverage {
        &self.coverage
    }

    /// Where the record came from.
    #[must_use]
    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The provenance of the record itself.
    #[must_use]
    pub fn record_provenance(&self) -> &Provenance {
        &self.provenance_record
    }
}

/// Why a [`FontCatalog`] refused a font.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FontCatalogError {
    /// Two fonts share one id.
    DuplicateId {
        /// The duplicated id.
        id: ContentId,
    },
}

/// The declared fonts, keyed by stable id.
#[derive(Clone, Debug, Default)]
pub struct FontCatalog {
    faces: BTreeMap<ContentId, FontFace>,
}

impl FontCatalog {
    /// An empty catalog.
    #[must_use]
    pub fn new() -> Self {
        Self {
            faces: BTreeMap::new(),
        }
    }

    /// Inserts a font, refusing an id that is already present.
    ///
    /// # Errors
    ///
    /// [`FontCatalogError::DuplicateId`].
    pub fn insert(&mut self, face: FontFace) -> Result<(), FontCatalogError> {
        let id = face.id().clone();
        if self.faces.contains_key(&id) {
            return Err(FontCatalogError::DuplicateId { id });
        }
        self.faces.insert(id, face);
        Ok(())
    }

    /// The font with `id`, if the catalog holds one.
    #[must_use]
    pub fn get(&self, id: &ContentId) -> Option<&FontFace> {
        self.faces.get(id)
    }

    /// How many fonts the catalog holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.faces.len()
    }

    /// Whether the catalog holds no fonts.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }

    /// The fonts, in id order.
    pub fn faces(&self) -> impl Iterator<Item = &FontFace> {
        self.faces.values()
    }

    /// The fonts that may be distributed with the release.
    ///
    /// An original private font is excluded here, so a packaging step that asks
    /// this question cannot ship one by accident.
    pub fn distributable(&self) -> impl Iterator<Item = &FontFace> {
        self.faces
            .values()
            .filter(|face| face.provenance().is_distributable())
    }

    /// Every character that **no** declared font can render, counted rather
    /// than hidden.
    ///
    /// A text may legitimately be drawn with any of the catalog's fonts, so a
    /// character is reported only when *every* face lacks it. The result is the
    /// union over `texts` of the per-character occurrence counts, which is what
    /// F51's AC02 audit needs: the number of character occurrences a locale
    /// would lose.
    #[must_use]
    pub fn missing_glyphs(&self, texts: &[String]) -> MissingGlyphReport {
        let mut report = MissingGlyphReport::default();
        for text in texts {
            let mut seen: BTreeSet<char> = BTreeSet::new();
            for ch in text.chars() {
                if ch.is_whitespace() || !seen.insert(ch) {
                    continue;
                }
                if self.faces.values().any(|face| face.coverage().covers(ch)) {
                    continue;
                }
                let occurrences = text.chars().filter(|other| *other == ch).count();
                report.add(ch, occurrences);
            }
        }
        report
    }
}

// ------------------------------------------------------- locale setting ---

/// The profile setting key under which the selected UI locale is persisted.
///
/// A **designed** engine key, not an original measurement: the original
/// release's settings layout is F48/F52's, and nothing here claims to know it.
/// The key is deliberately separate from every campaign, record and unlock
/// field, because changing locale must not be able to change save ids, mission
/// identity, numeric parsing or a protocol value (F51 non-negotiable behavior 5).
pub const LOCALE_SETTING_KEY: &str = "ui.locale";

/// Why a locale setting could not be declared, read or written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocaleSettingError {
    /// No supported locale label was declared, so no value could ever be
    /// accepted.
    NoLabels,
    /// The declared supported-locale set named one label twice, so the value
    /// space would be ambiguous.
    DuplicateLabel {
        /// The repeated label.
        label: String,
    },
    /// A stored settings list named the locale key more than once, so the value
    /// in force would depend on iteration order.
    DuplicateEntry,
    /// A declared supported label is not a canonical [`LocaleId`], so a value
    /// the rule accepts could not be read back as a locale (or written as one).
    /// The value space and the reader must agree, or the feature's own default
    /// could be unusable.
    InvalidLabel {
        /// The rejected label, as declared.
        label: String,
        /// Why it is not a usable locale label.
        reason: String,
    },
    /// The stored value is not a valid [`LocaleId`] label.
    MalformedStoredValue {
        /// The stored value, as written.
        value: String,
    },
}

impl fmt::Display for LocaleSettingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoLabels => {
                f.write_str("a locale setting must declare at least one supported label")
            }
            Self::DuplicateLabel { label } => {
                write!(f, "supported locale {label:?} is declared more than once")
            }
            Self::DuplicateEntry => {
                write!(
                    f,
                    "the locale setting key is stored more than once in the profile"
                )
            }
            Self::InvalidLabel { label, reason } => {
                write!(f, "supported locale {label:?} is not usable: {reason}")
            }
            Self::MalformedStoredValue { value } => {
                write!(f, "stored locale {value:?} is not a valid locale label")
            }
        }
    }
}

impl std::error::Error for LocaleSettingError {}

/// Why a declared label is not a canonical [`LocaleId`] string, if it is not.
///
/// [`LocaleId::new`] trims its input, so a label with surrounding whitespace
/// parses but does not spell the same value the reader returns. A rule that
/// declared it would accept a value the writer could never produce, so the label
/// is refused rather than silently normalized.
fn canonical_locale_label(label: &str) -> Result<(), String> {
    match LocaleId::new(label) {
        Ok(parsed) if parsed.as_str() == label => Ok(()),
        Ok(parsed) => Err(format!(
            "it parses as {:?}, not the declared spelling",
            parsed.as_str()
        )),
        Err(LocaleIdError::Empty) => Err("it is empty".to_owned()),
        Err(LocaleIdError::TooLong { len }) => Err(format!(
            "it is {len} bytes, over the {MAX_LOCALE_ID_LEN}-byte bound"
        )),
        Err(LocaleIdError::BadCharacter { ch }) => Err(format!("it contains the character {ch:?}")),
    }
}

/// The localization feature's own setting: the selected locale, persisted as one
/// profile setting and nothing else.
///
/// F48-C's rule is that *the feature that owns a setting owns its rule*; this is
/// the localization feature's. It is a thin, typed bridge over one
/// [`SettingEntry`] so a locale choice survives a save round-trip through the
/// ordinary settings list, and so nothing about it can reach the campaign,
/// record or unlock fields of a [`ProfileDocument`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LocaleSetting;

impl LocaleSetting {
    /// The [`SettingRule`] for the locale key over the caller's declared
    /// supported labels.
    ///
    /// The supported-locale list is the caller's because the original locale
    /// list is unmeasured (F51-A recorded that). The value is a
    /// [`ValueRule::Choice`] over exactly those labels, the change is
    /// [`SettingApply::Live`] — a locale change takes effect on the next layout,
    /// not after a restart — and the default is the first declared label.
    ///
    /// Every label is required to be a canonical [`LocaleId`]: the value space a
    /// rule declares and the values [`LocaleSetting::read`] and
    /// [`LocaleSetting::write`] carry must be the same set, or the rule's own
    /// default could be a stored value the reader refuses and no
    /// [`LocaleChain`](LocaleChain) could be built from it.
    ///
    /// # Errors
    ///
    /// [`LocaleSettingError::NoLabels`] for an empty declaration,
    /// [`LocaleSettingError::DuplicateLabel`] for a repeated label and
    /// [`LocaleSettingError::InvalidLabel`] for a label that is not a canonical
    /// [`LocaleId`].
    pub fn rule(labels: &'static [&'static str]) -> Result<SettingRule, LocaleSettingError> {
        let Some((default, _)) = labels.split_first() else {
            return Err(LocaleSettingError::NoLabels);
        };
        let mut seen = BTreeSet::new();
        for label in labels {
            if !seen.insert(*label) {
                return Err(LocaleSettingError::DuplicateLabel {
                    label: (*label).to_owned(),
                });
            }
            if let Err(reason) = canonical_locale_label(label) {
                return Err(LocaleSettingError::InvalidLabel {
                    label: (*label).to_owned(),
                    reason,
                });
            }
        }
        Ok(SettingRule {
            key: LOCALE_SETTING_KEY,
            apply: SettingApply::Live,
            value: ValueRule::Choice(labels),
            default,
        })
    }

    /// Reads the persisted locale from a profile document's settings, if any.
    ///
    /// # Errors
    ///
    /// [`LocaleSettingError::DuplicateEntry`] when the key is stored twice, and
    /// [`LocaleSettingError::MalformedStoredValue`] when the stored label is not
    /// a valid [`LocaleId`].
    pub fn read(document: &ProfileDocument) -> Result<Option<LocaleId>, LocaleSettingError> {
        let mut found: Option<&SettingEntry> = None;
        for entry in &document.settings {
            if entry.key == LOCALE_SETTING_KEY {
                if found.is_some() {
                    return Err(LocaleSettingError::DuplicateEntry);
                }
                found = Some(entry);
            }
        }
        match found {
            None => Ok(None),
            Some(entry) => LocaleId::new(&entry.value).map(Some).map_err(|_| {
                LocaleSettingError::MalformedStoredValue {
                    value: entry.value.clone(),
                }
            }),
        }
    }

    /// Writes the locale into a profile document's settings.
    ///
    /// Only the locale entry is touched: the key is replaced in place when it is
    /// present, appended when it is not, and every other field of the document —
    /// campaign, blueprints, records, other settings and unknown fields — is left
    /// exactly as it was. That is what makes a locale change unable to affect an
    /// unlock or a save identity, and it is invariantly true rather than a rule a
    /// caller has to remember.
    ///
    /// # Errors
    ///
    /// [`LocaleSettingError::DuplicateEntry`] when the key is stored twice: the
    /// contradiction is refused rather than resolved by overwriting one copy, so
    /// a damaged save is not silently rewritten.
    pub fn write(
        document: &mut ProfileDocument,
        locale: &LocaleId,
    ) -> Result<(), LocaleSettingError> {
        let existing = document
            .settings
            .iter()
            .filter(|entry| entry.key == LOCALE_SETTING_KEY)
            .count();
        match existing {
            0 => document.settings.push(SettingEntry {
                key: LOCALE_SETTING_KEY.to_owned(),
                apply: SettingApply::Live,
                value: locale.as_str().to_owned(),
            }),
            1 => {
                let entry = document
                    .settings
                    .iter_mut()
                    .find(|entry| entry.key == LOCALE_SETTING_KEY)
                    .expect("exactly one locale entry was counted");
                entry.apply = SettingApply::Live;
                entry.value = locale.as_str().to_owned();
            }
            _ => return Err(LocaleSettingError::DuplicateEntry),
        }
        Ok(())
    }
}

// --------------------------------------------------------------- fixtures --

/// The claim every synthetic fixture value in this module is recorded under.
pub const SYNTHETIC_CLAIM: &str = "f51a.localization";

/// The id of the deliberately over-long synthetic translation the layout
/// scenario wraps.
///
/// It is long enough that no plausible panel fits it, which is what makes
/// "fits or scrolls" observable in the acceptance tests.
pub const SYNTHETIC_LONG_TRANSLATION_KEY: &str = "mission.briefing.long";

/// A synthetic fixture catalog: two locales, one deliberately long string, one
/// string only the selected locale has, and one string nobody translated.
///
/// Every row is `Origin::SyntheticFixture` with designed provenance, so this
/// can never support an original-data claim.
#[must_use]
pub fn declared_synthetic_text_catalog() -> TextCatalog {
    let claim = || {
        cs_types::evidence::ClaimId::new(SYNTHETIC_CLAIM).expect("the synthetic claim id is valid")
    };
    let long_en = "Attention flight crew. The weather front over the shipping lanes has moved \
                   north since the last briefing, so the assigned corridor now crosses the edge of \
                   the storm cell. Expect reduced visibility, intermittent rain and a longer \
                   crossing than planned. Acknowledge this briefing and confirm your assigned \
                   corridor before you take the runway. The escort flight will hold at the relay \
                   point until the corridor is acknowledged, and the tanker group behind you will \
                   close up to the same spacing, so any deviation from the assigned corridor adds \
                   fuel to every aircraft behind you. Navigation lights are to be on, the escort \
                   frequency is to be monitored continuously, and no aircraft is to break formation \
                   inside the cell boundary regardless of the weather. If the crossing cannot be \
                   completed inside the fuel margin, return to the relay point and request a new \
                   corridor from the control ship; do not improvise a route through the rain band \
                   on the northern flank, because the cell edge moves with the wind and the \
                   surveyed marks inside it are two years old. Acknowledge this briefing.";
    let long_de = "Achtung Besatzung. Die Wetterfront über den Schifffahrtsrouten ist seit dem \
                   letzten Briefing nach Norden gewandert, der zugewiesene Korridor kreuzt daher \
                   den Rand der Sturmzelle. Geringere Sicht, zeitweiliger Regen und eine längere \
                   Überfahrt als geplant sind zu erwarten. Bestätigen Sie dieses Briefing und \
                   geben Sie Ihren zugewiesenen Korridor an, bevor Sie die Startbahn nehmen. Das \
                   Begleitflugzeug hält am Relaispunkt, bis der Korridor bestätigt ist, und die \
                   Tankergruppe hinter Ihnen schließt auf denselben Abstand auf, sodass jede \
                   Abweichung vom zugewiesenen Korridor den Brennstoff für alle hinter Ihnen \
                   vergrößert. Positionslichter sind einzuschalten, der Begleitfunk ist \
                   ununterbrochen zu überwachen, und keine Maschine löst sich innerhalb der \
                   Zellgrenze aus dem Verband, unabhängig vom Wetter. Lässt sich die Überfahrt \
                   innerhalb des Treibstoffrahmens nicht abschließen, kehren Sie zum Relaispunkt \
                   zurück und fordern Sie einen neuen Korridor vom Führungsschiff an; improvisieren \
                   Sie keinen Weg durch das Regenband am nördlichen Flanke, denn die Zellgrenze \
                   wandert mit dem Wind, und die vermessenen Marken darin sind zwei Jahre alt. \
                   Der Anflug auf die Startbahn erfolgt gegen den Wind, und die Sichtweite am \
                   Bodensegment kann unter dem Regen geringer ausfallen als am Kontrollschiff \
                   gemeldet; halten Sie die Anfluggeschwindigkeit nach Sicht, nicht nach Uhr. \
                   Meldet das Begleitflugzeug eine Triebwerksstörung, brechen Sie den Anflug ab und \
                   bleiben Sie in Holding, bis die Bodenkennung erneut freigegeben ist. Eine \
                   Freigabe durch den Lotsen ohne bestätigten Korridor wird nicht angenommen, und \
                   eine Verspätung gegenüber der im Fahrplan genannten Zeit wird an die Leitstelle \
                   gemeldet, bevor die Startbahn freigegeben wird. Bestätigen Sie dieses Briefing.";
    let long_fr = "Equipage, attention. Le front meteorologique au-dessus des couloirs maritimes a \
                   remonte vers le nord depuis le dernier briefing; le corridor assigne croise donc \
                   le bord de la cellule de tempete. Attendez-vous a une visibilite reduite, a des \
                   pluies intermittentes et a une traversée plus longue que prevu. Confirmez ce \
                   briefing et annoncez le corridor qui vous est assigne avant de prendre la piste. \
                   L'avion d'escorte se/maintiendra au point relais jusqu'a confirmation du \
                   corridor, et le groupe de ravitaillement derriere vous reduira au meme espacement, \
                   de sorte que tout ecart au corridor assigne augmente le carburant de tous les \
                   avions qui suivent. Les feux de position doivent etre allumes, la frequence \
                   d'escorte doit etre suivie en permanence, et aucun appareil ne doit rompre la \
                   formation a l'interieur de la limite de la cellule, quel que soit le temps. Si \
                   la traversee ne peut pas etre achevee dans la marge de carburant, revenez au \
                   point relais et demandez un nouveau corridor au navire de commandement; \
                   n'improvisez pas un itineraire dans la bande de pluie du flanc nord, car la \
                   limite de la cellule avance avec le vent et les reperes releves a l'interieur \
                   ont deux ans. L'approche se fait face au vent, et la visibilite au niveau du \
                   sol peut etre plus mauvaise que celle annoncee par le navire de commandement; \
                   tenez votre vitesse d'approche a la vue, pas a l'horloge. Si l'avion d'escorte \
                   signale une panne de moteur, interrompez l'approche et restez en attente jusqu'a \
                   une nouvelle liberation de l'identifiant au sol; une clearance de l'aerodrome \
                   sans corridor confirme n'est pas acceptee, et tout retard est signale au poste de \
                   commandement avant que la piste ne soit liberee. Confirmez ce briefing.";
    let mut catalog = TextCatalog::new();
    let row = |key: &str, locale: &str, text: &str| {
        LocalizedText::new(
            TextId::new(key).expect("the fixture key is a valid string id"),
            LocaleId::new(locale).expect("the fixture locale is a valid label"),
            text,
            Origin::SyntheticFixture,
            Provenance::designed(claim()),
        )
    };
    // The long briefing exists in all three fixture locales, at three
    // different lengths: a layout that only handles the shortest text will
    // pass on the English row and fail on the others.
    catalog
        .insert(row(SYNTHETIC_LONG_TRANSLATION_KEY, "en-us", long_en))
        .expect("the english row is inserted once");
    catalog
        .insert(row(SYNTHETIC_LONG_TRANSLATION_KEY, "de-de", long_de))
        .expect("the german row is inserted once");
    catalog
        .insert(row(SYNTHETIC_LONG_TRANSLATION_KEY, "fr-fr", long_fr))
        .expect("the french row is inserted once");
    catalog
        .insert(row(
            "mission.briefing.confirm",
            "en-us",
            "Confirm your corridor",
        ))
        .expect("the confirm row is inserted once");
    // `hud.untranslated` exists only in `en-us`, so resolving it through
    // `de-de -> en-us` exercises a real fallback while resolving it through
    // `de-de` alone is a genuine miss.
    catalog
        .insert(row("hud.untranslated", "en-us", "Target lost"))
        .expect("the untranslated row is inserted once");
    catalog
}

/// The synthetic markup grammar: the declared `[control]` / `{substitution}`
/// spelling with a `color` tag that takes an argument, a `bold` tag that does
/// not, and nothing else.
#[must_use]
pub fn synthetic_markup_grammar() -> MarkupGrammar {
    MarkupGrammar::new(
        MarkupDelimiters::DECLARED,
        [
            ControlTag::with_argument("color").expect("the color tag name is valid"),
            ControlTag::simple("bold").expect("the bold tag name is valid"),
        ],
    )
    .expect("the declared grammar is valid")
}

/// A synthetic font face: a licensed fallback with a verified permission and
/// declared ASCII coverage, so a translation containing a character outside
/// ASCII produces a real coverage gap.
#[must_use]
pub fn synthetic_font_face() -> FontFace {
    let claim =
        cs_types::evidence::ClaimId::new(SYNTHETIC_CLAIM).expect("the synthetic claim id is valid");
    let coverage = GlyphCoverage::from_chars((0x20u8..0x7Fu8).map(char::from));
    FontFace::try_new(FontFaceDraft {
        id: ContentId::from_source(ContentKind::Font, "synthetic.ui")
            .expect("the font id is valid"),
        family: "Synthetic UI".to_owned(),
        source: FontSource::LicensedFallback {
            license: FontLicense {
                name: "Synthetic UI License".to_owned(),
                permission: LicensePermission::Verified {
                    license: "synthetic-ui-1.0".to_owned(),
                },
            },
        },
        coverage,
        origin: Origin::SyntheticFixture,
        provenance: Provenance::designed(claim),
    })
    .expect("a verified licensed fallback is admitted")
}

// ------------------------------------------------ supported locales (F51-D) ---

/// Maximum number of locales one [`SupportedLocales`] set may declare.
///
/// A designed bound, not an original measurement: the original release's
/// supported-locale list is unmeasured (F51-A recorded that), so a set larger
/// than this is a reading or authoring error and is refused rather than
/// silently truncated.
pub const MAX_SUPPORTED_LOCALES: usize = 64;

/// Why a [`SupportedLocales`] set was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SupportedLocalesError {
    /// The set declared no locale at all, so no locale could be audited.
    Empty,
    /// The set held more than [`MAX_SUPPORTED_LOCALES`] locales.
    TooLong {
        /// How many locales were supplied.
        len: usize,
    },
    /// One locale was declared twice, so the set would depend on iteration
    /// order and the same locale could be audited twice.
    Duplicate {
        /// The repeated locale.
        locale: LocaleId,
    },
}

impl fmt::Display for SupportedLocalesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("a supported-locale set must declare at least one locale"),
            Self::TooLong { len } => write!(
                f,
                "supported-locale set holds {len} locales, max is {MAX_SUPPORTED_LOCALES}"
            ),
            Self::Duplicate { locale } => {
                write!(f, "locale {locale} is declared supported more than once")
            }
        }
    }
}

impl std::error::Error for SupportedLocalesError {}

/// The declared supported-locale set an F51-D audit walks, in declaration order.
///
/// The set is **caller-declared** for the same reason [`LanguageMap`] is: the
/// original release's supported-locale list is unmeasured, so a built-in list
/// would be a fabricated compatibility claim. Order is meaningful — it is the
/// fallback order an audit walks — so it is preserved rather than sorted, and a
/// locale declared twice or an empty set is refused instead of silently
/// deduplicated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SupportedLocales {
    locales: Vec<LocaleId>,
}

impl SupportedLocales {
    /// Validates and assembles the declared set, preserving declaration order.
    ///
    /// # Errors
    ///
    /// [`SupportedLocalesError::Empty`], [`SupportedLocalesError::TooLong`] or
    /// [`SupportedLocalesError::Duplicate`].
    pub fn new(locales: impl IntoIterator<Item = LocaleId>) -> Result<Self, SupportedLocalesError> {
        let locales: Vec<LocaleId> = locales.into_iter().collect();
        if locales.is_empty() {
            return Err(SupportedLocalesError::Empty);
        }
        if locales.len() > MAX_SUPPORTED_LOCALES {
            return Err(SupportedLocalesError::TooLong { len: locales.len() });
        }
        let mut seen = BTreeSet::new();
        for locale in &locales {
            if !seen.insert(locale.clone()) {
                return Err(SupportedLocalesError::Duplicate {
                    locale: locale.clone(),
                });
            }
        }
        Ok(Self { locales })
    }

    /// The declared locales, in declaration order.
    #[must_use]
    pub fn locales(&self) -> &[LocaleId] {
        &self.locales
    }

    /// How many locales the set declares.
    #[must_use]
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.locales.len()
    }

    /// Whether the set declares no locale. Always `false` for a set built by
    /// [`SupportedLocales::new`], which refuses the empty set.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.locales.is_empty()
    }

    /// Whether `locale` is one of the declared locales.
    #[must_use]
    pub fn contains(&self, locale: &LocaleId) -> bool {
        self.locales.iter().any(|entry| entry == locale)
    }
}

/// One declared locale's coverage of a catalog, audited against a chain that
/// starts with that locale and then falls back to the other declared locales.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocaleCoverage {
    /// The locale this record audits.
    pub locale: LocaleId,
    /// The chain that was walked, `locale` first.
    pub chain: Vec<LocaleId>,
    /// Distinct ids in the catalog — the coverage denominator.
    pub ids: usize,
    /// Ids the locale itself answered.
    pub translated: usize,
    /// Ids another declared locale answered, i.e. this locale is untranslated
    /// there.
    pub via_fallback: usize,
    /// Ids no declared locale in the chain answered.
    pub missing: Vec<TextId>,
}

/// The result of auditing every declared locale against one catalog (F51-D).
///
/// This is the machine-readable shape of "audit all strings for each declared
/// supported original locale": one [`LocaleCoverage`] per declared locale, the
/// catalog's distinct-id denominator, the locales the catalog holds rows for but
/// nobody declared, and the ids no declared locale answers at all.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MultiLocaleAudit {
    /// One record per declared locale, in declaration order.
    pub locales: Vec<LocaleCoverage>,
    /// Distinct ids in the catalog.
    pub ids: usize,
    /// Locales the catalog holds rows for that the declared set does not name,
    /// in sorted order.
    pub undeclared: Vec<LocaleId>,
    /// Ids no declared locale has a row for, in id order.
    pub missing_everywhere: Vec<TextId>,
}

impl MultiLocaleAudit {
    /// Whether every declared locale was audited and every id is answered by
    /// some declared locale, with no undeclared locale present.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.undeclared.is_empty() && self.missing_everywhere.is_empty()
    }

    /// The coverage record for `locale`, if it was declared.
    #[must_use]
    pub fn coverage_for(&self, locale: &LocaleId) -> Option<&LocaleCoverage> {
        self.locales
            .iter()
            .find(|coverage| &coverage.locale == locale)
    }

    /// The total number of id/locale pairs the declared locales answer
    /// themselves, i.e. the sum of every [`LocaleCoverage::translated`].
    #[must_use]
    pub fn translated_total(&self) -> usize {
        self.locales
            .iter()
            .map(|coverage| coverage.translated)
            .sum()
    }
}

impl TextCatalog {
    /// Audits every declared locale against this catalog.
    ///
    /// Each declared locale is audited against a chain of that locale followed
    /// by the other declared locales in declaration order (bounded to
    /// [`MAX_LOCALE_CHAIN_LEN`] entries), so `translated` is how much of the
    /// catalog this locale answers *itself* and `via_fallback` is how much its
    /// chain has to borrow from another declared locale. The denominator is the
    /// catalog's own distinct ids, so a string translated into three locales
    /// counts once. An id no declared locale answers is
    /// [`MultiLocaleAudit::missing_everywhere`]; a locale the catalog holds
    /// rows for but nobody declared is [`MultiLocaleAudit::undeclared`] — both
    /// are reported, never dropped.
    #[must_use]
    pub fn audit_locales(&self, supported: &SupportedLocales) -> MultiLocaleAudit {
        let ids = self.ids();
        let undeclared: Vec<LocaleId> = self
            .locales()
            .into_iter()
            .filter(|locale| !supported.contains(locale))
            .collect();

        let mut locales = Vec::with_capacity(supported.len());
        for locale in supported.locales() {
            // The chain starts at the audited locale and falls back to the
            // other declared locales in declaration order, bounded by the
            // chain limit. A locale can never appear twice, so the chain is
            // always valid.
            let fallbacks: Vec<LocaleId> = supported
                .locales()
                .iter()
                .filter(|other| *other != locale)
                .take(MAX_LOCALE_CHAIN_LEN - 1)
                .cloned()
                .collect();
            let chain = LocaleChain::new(locale.clone(), fallbacks)
                .expect("a nonempty bounded unique chain is valid");
            let audit = self.audit(&chain);
            locales.push(LocaleCoverage {
                locale: locale.clone(),
                chain: chain.locales().to_vec(),
                ids: audit.ids,
                translated: audit.resolved.saturating_sub(audit.served_by_fallback),
                via_fallback: audit.served_by_fallback,
                missing: audit.missing,
            });
        }

        let mut covered: BTreeSet<TextId> = BTreeSet::new();
        for locale in supported.locales() {
            for id in &ids {
                if self.get(id, locale).is_some() {
                    covered.insert(id.clone());
                }
            }
        }
        let missing_everywhere: Vec<TextId> = ids
            .iter()
            .filter(|id| !covered.contains(*id))
            .cloned()
            .collect();

        MultiLocaleAudit {
            locales,
            ids: ids.len(),
            undeclared,
            missing_everywhere,
        }
    }
}

// ------------------------------------------ measured locale set (F51-LOCALE-SET) ---

/// The [`LocaleId`] label a measured resource language id is declared under.
///
/// The label is `resource-<id>`, spelled from the measurement itself. It is
/// deliberately **not** a language name: the original release's own naming of
/// its locales is unmeasured, and mapping a Win32 language id to a conventional
/// name (`1033` to "en-US") would import an assumption about the 2000 release
/// that no measurement supports. Spelling the id keeps a locale label
/// truthful — it names what the files said — while staying a valid
/// [`LocaleId`] (ASCII alphanumerics and `-`).
#[must_use]
pub fn measured_locale_label(language: u32) -> String {
    format!("resource-{language}")
}

/// One measured occurrence of a resource language id.
///
/// An occurrence is *where the id was seen and how much came with it*: the
/// [`SourceSpan`] is the evidence (it carries the installation digest and the
/// container spelling), and `rows` is how many F12 string rows of that language
/// the container held. Two images of one installation usually each contribute
/// one occurrence of the same id, which is why occurrences are kept rather than
/// collapsed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanguageObservation {
    /// Where the language id was measured.
    pub source: SourceSpan,
    /// The third-level PE resource language id, verbatim (F12 § resource tree).
    pub language: u32,
    /// How many string rows of that language the container held.
    pub rows: usize,
}

/// The resource language table measured from an installation's string images.
///
/// This is the measured answer to "which languages do the original's own files
/// carry?", and it is what a [`SupportedLocales`] declaration is *derived* from
/// by [`MeasuredLocales::from_table`]. It is not, by itself, a statement about
/// which locales the release was localized into: one installation carries the
/// languages *it* was built with, and the release-wide set needs a second,
/// localized installation to be measured.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResourceLanguageTable {
    observations: Vec<LanguageObservation>,
}

impl ResourceLanguageTable {
    /// An empty table, before anything is measured.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one occurrence of `language`, with the span that proves it.
    ///
    /// A zero-row occurrence is kept: it says "this container declared the
    /// language and carried no strings in it", which is a measurement a
    /// coverage audit needs and a drop would hide.
    pub fn observe(&mut self, source: SourceSpan, language: u32, rows: usize) {
        self.observations.push(LanguageObservation {
            source,
            language,
            rows,
        });
    }

    /// Every occurrence, in the order it was measured.
    #[must_use]
    pub fn observations(&self) -> &[LanguageObservation] {
        &self.observations
    }

    /// The distinct language ids measured, in ascending order.
    #[must_use]
    pub fn languages(&self) -> Vec<u32> {
        let languages: BTreeSet<u32> = self
            .observations
            .iter()
            .map(|entry| entry.language)
            .collect();
        languages.into_iter().collect()
    }

    /// The summed rows of every occurrence of `language`, across containers.
    #[must_use]
    pub fn rows_for(&self, language: u32) -> usize {
        self.observations
            .iter()
            .filter(|entry| entry.language == language)
            .map(|entry| entry.rows)
            .sum()
    }

    /// How many containers contributed an occurrence.
    #[must_use]
    #[allow(clippy::len_without_is_empty)]
    pub fn containers(&self) -> usize {
        self.observations.len()
    }

    /// Whether nothing was measured, so no locale can be declared from it.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.observations.is_empty()
    }
}

/// Why a measured locale declaration was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeasuredLocalesError {
    /// The table held no occurrence at all, so a declaration would be a guess.
    NothingMeasured,
    /// The table held more distinct language ids than one language map may
    /// declare ([`MAX_LANGUAGE_MAP_LEN`]).
    TooManyLanguages {
        /// How many distinct language ids were measured.
        len: usize,
    },
}

impl fmt::Display for MeasuredLocalesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NothingMeasured => {
                f.write_str("no resource language was measured, so no locale can be declared")
            }
            Self::TooManyLanguages { len } => write!(
                f,
                "measured {len} resource languages, max is {MAX_LANGUAGE_MAP_LEN}"
            ),
        }
    }
}

impl std::error::Error for MeasuredLocalesError {}

/// A supported-locale declaration **derived from measurement**.
///
/// F51-A made the supported-locale set and the language map caller-declared,
/// because the original release's list was unmeasured. This type is what
/// replaces that caller choice whenever the original files themselves are
/// available: the caller hands in the [`ResourceLanguageTable`] measured from
/// the installation's string images, and the [`SupportedLocales`] set plus the
/// [`LanguageMap`] are built from the ids that were actually observed.
///
/// Two properties follow, and both are the point:
///
/// * **No guessed language.** Every declared locale is named
///   [`measured_locale_label`], so a label cannot assert a language the
///   measurement did not contain. A locale that is not in the table cannot be
///   declared here at all.
/// * **The measurement stays attached.** [`MeasuredLocales::table`] keeps every
///   occurrence with its span and row count, so a report can name the source of
///   each declared locale instead of asserting a list.
///
/// The derived set is still the *installation's* locale set, not the release's.
/// A second, localized installation is what measures the release-wide list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeasuredLocales {
    table: ResourceLanguageTable,
    locales: SupportedLocales,
    languages: LanguageMap,
}

impl MeasuredLocales {
    /// Derives the declaration from a measured table.
    ///
    /// The declared locales are the measured language ids in ascending order,
    /// each labelled by [`measured_locale_label`], so the declaration is a
    /// function of the measurement and nothing else.
    ///
    /// # Errors
    ///
    /// [`MeasuredLocalesError::NothingMeasured`] when no occurrence was
    /// recorded, and [`MeasuredLocalesError::TooManyLanguages`] when the table
    /// holds more distinct ids than [`MAX_LANGUAGE_MAP_LEN`].
    pub fn from_table(table: ResourceLanguageTable) -> Result<Self, MeasuredLocalesError> {
        let languages = table.languages();
        if languages.is_empty() {
            return Err(MeasuredLocalesError::NothingMeasured);
        }
        if languages.len() > MAX_LANGUAGE_MAP_LEN {
            return Err(MeasuredLocalesError::TooManyLanguages {
                len: languages.len(),
            });
        }
        let locales = SupportedLocales::new(
            languages
                .iter()
                .map(|language| LocaleId::new(&measured_locale_label(*language)))
                .map(|label| label.expect("a measured label is ASCII alphanumerics and one dash")),
        )
        .expect("measured labels are unique because the language ids are");
        let language_map = LanguageMap::new(languages.iter().map(|language| {
            (
                *language,
                LocaleId::new(&measured_locale_label(*language))
                    .expect("a measured label is a valid locale label"),
            )
        }))
        .expect("measured languages are distinct, so the map has no duplicate");
        Ok(Self {
            table,
            locales,
            languages: language_map,
        })
    }

    /// The measured table the declaration was derived from.
    #[must_use]
    pub fn table(&self) -> &ResourceLanguageTable {
        &self.table
    }

    /// The declared supported-locale set, in measured-id order.
    #[must_use]
    pub fn supported(&self) -> &SupportedLocales {
        &self.locales
    }

    /// The resource-language map the audit decodes rows with.
    #[must_use]
    pub fn language_map(&self) -> &LanguageMap {
        &self.languages
    }

    /// The measured language ids, in ascending order.
    #[must_use]
    pub fn languages(&self) -> Vec<u32> {
        self.table.languages()
    }

    /// The locale a measured resource language id was declared under.
    #[must_use]
    pub fn locale_for(&self, language: u32) -> Option<&LocaleId> {
        self.languages.locale(language)
    }

    /// How many locales the measurement declared.
    #[must_use]
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.locales.len()
    }

    /// Always `false`: [`MeasuredLocales::from_table`] refuses an empty
    /// measurement, so a value that exists always declares at least one locale.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.locales.is_empty()
    }
}

/// The id-numbering comparison of two locales of the same string surface.
///
/// This is the machine-readable shape of F12 AC04 — *a localized installation
/// preserves stable ids while changing display text*. Stability is a property
/// of the **id sets**: a translation that renumbers or drops ids is not a
/// translation. The text half is counted too, so a comparison can say that ids
/// held while the display text changed, and can distinguish that from two
/// locales that carry the very same strings.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IdNumbering {
    /// Ids both locales answer.
    pub shared: usize,
    /// Ids only the first locale answers.
    pub only_first: Vec<TextId>,
    /// Ids only the second locale answers.
    pub only_second: Vec<TextId>,
    /// Shared ids whose two rows hold exactly the same text.
    pub identical_text: usize,
    /// Shared ids whose two rows hold different text.
    pub changed_text: usize,
}

impl IdNumbering {
    /// Whether both locales answer exactly the same ids.
    ///
    /// This is the stability question, and it is deliberately independent of
    /// the text counts: an unchanged proper noun is not a renumbering, and a
    /// changed string under a vanished id is not stability either.
    #[must_use]
    pub fn is_stable(&self) -> bool {
        self.only_first.is_empty() && self.only_second.is_empty()
    }

    /// How many ids the two locales answered in common.
    #[must_use]
    pub fn compared(&self) -> usize {
        self.shared
    }

    /// How many ids one locale has and the other does not.
    #[must_use]
    pub fn renumbered(&self) -> usize {
        self.only_first.len() + self.only_second.len()
    }

    /// How many shared ids changed their display text.
    #[must_use]
    pub fn changed(&self) -> usize {
        self.changed_text
    }
}

/// The answer to "do two measured locales keep the same string ids?".
///
/// F12 AC04 — *a localized installation preserves stable ids while changing
/// display text* — is a question about **two** installations' worth of strings.
/// One installation carries one language id, so for a single-language
/// installation the question cannot be asked, and the honest result is
/// [`IdStability::SingleLocale`] naming what *was* measured. That distinction is
/// the reason this type exists: a one-installation run must never be able to
/// report [`IdStability::Compared`] and therefore must never be able to look
/// like AC04 evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdStability {
    /// Only one locale was available, so no comparison was made.
    SingleLocale {
        /// The locale that *was* measured.
        measured: LocaleId,
        /// How many ids it answered.
        ids: usize,
    },
    /// Two locales were measured and compared.
    Compared {
        /// The first locale, as measured.
        first: LocaleId,
        /// The second locale, as measured.
        second: LocaleId,
        /// The comparison itself.
        numbering: IdNumbering,
    },
}

impl IdStability {
    /// Whether two locales were actually compared.
    #[must_use]
    pub fn is_compared(&self) -> bool {
        matches!(self, Self::Compared { .. })
    }

    /// The comparison, if two locales were measured.
    #[must_use]
    pub fn numbering(&self) -> Option<&IdNumbering> {
        match self {
            Self::Compared { numbering, .. } => Some(numbering),
            Self::SingleLocale { .. } => None,
        }
    }

    /// Whether the two measured locales kept the same id numbering.
    ///
    /// `false` for [`IdStability::SingleLocale`]: a single installation is not
    /// evidence of id stability, and reporting otherwise would turn an
    /// unmeasured criterion into a pass.
    #[must_use]
    pub fn is_stable(&self) -> bool {
        self.numbering().is_some_and(IdNumbering::is_stable)
    }
}

/// Compares the id numbering of two measured locales, or records that only one
/// locale was available.
///
/// `localized` is `None` when no second installation was available: the result
/// is then [`IdStability::SingleLocale`] naming the locale that was measured,
/// which is a measurement rather than an assumption.
#[must_use]
pub fn measure_id_stability(
    first_catalog: &TextCatalog,
    first: &LocaleId,
    localized: Option<(&TextCatalog, &LocaleId)>,
) -> IdStability {
    let Some((second_catalog, second)) = localized else {
        return IdStability::SingleLocale {
            measured: first.clone(),
            ids: first_catalog.ids_for(first).len(),
        };
    };
    IdStability::Compared {
        first: first.clone(),
        second: second.clone(),
        numbering: first_catalog.compare_installation_ids(first, second_catalog, second),
    }
}

impl TextCatalog {
    /// The distinct ids `locale` answers, in id order.
    #[must_use]
    pub fn ids_for(&self, locale: &LocaleId) -> Vec<TextId> {
        self.rows()
            .filter(|row| row.locale() == locale)
            .map(|row| row.id().clone())
            .collect()
    }

    /// Compares the id numbering of two locales of this catalog.
    ///
    /// This is the same-image form: a catalog decoded from one image that
    /// carries two languages.
    #[must_use]
    pub fn compare_locale_ids(&self, first: &LocaleId, second: &LocaleId) -> IdNumbering {
        self.compare_installation_ids(first, self, second)
    }

    /// Compares the id numbering of one locale of this catalog against one
    /// locale of another catalog.
    ///
    /// This is the cross-installation form of F12 AC04: hand it the catalog
    /// decoded from an English installation and the catalog decoded from a
    /// localized one, and it reports whether the localized build kept the id
    /// numbering. Nothing here is asserted about a second installation — the
    /// comparison is a measurement that needs both catalogs, and with one
    /// installation the honest answer is that the question was not asked.
    #[must_use]
    pub fn compare_installation_ids(
        &self,
        first: &LocaleId,
        other: &Self,
        second: &LocaleId,
    ) -> IdNumbering {
        let first_ids: BTreeSet<TextId> = self.ids_for(first).into_iter().collect();
        let second_ids: BTreeSet<TextId> = other.ids_for(second).into_iter().collect();

        let only_first: Vec<TextId> = first_ids.difference(&second_ids).cloned().collect();
        let only_second: Vec<TextId> = second_ids.difference(&first_ids).cloned().collect();

        let mut identical_text = 0usize;
        let mut changed_text = 0usize;
        for id in first_ids.intersection(&second_ids) {
            match (self.get(id, first), other.get(id, second)) {
                // Two rows under one id always carry the same text, because a
                // duplicate `(id, locale)` pair is refused when it is inserted,
                // so `None` on either side is unreachable and must not be
                // silently counted as "unchanged".
                (Some(left), Some(right)) if left.text() == right.text() => identical_text += 1,
                (Some(_), Some(_)) => changed_text += 1,
                (Some(_), None) | (None, Some(_)) | (None, None) => {
                    unreachable!("an id collected from a locale has a row in it")
                }
            }
        }

        IdNumbering {
            shared: first_ids.intersection(&second_ids).count(),
            only_first,
            only_second,
            identical_text,
            changed_text,
        }
    }
}
