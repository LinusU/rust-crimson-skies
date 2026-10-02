//! The draw consumer of the composed visibility verdict: what the renderer
//! places, and what it reports it did not place
//! (`specs/F20-object-animation-and-authored-destruction-states.md`, stage
//! `### F20-C`, and `specs/F17-rendering-material-fidelity-and-scalable-
//! presentation.md`, stage `### F17-C`; shared contract
//! `docs/contracts/IDENTITY-CONTENT.md`).
//!
//! F20-C.03 composes **one** draw verdict for a node
//! ([`composed_visibility_verdict`], whose [`DrawVerdict`] is `Drawn`,
//! `HiddenByAnimation`, `LodCulled` or `Disabled`) out of two records: F11-C's
//! [`NodePresentation`](crate::scene::NodePresentation) and F20's
//! [`NodeAnimatedVisibility`](crate::animation::visibility::NodeAnimatedVisibility).
//! That composition
//! had no reader, and the render path did not draw from it: the only thing that
//! kept a part off the screen was the batcher, and it decided from a *different
//! and older* record — a copy of the damage state taken when the frame was
//! built ([`InstanceVisual`](crate::render::batch::InstanceVisual) and
//! [`withheld_codes::DESTROYED_PART`](crate::render::batch::withheld_codes::DESTROYED_PART)).
//! So a
//! culled LOD band was drawn on top of the band that was selected, and a node a
//! clip hides at its authored tick was drawn anyway.
//!
//! The batcher's withholding is **not** replaced here, and the two reports are
//! kept apart. A part its damage snapshot named is absent from the frame before
//! this module sees a row; a caller that builds a frame without that snapshot
//! gets the same part withheld here from the marker's own record. Both answers
//! are the same, and both are reported: the frame's own withheld list, and
//! [`VisibilityReport`].
//!
//! This module is the reader, and it is deliberately **not** a second opinion
//! about priority:
//!
//! * **The verdict is composed once, in one place.** Every row's decision is
//!   [`composed_visibility_verdict`]'s, read at the moment the frame is synced,
//!   so a distance change, a damage pass and a clip pass all reach the screen
//!   through the same answer and no consumer can re-rank them
//!   (F20 non-negotiable behavior 1: interpolation may fade a pose, never
//!   re-decide a draw).
//! * **The consumer decides nothing about why.** [`decide`] carries the composed
//!   verdict through unchanged, and the two places that *interpret* it —
//!   [`RowDraw::reason`] and [`VisibilityReport::record`] — match every
//!   [`DrawVerdict`] the composition defines and name no other. A combination
//!   the composition does not define therefore cannot be defaulted into a draw:
//!   a fifth variant would not compile until this consumer had decided it.
//! * **Absence of a record is not a cull.** A row whose part identity no live
//!   scene entity carries has no presentation evidence at all. It is placed,
//!   and counted in [`VisibilityReport::no_record`], because the alternative
//!   would let a missing scene silently delete every draw — and F17
//!   non-negotiable 4 forbids a visual decision that removes gameplay geometry.
//!   This is the same rule the composition already states about a missing
//!   [`NodePresentation`](crate::scene::NodePresentation), applied one level up.
//!
//! # Which entity a row's verdict is read from
//!
//! Through [`LiveAirframeScene`], the ownership record the F11-C load path
//! publishes: it maps a
//! [`SceneNodeId`](cs_content::scene::SceneNodeId) to the entity of the generation that is
//! live, and F11-C states that nothing outside it may address a scene node. A
//! superseded generation is therefore unreachable from here even in the frame
//! where a reload has committed the new one and not yet released the old, so a
//! row is never decided from a node entity that is about to be torn down.
//!
//! # Not a collider
//!
//! Nothing here writes collision. [`VisibilityVerdict::collider`] is the
//! collision half of the same composed verdict and its consumer is a separate
//! task (#504): a draw decision must never remove or add a collider (F17
//! non-negotiable 4).
//!
//! # Designed, not original
//!
//! Which record an original renderer consulted when a node was culled, hidden or
//! destroyed is **unmeasured** — the original animation containers are undecoded
//! (F13), and the original's coupling of visibility to collision is F20-A's
//! recorded unknown. What is fixed here is the *rule* this reimplementation
//! follows, which is F11-C's and F20-C.03's, not a claim about the original.

use bevy::ecs::prelude::Entity;
use bevy::ecs::world::World;

use crate::animation::visibility::{DrawVerdict, VisibilityVerdict, composed_visibility_verdict};
use crate::render::batch::PartRef;
use crate::scene::LiveAirframeScene;

/// The entity whose presentation records decide `part`, when the live scene
/// carries one.
///
/// `None` — and therefore *no* draw decision — in three cases, all of them "the
/// world holds no evidence", never "the row may not be drawn":
///
/// * no [`LiveAirframeScene`] is published, so no node entity is owned by
///   anything;
/// * the row's part identity is unresolved, which the batcher already reports
///   as [`limitation_codes::UNRESOLVED_PART_IDENTITY`];
/// * the part identity is established but names no node of the live scene, a
///   producer/scene mismatch that is counted in [`VisibilityReport::no_record`]
///   rather than guessed around.
///
/// [`limitation_codes::UNRESOLVED_PART_IDENTITY`]:
///   crate::render::batch::limitation_codes::UNRESOLVED_PART_IDENTITY
#[must_use]
pub fn presentation_entity(world: &World, part: PartRef<'_>) -> Option<Entity> {
    let live = world.get_resource::<LiveAirframeScene>()?;
    part.known().and_then(|node| live.entity(node))
}

/// The composed draw decision for one batch row, read from the records the
/// world holds right now.
///
/// The read is a composition, not one writer's opinion: a distance pass that
/// rewrote [`NodePresentation`](crate::scene::NodePresentation) under a clip's
/// [`NodeAnimatedVisibility`](crate::animation::visibility::NodeAnimatedVisibility)
/// is
/// exactly the case this answers, and neither record can lose the other's
/// decision because neither is consulted for authority.
#[must_use]
pub fn row_draw(world: &World, part: PartRef<'_>) -> RowDraw {
    decide(
        presentation_entity(world, part).map(|entity| composed_visibility_verdict(world, entity)),
    )
}

/// The placement decision a composed verdict produces for one row.
///
/// `None` when the composition has no verdict to give, which is the only way a
/// [`RowDraw`] without a verdict exists. The verdict itself is carried, not
/// re-derived: this function adds no priority of its own, so there is nothing
/// here for a caller to disagree with.
#[must_use]
pub const fn decide(verdict: Option<VisibilityVerdict>) -> RowDraw {
    RowDraw {
        verdict: match verdict {
            None => None,
            Some(verdict) => Some(verdict.draw()),
        },
    }
}

/// What the consumer decided about one batch row.
///
/// The composed verdict is the only thing that can withhold a row; this type
/// adds no reason of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowDraw {
    /// The verdict the world's records composed to, `None` when no record
    /// establishes one.
    verdict: Option<DrawVerdict>,
}

impl RowDraw {
    /// The verdict this decision came from, `None` when no record establishes
    /// one.
    #[must_use]
    pub const fn verdict(self) -> Option<DrawVerdict> {
        self.verdict
    }

    /// Whether the row is placed as a draw.
    ///
    /// A row with no record is placed: absence of a record is not a cull.
    #[must_use]
    pub const fn drawn(self) -> bool {
        matches!(self.verdict, None | Some(DrawVerdict::Drawn))
    }

    /// The stable reason code a withheld row carries, when it is withheld.
    ///
    /// The code is the composed verdict's own label, so a report can never
    /// disagree with the verdict about why a row is missing.
    #[must_use]
    pub const fn reason(self) -> Option<&'static str> {
        match self.verdict {
            None => None,
            Some(DrawVerdict::Drawn) => None,
            Some(verdict) => Some(verdict.label()),
        }
    }
}

/// What the composed visibility verdict decided for one frame's rows.
///
/// A report, not a decision: every row the frame holds is counted exactly once,
/// so "what the renderer drew" can be compared against what the frame planned
/// (whose own withheld list is the batcher's, taken from the damage snapshot)
/// without either being silent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VisibilityReport {
    /// Rows the composed verdict drew and the sync placed.
    pub drawn: usize,
    /// Rows a playing clip's visibility channel hides.
    pub hidden_by_animation: usize,
    /// Rows LOD culled at the viewer distance the LOD pass last ran at.
    pub lod_culled: usize,
    /// Rows the node or an ancestor is disabled for.
    pub disabled: usize,
    /// Rows with **no** presentation record to decide from, which are drawn: no
    /// live scene, an unresolved part identity, or a part the live scene does
    /// not contain. Counted so the gap is visible instead of looking like a
    /// decision.
    pub no_record: usize,
}

impl VisibilityReport {
    /// Counts one row's decision.
    ///
    /// Exhaustive over the composed verdict for the same reason [`decide`] is:
    /// a verdict the composition did not define would have to be given a count,
    /// and a count is a default.
    pub const fn record(&mut self, decision: RowDraw) {
        match decision.verdict() {
            None => self.no_record += 1,
            Some(DrawVerdict::Drawn) => self.drawn += 1,
            Some(DrawVerdict::HiddenByAnimation) => self.hidden_by_animation += 1,
            Some(DrawVerdict::LodCulled) => self.lod_culled += 1,
            Some(DrawVerdict::Disabled) => self.disabled += 1,
        }
    }

    /// How many rows the composed verdict kept off the screen.
    #[must_use]
    pub const fn withheld(&self) -> usize {
        self.hidden_by_animation + self.lod_culled + self.disabled
    }

    /// How many rows the sync placed.
    #[must_use]
    pub const fn placed(&self) -> usize {
        self.drawn + self.no_record
    }
}
