//! Acceptance stage F47-C: the paged scrapbook screen and its replay launch
//! (`specs/F47-scrapbook-records-mementos-and-mission-replay.md`, `### F47-C`).
//!
//! The minimum scenario is AC03: change the locale and every saved entry id
//! still resolves — identity is locale-free while the text moves. The rest
//! covers the paged walk of the visible pages, the launch into the loading
//! path's own `LoadTarget`, every refusal (nothing selected, locked, link-less,
//! a mission key that is no scope label, a closed screen) together with the
//! retry each one leaves open, the teardown, and the persisted producer behind
//! `refresh_stored`.
//!
//! Every value here is authored synthetic data: this proves the screen and its
//! wiring, never an original scrapbook rule, page size or layout (F47-D).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cs_app::profile::{ProfileSession, SessionError};
use cs_app::text::{
    FontMeasurer, FontSet, TextMetrics, TextSession, TextSessionError, synthetic_monospace,
};
use cs_app::ui::scrapbook::{
    LaunchError, PageError, PageView, PersistError, ScrapbookUi, SelectError, persist_stunt,
    record_mission, record_stunt, resolve_saved,
};
use cs_content::localization::{
    FontCatalog, FontFace, LocaleChain, LocaleId, LocalizedText, TextCatalog, TextId,
    synthetic_font_face, synthetic_markup_grammar,
};
use cs_content::save::settings::SettingCatalog;
use cs_sim::records::ScrapbookRecords;
use cs_types::asset_id::WorldGroup;
use cs_types::content::Origin;
use cs_types::profile::ExtraField;

use crate::*;

// ------------------------------------------------------------ fixtures ---

/// The claim every designed value in this file is recorded under.
fn claim() -> ClaimId {
    ClaimId::new("f47c.paged-screen-test").expect("claim id")
}

fn locale(label: &str) -> LocaleId {
    LocaleId::new(label).expect("the fixture locale is a valid label")
}

/// A one-locale chain: the selected locale, with no fallback behind it.
fn chain(selected: &str) -> LocaleChain {
    LocaleChain::new(locale(selected), std::iter::empty::<LocaleId>())
        .expect("the fixture chain is valid")
}

/// One title row per entry of `catalog`, in `locale_label`, text spelled
/// `<prefix> <entry key>` so a test can tell which locale answered.
fn titles_for(catalog: &ScrapbookCatalog, locale_label: &str, prefix: &str) -> TextCatalog {
    let mut texts = TextCatalog::new();
    for declared in catalog.entries() {
        let key = declared.id.key();
        let title = TextId::new(&format!("{key}-title")).expect("the title id is valid");
        texts
            .insert(LocalizedText::new(
                title,
                locale(locale_label),
                format!("{prefix} {key}"),
                Origin::SyntheticFixture,
                Provenance::designed(claim()),
            ))
            .expect("one row per entry and locale");
    }
    texts
}

/// A measurer that stands in for the unmeasured original font format: the
/// synthetic face's declared monospace metrics.
struct Measurer;

impl FontMeasurer for Measurer {
    fn measure(&self, _face: &FontFace) -> Result<TextMetrics, String> {
        Ok(synthetic_monospace(16.0))
    }
}

/// A text session in `en-us` over `texts`, with the synthetic face loaded.
fn session(texts: TextCatalog) -> TextSession {
    let mut faces = FontCatalog::new();
    faces
        .insert(synthetic_font_face())
        .expect("the fixture face is inserted once");
    let fonts = FontSet::load(&faces, &Measurer).expect("the synthetic face is measurable");
    TextSession::try_new(
        chain("en-us"),
        texts,
        synthetic_markup_grammar(),
        fonts,
        synthetic_font_face().id().clone(),
    )
    .expect("the session's default font is loaded")
}

/// Stunts `a` and `b` completed and `m1` succeeded: `page-m2` is still hidden.
fn some_records() -> ScrapbookRecords {
    let mut records = ScrapbookRecords::default();
    record_stunt(&mut records, &stunt("a"));
    record_stunt(&mut records, &stunt("b"));
    record_mission(
        &mut records,
        &result(1, mission("m1"), Outcome::Succeeded, 5),
    )
    .expect("m1");
    records
}

fn ids(pages: &[PageView]) -> Vec<String> {
    pages.iter().map(|page| page.id.key().to_owned()).collect()
}

fn saved_keys(ui: &ScrapbookUi) -> Vec<String> {
    ui.saved_ids()
        .iter()
        .map(|id| id.key().to_owned())
        .collect()
}

fn titles(ui: &ScrapbookUi) -> Vec<Option<&str>> {
    ui.page().iter().map(|page| page.title.as_deref()).collect()
}

/// An entry the fixture catalog's own `entry` builds, shown before it unlocks
/// and linked to a mission (or not).
fn linked(key: &str, unlock: Resolved<Unlock>, replay: Option<ReplayLink>) -> ScrapbookEntry {
    let mut declared = entry(key, EntryKind::Page, unlock);
    declared.visibility = EntryVisibility::Shown;
    declared.replay = replay;
    declared
}

/// A temporary directory that cleans up after itself; no original data.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-f47-c-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("fixture base");
        Self(root)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// ---------------------------------------------------------------- tests ---

#[test]
fn accept_f47_c_paged_screen_walks_the_visible_pages_in_order() {
    // A page must hold at least one entry: a zero-sized page would make every
    // page empty and the page count meaningless.
    assert!(matches!(ScrapbookUi::new(0), Err(PageError::ZeroPageSize)));

    let catalog = catalog();
    let records = some_records();
    let texts = session(titles_for(&catalog, "en-us", "en"));
    let mut ui = ScrapbookUi::new(2).expect("a page of two entries");

    let opened = ui.refresh(&catalog, &records, &texts);
    assert_eq!(opened.pages, 3);
    assert!(opened.unresolved.is_empty());
    assert_eq!(opened.dropped, None);
    assert_eq!(ui.page_index(), 0);
    assert_eq!(ids(ui.page()), ["intro", "page-m1"]);
    // The stunt photos and the memento unlocked, but `page-m2`'s mission never
    // happened: the projection never lists a hidden page, so it cannot be
    // paged to or selected rather than being shown while locked.
    assert!(!ui.saved_ids().contains(&item("page-m2")));

    ui.next_page().expect("the second page exists");
    assert_eq!(ids(ui.page()), ["photo-a", "photo-b"]);
    ui.next_page().expect("the third page exists");
    assert_eq!(ids(ui.page()), ["memento-m1"]);
    assert_eq!(ui.page_index(), 2);
    assert!(matches!(
        ui.next_page(),
        Err(PageError::OutOfRange { page: 3, pages: 3 })
    ));
    assert_eq!(ui.page_index(), 2, "a refused move changes nothing");

    ui.prev_page().expect("back to the second page");
    assert_eq!(ui.page_index(), 1);
    ui.go_to_page(0).expect("back to the first page");
    assert!(matches!(ui.prev_page(), Err(PageError::FirstPage)));
    assert!(matches!(
        ui.go_to_page(3),
        Err(PageError::OutOfRange { page: 3, pages: 3 })
    ));

    // Selection is by stable id, never by position on the page, and it does
    // not move the page: the screen keeps showing the page it was left on.
    ui.go_to_page(1).expect("back to the second page");
    ui.select(&item("photo-b")).expect("it is on this page");
    assert_eq!(
        ui.selected().map(|page| page.id.clone()),
        Some(item("photo-b"))
    );
    assert_eq!(ui.page_index(), 1, "selecting does not move the page");
    ui.select(&item("intro")).expect("it is on the first page");
    assert_eq!(ui.page_index(), 1, "an id from another page moves nothing");
    // A hidden page is not on screen, so it cannot be selected — and neither
    // can an id the catalog never declared.
    assert!(matches!(
        ui.select(&item("page-m2")),
        Err(SelectError::NotVisible { .. })
    ));
    assert!(matches!(
        ui.select(&item("no-such-page")),
        Err(SelectError::NotVisible { .. })
    ));
    assert_eq!(
        ui.selected_id(),
        Some(&item("intro")),
        "a refused selection changes nothing"
    );
}

#[test]
fn accept_f47_c_locale_change_keeps_saved_entry_ids_resolving() {
    let catalog = catalog();
    let records = some_records();
    let mut texts = session(titles_for(&catalog, "en-us", "en"));
    let mut ui = ScrapbookUi::new(2).expect("a page of two entries");
    ui.refresh(&catalog, &records, &texts);

    let saved: Vec<ContentId> = ui.saved_ids().to_vec();
    assert_eq!(
        saved_keys(&ui),
        ["intro", "page-m1", "photo-a", "photo-b", "memento-m1"]
    );
    ui.select(&item("page-m1")).expect("it is on screen");
    ui.next_page().expect("the second page");
    assert_eq!(titles(&ui), [Some("en photo-a"), Some("en photo-b")]);

    // The player switches locale. Only the text may move.
    texts
        .switch_locale(chain("sv-se"), titles_for(&catalog, "sv-se", "sv"))
        .expect("the Swedish rows answer the chain");
    let after = ui.refresh(&catalog, &records, &texts);
    assert!(
        after.unresolved.is_empty(),
        "every saved entry id still resolves: {:?}",
        after.unresolved
    );
    assert!(resolve_saved(&catalog, &saved).iter().all(Option::is_some));
    assert_eq!(
        ui.saved_ids(),
        saved.as_slice(),
        "identity is locale-free: the same ids in the same order"
    );
    assert_eq!(
        after.selection,
        Some(item("page-m1")),
        "the selection stays on the same id"
    );
    assert_eq!(after.dropped, None);
    assert_eq!(ui.page_index(), 1, "the screen stays where it was");
    assert_eq!(titles(&ui), [Some("sv photo-a"), Some("sv photo-b")]);

    // A switch that cannot be answered is refused with the previous locale
    // still installed, so the running screen keeps its text.
    assert!(matches!(
        texts.switch_locale(chain("fr-fr"), TextCatalog::new()),
        Err(TextSessionError::NoContentForLocale { .. })
    ));
    let retry = ui.refresh(&catalog, &records, &texts);
    assert!(retry.unresolved.is_empty());
    assert_eq!(ui.saved_ids(), saved.as_slice());
    assert_eq!(
        titles(&ui),
        [Some("sv photo-a"), Some("sv photo-b")],
        "the refused switch leaves the screen readable"
    );
}

#[test]
fn accept_f47_c_refresh_reports_an_id_the_catalog_lost() {
    let catalog = catalog();
    let records = some_records();
    let texts = session(titles_for(&catalog, "en-us", "en"));
    let mut ui = ScrapbookUi::new(4).expect("a page of four entries");
    ui.refresh(&catalog, &records, &texts);
    ui.select(&item("page-m1")).expect("it is on screen");

    // The stored screen state comes back, and the catalog no longer declares
    // the ids it was showing: they are reported, and the selection is dropped
    // rather than left pointing at an entry that is not there.
    ui.restore(&[item("page-m1"), item("photo-a")]);
    let mut intro = entry("intro", EntryKind::Page, known(Unlock::Always));
    intro.visibility = EntryVisibility::Shown;
    let shrunk = ScrapbookCatalog::new(vec![intro]).expect("a catalog that lost an entry");

    let refresh = ui.refresh(&shrunk, &records, &texts);
    assert_eq!(
        refresh.unresolved,
        [item("page-m1"), item("photo-a")],
        "the ids that stopped resolving are named"
    );
    assert_eq!(refresh.dropped, Some(item("page-m1")));
    assert_eq!(refresh.selection, None, "a lost id selects nothing else");
    assert_eq!(ids(ui.page()), ["intro"]);
    assert_eq!(saved_keys(&ui), ["intro"]);
}

#[test]
fn accept_f47_c_select_reads_the_projection_not_a_pending_stored_state() {
    let catalog = catalog();
    let records = some_records();
    let texts = session(titles_for(&catalog, "en-us", "en"));
    let mut ui = ScrapbookUi::new(4).expect("a page of four entries");
    ui.refresh(&catalog, &records, &texts);

    // `restore` carries the ids of a stored screen state so the *next*
    // refresh can check them against the catalog. Until that refresh runs
    // they are not what the screen shows, so they must not decide what the
    // player can select: `page-m2` is hidden, and `intro` is on the screen
    // even though the restored list does not name it.
    ui.restore(&[item("page-m2")]);
    assert_eq!(
        ui.saved_ids(),
        [item("page-m2")],
        "the restored list is the pending state, not the projection"
    );
    assert!(matches!(
        ui.select(&item("page-m2")),
        Err(SelectError::NotVisible { .. })
    ));
    ui.select(&item("intro")).expect("intro is on the screen");
    assert_eq!(ui.selected_id(), Some(&item("intro")));
    assert_eq!(
        ui.selected().map(|page| page.id.clone()),
        Some(item("intro")),
        "selected_id never names an entry selected() cannot produce"
    );

    // After the refresh the screen state is the projection again, and every
    // projected entry is selectable.
    ui.refresh(&catalog, &records, &texts);
    ui.select(&item("photo-b")).expect("it is projected");
    assert_eq!(ui.selected_id(), Some(&item("photo-b")));
}

#[test]
fn accept_f47_c_replay_launch_names_the_loading_target() {
    let catalog = catalog();
    let records = some_records();
    let texts = session(titles_for(&catalog, "en-us", "en"));
    let mut ui = ScrapbookUi::new(4).expect("a page of four entries");
    ui.refresh(&catalog, &records, &texts);
    assert!(matches!(
        ui.launch(&catalog, &records),
        Err(LaunchError::NoSelection)
    ));

    ui.select(&item("page-m1")).expect("it is on screen");
    let launch = ui.launch(&catalog, &records).expect("unlocked and linked");
    assert_eq!(launch.entry, item("page-m1"));
    assert_eq!(launch.request.mission, mission("m1"));
    assert_eq!(launch.request.variant, Some(mission("m1-night")));
    assert_eq!(
        launch.scope.as_str(),
        "m1-night",
        "the variant is the mission-kind element the load runs under"
    );

    let world = WorldGroup::new("zbd/c1").expect("a valid world group");
    let target = launch.load_target(world.clone());
    assert_eq!(target.world, Some(world));
    assert_eq!(target.mission, Some(launch.scope));
    assert_eq!(target.to_string(), "world zbd/c1, mission m1-night");

    // An unlocked page with no link refuses instead of launching something
    // the entry never named.
    ui.select(&item("photo-a")).expect("it is on screen");
    assert_eq!(
        ui.launch(&catalog, &records),
        Err(LaunchError::Refused(
            cs_app::ui::scrapbook::ScrapbookActionError::NoReplay(item("photo-a"))
        ))
    );

    // A link without a variant replays the mission itself.
    let plain = ScrapbookCatalog::new(vec![linked(
        "plain",
        known(Unlock::Always),
        Some(ReplayLink {
            mission: mission("m1"),
            variant: None,
        }),
    )])
    .expect("a catalog with one link");
    let plain_records = ScrapbookRecords::default();
    ui.refresh(&plain, &plain_records, &texts);
    ui.select(&item("plain")).expect("it is on screen");
    let launch = ui
        .launch(&plain, &plain_records)
        .expect("unlocked and linked");
    assert_eq!(launch.request.variant, None);
    assert_eq!(launch.scope.as_str(), "m1");

    // A mission key the scope label grammar refuses is reported, not guessed
    // around: the loading path would have no label to load under.
    let odd = ScrapbookCatalog::new(vec![linked(
        "odd",
        known(Unlock::Always),
        Some(ReplayLink {
            mission: mission("-night"),
            variant: None,
        }),
    )])
    .expect("a catalog with an unfolderable link");
    ui.refresh(&odd, &plain_records, &texts);
    ui.select(&item("odd")).expect("it is on screen");
    let refused = ui
        .launch(&odd, &plain_records)
        .expect_err("a leading dash is no mission-scope label");
    assert!(matches!(
        &refused,
        LaunchError::Scope(failure)
        if failure.entry == item("odd") && failure.mission == crate::mission("-night")
    ));
    assert!(refused.to_string().contains("-night"));
    assert_eq!(
        ui.selected_id(),
        Some(&item("odd")),
        "a refused launch keeps the selection so the press can be retried"
    );
}

#[test]
fn accept_f47_c_replay_launch_refuses_then_retries_after_the_producer() {
    let catalog = ScrapbookCatalog::new(vec![linked(
        "late-link",
        known(fact(UnlockFactKind::MissionSucceeded, mission("m1"))),
        Some(ReplayLink {
            mission: mission("m1"),
            variant: None,
        }),
    )])
    .expect("a catalog with one locked, linked page");
    let mut records = ScrapbookRecords::default();
    let texts = session(titles_for(&catalog, "en-us", "en"));
    let mut ui = ScrapbookUi::new(4).expect("a page of four entries");

    let opened = ui.refresh(&catalog, &records, &texts);
    assert_eq!(ids(ui.page()), ["late-link"], "shown before it unlocks");
    assert!(!ui.page()[0].unlocked);
    assert!(!ui.page()[0].replayable, "a locked page offers no replay");
    assert_eq!(opened.dropped, None);

    ui.select(&item("late-link")).expect("it is on screen");
    let refused = ui
        .launch(&catalog, &records)
        .expect_err("the mission has not succeeded yet");
    assert!(matches!(
        &refused,
        LaunchError::Refused(cs_app::ui::scrapbook::ScrapbookActionError::Locked(id))
        if id == &item("late-link")
    ));
    assert_eq!(
        ui.selected_id(),
        Some(&item("late-link")),
        "a refusal changes nothing, so the press is retried"
    );

    // The producer records the outcome; the consumer sees it on the next
    // refresh and the same launch now goes through.
    record_mission(
        &mut records,
        &result(1, mission("m1"), Outcome::Succeeded, 5),
    )
    .expect("m1");
    ui.refresh(&catalog, &records, &texts);
    assert!(ui.page()[0].unlocked);
    assert!(ui.page()[0].replayable);
    assert_eq!(ui.selected_id(), Some(&item("late-link")));
    let launch = ui.launch(&catalog, &records).expect("the retry succeeds");
    assert_eq!(launch.entry, item("late-link"));
    assert_eq!(launch.scope.as_str(), "m1");
}

#[test]
fn accept_f47_c_close_tears_the_screen_down_and_a_refresh_reopens_it() {
    let catalog = catalog();
    let records = some_records();
    let texts = session(titles_for(&catalog, "en-us", "en"));
    let mut ui = ScrapbookUi::new(2).expect("a page of two entries");
    ui.refresh(&catalog, &records, &texts);
    ui.select(&item("page-m1")).expect("it is on screen");

    ui.close();
    ui.close();
    assert!(!ui.is_open(), "close is idempotent");
    assert_eq!(ui.page_count(), 0);
    assert!(ui.page().is_empty());
    assert!(ui.saved_ids().is_empty());
    assert_eq!(ui.selected_id(), None);
    assert!(matches!(
        ui.select(&item("page-m1")),
        Err(SelectError::Closed)
    ));
    assert!(matches!(
        ui.launch(&catalog, &records),
        Err(LaunchError::Closed)
    ));
    assert!(matches!(
        ui.go_to_page(0),
        Err(PageError::OutOfRange { page: 0, pages: 0 })
    ));
    assert!(matches!(
        ui.next_page(),
        Err(PageError::OutOfRange { page: 1, pages: 0 })
    ));

    ui.refresh(&catalog, &records, &texts);
    assert!(ui.is_open());
    assert_eq!(ui.page_count(), 3);
    assert_eq!(
        ui.selected_id(),
        None,
        "teardown is not undone by reopening"
    );
    assert_eq!(ids(ui.page()), ["intro", "page-m1"]);
}

#[test]
fn accept_f47_c_stored_records_produce_the_pages() {
    let base = TempDir::new();
    let catalog = catalog();
    let texts = session(titles_for(&catalog, "en-us", "en"));
    let mut ui = ScrapbookUi::new(4).expect("a page of four entries");

    let profile =
        ProfileSession::open_sandbox(base.path(), &SettingCatalog::new([]).expect("catalog"))
            .expect("open a sandbox");
    // No profile is selected, so the producer cannot answer: the refusal is
    // propagated and opens no screen.
    assert!(matches!(
        ui.refresh_stored(&catalog, &profile, &texts),
        Err(PersistError::Session(SessionError::NoProfileSelected))
    ));
    assert!(!ui.is_open());
    assert!(ui.page().is_empty());

    let mut profile = profile;
    profile.create("Pilot").expect("create");
    persist_stunt(&mut profile, &stunt("a")).expect("persist");
    let refresh = ui
        .refresh_stored(&catalog, &profile, &texts)
        .expect("the stored records are readable");
    assert_eq!(refresh.pages, 1);
    assert!(refresh.unresolved.is_empty());
    assert_eq!(ids(ui.page()), ["intro", "photo-a"]);

    // A stored `scrapbook.` field the reader refuses propagates, and the
    // screen keeps what it was showing instead of projecting a half-read
    // state.
    profile
        .commit_with(|document| {
            document.extra.push(ExtraField {
                key: "scrapbook.mystery".to_owned(),
                value: "x".to_owned(),
            });
            Ok(())
        })
        .expect("plant a bad field");
    assert!(matches!(
        ui.refresh_stored(&catalog, &profile, &texts),
        Err(PersistError::Restore(_))
    ));
    assert_eq!(ids(ui.page()), ["intro", "photo-a"]);
}
