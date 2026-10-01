//! Acceptance scenario F27-A: the declared weapon, ammunition and loadout
//! schema validates its identity discipline and keeps unknowns explicit.
//!
//! Spec: `specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`,
//! stage `### F27-A`. Task test prefix: `accept_f27_a_`.
//!
//! These tests drive production code only:
//! [`cs_content::weapons`]'s [`DeclaredGunDefinition::try_new`],
//! [`DeclaredAmmunition::try_new`], [`DeclaredLoadout::try_new`] and the
//! `declared_synthetic_*` fixtures. Removing the validation, or defaulting
//! an unknown instead of preserving it, fails a test or fails to compile.
//!
//! Every value here is newly authored synthetic fixture data, never
//! original game data.

use cs_content::damage::DamageNodeKey;
use cs_content::weapons::{
    AmmunitionId, DeclaredAmmunition, DeclaredCaliber, DeclaredDamageChannel,
    DeclaredFriendlyFireRule, DeclaredGunDefinition, DeclaredGunMountKind, DeclaredGunRate,
    DeclaredInheritanceRule, DeclaredLoadout, DeclaredSelfHitRule, DeclaredSpreadCone,
    DeclaredWeaponDamage, InteractionRules, LoadoutSchemaError, WeaponSchemaError,
    declared_synthetic_ammunition, declared_synthetic_gun, declared_synthetic_loadout,
    synthetic_ammunition_id, synthetic_effect_id, synthetic_sound_id,
};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::space::Radians;

fn claim() -> ClaimId {
    ClaimId::new("f27a.test-schema").expect("a valid claim id")
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, Provenance::designed(claim())))
}

fn unknown<T>() -> Resolved<T> {
    Resolved::unknown(claim(), "the original value is unmeasured").expect("a nonempty reason")
}

fn mount() -> DamageNodeKey {
    DamageNodeKey::new("gun_mount_1").expect("a valid mount key")
}

fn caliber() -> Resolved<DeclaredCaliber> {
    known(DeclaredCaliber::try_new("test caliber").expect("a valid caliber"))
}

fn damage() -> DeclaredWeaponDamage {
    DeclaredWeaponDamage {
        armor: known(6.0),
        internal: known(3.0),
    }
}

fn rules() -> InteractionRules {
    InteractionRules {
        self_hit: known(DeclaredSelfHitRule::Excluded),
        friendly_fire: known(DeclaredFriendlyFireRule::HostileOnly),
        penetration: known(false),
        ricochet: known(false),
        ammo_switching: known(false),
    }
}

fn gun_id() -> ContentId {
    ContentId::from_source(ContentKind::Weapon, "test.gun").expect("a valid content id")
}

/// A fully known declared gun, with the individual fields a test overrides.
struct GunParts {
    gun: ContentId,
    mount: DamageNodeKey,
    caliber: Resolved<DeclaredCaliber>,
    ammunition: Resolved<AmmunitionId>,
    rate: Resolved<DeclaredGunRate>,
    muzzle_velocity_mps: Resolved<f64>,
    lifetime_ticks: Resolved<u64>,
    spread: DeclaredSpreadCone,
    damage: DeclaredWeaponDamage,
    inheritance: Resolved<DeclaredInheritanceRule>,
    effect: Resolved<ContentId>,
    sound: Resolved<ContentId>,
    rules: InteractionRules,
}

impl Default for GunParts {
    fn default() -> Self {
        Self {
            gun: gun_id(),
            mount: mount(),
            caliber: caliber(),
            ammunition: known(synthetic_ammunition_id()),
            rate: known(DeclaredGunRate {
                ticks_between_shots: 4,
            }),
            muzzle_velocity_mps: known(640.0),
            lifetime_ticks: known(90),
            spread: DeclaredSpreadCone {
                half_angle: known(Radians(0.004)),
            },
            damage: damage(),
            inheritance: known(DeclaredInheritanceRule::Full),
            effect: known(synthetic_effect_id()),
            sound: known(synthetic_sound_id()),
            rules: rules(),
        }
    }
}

impl GunParts {
    fn build(self) -> Result<DeclaredGunDefinition, WeaponSchemaError> {
        DeclaredGunDefinition::try_new(
            self.gun,
            Origin::SyntheticFixture,
            self.mount,
            DeclaredGunMountKind::Nose,
            None,
            self.caliber,
            self.ammunition,
            self.rate,
            self.muzzle_velocity_mps,
            self.lifetime_ticks,
            self.spread,
            self.damage,
            self.inheritance,
            self.effect,
            self.sound,
            self.rules,
            Provenance::designed(claim()),
        )
    }
}

/// The declared synthetic fixtures assemble, and each one says what it is:
/// synthetic-fixture origin, designed provenance, and no claim to be
/// original data.
#[test]
fn accept_f27_a_synthetic_fixture_validates_and_carries_provenance() {
    let gun = declared_synthetic_gun();
    assert_eq!(gun.gun().as_str(), "weapon/synthetic.fixture_gun");
    assert_eq!(gun.origin(), &Origin::SyntheticFixture);
    assert_eq!(
        gun.mount(),
        &mount(),
        "the gun is on a weapon-mount damage node"
    );
    assert_eq!(gun.mount_kind(), DeclaredGunMountKind::Nose);
    assert_eq!(gun.known_caliber(), Some("synthetic fixture caliber"));
    assert_eq!(
        gun.known_ammunition(),
        Some(&synthetic_ammunition_id()),
        "ammunition is a catalog id, not an enum variant"
    );
    assert_eq!(
        gun.known_effect().expect("known").kind(),
        ContentKind::HardpointEquipment
    );
    assert_eq!(gun.known_sound().expect("known").kind(), ContentKind::Sound);
    assert_eq!(gun.known_inheritance(), Some(DeclaredInheritanceRule::Full));
    // Every interaction rule is Known — with *designed* provenance, which is
    // the point: a synthetic fixture supplies a value so a session has
    // something to run under, never as evidence.
    for (_, resolved) in gun.damage().entries() {
        assert!(resolved.is_known());
    }
    assert!(gun.rules().self_hit.is_known());
    assert!(gun.rules().friendly_fire.is_known());
    assert!(gun.rules().penetration.is_known());
    assert!(gun.rules().ricochet.is_known());
    assert!(gun.rules().ammo_switching.is_known());

    let ammunition = declared_synthetic_ammunition();
    assert_eq!(
        ammunition.ammunition().as_str(),
        "ammo/synthetic.fixture_slug"
    );
    assert_eq!(ammunition.origin(), &Origin::SyntheticFixture);
    assert!(ammunition.known_caliber().is_some());
    assert!(ammunition.rules().self_hit.is_known());

    let loadout = declared_synthetic_loadout();
    assert_eq!(
        loadout.subject().as_str(),
        "loadout/synthetic.fixture_loadout"
    );
    assert_eq!(loadout.origin(), &Origin::SyntheticFixture);
    assert_eq!(loadout.guns().len(), 1);
    assert_eq!(loadout.ammunition().len(), 1);
    assert_eq!(
        loadout.pairings().len(),
        1,
        "one gun with one type is one pairing for F27-D's audit to walk"
    );
}

/// The identity discipline: a gun is a `weapon`, an effect is
/// `hardpoint_equipment`, a sound is a `sound` and ammunition is `ammo` —
/// each namespace is checked, so a mistyped id is refused at the schema
/// rather than discovered at the boundary.
#[test]
fn accept_f27_a_schema_refuses_wrong_namespace_identities() {
    // A mission id is not a gun.
    let mission = ContentId::from_source(ContentKind::Mission, "m01").expect("valid");
    assert_eq!(
        GunParts {
            gun: mission.clone(),
            ..GunParts::default()
        }
        .build(),
        Err(WeaponSchemaError::GunKindMismatch { id: mission })
    );

    // A sound id is not a muzzle effect.
    assert_eq!(
        GunParts {
            effect: known(synthetic_sound_id()),
            ..GunParts::default()
        }
        .build(),
        Err(WeaponSchemaError::EffectKindMismatch {
            id: synthetic_sound_id()
        })
    );

    // A gun id is not a sound.
    assert_eq!(
        GunParts {
            sound: known(gun_id()),
            ..GunParts::default()
        }
        .build(),
        Err(WeaponSchemaError::SoundKindMismatch { id: gun_id() })
    );

    // An ammunition id outside the `ammo` namespace is refused.
    assert_eq!(
        AmmunitionId::try_new(gun_id()),
        Err(WeaponSchemaError::AmmoKindMismatch { id: gun_id() }),
        "ammunition identity is a namespaced catalog id, not free text"
    );

    // A loadout subject must be a loadout, its guns must be weapons and must
    // not repeat.
    assert_eq!(
        DeclaredLoadout::try_new(
            gun_id(),
            Origin::SyntheticFixture,
            vec![gun_id()],
            vec![synthetic_ammunition_id()],
            Provenance::designed(claim()),
        ),
        Err(LoadoutSchemaError::SubjectKindMismatch { id: gun_id() })
    );
    assert_eq!(
        DeclaredLoadout::try_new(
            ContentId::from_source(ContentKind::Loadout, "test.loadout").expect("valid"),
            Origin::SyntheticFixture,
            vec![],
            vec![synthetic_ammunition_id()],
            Provenance::designed(claim()),
        ),
        Err(LoadoutSchemaError::EmptyLoadout),
        "a loadout with no gun fires nothing"
    );
    assert_eq!(
        DeclaredLoadout::try_new(
            ContentId::from_source(ContentKind::Loadout, "test.loadout").expect("valid"),
            Origin::SyntheticFixture,
            vec![gun_id(), gun_id()],
            vec![synthetic_ammunition_id()],
            Provenance::designed(claim()),
        ),
        Err(LoadoutSchemaError::DuplicateGun { id: gun_id() }),
        "the same gun twice is one gun with a doubled slot, not two guns"
    );
    assert_eq!(
        DeclaredLoadout::try_new(
            ContentId::from_source(ContentKind::Loadout, "test.loadout").expect("valid"),
            Origin::SyntheticFixture,
            vec![gun_id()],
            vec![synthetic_ammunition_id(), synthetic_ammunition_id()],
            Provenance::designed(claim()),
        ),
        Err(LoadoutSchemaError::DuplicateAmmunition {
            id: "ammo/synthetic.fixture_slug".to_owned()
        })
    );
}

/// The declared records refuse corrupt *known* values: a caliber that is
/// empty or over-long, a zero rate, a non-finite or non-positive muzzle
/// velocity, a zero lifetime, a corrupt damage amount, an out-of-range
/// spread cone and an out-of-range inheritance share.
#[test]
fn accept_f27_a_schema_refuses_corrupt_known_values() {
    // A caliber must be non-empty and bounded.
    assert_eq!(
        DeclaredCaliber::try_new("   "),
        Err(WeaponSchemaError::EmptyCaliber)
    );
    assert!(matches!(
        DeclaredCaliber::try_new(&"c".repeat(65)),
        Err(WeaponSchemaError::CaliberTooLong { len: 65 })
    ));

    // A zero rate interval.
    assert_eq!(
        GunParts {
            rate: known(DeclaredGunRate {
                ticks_between_shots: 0
            }),
            ..GunParts::default()
        }
        .build(),
        Err(WeaponSchemaError::ZeroRateInterval)
    );

    // Non-finite and non-positive muzzle velocities.
    assert_eq!(
        GunParts {
            muzzle_velocity_mps: known(f64::NAN),
            ..GunParts::default()
        }
        .build(),
        Err(WeaponSchemaError::NonFiniteMuzzleVelocity)
    );
    assert_eq!(
        GunParts {
            muzzle_velocity_mps: known(-1.0),
            ..GunParts::default()
        }
        .build(),
        Err(WeaponSchemaError::NonPositiveMuzzleVelocity {
            muzzle_velocity_mps: -1.0
        })
    );

    // A zero lifetime.
    assert_eq!(
        GunParts {
            lifetime_ticks: known(0),
            ..GunParts::default()
        }
        .build(),
        Err(WeaponSchemaError::ZeroLifetime)
    );

    // Corrupt damage, naming the channel that carried it.
    assert_eq!(
        GunParts {
            damage: DeclaredWeaponDamage {
                armor: known(f64::INFINITY),
                internal: known(3.0),
            },
            ..GunParts::default()
        }
        .build(),
        Err(WeaponSchemaError::NonFiniteDamage {
            channel: DeclaredDamageChannel::Armor
        })
    );
    assert_eq!(
        GunParts {
            damage: DeclaredWeaponDamage {
                armor: known(6.0),
                internal: known(-2.0),
            },
            ..GunParts::default()
        }
        .build(),
        Err(WeaponSchemaError::NegativeDamage {
            channel: DeclaredDamageChannel::Internal,
            amount: -2.0
        })
    );

    // An out-of-range or non-finite spread cone.
    assert_eq!(
        GunParts {
            spread: DeclaredSpreadCone {
                half_angle: known(Radians(2.0)),
            },
            ..GunParts::default()
        }
        .build(),
        Err(WeaponSchemaError::SpreadOutOfRange {
            half_angle_radians: 2.0
        })
    );
    assert_eq!(
        GunParts {
            spread: DeclaredSpreadCone {
                half_angle: known(Radians(f64::NAN)),
            },
            ..GunParts::default()
        }
        .build(),
        Err(WeaponSchemaError::NonFiniteSpread)
    );

    // An inheritance share outside [0, 1].
    assert_eq!(
        GunParts {
            inheritance: known(DeclaredInheritanceRule::Fraction { share: 1.5 }),
            ..GunParts::default()
        }
        .build(),
        Err(WeaponSchemaError::InvalidInheritanceShare { share: 1.5 })
    );
    assert!(
        GunParts {
            inheritance: known(DeclaredInheritanceRule::Fraction { share: 1.0 }),
            ..GunParts::default()
        }
        .build()
        .is_ok()
    );

    // The ammunition record refuses the same corrupt damage.
    assert_eq!(
        DeclaredAmmunition::try_new(
            synthetic_ammunition_id(),
            Origin::SyntheticFixture,
            caliber(),
            DeclaredWeaponDamage {
                armor: known(-1.0),
                internal: known(3.0),
            },
            rules(),
            Provenance::designed(claim()),
        ),
        Err(WeaponSchemaError::NegativeDamage {
            channel: DeclaredDamageChannel::Armor,
            amount: -1.0
        })
    );
}

/// The heart of F27 non-negotiable 1: an unmeasured value is an *explicit
/// unknown* carrying its claim and reason — never a silent default, never a
/// multiplier table standing in for the original catalogue.
#[test]
fn accept_f27_a_unknowns_survive_as_explicit_unknowns() {
    // Every ballistic parameter unknown at once is still a *valid declared
    // record*: the schema records what it does not know and the lowering
    // boundary (cs_app) is what refuses it.
    let declared = GunParts {
        caliber: unknown(),
        ammunition: unknown(),
        rate: unknown(),
        muzzle_velocity_mps: unknown(),
        lifetime_ticks: unknown(),
        spread: DeclaredSpreadCone {
            half_angle: unknown(),
        },
        damage: DeclaredWeaponDamage {
            armor: unknown(),
            internal: unknown(),
        },
        inheritance: unknown(),
        effect: unknown(),
        sound: unknown(),
        rules: InteractionRules {
            self_hit: unknown(),
            friendly_fire: unknown(),
            penetration: unknown(),
            ricochet: unknown(),
            ammo_switching: unknown(),
        },
        ..GunParts::default()
    }
    .build()
    .expect("a record of unknowns is a valid declared record");

    assert!(!declared.caliber().is_known());
    assert!(!declared.ammunition().is_known());
    assert!(!declared.rate().is_known());
    assert!(!declared.muzzle_velocity_mps().is_known());
    assert!(!declared.lifetime_ticks().is_known());
    assert!(!declared.spread().half_angle.is_known());
    assert!(!declared.inheritance().is_known());
    assert!(!declared.effect().is_known());
    assert!(!declared.sound().is_known());
    for (_, resolved) in declared.damage().entries() {
        assert!(
            !resolved.is_known(),
            "a damage channel may be unknown, never defaulted"
        );
    }
    assert!(!declared.rules().self_hit.is_known());
    assert!(!declared.rules().friendly_fire.is_known());
    assert!(!declared.rules().penetration.is_known());
    assert!(!declared.rules().ricochet.is_known());
    assert!(!declared.rules().ammo_switching.is_known());

    // The claim and the reason survive with the unknown, so a refusal can
    // name what it is refusing rather than reporting a bare "missing".
    let Resolved::Unknown { claim_id, reason } = declared.muzzle_velocity_mps() else {
        panic!("the muzzle velocity was declared unknown");
    };
    assert_eq!(claim_id, &claim());
    assert_eq!(reason, "the original value is unmeasured");

    // An unknown effect or sound id is *not* a namespace violation: the
    // schema only checks a namespace it actually has a value for, so an
    // unknown never trips the wrong-kind check.
    assert!(
        GunParts {
            effect: unknown(),
            sound: unknown(),
            ..GunParts::default()
        }
        .build()
        .is_ok(),
        "an unresolved id carries no namespace to check"
    );

    // The ammunition record keeps its unknowns too.
    let ammunition = DeclaredAmmunition::try_new(
        synthetic_ammunition_id(),
        Origin::SyntheticFixture,
        unknown(),
        DeclaredWeaponDamage {
            armor: unknown(),
            internal: known(3.0),
        },
        InteractionRules {
            self_hit: unknown(),
            friendly_fire: unknown(),
            penetration: unknown(),
            ricochet: unknown(),
            ammo_switching: unknown(),
        },
        Provenance::designed(claim()),
    )
    .expect("a record of unknowns is valid");
    assert!(!ammunition.caliber().is_known());
    assert!(
        ammunition
            .known_damage(DeclaredDamageChannel::Armor)
            .is_none()
    );
    assert_eq!(
        ammunition.known_damage(DeclaredDamageChannel::Internal),
        Some(3.0),
        "one known channel and one unknown channel coexist without a default"
    );
    assert!(!ammunition.rules().penetration.is_known());
}

/// The declared mount vocabulary and the runtime-facing shape of the
/// damage profile: every channel is addressable by its own label, and the
/// interaction rules are five separate options rather than one flag.
#[test]
fn accept_f27_a_mount_and_damage_channel_vocabularies_are_explicit() {
    assert_eq!(DeclaredGunMountKind::ALL.len(), 5);
    assert!(
        DeclaredGunMountKind::ALL
            .iter()
            .filter(|kind| kind.is_wing())
            .count()
            == 2,
        "two wing kinds exist so AC02's disabled wing gun is expressible"
    );
    for kind in DeclaredGunMountKind::ALL {
        assert_eq!(kind.label().len(), kind.to_string().len());
        assert!(!kind.label().is_empty());
    }

    // The channel labels are the stable keys a report and a lowering map use.
    assert_eq!(DeclaredDamageChannel::ALL.len(), 2);
    assert_eq!(DeclaredDamageChannel::Armor.label(), "armor");
    assert_eq!(DeclaredDamageChannel::Internal.label(), "internal");

    let profile = damage();
    assert_eq!(
        profile.known_amount(DeclaredDamageChannel::Armor),
        Some(6.0)
    );
    assert_eq!(
        profile.known_amount(DeclaredDamageChannel::Internal),
        Some(3.0)
    );
    assert_eq!(
        profile
            .entries()
            .iter()
            .map(|(label, _)| *label)
            .collect::<Vec<_>>(),
        vec!["armor", "internal"],
        "a damage profile is addressable per channel in a stable order"
    );
}

/// The loadout's pairings are the rows F27-D's ammunition audit walks: every
/// declared gun with every declared ammunition type, so an audit cannot
/// quietly cover only one pairing.
#[test]
fn accept_f27_a_loadout_pairings_cover_every_gun_with_every_type() {
    let second_gun =
        ContentId::from_source(ContentKind::Weapon, "test.gun_two").expect("a valid content id");
    let second_ammo = AmmunitionId::try_new(
        ContentId::from_source(ContentKind::Ammo, "test.ammo_two").expect("a valid content id"),
    )
    .expect("a valid ammunition id");
    let loadout = DeclaredLoadout::try_new(
        ContentId::from_source(ContentKind::Loadout, "test.loadout").expect("valid"),
        Origin::SyntheticFixture,
        vec![gun_id(), second_gun.clone()],
        vec![synthetic_ammunition_id(), second_ammo.clone()],
        Provenance::designed(claim()),
    )
    .expect("a valid loadout");

    let pairings = loadout.pairings();
    assert_eq!(
        pairings.len(),
        4,
        "two guns with two types is four pairings an audit must cover"
    );
    assert_eq!(
        pairings
            .iter()
            .map(|(gun, ammo)| (gun.as_str(), ammo.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("weapon/test.gun", "ammo/synthetic.fixture_slug"),
            ("weapon/test.gun", "ammo/test.ammo_two"),
            ("weapon/test.gun_two", "ammo/synthetic.fixture_slug"),
            ("weapon/test.gun_two", "ammo/test.ammo_two"),
        ],
        "every gun is paired with every declared ammunition type"
    );

    // A loadout carrying one type declares exactly one pairing per gun, so
    // adding a gun to an audit's scope cannot silently drop the others.
    let single = declared_synthetic_loadout();
    assert_eq!(
        single.pairings().len(),
        single.guns().len() * single.ammunition().len()
    );
}
