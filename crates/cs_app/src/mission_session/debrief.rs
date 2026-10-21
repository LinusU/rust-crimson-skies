//! The run's debrief fields (PARITY-M01-OUTCOME-B, #1654): Run Time, Gun Hit
//! Ratio, Cash Earned and Overall Planes Downed, read once when the run
//! settles.
//!
//! Spec: `missions/M01.md` (`M01-FAILURE`), shared contract
//! `docs/contracts/STATE-TRANSACTIONS.md` (outcome transaction).
//!
//! # What is measured, and what is not
//!
//! * **Run Time** is the host tick the run settled on over the session's
//!   fixed rate, floored to whole seconds. Where the original's clock starts
//!   (launch, end of the launch cinematic, first control) is unmeasured; the
//!   tick count from the composed entry's first tick is what is reported.
//! * **Gun Hit Ratio** is hits over shots of the *player's* guns:
//!   [`MissionTally`] counts each accepted player [`FireEvent`] as a shot and
//!   each swept round whose resolved hits name the player as attacker as a
//!   hit (a round hits at most once). The original's rounding is unmeasured,
//!   so the ratio is kept as the two counts.
//! * **Overall Planes Downed** is the kills the session's score ledger
//!   credited to the player, as the ledger's score over its per-kill value
//!   (guns and rockets alike, each victim exactly once).
//! * **Cash Earned** is an explicit unknown: no cash rule or award table is
//!   measured (see `score.rs`, "Designed value"), so nothing is invented.
//!
//! [`FireEvent`]: cs_sim::weapons::FireEvent

use bevy::prelude::{Resource, World};
use cs_types::content::Resolved;
use cs_types::evidence::ClaimId;
use cs_types::net::SessionId;

use crate::mission_world_actors::SESSION_TICKS_PER_SECOND;
use crate::score::SessionScoreLedger;
use crate::weapons::WeaponScheduleReport;

use super::player_victim::PlayerVictim;

/// The claim the unmeasured cash award is filed under.
pub const M01_CASH_EARNED_UNKNOWN_CLAIM: &str = "m01-debrief.cash-earned-unmeasured";

/// The player's gun counts for the session being served.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct MissionTally {
    /// The session generation the counts belong to; a new generation starts
    /// from zero.
    session: u64,
    /// Rounds the player's guns fired.
    pub shots: u64,
    /// Rounds that hit something the player was credited with.
    pub hits: u64,
}

impl MissionTally {
    /// The session generation these counts belong to.
    #[must_use]
    pub const fn session(&self) -> u64 {
        self.session
    }
}

/// Folds the tick's weapon step into the player's gun tally.
///
/// Runs after the weapon and ordnance steps and before the damage schedule,
/// on the [`WeaponScheduleReport`] the step just wrote.
pub fn tally_player_gun(world: &mut World) {
    let Some(player) = world
        .get_resource::<PlayerVictim>()
        .map(|victim| victim.actor)
    else {
        return;
    };
    let Some(report) = world.get_resource::<WeaponScheduleReport>() else {
        return;
    };
    let Some(session) = report.session else {
        return;
    };
    let (mut shots, mut hits) = (0, 0);
    if let Some(stepped) = &report.stepped {
        shots = stepped
            .accepted
            .iter()
            .filter(|fired| fired.shooter == player)
            .count() as u64;
        hits = stepped
            .routed
            .iter()
            .filter(|routed| {
                routed
                    .outcome
                    .sweep
                    .damage()
                    .iter()
                    .any(|hit| hit.attacker == Some(player) && hit.target != player)
            })
            .count() as u64;
    }
    let mut tally = world.get_resource_or_insert_with(MissionTally::default);
    if tally.session != session {
        *tally = MissionTally {
            session,
            ..MissionTally::default()
        };
    }
    tally.shots += shots;
    tally.hits += hits;
}

/// The fields the debrief shows for one settled run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissionDebrief {
    /// Host ticks the run lasted.
    pub run_ticks: u64,
    /// Ticks per simulated second at which they were counted.
    pub ticks_per_second: u32,
    /// Rounds the player's guns fired.
    pub shots: u64,
    /// Rounds that hit.
    pub hits: u64,
    /// Aircraft the player downed.
    pub planes_downed: u64,
    /// Cash earned: an explicit unknown.
    pub cash_earned: Resolved<u64>,
}

impl MissionDebrief {
    /// Reads the debrief of the run `session` served, which settled on host
    /// tick `run_ticks`.
    ///
    /// A tally or ledger of another generation contributes nothing.
    #[must_use]
    pub fn read(world: &World, session: SessionId, run_ticks: u64) -> Self {
        let (shots, hits) = world
            .get_resource::<MissionTally>()
            .filter(|tally| tally.session == session.get())
            .map_or((0, 0), |tally| (tally.shots, tally.hits));
        let planes_downed = world
            .get_resource::<SessionScoreLedger>()
            .filter(|ledger| ledger.session() == session)
            .zip(world.get_resource::<PlayerVictim>())
            .map_or(0, |(ledger, victim)| {
                let points = ledger.ledger().kill_score().points();
                if points > 0 {
                    u64::try_from(ledger.ledger().score_for(victim.actor) / points).unwrap_or(0)
                } else {
                    0
                }
            });
        let cash_earned = Resolved::<u64>::unknown(
            ClaimId::new(M01_CASH_EARNED_UNKNOWN_CLAIM).expect("a claim id this module declares"),
            "no cash award rule or table is measured, so the debrief's Cash Earned stays unknown",
        )
        .expect("the reason is not empty");
        Self {
            run_ticks,
            ticks_per_second: SESSION_TICKS_PER_SECOND,
            shots,
            hits,
            planes_downed,
            cash_earned,
        }
    }

    /// Whole seconds the run lasted.
    #[must_use]
    pub fn run_seconds(&self) -> u64 {
        self.run_ticks / u64::from(self.ticks_per_second.max(1))
    }

    /// Run Time as `m:ss`.
    #[must_use]
    pub fn run_time(&self) -> String {
        let seconds = self.run_seconds();
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }

    /// Gun Hit Ratio, `hits / shots`; `None` when no shot was fired.
    #[must_use]
    pub fn gun_hit_ratio(&self) -> Option<f64> {
        (self.shots > 0).then(|| self.hits as f64 / self.shots as f64)
    }

    /// The fields as the one fragment the terminal's report line carries.
    #[must_use]
    pub fn report_fragment(&self) -> String {
        let ratio = self
            .gun_hit_ratio()
            .map_or_else(|| "none".to_owned(), |ratio| format!("{ratio:.3}"));
        format!(
            "run_time={} shots={} hits={} gun_hit_ratio={} cash_earned=unknown planes_downed={}",
            self.run_time(),
            self.shots,
            self.hits,
            ratio,
            self.planes_downed,
        )
    }
}
