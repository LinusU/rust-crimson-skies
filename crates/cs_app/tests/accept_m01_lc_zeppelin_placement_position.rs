//! #814: the applied position of M01's three `zeppelins.zrd` records, from
//! the source the original applies last.
//!
//! The retail case reads `ZBD/C1C/M01/zrdr.zbd` through production
//! discovery, decodes *both* carriers independently, and asserts that the
//! declared actor's `position_m` is the position of whichever carrier's
//! write survives — `placezeps.zrd`'s `OBJECT_TRANSLATE_STATE` triple for
//! all three nodes, provenanced from the statement's own `STATE` span —
//! with the record's own spawn `position` still named as the residue. The
//! expected values are re-decoded from the members here, never copied from
//! the implementation, so a binding that kept the carrier's spawn value
//! (z −11 985, −9 120, 4 767) fails where the startup placement states
//! −8 704, −7 680 and −5 632.

use cs_app::mission_world_actors::{PositionSource, SPAWN_POSE_CLAIM, bind_mission_world_actors};
use cs_assets::install;
use cs_content::world_actors::DeclaredMotion;
use cs_formats::script_raw::discovery::discover_container;
use cs_formats::zbd::placezeps::{PLACEZEPS_MEMBER, read_placezeps_member};
use cs_formats::zbd::zeppelins::{ZEPPELINS_MEMBER, read_zeppelins_member};
use cs_types::content::{ContentId, ContentKind, Resolved};

const ARCHIVE: &str = "zbd/c1c/m01/zrdr.zbd";

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_zeppelin_placement_position_retail_records_bind_the_last_written_source() {
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

    // Both carriers, decoded independently of the program binding. The
    // startup translates keep their `STATE` ranges so the provenance check
    // is against the statement's own bytes, not the member's whole span.
    let carrier = read_zeppelins_member(member(ZEPPELINS_MEMBER).bytes()).expect("decodes");
    let placezeps = member(PLACEZEPS_MEMBER);
    let member_offset = placezeps.locator().span().offset;
    let startup = read_placezeps_member(placezeps.bytes()).expect("decodes");
    // The premise the binding is checked under: only an `ON_STARTUP`
    // definition's statements write at mission start (activation byte
    // `+0xa1 == 4`, #792 §2) — any other spelling would unsettle the
    // ordering instead of feeding it.
    for definition in startup.definitions() {
        assert_eq!(
            definition.activation(),
            "ON_STARTUP",
            "definition {} runs at startup",
            definition.index()
        );
    }
    let startup_translates: Vec<(&str, [f32; 3], u64, u64)> = startup
        .definitions()
        .iter()
        .filter_map(|definition| definition.sequence().translate())
        .map(|statement| {
            (
                statement.node(),
                statement.parsed(),
                member_offset + statement.state_range().start,
                statement.state_range().end - statement.state_range().start,
            )
        })
        .collect();
    assert_eq!(
        carrier.records().len(),
        3,
        "M01 places three zeppelins, all of which the startup carrier translates"
    );

    // The production binding, over the retail installation.
    let subject = ContentId::from_source(ContentKind::Mission, "ch1-m01").expect("id");
    let bound = bind_mission_world_actors(&root, &found, "zbd/c1c/m01", "zbd/c1c", &subject, 64);
    assert_eq!(bound.rows().len(), 3);
    let program = bound
        .program()
        .expect("all three records join their world nodes, so a program assembles");

    // The position is measured, so it never appears in the launch surface's
    // open-field list; the faction stays open under its own claim.
    assert!(
        bound
            .open_fields()
            .iter()
            .all(|open| open.field != "position_m"),
        "the applied position is not an open field: {:?}",
        bound.open_fields()
    );

    let mut applied: Vec<(&str, [f64; 3])> = Vec::new();
    for record in carrier.records() {
        let row = bound
            .rows()
            .iter()
            .find(|row| row.node == record.node())
            .unwrap_or_else(|| panic!("{} joined a row", record.node()));

        // The source the original applies last, recomputed from the
        // members: the startup translate's absolute triple where the member
        // states one for this node, the record's own `position` where it
        // does not.
        let (source_name, expected, expected_span) = match startup_translates
            .iter()
            .find(|(node, ..)| *node == record.node())
        {
            Some((_, position_m, offset, length)) => (
                "placezeps.zrd startup state",
                position_m.map(f64::from),
                Some((*offset, *length)),
            ),
            None => (
                "zeppelins.zrd spawn position",
                record.position().map(f64::from),
                None,
            ),
        };

        let source = row
            .position_source()
            .unwrap_or_else(|| panic!("{}'s position is settled", record.node()));
        match (source, source_name) {
            (
                PositionSource::StartupPlacement {
                    position_m,
                    state_span,
                },
                "placezeps.zrd startup state",
            ) => {
                assert_eq!(
                    position_m.map(f64::from),
                    expected,
                    "{} takes the startup translate's absolute triple",
                    record.node()
                );
                let (offset, length) =
                    expected_span.expect("a startup source names its STATE span");
                assert_eq!(
                    state_span.offset(),
                    offset,
                    "{}: STATE span offset",
                    record.node()
                );
                assert_eq!(
                    state_span.length(),
                    length,
                    "{}: STATE span length",
                    record.node()
                );
                assert_eq!(
                    state_span.member_key(),
                    Some(PLACEZEPS_MEMBER),
                    "{}: the span names the startup member",
                    record.node()
                );
            }
            (PositionSource::CarrierSpawn { position_m }, "zeppelins.zrd spawn position") => {
                assert_eq!(
                    position_m.map(f64::from),
                    expected,
                    "{} keeps the record's own spawn position",
                    record.node()
                );
            }
            (source, _) => panic!(
                "{} takes the source the original applies last: {source:?}",
                record.node()
            ),
        }

        let actor = program
            .actors()
            .iter()
            .find(|actor| actor.actor.0 == u32::try_from(row.index).expect("index fits"))
            .unwrap_or_else(|| panic!("{} declared an actor", record.node()));
        let DeclaredMotion::Held { position_m, .. } = &actor.motion else {
            panic!("{} declares a held pose", record.node());
        };
        let Resolved::Known(applied_position) = position_m else {
            panic!(
                "{}'s position binds measured, not {:?}",
                record.node(),
                position_m
            );
        };
        assert_eq!(
            applied_position.provenance.claim_id.as_str(),
            SPAWN_POSE_CLAIM,
            "{} binds the spawn-pose claim",
            record.node()
        );
        assert_eq!(
            applied_position.provenance.class,
            cs_types::evidence::ClaimStatus::ObservedTool,
            "the position is observed_tool, never stronger"
        );
        assert_eq!(
            applied_position.value,
            expected,
            "{} from {source_name}",
            record.node()
        );
        if let Some((offset, length)) = expected_span {
            let span = applied_position
                .provenance
                .source
                .as_ref()
                .expect("a bound position carries its source span");
            assert_eq!(
                (span.offset(), span.length()),
                (offset, length),
                "{} is provenanced from the STATE list's own bytes",
                record.node()
            );
        }
        applied.push((record.node(), applied_position.value));

        // The losing source is named, never dropped.
        let residue = row.position_residue();
        match source_name {
            "placezeps.zrd startup state" => assert!(
                residue.contains("zeppelins.zrd") && residue.contains("spawn position"),
                "{}'s residue names the spawn position it overwrites: {residue}",
                record.node()
            ),
            _ => assert!(
                residue.contains("placezeps.zrd") && residue.contains("OBJECT_TRANSLATE_STATE"),
                "{}'s residue names the startup carrier's silence: {residue}",
                record.node()
            ),
        }
        eprintln!(
            "{}: source={source_name} position_m={:?} residue={residue}",
            record.node(),
            applied_position.value
        );
    }

    // The three applied positions, spelled out: all three nodes end on the
    // startup carrier's absolute translate (#791's measured table), not on
    // the record's spawn `position` — a binding that kept the carrier's
    // value (−11 985.3, −9 120, 4 767.6) fails here.
    for (node, want) in [
        ("piratezep", [-3584.0, 1360.0, -8704.0]),
        ("workersvoyagezep", [-5972.0, 1460.0, -7680.0]),
        ("blackswanzep", [-6656.0, 1960.0, -5632.0]),
    ] {
        let (_, got) = applied
            .iter()
            .find(|(name, _)| *name == node)
            .unwrap_or_else(|| panic!("{node} applied a position"));
        assert_eq!(*got, want, "{node}'s measured position");
    }
}
