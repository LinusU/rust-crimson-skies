//! F62 fuzz target — arbitrary bytes against the production parser,
//! under the default bounded context. See `corpus` in `cs_xtask` for the
//! contract this target exercises.

#![no_main]

use cs_formats::{ParseContext, RofLimits, read_member, read_tree};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    let mut context = ParseContext::with_defaults("fuzz/rof.member");
    let Ok(tree) = read_tree(&mut context, bytes) else {
        return;
    };
    for member in tree.members() {
        let _ = read_member(&context, bytes, member, &RofLimits::default());
    }
});
