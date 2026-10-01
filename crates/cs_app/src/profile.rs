//! Profile slot locations, population separation, the runtime library handle
//! and the session that owns a profile for a run (F48-A, F48-B, F48-C).
//!
//! Spec: `specs/F48-profiles-saves-settings-migration-and-recovery.md`
//! (non-negotiable 3, 4 and 5); contract `docs/contracts/STATE-TRANSACTIONS.md`
//! ("Session reset", "Outcome and economy transaction", "Persistence").
//! Production, synthetic, modded and evidence profiles live in separate
//! subtrees of one user-data base, and a slot directory is derived only from a
//! [`ProfileKind`] label and the numeric [`ProfileId`], never from a display
//! name, so a name cannot traverse out of the base. An automated session is
//! refused the production subtree outright.
//!
//! Choosing the user-data base itself is F61. This module maps
//! `(base, origin, kind, id)` to a directory and opens the production
//! [`ProfileLibrary`] through that mapping, so the rule that automation never
//! touches a live profile is enforced where the path is produced rather than
//! left to every caller to remember. The library itself does not know about
//! sessions: it is handed one population's directory and does IO there.
//!
//! Stage `### F48-C` adds the wiring that makes the library a *runtime*
//! resource rather than a thing tests call:
//!
//! * [`ProfileSession`] is the producer/consumer pair for a run: it opens one
//!   population, **exclusively**, loads the selected profile's document and
//!   settings, applies changes to an in-memory copy and commits them as one
//!   whole revision through the library's atomic path.
//! * [`PopulationClaim`] is that exclusivity. F48-B left two live libraries
//!   over one directory unprevented; a run now holds a claim it must release,
//!   and a second session on the same population is refused rather than
//!   silently interleaving two owners of one revision counter.
//! * [`ProfileSession::commit_with`] is the one write path, and it **retries**
//!   by re-reading the stored revision: a conflict refreshes the view and the
//!   change is re-applied to what is actually stored, never written over it.
//!   A refresh replaces the *document* only — this session's own uncommitted
//!   settings are re-applied on top of it, so a retry adopts the other writer's
//!   progression without discarding the change this session was asked to make.
//! * Every commit is checked against the session's own profile id, so no caller
//!   can retarget this session's writes at another pilot's save.
//! * [`ProfileSession::finish`] is the explicit teardown that reports whether
//!   uncommitted work was dropped; dropping the session releases the claim
//!   either way, so an error path cannot leak ownership.
//! * [`open_sandbox`] is the sandbox rule: an automated session over the
//!   synthetic population, which is never a player's live profile tree.
//! * Opening a population **reads** the persisted active pointer and never
//!   rewrites it, so launching costs no write and a read-only profile tree is
//!   still openable.

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use cs_content::save::fs::SlotError;
use cs_content::save::library::{LibraryError, LoadedProfile, ProfileLibrary};
use cs_content::save::settings::{CatalogError, SettingCatalog, SettingOutcome, SettingsState};
use cs_content::save::store::CommitError;
use cs_types::profile::{
    CampaignState, MAX_DISPLAY_NAME_BYTES, ProfileDocument, ProfileFieldError, ProfileId,
    ProfileKind, Revision, validate_text,
};

/// Who is asking for a profile location.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionOrigin {
    /// A person playing.
    Interactive,
    /// Tests, evidence harnesses and other automation.
    Automated,
}

/// Refusal to hand out a location.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileDirError {
    /// Automation asked for the live production profile tree.
    AutomatedProductionAccess,
}

impl fmt::Display for ProfileDirError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AutomatedProductionAccess => {
                write!(
                    f,
                    "automated sessions may not use the production profile tree"
                )
            }
        }
    }
}

impl std::error::Error for ProfileDirError {}

/// The directory holding one profile's `profile.sav`/`.bak`/`.tmp`.
pub fn slot_dir(
    base: &Path,
    origin: SessionOrigin,
    kind: ProfileKind,
    id: ProfileId,
) -> Result<PathBuf, ProfileDirError> {
    if origin == SessionOrigin::Automated && kind == ProfileKind::Production {
        return Err(ProfileDirError::AutomatedProductionAccess);
    }
    Ok(base.join(kind.label()).join(format!("profile-{id}")))
}

/// The root of one population's subtree: where its registry and its profile
/// slots live.
pub fn population_dir(
    base: &Path,
    origin: SessionOrigin,
    kind: ProfileKind,
) -> Result<PathBuf, ProfileDirError> {
    // The same refusal as `slot_dir`, and for the same reason: the population
    // root contains every slot, so a session that may not open a production
    // slot may not open the production root either.
    if origin == SessionOrigin::Automated && kind == ProfileKind::Production {
        return Err(ProfileDirError::AutomatedProductionAccess);
    }
    Ok(base.join(kind.label()))
}

/// Why a population's library could not be opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LibraryOpenError {
    /// Automation asked for the live production profile tree.
    Dir(ProfileDirError),
    /// The directory or a file in it could not be used.
    Slot(SlotError),
    /// The registry, a slot or a write failed.
    Library(LibraryError),
}

impl fmt::Display for LibraryOpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dir(error) => write!(f, "{error}"),
            Self::Slot(error) => write!(f, "{error}"),
            Self::Library(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for LibraryOpenError {}

impl From<ProfileDirError> for LibraryOpenError {
    fn from(error: ProfileDirError) -> Self {
        Self::Dir(error)
    }
}

impl From<SlotError> for LibraryOpenError {
    fn from(error: SlotError) -> Self {
        Self::Slot(error)
    }
}

impl From<LibraryError> for LibraryOpenError {
    fn from(error: LibraryError) -> Self {
        Self::Library(error)
    }
}

/// Opens one population's profile library, enforcing the population rule at the
/// point the path is produced.
///
/// The directory is created if it is absent, and the registry slot is recovered
/// on the way in; a registry that does not decode is an error rather than an
/// empty library, because an unreadable high-water mark read as zero is what
/// would reissue a deleted profile id.
pub fn open_library(
    base: &Path,
    origin: SessionOrigin,
    kind: ProfileKind,
) -> Result<ProfileLibrary, LibraryOpenError> {
    let directory = population_dir(base, origin, kind)?;
    Ok(ProfileLibrary::open(directory, kind)?)
}

// ---------------------------------------------------------------------------
// F48-C: the session that owns a profile for a run
// ---------------------------------------------------------------------------

/// How many times one commit re-reads the stored revision and re-applies its
/// change before it gives up.
///
/// One attempt is the normal case; the loop exists for the case the contract
/// names — two writers, one profile — and it stops rather than retrying a
/// conflict that keeps moving, so a permanently busy profile is reported as a
/// conflict the caller can show instead of spinning.
pub const MAX_COMMIT_ATTEMPTS: u32 = 3;

/// The population directories this process holds open, so two live sessions
/// cannot both believe they own one population's revision counters.
///
/// In-process only, and honestly so: a second *process* is caught by the
/// registry's revision check, which is a conflict and not corruption. What this
/// prevents is the case inside one process — a menu, a mission teardown and a
/// campaign task each opening the same library and interleaving writes to one
/// profile.
static HELD_POPULATIONS: OnceLock<Mutex<BTreeSet<PathBuf>>> = OnceLock::new();

fn held_populations() -> &'static Mutex<BTreeSet<PathBuf>> {
    HELD_POPULATIONS.get_or_init(|| Mutex::new(BTreeSet::new()))
}

/// Why a population could not be claimed for a session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClaimError {
    /// Another live session in this process already holds this population.
    AlreadyHeld { directory: PathBuf },
}

impl fmt::Display for ClaimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyHeld { directory } => write!(
                f,
                "{} is already held by another session in this process",
                directory.display()
            ),
        }
    }
}

impl std::error::Error for ClaimError {}

/// Exclusive ownership of one population directory for the lifetime of a
/// session.
///
/// Released on drop, so an error path — a panic, an early return, a refused
/// open — cannot leave a population permanently unopenable. [`ProfileSession::
/// finish`] releases it explicitly and reports what the session ended with;
/// dropping a session without finishing it releases the claim and nothing else,
/// which is why [`ProfileSession`] reports uncommitted work rather than
/// committing it behind the caller's back.
#[derive(Debug)]
pub struct PopulationClaim {
    directory: PathBuf,
}

impl PopulationClaim {
    /// Claims a population directory exclusively.
    ///
    /// The key is the canonicalized directory, so two spellings of one directory
    /// (a symlink, a relative path) are one population rather than two. A
    /// directory that does not exist yet cannot be canonicalized, so the key
    /// falls back to the path as given — the claim is still exclusive, it is just
    /// spelling-sensitive until the first write creates the directory. That is a
    /// bound on the in-process guard, not a filesystem lock; the registry's
    /// revision check is what catches a second *process*.
    fn acquire(directory: &Path) -> Result<Self, ClaimError> {
        let key = std::fs::canonicalize(directory).unwrap_or_else(|_| directory.to_path_buf());
        let mut held = held_populations()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !held.insert(key.clone()) {
            return Err(ClaimError::AlreadyHeld {
                directory: key.clone(),
            });
        }
        Ok(Self { directory: key })
    }

    /// The claimed directory, as it was resolved when it was claimed.
    pub fn directory(&self) -> &Path {
        &self.directory
    }
}

impl Drop for PopulationClaim {
    fn drop(&mut self) {
        if let Ok(mut held) = held_populations().lock() {
            held.remove(&self.directory);
        }
    }
}

/// Why a change to a profile's campaign state was refused before anything was
/// written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeRefusal {
    /// What was being applied.
    pub subject: String,
    pub reason: ChangeRefusalReason,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChangeRefusalReason {
    /// The outcome id is already in the profile's applied list, so replaying it
    /// would pay twice (contract: "A crash before acknowledgment can safely
    /// replay the same outcome without double reward").
    AlreadyApplied,
    /// The subject is not a usable key in a save document.
    Malformed(String),
}

impl fmt::Display for ChangeRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.reason {
            ChangeRefusalReason::AlreadyApplied => {
                write!(f, "{} was already applied to this profile", self.subject)
            }
            ChangeRefusalReason::Malformed(reason) => {
                write!(
                    f,
                    "{} is not usable as an outcome key: {reason}",
                    self.subject
                )
            }
        }
    }
}

/// What a commit attempt did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutcomeRecord {
    /// The outcome id was added and written as one whole revision.
    Recorded,
    /// The outcome id was already in the profile's applied list. Nothing was
    /// written, which is what makes a replay after a crash safe.
    AlreadyApplied,
}

/// Why a session operation failed. Every failure propagates as one of these and
/// none is reported as success.
#[derive(Debug)]
pub enum SessionError {
    /// The population could not be opened.
    Open(LibraryOpenError),
    /// The population is already held by another session in this process.
    Claim(ClaimError),
    /// The declared settings catalog does not hold together.
    Catalog(CatalogError),
    /// A name, key or value is outside the bounds a save may hold.
    Field(ProfileFieldError),
    /// The library refused or failed.
    Library(LibraryError),
    /// The operation needs a selected profile and none is selected.
    NoProfileSelected,
    /// The profile is live in the registry but its slot holds no whole revision,
    /// so there is no pilot to select. Recovery normally prevents this — an
    /// adopted profile must hold a save file — so it means the slot lost its
    /// files after the registry named it.
    EmptySlot { profile: ProfileId },
    /// A commit tried to write a document naming a profile other than the one
    /// this session owns. Nothing was written: the library would have installed
    /// it into that other profile's slot.
    ForeignProfile {
        expected: ProfileId,
        offered: ProfileId,
    },
    /// The revision counter is at the top of its range.
    RevisionExhausted { profile: ProfileId },
    /// Every attempt hit a moved expected revision.
    Conflict {
        profile: ProfileId,
        stored: Revision,
        attempts: u32,
    },
    /// The change itself was refused; nothing was written.
    Refused(ChangeRefusal),
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open(error) => write!(f, "cannot open the profile population: {error}"),
            Self::Claim(error) => write!(f, "cannot own the profile population: {error}"),
            Self::Catalog(error) => write!(f, "settings catalog is unusable: {error}"),
            Self::Field(error) => write!(f, "profile field is unusable: {error}"),
            Self::Library(error) => write!(f, "{error}"),
            Self::NoProfileSelected => write!(f, "no profile is selected"),
            Self::EmptySlot { profile } => {
                write!(f, "profile {profile} has no readable revision to select")
            }
            Self::ForeignProfile { expected, offered } => write!(
                f,
                "this session owns profile {expected}, so it will not write a \
                 document naming profile {offered}"
            ),
            Self::RevisionExhausted { profile } => {
                write!(
                    f,
                    "profile {profile} cannot be saved again: revision is exhausted"
                )
            }
            Self::Conflict {
                profile,
                stored,
                attempts,
            } => write!(
                f,
                "profile {profile} moved to revision {} while this session wrote; \
                 gave up after {attempts} attempts",
                stored.0
            ),
            Self::Refused(refusal) => write!(f, "{refusal}"),
        }
    }
}

impl std::error::Error for SessionError {}

impl From<LibraryOpenError> for SessionError {
    fn from(error: LibraryOpenError) -> Self {
        Self::Open(error)
    }
}

impl From<ClaimError> for SessionError {
    fn from(error: ClaimError) -> Self {
        Self::Claim(error)
    }
}

impl From<CatalogError> for SessionError {
    fn from(error: CatalogError) -> Self {
        Self::Catalog(error)
    }
}

impl From<ProfileFieldError> for SessionError {
    fn from(error: ProfileFieldError) -> Self {
        Self::Field(error)
    }
}

impl From<LibraryError> for SessionError {
    fn from(error: LibraryError) -> Self {
        Self::Library(error)
    }
}

impl From<ChangeRefusal> for SessionError {
    fn from(refusal: ChangeRefusal) -> Self {
        Self::Refused(refusal)
    }
}

/// One live session over one population, holding one selected profile.
///
/// This is the runtime wiring F48-C adds: what a run opens instead of a bare
/// [`ProfileLibrary`]. It owns the population exclusively ([`PopulationClaim`]),
/// keeps the selected profile's document and its resolved settings in memory,
/// and is the only thing that writes a revision — through
/// [`ProfileSession::commit_with`], which re-reads and re-applies on a
/// conflict instead of overwriting whatever moved.
///
/// ```no_run
/// use cs_app::profile::{ProfileSession, SessionOrigin};
/// use cs_content::save::settings::SettingCatalog;
/// use cs_types::profile::ProfileKind;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let mut session = ProfileSession::open(
///     std::path::Path::new("/tmp/user-data"),
///     SessionOrigin::Interactive,
///     ProfileKind::Production,
///     &SettingCatalog::empty(),
/// )?;
/// let profile = session.create("Second pilot")?;
/// let report = session.finish()?;
/// println!("{profile} discarded={}", report.uncommitted_changes);
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct ProfileSession {
    library: ProfileLibrary,
    claim: PopulationClaim,
    catalog: SettingCatalog,
    selected: Option<LoadedProfile>,
    settings: Option<SettingsState>,
    /// Settings changed since the last commit. The session never writes them
    /// without an explicit commit, so a session that ends early loses them and
    /// says so.
    uncommitted_settings: bool,
    /// Diagnostics from a profile the population's active pointer named that
    /// could not be selected at all. Text, because the caller shows it.
    open_failures: Vec<String>,
}

impl ProfileSession {
    /// Opens one population's library and claims it for this session.
    ///
    /// The registry is recovered on the way in, and every diagnostic it
    /// produced is kept in [`ProfileSession::warnings`] — a fallback to an
    /// older registry is something a caller shows, not something it logs. A
    /// population that already names a profile has its active profile selected;
    /// a population with none is opened with nothing selected, which
    /// [`ProfileSession::create`] then fills.
    pub fn open(
        base: &Path,
        origin: SessionOrigin,
        kind: ProfileKind,
        catalog: &SettingCatalog,
    ) -> Result<Self, SessionError> {
        let library = open_library(base, origin, kind)?;
        let claim = PopulationClaim::acquire(library.base())?;
        let mut session = Self {
            library,
            claim,
            catalog: catalog.clone(),
            selected: None,
            settings: None,
            uncommitted_settings: false,
            open_failures: Vec::new(),
        };
        // The active profile is selected from what is stored, not from the
        // registry alone. A profile whose slot cannot be read — a future
        // schema, a slot with no whole revision — does not stop the population
        // from opening: the caller is told which profile could not be selected
        // and nothing is selected, so another profile can be picked. Failing
        // the whole session would make one unreadable save hide every other
        // pilot in the tree.
        //
        // The pointer is *read*, never rewritten, on the way in. It already
        // names this profile, so writing it back would change nothing and cost
        // a full atomic registry revision on every launch — and would make a
        // read-only population (a mounted archive, a permissions problem)
        // impossible to open at all, which is exactly when a player most needs
        // to see the pilot they have.
        if let Some(active) = session.library.active() {
            match session.load_selected(active) {
                Ok(found) => session.install(found),
                Err(error) => session.open_failures.push(error.to_string()),
            }
        }
        Ok(session)
    }

    /// Opens the sandbox: an automated session over the synthetic population.
    ///
    /// A sandbox profile is never a player's live profile, so this is the only
    /// constructor an automated caller has, and it cannot be pointed at the
    /// production population at all — the population rule is applied inside
    /// [`open_library`], before any directory is touched.
    pub fn open_sandbox(base: &Path, catalog: &SettingCatalog) -> Result<Self, SessionError> {
        Self::open(
            base,
            SessionOrigin::Automated,
            ProfileKind::Synthetic,
            catalog,
        )
    }

    /// The population this session owns.
    pub fn library(&self) -> &ProfileLibrary {
        &self.library
    }

    /// The owned library, mutably.
    ///
    /// The session's own writes go through [`ProfileSession::commit`] and
    /// [`ProfileSession::commit_with`]; this is for a caller that must reach the
    /// library directly — the population rule and the revision check still
    /// apply, because this is the same object, not a second owner of it.
    pub fn library_mut(&mut self) -> &mut ProfileLibrary {
        &mut self.library
    }

    /// The claimed directory.
    pub fn directory(&self) -> &Path {
        self.claim.directory()
    }

    /// The population this session owns.
    pub const fn kind(&self) -> ProfileKind {
        self.library.kind()
    }

    /// The live profile ids, in allocation order.
    pub fn live(&self) -> &[ProfileId] {
        self.library.live()
    }

    /// The selected profile, if one is selected.
    pub fn selected(&self) -> Option<ProfileId> {
        self.selected.as_ref().map(|found| found.id)
    }

    /// The selected profile's newest whole revision, as this session loaded it.
    pub fn document(&self) -> Option<&ProfileDocument> {
        self.selected
            .as_ref()
            .and_then(|found| found.document.as_ref())
    }

    /// The selected profile's settings as this session resolved them.
    pub fn settings(&self) -> Option<&SettingsState> {
        self.settings.as_ref()
    }

    /// Whether settings have changed since the last commit.
    pub const fn has_uncommitted_settings(&self) -> bool {
        self.uncommitted_settings
    }

    /// The selected profile's campaign state, which is owned by the profile and
    /// not by the session.
    pub fn campaign(&self) -> Option<&CampaignState> {
        self.document().map(|document| &document.campaign)
    }

    /// Every diagnostic the open and the selected profile's recovery produced,
    /// as text a caller can show.
    pub fn warnings(&self) -> Vec<String> {
        let mut lines = self.library.status().warning_lines();
        lines.extend(self.open_failures.iter().cloned());
        if let Some(selected) = &self.selected {
            lines.extend(selected.warnings.iter().cloned());
        }
        if let Some(settings) = &self.settings {
            lines.extend(settings.refusal_lines());
        }
        lines
    }

    /// Creates a profile in this population, selects it, and returns its
    /// persistent id.
    ///
    /// The name is bounded before the library is asked to allocate anything, so
    /// a name a save could not hold never consumes an id. The id comes from the
    /// persisted high-water mark, and the document written carries that same id:
    /// it is the identity, not a label derived afterwards.
    ///
    /// The new profile becomes the persisted active pointer as well as this
    /// session's selection. A selection that lives only in memory is not a
    /// selection: the next session opens the population by reading that pointer,
    /// so a profile created but not pointed at would be a pilot the player never
    /// returns to.
    pub fn create(&mut self, display_name: &str) -> Result<ProfileId, SessionError> {
        validate_text("display_name", display_name, MAX_DISPLAY_NAME_BYTES, false)?;
        let kind = self.library.kind();
        // The library assigns the real id and revision; the document only has to
        // carry a valid non-zero placeholder until it does.
        let draft = ProfileDocument {
            kind,
            display_name: display_name.to_owned(),
            settings: self.settings_seed(),
            ..ProfileDocument::synthetic(
                ProfileId::new(1).expect("one is a non-zero id"),
                Revision(1),
            )
        };
        let (id, document) = self.library.create(draft)?;
        // The registry makes the first profile active on its own; a later one has
        // to be pointed at explicitly, or the pointer keeps naming the pilot
        // that was active before.
        if self.library.active() != Some(id) {
            self.library.set_active(id)?;
        }
        self.install(LoadedProfile {
            id,
            document: Some(document),
            source: Some(cs_content::save::store::SaveFile::Current),
            warnings: Vec::new(),
        });
        Ok(id)
    }

    /// Selects a live profile, replacing whatever was selected.
    ///
    /// The profile is re-read from disk rather than taken from the registry, so
    /// selecting a profile shows what is stored — including a recovered backup
    /// and its warnings — and never a stale copy this session happened to hold.
    pub fn select(&mut self, id: ProfileId) -> Result<(), SessionError> {
        let found = self.load_selected(id)?;
        // The pointer is written after the read succeeds, so a profile that
        // cannot be loaded is never made active.
        self.library.set_active(id)?;
        self.install(found);
        Ok(())
    }

    /// Reads one profile's newest whole revision and resolves its settings,
    /// without touching the active pointer.
    fn load_selected(&self, id: ProfileId) -> Result<LoadedProfile, SessionError> {
        let found = self.library.load(id)?;
        if found.document.is_none() {
            // A live profile with no readable revision cannot be played. It is
            // reported rather than presented as an empty pilot.
            return Err(SessionError::EmptySlot { profile: id });
        }
        Ok(found)
    }

    /// Adopts a freshly read profile as the session's selection.
    fn install(&mut self, found: LoadedProfile) {
        self.settings = found
            .document
            .as_ref()
            .map(|document| SettingsState::open(&self.catalog, &document.settings));
        self.uncommitted_settings = false;
        self.selected = Some(found);
    }

    /// Deletes a profile. If it was the selected one, the selection moves to
    /// the profile the library now has active, or to nothing.
    ///
    /// The high-water mark does not move, so the id is never issued again — and
    /// because the session forgets the deleted profile's settings with it, an id
    /// from before the deletion can never come to mean this session's state.
    pub fn delete(&mut self, id: ProfileId) -> Result<(), SessionError> {
        let was_selected = self.selected() == Some(id);
        self.library.delete(id)?;
        if was_selected {
            self.selected = None;
            self.settings = None;
            self.uncommitted_settings = false;
            if let Some(active) = self.library.active() {
                self.select(active)?;
            }
        }
        Ok(())
    }

    /// Changes one setting of the selected profile.
    ///
    /// The change is held in memory and takes effect now or after a restart
    /// according to the catalog's rule; it is written to disk only by
    /// [`ProfileSession::commit`] or [`ProfileSession::commit_with`]. A value
    /// the rule refuses stores nothing and reports the value that stays in
    /// force, which is the safe recovery path the spec requires for a setting
    /// that cannot be used.
    pub fn set_setting(&mut self, key: &str, value: &str) -> Result<SettingOutcome, SessionError> {
        let catalog = self.catalog.clone();
        let settings = self
            .settings
            .as_mut()
            .ok_or(SessionError::NoProfileSelected)?;
        let outcome = settings.set(&catalog, key, value)?;
        if matches!(outcome, SettingOutcome::Refused { .. }) {
            // A refused value changes nothing, so nothing is owed to the disk.
            return Ok(outcome);
        }
        self.uncommitted_settings = true;
        Ok(outcome)
    }

    /// Writes the selected profile's current in-memory state — its settings and
    /// its campaign state — as one whole revision.
    ///
    /// This is the plain commit, for a session that has already mutated its own
    /// in-memory document through [`ProfileSession::document_mut`]. A commit
    /// that finds a moved expected revision does **not** retry: it reports the
    /// conflict with the stored revision, because this session holds no record
    /// of what it meant to change and guessing would overwrite unrelated
    /// progression. [`ProfileSession::commit_with`] is the retrying form, for a
    /// change the session can state as a function.
    pub fn commit(&mut self) -> Result<Revision, SessionError> {
        let revision = self.write_once()?;
        self.uncommitted_settings = false;
        Ok(revision)
    }

    /// Applies `change` to the selected profile and writes the result as one
    /// whole revision, re-applying it to the stored revision if a concurrent
    /// writer moved it first.
    ///
    /// The retry is what makes this a transaction rather than a read-modify-
    /// write: `change` is a function of the document, so re-reading the stored
    /// revision and applying the same function to it produces the same intended
    /// result *on top of whatever else is now there*, instead of restoring an
    /// obsolete copy. After [`MAX_COMMIT_ATTEMPTS`] conflicts it reports the
    /// conflict and the stored revision rather than continuing.
    pub fn commit_with(
        &mut self,
        mut change: impl FnMut(&mut ProfileDocument) -> Result<(), ChangeRefusal>,
    ) -> Result<Revision, SessionError> {
        let id = self.selected().ok_or(SessionError::NoProfileSelected)?;
        let mut attempts = 0;
        loop {
            attempts += 1;
            match self.commit_once(id, &mut change) {
                Ok(revision) => return Ok(revision),
                Err(WriteAttempt::Moved { stored }) => {
                    // The contract's rule: a conflicting revision fails and
                    // refreshes the view. The view is refreshed, and the change
                    // is re-applied to it; unrelated progression is never
                    // overwritten because it is read, not replaced.
                    self.refresh(id)?;
                    if attempts >= MAX_COMMIT_ATTEMPTS {
                        return Err(SessionError::Conflict {
                            profile: id,
                            stored,
                            attempts,
                        });
                    }
                }
                Err(WriteAttempt::Failed(error)) => return Err(error),
            }
        }
    }

    /// Records that an outcome id has been applied to this profile's campaign.
    ///
    /// This is the idempotence half of the contract's outcome transaction: a
    /// replay after a crash before acknowledgment finds the id already applied
    /// and returns [`OutcomeRecord::AlreadyApplied`] without writing, so the
    /// reward cannot be paid twice. The reward *amount* is not decided here —
    /// progression and rewards are F43-B's; what this owns is that the applied
    /// list lives in the profile and is written atomically with it.
    ///
    /// The idempotence check happens inside the change, against the document the
    /// commit is about to write, so it holds after a conflict retry as well as
    /// on a first attempt: if a concurrent writer applied the same outcome id in
    /// the meantime, the retry sees it and reports
    /// [`OutcomeRecord::AlreadyApplied`] rather than an error, because a
    /// duplicate application is the safe outcome the contract asks for, not a
    /// failure. Nothing is written on that path.
    pub fn record_outcome(&mut self, outcome: &str) -> Result<OutcomeRecord, SessionError> {
        if let Some(state) = self.campaign()
            && state.applied_outcomes.iter().any(|held| held == outcome)
        {
            return Ok(OutcomeRecord::AlreadyApplied);
        }
        let subject = outcome.to_owned();
        match self.commit_with(move |document| {
            cs_types::profile::validate_key("campaign.outcome", &subject).map_err(|reason| {
                ChangeRefusal {
                    subject: subject.clone(),
                    reason: ChangeRefusalReason::Malformed(reason.to_string()),
                }
            })?;
            if document.campaign.applied_outcomes.contains(&subject) {
                return Err(ChangeRefusal {
                    subject: subject.clone(),
                    reason: ChangeRefusalReason::AlreadyApplied,
                });
            }
            document.campaign.applied_outcomes.push(subject.clone());
            Ok(())
        }) {
            Ok(_) => Ok(OutcomeRecord::Recorded),
            // A concurrent writer applied the same id between this session's read
            // and its write. The retry saw it and refused to apply it twice,
            // which is the contract's required outcome — reported as such rather
            // than as a failure, so a caller replaying after a crash does not
            // treat a correctly-suppressed double reward as an error. Nothing
            // was written on that path.
            Err(SessionError::Refused(refusal))
                if refusal.reason == ChangeRefusalReason::AlreadyApplied =>
            {
                Ok(OutcomeRecord::AlreadyApplied)
            }
            Err(error) => Err(error),
        }
    }

    /// Starts or continues a campaign run on this profile.
    pub fn begin_campaign_run(&mut self, run: &str) -> Result<Revision, SessionError> {
        let run = run.to_owned();
        self.commit_with(move |document| {
            cs_types::profile::validate_key("campaign.run", &run).map_err(|reason| {
                ChangeRefusal {
                    subject: run.clone(),
                    reason: ChangeRefusalReason::Malformed(reason.to_string()),
                }
            })?;
            document.campaign.run_id = Some(run.clone());
            Ok(())
        })
    }

    /// Ends the session: releases the population and reports what it ended
    /// with.
    ///
    /// Uncommitted settings are **dropped**, not written. A session that ends
    /// early therefore reports `uncommitted_changes: true` and the caller can
    /// decide whether that was acceptable; the alternative — committing on the
    /// way out — would turn a failed run into a partial save, which is what the
    /// contract's "Persistent profile data receives only an explicit outcome
    /// transaction" forbids.
    pub fn finish(self) -> Result<TeardownReport, SessionError> {
        Ok(TeardownReport {
            population: self.claim.directory().to_path_buf(),
            selected: self.selected.as_ref().map(|found| found.id),
            revision: self
                .selected
                .as_ref()
                .and_then(|found| found.document.as_ref())
                .map(|document| document.revision),
            uncommitted_changes: self.uncommitted_settings,
            warnings: self.warnings(),
        })
    }

    /// The settings entries a newly created profile starts with: the catalog's
    /// declared defaults, so a profile created by this build carries every
    /// setting it knows rather than an empty list another build must guess at.
    fn settings_seed(&self) -> Vec<cs_types::profile::SettingEntry> {
        SettingsState::open(&self.catalog, &[]).entries()
    }

    /// One write attempt. `Moved` carries the stored revision the retry must
    /// re-read; anything else is final.
    fn commit_once(
        &mut self,
        id: ProfileId,
        change: &mut impl FnMut(&mut ProfileDocument) -> Result<(), ChangeRefusal>,
    ) -> Result<Revision, WriteAttempt> {
        let base = self
            .document()
            .cloned()
            .ok_or(WriteAttempt::Failed(SessionError::NoProfileSelected))?;
        let next =
            base.revision
                .next()
                .ok_or(WriteAttempt::Failed(SessionError::RevisionExhausted {
                    profile: id,
                }))?;
        let mut draft = base;
        draft.revision = next;
        self.check_owns(&draft).map_err(WriteAttempt::from)?;
        change(&mut draft).map_err(WriteAttempt::from)?;
        // The session's resolved settings are what the disk must hold, so they
        // are written into the draft before the commit: a document that carried
        // a hand-edited setting list could otherwise reintroduce a value the
        // catalog refused.
        if let Some(settings) = &self.settings {
            draft.settings = settings.entries();
        }
        match self.library.save(&draft) {
            Ok(()) => {
                let found = self.selected.as_mut().expect("a selected profile");
                found.document = Some(draft);
                found.source = Some(cs_content::save::store::SaveFile::Current);
                found.warnings.clear();
                self.uncommitted_settings = false;
                Ok(next)
            }
            Err(LibraryError::Commit(CommitError::RevisionConflict { stored, .. })) => {
                Err(WriteAttempt::Moved { stored })
            }
            Err(error) => Err(WriteAttempt::Failed(SessionError::Library(error))),
        }
    }

    /// Re-reads the selected profile from disk and re-resolves its settings, so
    /// a retry applies to what is actually stored. A profile that has become
    /// unreadable in the meantime fails the session rather than continuing from
    /// a copy it knows is stale. The active pointer is left alone: it already
    /// names this profile.
    ///
    /// Only the *document* is refreshed. The session's own uncommitted settings
    /// are its own work, not a copy of what another writer holds, so they are
    /// carried across the refresh and re-applied on top of the freshly-read
    /// document — a concurrent writer's progression survives, and so does the
    /// change this session was asked to make.
    fn refresh(&mut self, id: ProfileId) -> Result<(), SessionError> {
        // The session's own uncommitted setting values, read before the refresh
        // replaces the state they live in.
        let pending: Vec<(String, String)> = self
            .settings
            .as_ref()
            .map(|settings| {
                settings
                    .changed()
                    .into_iter()
                    .map(|(key, value)| (key.to_owned(), value.to_owned()))
                    .collect()
            })
            .unwrap_or_default();
        let found = self.load_selected(id)?;
        self.install(found);
        if let Some(resolved) = self.settings.as_mut() {
            // Re-applied through the same rules a live change goes through, so a
            // pending value is validated and labeled exactly as it was the first
            // time and cannot slip past a rule it did not satisfy then.
            for (key, value) in &pending {
                let catalog = self.catalog.clone();
                let _ = resolved.set(&catalog, key, value);
            }
        }
        // `install` cleared the flag with the state it replaced; whether there is
        // still uncommitted work is now a fact about the re-applied values.
        self.uncommitted_settings = !pending.is_empty();
        Ok(())
    }

    /// The plain commit's single write.
    fn write_once(&mut self) -> Result<Revision, SessionError> {
        let id = self.selected().ok_or(SessionError::NoProfileSelected)?;
        let base = self
            .document()
            .cloned()
            .ok_or(SessionError::NoProfileSelected)?;
        let next = base
            .revision
            .next()
            .ok_or(SessionError::RevisionExhausted { profile: id })?;
        let mut draft = base;
        draft.revision = next;
        self.check_owns(&draft)?;
        if let Some(settings) = &self.settings {
            draft.settings = settings.entries();
        }
        self.library.save(&draft)?;
        let found = self.selected.as_mut().expect("a selected profile");
        found.document = Some(draft);
        found.source = Some(cs_content::save::store::SaveFile::Current);
        found.warnings.clear();
        Ok(next)
    }

    /// Refuses a draft that names a profile other than the one this session
    /// selected.
    ///
    /// A session owns exactly one profile, and the library writes a document
    /// into the slot its `profile_id` names. [`ProfileSession::document_mut`]
    /// hands out the whole in-memory document, so a caller that renames its
    /// `profile_id` — by accident or on purpose — would otherwise have this
    /// session write one pilot's entire state over another pilot's save, under
    /// a revision high enough to pass the conflict check. That is precisely the
    /// "an id never refers to a new profile" property this whole feature is
    /// about, violated by the write path, so it is checked on the way out.
    fn check_owns(&self, draft: &ProfileDocument) -> Result<(), SessionError> {
        let selected = self.selected().ok_or(SessionError::NoProfileSelected)?;
        if draft.profile_id == selected {
            Ok(())
        } else {
            Err(SessionError::ForeignProfile {
                expected: selected,
                offered: draft.profile_id,
            })
        }
    }
}

/// The result of one commit attempt's write.
enum WriteAttempt {
    /// Another writer moved the stored revision; re-read and try again.
    Moved { stored: Revision },
    /// Final failure.
    Failed(SessionError),
}

impl From<ChangeRefusal> for WriteAttempt {
    fn from(refusal: ChangeRefusal) -> Self {
        Self::Failed(SessionError::Refused(refusal))
    }
}

impl From<SessionError> for WriteAttempt {
    fn from(error: SessionError) -> Self {
        Self::Failed(error)
    }
}

/// What a session ended with. Produced by [`ProfileSession::finish`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TeardownReport {
    /// The population that was released.
    pub population: PathBuf,
    /// The profile that was selected, if any.
    pub selected: Option<ProfileId>,
    /// The revision that was last written for it.
    pub revision: Option<Revision>,
    /// Whether settings were changed and dropped rather than written.
    pub uncommitted_changes: bool,
    /// Every diagnostic the session held, as text a caller can show.
    pub warnings: Vec<String>,
}

/// Mutable access to the selected profile's in-memory document.
///
/// Present so a caller can change a field the session does not model; anything
/// written this way is committed by [`ProfileSession::commit`], which has no
/// change to re-apply on a retry. The session's resolved settings are written
/// into the draft on every commit either way.
impl ProfileSession {
    /// The selected profile's in-memory document, mutably.
    pub fn document_mut(&mut self) -> Result<&mut ProfileDocument, SessionError> {
        self.selected
            .as_mut()
            .and_then(|found| found.document.as_mut())
            .ok_or(SessionError::NoProfileSelected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusal_text_names_the_subject_and_the_reason() {
        let refusal = ChangeRefusal {
            subject: "m01.win".to_owned(),
            reason: ChangeRefusalReason::AlreadyApplied,
        };
        assert_eq!(
            refusal.to_string(),
            "m01.win was already applied to this profile"
        );
        let malformed = ChangeRefusal {
            subject: "bad key".to_owned(),
            reason: ChangeRefusalReason::Malformed("must not be empty".to_owned()),
        };
        assert!(
            malformed
                .to_string()
                .contains("not usable as an outcome key")
        );
    }

    #[test]
    fn an_exhausted_revision_and_a_conflict_are_told_apart() {
        let exhausted = SessionError::RevisionExhausted {
            profile: ProfileId::new(7).expect("nonzero"),
        };
        assert!(exhausted.to_string().contains("revision is exhausted"));
        let conflict = SessionError::Conflict {
            profile: ProfileId::new(7).expect("nonzero"),
            stored: Revision(4),
            attempts: MAX_COMMIT_ATTEMPTS,
        };
        assert!(conflict.to_string().contains("moved to revision 4"));
    }
}
