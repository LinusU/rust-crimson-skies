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
//! 3. Eligible mounts that hold the key are ranked, and the rank decides:
//!    * [`LookupOrder::Precedence`] — the designed order of
//!      [`PrecedenceClass`], spec F04 non-negotiable behavior 2 (opt-in mods
//!      over patch overlays over mission/world-specific sources over shared
//!      sources). Two or more at the top rank return
//!      [`ResolveError::Ambiguous`] carrying **both origins**;
//!    * [`LookupOrder::GosRegistration`] — the original's **registration
//!      order**, which is the only rule `MetaOpenFile` has (task #686, see
//!      [`crate::vfs::gos`]): the sources are consulted in the order they
//!      were registered and the **first that holds the name wins**.
//!      [`crate::vfs::gos::SessionBuilder::mount_gos_chain`] registers them
//!      in that order; nothing else may reorder them.
//! 4. No candidate at all returns [`ResolveError::NotFound`] carrying the
//!    attempts.
//!
//! Which order decided a lookup is reported on every trace as
//! [`LookupOrder`] plus its [`ClaimStatus`]: the designed order is
//! [`PRECEDENCE_ORDER_STATUS`] (`designed`, until original lookup behavior
//! is measured) and the GOS registration order is
//! [`crate::vfs::gos::GOS_ORDER_STATUS`] (`inferred`, code-derived). The
//! former is why [`Vfs::resolve_blocking_unmeasured`] — the lookup content
//! sessions use — refuses any retail answer that the designed order alone
//! decided between different bytes; the latter is not designed, so a GOS
//! answer is served and reports what it rests on.

use std::fmt;
use std::sync::Arc;

use cs_types::asset_id::{
    AssetKey, MountId, MountNamespace, PRECEDENCE_ORDER_STATUS, PrecedenceClass, ResolveContext,
    SourceSpan,
};
use cs_types::evidence::{ClaimStatus, ContentHash};

use crate::vfs::gos::{GOS_NAMESPACE, GOS_ORDER_STATUS, GosNameMatch};
use crate::vfs::mount::{MemberRecord, Mount, MountError, SkipReason};
use crate::vfs::source::{self, ReadError};

/// The order one key space resolves its keys by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LookupOrder {
    /// The designed [`PrecedenceClass`] order of spec F04 non-negotiable
    /// behavior 2. Status [`PRECEDENCE_ORDER_STATUS`]: `designed`, because
    /// no original lookup order has been measured for these key spaces.
    Precedence,
    /// The original's GOS **registration order**: the sources
    /// `roffile.dll`'s `AddNewROFDirectory` pushed back, walked in that
    /// order by `MetaOpenFile`, first hit wins (task #686,
    /// `docs/findings/2026-10-05-f04-d-original-lookup-order.md` section D).
    /// Status [`GOS_ORDER_STATUS`]: `inferred` from static analysis of the
    /// original executable and `roffile.dll` — not a runtime capture of the
    /// original running.
    GosRegistration,
}

impl LookupOrder {
    /// The order a lookup in `namespace` is decided by: the GOS
    /// registration order for [`GOS_NAMESPACE`], the designed precedence
    /// order everywhere else.
    pub fn for_namespace(namespace: &MountNamespace) -> Self {
        if namespace.as_str() == GOS_NAMESPACE {
            Self::GosRegistration
        } else {
            Self::Precedence
        }
    }

    /// How well this order is known today.
    pub const fn status(self) -> ClaimStatus {
        match self {
            Self::Precedence => PRECEDENCE_ORDER_STATUS,
            Self::GosRegistration => GOS_ORDER_STATUS,
        }
    }

    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Precedence => "precedence",
            Self::GosRegistration => "gos_registration",
        }
    }
}

impl fmt::Display for LookupOrder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// An order and how well it is known: what a trace or a report carries so a
/// reader can see what an answer rests on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LookupOrderStatus {
    /// Which order decided.
    pub order: LookupOrder,
    /// How well that order is known ([`LookupOrder::status`]).
    pub status: ClaimStatus,
}

impl LookupOrderStatus {
    /// The pair for `order`.
    pub const fn of(order: LookupOrder) -> Self {
        Self {
            order,
            status: order.status(),
        }
    }
}

impl fmt::Display for LookupOrderStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.order, self.status.label())
    }
}

/// How one mount ended up participating in a lookup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttemptOutcome {
    /// This mount served the key.
    Selected,
    /// The mount holds the key, but a mount earlier in the deciding order
    /// does too — so it did not win. Its presence in the trace is what
    /// proves the choice was the order, not chance.
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

/// Everything a resolution saw: the attempts in the order that decided
/// them, and how well that ordering is known.
///
/// A successful resolution and every failure kind carry the same trace, so
/// a caller can always explain the answer — including *why* another
/// world's mount did not serve this context.
///
/// The attempts are ordered by [`ResolutionTrace::order`]: descending
/// precedence with registration order as the tiebreak for
/// [`LookupOrder::Precedence`], registration order alone for
/// [`LookupOrder::GosRegistration`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolutionTrace {
    /// The mounts consulted, in deciding order.
    pub attempts: Vec<ResolutionAttempt>,
    /// The order that decided this lookup, and how well it is known.
    ///
    /// `PRECEDENCE_ORDER_STATUS` — `designed` — for
    /// [`LookupOrder::Precedence`]; `GOS_ORDER_STATUS` — `inferred` — for
    /// [`LookupOrder::GosRegistration`]. Neither is
    /// `verified_original`: neither order was measured by running the
    /// original.
    pub order: LookupOrderStatus,
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
    /// A retail mount won over other retail mounts that hold different
    /// (or unhashed) bytes for the key, and the only thing that decided
    /// between them is the precedence order — which is still `designed`,
    /// not measured. Spec F04 non-negotiable behavior 2: "label the
    /// baseline order designed and block conflicting retail resolutions".
    /// Returned by [`Vfs::resolve_blocking_unmeasured`] only.
    UnmeasuredOrder {
        /// The key that was asked for.
        key: Box<AssetKey>,
        /// The origin the designed order would have served.
        selected: Box<ConflictOrigin>,
        /// Every lower-ranked origin with different or unknown bytes.
        shadowed: Vec<ConflictOrigin>,
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
            Self::UnmeasuredOrder {
                key,
                selected,
                shadowed,
                trace,
            } => {
                let rendered: Vec<String> =
                    shadowed.iter().map(ConflictOrigin::to_string).collect();
                write!(
                    f,
                    "{key} is blocked: {selected} would shadow {} with different bytes, \
                     and the precedence order that decides it is {}, not measured; \
                     attempts: {trace}",
                    rendered.join(", "),
                    trace.order.status.label()
                )
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

/// Builds the trace, ordered by the rule that decided it, marking the
/// registration index that won.
///
/// [`LookupOrder::Precedence`] sorts highest precedence first and breaks a
/// tie by registration order; [`LookupOrder::GosRegistration`] sorts by
/// registration order alone, because that is the whole rule.
fn trace_of(
    mut considered: Vec<Considered<'_>>,
    selected: Option<usize>,
    order: LookupOrder,
) -> ResolutionTrace {
    match order {
        LookupOrder::Precedence => considered.sort_by(|left, right| {
            right
                .rank
                .cmp(&left.rank)
                .then(left.index.cmp(&right.index))
        }),
        LookupOrder::GosRegistration => considered.sort_by_key(|entry| entry.index),
    }
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
        order: LookupOrderStatus::of(order),
    }
}

/// Builds the conflict record of a mount that holds `key` under `matching`.
///
/// # Panics
///
/// Only for a mount already classified as holding `key`, which
/// [`Vfs::serving_member`] just answered.
fn origin_of(mount: &Mount, key: &AssetKey, matching: GosNameMatch) -> ConflictOrigin {
    let member = match matching {
        GosNameMatch::AsciiInsensitive => mount.member(key),
        GosNameMatch::ExactSpelling => mount.member_spelled_exactly(key),
    }
    .expect("a conflict origin is a mount that holds the key under the matching rule");
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
/// Resolution never opens a file; [`Vfs::read_range`] and
/// [`Vfs::read_all`] read the bytes of a resolution from its mount's host
/// source, read-only. Mount lifetimes belong to a content session
/// (non-negotiable behavior 4): [`crate::vfs::session::ContentSession`]
/// owns one `Vfs`, and its mounts are registered, consulted and dropped
/// together. Mounts are shared (`Arc`) so a read that is still in flight
/// when the session closes keeps its own mount description alive instead
/// of pointing into freed state.
#[derive(Clone, Debug, Default)]
pub struct Vfs {
    mounts: Vec<Arc<Mount>>,
    /// How a GOS request's name is matched, for the sources of the
    /// [`GOS_NAMESPACE`] key space.
    ///
    /// A property of the VFS rather than of one mount, because it is a
    /// property of `MetaOpenFile` — the one lookup that walks all registered
    /// sources — and the original registers all of them together. It is
    /// `AsciiInsensitive` until a chain states otherwise, because that is
    /// the rule every other legacy lookup here applies.
    ///
    /// It applies to the [`GOS_NAMESPACE`] key space **only**; see
    /// [`Vfs::matching_for`]. A session that also holds `install`, `reader`
    /// or `world` mounts keeps folding case in those key spaces whatever a
    /// GOS chain states.
    gos_name_match: GosNameMatch,
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
        self.mounts.push(Arc::new(mount));
        Ok(())
    }

    /// The registered mounts, in registration order.
    pub fn mounts(&self) -> impl Iterator<Item = &Mount> {
        self.mounts.iter().map(Arc::as_ref)
    }

    /// How many mounts are registered.
    pub fn len(&self) -> usize {
        self.mounts.len()
    }

    /// Whether no mount is registered.
    pub fn is_empty(&self) -> bool {
        self.mounts.is_empty()
    }

    /// How a GOS request's name is matched against the sources' names.
    ///
    /// [`GosNameMatch::AsciiInsensitive`] until a
    /// [`crate::vfs::gos::SessionBuilder::mount_gos_chain`] states the rule
    /// for its chain, which is what makes the matching rule (#693 settled
    /// what the original does with a request) an explicit input instead of
    /// an assumption.
    pub fn gos_name_match(&self) -> GosNameMatch {
        self.gos_name_match
    }

    /// Sets the GOS name-matching rule.
    ///
    /// Refuses a second, different rule: two chains in one VFS that answer
    /// the same key space under different rules would make the answer depend
    /// on which chain a caller happened to mount, so that is a mistake to
    /// report rather than a rule to keep. Stating the same rule twice is
    /// accepted.
    pub fn set_gos_name_match(&mut self, matching: GosNameMatch) -> Result<(), MountError> {
        if self.gos_name_match != matching && self.has_gos_mounts() {
            return Err(MountError::ConflictingGosNameMatch {
                installed: self.gos_name_match,
                requested: matching,
            });
        }
        self.gos_name_match = matching;
        Ok(())
    }

    /// Whether any mount of the [`GOS_NAMESPACE`] key space is registered.
    pub fn has_gos_mounts(&self) -> bool {
        self.mounts
            .iter()
            .any(|mount| mount.namespace().as_str() == GOS_NAMESPACE)
    }

    /// The name-matching rule that answers `key`.
    ///
    /// Only the [`GOS_NAMESPACE`] key space answers under the stated GOS
    /// rule. **Every other key space folds ASCII case and separators**
    /// ([`Mount::member`]), which is spec F04 non-negotiable behavior 1 and
    /// what `install`, `reader` and `world` mounts have always done.
    ///
    /// Scoping matters because [`Vfs::gos_name_match`] is a property of the
    /// VFS: a session that mounts an installation *and* a GOS chain holds
    /// both key spaces at once. Without this scope, a chain that states the
    /// [`GosNameMatch::ExactSpelling`] rule (#693 showed it is not the
    /// original's) would silently
    /// stop case folding in the installation's own key space too — a change
    /// to an unrelated key space that no caller asked for and no trace
    /// records.
    fn matching_for(&self, key: &AssetKey) -> GosNameMatch {
        if key.namespace().as_str() == GOS_NAMESPACE {
            self.gos_name_match
        } else {
            GosNameMatch::AsciiInsensitive
        }
    }

    /// The member of `mount` that serves `key` under `matching`.
    fn serving_member<'m>(
        &self,
        mount: &'m Mount,
        key: &AssetKey,
        matching: GosNameMatch,
    ) -> Option<&'m MemberRecord> {
        match matching {
            GosNameMatch::AsciiInsensitive => mount.member(key),
            GosNameMatch::ExactSpelling => mount.member_spelled_exactly(key),
        }
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
        let order = LookupOrder::for_namespace(key.namespace());
        let matching = self.matching_for(key);
        let mut considered: Vec<Considered<'_>> = Vec::new();
        for (index, mount) in self.mounts().enumerate() {
            if mount.namespace() != key.namespace() {
                // Other namespaces are other key spaces, not attempts of
                // this lookup.
                continue;
            }
            let outcome = match mount.scope().admit(context) {
                Err(reason) => AttemptOutcome::Skipped(reason),
                Ok(()) if self.serving_member(mount, key, matching).is_some() => {
                    AttemptOutcome::Candidate
                }
                Ok(()) => AttemptOutcome::Miss,
            };
            considered.push(Considered {
                index,
                mount,
                rank: effective_rank(mount, context),
                outcome,
            });
        }

        let selected = match order {
            LookupOrder::Precedence => {
                let best_rank = considered
                    .iter()
                    .filter(|entry| entry.outcome == AttemptOutcome::Candidate)
                    .map(|entry| entry.rank)
                    .max();
                let winners: Vec<usize> = match best_rank {
                    Some(rank) => considered
                        .iter()
                        .filter(|entry| {
                            entry.outcome == AttemptOutcome::Candidate && entry.rank == rank
                        })
                        .map(|entry| entry.index)
                        .collect(),
                    None => Vec::new(),
                };
                match winners.len() {
                    0 => None,
                    1 => Some(winners[0]),
                    _ => {
                        let candidates: Vec<ConflictOrigin> = winners
                            .iter()
                            .map(|index| origin_of(considered[*index].mount, key, matching))
                            .collect();
                        let trace = trace_of(considered, None, order);
                        return Err(ResolveError::Ambiguous {
                            key: Box::new(key.clone()),
                            candidates,
                            trace: Box::new(trace),
                        });
                    }
                }
            }
            LookupOrder::GosRegistration => {
                // `MetaOpenFile` walks the registered sources in order and
                // takes the first that has the name. The registration order
                // is the VFS mount order, so the earliest candidate wins and
                // the later ones stay in the trace as candidates that were
                // passed over — never as a tie, because the original has no
                // ambiguity to report.
                considered
                    .iter()
                    .filter(|entry| entry.outcome == AttemptOutcome::Candidate)
                    .map(|entry| entry.index)
                    .min()
            }
        };

        let trace = trace_of(considered, selected, order);
        let Some(index) = selected else {
            return Err(ResolveError::NotFound {
                key: Box::new(key.clone()),
                trace: Box::new(trace),
            });
        };

        let mount = &self.mounts[index];
        let member = self
            .serving_member(mount, key, matching)
            .expect("the selected mount holds the key under the matching rule");
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

    /// [`Vfs::resolve`], refusing an answer that only the unmeasured
    /// precedence order decided between retail sources.
    ///
    /// When a retail mount ([`Mount::is_retail`]) wins over other retail
    /// mounts that hold the key with different or unhashed bytes, and the
    /// only thing that decided between them is the **designed**
    /// [`PrecedenceClass`] order, the result depends on
    /// [`PRECEDENCE_ORDER_STATUS`]. While that is anything but
    /// `verified_original` the lookup fails with
    /// [`ResolveError::UnmeasuredOrder`] naming every origin (spec F04
    /// non-negotiable behavior 2). Non-retail sources and opted-in mods
    /// are exempt (their order is the caller's or user's choice, not an
    /// original-behavior claim), and shadowed copies with identical digests
    /// are no conflict (either order yields the same bytes). Content
    /// sessions resolve through this.
    ///
    /// A [`GOS_NAMESPACE`] key is **not** blocked. Its order is the
    /// original's own registration order, read out of `roffile.dll`
    /// (`MetaOpenFile`), not a design decision this workspace made — see
    /// [`LookupOrder::GosRegistration`]. Blocking it would refuse every
    /// GOS answer for the sake of a status the order does not depend on: the
    /// order is still reported as [`GOS_ORDER_STATUS`] (`inferred`) on the
    /// trace, so what the answer rests on stays visible, and raising that to
    /// `verified_original` needs an original run, not this task.
    pub fn resolve_blocking_unmeasured(
        &self,
        context: &ResolveContext,
        key: &AssetKey,
    ) -> Result<ResolvedAsset, ResolveError> {
        let resolved = self.resolve(context, key)?;
        if resolved.trace.order.order == LookupOrder::GosRegistration {
            return Ok(resolved);
        }
        let matching = self.matching_for(key);
        let mount = self
            .mounts()
            .find(|mount| *mount.id() == resolved.mount)
            .expect("the selected mount is registered");
        if PRECEDENCE_ORDER_STATUS == ClaimStatus::VerifiedOriginal
            || resolved.precedence == PrecedenceClass::Mod
            || !mount.is_retail()
        {
            return Ok(resolved);
        }
        let selected_sha256 = resolved.span.member_sha256();
        let shadowed: Vec<ConflictOrigin> = resolved
            .trace
            .attempts
            .iter()
            .filter(|attempt| attempt.outcome == AttemptOutcome::Candidate)
            .filter_map(|attempt| self.mounts().find(|other| *other.id() == attempt.mount))
            .filter(|other| other.is_retail() && other.precedence() != PrecedenceClass::Mod)
            .map(|other| origin_of(other, key, matching))
            .filter(|origin| selected_sha256.is_none() || origin.sha256 != selected_sha256)
            .collect();
        if shadowed.is_empty() {
            return Ok(resolved);
        }
        Err(ResolveError::UnmeasuredOrder {
            key: Box::new(key.clone()),
            selected: Box::new(origin_of(mount, key, matching)),
            shadowed,
            trace: Box::new(resolved.trace),
        })
    }

    /// Reads `length` bytes starting `start` bytes into the member that
    /// `resolved` names — the random read of spec F04 non-negotiable
    /// behavior 5. Nothing is written or extracted.
    ///
    /// The resolution must still describe a member of a mount this VFS
    /// holds, with the same container, spelling, range and digest;
    /// otherwise it is refused as stale instead of reading whatever now
    /// answers the key.
    pub fn read_range(
        &self,
        resolved: &ResolvedAsset,
        start: u64,
        length: u64,
    ) -> Result<Vec<u8>, ReadError> {
        let (mount, member) = self.current_member(resolved)?;
        source::read_member_range(mount, member, start, length)
    }

    /// Reads the whole member that `resolved` names and checks its digest
    /// against the one recorded when it was mounted.
    pub fn read_all(&self, resolved: &ResolvedAsset) -> Result<Vec<u8>, ReadError> {
        let (mount, member) = self.current_member(resolved)?;
        read_whole_member(mount, member)
    }

    /// The shared mount `resolved` names, if the resolution still
    /// describes one of its members exactly. A pending read keeps this
    /// handle so it can finish after the session that issued it closed.
    pub(crate) fn current_mount(&self, resolved: &ResolvedAsset) -> Result<Arc<Mount>, ReadError> {
        let mount = self
            .mounts
            .iter()
            .find(|mount| *mount.id() == resolved.mount)
            .ok_or_else(|| ReadError::UnknownMount {
                mount: resolved.mount.to_string(),
            })?;
        member_matching(mount, resolved)?;
        Ok(Arc::clone(mount))
    }

    /// The mount and member `resolved` names, if the resolution still
    /// describes them exactly.
    fn current_member(
        &self,
        resolved: &ResolvedAsset,
    ) -> Result<(&Mount, &MemberRecord), ReadError> {
        let mount = self
            .mounts()
            .find(|mount| *mount.id() == resolved.mount)
            .ok_or_else(|| ReadError::UnknownMount {
                mount: resolved.mount.to_string(),
            })?;
        Ok((mount, member_matching(mount, resolved)?))
    }
}

/// The member of `mount` that `resolved` names, if the resolution still
/// describes it exactly: same key, container, spelling, range and digest.
/// Anything else is a stale answer and is refused rather than read from
/// whatever now sits there.
pub(crate) fn member_matching<'m>(
    mount: &'m Mount,
    resolved: &ResolvedAsset,
) -> Result<&'m MemberRecord, ReadError> {
    let stale = || ReadError::StaleResolution {
        mount: resolved.mount.to_string(),
    };
    if *mount.id() != resolved.mount {
        return Err(stale());
    }
    let member = mount.member(&resolved.key).ok_or_else(stale)?;
    let span = &resolved.span;
    let matches = span.container_path() == mount.container()
        && span.member_key() == Some(member.spelling().as_str())
        && span.offset() == member.offset()
        && span.length() == member.size_bytes()
        && span.member_sha256() == member.sha256();
    if !matches {
        return Err(stale());
    }
    Ok(member)
}

/// Reads all of `member` and checks its digest against the one recorded
/// when it was mounted.
pub(crate) fn read_whole_member(
    mount: &Mount,
    member: &MemberRecord,
) -> Result<Vec<u8>, ReadError> {
    let bytes = source::read_member_range(mount, member, 0, member.size_bytes())?;
    check_member_digest(mount, member, &bytes)?;
    Ok(bytes)
}

/// Checks the bytes of a whole member against the digest recorded when it
/// was mounted.
pub(crate) fn check_member_digest(
    mount: &Mount,
    member: &MemberRecord,
    bytes: &[u8],
) -> Result<(), ReadError> {
    if let Some(mounted) = member.sha256() {
        let found = crate::install::sha256(bytes);
        if found != mounted {
            let path = mount
                .host_root()
                .zip(member.host_relative())
                .map(|(root, relative)| root.join(relative))
                .unwrap_or_default();
            return Err(ReadError::DigestMismatch {
                path,
                mounted,
                found,
            });
        }
    }
    Ok(())
}
