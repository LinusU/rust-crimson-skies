//! The campaign boundary (F43-A): lower the declared
//! `cs_content::campaign` graph into the runtime `cs_sim::campaign` graph,
//! refusing every mandatory field that is still an explicit unknown.
//!
//! Spec: `specs/F43-campaign-progression-outcomes-and-economy-rules.md`,
//! stage `### F43-A`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! The lowering rule is the boundary contract used across the crate (see
//! `crate::audio`): a [`Resolved::Unknown`] on a mandatory field — a mission
//! node with no catalog binding, a reward with no measured amount — refuses
//! here, where a session can still decline the record, instead of paying or
//! routing on a guess. Interlude assets are *optional* data: an unbound
//! interlude lowers to a beat that plays nothing rather than refusing the
//! campaign.
//!
//! [`Resolved::Unknown`]: cs_types::content::Resolved::Unknown

use std::fmt;

use cs_content::campaign::{
    CampaignDefinition, CampaignNode, CampaignNodeId, EdgeCondition, NodeKind,
};
use cs_content::construction::{AircraftBlueprint, ConstructionRules, PriceBook};
use cs_sim::campaign::{
    AppliedOutcome, CampaignError, CampaignGraph, CampaignNodeKey, CampaignRunId, CampaignSnapshot,
    CampaignState, DifficultyId, EventKey, GraphError, InterludeAdvance, LoadoutWeight,
    MissionOutcome, NodeProgress, Outcome, OutcomeId, ProfileId as CampaignProfileId,
    PurchaseDraft, PurchaseReceipt, Reward, RosterGate, RuntimeEdge, RuntimeNode, RuntimeNodeKind,
    SellDraft, SellReceipt, SessionGeneration, SymbolId,
};
use cs_types::Tick;
use cs_types::content::{ContentId, Resolved};
use cs_types::profile::{ExtraField, ProfileDocument};

use crate::profile::{ChangeRefusal, ChangeRefusalReason, ProfileSession, SessionError};

/// Why a declared campaign record refused to lower.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CampaignLowerError {
    /// A mission node's catalog binding is unknown — the node cannot say
    /// which mission it is.
    UnknownMissionBinding {
        /// The node.
        node: CampaignNodeId,
        /// The recorded reason.
        reason: String,
    },
    /// An edge's reward amount is unknown — the transaction would pay a
    /// guess (spec F43 non-negotiable behavior 4).
    UnknownReward {
        /// The edge's source node.
        node: CampaignNodeId,
        /// The edge's condition.
        on: EdgeCondition,
        /// The recorded reason.
        reason: String,
    },
    /// A node id failed runtime-key validation — the lowered key must be a
    /// faithful copy, not a re-parse.
    BadNodeKey {
        /// The rejected id.
        node: CampaignNodeId,
    },
    /// The lowered graph failed its own validation — the runtime invariants
    /// match the declared ones, so this names a lowering defect, not a bad
    /// record.
    Graph {
        /// The runtime refusal.
        source: GraphError,
    },
}

impl fmt::Display for CampaignLowerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownMissionBinding { node, reason } => {
                write!(f, "campaign node {node} has no mission binding: {reason}")
            }
            Self::UnknownReward { node, on, reason } => write!(
                f,
                "the {} edge of node {node} has no measured reward: {reason}",
                on.label()
            ),
            Self::BadNodeKey { node } => {
                write!(f, "node id {node} cannot be a runtime key")
            }
            Self::Graph { source } => write!(f, "lowered graph invalid: {source}"),
        }
    }
}

impl std::error::Error for CampaignLowerError {}

/// The declared condition vocabulary maps one-to-one onto the mission
/// runtime's terminal outcomes.
fn lower_condition(on: EdgeCondition) -> Outcome {
    match on {
        EdgeCondition::Victory => Outcome::Succeeded,
        EdgeCondition::Defeat => Outcome::Failed,
        EdgeCondition::Abort => Outcome::Aborted,
    }
}

fn lower_key(id: &CampaignNodeId) -> Result<CampaignNodeKey, CampaignLowerError> {
    CampaignNodeKey::new(id.as_str())
        .map_err(|_| CampaignLowerError::BadNodeKey { node: id.clone() })
}

fn lower_node(node: &CampaignNode) -> Result<RuntimeNode, CampaignLowerError> {
    let kind = match &node.kind {
        NodeKind::Mission { mission } => {
            let mission = match mission {
                Resolved::Known(known) => known.value.clone(),
                Resolved::Unknown { reason, .. } => {
                    return Err(CampaignLowerError::UnknownMissionBinding {
                        node: node.id.clone(),
                        reason: reason.clone(),
                    });
                }
            };
            RuntimeNodeKind::Mission { mission }
        }
        // An unbound interlude asset is optional content, not a refusal.
        NodeKind::Interlude { .. } => RuntimeNodeKind::Interlude,
        NodeKind::Ending => RuntimeNodeKind::Ending,
    };
    let mut edges = Vec::with_capacity(node.edges.len());
    for edge in &node.edges {
        let grant = match &edge.grant {
            Some(spec) => {
                let currency = match &spec.currency {
                    Resolved::Known(known) => known.value,
                    Resolved::Unknown { reason, .. } => {
                        return Err(CampaignLowerError::UnknownReward {
                            node: node.id.clone(),
                            on: edge.on,
                            reason: reason.clone(),
                        });
                    }
                };
                Reward {
                    currency,
                    unlocks: spec.unlocks.clone(),
                }
            }
            None => Reward::default(),
        };
        edges.push(RuntimeEdge {
            on: lower_condition(edge.on),
            to: lower_key(&edge.to)?,
            grant,
        });
    }
    Ok(RuntimeNode {
        id: lower_key(&node.id)?,
        kind,
        edges,
    })
}

/// Resolves the weight verdict input of a purchase draft: the exact mass of
/// `loadout` (the blueprint the purchase would produce) against the ceiling of
/// `rules`, both read from the F44-A declared records.
///
/// `cs_sim` cannot depend on `cs_content`, so the campaign transaction is
/// handed plain integers (`cs_sim::campaign::LoadoutWeight`) and this boundary
/// is where the content types are read. Any unknown — an unmeasured limit or
/// component mass, an unpriced component, a mismatched airframe or an
/// overflowing total — becomes [`LoadoutWeight::Unknown`] carrying the named
/// budget refusal, so it can never pass as a loadout that fits. The numbers
/// are only as good as the records: no original mass or ceiling is measured
/// yet (F44-D).
#[must_use]
pub fn loadout_weight(
    rules: &ConstructionRules,
    book: &PriceBook,
    loadout: &AircraftBlueprint,
) -> LoadoutWeight {
    match rules.assess(loadout, book) {
        Ok(assessment) => match rules.max_mass().clone().known() {
            Some(limit) => LoadoutWeight::Measured {
                total: assessment.totals().mass().as_units(),
                limit: limit.as_units(),
            },
            // `assess` refuses an unknown limit, so this is unreachable in
            // practice; it is still answered as unknown rather than assumed.
            None => LoadoutWeight::Unknown {
                reason: "the weight ceiling is unmeasured".to_owned(),
            },
        },
        Err(refusal) => LoadoutWeight::Unknown {
            reason: refusal.to_string(),
        },
    }
}

/// Lowers a declared campaign into the validated runtime graph. The first
/// refusal wins; a half-lowered graph is never produced.
///
/// # Errors
///
/// [`CampaignLowerError`].
pub fn lower_campaign(declared: &CampaignDefinition) -> Result<CampaignGraph, CampaignLowerError> {
    let mut nodes = Vec::with_capacity(declared.nodes().count());
    for node in declared.nodes() {
        nodes.push(lower_node(node)?);
    }
    let entry = lower_key(declared.entry())?;
    let mut roster = Vec::with_capacity(declared.roster().len());
    for gate in declared.roster() {
        roster.push(RosterGate {
            item: gate.item.clone(),
            gate: lower_key(&gate.available_from)?,
        });
    }
    CampaignGraph::try_new(nodes, entry, roster)
        .map_err(|source| CampaignLowerError::Graph { source })
}

// ---------------------------------------------------------------------------
// Save transitions (F43-C)
// ---------------------------------------------------------------------------
//
// The campaign's progress lives in the F48 profile document as one whole
// revision. A transition (a result screen's outcome, the walk across a
// briefing, a loadout purchase or sale) is computed on a *copy* of the state
// and the copy becomes the live state only after the profile commit returned:
// a refused or failed save leaves the in-memory run exactly as the disk has
// it, and a crash lands on either the whole old or the whole new revision (the
// F48 atomic write), never a mixture. A replayed result after a crash carries
// the same `OutcomeId`, which the restored ledger already holds.

/// The extra-field key prefix every persisted campaign field uses.
const PREFIX: &str = "campaign.";
const META_KEY: &str = "campaign.meta";
const FORMAT: &str = "v1";
const SEP: char = ';';

/// Why a campaign could not be read from, written to or committed to a save.
#[derive(Debug)]
pub enum CampaignSaveError {
    /// The transition itself was refused; nothing was written.
    Campaign(CampaignError),
    /// The profile session refused or failed the commit.
    Session(SessionError),
    /// The saved campaign fields are damaged or from another format.
    Corrupt(String),
    /// The save names a different profile or run than the one being resumed.
    WrongRun {
        /// What the save holds.
        saved: String,
    },
    /// The stored campaign moved on since this run last read it: another
    /// writer committed first. Nothing was written; reload before retrying.
    Stale {
        /// The revision this run's transition was computed from.
        expected: u64,
        /// The revision the store holds.
        stored: u64,
    },
    /// A field would not survive the save's text format.
    Unrepresentable(String),
}

impl fmt::Display for CampaignSaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Campaign(error) => write!(f, "{error}"),
            Self::Session(error) => write!(f, "campaign save failed: {error}"),
            Self::Corrupt(reason) => write!(f, "saved campaign is damaged: {reason}"),
            Self::WrongRun { saved } => write!(f, "the save holds another campaign run: {saved}"),
            Self::Stale { expected, stored } => write!(
                f,
                "campaign moved to revision {stored} while this run held {expected}; reload"
            ),
            Self::Unrepresentable(what) => write!(f, "cannot be saved: {what}"),
        }
    }
}

impl std::error::Error for CampaignSaveError {}

impl From<CampaignError> for CampaignSaveError {
    fn from(error: CampaignError) -> Self {
        Self::Campaign(error)
    }
}

fn field(text: &str, what: &str) -> Result<(), CampaignSaveError> {
    if text.contains(SEP) {
        return Err(CampaignSaveError::Unrepresentable(format!(
            "{what} {text:?} contains {SEP:?}"
        )));
    }
    Ok(())
}

fn outcome_letter(outcome: Outcome) -> char {
    match outcome {
        Outcome::Succeeded => 'S',
        Outcome::Failed => 'F',
        Outcome::Aborted => 'A',
    }
}

fn outcome_from_letter(text: &str) -> Option<Outcome> {
    match text {
        "S" => Some(Outcome::Succeeded),
        "F" => Some(Outcome::Failed),
        "A" => Some(Outcome::Aborted),
        _ => None,
    }
}

fn encode_outcome_id(id: &OutcomeId) -> String {
    let key = id.terminal_event;
    [
        id.profile.as_str().to_owned(),
        id.run.as_str().to_owned(),
        id.session.0.to_string(),
        key.session.0.to_string(),
        key.tick.0.to_string(),
        key.source.0.to_string(),
        key.sequence.to_string(),
    ]
    .join(&SEP.to_string())
}

fn corrupt<T>(reason: impl Into<String>) -> Result<T, CampaignSaveError> {
    Err(CampaignSaveError::Corrupt(reason.into()))
}

fn number<T: std::str::FromStr>(text: &str, what: &str) -> Result<T, CampaignSaveError> {
    text.parse()
        .or_else(|_| corrupt(format!("{what} {text:?} is not a number")))
}

fn decode_outcome_id(value: &str) -> Result<OutcomeId, CampaignSaveError> {
    let parts: Vec<&str> = value.split(SEP).collect();
    let [profile, run, session, ev_session, tick, source, sequence] = parts[..] else {
        return corrupt(format!("applied outcome {value:?} has the wrong shape"));
    };
    Ok(OutcomeId {
        profile: CampaignProfileId::new(profile)
            .or_else(|e| corrupt(format!("applied outcome profile: {e}")))?,
        run: CampaignRunId::new(run).or_else(|e| corrupt(format!("applied outcome run: {e}")))?,
        session: SessionGeneration(number(session, "session")?),
        terminal_event: EventKey {
            session: SessionGeneration(number(ev_session, "event session")?),
            tick: Tick(number(tick, "tick")?),
            source: SymbolId(number(source, "source")?),
            sequence: number(sequence, "sequence")?,
        },
    })
}

/// Writes `snapshot` into `document`'s campaign fields, replacing whatever
/// campaign fields it held. Other extra fields are kept verbatim.
///
/// # Errors
///
/// [`CampaignSaveError::Unrepresentable`] for text the format cannot carry.
pub fn write_snapshot(
    document: &mut ProfileDocument,
    snapshot: &CampaignSnapshot,
) -> Result<(), CampaignSaveError> {
    field(snapshot.profile.as_str(), "profile")?;
    let mut extra: Vec<ExtraField> = Vec::new();
    let mut push = |key: String, value: String| extra.push(ExtraField { key, value });
    push(
        META_KEY.to_owned(),
        [
            FORMAT.to_owned(),
            snapshot.profile.as_str().to_owned(),
            snapshot.run.as_str().to_owned(),
            snapshot.difficulty.as_str().to_owned(),
            snapshot.current.as_str().to_owned(),
            snapshot.revision.to_string(),
            u8::from(snapshot.modified).to_string(),
        ]
        .join(&SEP.to_string()),
    );
    for (index, (node, progress)) in snapshot.progress.iter().enumerate() {
        let latest = match progress.latest {
            Some((outcome, score)) => format!("{}{score}", outcome_letter(outcome)),
            None => "-".to_owned(),
        };
        push(
            format!("{PREFIX}node.{index}"),
            format!(
                "{node};{};{};{};{};{latest}",
                progress.victories, progress.defeats, progress.aborts, progress.best_score
            ),
        );
    }
    for (index, item) in snapshot.unlocks.iter().enumerate() {
        field(item.as_str(), "unlock")?;
        push(format!("{PREFIX}unlock.{index}"), item.as_str().to_owned());
    }
    for (index, (item, price)) in snapshot.paid.iter().enumerate() {
        push(
            format!("{PREFIX}paid.{index}"),
            format!("{};{price}", item.as_str()),
        );
    }
    for (index, id) in snapshot.applied.iter().enumerate() {
        push(format!("{PREFIX}applied.{index}"), encode_outcome_id(id));
    }
    document.extra.retain(|held| !held.key.starts_with(PREFIX));
    document.extra.extend(extra);
    document.campaign.run_id = Some(snapshot.run.as_str().to_owned());
    document.campaign.money_minor = snapshot.currency;
    Ok(())
}

/// Reads the campaign `document` holds: `None` when it holds none.
///
/// # Errors
///
/// [`CampaignSaveError::Corrupt`] for any field that does not parse.
pub fn read_snapshot(
    document: &ProfileDocument,
) -> Result<Option<CampaignSnapshot>, CampaignSaveError> {
    let Some(meta) = document.extra.iter().find(|held| held.key == META_KEY) else {
        return Ok(None);
    };
    let parts: Vec<&str> = meta.value.split(SEP).collect();
    let [
        format,
        profile,
        run,
        difficulty,
        current,
        revision,
        modified,
    ] = parts[..]
    else {
        return corrupt("the campaign header has the wrong shape");
    };
    if format != FORMAT {
        return corrupt(format!("unknown campaign format {format:?}"));
    }
    let mut snapshot = CampaignSnapshot {
        profile: CampaignProfileId::new(profile).or_else(|e| corrupt(format!("profile: {e}")))?,
        run: CampaignRunId::new(run).or_else(|e| corrupt(format!("run: {e}")))?,
        difficulty: DifficultyId::new(difficulty)
            .or_else(|e| corrupt(format!("difficulty: {e}")))?,
        current: CampaignNodeKey::new(current).or_else(|e| corrupt(format!("node: {e}")))?,
        progress: Vec::new(),
        currency: document.campaign.money_minor,
        unlocks: Vec::new(),
        paid: Vec::new(),
        applied: Vec::new(),
        revision: number(revision, "revision")?,
        modified: match modified {
            "0" => false,
            "1" => true,
            other => return corrupt(format!("modified flag {other:?}")),
        },
    };
    if document.campaign.run_id.as_deref() != Some(run) {
        return corrupt("the profile's run id disagrees with the campaign header");
    }
    for held in &document.extra {
        let Some(rest) = held.key.strip_prefix(PREFIX) else {
            continue;
        };
        let kind = rest.split('.').next().unwrap_or_default();
        let value = held.value.as_str();
        match kind {
            "meta" => {}
            "node" => {
                let parts: Vec<&str> = value.split(SEP).collect();
                let [node, victories, defeats, aborts, best, latest] = parts[..] else {
                    return corrupt(format!("node record {value:?} has the wrong shape"));
                };
                let latest = if latest == "-" {
                    None
                } else {
                    let (letter, score) = latest.split_at(latest.len().min(1));
                    let Some(outcome) = outcome_from_letter(letter) else {
                        return corrupt(format!("latest result {latest:?}"));
                    };
                    Some((outcome, number(score, "latest score")?))
                };
                snapshot.progress.push((
                    CampaignNodeKey::new(node).or_else(|e| corrupt(format!("node: {e}")))?,
                    NodeProgress {
                        victories: number(victories, "victories")?,
                        defeats: number(defeats, "defeats")?,
                        aborts: number(aborts, "aborts")?,
                        best_score: number(best, "best score")?,
                        latest,
                    },
                ));
            }
            "unlock" => snapshot.unlocks.push(
                ContentId::parse(value).or_else(|e| corrupt(format!("unlock {value:?}: {e}")))?,
            ),
            "paid" => {
                let Some((item, price)) = value.split_once(SEP) else {
                    return corrupt(format!("paid record {value:?} has the wrong shape"));
                };
                snapshot.paid.push((
                    ContentId::parse(item).or_else(|e| corrupt(format!("paid {item:?}: {e}")))?,
                    number(price, "price")?,
                ));
            }
            "applied" => snapshot.applied.push(decode_outcome_id(value)?),
            other => return corrupt(format!("unknown campaign field {other:?}")),
        }
    }
    Ok(Some(snapshot))
}

/// One campaign run over a profile session: the graph it plays, the live
/// state, and the save transitions between them.
#[derive(Debug)]
pub struct CampaignRun {
    graph: CampaignGraph,
    state: CampaignState,
}

impl CampaignRun {
    /// Resumes the run the selected profile saved, or begins `run` at the
    /// graph's entry and writes that first revision, so a profile that has
    /// started a campaign always has one on disk.
    ///
    /// # Errors
    ///
    /// [`CampaignSaveError`]: a damaged save, or a save of a different run.
    /// A save that names another run is refused, never overwritten.
    pub fn open(
        session: &mut ProfileSession,
        graph: CampaignGraph,
        profile: &CampaignProfileId,
        run: &CampaignRunId,
        difficulty: &DifficultyId,
    ) -> Result<Self, CampaignSaveError> {
        let document = session
            .document()
            .ok_or(CampaignSaveError::Session(SessionError::NoProfileSelected))?;
        if let Some(snapshot) = read_snapshot(document)? {
            if snapshot.profile != *profile || snapshot.run != *run {
                return Err(CampaignSaveError::WrongRun {
                    saved: format!("{} / {}", snapshot.profile, snapshot.run),
                });
            }
            let state = CampaignState::restore(&graph, snapshot)?;
            return Ok(Self { graph, state });
        }
        let mut fresh = Self {
            state: CampaignState::begin(profile.clone(), run.clone(), difficulty.clone(), &graph),
            graph,
        };
        let state = fresh.state.clone();
        fresh.persist(session, state, None)?;
        Ok(fresh)
    }

    /// The live state, as last committed.
    pub fn state(&self) -> &CampaignState {
        &self.state
    }

    /// The graph the run plays.
    pub fn graph(&self) -> &CampaignGraph {
        &self.graph
    }

    /// Commits `next` (computed from `base`, the revision this run held) and
    /// only then adopts it.
    fn persist(
        &mut self,
        session: &mut ProfileSession,
        next: CampaignState,
        base: Option<u64>,
    ) -> Result<(), CampaignSaveError> {
        let snapshot = next.snapshot();
        let mut failure: Option<CampaignSaveError> = None;
        let committed = session.commit_with(|document| {
            let refuse = |reason: &str| ChangeRefusal {
                subject: "campaign".to_owned(),
                reason: ChangeRefusalReason::Malformed(reason.to_owned()),
            };
            // A conflict retry re-reads the store: if another writer moved the
            // campaign, applying this transition over it would overwrite
            // progression this run never saw.
            match read_snapshot(document) {
                Ok(stored) => {
                    let stored = stored.map(|held| held.revision);
                    if stored != base {
                        failure = Some(CampaignSaveError::Stale {
                            expected: base.unwrap_or(0),
                            stored: stored.unwrap_or(0),
                        });
                        return Err(refuse("the stored campaign moved"));
                    }
                }
                Err(error) => {
                    failure = Some(error);
                    return Err(refuse("the stored campaign is damaged"));
                }
            }
            if let Err(error) = write_snapshot(document, &snapshot) {
                failure = Some(error);
                return Err(refuse("the campaign cannot be represented"));
            }
            Ok(())
        });
        match committed {
            Ok(_) => {
                self.state = next;
                Ok(())
            }
            Err(SessionError::Refused(_)) if failure.is_some() => {
                Err(failure.expect("checked just above"))
            }
            Err(error) => Err(CampaignSaveError::Session(error)),
        }
    }

    /// Runs one transition on a copy of the state and commits it. A transition
    /// that changed nothing (a replayed outcome, a walk with no interlude to
    /// cross) writes nothing.
    fn transact<T>(
        &mut self,
        session: &mut ProfileSession,
        transition: impl FnOnce(&mut CampaignState, &CampaignGraph) -> Result<T, CampaignError>,
    ) -> Result<T, CampaignSaveError> {
        let mut next = self.state.clone();
        let value = transition(&mut next, &self.graph)?;
        if next.revision() == self.state.revision() {
            return Ok(value);
        }
        let base = self.state.revision();
        self.persist(session, next, Some(base))?;
        Ok(value)
    }

    /// The results screen's transition: applies a finished mission's outcome
    /// and saves it as one revision. Replaying an applied outcome (after a
    /// crash before the screen acknowledged) reports
    /// `OutcomeReceipt::AlreadyApplied` and pays nothing.
    ///
    /// # Errors
    ///
    /// [`CampaignSaveError`]; on any error the run and the save are unchanged.
    pub fn report_outcome(
        &mut self,
        session: &mut ProfileSession,
        outcome: &MissionOutcome,
    ) -> Result<AppliedOutcome, CampaignSaveError> {
        self.transact(session, |state, graph| state.apply_outcome(graph, outcome))
    }

    /// The briefing/cutscene transition: crosses the selected interlude beats
    /// onto the next mission and saves the crossing (and any grant on it).
    ///
    /// # Errors
    ///
    /// [`CampaignSaveError`].
    pub fn advance_interludes(
        &mut self,
        session: &mut ProfileSession,
    ) -> Result<InterludeAdvance, CampaignSaveError> {
        self.transact(session, |state, graph| state.advance_interludes(graph))
    }

    /// The loadout screen's purchase, saved with the balance it charges.
    ///
    /// # Errors
    ///
    /// [`CampaignSaveError`].
    pub fn purchase(
        &mut self,
        session: &mut ProfileSession,
        draft: &PurchaseDraft,
    ) -> Result<PurchaseReceipt, CampaignSaveError> {
        self.transact(session, |state, graph| state.purchase(graph, draft))
    }

    /// The loadout screen's sale, saved with the refund it credits.
    ///
    /// # Errors
    ///
    /// [`CampaignSaveError`].
    pub fn sell(
        &mut self,
        session: &mut ProfileSession,
        draft: &SellDraft,
    ) -> Result<SellReceipt, CampaignSaveError> {
        self.transact(session, |state, _| state.sell(draft))
    }
}
