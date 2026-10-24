//! Task #1973: the pilot, the completed count and the mission record survive
//! the start flow's mission-to-cabin return. Task test prefix:
//! `accept_parity5_start_flow_pilot_state_`.
//!
//! The chain's legs are separate processes, so each step below opens the
//! population afresh from the directory the leg would use and drops it again,
//! exactly as an `exec` does. Nothing needs the retail installation.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use cs_app::profile::ProfileSession;
use cs_app::ui::front_end::MissionEnd;
use cs_app::ui::front_end::pilot_progress::{
    Attempt, ProgressError, chain_profile_base, open_session, progress_of, record_attempt,
    record_mission_end,
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Root(PathBuf);

impl Root {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "cs-pilot-state-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the temp directory is created");
        Self(dir)
    }

    fn leg(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn reopen(leg: &Path) -> ProfileSession {
    open_session(Some(leg)).expect("the chain's population opens")
}

#[test]
fn accept_parity5_start_flow_pilot_state_every_leg_opens_one_population() {
    let root = Path::new("/run/chain");
    let base = chain_profile_base(&root.join("front-end"));
    for leg in ["resumed", "mission", "replay"] {
        assert_eq!(chain_profile_base(&root.join(leg)), base, "{leg}");
    }
    // A capture directory that is not a chain leg keeps its pilots under itself.
    assert_eq!(
        chain_profile_base(Path::new("private/front-end-smoke")),
        Path::new("private/front-end-smoke/profiles")
    );
}

#[test]
fn accept_parity5_start_flow_pilot_state_pilot_and_record_survive_the_return() {
    let root = Root::new();
    // Leg 1: the pilot page creates the pilot.
    let id = {
        let mut session = reopen(&root.leg("front-end"));
        let id = session.create("Eve").expect("the pilot is created");
        let progress = progress_of(&session).expect("a pilot is selected");
        assert_eq!((progress.completed, progress.attempts), (0, 0));
        id
    };
    // Leg 2: the player dies in M01 and returns to the cabin.
    record_mission_end(
        Some(&root.leg("mission")),
        "M01",
        MissionEnd::ReturnedToCabin,
    )
    .expect("the failed flight is recorded");
    // Leg 3: the cabin, resumed, shows the same pilot with the attempt.
    {
        let session = reopen(&root.leg("resumed"));
        assert_eq!(session.selected(), Some(id));
        let progress = progress_of(&session).expect("a pilot is selected");
        assert_eq!(progress.name, "Eve");
        assert_eq!(progress.completed, 0, "a failed flight completes nothing");
        assert_eq!(progress.attempts, 1);
        assert_eq!(progress.last, Some(Attempt::Failed));
    }
    // Replay, quit from the pause map, and come back again.
    record_mission_end(Some(&root.leg("replay")), "M01", MissionEnd::Abandoned)
        .expect("the abandoned flight is recorded");
    let session = reopen(&root.leg("resumed"));
    let progress = progress_of(&session).expect("a pilot is selected");
    assert_eq!(session.selected(), Some(id));
    assert_eq!((progress.completed, progress.attempts), (0, 2));
    assert_eq!(progress.last, Some(Attempt::Abandoned));
}

#[test]
fn accept_parity5_start_flow_pilot_state_completed_count_counts_missions_not_attempts() {
    let root = Root::new();
    {
        let mut session = reopen(&root.leg("front-end"));
        session.create("Eve").expect("the pilot is created");
        record_attempt(&mut session, "M01", Attempt::Completed).expect("recorded");
        record_attempt(&mut session, "M01", Attempt::Completed).expect("recorded");
        record_attempt(&mut session, "M02", Attempt::Failed).expect("recorded");
    }
    let progress = progress_of(&reopen(&root.leg("resumed"))).expect("a pilot is selected");
    assert_eq!(progress.completed, 1, "M01 twice is one completed mission");
    assert_eq!(progress.attempts, 3);
    assert_eq!(progress.last, Some(Attempt::Failed));
}

#[test]
fn accept_parity5_start_flow_pilot_state_no_pilot_means_no_record() {
    let root = Root::new();
    let refused = record_mission_end(Some(&root.leg("mission")), "M01", MissionEnd::Abandoned);
    assert!(
        matches!(refused, Err(ProgressError::NoPilot)),
        "{refused:?}"
    );
    assert!(progress_of(&reopen(&root.leg("resumed"))).is_none());
}

#[test]
fn accept_parity5_start_flow_pilot_state_a_second_pilot_keeps_its_own_record() {
    let root = Root::new();
    let (eve, bob) = {
        let mut session = reopen(&root.leg("front-end"));
        let eve = session.create("Eve").expect("created");
        record_attempt(&mut session, "M01", Attempt::Completed).expect("recorded");
        let bob = session.create("Bob").expect("created");
        (eve, bob)
    };
    let mut session = reopen(&root.leg("resumed"));
    assert_eq!(session.selected(), Some(bob), "the newest pilot is active");
    assert_eq!(progress_of(&session).map(|p| p.completed), Some(0));
    session.select(eve).expect("Eve is chosen");
    assert_eq!(progress_of(&session).map(|p| p.completed), Some(1));
}
