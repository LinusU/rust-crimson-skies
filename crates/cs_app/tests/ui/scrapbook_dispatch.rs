//! Acceptance task F47-C1 (#760): the production front end drives the paged
//! scrapbook.
//!
//! F47-C delivered the screen (`crates/cs_app/src/ui/scrapbook/paged.rs`);
//! this task is the dispatch: `FrontEndFlow` holds the one [`ScrapbookUi`],
//! opens it on the selected profile's stored records through the declared
//! catalog and the running text session, turns its page and entry actions
//! into the screen's own fallible operations, and hands a launched replay to
//! the loading path as a `LoadTarget` over the application-declared world.
//! Every refusal (`PageError`, `SelectError`, `LaunchError`, `PersistError`,
//! a missing catalog or text session, a world-less declared target) leaves
//! the machine on the scrapbook with the reason the screen shows — nothing
//! is swallowed — and leaving the screen is its teardown.
//!
//! All data is authored fixture data: no original scrapbook rule, page size,
//! layout or control is claimed (F47-D owns the audit).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cs_app::campaign::lower_campaign;
use cs_app::loading::{Criticality, LoadItem, LoadTarget};
use cs_app::profile::ProfileSession;
use cs_app::text::{FontMeasurer, FontSet, TextMetrics, TextSession, synthetic_monospace};
use cs_app::ui::front_end::{
    Action, FlowError, FlowSetup, FrontEndFlow, LoadError, LoadPlan, ScrapbookInputs, Screen,
};
use cs_app::ui::scrapbook::{
    LaunchError, MissionResult, ScrapbookActionError, SelectError, persist_mission, persist_stunt,
};
use cs_assets::cache::{CacheBudget, CacheDirectory, CacheStore};
use cs_assets::vfs::{ContentSession, MountBuilder, SessionBuilder};
use cs_content::campaign::declared_synthetic_campaign;
use cs_content::localization::{
    FontCatalog, FontFace, LocaleChain, LocaleId, LocalizedText, TextCatalog, TextId,
    synthetic_font_face, synthetic_markup_grammar,
};
use cs_content::save::settings::SettingCatalog;
use cs_content::scrapbook::{
    EntryKind, EntryVisibility, ReplayLink, ScrapbookCatalog, ScrapbookEntry, Unlock, UnlockFact,
    UnlockFactKind,
};
use cs_sim::campaign::{
    CampaignRunId, DifficultyId, EventKey, Outcome, OutcomeId, ProfileId as CampaignProfileId,
    SessionGeneration, SymbolId,
};
use cs_sim::records::{BetterIs, DifficultyScope, RecordRule};
use cs_types::Tick;
use cs_types::asset_id::{
    AssetKey, MountId, MountNamespace, PrecedenceClass, ResolveContext, WorldGroup,
};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ContentHash};
use cs_types::profile::ExtraField;

use super::id;
use super::screens::preflight_deck;

// ------------------------------------------------------------ fixtures ---

/// The world group the fixture content session mounts.
const WORLD: &str = "zbd/c1";

/// The claim every designed value in this file is recorded under.
fn claim() -> ClaimId {
    ClaimId::new("f47c1.dispatch-test").expect("claim id")
}

fn designed() -> Provenance {
    Provenance::designed(claim())
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, designed()))
}

fn mission(key: &str) -> ContentId {
    id(ContentKind::Mission, key)
}

fn stunt(key: &str) -> ContentId {
    id(ContentKind::Stunt, key)
}

fn item(key: &str) -> ContentId {
    id(ContentKind::ScrapbookItem, key)
}

fn text(key: &str) -> ContentId {
    id(ContentKind::StringResource, key)
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
                designed(),
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

/// A text session in `locale_label` over `texts`, with the synthetic face
/// loaded.
fn session(locale_label: &str, texts: TextCatalog) -> TextSession {
    let mut faces = FontCatalog::new();
    faces
        .insert(synthetic_font_face())
        .expect("the fixture face is inserted once");
    let fonts = FontSet::load(&faces, &Measurer).expect("the synthetic face is measurable");
    TextSession::try_new(
        chain(locale_label),
        texts,
        synthetic_markup_grammar(),
        fonts,
        synthetic_font_face().id().clone(),
    )
    .expect("the session's default font is loaded")
}

const HIGH: RecordRule = RecordRule {
    better: BetterIs::Higher,
    scope: DifficultyScope::AllDifficulties,
};

fn outcome_id(serial: u32) -> OutcomeId {
    OutcomeId {
        profile: CampaignProfileId::new("p1").expect("profile"),
        run: CampaignRunId::new("run.f47c1").expect("run"),
        session: SessionGeneration(serial),
        terminal_event: EventKey {
            session: SessionGeneration(serial),
            tick: Tick(100),
            source: SymbolId(1),
            sequence: 0,
        },
    }
}

fn result(serial: u32, mission: ContentId, outcome: Outcome, score: u64) -> MissionResult {
    MissionResult {
        outcome_id: outcome_id(serial),
        mission,
        difficulty: DifficultyId::new("normal").expect("difficulty"),
        outcome,
        score,
        rule: HIGH,
    }
}

fn fact(kind: UnlockFactKind, subject: ContentId) -> Unlock {
    Unlock::Fact(UnlockFact { kind, subject })
}

fn entry(key: &str, kind: EntryKind, unlock: Resolved<Unlock>) -> ScrapbookEntry {
    ScrapbookEntry {
        id: item(key),
        kind,
        title: text(&format!("{key}-title")),
        image: Some(id(ContentKind::Image, &format!("{key}-image"))),
        unlock,
        visibility: EntryVisibility::HiddenUntilUnlocked,
        replay: None,
    }
}

/// An entry shown before it unlocks — the shape a locked entry the player
/// can still select has — linked to a mission (or not).
fn linked(key: &str, unlock: Resolved<Unlock>, replay: Option<ReplayLink>) -> ScrapbookEntry {
    let mut declared = entry(key, EntryKind::Page, unlock);
    declared.visibility = EntryVisibility::Shown;
    declared.replay = replay;
    declared
}

/// The fixture catalog: `intro` always shown without a link, `locked-link`
/// shown but locked on `m2`, `page-m1` hidden until `m1` succeeds and linked
/// to its `m1-night` variant, `photo-a`/`photo-b` unlocked by their stunts.
fn catalog() -> ScrapbookCatalog {
    let mut intro = entry("intro", EntryKind::Page, known(Unlock::Always));
    intro.visibility = EntryVisibility::Shown;
    let locked = linked(
        "locked-link",
        known(fact(UnlockFactKind::MissionSucceeded, mission("m2"))),
        Some(ReplayLink {
            mission: mission("m2"),
            variant: None,
        }),
    );
    let mut m1 = entry(
        "page-m1",
        EntryKind::Page,
        known(fact(UnlockFactKind::MissionSucceeded, mission("m1"))),
    );
    m1.replay = Some(ReplayLink {
        mission: mission("m1"),
        variant: Some(mission("m1-night")),
    });
    let photo_a = entry(
        "photo-a",
        EntryKind::StuntPhoto,
        known(fact(UnlockFactKind::StuntCompleted, stunt("a"))),
    );
    let photo_b = entry(
        "photo-b",
        EntryKind::StuntPhoto,
        known(fact(UnlockFactKind::StuntCompleted, stunt("b"))),
    );
    ScrapbookCatalog::new(vec![intro, locked, m1, photo_a, photo_b]).expect("catalog")
}

/// A unique, disposable directory under the system temporary directory.
struct TempTree {
    root: PathBuf,
}

impl TempTree {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-f47-c1-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("the fixture root is created");
        Self { root }
    }

    fn path(&self) -> &Path {
        &self.root
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// The install/cache/profile roots of one fixture, plus the flow they build.
struct Fixture {
    tree: TempTree,
    catalog: ScrapbookCatalog,
    profile: cs_types::profile::ProfileId,
}

impl Fixture {
    /// The stored scrapbook is the seeded profile's: stunts `a` and `b`
    /// completed and `m1` succeeded — `locked-link` (on `m2`) stays locked.
    fn new(label: &str) -> Self {
        let tree = TempTree::new(label);
        let profiles = tree.path().join("profiles");
        let mut session = ProfileSession::open_sandbox(&profiles, &SettingCatalog::empty())
            .expect("the sandbox opens");
        let profile = session.create("Pilot").expect("the pilot is created");
        persist_stunt(&mut session, &stunt("a")).expect("stunt a persists");
        persist_stunt(&mut session, &stunt("b")).expect("stunt b persists");
        persist_mission(
            &mut session,
            &result(1, mission("m1"), Outcome::Succeeded, 5),
        )
        .expect("the m1 result persists");
        session.finish().expect("the seeded profile is written");

        let world = tree.path().join("install").join("zbd").join("c1");
        fs::create_dir_all(&world).expect("the world directory exists");
        fs::write(world.join("hull.bm"), b"fixture hull bytes").expect("a member is written");
        Self {
            tree,
            catalog: catalog(),
            profile,
        }
    }

    fn profiles(&self) -> PathBuf {
        self.tree.path().join("profiles")
    }

    fn install(&self) -> PathBuf {
        self.tree.path().join("install")
    }

    fn cache(&self) -> PathBuf {
        self.tree.path().join("cache")
    }

    fn world(&self) -> PathBuf {
        self.install().join("zbd").join("c1")
    }

    fn session(&self) -> ContentSession {
        let mut builder = SessionBuilder::new(
            ResolveContext::new(install_hash())
                .with_world_group(WorldGroup::new(WORLD).expect("the fixture world spelling")),
        );
        let mount = MountBuilder::new(
            MountId::new("world-0").expect("a mount id"),
            MountNamespace::new("world").expect("a namespace"),
            PrecedenceClass::MissionWorld,
            WORLD,
        )
        .with_world_group(WorldGroup::new(WORLD).expect("the fixture world spelling"));
        builder
            .mount_directory(mount, &self.world())
            .expect("the world directory mounts");
        builder.open()
    }

    fn store(&self) -> CacheStore {
        fs::create_dir_all(self.cache()).expect("the cache root exists");
        CacheStore::open(
            CacheDirectory::open(&self.cache(), &self.install())
                .expect("a cache outside the install"),
            CacheBudget::new(8, 8 << 20).expect("a nonzero budget"),
        )
        .expect("the store opens")
    }

    /// The declared setup: the fixture catalog at two entries a page.
    fn setup(&self) -> FlowSetup {
        FlowSetup::new(
            self.profiles(),
            lower_campaign(&declared_synthetic_campaign()).expect("the fixture lowers"),
            CampaignRunId::new("run.f47c1").expect("valid run id"),
            DifficultyId::new("standard").expect("valid difficulty"),
        )
        .sandbox()
        .profile_name("Pilot")
        .scrapbook(ScrapbookInputs::try_new(self.catalog.clone(), 2).expect("a nonzero page size"))
    }

    /// A flow with the authored preflight deck, the declared closure over the
    /// one member the install carries, and the running `en-us` text session.
    fn flow(&self) -> FrontEndFlow {
        let mut flow = FrontEndFlow::new(preflight_deck(), self.setup());
        flow.set_content_session(self.session());
        flow.set_cache_store(self.store());
        flow.set_load_plan(load_plan());
        flow.set_text_session(session("en-us", titles_for(&self.catalog, "en-us", "en")));
        flow
    }
}

fn install_hash() -> ContentHash {
    ContentHash::from_bytes([0x5A; 32])
}

fn member(name: &str) -> AssetKey {
    AssetKey::from_spelling("world", name, "default").expect("the fixture key is valid")
}

/// The declared closure: the world group a replay loads under, and one
/// member the installation carries.
fn load_plan() -> LoadPlan {
    LoadPlan {
        target: LoadTarget::world(WorldGroup::new(WORLD).expect("the fixture world spelling")),
        items: vec![
            LoadItem::new(
                member("hull.bm"),
                id(ContentKind::Image, "fixture.hull"),
                Criticality::GameplayCritical,
                64,
            )
            .expect("a declared work unit"),
        ],
    }
}

/// Walks the flow to the cabin over the fixture's seeded profile, then into
/// the scrapbook.
fn to_scrapbook(flow: &mut FrontEndFlow) {
    flow.press(Action::InstallVerified)
        .expect("the install verifies");
    flow.press(Action::ContinueProfile)
        .expect("the profile continues");
    flow.press(Action::ConfirmProfile)
        .expect("the profile opens");
    flow.press(Action::OpenScrapbook)
        .expect("the scrapbook opens");
    assert_eq!(flow.front_end().screen(), Screen::Scrapbook);
}

fn page_keys(flow: &FrontEndFlow) -> Vec<String> {
    flow.domain()
        .scrapbook()
        .expect("the catalog was declared")
        .page()
        .iter()
        .map(|page| page.id.key().to_owned())
        .collect()
}

fn saved_keys(flow: &FrontEndFlow) -> Vec<String> {
    flow.domain()
        .scrapbook()
        .expect("the catalog was declared")
        .saved_ids()
        .iter()
        .map(|id| id.key().to_owned())
        .collect()
}

fn titles(flow: &FrontEndFlow) -> Vec<Option<String>> {
    flow.domain()
        .scrapbook()
        .expect("the catalog was declared")
        .page()
        .iter()
        .map(|page| page.title.clone())
        .collect()
}

fn reason(flow: &FrontEndFlow) -> Option<String> {
    flow.domain().scrapbook_reason().map(str::to_owned)
}

fn ui_open(flow: &FrontEndFlow) -> bool {
    flow.domain().scrapbook().is_some_and(|ui| ui.is_open())
}

fn ui_page_count(flow: &FrontEndFlow) -> usize {
    flow.domain().scrapbook().map_or(0, |ui| ui.page_count())
}

fn ui_page_empty(flow: &FrontEndFlow) -> bool {
    flow.domain()
        .scrapbook()
        .is_none_or(|ui| ui.page().is_empty())
}

fn ui_selection(flow: &FrontEndFlow) -> Option<ContentId> {
    flow.domain()
        .scrapbook()
        .and_then(|ui| ui.selected_id().cloned())
}

// ---------------------------------------------------------------- tests ---

/// The happy path end to end: the stored records project, paging walks them,
/// a stable-id selection lands, Back is the teardown, and reopening rebuilds
/// the screen fresh.
#[test]
fn accept_f47_c1_open_pages_selects_and_backs_out() {
    let fixture = Fixture::new("walk");
    let mut flow = fixture.flow();
    to_scrapbook(&mut flow);

    assert!(ui_open(&flow));
    // The projection over the stored records: intro and locked-link show,
    // page-m1 unlocked through the stored result, both photos through their
    // stunts — two entries a page over three pages.
    assert_eq!(
        saved_keys(&flow),
        ["intro", "locked-link", "page-m1", "photo-a", "photo-b"]
    );
    assert_eq!(page_keys(&flow), ["intro", "locked-link"]);
    assert_eq!(ui_page_count(&flow), 3);
    assert_eq!(reason(&flow), None);

    // Paging is the table's own actions, not a parallel input: the machine
    // asks and the screen answers.
    flow.press(Action::NextPage).expect("page two");
    assert_eq!(page_keys(&flow), ["page-m1", "photo-a"]);
    flow.press(Action::NextPage).expect("page three");
    assert_eq!(page_keys(&flow), ["photo-b"]);

    // One page too far is the screen's own refusal, surfaced as the reason
    // the screen shows — the machine does not move.
    let refused = flow
        .press(Action::NextPage)
        .expect_err("a fourth page does not exist");
    assert!(matches!(
        refused,
        FlowError::ScrapbookPage(cs_app::ui::scrapbook::PageError::OutOfRange {
            page: 3,
            pages: 3
        })
    ));
    assert_eq!(flow.front_end().screen(), Screen::Scrapbook);
    let shown = reason(&flow).expect("the refusal is the visible reason");
    assert!(shown.contains("page"), "{shown}");
    flow.press(Action::PrevPage).expect("back to page two");
    assert_eq!(page_keys(&flow), ["page-m1", "photo-a"]);
    assert_eq!(reason(&flow), None, "a successful step clears the reason");

    // Selection names the stable id; an id that is not on the screen — a
    // mission id is no scrapbook entry — is the screen's refusal.
    let hidden = flow
        .select_scrapbook(mission("m1"))
        .expect_err("a mission id is no scrapbook entry");
    assert!(matches!(
        hidden,
        FlowError::ScrapbookSelect(SelectError::NotVisible { ref id })
            if *id == mission("m1")
    ));
    assert_eq!(flow.front_end().screen(), Screen::Scrapbook);
    flow.select_scrapbook(item("page-m1"))
        .expect("the entry is on the page");
    assert_eq!(
        ui_selection(&flow),
        Some(item("page-m1")),
        "the stable id is the selection"
    );

    // Back is the teardown: the screen closes, and reopening rebuilds it
    // from the stored records rather than remembering anything.
    flow.press(Action::Back).expect("back to the cabin");
    assert_eq!(flow.front_end().screen(), Screen::Cabin);
    assert!(!ui_open(&flow), "leaving closed the screen");
    assert!(ui_page_empty(&flow));
    assert_eq!(ui_selection(&flow), None);
    flow.press(Action::OpenScrapbook).expect("reopen");
    assert!(ui_open(&flow));
    assert_eq!(
        saved_keys(&flow),
        ["intro", "locked-link", "page-m1", "photo-a", "photo-b"]
    );
    assert_eq!(
        ui_selection(&flow),
        None,
        "teardown is not undone by reopening"
    );
}

/// The stage's minimum scenario, through the production front end: a locale
/// change re-resolves every title while every saved entry id still resolves —
/// identity is locale-free.
#[test]
fn accept_f47_c1_a_locale_change_keeps_every_saved_id_and_retitles_the_pages() {
    let fixture = Fixture::new("locale");
    let mut flow = fixture.flow();
    to_scrapbook(&mut flow);
    flow.select_scrapbook(item("page-m1")).expect("select");
    let before = saved_keys(&flow);
    assert_eq!(
        titles(&flow),
        [
            Some("en intro".to_owned()),
            Some("en locked-link".to_owned())
        ]
    );

    // The running text session is the application's own object: a locale
    // switch replaces it and the open screen refreshes through it.
    flow.set_text_session(session(
        "fr-fr",
        titles_for(&fixture.catalog, "fr-fr", "fr"),
    ));

    assert_eq!(
        saved_keys(&flow),
        before,
        "locale moves titles, never identity"
    );
    assert_eq!(
        titles(&flow),
        [
            Some("fr intro".to_owned()),
            Some("fr locked-link".to_owned())
        ]
    );
    let refresh = flow
        .domain()
        .scrapbook_refresh()
        .cloned()
        .expect("the reopening refresh is reported");
    assert!(
        refresh.unresolved.is_empty(),
        "every saved id still resolves after the locale change: {:?}",
        refresh.unresolved
    );
    assert_eq!(
        refresh.selection.as_ref(),
        Some(&item("page-m1")),
        "the selection survives the locale change"
    );
}

/// The launch: the selection's replay link becomes the load's own target —
/// the declared world plus the entry's validated scope — and teardown puts
/// the declared target back.
#[test]
fn accept_f47_c1_launch_loads_the_replay_scope_and_restores_the_declared_target() {
    let fixture = Fixture::new("launch");
    let mut flow = fixture.flow();
    to_scrapbook(&mut flow);
    flow.select_scrapbook(item("page-m1")).expect("select");

    flow.press(Action::Launch).expect("the replay launches");
    assert_eq!(flow.front_end().screen(), Screen::Loading);
    let target = flow
        .load()
        .target()
        .expect("the attempt is running the replay");
    assert_eq!(
        target.world.as_ref(),
        Some(&WorldGroup::new(WORLD).expect("the fixture world spelling")),
        "the world is the application's declaration"
    );
    assert_eq!(
        target.mission.as_ref().map(|scope| scope.as_str()),
        Some("m1-night"),
        "the scope is the entry's own validated variant"
    );

    // Leaving the loading screen — a cancelled load here — tears the attempt
    // down and restores the declared target for the next launch.
    flow.press(Action::Cancel).expect("cancel the load");
    assert_eq!(flow.front_end().screen(), Screen::FlightCheck);
    let restored = flow.load().plan().expect("the declared plan");
    assert_eq!(
        restored.target.world.as_ref(),
        Some(&WorldGroup::new(WORLD).expect("the fixture world spelling"))
    );
    assert_eq!(
        restored.target.mission, None,
        "the replay's scope is gone with its attempt"
    );
}

/// Every way a launch is refused stays on the scrapbook with its reason
/// showing: no selection, a locked entry, a link-less entry, an unreadable
/// stored scrapbook, a declared target with no world.
#[test]
fn accept_f47_c1_a_refused_launch_stays_on_the_screen_with_the_reason_showing() {
    let fixture = Fixture::new("refused");
    let mut flow = fixture.flow();
    to_scrapbook(&mut flow);

    // Nothing selected.
    let refused = flow
        .press(Action::Launch)
        .expect_err("nothing was selected");
    assert!(matches!(
        refused,
        FlowError::ScrapbookLaunch(LaunchError::NoSelection)
    ));
    assert_eq!(flow.front_end().screen(), Screen::Scrapbook);
    assert!(reason(&flow).is_some());

    // A locked entry selects — it is on the page — but its replay refuses.
    flow.select_scrapbook(item("locked-link"))
        .expect("the locked entry is visible");
    let refused = flow
        .press(Action::Launch)
        .expect_err("m2 has not succeeded");
    assert!(matches!(
        &refused,
        FlowError::ScrapbookLaunch(LaunchError::Refused(ScrapbookActionError::Locked(id)))
            if id == &item("locked-link")
    ));
    assert_eq!(flow.front_end().screen(), Screen::Scrapbook);
    let shown = reason(&flow).expect("the refusal is visible");
    assert!(shown.contains("locked"), "{shown}");
    assert_eq!(
        ui_selection(&flow),
        Some(item("locked-link")),
        "a refused launch keeps the selection so the press can be retried"
    );

    // A link-less entry has no replay to launch.
    flow.select_scrapbook(item("intro")).expect("select intro");
    let refused = flow
        .press(Action::Launch)
        .expect_err("intro carries no replay link");
    assert!(matches!(
        &refused,
        FlowError::ScrapbookLaunch(LaunchError::Refused(ScrapbookActionError::NoReplay(id)))
            if id == &item("intro")
    ));
    assert_eq!(flow.front_end().screen(), Screen::Scrapbook);
}

/// The screen still opens when the stored scrapbook will not answer: the
/// `PersistError` is the reason it shows rather than a swallowed failure or
/// a missing screen — and repairing the record through the producer's own
/// persistence path is the retry.
#[test]
fn accept_f47_c1_an_unreadable_stored_scrapbook_is_the_screens_reason_not_its_end() {
    let fixture = Fixture::new("corrupt");
    // Plant a field the records reader refuses, through the profile's own
    // document transaction — the producer path, not an edit behind its back.
    let mut session = ProfileSession::open_sandbox(&fixture.profiles(), &SettingCatalog::empty())
        .expect("the sandbox opens");
    session
        .select(fixture.profile)
        .expect("the seeded profile selects");
    session
        .commit_with(|document| {
            document.extra.push(ExtraField {
                key: "scrapbook.mystery".to_owned(),
                value: "x".to_owned(),
            });
            Ok(())
        })
        .expect("the bad field is planted");
    session.finish().expect("the planted profile is written");

    let mut flow = fixture.flow();
    flow.press(Action::InstallVerified)
        .expect("the install verifies");
    flow.press(Action::ContinueProfile)
        .expect("the profile continues");
    flow.press(Action::ConfirmProfile)
        .expect("the profile opens");
    flow.press(Action::OpenScrapbook)
        .expect("the screen still opens");
    assert_eq!(flow.front_end().screen(), Screen::Scrapbook);
    let shown = reason(&flow).expect("the PersistError is the visible reason");
    assert!(!shown.is_empty());
    assert!(
        !flow.domain().scrapbook().expect("catalog").is_open(),
        "a projection that could not be built is not shown"
    );

    // Back tears down; the profile closes; the corrupt field is removed
    // through the profile's own document transaction — `persist_*` restores
    // before it writes, so a field it refuses cannot be repaired by writing
    // over it — and the reopen is the retry.
    flow.press(Action::Back).expect("back to the cabin");
    flow.press(Action::Back).expect("close the profile");
    assert_eq!(flow.front_end().screen(), Screen::MainMenu);
    let mut session = ProfileSession::open_sandbox(&fixture.profiles(), &SettingCatalog::empty())
        .expect("the sandbox reopens");
    session
        .select(fixture.profile)
        .expect("the profile selects");
    session
        .commit_with(|document| {
            document
                .extra
                .retain(|field| field.key != "scrapbook.mystery");
            Ok(())
        })
        .expect("the corrupt field is removed");
    session.finish().expect("the repaired profile is written");

    flow.press(Action::ContinueProfile)
        .expect("the profile continues");
    flow.press(Action::ConfirmProfile)
        .expect("the profile reopens");
    flow.press(Action::OpenScrapbook).expect("the retry opens");
    assert_eq!(reason(&flow), None);
    assert_eq!(
        saved_keys(&flow),
        ["intro", "locked-link", "page-m1", "photo-a", "photo-b"]
    );
}

/// The wiring refusals: no declared catalog, no running text session, and a
/// declared load target without a world are each refused before the machine
/// moves — the domain asks for what it needs rather than guessing.
#[test]
fn accept_f47_c1_opening_and_launching_refuse_when_the_declaration_is_missing() {
    // A dropped flow keeps its profile claim until it is closed, so each leg
    // gets its own fixture directory rather than reopening the same one.
    let fixture = Fixture::new("wiring-text");

    // No text session was ever handed over.
    let mut flow = FrontEndFlow::new(preflight_deck(), fixture.setup());
    flow.set_content_session(fixture.session());
    flow.set_cache_store(fixture.store());
    flow.set_load_plan(load_plan());
    flow.press(Action::InstallVerified)
        .expect("the install verifies");
    flow.press(Action::ContinueProfile)
        .expect("the profile continues");
    flow.press(Action::ConfirmProfile)
        .expect("the profile opens");
    let refused = flow
        .press(Action::OpenScrapbook)
        .expect_err("no running text session");
    assert!(matches!(refused, FlowError::NoTextSession));
    assert_eq!(flow.front_end().screen(), Screen::Cabin);

    // No catalog declared at all.
    let fixture = Fixture::new("wiring-catalog");
    let mut bare = fixture.setup();
    bare.scrapbook = None;
    let mut flow = FrontEndFlow::new(preflight_deck(), bare);
    flow.set_content_session(fixture.session());
    flow.set_cache_store(fixture.store());
    flow.set_load_plan(load_plan());
    flow.set_text_session(session("en-us", TextCatalog::new()));
    flow.press(Action::InstallVerified)
        .expect("the install verifies");
    flow.press(Action::ContinueProfile)
        .expect("the profile continues");
    flow.press(Action::ConfirmProfile)
        .expect("the profile opens");
    let refused = flow
        .press(Action::OpenScrapbook)
        .expect_err("no scrapbook catalog was declared");
    assert!(matches!(refused, FlowError::NoScrapbookCatalog));
    assert_eq!(flow.front_end().screen(), Screen::Cabin);
    assert!(flow.domain().scrapbook().is_none());

    // A declared target with no world: the replay has nowhere to load under,
    // and the refusal comes before the machine leaves the scrapbook — the
    // selection survives it.
    let fixture = Fixture::new("wiring-world");
    let mut flow = fixture.flow();
    let mut worldless = load_plan();
    worldless.target = LoadTarget {
        world: None,
        mission: None,
    };
    flow.set_load_plan(worldless);
    to_scrapbook(&mut flow);
    flow.select_scrapbook(item("page-m1")).expect("select");
    let refused = flow
        .press(Action::Launch)
        .expect_err("the declared target names no world");
    assert!(matches!(refused, FlowError::Load(LoadError::NoWorld)));
    assert_eq!(flow.front_end().screen(), Screen::Scrapbook);
    assert_eq!(
        ui_selection(&flow),
        Some(item("page-m1")),
        "a launch refused before the transition keeps the selection"
    );
}

/// Selecting without declaring the entry is the machine's own refusal: the
/// table row exists, the declared input does not.
#[test]
fn accept_f47_c1_selecting_nothing_is_refused_by_name() {
    let fixture = Fixture::new("no-choice");
    let mut flow = fixture.flow();
    to_scrapbook(&mut flow);
    let refused = flow
        .press(Action::SelectEntry)
        .expect_err("no entry was declared");
    assert!(matches!(refused, FlowError::Screen(_)));
    assert_eq!(flow.front_end().screen(), Screen::Scrapbook);
}
