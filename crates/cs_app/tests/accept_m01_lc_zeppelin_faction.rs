//! #793: the measured faction verdict of M01's three `zeppelins.zrd`
//! records, as #1155 re-measured it.
//!
//! The retail case reads `ZBD/C1C/M01/zrdr.zbd` through production discovery
//! and asserts that no record states a `team`, that the mission's `net.zrd`
//! holds no text (so no net→faction table) — and that the declared actors
//! take their faction from #1155's measured allegiance resolver instead:
//! every faction is `Resolved::Known` under `f34-world.zeppelin-allegiance`,
//! never the retired `f34-world.zeppelin-faction-absent` verdict.

use cs_app::mission_world_actors::{
    ALLEGIANCE_RESOLVED_CLAIM, ZEPPELIN_MEMBER, bind_mission_world_actors,
};
use cs_assets::install;
use cs_formats::script_raw::discovery::discover_container;
use cs_formats::zbd::zeppelins::{ZeppelinKey, read_zeppelins_member};
use cs_types::content::{ContentId, ContentKind, Resolved};

const ARCHIVE: &str = "zbd/c1c/m01/zrdr.zbd";

/// Walks one `.zrd` node (F09's grammar: tag, then payload; a list word `N`
/// holds `N - 1` children) and reports whether any string node occurs.
fn holds_text(bytes: &[u8], at: &mut usize) -> bool {
    let word = |at: &mut usize| {
        let v = u32::from_le_bytes(bytes[*at..*at + 4].try_into().unwrap());
        *at += 4;
        v
    };
    match word(at) {
        1 | 2 => {
            *at += 4;
            false
        }
        3 => {
            let len = word(at) as usize;
            *at += len;
            true
        }
        4 => {
            let n = word(at);
            let mut text = false;
            for _ in 1..n {
                text |= holds_text(bytes, at);
            }
            text
        }
        tag => panic!("undefined tag {tag}"),
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_zeppelin_faction_records_state_no_team_and_the_binding_binds_a_faction() {
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

    // Independent of the program binding: the decoded records state no team.
    let decoded = read_zeppelins_member(member(ZEPPELIN_MEMBER).bytes()).expect("decodes");
    assert_eq!(decoded.len(), 3, "M01 places three zeppelins");
    for record in decoded.records() {
        assert!(
            !record.keys().contains(&ZeppelinKey::Team) && record.team().is_none(),
            "{} states no team",
            record.node()
        );
    }

    // The mission's net.zrd is numbers only: no spelling for a faction table.
    let net = member("net.zrd").bytes();
    let mut at = 0;
    assert!(!holds_text(net, &mut at), "net.zrd holds no text node");
    assert_eq!(at, net.len(), "net.zrd decodes with no byte left over");

    // The production binding binds every declared actor's faction from the
    // measured allegiance resolver (#1155): no faction stays open, and each
    // one is filed under the allegiance claim rather than the retired
    // measured-absent verdict.
    let subject = ContentId::from_source(ContentKind::Mission, "ch1-m01").expect("id");
    let bound = bind_mission_world_actors(&root, &found, "zbd/c1c/m01", "zbd/c1c", &subject, 64);
    assert_eq!(bound.rows().len(), 3);
    assert!(bound.rows().iter().all(|row| row.team.is_none()));
    assert!(
        bound
            .open_fields()
            .iter()
            .all(|field| field.field != "faction"),
        "no record's faction stays open: {:?}",
        bound.open_fields()
    );
    let program = bound.program().expect("the records assemble a program");
    assert_eq!(program.actors().len(), 3);
    for actor in program.actors() {
        let Resolved::Known(known) = &actor.faction else {
            panic!(
                "{} binds a measured faction: {:?}",
                actor.subject, actor.faction
            );
        };
        assert_eq!(
            known.provenance.claim_id.as_str(),
            ALLEGIANCE_RESOLVED_CLAIM,
            "the faction is filed under the measured allegiance claim"
        );
    }
}
