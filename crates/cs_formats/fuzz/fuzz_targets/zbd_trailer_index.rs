//! F62 fuzz target — arbitrary bytes against the production parser,
//! under the default bounded context. See `corpus` in `cs_xtask` for the
//! contract this target exercises.

#![no_main]

use cs_formats::ParseContext;
use cs_formats::zbd::{ZbdProbe, dispatch, read_version_one_index};
use cs_types::install::RelativePath;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    let path = RelativePath::new("zbd/zrdr.zbd").expect("fixed role spelling is valid");
    let Ok(decided) = dispatch(ZbdProbe::new("fuzz/zbd.trailer", &path, &bytes[..bytes.len().min(64)])) else {
        return;
    };
    let mut context = ParseContext::with_defaults("fuzz/zbd.trailer_index");
    let _ = read_version_one_index(&mut context, decided, bytes);
});
