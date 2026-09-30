//! F18-C: safe visibility and streaming — what a focus holds, what it holds
//! anyway, and what a streamed-out sector is summarized as.
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
//! stage `### F18-C`. Task test prefix: `accept_f18_c_`.
//!
//! These tests drive `cs_app::world::{update_visibility, holds_sector,
//! retained_sectors, VisibilityRequest}` on the same depot world
//! `cs_app::world::depot_world` the overlay tests fly through, and they read the
//! world's own entities rather than the residency record wherever the claim is
//! about geometry. Nothing here carries its own streaming rule.
//!
//! What is pinned here:
//!
//! * **the hold**: a sector the focus is within `radius` of stays loaded, and one
//!   it is not is unloaded — with the door's own sector **held** when the mission
//!   declares the panel gameplay-required (F18 non-negotiable behavior 3).
//! * **the summary**: a sector that *was* streamed away comes back with the
//!   condition and the applied overlay the load already held, so a door that
//!   opened before its sector went is open when it returns.
//! * **the refusals**: no world loaded, a non-finite focus, a non-finite radius
//!   and a negative radius each name themselves and move nothing.
//! * **the geometry of the decision**, which is pure and needs no app: a sector
//!   is held by point-to-box distance, not by distance to its centre, so a focus
//!   just past a sector's far face does not keep it.

use avian3d::prelude::Position;
use bevy::prelude::{App, Entity, With};
use cs_app::world::{
    DEPOT_OBJECT_CRATE, DEPOT_OBJECT_DOOR, DEPOT_OBJECT_HANGAR, DEPOT_SECTOR_ANNEX,
    DEPOT_SECTOR_APPROACH, DEPOT_SECTOR_YARD, MESH_SETTLE_UPDATES, ObjectCondition, SpawnedWorld,
    VisibilityError, VisibilityRequest, VisibilityUpdate, WorldLoadError, WorldObjectBinding,
    condition_of, damage_object, depot_meshes, depot_mission, depot_world, holds_sector,
    load_sector, load_world, residency, retained_sectors, unload_sector, update_visibility,
    world_app,
};
use cs_content::world::{SectorId, WorldDefinition, WorldObjectCondition, WorldObjectId};

/// The depot world, built by production code.
fn depot() -> WorldDefinition {
    depot_world().expect("the synthetic depot world is well formed")
}

fn object(key: &str) -> WorldObjectId {
    WorldObjectId::new(key).expect("the fixture object key is valid")
}

/// The annex crate's id, for a match arm where the binding shadows [`object`].
fn annex_crate() -> WorldObjectId {
    object(DEPOT_OBJECT_CRATE)
}

fn sector(key: &str) -> SectorId {
    SectorId::new(key).expect("the fixture sector key is valid")
}

/// A focus far west of the depot, with a radius that reaches only the approach.
///
/// The approach's own bounds are `x ∈ [-40, -1]`, the yard's `x ∈ [-1, 14]` and
/// the annex's `x ∈ [20, 40]`, so a focus at `x = -35` with a 2 m radius is
/// inside the approach and 3 m clear of the yard's near face.
const FAR_WEST: VisibilityRequest = VisibilityRequest {
    focus_m: [-35.0, 1.5, 0.0],
    radius_m: 2.0,
};

/// A focus inside the yard, with a radius that also reaches the annex.
const IN_THE_YARD: VisibilityRequest = VisibilityRequest {
    focus_m: [0.0, 1.5, 0.0],
    radius_m: 30.0,
};

/// The headless world with the depot world loaded and settled.
fn loaded(required: &[&str]) -> (App, SpawnedWorld) {
    let definition = depot();
    let instance = depot_mission(&definition, true, required).expect("a valid depot mission");
    let mut app = world_app();
    let report = load_world(&mut app, &definition, &instance, &depot_meshes())
        .expect("the depot world loads");
    for _ in 0..MESH_SETTLE_UPDATES {
        app.update();
    }
    (app, report)
}

fn meshes() -> cs_app::world::WorldMeshes {
    depot_meshes()
}

/// Every entity the Bevy world currently holds for `object`, read through the
/// binding every entity an object owns carries.
fn entities_of(app: &mut App, object: &WorldObjectId) -> Vec<Entity> {
    let world = app.world_mut();
    let mut query = world.query_filtered::<Entity, With<WorldObjectBinding>>();
    let mut entities: Vec<Entity> = query
        .iter(world)
        .filter(|entity| {
            world
                .get::<WorldObjectBinding>(*entity)
                .is_some_and(|binding| binding.object() == object)
        })
        .collect();
    entities.sort_unstable();
    entities
}

fn loaded_sectors(app: &App) -> Vec<String> {
    residency(app.world())
        .expect("a world is loaded")
        .resident()
        .loaded_sectors()
        .iter()
        .map(|sector| sector.as_str().to_owned())
        .collect()
}

/// **A visibility pass holds the sector a gameplay-required object is in, and
/// streams the ones nothing needs.**
///
/// The depot has three sectors: the approach the focus is standing in, the yard
/// with the hangar, the panel and the ground, and an annex with a crate. The
/// mission declares the panel gameplay-required, so the yard is held even though
/// the focus cannot see it, while the annex goes. That is the first half of F18
/// non-negotiable behavior 3 — a gameplay-required object is *still simulated*,
/// still collidable, not summarized — and it is the difference between a
/// streaming policy and a policy that eats the mission.
///
/// The geometry is read from the world: the crate's entities are gone, the
/// panel's are not, and the residency record agrees.
///
/// Observable failure: the yard is unloaded, the crate stays, or the panel's
/// collider is gone while the record claims it is present.
#[test]
fn accept_f18_c_a_visibility_pass_holds_a_required_objects_sector_and_streams_the_rest() {
    let (mut app, _report) = loaded(&[DEPOT_OBJECT_DOOR]);
    assert_eq!(
        loaded_sectors(&app),
        vec![
            DEPOT_SECTOR_ANNEX.to_owned(),
            DEPOT_SECTOR_APPROACH.to_owned(),
            DEPOT_SECTOR_YARD.to_owned(),
        ],
        "a load brings every declared sector in"
    );

    let update = update_visibility(&mut app, &FAR_WEST, &meshes()).expect("the pass runs");
    assert_eq!(
        update.retained,
        vec![sector(DEPOT_SECTOR_YARD)],
        "the yard is out of the focus's range and is held anyway: {:?}",
        update
    );
    assert_eq!(update.loaded, Vec::<SectorId>::new(), "nothing had to load");
    assert_eq!(
        update.unloaded,
        vec![sector(DEPOT_SECTOR_ANNEX)],
        "the annex holds nothing the mission needs, so it goes: {:?}",
        update
    );
    assert_eq!(
        update.moved,
        vec![sector(DEPOT_SECTOR_ANNEX)],
        "and the report names what moved: {:?}",
        update
    );
    assert!(!update.is_empty(), "the pass moved a sector");

    // The world itself: the crate is gone, the panel is not.
    assert!(
        entities_of(&mut app, &object(DEPOT_OBJECT_CRATE)).is_empty(),
        "the annex crate's entities are gone"
    );
    let panel = entities_of(&mut app, &object(DEPOT_OBJECT_DOOR));
    assert!(
        !panel.is_empty(),
        "and the panel gameplay requires is still simulated: {panel:?}"
    );
    for entity in &panel {
        assert!(
            app.world().get::<Position>(*entity).is_some()
                || app
                    .world()
                    .get::<bevy::prelude::Transform>(*entity)
                    .is_some(),
            "entity {entity:?} still carries a pose, so it is a live body and not a \
             leftover marker"
        );
    }
    assert_eq!(
        loaded_sectors(&app),
        vec![
            DEPOT_SECTOR_APPROACH.to_owned(),
            DEPOT_SECTOR_YARD.to_owned(),
        ],
        "and the residency agrees: the approach and the yard are loaded"
    );
    assert!(
        residency(app.world())
            .expect("a world is loaded")
            .resident()
            .is_present(&object(DEPOT_OBJECT_HANGAR)),
        "the hangar in the held sector is still present too"
    );
}

/// **A sector that was streamed away comes back summarized: the condition and the
/// applied overlay the load already held.**
///
/// The other half of F18 non-negotiable behavior 3. This mission requires
/// nothing, so the yard *does* go: the panel's entities are destroyed. The load
/// keeps what it knows — the panel's condition, and the fact that its door
/// overlay has been applied — and the sector load re-applies both to the fresh
/// entities, so the panel comes back **open** rather than snapping shut, and a
/// damaged one comes back damaged.
///
/// Without the re-application, a player who opened a door, flew far enough away
/// for the sector to stream out, and came back would find the door shut and the
/// mission's progress with it.
///
/// Observable failure: the panel returns shut, the condition returns sound, the
/// applied set is empty after the reload, or the reload reports a duplicate.
#[test]
fn accept_f18_c_a_streamed_out_object_returns_with_the_state_its_load_holds() {
    let (mut app, _report) = loaded(&[]);
    let door = object(DEPOT_OBJECT_DOOR);
    let trigger = object("trigger.depot");

    // Gameplay opens the door and damages the shell, then the sector goes.
    cs_app::world::apply_overlay(app.world_mut(), &trigger).expect("the door opens");
    damage_object(&mut app, &object(DEPOT_OBJECT_HANGAR)).expect("the shell is damaged");
    let (drawn_before, _, collided_before) = panel_pose(&app);
    assert!(
        (collided_before.z - 2.0).abs() < 1e-5,
        "the panel is open before anything streams: {collided_before:?}"
    );

    let update = update_visibility(&mut app, &FAR_WEST, &meshes()).expect("the pass runs");
    assert_eq!(
        update.unloaded,
        vec![sector(DEPOT_SECTOR_ANNEX), sector(DEPOT_SECTOR_YARD)],
        "with nothing required, the yard goes with the annex: the focus is inside \
         the approach, so that one is held by being in range, and the other two \
         are out of it: {:?}",
        update
    );
    assert!(
        update.retained.is_empty(),
        "and nothing was retained, because this mission requires nothing: {:?}",
        update
    );
    assert!(
        entities_of(&mut app, &door).is_empty(),
        "and the panel's entities are gone"
    );
    assert_eq!(
        condition_of(app.world(), &object(DEPOT_OBJECT_HANGAR)),
        Some(WorldObjectCondition::Damaged),
        "but the load still holds the damage, which is the only honest summary of \
         an object outside render visibility"
    );
    assert_eq!(
        residency(app.world())
            .expect("a world is loaded")
            .resident()
            .applied_overlays(),
        &std::collections::BTreeSet::from([trigger.clone()]),
        "and the applied overlay, so it is not re-decided as if it had never fired"
    );

    // The focus comes back, and the sector with it.
    let update = update_visibility(&mut app, &IN_THE_YARD, &meshes()).expect("the pass runs");
    assert_eq!(
        update.loaded,
        vec![sector(DEPOT_SECTOR_ANNEX), sector(DEPOT_SECTOR_YARD)],
        "the sectors the focus now reaches come back, and the approach — still \
         loaded from the first pass, because the focus was standing in it — is \
         not loaded twice: {:?}",
        update
    );
    let panel = entities_of(&mut app, &door);
    assert_eq!(
        panel.len(),
        2,
        "with one entity per half again, and no duplicate: {panel:?}"
    );
    let (drawn_after, _, collided_after) = panel_pose(&app);
    assert!(
        (collided_after.z - 2.0).abs() < 1e-5,
        "the panel is **open**, because the load had already applied the overlay \
         when it was rebuilt: {collided_after:?}"
    );
    assert!(
        (drawn_after - drawn_before).length() < 1e-5
            && (collided_after - collided_before).length() < 1e-5,
        "and it is in exactly the place it was before the sector went: {drawn_before:?} \
         / {collided_before:?} against {drawn_after:?} / {collided_after:?}"
    );
    assert_eq!(
        condition_of(app.world(), &object(DEPOT_OBJECT_HANGAR)),
        Some(WorldObjectCondition::Damaged),
        "the reloaded shell is still the damaged one"
    );
    for entity in entities_of(&mut app, &object(DEPOT_OBJECT_HANGAR)) {
        assert_eq!(
            app.world()
                .get::<ObjectCondition>(entity)
                .map(|c| c.condition()),
            Some(WorldObjectCondition::Damaged),
            "and the component a query reads agrees with the record"
        );
    }
    assert_eq!(
        residency(app.world())
            .expect("a world is loaded")
            .resident()
            .applied_overlays(),
        &std::collections::BTreeSet::from([trigger]),
        "and the applied set did not grow on the reload: 'once' is still the load's rule"
    );
}

/// The door panel's drawn and collided translations, read from the world.
fn panel_pose(
    app: &App,
) -> (
    bevy::prelude::Vec3,
    bevy::prelude::Vec3,
    bevy::prelude::Vec3,
) {
    let resident = residency(app.world()).expect("a world is loaded");
    let spawned = resident
        .resident()
        .object(&object(DEPOT_OBJECT_DOOR))
        .expect("the panel is present");
    let collider = spawned
        .collider
        .as_ref()
        .expect("a solid cuboid object has a collider")
        .entity;
    (
        app.world()
            .get::<bevy::prelude::Transform>(spawned.visual)
            .expect("the panel's presentation entity exists")
            .translation,
        app.world()
            .get::<bevy::prelude::GlobalTransform>(spawned.visual)
            .expect("the panel's presentation entity exists")
            .translation(),
        app.world()
            .get::<Position>(collider)
            .expect("the panel's collider entity exists")
            .0,
    )
}

/// **Every visibility refusal names what it refused, and moved nothing.**
///
/// A streaming pass is the one place a bad number could quietly empty a world, so
/// each of the four refusals is checked for the residency it left behind as well
/// as for what it said: nothing loaded, a non-finite focus axis, a non-finite
/// radius and a negative radius. A negative radius is refused rather than made
/// absolute, because "the focus point itself" is the only reading of it and
/// clamping would turn a broken producer into a working-looking one.
#[test]
fn accept_f18_c_every_visibility_refusal_names_what_it_refused_and_moved_nothing() {
    // Nothing loaded at all.
    let mut empty = world_app();
    assert_eq!(
        update_visibility(&mut empty, &FAR_WEST, &meshes()),
        Err(VisibilityError::NoResidentWorld),
        "a pass with no world loaded says so"
    );

    let (mut app, _report) = loaded(&[DEPOT_OBJECT_DOOR]);
    let before = loaded_sectors(&app);
    for (request, expected) in [
        (
            VisibilityRequest::new([f64::NAN, 1.5, 0.0], 2.0),
            VisibilityError::UnusableFocus { axis: 0 },
        ),
        (
            VisibilityRequest::new([0.0, f64::INFINITY, 0.0], 2.0),
            VisibilityError::UnusableFocus { axis: 1 },
        ),
        (
            VisibilityRequest::new([0.0, 1.5, 0.0], f64::NAN),
            VisibilityError::UnusableRadius,
        ),
        (
            VisibilityRequest::new([0.0, 1.5, 0.0], -1.0),
            VisibilityError::NegativeRadius { radius_m: -1.0 },
        ),
    ] {
        assert_eq!(
            update_visibility(&mut app, &request, &meshes()),
            Err(expected),
            "a request naming {request:?} is refused"
        );
        assert_eq!(
            loaded_sectors(&app),
            before,
            "and moved no sector: {before:?}"
        );
        assert!(
            !entities_of(&mut app, &object(DEPOT_OBJECT_CRATE)).is_empty(),
            "and the annex crate is still there: a bad radius must never be the \
             reason a world empties itself"
        );
    }
}

/// **The decision is point-to-box distance, and it is pure.**
///
/// A sector is held when the focus is within the radius of the box itself, not
/// of its centre: a focus a metre past a sector's far face is one metre from the
/// sector and a hundred from its middle, and only the first is a question about
/// visibility. The second would keep a sector alive long after the player could
/// not see any of it, which is memory the player pays for without seeing.
///
/// The pure decision is asserted without an app, because it is the part a
/// reviewer can check by reading: which sectors a focus holds, and which of those
/// a required object holds anyway.
#[test]
fn accept_f18_c_the_policy_holds_by_point_to_box_distance_and_holds_a_required_sector() {
    let definition = depot();
    let yard = sector(DEPOT_SECTOR_YARD);
    let annex = sector(DEPOT_SECTOR_ANNEX);
    let approach = sector(DEPOT_SECTOR_APPROACH);

    // Inside the yard, with no radius: the yard is held because the focus is in
    // it, the annex is 20 m away and is not.
    let in_the_yard = VisibilityRequest::new([0.0, 1.5, 0.0], 0.0);
    assert!(holds_sector(&definition, &in_the_yard, &yard));
    assert!(!holds_sector(&definition, &in_the_yard, &annex));
    assert!(!holds_sector(&definition, &in_the_yard, &approach));

    // Just past the yard's far face (x = 14) by one metre: held at a 2 m radius
    // because the *box* is a metre away, and not held at 0.5 m even though the
    // box's centre is 7.5 m away in both cases.
    let past_the_face = VisibilityRequest::new([15.0, 1.5, 0.0], 2.0);
    assert!(
        holds_sector(&definition, &past_the_face, &yard),
        "a focus 1 m past a sector's far face is 1 m from the sector"
    );
    let just_past = VisibilityRequest::new([15.0, 1.5, 0.0], 0.5);
    assert!(
        !holds_sector(&definition, &just_past, &yard),
        "and with a half-metre radius it is not: the distance is to the box, not \
         to its centre"
    );
    // Far away, nothing is held however large the radius.
    let nowhere = VisibilityRequest::new([1000.0, 1.5, 0.0], 10.0);
    assert!(!holds_sector(&definition, &nowhere, &yard));

    // The required set holds a sector the focus cannot see, and *only* sectors
    // that are out of range: a sector in range is not "retained", it is held.
    let required = std::collections::BTreeSet::from([object(DEPOT_OBJECT_DOOR)]);
    assert_eq!(
        retained_sectors(&definition, &in_the_yard, &required),
        std::collections::BTreeSet::new(),
        "a sector in range needs no retention"
    );
    assert_eq!(
        retained_sectors(&definition, &nowhere, &required),
        std::collections::BTreeSet::from([yard.clone()]),
        "the yard is retained for the panel gameplay requires"
    );
    assert_eq!(
        retained_sectors(
            &definition,
            &nowhere,
            &std::collections::BTreeSet::from([object(DEPOT_OBJECT_CRATE)]),
        ),
        std::collections::BTreeSet::from([annex.clone()]),
        "and the annex for the crate, so the rule follows the load's declaration \
         rather than a hard-coded sector"
    );
    assert_eq!(
        retained_sectors(&definition, &nowhere, &std::collections::BTreeSet::new()),
        std::collections::BTreeSet::new(),
        "with nothing required, nothing is retained"
    );
    // The resident object: it names no sector, so it needs no retention either.
    let _ = approach;
}

/// **A pass that cannot finish says so, and leaves the world it has.**
///
/// The pass decides everything and then moves it, loads before unloads, so a
/// sector it cannot move is reported by name instead of swallowed. This reaches
/// that by despawning a presented entity behind the residency record's back —
/// the one inconsistency `VanishedEntity` exists for — and then asking for a
/// pass that has to stream the sector holding it.
///
/// Two things are checked: the error **names** the object, and the sector the
/// pass would have unloaded is still loaded, because the failure happened before
/// it moved. The order is the point: a pass that unloaded first could have
/// removed geometry and then failed to put anything back.
///
/// Observable failure: the pass returns `Ok` with a silently missing sector, or
/// the refusal is swallowed and the caller is told a sector moved that did not.
#[test]
fn accept_f18_c_a_pass_that_cannot_move_a_sector_says_which_and_keeps_it() {
    let (mut app, report) = loaded(&[]);
    // The annex crate is a presented object; take its collider away behind the
    // residency record's back, so unloading the annex cannot succeed.
    let crate_report = report
        .object(&object(DEPOT_OBJECT_CRATE))
        .expect("the annex crate is in the report");
    let collider = crate_report
        .collider
        .as_ref()
        .expect("a solid cuboid object has a collider")
        .entity;
    app.world_mut().entity_mut(collider).despawn();

    let error = update_visibility(&mut app, &FAR_WEST, &meshes())
        .expect_err("the pass cannot move a sector whose object has lost an entity");
    assert!(
        matches!(&error, VisibilityError::Sector(WorldLoadError::VanishedEntity { object, .. })
            if *object == annex_crate()),
        "and it names the object that is gone, rather than dropping the failure on \
         the floor: {error}"
    );
    let message = error.to_string();
    assert!(
        message.contains(DEPOT_OBJECT_CRATE) && message.contains("sector"),
        "a caller that only logs the message can still see what failed: {message}"
    );
    // The world is still coherent: the residency still describes what is there,
    // and nothing the pass could not do was reported as done.
    let resident = residency(app.world()).expect("a world is loaded");
    assert!(
        resident.resident().is_present(&object(DEPOT_OBJECT_CRATE)),
        "the record still claims the crate is present, so the inconsistency stays \
         visible rather than being papered over"
    );
    assert!(
        resident
            .resident()
            .loaded_sectors()
            .contains(&sector(DEPOT_SECTOR_ANNEX)),
        "and the annex is still loaded, because the pass that would have unloaded \
         it failed: {:?}",
        resident.resident().loaded_sectors()
    );
}
/// **A sector streamed back in without its geometry is a reported gap, not a
/// silent success and not a refusal.**
///
/// The pass hands the sector load the mesh source it was given, and a
/// `FromMesh` object the source does not hold is F18-B's
/// [`SkipReason::MeshUnavailable`](cs_app::world::SkipReason::MeshUnavailable):
/// the object is presented, appears in the load's report with that reason, and
/// takes no collider. **No geometry is invented for it**, which is the property
/// worth pinning here — a streamed-in sector whose geometry nobody supplied must
/// not come back with a box in place of the hangar shell, because a substituted
/// shape is how a traversable opening gets closed.
///
/// The *refusal* half of this path is stated rather than tested.
/// [`load_sector`](cs_app::world::load_sector) can also fail with
/// [`WorldSpawnError::UnrepresentableTransform`](cs_app::world::WorldSpawnError)
/// and with [`OverlayError::VanishedEntity`](cs_app::world::OverlayError), and
/// neither is reachable here for the same reason F18-B recorded for its own
/// abort path: a load refuses a definition with an unrepresentable matrix at the
/// door, so a sector reloaded from the same definition cannot meet one, and an
/// object the load is spawning does not yet exist to have lost an entity. The
/// branch is handled rather than assumed, and a mutation that swallows it leaves
/// this suite green — the same honest statement F18-B makes about its
/// `rollback`.
///
/// Observable failure: the streamed-in hangar is presented with no collider and
/// no substitute shape, and the report names why.
#[test]
fn accept_f18_c_a_sector_streamed_back_in_without_geometry_is_a_reported_gap() {
    let (mut app, _report) = loaded(&[]);
    update_visibility(&mut app, &FAR_WEST, &meshes()).expect("the pass runs");
    assert!(
        !residency(app.world())
            .expect("a world is loaded")
            .resident()
            .loaded_sectors()
            .contains(&sector(DEPOT_SECTOR_YARD)),
        "the yard is out of range and nothing requires it, so it went"
    );

    // A source that holds nothing: the hangar shell's upload is gone, so a
    // streamed-in yard has a `FromMesh` object it cannot build collision for.
    let update = update_visibility(&mut app, &IN_THE_YARD, &cs_app::world::WorldMeshes::new())
        .expect(
            "a sector whose geometry the source lacks still loads, so the \
                     gap is visible rather than a hole",
        );
    assert_eq!(
        update.loaded,
        vec![sector(DEPOT_SECTOR_ANNEX), sector(DEPOT_SECTOR_YARD)],
        "and the pass reports what it brought in: {update:?}"
    );

    // The hangar is present, has no collider, and nothing took its place.
    let resident = residency(app.world()).expect("a world is loaded");
    let hangar = resident
        .resident()
        .object(&object(DEPOT_OBJECT_HANGAR))
        .expect("the streamed-in hangar is present");
    assert!(
        hangar.collider.is_none(),
        "the hangar is presented with no collider, because the source has no \
         geometry for it: {hangar:?}"
    );
    assert_eq!(
        hangar.skipped,
        Some(cs_app::world::SkipReason::MeshUnavailable),
        "and the reason is a *load* gap — the record names a mesh this source does \
         not hold — not an unevidenced reference, which is a different fact"
    );
    // The depot's five objects, of which four are cuboids: the door, the trigger
    // volume, the ground slab and the annex crate. Counting the colliders rather
    // than the number is what makes a substitution visible: a spawn that quietly
    // built a box for the missing hangar would make this five, and the count is
    // what says so.
    let mut query = app
        .world_mut()
        .query_filtered::<Entity, With<cs_app::world::WorldColliderInstance>>();
    let world_colliders = query
        .iter(app.world())
        .filter(|entity| {
            app.world()
                .get::<avian3d::prelude::Collider>(*entity)
                .is_some()
        })
        .count();
    assert_eq!(
        world_colliders, 4,
        "and the world holds exactly the depot's four cuboid colliders — nothing \
         was invented for the hangar: {world_colliders}"
    );
}

/// **A required sector that something outside the policy unloaded is restored,
/// not left as a record with nothing behind it.**
///
/// Rule 1 holds a required sector in **both** directions. The obvious half is
/// "the pass never unloads it"; the half that is easy to get wrong is what
/// happens when the sector is gone for some other reason — a teardown, another
/// subsystem's pass, a caller's explicit
/// [`unload_sector`](cs_app::world::unload_sector). The pass is stated as
/// "residency should match the held set", and the held set contains every
/// required sector, so the next pass puts it back.
///
/// This matters because the two remaining options are both worse. Leaving it
/// unloaded means a gameplay-required object is neither simulated nor
/// summarized *in the world* — the record still names it, and nothing is there
/// — which is the state F18 non-negotiable behavior 3 exists to prevent, and it
/// is a state a caller cannot see from the return value, because the pass would
/// have reported a clean no-op.
///
/// Observable failure: the yard stays unloaded after a pass, or the pass reports
/// it as loaded while its objects are still gone.
#[test]
fn accept_f18_c_a_required_sector_unloaded_outside_the_policy_comes_back() {
    let (mut app, _report) = loaded(&[DEPOT_OBJECT_DOOR]);
    unload_sector(&mut app, &sector(DEPOT_SECTOR_YARD)).expect("the yard is loaded");
    assert!(
        entities_of(&mut app, &object(DEPOT_OBJECT_DOOR)).is_empty(),
        "the panel's entities are gone, so the record is the only thing that still \
         names it"
    );
    assert!(
        !loaded_sectors(&app).contains(&DEPOT_SECTOR_YARD.to_owned()),
        "and the sector is not resident"
    );

    let update = update_visibility(&mut app, &FAR_WEST, &meshes()).expect("the pass runs");
    assert_eq!(
        update.retained,
        vec![sector(DEPOT_SECTOR_YARD)],
        "the yard is out of the focus's range and is held anyway: {update:?}"
    );
    assert_eq!(
        update.loaded,
        vec![sector(DEPOT_SECTOR_YARD)],
        "and a pass that finds it missing loads it again, because the held set is \
         what residency is made to match: {update:?}"
    );
    assert_eq!(
        update.unloaded,
        vec![sector(DEPOT_SECTOR_ANNEX)],
        "while the sector nothing needs still goes: {update:?}"
    );
    let panel = entities_of(&mut app, &object(DEPOT_OBJECT_DOOR));
    assert_eq!(
        panel.len(),
        2,
        "and the panel gameplay requires is live again, one entity per half: {panel:?}"
    );
    assert!(
        residency(app.world())
            .expect("a world is loaded")
            .resident()
            .is_present(&object(DEPOT_OBJECT_HANGAR)),
        "with the rest of its sector"
    );
    let _ = VisibilityUpdate::default();
}

/// **A pass that is asked twice for the same focus changes nothing the second
/// time.**
///
/// The policy has to be convergent, or a caller that asks every tick would churn
/// sectors forever. A pass with nothing to do reports nothing moved, and says so
/// rather than pretending it held something.
#[test]
fn accept_f18_c_a_second_pass_over_the_same_focus_moves_nothing() {
    let (mut app, _report) = loaded(&[DEPOT_OBJECT_DOOR]);
    let first = update_visibility(&mut app, &FAR_WEST, &meshes()).expect("the pass runs");
    assert!(
        !first.is_empty(),
        "the first pass has work to do: {first:?}"
    );
    let second = update_visibility(&mut app, &FAR_WEST, &meshes()).expect("the pass runs");
    assert!(
        second.is_empty() && second.moved.is_empty() && second.unloaded.is_empty(),
        "a second pass over the same focus has nothing to do: {second:?}"
    );
    assert_eq!(
        second.retained,
        vec![sector(DEPOT_SECTOR_YARD)],
        "and it still reports the sector it is holding, which is the interesting \
         half: {second:?}"
    );
    // And the residency is untouched by the empty pass.
    assert_eq!(
        loaded_sectors(&app),
        vec![
            DEPOT_SECTOR_APPROACH.to_owned(),
            DEPOT_SECTOR_YARD.to_owned(),
        ],
        "the annex is still gone and the yard is still here"
    );
}

/// The sector load a caller asks for explicitly is the load transaction's own,
/// not the policy's: the policy holds and releases, and this is how a required
/// sector is brought back on purpose.
#[test]
fn accept_f18_c_a_sector_a_pass_unloaded_can_be_loaded_again_on_purpose() {
    let (mut app, _report) = loaded(&[]);
    update_visibility(&mut app, &FAR_WEST, &meshes()).expect("the pass runs");
    assert!(
        !loaded_sectors(&app).contains(&DEPOT_SECTOR_YARD.to_owned()),
        "the yard is out of range and nothing requires it, so it went"
    );
    let load = load_sector(&mut app, &sector(DEPOT_SECTOR_YARD), &meshes())
        .expect("the yard loads on request");
    assert_eq!(
        load.spawned,
        vec![object(DEPOT_OBJECT_DOOR), object(DEPOT_OBJECT_HANGAR)],
        "and it brings its own objects back, in the load's stable id order"
    );
    assert!(
        unload_sector(&mut app, &sector(DEPOT_SECTOR_YARD)).is_ok(),
        "and the transaction still owns it, so a caller may unload it again"
    );
    let _: VisibilityUpdate = VisibilityUpdate::default();
}
