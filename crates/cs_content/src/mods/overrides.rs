//! Content overrides and the two policies that classify them (F53-A).
//!
//! Spec: `specs/F53-mod-mounts-custom-content-and-compatibility-signatures.md`,
//! stage `### F53-A`.
//!
//! An override is one mod's claim about one piece of content: the stable
//! catalog id it targets, whether it *adds* a new id or *replaces* an
//! existing one, the mod-local file that supplies it and the byte count the
//! manifest declares for it. The mod id, version, dependencies and payload
//! list live on [`super::manifest::ModManifest`].
//!
//! # Two classifications, both derived and neither author-asserted
//!
//! F53 non-negotiable 3 says a "cosmetic-only" classification "requires a
//! hash-policy definition, not an author assertion alone", and
//! non-negotiable 2 says a sandboxed mission IR "uses the same bounded
//! validator as original adapters". Both are stated here as **total
//! functions from the target's [`ContentKind`]**, so the answer is a
//! property of the id being overridden and can never be a claim the mod
//! author chose:
//!
//! * [`classify_effect`] answers [`OverrideEffect::Cosmetic`] only for the
//!   presentation kinds that carry no simulation state and no rule the
//!   runtime reads. Every other kind is [`OverrideEffect::Gameplay`]. The
//!   set is deliberately small: a render [`ContentKind::Mesh`] is **not**
//!   cosmetic, because a mesh can be the source of a derived collider in
//!   this project (`docs/findings/2026-09-23-avian-collider-from-mesh-
//!   needs-bevy-asset-stack.md`), so a mesh override cannot be *proved*
//!   cosmetic. A false "cosmetic" would let a gameplay change escape the
//!   marking non-negotiable 3 demands; a false "gameplay" only costs
//!   caution.
//! * [`classify_validation`] answers [`OverrideValidation::SandboxedProgram`]
//!   for the kinds that carry executable or mission-program content, which
//!   F53-B must hand to the *same* bounded validator the original adapter
//!   uses (`cs_script::ir::MissionProgram::validate`). This stage only
//!   classifies which overrides need it; it does not run a validator.
//!
//! Both functions match every [`ContentKind`] variant, so adding a kind to
//! the vocabulary is a compile error here until the policies decide what it
//! means. That totality is the hash-policy definition: there is no
//! `Unknown` arm and no default that could quietly classify a new kind as
//! cosmetic.
//!
//! **Designed engine policy, not an original claim.** The original game's
//! own notion of a cosmetic change, and which of its content kinds may be
//! overridden at all, are unmeasured (F53 "Research boundary"; F53-D). The
//! sets below are this engine's declaration of what it will and will not
//! treat as non-gameplay.

use std::fmt;

use cs_types::content::{ContentId, ContentKind};
use cs_types::install::RelativePath;

use super::manifest::ManifestError;

/// Whether an override introduces a new content id or takes over an existing
/// one.
///
/// The two are different claims with different preconditions, and the mount
/// plan checks them differently ([`super::plan::PlanProblem`]): an [`Add`]
/// may not name an id the base content already provides, and a [`Replace`]
/// may not name an id nothing provides.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum OverrideAction {
    /// The mod introduces a new content id the base content does not have.
    Add,
    /// The mod takes over an id the base content already provides.
    Replace,
}

impl OverrideAction {
    /// The stable label used in reports and in the plan's JSON.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Replace => "replace",
        }
    }
}

impl fmt::Display for OverrideAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// What an override does to the simulation, as decided by
/// [`classify_effect`] and never by the manifest.
///
/// [`Cosmetic`](Self::Cosmetic) means the kind carries no simulation state;
/// [`Gameplay`](Self::Gameplay) means it does, so a mount that enables it
/// must mark its sessions, saves, replays and network handshakes (F53
/// non-negotiable 3). The *set* level answer is [`ModModification`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum OverrideEffect {
    /// The overridden kind cannot change the simulation.
    Cosmetic,
    /// The overridden kind can change the simulation.
    Gameplay,
}

impl OverrideEffect {
    /// The stable label used in reports and in the plan's JSON.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Cosmetic => "cosmetic",
            Self::Gameplay => "gameplay",
        }
    }

    /// Whether this effect leaves the simulation alone.
    #[must_use]
    pub const fn is_cosmetic(self) -> bool {
        matches!(self, Self::Cosmetic)
    }
}

impl fmt::Display for OverrideEffect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Which validator an override's payload has to pass, as decided by
/// [`classify_validation`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum OverrideValidation {
    /// Ordinary content: read through its own bounded reader.
    OriginalAdapter,
    /// Mission-program or script content. F53-B must run it through the
    /// *same* bounded validator the original adapter uses, so a mod can
    /// never reach a native binding the original data would not
    /// (F53 non-negotiable 2).
    SandboxedProgram,
}

impl OverrideValidation {
    /// The stable label used in reports and in the plan's JSON.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::OriginalAdapter => "original_adapter",
            Self::SandboxedProgram => "sandboxed_program",
        }
    }
}

impl fmt::Display for OverrideValidation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The content kinds this engine declares **cosmetic**: presentation data
/// with no simulation state and no rule the runtime reads.
///
/// A mod that overrides nothing else is a cosmetic-only mod, and its mount
/// need not mark sessions, saves, replays or handshakes for gameplay
/// reasons. Every kind outside this list is gameplay, including
/// [`ContentKind::Mesh`] (a mesh can feed a derived collider) and every
/// airframe, engine, gun, blueprint, mission, script and world kind.
pub const COSMETIC_CONTENT_KINDS: &[ContentKind] = &[
    ContentKind::Image,
    ContentKind::Material,
    ContentKind::PaintMask,
    ContentKind::AnimationTrack,
    ContentKind::CameraTrack,
    ContentKind::Sound,
    ContentKind::Music,
    ContentKind::Dialogue,
    ContentKind::Video,
    ContentKind::Font,
    ContentKind::StringResource,
    ContentKind::UiResource,
];

/// The content kinds whose payloads are mission programs or script content
/// and therefore have to pass the bounded sandbox validator.
pub const SANDBOXED_PROGRAM_CONTENT_KINDS: &[ContentKind] = &[
    ContentKind::Mission,
    ContentKind::IaScenario,
    ContentKind::MultiplayerScenario,
    ContentKind::Script,
    ContentKind::Instruction,
    ContentKind::NativeBinding,
    ContentKind::Objective,
    ContentKind::Trigger,
    ContentKind::Route,
];

/// Classifies what overriding `kind` does to the simulation.
///
/// This is the hash policy F53 non-negotiable 3 requires: the answer is
/// computed from the target kind, so a manifest cannot classify itself
/// cosmetic by asserting it. It is a total function over [`ContentKind::ALL`]
/// — there is no fall-through and no `Unknown` arm.
#[must_use]
pub const fn classify_effect(kind: ContentKind) -> OverrideEffect {
    match kind {
        ContentKind::Image
        | ContentKind::Material
        | ContentKind::PaintMask
        | ContentKind::AnimationTrack
        | ContentKind::CameraTrack
        | ContentKind::Sound
        | ContentKind::Music
        | ContentKind::Dialogue
        | ContentKind::Video
        | ContentKind::Font
        | ContentKind::StringResource
        | ContentKind::UiResource => OverrideEffect::Cosmetic,
        ContentKind::InstallFile
        | ContentKind::World
        | ContentKind::SceneNode
        | ContentKind::Mesh
        | ContentKind::CollisionSurface
        | ContentKind::Airframe
        | ContentKind::Engine
        | ContentKind::Armor
        | ContentKind::Gun
        | ContentKind::Ammo
        | ContentKind::HardpointEquipment
        | ContentKind::Blueprint
        | ContentKind::Pilot
        | ContentKind::Voice
        | ContentKind::Faction
        | ContentKind::Mission
        | ContentKind::Script
        | ContentKind::Instruction
        | ContentKind::NativeBinding
        | ContentKind::Objective
        | ContentKind::Trigger
        | ContentKind::Route
        | ContentKind::Stunt
        | ContentKind::ScrapbookItem
        | ContentKind::IaScenario
        | ContentKind::IaPreset
        | ContentKind::MultiplayerScenario
        | ContentKind::MultiplayerRules
        | ContentKind::CustomPlane
        | ContentKind::Loadout
        | ContentKind::Weapon => OverrideEffect::Gameplay,
    }
}

/// Classifies which validator an override of `kind` has to pass.
///
/// Total over [`ContentKind::ALL`] for the same reason as
/// [`classify_effect`]: a mod-supplied mission program is identifiable from
/// its id, not from anything the manifest says about it.
#[must_use]
pub const fn classify_validation(kind: ContentKind) -> OverrideValidation {
    match kind {
        ContentKind::Mission
        | ContentKind::IaScenario
        | ContentKind::MultiplayerScenario
        | ContentKind::Script
        | ContentKind::Instruction
        | ContentKind::NativeBinding
        | ContentKind::Objective
        | ContentKind::Trigger
        | ContentKind::Route => OverrideValidation::SandboxedProgram,
        ContentKind::InstallFile
        | ContentKind::World
        | ContentKind::SceneNode
        | ContentKind::Mesh
        | ContentKind::Material
        | ContentKind::CollisionSurface
        | ContentKind::Image
        | ContentKind::Airframe
        | ContentKind::Engine
        | ContentKind::Armor
        | ContentKind::Gun
        | ContentKind::Ammo
        | ContentKind::HardpointEquipment
        | ContentKind::Blueprint
        | ContentKind::PaintMask
        | ContentKind::Pilot
        | ContentKind::Voice
        | ContentKind::Faction
        | ContentKind::AnimationTrack
        | ContentKind::CameraTrack
        | ContentKind::Sound
        | ContentKind::Music
        | ContentKind::Dialogue
        | ContentKind::Video
        | ContentKind::Font
        | ContentKind::StringResource
        | ContentKind::UiResource
        | ContentKind::Stunt
        | ContentKind::ScrapbookItem
        | ContentKind::IaPreset
        | ContentKind::MultiplayerRules
        | ContentKind::CustomPlane
        | ContentKind::Loadout
        | ContentKind::Weapon => OverrideValidation::OriginalAdapter,
    }
}

/// Whether the mounted set as a whole changes the simulation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ModModification {
    /// Every enabled override is cosmetic by [`classify_effect`].
    CosmeticOnly,
    /// At least one enabled override is gameplay, so the mount marks its
    /// sessions, saves, replays and network handshakes (F53
    /// non-negotiable 3).
    Gameplay,
}

impl ModModification {
    /// The stable label used in reports and in the plan's JSON.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::CosmeticOnly => "cosmetic_only",
            Self::Gameplay => "gameplay",
        }
    }

    /// Whether the mount has to mark its sessions and handshakes for
    /// gameplay reasons.
    #[must_use]
    pub const fn marks_sessions(self) -> bool {
        matches!(self, Self::Gameplay)
    }
}

impl fmt::Display for ModModification {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// One mod's claim about one content id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentOverride {
    target: ContentId,
    action: OverrideAction,
    source: RelativePath,
    declared_bytes: u64,
}

impl ContentOverride {
    /// Validates and records one override from raw spellings.
    ///
    /// `source` is a spelling **relative to the mod's own root**; it is
    /// validated here (`..`, absolute spellings, drive prefixes, `.`, empty
    /// components and NUL bytes are all refused) and joined to the mod root
    /// only by the mount, which is F53-B. Nothing in this record is ever
    /// joined to a path before [`RelativePath::new`] has accepted it.
    ///
    /// `declared_bytes` is the manifest's *own* statement of the payload's
    /// size. It is unverified: no byte is read at this stage, so the budget
    /// verdicts built on it are declared-size verdicts
    /// (`docs/findings/2026-10-01-f53-a-mod-manifest-and-override-
    /// validation.md`).
    ///
    /// # Errors
    ///
    /// [`ManifestError::UnsafePath`] when `source` is not a safe relative
    /// spelling.
    pub fn try_new(
        target: ContentId,
        action: OverrideAction,
        source: &str,
        declared_bytes: u64,
    ) -> Result<Self, ManifestError> {
        let source = RelativePath::new(source).map_err(|error| ManifestError::UnsafePath {
            spelling: source.to_owned(),
            error,
        })?;
        Ok(Self {
            target,
            action,
            source,
            declared_bytes,
        })
    }

    /// The stable catalog id this override claims.
    pub fn target(&self) -> &ContentId {
        &self.target
    }

    /// Whether the override introduces or replaces its target.
    pub fn action(&self) -> OverrideAction {
        self.action
    }

    /// The mod-local source spelling, as the manifest wrote it.
    pub fn source(&self) -> &RelativePath {
        &self.source
    }

    /// The byte count the manifest declares for the payload.
    pub fn declared_bytes(&self) -> u64 {
        self.declared_bytes
    }

    /// What this override does to the simulation, computed from the target
    /// kind by [`classify_effect`].
    ///
    /// There is deliberately no setter and no manifest field for it: the
    /// author cannot assert the effect (F53 non-negotiable 3).
    pub fn effect(&self) -> OverrideEffect {
        classify_effect(self.target.kind())
    }

    /// Which validator this override's payload has to pass, computed from
    /// the target kind by [`classify_validation`].
    pub fn validation(&self) -> OverrideValidation {
        classify_validation(self.target.kind())
    }
}

impl fmt::Display for ContentOverride {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} via {}", self.action, self.target, self.source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cs_types::evidence::ClaimStatus;

    fn image(key: &str) -> ContentId {
        ContentId::from_source(ContentKind::Image, key).expect("valid image id")
    }

    fn airframe(key: &str) -> ContentId {
        ContentId::from_source(ContentKind::Airframe, key).expect("valid airframe id")
    }

    fn mission(key: &str) -> ContentId {
        ContentId::from_source(ContentKind::Mission, key).expect("valid mission id")
    }

    /// The effect policy is a **total** function over the content-kind
    /// vocabulary, and the cosmetic set is exactly the presentation kinds
    /// this engine declared. Both halves matter: the first means a new kind
    /// cannot fall through into a default, the second means the
    /// hash-policy definition F53 non-negotiable 3 demands is pinned by a
    /// test rather than by a comment.
    #[test]
    fn accept_f53_a_effect_policy_is_derived_from_the_target_kind() {
        for kind in ContentKind::ALL {
            let effect = classify_effect(*kind);
            assert_eq!(
                effect.is_cosmetic(),
                COSMETIC_CONTENT_KINDS.contains(kind),
                "{kind} classifies as {effect}, which disagrees with the declared cosmetic set"
            );
            // A second call is the same answer: the policy is a function of
            // the kind, never of a caller's state.
            assert_eq!(effect, classify_effect(*kind));
        }
        assert!(!classify_effect(ContentKind::Airframe).is_cosmetic());
        assert!(!classify_effect(ContentKind::Blueprint).is_cosmetic());
        assert!(!classify_effect(ContentKind::Gun).is_cosmetic());
        assert!(!classify_effect(ContentKind::Mission).is_cosmetic());
        assert!(!classify_effect(ContentKind::World).is_cosmetic());
        assert!(!classify_effect(ContentKind::CollisionSurface).is_cosmetic());
        assert!(classify_effect(ContentKind::Image).is_cosmetic());
        assert!(classify_effect(ContentKind::Material).is_cosmetic());
        assert!(classify_effect(ContentKind::PaintMask).is_cosmetic());

        // A render mesh is *not* provably cosmetic in this project: a mesh
        // can be the source of a derived collider, so a mesh override is a
        // gameplay change until measured otherwise.
        assert!(!classify_effect(ContentKind::Mesh).is_cosmetic());
    }

    /// The classification is a property of the id, not of the manifest: two
    /// overrides of the same target always agree, an override of an
    /// airframe is gameplay whatever the author intended, and the
    /// sandbox verdict is derived the same way.
    #[test]
    fn accept_f53_a_override_effect_is_computed_and_never_asserted() {
        let paint = ContentOverride::try_new(
            image("synthetic.hull-panel"),
            OverrideAction::Replace,
            "art/panel.png",
            4_096,
        )
        .expect("the cosmetic override is valid");
        let plane = ContentOverride::try_new(
            airframe("synthetic.bolt-ii"),
            OverrideAction::Replace,
            "tuning/bolt-ii.toml",
            512,
        )
        .expect("the gameplay override is valid");

        assert_eq!(paint.effect(), OverrideEffect::Cosmetic);
        assert_eq!(plane.effect(), OverrideEffect::Gameplay);
        assert_eq!(paint.effect().label(), "cosmetic");
        assert_eq!(plane.effect().label(), "gameplay");

        // Same target, different source: the effect cannot move, because it
        // is not read from the record's other fields.
        let repaint = ContentOverride::try_new(
            image("synthetic.hull-panel"),
            OverrideAction::Add,
            "art/repaint.png",
            9,
        )
        .expect("the second override of the target is valid");
        assert_eq!(repaint.effect(), paint.effect());

        // The action is a real distinction and is reported as one.
        assert_eq!(repaint.action().label(), "add");
        assert_eq!(paint.action().label(), "replace");
        assert_eq!(
            repaint.to_string(),
            "add image/synthetic.hull-panel via art/repaint.png"
        );
        assert_eq!(
            paint.to_string(),
            "replace image/synthetic.hull-panel via art/panel.png"
        );

        // F53 non-negotiable 2: a mission program is identifiable from its
        // id, so the plan can name the validator it has to pass.
        let scripted = ContentOverride::try_new(
            mission("synthetic.first-sortie"),
            OverrideAction::Replace,
            "mission/first-sortie.json",
            1_024,
        )
        .expect("the mission override is valid");
        assert_eq!(scripted.validation(), OverrideValidation::SandboxedProgram);
        assert_eq!(paint.validation(), OverrideValidation::OriginalAdapter);
        for kind in ContentKind::ALL {
            assert_eq!(
                classify_validation(*kind).label(),
                if SANDBOXED_PROGRAM_CONTENT_KINDS.contains(kind) {
                    "sandboxed_program"
                } else {
                    "original_adapter"
                },
                "{kind} has an undeclared sandbox verdict"
            );
        }
    }

    /// The record keeps the manifest's spelling and byte count, and refuses
    /// every spelling that could escape the mod root (F53 AC02's path half).
    #[test]
    fn accept_f53_a_override_sources_are_safe_relative_spellings() {
        let override_of = |spelling: &str| {
            ContentOverride::try_new(
                image("synthetic.hull-panel"),
                OverrideAction::Replace,
                spelling,
                1,
            )
        };
        for hostile in [
            "../outside.png",
            "art/../../outside.png",
            "/etc/passwd",
            "\\windows\\system32\\x.dll",
            "C:/mods/panel.png",
            "./panel.png",
            "art//panel.png",
            "art/panel.png\u{0}",
            "",
        ] {
            let refused = override_of(hostile).expect_err("a hostile source must be refused");
            assert!(
                matches!(refused, ManifestError::UnsafePath { .. }),
                "{hostile:?} was refused as {refused:?}, which is not a path refusal"
            );
        }
        // The accepted spelling survives byte-for-byte for diagnostics, and
        // the declared size is carried through untouched.
        let good = override_of("art/panel.png").expect("a safe source is accepted");
        assert_eq!(good.source().as_str(), "art/panel.png");
        assert_eq!(good.declared_bytes(), 1);
        assert_eq!(good.target().as_str(), "image/synthetic.hull-panel");
    }

    /// The set-level answer that F53 non-negotiable 3 turns into session,
    /// save, replay and handshake marking, and the claim status vocabulary it
    /// is not: nothing in this stage can report original evidence.
    #[test]
    fn accept_f53_a_modification_labels_and_never_claim_original_evidence() {
        assert_eq!(ModModification::CosmeticOnly.label(), "cosmetic_only");
        assert_eq!(ModModification::Gameplay.label(), "gameplay");
        assert!(!ModModification::CosmeticOnly.marks_sessions());
        assert!(ModModification::Gameplay.marks_sessions());
        assert_ne!(ClaimStatus::Designed, ClaimStatus::VerifiedOriginal);
    }
}
