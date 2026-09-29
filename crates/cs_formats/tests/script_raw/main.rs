//! Acceptance stage F13-A: the script-container inventory and its
//! disassembly-neutral evidence schema
//! (`specs/F13-mission-language-discovery-and-compatibility-closure.md`,
//! section `### F13-A`, AC01: "Inventory all candidate script containers
//! without pretending unknown records are instructions").
//!
//! Every byte in this file is authored here. No original game data, no
//! `CS_GAME_DIR` access.

use cs_formats::script_raw::{
    ByteSpan, Candidacy, Confidence, DiscoveryFinding, DispatchOutcome, EXCLUDED_FAMILY_REASON,
    EvidenceError, EvidenceLocator, FormatDiscriminator, INTERP_BODY_UNESTABLISHED,
    InstructionClaimError, InstructionStatus, InventoryFinding, LocatorKind, MAX_LEADS_PER_RECORD,
    MAX_NOTE_BYTES, OpcodeEntry, OpcodeLedger, ProbeConfig, ProbeSession, ProgramError,
    ProgramKind, ProgramLocator, RecordKind, RecordReachability, ReferenceKind, ResearchMethod,
    STRUCTURE_UNUSED_REASON, ScriptContainerEntry, ScriptEvidence, ScriptInventory, ScriptRole,
    ScriptSource, SignatureClaim, SignatureShape, SignatureTable, UNDECODED_UNESTABLISHED,
    UNREACHED_UNUSED_REASON, discover_container, inventory_scripts, mission_scope, probe_records,
    walk_program,
};
use cs_formats::zbd::{
    ANIMATION_SIGNATURE, ANIMATION_VERSION, INTERP_SIGNATURE, INTERP_VERSION, ZbdFamily,
};
use cs_formats::{INDEX_ENTRY_BYTES, INTERP_HEADER_BYTES, NAME_FIELD_BYTES};
use cs_types::install::RelativePath;

use std::fs;
use std::path::PathBuf;

/// One authored script: its name and its lines of `(argument_count, data)`.
type AuthoredScript<'a> = (&'a [u8], &'a [(u32, &'a [u8])]);

/// Builds an INTERP container: header, index, then each script's lines of
/// `(argument_count, data)` and its zero terminator, followed by `trailer`.
fn interp(scripts: &[AuthoredScript<'_>], trailer: &[u8]) -> Vec<u8> {
    let body_start = INTERP_HEADER_BYTES + scripts.len() * INDEX_ENTRY_BYTES;
    let mut body = Vec::new();
    let mut offsets = Vec::new();
    for (_, lines) in scripts {
        offsets.push((body_start + body.len()) as u32);
        for (count, data) in *lines {
            body.extend_from_slice(&(data.len() as u32).to_le_bytes());
            body.extend_from_slice(&count.to_le_bytes());
            body.extend_from_slice(data);
        }
        body.extend_from_slice(&0u32.to_le_bytes());
    }
    let mut bytes = Vec::new();
    for word in [INTERP_SIGNATURE, INTERP_VERSION, scripts.len() as u32] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    for ((name, _), offset) in scripts.iter().zip(&offsets) {
        let mut field = [0u8; NAME_FIELD_BYTES];
        field[..name.len()].copy_from_slice(name);
        bytes.extend_from_slice(&field);
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&offset.to_le_bytes());
    }
    bytes.extend_from_slice(&body);
    bytes.extend_from_slice(trailer);
    bytes
}

/// An animation-family header followed by `payload`.
fn animation(payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for word in [ANIMATION_SIGNATURE, ANIMATION_VERSION] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes.extend_from_slice(payload);
    bytes
}

fn path(spelling: &str) -> RelativePath {
    RelativePath::new(spelling).expect("authored paths are valid")
}

fn entry<'a>(inventory: &'a ScriptInventory, spelling: &str) -> &'a ScriptContainerEntry {
    inventory
        .entries()
        .iter()
        .find(|entry| entry.path().as_str() == spelling)
        .unwrap_or_else(|| panic!("{spelling} is missing from the inventory"))
}

/// Asserts the records of `entry` cover every byte of the container, so no
/// byte range is hidden from the report.
fn assert_covers_container(entry: &ScriptContainerEntry) {
    let mut spans: Vec<ByteSpan> = entry.records().iter().map(|r| r.span()).collect();
    spans.sort();
    let mut cursor = 0;
    for span in spans {
        assert!(
            span.offset <= cursor,
            "{}: bytes 0x{cursor:x}..0x{:x} are in no record",
            entry.path().as_str(),
            span.offset
        );
        cursor = cursor.max(span.end());
    }
    assert_eq!(
        cursor,
        entry.len(),
        "{}: tail is in no record",
        entry.path().as_str()
    );
}

fn scan_evidence(container: &str, span: ByteSpan) -> ScriptEvidence {
    ScriptEvidence::new(
        ResearchMethod::ContainerScan,
        Confidence::Lead,
        EvidenceLocator::ContainerSpan {
            container: container.to_owned(),
            span,
        },
        "printable run looks like a command name",
    )
    .expect("a scan lead is valid evidence")
}

fn decode_evidence(container: &str, span: ByteSpan, confidence: Confidence) -> ScriptEvidence {
    ScriptEvidence::new(
        ResearchMethod::StructuralDecode,
        confidence,
        EvidenceLocator::ContainerSpan {
            container: container.to_owned(),
            span,
        },
        "a validating decoder walked these bytes as instructions",
    )
    .expect("structural-decode evidence is valid")
}

/// AC01: every candidate container is listed with its role, every byte of
/// every candidate is in a record, and no record is claimed as instructions.
#[test]
fn accept_f13_a_inventories_every_candidate_without_claiming_instructions() {
    let loading = interp(
        &[
            (b"alpha", &[(2, b"load\0one\0".as_slice())]),
            (
                b"beta",
                &[(1, b"go\0".as_slice()), (1, b"stop\0".as_slice())],
            ),
        ],
        b"\x01\x02JUNKDATA\x03",
    );
    let camera = animation(b"\0\0camera01\0\x7f\xff");
    let mission = animation(b"\x10\x20\x30");
    let reader = b"\xff\xfeRDR\0record-name\0\x01".to_vec();
    let texture = vec![0u8; 16];
    let unknown = b"????unknown-bytes".to_vec();

    let paths = [
        path("ZBD/interp.zbd"),
        path("ZBD/C1/cam_anim.zbd"),
        path("ZBD/C1/M01/mis_anim.zbd"),
        path("ZBD/C1/zrdr.zbd"),
        path("ZBD/C1/texture.zbd"),
        path("ZBD/C1/M01/strange.zbd"),
    ];
    let bytes = [&loading, &camera, &mission, &reader, &texture, &unknown];
    let sources: Vec<ScriptSource<'_>> = paths
        .iter()
        .zip(bytes)
        .map(|(path, bytes)| ScriptSource::new(path.as_str(), path, bytes))
        .collect();
    let inventory = inventory_scripts(&sources);

    let stats = inventory.stats();
    assert_eq!(stats.containers, 6);
    assert_eq!(stats.candidates, 5);
    assert_eq!(stats.excluded, 1);
    assert_eq!(stats.refused, 1);
    assert_eq!(
        stats.established, 0,
        "the inventory never claims instructions"
    );
    assert_eq!(inventory.candidates().count(), 5);

    for candidate in inventory.candidates() {
        assert_covers_container(candidate);
        for record in candidate.records() {
            assert!(
                !matches!(record.instructions(), InstructionStatus::Established { .. }),
                "{} {} was claimed as instructions",
                candidate.path().as_str(),
                record.span()
            );
        }
    }

    // INTERP: header, two index entries, two script bodies, one unclaimed tail.
    let loading_entry = entry(&inventory, "ZBD/interp.zbd");
    assert_eq!(
        loading_entry.candidacy(),
        Candidacy::Candidate {
            role: ScriptRole::LoadingScript,
            confidence: Confidence::Documented
        }
    );
    let kinds: Vec<RecordKind> = loading_entry.records().iter().map(|r| r.kind()).collect();
    assert_eq!(
        kinds,
        [
            RecordKind::InterpHeader,
            RecordKind::InterpIndexEntry { position: 0 },
            RecordKind::InterpIndexEntry { position: 1 },
            RecordKind::InterpScript { position: 0 },
            RecordKind::InterpScript { position: 1 },
            RecordKind::Unclaimed,
        ]
    );
    let records = loading_entry.records();
    assert_eq!(
        records[0].discriminator(),
        FormatDiscriminator::Signature {
            signature: INTERP_SIGNATURE,
            version: INTERP_VERSION
        }
    );
    assert_eq!(
        records[0].instructions(),
        &InstructionStatus::ContainerStructure
    );
    // The index entry names its script by span, not by copied bytes.
    let name = records[1].references()[0];
    assert_eq!(name.kind, ReferenceKind::ScriptName);
    assert_eq!(name.span, ByteSpan::new(INTERP_HEADER_BYTES as u64, 5));
    // Script bodies are decoded loading lines, not established instructions.
    let first_body = INTERP_HEADER_BYTES as u64 + 2 * INDEX_ENTRY_BYTES as u64;
    assert_eq!(records[3].span(), ByteSpan::new(first_body, 8 + 9 + 4));
    assert_eq!(
        records[3].instructions(),
        &InstructionStatus::Unestablished {
            reason: INTERP_BODY_UNESTABLISHED
        }
    );
    assert!(!records[3].evidence().is_empty());
    // The unclaimed tail stays visible, with its printable run as a lead.
    let tail = &records[5];
    assert_eq!(tail.span(), ByteSpan::new(loading.len() as u64 - 11, 11));
    assert_eq!(tail.discriminator(), FormatDiscriminator::Unknown);
    assert_eq!(
        tail.instructions(),
        &InstructionStatus::Unestablished {
            reason: UNDECODED_UNESTABLISHED
        }
    );
    let leads: Vec<ByteSpan> = tail.references().iter().map(|r| r.span).collect();
    assert_eq!(leads, [ByteSpan::new(loading.len() as u64 - 9, 8)]);
    assert!(
        tail.references()
            .iter()
            .all(|r| r.kind == ReferenceKind::StringLead && r.confidence == Confidence::Lead)
    );

    // Animation archives: distinct roles, validated header, one opaque record.
    let camera_entry = entry(&inventory, "ZBD/C1/cam_anim.zbd");
    assert_eq!(
        camera_entry.candidacy(),
        Candidacy::Candidate {
            role: ScriptRole::CameraAnimation,
            confidence: Confidence::Inferred
        }
    );
    let camera_record = &camera_entry.records()[0];
    assert_eq!(camera_entry.records().len(), 1);
    assert_eq!(camera_record.kind(), RecordKind::Opaque);
    assert_eq!(
        camera_record.discriminator(),
        FormatDiscriminator::Signature {
            signature: ANIMATION_SIGNATURE,
            version: ANIMATION_VERSION
        }
    );
    assert_eq!(camera_record.references()[0].span, ByteSpan::new(10, 8));
    assert_eq!(
        entry(&inventory, "ZBD/C1/M01/mis_anim.zbd").candidacy(),
        Candidacy::Candidate {
            role: ScriptRole::MissionAnimation,
            confidence: Confidence::Inferred
        }
    );

    // Reader archive: role from its name only, header unvalidated.
    let reader_entry = entry(&inventory, "ZBD/C1/zrdr.zbd");
    assert!(matches!(
        reader_entry.candidacy(),
        Candidacy::Candidate {
            role: ScriptRole::ReaderFile,
            confidence: Confidence::Inferred
        }
    ));
    assert!(matches!(
        reader_entry.records()[0].discriminator(),
        FormatDiscriminator::Unvalidated { .. }
    ));

    // Texture: listed, excluded with the reason, not searched.
    let texture_entry = entry(&inventory, "ZBD/C1/texture.zbd");
    assert!(matches!(
        texture_entry.candidacy(),
        Candidacy::Excluded {
            reason: EXCLUDED_FAMILY_REASON,
            ..
        }
    ));
    assert!(texture_entry.records().is_empty());

    // Unroutable: still a candidate, role unknown, the refusal on record.
    let unknown_entry = entry(&inventory, "ZBD/C1/M01/strange.zbd");
    assert_eq!(
        unknown_entry.dispatch(),
        DispatchOutcome::Refused {
            code: "unknown_family"
        }
    );
    assert_eq!(
        unknown_entry.candidacy(),
        Candidacy::Candidate {
            role: ScriptRole::Unknown,
            confidence: Confidence::Unknown
        }
    );
    assert_eq!(unknown_entry.records()[0].kind(), RecordKind::Opaque);
}

/// Failure case: an INTERP container the F07 reader refuses is kept as one
/// opaque candidate record with the refusal recorded, never dropped.
#[test]
fn accept_f13_a_refused_interp_stays_visible_as_opaque_record() {
    let mut bytes = interp(&[(b"alpha", &[(1, b"go\0".as_slice())])], b"");
    // Point the only script past the end of the container.
    let offset_field = INTERP_HEADER_BYTES + NAME_FIELD_BYTES + 4;
    bytes[offset_field..offset_field + 4].copy_from_slice(&0xffffu32.to_le_bytes());
    let path = path("zbd/interp.zbd");
    let inventory = inventory_scripts(&[ScriptSource::new("synthetic", &path, &bytes)]);

    let entry = &inventory.entries()[0];
    assert!(matches!(
        entry.dispatch(),
        DispatchOutcome::Dispatched { .. }
    ));
    assert!(matches!(
        entry.findings(),
        [InventoryFinding::ReaderRefused {
            reader: "interp",
            ..
        }]
    ));
    assert_eq!(entry.records().len(), 1);
    let record = &entry.records()[0];
    assert_eq!(record.kind(), RecordKind::Opaque);
    assert_eq!(record.span(), ByteSpan::new(0, bytes.len() as u64));
    assert!(matches!(
        record.instructions(),
        InstructionStatus::Unestablished { .. }
    ));
}

/// Failure case: bytes at the INTERP path without the INTERP header are a
/// refused dispatch and an unknown-role candidate, not an INTERP decode.
#[test]
fn accept_f13_a_header_mismatch_is_an_unknown_candidate() {
    let bytes = b"not an interp container".to_vec();
    let path = path("ZBD/interp.zbd");
    let inventory = inventory_scripts(&[ScriptSource::new("synthetic", &path, &bytes)]);
    let entry = &inventory.entries()[0];
    assert_eq!(
        entry.dispatch(),
        DispatchOutcome::Refused {
            code: "header_mismatch"
        }
    );
    assert_eq!(
        entry.candidacy(),
        Candidacy::Candidate {
            role: ScriptRole::Unknown,
            confidence: Confidence::Unknown
        }
    );
    assert_eq!(entry.records().len(), 1);
    assert_eq!(entry.records()[0].kind(), RecordKind::Opaque);
    assert_covers_container(entry);
}

/// A scan is not a decoder: scan or inferred evidence cannot mark a record
/// as instructions, structure cannot be claimed, and evidence must lie inside
/// the record of the same container. Only then does the claim stand.
#[test]
fn accept_f13_a_only_semantic_evidence_establishes_instructions() {
    let bytes = interp(&[(b"alpha", &[(1, b"go\0".as_slice())])], b"");
    let path = path("ZBD/interp.zbd");
    let mut inventory = inventory_scripts(&[ScriptSource::new("synthetic", &path, &bytes)]);
    let records = inventory.entries_mut()[0].records_mut();
    let body = records
        .iter()
        .position(|r| matches!(r.kind(), RecordKind::InterpScript { .. }))
        .expect("one script body");
    let body_span = records[body].span();

    assert_eq!(
        records[body]
            .establish_instructions(scan_evidence("synthetic", body_span))
            .map_err(|e| e.code()),
        Err("insufficient_evidence")
    );
    assert_eq!(
        records[body]
            .establish_instructions(decode_evidence(
                "synthetic",
                body_span,
                Confidence::Inferred
            ))
            .map_err(|e| e.code()),
        Err("insufficient_evidence")
    );
    let header_span = records[0].span();
    assert_eq!(
        records[0].establish_instructions(decode_evidence(
            "synthetic",
            header_span,
            Confidence::ObservedTool
        )),
        Err(InstructionClaimError::ContainerStructure { span: header_span })
    );
    let outside = ByteSpan::new(0, body_span.end());
    assert_eq!(
        records[body]
            .establish_instructions(decode_evidence(
                "synthetic",
                outside,
                Confidence::ObservedTool
            ))
            .map_err(|e| e.code()),
        Err("outside_record")
    );
    assert_eq!(
        records[body].establish_instructions(decode_evidence(
            "another",
            body_span,
            Confidence::ObservedTool
        )),
        Err(InstructionClaimError::OtherContainer {
            record: "synthetic".to_owned(),
            evidence: "another".to_owned(),
        })
    );
    assert!(matches!(
        records[body].instructions(),
        InstructionStatus::Unestablished { .. }
    ));
    assert_eq!(inventory.stats().established, 0);

    let records = inventory.entries_mut()[0].records_mut();
    records[body]
        .establish_instructions(decode_evidence(
            "synthetic",
            body_span,
            Confidence::ObservedTool,
        ))
        .expect("observed structural evidence inside the body establishes it");
    assert_eq!(inventory.stats().established, 1);
}

/// The evidence schema records method, place and summary — never content:
/// methods and locators must agree, confidence is capped by method and a note
/// cannot carry a listing.
#[test]
fn accept_f13_a_evidence_schema_is_disassembly_neutral() {
    let executable = EvidenceLocator::Executable {
        module: "game.exe".to_owned(),
        rva: 0x1234,
    };
    let static_analysis = ScriptEvidence::new(
        ResearchMethod::ExecutableStaticAnalysis,
        Confidence::ObservedTool,
        executable.clone(),
        "dispatch routine indexes a 4-byte opcode table",
    )
    .expect("an address and a summary are valid evidence");
    assert_eq!(static_analysis.locator(), &executable);

    let span = EvidenceLocator::ContainerSpan {
        container: "synthetic".to_owned(),
        span: ByteSpan::new(0, 4),
    };
    assert_eq!(
        ScriptEvidence::new(
            ResearchMethod::ExecutableStaticAnalysis,
            Confidence::Inferred,
            span.clone(),
            "summary",
        ),
        Err(EvidenceError::LocatorMismatch {
            method: ResearchMethod::ExecutableStaticAnalysis,
            expected: LocatorKind::Executable,
            found: LocatorKind::ContainerSpan,
        })
    );
    assert_eq!(
        ScriptEvidence::new(
            ResearchMethod::ContainerScan,
            Confidence::Inferred,
            span.clone(),
            "summary",
        ),
        Err(EvidenceError::Overclaim {
            method: ResearchMethod::ContainerScan,
            requested: Confidence::Inferred,
            ceiling: Confidence::Lead,
        })
    );
    assert_eq!(
        ScriptEvidence::new(
            ResearchMethod::DocumentReview,
            Confidence::ObservedTool,
            EvidenceLocator::Document {
                citation: "S07".to_owned()
            },
            "summary",
        )
        .map_err(|e| e.code()),
        Err("overclaim")
    );
    assert_eq!(
        ScriptEvidence::new(
            ResearchMethod::OriginalRuntimeObservation,
            Confidence::ObservedTool,
            executable.clone(),
            "summary",
        )
        .map_err(|e| e.code()),
        Err("locator_mismatch")
    );
    let listing = "mov eax, [ecx+4]\n".repeat(20);
    assert!(listing.len() > MAX_NOTE_BYTES);
    assert_eq!(
        ScriptEvidence::new(
            ResearchMethod::ExecutableStaticAnalysis,
            Confidence::Inferred,
            executable.clone(),
            listing.clone(),
        ),
        Err(EvidenceError::NoteTooLong { len: listing.len() })
    );
    assert_eq!(
        ScriptEvidence::new(
            ResearchMethod::ExecutableStaticAnalysis,
            Confidence::Inferred,
            executable,
            "  ",
        ),
        Err(EvidenceError::EmptyNote)
    );
    let capture = ScriptEvidence::new(
        ResearchMethod::OriginalRuntimeObservation,
        Confidence::ObservedTool,
        EvidenceLocator::RuntimeCapture {
            capture: "REF-EXAMPLE".to_owned(),
            tick: Some(120),
        },
        "timer fired before any kill",
    )
    .expect("a capture position and a summary are valid evidence");
    assert!(capture.establishes_semantics());
    assert!(!scan_evidence("synthetic", ByteSpan::new(0, 4)).establishes_semantics());
}

/// String leads are capped per record and the overflow is counted in a
/// finding, so a large opaque record neither floods the report nor hides
/// that it had more leads.
#[test]
fn accept_f13_a_string_leads_are_capped_and_counted() {
    let total = MAX_LEADS_PER_RECORD + 44;
    let bytes = b"abcd\0".repeat(total);
    let path = path("zbd/zrdr.zbd");
    let inventory = inventory_scripts(&[ScriptSource::new("synthetic", &path, &bytes)]);
    let entry = &inventory.entries()[0];
    let record = &entry.records()[0];
    assert_eq!(record.references().len(), MAX_LEADS_PER_RECORD);
    assert_eq!(record.references()[1].span, ByteSpan::new(5, 4));
    assert_eq!(
        entry.findings(),
        [InventoryFinding::LeadsTruncated {
            record: ByteSpan::new(0, bytes.len() as u64),
            kept: MAX_LEADS_PER_RECORD,
            found: total,
        }]
    );
    // A run shorter than the lead minimum is not a lead.
    let short = b"abc\0xy".to_vec();
    let inventory = inventory_scripts(&[ScriptSource::new("synthetic", &path, &short)]);
    assert!(inventory.entries()[0].records()[0].references().is_empty());
}

/// The inventory is ordered by case-insensitive logical path, independent
/// of the order containers were found in.
#[test]
fn accept_f13_a_inventory_order_is_stable() {
    let (a, b, c) = (
        path("ZBD/C2/zrdr.zbd"),
        path("zbd/c1/ZRDR.zbd"),
        path("ZBD/C1/cam_anim.zbd"),
    );
    let bytes = [0u8; 8];
    let anim = animation(b"");
    let sources = [
        ScriptSource::new("a", &a, &bytes),
        ScriptSource::new("b", &b, &bytes),
        ScriptSource::new("c", &c, &anim),
    ];
    let forward = inventory_scripts(&sources);
    let mut reversed_sources = sources;
    reversed_sources.reverse();
    let reversed = inventory_scripts(&reversed_sources);
    assert_eq!(forward, reversed);
    let order: Vec<&str> = forward.entries().iter().map(|e| e.container()).collect();
    assert_eq!(order, ["c", "b", "a"]);
}

// ---------------------------------------------------------------------------
// Stage F13-B: locate and classify loading, mission and animation programs
// ---------------------------------------------------------------------------

/// Builds a reader-family archive (F06/indexed by a version-one trailer) whose
/// members are `(name, body)` in declaration order: member data, then one
/// 148-byte index entry per member, then the version-one trailer.
fn reader_archive(members: &[(&[u8], &[u8])]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut extents = Vec::new();
    for (name, body) in members {
        extents.push((bytes.len() as u32, body.len() as u32, *name));
        bytes.extend_from_slice(body);
    }
    for (start, length, name) in extents {
        bytes.extend_from_slice(&start.to_le_bytes());
        bytes.extend_from_slice(&length.to_le_bytes());
        let mut field = [0u8; 64];
        field[..name.len()].copy_from_slice(name);
        bytes.extend_from_slice(&field);
        bytes.extend_from_slice(&[0u8; 76]);
    }
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&(members.len() as u32).to_le_bytes());
    bytes
}

/// The absolute path of the read-only installation, or a loud failure when
/// the `retail` capability is missing.
fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set to run retail tests"),
    )
}

/// The F13-B minimum scenario: an opcode the ledger does not name, reached at a
/// program counter of a located mission program, fails with the mission and the
/// program's source location.
#[test]
fn accept_f13_b_unknown_opcode_at_reached_pc_fails_with_mission_and_location() {
    let member = b"\x0a\x00\x00\x00\x0b\x00\x00\x00";
    let bytes = reader_archive(&[(b"objectives.zrd", member)]);
    let container = path("zbd/c1/m02/zrdr.zbd");
    let discovery = discover_container("synthetic", &container, &bytes);
    assert_eq!(discovery.family(), Some(ZbdFamily::Reader));
    assert_eq!(discovery.len(), 1, "one member is one program");
    let program = &discovery.programs()[0];
    assert_eq!(program.kind(), ProgramKind::Mission);
    assert_eq!(program.confidence(), Confidence::Inferred);
    assert_eq!(program.mission(), Some("zbd/c1/m02"));
    assert_eq!(program.mission_label(), "zbd/c1/m02");
    assert_eq!(program.container(), "synthetic");
    assert_eq!(program.locator().member(), Some("objectives.zrd"));
    assert_eq!(
        program.locator().span(),
        ByteSpan::new(0, member.len() as u64)
    );
    assert_eq!(program.bytes(), member);

    let error = program
        .walk(&OpcodeLedger::new(), 4, 4096)
        .expect_err("the empty ledger refuses the first reached opcode");
    assert_eq!(error.code(), "unknown_opcode");
    assert_eq!(error.mission(), Some("zbd/c1/m02"));
    assert_eq!(error.locator(), Some(program.locator()));
    let text = error.to_string();
    assert!(text.contains("mission `zbd/c1/m02`"), "{text}");
    assert!(text.contains("objectives.zrd"), "{text}");
    match error {
        ProgramError::UnknownOpcode {
            mission,
            locator,
            pc,
            opcode,
        } => {
            assert_eq!(mission, "zbd/c1/m02");
            assert_eq!(&locator, program.locator());
            assert_eq!(pc, ByteSpan::new(0, 4));
            assert_eq!(opcode, 0x0000_000a);
        }
        other => panic!("expected unknown_opcode, got {other:?}"),
    }
}

/// The ledger is consulted at every reached program counter and fails closed:
/// removing a naming entry makes the walk stop at the next counter, and an
/// entry that says nothing or repeats a value is refused.
#[test]
fn accept_f13_b_ledger_names_reached_opcodes_and_fails_closed() {
    let locator = ProgramLocator::new("synthetic", Some("x.zrd".to_owned()), ByteSpan::new(0, 8));
    let one = [1u8, 0, 0, 0];
    let two = [1u8, 0, 0, 0, 2, 0, 0, 0];
    let mut ledger = OpcodeLedger::new();
    assert!(ledger.is_empty());
    ledger
        .insert(
            OpcodeEntry::new(
                1,
                "first",
                ProgramKind::Loading,
                Confidence::Inferred,
                "synthetic probe",
            )
            .expect("a complete entry is accepted"),
        )
        .expect("the first entry of an opcode is accepted");

    let reached =
        walk_program("m", &locator, &one, 4, &ledger, 16).expect("the known opcode is walked");
    assert_eq!(reached.len(), 1, "only the named opcode is reached");
    assert_eq!(reached[0].opcode, 1);
    assert_eq!(reached[0].pc, ByteSpan::new(0, 4));
    let error = walk_program("m", &locator, &two, 4, &ledger, 16)
        .expect_err("the second opcode is not named");
    assert_eq!(error.code(), "unknown_opcode");
    match error {
        ProgramError::UnknownOpcode { pc, opcode, .. } => {
            assert_eq!(pc, ByteSpan::new(4, 4));
            assert_eq!(opcode, 2);
        }
        other => panic!("expected unknown_opcode, got {other:?}"),
    }

    // An entry that names nothing is not a claim.
    assert_eq!(
        OpcodeEntry::new(7, "  ", ProgramKind::Mission, Confidence::Inferred, "probe")
            .map_err(|error| error.code()),
        Err("empty_spelling")
    );
    assert_eq!(
        OpcodeEntry::new(7, "x", ProgramKind::Mission, Confidence::Inferred, "")
            .map_err(|error| error.code()),
        Err("empty_source")
    );
    assert_eq!(
        OpcodeEntry::new(7, "x", ProgramKind::Mission, Confidence::Unknown, "probe")
            .map_err(|error| error.code()),
        Err("unknown_confidence")
    );
    // One opcode value with two meanings is ambiguous, not overwritten.
    assert_eq!(
        ledger
            .insert(
                OpcodeEntry::new(
                    1,
                    "other",
                    ProgramKind::Mission,
                    Confidence::Inferred,
                    "probe"
                )
                .expect("the entry itself is complete")
            )
            .map_err(|error| error.code()),
        Err("duplicate_opcode")
    );
    assert_eq!(ledger.lookup(1).map(OpcodeEntry::spelling), Some("first"));
    assert_eq!(ledger.entries().count(), 1);
}

/// The walk refuses empty bytes, a word width outside the measured range, a
/// trailing partial word and an unbounded program.
#[test]
fn accept_f13_b_walk_refuses_empty_truncated_and_unbounded_programs() {
    let locator = ProgramLocator::new("synthetic", None, ByteSpan::new(0, 0));
    let empty = walk_program("m", &locator, &[], 4, &OpcodeLedger::new(), 8);
    assert_eq!(empty.map_err(|error| error.code()), Err("empty_program"));
    assert_eq!(
        walk_program("m", &locator, &[1, 0, 0, 0], 0, &OpcodeLedger::new(), 8)
            .map_err(|error| error.code()),
        Err("invalid_word_width")
    );
    assert_eq!(
        walk_program("m", &locator, &[1, 0, 0, 0], 8, &OpcodeLedger::new(), 8)
            .map_err(|error| error.code()),
        Err("invalid_word_width")
    );
    assert_eq!(
        walk_program("m", &locator, &[1, 2], 4, &OpcodeLedger::new(), 8)
            .map_err(|error| error.code()),
        Err("truncated_opcode")
    );

    // A ledger that names every value still stops at its instruction budget.
    let mut ledger = OpcodeLedger::new();
    ledger
        .insert(
            OpcodeEntry::new(
                1,
                "loop",
                ProgramKind::Loading,
                Confidence::Inferred,
                "probe",
            )
            .expect("complete"),
        )
        .expect("first");
    let error = walk_program("m", &locator, &[1, 0, 0, 0, 1, 0, 0, 0], 4, &ledger, 1)
        .expect_err("the budget bounds the walk");
    assert_eq!(error.code(), "budget_exceeded");
    // The budget caps decoded instructions, so zero decodes none of a
    // non-empty program instead of accepting its first word.
    let none = walk_program("m", &locator, &[1, 0, 0, 0], 4, &ledger, 0)
        .expect_err("a zero budget bounds the walk before the first word");
    assert_eq!(none.code(), "budget_exceeded");
}

/// A mission directory scopes a program; a group or content-root archive does
/// not, and the key is case-insensitive.
#[test]
fn accept_f13_b_mission_scope_is_the_mission_directory() {
    assert_eq!(
        mission_scope(&path("zbd/c1/m02/zrdr.zbd")).as_deref(),
        Some("zbd/c1/m02")
    );
    assert_eq!(
        mission_scope(&path("ZBD/C1/M02/ZRDR.ZBD")).as_deref(),
        Some("zbd/c1/m02")
    );
    assert_eq!(mission_scope(&path("zbd/c1/zrdr.zbd")), None);
    assert_eq!(mission_scope(&path("zbd/interp.zbd")), None);
    assert_eq!(mission_scope(&path("zbd/c1/m02/deeper/zrdr.zbd")), None);
}

/// INTERP script bodies are documented loading programs; animation containers
/// are inferred from their names and located after the validated header.
#[test]
fn accept_f13_b_classifies_loading_and_animation_programs() {
    let loading = interp(&[(b"boot", &[(1, b"go\0".as_slice())])], b"");
    let loading_path = path("zbd/interp.zbd");
    let discovery = discover_container("loading", &loading_path, &loading);
    assert_eq!(discovery.family(), Some(ZbdFamily::Interp));
    assert_eq!(discovery.len(), 1);
    let program = &discovery.programs()[0];
    assert_eq!(program.kind(), ProgramKind::Loading);
    assert_eq!(program.confidence(), Confidence::Documented);
    assert_eq!(program.mission(), None);
    assert_eq!(program.mission_label(), "loading");
    let body_start = (INTERP_HEADER_BYTES + INDEX_ENTRY_BYTES) as u64;
    assert_eq!(program.locator().span(), ByteSpan::new(body_start, 15));
    assert_eq!(program.bytes(), &loading[body_start as usize..]);
    // A loading program takes the same fail-closed walk as any other.
    assert_eq!(
        program
            .walk(&OpcodeLedger::new(), 4, 64)
            .map_err(|e| e.code()),
        Err("unknown_opcode")
    );

    let camera = animation(b"\x11\x22\x33\x44");
    let camera_path = path("zbd/c1/cam_anim.zbd");
    let discovery = discover_container("camera", &camera_path, &camera);
    assert_eq!(discovery.family(), Some(ZbdFamily::Animation));
    assert_eq!(discovery.len(), 1);
    let program = &discovery.programs()[0];
    assert_eq!(program.kind(), ProgramKind::CameraAnimation);
    assert_eq!(program.confidence(), Confidence::Inferred);
    assert_eq!(program.mission(), None);
    assert_eq!(program.locator().span(), ByteSpan::new(8, 4));
    assert_eq!(program.bytes(), b"\x11\x22\x33\x44");

    let mission = animation(b"\x55\x66\x77\x88");
    let mission_path = path("zbd/c1/m02/mis_anim.zbd");
    let discovery = discover_container("mission-anim", &mission_path, &mission);
    let program = &discovery.programs()[0];
    assert_eq!(program.kind(), ProgramKind::MissionAnimation);
    assert_eq!(program.confidence(), Confidence::Inferred);
    assert_eq!(program.mission(), Some("zbd/c1/m02"));
    assert_eq!(program.bytes(), b"\x55\x66\x77\x88");
    let error = program
        .walk(&OpcodeLedger::new(), 4, 64)
        .expect_err("empty ledger");
    assert_eq!(error.code(), "unknown_opcode");
    assert_eq!(error.mission(), Some("zbd/c1/m02"));
}

/// Reader members are classified by their observed names and, when no name
/// names a role, by the mission the path scopes them to.
#[test]
fn accept_f13_b_reader_members_are_classified_by_name_and_mission_scope() {
    let members: &[(&[u8], &[u8])] = &[
        (b"aiv.zrd", b"\x01\x00\x00\x00"),
        (b"mis_anim.zrd", b"\x02\x00\x00\x00"),
        (b"cam_anim.zrd", b"\x03\x00\x00\x00"),
        (b"scene.zrd", b"\x04\x00\x00\x00"),
    ];
    let bytes = reader_archive(members);
    let container = path("ZBD/C1/M02/zrdr.zbd");
    let discovery = discover_container("mission", &container, &bytes);
    let kinds: Vec<(Option<&str>, ProgramKind, Confidence)> = discovery
        .programs()
        .iter()
        .map(|program| {
            (
                program.locator().member(),
                program.kind(),
                program.confidence(),
            )
        })
        .collect();
    assert_eq!(
        kinds,
        [
            (Some("aiv.zrd"), ProgramKind::Mission, Confidence::Inferred),
            (
                Some("mis_anim.zrd"),
                ProgramKind::MissionAnimation,
                Confidence::Inferred
            ),
            (
                Some("cam_anim.zrd"),
                ProgramKind::CameraAnimation,
                Confidence::Inferred
            ),
            (
                Some("scene.zrd"),
                ProgramKind::Mission,
                Confidence::Inferred
            ),
        ]
    );
    assert!(discovery.findings().is_empty());
    // Every member is located at its own byte range; no bytes are copied.
    assert_eq!(
        discovery.programs()[0].locator().span(),
        ByteSpan::new(0, 4)
    );
    assert_eq!(
        discovery.programs()[3].locator().span(),
        ByteSpan::new(12, 4)
    );

    // A group archive has no mission: its unnamed members stay reader entries.
    let group = reader_archive(&[(b"templates.zrd", b"\x09\x00\x00\x00")]);
    let group_path = path("zbd/c1/zrdr.zbd");
    let discovery = discover_container("group", &group_path, &group);
    assert_eq!(discovery.programs()[0].mission(), None);
    assert_eq!(discovery.programs()[0].kind(), ProgramKind::ReaderEntry);
    assert_eq!(discovery.programs()[0].confidence(), Confidence::Inferred);
}

/// A refused dispatch, an excluded family and a refused member index are kept
/// as findings with no guessed program.
#[test]
fn accept_f13_b_refusals_stay_visible_without_guessing_programs() {
    let texture = vec![0u8; 16];
    let texture_path = path("zbd/c1/texture.zbd");
    let discovery = discover_container("texture", &texture_path, &texture);
    assert_eq!(discovery.family(), Some(ZbdFamily::Texture));
    assert!(discovery.is_empty());
    assert!(matches!(
        discovery.findings(),
        [DiscoveryFinding::Excluded {
            family: ZbdFamily::Texture,
            ..
        }]
    ));

    let unknown = b"????unknown-bytes".to_vec();
    let unknown_path = path("zbd/c1/strange.zbd");
    let discovery = discover_container("unknown", &unknown_path, &unknown);
    assert_eq!(discovery.family(), None);
    assert_eq!(
        discovery.findings(),
        [DiscoveryFinding::DispatchRefused {
            code: "unknown_family"
        }]
    );
    assert!(discovery.is_empty());

    // A reader archive whose trailer is not a version-one index is refused,
    // and the refusal is a finding rather than a guessed program.
    let broken = b"\x00\x00\x00\x00BADTRAILER".to_vec();
    let broken_path = path("zbd/c1/zrdr.zbd");
    let discovery = discover_container("broken", &broken_path, &broken);
    assert_eq!(discovery.family(), Some(ZbdFamily::Reader));
    assert!(
        discovery
            .findings()
            .iter()
            .any(|finding| finding.code() == "index_refused")
    );
    assert!(discovery.is_empty());
}

/// AC02 over the original installation: every campaign program is located, the
/// mission program's empty-ledger walk fails with its mission and source
/// location, and no role is claimed beyond the name/path rules.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f13_b_retail_locates_mission_programs() {
    let root = game_dir();

    // The loading container: documented loading programs.
    let interp_bytes = fs::read(root.join("ZBD/interp.zbd")).expect("the retail interp container");
    let interp_path = path("zbd/interp.zbd");
    let loading = discover_container("retail-interp", &interp_path, &interp_bytes);
    assert_eq!(loading.family(), Some(ZbdFamily::Interp));
    assert!(!loading.is_empty());
    assert!(loading.programs().iter().all(|program| {
        program.kind() == ProgramKind::Loading && program.confidence() == Confidence::Documented
    }));
    assert!(
        loading
            .programs()
            .iter()
            .all(|program| program.mission().is_none())
    );

    // A mission reader archive: the mission is the path, the role is inferred.
    let zrdr_path = root.join("ZBD/C1/M02/zrdr.zbd");
    let zrdr_bytes = fs::read(&zrdr_path).expect("the retail mission reader archive");
    let container = path("zbd/c1/m02/zrdr.zbd");
    let discovery = discover_container("retail-zrdr", &container, &zrdr_bytes);
    assert_eq!(discovery.family(), Some(ZbdFamily::Reader));
    assert!(
        discovery.findings().is_empty(),
        "{:?}",
        discovery.findings()
    );
    let objectives = discovery
        .programs()
        .iter()
        .find(|program| program.locator().member() == Some("objectives.zrd"))
        .expect("objectives.zrd is a located program");
    assert_eq!(objectives.kind(), ProgramKind::Mission);
    assert_eq!(objectives.mission(), Some("zbd/c1/m02"));
    assert!(!objectives.bytes().is_empty());

    // The F13-B minimum scenario on original bytes.
    let error = objectives
        .walk(&OpcodeLedger::new(), 4, 4096)
        .expect_err("the empty ledger refuses the first reached opcode");
    assert_eq!(error.code(), "unknown_opcode");
    assert_eq!(error.mission(), Some("zbd/c1/m02"));
    assert_eq!(error.locator(), Some(objectives.locator()));
    match &error {
        ProgramError::UnknownOpcode { locator, pc, .. } => {
            assert_eq!(locator, objectives.locator());
            assert_eq!(*pc, ByteSpan::new(0, 4));
        }
        other => panic!("expected unknown_opcode, got {other:?}"),
    }

    // The mission and camera animation containers are located by name too.
    let mis_bytes = fs::read(root.join("ZBD/C1/M02/mis_anim.zbd")).expect("mis_anim.zbd");
    let mis_path = path("zbd/c1/m02/mis_anim.zbd");
    let discovery = discover_container("retail-mis-anim", &mis_path, &mis_bytes);
    assert_eq!(discovery.len(), 1);
    assert_eq!(
        discovery.programs()[0].kind(),
        ProgramKind::MissionAnimation
    );
    assert_eq!(discovery.programs()[0].confidence(), Confidence::Inferred);
    assert_eq!(discovery.programs()[0].mission(), Some("zbd/c1/m02"));

    let cam_bytes = fs::read(root.join("ZBD/C1/cam_anim.zbd")).expect("cam_anim.zbd");
    let cam_path = path("zbd/c1/cam_anim.zbd");
    let discovery = discover_container("retail-cam-anim", &cam_path, &cam_bytes);
    assert_eq!(discovery.len(), 1);
    assert_eq!(discovery.programs()[0].kind(), ProgramKind::CameraAnimation);
    assert_eq!(discovery.programs()[0].mission(), None);
}

// --- F13-C: isolated signature probes and record reachability ----------------
//
// Acceptance stage F13-C (`specs/F13-mission-language-discovery-and-
// compatibility-closure.md`, section `### F13-C`, AC03: "An unused unknown
// record remains visible in the report, with reachability evidence").
// The bytes are still authored here; nothing in this section reads
// `CS_GAME_DIR` except the single `#[ignore]`d retail test.

/// Document-backed evidence for a signature claim: a cited findings note is
/// the only thing a `--signatures` file can honestly carry.
fn cited_evidence(citation: &str) -> ScriptEvidence {
    ScriptEvidence::new(
        ResearchMethod::DocumentReview,
        Confidence::Documented,
        EvidenceLocator::Document {
            citation: citation.to_owned(),
        },
        "an isolated probe measured this signature and recorded it in a cited note",
    )
    .expect("document evidence is valid")
}

/// A complete, measured signature claim over the authored fixtures.
fn measured_claim(opcode: u32, spelling: &str, program: ProgramKind) -> SignatureClaim {
    SignatureClaim::new(
        opcode,
        spelling,
        program,
        0,
        SignatureShape::new(
            "u32 -> unit",
            "records the fixture value",
            "immediate",
            "never",
        ),
        cited_evidence("docs/findings/2026-09-29-f13-c-signature-probes-and-reachability.md"),
    )
    .expect("a complete claim is accepted")
}

/// AC03: the unclaimed tail of a container no program covers stays in the
/// inventory and in the report, carrying the reachability evidence that says
/// nothing reaches it.
#[test]
fn accept_f13_c_unused_unknown_record_stays_visible_with_reachability_evidence() {
    let loading = interp(
        &[(b"boot", &[(1, b"go\0".as_slice())])],
        b"\x01\x02JUNKDATA\x03",
    );
    let container = path("zbd/interp.zbd");
    let discovery = discover_container("synthetic", &container, &loading);
    assert_eq!(discovery.family(), Some(ZbdFamily::Interp));
    assert_eq!(discovery.len(), 1, "one loading program is located");

    let sources = [ScriptSource::new("synthetic", &container, &loading)];
    let mut inventory = inventory_scripts(&sources);
    let entry = inventory
        .entries_mut()
        .first_mut()
        .expect("one source yields one entry");

    // "Unused" is a measured verdict, never a default for an unprobed record.
    assert!(
        entry
            .records()
            .iter()
            .all(|record| record.reachability().is_none())
    );
    assert!(
        entry
            .records()
            .iter()
            .all(|record| !record.is_unused_unknown())
    );

    let stats = probe_records(entry, &discovery).expect("the same container");
    // header, one index entry, one script body, one unclaimed tail.
    assert_eq!(stats.records, 4);
    assert_eq!(stats.used, 1, "only the script body overlaps a program");
    assert_eq!(stats.unused, 3);
    assert_eq!(stats.unused_unknown, 1, "the AC03 case");
    assert_eq!(
        entry.records().len(),
        4,
        "the resolution pass drops no record"
    );

    let tail = entry
        .records()
        .iter()
        .find(|record| record.kind() == RecordKind::Unclaimed)
        .expect("the unclaimed tail is still listed");
    assert!(tail.is_unused_unknown());
    assert_eq!(
        tail.reachability(),
        Some(RecordReachability::Unused {
            reason: UNREACHED_UNUSED_REASON
        })
    );
    let evidence = tail
        .reachability_evidence()
        .expect("reachability evidence accompanies the verdict");
    assert_eq!(evidence.method(), ResearchMethod::StructuralDecode);
    assert_eq!(evidence.confidence(), Confidence::ObservedTool);
    assert_eq!(
        evidence.locator(),
        &EvidenceLocator::ContainerSpan {
            container: "synthetic".to_owned(),
            span: tail.span(),
        }
    );
    assert_eq!(evidence.note(), UNREACHED_UNUSED_REASON);
    assert!(
        evidence.establishes_semantics(),
        "reachability is measured, not scanned"
    );
    // The string lead F13-A recorded is still there: nothing was erased.
    assert!(!tail.references().is_empty());

    let body = entry
        .records()
        .iter()
        .find(|record| record.kind() == RecordKind::InterpScript { position: 0 })
        .expect("the script body is listed");
    assert_eq!(
        body.reachability(),
        Some(RecordReachability::Used { programs: 1 })
    );
    assert_eq!(
        body.reachability_evidence().expect("evidence").note(),
        "1 located program(s) overlap this record"
    );
    assert!(!body.is_unused_unknown());

    for record in entry.records() {
        let structural = record.kind() == RecordKind::InterpHeader
            || matches!(record.kind(), RecordKind::InterpIndexEntry { .. });
        if !structural {
            continue;
        }
        assert_eq!(
            record.reachability(),
            Some(RecordReachability::Unused {
                reason: STRUCTURE_UNUSED_REASON
            })
        );
        assert!(!record.is_unused_unknown());
    }

    // A discovery of another container is refused instead of joined wrongly,
    // and the refusal leaves the measured verdicts alone.
    let other_path = path("zbd/c1/m02/zrdr.zbd");
    let other_bytes = reader_archive(&[(b"aiv.zrd", b"\x01\x00\x00\x00")]);
    let other = discover_container("other", &other_path, &other_bytes);
    let error = probe_records(entry, &other).expect_err("a mismatched join is refused");
    assert_eq!(error.code(), "path_mismatch");
    assert!(error.to_string().contains("zbd/interp.zbd"));
    assert_eq!(
        entry
            .records()
            .iter()
            .filter(|r| r.reachability().is_some())
            .count(),
        4,
        "the refused probe changed nothing"
    );
}

/// One program at a time: a measured signature moves the stop forward, a
/// second one resolves the program, and the stop is recorded as data.
#[test]
fn accept_f13_c_signature_claims_resolve_a_program_in_isolation() {
    let member = b"\x0a\x00\x00\x00\x0b\x00\x00\x00";
    let bytes = reader_archive(&[(b"objectives.zrd", member)]);
    let container = path("zbd/c1/m02/zrdr.zbd");
    let discovery = discover_container("synthetic", &container, &bytes);
    let program = &discovery.programs()[0];

    let config = ProbeConfig::new(4, 64).expect("a valid instruction unit");
    assert_eq!(config.word_bytes(), 4);
    assert_eq!(config.budget(), 64);
    let mut session = ProbeSession::new(SignatureTable::new(), config);

    let index = session.probe(program).expect("the session runs probes");
    let probe = &session.probes()[index];
    assert_eq!(probe.mission(), "zbd/c1/m02");
    assert_eq!(probe.kind(), ProgramKind::Mission);
    assert_eq!(probe.confidence(), Confidence::Inferred);
    assert_eq!(probe.attempts(), 1);
    assert!(!probe.resolved());
    assert!(probe.retryable());
    assert_eq!(probe.code(), "unknown_opcode");
    assert_eq!(
        probe.first_unknown(),
        Some((ByteSpan::new(0, 4), 0x0000_000a))
    );
    assert!(
        probe.reached().is_empty(),
        "a stopped walk reports no partial progress"
    );
    assert_eq!(
        probe.stop().and_then(ProgramError::mission),
        Some("zbd/c1/m02")
    );

    // One measured signature moves the stop exactly one instruction forward.
    session
        .extend(measured_claim(0x0a, "first", ProgramKind::Mission))
        .expect("the claim is measured");
    session
        .retry(index, program)
        .expect("an unknown opcode is the retryable stop");
    let probe = &session.probes()[index];
    assert_eq!(probe.attempts(), 2);
    assert_eq!(
        probe.first_unknown(),
        Some((ByteSpan::new(4, 4), 0x0000_000b))
    );

    // The second signature resolves the whole program.
    session
        .extend(measured_claim(0x0b, "second", ProgramKind::Mission))
        .expect("the claim is measured");
    session
        .retry(index, program)
        .expect("the last unknown opcode is retryable");
    let probe = &session.probes()[index];
    assert!(probe.resolved());
    assert_eq!(probe.code(), "resolved");
    assert_eq!(probe.attempts(), 3);
    assert_eq!(probe.reached().len(), 2);
    assert_eq!(
        probe
            .reached()
            .iter()
            .map(|reached| reached.opcode)
            .collect::<Vec<_>>(),
        [0x0a, 0x0b]
    );
    assert_eq!(probe.reached()[1].pc, ByteSpan::new(4, 4));

    // A resolved program has nothing left to fix, so it is not re-run.
    assert_eq!(
        session.retry(index, program).map_err(|error| error.code()),
        Err("not_retryable")
    );

    let report = session.teardown();
    assert!(session.is_closed());
    assert_eq!(report.len(), 1);
    assert_eq!(report.resolved(), 1);
    assert_eq!(report.unresolved(), 0);
    assert_eq!(report.retryable(), 0);
    assert!(report.complete());
    assert_eq!(report.table().len(), 2);
    assert_eq!(report.config(), config);
    let ledger = report.table().ledger();
    assert_eq!(
        ledger.lookup(0x0a).map(OpcodeEntry::spelling),
        Some("first")
    );
    assert_eq!(
        ledger.lookup(0x0b).map(OpcodeEntry::spelling),
        Some("second")
    );

    // Teardown is a real boundary: the closed session refuses more work
    // instead of continuing against a state nobody will read.
    assert_eq!(
        session.probe(program).map_err(|error| error.code()),
        Err("session_closed")
    );
    assert_eq!(
        session
            .extend(measured_claim(0x0c, "third", ProgramKind::Mission))
            .map_err(|error| error.code()),
        Err("session_closed")
    );
    assert_eq!(
        session.retry(index, program).map_err(|error| error.code()),
        Err("session_closed")
    );
    assert!(session.is_closed());
}

/// Every refusal the probe layer has to propagate: a claim that says nothing
/// or cites a scan, an ambiguous opcode, a structural stop and a program that
/// is not the one that was probed. Two programs are probed even though the
/// first one stops, so one failure never aborts the run.
#[test]
fn accept_f13_c_probe_session_refuses_weak_claims_and_structural_stops() {
    assert_eq!(
        ProbeConfig::new(0, 8).map_err(|error| error.code()),
        Err("invalid_word_width")
    );
    assert_eq!(
        ProbeConfig::new(5, 8).map_err(|error| error.code()),
        Err("invalid_word_width")
    );
    // A zero budget is allowed: the walk refuses the program at once.
    assert!(ProbeConfig::new(4, 0).is_ok());

    let shape = || SignatureShape::new("u32 -> unit", "effect", "immediate", "never");
    assert_eq!(
        SignatureClaim::new(
            1,
            "  ",
            ProgramKind::Mission,
            0,
            shape(),
            cited_evidence("docs/findings/2026-09-29-f13-c-signature-probes-and-reachability.md"),
        )
        .map_err(|error| error.code()),
        Err("empty_field")
    );
    assert_eq!(
        SignatureClaim::new(
            1,
            "spelling",
            ProgramKind::Mission,
            0,
            SignatureShape::new("", "effect", "immediate", "never"),
            cited_evidence("docs/findings/2026-09-29-f13-c-signature-probes-and-reachability.md"),
        )
        .map_err(|error| error.code()),
        Err("empty_field")
    );
    // A scan lead never resolves a signature: it cannot establish meaning.
    assert_eq!(
        SignatureClaim::new(
            1,
            "spelling",
            ProgramKind::Mission,
            0,
            shape(),
            scan_evidence("synthetic", ByteSpan::new(0, 4)),
        )
        .map_err(|error| error.code()),
        Err("weak_evidence")
    );

    let mut table = SignatureTable::new();
    assert!(table.is_empty());
    table
        .insert(measured_claim(1, "first", ProgramKind::Mission))
        .expect("the first claim is accepted");
    assert_eq!(
        table
            .insert(measured_claim(1, "other", ProgramKind::Mission))
            .map_err(|error| error.code()),
        Err("duplicate_opcode"),
        "one opcode value with two signatures is ambiguous"
    );
    assert_eq!(table.len(), 1);
    assert!(table.get(1).is_some());
    assert!(table.get(2).is_none());

    // A program whose bytes are not a whole instruction stops structurally:
    // it is reported, and more evidence cannot make it retryable.
    let truncated_bytes = reader_archive(&[(b"a.zrd", b"\x01\x02\x03")]);
    let truncated_path = path("zbd/c1/m02/zrdr.zbd");
    let truncated = discover_container("truncated", &truncated_path, &truncated_bytes);
    // A second, independent container: probing it must not disturb the first.
    let other_bytes = reader_archive(&[(b"b.zrd", b"\x02\x00\x00\x00")]);
    let other_path = path("zbd/c1/m03/zrdr.zbd");
    let other = discover_container("other", &other_path, &other_bytes);

    let config = ProbeConfig::new(4, 64).expect("a valid instruction unit");
    let mut session = ProbeSession::new(table, config);
    let truncated_index = session
        .probe(&truncated.programs()[0])
        .expect("the first program is probed");
    let other_index = session
        .probe(&other.programs()[0])
        .expect("one stopped probe does not abort the run");
    assert_eq!(session.probes().len(), 2, "the probes are isolated");
    assert_eq!(session.probes()[truncated_index].code(), "truncated_opcode");
    assert!(!session.probes()[truncated_index].retryable());
    // The second program stops where its own bytes say, not where the first
    // one did.
    assert_eq!(
        session.probes()[other_index].first_unknown(),
        Some((ByteSpan::new(0, 4), 0x0000_0002))
    );

    assert_eq!(
        session
            .retry(truncated_index, &truncated.programs()[0])
            .map_err(|error| error.code()),
        Err("not_retryable")
    );
    assert_eq!(
        session
            .retry(7, &truncated.programs()[0])
            .map_err(|error| error.code()),
        Err("no_such_probe")
    );
    assert_eq!(
        session
            .retry(other_index, &truncated.programs()[0])
            .map_err(|error| error.code()),
        Err("mismatched_program")
    );
    let refusal = session
        .retry(other_index, &truncated.programs()[0])
        .map(|()| String::new())
        .unwrap_or_else(|error| error.to_string());
    assert!(
        refusal.contains("container `other`") && refusal.contains("container `truncated`"),
        "the refusal names both programs: {refusal}"
    );

    let report = session.teardown();
    assert_eq!(report.len(), 2);
    assert_eq!(report.resolved(), 0);
    assert_eq!(report.unresolved(), 2);
    assert!(!report.complete());
    // Exactly one stop is the retryable unknown opcode.
    assert_eq!(report.retryable(), 1);
}

/// AC03 over the original installation: every sampled container's records
/// carry a reachability verdict and its evidence, no retail byte range is
/// both unknown and unused, and the empty table resolves nothing — the honest
/// state of a corpus whose mission opcode table is unmeasured.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f13_c_retail_reachability_covers_the_campaign_programs() {
    let root = game_dir();
    let samples = [
        ("ZBD/interp.zbd", "zbd/interp.zbd"),
        ("ZBD/C1/M02/zrdr.zbd", "zbd/c1/m02/zrdr.zbd"),
        ("ZBD/C1/M02/mis_anim.zbd", "zbd/c1/m02/mis_anim.zbd"),
        ("ZBD/C1/cam_anim.zbd", "zbd/c1/cam_anim.zbd"),
    ];
    let config = ProbeConfig::new(4, 4096).expect("a valid instruction unit");
    let mut session = ProbeSession::new(SignatureTable::new(), config);
    let mut records = 0usize;
    let mut probed = 0usize;
    let mut unused_unknown = 0usize;
    for (host, logical) in samples {
        let bytes =
            fs::read(root.join(host)).unwrap_or_else(|error| panic!("read {host}: {error}"));
        let relative = path(logical);
        let discovery = discover_container(logical, &relative, &bytes);
        assert!(
            discovery.findings().is_empty(),
            "{logical}: {:?}",
            discovery.findings()
        );
        assert!(!discovery.is_empty(), "{logical} locates a program");
        let sources = [ScriptSource::new(logical, &relative, &bytes)];
        let mut inventory = inventory_scripts(&sources);
        let entry = inventory
            .entries_mut()
            .first_mut()
            .expect("one source yields one entry");
        let stats = probe_records(entry, &discovery).expect("the same container");
        records += stats.records;
        unused_unknown += stats.unused_unknown;
        for record in entry.records() {
            assert!(
                record.reachability().is_some(),
                "{logical} {} was never probed",
                record.span()
            );
            assert!(
                record.reachability_evidence().is_some(),
                "{logical} {} has no reachability evidence",
                record.span()
            );
        }
        for program in discovery.programs() {
            let index = session.probe(program).expect("the session runs probes");
            let probe = &session.probes()[index];
            assert!(
                !probe.resolved(),
                "{logical} {} resolved without a claim",
                program.locator().span()
            );
            probed += 1;
        }
    }
    // One header, 98 index entries and 98 script bodies from interp.zbd,
    // plus one opaque record from each of the three archives.
    assert_eq!(records, 200);
    assert_eq!(unused_unknown, 0, "every retail unknown range is covered");
    assert!(probed > 98, "every sampled program was probed");

    let report = session.teardown();
    assert_eq!(report.len(), probed);
    assert_eq!(
        report.resolved(),
        0,
        "the fleet has measured no mission opcode"
    );
    assert_eq!(report.retryable(), report.unresolved());
    assert!(!report.complete());
    assert!(report.table().is_empty());
}
