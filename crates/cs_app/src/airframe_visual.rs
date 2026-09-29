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

use bevy::ecs::component::Component;
use cs_content::scene::{SceneError, SceneRootRef};
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
        }
    }
}

impl std::error::Error for AirframeVisualError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::AirframeKind { .. } => None,
            Self::Root(error) => Some(error),
        }
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
        Ok(Self { airframe, root })
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use cs_content::scene::SceneNodeId;

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
}
