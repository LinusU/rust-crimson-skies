//! What the installation says about the pilot's view, measured against this
//! camera contract (F21-D).
//!
//! Spec: `specs/F21-cameras-cockpit-views-and-spyglass.md`, stage
//! `### F21-D`. Shared contract: `docs/contracts/UI-NETWORK.md`; evidence
//! contract: `docs/contracts/CLI-EVIDENCE.md`.
//!
//! F21-A declared the mode records, F21-B built the rigs and F21-C ran them
//! under script cameras and capture flags. Every value in those stages is
//! authored design, because nothing about the original's views had been read.
//! This module is the stage that reads: two subjects of the original's own
//! decoded bytes are walked here, and both are answers to a question the sheet
//! asks.
//!
//! 1. **The view controls the original declares.**
//!    [`discover_view_controls`] walks a decoded loading-script container and
//!    reports every occurrence of the camera commands a caller claims, with the
//!    script it came from, its byte offset, its stored arguments and its spans.
//!    The original's `support\display.gw` creates camera *objects* and binds
//!    them to a world and a window, and every world group's `load.gw` levels a
//!    camera's horizon; those are facts about the original's view system, and
//!    the coverage rows say which of them this contract can consume.
//! 2. **The cockpit bindings the original declares.**
//!    [`discover_cockpit_bindings`] walks the loading script that binds the
//!    player plane's cockpit nodes and reports the ordered, deduplicated
//!    binding set; [`audit_cockpit_coverage`] then checks that set against the
//!    real node array of the airframe archive, per airframe, and reports which
//!    model nodes are **verified** and which are missing.
//!
//! # What is measured and what is claimed
//!
//! The **census** is measured: production decoders read the bytes, and a claim
//! that names a command the container never writes, a script it does not hold,
//! or a line stored with a different shape is refused or reported by name. That
//! is what makes a claim falsifiable — the same idiom F11-D2's
//! `RosterDeclarations` uses for the airframe roster.
//!
//! The **classification** is the caller's, and it carries its own
//! [`Provenance`]: whether a camera command is consumed by
//! [`CameraOperation::PlayerRig`] or has no consumer at all is a statement
//! about this engine, made by whoever declares the coverage, not something a
//! byte of the original can answer.
//!
//! # What is **not** claimed, and cannot be from these files
//!
//! * **No behavior.** Nothing here observes the original *running*. Which key
//!   fires which view command, what a camera does when its target dies, whether
//!   a capture leaves the view as it found it: those are runtime facts and need
//!   an owner-supplied original run.
//! * **No default binding.** No shipped file holds a key → command map (F22-H
//!   measured the label vocabulary and found the bindings themselves native).
//! * **No viewpoint transform.** The original declares *which* cockpit nodes
//!   exist; it does not declare, in any file this audit reads, where the
//!   pilot's eye sits inside them. [`eye_placement`] therefore reports
//!   [`CockpitEyeCoverage::Undeclared`] unconditionally rather than accepting a
//!   value from a caller — a cockpit eye invented from a mesh's bounds would be
//!   exactly the "HUD-only synthetic camera" F21 non-negotiable behavior 1
//!   forbids.
//! * **No authored camera timelines.** The world's `cam_anim` carriers exist
//!   and are fingerprinted (F20-D), and the `.zan`/`.zrd` clips they reference
//!   are still an undecoded layout; nothing here reads one.
//!
//! The measured write-up is
//! `docs/findings/2026-10-03-f21-d-original-view-controls-and-cockpit-coverage.md`.

use std::collections::BTreeSet;
use std::fmt;

use cs_content::cameras::CockpitBindingSource;
use cs_content::scene::{DiscoveredAirframe, SceneRootRef};
use cs_formats::gamez::GameZNodes;
use cs_formats::interp::{DecodedInterp, InterpLine};
use cs_types::content::{ContentId, Provenance};

/// The longest a command or binding spelling this module accepts, in bytes.
///
/// Bounded because a declared spelling is echoed into reports, keys and
/// evidence records, so it cannot be an unbounded string from a file nobody
/// fingerprinted.
pub const MAX_DECLARED_NAME: usize = 64;

/// Compares stored bytes with a declared spelling, case-insensitively.
///
/// The loading-script container's names and arguments have **no established
/// encoding** (F07-B): the decoder hands out verbatim bytes and this module
/// never decodes them. Comparing them case-insensitively is how the original's
/// own spellings are matched — `FindSubNode` and `findsubnode` are the same
/// command — and every *value* this module reports keeps its bytes, so a
/// reader can see exactly what was stored.
fn same_name(stored: &[u8], declared: &str) -> bool {
    stored.eq_ignore_ascii_case(declared.as_bytes())
}

/// The stored bytes of an argument as a display string.
///
/// Lossy on purpose and **display only**: no caller decision is taken on the
/// result, every comparison in this module is a byte comparison, and a name the
/// container stores in an encoding this project has not established shows up as
/// a replacement character instead of being silently repaired.
fn display(stored: &[u8]) -> String {
    String::from_utf8_lossy(stored).into_owned()
}

// ======================================================== view controls ====

/// What this camera contract can consume.
///
/// Production-owned: it is the closed set of operations
/// `cs_app::camera` actually has, and a coverage claim may only name one of
/// them. Adding a value here is a statement that the camera path gained the
/// matching operation, which is a code change and not a documentation change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CameraOperation {
    /// The player's own rig: cockpit, external, free look and spyglass (F21-B).
    PlayerRig,
    /// A scripted camera over a bounded span of ticks (F21-C).
    AuthoredCamera,
    /// One deterministic capture frame (F21-C).
    CaptureFrame,
    /// The magnified view of the currently selected target (F21-B).
    MagnifiedTarget,
}

impl CameraOperation {
    /// Every operation, in a stable order.
    pub const ALL: &'static [CameraOperation] = &[
        Self::PlayerRig,
        Self::AuthoredCamera,
        Self::CaptureFrame,
        Self::MagnifiedTarget,
    ];

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::PlayerRig => "player_rig",
            Self::AuthoredCamera => "authored_camera",
            Self::CaptureFrame => "capture_frame",
            Self::MagnifiedTarget => "magnified_target",
        }
    }
}

impl fmt::Display for CameraOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// What one declared camera command asks the original's camera system to do.
///
/// The vocabulary is **project design** and deliberately describes operations
/// rather than original semantics: no value here is a measurement, and the
/// coverage claim that uses it is what a reader checks against this engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ViewControlEffect {
    /// Creates a named camera object.
    CreatesCamera,
    /// Makes a camera the active one.
    ActivatesCamera,
    /// Binds a camera to a world.
    BindsCameraToWorld,
    /// Binds a camera to a window.
    BindsCameraToWindow,
    /// Levels a camera's up axis against the world's horizon.
    LevelsHorizon,
    /// Levels a camera's up axis against a named zone's plane.
    LevelsHorizonToZone,
}

impl ViewControlEffect {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::CreatesCamera => "creates_camera",
            Self::ActivatesCamera => "activates_camera",
            Self::BindsCameraToWorld => "binds_camera_to_world",
            Self::BindsCameraToWindow => "binds_camera_to_window",
            Self::LevelsHorizon => "levels_horizon",
            Self::LevelsHorizonToZone => "levels_horizon_to_zone",
        }
    }
}

impl fmt::Display for ViewControlEffect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Whether a declared command has a consumer in this camera contract.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ViewControlCoverage {
    /// The camera path can consume it.
    Consumed {
        /// Which operation consumes it.
        operation: CameraOperation,
    },
    /// Nothing in the camera path consumes it, and why.
    Unconsumed {
        /// What the camera contract would have to gain. Never empty: a gap that
        /// does not say what it is missing is not a finding.
        reason: String,
    },
}

impl ViewControlCoverage {
    /// The stable label used in reports.
    #[must_use]
    pub fn label(&self) -> &str {
        match self {
            Self::Consumed { operation } => operation.label(),
            Self::Unconsumed { .. } => "unconsumed",
        }
    }
}

/// One camera command a caller claims the container holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewCommandClaim {
    command: String,
    arguments: usize,
    effect: ViewControlEffect,
    coverage: ViewControlCoverage,
    provenance: Provenance,
}

impl ViewCommandClaim {
    /// Assembles a claim about one command spelling.
    ///
    /// `arguments` is the arity the claim says the container stores: the head
    /// plus its arguments, so `CameraSetHorizon horizon` is two. The census
    /// checks it against the bytes and reports a line stored with another shape
    /// as a finding rather than repairing it.
    ///
    /// # Errors
    ///
    /// [`ViewControlError`] when the spelling is empty or longer than
    /// [`MAX_DECLARED_NAME`], or when an [`ViewControlCoverage::Unconsumed`]
    /// names no reason.
    pub fn try_new(
        command: impl Into<String>,
        arguments: usize,
        effect: ViewControlEffect,
        coverage: ViewControlCoverage,
        provenance: Provenance,
    ) -> Result<Self, ViewControlError> {
        let command = command.into();
        require_view_name("command", &command)?;
        if let ViewControlCoverage::Unconsumed { reason } = &coverage
            && reason.trim().is_empty()
        {
            return Err(ViewControlError::UnconsumedWithoutReason { command });
        }
        Ok(Self {
            command,
            arguments,
            effect,
            coverage,
            provenance,
        })
    }

    /// The claimed spelling.
    #[must_use]
    pub fn command(&self) -> &str {
        &self.command
    }

    /// The arity the claim says the container stores.
    #[must_use]
    pub const fn arguments(&self) -> usize {
        self.arguments
    }

    /// What the command asks the original's camera system to do, as this
    /// project classifies it.
    #[must_use]
    pub const fn effect(&self) -> ViewControlEffect {
        self.effect
    }

    /// Whether the camera contract consumes it.
    #[must_use]
    pub const fn coverage(&self) -> &ViewControlCoverage {
        &self.coverage
    }

    /// Where the claim was measured.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// One occurrence of a declared command in the container.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewControlOccurrence {
    command: String,
    script: String,
    offset: u64,
    arguments: Vec<String>,
    provenance: Provenance,
}

impl ViewControlOccurrence {
    /// The command that was stored.
    #[must_use]
    pub fn command(&self) -> &str {
        &self.command
    }

    /// The script the occurrence came from, as the container spells it.
    #[must_use]
    pub fn script(&self) -> &str {
        &self.script
    }

    /// Absolute offset of the line's own header inside the container.
    #[must_use]
    pub const fn offset(&self) -> u64 {
        self.offset
    }

    /// The line's arguments after the command, in stored order.
    #[must_use]
    pub fn arguments(&self) -> &[String] {
        &self.arguments
    }

    /// Where the occurrence was measured.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// One row of the coverage census: a claimed command and everything measured
/// about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewControlRow {
    command: String,
    effect: ViewControlEffect,
    coverage: ViewControlCoverage,
    occurrences: Vec<ViewControlOccurrence>,
    findings: Vec<ViewControlFinding>,
    provenance: Provenance,
}

impl ViewControlRow {
    /// The claimed spelling.
    #[must_use]
    pub fn command(&self) -> &str {
        &self.command
    }

    /// The classified effect.
    #[must_use]
    pub const fn effect(&self) -> ViewControlEffect {
        self.effect
    }

    /// The coverage verdict the claim carries.
    #[must_use]
    pub const fn coverage(&self) -> &ViewControlCoverage {
        &self.coverage
    }

    /// Every occurrence, in container order.
    #[must_use]
    pub fn occurrences(&self) -> &[ViewControlOccurrence] {
        &self.occurrences
    }

    /// How many lines the container stores with this command.
    #[must_use]
    pub fn count(&self) -> usize {
        self.occurrences.len()
    }

    /// The scripts that use it, in first-use order.
    #[must_use]
    pub fn scripts(&self) -> Vec<&str> {
        let mut seen: Vec<&str> = Vec::new();
        for occurrence in &self.occurrences {
            if !seen.iter().any(|name| *name == occurrence.script()) {
                seen.push(occurrence.script());
            }
        }
        seen
    }

    /// What the walk could not read about this command.
    #[must_use]
    pub fn findings(&self) -> &[ViewControlFinding] {
        &self.findings
    }

    /// Where the claim was measured.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// Why the census could not read a line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ViewControlFinding {
    /// A line uses a declared command but is stored with another arity.
    LineUnreadable {
        /// Which command.
        command: String,
        /// The script the line came from.
        script: String,
        /// Absolute offset of the line.
        offset: u64,
        /// The arity the container actually stores.
        stored_arguments: usize,
    },
    /// A line's arguments hold bytes that are not ASCII text.
    ArgumentNotAscii {
        /// Which command.
        command: String,
        /// Absolute offset of the line.
        offset: u64,
    },
}

/// The census: one row per claimed command, plus what the whole walk found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewControlCensus {
    container: String,
    rows: Vec<ViewControlRow>,
    lines_walked: usize,
}

impl ViewControlCensus {
    /// The container label the census was measured over.
    #[must_use]
    pub fn container(&self) -> &str {
        &self.container
    }

    /// One row per claimed command, in claim order.
    #[must_use]
    pub fn rows(&self) -> &[ViewControlRow] {
        &self.rows
    }

    /// The row for one claimed spelling.
    #[must_use]
    pub fn row(&self, command: &str) -> Option<&ViewControlRow> {
        self.rows.iter().find(|row| row.command == command)
    }

    /// How many lines the walk read.
    #[must_use]
    pub const fn lines_walked(&self) -> usize {
        self.lines_walked
    }

    /// Total occurrences across every row.
    #[must_use]
    pub fn occurrences(&self) -> usize {
        self.rows.iter().map(ViewControlRow::count).sum()
    }

    /// Every occurrence, in claim order then container order.
    pub fn all_occurrences(&self) -> impl Iterator<Item = &ViewControlOccurrence> {
        self.rows.iter().flat_map(|row| row.occurrences.iter())
    }

    /// Occurrences whose command the camera contract cannot consume.
    pub fn unconsumed(&self) -> impl Iterator<Item = (&ViewControlRow, &ViewControlOccurrence)> {
        self.rows
            .iter()
            .filter(|row| matches!(row.coverage, ViewControlCoverage::Unconsumed { .. }))
            .flat_map(|row| {
                row.occurrences
                    .iter()
                    .map(move |occurrence| (row, occurrence))
            })
    }

    /// Whether nothing the walk read was a finding.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.rows.iter().all(|row| row.findings.is_empty())
    }
}

/// Why a view-control claim or census was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ViewControlError {
    /// A declared spelling was empty or longer than [`MAX_DECLARED_NAME`].
    DeclaredName {
        /// Which field.
        field: &'static str,
        /// Its length in bytes.
        len: usize,
    },
    /// Two claims declared the same spelling.
    DuplicateCommand {
        /// The spelling.
        command: String,
    },
    /// The claim list was empty, so there is nothing to census.
    EmptyClaim,
    /// An `Unconsumed` verdict named no reason.
    UnconsumedWithoutReason {
        /// The spelling.
        command: String,
    },
    /// The container stores no line with this spelling: the claim is not what
    /// the corpus holds, and a coverage verdict about it would be about nothing.
    ClaimUnseen {
        /// The spelling.
        command: String,
    },
}

impl fmt::Display for ViewControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeclaredName { field, len } => write!(
                f,
                "a declared {field} is non-empty and at most {MAX_DECLARED_NAME} bytes, got {len}"
            ),
            Self::DuplicateCommand { command } => {
                write!(f, "the command {command} is claimed more than once")
            }
            Self::EmptyClaim => {
                f.write_str("a view-control census needs at least one claimed command")
            }
            Self::UnconsumedWithoutReason { command } => write!(
                f,
                "{command} is declared unconsumed without saying what the camera contract would \
                 have to gain"
            ),
            Self::ClaimUnseen { command } => write!(
                f,
                "the container stores no line with the claimed command {command}, so no coverage \
                 verdict about it could be measured"
            ),
        }
    }
}

impl std::error::Error for ViewControlError {}

/// Walks a decoded loading-script container and censuses the claimed camera
/// commands.
///
/// The walk is driven entirely by `claims`: for each one it reads every line of
/// every script, and a line whose head matches the claimed spelling is an
/// occurrence when its stored arity is the claimed one and a
/// [`ViewControlFinding::LineUnreadable`] finding when it is not. A spelling no
/// line matches is [`ViewControlError::ClaimUnseen`], so a claim that is not what
/// the corpus holds fails instead of producing a confident empty row.
///
/// # Errors
///
/// [`ViewControlError`] for an empty claim list, a duplicated spelling or a
/// spelling the container never stores. Line-level problems are findings, not
/// errors: one unreadable line must not hide the lines the walk did read.
pub fn discover_view_controls<'bytes>(
    decoded: &DecodedInterp<'bytes>,
    claims: &[ViewCommandClaim],
    container: &str,
    install: Provenance,
) -> Result<ViewControlCensus, ViewControlError> {
    if claims.is_empty() {
        return Err(ViewControlError::EmptyClaim);
    }
    let mut claimed: BTreeSet<&str> = BTreeSet::new();
    for claim in claims {
        if !claimed.insert(claim.command()) {
            return Err(ViewControlError::DuplicateCommand {
                command: claim.command.clone(),
            });
        }
    }

    let mut rows: Vec<ViewControlRow> = claims
        .iter()
        .map(|claim| ViewControlRow {
            command: claim.command.clone(),
            effect: claim.effect,
            coverage: claim.coverage.clone(),
            occurrences: Vec::new(),
            findings: Vec::new(),
            provenance: claim.provenance.clone(),
        })
        .collect();

    let mut lines_walked = 0_usize;
    for script in decoded.scripts() {
        let script_name = display(script.name());
        for line in script.lines() {
            lines_walked += 1;
            let Some(head) = line_head(line) else {
                continue;
            };
            let Some(index) = claims
                .iter()
                .position(|claim| same_name(head, claim.command()))
            else {
                continue;
            };
            let claim = &claims[index];
            if line.len() != claim.arguments {
                rows[index]
                    .findings
                    .push(ViewControlFinding::LineUnreadable {
                        command: claim.command.clone(),
                        script: script_name.clone(),
                        offset: line.offset(),
                        stored_arguments: line.len(),
                    });
                continue;
            }
            if line.tokens()[1..]
                .iter()
                .any(|token| !token.bytes().is_ascii())
            {
                rows[index]
                    .findings
                    .push(ViewControlFinding::ArgumentNotAscii {
                        command: claim.command.clone(),
                        offset: line.offset(),
                    });
                continue;
            }
            rows[index].occurrences.push(ViewControlOccurrence {
                command: claim.command.clone(),
                script: script_name.clone(),
                offset: line.offset(),
                arguments: line.tokens()[1..]
                    .iter()
                    .map(|token| display(token.bytes()))
                    .collect(),
                provenance: install.clone(),
            });
        }
    }

    if let Some(unseen) = rows.iter().find(|row| row.occurrences.is_empty()) {
        return Err(ViewControlError::ClaimUnseen {
            command: unseen.command.clone(),
        });
    }

    Ok(ViewControlCensus {
        container: container.to_owned(),
        rows,
        lines_walked,
    })
}

/// The head argument of a line, when it has one.
fn line_head<'a>(line: &'a InterpLine<'_>) -> Option<&'a [u8]> {
    line.tokens().first().map(|token| token.bytes())
}

// ===================================================== cockpit bindings ====

/// One declared spelling of the script that binds an aircraft's cockpit nodes.
///
/// The caller declares it, with the spans it measured it at; the walk below
/// reads the container and the airframe archive and reports what it finds. That
/// split is deliberate and is F11-D2's roster idiom: which script and which
/// commands declare the bindings is a claim somebody made against fingerprinted
/// bytes, and the *rows* are measured, so a wrong claim produces no rows and a
/// test pins the count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CockpitBindingClaim {
    script: String,
    node_command: String,
    subnode_command: String,
    airframe_variable: String,
    provenance: Provenance,
}

impl CockpitBindingClaim {
    /// Assembles the claim.
    ///
    /// # Errors
    ///
    /// [`CockpitCoverageError::DeclaredName`] when a spelling is empty or
    /// longer than [`MAX_DECLARED_NAME`], and
    /// [`CockpitCoverageError::RepeatedCommand`] when the two commands are the
    /// same spelling, which would make every line match both.
    pub fn try_new(
        script: impl Into<String>,
        node_command: impl Into<String>,
        subnode_command: impl Into<String>,
        airframe_variable: impl Into<String>,
        provenance: Provenance,
    ) -> Result<Self, CockpitCoverageError> {
        let script = script.into();
        let node_command = node_command.into();
        let subnode_command = subnode_command.into();
        let airframe_variable = airframe_variable.into();
        require_name("script", &script)?;
        require_name("node command", &node_command)?;
        require_name("subnode command", &subnode_command)?;
        require_name("airframe variable", &airframe_variable)?;
        if same_name(node_command.as_bytes(), &subnode_command) {
            return Err(CockpitCoverageError::RepeatedCommand {
                command: node_command,
            });
        }
        Ok(Self {
            script,
            node_command,
            subnode_command,
            airframe_variable,
            provenance,
        })
    }

    /// The script the claim says declares the bindings.
    #[must_use]
    pub fn script(&self) -> &str {
        &self.script
    }

    /// The command that names the airframe.
    #[must_use]
    pub fn node_command(&self) -> &str {
        &self.node_command
    }

    /// The command that names a node inside the airframe.
    #[must_use]
    pub fn subnode_command(&self) -> &str {
        &self.subnode_command
    }

    /// The variable the claim says holds the airframe's root name.
    #[must_use]
    pub fn airframe_variable(&self) -> &str {
        &self.airframe_variable
    }

    /// Where the claim was measured.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// One cockpit node the original's own script binds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CockpitBinding {
    node: String,
    occurrences: usize,
    first_at: u64,
    provenance: Provenance,
}

impl CockpitBinding {
    /// The node name, as the container stores it.
    #[must_use]
    pub fn node(&self) -> &str {
        &self.node
    }

    /// How many lines of the script address this node.
    #[must_use]
    pub const fn occurrences(&self) -> usize {
        self.occurrences
    }

    /// Absolute offset of the first line that addresses it.
    #[must_use]
    pub const fn first_at(&self) -> u64 {
        self.first_at
    }

    /// Where the binding was measured.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// Why the cockpit walk could not read a line, or where a binding sat.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CockpitFinding {
    /// A line uses a declared command but is stored with another arity.
    LineUnreadable {
        /// Which command.
        command: String,
        /// Absolute offset of the line.
        offset: u64,
        /// The arity the container actually stores.
        stored_arguments: usize,
    },
    /// A subnode line appears before any airframe line, so nothing says which
    /// airframe it belongs to and it is not counted as a binding.
    SubnodeOutsideAirframe {
        /// Absolute offset of the line.
        offset: u64,
        /// The node the line names, if it holds printable bytes.
        node: String,
    },
    /// A line's arguments hold bytes that are not ASCII text.
    ArgumentNotAscii {
        /// Which command.
        command: String,
        /// Absolute offset of the line.
        offset: u64,
    },
    /// An airframe line names a variable the claim did not declare, so the walk
    /// does not treat it as the airframe this claim is about.
    ForeignAirframeVariable {
        /// Absolute offset of the line.
        offset: u64,
        /// The variable the line names, if it holds printable bytes.
        variable: String,
    },
}

/// The cockpit binding set one script declares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CockpitBindingDiscovery {
    container: String,
    script: String,
    bindings: Vec<CockpitBinding>,
    findings: Vec<CockpitFinding>,
    lines_walked: usize,
    provenance: Provenance,
}

impl CockpitBindingDiscovery {
    /// The container label the discovery was measured over.
    #[must_use]
    pub fn container(&self) -> &str {
        &self.container
    }

    /// The script the bindings came from, as the container spells it.
    #[must_use]
    pub fn script(&self) -> &str {
        &self.script
    }

    /// The bindings, in first-use order and deduplicated by node name.
    #[must_use]
    pub fn bindings(&self) -> &[CockpitBinding] {
        &self.bindings
    }

    /// The binding for one node name.
    #[must_use]
    pub fn binding(&self, node: &str) -> Option<&CockpitBinding> {
        self.bindings.iter().find(|binding| binding.node == node)
    }

    /// Everything the walk could not read.
    #[must_use]
    pub fn findings(&self) -> &[CockpitFinding] {
        &self.findings
    }

    /// How many lines of the script were read.
    #[must_use]
    pub const fn lines_walked(&self) -> usize {
        self.lines_walked
    }

    /// Where the claim was measured.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// Whether the walk read everything it read without a finding.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }

    /// Whether the discovery found no binding at all, which is a refusal in
    /// everything but the name: a script that binds nothing has not been read.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }
}

/// Why a cockpit claim, discovery or coverage audit was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CockpitCoverageError {
    /// A declared spelling was empty or longer than [`MAX_DECLARED_NAME`].
    DeclaredName {
        /// Which field.
        field: &'static str,
        /// Its length in bytes.
        len: usize,
    },
    /// The claim named one spelling for both commands.
    RepeatedCommand {
        /// The spelling.
        command: String,
    },
    /// The container holds no script with the claimed name.
    ScriptAbsent {
        /// The claimed name.
        script: String,
    },
    /// The container holds more than one script with the claimed name, so which
    /// one declares the bindings is not answerable.
    ScriptAmbiguous {
        /// The claimed name.
        script: String,
        /// How many scripts carry it.
        matches: usize,
    },
    /// The declared script declares no cockpit binding.
    NoBindings {
        /// The script that was read.
        script: String,
    },
    /// An audited airframe's root has no node in the airframe archive.
    RootMissing {
        /// The airframe.
        airframe: String,
        /// The root the roster declares.
        root: String,
    },
    /// Two audited airframes declare the same root, so a per-airframe audit
    /// would attribute one airframe's geometry to another.
    DuplicateRoot {
        /// The root.
        root: String,
    },
    /// A root reference's key does not carry its own container's prefix, so the
    /// stored node name it names cannot be derived.
    RootKeyNotQualified {
        /// The airframe.
        airframe: String,
        /// The container the reference claims.
        container: String,
        /// The root key as it stands.
        root: String,
    },
    /// The airframe archive is not the one the audited airframes live in.
    ContainerMismatch {
        /// The airframe.
        airframe: String,
        /// The container the roster declares.
        declared: String,
        /// The container the node array is.
        measured: String,
    },
}

impl fmt::Display for CockpitCoverageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeclaredName { field, len } => write!(
                f,
                "a declared {field} is non-empty and at most {MAX_DECLARED_NAME} bytes, got {len}"
            ),
            Self::RepeatedCommand { command } => write!(
                f,
                "the node and subnode commands cannot be the same spelling, got {command}"
            ),
            Self::ScriptAbsent { script } => {
                write!(f, "the container holds no script named {script:?}")
            }
            Self::ScriptAmbiguous { script, matches } => write!(
                f,
                "{matches} scripts are named {script:?}, so which one declares the cockpit \
                 bindings is not answerable"
            ),
            Self::NoBindings { script } => write!(
                f,
                "the script {script:?} declares no cockpit binding, so nothing was measured"
            ),
            Self::RootMissing { airframe, root } => write!(
                f,
                "the airframe {airframe} declares the root {root:?}, which the airframe archive \
                 does not hold"
            ),
            Self::DuplicateRoot { root } => write!(
                f,
                "two audited airframes declare the root {root:?}, so a per-airframe coverage row \
                 would attribute one airframe's geometry to another"
            ),
            Self::RootKeyNotQualified {
                airframe,
                container,
                root,
            } => write!(
                f,
                "the airframe {airframe}'s root key {root:?} does not start with its own container \
                 {container:?}, so the stored node name it names is not derivable"
            ),
            Self::ContainerMismatch {
                airframe,
                declared,
                measured,
            } => write!(
                f,
                "the airframe {airframe} lives in {declared}, but the measured node array is {measured}"
            ),
        }
    }
}

impl std::error::Error for CockpitCoverageError {}

/// Walks the declared script and reports the cockpit bindings it declares.
///
/// A binding is a subnode line that follows an airframe line: the airframe line
/// says *which aircraft*, the subnode line says *which node of it*, and a
/// subnode line with no airframe line above it is reported as
/// [`CockpitFinding::SubnodeOutsideAirframe`] rather than attributed to an
/// aircraft nothing named. The same node addressed by several lines is one
/// binding with an occurrence count, so the census is a *set* of names and not
/// a transcript.
///
/// # Errors
///
/// [`CockpitCoverageError::ScriptAbsent`] or
/// [`CockpitCoverageError::ScriptAmbiguous`] when the container does not hold
/// exactly one script with the claimed name, and
/// [`CockpitCoverageError::NoBindings`] when that script declares none — a
/// discovery that found nothing has not discovered anything.
pub fn discover_cockpit_bindings<'bytes>(
    decoded: &DecodedInterp<'bytes>,
    claim: &CockpitBindingClaim,
    container: &str,
    install: Provenance,
) -> Result<CockpitBindingDiscovery, CockpitCoverageError> {
    let mut matching = decoded
        .scripts()
        .iter()
        .filter(|script| same_name(script.name(), claim.script()));
    let script = matching
        .next()
        .ok_or_else(|| CockpitCoverageError::ScriptAbsent {
            script: claim.script().to_owned(),
        })?;
    let matches = 1 + matching.count();
    if matches > 1 {
        return Err(CockpitCoverageError::ScriptAmbiguous {
            script: claim.script().to_owned(),
            matches,
        });
    }

    let mut bindings: Vec<CockpitBinding> = Vec::new();
    let mut findings: Vec<CockpitFinding> = Vec::new();
    let mut within_airframe = false;
    let mut lines_walked = 0_usize;

    for line in script.lines() {
        lines_walked += 1;
        let Some(head) = line_head(line) else {
            continue;
        };
        if same_name(head, claim.node_command()) {
            if line.len() != 2 {
                findings.push(CockpitFinding::LineUnreadable {
                    command: claim.node_command().to_owned(),
                    offset: line.offset(),
                    stored_arguments: line.len(),
                });
                continue;
            }
            let argument = line.tokens()[1].bytes();
            if !argument.is_ascii() {
                findings.push(CockpitFinding::ArgumentNotAscii {
                    command: claim.node_command().to_owned(),
                    offset: line.offset(),
                });
                continue;
            }
            if same_name(argument, claim.airframe_variable()) {
                within_airframe = true;
            } else {
                within_airframe = false;
                findings.push(CockpitFinding::ForeignAirframeVariable {
                    offset: line.offset(),
                    variable: display(argument),
                });
            }
            continue;
        }
        if !same_name(head, claim.subnode_command()) {
            continue;
        }
        if line.len() != 2 {
            findings.push(CockpitFinding::LineUnreadable {
                command: claim.subnode_command().to_owned(),
                offset: line.offset(),
                stored_arguments: line.len(),
            });
            continue;
        }
        let argument = line.tokens()[1].bytes();
        if !argument.is_ascii() {
            findings.push(CockpitFinding::ArgumentNotAscii {
                command: claim.subnode_command().to_owned(),
                offset: line.offset(),
            });
            continue;
        }
        if !within_airframe {
            findings.push(CockpitFinding::SubnodeOutsideAirframe {
                offset: line.offset(),
                node: display(argument),
            });
            continue;
        }
        let node = display(argument);
        match bindings
            .iter_mut()
            .find(|binding| same_name(binding.node.as_bytes(), &node))
        {
            Some(binding) => binding.occurrences += 1,
            None => bindings.push(CockpitBinding {
                node,
                occurrences: 1,
                first_at: line.offset(),
                provenance: install.clone(),
            }),
        }
    }

    if bindings.is_empty() {
        return Err(CockpitCoverageError::NoBindings {
            script: claim.script().to_owned(),
        });
    }

    Ok(CockpitBindingDiscovery {
        container: container.to_owned(),
        script: display(script.name()),
        bindings,
        findings,
        lines_walked,
        provenance: claim.provenance().clone(),
    })
}

/// One airframe the coverage audit checks: the catalog element, and the root
/// its declaring script created inside the airframe archive.
///
/// Deliberately a thin record over two production types rather than a copy of
/// them: [`SceneRootRef`] is the checked `(container, root)` pair F11's roster
/// discovery produces, and [`From<&DiscoveredAirframe>`] builds one from a real
/// roster row, so a caller that has the roster does not restate it and a caller
/// with no roster can still exercise the audit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CockpitAirframe {
    airframe: ContentId,
    root: SceneRootRef,
}

impl CockpitAirframe {
    /// Pairs a catalog element with the root its script created.
    #[must_use]
    pub const fn new(airframe: ContentId, root: SceneRootRef) -> Self {
        Self { airframe, root }
    }

    /// The airframe element.
    #[must_use]
    pub const fn airframe(&self) -> &ContentId {
        &self.airframe
    }

    /// The checked root reference.
    #[must_use]
    pub const fn root(&self) -> &SceneRootRef {
        &self.root
    }

    /// The archive the root lives in.
    #[must_use]
    pub fn container(&self) -> &ContentId {
        self.root.container()
    }

    /// The root's authored name inside that archive.
    #[must_use]
    pub fn root_name(&self) -> &str {
        self.root.root().key()
    }
}

impl From<&DiscoveredAirframe> for CockpitAirframe {
    fn from(row: &DiscoveredAirframe) -> Self {
        Self {
            airframe: row.airframe().clone(),
            root: row.root().clone(),
        }
    }
}

/// How one declared cockpit binding resolved in one airframe's real subtree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CockpitNodeCoverage {
    /// Exactly one node of the airframe's subtree carries this name, and it
    /// references a stored mesh.
    Bound {
        /// The node's index in the airframe archive's node array.
        node_index: u32,
        /// The mesh index its node references.
        mesh_index: u32,
    },
    /// More than one node of the subtree carries this name, so no single
    /// geometry can be named for it.
    Ambiguous {
        /// The indices, in array order.
        node_indices: Vec<u32>,
    },
    /// The subtree holds no node with this name.
    Absent,
    /// The node exists but references no stored mesh, so there is nothing to
    /// draw and no geometry to bind a viewpoint to.
    NoMesh {
        /// The node's index in the airframe archive's node array.
        node_index: u32,
    },
}

impl CockpitNodeCoverage {
    /// Whether this binding resolved to real geometry in this airframe.
    #[must_use]
    pub const fn is_bound(&self) -> bool {
        matches!(self, Self::Bound { .. })
    }

    /// The node index this binding resolved to, when it resolved to one.
    #[must_use]
    pub const fn node_index(&self) -> Option<u32> {
        match self {
            Self::Bound { node_index, .. } | Self::NoMesh { node_index } => Some(*node_index),
            Self::Ambiguous { .. } | Self::Absent => None,
        }
    }

    /// The mesh index this binding resolved to, when it resolved to one.
    #[must_use]
    pub const fn mesh_index(&self) -> Option<u32> {
        match self {
            Self::Bound { mesh_index, .. } => Some(*mesh_index),
            Self::NoMesh { .. } | Self::Ambiguous { .. } | Self::Absent => None,
        }
    }
}

/// One declared binding as it resolved in one airframe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CockpitBindingCoverage {
    node: String,
    coverage: CockpitNodeCoverage,
    occurrences: usize,
    first_at: u64,
    provenance: Provenance,
}

impl CockpitBindingCoverage {
    /// The declared node name.
    #[must_use]
    pub fn node(&self) -> &str {
        &self.node
    }

    /// How the binding resolved.
    #[must_use]
    pub const fn coverage(&self) -> &CockpitNodeCoverage {
        &self.coverage
    }

    /// How many lines of the declaring script address the node.
    #[must_use]
    pub const fn occurrences(&self) -> usize {
        self.occurrences
    }

    /// Absolute offset of the first line that addressed it.
    #[must_use]
    pub const fn first_at(&self) -> u64 {
        self.first_at
    }

    /// Where the binding was measured.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// One airframe's cockpit coverage: every declared binding, as measured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AirframeCockpitCoverage {
    airframe: String,
    root: String,
    root_ref: String,
    container: String,
    root_index: u32,
    subtree_nodes: usize,
    bindings: Vec<CockpitBindingCoverage>,
}

impl AirframeCockpitCoverage {
    /// The audited airframe's id.
    #[must_use]
    pub fn airframe(&self) -> &str {
        &self.airframe
    }

    /// The root's **stored** node name, which is what the archive's node array
    /// holds.
    #[must_use]
    pub fn root(&self) -> &str {
        &self.root
    }

    /// The catalog key the root reference carries, container-qualified.
    ///
    /// Both are reported because they answer different questions: the stored
    /// name is what the archive holds and the audit looked up, and the
    /// reference is what a catalog consumer addresses the root by.
    #[must_use]
    pub fn root_ref(&self) -> &str {
        &self.root_ref
    }

    /// The archive the root lives in.
    #[must_use]
    pub fn container(&self) -> &str {
        &self.container
    }

    /// The root's index in the archive's node array.
    #[must_use]
    pub const fn root_index(&self) -> u32 {
        self.root_index
    }

    /// How many nodes the root's subtree holds, measured over the raw parent and
    /// child arrays.
    #[must_use]
    pub const fn subtree_nodes(&self) -> usize {
        self.subtree_nodes
    }

    /// Every declared binding, in the declaring script's order.
    #[must_use]
    pub fn bindings(&self) -> &[CockpitBindingCoverage] {
        &self.bindings
    }

    /// How many bindings resolved to real geometry.
    #[must_use]
    pub fn bound(&self) -> usize {
        self.bindings
            .iter()
            .filter(|binding| binding.coverage().is_bound())
            .count()
    }

    /// The bindings that did not resolve, in declared order.
    #[must_use]
    pub fn unresolved(&self) -> Vec<&CockpitBindingCoverage> {
        self.bindings
            .iter()
            .filter(|binding| !binding.coverage().is_bound())
            .collect()
    }

    /// Whether every declared binding resolved to real geometry.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.bound() == self.bindings.len()
    }

    /// The **verified** model bindings: one [`CockpitBindingSource`] per
    /// resolved binding, in declared order.
    ///
    /// This is the answer F21 non-negotiable behavior 1 asks for — a name a
    /// cockpit viewpoint can be bound to because the original's own script
    /// addresses that node and the airframe archive really holds it — and it is
    /// deliberately *not* a viewpoint: see [`eye_placement`].
    #[must_use]
    pub fn verified_bindings(&self) -> Vec<CockpitBindingSource> {
        self.bindings
            .iter()
            .filter(|binding| binding.coverage().is_bound())
            .map(|binding| CockpitBindingSource::ModelNode {
                node: binding.node.clone(),
            })
            .collect()
    }

    /// The drawable geometry this airframe's cockpit bindings resolve to, as
    /// `(node, mesh index)` in declared order.
    #[must_use]
    pub fn drawable_meshes(&self) -> Vec<(&str, u32)> {
        self.bindings
            .iter()
            .filter_map(|binding| {
                binding
                    .coverage()
                    .mesh_index()
                    .map(|mesh_index| (binding.node.as_str(), mesh_index))
            })
            .collect()
    }
}

/// Where the pilot's eye sits inside a cockpit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CockpitEyeCoverage {
    /// No file this audit reads declares the eye.
    ///
    /// Unconditional and production-owned on purpose. The original's scripts
    /// say *which* cockpit nodes an aircraft has; the transform that puts the
    /// pilot's eye inside one is not in the loading scripts, the airframe
    /// archive or any other readable container this project opens, so the audit
    /// reports it undeclared instead of accepting a value from a caller. An eye
    /// derived from a mesh's bounds would be the "HUD-only synthetic camera"
    /// F21 non-negotiable behavior 1 forbids in place of a verified binding.
    Undeclared,
}

impl CockpitEyeCoverage {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Undeclared => "undeclared",
        }
    }
}

/// The coverage of every audited airframe's cockpit bindings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CockpitCoverageReport {
    archive: String,
    rows: Vec<AirframeCockpitCoverage>,
    declared: usize,
    eye: CockpitEyeCoverage,
}

impl CockpitCoverageReport {
    /// The airframe archive the audit measured over.
    #[must_use]
    pub fn archive(&self) -> &str {
        &self.archive
    }

    /// One row per audited airframe, in the roster's order.
    #[must_use]
    pub fn rows(&self) -> &[AirframeCockpitCoverage] {
        &self.rows
    }

    /// The row for one airframe.
    #[must_use]
    pub fn row(&self, airframe: &str) -> Option<&AirframeCockpitCoverage> {
        self.rows.iter().find(|row| row.airframe == airframe)
    }

    /// How many bindings the declaring script declares, once per airframe.
    #[must_use]
    pub const fn declared_per_airframe(&self) -> usize {
        self.declared
    }

    /// Where the pilot's eye sits: [`CockpitEyeCoverage::Undeclared`].
    #[must_use]
    pub const fn eye(&self) -> CockpitEyeCoverage {
        self.eye
    }

    /// Every airframe whose declared bindings all resolved.
    pub fn complete(&self) -> impl Iterator<Item = &AirframeCockpitCoverage> {
        self.rows.iter().filter(|row| row.is_complete())
    }

    /// Every `(airframe, node, mesh index)` triple the audit resolved, in row
    /// order: the geometry a capture may draw.
    #[must_use]
    pub fn drawable_meshes(&self) -> Vec<(&str, &str, u32)> {
        self.rows
            .iter()
            .flat_map(|row| {
                row.drawable_meshes()
                    .into_iter()
                    .map(move |(node, mesh_index)| (row.airframe(), node, mesh_index))
            })
            .collect()
    }

    /// Whether every audited airframe resolved every declared binding.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        !self.rows.is_empty() && self.rows.iter().all(AirframeCockpitCoverage::is_complete)
    }
}

/// Where the pilot's eye sits: [`CockpitEyeCoverage::Undeclared`].
///
/// A function rather than a value a caller may set, so "the eye is unknown" is
/// a property of this audit and not something a caller can quietly fill in. See
/// [`CockpitEyeCoverage::Undeclared`].
#[must_use]
pub const fn eye_placement() -> CockpitEyeCoverage {
    CockpitEyeCoverage::Undeclared
}

/// Checks a discovered binding set against the real node array of the airframe
/// archive, once per audited airframe.
///
/// Each binding is looked up by **name inside that airframe's own subtree**,
/// walked over the raw parent and child arrays: a node with the same name under
/// a different aircraft is another aircraft's node, and an airframe must never
/// be given another's geometry. A node that resolves but references no stored
/// mesh is [`CockpitNodeCoverage::NoMesh`] — present, named by the original, and
/// not drawable — because "the node exists" is not "there is a cockpit there".
///
/// # Errors
///
/// `archive_id` is the **catalog id** of the archive the caller read, built
/// through [`cs_content::catalog::baseline::install_file_key`] exactly as the
/// roster discovery builds its container keys: comparing a typed id against a
/// hand-typed string would be a check a caller could satisfy by spelling the
/// string the way the check expects.
///
/// [`CockpitCoverageError::ContainerMismatch`] when an airframe's declared
/// container is not the measured archive, [`CockpitCoverageError::DuplicateRoot`]
/// when two airframes claim one root, and
/// [`CockpitCoverageError::RootMissing`] when an airframe's root has no node at
/// all. An airframe that cannot be located is refused rather than reported
/// empty: "no cockpit" and "no aircraft" are different findings.
pub fn audit_cockpit_coverage(
    discovery: &CockpitBindingDiscovery,
    airframes: &[CockpitAirframe],
    archive: &GameZNodes,
    archive_id: &ContentId,
) -> Result<CockpitCoverageReport, CockpitCoverageError> {
    if discovery.is_empty() {
        return Err(CockpitCoverageError::NoBindings {
            script: discovery.script().to_owned(),
        });
    }
    let mut roots: BTreeSet<&str> = BTreeSet::new();
    let mut rows = Vec::with_capacity(airframes.len());
    let archive_label = archive_id.as_str();
    for airframe in airframes {
        if airframe.container() != archive_id {
            return Err(CockpitCoverageError::ContainerMismatch {
                airframe: airframe.airframe().as_str().to_owned(),
                declared: airframe.container().as_str().to_owned(),
                measured: archive_label.to_owned(),
            });
        }
        let declared = archive_label;
        let (root_name, root_ref) = stored_root_name(airframe)?;
        if !roots.insert(root_name) {
            return Err(CockpitCoverageError::DuplicateRoot {
                root: root_name.to_owned(),
            });
        }
        let Some(root) = archive.nodes.iter().find(|node| node.name == root_name) else {
            return Err(CockpitCoverageError::RootMissing {
                airframe: airframe.airframe().as_str().to_owned(),
                root: root_name.to_owned(),
            });
        };
        let subtree = subtree_indices(archive, root.index);
        let bindings = discovery
            .bindings()
            .iter()
            .map(|binding| {
                let mut found: Vec<u32> = subtree
                    .iter()
                    .copied()
                    .filter(|index| {
                        archive
                            .get(*index)
                            .is_some_and(|node| node.name == binding.node())
                    })
                    .collect();
                found.sort_unstable();
                let coverage = match found.as_slice() {
                    [] => CockpitNodeCoverage::Absent,
                    [only] => match archive.get(*only).map(|node| node.info.mesh_index) {
                        Some(mesh_index) if mesh_index >= 0 => CockpitNodeCoverage::Bound {
                            node_index: *only,
                            mesh_index: mesh_index as u32,
                        },
                        _ => CockpitNodeCoverage::NoMesh { node_index: *only },
                    },
                    many => CockpitNodeCoverage::Ambiguous {
                        node_indices: many.to_vec(),
                    },
                };
                CockpitBindingCoverage {
                    node: binding.node().to_owned(),
                    coverage,
                    occurrences: binding.occurrences(),
                    first_at: binding.first_at(),
                    provenance: binding.provenance().clone(),
                }
            })
            .collect();
        rows.push(AirframeCockpitCoverage {
            airframe: airframe.airframe().as_str().to_owned(),
            root: root.name.clone(),
            root_ref: root_ref.to_owned(),
            container: declared.to_owned(),
            root_index: root.index,
            subtree_nodes: subtree.len(),
            bindings,
        });
    }

    Ok(CockpitCoverageReport {
        archive: archive_label.to_owned(),
        rows,
        declared: discovery.bindings().len(),
        eye: eye_placement(),
    })
}

/// The stored node name a root reference names.
///
/// A [`SceneRootRef`]'s key is **container-qualified** — `zbd_2f_planes.zbd.player_kestrel`
/// — while the archive's node array stores the bare node name
/// (`player_kestrel`). The prefix is stripped here, and a root key that does not
/// carry its container's prefix is [`CockpitCoverageError::RootKeyNotQualified`]
/// rather than a lookup for a name no node could hold.
fn stored_root_name(airframe: &CockpitAirframe) -> Result<(&str, &str), CockpitCoverageError> {
    let reference = airframe.root_name();
    let prefix = format!("{}.", airframe.container().key());
    match reference.strip_prefix(&prefix) {
        Some(stored) if !stored.is_empty() => Ok((stored, reference)),
        _ => Err(CockpitCoverageError::RootKeyNotQualified {
            airframe: airframe.airframe().as_str().to_owned(),
            container: airframe.container().key().to_owned(),
            root: reference.to_owned(),
        }),
    }
}

/// Every node index in one root's subtree, the root first, in array order.
///
/// Iterative on purpose: the archive's own arrays are the only structure walked,
/// an untrusted graph gets no recursion depth from this function, and a cycle
/// in the parent/child arrays stops at the already-visited set instead of
/// looping forever. A node whose stored index is past the array is skipped and
/// counted nowhere, because there is no node to attribute it to.
fn subtree_indices(archive: &GameZNodes, root: u32) -> Vec<u32> {
    let mut visited: BTreeSet<u32> = BTreeSet::new();
    let mut queue = vec![root];
    while let Some(index) = queue.pop() {
        if !visited.insert(index) {
            continue;
        }
        let Some(node) = archive.get(index) else {
            continue;
        };
        queue.extend(node.children.iter().copied());
        if let Some(parent) = node.parent {
            queue.push(parent);
        }
    }
    visited.into_iter().collect()
}

/// The length a declared spelling has, or the length it must be refused for.
fn declared_name_len(value: &str) -> Result<usize, usize> {
    if value.is_empty() || value.len() > MAX_DECLARED_NAME {
        Err(value.len())
    } else {
        Ok(value.len())
    }
}

/// Requires a declared spelling to be non-empty and bounded.
fn require_name(field: &'static str, value: &str) -> Result<(), CockpitCoverageError> {
    declared_name_len(value)
        .map(|_| ())
        .map_err(|len| CockpitCoverageError::DeclaredName { field, len })
}

/// Requires a declared spelling to be non-empty and bounded.
fn require_view_name(field: &'static str, value: &str) -> Result<(), ViewControlError> {
    declared_name_len(value)
        .map(|_| ())
        .map_err(|len| ViewControlError::DeclaredName { field, len })
}
