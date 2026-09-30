//! The faithful profile and the independently switchable enhanced options
//! (`specs/F17-rendering-material-fidelity-and-scalable-presentation.md`,
//! stage `### F17-C`; shared contract `docs/contracts/IDENTITY-CONTENT.md`).
//!
//! F17-B pinned a *fixed comparison set* — exposure 1.0, no tone curve, gamma
//! 2.2, one sample per pixel — and refused a capture taken under anything else
//! ([`crate::render::capture::capture`]). That is the baseline; this module
//! names it, adds the presentation a modern renderer can offer, and keeps the
//! two apart:
//!
//! * [`RenderProfile::faithful`] is the fidelity baseline. It resolves to
//!   exactly the fixed comparison set, so a frame captured under it is
//!   comparable to another and to a reference capture.
//! * An **enhancement** is a designed improvement, switchable on its own
//!   ([`RenderProfile::with`], [`RenderProfile::without`]) and recorded as
//!   [`ProfileParity::DesignedImprovement`]. Spec F17 non-negotiable 5 calls
//!   modern resolution, antialiasing and optional shadows "designed
//!   improvements, not proof of original parity", and non-negotiable 3 fixes
//!   exposure, tonemapping and gamma *in comparison mode* — so a profile with
//!   any enhancement on is refused as comparison evidence
//!   ([`ProfileError::NotComparisonEvidence`]) rather than quietly producing a
//!   capture that compares nothing.
//!
//! ## What an enhancement may and may not touch
//!
//! An enhancement is a *presentation* decision. It may change the sample
//! count, the tone curve, whether the scene's lights cast shadows, and the
//! resolution the window renders at. It may not change a material class, a
//! blend state, a depth write, a cull face, an alpha test, an address mode, a
//! vertex color, a sort order, which surfaces are drawn, or anything a
//! collider reads. That is structural rather than promised: [`Presentation`]
//! is the *only* value an enhancement reaches, and it holds those four
//! fields and nothing else, so there is no field for an enhancement to change
//! a draw with. Spec F17's deliverable states the same rule ("enhanced options
//! are independently switchable and cannot alter collision or visibility
//! rules"), and the acceptance test for this stage compares the batched frame
//! built under the fully enhanced profile against the faithful one to see the
//! draw content is identical.
//!
//! ## Two options are refused by the type
//!
//! [`Enhancement::TexturalUpscaling`] and [`Enhancement::AssetRedistribution`]
//! exist in the option vocabulary only to be refused
//! ([`ProfileError::RefusedOption`]). Spec F17 non-negotiable 5 forbids
//! "automatic texture upscaling or asset redistribution pipeline": naming them
//! as refused options means a caller that asks for one gets a reason code
//! instead of a silent no-op, and the prohibition is visible in the type
//! rather than in a comment.
//!
//! # Designed, not original
//!
//! Every value here is new-engine design. Nothing asserts what the original
//! 2000 renderer used: not its sample count, not its tone curve, not whether
//! it cast shadows, not its resolution. The sample counts available are the
//! ones Bevy 0.19's `Msaa` can express (`Off` is one sample per pixel, which
//! is why [`msaa_for`] maps 1 to `Off`), and that is an engine fact, not a
//! measurement of the game
//! (`docs/findings/2026-09-30-f17-c-profiles-and-instance-batching.md`).

use std::fmt;

use bevy::core_pipeline::tonemapping::Tonemapping as BevyTonemapping;
use bevy::render::view::Msaa;
use cs_assets::install::sha256;
use cs_types::evidence::ContentHash;

use crate::render::capture::{COMPARISON_MSAA_SAMPLES, ComparisonSettings, Tonemap};

/// The internal render resolution a modern renderer may be asked for.
///
/// Physical pixels, in the order width then height. Both must be positive: a
/// zero extent is not a smaller window, it is a frame that cannot be
/// allocated, and it is refused ([`ProfileError::Resolution`]) rather than
/// clamped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Resolution {
    /// Width in physical pixels.
    pub width: u32,
    /// Height in physical pixels.
    pub height: u32,
}

impl Resolution {
    /// A resolution with both extents positive.
    ///
    /// # Errors
    ///
    /// [`ProfileError::Resolution`] when either extent is zero.
    pub fn new(width: u32, height: u32) -> Result<Self, ProfileError> {
        if width == 0 || height == 0 {
            return Err(ProfileError::Resolution { width, height });
        }
        Ok(Self { width, height })
    }
}

/// Which presentation decision one option makes.
///
/// The kind is the option's identity: a profile holds at most one option of
/// each kind, so switching one on replaces the previous setting of *that*
/// decision and leaves every other decision alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EnhancementKind {
    /// Samples per pixel.
    Antialiasing,
    /// The display tone curve.
    ToneMapping,
    /// Whether the scene's lights cast shadows.
    ShadowMapping,
    /// The internal render resolution.
    RenderResolution,
    /// Automatic texture upscaling. Always refused.
    TexturalUpscaling,
    /// Redistributing assets. Always refused.
    AssetRedistribution,
}

impl EnhancementKind {
    /// Every kind in the canonical order a profile stores them in.
    pub const ALL: [Self; 6] = [
        Self::Antialiasing,
        Self::ToneMapping,
        Self::ShadowMapping,
        Self::RenderResolution,
        Self::TexturalUpscaling,
        Self::AssetRedistribution,
    ];

    /// Stable lowercase identifier.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Antialiasing => "antialiasing",
            Self::ToneMapping => "tone_mapping",
            Self::ShadowMapping => "shadow_mapping",
            Self::RenderResolution => "render_resolution",
            Self::TexturalUpscaling => "textural_upscaling",
            Self::AssetRedistribution => "asset_redistribution",
        }
    }
}

impl fmt::Display for EnhancementKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// One optional presentation improvement, or one refused option.
///
/// The two refused variants are part of the vocabulary so that a caller who
/// asks for the thing spec F17 forbids is told so. See the module docs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Enhancement {
    /// Multisample antialiasing at `samples` samples per pixel. Only the
    /// counts [`msaa_for`] accepts are constructible into a profile.
    Antialiasing {
        /// Samples per pixel.
        samples: u32,
    },
    /// A display tone curve, the same vocabulary the comparison settings use.
    ToneMapping {
        /// The curve.
        curve: Tonemap,
    },
    /// The scene's directional lights cast shadows.
    ShadowMapping,
    /// Render at a fixed internal resolution instead of the window's own.
    RenderResolution {
        /// The requested resolution.
        resolution: Resolution,
    },
    /// Refused: spec F17 non-negotiable 5 forbids automatic texture
    /// upscaling, and a resampled texture is no longer the stored one.
    TexturalUpscaling,
    /// Refused: spec F17 non-negotiable 5 forbids an asset redistribution
    /// pipeline, and no stage of this engine writes derived assets.
    AssetRedistribution,
}

impl Enhancement {
    /// Which presentation decision this option makes.
    pub const fn kind(self) -> EnhancementKind {
        match self {
            Self::Antialiasing { .. } => EnhancementKind::Antialiasing,
            Self::ToneMapping { .. } => EnhancementKind::ToneMapping,
            Self::ShadowMapping => EnhancementKind::ShadowMapping,
            Self::RenderResolution { .. } => EnhancementKind::RenderResolution,
            Self::TexturalUpscaling => EnhancementKind::TexturalUpscaling,
            Self::AssetRedistribution => EnhancementKind::AssetRedistribution,
        }
    }

    /// Stable lowercase identifier, equal to the kind's code.
    pub const fn code(self) -> &'static str {
        self.kind().code()
    }

    /// Whether this option is one the sheet refuses.
    pub const fn is_refused(self) -> bool {
        matches!(self, Self::TexturalUpscaling | Self::AssetRedistribution)
    }
}

impl fmt::Display for Enhancement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Antialiasing { samples } => write!(f, "antialiasing at {samples} samples"),
            Self::ToneMapping { curve } => write!(f, "tone mapping {curve:?}"),
            Self::ShadowMapping => f.write_str("shadow mapping"),
            Self::RenderResolution { resolution } => write!(
                f,
                "render resolution {}x{}",
                resolution.width, resolution.height
            ),
            Self::TexturalUpscaling => f.write_str("textural upscaling (refused)"),
            Self::AssetRedistribution => f.write_str("asset redistribution (refused)"),
        }
    }
}

/// What a profile resolves to: the four presentation decisions a renderer
/// takes, and nothing else.
///
/// There is no field here for a material class, a blend state, a depth write,
/// a cull face, an alpha test, a sort key or a collider, which is what makes
/// "an enhancement cannot alter collision or visibility rules" a property of
/// the type rather than a rule the code follows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Presentation {
    msaa_samples: u32,
    tonemap: Tonemap,
    shadows: bool,
    render_resolution: Option<Resolution>,
}

impl Presentation {
    /// The fidelity baseline: the fixed comparison set with one sample per
    /// pixel, no tone curve, no shadows and no resolution override.
    pub const fn fidelity() -> Self {
        Self {
            msaa_samples: COMPARISON_MSAA_SAMPLES,
            tonemap: Tonemap::None,
            shadows: false,
            render_resolution: None,
        }
    }

    /// Samples per pixel.
    pub const fn msaa_samples(&self) -> u32 {
        self.msaa_samples
    }

    /// The display tone curve.
    pub const fn tonemap(&self) -> Tonemap {
        self.tonemap
    }

    /// Whether the scene's lights cast shadows.
    pub const fn shadows(&self) -> bool {
        self.shadows
    }

    /// The internal render resolution, `None` for "the window's own".
    pub const fn render_resolution(&self) -> Option<Resolution> {
        self.render_resolution
    }

    /// Whether this presentation is exactly the fixed comparison set, which is
    /// what makes a frame captured under it comparable to another.
    pub fn is_fidelity(&self) -> bool {
        *self == Self::fidelity()
    }
}

/// The samples-per-pixel count as Bevy's antialiasing setting.
///
/// `Msaa::Off` renders one sample per pixel, so 1 is `Off` — which is why
/// [`Presentation::fidelity`] and the fixed comparison set agree without a
/// translation table between them.
///
/// # Errors
///
/// [`ProfileError::SampleCount`] for a count Bevy 0.19 cannot express.
/// Nothing is defaulted: an unexpressible count is refused, not rounded down
/// to the nearest one that works.
pub fn msaa_for(samples: u32) -> Result<Msaa, ProfileError> {
    match samples {
        1 => Ok(Msaa::Off),
        2 => Ok(Msaa::Sample2),
        4 => Ok(Msaa::Sample4),
        8 => Ok(Msaa::Sample8),
        other => Err(ProfileError::SampleCount { samples: other }),
    }
}

/// The tone curve as Bevy's camera tone-mapping setting.
///
/// # Errors
///
/// Never: both curves of [`Tonemap`] exist in Bevy 0.19. The result type keeps
/// the mapping total for a caller that adds a curve later, and the
/// `unreachable!` names the curve that would need it.
pub fn bevy_tonemapping(curve: Tonemap) -> BevyTonemapping {
    match curve {
        Tonemap::None => BevyTonemapping::None,
        Tonemap::Filmic => BevyTonemapping::BlenderFilmic,
    }
}

/// What a profile may be used as evidence for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProfileParity {
    /// The fidelity baseline: a frame captured under it compares to another
    /// frame captured under it.
    FidelityBaseline,
    /// A designed improvement. Frames captured under it are not comparison
    /// evidence, whatever they look like.
    DesignedImprovement,
}

impl ProfileParity {
    /// Stable lowercase identifier.
    pub const fn code(self) -> &'static str {
        match self {
            Self::FidelityBaseline => "fidelity_baseline",
            Self::DesignedImprovement => "designed_improvement",
        }
    }
}

impl fmt::Display for ProfileParity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// Why a profile could not be built, or could not be used as evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileError {
    /// A sample count Bevy 0.19 cannot express.
    SampleCount {
        /// The count that was asked for.
        samples: u32,
    },
    /// A render resolution with a zero extent.
    Resolution {
        /// The requested width.
        width: u32,
        /// The requested height.
        height: u32,
    },
    /// An option the sheet forbids, named so the caller learns it was refused
    /// rather than ignored.
    RefusedOption {
        /// The refused option's stable code.
        option: &'static str,
    },
    /// The profile has enhancements on, so a frame captured under it would
    /// not be comparable to a frame captured under the fixed set.
    NotComparisonEvidence {
        /// The options that are on, in canonical order.
        options: Vec<&'static str>,
    },
}

impl ProfileError {
    /// Stable lowercase identifier, used as an unsupported reason.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::SampleCount { .. } => "unsupported_sample_count",
            Self::Resolution { .. } => "invalid_render_resolution",
            Self::RefusedOption { option } => option,
            Self::NotComparisonEvidence { .. } => "not_comparison_evidence",
        }
    }
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SampleCount { samples } => write!(
                f,
                "bevy 0.19 expresses 1, 2, 4 or 8 samples per pixel, not {samples}"
            ),
            Self::Resolution { width, height } => {
                write!(
                    f,
                    "a render resolution must be positive, got {width}x{height}"
                )
            }
            Self::RefusedOption { option } => write!(
                f,
                "{option} is refused: spec F17 non-negotiable 5 allows no automatic texture \
                 upscaling and no asset redistribution pipeline"
            ),
            Self::NotComparisonEvidence { options } => write!(
                f,
                "a profile with {} on is a designed improvement, not comparison evidence",
                options.join(", ")
            ),
        }
    }
}

impl std::error::Error for ProfileError {}

/// A rendering profile: the fidelity baseline plus the enhancements that are
/// switched on.
///
/// A profile holds at most one option of each [`EnhancementKind`], stored in
/// the canonical order of [`EnhancementKind::ALL`], so two profiles that
/// switched the same options on in different orders are equal and fingerprint
/// the same.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RenderProfile {
    enhancements: Vec<Enhancement>,
}

impl RenderProfile {
    /// The fidelity baseline: no enhancement on.
    pub const fn faithful() -> Self {
        Self {
            enhancements: Vec::new(),
        }
    }

    /// A profile with exactly these options, validated.
    ///
    /// # Errors
    ///
    /// [`ProfileError`] for a refused option, an unexpressible sample count or
    /// a zero-extent resolution. An invalid option builds no profile at all:
    /// there is no half-applied state to retry from.
    pub fn new(enhancements: impl IntoIterator<Item = Enhancement>) -> Result<Self, ProfileError> {
        let mut profile = Self::faithful();
        for enhancement in enhancements {
            profile = profile.with(enhancement)?;
        }
        Ok(profile)
    }

    /// The same profile with `enhancement` switched on, replacing any previous
    /// option of the same kind and leaving every other decision alone.
    ///
    /// # Errors
    ///
    /// [`ProfileError`] for a refused option, an unexpressible sample count or
    /// a zero-extent resolution. The receiver is left untouched on an error,
    /// so a caller can retry with a corrected value.
    pub fn with(&self, enhancement: Enhancement) -> Result<Self, ProfileError> {
        // Validation first, so a refused option cannot half-apply.
        match enhancement {
            Enhancement::Antialiasing { samples } => {
                msaa_for(samples)?;
            }
            Enhancement::RenderResolution { resolution } => {
                Resolution::new(resolution.width, resolution.height)?;
            }
            other if other.is_refused() => {
                return Err(ProfileError::RefusedOption {
                    option: other.code(),
                });
            }
            _ => {}
        }
        let mut enhancements = self
            .enhancements
            .iter()
            .copied()
            .filter(|existing| existing.kind() != enhancement.kind())
            .collect::<Vec<_>>();
        enhancements.push(enhancement);
        enhancements.sort_by_key(|existing| existing.kind());
        Ok(Self { enhancements })
    }

    /// The same profile with the option of `kind` switched off. Switching off
    /// an option that is not on is a no-op, which is what makes every option
    /// independently switchable.
    pub fn without(&self, kind: EnhancementKind) -> Self {
        Self {
            enhancements: self
                .enhancements
                .iter()
                .copied()
                .filter(|existing| existing.kind() != kind)
                .collect(),
        }
    }

    /// The option of `kind`, when it is on.
    pub fn enhancement(&self, kind: EnhancementKind) -> Option<Enhancement> {
        self.enhancements
            .iter()
            .copied()
            .find(|enhancement| enhancement.kind() == kind)
    }

    /// Every option that is on, in canonical order.
    pub fn enhancements(&self) -> &[Enhancement] {
        &self.enhancements
    }

    /// Whether no enhancement is on: the profile is the fidelity baseline.
    pub fn is_faithful(&self) -> bool {
        self.enhancements.is_empty()
    }

    /// What this profile may be used as evidence for.
    pub fn parity(&self) -> ProfileParity {
        if self.is_faithful() {
            ProfileParity::FidelityBaseline
        } else {
            ProfileParity::DesignedImprovement
        }
    }

    /// The presentation this profile resolves to.
    pub fn presentation(&self) -> Presentation {
        let mut presentation = Presentation::fidelity();
        for enhancement in &self.enhancements {
            match *enhancement {
                Enhancement::Antialiasing { samples } => presentation.msaa_samples = samples,
                Enhancement::ToneMapping { curve } => presentation.tonemap = curve,
                Enhancement::ShadowMapping => presentation.shadows = true,
                Enhancement::RenderResolution { resolution } => {
                    presentation.render_resolution = Some(resolution);
                }
                // The two refused options cannot be in a profile: `with`
                // refused them and `new` goes through `with`.
                Enhancement::TexturalUpscaling | Enhancement::AssetRedistribution => {}
            }
        }
        presentation
    }

    /// The settings a frame rendered under this profile was handed.
    ///
    /// Every presentation decision the profile owns is in here, which is what
    /// lets [`capture`](crate::render::capture::capture) decide on its own
    /// whether a frame was rendered under the fidelity baseline.
    pub fn settings(&self) -> ComparisonSettings {
        let presentation = self.presentation();
        ComparisonSettings::for_presentation(
            presentation.tonemap(),
            presentation.msaa_samples(),
            presentation.shadows(),
            presentation.render_resolution(),
        )
    }

    /// The fixed comparison settings this profile renders under.
    ///
    /// # Errors
    ///
    /// [`ProfileError::NotComparisonEvidence`] when any enhancement is on. The
    /// fixed set pins exposure, tonemapping and gamma (spec F17
    /// non-negotiable 3) and, since F17-C, the sample count, the shadow
    /// setting and the render resolution — every decision the profile owns — so
    /// a profile that changes any of them cannot produce a comparable capture.
    /// The refusal names which options are on, which the capture's own refusal
    /// cannot do.
    pub fn comparison_settings(&self) -> Result<ComparisonSettings, ProfileError> {
        let settings = self.settings();
        if settings.is_fixed() {
            return Ok(settings);
        }
        Err(ProfileError::NotComparisonEvidence {
            options: self
                .enhancements
                .iter()
                .map(|enhancement| enhancement.code())
                .collect(),
        })
    }

    /// A canonical digest of this profile: its parity and every option that is
    /// on, in canonical order.
    ///
    /// It identifies the *profile*, a
    /// [`cs_types::evidence::FingerprintKind::Artifact`] product. It never
    /// identifies original data, and two profiles that differ in any option
    /// differ here.
    pub fn fingerprint(&self) -> ContentHash {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"cs/render/profile/v1\0");
        bytes.extend_from_slice(self.parity().code().as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(&(self.enhancements.len() as u32).to_le_bytes());
        for enhancement in &self.enhancements {
            bytes.extend_from_slice(enhancement.code().as_bytes());
            bytes.push(0);
            match *enhancement {
                Enhancement::Antialiasing { samples } => {
                    bytes.extend_from_slice(&samples.to_le_bytes());
                }
                Enhancement::ToneMapping { curve } => {
                    bytes.extend_from_slice(curve.code().as_bytes());
                    bytes.push(0);
                }
                Enhancement::ShadowMapping => {}
                Enhancement::RenderResolution { resolution } => {
                    bytes.extend_from_slice(&resolution.width.to_le_bytes());
                    bytes.extend_from_slice(&resolution.height.to_le_bytes());
                }
                Enhancement::TexturalUpscaling | Enhancement::AssetRedistribution => {}
            }
            bytes.push(0);
        }
        sha256(&bytes)
    }
}
