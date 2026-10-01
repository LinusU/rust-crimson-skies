//! Lobby state and the rules-revision protocol (F55-A).
//!
//! Spec: `specs/F55-multiplayer-lobby-host-rules-readiness-and-ux.md`, stage
//! `### F55-A`; contract `docs/contracts/UI-NETWORK.md` ("Lobby host changes
//! rules through a validated server action").
//!
//! A [`Lobby`] is the host's authoritative lobby record. Everything a client
//! may do is a [`PeerRequest`]; everything only the host may do is a
//! [`HostAction`]; both enter through [`Lobby::apply`], which checks authority
//! first, so "no client can grant itself host authority" is one
//! [`LobbyError::NotHost`] path and not a convention. No client-owned verb
//! launches: [`Lobby::begin_launch`] is a separate host entry point.
//!
//! **Rules revisions.** [`LobbyRules`] (scenario, banned components, team
//! mode, late-join policy) is versioned by a [`Revision`] and summarised by a
//! [`RulesDigest`]. Every host change that alters the rules bumps the
//! revision, and every [`Ready`] state records the revision it acknowledged:
//!
//! * a scenario or team-mode change revokes *every* member's readiness;
//! * a ban revokes readiness only of members whose loadout holds the banned
//!   component (a ban only narrows the rules, so everyone else's loadout is
//!   still valid and their readiness is carried to the new revision);
//! * an unban only widens the rules and revokes nothing.
//!
//! Each revocation is a [`ReadyRevocation`] naming the peer and the
//! [`RevokeReason`], returned in [`Applied::events`] as a
//! [`LobbyEvent::ReadyRevoked`] so the UI can display the reason (F55 AC01).
//! A [`PeerRequest::SetReady`] stamped with an old revision is refused with
//! [`LobbyError::StaleRevision`], so a client racing a ban cannot ready itself
//! into rules it has not seen.
//!
//! **Launch** is a two-step acknowledged transition bound to the revision and
//! digest ([`Lobby::begin_launch`] then [`Lobby::acknowledge_launch`]). A
//! request for an old revision or digest is refused ([`LaunchError`], F55
//! AC02), rules are locked while a launch is pending, and a departing member
//! or a host cancel returns the lobby to gathering without half a launch.
//!
//! **Joining** ([`Lobby::admit`]) refuses with distinct errors
//! ([`JoinError::WrongPassword`], [`JoinError::LobbyFull`],
//! [`JoinError::ContentMismatch`], F55 AC03).
//!
//! Everything here is newly authored design. The original multiplayer option
//! table is not known; see `docs/findings/2026-10-02-f55-a-lobby-state-and-
//! revision-protocol.md`. [`RulesDigest`] is an FNV-1a staleness check, not a
//! security primitive, and the password comparison is a placeholder for the
//! F58-A session identity rules.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cs_types::content::{ContentId, ContentKind};
use cs_types::net::{PeerId, SessionId};

use crate::compat::{
    ClientHello, Compatibility, HandshakeReject, PROTOCOL_VERSION, PeerAllocError, PeerAllocator,
    SessionParameters, evaluate_hello,
};

/// Longest callsign, in characters.
pub const MAX_CALLSIGN_CHARS: usize = 24;
/// Longest chat line, in characters.
pub const MAX_CHAT_CHARS: usize = 200;
/// Most components one lobby may ban.
pub const MAX_BANNED_COMPONENTS: usize = 256;
/// Most components one loadout may name.
pub const MAX_LOADOUT_COMPONENTS: usize = 64;
/// Longest lobby password, in bytes.
pub const MAX_PASSWORD_BYTES: usize = 64;
/// Most teams a team mode may declare.
pub const MAX_TEAMS: u8 = 8;

/// The rules revision: increments on every host change that alters
/// [`LobbyRules`]. Starts at 1; 0 is never a live revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Revision(u32);

impl Revision {
    /// The first revision of a new lobby.
    pub const FIRST: Self = Self(1);

    /// Wraps a revision number; zero is refused.
    pub const fn new(value: u32) -> Option<Self> {
        if value == 0 { None } else { Some(Self(value)) }
    }

    /// The number.
    pub const fn get(self) -> u32 {
        self.0
    }

    fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

impl fmt::Display for Revision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "rules revision {}", self.0)
    }
}

/// A staleness digest of the canonical [`LobbyRules`] (64-bit FNV-1a over a
/// canonical byte form). It detects a launch bound to different rules than the
/// ones in force; it is not collision- or attack-resistant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RulesDigest(u64);

impl RulesDigest {
    /// The digest number.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A team slot, `0..teams`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TeamId(pub u8);

/// How members group.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TeamMode {
    /// Everyone for themselves; members hold no team.
    FreeForAll,
    /// `teams` teams (`2..=`[`MAX_TEAMS`]); every ready member holds one.
    Teams {
        /// The team count.
        teams: u8,
    },
}

/// Whether a lobby that already launched admits new members. Explicit per
/// lobby (spec: "late join rules are explicit per mode").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LateJoin {
    /// Joining is refused once the match launched.
    Closed,
    /// Joining stays open during the match.
    Open,
}

/// What happens when the host's connection is lost. A designed policy, not an
/// accident (spec non-negotiable 4); host migration is out of scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HostLoss {
    /// The lobby closes at once and every member is told.
    EndSession,
    /// The lobby pauses for up to `ticks` simulation ticks, then closes
    /// unless the host returns.
    PauseThenEnd {
        /// The grace period.
        ticks: u32,
    },
}

/// The versioned rules the host changes through [`HostAction`]s.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LobbyRules {
    /// The selected multiplayer scenario.
    pub scenario: ContentId,
    /// Components no loadout may contain.
    pub banned: BTreeSet<ContentId>,
    /// Team grouping.
    pub team_mode: TeamMode,
    /// Late-join policy.
    pub late_join: LateJoin,
}

impl LobbyRules {
    /// The canonical digest of these rules.
    pub fn digest(&self) -> RulesDigest {
        let mut hash = Fnv::new();
        hash.text(self.scenario.as_str());
        hash.number(self.banned.len() as u64);
        for banned in &self.banned {
            hash.text(banned.as_str());
        }
        match self.team_mode {
            TeamMode::FreeForAll => hash.number(0),
            TeamMode::Teams { teams } => {
                hash.number(1);
                hash.number(u64::from(teams));
            }
        }
        hash.number(match self.late_join {
            LateJoin::Closed => 0,
            LateJoin::Open => 1,
        });
        RulesDigest(hash.0)
    }
}

struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
    fn byte(&mut self, byte: u8) {
        self.0 ^= u64::from(byte);
        self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
    }
    fn number(&mut self, value: u64) {
        for byte in value.to_le_bytes() {
            self.byte(byte);
        }
    }
    fn text(&mut self, text: &str) {
        self.number(text.len() as u64);
        for byte in text.bytes() {
            self.byte(byte);
        }
    }
}

/// A password. Its `Debug` output is redacted so a log line can never carry it.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// Wraps a nonempty password of at most [`MAX_PASSWORD_BYTES`] bytes.
    pub fn new(text: &str) -> Option<Self> {
        if text.is_empty() || text.len() > MAX_PASSWORD_BYTES {
            None
        } else {
            Some(Self(text.to_owned()))
        }
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

/// Who may join. Not part of the revisioned rules: changing it never
/// invalidates a loadout or a readiness.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AccessPolicy {
    /// Anyone compatible may join.
    Open,
    /// Joining needs this password.
    Password(Secret),
}

/// Host configuration fixed or changed outside the rules revision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Admission {
    /// Who may join.
    pub access: AccessPolicy,
    /// The member cap, host included (`1..=`[`crate::bounds::MAX_SESSION_PEERS`]).
    pub max_members: usize,
    /// What a lost host does to the lobby.
    pub host_loss: HostLoss,
}

/// A callsign: nonempty, at most [`MAX_CALLSIGN_CHARS`] characters, no control
/// characters, no leading or trailing space.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Callsign(String);

impl Callsign {
    /// Validates a callsign.
    ///
    /// # Errors
    ///
    /// [`LobbyError::InvalidCallsign`].
    pub fn new(text: &str) -> Result<Self, LobbyError> {
        let count = text.chars().count();
        if count == 0
            || count > MAX_CALLSIGN_CHARS
            || text.trim() != text
            || text.chars().any(char::is_control)
        {
            return Err(LobbyError::InvalidCallsign);
        }
        Ok(Self(text.to_owned()))
    }

    /// The text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One chat line: plain, bounded text. It is data, never markup: a renderer
/// shows [`ChatText::as_str`] literally, and [`ChatText::escaped`] is the form
/// for any sink that does interpret markup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatText(String);

impl ChatText {
    /// Validates a chat line: nonempty, at most [`MAX_CHAT_CHARS`] characters,
    /// no control characters.
    ///
    /// # Errors
    ///
    /// [`LobbyError::InvalidChat`].
    pub fn new(text: &str) -> Result<Self, LobbyError> {
        let count = text.chars().count();
        if text.trim().is_empty() || count > MAX_CHAT_CHARS || text.chars().any(char::is_control) {
            return Err(LobbyError::InvalidChat);
        }
        Ok(Self(text.to_owned()))
    }

    /// The literal text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The text with `& < > " '` replaced by entities.
    pub fn escaped(&self) -> String {
        let mut out = String::with_capacity(self.0.len());
        for ch in self.0.chars() {
            match ch {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '"' => out.push_str("&quot;"),
                '\'' => out.push_str("&#39;"),
                other => out.push(other),
            }
        }
        out
    }
}

/// A member's chosen aircraft and component set, by catalog id. The shared
/// blueprint validator (F44) judges budgets through [`LoadoutValidator`]; the
/// lobby itself judges only the host's bans.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loadout {
    /// The blueprint the member would fly.
    pub blueprint: ContentId,
    /// The components it carries (guns, ammunition, armor, equipment...).
    pub components: Vec<ContentId>,
}

/// Why a loadout is not acceptable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadoutProblem {
    /// It names a component the host banned.
    Banned {
        /// The banned component.
        component: ContentId,
    },
    /// It lists more than [`MAX_LOADOUT_COMPONENTS`] components, or a
    /// component twice, or an id of a kind that is not a component.
    Malformed,
    /// The shared blueprint validator refused it, with its stable code.
    Invalid {
        /// The validator's stable reason code.
        code: &'static str,
    },
}

/// The shared blueprint/budget validator (F44) as the lobby sees it. The
/// lobby never accepts a loadout this refuses; the production implementation
/// is wired by F55-B in `cs_content`/`cs_app`, since `cs_net` cannot depend on
/// content crates.
pub trait LoadoutValidator {
    /// Judges a loadout against the shared construction rules.
    ///
    /// # Errors
    ///
    /// A stable reason code.
    fn validate(&self, loadout: &Loadout) -> Result<(), &'static str>;
}

/// Whether `kind` names something a loadout carries or a host may ban.
pub const fn is_component_kind(kind: ContentKind) -> bool {
    matches!(
        kind,
        ContentKind::Airframe
            | ContentKind::Engine
            | ContentKind::Armor
            | ContentKind::Gun
            | ContentKind::Ammo
            | ContentKind::HardpointEquipment
    )
}

/// A member's readiness: bound to the rules revision it acknowledged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ready {
    /// Not ready.
    NotReady,
    /// Ready under `revision`.
    Ready {
        /// The acknowledged rules revision.
        revision: Revision,
    },
}

/// One lobby member.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    /// The member's callsign.
    pub callsign: Callsign,
    /// The member's team, when the mode has teams.
    pub team: Option<TeamId>,
    /// The chosen loadout.
    pub loadout: Option<Loadout>,
    /// Readiness.
    pub ready: Ready,
}

/// Where the lobby is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Members join, edit and ready up; the host edits rules.
    Gathering,
    /// A launch is pending acknowledgment; rules and readiness are locked.
    Launching,
    /// The match is running.
    InMatch,
    /// The lobby is paused because the host is gone ([`HostLoss::PauseThenEnd`]).
    HostPaused,
    /// The lobby is over.
    Closed,
}

/// A change only the host may request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostAction {
    /// Select the scenario.
    SetScenario(ContentId),
    /// Ban a component.
    Ban(ContentId),
    /// Lift a ban.
    Unban(ContentId),
    /// Change team grouping.
    SetTeamMode(TeamMode),
    /// Change the late-join policy.
    SetLateJoin(LateJoin),
}

/// A change any member may request for itself. None of these asserts
/// host-owned state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeerRequest {
    /// Change callsign.
    SetCallsign(Callsign),
    /// Pick a team (only in a team mode).
    SetTeam(TeamId),
    /// Pick a loadout; clears readiness.
    SetLoadout(Loadout),
    /// Ready up under the revision the member last saw.
    SetReady {
        /// The revision being acknowledged.
        revision: Revision,
    },
    /// Withdraw readiness.
    Unready,
    /// Say something to the lobby.
    Chat(ChatText),
}

/// Either kind of lobby command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LobbyCommand {
    /// A host-only rules change.
    Host(HostAction),
    /// A member's own request.
    Peer(PeerRequest),
}

/// Why readiness was revoked. Displayed to the member (F55 AC01).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RevokeReason {
    /// The host banned a component the member's loadout carries.
    ComponentBanned {
        /// The banned component.
        component: ContentId,
    },
    /// The host changed the scenario.
    ScenarioChanged,
    /// The host changed team grouping; team picks were cleared.
    TeamsChanged,
    /// The member changed its own loadout, team or callsign.
    SelfEdited,
}

impl RevokeReason {
    /// The stable localization key a UI looks the reason up under.
    pub const fn message_key(&self) -> &'static str {
        match self {
            Self::ComponentBanned { .. } => "lobby.ready_revoked.component_banned",
            Self::ScenarioChanged => "lobby.ready_revoked.scenario_changed",
            Self::TeamsChanged => "lobby.ready_revoked.teams_changed",
            Self::SelfEdited => "lobby.ready_revoked.self_edited",
        }
    }
}

/// One revoked readiness.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadyRevocation {
    /// Whose readiness was revoked.
    pub peer: PeerId,
    /// Why.
    pub reason: RevokeReason,
}

/// What the lobby tells its members. The host sends these reliably; nothing
/// here is client-asserted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LobbyEvent {
    /// The rules changed.
    RulesChanged {
        /// The new revision.
        revision: Revision,
        /// The new digest.
        digest: RulesDigest,
    },
    /// A member's readiness was revoked.
    ReadyRevoked(ReadyRevocation),
    /// A member joined.
    MemberJoined {
        /// Who.
        peer: PeerId,
    },
    /// A member left.
    MemberLeft {
        /// Who.
        peer: PeerId,
    },
    /// A member's readiness was set.
    MemberReady {
        /// Who.
        peer: PeerId,
    },
    /// A chat line.
    Chat {
        /// The sender.
        from: PeerId,
        /// The line.
        text: ChatText,
    },
    /// A launch was cancelled before it committed.
    LaunchCancelled,
    /// The lobby changed phase.
    PhaseChanged(Phase),
}

/// The result of an accepted command: the events to broadcast.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Applied {
    /// The events, in order.
    pub events: Vec<LobbyEvent>,
}

impl Applied {
    /// The readiness revocations among the events.
    pub fn revocations(&self) -> impl Iterator<Item = &ReadyRevocation> {
        self.events.iter().filter_map(|event| match event {
            LobbyEvent::ReadyRevoked(revocation) => Some(revocation),
            _ => None,
        })
    }
}

/// Why a command was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LobbyError {
    /// A host-only action came from a member that is not the host.
    NotHost {
        /// The sender.
        peer: PeerId,
    },
    /// The sender is not a member.
    UnknownPeer {
        /// The sender.
        peer: PeerId,
    },
    /// The command is not allowed in this phase.
    WrongPhase {
        /// The current phase.
        phase: Phase,
    },
    /// The callsign is empty, too long, padded or has control characters.
    InvalidCallsign,
    /// Another member holds the callsign.
    CallsignTaken,
    /// The chat line is empty, too long or has control characters.
    InvalidChat,
    /// The id is not a component kind.
    NotAComponent {
        /// The offered id.
        id: ContentId,
    },
    /// The id is not a multiplayer scenario.
    NotAScenario {
        /// The offered id.
        id: ContentId,
    },
    /// The ban list is full.
    TooManyBans,
    /// The team mode declares fewer than 2 or more than [`MAX_TEAMS`] teams.
    InvalidTeamMode,
    /// The team does not exist in the current mode.
    TeamOutOfRange,
    /// The lobby has no teams, so a team cannot be picked.
    NoTeams,
    /// The member must pick a loadout first.
    NoLoadout,
    /// The member must pick a team first.
    NoTeam,
    /// The loadout is not acceptable.
    LoadoutRejected(LoadoutProblem),
    /// The member acknowledged a rules revision that is no longer current.
    StaleRevision {
        /// The live revision.
        current: Revision,
        /// What the member sent.
        found: Revision,
    },
}

impl fmt::Display for LobbyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotHost { peer } => write!(f, "{peer} is not the host"),
            Self::UnknownPeer { peer } => write!(f, "{peer} is not in the lobby"),
            Self::WrongPhase { phase } => write!(f, "not allowed while the lobby is {phase:?}"),
            Self::InvalidCallsign => f.write_str("invalid callsign"),
            Self::CallsignTaken => f.write_str("callsign already taken"),
            Self::InvalidChat => f.write_str("invalid chat line"),
            Self::NotAComponent { id } => write!(f, "{} is not a component", id.as_str()),
            Self::NotAScenario { id } => write!(f, "{} is not a scenario", id.as_str()),
            Self::TooManyBans => write!(f, "more than {MAX_BANNED_COMPONENTS} banned components"),
            Self::InvalidTeamMode => f.write_str("invalid team count"),
            Self::TeamOutOfRange => f.write_str("no such team"),
            Self::NoTeams => f.write_str("this lobby has no teams"),
            Self::NoLoadout => f.write_str("pick a loadout first"),
            Self::NoTeam => f.write_str("pick a team first"),
            Self::LoadoutRejected(problem) => write!(f, "loadout rejected: {problem:?}"),
            Self::StaleRevision { current, found } => write!(
                f,
                "acknowledged revision {} but the rules are at {}",
                found.get(),
                current.get()
            ),
        }
    }
}

impl std::error::Error for LobbyError {}

/// A join request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JoinRequest {
    /// The joining client's handshake offer.
    pub hello: ClientHello,
    /// The desired callsign.
    pub callsign: Callsign,
    /// The password offered, if any.
    pub password: Option<Secret>,
}

/// Why a join was refused. The three the spec names are distinct variants.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JoinError {
    /// The password is missing or wrong.
    WrongPassword,
    /// The lobby is at its member cap.
    LobbyFull {
        /// The cap.
        max: usize,
    },
    /// Protocol, rules, content or mods differ; the handshake reason says
    /// which.
    ContentMismatch(HandshakeReject),
    /// The match is running and late join is closed.
    LateJoinClosed,
    /// The lobby is not accepting anyone (launching, paused or closed).
    NotAccepting {
        /// The phase.
        phase: Phase,
    },
    /// Another member holds the callsign.
    CallsignTaken,
}

/// The launch order the host sends once every member acknowledged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchOrder {
    /// The session being launched.
    pub session: SessionId,
    /// The rules revision it is bound to.
    pub revision: Revision,
    /// The digest of those rules.
    pub digest: RulesDigest,
    /// The members, with their loadouts and teams.
    pub members: BTreeMap<PeerId, (Callsign, Option<TeamId>, Loadout)>,
}

/// A host's launch request: bound to the rules it believes are in force.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LaunchRequest {
    /// The rules revision.
    pub revision: Revision,
    /// The rules digest.
    pub digest: RulesDigest,
}

/// Why a launch was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaunchError {
    /// The sender is not the host.
    NotHost {
        /// The sender.
        peer: PeerId,
    },
    /// The request names a rules revision that is not the live one.
    StaleRevision {
        /// The live revision.
        current: Revision,
        /// What the request named.
        found: Revision,
    },
    /// The revision matches but the digest does not.
    DigestMismatch,
    /// These members are not ready under the live revision.
    NotReady {
        /// Who.
        peers: Vec<PeerId>,
    },
    /// A launch needs at least one member.
    Empty,
    /// The lobby is not gathering.
    WrongPhase {
        /// The current phase.
        phase: Phase,
    },
    /// The acknowledgment names another revision or digest than the pending
    /// launch.
    AckMismatch,
    /// No launch is pending.
    NoLaunchPending,
}

/// What an acknowledgment did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaunchProgress {
    /// Still waiting on these members.
    Waiting {
        /// Who has not acknowledged.
        pending: Vec<PeerId>,
    },
    /// Everyone acknowledged: the launch committed atomically.
    Committed(LaunchOrder),
}

/// What losing a member did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Departure {
    /// A non-host member left; the events to broadcast.
    MemberLeft(Applied),
    /// The host is gone and the policy closed the lobby.
    LobbyClosed,
    /// The host is gone and the lobby paused for up to `ticks` ticks.
    LobbyPaused {
        /// The grace period.
        ticks: u32,
    },
}

/// The host's authoritative lobby record.
#[derive(Debug)]
pub struct Lobby {
    session: SessionId,
    host: PeerId,
    params: SessionParameters,
    admission: Admission,
    rules: LobbyRules,
    revision: Revision,
    members: BTreeMap<PeerId, Member>,
    phase: Phase,
    pending_acks: BTreeSet<PeerId>,
    allocator: PeerAllocator,
}

impl Lobby {
    /// Opens a lobby with the host as its first member.
    ///
    /// # Errors
    ///
    /// [`LobbyError::InvalidTeamMode`], [`LobbyError::NotAScenario`],
    /// [`LobbyError::NotAComponent`] or [`LobbyError::TooManyBans`] for
    /// invalid starting rules, and [`LobbyError::WrongPhase`] when the member
    /// cap leaves no room for the host.
    pub fn open(
        session: SessionId,
        params: SessionParameters,
        admission: Admission,
        rules: LobbyRules,
        host_callsign: Callsign,
    ) -> Result<Self, LobbyError> {
        validate_rules(&rules)?;
        if admission.max_members == 0 || admission.max_members > crate::bounds::MAX_SESSION_PEERS {
            return Err(LobbyError::WrongPhase {
                phase: Phase::Closed,
            });
        }
        let mut allocator = PeerAllocator::new();
        let host = allocator
            .allocate()
            .map_err(|PeerAllocError::Full| LobbyError::WrongPhase {
                phase: Phase::Closed,
            })?;
        let mut members = BTreeMap::new();
        members.insert(host, new_member(host_callsign));
        Ok(Self {
            session,
            host,
            params,
            admission,
            rules,
            revision: Revision::FIRST,
            members,
            phase: Phase::Gathering,
            pending_acks: BTreeSet::new(),
            allocator,
        })
    }

    /// The host's peer id.
    pub fn host(&self) -> PeerId {
        self.host
    }
    /// The session epoch.
    pub fn session(&self) -> SessionId {
        self.session
    }
    /// The live rules.
    pub fn rules(&self) -> &LobbyRules {
        &self.rules
    }
    /// The live rules revision.
    pub fn revision(&self) -> Revision {
        self.revision
    }
    /// The live rules digest.
    pub fn digest(&self) -> RulesDigest {
        self.rules.digest()
    }
    /// The phase.
    pub fn phase(&self) -> Phase {
        self.phase
    }
    /// One member.
    pub fn member(&self, peer: PeerId) -> Option<&Member> {
        self.members.get(&peer)
    }
    /// Every member, in peer order.
    pub fn members(&self) -> impl Iterator<Item = (PeerId, &Member)> {
        self.members.iter().map(|(peer, member)| (*peer, member))
    }

    /// Admits a joining client, or says exactly why not.
    ///
    /// Checks run password, content, phase/late-join, capacity, callsign;
    /// nothing is allocated unless every check passed, so a refused client
    /// spends no peer id.
    ///
    /// # Errors
    ///
    /// A [`JoinError`].
    pub fn admit(&mut self, request: JoinRequest) -> Result<(PeerId, Applied), JoinError> {
        if let AccessPolicy::Password(expected) = &self.admission.access
            && request.password.as_ref() != Some(expected)
        {
            return Err(JoinError::WrongPassword);
        }
        evaluate_hello(&self.params, &request.hello).map_err(JoinError::ContentMismatch)?;
        match self.phase {
            Phase::Gathering => {}
            Phase::InMatch if self.rules.late_join == LateJoin::Open => {}
            Phase::InMatch => return Err(JoinError::LateJoinClosed),
            phase => return Err(JoinError::NotAccepting { phase }),
        }
        if self.members.len() >= self.admission.max_members {
            return Err(JoinError::LobbyFull {
                max: self.admission.max_members,
            });
        }
        if self.callsign_taken(&request.callsign, None) {
            return Err(JoinError::CallsignTaken);
        }
        let peer =
            self.allocator
                .allocate()
                .map_err(|PeerAllocError::Full| JoinError::LobbyFull {
                    max: crate::bounds::MAX_SESSION_PEERS,
                })?;
        self.members.insert(peer, new_member(request.callsign));
        Ok((
            peer,
            Applied {
                events: vec![LobbyEvent::MemberJoined { peer }],
            },
        ))
    }

    /// Applies one command from `from`. Authority is checked before anything
    /// else; a refused command changes nothing.
    ///
    /// # Errors
    ///
    /// A [`LobbyError`].
    pub fn apply(
        &mut self,
        from: PeerId,
        command: LobbyCommand,
        validator: &dyn LoadoutValidator,
    ) -> Result<Applied, LobbyError> {
        if !self.members.contains_key(&from) {
            return Err(LobbyError::UnknownPeer { peer: from });
        }
        match command {
            LobbyCommand::Host(action) => {
                if from != self.host {
                    return Err(LobbyError::NotHost { peer: from });
                }
                self.require_gathering()?;
                self.apply_host(action)
            }
            LobbyCommand::Peer(request) => {
                if !matches!(request, PeerRequest::Chat(_)) {
                    self.require_gathering()?;
                }
                self.apply_peer(from, request, validator)
            }
        }
    }

    fn require_gathering(&self) -> Result<(), LobbyError> {
        if self.phase == Phase::Gathering {
            Ok(())
        } else {
            Err(LobbyError::WrongPhase { phase: self.phase })
        }
    }

    fn callsign_taken(&self, callsign: &Callsign, except: Option<PeerId>) -> bool {
        self.members
            .iter()
            .any(|(peer, member)| Some(*peer) != except && &member.callsign == callsign)
    }

    fn apply_host(&mut self, action: HostAction) -> Result<Applied, LobbyError> {
        let mut next = self.rules.clone();
        let mut revoked: Vec<ReadyRevocation> = Vec::new();
        let mut clear_teams = false;
        let revoke_everyone: Option<RevokeReason>;
        match action {
            HostAction::SetScenario(scenario) => {
                if scenario.kind() != ContentKind::MultiplayerScenario {
                    return Err(LobbyError::NotAScenario { id: scenario });
                }
                revoke_everyone =
                    (scenario != next.scenario).then_some(RevokeReason::ScenarioChanged);
                next.scenario = scenario;
            }
            HostAction::Ban(component) => {
                if !is_component_kind(component.kind()) {
                    return Err(LobbyError::NotAComponent { id: component });
                }
                if !next.banned.contains(&component) && next.banned.len() >= MAX_BANNED_COMPONENTS {
                    return Err(LobbyError::TooManyBans);
                }
                if next.banned.insert(component.clone()) {
                    for (peer, member) in &self.members {
                        let carries = member
                            .loadout
                            .as_ref()
                            .is_some_and(|loadout| loadout_holds(loadout, &component));
                        if carries && matches!(member.ready, Ready::Ready { .. }) {
                            revoked.push(ReadyRevocation {
                                peer: *peer,
                                reason: RevokeReason::ComponentBanned {
                                    component: component.clone(),
                                },
                            });
                        }
                    }
                }
                revoke_everyone = None;
            }
            HostAction::Unban(component) => {
                next.banned.remove(&component);
                revoke_everyone = None;
            }
            HostAction::SetTeamMode(mode) => {
                validate_team_mode(mode)?;
                if mode != next.team_mode {
                    clear_teams = true;
                    revoke_everyone = Some(RevokeReason::TeamsChanged);
                } else {
                    revoke_everyone = None;
                }
                next.team_mode = mode;
            }
            HostAction::SetLateJoin(policy) => {
                next.late_join = policy;
                revoke_everyone = None;
            }
        }
        if next == self.rules {
            return Ok(Applied { events: Vec::new() });
        }
        let previous = self.revision;
        self.revision = previous.next();
        self.rules = next;
        if clear_teams {
            for member in self.members.values_mut() {
                member.team = None;
            }
        }
        if let Some(reason) = revoke_everyone {
            for (peer, member) in &self.members {
                if matches!(member.ready, Ready::Ready { .. }) {
                    revoked.push(ReadyRevocation {
                        peer: *peer,
                        reason: reason.clone(),
                    });
                }
            }
        }
        let revoked_peers: BTreeSet<PeerId> = revoked.iter().map(|r| r.peer).collect();
        let revision = self.revision;
        for (peer, member) in &mut self.members {
            if revoked_peers.contains(peer) {
                member.ready = Ready::NotReady;
            } else if let Ready::Ready { revision: held } = &mut member.ready {
                // A ban or unban cannot invalidate this member's loadout, so
                // its readiness is carried to the new revision.
                debug_assert_eq!(*held, previous);
                *held = revision;
            }
        }
        let mut events = vec![LobbyEvent::RulesChanged {
            revision,
            digest: self.rules.digest(),
        }];
        events.extend(revoked.into_iter().map(LobbyEvent::ReadyRevoked));
        Ok(Applied { events })
    }

    fn apply_peer(
        &mut self,
        from: PeerId,
        request: PeerRequest,
        validator: &dyn LoadoutValidator,
    ) -> Result<Applied, LobbyError> {
        let mut events = Vec::new();
        match request {
            PeerRequest::Chat(text) => events.push(LobbyEvent::Chat { from, text }),
            PeerRequest::SetCallsign(callsign) => {
                if self.callsign_taken(&callsign, Some(from)) {
                    return Err(LobbyError::CallsignTaken);
                }
                self.members.get_mut(&from).expect("checked").callsign = callsign;
            }
            PeerRequest::SetTeam(team) => {
                let TeamMode::Teams { teams } = self.rules.team_mode else {
                    return Err(LobbyError::NoTeams);
                };
                if team.0 >= teams {
                    return Err(LobbyError::TeamOutOfRange);
                }
                let member = self.members.get_mut(&from).expect("checked");
                member.team = Some(team);
                revoke_self(from, member, &mut events);
            }
            PeerRequest::SetLoadout(loadout) => {
                check_loadout(&self.rules, &loadout, validator)?;
                let member = self.members.get_mut(&from).expect("checked");
                member.loadout = Some(loadout);
                revoke_self(from, member, &mut events);
            }
            PeerRequest::SetReady { revision } => {
                if revision != self.revision {
                    return Err(LobbyError::StaleRevision {
                        current: self.revision,
                        found: revision,
                    });
                }
                let member = self.members.get(&from).expect("checked");
                let loadout = member.loadout.as_ref().ok_or(LobbyError::NoLoadout)?;
                check_loadout(&self.rules, loadout, validator)?;
                if matches!(self.rules.team_mode, TeamMode::Teams { .. }) && member.team.is_none() {
                    return Err(LobbyError::NoTeam);
                }
                self.members.get_mut(&from).expect("checked").ready = Ready::Ready { revision };
                events.push(LobbyEvent::MemberReady { peer: from });
            }
            PeerRequest::Unready => {
                self.members.get_mut(&from).expect("checked").ready = Ready::NotReady;
            }
        }
        Ok(Applied { events })
    }

    /// The host asks to launch. Refused unless it names the live revision and
    /// digest and every member is ready under that revision. On success the
    /// lobby locks (rules, loadouts and readiness stop changing) and waits for
    /// every other member's [`Lobby::acknowledge_launch`].
    ///
    /// # Errors
    ///
    /// A [`LaunchError`]; a refused request changes nothing.
    pub fn begin_launch(
        &mut self,
        from: PeerId,
        request: LaunchRequest,
    ) -> Result<LaunchProgress, LaunchError> {
        if from != self.host {
            return Err(LaunchError::NotHost { peer: from });
        }
        if self.phase != Phase::Gathering {
            return Err(LaunchError::WrongPhase { phase: self.phase });
        }
        if request.revision != self.revision {
            return Err(LaunchError::StaleRevision {
                current: self.revision,
                found: request.revision,
            });
        }
        if request.digest != self.rules.digest() {
            return Err(LaunchError::DigestMismatch);
        }
        if self.members.is_empty() {
            return Err(LaunchError::Empty);
        }
        let not_ready: Vec<PeerId> = self
            .members
            .iter()
            .filter(|(_, member)| {
                member.ready
                    != Ready::Ready {
                        revision: self.revision,
                    }
            })
            .map(|(peer, _)| *peer)
            .collect();
        if !not_ready.is_empty() {
            return Err(LaunchError::NotReady { peers: not_ready });
        }
        self.pending_acks = self
            .members
            .keys()
            .copied()
            .filter(|p| *p != self.host)
            .collect();
        self.phase = Phase::Launching;
        Ok(self.launch_progress())
    }

    /// A member acknowledges the pending launch under the revision and digest
    /// it holds. The last acknowledgment commits the launch.
    ///
    /// # Errors
    ///
    /// [`LaunchError::NoLaunchPending`] or [`LaunchError::AckMismatch`].
    pub fn acknowledge_launch(
        &mut self,
        from: PeerId,
        ack: LaunchRequest,
    ) -> Result<LaunchProgress, LaunchError> {
        if self.phase != Phase::Launching {
            return Err(LaunchError::NoLaunchPending);
        }
        if ack.revision != self.revision || ack.digest != self.rules.digest() {
            return Err(LaunchError::AckMismatch);
        }
        self.pending_acks.remove(&from);
        Ok(self.launch_progress())
    }

    fn launch_progress(&mut self) -> LaunchProgress {
        if !self.pending_acks.is_empty() {
            return LaunchProgress::Waiting {
                pending: self.pending_acks.iter().copied().collect(),
            };
        }
        self.phase = Phase::InMatch;
        let members = self
            .members
            .iter()
            .filter_map(|(peer, member)| {
                member
                    .loadout
                    .clone()
                    .map(|loadout| (*peer, (member.callsign.clone(), member.team, loadout)))
            })
            .collect();
        LaunchProgress::Committed(LaunchOrder {
            session: self.session,
            revision: self.revision,
            digest: self.rules.digest(),
            members,
        })
    }

    /// The host cancels a pending launch; readiness is kept.
    ///
    /// # Errors
    ///
    /// [`LaunchError::NotHost`] or [`LaunchError::NoLaunchPending`].
    pub fn cancel_launch(&mut self, from: PeerId) -> Result<Applied, LaunchError> {
        if from != self.host {
            return Err(LaunchError::NotHost { peer: from });
        }
        if self.phase != Phase::Launching {
            return Err(LaunchError::NoLaunchPending);
        }
        self.phase = Phase::Gathering;
        self.pending_acks.clear();
        Ok(Applied {
            events: vec![LobbyEvent::LaunchCancelled],
        })
    }

    /// The match ended: the lobby returns to gathering with everyone unready
    /// (F55 AC04: back to the lobby without restarting).
    ///
    /// # Errors
    ///
    /// [`LobbyError::WrongPhase`] unless a match is running.
    pub fn finish_match(&mut self) -> Result<Applied, LobbyError> {
        if self.phase != Phase::InMatch {
            return Err(LobbyError::WrongPhase { phase: self.phase });
        }
        self.phase = Phase::Gathering;
        for member in self.members.values_mut() {
            member.ready = Ready::NotReady;
        }
        Ok(Applied {
            events: vec![LobbyEvent::PhaseChanged(Phase::Gathering)],
        })
    }

    /// A member disconnects or leaves. A pending launch is cancelled; the host
    /// leaving follows [`HostLoss`].
    pub fn remove_peer(&mut self, peer: PeerId) -> Option<Departure> {
        self.members.remove(&peer)?;
        if peer == self.host {
            return Some(match self.admission.host_loss {
                HostLoss::EndSession => {
                    self.phase = Phase::Closed;
                    Departure::LobbyClosed
                }
                HostLoss::PauseThenEnd { ticks } => {
                    self.phase = Phase::HostPaused;
                    Departure::LobbyPaused { ticks }
                }
            });
        }
        let mut events = vec![LobbyEvent::MemberLeft { peer }];
        self.pending_acks.remove(&peer);
        if self.phase == Phase::Launching {
            self.phase = Phase::Gathering;
            self.pending_acks.clear();
            events.push(LobbyEvent::LaunchCancelled);
        }
        Some(Departure::MemberLeft(Applied { events }))
    }
}

fn new_member(callsign: Callsign) -> Member {
    Member {
        callsign,
        team: None,
        loadout: None,
        ready: Ready::NotReady,
    }
}

fn revoke_self(peer: PeerId, member: &mut Member, events: &mut Vec<LobbyEvent>) {
    if matches!(member.ready, Ready::Ready { .. }) {
        member.ready = Ready::NotReady;
        events.push(LobbyEvent::ReadyRevoked(ReadyRevocation {
            peer,
            reason: RevokeReason::SelfEdited,
        }));
    }
}

fn loadout_holds(loadout: &Loadout, component: &ContentId) -> bool {
    loadout.blueprint == *component || loadout.components.contains(component)
}

fn validate_team_mode(mode: TeamMode) -> Result<(), LobbyError> {
    match mode {
        TeamMode::Teams { teams } if !(2..=MAX_TEAMS).contains(&teams) => {
            Err(LobbyError::InvalidTeamMode)
        }
        _ => Ok(()),
    }
}

fn validate_rules(rules: &LobbyRules) -> Result<(), LobbyError> {
    if rules.scenario.kind() != ContentKind::MultiplayerScenario {
        return Err(LobbyError::NotAScenario {
            id: rules.scenario.clone(),
        });
    }
    if rules.banned.len() > MAX_BANNED_COMPONENTS {
        return Err(LobbyError::TooManyBans);
    }
    if let Some(id) = rules.banned.iter().find(|id| !is_component_kind(id.kind())) {
        return Err(LobbyError::NotAComponent { id: id.clone() });
    }
    validate_team_mode(rules.team_mode)
}

/// The lobby's own loadout judgement: shape, then the host's bans, then the
/// shared validator.
fn check_loadout(
    rules: &LobbyRules,
    loadout: &Loadout,
    validator: &dyn LoadoutValidator,
) -> Result<(), LobbyError> {
    let unique: BTreeSet<&ContentId> = loadout.components.iter().collect();
    if loadout.components.len() > MAX_LOADOUT_COMPONENTS
        || unique.len() != loadout.components.len()
        || loadout.blueprint.kind() != ContentKind::Blueprint
        || loadout
            .components
            .iter()
            .any(|c| !is_component_kind(c.kind()))
    {
        return Err(LobbyError::LoadoutRejected(LoadoutProblem::Malformed));
    }
    if let Some(component) = rules
        .banned
        .iter()
        .find(|banned| loadout_holds(loadout, banned))
    {
        return Err(LobbyError::LoadoutRejected(LoadoutProblem::Banned {
            component: component.clone(),
        }));
    }
    validator
        .validate(loadout)
        .map_err(|code| LobbyError::LoadoutRejected(LoadoutProblem::Invalid { code }))
}

/// The hello a joiner built from its own signature, at this build's protocol.
pub fn hello_for(compatibility: Compatibility) -> ClientHello {
    ClientHello {
        protocol: PROTOCOL_VERSION,
        compatibility,
    }
}
