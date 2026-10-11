//! What the original's `CameraSetHorizon`/`CameraSetHorizonXZ` commands do,
//! measured (task #541, an F21-D follow-up).
//!
//! Spec: `specs/F21-cameras-cockpit-views-and-spyglass.md`, stage
//! `### F21-D`. Shared contract: `docs/contracts/UI-NETWORK.md`.
//!
//! F21-D censused the two commands from the loading scripts and could only
//! classify them from their spellings: `CameraSetHorizon horizon` *looked*
//! like it levels the camera's up axis, so the coverage claims recorded a
//! `LevelsHorizon`/`LevelsHorizonToZone` effect and an `Unconsumed` verdict —
//! both marked project classification, not measurement.
//!
//! Task #541's bounded static analysis of the owner-supplied decrypted
//! executable (`crimson.decrypted.exe`, SHA-256
//! `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`, the
//! same image F16-E's landmarks were measured in) established the mechanism,
//! and it is **not** an up-axis lock:
//!
//! * The command dispatch chain (`0x5b85e3`…, `.text`) matches the stored
//!   names with `strnicmp`, checking `CameraSetHorizonXZ` (18 bytes)
//!   **before** `CameraSetHorizon` (16 bytes) — the only order that works,
//!   since a 16-byte prefix match would otherwise swallow the longer name.
//!   Each handler takes the next script argument and resolves it through the
//!   global name registry (`0x4d0280`, mode 7 — the same pool `FindNode`
//!   binds from), producing a **scene-node pointer**, not a camera axis.
//! * The handler then calls a `Camera.c` routine that stores that node on
//!   the camera object: `0x4d2ec0` writes camera data `+0x8` (the
//!   `horizon` field the camera dump prints as `horizon = %s`) and
//!   `0x4d2f40` writes `+0xc` (`horizonXZ`). Both refuse a null node and a
//!   camera of another class with their own assert strings.
//! * Every render pass, `0x4d6390` runs *after* the camera's world matrix
//!   is computed (`0x4d2fc0`) and *before* camera→render setup
//!   (`0x5409b0`), from the same three call sites each time. It reads the
//!   current camera's world translation (`camdata+0x2c/+0x30/+0x34`) and
//!   writes it into the bound nodes' local translations through
//!   `0x4d1d50` — the routine that sets a node's translate and marks its
//!   subtree dirty — then runs the scene-graph update passes (`0x4d0210`)
//!   so the moved subtrees draw in their new places.
//! * For the `+0xc` node the write is lateral only: `0x4d1cc0` reads the
//!   bound node's **own** stored Y first, and the translate it writes is
//!   `(camera.x, node.y, camera.z)` — the node slides under the camera at
//!   whatever altitude the content gave it. For the `+0x8` node the write
//!   is all three axes verbatim.
//!
//! The direction is therefore **camera → scenery**: the named nodes are
//! world scenery — `horizon` sits under `world1` in every world archive
//! with the zone-sky containers (`zone1`/`zone2`/`zone3`, the `moon`,
//! `stars` and `sun` nodes and the `h_zone*scroll` cloud layers) parented
//! beneath it — and each pass re-anchors them to where the camera is. The
//! camera's own rotation, basis and up axis are never read and never
//! written. F21-D's "levels the camera's up axis" classification is
//! falsified by the bytes, and this module replaces it with the measured
//! mechanism.
//!
//! # What this module models, and what it does not
//!
//! [`HorizonBinding`] is the bound state the command installs: which named
//! node, under which rule. [`HorizonBinding::anchored_translation`] is the
//! measured per-pass write expressed as a value: given the camera's world
//! position and the bound node's current stored translation, the
//! translation the pass leaves behind. The model deliberately carries no
//! scene graph: resolving the name to a live node and marking its subtree
//! dirty are the world side's business, exactly as `CameraSetWorld`'s
//! consumer is the rig even though the world is elsewhere.
//!
//! What stays unknown is recorded, not filled in: `zone2_cloud_floor` and
//! `moon_reflection` are resolved by the original's runtime name registry
//! but are not stored names in any `gamez.zbd` node array of this
//! installation — where the original instantiates them is not established
//! here. And none of this is runtime observation: it is byte inspection of
//! the owner-supplied image, the same evidence class as F16-E's landmarks,
//! never `VerifiedOriginal`.
//!
//! The measured write-up is
//! `docs/findings/2026-10-11-t541-horizon-commands-anchor-scenery-to-camera.md`.

use std::fmt;

use cs_content::coordinates::EngineImage;
use cs_types::asset_id::SourceSpan;
use cs_types::content::{Origin, Provenance};
use cs_types::evidence::ContentHash;
use cs_types::space::WorldPosition;

use super::coverage::{MAX_DECLARED_NAME, ViewControlCensus};

/// Which translation the per-pass application writes into a bound node.
///
/// The two variants are the two slots the camera's own data holds — `+0x8`
/// written by the `CameraSetHorizon` handler and `+0xc` written by
/// `CameraSetHorizonXZ` — so they are measured facts about the image, not
/// a vocabulary invented for the classification enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum HorizonAnchorRule {
    /// `CameraSetHorizon <node>`: the node's stored translation becomes the
    /// camera's world position on every axis (`0x4d6390` → `0x4d1d50` with
    /// the camera's `+0x2c/+0x30/+0x34` verbatim).
    AllAxes,
    /// `CameraSetHorizonXZ <node>`: the node's X and Z become the camera's;
    /// its own stored Y is kept (`0x4d6390` reads the node's Y through
    /// `0x4d1cc0` and passes it back into the same `0x4d1d50` write).
    KeepOwnHeight,
}

impl HorizonAnchorRule {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::AllAxes => "all_axes",
            Self::KeepOwnHeight => "keep_own_height",
        }
    }

    /// The rule a measured command installs, or `None` for anything else.
    ///
    /// Compared the way the original's own dispatch compares them —
    /// `strnicmp`, case-insensitively — and on the **whole** stored name,
    /// so `CameraSetHorizonXZ` can never be mistaken for the 16-byte
    /// `CameraSetHorizon` that prefixes it.
    #[must_use]
    pub fn for_command(command: &str) -> Option<Self> {
        if command.eq_ignore_ascii_case("CameraSetHorizonXZ") {
            Some(Self::KeepOwnHeight)
        } else if command.eq_ignore_ascii_case("CameraSetHorizon") {
            Some(Self::AllAxes)
        } else {
            None
        }
    }

    /// The translation the bound node carries after one application.
    ///
    /// `camera` is the camera's world position (the `camdata+0x2c..+0x34`
    /// triple the apply routine reads) and `node` is the bound node's
    /// stored translation as it stands before the pass. The returned value
    /// is what `0x4d1d50` writes: the camera's position verbatim under
    /// [`Self::AllAxes`], or the camera's X and Z over the node's own Y
    /// under [`Self::KeepOwnHeight`].
    #[must_use]
    pub fn anchored_translation(self, camera: WorldPosition, node: WorldPosition) -> WorldPosition {
        match self {
            Self::AllAxes => camera,
            Self::KeepOwnHeight => WorldPosition::try_new([camera.x(), node.y(), camera.z()])
                .expect("two finite components from each validated position compose a finite one"),
        }
    }
}

impl fmt::Display for HorizonAnchorRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// One named scenery node a camera script bound to the camera: the state
/// the command's handler installs and every render pass applies.
///
/// `node` is the stored argument — `horizon`, `zone2_cloud_floor`,
/// `moon_reflection` — the name the runtime registry resolves a scene node
/// from. `origin` and `provenance` say where the declaration was measured:
/// an `Origin::Installation` binding came from bytes of a real loading
/// script and carries its claim provenance; an authored fixture says so.
/// Nothing in the record touches a camera's rotation: the measured write
/// is scenery ← camera, one direction only.
#[derive(Clone, Debug, PartialEq)]
pub struct HorizonBinding {
    node: String,
    rule: HorizonAnchorRule,
    origin: Origin,
    provenance: Provenance,
}

impl HorizonBinding {
    /// Builds a binding, validating the declared node name.
    ///
    /// `node` is bounded to [`MAX_DECLARED_NAME`], the same bound the
    /// coverage module puts on spellings echoed into reports and evidence.
    ///
    /// # Errors
    ///
    /// [`HorizonError::DeclaredName`] when `node` is empty or longer than
    /// the bound.
    pub fn try_new(
        node: impl Into<String>,
        rule: HorizonAnchorRule,
        origin: Origin,
        provenance: Provenance,
    ) -> Result<Self, HorizonError> {
        let node = node.into();
        if node.is_empty() || node.len() > MAX_DECLARED_NAME {
            return Err(HorizonError::DeclaredName {
                field: "node",
                len: node.len(),
            });
        }
        Ok(Self {
            node,
            rule,
            origin,
            provenance,
        })
    }

    /// Builds a binding from the command that declared it, deriving the
    /// rule the way the native dispatch does.
    ///
    /// # Errors
    ///
    /// [`HorizonError::NotAHorizonCommand`] when `command` is neither
    /// measured command, and [`HorizonError::DeclaredName`] as
    /// [`Self::try_new`].
    pub fn for_command(
        command: &str,
        node: impl Into<String>,
        origin: Origin,
        provenance: Provenance,
    ) -> Result<Self, HorizonError> {
        let rule = HorizonAnchorRule::for_command(command).ok_or_else(|| {
            HorizonError::NotAHorizonCommand {
                command: command.to_owned(),
            }
        })?;
        Self::try_new(node, rule, origin, provenance)
    }

    /// The bound node's name, as the declaration stores it.
    #[must_use]
    pub fn node(&self) -> &str {
        &self.node
    }

    /// Which translation rule the application applies.
    #[must_use]
    pub const fn rule(&self) -> HorizonAnchorRule {
        self.rule
    }

    /// Where the declaration was measured.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The claim provenance the declaration carries.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// The translation the bound node carries after one application:
    /// [`HorizonAnchorRule::anchored_translation`] under this binding's rule.
    #[must_use]
    pub fn anchored_translation(
        &self,
        camera: WorldPosition,
        node: WorldPosition,
    ) -> WorldPosition {
        self.rule.anchored_translation(camera, node)
    }
}

/// The bindings a measured view-control census declares.
///
/// One [`HorizonBinding`] per occurrence of either horizon command, in row
/// order then container order — every `CameraSetHorizon`/`CameraSetHorizonXZ`
/// line the census measured becomes the state its handler installs, with
/// the occurrence's own provenance carried over. `origin` is the census's
/// own: every occurrence came from one container, so a census measured over
/// `ZBD/interp.zbd` yields [`Origin::Installation`] bindings and an
/// authored container yields [`Origin::SyntheticFixture`].
///
/// # Errors
///
/// [`HorizonError`] when a measured occurrence carries no node argument, or
/// carries a spelling [`HorizonAnchorRule::for_command`] does not cover —
/// both shapes the census's own arity check already excludes, so a failure
/// here means the rows do not say what the census reported.
pub fn horizon_bindings(
    census: &ViewControlCensus,
    origin: &Origin,
) -> Result<Vec<HorizonBinding>, HorizonError> {
    census
        .all_occurrences()
        .filter(|occurrence| HorizonAnchorRule::for_command(occurrence.command()).is_some())
        .map(|occurrence| {
            HorizonBinding::for_command(
                occurrence.command(),
                occurrence
                    .arguments()
                    .first()
                    .ok_or_else(|| HorizonError::MissingNode {
                        command: occurrence.command().to_owned(),
                    })?,
                origin.clone(),
                occurrence.provenance().clone(),
            )
        })
        .collect()
}

/// Why a horizon binding or the mechanism's image evidence was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum HorizonError {
    /// A declared node name was empty or longer than [`MAX_DECLARED_NAME`].
    DeclaredName {
        /// Which field.
        field: &'static str,
        /// Its length in bytes.
        len: usize,
    },
    /// The command is neither `CameraSetHorizon` nor `CameraSetHorizonXZ`.
    NotAHorizonCommand {
        /// The spelling that was checked.
        command: String,
    },
    /// An occurrence carried no node argument to bind.
    MissingNode {
        /// The command whose argument was missing.
        command: String,
    },
}

impl fmt::Display for HorizonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeclaredName { field, len } => write!(
                f,
                "a declared {field} is non-empty and at most {MAX_DECLARED_NAME} bytes, got {len}"
            ),
            Self::NotAHorizonCommand { command } => write!(
                f,
                "{command} is not one of the two measured horizon commands"
            ),
            Self::MissingNode { command } => {
                write!(f, "a {command} occurrence carried no node name to bind")
            }
        }
    }
}

impl std::error::Error for HorizonError {}

// ------------------------------------------------- the measured image -----

/// The image the mechanism was measured in: the owner-supplied decrypted
/// executable `$CS_ENGINE_IMAGE` names, whose digest is
/// [`cs_content::coordinates::ORIGINAL_IMAGE_SHA256`].
pub const HORIZON_IMAGE: &str = "crimson.decrypted.exe";

/// File offset of the stored `CameraSetHorizon` command name (`.data`, VA
/// `0x63f520`), and its length including the terminator.
pub const HORIZON_COMMAND_OFFSET: u64 = 0x23_f520;
pub const HORIZON_COMMAND_LENGTH: u64 = 17;

/// File offset of `CameraSetHorizonXZ` (`.data`, VA `0x63f534`) with its
/// terminator.
pub const HORIZON_XZ_COMMAND_OFFSET: u64 = 0x23_f534;
pub const HORIZON_XZ_COMMAND_LENGTH: u64 = 19;

/// The `Camera.c` routine the `CameraSetHorizon` handler calls (`.text`,
/// VA `0x4d2ec0`..`0x4d2f40`): asserts the resolved node and the camera's
/// class, then stores the node at camera data `+0x8` (`mov [ecx+8], eax`
/// at VA `0x4d2f2e`).
pub const HORIZON_BIND_OFFSET: u64 = 0x0d_2ec0;
pub const HORIZON_BIND_LENGTH: u64 = 0x80;

/// The `CameraSetHorizonXZ` routine (`.text`, VA `0x4d2f40`..`0x4d2fc0`):
/// the same shape, storing at `+0xc` (`mov [ecx+0xc], eax` at VA
/// `0x4d2fae`).
pub const HORIZON_XZ_BIND_OFFSET: u64 = 0x0d_2f40;
pub const HORIZON_XZ_BIND_LENGTH: u64 = 0x80;

/// The per-render application (`.text`, VA `0x4d6390`..`0x4d6413`): reads
/// the bound nodes at `+0x8`/`+0xc` and the camera world translation at
/// `+0x2c..+0x34`, writes through `0x4d1d50`, then calls the scene-graph
/// update `0x4d0210`. This is the measured write itself.
pub const HORIZON_APPLY_OFFSET: u64 = 0x0d_6390;
pub const HORIZON_APPLY_LENGTH: u64 = 0x83;

/// The native landmarks the mechanism was measured against, as spans over
/// the owner-supplied decrypted image.
///
/// Built by [`mechanism_source`], which refuses an image that is not the
/// measured one — the same idiom `cs_app::mission_start::engine_state_source`
/// uses for its own static-analysis spans.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HorizonMechanism {
    /// The `CameraSetHorizon` name the dispatch chain matches.
    pub command: SourceSpan,
    /// The `CameraSetHorizonXZ` name, checked first because the shorter
    /// name prefixes it.
    pub command_xz: SourceSpan,
    /// The routine storing a resolved node at camera data `+0x8`.
    pub bind: SourceSpan,
    /// The routine storing a resolved node at `+0xc`.
    pub bind_xz: SourceSpan,
    /// The per-pass application: the write this module models.
    pub apply: SourceSpan,
}

/// Why the mechanism's image evidence could not be built.
#[derive(Clone, Debug, PartialEq)]
pub enum HorizonMechanismError {
    /// The image supplied is not the measured one: evidence about other
    /// bytes cannot stand behind this claim.
    DigestMismatch {
        /// What the supplied image hashed to.
        found: ContentHash,
    },
}

impl fmt::Display for HorizonMechanismError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DigestMismatch { found } => write!(
                f,
                "the supplied image hashes to {found}, not to the measured image, so its spans \
                 cannot back the mechanism claim"
            ),
        }
    }
}

impl std::error::Error for HorizonMechanismError {}

/// Builds the mechanism's [`SourceSpan`]s over the measured image.
///
/// The spans name [`HORIZON_IMAGE`]; their `install_sha256` is the image's
/// own digest, because the image — not the installation inventory — is the
/// source this claim stands on.
///
/// # Errors
///
/// [`HorizonMechanismError::DigestMismatch`] when `image` is not the
/// measured image.
pub fn mechanism_source(image: &EngineImage) -> Result<HorizonMechanism, HorizonMechanismError> {
    let expected = cs_content::coordinates::original_image_digest();
    if image.digest != expected {
        return Err(HorizonMechanismError::DigestMismatch {
            found: image.digest,
        });
    }
    let span = |offset: u64, length: u64| {
        SourceSpan::new(expected, HORIZON_IMAGE, None, offset, length, None)
            .expect("a span over the measured image's bytes is valid")
    };
    Ok(HorizonMechanism {
        command: span(HORIZON_COMMAND_OFFSET, HORIZON_COMMAND_LENGTH),
        command_xz: span(HORIZON_XZ_COMMAND_OFFSET, HORIZON_XZ_COMMAND_LENGTH),
        bind: span(HORIZON_BIND_OFFSET, HORIZON_BIND_LENGTH),
        bind_xz: span(HORIZON_XZ_BIND_OFFSET, HORIZON_XZ_BIND_LENGTH),
        apply: span(HORIZON_APPLY_OFFSET, HORIZON_APPLY_LENGTH),
    })
}
