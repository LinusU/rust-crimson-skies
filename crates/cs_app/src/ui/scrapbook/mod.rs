//! The scrapbook projection and replay request (F47-A).
//!
//! Spec: `specs/F47-scrapbook-records-mementos-and-mission-replay.md`, stage
//! `### F47-A`. Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! This is the boundary between the declared [`ScrapbookCatalog`]
//! (`cs_content::scrapbook`) and the persisted [`ScrapbookRecords`]
//! (`cs_sim::records`):
//!
//! * [`record_mission`] applies one finished mission's outcome. It notes the
//!   mission-succeeded fact (only for `Succeeded`) and submits the score to
//!   the [`RecordBook`](cs_sim::records::RecordBook); it touches nothing else,
//!   so a better replay moves best and leaves other unlocks alone.
//! * [`record_stunt`] notes one stunt fact: it unlocks exactly the entries
//!   whose predicate names that stunt.
//! * [`project`] lists the pages to show: hidden pages stay absent until their
//!   rule holds, and titles are resolved per locale through a caller closure
//!   while identity stays the locale-free [`ContentId`].
//! * [`resolve_saved`] re-resolves persisted entry ids against the catalog.
//! * [`replay_request`] turns an unlocked entry's replay link into the
//!   request the normal loading path consumes; a locked or link-less entry is
//!   refused.
//! * [`choose_memento`] validates the cabin choice against the unlocked
//!   mementos.
//!
//! Stage `### F47-B` persists all of that in the selected profile:
//! [`persist_mission`], [`persist_stunt`] and [`persist_memento`] each load the
//! stored scrapbook, apply the one change and write one whole profile revision
//! through [`ProfileSession::commit_with`], replacing only the `scrapbook.`
//! extra fields. A replayed result or repeated choice writes nothing, and
//! campaign progression is never rewritten.
//!
//! The synthetic catalog and records this stage is tested with prove the
//! projection only, never an original scrapbook rule.

use std::collections::BTreeSet;
use std::fmt;

use cs_content::scrapbook::{
    EntryKind, ScrapbookCatalog, ScrapbookEntry, UnlockFact, UnlockFactKind,
};
use cs_sim::campaign::{DifficultyId, Outcome, OutcomeId};
use cs_sim::records::{
    FIELD_PREFIX, Fact, FactKind, MementoError, RecordError, RecordReceipt, RecordRule,
    RestoreError, ScrapbookRecords,
};
use cs_types::content::ContentId;
use cs_types::profile::{ExtraField, ProfileDocument};

use crate::profile::{ChangeRefusal, ChangeRefusalReason, ProfileSession, SessionError};

fn fact_kind(kind: UnlockFactKind) -> FactKind {
    match kind {
        UnlockFactKind::MissionSucceeded => FactKind::MissionSucceeded,
        UnlockFactKind::StuntCompleted => FactKind::StuntCompleted,
        UnlockFactKind::AceDefeated => FactKind::AceDefeated,
    }
}

/// Whether the ledger holds the declared leaf.
fn has(records: &ScrapbookRecords) -> impl Fn(&UnlockFact) -> bool + '_ {
    move |leaf| {
        records.facts.has(&Fact {
            kind: fact_kind(leaf.kind),
            subject: leaf.subject.clone(),
        })
    }
}

/// One finished mission, as the scrapbook sees it.
#[derive(Clone, Debug)]
pub struct MissionResult {
    /// The outcome transaction identity (dedups a replayed result).
    pub outcome_id: OutcomeId,
    /// The mission.
    pub mission: ContentId,
    /// The run's difficulty.
    pub difficulty: DifficultyId,
    /// What happened.
    pub outcome: Outcome,
    /// The recorded score.
    pub score: u64,
    /// How this mission's records are kept.
    pub rule: RecordRule,
}

/// Applies `result`: the record, then the mission fact on success.
///
/// # Errors
///
/// [`RecordError`]; nothing is changed, fact included.
pub fn record_mission(
    records: &mut ScrapbookRecords,
    result: &MissionResult,
) -> Result<RecordReceipt, RecordError> {
    let receipt = records.records.submit(
        &result.outcome_id,
        &result.mission,
        &result.difficulty,
        result.score,
        result.rule,
    )?;
    if matches!(receipt, RecordReceipt::Applied { .. }) && result.outcome == Outcome::Succeeded {
        // The receipt already reports whether anything changed; the ledger only
        // ever grows, so its answer is not needed here.
        let _ = records.facts.note(Fact {
            kind: FactKind::MissionSucceeded,
            subject: result.mission.clone(),
        });
    }
    Ok(receipt)
}

/// Notes a completed stunt; returns whether it was new.
pub fn record_stunt(records: &mut ScrapbookRecords, stunt: &ContentId) -> bool {
    records.facts.note(Fact {
        kind: FactKind::StuntCompleted,
        subject: stunt.clone(),
    })
}

/// One page to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageView {
    /// The stable id.
    pub id: ContentId,
    /// What it is.
    pub kind: EntryKind,
    /// The title in the requested locale; `None` when the locale has none.
    pub title: Option<String>,
    /// Whether it is unlocked.
    pub unlocked: bool,
    /// The artwork id; present only when unlocked.
    pub image: Option<ContentId>,
    /// Whether a replay can be launched from it.
    pub replayable: bool,
}

/// The pages to show, in declared order. `title` resolves a title id for the
/// current locale.
pub fn project(
    catalog: &ScrapbookCatalog,
    records: &ScrapbookRecords,
    title: &impl Fn(&ContentId) -> Option<String>,
) -> Vec<PageView> {
    let has = has(records);
    catalog
        .visible(&has)
        .map(|(entry, unlocked)| PageView {
            id: entry.id.clone(),
            kind: entry.kind,
            title: title(&entry.title),
            unlocked,
            image: unlocked.then(|| entry.image.clone()).flatten(),
            replayable: unlocked && entry.replay.is_some(),
        })
        .collect()
}

/// Looks every saved id up in the catalog; the order and length match.
pub fn resolve_saved<'a>(
    catalog: &'a ScrapbookCatalog,
    saved: &[ContentId],
) -> Vec<Option<&'a ScrapbookEntry>> {
    saved.iter().map(|id| catalog.entry(id)).collect()
}

/// A request for the normal mission loading path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayRequest {
    /// The mission to load.
    pub mission: ContentId,
    /// Its variant, when the entry names one.
    pub variant: Option<ContentId>,
}

/// Why a replay or memento action was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScrapbookActionError {
    /// The entry is not in the catalog.
    UnknownEntry(ContentId),
    /// The entry is locked.
    Locked(ContentId),
    /// The entry has no replay link.
    NoReplay(ContentId),
    /// The entry is not a memento.
    NotAMemento(ContentId),
    /// The memento choice was refused.
    Memento(MementoError),
}

impl fmt::Display for ScrapbookActionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownEntry(id) => write!(f, "no scrapbook entry {id}"),
            Self::Locked(id) => write!(f, "scrapbook entry {id} is locked"),
            Self::NoReplay(id) => write!(f, "scrapbook entry {id} has no replay link"),
            Self::NotAMemento(id) => write!(f, "scrapbook entry {id} is not a memento"),
            Self::Memento(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for ScrapbookActionError {}

/// The replay request an unlocked entry offers.
///
/// # Errors
///
/// [`ScrapbookActionError`].
pub fn replay_request(
    catalog: &ScrapbookCatalog,
    records: &ScrapbookRecords,
    entry: &ContentId,
) -> Result<ReplayRequest, ScrapbookActionError> {
    let declared = catalog
        .entry(entry)
        .ok_or_else(|| ScrapbookActionError::UnknownEntry(entry.clone()))?;
    if !catalog.is_unlocked(declared, &has(records)) {
        return Err(ScrapbookActionError::Locked(entry.clone()));
    }
    let link = declared
        .replay
        .as_ref()
        .ok_or_else(|| ScrapbookActionError::NoReplay(entry.clone()))?;
    Ok(ReplayRequest {
        mission: link.mission.clone(),
        variant: link.variant.clone(),
    })
}

/// Chooses the cabin memento from the unlocked mementos.
///
/// # Errors
///
/// [`ScrapbookActionError`]; the choice is unchanged.
pub fn choose_memento(
    catalog: &ScrapbookCatalog,
    records: &mut ScrapbookRecords,
    memento: &ContentId,
) -> Result<(), ScrapbookActionError> {
    let declared = catalog
        .entry(memento)
        .ok_or_else(|| ScrapbookActionError::UnknownEntry(memento.clone()))?;
    if declared.kind != EntryKind::Memento {
        return Err(ScrapbookActionError::NotAMemento(memento.clone()));
    }
    let unlocked: BTreeSet<ContentId> = catalog.unlocked(&has(records));
    records
        .memento
        .choose(memento, &unlocked)
        .map_err(ScrapbookActionError::Memento)
}

/// Why a scrapbook change could not be persisted.
#[derive(Debug)]
pub enum PersistError {
    /// The stored scrapbook fields could not be read back. Nothing is written
    /// over them.
    Restore(RestoreError),
    /// The record submission was refused.
    Record(RecordError),
    /// The memento choice was refused.
    Action(ScrapbookActionError),
    /// The profile session failed or refused the write.
    Session(SessionError),
}

impl fmt::Display for PersistError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Restore(error) => write!(f, "stored scrapbook is unreadable: {error}"),
            Self::Record(error) => error.fmt(f),
            Self::Action(error) => error.fmt(f),
            Self::Session(error) => write!(f, "scrapbook was not saved: {error}"),
        }
    }
}

impl std::error::Error for PersistError {}

/// What a persisting call did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Persisted<T> {
    /// The change was applied and written as one whole profile revision.
    Written(T),
    /// The change was already part of the stored scrapbook; nothing was
    /// written, which is what makes replaying a result after a crash safe.
    AlreadyApplied,
}

/// Reads the scrapbook out of a profile document. A profile that never saved
/// one yields an empty scrapbook.
///
/// # Errors
///
/// [`RestoreError`].
pub fn load(document: &ProfileDocument) -> Result<ScrapbookRecords, RestoreError> {
    ScrapbookRecords::from_fields(
        document
            .extra
            .iter()
            .map(|field| (field.key.as_str(), field.value.as_str())),
    )
}

/// The scrapbook stored in the session's selected profile.
///
/// # Errors
///
/// [`PersistError`].
pub fn stored(session: &ProfileSession) -> Result<ScrapbookRecords, PersistError> {
    let document = session
        .document()
        .ok_or(PersistError::Session(SessionError::NoProfileSelected))?;
    load(document).map_err(PersistError::Restore)
}

fn refuse(error: PersistError, slot: &mut Option<PersistError>, subject: &str) -> ChangeRefusal {
    *slot = Some(error);
    ChangeRefusal {
        subject: subject.to_owned(),
        reason: ChangeRefusalReason::Malformed("scrapbook change refused".to_owned()),
    }
}

/// Applies `change` to the scrapbook stored in the profile and writes the
/// result as one whole revision. Only the `scrapbook.` extra fields are
/// replaced; campaign progression and every other field are read, not
/// rewritten. A conflicting revision re-applies `change` to the stored one.
///
/// `change` answers `Ok(Some(value))` for a change, `Ok(None)` when it was
/// already applied (nothing is written).
fn persist<T>(
    session: &mut ProfileSession,
    subject: &str,
    mut change: impl FnMut(&mut ScrapbookRecords) -> Result<Option<T>, PersistError>,
) -> Result<Persisted<T>, PersistError> {
    let mut failure = None;
    let mut answer = None;
    let written = session.commit_with(|document| {
        let mut records = load(document)
            .map_err(|error| refuse(PersistError::Restore(error), &mut failure, subject))?;
        match change(&mut records) {
            Err(error) => return Err(refuse(error, &mut failure, subject)),
            Ok(None) => {
                answer = None;
                return Err(ChangeRefusal {
                    subject: subject.to_owned(),
                    reason: ChangeRefusalReason::AlreadyApplied,
                });
            }
            Ok(Some(value)) => answer = Some(value),
        }
        document
            .extra
            .retain(|field| !field.key.starts_with(FIELD_PREFIX));
        document.extra.extend(
            records
                .to_fields()
                .into_iter()
                .map(|(key, value)| ExtraField { key, value }),
        );
        Ok(())
    });
    match written {
        Ok(_) => Ok(Persisted::Written(
            answer.expect("a written change has an answer"),
        )),
        Err(SessionError::Refused(refusal)) => match failure {
            Some(error) => Err(error),
            None if refusal.reason == ChangeRefusalReason::AlreadyApplied => {
                Ok(Persisted::AlreadyApplied)
            }
            None => Err(PersistError::Session(SessionError::Refused(refusal))),
        },
        Err(error) => Err(PersistError::Session(error)),
    }
}

/// Persists one finished mission: [`record_mission`], written atomically.
/// A replayed [`MissionResult::outcome_id`] writes nothing.
///
/// # Errors
///
/// [`PersistError`]; the stored scrapbook is unchanged.
pub fn persist_mission(
    session: &mut ProfileSession,
    result: &MissionResult,
) -> Result<Persisted<RecordReceipt>, PersistError> {
    persist(
        session,
        result.mission.as_str(),
        |records| match record_mission(records, result).map_err(PersistError::Record)? {
            RecordReceipt::AlreadyApplied => Ok(None),
            receipt => Ok(Some(receipt)),
        },
    )
}

/// Persists one completed stunt. Completing it again writes nothing.
///
/// # Errors
///
/// [`PersistError`].
pub fn persist_stunt(
    session: &mut ProfileSession,
    stunt: &ContentId,
) -> Result<Persisted<()>, PersistError> {
    persist(session, stunt.as_str(), |records| {
        Ok(record_stunt(records, stunt).then_some(()))
    })
}

/// Persists the cabin memento choice, which is checked against the mementos
/// the stored facts unlock. Choosing the memento already chosen writes nothing.
///
/// # Errors
///
/// [`PersistError`]; the stored choice is unchanged.
pub fn persist_memento(
    session: &mut ProfileSession,
    catalog: &ScrapbookCatalog,
    memento: &ContentId,
) -> Result<Persisted<()>, PersistError> {
    persist(session, memento.as_str(), |records| {
        if records.memento.chosen() == Some(memento) {
            return Ok(None);
        }
        choose_memento(catalog, records, memento)
            .map(Some)
            .map_err(PersistError::Action)
    })
}
