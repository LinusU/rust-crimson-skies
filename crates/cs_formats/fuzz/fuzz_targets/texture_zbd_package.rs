//! F62 fuzz target — arbitrary bytes against the production parser,
//! under the default bounded context. See `corpus` in `cs_xtask` for the
//! contract this target exercises.

#![no_main]

use cs_formats::{AllocationBudget, read_zbd_textures};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    let mut budget = AllocationBudget::with_defaults("fuzz/texture.zbd_package");
    let _ = read_zbd_textures("fuzz/texture.zbd_package", bytes, &mut budget);
});
