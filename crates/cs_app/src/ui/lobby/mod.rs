//! Lobby screen projection and screen (F55-A, F55-C).
//!
//! Spec: `specs/F55-multiplayer-lobby-host-rules-readiness-and-ux.md`, stages
//! `### F55-A` and `### F55-C`. The lobby truth is `cs_net::lobby::Lobby`,
//! owned by the host; this module is the client's view of the events the
//! host sends and owns only client state: the stage and form drafts, the
//! focus, and the mute list.
//!
//! [`LobbyView::observe`] turns each [`LobbyEvent`] into display lines. A
//! readiness revocation becomes a [`Notice::ReadyRevoked`] naming the
//! affected member and the reason's localization key, so "ready state is
//! revoked and reason displayed" (F55 AC01) is a property of the projection,
//! not of a renderer. Chat from a muted peer is dropped here, and chat text is
//! kept as literal text, never markup.
//!
//! F55-C adds the screen itself ([`screen::LobbyScreen`]: the stage table and
//! the join/host/team/loadout/chat forms) and its connection to the
//! authoritative lobby ([`link::LobbyLink`]). No widget layout or
//! localization lookup is wired yet: the reason carries its stable key and
//! the designed English fallback only.
//!
//! F55-BU2 adds the production loadout validator
//! ([`validator::ConstructionLoadoutValidator`]): the
//! `cs_net::lobby::LoadoutValidator` the lobby had only test doubles for,
//! judged over the shared `cs_content::construction` rules through an authored
//! blueprint catalog (`cs_content::lobby`). Without one injected, the lobby
//! refuses a loadout it cannot judge rather than accepting it unjudged.

pub mod launch;
pub mod link;
pub mod results;
pub mod screen;
pub mod validator;

use std::collections::BTreeSet;

use cs_net::lifecycle::ClientNotice;
use cs_net::lobby::{ChatText, LobbyEvent, Revision, RevokeReason};
use cs_net::message::LobbyRefusal;
use cs_types::net::PeerId;

pub use launch::{LaunchInputs, LaunchRefusal, LaunchStep, MatchLauncher};
pub use link::{HostSettings, LobbyLink};
pub use results::ResultsScreen;
pub use screen::{
    Back, Field, FieldProblem, HostDraft, JoinDraft, LobbyScreen, LobbyUiError, Stage,
};
pub use validator::ConstructionLoadoutValidator;

/// A line the lobby screen shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    /// A member's readiness was revoked, with the reason.
    ReadyRevoked {
        /// Whose readiness.
        peer: PeerId,
        /// Why.
        reason: RevokeReason,
    },
    /// The rules changed.
    RulesChanged {
        /// The new revision.
        revision: Revision,
    },
    /// A chat line, literal text.
    Chat {
        /// The sender.
        from: PeerId,
        /// The line.
        text: ChatText,
    },
    /// A member joined the lobby.
    MemberJoined {
        /// Who.
        peer: PeerId,
    },
    /// A member left the lobby cleanly (spec non-negotiable 4).
    MemberLeft {
        /// Who.
        peer: PeerId,
    },
    /// A pending launch was cancelled.
    LaunchCancelled,
    /// The host refused a packet this client sent (F55-BU1-WIRE); the lobby
    /// is unchanged and the player may correct and retry.
    PacketRefused {
        /// The class of refusal the host reported.
        refusal: LobbyRefusal,
    },
}

impl Notice {
    /// The designed English fallback line. Original strings are not
    /// involved; localization (F51) supplies the real text by
    /// [`RevokeReason::message_key`].
    pub fn fallback_text(&self) -> String {
        match self {
            Self::ReadyRevoked { peer, reason } => {
                let why = match reason {
                    RevokeReason::ComponentBanned { component } => {
                        format!("the host banned {}", component.as_str())
                    }
                    RevokeReason::ScenarioChanged => "the host changed the scenario".to_owned(),
                    RevokeReason::TeamsChanged => "the host changed the teams".to_owned(),
                    RevokeReason::SelfEdited => "you changed your setup".to_owned(),
                };
                format!("{peer} is no longer ready: {why}")
            }
            Self::RulesChanged { revision } => format!("The rules changed ({revision})"),
            Self::Chat { from, text } => format!("{from}: {}", text.as_str()),
            Self::MemberJoined { peer } => format!("{peer} joined"),
            Self::MemberLeft { peer } => format!("{peer} left"),
            Self::LaunchCancelled => "Launch cancelled".to_owned(),
            Self::PacketRefused { refusal } => match refusal {
                LobbyRefusal::StaleRevision => {
                    "The host refused that: the rules changed meanwhile".to_owned()
                }
                other => format!("The host refused that ({})", other.label()),
            },
        }
    }
}

/// The client's local view of one lobby.
#[derive(Debug, Default)]
pub struct LobbyView {
    notices: Vec<Notice>,
    muted: BTreeSet<PeerId>,
}

/// The most notices kept on screen; older ones are dropped.
pub const MAX_NOTICES: usize = 64;

impl LobbyView {
    /// An empty view.
    pub fn new() -> Self {
        Self::default()
    }

    /// Stops showing chat from `peer`. Local only: mute is never sent.
    pub fn mute(&mut self, peer: PeerId) {
        self.muted.insert(peer);
    }

    /// Shows chat from `peer` again.
    pub fn unmute(&mut self, peer: PeerId) {
        self.muted.remove(&peer);
    }

    /// Whether `peer` is muted.
    pub fn is_muted(&self, peer: PeerId) -> bool {
        self.muted.contains(&peer)
    }

    /// Drops every notice: what entering or leaving a screen resets. The
    /// mute list is client preference and is kept.
    pub fn clear(&mut self) {
        self.notices.clear();
    }

    /// Projects one host event into display notices.
    pub fn observe(&mut self, event: &LobbyEvent) {
        let notice = match event {
            LobbyEvent::ReadyRevoked(revocation) => Notice::ReadyRevoked {
                peer: revocation.peer,
                reason: revocation.reason.clone(),
            },
            LobbyEvent::RulesChanged { revision, .. } => Notice::RulesChanged {
                revision: *revision,
            },
            LobbyEvent::Chat { from, text } if !self.muted.contains(from) => Notice::Chat {
                from: *from,
                text: text.clone(),
            },
            LobbyEvent::MemberJoined { peer } => Notice::MemberJoined { peer: *peer },
            LobbyEvent::MemberLeft { peer } => Notice::MemberLeft { peer: *peer },
            LobbyEvent::LaunchCancelled => Notice::LaunchCancelled,
            _ => return,
        };
        self.push(notice);
    }

    /// Projects what a session client pump observed: every lobby event the
    /// host published reaches [`Self::observe`], and a refusal of this
    /// client's own packet becomes a [`Notice::PacketRefused`]. Notices that
    /// are not about the lobby are ignored. The session already deduplicated
    /// the events by their `EventId`, so each is observed once.
    pub fn observe_session(&mut self, notices: &[ClientNotice]) {
        for notice in notices {
            match notice {
                ClientNotice::Lobby { event, .. } => self.observe(event),
                ClientNotice::LobbyRefused { refusal, .. } => {
                    self.push(Notice::PacketRefused { refusal: *refusal });
                }
                _ => {}
            }
        }
    }

    fn push(&mut self, notice: Notice) {
        if self.notices.len() == MAX_NOTICES {
            self.notices.remove(0);
        }
        self.notices.push(notice);
    }

    /// The notices, oldest first.
    pub fn notices(&self) -> &[Notice] {
        &self.notices
    }
}

// F56-C2: [`launch`] resolves the host's options into a match before the
// lobby commits the launch, and [`results`] shows the finished match and
// returns to the lobby.
