//! The lobby screen's connection to the authoritative lobby (F55-C).
//!
//! Spec: `specs/F55-multiplayer-lobby-host-rules-readiness-and-ux.md`, stage
//! `### F55-C`. Shared contract: `docs/contracts/UI-NETWORK.md` ("A UI action
//! requests a domain transaction; it does not directly edit ... fields",
//! "Lobby host changes rules through a validated server action").
//!
//! [`LobbyLink`] is that seam: the screen produces typed requests
//! ([`JoinRequest`], [`LobbyCommand`]), the link hands them to
//! [`cs_net::lobby::Lobby`] — the single owner of lobby truth — and the
//! [`Applied`] events it answers with are the only thing the screen ever
//! displays. Nothing here edits a member, a rule or a readiness directly.
//!
//! **What this link is, and what it is not.** The link holds a real
//! [`Lobby`], so it is the production path a *host* runs: the host's own
//! screen drives the authoritative record through it, and a joining player's
//! [`JoinRequest`] is admitted by that same [`Lobby::admit`]. Carrying those
//! requests to a lobby in **another process** needs the F54 session wire;
//! `cs_net::message`/`lifecycle` are outside this task's owner paths and the
//! lobby envelope is still not carried (follow-up #763, `F55-BU1-WIRE`). So
//! today the endpoint behind a join form is a lobby in this process, and no
//! claim of remote joining is made for F55-C. Resolving the join form's
//! direct address into an endpoint — and LAN discovery — is likewise not
//! implemented here.

use std::fmt;

use cs_net::compat::SessionParameters;
use cs_net::lobby::{
    Admission, Applied, Callsign, Departure, JoinError, JoinRequest, LoadoutValidator, Lobby,
    LobbyCommand, LobbyError, LobbyRules, Revision,
};
use cs_types::net::{PeerId, SessionId};

/// Everything the host form asks the domain to do when it opens a lobby.
///
/// The session epoch and the compatibility signature are supplied by the
/// application, not invented by the UI: they belong to the transport and the
/// mounted content ("Protocol messages carry session epoch and sequence/tick").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostSettings {
    /// The session epoch the lobby is opened under.
    pub session: SessionId,
    /// The signature every member must match.
    pub params: SessionParameters,
    /// The host form's admission settings (access, member cap, host loss).
    pub admission: Admission,
    /// The host form's rules (scenario, bans, teams, late join).
    pub rules: LobbyRules,
    /// The host's own callsign.
    pub host_callsign: Callsign,
}

/// The screen's typed connection to one lobby: the authoritative [`Lobby`]
/// plus the shared loadout validator every command is judged by.
///
/// The validator is *injected*, never invented here: `cs_net` cannot depend
/// on the content crates, and no production implementation of
/// [`LoadoutValidator`] exists yet (follow-up #764, `F55-BU2-VALIDATOR`).
/// The application supplies one; until it does, budget/fitment validation of
/// lobby loadouts stays unproven exactly as that task records.
pub struct LobbyLink {
    lobby: Lobby,
    validator: Box<dyn LoadoutValidator>,
}

impl fmt::Debug for LobbyLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LobbyLink")
            .field("lobby", &self.lobby)
            .field("validator", &"<loadout validator>")
            .finish()
    }
}
impl LobbyLink {
    /// Opens the host's lobby from the host form's requested transaction.
    ///
    /// # Errors
    ///
    /// The [`LobbyError`] the domain reports for invalid rules, an unusable
    /// member cap or an invalid host callsign; nothing is opened.
    pub fn host(
        settings: HostSettings,
        validator: Box<dyn LoadoutValidator>,
    ) -> Result<Self, LobbyError> {
        let lobby = Lobby::open(
            settings.session,
            settings.params,
            settings.admission,
            settings.rules,
            settings.host_callsign,
        )?;
        Ok(Self { lobby, validator })
    }

    /// Admits one joining player through the host's own admission check
    /// (password → content → phase/late join → capacity → callsign).
    ///
    /// # Errors
    ///
    /// The [`JoinError`] naming exactly why the join was refused; a refused
    /// join allocates no peer id, so the caller may correct and retry.
    pub fn join(&mut self, request: JoinRequest) -> Result<(PeerId, Applied), JoinError> {
        self.lobby.admit(request)
    }

    /// Sends one command to the lobby. Host authority, phase, revision and
    /// loadout rules are all judged by the lobby itself.
    ///
    /// # Errors
    ///
    /// The [`LobbyError`] refusing the command; a refused command changes
    /// nothing and stays retryable.
    pub fn command(&mut self, from: PeerId, command: LobbyCommand) -> Result<Applied, LobbyError> {
        self.lobby.apply(from, command, self.validator.as_ref())
    }

    /// A member (or the host) leaves: the departure the application
    /// broadcasts to everybody still in the lobby.
    pub fn leave(&mut self, from: PeerId) -> Option<Departure> {
        self.lobby.remove_peer(from)
    }

    /// The authoritative lobby, read-only.
    #[must_use]
    pub fn lobby(&self) -> &Lobby {
        &self.lobby
    }

    /// The host's own peer id.
    #[must_use]
    pub fn host_peer(&self) -> PeerId {
        self.lobby.host()
    }

    /// The live rules revision a readiness must acknowledge.
    #[must_use]
    pub fn revision(&self) -> Revision {
        self.lobby.revision()
    }

    /// Whether `peer` is the host. UI controls may be *shaped* by this; the
    /// authority check itself is the lobby's and cannot be reached around.
    #[must_use]
    pub fn is_host(&self, peer: PeerId) -> bool {
        peer == self.lobby.host()
    }
}
