//! The loading-plan adapter: a decoded INTERP container in, the assets a
//! world needs out (F07-C).
//!
//! Spec `specs/F07-interp-loading-script-container.md` (`### F07-C`),
//! `docs/contracts/SCRIPT-MISSION.md`. The producer of this stage's input is
//! `cs_formats::decode_interp` and its classifier is
//! `cs_formats::plan_interp_loading`; this module is the **consumer**: it is
//! the first place a decoded container is read as a *loading plan* rather than
//! as a table of tokens, and the only place a plan is connected to the
//! virtual filesystem.
//!
//! What it does, and nothing more:
//!
//! * every script of the plan becomes a [`LoadingScript`] with its
//!   [`ScriptOrigin`] and the SHA-256 of **its own bytes**
//!   (`script_offset..end`), which is its identity. The `timestamp` word is
//!   carried as metadata and is never hashed, compared or used as a key
//!   (spec non-negotiable #5: content hashes determine identity). Two scripts
//!   with equal names, equal timestamps and equal bodies keep distinct origins;
//!   two scripts with equal names and different bodies additionally differ in
//!   content hash, which is what a consumer would key a cache on;
//! * every registered loading command becomes one [`LoadingDependency`]
//!   carrying the [`AssetKey`] its arguments spell, where the key was asked
//!   for (script index, line position, the line's and the head token's
//!   absolute offsets) and what the VFS answered: a resolved [`SourceSpan`],
//!   a refusal with a code and detail, or a `Composed` key, whose value is
//!   assembled at run time and therefore has nothing to resolve here.
//!   `docs/contracts/SCRIPT-MISSION.md` asks for exactly this tally —
//!   resolved host calls and dynamic lookups counted separately;
//! * every unclassified, malformed or **unsupported** line becomes a
//!   [`LoadingFailure`] with its source offset and the **affected world**,
//!   which is the world the resolving session's context selected — never a
//!   name guessed from the script's spelling and never a fake loaded state.
//!   An unsupported line is one whose command is recognized as a resource
//!   load but has no key domain this stage can spell (a directory, a
//!   `%VARIABLE%` spelling): it fails with `unsupported_command` instead of
//!   being treated as loaded or as no load at all (spec F07-D, AC04). With
//!   the shipped-empty command table every line of every container fails this
//!   way, which is the honest answer: nobody has measured which commands load
//!   resources (non-negotiable #3, and F07-D is the stage that measures it);
//! * a line whose command is classified as **not** a resource load
//!   (`PlanLineKind::Behavior`) becomes no dependency and no failure: the
//!   world is not unloadable because a scene or camera command sits beside its
//!   loading commands. This stage does not interpret it either, so it is
//!   neither resolved nor reported;
//! * the report is a **value**. It holds no file handle and no session, and
//!   its dependencies are stamped with the session generation that resolved
//!   them, so a report cannot be read through a session that replaced the one
//!   that produced it ([`LoadingPlanReport::read_dependency`]). Teardown is
//!   the session's own ([`ContentSession::close`]); a caller that must retry
//!   builds a second report, which starts from nothing.
//!
//! Argument bytes are never decoded. A key is built from the bytes as stored,
//! through [`AssetKey::from_spelling`], and bytes that are not a valid
//! namespace, path and variant are a refusal with the reason
//! [`cs_types`] gave — not a lossy `String` and not a repaired path. An
//! original command that spells `%VARIABLE%` or a `..` path is therefore
//! reported, not silently resolved (spec non-negotiable #1).
//!
//! ```
//! use cs_content::loading::{resolve_loading_plan, LoadingPlanReport};
//! use cs_formats::{LoadCommandTable, ParseContext, decode_interp, plan_interp_loading};
//!
//! // A container whose single line names a command no table registers.
//! let mut bytes = Vec::new();
//! for word in [0x0897_1119u32, 7, 1] {
//!     bytes.extend_from_slice(&word.to_le_bytes());
//! }
//! let mut name = [0u8; 120];
//! name[..4].copy_from_slice(b"demo");
//! bytes.extend_from_slice(&name);
//! bytes.extend_from_slice(&0u32.to_le_bytes());
//! bytes.extend_from_slice(&140u32.to_le_bytes());
//! bytes.extend_from_slice(&10u32.to_le_bytes()); // size
//! bytes.extend_from_slice(&2u32.to_le_bytes()); // argument count
//! bytes.extend_from_slice(b"cmd\0a.flt\0");
//! bytes.extend_from_slice(&0u32.to_le_bytes());
//!
//! let table = LoadCommandTable::new();
//! let decoded = decode_interp(&mut ParseContext::with_defaults("demo.interp"), &bytes)
//!     .expect("the container validates");
//! let plan = plan_interp_loading(&decoded, &table);
//! // The report is pure: it needs no session, because with no registered
//! // command there is nothing to resolve.
//! let report = resolve_loading_plan(None, &decoded, &plan)
//!     .expect("the container's own extents are consistent");
//! assert!(!report.is_complete());
//! assert_eq!(report.stats().unclassified_commands, 1);
//! assert_eq!(report.failures().len(), 1);
//! assert_eq!(report.dependencies().len(), 0);
//! assert!(LoadingPlanReport::describe(&report).contains("no installation fingerprint"));
//! ```

use std::fmt;

use cs_assets::install::Sha256;
use cs_assets::vfs::{ContentSession, ReadError, SessionAsset, SessionGeneration};
use cs_formats::{InterpLoadPlan, KeySpelling, LoadCommand, PlanLineKind, PlanStats, ScriptOrigin};
use cs_types::asset_id::{AssetKey, AssetKeyError, AssetVariant, SourceSpan, WorldGroup};
use cs_types::evidence::ContentHash;

/// How one dependency of a loading plan ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DependencyState {
    /// The VFS resolved the key to exactly one origin, and the immutable span
    /// of the bytes that answer it.
    Resolved {
        /// Where the bytes live. Provenance, never a path to open.
        span: SourceSpan,
    },
    /// The key is well-formed but nothing answers it, or more than one origin
    /// does. The plan fails its world; it does not substitute a default file.
    Unresolved {
        /// Stable lowercase identifier, matched from the VFS error class.
        code: &'static str,
        /// The VFS's own message, which carries the ordered attempts.
        detail: String,
    },
    /// The registration says the key is assembled from other arguments at run
    /// time, so no key exists to resolve. Counted separately from a refusal,
    /// because it is not an error: it is a dynamic lookup
    /// (`docs/contracts/SCRIPT-MISSION.md`).
    Composed {
        /// The key's spelling kind, `literal` or `composed`.
        spelling_kind: KeySpelling,
    },
    /// The stored arguments are not a valid key: a name that is not a label, a
    /// path that is absolute or holds a `..` component, bytes that are not
    /// text. Reported with the reason and never repaired.
    Invalid {
        /// Which part of the key was refused.
        part: &'static str,
        /// The refusal, as [`AssetKeyError`] renders it.
        detail: String,
    },
}

impl DependencyState {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Resolved { .. } => "resolved",
            Self::Unresolved { code, .. } => code,
            Self::Composed { .. } => "composed",
            Self::Invalid { .. } => "invalid",
        }
    }

    /// Whether this dependency is ready to be read.
    pub const fn is_resolved(&self) -> bool {
        matches!(self, Self::Resolved { .. })
    }

    /// Whether this dependency prevents its world's plan from being complete.
    ///
    /// A `Composed` key is not a failure: it is a lookup the original engine
    /// performs later, and the report says so rather than pretending the world
    /// is fully known. Completeness therefore accounts for it separately.
    pub const fn is_failure(&self) -> bool {
        matches!(self, Self::Unresolved { .. } | Self::Invalid { .. })
    }
}

/// Where one dependency was asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DependencySite {
    /// Index of the script that asked for it.
    pub script: usize,
    /// Position of the line inside that script.
    pub line: usize,
    /// Absolute offset of the line's `size` word.
    pub source_offset: u64,
    /// Absolute offset of the head token's first byte.
    pub head_offset: u64,
}

/// One asset a loading plan says a world needs, and what happened to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadingDependency {
    /// The key the command's arguments spell.
    pub key: Option<AssetKey>,
    /// Where the command was.
    pub site: DependencySite,
    /// What the VFS answered.
    pub state: DependencyState,
    /// The registration that classified the line, kept so a report can be
    /// traced back to the claim it was built on.
    pub command: LoadCommand,
    resolved: Option<SessionAsset>,
}

impl LoadingDependency {
    /// The key the command's arguments spell, or `None` when the arguments
    /// were not a valid key.
    pub fn key(&self) -> Option<&AssetKey> {
        self.key.as_ref()
    }

    /// The immutable span of the bytes that answer this dependency, when the
    /// VFS resolved it.
    pub fn span(&self) -> Option<&SourceSpan> {
        match &self.state {
            DependencyState::Resolved { span } => Some(span),
            _ => None,
        }
    }

    /// What the VFS answered.
    pub fn state(&self) -> &DependencyState {
        &self.state
    }

    /// The resolved asset stamped with the session that produced it.
    pub(crate) fn resolved(&self) -> Option<&SessionAsset> {
        self.resolved.as_ref()
    }
}

/// One line the plan could not turn into a dependency, with the world it
/// affects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadingFailure {
    /// Stable lowercase identifier. A line the plan could not classify is
    /// `unclassified` or `malformed`; a dependency whose arguments are not a
    /// key is `invalid_key`; a dependency that was looked up and not answered
    /// carries the VFS's own failure class instead: `not_found`, `ambiguous`,
    /// `unmeasured_order`, or `no_session` when no session was given to
    /// search. A caller can therefore branch on *why* a key was not resolved,
    /// and no code here is a synonym for another.
    pub code: &'static str,
    /// Where the failure is in the container.
    pub site: DependencySite,
    /// The world whose load this failure prevents, when the resolving session
    /// selected one.
    pub world: Option<WorldGroup>,
    /// The registration the line matched, when it matched one.
    pub command: Option<usize>,
    /// The reason, in words: what the registration expected, what the VFS
    /// said. Argument bytes stay out of it.
    pub detail: String,
}

impl fmt::Display for LoadingFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.world {
            Some(world) => write!(
                f,
                "{} at offset {} of line {} of script {} affects world {}: {}",
                self.code,
                self.site.source_offset,
                self.site.line,
                self.site.script,
                world,
                self.detail
            )?,
            None => write!(
                f,
                "{} at offset {} of line {} of script {}: {}",
                self.code, self.site.source_offset, self.site.line, self.site.script, self.detail
            )?,
        }
        Ok(())
    }
}

/// How one script of a loading plan ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptState {
    /// Every line is a registered loading command and none of them failed:
    /// every dependency either resolved or is a dynamic lookup the original
    /// engine resolves later.
    ///
    /// A `Ready` script is not a *complete* one when it holds a dynamic
    /// lookup; [`LoadingPlanReport::is_complete`] is the check that accounts
    /// for those, and the report's `dynamic_lookups` names them.
    Ready,
    /// The script is blocked: at least one line was unclassified or
    /// malformed, so at least one line of it is not understood at all.
    ///
    /// A script can be blocked *and* hold a dependency that did not resolve,
    /// so `failures` is carried here as well. Reporting only one of the two
    /// would hide the other from a caller that switches on this state, and
    /// both are in [`LoadingPlanReport::failures`] with their own codes.
    Blocked {
        /// How many lines are unclassified or malformed.
        lines: usize,
        /// How many of the script's dependencies did not resolve. Zero when
        /// every registered line resolved (or was a dynamic lookup).
        failures: usize,
    },
    /// Every line is classified, but at least one dependency did not resolve.
    Incomplete {
        /// How many of the script's dependencies did not resolve.
        failures: usize,
    },
}

impl ScriptState {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Blocked { .. } => "blocked",
            Self::Incomplete { .. } => "incomplete",
        }
    }

    /// Whether no line of this script failed. Dynamic lookups do not make a
    /// script unready; they keep the *plan* incomplete.
    pub const fn is_ready(self) -> bool {
        matches!(self, Self::Ready)
    }

    /// How many lines of this script are unclassified or malformed.
    pub const fn blocking_lines(self) -> usize {
        match self {
            Self::Ready | Self::Incomplete { .. } => 0,
            Self::Blocked { lines, .. } => lines,
        }
    }

    /// How many of this script's dependencies did not resolve.
    pub const fn failed_dependencies(self) -> usize {
        match self {
            Self::Ready => 0,
            Self::Blocked { failures, .. } | Self::Incomplete { failures } => failures,
        }
    }
}

/// One script's place in a resolved loading plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadingScript {
    /// Where the script is in its container.
    pub origin: ScriptOrigin,
    /// The name bytes, exactly as stored. Not an identity.
    pub name: Vec<u8>,
    /// The `timestamp` word, verbatim: metadata only, never an identity and
    /// never a cache key (non-negotiable #5).
    pub raw_timestamp: u32,
    /// SHA-256 over the script's own bytes, `script_offset..end`. This is the
    /// identity a consumer may key on, and it is a content hash.
    pub content_sha256: ContentHash,
    /// How the script ended.
    pub state: ScriptState,
    /// Indices into [`LoadingPlanReport::dependencies`], in line order.
    pub dependencies: Vec<usize>,
}

impl LoadingScript {
    /// The identity of this script: its content hash, not its name and not
    /// its timestamp.
    pub fn identity(&self) -> ContentHash {
        self.content_sha256
    }
}

/// A structural refusal of the whole report.
///
/// The plan's own per-line outcomes are **not** errors here: they are the
/// report's content, including the failures that keep a world from loading.
/// This error type covers only a plan whose own record disagrees with the
/// container it came from, which the decoder cannot produce, plus the refusals
/// of a read.
///
/// [`ReadError`] is neither `Clone` nor `Eq`, so neither is derived here.
/// Equality is implemented on the variants' own fields and, for a read, on the
/// code and the rendered message: a caller comparing two read failures compares
/// the failure, not the private state of the VFS that produced it.
#[derive(Debug)]
pub enum LoadingError {
    /// A script's extent falls outside the container the plan was decoded
    /// from, so its content hash cannot be taken over the range it names.
    Extent {
        /// The script whose extent is not inside the container.
        origin: ScriptOrigin,
        /// Length of the container the plan was decoded from.
        container_len: u64,
    },
    /// A resolved dependency is not in the report, or is not resolved, so it
    /// cannot be read.
    NotReadable {
        /// The index asked for.
        index: usize,
        /// Why.
        detail: String,
    },
    /// The report was read through a session that is not the one that resolved
    /// it. The bytes a report names belong to a closed session, so they are
    /// refused rather than re-resolved.
    ForeignSession {
        /// The session generation the report was resolved with.
        report: SessionGeneration,
        /// The generation of the session asked to read it.
        session: SessionGeneration,
    },
    /// The bytes could not be read.
    Read(ReadError),
}

impl LoadingError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Extent { .. } => "extent",
            Self::NotReadable { .. } => "not_readable",
            Self::ForeignSession { .. } => "foreign_session",
            Self::Read(_) => "read",
        }
    }
}

impl fmt::Display for LoadingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Extent {
                origin,
                container_len,
            } => write!(
                f,
                "script {} at offset {} ends at {}, past the {container_len}-byte container",
                origin.index(),
                origin.script_offset(),
                origin.end()
            ),
            Self::NotReadable { index, detail } => {
                write!(f, "dependency {index} is not readable: {detail}")
            }
            Self::ForeignSession { report, session } => write!(
                f,
                "this report was resolved by {report} and cannot be read through {session}: \
                 its bytes belong to a session that has been replaced"
            ),
            Self::Read(error) => write!(f, "{error}"),
        }
    }
}

impl PartialEq for LoadingError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::Extent {
                    origin: a,
                    container_len: a_len,
                },
                Self::Extent {
                    origin: b,
                    container_len: b_len,
                },
            ) => a == b && a_len == b_len,
            (
                Self::NotReadable {
                    index: a_index,
                    detail: a_detail,
                },
                Self::NotReadable {
                    index: b_index,
                    detail: b_detail,
                },
            ) => a_index == b_index && a_detail == b_detail,
            (
                Self::ForeignSession {
                    report: a_report,
                    session: a_session,
                },
                Self::ForeignSession {
                    report: b_report,
                    session: b_session,
                },
            ) => a_report == b_report && a_session == b_session,
            (Self::Read(a), Self::Read(b)) => a.to_string() == b.to_string(),
            _ => false,
        }
    }
}

impl std::error::Error for LoadingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read(error) => Some(error),
            _ => None,
        }
    }
}

/// What a resolved loading plan found: the world's assets, the lines that
/// block it and the counts of both.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadingPlanReport {
    /// The world the resolving session selected, when it selected one. The
    /// affected world of every failure below.
    world: Option<WorldGroup>,
    /// The installation the dependencies were resolved against, when a session
    /// was given. Provenance for the whole report.
    installation: Option<ContentHash>,
    /// The session generation the dependencies are stamped with.
    generation: Option<SessionGeneration>,
    /// SHA-256 over the whole container, so a report is tied to the exact
    /// bytes it was built from.
    container_sha256: ContentHash,
    /// Length of the container in bytes.
    container_len: u64,
    scripts: Vec<LoadingScript>,
    dependencies: Vec<LoadingDependency>,
    failures: Vec<LoadingFailure>,
    stats: PlanStats,
    dynamic_lookups: usize,
}

impl LoadingPlanReport {
    /// The world this plan was resolved for, when a context selected one.
    pub fn world(&self) -> Option<&WorldGroup> {
        self.world.as_ref()
    }

    /// The installation fingerprint the dependencies resolve against.
    pub fn installation(&self) -> Option<ContentHash> {
        self.installation
    }

    /// The session generation the resolved dependencies are stamped with.
    pub fn generation(&self) -> Option<SessionGeneration> {
        self.generation
    }

    /// SHA-256 over the container this report was built from.
    pub fn container_sha256(&self) -> ContentHash {
        self.container_sha256
    }

    /// Length of the container in bytes.
    pub fn container_len(&self) -> u64 {
        self.container_len
    }

    /// Every script, in index order.
    pub fn scripts(&self) -> &[LoadingScript] {
        &self.scripts
    }

    /// The script at `index`, or `None` when it is out of range.
    pub fn script(&self, index: usize) -> Option<&LoadingScript> {
        self.scripts.get(index)
    }

    /// Every dependency, in script and line order.
    pub fn dependencies(&self) -> &[LoadingDependency] {
        &self.dependencies
    }

    /// The dependencies of one script, in line order.
    pub fn dependencies_of(&self, script: usize) -> Vec<&LoadingDependency> {
        self.scripts
            .get(script)
            .map(|entry| {
                entry
                    .dependencies
                    .iter()
                    .filter_map(|index| self.dependencies.get(*index))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Every line that failed, in script and line order.
    pub fn failures(&self) -> &[LoadingFailure] {
        &self.failures
    }

    /// The lines of one script that failed, in line order.
    pub fn failures_of(&self, script: usize) -> Vec<&LoadingFailure> {
        self.failures
            .iter()
            .filter(|failure| failure.site.script == script)
            .collect()
    }

    /// The classifier's tally: what was read, decoded and not understood.
    pub fn stats(&self) -> PlanStats {
        self.stats
    }

    /// How many dependencies are dynamic lookups: registered commands whose
    /// key is assembled at run time and so has nothing to resolve here.
    pub fn dynamic_lookups(&self) -> usize {
        self.dynamic_lookups
    }

    /// How many dependencies resolved to exactly one origin.
    pub fn resolved_count(&self) -> usize {
        self.dependencies
            .iter()
            .filter(|dependency| dependency.state.is_resolved())
            .count()
    }

    /// Whether the plan is complete: no line failed and every dependency
    /// resolved.
    ///
    /// A dynamic lookup makes a plan incomplete on purpose, because
    /// [`DependencyState::Composed`] is not
    /// [`DependencyState::is_resolved`]: the original engine assembles that key
    /// later, so a plan that reported itself complete while such a lookup was
    /// outstanding would claim more than is known. The rule is stated once, on
    /// the states, rather than restated as a separate count here.
    pub fn is_complete(&self) -> bool {
        self.failures.is_empty()
            && self
                .dependencies
                .iter()
                .all(|dependency| dependency.state.is_resolved())
    }

    /// A one-line human summary naming the counts, for a CLI or a log.
    pub fn describe(&self) -> String {
        let mut out = format!(
            "{} script(s), {} line(s): {} loading command(s), {} unclassified, {} malformed, \
             {} unsupported, {} behavior, {} resolved, {} dynamic",
            self.scripts.len(),
            self.stats.lines,
            self.stats.loading_commands,
            self.stats.unclassified_commands,
            self.stats.malformed_commands,
            self.stats.unsupported_commands,
            self.stats.behavior_commands,
            self.resolved_count(),
            self.dynamic_lookups,
        );
        if self.installation.is_none() {
            out.push_str("; no installation fingerprint, so nothing was resolved");
        }
        match &self.world {
            Some(world) => out.push_str(&format!("; world {world}")),
            None => out.push_str("; no world selected, so every failure is world-less"),
        }
        out
    }

    /// Reads the bytes of the dependency at `index` through `session`.
    ///
    /// The session must be the one that resolved this report: a report's
    /// `SourceSpan` describes bytes owned by a session that may since have been
    /// closed and replaced, so a foreign session is refused
    /// ([`LoadingError::ForeignSession`]) rather than allowed to re-resolve
    /// something the report never saw. The read itself is digest-checked by
    /// the VFS.
    pub fn read_dependency(
        &self,
        session: &ContentSession,
        index: usize,
    ) -> Result<Vec<u8>, LoadingError> {
        let Some(dependency) = self.dependencies.get(index) else {
            return Err(LoadingError::NotReadable {
                index,
                detail: format!("the report holds {} dependencies", self.dependencies.len()),
            });
        };
        let Some(expected) = self.generation else {
            return Err(LoadingError::NotReadable {
                index,
                detail: "the report was built without a session, so nothing resolved".to_owned(),
            });
        };
        if session.generation() != expected {
            return Err(LoadingError::ForeignSession {
                report: expected,
                session: session.generation(),
            });
        }
        let Some(asset) = dependency.resolved() else {
            return Err(LoadingError::NotReadable {
                index,
                detail: format!("the dependency is {}", dependency.state.code()),
            });
        };
        session.read_all(asset).map_err(LoadingError::Read)
    }
}

/// Resolves a loading plan against a content session.
///
/// `session` is optional and its absence is *recorded*, not fatal: with no
/// session there is no installation fingerprint, no world and no VFS, so every
/// registered command becomes an `Unresolved` dependency whose detail says
/// exactly that, and no line is reported as loaded. A caller that wants
/// resolution passes the session; a caller that only wants the classification
/// (a researcher reading a container, for instance) passes `None` and gets the
/// same classification with nothing resolved.
///
/// The report is built in one pass and owns everything it needs: it holds no
/// session reference, no file handle and no borrow of the container, so it
/// outlives both. A caller that retries builds a second report.
///
/// # Errors
///
/// Only [`LoadingError::Extent`], when a script's recorded extent falls outside
/// the container the plan was decoded from — a disagreement the decoder cannot
/// produce, refused rather than hashed over a range that does not exist.
pub fn resolve_loading_plan<'a>(
    session: Option<&ContentSession>,
    decoded: &cs_formats::DecodedInterp<'a>,
    plan: &InterpLoadPlan<'a>,
) -> Result<LoadingPlanReport, LoadingError> {
    let bytes = decoded.bytes();
    let container_len = bytes.len() as u64;
    let world = session.and_then(|session| session.context().world_group.clone());
    let installation = session.map(|session| session.context().installation);
    let generation = session.map(ContentSession::generation);

    let mut dependencies = Vec::new();
    let mut failures = Vec::new();
    let mut dynamic_lookups = 0usize;
    let mut scripts = Vec::with_capacity(plan.scripts().len());

    for planned in plan.scripts() {
        let origin = planned.origin();
        let end = origin.end();
        if u64::from(origin.script_offset()) > container_len || end > container_len {
            return Err(LoadingError::Extent {
                origin,
                container_len,
            });
        }
        // Identity is a hash of the script's own bytes, and of nothing else.
        // The name is not hashed (two scripts may share it) and the timestamp
        // is not hashed (it is metadata, non-negotiable #5).
        let from = origin.script_offset() as usize;
        let content_sha256 = sha256(&bytes[from..end as usize]);

        let mut indices = Vec::new();
        let mut blocking = 0usize;
        let mut failing = 0usize;
        for line in planned.lines() {
            let site = DependencySite {
                script: origin.index(),
                line: line.position(),
                source_offset: line.source_offset(),
                head_offset: line.head_offset(),
            };
            match *line.kind() {
                PlanLineKind::Loading { command, key } => {
                    let registration = plan
                        .command(command)
                        .cloned()
                        .expect("a classified line names a registration of the plan");
                    let resolved = resolve_line(session, &registration, &key, &mut dynamic_lookups);
                    let failure = match &resolved.state {
                        DependencyState::Unresolved { code, detail } => Some(LoadingFailure {
                            code,
                            site,
                            world: world.clone(),
                            command: Some(command),
                            detail: detail.clone(),
                        }),
                        DependencyState::Invalid { part, detail } => Some(LoadingFailure {
                            code: "invalid_key",
                            site,
                            world: world.clone(),
                            command: Some(command),
                            detail: format!(
                                "the {part} argument is not a valid key part: {detail}"
                            ),
                        }),
                        _ => None,
                    };
                    if let Some(failure) = failure {
                        failing += 1;
                        failures.push(failure);
                    }
                    indices.push(dependencies.len());
                    dependencies.push(LoadingDependency {
                        key: resolved.key,
                        site,
                        state: resolved.state,
                        command: registration,
                        resolved: resolved.asset,
                    });
                }
                PlanLineKind::Malformed { command, reason } => {
                    blocking += 1;
                    let registration = plan.command(command);
                    failures.push(LoadingFailure {
                        code: "malformed",
                        site,
                        world: world.clone(),
                        command: Some(command),
                        detail: format!(
                            "the registered argument domain does not match the line: {reason} \
                             (command status {}, source {})",
                            registration
                                .map(|rule| rule.status.label())
                                .unwrap_or("unknown"),
                            registration
                                .map(|rule| rule.source.as_str())
                                .unwrap_or("unknown")
                        ),
                    });
                }
                PlanLineKind::Unsupported { opcode } => {
                    blocking += 1;
                    let classification = plan.opcode(opcode);
                    failures.push(LoadingFailure {
                        code: "unsupported_command",
                        site,
                        world: world.clone(),
                        command: None,
                        detail: format!(
                            "the command is classified as resource-loading but has no supported \
                             key domain here (classification {}, source {})",
                            classification
                                .map(|rule| rule.status.label())
                                .unwrap_or("unknown"),
                            classification
                                .map(|rule| rule.source.as_str())
                                .unwrap_or("unknown")
                        ),
                    });
                }
                PlanLineKind::Behavior { .. } => {
                    // Classified as not a resource load: it contributes no
                    // dependency and does not fail the script. Interpreting
                    // what it does is the mission language's business (F13).
                }
                PlanLineKind::Unclassified => {
                    blocking += 1;
                    failures.push(LoadingFailure {
                        code: "unclassified",
                        site,
                        world: world.clone(),
                        command: None,
                        detail: "no registered loading command claims this line's head token"
                            .to_owned(),
                    });
                }
            }
        }

        let state = match (blocking, failing) {
            (0, 0) => ScriptState::Ready,
            (0, failing) => ScriptState::Incomplete { failures: failing },
            // Both kinds of problem are reported, not one in place of the
            // other: a caller that switches on the state must not have to
            // re-walk the failures to discover the lines it could not read.
            (lines, failures) => ScriptState::Blocked { lines, failures },
        };
        scripts.push(LoadingScript {
            origin,
            name: planned.name().to_vec(),
            raw_timestamp: planned.raw_timestamp(),
            content_sha256,
            state,
            dependencies: indices,
        });
    }

    Ok(LoadingPlanReport {
        world,
        installation,
        generation,
        container_sha256: sha256(bytes),
        container_len,
        scripts,
        dependencies,
        failures,
        stats: plan.stats(),
        dynamic_lookups,
    })
}

/// One classified line, resolved: the key it spells, what the VFS answered and
/// the session-stamped asset that answered it.
struct ResolvedLine {
    key: Option<AssetKey>,
    state: DependencyState,
    asset: Option<SessionAsset>,
}

/// Resolves one classified line against the session, if there is one.
///
/// The order of the checks is the order of what a caller could get wrong: a
/// registration that says the key is assembled at run time has nothing to
/// resolve (a dynamic lookup, counted and reported, not a failure); stored
/// bytes that are not a valid key are refused with the part that failed; and
/// only a well-formed literal key is looked up in the installation.
fn resolve_line(
    session: Option<&ContentSession>,
    registration: &LoadCommand,
    key: &cs_formats::KeyTokens<'_>,
    dynamic_lookups: &mut usize,
) -> ResolvedLine {
    if registration.spelling_kind == KeySpelling::Composed {
        *dynamic_lookups += 1;
        return ResolvedLine {
            key: None,
            state: DependencyState::Composed {
                spelling_kind: registration.spelling_kind,
            },
            asset: None,
        };
    }
    let asset_key = match build_key(key) {
        Ok(key) => key,
        Err((part, detail)) => {
            return ResolvedLine {
                key: None,
                state: DependencyState::Invalid { part, detail },
                asset: None,
            };
        }
    };
    let Some(session) = session else {
        return ResolvedLine {
            key: Some(asset_key),
            state: DependencyState::Unresolved {
                code: "no_session",
                detail: "no content session was given, so no installation was searched".to_owned(),
            },
            asset: None,
        };
    };
    match session.resolve(&asset_key) {
        Ok(asset) => ResolvedLine {
            key: Some(asset_key),
            state: DependencyState::Resolved {
                span: asset.resolved().span.clone(),
            },
            asset: Some(asset),
        },
        Err(error) => ResolvedLine {
            key: Some(asset_key),
            state: DependencyState::Unresolved {
                code: resolve_error_code(&error),
                detail: error.to_string(),
            },
            asset: None,
        },
    }
}

/// The stable identifier of a VFS refusal, matched on the variant rather than
/// on the message so a caller can branch on the failure class.
fn resolve_error_code(error: &cs_assets::vfs::ResolveError) -> &'static str {
    use cs_assets::vfs::ResolveError;
    match error {
        ResolveError::NotFound { .. } => "not_found",
        ResolveError::Ambiguous { .. } => "ambiguous",
        ResolveError::UnmeasuredOrder { .. } => "unmeasured_order",
    }
}

/// Builds an [`AssetKey`] from the three stored argument tokens, or the part
/// that was refused and why.
///
/// Bytes are never decoded leniently: a namespace or a variant that is not text
/// cannot be a label ([`AssetKey::from_spelling`] validates both), and a path
/// that is absolute or holds a `..` component is refused, because where the
/// original engine resolved such a path from is unmeasured (F07-D). A `%NAME%`
/// spelling is refused the same way — the variable syntax of the loading
/// language is not established, so it is not treated as a path here.
fn build_key(key: &cs_formats::KeyTokens<'_>) -> Result<AssetKey, (&'static str, String)> {
    // Each part is decoded into an owned `String` first: the token bytes live
    // in the container, and `AssetKey` keeps its own copies anyway, so nothing
    // is borrowed across the conversion and no byte is reinterpreted.
    fn part(
        name: &'static str,
        token: cs_formats::InterpToken<'_>,
    ) -> Result<String, (&'static str, String)> {
        std::str::from_utf8(token.bytes())
            .map(str::to_owned)
            .map_err(|error| (name, format!("the bytes are not text ({error})")))
    }
    let namespace = part("namespace", key.namespace())?;
    let path = part("path", key.path())?;
    let variant = match key.variant() {
        Some(token) => part("variant", token)?,
        // A registration that names no variant position resolves under the
        // engine's own neutral variant label, taken from `cs_types` rather
        // than spelled again here: it is authored engine design, not a value
        // observed in an original container.
        None => AssetVariant::default().as_str().to_owned(),
    };
    AssetKey::from_spelling(&namespace, &path, &variant).map_err(|error: AssetKeyError| {
        let part = match &error {
            AssetKeyError::Namespace(_) => "namespace",
            AssetKeyError::Path(_) => "path",
            AssetKeyError::Variant(_) => "variant",
        };
        (part, error.to_string())
    })
}

/// The bytes' SHA-256 as a [`ContentHash`].
fn sha256(bytes: &[u8]) -> ContentHash {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize()
}

#[cfg(test)]
mod tests {
    //! F07-C unit tests for the adapter itself: the teardown, retry and error
    //! propagation that a CLI report cannot show. Every tree and container here
    //! is newly authored synthetic bytes below the system temporary directory;
    //! no original game data, no `CS_GAME_DIR` access.

    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use cs_assets::install;
    use cs_assets::vfs::{ContentSession, SessionBuilder, SessionError};
    use cs_formats::{
        ClassifiedOpcode, INDEX_ENTRY_BYTES, KeyArguments, KeySpelling, LoadCommand,
        LoadCommandTable, NAME_FIELD_BYTES, OpcodeClass, OpcodeClassTable, ParseContext,
        decode_interp, plan_interp_loading, plan_interp_loading_classified,
    };
    use cs_types::asset_id::{ResolveContext, WorldGroup};
    use cs_types::evidence::ClaimStatus;

    use super::*;

    static NEXT: AtomicU64 = AtomicU64::new(0);

    /// A disposable directory, removed on drop.
    struct Temp(PathBuf);

    impl Temp {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "cs-f07-c-loading-{label}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).expect("temp dir is created");
            Self(root)
        }

        fn write(&self, spelling: &str, bytes: &[u8]) {
            let path = self.0.join(spelling);
            fs::create_dir_all(path.parent().expect("has a parent")).expect("dirs");
            fs::write(path, bytes).expect("bytes are written");
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

    /// The first byte a one-entry container's script may occupy: past the
    /// header and its single index entry.
    fn script_start() -> u32 {
        (cs_formats::INTERP_HEADER_BYTES + INDEX_ENTRY_BYTES) as u32
    }

    /// One authored container: one script whose lines are the given stored
    /// argument blocks. The declared count is the number of `0x00` delimiters,
    /// which is the rule the decoder enforces.
    fn container(name: &[u8], lines: &[&[u8]]) -> Vec<u8> {
        let mut body = Vec::new();
        for data in lines {
            body.extend_from_slice(&(data.len() as u32).to_le_bytes());
            let count = data.iter().filter(|byte| **byte == 0).count();
            body.extend_from_slice(&(count as u32).to_le_bytes());
            body.extend_from_slice(data);
        }
        body.extend_from_slice(&0u32.to_le_bytes());
        let mut bytes = Vec::new();
        for word in [0x0897_1119u32, 7, 1] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        let mut field = [0u8; NAME_FIELD_BYTES];
        field[..name.len()].copy_from_slice(name);
        bytes.extend_from_slice(&field);
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&script_start().to_le_bytes());
        assert_eq!(bytes.len(), script_start() as usize);
        bytes.extend_from_slice(&body);
        bytes
    }

    /// One registration: a synthetic rule that exercises the adapter, never a
    /// claim about an original command.
    fn registration(spelling: &[u8], kind: KeySpelling) -> LoadCommand {
        LoadCommand {
            spelling: spelling.to_vec(),
            arguments: KeyArguments {
                namespace: 1,
                path: 2,
                variant: None,
            },
            spelling_kind: kind,
            status: ClaimStatus::Designed,
            source: "synthetic test table: exercises the adapter".to_owned(),
        }
    }

    /// A one-world installation with one file, mounted in a real session.
    fn session(tree: &Temp, world: &str) -> Result<ContentSession, SessionError> {
        let found = install::discover(tree.path()).expect("the fixture installation discovers");
        let context = ResolveContext::new(install::fingerprint(&found.manifest))
            .with_world_group(WorldGroup::new(world).expect("the fixture world is valid"));
        let mut builder = SessionBuilder::new(context);
        builder.mount_installation(tree.path(), &found.diagnosis)?;
        Ok(builder.open())
    }

    /// The teardown and retry contract: a report reads through the session
    /// that resolved it, is refused by any other, and a second report built
    /// after the first session closed resolves again from scratch.
    ///
    /// The failure this catches is a report that re-resolves lazily: a plan
    /// whose dependencies were stamped with a session that no longer exists
    /// would otherwise hand out bytes from a world that has been unloaded.
    #[test]
    fn accept_f07_c_report_reads_only_through_the_session_that_resolved_it() {
        let tree = Temp::new("read");
        tree.write("ZBD/c1/plane.flt", b"world one plane");
        let bytes = container(b"load", &[b"loadmesh\0world\0plane.flt\0"]);

        let mut table = LoadCommandTable::new();
        table
            .insert(registration(b"loadmesh", KeySpelling::Literal))
            .expect("the rule registers");
        let decoded = decode_interp(&mut ParseContext::with_defaults("load.interp"), &bytes)
            .expect("the container validates");
        let plan = plan_interp_loading(&decoded, &table);

        let first = session(&tree, "zbd/c1").expect("the session opens");
        let first_generation = first.generation();
        let report = resolve_loading_plan(Some(&first), &decoded, &plan).expect("the plan builds");
        assert!(report.is_complete(), "{}", report.describe());
        assert_eq!(report.resolved_count(), 1);
        assert_eq!(report.generation(), Some(first_generation));
        assert_eq!(
            report
                .world()
                .map(WorldGroup::as_relative)
                .map(|p| p.as_str()),
            Some("zbd/c1")
        );

        // The bytes come back through the same session, digest-checked.
        let read = report
            .read_dependency(&first, 0)
            .expect("the same session reads");
        assert_eq!(read, b"world one plane");
        // An index that is not a dependency is refused, not read as one.
        assert!(matches!(
            report.read_dependency(&first, 1),
            Err(LoadingError::NotReadable { index: 1, .. })
        ));
        assert_eq!(
            report.read_dependency(&first, 1).unwrap_err().code(),
            "not_readable"
        );

        // A report is refused by a session that is not the one that resolved
        // it, even though the installation and the key are identical. Checked
        // while both sessions exist, so the identity under test is exact.
        let second = session(&tree, "zbd/c1").expect("a second session opens");
        assert_ne!(second.generation(), first_generation);
        let error = report
            .read_dependency(&second, 0)
            .expect_err("a foreign session");
        assert_eq!(error.code(), "foreign_session");
        assert!(
            matches!(
                error,
                LoadingError::ForeignSession {
                    report,
                    session: _
                } if report == first_generation
            ),
            "{error}"
        );
        assert!(error.to_string().contains("belong to a session"), "{error}");

        // The teardown: closing a session releases its mounts and consumes it,
        // so the report's own session is gone and cannot be read through.
        let teardown = first.close();
        assert_eq!(teardown.generation, first_generation);
        assert_eq!(teardown.released.len(), 2, "install plus one world");

        // A retry builds a second report from nothing and resolves again: the
        // refusal above is about identity, not about the installation.
        let retry = resolve_loading_plan(Some(&second), &decoded, &plan).expect("the retry builds");
        assert_eq!(retry.generation(), Some(second.generation()));
        assert!(
            retry.dependencies()[0]
                .span()
                .map(|span| span.install_sha256())
                .is_some(),
            "the retry names the installation it resolved against"
        );
        assert_eq!(
            retry.read_dependency(&second, 0).expect("the retry reads"),
            b"world one plane"
        );
        // The first report is a plain value and outlives its session, but its
        // bytes went with the session: reading it now still names the session
        // it needs rather than re-resolving against the new one.
        let error = report
            .read_dependency(&second, 0)
            .expect_err("still a foreign session");
        assert_eq!(error.code(), "foreign_session");
        second.close();
    }

    /// A key no mount holds is a failure with its VFS code, and the world's
    /// own file answers the other line — never a default file for the missing
    /// one.
    #[test]
    fn accept_f07_c_missing_key_is_a_failure_not_a_default_file() {
        let tree = Temp::new("missing");
        tree.write("ZBD/c1/plane.flt", b"world one plane");
        let bytes = container(
            b"load",
            &[
                b"loadmesh\0world\0plane.flt\0",
                b"loadmesh\0world\0absent.flt\0",
            ],
        );
        let mut table = LoadCommandTable::new();
        table
            .insert(registration(b"loadmesh", KeySpelling::Literal))
            .expect("the rule registers");
        let decoded = decode_interp(&mut ParseContext::with_defaults("load.interp"), &bytes)
            .expect("the container validates");
        let plan = plan_interp_loading(&decoded, &table);
        let open = session(&tree, "zbd/c1").expect("the session opens");
        let report = resolve_loading_plan(Some(&open), &decoded, &plan).expect("the plan builds");

        assert!(!report.is_complete());
        assert_eq!(report.resolved_count(), 1);
        assert_eq!(
            report.dependencies().len(),
            2,
            "both lines became dependencies"
        );
        assert_eq!(report.dependencies()[0].state().code(), "resolved");
        assert_eq!(report.dependencies()[1].state().code(), "not_found");
        assert!(report.dependencies()[1].span().is_none());
        assert!(
            report.dependencies()[1].key().is_some(),
            "the key itself is valid"
        );

        // The failure names the offset, the world and the reason.
        assert_eq!(report.failures().len(), 1);
        let failure = &report.failures()[0];
        assert_eq!(failure.code, "not_found");
        assert_eq!(failure.site.line, 1);
        // The second line sits just past the first one's stored data, and the
        // head token just past the line header, so the offsets are derived
        // from the bytes the test wrote rather than counted by hand.
        let first = b"loadmesh\0world\0plane.flt\0";
        let second_start = script_start() as usize + 8 + first.len();
        assert_eq!(failure.site.source_offset, second_start as u64);
        assert_eq!(failure.site.head_offset, (second_start + 8) as u64);
        assert_eq!(
            &bytes[second_start + 8..second_start + 8 + 8],
            b"loadmesh",
            "the offset points at that line's own head token"
        );
        assert_eq!(
            failure.world.as_ref().map(|w| w.logical_key()),
            Some("zbd/c1".to_owned())
        );
        assert!(failure.detail.contains("attempts"), "{}", failure.detail);
        let rendered = failure.to_string();
        assert!(rendered.contains("affects world zbd/c1"), "{rendered}");
        assert!(
            rendered.contains(&format!("not_found at offset {second_start}")),
            "{rendered}"
        );
        assert_eq!(report.failures_of(0).len(), 1);
        assert!(!report.script(0).expect("one script").state.is_ready());
        assert_eq!(
            report.script(0).expect("one script").state.code(),
            "incomplete"
        );
        // A dependency that did not resolve cannot be read, and says why.
        assert!(matches!(
            report.read_dependency(&open, 1),
            Err(LoadingError::NotReadable { index: 1, .. })
        ));
        open.close();
    }

    /// A script can be blocked *and* hold a dependency that did not resolve.
    /// Both counts have to survive into its state, because a caller that
    /// switches on the state is how the two problems get told apart: a report
    /// that named only the unresolved dependency would leave the unclassified
    /// line visible only to a reader who re-walks every failure.
    #[test]
    fn accept_f07_c_a_blocked_script_still_counts_its_unresolved_dependencies() {
        let tree = Temp::new("mixed");
        tree.write("ZBD/c1/plane.flt", b"world one plane");
        // Line 0 is a registered command whose key nothing holds; line 1 is a
        // command no registration claims.
        let bytes = container(b"load", &[b"loadmesh\0world\0absent.flt\0", b"Quit\0"]);
        let mut table = LoadCommandTable::new();
        table
            .insert(registration(b"loadmesh", KeySpelling::Literal))
            .expect("the rule registers");
        let decoded = decode_interp(&mut ParseContext::with_defaults("load.interp"), &bytes)
            .expect("the container validates");
        let plan = plan_interp_loading(&decoded, &table);
        let open = session(&tree, "zbd/c1").expect("the session opens");
        let report = resolve_loading_plan(Some(&open), &decoded, &plan).expect("the plan builds");

        let state = report.script(0).expect("one script").state;
        assert_eq!(
            state,
            ScriptState::Blocked {
                lines: 1,
                failures: 1
            },
            "the unclassified line and the unresolved key are both reported"
        );
        assert_eq!(state.code(), "blocked");
        assert!(!state.is_ready());
        assert_eq!(state.blocking_lines(), 1);
        assert_eq!(state.failed_dependencies(), 1);
        // And both problems really are in the report, with their own codes.
        let codes: Vec<&str> = report
            .failures()
            .iter()
            .map(|failure| failure.code)
            .collect();
        assert_eq!(codes, ["not_found", "unclassified"]);
        assert!(!report.is_complete());
        open.close();
    }

    /// Arguments that are not a valid key are refused with the part that
    /// failed, and a `composed` registration is a dynamic lookup rather than
    /// either outcome. Neither is repaired.
    #[test]
    fn accept_f07_c_invalid_keys_and_composed_keys_are_not_repaired() {
        let tree = Temp::new("invalid");
        tree.write("ZBD/c1/plane.flt", b"world one plane");
        let bytes = container(
            b"load",
            &[
                // A `..` component: where the original engine resolved it from
                // is unmeasured, so the key is refused, not normalised.
                b"loadmesh\0world\0..\\data\\plane.flt\0",
                // A namespace that is not a label.
                b"loadmesh\0WORLD NS\0plane.flt\0",
                // Bytes that are not text.
                b"loadmesh\0world\0\xff\xfe.flt\0",
                // A registration that says the key is assembled at run time.
                b"compose\0world\0%ZBD_DIR%/plane.flt\0",
            ],
        );
        let mut table = LoadCommandTable::new();
        table
            .insert(registration(b"loadmesh", KeySpelling::Literal))
            .expect("the rule registers");
        table
            .insert(registration(b"compose", KeySpelling::Composed))
            .expect("the rule registers");
        let decoded = decode_interp(&mut ParseContext::with_defaults("load.interp"), &bytes)
            .expect("the container validates");
        let plan = plan_interp_loading(&decoded, &table);
        let open = session(&tree, "zbd/c1").expect("the session opens");
        let report = resolve_loading_plan(Some(&open), &decoded, &plan).expect("the plan builds");

        assert_eq!(report.dependencies().len(), 4);
        assert_eq!(report.dependencies()[0].state().code(), "invalid");
        assert_eq!(report.resolved_count(), 0);
        assert_eq!(report.dynamic_lookups(), 1);
        assert_eq!(report.dependencies()[3].state().code(), "composed");
        assert!(report.dependencies()[3].key().is_none());
        assert!(report.dependencies()[3].span().is_none());
        // Three invalid keys, each naming the part that was refused, and no
        // dynamic lookup among them.
        assert_eq!(report.failures().len(), 3);
        let parts: Vec<&str> = report
            .dependencies()
            .iter()
            .filter_map(|dependency| match dependency.state() {
                DependencyState::Invalid { part, .. } => Some(*part),
                _ => None,
            })
            .collect();
        assert_eq!(parts, ["path", "namespace", "path"]);
        assert!(
            report.failures()[0].detail.contains("`..`"),
            "{}",
            report.failures()[0].detail
        );
        assert!(
            report.failures()[1].detail.contains("namespace"),
            "{}",
            report.failures()[1].detail
        );
        assert!(
            report.failures()[2].detail.contains("not text"),
            "{}",
            report.failures()[2].detail
        );
        // All four lines were registered commands, so nothing was unclassified
        // and nothing was malformed: the lines failed on their arguments, not
        // on their classification.
        assert_eq!(report.stats().loading_commands, 4);
        assert_eq!(report.stats().unclassified_commands, 0);
        assert_eq!(report.stats().malformed_commands, 0);
        assert!(
            !report.is_complete(),
            "a dynamic lookup keeps the plan incomplete"
        );
        // The rule is on the state, not on a separate count: a `Composed`
        // dependency is simply not a resolved one, so removing the
        // `dynamic_lookups` field from the completeness rule changes nothing.
        assert!(!report.dependencies()[3].state().is_resolved());
        assert!(!report.dependencies()[3].state().is_failure());
        let summary = report.describe();
        assert!(summary.contains("1 dynamic"), "{summary}");
        assert!(summary.contains("0 resolved"), "{summary}");
        open.close();
    }

    /// With no session at all the plan is still built and every registered
    /// command is reported unresolved for that reason, never as loaded.
    #[test]
    fn accept_f07_c_without_a_session_nothing_resolves_and_the_reason_is_recorded() {
        let bytes = container(b"load", &[b"loadmesh\0world\0plane.flt\0"]);
        let mut table = LoadCommandTable::new();
        table
            .insert(registration(b"loadmesh", KeySpelling::Literal))
            .expect("the rule registers");
        let decoded = decode_interp(&mut ParseContext::with_defaults("load.interp"), &bytes)
            .expect("the container validates");
        let plan = plan_interp_loading(&decoded, &table);
        let report = resolve_loading_plan(None, &decoded, &plan).expect("the plan builds");

        assert!(!report.is_complete());
        assert_eq!(report.resolved_count(), 0);
        assert_eq!(report.installation(), None);
        assert_eq!(report.generation(), None);
        assert!(report.world().is_none());
        assert_eq!(report.dependencies().len(), 1);
        assert_eq!(report.dependencies()[0].state().code(), "no_session");
        // The key is still built and reported; only the lookup is missing.
        assert!(report.dependencies()[0].key().is_some());
        assert_eq!(report.failures().len(), 1);
        assert_eq!(report.failures()[0].code, "no_session");
        assert!(report.failures()[0].world.is_none());
        assert!(report.describe().contains("no installation fingerprint"));
        assert!(report.describe().contains("no world selected"));
        // A report built without a session has no resolved span and no
        // generation, so a consumer sees that there is nothing to read rather
        // than an empty success.
        assert!(report.dependencies()[0].span().is_none());
    }

    /// The report is tied to the exact bytes it was built from: the container
    /// hash and each script's own hash change with the content, and the
    /// `timestamp` word is never part of any of them.
    #[test]
    fn accept_f07_c_identity_is_a_content_hash_not_a_name_or_a_timestamp() {
        let tree = Temp::new("identity");
        tree.write("ZBD/c1/plane.flt", b"world one plane");
        let first = container(b"twin", &[b"loadmesh\0world\0plane.flt\0"]);
        // The same container with the timestamp word changed. The plan's
        // identity must not move, because the timestamp is metadata.
        let mut moved = first.clone();
        let stamp = 12 + NAME_FIELD_BYTES;
        moved[stamp..stamp + 4].copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
        assert_ne!(first, moved, "the two containers really differ");

        let table = LoadCommandTable::new();
        let mut registered = table.clone();
        registered
            .insert(registration(b"loadmesh", KeySpelling::Literal))
            .expect("the rule registers");

        let hash_of = |bytes: &[u8]| {
            let decoded = decode_interp(&mut ParseContext::with_defaults("twin.interp"), bytes)
                .expect("valid");
            let plan = plan_interp_loading(&decoded, &registered);
            let report = resolve_loading_plan(None, &decoded, &plan)
                .expect("the plan builds without a session");
            (
                report.container_sha256(),
                report.script(0).expect("one script").identity(),
                report.script(0).expect("one script").raw_timestamp,
            )
        };
        let (container_a, script_a, stamp_a) = hash_of(&first);
        let (container_b, script_b, stamp_b) = hash_of(&moved);
        assert_ne!(stamp_a, stamp_b, "the timestamps differ");
        // The container hash covers the index table, so the timestamp word is
        // inside it and it does move: these are two different files and the
        // report says so.
        assert_ne!(container_a, container_b, "the index entry changed");
        // The script hash covers only the script's own bytes, and the
        // timestamp is not among them, so a script's identity does not move
        // when only its metadata does. This is the rule that matters: no
        // consumer may key a cache on a timestamp (non-negotiable #5).
        assert_eq!(script_a, script_b, "the script's own bytes are identical");
        // Both are real content hashes of the stored bytes.
        assert_eq!(container_a, install::sha256(&first));
        assert_eq!(stamp_a, 0);
        assert_eq!(stamp_b, 0xDEAD_BEEF);

        // A different body under the same name is a different identity.
        let other = container(b"twin", &[b"loadmesh\0world\0absent.flt\0"]);
        let (container_c, script_c, _) = hash_of(&other);
        assert_ne!(container_a, container_c);
        assert_ne!(script_a, script_c);
    }

    /// Two scripts with equal names, equal timestamps and different bodies
    /// keep distinct origins *and* distinct content hashes, so neither the
    /// plan nor a consumer that keys on the hash can collapse them. This is
    /// AC03 through the adapter.
    #[test]
    fn accept_f07_c_equal_names_keep_distinct_origins_and_hashes() {
        let tree = Temp::new("twins");
        tree.write("ZBD/c1/plane.flt", b"world one plane");

        // Two scripts, same name, same timestamp, one body each.
        let bodies: [&[u8]; 2] = [
            b"loadmesh\0world\0plane.flt\0",
            b"loadmesh\0world\0other.flt\0",
        ];
        let mut bodies_bytes: Vec<Vec<u8>> = Vec::new();
        for data in bodies {
            let mut body = Vec::new();
            body.extend_from_slice(&(data.len() as u32).to_le_bytes());
            body.extend_from_slice(&3u32.to_le_bytes());
            body.extend_from_slice(data);
            body.extend_from_slice(&0u32.to_le_bytes());
            bodies_bytes.push(body);
        }
        let start = 12 + 2 * INDEX_ENTRY_BYTES;
        let mut bytes = Vec::new();
        for word in [0x0897_1119u32, 7, 2] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        for position in 0..bodies_bytes.len() {
            let mut field = [0u8; NAME_FIELD_BYTES];
            field[..4].copy_from_slice(b"twin");
            bytes.extend_from_slice(&field);
            bytes.extend_from_slice(&0x1234_5678u32.to_le_bytes());
            let offset = start + bodies_bytes[..position].iter().map(Vec::len).sum::<usize>();
            bytes.extend_from_slice(&(offset as u32).to_le_bytes());
        }
        assert_eq!(bytes.len(), start);
        for body in &bodies_bytes {
            bytes.extend_from_slice(body);
        }

        let mut table = LoadCommandTable::new();
        table
            .insert(registration(b"loadmesh", KeySpelling::Literal))
            .expect("the rule registers");
        let decoded = decode_interp(&mut ParseContext::with_defaults("twin.interp"), &bytes)
            .expect("the container validates");
        let plan = plan_interp_loading(&decoded, &table);
        let open = session(&tree, "zbd/c1").expect("the session opens");
        let report = resolve_loading_plan(Some(&open), &decoded, &plan).expect("the plan builds");

        assert_eq!(report.scripts().len(), 2);
        let [a, b] = report.scripts() else {
            panic!("two scripts expected")
        };
        assert_eq!(a.name, b.name, "the names are equal");
        assert_eq!(a.raw_timestamp, 0x1234_5678);
        assert_eq!(b.raw_timestamp, 0x1234_5678);
        assert_ne!(a.origin, b.origin, "the origins are distinct");
        assert_eq!((a.origin.index(), b.origin.index()), (0, 1));
        assert_eq!(
            (a.origin.entry_offset(), b.origin.entry_offset()),
            (12, 12 + INDEX_ENTRY_BYTES as u64)
        );
        assert_ne!(a.content_sha256, b.content_sha256, "the bodies differ");
        assert_ne!(a.identity(), b.identity());
        // The first script's key resolves; the second's does not exist, so the
        // two are distinguishable in the report as well as in the hashes.
        assert_eq!(report.dependencies_of(0).len(), 1);
        assert_eq!(report.dependencies_of(1).len(), 1);
        assert_eq!(report.dependencies_of(0)[0].state().code(), "resolved");
        assert_eq!(report.dependencies_of(1)[0].state().code(), "not_found");
        assert!(!report.is_complete());
        open.close();
    }

    /// A report is refused with [`LoadingError::Extent`] when the plan's
    /// recorded script extent falls outside the container its bytes come from,
    /// rather than hashing over a range that does not exist.
    ///
    /// The decoder cannot produce such a plan — that is why this pairs a plan
    /// with a *different*, shorter container — but the adapter checks anyway,
    /// because a hash over a truncated range would be a plausible-looking wrong
    /// identity rather than a loud failure.
    #[test]
    fn accept_f07_c_a_plan_against_foreign_bytes_is_refused() {
        let long = container(b"load", &[b"loadmesh\0world\0plane.flt\0"]);
        let short = container(b"load", &[b"a\0"]);
        assert!(short.len() < long.len());
        let mut table = LoadCommandTable::new();
        table
            .insert(registration(b"loadmesh", KeySpelling::Literal))
            .expect("the rule registers");

        // Each container's own plan builds, and the hashes cover exactly the
        // stored bytes: the whole container, and one script's own extent.
        let decoded_long = decode_interp(&mut ParseContext::with_defaults("load.interp"), &long)
            .expect("the container validates");
        let plan_long = plan_interp_loading(&decoded_long, &table);
        let report_long = resolve_loading_plan(None, &decoded_long, &plan_long)
            .expect("a plan of its own container builds");
        assert_eq!(report_long.container_sha256(), install::sha256(&long));
        let origin = report_long.scripts()[0].origin;
        assert_eq!(
            report_long.scripts()[0].content_sha256,
            install::sha256(&long[origin.script_offset() as usize..origin.end() as usize])
        );

        let decoded_short = decode_interp(&mut ParseContext::with_defaults("other.interp"), &short)
            .expect("the shorter container validates");
        let plan_short = plan_interp_loading(&decoded_short, &table);
        let report_short = resolve_loading_plan(None, &decoded_short, &plan_short)
            .expect("each plan builds against its own bytes");
        assert_eq!(report_short.container_sha256(), install::sha256(&short));
        assert_ne!(origin, report_short.scripts()[0].origin);

        // Pairing the long plan with the short container is refused, with the
        // script and the length that did not fit.
        let error =
            resolve_loading_plan(None, &decoded_short, &plan_long).expect_err("foreign bytes");
        assert_eq!(error.code(), "extent");
        assert!(
            matches!(
                error,
                LoadingError::Extent { origin: at, container_len }
                    if at == origin && container_len == short.len() as u64
            ),
            "{error:?}"
        );
        let rendered = error.to_string();
        assert!(rendered.contains("past the"), "{rendered}");
        assert!(rendered.contains(&short.len().to_string()), "{rendered}");
    }

    /// The minimum F07-D scenario: a command recognized as resource-loading
    /// but with no supported key domain must fail with its source offset and
    /// its affected world, not be reported as a loaded script.
    ///
    /// The failure this catches is a validator that folds `Unsupported` into
    /// "loaded nothing" or into "unclassified": either would hide a load the
    /// world really needs and let a caller believe the world is ready.
    #[test]
    fn accept_f07_d_unsupported_command_yields_its_offset_and_world_not_a_loaded_state() {
        let tree = Temp::new("unsupported");
        tree.write("ZBD/c1/plane.flt", b"plane");
        let bytes = container(b"unsupported", &[b"setdir\0world\0subdir\0"]);

        let mut classes = OpcodeClassTable::new();
        classes
            .insert(ClassifiedOpcode {
                spelling: b"setdir".to_vec(),
                class: OpcodeClass::Unsupported,
                status: ClaimStatus::Inferred,
                source: "synthetic test classification: no supported key domain".to_owned(),
            })
            .expect("the classification registers");
        let decoded = decode_interp(
            &mut ParseContext::with_defaults("unsupported.interp"),
            &bytes,
        )
        .expect("the container validates");
        let plan = plan_interp_loading_classified(&decoded, &classes);

        let session = session(&tree, "zbd/c1").expect("the session opens");
        let report =
            resolve_loading_plan(Some(&session), &decoded, &plan).expect("the plan builds");

        // The command yields its source offset and the world it would block.
        assert_eq!(report.stats().unsupported_commands, 1);
        assert_eq!(report.failures().len(), 1);
        let failure = &report.failures()[0];
        assert_eq!(failure.code, "unsupported_command");
        assert_eq!(failure.site.source_offset, 140);
        assert_eq!(failure.site.script, 0);
        assert_eq!(failure.site.line, 0);
        assert_eq!(
            failure
                .world
                .as_ref()
                .map(WorldGroup::as_relative)
                .map(|path| path.as_str()),
            Some("zbd/c1")
        );
        // It is not a fake loaded state: no dependency was invented for it.
        assert_eq!(report.dependencies().len(), 0);
        assert!(report.dependencies_of(0).is_empty());
        assert!(!report.is_complete());
        assert_eq!(
            report.script(0).expect("one script").state.code(),
            "blocked"
        );
        assert_eq!(
            report.script(0).expect("one script").state.blocking_lines(),
            1
        );
        let rendered = failure.to_string();
        assert!(rendered.contains("offset 140"), "{rendered}");
        assert!(rendered.contains("zbd/c1"), "{rendered}");
    }

    /// A command classified as *not* a resource load is carried: it blocks
    /// nothing and contributes no dependency, but it is not interpreted either.
    #[test]
    fn accept_f07_d_behavior_command_is_carried_without_a_dependency_or_a_failure() {
        let bytes = container(b"behavior", &[b"setcamera\0follow\0target\0"]);
        let mut classes = OpcodeClassTable::new();
        classes
            .insert(ClassifiedOpcode {
                spelling: b"setcamera".to_vec(),
                class: OpcodeClass::Behavior,
                status: ClaimStatus::Inferred,
                source: "synthetic test classification: not a resource load".to_owned(),
            })
            .expect("the classification registers");
        let decoded = decode_interp(&mut ParseContext::with_defaults("behavior.interp"), &bytes)
            .expect("the container validates");
        let plan = plan_interp_loading_classified(&decoded, &classes);
        let report = resolve_loading_plan(None, &decoded, &plan).expect("the plan builds");

        assert_eq!(report.stats().behavior_commands, 1);
        assert_eq!(report.dependencies().len(), 0);
        assert!(report.failures().is_empty());
        assert!(report.is_complete());
        assert_eq!(report.script(0).expect("one script").state.code(), "ready");
    }
}
