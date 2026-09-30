//! The airframe → scene-root binding record
//! (`specs/F11-scene-hierarchy-aircraft-parts-sockets-and-lod.md`, stage
//! `### F11-A`; shared contract `docs/contracts/IDENTITY-CONTENT.md`).
//!
//! An airframe's visual is anchored by a [`SceneRootRef`]: the container the
//! node array lives in plus the root's stable [`ContentId`]. Nothing here
//! selects a model by mesh-array or node-array position (F11 deliverable),
//! and nothing asserts that a resolvable root means the airframe is
//! player-selectable — roster availability is a separate discovery (F11
//! non-negotiable behavior 3).
//!
//! This is the typed record only; the spawn path that attaches it to entities
//! is F11-C's.
//!
//! # F25-A: rotor visuals
//!
//! An exceptional airframe's rotors are declared here as
//! [`RotorVisualBinding`]s: the scene node that spins, plus the **explicit**
//! [`RotorSpeedMapping`] from the authoritative physical rotor rate to the rate
//! the mesh is drawn at (F25 non-negotiable behavior 3: "Physical and visual
//! rotor speeds may differ but require an explicit mapping"). The binding is
//! content-addressed like everything else here — a rotor node must descend from
//! the airframe's own root, or the binding is refused — and
//! it carries no time of its own: [`RotorVisualBinding::sample`] takes `&self`
//! and derives the drawn phase from the drive's physical rate plus the render
//! frame time, so a render frame can never reach the simulation.
//! [`RotorSpeedMapping`], [`RotorDrive`] and [`RotorVisualSample`] are the
//! `cs_sim` numeric types F25-A defines; mapping the roster's `Resolved` rotor
//! ratio into one is F25-C's.

use bevy::ecs::component::Component;
use cs_content::scene::{SceneError, SceneNodeId, SceneRootRef};
use cs_sim::flight::{RotorDrive, RotorSpeedMapping, RotorVisualSample, TelemetryError};
use cs_types::content::{ContentId, ContentKind};

/// Component: the canonical scene root an airframe's visual subtree hangs
/// from.
///
/// `airframe` is the `airframe/<key>` catalog element the visual serves;
/// `root` is the checked [`SceneRootRef`] naming the container and the root
/// node inside it.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct AirframeVisual {
    airframe: ContentId,
    root: SceneRootRef,
    rotors: Vec<RotorVisualBinding>,
}

/// Why an [`AirframeVisual`] was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum AirframeVisualError {
    /// The airframe id names another kind.
    AirframeKind {
        /// The kind the id actually names.
        kind: ContentKind,
    },
    /// The root reference failed its own contract.
    Root(SceneError),
    /// A rotor node does not live in the airframe's own container, so the
    /// binding would point at another tree's node.
    RotorNodeOutsideContainer {
        /// The container the airframe's root lives in.
        container: String,
        /// The refused rotor node's key.
        node: String,
    },
    /// A rotor node is in the container but does not descend from the airframe's
    /// own root, so it belongs to a different root in the same container.
    RotorNodeOutsideAirframe {
        /// The airframe root the rotor node would have to descend from.
        root: String,
        /// The refused rotor node's key.
        node: String,
    },
    /// The same rotor node was bound twice, which would make the drawn phase
    /// depend on which binding a consumer reached first.
    DuplicateRotorNode {
        /// The repeated node's key.
        node: String,
    },
    /// Deriving a rotor's visual sample was refused by the numeric boundary.
    RotorVisual(TelemetryError),
}

impl core::fmt::Display for AirframeVisualError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::AirframeKind { kind } => {
                write!(
                    f,
                    "an airframe visual must reference an airframe, got {kind}"
                )
            }
            Self::Root(error) => write!(f, "invalid scene root reference: {error}"),
            Self::RotorNodeOutsideContainer { container, node } => write!(
                f,
                "the rotor node {node} does not live in the airframe container {container}"
            ),
            Self::RotorNodeOutsideAirframe { root, node } => write!(
                f,
                "the rotor node {node} is not a part of the airframe rooted at {root}"
            ),
            Self::DuplicateRotorNode { node } => {
                write!(f, "the rotor node {node} is bound more than once")
            }
            Self::RotorVisual(error) => write!(f, "invalid rotor visual sample: {error}"),
        }
    }
}

impl std::error::Error for AirframeVisualError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::AirframeKind { .. }
            | Self::RotorNodeOutsideContainer { .. }
            | Self::RotorNodeOutsideAirframe { .. }
            | Self::DuplicateRotorNode { .. } => None,
            Self::Root(error) => Some(error),
            Self::RotorVisual(error) => Some(error),
        }
    }
}

impl From<TelemetryError> for AirframeVisualError {
    fn from(error: TelemetryError) -> Self {
        Self::RotorVisual(error)
    }
}

/// One rotor's visual binding: the node that spins and the explicit mapping
/// from the authoritative physical rate to the rate it is drawn at.
///
/// The binding is pure presentation. It holds no elapsed time, no accumulator
/// and no `&mut self` mutator, so rotor *animation* cannot become the source of
/// physics dt (F25 non-negotiable behavior 3): the physical rate comes from
/// [`RotorDrive`], which only a fixed simulation tick advances, and
/// [`RotorVisualBinding::sample`] derives the drawn phase from it.
#[derive(Clone, Debug, PartialEq)]
pub struct RotorVisualBinding {
    node: SceneNodeId,
    mapping: RotorSpeedMapping,
}

impl RotorVisualBinding {
    /// Binds one rotor node to its declared visual/physical mapping.
    #[must_use]
    pub const fn new(node: SceneNodeId, mapping: RotorSpeedMapping) -> Self {
        Self { node, mapping }
    }

    /// The scene node this binding spins.
    #[must_use]
    pub const fn node(&self) -> &SceneNodeId {
        &self.node
    }

    /// The declared physical-to-visual mapping.
    #[must_use]
    pub const fn mapping(&self) -> &RotorSpeedMapping {
        &self.mapping
    }

    /// Derives this rotor's drawn phase for one render frame.
    ///
    /// `render_dt_s` is the *render* frame time and reaches the picture only:
    /// the authoritative rate reported back comes from `rotor` and is identical
    /// however many times this is called per simulation tick.
    ///
    /// # Errors
    ///
    /// [`AirframeVisualError::RotorVisual`] for a non-finite or non-positive
    /// `render_dt_s`, or a non-finite `previous` phase.
    pub fn sample(
        &self,
        rotor: &RotorDrive,
        previous: RotorVisualSample,
        render_dt_s: f64,
    ) -> Result<RotorVisualSample, AirframeVisualError> {
        rotor
            .visual_sample(Some(&self.mapping), previous, render_dt_s)
            .map_err(AirframeVisualError::from)
    }
}

impl AirframeVisual {
    /// Binds an airframe element to its scene root.
    ///
    /// # Errors
    ///
    /// [`AirframeVisualError::AirframeKind`] when `airframe` is not an
    /// `airframe` id; [`SceneRootRef::new`]'s errors propagate as
    /// [`AirframeVisualError::Root`].
    pub fn new(
        airframe: ContentId,
        container: ContentId,
        root: cs_content::scene::SceneNodeId,
    ) -> Result<Self, AirframeVisualError> {
        if airframe.kind() != ContentKind::Airframe {
            return Err(AirframeVisualError::AirframeKind {
                kind: airframe.kind(),
            });
        }
        let root = SceneRootRef::new(container, root).map_err(AirframeVisualError::Root)?;
        Ok(Self {
            airframe,
            root,
            rotors: Vec::new(),
        })
    }

    /// The airframe element this visual serves.
    #[must_use]
    pub fn airframe(&self) -> &ContentId {
        &self.airframe
    }

    /// The checked container/root reference.
    #[must_use]
    pub fn root(&self) -> &SceneRootRef {
        &self.root
    }

    /// Binds one rotor node to this airframe's visual.
    ///
    /// The node is checked against the airframe's own root by content id —
    /// never by hierarchy position — so a rotor can only be drawn on a node of
    /// the tree this visual anchors: a node in another container, a node
    /// belonging to another root of the same container, and the airframe root
    /// itself (spinning the whole airframe is not a rotor) are each refused by
    /// name.
    ///
    /// # Errors
    ///
    /// [`AirframeVisualError::RotorNodeOutsideContainer`] when the node does not
    /// live in this airframe's container,
    /// [`AirframeVisualError::RotorNodeOutsideAirframe`] when the node does not
    /// descend from this airframe's root, and
    /// [`AirframeVisualError::DuplicateRotorNode`] when that node is already
    /// bound.
    pub fn bind_rotor(&mut self, binding: RotorVisualBinding) -> Result<(), AirframeVisualError> {
        let container = self.root.container().key();
        let node = binding.node.key();
        if !node.starts_with(&format!("{container}.")) {
            return Err(AirframeVisualError::RotorNodeOutsideContainer {
                container: container.to_owned(),
                node: node.to_owned(),
            });
        }
        let airframe_root = self.root.root().key();
        if !node.starts_with(&format!("{airframe_root}.")) {
            return Err(AirframeVisualError::RotorNodeOutsideAirframe {
                root: airframe_root.to_owned(),
                node: node.to_owned(),
            });
        }
        if self
            .rotors
            .iter()
            .any(|existing| existing.node == binding.node)
        {
            return Err(AirframeVisualError::DuplicateRotorNode {
                node: node.to_owned(),
            });
        }
        self.rotors.push(binding);
        Ok(())
    }

    /// Every rotor this airframe's visual binds, in binding order.
    #[must_use]
    pub fn rotors(&self) -> &[RotorVisualBinding] {
        &self.rotors
    }

    /// The binding for `node`, or `None` when this airframe has no such rotor.
    #[must_use]
    pub fn rotor(&self, node: &SceneNodeId) -> Option<&RotorVisualBinding> {
        self.rotors.iter().find(|binding| &binding.node == node)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cs_sim::flight::{SYNTHETIC_TICK_DT_S, synthetic_rotor_drive, synthetic_rotor_mapping};
    use cs_types::Tick;

    fn cid(kind: ContentKind, key: &str) -> ContentId {
        ContentId::from_source(kind, key).expect("test id is valid")
    }

    /// An airframe visual references a root *by content id*: the right kind
    /// is enforced, a nested node is not a root, and a root from another
    /// container is refused.
    #[test]
    fn accept_f11_a_airframe_visual_references_roots_by_id_not_position() {
        let airframe = cid(ContentKind::Airframe, "corsair");
        let planes = cid(ContentKind::InstallFile, "planes");
        let root = SceneNodeId::from_content_id(cid(ContentKind::SceneNode, "planes.corsair"))
            .expect("scene node id");

        let visual = AirframeVisual::new(airframe.clone(), planes.clone(), root.clone())
            .expect("valid airframe visual");
        assert_eq!(visual.airframe(), &airframe);
        assert_eq!(visual.root().container(), &planes);
        assert_eq!(visual.root().root(), &root);

        // A non-airframe id cannot serve as the airframe.
        let wrong_kind = cid(ContentKind::Mesh, "planes.1");
        assert_eq!(
            AirframeVisual::new(wrong_kind, planes.clone(), root.clone()),
            Err(AirframeVisualError::AirframeKind {
                kind: ContentKind::Mesh
            })
        );

        // A nested node is not a root a definition can reference.
        let nested =
            SceneNodeId::from_content_id(cid(ContentKind::SceneNode, "planes.corsair.wing_l"))
                .expect("scene node id");
        assert_eq!(
            AirframeVisual::new(airframe.clone(), planes.clone(), nested),
            Err(AirframeVisualError::Root(SceneError::NotARootNode {
                root: "planes.corsair.wing_l".to_owned()
            }))
        );

        // A root living in another container is refused.
        let other = SceneNodeId::from_content_id(cid(ContentKind::SceneNode, "gamez.corsair"))
            .expect("scene node id");
        assert_eq!(
            AirframeVisual::new(airframe, planes, other),
            Err(AirframeVisualError::Root(
                SceneError::RootOutsideContainer {
                    container: "planes".to_owned(),
                    root: "gamez.corsair".to_owned(),
                }
            ))
        );
    }

    /// A rotor binds by content id inside the airframe's own root: a node
    /// from another tree, a node belonging to a sibling root of the same
    /// container and the airframe root itself are all refused, a repeated node
    /// is refused, and sampling the visual cannot move the authoritative rotor
    /// rate however many render frames a simulation tick contains.
    #[test]
    fn accept_f25_a_rotor_visual_binds_by_id_and_cannot_drive_physics() {
        let planes = cid(ContentKind::InstallFile, "planes");
        let root = SceneNodeId::from_content_id(cid(ContentKind::SceneNode, "planes.corsair"))
            .expect("scene node id");
        let root_key = root.clone();
        let rotor_node =
            SceneNodeId::from_content_id(cid(ContentKind::SceneNode, "planes.corsair.rotor_main"))
                .expect("scene node id");

        let mut visual =
            AirframeVisual::new(cid(ContentKind::Airframe, "corsair"), planes.clone(), root)
                .expect("valid airframe visual");
        assert!(visual.rotors().is_empty());

        let mapping = synthetic_rotor_mapping();
        let binding = RotorVisualBinding::new(rotor_node.clone(), mapping.clone());
        assert_eq!(binding.node(), &rotor_node);
        assert_eq!(binding.mapping(), &mapping);
        visual
            .bind_rotor(binding.clone())
            .expect("a rotor under the airframe's own root binds");
        assert_eq!(visual.rotors(), std::slice::from_ref(&binding));
        assert!(visual.rotor(&rotor_node).is_some());

        // The same node twice would make the drawn phase depend on lookup order.
        assert_eq!(
            visual.bind_rotor(binding.clone()),
            Err(AirframeVisualError::DuplicateRotorNode {
                node: "planes.corsair.rotor_main".to_owned()
            })
        );

        // A rotor in another container is not this airframe's rotor.
        let foreign =
            SceneNodeId::from_content_id(cid(ContentKind::SceneNode, "gamez.corsair.rotor"))
                .expect("scene node id");
        assert_eq!(
            visual.bind_rotor(RotorVisualBinding::new(foreign, mapping.clone())),
            Err(AirframeVisualError::RotorNodeOutsideContainer {
                container: "planes".to_owned(),
                node: "gamez.corsair.rotor".to_owned()
            })
        );

        // Another root of the same container is another airframe: binding its
        // part would spin this airframe's rotor on somebody else's node.
        let sibling_root =
            SceneNodeId::from_content_id(cid(ContentKind::SceneNode, "planes.corsair_mk2"))
                .expect("scene node id");
        let sibling_part = SceneNodeId::from_content_id(cid(
            ContentKind::SceneNode,
            "planes.corsair_mk2.rotor_main",
        ))
        .expect("scene node id");
        assert_eq!(
            visual.bind_rotor(RotorVisualBinding::new(sibling_part, mapping.clone())),
            Err(AirframeVisualError::RotorNodeOutsideAirframe {
                root: root_key.key().to_owned(),
                node: "planes.corsair_mk2.rotor_main".to_owned()
            })
        );

        // The airframe root itself is the whole airframe, not one of its rotors.
        let mut sibling_visual = AirframeVisual::new(
            cid(ContentKind::Airframe, "corsair_mk2"),
            planes.clone(),
            sibling_root.clone(),
        )
        .expect("valid airframe visual");
        assert_eq!(
            sibling_visual.bind_rotor(RotorVisualBinding::new(sibling_root, mapping.clone())),
            Err(AirframeVisualError::RotorNodeOutsideAirframe {
                root: "planes.corsair_mk2".to_owned(),
                node: "planes.corsair_mk2".to_owned()
            })
        );
        assert_eq!(
            visual.rotors().len(),
            1,
            "a refused binding changes nothing"
        );

        // Drawing the rotor at one frame per tick and at many frames per tick
        // leaves the simulation identical.
        let draw = |frames_per_tick: u32| {
            let mut drive = synthetic_rotor_drive();
            let mut sample = RotorVisualSample::at_rest();
            let frame_dt = SYNTHETIC_TICK_DT_S / f64::from(frames_per_tick);
            for tick in 1..=120u64 {
                drive
                    .advance_tick(40.0, 48.0, Tick(tick), SYNTHETIC_TICK_DT_S)
                    .expect("each tick is newer than the last");
                for _ in 0..frames_per_tick {
                    sample = binding
                        .sample(&drive, sample, frame_dt)
                        .expect("the drawn sample is finite");
                }
            }
            (drive, sample)
        };
        let (slow_drive, slow) = draw(1);
        let (fast_drive, fast) = draw(60);
        assert_eq!(
            slow_drive.physical_speed_radps(),
            fast_drive.physical_speed_radps()
        );
        assert!(slow.phase_rad >= 0.0);
        assert!((slow.phase_rad - fast.phase_rad).abs() < 1e-9);
        assert_eq!(
            slow.visual_speed_radps(),
            Some(slow.physical_speed_radps * 1.5),
            "the declared mapping drives the drawn rate"
        );

        // A render frame time that is not positive cannot reach the picture.
        assert!(matches!(
            binding.sample(&slow_drive, slow, 0.0),
            Err(AirframeVisualError::RotorVisual(
                TelemetryError::NonPositive {
                    field: "rotor_visual.render_dt_s"
                }
            ))
        ));
        let _ = planes;
    }
}
