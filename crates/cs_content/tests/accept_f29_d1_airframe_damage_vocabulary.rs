//! Acceptance scenario F29-D.1: the measured original airframe
//! damage-region vocabulary bound into F29's declared graph shape
//! (`specs/F29-damage-zones-armor-destruction-and-bailout.md`, stages
//! `### F29-B` and `### F29-D`; shared contract
//! `docs/contracts/STATE-TRANSACTIONS.md`).
//!
//! Three things are under test, and nothing else:
//!
//! 1. **The measurement.** `cs_content::damage::observe_airframe_damage_
//!    vocabulary` reads `ZBD/planes.zbd` with `cs_formats`' production
//!    readers — never a side channel, never a table this file carries — and
//!    records the four damage-region node names and the eleven
//!    `<prefix>_damage` materials the container stores, each with the span it
//!    was read from under an `observed_tool` provenance.
//! 2. **The record.** The declared schema carries exactly those names, that
//!    container key and that SHA-256, with `Origin::Installation` provenance.
//! 3. **The lowering.** `DeclaredDamageGraph::declare_airframe_regions`
//!    accepts a region slot count only when it equals the number the
//!    observation measures, and refuses anything else *by name* — the
//!    subject, the declared count and the observed count in the message.
//!
//! The retail test is `#[ignore = "requires CS_GAME_DIR"]` and fails loudly
//! without `CS_GAME_DIR`: its claim is about the installation's bytes and no
//! fixture can make it. The unignored tests are the fast synthetic
//! regression CI runs, and they are what breaks if the lowering is removed
//! or if it accepts a count the observation never measured.

use std::collections::BTreeMap;
use std::path::PathBuf;

use cs_content::damage::{
    AirframeDamageObservationError, DamageNodeKey, DamageNodeKind, DeclaredDamageGraph,
    DeclaredDamageNode, GraphRules, GraphSubjectKind, synthetic_airframe_damage_vocabulary,
};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ClaimStatus};

// ------------------------------------------------------------- fixtures ---

fn claim() -> ClaimId {
    ClaimId::new("f29d1.acceptance").expect("the fixture claim id is valid")
}

fn key(name: &str) -> DamageNodeKey {
    DamageNodeKey::new(name).expect("the fixture slot keys are valid")
}

/// Region slot keys, one per name, so a test can declare a count that does
/// or does not match its observation.
fn slot_keys(prefix: &str, count: usize) -> Vec<DamageNodeKey> {
    (0..count)
        .map(|position| key(&format!("{prefix}_{position}")))
        .collect()
}

/// The airframe graph the fixture provides: an `Aircraft` subject with four
/// nodes, so the shape has a graph to refuse or accept.
fn airframe_graph() -> DeclaredDamageGraph {
    cs_content::damage::declared_synthetic_airframe_damage()
}

/// A declared graph that does **not** describe an aircraft, so the shape's
/// own guard is reachable.
fn world_graph() -> DeclaredDamageGraph {
    DeclaredDamageGraph::try_new(
        ContentId::from_source(ContentKind::World, "synthetic.depot").expect("a valid world id"),
        Origin::SyntheticFixture,
        GraphSubjectKind::WorldObject,
        GraphRules {
            lethal_attribution: Resolved::Known(Known::new(
                cs_content::damage::AttributionRule::FirstLethalHit,
                Provenance::designed(claim()),
            )),
        },
        vec![DeclaredDamageNode {
            key: key("hull"),
            kind: DamageNodeKind::InternalStructure,
            scene_binding: None,
            integrity: Resolved::Known(Known::new(10.0, Provenance::designed(claim()))),
            lethal: true,
            disables: None,
            guarded_by: None,
            overflow: None,
        }],
        Provenance::designed(claim()),
    )
    .expect("the fixture world graph is valid")
}

fn region_count_error(
    result: Result<DeclaredDamageGraph, cs_content::damage::DamageSchemaError>,
) -> cs_content::damage::DamageSchemaError {
    match result {
        Ok(_) => panic!(
            "the lowering accepted a region count its observation never measured: a missing or \
             permissive check cannot pass this suite"
        ),
        Err(error) => error,
    }
}

// ------------------------------------------------------- synthetic shape ---

/// The accepted count is the one the observation holds, not a constant:
/// a vocabulary measuring three regions makes four a refusal, so a check
/// that hard-coded `4` — or that accepted anything — fails here.
#[test]
fn accept_f29_d1_region_shape_reads_its_counts_from_the_observation() {
    let graph = airframe_graph();

    let three = synthetic_airframe_damage_vocabulary(&["a_region", "b_region", "c_region"], None);
    assert_eq!(three.region_count(), 3);
    let error = region_count_error(graph.declare_airframe_regions(
        slot_keys("slot", 4),
        Vec::new(),
        &three,
    ));
    assert_eq!(
        error.to_string(),
        "airframe airframe/synthetic.devastator declares 4 damage-region slots, but its measured \
         vocabulary holds 3"
    );
    let declared = graph
        .declare_airframe_regions(slot_keys("slot", 3), Vec::new(), &three)
        .expect("three declared slots are exactly what a three-region observation measures");
    assert_eq!(
        declared
            .airframe_regions()
            .expect("the shape is stored")
            .regions()
            .len(),
        3
    );

    let four = synthetic_airframe_damage_vocabulary(
        &["a_region", "b_region", "c_region", "d_region"],
        None,
    );
    assert_eq!(four.region_count(), 4);
    graph
        .declare_airframe_regions(slot_keys("slot", 4), Vec::new(), &four)
        .expect("four declared slots match the four-region observation");
    let too_few =
        region_count_error(graph.declare_airframe_regions(slot_keys("slot", 3), Vec::new(), &four));
    assert!(
        too_few
            .to_string()
            .contains("declares 3 damage-region slots")
            && too_few.to_string().contains("vocabulary holds 4"),
        "{too_few}"
    );
    let too_many =
        region_count_error(graph.declare_airframe_regions(slot_keys("slot", 5), Vec::new(), &four));
    assert!(
        too_many
            .to_string()
            .contains("declares 5 damage-region slots")
            && too_many.to_string().contains("vocabulary holds 4"),
        "{too_many}"
    );
}

/// The wreck half of the shape: a graph may declare no wreck slot, and at
/// most what one measured airframe binds. The ceiling is read from the
/// observation, so a fixture measuring no wreck material refuses one.
#[test]
fn accept_f29_d1_region_shape_refuses_more_wreck_slots_than_one_airframe_binds() {
    let graph = airframe_graph();
    let regions = slot_keys("region", 4);

    let bound = synthetic_airframe_damage_vocabulary(
        &["a_region", "b_region", "c_region", "d_region"],
        Some("synthetic_damage"),
    );
    assert_eq!(bound.max_wreck_materials_per_airframe(), 1);
    graph
        .declare_airframe_regions(regions.clone(), slot_keys("wreck", 1), &bound)
        .expect("one wreck slot is what one measured airframe binds");
    graph
        .declare_airframe_regions(regions.clone(), Vec::new(), &bound)
        .expect("declaring no wreck slot is always within the measured ceiling");
    let error = region_count_error(graph.declare_airframe_regions(
        regions.clone(),
        slot_keys("wreck", 2),
        &bound,
    ));
    assert!(
        error
            .to_string()
            .contains("declares 2 wreck presentation slots")
            && error.to_string().contains("binds at most 1"),
        "{error}"
    );

    let unbound = synthetic_airframe_damage_vocabulary(
        &["a_region", "b_region", "c_region", "d_region"],
        None,
    );
    assert_eq!(unbound.max_wreck_materials_per_airframe(), 0);
    graph
        .declare_airframe_regions(regions.clone(), Vec::new(), &unbound)
        .expect("an observation measuring no wreck material still accepts no wreck slot");
    let error = region_count_error(graph.declare_airframe_regions(
        regions,
        slot_keys("wreck", 1),
        &unbound,
    ));
    assert!(
        error
            .to_string()
            .contains("declares 1 wreck presentation slots")
            && error.to_string().contains("binds at most 0"),
        "{error}"
    );
}

/// The slot lists are sets: a repeated key cannot inflate the count the
/// check compares, and the shape is refused for a graph that is not an
/// aircraft at all — every refusal names its subject.
#[test]
fn accept_f29_d1_region_shape_refuses_duplicate_slots_and_non_aircraft_graphs() {
    let vocabulary = synthetic_airframe_damage_vocabulary(
        &["a_region", "b_region", "c_region", "d_region"],
        Some("synthetic_damage"),
    );

    let mut duplicated = slot_keys("region", 3);
    duplicated.push(key("region_0"));
    let error = region_count_error(airframe_graph().declare_airframe_regions(
        duplicated,
        Vec::new(),
        &vocabulary,
    ));
    assert!(
        error
            .to_string()
            .contains("declares the shape slot region_0 more than once"),
        "{error}"
    );

    let error = region_count_error(world_graph().declare_airframe_regions(
        slot_keys("region", 4),
        Vec::new(),
        &vocabulary,
    ));
    assert!(
        error.to_string().contains(
            "cannot be declared for world/synthetic.depot, which describes a world_object"
        ),
        "{error}"
    );

    // The accepted shape carries the observation's own provenance, so the
    // record the schema holds stays traceable to the measurement.
    let accepted = airframe_graph()
        .declare_airframe_regions(slot_keys("region", 4), Vec::new(), &vocabulary)
        .expect("the well-formed declaration is accepted");
    let shape = accepted.airframe_regions().expect("the shape is stored");
    assert_eq!(shape.regions().len(), 4);
    assert_eq!(shape.wreck_slots().len(), 0);
    assert_eq!(shape.provenance(), vocabulary.provenance());
    assert_eq!(shape.provenance().class, ClaimStatus::Designed);
}

// --------------------------------------------------------------- retail ---

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

/// F11-D2's measured eleven-airframe roster
/// (`docs/findings/2026-10-02-f11-d-2-airframe-roster-discovery.md`),
/// transcribed as the catalog ids that discovery read, so the per-airframe
/// claim below is checked against an independently measured list rather than
/// against a count.
const RETAIL_AIRFRAME_ROSTER: [&str; 11] = [
    "player_pfighter",
    "player_bhawk",
    "player_fbrand",
    "player_brigand",
    "player_fury",
    "player_autogyro",
    "player_avenger",
    "player_kestrel",
    "player_peacemaker",
    "player_warhawk",
    "player_balmoral",
];

/// The four damage-region node names the container stores, measured through
/// the production readers and pinned here so a silent change of the
/// selection rule is a failure.
const RETAIL_REGION_NAMES: [&str; 4] = [
    "leftwingdamage",
    "nosedamage",
    "rightwingdamage",
    "taildamage",
];

/// The eleven `<prefix>_damage` material stems the container stores, pinned
/// the same way.
const RETAIL_WRECK_STEMS: [&str; 11] = [
    "agyro_damage",
    "avenger_damage",
    "bal_damage",
    "bldhwk_damage",
    "brigand_damage",
    "de_damage",
    "firebrand_damage",
    "fury_damage",
    "ke_damage",
    "pm_damage",
    "whawk_damage",
];

/// Which airframe's group binds which material, measured from the mesh
/// section: each airframe subtree's meshes reference exactly one of the
/// eleven materials, and it is always the one whose prefix is that
/// airframe's own (`de_damage` is the pirate fighter's, which is what the
/// bytes say).
const RETAIL_WRECK_BY_AIRFRAME: [(&str, &str); 11] = [
    ("player_pfighter", "de_damage"),
    ("player_bhawk", "bldhwk_damage"),
    ("player_fbrand", "firebrand_damage"),
    ("player_brigand", "brigand_damage"),
    ("player_fury", "fury_damage"),
    ("player_autogyro", "agyro_damage"),
    ("player_avenger", "avenger_damage"),
    ("player_kestrel", "ke_damage"),
    ("player_peacemaker", "pm_damage"),
    ("player_warhawk", "whawk_damage"),
    ("player_balmoral", "bal_damage"),
];

/// The SHA-256 of `ZBD/planes.zbd` itself, the container the vocabulary is
/// read from.
const RETAIL_CONTAINER_SHA256: &str =
    "45da54a8e1886a8182e84bef03eb5e099481e483d79e458e7db5356538fbc21b";

/// **The installation really stores this vocabulary.**
///
/// Everything below is read by `observe_airframe_damage_vocabulary` through
/// `read_gamez_nodes`, `read_gamez_materials` and `read_gamez_meshes`; this
/// test never scans the file itself. It pins what those readers produced:
/// the container identity, the four region names with one record per
/// airframe group, the eleven wreck materials with one binding per airframe,
/// the names the selection rule left out, and every span read back out of
/// the container's own bytes.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f29_d1_retail_the_installation_names_four_regions_and_eleven_wreck_materials() {
    let dir = game_dir();
    let vocabulary =
        cs_content::damage::observe_airframe_damage_vocabulary(&dir).unwrap_or_else(|error| {
            panic!("the production reader must measure the installation: {error}")
        });

    // The container identity and the provenance the record carries.
    assert_eq!(
        vocabulary.container().to_string(),
        "install_file/zbd_2f_planes.zbd"
    );
    assert_eq!(
        vocabulary.container_sha256().to_hex(),
        RETAIL_CONTAINER_SHA256,
        "the container digest is the installation's own file digest"
    );
    assert_eq!(vocabulary.provenance().class, ClaimStatus::ObservedTool);
    assert_eq!(
        vocabulary.provenance().claim_id.as_str(),
        "f29d1.airframe-damage-vocabulary"
    );
    let Origin::Installation { source } = vocabulary.origin() else {
        panic!(
            "a retail measurement must carry Origin::Installation, found {:?}",
            vocabulary.origin()
        )
    };
    assert_eq!(source.container_path(), "ZBD/planes.zbd");
    assert_eq!(source.install_sha256(), vocabulary.install_sha256());
    assert_eq!(source.offset(), 0);
    assert_eq!(
        source.member_sha256(),
        Some(vocabulary.container_sha256()),
        "the whole-container span is bound to the container's own digest"
    );
    assert_eq!(
        source.length() as usize,
        std::fs::metadata(dir.join("ZBD/planes.zbd"))
            .expect("the container is readable")
            .len() as usize
    );
    assert_eq!(
        vocabulary.provenance().source.as_ref(),
        Some(source),
        "the provenance locates the same bytes the origin does"
    );

    // The measured region vocabulary: four names, forty-four records.
    assert_eq!(
        vocabulary.region_names(),
        RETAIL_REGION_NAMES
            .iter()
            .map(|name| (*name).to_owned())
            .collect::<Vec<_>>()
    );
    assert_eq!(vocabulary.region_count(), 4);

    let mut per_name: BTreeMap<&str, usize> = BTreeMap::new();
    let mut zone_ids: BTreeMap<u32, usize> = BTreeMap::new();
    for group in vocabulary.groups() {
        for region in &group.regions {
            *per_name.entry(region.name.as_str()).or_default() += 1;
            *zone_ids.entry(region.zone_id).or_default() += 1;
        }
    }
    assert_eq!(
        per_name.values().copied().collect::<Vec<_>>(),
        vec![11, 11, 11, 11],
        "each of the four names is stored once per airframe group"
    );
    assert_eq!(
        zone_ids.get(&255).copied(),
        Some(44),
        "every region node stores the zone id default: no measured node is assigned a zone"
    );

    // Per airframe: eleven groups, one roster airframe each, four regions.
    assert_eq!(vocabulary.groups().len(), 11);
    let mut bound_airframes: BTreeMap<&str, usize> = BTreeMap::new();
    let mut roster_hits: BTreeMap<&str, usize> = BTreeMap::new();
    for group in vocabulary.groups() {
        let mut matches = Vec::new();
        for airframe in RETAIL_AIRFRAME_ROSTER {
            if group.ancestry.iter().any(|node| node.name == airframe) {
                matches.push(airframe);
            }
        }
        assert_eq!(
            matches.len(),
            1,
            "each region group hangs under exactly one measured airframe, ancestry {:?}",
            group.ancestry
        );
        let airframe = matches[0];
        *bound_airframes.entry(airframe).or_default() += 1;
        *roster_hits.entry(airframe).or_default() += 1;
        let mut names = group.region_names();
        names.sort_unstable();
        assert_eq!(
            names,
            RETAIL_REGION_NAMES.to_vec(),
            "every airframe group holds all four region names"
        );
        assert_eq!(
            group.ancestry[0].name, "damageindicator",
            "the four regions are siblings under one authored node"
        );
    }
    assert_eq!(
        roster_hits.values().copied().collect::<Vec<_>>(),
        vec![1; 11],
        "every one of F11-D2's eleven measured airframes owns one region group"
    );
    assert_eq!(bound_airframes.len(), 11);

    // The eleven wreck materials, one present material record and one
    // airframe binding each.
    assert_eq!(
        vocabulary.wreck_material_stems(),
        RETAIL_WRECK_STEMS
            .iter()
            .map(|stem| (*stem).to_owned())
            .collect::<Vec<_>>()
    );
    assert_eq!(vocabulary.wreck_materials().len(), 11);
    assert_eq!(vocabulary.max_wreck_materials_per_airframe(), 1);
    let mut by_airframe: BTreeMap<&str, &str> = BTreeMap::new();
    for material in vocabulary.wreck_materials() {
        assert_eq!(
            material.material_indices.len(),
            1,
            "each wreck texture is named by exactly one present material record"
        );
        assert_eq!(
            material.bindings.len(),
            1,
            "{} is bound by exactly one airframe subtree",
            material.stem
        );
        let binding = &material.bindings[0];
        let group = vocabulary
            .groups()
            .iter()
            .find(|group| group.parent() == binding.group_parent)
            .expect("a binding names a group that was measured");
        let airframe = RETAIL_AIRFRAME_ROSTER
            .iter()
            .find(|airframe| group.ancestry.iter().any(|node| node.name == **airframe))
            .expect("the binding's group hangs under a measured airframe");
        let previous = by_airframe.insert(airframe, material.stem.as_str());
        assert_eq!(
            previous, None,
            "{airframe} binds more than one wreck material"
        );
    }
    let expected: BTreeMap<&str, &str> = RETAIL_WRECK_BY_AIRFRAME.into_iter().collect();
    assert_eq!(by_airframe, expected);

    // What the selection rule did not take, so the rule's boundary is part
    // of the record and not a silent filter.
    let discarded: Vec<(String, usize, &'static str)> = vocabulary
        .discarded()
        .iter()
        .map(|entry| (entry.name.clone(), entry.occurrences, entry.table.label()))
        .collect();
    assert_eq!(
        discarded,
        vec![
            ("damageindicator".to_owned(), 11, "nodes"),
            ("player_damage_off".to_owned(), 14, "nodes"),
            ("player_damage_on".to_owned(), 12, "nodes"),
            ("damage1.tif".to_owned(), 1, "textures"),
            ("damage2.tif".to_owned(), 1, "textures"),
        ],
        "the names carrying the marker but not the suffix are kept, ordered by table then name"
    );

    // Every span reads the stored name back out of the container's bytes.
    let bytes = std::fs::read(dir.join("ZBD/planes.zbd")).expect("the container is readable");
    let read_back = |label: &str, span: &cs_types::asset_id::SourceSpan, needle: &str| {
        let start = usize::try_from(span.offset()).expect("an offset fits a usize");
        let end = start
            .checked_add(usize::try_from(span.length()).expect("a length fits a usize"))
            .expect("the span is inside the container");
        let window = &bytes
            .get(start..end)
            .unwrap_or_else(|| panic!("{label}: span {start}..{end} is outside the container"));
        assert!(
            window
                .windows(needle.len())
                .any(|candidate| candidate == needle.as_bytes()),
            "{label}: {needle:?} is not inside its own span {start}..{end}"
        );
        assert_eq!(span.install_sha256(), vocabulary.install_sha256());
        assert_eq!(span.member_sha256(), Some(vocabulary.container_sha256()));
    };
    for group in vocabulary.groups() {
        for region in &group.regions {
            read_back(&region.name, &region.span, &region.name);
        }
    }
    for material in vocabulary.wreck_materials() {
        read_back(&material.stem, &material.span, &material.stem);
    }

    // The lowering: the declared schema accepts the measured count and
    // refuses every other one by name.
    let graph = airframe_graph();
    let region_slots: Vec<DamageNodeKey> = vocabulary
        .region_names()
        .iter()
        .map(|name| key(name))
        .collect();
    let declared = graph
        .declare_airframe_regions(region_slots.clone(), slot_keys("wreck", 1), &vocabulary)
        .expect("the measured count is the accepted count");
    let shape = declared.airframe_regions().expect("the shape is stored");
    assert_eq!(shape.regions().len(), 4);
    assert_eq!(shape.wreck_slots().len(), 1);
    assert_eq!(
        shape.provenance(),
        vocabulary.provenance(),
        "the declared shape carries the measurement's own provenance"
    );
    assert_eq!(shape.provenance().class, ClaimStatus::ObservedTool);

    let error = region_count_error(graph.declare_airframe_regions(
        region_slots[..3].to_vec(),
        Vec::new(),
        &vocabulary,
    ));
    assert!(
        error
            .to_string()
            .contains("airframe airframe/synthetic.devastator declares 3 damage-region slots")
            && error.to_string().contains("vocabulary holds 4"),
        "{error}"
    );
    let mut five = region_slots.clone();
    five.push(key("fifth_region"));
    let error = region_count_error(graph.declare_airframe_regions(five, Vec::new(), &vocabulary));
    assert!(
        error.to_string().contains("declares 5 damage-region slots"),
        "{error}"
    );
    let error = region_count_error(graph.declare_airframe_regions(
        region_slots,
        slot_keys("wreck", 2),
        &vocabulary,
    ));
    assert!(
        error
            .to_string()
            .contains("declares 2 wreck presentation slots"),
        "{error}"
    );
}

/// The reader refuses an installation whose container is not the one the
/// inventory fingerprinted, instead of measuring whatever bytes it found.
/// The refusal path is exercised with a root that inventories no such file,
/// which is the case a caller can produce without the original data.
#[test]
fn accept_f29_d1_observation_names_an_installation_without_the_container() {
    let empty = std::env::temp_dir().join(format!("cs-f29d1-empty-{}", std::process::id()));
    std::fs::create_dir_all(&empty).expect("a scratch directory is creatable");
    let error = cs_content::damage::observe_airframe_damage_vocabulary(&empty)
        .expect_err("an installation with no inventory rows cannot be measured");
    let message = match error {
        AirframeDamageObservationError::Discovery(_)
        | AirframeDamageObservationError::ContainerNotInventoried { .. } => error.to_string(),
        other => panic!("the reader must refuse before measuring, found: {other}"),
    };
    assert!(!message.is_empty());
    let _ = std::fs::remove_dir_all(&empty);
}
