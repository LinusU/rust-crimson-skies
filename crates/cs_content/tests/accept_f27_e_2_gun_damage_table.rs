//! Acceptance tests for task F27-E.2 (#549): the original's gun damage table is
//! shipped in `weapons.zrd`, one record per (gun, ammunition kind) the engine's
//! own map selects, and is read through `cs_content::weapons` with an observed
//! provenance naming the bytes.
//!
//! The fast tests drive the production reader over a synthetic `.zrd` member
//! built in the test; the `#[ignore]`d ones re-measure the retail installation.

use cs_assets::install::sha256;
use cs_content::original_airframe::SpannedDocument;
use cs_content::weapons::{
    LimitEvidence, LimitOutcome, ORIGINAL_DAMAGE_AMMO_KINDS, ORIGINAL_GUN_COUNT,
    ORIGINAL_GUN_RECORD_BASES, OriginalGunDamageTable, OriginalLimitClaim, OriginalLimitReport,
    is_observed_evidence, original_gun_damage_table_from_document, read_original_gun_damage_table,
};
use cs_types::evidence::ClaimStatus;

enum N {
    Int(u32),
    Float(f32),
    Text(&'static str),
    Owned(String),
    List(Vec<N>),
}

fn encode(node: &N, out: &mut Vec<u8>) {
    let word = |out: &mut Vec<u8>, value: u32| out.extend_from_slice(&value.to_le_bytes());
    let text = |out: &mut Vec<u8>, text: &str| {
        word(out, 3);
        word(out, u32::try_from(text.len()).expect("short"));
        out.extend_from_slice(text.as_bytes());
    };
    match node {
        N::Int(value) => {
            word(out, 1);
            word(out, *value);
        }
        N::Float(value) => {
            word(out, 2);
            word(out, value.to_bits());
        }
        N::Text(value) => text(out, value),
        N::Owned(value) => text(out, value),
        N::List(children) => {
            word(out, 4);
            word(out, u32::try_from(children.len() + 1).expect("short"));
            for child in children {
                encode(child, out);
            }
        }
    }
}

/// One `wep_NN` record with the two amounts; `None` leaves the key out.
fn record(index: u32, armor: Option<f32>, health: Option<f32>) -> [N; 2] {
    let mut body = vec![N::Text("CALIBER"), N::List(vec![N::Int(50)])];
    if let Some(armor) = armor {
        body.push(N::Text("ARMOR_DAMAGE"));
        body.push(N::List(vec![N::Float(armor)]));
    }
    if let Some(health) = health {
        body.push(N::Text("HEALTH_DAMAGE"));
        body.push(N::List(vec![N::Float(health)]));
    }
    [N::Owned(format!("wep_{index:02}")), N::List(body)]
}

/// Armor and health the synthetic member gives (gun, ammo): distinct per cell.
fn amounts(gun: u32, ammo: u32) -> (f32, f32) {
    (
        (gun * 10 + ammo) as f32 + 0.25,
        (gun * 10 + ammo) as f32 + 0.75,
    )
}

fn member(skip: Option<u32>, broken: Option<u32>) -> Vec<u8> {
    let mut root = vec![N::Text("VERSION"), N::List(vec![N::Int(1)])];
    for (gun, base) in ORIGINAL_GUN_RECORD_BASES.iter().enumerate() {
        for ammo in 0..ORIGINAL_DAMAGE_AMMO_KINDS {
            let index = base + ammo;
            if skip == Some(index) {
                continue;
            }
            let (armor, health) = amounts(u32::try_from(gun).unwrap(), ammo);
            let health = if broken == Some(index) {
                None
            } else {
                Some(health)
            };
            root.extend(record(index, Some(armor), health));
        }
    }
    let mut out = Vec::new();
    encode(&N::List(root), &mut out);
    out
}

fn table_of(bytes: &[u8]) -> OriginalGunDamageTable {
    let document =
        SpannedDocument::decode(sha256(b"install"), "weapons.zrd", bytes, 100, sha256(bytes))
            .expect("the synthetic member decodes");
    original_gun_damage_table_from_document(&document).expect("the table reads")
}

#[test]
fn accept_f27_e_2_every_gun_and_ammunition_cell_is_read_from_the_record_the_map_names() {
    let table = table_of(&member(None, None));
    assert_eq!(
        table.cells().len(),
        ORIGINAL_GUN_COUNT * ORIGINAL_DAMAGE_AMMO_KINDS as usize
    );
    assert!(table.unmeasured().is_empty());
    for gun in 0..5u32 {
        for ammo in 0..4u32 {
            let cell = table.cell(gun, ammo).expect("every cell is read");
            let index = ORIGINAL_GUN_RECORD_BASES[gun as usize] + ammo;
            assert_eq!(cell.record(), format!("wep_{index}"));
            let (armor, health) = amounts(gun, ammo);
            assert_eq!(
                cell.damage().armor.clone().known(),
                Some(f64::from(armor)),
                "wep_{index} armor"
            );
            assert_eq!(
                cell.damage().internal.clone().known(),
                Some(f64::from(health)),
                "wep_{index} health"
            );
        }
    }
}

#[test]
fn accept_f27_e_2_each_amount_carries_an_observation_naming_its_own_bytes() {
    let bytes = member(None, None);
    let table = table_of(&bytes);
    for cell in table.cells() {
        for (channel, amount) in cell.damage().entries() {
            let cs_types::content::Resolved::Known(known) = amount else {
                panic!("{} {channel} is unknown", cell.record());
            };
            assert!(is_observed_evidence(known.provenance.class));
            assert_eq!(known.provenance.class, ClaimStatus::ObservedTool);
            let span = known.provenance.source.as_ref().expect("a source span");
            assert_eq!(span.container_path(), "ZBD/zrdr.zbd");
            assert_eq!(span.member_key(), Some("weapons.zrd"));
            // The span is absolute (member offset 100); the node is the float.
            let start = usize::try_from(span.offset() - 100).unwrap();
            let end = start + usize::try_from(span.length()).unwrap();
            let node = &bytes[start..end];
            assert_eq!(node.len(), 8, "tag word + f32");
            assert_eq!(&node[..4], 2u32.to_le_bytes());
            assert_eq!(
                f64::from(f32::from_le_bytes(node[4..8].try_into().unwrap())),
                known.value
            );
        }
    }
}

#[test]
fn accept_f27_e_2_a_missing_or_incomplete_record_is_unmeasured_not_defaulted() {
    let table = table_of(&member(Some(51), Some(62)));
    assert_eq!(table.cells().len(), 18);
    assert!(table.cell(2, 1).is_none(), "wep_51 is absent");
    assert!(table.cell(3, 2).is_none(), "wep_62 lacks HEALTH_DAMAGE");
    assert_eq!(table.unmeasured().len(), 2);
    assert!(table.unmeasured()[0].starts_with("wep_51"));
    assert!(table.unmeasured()[1].starts_with("wep_62"));
    assert!(matches!(
        table.limit_evidence(),
        LimitEvidence::PartlyMeasured { unmeasured: 2, .. }
    ));
}

#[test]
fn accept_f27_e_2_a_complete_table_resolves_the_damage_claim_and_nothing_else() {
    let table = table_of(&member(None, None));
    let mut report = OriginalLimitReport::new();
    report
        .record(OriginalLimitClaim::AmmoNamesDamage, table.limit_evidence())
        .expect("an observation resolves the claim");
    assert_eq!(report.bound(), vec![OriginalLimitClaim::AmmoNamesDamage]);
    assert!(matches!(
        report.row(OriginalLimitClaim::AmmoNamesDamage).unwrap().outcome(),
        LimitOutcome::Bound { provenance } if provenance.class == ClaimStatus::ObservedTool
    ));
    assert!(report.row(OriginalLimitClaim::GunGroupAssignment).is_none());

    // A table with a hole is not a resolution.
    let partial = table_of(&member(Some(30), None));
    let mut report = OriginalLimitReport::new();
    report
        .record(
            OriginalLimitClaim::AmmoNamesDamage,
            partial.limit_evidence(),
        )
        .unwrap();
    assert!(report.bound().is_empty());
}

// ------------------------------------------------------------------ retail --

fn game_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must name the installation"),
    )
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f27_e_2_retail_weapons_zrd_states_all_twenty_gun_ammunition_cells() {
    let table = read_original_gun_damage_table(&game_dir()).expect("the table reads");
    assert_eq!(table.cells().len(), 20, "{:?}", table.unmeasured());
    assert!(table.unmeasured().is_empty());
    assert!(matches!(
        table.limit_evidence(),
        LimitEvidence::Measured { .. }
    ));

    // The engine's own record bases, re-read from the committed static facts.
    let facts = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/owner-static/engine-facts.toml"),
    )
    .expect("engine-facts.toml");
    for (gun, base) in ORIGINAL_GUN_RECORD_BASES.iter().enumerate() {
        let line = format!("gun_base_{gun} = {base}");
        assert!(facts.lines().any(|l| l == line), "{line} not in the facts");
    }

    // Pinned cells (record, armor, health) spanning every ammunition kind and
    // the smallest and largest guns.
    for (gun, ammo, armor, health) in [
        (0, 0, 3.0, 3.0),
        (0, 1, 1.5, 4.5),
        (0, 2, 4.5, 1.5),
        (0, 3, 3.5, 2.5),
        (2, 0, 6.25, 6.25),
        (2, 1, 3.125, 9.375),
        (4, 2, 17.625, 5.875),
        (4, 3, 12.25, 11.25),
    ] {
        let cell = table.cell(gun, ammo).expect("cell");
        assert_eq!(
            cell.damage()
                .known_amount(cs_content::weapons::DeclaredDamageChannel::Armor),
            Some(armor)
        );
        assert_eq!(
            cell.damage()
                .known_amount(cs_content::weapons::DeclaredDamageChannel::Internal),
            Some(health)
        );
    }

    assert_eq!(table.provenance().class, ClaimStatus::ObservedTool);
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f27_e_2_retail_the_damage_claim_binds_on_an_observation() {
    let table = read_original_gun_damage_table(&game_dir()).expect("the table reads");
    let mut report = OriginalLimitReport::new();
    report
        .record(OriginalLimitClaim::AmmoNamesDamage, table.limit_evidence())
        .expect("recorded");
    assert_eq!(report.bound(), vec![OriginalLimitClaim::AmmoNamesDamage]);
    for cell in table.cells() {
        for (_, amount) in cell.damage().entries() {
            let cs_types::content::Resolved::Known(known) = amount else {
                panic!("unknown amount in {}", cell.record());
            };
            assert!(known.value > 0.0);
            assert_eq!(known.provenance.class, ClaimStatus::ObservedTool);
        }
    }
}
