//! Resolution: `resolve(context, key) -> Result<ResolvedAsset, ResolveError>`.
//!
//! The lookup contract of `docs/contracts/IDENTITY-CONTENT.md`: an exact
//! origin plus the ordered attempts that produced it, with **multiple
//! equal-priority candidates failing visibly**. There is no filename
//! guessing loop and no first-wins map — spec F04 non-negotiable behavior
//! 3.
//!
//! The decision rule, in full:
//!
//! 1. Only mounts in the key's own [`MountNamespace`] are consulted;
//!    namespaces partition key spaces, so a sound mount never shadows a
//!    texture mount by accident. Mounts elsewhere are not attempts of this
//!    lookup at all.
//! 2. A mount is eligible when its [`MountScope`] admits the
//!    [`ResolveContext`]; otherwise the attempt records *why* it was
//!    skipped (`scope_mismatch`, `mod_not_opted_in`).
//! 3. Eligible mounts that hold the key are ranked by
//!    `(precedence rank, position in the mod stack)` — spec F04
//!    non-negotiable behavior 2: opt-in mods over patch overlays over
//!    mission/world-specific sources over shared sources. Registration
//!    order is never a tiebreak.
//! 4. One mount at the top rank serves the key. Two or more return
//!    [`ResolveError::Ambiguous`] carrying **both origins**. No candidate
//!    at all returns [`ResolveError::NotFound`] carrying the attempts.
//!
//! The ordering that decides step 3 is reported as
//! [`PRECEDENCE_ORDER_STATUS`] on every trace: `designed` until F04-D
//! measures original lookup behavior, never presented as measured.

use std::fmt;

use cs_types::asset_id::{
    AssetKey, MountId, PRECEDENCE_ORDER_STATUS, PrecedenceClass, ResolveContext, SourceSpan,
};
use cs_types::evidence::{ClaimStatus, ContentHash};

use crate::vfs::mount::{Mount, MountError, SkipReason};

/// How one mount ended up participating in a lookup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttemptOutcome {
    /// This mount served the key.
    Selected,
    /// The mount holds the key, but a mount at higher precedence does
    /// too — so it did not win. Its presence in the trace is what proves
    /// the choice was precedence, not chance.
    Candidate,
    /// The mount was eligible but does not hold the key.
    Miss,
    /// The mount was not eligible for this context.
    Skipped(SkipReason),
}

impl AttemptOutcome {
    /// The stable label used in traces and reports.
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Selected => "selected",
            Self::Candidate => "candidate",
            Self::Miss => "miss",
            Self::Skipped(reason) => reason.label(),
        }
    }
}

impl fmt::Display for AttemptOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Skipped(reason) => write!(f, "skipped({reason})"),
            other => f.write_str(other.label()),
        }
    }
}

/// One ordered step of a resolution: which mount was consulted, where its
/// bytes live and what it did for the key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolutionAttempt {
    /// The mount consulted.
    pub mount: MountId,
    /// Its container, so the attempt names an origin and not just an id.
    pub container: String,
    /// How strongly the mount competed.
    pub precedence: PrecedenceClass,
    /// What the mount did for this key.
    pub outcome: AttemptOutcome,
}

impl fmt::Display for ResolutionAttempt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({}) [{}] {}",
            self.mount, self.container, self.precedence, self.outcome
        )
    }
}

/// Everything a resolution saw: the attempts in descending precedence
/// order (ties by registration order) and how well that ordering is known.
///
/// A successful resolution and both failure kinds carry the same trace, so
/// a caller can always explain the answer — including *why* another
/// world's mount did not serve this context.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolutionTrace {
    /// The mounts consulted, highest precedence first.
    pub attempts: Vec<ResolutionAttempt>,
    /// Evidence status of the ordering that decided this trace. It is
    /// [`PRECEDENCE_ORDER_STATUS`] — `designed` — until F04-D measures
    /// original lookup behavior.
    pub precedence_status: ClaimStatus,
}

impl fmt::Display for ResolutionTrace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rendered: Vec<String> = self
            .attempts
            .iter()
            .map(ResolutionAttempt::to_string)
            .collect();
        write!(f, "{}", rendered.join("; "))
    }
}

/// One resolved asset: the exact origin of the bytes, as an immutable
/// [`SourceSpan`], plus the trace that justified it (spec F04,
/// "Deliverable and interfaces").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedAsset {
    /// The key that was asked for, as spelled by the caller.
    pub key: AssetKey,
    /// The mount that won.
    pub mount: MountId,
    /// The precedence class that won with it.
    pub precedence: PrecedenceClass,
    /// Where the bytes live: installation, container, member, range and
    /// digest.
    pub span: SourceSpan,
    /// The ordered attempts that produced this answer.
    pub trace: ResolutionTrace,
}

/// One of the equal-priority origins of an ambiguous key.
///
/// Carried in full because spec F04 AC02 requires a case-only or
/// same-priority collision to fail **with both origins**.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictOrigin {
    /// The mount holding the duplicate.
    pub mount: MountId,
    /// The container the duplicate lives in.
    pub container: String,
    /// The member's original spelling inside that container.
    pub member_spelling: String,
    /// The precedence class the duplicate competes at.
    pub precedence: PrecedenceClass,
    /// The duplicate's digest, when it was hashed.
    pub sha256: Option<ContentHash>,
}

impl fmt::Display for ConflictOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({} [{}], {})",
            self.mount, self.container, self.member_spelling, self.precedence
        )
    }
}

/// Why a lookup produced no single origin.
///
/// The payloads are boxed so the error itself stays small on the
/// `Result` path; pattern matching keeps the named fields, they are just
/// behind a `Box`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolveError {
    /// No eligible mount holds the key. The trace says which mounts were
    /// consulted and why each did not serve.
    NotFound {
        /// The key that was asked for.
        key: Box<AssetKey>,
        /// The attempts made.
        trace: Box<ResolutionTrace>,
    },
    /// Two or more mounts at the same effective precedence hold the key.
    /// Never flattened to a first-wins answer (non-negotiable behavior 3).
    Ambiguous {
        /// The key that was asked for.
        key: Box<AssetKey>,
        /// Every origin that tied, each with its container, spelling and
        /// digest.
        candidates: Vec<ConflictOrigin>,
        /// The attempts made.
        trace: Box<ResolutionTrace>,
    },
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { key, trace } => {
                write!(f, "no mount holds {key} in namespace {}", key.namespace())?;
                if trace.attempts.is_empty() {
                    write!(f, "; no mount serves that namespace")?;
                } else {
                    write!(f, "; attempts: {trace}")?;
                }
                Ok(())
            }
            Self::Ambiguous {
                key,
                candidates,
                trace,
            } => {
                write!(
                    f,
                    "{key} is ambiguous: {} mounts at equal precedence hold it: ",
                    candidates.len()
                )?;
                let rendered: Vec<String> =
                    candidates.iter().map(ConflictOrigin::to_string).collect();
                write!(f, "{}; attempts: {trace}", rendered.join(" vs. "))
            }
        }
    }
}

impl std::error::Error for ResolveError {}

/// One mount considered by a lookup, with the rank it competes at.
struct Considered<'a> {
    /// Index in the VFS registration order.
    index: usize,
    /// The mount itself.
    mount: &'a Mount,
    /// `(precedence rank, mod stack position)`; higher wins.
    rank: (u8, usize),
    /// What the mount did for this key.
    outcome: AttemptOutcome,
}

/// The rank a mount competes at inside one context.
///
/// Only [`PrecedenceClass::Mod`] looks at the context beyond its class:
/// mods are ordered by their position in the [`cs_types::asset_id::ModStack`],
/// a later entry outranking an earlier one. That load order is a designed
/// convention, covered by [`PRECEDENCE_ORDER_STATUS`].
fn effective_rank(mount: &Mount, context: &ResolveContext) -> (u8, usize) {
    match mount.precedence() {
        PrecedenceClass::Mod => {
            let position = mount
                .scope()
                .mod_id
                .as_ref()
                .and_then(|mod_id| context.mods.position(mod_id))
                .unwrap_or(0);
            (PrecedenceClass::Mod.rank(), position)
        }
        class => (class.rank(), 0),
    }
}

/// Builds the trace, sorted highest precedence first, marking the
/// registration index that won.
fn trace_of(mut considered: Vec<Considered<'_>>, selected: Option<usize>) -> ResolutionTrace {
    considered.sort_by(|left, right| {
        right
            .rank
            .cmp(&left.rank)
            .then(left.index.cmp(&right.index))
    });
    let attempts = considered
        .into_iter()
        .map(|entry| ResolutionAttempt {
            mount: entry.mount.id().clone(),
            container: entry.mount.container().to_owned(),
            precedence: entry.mount.precedence(),
            outcome: if selected == Some(entry.index) {
                AttemptOutcome::Selected
            } else {
                entry.outcome
            },
        })
        .collect();
    ResolutionTrace {
        attempts,
        precedence_status: PRECEDENCE_ORDER_STATUS,
    }
}

/// Builds the conflict record of a mount that holds `key`.
///
/// # Panics
///
/// Only for a mount already classified as holding `key`, which
/// [`Mount::member`] just answered.
fn origin_of(mount: &Mount, key: &AssetKey) -> ConflictOrigin {
    let member = mount
        .member(key)
        .expect("a conflict origin is a mount that holds the key");
    ConflictOrigin {
        mount: mount.id().clone(),
        container: mount.container().to_owned(),
        member_spelling: member.spelling().as_str().to_owned(),
        precedence: mount.precedence(),
        sha256: member.sha256(),
    }
}

/// The mounted sources a resolution consults.
///
/// A VFS holds mounts only; it never opens a file here. Mount lifetimes
/// belong to a content session (non-negotiable behavior 4), so the mounts
/// of one session are registered, consulted and dropped together; that
/// lifecycle is F04-C work.
#[derive(Clone, Debug, Default)]
pub struct Vfs {
    mounts: Vec<Mount>,
}

impl Vfs {
    /// An empty VFS with no mounts.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a mount.
    ///
    /// Mount ids must be unique: a trace has to point at exactly one
    /// source, so a second mount with the same id is refused rather than
    /// shadowing the first.
    pub fn mount(&mut self, mount: Mount) -> Result<(), MountError> {
        if self.mounts.iter().any(|mounted| mounted.id() == mount.id()) {
            return Err(MountError::DuplicateMountId {
                id: mount.id().clone(),
            });
        }
        self.mounts.push(mount);
        Ok(())
    }

    /// How many mounts are registered.
    pub fn len(&self) -> usize {
        self.mounts.len()
    }

    /// Whether no mount is registered.
    pub fn is_empty(&self) -> bool {
        self.mounts.is_empty()
    }

    /// `resolve(context, key)` — the lookup contract of
    /// `docs/contracts/IDENTITY-CONTENT.md`.
    ///
    /// Returns the exact origin (an immutable [`SourceSpan`]) plus the
    /// ordered attempts, or a failure that carries the same trace. The
    /// span names `context.installation`, so the answer always says which
    /// installation its bytes came from.
    pub fn resolve(
        &self,
        context: &ResolveContext,
        key: &AssetKey,
    ) -> Result<ResolvedAsset, ResolveError> {
        let mut considered: Vec<Considered<'_>> = Vec::new();
        for (index, mount) in self.mounts.iter().enumerate() {
            if mount.namespace() != key.namespace() {
                // Other namespaces are other key spaces, not attempts of
                // this lookup.
                continue;
            }
            let outcome = match mount.scope().admit(context) {
                Err(reason) => AttemptOutcome::Skipped(reason),
                Ok(()) if mount.member(key).is_some() => AttemptOutcome::Candidate,
                Ok(()) => AttemptOutcome::Miss,
            };
            considered.push(Considered {
                index,
                mount,
                rank: effective_rank(mount, context),
                outcome,
            });
        }

        let best_rank = considered
            .iter()
            .filter(|entry| entry.outcome == AttemptOutcome::Candidate)
            .map(|entry| entry.rank)
            .max();
        let winners: Vec<usize> = match best_rank {
            Some(rank) => considered
                .iter()
                .filter(|entry| entry.outcome == AttemptOutcome::Candidate && entry.rank == rank)
                .map(|entry| entry.index)
                .collect(),
            None => Vec::new(),
        };

        let selected = match winners.len() {
            0 => None,
            1 => Some(winners[0]),
            _ => {
                let candidates: Vec<ConflictOrigin> = winners
                    .iter()
                    .map(|index| origin_of(considered[*index].mount, key))
                    .collect();
                let trace = trace_of(considered, None);
                return Err(ResolveError::Ambiguous {
                    key: Box::new(key.clone()),
                    candidates,
                    trace: Box::new(trace),
                });
            }
        };

        let trace = trace_of(considered, selected);
        let Some(index) = selected else {
            return Err(ResolveError::NotFound {
                key: Box::new(key.clone()),
                trace: Box::new(trace),
            });
        };

        let mount = &self.mounts[index];
        let member = mount.member(key).expect("the selected mount holds the key");
        let span = SourceSpan::new(
            context.installation,
            mount.container(),
            Some(member.spelling().as_str()),
            member.offset(),
            member.size_bytes(),
            member.sha256(),
        )
        .expect(
            "MountBuilder validated the container spelling, the member spelling \
             and the member byte range",
        );

        Ok(ResolvedAsset {
            key: key.clone(),
            mount: mount.id().clone(),
            precedence: mount.precedence(),
            span,
            trace,
        })
    }
}
