//! F62 fuzz target — arbitrary bytes against the production parser,
//! under the default bounded context. See `corpus` in `cs_xtask` for the
//! contract this target exercises.

#![no_main]

use cs_formats::{AllocationBudget, read_tga};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    let mut budget = AllocationBudget::with_defaults("fuzz/texture.tga");
    let _ = read_tga("fuzz/texture.tga", bytes, &mut budget);
});
