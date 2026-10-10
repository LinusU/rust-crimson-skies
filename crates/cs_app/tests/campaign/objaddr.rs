//! `M06-B-FU3` acceptance (Rally #819): the cross-objective address
//! convention, pinned once across the reconciled suites.
//!
//! Four campaign suites read the same spelled integers out of their control
//! records, and two readings of them were merged side by side: M02-B's and
//! M04-B's graph walks treated a spelled integer as a **zero-based record
//! index** while M03-B's and M06-B's read it as the **one-based block
//! number**. M02-B-FU3 (#802) settled the question by measuring the
//! original's parse in the decrypted executable: the store loops at
//! `0x468c40` (wake array), `0x468cf0` (nap target) and `0x4679fc`
//! (dependency gate) decrement every objective address before storing it,
//! while `DEDG`'s integers and a nap's seconds are stored without the
//! decrement — so the document spells **block numbers** and the record stores
//! indices, `a − 1`. The engine-side rule is
//! `cs_sim::objectives::address::resolve_objective_address`
//! (`[1, objectives]`, refused by name outside), and the convention is
//! recorded in `docs/findings/2026-10-09-m02-b-fu3-out-of-range-wake-address.md`
//! and reconciled in `docs/findings/2026-10-10-m06-b-fu3-objective-address-convention.md`.
//!
//! The suite pins the convention itself rather than any one mission: the
//! synthetic test holds the parse conversion's boundaries (a spelled value
//! equal to the block count is in range — the reading a zero-based
//! interpretation cannot explain), and the retail test re-walks all four
//! reconciled records (`zbd/c1/m02`, `zbd/c1b/m03`, `zbd/c1/m04`,
//! `zbd/c2/m01`) and pins the discriminating fact on each: every one of them
//! spells an address equal to its own block count.
//!
//! The retail test is `#[ignore = "requires CS_GAME_DIR"]`; the synthetic
//! test runs in CI.

use std::path::PathBuf;

use cs_app::mission_control::read_control_member;
use cs_content::objectives::objective_block_number;
use cs_content::stunts::{ZrdValue, objective_record, zrd_flat_fields};
use cs_script::ir::SymbolId;
use cs_sim::objectives::address::{
    AddressRefusal, OUT_OF_RANGE_OBJECTIVE_ADDRESS, address_of, resolve_objective_address,
};

/// The original installation, as the environment declares it.
pub(crate) fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: M06-B-FU3 needs the retail capability; run this suite with \
             `--include-ignored` and CS_GAME_DIR pointing at the read-only installation"
        )
    }))
}

/// The eight keys that spell a cross-objective address — the set the
/// original's parser feeds through `dec` before storing it (measured in
/// `docs/findings/2026-10-09-m02-b-fu3-out-of-range-wake-address.md`).
pub(crate) const ADDRESS_KEYS: [&str; 8] = [
    "WAKE_OBJECTIVE",
    "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
    "KILL_OBJECTIVE_WHEN_I_COMPLETE",
    "NAP_OBJECTIVE_WHEN_I_COMPLETE",
    "SLEEP_OBJECTIVE_WHEN_I_COMPLETE",
    "WAKE_OBJECTIVE_WHEN_I_SLEEP",
    "TICK_DEPENDS_ON_OBJ",
    "HIDE_OBJ",
];

/// The addresses one site spells under the measured child rules: a nap's
/// child0 is the target block while child1 is the re-wake delay in seconds —
/// never an address — and a wake/kill/sleep list contributes every integer it
/// spells.
pub(crate) fn spelled_addresses(key: &str, args: &[ZrdValue]) -> Vec<u32> {
    let ints = || args.iter().filter_map(ZrdValue::as_int);
    if key == "NAP_OBJECTIVE_WHEN_I_COMPLETE" {
        ints().take(1).collect()
    } else {
        ints().collect()
    }
}

/// One address-carrying site of a numbered block: the authored block number,
/// the directive key and the block numbers it spells, in document order.
pub(crate) struct Site {
    pub(crate) block: u32,
    pub(crate) key: String,
    pub(crate) addresses: Vec<u32>,
}

/// Walks a decoded control record into its numbered blocks' address sites,
/// reading the grammar the census measured: a text key, then its argument —
/// a list, a bare end (a text follower or the block's own end), or the
/// `not_a_list` scalar, which the site keeps as its one-element argument so
/// the walk accounts for it rather than silently dropping the rest of the
/// block.
pub(crate) fn walk(document: &ZrdValue) -> Vec<Site> {
    let mut sites = Vec::new();
    for (key, value) in zrd_flat_fields(objective_record(document)) {
        let Some(number) = objective_block_number(key) else {
            continue;
        };
        let Some(children) = value.as_list() else {
            continue;
        };
        let mut cursor = 0;
        while cursor < children.len() {
            let Some(name) = children[cursor].as_text() else {
                break;
            };
            let (args, advance) = match children.get(cursor + 1) {
                Some(next) if next.as_list().is_some() => {
                    (next.as_list().unwrap_or_default().to_vec(), 2)
                }
                // A text follower is the next directive's key.
                Some(next) if next.as_text().is_some() => (Vec::new(), 1),
                Some(scalar) => (vec![scalar.clone()], 2),
                None => (Vec::new(), 1),
            };
            cursor += advance;
            if ADDRESS_KEYS.contains(&name) {
                sites.push(Site {
                    block: number,
                    key: name.to_owned(),
                    addresses: spelled_addresses(name, &args),
                });
            }
        }
    }
    sites
}

/// The numbered blocks a decoded record declares.
pub(crate) fn block_numbers(document: &ZrdValue) -> Vec<u32> {
    zrd_flat_fields(objective_record(document))
        .into_iter()
        .filter_map(|(key, _)| objective_block_number(key))
        .collect()
}

/// **A spelled cross-objective address is the one-based block number; the
/// parse's `dec` stores it as record index `address − 1`.**
///
/// The conversion is pinned at both boundaries of the one-based range so a
/// regression to a zero-based reading fails on both sides at once: `1` names
/// the first record (index `0`) and `objectives` — a spelled value equal to
/// the block count — names the last record (index `objectives − 1`), which a
/// zero-based reading would put one past the array. `0` — the value a
/// zero-based reading would need to spell to name the first block — refuses
/// by name, and so does `objectives + 1`.
#[test]
fn accept_objaddr_a_spelled_address_is_the_one_based_block_number() {
    const BLOCKS: u32 = 50;

    // Both boundaries of the measured range, then the whole interior.
    assert_eq!(
        resolve_objective_address(1, BLOCKS),
        Ok(SymbolId(0)),
        "spelled 1 names the first block: the parse decrements it to index 0"
    );
    assert_eq!(
        resolve_objective_address(i64::from(BLOCKS), BLOCKS),
        Ok(SymbolId(BLOCKS - 1)),
        "a spelled value equal to the block count names the last block — \
         in range only under the one-based reading"
    );
    for address in 1..=i64::from(BLOCKS) {
        let symbol = resolve_objective_address(address, BLOCKS)
            .unwrap_or_else(|refusal| panic!("block number {address} resolves: {refusal}"));
        assert_eq!(
            i64::from(symbol.0),
            address - 1,
            "the conversion is the parse's `dec`, not a decimal parse"
        );
        assert_eq!(
            address_of(symbol),
            address,
            "the one-based address round-trips through the record index"
        );
    }

    // The values a zero-based reading would spell: `0` for the first block
    // and `objectives` for one past the last — the first is out of range, the
    // second is the boundary the reconciled records rely on.
    assert_eq!(
        resolve_objective_address(0, BLOCKS),
        Err(AddressRefusal {
            address: 0,
            objectives: BLOCKS
        }),
        "a spelled 0 names no block — the one-based range starts at 1"
    );
    let refusal = resolve_objective_address(i64::from(BLOCKS) + 1, BLOCKS)
        .expect_err("one past the block count refuses, never clamps");
    assert_eq!(
        refusal,
        AddressRefusal {
            address: i64::from(BLOCKS) + 1,
            objectives: BLOCKS
        },
        "the refusal carries the spelled address and the record's own count"
    );
    assert!(
        refusal.to_string().contains(OUT_OF_RANGE_OBJECTIVE_ADDRESS),
        "the refusal names the rule: {refusal}"
    );
}

/// **Every reconciled record spells one-based block numbers — and each of
/// the four spells an address equal to its own block count.**
///
/// The convention's discriminator is not one record's word: M02's block 13
/// wakes `[14, 50]` on a 50-block record, M03's block 21 naps `55` on a
/// 55-block record, M04's block 23 wakes `[37, 52]` on a 52-block record and
/// M06's block 67 wakes `[68, 71, 82]` on an 82-block record — under a
/// zero-based reading each of those addresses is one past the record's last
/// index, while under the measured one-based rule each names the record's
/// last block (`OBJECTIVE50`, `OBJECTIVE55`, `OBJECTIVE52`, `OBJECTIVE82`).
/// M06's second discriminator is pinned with them: nothing in its record
/// spells `50` although block 50 naps `51`, so under an index reading its
/// `INSTANTLOSS` latch (block 51) would have no completion edge at all.
///
/// Each record is re-read from the installation through production
/// discovery, its blocks are walked by the measured grammar and every
/// spelled address is resolved by the production rule — so this fails if the
/// records change, if the walk misreads them or if the rule's range moves.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_objaddr_every_reconciled_record_spells_its_own_block_count() {
    // (census row, expected block count, the site that spells the count)
    let reconciled = [
        (
            "zbd/c1/m02",
            50u32,
            (13u32, "WAKE_OBJECTIVE_WHEN_I_COMPLETE"),
        ),
        ("zbd/c1b/m03", 55, (21, "NAP_OBJECTIVE_WHEN_I_COMPLETE")),
        ("zbd/c1/m04", 52, (23, "WAKE_OBJECTIVE_WHEN_I_COMPLETE")),
        ("zbd/c2/m01", 82, (67, "WAKE_OBJECTIVE_WHEN_I_COMPLETE")),
    ];

    for (mission, blocks, (speller, key)) in reconciled {
        let (document, member) = read_control_member(&game_dir(), mission)
            .unwrap_or_else(|| panic!("the measured rule finds {mission}'s control member"));
        assert_eq!(
            member.objective_blocks, blocks,
            "{mission} declares {blocks} numbered blocks"
        );
        assert_eq!(
            block_numbers(&document),
            (1..=blocks).collect::<Vec<_>>(),
            "{mission}'s record numbers its blocks 1..={blocks}, no gaps"
        );

        let sites = walk(&document);
        assert!(
            !sites.is_empty(),
            "{mission}'s record spells cross-objective addresses"
        );

        // Every address the record spells resolves to one of its own blocks.
        for site in &sites {
            for address in &site.addresses {
                let symbol = resolve_objective_address(i64::from(*address), blocks).unwrap_or_else(
                    |refusal| {
                        panic!(
                            "{mission} OBJECTIVE{} {} spells {address}: {refusal}",
                            site.block, site.key
                        )
                    },
                );
                assert_eq!(
                    i64::from(symbol.0) + 1,
                    i64::from(*address),
                    "{mission}: the spelled block number resolves to its own record index"
                );
            }
        }

        // The discriminating datum: a spelled value equal to the block count
        // is in range and names the record's last block.
        let site = sites
            .iter()
            .find(|site| {
                site.block == speller && site.key == key && site.addresses.contains(&blocks)
            })
            .unwrap_or_else(|| {
                panic!(
                    "{mission} OBJECTIVE{speller} spells {blocks} — its own block count — \
                     through {key}"
                )
            });
        assert_eq!(
            resolve_objective_address(i64::from(blocks), blocks),
            Ok(SymbolId(blocks - 1)),
            "{mission}: spelled {blocks} names OBJECTIVE{blocks}, the record's last block"
        );
        assert!(
            site.block == speller,
            "{mission}: the boundary site is OBJECTIVE{speller}'s {key} {:?}",
            site.addresses
        );
    }

    // M06's second discriminator: under an index reading the INSTANTLOSS
    // latch (block 51) would have no incoming edge — block 50 naps 51 and no
    // site anywhere in the record spells 50.
    let (document, _member) = read_control_member(&game_dir(), "zbd/c2/m01")
        .expect("the measured rule finds zbd/c2/m01's control member");
    let sites = walk(&document);
    assert!(
        sites.iter().all(|site| !site.addresses.contains(&50)),
        "nothing in M06 spells 50, so an index reading leaves the failure \
         latch unreachable"
    );
    assert!(
        sites.iter().any(|site| {
            site.block == 50
                && site.key == "NAP_OBJECTIVE_WHEN_I_COMPLETE"
                && site.addresses.contains(&51)
        }),
        "block 50 naps 51 — the failure latch's one completion edge, present \
         only under the block-number reading"
    );
}
