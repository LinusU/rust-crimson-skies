//! F62 fuzz target — arbitrary bytes against the production parser,
//! under the default bounded context. See `corpus` in `cs_xtask` for the
//! contract this target exercises.

#![no_main]

use cs_formats::script_raw::discover_container;
use cs_types::install::RelativePath;
use libfuzzer_sys::fuzz_target;

const PATHS: &[&str] = &[
    "zbd/interp.zbd",
    "zbd/c1/m01/zrdr.zbd",
    "zbd/c1/m01/mis_anim.zbd",
    "elsewhere/file.bin",
];

fuzz_target!(|bytes: &[u8]| {
    let spelling = PATHS[(bytes.first().copied().unwrap_or(0) as usize) % PATHS.len()];
    let path = RelativePath::new(spelling).expect("fixed spellings are valid");
    let _ = discover_container("fuzz/script.discovery", &path, bytes);
});
