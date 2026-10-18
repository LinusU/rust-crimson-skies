//! M18-B-FU2 (#1255): which production consumer joins M18's `WAKE_ANIM` and
//! `ANIM_STATE` operands to the record that defines them.
//!
//! Block 25 of `ZBD/C4/M03/zrdr.zbd :: objectives.zrd` spells `pzhomebase` as
//! its `WAKE_ANIM` and `hooked_to_klondike` as its `ANIM_STATE`. Neither is
//! declared by another member of M18's own reader archive; the answer is in
//! `docs/findings/2026-10-18-m18-b-fu2-control-record-animation-names.md`.

use std::path::PathBuf;

use cs_app::animation::mission::bind_mission_animation;
use cs_app::mission_session::read_landings;

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!("CS_GAME_DIR is not set: M18-B-FU2 needs the retail capability")
    }))
}

/// Both names resolve through the production name join to the shared root's
/// `pzep_hookup.zrd` and to one `mis_anim.zbd` record each; the landing table
/// of M18's world group runs `hooked_to_klondike` at the pirate zeppelin's
/// landing nodes, which is the consumer the `ANIM_STATE` gate's record has.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m18_b_fu2_control_record_names_resolve_through_the_name_join_and_the_landing_table() {
    let dir = game_dir();
    let binding = bind_mission_animation(&dir, "zbd/c4/m03").expect("M18's scope binds");

    for (name, index) in [("pzhomebase", 214), ("hooked_to_klondike", 217)] {
        let resolution = binding.animation_by_name(name);
        let row = resolution
            .bound()
            .unwrap_or_else(|| panic!("{name} does not bind: {:?}", resolution.unbound()));
        assert!(row.is_playable(), "{name}: {:?}", row.refusals());
        let site = row
            .declaration()
            .resolution()
            .single()
            .unwrap_or_else(|| panic!("{name} has no single declaration"));
        assert_eq!(
            (site.archive(), site.member()),
            ("zbd/zrdr.zbd", "pzep_hookup.zrd"),
            "{name} is declared by the shared root, not by M18's own archive"
        );
        let record = row.bound_record().expect("a bound row has its record");
        assert_eq!(
            (record.carrier_key.as_str(), record.index),
            ("zbd/c4/m03/mis_anim.zbd", index),
            "{name}'s carrier record"
        );
    }

    // The `ANIM_STATE` name is also what M18's world-group landing table runs.
    let landings = read_landings(&dir, "zbd/c4").expect("the c4 landing table reads");
    assert!(landings.refusals.is_empty(), "{:?}", landings.refusals);
    let klondike: Vec<(&str, bool)> = landings
        .rows
        .iter()
        .filter(|row| row.anim == "hooked_to_klondike")
        .map(|row| (row.node.as_str(), row.auto))
        .collect();
    assert_eq!(
        klondike,
        [("pz_manual_land", false), ("pz_auto_land", true)]
    );
    assert!(
        landings.rows.iter().all(|row| row.anim != "pzhomebase"),
        "no landing row runs the WAKE_ANIM name"
    );
}
