//! Task #666 (`PLAYTEST-TEXTURES`) acceptance tests. Prefix:
//! `accept_playtest_textures_`. A module of the `playtest_retail` test binary.
//!
//! The retail half spawns the owner's `c1c` area and `bloodhawk` airframe through the
//! production readers and checks that both draw with materials whose base-colour
//! image was decoded out of the original texture archive, then renders them on the
//! real GPU and measures the colour detail against the untextured baseline. It is
//! `#[ignore]`d (`requires CS_GAME_DIR`) and **fails** without the variable.
//! Frames go under `private/evidence/PLAYTEST-TEXTURES/` (Git-ignored); nothing
//! original is committed.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use bevy::image::Image;
use bevy::mesh::Mesh;
use bevy::prelude::{Assets, ChildOf, Entity, Mesh3d, MeshMaterial3d, StandardMaterial};
use cs_app::playtest_retail::{
    AIRCRAFT_CONTAINER_KEY, PlaytestConfig, PlaytestScene, capture_playtest_views, playtest_app,
    read_playtest_sources, spawn_playtest_scene, teardown_playtest_scene,
};
use cs_app::playtest_textures::{
    NAME_READING, PLAYTEST_TEXTURE_ARCHIVE_IS_DESIGNED, PLAYTEST_TEXTURE_NAME_READING_IS_DESIGNED,
    PLAYTEST_TEXTURE_PRESENTATION_IS_PROVISIONAL, PlaytestTextureError, check_group, image_key,
};
use cs_content::mesh::TextureNameRule;
use cs_content::textures::TextureId;
use cs_types::asset_id::{AssetVariant, MountId};
use cs_types::install::RelativePath;

/// How many updates asset release needs to settle (see `playtest_retail`'s test).
const ASSET_RELEASE_UPDATES: usize = 3;

fn install() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set for the retail half"),
    )
}

fn capture_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|workspace| workspace.parent())
        .map(|root| root.join("private/evidence/PLAYTEST-TEXTURES").join(name))
        .expect("the workspace root is two levels above the crate");
    std::fs::create_dir_all(&dir).expect("private/evidence is writable");
    dir
}

fn id_in(archive: &str, entry: usize, name: &str) -> TextureId {
    TextureId {
        archive: RelativePath::new(archive).expect("a relative path"),
        mount: MountId::new("installation").expect("a mount label"),
        variant: AssetVariant::new("default").expect("a variant label"),
        entry_index: entry,
        name: name.to_owned(),
    }
}

// ------------------------------------------------------------------ no retail --

/// **An image from another world group's archive is refused, and a bound image's
/// key names its archive and member.**
#[test]
fn accept_playtest_textures_an_image_of_another_world_group_is_refused() {
    let own = id_in("ZBD/C1C/rtexture10.zbd", 7, "sky1");
    assert_eq!(check_group("c1c", &own), Ok(()));
    assert_eq!(
        check_group("C1C", &own),
        Ok(()),
        "the group is compared as a path"
    );

    let foreign = id_in("ZBD/C2/rtexture10.zbd", 7, "sky1");
    let refused = check_group("c1c", &foreign).expect_err("another group's archive is refused");
    assert!(matches!(refused, PlaytestTextureError::ForeignGroup { .. }));
    assert!(
        refused
            .to_string()
            .contains("never aliased across world groups")
    );

    // A group whose name is a prefix of another's is still another group.
    let sibling = id_in("ZBD/C1/texture.zbd", 0, "sky1");
    assert!(check_group("c1c", &sibling).is_err());
    let root = id_in("ZBD/rimage.zbd", 0, "sky1");
    assert!(
        check_group("c1c", &root).is_err(),
        "the UI archive is no world's"
    );

    assert_eq!(
        image_key(&own),
        "ZBD/C1C/rtexture10.zbd#7:sky1",
        "the key names the source archive and the member"
    );
}

/// **The three designed choices are filed under their own claims and the name
/// reading is the one the finding measured, scoped to the playtest.**
#[test]
fn accept_playtest_textures_every_designed_choice_has_its_own_claim() {
    let claims = [
        PLAYTEST_TEXTURE_ARCHIVE_IS_DESIGNED,
        PLAYTEST_TEXTURE_NAME_READING_IS_DESIGNED,
        PLAYTEST_TEXTURE_PRESENTATION_IS_PROVISIONAL,
    ];
    assert_eq!(claims.iter().collect::<BTreeSet<_>>().len(), 3);
    for claim in claims {
        assert!(claim.starts_with("playtest-textures."), "{claim}");
    }
    assert_eq!(NAME_READING, TextureNameRule::FirstDotCaseFolded);
    // The two spellings the corpus stores: an extension and a double dot.
    assert_eq!(NAME_READING.project("Sky1.tif"), "sky1");
    assert_eq!(
        NAME_READING.project("bldhwk_cowling..tif"),
        "bldhwk_cowling"
    );
}

// --------------------------------------------------------------------- retail --

/// Every `(mesh, material)` part drawn under `roots`' entities and the image each
/// material binds.
struct Drawn {
    parts: usize,
    textured: usize,
    images: BTreeSet<bevy::asset::AssetId<Image>>,
}

fn drawn_under(app: &mut bevy::prelude::App, roots: &[Entity]) -> Drawn {
    let mut drawn = Drawn {
        parts: 0,
        textured: 0,
        images: BTreeSet::new(),
    };
    let mut query = app
        .world_mut()
        .query::<(&ChildOf, &Mesh3d, &MeshMaterial3d<StandardMaterial>)>();
    let materials = app.world().resource::<Assets<StandardMaterial>>();
    for (parent, _, material) in query.iter(app.world()) {
        if !roots.contains(&parent.parent()) {
            continue;
        }
        drawn.parts += 1;
        let material = materials.get(&material.0).expect("the material is live");
        if let Some(image) = &material.base_color_texture {
            drawn.textured += 1;
            drawn.images.insert(image.id());
        }
    }
    drawn
}

fn area_roots(scene: &PlaytestScene) -> Vec<Entity> {
    scene.spawned().objects().iter().map(|o| o.visual).collect()
}

/// **The area and the aircraft are drawn with materials whose base-colour image
/// was decoded from the original archive, with counts reported; an image of
/// another group's archive is refused; teardown releases every material and
/// image.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_playtest_textures_retail_area_and_aircraft_draw_original_images() {
    let config = PlaytestConfig::documented();
    let sources = read_playtest_sources(&install(), &config.world_group).expect("reads");

    // -- the archive is explicit and belongs to the world's own group ----------
    let archive = sources.textures();
    assert_eq!(archive.group(), "c1c");
    assert!(
        archive.path().to_ascii_lowercase().starts_with("zbd/c1c/"),
        "{}",
        archive.path()
    );
    assert!(archive.path().to_ascii_lowercase().contains("rtexture"));
    assert!(!archive.sha256().is_empty());
    assert!(archive.texture_count() > 100);
    let own = archive.sample_id().expect("the archive stores textures");
    assert_eq!(check_group("c1c", &own), Ok(()));
    let mut as_other = own.clone();
    as_other.archive = RelativePath::new("ZBD/C2/rtexture10.zbd").unwrap();
    assert!(
        check_group("c1c", &as_other).is_err(),
        "the same member of another world group's archive is refused"
    );

    let mut app = playtest_app();
    app.update();
    let meshes_before = app.world().resource::<Assets<Mesh>>().len();
    let images_before = app.world().resource::<Assets<Image>>().len();
    let materials_before = app.world().resource::<Assets<StandardMaterial>>().len();
    let scene = spawn_playtest_scene(&mut app, &sources, &config).expect("the scene spawns");
    let report = scene.textures();

    // -- the counts ------------------------------------------------------------
    eprintln!("PLAYTEST-TEXTURES report {}", report.json());
    assert_eq!(report.archive, archive.path());
    assert_eq!(report.archive_sha256, archive.sha256());
    for key in ["zbd/c1c/gamez.zbd", AIRCRAFT_CONTAINER_KEY] {
        let subject = report
            .subject(key)
            .unwrap_or_else(|| panic!("{key} has a report"));
        eprintln!(
            "PLAYTEST-TEXTURES {key}: textured={} neutral={} (flat={}) parts textured={} \
             neutral={} images={} resolved={:?} unresolved={:?}",
            subject.textured_materials,
            subject.neutral_materials,
            subject.flat_materials,
            subject.textured_parts,
            subject.neutral_parts,
            subject.images.len(),
            subject.resolved_names,
            subject.unresolved_names,
        );
        assert!(
            subject.textured_materials > 0 && subject.textured_parts > 0,
            "{key} draws no original texture: {subject:?}"
        );
        assert!(!subject.resolved_names.is_empty());
        for unresolved in &subject.unresolved {
            assert!(unresolved.source_id.starts_with(key), "{unresolved:?}");
            assert!(
                unresolved.meshes > 0,
                "reported with its mesh count: {unresolved:?}"
            );
        }
        for image in &subject.images {
            assert!(
                image
                    .key
                    .to_ascii_lowercase()
                    .starts_with(&format!("{}#", archive.path().to_ascii_lowercase())),
                "an image key names its source archive: {}",
                image.key
            );
            assert!(image.key.contains(':'), "and its member: {}", image.key);
            assert!(image.width > 0 && image.height > 0);
        }
    }

    // -- the engine really holds decoded images bound as base colour -----------
    let area = drawn_under(&mut app, &area_roots(&scene));
    // The aircraft's pieces hang under its bindings, one level below its root.
    let bindings: Vec<Entity> = app
        .world()
        .get::<bevy::prelude::Children>(scene.aircraft_entity())
        .expect("the aircraft has its bindings")
        .iter()
        .copied()
        .collect();
    let aircraft = drawn_under(&mut app, &bindings);
    eprintln!(
        "PLAYTEST-TEXTURES drawn: area parts={} textured={} images={}; aircraft parts={} \
         textured={} images={}",
        area.parts,
        area.textured,
        area.images.len(),
        aircraft.parts,
        aircraft.textured,
        aircraft.images.len()
    );
    assert!(
        area.textured > 0 && area.images.len() > 1,
        "the area is textured"
    );
    assert!(
        aircraft.textured > 0 && !aircraft.images.is_empty(),
        "the aircraft is textured"
    );
    let images = app.world().resource::<Assets<Image>>();
    for id in area.images.iter().chain(aircraft.images.iter()) {
        let image = images.get(*id).expect("the bound image is live");
        let data = image.data.as_ref().expect("the image holds its texels");
        assert_eq!(
            data.len(),
            (image.width() * image.height() * 4) as usize,
            "decoded to RGBA8"
        );
        let colors: BTreeSet<[u8; 3]> = data
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| [p[0], p[1], p[2]])
            .collect();
        assert!(
            colors.len() > 4,
            "an original image holds detail, not a flat fill"
        );
    }

    // -- teardown releases materials and images, repeatedly --------------------
    let owned = scene.entities().len();
    let teardown = teardown_playtest_scene(&mut app, &scene);
    assert_eq!(teardown.entities, owned);
    assert!(
        teardown.images_before > images_before && teardown.materials_before > materials_before,
        "the scene held images and materials when the teardown began"
    );
    drop(scene);
    for _ in 0..ASSET_RELEASE_UPDATES {
        app.update();
    }
    assert_eq!(app.world().resource::<Assets<Image>>().len(), images_before);
    assert_eq!(
        app.world().resource::<Assets<StandardMaterial>>().len(),
        materials_before
    );
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), meshes_before);
    for round in 0..3 {
        let again = spawn_playtest_scene(&mut app, &sources, &config).expect("reloads");
        assert!(again.textures().textured_materials() > 0);
        teardown_playtest_scene(&mut app, &again);
        drop(again);
        for _ in 0..ASSET_RELEASE_UPDATES {
            app.update();
        }
        assert_eq!(
            app.world().resource::<Assets<Image>>().len(),
            images_before,
            "round {round} leaks images"
        );
        assert_eq!(
            app.world().resource::<Assets<StandardMaterial>>().len(),
            materials_before,
            "round {round} leaks materials"
        );
        assert_eq!(
            app.world().resource::<Assets<Mesh>>().len(),
            meshes_before,
            "round {round} leaks meshes"
        );
    }
}

/// **A real GPU capture shows texture detail the neutral-material render does
/// not.** The same views are rendered twice, once with the original textures and
/// once with the neutral baseline (`PlaytestConfig::textured = false`), and the
/// distinct colours over the aircraft's and the area's own pixels are compared.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_playtest_textures_retail_gpu_capture_shows_texture_detail() {
    let mut config = PlaytestConfig::documented();
    let sources = read_playtest_sources(&install(), &config.world_group).expect("reads");

    let mut render = |textured: bool, name: &str| {
        config.textured = textured;
        let mut app = playtest_app();
        app.update();
        let scene = spawn_playtest_scene(&mut app, &sources, &config).expect("spawns");
        let captures = capture_playtest_views(&mut app, &scene, &capture_dir(name))
            .expect("every view renders");
        teardown_playtest_scene(&mut app, &scene);
        captures
    };
    let neutral = render(false, "neutral");
    let textured = render(true, "textured");
    assert_eq!(neutral.len(), textured.len());
    for (flat, rich) in neutral.iter().zip(&textured) {
        eprintln!(
            "PLAYTEST-TEXTURES {}: aircraft colors {} -> {} (luma variance {:.1} -> {:.1}), \
             area colors {} -> {} (luma variance {:.1} -> {:.1}); {}",
            rich.view,
            flat.aircraft_distinct_colors,
            rich.aircraft_distinct_colors,
            flat.aircraft_luma_variance,
            rich.aircraft_luma_variance,
            flat.environment_distinct_colors,
            rich.environment_distinct_colors,
            flat.environment_luma_variance,
            rich.environment_luma_variance,
            rich.png
        );
        assert!(rich.drew_aircraft() && rich.drew_environment());
        assert!(
            rich.environment_distinct_colors > 2 * flat.environment_distinct_colors,
            "{}: the area shows no more colour detail than the neutral render",
            rich.view
        );
    }
    // The views that frame the aircraft large enough to read.
    let chase = (&neutral[0], &textured[0]);
    assert!(
        chase.1.aircraft_distinct_colors > 2 * chase.0.aircraft_distinct_colors,
        "the aircraft shows no more colour detail than the neutral render: {} vs {}",
        chase.1.aircraft_distinct_colors,
        chase.0.aircraft_distinct_colors
    );
}
