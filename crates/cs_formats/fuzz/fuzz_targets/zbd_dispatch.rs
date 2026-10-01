//! F62 fuzz target — arbitrary bytes against the production parser,
//! under the default bounded context. See `corpus` in `cs_xtask` for the
//! contract this target exercises.

#![no_main]

use cs_formats::zbd::{ZbdProbe, dispatch};
use cs_types::install::RelativePath;
use libfuzzer_sys::fuzz_target;

const ROLES: &[&str] = &[
    "zbd/interp.zbd",
    "zbd/planes.zbd",
    "zbd/soundsl.zbd",
    "zbd/c1/zrdr.zbd",
    "zbd/c1/m01/mis_anim.zbd",
    "zbd/c1/rtexture2.zbd",
    "elsewhere/file.bin",
];

fuzz_target!(|bytes: &[u8]| {
    let spelling = ROLES[(bytes.first().copied().unwrap_or(0) as usize) % ROLES.len()];
    let path = RelativePath::new(spelling).expect("fixed role spellings are valid");
    let _ = dispatch(ZbdProbe::new("fuzz/zbd.dispatch", &path, &bytes[..bytes.len().min(64)]));
});
