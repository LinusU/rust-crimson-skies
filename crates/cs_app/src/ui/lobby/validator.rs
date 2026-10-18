//! The production `LoadoutValidator` the lobby judges members' loadouts with
//! (F55-BU2).
//!
//! Spec: `specs/F55-multiplayer-lobby-host-rules-readiness-and-ux.md`; shared
//! contract `docs/contracts/UI-NETWORK.md` ("Lobby host changes rules through a
//! validated server action"). This is the seam its finding recorded: the hook
//! `cs_net::lobby::LoadoutValidator` had only test doubles (`AcceptAll` /
//! `RejectAll`), because `cs_net` cannot depend on the content crates and the
//! shared validator needs an authored [`AircraftBlueprint`] a flat loadout id
//! list does not carry.
//!
//! [`ConstructionLoadoutValidator`] is the thin adapter: it holds the
//! [`LobbyConstructionCatalog`] the application built from its authored
//! construction content and delegates the whole judgement to
//! [`LobbyConstructionCatalog::judge`] in `cs_content::lobby_loadout`, which documents
//! the mapping and the stable reason codes. Nothing is judged here, so the
//! rule the lobby enforces and the rule the construction screen enforces are
//! the same [`ConstructionRules::validate`] (F44-B).
//!
//! The catalog is *injected*, never invented: the application supplies the
//! authored blueprints, their airframes' rule profiles, the pairing/ban policy
//! and the price book. Until an application supplies a blueprint table
//! populated from the original installation, a lobby wired to this validator
//! refuses a blueprint it does not hold rather than accepting it unjudged
//! (see `cs_content::lobby_loadout`).
//!
//! The lobby names fitments in construction's vocabulary (owner decision,
//! 2026-10-17: `cs_net::lobby::is_component_kind` accepts `Weapon`), so the
//! fixture blueprint's own component set reaches this judgement. A loadout that
//! names lobby `Gun`/`Ammo` ids cannot equal an authored blueprint's fitments
//! and is judged `loadout_mismatch`. Provisioning the authored blueprint table
//! from an installation is F44-D's unmeasured work; nothing here claims retail
//! blueprints.

use cs_content::lobby_loadout::LobbyConstructionCatalog;
use cs_net::lobby::{Loadout, LoadoutValidator};

/// The production lobby loadout validator over `cs_content` construction.
///
/// It implements [`LoadoutValidator`] by delegating to
/// [`LobbyConstructionCatalog::judge`], so a refused loadout reaches the lobby
/// as `LoadoutProblem::Invalid { code }` with one of the content-side stable
/// codes.
#[derive(Debug)]
pub struct ConstructionLoadoutValidator {
    catalog: LobbyConstructionCatalog,
}

impl ConstructionLoadoutValidator {
    /// Wraps the authored construction catalog a lobby judges loadouts with.
    #[must_use]
    pub const fn new(catalog: LobbyConstructionCatalog) -> Self {
        Self { catalog }
    }

    /// The authored catalog every loadout is judged against.
    #[must_use]
    pub const fn catalog(&self) -> &LobbyConstructionCatalog {
        &self.catalog
    }
}

impl LoadoutValidator for ConstructionLoadoutValidator {
    fn validate(&self, loadout: &Loadout) -> Result<(), &'static str> {
        self.catalog.judge(&loadout.blueprint, &loadout.components)
    }
}
