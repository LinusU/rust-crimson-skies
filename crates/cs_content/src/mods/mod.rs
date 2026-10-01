//! Mod manifests, overrides and the deterministic mount plan (F53-A).
//!
//! Spec: `specs/F53-mod-mounts-custom-content-and-compatibility-signatures.md`,
//! stage `### F53-A`. Shared contract:
//! `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! This module is the **typed, side-effect-free half** of the mod system. It
//! defines what a mod says about itself, what a mod claims about content, and
//! what a set of mods means when they are all switched on together — and it
//! refuses every set that is ambiguous, unsatisfiable, unsafe or over
//! budget. It opens no archive, reads no byte, hashes no content, mounts
//! nothing and draws nothing. That is deliberate: F53's stages below this one
//! are `F53-B` (safe mounts and compatibility signatures), `F53-C`
//! (selection, diagnostics and private export tooling) and `F53-D`
//! (source protection, mod isolation and reproducible load order), and each of
//! them consumes these records rather than redefining them.
//!
//! # Records
//!
//! * [`manifest::ModManifest`] — the manifest: a stable [`ModId`], a
//!   display name, a [`manifest::ModVersion`], a closed
//!   [`manifest::EngineRange`], declared [`manifest::ModDependency`] rows,
//!   shipped [`manifest::ModPayload`] files and content overrides. Every
//!   text input is validated by a constructor, so an untrusted manifest can
//!   never hold an id that escapes its namespace or a source path that
//!   escapes the mod root.
//! * [`overrides::ContentOverride`] — one mod's claim about one content id:
//!   [`overrides::Add`] or [`overrides::Replace`], a mod-root-relative source
//!   spelling and a declared byte count.
//! * [`ModSet`] — a set of manifests as supplied, with no order assumed.
//! * [`ModPlan`] — the answer: a deterministic load order, a
//!   [`PrecedenceReport`] that names the winner of every contested content id
//!   and every mod it shadows, a [`overrides::ModModification`] verdict and a
//!   [`ModPlan::hash`], or a [`ModPlanError`] listing **every** reason the set
//!   cannot be enabled.
//!
//! # The load order is the F04-A mod stack
//!
//! The order [`plan_mods`] computes is not a private convention: it is fed
//! straight into [`cs_types::asset_id::ModStack`], the same stack a
//! [`cs_types::asset_id::ResolveContext`] opts into, so "the later mod wins"
//! here is literally the rule `cs_assets::vfs` applies when it resolves a
//! key. Two mods claiming one id is therefore not an error — it is a
//! [`PrecedenceEntry`] with one winner and a named list of shadowed claims,
//! which is what F53 AC01 asks for.
//!
//! # Everything the plan cannot know stays unclaimed
//!
//! The plan is computed from manifests and from what the caller tells it
//! ([`MountRequest`]). It cannot see the bytes a mod ships, the installation
//! it will run against or the retail content it overrides, so:
//!
//! * a manifest's `declared_bytes` is a **declared** size, not a measured
//!   one, and the byte budgets are declared-size budgets;
//! * the set-level [`overrides::ModModification`] is derived from
//!   [`overrides::classify_effect`], an explicit **hash policy** over content
//!   kinds, and not from the author's `cosmetic_only` flag — a mod that
//!   claims cosmetic while claiming a gameplay id is refused;
//! * nothing here produces a *compatibility signature*. F53-B's signature is
//!   over resolved content bytes, so this stage exposes only the inputs it
//!   would hash.
//!
//! # Designed vocabulary, not original data
//!
//! The original game's mod support — whether it had any, what a mod manifest
//! looked like, which content could be overridden, how versions and
//! dependencies were expressed, and what a "cosmetic" change was — is
//! **unmeasured** (F53 "Research boundary"; F53-D's retail stage). Every
//! label, bound and fixture here is newly authored project design carrying
//! designed provenance, recorded in
//! `docs/findings/2026-10-01-f53-a-mod-manifest-and-override-validation.md`.
//! No original file, byte or behavior is reproduced, and nothing in this
//! module may be cited as evidence about the original game.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cs_assets::install::Sha256;
use cs_types::asset_id::{ModId, ModStack};
use cs_types::content::{ContentId, ContentKind, Provenance};
use cs_types::evidence::{ClaimId, ContentHash};

mod manifest;
mod overrides;

pub use manifest::{
    DependencyStrength, EngineRange, MAX_MOD_NAME_BYTES, ManifestError, ModDependency, ModHeader,
    ModManifest, ModPayload, ModVersion, PayloadKind, VersionRange,
};
pub use overrides::{
    COSMETIC_CONTENT_KINDS, ContentOverride, ModModification, OverrideAction, OverrideEffect,
    OverrideValidation, SANDBOXED_PROGRAM_CONTENT_KINDS, classify_effect, classify_validation,
};

/// The claim id every synthetic fixture in this module is designed under.
const SYNTHETIC_MOD_CLAIM: &str = "f53a.synthetic-mod-set";

/// The domain separator for [`ModPlan::hash`]. Changing it changes every
/// plan hash, so a hash can never be compared across incompatible versions
/// of this scheme.
const PLAN_HASH_DOMAIN: &[u8] = b"cs-content-mod-plan-v1\n";

/// The claim id the synthetic mod fixtures are designed under.
///
/// A designed claim, like every other fixture claim in the workspace: it
/// identifies *this* module's design decisions and is never a claim about
/// the original game.
#[must_use]
pub fn synthetic_mod_claim() -> ClaimId {
    ClaimId::new(SYNTHETIC_MOD_CLAIM).expect("the synthetic mod claim id is valid")
}

/// A set of mod manifests, in whatever order the caller found them.
///
/// [`plan_mods`] treats this as a set: the order manifests are supplied in
/// never reaches the plan, so a directory listing that happens to sort
/// differently produces the same load order and the same report.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModSet {
    manifests: Vec<ModManifest>,
}

impl ModSet {
    /// Builds a set from manifests supplied in any order.
    #[must_use]
    pub fn new(manifests: Vec<ModManifest>) -> Self {
        Self { manifests }
    }

    /// Adds one manifest.
    pub fn push(&mut self, manifest: ModManifest) {
        self.manifests.push(manifest);
    }

    /// The manifests as supplied, unsorted.
    pub fn manifests(&self) -> &[ModManifest] {
        &self.manifests
    }

    /// How many manifests the set holds, duplicates included: a duplicate id
    /// is a [`PlanProblem`], not a silently deduplicated entry.
    pub fn len(&self) -> usize {
        self.manifests.len()
    }

    /// Whether the set is empty.
    pub fn is_empty(&self) -> bool {
        self.manifests.is_empty()
    }
}

/// What the caller knows when it asks for a plan.
///
/// This is the boundary between "what a mod declared" and "what the host can
/// check", and it is a parameter rather than a constant so a second host, a
/// test or a future retail probe can answer with its own numbers instead of
/// inheriting a fixture's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountRequest {
    engine: ModVersion,
    base_ids: BTreeSet<ContentId>,
    limits: MountLimits,
}

impl MountRequest {
    /// A request that knows the engine version and which content ids the
    /// base game already provides, under the default limits.
    #[must_use]
    pub fn new(engine: ModVersion, base_ids: impl IntoIterator<Item = ContentId>) -> Self {
        Self {
            engine,
            base_ids: base_ids.into_iter().collect(),
            limits: MountLimits::default(),
        }
    }

    /// Replaces the limits this request enforces.
    #[must_use]
    pub fn with_limits(mut self, limits: MountLimits) -> Self {
        self.limits = limits;
        self
    }

    /// The engine version the plan is computed under.
    pub fn engine(&self) -> ModVersion {
        self.engine
    }

    /// The content ids the base game provides.
    pub fn base_ids(&self) -> &BTreeSet<ContentId> {
        &self.base_ids
    }

    /// The limits this request enforces.
    pub fn limits(&self) -> &MountLimits {
        &self.limits
    }
}

/// The declared budgets a mount plan is held to.
///
/// Every field is a plain integer so a total can never round
/// (`IDENTITY-CONTENT`: "Integers represent money, ticks, counts, ammo and
/// ids"). The defaults are **designed bounds**, not measured original ones:
/// they exist to keep a hostile or corrupt manifest from asking for an
/// unbounded amount of work, and a host is free to set its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MountLimits {
    /// Most mods in one set.
    pub max_mods: usize,
    /// Most dependencies one manifest may declare.
    pub max_dependencies: usize,
    /// Most overrides one manifest may declare.
    pub max_overrides_per_mod: usize,
    /// Most declared bytes one manifest may claim.
    pub max_declared_bytes_per_mod: u64,
    /// Most declared bytes the whole set may claim.
    pub max_declared_bytes_total: u64,
}

impl Default for MountLimits {
    /// The designed default bounds.
    fn default() -> Self {
        Self {
            max_mods: 64,
            max_dependencies: 32,
            max_overrides_per_mod: 4_096,
            max_declared_bytes_per_mod: 64 * 1_024 * 1_024,
            max_declared_bytes_total: 512 * 1_024 * 1_024,
        }
    }
}

impl MountLimits {
    /// The stable text encoding folded into [`ModPlan::hash`], so two plans
    /// computed under different budgets cannot collide.
    #[must_use]
    pub fn encode(&self) -> String {
        format!(
            "mods={};deps={};overrides={};bytes_per_mod={};bytes_total={}",
            self.max_mods,
            self.max_dependencies,
            self.max_overrides_per_mod,
            self.max_declared_bytes_per_mod,
            self.max_declared_bytes_total
        )
    }
}

/// One mod as it appears in the plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedMod {
    id: ModId,
    version: ModVersion,
    position: usize,
    modification: ModModification,
    override_count: usize,
    declared_bytes: u64,
    dependencies: Vec<PlannedDependency>,
}

impl PlannedMod {
    /// The mod's stable id.
    pub fn id(&self) -> &ModId {
        &self.id
    }

    /// The mod's declared version.
    pub fn version(&self) -> ModVersion {
        self.version
    }

    /// The mod's position in the load order; a higher position outranks a
    /// lower one, which is the F04-A [`ModStack`] rule.
    pub fn position(&self) -> usize {
        self.position
    }

    /// Whether this mod alone changes the simulation.
    pub fn modification(&self) -> ModModification {
        self.modification
    }

    /// How many content ids the mod claims.
    pub fn override_count(&self) -> usize {
        self.override_count
    }

    /// The mod's declared payload bytes.
    pub fn declared_bytes(&self) -> u64 {
        self.declared_bytes
    }

    /// The mod's declared dependencies, with how each was satisfied.
    pub fn dependencies(&self) -> &[PlannedDependency] {
        &self.dependencies
    }
}

/// How one declared dependency was satisfied by the set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedDependency {
    /// The mod the row names.
    pub mod_id: ModId,
    /// How strongly it is asserted.
    pub strength: DependencyStrength,
    /// The versions it accepts.
    pub range: VersionRange,
    /// Whether the set satisfies it.
    pub satisfied: bool,
}

/// One contested content id, with the order that decided it.
///
/// F53 AC01's unit of answer: the winner, its load position, and every mod it
/// shadows, in load order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrecedenceEntry {
    target: ContentId,
    action: OverrideAction,
    validation: OverrideValidation,
    effect: OverrideEffect,
    winner: ModId,
    winner_position: usize,
    shadowed: Vec<(ModId, usize)>,
}

impl PrecedenceEntry {
    /// The contested content id.
    pub fn target(&self) -> &ContentId {
        &self.target
    }

    /// Whether the winner adds or replaces this id.
    pub fn action(&self) -> OverrideAction {
        self.action
    }

    /// Which validator the winning payload has to pass.
    pub fn validation(&self) -> OverrideValidation {
        self.validation
    }

    /// What the winning override does to the simulation.
    pub fn effect(&self) -> OverrideEffect {
        self.effect
    }

    /// The mod whose claim wins, which is the one loaded last.
    pub fn winner(&self) -> &ModId {
        &self.winner
    }

    /// The winner's load-order position.
    pub fn winner_position(&self) -> usize {
        self.winner_position
    }

    /// The losing claims, in load order.
    pub fn shadowed(&self) -> &[(ModId, usize)] {
        &self.shadowed
    }

    /// Whether any claim was shadowed, i.e. whether this id was contested.
    pub fn is_contested(&self) -> bool {
        !self.shadowed.is_empty()
    }
}

/// The deterministic, visible precedence report.
///
/// One entry per content id any mod claims, sorted by id, so two runs over
/// the same manifests serialize byte-for-byte identically (F53 AC01).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrecedenceReport {
    entries: Vec<PrecedenceEntry>,
}

impl PrecedenceReport {
    /// The entries, sorted by content id.
    pub fn entries(&self) -> &[PrecedenceEntry] {
        &self.entries
    }

    /// The entry for one content id, if any mod claims it.
    pub fn entry(&self, target: &ContentId) -> Option<&PrecedenceEntry> {
        self.entries.iter().find(|entry| &entry.target == target)
    }

    /// How many ids were contested by more than one mod.
    pub fn contested_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.is_contested())
            .count()
    }

    /// Every claim that lost, as `target -> (shadowed mod, winner)` in
    /// canonical order. This is the report F53 non-negotiable 1 requires to
    /// be *visible*, as opposed to resolved silently.
    pub fn shadowed_claims(&self) -> Vec<(&ContentId, &ModId, &ModId)> {
        let mut claims: Vec<(&ContentId, &ModId, &ModId)> = self
            .entries
            .iter()
            .flat_map(|entry| {
                entry
                    .shadowed
                    .iter()
                    .map(move |(loser, _)| (&entry.target, loser, &entry.winner))
            })
            .collect();
        claims.sort_by(|a, b| {
            a.0.as_str()
                .cmp(b.0.as_str())
                .then(a.1.as_str().cmp(b.1.as_str()))
        });
        claims
    }
}

/// Why a set of mods cannot be enabled.
///
/// Every reason is collected, not just the first, so a user fixing one mod
/// does not have to re-run to discover the next fault. The list is sorted
/// canonically, so the same set always reports the same problems in the same
/// order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModPlanError {
    problems: Vec<PlanProblem>,
}

impl ModPlanError {
    /// Every reason the set was refused, in canonical order.
    pub fn problems(&self) -> &[PlanProblem] {
        &self.problems
    }

    /// Whether any problem of this kind is present. A caller that only
    /// cares about one class (a selection UI listing the engine-range
    /// failures) does not have to match on the whole enum.
    pub fn contains(&self, problem: &PlanProblem) -> bool {
        self.problems.contains(problem)
    }
}

impl fmt::Display for ModPlanError {
    /// One line per problem, so a report quotes the whole set.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, problem) in self.problems.iter().enumerate() {
            if index > 0 {
                f.write_str("; ")?;
            }
            write!(f, "{problem}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ModPlanError {}

/// One reason a mod set cannot be enabled.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PlanProblem {
    /// Two manifests claim the same mod id, so the set has no single
    /// identity for it.
    DuplicateModId {
        /// The repeated id.
        id: ModId,
    },
    /// A required dependency is not in the set at all.
    MissingDependency {
        /// The mod that requires it.
        id: ModId,
        /// The mod it requires.
        requires: ModId,
    },
    /// A required dependency is in the set at a version outside the declared
    /// range.
    DependencyVersionMismatch {
        /// The mod that requires it.
        id: ModId,
        /// The mod it requires.
        requires: ModId,
        /// The versions it accepts.
        range: VersionRange,
        /// The version the set actually holds.
        found: ModVersion,
    },
    /// A conflict dependency names a mod that is in the set.
    ConflictingMods {
        /// The mod that declared the conflict.
        id: ModId,
        /// The mod it refuses to run beside.
        conflicts_with: ModId,
    },
    /// The dependency graph contains a cycle, so no load order satisfies it.
    ///
    /// A dependency cycle is invalid even though a *content reference* cycle
    /// can be legitimate (`IDENTITY-CONTENT`, "Dependency closure
    /// algorithm"): ownership and enablement order cannot be cyclic.
    DependencyCycle {
        /// The cycle, starting and ending at the same mod.
        cycle: Vec<ModId>,
    },
    /// The running engine is outside the mod's declared window.
    EngineOutOfRange {
        /// The mod.
        id: ModId,
        /// The window it declared.
        range: EngineRange,
        /// The engine the plan is computed under.
        engine: ModVersion,
    },
    /// The mod declares more dependencies than the budget allows.
    TooManyDependencies {
        /// The mod.
        id: ModId,
        /// How many it declared.
        count: usize,
        /// The budget.
        limit: usize,
    },
    /// The mod declares more overrides than the budget allows.
    TooManyOverrides {
        /// The mod.
        id: ModId,
        /// How many it declared.
        count: usize,
        /// The budget.
        limit: usize,
    },
    /// The mod declares more bytes than the per-mod budget allows.
    ModByteBudgetExceeded {
        /// The mod.
        id: ModId,
        /// The bytes it declared.
        declared: u64,
        /// The budget.
        limit: u64,
    },
    /// The set declares more bytes than the total budget allows.
    TotalByteBudgetExceeded {
        /// The bytes the set declared.
        declared: u64,
        /// The budget.
        limit: u64,
    },
    /// The set holds more mods than the budget allows.
    TooManyMods {
        /// How many manifests the set holds.
        count: usize,
        /// The budget.
        limit: usize,
    },
    /// The mod claims to be cosmetic-only while claiming a content id the
    /// effect policy classifies as gameplay.
    ///
    /// F53 non-negotiable 3: the classification comes from the policy, so a
    /// manifest that disagrees with it is refused rather than believed.
    CosmeticOnlyMismatch {
        /// The mod.
        id: ModId,
        /// The gameplay ids it claims, sorted.
        gameplay_targets: Vec<ContentId>,
    },
    /// A mod ships a native library or executable.
    ///
    /// F53 non-negotiable 2: no native DLL/plugin code is ever loaded from a
    /// mod archive, and a manifest that asks for it is refused rather than
    /// quietly stripped.
    NativePayload {
        /// The mod.
        id: ModId,
        /// The file it ships.
        path: String,
    },
    /// A payload or override source spelling is not a safe relative path.
    UnsafePath {
        /// The mod.
        id: ModId,
        /// The spelling as written.
        spelling: String,
    },
    /// Two mods claim the same content id with different actions, so which
    /// one introduces it and which one takes it over cannot both be true.
    ActionCollision {
        /// The contested id.
        target: ContentId,
        /// The mod that adds it.
        adds: ModId,
        /// The mod that replaces it.
        replaces: ModId,
    },
}

impl PlanProblem {
    /// A short, stable code for machine-readable reports.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::DuplicateModId { .. } => "duplicate_mod_id",
            Self::MissingDependency { .. } => "missing_dependency",
            Self::DependencyVersionMismatch { .. } => "dependency_version_mismatch",
            Self::ConflictingMods { .. } => "conflicting_mods",
            Self::DependencyCycle { .. } => "dependency_cycle",
            Self::EngineOutOfRange { .. } => "engine_out_of_range",
            Self::TooManyDependencies { .. } => "too_many_dependencies",
            Self::TooManyOverrides { .. } => "too_many_overrides",
            Self::ModByteBudgetExceeded { .. } => "mod_byte_budget_exceeded",
            Self::TotalByteBudgetExceeded { .. } => "total_byte_budget_exceeded",
            Self::TooManyMods { .. } => "too_many_mods",
            Self::CosmeticOnlyMismatch { .. } => "cosmetic_only_mismatch",
            Self::NativePayload { .. } => "native_payload",
            Self::UnsafePath { .. } => "unsafe_path",
            Self::ActionCollision { .. } => "action_collision",
        }
    }
}

impl fmt::Display for PlanProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateModId { id } => {
                write!(f, "two manifests claim the mod id {id}")
            }
            Self::MissingDependency { id, requires } => {
                write!(f, "mod {id} requires {requires}, which is not in the set")
            }
            Self::DependencyVersionMismatch {
                id,
                requires,
                range,
                found,
            } => write!(
                f,
                "mod {id} requires {requires} {range} but the set has {requires}@{found}"
            ),
            Self::ConflictingMods { id, conflicts_with } => write!(
                f,
                "mod {id} declares a conflict with {conflicts_with}, which is in the set"
            ),
            Self::DependencyCycle { cycle } => {
                let path = cycle
                    .iter()
                    .map(ModId::as_str)
                    .collect::<Vec<_>>()
                    .join(" -> ");
                write!(f, "the dependency graph has a cycle: {path}")
            }
            Self::EngineOutOfRange { id, range, engine } => write!(
                f,
                "mod {id} declares engine {range} but this build is {engine}"
            ),
            Self::TooManyDependencies { id, count, limit } => write!(
                f,
                "mod {id} declares {count} dependencies, the budget is {limit}"
            ),
            Self::TooManyOverrides { id, count, limit } => write!(
                f,
                "mod {id} declares {count} overrides, the budget is {limit}"
            ),
            Self::ModByteBudgetExceeded {
                id,
                declared,
                limit,
            } => write!(
                f,
                "mod {id} declares {declared} bytes, the per-mod budget is {limit}"
            ),
            Self::TotalByteBudgetExceeded { declared, limit } => write!(
                f,
                "the set declares {declared} bytes, the total budget is {limit}"
            ),
            Self::TooManyMods { count, limit } => {
                write!(f, "the set holds {count} mods, the budget is {limit}")
            }
            Self::CosmeticOnlyMismatch {
                id,
                gameplay_targets,
            } => {
                let targets = gameplay_targets
                    .iter()
                    .map(ContentId::as_str)
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(
                    f,
                    "mod {id} claims to be cosmetic-only but claims gameplay content: {targets}"
                )
            }
            Self::NativePayload { id, path } => write!(
                f,
                "mod {id} ships the native payload {path}, which this engine never loads"
            ),
            Self::UnsafePath { id, spelling } => write!(
                f,
                "mod {id} spells a payload path {spelling:?} that could escape the mod root"
            ),
            Self::ActionCollision {
                target,
                adds,
                replaces,
            } => write!(
                f,
                "{adds} adds {target} while {replaces} replaces it; one id cannot be both"
            ),
        }
    }
}

/// A validated, deterministic mount plan for a set of mods.
///
/// The plan is what a mount enables: an order, a visible precedence report
/// and a modification verdict. It carries no bytes and makes no claim about
/// the original game.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModPlan {
    engine: ModVersion,
    order: Vec<ModId>,
    mods: Vec<PlannedMod>,
    precedence: PrecedenceReport,
    modification: ModModification,
    hash: ContentHash,
}

impl ModPlan {
    /// The engine version this plan was computed under, copied from the
    /// request so a report can name the build it belongs to.
    pub fn engine(&self) -> ModVersion {
        self.engine
    }

    /// The deterministic load order, dependencies first.
    ///
    /// A mod that requires another appears after it, so a dependent mod is
    /// mounted on top of what it needs. The order is what
    /// [`ModPlan::mod_stack`] hands to the VFS.
    pub fn order(&self) -> &[ModId] {
        &self.order
    }

    /// The F04-A mod stack this plan mounts, in the same order.
    ///
    /// # Panics
    ///
    /// Never: the order is built from validated, duplicate-free mod ids, so
    /// [`ModStack::new`] cannot reject it.
    pub fn mod_stack(&self) -> ModStack {
        ModStack::new(self.order.clone()).expect("a plan order cannot repeat a mod id")
    }

    /// Every mod in the plan, in load order.
    pub fn mods(&self) -> &[PlannedMod] {
        &self.mods
    }

    /// One mod's plan entry.
    pub fn mod_entry(&self, id: &ModId) -> Option<&PlannedMod> {
        self.mods.iter().find(|entry| &entry.id == id)
    }

    /// The precedence report, one entry per claimed content id.
    pub fn precedence(&self) -> &PrecedenceReport {
        &self.precedence
    }

    /// Whether the set as a whole changes the simulation.
    ///
    /// A `Gameplay` verdict is the marking trigger of F53 non-negotiable 3:
    /// sessions, saves, replays and network handshakes built on this plan
    /// are modified sessions.
    pub fn modification(&self) -> ModModification {
        self.modification
    }

    /// The plan hash: the load order, every mod's id, version, declared
    /// dependencies and overrides, the engine version, the limits and the
    /// resulting precedence, in canonical order.
    ///
    /// This is **not** F53's compatibility signature. A signature has to
    /// cover the *content bytes* a mod resolves to, which this stage never
    /// reads; this hash covers only what the manifests declare, which is
    /// exactly the input such a signature is built from.
    pub fn hash(&self) -> ContentHash {
        self.hash
    }

    /// Whether the plan changed anything in the simulation.
    pub fn marks_sessions(&self) -> bool {
        self.modification.marks_sessions()
    }
}

/// Computes the deterministic mount plan for `set` under `request`.
///
/// The load order is a topological sort of the *required* dependency graph,
/// with ties broken by mod id, so the result depends only on the set's
/// contents and never on the order the manifests were supplied in
/// (F53 AC01). Every problem found is collected; if any exist, the set cannot
/// be enabled and they are all returned together.
///
/// # Errors
///
/// [`ModPlanError`] listing every [`PlanProblem`] found.
pub fn plan_mods(set: &ModSet, request: &MountRequest) -> Result<ModPlan, ModPlanError> {
    let engine = request.engine();
    let limits = *request.limits();
    let mut problems: Vec<PlanProblem> = Vec::new();

    if set.is_empty() {
        return Err(ModPlanError {
            problems: vec![PlanProblem::TooManyMods {
                count: 0,
                limit: limits.max_mods,
            }],
        });
    }

    if set.len() > limits.max_mods {
        problems.push(PlanProblem::TooManyMods {
            count: set.len(),
            limit: limits.max_mods,
        });
    }

    // Index by id, keeping every duplicate as its own problem rather than
    // silently letting the last one win.
    let mut by_id: BTreeMap<ModId, &ModManifest> = BTreeMap::new();
    for manifest in set.manifests() {
        if by_id.insert(manifest.id().clone(), manifest).is_some() {
            problems.push(PlanProblem::DuplicateModId {
                id: manifest.id().clone(),
            });
        }
    }

    let mut total_bytes: u64 = 0;
    for manifest in by_id.values() {
        let id = manifest.id();
        if !manifest.engine().contains(&engine) {
            problems.push(PlanProblem::EngineOutOfRange {
                id: id.clone(),
                range: manifest.engine(),
                engine,
            });
        }
        let dependencies = manifest.dependencies().len();
        if dependencies > limits.max_dependencies {
            problems.push(PlanProblem::TooManyDependencies {
                id: id.clone(),
                count: dependencies,
                limit: limits.max_dependencies,
            });
        }
        let overrides = manifest.override_count();
        if overrides > limits.max_overrides_per_mod {
            problems.push(PlanProblem::TooManyOverrides {
                id: id.clone(),
                count: overrides,
                limit: limits.max_overrides_per_mod,
            });
        }
        let declared = manifest.declared_bytes();
        total_bytes = total_bytes.saturating_add(declared);
        if declared > limits.max_declared_bytes_per_mod {
            problems.push(PlanProblem::ModByteBudgetExceeded {
                id: id.clone(),
                declared,
                limit: limits.max_declared_bytes_per_mod,
            });
        }
        // The cosmetic-only claim is checked against the effect policy, not
        // believed (F53 non-negotiable 3).
        if manifest.declared_cosmetic_only() {
            let mut gameplay: Vec<ContentId> = manifest
                .overrides()
                .iter()
                .filter(|entry| !entry.effect().is_cosmetic())
                .map(|entry| entry.target().clone())
                .collect();
            if !gameplay.is_empty() {
                gameplay.sort();
                problems.push(PlanProblem::CosmeticOnlyMismatch {
                    id: id.clone(),
                    gameplay_targets: gameplay,
                });
            }
        }
    }

    if total_bytes > limits.max_declared_bytes_total {
        problems.push(PlanProblem::TotalByteBudgetExceeded {
            declared: total_bytes,
            limit: limits.max_declared_bytes_total,
        });
    }

    // Dependency and action checks. Only required and conflict rows gate
    // enabling; an optional row is recorded and reported either way.
    let mut required_edges: BTreeMap<ModId, BTreeSet<ModId>> = BTreeMap::new();
    for manifest in by_id.values() {
        let id = manifest.id().clone();
        let mut edges: BTreeSet<ModId> = BTreeSet::new();
        for dependency in manifest.dependencies() {
            let present = by_id.get(dependency.mod_id());
            let satisfied = match (dependency.strength(), present) {
                (DependencyStrength::Required, Some(other)) => {
                    let found = other.version();
                    if dependency.range().contains(&found) {
                        true
                    } else {
                        problems.push(PlanProblem::DependencyVersionMismatch {
                            id: id.clone(),
                            requires: dependency.mod_id().clone(),
                            range: dependency.range(),
                            found,
                        });
                        false
                    }
                }
                (DependencyStrength::Required, None) => {
                    problems.push(PlanProblem::MissingDependency {
                        id: id.clone(),
                        requires: dependency.mod_id().clone(),
                    });
                    false
                }
                (DependencyStrength::Conflict, Some(_)) => {
                    problems.push(PlanProblem::ConflictingMods {
                        id: id.clone(),
                        conflicts_with: dependency.mod_id().clone(),
                    });
                    true
                }
                // A conflict on a mod that is absent is satisfied, and an
                // optional row never gates anything.
                (DependencyStrength::Conflict, None) | (DependencyStrength::Optional, _) => true,
            };
            if dependency.strength() == DependencyStrength::Required && satisfied {
                edges.insert(dependency.mod_id().clone());
            }
        }
        required_edges.insert(id, edges);
    }

    if let Some(cycle) = find_cycle(&required_edges) {
        problems.push(PlanProblem::DependencyCycle { cycle });
    }

    check_actions(&by_id, &mut problems);

    if !problems.is_empty() {
        problems.sort();
        problems.dedup();
        return Err(ModPlanError { problems });
    }

    let order = load_order(&required_edges);
    let mods = plan_mods_in_order(&order, &by_id, &required_edges);
    let precedence = precedence_report(&order, &by_id);
    let modification = if mods
        .iter()
        .any(|entry| entry.modification == ModModification::Gameplay)
    {
        ModModification::Gameplay
    } else {
        ModModification::CosmeticOnly
    };
    let hash = plan_hash(&order, &mods, &precedence, &by_id, request);
    Ok(ModPlan {
        engine,
        order,
        mods,
        precedence,
        modification,
        hash,
    })
}

/// A stable multi-line rendering of a plan, for a diagnostic or a report.
///
/// Derived entirely from the plan and from the manifests it was computed
/// from, in canonical order, so two runs over the same set produce identical
/// text.
#[must_use]
pub fn plan_to_text(plan: &ModPlan) -> String {
    let mut out = String::new();
    out.push_str(&format!("engine {}\n", plan.engine()));
    out.push_str(&format!("modification {}\n", plan.modification()));
    out.push_str(&format!("hash {}\n", plan.hash().to_hex()));
    out.push_str("order\n");
    for (position, id) in plan.order().iter().enumerate() {
        out.push_str(&format!("  {position} {id}\n"));
    }
    out.push_str("precedence\n");
    for entry in plan.precedence().entries() {
        out.push_str(&format!(
            "  {} {} winner {}#{}",
            entry.target(),
            entry.action(),
            entry.winner(),
            entry.winner_position()
        ));
        for (loser, position) in entry.shadowed() {
            out.push_str(&format!(" shadowed {loser}#{position}"));
        }
        out.push('\n');
    }
    out
}

/// Finds one dependency cycle, if any, in the required-dependency graph.
///
/// Edges point from a mod to the mods it requires, so a cycle is a set of
/// mods that cannot be ordered at all. Returned starting and ending at the
/// same mod so a report can print it as a path.
fn find_cycle(edges: &BTreeMap<ModId, BTreeSet<ModId>>) -> Option<Vec<ModId>> {
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        Unvisited,
        InProgress,
        Done,
    }
    let mut marks: BTreeMap<&ModId, Mark> = edges.keys().map(|id| (id, Mark::Unvisited)).collect();
    let mut stack: Vec<&ModId> = Vec::new();

    for start in edges.keys() {
        if marks[start] != Mark::Unvisited {
            continue;
        }
        // Iterative depth-first search: a hostile manifest can be deep, and
        // the bounds here are counts, not recursion depth.
        let mut work: Vec<(&ModId, bool)> = vec![(start, false)];
        while let Some((id, exiting)) = work.pop() {
            if exiting {
                marks.insert(id, Mark::Done);
                stack.pop();
                continue;
            }
            match marks[id] {
                Mark::Done => continue,
                Mark::InProgress => {
                    // A back edge: the cycle is the part of the stack from
                    // the first occurrence of this mod onwards.
                    let start_index = stack
                        .iter()
                        .position(|member| *member == id)
                        .expect("an in-progress mod is on the stack");
                    let mut cycle: Vec<ModId> = stack[start_index..]
                        .iter()
                        .map(|member| (*member).clone())
                        .collect();
                    cycle.push(id.clone());
                    return Some(cycle);
                }
                Mark::Unvisited => {}
            }
            marks.insert(id, Mark::InProgress);
            stack.push(id);
            work.push((id, true));
            if let Some(next) = edges.get(id) {
                for child in next.iter().rev() {
                    work.push((child, false));
                }
            }
        }
    }
    None
}

/// Refuses a content id that one mod adds while another replaces.
///
/// Adding and replacing the same id are contradictory claims about whether
/// the base game has it, and silently picking one would make the resulting
/// content depend on load order. Contesting the *same action* is fine and is
/// reported as precedence instead.
fn check_actions(by_id: &BTreeMap<ModId, &ModManifest>, problems: &mut Vec<PlanProblem>) {
    let mut actions: BTreeMap<&ContentId, BTreeMap<&ModId, OverrideAction>> = BTreeMap::new();
    for (id, manifest) in by_id {
        for entry in manifest.overrides() {
            actions
                .entry(entry.target())
                .or_default()
                .insert(id, entry.action());
        }
    }
    for (target, claimants) in actions {
        let mut adds: Option<&ModId> = None;
        let mut replaces: Option<&ModId> = None;
        for (id, action) in &claimants {
            match action {
                OverrideAction::Add => adds.get_or_insert(id),
                OverrideAction::Replace => replaces.get_or_insert(id),
            };
        }
        if let (Some(adds), Some(replaces)) = (adds, replaces)
            && adds != replaces
        {
            problems.push(PlanProblem::ActionCollision {
                target: target.clone(),
                adds: adds.clone(),
                replaces: replaces.clone(),
            });
        }
    }
}

/// The deterministic load order: a topological sort of the required
/// dependency graph with ties broken by mod id.
///
/// Ties are broken by id rather than by the order the manifests arrived in,
/// so two callers who discovered the same mods in different orders get the
/// same plan and the same hash.
fn load_order(edges: &BTreeMap<ModId, BTreeSet<ModId>>) -> Vec<ModId> {
    let mut remaining: BTreeMap<&ModId, usize> = edges
        .keys()
        .map(|id| (id, edges.get(id).map_or(0, BTreeSet::len)))
        .collect();
    let mut order: Vec<ModId> = Vec::with_capacity(edges.len());
    while let Some(id) = remaining
        .iter()
        .find(|(_, count)| **count == 0)
        .map(|(id, _)| (*id).clone())
    {
        remaining.remove(&id);
        order.push(id.clone());
        // Everything that *requires* `id` is one step closer to being ready.
        for (other, count) in &mut remaining {
            if edges.get(other).is_some_and(|set| set.contains(&id)) {
                *count -= 1;
            }
        }
    }
    order
}

/// Builds the per-mod plan entries in load order.
fn plan_mods_in_order(
    order: &[ModId],
    by_id: &BTreeMap<ModId, &ModManifest>,
    edges: &BTreeMap<ModId, BTreeSet<ModId>>,
) -> Vec<PlannedMod> {
    order
        .iter()
        .enumerate()
        .map(|(position, id)| {
            let manifest = by_id[id];
            let modification = if manifest
                .overrides()
                .iter()
                .any(|entry| !entry.effect().is_cosmetic())
            {
                ModModification::Gameplay
            } else {
                ModModification::CosmeticOnly
            };
            let dependencies = manifest
                .dependencies()
                .iter()
                .map(|dependency| {
                    let satisfied = match dependency.strength() {
                        DependencyStrength::Required => edges
                            .get(id)
                            .is_some_and(|set| set.contains(dependency.mod_id())),
                        DependencyStrength::Conflict => !by_id.contains_key(dependency.mod_id()),
                        DependencyStrength::Optional => by_id
                            .get(dependency.mod_id())
                            .is_some_and(|other| dependency.range().contains(&other.version())),
                    };
                    PlannedDependency {
                        mod_id: dependency.mod_id().clone(),
                        strength: dependency.strength(),
                        range: dependency.range(),
                        satisfied,
                    }
                })
                .collect();
            PlannedMod {
                id: id.clone(),
                version: manifest.version(),
                position,
                modification,
                override_count: manifest.override_count(),
                declared_bytes: manifest.declared_bytes(),
                dependencies,
            }
        })
        .collect()
}

/// Builds the precedence report: one entry per claimed content id, sorted by
/// id, naming the winner (the mod loaded last) and every claim it shadows.
fn precedence_report(order: &[ModId], by_id: &BTreeMap<ModId, &ModManifest>) -> PrecedenceReport {
    let positions: BTreeMap<&ModId, usize> = order
        .iter()
        .enumerate()
        .map(|(position, id)| (id, position))
        .collect();
    let mut claims: BTreeMap<&ContentId, Vec<(&ModId, &ContentOverride)>> = BTreeMap::new();
    for id in order {
        let Some(manifest) = by_id.get(id) else {
            continue;
        };
        for entry in manifest.overrides() {
            claims.entry(entry.target()).or_default().push((id, entry));
        }
    }
    let entries = claims
        .into_iter()
        .map(|(target, mut claimants)| {
            // Claims are collected in load order already, so the last one is
            // the winner and the rest are the shadowed claims, both in
            // order.
            let (winner_id, winner) = claimants
                .pop()
                .expect("a claimed id has at least one claim");
            let shadowed = claimants
                .iter()
                .map(|(id, _)| ((*id).clone(), positions[id]))
                .collect();
            PrecedenceEntry {
                target: target.clone(),
                action: winner.action(),
                validation: winner.validation(),
                effect: winner.effect(),
                winner: winner_id.clone(),
                winner_position: positions[winner_id],
                shadowed,
            }
        })
        .collect();
    PrecedenceReport { entries }
}

/// Hashes the plan's canonical inputs.
fn plan_hash(
    order: &[ModId],
    mods: &[PlannedMod],
    precedence: &PrecedenceReport,
    by_id: &BTreeMap<ModId, &ModManifest>,
    request: &MountRequest,
) -> ContentHash {
    let mut hasher = Sha256::new();
    hasher.update(PLAN_HASH_DOMAIN);
    hasher.update(request.engine().to_string().as_bytes());
    hasher.update(b"\n");
    hasher.update(request.limits().encode().as_bytes());
    hasher.update(b"\norder\n");
    for id in order {
        hasher.update(id.as_str().as_bytes());
        hasher.update(b"\n");
    }
    hasher.update(b"mods\n");
    for entry in mods {
        hasher.update(entry.id.as_str().as_bytes());
        hasher.update(b"\t");
        hasher.update(entry.version.to_string().as_bytes());
        hasher.update(b"\t");
        hasher.update(entry.modification.label().as_bytes());
        hasher.update(b"\t");
        hasher.update(entry.declared_bytes.to_string().as_bytes());
        hasher.update(b"\t");
        for dependency in &entry.dependencies {
            hasher.update(dependency.mod_id.as_str().as_bytes());
            hasher.update(b"\t");
            hasher.update(dependency.strength.label().as_bytes());
            hasher.update(b"\t");
            hasher.update(dependency.range.to_string().as_bytes());
            hasher.update(b"\t");
            hasher.update(if dependency.satisfied { b"ok" } else { b"no" });
            hasher.update(b"\n");
        }
        hasher.update(b"overrides\n");
        let Some(manifest) = by_id.get(&entry.id) else {
            continue;
        };
        // Overrides are folded in sorted by target id, so a manifest that
        // lists its claims in a different order still hashes the same.
        let mut claims: Vec<&ContentOverride> = manifest.overrides().iter().collect();
        claims.sort_by(|left, right| left.target().as_str().cmp(right.target().as_str()));
        for claim in claims {
            hasher.update(claim.target().as_str().as_bytes());
            hasher.update(b"\t");
            hasher.update(claim.action().label().as_bytes());
            hasher.update(b"\t");
            hasher.update(claim.source().as_str().as_bytes());
            hasher.update(b"\t");
            hasher.update(claim.declared_bytes().to_string().as_bytes());
            hasher.update(b"\t");
            hasher.update(claim.effect().label().as_bytes());
            hasher.update(b"\n");
        }
        hasher.update(b"end\n");
    }
    hasher.update(b"precedence\n");
    for entry in precedence.entries() {
        hasher.update(entry.target().as_str().as_bytes());
        hasher.update(b"\t");
        hasher.update(entry.action.label().as_bytes());
        hasher.update(b"\t");
        hasher.update(entry.validation.label().as_bytes());
        hasher.update(b"\t");
        hasher.update(entry.effect.label().as_bytes());
        hasher.update(b"\t");
        hasher.update(entry.winner.as_str().as_bytes());
        hasher.update(b"\t");
        hasher.update(entry.winner_position.to_string().as_bytes());
        hasher.update(b"\n");
    }
    hasher.finalize()
}

// ------------------------------------------------------------- fixtures ----

/// The content ids the synthetic base game provides, as a [`MountRequest`]
/// wants them.
///
/// A designed fixture list covering every branch the plan branches on: two
/// cosmetic presentation ids, two gameplay ids (an airframe and a gun), a
/// paint mask and one id deliberately *absent* so a test can ask for a
/// `Replace` of something nothing provides.
#[must_use]
pub fn synthetic_base_ids() -> Vec<ContentId> {
    [
        (ContentKind::Image, "synthetic.hull-panel"),
        (ContentKind::Material, "synthetic.panel-gloss"),
        (ContentKind::PaintMask, "synthetic.clan-stripe"),
        (ContentKind::Airframe, "synthetic.bolt-ii"),
        (ContentKind::Gun, "synthetic.vulcan"),
    ]
    .into_iter()
    .map(|(kind, key)| {
        ContentId::from_source(kind, key).expect("the synthetic base content id is valid")
    })
    .collect()
}

/// The engine version the synthetic fixtures declare their ranges against.
#[must_use]
pub fn synthetic_engine_version() -> ModVersion {
    ModVersion::new(0, 5, 0)
}

/// A [`MountRequest`] for the synthetic base content, this build's engine
/// version and the default limits.
#[must_use]
pub fn synthetic_mount_request() -> MountRequest {
    MountRequest::new(synthetic_engine_version(), synthetic_base_ids())
}

/// Builds one synthetic manifest from raw parts, with the designed
/// provenance every fixture in this module carries.
fn synthetic_manifest(
    id: &str,
    version: ModVersion,
    declared_cosmetic_only: bool,
    dependencies: Vec<ModDependency>,
    payloads: Vec<ModPayload>,
    overrides: Vec<ContentOverride>,
) -> ModManifest {
    let header = ModHeader::try_new(
        ModId::new(id).expect("the synthetic mod id is valid"),
        id,
        version,
        EngineRange::new(ModVersion::new(0, 1, 0), ModVersion::new(0, 9, 9))
            .expect("the synthetic engine range is valid"),
        declared_cosmetic_only,
        Provenance::designed(synthetic_mod_claim()),
    )
    .expect("the synthetic mod header is valid");
    ModManifest::try_new(header, dependencies, payloads, overrides)
        .expect("the synthetic mod manifest is valid")
}

/// An image override the synthetic mods use.
fn synthetic_image_override(
    key: &str,
    action: OverrideAction,
    source: &str,
    bytes: u64,
) -> ContentOverride {
    ContentOverride::try_new(
        ContentId::from_source(ContentKind::Image, key).expect("the synthetic image id is valid"),
        action,
        source,
        bytes,
    )
    .expect("the synthetic image override is valid")
}

/// The two conflicting cosmetic mods of the AC01 fixture: both replace the
/// same hull panel, and the later one in load order wins.
#[must_use]
pub fn synthetic_conflicting_mods() -> (ModManifest, ModManifest) {
    let first = synthetic_manifest(
        "synthetic.panel-repaint",
        ModVersion::new(1, 2, 0),
        true,
        vec![ModDependency::new(
            ModId::new("synthetic.bright-panels").expect("valid"),
            VersionRange::at_least(ModVersion::new(1, 0, 0)),
            DependencyStrength::Required,
        )],
        vec![ModPayload::new("art/panel.png").expect("valid")],
        vec![
            synthetic_image_override(
                "synthetic.hull-panel",
                OverrideAction::Replace,
                "art/panel.png",
                8_192,
            ),
            synthetic_image_override(
                "synthetic.repaint-stripe",
                OverrideAction::Add,
                "art/stripe.png",
                4_096,
            ),
        ],
    );
    let second = synthetic_manifest(
        "synthetic.bright-panels",
        ModVersion::new(1, 0, 0),
        true,
        Vec::new(),
        vec![ModPayload::new("art/panels.png").expect("valid")],
        vec![synthetic_image_override(
            "synthetic.hull-panel",
            OverrideAction::Replace,
            "art/panels.png",
            6_144,
        )],
    );
    (first, second)
}

/// A tuning mod that replaces a gun, so a plan containing it is a gameplay
/// plan under [`classify_effect`] no matter what the manifest claims.
#[must_use]
pub fn synthetic_tuning_mod() -> ModManifest {
    synthetic_manifest(
        "synthetic.tuning-pack",
        ModVersion::new(0, 9, 0),
        false,
        Vec::new(),
        vec![ModPayload::new("tuning/vulcan.toml").expect("valid")],
        vec![
            ContentOverride::try_new(
                ContentId::from_source(ContentKind::Gun, "synthetic.vulcan")
                    .expect("the synthetic gun id is valid"),
                OverrideAction::Replace,
                "tuning/vulcan.toml",
                512,
            )
            .expect("the synthetic gun override is valid"),
        ],
    )
}

/// A mod that claims to be cosmetic-only while claiming a gun, which
/// [`classify_effect`] says is gameplay.
///
/// It cannot be built through [`ModManifest::try_new`] with a valid
/// manifest unless the claim is a lie, which is exactly the point: the
/// manifest is *structurally* valid and the plan is what refuses it.
#[must_use]
pub fn synthetic_false_cosmetic_mod() -> ModManifest {
    synthetic_manifest(
        "synthetic.false-cosmetic",
        ModVersion::new(1, 0, 0),
        true,
        Vec::new(),
        Vec::new(),
        vec![
            ContentOverride::try_new(
                ContentId::from_source(ContentKind::Gun, "synthetic.vulcan")
                    .expect("the synthetic gun id is valid"),
                OverrideAction::Replace,
                "tuning/vulcan.toml",
                512,
            )
            .expect("the synthetic gun override is valid"),
        ],
    )
}

/// Two mods that require each other, which no load order can satisfy.
#[must_use]
pub fn synthetic_cyclic_mods() -> (ModManifest, ModManifest) {
    let first = synthetic_manifest(
        "synthetic.cycle-a",
        ModVersion::new(1, 0, 0),
        true,
        vec![ModDependency::new(
            ModId::new("synthetic.cycle-b").expect("valid"),
            VersionRange::at_least(ModVersion::new(1, 0, 0)),
            DependencyStrength::Required,
        )],
        Vec::new(),
        Vec::new(),
    );
    let second = synthetic_manifest(
        "synthetic.cycle-b",
        ModVersion::new(1, 0, 0),
        true,
        vec![ModDependency::new(
            ModId::new("synthetic.cycle-a").expect("valid"),
            VersionRange::at_least(ModVersion::new(1, 0, 0)),
            DependencyStrength::Required,
        )],
        Vec::new(),
        Vec::new(),
    );
    (first, second)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cs_types::content::{ContentKind, Provenance};

    fn mod_id(label: &str) -> ModId {
        ModId::new(label).expect("the fixture mod id is valid")
    }

    fn ac01_plan() -> ModPlan {
        let (first, second) = synthetic_conflicting_mods();
        plan_mods(
            &ModSet::new(vec![first, second]),
            &synthetic_mount_request(),
        )
        .expect("the two conflicting mods plan cleanly")
    }

    /// **F53 AC01.** Two mods claim one content id. The plan names the
    /// winner, names the loser, and the winner is the one loaded last, which
    /// is the same precedence rule F04-A's [`ModStack`] applies. Nothing
    /// fails silently and nothing depends on which manifest was supplied
    /// first.
    #[test]
    fn accept_f53_a_two_conflicting_mods_produce_a_deterministic_precedence_report() {
        let plan = ac01_plan();
        let panel =
            ContentId::from_source(ContentKind::Image, "synthetic.hull-panel").expect("valid id");

        let entry = plan
            .precedence()
            .entry(&panel)
            .expect("both mods claim the panel, so it has an entry");
        assert!(entry.is_contested());
        assert_eq!(entry.shadowed().len(), 1);
        assert_eq!(
            entry.winner().as_str(),
            "synthetic.panel-repaint",
            "the mod loaded last wins the contested id"
        );
        assert_eq!(
            entry.shadowed()[0].0.as_str(),
            "synthetic.bright-panels",
            "the earlier mod is named as the shadowed claim"
        );
        assert_eq!(entry.action(), OverrideAction::Replace);
        assert_eq!(entry.effect(), OverrideEffect::Cosmetic);
        assert_eq!(entry.validation(), OverrideValidation::OriginalAdapter);

        // The winner's position is strictly higher than the loser's, and
        // those positions are the F04-A mod-stack positions, so the report
        // and the VFS agree about which mod wins.
        let stack = plan.mod_stack();
        let winner_position = stack
            .position(entry.winner())
            .expect("the winner is in the stack");
        let loser_position = stack
            .position(&entry.shadowed()[0].0)
            .expect("the loser is in the stack");
        assert!(winner_position > loser_position);
        assert_eq!(entry.winner_position(), winner_position);
        assert_eq!(entry.shadowed()[0].1, loser_position);

        // The report is *visible*: it names every shadowed claim with its
        // winner, in canonical order.
        let shadowed = plan.precedence().shadowed_claims();
        assert_eq!(shadowed.len(), 1);
        assert_eq!(shadowed[0].0, &panel);
        assert_eq!(shadowed[0].1.as_str(), "synthetic.bright-panels");
        assert_eq!(shadowed[0].2.as_str(), "synthetic.panel-repaint");
        assert_eq!(plan.precedence().contested_count(), 1);

        // The uncontested claim is reported too, so a report is a complete
        // picture of what the set claims rather than only its conflicts.
        let stripe = ContentId::from_source(ContentKind::Image, "synthetic.repaint-stripe")
            .expect("valid id");
        let stripe_entry = plan
            .precedence()
            .entry(&stripe)
            .expect("the added stripe has an entry");
        assert!(!stripe_entry.is_contested());
        assert_eq!(stripe_entry.action(), OverrideAction::Add);
        assert_eq!(stripe_entry.winner().as_str(), "synthetic.panel-repaint");
        assert_eq!(plan.precedence().entries().len(), 2);

        // The dependency is honoured: the required mod is mounted first, so
        // the dependent mod is on top of it.
        assert_eq!(
            plan.order(),
            [
                mod_id("synthetic.bright-panels"),
                mod_id("synthetic.panel-repaint"),
            ]
        );
        assert!(
            plan.mod_stack()
                .contains(&mod_id("synthetic.bright-panels"))
        );

        // The order is a *dependency* order, not an alphabetical one. The
        // fixture above cannot tell the two apart, because its required mod
        // happens to sort first. This pair is built so it can: the dependent
        // mod's id sorts *before* its dependency's, so a plain alphabetical
        // order would mount it first and topple it.
        let ordered = plan_mods(
            &ModSet::new(vec![
                synthetic_manifest_with_dependencies(
                    "synthetic.aaa-user",
                    ModVersion::new(1, 0, 0),
                    true,
                    vec![ModDependency::new(
                        mod_id("synthetic.zzz-lib"),
                        VersionRange::at_least(ModVersion::new(1, 0, 0)),
                        DependencyStrength::Required,
                    )],
                ),
                synthetic_manifest_with_dependencies(
                    "synthetic.zzz-lib",
                    ModVersion::new(1, 0, 0),
                    true,
                    Vec::new(),
                ),
            ]),
            &synthetic_mount_request(),
        )
        .expect("a satisfied dependency plans");
        assert_eq!(
            ordered.order(),
            [mod_id("synthetic.zzz-lib"), mod_id("synthetic.aaa-user")],
            "a required mod is mounted before the mod that requires it, whatever their ids sort like"
        );
    }

    /// Determinism is the property AC01 actually names: the same set of
    /// manifests plans the same way no matter what order they arrive in, in
    /// the order, the report, the text and the hash.
    #[test]
    fn accept_f53_a_the_plan_is_identical_whatever_order_mods_arrive_in() {
        let (first, second) = synthetic_conflicting_mods();
        let forward = plan_mods(
            &ModSet::new(vec![first.clone(), second.clone()]),
            &synthetic_mount_request(),
        )
        .expect("forward plans");
        let reversed = plan_mods(
            &ModSet::new(vec![second.clone(), first.clone()]),
            &synthetic_mount_request(),
        )
        .expect("reversed plans");

        assert_eq!(forward.order(), reversed.order());
        assert_eq!(forward.precedence(), reversed.precedence());
        assert_eq!(forward.hash(), reversed.hash());
        assert_eq!(plan_to_text(&forward), plan_to_text(&reversed));
        assert_eq!(forward.mods(), reversed.mods());
        assert_eq!(forward.modification(), reversed.modification());
    }

    /// A required dependency that is absent, one that is present at the
    /// wrong version, and a declared conflict are all refused — and all
    /// reported together, so fixing one fault does not hide the next.
    #[test]
    fn accept_f53_a_unsatisfiable_dependencies_are_all_reported() {
        let (first, _second) = synthetic_conflicting_mods();
        let absent = plan_mods(
            &ModSet::new(vec![first.clone()]),
            &synthetic_mount_request(),
        )
        .expect_err("a required dependency outside the set cannot be planned");
        assert_eq!(
            absent.problems(),
            [PlanProblem::MissingDependency {
                id: mod_id("synthetic.panel-repaint"),
                requires: mod_id("synthetic.bright-panels"),
            }]
        );
        assert!(
            absent.contains(&PlanProblem::MissingDependency {
                id: mod_id("synthetic.panel-repaint"),
                requires: mod_id("synthetic.bright-panels"),
            }),
            "a caller can look for one problem without matching the whole list"
        );
        assert_eq!(
            absent.to_string(),
            "mod synthetic.panel-repaint requires synthetic.bright-panels, which is not in the set"
        );

        // Present, but at a version the range excludes.
        let (_bright, second) = synthetic_conflicting_mods();
        let repaint = ModManifest::try_new(
            ModHeader::try_new(
                mod_id("synthetic.panel-repaint"),
                "synthetic.panel-repaint",
                ModVersion::new(1, 2, 0),
                EngineRange::new(ModVersion::new(0, 1, 0), ModVersion::new(0, 9, 9))
                    .expect("valid"),
                true,
                Provenance::designed(synthetic_mod_claim()),
            )
            .expect("valid"),
            vec![ModDependency::new(
                mod_id("synthetic.bright-panels"),
                VersionRange::between(ModVersion::new(2, 0, 0), ModVersion::new(3, 0, 0))
                    .expect("valid"),
                DependencyStrength::Required,
            )],
            Vec::new(),
            Vec::new(),
        )
        .expect("structurally valid");
        let mismatch = plan_mods(
            &ModSet::new(vec![repaint, second]),
            &synthetic_mount_request(),
        )
        .expect_err("a version outside the range cannot be planned");
        assert_eq!(
            mismatch.problems(),
            [PlanProblem::DependencyVersionMismatch {
                id: mod_id("synthetic.panel-repaint"),
                requires: mod_id("synthetic.bright-panels"),
                range: VersionRange::between(ModVersion::new(2, 0, 0), ModVersion::new(3, 0, 0),)
                    .expect("valid"),
                found: ModVersion::new(1, 0, 0),
            }]
        );

        // A declared conflict with a mod that is present. The conflicting
        // pair is built from scratch with conflict rows only, so the two
        // conflicts reported here are the only problems in the set.
        let a = synthetic_manifest_with_dependencies(
            "synthetic.refuses-b",
            ModVersion::new(1, 0, 0),
            true,
            vec![ModDependency::new(
                mod_id("synthetic.refuses-a"),
                VersionRange::at_least(ModVersion::new(0, 0, 1)),
                DependencyStrength::Conflict,
            )],
        );
        let b = synthetic_manifest_with_dependencies(
            "synthetic.refuses-a",
            ModVersion::new(1, 0, 0),
            true,
            vec![ModDependency::new(
                mod_id("synthetic.refuses-b"),
                VersionRange::at_least(ModVersion::new(0, 0, 1)),
                DependencyStrength::Conflict,
            )],
        );
        let conflicting = plan_mods(&ModSet::new(vec![a, b]), &synthetic_mount_request())
            .expect_err("two mods that each refuse the other cannot be planned");
        assert_eq!(
            conflicting.problems(),
            [
                PlanProblem::ConflictingMods {
                    id: mod_id("synthetic.refuses-a"),
                    conflicts_with: mod_id("synthetic.refuses-b"),
                },
                PlanProblem::ConflictingMods {
                    id: mod_id("synthetic.refuses-b"),
                    conflicts_with: mod_id("synthetic.refuses-a"),
                },
            ],
            "both directions of the conflict are named, in canonical order"
        );
    }

    /// F53 AC02's dependency half. A cycle has no topological order, so no
    /// mount can satisfy it; the plan refuses and prints the cycle as a path
    /// so the author can see it.
    #[test]
    fn accept_f53_a_a_cyclic_dependency_is_rejected() {
        let (first, second) = synthetic_cyclic_mods();
        let refused = plan_mods(
            &ModSet::new(vec![first, second]),
            &synthetic_mount_request(),
        )
        .expect_err("a dependency cycle has no load order");
        let cycle = refused
            .problems()
            .iter()
            .find_map(|problem| match problem {
                PlanProblem::DependencyCycle { cycle } => Some(cycle.clone()),
                _ => None,
            })
            .expect("the refusal names the cycle");
        assert!(
            cycle.len() >= 2,
            "a cycle visits at least the two mods that require each other"
        );
        assert_eq!(cycle.first(), cycle.last(), "the path closes on itself");
        assert!(cycle.contains(&mod_id("synthetic.cycle-a")));
        assert!(cycle.contains(&mod_id("synthetic.cycle-b")));
        assert!(
            refused
                .to_string()
                .contains("the dependency graph has a cycle"),
            "the report says what went wrong: {}",
            refused
        );
        assert_eq!(
            refused.problems()[0].code(),
            "dependency_cycle",
            "a caller can branch on a stable code"
        );
    }

    /// A manifest that claims to be cosmetic-only while claiming a gameplay
    /// id is refused, and the refusal is derived from the effect policy
    /// rather than from the author's flag. A mod that is honest about
    /// changing gameplay plans cleanly and marks its sessions.
    #[test]
    fn accept_f53_a_the_cosmetic_only_claim_is_checked_against_the_policy() {
        let refused = plan_mods(
            &ModSet::new(vec![synthetic_false_cosmetic_mod()]),
            &synthetic_mount_request(),
        )
        .expect_err("a false cosmetic claim cannot be planned");
        assert_eq!(
            refused.problems(),
            [PlanProblem::CosmeticOnlyMismatch {
                id: mod_id("synthetic.false-cosmetic"),
                gameplay_targets: vec![
                    ContentId::from_source(ContentKind::Gun, "synthetic.vulcan").expect("valid")
                ],
            }]
        );
        assert_eq!(refused.problems()[0].code(), "cosmetic_only_mismatch");

        // The honest tuning mod plans, and the plan marks sessions because
        // its effect is gameplay.
        let plan = plan_mods(
            &ModSet::new(vec![synthetic_tuning_mod()]),
            &synthetic_mount_request(),
        )
        .expect("an honest gameplay mod plans");
        assert_eq!(plan.modification(), ModModification::Gameplay);
        assert!(plan.marks_sessions());
        assert_eq!(
            plan.mod_entry(&mod_id("synthetic.tuning-pack"))
                .expect("the mod has an entry")
                .modification(),
            ModModification::Gameplay
        );

        // The two cosmetic mods alone are a cosmetic-only plan that does not
        // mark sessions.
        let (first, second) = synthetic_conflicting_mods();
        let cosmetic = plan_mods(
            &ModSet::new(vec![first, second]),
            &synthetic_mount_request(),
        )
        .expect("the cosmetic mods plan");
        assert_eq!(cosmetic.modification(), ModModification::CosmeticOnly);
        assert!(!cosmetic.marks_sessions());
    }

    /// Every budget is checked, and each one is a real limit: dropping a
    /// check would let a manifest exceed it silently.
    #[test]
    fn accept_f53_a_budgets_are_enforced() {
        let request = MountRequest::new(synthetic_engine_version(), synthetic_base_ids())
            .with_limits(MountLimits {
                max_mods: 1,
                max_dependencies: 1,
                max_overrides_per_mod: 1,
                max_declared_bytes_per_mod: 8_192,
                max_declared_bytes_total: 16_384,
            });
        let (first, second) = synthetic_conflicting_mods();
        let refused = plan_mods(&ModSet::new(vec![first, second]), &request)
            .expect_err("two mods exceed a one-mod budget");
        assert_eq!(
            refused.problems(),
            [
                PlanProblem::TooManyOverrides {
                    id: mod_id("synthetic.panel-repaint"),
                    count: 2,
                    limit: 1,
                },
                PlanProblem::ModByteBudgetExceeded {
                    id: mod_id("synthetic.panel-repaint"),
                    declared: 12_288,
                    limit: 8_192,
                },
                PlanProblem::TotalByteBudgetExceeded {
                    declared: 18_432,
                    limit: 16_384,
                },
                PlanProblem::TooManyMods { count: 2, limit: 1 },
            ],
            "all four budget problems are reported, in canonical order"
        );

        // The per-mod byte budget on its own.
        let tight = MountRequest::new(synthetic_engine_version(), synthetic_base_ids())
            .with_limits(MountLimits {
                max_declared_bytes_per_mod: 256,
                ..MountLimits::default()
            });
        let refused = plan_mods(&ModSet::new(vec![synthetic_tuning_mod()]), &tight)
            .expect_err("512 declared bytes exceed a 256-byte per-mod budget");
        assert_eq!(
            refused.problems(),
            [PlanProblem::ModByteBudgetExceeded {
                id: mod_id("synthetic.tuning-pack"),
                declared: 512,
                limit: 256,
            }],
            "the per-mod byte budget is enforced on its own"
        );

        // The dependency budget on its own.
        let narrow = MountRequest::new(synthetic_engine_version(), synthetic_base_ids())
            .with_limits(MountLimits {
                max_dependencies: 0,
                ..MountLimits::default()
            });
        let (first, second) = synthetic_conflicting_mods();
        let refused = plan_mods(&ModSet::new(vec![first, second]), &narrow)
            .expect_err("a declared dependency exceeds a zero-dependency budget");
        assert_eq!(
            refused.problems(),
            [PlanProblem::TooManyDependencies {
                id: mod_id("synthetic.panel-repaint"),
                count: 1,
                limit: 0,
            }]
        );
    }

    /// The engine range is validated against the running build, in both
    /// directions: a mod that needs a newer engine and one that only supports
    /// an older one are both refused, and a mod whose window contains this
    /// build is not.
    #[test]
    fn accept_f53_a_the_declared_engine_range_is_validated() {
        let too_new = ModManifest::try_new(
            ModHeader::try_new(
                mod_id("synthetic.future"),
                "future",
                ModVersion::new(1, 0, 0),
                EngineRange::new(ModVersion::new(1, 0, 0), ModVersion::new(2, 0, 0))
                    .expect("valid"),
                true,
                Provenance::designed(synthetic_mod_claim()),
            )
            .expect("valid"),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .expect("valid");
        let too_old = ModManifest::try_new(
            ModHeader::try_new(
                mod_id("synthetic.legacy"),
                "legacy",
                ModVersion::new(1, 0, 0),
                EngineRange::new(ModVersion::new(0, 0, 1), ModVersion::new(0, 4, 9))
                    .expect("valid"),
                true,
                Provenance::designed(synthetic_mod_claim()),
            )
            .expect("valid"),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .expect("valid");
        let refused = plan_mods(
            &ModSet::new(vec![too_new, too_old]),
            &synthetic_mount_request(),
        )
        .expect_err("neither window contains this build");
        assert_eq!(
            refused.problems(),
            [
                PlanProblem::EngineOutOfRange {
                    id: mod_id("synthetic.future"),
                    range: EngineRange::new(ModVersion::new(1, 0, 0), ModVersion::new(2, 0, 0))
                        .expect("valid"),
                    engine: ModVersion::new(0, 5, 0),
                },
                PlanProblem::EngineOutOfRange {
                    id: mod_id("synthetic.legacy"),
                    range: EngineRange::new(ModVersion::new(0, 0, 1), ModVersion::new(0, 4, 9))
                        .expect("valid"),
                    engine: ModVersion::new(0, 5, 0),
                },
            ]
        );
        assert_eq!(refused.problems()[0].code(), "engine_out_of_range");
    }

    /// The plan hash covers what the manifests declare, so a different set
    /// of declarations hashes differently, and the limits and engine version
    /// are part of it. It is explicitly *not* a content signature: no byte of
    /// any mod is read at this stage.
    #[test]
    fn accept_f53_a_the_plan_hash_covers_the_declarations_and_the_request() {
        let (first, second) = synthetic_conflicting_mods();
        let base = plan_mods(
            &ModSet::new(vec![first.clone(), second.clone()]),
            &synthetic_mount_request(),
        )
        .expect("plans");
        let with_tuning = plan_mods(
            &ModSet::new(vec![first, second, synthetic_tuning_mod()]),
            &synthetic_mount_request(),
        )
        .expect("plans");
        assert_ne!(base.hash(), with_tuning.hash());

        // The same set under different limits is a different plan.
        let other_limits = plan_mods(
            &ModSet::new(vec![
                synthetic_conflicting_mods().0,
                synthetic_conflicting_mods().1,
            ]),
            &synthetic_mount_request().with_limits(MountLimits {
                max_mods: 2,
                ..MountLimits::default()
            }),
        )
        .expect("plans");
        assert_ne!(base.hash(), other_limits.hash());

        // The same set under a different engine version is a different plan,
        // even though the manifests themselves are untouched.
        let other_engine = plan_mods(
            &ModSet::new(vec![
                synthetic_conflicting_mods().0,
                synthetic_conflicting_mods().1,
            ]),
            &MountRequest::new(ModVersion::new(0, 6, 0), synthetic_base_ids()),
        )
        .expect("plans");
        assert_ne!(base.hash(), other_engine.hash());

        // The hash is a canonical lowercase hex digest, per IDENTITY-CONTENT.
        let hex = base.hash().to_hex();
        assert_eq!(hex.len(), 64);
        assert!(
            hex.chars()
                .all(|ch| ch.is_ascii_hexdigit() && !ch.is_uppercase())
        );
    }

    /// Two mods claiming one id with *different* actions contradict each
    /// other about whether the base game has it, so the plan refuses rather
    /// than picking a winner.
    #[test]
    fn accept_f53_a_add_and_replace_of_one_id_collide() {
        let (mut adder, mut replacer) = synthetic_conflicting_mods();
        adder = with_extra_override(
            &adder,
            synthetic_image_override(
                "synthetic.clan-stripe",
                OverrideAction::Add,
                "art/added.png",
                128,
            ),
        );
        replacer = with_extra_override(
            &replacer,
            synthetic_image_override(
                "synthetic.clan-stripe",
                OverrideAction::Replace,
                "art/replaced.png",
                128,
            ),
        );
        let refused = plan_mods(
            &ModSet::new(vec![adder, replacer]),
            &synthetic_mount_request(),
        )
        .expect_err("one id cannot be both added and replaced");
        assert_eq!(
            refused.problems(),
            [PlanProblem::ActionCollision {
                target: ContentId::from_source(ContentKind::Image, "synthetic.clan-stripe")
                    .expect("valid"),
                adds: mod_id("synthetic.panel-repaint"),
                replaces: mod_id("synthetic.bright-panels"),
            }]
        );
        assert_eq!(refused.problems()[0].code(), "action_collision");
    }

    /// A duplicate mod id is a collision the set cannot resolve, and an
    /// empty set is refused rather than planned as an empty, valid plan.
    #[test]
    fn accept_f53_a_duplicate_ids_and_empty_sets_are_refused() {
        let (first, _) = synthetic_conflicting_mods();
        let duplicate = plan_mods(
            &ModSet::new(vec![first.clone(), first]),
            &synthetic_mount_request(),
        )
        .expect_err("two manifests with one id have no single identity");
        assert_eq!(
            duplicate.problems(),
            [
                PlanProblem::DuplicateModId {
                    id: mod_id("synthetic.panel-repaint"),
                },
                // The second copy of the manifest is still validated on its
                // own terms: its required dependency is absent from a set
                // that holds only it, so both faults are named at once.
                PlanProblem::MissingDependency {
                    id: mod_id("synthetic.panel-repaint"),
                    requires: mod_id("synthetic.bright-panels"),
                },
            ],
            "the collision and the dependency fault are both reported, canonically ordered"
        );
        assert_eq!(duplicate.problems()[0].code(), "duplicate_mod_id");

        let empty = plan_mods(&ModSet::default(), &synthetic_mount_request())
            .expect_err("an empty set is not a mountable set");
        assert_eq!(
            empty.problems(),
            [PlanProblem::TooManyMods {
                count: 0,
                limit: MountLimits::default().max_mods,
            }]
        );
    }

    /// A mod that ships native code is refused by extension, whatever the
    /// manifest claims, and a hostile source spelling never becomes a
    /// record. The two rules F53 non-negotiable 2 and AC02 need are enforced
    /// by construction, not by a caller remembering to check.
    #[test]
    fn accept_f53_a_native_payloads_and_unsafe_paths_cannot_reach_a_plan() {
        let native = ModPayload::new("bin/plugin.dll").expect("the spelling is safe");
        let refused = ModManifest::try_new(
            ModHeader::try_new(
                mod_id("synthetic.native"),
                "native",
                ModVersion::new(1, 0, 0),
                EngineRange::new(ModVersion::new(0, 1, 0), ModVersion::new(0, 9, 9))
                    .expect("valid"),
                true,
                Provenance::designed(synthetic_mod_claim()),
            )
            .expect("valid"),
            Vec::new(),
            vec![native],
            Vec::new(),
        )
        .expect_err("a manifest that ships native code is refused");
        assert_eq!(
            refused,
            ManifestError::NativePayloadForbidden {
                id: mod_id("synthetic.native"),
                path: "bin/plugin.dll".to_owned(),
            }
        );

        for hostile in [
            "../outside.png",
            "/etc/passwd",
            "art/../../x.png",
            "C:/x.png",
        ] {
            let refused = ContentOverride::try_new(
                ContentId::from_source(ContentKind::Image, "synthetic.hull-panel").expect("valid"),
                OverrideAction::Replace,
                hostile,
                1,
            )
            .expect_err("a hostile source spelling is refused");
            assert!(matches!(refused, ManifestError::UnsafePath { .. }));
        }
    }

    /// The plan records what each mod declared and what the plan concluded
    /// about it, so a report never has to re-derive the numbers.
    #[test]
    fn accept_f53_a_the_plan_reports_each_mod_s_declarations() {
        let plan = ac01_plan();
        let repaint = plan
            .mod_entry(&mod_id("synthetic.panel-repaint"))
            .expect("the mod has an entry");
        assert_eq!(repaint.version(), ModVersion::new(1, 2, 0));
        assert_eq!(repaint.override_count(), 2);
        assert_eq!(repaint.declared_bytes(), 12_288);
        assert_eq!(repaint.modification(), ModModification::CosmeticOnly);
        assert_eq!(repaint.dependencies().len(), 1);
        assert_eq!(
            repaint.dependencies()[0].mod_id.as_str(),
            "synthetic.bright-panels"
        );
        assert!(repaint.dependencies()[0].satisfied);
        assert_eq!(
            repaint.dependencies()[0].strength,
            DependencyStrength::Required
        );

        let bright = plan
            .mod_entry(&mod_id("synthetic.bright-panels"))
            .expect("the mod has an entry");
        assert_eq!(bright.override_count(), 1);
        assert_eq!(bright.declared_bytes(), 6_144);
        assert!(bright.dependencies().is_empty());
        assert_eq!(plan.mods().len(), 2);

        // The text report names the build, the verdict, the hash, the order
        // and the contested id with both claimants.
        let text = plan_to_text(&plan);
        assert!(text.contains("engine 0.5.0"));
        assert!(text.contains("modification cosmetic_only"));
        assert!(text.contains(&format!("hash {}", plan.hash().to_hex())));
        assert!(text.contains("0 synthetic.bright-panels"));
        assert!(text.contains("1 synthetic.panel-repaint"));
        assert!(text.contains("image/synthetic.hull-panel"));
        assert!(text.contains("shadowed synthetic.bright-panels#0"));
    }

    // --------------------------------------------------------- helpers ----

    /// A synthetic manifest with dependency rows and no content, for the
    /// dependency-graph cases that need a set with nothing else in it.
    fn synthetic_manifest_with_dependencies(
        id: &str,
        version: ModVersion,
        declared_cosmetic_only: bool,
        dependencies: Vec<ModDependency>,
    ) -> ModManifest {
        synthetic_manifest(
            id,
            version,
            declared_cosmetic_only,
            dependencies,
            Vec::new(),
            Vec::new(),
        )
    }

    /// Rebuilds a manifest with one extra override row.
    fn with_extra_override(manifest: &ModManifest, extra: ContentOverride) -> ModManifest {
        let mut overrides = manifest.overrides().to_vec();
        overrides.push(extra);
        ModManifest::try_new(
            ModHeader::try_new(
                manifest.id().clone(),
                manifest.header().name(),
                manifest.version(),
                manifest.engine(),
                manifest.declared_cosmetic_only(),
                manifest.provenance().clone(),
            )
            .expect("the header is valid"),
            manifest.dependencies().to_vec(),
            manifest.payloads().to_vec(),
            overrides,
        )
        .expect("the rebuilt manifest is valid")
    }
}
