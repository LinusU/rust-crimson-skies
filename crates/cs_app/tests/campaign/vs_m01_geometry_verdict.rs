//! The `world_geometry` launch verdict, read off #771's residual counters
//! (`VS-M01-GEOMETRY-VERDICT-ON-MAIN`).
//!
//! `geometry_verdict` asks one question of the import's own
//! [`WorldImportReport`](cs_content::world::WorldImportReport): did the
//! container leave a record whose collision behaviour this conversion had to
//! leave open? For a long while the verdict read
//! [`partition_records_fog_volume`](cs_content::world::WorldImportReport::partition_records_fog_volume)
//! as that open question — but that accessor is an **overlap** count (how many
//! grid-named records the original's fog consumer keys, #716/#727/#771), and
//! every record it covers resolves role `None`, so M01's plan could not read
//! `11/11` off `main` however measured the container was. The measurement is
//! `docs/findings/2026-10-08-m01-lc-world-residual-roles.md`.
//!
//! Two members, both driving the **production** verdict:
//!
//! * `accept_vs_m01_geometry_verdict_...zero_residual...` is synthetic: it
//!   imports one fixture container twice through the production
//!   `import_world_container` — once where the only records the container
//!   might have been silent about (a grid-named `fvol*` and a grid record that
//!   stores no geometry) are exactly the ones #771 answered, once with the
//!   unindexed geometry-bearing record nothing answers — and asserts
//!   `Satisfied` against `Unknown`. It runs in CI, which has no original data.
//! * the retail member (`#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]`)
//!   re-plans M01 against the owner's installation and asserts the plan
//!   reports **zero** gaps and `launchable()` is true.
//!
//! The synthetic member is written so that it fails if the verdict is put back
//! on the overlap counter or on the un-subtracted mesh-less index: the fixture
//! deliberately carries a `partition_records_fog_volume` of 1 beside an
//! `objects_unresolved_collision` of 0, which is the retail `c1c` shape.
//!
//! Nothing here is `verified_original`: the fixture is authored bytes, and the
//! retail half reads the installation through production code.

#[path = "../world/import_retail.rs"]
mod world_import;

use std::path::PathBuf;

use cs_app::mission_launch::{
    LaunchSurface, SurfaceVerdict, geometry_verdict, plan_mission_launch,
};
use cs_content::coordinates::{CoordinateSource, SourceAdapter};
use cs_content::world::{
    GRID_NAMED_FOG_VOLUME_ROLE_UNMEASURED, UNINDEXED_ROLE_UNMEASURED,
    WORLD_AXIS_CONVENTION_MEASURED, WorldId, WorldImportReport, import_world_container,
};
use cs_types::asset_id::SourceSpan;
use cs_types::content::Origin;
use cs_types::evidence::ContentHash;

use self::world_import::{
    Fixture, ObjectSpec, VOLUME, mesh_slots, provenance, read, write_container,
};
use crate::common::{label, load_inventory};

/// The mesh slot the fixture's grid-named `fvol*` record binds — the fixture
/// table holds `10` slots (`0..10`).
const MESH_FOG: i32 = 9;

/// A span that stands for "the container the measurement ran over" in a
/// non-retail test: the shape is exercised, the bytes are not claimed. It is
/// the same stand-in the axis-convention member of `tests/world/fvol_roles.rs`
/// uses, and it is a fixture key — no installation is read here.
fn span() -> SourceSpan {
    SourceSpan::new(
        ContentHash::from_bytes([0; 32]),
        "zbd/c1c/gamez.zbd",
        None,
        0,
        0,
        None,
    )
    .expect("a span for an installation-backed source is valid")
}

/// The conversion the fixture is imported through: the measured GameZ source
/// (#677's scale landmark, #436's axis landmarks), which is what makes
/// `axis_class()` report `ObservedTool` for the identity map the fixture
/// writes — the same arm a retail container goes through.
fn adapter() -> SourceAdapter {
    SourceAdapter::new(CoordinateSource::retail_gamez(span()))
}

/// One fixture container driven through the production
/// `import_world_container`, reported.
fn report(fixture: &Fixture) -> WorldImportReport {
    let bytes = write_container(fixture);
    let imported = import_world_container(
        WorldId::from_key("fixture").expect("the fixture world key is valid"),
        Origin::SyntheticFixture,
        &read(&bytes),
        &bytes,
        &mesh_slots(),
        &adapter(),
        provenance(),
    )
    .expect("the fixture container imports");
    imported.into_parts().1
}

/// The two records the container *could* have been silent about, added to the
/// fixture's partition grid, both answered:
///
/// * a grid-named `fvol*` whose stored narrow-phase bit is clear, so the
///   intersection walk drops it before any box test — resolved `None`
///   (#771), while still counting toward the **overlap**
///   `partition_records_fog_volume` reports;
/// * a grid record that stores no mesh index and no box — resolved `None`
///   because the store gives a collider nothing to come from (#771), and
///   counted by `partition_records_stores_no_geometry`.
fn with_answered_grid_records(fixture: &mut Fixture) {
    let fog = 1 + u32::try_from(fixture.objects.len()).expect("a fixture slot fits");
    // The stored node flag word the installation's grid-named `fvol*` records
    // hold (`0x0308831c`): its narrow-phase bit `0x40` is clear, which is
    // exactly the class `cls_di.c`'s walk drops before any box test.
    fixture.objects.push(
        ObjectSpec::new("fvol_edge", MESH_FOG)
            .extent([-6.0, 0.0, -6.0], [-5.0, 1.0, -5.0])
            .flags(0x0308_831c),
    );
    let empty = 1 + u32::try_from(fixture.objects.len()).expect("a fixture slot fits");
    fixture.objects.push(ObjectSpec::new("empty", -1));
    fixture.grid[0].extend([fog, empty]);
}

/// The retail shape: no record is left without an answer. The default
/// fixture's unindexed geometry-bearing `volume` — the one class the
/// container states nothing about — is removed, and the two answered records
/// are added, so the report's fog overlap is `1` while its residual is `0`.
fn zero_residual() -> Fixture {
    let mut fixture = Fixture::default();
    fixture.objects.retain(|object| object.name != "volume");
    fixture.stored_children.retain(|slot| *slot != VOLUME);
    with_answered_grid_records(&mut fixture);
    fixture
}

/// The same container **with** the unindexed geometry-bearing record the
/// default fixture authors: exactly one record whose collision role nothing in
/// the container states.
fn nonzero_residual() -> Fixture {
    let mut fixture = Fixture::default();
    with_answered_grid_records(&mut fixture);
    fixture
}

/// **A report with no residual record is `Satisfied`; a report with one is
/// `Unknown` and names the counter it is read off.**
///
/// The zero half is the discriminating one: its
/// `partition_records_fog_volume` is `1`, so a verdict that still reads that
/// overlap as an open question reports `Unknown` and this test fails. The
/// nonzero half pins the other direction — the single unindexed record is
/// named by `UNINDEXED_ROLE_UNMEASURED`, while the answered overlap and the
/// answered mesh-less half are never named as open.
#[test]
fn accept_vs_m01_geometry_verdict_a_zero_residual_report_is_satisfied_and_a_residual_is_named() {
    // (a) Nothing is open, and the report says so in counters rather than in
    // this test's own restatement of them.
    let answered = report(&zero_residual());
    assert_eq!(
        answered.objects_unresolved_collision(),
        0,
        "no record's collision role is left Unknown"
    );
    assert_eq!(
        answered.objects_unindexed_unresolved(),
        0,
        "and no unindexed record is among them"
    );
    assert_eq!(
        answered.partition_records_fog_volume(),
        1,
        "the overlap the old reading mistook for a gap is still counted: one \
         grid-named `fvol*` record"
    );
    assert_eq!(
        answered.partition_records_stores_no_geometry(),
        1,
        "and the grid record that stores no geometry is counted as answered"
    );
    let verdict = geometry_verdict(&answered);
    let SurfaceVerdict::Satisfied { consumer } = &verdict else {
        panic!(
            "the overlap and the store's own silence are answers, not gaps: {}",
            verdict.describe()
        );
    };
    assert!(
        consumer.contains("objects with an `Unknown` collision role: 0"),
        "the zero residual is stated, not omitted: {consumer}"
    );
    assert!(
        consumer.contains(WORLD_AXIS_CONVENTION_MEASURED),
        "the axis claim the verdict reads is cited: {consumer}"
    );

    // (b) One record is open, and the verdict names the counter it is read
    // off — never the overlap, never the answered half.
    let residual = report(&nonzero_residual());
    assert_eq!(
        residual.objects_unresolved_collision(),
        1,
        "the unindexed geometry-bearing record is the whole residual"
    );
    assert_eq!(
        residual.partition_records_fog_volume(),
        1,
        "the answered overlap is present here too, so the two are told apart"
    );
    let verdict = geometry_verdict(&residual);
    let SurfaceVerdict::Unknown { detail } = &verdict else {
        panic!(
            "a record no stage answered keeps the surface open: {}",
            verdict.describe()
        );
    };
    assert!(
        detail.contains(UNINDEXED_ROLE_UNMEASURED),
        "the residual counter's claim id is named: {detail}"
    );
    assert!(
        detail.contains("1 unindexed geometry-bearing records"),
        "and the count is spelled out: {detail}"
    );
    assert!(
        !detail.contains(GRID_NAMED_FOG_VOLUME_ROLE_UNMEASURED),
        "the answered fog overlap is never named as open: {detail}"
    );
    assert!(
        !detail.contains("no mesh index drew"),
        "the answered mesh-less half is never named as open: {detail}"
    );
}

/// The original installation, as the environment declares it.
fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: VS-M01-GEOMETRY-VERDICT-ON-MAIN needs the retail \
             capability; run this suite with `--include-ignored` and CS_GAME_DIR pointing \
             at the read-only installation"
        )
    }))
}

/// The declared discovery title of `M01`, read from the committed inventory
/// rather than repeated here.
fn discovery_title() -> String {
    load_inventory()
        .iter()
        .find(|(label, _)| label.as_str() == "M01")
        .map(|(_, title)| title.clone())
        .expect("the declared inventory has an M01 work order")
}

/// **Retail: M01's plan reports zero gaps and `launchable()` is true.**
///
/// This is the same read the CLI makes — `cs --cs-path "$CS_GAME_DIR" --mission
/// M01` — through [`plan_mission_launch`]: every one of the eleven surfaces
/// is reported, the `world_geometry` one reads `Satisfied` off the retail
/// container's residual counters, and the gate therefore names no gap.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_vs_m01_geometry_verdict_retail_m01_s_plan_reports_zero_gaps_and_is_launchable() {
    let plan = plan_mission_launch(&game_dir(), label("M01"), &discovery_title())
        .expect("the installation yields a launch plan for M01");

    let geometry = plan
        .surface(LaunchSurface::WorldGeometry)
        .expect("the surface is reported");
    let SurfaceVerdict::Satisfied { consumer } = &geometry.verdict else {
        panic!(
            "the retail container leaves no record without a measured answer: {}",
            geometry.verdict.describe()
        );
    };
    assert!(
        consumer.contains("objects with an `Unknown` collision role: 0"),
        "the retail residual is zero and stated: {consumer}"
    );

    let gaps: Vec<&str> = plan.gaps().map(|report| report.surface.label()).collect();
    assert!(
        gaps.is_empty(),
        "the plan reports zero gaps — every surface is satisfied, so the gate names \
         nothing to measure: {gaps:?}"
    );
    assert!(
        plan.launchable(),
        "every surface satisfied means the gate passes: {:?}",
        plan.surfaces
            .iter()
            .map(|report| format!("{}: {}", report.surface.label(), report.verdict.describe()))
            .collect::<Vec<_>>()
    );
}
