//! World-object identity on entities and the contact log (F18-A).
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
//! stage `### F18-A`.
//!
//! Every entity a world instance produces carries a [`WorldObjectBinding`]:
//! the world, the object's stable id, its sector membership and the
//! provenance of the record it came from. That binding is what lets a
//! contact answer *"which authored object was hit, in which sector"* — the
//! question F18's surface rules and streaming both need.
//!
//! [`record_world_contacts`] reads Avian's [`CollisionStart`] and appends a
//! [`WorldContact`] to the [`WorldContacts`] log. It only records pairs with
//! **exactly one** world collider: an actor touching world geometry is a
//! gameplay contact, while two static world objects resting against each
//! other is authoring geometry, not gameplay.

use avian3d::prelude::{CollisionStart, PhysicsSystems};
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::prelude::{
    Component, Entity, FixedPostUpdate, MessageReader, Plugin, Query, ResMut, Resource,
};
use cs_content::world::{SectorId, WorldCollisionRole, WorldId, WorldObjectId};
use cs_types::content::Provenance;

/// The identity a spawned world entity carries.
///
/// One binding per object instance, cloned onto both the visual entity and
/// the collider entity, so a contact and a draw resolve to the same authored
/// id.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct WorldObjectBinding {
    world: WorldId,
    object: WorldObjectId,
    sectors: Vec<SectorId>,
    provenance: Provenance,
}

impl WorldObjectBinding {
    /// Builds a binding from the record's own parts.
    #[must_use]
    pub fn new(
        world: WorldId,
        object: WorldObjectId,
        sectors: Vec<SectorId>,
        provenance: Provenance,
    ) -> Self {
        Self {
            world,
            object,
            sectors,
            provenance,
        }
    }

    /// The world this object belongs to.
    #[must_use]
    pub fn world(&self) -> &WorldId {
        &self.world
    }

    /// The object's stable identity.
    #[must_use]
    pub fn object(&self) -> &WorldObjectId {
        &self.object
    }

    /// The sectors the object belongs to; empty for a resident object.
    #[must_use]
    pub fn sectors(&self) -> &[SectorId] {
        &self.sectors
    }

    /// The provenance of the record the binding came from.
    #[must_use]
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// Marks the entity that *presents* one object instance.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorldVisual;

/// Marks the entity that *blocks* or *reports* one object instance, and
/// remembers which role it was built with.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorldColliderInstance {
    role: WorldCollisionRole,
}

impl WorldColliderInstance {
    /// Builds the marker with the role the record declared.
    #[must_use]
    pub const fn new(role: WorldCollisionRole) -> Self {
        Self { role }
    }

    /// The declared role of this collider.
    #[must_use]
    pub const fn role(&self) -> WorldCollisionRole {
        self.role
    }
}

/// One recorded contact between an actor and a world object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldContact {
    /// The world whose object was touched.
    pub world: WorldId,
    /// The authored object that was touched.
    pub object: WorldObjectId,
    /// The sectors that object belongs to at the moment of contact.
    pub sectors: Vec<SectorId>,
    /// The role the touched collider was built with.
    pub role: WorldCollisionRole,
    /// The actor entity that touched it.
    pub other: Entity,
}

/// The world's contact log for the current session.
///
/// The log is append-only within a run; gameplay drains it, and a test reads
/// it to prove which authored object a body actually reached.
#[derive(Resource, Default, Debug)]
pub struct WorldContacts {
    contacts: Vec<WorldContact>,
}

impl WorldContacts {
    /// Every recorded contact, oldest first.
    #[must_use]
    pub fn contacts(&self) -> &[WorldContact] {
        &self.contacts
    }

    /// The distinct authored objects that were touched, in first-contact
    /// order.
    #[must_use]
    pub fn objects_touched(&self) -> Vec<&WorldObjectId> {
        let mut seen: Vec<&WorldObjectId> = Vec::new();
        for contact in &self.contacts {
            if !seen.contains(&&contact.object) {
                seen.push(&contact.object);
            }
        }
        seen
    }

    /// Appends a contact.
    pub fn record(&mut self, contact: WorldContact) {
        self.contacts.push(contact);
    }

    /// Drops every recorded contact.
    pub fn clear(&mut self) {
        self.contacts.clear();
    }
}

/// Records actor↔world contacts into [`WorldContacts`].
///
/// Runs in `FixedPostUpdate` after the physics step, so the same tick's
/// contacts are already in the log when the tick's gameplay reads it.
///
/// Observable failure if this system is removed or scheduled before the
/// step: [`WorldContacts`] stays empty while bodies visibly hit world
/// geometry.
pub fn record_world_contacts(
    mut reader: MessageReader<CollisionStart>,
    colliders: Query<&WorldColliderInstance>,
    bindings: Query<&WorldObjectBinding>,
    mut log: ResMut<WorldContacts>,
) {
    for event in reader.read() {
        let world_first = colliders.get(event.collider1).is_ok();
        let world_second = colliders.get(event.collider2).is_ok();
        // Exactly one side may be world geometry: two static world objects
        // touching is authoring, not gameplay.
        if world_first == world_second {
            continue;
        }
        let (world_entity, other) = if world_first {
            (event.collider1, event.body2.unwrap_or(event.collider2))
        } else {
            (event.collider2, event.body1.unwrap_or(event.collider1))
        };
        let Ok(binding) = bindings.get(world_entity) else {
            continue;
        };
        let Ok(marker) = colliders.get(world_entity) else {
            continue;
        };
        log.record(WorldContact {
            world: binding.world().clone(),
            object: binding.object().clone(),
            sectors: binding.sectors().to_vec(),
            role: marker.role(),
            other,
        });
    }
}

/// Installs the world contact log and its reader.
///
/// The order is load-bearing: contacts are emitted inside the physics step,
/// so the reader must run after it or the log would lag a tick behind (and a
/// body that hits and stops on one tick would report on the next).
pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut bevy::prelude::App) {
        app.init_resource::<WorldContacts>().add_systems(
            FixedPostUpdate,
            record_world_contacts.after(PhysicsSystems::StepSimulation),
        );
    }
}
