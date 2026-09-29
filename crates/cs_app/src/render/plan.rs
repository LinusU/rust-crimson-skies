//! The ordered draw plan: which surfaces draw in which pass and in which
//! sequence (`specs/F17-rendering-material-fidelity-and-scalable-
//! presentation.md`, stage `### F17-A`).
//!
//! [`DrawPlan::build`] groups the submitted [`DrawItem`]s into the fixed
//! [`RenderPhase`] order and sorts the depth-sensitive phases
//! back-to-front against one [`SceneView`]. The ordering rules are the
//! new-engine contract for spec F17's "ordered effects":
//!
//! * opaque and masked items keep their submission order — authored
//!   material ordering is preserved, never re-sorted away (spec F17
//!   non-negotiable #1);
//! * translucent and additive items sort by descending view depth, so
//!   overlapping glass blends over the pane behind it; ties keep
//!   submission order, and every equal-depth pair is **reported** in
//!   [`DrawPlan::limitations`], because that tie-break is a deterministic
//!   rule, not measured correctness (spec F17 non-negotiable #2: "report
//!   sorting limitations instead of hiding geometry").
//!
//! The plan borrows nothing: each entry names its item by index into the
//! submission list, so the plan stays valid while the scene mutates around
//! it and is the thing a fingerprint can pin.

use std::fmt;

use cs_assets::install::sha256;
use cs_types::evidence::ContentHash;

use crate::render::material::{ClassifiedMaterial, RenderPhase};

/// A stable name for one draw item inside a scene build.
///
/// Validated at construction: a render scene addresses its items by this
/// key in logs, limitations and fingerprints, never by a slice position.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DrawItemKey(String);

/// Why a [`DrawItemKey`] was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyError {
    /// The key is empty or longer than [`MAX_KEY_LEN`].
    Length {
        /// Its byte length.
        len: usize,
    },
    /// The key contains a byte outside the id grammar
    /// (`a-z`, `0-9`, `_`, `-`, `.`, `/`).
    Character {
        /// The offending byte.
        byte: u8,
    },
}

/// Longest draw-item key accepted.
pub const MAX_KEY_LEN: usize = 64;

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Length { len } => {
                write!(
                    f,
                    "draw item key is {len} bytes, expected 1..={MAX_KEY_LEN}"
                )
            }
            Self::Character { byte } => {
                write!(f, "draw item key contains the byte 0x{byte:02X}")
            }
        }
    }
}

impl std::error::Error for KeyError {}

impl DrawItemKey {
    /// Validates `key` against the id grammar.
    ///
    /// # Errors
    ///
    /// [`KeyError`] for an empty, overlong or out-of-grammar key.
    pub fn new(key: &str) -> Result<Self, KeyError> {
        if key.is_empty() || key.len() > MAX_KEY_LEN {
            return Err(KeyError::Length { len: key.len() });
        }
        if let Some(&byte) = key
            .as_bytes()
            .iter()
            .find(|b| !matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.' | b'/'))
        {
            return Err(KeyError::Character { byte });
        }
        Ok(Self(key.to_owned()))
    }

    /// The key text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DrawItemKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The view the draw plan sorts translucent surfaces against.
///
/// A minimal camera stand-in for the contract stage: a position and a
/// forward direction. F21 owns real cameras; what the plan needs is a
/// depth order, and `depth` is all this type computes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneView {
    position_m: [f32; 3],
    forward: [f32; 3],
}

/// Why a [`SceneView`] was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewError {
    /// A component is not finite.
    NonFinite,
    /// The forward direction is the zero vector: no depth order exists.
    ZeroForward,
}

impl fmt::Display for ViewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite => write!(f, "view position or forward is not finite"),
            Self::ZeroForward => write!(f, "view forward is the zero vector"),
        }
    }
}

impl std::error::Error for ViewError {}

impl SceneView {
    /// Builds the view, normalizing `forward`.
    ///
    /// # Errors
    ///
    /// [`ViewError::NonFinite`] for a non-finite component and
    /// [`ViewError::ZeroForward`] when `forward` has no direction.
    pub fn new(position_m: [f32; 3], forward: [f32; 3]) -> Result<Self, ViewError> {
        if !position_m
            .iter()
            .chain(forward.iter())
            .all(|v| v.is_finite())
        {
            return Err(ViewError::NonFinite);
        }
        // Scale first so the norm cannot overflow or underflow: a naive
        // `sqrt(f·f)` turns `f32::MAX` components into `inf` (a zero
        // forward would slip through) and subnormal ones into `0`.
        let max = forward[0].abs().max(forward[1].abs()).max(forward[2].abs());
        if max == 0.0 {
            return Err(ViewError::ZeroForward);
        }
        let scaled = [forward[0] / max, forward[1] / max, forward[2] / max];
        let len = (scaled[0] * scaled[0] + scaled[1] * scaled[1] + scaled[2] * scaled[2]).sqrt();
        Ok(Self {
            position_m,
            forward: [scaled[0] / len, scaled[1] / len, scaled[2] / len],
        })
    }

    /// The view position in meters.
    pub const fn position_m(&self) -> [f32; 3] {
        self.position_m
    }

    /// The normalized forward direction.
    pub const fn forward(&self) -> [f32; 3] {
        self.forward
    }

    /// Signed view depth of `point_m`: positive is in front of the view.
    ///
    /// Always finite: the dot is accumulated in `f64` and clamped to the
    /// `f32` range, so two finite-but-huge coordinates saturate instead of
    /// leaking `inf`/`NaN` into the depth-sorted phases — the sort must
    /// always get a decidable order and a reportable tie.
    pub fn depth(&self, point_m: [f32; 3]) -> f32 {
        let d = [
            f64::from(point_m[0]) - f64::from(self.position_m[0]),
            f64::from(point_m[1]) - f64::from(self.position_m[1]),
            f64::from(point_m[2]) - f64::from(self.position_m[2]),
        ];
        (d[0] * f64::from(self.forward[0])
            + d[1] * f64::from(self.forward[1])
            + d[2] * f64::from(self.forward[2]))
        .clamp(f64::from(-f32::MAX), f64::from(f32::MAX)) as f32
    }
}

/// One submitted surface: a classified material at a place, with whatever
/// per-corner colors the mesh stored.
///
/// The material must already be [`ClassifiedMaterial`]: an unclassifiable
/// surface cannot become a draw item at all, so "unclassified is drawn as
/// opaque" is a type-level impossibility, not a runtime check.
#[derive(Clone, Debug, PartialEq)]
pub struct DrawItem {
    key: DrawItemKey,
    material: ClassifiedMaterial,
    center_m: [f32; 3],
    corner_colors: Option<[[f32; 3]; 4]>,
}

/// Why a [`DrawItem`] was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawItemError {
    /// The center or a corner color is not finite.
    NonFinite,
}

impl fmt::Display for DrawItemError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite => write!(f, "draw item center or corner color is not finite"),
        }
    }
}

impl std::error::Error for DrawItemError {}

impl DrawItem {
    /// Builds the item.
    ///
    /// `corner_colors` are the stored per-corner colors, carried
    /// bit-exactly: no colorspace change, clamp or remap happens here —
    /// their meaning is `MaterialUnknown::VertexColorMeaning` until
    /// measured (spec F17 non-negotiable #1 keeps them on the item).
    ///
    /// # Errors
    ///
    /// [`DrawItemError::NonFinite`] for a non-finite center or corner
    /// color.
    pub fn new(
        key: DrawItemKey,
        material: ClassifiedMaterial,
        center_m: [f32; 3],
        corner_colors: Option<[[f32; 3]; 4]>,
    ) -> Result<Self, DrawItemError> {
        let finite = center_m.iter().all(|v| v.is_finite())
            && corner_colors
                .iter()
                .flatten()
                .flatten()
                .all(|v| v.is_finite());
        if !finite {
            return Err(DrawItemError::NonFinite);
        }
        Ok(Self {
            key,
            material,
            center_m,
            corner_colors,
        })
    }

    /// The item key.
    pub const fn key(&self) -> &DrawItemKey {
        &self.key
    }

    /// The classified material.
    pub const fn material(&self) -> &ClassifiedMaterial {
        &self.material
    }

    /// The item's scene position in meters.
    pub const fn center_m(&self) -> [f32; 3] {
        self.center_m
    }

    /// The stored per-corner colors, bit-exact as submitted.
    pub const fn corner_colors(&self) -> Option<[[f32; 3]; 4]> {
        self.corner_colors
    }
}

/// One entry of the ordered plan: which submitted item draws, in which
/// phase, at which view depth.
#[derive(Clone, Debug, PartialEq)]
pub struct PlannedDraw {
    /// Index into the submission list — the item's stable identity.
    pub item: usize,
    /// The item's key, copied so a limitation and a fingerprint can name it.
    pub key: DrawItemKey,
    /// The phase this entry draws in.
    pub phase: RenderPhase,
    /// The view depth the sort used.
    pub depth_m: f32,
}

/// A way the plan's ordering is known to be limited.
///
/// Entries here are the honest report spec F17 non-negotiable #2 asks for:
/// the plan always draws every submitted item, and says where its order is
/// a rule rather than a measurement.
#[derive(Clone, Debug, PartialEq)]
pub enum SortingLimitation {
    /// Two items of one depth-sorted phase sit at the same view depth.
    /// Their relative order is the deterministic tie-break (submission
    /// order), not measured correctness — interpenetrating or
    /// equidistant translucent surfaces have no correct object-level
    /// order.
    EqualViewDepth {
        /// The phase the pair belongs to.
        phase: RenderPhase,
        /// The item that drew first (earlier in submission order — at
        /// equal depth neither is farther).
        first: DrawItemKey,
        /// The item that drew second.
        second: DrawItemKey,
    },
}

impl SortingLimitation {
    /// Stable lowercase identifier, used as an unsupported reason.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::EqualViewDepth { .. } => "equal_view_depth",
        }
    }
}

/// The full ordered draw plan: every submitted item exactly once, in
/// phase order, plus the sorting limitations the plan ran into.
#[derive(Clone, Debug, PartialEq)]
pub struct DrawPlan {
    entries: Vec<PlannedDraw>,
    limitations: Vec<SortingLimitation>,
}

impl DrawPlan {
    /// Orders `items` for `view`.
    ///
    /// Total over validated inputs: [`DrawItem`] and [`SceneView`]
    /// constructors reject everything that could make the sort
    /// undecidable, so building a plan cannot fail.
    pub fn build(items: &[DrawItem], view: &SceneView) -> Self {
        let mut entries = Vec::with_capacity(items.len());
        let mut limitations = Vec::new();
        for phase in RenderPhase::ALL {
            // The phase's items, in submission order. For a depth-sorted
            // phase the stable sort below orders them back-to-front;
            // equal depths keep submission order.
            let mut phase_entries: Vec<PlannedDraw> = items
                .iter()
                .enumerate()
                .filter(|(_, item)| item.material().phase() == phase)
                .map(|(index, item)| PlannedDraw {
                    item: index,
                    key: item.key().clone(),
                    phase,
                    depth_m: view.depth(item.center_m()),
                })
                .collect();
            if phase.depth_sorted() {
                phase_entries.sort_by(|a, b| b.depth_m.total_cmp(&a.depth_m));
                for window in phase_entries.windows(2) {
                    if window[0].depth_m == window[1].depth_m {
                        limitations.push(SortingLimitation::EqualViewDepth {
                            phase,
                            first: window[0].key.clone(),
                            second: window[1].key.clone(),
                        });
                    }
                }
            }
            entries.extend(phase_entries);
        }

        Self {
            entries,
            limitations,
        }
    }

    /// Every entry in draw order.
    pub fn entries(&self) -> &[PlannedDraw] {
        &self.entries
    }

    /// The sorting limitations encountered, in plan order.
    pub fn limitations(&self) -> &[SortingLimitation] {
        &self.limitations
    }

    /// A canonical fingerprint of this plan: phase, key and view depth of
    /// every entry in draw order, plus the limitations.
    ///
    /// The fingerprint identifies a *plan*, a
    /// [`cs_types::evidence::FingerprintKind::Artifact`] product — it pins
    /// ordering behavior for goldens and evidence, never original data.
    pub fn fingerprint(&self) -> ContentHash {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"cs/render/plan/v1\0");
        bytes.extend_from_slice(&(self.entries.len() as u32).to_le_bytes());
        for entry in &self.entries {
            bytes.extend_from_slice(entry.phase.code().as_bytes());
            bytes.push(0);
            bytes.extend_from_slice(entry.key.as_str().as_bytes());
            bytes.push(0);
            bytes.extend_from_slice(&entry.depth_m.to_bits().to_le_bytes());
        }
        bytes.extend_from_slice(&(self.limitations.len() as u32).to_le_bytes());
        for limitation in &self.limitations {
            bytes.extend_from_slice(limitation.code().as_bytes());
            bytes.push(0);
        }
        sha256(&bytes)
    }
}
