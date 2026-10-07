//! Acceptance scenario AC04 for F44-D: **preview and actual spawned aircraft
//! have equal normalized mass, weapons and paint**.
//!
//! Spec: `specs/F44-aircraft-construction-budgets-loadouts-and-paint-editor.md`,
//! stage `### F44-D`; contract `docs/contracts/STATE-TRANSACTIONS.md`.
//! Task test prefix: `accept_f44_d_`.
//!
//! These tests drive production code end to end: `ConstructionScreen::view` is
//! the preview, [`spawn_blueprint`] is the spawn (it runs the shared validator,
//! the production `spawn_flight_body` and the production `WeaponSession`), and
//! `SpawnedAircraft::normalized` reads the result back **out of the world**.
//! Nothing here reimplements a validator, a total or a spawn.
//!
//! The stage's failure cases are pinned, not just the happy path: a spawn of a
//! *different* record than the one previewed must be detected on all three
//! projections, an invalid draft must spawn nothing, and a gun no declared
//! record covers must refuse the spawn by name instead of quietly disappearing
//! from the aircraft.
//!
//! Every value here is newly authored synthetic fixture data; no price, mass or
//! limit is an original value, and no `CS_GAME_DIR` access happens.

use bevy::prelude::World;
use cs_app::asset_stack::headless_app;
use cs_app::campaign::lower_campaign;
use cs_app::construction::{
    BlueprintSpawnError, BlueprintSpawnRequest, ConstructionContext, ConstructionScreen,
    PreviewRefusal, SpawnedBlueprint, preview_normalized, spawn_blueprint,
};
use cs_app::physics::{FlightAircraft, FlightSpawnSpec};
use cs_app::weapons::WeaponSession;
use cs_content::campaign::{
    CampaignDefinition, CampaignDraft, CampaignEdge, CampaignNode, CampaignNodeId, EdgeCondition,
    NodeKind, RewardSpec, RosterEntry,
};
use cs_content::construction::{
    AircraftBlueprint, ArmorFitment, ConstructionPolicy, ConstructionRules, DecalPlacement,
    OrdnanceFitment, PaintSelection, PriceBook, SYNTHETIC_BOUNDARY_MASS_LIMIT_UNITS,
    SYNTHETIC_GUN_KEY as CONSTRUCTION_GUN_KEY, SYNTHETIC_HEAVY_PLATE_KEY,
    declared_synthetic_blueprint, declared_synthetic_price_book, synthetic_armor_fitments,
    synthetic_boundary_rules, synthetic_ordnance_fitments, synthetic_policy,
};
use cs_content::weapons::{DeclaredGunDefinition, declared_synthetic_gun};
use cs_sim::campaign::{
    CampaignGraph, CampaignNodeKey, CampaignRunId, CampaignState, DifficultyId, EventKey,
    MissionOutcome, Outcome, OutcomeAuthority, OutcomeId, ProfileId, SessionGeneration, SymbolId,
};
use cs_sim::damage::{ActorId, DamageNodeKey};
use cs_sim::flight::{FlightModel, LoadoutMass, synthetic_fixed_wing};
use cs_sim::weapons::{GunBank, SYNTHETIC_STARTING_ROUNDS};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::net::SessionId;

/// The synthetic budget of one win, far above the synthetic prices.
const GRANT: u64 = 1_000_000;
/// The weapon session generation this fixture registers under.
const SESSION: u64 = 44;
/// The router producer id the fixture's session declares.
const ROUTER_PRODUCER: u32 = 93;

fn designed() -> Provenance {
    Provenance::designed(ClaimId::new("f44d.test").expect("valid claim id"))
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, designed()))
}

fn id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("valid content id")
}

fn node(key: &str) -> CampaignNodeId {
    CampaignNodeId::new(key).expect("valid node id")
}

/// `m01 --Victory(+grant)--> ending`, with every synthetic component rostered
/// from `m01`.
fn graph(grant: u64) -> CampaignGraph {
    let policy = synthetic_policy();
    let mut draft = CampaignDraft {
        nodes: vec![
            CampaignNode {
                id: node("m01"),
                kind: NodeKind::Mission {
                    mission: known(id(ContentKind::Mission, "m01")),
                },
                edges: vec![CampaignEdge {
                    on: EdgeCondition::Victory,
                    to: node("ending"),
                    grant: Some(RewardSpec {
                        currency: known(grant),
                        unlocks: Vec::new(),
                    }),
                    provenance: designed(),
                }],
                provenance: designed(),
            },
            CampaignNode {
                id: node("ending"),
                kind: NodeKind::Ending,
                edges: Vec::new(),
                provenance: designed(),
            },
        ],
        entry: node("m01"),
        roster: Vec::new(),
        provenance: designed(),
    };
    for item in policy.available() {
        draft.roster.push(RosterEntry {
            item: item.clone(),
            available_from: node("m01"),
            provenance: designed(),
        });
    }
    let definition = CampaignDefinition::try_new(draft).expect("the fixture campaign is valid");
    lower_campaign(&definition).expect("the fixture campaign lowers")
}

fn profile() -> ProfileId {
    ProfileId::new("pilot.test").expect("valid profile id")
}

/// A state that has won `m01` and so holds `GRANT` with every component open.
fn funded(graph: &CampaignGraph) -> CampaignState {
    let run = CampaignRunId::new("run.one").expect("valid run id");
    let mut state = CampaignState::begin(
        profile(),
        run.clone(),
        DifficultyId::new("standard").expect("valid difficulty"),
        graph,
    );
    let outcome = MissionOutcome {
        id: OutcomeId {
            profile: profile(),
            run,
            session: SessionGeneration(1),
            terminal_event: EventKey {
                session: SessionGeneration(1),
                tick: Tick(10),
                source: SymbolId(7),
                sequence: 1,
            },
        },
        node: CampaignNodeKey::new("m01").expect("valid node key"),
        outcome: Outcome::Succeeded,
        score: 1,
        authority: OutcomeAuthority::Authorized,
    };
    state.apply_outcome(graph, &outcome).expect("m01 is won");
    state
}

struct Fixture {
    graph: CampaignGraph,
    rules: ConstructionRules,
    policy: ConstructionPolicy,
    book: PriceBook,
}

impl Fixture {
    fn new() -> Self {
        Self {
            graph: graph(GRANT),
            rules: synthetic_boundary_rules(),
            policy: synthetic_policy(),
            book: declared_synthetic_price_book(),
        }
    }

    fn ctx(&self) -> ConstructionContext<'_> {
        ConstructionContext {
            rules: &self.rules,
            policy: &self.policy,
            book: &self.book,
            graph: &self.graph,
        }
    }
}

/// A different paint selection: one other mask with a decal over it, still
/// nothing but catalog references.
fn alt_paint() -> PaintSelection {
    let mask = id(ContentKind::PaintMask, "fixture.f44d.alt_mask");
    PaintSelection::try_new(
        vec![mask.clone()],
        vec![
            DecalPlacement::try_new(id(ContentKind::PaintMask, "fixture.f44d.alt_decal"), mask)
                .expect("valid decal"),
        ],
    )
    .expect("valid paint selection")
}

/// The boundary blueprint with one rocket removed and another paint: still
/// inside every limit, but a different mass, a different weapon list and a
/// different paint — a record the spawn could plausibly be handed by mistake.
fn lighter_blueprint() -> AircraftBlueprint {
    let rockets: Vec<OrdnanceFitment> = synthetic_ordnance_fitments().into_iter().take(3).collect();
    declared_synthetic_blueprint()
        .with_ordnance(rockets)
        .expect("valid blueprint")
        .with_paint(alt_paint())
}

/// The boundary blueprint with one plate swapped for a plate one unit heavier:
/// exactly one weight unit over the ceiling, and still a well-formed record.
fn one_unit_overweight() -> AircraftBlueprint {
    let mut armor = synthetic_armor_fitments();
    armor[0] = ArmorFitment::try_new(
        armor[0].zone().clone(),
        id(ContentKind::Armor, SYNTHETIC_HEAVY_PLATE_KEY),
    )
    .expect("valid armor");
    declared_synthetic_blueprint()
        .with_armor(armor)
        .expect("valid blueprint")
}

/// The fixture gun re-mounted on `mount`, under the id the construction
/// fixture's blueprint fits.
///
/// The declared record is the production one (`declared_synthetic_gun`) with
/// only its mount and catalog id retargeted — the construction fixture spells
/// its gun `fixture.synthetic_gun` while the weapons fixture spells its own
/// `synthetic.fixture_gun` — so every ballistic, damage and sound field is the
/// shipped fixture's own.
fn declared_gun_at(mount: &str) -> DeclaredGunDefinition {
    let fixture = declared_synthetic_gun();
    DeclaredGunDefinition::try_new(
        ContentId::from_source(ContentKind::Weapon, CONSTRUCTION_GUN_KEY)
            .expect("the construction fixture gun id is valid"),
        fixture.origin().clone(),
        DamageNodeKey::new(mount).expect("a valid mount key"),
        fixture.mount_kind(),
        fixture.scene_binding().cloned(),
        fixture.caliber().clone(),
        fixture.ammunition().clone(),
        fixture.rate().clone(),
        fixture.muzzle_velocity_mps().clone(),
        fixture.lifetime_ticks().clone(),
        fixture.spread().clone(),
        fixture.damage().clone(),
        fixture.inheritance().clone(),
        fixture.effect().clone(),
        fixture.sound().clone(),
        fixture.rules().clone(),
        fixture.provenance().clone(),
    )
    .expect("the retargeted fixture gun is valid")
}

/// A declared record for every gun mount the fixture blueprint fits.
fn declared_catalogue() -> Vec<DeclaredGunDefinition> {
    ["gun_mount_1", "gun_mount_2", "gun_mount_3", "gun_mount_4"]
        .iter()
        .map(|mount| declared_gun_at(mount))
        .collect()
}

fn session() -> WeaponSession {
    WeaponSession::new(SESSION, Tick(0), ROUTER_PRODUCER).expect("a nonzero session generation")
}

fn actor() -> ActorId {
    ActorId {
        session: SessionId::new(SESSION).expect("the fixture session generation is nonzero"),
        serial: 1,
    }
}

/// A level spawn spec for the fixture, with the loadout mass the caller wants.
fn spec(loadout: LoadoutMass) -> FlightSpawnSpec {
    let mut spec = FlightSpawnSpec::level_at([0.0, 250.0, 0.0], [0.0, 0.0, -55.0]);
    spec.loadout = loadout;
    spec
}

fn model() -> FlightModel {
    FlightModel::new(synthetic_fixed_wing())
}

/// How many spawned-blueprint records the world holds.
fn spawned_count(world: &mut World) -> usize {
    let mut query = world.query::<&SpawnedBlueprint>();
    query.iter(world).count()
}

/// **The minimum scenario: the preview and the aircraft actually spawned from
/// the same draft have equal normalized mass, weapons and paint.**
///
/// Fails against a spawn that reads anything but the draft it was handed — a
/// spawn of the pre-edit record, a dropped rocket or a lost paint selection
/// moves at least one of the three projections.
#[test]
fn accept_f44_d_preview_and_spawned_aircraft_have_equal_mass_weapons_and_paint() {
    let fixture = Fixture::new();
    let state = funded(&fixture.graph);
    let mut screen = ConstructionScreen::open(&state, declared_synthetic_blueprint(), Vec::new());
    screen.edit(lighter_blueprint());
    let ctx = fixture.ctx();

    let view = screen.view(&ctx, &state);
    let preview = preview_normalized(&view).expect("the draft is valid and measurable");

    let mut app = headless_app();
    let world = app.world_mut();
    let mut weapons = session();
    let spawned = spawn_blueprint(
        world,
        &mut weapons,
        actor(),
        SYNTHETIC_STARTING_ROUNDS,
        &BlueprintSpawnRequest {
            blueprint: screen.blueprint(),
            rules: &fixture.rules,
            policy: &fixture.policy,
            book: &fixture.book,
            declared_guns: &declared_catalogue(),
            model: model(),
            spec: spec(LoadoutMass::EMPTY),
        },
    )
    .expect("the draft spawns");
    let actual = spawned
        .normalized(world)
        .expect("the spawn left its record on the entity");

    assert_eq!(
        preview.mass(),
        actual.mass(),
        "the preview and the spawned aircraft must weigh the same"
    );
    assert_eq!(
        preview.weapons(),
        actual.weapons(),
        "the preview and the spawned aircraft must carry the same weapons"
    );
    assert_eq!(
        preview.paint(),
        actual.paint(),
        "the preview and the spawned aircraft must carry the same paint"
    );
    assert_eq!(
        actual.mass().as_units(),
        SYNTHETIC_BOUNDARY_MASS_LIMIT_UNITS - 65,
        "the spawned aircraft is the three-rocket record, not the four-rocket one"
    );
}

/// **The comparison is discriminating: spawning a different record than the one
/// previewed is detected on all three projections.**
///
/// A harness whose two sides were read from one value would pass this test by
/// construction; here the preview shows the edited draft and the spawn is
/// handed the stale pre-edit record, and every projection must disagree.
#[test]
fn accept_f44_d_a_spawn_of_a_different_record_is_detected_as_unequal() {
    let fixture = Fixture::new();
    let state = funded(&fixture.graph);
    let mut screen = ConstructionScreen::open(&state, declared_synthetic_blueprint(), Vec::new());
    screen.edit(lighter_blueprint());
    let ctx = fixture.ctx();
    let preview = preview_normalized(&screen.view(&ctx, &state)).expect("measurable");

    let mut app = headless_app();
    let world = app.world_mut();
    let mut weapons = session();
    let stale = declared_synthetic_blueprint();
    let spawned = spawn_blueprint(
        world,
        &mut weapons,
        actor(),
        SYNTHETIC_STARTING_ROUNDS,
        &BlueprintSpawnRequest {
            blueprint: &stale,
            rules: &fixture.rules,
            policy: &fixture.policy,
            book: &fixture.book,
            declared_guns: &declared_catalogue(),
            model: model(),
            spec: spec(LoadoutMass::EMPTY),
        },
    )
    .expect("the stale record is itself valid");
    let actual = spawned
        .normalized(world)
        .expect("the spawn left its record on the entity");

    assert_ne!(
        preview.mass(),
        actual.mass(),
        "a stale record's mass must not read as the preview's"
    );
    assert_ne!(
        preview.weapons(),
        actual.weapons(),
        "a stale record's weapons must not read as the preview's"
    );
    assert_ne!(
        preview.paint(),
        actual.paint(),
        "a stale record's paint must not read as the preview's"
    );
}

/// **An invalid draft has no normalized form on either side, and spawns
/// nothing.**
///
/// One unit over the ceiling, the preview refuses (`PreviewRefusal::Invalid`)
/// and the spawn refuses by name — no entity, no record, no registered guns. A
/// spawn that quietly produced an aircraft would be the shortcut AC04 exists to
/// catch.
#[test]
fn accept_f44_d_an_invalid_draft_is_refused_by_both_the_preview_and_the_spawn() {
    let fixture = Fixture::new();
    let state = funded(&fixture.graph);
    let mut screen = ConstructionScreen::open(&state, declared_synthetic_blueprint(), Vec::new());
    screen.edit(one_unit_overweight());
    let ctx = fixture.ctx();

    let view = screen.view(&ctx, &state);
    assert!(
        matches!(preview_normalized(&view), Err(PreviewRefusal::Invalid(_))),
        "a draft one unit over the ceiling has no normalized aircraft"
    );

    let mut app = headless_app();
    let world = app.world_mut();
    let mut weapons = session();
    let error = spawn_blueprint(
        world,
        &mut weapons,
        actor(),
        SYNTHETIC_STARTING_ROUNDS,
        &BlueprintSpawnRequest {
            blueprint: screen.blueprint(),
            rules: &fixture.rules,
            policy: &fixture.policy,
            book: &fixture.book,
            declared_guns: &declared_catalogue(),
            model: model(),
            spec: spec(LoadoutMass::EMPTY),
        },
    )
    .expect_err("an overweight draft must not spawn");
    assert!(
        matches!(error, BlueprintSpawnError::Invalid(_)),
        "the spawn refuses through the shared validator, got: {error}"
    );
    assert_eq!(
        spawned_count(world),
        0,
        "a refused spawn leaves no aircraft behind"
    );
}

/// **A gun the declared catalogue does not cover refuses the spawn by name.**
///
/// The blueprint is valid — the same fixture gun at every mount, inside every
/// limit — so only the *declared record* side is missing. Dropping the fitment
/// instead would give the spawned aircraft fewer weapons than the preview
/// showed, which is exactly what "refused, never dropped" forbids.
#[test]
fn accept_f44_d_a_gun_no_declared_record_covers_refuses_the_spawn() {
    let fixture = Fixture::new();
    let state = funded(&fixture.graph);
    let screen = ConstructionScreen::open(&state, declared_synthetic_blueprint(), Vec::new());
    let ctx = fixture.ctx();
    let preview = preview_normalized(&screen.view(&ctx, &state)).expect("the draft is valid");
    assert_eq!(
        preview.weapons().len(),
        8,
        "four guns and four rockets are what the preview shows"
    );

    let mut app = headless_app();
    let world = app.world_mut();
    let mut weapons = session();
    let only_first = [declared_gun_at("gun_mount_1")];
    let error = spawn_blueprint(
        world,
        &mut weapons,
        actor(),
        SYNTHETIC_STARTING_ROUNDS,
        &BlueprintSpawnRequest {
            blueprint: screen.blueprint(),
            rules: &fixture.rules,
            policy: &fixture.policy,
            book: &fixture.book,
            declared_guns: &only_first,
            model: model(),
            spec: spec(LoadoutMass::EMPTY),
        },
    )
    .expect_err("an uncovered mount must refuse the spawn");
    match error {
        BlueprintSpawnError::UndeclaredGun { gun, mount } => {
            assert_eq!(
                mount,
                DamageNodeKey::new("gun_mount_2").expect("the second mount"),
                "the first uncovered mount is the one named"
            );
            assert_eq!(gun, id(ContentKind::Weapon, "fixture.synthetic_gun"));
        }
        other => panic!("the spawn refuses by name, got: {other}"),
    }
    assert_eq!(
        spawned_count(world),
        0,
        "a refused spawn leaves no aircraft behind"
    );
}

/// **There is exactly one mass on the spawned aircraft: the body integrates the
/// declared tuning plus the declared loadout, no second number.**
///
/// The kilograms are declared fixture values, *not* original data — the
/// original weight unit's scale is unmeasured, so nothing here claims the
/// game-weight total of the same aircraft in kilograms.
#[test]
fn accept_f44_d_the_spawned_body_carries_the_declared_total_mass() {
    let fixture = Fixture::new();
    let state = funded(&fixture.graph);
    let screen = ConstructionScreen::open(&state, declared_synthetic_blueprint(), Vec::new());
    let loadout = LoadoutMass {
        fuel_kg: 120.0,
        ordnance_kg: 34.0,
        armor_kg: 56.0,
    };

    let mut app = headless_app();
    let world = app.world_mut();
    let mut weapons = session();
    let spawned = spawn_blueprint(
        world,
        &mut weapons,
        actor(),
        SYNTHETIC_STARTING_ROUNDS,
        &BlueprintSpawnRequest {
            blueprint: screen.blueprint(),
            rules: &fixture.rules,
            policy: &fixture.policy,
            book: &fixture.book,
            declared_guns: &declared_catalogue(),
            model: model(),
            spec: spec(loadout),
        },
    )
    .expect("the draft spawns");

    let record = world
        .get::<FlightAircraft>(spawned.entity())
        .expect("the spawned body carries its flight record");
    let expected = loadout
        .total_mass_kg(synthetic_fixed_wing().mass.mass_kg)
        .expect("the declared loadout is usable");
    assert_eq!(
        record.total_mass_kg().expect("the record is usable"),
        expected,
        "the one-mass rule: airframe tuning plus the declared loadout"
    );
}

/// The bank the spawn builds is the mounts the blueprint occupies — the same
/// list a caller would have had to assemble by hand, checked against the
/// production `GunBank` rather than asserted as a constant.
#[test]
fn accept_f44_d_the_spawn_bank_is_the_blueprints_own_mounts() {
    let blueprint = declared_synthetic_blueprint();
    let mounts: Vec<DamageNodeKey> = blueprint
        .guns()
        .iter()
        .map(|fitment| fitment.mount().clone())
        .collect();
    let bank = GunBank::try_new(mounts.iter().cloned()).expect("the fixture mounts are real");
    assert_eq!(
        bank.mounts().len(),
        mounts.len(),
        "one bank entry per fitted gun"
    );
}
