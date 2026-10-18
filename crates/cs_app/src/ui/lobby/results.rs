//! The multiplayer results screen (F56-C2).
//!
//! Spec: `specs/F56-original-multiplayer-scenarios-and-mode-rules.md`, stage
//! `### F56-C`; contract `docs/contracts/UI-NETWORK.md` (lobby join -> ready
//! -> match -> results -> lobby). [`ResultsScreen`] renders a
//! `cs_sim::multiplayer::session::EndOfMatch` through its stable
//! localization keys (`match.end.*`, `match.outcome.*`) plus a designed
//! English fallback; the original's end-of-match presentation is unknown
//! (F56-C findings), so no layout is claimed. [`ResultsScreen::back_to_lobby`]
//! is the `results -> lobby` transition: it drops the finished match, so the
//! next launch is a new session generation.

use cs_sim::multiplayer::result::Side;
use cs_sim::multiplayer::session::EndOfMatch;

use super::launch::{LaunchRefusal, MatchLauncher};
use super::link::LobbyLink;

/// One line of the standings table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StandingRow {
    /// Who scored.
    pub side: Side,
    /// The score.
    pub score: i32,
}

/// The results of one finished match, ready to draw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultsScreen {
    record: EndOfMatch,
}

impl ResultsScreen {
    /// The results screen of the launcher's match, once it has ended.
    pub fn from_launcher(launcher: &MatchLauncher) -> Option<Self> {
        launcher
            .session()?
            .end_of_match()
            .map(|record| Self { record })
    }

    /// The record shown.
    pub fn record(&self) -> &EndOfMatch {
        &self.record
    }

    /// The localization key of why the match ended.
    pub fn reason_key(&self) -> &'static str {
        self.record.reason_key()
    }

    /// The localization key of the outcome.
    pub fn outcome_key(&self) -> &'static str {
        self.record.outcome_key()
    }

    /// The standings, highest first.
    pub fn standings(&self) -> Vec<StandingRow> {
        self.record
            .standings
            .iter()
            .map(|(side, score)| StandingRow {
                side: *side,
                score: *score,
            })
            .collect()
    }

    /// The designed English lines: reason, outcome, then the standings.
    pub fn fallback_lines(&self) -> Vec<String> {
        let reason = match self.reason_key() {
            "match.end.score_limit" => "The score limit was reached",
            "match.end.time_limit" => "The time limit was reached",
            "match.end.departure" => "A pilot left and the match ended",
            _ => "The score and time limits were reached together",
        };
        let outcome = match &self.record.outcome {
            cs_sim::multiplayer::result::Outcome::Winner(side) => {
                format!("Winner: {}", side_text(*side))
            }
            cs_sim::multiplayer::result::Outcome::Draw(sides) => format!(
                "Draw: {}",
                sides
                    .iter()
                    .map(|s| side_text(*s))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
        let mut lines = vec![reason.to_owned(), outcome];
        lines.extend(
            self.record
                .standings
                .iter()
                .map(|(side, score)| format!("{}: {score}", side_text(*side))),
        );
        lines
    }

    /// `results -> lobby`: the lobby returns to gathering (everyone unready)
    /// and the finished match is dropped.
    ///
    /// # Errors
    ///
    /// The [`LaunchRefusal`] from [`MatchLauncher::finish`].
    pub fn back_to_lobby(
        self,
        launcher: &mut MatchLauncher,
        link: &mut LobbyLink,
    ) -> Result<(), LaunchRefusal> {
        launcher.finish(link)
    }
}

fn side_text(side: Side) -> String {
    match side {
        Side::Participant(peer) => peer.to_string(),
        Side::Team(team) => format!("team {team}"),
    }
}
