//! F62 fuzz target — arbitrary bytes against the production parser,
//! under the default bounded context. See `corpus` in `cs_xtask` for the
//! contract this target exercises.

#![no_main]

use cs_formats::script_raw::{
    ByteSpan, Confidence, OpcodeEntry, OpcodeLedger, ProgramKind, ProgramLocator, walk_program,
};
use libfuzzer_sys::fuzz_target;

// A ledger that knows every one-byte opcode value, so the walk exercises
// the truncation boundary instead of stopping at the first unknown byte.
fuzz_target!(|bytes: &[u8]| {
    let mut ledger = OpcodeLedger::new();
    for opcode in 0..=255u32 {
        ledger
            .insert(
                OpcodeEntry::new(opcode, "OP", ProgramKind::Unknown, Confidence::Lead, "fuzz")
                    .expect("the ledger entry is well-formed"),
            )
            .expect("opcode values are distinct");
    }
    let locator = ProgramLocator::new("fuzz/script.program", None, ByteSpan::new(0, bytes.len() as u64));
    let _ = walk_program("fuzz", &locator, bytes, 1, &ledger, 1024);
});
