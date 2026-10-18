//! F55-BU2 acceptance: the production `ConstructionLoadoutValidator` wired into
//! the lobby host form.
//!
//! Owner decision 2026-10-17 (task #764): the lobby names fitments in
//! construction's vocabulary. The fixture blueprint's own component set must
//! therefore reach the content judgement (accepted, or a named
//! `LoadoutProblem::Invalid { code }`) instead of `LoadoutRejected(Malformed)`,
//! and a lobby `Gun`-kind id, which no authored blueprint fits, is judged
//! `loadout_mismatch`. Everything here goes through [`LobbyScreen::submit_host`]
//! and [`LobbyLink`] with the production validator, not a test double.

use std::collections::BTreeSet;

use cs_app::ui::lobby::LobbyUiError;
use cs_app::ui::lobby::{ConstructionLoadoutValidator, LobbyLink, LobbyScreen};
use cs_content::construction::{
    declared_synthetic_blueprint, declared_synthetic_price_book, synthetic_boundary_rules,
    synthetic_policy,
};
use cs_content::lobby_loadout::{LOADOUT_MISMATCH, LobbyConstructionCatalog, UNKNOWN_BLUEPRINT};
use cs_net::fixture::{SYNTHETIC_SESSION, synthetic_parameters};
use cs_net::lobby::{Loadout, LoadoutProblem, LobbyError, TeamMode};
use cs_types::content::{ContentId, ContentKind};

fn host() -> (LobbyScreen, LobbyLink) {
    let catalog = LobbyConstructionCatalog::new(
        vec![declared_synthetic_blueprint()],
        vec![synthetic_boundary_rules()],
        synthetic_policy(),
        declared_synthetic_price_book(),
    );
    let mut screen = LobbyScreen::new();
    screen.show_host().unwrap();
    screen
        .set_scenario(
            ContentId::from_source(ContentKind::MultiplayerScenario, "synthetic_duel").unwrap(),
        )
        .unwrap();
    screen.set_host_callsign("Host").unwrap();
    screen.set_max_members(4).unwrap();
    screen.set_team_mode(TeamMode::FreeForAll).unwrap();
    let link = screen
        .submit_host(
            SYNTHETIC_SESSION,
            synthetic_parameters(),
            Box::new(ConstructionLoadoutValidator::new(catalog)),
        )
        .expect("the host form opens a lobby");
    (screen, link)
}

fn fixture_loadout() -> Loadout {
    let blueprint = declared_synthetic_blueprint();
    let distinct: BTreeSet<ContentId> = blueprint
        .components()
        .into_iter()
        .map(|(_, component)| component.clone())
        .collect();
    Loadout {
        blueprint: blueprint.id().clone(),
        components: distinct.into_iter().collect(),
    }
}

#[test]
fn accept_f55_bu2_lobby_fixture_blueprint_components_reach_the_content_judgement() {
    let (mut screen, mut link) = host();
    screen
        .set_loadout(&mut link, fixture_loadout())
        .expect("the boundary blueprint's own component set passes shape and content judgement");
}

#[test]
fn accept_f55_bu2_lobby_unknown_blueprint_is_invalid_with_a_stable_code() {
    let (mut screen, mut link) = host();
    let mut loadout = fixture_loadout();
    loadout.blueprint = ContentId::from_source(ContentKind::Blueprint, "not_authored").unwrap();
    assert_eq!(
        screen.set_loadout(&mut link, loadout).unwrap_err(),
        LobbyUiError::Domain(LobbyError::LoadoutRejected(LoadoutProblem::Invalid {
            code: UNKNOWN_BLUEPRINT
        }))
    );
}

#[test]
fn accept_f55_bu2_lobby_gun_kind_id_is_refused_loadout_mismatch() {
    let (mut screen, mut link) = host();
    let mut loadout = fixture_loadout();
    loadout
        .components
        .push(ContentId::from_source(ContentKind::Gun, "synthetic_mg").unwrap());
    assert_eq!(
        screen.set_loadout(&mut link, loadout).unwrap_err(),
        LobbyUiError::Domain(LobbyError::LoadoutRejected(LoadoutProblem::Invalid {
            code: LOADOUT_MISMATCH
        }))
    );
}
