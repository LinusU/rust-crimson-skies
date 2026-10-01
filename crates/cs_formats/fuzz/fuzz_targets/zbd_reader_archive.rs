//! F62 fuzz target — arbitrary bytes against the production parser,
//! under the default bounded context. See `corpus` in `cs_xtask` for the
//! contract this target exercises.

#![no_main]

use cs_formats::ParseContext;
use cs_formats::zbd::{MemberExtent, MemberTable, ZbdProbe, dispatch, read_reader_archive};
use cs_types::evidence::SourceSpan;
use cs_types::install::RelativePath;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    let path = RelativePath::new("zbd/zrdr.zbd").expect("fixed role spelling is valid");
    let Ok(decided) = dispatch(ZbdProbe::new("fuzz/zbd.reader", &path, &bytes[..bytes.len().min(64)])) else {
        return;
    };
    // Member extents the fuzzer supplies: two u64s then the member name.
    let (a, b) = match bytes.get(..16) {
        Some(words) => (
            u64::from_le_bytes(words[..8].try_into().unwrap()),
            u64::from_le_bytes(words[8..].try_into().unwrap()),
        ),
        None => (0, bytes.len() as u64),
    };
    let members = [MemberExtent::new(b"fuzz", Some(1), SourceSpan { offset: a % 4096, length: b % 8192 })];
    let table = MemberTable::from_dispatch(&decided, &members);
    let mut context = ParseContext::with_defaults("fuzz/zbd.reader_archive");
    let _ = read_reader_archive(&mut context, &table, bytes);
});
