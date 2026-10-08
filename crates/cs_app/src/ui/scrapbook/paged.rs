//! The paged scrapbook screen and its replay launch (F47-C).
//!
//! Spec: `specs/F47-scrapbook-records-mementos-and-mission-replay.md`, stage
//! `### F47-C`. Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! [`ScrapbookUi`] is the screen half of the scrapbook: it slices the pages
//! [`project`] produces into a fixed number of slots per screen, keeps one
//! selection, and hands the selected entry to [`ScrapbookUi::launch`]. It is
//! wired to both ends of the feature:
//!
//! * **Producer.** [`ScrapbookUi::refresh_stored`] reads the records the
//!   profile session persisted (F47-B) and re-projects them;
//!   [`ScrapbookUi::refresh`] does the same for an already-loaded
//!   [`ScrapbookRecords`]. A profile that will not answer — nothing selected,
//!   or a `scrapbook.` field the reader refuses — propagates as
//!   [`PersistError`] and changes nothing.
//! * **Locale.** Titles are read through the one [`TextSession`] that owns the
//!   selected locale (F51-C), so the screen cannot resolve an id differently
//!   from the rest of the UI. Every refresh re-checks the ids the screen was
//!   showing with [`resolve_saved`] and reports the ones that no longer
//!   resolve, which is the AC03 check: a locale change moves text, never
//!   identity.
//! * **Consumer.** [`ScrapbookUi::launch`] turns the selection into a
//!   [`ReplayLaunch`]: the [`ReplayRequest`] from F47-A plus the validated
//!   [`MissionScope`] the normal loading path names the mission by, and
//!   [`ReplayLaunch::load_target`] composes the [`LoadTarget`] that request
//!   starts from.
//!
//! Teardown and retry are part of the contract: [`ScrapbookUi::close`] is
//! idempotent and leaves the screen empty, a refused launch changes nothing so
//! the same press can be retried once the producer records the fact that was
//! missing, and a refresh clamps a page index whose pages shrank instead of
//! showing a slice past the end.
//!
//! # Designed, not measured
//!
//! The original scrapbook's page size, layout, input bindings and the way it
//! names a mission variant are unmeasured (F47-A recorded the same boundary).
//! The page size here is caller-declared, and the launch maps the variant a
//! link names onto the mission scope the load runs under — both are new-engine
//! design recorded in `docs/findings/2026-10-08-f47-c-scrapbook-paged-ui-and-replay.md`.
//! Nothing here awards an unlock: the predicates are F47-A's and the facts are
//! the persisted ones.

use std::fmt;

use cs_content::localization::TextId;
use cs_content::scrapbook::ScrapbookCatalog;
use cs_sim::records::ScrapbookRecords;
use cs_types::asset_id::{LabelError, MissionScope, WorldGroup};
use cs_types::content::ContentId;

use crate::loading::LoadTarget;
use crate::profile::ProfileSession;
use crate::text::TextSession;

use super::{
    PageView, PersistError, ReplayRequest, ScrapbookActionError, project, replay_request,
    resolve_saved, stored,
};

/// Why a page move or a construction was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageError {
    /// The screen was asked to hold zero entries per page, which would make
    /// every page empty and the page count meaningless.
    ZeroPageSize,
    /// The screen is already on its first page.
    FirstPage,
    /// The requested page does not exist.
    OutOfRange {
        /// The page that was asked for (0-based).
        page: usize,
        /// How many pages the screen has.
        pages: usize,
    },
}

impl fmt::Display for PageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroPageSize => f.write_str("a scrapbook page must hold at least one entry"),
            Self::FirstPage => f.write_str("the scrapbook is already on its first page"),
            Self::OutOfRange { page, pages } => write!(
                f,
                "scrapbook page {page} does not exist; the screen has {pages} pages"
            ),
        }
    }
}

impl std::error::Error for PageError {}

/// Why a selection was refused; the screen is unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelectError {
    /// The screen is closed and holds nothing to select.
    Closed,
    /// The id is not one of the entries this screen shows. A hidden page, an
    /// unknown id and an id that only exists in another locale's catalog are
    /// all this: the caller cannot select what the player cannot see.
    NotVisible {
        /// The refused id.
        id: ContentId,
    },
}

impl fmt::Display for SelectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => f.write_str("the scrapbook screen is closed"),
            Self::NotVisible { id } => write!(f, "scrapbook entry {id} is not on this screen"),
        }
    }
}

impl std::error::Error for SelectError {}

/// Why a link's mission could not become the mission scope the loading path
/// loads under; the entry is named so the caller can report which link is at
/// fault.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScopeFailure {
    /// The entry whose link was refused.
    pub entry: ContentId,
    /// The mission id that did not spell a scope label.
    pub mission: ContentId,
    /// What the scope validator said.
    pub source: LabelError,
}

/// Why a replay could not be launched; the screen keeps its selection, so the
/// same press is a retry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaunchError {
    /// The screen is closed: teardown happened, so there is nothing to launch.
    Closed,
    /// Nothing is selected.
    NoSelection,
    /// The entry exists but its replay link refused the request (locked,
    /// unknown or link-less).
    Refused(ScrapbookActionError),
    /// The link names a mission whose key is not a valid mission scope label,
    /// so the loading path could not name what to load.
    ///
    /// Boxed because two content ids plus the validator's error would
    /// otherwise be the largest error this crate returns (clippy's
    /// `result_large_err`); the payload is identical either way.
    Scope(Box<ScopeFailure>),
}

impl fmt::Display for LaunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => f.write_str("the scrapbook screen is closed"),
            Self::NoSelection => f.write_str("no scrapbook entry is selected"),
            Self::Refused(error) => error.fmt(f),
            Self::Scope(failure) => write!(
                f,
                "{}: replay mission {} has no scope: {}",
                failure.entry, failure.mission, failure.source
            ),
        }
    }
}

impl std::error::Error for LaunchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Refused(error) => Some(error),
            Self::Scope(failure) => Some(&failure.source),
            Self::Closed | Self::NoSelection => None,
        }
    }
}

/// What one [`ScrapbookUi::refresh`] found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refresh {
    /// How many pages the screen now has.
    pub pages: usize,
    /// Ids that were on screen before the refresh and no longer resolve in the
    /// catalog. Identity is locale-free, so this list must stay empty across a
    /// locale change; a non-empty list means the catalog lost an entry.
    pub unresolved: Vec<ContentId>,
    /// The selection after the refresh: the same entry, still visible.
    pub selection: Option<ContentId>,
    /// The selection that was dropped because it is no longer one of the
    /// entries on screen. It is `None` when the selection survived or when
    /// there was nothing to drop — a lost id never silently points at another
    /// entry.
    pub dropped: Option<ContentId>,
}

/// One selected, unlocked entry and the loading path it replays through.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayLaunch {
    /// The entry the launch came from.
    pub entry: ContentId,
    /// The mission and variant the entry links to.
    pub request: ReplayRequest,
    /// The validated mission scope the load runs under: the mission-kind
    /// element the link actually launches, which is the variant when the link
    /// names one. How the original distinguishes variants is unmeasured
    /// (F47-A), so this mapping is designed and is audited by F47-D.
    pub scope: MissionScope,
}

impl ReplayLaunch {
    /// The load target this replay hands the normal loading path: the world
    /// group is the application's declaration (the dependency closure and its
    /// target are app-declared, `crate::ui::front_end::LoadPlan`), and the
    /// mission scope is this launch's own.
    #[must_use]
    pub fn load_target(&self, world: WorldGroup) -> LoadTarget {
        LoadTarget::world(world).with_mission(self.scope.clone())
    }
}

/// The titles the projection resolves, in the text session's current locale.
///
/// An id the session's chain does not answer stays `None`: a missing
/// translation is reported, never invented (F51).
fn title_resolver(texts: &TextSession) -> impl Fn(&ContentId) -> Option<String> + '_ {
    move |id| {
        let text = TextId::try_from_content(id.clone()).ok()?;
        texts
            .catalog()
            .resolve(&text, texts.chain())
            .row()
            .map(|row| row.text().to_owned())
    }
}

/// The paged scrapbook screen: pages of a fixed size, one selection, the
/// locale's titles, and the replay the selection offers.
///
/// The screen owns presentation state only — no unlock, no record and no
/// profile data. Every fact it shows comes from the [`ScrapbookRecords`] the
/// caller passes in, so nothing here can award an entry by itself.
///
/// [`project`] has already dropped the entries that are hidden until they
/// unlock, so `saved` and `pages` together are exactly what the player can
/// reach: an id that is not there cannot be selected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScrapbookUi {
    page_size: usize,
    pages: Vec<PageView>,
    page: usize,
    selection: Option<ContentId>,
    saved: Vec<ContentId>,
    open: bool,
}

impl ScrapbookUi {
    /// An empty screen that fits `page_size` entries on a page.
    ///
    /// # Errors
    ///
    /// [`PageError::ZeroPageSize`]: a page must hold at least one entry.
    pub fn new(page_size: usize) -> Result<Self, PageError> {
        if page_size == 0 {
            return Err(PageError::ZeroPageSize);
        }
        Ok(Self {
            page_size,
            pages: Vec::new(),
            page: 0,
            selection: None,
            saved: Vec::new(),
            open: false,
        })
    }

    /// Carries the ids a stored screen state held, so the next refresh can
    /// check them against the catalog instead of assuming they are still
    /// there.
    ///
    /// Until that refresh runs this is the pending state, not the projection:
    /// [`Self::saved_ids`] answers the restored list while the pages still
    /// show what the last refresh built, which is why [`Self::select`] reads
    /// the projection.
    pub fn restore(&mut self, saved: &[ContentId]) {
        self.saved = saved.to_vec();
    }

    /// The ids the screen holds as its state, in order: right after a refresh
    /// that is the projection, right after a [`Self::restore`] the restored
    /// list awaiting that refresh.
    #[must_use]
    pub fn saved_ids(&self) -> &[ContentId] {
        &self.saved
    }

    /// Whether the screen is open (a refresh opens it, a close shuts it).
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Rebuilds every page from `records`, with the titles the text session's
    /// locale answers, and re-checks the ids the screen was showing.
    ///
    /// This opens a closed screen. The page index is clamped to the pages that
    /// remain, and a selection that is no longer visible is dropped and
    /// reported rather than moved to another entry.
    pub fn refresh(
        &mut self,
        catalog: &ScrapbookCatalog,
        records: &ScrapbookRecords,
        texts: &TextSession,
    ) -> Refresh {
        let previous = std::mem::take(&mut self.saved);
        let unresolved = resolve_saved(catalog, &previous)
            .into_iter()
            .zip(previous)
            .filter_map(|(declared, id)| declared.is_none().then_some(id))
            .collect();
        self.pages = project(catalog, records, &title_resolver(texts));
        self.saved = self.pages.iter().map(|page| page.id.clone()).collect();
        let pages = self.page_count();
        self.page = self.page.min(pages.saturating_sub(1));
        self.open = true;
        let dropped = match self.selection.as_ref() {
            Some(id) if !self.saved.contains(id) => self.selection.take(),
            _ => None,
        };
        Refresh {
            pages,
            unresolved,
            selection: self.selection.clone(),
            dropped,
        }
    }

    /// The producer-facing refresh: the records are the ones the selected
    /// profile persisted (F47-B).
    ///
    /// # Errors
    ///
    /// [`PersistError`] when the profile will not answer; the screen keeps
    /// whatever it was showing.
    pub fn refresh_stored(
        &mut self,
        catalog: &ScrapbookCatalog,
        profile: &ProfileSession,
        texts: &TextSession,
    ) -> Result<Refresh, PersistError> {
        let records = stored(profile)?;
        Ok(self.refresh(catalog, &records, texts))
    }

    /// How many pages the screen has (0 when it shows nothing).
    #[must_use]
    pub fn page_count(&self) -> usize {
        self.pages.len().div_ceil(self.page_size)
    }

    /// The 0-based index of the page on screen.
    #[must_use]
    pub fn page_index(&self) -> usize {
        self.page
    }

    /// The entries on screen, in declared order.
    ///
    /// The last page is usually short: it carries the entries that are left,
    /// never padded and never a slice past the end.
    #[must_use]
    pub fn page(&self) -> &[PageView] {
        let start = self.page * self.page_size;
        let rest = self.pages.get(start..).unwrap_or(&[]);
        &rest[..rest.len().min(self.page_size)]
    }

    /// Moves to `page` (0-based).
    ///
    /// # Errors
    ///
    /// [`PageError::OutOfRange`]; the screen stays where it was.
    pub fn go_to_page(&mut self, page: usize) -> Result<(), PageError> {
        let pages = self.page_count();
        if page >= pages {
            return Err(PageError::OutOfRange { page, pages });
        }
        self.page = page;
        Ok(())
    }

    /// Moves to the next page.
    ///
    /// # Errors
    ///
    /// [`PageError::OutOfRange`] on the last page.
    pub fn next_page(&mut self) -> Result<(), PageError> {
        self.go_to_page(self.page + 1)
    }

    /// Moves to the previous page.
    ///
    /// # Errors
    ///
    /// [`PageError::FirstPage`] on the first page.
    pub fn prev_page(&mut self) -> Result<(), PageError> {
        if self.page == 0 {
            return Err(PageError::FirstPage);
        }
        self.go_to_page(self.page - 1)
    }

    /// Selects one of the entries the screen shows, wherever it is on which
    /// page. Selecting never moves the page: the selection is a stable id and
    /// the screen stays on the page it was left on.
    ///
    /// Selecting a locked page is allowed — the player can look at it — and
    /// the launch is what refuses it.
    ///
    /// The check is against the projection itself, not against
    /// [`Self::saved_ids`]: a stored screen state carried by
    /// [`Self::restore`] since the last refresh names ids the screen is not
    /// showing yet, and selecting one would leave [`Self::selected_id`] naming
    /// an entry [`Self::selected`] cannot produce.
    ///
    /// # Errors
    ///
    /// [`SelectError`]; the selection is unchanged.
    pub fn select(&mut self, id: &ContentId) -> Result<(), SelectError> {
        if !self.open {
            return Err(SelectError::Closed);
        }
        if !self.pages.iter().any(|page| &page.id == id) {
            return Err(SelectError::NotVisible { id: id.clone() });
        }
        self.selection = Some(id.clone());
        Ok(())
    }

    /// The selected entry as the screen shows it.
    #[must_use]
    pub fn selected(&self) -> Option<&PageView> {
        let id = self.selection.as_ref()?;
        self.pages.iter().find(|page| &page.id == id)
    }

    /// The selected entry's stable id.
    #[must_use]
    pub fn selected_id(&self) -> Option<&ContentId> {
        self.selection.as_ref()
    }

    /// Launches the selected entry's replay through the normal loading path.
    ///
    /// Nothing is mutated: a refusal leaves the selection and the pages
    /// exactly as they were, so pressing again is a retry once the producer
    /// records the fact that was missing.
    ///
    /// # Errors
    ///
    /// [`LaunchError`].
    pub fn launch(
        &self,
        catalog: &ScrapbookCatalog,
        records: &ScrapbookRecords,
    ) -> Result<ReplayLaunch, LaunchError> {
        if !self.open {
            return Err(LaunchError::Closed);
        }
        let entry = self.selection.clone().ok_or(LaunchError::NoSelection)?;
        let request = replay_request(catalog, records, &entry).map_err(LaunchError::Refused)?;
        let mission = request
            .variant
            .clone()
            .unwrap_or_else(|| request.mission.clone());
        let scope = MissionScope::new(mission.key()).map_err(|source| {
            LaunchError::Scope(Box::new(ScopeFailure {
                entry: entry.clone(),
                mission: mission.clone(),
                source,
            }))
        })?;
        Ok(ReplayLaunch {
            entry,
            request,
            scope,
        })
    }

    /// Tears the screen down: no pages, no selection, no saved ids. Calling it
    /// twice is the same as calling it once, and a later [`refresh`] reopens
    /// the screen from the producer's records.
    pub fn close(&mut self) {
        self.pages.clear();
        self.saved.clear();
        self.page = 0;
        self.selection = None;
        self.open = false;
    }
}
