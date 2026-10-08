//! The join / host / team / loadout / chat lobby screen (F55-C).
//!
//! Spec: `specs/F55-multiplayer-lobby-host-rules-readiness-and-ux.md`, stage
//! `### F55-C`. Shared contract: `docs/contracts/UI-NETWORK.md` ("Use an
//! explicit state table for every screen and Back/Cancel path. A UI action
//! requests a domain transaction; it does not directly edit ... fields",
//! "Dropdown selection is a content id, never a transient row number",
//! "Focus and accessibility state reset sensibly on entry").
//!
//! The screen owns exactly what a screen may own: which stage it is on
//! ([`Stage`]), the form drafts, the local focus and the display projection
//! ([`LobbyView`]). Everything that changes a lobby is requested through a
//! [`LobbyLink`] and answered by [`cs_net::lobby::Lobby`]; a refusal comes
//! back as a [`LobbyUiError`] and is kept for display
//! ([`LobbyScreen::refusal`]). Nothing here edits a member, a rule or a
//! readiness directly.
//!
//! ## The stage table
//!
//! | stage | `show_join` | `show_host` | `submit_join` | `submit_host` | room actions | `back` |
//! | --- | --- | --- | --- | --- | --- | --- |
//! | [`Stage::Join`] | keep | switch | submit | refuse | refuse | exit, discard drafts |
//! | [`Stage::Host`] | switch | keep | refuse | submit | refuse | exit, discard drafts |
//! | [`Stage::Room`] | refuse | refuse | refuse | refuse | run | leave the room first |
//!
//! Room actions are `set_team`, `set_loadout`, `set_ready`, `set_unready`,
//! `send_chat`, `host_action` (host only, and the lobby re-checks that) and
//! `leave`. Nothing in a form stage can reach a room action: it is refused
//! with [`LobbyUiError::WrongStage`] before any request is built.
//!
//! ## What is wired and what is not
//!
//! The join form produces a real [`JoinRequest`] and the host's admission
//! answers it, so "wrong password, full lobby and content mismatch return
//! distinct errors" (F55 AC03) is a property of this screen: the three come
//! back as three different [`JoinError`] variants with three different
//! message keys and three different lines ([`LobbyScreen::refusal`] and
//! [`cs_net::lobby::JoinError::message_key`]).
//!
//! The form's *direct address* is validated but **not resolved**: turning an
//! address into an endpoint, and LAN discovery, need the transport
//! (`cs_net::message`/`lifecycle` and a socket), which is outside this
//! task's owner paths; the endpoint behind `submit_join` is handed over by
//! the application and today is a lobby in this process (follow-up #763,
//! `F55-BU1-WIRE`). Internet connectivity is never labelled as discovery or
//! the other way round: [`JoinDraft::target`] is an address, and no
//! matchmaking wording exists anywhere in this module.
//!
//! The lobby screen is also not reachable from the front-end state table
//! yet: `crates/cs_app/src/ui/front_end/` is not an F55 owner path.

use std::collections::BTreeSet;
use std::fmt;

use cs_net::bounds::MAX_SESSION_PEERS;
use cs_net::compat::{ClientHello, SessionParameters};
use cs_net::lobby::{
    AccessPolicy, Admission, Callsign, ChatText, Departure, HostAction, HostLoss, JoinError,
    JoinRequest, LateJoin, Loadout, LoadoutValidator, LobbyCommand, LobbyError, LobbyEvent,
    LobbyRules, MAX_CALLSIGN_CHARS, MAX_CHAT_CHARS, MAX_PASSWORD_BYTES, MAX_TEAMS, PeerRequest,
    Secret, TeamId, TeamMode,
};
use cs_types::content::{ContentId, ContentKind};
use cs_types::net::{PeerId, SessionId};

use super::link::{HostSettings, LobbyLink};
use super::{LobbyView, Notice};

/// The longest direct address the join form holds: a host name or
/// `host:port`. A designed UI bound, not an original one — no original
/// address field is known.
pub const MAX_TARGET_BYTES: usize = 128;

/// Where the lobby screen is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// The join form: direct address, callsign, password.
    Join,
    /// The host form: scenario, callsign, password, member cap.
    Host,
    /// Inside the lobby: team, loadout, readiness, chat, host rules.
    Room,
}

impl Stage {
    /// The controls of this stage, in keyboard/focus order.
    pub const fn fields(self) -> &'static [Field] {
        match self {
            Self::Join => &[Field::Target, Field::Callsign, Field::Password],
            Self::Host => &[
                Field::Scenario,
                Field::Callsign,
                Field::Password,
                Field::MaxMembers,
            ],
            Self::Room => &[Field::Chat],
        }
    }
}

impl Field {
    /// The stable localization key of this control.
    #[must_use]
    pub const fn message_key(self) -> &'static str {
        match self {
            Self::Target => "lobby.field.target",
            Self::Callsign => "lobby.field.callsign",
            Self::Password => "lobby.field.password",
            Self::Chat => "lobby.field.chat",
            Self::Scenario => "lobby.field.scenario",
            Self::MaxMembers => "lobby.field.max_members",
        }
    }
}

/// One focusable control of the current stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    /// The direct address the join form is aimed at.
    Target,
    /// The player's callsign.
    Callsign,
    /// The password (join: what is offered; host: what is required).
    Password,
    /// The chat line.
    Chat,
    /// The scenario the host selected.
    Scenario,
    /// The host's member cap.
    MaxMembers,
}

/// Why a form value is not usable. The bounds a domain type owns are
/// reported by that type's refusal (see [`LobbyUiError::Domain`]); these are
/// the checks the screen itself performs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldProblem {
    /// The entry has not been made yet.
    Missing,
    /// The entry is present but empty.
    Empty,
    /// The entry is longer than the bound.
    TooLong {
        /// The bound, in characters (bytes for [`Field::Password`]).
        max: usize,
    },
    /// The number is outside the accepted range.
    OutOfRange {
        /// The lowest accepted value.
        min: usize,
        /// The highest accepted value.
        max: usize,
    },
    /// The domain's own constructor refused the value.
    Invalid,
}

impl fmt::Display for FieldProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => f.write_str("this entry is required"),
            Self::Empty => f.write_str("this entry must not be empty"),
            Self::TooLong { max } => write!(f, "at most {max}"),
            Self::OutOfRange { min, max } => write!(f, "between {min} and {max}"),
            Self::Invalid => f.write_str("not a valid value"),
        }
    }
}

/// Why the screen refused an action. Every refusal is kept for display
/// ([`LobbyScreen::refusal`]); none of them changes anything.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LobbyUiError {
    /// The action does not belong to the stage the screen is on: the state
    /// table's own refusal.
    WrongStage {
        /// The stage the screen is on.
        stage: Stage,
        /// The refused action.
        action: &'static str,
    },
    /// A form entry the screen validates itself is not usable.
    Field {
        /// Which control.
        field: Field,
        /// Why.
        problem: FieldProblem,
    },
    /// The host refused the join. The three spec cases are three distinct
    /// variants (F55 AC03).
    Join(JoinError),
    /// The lobby refused the command, or a value the lobby owns is invalid.
    Domain(LobbyError),
}

impl LobbyUiError {
    /// The stable localization key of the refusal. Every [`JoinError`] key
    /// is distinct, so the join form's refusals can never collapse into one
    /// line (F55 AC03).
    #[must_use]
    pub fn message_key(&self) -> &'static str {
        match self {
            Self::Join(why) => why.message_key(),
            Self::WrongStage { .. } => "lobby.ui.wrong_stage",
            Self::Field { field, .. } => field.message_key(),
            Self::Domain(_) => "lobby.command.refused",
        }
    }
}

impl fmt::Display for LobbyUiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongStage { stage, action } => {
                write!(f, "{action} is not available while the screen is {stage:?}")
            }
            Self::Field { field, problem } => write!(f, "{field:?}: {problem}"),
            Self::Join(why) => write!(f, "{why}"),
            Self::Domain(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for LobbyUiError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Join(why) => Some(why),
            Self::Domain(why) => Some(why),
            Self::WrongStage { .. } | Self::Field { .. } => None,
        }
    }
}

/// What [`LobbyScreen::back`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Back {
    /// Back inside the room tore the room down first; the departure to
    /// broadcast.
    LeftRoom(Option<Departure>),
    /// Back on a form left the lobby screen entirely and discarded the
    /// drafts, so the caller returns to the menu.
    Exit,
}

/// The join form's contents.
///
/// `target` is the **direct address** the application resolves into an
/// endpoint. The screen validates its shape and never resolves it: address
/// resolution and LAN discovery belong to the transport (see the module
/// doc).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JoinDraft {
    /// The direct address to join.
    pub target: String,
    /// The callsign to join as.
    pub callsign: String,
    /// The password offered, empty when none is offered.
    pub password: String,
}

/// The host form's contents.
///
/// No field carries an original default: the original's lobby options are
/// not measured (F55-D / F56-A). `team_mode`, `late_join` and `host_loss`
/// start at the conservative designed initial state and the host changes
/// them through the form; `scenario`, `host_callsign`, `password` and
/// `max_members` start *unset* and the form refuses to submit until they
/// are entered, rather than inventing a value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostDraft {
    /// The scenario to host, a `multiplayer_scenario` content id.
    pub scenario: Option<ContentId>,
    /// The host's own callsign.
    pub host_callsign: String,
    /// The required password; empty opens the lobby.
    pub password: String,
    /// The member cap including the host; `0` means "not entered yet".
    pub max_members: usize,
    /// The team grouping.
    pub team_mode: TeamMode,
    /// The late-join policy.
    pub late_join: LateJoin,
    /// What a lost host does to the lobby.
    pub host_loss: HostLoss,
    /// Components no loadout may contain.
    pub banned: BTreeSet<ContentId>,
}

impl Default for HostDraft {
    fn default() -> Self {
        Self {
            scenario: None,
            host_callsign: String::new(),
            password: String::new(),
            max_members: 0,
            team_mode: TeamMode::FreeForAll,
            late_join: LateJoin::Closed,
            host_loss: HostLoss::EndSession,
            banned: BTreeSet::new(),
        }
    }
}

/// The lobby screen: stage, drafts, focus, refusals and the display
/// projection of the lobby's events.
#[derive(Debug)]
pub struct LobbyScreen {
    stage: Stage,
    local: Option<PeerId>,
    focus: Field,
    join: JoinDraft,
    host: HostDraft,
    chat: String,
    view: LobbyView,
    refusal: Option<LobbyUiError>,
}

impl Default for LobbyScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl LobbyScreen {
    /// A fresh screen on the join form, focus on its first control.
    #[must_use]
    pub fn new() -> Self {
        Self {
            stage: Stage::Join,
            local: None,
            focus: Field::Target,
            join: JoinDraft::default(),
            host: HostDraft::default(),
            chat: String::new(),
            view: LobbyView::new(),
            refusal: None,
        }
    }

    // ----------------------------------------------------------- display ---

    /// The stage.
    #[must_use]
    pub fn stage(&self) -> Stage {
        self.stage
    }

    /// This player's peer id, once it is in a room.
    #[must_use]
    pub fn local(&self) -> Option<PeerId> {
        self.local
    }

    /// The refusal to display, cleared by the next accepted action.
    #[must_use]
    pub fn refusal(&self) -> Option<&LobbyUiError> {
        self.refusal.as_ref()
    }

    /// The focused control.
    #[must_use]
    pub fn focus(&self) -> Field {
        self.focus
    }

    /// The notices the room shows, oldest first.
    #[must_use]
    pub fn notices(&self) -> &[Notice] {
        self.view.notices()
    }

    /// The whole display projection.
    #[must_use]
    pub fn view(&self) -> &LobbyView {
        &self.view
    }

    /// Feeds one host event into this screen's display. The application
    /// calls this for every event the lobby broadcast, including the ones
    /// another local screen's action produced.
    pub fn observe(&mut self, event: &LobbyEvent) {
        self.view.observe(event);
    }

    /// Stops showing chat from `peer`. Client-local; never sent.
    pub fn mute(&mut self, peer: PeerId) {
        self.view.mute(peer);
    }

    /// Shows chat from `peer` again.
    pub fn unmute(&mut self, peer: PeerId) {
        self.view.unmute(peer);
    }

    /// Whether `peer` is muted.
    #[must_use]
    pub fn is_muted(&self, peer: PeerId) -> bool {
        self.view.is_muted(peer)
    }

    /// The join form's draft.
    #[must_use]
    pub fn join_draft(&self) -> &JoinDraft {
        &self.join
    }

    /// The host form's draft.
    #[must_use]
    pub fn host_draft(&self) -> &HostDraft {
        &self.host
    }

    /// The unsent chat line.
    #[must_use]
    pub fn chat_draft(&self) -> &str {
        &self.chat
    }

    // ------------------------------------------------------ the stage table ---

    /// Shows the join form. Refused inside the room: leave first, so a
    /// player can never drop out of a room by switching forms.
    ///
    /// # Errors
    ///
    /// [`LobbyUiError::WrongStage`] in [`Stage::Room`].
    pub fn show_join(&mut self) -> Result<(), LobbyUiError> {
        if self.stage == Stage::Room {
            return self.record(Err(LobbyUiError::WrongStage {
                stage: Stage::Room,
                action: "show_join",
            }));
        }
        self.stage = Stage::Join;
        self.entered();
        self.record(Ok(()))
    }

    /// Shows the host form. Refused inside the room.
    ///
    /// # Errors
    ///
    /// [`LobbyUiError::WrongStage`] in [`Stage::Room`].
    pub fn show_host(&mut self) -> Result<(), LobbyUiError> {
        if self.stage == Stage::Room {
            return self.record(Err(LobbyUiError::WrongStage {
                stage: Stage::Room,
                action: "show_host",
            }));
        }
        self.stage = Stage::Host;
        self.entered();
        self.record(Ok(()))
    }

    /// Back: inside the room it tears the room down
    /// ([`LobbyScreen::leave`]); on a form it leaves the lobby screen and
    /// discards the drafts.
    ///
    /// # Errors
    ///
    /// As [`LobbyScreen::leave`] when it runs.
    pub fn back(&mut self, link: &mut LobbyLink) -> Result<Back, LobbyUiError> {
        if self.stage == Stage::Room {
            return self.leave(link).map(Back::LeftRoom);
        }
        self.join = JoinDraft::default();
        self.host = HostDraft::default();
        self.view.clear();
        self.stage = Stage::Join;
        self.entered();
        self.record(Ok(Back::Exit))
    }

    /// Moves focus to another control of the current stage.
    ///
    /// # Errors
    ///
    /// [`LobbyUiError::WrongStage`] when the control is not on this stage.
    pub fn focus_on(&mut self, field: Field) -> Result<(), LobbyUiError> {
        if !self.stage.fields().contains(&field) {
            return self.record(Err(LobbyUiError::WrongStage {
                stage: self.stage,
                action: "focus",
            }));
        }
        self.focus = field;
        self.record(Ok(()))
    }

    /// Moves focus to the next (or previous) control of the current stage.
    pub fn move_focus(&mut self, forward: bool) {
        let fields = self.stage.fields();
        let at = fields
            .iter()
            .position(|field| *field == self.focus)
            .unwrap_or(0);
        let next = if forward {
            (at + 1) % fields.len()
        } else {
            (at + fields.len() - 1) % fields.len()
        };
        self.focus = fields[next];
    }

    // ------------------------------------------------------------ the forms ---

    /// Types into the join form's direct address.
    ///
    /// # Errors
    ///
    /// [`LobbyUiError::Field`] when the entry is too long or holds control
    /// characters; the draft is unchanged.
    pub fn set_target(&mut self, text: &str) -> Result<(), LobbyUiError> {
        let outcome = bounded(Field::Target, MAX_TARGET_BYTES, text);
        if outcome.is_ok() {
            self.join.target = text.to_owned();
        }
        self.record(outcome)
    }

    /// Types into the join form's callsign.
    ///
    /// # Errors
    ///
    /// [`LobbyUiError::Field`] when the entry is longer than
    /// [`MAX_CALLSIGN_CHARS`] or holds control characters; the draft is
    /// unchanged.
    pub fn set_callsign(&mut self, text: &str) -> Result<(), LobbyUiError> {
        let outcome = bounded(Field::Callsign, MAX_CALLSIGN_CHARS, text);
        if outcome.is_ok() {
            self.join.callsign = text.to_owned();
        }
        self.record(outcome)
    }

    /// Types into the join form's password.
    ///
    /// # Errors
    ///
    /// [`LobbyUiError::Field`] when the entry is longer than
    /// [`MAX_PASSWORD_BYTES`]; the draft is unchanged.
    pub fn set_password(&mut self, text: &str) -> Result<(), LobbyUiError> {
        let outcome = bounded_bytes(Field::Password, MAX_PASSWORD_BYTES, text);
        if outcome.is_ok() {
            self.join.password = text.to_owned();
        }
        self.record(outcome)
    }

    /// The host form's own callsign. The join form and the host form keep
    /// separate callsign entries: they are separate forms.
    ///
    /// # Errors
    ///
    /// As [`LobbyScreen::set_callsign`].
    pub fn set_host_callsign(&mut self, text: &str) -> Result<(), LobbyUiError> {
        let outcome = bounded(Field::Callsign, MAX_CALLSIGN_CHARS, text);
        if outcome.is_ok() {
            self.host.host_callsign = text.to_owned();
        }
        self.record(outcome)
    }

    /// The host form's required password; empty opens the lobby.
    ///
    /// # Errors
    ///
    /// As [`LobbyScreen::set_password`].
    pub fn set_host_password(&mut self, text: &str) -> Result<(), LobbyUiError> {
        let outcome = bounded_bytes(Field::Password, MAX_PASSWORD_BYTES, text);
        if outcome.is_ok() {
            self.host.password = text.to_owned();
        }
        self.record(outcome)
    }

    /// Selects the scenario to host. A dropdown row is a **content id**,
    /// never a row number.
    ///
    /// # Errors
    ///
    /// [`LobbyUiError::Field`] when the id is not a multiplayer scenario.
    pub fn set_scenario(&mut self, scenario: ContentId) -> Result<(), LobbyUiError> {
        let outcome = if scenario.kind() == ContentKind::MultiplayerScenario {
            Ok(())
        } else {
            Err(LobbyUiError::Field {
                field: Field::Scenario,
                problem: FieldProblem::Invalid,
            })
        };
        if outcome.is_ok() {
            self.host.scenario = Some(scenario);
        }
        self.record(outcome)
    }

    /// Sets the host's member cap, host included.
    ///
    /// # Errors
    ///
    /// [`LobbyUiError::Field`] outside `1..=`[`MAX_SESSION_PEERS`].
    pub fn set_max_members(&mut self, max: usize) -> Result<(), LobbyUiError> {
        let outcome = if (1..=MAX_SESSION_PEERS).contains(&max) {
            Ok(())
        } else {
            Err(LobbyUiError::Field {
                field: Field::MaxMembers,
                problem: FieldProblem::OutOfRange {
                    min: 1,
                    max: MAX_SESSION_PEERS,
                },
            })
        };
        if outcome.is_ok() {
            self.host.max_members = max;
        }
        self.record(outcome)
    }

    /// Sets the team grouping the host requires.
    ///
    /// # Errors
    ///
    /// [`LobbyUiError::Domain`] for a team count the lobby refuses.
    pub fn set_team_mode(&mut self, mode: TeamMode) -> Result<(), LobbyUiError> {
        if let TeamMode::Teams { teams } = mode
            && !(2..=MAX_TEAMS).contains(&teams)
        {
            return self.record(Err(LobbyUiError::Domain(LobbyError::InvalidTeamMode)));
        }
        self.host.team_mode = mode;
        self.record(Ok(()))
    }

    /// Sets the late-join policy the lobby opens with.
    pub fn set_late_join(&mut self, policy: LateJoin) -> Result<(), LobbyUiError> {
        self.host.late_join = policy;
        self.record(Ok(()))
    }

    /// Sets what a lost host does to the lobby.
    pub fn set_host_loss(&mut self, policy: HostLoss) -> Result<(), LobbyUiError> {
        self.host.host_loss = policy;
        self.record(Ok(()))
    }

    /// Types the chat line; it is validated when it is sent.
    ///
    /// # Errors
    ///
    /// [`LobbyUiError::Field`] when the draft would be longer than
    /// [`MAX_CHAT_CHARS`]; the draft is unchanged.
    pub fn set_chat(&mut self, text: &str) -> Result<(), LobbyUiError> {
        let outcome = bounded(Field::Chat, MAX_CHAT_CHARS, text);
        if outcome.is_ok() {
            self.chat = text.to_owned();
        }
        self.record(outcome)
    }

    /// The host form's submit: requests the domain transaction that opens
    /// the lobby and, on success, enters the room as its host.
    ///
    /// The session epoch and the compatibility signature come from the
    /// application — they belong to the transport and the mounted content,
    /// not to the form.
    ///
    /// # Errors
    ///
    /// [`LobbyUiError::WrongStage`] off the host form,
    /// [`LobbyUiError::Field`] for a form entry that is still missing, or
    /// [`LobbyUiError::Domain`] when the lobby refuses the opening. No
    /// lobby is opened.
    pub fn submit_host(
        &mut self,
        session: SessionId,
        params: SessionParameters,
        validator: Box<dyn LoadoutValidator>,
    ) -> Result<LobbyLink, LobbyUiError> {
        if self.stage != Stage::Host {
            return self.record(Err(LobbyUiError::WrongStage {
                stage: self.stage,
                action: "submit_host",
            }));
        }
        let outcome = self.open_host(session, params, validator);
        let link = self.record(outcome)?;
        Ok(link)
    }

    fn open_host(
        &mut self,
        session: SessionId,
        params: SessionParameters,
        validator: Box<dyn LoadoutValidator>,
    ) -> Result<LobbyLink, LobbyUiError> {
        let Some(scenario) = self.host.scenario.clone() else {
            return Err(LobbyUiError::Field {
                field: Field::Scenario,
                problem: FieldProblem::Missing,
            });
        };
        if self.host.max_members == 0 {
            return Err(LobbyUiError::Field {
                field: Field::MaxMembers,
                problem: FieldProblem::Missing,
            });
        }
        let host_callsign =
            Callsign::new(&self.host.host_callsign).map_err(|_| LobbyUiError::Field {
                field: Field::Callsign,
                problem: FieldProblem::Invalid,
            })?;
        let access = if self.host.password.is_empty() {
            AccessPolicy::Open
        } else {
            AccessPolicy::Password(Secret::new(&self.host.password).ok_or(LobbyUiError::Field {
                field: Field::Password,
                problem: FieldProblem::TooLong {
                    max: MAX_PASSWORD_BYTES,
                },
            })?)
        };
        let settings = HostSettings {
            session,
            params,
            admission: Admission {
                access,
                max_members: self.host.max_members,
                host_loss: self.host.host_loss,
            },
            rules: LobbyRules {
                scenario,
                banned: self.host.banned.clone(),
                team_mode: self.host.team_mode,
                late_join: self.host.late_join,
            },
            host_callsign,
        };
        let link = LobbyLink::host(settings, validator).map_err(LobbyUiError::Domain)?;
        self.local = Some(link.host_peer());
        self.stage = Stage::Room;
        self.entered();
        Ok(link)
    }

    /// The join form's submit: offers the draft's callsign and password to
    /// `link`, the endpoint the application resolved from the entered
    /// direct address.
    ///
    /// A refusal leaves the stage, the draft and `link` exactly as they
    /// were, so the form can be corrected and submitted again.
    ///
    /// # Errors
    ///
    /// [`LobbyUiError::WrongStage`] off the join form,
    /// [`LobbyUiError::Field`] for an unusable entry, or
    /// [`LobbyUiError::Join`] with the host's distinct reason.
    pub fn submit_join(
        &mut self,
        link: &mut LobbyLink,
        hello: &ClientHello,
    ) -> Result<PeerId, LobbyUiError> {
        if self.stage != Stage::Join {
            return self.record(Err(LobbyUiError::WrongStage {
                stage: self.stage,
                action: "submit_join",
            }));
        }
        let outcome = self.join_lobby(link, hello);
        let peer = self.record(outcome)?;
        Ok(peer)
    }

    fn join_lobby(
        &mut self,
        link: &mut LobbyLink,
        hello: &ClientHello,
    ) -> Result<PeerId, LobbyUiError> {
        if self.join.target.trim().is_empty() {
            return Err(LobbyUiError::Field {
                field: Field::Target,
                problem: FieldProblem::Empty,
            });
        }
        let callsign = Callsign::new(&self.join.callsign).map_err(|_| LobbyUiError::Field {
            field: Field::Callsign,
            problem: FieldProblem::Invalid,
        })?;
        let password = if self.join.password.is_empty() {
            None
        } else {
            Some(Secret::new(&self.join.password).ok_or(LobbyUiError::Field {
                field: Field::Password,
                problem: FieldProblem::TooLong {
                    max: MAX_PASSWORD_BYTES,
                },
            })?)
        };
        let (peer, applied) = link
            .join(JoinRequest {
                hello: hello.clone(),
                callsign,
                password,
            })
            .map_err(LobbyUiError::Join)?;
        self.observe_all(&applied.events);
        self.local = Some(peer);
        self.stage = Stage::Room;
        self.entered();
        Ok(peer)
    }

    // -------------------------------------------------------- room actions ---

    /// Picks a team.
    ///
    /// # Errors
    ///
    /// [`LobbyUiError::WrongStage`] outside the room, or
    /// [`LobbyUiError::Domain`] as the lobby answers it.
    pub fn set_team(
        &mut self,
        link: &mut LobbyLink,
        team: TeamId,
    ) -> Result<Vec<LobbyEvent>, LobbyUiError> {
        self.request(link, "set_team", PeerRequest::SetTeam(team))
    }

    /// Picks a loadout.
    ///
    /// # Errors
    ///
    /// As [`LobbyScreen::set_team`].
    pub fn set_loadout(
        &mut self,
        link: &mut LobbyLink,
        loadout: Loadout,
    ) -> Result<Vec<LobbyEvent>, LobbyUiError> {
        self.request(link, "set_loadout", PeerRequest::SetLoadout(loadout))
    }

    /// Readies up under the revision the screen last saw.
    ///
    /// # Errors
    ///
    /// As [`LobbyScreen::set_team`].
    pub fn set_ready(&mut self, link: &mut LobbyLink) -> Result<Vec<LobbyEvent>, LobbyUiError> {
        let revision = link.revision();
        self.request(link, "set_ready", PeerRequest::SetReady { revision })
    }

    /// Withdraws readiness.
    ///
    /// # Errors
    ///
    /// As [`LobbyScreen::set_team`].
    pub fn set_unready(&mut self, link: &mut LobbyLink) -> Result<Vec<LobbyEvent>, LobbyUiError> {
        self.request(link, "set_unready", PeerRequest::Unready)
    }

    /// Sends the chat line. Plain bounded text: the lobby owns the bound and
    /// the control-character rule, and the screen never interprets it as
    /// markup.
    ///
    /// # Errors
    ///
    /// [`LobbyUiError::WrongStage`] outside the room,
    /// [`LobbyUiError::Field`] for an empty or invalid line, or
    /// [`LobbyUiError::Domain`] as the lobby answers it.
    pub fn send_chat(
        &mut self,
        link: &mut LobbyLink,
        text: &str,
    ) -> Result<Vec<LobbyEvent>, LobbyUiError> {
        let chat = ChatText::new(text).map_err(|_| LobbyUiError::Field {
            field: Field::Chat,
            problem: FieldProblem::Invalid,
        })?;
        self.request(link, "send_chat", PeerRequest::Chat(chat))
    }

    /// The host changes the rules. The lobby re-checks host authority, so
    /// this is a request, never an edit.
    ///
    /// # Errors
    ///
    /// As [`LobbyScreen::set_team`].
    pub fn host_action(
        &mut self,
        link: &mut LobbyLink,
        action: HostAction,
    ) -> Result<Vec<LobbyEvent>, LobbyUiError> {
        let peer = match self.require_room("host_action") {
            Ok(peer) => peer,
            Err(error) => return self.record(Err(error)),
        };
        let applied = match link.command(peer, LobbyCommand::Host(action)) {
            Ok(applied) => applied,
            Err(error) => return self.record(Err(LobbyUiError::Domain(error))),
        };
        let events = applied.events;
        self.observe_all(&events);
        self.record(Ok(events))
    }

    /// Bans a component (F55 AC01's host action).
    ///
    /// # Errors
    ///
    /// As [`LobbyScreen::host_action`].
    pub fn ban(
        &mut self,
        link: &mut LobbyLink,
        component: ContentId,
    ) -> Result<Vec<LobbyEvent>, LobbyUiError> {
        self.host_action(link, HostAction::Ban(component))
    }

    /// Lifts a ban.
    ///
    /// # Errors
    ///
    /// As [`LobbyScreen::host_action`].
    pub fn unban(
        &mut self,
        link: &mut LobbyLink,
        component: ContentId,
    ) -> Result<Vec<LobbyEvent>, LobbyUiError> {
        self.host_action(link, HostAction::Unban(component))
    }

    fn request(
        &mut self,
        link: &mut LobbyLink,
        action: &'static str,
        request: PeerRequest,
    ) -> Result<Vec<LobbyEvent>, LobbyUiError> {
        let peer = match self.require_room(action) {
            Ok(peer) => peer,
            Err(error) => return self.record(Err(error)),
        };
        let applied = match link.command(peer, LobbyCommand::Peer(request)) {
            Ok(applied) => applied,
            Err(error) => return self.record(Err(LobbyUiError::Domain(error))),
        };
        let events = applied.events;
        self.observe_all(&events);
        self.record(Ok(events))
    }

    /// Leaves the room: this player departs the lobby and the screen is
    /// back on the join form with its transient state cleared. The
    /// departure is returned for the application to broadcast; the notices
    /// are reset to what the departure itself says.
    ///
    /// # Errors
    ///
    /// [`LobbyUiError::WrongStage`] outside the room.
    pub fn leave(&mut self, link: &mut LobbyLink) -> Result<Option<Departure>, LobbyUiError> {
        let outcome = self.depart(link);
        self.record(outcome)
    }

    fn depart(&mut self, link: &mut LobbyLink) -> Result<Option<Departure>, LobbyUiError> {
        let peer = match self.require_room("leave") {
            Ok(peer) => peer,
            Err(error) => return self.record(Err(error)),
        };
        let departure = link.leave(peer);
        self.view.clear();
        if let Some(Departure::MemberLeft(applied)) = &departure {
            let events = applied.events.clone();
            self.observe_all(&events);
        }
        self.local = None;
        self.stage = Stage::Join;
        self.entered();
        Ok(departure)
    }

    // ------------------------------------------------------------- internals ---

    fn require_room(&self, action: &'static str) -> Result<PeerId, LobbyUiError> {
        match (self.stage, self.local) {
            (Stage::Room, Some(peer)) => Ok(peer),
            _ => Err(LobbyUiError::WrongStage {
                stage: self.stage,
                action,
            }),
        }
    }

    /// Entering a stage resets the state that belongs to the stage: the
    /// chat draft and the focus position.
    fn entered(&mut self) {
        self.chat.clear();
        self.focus = self.stage.fields()[0];
    }

    fn observe_all(&mut self, events: &[LobbyEvent]) {
        for event in events {
            self.view.observe(event);
        }
    }

    /// Keeps the last refusal for display, or clears it after an accepted
    /// action.
    fn record<T>(&mut self, outcome: Result<T, LobbyUiError>) -> Result<T, LobbyUiError> {
        self.refusal = match &outcome {
            Ok(_) => None,
            Err(error) => Some(error.clone()),
        };
        outcome
    }
}

/// The screen's own length and control-character check for a text entry.
///
/// # Errors
///
/// [`LobbyUiError::Field`] when the entry is too long or holds a control
/// character.
fn bounded(field: Field, max: usize, text: &str) -> Result<(), LobbyUiError> {
    if text.chars().count() > max {
        return Err(LobbyUiError::Field {
            field,
            problem: FieldProblem::TooLong { max },
        });
    }
    if text.chars().any(char::is_control) {
        return Err(LobbyUiError::Field {
            field,
            problem: FieldProblem::Invalid,
        });
    }
    Ok(())
}

/// The same check against a byte bound (the password's).
///
/// # Errors
///
/// [`LobbyUiError::Field`] as [`bounded`].
fn bounded_bytes(field: Field, max: usize, text: &str) -> Result<(), LobbyUiError> {
    if text.len() > max {
        return Err(LobbyUiError::Field {
            field,
            problem: FieldProblem::TooLong { max },
        });
    }
    if text.chars().any(char::is_control) {
        return Err(LobbyUiError::Field {
            field,
            problem: FieldProblem::Invalid,
        });
    }
    Ok(())
}
