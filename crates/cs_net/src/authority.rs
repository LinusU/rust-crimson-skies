//! The authoritative ownership table (F54-A).
//!
//! `docs/contracts/UI-NETWORK.md`, "Network ownership table": "Server owns
//! ActorId allocation, physics truth, weapon acceptance, hit/damage,
//! faction/interaction, mission program, score and result. Client owns only
//! local input requests, UI and predicted cosmetics."
//!
//! This module is that table as data: every [`AuthorityDomain`] names its
//! [`Owner`]. The table is enforced structurally by the wire vocabulary in
//! [`crate::message`]: spawn, snapshot, score and outcome payloads exist only
//! as [`crate::message::ServerPayload`] variants, so a client message cannot
//! express them at all, and [`crate::message::SessionMessage::verify_origin`]
//! refuses a packet that arrives from the wrong side of the session.

/// Which side of a session owns a domain's truth.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Owner {
    /// The host/server: its word is authoritative and its packets carry the
    /// domain.
    Server,
    /// The client: the domain is local-only and its packets may carry it
    /// only as a request, never as truth.
    Client,
}

/// One entry of the contract's ownership table: a domain of the session
/// whose truth exactly one side owns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AuthorityDomain {
    /// Which actors exist and which serials they get
    /// ([`cs_types::net::ActorAllocator`]).
    ActorAllocation,
    /// Simulated pose/velocity truth carried by snapshots.
    PhysicsTruth,
    /// Whether a fire intent becomes a shot.
    WeaponAcceptance,
    /// Hit resolution and damage application.
    HitDamage,
    /// Faction assignment and actor interactions.
    FactionInteraction,
    /// The mission program's progress and phase.
    MissionProgram,
    /// Score and match result.
    ScoreResult,
    /// Lobby rules revisions and host options (F55).
    LobbyRules,
    /// Session membership: who is joined or gone.
    SessionMembership,
    /// A client's own tick-stamped input *requests* — never state.
    LocalInput,
    /// A client's local UI.
    UiState,
    /// A client's predicted cosmetics; never damage or rewards.
    PredictedCosmetics,
}

impl AuthorityDomain {
    /// Every domain, in a stable order.
    pub const ALL: &'static [AuthorityDomain] = &[
        Self::ActorAllocation,
        Self::PhysicsTruth,
        Self::WeaponAcceptance,
        Self::HitDamage,
        Self::FactionInteraction,
        Self::MissionProgram,
        Self::ScoreResult,
        Self::LobbyRules,
        Self::SessionMembership,
        Self::LocalInput,
        Self::UiState,
        Self::PredictedCosmetics,
    ];

    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::ActorAllocation => "actor_allocation",
            Self::PhysicsTruth => "physics_truth",
            Self::WeaponAcceptance => "weapon_acceptance",
            Self::HitDamage => "hit_damage",
            Self::FactionInteraction => "faction_interaction",
            Self::MissionProgram => "mission_program",
            Self::ScoreResult => "score_result",
            Self::LobbyRules => "lobby_rules",
            Self::SessionMembership => "session_membership",
            Self::LocalInput => "local_input",
            Self::UiState => "ui_state",
            Self::PredictedCosmetics => "predicted_cosmetics",
        }
    }

    /// Who owns the domain's truth, per the contract table.
    pub const fn owner(self) -> Owner {
        match self {
            Self::ActorAllocation
            | Self::PhysicsTruth
            | Self::WeaponAcceptance
            | Self::HitDamage
            | Self::FactionInteraction
            | Self::MissionProgram
            | Self::ScoreResult
            | Self::LobbyRules
            | Self::SessionMembership => Owner::Server,
            Self::LocalInput | Self::UiState | Self::PredictedCosmetics => Owner::Client,
        }
    }
}
