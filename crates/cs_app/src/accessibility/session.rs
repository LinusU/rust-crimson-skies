//! The live settings session: safe recovery, labelled control profiles and
//! the one projection every consumer reads (F52-C).
//!
//! Spec: `specs/F52-accessibility-and-explicitly-separated-modern-options.md`,
//! stage `### F52-C`. Shared contract: `docs/contracts/UI-NETWORK.md`.
//!
//! F52-A built the pieces (the atomic file, the never-failing safe-defaults
//! startup, the presentation/gameplay split) and F52-B read the objectives
//! page through them. This module is where the application holds them *live*:
//! one [`SettingsSession`] owns the file, the current settings and the reason
//! the file was or was not usable, and every consumer reads from that session
//! instead of from a [`Settings`] value of its own:
//!
//! * **Safe recovery** (non-negotiable behavior 5).
//!   [`SettingsSession::open`] never fails and never writes: no file starts
//!   from the designed settings, an unusable file is reported by
//!   [`SettingsSession::recovery`] and left untouched on disk, and
//!   `safe_defaults` skips reading it at all. The report stays with the
//!   session, so the front end can tell the player *why* their settings were
//!   not used instead of silently presenting defaults.
//! * **Apply, retry, teardown.** [`SettingsSession::apply`] validates before
//!   anything moves, then persists atomically. A change that cannot be
//!   written is kept and reported ([`ApplyError::Persist`]), never dropped:
//!   [`SettingsSession::retry`] and [`SettingsSession::teardown`] flush it
//!   again, a failed teardown leaves the session open so the caller *can*
//!   retry, and only a successful teardown closes it — after which every
//!   write is refused with [`ApplyError::Closed`].
//! * **The consumers.** [`SettingsSession::present`] is the reduced-motion
//!   filter, [`SettingsSession::page`] the objectives page at this session's
//!   UI scale and colour filter, [`SettingsSession::gameplay_inputs`] the only
//!   view the simulation may read, [`SettingsSession::control_profile`] the
//!   labelled control profile, and [`SettingsSession::project`] hands the
//!   gameplay inputs, the filtered effects and the fidelity label to one
//!   frame together — so a consumer can never pair this frame's presentation
//!   with another setting's gameplay (AC03, AC04).
//!
//! Nothing here draws, plays or flies: it owns the settings and the seam.
//! What still has to reach the renderer, the audio mixer, the CLI
//! (`--safe-settings`) and the flight input through this session is recorded
//! in `docs/findings/2026-10-08-f52-c-settings-session-integration.md`.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use cs_content::settings::{
    FidelityLabel, GameplayInputs, ModernAssist, Presentation, ProfileKind, Settings, SettingsError,
};

use super::motion::{Effect, filter_effects};
use super::objective_page::{self, ObjectivePage};
use super::store::{self, LoadError, StartupOrigin};
use crate::ui::hud::PageView;

/// Why a settings change did not land.
///
/// Every variant says what state the session and the file are in: nothing is
/// ever reported as applied when it was not, and no failed change is dropped.
#[derive(Debug)]
pub enum ApplyError {
    /// The session was torn down; no change was made.
    Closed,
    /// The change itself is not valid settings; the session and the file are
    /// untouched.
    Invalid(SettingsError),
    /// The change is **live in the session** but could not be written; it
    /// stays staged for [`SettingsSession::retry`] and
    /// [`SettingsSession::teardown`], and the file still holds its previous
    /// contents (the write is atomic).
    Persist(io::Error),
}

impl fmt::Display for ApplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => f.write_str("the settings session is closed; no change was made"),
            Self::Invalid(error) => write!(f, "the settings change was refused: {error}"),
            Self::Persist(error) => {
                write!(f, "the change is live but could not be saved: {error}")
            }
        }
    }
}

impl std::error::Error for ApplyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Closed => None,
            Self::Invalid(error) => Some(error),
            Self::Persist(error) => Some(error),
        }
    }
}

/// The control profile in force, with the label comparison and replay
/// metadata must record beside it (AC04).
///
/// It is built in one place from one reading of the settings, so the assists
/// a consumer acts on and the assists a record names cannot disagree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlProfile {
    /// Which rule set is active.
    pub kind: ProfileKind,
    /// The assists actually in force; empty under the original rules.
    pub assists: Vec<ModernAssist>,
    /// The label the record carries.
    pub label: FidelityLabel,
}

/// One frame handed to its three consumers together.
///
/// [`SettingsSession::project`] builds it from a single reading of the
/// session, so the gameplay value, the presented effects and the fidelity
/// label always describe the same settings.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectedFrame<G> {
    /// What the simulation produced for this frame, measured under the
    /// label in [`Self::fidelity`].
    pub gameplay: G,
    /// The effects the presentation layer may show this frame.
    pub presented: Vec<Effect>,
    /// The metadata comparison and replay records carry.
    pub fidelity: Vec<(String, String)>,
}

/// The application's live settings: the file, the current values, why the
/// file was (or was not) usable, and what is still waiting to be written.
///
/// Consumers read through this type — [`Self::present`], [`Self::page`],
/// [`Self::gameplay_inputs`], [`Self::control_profile`] and [`Self::project`]
/// — never through a detached [`Settings`] copy, so a change the player makes
/// reaches them on the next read and a change that failed to save is visible
/// as such instead of looking persisted.
#[derive(Debug)]
pub struct SettingsSession {
    path: PathBuf,
    settings: Settings,
    origin: StartupOrigin,
    /// Whether the settings on disk are behind [`Self::settings`].
    staged: bool,
    closed: bool,
}

impl SettingsSession {
    /// Opens the session at `path`.
    ///
    /// Never fails and never writes: [`store::startup`] decides between the
    /// file, the designed defaults and the safe flag, and the reason is kept
    /// for [`Self::origin`] / [`Self::recovery`]. An unusable file is left on
    /// disk exactly as it was; only a later successful
    /// [`Self::apply`] replaces it.
    #[must_use]
    pub fn open(path: impl AsRef<Path>, safe_defaults: bool) -> Self {
        let path = path.as_ref();
        let started = store::startup(path, safe_defaults);
        Self {
            path: path.to_path_buf(),
            settings: started.settings,
            origin: started.origin,
            staged: false,
            closed: false,
        }
    }

    /// The file this session persists to.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Where the startup settings came from.
    #[must_use]
    pub const fn origin(&self) -> &StartupOrigin {
        &self.origin
    }

    /// Why the file could not be used, when it could not.
    ///
    /// This is the recovery report a player-facing screen shows: the session
    /// keeps the [`LoadError`] instead of swallowing it, and the file itself
    /// is never overwritten just because it was unreadable.
    #[must_use]
    pub fn recovery(&self) -> Option<&LoadError> {
        match &self.origin {
            StartupOrigin::Recovered(error) => Some(error),
            _ => None,
        }
    }

    /// The current settings.
    #[must_use]
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Whether a change is live here but not yet on disk.
    #[must_use]
    pub const fn is_staged(&self) -> bool {
        self.staged
    }

    /// Whether the session has been torn down (writes refused, reads fine).
    #[must_use]
    pub const fn is_closed(&self) -> bool {
        self.closed
    }

    /// Validates and applies `change`, then persists it atomically.
    ///
    /// The change is computed from the current settings and validated before
    /// anything moves, so [`ApplyError::Invalid`] leaves the session and the
    /// file exactly as they were. A change that validates but cannot be
    /// written becomes live in the session and is reported as
    /// [`ApplyError::Persist`] with [`Self::is_staged`] set: the caller sees
    /// the failure, the file keeps its previous contents, and the change is
    /// still there for [`Self::retry`] or [`Self::teardown`] rather than
    /// being silently dropped.
    ///
    /// # Errors
    ///
    /// [`ApplyError::Closed`], [`ApplyError::Invalid`] or
    /// [`ApplyError::Persist`], as documented.
    pub fn apply(&mut self, change: impl FnOnce(&mut Settings)) -> Result<(), ApplyError> {
        if self.closed {
            return Err(ApplyError::Closed);
        }
        let mut next = self.settings.clone();
        change(&mut next);
        next.validate().map_err(ApplyError::Invalid)?;
        self.settings = next;
        self.staged = true;
        self.persist()
    }

    /// Writes a staged change again; `Ok(())` when nothing is staged.
    ///
    /// # Errors
    ///
    /// [`ApplyError::Persist`] while the cause is still there — the change
    /// stays staged, so nothing is lost between attempts — and
    /// [`ApplyError::Closed`] after a successful teardown.
    pub fn retry(&mut self) -> Result<(), ApplyError> {
        if self.closed {
            return Err(ApplyError::Closed);
        }
        self.persist()
    }

    /// Flushes a staged change and closes the session.
    ///
    /// A failure is reported and the session stays **open**: the change is
    /// still staged, so the caller can fix the cause and try again instead of
    /// losing the player's settings on the way out. Only a successful flush
    /// closes it, after which [`Self::apply`] reports [`ApplyError::Closed`].
    /// Tearing down twice is not an error.
    ///
    /// # Errors
    ///
    /// [`ApplyError::Persist`] for a change that could not be written.
    pub fn teardown(&mut self) -> Result<(), ApplyError> {
        if self.closed {
            return Ok(());
        }
        self.persist()?;
        self.closed = true;
        Ok(())
    }

    fn persist(&mut self) -> Result<(), ApplyError> {
        if !self.staged {
            return Ok(());
        }
        store::save_atomic(&self.path, &self.settings).map_err(ApplyError::Persist)?;
        self.staged = false;
        Ok(())
    }

    /// The presentation settings every presentation consumer reads.
    #[must_use]
    pub const fn presentation(&self) -> &Presentation {
        &self.settings.presentation
    }

    /// The only view of the settings the simulation and the evidence runs
    /// may read (AC03): built from the modern profile alone, so no
    /// presentation setting can reach it.
    #[must_use]
    pub fn gameplay_inputs(&self) -> GameplayInputs {
        self.settings.gameplay_inputs()
    }

    /// The label comparison and replay metadata record (AC04).
    #[must_use]
    pub fn fidelity(&self) -> FidelityLabel {
        self.settings.fidelity()
    }

    /// The control profile in force with its label, built from one reading.
    #[must_use]
    pub fn control_profile(&self) -> ControlProfile {
        let assists = self.settings.effective_assists();
        ControlProfile {
            kind: self.settings.profile,
            label: FidelityLabel {
                assists: assists.clone(),
            },
            assists,
        }
    }

    /// The effects this session lets the presentation layer show.
    ///
    /// The consumer of [`super::motion::filter_effects`]: required damage and
    /// target notifications always pass, cosmetic shake and flash pass only
    /// while their switch is on.
    #[must_use]
    pub fn present(&self, effects: &[Effect]) -> Vec<Effect> {
        filter_effects(self.presentation(), effects)
    }

    /// The objectives page this session's UI scale and colour filter ask for;
    /// `None` for a view that is not the objectives page.
    #[must_use]
    pub fn page(&self, view: &PageView, viewport_px: u32) -> Option<ObjectivePage> {
        objective_page::from_view(view, self.presentation(), viewport_px)
    }

    /// Projects one frame to its three consumers from a single reading.
    ///
    /// `gameplay` receives the simulation's own view of the settings
    /// ([`Self::gameplay_inputs`]) and produces whatever the run measures;
    /// the frame carries it beside the filtered effects and the fidelity
    /// metadata, so a record cannot claim one profile while the frame showed
    /// another (AC03, AC04).
    #[must_use]
    pub fn project<G>(
        &self,
        effects: &[Effect],
        gameplay: impl FnOnce(&GameplayInputs) -> G,
    ) -> ProjectedFrame<G> {
        ProjectedFrame {
            gameplay: gameplay(&self.settings.gameplay_inputs()),
            presented: self.present(effects),
            fidelity: self.settings.fidelity().metadata(),
        }
    }
}
