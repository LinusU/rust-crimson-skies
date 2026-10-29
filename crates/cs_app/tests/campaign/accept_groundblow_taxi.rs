//! GROUNDBLOW-TAXI-PATH (#2215): the plane record's taxi path —
//! `aiv.zrd` field 40, the record's `+0x11c` slot the loader reads at
//! `0x47c3a9` — imported as `ScriptedAircraft::taxi_path`, resolved through
//! the mission's `*_aipath` scene nodes into the vehicle's `+0xcc` byte on
//! [`ScriptedAircraftActor::on_path`], and the ground-blow sweep reporting
//! `SegmentProbeOwner::Vehicle { on_path }` from it.
//!
//! The original's gate: the registry `0x4511b0` resolves the name, and only
//! a path with at least two points sets `V+0xcc = 1` (`0x47c3f1`,
//! `0x47c568`); the ground blow's player gate then keeps that hit. The
//! retail half (`CS_GAME_DIR`) proves M01's measured path state and that
//! every record which *does* name a taxi path resolves to at least two
//! points in its own chapter's `gamez.zbd`. A synthetic run never needs
//! `$CS_GAME_DIR` or `$CS_ENGINE_IMAGE`.

use std::path::{Path, PathBuf};

use avian3d::prelude::{Collider, Position, Rotation};
use bevy::ecs::world::World;
use bevy::math::{Quat, Vec3};
use cs_app::mission_actors::{MissionAircraftRoster, ScriptedAircraftActor, spawn_mission_actors};
use cs_app::mission_session::{
    MissionHost, MissionTaxiPaths, host_session_id, mint_host_generation, read_taxi_paths,
};
use cs_app::mission_start::{
    ScriptedAircraft, StoredStartPose, read_aircraft_records, spawn_dispatch,
};
use cs_app::physics::BodyLayer;
use cs_app::playtest::world_sweep::{owner_of, probe_in_world};
use cs_content::stunts::{ZrdValue, decode_zrd};
use cs_sim::collision::CollisionLayer;
use cs_sim::flight::original::SegmentProbeOwner;
use cs_types::asset_id::SourceSpan;
use cs_types::content::Provenance;
use cs_types::evidence::ClaimId;

use crate::campaign::vs_m01_rt_window::{Scratch, synthetic_stage};

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"))
}

/// The decoded document and member span of `member` inside the
/// installation's `container_key` archive — the same walk
/// `m01_aiv_layout`'s member reader performs, kept local because the suite
/// trees share only `common.rs`, and `None` when the archive declares no
/// such member (a chapter's `zbd/cN/zrdr.zbd` declares no `aiv.zrd`).
fn member_document(
    root: &Path,
    container_key: &str,
    member: &str,
) -> Option<(ZrdValue, SourceSpan)> {
    let found = cs_assets::install::discover(root).expect("the installation discovers");
    let record = found
        .manifest
        .files
        .iter()
        .find(|record| record.relative_spelling.logical_key() == container_key.to_ascii_lowercase())
        .unwrap_or_else(|| panic!("the installation has {container_key}"));
    let spelling = record.relative_spelling.as_str().to_owned();
    let path = cs_types::install::RelativePath::new(&spelling.to_lowercase()).expect("a path");
    let bytes = std::fs::read(found.manifest.host_root.join(&spelling)).expect("readable");
    let discovery = cs_formats::script_raw::discover_container(container_key, &path, &bytes);
    let program = discovery.programs().iter().find(|program| {
        program
            .locator()
            .member()
            .is_some_and(|name| name.eq_ignore_ascii_case(member))
    })?;
    let document =
        decode_zrd(program.bytes()).unwrap_or_else(|error| panic!("{member} decodes: {error:?}"));
    let span = program.locator().span();
    let source = SourceSpan::new(
        cs_assets::install::fingerprint(&found.manifest),
        &spelling,
        Some(member),
        span.offset,
        span.len,
        None,
    )
    .expect("a member span");
    Some((document, source))
}

/// A record the spawn can place: a measured-shape stored pose, `enabled`,
/// and asleep so the airframe table is not part of the test. `taxi_path` is
/// the one field this suite varies.
fn spawnable(name: &str, taxi_path: Option<&str>) -> ScriptedAircraft {
    ScriptedAircraft {
        name: name.to_owned(),
        index: 0,
        field_count: 81,
        netids: None,
        stored_pose: Some(StoredStartPose {
            position: [40.0, 20.0, -60.0],
            heading: 0.0,
        }),
        team: Some(2),
        group: Some(1),
        enabled: Some(true),
        primary_target: None,
        title: None,
        starts_asleep: Some(true),
        taxi_path: taxi_path.map(str::to_owned),
        dispatch: spawn_dispatch(name),
        provenance: Provenance::designed(
            ClaimId::new("test.groundblow-taxi.record").expect("the claim id is valid"),
        ),
    }
}

/// The spawn gives the vehicle's own `+0xcc` byte from the record's
/// taxi-path field *and* the path's resolved point count: a record whose
/// name resolves to two or more `*_aipN` points spawns `on_path`, and a
/// record with no name — or a name the table does not resolve, or a path
/// shorter than two points — does not.
#[test]
fn accept_groundblow_taxi_spawn_states_on_path_from_the_record_and_the_table() {
    let scratch = Scratch::new("groundblow-taxi-spawn");
    let stage = synthetic_stage(scratch.path());
    let generation = mint_host_generation();
    let host = MissionHost::launch(&stage, generation, host_session_id(generation))
        .expect("the synthetic stage launches a host");

    let mut world = World::new();
    world.insert_resource(host);
    world.insert_resource(MissionAircraftRoster::from_records(&[
        spawnable("kestrel_1", Some("pp1")),
        spawnable("kestrel_2", None),
        spawnable("kestrel_3", Some("nowhere")),
        spawnable("kestrel_4", Some("pp_short")),
    ]));
    world.insert_resource(MissionTaxiPaths {
        paths: [("pp1".to_owned(), 4), ("pp_short".to_owned(), 1)]
            .into_iter()
            .collect(),
        refusals: Vec::new(),
    });

    spawn_mission_actors(&mut world);

    let mut actors = world.query::<&ScriptedAircraftActor>();
    let flags: Vec<(&str, bool)> = actors
        .iter(&world)
        .map(|actor| (actor.record.name.as_str(), actor.on_path))
        .collect();
    assert_eq!(
        flags,
        [
            ("kestrel_1", true),
            ("kestrel_2", false),
            ("kestrel_3", false),
            ("kestrel_4", false),
        ],
        "only the record whose taxi path resolves to at least two points spawns on-path"
    );
}

/// One `ScriptedAircraftActor` carrying an `Aircraft`-layer collider: the
/// sweep resolves its owner to `Vehicle { on_path }` — the flag from the
/// entity, not a constant.
#[test]
fn accept_groundblow_taxi_sweep_reports_the_actor_flag() {
    let mut world = World::new();
    let prober = world.spawn_empty().id();
    let generation = cs_app::scene::SceneGeneration(0);
    let actor_id = cs_sim::damage::ActorId {
        session: cs_types::net::SessionId::new(9).expect("session 9"),
        serial: 7,
    };
    let spawn_actor = |world: &mut World, on_path: bool| {
        world
            .spawn((
                Collider::cuboid(12.0, 2.0, 10.0),
                Position(Vec3::new(0.0, 50.0, 0.0)),
                Rotation(Quat::IDENTITY),
                BodyLayer(CollisionLayer::Aircraft),
                ScriptedAircraftActor {
                    record: spawnable("kestrel_1", None),
                    vehicle_record: "kestrel".to_owned(),
                    damage_actor: actor_id,
                    generation,
                    asleep: true,
                    on_path,
                },
            ))
            .id()
    };
    let parked = spawn_actor(&mut world, true);
    let hit = probe_in_world(
        &world,
        Vec3::new(0.0, 50.0, 300.0),
        Quat::IDENTITY,
        400.0,
        prober,
    )
    .expect("the aircraft box answers");
    assert_eq!(hit.entity, parked);
    assert_eq!(hit.owner, SegmentProbeOwner::Vehicle { on_path: true });

    // The same body without the flag — and a body that is no scripted
    // aircraft at all — both resolve `on_path: false`, the loader's default.
    let mut bare = World::new();
    let bare_prober = bare.spawn_empty().id();
    bare.spawn((
        Collider::cuboid(12.0, 2.0, 10.0),
        Position(Vec3::new(0.0, 50.0, 0.0)),
        Rotation(Quat::IDENTITY),
        BodyLayer(CollisionLayer::Aircraft),
    ));
    let hit = probe_in_world(
        &bare,
        Vec3::new(0.0, 50.0, 300.0),
        Quat::IDENTITY,
        400.0,
        bare_prober,
    )
    .expect("the bare aircraft box answers");
    assert_eq!(hit.owner, SegmentProbeOwner::Vehicle { on_path: false });

    // `owner_of` takes the flag directly: the production query and the
    // read-only path share this rule.
    assert_eq!(
        owner_of(false, false, false, CollisionLayer::Aircraft, false, true),
        Some(SegmentProbeOwner::Vehicle { on_path: true })
    );
}

/// M01's measured path state: every `zbd/c3/m01` `aiv.zrd` record's field
/// 40 is empty text, so no record names a taxi path and every resolved
/// `+0xcc` answer is `false` — stated per record with the member-span
/// provenance the record itself carries.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_groundblow_taxi_m01_records_carry_no_taxi_path() {
    let root = game_dir();
    let (document, source) =
        member_document(&root, "zbd/c3/m01/zrdr.zbd", "aiv.zrd").expect("M01's aiv.zrd");
    let aircraft = read_aircraft_records("zbd/c3/m01", &document, &source).expect("read");
    assert_eq!(aircraft.len(), 5, "M01's aiv.zrd holds five records");

    let paths = read_taxi_paths(&root, "zbd/c3");
    // The chapter's one authored path (`bauda_aipath`, four points) is
    // measured here so the report states what M01's container *does* carry
    // — it is not a claim that C3 has no paths.
    assert_eq!(
        paths.point_count("bauda"),
        4,
        "C3's bauda_aipath is measured"
    );

    for record in &aircraft {
        assert_eq!(
            record.taxi_path.as_deref(),
            None,
            "{}: field 40 is empty text (span {:?})",
            record.name,
            record.provenance.source
        );
        assert!(
            !paths.is_on_path(record.taxi_path.as_deref()),
            "{}: the record's resolved +0xcc is 0",
            record.name
        );
    }
}

/// Every retail `aiv.zrd` record that *does* name a taxi path resolves it
/// in its own chapter's `gamez.zbd` to at least two `*_aipN` points — the
/// corpus half of "the record's path state is measured": ten records in
/// `c1/m04`, `c2/m02` and `c5/m01`, and `is_on_path` answers `true` for
/// each.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_groundblow_taxi_named_paths_resolve_in_their_chapter() {
    let root = game_dir();
    let found = cs_assets::install::discover(&root).expect("the installation discovers");
    let mut named = Vec::new();
    for record in &found.manifest.files {
        let key = record.relative_spelling.logical_key();
        if !key.ends_with("/zrdr.zbd") {
            continue;
        }
        // The mission the record belongs to is the archive's directory,
        // e.g. `zbd/c1/m04` → records keyed by their own names. A chapter's
        // own `zbd/cN/zrdr.zbd` declares no `aiv.zrd` and is skipped.
        let mission = key.trim_end_matches("/zrdr.zbd");
        let Some((document, source)) = member_document(&root, &key, "aiv.zrd") else {
            continue;
        };
        for aircraft in read_aircraft_records(mission, &document, &source).expect("read") {
            if let Some(taxi_path) = &aircraft.taxi_path {
                named.push((key.clone(), aircraft.name.clone(), taxi_path.clone()));
            }
        }
    }
    named.sort();
    let measured: Vec<(&str, &str, &str)> = vec![
        ("zbd/c1/m04/zrdr.zbd", "blakepeace_2_3", "pp1"),
        ("zbd/c1/m04/zrdr.zbd", "blakepeace_2_4", "pp2"),
        ("zbd/c1/m04/zrdr.zbd", "blakepeace_2_5", "pp3"),
        ("zbd/c1/m04/zrdr.zbd", "blakepeace_2_6", "pp4"),
        ("zbd/c2/m02/zrdr.zbd", "devastator_1", "pp1"),
        ("zbd/c2/m02/zrdr.zbd", "secgyro_1", "pp3"),
        ("zbd/c2/m02/zrdr.zbd", "secgyro_2", "pp4"),
        ("zbd/c2/m02/zrdr.zbd", "secgyro_3", "pp5"),
        ("zbd/c2/m02/zrdr.zbd", "secgyro_4", "pp6"),
        ("zbd/c5/m01/zrdr.zbd", "autogyro_1", "pp1"),
    ];
    let measured: Vec<(String, String, String)> = measured
        .into_iter()
        .map(|(a, b, c)| (a.to_owned(), b.to_owned(), c.to_owned()))
        .collect();
    assert_eq!(
        named, measured,
        "the measured corpus: exactly these ten records name a taxi path, \
         all else is empty"
    );

    // Each named path resolves in the record's own chapter (`zbd/cN` is the
    // world-group directory one level up from the mission directory).
    let mut resolved = 0;
    for (key, name, taxi_path) in &named {
        let chapter = key.split('/').nth(1).expect("a chapter directory");
        let directory = format!("zbd/{chapter}");
        let paths = read_taxi_paths(&root, &directory);
        assert!(
            paths.is_on_path(Some(taxi_path.as_str())),
            "{key} {name}: `{taxi_path}` resolves to at least two points in {directory} \
             (measured count {})",
            paths.point_count(taxi_path)
        );
        resolved += 1;
    }
    assert_eq!(resolved, 10);
}
