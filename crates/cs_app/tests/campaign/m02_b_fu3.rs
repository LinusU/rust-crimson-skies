//! `M02-B-FU3` acceptance: the cross-objective address rule on M02's own
//! record (retail — needs `CS_GAME_DIR`).
//!
//! Task: Rally #802, "Measure what the original does with a cross-objective
//! address past the record's block count". Shared contract:
//! `docs/contracts/SCRIPT-MISSION.md` ("IR requirements"). Measurement, its
//! provenance and its residual unknowns:
//! `docs/findings/2026-10-09-m02-b-fu3-out-of-range-wake-address.md`.
//!
//! M02's control record is the record that carries the mission's only
//! cross-objective address past its own block count when the document's
//! integers are read as record positions: block `OBJECTIVE13` spells
//! `WAKE_OBJECTIVE_WHEN_I_COMPLETE [14, 50]` while the record declares 50
//! numbered blocks. What the original *does* with such an address is measured
//! from the executable (the parse decrements the address; the wake walk
//! `0x469af0` checks nothing at all), and this test pins the rule this engine
//! decided on top of that measurement against the record itself:
//!
//! * every cross-objective address M02 spells resolves **inside** its block
//!   count under the one-based rule — so the record contains no address this
//!   engine would have to refuse;
//! * M02's own `50` resolves to record index 49, the block the record spells
//!   `OBJECTIVE50`, and **not** to a record past the end;
//! * one past M02's count refuses, so the retail boundary is the same
//!   boundary the synthetic suite exercises (`cs_sim`'s
//!   `accept_m02_b_fu3_*`).
//!
//! The test re-derives the record through production code
//! (`SourceContext::control_program`, `discover_container`, `decode_zrd`) and
//! asserts no expected value it did not re-read from the decoded document.

use std::path::PathBuf;
use std::sync::OnceLock;

use cs_app::mission_control::survey_mission_control_programs;
use cs_content::campaign_bindings::{MissionControlBinding, SourceContext};
use cs_content::objectives::objective_block_number;
use cs_content::stunts::{ZrdValue, decode_zrd, objective_record, zrd_flat_fields};
use cs_formats::script_raw::discover_container;
use cs_script::ir::SymbolId;
use cs_sim::objectives::address::{
    AddressRefusal, OUT_OF_RANGE_OBJECTIVE_ADDRESS, resolve_objective_address,
};
use cs_types::install::RelativePath;

use crate::common::{label, load_inventory};

/// Every directive key that spells a cross-objective address — the same list
/// the original's parser feeds through `dec` into a `−1`-terminated index
/// array, plus the two scalar spellings (`TICK_DEPENDS_ON_OBJ` at `+0x10` and
/// `HIDE_OBJ` at `+0xd8`) that take one address instead of a list.
const DIRECTIVE_KEYS: [&str; 8] = [
    "WAKE_OBJECTIVE",
    "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
    "KILL_OBJECTIVE_WHEN_I_COMPLETE",
    "NAP_OBJECTIVE_WHEN_I_COMPLETE",
    "SLEEP_OBJECTIVE_WHEN_I_COMPLETE",
    "WAKE_OBJECTIVE_WHEN_I_SLEEP",
    "TICK_DEPENDS_ON_OBJ",
    "HIDE_OBJ",
];

/// The original installation, as the environment declares it.
fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: M02-B-FU3 needs the retail capability; run this suite with \
             `--include-ignored` and CS_GAME_DIR pointing at the read-only installation"
        )
    }))
}

/// The source context, read once for the whole suite (fingerprinting the
/// installation walks every file, so it happens exactly once).
fn context() -> &'static SourceContext {
    static CONTEXT: OnceLock<SourceContext> = OnceLock::new();
    CONTEXT.get_or_init(|| {
        SourceContext::read(&game_dir()).expect("the installation yields a source context")
    })
}

/// M02's control binding, derived fresh through production code: the archive
/// the campaign layout declares and the member the measured control-member
/// rule picks out of its whole member set.
pub(crate) fn control_binding() -> MissionControlBinding {
    let title = load_inventory()
        .iter()
        .find(|(work_order, _)| work_order.as_str() == "M02")
        .map(|(_, title)| title.clone())
        .expect("the declared inventory has an M02 work order");
    context()
        .control_program(label("M02"), &title)
        .expect("M02's control program binds through the measured rule")
}

/// M02's control member, decoded a second time from the bytes on disk through
/// production discovery — an independent walk from the binding's, so the
/// assertions below read the document rather than the measurement of it.
pub(crate) fn control_document() -> ZrdValue {
    let binding = control_binding();
    let bytes = std::fs::read(game_dir().join(&binding.program_asset))
        .expect("M02's reader archive reads from disk");
    let relative =
        RelativePath::new(&binding.program_asset.to_lowercase()).expect("the path is relative");
    let discovery = discover_container(&relative.logical_key(), &relative, &bytes);
    assert!(
        discovery.findings().is_empty(),
        "the archive locates without findings: {:?}",
        discovery.findings()
    );
    for program in discovery.programs() {
        if program.locator().member() != Some(binding.control_member.as_str()) {
            continue;
        }
        return decode_zrd(program.bytes())
            .unwrap_or_else(|error| panic!("the control member decodes: {error}"));
    }
    panic!(
        "the member the measured rule chose ({}) is in the archive",
        binding.control_member
    );
}

/// One numbered `OBJECTIVE<N>` block: its authored number and every
/// cross-objective address it spells, in spelling order.
pub(crate) struct Block {
    pub(crate) number: u32,
    pub(crate) addresses: Vec<(String, Vec<u32>)>,
}

/// Walks the decoded record's numbered blocks with the measured directive
/// grammar (a site is a key plus the list beside it; a text follower ends the
/// site), keeping only the integer arguments of the keys
/// [`DIRECTIVE_KEYS`] names.
pub(crate) fn blocks_with_addresses(document: &ZrdValue) -> Vec<Block> {
    let mut blocks = Vec::new();
    for (key, value) in zrd_flat_fields(objective_record(document)) {
        let Some(number) = objective_block_number(key) else {
            continue;
        };
        let mut addresses = Vec::new();
        let Some(children) = value.as_list() else {
            blocks.push(Block { number, addresses });
            continue;
        };
        let mut cursor = 0;
        while cursor < children.len() {
            let Some(name) = children[cursor].as_text() else {
                break;
            };
            let next = children.get(cursor + 1);
            let args = match next {
                Some(next) if next.as_list().is_some() => {
                    cursor += 2;
                    next.as_list().unwrap_or_default()
                }
                Some(next) if next.as_text().is_some() => {
                    cursor += 1;
                    &[]
                }
                Some(_) => {
                    cursor += 2;
                    &[]
                }
                None => {
                    cursor += 1;
                    &[]
                }
            };
            if DIRECTIVE_KEYS.contains(&name) {
                let spelled: Vec<u32> = args.iter().filter_map(ZrdValue::as_int).collect();
                addresses.push((name.to_owned(), spelled));
            }
        }
        blocks.push(Block { number, addresses });
    }
    blocks
}

/// **Every cross-objective address M02 spells resolves inside its block count,
/// and M02's own `50` names the record the block `OBJECTIVE50` spells — one
/// past M02's count refuses.**
///
/// The record is read fresh from the installation, the addresses are taken
/// from the decoded document (never from the measurement's counts), and the
/// rule is the production `cs_sim::objectives::address` resolver — so this
/// fails if the rule's range changes, if the record's block count changes, or
/// if a future decode reads M02's addresses differently.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m02_b_fu3_m02s_wake_addresses_resolve_inside_its_block_count() {
    let document = control_document();
    let binding = control_binding();
    let blocks = blocks_with_addresses(&document);
    assert_eq!(
        blocks.len() as u32,
        binding.record.blocks(),
        "the independent walk sees every numbered block the measurement counted"
    );
    let count = blocks.len() as u32;
    assert!(count > 0, "M02's record declares numbered blocks");

    // The census measures the same record a second time through production
    // code, so the count the rule resolves against is not a single walk's word.
    let census = survey_mission_control_programs(&game_dir())
        .expect("the installation measures a control census");
    let row = census
        .row("zbd/c1/m02")
        .expect("M02's reader archive is measured by the census");
    assert_eq!(
        row.record()
            .expect("M02's census row carries its measured control record")
            .blocks(),
        count,
        "the census and this walk agree on M02's block count"
    );

    // Every address M02 spells resolves — the lower and upper bound of the
    // one-based range hold over the whole record, not only over the site the
    // findings name.
    let mut spelled = 0usize;
    for block in &blocks {
        for (key, args) in &block.addresses {
            for address in args {
                spelled += 1;
                let symbol = resolve_objective_address(i64::from(*address), count).unwrap_or_else(|refusal| {
                    panic!(
                        "{}'s address in block OBJECTIVE{} is inside M02's {count} blocks: {refusal}",
                        key, block.number
                    )
                });
                assert_eq!(
                    i64::from(symbol.0) + 1,
                    i64::from(*address),
                    "address {address} resolves to record index {} of {count}",
                    symbol.0
                );
            }
        }
    }
    assert!(
        spelled > 0,
        "M02 spells cross-objective addresses; the walk above measured some"
    );

    // The site the findings name: `OBJECTIVE13` wakes `[14, 50]`, and `50` is
    // exactly M02's block count — the address a zero-based reading would put
    // past the record's end and the one-based reading (the parse's `dec`)
    // spells as the last block. The literal is pinned by M02-B's graph test;
    // what is asserted here is that the datum really sits *on* the count.
    let waking = blocks
        .iter()
        .find_map(|block| {
            (block.number == 13).then(|| {
                block
                    .addresses
                    .iter()
                    .find(|(key, _)| key.contains("WAKE"))
                    .cloned()
            })
        })
        .flatten()
        .expect("M02's OBJECTIVE13 spells a wake directive");
    let boundary = i64::from(count);
    assert!(
        waking
            .1
            .iter()
            .any(|address| i64::from(*address) == boundary),
        "OBJECTIVE13 spells the address {boundary} — M02's own block count: {:?}",
        waking.1
    );

    // The rule's answer for M02's own at-the-count address: the record index
    // 49 — the block the record spells `OBJECTIVE50`, the last one — and never
    // a record past the end.
    assert_eq!(
        resolve_objective_address(boundary, count),
        Ok(SymbolId(count - 1)),
        "M02's address {boundary} names the last of its {count} records \
         (one-based), not record {count}"
    );
    assert_eq!(
        blocks[(count - 1) as usize].number,
        count,
        "record index {} is the block the record spells OBJECTIVE{count}",
        count - 1
    );

    // And the out-of-range arm against M02's real count: one past refuses, by
    // name, with the rule spelled out.
    let refusal: AddressRefusal = resolve_objective_address(i64::from(count) + 1, count)
        .expect_err("one past M02's block count must refuse, never clamp");
    assert_eq!(
        refusal,
        AddressRefusal {
            address: i64::from(count) + 1,
            objectives: count
        },
        "the refusal carries M02's own count"
    );
    let message = refusal.to_string();
    assert!(
        message.contains(OUT_OF_RANGE_OBJECTIVE_ADDRESS),
        "the refusal names the rule: {message}"
    );
}
