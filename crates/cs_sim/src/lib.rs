//! Gameplay state, flight forces, combat, AI and objectives.
//!
//! Owner crate per `docs/01-ARCHITECTURE.md`. Allowed dependencies: [`cs_types`]
//! and [`cs_script`]; it consumes normalized records and never parses retail
//! bytes. `bevy_ecs`/`bevy_math` may only enter through an approved boundary,
//! and the physics adapter stays in `cs_app`.
//!
//! [`time`] is the typed-time foundation of
//! `specs/F16-coordinates-units-origin-management-and-clocks.md` (stage
//! F16-A): integer simulation ticks with a fixed dt, the four distinct time
//! domains (simulation, UI wall, unscaled media, authoritative gameplay) and
//! the explicit per-subsystem pause and speed-up policies. Gameplay systems
//! land with the F13+ tasks and consume these clocks instead of inventing
//! their own timers.
//!
//! [`environment`] owns the air-relative velocity conversion and nothing else
//! (`specs/F19-sky-atmosphere-weather-and-visibility.md`, non-negotiable
//! behavior 2; task #434 `F19-WIND-CONVERSION-OWNER`):
//! [`environment::air_relative_velocity_m_s`] is the single implementation of
//! the `FLIGHT-PHYSICS` convention `v_air = v_world - wind_world`, and the
//! flight models, `cs_app::environment::air` and every future weapon consumer
//! call it. It sits here because the code that *applies* a wind is below the
//! crate that first needed it, and `cs_app -> cs_sim` is one-way: F19-B could
//! not reach its own conversion from here. No wind record, no still-air default
//! and no wind profile live in this module — reading the authoritative field
//! out of a `cs_content` environment state stays in `cs_app`, because
//! `cs_content` is not a dependency this crate may take.
//!
//! [`visibility`] is the environment's time domain
//! (`specs/F19-sky-atmosphere-weather-and-visibility.md`, stage `### F19-A`):
//! [`visibility::ENVIRONMENT_TIME_DOMAIN`] puts authored weather changes on
//! authoritative-gameplay time, [`visibility::environment_clock_policy`]
//! states that pause freezes them and that no local authority may inject
//! ticks, and [`visibility::VisibilityTimeline`] installs an event's state
//! only on the whole tick the clock committed — the record it runs is
//! `cs_content::environment::EnvironmentTimeline`, wired in
//! `cs_app::environment`. No value here is derived from screen fog, and no
//! default is invented for a state the caller passes in.
//!
//! [`animated_object`] is the F20-A animation runtime
//! (`specs/F20-object-animation-and-authored-destruction-states.md`, stage
//! `### F20-A`): the tick-indexed channel records (transform, visibility,
//! material, attachment), the gameplay/presentation event markers with their
//! once-per-activation dedup, the [`animated_object::AnimatedObject`]
//! fixed-tick evaluator whose per-node state keeps mesh and collider on the
//! same evaluated pose, and the minimal synthetic door/propeller fixtures.
//! The declared, provenance-carrying clip form is `cs_content::animation`;
//! the conversion boundary and presentation interpolation are
//! `cs_app::animation` (F20-B wires real tracks, F20-C stateful props).
//!
//!
//! [`control`] is the F22-A/F22-B command schema's simulation consumer
//! (`specs/F22-input-bindings-devices-and-control-ownership.md`): the
//! [`control::ControlBuffer`] that keeps continuous axes and one-shot edges
//! separate and delivers a one-frame key edge exactly once across every
//! physics substep, the [`control::ControlGate`] that enforces exactly one
//! control authority and gates local input by `cs_types::input::InputContext`,
//! and the F22-B [`control::ThrottleSteps`], whose steps and direct settings
//! are applied at the input boundary so they never depend on the render frame
//! rate. The device adapters and calibration are `cs_app::input::devices`;
//! focus, replay and full ownership wiring are F22-C.
//!
//! [`collision`] is the F23-A collision vocabulary
//! (`specs/F23-avian-integration-collision-and-fixed-step-authority.md`,
//! stage `### F23-A`): the six declared [`collision::CollisionLayer`]s, the
//! designed interaction matrix, the [`collision::CollisionLayers`] bitmask and
//! [`collision::classify_contact`], which makes a sensor overlap distinct from
//! a solid contact in code. It creates no Avian body; the schedule adapter is
//! `cs_app::physics` (F23-B/C create and drive the actual bodies).
//!
//! [`flight`] is the F24-A fixed-wing contract and equations
//! (`specs/F24-fixed-wing-flight-engine-stall-and-arcade-assists.md`, stage
//! `### F24-A`): the normalized [`flight::AirframeTuning`] schema, the
//! loadout-mass and damage records, the pure [`flight::FlightModel`] force
//! equations (air-relative velocity, lift/drag, thrust, world-space gravity
//! and a bounded rate-command torque) and the synthetic fixture/probe. At zero
//! airspeed every computed value is finite and gravity still acts. The
//! provenance-carrying tuning schema is `cs_content::flight_tuning`; the
//! Avian wiring, instruments and profile selection are F24-B/F24-C.
//!
//! [`probes`] is the F26-A handling telemetry and reference-envelope schema
//! (`specs/F26-handling-probes-and-original-behavior-calibration.md`, stage
//! `### F26-A`): the closed [`probes::ProbeKind`]/[`probes::ProbeQuantity`]
//! vocabulary, the provenance-carrying [`probes::ReferenceEnvelope`] with its
//! recorded input, initial state, difficulty, loadout, timing uncertainty and
//! units, and [`probes::compare`], the holdout rule that refuses a candidate
//! whose held-out maneuver is outside the envelope. At least one entry must be
//! held out of the fit, and a synthetic envelope can never support an
//! original-fidelity claim. The headless probes that produce a real candidate
//! are F26-B; the roster-wide audit and deviation report is `probes::audit` (F26-C).
//!
//! [`weapons`] is the F27-A weapon contract
//! (`specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`, stage
//! `### F27-A`): the [`weapons::GunDefinition`] whose mount is the same
//! [`damage::DamageNodeKey`] the F29 damage graph disables, the
//! [`weapons::WeaponState`] of selected [`weapons::GunBank`], per-mount
//! cooldown in ticks, rounds and disabled mounts, the
//! [`weapons::FireIntent`] → [`weapons::FireResolution`] pair that the
//! per-session [`weapons::FireResolver`] resolves exactly once, and the
//! [`weapons::Ballistics`] swept-segment query with relative motion,
//! earliest-time-of-impact ordering and a once-per-projectile ledger.
//! The declared, provenance-carrying schema is `cs_content::weapons`; the
//! lowering boundary is `cs_app::weapons`; the cadence loop and the mount
//! transforms from the live hierarchy are F27-B, and the damage, effect,
//! audio and bank-selection wiring is F27-C.
//!
//! [`damage`] is the F29-A damage contract
//! (`specs/F29-damage-zones-armor-destruction-and-bailout.md`, stage
//! `### F29-A`): the per-actor [`damage::DamageGraph`] of armor zones,
//! internal structure, engines and weapon mounts, the immutable
//! [`damage::HitEvent`] input and ordered [`damage::DamageEvent`] output
//! vocabulary, the five distinct [`damage::LifecycleKind`]s, and the
//! per-session [`damage::DamageResolver`] whose declared hit ordering and
//! per-actor declared attribution award a single kill per destruction.
//! The declared, provenance-carrying schema is `cs_content::damage`; the
//! lowering boundary and ECS bindings are `cs_app::damage`; armor-driven
//! disablement is F29-B and visual/scoring wiring is F29-C.
//!
//! [`targeting`] is the F30-A/F30-B targeting contract
//! (`specs/F30-targeting-classification-aim-assistance-and-threat-cues.md`,
//! stages `### F30-A`/`### F30-B`): the per-session
//! [`targeting::TargetStore`] owning the target roster, the directed
//! [`targeting::AllegianceTable`] and the authoritative-attack threat
//! ledger; the typed [`targeting::TargetRecord`] input, the
//! [`targeting::SelectionRequest`] vocabulary and the read-only
//! [`targeting::TargetInfo`] snapshot HUD and spyglass consume; the
//! total `(distance, ActorId)` ordering that makes equal-distance cycling
//! deterministic; and F30-B's production path — the
//! [`targeting::SelectionBinding`] command-edge table with
//! [`targeting::TargetStore::act`], the [`targeting::TargetStore::phase`]
//! record whose reticle and AI hostility gate are one read, and
//! [`targeting::TargetStore::record_hits`] as the threat state's only feed.
//! F30-C adds the consumer contract those records need:
//! [`targeting::TargetPhase::cleared`] with
//! [`targeting::SelectionClearReason`], so a consumer learns from one read
//! that its target went away and why, and [`targeting::TargetStore::present`],
//! which separates "still in the world" from
//! [`targeting::TargetStore::eligible`] for a threat cue's attacker.
//! The declared schema is `cs_content::target_rules`; the conversion
//! boundary, ECS bindings and the HUD/spyglass/guidance consumer views are
//! `cs_app::targeting`; verification is F30-D.
//!
//! [`ai`] is the F31-A/F31-B navigation contract
//! (`specs/F31-ai-navigation-routes-and-obstacle-avoidance.md`, stages
//! `### F31-A`/`### F31-B`): the Bevy-free [`ai::navigation::RouteGraph`] with
//! its stable node ids and authored sequences, the
//! [`ai::navigation::ManeuverEnvelope`] that bounds every command, the
//! monotonic [`ai::navigation::RouteProgress`], the swept arrival and
//! [`ai::navigation::Blocker`] tests and [`ai::navigation::Navigator::decide`],
//! a pure function of one typed tick that emits the same
//! [`flight::FlightInput`] a player's controls produce; and the stateful
//! [`ai::navigation::NavigationSet`], which owns one
//! [`ai::navigation::PursuitState`] per actor and derives each actor's
//! tie-break from the mission seed, so reordering the ECS entities that
//! present the actors cannot change a local decision sequence. The
//! provenance-carrying producer record is `cs_content::routes`; F31-C wires
//! the set into the ECS and the original routes.
//!
//! [`allies`] is the F33-A pilot/aircraft/faction identity contract, the
//! F33-B allegiance and wingmate assignment rules, and the F33-C runtime
//! lifecycle and mission callbacks
//! (`specs/F33-wingmates-factions-neutral-traffic-and-pilot-identity.md`,
//! stages `### F33-A`, `### F33-B` and `### F33-C`): the per-session
//! [`allies::AlliesRoster`] whose [`allies::AllyRecord`] keeps
//! [`allies::PilotId`], [`allies::FactionId`], [`allies::GeometryId`] and the
//! shared [`damage::ActorId`] identity as four distinct types; the
//! [`allies::WingmateAssignment`] store and [`allies::SurvivabilityPolicy`];
//! and [`allies::AlliesRoster::capture`], the ownership transaction that
//! changes an actor's faction and returns the geometry id it did **not**
//! change (AC01). F33-B adds [`allies::briefed_wingmates`], the pure rule
//! that turns a [`allies::BriefingPlan`] into assignments, and
//! [`allies::AlliesRoster::reset_wingmates`] / `register_wingmate`, the retry
//! reset and the player-faction commitment. F33-C adds
//! [`allies::AlliesRoster::record_lifecycle`] /
//! [`allies::AlliesRoster::register_with_role`], which turn an authoritative
//! F29 [`damage::LifecycleKind`] into the [`allies::AllyEvent`] mission
//! callback — a lost wingmate, a protected-neutral loss, an ordinary ally
//! loss, a capture or a bailout — carrying the actor's authored voice, and
//! [`allies::AlliesRoster::may_fire`], the gate that grounds a destroyed,
//! despawned or mission-removed actor. The declared roster schema is
//! `cs_content::pilots`; the conversion boundary, the ECS binding and the
//! firing/lifecycle consumer seam are `cs_app::roster`.
//!
//! [`world_actors`] is the F34-A world-actor contract
//! (`specs/F34-ground-vehicles-boats-trains-and-mission-machinery.md`, stage
//! `### F34-A`): tick-indexed [`world_actors::trajectory::Trajectory`] whose
//! position and velocity share one function, the single
//! [`world_actors::anchor::anchor_sample`] renderer and pickup both read,
//! relative-velocity pickup eligibility, the explicit
//! [`world_actors::graph::SupportGraph`] and detached-payload release. The
//! runtime is F34-B and the wiring is F34-C.
//!
//! [`capital`] is the F35-A capital-ship contract
//! (`specs/F35-zeppelins-capital-ships-subsystems-and-launch-bays.md`, stage
//! `### F35-A`): the [`capital::SubsystemGraph`] of engines, bays, turrets,
//! docking anchors, gas cells and structural sections whose
//! [`capital::SubsystemGraph::disable`] applies the destroyed part's
//! behavior; the [`capital::EngineSpec`] thrust sum a disabled engine
//! removes (the minimum scenario); the explicit tick-indexed
//! [`capital::ExposureWindow`] that makes a bay a weakpoint only while open;
//! the once-only [`capital::LaunchLedger`] and [`capital::LaunchSocket`]
//! release; and the staged [`capital::CaptureTransaction`] ownership
//! contract. The declared schema is `cs_content::capital` and the lowering
//! boundary is `cs_app::capital`; the movement, weakpoint and turret runtime
//! is F35-B and the launch/capture wiring is F35-C.
//!
//! [`interaction`] is the F36-A docking/pickup/boarding/plane-swap contract
//! (`specs/F36-docking-passenger-pickups-boarding-and-plane-swaps.md`, stage
//! `### F36-A`): the four [`interaction::InteractionKind`]s that share
//! infrastructure but keep distinct effects, the explicit
//! [`interaction::InteractionState`] chain, the stable
//! [`interaction::InteractionId`] binding initiator, target, authorization
//! and session, the swept [`interaction::evaluate_eligibility`] whose closest
//! approach over relative motion replaces a single radius test, and
//! [`interaction::InteractionTransaction`] with its declared per-transition
//! [`interaction::TransferPolicy`]. The declared schema is
//! `cs_content::interaction`; the lowering boundary is `cs_app::interaction`;
//! the moving-frame runtime and consumer wiring are F36-B/C.
//!
//! [`mission`] is the F37-A/F37-B/F37-C mission session
//! (`specs/F37-mission-ir-and-deterministic-runtime-core.md`): it launches only
//! a validated `cs_script::ir::MissionProgram`, drives the tick-ordered
//! objective state, and applies the effects that state asks for through
//! [`mission::HostLedger`] — the authoritative record of granted rewards, the
//! session's one resolved outcome and the effects refused for a retry. It also
//! moves the evaluator state and that host record together across a save.
//! Native host bindings are F38.
//!
//! [`objectives`] is the F39-A objective/trigger/spawn vocabulary
//! (`specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`,
//! stage `### F39-A`): the seven objective states, swept entry/exit triggers
//! that never sweep a teleport, per-category actor counters and the
//! per-session idempotency ledger for spawns and cues. The runtime is F39-B.
//!
//! [`cinematic_state`] is the F40-A cutscene contract
//! (`specs/F40-cutscenes-video-scripted-cameras-and-transitions.md`, stage
//! `### F40-A`): the explicit [`cinematic_state::CinematicState`] chain of the
//! [`cinematic_state::CinematicPlayer`], and the
//! [`cinematic_state::SemanticAction`]s kept apart from media presentation and
//! applied exactly once whether the scene plays, is skipped or its media
//! fails. The declared schema is `cs_content::cinematics`; the lowering
//! boundary is `cs_app::cinematics`; decoded playback and wiring are F40-B/C.
//!
//! [`audio_events`] is the F41-A audio runtime
//! (`specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stage
//! `### F41-A`): the audio-scoped [`audio_events::AudioEventId`] event identity,
//! the bounded [`audio_events::AudioRouter`] whose per-`(session, producer)`
//! sequence ledger accepts a one-shot exactly once and suppresses a replay, and
//! the loop-emitter registry that stops on despawn, swap, declared pause policy
//! or device loss. The provenance-carrying producer record is
//! `cs_content::audio`; the conversion boundary is `cs_app::audio` (F41-B
//! implements decoding, spatial emitters and the real mixer).
//!
//! [`stunts`] is the F42-A stunt contract
//! (`specs/F42-stunts-fame-photos-and-optional-achievement-events.md`, stage
//! `### F42-A`): the lowered [`stunts::Gate`] whose `classify` predicate
//! answers whether a segment actually *flew through* an authored aperture
//! (mid-plane crossing inside the hole, authored direction cosine, authored
//! rim margin), the typed [`stunts::StuntMovement`] that keeps a rebase
//! continuous and a teleport unearnable, the [`stunts::StuntAuthority`] that
//! stops a developer camera from scoring, and the
//! [`stunts::StuntRewardKey`] / [`stunts::StuntLedger`] pair that
//! deduplicates a one-time reward by profile/mission/stunt identity across a
//! mission retry. The declared, provenance-carrying schema is
//! `cs_content::stunts`; the lowering boundary is `cs_app::stunts`; the
//! multi-gate sequence detection is F42-B and the fame, AI and scrapbook
//! wiring is F42-C.
//!
//! [`campaign`] is the F43-A campaign contract
//! (`specs/F43-campaign-progression-outcomes-and-economy-rules.md`, stage
//! `### F43-A`): the validated [`campaign::CampaignGraph`] keyed on
//! `cs_script::Outcome`, the [`campaign::OutcomeId`] tuple the exactly-once
//! ledger dedups, the immutable [`campaign::MissionOutcome`] transaction
//! input and [`campaign::CampaignState`], whose
//! [`campaign::CampaignState::apply_outcome`] checks eligibility and prior
//! application, computes the whole change set in memory and commits it in
//! one revision — a replayed packet can never pay twice and a replayed
//! mission never moves progression. The declared, provenance-carrying
//! schema is `cs_content::campaign`; the lowering boundary is
//! `cs_app::campaign`; purchases, saves and briefing wiring are F43-B/C.
//!
//! [`multiplayer`] is the F56-A multiplayer rule layer
//! (`specs/F56-original-multiplayer-scenarios-and-mode-rules.md`, stage
//! `### F56-A`): [`multiplayer::result::MatchResolver`] folds lethal events and
//! the time/score limits into one sealed final result, deduplicating a
//! retransmitted event by its `EventId` and refusing another session's events.
//! The original rule values are unknown, so the score table and limits are
//! caller inputs; mode state machines are F56-B.
//!
//! [`net_state`] is the F57-A authoritative network state
//! (`specs/F57-networked-aircraft-prediction-interpolation-and-projectiles.md`,
//! stage `### F57-A`): the per-session [`net_state::NetStateLedger`] that owns
//! every actor's authoritative [`net_state::NetActorState`], allocates the
//! nonzero [`net_state::ActorGeneration`] each actor's identity is checked
//! against, records a destruction exactly once per generation however many times
//! it is reported, and advances only the client input acknowledgment. Nothing
//! client-authored can move an actor, spend a round or boost capacity, or end a
//! record, which is what keeps a client's prediction from becoming authority
//! (contract `docs/contracts/UI-NETWORK.md`). The wire schema and its declared
//! quantization budgets are `cs_net::snapshot`; the receiver boundary is
//! `cs_app::network::physics`.
//!
//! [`cs_types`]: cs_types
//! [`cs_script`]: cs_script

pub mod ai;
pub mod allies;
pub mod animated_object;
pub mod audio_events;
pub mod campaign;
pub mod capital;
pub mod cinematic_state;
pub mod collision;
pub mod control;
pub mod damage;
pub mod environment;
pub mod flight;
pub mod interaction;
pub mod mission;
pub mod multiplayer;
pub mod net_state;
pub mod objectives;
pub mod probes;
pub mod records;
pub mod stunts;
pub mod targeting;
pub mod time;
pub mod visibility;
pub mod weapons;
pub mod world_actors;
