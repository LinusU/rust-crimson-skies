//! F39-E-ACTORID-SPACE: the declared-vs-spawned actor-id contract, end to end.
//!
//! Task test prefix: `accept_f39_e_actorid_`.
//!
//! A declared program's [`ProgramActorRef::Spawned`] references lower into the
//! band the runtime's allocator hands out (`SPAWNED_ACTOR_BASE` upward), and
//! its [`ProgramActorRef::Placed`] references keep the ids a mission's
//! pre-placed actors carry. The collision this contract exists for — a placed
//! actor whose record-index id equals a wave instance's — can no longer be
//! written: the waves never take that id. The decision record is
//! `docs/findings/2026-10-13-f39-e-actorid-space.md`; the boundary is
//! **designed**, not measured.

use cs_app::objectives::{ObjectiveSession, lower_program};
use cs_content::objectives::{
    DeclaredCompletion, DeclaredCondition, DeclaredCountKind, DeclaredCountReaction,
    DeclaredObjective, DeclaredObjectiveProgram, DeclaredObjectiveState, DeclaredPrecedence,
    DeclaredRevealRule, DeclaredSpawnGroup, DeclaredTimeDomain, DeclaredTimer, DeclaredTimerAction,
    DeclaredTimerStart, ProgramActorRef, ProgramSymbol, SPAWNED_ACTOR_BASE,
};
use cs_script::ir::{ActorId, SymbolId};
use cs_script::runtime::SessionGeneration;
use cs_sim::damage::LifecycleKind;
use cs_sim::objectives::counters::CountKind;
use cs_sim::objectives::runtime::TickInput;
use cs_sim::objectives::timer::TimerRequest;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

const GEN: SessionGeneration = SessionGeneration(1);

const PRIMARY: ProgramSymbol = ProgramSymbol(1);
const RAIDERS_LOST: ProgramSymbol = ProgramSymbol(10);
const WAVE: ProgramSymbol = ProgramSymbol(20);
const RAIDERS: ProgramSymbol = ProgramSymbol(40);

/// The actor id the lowered reference names: the k-th instance the waves
/// admit, `spawned(k)` → `ActorId(SPAWNED_ACTOR_BASE + k - 1)`.
const fn spawned(k: u32) -> ActorId {
    ActorId(SPAWNED_ACTOR_BASE + k - 1)
}

/// A pre-placed actor wearing a small record-index id — the shape the
/// measured importer (`cs_app::mission_world_actors`) mints. Under the
/// pre-contract allocator, wave 1 took exactly this id.
const PLACED_INDEX: ActorId = ActorId(1);

fn content(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("a valid content id")
}

/// The contract-exercising program: one objective, one `Destroyed` condition
/// over the first two spawned wave members, and one two-raider wave timer.
fn wave_program() -> DeclaredObjectiveProgram {
    let provenance =
        Provenance::designed(ClaimId::new("f39e.actorid-space").expect("a valid claim id"));
    DeclaredObjectiveProgram::try_new(
        content(ContentKind::Mission, "synthetic.f39e.actorid"),
        Origin::SyntheticFixture,
        provenance.clone(),
        Resolved::Known(Known::new(
            DeclaredPrecedence::SyntheticConservative,
            provenance,
        )),
        vec![DeclaredObjective {
            symbol: PRIMARY,
            content: content(ContentKind::Objective, "synthetic.f39e.primary"),
            initial: DeclaredObjectiveState::Active,
            reveal: DeclaredRevealRule::Immediate,
            on_complete: DeclaredCompletion::Continue,
            completion_effects: Vec::new(),
        }],
        vec![DeclaredCondition {
            symbol: RAIDERS_LOST,
            kind: DeclaredCountKind::Destroyed,
            roster: vec![ProgramActorRef::Spawned(1), ProgramActorRef::Spawned(2)],
            required: 2,
            reaction: DeclaredCountReaction::ReportOnly,
        }],
        vec![DeclaredTimer {
            symbol: WAVE,
            domain: DeclaredTimeDomain::AuthoritativeGameplay,
            start: DeclaredTimerStart::OnArm,
            period_ticks: 1,
            action: DeclaredTimerAction::SpawnGroup {
                key: "synthetic.f39e.wave-1".to_owned(),
                group: RAIDERS,
                count: 2,
            },
        }],
        Vec::new(),
        vec![DeclaredSpawnGroup {
            symbol: RAIDERS,
            subject: content(ContentKind::Airframe, "synthetic.raider"),
        }],
    )
    .expect("the actor-space program is valid")
}

fn input<'a>(tick: u64, committed: u64) -> TickInput<'a> {
    TickInput {
        tick: Tick(tick),
        committed_ticks: committed,
        lifecycles: &[],
        movements: &[],
        signals: &[],
        timer_requests: &[],
        objective_requests: &[],
        terminal_requests: &[],
    }
}

fn launch() -> ObjectiveSession {
    ObjectiveSession::launch(
        lower_program(&wave_program()).expect("the program lowers"),
        GEN,
    )
    .expect("the lowered program launches")
}

/// The band has one value wherever it is carried: `cs_content` and `cs_sim`
/// each name it because no crate between them can, and the lowering boundary
/// pins them equal at compile time — this is the run-time mirror of that
/// assertion, so a divergence is caught by a test as well as by the build.
#[test]
fn accept_f39_e_actorid_the_two_crates_carry_one_band_base() {
    assert_eq!(
        SPAWNED_ACTOR_BASE,
        cs_sim::objectives::runtime::SPAWNED_ACTOR_BASE,
        "the declared and runtime spawned-actor bases are one boundary"
    );
}

/// Lowering maps the typed reference into the single `ActorId` space by band:
/// `placed` keeps its id, `spawned(k)` lands on `SPAWNED_ACTOR_BASE + k - 1`.
#[test]
fn accept_f39_e_actorid_a_spawned_reference_lowers_into_the_band() {
    let lowered = lower_program(&wave_program()).expect("the program lowers");
    let (condition, _) = &lowered.conditions[0];
    assert_eq!(
        condition.roster.iter().copied().collect::<Vec<_>>(),
        vec![spawned(1), spawned(2)],
        "the spawned roster names the ids the admission order hands out"
    );
}

/// The session-level conflation the contract prevents: a placed actor's
/// destruction under the small record-index id the old allocator gave wave 1
/// is *not* the wave instance's — the roster counts only the band ids the
/// admission actually handed out.
#[test]
fn accept_f39_e_actorid_a_placed_actor_is_never_counted_as_a_wave_instance() {
    let mut session = launch();
    let arm = [TimerRequest::Arm(SymbolId(WAVE.0))];
    let mut facts = input(1, 1);
    facts.timer_requests = &arm;
    let stepped = session.step(&facts).expect("a legal tick");

    assert_eq!(stepped.spawns.len(), 1);
    assert_eq!(stepped.spawns[0].instances, vec![spawned(1), spawned(2)]);
    assert_eq!(
        session.live_wave(SymbolId(RAIDERS.0)),
        &[spawned(1), spawned(2)][..]
    );

    // The placed actor at the same small index the pre-contract waves took:
    // its lifecycle fact is its own, never the wave member's.
    let mut facts = input(2, 0);
    facts.lifecycles = &[(PLACED_INDEX, LifecycleKind::Destroyed)];
    session.step(&facts).expect("a legal tick");
    assert!(!session.runtime().condition_met(SymbolId(RAIDERS_LOST.0)));
    assert!(
        session
            .runtime()
            .is_counted(CountKind::Destroyed, PLACED_INDEX)
    );
    assert!(
        !session
            .runtime()
            .is_counted(CountKind::Destroyed, spawned(1))
    );

    // The wave members, under their band ids, satisfy the roster.
    let deaths = [
        (spawned(1), LifecycleKind::Destroyed),
        (spawned(2), LifecycleKind::Destroyed),
    ];
    let mut facts = input(3, 0);
    facts.lifecycles = &deaths;
    session.step(&facts).expect("a legal tick");
    assert!(session.runtime().condition_met(SymbolId(RAIDERS_LOST.0)));
}

/// A retry keeps the contract: the teardown names the band ids the old
/// session still owned, and the new generation's re-admission hands out the
/// same band ids — the world despawns and re-binds them without either side
/// ever seeing a placed-space id.
#[test]
fn accept_f39_e_actorid_a_retry_rebinds_the_wave_into_the_same_band_ids() {
    let mut session = launch();
    let arm = [TimerRequest::Arm(SymbolId(WAVE.0))];
    let mut facts = input(1, 1);
    facts.timer_requests = &arm;
    session.step(&facts).expect("a legal tick");

    let teardown = session.retry(SessionGeneration(2)).expect("retry");
    assert_eq!(teardown.actors, vec![spawned(1), spawned(2)]);

    let mut facts = input(1, 1);
    facts.timer_requests = &arm;
    let stepped = session.step(&facts).expect("a legal tick");
    assert_eq!(stepped.spawns[0].instances, vec![spawned(1), spawned(2)]);
    assert_eq!(session.live_actors(), vec![spawned(1), spawned(2)]);
}
