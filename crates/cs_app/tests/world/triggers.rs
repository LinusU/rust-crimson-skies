//! Task #427 acceptance tests: the retail trigger-volume measurement, and what
//! it does and does not decide.
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`
//! (F18-D's evidence stage and F18-C's overlay layer). Task test prefix:
//! `accept_t427_`.
//!
//! Task #401 fixed the *hold* a swept body paid at a trigger volume's face and
//! left the *report* boundary: a volume thinner than one tick of the reaching
//! body's travel is never sampled. Task #427 asked for the one number nobody
//! had — how thick an **original** trigger volume is — and for the verdict that
//! number implies for a fast aircraft. F18-C's depot volume (1 m) and F18-A's
//! arch volume (8 m) are fixture choices, so they cannot answer it.
//!
//! Nine of the twelve tests here read no original data and CI runs them: they
//! pin the contract — that a stored extent is never a length in metres, that the
//! one-tick verdict refuses to decide without a unit factor and reports the
//! factor at which it would flip, that a zone with no thickness cannot be
//! thickened by any factor, and that the survey's own refusals are typed and
//! name the zone they refused. The other three need `CS_GAME_DIR` and are
//! ignored in CI: they measure the real corpus through the production node
//! reader.
//!
//! Nothing here is `verified_original`: no original run happened, and reading
//! the installation's files is not evidence of how the game behaves.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use cs_app::world::triggers::{
    TriggerVolumeSurveyError, ZONE_PREFIX, survey_retail_trigger_volumes, zone_box_field,
};
use cs_content::world::{
    DETECTION_ZONE_PARENT, DETECTION_ZONE_PREFIX, MissionZoneDeclaration, RetailTriggerVolume,
    RetailTriggerVolumeSurvey, StoredVolume, TriggerTickVerdict, TriggerVolumeError,
    TriggerVolumeSpan, WorldId, ZoneDeclarationKey, is_detection_zone_name,
};

/// The tick rate the world simulation runs at (F23-A's fixed schedule).
const TICK_HZ: f64 = 120.0;

/// A fast aircraft's speed, the one task #401 measured at (3.33 m per tick).
const FAST_M_S: f64 = 400.0;

/// An ordinary speed, 0.5 m per tick at [`TICK_HZ`].
const ORDINARY_M_S: f64 = 60.0;

// -------------------------------------------------------------- fixtures ---

/// The synthetic survey every unignored contract test is built from: two zones
/// in one world and one in a second, all authored stored extents.
fn synthetic_survey(scale: Option<f64>) -> RetailTriggerVolumeSurvey {
    build_survey(scale).expect("the synthetic survey is well formed")
}

/// The same survey before its own validation, so a test can reach the refusals.
fn build_survey(scale: Option<f64>) -> Result<RetailTriggerVolumeSurvey, TriggerVolumeError> {
    let world = WorldId::from_key("c5").expect("a valid world key");
    let other = WorldId::from_key("c3").expect("a valid world key");
    let volume = |world: &WorldId, zone: &str, extent: [f64; 3]| {
        RetailTriggerVolume::new(
            TriggerVolumeSpan::new(
                world.clone(),
                format!("ZBD/{}/{zone}/gamez.zbd", world.key().to_uppercase()),
                "0".repeat(64),
                4636,
                6_242_124,
                212,
            ),
            zone.to_owned(),
            Some(949),
            StoredVolume::new([0.0, 0.0, 0.0], extent).expect("a well-formed stored box"),
        )
    };
    RetailTriggerVolumeSurvey::new(
        "a".repeat(64),
        scale,
        vec![
            volume(
                &world,
                "dzpath1",
                [64.0, 88.602_294_921_875, 631.100_585_937_5],
            ),
            volume(&world, "dzpath2", [631.1, 107.561, 2261.557]),
            volume(&other, "dzpath3", [32.0, 74.0, 400.0]),
        ],
    )
}

// ------------------------------------------- the name rule, measured or not ---

/// The rule that decides which world node is a numbered detection zone is the
/// measured one: the prefix plus at least one decimal digit and nothing else.
///
/// The parent node carries the prefix and no digits, and a fixture name a
/// measured corpus does not contain must be refused rather than truncated.
#[test]
fn accept_t427_the_zone_name_rule_is_the_measured_one_and_nothing_else() {
    assert!(
        is_detection_zone_name("dzpath1"),
        "a measured zone name must be accepted"
    );
    assert!(
        is_detection_zone_name(&format!("{DETECTION_ZONE_PREFIX}34")),
        "the two-digit measured names must be accepted"
    );
    assert!(
        !is_detection_zone_name(DETECTION_ZONE_PARENT),
        "the parent node carries no zone of its own: accepting it would report a \
         volume the store does not have"
    );
    assert!(
        !is_detection_zone_name(&format!("{DETECTION_ZONE_PREFIX}1_backup")),
        "a suffix that is not digits is not a zone: truncating it to {DETECTION_ZONE_PREFIX}1 \
         would attribute another node's volume to a zone"
    );
    assert!(
        !is_detection_zone_name(&format!("{DETECTION_ZONE_PREFIX}_")),
        "a non-digit suffix is not a zone index"
    );
    assert!(
        !is_detection_zone_name("dzpath"),
        "the bare prefix names no zone"
    );
    assert!(
        !is_detection_zone_name("hangar"),
        "an ordinary node is not a zone"
    );
}

// ------------------------------------- a stored extent is not a length ---

/// A stored extent is stored units, and the survey refuses to compare it with
/// one tick of canonical travel while the unit is unmeasured — reporting the
/// factor at which the answer would flip instead.
///
/// This is the whole of task #427's second acceptance criterion in its current
/// state, and it is a refusal with a number in it: the break-even factor is
/// what a later stage has to go and measure.
#[test]
fn accept_t427_a_stored_extent_is_never_read_as_a_length() {
    let survey = synthetic_survey(None);
    assert_eq!(
        survey.vertex_scale_to_m(),
        None,
        "no measurement in this workspace has established the original's world-vertex unit"
    );

    let verdict = survey
        .tick_verdict(FAST_M_S, TICK_HZ)
        .expect("a finite speed and a non-zero rate are valid");
    assert_eq!(
        verdict.travel_m_per_tick(),
        Some(FAST_M_S / TICK_HZ),
        "one tick of travel is a canonical-metre quantity and is computable without the unit"
    );
    assert!(
        !verdict.is_decided(),
        "the survey must not claim an answer while the stored unit is unmeasured"
    );
    match &verdict {
        TriggerTickVerdict::UnitUnmeasured {
            thinnest_stored_extent,
            thinnest_zone,
            break_even_meters_per_unit,
            travel_m_per_tick,
            ..
        } => {
            assert_eq!(*thinnest_stored_extent, 32.0, "the thinnest authored zone");
            assert_eq!(thinnest_zone, "dzpath3", "the zone it came from is named");
            assert!(
                (break_even_meters_per_unit - (FAST_M_S / TICK_HZ) / 32.0).abs() < 1.0e-12,
                "the break-even factor is one tick's travel divided by the thinnest stored extent, \
                 so it is the exact value at which the verdict flips: {break_even_meters_per_unit}"
            );
            assert!(
                *break_even_meters_per_unit < 0.11,
                "the measured corpus would have to make one stored unit worth more than ~11 cm for \
                 the thinnest zone to be outrun at {FAST_M_S} m/s, which is why the factor is \
                 worth reporting rather than assuming"
            );
            assert!(
                *travel_m_per_tick > 0.0,
                "the tick travel is a real quantity whatever the verdict is"
            );
        }
        other => panic!("the verdict must be UnitUnmeasured, got {other:?}"),
    }
    assert_eq!(
        verdict.break_even_meters_per_unit(),
        verdict.travel_m_per_tick().map(|travel| travel / 32.0),
        "the break-even factor is reachable without matching on the variant"
    );
}

/// The same survey, with a unit factor supplied, answers instead of refusing —
/// and answers *both* ways, so the refusal above is the unit's doing and not the
/// verdict's.
#[test]
fn accept_t427_the_unit_factor_decides_the_verdict_and_only_the_unit_factor() {
    // One stored unit is one metre: the 32-unit zone is 32 m, far more than the
    // 3.33 m a tick buys at 400 m/s.
    let metres = synthetic_survey(Some(1.0));
    let decided = metres
        .tick_verdict(FAST_M_S, TICK_HZ)
        .expect("a valid tick");
    assert!(decided.is_decided());
    match &decided {
        TriggerTickVerdict::EveryZoneSpansATick {
            thinnest_m,
            thinnest_zone,
            travel_m_per_tick,
            ..
        } => {
            assert_eq!(*thinnest_m, 32.0, "32 stored units at one metre each");
            assert_eq!(thinnest_zone, "dzpath3");
            assert!(*thinnest_m > *travel_m_per_tick);
        }
        other => panic!("every zone is thicker than a tick, got {other:?}"),
    }

    // One stored unit is a fifth of a metre: the same 32-unit zone is 6.4 m,
    // still thicker than a tick.
    let fifths = synthetic_survey(Some(0.2));
    assert!(matches!(
        fifths
            .tick_verdict(FAST_M_S, TICK_HZ)
            .expect("a valid tick"),
        TriggerTickVerdict::EveryZoneSpansATick { .. }
    ));
    // …but 6.4 m is more than two ticks at 400 m/s, so at a rate coarser than
    // the 120 Hz the world runs at, the same zone can be stepped over. The
    // verdict is about the tick, not about the zone alone.
    assert!(matches!(
        fifths.tick_verdict(FAST_M_S, 30.0).expect("a valid tick"),
        TriggerTickVerdict::ThinnestZoneOutrun { .. }
    ));
    // And an ordinary aircraft never outruns it, at any of these factors.
    assert!(matches!(
        fifths
            .tick_verdict(ORDINARY_M_S, 30.0)
            .expect("a valid tick"),
        TriggerTickVerdict::EveryZoneSpansATick { .. }
    ));
}

/// A zone with **no thickness** is the one stored box a unit factor cannot
/// change, and the API has to say so rather than report a finite number nobody
/// could act on.
///
/// `StoredVolume::new` accepts a degenerate box because a plane is a real
/// authored volume, and the survey keeps it — an all-zero box is a different
/// state and is refused separately. So a zero-thickness zone is reachable, and
/// `travel / 0` is `+inf`: a zone with no thickness is thinner than one tick
/// under **every** factor, so no finite factor flips the verdict. `Some(inf)` and
/// not `None`, because the zone was measured and the comparison was made; and
/// `inf` and not a large finite number, because a consumer has to be able to see
/// that there is no finite one.
#[test]
fn accept_t427_a_zone_with_no_thickness_is_outrun_by_every_tick_and_never_flips() {
    let world = WorldId::from_key("c4").expect("a valid world key");
    let plane = |scale| {
        RetailTriggerVolumeSurvey::new(
            "a".repeat(64),
            scale,
            vec![RetailTriggerVolume::new(
                TriggerVolumeSpan::new(
                    world.clone(),
                    "ZBD/C4/gamez.zbd",
                    "0".repeat(64),
                    0,
                    512,
                    212,
                ),
                "dzpath1".to_owned(),
                Some(949),
                // A plane: zero along `y`, 40 units along `x`.
                StoredVolume::new([0.0, 500.0, 0.0], [40.0, 500.0, 0.0])
                    .expect("a flat box is a volume"),
            )],
        )
        .expect("the plane survey is well formed")
    };

    // With no factor the survey is still undecided — it does not know how thick a
    // stored unit is — and the factor it reports says there is no finite one.
    let unmeasured = plane(None);
    let verdict = unmeasured
        .tick_verdict(FAST_M_S, TICK_HZ)
        .expect("a finite speed and a positive rate");
    assert!(!verdict.is_decided());
    match &verdict {
        TriggerTickVerdict::UnitUnmeasured {
            thinnest_stored_extent,
            break_even_meters_per_unit,
            ..
        } => {
            assert_eq!(*thinnest_stored_extent, 0.0, "the plane has no thickness");
            assert_eq!(
                *break_even_meters_per_unit,
                f64::INFINITY,
                "no finite factor makes a zero-thickness zone one tick thick"
            );
        }
        other => panic!("the verdict must be UnitUnmeasured, got {other:?}"),
    }
    assert_eq!(
        verdict.break_even_meters_per_unit(),
        Some(f64::INFINITY),
        "and the same infinity through the accessor, rather than a finite stand-in"
    );

    // With a factor supplied the answer is the one the geometry already gives:
    // a tick is thicker than the plane, so the plane is outrun. The break-even
    // stays infinite because the verdict cannot be flipped back.
    for scale in [1.0, 0.001, 1.0e-9] {
        let decided = plane(Some(scale))
            .tick_verdict(FAST_M_S, TICK_HZ)
            .expect("a valid tick");
        match &decided {
            TriggerTickVerdict::ThinnestZoneOutrun {
                thinnest_m,
                travel_m_per_tick,
                ..
            } => {
                assert_eq!(
                    *thinnest_m, 0.0,
                    "however large the factor, zero units are zero metres"
                );
                assert!(
                    *travel_m_per_tick > *thinnest_m,
                    "one tick of travel always exceeds a plane's thickness"
                );
            }
            other => panic!("a plane is outrun at every factor, got {other:?}"),
        }
        assert_eq!(
            decided.break_even_meters_per_unit(),
            Some(f64::INFINITY),
            "and no factor flips it back"
        );
    }
}

// ----------------------------------------------------- refusals are typed ---

/// Every refusal the survey can produce is typed and names what it refused:
/// a box that is not a box, a factor no comparison could use, a duplicate zone
/// name inside one world, a speed with no number in it and a rate with no tick.
#[test]
fn accept_t427_every_trigger_volume_refusal_names_what_it_refused() {
    assert_eq!(
        StoredVolume::new([0.0, 1.0, 0.0], [0.0, 0.0, 0.0]),
        Err(TriggerVolumeError::Inverted { axis: 1 }),
        "a minimum above its maximum is not a box"
    );
    assert_eq!(
        StoredVolume::new([f64::NAN, 0.0, 0.0], [1.0, 1.0, 1.0]),
        Err(TriggerVolumeError::NonFiniteCorner { corner: 0, axis: 0 }),
        "a corner no arithmetic can use is refused, not compared"
    );
    assert_eq!(
        StoredVolume::new([0.0; 3], [0.0, f64::INFINITY, 0.0]),
        Err(TriggerVolumeError::NonFiniteCorner { corner: 1, axis: 1 }),
        "the maximum corner is named as the maximum"
    );
    // NaN does not compare equal to itself, so the variant is matched instead of
    // the value: a factor that is not a number must be refused as exactly that.
    assert!(
        matches!(
            build_survey(Some(f64::NAN)),
            Err(TriggerVolumeError::NonFiniteScale { scale }) if scale.is_nan()
        ),
        "a NaN factor is refused"
    );
    assert_eq!(
        build_survey(Some(-1.0)).err(),
        Some(TriggerVolumeError::NonPositiveScale { scale: -1.0 }),
        "a negative factor would flip every length"
    );

    let world = WorldId::from_key("c5").expect("a valid world key");
    let box_of = |zone: &str| {
        RetailTriggerVolume::new(
            TriggerVolumeSpan::new(
                world.clone(),
                "ZBD/C5/gamez.zbd",
                "0".repeat(64),
                4636,
                6_242_124,
                212,
            ),
            zone.to_owned(),
            Some(949),
            StoredVolume::new([0.0; 3], [1.0, 1.0, 1.0]).expect("a box"),
        )
    };
    assert_eq!(
        RetailTriggerVolumeSurvey::new(
            "a".repeat(64),
            None,
            vec![box_of("dzpath1"), box_of("dzpath1")]
        )
        .err(),
        Some(TriggerVolumeError::DuplicateZone {
            world: "c5".to_owned(),
            zone: "dzpath1".to_owned()
        }),
        "two zones of one world claiming one name is a real state, not a tie a consumer breaks"
    );
    // The same name in two different worlds is not a duplicate: the survey is
    // keyed by (world, zone), not by name.
    let elsewhere = RetailTriggerVolume::new(
        TriggerVolumeSpan::new(
            WorldId::from_key("c3").expect("a valid world key"),
            "ZBD/C3/gamez.zbd",
            "0".repeat(64),
            4636,
            6_242_124,
            212,
        ),
        "dzpath1".to_owned(),
        Some(949),
        StoredVolume::new([0.0; 3], [1.0, 1.0, 1.0]).expect("a box"),
    );
    assert!(
        RetailTriggerVolumeSurvey::new("a".repeat(64), None, vec![box_of("dzpath1"), elsewhere])
            .is_ok(),
        "a zone name reused across world containers is two zones, not a collision"
    );

    let survey = synthetic_survey(None);
    assert!(
        matches!(
            survey.tick_verdict(f64::NAN, TICK_HZ),
            Err(TriggerVolumeError::NonFiniteSpeed { speed_m_s }) if speed_m_s.is_nan()
        ),
        "a speed that is not a number has no tick of travel"
    );
    assert_eq!(
        survey.tick_verdict(f64::INFINITY, TICK_HZ).err(),
        Some(TriggerVolumeError::NonFiniteSpeed {
            speed_m_s: f64::INFINITY
        }),
        "an infinite speed is refused by value, so the message names what it got"
    );
    assert_eq!(
        survey.tick_verdict(FAST_M_S, 0.0).err(),
        Some(TriggerVolumeError::NonPositiveTickRate { tick_hz: 0.0 }),
        "a rate of zero has no tick in it"
    );
    assert_eq!(
        survey.tick_verdict(FAST_M_S, -TICK_HZ).err(),
        Some(TriggerVolumeError::NonPositiveTickRate { tick_hz: -TICK_HZ }),
        "a negative rate would make one tick of travel a distance backwards, which compares \
         nothing"
    );
    // NaN does not compare equal to itself, so the variant is matched rather
    // than the value: a rate that is not a number is refused under the same
    // variant that refuses a negative one, so a caller has one case to handle.
    assert!(
        matches!(
            survey.tick_verdict(FAST_M_S, f64::NAN),
            Err(TriggerVolumeError::NonPositiveTickRate { tick_hz }) if tick_hz.is_nan()
        ),
        "a rate that is not a number has no tick in it either"
    );
    assert_eq!(
        survey.tick_verdict(0.0, TICK_HZ).err(),
        Some(TriggerVolumeError::NonPositiveSpeed { speed_m_s: 0.0 }),
        "a body that is not moving has no one tick of travel to be compared against"
    );
    assert_eq!(
        survey.tick_verdict(-FAST_M_S, TICK_HZ).err(),
        Some(TriggerVolumeError::NonPositiveSpeed {
            speed_m_s: -FAST_M_S
        }),
        "a negative speed would invert the comparison rather than answer it"
    );
    assert_eq!(
        RetailTriggerVolumeSurvey::new("a".repeat(64), None, Vec::new())
            .expect("an empty survey is constructible")
            .tick_verdict(FAST_M_S, TICK_HZ)
            .expect("a valid tick"),
        TriggerTickVerdict::NoZones,
        "a survey that measured nothing has nothing to compare, and says so rather than \
         reporting a pass"
    );
}

/// The survey reports its own provenance rather than being trusted: every zone
/// carries the container it came from, that container's digest, the
/// installation fingerprint, and the node's own byte span, and no zone is
/// reported twice.
#[test]
fn accept_t427_every_zone_carries_the_span_and_fingerprint_it_was_measured_from() {
    let survey = synthetic_survey(None);
    assert_eq!(survey.install_sha256().len(), 64, "a SHA-256 in hex");
    assert!(!survey.volumes().is_empty());
    let mut identities = BTreeSet::new();
    for volume in survey.volumes() {
        assert_eq!(volume.container_sha256().len(), 64, "a SHA-256 in hex");
        assert_eq!(
            volume.node_bytes(),
            212,
            "the node's own span is the 212-byte slot the production reader read"
        );
        assert!(volume.node_offset() > 0, "a node offset into the container");
        assert!(
            volume.container().ends_with("gamez.zbd"),
            "the container the bytes came from: {}",
            volume.container()
        );
        assert!(
            volume.mesh_index().is_some(),
            "a measured zone binds a mesh, which is what makes it a region of the world \
             rather than a bare marker"
        );
        assert!(
            is_detection_zone_name(volume.zone()),
            "only a numbered zone is reported: {}",
            volume.zone()
        );
        assert!(
            volume.thinnest_stored_extent() > 0.0,
            "an empty box is refused by the survey, not reported as a zone"
        );
        assert!(
            identities.insert((volume.world().key().to_owned(), volume.zone().to_owned())),
            "one row per (world, zone): {}",
            volume.zone()
        );
    }
    assert_eq!(
        survey.meshless_zones().len(),
        0,
        "every measured zone binds a mesh"
    );
    assert_eq!(
        survey
            .volumes_in(&WorldId::from_key("c5").expect("a valid world key"))
            .len(),
        2,
        "volumes_in selects by world, not by position"
    );
    // The survey's own claim about the campaign's declarations, asserted rather
    // than left in a comment: it has not decoded them, so it must not look as
    // though a mission's `dzones.zrd` had been read.
    assert!(
        !survey.zone_declarations_are_decoded(),
        "a survey nobody attached a decode to must not look as though a mission's \
         `dzones.zrd` had been read"
    );
    let attached = synthetic_survey(None)
        .with_declarations(Vec::new())
        .expect("an empty declaration set attaches");
    assert!(
        attached.zone_declarations_are_decoded(),
        "once the mission side is attached the survey says the declarations are decoded"
    );
}

/// The node info field the measurement reads, and the box-corner order it
/// assumes, are both pinned rather than left in a comment — a reader that
/// re-pointed the survey at one of the other two candidate boxes (or swapped the
/// corner order) would silently produce a different corpus with the same
/// provenance fields, and that is exactly the substitution this task exists to
/// prevent.
#[test]
fn accept_t427_the_measured_field_and_corner_order_are_named() {
    assert_eq!(
        zone_box_field(),
        "unk140",
        "measured: unk140 is non-zero in all 80 numbered zones and unk116 and unk164 are zero \
         in all 80"
    );
    assert!(
        ZONE_PREFIX.starts_with(DETECTION_ZONE_PREFIX) && ZONE_PREFIX == DETECTION_ZONE_PREFIX,
        "the module's re-export is the same constant the content layer declares, not a second \
         spelling of it"
    );
}

// --------------------------------------------- the survey over a real file ---

/// One authored node in a fixture container, so a test can decide the two things
/// the survey refuses on: what kind of node a numbered zone is, and whether it
/// stores a transform.
///
/// [`SyntheticNode::object`] is the measured shape — every one of the 80
/// numbered zones in the owner's installation is an `object3d` record with
/// `Object3dCsC.flags == OBJECT3D_FLAGS_IDENTITY`. The other constructors exist
/// so those refusals are reachable in CI rather than only over `$CS_GAME_DIR`.
struct SyntheticNode<'a> {
    name: &'a str,
    corners: [[f32; 3]; 2],
    mesh_index: i32,
    kind: u32,
    object_flags: u32,
}

impl<'a> SyntheticNode<'a> {
    /// `NODE_TYPE_OBJECT3D`.
    const OBJECT3D: u32 = 5;
    /// `NODE_TYPE_CAMERA`: a kind whose record is **not** an object record, so
    /// its info record is not the one the survey's box field belongs to.
    const CAMERA: u32 = 1;
    /// `Object3dCsC.flags` for a record that stores no transform.
    const IDENTITY: u32 = 40;
    /// `Object3dCsC.flags` for a record that stores one.
    const TRANSFORMED: u32 = 32;
    /// `Object3dCsC` / `CameraC` record lengths, from the reader's constants.
    const OBJECT3D_BYTES: usize = 144;
    const CAMERA_BYTES: usize = 488;

    /// The measured shape: an `object3d` node that stores no transform.
    const fn object(name: &'a str, corners: [[f32; 3]; 2], mesh_index: i32) -> Self {
        Self {
            name,
            corners,
            mesh_index,
            kind: Self::OBJECT3D,
            object_flags: Self::IDENTITY,
        }
    }

    /// An `object3d` node that stores a transform, so its box is in the node's
    /// own space.
    const fn transformed(name: &'a str, corners: [[f32; 3]; 2], mesh_index: i32) -> Self {
        Self {
            name,
            corners,
            mesh_index,
            kind: Self::OBJECT3D,
            object_flags: Self::TRANSFORMED,
        }
    }

    /// A numbered zone that is not an object record at all.
    const fn camera(name: &'a str, corners: [[f32; 3]; 2], mesh_index: i32) -> Self {
        Self {
            name,
            corners,
            mesh_index,
            kind: Self::CAMERA,
            object_flags: Self::IDENTITY,
        }
    }

    /// How many bytes this node's own record occupies in the data section. Both
    /// records have no parent word, because the fixture writes
    /// `parent_count == 0` and only a LOD or light record reads one regardless.
    const fn data_bytes(&self) -> usize {
        match self.kind {
            Self::OBJECT3D => Self::OBJECT3D_BYTES,
            Self::CAMERA => Self::CAMERA_BYTES,
            _ => panic!("the fixture writes no record for this node kind"),
        }
    }
}

/// A synthetic CS GameZ container holding exactly the nodes `nodes` names.
///
/// Authored bytes: the header words, the 212-byte info slot and the object
/// records are written from the field offsets the production reader documents,
/// independently of the reader. The data section ends exactly at the
/// container's end, which is the check the reader makes and the reason a
/// mis-sized fixture fails rather than passing quietly.
///
/// This exists because the two choices the retail measurement rests on — **which
/// info field** and **which corner is the minimum** — are otherwise only
/// observable over `$CS_GAME_DIR`, and a CI run would not notice either of them
/// changing. Here they are observable over authored bytes.
fn synthetic_world_container(nodes: &[SyntheticNode<'_>]) -> Vec<u8> {
    /// The signature and version a CS GameZ archive stores.
    const SIGNATURE: u32 = 43_455_010;
    const VERSION: u32 = 42;
    /// Where the fixture's node array starts. Any value past the 40-byte header
    /// works; the reader reads the header's own word rather than assuming one.
    const NODES_OFFSET: u32 = 512;
    /// The info slot's stride: a 208-byte record plus the 4-byte index word.
    const SLOT: usize = 212;

    // The data section starts where the info array ends, so each record's
    // offset is known before any byte is written.
    let data_offset = NODES_OFFSET as usize + SLOT * nodes.len();
    let mut offsets = Vec::with_capacity(nodes.len());
    let mut cursor = data_offset;
    for node in nodes {
        offsets.push(cursor);
        cursor += node.data_bytes();
    }

    let mut bytes = vec![0_u8; cursor];
    let word = |bytes: &mut Vec<u8>, at: usize, value: u32| {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    };
    let half = |bytes: &mut Vec<u8>, at: usize, value: u16| {
        bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
    };
    let float = |bytes: &mut Vec<u8>, at: usize, value: f32| {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    };

    for (field, value) in [
        (0_usize, SIGNATURE),
        (4, VERSION),
        (8, 0x1234_5678),
        (12, 1),
        (16, 40),
        (20, 248),
        (24, 256),
        (28, nodes.len() as u32),
        (32, 0),
        (36, NODES_OFFSET),
    ] {
        word(&mut bytes, field, value);
    }

    for (index, node) in nodes.iter().enumerate() {
        let at = NODES_OFFSET as usize + SLOT * index;
        let name = node.name.as_bytes();
        assert!(
            name.len() < 36,
            "the fixture's names fit their 36-byte field"
        );
        bytes[at..at + name.len()].copy_from_slice(name);
        // The reference's asserted profile: bits 19 and 24 set, `unk044` 1,
        // `ZONE_DEFAULT`, the kind tag, the record's own offset, the mesh slot,
        // `action_priority` 1 and `unk196` 160 for an object record.
        word(&mut bytes, at + 36, 0x0180_001c);
        word(&mut bytes, at + 44, 1);
        word(&mut bytes, at + 48, 255);
        word(&mut bytes, at + 52, node.kind);
        word(&mut bytes, at + 56, offsets[index] as u32);
        word(&mut bytes, at + 60, node.mesh_index as u32);
        word(&mut bytes, at + 68, 1);
        word(&mut bytes, at + 196, 160);
        word(&mut bytes, at + 208, 0x0200_0000 | index as u32);
        half(&mut bytes, at + 84, 0);
        half(&mut bytes, at + 86, 0);
        // All three candidate boxes: only `unk140` is written, which is the
        // discrimination the retail measurement rests on.
        for (axis, value) in node.corners[0].iter().enumerate() {
            float(&mut bytes, at + 140 + 4 * axis, *value);
        }
        for (axis, value) in node.corners[1].iter().enumerate() {
            float(&mut bytes, at + 140 + 12 + 4 * axis, *value);
        }
        // The object record. `OBJECT3D_FLAGS_IDENTITY` with an identity
        // rotation, scale, matrix and translation, so the reader reports the
        // record as storing no transform **and** finds nothing to complain
        // about — which is what the measured corpus holds for every zone, and
        // what the survey's identity refusal reads.
        let data = offsets[index];
        if node.kind == SyntheticNode::OBJECT3D {
            word(&mut bytes, data, node.object_flags);
            for axis in 0..3 {
                float(&mut bytes, data + 36 + 4 * axis, 1.0);
            }
            for axis in 0..3 {
                float(&mut bytes, data + 48 + 12 * axis, 1.0);
                float(&mut bytes, data + 48 + 4 * axis + 4, 0.0);
                float(&mut bytes, data + 48 + 4 * axis + 8, 0.0);
            }
        }
    }
    bytes
}

/// A throwaway installation tree holding one synthetic world group.
struct TempInstallation {
    root: PathBuf,
}

impl TempInstallation {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the system clock is after the Unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "crimson-t427-{label}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("the fixture root is created");
        Self { root }
    }

    fn write(&self, spelling: &str, bytes: &[u8]) {
        let path = self.root.join(spelling);
        fs::create_dir_all(path.parent().expect("a fixture spelling has a parent"))
            .expect("the fixture directories are created");
        fs::write(&path, bytes).expect("the fixture bytes are written");
    }
}

impl Drop for TempInstallation {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// The survey, run over a **file** rather than over `$CS_GAME_DIR`.
///
/// This is the same production entry point the retail tests use — production
/// discovery, the production node reader, the same content-layer records — with
/// the corpus authored instead of read. It is what makes the two choices the
/// measurement rests on falsifiable in CI: `unk140` is the field carrying the
/// box, and the first stored corner is the minimum.
#[test]
fn accept_t427_the_survey_reads_the_box_out_of_the_field_and_order_it_measured() {
    let install = TempInstallation::new("field");
    install.write(
        "ZBD/C5/gamez.zbd",
        &synthetic_world_container(&[
            SyntheticNode::object("dzpaths", [[0.0, 0.0, 0.0], [0.0, 0.0, 0.0]], -1),
            SyntheticNode::object("dzpath1", [[-10.0, 4.0, -20.0], [-2.0, 9.5, -1.0]], 949),
            SyntheticNode::object("hangar", [[-500.0, 0.0, 0.0], [500.0, 60.0, 0.0]], 12),
            SyntheticNode::object("dzpath2", [[0.0, 0.0, 0.0], [40.0, 12.0, 90.0]], 950),
            SyntheticNode::object("dzpath1_backup", [[0.0, 0.0, 0.0], [9.0, 9.0, 9.0]], 7),
        ]),
    );

    let survey = survey_retail_trigger_volumes(&install.root)
        .unwrap_or_else(|error| panic!("the fixture installation surveys: {error}"));
    assert_eq!(
        survey.volumes().len(),
        2,
        "only the two numbered zones are reported: the parent, an ordinary node and a \
         non-numeric suffix are not zones"
    );
    let names: Vec<&str> = survey.volumes().iter().map(|zone| zone.zone()).collect();
    assert_eq!(
        names,
        ["dzpath1", "dzpath2"],
        "in the container's node order"
    );
    assert_eq!(
        survey
            .volumes_in(&WorldId::from_key("c5").expect("a valid world key"))
            .len(),
        2,
        "one world group, two zones"
    );

    // The corners, read out of `unk140` in `[min, max]` order: the first zone's
    // minimum is the negative triple and its extent is the difference. A reader
    // that pointed at `unk116` or `unk164` would read zeros and refuse; a reader
    // that swapped the corners would see an inverted box and be refused by name.
    let first = &survey.volumes()[0];
    assert_eq!(
        first.volume().min(),
        [-10.0, 4.0, -20.0],
        "the stored minimum, read from the measured field in the measured corner order"
    );
    assert_eq!(first.volume().max(), [-2.0, 9.5, -1.0]);
    assert_eq!(first.thinnest_stored_extent(), 5.5, "x: 8, y: 5.5, z: 19");
    assert_eq!(
        first.volume().thinnest_axis(),
        1,
        "the y axis is the thinnest"
    );
    assert_eq!(
        first.mesh_index(),
        Some(949),
        "the node's mesh binding is carried"
    );
    assert_eq!(first.node_slot(), 1, "the node array slot, in stored order");
    assert_eq!(first.node_bytes(), 212, "the info slot's own stride");
    assert_eq!(
        first.node_offset(),
        512 + 212,
        "the node's own byte offset, computed from the reader's own arithmetic"
    );
    assert_eq!(first.container(), "zbd/c5/gamez.zbd", "the logical key");
    assert_eq!(first.container_sha256().len(), 64, "the container's digest");
    assert_eq!(
        survey.install_sha256().len(),
        64,
        "the installation fingerprint"
    );
    // The survey's own factor, which is the whole of the second acceptance
    // criterion: this entry point measures **stored units** and supplies no
    // factor, whatever the corpus it reads. A reader that supplied one — even
    // 1.0, even a plausible-looking constant — would be inventing the original's
    // world-vertex unit, which is exactly what task #427 was filed to replace
    // with a measurement.
    assert_eq!(
        survey.vertex_scale_to_m(),
        None,
        "this entry point supplies no stored-unit-to-metre factor: nothing in the workspace has \
         measured the original's world-vertex unit (task #436)"
    );

    let second = &survey.volumes()[1];
    assert_eq!(
        second.volume().min(),
        [0.0, 0.0, 0.0],
        "a zone whose stored minimum is the origin reads as the origin"
    );
    assert_eq!(
        second.thinnest_stored_extent(),
        12.0,
        "y is its thinnest axis"
    );

    // The parent node carries no box, so a survey that reported it would have a
    // row whose every extent is zero.
    assert!(
        survey
            .volumes()
            .iter()
            .all(|zone| !zone.volume().is_empty()),
        "no reported zone is the all-zero parent: {names:?}"
    );
}

/// The survey's own refusals, over files rather than in-memory values: an
/// installation with no world group, a world group whose container is missing
/// from the manifest, a container that is not a node array at all, and a zone
/// whose stored corners are inverted.
///
/// Each is a **named** refusal rather than an empty survey, because an empty
/// survey is indistinguishable from an installation that happens to hold no
/// zones — and the difference is the difference between "measured nothing" and
/// "could not measure".
#[test]
fn accept_t427_every_survey_refusal_names_the_container_it_could_not_measure() {
    // No world group at all.
    let bare = TempInstallation::new("bare");
    bare.write("ZBD/planes.zbd", b"authored fixture");
    assert!(
        matches!(
            survey_retail_trigger_volumes(&bare.root),
            Err(TriggerVolumeSurveyError::NoWorldGroups)
        ),
        "an installation with no world container has nothing a zone could live in"
    );

    // A world group whose geometry container is not a node array.
    let garbage = TempInstallation::new("garbage");
    garbage.write("ZBD/C1/gamez.zbd", &[0_u8; 600]);
    match survey_retail_trigger_volumes(&garbage.root) {
        Err(TriggerVolumeSurveyError::Nodes { container, .. }) => {
            assert_eq!(
                container, "zbd/c1/gamez.zbd",
                "the refusal names the container it could not decode"
            );
        }
        other => panic!("a container that is not a node array must be refused, got {other:?}"),
    }

    // A numbered zone whose stored corners are inverted: `unk140` swapped.
    let inverted = TempInstallation::new("inverted");
    inverted.write(
        "ZBD/C2/gamez.zbd",
        &synthetic_world_container(&[SyntheticNode::object(
            "dzpath1",
            [[10.0, 0.0, 0.0], [2.0, 5.0, 5.0]],
            949,
        )]),
    );
    match survey_retail_trigger_volumes(&inverted.root) {
        Err(TriggerVolumeSurveyError::Volume {
            world,
            zone,
            reason,
        }) => {
            assert_eq!(world, "c2", "the world it was measuring");
            assert_eq!(zone, "dzpath1", "the zone it was measuring");
            assert_eq!(
                reason,
                TriggerVolumeError::Inverted { axis: 0 },
                "an inverted box is refused as exactly that, rather than silently swapped"
            );
        }
        other => panic!("an inverted zone box must be refused, got {other:?}"),
    }

    // A numbered zone that stores an all-zero box: a reportable gap, so it is
    // refused by name rather than dropped, and a consumer can see that the
    // survey met one instead of inferring it from a smaller count.
    let empty = TempInstallation::new("empty");
    empty.write(
        "ZBD/C3/gamez.zbd",
        &synthetic_world_container(&[SyntheticNode::object("dzpath1", [[0.0; 3], [0.0; 3]], 949)]),
    );
    match survey_retail_trigger_volumes(&empty.root) {
        Err(TriggerVolumeSurveyError::NoBox { world, zone }) => {
            assert_eq!(world, "c3", "the world it was measuring");
            assert_eq!(zone, "dzpath1", "the zone that stored no box");
        }
        other => panic!("a zone with no box must be refused, got {other:?}"),
    }

    // A box that is **flat on one axis** is not an absent box: it is a plane,
    // and a plane is a real authored volume. A reader that conflated "one axis
    // has zero extent" with "no box" would refuse it.
    let plane = TempInstallation::new("plane");
    plane.write(
        "ZBD/C4/gamez.zbd",
        &synthetic_world_container(&[SyntheticNode::object(
            "dzpath1",
            [[0.0, 500.0, 0.0], [40.0, 500.0, 0.0]],
            949,
        )]),
    );
    let flat = survey_retail_trigger_volumes(&plane.root)
        .unwrap_or_else(|error| panic!("a flat zone box is a volume, not an absence: {error}"));
    assert_eq!(flat.volumes().len(), 1, "the plane is reported");
    assert_eq!(
        flat.volumes()[0].thinnest_stored_extent(),
        0.0,
        "and its thinnest axis measures zero"
    );
    assert_eq!(
        flat.volumes()[0].volume().thinnest_axis(),
        1,
        "the y axis, which is the degenerate one"
    );

    // A numbered zone that is **not an object record**. Only an object record
    // stores a transform, so only its info record is the one this stage's box
    // field belongs to; a zone of any other kind is refused by name. A reader
    // that skipped it instead would report a shorter list as if it were the
    // measurement, which is the one outcome this survey exists to avoid.
    let camera = TempInstallation::new("camera");
    camera.write(
        "ZBD/C5/gamez.zbd",
        &synthetic_world_container(&[
            SyntheticNode::object("dzpath1", [[0.0; 3], [64.0, 64.0, 64.0]], 949),
            SyntheticNode::camera("dzpath2", [[0.0; 3], [64.0, 64.0, 64.0]], 950),
        ]),
    );
    match survey_retail_trigger_volumes(&camera.root) {
        Err(TriggerVolumeSurveyError::UnexpectedKind { world, zone, kind }) => {
            assert_eq!(world, "c5", "the world it was measuring");
            assert_eq!(zone, "dzpath2", "the zone it could not read");
            assert_eq!(kind, "camera", "and the node kind it found instead");
        }
        other => panic!("a numbered zone of another kind must be refused, got {other:?}"),
    }

    // A numbered zone that stores a **transform**. The measured corpus holds
    // none — every one of the 80 zones is an identity record — so this is the
    // invariant being enforced rather than a case the retail corpus trips: a
    // transformed node's box is in the node's own space, and reporting its
    // extents per axis would put the thinnest one on the wrong axis. The one-tick
    // verdict turns on exactly that axis, so the zone is refused rather than
    // measured wrongly.
    let rotated = TempInstallation::new("rotated");
    rotated.write(
        "ZBD/C5/gamez.zbd",
        &synthetic_world_container(&[
            SyntheticNode::object("dzpath1", [[0.0; 3], [64.0, 64.0, 64.0]], 949),
            SyntheticNode::transformed("dzpath2", [[0.0; 3], [8.0, 64.0, 64.0]], 950),
        ]),
    );
    match survey_retail_trigger_volumes(&rotated.root) {
        Err(TriggerVolumeSurveyError::TransformedZone { world, zone, flags }) => {
            assert_eq!(world, "c5", "the world it was measuring");
            assert_eq!(zone, "dzpath2", "the zone whose box is in its own space");
            assert_eq!(flags, 32, "`Object3dCsC.flags` as it stored it");
        }
        other => panic!("a zone that stores a transform must be refused, got {other:?}"),
    }
}

// ------------------------------------------------------------------ retail ---

/// The retail root, or a loud failure.
///
/// CI has no `CS_GAME_DIR` and the tests that call this are `#[ignore]`d, so the
/// expectation is that the variable is set when they run. A test that skipped
/// itself here would report a pass it never earned.
fn retail_root() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR is set for a retail test"))
}

/// The measured corpus, through the production discovery pass and the
/// production node reader.
///
/// Measured over the owner's installation: **80** numbered detection zones
/// across **six** world containers (`c1`, `c1b`, `c2`, `c3`, `c4`, `c5`;
/// `c1c` and `c2b` carry none), each storing a real axis-aligned box and binding
/// a mesh index. The thinnest stored extent over all 80 is exactly **32.0**
/// units and the thickest is about **860**. Every zone's node slot, byte offset,
/// container key, container digest and installation fingerprint is carried, so
/// each row is checkable against the bytes.
///
/// This test passing is also the assertion that all 80 are `object3d` records
/// that store **no transform**: the survey refuses a zone of either kind by
/// name, so a corpus that held one would error instead of measuring.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t427_retail_every_detection_zone_extent_is_measured_with_its_span() {
    let survey = survey_retail_trigger_volumes(&retail_root()).expect("the retail corpus surveys");
    assert_eq!(
        survey.install_sha256().len(),
        64,
        "the installation fingerprint the measurement was taken over"
    );
    assert_eq!(
        survey.volumes().len(),
        80,
        "80 numbered detection zones across the six world containers that carry any"
    );
    assert_eq!(
        survey.meshless_zones().len(),
        0,
        "every measured zone binds a mesh index: a zone with no geometry would be a marker, \
         and the corpus has none"
    );

    let mut worlds = BTreeSet::new();
    let mut thinnest = f64::INFINITY;
    let mut thickest: f64 = 0.0;
    for volume in survey.volumes() {
        worlds.insert(volume.world().key().to_owned());
        assert_eq!(volume.node_bytes(), 212, "the node's own slot");
        assert!(
            volume.node_offset() > 0,
            "a node offset the reader's own arithmetic produced"
        );
        assert_eq!(
            volume.container_sha256().len(),
            64,
            "a SHA-256 in hex: {}",
            volume.container()
        );
        let stored = volume.volume();
        assert!(
            (0..3).all(|axis| stored.extent(axis) > 0.0),
            "every measured zone stores a non-degenerate box: {}",
            volume.zone()
        );
        assert_eq!(
            stored.thinnest_extent(),
            volume.thinnest_stored_extent(),
            "the thinnest axis is the one the comparison turns on"
        );
        thinnest = thinnest.min(volume.thinnest_stored_extent());
        thickest = thickest.max(stored.extent(2).max(stored.extent(0)));
    }
    assert_eq!(
        worlds.len(),
        6,
        "six world containers carry numbered zones; c1c and c2b carry none: {worlds:?}"
    );
    assert!(
        (thinnest - 32.0).abs() < 1.0e-9,
        "the thinnest zone measures exactly 32 stored units, measured: {thinnest}"
    );
    assert!(
        thickest > 800.0,
        "the thickest zone is over 800 stored units, so the corpus spans more than an order of \
         magnitude: {thickest}"
    );
}

/// The verdict task #427 asked for, over the measured corpus, stated the way
/// the evidence supports it.
///
/// The unit is unmeasured, so the survey refuses to answer and reports the
/// factor at which it would change. That factor is the second acceptance
/// criterion's answer: for the verdict to be "an original trigger is outrun by a
/// tick", one stored unit would have to be worth more than
/// `3.3333 m / 32 units ≈ 0.104 m` — roughly ten centimetres. A world whose
/// trigger boxes are a tenth of a metre across in their own coordinate system
/// would be a world whose *aircraft* are a tenth of a metre long, so the verdict
/// is one the measurement effectively rules out, without this task having to
/// guess the unit to say so.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t427_retail_the_thin_original_trigger_needs_a_tenth_of_a_metre_per_unit_to_be_outrun() {
    let survey = survey_retail_trigger_volumes(&retail_root()).expect("the retail corpus surveys");
    assert_eq!(
        survey.vertex_scale_to_m(),
        None,
        "this survey supplies no factor, so the verdict cannot be a guess"
    );

    for speed in [FAST_M_S, ORDINARY_M_S, 30.0] {
        let verdict = survey
            .tick_verdict(speed, TICK_HZ)
            .expect("a finite speed and a non-zero rate");
        assert!(
            !verdict.is_decided(),
            "the verdict is undecided at {speed} m/s, and says so"
        );
        let break_even = verdict
            .break_even_meters_per_unit()
            .expect("a measured zone gives a break-even factor");
        let travel = verdict
            .travel_m_per_tick()
            .expect("one tick of travel is computable");
        assert!(
            break_even < 0.105,
            "at {speed} m/s and {TICK_HZ} Hz the verdict only flips if one stored unit is worth \
             more than {break_even:.6} m; the corpus's thinnest zone is 32 stored units, so the \
             flip needs a stored unit worth ~10 cm"
        );
        assert!(
            travel > 0.0,
            "the tick travel is a real quantity whatever the verdict is"
        );
    }
    // The break-even falls with the tick: it is one tick's travel over the same
    // stored extent, so a coarser tick or a slower body only raises it.
    let fast = survey
        .tick_verdict(FAST_M_S, TICK_HZ)
        .expect("a valid tick")
        .break_even_meters_per_unit()
        .expect("a measured zone");
    let slow = survey
        .tick_verdict(30.0, TICK_HZ)
        .expect("a valid tick")
        .break_even_meters_per_unit()
        .expect("a measured zone");
    assert!(
        slow < fast,
        "a slower body needs a larger factor to be outrun"
    );
    let coarse = survey
        .tick_verdict(FAST_M_S, 30.0)
        .expect("a valid tick")
        .break_even_meters_per_unit()
        .expect("a measured zone");
    assert!(
        coarse > fast,
        "a coarser tick needs a larger factor to be outrun"
    );
}

// ------------------------------------------------ the mission side (task #513) ---

fn declaration(
    mission: &str,
    world: &str,
    disable: &[&str],
    objectives: &[(&str, u32)],
) -> MissionZoneDeclaration {
    let mut keys = Vec::new();
    if !disable.is_empty() {
        keys.push(ZoneDeclarationKey::Disable);
    }
    if !objectives.is_empty() {
        keys.push(ZoneDeclarationKey::ObjectiveNumbers);
    }
    MissionZoneDeclaration::new(
        mission,
        WorldId::from_key(world).expect("a valid world key"),
        format!("{mission}/zrdr.zbd"),
        "b".repeat(64),
        (100, 155),
        keys,
        disable.iter().map(|zone| (*zone).to_owned()).collect(),
        Vec::new(),
        objectives
            .iter()
            .map(|(zone, number)| ((*zone).to_owned(), *number))
            .collect(),
    )
}

/// A mission naming a zone its world container has no node for is a **reported
/// gap**, never a silent drop; a zone another world has does not count.
#[test]
fn accept_t427_dzones_a_declared_zone_no_container_has_is_a_reported_gap() {
    let survey = synthetic_survey(None)
        .with_declarations(vec![
            // c5 has dzpath1 and dzpath2; dzpath3 exists only in c3.
            declaration(
                "zbd/c5/m01",
                "c5",
                &["dzpath1", "dzpath3"],
                &[("dzpath2", 18)],
            ),
            declaration("zbd/c3/m01", "c3", &[], &[("dzpath3", 20), ("dzpath9", 21)]),
        ])
        .expect("two distinct missions attach");
    assert!(survey.zone_declarations_are_decoded());
    assert_eq!(survey.declarations().len(), 2);
    let gaps = survey.declaration_gaps();
    let found: Vec<(&str, &str, ZoneDeclarationKey)> = gaps
        .iter()
        .map(|gap| (gap.mission.as_str(), gap.zone.as_str(), gap.key))
        .collect();
    assert_eq!(
        found,
        vec![
            ("zbd/c5/m01", "dzpath3", ZoneDeclarationKey::Disable),
            (
                "zbd/c3/m01",
                "dzpath9",
                ZoneDeclarationKey::ObjectiveNumbers
            ),
        ]
    );

    let none = synthetic_survey(None)
        .with_declarations(vec![declaration("zbd/c5/m01", "c5", &["dzpath1"], &[])])
        .expect("attaches");
    assert!(none.declaration_gaps().is_empty());

    assert!(matches!(
        synthetic_survey(None).with_declarations(vec![
            declaration("zbd/c5/m01", "c5", &["dzpath1"], &[]),
            declaration("zbd/c5/m01", "c5", &["dzpath2"], &[]),
        ]),
        Err(TriggerVolumeError::DuplicateMission { .. })
    ));
}

/// The corpus: every one of the 23 members decodes through the production
/// discovery and reader, each is cross-checked against the world containers, and
/// the gap list is stated rather than assumed.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t427_dzones_retail_every_campaign_member_decodes_and_is_cross_checked() {
    let survey = survey_retail_trigger_volumes(&retail_root()).expect("the retail corpus surveys");
    assert!(survey.zone_declarations_are_decoded());
    let declarations = survey.declarations();
    assert_eq!(
        declarations.len(),
        23,
        "23 campaign readers carry the member"
    );
    let bytes: u64 = declarations.iter().map(|d| d.member_span().1).sum();
    assert_eq!(bytes, 9_197, "the members total the measured byte count");
    let smallest = declarations.iter().map(|d| d.member_span().1).min();
    let largest = declarations.iter().map(|d| d.member_span().1).max();
    assert_eq!((smallest, largest), (Some(155), Some(826)));

    let count = |key: ZoneDeclarationKey| {
        declarations
            .iter()
            .filter(|d| d.keys().contains(&key))
            .count()
    };
    assert_eq!(count(ZoneDeclarationKey::Disable), 16);
    assert_eq!(count(ZoneDeclarationKey::NoSnapshot), 12);
    assert_eq!(count(ZoneDeclarationKey::ObjectiveNumbers), 20);

    for declaration in declarations {
        assert_eq!(declaration.member_container_sha256().len(), 64);
        assert!(declaration.member_container().ends_with("zrdr.zbd"));
        assert!(
            declaration
                .mission()
                .starts_with(&format!("zbd/{}/", declaration.world().key()))
        );
        let numbers: BTreeSet<u32> = declaration
            .objective_numbers()
            .iter()
            .map(|(_, number)| *number)
            .collect();
        assert_eq!(
            numbers.len(),
            declaration.objective_numbers().len(),
            "{}: an objective number is bound to one zone",
            declaration.mission()
        );
        for (_, number) in declaration.objective_numbers() {
            assert!(
                (18..=31).contains(number),
                "{}: {number}",
                declaration.mission()
            );
        }
    }

    // The cross-check: the measured corpus names no zone a container lacks. This
    // is the *measured* answer; the gap list is the mechanism that would report
    // one, and the synthetic test above proves it does.
    assert_eq!(
        survey.declaration_gaps(),
        Vec::new(),
        "every declared zone has a node in its mission's world container"
    );
}

/// A version-one reader archive: member data, then one 148-byte index entry per
/// member (u32 start, u32 length, a 64-byte NUL-padded name, 76 bytes), then the
/// u32 version `1` and the u32 member count. Authored for this file.
fn reader_archive(members: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut entries = Vec::new();
    for (name, member) in members {
        entries.push((bytes.len() as u32, member.len() as u32, *name));
        bytes.extend_from_slice(member);
    }
    for (start, length, name) in &entries {
        bytes.extend_from_slice(&start.to_le_bytes());
        bytes.extend_from_slice(&length.to_le_bytes());
        let mut field = [0_u8; 64];
        field[..name.len()].copy_from_slice(name.as_bytes());
        bytes.extend_from_slice(&field);
        bytes.extend_from_slice(&[0_u8; 76]);
    }
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&(members.len() as u32).to_le_bytes());
    bytes
}

fn zrd_text(value: &str) -> Vec<u8> {
    let mut out = 3_u32.to_le_bytes().to_vec();
    out.extend((value.len() as u32).to_le_bytes());
    out.extend(value.as_bytes());
    out
}

fn zrd_list(children: Vec<Vec<u8>>) -> Vec<u8> {
    let mut out = 4_u32.to_le_bytes().to_vec();
    out.extend((children.len() as u32 + 1).to_le_bytes());
    children.into_iter().for_each(|child| out.extend(child));
    out
}

fn mission_world_install(label: &str, member: &[u8]) -> TempInstallation {
    let install = TempInstallation::new(label);
    install.write(
        "ZBD/C5/gamez.zbd",
        &synthetic_world_container(&[
            SyntheticNode::object("dzpath1", [[-10.0, 4.0, -20.0], [-2.0, 9.5, -1.0]], 949),
            SyntheticNode::object("dzpath2", [[0.0, 0.0, 0.0], [40.0, 12.0, 90.0]], 950),
        ]),
    );
    install.write(
        "ZBD/C5/M01/zrdr.zbd",
        &reader_archive(&[("dzones.zrd", member.to_vec())]),
    );
    install
}

/// The survey, over a **file**: production discovery finds the mission's reader
/// archive, the production reader decodes its member, the content layer joins it
/// to the world's zone nodes, and a zone the container lacks is a reported gap.
/// This is the CI-visible half of the retail test, so a survey that decoded
/// nothing fails here too.
#[test]
fn accept_t427_dzones_the_survey_joins_a_mission_member_to_its_world_container() {
    let member = zrd_list(vec![
        zrd_text("disable"),
        zrd_list(vec![zrd_text("dzpath2"), zrd_text("dzpath7")]),
    ]);
    let install = mission_world_install("dzones", &member);
    let survey = survey_retail_trigger_volumes(&install.root)
        .unwrap_or_else(|error| panic!("the fixture installation surveys: {error}"));
    assert!(survey.zone_declarations_are_decoded());
    let [declaration] = survey.declarations() else {
        panic!("one mission declares zones: {:?}", survey.declarations());
    };
    assert_eq!(declaration.mission(), "zbd/c5/m01");
    assert_eq!(declaration.world().key(), "c5");
    assert_eq!(declaration.disable(), ["dzpath2", "dzpath7"]);
    assert_eq!(declaration.keys(), [ZoneDeclarationKey::Disable]);
    assert!(declaration.member_container().ends_with("zrdr.zbd"));
    assert_eq!(declaration.member_container_sha256().len(), 64);
    assert_eq!(declaration.member_span().1, member.len() as u64);
    let gaps = survey.declaration_gaps();
    assert_eq!(gaps.len(), 1, "{gaps:?}");
    assert_eq!(gaps[0].zone, "dzpath7");
}

/// A member that does not decode aborts the survey by name rather than being
/// dropped.
#[test]
fn accept_t427_dzones_an_undecodable_member_refuses_the_survey_by_name() {
    let mut member = zrd_list(vec![zrd_text("disable"), zrd_list(vec![])]);
    member.extend([0, 0]);
    let install = mission_world_install("dzones-bad", &member);
    assert!(matches!(
        survey_retail_trigger_volumes(&install.root),
        Err(TriggerVolumeSurveyError::Declarations { .. })
    ));
}
