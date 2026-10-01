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
//! # What this stage does not do
//!
//! No font is parsed, no glyph is rasterized, no string is loaded from the
//! owner's installation, and the F12 PE string catalog is deliberately not
//! bridged in: mapping a `StringRow.language` to a [`LocaleId`] needs the
//! language ids the retail images carry, which is F51-B's measurement. This
//! crate therefore never depends on `cs_formats` through this module and stays
//! Bevy-free.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentId, ContentIdError, ContentKind, Origin, Provenance};

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

    /// The string ids the catalog holds, deduplicated and in id order.
    #[must_use]
    pub fn ids(&self) -> Vec<TextId> {
        self.rows.keys().map(|(id, _)| id.clone()).collect()
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

    /// Audits every id in the catalog against `chain`.
    ///
    /// This is the machine-readable shape of F51's AC04 ("audit all strings and
    /// media for each declared supported original locale"): the coverage
    /// denominator is the catalog's own ids, and an id with no row anywhere in
    /// the chain stays counted as missing instead of being dropped.
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
    /// How many distinct string ids the catalog holds.
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
    Control {
        /// The tag's name.
        name: String,
        /// The `=argument` value, when the tag takes one.
        argument: Option<String>,
    },
    /// An admitted substitution: an id whose value the layout supplies.
    Substitution {
        /// The substitution id, as written between the substitution
        /// delimiters.
        id: String,
    },
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
    /// name is appended to `unresolved` so the caller can report it.
    #[must_use]
    pub fn paragraphs(
        &self,
        substitutions: &SubstitutionTable,
    ) -> (Vec<String>, Vec<SubstitutionId>) {
        let mut paragraphs = vec![String::new()];
        let mut unresolved = Vec::new();
        for token in &self.tokens {
            match token {
                MarkupToken::Text(text) => {
                    paragraphs
                        .last_mut()
                        .expect("one paragraph exists")
                        .push_str(text);
                }
                MarkupToken::LineBreak => paragraphs.push(String::new()),
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
                        if !unresolved.iter().any(|seen: &String| seen == id) {
                            unresolved.push(id.clone());
                        }
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
    pub fn insert(&mut self, id: impl Into<SubstitutionId>, value: impl Into<String>) {
        self.values.insert(id.into(), value.into());
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
            keep_literal(document, literal, delimiters, closed);
            document
                .issues
                .push(MarkupIssue::new(MarkupIssueKind::EmptySubstitution {
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
            let uncovered: BTreeSet<char> = text
                .chars()
                .filter(|ch| !ch.is_whitespace())
                .filter(|ch| self.faces.values().all(|face| !face.coverage().covers(*ch)))
                .collect();
            for ch in uncovered {
                let occurrences = text.chars().filter(|other| *other == ch).count();
                report.add(ch, occurrences);
            }
        }
        report
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
                   corridor before you take the runway.";
    let long_de = "Achtung Besatzung. Die Wetterfront ueber den Schifffahrtsrouten ist seit dem \
                   letzten Briefing nach Norden gewandert, der zugewiesene Korridor kreuzt daher \
                   den Rand der Sturmzelle. Geringere Sicht, zeitweiliger Regen und eine laengere \
                   Ueberfahrt als geplant sind zu erwarten. Bestaetigen Sie dieses Briefing und \
                   geben Sie Ihren zugewiesenen Korridor an, bevor Sie die Startbahn nehmen.";
    let long_fr = "Equipage, attention. Le front meteorologique au-dessus des couloirs maritimes a \
                   remonte vers le nord depuis le dernier briefing; le corridor assigne croise donc \
                   le bord de la cellule de tempete. Attendez-vous a une visibilite reduite, a des \
                   pluies intermittentes et a une traversée plus longue que prevu.";
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
