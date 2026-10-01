//! F62 fuzz target — arbitrary bytes against the production parser,
//! under the default bounded context. See `corpus` in `cs_xtask` for the
//! contract this target exercises.

#![no_main]

use cs_formats::zbd::read_wave_header;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    let _ = read_wave_header(bytes);
});
