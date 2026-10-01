//! F62 fuzz target — arbitrary bytes against the production parser,
//! under the default bounded context. See `corpus` in `cs_xtask` for the
//! contract this target exercises.

#![no_main]

use cs_formats::gamez::read_gamez_meshes;
use cs_formats::ParseContext;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    let mut context = ParseContext::with_defaults("fuzz/gamez.meshes");
    let _ = read_gamez_meshes(&mut context, "fuzz/gamez.meshes", bytes);
});
