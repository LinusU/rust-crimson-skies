//! Acceptance tests for task #1849: the retail import of the `player.zrd`
//! `crash` record the contact response reads.
//!
//! Test prefix: `accept_parity4_m01_contact_`. The retail test is
//! `#[ignore = "requires CS_GAME_DIR"]`; it fails loudly without the
//! installation and is run with `--include-ignored`.

use std::path::PathBuf;

use cs_content::original_airframe::import_retail_crash;

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity4_m01_contact_crash_record_imports_the_retail_values() {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR is set"));
    let crash = import_retail_crash(&root).expect("the crash record imports");
    // The file stores single-precision numbers.
    assert_eq!(crash.bounce_factor, f64::from(0.6_f32));
    assert_eq!(crash.armor_damage_range, [50.0, 300.0]);
    assert_eq!(crash.health_damage_range, [50.0, 300.0]);
}
