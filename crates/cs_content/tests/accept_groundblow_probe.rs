//! Acceptance tests for task #2210 (`GROUNDBLOW-PROBE-SEAM`): the three
//! ground-blow globals of `player.zrd` imported through the production
//! importer, and the segment-probe seam `cs_sim`'s law declares for them.
//!
//! Test prefix: `accept_groundblow_probe_`. The retail values 400 / 10 / 0.5
//! and their consumer sites (`0x48bfa3` reads `[0x71c420]`, `0x48c220` scales
//! by `groundblow_mag` or `ai_groundblow`) are
//! `docs/findings/2026-10-29-groundblow-world-segment-sweep.md` section 1.
//! The test that reads `CS_GAME_DIR` is `#[ignore = "requires CS_GAME_DIR"]`
//! and fails loudly without it.
//!
//! What this task does **not** do is stated in the finding and in
//! `docs/findings/2026-10-29-groundblow-probe-seam.md`: the probe's world query
//! is not wired, because none of the port's queries answers what `0x4c8ec0`
//! answers (section 8 there), and the mission's actual step site is not in
//! either file the task names.

use std::path::PathBuf;

use cs_content::original_airframe::{GLOBAL_FIELDS, OriginalDocuments};
use cs_sim::flight::original::OriginalGlobals;

/// The three fields this task adds, with retail's values.
const GROUND_BLOW: [(&str, &str, f64); 3] = [
    // (vocabulary field, `player.zrd` key, retail value)
    ("groundblow_elev_m", "groundblow_elev", 400.0),
    ("groundblow_mag", "groundblow_mag", 10.0),
    ("ai_groundblow", "ai_groundblow", 0.5),
];

/// The importer's vocabulary and the law's must stay the same list, and the
/// three ground-blow names must be in it: a rename on either side would
/// silently drop them at the crate boundary.
#[test]
fn accept_groundblow_probe_globals_are_declared_on_both_sides() {
    assert_eq!(
        GLOBAL_FIELDS,
        cs_sim::flight::original::GLOBAL_FIELDS,
        "the importer and the law must consume the same global fields"
    );
    for (field, _, _) in GROUND_BLOW {
        assert!(
            GLOBAL_FIELDS.contains(&field),
            "GLOBAL_FIELDS must declare {field}"
        );
    }
    // The ground-blow names carry their unit where they have one: elevation is
    // metres because the world is, and the gains are dimensionless.
    assert!(GLOBAL_FIELDS.contains(&"groundblow_elev_m"));
}

/// **The retail table**: the first `player.zrd` entry states the ground blow on
/// (`docs/findings/2026-10-29-groundblow-world-segment-sweep.md` section 1),
/// and both the importer and the law accept those numbers as data.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_groundblow_probe_reads_the_retail_ground_blow_globals() {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR is set"));
    let documents = OriginalDocuments::read(&root).expect("the installation's zrdr.zbd reads");
    let globals = documents.globals().expect("the globals import");

    for (field, key, expected) in GROUND_BLOW {
        let value = globals
            .value(field)
            .unwrap_or_else(|| panic!("the globals state {field}"));
        assert_eq!(value.value, expected, "{field}");
        assert_eq!(value.key_path, key, "{field}'s key path");
        assert_eq!(
            value
                .provenance
                .source
                .as_ref()
                .map(|source| source.member_key()),
            Some(Some("player.zrd")),
            "{field} must record where it was read"
        );
    }

    // The law consumes the same flat list and reads the three values by name:
    // removing any of them from `GLOBAL_FIELDS` fails the import, not a check.
    let law = OriginalGlobals::from_values(&globals.law_values())
        .expect("the law accepts the retail globals");
    assert_eq!(law.groundblow_elev_m, 400.0);
    assert_eq!(law.groundblow_mag, 10.0);
    assert_eq!(law.ai_groundblow, 0.5);
}
