//! Safe mounts and the compatibility signature (F53-B).
//!
//! Spec: `specs/F53-mod-mounts-custom-content-and-compatibility-signatures.md`,
//! stage `### F53-B`; shared contract `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! [`super::plan_mods`] answers *what a set of mods means* without touching
//! a byte. This module is the half that touches bytes: [`mount_mods`] opens
//! each planned mod's root through [`cs_assets::mods::ModRoot`], resolves
//! every declared source against the root that ships it, measures what it
//! actually found, refuses anything unsafe or unvalidated, and returns a
//! [`MountedMods`] carrying the F53-B **compatibility signature** — a hash
//! over the *resolved content bytes*, which is the thing
//! [`super::ModPlan::hash`] explicitly was not.
//!
//! # What a mount refuses
//!
//! * **Every plan problem, before any IO.** The plan is computed first, so a
//!   cyclic dependency, a missing required dependency, a native payload or
//!   an out-of-range engine version stops the mount with the plan's own
//!   report and no root is ever opened (F53 non-negotiable 1 and 2, F53
//!   AC02's cycle half).
//! * **A malicious relative path.** Every declared source is re-validated
//!   and looked up by [`cs_assets::mods::ModRoot::resolve`], which refuses
//!   `..`, absolute and drive-prefixed spellings, `.`/empty components and
//!   NUL bytes at the join, and never follows a symbolic link: a source
//!   only a link would satisfy is reported as not shipped (F53 AC02's path
//!   half).
//! * **Measured bytes over budget.** F53-A's budgets were over *declared*
//!   sizes because that stage reads nothing; this stage re-checks the same
//!   limits against the sizes the mount indexed, so a manifest that
//!   under-declares its payload still cannot ask for unbounded work.
//! * **Mission and script content with no validator.** An override whose
//!   target [`super::classify_validation`] calls
//!   [`OverrideValidation::SandboxedProgram`] mounts only if the host
//!   supplied a [`ProgramValidator`] and it accepted the bytes read from
//!   disk (F53 non-negotiable 2). With no validator the mount **fails**
//!   rather than enabling the payload: missing capability blocks, it never
//!   passes.
//!
//! Ordinary content is deliberately *not* decoded here. Its bounded reader
//! runs when the content is resolved and consumed — that is the original
//! adapter path, and duplicating it would be a second implementation of
//! every format. Only executable content needs a gate *before* enabling,
//! because it is the content that can act without a decoder.
//!
//! # What the signature covers
//!
//! [`MountedMods::signature`] is SHA-256 over a domain separator, the base
//! installation fingerprint the mount was asked under, the plan hash (load
//! order, ids, versions, dependencies, declared overrides and precedence)
//! and, for every content id that actually mounts, the winning mod, its
//! position, the payload's measured length and the payload's own SHA-256.
//! Two hosts agree on it exactly when they run the same base and the same
//! mod bytes in the same order — which is what a lobby or a save has to
//! agree on (F53 AC03's input; the lobby check itself is F53-C).
//!
//! # Designed, not original
//!
//! The original game's mod support is unmeasured (F53 "Research
//! boundary"; F53-D). The signature's domain separator, the refusal
//! vocabulary and the fixtures here are newly authored project design
//! carrying designed provenance; nothing in this module is evidence about
//! the original game.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use cs_assets::install::Sha256;
use cs_assets::mods::{ModMountError, ModRoot};
use cs_assets::vfs::{Mount, ReadError, RejectedEntry};
use cs_types::asset_id::ModId;
use cs_types::content::ContentId;
use cs_types::evidence::ContentHash;
use cs_types::install::{RelativePath, RelativePathError};

use super::manifest::ModManifest;
use super::overrides::{ContentOverride, OverrideEffect, OverrideValidation};
use super::{ModPlan, ModPlanError, ModSet, MountRequest, plan_mods};

/// The domain separator for [`MountedMods::signature`]. Changing it changes
/// every signature, so a signature can never be compared across
/// incompatible versions of this scheme.
const COMPAT_SIGNATURE_DOMAIN: &[u8] = b"cs-content-compat-signature-v1\n";

/// The host's bounded validator for mission and script payloads.
///
/// F53 non-negotiable 2 says sandboxed mission IR "uses the same bounded
/// validator as original adapters" — in this workspace that validator is
/// `cs_script::ir::MissionProgram::validate` behind the adapter chain that
/// decodes original bytes into the IR. `cs_content` may not depend on
/// `cs_script` (`docs/01-ARCHITECTURE.md`: `cs_content`'s allowed
/// dependencies are `cs_types`, `cs_formats` and the `cs_assets` VFS
/// interfaces), so the mount states the rule and the host supplies the
/// validator that implements it:
///
/// * a [`ProgramValidator`] must accept a payload only after running that
///   bounded validation, and must reject rather than defer;
/// * with **no** validator the mount refuses the override outright
///   ([`MountError::UnvalidatedProgram`]), because no measured decoder from
///   mod-authored bytes into a mission program exists in this workspace yet
///   (`cs_content::mission_control` still reports why it cannot lower one)
///   — a missing capability blocks, it never passes.
///
/// This is a *capability*, not a policy knob: it cannot make a payload
/// mountable that the validator refused, and it is never consulted for a
/// non-sandboxed target.
pub trait ProgramValidator: fmt::Debug {
    /// Runs the bounded validation over `bytes`, which were read from the
    /// mod root for the content id `target`.
    ///
    /// # Errors
    ///
    /// A reason the payload must not be enabled; the mount quotes it in
    /// [`MountError::ProgramRejected`].
    fn validate(&self, target: &ContentId, bytes: &[u8]) -> Result<(), String>;
}

/// Everything the host supplies around one mount: where each mod's root is,
/// which base installation the mount is made against, and (for executable
/// content) the bounded validator.
pub struct MountEnvironment<'a> {
    base_fingerprint: ContentHash,
    roots: BTreeMap<ModId, PathBuf>,
    program_validator: Option<&'a dyn ProgramValidator>,
}

impl<'a> MountEnvironment<'a> {
    /// An environment with no roots and no validator, made against the base
    /// installation fingerprint `base_fingerprint` (the F02
    /// `cs_assets::install::fingerprint` of the installation the session
    /// will run against).
    #[must_use]
    pub fn new(base_fingerprint: ContentHash) -> Self {
        Self {
            base_fingerprint,
            roots: BTreeMap::new(),
            program_validator: None,
        }
    }

    /// Declares where one planned mod's root directory is.
    ///
    /// A planned mod with no root here is refused with
    /// [`MountError::RootNotSupplied`] rather than mounted empty: a mod
    /// that is enabled but has no bytes on disk would otherwise look like a
    /// mod whose overrides silently vanished.
    #[must_use]
    pub fn with_root(mut self, mod_id: ModId, root: impl Into<PathBuf>) -> Self {
        self.roots.insert(mod_id, root.into());
        self
    }

    /// Supplies the host's bounded validator for sandboxed program content.
    ///
    /// Without it, a mount whose set claims mission or script content is
    /// refused ([`MountError::UnvalidatedProgram`]).
    #[must_use]
    pub fn with_program_validator(mut self, validator: &'a dyn ProgramValidator) -> Self {
        self.program_validator = Some(validator);
        self
    }

    /// The base installation fingerprint this mount is made against.
    #[must_use]
    pub fn base_fingerprint(&self) -> ContentHash {
        self.base_fingerprint
    }

    /// Where `mod_id`'s root is, if the host declared one.
    #[must_use]
    pub fn root(&self, mod_id: &ModId) -> Option<&Path> {
        self.roots.get(mod_id).map(PathBuf::as_path)
    }

    /// The validator the host supplied, if any.
    #[must_use]
    pub fn program_validator(&self) -> Option<&'a dyn ProgramValidator> {
        self.program_validator
    }
}

/// One content id that actually mounts: the winning claim, measured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountedPayload {
    target: ContentId,
    mod_id: ModId,
    position: usize,
    source: RelativePath,
    size_bytes: u64,
    sha256: ContentHash,
    effect: OverrideEffect,
    validation: OverrideValidation,
}

impl MountedPayload {
    /// The catalog id this payload serves.
    pub fn target(&self) -> &ContentId {
        &self.target
    }

    /// The mod whose payload wins for that id.
    pub fn mod_id(&self) -> &ModId {
        &self.mod_id
    }

    /// The winning mod's position in the load order.
    pub fn position(&self) -> usize {
        self.position
    }

    /// The declared source the bytes were read from, mod-root-relative.
    pub fn source(&self) -> &RelativePath {
        &self.source
    }

    /// The payload's **measured** length, not its declared one.
    pub fn size_bytes(&self) -> u64 {
        self.size_bytes
    }

    /// SHA-256 of the payload's bytes as the mount indexed them.
    pub fn sha256(&self) -> ContentHash {
        self.sha256
    }

    /// What this payload does to the simulation (F53 non-negotiable 3).
    pub fn effect(&self) -> OverrideEffect {
        self.effect
    }

    /// Which validator this payload had to pass (F53 non-negotiable 2).
    pub fn validation(&self) -> OverrideValidation {
        self.validation
    }
}

/// A validated, measured mount: the plan, the roots it mounted, the payloads
/// that actually serve content, and the compatibility signature over all of
/// it.
#[derive(Debug)]
pub struct MountedMods {
    plan: ModPlan,
    roots: BTreeMap<ModId, ModRoot>,
    payloads: Vec<MountedPayload>,
    measured_bytes: u64,
    signature: ContentHash,
}

impl MountedMods {
    /// The plan this mount was made from.
    pub fn plan(&self) -> &ModPlan {
        &self.plan
    }

    /// Every content id that mounts, sorted by content id — the winners
    /// only; a shadowed claim contributes nothing to the running content
    /// and nothing to the signature.
    pub fn payloads(&self) -> &[MountedPayload] {
        &self.payloads
    }

    /// The payload serving `target`, if any mod in this set wins it.
    pub fn payload(&self, target: &ContentId) -> Option<&MountedPayload> {
        self.payloads
            .iter()
            .find(|payload| &payload.target == target)
    }

    /// The compatibility signature: SHA-256 over the base fingerprint, the
    /// plan hash and every mounted payload's measured digest.
    ///
    /// This is the F53 signature [`super::ModPlan::hash`] says it is not:
    /// it covers the resolved content bytes, so changing a mod's bytes
    /// changes it while changing nothing about the plan does not.
    pub fn signature(&self) -> ContentHash {
        self.signature
    }

    /// The total measured payload bytes this mount enabled.
    #[must_use]
    pub fn measured_bytes(&self) -> u64 {
        self.measured_bytes
    }

    /// Whether the mounted set changes the simulation, so its sessions,
    /// saves, replays and network handshakes are modified (F53
    /// non-negotiable 3).
    pub fn modification(&self) -> super::ModModification {
        self.plan.modification()
    }

    /// Whether sessions built on this mount must be marked.
    pub fn marks_sessions(&self) -> bool {
        self.plan.marks_sessions()
    }

    /// The mounted roots, in load order.
    pub fn roots(&self) -> impl Iterator<Item = (&ModId, &ModRoot)> {
        self.plan
            .order()
            .iter()
            .filter_map(|id| self.roots.get(id).map(|root| (id, root)))
    }

    /// One planned mod's mounted root. The private export reads winners
    /// straight from it (`ModRoot::read` re-checks each member's digest), so
    /// an export can never be served bytes the walk did not index.
    pub fn root(&self, mod_id: &ModId) -> Option<&ModRoot> {
        self.roots.get(mod_id)
    }

    /// The payload mounts in load order, ready to be handed to a content
    /// session. Each is mod-precedence and scoped to its own mod, so a
    /// context that has not opted into it never sees it.
    pub fn mounts(&self) -> impl Iterator<Item = &Mount> {
        self.roots().map(|(_, root)| root.mount_record())
    }

    /// Every entry the directory walks refused to mount — symbolic links and
    /// non-regular files — with the mod that observed it. Refusal stays
    /// visible instead of becoming a silently missing file.
    pub fn rejections(&self) -> Vec<(&ModId, &RejectedEntry)> {
        self.roots()
            .flat_map(|(id, root)| root.rejected().iter().map(move |entry| (id, entry)))
            .collect()
    }
}

/// Why a mount could not be made.
#[derive(Debug)]
pub enum MountError {
    /// The set cannot be planned at all: a cycle, a missing or
    /// wrong-version dependency, a native payload, an out-of-range engine
    /// version, a budget or an action collision. No root was opened.
    Plan(ModPlanError),
    /// A planned mod has no root declared in the environment. Enabling a
    /// mod with no bytes on disk would look like its overrides had
    /// vanished, so it is refused.
    RootNotSupplied {
        /// The mod that has no root.
        mod_id: ModId,
    },
    /// A mod's root could not be walked or indexed.
    Root {
        /// The mod whose root failed.
        mod_id: ModId,
        /// Why it failed.
        source: Box<ModMountError>,
    },
    /// A declared source is not a safe relative spelling (F53 AC02).
    UnsafeSource {
        /// The mod that declared it.
        mod_id: ModId,
        /// The content id it claims.
        target: ContentId,
        /// The spelling exactly as it was given.
        spelling: String,
        /// Which rule refused it.
        reason: RelativePathError,
    },
    /// A declared source is safe but the mod does not ship such a file
    /// below its root — a symbolic link is not a member, so a source only a
    /// link would satisfy lands here.
    SourceMissing {
        /// The mod that does not ship it.
        mod_id: ModId,
        /// The content id it claims.
        target: ContentId,
        /// The spelling that was asked for.
        spelling: String,
    },
    /// One mod's measured payload bytes exceed the per-mod budget.
    MeasuredByteBudgetExceeded {
        /// The mod.
        mod_id: ModId,
        /// The bytes the mount measured for it.
        measured: u64,
        /// The budget.
        limit: u64,
    },
    /// The set's measured payload bytes exceed the total budget.
    TotalMeasuredByteBudgetExceeded {
        /// The bytes the mount measured for the whole set.
        measured: u64,
        /// The budget.
        limit: u64,
    },
    /// A mounted member has no recorded digest, so it cannot enter the
    /// signature. A directory mount hashes every file it indexes, so this
    /// means the member did not come from a safe walk.
    MissingDigest {
        /// The mod that ships it.
        mod_id: ModId,
        /// The content id it serves.
        target: ContentId,
    },
    /// A payload that carries mission or script content was about to be
    /// enabled with no bounded validator supplied (F53 non-negotiable 2).
    /// The mount fails: missing capability blocks, it never passes.
    UnvalidatedProgram {
        /// The mod that declared it.
        mod_id: ModId,
        /// The content id it claims.
        target: ContentId,
    },
    /// The host's bounded validator refused a payload, so it does not
    /// mount.
    ProgramRejected {
        /// The mod that declared it.
        mod_id: ModId,
        /// The content id it claims.
        target: ContentId,
        /// The validator's reason.
        reason: String,
    },
    /// A payload's bytes could not be read coherently from the root.
    Read {
        /// The mod that ships it.
        mod_id: ModId,
        /// The content id it serves.
        target: ContentId,
        /// Why the read failed.
        source: Box<ReadError>,
    },
}

impl MountError {
    /// The plan's report, when the mount was refused before any IO.
    #[must_use]
    pub fn plan_error(&self) -> Option<&ModPlanError> {
        match self {
            Self::Plan(error) => Some(error),
            _ => None,
        }
    }

    /// A short, stable code for machine-readable reports, mirroring
    /// [`super::PlanProblem::code`].
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Plan(_) => "plan_refused",
            Self::RootNotSupplied { .. } => "root_not_supplied",
            Self::Root { .. } => "root_unavailable",
            Self::UnsafeSource { .. } => "unsafe_source",
            Self::SourceMissing { .. } => "source_missing",
            Self::MeasuredByteBudgetExceeded { .. } => "measured_mod_budget_exceeded",
            Self::TotalMeasuredByteBudgetExceeded { .. } => "measured_total_budget_exceeded",
            Self::MissingDigest { .. } => "missing_digest",
            Self::UnvalidatedProgram { .. } => "unvalidated_program",
            Self::ProgramRejected { .. } => "program_rejected",
            Self::Read { .. } => "payload_read_failed",
        }
    }
}

impl fmt::Display for MountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plan(error) => write!(f, "the mod set cannot be enabled: {error}"),
            Self::RootNotSupplied { mod_id } => {
                write!(f, "mod {mod_id} is planned but has no root directory")
            }
            Self::Root { mod_id, source } => {
                write!(f, "cannot mount the root of mod {mod_id}: {source}")
            }
            Self::UnsafeSource {
                mod_id,
                target,
                spelling,
                reason,
            } => write!(
                f,
                "mod {mod_id} claims {target} through the unsafe source {spelling:?}: {reason}"
            ),
            Self::SourceMissing {
                mod_id,
                target,
                spelling,
            } => write!(
                f,
                "mod {mod_id} claims {target} through {spelling:?}, which it does not ship"
            ),
            Self::MeasuredByteBudgetExceeded {
                mod_id,
                measured,
                limit,
            } => write!(
                f,
                "mod {mod_id} measures {measured} payload bytes, the budget is {limit}"
            ),
            Self::TotalMeasuredByteBudgetExceeded { measured, limit } => write!(
                f,
                "the mod set measures {measured} payload bytes, the budget is {limit}"
            ),
            Self::MissingDigest { mod_id, target } => write!(
                f,
                "the payload mod {mod_id} ships for {target} has no recorded digest"
            ),
            Self::UnvalidatedProgram { mod_id, target } => write!(
                f,
                "mod {mod_id} claims {target}, which is mission or script content, and no \
                 bounded validator is supplied to validate it"
            ),
            Self::ProgramRejected {
                mod_id,
                target,
                reason,
            } => write!(
                f,
                "the bounded validator refused the payload mod {mod_id} ships for {target}: \
                 {reason}"
            ),
            Self::Read {
                mod_id,
                target,
                source,
            } => write!(
                f,
                "cannot read the payload mod {mod_id} ships for {target}: {source}"
            ),
        }
    }
}

impl std::error::Error for MountError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Plan(error) => Some(error),
            Self::Root { source, .. } => Some(source.as_ref()),
            Self::UnsafeSource { reason, .. } => Some(reason),
            Self::Read { source, .. } => Some(source.as_ref()),
            Self::RootNotSupplied { .. }
            | Self::SourceMissing { .. }
            | Self::MeasuredByteBudgetExceeded { .. }
            | Self::TotalMeasuredByteBudgetExceeded { .. }
            | Self::MissingDigest { .. }
            | Self::UnvalidatedProgram { .. }
            | Self::ProgramRejected { .. } => None,
        }
    }
}

/// One declared override as the mount measured it, before the winner is
/// known.
#[derive(Clone, Debug)]
struct Claim {
    mod_id: ModId,
    position: usize,
    override_entry: ContentOverride,
    size_bytes: u64,
    sha256: ContentHash,
}

/// Mounts `set` under `request`, against the roots and validator the host
/// supplies.
///
/// The order of work is the safety argument, and each step refuses rather
/// than degrading:
///
/// 1. [`plan_mods`] computes the plan. Every plan problem — a dependency
///    cycle above all — stops the mount here, with no root opened.
/// 2. Each planned mod's root is walked, indexed and hashed by
///    [`cs_assets::mods::ModRoot::mount`]. A planned mod with no declared
///    root is refused.
/// 3. Every declared source is resolved against its own mod's root, with
///    the spelling re-validated at the join, and its **measured** size is
///    checked against the per-mod and total budgets.
/// 4. For every content id the plan says wins, an executable
///    ([`OverrideValidation::SandboxedProgram`]) payload is read and handed
///    to the host's [`ProgramValidator`]; with no validator the mount is
///    refused, and a rejection is reported as one.
/// 5. The compatibility signature is computed over the base fingerprint,
///    the plan hash and the measured digests of the payloads that mount.
///
/// # Errors
///
/// [`MountError`] naming exactly which of the steps above refused and why.
pub fn mount_mods(
    set: &ModSet,
    request: &MountRequest,
    environment: &MountEnvironment<'_>,
) -> Result<MountedMods, MountError> {
    let plan = plan_mods(set, request).map_err(MountError::Plan)?;
    let limits = request.limits();

    // 1 + 2: the plan first, then the roots.
    let mut roots: BTreeMap<ModId, ModRoot> = BTreeMap::new();
    for id in plan.order() {
        let path = environment
            .root(id)
            .ok_or_else(|| MountError::RootNotSupplied { mod_id: id.clone() })?;
        let root = ModRoot::mount(id.clone(), path).map_err(|source| MountError::Root {
            mod_id: id.clone(),
            source: Box::new(source),
        })?;
        roots.insert(id.clone(), root);
    }

    // 3: resolve every declared claim against its own mod's root and
    // measure it. A claim the mod does not ship is refused here, before
    // anything could serve it.
    let mut claims: Vec<Claim> = Vec::new();
    let mut measured_bytes: u64 = 0;
    for (position, id) in plan.order().iter().enumerate() {
        let manifest: &ModManifest = set
            .manifests()
            .iter()
            .find(|manifest| manifest.id() == id)
            .expect("a planned mod is in the set it was planned from");
        let root = roots
            .get(id)
            .expect("every planned mod's root was mounted above");
        let mut mod_measured: u64 = 0;
        for entry in manifest.overrides() {
            let member = resolve_claim(root, id, entry)?;
            let size_bytes = member.size_bytes();
            mod_measured = mod_measured.saturating_add(size_bytes);
            measured_bytes = measured_bytes.saturating_add(size_bytes);
            let sha256 = ModRoot::digest(member).ok_or_else(|| MountError::MissingDigest {
                mod_id: id.clone(),
                target: entry.target().clone(),
            })?;
            claims.push(Claim {
                mod_id: id.clone(),
                position,
                override_entry: entry.clone(),
                size_bytes,
                sha256,
            });
        }
        if mod_measured > limits.max_declared_bytes_per_mod {
            return Err(MountError::MeasuredByteBudgetExceeded {
                mod_id: id.clone(),
                measured: mod_measured,
                limit: limits.max_declared_bytes_per_mod,
            });
        }
    }
    if measured_bytes > limits.max_declared_bytes_total {
        return Err(MountError::TotalMeasuredByteBudgetExceeded {
            measured: measured_bytes,
            limit: limits.max_declared_bytes_total,
        });
    }

    // 4: the winners. Payloads are pushed in precedence order, which is
    // sorted by content id, so the signature is canonical.
    let mut payloads: Vec<MountedPayload> = Vec::with_capacity(plan.precedence().entries().len());
    for entry in plan.precedence().entries() {
        let claim = claims
            .iter()
            .find(|claim| {
                claim.mod_id == *entry.winner() && claim.override_entry.target() == entry.target()
            })
            .expect("a declared winner has a measured claim");
        let root = roots
            .get(&claim.mod_id)
            .expect("a declared winner's root was mounted");
        if claim.override_entry.validation() == OverrideValidation::SandboxedProgram {
            let member = resolve_claim(root, &claim.mod_id, &claim.override_entry)?;
            let bytes = root.read(member).map_err(|source| MountError::Read {
                mod_id: claim.mod_id.clone(),
                target: claim.override_entry.target().clone(),
                source: Box::new(source),
            })?;
            match environment.program_validator() {
                None => {
                    return Err(MountError::UnvalidatedProgram {
                        mod_id: claim.mod_id.clone(),
                        target: claim.override_entry.target().clone(),
                    });
                }
                Some(validator) => validator
                    .validate(claim.override_entry.target(), &bytes)
                    .map_err(|reason| MountError::ProgramRejected {
                        mod_id: claim.mod_id.clone(),
                        target: claim.override_entry.target().clone(),
                        reason,
                    })?,
            }
        }
        payloads.push(MountedPayload {
            target: claim.override_entry.target().clone(),
            mod_id: claim.mod_id.clone(),
            position: claim.position,
            source: claim.override_entry.source().clone(),
            size_bytes: claim.size_bytes,
            sha256: claim.sha256,
            effect: claim.override_entry.effect(),
            validation: claim.override_entry.validation(),
        });
    }

    let signature = compatibility_signature(environment.base_fingerprint(), &plan, &payloads);
    Ok(MountedMods {
        plan,
        roots,
        payloads,
        measured_bytes,
        signature,
    })
}

/// Resolves one declared claim against its own mod's root, mapping the
/// root-join refusals onto the mount's own vocabulary so a report can name
/// the content id that asked for the unsafe or missing source.
fn resolve_claim<'a>(
    root: &'a ModRoot,
    mod_id: &ModId,
    entry: &ContentOverride,
) -> Result<&'a cs_assets::vfs::MemberRecord, MountError> {
    root.resolve(entry.source().as_str())
        .map_err(|error| match error {
            ModMountError::UnsafeSource {
                spelling, reason, ..
            } => MountError::UnsafeSource {
                mod_id: mod_id.clone(),
                target: entry.target().clone(),
                spelling,
                reason,
            },
            ModMountError::SourceNotMounted { spelling, .. } => MountError::SourceMissing {
                mod_id: mod_id.clone(),
                target: entry.target().clone(),
                spelling,
            },
            other => MountError::Root {
                mod_id: mod_id.clone(),
                source: Box::new(other),
            },
        })
}

/// The compatibility signature: what two hosts must agree on before they
/// share a session, a save, a replay or a handshake.
///
/// Canonical by construction: the payloads are already ordered by content
/// id, and the inputs are the base fingerprint, the plan hash and each
/// payload's measured length and digest.
fn compatibility_signature(
    base_fingerprint: ContentHash,
    plan: &ModPlan,
    payloads: &[MountedPayload],
) -> ContentHash {
    let mut hasher = Sha256::new();
    hasher.update(COMPAT_SIGNATURE_DOMAIN);
    hasher.update(base_fingerprint.as_bytes());
    hasher.update(b"\nplan\n");
    hasher.update(plan.hash().as_bytes());
    hasher.update(b"\npayloads\n");
    for payload in payloads {
        hasher.update(payload.target.as_str().as_bytes());
        hasher.update(b"\t");
        hasher.update(payload.mod_id.as_str().as_bytes());
        hasher.update(b"\t");
        hasher.update(payload.position.to_string().as_bytes());
        hasher.update(b"\t");
        hasher.update(payload.size_bytes.to_string().as_bytes());
        hasher.update(b"\t");
        hasher.update(payload.sha256.as_bytes());
        hasher.update(b"\n");
    }
    hasher.finalize()
}

#[cfg(test)]
mod tests {
    //! F53-B acceptance tests for the mount. Every tree and every payload
    //! here is newly authored synthetic bytes below the system temporary
    //! directory; no original game data and no `CS_GAME_DIR` access.

    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use cs_types::content::ContentKind;
    use cs_types::evidence::ContentHash;

    use super::super::{
        ModSet, MountLimits, MountRequest, PlanProblem, synthetic_base_ids,
        synthetic_conflicting_mods, synthetic_cyclic_mods, synthetic_engine_version,
        synthetic_mission_mod, synthetic_mount_request, synthetic_tuning_mod,
    };
    use super::*;

    static NEXT: AtomicU64 = AtomicU64::new(0);

    /// A disposable directory, removed on drop.
    struct Temp(PathBuf);

    impl Temp {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "cs-f53-b-mount-{label}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).expect("temp dir is created");
            Self(root)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// The synthetic bytes one declared override ships: the content id it
    /// claims plus a fixed tail, so every payload is distinct, deterministic
    /// and traceable to its target.
    fn payload_bytes(entry: &ContentOverride) -> Vec<u8> {
        let mut bytes = entry.target().as_str().as_bytes().to_vec();
        bytes.extend_from_slice(b"::synthetic payload");
        bytes
    }

    /// Writes one mod's declared sources below `root`, as a shipped mod
    /// would have them on disk.
    fn write_mod(root: &Temp, manifest: &ModManifest) {
        for entry in manifest.overrides() {
            let path = root.path().join(entry.source().as_str());
            fs::create_dir_all(path.parent().expect("has a parent")).expect("dirs");
            fs::write(path, payload_bytes(entry)).expect("bytes are written");
        }
    }

    /// The base fingerprint every fixture here is mounted against: a
    /// designed constant standing in for the F02 installation fingerprint
    /// a real caller supplies.
    fn base_fingerprint() -> ContentHash {
        ContentHash::from_bytes([7u8; 32])
    }

    /// **F53 AC02, cycle half.** A cyclic dependency set is refused by the
    /// plan, and the mount refuses it *before opening anything*: the
    /// environment here declares no roots at all, so a mount that had
    /// reached the root stage would report a missing root instead. The
    /// report is the plan's own cycle, with the mod path.
    #[test]
    fn accept_f53_b_a_cyclic_dependency_is_rejected_before_any_root_is_opened() {
        let (first, second) = synthetic_cyclic_mods();
        let set = ModSet::new(vec![first, second]);
        let environment = MountEnvironment::new(base_fingerprint());

        let error = mount_mods(&set, &synthetic_mount_request(), &environment)
            .expect_err("a cyclic set cannot be mounted");

        let plan_error = error
            .plan_error()
            .expect("the refusal is the plan's, so no root was opened");
        assert!(
            plan_error
                .problems()
                .iter()
                .any(|problem| matches!(problem, PlanProblem::DependencyCycle { .. })),
            "the report names the cycle: {plan_error}"
        );
        assert_eq!(error.code(), "plan_refused");
        assert!(
            plan_error.to_string().contains("cycle"),
            "the refusal is readable: {plan_error}"
        );
    }

    /// **F53 AC02, path half.** A declared source that only a symbolic link
    /// would satisfy is refused: the link is never mounted, so the claim
    /// lands as "the mod does not ship it" instead of resolving out of the
    /// root.
    #[cfg(unix)]
    #[test]
    fn accept_f53_b_a_declared_source_behind_a_symbolic_link_is_refused() {
        let outside = Temp::new("link-outside");
        fs::write(outside.path().join("stolen.png"), b"outside the mod").expect("bytes");

        let (first, second) = synthetic_conflicting_mods();
        let root = Temp::new("link");
        write_mod(&root, &first);
        write_mod(&root, &second);
        fs::remove_file(root.path().join("art/panel.png")).expect("the regular file is removed");
        std::os::unix::fs::symlink(
            outside.path().join("stolen.png"),
            root.path().join("art/panel.png"),
        )
        .expect("the link is created");

        let environment = MountEnvironment::new(base_fingerprint())
            .with_root(first.id().clone(), root.path())
            .with_root(second.id().clone(), root.path());
        let error = mount_mods(
            &ModSet::new(vec![first.clone(), second.clone()]),
            &synthetic_mount_request(),
            &environment,
        )
        .expect_err("a payload only a link would satisfy cannot mount");

        assert_eq!(error.code(), "source_missing");
        match error {
            MountError::SourceMissing {
                mod_id,
                target,
                spelling,
            } => {
                assert_eq!(mod_id, *first.id());
                assert_eq!(spelling, "art/panel.png");
                assert!(
                    target.as_str().contains("synthetic.hull-panel"),
                    "the refusal names the content id: {target}"
                );
            }
            other => panic!("the link is refused as a missing source, got: {other}"),
        }
    }

    /// **F53 AC03's input.** The compatibility signature covers the
    /// resolved content bytes: it is stable across two mounts of the same
    /// files, it changes when a payload's byte changes while the plan does
    /// not, and it changes when the base installation the mount was made
    /// against changes.
    #[test]
    fn accept_f53_b_the_compatibility_signature_covers_the_resolved_payload_bytes() {
        let (first, second) = synthetic_conflicting_mods();
        let tuning = synthetic_tuning_mod();
        let set = ModSet::new(vec![first.clone(), second.clone(), tuning.clone()]);
        let root = Temp::new("signature");
        write_mod(&root, &first);
        write_mod(&root, &second);
        write_mod(&root, &tuning);

        let environment = || {
            MountEnvironment::new(base_fingerprint())
                .with_root(first.id().clone(), root.path())
                .with_root(second.id().clone(), root.path())
                .with_root(tuning.id().clone(), root.path())
        };

        let mounted = mount_mods(&set, &synthetic_mount_request(), &environment())
            .expect("the fixture set mounts");
        let again = mount_mods(&set, &synthetic_mount_request(), &environment())
            .expect("the fixture set mounts again");

        // Deterministic: the same files, the same plan, the same signature.
        assert_eq!(mounted.signature(), again.signature());
        assert_eq!(mounted.signature().to_hex().len(), 64);

        // Every payload's recorded digest is the SHA-256 of the bytes on
        // disk, and its size is the measured size rather than the declared
        // one.
        assert_eq!(
            mounted.payloads().len(),
            mounted.plan().precedence().entries().len()
        );
        for payload in mounted.payloads() {
            let bytes = fs::read(root.path().join(payload.source().as_str()))
                .expect("the payload is on disk");
            assert_eq!(payload.size_bytes(), bytes.len() as u64);
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            assert_eq!(
                payload.sha256(),
                hasher.finalize(),
                "{} is hashed from its own bytes",
                payload.target()
            );
        }

        // The mounts are the plan's load order, mod-precedence and scoped to
        // the mod they belong to.
        let mounts: Vec<&Mount> = mounted.mounts().collect();
        assert_eq!(mounts.len(), 3);
        assert_eq!(
            mounted
                .roots()
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>(),
            mounted.plan().order()
        );
        for mount in mounts {
            assert_eq!(mount.precedence(), cs_types::asset_id::PrecedenceClass::Mod);
            assert!(!mount.is_retail());
        }

        // One byte of one payload changes: the plan is unchanged and the
        // signature is not.
        let tuning_bytes = root.path().join("tuning/vulcan.toml");
        fs::write(&tuning_bytes, b"gun/synthetic.vulcan::synthetic payl0ad")
            .expect("the byte is rewritten");
        let tampered = mount_mods(&set, &synthetic_mount_request(), &environment())
            .expect("the fixture set still mounts");
        assert_eq!(
            tampered.plan().hash(),
            mounted.plan().hash(),
            "the plan is the same"
        );
        assert_ne!(
            tampered.signature(),
            mounted.signature(),
            "one changed payload byte must change the signature"
        );

        // A different base installation changes it too: two hosts that run
        // the same mods against different retail content do not agree.
        let other_base = MountEnvironment::new(ContentHash::from_bytes([8u8; 32]))
            .with_root(first.id().clone(), root.path())
            .with_root(second.id().clone(), root.path())
            .with_root(tuning.id().clone(), root.path());
        let relocated = mount_mods(&set, &synthetic_mount_request(), &other_base)
            .expect("the fixture set mounts");
        assert_ne!(
            relocated.signature(),
            tampered.signature(),
            "the base fingerprint is part of the signature"
        );

        // A gameplay mod still marks its sessions (non-negotiable 3), and
        // the mount reports it alongside the signature.
        assert!(mounted.marks_sessions());
        assert_eq!(
            mounted.modification(),
            super::super::ModModification::Gameplay
        );
    }

    /// **F53 non-negotiable 2.** Mission and script content mounts only
    /// through the host's bounded validator, and without one the mount is
    /// *refused* — a missing capability blocks, it never passes. When a
    /// validator is supplied, its verdict is final either way and it is
    /// handed exactly the bytes that came off disk.
    #[test]
    fn accept_f53_b_mission_content_mounts_only_through_a_bounded_validator() {
        let mission = synthetic_mission_mod();
        let root = Temp::new("mission");
        write_mod(&root, &mission);
        let set = ModSet::new(vec![mission.clone()]);
        let environment = || {
            MountEnvironment::new(base_fingerprint()).with_root(mission.id().clone(), root.path())
        };

        // No validator: refused, and nothing is enabled.
        let error = mount_mods(&set, &synthetic_mount_request(), &environment())
            .expect_err("mission content without a validator cannot mount");
        assert!(matches!(error, MountError::UnvalidatedProgram { .. }));
        assert_eq!(error.code(), "unvalidated_program");
        assert!(
            error.to_string().contains("synthetic.training-mission"),
            "the refusal names the mod: {error}"
        );

        // A validator that refuses: its verdict is the mount's verdict.
        let rejecting = RejectingValidator::default();
        let error = mount_mods(
            &set,
            &synthetic_mount_request(),
            &environment().with_program_validator(&rejecting),
        )
        .expect_err("a refused mission payload cannot mount");
        assert!(matches!(error, MountError::ProgramRejected { .. }));
        assert_eq!(error.code(), "program_rejected");
        assert_eq!(
            rejecting.seen.borrow().len(),
            1,
            "the validator was handed the payload once"
        );

        // A validator that accepts: the payload mounts, with the bytes it
        // validated recorded in the signature.
        let accepting = AcceptingValidator::default();
        let mounted = mount_mods(
            &set,
            &synthetic_mount_request(),
            &environment().with_program_validator(&accepting),
        )
        .expect("the validated mission payload mounts");
        let payload = mounted
            .payload(
                &ContentId::from_source(ContentKind::Mission, "synthetic.training")
                    .expect("the id is valid"),
            )
            .expect("the mission id mounts");
        assert_eq!(payload.validation(), OverrideValidation::SandboxedProgram);
        let expected = fs::read(root.path().join("missions/training.mis")).expect("bytes");
        assert_eq!(
            accepting.seen.borrow()[0],
            expected,
            "the validator saw exactly the bytes the mount read"
        );
        assert!(mounted.marks_sessions());
    }

    /// F53-A's budgets were over *declared* sizes because that stage reads
    /// nothing. This stage measures, and a manifest that under-declares its
    /// payload is refused by the same budget it tried to stay under.
    #[test]
    fn accept_f53_b_measured_payload_bytes_are_held_to_the_mount_budget() {
        let tuning = synthetic_tuning_mod();
        let root = Temp::new("budget");
        write_mod(&root, &tuning);
        // The manifest declares 512 bytes; the file the mod actually ships
        // is four times that, which is exactly the discrepancy F53-A could
        // not check because it reads nothing.
        fs::write(root.path().join("tuning/vulcan.toml"), vec![b'x'; 4_096])
            .expect("the payload is rewritten");
        let set = ModSet::new(vec![tuning.clone()]);
        let request = MountRequest::new(synthetic_engine_version(), synthetic_base_ids())
            .with_limits(MountLimits {
                max_declared_bytes_per_mod: 1_024,
                ..MountLimits::default()
            });
        let environment =
            MountEnvironment::new(base_fingerprint()).with_root(tuning.id().clone(), root.path());

        // The declaration (512 bytes) fits the budget, so the plan passes;
        // the file on disk does not, and the mount refuses it.
        let error = mount_mods(&set, &request, &environment)
            .expect_err("a payload larger than the budget cannot mount");
        assert!(matches!(
            error,
            MountError::MeasuredByteBudgetExceeded { measured, limit, .. }
                if limit == 1_024 && measured > 1_024
        ));
        assert_eq!(error.code(), "measured_mod_budget_exceeded");

        // With a budget the measured size fits, the same set mounts and the
        // measured total is the real one, not the declared 512.
        let relaxed = MountRequest::new(synthetic_engine_version(), synthetic_base_ids())
            .with_limits(MountLimits {
                max_declared_bytes_per_mod: 64 * 1_024 * 1_024,
                ..MountLimits::default()
            });
        let mounted = mount_mods(&set, &relaxed, &environment)
            .expect("the fixture payload is well inside the default budget");
        assert_eq!(mounted.measured_bytes(), 4_096);
    }

    /// A validator that refuses every payload, recording what it was asked.
    #[derive(Debug, Default)]
    struct RejectingValidator {
        seen: std::cell::RefCell<Vec<Vec<u8>>>,
    }

    impl ProgramValidator for RejectingValidator {
        fn validate(&self, target: &ContentId, bytes: &[u8]) -> Result<(), String> {
            self.seen.borrow_mut().push(bytes.to_vec());
            Err(format!("{target} is refused by the fixture validator"))
        }
    }

    /// A validator that accepts every payload, recording what it was asked.
    #[derive(Debug, Default)]
    struct AcceptingValidator {
        seen: std::cell::RefCell<Vec<Vec<u8>>>,
    }

    impl ProgramValidator for AcceptingValidator {
        fn validate(&self, _target: &ContentId, bytes: &[u8]) -> Result<(), String> {
            self.seen.borrow_mut().push(bytes.to_vec());
            Ok(())
        }
    }
}
