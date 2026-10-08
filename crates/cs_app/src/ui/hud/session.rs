//! The in-flight HUD session (F46-C): which page is up, the local pause it
//! holds, and the read-only projections the cockpit, map, objectives and
//! recon pages draw.
//!
//! [`HudSession`] is the consumer the flight loop drives once per frame. It
//! owns **no** gameplay state — every row it reports is projected from a
//! session authority the caller supplies, so the displayed page can never
//! disagree with the simulation that produced it:
//!
//! * [`MissionPage::Cockpit`] is the F46-B [`HudFrame`] verbatim;
//! * [`MissionPage::Map`] is the authored geography of the load's
//!   [`WorldDefinition`]/[`WorldInstance`] pair, the visible objectives of
//!   the mission's [`ObjectiveDisplay`] and the *revealed* contacts of the
//!   [`TargetStore`] — an unrevealed actor or objective is never a mark, so
//!   the fidelity mode cannot leak it;
//! * [`MissionPage::Objectives`] is [`ObjectiveDisplay::visible`], the
//!   event-driven rows only;
//! * [`MissionPage::Recon`] is the published [`SpyglassReadout`], shown only
//!   while the consumers are bound to this observer.
//!
//! Pause is a **request**, not a state write: [`HudSession::pause`] and
//! [`HudSession::open`] ask for the local pause under the session's
//! [`SessionMode`], and the answer is the same [`PauseDecision`] the input
//! path returns — a networked session reports
//! [`PauseDecision::NoLocalAuthority`] and keeps flying, because the UI can
//! never pause the server. The commands the map page offers are the
//! front-end pause screen's own rows ([`PauseCommand`]).
//!
//! A swap or retry is a binding change, not a reset of the simulation:
//! [`HudSession::teardown`] reports what the old display held and leaves the
//! HUD unbound, [`HudSession::retry`] rebinds to the next generation's actor
//! in one step. Either way the stale-state discipline of [`Hud::frame`]
//! applies unchanged — a sample or authority from the previous generation
//! is refused, never read as an empty page.
//!
//! Everything page-shaped here is **designed**: the original game's map
//! composition, page order, pause-menu rows and whether an in-flight recon
//! page existed at all are unmeasured (F46-D). What is *not* designed is
//! which authorities the pages read — that is fixed by the sessions above.
//! `docs/findings/2026-10-08-f46-c-hud-pages-map-and-pause.md` records the
//! decisions and the unknowns.

use cs_content::world::{WorldDefinition, WorldInstance, WorldObjectCondition, WorldObjectId};
use cs_sim::targeting::{Allegiance, TargetClass, TargetStore};
use cs_types::net::{ActorId, SessionId};
use cs_types::space::{Radians, WorldPosition};

use crate::input::{PauseDecision, PauseReason, SessionMode};
use crate::objectives::{DisplayedObjective, ObjectiveDisplay};
use crate::targeting::{SpyglassReadout, TargetConsumers};

use super::{AircraftSample, Hud, HudError, HudFrame, HudSources, Instruments};

/// Which in-flight view the HUD shows.
///
/// `Cockpit` is flight itself: the instruments and gauges. The other three
/// are the pause surfaces the F46 feature names — the map, the objective
/// list and the recon readout. The set and the order are designed; which
/// pages the original grouped onto one screen, and their artwork, are
/// F46-D's to measure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MissionPage {
    /// The flight instruments and gauges.
    Cockpit,
    /// The mission map: geography, contacts, objectives and pause commands.
    Map,
    /// The objective list.
    Objectives,
    /// The recon readout: what the spyglass would magnify.
    Recon,
}

impl MissionPage {
    /// Every page, in a stable order.
    pub const ALL: &'static [MissionPage] =
        &[Self::Cockpit, Self::Map, Self::Objectives, Self::Recon];

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Cockpit => "cockpit",
            Self::Map => "map",
            Self::Objectives => "objectives",
            Self::Recon => "recon",
        }
    }
}

/// One row of the pause menu the map page offers.
///
/// The rows are the front-end pause screen's own commands
/// (`Resume`, `OpenSettings`, `AbortMission` — its `Back` lands on `Flight`,
/// which is `Resume` again, so the map offers the three distinct
/// statements). Designed ordering; the original menu's exact rows are
/// F46-D's to measure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PauseCommand {
    /// Leave the pause surface and fly.
    Resume,
    /// Open the pause-time settings screen.
    Settings,
    /// Abandon the flight.
    AbortMission,
}

impl PauseCommand {
    /// Every command, in menu order.
    pub const ALL: &'static [PauseCommand] = &[Self::Resume, Self::Settings, Self::AbortMission];

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Resume => "resume",
            Self::Settings => "settings",
            Self::AbortMission => "abort-mission",
        }
    }
}

/// The authorities one in-flight view is projected from.
///
/// [`HudSources`] is the cockpit frame's own set; the map additionally reads
/// the world record and the target roster, and the objective rows come from
/// the mission's display. Each `Option` is an absent authority — the page
/// draws nothing for it — which is a different statement from an authority
/// of the wrong session generation: those are refused.
#[derive(Clone, Copy, Default)]
pub struct MissionSources<'a> {
    /// The cockpit frame's authorities.
    pub hud: HudSources<'a>,
    /// The world definition the mission loads, with the load record that
    /// selects which objects are active.
    pub world: Option<(&'a WorldDefinition, &'a WorldInstance)>,
    /// The mission's objective display.
    pub objectives: Option<&'a ObjectiveDisplay>,
    /// The session's target roster, for the map's contacts.
    pub roster: Option<&'a TargetStore>,
}

/// One authored world object as the map draws it.
///
/// `object` is the record's own stable id and `position` its authored
/// translation — the map never re-derives placement. `condition` is the
/// *load's* initial-damage answer ([`WorldInstance::initial_condition`]): a
/// damaged-in-editor hangar is a different mark from an intact one, and an
/// object the load never activates is not a mark at all.
#[derive(Clone, Debug, PartialEq)]
pub struct GeographyMark {
    /// The world object's stable id.
    pub object: WorldObjectId,
    /// The authored position, canonical meters.
    pub position: [f64; 3],
    /// The condition this load authors it in.
    pub condition: WorldObjectCondition,
}

/// The bound aircraft's own mark on the map.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerMark {
    /// The roster's canonical position for the bound actor.
    pub position: WorldPosition,
    /// The heading the instruments project, `None` when the nose is
    /// vertical and a heading has no meaning.
    pub heading: Option<Radians>,
}

/// One contact the map may show: a roster actor that is **revealed** and
/// still in the world.
///
/// The fidelity gate is structural, not a presenter filter: the projection
/// reads `TargetRecord::revealed` and [`TargetStore::present`], so an
/// unrevealed or ended actor is never in the list to begin with.
/// `allegiance` is the store's own declared relation, `None` when none is
/// declared — an undeclared relation is unknown, never guessed hostile or
/// friendly.
#[derive(Clone, Debug, PartialEq)]
pub struct ContactMark {
    /// The contact's session-qualified identity.
    pub actor: ActorId,
    /// Its canonical position as the roster records it.
    pub position: WorldPosition,
    /// What kind of actor this is.
    pub class: TargetClass,
    /// The observer's declared relation to the contact's faction.
    pub allegiance: Option<Allegiance>,
    /// Whether mission rules flag the actor as an objective target.
    pub objective: bool,
}

/// The map page for one frame: a read-only projection of the session's own
/// records.
#[derive(Clone, Debug, PartialEq)]
pub struct MapView {
    /// The bound aircraft's mark; `None` when no roster was supplied or the
    /// actor is not registered in it — the map draws no player dot rather
    /// than inventing a position.
    pub player: Option<PlayerMark>,
    /// The load's active world objects, in authored order.
    pub geography: Vec<GeographyMark>,
    /// The revealed live contacts, in stable actor order.
    pub contacts: Vec<ContactMark>,
    /// The mission's visible objective rows.
    pub objectives: Vec<DisplayedObjective>,
    /// The pause menu rows this page offers.
    pub commands: &'static [PauseCommand],
    /// The pause the session holds, if any.
    pub pause: Option<PauseReason>,
}

/// What one [`HudSession::view`] produced for the page it is on.
#[derive(Clone, Debug, PartialEq)]
pub enum PageView {
    /// The cockpit frame.
    Cockpit(Box<HudFrame>),
    /// The mission map.
    Map(Box<MapView>),
    /// The visible objectives.
    Objectives(Vec<DisplayedObjective>),
    /// The recon readout, `None` when no consumers were supplied or they are
    /// bound to another observer.
    Recon(Option<SpyglassReadout>),
}

/// What a pause-changing call answered.
///
/// `pause` is the input path's own vocabulary: `NoLocalAuthority` on a
/// networked session means the UI opened its map but the world kept flying.
/// `resumed` is the pause that ended, so a caller that bridges to
/// [`crate::input::InputSession`] can replay the same transition there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageOutcome {
    /// The page the session is on after the call.
    pub page: MissionPage,
    /// What the pause request decided.
    pub pause: PauseDecision,
    /// The pause reason that was cleared, when the call resumed.
    pub resumed: Option<PauseReason>,
}

/// What [`HudSession::teardown`] reports: the state the old display held.
///
/// Like [`crate::objectives::TeardownReport`], this is data, not a
/// narrative: a caller that owns the real sessions uses it to despawn the
/// old generation's presenter state before the next binding lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HudTeardown {
    /// The `(session, actor)` the display was bound to, if any.
    pub bound: Option<(SessionId, ActorId)>,
    /// The page that was up.
    pub page: MissionPage,
    /// The pause that was held.
    pub pause: Option<PauseReason>,
}

/// The in-flight UI session: the bound HUD, the page it is on and the local
/// pause it holds.
///
/// Constructed per flight; a swap rebinds it ([`HudSession::bind`]) and a
/// mission retry moves it to the next generation
/// ([`HudSession::retry`]). It keeps one invariant by construction: a held
/// pause only exists on a menu page — `resume` always lands on the cockpit,
/// so "paused while flying the instruments" is not a state this type can
/// express.
#[derive(Clone, Debug)]
pub struct HudSession {
    hud: Hud,
    mode: SessionMode,
    page: MissionPage,
    pause: Option<PauseReason>,
}

impl HudSession {
    /// A session on the cockpit page under `mode`, with the HUD's own
    /// display policy.
    ///
    /// # Errors
    ///
    /// [`HudError::Policy`] when the policy does not validate.
    pub fn new(mode: SessionMode, policy: cs_content::hud::HudPolicy) -> Result<Self, HudError> {
        Ok(Self {
            hud: Hud::new(policy)?,
            mode,
            page: MissionPage::Cockpit,
            pause: None,
        })
    }

    /// The session mode pause authority was declared under.
    #[must_use]
    pub const fn mode(&self) -> SessionMode {
        self.mode
    }

    /// Binds the display to `actor` in `session` — the aircraft-swap
    /// transition, which drops everything remembered about the previous
    /// aircraft inside [`Hud::bind`].
    ///
    /// # Errors
    ///
    /// [`HudError::ActorSessionMismatch`] when the actor is of another
    /// session.
    pub fn bind(&mut self, session: SessionId, actor: ActorId) -> Result<(), HudError> {
        self.hud.bind(session, actor)
    }

    /// The aircraft the display reads, if any.
    #[must_use]
    pub fn bound(&self) -> Option<(SessionId, ActorId)> {
        self.hud.bound()
    }

    /// The page the session is on.
    #[must_use]
    pub const fn page(&self) -> MissionPage {
        self.page
    }

    /// Whether the local session holds a pause.
    #[must_use]
    pub const fn is_paused(&self) -> bool {
        self.pause.is_some()
    }

    /// Why the session is paused, when it is.
    #[must_use]
    pub const fn pause_reason(&self) -> Option<PauseReason> {
        self.pause
    }

    /// The raw HUD, for callers that drive [`Hud::project`] themselves.
    #[must_use]
    pub const fn hud(&self) -> &Hud {
        &self.hud
    }

    /// The pause command rows a menu page offers — the same list the map
    /// view reports.
    #[must_use]
    pub const fn pause_commands(&self) -> &'static [PauseCommand] {
        PauseCommand::ALL
    }

    /// Opens `page`, requesting the local pause a menu page implies.
    ///
    /// Opening the cockpit is leaving the pause surface, so it resumes.
    /// Opening a menu page asks for `PauseReason::Menu`; the returned
    /// [`PauseDecision`] is the mode's own answer — `NoLocalAuthority` in a
    /// networked session, where the page still opens but nothing pauses.
    pub fn open(&mut self, page: MissionPage) -> PageOutcome {
        if page == MissionPage::Cockpit {
            return self.resume();
        }
        self.page = page;
        PageOutcome {
            page,
            pause: self.hold(PauseReason::Menu),
            resumed: None,
        }
    }

    /// The player's pause request: the map page opens and the local pause
    /// is asked for as `PauseReason::PlayerRequest`.
    ///
    /// The decision is the mode's answer, so on a networked session the map
    /// opens (`NoLocalAuthority`) and the world keeps flying — the request
    /// is never quietly treated as a server pause.
    pub fn pause(&mut self) -> PageOutcome {
        self.page = MissionPage::Map;
        PageOutcome {
            page: MissionPage::Map,
            pause: self.hold(PauseReason::PlayerRequest),
            resumed: None,
        }
    }

    /// Leaves the pause surface: back to the cockpit, releasing the held
    /// pause. `resumed` carries the reason that was held, `None` when the
    /// session was not paused — resuming an unpaused flight is not an
    /// error, it is the same cockpit the front-end's `Back` lands on.
    pub fn resume(&mut self) -> PageOutcome {
        self.page = MissionPage::Cockpit;
        PageOutcome {
            page: MissionPage::Cockpit,
            pause: PauseDecision::Unchanged,
            resumed: self.pause.take(),
        }
    }

    /// The current page's projection for `sample`.
    ///
    /// Every page runs [`Hud::project`]'s bound check first — the view can
    /// only ever describe the bound `(session, actor)`, and a stale sample
    /// or a foreign-generation authority is refused rather than drawn.
    ///
    /// # Errors
    ///
    /// Every error [`Hud::project`] and [`Hud::frame`] can return, plus
    /// [`HudError::WorldMismatch`] when the load record reads a different
    /// definition than the one supplied; on an error nothing is projected
    /// and the session's page and pause are unchanged.
    pub fn view(
        &mut self,
        sample: &AircraftSample,
        sources: &MissionSources<'_>,
    ) -> Result<PageView, HudError> {
        match self.page {
            MissionPage::Cockpit => Ok(PageView::Cockpit(Box::new(
                self.hud.frame(sample, &sources.hud)?,
            ))),
            MissionPage::Map => {
                let instruments = self.hud.project(sample)?;
                Ok(PageView::Map(Box::new(self.map(&instruments, sources)?)))
            }
            MissionPage::Objectives => {
                self.hud.project(sample)?;
                Ok(PageView::Objectives(
                    sources
                        .objectives
                        .map(ObjectiveDisplay::visible)
                        .unwrap_or_default(),
                ))
            }
            MissionPage::Recon => {
                let instruments = self.hud.project(sample)?;
                Ok(PageView::Recon(recon(
                    sources.hud.targets,
                    instruments.actor,
                )))
            }
        }
    }

    /// Ends the display: unbinds the HUD, returns to the cockpit and
    /// releases any held pause, reporting what it gave up.
    ///
    /// The next view refuses `Unbound` until `bind` lands, so a page can
    /// never draw the ended session's aircraft by accident.
    #[must_use]
    pub fn teardown(&mut self) -> HudTeardown {
        let report = HudTeardown {
            bound: self.hud.bound(),
            page: self.page,
            pause: self.pause,
        };
        // The policy already validated once; `Hud::new` cannot refuse it
        // again.
        self.hud = Hud::new(self.hud.policy().clone()).expect("the bound policy validated");
        self.page = MissionPage::Cockpit;
        self.pause = None;
        report
    }

    /// Retears and rebinds in one step: the mission retry the objective
    /// session's own `retry` performs, applied to the display.
    ///
    /// The new binding is checked **before** anything is released — a retry
    /// whose actor does not belong to `session` is refused
    /// ([`HudError::ActorSessionMismatch`]) and the old display is left
    /// exactly as it was.
    ///
    /// # Errors
    ///
    /// [`HudError::ActorSessionMismatch`] when the actor is of another
    /// session.
    pub fn retry(&mut self, session: SessionId, actor: ActorId) -> Result<HudTeardown, HudError> {
        if actor.session != session {
            return Err(HudError::ActorSessionMismatch);
        }
        let report = self.teardown();
        self.hud.bind(session, actor)?;
        Ok(report)
    }

    /// Asks for the local pause under the mode's own authority.
    fn hold(&mut self, reason: PauseReason) -> PauseDecision {
        if !self.mode.may_pause_locally() {
            return PauseDecision::NoLocalAuthority;
        }
        match self.pause {
            Some(existing) => PauseDecision::AlreadyPaused(existing),
            None => {
                self.pause = Some(reason);
                PauseDecision::Paused(reason)
            }
        }
    }

    /// The map page's projection: authored geography, revealed contacts,
    /// visible objectives and the pause commands.
    fn map(
        &self,
        instruments: &Instruments,
        sources: &MissionSources<'_>,
    ) -> Result<MapView, HudError> {
        let (session, _) = self.hud.bound().expect("project proved the binding");
        let geography = match sources.world {
            Some((definition, load)) => {
                if load.definition() != definition.id() {
                    return Err(HudError::WorldMismatch {
                        definition: definition.id().clone(),
                        load: load.definition().clone(),
                    });
                }
                definition
                    .objects()
                    .iter()
                    .filter(|object| load.activates(object.id()))
                    .map(|object| GeographyMark {
                        object: object.id().clone(),
                        position: object.transform().translation(),
                        condition: load.initial_condition(object.id()),
                    })
                    .collect()
            }
            None => Vec::new(),
        };
        let (player, contacts) = match sources.roster {
            Some(store) => {
                if store.session() != session.get() {
                    return Err(HudError::ForeignAuthority {
                        source: "roster",
                        expected: session,
                        found: store.session(),
                    });
                }
                self.contacts(store, instruments)
            }
            None => (None, Vec::new()),
        };
        Ok(MapView {
            player,
            geography,
            contacts,
            objectives: sources
                .objectives
                .map(ObjectiveDisplay::visible)
                .unwrap_or_default(),
            commands: PauseCommand::ALL,
            pause: self.pause,
        })
    }

    /// The player mark and the revealed contacts of `store`, for the bound
    /// actor.
    fn contacts(
        &self,
        store: &TargetStore,
        instruments: &Instruments,
    ) -> (Option<PlayerMark>, Vec<ContactMark>) {
        let (_, actor) = self.hud.bound().expect("project proved the binding");
        let (player, observer_faction) = match store.record(&actor) {
            Some(record) => (
                Some(PlayerMark {
                    position: record.position,
                    heading: instruments.attitude.heading,
                }),
                Some(record.faction.clone()),
            ),
            None => (None, None),
        };
        let mut contacts = Vec::new();
        for other in store.registered() {
            if other == actor {
                continue;
            }
            let Some(record) = store.record(&other) else {
                continue;
            };
            // The fidelity gate: an unrevealed actor or one whose entity
            // left the world is never a mark.
            if !record.revealed || !store.present(&other) {
                continue;
            }
            contacts.push(ContactMark {
                actor: other,
                position: record.position,
                class: record.class,
                allegiance: observer_faction
                    .as_ref()
                    .and_then(|faction| store.allegiance(faction, &record.faction)),
                objective: record.objective,
            });
        }
        (player, contacts)
    }
}

/// The recon page's projection: the published spyglass readout, only while
/// the consumers are bound to this observer — another aircraft's view is
/// never drawn.
fn recon(targets: Option<&TargetConsumers>, observer: ActorId) -> Option<SpyglassReadout> {
    let consumers = targets?;
    match consumers.bound() {
        Some(bound) if bound.observer == observer => consumers.spyglass().cloned(),
        _ => None,
    }
}
