//! Declared aircraft construction constraints and exact budget arithmetic
//! (F44-A).
//!
//! Spec: `specs/F44-aircraft-construction-budgets-loadouts-and-paint-editor.md`,
//! stage `### F44-A`. Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`,
//! section "Outcome and economy transaction".
//!
//! The sheet's deliverable is one [`AircraftBlueprint`] record and one
//! validator shared by campaign construction, Instant Action, multiplayer and
//! imports, returning "exact weight/cost totals, constraints, warnings and a
//! normalized performance preview". **This stage owns only the inputs and the
//! arithmetic** — the typed blueprint, the per-airframe
//! [`ConstructionRules`] limit record, the priced catalog and the exact
//! integer totals with their boundary verdict. The validator's constraint
//! rules (paired-gun compatibility, banned components, availability), the
//! transactional purchase/sell draft and the preview are F44-B, F44-B again
//! and F44-C.
//!
//! Three properties of this stage are load-bearing and each is enforced by a
//! test rather than a comment:
//!
//! 1. **Every number is an integer.** [`WeightUnits`] is a `u64` count of
//!    [`Unit::GameWeight`] and [`MoneyMinor`] a `u64` count of minor currency
//!    units, matching `IDENTITY-CONTENT`'s "Integers represent money, ticks,
//!    counts, ammo and ids" and `STATE-TRANSACTIONS`' "Currency uses integer
//!    minor game units". There is no `f64` anywhere in this module, so no
//!    float can round a total and no float formatting can move a purchase
//!    across a boundary.
//! 2. **The limits are profile data, never constants.** Non-negotiable 1
//!    reports four gun positions and up to eight rocket hardpoints as
//!    *observed manual* constraints and demands they be "confirmed against
//!    each discovered airframe/rule profile before declaring universal
//!    limits". They are therefore [`Resolved`] fields of one airframe's
//!    [`ConstructionRules`] — read from the profile argument, so a second
//!    profile with a different rack judges the same blueprint by its own
//!    numbers.
//! 3. **An unknown is refused, never assumed.** An unmeasured price, an
//!    unmeasured limit and an unmeasured gun-position cost each produce a
//!    named [`BudgetRefusal`]. Nothing defaults to zero, so an unmeasured
//!    value can never masquerade as a loadout that fits.
//!
//! **Designed and synthetic, not original data.** Every fixture here carries
//! [`Origin::SyntheticFixture`] and designed provenance. The original
//! component catalog, its masses, its prices, its weight unit's scale, its
//! per-airframe racks and whether a paint job carries a purchase price are
//! all **unmeasured** — F44-D's audit. Nothing here may be read as an original
//! component price, rack limit or blueprint.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved, Unit};
use cs_types::evidence::ClaimId;

use crate::damage::DamageNodeKey;

// ------------------------------------------------------------------ units ----

/// Integer weight in this project's documented **game-weight** unit.
///
/// `IDENTITY-CONTENT` names kilograms and "explicitly documented game-weight
/// units" as the acceptable weight units and forbids assuming SI for an
/// unknown original unit; F44 non-negotiable 2 asks for "integer money and
/// documented weight units". A weight is therefore a `u64` count of
/// [`Unit::GameWeight`] units — exact, never a float, and never a claim that
/// the original measured kilograms. What one game-weight unit is worth is
/// **unmeasured** (F44-D); that conversion belongs to the reader that finds
/// it, not to this type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WeightUnits(u64);

impl WeightUnits {
    /// The unit this count is expressed in, as the shared unit vocabulary
    /// spells it.
    pub const UNIT: Unit = Unit::GameWeight;

    /// A raw count of game-weight units.
    ///
    /// There is no range check: an unsigned integer cannot express a negative
    /// mass, and an *unmeasured* mass is not this type's problem — it is a
    /// [`Resolved`] unknown that the caller never lowers to a value.
    #[must_use]
    pub const fn new(units: u64) -> Self {
        Self(units)
    }

    /// The exact count of game-weight units.
    #[must_use]
    pub const fn as_units(self) -> u64 {
        self.0
    }

    /// The sum, refusing an `u64` overflow rather than wrapping to a smaller
    /// total that would silently pass a limit check.
    fn checked_add(self, other: Self) -> Option<Self> {
        self.0.checked_add(other.0).map(Self)
    }
}

impl fmt::Display for WeightUnits {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.0, Self::UNIT)
    }
}

/// Integer money in **minor** game currency units.
///
/// `STATE-TRANSACTIONS` requires "integer minor game units with a documented
/// display mapping"; this is the integer, and [`DisplayMapping`] is the
/// mapping. The *scale* of one minor unit in the original is unmeasured
/// (F44-D), so [`DisplayMapping::try_new`] takes the divisor as declared data
/// rather than baking a conversion into the type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MoneyMinor(u64);

impl MoneyMinor {
    /// A raw count of minor currency units.
    #[must_use]
    pub const fn new(minor: u64) -> Self {
        Self(minor)
    }

    /// The exact count of minor units.
    #[must_use]
    pub const fn as_minor(self) -> u64 {
        self.0
    }

    /// The sum, refusing an `u64` overflow rather than wrapping.
    fn checked_add(self, other: Self) -> Option<Self> {
        self.0.checked_add(other.0).map(Self)
    }

    /// The display text for this exact amount under `mapping`.
    ///
    /// Integer division and remainder only — there is no float in this module
    /// to round. The text is a *lossy projection* of the exact value and is
    /// therefore never an input to a purchase decision; see
    /// [`BlueprintAssessment::is_within_limits`], which reads the integers.
    #[must_use]
    pub fn format_display(&self, mapping: DisplayMapping) -> String {
        let divisor = u64::from(mapping.minor_per_major);
        let major = self.0 / divisor;
        let minor = self.0 % divisor;
        if mapping.decimal_width() == 0 {
            // No fractional part is displayed, so printing a `0` would invent a
            // decimal place the mapping does not declare.
            return major.to_string();
        }
        format!("{major}.{minor:0width$}", width = mapping.decimal_width())
    }
}

impl fmt::Display for MoneyMinor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} minor", self.0)
    }
}

/// The documented mapping from exact minor units to displayed major units.
///
/// `STATE-TRANSACTIONS` asks for "integer minor game units with a documented
/// display mapping" and F44 non-negotiable 2 requires that "rounding occurs at
/// a specified boundary". This type *is* that specification: it names how many
/// minor units make one displayed major unit, and it is display only. Nothing
/// in the budget path reads it, so a mapping coarse enough to print two
/// different totals as the same text cannot make either of them purchasable.
///
/// The divisor is a **power of ten** ([`DisplayMapping::try_new`] refuses any
/// other value). The fraction field is a fixed-width decimal, and only a power
/// of ten renders an integer remainder at its true scale: with a divisor of
/// `2500` the remainder `2000` is `0.8` major units, not `0.2000`. Refusing
/// the divisor a fixed-width fraction cannot represent exactly keeps every
/// constructible mapping *exact*, so displaying an amount can never produce a
/// plausible-looking wrong number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayMapping {
    minor_per_major: u32,
}

impl DisplayMapping {
    /// A mapping with `minor_per_major` minor units per displayed major unit.
    ///
    /// `minor_per_major` must be a power of ten — `1`, `10`, `100`, … up to
    /// `1_000_000_000`, the largest that fits this integer.
    ///
    /// # Errors
    ///
    /// [`ConstructionSchemaError::ZeroMinorPerMajor`] when the divisor is zero,
    /// which would make every amount display as an undefined fraction, and
    /// [`ConstructionSchemaError::MinorPerMajorNotPowerOfTen`] when it is a
    /// nonzero divisor whose remainder has no exact fixed-width decimal form.
    pub const fn try_new(minor_per_major: u32) -> Result<Self, ConstructionSchemaError> {
        if minor_per_major == 0 {
            return Err(ConstructionSchemaError::ZeroMinorPerMajor);
        }
        if !is_power_of_ten(minor_per_major) {
            return Err(ConstructionSchemaError::MinorPerMajorNotPowerOfTen { minor_per_major });
        }
        Ok(Self { minor_per_major })
    }

    /// The declared divisor.
    #[must_use]
    pub const fn minor_per_major(self) -> u32 {
        self.minor_per_major
    }

    /// How many decimal digits the fraction part of a display string needs.
    #[must_use]
    pub const fn decimal_width(self) -> usize {
        decimal_width(self.minor_per_major)
    }
}

/// The number of decimal digits `divisor` needs in a fraction field.
///
/// `1000` needs three (`"1.000"`), `10_000` needs four (`"1.0000"`). Counted
/// from the integer value, so a display mapping never depends on a float's
/// printed precision.
///
/// Only ever called with a [`DisplayMapping`]-accepted divisor, which is a power
/// of ten of at most `1_000_000_000`, so `scale` cannot overflow this `u32`.
const fn decimal_width(divisor: u32) -> usize {
    let mut width = 0;
    let mut scale = 1;
    while scale < divisor {
        width += 1;
        scale *= 10;
    }
    width
}

/// Whether `value` is `10^n` for some `n`.
///
/// `0` is not a power of ten, so this agrees with
/// [`DisplayMapping::try_new`]'s zero check rather than duplicating it.
const fn is_power_of_ten(value: u32) -> bool {
    let mut rest = value;
    while rest > 1 {
        if !rest.is_multiple_of(10) {
            return false;
        }
        rest /= 10;
    }
    rest == 1
}

// ------------------------------------------------------------ price book ----

/// One purchasable component's declared mass and price.
///
/// Both are [`Resolved`]: an unmeasured mass or price stays unknown and
/// refuses the budget rather than counting as zero, so a loadout is never
/// declared cheap because nobody measured it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentQuote {
    mass: Resolved<WeightUnits>,
    cost: Resolved<MoneyMinor>,
}

impl ComponentQuote {
    /// A quote with both numbers already resolved by the caller.
    #[must_use]
    pub const fn new(mass: Resolved<WeightUnits>, cost: Resolved<MoneyMinor>) -> Self {
        Self { mass, cost }
    }

    /// The component's declared mass, or an explicit unknown.
    #[must_use]
    pub const fn mass(&self) -> &Resolved<WeightUnits> {
        &self.mass
    }

    /// The component's declared price, or an explicit unknown.
    #[must_use]
    pub const fn cost(&self) -> &Resolved<MoneyMinor> {
        &self.cost
    }
}

/// The declared mass and price of every component a blueprint may name.
///
/// The book is keyed by [`ContentId`] and refuses a duplicate id, so a
/// component's price has exactly one statement and a lookup never depends on
/// insertion order.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct PriceBook {
    quotes: BTreeMap<ContentId, ComponentQuote>,
}

impl PriceBook {
    /// Builds a book, refusing a component that is quoted twice.
    ///
    /// # Errors
    ///
    /// [`ConstructionSchemaError::DuplicateComponent`] naming the second id.
    pub fn try_new(
        quotes: Vec<(ContentId, ComponentQuote)>,
    ) -> Result<Self, ConstructionSchemaError> {
        let mut book = Self {
            quotes: BTreeMap::new(),
        };
        for (id, quote) in quotes {
            if book.quotes.insert(id.clone(), quote).is_some() {
                return Err(ConstructionSchemaError::DuplicateComponent { component: id });
            }
        }
        Ok(book)
    }

    /// The quote for `component`, or `None` when the book does not price it.
    ///
    /// A missing quote is a refusal, not a free component: an unpriced id
    /// cannot be shown to cost nothing.
    #[must_use]
    pub fn quote(&self, component: &ContentId) -> Option<&ComponentQuote> {
        self.quotes.get(component)
    }

    /// How many components the book prices.
    #[must_use]
    pub fn len(&self) -> usize {
        self.quotes.len()
    }

    /// Whether the book prices nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.quotes.is_empty()
    }
}

// ------------------------------------------------------- constraint rules ----

/// One airframe's construction rule profile: the limits a blueprint built on
/// it is measured against.
///
/// **These are per-airframe declared values, not universal constants.**
/// Non-negotiable 1 reports four gun positions and up to eight rocket
/// hardpoints as *observed manual* constraints and requires them to be
/// confirmed against each discovered airframe/rule profile "before declaring
/// universal limits". Every limit is therefore a [`Resolved`] field read from
/// the profile: two profiles over the same airframe may declare different
/// racks, and an unmeasured limit refuses the comparison instead of becoming
/// "no limit".
///
/// The paired-gun compatibility rule and the banned-component list are *not*
/// here: they are the validator's constraint set (F44-B) and its import check
/// (F44-C). This record declares what a loadout is **measured against**, and
/// nothing about what is legal.
#[derive(Clone, Debug, PartialEq)]
pub struct ConstructionRules {
    airframe: ContentId,
    gun_positions: Resolved<u32>,
    rocket_hardpoints: Resolved<u32>,
    max_mass: Resolved<WeightUnits>,
    max_cost: Resolved<MoneyMinor>,
    origin: Origin,
    provenance: Provenance,
}

impl ConstructionRules {
    /// Assembles one airframe's rule profile.
    ///
    /// # Errors
    ///
    /// [`ConstructionSchemaError::NotAnAirframe`] when `airframe` is not in
    /// the `airframe` namespace.
    pub fn try_new(
        airframe: ContentId,
        gun_positions: Resolved<u32>,
        rocket_hardpoints: Resolved<u32>,
        max_mass: Resolved<WeightUnits>,
        max_cost: Resolved<MoneyMinor>,
        origin: Origin,
        provenance: Provenance,
    ) -> Result<Self, ConstructionSchemaError> {
        if airframe.kind() != ContentKind::Airframe {
            return Err(ConstructionSchemaError::NotAnAirframe {
                kind: airframe.kind(),
            });
        }
        Ok(Self {
            airframe,
            gun_positions,
            rocket_hardpoints,
            max_mass,
            max_cost,
            origin,
            provenance,
        })
    }

    /// The airframe whose components this profile describes.
    #[must_use]
    pub const fn airframe(&self) -> &ContentId {
        &self.airframe
    }

    /// How many gun positions the rack offers, or an explicit unknown.
    #[must_use]
    pub const fn gun_positions(&self) -> &Resolved<u32> {
        &self.gun_positions
    }

    /// How many rocket hardpoints are available, or an explicit unknown.
    #[must_use]
    pub const fn rocket_hardpoints(&self) -> &Resolved<u32> {
        &self.rocket_hardpoints
    }

    /// The weight ceiling, or an explicit unknown.
    #[must_use]
    pub const fn max_mass(&self) -> &Resolved<WeightUnits> {
        &self.max_mass
    }

    /// The price ceiling, or an explicit unknown.
    #[must_use]
    pub const fn max_cost(&self) -> &Resolved<MoneyMinor> {
        &self.max_cost
    }

    /// Where the profile came from.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The claim the profile backs.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

// ----------------------------------------------------- blueprint inputs ----

/// One armor zone's fitted armor.
///
/// The zone is the same [`DamageNodeKey`] the declared damage graph names, so
/// "which armor protects which part" is one key grammar shared with F29-A
/// rather than a second index that could drift.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArmorFitment {
    zone: DamageNodeKey,
    armor: ContentId,
}

impl ArmorFitment {
    /// Fits `armor` to `zone`.
    ///
    /// # Errors
    ///
    /// [`ConstructionSchemaError::WrongKind`] when `armor` is not an `armor`
    /// id.
    pub fn try_new(zone: DamageNodeKey, armor: ContentId) -> Result<Self, ConstructionSchemaError> {
        require_kind(&armor, ContentKind::Armor)?;
        Ok(Self { zone, armor })
    }

    /// The damage-graph armor zone this fitment covers.
    #[must_use]
    pub const fn zone(&self) -> &DamageNodeKey {
        &self.zone
    }

    /// The armor component fitted there.
    #[must_use]
    pub const fn armor(&self) -> &ContentId {
        &self.armor
    }
}

/// One gun fitted to the airframe, and the gun positions it consumes.
///
/// The public manual reports four gun positions that accept a single gun or a
/// pair (non-negotiable 1). **Which** selections a given airframe offers, and
/// when two guns may legally be mated, is F44-B's paired-gun rule; this stage
/// declares only the **accounting**, so the position count travels with the
/// fitment as a [`Resolved<u32>`]. An unresolved count refuses the total
/// instead of quietly occupying one position, and a known count of zero is
/// refused at construction so no fitment can free a slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GunFitment {
    gun: ContentId,
    mount: DamageNodeKey,
    positions: Resolved<u32>,
}

impl GunFitment {
    /// Fits `gun` at `mount`, occupying `positions` gun positions.
    ///
    /// # Errors
    ///
    /// [`ConstructionSchemaError::WrongKind`] when `gun` is not a `weapon`
    /// id, or [`ConstructionSchemaError::ZeroGunPositions`] when a known
    /// position count is zero.
    pub fn try_new(
        gun: ContentId,
        mount: DamageNodeKey,
        positions: Resolved<u32>,
    ) -> Result<Self, ConstructionSchemaError> {
        require_kind(&gun, ContentKind::Weapon)?;
        if let Resolved::Known(known) = &positions
            && known.value == 0
        {
            return Err(ConstructionSchemaError::ZeroGunPositions);
        }
        Ok(Self {
            gun,
            mount,
            positions,
        })
    }

    /// The gun fitted here.
    #[must_use]
    pub const fn gun(&self) -> &ContentId {
        &self.gun
    }

    /// The damage-graph weapon-mount node this gun occupies.
    #[must_use]
    pub const fn mount(&self) -> &DamageNodeKey {
        &self.mount
    }

    /// How many gun positions it consumes, or an explicit unknown.
    #[must_use]
    pub const fn positions(&self) -> &Resolved<u32> {
        &self.positions
    }

    /// The position count when it is known.
    #[must_use]
    pub fn known_positions(&self) -> Option<u32> {
        self.positions.clone().known()
    }

    /// The same fitment occupying a different number of gun positions.
    ///
    /// This is what distinguishes a single from a paired selection in the
    /// *accounting*: the gun, the mount and the component's price are unchanged,
    /// and only the rack positions consumed move (non-negotiable 1's "single/pair
    /// selections"). Whether a pair is legal on this airframe is F44-B's rule.
    ///
    /// # Errors
    ///
    /// [`ConstructionSchemaError::ZeroGunPositions`] when a known position
    /// count is zero.
    pub fn with_positions(
        mut self,
        positions: Resolved<u32>,
    ) -> Result<Self, ConstructionSchemaError> {
        if let Resolved::Known(known) = &positions
            && known.value == 0
        {
            return Err(ConstructionSchemaError::ZeroGunPositions);
        }
        self.positions = positions;
        Ok(self)
    }
}

/// One ordnance item fitted to a rocket hardpoint.
///
/// The ordnance id is in the `weapon` namespace, the same identity
/// `cs_content::ordnance` validates its declared projectiles against, so a
/// rocket fitted to a hardpoint is the same catalog element the ordnance
/// record describes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrdnanceFitment {
    hardpoint: DamageNodeKey,
    ordnance: ContentId,
}

impl OrdnanceFitment {
    /// Fits `ordnance` at `hardpoint`.
    ///
    /// # Errors
    ///
    /// [`ConstructionSchemaError::WrongKind`] when `ordnance` is not a
    /// `weapon` id.
    pub fn try_new(
        hardpoint: DamageNodeKey,
        ordnance: ContentId,
    ) -> Result<Self, ConstructionSchemaError> {
        require_kind(&ordnance, ContentKind::Weapon)?;
        Ok(Self {
            hardpoint,
            ordnance,
        })
    }

    /// The hardpoint node this item occupies.
    #[must_use]
    pub const fn hardpoint(&self) -> &DamageNodeKey {
        &self.hardpoint
    }

    /// The ordnance component fitted there.
    #[must_use]
    pub const fn ordnance(&self) -> &ContentId {
        &self.ordnance
    }
}

/// One decal applied over one paint mask.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecalPlacement {
    decal: ContentId,
    mask: ContentId,
}

impl DecalPlacement {
    /// Places `decal` over `mask`.
    ///
    /// # Errors
    ///
    /// [`ConstructionSchemaError::WrongKind`] when either id is not in the
    /// `paint_mask` namespace.
    pub fn try_new(decal: ContentId, mask: ContentId) -> Result<Self, ConstructionSchemaError> {
        require_kind(&decal, ContentKind::PaintMask)?;
        require_kind(&mask, ContentKind::PaintMask)?;
        Ok(Self { decal, mask })
    }

    /// The decal's catalog id.
    #[must_use]
    pub const fn decal(&self) -> &ContentId {
        &self.decal
    }

    /// The mask the decal is applied over.
    #[must_use]
    pub const fn mask(&self) -> &ContentId {
        &self.mask
    }
}

/// The paint and decals a blueprint carries — **references only**.
///
/// Non-negotiable 5 requires that custom paint preserve source masks and that
/// an export/import "never includes copyrighted source textures implicitly".
/// This record therefore holds catalog ids and nothing else: no pixel data, no
/// encoded blob and no filesystem path, so a blueprint that is exported is a
/// list of what to look up, never a copy of protected source art.
///
/// Whether a custom paint job carries a **purchase price** is unmeasured, so
/// this stage prices no paint: [`BudgetCategory`] has no paint row and the
/// findings record the gap. Which mask combinations are valid is F44-B's
/// constraint; the editor that authors them is F44-C's path.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PaintSelection {
    masks: Vec<ContentId>,
    decals: Vec<DecalPlacement>,
}

impl PaintSelection {
    /// Builds a paint selection, refusing a repeated mask or decal.
    ///
    /// # Errors
    ///
    /// [`ConstructionSchemaError::WrongKind`] when a mask is not a
    /// `paint_mask` id, or [`ConstructionSchemaError::DuplicatePaint`] for a
    /// repeated mask or decal.
    pub fn try_new(
        masks: Vec<ContentId>,
        decals: Vec<DecalPlacement>,
    ) -> Result<Self, ConstructionSchemaError> {
        let mut seen: Vec<ContentId> = Vec::with_capacity(masks.len());
        for mask in &masks {
            require_kind(mask, ContentKind::PaintMask)?;
            if seen.contains(mask) {
                return Err(ConstructionSchemaError::DuplicatePaint {
                    component: mask.clone(),
                });
            }
            seen.push(mask.clone());
        }
        let mut decal_ids: Vec<ContentId> = Vec::with_capacity(decals.len());
        for placement in &decals {
            if decal_ids.contains(placement.decal()) {
                return Err(ConstructionSchemaError::DuplicatePaint {
                    component: placement.decal.clone(),
                });
            }
            decal_ids.push(placement.decal.clone());
        }
        Ok(Self { masks, decals })
    }

    /// The paint masks, in authored order.
    #[must_use]
    pub fn masks(&self) -> &[ContentId] {
        &self.masks
    }

    /// The decals, in authored order.
    #[must_use]
    pub fn decals(&self) -> &[DecalPlacement] {
        &self.decals
    }
}

/// One aircraft blueprint: the typed input every construction path shares.
///
/// This is the record the sheet's deliverable names — airframe, engine,
/// per-zone armor, gun positions, rocket hardpoints, equipment, paint and
/// decals — with identity discipline from `IDENTITY-CONTENT`: a `blueprint`
/// catalog id, kind-checked component ids and no repeated damage-graph node,
/// so a zone, a weapon mount and a hardpoint each name one thing.
#[derive(Clone, Debug, PartialEq)]
pub struct AircraftBlueprint {
    id: ContentId,
    airframe: ContentId,
    engine: ContentId,
    armor: Vec<ArmorFitment>,
    guns: Vec<GunFitment>,
    ordnance: Vec<OrdnanceFitment>,
    equipment: Vec<ContentId>,
    paint: PaintSelection,
    origin: Origin,
    provenance: Provenance,
}

impl AircraftBlueprint {
    /// Assembles a blueprint, refusing a wrong-namespace id or a repeated
    /// damage-graph node.
    ///
    /// # Errors
    ///
    /// [`ConstructionSchemaError`] when the blueprint, airframe, engine or an
    /// equipment id is in the wrong namespace, or when two fitments claim the
    /// same armor zone, weapon mount or hardpoint.
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        id: ContentId,
        airframe: ContentId,
        engine: ContentId,
        armor: Vec<ArmorFitment>,
        guns: Vec<GunFitment>,
        ordnance: Vec<OrdnanceFitment>,
        equipment: Vec<ContentId>,
        paint: PaintSelection,
        origin: Origin,
        provenance: Provenance,
    ) -> Result<Self, ConstructionSchemaError> {
        require_kind(&id, ContentKind::Blueprint)?;
        require_kind(&airframe, ContentKind::Airframe)?;
        require_kind(&engine, ContentKind::Engine)?;
        reject_repeated(
            armor.iter().map(|fitment| fitment.zone().clone()),
            ConstructionSlot::ArmorZone,
        )?;
        reject_repeated(
            guns.iter().map(|fitment| fitment.mount().clone()),
            ConstructionSlot::WeaponMount,
        )?;
        reject_repeated(
            ordnance.iter().map(|fitment| fitment.hardpoint().clone()),
            ConstructionSlot::Hardpoint,
        )?;
        let mut equipment_seen: Vec<ContentId> = Vec::with_capacity(equipment.len());
        for item in &equipment {
            require_kind(item, ContentKind::HardpointEquipment)?;
            if equipment_seen.contains(item) {
                return Err(ConstructionSchemaError::DuplicateEquipment {
                    component: item.clone(),
                });
            }
            equipment_seen.push(item.clone());
        }
        Ok(Self {
            id,
            airframe,
            engine,
            armor,
            guns,
            ordnance,
            equipment,
            paint,
            origin,
            provenance,
        })
    }

    /// The blueprint's `blueprint` catalog id.
    #[must_use]
    pub const fn id(&self) -> &ContentId {
        &self.id
    }

    /// The airframe this blueprint is built on.
    #[must_use]
    pub const fn airframe(&self) -> &ContentId {
        &self.airframe
    }

    /// The fitted engine.
    #[must_use]
    pub const fn engine(&self) -> &ContentId {
        &self.engine
    }

    /// The per-zone armor fitments, in authored order.
    #[must_use]
    pub fn armor(&self) -> &[ArmorFitment] {
        &self.armor
    }

    /// The gun fitments, in authored order.
    #[must_use]
    pub fn guns(&self) -> &[GunFitment] {
        &self.guns
    }

    /// The rocket hardpoint fitments, in authored order.
    #[must_use]
    pub fn ordnance(&self) -> &[OrdnanceFitment] {
        &self.ordnance
    }

    /// The fitted equipment, in authored order.
    #[must_use]
    pub fn equipment(&self) -> &[ContentId] {
        &self.equipment
    }

    /// The paint and decal references.
    #[must_use]
    pub const fn paint(&self) -> &PaintSelection {
        &self.paint
    }

    /// Where the blueprint came from.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The claim the blueprint backs.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// The blueprint with its armor fitments replaced.
    ///
    /// The replacement is re-validated, so an edit that fits two armors to one
    /// zone is refused here rather than surfacing as a wrong total.
    ///
    /// # Errors
    ///
    /// [`ConstructionSchemaError`] on a repeated armor zone.
    pub fn with_armor(mut self, armor: Vec<ArmorFitment>) -> Result<Self, ConstructionSchemaError> {
        reject_repeated(
            armor.iter().map(|fitment| fitment.zone().clone()),
            ConstructionSlot::ArmorZone,
        )?;
        self.armor = armor;
        Ok(self)
    }

    /// The blueprint with its gun fitments replaced.
    ///
    /// # Errors
    ///
    /// [`ConstructionSchemaError`] on a repeated weapon mount.
    pub fn with_guns(mut self, guns: Vec<GunFitment>) -> Result<Self, ConstructionSchemaError> {
        reject_repeated(
            guns.iter().map(|fitment| fitment.mount().clone()),
            ConstructionSlot::WeaponMount,
        )?;
        self.guns = guns;
        Ok(self)
    }

    /// The blueprint with its rocket hardpoint fitments replaced.
    ///
    /// # Errors
    ///
    /// [`ConstructionSchemaError`] on a repeated hardpoint.
    pub fn with_ordnance(
        mut self,
        ordnance: Vec<OrdnanceFitment>,
    ) -> Result<Self, ConstructionSchemaError> {
        reject_repeated(
            ordnance.iter().map(|fitment| fitment.hardpoint().clone()),
            ConstructionSlot::Hardpoint,
        )?;
        self.ordnance = ordnance;
        Ok(self)
    }

    /// The blueprint with a different airframe.
    ///
    /// # Errors
    ///
    /// [`ConstructionSchemaError::NotAnAirframe`] when the new id is not an
    /// `airframe`.
    pub fn with_airframe(mut self, airframe: ContentId) -> Result<Self, ConstructionSchemaError> {
        require_kind(&airframe, ContentKind::Airframe)?;
        self.airframe = airframe;
        Ok(self)
    }
}

/// Which damage-graph slot a repeated node would collide in.
///
/// Public because it names which slot
/// [`ConstructionSchemaError::DuplicateSlot`] collided: "two fitments claim the
/// same weapon mount" is a different defect from "…the same armor zone", and a
/// caller repairing an imported blueprint needs to know which one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstructionSlot {
    /// An armor zone.
    ArmorZone,
    /// A weapon mount.
    WeaponMount,
    /// A rocket hardpoint.
    Hardpoint,
}

impl fmt::Display for ConstructionSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ArmorZone => "armor zone",
            Self::WeaponMount => "weapon mount",
            Self::Hardpoint => "rocket hardpoint",
        })
    }
}

fn require_kind(id: &ContentId, expected: ContentKind) -> Result<(), ConstructionSchemaError> {
    if id.kind() == expected {
        Ok(())
    } else {
        Err(ConstructionSchemaError::WrongKind {
            id: id.clone(),
            expected,
        })
    }
}

fn reject_repeated<T: PartialEq>(
    nodes: impl Iterator<Item = T>,
    slot: ConstructionSlot,
) -> Result<(), ConstructionSchemaError> {
    let mut seen: Vec<T> = Vec::new();
    for node in nodes {
        if seen.contains(&node) {
            return Err(ConstructionSchemaError::DuplicateSlot { slot });
        }
        seen.push(node);
    }
    Ok(())
}

// ---------------------------------------------------------- schema errors ----

/// Why a construction record was refused at construction.
///
/// These are *structural* refusals: a wrong namespace, a repeated node, a
/// degenerate value. A record that cannot be built at all never reaches the
/// budget, so no arithmetic has to defend against it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConstructionSchemaError {
    /// An id is not in the namespace its slot requires.
    WrongKind {
        /// The offending id.
        id: ContentId,
        /// The namespace the slot requires.
        expected: ContentKind,
    },
    /// A rule profile named something that is not an airframe.
    NotAnAirframe {
        /// The namespace the id actually names.
        kind: ContentKind,
    },
    /// The price book quotes the same component twice.
    DuplicateComponent {
        /// The component quoted more than once.
        component: ContentId,
    },
    /// Two fitments claim the same damage-graph node in one slot.
    DuplicateSlot {
        /// Which slot collided.
        slot: ConstructionSlot,
    },
    /// The same equipment item is fitted twice.
    DuplicateEquipment {
        /// The duplicated component.
        component: ContentId,
    },
    /// The same paint mask or decal is listed twice.
    DuplicatePaint {
        /// The duplicated id.
        component: ContentId,
    },
    /// A gun fitment declared that it occupies no gun position.
    ZeroGunPositions,
    /// A display mapping declared zero minor units per major unit.
    ZeroMinorPerMajor,
    /// A display mapping declared a divisor that is not a power of ten, whose
    /// remainder has no exact fixed-width decimal form.
    MinorPerMajorNotPowerOfTen {
        /// The rejected divisor.
        minor_per_major: u32,
    },
}

impl fmt::Display for ConstructionSchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongKind { id, expected } => {
                write!(f, "{id} must be in the {expected} namespace")
            }
            Self::NotAnAirframe { kind } => {
                write!(
                    f,
                    "a construction rule profile must name an airframe, got {kind}"
                )
            }
            Self::DuplicateComponent { component } => {
                write!(f, "{component} is quoted more than once in the price book")
            }
            Self::DuplicateSlot { slot } => {
                write!(f, "two fitments claim the same {slot}")
            }
            Self::DuplicateEquipment { component } => {
                write!(f, "{component} is fitted as equipment more than once")
            }
            Self::DuplicatePaint { component } => {
                write!(
                    f,
                    "{component} is listed in the paint selection more than once"
                )
            }
            Self::ZeroGunPositions => {
                f.write_str("a fitted gun must occupy at least one gun position")
            }
            Self::ZeroMinorPerMajor => {
                f.write_str("a display mapping needs at least one minor unit per major unit")
            }
            Self::MinorPerMajorNotPowerOfTen { minor_per_major } => {
                write!(
                    f,
                    "{minor_per_major} minor units per major unit is not a power of ten, so its \
                     fraction has no exact fixed decimal width"
                )
            }
        }
    }
}

impl std::error::Error for ConstructionSchemaError {}

// ------------------------------------------------------- budget arithmetic ----

/// The priced categories a budget is the sum of.
///
/// The list is **closed and ordered**: [`BlueprintTotals::from_breakdown`]
/// folds over exactly these six rows, so a seventh category cannot appear
/// without changing the arithmetic the totals are defined by. Paint has no row
/// because a custom paint job's purchase price is unmeasured; that gap is
/// recorded in the findings, not hidden by a silent zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BudgetCategory {
    /// The airframe itself.
    Airframe,
    /// The fitted engine.
    Engine,
    /// Every armor fitment.
    Armor,
    /// Every gun fitment.
    Guns,
    /// Every rocket hardpoint fitment.
    Ordnance,
    /// Every equipment item.
    Equipment,
}

impl BudgetCategory {
    /// Every priced category, in the canonical summation order.
    pub const ALL: [Self; 6] = [
        Self::Airframe,
        Self::Engine,
        Self::Armor,
        Self::Guns,
        Self::Ordnance,
        Self::Equipment,
    ];

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Airframe => "airframe",
            Self::Engine => "engine",
            Self::Armor => "armor",
            Self::Guns => "guns",
            Self::Ordnance => "ordnance",
            Self::Equipment => "equipment",
        }
    }

    /// Looks a category up by its label; `None` for an unknown label.
    #[must_use]
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|category| category.label() == label)
    }

    /// The row this category occupies in [`BudgetBreakdown::lines`].
    const fn index(self) -> usize {
        match self {
            Self::Airframe => 0,
            Self::Engine => 1,
            Self::Armor => 2,
            Self::Guns => 3,
            Self::Ordnance => 4,
            Self::Equipment => 5,
        }
    }
}

impl fmt::Display for BudgetCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// One category's exact subtotals.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BudgetLine {
    mass: WeightUnits,
    cost: MoneyMinor,
}

impl BudgetLine {
    /// The additive identity.
    pub const ZERO: Self = Self {
        mass: WeightUnits::new(0),
        cost: MoneyMinor::new(0),
    };

    /// The category's exact weight.
    #[must_use]
    pub const fn mass(&self) -> WeightUnits {
        self.mass
    }

    /// The category's exact price.
    #[must_use]
    pub const fn cost(&self) -> MoneyMinor {
        self.cost
    }

    /// Adds one component, refusing an integer overflow instead of wrapping to
    /// a smaller subtotal that could pass a limit check.
    fn add(&mut self, mass: WeightUnits, cost: MoneyMinor) -> Result<(), BudgetQuantity> {
        self.mass = self.mass.checked_add(mass).ok_or(BudgetQuantity::Mass)?;
        self.cost = self.cost.checked_add(cost).ok_or(BudgetQuantity::Cost)?;
        Ok(())
    }
}

/// The exact per-category subtotals of one blueprint.
///
/// The breakdown is the auditable form of a total: it names where every unit
/// came from, so a reviewer can add the columns and see the same numbers the
/// [`BlueprintTotals`] reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BudgetBreakdown {
    lines: [BudgetLine; 6],
}

impl BudgetBreakdown {
    /// The empty breakdown.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            lines: [BudgetLine::ZERO; 6],
        }
    }

    /// Every category's subtotal, in the canonical summation order.
    #[must_use]
    pub const fn lines(&self) -> [(BudgetCategory, BudgetLine); 6] {
        [
            (
                BudgetCategory::Airframe,
                self.lines[BudgetCategory::Airframe.index()],
            ),
            (
                BudgetCategory::Engine,
                self.lines[BudgetCategory::Engine.index()],
            ),
            (
                BudgetCategory::Armor,
                self.lines[BudgetCategory::Armor.index()],
            ),
            (
                BudgetCategory::Guns,
                self.lines[BudgetCategory::Guns.index()],
            ),
            (
                BudgetCategory::Ordnance,
                self.lines[BudgetCategory::Ordnance.index()],
            ),
            (
                BudgetCategory::Equipment,
                self.lines[BudgetCategory::Equipment.index()],
            ),
        ]
    }

    /// One category's subtotal.
    #[must_use]
    pub const fn line(&self, category: BudgetCategory) -> BudgetLine {
        self.lines[category.index()]
    }

    fn add(
        &mut self,
        category: BudgetCategory,
        mass: WeightUnits,
        cost: MoneyMinor,
    ) -> Result<(), BudgetQuantity> {
        self.lines[category.index()].add(mass, cost)
    }
}

/// Which measured quantity a message is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BudgetQuantity {
    /// Total weight.
    Mass,
    /// Total price.
    Cost,
    /// Gun positions consumed.
    GunPositions,
    /// Rocket hardpoints consumed.
    RocketHardpoints,
}

impl BudgetQuantity {
    /// Every measured quantity, in the canonical breach order.
    pub const ALL: [Self; 4] = [
        Self::Mass,
        Self::Cost,
        Self::GunPositions,
        Self::RocketHardpoints,
    ];

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Mass => "mass",
            Self::Cost => "cost",
            Self::GunPositions => "gun_positions",
            Self::RocketHardpoints => "rocket_hardpoints",
        }
    }
}

impl fmt::Display for BudgetQuantity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

impl BudgetQuantity {
    /// Looks a quantity up by its label; `None` for an unknown label.
    #[must_use]
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|quantity| quantity.label() == label)
    }
}

/// One blueprint's exact totals.
///
/// The four numbers are the whole measurement: two resource totals and two
/// position counts. All are integers, and the resource totals are the checked
/// fold of [`BudgetBreakdown`] over every priced category.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlueprintTotals {
    mass: WeightUnits,
    cost: MoneyMinor,
    gun_positions: u32,
    rocket_hardpoints: u32,
}

impl BlueprintTotals {
    /// The totals of `breakdown` plus the two position counts.
    ///
    /// # Errors
    ///
    /// [`BudgetRefusal::Overflow`] naming the quantity whose `u64` sum does not
    /// fit. A wrapped total would be *smaller* than the real one and could pass
    /// a limit check, so the sum is refused instead of truncated.
    pub fn from_breakdown(
        breakdown: &BudgetBreakdown,
        gun_positions: u32,
        rocket_hardpoints: u32,
    ) -> Result<Self, BudgetRefusal> {
        let mut mass = WeightUnits::new(0);
        let mut cost = MoneyMinor::new(0);
        for (category, line) in breakdown.lines() {
            mass = mass
                .checked_add(line.mass())
                .ok_or(BudgetRefusal::Overflow {
                    quantity: BudgetQuantity::Mass,
                    category: Some(category),
                })?;
            cost = cost
                .checked_add(line.cost())
                .ok_or(BudgetRefusal::Overflow {
                    quantity: BudgetQuantity::Cost,
                    category: Some(category),
                })?;
        }
        Ok(Self {
            mass,
            cost,
            gun_positions,
            rocket_hardpoints,
        })
    }

    /// The exact total weight.
    #[must_use]
    pub const fn mass(&self) -> WeightUnits {
        self.mass
    }

    /// The exact total price.
    #[must_use]
    pub const fn cost(&self) -> MoneyMinor {
        self.cost
    }

    /// How many gun positions the fitted guns consume.
    #[must_use]
    pub const fn gun_positions(&self) -> u32 {
        self.gun_positions
    }

    /// How many rocket hardpoints the fitted ordnance consumes.
    #[must_use]
    pub const fn rocket_hardpoints(&self) -> u32 {
        self.rocket_hardpoints
    }

    /// How much over the limit `quantity` is, or zero when it is not breached.
    ///
    /// The difference is computed on exact integers, so a shortfall reported to
    /// a player is the same number the comparison rejected. The subtraction
    /// saturates at zero: a `breach` that is not actually over its limit — a
    /// limit from a different profile, say — reports no excess instead of
    /// underflowing a `u64`.
    #[must_use]
    pub fn excess(&self, breach: LimitBreach) -> u64 {
        match breach {
            LimitBreach::Mass { limit, total } => total.as_units().saturating_sub(limit.as_units()),
            LimitBreach::Cost { limit, total } => total.as_minor().saturating_sub(limit.as_minor()),
            LimitBreach::GunPositions { limit, used } => u64::from(used.saturating_sub(limit)),
            LimitBreach::RocketHardpoints { limit, used } => u64::from(used.saturating_sub(limit)),
        }
    }
}

/// One limit a blueprint is over.
///
/// A breach is reported with both the limit and the measured total, so a
/// caller never has to re-derive the arithmetic that rejected the loadout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitBreach {
    /// The loadout is heavier than the profile allows.
    Mass {
        /// The profile's weight ceiling.
        limit: WeightUnits,
        /// The measured total.
        total: WeightUnits,
    },
    /// The loadout costs more than the profile allows.
    Cost {
        /// The profile's price ceiling.
        limit: MoneyMinor,
        /// The measured total.
        total: MoneyMinor,
    },
    /// The fitted guns need more gun positions than the rack has.
    GunPositions {
        /// The profile's gun-position count.
        limit: u32,
        /// The positions consumed.
        used: u32,
    },
    /// More ordnance is fitted than the profile has hardpoints.
    RocketHardpoints {
        /// The profile's hardpoint count.
        limit: u32,
        /// The hardpoints consumed.
        used: u32,
    },
}

impl LimitBreach {
    /// Which quantity this breach is about.
    #[must_use]
    pub const fn quantity(&self) -> BudgetQuantity {
        match self {
            Self::Mass { .. } => BudgetQuantity::Mass,
            Self::Cost { .. } => BudgetQuantity::Cost,
            Self::GunPositions { .. } => BudgetQuantity::GunPositions,
            Self::RocketHardpoints { .. } => BudgetQuantity::RocketHardpoints,
        }
    }
}

impl fmt::Display for LimitBreach {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mass { limit, total } => write!(f, "weight {total} exceeds the limit {limit}"),
            Self::Cost { limit, total } => write!(f, "cost {total} exceeds the limit {limit}"),
            Self::GunPositions { limit, used } => {
                write!(f, "{used} gun positions exceed the {limit} the rack has")
            }
            Self::RocketHardpoints { limit, used } => {
                write!(f, "{used} rockets exceed the {limit} hardpoints available")
            }
        }
    }
}

/// Why a blueprint could not be measured against a rule profile at all.
///
/// Every variant is an **unknown or an arithmetic failure**, never a verdict.
/// A refusal means "nothing can be concluded", which is deliberately different
/// from [`LimitBreach`], which means "measured and over".
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BudgetRefusal {
    /// The blueprint is on a different airframe than the profile describes, so
    /// the profile's limits would be the wrong limits.
    AirframeMismatch {
        /// The blueprint's airframe.
        blueprint: ContentId,
        /// The profile's airframe.
        rules: ContentId,
    },
    /// The price book does not price a component the blueprint names.
    NotPriced {
        /// The unpriced component.
        component: ContentId,
        /// The category it was fitted into.
        category: BudgetCategory,
    },
    /// A component's mass is an explicit unknown, so no exact total exists.
    UnknownMass {
        /// The component whose mass is unmeasured.
        component: ContentId,
    },
    /// A component's price is an explicit unknown, so no exact total exists.
    UnknownCost {
        /// The component whose price is unmeasured.
        component: ContentId,
    },
    /// A gun fitment's gun-position count is an explicit unknown, so the rack
    /// usage cannot be totalled.
    UnknownGunPositions {
        /// The gun whose position count is unmeasured.
        component: ContentId,
    },
    /// The profile's own limit for a quantity is an explicit unknown, so the
    /// comparison cannot be made. It is never read as "no limit".
    UnknownLimit {
        /// The limit that is unmeasured.
        quantity: BudgetQuantity,
    },
    /// An integer sum does not fit its `u64` and was refused rather than
    /// wrapped.
    Overflow {
        /// The quantity that overflowed.
        quantity: BudgetQuantity,
        /// The category whose addition overflowed, when the overflow happened
        /// inside the subtotal fold.
        category: Option<BudgetCategory>,
    },
}

impl fmt::Display for BudgetRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AirframeMismatch { blueprint, rules } => write!(
                f,
                "the rule profile describes {rules}, not the blueprint's airframe {blueprint}"
            ),
            Self::NotPriced {
                component,
                category,
            } => {
                write!(f, "the price book does not price {component} as {category}")
            }
            Self::UnknownMass { component } => {
                write!(
                    f,
                    "the mass of {component} is unmeasured, so no exact total exists"
                )
            }
            Self::UnknownCost { component } => {
                write!(
                    f,
                    "the price of {component} is unmeasured, so no exact total exists"
                )
            }
            Self::UnknownGunPositions { component } => write!(
                f,
                "the gun positions {component} occupies are unmeasured, so the rack usage is unknown"
            ),
            Self::UnknownLimit { quantity } => write!(
                f,
                "the {quantity} limit of this airframe is unmeasured, so no comparison is possible"
            ),
            Self::Overflow { quantity, category } => match category {
                Some(category) => {
                    write!(f, "the {quantity} total overflowed while adding {category}")
                }
                None => write!(f, "the {quantity} total overflowed"),
            },
        }
    }
}

impl std::error::Error for BudgetRefusal {}

/// The four limits an assessment compares against, resolved once.
struct Limits {
    mass: WeightUnits,
    cost: MoneyMinor,
    gun_positions: u32,
    rocket_hardpoints: u32,
}

impl Limits {
    /// Resolves every limit of a profile, refusing an unmeasured one.
    ///
    /// The limits are read *before* any pricing work, so a profile whose own
    /// limit is unmeasured reports that rather than spending effort on a total
    /// it could never compare.
    fn from_rules(rules: &ConstructionRules) -> Result<Self, BudgetRefusal> {
        Ok(Self {
            mass: rules
                .max_mass()
                .clone()
                .known()
                .ok_or(BudgetRefusal::UnknownLimit {
                    quantity: BudgetQuantity::Mass,
                })?,
            cost: rules
                .max_cost()
                .clone()
                .known()
                .ok_or(BudgetRefusal::UnknownLimit {
                    quantity: BudgetQuantity::Cost,
                })?,
            gun_positions: rules.gun_positions().clone().known().ok_or(
                BudgetRefusal::UnknownLimit {
                    quantity: BudgetQuantity::GunPositions,
                },
            )?,
            rocket_hardpoints: rules.rocket_hardpoints().clone().known().ok_or(
                BudgetRefusal::UnknownLimit {
                    quantity: BudgetQuantity::RocketHardpoints,
                },
            )?,
        })
    }
}

/// Adds one component's exact mass and price into its category subtotal.
fn price(
    book: &PriceBook,
    component: &ContentId,
    category: BudgetCategory,
    breakdown: &mut BudgetBreakdown,
) -> Result<(), BudgetRefusal> {
    let quote = book
        .quote(component)
        .ok_or_else(|| BudgetRefusal::NotPriced {
            component: component.clone(),
            category,
        })?;
    let mass = quote
        .mass()
        .clone()
        .known()
        .ok_or_else(|| BudgetRefusal::UnknownMass {
            component: component.clone(),
        })?;
    let cost = quote
        .cost()
        .clone()
        .known()
        .ok_or_else(|| BudgetRefusal::UnknownCost {
            component: component.clone(),
        })?;
    breakdown
        .add(category, mass, cost)
        .map_err(|quantity| BudgetRefusal::Overflow {
            quantity,
            category: Some(category),
        })
}

/// One blueprint measured against one airframe's rule profile.
///
/// `is_within_limits` is the verdict AC01 asks for, and it compares the
/// **integers**: a loadout exactly at a limit is inside it, and a loadout one
/// unit over is outside it. No display string, rounding rule or float is
/// consulted, so a UI that prints two different totals as the same text cannot
/// change either verdict.
///
/// Being inside the limits is a *measurement*, not a legality claim: paired-gun
/// compatibility, banned components and availability are the validator's
/// constraint set (F44-B/F44-C), and this record deliberately does not decide
/// them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlueprintAssessment {
    blueprint: ContentId,
    airframe: ContentId,
    totals: BlueprintTotals,
    breakdown: BudgetBreakdown,
    breaches: Vec<LimitBreach>,
}

impl BlueprintAssessment {
    /// The blueprint that was measured.
    #[must_use]
    pub const fn blueprint(&self) -> &ContentId {
        &self.blueprint
    }

    /// The airframe whose profile was applied.
    #[must_use]
    pub const fn airframe(&self) -> &ContentId {
        &self.airframe
    }

    /// The exact totals.
    #[must_use]
    pub const fn totals(&self) -> &BlueprintTotals {
        &self.totals
    }

    /// The per-category subtotals the totals are the fold of.
    #[must_use]
    pub const fn breakdown(&self) -> &BudgetBreakdown {
        &self.breakdown
    }

    /// Whether every measured quantity is inside its limit.
    ///
    /// A total exactly equal to a limit is **inside** it.
    #[must_use]
    pub fn is_within_limits(&self) -> bool {
        self.breaches.is_empty()
    }

    /// The limits this loadout is over, in the canonical quantity order.
    #[must_use]
    pub fn breaches(&self) -> &[LimitBreach] {
        &self.breaches
    }

    /// The first limit this loadout is over.
    #[must_use]
    pub fn first_breach(&self) -> Option<&LimitBreach> {
        self.breaches.first()
    }
}

impl ConstructionRules {
    /// Measures `blueprint` against this profile with exact integer
    /// arithmetic.
    ///
    /// The refusals are ordered and deterministic: the airframe must match, then
    /// every limit must be measured, then every component must be priced, then
    /// the rack usage must be known, and only then is a comparison made. An
    /// unknown at any step yields its named [`BudgetRefusal`] and no totals, so
    /// a caller can never mistake "unmeasured" for "fits".
    ///
    /// # Errors
    ///
    /// [`BudgetRefusal::AirframeMismatch`], [`BudgetRefusal::UnknownLimit`],
    /// [`BudgetRefusal::NotPriced`], [`BudgetRefusal::UnknownMass`],
    /// [`BudgetRefusal::UnknownCost`], [`BudgetRefusal::UnknownGunPositions`]
    /// or [`BudgetRefusal::Overflow`].
    pub fn assess(
        &self,
        blueprint: &AircraftBlueprint,
        book: &PriceBook,
    ) -> Result<BlueprintAssessment, BudgetRefusal> {
        if blueprint.airframe() != &self.airframe {
            return Err(BudgetRefusal::AirframeMismatch {
                blueprint: blueprint.airframe().clone(),
                rules: self.airframe.clone(),
            });
        }
        let limits = Limits::from_rules(self)?;

        let mut breakdown = BudgetBreakdown::new();
        price(
            book,
            blueprint.airframe(),
            BudgetCategory::Airframe,
            &mut breakdown,
        )?;
        price(
            book,
            blueprint.engine(),
            BudgetCategory::Engine,
            &mut breakdown,
        )?;
        for fitment in blueprint.armor() {
            price(book, fitment.armor(), BudgetCategory::Armor, &mut breakdown)?;
        }
        for fitment in blueprint.guns() {
            price(book, fitment.gun(), BudgetCategory::Guns, &mut breakdown)?;
        }
        for fitment in blueprint.ordnance() {
            price(
                book,
                fitment.ordnance(),
                BudgetCategory::Ordnance,
                &mut breakdown,
            )?;
        }
        for item in blueprint.equipment() {
            price(book, item, BudgetCategory::Equipment, &mut breakdown)?;
        }

        let mut gun_positions: u32 = 0;
        for fitment in blueprint.guns() {
            let positions =
                fitment
                    .known_positions()
                    .ok_or_else(|| BudgetRefusal::UnknownGunPositions {
                        component: fitment.gun().clone(),
                    })?;
            gun_positions =
                gun_positions
                    .checked_add(positions)
                    .ok_or(BudgetRefusal::Overflow {
                        quantity: BudgetQuantity::GunPositions,
                        category: None,
                    })?;
        }
        let rocket_hardpoints =
            u32::try_from(blueprint.ordnance().len()).map_err(|_| BudgetRefusal::Overflow {
                quantity: BudgetQuantity::RocketHardpoints,
                category: None,
            })?;

        let totals = BlueprintTotals::from_breakdown(&breakdown, gun_positions, rocket_hardpoints)?;

        let mut breaches = Vec::new();
        if totals.mass > limits.mass {
            breaches.push(LimitBreach::Mass {
                limit: limits.mass,
                total: totals.mass,
            });
        }
        if totals.cost > limits.cost {
            breaches.push(LimitBreach::Cost {
                limit: limits.cost,
                total: totals.cost,
            });
        }
        if gun_positions > limits.gun_positions {
            breaches.push(LimitBreach::GunPositions {
                limit: limits.gun_positions,
                used: gun_positions,
            });
        }
        if rocket_hardpoints > limits.rocket_hardpoints {
            breaches.push(LimitBreach::RocketHardpoints {
                limit: limits.rocket_hardpoints,
                used: rocket_hardpoints,
            });
        }

        Ok(BlueprintAssessment {
            blueprint: blueprint.id().clone(),
            airframe: self.airframe.clone(),
            totals,
            breakdown,
            breaches,
        })
    }
}

// ------------------------------------------------------ shared validator ----

/// The host-and-catalog side of the shared validator (F44-B): which guns may
/// be mated, which components the host bans and which components exist for
/// this caller at all.
///
/// One policy value is built per caller — the campaign boundary derives
/// availability from the profile's owned and purchasable items, Instant Action
/// and multiplayer from the host's catalog — and **every** caller runs the same
/// [`ConstructionRules::validate`], so an imported blueprint meets the rules a
/// hand-built one does (sheet AC03).
///
/// The pairing rule is [`Resolved`]: which guns the original lets a player mate
/// is unmeasured (F44-D), so an unmeasured rule refuses a paired selection
/// instead of allowing or forbidding it by guess.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConstructionPolicy {
    pairable_guns: Resolved<BTreeSet<ContentId>>,
    banned: BTreeSet<ContentId>,
    available: BTreeSet<ContentId>,
}

impl ConstructionPolicy {
    /// A policy over the declared pairing rule, the host's banned list and the
    /// components available to this caller.
    #[must_use]
    pub const fn new(
        pairable_guns: Resolved<BTreeSet<ContentId>>,
        banned: BTreeSet<ContentId>,
        available: BTreeSet<ContentId>,
    ) -> Self {
        Self {
            pairable_guns,
            banned,
            available,
        }
    }

    /// The declared pairing rule.
    #[must_use]
    pub const fn pairable_guns(&self) -> &Resolved<BTreeSet<ContentId>> {
        &self.pairable_guns
    }

    /// The components the host forbids regardless of availability.
    #[must_use]
    pub const fn banned(&self) -> &BTreeSet<ContentId> {
        &self.banned
    }

    /// The components this caller may use.
    #[must_use]
    pub const fn available(&self) -> &BTreeSet<ContentId> {
        &self.available
    }

    /// The same policy with `component` banned.
    #[must_use]
    pub fn with_banned(mut self, component: ContentId) -> Self {
        self.banned.insert(component);
        self
    }
}

/// One rule a blueprint breaks. A violation is a measured verdict, unlike a
/// [`ValidationRefusal`], which is an unknown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConstraintViolation {
    /// A gun fitment occupies a number of positions other than one (single) or
    /// two (pair), the only selections non-negotiable 1 observes.
    UnsupportedGunSelection {
        /// The mount.
        mount: DamageNodeKey,
        /// The positions it claimed.
        positions: u32,
    },
    /// A paired gun fitment names a gun the pairing rule does not let be mated.
    GunNotPairable {
        /// The gun.
        gun: ContentId,
        /// The mount.
        mount: DamageNodeKey,
    },
    /// The host bans the component.
    BannedComponent {
        /// The banned component.
        component: ContentId,
        /// The category it was fitted into.
        category: BudgetCategory,
    },
    /// The component is not available to this caller.
    Unavailable {
        /// The unavailable component.
        component: ContentId,
        /// The category it was fitted into.
        category: BudgetCategory,
    },
}

impl fmt::Display for ConstraintViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedGunSelection { positions, .. } => write!(
                f,
                "a gun selection of {positions} positions is neither single nor pair"
            ),
            Self::GunNotPairable { gun, .. } => write!(f, "{gun} cannot be mated as a pair"),
            Self::BannedComponent {
                component,
                category,
            } => write!(f, "{component} ({category}) is banned by the host"),
            Self::Unavailable {
                component,
                category,
            } => write!(f, "{component} ({category}) is not available"),
        }
    }
}

/// Why a blueprint could not be judged at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidationRefusal {
    /// The budget arithmetic could not be measured.
    Budget(BudgetRefusal),
    /// A paired selection exists but the pairing rule is unmeasured.
    UnknownPairingRule {
        /// The paired gun.
        gun: ContentId,
    },
}

impl fmt::Display for ValidationRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Budget(refusal) => write!(f, "{refusal}"),
            Self::UnknownPairingRule { gun } => write!(
                f,
                "the paired-gun rule is unmeasured, so a pair of {gun} cannot be judged"
            ),
        }
    }
}

impl std::error::Error for ValidationRefusal {}

impl From<BudgetRefusal> for ValidationRefusal {
    fn from(refusal: BudgetRefusal) -> Self {
        Self::Budget(refusal)
    }
}

/// The validator's whole answer: exact totals plus every broken limit and
/// constraint. Warnings are not produced yet: no original warning rule is
/// known, so none is invented.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlueprintVerdict {
    assessment: BlueprintAssessment,
    violations: Vec<ConstraintViolation>,
}

impl BlueprintVerdict {
    /// The exact totals and limit breaches.
    #[must_use]
    pub const fn assessment(&self) -> &BlueprintAssessment {
        &self.assessment
    }

    /// Every broken constraint, in blueprint order.
    #[must_use]
    pub fn violations(&self) -> &[ConstraintViolation] {
        &self.violations
    }

    /// Whether the blueprint is inside every limit and breaks no constraint.
    /// Reads integers and sets only.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.assessment.is_within_limits() && self.violations.is_empty()
    }
}

impl AircraftBlueprint {
    /// Every component this blueprint names, with the category it is fitted
    /// into, in the order the budget prices them.
    #[must_use]
    pub fn components(&self) -> Vec<(BudgetCategory, &ContentId)> {
        let mut out = vec![
            (BudgetCategory::Airframe, &self.airframe),
            (BudgetCategory::Engine, &self.engine),
        ];
        out.extend(self.armor.iter().map(|a| (BudgetCategory::Armor, &a.armor)));
        out.extend(self.guns.iter().map(|g| (BudgetCategory::Guns, &g.gun)));
        out.extend(
            self.ordnance
                .iter()
                .map(|o| (BudgetCategory::Ordnance, &o.ordnance)),
        );
        out.extend(
            self.equipment
                .iter()
                .map(|e| (BudgetCategory::Equipment, e)),
        );
        out
    }
}

impl ConstructionRules {
    /// The one validator every construction path shares: the exact budget
    /// ([`Self::assess`]) plus the paired-gun, banned-component and
    /// availability constraints of `policy`.
    ///
    /// Symmetry is not a constraint: an asymmetric loadout is as valid as a
    /// mirrored one.
    ///
    /// # Errors
    ///
    /// [`ValidationRefusal`] when the budget cannot be measured or a paired
    /// selection meets an unmeasured pairing rule.
    pub fn validate(
        &self,
        policy: &ConstructionPolicy,
        blueprint: &AircraftBlueprint,
        book: &PriceBook,
    ) -> Result<BlueprintVerdict, ValidationRefusal> {
        let assessment = self.assess(blueprint, book)?;
        let mut violations = Vec::new();
        for (category, component) in blueprint.components() {
            if policy.banned.contains(component) {
                violations.push(ConstraintViolation::BannedComponent {
                    component: component.clone(),
                    category,
                });
            }
            if !policy.available.contains(component) {
                violations.push(ConstraintViolation::Unavailable {
                    component: component.clone(),
                    category,
                });
            }
        }
        for fitment in blueprint.guns() {
            // `assess` refused an unknown position count, so it is known here.
            let Some(positions) = fitment.known_positions() else {
                continue;
            };
            match positions {
                1 => {}
                2 => match policy.pairable_guns.clone().known() {
                    None => {
                        return Err(ValidationRefusal::UnknownPairingRule {
                            gun: fitment.gun().clone(),
                        });
                    }
                    Some(pairable) if !pairable.contains(fitment.gun()) => {
                        violations.push(ConstraintViolation::GunNotPairable {
                            gun: fitment.gun().clone(),
                            mount: fitment.mount().clone(),
                        });
                    }
                    Some(_) => {}
                },
                other => violations.push(ConstraintViolation::UnsupportedGunSelection {
                    mount: fitment.mount().clone(),
                    positions: other,
                }),
            }
        }
        Ok(BlueprintVerdict {
            assessment,
            violations,
        })
    }
}

/// A policy for the synthetic fixture: the fixture gun is pairable, nothing is
/// banned and every priced fixture component is available.
#[must_use]
pub fn synthetic_policy() -> ConstructionPolicy {
    let mut available = BTreeSet::new();
    for (kind, key) in [
        (ContentKind::Airframe, SYNTHETIC_AIRFRAME_KEY),
        (ContentKind::Engine, SYNTHETIC_ENGINE_KEY),
        (ContentKind::Armor, SYNTHETIC_PLATE_KEY),
        (ContentKind::Armor, SYNTHETIC_HEAVY_PLATE_KEY),
        (ContentKind::Weapon, SYNTHETIC_GUN_KEY),
        (ContentKind::Weapon, SYNTHETIC_MISSILE_KEY),
        (ContentKind::Weapon, SYNTHETIC_HEAVY_MISSILE_KEY),
        (ContentKind::HardpointEquipment, SYNTHETIC_RADIO_KEY),
    ] {
        available.insert(fixture_id(kind, key));
    }
    ConstructionPolicy::new(
        known(BTreeSet::from([fixture_id(
            ContentKind::Weapon,
            SYNTHETIC_GUN_KEY,
        )])),
        BTreeSet::new(),
        available,
    )
}

// ------------------------------------------------------------- fixture ------

/// The synthetic fixture's airframe key.
pub const SYNTHETIC_AIRFRAME_KEY: &str = "fixture.synthetic_fighter";
/// The synthetic fixture's second airframe key, used only for the
/// airframe-mismatch refusal.
pub const SYNTHETIC_OTHER_AIRFRAME_KEY: &str = "fixture.synthetic_other_fighter";
/// The synthetic fixture's engine key.
pub const SYNTHETIC_ENGINE_KEY: &str = "fixture.synthetic_inline";
/// The synthetic fixture's armor key.
pub const SYNTHETIC_PLATE_KEY: &str = "fixture.synthetic_plate";
/// A plate exactly one weight unit heavier than [`SYNTHETIC_PLATE_KEY`].
pub const SYNTHETIC_HEAVY_PLATE_KEY: &str = "fixture.synthetic_heavy_plate";
/// A component the price book deliberately does not price.
pub const SYNTHETIC_ABSENT_PLATE_KEY: &str = "fixture.synthetic_absent_plate";
/// The synthetic fixture's gun key.
pub const SYNTHETIC_GUN_KEY: &str = "fixture.synthetic_gun";
/// The synthetic fixture's missile key.
pub const SYNTHETIC_MISSILE_KEY: &str = "fixture.synthetic_missile";
/// A missile exactly one minor unit dearer than [`SYNTHETIC_MISSILE_KEY`].
pub const SYNTHETIC_HEAVY_MISSILE_KEY: &str = "fixture.synthetic_heavy_missile";
/// The synthetic fixture's equipment key.
pub const SYNTHETIC_RADIO_KEY: &str = "fixture.synthetic_radio";
/// An engine whose mass is an explicit unknown.
pub const SYNTHETIC_UNMEASURED_ENGINE_KEY: &str = "fixture.synthetic_unmeasured_engine";
/// A plate whose price is an explicit unknown.
pub const SYNTHETIC_UNPRICED_PLATE_KEY: &str = "fixture.synthetic_unpriced_plate";
/// An engine heavy enough to overflow the weight sum.
pub const SYNTHETIC_OVERFLOW_ENGINE_KEY: &str = "fixture.synthetic_enormous_engine";
/// The synthetic fixture's blueprint key.
pub const SYNTHETIC_BLUEPRINT_KEY: &str = "fixture.synthetic_boundary_blueprint";
/// The synthetic fixture's paint mask key.
pub const SYNTHETIC_PAINT_MASK_KEY: &str = "fixture.synthetic_base_mask";
/// The synthetic fixture's decal key.
pub const SYNTHETIC_DECAL_KEY: &str = "fixture.synthetic_decal";

/// The boundary blueprint's exact weight, in game-weight units.
///
/// 4000 airframe + 600 engine + 2x250 armor + 4x160 guns + 4x65 missiles +
/// 40 radio. It is **exactly** [`SYNTHETIC_BOUNDARY_MASS_LIMIT_UNITS`], so
/// swapping in one heavier plate is the one-unit-over case AC01 names.
pub const SYNTHETIC_BOUNDARY_MASS_UNITS: u64 = 6_040;
/// The boundary blueprint's exact price, in minor currency units.
///
/// 12000 airframe + 3000 engine + 2x1000 armor + 4x1500 guns + 4x4250 missiles
/// + 2000 radio, which is exactly [`SYNTHETIC_BOUNDARY_COST_LIMIT_MINOR`].
///
/// The four missiles cost 4250 each against a 1500 gun on purpose: the numbers
/// are chosen so the totals land on round boundaries, not to resemble a
/// plausible price list. No original price has been read.
pub const SYNTHETIC_BOUNDARY_COST_MINOR: u64 = 42_000;

/// The boundary rule profile's weight ceiling, equal to the blueprint's total.
pub const SYNTHETIC_BOUNDARY_MASS_LIMIT_UNITS: u64 = 6_040;
/// The boundary rule profile's price ceiling, equal to the blueprint's total.
pub const SYNTHETIC_BOUNDARY_COST_LIMIT_MINOR: u64 = 42_000;
/// The boundary rule profile's gun-position count: the manual's observed four.
pub const SYNTHETIC_BOUNDARY_GUN_POSITIONS: u32 = 4;
/// The boundary rule profile's rocket hardpoint count: the manual's observed
/// "up to eight".
pub const SYNTHETIC_BOUNDARY_ROCKET_HARDPOINTS: u32 = 8;

fn claim(id: &str) -> ClaimId {
    ClaimId::new(id).expect("the declared claim id is valid")
}

fn designed(id: &str) -> Provenance {
    Provenance::designed(claim(id))
}

fn known<T: Clone>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, designed("f44a.fixture.value")))
}

fn unknown<T>(claim: &str, reason: &str) -> Resolved<T> {
    Resolved::unknown(ClaimId::new(claim).expect("the claim id is valid"), reason)
        .expect("a reason is present")
}

fn fixture_id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("the synthetic fixture id is valid")
}

fn fixture_key(key: &str) -> DamageNodeKey {
    DamageNodeKey::new(key).expect("the synthetic fixture node key is valid")
}

/// The fixture's declared component masses and prices.
///
/// Every value is **designed** with no original counterpart: the original
/// component catalog, its masses and its prices are unmeasured (F44-D). Four
/// entries exist only to make a refusal reachable — an engine of unknown mass,
/// a plate of unknown price, an engine heavy enough to overflow the `u64`
/// weight sum, and a plate the book simply does not quote.
#[must_use]
pub fn declared_synthetic_price_book() -> PriceBook {
    let quote = |mass: u64, cost: u64| {
        ComponentQuote::new(known(WeightUnits::new(mass)), known(MoneyMinor::new(cost)))
    };
    PriceBook::try_new(vec![
        (
            fixture_id(ContentKind::Airframe, SYNTHETIC_AIRFRAME_KEY),
            quote(4_000, 12_000),
        ),
        (
            fixture_id(ContentKind::Airframe, SYNTHETIC_OTHER_AIRFRAME_KEY),
            quote(4_400, 13_000),
        ),
        (
            fixture_id(ContentKind::Engine, SYNTHETIC_ENGINE_KEY),
            quote(600, 3_000),
        ),
        (
            fixture_id(ContentKind::Armor, SYNTHETIC_PLATE_KEY),
            quote(250, 1_000),
        ),
        (
            fixture_id(ContentKind::Armor, SYNTHETIC_HEAVY_PLATE_KEY),
            quote(251, 1_000),
        ),
        (
            fixture_id(ContentKind::Armor, SYNTHETIC_UNPRICED_PLATE_KEY),
            ComponentQuote::new(
                known(WeightUnits::new(250)),
                unknown(
                    "f44a.fixture.unpriced-plate.cost",
                    "no original component price was measured",
                ),
            ),
        ),
        (
            fixture_id(ContentKind::Weapon, SYNTHETIC_GUN_KEY),
            quote(160, 1_500),
        ),
        (
            fixture_id(ContentKind::Weapon, SYNTHETIC_MISSILE_KEY),
            quote(65, 4_250),
        ),
        (
            fixture_id(ContentKind::Weapon, SYNTHETIC_HEAVY_MISSILE_KEY),
            quote(65, 4_251),
        ),
        (
            fixture_id(ContentKind::HardpointEquipment, SYNTHETIC_RADIO_KEY),
            quote(40, 2_000),
        ),
        (
            fixture_id(ContentKind::Engine, SYNTHETIC_UNMEASURED_ENGINE_KEY),
            ComponentQuote::new(
                unknown(
                    "f44a.fixture.unmeasured-engine.mass",
                    "no original component mass was measured",
                ),
                known(MoneyMinor::new(3_000)),
            ),
        ),
        (
            fixture_id(ContentKind::Engine, SYNTHETIC_OVERFLOW_ENGINE_KEY),
            quote(u64::MAX, 3_000),
        ),
    ])
    .expect("the synthetic price book is valid")
}

/// The boundary rule profile: the fixture airframe's four gun positions, eight
/// rocket hardpoints, and weight and price ceilings **exactly equal** to the
/// boundary blueprint's totals.
///
/// The two ceilings are deliberately equal to the blueprint rather than rounded
/// above it, because AC01 is a statement about the boundary itself: this profile
/// is the one where "exactly at the limit" is a state that actually occurs.
#[must_use]
pub fn synthetic_boundary_rules() -> ConstructionRules {
    ConstructionRules::try_new(
        fixture_id(ContentKind::Airframe, SYNTHETIC_AIRFRAME_KEY),
        known(SYNTHETIC_BOUNDARY_GUN_POSITIONS),
        known(SYNTHETIC_BOUNDARY_ROCKET_HARDPOINTS),
        known(WeightUnits::new(SYNTHETIC_BOUNDARY_MASS_LIMIT_UNITS)),
        known(MoneyMinor::new(SYNTHETIC_BOUNDARY_COST_LIMIT_MINOR)),
        Origin::SyntheticFixture,
        designed("f44a.rules.synthetic-boundary"),
    )
    .expect("the synthetic boundary rule profile is valid")
}

/// A second rule profile for the **same** airframe with a different rack: six
/// gun positions and only two rocket hardpoints.
///
/// It exists to make non-negotiable 1 executable. The manual's four positions
/// and eight hardpoints are *profile data*, so a loadout that the boundary
/// profile refuses for its rack can be inside this one, and the rack numbers a
/// comparison uses are demonstrably read from the profile argument rather than
/// compiled in as the constant `4`.
#[must_use]
pub fn synthetic_wide_rack_rules() -> ConstructionRules {
    ConstructionRules::try_new(
        fixture_id(ContentKind::Airframe, SYNTHETIC_AIRFRAME_KEY),
        known(6),
        known(2),
        known(WeightUnits::new(SYNTHETIC_BOUNDARY_MASS_LIMIT_UNITS)),
        known(MoneyMinor::new(SYNTHETIC_BOUNDARY_COST_LIMIT_MINOR)),
        Origin::SyntheticFixture,
        designed("f44a.rules.synthetic-wide-rack"),
    )
    .expect("the synthetic wide-rack rule profile is valid")
}

/// A rule profile whose weight ceiling is an explicit unknown.
///
/// An unmeasured limit is refused, never read as "no limit": a construction
/// screen cannot certify a loadout against a ceiling nobody measured.
#[must_use]
pub fn synthetic_unmeasured_limit_rules() -> ConstructionRules {
    ConstructionRules::try_new(
        fixture_id(ContentKind::Airframe, SYNTHETIC_AIRFRAME_KEY),
        known(SYNTHETIC_BOUNDARY_GUN_POSITIONS),
        known(SYNTHETIC_BOUNDARY_ROCKET_HARDPOINTS),
        unknown(
            "f44a.rules.synthetic-unmeasured.mass-limit",
            "no original weight ceiling was measured",
        ),
        known(MoneyMinor::new(SYNTHETIC_BOUNDARY_COST_LIMIT_MINOR)),
        Origin::SyntheticFixture,
        designed("f44a.rules.synthetic-unmeasured-limit"),
    )
    .expect("the synthetic unmeasured-limit rule profile is valid")
}

/// The fixture blueprint's two armor fitments, in authored order.
#[must_use]
pub fn synthetic_armor_fitments() -> Vec<ArmorFitment> {
    vec![
        ArmorFitment::try_new(
            fixture_key("armor_zone_fuselage"),
            fixture_id(ContentKind::Armor, SYNTHETIC_PLATE_KEY),
        )
        .expect("the fixture armor fitment is valid"),
        ArmorFitment::try_new(
            fixture_key("armor_zone_wing"),
            fixture_id(ContentKind::Armor, SYNTHETIC_PLATE_KEY),
        )
        .expect("the fixture armor fitment is valid"),
    ]
}

/// The fixture blueprint's four gun fitments, one per weapon-mount node, each
/// occupying a single gun position.
#[must_use]
pub fn synthetic_gun_fitments() -> Vec<GunFitment> {
    ["gun_mount_1", "gun_mount_2", "gun_mount_3", "gun_mount_4"]
        .iter()
        .map(|mount| {
            GunFitment::try_new(
                fixture_id(ContentKind::Weapon, SYNTHETIC_GUN_KEY),
                fixture_key(mount),
                known(1),
            )
            .expect("the fixture gun fitment is valid")
        })
        .collect()
}

/// The fixture blueprint's four rocket hardpoint fitments, in authored order.
#[must_use]
pub fn synthetic_ordnance_fitments() -> Vec<OrdnanceFitment> {
    ["hardpoint_1", "hardpoint_2", "hardpoint_3", "hardpoint_4"]
        .iter()
        .map(|hardpoint| {
            OrdnanceFitment::try_new(
                fixture_key(hardpoint),
                fixture_id(ContentKind::Weapon, SYNTHETIC_MISSILE_KEY),
            )
            .expect("the fixture ordnance fitment is valid")
        })
        .collect()
}

/// The fixture blueprint's paint selection: one mask and one decal over it,
/// both catalog references with no pixel data.
#[must_use]
pub fn synthetic_paint_selection() -> PaintSelection {
    let mask = fixture_id(ContentKind::PaintMask, SYNTHETIC_PAINT_MASK_KEY);
    PaintSelection::try_new(
        vec![mask.clone()],
        vec![
            DecalPlacement::try_new(
                fixture_id(ContentKind::PaintMask, SYNTHETIC_DECAL_KEY),
                mask,
            )
            .expect("the fixture decal placement is valid"),
        ],
    )
    .expect("the fixture paint selection is valid")
}

/// The synthetic boundary blueprint: four guns, four missiles, two plates, one
/// engine, one radio and a paint selection.
///
/// It sits **exactly** on all four of the boundary profile's limits, so it is
/// the AC01 "exactly at the limit" case and every other fixture is a one-unit or
/// one-slot move away from it.
#[must_use]
pub fn declared_synthetic_blueprint() -> AircraftBlueprint {
    AircraftBlueprint::try_new(
        fixture_id(ContentKind::Blueprint, SYNTHETIC_BLUEPRINT_KEY),
        fixture_id(ContentKind::Airframe, SYNTHETIC_AIRFRAME_KEY),
        fixture_id(ContentKind::Engine, SYNTHETIC_ENGINE_KEY),
        synthetic_armor_fitments(),
        synthetic_gun_fitments(),
        synthetic_ordnance_fitments(),
        vec![fixture_id(
            ContentKind::HardpointEquipment,
            SYNTHETIC_RADIO_KEY,
        )],
        synthetic_paint_selection(),
        Origin::SyntheticFixture,
        designed("f44a.blueprint.synthetic-boundary"),
    )
    .expect("the synthetic boundary blueprint is valid")
}

/// The synthetic boundary blueprint with a different engine fitted.
///
/// # Errors
///
/// [`ConstructionSchemaError::NotAnAirframe`] never fires here; the error is
/// returned so a caller can propagate it without an unwrap.
pub fn synthetic_blueprint_with_engine(
    engine: ContentId,
) -> Result<AircraftBlueprint, ConstructionSchemaError> {
    AircraftBlueprint::try_new(
        fixture_id(ContentKind::Blueprint, SYNTHETIC_BLUEPRINT_KEY),
        fixture_id(ContentKind::Airframe, SYNTHETIC_AIRFRAME_KEY),
        engine,
        synthetic_armor_fitments(),
        synthetic_gun_fitments(),
        synthetic_ordnance_fitments(),
        vec![fixture_id(
            ContentKind::HardpointEquipment,
            SYNTHETIC_RADIO_KEY,
        )],
        synthetic_paint_selection(),
        Origin::SyntheticFixture,
        designed("f44a.blueprint.synthetic-boundary"),
    )
}
