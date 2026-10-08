//! F55-C acceptance: the join/host/team/loadout/chat lobby screen.
//!
//! Minimum scenario (spec F55 AC03): wrong password, full lobby and content
//! mismatch return **distinct** errors. The rest pins the stage table's
//! Back/Cancel paths, field bounds before a request is built, the host /
//! team / loadout / chat path travelling to the lobby and its events coming
//! back to the display, and teardown plus retry.
//!
//! Everything here calls production code: [`LobbyScreen`] and [`LobbyLink`]
//! in `cs_app::ui::lobby`, and `Lobby::admit` / `Lobby::apply` /
//! `Lobby::remove_peer` behind them.

use std::collections::BTreeSet;

use cs_app::ui::lobby::screen::MAX_TARGET_BYTES;
use cs_app::ui::lobby::{
    Back, Field, FieldProblem, LobbyLink, LobbyScreen, LobbyUiError, Notice, Stage,
};
use cs_net::compat::HandshakeReject;
use cs_net::fixture::{SYNTHETIC_SESSION, synthetic_hello, synthetic_parameters};
use cs_net::lobby::{
    Departure, JoinError, Loadout, LoadoutValidator, LobbyError, Ready, TeamId, TeamMode,
};
use cs_types::content::{ContentId, ContentKind};
use cs_types::evidence::ContentHash;

/// The shared validator the lobby asks about budgets. `cs_net` cannot depend
/// on the content crates and no production implementation exists yet
/// (#764), so the link *requires* one to be supplied instead of assuming a
/// permissive default.
struct Accept;
impl LoadoutValidator for Accept {
    fn validate(&self, _: &Loadout) -> Result<(), &'static str> {
        Ok(())
    }
}

fn id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).unwrap()
}

fn scenario() -> ContentId {
    id(ContentKind::MultiplayerScenario, "synthetic_duel")
}

fn blueprint() -> Loadout {
    Loadout {
        blueprint: id(ContentKind::Blueprint, "synthetic_zephyr"),
        components: Vec::new(),
    }
}

/// Opens a lobby through the host form and returns the screen standing in
/// its room with the link it opened.
fn host_session(
    password: Option<&str>,
    max_members: usize,
    team_mode: TeamMode,
) -> (LobbyScreen, LobbyLink) {
    let mut screen = LobbyScreen::new();
    screen.show_host().unwrap();
    screen.set_scenario(scenario()).unwrap();
    screen.set_host_callsign("Host").unwrap();
    screen.set_max_members(max_members).unwrap();
    screen.set_team_mode(team_mode).unwrap();
    if let Some(text) = password {
        screen.set_host_password(text).unwrap();
    }
    let link = screen
        .submit_host(SYNTHETIC_SESSION, synthetic_parameters(), Box::new(Accept))
        .expect("the host form opens a lobby");
    assert_eq!(screen.stage(), Stage::Room);
    assert_eq!(screen.local(), Some(link.host_peer()));
    (screen, link)
}

/// Fills the join form without submitting it.
fn joiner(target: &str, callsign: &str, password: &str) -> LobbyScreen {
    let mut screen = LobbyScreen::new();
    screen.set_target(target).unwrap();
    screen.set_callsign(callsign).unwrap();
    screen.set_password(password).unwrap();
    screen
}

const TARGET: &str = "192.0.2.10:7777";

#[test]
fn accept_f55_c_wrong_password_full_lobby_and_content_mismatch_return_distinct_errors() {
    let (host, mut link) = host_session(Some("hunter2"), 2, TeamMode::FreeForAll);
    let hello = synthetic_hello();

    // 1. Wrong password: refused before anything else.
    let mut wrong = joiner(TARGET, "Ace", "nope");
    let wrong_password = wrong.submit_join(&mut link, &hello).unwrap_err();
    assert_eq!(wrong_password, LobbyUiError::Join(JoinError::WrongPassword));
    assert_eq!(wrong.stage(), Stage::Join);
    assert_eq!(wrong.local(), None);
    assert_eq!(
        link.lobby().members().count(),
        1,
        "a refused join allocates no member"
    );

    // 2. Content mismatch: the password is right, the signature is not.
    let mut mismatched = joiner(TARGET, "Ace", "hunter2");
    let mut foreign = synthetic_hello();
    foreign.compatibility.content_sha256 = ContentHash::from_bytes([1; 32]);
    let content_mismatch = mismatched.submit_join(&mut link, &foreign).unwrap_err();
    assert!(matches!(
        content_mismatch,
        LobbyUiError::Join(JoinError::ContentMismatch(
            HandshakeReject::ContentMismatch { .. }
        ))
    ));
    assert_eq!(mismatched.stage(), Stage::Join);
    assert_eq!(link.lobby().members().count(), 1);

    // 3. The lobby fills up: the host holds one of the two seats.
    let mut seated = joiner(TARGET, "Ace", "hunter2");
    let ace = seated
        .submit_join(&mut link, &hello)
        .expect("the seat is free");
    assert_eq!(link.lobby().members().count(), 2);

    // 4. Full lobby: the right password and the right signature, refused
    //    anyway because there is no seat left.
    let mut turned_away = joiner(TARGET, "Bee", "hunter2");
    let lobby_full = turned_away.submit_join(&mut link, &hello).unwrap_err();
    assert_eq!(
        lobby_full,
        LobbyUiError::Join(JoinError::LobbyFull { max: 2 })
    );
    assert_eq!(turned_away.stage(), Stage::Join);
    assert_eq!(
        link.lobby().members().count(),
        2,
        "a full lobby admits nobody"
    );

    // The three refusals are three distinct errors, by key and by line.
    let refusals = [&wrong_password, &content_mismatch, &lobby_full];
    let keys: BTreeSet<&'static str> = refusals.iter().map(|error| error.message_key()).collect();
    assert_eq!(
        keys,
        BTreeSet::from([
            "lobby.join.wrong_password",
            "lobby.join.content_mismatch",
            "lobby.join.full",
        ])
    );
    let lines: BTreeSet<String> = refusals.iter().map(|error| error.to_string()).collect();
    assert_eq!(lines.len(), 3, "three refusals must render three lines");

    // Each screen keeps its own refusal for display; an accepted action
    // clears it.
    assert_eq!(wrong.refusal(), Some(&wrong_password));
    assert_eq!(turned_away.refusal(), Some(&lobby_full));
    assert_eq!(seated.refusal(), None);
    assert_eq!(host.refusal(), None);
    assert!(ace.get() > link.host_peer().get());
}

#[test]
fn accept_f55_c_the_join_form_refuses_a_bad_entry_before_any_request() {
    let (_host, mut link) = host_session(None, 4, TeamMode::FreeForAll);
    let hello = synthetic_hello();

    // Length and control-character bounds belong to the form.
    let mut screen = LobbyScreen::new();
    assert_eq!(
        screen.set_target(&"a".repeat(MAX_TARGET_BYTES + 1)),
        Err(LobbyUiError::Field {
            field: Field::Target,
            problem: FieldProblem::TooLong {
                max: MAX_TARGET_BYTES
            },
        })
    );
    assert_eq!(
        screen.set_callsign("two\nlines"),
        Err(LobbyUiError::Field {
            field: Field::Callsign,
            problem: FieldProblem::Invalid,
        })
    );
    assert_eq!(
        screen.join_draft().target,
        "",
        "a refused entry is not stored"
    );

    // An empty address stops the submit before the endpoint is asked
    // anything.
    screen.set_target("   ").unwrap();
    let empty_target = screen.submit_join(&mut link, &hello).unwrap_err();
    assert_eq!(
        empty_target,
        LobbyUiError::Field {
            field: Field::Target,
            problem: FieldProblem::Empty,
        }
    );

    // A callsign the domain refuses does too.
    screen.set_target(TARGET).unwrap();
    screen.set_callsign(" Ace").unwrap();
    let bad_callsign = screen.submit_join(&mut link, &hello).unwrap_err();
    assert_eq!(
        bad_callsign,
        LobbyUiError::Field {
            field: Field::Callsign,
            problem: FieldProblem::Invalid,
        }
    );
    assert_eq!(
        link.lobby().members().count(),
        1,
        "nothing reached the lobby"
    );
    assert_eq!(screen.stage(), Stage::Join);

    // The host form has the same discipline: nothing is invented for it.
    let mut host_form = LobbyScreen::new();
    host_form.show_host().unwrap();
    assert_eq!(
        host_form
            .submit_host(SYNTHETIC_SESSION, synthetic_parameters(), Box::new(Accept))
            .unwrap_err(),
        LobbyUiError::Field {
            field: Field::Scenario,
            problem: FieldProblem::Missing,
        }
    );
    host_form.set_scenario(scenario()).unwrap();
    assert_eq!(
        host_form
            .submit_host(SYNTHETIC_SESSION, synthetic_parameters(), Box::new(Accept))
            .unwrap_err(),
        LobbyUiError::Field {
            field: Field::MaxMembers,
            problem: FieldProblem::Missing,
        }
    );
    assert_eq!(
        host_form.set_max_members(0),
        Err(LobbyUiError::Field {
            field: Field::MaxMembers,
            problem: FieldProblem::OutOfRange { min: 1, max: 32 },
        })
    );
    assert_eq!(
        host_form.set_scenario(id(ContentKind::Mission, "not_a_scenario")),
        Err(LobbyUiError::Field {
            field: Field::Scenario,
            problem: FieldProblem::Invalid,
        })
    );
    assert_eq!(
        host_form.stage(),
        Stage::Host,
        "no form error moves the screen"
    );
}

#[test]
fn accept_f55_c_team_loadout_chat_and_a_host_ban_travel_to_the_lobby_and_back() {
    let (mut host, mut link) = host_session(None, 4, TeamMode::Teams { teams: 2 });
    let hello = synthetic_hello();

    let mut ace = joiner(TARGET, "Ace", "");
    let ace_peer = ace.submit_join(&mut link, &hello).unwrap();
    let host_peer = host.local().expect("the host is in its own room");
    let cannon = id(ContentKind::Gun, "synthetic_cannon");

    // Loadout, team and readiness are requests; the lobby answers with the
    // events each screen displays.
    let loadout_events = host
        .set_loadout(
            &mut link,
            Loadout {
                blueprint: id(ContentKind::Blueprint, "synthetic_zephyr"),
                components: vec![id(ContentKind::Gun, "synthetic_mg")],
            },
        )
        .unwrap();
    assert!(
        loadout_events.is_empty(),
        "a loadout change broadcasts nothing on its own"
    );
    assert!(host.set_team(&mut link, TeamId(0)).unwrap().is_empty());
    assert_eq!(
        host.set_ready(&mut link).unwrap().len(),
        1,
        "readiness is announced"
    );

    assert!(
        ace.set_loadout(
            &mut link,
            Loadout {
                blueprint: id(ContentKind::Blueprint, "synthetic_zephyr"),
                components: vec![cannon.clone()],
            },
        )
        .unwrap()
        .is_empty()
    );
    assert!(ace.set_team(&mut link, TeamId(1)).unwrap().is_empty());
    assert_eq!(ace.set_ready(&mut link).unwrap().len(), 1);

    // Chat: plain bounded text in, literal text out — markup is never
    // interpreted.
    let chat = ace
        .send_chat(&mut link, "<b>ready</b> & waiting")
        .expect("a member may chat while gathering");
    assert_eq!(chat.len(), 1);
    for event in &chat {
        host.observe(event);
    }
    let line = host
        .notices()
        .iter()
        .find_map(|notice| match notice {
            Notice::Chat { from, text } => Some((*from, text.as_str().to_owned())),
            _ => None,
        })
        .expect("the chat line is displayed");
    assert_eq!(line.0, ace_peer);
    assert_eq!(line.1, "<b>ready</b> & waiting");
    let rendered = host.notices()[host.notices().len() - 1].fallback_text();
    assert_eq!(rendered, "peer 2: <b>ready</b> & waiting");
    assert!(
        !rendered.contains("&lt;b&gt;"),
        "the screen shows the literal text, not an escaped form of it"
    );

    // Mute is client-local and never sent.
    host.mute(ace_peer);
    assert!(host.is_muted(ace_peer));
    let shown = |screen: &LobbyScreen| {
        screen
            .notices()
            .iter()
            .filter(|notice| matches!(notice, Notice::Chat { from, .. } if *from == ace_peer))
            .count()
    };
    let before_mute = shown(&host);
    assert_eq!(before_mute, 1);
    for event in ace.send_chat(&mut link, "still there?").unwrap() {
        host.observe(&event);
    }
    assert_eq!(
        shown(&host),
        before_mute,
        "a muted peer's chat is not displayed"
    );
    host.unmute(ace_peer);
    for event in ace.send_chat(&mut link, "and again").unwrap() {
        host.observe(&event);
    }
    assert_eq!(shown(&host), before_mute + 1, "unmuting shows it again");

    // F55 AC01 through the screen: the host bans the weapon a ready member
    // carries, the readiness is revoked, and both screens show why.
    let banned = host
        .ban(&mut link, cannon.clone())
        .expect("the host may ban a component");
    assert_eq!(
        banned.len(),
        2,
        "the rules changed and the readiness was revoked"
    );
    for event in &banned {
        ace.observe(event);
    }
    let revoked = ace
        .notices()
        .iter()
        .find_map(|notice| match notice {
            Notice::ReadyRevoked { peer, reason } => Some((*peer, reason.clone(), notice)),
            _ => None,
        })
        .expect("the revocation is displayed to the revoked member");
    assert_eq!(revoked.0, ace_peer);
    assert!(revoked.2.fallback_text().contains("synthetic_cannon"));
    assert_eq!(
        link.lobby().member(ace_peer).unwrap().ready,
        Ready::NotReady,
        "the lobby really revoked it"
    );
    assert_eq!(
        link.lobby().revision().get(),
        2,
        "the ban moved the rules revision"
    );

    // The authority check is the lobby's, not the screen's: a member cannot
    // ban, and the screen reports the lobby's own refusal.
    let refused = ace.ban(&mut link, cannon).unwrap_err();
    assert!(matches!(
        refused,
        LobbyUiError::Domain(LobbyError::NotHost { .. })
    ));
    assert_eq!(link.lobby().rules().banned.len(), 1);

    assert_eq!(host.local(), Some(host_peer));
    assert_eq!(host.stage(), Stage::Room);
}

#[test]
fn accept_f55_c_leaving_tears_the_screen_down_and_a_corrected_form_retries() {
    let (mut host, mut link) = host_session(Some("hunter2"), 2, TeamMode::FreeForAll);
    let hello = synthetic_hello();

    // A refused join keeps the form, so it can be corrected.
    let mut screen = joiner(TARGET, "Ace", "wrong");
    let refused = screen.submit_join(&mut link, &hello).unwrap_err();
    assert_eq!(refused, LobbyUiError::Join(JoinError::WrongPassword));
    assert_eq!(screen.join_draft().password, "wrong");

    screen.set_password("hunter2").unwrap();
    let peer = screen
        .submit_join(&mut link, &hello)
        .expect("the corrected form joins");
    assert_eq!(
        screen.refusal(),
        None,
        "an accepted action clears the refusal"
    );
    assert_eq!(link.lobby().members().count(), 2);

    // Teardown: the member leaves cleanly and the screen is back on the
    // join form with its identity gone, the lobby really one member short,
    // and the room's notices reset to what the departure itself says.
    let departure = screen.leave(&mut link).unwrap();
    assert!(matches!(departure, Some(Departure::MemberLeft(_))));
    assert_eq!(screen.stage(), Stage::Join);
    assert_eq!(screen.local(), None);
    assert_eq!(link.lobby().members().count(), 1);
    assert_eq!(
        link.lobby().member(peer),
        None,
        "the lobby really lost the member"
    );
    assert_eq!(
        screen.join_draft().callsign,
        "Ace",
        "the form survives for a retry"
    );
    assert!(
        screen
            .notices()
            .iter()
            .any(|notice| notice.fallback_text().contains("left"))
    );

    // Retry on the same link.
    let again = screen
        .submit_join(&mut link, &hello)
        .expect("the retry joins");
    assert_eq!(
        again.get(),
        peer.get() + 1,
        "the lobby allocates a fresh peer id"
    );

    // Back inside the room tears the room down first...
    assert_eq!(screen.stage(), Stage::Room);
    let back = screen.back(&mut link).unwrap();
    assert!(matches!(back, Back::LeftRoom(Some(_))));
    assert_eq!(screen.stage(), Stage::Join);

    // ...and Back on a form leaves the lobby screen, discarding the draft.
    screen.set_target(TARGET).unwrap();
    let exit = screen.back(&mut link).unwrap();
    assert_eq!(exit, Back::Exit);
    assert_eq!(screen.join_draft().target, "", "Back discards the draft");
    assert_eq!(screen.stage(), Stage::Join);

    // The host tearing itself down follows the declared host-loss policy.
    let departure = host.leave(&mut link).unwrap();
    assert_eq!(departure, Some(Departure::LobbyClosed));
    assert_eq!(host.stage(), Stage::Join);
    assert_eq!(host.local(), None);
}

#[test]
fn accept_f55_c_the_stage_table_refuses_every_action_outside_its_stage() {
    let (mut host, mut link) = host_session(None, 4, TeamMode::FreeForAll);
    let hello = synthetic_hello();
    let cannon = id(ContentKind::Gun, "synthetic_cannon");

    // Join stage: the forms and the join submit belong here, room actions
    // do not.
    let mut screen = LobbyScreen::new();
    assert_eq!(screen.focus(), Field::Target);
    let refusals = [
        ("set_ready", screen.set_ready(&mut link).map(|_| ())),
        ("set_unready", screen.set_unready(&mut link).map(|_| ())),
        (
            "set_loadout",
            screen.set_loadout(&mut link, blueprint()).map(|_| ()),
        ),
        (
            "set_team",
            screen.set_team(&mut link, TeamId(0)).map(|_| ()),
        ),
        ("send_chat", screen.send_chat(&mut link, "hi").map(|_| ())),
        (
            "host_action",
            screen.ban(&mut link, cannon.clone()).map(|_| ()),
        ),
        ("leave", screen.leave(&mut link).map(|_| ())),
        (
            "submit_host",
            screen
                .submit_host(SYNTHETIC_SESSION, synthetic_parameters(), Box::new(Accept))
                .map(|_| ()),
        ),
    ];
    for (name, outcome) in refusals {
        let error = outcome.unwrap_err();
        assert!(
            matches!(
                &error,
                LobbyUiError::WrongStage {
                    stage: Stage::Join,
                    action
                } if *action == name
            ),
            "{name} must be refused on the join form, got {error:?}"
        );
    }
    assert_eq!(
        screen.focus_on(Field::Chat),
        Err(LobbyUiError::WrongStage {
            stage: Stage::Join,
            action: "focus",
        }),
        "a control of another stage cannot hold focus"
    );

    // Host stage: the join submit is refused here, and entering a stage
    // resets the focus to its first control.
    screen.show_host().unwrap();
    assert_eq!(screen.focus(), Field::Scenario);
    assert_eq!(
        screen.submit_join(&mut link, &hello),
        Err(LobbyUiError::WrongStage {
            stage: Stage::Host,
            action: "submit_join",
        })
    );
    assert_eq!(
        screen.set_ready(&mut link).unwrap_err().to_string(),
        "set_ready is not available while the screen is Host"
    );

    // Room: both entry forms and both submits are refused; the way out is
    // Back, which tears the room down.
    assert_eq!(
        host.show_join(),
        Err(LobbyUiError::WrongStage {
            stage: Stage::Room,
            action: "show_join",
        })
    );
    assert_eq!(
        host.show_host(),
        Err(LobbyUiError::WrongStage {
            stage: Stage::Room,
            action: "show_host",
        })
    );

    let mut joined = joiner(TARGET, "Ace", "");
    joined.submit_join(&mut link, &hello).unwrap();
    assert_eq!(
        joined.submit_join(&mut link, &hello),
        Err(LobbyUiError::WrongStage {
            stage: Stage::Room,
            action: "submit_join",
        })
    );
    assert_eq!(
        joined
            .submit_host(SYNTHETIC_SESSION, synthetic_parameters(), Box::new(Accept))
            .unwrap_err(),
        LobbyUiError::WrongStage {
            stage: Stage::Room,
            action: "submit_host",
        }
    );
    assert_eq!(joined.focus(), Field::Chat, "focus follows the stage");
    joined.move_focus(true);
    assert_eq!(joined.focus(), Field::Chat, "one control wraps onto itself");

    // A room action on the screen that is in the room works.
    assert_eq!(joined.send_chat(&mut link, "here").unwrap().len(), 1);

    // Switching forms is allowed before connecting and keeps both drafts.
    joined.back(&mut link).unwrap();
    joined.show_host().unwrap();
    joined.set_host_callsign("Host").unwrap();
    joined.show_join().unwrap();
    assert_eq!(
        joined.host_draft().host_callsign,
        "Host",
        "switching forms does not discard the other form"
    );
    assert_eq!(joined.join_draft().target, TARGET);
}
