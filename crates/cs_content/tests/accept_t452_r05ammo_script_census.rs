//! Task #2173 (T452-R05-AMMO-SCRIPT): the `ammo` command census over the
//! installation's INTERP scripts.
//!
//! Task test prefix: `accept_t452_r05ammo_`. Finding:
//! `docs/findings/2026-10-28-t452-r05-ammo-command-census.md`. The synthetic
//! half drives the production resolver over newly authored container bytes and
//! runs in CI; the retail half fails loudly without `CS_GAME_DIR`.

use std::collections::BTreeSet;

use cs_content::ordnance_ammo::{
    AMMO_SLOT_COUNT, AmmoAction, AmmoLineIssue, ammo_record_name, census_ammo_commands,
    read_original_ammo_census, record_indices,
};
use cs_formats::ParseContext;
use cs_formats::interp::decode_interp;

/// A container of `scripts`, each a list of lines, each a list of arguments.
fn container(scripts: &[(&str, &[&[&str]])]) -> Vec<u8> {
    let mut bodies: Vec<Vec<u8>> = Vec::new();
    for (_, lines) in scripts {
        let mut body = Vec::new();
        for line in *lines {
            let mut data = Vec::new();
            for argument in *line {
                data.extend_from_slice(argument.as_bytes());
                data.push(0);
            }
            body.extend_from_slice(&u32::try_from(data.len()).expect("size").to_le_bytes());
            body.extend_from_slice(&u32::try_from(line.len()).expect("count").to_le_bytes());
            body.extend_from_slice(&data);
        }
        body.extend_from_slice(&0u32.to_le_bytes());
        bodies.push(body);
    }
    let mut out = Vec::new();
    out.extend_from_slice(&0x0897_1119u32.to_le_bytes());
    out.extend_from_slice(&7u32.to_le_bytes());
    out.extend_from_slice(&u32::try_from(scripts.len()).expect("count").to_le_bytes());
    let mut offset = 12 + 128 * scripts.len();
    for ((name, _), body) in scripts.iter().zip(&bodies) {
        let mut field = [0u8; 120];
        field[..name.len()].copy_from_slice(name.as_bytes());
        out.extend_from_slice(&field);
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&u32::try_from(offset).expect("offset").to_le_bytes());
        offset += body.len();
    }
    for body in &bodies {
        out.extend_from_slice(body);
    }
    out
}

fn known() -> BTreeSet<u32> {
    record_indices(["wep_05", "wep_15", "wep_27", "wep_140", "not_a_record"])
}

#[test]
fn accept_t452_r05ammo_synthetic_resolves_both_arities_and_reports_out_of_range() {
    let bytes = container(&[
        (
            "one.gw",
            &[
                &["NodeSetActive", "x"],
                &["ammo", "4", "27"],
                &["AMMO", "15"],
            ],
        ),
        (
            "two.gw",
            &[
                &["ammo", "12", "5"],
                &["ammo", "0", "99"],
                &["ammo", "1", "2", "3"],
                &["ammo", "x", "5"],
                &["ammo"],
            ],
        ),
    ]);
    let decoded =
        decode_interp(&mut ParseContext::with_defaults("synthetic"), &bytes).expect("decodes");
    let census = census_ammo_commands(&decoded, &known());
    assert_eq!((census.scripts, census.lines), (2, 8));
    assert_eq!(census.commands.len(), 7);
    assert_eq!(
        census.per_script().into_iter().collect::<Vec<_>>(),
        [("one.gw", 2), ("two.gw", 5)]
    );

    let c = &census.commands;
    assert_eq!(
        c[0].action,
        Some(AmmoAction::SetSlot { slot: 4, index: 27 })
    );
    assert!(c[0].issues.is_empty());
    assert_eq!(c[1].action, Some(AmmoAction::FillRockets { index: 15 }));
    assert!(c[1].issues.is_empty());
    assert_eq!(
        c[2].issues,
        [AmmoLineIssue::SlotOutOfRange {
            slot: AMMO_SLOT_COUNT
        }]
    );
    assert_eq!(c[3].issues, [AmmoLineIssue::IndexOutOfRange { index: 99 }]);
    assert_eq!(
        c[4].issues,
        [AmmoLineIssue::UnsupportedArity { arguments: 3 }]
    );
    assert_eq!(c[5].action, None);
    assert_eq!(
        c[5].issues,
        [AmmoLineIssue::UnreadableArgument { position: 0 }]
    );
    assert_eq!(
        c[6].issues,
        [AmmoLineIssue::UnsupportedArity { arguments: 0 }]
    );
    assert_eq!(census.reported().len(), 5);

    assert_eq!(census.installed_indices(), BTreeSet::from([5, 15, 27, 99]));
    assert_eq!(census.slots_of(27), BTreeSet::from([4]));
    assert_eq!(census.slots_of(15), (4..12).collect::<BTreeSet<_>>());
    assert_eq!(ammo_record_name(4), "wep_04");
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_r05ammo_retail_installs_no_record_through_any_script() {
    let root = std::path::PathBuf::from(std::env::var_os("CS_GAME_DIR").expect(
        "CS_GAME_DIR is not set: this measurement needs the original installation \
         (capability `retail`)",
    ));
    let census = read_original_ammo_census(&root).expect("the installation census reads");
    assert_eq!(census.scripts, 98, "the INTERP bodies of ZBD/interp.zbd");
    assert_eq!(census.lines, 5083);
    assert!(
        census.commands.is_empty(),
        "no retail INTERP script names the `ammo` command: {:?}",
        census.commands
    );
    assert!(census.installed_indices().is_empty());
    for index in [4, 24, 25, 26, 27] {
        assert!(
            census.slots_of(index).is_empty(),
            "{}",
            ammo_record_name(index)
        );
    }
}
