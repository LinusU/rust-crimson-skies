//! The registry: one [`Binding`] per declared [`ContainerSpec`] connecting
//! the manifest id to the production entrypoint it names.
//!
//! Every probe calls real `cs_formats` code — never a test-local parser —
//! and reduces the result to a [`ProbeOutcome`]: accepted, refused with a
//! structured diagnostic code, or completed-but-reporting-lost-content for
//! the `ExtentStatus` containers. A binding that fails to compile means a
//! declared entrypoint went away: the contract then fails loudly, which is
//! the point of binding by name.

use cs_formats::gamez::{read_gamez_materials, read_gamez_meshes};
use cs_formats::script_raw::{
    ByteSpan, Confidence, OpcodeEntry, OpcodeLedger, ProgramKind, ProgramLocator, ScriptSource,
    discover_container, inventory_scripts, walk_program,
};
use cs_formats::text::{read_keyed_list, read_resource_header, scan_lines};
use cs_formats::texture::{read_bmp, read_tga, read_zbd_textures};
use cs_formats::zbd::{
    MemberExtent, MemberTable, ZbdProbe, dispatch, list_members, read_reader_archive,
    read_sound_archive, read_version_one_index, read_wave_header,
};
use cs_formats::{
    AllocationBudget, ParseContext, RofLimits, decode_interp, read_bm, read_directory, read_interp,
    read_member, read_pe_layout, read_pe_resources, read_tree,
};
use cs_types::evidence::SourceSpan;
use cs_types::install::RelativePath;

use crate::fixtures;
use crate::spans::CorpusFixture;

/// What probing one input produced.
#[derive(Debug)]
pub enum ProbeOutcome {
    /// The entrypoint accepted the input.
    Accepted(&'static str),
    /// The entrypoint refused it with a bounded diagnostic (the payload is
    /// the error's `code()` — always metadata, never input bytes).
    Refused(&'static str),
    /// For `ExtentStatus` containers: the parse completed and the listing
    /// reports the lost content.
    Damaged,
}

/// One manifest container bound to its fixture builder and probe.
pub struct Binding {
    /// The [`cs_xtask::corpus::ContainerSpec::id`].
    pub container: &'static str,
    /// Builds the entry's bytes and span map.
    pub fixture: fn() -> CorpusFixture,
    /// Runs the production entrypoint over `cut`, with `fixture` available
    /// for inputs the bytes do not carry (a member table, a resolved
    /// member, a dispatch path).
    pub probe: fn(&CorpusFixture, &[u8]) -> ProbeOutcome,
}

/// The binding registry, one row per declared container.
pub fn registry() -> &'static [Binding] {
    &[
        Binding {
            container: "rof.directory",
            fixture: fixtures::rof_directory,
            probe: |_, cut| {
                let mut context = ParseContext::with_defaults("corpus/rof.directory");
                read_directory(&mut context, cut)
                    .map(|_| ProbeOutcome::Accepted("directory"))
                    .unwrap_or_else(|error| ProbeOutcome::Refused(error.code()))
            },
        },
        Binding {
            container: "rof.tree",
            fixture: fixtures::rof_tree,
            probe: |_, cut| {
                let mut context = ParseContext::with_defaults("corpus/rof.tree");
                read_tree(&mut context, cut)
                    .map(|_| ProbeOutcome::Accepted("tree"))
                    .unwrap_or_else(|error| ProbeOutcome::Refused(error.code()))
            },
        },
        Binding {
            container: "rof.member",
            fixture: fixtures::rof_member,
            probe: |fixture, cut| {
                let mut context = ParseContext::with_defaults("corpus/rof.member");
                let Ok(tree) = read_tree(&mut context, &fixture.bytes) else {
                    unreachable!("the authored fixture always parses")
                };
                read_member(&context, cut, &tree.members()[0], &RofLimits::default())
                    .map(|_| ProbeOutcome::Accepted("member"))
                    .unwrap_or_else(|error| ProbeOutcome::Refused(error.code()))
            },
        },
        Binding {
            container: "zbd.dispatch",
            fixture: fixtures::zbd_dispatch,
            probe: |_, cut| {
                dispatch(ZbdProbe::new(
                    "corpus/zbd.dispatch",
                    &role("zbd/interp.zbd"),
                    &cut[..cut.len().min(64)],
                ))
                .map(|_| ProbeOutcome::Accepted("dispatch"))
                .unwrap_or_else(|error| ProbeOutcome::Refused(error.code()))
            },
        },
        Binding {
            container: "zbd.reader_archive",
            fixture: fixtures::zbd_reader_archive,
            probe: |fixture, cut| archive_probe(fixture, cut, "zbd/zrdr.zbd", false),
        },
        Binding {
            container: "zbd.list_members",
            fixture: fixtures::zbd_list_members,
            probe: |fixture, cut| {
                let table = archive_table(fixture, "zbd/zrdr.zbd");
                let mut context = ParseContext::with_defaults("corpus/zbd.list_members");
                match list_members(&mut context, cut, &table) {
                    Err(error) => ProbeOutcome::Refused(error.code()),
                    Ok(listing) => match listing.status() {
                        cs_formats::zbd::ContainerStatus::Clean => ProbeOutcome::Accepted("clean"),
                        cs_formats::zbd::ContainerStatus::Failed { .. } => ProbeOutcome::Damaged,
                    },
                }
            },
        },
        Binding {
            container: "zbd.sound_archive",
            fixture: fixtures::zbd_sound_archive,
            probe: |fixture, cut| archive_probe(fixture, cut, "zbd/soundsl.zbd", true),
        },
        Binding {
            container: "zbd.trailer_index",
            fixture: fixtures::zbd_trailer_index,
            probe: |_, cut| {
                let path = role("zbd/zrdr.zbd");
                let decided = match dispatch(ZbdProbe::new(
                    "corpus/zbd.trailer_index",
                    &path,
                    &cut[..cut.len().min(64)],
                )) {
                    Ok(decided) => decided,
                    Err(error) => return ProbeOutcome::Refused(error.code()),
                };
                let mut context = ParseContext::with_defaults("corpus/zbd.trailer_index");
                read_version_one_index(&mut context, decided, cut)
                    .map(|_| ProbeOutcome::Accepted("index"))
                    .unwrap_or_else(|error| ProbeOutcome::Refused(error.code()))
            },
        },
        Binding {
            container: "zbd.wave_header",
            fixture: fixtures::zbd_wave_header,
            probe: |_, cut| {
                read_wave_header(cut)
                    .map(|_| ProbeOutcome::Accepted("wave"))
                    .unwrap_or_else(|error| ProbeOutcome::Refused(error.code()))
            },
        },
        Binding {
            container: "interp.container",
            fixture: fixtures::interp_container,
            probe: |_, cut| {
                let mut context = ParseContext::with_defaults("corpus/interp.container");
                read_interp(&mut context, cut)
                    .map(|_| ProbeOutcome::Accepted("interp"))
                    .unwrap_or_else(|error| ProbeOutcome::Refused(error.code()))
            },
        },
        Binding {
            container: "interp.decode",
            fixture: fixtures::interp_container,
            probe: |_, cut| {
                let mut context = ParseContext::with_defaults("corpus/interp.decode");
                decode_interp(&mut context, cut)
                    .map(|_| ProbeOutcome::Accepted("decoded"))
                    .unwrap_or_else(|error| ProbeOutcome::Refused(error.code()))
            },
        },
        Binding {
            container: "bm.image",
            fixture: fixtures::bm_image,
            probe: |_, cut| {
                let mut context = ParseContext::with_defaults("corpus/bm.image");
                read_bm(&mut context, cut)
                    .map(|_| ProbeOutcome::Accepted("bm"))
                    .unwrap_or_else(|error| ProbeOutcome::Refused(error.code()))
            },
        },
        Binding {
            container: "texture.bmp",
            fixture: fixtures::texture_bmp,
            probe: |_, cut| {
                let mut budget = AllocationBudget::with_defaults("corpus/texture.bmp");
                read_bmp("corpus/texture.bmp", cut, &mut budget)
                    .map(|_| ProbeOutcome::Accepted("bmp"))
                    .unwrap_or_else(|error| ProbeOutcome::Refused(error.code()))
            },
        },
        Binding {
            container: "texture.tga",
            fixture: fixtures::texture_tga,
            probe: |_, cut| {
                let mut budget = AllocationBudget::with_defaults("corpus/texture.tga");
                read_tga("corpus/texture.tga", cut, &mut budget)
                    .map(|_| ProbeOutcome::Accepted("tga"))
                    .unwrap_or_else(|error| ProbeOutcome::Refused(error.code()))
            },
        },
        Binding {
            container: "texture.zbd_package",
            fixture: fixtures::texture_zbd_package,
            probe: |_, cut| {
                let mut budget = AllocationBudget::with_defaults("corpus/texture.zbd_package");
                read_zbd_textures("corpus/texture.zbd_package", cut, &mut budget)
                    .map(|_| ProbeOutcome::Accepted("package"))
                    .unwrap_or_else(|error| ProbeOutcome::Refused(error.code()))
            },
        },
        Binding {
            container: "gamez.meshes",
            fixture: fixtures::gamez_meshes,
            probe: |_, cut| {
                let mut context = ParseContext::with_defaults("corpus/gamez.meshes");
                read_gamez_meshes(&mut context, "corpus/gamez.meshes", cut)
                    .map(|_| ProbeOutcome::Accepted("meshes"))
                    .unwrap_or_else(|error| ProbeOutcome::Refused(error.code()))
            },
        },
        Binding {
            container: "gamez.materials",
            fixture: fixtures::gamez_materials,
            probe: |_, cut| {
                let mut context = ParseContext::with_defaults("corpus/gamez.materials");
                read_gamez_materials(&mut context, "corpus/gamez.materials", cut)
                    .map(|_| ProbeOutcome::Accepted("materials"))
                    .unwrap_or_else(|error| ProbeOutcome::Refused(error.code()))
            },
        },
        Binding {
            container: "pe.layout",
            fixture: fixtures::pe_layout,
            probe: |_, cut| {
                let mut context = ParseContext::with_defaults("corpus/pe.layout");
                read_pe_layout(&mut context, cut)
                    .map(|_| ProbeOutcome::Accepted("pe"))
                    .unwrap_or_else(|error| ProbeOutcome::Refused(error.code()))
            },
        },
        Binding {
            container: "pe.resources",
            fixture: fixtures::pe_resources,
            probe: |_, cut| {
                let mut context = ParseContext::with_defaults("corpus/pe.resources");
                read_pe_resources(&mut context, cut)
                    .map(|_| ProbeOutcome::Accepted("resources"))
                    .unwrap_or_else(|error| ProbeOutcome::Refused(error.code()))
            },
        },
        Binding {
            container: "script.program",
            fixture: fixtures::script_program,
            probe: |_, cut| {
                let mut ledger = OpcodeLedger::new();
                for (opcode, spelling) in
                    [(0x10, "OP_TEN"), (0x20, "OP_TWENTY"), (0x30, "OP_THIRTY")]
                {
                    ledger
                        .insert(
                            OpcodeEntry::new(
                                opcode,
                                spelling,
                                ProgramKind::Unknown,
                                Confidence::Inferred,
                                "corpus",
                            )
                            .expect("the corpus entries are well-formed"),
                        )
                        .expect("the corpus opcodes are distinct");
                }
                let locator =
                    ProgramLocator::new("corpus/script.program", None, ByteSpan::new(0, 12));
                walk_program("corpus", &locator, cut, 4, &ledger, 1024)
                    .map(|_| ProbeOutcome::Accepted("walked"))
                    .unwrap_or_else(|error| ProbeOutcome::Refused(error.code()))
            },
        },
        Binding {
            container: "text.lines",
            fixture: fixtures::text_lines,
            probe: |_, cut| {
                let mut context = ParseContext::with_defaults("corpus/text.lines");
                scan_lines(&mut context, cut)
                    .map(|_| ProbeOutcome::Accepted("scanned"))
                    .unwrap_or_else(|_| ProbeOutcome::Refused("budget"))
            },
        },
        Binding {
            container: "text.keyed_list",
            fixture: fixtures::text_keyed_list,
            probe: |_, cut| {
                let mut context = ParseContext::with_defaults("corpus/text.keyed_list");
                read_keyed_list(&mut context, cut)
                    .map(|_| ProbeOutcome::Accepted("listed"))
                    .unwrap_or_else(|_| ProbeOutcome::Refused("budget"))
            },
        },
        Binding {
            container: "text.resource_header",
            fixture: fixtures::text_resource_header,
            probe: |_, cut| {
                let mut context = ParseContext::with_defaults("corpus/text.resource_header");
                read_resource_header(&mut context, cut)
                    .map(|_| ProbeOutcome::Accepted("header"))
                    .unwrap_or_else(|_| ProbeOutcome::Refused("budget"))
            },
        },
        Binding {
            container: "script.discovery",
            fixture: fixtures::script_discovery,
            probe: |_, cut| {
                let discovery =
                    discover_container("corpus/script.discovery", &role("zbd/interp.zbd"), cut);
                if discovery.findings().is_empty() {
                    ProbeOutcome::Accepted("clean")
                } else {
                    ProbeOutcome::Accepted("findings")
                }
            },
        },
        Binding {
            container: "script.inventory",
            fixture: fixtures::script_inventory,
            probe: |fixture, cut| {
                let interp_path = role("zbd/interp.zbd");
                let sound_path = role("zbd/soundsl.zbd");
                let sources = [
                    ScriptSource::new("corpus/script.inventory", &interp_path, cut),
                    ScriptSource::new("corpus/script.inventory", &sound_path, &fixture.bytes),
                ];
                let _ = inventory_scripts(&sources);
                ProbeOutcome::Accepted("inventoried")
            },
        },
    ]
}

/// The install-relative spellings the ZBD role rules key on.
fn role(spelling: &'static str) -> RelativePath {
    RelativePath::new(spelling).expect("corpus spellings are valid relative paths")
}

/// The member extents the `ExtentStatus` probes declare for the archive
/// fixtures: one member over the load-bearing payload span and one over
/// the unreferenced tail.
const ARCHIVE_MEMBERS: &[MemberExtent<'static>] = &[
    MemberExtent::new(
        b"readme",
        Some(1),
        SourceSpan {
            offset: 0,
            length: 10,
        },
    ),
    MemberExtent::new(
        b"slack",
        Some(2),
        SourceSpan {
            offset: 10,
            length: 4,
        },
    ),
];

/// Builds the member table the family's own readers would declare for the
/// fixture: dispatch on the fixture's role path, then the shared member
/// extents.
fn archive_table(fixture: &CorpusFixture, spelling: &'static str) -> MemberTable<'static> {
    let path = role(spelling);
    let decided = dispatch(ZbdProbe::new(
        "corpus/zbd.archive",
        &path,
        &fixture.bytes[..fixture.bytes.len().min(64)],
    ))
    .expect("the role paths dispatch without a signature");
    MemberTable::from_dispatch(&decided, ARCHIVE_MEMBERS)
}

/// The shared probe for the two `ExtentStatus` archives: build the member
/// table the family's own reader would declare for the fixture, then hand
/// the (possibly truncated) bytes to the production listing.
fn archive_probe(
    fixture: &CorpusFixture,
    cut: &[u8],
    spelling: &'static str,
    sound: bool,
) -> ProbeOutcome {
    let table = archive_table(fixture, spelling);
    let mut context = ParseContext::with_defaults("corpus/zbd.archive");
    let outcome = if sound {
        read_sound_archive(&mut context, &table, cut)
            .map(|archive| archive.status())
            .map_err(|error| error.code())
    } else {
        read_reader_archive(&mut context, &table, cut)
            .map(|archive| archive.status())
            .map_err(|error| error.code())
    };
    match outcome {
        Err(code) => ProbeOutcome::Refused(code),
        Ok(cs_formats::zbd::ContainerStatus::Clean) => ProbeOutcome::Accepted("clean"),
        Ok(cs_formats::zbd::ContainerStatus::Failed { .. }) => ProbeOutcome::Damaged,
    }
}
