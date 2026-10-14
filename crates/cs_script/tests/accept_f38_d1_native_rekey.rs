//! F38-D.1 acceptance: UI host-call families keyed by **(native, dispatch
//! value)**, with goslocalize, gospageinit and `mail` bound from the owner's
//! static evidence.
//!
//! `specs/F38-original-program-adapters-and-native-behavior-bindings.md`, stage
//! `### F38-D`; shared contract `docs/contracts/SCRIPT-MISSION.md`; owner
//! evidence: Rally #159 owner note (2026-10-05), decrypted executable sha256
//! `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`. Claim
//! class: static code evidence, never `verified_original`.
//!
//! Every program here is authored synthetic text in the measured dialect; no
//! byte of the installation is committed. The producer is
//! `cs_formats::script_raw::ui_host_calls` (native table, scanner, corpus); the
//! consumer is `cs_script::bindings::{observed, located}`. The crossing is
//! performed field for field through the published wire codes, as
//! `cs_script::bindings::measured` documents. The retail census is pinned in
//! `accept_f38_d1_retail_native_census` against `$CS_GAME_DIR`.
//!
//! These tests fail if:
//! * the family key drops the native (two natives sharing a dispatch value
//!   would merge, or the corpus would refuse them as duplicates);
//! * a goslocalize (or gospageinit, or `mail`) site stops binding;
//! * an unmeasured uidata/goscallback/uicontrol id is reported as bound.

use cs_formats::script_raw::ui_host_calls::{
    CorpusMember, HostKind, UiNative, UiScriptLimits, measure_host_call_corpus, scan_ui_program,
};
use cs_script::bindings::located::{SiteOrigin, SiteRow, audit_sites};
use cs_script::bindings::observed::{
    CoverageError, MeasuredCallRow, MeasuredHost, MeasuredNative, MeasuredShape,
    ObservedBindingTable, ObservedDisposition, UnimplementedReason,
};

/// Id 6 is spelled under two different natives in the retail corpus (owner
/// census, Rally #159). This fixture reproduces exactly that shape.
const TWO_NATIVES_ONE_ID: &[u8] =
    b"main\r\n{\r\ncallback($$A$$, 6, 1)\r\ncallback($$UB$$, 6, 1)\r\n}\r\n";

/// Only the three hosts with a measured meaning: goslocalize with a literal
/// and a computed string id, gospageinit with its page argument, and `mail`
/// with a literal and a computed message id.
const MEASURED_HOSTS: &[u8] = b"main\r\n{\r\n\
callback($$NB$$, 500, 1)\r\n\
callback($$NB$$, (SID), 1)\r\n\
callback($$JI$$, PAGE_MAIN)\r\n\
mail(10001, this)\r\n\
mail((MSG), this)\r\n\
}\r\n";

/// One site per unmeasured host: a goscallback id, a uidata id, a uicontrol
/// id and a dispatch target token outside the five-native table.
const UNMEASURED_HOSTS: &[u8] = b"main\r\n{\r\n\
callback($$A$$, 13, 1)\r\n\
callback($$E$$, 2133, -1)\r\n\
callback($$UB$$, 6, 1)\r\n\
callback($$XX$$, 42, 1)\r\n\
}\r\n";

/// One goslocalize family whose sites vary in arity and argument shape: the
/// mode/index variation F38-C's fixed-signature model refused as `no_family`.
const PROTOCOL_VARIATION: &[u8] = b"main\r\n{\r\n\
callback($$NB$$, 500, 1)\r\n\
callback($$NB$$, 500, CAPTION)\r\n\
callback($$NB$$, 500)\r\n\
}\r\n";

fn corpus_of(bytes: &[u8]) -> cs_formats::script_raw::ui_host_calls::HostCallCorpus {
    measure_host_call_corpus(
        [CorpusMember {
            spelling: "synthetic.script",
            bytes,
        }],
        UiScriptLimits::default(),
    )
    .expect("the authored program scans")
}

fn host_of(call: &cs_formats::script_raw::ui_host_calls::ObservedHostCall) -> MeasuredHost {
    MeasuredHost::from_code(call.host.code())
        .unwrap_or_else(|| panic!("code {} is a measured host", call.host.code()))
}

fn rows(corpus: &cs_formats::script_raw::ui_host_calls::HostCallCorpus) -> Vec<MeasuredCallRow> {
    corpus
        .calls
        .iter()
        .map(|call| MeasuredCallRow {
            host: host_of(call),
            native_id: call.native_id,
            sites: call.sites,
            scripts: call.scripts,
            arities: call.arities.clone(),
            arg_shapes: call
                .arg_shapes
                .iter()
                .map(|counts| {
                    counts
                        .is_uniform()
                        .then(|| counts.dominant())
                        .flatten()
                        .and_then(|shape| MeasuredShape::from_code(shape.code()))
                })
                .collect(),
            evidence: call.evidence.note().to_owned(),
        })
        .collect()
}

/// The site rows a consumer reads from the producer's scan, crossed field for
/// field (host through its wire code, id as the scan measured it, argument
/// shape codes verbatim).
fn site_rows(spelling: &str, bytes: &[u8]) -> Vec<SiteRow> {
    let scan = scan_ui_program(spelling, bytes, UiScriptLimits::default()).expect("scans");
    scan.sites
        .iter()
        .map(|site| SiteRow {
            host: MeasuredHost::from_code(site.host().code())
                .unwrap_or_else(|| panic!("code {} is a measured host", site.host().code())),
            native_id: site.native_id,
            arg_shape_codes: site.args.iter().map(|s| s.code()).collect(),
            origin: SiteOrigin {
                member: spelling.to_owned(),
                offset: site.span.offset,
                len: site.span.len,
                line: 1,
                column: 1,
            },
        })
        .collect()
}

/// **The family key keeps the native.** Id 6 under `$$A$$` (goscallback) and
/// `$$UB$$` (uicontrol) is two families, not one and not a duplicate: the
/// producer keys them apart, the crossing keeps the native, the table accepts
/// both, the spellings differ, and the audit judges each site against its own
/// native. Removing the native from the key fails this test three ways: the
/// corpus/table would collapse or refuse the pair, the spellings would
/// collide, and one site would be judged by the other native's family.
#[test]
fn accept_f38_d1_the_family_key_keeps_the_native() {
    // The two crates publish the same native wire vocabulary.
    for native in UiNative::ALL {
        assert_eq!(
            MeasuredNative::from_code(native.code()),
            Some(match native {
                UiNative::GosCallback => MeasuredNative::GosCallback,
                UiNative::UiData => MeasuredNative::UiData,
                UiNative::UiControl => MeasuredNative::UiControl,
                UiNative::GosPageInit => MeasuredNative::GosPageInit,
                UiNative::GosLocalize => MeasuredNative::GosLocalize,
            }),
            "code {} names the same native in both crates",
            native.code()
        );
        assert_eq!(
            MeasuredHost::from_code(native.code()).map(MeasuredHost::label),
            Some(native.label())
        );
    }
    assert_eq!(MeasuredHost::from_code(6), Some(MeasuredHost::Mail));
    assert_eq!(MeasuredHost::from_code(7), Some(MeasuredHost::Unmeasured));
    assert_eq!(
        MeasuredHost::from_code(8),
        None,
        "an unknown code is refused"
    );

    let corpus = corpus_of(TWO_NATIVES_ONE_ID);
    assert_eq!(corpus.sites, 2);
    let gos_callback = corpus
        .call(HostKind::Native(UiNative::GosCallback), Some(6))
        .expect("the goscallback 6 family exists");
    let ui_control = corpus
        .call(HostKind::Native(UiNative::UiControl), Some(6))
        .expect("the uicontrol 6 family exists");
    assert_eq!(gos_callback.sites, 1);
    assert_eq!(ui_control.sites, 1);
    assert_ne!(
        gos_callback.host, ui_control.host,
        "the same dispatch value under two natives is two families"
    );

    let table = ObservedBindingTable::measure(&rows(&corpus)).expect("both rows are valid");
    assert_eq!(
        table.coverage().families,
        2,
        "the dedup key kept the native: the same id twice is not a duplicate"
    );
    let family = |native: MeasuredNative| {
        table
            .family(MeasuredHost::Native(native), Some(6))
            .expect("the (native, 6) family exists")
    };
    let gos_callback = family(MeasuredNative::GosCallback);
    let ui_control = family(MeasuredNative::UiControl);
    assert_eq!(gos_callback.call.spelling(), "goscallback#6");
    assert_eq!(ui_control.call.spelling(), "uicontrol#6");
    assert_ne!(gos_callback.call.spelling(), ui_control.call.spelling());
    // Both are unmeasured and refuse with their own native and handler.
    for (native, family) in [
        (MeasuredNative::GosCallback, gos_callback),
        (MeasuredNative::UiControl, ui_control),
    ] {
        match &family.disposition {
            ObservedDisposition::Unimplemented {
                reason: UnimplementedReason::MeaningNotMeasured { native: found, .. },
                ..
            } => assert_eq!(*found, native),
            other => panic!("an unmeasured native refuses by meaning: {other:?}"),
        }
    }

    // The audit judges each site by its own native.
    let audit = audit_sites(&table, &site_rows("A.SCRIPT", TWO_NATIVES_ONE_ID), 0).expect("audit");
    let spellings: Vec<&str> = audit
        .sites
        .iter()
        .map(|site| match &site.verdict {
            cs_script::bindings::located::SiteVerdict::Bound { spelling }
            | cs_script::bindings::located::SiteVerdict::Refused { spelling, .. } => {
                spelling.as_str()
            }
            other => panic!("both sites have a family: {other:?}"),
        })
        .collect();
    assert_eq!(spellings, ["goscallback#6", "uicontrol#6"]);
    assert_eq!(audit.bound(), 0);
    assert!(!audit.campaign_ready());

    // A true duplicate — the same host and value twice — is still refused.
    let row = rows(&corpus).into_iter().next().expect("one row");
    assert_eq!(
        ObservedBindingTable::measure(&[row.clone(), row]),
        Err(CoverageError::DuplicateCall {
            host: MeasuredHost::Native(MeasuredNative::GosCallback),
            native_id: Some(6),
        })
    );
}

/// **The measured hosts bind with their measured meanings.** goslocalize
/// binds every id — literal or computed — as a localized string id;
/// gospageinit binds its page argument; `mail` binds every message id as the
/// receiving script's mailbox case. A corpus of only measured hosts is
/// campaign-ready, which is what proves the gate is a measurement rather than
/// a constant `false`.
#[test]
fn accept_f38_d1_the_measured_hosts_bind_with_their_measured_meanings() {
    let corpus = corpus_of(MEASURED_HOSTS);
    let table = ObservedBindingTable::measure(&rows(&corpus)).expect("rows are valid");
    let bound: Vec<(&str, &str)> = table
        .families()
        .iter()
        .map(|family| match &family.disposition {
            ObservedDisposition::Bound { spelling, meaning } => {
                (spelling.as_str(), meaning.label())
            }
            ObservedDisposition::Unimplemented { reason, .. } => {
                panic!(
                    "{} must be bound, not refused: {reason}",
                    family.call.spelling()
                )
            }
        })
        .collect();
    assert_eq!(
        bound,
        [
            // BTreeMap order over (host, value): natives before mail, and a
            // host's computed-dispatch family (None) before its integer ids.
            ("gospageinit#computed", "page_argument"),
            ("goslocalize#computed", "localized_string_id"),
            ("goslocalize#500", "localized_string_id"),
            ("mail#computed", "script_mailbox_message"),
            ("mail#10001", "script_mailbox_message"),
        ]
    );
    assert!(
        table.registry().is_empty(),
        "bound here means a measured meaning, not a mission-registry entry: a mission \\
         program referencing a UI host call must still fail validation"
    );

    let audit = audit_sites(&table, &site_rows("B.SCRIPT", MEASURED_HOSTS), 0).expect("audit");
    assert_eq!(audit.sites.len(), 5);
    assert_eq!(audit.bound(), 5, "every measured-host site binds");
    assert_eq!(audit.refused(), 0);
    assert_eq!(audit.malformed(), 0);
    assert_eq!(audit.unmeasured_families().len(), 0);
    assert!(
        audit.campaign_ready(),
        "a corpus whose every family has a measured meaning is covered: the gate is \\
         a measurement, not a constant false"
    );

    // The goslocalize binding is per native, not per shape: the computed-id
    // site carries a NameRef argument where the literal-id site carries an
    // integer, and both still bind — the protocol keeps the variation.
    let computed = table
        .family(MeasuredHost::Native(MeasuredNative::GosLocalize), None)
        .expect("the computed-dispatch goslocalize family");
    assert_eq!(computed.call.spelling(), "goslocalize#computed");
    assert!(computed.is_bound());
}

/// **Unmeasured ids are named and never bound.** Every goscallback, uidata and
/// uicontrol value refuses with `MeaningNotMeasured`, its native and its
/// handler address; a dispatch target outside the native table refuses with
/// `HostNotMeasured`; the audit names each unmeasured family by native and id,
/// and the gate stays closed.
#[test]
fn accept_f38_d1_unmeasured_families_are_named_and_never_bound() {
    let corpus = corpus_of(UNMEASURED_HOSTS);
    let table = ObservedBindingTable::measure(&rows(&corpus)).expect("rows are valid");
    let audit = audit_sites(&table, &site_rows("C.SCRIPT", UNMEASURED_HOSTS), 0).expect("audit");

    // Every site of every unmeasured host is refused, never bound.
    assert_eq!(
        audit.bound(),
        0,
        "no unmeasured id may be reported as bound"
    );
    for site in &audit.sites {
        assert_eq!(
            site.verdict.code(),
            "refused",
            "an unmeasured family is refused, not silently judged: {site}"
        );
    }

    // The audit names each unmeasured family by native and id.
    assert_eq!(
        audit.unmeasured_families(),
        [
            "goscallback#13",
            "uidata#2133",
            "uicontrol#6",
            "unmeasured#42"
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<std::collections::BTreeSet<_>>()
    );

    // Each refusal names its handler address, so the open question stays
    // traceable to the image it must be measured from.
    let display = |spelling: &str| {
        audit
            .sites
            .iter()
            .find(|site| site.to_string().contains(&format!("`{spelling}`")))
            .unwrap_or_else(|| panic!("a site named {spelling}"))
            .to_string()
    };
    assert!(display("goscallback#13").contains("0x407670"));
    assert!(display("uidata#2133").contains("0x4093a0"));
    assert!(display("uicontrol#6").contains("0x404960"));

    // The token outside the native table is its own kind of unmeasured.
    let unmeasured = table
        .family(MeasuredHost::Unmeasured, Some(42))
        .expect("the unmeasured-target family");
    assert!(matches!(
        unmeasured.disposition,
        ObservedDisposition::Unimplemented {
            reason: UnimplementedReason::HostNotMeasured { .. },
            ..
        }
    ));

    // The family-level gate refuses too, and it names what is missing.
    let coverage = table.coverage();
    assert_eq!(coverage.families, 4);
    assert_eq!(coverage.bound_families, 0);
    assert!(!coverage.complete() && !coverage.campaign_ready());
    assert!(!audit.campaign_ready());

    // No measured-host spelling ever appears among the unmeasured families:
    // an unmeasured uidata/goscallback/uicontrol id cannot hide behind a
    // measured host's name.
    for spelling in audit.unmeasured_families() {
        for measured in ["goslocalize#", "gospageinit#", "mail#"] {
            assert!(
                !spelling.starts_with(measured),
                "{spelling} is unmeasured and must not carry a measured host's name"
            );
        }
    }
}

/// **Protocol variation is judged, not dropped.** One (native, id) with
/// several arities and a varying argument shape is a protocol: every site is
/// judged against it (bound for a measured host), the variation stays visible
/// in the family, and nothing lands in `no_family` — F38-C's fixed-signature
/// model refused exactly this shape 994 times on the retail corpus.
#[test]
fn accept_f38_d1_protocol_variation_is_judged_not_dropped() {
    let corpus = corpus_of(PROTOCOL_VARIATION);
    let family = corpus
        .call(HostKind::Native(UiNative::GosLocalize), Some(500))
        .expect("one family");
    assert_eq!(family.sites, 3);
    assert_eq!(family.arities, vec![0, 1], "two observed arities");
    assert_eq!(
        family.arg_shapes[0].count(cs_formats::script_raw::ui_host_calls::ArgShape::IntegerLiteral),
        1
    );
    assert_eq!(
        family.arg_shapes[0].count(cs_formats::script_raw::ui_host_calls::ArgShape::NameRef),
        1
    );
    assert!(
        !family.arg_shapes[0].is_uniform(),
        "the position varied; the producer counted both readings"
    );

    let table = ObservedBindingTable::measure(&rows(&corpus)).expect("rows are valid");
    let measured = table
        .family(MeasuredHost::Native(MeasuredNative::GosLocalize), Some(500))
        .expect("the family crossed");
    assert_eq!(measured.call.arities, vec![0, 1]);
    assert_eq!(
        measured.call.arg_shapes,
        vec![None],
        "the crossing hands the variation through as `None`, never resolved to one shape"
    );

    let audit = audit_sites(&table, &site_rows("D.SCRIPT", PROTOCOL_VARIATION), 0).expect("audit");
    assert_eq!(
        audit.bound(),
        3,
        "every site of a measured protocol is judged: arity/shape variation is not \\
         a reason to drop one"
    );
    assert_eq!(audit.with_code("no_family").count(), 0);
    assert_eq!(audit.malformed(), 0);
}
