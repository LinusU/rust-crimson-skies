//! The cockpit eye binding: where the original puts the pilot's eye inside an
//! aircraft, read off the owner-supplied decrypted executable and the airframe
//! archive's `cockpit_camera` nodes (task #542, the F21-D follow-up).
//!
//! F21-D's binding audit reported the pilot's eye
//! [`CockpitEyeCoverage::Undeclared`](super::coverage::CockpitEyeCoverage) —
//! the twenty cockpit bindings the loading scripts declare name instruments
//! and gauges, never a viewpoint, and no readable file the audit then opened
//! declared one. The follow-up's bounded static analysis of the
//! owner-supplied image (`crimson.decrypted.exe`,
//! [`cs_content::coordinates::ORIGINAL_IMAGE_SHA256`]) found the declaration
//! in the airframe archive itself: the original looks the eye up by **node
//! name**, not by script binding.
//!
//! The mechanism, measured end to end (VAs, `.text` unless noted):
//!
//! 1. `0x4d8cf0` is the engine's recursive child-by-name search: a byte-exact
//!    (case-sensitive) name compare, then recursion over the child array
//!    **last-to-first**, so the last stored child with a matching name wins.
//! 2. `0x473480` is the per-vehicle registration: it resolves `healthy`,
//!    `dontmove`, `cockpit1`, `geometry`, `markers` — and `cockpit_camera`
//!    (`.rdata` `0x62747c`) — from the vehicle root, and when the
//!    `cockpit_camera` node resolves it calls `0x4d1cc0`, which reads the
//!    node's **stored local translation** (the `+0x54`/`+0x58`/`+0x5c` triple
//!    of its object3d record) into the eye-offset globals
//!    `0x71c328..0x71c330`. A null vehicle restores the triple from the
//!    defaults at `0x75d1b8..0x75d1c0`. `0x4d1cc0` is the same local-translate
//!    reader [`horizon`]'s measured apply path reads node translations
//!    through — two mechanism claims pin the same reader.
//! 3. The camera dispatcher at `0x42c5c0` loads the camera-mode index
//!    (`[cam + 0x14c]`), bounds it to ten entries, and jumps through the
//!    table at `0x42c648`; entries **6 and 7** reach the `0x42c61d` thunk →
//!    `0x42d980`, which transforms the registered eye triple by the
//!    aircraft's world matrix (`eye_world = M·eye_local + t`) and feeds it to
//!    the camera position setters — the cockpit-mode camera placement.
//! 4. The script-variable setter chain's `cocky`/`cockz` branches
//!    (`0x43e3be..0x43e442`, names at `.rdata` `0x622ebc`/`0x622ec4`) overwrite
//!    the **second and third** eye-global components from script values — an
//!    authored override channel that keeps "the stored translation is the
//!    eye" true only when no script wrote it. No `cockx` name sits beside
//!    them: the first component has no script override in this image.
//!
//! What that means for the archive: the pilot's eye for an airframe is the
//! stored local translation of the `cockpit_camera` node inside that
//! airframe's own subtree — exactly what
//! [`resolve_cockpit_eye`](super::coverage::resolve_cockpit_eye) measures.
//! The registration reads the *local* slot directly, so the answer does not
//! depend on ancestor transforms; in the retail corpus every `cockpit_camera`
//! is an `object3d` marker child of `markers` under `geometry`.
//!
//! What is **not** claimed: which world axes the three stored components are
//! (the archive stores a source-unit triple; the axis map to this project's
//! right/up/forward body frame is a separate measurement); what the two
//! dispatched camera modes are called in the original UI; and what a script
//! wrote when `cocky`/`cockz` fired — the channel is measured, its values
//! are runtime state.
//!
//! Spec: `specs/F21-cameras-cockpit-views-and-spyglass.md`, the `### F21-D`
//! follow-up task #542. The measurement and its limits are written down in
//! `docs/findings/2026-10-11-t542-cockpit-camera-eye-placement.md`.

use std::fmt;

use cs_content::cameras::CockpitBindingSource;
use cs_content::coordinates::EngineImage;
use cs_types::asset_id::SourceSpan;
use cs_types::evidence::ContentHash;

use super::coverage::{COCKPIT_EYE_NODE, CockpitEyeState};

// ------------------------------------------------- the measured image -----

/// The image the mechanism was measured in: the owner-supplied decrypted
/// executable `$CS_ENGINE_IMAGE` names, whose digest is
/// [`cs_content::coordinates::ORIGINAL_IMAGE_SHA256`].
pub const EYE_IMAGE: &str = "crimson.decrypted.exe";

/// File offset of the stored `cockpit_camera` node name (`.rdata`, VA
/// `0x62747c`), including its terminator.
pub const EYE_NAME_OFFSET: u64 = 0x22_747c;
pub const EYE_NAME_LENGTH: u64 = 0x0f;

/// The recursive child-by-name search (`.text`, VA `0x4d8cf0`..`0x4d8d6a`):
/// byte-exact name compare, then children recursed last-to-first.
pub const EYE_LOOKUP_OFFSET: u64 = 0x0d_8cf0;
pub const EYE_LOOKUP_LENGTH: u64 = 0x7a;

/// The per-vehicle registration (`.text`, VA `0x473480`..`0x47357a`):
/// resolves `cockpit_camera` under the vehicle root and, when it resolves,
/// reads the node's stored local translation into the eye-offset globals.
pub const EYE_REGISTER_OFFSET: u64 = 0x07_3480;
pub const EYE_REGISTER_LENGTH: u64 = 0xfa;

/// The object3d local-translate reader the registration calls (`.text`, VA
/// `0x4d1cc0`..`0x4d1d50`): the `+0x54`/`+0x58`/`+0x5c` triple of the node
/// record, the same routine `horizon`'s apply path reads translations
/// through.
pub const EYE_READ_OFFSET: u64 = 0x0d_1cc0;
pub const EYE_READ_LENGTH: u64 = 0x90;

/// The camera-mode dispatch head (`.text`, VA `0x42c5c0`..`0x42c5d7`): loads
/// the mode index at `[cam + 0x14c]`, bounds it to ten, and jumps through
/// [`EYE_TABLE_OFFSET`].
pub const EYE_DISPATCH_OFFSET: u64 = 0x02_c5c0;
pub const EYE_DISPATCH_LENGTH: u64 = 0x17;

/// The ten-entry dispatch table (`.text`, VA `0x42c648`..`0x42c670`): entries
/// 6 and 7 hold the [`EYE_THUNK_OFFSET`] trampoline that tail-calls
/// [`EYE_APPLY_OFFSET`].
pub const EYE_TABLE_OFFSET: u64 = 0x02_c648;
pub const EYE_TABLE_LENGTH: u64 = 0x28;

/// The dispatch thunk two table entries share (`.text`, VA
/// `0x42c61d`..`0x42c62b`): loads the argument and calls
/// [`EYE_APPLY_OFFSET`].
pub const EYE_THUNK_OFFSET: u64 = 0x02_c61d;
pub const EYE_THUNK_LENGTH: u64 = 0x0e;

/// The eye-transform head of the mode-6/7 camera update (`.text`, VA
/// `0x42d980`..`0x42da50`): the full affine `eye_world = M·eye + t` and the
/// setter calls it feeds. The function continues past the span with
/// bookkeeping the eye does not concern.
pub const EYE_APPLY_OFFSET: u64 = 0x02_d980;
pub const EYE_APPLY_LENGTH: u64 = 0xd0;

/// The `cocky`/`cockz` script-variable names (`.rdata`, VA
/// `0x622ebc`..`0x622eca`), terminators and padding included.
pub const EYE_OVERRIDE_NAMES_OFFSET: u64 = 0x22_2ebc;
pub const EYE_OVERRIDE_NAMES_LENGTH: u64 = 0x0e;

/// The compare-and-store branches the setter chain runs for `cocky`/`cockz`
/// (`.text`, VA `0x43e3be`..`0x43e442`): each writes one eye-global
/// component — the second and third — from a script value.
pub const EYE_OVERRIDE_OFFSET: u64 = 0x03_e3be;
pub const EYE_OVERRIDE_LENGTH: u64 = 0x84;

/// The native landmarks the mechanism was measured against, as spans over
/// the owner-supplied decrypted image.
///
/// Built by [`eye_mechanism_source`], which refuses an image that is not the
/// measured one — the same idiom [`horizon::mechanism_source`] uses for its
/// own static-analysis spans.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EyeMechanism {
    /// The `cockpit_camera` name the registration's lookup matches.
    pub name: SourceSpan,
    /// The recursive, byte-exact name search the registration calls.
    pub lookup: SourceSpan,
    /// The registration that installs the node's translation as the eye.
    pub register: SourceSpan,
    /// The local-translate reader it calls.
    pub read: SourceSpan,
    /// The camera-mode dispatch head that bounds and jumps through
    /// `dispatch_table`.
    pub dispatch: SourceSpan,
    /// The dispatch table whose entries 6 and 7 route to the eye transform.
    pub dispatch_table: SourceSpan,
    /// The thunk the two entries share, which tail-calls `apply`.
    pub thunk: SourceSpan,
    /// The eye transform and the setter calls it feeds.
    pub apply: SourceSpan,
    /// The `cocky`/`cockz` variable names.
    pub override_names: SourceSpan,
    /// The compare-and-store branches that write two eye components.
    pub override_: SourceSpan,
}

/// Why a cockpit eye or the mechanism's image evidence was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum CockpitEyeError {
    /// The eye state a binding was asked to carry is not
    /// [`CockpitEyeState::Declared`]: an `Absent`, `Ambiguous` or non-object
    /// node has no stored translation to install.
    NotDeclared,
    /// The image supplied is not the measured one: evidence about other
    /// bytes cannot stand behind this claim.
    DigestMismatch {
        /// What the supplied image hashed to.
        found: ContentHash,
    },
}

impl fmt::Display for CockpitEyeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotDeclared => write!(
                f,
                "only a declared {COCKPIT_EYE_NODE} node carries a cockpit eye"
            ),
            Self::DigestMismatch { found } => write!(
                f,
                "the supplied image hashes to {found}, not to the measured image, so its spans \
                 cannot back the mechanism claim"
            ),
        }
    }
}

impl std::error::Error for CockpitEyeError {}

/// Builds the mechanism's [`SourceSpan`]s over the measured image.
///
/// The spans name [`EYE_IMAGE`]; their `install_sha256` is the image's own
/// digest, because the image — not the installation inventory — is the
/// source this claim stands on.
///
/// # Errors
///
/// [`CockpitEyeError::DigestMismatch`] when `image` is not the measured
/// image.
pub fn eye_mechanism_source(image: &EngineImage) -> Result<EyeMechanism, CockpitEyeError> {
    let expected = cs_content::coordinates::original_image_digest();
    if image.digest != expected {
        return Err(CockpitEyeError::DigestMismatch {
            found: image.digest,
        });
    }
    let span = |offset: u64, length: u64| {
        SourceSpan::new(expected, EYE_IMAGE, None, offset, length, None)
            .expect("a span over the measured image's bytes is valid")
    };
    Ok(EyeMechanism {
        name: span(EYE_NAME_OFFSET, EYE_NAME_LENGTH),
        lookup: span(EYE_LOOKUP_OFFSET, EYE_LOOKUP_LENGTH),
        register: span(EYE_REGISTER_OFFSET, EYE_REGISTER_LENGTH),
        read: span(EYE_READ_OFFSET, EYE_READ_LENGTH),
        dispatch: span(EYE_DISPATCH_OFFSET, EYE_DISPATCH_LENGTH),
        dispatch_table: span(EYE_TABLE_OFFSET, EYE_TABLE_LENGTH),
        thunk: span(EYE_THUNK_OFFSET, EYE_THUNK_LENGTH),
        apply: span(EYE_APPLY_OFFSET, EYE_APPLY_LENGTH),
        override_names: span(EYE_OVERRIDE_NAMES_OFFSET, EYE_OVERRIDE_NAMES_LENGTH),
        override_: span(EYE_OVERRIDE_OFFSET, EYE_OVERRIDE_LENGTH),
    })
}

/// The cockpit-eye binding one airframe's `cockpit_camera` node installs: the
/// model node the original's own registration resolves, and the stored local
/// translation it reads into the eye globals.
///
/// A `CockpitViewpoint` built on this binding gets F21 non-negotiable
/// behavior 1's "verified model/config binding" for free — the source is the
/// very node the original resolves — while the *offset* it carries stays the
/// stored source-unit triple until a separate measurement fixes the axis map.
#[derive(Clone, Debug, PartialEq)]
pub struct CockpitEyeBinding {
    /// Always `ModelNode { node: "cockpit_camera" }`: the node the
    /// registration searches for under the vehicle root.
    source: CockpitBindingSource,
    /// The airframe archive's eye state this binding was built from.
    state: CockpitEyeState,
    /// The node's index in the airframe archive's node array.
    node_index: u32,
    /// The node's stored local translation — the eye offset in the archive's
    /// own source units, three unlabeled components.
    translation: [f32; 3],
}

impl CockpitEyeBinding {
    /// Binds one airframe's measured eye state.
    ///
    /// # Errors
    ///
    /// [`CockpitEyeError::NotDeclared`] when the state is not
    /// [`CockpitEyeState::Declared`] — a missing or ambiguous `cockpit_camera`
    /// is a finding, not a viewpoint to fall back on.
    pub fn try_new(state: &CockpitEyeState) -> Result<Self, CockpitEyeError> {
        let CockpitEyeState::Declared {
            node_index,
            translation,
        } = state
        else {
            return Err(CockpitEyeError::NotDeclared);
        };
        Ok(Self {
            source: CockpitBindingSource::ModelNode {
                node: COCKPIT_EYE_NODE.to_owned(),
            },
            state: state.clone(),
            node_index: *node_index,
            translation: *translation,
        })
    }

    /// The model node the binding resolves, for a `CockpitViewpoint::source`.
    #[must_use]
    pub const fn source(&self) -> &CockpitBindingSource {
        &self.source
    }

    /// The measured eye state the binding carries.
    #[must_use]
    pub const fn state(&self) -> &CockpitEyeState {
        &self.state
    }

    /// The node's index in the airframe archive's node array.
    #[must_use]
    pub const fn node_index(&self) -> u32 {
        self.node_index
    }

    /// The stored local translation the registration installs as the eye —
    /// the offset in the archive's own source units, three unlabeled
    /// components. Deliberately not a [`cs_content::cameras::BodyOffset`]:
    /// naming one component "up" is a coordinate-convention claim this
    /// measurement does not carry.
    #[must_use]
    pub const fn translation(&self) -> [f32; 3] {
        self.translation
    }
}
