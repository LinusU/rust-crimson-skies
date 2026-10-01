//! F62 fuzz target — arbitrary bytes against the production parser,
//! under the default bounded context. See `corpus` in `cs_xtask` for the
//! contract this target exercises.

#![no_main]

use cs_formats::script_raw::{ScriptSource, inventory_scripts};
use cs_types::install::RelativePath;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    let path = RelativePath::new("zbd/interp.zbd").expect("fixed spelling is valid");
    let sources = [ScriptSource::new("fuzz/script.inventory", &path, bytes)];
    let _ = inventory_scripts(&sources);
});
