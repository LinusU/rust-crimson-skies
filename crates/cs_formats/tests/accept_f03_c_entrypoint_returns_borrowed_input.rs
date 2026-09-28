//! Acceptance scenario F03-C: the entrypoint hands borrowed input back out.
//!
//! `ParseContext::parse` ties the reader it gives an attempt to the lifetime
//! of that attempt's bytes, so a bounded field read out of them comes back as
//! a *slice of the input*, not a copy of it. A parser forced to heap-copy
//! every bounded string or member range just to return it would bypass the
//! allocation budget that `specs/F03-bounded-binary-parsing-primitives.md`
//! non-negotiable #2 exists to bound.

mod common;

use common::{CONTAINER, EXPECTED_LABEL, EXPECTED_MAGIC, LABEL_LEN, record_bytes};
use cs_formats::ParseContext;

/// The field an attempt returns aliases the input bytes — the pointer is the
/// address inside the slice that was handed in — and reading records charges
/// the allocation ledger nothing.
#[test]
fn accept_f03_c_entrypoint_hands_back_borrowed_input() {
    let bytes = record_bytes();
    let mut context = ParseContext::with_defaults(CONTAINER);

    let (magic, label): (u32, &str) = context
        .parse("record", &bytes, |reader, _allocation, _recursion| {
            let magic = reader.read_u32("header.magic")?;
            let _version = reader.read_u16("header.version")?;
            let label = reader.read_bounded_cstr("header.label", LABEL_LEN)?;
            Ok((magic, label))
        })
        .expect("the intact record parses through the entrypoint");

    assert_eq!(magic, EXPECTED_MAGIC);
    assert_eq!(label, EXPECTED_LABEL);
    // `magic` and `label` came out of one attempt and still name this input:
    // `header.label` starts at offset 6, so a copy would have a different
    // address than the bytes of `record_bytes()` that were passed in.
    assert_eq!(
        label.as_ptr(),
        bytes[6..].as_ptr(),
        "the returned field aliases the input: no copy was made"
    );
    assert_eq!(
        context.allocation().used(),
        0,
        "returning a borrowed field charges nothing to the budget"
    );
    assert_eq!(context.recursion().depth(), 0);
}
