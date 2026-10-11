//! Task #553 (F28-AE2): the session's supported ordnance catalogue gates
//! every registration, so an import cannot register a component the catalogue
//! does not hold (F28 non-negotiable 5).
//!
//! Task test prefix: `accept_f28_ae2_`. The refusals are driven through
//! `OrdnanceSession::register` / `register_installed`, the production entry
//! point. Every value is newly authored synthetic fixture data.

use std::collections::BTreeSet;

use cs_app::ordnance::{OrdnanceRegistrationError, OrdnanceSession, lower_ordnance};
use cs_content::ordnance::{
    DeclaredEquipmentRules, DeclaredOrdnance, DeclaredOrdnanceDetails, DeclaredOrdnanceFamily,
    declared_synthetic_guided, declared_synthetic_provenance,
};
use cs_sim::damage::ActorId;
use cs_sim::time::TickRate;
use cs_sim::weapons::{CompatibilityVerdict, OrdnanceRegistry, OrdnanceRegistryError};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::net::SessionId;

const SESSION: u64 = 53;

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: SessionId::new(SESSION).expect("nonzero"),
        serial,
    }
}

fn weapon(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::Weapon, key).expect("a weapon id")
}

/// The guided fixture under another id and with the given equipment rules.
fn guided_as(key: &str, rules: DeclaredEquipmentRules) -> DeclaredOrdnance {
    let fixture = declared_synthetic_guided();
    let mut projectile = match fixture.details() {
        DeclaredOrdnanceDetails::Projectile(projectile) => (**projectile).clone(),
        DeclaredOrdnanceDetails::Nitro(_) => panic!("the guided fixture is a projectile"),
    };
    projectile.equipment_rules = rules;
    DeclaredOrdnance::try_new(
        weapon(key),
        Origin::SyntheticFixture,
        DeclaredOrdnanceFamily::GuidedRocket,
        DeclaredOrdnanceDetails::Projectile(Box::new(projectile)),
        None,
        declared_synthetic_provenance(),
    )
    .expect("the retargeted guided fixture is valid")
}

fn plain(key: &str) -> DeclaredOrdnance {
    guided_as(key, DeclaredEquipmentRules::default())
}

fn catalogue_of(records: &[&DeclaredOrdnance]) -> OrdnanceRegistry {
    let mut catalogue = OrdnanceRegistry::new();
    for record in records {
        catalogue
            .register(lower_ordnance(record).expect("a fixture lowers"))
            .expect("distinct ids");
    }
    catalogue
}

fn session(catalogue: OrdnanceRegistry) -> OrdnanceSession {
    OrdnanceSession::new(
        SESSION,
        Tick(0),
        TickRate::new(30).expect("nonzero"),
        7,
        catalogue,
    )
    .expect("a nonzero session opens")
}

fn known(id: ContentId) -> Resolved<ContentId> {
    Resolved::Known(Known::new(
        id,
        Provenance::designed(ClaimId::new("f28ae2.test").expect("a claim id")),
    ))
}

#[test]
fn accept_f28_ae2_unknown_id_is_refused_by_name_and_registers_nothing() {
    let supported = plain("supported.rocket");
    let smuggled = plain("smuggled.rocket");
    let mut session = session(catalogue_of(&[&supported]));

    let refusal = session
        .register(actor(1), &[supported.clone(), smuggled.clone()])
        .expect_err("an id outside the catalogue is refused");
    let OrdnanceRegistrationError::Unsupported(OrdnanceRegistryError::UnknownOrdnance { ordnance }) =
        refusal
    else {
        panic!("expected a named unsupported refusal, got {refusal:?}");
    };
    assert_eq!(ordnance.as_str(), weapon("smuggled.rocket").as_str());
    assert!(
        session.registered_ids(&actor(1)).is_empty(),
        "the supported record of the refused batch is not registered either"
    );

    session
        .register(actor(1), &[supported])
        .expect("the supported record alone registers");
    assert_eq!(session.registered_ids(&actor(1)).len(), 1);
}

#[test]
fn accept_f28_ae2_empty_catalogue_supports_nothing() {
    let record = plain("any.rocket");
    let mut session = session(OrdnanceRegistry::new());
    assert!(matches!(
        session.register(actor(1), &[record]),
        Err(OrdnanceRegistrationError::Unsupported(
            OrdnanceRegistryError::UnknownOrdnance { .. }
        ))
    ));
    assert!(session.registered_shooters().is_empty());
}

#[test]
fn accept_f28_ae2_repeated_id_is_refused_and_registers_nothing() {
    let record = plain("twin.rocket");
    let mut session = session(catalogue_of(&[&record]));

    assert!(matches!(
        session.register(actor(1), &[record.clone(), record.clone()]),
        Err(OrdnanceRegistrationError::DuplicateOrdnance { .. })
    ));
    assert!(session.registered_ids(&actor(1)).is_empty());

    session
        .register(actor(1), std::slice::from_ref(&record))
        .expect("one copy registers");
    assert!(matches!(
        session.register(actor(1), &[record]),
        Err(OrdnanceRegistrationError::DuplicateOrdnance { .. })
    ));
    assert_eq!(session.registered_ids(&actor(1)).len(), 1);
}

#[test]
fn accept_f28_ae2_equipment_verdicts_reach_the_registration() {
    let needy = guided_as(
        "needy.rocket",
        DeclaredEquipmentRules {
            requires: Some(known(weapon("rack.heavy"))),
            forbids: vec![known(weapon("rack.light"))],
        },
    );
    let mut session = session(catalogue_of(&[&needy]));

    let bare = BTreeSet::new();
    let refusal = session
        .register_installed(actor(1), std::slice::from_ref(&needy), &bare)
        .expect_err("a missing required item refuses");
    assert!(matches!(
        refusal,
        OrdnanceRegistrationError::Incompatible {
            verdict: CompatibilityVerdict::MissingRequired { .. },
            ..
        }
    ));

    let both = BTreeSet::from([weapon("rack.heavy"), weapon("rack.light")]);
    assert!(matches!(
        session.register_installed(actor(1), std::slice::from_ref(&needy), &both),
        Err(OrdnanceRegistrationError::Incompatible {
            verdict: CompatibilityVerdict::Forbidden { .. },
            ..
        })
    ));
    assert!(session.registered_ids(&actor(1)).is_empty());

    let fitted = BTreeSet::from([weapon("rack.heavy")]);
    session
        .register_installed(actor(1), &[needy], &fitted)
        .expect("a compatible airframe registers");
    assert_eq!(session.registered_ids(&actor(1)).len(), 1);
}
