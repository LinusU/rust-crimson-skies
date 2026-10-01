//! Versioned profile document and persistent profile ids (F48-A).
//!
//! Spec: `specs/F48-profiles-saves-settings-migration-and-recovery.md`,
//! contract `docs/contracts/STATE-TRANSACTIONS.md` ("Persistence").
//!
//! These are newly authored engine-save records; nothing here is derived from
//! original game data (legacy import is F64). The byte encoding and the
//! atomic write protocol live in `cs_content::save`.
//!
//! * A [`ProfileId`] is a persistent number allocated from a
//!   [`ProfileRegistry`] high-water mark. It is not a display name and not a
//!   list index, and deleting a profile never lowers the mark, so an old id
//!   never refers to a new profile.
//! * A [`ProfileDocument`] is one complete, monotonically revisioned state.
//!   Fields this build does not know are kept in
//!   [`ProfileDocument::extra`] so a same-major newer save is not stripped.

use std::fmt;

use crate::content::ContentId;

/// Largest accepted display name, in bytes.
pub const MAX_DISPLAY_NAME_BYTES: usize = 64;
/// Largest accepted key (setting, record, fingerprint, extra) in bytes.
pub const MAX_KEY_BYTES: usize = 64;
/// Largest accepted free-text value in bytes.
pub const MAX_VALUE_BYTES: usize = 256;
/// Largest accepted number of entries in any one list of a document.
pub const MAX_LIST_ENTRIES: usize = 512;

/// Schema version of a [`ProfileDocument`]. A different major is unreadable;
/// a newer minor of the same major is readable and its unknown fields are
/// preserved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SchemaVersion {
    pub major: u16,
    pub minor: u16,
}

impl SchemaVersion {
    /// The version this build writes.
    pub const CURRENT: Self = Self { major: 1, minor: 0 };

    /// Whether this build can read a document of this version.
    pub const fn is_readable(self) -> bool {
        self.major == Self::CURRENT.major
    }
}

impl fmt::Display for SchemaVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// Persistent profile identity. Never zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProfileId(u64);

impl ProfileId {
    /// Builds an id from its persisted number; zero is refused.
    pub const fn new(value: u64) -> Option<Self> {
        if value == 0 { None } else { Some(Self(value)) }
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for ProfileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Which population a profile belongs to. Kinds live in separate stores so an
/// automated or modded session never touches the player's production profile
/// (spec F48 non-negotiable 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProfileKind {
    Production,
    Synthetic,
    Modded,
    Evidence,
}

impl ProfileKind {
    pub const ALL: [Self; 4] = [
        Self::Production,
        Self::Synthetic,
        Self::Modded,
        Self::Evidence,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Production => "production",
            Self::Synthetic => "synthetic",
            Self::Modded => "modded",
            Self::Evidence => "evidence",
        }
    }

    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.label() == label)
    }
}

/// Monotonic revision of one profile; each committed write increases it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Revision(pub u64);

impl Revision {
    /// The revision after this one, or `None` on overflow.
    pub const fn next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

/// Whether a setting change applies immediately or needs a restart (spec F48
/// non-negotiable 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingApply {
    Live,
    RestartRequired,
}

impl SettingApply {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::RestartRequired => "restart",
        }
    }

    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "live" => Some(Self::Live),
            "restart" => Some(Self::RestartRequired),
            _ => None,
        }
    }
}

/// One stored setting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingEntry {
    pub key: String,
    pub apply: SettingApply,
    pub value: String,
}

/// Campaign progress. Currency is integer minor game units; the display
/// mapping is not decided here. Applied outcome ids make a replayed outcome
/// idempotent (contract "Outcome and economy transaction").
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CampaignState {
    pub run_id: Option<String>,
    pub money_minor: u64,
    pub applied_outcomes: Vec<String>,
}

/// A named counter (record).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordEntry {
    pub key: String,
    pub value: u64,
}

/// A compatibility fingerprint (for example of the installation or content
/// catalog) the save was written against.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FingerprintEntry {
    pub name: String,
    pub hash: u64,
}

/// A field this build does not understand, kept verbatim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtraField {
    pub key: String,
    pub value: String,
}

/// One complete profile revision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileDocument {
    pub schema: SchemaVersion,
    pub profile_id: ProfileId,
    pub kind: ProfileKind,
    pub display_name: String,
    pub revision: Revision,
    pub campaign: CampaignState,
    pub blueprints: Vec<ContentId>,
    pub records: Vec<RecordEntry>,
    pub settings: Vec<SettingEntry>,
    pub fingerprints: Vec<FingerprintEntry>,
    pub extra: Vec<ExtraField>,
}

/// Why a key, name or value was refused. `NoSuccessor` is a monotonic counter
/// at the top of its range: it could never be advanced again, so the document
/// carrying it is refused rather than written or accepted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileFieldError {
    Empty { field: &'static str },
    TooLong { field: &'static str, max: usize },
    BadCharacter { field: &'static str },
    TooManyEntries { field: &'static str, max: usize },
    DuplicateKey { field: &'static str, key: String },
    NoSuccessor { field: &'static str },
}

impl fmt::Display for ProfileFieldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty { field } => write!(f, "{field} must not be empty"),
            Self::TooLong { field, max } => write!(f, "{field} exceeds {max} bytes"),
            Self::BadCharacter { field } => write!(f, "{field} contains a disallowed character"),
            Self::TooManyEntries { field, max } => write!(f, "{field} has more than {max} entries"),
            Self::DuplicateKey { field, key } => write!(f, "{field} repeats key {key:?}"),
            Self::NoSuccessor { field } => write!(
                f,
                "the {field} is at the end of its range, so nothing could be written after it"
            ),
        }
    }
}

impl std::error::Error for ProfileFieldError {}

/// Keys are `[A-Za-z0-9._-]`, 1..=[`MAX_KEY_BYTES`] bytes, and never start
/// with `.` — so a key can never be a path component that traverses.
pub fn validate_key(field: &'static str, key: &str) -> Result<(), ProfileFieldError> {
    if key.is_empty() {
        return Err(ProfileFieldError::Empty { field });
    }
    if key.len() > MAX_KEY_BYTES {
        return Err(ProfileFieldError::TooLong {
            field,
            max: MAX_KEY_BYTES,
        });
    }
    let ok = key
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        && !key.starts_with('.');
    if ok {
        Ok(())
    } else {
        Err(ProfileFieldError::BadCharacter { field })
    }
}

/// Free text must have no control characters and fit `max` bytes.
pub fn validate_text(
    field: &'static str,
    text: &str,
    max: usize,
    allow_empty: bool,
) -> Result<(), ProfileFieldError> {
    if text.is_empty() && !allow_empty {
        return Err(ProfileFieldError::Empty { field });
    }
    if text.len() > max {
        return Err(ProfileFieldError::TooLong { field, max });
    }
    if text.chars().any(char::is_control) {
        return Err(ProfileFieldError::BadCharacter { field });
    }
    Ok(())
}

fn validate_unique<'a>(
    field: &'static str,
    keys: impl ExactSizeIterator<Item = &'a str>,
) -> Result<(), ProfileFieldError> {
    if keys.len() > MAX_LIST_ENTRIES {
        return Err(ProfileFieldError::TooManyEntries {
            field,
            max: MAX_LIST_ENTRIES,
        });
    }
    let mut seen = std::collections::BTreeSet::new();
    for key in keys {
        if !seen.insert(key) {
            return Err(ProfileFieldError::DuplicateKey {
                field,
                key: key.to_owned(),
            });
        }
    }
    Ok(())
}

impl ProfileDocument {
    /// The minimal synthetic fixture: a fresh, empty profile at revision 1.
    /// Newly authored (`SYNTHETIC`), never retail data.
    pub fn synthetic(profile_id: ProfileId, revision: Revision) -> Self {
        Self {
            schema: SchemaVersion::CURRENT,
            profile_id,
            kind: ProfileKind::Synthetic,
            display_name: "Synthetic Pilot".to_owned(),
            revision,
            campaign: CampaignState::default(),
            blueprints: Vec::new(),
            records: Vec::new(),
            settings: Vec::new(),
            fingerprints: Vec::new(),
            extra: Vec::new(),
        }
    }

    /// Checks every size, character and uniqueness bound. The encoder and the
    /// decoder both call this, so an out-of-range document is neither written
    /// nor accepted.
    pub fn validate(&self) -> Result<(), ProfileFieldError> {
        if self.revision.next().is_none() {
            // A revision with no successor could never be written again. The
            // encoder refuses to produce one and the decoder refuses to accept
            // one, so a slot carrying it falls back to its previous whole
            // revision instead of becoming a profile that can never be saved.
            return Err(ProfileFieldError::NoSuccessor { field: "revision" });
        }
        validate_text(
            "display_name",
            &self.display_name,
            MAX_DISPLAY_NAME_BYTES,
            false,
        )?;
        if let Some(run) = &self.campaign.run_id {
            validate_key("campaign.run", run)?;
        }
        for outcome in &self.campaign.applied_outcomes {
            validate_key("campaign.outcome", outcome)?;
        }
        validate_unique(
            "campaign.outcome",
            self.campaign
                .applied_outcomes
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .into_iter(),
        )?;
        validate_unique(
            "blueprint",
            self.blueprints
                .iter()
                .map(ContentId::as_str)
                .collect::<Vec<_>>()
                .into_iter(),
        )?;
        for record in &self.records {
            validate_key("record", &record.key)?;
        }
        validate_unique(
            "record",
            self.records
                .iter()
                .map(|r| r.key.as_str())
                .collect::<Vec<_>>()
                .into_iter(),
        )?;
        for setting in &self.settings {
            validate_key("setting", &setting.key)?;
            validate_text("setting value", &setting.value, MAX_VALUE_BYTES, true)?;
        }
        validate_unique(
            "setting",
            self.settings
                .iter()
                .map(|s| s.key.as_str())
                .collect::<Vec<_>>()
                .into_iter(),
        )?;
        for fingerprint in &self.fingerprints {
            validate_key("fingerprint", &fingerprint.name)?;
        }
        validate_unique(
            "fingerprint",
            self.fingerprints
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>()
                .into_iter(),
        )?;
        if self.extra.len() > MAX_LIST_ENTRIES {
            return Err(ProfileFieldError::TooManyEntries {
                field: "extra",
                max: MAX_LIST_ENTRIES,
            });
        }
        for extra in &self.extra {
            validate_key("extra key", &extra.key)?;
            validate_text("extra value", &extra.value, MAX_VALUE_BYTES, true)?;
        }
        Ok(())
    }
}

/// Persistent id allocation: the registry of live profiles, the active
/// pointer and the high-water mark (contract: "allocated from a persistent
/// high-water mark").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileRegistry {
    high_water: u64,
    live: Vec<ProfileId>,
    active: Option<ProfileId>,
}

/// Registry refusal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegistryError {
    /// The persisted high-water mark is below a live id.
    HighWaterBelowLive { high_water: u64, live: ProfileId },
    /// The id is not a live profile.
    UnknownProfile(ProfileId),
    /// The live list names an id twice.
    DuplicateLive(ProfileId),
    /// Ids are exhausted: the mark is at the top of its range, so no further
    /// id could ever be allocated from it.
    Exhausted,
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HighWaterBelowLive { high_water, live } => {
                write!(
                    f,
                    "high-water mark {high_water} is below live profile {live}"
                )
            }
            Self::UnknownProfile(id) => write!(f, "profile {id} is not live"),
            Self::DuplicateLive(id) => write!(f, "profile {id} is listed twice"),
            Self::Exhausted => write!(f, "profile ids exhausted"),
        }
    }
}

impl std::error::Error for RegistryError {}

impl ProfileRegistry {
    pub const fn new() -> Self {
        Self {
            high_water: 0,
            live: Vec::new(),
            active: None,
        }
    }

    /// Rebuilds a registry from persisted parts, refusing an inconsistent set.
    pub fn from_parts(
        high_water: u64,
        live: Vec<ProfileId>,
        active: Option<ProfileId>,
    ) -> Result<Self, RegistryError> {
        if high_water == u64::MAX {
            // A mark with no successor could never allocate another id, so a
            // set that carries one is refused where its parts are validated
            // rather than accepted and left as a population that can never gain
            // a profile again.
            return Err(RegistryError::Exhausted);
        }
        for (index, id) in live.iter().enumerate() {
            if id.get() > high_water {
                return Err(RegistryError::HighWaterBelowLive {
                    high_water,
                    live: *id,
                });
            }
            if live[..index].contains(id) {
                return Err(RegistryError::DuplicateLive(*id));
            }
        }
        // A dangling active pointer is dropped rather than trusted.
        let active = active.filter(|id| live.contains(id));
        Ok(Self {
            high_water,
            live,
            active,
        })
    }

    /// Recovery when the high-water file is missing: the mark can only be
    /// inferred from the surviving ids, which cannot know about a deleted
    /// higher id. Callers must surface that limitation (see the F48-A
    /// findings).
    ///
    /// The surviving ids are not a trusted source, so this validates them the
    /// same way [`ProfileRegistry::from_parts`] does and reports the refusal
    /// rather than panicking on it: a repeated id, or a set that would have no
    /// successor left, is an error the caller has to surface.
    pub fn rebuild_from_live(
        live: Vec<ProfileId>,
        active: Option<ProfileId>,
    ) -> Result<Self, RegistryError> {
        let high_water = live.iter().map(|id| id.get()).max().unwrap_or(0);
        Self::from_parts(high_water, live, active)
    }

    pub const fn high_water(&self) -> u64 {
        self.high_water
    }

    pub fn live(&self) -> &[ProfileId] {
        &self.live
    }

    pub const fn active(&self) -> Option<ProfileId> {
        self.active
    }

    /// Allocates a fresh id above every id ever issued and makes it live. The
    /// first profile becomes active.
    pub fn allocate(&mut self) -> Result<ProfileId, RegistryError> {
        let next = self
            .high_water
            .checked_add(1)
            .ok_or(RegistryError::Exhausted)?;
        let id = ProfileId::new(next).ok_or(RegistryError::Exhausted)?;
        self.high_water = next;
        self.live.push(id);
        if self.active.is_none() {
            self.active = Some(id);
        }
        Ok(id)
    }

    /// Deletes a profile. The high-water mark does not move, so the id is
    /// never issued again.
    pub fn delete(&mut self, id: ProfileId) -> Result<(), RegistryError> {
        let Some(position) = self.live.iter().position(|live| *live == id) else {
            return Err(RegistryError::UnknownProfile(id));
        };
        self.live.remove(position);
        if self.active == Some(id) {
            self.active = self.live.first().copied();
        }
        Ok(())
    }

    pub fn set_active(&mut self, id: ProfileId) -> Result<(), RegistryError> {
        if !self.live.contains(&id) {
            return Err(RegistryError::UnknownProfile(id));
        }
        self.active = Some(id);
        Ok(())
    }
}

impl Default for ProfileRegistry {
    fn default() -> Self {
        Self::new()
    }
}
