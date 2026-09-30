//! Safe visibility and streaming: which sectors a load keeps resident, and why
//! (F18-C).
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
//! stage `### F18-C`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! # What the policy is
//!
//! [`super::residency`] is the transaction — what a load *means*. This module is
//! the *decision* about which sectors a load should hold right now, given a
//! focus point and a radius. It is a radius policy because that is the smallest
//! honest answer: a sector's own [`Aabb`](cs_content::world::Aabb) is the extent
//! the record already declares, and "is the focus within `radius` of that
//! extent" needs no invented per-sector visibility metadata. **The original's
//! own streaming rule is unmeasured** — whether it streamed by distance at all,
//! by a mission-authored mask, or by neither — and nothing here claims to
//! reproduce it; see
//! `docs/findings/2026-09-30-f18-c-mission-overlays-and-visibility-streaming.md`.
//!
//! # Safe, in two rules
//!
//! 1. **A gameplay-required object is never streamed away.** The load declares
//!    it ([`WorldInstance::required_objects`]), and this policy holds every
//!    sector that holds one whatever the focus is. That is the first half of F18
//!    non-negotiable behavior 3: the object is still there, still collidable and
//!    still stamped, not summarized.
//! 2. **Everything else that streams away is *summarized*, not forgotten.** Its
//!    objects keep their condition
//!    ([`super::residency::damage_object`]) and the overlays the load already
//!    applied ([`super::overlays`]) in the residency record, so a reloaded
//!    sector comes back with them. That is the second half of the same rule: a
//!    door this stage opens, streams out and finds open again.
//!
//! Rule 1 is a *hold*, not a force-load. A load already has every declared
//! sector resident ([`super::residency::load_world`]), so "the required object is
//! there" holds at the start of a mission, and a pass is the only thing that can
//! take it away — which rule 1 forbids. A caller that wants a required sector
//! present after it *was* streamed away asks for it with
//! [`super::residency::load_sector`].
//!
//! # No arbitrary wall
//!
//! F18 non-negotiable behavior 4 forbids an arbitrary invisible wall in fidelity
//! mode, and this module cannot add one: it moves *whole sectors* the definition
//! itself partitioned, using the extents the record declares, and it *holds* a
//! sector rather than putting geometry in front of an aircraft. Streaming the
//! sector an aircraft is inside of is a real hazard this stage does not solve —
//! it is recorded as a limitation, not silently left to the caller.
//!
//! # Why the pass is a function and not a system
//!
//! Streaming loads and despawns entities through
//! [`super::residency::load_sector`] and [`super::residency::unload_sector`],
//! and a sector load reaches the mesh spawn path, which takes an [`App`] rather
//! than a [`World`]. A scheduled system cannot hold an `App`, so the pass is a
//! function a mission composition calls when its focus moves — the same shape as
//! the load transaction itself, and for the same reason. The decision half —
//! [`holds_sector`], [`retained_sectors`] and
//! [`VisibilityRequest::holds`] — is pure and needs no app at all, so "would this
//! sector be streamed?" is answerable before anything moves.

use std::collections::BTreeSet;

use bevy::prelude::{App, Vec3};
use cs_content::world::{Aabb, SectorId, WorldDefinition, WorldObjectId};

use super::meshes::WorldMeshes;
use super::overlays::OverlayError;
use super::residency::{WorldLoadError, WorldResidency, load_sector, residency, unload_sector};

/// The focus a visibility pass is asked about.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VisibilityRequest {
    /// The point the policy measures from, in canonical meters: a camera's or a
    /// player aircraft's world position.
    pub focus_m: [f64; 3],
    /// How far from that point a sector is still held, in meters. `0.0` holds
    /// only the sectors the focus is inside.
    pub radius_m: f64,
}

impl VisibilityRequest {
    /// A request at `focus_m` with an explicit `radius_m`.
    #[must_use]
    pub const fn new(focus_m: [f64; 3], radius_m: f64) -> Self {
        Self { focus_m, radius_m }
    }

    /// Whether the request names a usable focus and radius.
    ///
    /// A non-finite coordinate, a non-finite radius and a negative radius are
    /// all refused rather than clamped. A policy that quietly turned a
    /// nonsensical radius into `0.0` would stream a whole world away on one bad
    /// frame, and one that turned it into "everything" would hide the bug by
    /// streaming nothing at all; both would make a broken producer look like a
    /// working one.
    pub const fn validate(&self) -> Result<(), VisibilityError> {
        let mut axis = 0;
        while axis < 3 {
            if !self.focus_m[axis].is_finite() {
                return Err(VisibilityError::UnusableFocus { axis });
            }
            axis += 1;
        }
        if !self.radius_m.is_finite() {
            return Err(VisibilityError::UnusableRadius);
        }
        if self.radius_m < 0.0 {
            return Err(VisibilityError::NegativeRadius {
                radius_m: self.radius_m,
            });
        }
        Ok(())
    }

    /// Whether `bounds` lies within [`VisibilityRequest::radius_m`] of the focus.
    ///
    /// The test is point-to-box distance: `0` inside the box, and the distance
    /// to the nearest face outside it. Comparing against the box's *centre*
    /// instead would keep holding a sector while the focus is hundreds of metres
    /// past its far face, which is exactly the popping a visibility policy exists
    /// to avoid — and a sector held for the wrong reason is memory the player
    /// pays for without seeing.
    #[must_use]
    pub fn holds(&self, bounds: Aabb) -> bool {
        let focus = Vec3::new(
            self.focus_m[0] as f32,
            self.focus_m[1] as f32,
            self.focus_m[2] as f32,
        );
        let min = Vec3::new(
            bounds.min()[0] as f32,
            bounds.min()[1] as f32,
            bounds.min()[2] as f32,
        );
        let max = Vec3::new(
            bounds.max()[0] as f32,
            bounds.max()[1] as f32,
            bounds.max()[2] as f32,
        );
        let nearest = focus.clamp(min, max);
        (focus - nearest).length() <= self.radius_m as f32
    }
}

/// Why a visibility pass was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum VisibilityError {
    /// No world is loaded, so there is nothing whose residency could be streamed.
    NoResidentWorld,
    /// A focus coordinate was NaN or infinite.
    UnusableFocus {
        /// The axis that was not finite.
        axis: usize,
    },
    /// The radius was NaN or infinite.
    UnusableRadius,
    /// The radius was negative. There is no "the focus point itself" reading of a
    /// negative distance, so it is refused rather than made absolute.
    NegativeRadius {
        /// The radius the caller asked for.
        radius_m: f64,
    },
    /// A sector the pass decided to move could not be moved. The sectors already
    /// moved stay moved, and the pass stops here rather than continuing: the
    /// caller is told what it is in, and a retry re-decides from the residency
    /// record rather than from the request that failed.
    Sector(WorldLoadError),
    /// A sector load could not re-apply an overlay, which is the load's own
    /// error and is reported as one.
    Overlay(OverlayError),
}

impl std::fmt::Display for VisibilityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoResidentWorld => {
                write!(f, "no world is loaded, so there is nothing to stream")
            }
            Self::UnusableFocus { axis } => {
                write!(f, "the visibility focus is not finite on axis {axis}")
            }
            Self::UnusableRadius => write!(f, "the visibility radius is not finite"),
            Self::NegativeRadius { radius_m } => write!(
                f,
                "the visibility radius {radius_m} m is negative, and no sector is that \
                 close to nothing"
            ),
            Self::Sector(err) => write!(f, "a sector could not be streamed: {err}"),
            Self::Overlay(err) => write!(
                f,
                "a sector could not be streamed because its overlays could not be \
                 re-applied: {err}"
            ),
        }
    }
}

impl std::error::Error for VisibilityError {}

impl From<WorldLoadError> for VisibilityError {
    fn from(err: WorldLoadError) -> Self {
        Self::Sector(err)
    }
}

impl From<OverlayError> for VisibilityError {
    fn from(err: OverlayError) -> Self {
        Self::Overlay(err)
    }
}

/// What one pass decided and did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VisibilityUpdate {
    /// The sectors whose residency changed, in the order the pass moved them.
    pub moved: Vec<SectorId>,
    /// The sectors the pass loaded, in the order it loaded them.
    pub loaded: Vec<SectorId>,
    /// The sectors the pass unloaded, in the order it unloaded them.
    pub unloaded: Vec<SectorId>,
    /// The sectors that were out of the focus's range and were **held** anyway
    /// because a gameplay-required object lives in them.
    ///
    /// This is the interesting half of the report: the difference between a
    /// streaming policy and a policy that eats the mission.
    pub retained: Vec<SectorId>,
}

impl VisibilityUpdate {
    /// Whether the pass changed no sector's residency.
    ///
    /// Distinct from "did nothing": a pass that only retained sectors has
    /// nothing to report in `moved` and is exactly the policy doing its job.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.moved.is_empty()
    }
}

/// Whether the policy would hold `sector` for `request` on its geometry alone.
///
/// The pure decision, with no residency involved: a sector is held when the
/// focus is within the radius of its own bounds. Exported so the geometry of the
/// policy can be pinned without an app, and so a caller can ask "would this be
/// streamed?" before it streams anything.
#[must_use]
pub fn holds_sector(
    definition: &WorldDefinition,
    request: &VisibilityRequest,
    sector: &SectorId,
) -> bool {
    definition
        .sector(sector)
        .is_some_and(|record| request.holds(record.bounds()))
}

/// The sectors `request` does **not** reach that a gameplay-required object
/// lives in.
///
/// An object that names **no** sector is resident by definition
/// ([`WorldDefinition::resident_objects`]) and names no sector here either: the
/// rule that protects it is the residency rule, not this one, and inventing a
/// sector to hold would be a sector the record never declared.
#[must_use]
pub fn retained_sectors(
    definition: &WorldDefinition,
    request: &VisibilityRequest,
    required: &BTreeSet<WorldObjectId>,
) -> BTreeSet<SectorId> {
    let out_of_range: BTreeSet<SectorId> = definition
        .sectors()
        .iter()
        .filter(|record| !request.holds(record.bounds()))
        .map(|record| record.id().clone())
        .collect();
    required
        .iter()
        .filter_map(|object| definition.object(object))
        .flat_map(|record| record.sectors().iter())
        .filter(|sector| out_of_range.contains(sector))
        .cloned()
        .collect()
}

/// Runs one visibility pass over the resident world and reports what it moved.
///
/// Everything is decided before anything is moved: the request, the set of
/// sectors to load and the set to unload are established first, so a refusal
/// leaves the residency exactly as it was.
///
/// **Loads run before unloads.** A pass that unloaded first could empty a world
/// and then fail to fill it again, which is the one outcome a streaming policy
/// must never produce. A pass that loads first can fail with a world that is
/// merely *larger* than it was, and every sector it did move is in the report.
///
/// # Errors
///
/// [`VisibilityError::NoResidentWorld`] when nothing is loaded;
/// [`VisibilityError::UnusableFocus`], [`VisibilityError::UnusableRadius`] and
/// [`VisibilityError::NegativeRadius`] for a request that names no usable focus;
/// and [`VisibilityError::Sector`] when a sector the pass decided to move could
/// not be moved.
pub fn update_visibility(
    app: &mut App,
    request: &VisibilityRequest,
    meshes: &WorldMeshes,
) -> Result<VisibilityUpdate, VisibilityError> {
    request.validate()?;

    // 1. Decide, read-only.
    let (retained, to_load, to_unload) = {
        let Some(resident) = residency(app.world()).map(WorldResidency::resident) else {
            return Err(VisibilityError::NoResidentWorld);
        };
        let definition = resident.definition();
        let loaded = resident.loaded_sectors();
        let held: BTreeSet<SectorId> = definition
            .sectors()
            .iter()
            .filter(|record| request.holds(record.bounds()))
            .map(|record| record.id().clone())
            .collect();
        let retained = retained_sectors(definition, request, resident.required_objects());
        let keep: BTreeSet<SectorId> = held.union(&retained).cloned().collect();
        (
            retained,
            keep.difference(loaded).cloned().collect::<Vec<_>>(),
            loaded.difference(&keep).cloned().collect::<Vec<_>>(),
        )
    };

    // 2. Change. Loads first, so a failure leaves a world that is too large
    //    rather than one that is missing geometry it was holding.
    let mut update = VisibilityUpdate {
        retained: retained.into_iter().collect(),
        ..VisibilityUpdate::default()
    };
    for sector in to_load {
        load_sector(app, &sector, meshes)?;
        update.loaded.push(sector.clone());
        update.moved.push(sector);
    }
    for sector in to_unload {
        unload_sector(app, &sector)?;
        update.unloaded.push(sector.clone());
        update.moved.push(sector);
    }
    Ok(update)
}
