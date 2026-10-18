//! F29-C.3c acceptance: what an extraction records as a campaign and
//! scrapbook outcome is **unmeasured**, so both builders refuse it by name
//! and a settled success or failure still records (task #1427, follow-up of
//! F29-C.3b). Task test prefix: `accept_f29_c_3c_`. The evidence search and
//! the decision are in
//! `docs/findings/2026-10-18-f29-c-3c-extraction-outcome.md`.
//!
//! Removing the refusal (mapping `Extraction` to any `Outcome`) fails the
//! first test; collapsing the match so that every terminal is refused fails
//! the second. Values are newly authored synthetic data.

use cs_app::mission_session::{MissionTerminal, MissionTerminalSource};
use cs_app::score::{OutcomeRecordError, mission_outcome, mission_result};
use cs_script::ir::{Outcome, SymbolId};
use cs_sim::campaign::{
    CampaignNodeKey, CampaignRunId, DifficultyId, EventKey, OutcomeAuthority, OutcomeId, ProfileId,
    SessionGeneration,
};
use cs_sim::objectives::terminal::TerminalOutcome;
use cs_sim::records::RecordRule;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};

fn terminal(outcome: TerminalOutcome) -> MissionTerminal {
    MissionTerminal::new(
        outcome,
        MissionTerminalSource::Objectives,
        Tick(10),
        SessionGeneration(1),
        None,
        0,
        0,
        7,
    )
}

fn id() -> OutcomeId {
    OutcomeId {
        profile: ProfileId::new("pilot.nathan").expect("valid profile"),
        run: CampaignRunId::new("run.one").expect("valid run"),
        session: SessionGeneration(1),
        terminal_event: EventKey {
            session: SessionGeneration(1),
            tick: Tick(10),
            source: SymbolId(0),
            sequence: 0,
        },
    }
}

fn mission() -> ContentId {
    ContentId::from_source(ContentKind::Mission, "synthetic.m01").expect("fixture mission id")
}

#[test]
fn accept_f29_c_3c_an_extraction_is_refused_by_both_outcome_builders() {
    let run = terminal(TerminalOutcome::Extraction);
    let node = CampaignNodeKey::new("m01").expect("fixture node key");
    assert_eq!(
        mission_outcome(&run, id(), node, OutcomeAuthority::Authorized),
        Err(OutcomeRecordError::Extraction),
        "no campaign record: the extraction mapping is unmeasured"
    );
    let difficulty = DifficultyId::new("standard").expect("valid difficulty");
    assert_eq!(
        mission_result(&run, id(), mission(), difficulty, RecordRule::DEFAULT).err(),
        Some(OutcomeRecordError::Extraction),
        "no scrapbook record: the extraction mapping is unmeasured"
    );
}

#[test]
fn accept_f29_c_3c_success_and_failure_still_record_their_own_outcome() {
    for (terminal_outcome, expected) in [
        (TerminalOutcome::Success, Outcome::Succeeded),
        (TerminalOutcome::Failure, Outcome::Failed),
    ] {
        let node = CampaignNodeKey::new("m01").expect("fixture node key");
        let record = mission_outcome(
            &terminal(terminal_outcome),
            id(),
            node,
            OutcomeAuthority::Authorized,
        )
        .expect("a success or failure builds its record");
        assert_eq!(record.outcome, expected);
        assert_eq!(record.score, 7);
    }
}
