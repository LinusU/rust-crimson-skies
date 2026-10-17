//! The persistence boundary of the Instant Action record scope (F49-C1).
//!
//! Spec: `specs/F49-instant-action-presets-and-custom-scenarios.md`, shared
//! contract `docs/contracts/STATE-TRANSACTIONS.md` ("Persistence", "Persistent
//! profile data receives only an explicit outcome transaction").
//!
//! A settled session is one `extra` field of the profile document, keyed
//! `ia.record.<generation>`, written through [`ProfileSession::commit_with`] as
//! one whole revision. The campaign's own fields (`campaign.*` extras, the
//! applied-outcome ledger, currency) are never read or written here, so an
//! Instant Action settlement cannot move campaign progression (F49
//! non-negotiable 3). The profile that owns a record is the document's own
//! profile, so a record cannot be filed under another pilot's save.
//!
//! The field layout (`subject;seed;result;ended_tick;attempt`) is this build's
//! own designed vocabulary; the original game's record format is unmeasured
//! (F49-D).

use std::fmt;

use cs_content::instant_action::ScenarioSeed;
use cs_script::runtime::SessionGeneration;
use cs_types::content::ContentId;
use cs_types::profile::{ExtraField, ProfileDocument, Revision};

use super::{IaRecordBook, IaRecordEntry, IaRecordError, ScenarioResult};
use crate::campaign::campaign_profile_id;
use crate::profile::{ChangeRefusal, ChangeRefusalReason, ProfileSession, SessionError};

/// The extra-field key prefix every persisted Instant Action record uses.
const PREFIX: &str = "ia.record.";

/// Why an Instant Action record could not be read or written.
#[derive(Debug)]
pub enum IaPersistError {
    /// The record scope refused the entry (another profile, or a generation
    /// already stored). Nothing was written.
    Record(IaRecordError),
    /// The profile session refused or failed the commit.
    Session(SessionError),
    /// A stored `ia.record.*` field could not be read back. The field is
    /// reported, never skipped, so a damaged record is not silently dropped.
    Damaged {
        /// The extra-field key.
        key: String,
        /// What was wrong with it.
        reason: String,
    },
}

impl fmt::Display for IaPersistError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Record(error) => write!(f, "{error}"),
            Self::Session(error) => write!(f, "{error}"),
            Self::Damaged { key, reason } => {
                write!(f, "stored Instant Action record {key} is damaged: {reason}")
            }
        }
    }
}

impl std::error::Error for IaPersistError {}

impl From<SessionError> for IaPersistError {
    fn from(error: SessionError) -> Self {
        Self::Session(error)
    }
}

fn key_for(generation: SessionGeneration) -> String {
    format!("{PREFIX}{}", generation.0)
}

fn encode(entry: &IaRecordEntry) -> ExtraField {
    ExtraField {
        key: key_for(entry.generation()),
        value: format!(
            "{};{};{};{};{}",
            entry.subject(),
            entry.seed().root(),
            entry.result().label(),
            entry.ended_tick(),
            entry.attempt()
        ),
    }
}

fn decode(
    document: &ProfileDocument,
    field: &ExtraField,
    generation: SessionGeneration,
) -> Result<IaRecordEntry, IaPersistError> {
    let damaged = |reason: &str| IaPersistError::Damaged {
        key: field.key.clone(),
        reason: reason.to_owned(),
    };
    let parts: Vec<&str> = field.value.split(';').collect();
    let [subject, seed, result, ended, attempt] = parts[..] else {
        return Err(damaged("expected five ';'-separated parts"));
    };
    let subject = ContentId::parse(subject).map_err(|_| damaged("subject is not a content id"))?;
    let seed = ScenarioSeed::new(seed.parse().map_err(|_| damaged("seed is not a number"))?);
    let result = match result {
        "victory" => ScenarioResult::Victory,
        "defeat" => ScenarioResult::Defeat,
        "draw" => ScenarioResult::Draw,
        _ => return Err(damaged("result is not a known label")),
    };
    let ended = ended
        .parse()
        .map_err(|_| damaged("ended tick is not a number"))?;
    let attempt = attempt
        .parse()
        .map_err(|_| damaged("attempt is not a number"))?;
    Ok(IaRecordEntry::from_parts(
        campaign_profile_id(document.kind, document.profile_id),
        generation,
        subject,
        seed,
        result,
        ended,
        attempt,
    ))
}

/// Reads the Instant Action record scope out of `document`, oldest generation
/// first.
///
/// # Errors
///
/// [`IaPersistError::Damaged`] for a stored field that does not parse.
pub fn read_records(document: &ProfileDocument) -> Result<IaRecordBook, IaPersistError> {
    let profile = campaign_profile_id(document.kind, document.profile_id);
    let mut entries = Vec::new();
    for field in &document.extra {
        let Some(number) = field.key.strip_prefix(PREFIX) else {
            continue;
        };
        let generation =
            number
                .parse()
                .map(SessionGeneration)
                .map_err(|_| IaPersistError::Damaged {
                    key: field.key.clone(),
                    reason: "generation is not a number".to_owned(),
                })?;
        entries.push(decode(document, field, generation)?);
    }
    entries.sort_by_key(IaRecordEntry::generation);
    let mut book = IaRecordBook::new(profile);
    for entry in entries {
        book.record(entry).map_err(IaPersistError::Record)?;
    }
    Ok(book)
}

impl ProfileSession {
    /// The selected profile's stored Instant Action records.
    ///
    /// # Errors
    ///
    /// [`SessionError::NoProfileSelected`] (as [`IaPersistError::Session`]), or
    /// [`IaPersistError::Damaged`].
    pub fn instant_action_records(&self) -> Result<IaRecordBook, IaPersistError> {
        let document = self.document().ok_or(SessionError::NoProfileSelected)?;
        read_records(document)
    }

    /// Settles one finished Instant Action session into the selected profile
    /// as one whole revision, touching no campaign field.
    ///
    /// Idempotent per session generation: a generation already stored is
    /// refused as [`IaRecordError::DuplicateGeneration`], and an entry that
    /// names another profile as [`IaRecordError::ForeignProfile`]. Both checks
    /// run inside the commit's change, so they hold after a conflict retry too.
    ///
    /// # Errors
    ///
    /// [`IaPersistError::Record`] for a refused entry (nothing written);
    /// [`IaPersistError::Session`] when the commit fails;
    /// [`IaPersistError::Damaged`] when a stored record cannot be read.
    pub fn record_instant_action(
        &mut self,
        entry: &IaRecordEntry,
    ) -> Result<Revision, IaPersistError> {
        let mut refused: Option<IaPersistError> = None;
        let result = self.commit_with(|document| {
            let mut book = match read_records(document) {
                Ok(book) => book,
                Err(error) => return Err(refuse(&mut refused, entry, error)),
            };
            if let Err(error) = book.record(entry.clone()) {
                return Err(refuse(&mut refused, entry, IaPersistError::Record(error)));
            }
            document.extra.push(encode(entry));
            Ok(())
        });
        match result {
            Ok(revision) => Ok(revision),
            Err(SessionError::Refused(_)) if refused.is_some() => {
                Err(refused.take().expect("checked above"))
            }
            Err(error) => Err(IaPersistError::Session(error)),
        }
    }
}

fn refuse(
    slot: &mut Option<IaPersistError>,
    entry: &IaRecordEntry,
    error: IaPersistError,
) -> ChangeRefusal {
    let refusal = ChangeRefusal {
        subject: key_for(entry.generation()),
        reason: ChangeRefusalReason::Malformed(error.to_string()),
    };
    *slot = Some(error);
    refusal
}
