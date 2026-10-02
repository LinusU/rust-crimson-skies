//! The visibility consumer: what a visibility swap means for drawing and for
//! collision, and who wins when LOD, damage and a playing clip disagree
//! (F20-C.03).
//!
//! Spec: `specs/F20-object-animation-and-authored-destruction-states.md`,
//! stage `### F20-C`, non-negotiable behavior 3. Shared contract:
//! `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! F20-A's evaluator already produces the visibility verdict
//! ([`AnimatedNodeState::visibility`](cs_sim::animated_object::AnimatedNodeState::visibility),
//! [`collider_enabled`](cs_sim::animated_object::AnimatedNodeState::collider_enabled))
//! and F20-B deliberately stopped before applying it (its boundary 2):
//! [`NodePresentation`](crate::scene::NodePresentation) is the single field
//! F11-C's [`select_lod_presentation`](crate::scene::select_lod_presentation)
//! rewrites on every pass, so an animation that wrote it would be overwritten
//! by the next distance change — and, in the other direction, an animation pass
//! that wrote `Drawn` over `Disabled` would **re-draw a destroyed node** (F20
//! non-negotiable behavior 3). This module makes that decision instead of
//! deferring it, and it makes it **without two writers**:
//!
//! * **The clip's fact** is [`NodeAnimatedVisibility`], written by
//!   [`super::advance_animation`] on the same verified-binding path as the
//!   other three channels: a value only for a playing instance of a live scene
//!   generation that drives that node, written only when it changed, released
//!   by the instance teardown.
//! * **The composed verdict** is [`VisibilityVerdict`], computed by
//!   [`composed_visibility_verdict`] from the two records that exist — F11-C's
//!   `NodePresentation` and this module's `NodeAnimatedVisibility`.
//!
//! # The verdict is composed at read time, not stored
//!
//! The composition is deliberately **not** a component. A stored verdict would
//! have to be recomputed by a system ordered *after*
//! `select_lod_presentation`, and that system lives in
//! `crates/cs_app/src/scene.rs`, which is outside this task's owner paths, so
//! no honest schedule constraint can be placed on it; a verdict written before
//! the LOD pass would be one distance change stale, which is exactly the
//! silent loss this stage exists to prevent. Composing on read removes the
//! ordering question instead of solving it: the answer is computed from the
//! records **as they are at the moment of the read**, so
//!
//! * the LOD pass cannot lose the animation's verdict — it never touches
//!   [`NodeAnimatedVisibility`];
//! * the animation cannot silently override LOD or damage — it never touches
//!   `NodePresentation` or [`NodeDisabled`](crate::scene::NodeDisabled);
//!
//! and both hold whatever order the two run in, in any schedule.
//!
//! # Who wins, combination by combination
//!
//! The draw half of the verdict:
//!
//! | `NodePresentation` (LOD/damage) | clip visibility | [`DrawVerdict`] |
//! | --- | --- | --- |
//! | `Disabled` (self or an ancestor is `NodeDisabled`) | `Visible` or `Hidden` | `Disabled` |
//! | `LodCulled` | `Visible` or `Hidden` | `LodCulled` |
//! | `Drawn` | `Hidden` | `HiddenByAnimation` |
//! | `Drawn` | `Visible`, or no channel | `Drawn` |
//! | no presentation record | `Hidden` | `HiddenByAnimation` |
//! | no presentation record | `Visible`, or no channel | `Drawn` |
//!
//! * **Damage wins over everything.** It is F11-C's own rule one level up
//!   (`select_lod_presentation`: "`Disabled` wins over `LodCulled` at any
//!   depth, so a destroyed wing stays destroyed whichever band its parent
//!   group selects"), and the ancestor fold is already in that record, so the
//!   composition needs no hierarchy walk of its own. A looping clip that
//!   re-shows the node on every pass and a distance change therefore both
//!   leave a destroyed node not drawn — non-negotiable behavior 3.
//! * **LOD's reason outranks the clip's, without losing the clip's fact.** A
//!   culled variant is reported culled: at that distance it is not the band
//!   the group chose, and blaming the clip for a distance decision would be a
//!   lie. The clip's own record still reads `Hidden` and still reaches
//!   collision (below).
//! * **The clip decides only against `Drawn`.** Nothing in LOD or damage
//!   opposes the node, so the clip's own verdict is the answer.
//! * **A missing presentation record is not a cull.** An entity the LOD pass
//!   has never written for carries no evidence of a distance decision, so the
//!   composition reports what the clip says instead of inventing a cull.
//!
//! # Collision is composed on the clip's own record
//!
//! [`ColliderVerdict::NoCollider`] exactly when a playing clip hides the node
//! — F20-A's designed rule (`hidden ⇒ no collider`), carried unchanged from
//! the evaluator's `collider_enabled()` — and [`ColliderVerdict::Undecided`]
//! otherwise, because nothing in *this* composition decides collision for a
//! drawn node (its authored `CollisionRole` does, F11-C/F29) or for a disabled
//! one (`NodeDisabled` is a presentation marker; F11-C states that collision
//! and damage identity read their own records, never `NodePresentation`).
//! A collider verdict is never guessed. Because this half is not gated on the
//! draw reason, a node the clip hides carries no collider whether or not LOD
//! culls it, so a render consumer and a collision consumer cannot disagree
//! about whether the clip hid it.
//!
//! # What the original does here is still unknown
//!
//! Whether the original couples node visibility to collision at all is
//! **unmeasured** (F20-A's recorded unknown: the original animation container
//! layouts are undecoded, F13), and so is whether an original visibility swap
//! hid the node's whole **subtree** — the way `NodeDisabled` and a culled band
//! both propagate. Nothing here folds ancestors for the animation half: the
//! clip names one node, and a subtree rule would be a guess. This feature is
//! **designed**, not original-verified; F20-D keeps the validation gate.

use std::fmt;

use bevy::ecs::component::Component;
use bevy::ecs::world::World;
use bevy::prelude::Entity;
use cs_sim::animated_object::Visibility;

use crate::scene::{NodePresentation, PresentationState};

/// Component: the visibility the playing clip's visibility channel applied to
/// this node.
///
/// This is the clip's **fact**, written by [`super::advance_animation`] through
/// the same verified-binding path as the other three channels: a playing
/// instance of the live scene generation, a driven node, an aspect with a
/// reached key, and a write only when the value changed (idempotence, F20
/// non-negotiable behavior 3). An entity without the component has not been
/// driven by a visibility channel and keeps whatever base state it was spawned
/// with.
///
/// It is deliberately *not* the draw verdict: what the world does with this
/// value also depends on LOD and damage, and that composition is
/// [`VisibilityVerdict`], computed on read. A node this component reports
/// `Hidden` while it is [`NodeDisabled`](crate::scene::NodeDisabled) is not a
/// contradiction — the record is the clip's, the verdict is the world's.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeAnimatedVisibility {
    visibility: Visibility,
}

impl NodeAnimatedVisibility {
    /// Adopts the visibility a playing clip's channel evaluated.
    ///
    /// Both variants are accepted and neither is defaulted: the value always
    /// comes from the evaluator's reached key, so a consumer never has to tell
    /// "shown by the clip" from "assumed shown here".
    #[must_use]
    pub const fn new(visibility: Visibility) -> Self {
        Self { visibility }
    }

    /// The applied visibility, exactly as the evaluator reached it.
    #[must_use]
    pub const fn visibility(&self) -> Visibility {
        self.visibility
    }

    /// Whether the node keeps its collider under F20-A's designed rule.
    ///
    /// `hidden ⇒ no collider`, so a visibility swap cannot leave an invisible
    /// obstacle behind. **Designed, not observed**: whether the original
    /// couples visibility to collision is unmeasured and stays recorded as
    /// unknown. A node this component does not carry keeps its authored
    /// collider state — this method is only about the value a clip applied.
    #[must_use]
    pub const fn collider_enabled(&self) -> bool {
        !matches!(self.visibility, Visibility::Hidden)
    }
}

/// Why a node is not drawn, when it is not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawVerdict {
    /// The node is presented: nothing in LOD or damage opposes it and no
    /// playing clip hides it.
    Drawn,
    /// A playing clip's visibility channel hides the node. The animation's own
    /// reason, and the only one that is about the clip.
    HiddenByAnimation,
    /// LOD selected another variant of this node's group, so at this distance
    /// this node is not the one presented. LOD's reason, and it outranks the
    /// clip's — the clip's own record is unaffected.
    LodCulled,
    /// The node or an ancestor is disabled: damage's reason, and it outranks
    /// both, at any depth (F20 non-negotiable behavior 3).
    Disabled,
}

impl DrawVerdict {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Drawn => "drawn",
            Self::HiddenByAnimation => "hidden by animation",
            Self::LodCulled => "lod culled",
            Self::Disabled => "disabled",
        }
    }
}

/// What the composed verdict decides about the node's collision surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColliderVerdict {
    /// No collider: a playing clip hides the node, which is F20-A's designed
    /// `hidden ⇒ no collider` rule. This is the one collision fact this
    /// composition owns, and it does not depend on the draw verdict.
    NoCollider,
    /// This composition decides nothing about collision. A drawn node's
    /// collider is its authored collision role's business (F11-C/F29) and a
    /// culled or disabled node's is its damage record's; a consumer that needs
    /// that reads its own record instead of guessing from a draw verdict.
    Undecided,
}

impl ColliderVerdict {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::NoCollider => "no collider",
            Self::Undecided => "collision undecided",
        }
    }
}

/// The single composed verdict for one node: what a draw consumer and a
/// collision consumer both read, so the two cannot diverge.
///
/// Built by [`Self::compose`] from the two records that exist and read through
/// [`composed_visibility_verdict`]. See the module doc for the priority table
/// and for why the composition is computed on read instead of stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisibilityVerdict {
    draw: DrawVerdict,
    collider: ColliderVerdict,
}

impl VisibilityVerdict {
    /// Composes the verdict from the two records.
    ///
    /// `presentation` is the LOD/damage record
    /// ([`NodePresentation`], `None` when the LOD pass has never written one)
    /// and `animated` the clip's
    /// ([`NodeAnimatedVisibility`], `None` when no playing clip drives the
    /// node's visibility). Pure: the same two records always give the same
    /// verdict, so re-reading it after any pass cannot change the answer
    /// without a record having changed.
    #[must_use]
    pub const fn compose(
        presentation: Option<PresentationState>,
        animated: Option<Visibility>,
    ) -> Self {
        let hidden = matches!(animated, Some(Visibility::Hidden));
        let draw = match presentation {
            // Damage's record outranks everything, and it already carries the
            // ancestor fold, so a destroyed node is never re-drawn here.
            Some(PresentationState::Disabled) => DrawVerdict::Disabled,
            // LOD's own reason: at this distance another band is presented.
            Some(PresentationState::LodCulled) => DrawVerdict::LodCulled,
            // `Drawn`, and no record at all: nothing in LOD or damage opposes
            // the node, so the clip's own verdict decides.
            Some(PresentationState::Drawn) | None => {
                if hidden {
                    DrawVerdict::HiddenByAnimation
                } else {
                    DrawVerdict::Drawn
                }
            }
        };
        // The clip's hidden fact reaches collision whatever the draw reason
        // is; everything else is undecided here rather than guessed.
        let collider = if hidden {
            ColliderVerdict::NoCollider
        } else {
            ColliderVerdict::Undecided
        };
        Self { draw, collider }
    }

    /// Why the node is drawn, or not.
    #[must_use]
    pub const fn draw(self) -> DrawVerdict {
        self.draw
    }

    /// What this composition decides about the node's collider.
    #[must_use]
    pub const fn collider(self) -> ColliderVerdict {
        self.collider
    }

    /// Whether the node is drawn right now.
    #[must_use]
    pub const fn drawn(self) -> bool {
        matches!(self.draw, DrawVerdict::Drawn)
    }
}

impl fmt::Display for VisibilityVerdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.draw.label(), self.collider.label())
    }
}

/// The composed visibility verdict of one entity, read from the records that
/// exist right now.
///
/// This is **the** entry point for a presentation or collision consumer: it
/// reads [`NodePresentation`] and [`NodeAnimatedVisibility`] and returns the
/// composed [`VisibilityVerdict`]. Neither writer is consulted, neither is
/// trusted over the other, and nothing is cached — so the answer follows the
/// last LOD or damage pass and the last animation advance by construction,
/// with no schedule constraint to maintain and no stale copy to read.
///
/// An entity that carries neither record is reported
/// [`DrawVerdict::Drawn`]/[`ColliderVerdict::Undecided`]: absence of an
/// animation record is not a hide, and absence of an LOD record is not a cull.
#[must_use]
pub fn composed_visibility_verdict(world: &World, entity: Entity) -> VisibilityVerdict {
    VisibilityVerdict::compose(
        world
            .get::<NodePresentation>(entity)
            .map(|presentation| presentation.0),
        world
            .get::<NodeAnimatedVisibility>(entity)
            .map(NodeAnimatedVisibility::visibility),
    )
}
