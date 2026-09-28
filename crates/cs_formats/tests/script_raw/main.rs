//! Acceptance stage F13-A: the script-container inventory and its
//! disassembly-neutral evidence schema
//! (`specs/F13-mission-language-discovery-and-compatibility-closure.md`,
//! section `### F13-A`, AC01: "Inventory all candidate script containers
//! without pretending unknown records are instructions").
//!
//! Every byte in this file is authored here. No original game data, no
//! `CS_GAME_DIR` access.

use cs_formats::script_raw::{
    ByteSpan, Candidacy, Confidence, DispatchOutcome, EXCLUDED_FAMILY_REASON, EvidenceError,
    EvidenceLocator, FormatDiscriminator, INTERP_BODY_UNESTABLISHED, InstructionClaimError,
    InstructionStatus, InventoryFinding, LocatorKind, MAX_LEADS_PER_RECORD, MAX_NOTE_BYTES,
    RecordKind, ReferenceKind, ResearchMethod, ScriptContainerEntry, ScriptEvidence,
    ScriptInventory, ScriptRole, ScriptSource, UNDECODED_UNESTABLISHED, inventory_scripts,
};
use cs_formats::zbd::{ANIMATION_SIGNATURE, ANIMATION_VERSION, INTERP_SIGNATURE, INTERP_VERSION};
use cs_formats::{INDEX_ENTRY_BYTES, INTERP_HEADER_BYTES, NAME_FIELD_BYTES};
use cs_types::install::RelativePath;

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
