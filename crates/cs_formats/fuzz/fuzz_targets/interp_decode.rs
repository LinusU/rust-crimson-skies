//! F62 fuzz target — arbitrary bytes against the production parser,
//! under the default bounded context. See `corpus` in `cs_xtask` for the
//! contract this target exercises.

#![no_main]

use cs_formats::{ParseContext, decode_interp};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    let mut context = ParseContext::with_defaults("fuzz/interp.decode");
    let _ = decode_interp(&mut context, bytes);
});
