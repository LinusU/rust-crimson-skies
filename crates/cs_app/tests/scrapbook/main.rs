//! Acceptance stage F47-A: scrapbook records and unlock predicates
//! (`specs/F47-scrapbook-records-mementos-and-mission-replay.md`, `### F47-A`).
//!
//! The minimum scenario is AC01: a better replay updates best but not
//! unrelated progression. The rest covers AC02/AC03's data halves (one stunt
//! photo unlocks alone; saved ids resolve in every locale), replay links, the
//! memento choice and catalog validation. All data is authored synthetic data;
//! this proves the contract, never an original scrapbook rule (F47-D).
//!
//! The `persistence` and `paged` modules carry their own headers: stage
//! `### F47-B` is the idempotent write path, stage `### F47-C` the paged
//! screen, the locale it reads and the replay it launches.

use cs_app::ui::scrapbook::MissionResult;
use cs_content::scrapbook::{
    EntryKind, EntryVisibility, ReplayLink, ScrapbookCatalog, ScrapbookEntry, Unlock, UnlockFact,
    UnlockFactKind,
};
use cs_sim::campaign::{
    CampaignRunId, DifficultyId, EventKey, Outcome, OutcomeId, ProfileId, SessionGeneration,
    SymbolId,
};
use cs_sim::records::{BetterIs, DifficultyScope, RecordRule};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;

mod mementos;
mod paged;
mod pages;
mod persistence;
mod records;
mod validation;

pub fn id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("content id")
}

pub fn mission(key: &str) -> ContentId {
    id(ContentKind::Mission, key)
}

pub fn stunt(key: &str) -> ContentId {
    id(ContentKind::Stunt, key)
}

pub fn item(key: &str) -> ContentId {
    id(ContentKind::ScrapbookItem, key)
}

pub fn text(key: &str) -> ContentId {
    id(ContentKind::StringResource, key)
}

pub const HIGH: RecordRule = RecordRule {
    better: BetterIs::Higher,
    scope: DifficultyScope::AllDifficulties,
};

pub fn outcome_id(serial: u32) -> OutcomeId {
    OutcomeId {
        profile: ProfileId::new("p1").expect("profile"),
        run: CampaignRunId::new("run1").expect("run"),
        session: SessionGeneration(serial),
        terminal_event: EventKey {
            session: SessionGeneration(serial),
            tick: Tick(100),
            source: SymbolId(1),
            sequence: 0,
        },
    }
}

pub fn difficulty(name: &str) -> DifficultyId {
    DifficultyId::new(name).expect("difficulty")
}

pub fn result(serial: u32, mission: ContentId, outcome: Outcome, score: u64) -> MissionResult {
    MissionResult {
        outcome_id: outcome_id(serial),
        mission,
        difficulty: difficulty("normal"),
        outcome,
        score,
        rule: HIGH,
    }
}

pub fn known(unlock: Unlock) -> Resolved<Unlock> {
    Resolved::Known(Known::new(
        unlock,
        Provenance::designed(ClaimId::new("f47a.synthetic-unlock").expect("claim")),
    ))
}

pub fn fact(kind: UnlockFactKind, subject: ContentId) -> Unlock {
    Unlock::Fact(UnlockFact { kind, subject })
}

pub fn entry(key: &str, kind: EntryKind, unlock: Resolved<Unlock>) -> ScrapbookEntry {
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

/// Two missions, two stunt photos, a memento and a link-less hidden page.
pub fn catalog() -> ScrapbookCatalog {
    let mut m1 = entry(
        "page-m1",
        EntryKind::Page,
        known(fact(UnlockFactKind::MissionSucceeded, mission("m1"))),
    );
    m1.replay = Some(ReplayLink {
        mission: mission("m1"),
        variant: Some(mission("m1-night")),
    });
    let m2 = entry(
        "page-m2",
        EntryKind::Page,
        known(fact(UnlockFactKind::MissionSucceeded, mission("m2"))),
    );
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
    let memento = entry(
        "memento-m1",
        EntryKind::Memento,
        known(fact(UnlockFactKind::MissionSucceeded, mission("m1"))),
    );
    let mut intro = entry("intro", EntryKind::Page, known(Unlock::Always));
    intro.visibility = EntryVisibility::Shown;
    ScrapbookCatalog::new(vec![intro, m1, m2, photo_a, photo_b, memento]).expect("catalog")
}
