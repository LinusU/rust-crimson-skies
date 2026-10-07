//! The host's half of a mod mount: the bounded payload validator
//! (F53-B-FU1) and the mod screen projection (F53-C).
//!
//! Two stages of `specs/F53-mod-mounts-custom-content-and-compatibility-signatures.md`
//! live in this module, both host-side because `cs_content` may not name
//! `cs_script` (nor `cs_net`) types (`docs/01-ARCHITECTURE.md`): the
//! capability behind F53-B's fail-closed gate, and the view + wire record
//! the mounted set projects into. Shared contracts:
//! `docs/contracts/IDENTITY-CONTENT.md` and
//! `docs/contracts/SCRIPT-MISSION.md` ("IR requirements", "Program
//! security"). The validator's owner paths, payload-encoding decision,
//! unknowns and checks are recorded in
//! `docs/findings/2026-10-07-f53-b-fu1-bounded-mod-mission-payload-validator.md`.
//!
//! # The bounded validator (task F53-B-FU1, #743)
//!
//! [`cs_content::mods::mount_mods`] already refuses a mission or script
//! override when the host supplies no [`ProgramValidator`], and refuses it
//! when the validator refuses — the gate is fail-closed. What F53-B (#213)
//! left behind is the *capability* behind that gate: nothing in the
//! workspace could decode **mod-authored bytes** into a
//! [`cs_script::ir::MissionProgram`], so every sandboxed override stayed
//! refused. This module is that capability: [`decode_mission_program`] runs
//! the measured reader chain over a payload and ends at
//! `MissionProgram::validate`, [`MissionPayloadValidator`] exposes it as
//! the [`ProgramValidator`] the mount asks, and [`mount_selection`] /
//! [`mount_environment`] are where this host attaches it to the mounts it
//! makes.
//!
//! ## Why this lives in `cs_app`
//!
//! `cs_content` may not name `cs_script`'s types (`docs/01-ARCHITECTURE.md`),
//! so the mount states the rule and the *host* supplies the capability that
//! implements it. `cs_app` is the crate that already owns that crossing —
//! [`crate::control_lowering::lower_control_record`] is the adapter retail
//! mission records are lowered through — and the F53 sheet names
//! `crates/cs_app/src/ui/mods.rs` as F53's `cs_app` owner path, so the host
//! half of a mod mount belongs here rather than in a new crate.
//!
//! ## The payload encoding, and what is designed rather than measured
//!
//! A payload for a [`ContentKind::Mission`] target is the mission's **control
//! record** in the measured `.zrd` encoding: a one-element wrapper around a
//! flat key/value record whose numbered `OBJECTIVE<N>` blocks the M01-LC
//! stages measured (`docs/findings/2026-10-04-m01-lc-mission-program.md`).
//! Nothing here invents a byte layout. The decode is
//! [`cs_content::stunts::decode_zrd`] — a bounded reader that refuses on an
//! unknown tag, a count larger than the bytes left, an over-deep document or
//! trailing bytes — and everything after it is the retail adapter chain
//! (`measure_control_record` → [`crate::control_lowering::lower_control_record`]
//! → `MissionProgram::validate`), so a mod payload is decoded and validated by
//! exactly the code the original-adapter path uses.
//!
//! What *is* designed, and labelled as such: that a mod ships the control
//! record document itself rather than the retail `zrdr.zbd` reader archive that
//! wraps it in the installation. The original game's mod support is unmeasured
//! (F53 "Research boundary"), so no mod packaging was measured; a format that
//! had to be *invented* would instead keep the mount refused. The `.zrd`
//! encoding of the program itself is measured, and that is what this decoder
//! reads. The packaging question is recorded as an open unknown in the finding
//! above; if it is ever measured to be the wrapped archive, the decode step
//! here changes and nothing else does.
//!
//! ## What is refused, and why nothing degrades
//!
//! Every arm of [`MissionPayloadRefusal`] is a refusal of the *mount*, quoted
//! in [`cs_content::mods::MountError::ProgramRejected`]:
//!
//! * a target whose kind has no measured payload encoding in this build —
//!   [`ContentKind::Mission`] is the only one, so `Script`, `Objective`,
//!   `Trigger`, `Route`, `Instruction`, `NativeBinding`, `IaScenario` and
//!   `MultiplayerScenario` overrides stay refused rather than guessed;
//! * bytes that do not decode as a `.zrd` document;
//! * a document that declares no numbered `OBJECTIVE<N>` block, so it is not a
//!   mission control program at all;
//! * a record the adapter could not lower — an unmeasured directive key binds
//!   nothing, a damaged block carries no trustworthy condition — reported with
//!   [`cs_content::mission_control::MeasuredControlRecord::to_lowering_refusal`];
//! * a lowered program that `MissionProgram::validate` refuses, which is the
//!   check F53 non-negotiable 2 names.
//!
//! The decode result is the *evidence for the mount's decision*; this stage
//! does not hand the program to a session (there is no mod mission loader yet —
//! F53-C wires selection, diagnostics and export, and the missing consumer is
//! recorded in the finding rather than papered over).
//!
//! # The screen projection and the lobby fold (F53-C)
//!
//! Spec: `specs/F53-mod-mounts-custom-content-and-compatibility-signatures.md`,
//! stage `### F53-C`. The selection truth is
//! [`cs_content::mods::ModSelection`], owned by the host's producer; this
//! module is the view's projection of it plus the one place the mounted
//! set's compatibility signature enters `cs_net` vocabulary
//! ([`lobby_compatibility`]). No game state lives here: a view is rebuilt
//! from a selection and a mount outcome, never mutated into one.
//!
//! [`ModsView::project`] turns a selection and the mount it produced into
//! display rows and notices. A refused mount keeps its
//! [`cs_content::mods::MountError::code`] and message verbatim — the
//! diagnosis is a property of the projection, not of a renderer — and the
//! mount's full [`cs_content::mods::mount_to_text`] report is carried for
//! the diagnostics pane.
//!
//! [`lobby_compatibility`] answers what the session announces on the wire
//! (F53 AC03): a stock session announces the base installation fingerprint;
//! a mounted session announces [`cs_content::mods::MountedMods::signature`],
//! which covers the plan and the measured payload bytes, so a tuning mod
//! can never silently share a lobby with stock content — the handshake
//! refuses the pair as `ContentMismatch` before launch.
//!
//! No widget layout or input is wired (that is the front-end's stage);
//! notices carry their stable code and the designed English fallback only.

use std::fmt;

use cs_content::mission_control::measure_control_record;
use cs_content::mods::{
    AvailableMod, ModManifest, ModModification, ModSelection, ModVersion, MountEnvironment,
    MountError, MountRequest, MountedMods, ProgramValidator, content_signature, mount_to_text,
};
use cs_content::stunts::decode_zrd;
use cs_net::compat::Compatibility;
use cs_script::ir::ValidatedProgram;
use cs_types::asset_id::ModId;
use cs_types::content::{ContentId, ContentKind};
use cs_types::evidence::ContentHash;

use crate::control_lowering::lower_control_record;

/// Why a sandboxed mod payload may not be enabled.
///
/// The mount quotes [`fmt::Display`] as
/// [`cs_content::mods::MountError::ProgramRejected`]'s `reason`, so every arm
/// has to say what the payload *is* and what refused it, without leaking a
/// host path: only the target id, the decoder's own refusal code and the
/// adapter's own diagnostic appear.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MissionPayloadRefusal {
    /// The target's kind has no measured payload encoding in this build.
    ///
    /// [`cs_content::mods::classify_validation`] marks nine kinds sandboxed;
    /// only [`ContentKind::Mission`] has a payload encoding this build can
    /// read, so the other eight stay refused.
    UnsupportedKind {
        /// The payload's target id.
        target: ContentId,
        /// That target's kind, as the id itself spells it.
        kind: ContentKind,
    },
    /// The bytes are not a decodable `.zrd` document.
    Undecodable {
        /// The bounded reader's own refusal, code and offset included.
        detail: String,
    },
    /// The document carries no numbered `OBJECTIVE<N>` block, so it is not a
    /// mission control program.
    NotAControlRecord,
    /// The control record could not be lowered into a mission program.
    Lowering {
        /// One line per unmet lowering requirement, from the record itself.
        detail: String,
    },
    /// The lowered program was refused by `MissionProgram::validate`.
    Validation {
        /// The validation error, locator included.
        detail: String,
    },
}

impl fmt::Display for MissionPayloadRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedKind { target, kind } => write!(
                f,
                "{target} is {} content: this build has no measured payload encoding for that \
                 kind, so the sandboxed payload stays refused",
                kind.label()
            ),
            Self::Undecodable { detail } => write!(
                f,
                "the payload is not a decodable .zrd control record: {detail}"
            ),
            Self::NotAControlRecord => write!(
                f,
                "the payload decodes but declares no numbered OBJECTIVE block, so it is not a \
                 mission control program"
            ),
            Self::Lowering { detail } => write!(
                f,
                "the payload's control record did not lower into a mission program: {detail}"
            ),
            Self::Validation { detail } => write!(
                f,
                "the lowered mission program is refused by MissionProgram::validate: {detail}"
            ),
        }
    }
}

impl std::error::Error for MissionPayloadRefusal {}

/// Decodes a sandboxed mod payload into the mission IR and validates it.
///
/// This is the production path F53 non-negotiable 2 asks for: the payload's
/// bytes go through the bounded `.zrd` reader, the measured control-record
/// walk and the retail lowering adapter, and the result is handed to
/// `cs_script::ir::MissionProgram::validate` — the same function an original
/// adapter's program is validated with, because it *is* that function, run
/// over a program the same adapter chain built.
///
/// `target` is the content id the mount is serving, so the program's mission
/// identity comes from the id rather than from anything the payload asserts:
/// the payload cannot choose which mission it claims to be.
///
/// # Errors
///
/// One [`MissionPayloadRefusal`] per way the payload may not be enabled; none
/// of them is a warning and none is retried with a permissive fallback.
pub fn decode_mission_program(
    target: &ContentId,
    bytes: &[u8],
) -> Result<ValidatedProgram, MissionPayloadRefusal> {
    if target.kind() != ContentKind::Mission {
        return Err(MissionPayloadRefusal::UnsupportedKind {
            target: target.clone(),
            kind: target.kind(),
        });
    }

    // 1. The bounded reader: unknown tag, impossible count, over-deep nesting
    //    and trailing bytes are all refusals, so the rest of this function only
    //    ever sees a whole, well-formed document.
    let document = decode_zrd(bytes).map_err(|error| MissionPayloadRefusal::Undecodable {
        detail: error.to_string(),
    })?;

    // 2. The measured control-member rule, applied to the document itself: a
    //    payload with no numbered block is not a mission program, whatever it
    //    contains.
    let record = measure_control_record(&document);
    if record.blocks() == 0 {
        return Err(MissionPayloadRefusal::NotAControlRecord);
    }

    // 3. The retail adapter. The mission identity is the target id, resolved;
    //    every per-site and per-block refusal is reported, never skipped.
    let lowered = lower_control_record(Ok(target.clone()), target.as_str(), &document, &record);
    let program = lowered
        .program()
        .cloned()
        .ok_or_else(|| MissionPayloadRefusal::Lowering {
            detail: record.to_lowering_refusal(lowered.attempt()),
        })?;

    // 4. F53 non-negotiable 2's own words: the bounded validator the original
    //    adapters use. A program that bound but does not validate is refused
    //    here, by name.
    program
        .validate()
        .map_err(|error| MissionPayloadRefusal::Validation {
            detail: error.to_string(),
        })
}

/// The capability the mount asks for before it enables a sandboxed payload.
///
/// Stateless on purpose: it decides whether bytes may be enabled, and the
/// verdict is the [`ValidatedProgram`] [`decode_mission_program`] produced. It
/// cannot accept a payload the bounded validator refused, and the mount never
/// consults it for a non-sandboxed target, so it is a capability rather than a
/// policy knob.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MissionPayloadValidator;

impl ProgramValidator for MissionPayloadValidator {
    fn validate(&self, target: &ContentId, bytes: &[u8]) -> Result<(), String> {
        decode_mission_program(target, bytes)
            .map(|_| ())
            .map_err(|refusal| refusal.to_string())
    }
}

/// The validator every mod mount this host makes carries.
///
/// `'static` and stateless so both mounting paths below can hand the mount a
/// borrow that outlives any caller-built root list.
static HOST_MISSION_PAYLOAD_VALIDATOR: MissionPayloadValidator = MissionPayloadValidator;

/// The mount this host makes: the F53-C producer path with the bounded
/// validator attached.
///
/// [`ModSelection::mount`] takes the validator as an `Option` because
/// `cs_content` may not name `cs_script`'s types; this function is where the
/// host answers that question, in one place, so a caller cannot forget it:
/// a sandboxed payload either passes [`decode_mission_program`] or the mount
/// is refused. Passing `None` (what [`ModSelection::mount`] itself does when
/// the host supplies no validator) is F53-B's fail-closed state, where every
/// mission or script override is refused with
/// [`cs_content::mods::MountError::UnvalidatedProgram`] — and
/// `accept_f53_b_fu1_the_selection_mount_carries_the_validator` fails.
///
/// # Errors
///
/// Every [`MountError`] of [`cs_content::mods::mount_mods`], propagated
/// unchanged.
pub fn mount_selection(
    selection: &ModSelection,
    request: &MountRequest,
    base_fingerprint: ContentHash,
) -> Result<MountedMods, MountError> {
    selection.mount(
        request,
        base_fingerprint,
        Some(&HOST_MISSION_PAYLOAD_VALIDATOR),
    )
}

/// The [`MountEnvironment`] this host mounts mods with: the base installation
/// fingerprint the caller names, plus the bounded validator F53-B's gate asks
/// for. This is the raw path for a caller that assembles its own roots and
/// calls [`cs_content::mods::mount_mods`] directly; the producer path above
/// ([`mount_selection`]) is what the selection-based host uses. Both attach
/// the same validator, because it is this build's capability rather than a
/// caller choice.
///
/// Roots are *not* taken here — only the caller knows where each mod's root
/// directory is, so they are chained with
/// [`MountEnvironment::with_root`] as before. Deleting the
/// [`MountEnvironment::with_program_validator`] call below returns this path
/// to F53-B's fail-closed state, where every mission or script override is
/// refused with [`cs_content::mods::MountError::UnvalidatedProgram`] — and
/// `accept_f53_b_fu1_a_valid_sandboxed_mission_payload_mounts` fails.
///
/// # Example
///
/// ```ignore
/// let environment = mount_environment(base_fingerprint)
///     .with_root(mod_id, mod_root_directory);
/// let mounted = cs_content::mods::mount_mods(&set, &request, &environment)?;
/// ```
#[must_use = "a mount built without this environment refuses sandboxed payloads"]
pub fn mount_environment(base_fingerprint: ContentHash) -> MountEnvironment<'static> {
    MountEnvironment::new(base_fingerprint).with_program_validator(&HOST_MISSION_PAYLOAD_VALIDATOR)
}

/// The `cs_net` handshake record a session announces, with the F53-B
/// compatibility signature folded into `content_sha256` (AC03).
///
/// `rules_sha256` stays the caller's own rules hash; `base_fingerprint` is
/// the same installation fingerprint the mount was measured against, so a
/// stock session announces exactly the bytes it runs and a mounted session
/// announces the domain-separated signature of its plan and payload bytes.
///
/// The wire's `mods` field names enabled mods as *catalog content ids* —
/// and no [`cs_types::content::ContentKind`] names a mod yet, so this stage
/// leaves it empty rather than mislabel a mod as another kind. Nothing is
/// lost: the signature already carries the enabled set and its bytes, so the
/// gate is complete; the list is display information pending the
/// vocabulary (filed as a follow-up).
#[must_use]
pub fn lobby_compatibility(
    rules_sha256: ContentHash,
    base_fingerprint: ContentHash,
    mounted: Option<&MountedMods>,
) -> Compatibility {
    Compatibility {
        rules_sha256,
        content_sha256: content_signature(mounted, base_fingerprint),
        mods: Vec::new(),
    }
}

/// One row of the mod screen: a discovered mod and the mount's verdict on
/// it. `load_position` and `modification` are set only when the enabled
/// set mounted — a disabled or refused mod has no order to report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModRow {
    /// The mod's id.
    pub id: ModId,
    /// Its display name, from the manifest.
    pub name: String,
    /// Its declared version.
    pub version: ModVersion,
    /// Whether the selection enables it.
    pub enabled: bool,
    /// Its position in the mounted load order, if mounted.
    pub load_position: Option<usize>,
    /// Its modification class under the mounted plan, if mounted.
    pub modification: Option<ModModification>,
}

/// A line the mod screen shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModsNotice {
    /// The mount was refused. `code` is the stable
    /// [`MountError::code`] for machine-readable reports; `detail` is the
    /// error's own message, verbatim.
    MountRefused {
        /// The stable refusal code.
        code: &'static str,
        /// The refusal message.
        detail: String,
    },
    /// One concrete plan problem inside a refused mount, kept separate
    /// from the summary so the screen can list each.
    PlanProblem {
        /// The stable problem code.
        code: &'static str,
        /// The problem message.
        detail: String,
    },
    /// A mounted session marks its saves, replays and handshakes for
    /// gameplay reasons (F53 non-negotiable 3) — shown so the operator
    /// knows the run is modified before they join anything.
    SessionMarked,
}

impl ModsNotice {
    /// The designed English fallback line. Original strings are not
    /// involved; localization (F51) supplies the real text by `code`.
    pub fn fallback_text(&self) -> String {
        match self {
            Self::MountRefused { code, detail } => format!("Mods are disabled ({code}): {detail}"),
            Self::PlanProblem { code, detail } => format!("Mod plan problem ({code}): {detail}"),
            Self::SessionMarked => {
                "This session is modified: saves, replays and multiplayer joins are marked"
                    .to_owned()
            }
        }
    }
}

/// The view's projection of a [`ModSelection`] and the mount it produced.
#[derive(Clone, Debug, Default)]
pub struct ModsView {
    rows: Vec<ModRow>,
    notices: Vec<ModsNotice>,
    signature: Option<ContentHash>,
    mount_report: Option<String>,
}

impl ModsView {
    /// An empty view.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Projects a selection and its mount outcome into rows, notices and
    /// the announced signature.
    ///
    /// `outcome` is `Some(Ok(mounted))` when the enabled set mounted,
    /// `Some(Err(error))` when [`cs_content::mods::mount_mods`] refused it —
    /// the refusal is surfaced per problem, never silently dropped — and
    /// `None` when the host has not mounted the current selection yet
    /// (rows still show what is enabled, with no load order to report).
    #[must_use]
    pub fn project(
        selection: &ModSelection,
        outcome: Option<Result<&MountedMods, &MountError>>,
    ) -> Self {
        let mounted = outcome.and_then(Result::ok);
        let mut view = Self {
            rows: selection
                .available()
                .map(|available| Self::row(selection, available, mounted))
                .collect(),
            notices: Vec::new(),
            signature: mounted.map(MountedMods::signature),
            mount_report: mounted.map(mount_to_text),
        };
        if let Some(Err(error)) = outcome {
            view.notices.push(ModsNotice::MountRefused {
                code: error.code(),
                detail: error.to_string(),
            });
            if let Some(plan) = error.plan_error() {
                view.notices.extend(plan.problems().iter().map(|problem| {
                    ModsNotice::PlanProblem {
                        code: problem.code(),
                        detail: problem.to_string(),
                    }
                }));
            }
        }
        if mounted.is_some_and(MountedMods::marks_sessions) {
            view.notices.push(ModsNotice::SessionMarked);
        }
        view
    }

    fn row(
        selection: &ModSelection,
        available: &AvailableMod,
        mounted: Option<&MountedMods>,
    ) -> ModRow {
        let manifest: &ModManifest = available.manifest();
        let planned = mounted.and_then(|mounted| mounted.plan().mod_entry(manifest.id()));
        ModRow {
            id: manifest.id().clone(),
            name: manifest.header().name().to_owned(),
            version: manifest.version(),
            enabled: selection.is_enabled(manifest.id()),
            load_position: planned.map(|entry| entry.position()),
            modification: planned.map(|entry| entry.modification()),
        }
    }

    /// The rows, in mod-id order (the selection's own order).
    pub fn rows(&self) -> &[ModRow] {
        &self.rows
    }

    /// The notices, in the order they were diagnosed.
    pub fn notices(&self) -> &[ModsNotice] {
        &self.notices
    }

    /// The compatibility signature the enabled set announced, if mounted.
    /// This is what [`lobby_compatibility`] puts in `content_sha256`.
    pub fn signature(&self) -> Option<ContentHash> {
        self.signature
    }

    /// The full mount report (`mount_to_text`), for the diagnostics pane.
    pub fn mount_report(&self) -> Option<&str> {
        self.mount_report.as_deref()
    }
}

#[cfg(test)]
mod tests {
    //! `accept_f53_b_fu1_*` (#743) and `accept_f53_c_*` (F53-C) acceptance.
    //! Every tree and every payload here is newly authored synthetic bytes
    //! below the system temporary directory: no original game data and no
    //! `CS_GAME_DIR` access.
    //!
    //! The F53-B-FU1 suite runs the **production** chain end to end: the
    //! payloads are written where a mod ships them, `mount_mods` reads them,
    //! and the validator this module wires into [`mount_environment`] and
    //! [`mount_selection`] is what decides.

    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use cs_content::mods::{
        ContentOverride, EngineRange, ModHeader, ModManifest, ModPayload, ModSelection, ModSet,
        ModVersion, MountError, MountedMods, OverrideAction, OverrideValidation, mount_mods,
        synthetic_conflicting_mods, synthetic_mission_mod, synthetic_mod_claim,
        synthetic_mount_request, synthetic_tuning_mod,
    };
    use cs_content::stunts::{ZRD_TAG_FLOAT, ZRD_TAG_LIST, ZRD_TAG_TEXT};
    use cs_net::compat::{
        ClientHello, HandshakeReject, PROTOCOL_VERSION, SessionParameters, evaluate_hello,
    };
    use cs_types::asset_id::ModId;
    use cs_types::content::{ContentId, ContentKind, Provenance};

    use super::*;

    static NEXT: AtomicU64 = AtomicU64::new(0);

    /// A disposable directory, removed on drop.
    struct Temp(PathBuf);

    impl Temp {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "cs-f53-mods-{label}-{}-{}",
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

    /// Writes one manifest's declared sources below `root`, so the mount's
    /// measured digests are over real files.
    fn write_mod(root: &Temp, manifest: &ModManifest) {
        for entry in manifest.overrides() {
            let path = root.path().join(entry.source().as_str());
            fs::create_dir_all(path.parent().expect("has a parent")).expect("dirs");
            let mut bytes = entry.target().as_str().as_bytes().to_vec();
            bytes.extend_from_slice(b"::synthetic payload");
            fs::write(path, bytes).expect("bytes are written");
        }
    }

    /// The designed installation fingerprint the test mounts against.
    fn base_fingerprint() -> ContentHash {
        ContentHash::from_bytes([7u8; 32])
    }

    /// The designed rules hash the handshake also compares.
    fn rules() -> ContentHash {
        ContentHash::from_bytes([9u8; 32])
    }

    /// One manifest under its own shipped root, enabled.
    fn enabled(manifest: &ModManifest) -> (ModSelection, Temp) {
        let root = Temp::new(manifest.id().as_str());
        write_mod(&root, manifest);
        let mut selection = ModSelection::new();
        selection
            .offer(manifest.clone(), root.path())
            .expect("offered");
        selection.enable(manifest.id()).expect("enabled");
        (selection, root)
    }

    /// Mounts a selection the way the host does.
    fn mount(selection: &ModSelection) -> MountedMods {
        selection
            .mount(&synthetic_mount_request(), base_fingerprint(), None)
            .expect("the fixture set mounts")
    }

    fn hello(compatibility: &Compatibility) -> ClientHello {
        ClientHello {
            protocol: PROTOCOL_VERSION,
            compatibility: compatibility.clone(),
        }
    }

    fn parameters(compatibility: &Compatibility) -> SessionParameters {
        SessionParameters {
            compatibility: compatibility.clone(),
        }
    }

    // ------------------------------------------------ .zrd authoring helpers --

    /// A `.zrd` float node: the measured tag word and the bit pattern.
    fn zrd_float(value: f32) -> Vec<u8> {
        let mut bytes = ZRD_TAG_FLOAT.to_le_bytes().to_vec();
        bytes.extend_from_slice(&value.to_bits().to_le_bytes());
        bytes
    }

    /// A `.zrd` text node: the measured tag word, the byte length and the bytes.
    fn zrd_text(text: &str) -> Vec<u8> {
        let mut bytes = ZRD_TAG_TEXT.to_le_bytes().to_vec();
        bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
        bytes.extend_from_slice(text.as_bytes());
        bytes
    }

    /// A `.zrd` list node: the measured tag word, **`children.len() + 1`** as
    /// the count (the `count - 1` grammar `decode_zrd` reads), then the
    /// children.
    fn zrd_list(children: Vec<Vec<u8>>) -> Vec<u8> {
        let mut bytes = ZRD_TAG_LIST.to_le_bytes().to_vec();
        bytes.extend_from_slice(&((children.len() as u32) + 1).to_le_bytes());
        for child in children {
            bytes.extend_from_slice(&child);
        }
        bytes
    }

    /// A flat alternating key/value `.zrd` record, the shape `objectives.zrd`
    /// spells.
    fn zrd_flat(entries: Vec<(&str, Vec<u8>)>) -> Vec<u8> {
        let mut children = Vec::with_capacity(entries.len() * 2);
        for (key, value) in entries {
            children.push(zrd_text(key));
            children.push(value);
        }
        zrd_list(children)
    }

    /// One authored directive's children, as its block holds them: the key,
    /// and — unless it is authored bare — the argument-list node beside it.
    /// A block is the flat concatenation of these, which is exactly the shape
    /// the measured directive grammar reads.
    fn directive(key: &str, args: Option<Vec<u8>>) -> Vec<Vec<u8>> {
        let mut children = vec![zrd_text(key)];
        if let Some(args) = args {
            children.push(args);
        }
        children
    }

    /// One authored `OBJECTIVE1` block: the key, and the list whose children
    /// are its directives' children in order.
    fn block(directives: Vec<Vec<Vec<u8>>>) -> (&'static str, Vec<u8>) {
        let children: Vec<Vec<u8>> = directives.into_iter().flatten().collect();
        ("OBJECTIVE1", zrd_list(children))
    }

    /// A wrapped control record: the one-element list around the flat record,
    /// exactly the shape the production reader unwraps with
    /// `cs_content::stunts::objective_record`.
    fn control_document(fields: Vec<(&str, Vec<u8>)>) -> Vec<u8> {
        zrd_list(vec![zrd_flat(fields)])
    }

    /// A payload that lowers and validates: one numbered block whose directives
    /// are all measured or terminal keys, so every site binds and the block's
    /// condition lowers to its measured gate.
    fn valid_payload() -> Vec<u8> {
        control_document(vec![block(vec![
            directive("BEGIN_DORMANT", Some(zrd_list(vec![zrd_float(-1.0)]))),
            directive("WAKE_ANIM", Some(zrd_list(vec![zrd_text("wv_hookup")]))),
            directive("INSTANTWIN", None),
        ])])
    }

    /// A payload that decodes and measures but whose block cannot predicate: a
    /// scalar where the block's directive list belongs leaves the block
    /// unreadable, so its condition is `Condition::Unknown` and
    /// `MissionProgram::validate` refuses it.
    fn invalid_condition_payload() -> Vec<u8> {
        control_document(vec![("OBJECTIVE1", zrd_text("not a directive list"))])
    }

    /// A payload whose block spells a key nobody measured: the call binds
    /// nothing, so no program is assembled at all.
    fn unmeasured_directive_payload() -> Vec<u8> {
        control_document(vec![block(vec![
            directive("WAKE_ANIM", Some(zrd_list(vec![zrd_text("x")]))),
            directive("SET_AI_", Some(zrd_list(vec![zrd_text("trouble")]))),
            directive("INSTANTWIN", None),
        ])])
    }

    /// Writes a payload where the mod ships it, as `write_mod` in F53-C's suite
    /// does for its own fixtures.
    fn write_payload(root: &Temp, source: &str, bytes: &[u8]) {
        let path = root.path().join(source);
        fs::create_dir_all(path.parent().expect("the source has a parent")).expect("dirs");
        fs::write(path, bytes).expect("the payload is written");
    }

    /// The F53-B mission fixture, whose single override target is a
    /// [`ContentKind::Mission`] id and whose payload path is
    /// `missions/training.mis`.
    fn mission_mod_root(payload: &[u8]) -> (Temp, ModManifest) {
        let mission = synthetic_mission_mod();
        let root = Temp::new("mission");
        write_payload(&root, "missions/training.mis", payload);
        (root, mission)
    }

    /// A mod that claims a sandboxed target this build has no measured payload
    /// encoding for: `ContentKind::Script`, whose retail bytes are the reader
    /// archive rather than a control record.
    fn script_target_mod() -> ModManifest {
        let header = ModHeader::try_new(
            ModId::new("synthetic.script-payload").expect("the mod id is valid"),
            "synthetic.script-payload",
            ModVersion::new(1, 0, 0),
            EngineRange::new(ModVersion::new(0, 1, 0), ModVersion::new(0, 9, 9))
                .expect("the synthetic engine range is valid"),
            false,
            Provenance::designed(synthetic_mod_claim()),
        )
        .expect("the synthetic mod header is valid");
        ModManifest::try_new(
            header,
            Vec::new(),
            vec![ModPayload::new("missions/training.mis").expect("valid payload")],
            vec![
                ContentOverride::try_new(
                    ContentId::from_source(ContentKind::Script, "synthetic.training")
                        .expect("the synthetic script id is valid"),
                    OverrideAction::Add,
                    "missions/training.mis",
                    512,
                )
                .expect("the synthetic script override is valid"),
            ],
        )
        .expect("the synthetic mod manifest is valid")
    }

    /// Mounts `set` against `root` under this host's own raw-path environment.
    fn host_environment(root: &Temp, mod_id: &ModId) -> MountEnvironment<'static> {
        mount_environment(base_fingerprint()).with_root(mod_id.clone(), root.path())
    }

    /// One enabled selection whose override ships `payload` at
    /// `missions/training.mis` — the F53-C producer [`mount_selection`]
    /// mounts through.
    fn sandboxed_selection(manifest: ModManifest, payload: &[u8]) -> (ModSelection, Temp) {
        let root = Temp::new(manifest.id().as_str());
        write_payload(&root, "missions/training.mis", payload);
        let mut selection = ModSelection::new();
        selection
            .offer(manifest.clone(), root.path())
            .expect("offered");
        selection.enable(manifest.id()).expect("enabled");
        (selection, root)
    }

    /// **The wiring, on the mount that hosts the mod.** [`ModSelection::mount`]
    /// is the F53-C producer path and takes the validator as a parameter;
    /// [`mount_selection`] is where this host answers it. A payload that
    /// decodes, lowers and validates mounts through it, the same selection
    /// with no validator is refused `unvalidated_program`, and a payload the
    /// bounded reader refuses is quoted back as a `program_rejected`. Passing
    /// `None` instead of the validator below fails this test.
    #[test]
    fn accept_f53_b_fu1_the_selection_mount_carries_the_validator() {
        let (selection, _root) = sandboxed_selection(synthetic_mission_mod(), &valid_payload());
        let mounted = mount_selection(&selection, &synthetic_mount_request(), base_fingerprint())
            .expect("a payload that validates mounts through the producer path");
        assert!(mounted.marks_sessions());

        let error = selection
            .mount(&synthetic_mount_request(), base_fingerprint(), None)
            .expect_err("the same selection with no validator refuses the sandboxed payload");
        assert_eq!(error.code(), "unvalidated_program");

        let (undecodable, _other_root) = sandboxed_selection(
            synthetic_mission_mod(),
            b"this is not a .zrd document at all",
        );
        let error = mount_selection(&undecodable, &synthetic_mount_request(), base_fingerprint())
            .expect_err("an undecodable payload cannot mount through the producer path");
        assert!(
            matches!(error, MountError::ProgramRejected { .. }),
            "the producer path quotes the validator's refusal: {error}"
        );
    }

    /// **The wiring, and the valid half of the scenario.** A payload that
    /// decodes, lowers and validates mounts through this host's environment,
    /// and the same bytes decode to a validated program whose mission identity
    /// is the target id. Without the validator [`mount_environment`] attaches,
    /// the very same mount is refused with `unvalidated_program` — so removing
    /// the wiring fails exactly this test.
    #[test]
    fn accept_f53_b_fu1_a_valid_sandboxed_mission_payload_mounts() {
        let (root, mission) = mission_mod_root(&valid_payload());
        let set = ModSet::new(vec![mission.clone()]);
        let target = ContentId::from_source(ContentKind::Mission, "synthetic.training")
            .expect("the synthetic mission id is valid");

        let mounted = mount_mods(
            &set,
            &synthetic_mount_request(),
            &host_environment(&root, mission.id()),
        )
        .expect("a payload that validates mounts");
        let payload = mounted
            .payload(&target)
            .expect("the mission payload the plan put in front of the validator mounts");
        assert_eq!(payload.validation(), OverrideValidation::SandboxedProgram);
        assert!(mounted.marks_sessions());
        assert_eq!(
            payload.size_bytes() as usize,
            valid_payload().len(),
            "the mounted payload is the bytes on disk"
        );

        // The same bytes, through the production decoder: the program is the
        // target's, not the payload's own say-so.
        let validated = decode_mission_program(&target, &valid_payload())
            .expect("the payload the mount accepted also decodes and validates");
        assert_eq!(validated.program().mission, target);
        assert_eq!(validated.program().objectives.len(), 1);

        // The contrast that makes this a wiring test: the same set under an
        // environment with no validator is refused, not silently mounted.
        let error = mount_mods(
            &set,
            &synthetic_mount_request(),
            &MountEnvironment::new(base_fingerprint()).with_root(mission.id().clone(), root.path()),
        )
        .expect_err("without the host wiring nothing sandboxed mounts");
        assert_eq!(error.code(), "unvalidated_program");
    }

    /// **Invalid payload, refused half of the scenario.** Bytes that are not a
    /// control record cannot reach the IR: the bounded reader refuses them and
    /// the mount quotes that refusal.
    #[test]
    fn accept_f53_b_fu1_an_undecodable_payload_is_refused() {
        let (root, mission) = mission_mod_root(b"this is not a .zrd document at all");
        let error = mount_mods(
            &ModSet::new(vec![mission.clone()]),
            &synthetic_mount_request(),
            &host_environment(&root, mission.id()),
        )
        .expect_err("bytes that do not decode cannot mount");
        match error {
            MountError::ProgramRejected { target, reason, .. } => {
                assert!(
                    target.as_str().ends_with("synthetic.training"),
                    "the refusal names the target: {target}"
                );
                assert!(
                    reason.contains(".zrd") && reason.contains("not a decodable"),
                    "the refusal quotes the bounded reader: {reason}"
                );
            }
            other => panic!("an undecodable payload is refused by the validator, got: {other}"),
        }
    }

    /// **A decodable document is not automatically a mission program.**
    /// Bytes that read as a well-formed `.zrd` document but declare no
    /// numbered `OBJECTIVE<N>` block carry no mission program at all, so the
    /// mount refuses them before any adapter sees them.
    #[test]
    fn accept_f53_b_fu1_a_payload_that_is_not_a_control_record_is_refused() {
        let (root, mission) = mission_mod_root(&control_document(vec![(
            "MISSION_NAME",
            zrd_text("training"),
        )]));
        let error = mount_mods(
            &ModSet::new(vec![mission.clone()]),
            &synthetic_mount_request(),
            &host_environment(&root, mission.id()),
        )
        .expect_err("a document with no numbered block cannot mount");
        match error {
            MountError::ProgramRejected { reason, .. } => {
                assert!(
                    reason.contains("no numbered OBJECTIVE block"),
                    "the refusal names what the payload is missing: {reason}"
                );
            }
            other => panic!("a non-control-record payload is refused, got: {other}"),
        }
    }

    /// **The validator is the gate, not a formality.** A payload that decodes
    /// and measures but whose program `MissionProgram::validate` refuses is
    /// refused *by that check*: dropping the `validate()` call from
    /// [`decode_mission_program`] lets it mount and fails this test.
    #[test]
    fn accept_f53_b_fu1_a_payload_that_fails_mission_program_validate_is_refused() {
        let (root, mission) = mission_mod_root(&invalid_condition_payload());
        let error = mount_mods(
            &ModSet::new(vec![mission.clone()]),
            &synthetic_mount_request(),
            &host_environment(&root, mission.id()),
        )
        .expect_err("a program that does not validate cannot mount");
        match error {
            MountError::ProgramRejected { reason, .. } => {
                assert!(
                    reason.contains("MissionProgram::validate"),
                    "the refusal is the bounded validator's: {reason}"
                );
            }
            other => panic!("an unvalidatable program is refused, got: {other}"),
        }
    }

    /// **Unknown vocabulary stays unknown.** A block spelling a directive key
    /// no finding measured binds no call, so there is no program to validate
    /// and the mount quotes the record's own lowering refusal — never a
    /// convenient operation.
    #[test]
    fn accept_f53_b_fu1_a_payload_with_an_unmeasured_directive_is_refused() {
        let (root, mission) = mission_mod_root(&unmeasured_directive_payload());
        let error = mount_mods(
            &ModSet::new(vec![mission.clone()]),
            &synthetic_mount_request(),
            &host_environment(&root, mission.id()),
        )
        .expect_err("a payload with an unmeasured directive cannot mount");
        match error {
            MountError::ProgramRejected { reason, .. } => {
                assert!(
                    reason.contains("SET_AI_"),
                    "the refusal names the key nobody measured: {reason}"
                );
            }
            other => panic!("an unmeasured directive is refused, got: {other}"),
        }
    }

    /// **No measured encoding, no mount.** The other eight sandboxed kinds
    /// keep the fail-closed behaviour F53-B left: the validator refuses them
    /// by name instead of guessing a format for them.
    #[test]
    fn accept_f53_b_fu1_a_sandboxed_target_without_a_measured_encoding_is_refused() {
        let script = script_target_mod();
        let root = Temp::new("script");
        write_payload(&root, "missions/training.mis", &valid_payload());
        let error = mount_mods(
            &ModSet::new(vec![script.clone()]),
            &synthetic_mount_request(),
            &host_environment(&root, script.id()),
        )
        .expect_err("a script-kind payload has no measured encoding and cannot mount");
        match error {
            MountError::ProgramRejected { target, reason, .. } => {
                assert_eq!(
                    target,
                    ContentId::from_source(ContentKind::Script, "synthetic.training")
                        .expect("the synthetic script id is valid")
                );
                assert!(
                    reason.contains("script") && reason.contains("no measured payload encoding"),
                    "the refusal names the kind and the reason: {reason}"
                );
            }
            other => panic!("an unsupported sandboxed kind is refused, got: {other}"),
        }
    }

    /// **F53 AC03, the acceptance scenario.** A stock lobby announces the
    /// base fingerprint; a session running a tuning mod announces the
    /// mount signature. The handshake refuses the pair in both directions
    /// with `ContentMismatch` naming both signatures, and admits either
    /// side against itself.
    #[test]
    fn accept_f53_c_a_tuning_mod_changes_the_lobby_signature_and_a_stock_lobby_refuses_it() {
        let tuning = synthetic_tuning_mod();
        let (selection, _root) = enabled(&tuning);
        let mounted = mount(&selection);
        let base = base_fingerprint();

        let stock = lobby_compatibility(rules(), base, None);
        let modded = lobby_compatibility(rules(), base, Some(&mounted));
        assert_eq!(stock.content_sha256, base);
        assert_eq!(modded.content_sha256, mounted.signature());
        assert_ne!(stock.content_sha256, modded.content_sha256);

        // A modded client cannot join the stock lobby: the refusal names
        // both signatures, not a bare disconnect.
        let error = evaluate_hello(&parameters(&stock), &hello(&modded))
            .expect_err("a tuning mod cannot join a stock lobby");
        assert_eq!(
            error,
            HandshakeReject::ContentMismatch {
                expected: base,
                offered: mounted.signature()
            },
            "the refusal identifies the incompatibility"
        );
        assert!(
            error.to_string().contains("content mismatch"),
            "the reason is readable: {error}"
        );

        // Nor can a stock client join the modded lobby.
        let error = evaluate_hello(&parameters(&modded), &hello(&stock))
            .expect_err("a stock client cannot join a modded lobby");
        assert_eq!(
            error,
            HandshakeReject::ContentMismatch {
                expected: mounted.signature(),
                offered: base
            }
        );

        // Identical sets admit each other.
        evaluate_hello(&parameters(&modded), &hello(&modded)).expect("modded admits modded");
        evaluate_hello(&parameters(&stock), &hello(&stock)).expect("stock admits stock");
    }

    /// **Projection.** The view carries the selection's rows with the
    /// mount's verdict — positions, the modification class, the announced
    /// signature and the marking notice — and surfaces a refused mount as
    /// its own code plus every plan problem, verbatim.
    #[test]
    fn accept_f53_c_the_view_projects_rows_refusals_and_the_marking_notice() {
        let (repaint, bright) = synthetic_conflicting_mods();
        let tuning = synthetic_tuning_mod();
        let mut selection = ModSelection::new();
        let mut roots = Vec::new();
        for manifest in [&repaint, &bright, &tuning] {
            let root = Temp::new(manifest.id().as_str());
            write_mod(&root, manifest);
            selection
                .offer(manifest.clone(), root.path())
                .expect("offered");
            selection.enable(manifest.id()).expect("enabled");
            roots.push(root);
        }
        let mounted = mount(&selection);

        let view = ModsView::project(&selection, Some(Ok(&mounted)));
        assert_eq!(view.rows().len(), 3);
        let tuning_row = view
            .rows()
            .iter()
            .find(|row| row.id == *tuning.id())
            .expect("the tuning mod has a row");
        assert!(tuning_row.enabled);
        assert_eq!(
            tuning_row.load_position,
            Some(
                mounted
                    .plan()
                    .mod_entry(tuning.id())
                    .expect("planned")
                    .position()
            )
        );
        assert_eq!(tuning_row.modification, Some(ModModification::Gameplay));
        // Rows that mounted but had no gameplay overrides stay cosmetic.
        let bright_row = view
            .rows()
            .iter()
            .find(|row| row.id == *bright.id())
            .expect("a row");
        assert_eq!(bright_row.modification, Some(ModModification::CosmeticOnly));
        assert!(
            view.notices().contains(&ModsNotice::SessionMarked),
            "the marking verdict is on screen"
        );
        assert_eq!(view.signature(), Some(mounted.signature()));
        // The wire record and the view agree about what is announced.
        assert_eq!(
            lobby_compatibility(rules(), base_fingerprint(), Some(&mounted)).content_sha256,
            view.signature().expect("announced")
        );
        assert!(
            view.mount_report()
                .is_some_and(|report| report.contains(mounted.signature().to_hex().as_str())),
            "the diagnostics pane holds the full report"
        );

        // A refused mount projects the refusal and every plan problem.
        let mut broken = ModSelection::new();
        broken
            .offer(repaint.clone(), Temp::new("unused").path())
            .expect("offered");
        broken.enable(repaint.id()).expect("enabled");
        let error = broken
            .mount(&synthetic_mount_request(), base_fingerprint(), None)
            .expect_err("a required dependency missing from the set is refused");
        let refused = ModsView::project(&broken, Some(Err(&error)));
        assert!(refused.signature().is_none());
        assert!(refused.notices().iter().any(|notice| matches!(
            notice,
            ModsNotice::MountRefused {
                code: "plan_refused",
                ..
            }
        )));
        assert!(
            refused.notices().iter().any(|notice| matches!(
                notice,
                ModsNotice::PlanProblem {
                    code: "missing_dependency",
                    ..
                }
            )),
            "the plan's own problem codes surface: {:?}",
            refused.notices()
        );
        assert!(
            refused
                .notices()
                .iter()
                .all(|notice| !notice.fallback_text().is_empty())
        );

        // Before any mount, the rows still say what is enabled.
        let pending = ModsView::project(&selection, None);
        assert!(pending.notices().is_empty());
        assert!(pending.rows().iter().all(|row| row.enabled));
        assert!(pending.rows().iter().all(|row| row.load_position.is_none()));
        assert_eq!(pending.signature(), None);
    }
}
