//! Acceptance scenario M02-T2: a retail world group holds several campaign
//! missions, so a world id alone never identifies one.
//!
//! The synthetic test builds a campaign layout by hand and exercises
//! `missions_in_world_group`; the retail test (`#[ignore = "requires
//! CS_GAME_DIR"]`) reads the installation's own layout at run time and names
//! the missions of every shared group.

use std::collections::BTreeSet;
use std::path::PathBuf;

use cs_content::campaign_bindings::{CampaignMission, SourceContext, missions_in_world_group};
use cs_types::content::{ContentId, ContentKind};

fn mission(chapter: u32, number: u32, group: &str, dir: &str) -> CampaignMission {
    CampaignMission {
        chapter,
        mission_number: number,
        world_group: group.to_owned(),
        program_asset: format!("ZBD/{dir}/zrdr.zbd"),
        program_present: true,
    }
}

#[test]
fn accept_m02_t2_synthetic_world_group_lists_each_mission() {
    let campaign = vec![
        mission(2, 1, "c1", "C1/M02"),
        mission(2, 2, "c1", "C1/M04"),
        mission(2, 3, "c1c", "C1C/M01"),
    ];
    let world = ContentId::from_source(ContentKind::World, "c1").unwrap();
    let members = missions_in_world_group(&campaign, &world).unwrap();
    assert_eq!(members.len(), 2);
    assert_ne!(members[0].mission_id, members[1].mission_id);
    assert_ne!(members[0].program_id, members[1].program_id);

    let alone = ContentId::from_source(ContentKind::World, "c1c").unwrap();
    assert_eq!(missions_in_world_group(&campaign, &alone).unwrap().len(), 1);
    let none = ContentId::from_source(ContentKind::World, "zz").unwrap();
    assert!(
        missions_in_world_group(&campaign, &none)
            .unwrap()
            .is_empty()
    );
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m02_t2_retail_world_group_names_its_missions() {
    let dir = PathBuf::from(std::env::var("CS_GAME_DIR").expect(
        "CS_GAME_DIR is not set: M02-T2 needs the retail capability; run with --include-ignored",
    ));
    let context = SourceContext::read(&dir).expect("source context");
    let groups: BTreeSet<_> = context
        .campaign()
        .iter()
        .map(|entry| entry.world_group.clone())
        .collect();
    let mut shared = 0;
    for group in groups {
        let world = ContentId::from_source(ContentKind::World, &group).unwrap();
        let members = context.missions_in_world_group(&world).unwrap();
        let expected = context
            .campaign()
            .iter()
            .filter(|entry| entry.world_group == group)
            .count();
        assert_eq!(members.len(), expected, "group {group}");
        let ids: BTreeSet<_> = members.iter().map(|m| m.mission_id.clone()).collect();
        assert_eq!(ids.len(), members.len(), "mission ids in {group} collide");
        let programs: BTreeSet<_> = members.iter().map(|m| m.program_id.clone()).collect();
        assert_eq!(
            programs.len(),
            members.len(),
            "program ids in {group} collide"
        );
        if members.len() > 1 {
            shared += 1;
            let names: Vec<_> = members.iter().map(|m| m.mission_id.as_str()).collect();
            eprintln!("world/{group} holds {names:?}");
        }
    }
    assert!(shared > 0, "no retail world group holds several missions");
}
