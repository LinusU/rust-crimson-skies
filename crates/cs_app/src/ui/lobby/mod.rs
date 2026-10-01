//! Lobby screen projection (F55-A).
//!
//! Spec: `specs/F55-multiplayer-lobby-host-rules-readiness-and-ux.md`, stage
//! `### F55-A`. The lobby truth is `cs_net::lobby::Lobby`, owned by the host;
//! this module is the client's read-only view of the events the host sends and
//! owns only client state: the notices on screen and the mute list.
//!
//! [`LobbyView::observe`] turns each [`LobbyEvent`] into display lines. A
//! readiness revocation becomes a [`Notice::ReadyRevoked`] naming the
//! affected member and the reason's localization key, so "ready state is
//! revoked and reason displayed" (F55 AC01) is a property of the projection,
//! not of a renderer. Chat from a muted peer is dropped here, and chat text is
//! kept as literal text, never markup.
//!
//! No widget layout, localization lookup or input is wired yet (F55-C); the
//! reason carries its stable key and the designed English fallback only.

use std::collections::BTreeSet;

use cs_net::lobby::{ChatText, LobbyEvent, Revision, RevokeReason};
use cs_types::net::PeerId;

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
    /// A pending launch was cancelled.
    LaunchCancelled,
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
            Self::LaunchCancelled => "Launch cancelled".to_owned(),
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
            LobbyEvent::LaunchCancelled => Notice::LaunchCancelled,
            _ => return,
        };
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
