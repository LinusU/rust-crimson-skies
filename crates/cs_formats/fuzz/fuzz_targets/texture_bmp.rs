//! F62 fuzz target — arbitrary bytes against the production parser,
//! under the default bounded context. See `corpus` in `cs_xtask` for the
//! contract this target exercises.

#![no_main]

use cs_formats::{AllocationBudget, read_bmp};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    let mut budget = AllocationBudget::with_defaults("fuzz/texture.bmp");
    let _ = read_bmp("fuzz/texture.bmp", bytes, &mut budget);
});
