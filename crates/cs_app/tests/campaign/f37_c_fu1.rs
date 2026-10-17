//! F37-C-FU1: the mission host seeds the control evaluator from the run's
//! mission seed and resolves its reward catalog from declared campaign content.
//!
//! Synthetic stage only; the seed recipe and the catalog are engine designs.

use cs_app::campaign::mission_reward_catalog;
use cs_app::environment::{EnvironmentSession, RunSeeds};
use cs_app::mission_session::{
    MissionContent, MissionHost, MissionStage, host_session_id, mint_host_generation,
};
use cs_content::campaign::declared_synthetic_campaign;
use cs_script::runtime::evaluator_seed;
use cs_types::content::{ContentId, ContentKind};

use crate::campaign::vs_m01_rt_window::{Scratch, synthetic_stage};

fn staged(scratch: &Scratch, root: u64, rewards: Vec<ContentId>) -> MissionStage {
    let mut stage = synthetic_stage(scratch.path());
    stage.host.environment = EnvironmentSession::new(
        cs_app::environment::fixture::clear_sky_environment().unwrap(),
        MissionContent::tick_rate(),
        RunSeeds::from_root(root),
    )
    .unwrap();
    stage.host.rewards = rewards;
    stage
}

fn launch(stage: &MissionStage) -> MissionHost {
    let generation = mint_host_generation();
    MissionHost::launch(stage, generation, host_session_id(generation)).unwrap()
}

#[test]
fn accept_f37_c_fu1_host_seeds_the_evaluator_from_the_run_seed_not_the_generation() {
    let scratch = Scratch::new("fu1-seed");
    let mission = ContentId::from_source(ContentKind::Mission, "synthetic-mission-host").unwrap();
    let seed_of = |root| {
        launch(&staged(&scratch, root, vec![]))
            .script()
            .snapshot()
            .state
            .seed
    };
    // Two hosts, two minted generations, one root seed: one stream.
    assert_eq!(seed_of(11), seed_of(11));
    assert_eq!(seed_of(11), evaluator_seed(11, &mission));
    assert_ne!(seed_of(11), seed_of(12));
}

#[test]
fn accept_f37_c_fu1_host_catalog_comes_from_declared_campaign_content() {
    let campaign = declared_synthetic_campaign();
    let m01 = ContentId::from_source(ContentKind::Mission, "synthetic.m01").unwrap();
    let m02 = ContentId::from_source(ContentKind::Mission, "synthetic.m02").unwrap();
    let stranger = ContentId::from_source(ContentKind::Mission, "undeclared").unwrap();
    let unlock = ContentId::from_source(ContentKind::Blueprint, "synthetic.devastator").unwrap();

    let catalog = mission_reward_catalog(&campaign, &m01);
    assert_eq!(catalog, std::slice::from_ref(&unlock));
    assert!(mission_reward_catalog(&campaign, &m02).is_empty());
    assert!(mission_reward_catalog(&campaign, &stranger).is_empty());

    let scratch = Scratch::new("fu1-catalog");
    let declared = launch(&staged(&scratch, 1, catalog));
    assert!(declared.script().host().can_apply(&unlock));
    let undeclared = launch(&staged(
        &scratch,
        1,
        mission_reward_catalog(&campaign, &stranger),
    ));
    assert!(!undeclared.script().host().can_apply(&unlock));
}
