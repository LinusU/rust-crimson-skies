//! #792: the composed attitude of M01's three `zeppelins.zrd` records, from
//! the source the original applies last.
//!
//! The retail case reads `ZBD/C1C/M01/zrdr.zbd` through production discovery,
//! decodes *both* carriers independently, and asserts that the declared
//! actor's `orientation` is the composed attitude of whichever carrier's
//! write survives — the `placezeps.zrd` startup state for the two nodes it
//! rotates, the record's own spawn attitude for the node it does not — with
//! the losing source still named as the residue. The compose is recomputed
//! here from the rotation matrices (#770's `M = Ry·Rx·Rz`) rather than
//! called, so a wrong product in the implementation fails this test instead
//! of agreeing with it.

use cs_app::mission_start::stored_heading_radians;
use cs_app::mission_world_actors::{
    AttitudeSource, SPAWN_ATTITUDE_CLAIM, bind_mission_world_actors,
};
use cs_assets::install;
use cs_content::world_actors::DeclaredMotion;
use cs_formats::script_raw::discovery::discover_container;
use cs_formats::zbd::placezeps::{PLACEZEPS_MEMBER, read_placezeps_member};
use cs_formats::zbd::zeppelins::{ZEPPELINS_MEMBER, read_zeppelins_member};
use cs_types::content::{ContentId, ContentKind, Resolved};

const ARCHIVE: &str = "zbd/c1c/m01/zrdr.zbd";

/// The right-handed rotation matrix about one axis, in #770's convention.
fn rot_x(a: f64) -> [[f64; 3]; 3] {
    let (s, c) = a.sin_cos();
    [[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]]
}

fn rot_y(a: f64) -> [[f64; 3]; 3] {
    let (s, c) = a.sin_cos();
    [[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]]
}

fn rot_z(a: f64) -> [[f64; 3]; 3] {
    let (s, c) = a.sin_cos();
    [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]]
}

fn mul(a: [[f64; 3]; 3], b: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let mut out = [[0.0; 3]; 3];
    for (row, line) in out.iter_mut().enumerate() {
        for (col, cell) in line.iter_mut().enumerate() {
            *cell = (0..3).map(|k| a[row][k] * b[k][col]).sum();
        }
    }
    out
}

/// The measured compose as a matrix — `M = Ry(r1)·Rx(r0)·Rz(r2)` (#770
/// §12.2, the build `0x53bf40` performs and `0x53df30` inverts).
fn compose_matrix(r0: f64, r1: f64, r2: f64) -> [[f64; 3]; 3] {
    mul(mul(rot_y(r1), rot_x(r0)), rot_z(r2))
}

/// The unit quaternion `[x, y, z, w]` of a rotation matrix, derived from the
/// matrix itself (the trace/branch form), so the test's own number path does
/// not reuse the implementation's quaternion product.
fn quat_of(m: [[f64; 3]; 3]) -> [f64; 4] {
    let trace = m[0][0] + m[1][1] + m[2][2];
    let q = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        [
            (m[2][1] - m[1][2]) / s,
            (m[0][2] - m[2][0]) / s,
            (m[1][0] - m[0][1]) / s,
            0.25 * s,
        ]
    } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
        let s = (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt() * 2.0;
        [
            0.25 * s,
            (m[0][1] + m[1][0]) / s,
            (m[0][2] + m[2][0]) / s,
            (m[2][1] - m[1][2]) / s,
        ]
    } else if m[1][1] > m[2][2] {
        let s = (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt() * 2.0;
        [
            (m[0][1] + m[1][0]) / s,
            0.25 * s,
            (m[1][2] + m[2][1]) / s,
            (m[0][2] - m[2][0]) / s,
        ]
    } else {
        let s = (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt() * 2.0;
        [
            (m[0][2] + m[2][0]) / s,
            (m[1][2] + m[2][1]) / s,
            0.25 * s,
            (m[1][0] - m[0][1]) / s,
        ]
    };
    let norm = q.iter().map(|v| v * v).sum::<f64>().sqrt();
    q.map(|v| v / norm)
}

/// The same rotation, up to the sign a matrix form and an axis product may
/// legitimately disagree on: the sign is aligned first, then every component
/// is compared, so a *different* rotation still fails.
fn close(a: [f64; 4], b: [f64; 4], what: &str) {
    let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    assert!(
        (dot.abs() - 1.0).abs() < 1e-9,
        "{what}: {a:?} and {b:?} are {dot} apart, not the same rotation"
    );
    let sign = if dot < 0.0 { -1.0 } else { 1.0 };
    for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
        assert!(
            (x - sign * y).abs() < 1e-9,
            "{what}: component {i} is {x}, expected {y}"
        );
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_zeppelin_attitude_retail_records_compose_from_the_last_written_source() {
    let root = std::path::PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("a retail test needs CS_GAME_DIR to be set"),
    );
    let found = install::discover(&root).expect("the installation is discoverable");
    let record = found
        .manifest
        .files
        .iter()
        .find(|record| record.relative_spelling.logical_key() == ARCHIVE)
        .expect("M01's reader archive is installed");
    let bytes = std::fs::read(root.join(record.relative_spelling.as_str())).expect("readable");
    let discovery = discover_container(ARCHIVE, &record.relative_spelling, &bytes);
    let member = |name: &str| {
        discovery
            .programs()
            .iter()
            .find(|p| {
                p.locator()
                    .member()
                    .is_some_and(|m| m.eq_ignore_ascii_case(name))
            })
            .unwrap_or_else(|| panic!("{name} is in the archive"))
    };

    // Both carriers, decoded independently of the program binding.
    let carrier = read_zeppelins_member(member(ZEPPELINS_MEMBER).bytes()).expect("decodes");
    let startup = read_placezeps_member(member(PLACEZEPS_MEMBER).bytes()).expect("decodes");
    let startup_rotations: Vec<(&str, [f32; 3])> = startup
        .definitions()
        .iter()
        .filter_map(|definition| definition.sequence().rotate())
        .map(|statement| (statement.node(), statement.parsed()))
        .collect();
    assert_eq!(
        carrier.records().len(),
        3,
        "M01 places three zeppelins, two of which the startup carrier rotates"
    );
    let mut applied: Vec<(&str, [f64; 4])> = Vec::new();

    // The production binding, over the retail installation.
    let subject = ContentId::from_source(ContentKind::Mission, "ch1-m01").expect("id");
    let bound = bind_mission_world_actors(&root, &found, "zbd/c1c/m01", "zbd/c1c", &subject, 64);
    assert_eq!(bound.rows().len(), 3);
    let program = bound
        .program()
        .expect("all three records join their world nodes, so a program assembles");

    // The attitude is measured, so it never appears in the launch surface's
    // open-field list any more; the faction stays open under its own claim.
    assert!(
        bound
            .open_fields()
            .iter()
            .all(|open| open.field != "orientation"),
        "the composed attitude is not an open field: {:?}",
        bound.open_fields()
    );

    for record in carrier.records() {
        let row = bound
            .rows()
            .iter()
            .find(|row| row.node == record.node())
            .unwrap_or_else(|| panic!("{} joined a row", record.node()));

        // The source the original applies last, recomputed from the members.
        let (source_name, expected) = match startup_rotations
            .iter()
            .find(|(node, _)| *node == record.node())
        {
            Some((_, rotation_radians)) => {
                let [r0, r1, r2] = rotation_radians.map(f64::from);
                (
                    "placezeps.zrd startup state",
                    quat_of(compose_matrix(r0, r1, r2)),
                )
            }
            None => {
                // The spawn path hands `SetRotation` the pitch, the yaw and a
                // clear roll slot (`0x4bf950`), the pitch clamped to the
                // record's own limits the way `0x4bdbc6` clamps it.
                let min = f64::from(stored_heading_radians(record.min_pitch()));
                let max = f64::from(stored_heading_radians(record.max_pitch()));
                let pitch = f64::from(stored_heading_radians(record.pitch()));
                let yaw = f64::from(stored_heading_radians(record.yaw()));
                (
                    "zeppelins.zrd spawn attitude",
                    quat_of(compose_matrix(pitch.clamp(min, max), yaw, 0.0)),
                )
            }
        };

        let source = row
            .attitude_source()
            .unwrap_or_else(|| panic!("{}'s attitude is settled", record.node()));
        assert!(
            matches!(
                (source, source_name),
                (
                    AttitudeSource::StartupPlacement { .. },
                    "placezeps.zrd startup state"
                ) | (
                    AttitudeSource::CarrierSpawn { .. },
                    "zeppelins.zrd spawn attitude"
                )
            ),
            "{} takes the source the original applies last: {source:?}",
            record.node()
        );

        let actor = program
            .actors()
            .iter()
            .find(|actor| actor.actor.0 == u32::try_from(row.index).expect("index fits"))
            .unwrap_or_else(|| panic!("{} declared an actor", record.node()));
        let DeclaredMotion::Held { orientation, .. } = &actor.motion else {
            panic!("{} declares a held pose", record.node());
        };
        let Resolved::Known(attitude) = orientation else {
            panic!(
                "{}'s attitude binds measured, not {:?}",
                record.node(),
                orientation
            );
        };
        assert_eq!(
            attitude.provenance.claim_id.as_str(),
            SPAWN_ATTITUDE_CLAIM,
            "{} binds the compose claim",
            record.node()
        );
        assert_eq!(
            attitude.provenance.class,
            cs_types::evidence::ClaimStatus::ObservedTool,
            "the attitude is observed_tool, never stronger"
        );
        close(
            attitude.value,
            expected,
            &format!("{} from {source_name}", record.node()),
        );
        applied.push((record.node(), attitude.value));

        // The losing source is named, never dropped.
        let residue = row.attitude_residue();
        match source_name {
            "placezeps.zrd startup state" => assert!(
                residue.contains("zeppelins.zrd") && residue.contains("spawn attitude"),
                "{}'s residue names the spawn attitude it overwrites: {residue}",
                record.node()
            ),
            _ => assert!(
                residue.contains("placezeps.zrd") && residue.contains("OBJECT_ROTATE_STATE"),
                "{}'s residue names the startup carrier's silence: {residue}",
                record.node()
            ),
        }
        eprintln!(
            "{}: source={source_name} orientation={:?} residue={residue}",
            record.node(),
            attitude.value
        );
    }

    // The three composed values, spelled out: `piratezep` and
    // `workersvoyagezep` end on the startup state's `Ry(180°)` (its `STATE`
    // is (0, 180, 0)°, #791) and `blackswanzep` — which the startup carrier
    // does not rotate — keeps the record's own `Ry(340°)`. A bug that moved
    // the decode and the compose together still fails here.
    for (node, want) in [
        (
            "piratezep",
            [0.0, 0.999_999_999_999_999, 0.0, -4.371_139_000_186_241e-8],
        ),
        (
            "workersvoyagezep",
            [0.0, 0.999_999_999_999_999, 0.0, -4.371_139_000_186_241e-8],
        ),
        (
            "blackswanzep",
            [0.0, 0.173_648_292_019_053_68, 0.0, -0.984_807_732_848_836_6],
        ),
    ] {
        let (_, got) = applied
            .iter()
            .find(|(name, _)| *name == node)
            .unwrap_or_else(|| panic!("{node} composed an orientation"));
        close(*got, want, &format!("{node}'s measured attitude"));
    }
}
