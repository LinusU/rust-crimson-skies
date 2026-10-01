//! F62 fuzz target — arbitrary bytes against the production parser,
//! under the default bounded context. See `corpus` in `cs_xtask` for the
//! contract this target exercises.

#![no_main]

use cs_formats::legacy_profile::{LegacyLimits, read_legacy_profile, synthetic_layout};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    let layout = synthetic_layout();
    let _ = read_legacy_profile(bytes, &layout, &LegacyLimits::designed());
});
