//! Observed collisions and how the VFS resolves each of them (F04-D).
//!
//! Spec F04 stage D: "Compare original lookup behavior for every observed
//! collision". A *collision* here is every set of two or more mounted
//! members that share a file name (compared by the legacy logical form:
//! separators normalized, ASCII case folded) — the retail installation has
//! `zrdr.zbd` at the root, world and mission level of `ZBD`, a
//! `texture.zbd` per world group, and so on. Grouping by file name, not by
//! full path, is deliberate: it is exactly the grouping a first-wins
//! basename map would flatten (non-negotiable behavior 3), so every group
//! is a place where a wrong lookup rule would serve the wrong bytes.
//!
//! [`observe_collisions`] lists the groups; [`compare_collisions`] looks
//! every member up by its own key under every given context, through
//! [`Vfs::resolve_blocking_unmeasured`] — the lookup content sessions use —
//! and classifies the group:
//!
//! * [`CollisionVerdict::DistinctByPath`]: every member that a context
//!   admits resolves to itself, so the collision is only a shared name and
//!   nothing depends on the precedence order;
//! * [`CollisionVerdict::ShadowedByIdenticalBytes`]: some lookup was served
//!   by another member with the same digest, so the order decided which
//!   origin is reported but not which bytes;
//! * [`CollisionVerdict::Conflicting`]: some lookup was ambiguous, blocked
//!   as decided only by the unmeasured order, or served different bytes.
//!
//! What the *original* engine does for the same lookups is not measured by
//! this module; the report carries [`PRECEDENCE_ORDER_STATUS`] so no
//! comparison built on it can be mistaken for original behavior.

use std::collections::BTreeMap;
use std::fmt;

use cs_types::asset_id::{
    AssetKey, MountId, MountNamespace, PRECEDENCE_ORDER_STATUS, PrecedenceClass, ResolveContext,
};
use cs_types::evidence::{ClaimStatus, ContentHash};

use crate::vfs::mount::MountScope;
use crate::vfs::resolve::{ConflictOrigin, LookupOrder, LookupOrderStatus, ResolveError, Vfs};

/// One mounted member that shares its file name with another.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollisionMember {
    /// The mount holding it.
    pub mount: MountId,
    /// That mount's key space.
    pub namespace: MountNamespace,
    /// That mount's container label.
    pub container: String,
    /// That mount's precedence class.
    pub precedence: PrecedenceClass,
    /// That mount's scope.
    pub scope: MountScope,
    /// The variant the member is indexed under.
    pub variant: String,
    /// The member's original spelling inside the container.
    pub spelling: String,
    /// The member's length in bytes.
    pub size_bytes: u64,
    /// The member's digest, when it was hashed.
    pub sha256: Option<ContentHash>,
}

impl CollisionMember {
    /// The key a consumer asks for to reach exactly this member.
    ///
    /// # Panics
    ///
    /// Never for a member of a built mount: its namespace, spelling and
    /// variant were validated when it was mounted.
    pub fn key(&self) -> AssetKey {
        AssetKey::from_spelling(self.namespace.as_str(), &self.spelling, &self.variant)
            .expect("a mounted member's namespace, spelling and variant are valid")
    }
}

/// Two or more members sharing one logical file name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Collision {
    /// The shared file name, separator-normalized and ASCII-lowercased.
    pub file_name: String,
    /// Every member with that name, in mount registration order, then
    /// member key order.
    pub members: Vec<CollisionMember>,
}

impl Collision {
    /// How many different digests the members hold (unhashed members
    /// count as one "unknown" digest each).
    pub fn distinct_digests(&self) -> usize {
        let mut known: Vec<ContentHash> = self.members.iter().filter_map(|m| m.sha256).collect();
        known.sort_by_key(|hash| hash.to_hex());
        known.dedup();
        known.len() + self.members.iter().filter(|m| m.sha256.is_none()).count()
    }
}

/// Lists every file-name collision among the mounts of `vfs`.
pub fn observe_collisions(vfs: &Vfs) -> Vec<Collision> {
    let mut groups: BTreeMap<String, Vec<CollisionMember>> = BTreeMap::new();
    for mount in vfs.mounts() {
        for (variant, member) in mount.members() {
            let logical = member.spelling().logical_key();
            let file_name = logical
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(logical.as_str())
                .to_owned();
            groups.entry(file_name).or_default().push(CollisionMember {
                mount: mount.id().clone(),
                namespace: mount.namespace().clone(),
                container: mount.container().to_owned(),
                precedence: mount.precedence(),
                scope: mount.scope().clone(),
                variant: variant.to_owned(),
                spelling: member.spelling().as_str().to_owned(),
                size_bytes: member.size_bytes(),
                sha256: member.sha256(),
            });
        }
    }
    groups
        .into_iter()
        .filter(|(_, members)| members.len() > 1)
        .map(|(file_name, members)| Collision { file_name, members })
        .collect()
}

/// What looking one colliding member up by its own key produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LookupOutcome {
    /// The context does not admit the member's mount; this is not a
    /// lookup of that member at all.
    NotEligible,
    /// The member itself served its key.
    Own,
    /// Another origin served the member's key.
    Other {
        /// The origin that served it.
        mount: MountId,
        /// Its spelling.
        spelling: String,
        /// Whether its digest equals the member's (both hashed and equal).
        same_bytes: bool,
    },
    /// Equal-precedence origins tied.
    Ambiguous(Vec<ConflictOrigin>),
    /// Refused: only the unmeasured precedence order decided it.
    Blocked {
        /// The origin the designed order would have served.
        selected: ConflictOrigin,
        /// The origins it would have shadowed.
        shadowed: Vec<ConflictOrigin>,
    },
    /// Nothing served the key although the member's mount is eligible —
    /// a VFS defect, reported rather than hidden.
    NotFound,
}

impl LookupOutcome {
    /// The stable label used in reports.
    pub const fn label(&self) -> &'static str {
        match self {
            Self::NotEligible => "not_eligible",
            Self::Own => "own",
            Self::Other {
                same_bytes: true, ..
            } => "other_same_bytes",
            Self::Other { .. } => "other_different_bytes",
            Self::Ambiguous(_) => "ambiguous",
            Self::Blocked { .. } => "blocked_unmeasured_order",
            Self::NotFound => "not_found",
        }
    }
}

impl fmt::Display for LookupOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// One member looked up under one context.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberLookup {
    /// Index into [`Collision::members`].
    pub member: usize,
    /// Index into [`CollisionReport::contexts`].
    pub context: usize,
    /// What the lookup produced.
    pub outcome: LookupOutcome,
    /// The order that decides this lookup, and how well it is known.
    ///
    /// A collision is grouped by file name across *every* mounted key space,
    /// so one comparison can hold members the designed precedence order
    /// competes and members the GOS registration order serves in turn (task
    /// #686). [`CollisionReport::precedence_status`] cannot describe both, so
    /// each lookup carries its own: `precedence`/`designed` for an
    /// installation, world or reader member, `gos_registration`/`inferred` for
    /// a GOS one. Without it a reader would take a GOS verdict for a product
    /// of the *designed* order, which is the opposite of what decided it.
    pub order: LookupOrderStatus,
}

/// How one collision is resolved across all compared contexts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CollisionVerdict {
    /// Every eligible lookup served the member itself.
    DistinctByPath,
    /// Some lookup was served by another member with identical bytes.
    ShadowedByIdenticalBytes,
    /// Some lookup was ambiguous, blocked, not found, or served other
    /// bytes.
    Conflicting,
}

impl CollisionVerdict {
    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::DistinctByPath => "distinct_by_path",
            Self::ShadowedByIdenticalBytes => "shadowed_by_identical_bytes",
            Self::Conflicting => "conflicting",
        }
    }
}

impl fmt::Display for CollisionVerdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// One collision with every lookup made for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollisionComparison {
    /// The collision.
    pub collision: Collision,
    /// One entry per (member, context), members outer.
    pub lookups: Vec<MemberLookup>,
    /// The worst outcome across the lookups.
    pub verdict: CollisionVerdict,
}

/// Every observed collision, compared under every given context.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollisionReport {
    /// The contexts every member was looked up under.
    pub contexts: Vec<ResolveContext>,
    /// One comparison per collision, by file name.
    pub comparisons: Vec<CollisionComparison>,
    /// How well the **precedence** order behind these lookups is known. It is
    /// [`PRECEDENCE_ORDER_STATUS`]: this report compares the VFS with itself
    /// across contexts, never with measured original behavior.
    ///
    /// It describes the precedence order only. A comparison in the GOS key
    /// space is decided by the GOS registration order instead (task #686), and
    /// each [`MemberLookup`] carries its own deciding order and status; do not
    /// read a GOS lookup's verdict as a product of this field.
    pub precedence_status: ClaimStatus,
}

impl CollisionReport {
    /// The comparisons whose verdict is [`CollisionVerdict::Conflicting`].
    pub fn conflicting(&self) -> impl Iterator<Item = &CollisionComparison> {
        self.comparisons
            .iter()
            .filter(|comparison| comparison.verdict == CollisionVerdict::Conflicting)
    }
}

/// Looks every colliding member of `vfs` up by its own key under each of
/// `contexts`, the way a content session would, and classifies each
/// collision.
pub fn compare_collisions(vfs: &Vfs, contexts: &[ResolveContext]) -> CollisionReport {
    let comparisons = observe_collisions(vfs)
        .into_iter()
        .map(|collision| compare_one(vfs, contexts, collision))
        .collect();
    CollisionReport {
        contexts: contexts.to_vec(),
        comparisons,
        precedence_status: PRECEDENCE_ORDER_STATUS,
    }
}

fn compare_one(
    vfs: &Vfs,
    contexts: &[ResolveContext],
    collision: Collision,
) -> CollisionComparison {
    let mut lookups = Vec::new();
    let mut verdict = CollisionVerdict::DistinctByPath;
    for (member_index, member) in collision.members.iter().enumerate() {
        let key = member.key();
        for (context_index, context) in contexts.iter().enumerate() {
            let outcome = lookup(vfs, context, &key, member);
            let severity = match &outcome {
                LookupOutcome::NotEligible | LookupOutcome::Own => CollisionVerdict::DistinctByPath,
                LookupOutcome::Other {
                    same_bytes: true, ..
                } => CollisionVerdict::ShadowedByIdenticalBytes,
                _ => CollisionVerdict::Conflicting,
            };
            verdict = verdict.max(severity);
            lookups.push(MemberLookup {
                member: member_index,
                context: context_index,
                outcome,
                order: LookupOrderStatus::of(LookupOrder::for_namespace(&member.namespace)),
            });
        }
    }
    CollisionComparison {
        collision,
        lookups,
        verdict,
    }
}

fn lookup(
    vfs: &Vfs,
    context: &ResolveContext,
    key: &AssetKey,
    member: &CollisionMember,
) -> LookupOutcome {
    if !member.scope.matches(context) {
        return LookupOutcome::NotEligible;
    }
    match vfs.resolve_blocking_unmeasured(context, key) {
        Ok(resolved) => {
            let spelling = resolved.span.member_key().unwrap_or_default();
            if resolved.mount == member.mount && spelling == member.spelling {
                LookupOutcome::Own
            } else {
                let served = resolved.span.member_sha256();
                LookupOutcome::Other {
                    mount: resolved.mount,
                    spelling: spelling.to_owned(),
                    same_bytes: served.is_some() && served == member.sha256,
                }
            }
        }
        Err(ResolveError::Ambiguous { candidates, .. }) => LookupOutcome::Ambiguous(candidates),
        Err(ResolveError::UnmeasuredOrder {
            selected, shadowed, ..
        }) => LookupOutcome::Blocked {
            selected: *selected,
            shadowed,
        },
        Err(ResolveError::NotFound { .. }) => LookupOutcome::NotFound,
    }
}
