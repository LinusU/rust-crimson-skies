//! Acceptance stage F45-A: the front-end state table
//! (`specs/F45-main-menu-pandora-cabin-briefing-and-flight-check.md`, section
//! `### F45-A`).
//!
//! The minimum scenario is AC01's shape — a fresh profile to first flight and
//! back, through buttons only — plus AC02's shape (Cancel/Back at each
//! preflight screen asks the domain for nothing), AC03's data half (a failed
//! load keeps the selection and retries without a restart) and the contract's
//! hotspot rule. Everything is authored synthetic data; no original screen,
//! coordinate or file is read, so this proves the table and its machine only,
//! never the original front end (F45-B/D).
//!
//! Acceptance stage F45-B (`mod screens`) adds the original-asset screens:
//! a validated `ScreenDeck` of artwork and hotspots presented by
//! `ScreenSession`, and AC02's minimum scenario driven through that authored
//! art. Its fixture data is authored for the same
//! reason as F45-A's: no original front-end layout is decoded anywhere in this
//! repository (see
//! `docs/findings/2026-10-07-f45-b-original-asset-screen-decks.md`), so the
//! original screens themselves are F45-D's capture.
//!
//! Acceptance stage F45-C (`mod wiring`) adds the flows themselves: the
//! `FrontEndFlow` that runs every transition's domain transaction *before*
//! the machine moves, the resource ledger that consumes the acquire/release
//! stream, and the loading screen's real `LoadingSession` over a content
//! session — AC03's minimum scenario, a load whose dependency is missing,
//! fails, is repaired and is retried in the same process.
//!
//! Acceptance stage F45-D (`mod stage_d`) adds the end-to-end review itself:
//! AC04's navigation half (every row of the table applied on the real machine,
//! every screen reached or named) and its capture half (a screen drawn on a
//! real GPU, and every screen-capable original image of the installation
//! measured, decoded and captured). The `retail` and `gpu` tests there are
//! `#[ignore]`d because CI has neither; the evidence report this stage writes
//! is `mod evidence`, deliberately **not** named `accept_f45_d_*`.
//!
//! Task F45-B.1 (#742, `mod original`) reads the original front end's own
//! layout (`ASSETS/LAYOUT.CSV` in `crimson.rof`) into the F45-B deck and
//! reports, per screen, what the original data supports and what it refuses;
//! its evidence report is `mod evidence_f45_b_1`, deliberately **not** named
//! `accept_f45_b_1_*`.
//!
//! Follow-up stage F49-C2 (#776, `mod instant_action`) wires the Instant Action
//! screens into the table and `FrontEndFlow`: four front-end screens mirroring
//! `IA_TABLE`, entered from the main menu and left for it, with the flow's
//! `BeginSession`/`EndSession` effects consumed by the resource ledger. Its
//! prefix is `accept_f49_c_2_`.
//!
//! Follow-up stage F45-C.2 (#748, `mod flight_check_options`) gives the flight
//! check's wingmate and ammunition slots the consumer F45-C left them: the
//! committed selection opens the session's F33 identity roster through
//! `cs_app::roster::open_roster`, with the authored slot's aircraft and
//! equipment taken from the flight check and the pilot, voice and survivability
//! left authored. Its prefix is `accept_f45_c_2_` (so `accept_f45_c_` still
//! selects it).
//!
//! Follow-up stage F55-CU1 (#787, `mod lobby`) wires the F55 lobby screen into
//! the table and `FrontEndFlow`: `Screen::Lobby` reached from the main menu's
//! `OpenLobby` and left by `Back`, the screen and the link it drives held by
//! the domain, and the application-supplied inputs (session epoch,
//! compatibility signature, resolved endpoint, client hello, loadout
//! validator) the forms submit with. Its prefix is `accept_f55_cu1_`.
//!
//! Task FE-MENU-1 (#1833, `mod fe_menu_1`) adds the windowed front-end
//! shell: `cs --cs-path <retail> --front-end` presents one `LAYOUT.CSV`
//! section's panes and 4-state buttons from the original art, applies only
//! the effects the data states, and its `--smoke-seconds` variant drives a
//! scripted capture run. Its prefix is `accept_fe_menu_1_`; the retail and
//! windowed tests are `#[ignore]`d because CI has no installation and no
//! GPU.
//!
//! Task FE-MENU-2 (#1834, `mod fe_menu_2`) adds the main menu's own
//! composition: the `M` row's `CrimFlag.MPG` looping under the panes, the
//! logo and button frame, the six buttons' `ResID` label record and their
//! presses routed through the F45 machine's start flow. Its prefix is
//! `accept_fe_menu_2_`; the retail and windowed tests are `#[ignore]`d for
//! the same reason as FE-MENU-1's.
//!
//! Task FE-MENU-3 (#1835, `mod fe_menu_3`) adds the Campaign pilot page:
//! the `E`/`L` rows' edit box and list, typing, arrow and pointer
//! selection, Continue / Delete / Cancel over the profile session, and the
//! machine's `ProfileSelect` rows. Its prefix is `accept_fe_menu_3_`; the
//! retail and windowed tests are `#[ignore]`d for the same reason.
//!
//! Task FE-MENU-4 (#1836, `mod fe_menu_4`) adds the front end's music loop
//! and rollover / press / text-entry cues, read from `GLOBALS.SCRIPT`'s
//! `sound` declarations. Its prefix is `accept_fe_menu_4_`; the retail and
//! windowed tests are `#[ignore]`d for the same reason.

mod evidence;
mod evidence_f45_b_1;
mod evidence_f45_c_5;
mod evidence_fe_menu_1;
mod evidence_fe_menu_2;
mod evidence_fe_menu_3;
mod evidence_fe_menu_4;
mod evidence_parity5_main_menu;
mod evidence_parity5_start_flow_04;
mod fe_menu_1;
mod fe_menu_2;
mod fe_menu_3;
mod fe_menu_4;
mod flight_check_options;
mod flow;
/// The import dialog's fixture, read through F64-C's own designed fixture
/// construction (`tests/f64_c_support/`), so the navigation walk and the
/// F64-C acceptance suite describe one source rather than two.
use crate::f64_c_support as import_fixture;
mod instant_action;
mod layout;
mod lobby;
mod m01_lc_fc_a;
mod original;
mod parity2_flight_check_page;
mod parity5_cabin_intro_01;
mod parity5_cabin_intro_02;
mod parity5_cabin_intro_03;
mod parity5_cabin_intro_04;
mod parity5_cabin_intro_05;
mod parity5_cabin_intro_06;
mod parity5_main_menu;
mod parity5_start_flow_02;
mod parity5_start_flow_03;
mod parity5_start_flow_04;
mod pause_map;
mod pause_map_preferences;
mod pause_map_presenter;
mod pause_map_restart;
mod preferences_overlay;
mod scrapbook_dispatch;
mod screens;
mod stage_d;
mod table;
mod wiring;

use cs_types::content::{ContentId, ContentKind};

/// A synthetic content id.
pub fn id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("test id")
}
