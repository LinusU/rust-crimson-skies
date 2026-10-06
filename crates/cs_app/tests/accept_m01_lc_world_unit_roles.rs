//! #677 (`M01-LC-WORLD-UNIT-ROLES`): the stored world-vertex unit is measured,
//! and the unindexed c1c records are classified by what their own bytes store.
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`
//! (`### F18-B` import, `### F18-D` evidence) and the first-mission surface of
//! `VS-M01-RUNTIME` (#359), which this measurement unblocks. Shared contract:
//! `docs/contracts/IDENTITY-CONTENT.md`. Task test prefix:
//! `accept_m01_lc_world_unit_roles_`.
//!
//! Two claims are pinned, and they are two different strengths:
//!
//! * **The scale is measured; the convention is not.** The GameZ landmark
//!   census — the `-9.8` gravity word all 61 animation containers store, the
//!   pilot-figure and airframe extents, the LOD switch bands and the world
//!   bounds — pins [`CalibratedQuantity::Scale`] to **one stored unit per
//!   metre** at `observed_tool`. The axis map, handedness and angle unit were
//!   never observed, so the source's whole-calibration status stays `Unknown`
//!   and its gap list names them. The non-retail half of this file asserts both
//!   halves of that sentence, because a source that reported `verified_original`
//!   — or a factor that silently reached a definition — would be a false claim,
//!   not a measurement.
//! * **The 53 unindexed c1c records are two measured classes, not one guess.**
//!   Every record the partition grid omits stores *either* a mesh index and a
//!   non-zero bounding box *or* neither of them, in all eight containers — the
//!   corpus has no third shape. The no-geometry half (the `horizon`, the
//!   `g*` transform groups, the zeppelin anchors) resolves to
//!   [`WorldCollisionRole::None`], because the store gives *this* record
//!   nothing a collider could be built from. The geometry-bearing half — the
//!   `fvol*` volumes — keeps an explicit `Unknown` role, because the container
//!   never says whether the original engine collided with it. The retail half
//!   asserts the split per container, the per-object consequences on the
//!   definition, and the spawn report the split produces.
//!
//! **What this file does not claim.** `retail` is file access: every number
//! here was read out of the owner's bytes by the production readers, and no
//! original executable ran. Nothing here is `verified_original`, and the tests
//! assert that too.

use std::collections::BTreeMap;

use cs_app::world::{
    MESH_SETTLE_UPDATES, RetailWorldContainer, RetailWorldContainers, read_world_containers,
    spawn_world, world_app,
};
use cs_content::coordinates::{
    CalibratedQuantity, CoordinateSource, GAMEZ_VERTEX_UNIT_IS_THE_METRE, SourceAdapter,
};
use cs_content::scene::{BindingMap, MeshSlot, scene_graph_from_gamez};
use cs_content::textures::WorldTextureLoad;
use cs_content::world::{
    OBJECT_STORES_NO_MESH, UNINDEXED_RECORD_STORES_NO_GEOMETRY, UNINDEXED_ROLE_UNMEASURED,
    WorldCollisionRole,
};
use cs_formats::gamez::read_gamez_nodes;
use cs_formats::io::ParseContext;
use cs_formats::zbd::anim::read_animation_index;
use cs_formats::zbd::{ZbdProbe, dispatch};
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentId, ContentKind, Origin, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ClaimStatus, ContentHash};
use cs_types::install::RelativePath;

/// The measured unindexed split of one world container (task #677's census).
///
/// `anchors + volumes == unindexed` in every row, and the census test asserts
/// that relation: the corpus holds no unindexed record carrying only one of a
/// mesh index and a bounding box.
struct Split {
    /// The group's directory spelling, as production discovery spells it.
    group: &'static str,
    /// How many unindexed records store no mesh and no extent: they resolve to
    /// `None`, because the record itself has nothing a collider could be built
    /// from.
    anchors: usize,
    /// How many unindexed records store geometry: they keep the role the
    /// container never states.
    volumes: usize,
    /// `anchors + volumes`, checked against the report's own child-list count.
    unindexed: usize,
}

/// The census, as the production readers measure it over the owner's
/// installation. The table is the assertion: a split that classified by name
/// or by guess could not hold these numbers per group.
const SPLITS: [Split; 8] = [
    Split {
        group: "C1",
        anchors: 56,
        volumes: 10,
        unindexed: 66,
    },
    Split {
        group: "C1B",
        anchors: 78,
        volumes: 0,
        unindexed: 78,
    },
    Split {
        group: "C1C",
        anchors: 36,
        volumes: 17,
        unindexed: 53,
    },
    Split {
        group: "C2",
        anchors: 24,
        volumes: 0,
        unindexed: 24,
    },
    Split {
        group: "C2B",
        anchors: 39,
        volumes: 9,
        unindexed: 48,
    },
    Split {
        group: "C3",
        anchors: 14,
        volumes: 0,
        unindexed: 14,
    },
    Split {
        group: "C4",
        anchors: 38,
        volumes: 13,
        unindexed: 51,
    },
    Split {
        group: "C5",
        anchors: 20,
        volumes: 85,
        unindexed: 105,
    },
];

/// A span that stands for "the container the census ran over" in a non-retail
/// test: the shape is exercised, the bytes are not claimed.
fn synthetic_span() -> SourceSpan {
    SourceSpan::new(
        ContentHash::from_bytes([0; 32]),
        "zbd/c1c/gamez.zbd",
        None,
        0,
        0,
        None,
    )
    .expect("a span for the measured source's declaration is valid")
}

/// The conversion a retail import is made through: the measured GameZ source
/// over this container's own span.
fn retail_adapter(container: &RetailWorldContainer) -> SourceAdapter {
    SourceAdapter::new(CoordinateSource::retail_gamez(container.span().clone()))
}

/// One production discovery pass, shared by every retail test.
fn retail() -> RetailWorldContainers {
    let root = std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR is set");
    read_world_containers(std::path::Path::new(&root)).expect("the installation is discovered once")
}

fn origin(container: &RetailWorldContainer) -> Origin {
    Origin::Installation {
        source: container.span().clone(),
    }
}

/// The eleven airframes the original's own loading script declares (F11-D2's
/// roster), spelled as `ZBD/planes.zbd` spells their roots.
const ROSTER_AIRFRAMES: [&str; 11] = [
    "autogyro",
    "bloodhawk",
    "fury",
    "piratefighter",
    "brigand",
    "avenger",
    "peacemaker",
    "kestrel",
    "firebrand",
    "warhawk",
    "balmoral",
];

/// **Every scale landmark the measurement rests on is re-observed here through
/// the production readers** (retail).
///
/// [`CoordinateSource::retail_gamez`]'s landmarks are prose in a source file,
/// and prose is not evidence. Each landmark's claim is a number over the
/// owner's bytes, so each is re-measured here from the same production paths
/// the census ran over — the ZBD-anim payload reader for the gravity word, the
/// GameZ node reader plus `SceneGraph` for the pilot figure and the airframes,
/// and the partition grid for the world theatre — and the landmark's own text
/// is asserted to still describe what those paths measure.
///
/// A landmark whose number drifts (a container set that changes, a reader that
/// stops walking the same records) fails here rather than surviving as a
/// confident sentence nobody re-reads.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_world_unit_roles_every_scale_landmark_is_re_observed_over_the_installation() {
    let root = std::path::PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR is set"));
    let found = cs_assets::install::discover(&root).expect("the installation is discovered");
    let read = |suffix: &str| -> Vec<u8> {
        let file = found
            .manifest
            .files
            .iter()
            .find(|f| f.relative_spelling.as_str().ends_with(suffix))
            .unwrap_or_else(|| panic!("the installation carries {suffix}"));
        std::fs::read(root.join(file.relative_spelling.as_str()))
            .unwrap_or_else(|error| panic!("read {suffix}: {error}"))
    };

    // ---- landmark 1: every animation container stores the gravity word ----
    let mut anim_containers = 0usize;
    let mut gravity_stores = 0usize;
    for file in &found.manifest.files {
        let key = file.relative_spelling.as_str();
        if !(key.ends_with("mis_anim.zbd") || key.ends_with("cam_anim.zbd")) {
            continue;
        }
        let bytes = read(key);
        anim_containers += 1;
        let mut context = ParseContext::with_defaults("accept_m01_lc_world_unit_roles");
        let path = RelativePath::new(key).expect("the manifest's own spelling is relative");
        let header_len = bytes.len().min(8);
        let decision = dispatch(ZbdProbe::new(key, &path, &bytes[..header_len]))
            .unwrap_or_else(|error| panic!("{key}: dispatch refused: {error}"));
        let index =
            read_animation_index(&mut context, decision, &bytes).expect("the carrier indexes");
        let payload = index.payload().expect("the payload header reads");
        if payload.header().gravity == -9.8 {
            gravity_stores += 1;
        }
    }
    assert_eq!(
        anim_containers, 61,
        "the census ran over the installation's 61 animation containers"
    );
    assert_eq!(
        gravity_stores, anim_containers,
        "every one stores the f32 -9.8 gravity word: the landmark that pins the \
         SI reading of the stored length unit"
    );

    // ---- landmarks 2 and 3: the pilot figure and the eleven airframes ----
    let planes_bytes = read("planes.zbd");
    let mut context = ParseContext::with_defaults("accept_m01_lc_world_unit_roles");
    let records = read_gamez_nodes(&mut context, &planes_bytes).expect("planes.zbd reads");
    let planes_file = found
        .manifest
        .files
        .iter()
        .find(|f| f.relative_spelling.as_str().ends_with("planes.zbd"))
        .expect("planes.zbd");
    let adapter = SourceAdapter::new(CoordinateSource::retail_gamez(
        SourceSpan::new(
            planes_file.sha256,
            planes_file.relative_spelling.logical_key().as_str(),
            None,
            0,
            planes_file.size_bytes,
            None,
        )
        .expect("planes.zbd's own span"),
    ));
    let slot_count = (records
        .nodes
        .iter()
        .map(|node| node.mesh_index())
        .max()
        .unwrap_or(-1)
        .max(0) as usize)
        + 1;
    let graph = scene_graph_from_gamez(
        &ContentId::from_source(ContentKind::World, "m01-lc-world-unit-roles-planes")
            .expect("the probe's container key is valid"),
        &records,
        &(0..slot_count)
            .map(|index| {
                MeshSlot::new(
                    ContentId::from_source(ContentKind::Mesh, &format!("probe-mesh-{index}"))
                        .expect("a mesh key is valid"),
                    Provenance::designed(
                        ClaimId::new("m01-lc-world-unit-roles.probe")
                            .expect("the probe's claim id is valid"),
                    ),
                )
                .expect("a mesh-kind id is a mesh slot")
            })
            .collect::<Vec<_>>(),
        &adapter,
        &BindingMap::default(),
    )
    .expect("the aircraft hierarchy converts whole");

    // The composed extent of one root's subtree, in stored units: each member's
    // own stored `unk140` box corners carried through the member's composed
    // `SceneGraph` transform. This is the measurement path the census took —
    // a subtree's stored boxes are stated in each record's own frame, so the
    // composed corners are what the airframe's size actually is.
    let composed_extent = |root: &str| -> [f64; 3] {
        let node = graph
            .nodes()
            .iter()
            .find(|candidate| candidate.name() == root)
            .unwrap_or_else(|| panic!("{root} is a parentless planes.zbd root"));
        let mut low = [f64::INFINITY; 3];
        let mut high = [f64::NEG_INFINITY; 3];
        for member in graph.subtree(node.id()) {
            let raw = records
                .nodes
                .iter()
                .find(|candidate| candidate.index == member.index())
                .unwrap_or_else(|| panic!("node slot {} is in the container", member.index()));
            for corner in [
                [false, false, false],
                [true, false, false],
                [false, true, false],
                [true, true, false],
                [false, false, true],
                [true, false, true],
                [false, true, true],
                [true, true, true],
            ] {
                let local = [
                    if corner[0] {
                        f64::from(raw.info.unk140[1][0])
                    } else {
                        f64::from(raw.info.unk140[0][0])
                    },
                    if corner[1] {
                        f64::from(raw.info.unk140[1][1])
                    } else {
                        f64::from(raw.info.unk140[0][1])
                    },
                    if corner[2] {
                        f64::from(raw.info.unk140[1][2])
                    } else {
                        f64::from(raw.info.unk140[0][2])
                    },
                ];
                let world = member.world_transform().apply(local);
                for axis in 0..3 {
                    low[axis] = low[axis].min(world[axis]);
                    high[axis] = high[axis].max(world[axis]);
                }
            }
        }
        [high[0] - low[0], high[1] - low[1], high[2] - low[2]]
    };

    for root in ["cpilot", "pickup_cpilot"] {
        let extent = composed_extent(root);
        assert!(
            (0.6..0.8).contains(&extent[0])
                && (1.8..2.0).contains(&extent[1])
                && (0.4..0.6).contains(&extent[2]),
            "{root}: about 0.7 x 1.9 x 0.5 stored units is a standing human at the \
             metre and a 0.58 m figure at the foot; measured {extent:?}"
        );
    }

    let mut airframe_spans: Vec<f64> = ROSTER_AIRFRAMES
        .iter()
        .map(|root| {
            let extent = composed_extent(root);
            extent[0].max(extent[1]).max(extent[2])
        })
        .collect();
    airframe_spans.sort_by(|left, right| left.partial_cmp(right).expect("finite extents"));
    assert_eq!(ROSTER_AIRFRAMES.len(), 11, "the roster is eleven airframes");
    assert!(
        (8.0..12.0).contains(&airframe_spans[0]) && (26.0..28.0).contains(&airframe_spans[10]),
        "the eleven airframe roots span roughly 8.8-27.2 stored units — \
         fighter-class aircraft at the metre, an 8.3 m 'fighter' at the foot; \
         measured {:.2}..{:.2}",
        airframe_spans[0],
        airframe_spans[10]
    );

    // ---- landmark 4: the stored LOD switch ranges ----
    // The finite bands only: one record stores 10 000 000 as its far bound,
    // which is "no far bound" rather than a ten-thousand-kilometre view
    // distance, and a landmark about working distances does not read it.
    const UNBOUNDED_FAR: f64 = 10_000_000.0;
    let mut finite_bounds: Vec<f64> = Vec::new();
    let mut lod_records = 0usize;
    for node in graph.nodes() {
        let Some(lod) = node.lod() else { continue };
        lod_records += 1;
        for bound in [lod.range_min.0, lod.range_max.0] {
            if bound > 0.0 && bound < UNBOUNDED_FAR {
                finite_bounds.push(bound);
            }
        }
    }
    let mut low = f64::INFINITY;
    let mut high = f64::NEG_INFINITY;
    for value in &finite_bounds {
        low = low.min(*value);
        high = high.max(*value);
    }
    assert!(
        (40.0..60.0).contains(&low) && (2900.0..3100.0).contains(&high),
        "the aircraft nodes' LOD switch ranges run 50-3000 stored units — tens \
         of metres to about three kilometres, which only read as distances in \
         metres; measured {low:.1}..{high:.1} over {lod_records} LOD records"
    );

    // ---- landmark 5: the world theatre and its grid cells ----
    // The theatre is the sectors the import publishes, in stored units, and the
    // cell size is the pitch between neighbouring grid cells' own stored header
    // floats. Both come from the production world path.
    let worlds = read_world_containers(&root).expect("the world containers are discovered");
    let mut theatre_low = f64::INFINITY;
    let mut theatre_high = f64::NEG_INFINITY;
    let mut cell_pitches: Vec<f64> = Vec::new();
    for group in worlds.groups() {
        let container = worlds
            .container(&group, &WorldTextureLoad::project_default())
            .expect("the container reads");
        let grid = container.partition_grid().expect("the grid reads");
        let mut previous: Option<f32> = None;
        for cell in grid.cells() {
            let origin = cell.header_floats()[0];
            if let Some(before) = previous {
                let pitch = f64::from(origin) - f64::from(before);
                if pitch > 0.0 {
                    cell_pitches.push(pitch);
                }
            }
            previous = Some(origin);
        }
        let imported = container
            .definition(origin(&container), &retail_adapter(&container))
            .expect("the container imports");
        for sector in imported.definition().sectors() {
            let bounds = sector.bounds();
            for axis in [0usize, 2] {
                theatre_low = theatre_low.min(bounds.min()[axis]);
                theatre_high = theatre_high.max(bounds.max()[axis]);
            }
        }
    }
    cell_pitches.sort_by(|left, right| left.partial_cmp(right).expect("finite pitches"));
    let median_pitch = cell_pitches[cell_pitches.len() / 2];
    assert!(
        (900.0..1100.0).contains(&median_pitch),
        "the partition grid's cell pitch is a kilometre of stored units — the \
         published sector extents agree, so a '300-unit cell' reading of this \
         container is a misreading, not a measurement; median pitch {median_pitch:.1} \
         over {} cells",
        cell_pitches.len()
    );
    assert!(
        theatre_low < -15000.0 && theatre_high > 200.0 && theatre_high < 300.0,
        "the world containers' published sector bounds run about -16384..256 \
         stored units on the horizontal axes: a 12-16 km archipelago at the \
         metre and a 3.7-5 km map at the foot; measured \
         {theatre_low:.1}..{theatre_high:.1}"
    );

    // ---- and the landmarks' own text must still say what was measured ----
    let calibration = CoordinateSource::retail_gamez(synthetic_span()).calibration();
    let descriptions: Vec<String> = calibration
        .landmarks()
        .iter()
        .map(|landmark| landmark.description().to_owned())
        .collect();
    assert!(
        descriptions
            .iter()
            .any(|text| text.contains("61 animation containers") && text.contains("-9.8")),
        "the gravity landmark still names the census it rests on: {descriptions:?}"
    );
    assert!(
        !descriptions
            .iter()
            .any(|text| text.contains("300-m dogfight cells") || text.contains("grid cells ~300")),
        "no landmark may claim a cell size the grid does not state: {descriptions:?}"
    );
}

/// **The measured source pins the scale at `observed_tool` and reports the
/// rest of the convention as the gaps it still is.**
///
/// The discriminating half: a source that claimed `verified_original` off
/// unfingerprinted tool evidence, or a calibration that reported itself
/// complete after one quantity's census, would both fail here.
#[test]
fn accept_m01_lc_world_unit_roles_the_scale_is_measured_and_the_convention_stays_open() {
    let source = CoordinateSource::retail_gamez(synthetic_span());
    assert_eq!(source.label(), "retail.gamez");
    assert_eq!(
        source.convention().meters_per_unit(),
        1.0,
        "the census pins one stored unit to the metre"
    );
    assert_eq!(
        source.provenance().claim_id.as_str(),
        GAMEZ_VERTEX_UNIT_IS_THE_METRE,
        "and the declaration names the claim it stands on"
    );
    assert_eq!(
        source.provenance().class,
        ClaimStatus::ObservedTool,
        "at the strongest class a byte census without an original run can carry"
    );

    let calibration = source.calibration();
    assert_eq!(
        calibration.quantity_status(CalibratedQuantity::Scale),
        ClaimStatus::ObservedTool,
        "the scale's own evidence class is what the import reports"
    );
    assert!(
        calibration.landmark_count(CalibratedQuantity::Scale) >= 3,
        "the quantity rule's minimum landmark count is met"
    );
    assert!(
        calibration.behavior_landmark_count(CalibratedQuantity::Scale) >= 1,
        "and at least one of them is an observed behaviour, as the rule requires"
    );

    // The other three quantities were never observed: each stays a gap, so the
    // whole-convention answer is honestly `Unknown` — measured is not complete.
    assert!(!calibration.is_complete());
    let gaps: Vec<_> = calibration.gaps().iter().map(|gap| gap.quantity).collect();
    for quantity in [
        CalibratedQuantity::Handedness,
        CalibratedQuantity::AxisOrder,
        CalibratedQuantity::AngleUnit,
    ] {
        assert!(
            gaps.contains(&quantity),
            "{} is unmeasured and the gap list says so",
            quantity.label()
        );
        assert_eq!(
            calibration.quantity_status(quantity),
            ClaimStatus::Unknown,
            "and its per-quantity class is unknown, not borrowed from the scale"
        );
    }
    assert_eq!(
        calibration.claim_status(),
        ClaimStatus::Unknown,
        "an incomplete calibration claims nothing at the convention level"
    );
    assert_ne!(
        calibration.claim_status(),
        ClaimStatus::VerifiedOriginal,
        "tool evidence without an original run never verifies the original"
    );

    // And the adapter built from it converts under the measured factor: a
    // stored position arrives scaled by exactly the metre.
    let adapter = SourceAdapter::new(source);
    assert_eq!(
        adapter.source().convention().meters_per_unit(),
        1.0,
        "the adapter carries the measured factor, not an assumed one"
    );
}

/// **A declared source still reports the unit unmeasured — the measurement is
/// the landmark census, not the number.** A conversion that declared `1.0` and
/// stopped there must read differently from one that measured it.
#[test]
fn accept_m01_lc_world_unit_roles_a_declared_source_stays_unmeasured() {
    let canonical = SourceAdapter::declared()
        .into_iter()
        .find(|adapter| adapter.source().label() == "canonical")
        .expect("the F16-A registry declares the canonical source");
    let calibration = canonical.source().calibration();
    assert_eq!(
        canonical.source().convention().meters_per_unit(),
        1.0,
        "the declared factor is also one — the class is what differs"
    );
    assert_eq!(
        calibration.quantity_status(CalibratedQuantity::Scale),
        ClaimStatus::Unknown,
        "declared is not measured: the same number without landmarks is unknown"
    );
    assert_eq!(
        calibration.claim_status(),
        ClaimStatus::Unknown,
        "and the declared convention makes no calibration claim at all"
    );
}

/// **All eight containers show the same split: every unindexed record either
/// stores no geometry (resolved `None`) or stores some (measured unknown).**
///
/// The per-group numbers are the census, and the object-level cross-check is
/// the discriminator: a `None` role on a mesh-bearing record — or a `Solid`
/// guessed onto an `fvol` — fails here, as does any unindexed record that is
/// neither.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_world_unit_roles_every_container_unindexed_split_is_measured() {
    let found = retail();

    for split in &SPLITS {
        let container = found
            .container(split.group, &WorldTextureLoad::project_default())
            .unwrap_or_else(|error| panic!("{}: the container reads: {error}", split.group));
        let imported = container
            .definition(origin(&container), &retail_adapter(&container))
            .unwrap_or_else(|error| panic!("{}: the container imports: {error}", split.group));
        let report = imported.report();
        let world = imported.definition();

        // The report's own census fields, against the measured table.
        assert_eq!(
            report.stored_child_list(),
            split.unindexed,
            "{}: the world node's own stored child list",
            split.group
        );
        assert_eq!(
            report.objects_unindexed_none(),
            split.anchors,
            "{}: the unindexed records with no mesh and no extent resolve to `None`",
            split.group
        );
        assert_eq!(
            report.objects_unindexed_unresolved(),
            split.volumes,
            "{}: the unindexed records that store geometry stay unknown",
            split.group
        );
        assert_eq!(
            split.anchors + split.volumes,
            split.unindexed,
            "{}: the two classes partition the unindexed records exactly — no \
             third shape exists in the corpus",
            split.group
        );

        // The object-level consequence, checked per record rather than as a
        // count: `None` is answered only by a record whose own store holds no
        // geometry, and the role the container owes is asked only of one that
        // does.
        for object in world.objects() {
            match object.known_collision() {
                Some(WorldCollisionRole::None) => {
                    assert!(
                        !object.mesh().is_known(),
                        "{}: {} resolved to `None` but names a mesh — the store \
                         holds geometry the role denies",
                        split.group,
                        object.id().as_str()
                    );
                    let Resolved::Unknown { claim_id, .. } = object.shape() else {
                        panic!(
                            "{}: {} has no geometry to take a shape from",
                            split.group,
                            object.id().as_str()
                        );
                    };
                    assert_eq!(claim_id.as_str(), UNINDEXED_RECORD_STORES_NO_GEOMETRY);
                    let Resolved::Unknown { claim_id, .. } = object.mesh() else {
                        panic!(
                            "{}: {} binds no mesh, and says so",
                            split.group,
                            object.id().as_str()
                        );
                    };
                    assert_eq!(claim_id.as_str(), OBJECT_STORES_NO_MESH);
                }
                Some(WorldCollisionRole::Solid) => {
                    assert_eq!(
                        object.known_shape(),
                        Some(cs_content::world::WorldCollisionShape::FromMesh),
                        "{}: {} resolved solid, so its collider is derived from \
                         its own mesh — the indexed rule keeps the pairing",
                        split.group,
                        object.id().as_str()
                    );
                }
                Some(WorldCollisionRole::Sensor) => {
                    panic!(
                        "{}: {} is a sensor — nothing in the container measured \
                         that, so a sensor role is a guess",
                        split.group,
                        object.id().as_str()
                    );
                }
                None => {
                    let Resolved::Unknown { claim_id, .. } = object.collision() else {
                        unreachable!("checked above")
                    };
                    assert_eq!(
                        claim_id.as_str(),
                        UNINDEXED_ROLE_UNMEASURED,
                        "{}: {} keeps the unknown the container owes it",
                        split.group,
                        object.id().as_str()
                    );
                    assert!(
                        object.mesh().is_known(),
                        "{}: {} asks the role question of a record that stores \
                         geometry — an anchor would have resolved `None`",
                        split.group,
                        object.id().as_str()
                    );
                }
            }
        }

        // And the unit the import ran under: the measured metre, at its own
        // evidence class, on the definition the runtime consumes.
        assert_eq!(
            report.meters_per_unit(),
            1.0,
            "{}: one stored unit is the metre",
            split.group
        );
        assert_eq!(
            report.unit_class(),
            ClaimStatus::ObservedTool,
            "{}: the factor's evidence is the census, not a guess and not an \
             original run",
            split.group
        );
    }
}

/// **On c1c, the 53 records the task asked about land as 36 presented anchors
/// and 17 still-unresolved `fvol` volumes, and the spawn says so.** (retail)
///
/// This is the number the `world_geometry` blocker was waiting on: the
/// definition imported from `ZBD/C1C/gamez.zbd` carries no invented roles, and
/// the spawn report shows exactly which records block and which never could.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_world_unit_roles_the_c1c_spawn_reports_the_measured_split() {
    let found = retail();
    let container = found
        .container("C1C", &WorldTextureLoad::project_default())
        .expect("the c1c geometry container reads");
    let imported = container
        .definition(origin(&container), &retail_adapter(&container))
        .expect("the c1c container imports");
    let world = imported.definition();
    let meshes = container
        .uploaded_meshes(world)
        .expect("the container's own meshes upload");

    let mut app = world_app();
    let spawned = spawn_world(&mut app, world, &meshes).expect("the world spawns");
    for _ in 0..MESH_SETTLE_UPDATES {
        app.update();
    }

    assert_eq!(spawned.objects().len(), world.objects().len());
    assert_eq!(
        spawned.colliders().len(),
        imported.report().partition_records_with_mesh(),
        "every indexed record that binds a mesh collides, from that mesh"
    );
    let mut reasons: BTreeMap<&str, usize> = BTreeMap::new();
    for entry in spawned.skipped() {
        *reasons.entry(entry.reason.label()).or_default() += 1;
    }
    assert_eq!(
        reasons,
        BTreeMap::from([("unknown_collision_role", 17), ("unknown_mesh", 1),]),
        "exactly the 17 geometry-bearing unindexed records report the missing \
         role, and the one mesh-less indexed record reports its own gap"
    );
    assert_eq!(
        spawned.non_colliding().len(),
        36,
        "the 36 anchors are presented and deliberately never block: a resolved \
         `None` is not a skip"
    );
    assert_eq!(spawned.presentation_gap_count(), 0);
}
