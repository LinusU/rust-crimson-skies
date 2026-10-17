//! The content-side lobby loadout judgement (F55-BU2).
//!
//! Spec: `specs/F55-multiplayer-lobby-host-rules-readiness-and-ux.md`; shared
//! contracts `docs/contracts/UI-NETWORK.md` ("Lobby host changes rules through
//! a validated server action") and `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! `cs_net::lobby::LoadoutValidator` is the hook the host's lobby judges a
//! member's loadout through. `cs_net` may not depend on the content crates
//! (AGENTS rule 7 and the `cs_content` manifest's "test-only edge" comment), so
//! the production implementation lives on this side of the boundary. This
//! module is the content-side half: [`LobbyConstructionCatalog::judge`] turns
//! one lobby loadout into a verdict from the shared
//! [`ConstructionRules::validate`] (F44-B) and one stable reason code. The
//! thin `LoadoutValidator` adapter is
//! `cs_app::ui::lobby::ConstructionLoadoutValidator`.
//!
//! ## The mapping, and why it is a designed rule
//!
//! A lobby loadout is the flat id list `cs_net::lobby::Loadout` carries — a
//! blueprint id and the component ids the member would fly. The shared
//! construction validator judges an [`AircraftBlueprint`] instead: it needs the
//! airframe, the per-zone armor fitments, the weapon mounts and the hardpoints,
//! none of which are in a flat id list. No authored blueprint-id ->
//! `AircraftBlueprint` table exists in this crate (task #764's finding), and
//! inventing fitment nodes from ids would be a guess (AGENTS rule 4).
//!
//! This module therefore does **not** reconstruct a blueprint from ids.
//! Instead the mapping is a declared rule, stated once here and enforced
//! mechanically:
//!
//! 1. The loadout's blueprint id names an **authored** [`AircraftBlueprint`]
//!    the caller supplied in the [`LobbyConstructionCatalog`]. A loadout whose
//!    blueprint is not authored is refused, never passed as if it were.
//! 2. The loadout's component list must name exactly that blueprint's fitted
//!    components ([`AircraftBlueprint::components`]), compared as the **set of
//!    distinct ids** the loadout carries (the lobby's own shape check already
//!    refuses a repeated component, so a lobby loadout is a set, not a
//!    multiset). A client may order its own list but may not claim a component
//!    the authored blueprint does not carry, nor omit one it does (for example,
//!    a smuggled heavier engine). A blueprint's fitments are the only place its
//!    per-fitment nodes, position counts and categories are declared, so a
//!    mismatch cannot be judged and is refused.
//! 3. The authored blueprint is then judged by the shared
//!    [`ConstructionRules::validate`] with the catalog's
//!    [`ConstructionPolicy`] (pairing rule, availability, bans) and
//!    [`PriceBook`], and its verdict is lowered to one code.
//!
//! The limited part is deliberate and recorded: the caller must hold the
//! authored blueprint for every id a lobby may name. Where that authored table
//! is provisioned from the original installation is F44-D's measurement, not a
//! value invented here; until it is populated, a lobby wired to this validator
//! refuses blueprints it does not hold (`unknown_blueprint`) rather than
//! accepting them unjudged.
//!
//! **Blocked: the lobby's component vocabulary is not this one.** A real
//! `cs_net::lobby::Loadout` may only name ids whose kind
//! `cs_net::lobby::is_component_kind` accepts (`Airframe`, `Engine`, `Armor`,
//! `Gun`, `Ammo`, `HardpointEquipment`), while a construction gun or ordnance
//! fitment requires `ContentKind::Weapon`. `ContentId` identity is
//! `namespace/key`, so the two never match and the lobby's own shape check
//! refuses a `Weapon` component as `LoadoutProblem::Malformed`. Until an owner
//! declares how the two vocabularies correspond, `judge` can only be reached
//! with construction-vocabulary ids; every other loadout is refused, never
//! passed. The evidence, and the decision required, are in
//! `docs/findings/2026-10-17-f55-bu2-lobby-loadout-validator-mapping.md`.
//!
//! ## Codes
//!
//! Every refusal is one `&'static str` the lobby surfaces verbatim as
//! `cs_net::lobby::LoadoutProblem::Invalid { code }`. A final but unjudgeable
//! blueprint (an unmeasured limit or price, an unknown pairing rule) is refused
//! with its own code: "unknown" is never read as "fits" (AGENTS rules 4 and 5).

use std::collections::{BTreeMap, BTreeSet};

use cs_types::content::ContentId;

use crate::construction::{
    AircraftBlueprint, BlueprintVerdict, ConstraintViolation, ConstructionPolicy,
    ConstructionRules, LimitBreach, PriceBook, ValidationRefusal,
};

/// The named blueprint is not in the authored catalog, so nothing can be
/// judged.
pub const UNKNOWN_BLUEPRINT: &str = "unknown_blueprint";
/// The named blueprint is authored but its airframe has no construction rule
/// profile in the catalog.
pub const UNKNOWN_RULES: &str = "unknown_rules";
/// The loadout's component list is not the authored blueprint's fitted
/// components.
pub const LOADOUT_MISMATCH: &str = "loadout_mismatch";
/// The budget could not be measured: an unmeasured limit, an unpriced or
/// unmeasured component, an unknown gun-position count or an overflow. An
/// unknown is refused, never treated as fitting.
pub const UNMEASURED_BUDGET: &str = "unmeasured_budget";
/// A paired selection meets an unmeasured paired-gun rule, so the pair cannot
/// be judged.
pub const UNKNOWN_PAIRING_RULE: &str = "unknown_pairing_rule";
/// The loadout is heavier than the profile allows.
pub const OVER_MASS: &str = "over_mass";
/// The loadout costs more than the profile allows.
pub const OVER_COST: &str = "over_cost";
/// The fitted guns need more gun positions than the rack has.
pub const OVER_GUN_POSITIONS: &str = "over_gun_positions";
/// More ordnance is fitted than the profile has hardpoints.
pub const OVER_HARDPOINTS: &str = "over_hardpoints";
/// The host forbids the component ([`ConstraintViolation::BannedComponent`]).
pub const BANNED_COMPONENT: &str = "banned_component";
/// The component is not available to this caller
/// ([`ConstraintViolation::Unavailable`]).
pub const UNAVAILABLE_COMPONENT: &str = "unavailable_component";
/// A gun selection occupies a number of positions that is neither single nor
/// pair ([`ConstraintViolation::UnsupportedGunSelection`]).
pub const UNSUPPORTED_GUN_SELECTION: &str = "unsupported_gun_selection";
/// A paired gun fitment names a gun the pairing rule does not let be mated
/// ([`ConstraintViolation::GunNotPairable`]).
pub const GUN_NOT_PAIRABLE: &str = "gun_not_pairable";

/// The authored construction content the lobby validator judges against.
///
/// One catalog is built per lobby from the caller's authored blueprints (the
/// construction screen's own records, the campaign's owned set, an imported
/// blueprint...) and the shared per-airframe rules, pairing/availability policy
/// and price book those blueprints are measured by. It owns only data: it
/// performs no I/O and holds no lobby state, so the same catalog answers every
/// member's loadout identically.
#[derive(Debug)]
pub struct LobbyConstructionCatalog {
    blueprints: BTreeMap<ContentId, AircraftBlueprint>,
    rules: BTreeMap<ContentId, ConstructionRules>,
    policy: ConstructionPolicy,
    book: PriceBook,
}

impl LobbyConstructionCatalog {
    /// Assembles a catalog from authored blueprints and their airframes' rule
    /// profiles.
    ///
    /// The rule profiles are keyed by [`ConstructionRules::airframe`], so a
    /// blueprint's profile is looked up by the airframe it names rather than
    /// by position. A blueprint whose airframe has no profile is still accepted
    /// here; [`Self::judge`] refuses it as [`UNKNOWN_RULES`] rather than
    /// treating a partial catalog as a permissive one.
    #[must_use]
    pub fn new(
        blueprints: impl IntoIterator<Item = AircraftBlueprint>,
        rules: impl IntoIterator<Item = ConstructionRules>,
        policy: ConstructionPolicy,
        book: PriceBook,
    ) -> Self {
        Self {
            blueprints: blueprints
                .into_iter()
                .map(|blueprint| (blueprint.id().clone(), blueprint))
                .collect(),
            rules: rules
                .into_iter()
                .map(|rules| (rules.airframe().clone(), rules))
                .collect(),
            policy,
            book,
        }
    }

    /// The pairing, ban and availability policy every loadout is judged by.
    #[must_use]
    pub const fn policy(&self) -> &ConstructionPolicy {
        &self.policy
    }

    /// The declared masses and prices every loadout is measured against.
    #[must_use]
    pub const fn book(&self) -> &PriceBook {
        &self.book
    }

    /// The authored blueprint with `id`, when the catalog holds one.
    #[must_use]
    pub fn blueprint(&self, id: &ContentId) -> Option<&AircraftBlueprint> {
        self.blueprints.get(id)
    }

    /// The construction rule profile for `airframe`, when the catalog holds
    /// one.
    #[must_use]
    pub fn rules(&self, airframe: &ContentId) -> Option<&ConstructionRules> {
        self.rules.get(airframe)
    }

    /// Judges one lobby loadout and returns the stable reason code of the
    /// first refusal, or `Ok(())` when the shared rules accept it.
    ///
    /// See the module documentation for the mapping. The verdict's limits are
    /// reported before its constraints (the canonical quantity order), so a
    /// loadout that is both over budget and breaking a fitment rule reports the
    /// budget breach.
    ///
    /// # Errors
    ///
    /// One of the module's stable codes.
    pub fn judge(
        &self,
        blueprint: &ContentId,
        components: &[ContentId],
    ) -> Result<(), &'static str> {
        let authored = self.blueprints.get(blueprint).ok_or(UNKNOWN_BLUEPRINT)?;
        if !components_match(authored, components) {
            return Err(LOADOUT_MISMATCH);
        }
        let rules = self.rules.get(authored.airframe()).ok_or(UNKNOWN_RULES)?;
        match rules.validate(&self.policy, authored, &self.book) {
            Err(ValidationRefusal::Budget(_)) => Err(UNMEASURED_BUDGET),
            Err(ValidationRefusal::UnknownPairingRule { .. }) => Err(UNKNOWN_PAIRING_RULE),
            Ok(verdict) if verdict.is_valid() => Ok(()),
            Ok(verdict) => Err(refusal_code(&verdict)),
        }
    }
}

/// Whether `components` names exactly the distinct components `blueprint`
/// carries.
///
/// Both sides are compared as sets of distinct ids: the lobby's `Loadout` is a
/// set (its own shape check refuses a repeated component), while an authored
/// blueprint may fit the same component more than once (two plates, four guns).
/// Two armor fitments of the same plate are one entry here, and the fitment
/// counts live only in the blueprint. Order is not significant — the lobby
/// loadout is a chosen component set, not a serialized fitment order.
fn components_match(blueprint: &AircraftBlueprint, components: &[ContentId]) -> bool {
    let fitted: BTreeSet<&ContentId> = blueprint
        .components()
        .into_iter()
        .map(|(_, component)| component)
        .collect();
    let claimed: BTreeSet<&ContentId> = components.iter().collect();
    fitted == claimed
}

/// The stable code of the first reason an invalid verdict is invalid: its first
/// limit breach (in the assessment's canonical quantity order), else its first
/// constraint violation.
fn refusal_code(verdict: &BlueprintVerdict) -> &'static str {
    if let Some(breach) = verdict.assessment().breaches().first() {
        return match breach {
            LimitBreach::Mass { .. } => OVER_MASS,
            LimitBreach::Cost { .. } => OVER_COST,
            LimitBreach::GunPositions { .. } => OVER_GUN_POSITIONS,
            LimitBreach::RocketHardpoints { .. } => OVER_HARDPOINTS,
        };
    }
    match verdict.violations().first() {
        Some(ConstraintViolation::UnsupportedGunSelection { .. }) => UNSUPPORTED_GUN_SELECTION,
        Some(ConstraintViolation::GunNotPairable { .. }) => GUN_NOT_PAIRABLE,
        Some(ConstraintViolation::BannedComponent { .. }) => BANNED_COMPONENT,
        Some(ConstraintViolation::Unavailable { .. }) => UNAVAILABLE_COMPONENT,
        // `is_valid` was false and there is no breach, so at least one
        // violation exists; the arm keeps the function total without a panic.
        None => UNMEASURED_BUDGET,
    }
}
