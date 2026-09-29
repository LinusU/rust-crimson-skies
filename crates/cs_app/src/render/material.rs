//! The material-classification contract: which render pipeline one surface
//! belongs to, and what may legitimately decide it
//! (`specs/F17-rendering-material-fidelity-and-scalable-presentation.md`,
//! stage `### F17-A`; shared contract
//! `docs/contracts/IDENTITY-CONTENT.md`).
//!
//! The original GameZ material record does **not** establish a render
//! class: its flag bits name "textured", "cycled" and three bookkeeping
//! bits, and nothing in the 40 stored bytes says "glass", "fence" or
//! "additive" (`docs/findings/2026-09-29-f10-c-02-gamez-material-records.md`).
//! So a class is never *derived* from raw bytes here — it is **declared**
//! with an epistemic status ([`DeclaredClass`]), and [`classify`] only
//! checks that the declaration is consistent with the facts the content
//! pipeline actually established ([`MaterialFacts`]). What the facts do not
//! establish is reported in [`Classification::Unclassified`], never
//! defaulted to opaque (spec F17 non-negotiable #1, the contract's
//! "unknown means unknown").
//!
//! [`MaterialClass`] is the closed vocabulary the sheet names — opaque,
//! masked, blended, additive, emissive — and each class maps to exactly one
//! [`RenderPhase`] of the ordered draw plan. The phase assignment is a
//! new-engine design decision, documented as such; no original ordering is
//! asserted.

use std::fmt;

use cs_formats::gamez::RawMaterialRecord;
use cs_formats::texture::{AlphaSource, AlphaTest};
use cs_types::evidence::ClaimStatus;

/// The semantic render class of one surface: which coverage and blend
/// treatment it gets.
///
/// This is the vocabulary the sheet's deliverable names. Classes are added
/// only with evidence for them ("other observed classes"); a class that is
/// asserted but not established stays in [`Classification::Unclassified`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MaterialClass {
    /// No transparency anywhere: coverage is ignored even when the image
    /// stores it. Drawn in [`RenderPhase::Opaque`].
    Opaque,
    /// Alpha-cut: coverage is tested against a threshold, texels below it
    /// are discarded and the rest is opaque (a fence, foliage). Depth is
    /// written; nothing is blended. Drawn in [`RenderPhase::Masked`].
    Masked,
    /// Alpha-blended translucency (glass, smoke, propeller discs): the
    /// surface blends over what is behind it, so it must draw back-to-front.
    /// Drawn in [`RenderPhase::Translucent`].
    Blended,
    /// Additive contribution (light sprites, muzzle flash): the surface
    /// adds to the framebuffer. Drawn in [`RenderPhase::Additive`], after
    /// every translucent surface.
    Additive,
    /// Self-illuminated: the surface emits light and is not dimmed by scene
    /// lighting. Emission is a shading property, not a coverage one, so it
    /// draws in [`RenderPhase::Opaque`].
    Emissive,
}

/// One pass of the ordered draw plan, in submission order.
///
/// The order is the new-engine design for F17's "ordered effects": fully
/// opaque surfaces first, alpha-tested surfaces next (they write depth but
/// discard texels), then depth-sorted translucency, then additive effects.
/// It is `Designed`, not measured original behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RenderPhase {
    /// Fully opaque surfaces.
    Opaque,
    /// Alpha-tested surfaces.
    Masked,
    /// Blended translucency, sorted back-to-front by view depth.
    Translucent,
    /// Additive effects, sorted back-to-front, after all translucency.
    Additive,
}

impl RenderPhase {
    /// Every phase in submission order.
    pub const ALL: [Self; 4] = [
        Self::Opaque,
        Self::Masked,
        Self::Translucent,
        Self::Additive,
    ];

    /// Stable lowercase identifier, for diagnostics and fingerprints.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Opaque => "opaque",
            Self::Masked => "masked",
            Self::Translucent => "translucent",
            Self::Additive => "additive",
        }
    }

    /// Whether the phase sorts its items back-to-front by view depth.
    pub const fn depth_sorted(self) -> bool {
        matches!(self, Self::Translucent | Self::Additive)
    }
}

impl fmt::Display for RenderPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl MaterialClass {
    /// Stable lowercase identifier, for diagnostics and fingerprints.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Opaque => "opaque",
            Self::Masked => "masked",
            Self::Blended => "blended",
            Self::Additive => "additive",
            Self::Emissive => "emissive",
        }
    }

    /// The phase this class draws in (new-engine ordering design).
    pub const fn phase(self) -> RenderPhase {
        match self {
            Self::Opaque | Self::Emissive => RenderPhase::Opaque,
            Self::Masked => RenderPhase::Masked,
            Self::Blended => RenderPhase::Translucent,
            Self::Additive => RenderPhase::Additive,
        }
    }

    /// Whether the class consumes coverage at all: a declared class that
    /// needs coverage contradicts a surface that carries none.
    const fn needs_coverage(self) -> bool {
        matches!(self, Self::Masked | Self::Blended)
    }
}

impl fmt::Display for MaterialClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// Where a surface's coverage (alpha) comes from.
///
/// This is the material-side projection of the texture descriptor's
/// `AlphaSource` (F08-A): the image variants establish where stored
/// coverage lives; `Uniform` covers surfaces whose coverage is a declared
/// constant (untextured tinted glass) and `Unknown` is the honest state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Coverage {
    /// No coverage anywhere: every texel is fully opaque.
    Opaque,
    /// Coverage is stored in the bound image, via this `AlphaSource`.
    /// Never [`AlphaSource::Opaque`] or [`AlphaSource::Unknown`] — the
    /// [`Self::from_source`] normalizer maps those to `Opaque`/`Unknown`.
    Texture(AlphaSource),
    /// A declared uniform opacity, `0` fully transparent to `255` fully
    /// opaque. Used for surfaces like untextured tinted glass whose
    /// coverage is a constant, not stored texels.
    Uniform(u8),
    /// Whether the surface carries coverage at all is not established.
    Unknown,
}

impl Coverage {
    /// Projects a texture descriptor's `AlphaSource` onto this vocabulary.
    pub const fn from_source(source: AlphaSource) -> Self {
        match source {
            AlphaSource::Opaque => Self::Opaque,
            AlphaSource::Unknown => Self::Unknown,
            other => Self::Texture(other),
        }
    }

    /// Whether the surface carries coverage a mask or a blend can use.
    /// A `Uniform(255)` is still coverage — declared constant translucency
    /// is the surface's whole point; `255` is the degenerate opaque edge.
    pub const fn has_coverage(self) -> bool {
        match self {
            Self::Opaque | Self::Unknown => false,
            Self::Texture(_) | Self::Uniform(_) => true,
        }
    }
}

/// One axis of texture-coordinate addressing.
///
/// The vocabulary of the new engine's sampler contract: what the original
/// renderer used per material is unmeasured, so an undeclared address mode
/// is a [`MaterialUnknown`], not a default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AddressMode {
    /// Coordinates wrap modulo 1.
    Repeat,
    /// Coordinates clamp to the edge texel.
    Clamp,
}

impl AddressMode {
    /// Stable lowercase identifier.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Repeat => "repeat",
            Self::Clamp => "clamp",
        }
    }
}

/// Texture addressing for both axes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextureAddress {
    /// Horizontal axis.
    pub u: AddressMode,
    /// Vertical axis.
    pub v: AddressMode,
}

/// The asserted render class of one material and the epistemic status of
/// the assertion.
///
/// `status` is the IDENTITY-CONTENT evidence class of the *class
/// assignment*: `Designed` for the authored synthetic scene, `Documented`
/// or stronger once original-material evidence lands. `Unknown` and
/// `Contradicted` are refused at construction — an assertion that asserts
/// nothing, or two irreconcilable ones, is not a declaration the classifier
/// may treat as input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeclaredClass {
    class: MaterialClass,
    status: ClaimStatus,
}

/// Why a [`DeclaredClass`] could not be built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeclaredClassError(pub ClaimStatus);

impl fmt::Display for DeclaredClassError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "a class declaration needs an asserting status, not {}",
            self.0.label()
        )
    }
}

impl std::error::Error for DeclaredClassError {}

impl DeclaredClass {
    /// Declares `class` with evidence status `status`.
    ///
    /// # Errors
    ///
    /// [`DeclaredClassError`] when `status` is [`ClaimStatus::Unknown`] or
    /// [`ClaimStatus::Contradicted`].
    pub fn new(class: MaterialClass, status: ClaimStatus) -> Result<Self, DeclaredClassError> {
        match status {
            ClaimStatus::Unknown | ClaimStatus::Contradicted => Err(DeclaredClassError(status)),
            _ => Ok(Self { class, status }),
        }
    }

    /// The asserted class.
    pub const fn class(&self) -> MaterialClass {
        self.class
    }

    /// The evidence status of the assertion.
    pub const fn status(&self) -> ClaimStatus {
        self.status
    }
}

/// Everything the classifier is allowed to see about one material.
///
/// Plain data a producer fills field by field — a GameZ audit row plus its
/// resolved texture descriptor for the real path, an authored literal for
/// the synthetic fixture. [`MaterialFacts::for_raw_record`] is what a stored
/// GameZ record *alone* establishes, and it is deliberately never enough to
/// classify: the record does not say which pipeline its surface needs.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialFacts {
    /// The declared class assertion, when any source makes one.
    pub declared: Option<DeclaredClass>,
    /// Where coverage comes from.
    pub coverage: Coverage,
    /// The alpha-test threshold, separate from the coverage source (F08
    /// non-negotiable #1).
    pub alpha_test: AlphaTest,
    /// Declared two-sidedness; `None` when unmeasured.
    pub two_sided: Option<bool>,
    /// Declared texture addressing; `None` when unmeasured.
    pub addressing: Option<TextureAddress>,
    /// Whether the bound mesh stores per-corner colors.
    pub vertex_colors: bool,
    /// Flag bits of the stored record that no reference names
    /// (`RawMaterialRecord::unknown_flag_bits`; zero for authored facts).
    pub unknown_flag_bits: u8,
}

impl MaterialFacts {
    /// Facts authored from nothing but a class declaration: no stored
    /// record behind it, so no flag unknowns.
    pub fn declared(declared: DeclaredClass) -> Self {
        Self {
            declared: Some(declared),
            coverage: Coverage::Unknown,
            alpha_test: AlphaTest::Unknown,
            two_sided: None,
            addressing: None,
            vertex_colors: false,
            unknown_flag_bits: 0,
        }
    }

    /// Facts a stored GameZ record alone establishes: its unnamed flag bits
    /// and nothing else. There is deliberately no `declared` — the record
    /// does not name a render class — and coverage stays `Unknown` until a
    /// resolved texture descriptor or an evidence-backed declaration
    /// supplies it.
    pub fn for_raw_record(record: &RawMaterialRecord) -> Self {
        Self {
            declared: None,
            coverage: Coverage::Unknown,
            alpha_test: AlphaTest::Unknown,
            two_sided: None,
            addressing: None,
            vertex_colors: false,
            unknown_flag_bits: record.unknown_flag_bits(),
        }
    }
}

/// A presentation decision the classification could not settle.
///
/// Mirrors the `PresentationUnknown` pattern of
/// `cs_content::textures`: an entry here is a reason the surface is not
/// ready for release presentation, never something the renderer may
/// quietly pick a default for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MaterialUnknown {
    /// Whether the original renderer drew the surface two-sided.
    TwoSided,
    /// The texture addressing mode of the original sampler.
    TextureAddressing,
    /// How stored per-corner colors are applied (modulate, add, ignore) —
    /// carried from `MeshPresentationUnknown::VertexColor`.
    VertexColorMeaning,
}

impl MaterialUnknown {
    /// Stable lowercase identifier, used as an unsupported reason.
    pub const fn code(self) -> &'static str {
        match self {
            Self::TwoSided => "two_sided_unknown",
            Self::TextureAddressing => "texture_addressing_unknown",
            Self::VertexColorMeaning => "vertex_color_meaning_unknown",
        }
    }
}

impl fmt::Display for MaterialUnknown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// Why a material could not be classified.
///
/// Every variant is a stable code a consumer groups by; a material that
/// fails is never silently drawn as opaque (spec F17 non-negotiable #1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClassificationFailure {
    /// The stored record carries flag bits no reference names, so even
    /// whether the material is textured is not established. The bits are
    /// reported exactly as stored.
    UnknownFlagBits {
        /// The unmapped flag bits.
        bits: u8,
    },
    /// No class assertion exists. The material record itself does not
    /// establish one, and no default is applied.
    Undeclared,
    /// The declared class needs coverage the facts say the surface does
    /// not carry: a declared mask or blend on an opaque surface is a
    /// contradiction, not a renderable material.
    ClassWithoutCoverage {
        /// The asserted class.
        class: MaterialClass,
    },
    /// The declared class needs coverage whose source is not established.
    CoverageUnknown {
        /// The asserted class.
        class: MaterialClass,
    },
    /// [`MaterialClass::Masked`] was declared but the alpha-test threshold
    /// is [`AlphaTest::Unknown`]: the cut point cannot be guessed.
    AlphaTestUnknown,
    /// [`MaterialClass::Masked`] was declared with the alpha test
    /// [`AlphaTest::Disabled`]: a mask that discards nothing contradicts
    /// the declaration.
    MaskedTestDisabled,
}

impl ClassificationFailure {
    /// Stable lowercase identifier.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::UnknownFlagBits { .. } => "unknown_flag_bits",
            Self::Undeclared => "undeclared",
            Self::ClassWithoutCoverage { .. } => "class_without_coverage",
            Self::CoverageUnknown { .. } => "coverage_unknown",
            Self::AlphaTestUnknown => "alpha_test_unknown",
            Self::MaskedTestDisabled => "masked_test_disabled",
        }
    }
}

impl fmt::Display for ClassificationFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownFlagBits { bits } => {
                write!(f, "the stored record has unmapped flag bits 0x{bits:02X}")
            }
            Self::Undeclared => write!(f, "no render class is asserted"),
            Self::ClassWithoutCoverage { class } => write!(
                f,
                "declared {class} on a surface the facts say has no coverage"
            ),
            Self::CoverageUnknown { class } => write!(
                f,
                "declared {class} but the coverage source is not established"
            ),
            Self::AlphaTestUnknown => {
                write!(f, "declared masked but the alpha-test threshold is unknown")
            }
            Self::MaskedTestDisabled => {
                write!(f, "declared masked but the alpha test is disabled")
            }
        }
    }
}

impl std::error::Error for ClassificationFailure {}

/// The classification output for one material: the render state the draw
/// plan groups on, plus everything still unmeasured.
#[derive(Clone, Debug, PartialEq)]
pub struct ClassifiedMaterial {
    class: MaterialClass,
    status: ClaimStatus,
    coverage: Coverage,
    alpha_test: AlphaTest,
    two_sided: Option<bool>,
    addressing: Option<TextureAddress>,
    vertex_colors: bool,
    unknowns: Vec<MaterialUnknown>,
}

impl ClassifiedMaterial {
    /// The established render class.
    pub const fn class(&self) -> MaterialClass {
        self.class
    }

    /// The phase this material draws in.
    pub const fn phase(&self) -> RenderPhase {
        self.class.phase()
    }

    /// The evidence status of the class assertion.
    pub const fn status(&self) -> ClaimStatus {
        self.status
    }

    /// Where coverage comes from.
    pub const fn coverage(&self) -> Coverage {
        self.coverage
    }

    /// The alpha-test threshold the declaration and facts agreed on:
    /// `Threshold` for a masked surface, whatever was declared otherwise
    /// (`Disabled` when no test applies).
    pub const fn alpha_test(&self) -> AlphaTest {
        self.alpha_test
    }

    /// Declared two-sidedness, `None` when unmeasured.
    pub const fn two_sided(&self) -> Option<bool> {
        self.two_sided
    }

    /// Declared texture addressing, `None` when unmeasured.
    pub const fn addressing(&self) -> Option<TextureAddress> {
        self.addressing
    }

    /// Whether the bound mesh stores per-corner colors. The values ride
    /// the draw item; how the original renderer applied them is
    /// [`MaterialUnknown::VertexColorMeaning`].
    pub const fn vertex_colors(&self) -> bool {
        self.vertex_colors
    }

    /// The presentation decisions still open, in a fixed order.
    pub fn unknowns(&self) -> &[MaterialUnknown] {
        &self.unknowns
    }

    /// Whether nothing is left open for release presentation.
    pub fn is_release_ready(&self) -> bool {
        self.unknowns.is_empty()
    }
}

/// What the classifier decided about one material's facts.
#[derive(Clone, Debug, PartialEq)]
pub enum Classification {
    /// The facts establish a render class consistently.
    Classified(ClassifiedMaterial),
    /// They do not. Every reason is listed; the surface is never drawn as
    /// a default class.
    Unclassified {
        /// All the reasons, in check order.
        reasons: Vec<ClassificationFailure>,
    },
}

impl Classification {
    /// The classified material, when there is one.
    pub fn classified(&self) -> Option<&ClassifiedMaterial> {
        match self {
            Self::Classified(material) => Some(material),
            Self::Unclassified { .. } => None,
        }
    }

    /// The refusal reasons, empty when classified.
    pub fn reasons(&self) -> &[ClassificationFailure] {
        match self {
            Self::Classified(_) => &[],
            Self::Unclassified { reasons } => reasons,
        }
    }
}

/// Classifies one material's facts into a render class, or reports every
/// reason they do not establish one.
///
/// Checks, in order: unnamed stored flag bits (the record itself is
/// suspect), a missing declaration, coverage the declared class needs but
/// the facts deny or leave unknown, and the alpha-test threshold a mask
/// needs. Every applicable failure is reported, so a record that is both
/// undeclared and bit-suspect names both.
pub fn classify(facts: &MaterialFacts) -> Classification {
    let mut reasons = Vec::new();
    if facts.unknown_flag_bits != 0 {
        reasons.push(ClassificationFailure::UnknownFlagBits {
            bits: facts.unknown_flag_bits,
        });
    }
    let declared = match facts.declared {
        Some(declared) => declared,
        None => {
            reasons.push(ClassificationFailure::Undeclared);
            return Classification::Unclassified { reasons };
        }
    };
    let class = declared.class();
    match facts.coverage {
        Coverage::Unknown if class.needs_coverage() => {
            reasons.push(ClassificationFailure::CoverageUnknown { class });
        }
        coverage if class.needs_coverage() && !coverage.has_coverage() => {
            reasons.push(ClassificationFailure::ClassWithoutCoverage { class });
        }
        _ => {}
    }
    if class == MaterialClass::Masked {
        match facts.alpha_test {
            AlphaTest::Threshold(_) => {}
            AlphaTest::Unknown => reasons.push(ClassificationFailure::AlphaTestUnknown),
            AlphaTest::Disabled => reasons.push(ClassificationFailure::MaskedTestDisabled),
        }
    }
    if !reasons.is_empty() {
        return Classification::Unclassified { reasons };
    }

    let mut unknowns = Vec::new();
    if facts.two_sided.is_none() {
        unknowns.push(MaterialUnknown::TwoSided);
    }
    if facts.addressing.is_none() {
        unknowns.push(MaterialUnknown::TextureAddressing);
    }
    if facts.vertex_colors {
        unknowns.push(MaterialUnknown::VertexColorMeaning);
    }

    Classification::Classified(ClassifiedMaterial {
        class,
        status: declared.status(),
        coverage: facts.coverage,
        alpha_test: facts.alpha_test,
        two_sided: facts.two_sided,
        addressing: facts.addressing,
        vertex_colors: facts.vertex_colors,
        unknowns,
    })
}
