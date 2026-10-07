//! Acceptance scenarios for F44-C: the construction, paint and loadout screen —
//! the producer and consumer of the `ConstructionSession` path.
//! Task test prefix: `accept_f44_c_`.
//!
//! Spec: `specs/F44-aircraft-construction-budgets-loadouts-and-paint-editor.md`,
//! stage `### F44-C` and AC03; contract `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! The stage's minimum scenario is AC03,
//! `accept_f44_c_an_imported_blueprint_cannot_bypass_pairing_or_banned_rules`.
//! Every test drives `cs_app::construction::ConstructionScreen`, the production
//! UI object, which calls the shared validator, the transactional economy and
//! the profile save; none reimplements them.
//!
//! Every value is newly authored synthetic data; no price, mass or limit is an
//! original value.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cs_app::campaign::{CampaignSaveError, lower_campaign, read_snapshot, write_snapshot};
use cs_app::construction::{
    ConstructionContext, ConstructionError, ConstructionScreen, ImportRejection, ScreenSaveError,
    banned_components,
};
use cs_app::profile::ProfileSession;
use cs_content::campaign::{
    CampaignDefinition, CampaignDraft, CampaignEdge, CampaignNode, CampaignNodeId, EdgeCondition,
    NodeKind, RewardSpec, RosterEntry,
};
use cs_content::construction::{
    AircraftBlueprint, ArmorFitment, BudgetQuantity, ConstraintViolation, ConstructionPolicy,
    ConstructionRules, DecalPlacement, PaintSelection, PriceBook,
    SYNTHETIC_BOUNDARY_COST_LIMIT_MINOR, SYNTHETIC_BOUNDARY_MASS_LIMIT_UNITS, SYNTHETIC_GUN_KEY,
    SYNTHETIC_HEAVY_PLATE_KEY, SYNTHETIC_MISSILE_KEY, SYNTHETIC_RADIO_KEY, ValidationRefusal,
    declared_synthetic_blueprint, declared_synthetic_price_book, synthetic_boundary_rules,
    synthetic_gun_fitments, synthetic_policy,
};
use cs_content::save::settings::SettingCatalog;
use cs_sim::campaign::{
    CampaignGraph, CampaignNodeKey, CampaignRunId, CampaignState, DifficultyId, EventKey,
    MissionOutcome, Outcome, OutcomeAuthority, OutcomeId, ProfileId, SessionGeneration, SymbolId,
};
use cs_sim::economy::EconomyError;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::profile::ExtraField;

/// The synthetic budget of one win, far above the synthetic prices.
const GRANT: u64 = 1_000_000;
/// The price of every distinct component of the boundary blueprint:
/// 12000 airframe + 3000 engine + 1000 plate + 1500 gun + 4250 missile + 2000 radio.
const DISTINCT_PRICE: u64 = 23_750;

fn designed() -> Provenance {
    Provenance::designed(ClaimId::new("f44c.test").expect("valid claim id"))
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

/// The boundary blueprint with its armor replaced by one heavy plate on the
/// first zone: exactly one weight unit over the ceiling.
fn one_unit_overweight() -> AircraftBlueprint {
    let mut armor = cs_content::construction::synthetic_armor_fitments();
    armor[0] = ArmorFitment::try_new(
        armor[0].zone().clone(),
        id(ContentKind::Armor, SYNTHETIC_HEAVY_PLATE_KEY),
    )
    .expect("valid armor");
    declared_synthetic_blueprint()
        .with_armor(armor)
        .expect("valid blueprint")
}

/// The boundary blueprint with its first gun mated into a pair (two positions)
/// and the second gun dropped, so the rack still uses four positions: one pair
/// plus two singles.
fn paired_blueprint() -> AircraftBlueprint {
    let mut guns = synthetic_gun_fitments();
    let first = guns.remove(0).with_positions(known(2)).expect("valid");
    guns.remove(0); // four positions: one pair plus two singles
    guns.insert(0, first);
    declared_synthetic_blueprint()
        .with_guns(guns)
        .expect("valid blueprint")
}

/// A different paint selection: one other mask with a decal over it, still
/// nothing but catalog references.
fn alt_paint() -> PaintSelection {
    let mask = id(ContentKind::PaintMask, "fixture.f44c.alt_mask");
    PaintSelection::try_new(
        vec![mask.clone()],
        vec![
            DecalPlacement::try_new(id(ContentKind::PaintMask, "fixture.f44c.alt_decal"), mask)
                .expect("valid decal"),
        ],
    )
    .expect("valid paint selection")
}

struct TempBase(PathBuf);

impl TempBase {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-f44-c-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("the fixture base is created");
        Self(root)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempBase {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn sandbox(base: &Path) -> ProfileSession {
    let catalog = SettingCatalog::new([]).expect("an empty catalog holds together");
    ProfileSession::open_sandbox(base, &catalog).expect("the sandbox session opens")
}

/// A sandbox with one pilot whose save already holds `state`'s campaign.
fn sandbox_with_campaign(base: &Path, state: &CampaignState) -> ProfileSession {
    let mut session = sandbox(base);
    session.create("Pilot").expect("a pilot is created");
    let snapshot = state.snapshot();
    session
        .commit_with(|document| {
            write_snapshot(document, &snapshot).expect("the campaign encodes");
            Ok(())
        })
        .expect("the campaign is stored");
    session
}

/// The stored campaign snapshot of the selected profile.
fn stored_campaign(session: &ProfileSession) -> cs_sim::campaign::CampaignSnapshot {
    read_snapshot(session.document().expect("a selected profile"))
        .expect("the stored campaign parses")
        .expect("a campaign is stored")
}

#[test]
fn accept_f44_c_an_imported_blueprint_cannot_bypass_pairing_or_banned_rules() {
    let fx = Fixture::new();
    let mut state = funded(&fx.graph);
    let mut screen = ConstructionScreen::open(&state, declared_synthetic_blueprint(), vec![]);

    // A pair of a gun the pairing rule does not allow, arriving as an import.
    let no_pairs = ConstructionPolicy::new(
        known(BTreeSet::new()),
        BTreeSet::new(),
        fx.policy.available().clone(),
    );
    let ctx_no_pairs = ConstructionContext {
        policy: &no_pairs,
        ..fx.ctx()
    };
    let rejection = screen
        .import(&ctx_no_pairs, paired_blueprint())
        .expect_err("a forbidden pair must not be adopted");
    let ImportRejection::Invalid(verdict) = rejection else {
        panic!("expected Invalid, got {rejection:?}");
    };
    assert!(verdict.violations().iter().any(
        |v| matches!(v, ConstraintViolation::GunNotPairable { gun, .. } if *gun
                == id(ContentKind::Weapon, SYNTHETIC_GUN_KEY))
    ));
    // The import was refused: the draft is the blueprint the screen opened on,
    // nothing is dirty and the refusal is on the screen's notice for display.
    assert_eq!(screen.blueprint(), &declared_synthetic_blueprint());
    assert!(!screen.is_dirty());
    assert!(screen.notice().is_some());

    // A host ban, arriving as an import, under the permissive pairing rule.
    let banned = fx
        .policy
        .clone()
        .with_banned(id(ContentKind::Weapon, SYNTHETIC_MISSILE_KEY));
    let ctx_banned = ConstructionContext {
        policy: &banned,
        ..fx.ctx()
    };
    let rejection = screen
        .import(&ctx_banned, declared_synthetic_blueprint())
        .expect_err("a host-banned component must not be adopted");
    let ImportRejection::Invalid(verdict) = rejection else {
        panic!("expected Invalid, got {rejection:?}");
    };
    assert_eq!(
        banned_components(&verdict),
        vec![&id(ContentKind::Weapon, SYNTHETIC_MISSILE_KEY); 4]
    );
    assert_eq!(screen.blueprint(), &declared_synthetic_blueprint());

    // The ban cannot be bypassed through a commit either: the commit
    // re-validates whatever the draft holds, imported or not.
    let mut forced = ConstructionScreen::open(&state, declared_synthetic_blueprint(), vec![]);
    forced.edit(declared_synthetic_blueprint());
    assert!(matches!(
        forced.commit(&ctx_banned, &mut state),
        Err(ConstructionError::Invalid(_))
    ));

    // Under the permissive policy the same import is adopted — the rejection
    // above came from the rule, not from the path — and it commits.
    let verdict = screen
        .import(&fx.ctx(), paired_blueprint())
        .expect("the permissive policy adopts it");
    assert!(verdict.is_valid());
    assert_eq!(screen.blueprint(), &paired_blueprint());
    assert!(screen.is_dirty());
    let receipt = screen
        .commit(&fx.ctx(), &mut state)
        .expect("the adopted import commits under its rule");
    // The paired blueprint names the same six distinct components.
    assert_eq!(receipt.charged, DISTINCT_PRICE);
}

#[test]
fn accept_f44_c_the_screen_previews_then_commits_and_saves_one_revision() {
    let fx = Fixture::new();
    let base = TempBase::new("preview");
    let mut state = funded(&fx.graph);
    let mut session = sandbox_with_campaign(base.path(), &state);
    let stored_revision = stored_campaign(&session).revision;
    let base_revision = state.revision();
    assert_eq!(
        stored_revision, base_revision,
        "the stored campaign is the live one"
    );

    let mut screen = ConstructionScreen::open(&state, declared_synthetic_blueprint(), vec![]);

    // The preview is the validator's own numbers, normalized for the bars.
    let view = screen.view(&fx.ctx(), &state);
    assert!(view.verdict.as_ref().expect("measurable").is_valid());
    let meter = |quantity| {
        view.meters
            .iter()
            .find(|m| m.quantity == quantity)
            .copied()
            .expect("a measured quantity has a meter")
    };
    let mass = meter(BudgetQuantity::Mass);
    assert_eq!(mass.used, SYNTHETIC_BOUNDARY_MASS_LIMIT_UNITS);
    assert_eq!(mass.limit, SYNTHETIC_BOUNDARY_MASS_LIMIT_UNITS);
    assert_eq!(mass.permille, 1000, "exactly at the weight limit");
    let cost = meter(BudgetQuantity::Cost);
    assert_eq!(cost.used, SYNTHETIC_BOUNDARY_COST_LIMIT_MINOR);
    assert_eq!(cost.permille, 1000, "exactly at the price limit");
    let guns = meter(BudgetQuantity::GunPositions);
    assert_eq!((guns.used, guns.limit, guns.permille), (4, 4, 1000));
    let rockets = meter(BudgetQuantity::RocketHardpoints);
    assert_eq!((rockets.used, rockets.limit, rockets.permille), (4, 8, 500));
    assert!(!view.dirty);
    assert!(view.notice.is_none());
    assert!(view.committed.is_none());

    // The staged transaction is exactly what the commit will run.
    let pending = view.pending.expect("a priced draft has a pending commit");
    assert_eq!(pending.buys.len(), 6, "six distinct components");
    assert_eq!(pending.charge, DISTINCT_PRICE);
    assert_eq!(pending.credit, 0);
    assert!(pending.sells.is_empty());

    let receipt = screen
        .commit_saved(&fx.ctx(), &mut state, &mut session)
        .expect("the boundary blueprint commits and saves");
    assert_eq!(receipt.charged, DISTINCT_PRICE);
    assert_eq!(receipt.revision, base_revision + 1, "one revision once");
    assert_eq!(state.currency(), GRANT - DISTINCT_PRICE);
    assert_eq!(state.unlocks().count(), 6);

    // And it is durable: a fresh session over the same save sees the purchase.
    let stored = stored_campaign(&session);
    assert_eq!(stored.revision, base_revision + 1);
    assert_eq!(stored.currency, GRANT - DISTINCT_PRICE);
    assert_eq!(stored.unlocks.len(), 6);

    // After the commit the screen is clean and re-based: nothing is staged, so
    // a repeated commit is refused as the empty draft it is, not as a second
    // charge.
    assert!(!screen.is_dirty());
    assert_eq!(screen.committed(), Some(&receipt));
    let error = screen
        .commit(&fx.ctx(), &mut state)
        .expect_err("a committed screen stages nothing new");
    assert!(
        matches!(error, ConstructionError::Economy(EconomyError::EmptyDraft)),
        "got {error:?}"
    );
}

#[test]
fn accept_f44_c_a_refused_commit_leaves_the_draft_open_and_the_save_untouched() {
    let fx = Fixture::new();
    let base = TempBase::new("refused");
    let mut state = funded(&fx.graph);
    let mut session = sandbox_with_campaign(base.path(), &state);
    let before = state.snapshot();
    let stored_before = session.document().expect("doc").revision;

    let mut screen = ConstructionScreen::open(&state, declared_synthetic_blueprint(), vec![]);
    screen.edit(one_unit_overweight());

    // The view refuses the draft before any commit is attempted: the verdict
    // names the breach and nothing is staged.
    let view = screen.view(&fx.ctx(), &state);
    let verdict = view.verdict.expect("measurable");
    assert!(!verdict.is_valid());
    assert!(matches!(
        verdict.assessment().first_breach(),
        Some(cs_content::construction::LimitBreach::Mass { .. })
    ));
    assert!(view.pending.is_none(), "an invalid draft stages nothing");

    // The commit is refused; the state, the save and the draft are all left.
    let error = screen
        .commit_saved(&fx.ctx(), &mut state, &mut session)
        .expect_err("one unit over must not commit");
    assert!(
        matches!(
            error,
            ScreenSaveError::Refused(ConstructionError::Invalid(_))
        ),
        "got {error:?}"
    );
    assert_eq!(state.snapshot(), before, "the profile is unchanged");
    assert_eq!(
        session.document().expect("doc").revision,
        stored_before,
        "nothing was written"
    );
    assert!(screen.notice().is_some(), "the refusal is displayed");
    assert!(screen.is_dirty(), "the draft is still there to fix");

    // Retry: the player fixes the loadout and the same screen commits.
    screen.edit(declared_synthetic_blueprint());
    assert!(
        screen.notice().is_none(),
        "a new edit clears the old refusal"
    );
    let receipt = screen
        .commit_saved(&fx.ctx(), &mut state, &mut session)
        .expect("the fixed draft commits");
    assert_eq!(receipt.charged, DISTINCT_PRICE);
    assert_eq!(stored_campaign(&session).currency, GRANT - DISTINCT_PRICE);
}

#[test]
fn accept_f44_c_cancelling_the_screen_leaves_the_profile_and_the_save_unchanged() {
    let fx = Fixture::new();
    let base = TempBase::new("cancel");
    let state = funded(&fx.graph);
    let session = sandbox_with_campaign(base.path(), &state);
    let before = state.snapshot();
    let stored_before = session.document().expect("doc").revision;

    let mut screen = ConstructionScreen::open(&state, declared_synthetic_blueprint(), vec![]);
    screen.set_paint(alt_paint());
    screen.queue_sale(id(ContentKind::Armor, "fixture.synthetic_plate"));
    screen.edit(paired_blueprint());
    assert!(screen.is_dirty());
    screen.cancel();

    assert_eq!(state.snapshot(), before, "cancel changed the profile");
    assert_eq!(
        session.document().expect("doc").revision,
        stored_before,
        "cancel wrote to the save"
    );
    assert_eq!(state.currency(), GRANT);
    assert_eq!(state.unlocks().count(), 0);
}

#[test]
fn accept_f44_c_a_moved_or_damaged_save_is_refused_not_overwritten() {
    let fx = Fixture::new();
    let base = TempBase::new("moved");
    let mut state = funded(&fx.graph);
    let mut session = sandbox_with_campaign(base.path(), &state);
    let before = state.snapshot();

    // A concurrent writer commits a campaign revision this screen has not seen.
    let mut rival = before.clone();
    rival.revision += 5;
    rival.currency = 77;
    session
        .commit_with(|document| {
            write_snapshot(document, &rival).expect("the rival encodes");
            Ok(())
        })
        .expect("the rival commits");

    let mut screen = ConstructionScreen::open(&state, declared_synthetic_blueprint(), vec![]);
    let error = screen
        .commit_saved(&fx.ctx(), &mut state, &mut session)
        .expect_err("a moved save is refused");
    assert!(
        matches!(
            error,
            ScreenSaveError::Save(CampaignSaveError::Stale {
                expected,
                stored
            }) if expected == before.revision && stored == rival.revision
        ),
        "got {error:?}"
    );
    assert_eq!(state.snapshot(), before, "the in-memory state is unchanged");
    assert_eq!(
        stored_campaign(&session),
        rival,
        "the rival's work survives"
    );

    // A damaged stored campaign is reported, not overwritten.
    session
        .commit_with(|document| {
            document.extra.push(ExtraField {
                key: "campaign.unrecognized".to_owned(),
                value: "x".to_owned(),
            });
            Ok(())
        })
        .expect("committed");
    let error = screen
        .commit_saved(&fx.ctx(), &mut state, &mut session)
        .expect_err("a damaged save is refused");
    assert!(
        matches!(error, ScreenSaveError::Save(CampaignSaveError::Corrupt(_))),
        "got {error:?}"
    );
    assert_eq!(state.snapshot(), before);
}

#[test]
fn accept_f44_c_paint_edits_stay_references_through_export_and_import() {
    let fx = Fixture::new();
    let mut state = funded(&fx.graph);

    let mut screen = ConstructionScreen::open(&state, declared_synthetic_blueprint(), vec![]);
    screen.set_paint(alt_paint());
    let view = screen.view(&fx.ctx(), &state);
    // The view's paint is the selection's references — the renderer resolves
    // them through the livery path; nothing textured is in the record.
    assert_eq!(view.blueprint.paint(), &alt_paint());
    assert!(view.verdict.expect("measurable").is_valid());

    // The export is the draft record itself: references only. Importing it
    // into another screen goes through the same validator and is adopted.
    let exported = screen.export();
    assert_eq!(exported.paint(), &alt_paint());
    let mut other = ConstructionScreen::open(&state, declared_synthetic_blueprint(), vec![]);
    other
        .import(&fx.ctx(), exported)
        .expect("a valid export imports");
    assert_eq!(other.blueprint().paint(), &alt_paint());
    let receipt = other
        .commit(&fx.ctx(), &mut state)
        .expect("the imported blueprint commits");
    // Paint is not a priced component: the charge is still the six parts.
    assert_eq!(receipt.charged, DISTINCT_PRICE);
}

#[test]
fn accept_f44_c_a_queued_sale_previews_its_refund_and_commits_once() {
    let fx = Fixture::new();
    let base = TempBase::new("sale");
    let mut state = funded(&fx.graph);
    let mut session = sandbox_with_campaign(base.path(), &state);

    // Buy the whole boundary blueprint first, so the profile owns every
    // component it names and has paid a recorded price for each.
    let mut buyer = ConstructionScreen::open(&state, declared_synthetic_blueprint(), vec![]);
    buyer
        .commit_saved(&fx.ctx(), &mut state, &mut session)
        .expect("the boundary blueprint buys");
    assert_eq!(state.unlocks().count(), 6);
    let after_buy = state.snapshot();

    let radio = id(ContentKind::HardpointEquipment, SYNTHETIC_RADIO_KEY);
    let paid = state
        .paid_for(&radio)
        .expect("the profile recorded what it paid for the radio");

    // A blueprint that no longer uses the radio, staging its sale.
    let no_equipment = declared_synthetic_blueprint()
        .with_equipment(Vec::new())
        .expect("a blueprint without equipment is still valid");
    let mut screen = ConstructionScreen::open(&state, no_equipment.clone(), vec![]);
    screen.queue_sale(radio.clone());
    assert!(screen.is_dirty());

    // The preview is the transaction the commit will run: nothing to buy (the
    // profile owns every part) and the refund the sale credits.
    let view = screen.view(&fx.ctx(), &state);
    assert!(view.verdict.expect("measurable").is_valid());
    let pending = view.pending.expect("a priced draft has a pending commit");
    assert!(pending.buys.is_empty(), "the profile owns every part");
    assert_eq!(pending.charge, 0);
    assert_eq!(pending.sells, vec![radio.clone()]);
    assert_eq!(
        pending.credit, paid,
        "the previewed credit is the refund the commit pays"
    );

    // The same sale staged twice is refused rather than refunded twice: the
    // economy rejects repeated lines and the profile is exactly as it was.
    let mut doubled = ConstructionScreen::open(&state, no_equipment, vec![]);
    doubled.queue_sale(radio.clone());
    doubled.queue_sale(radio.clone());
    let error = doubled
        .commit(&fx.ctx(), &mut state)
        .expect_err("one sale staged twice must not commit");
    assert!(
        matches!(
            error,
            ConstructionError::Economy(EconomyError::ConflictingLines { .. })
        ),
        "got {error:?}"
    );
    assert_eq!(
        state.snapshot(),
        after_buy,
        "the refused sale moved nothing"
    );

    // The one staged sale commits once, in memory and on the save.
    let receipt = screen
        .commit_saved(&fx.ctx(), &mut state, &mut session)
        .expect("the sale commits");
    assert_eq!(receipt.charged, 0);
    assert_eq!(receipt.refunded, paid);
    assert_eq!(state.unlocks().count(), 5, "the radio was sold");
    assert_eq!(state.currency(), GRANT - DISTINCT_PRICE + paid);

    let stored = stored_campaign(&session);
    assert_eq!(stored.revision, after_buy.revision + 1, "one more revision");
    assert_eq!(stored.currency, GRANT - DISTINCT_PRICE + paid);
    assert_eq!(stored.unlocks.len(), 5, "the sale is durable");
    assert!(!screen.is_dirty(), "the committed screen is clean again");
    assert_eq!(screen.committed(), Some(&receipt));
}

#[test]
fn accept_f44_c_an_unmeasurable_import_is_refused_not_adopted() {
    let fx = Fixture::new();
    let state = funded(&fx.graph);
    let mut screen = ConstructionScreen::open(&state, declared_synthetic_blueprint(), vec![]);

    // The host's pairing rule is unmeasured, so a paired import cannot be
    // judged at all — and "cannot be judged" is a refusal, never an adoption
    // by default. The draft, the dirty flag and the state are untouched.
    let unmeasured = ConstructionPolicy::new(
        Resolved::unknown(
            ClaimId::new("f44c.test.pairing").expect("valid claim id"),
            "the original pairing rule is unmeasured",
        )
        .expect("a reason is present"),
        BTreeSet::new(),
        fx.policy.available().clone(),
    );
    let ctx = ConstructionContext {
        policy: &unmeasured,
        ..fx.ctx()
    };
    let rejection = screen
        .import(&ctx, paired_blueprint())
        .expect_err("an unmeasurable import must not be adopted");
    let ImportRejection::Refused(refusal) = rejection else {
        panic!("expected Refused, got {rejection:?}");
    };
    assert!(
        matches!(refusal, ValidationRefusal::UnknownPairingRule { .. }),
        "got {refusal:?}"
    );
    assert_eq!(screen.blueprint(), &declared_synthetic_blueprint());
    assert!(!screen.is_dirty());
    assert!(screen.notice().is_some(), "the refusal is displayed");
}
