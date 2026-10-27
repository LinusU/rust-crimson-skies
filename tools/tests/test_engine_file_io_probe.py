"""Acceptance tests for the F15-F engine file-IO probe (task #403).

The probe in `tools/engine_file_io_probe.py` is the read-only instrument the
F15-F finding rests on.  These tests are the task's `accept_f15_f_` selection:

* a synthetic PE32 image built here in memory pins the parser's contract
  (import table, IAT resolution, the two reference shapes, `push imm32`
  naming, string identification) so the instrument is exercised without the
  owner's image;
* the digest guard is shown to refuse bytes other than the measured image, so
  no finding can be derived from a drifted binary;
* the recorded facts the finding cites are re-measured on the real
  `$CS_ENGINE_IMAGE`, and the retail reader container's compiled-node tags are
  re-measured under `$CS_GAME_DIR`.

Run with original data:

    python3 -m unittest discover -s tools/tests \\
        -p 'test_engine_file_io_probe.py' -v

The two data-dependent tests skip (they do not pass) when the variables are
unset, and fail when a variable is set but names something else.
"""

from __future__ import annotations

import hashlib
import os
import struct
import sys
import tempfile
import unittest

TOOLS_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if TOOLS_DIR not in sys.path:
    sys.path.insert(0, TOOLS_DIR)

import engine_file_io_probe as probe  # noqa: E402


# ---------------------------------------------------------------------------
# A synthetic PE32 image, built here so the parser is testable without data
# ---------------------------------------------------------------------------

IMAGE_BASE = 0x400000


def build_synthetic_pe() -> bytes:
    """A three-section PE32 with one DLL, two imports and two references."""
    headers_size = 0x200
    image = bytearray(0x800)

    # DOS header.
    image[0:2] = b"MZ"
    struct.pack_into("<I", image, 0x3C, 0x40)
    # PE signature and COFF header.
    image[0x40:0x44] = b"PE\0\0"
    coff = 0x44
    struct.pack_into("<H", image, coff + 0, 0x14C)  # i386
    struct.pack_into("<H", image, coff + 2, 3)  # sections
    struct.pack_into("<H", image, coff + 16, 0xE0)  # optional header size
    optional = coff + 20
    # Optional header (PE32).
    struct.pack_into("<H", image, optional + 0, 0x10B)
    struct.pack_into("<I", image, optional + 28, IMAGE_BASE)  # ImageBase
    struct.pack_into("<I", image, optional + 60, headers_size)  # SizeOfHeaders
    # Data directory 1 = import table, at optional + 96 + 8.
    struct.pack_into("<II", image, optional + 96 + 8, 0x3000, 0x28)

    # Section table.
    section_table = optional + 0xE0
    sections = (
        (b".text\0\0\0", 0x200, 0x1000, 0x200, 0x200),
        (b".rdata\0\0", 0x200, 0x2000, 0x200, 0x400),
        (b".idata\0\0", 0x200, 0x3000, 0x200, 0x600),
    )
    for index, (name, virtual_size, virtual_address, raw_size, raw_offset) in enumerate(
        sections
    ):
        entry = section_table + index * 40
        image[entry : entry + 8] = name
        struct.pack_into(
            "<IIII", image, entry + 8, virtual_size, virtual_address, raw_size, raw_offset
        )

    idata = 0x600
    # Import descriptor + terminator.
    struct.pack_into("<IIIII", image, idata + 0x00, 0x3028, 0, 0, 0x3040, 0x3060)
    # ILT at 0x3028, IAT at 0x3060, names at 0x3080 / 0x3090.
    struct.pack_into("<III", image, idata + 0x28, 0x3080, 0x3090, 0)
    image[idata + 0x40 : idata + 0x40 + 13] = b"KERNEL32.dll\0"
    struct.pack_into("<II", image, idata + 0x60, 0x3080, 0x3090)
    image[idata + 0x80 : idata + 0x80 + 14] = b"\0\0CreateFileA\0"
    image[idata + 0x90 : idata + 0x90 + 12] = b"\0\0WriteFile\0"

    # .rdata strings: "wb" at RVA 0x2008 and "hello" at RVA 0x2010.
    rdata = 0x400
    image[rdata + 0x08 : rdata + 0x08 + 3] = b"wb\0"
    image[rdata + 0x10 : rdata + 0x10 + 6] = b"hello\0"

    # .text: push "wb"; call [CreateFileA]; push "hello";
    #        mov eax, [WriteFile]; call eax; int3 padding.
    text = 0x200
    image[text + 0x00 : text + 0x05] = b"\x68" + struct.pack("<I", IMAGE_BASE + 0x2008)
    image[text + 0x05 : text + 0x0B] = b"\xFF\x15" + struct.pack("<I", IMAGE_BASE + 0x3060)
    image[text + 0x0B : text + 0x10] = b"\x68" + struct.pack("<I", IMAGE_BASE + 0x2010)
    image[text + 0x10 : text + 0x15] = b"\xA1" + struct.pack("<I", IMAGE_BASE + 0x3064)
    image[text + 0x15 : text + 0x17] = b"\xFF\xD0"
    for offset in range(0x17, 0x100):
        image[text + offset] = 0xCC
    return bytes(image)


class SyntheticPeParserTests(unittest.TestCase):
    """The parser contract, without any original data."""

    def setUp(self) -> None:
        self.image = probe.PeImage.parse(build_synthetic_pe())

    def test_accept_f15_f_synthetic_pe_sections_and_imports_resolve(self) -> None:
        self.assertEqual(self.image.image_base, IMAGE_BASE)
        self.assertEqual(
            [section.name for section in self.image.sections],
            [".text", ".rdata", ".idata"],
        )
        self.assertEqual(self.image.imports["CreateFileA"], IMAGE_BASE + 0x3060)
        self.assertEqual(self.image.imports["WriteFile"], IMAGE_BASE + 0x3064)
        self.assertEqual(self.image.import_libraries["CreateFileA"], "KERNEL32.dll")

    def test_accept_f15_f_synthetic_pe_both_reference_shapes_are_found(self) -> None:
        direct = self.image.references_to(IMAGE_BASE + 0x3060)
        self.assertEqual(direct, [(IMAGE_BASE + 0x1005, "call")])
        indirect = self.image.references_to(IMAGE_BASE + 0x3064)
        self.assertEqual(indirect, [(IMAGE_BASE + 0x1010, "load")])

    def test_accept_f15_f_synthetic_pe_push_naming_and_strings(self) -> None:
        self.assertEqual(
            self.image.push_string_before(IMAGE_BASE + 0x1005),
            (IMAGE_BASE + 0x2008, "wb"),
        )
        self.assertEqual(self.image.push_immediates(IMAGE_BASE + 0x2008), [IMAGE_BASE + 0x1000])
        self.assertEqual(self.image.push_immediates(IMAGE_BASE + 0x2010), [IMAGE_BASE + 0x100B])
        self.assertEqual(self.image.find_string_bytes("hello"), [IMAGE_BASE + 0x2010])
        self.assertEqual(
            self.image.strings_containing("hell"), [(IMAGE_BASE + 0x2010, "hello")]
        )

    def test_accept_f15_f_synthetic_pe_is_not_accepted_as_the_measured_image(self) -> None:
        data = build_synthetic_pe()
        with self.assertRaises(probe.ProbeError):
            probe.verify_measured_bytes("<synthetic>", data)
        with tempfile.NamedTemporaryFile(suffix=".exe", delete=False) as handle:
            handle.write(data)
            path = handle.name
        try:
            with self.assertRaises(probe.ProbeError):
                probe.load_image(path)
        finally:
            os.unlink(path)


# ---------------------------------------------------------------------------
# The recorded facts, re-measured on the owner-supplied image
# ---------------------------------------------------------------------------

CREATE_FILE_SITES = (
    0x406916,
    0x406C84,
    0x4A93DD,
    0x533FDB,
    0x5343BB,
    0x579D48,
    0x579E28,
    0x579EBA,
    0x59D95B,
    0x59DA0B,
    0x5A9A8D,
    0x5BFE8D,
    0x5BFF3E,
    0x5D015D,
)
WRITE_FILE_SITES = (0x406934, 0x533FF9, 0x59DBA6, 0x59DD6E, 0x5A9AAB, 0x5BFF5E)
CREATE_DIRECTORY_SITES = (0x406BF8,)
GET_TEMP_PATH_SITES = (0x406BDF, 0x59C9BC, 0x59CAB1)

VOCABULARY_SITES = {
    "st_cached": (0x60C3F9, 0x60D3C8),
    "st_streamed": (0x60C406, 0x60D3BC),
    "Expected st_cached or st_streamed": (0x60C3F0,),
    "PURGEABLE": (0x63A000,),
}

PATH_SITES = {
    "SavedGames\\%s\\%s": (0x61F7E0,),
    "Status.dat": (0x626F7B, 0x626F8B, 0x6270D0, 0x627114),
    "location.zrd": (0x628D10, 0x628D38, 0x628D48, 0x628D86, 0x628DC3),
    "dynamics.txt": (0x623A88, 0x623A9C, 0x623ACB),
    "zbench.out": (0x629284,),
    "%s.bmp": (0x6353B0,),
    "PrintUsedTextures": (0x63E708,),
    "Writing gamez.zbd; node %s has action callback defined.": (0x62E548,),
    "Error writing texture directory.": (0x635298,),
    "Source file (%s >> %s) newer than in compiled version": (0x6398E4,),
    "gamez.err": (0x63B8AC,),
    "gamez.out": (0x63B884,),
    "~RCSA~": (0x61F044,),
}

SECTION_ADDRESSES = {
    ".text": 0x1000,
    ".rdata": 0x203000,
    ".data": 0x219000,
    ".idata": 0x61F000,
    ".rsrc": 0x623000,
    ".reloc": 0x629000,
}

#: The retail reader container holds this many compiled string nodes carrying
#: "PURGEABLE" (tag 3, length 9) on the measured installation; the first is at
#: this byte offset.
RETAIL_PURGEABLE_NODES = 2633
RETAIL_FIRST_PURGEABLE_OFFSET = 0xE6635


class OwnerImageFactsTests(unittest.TestCase):
    """Every fact the F15-F finding cites, re-measured from the image."""

    @classmethod
    def setUpClass(cls) -> None:
        path = os.environ.get(probe.IMAGE_ENV_VAR)
        if not path:
            raise unittest.SkipTest(
                f"{probe.IMAGE_ENV_VAR} must name the owner-supplied decrypted "
                "engine image: these facts cannot pass without it"
            )
        cls.facts = probe.analysis_data(
            probe.load_image(path), os.environ.get(probe.GAME_DIR_ENV_VAR)
        )

    def test_accept_f15_f_the_measured_image_and_its_sections(self) -> None:
        self.assertEqual(self.facts["image"]["sha256"], probe.RECORDED_IMAGE_SHA256)
        self.assertEqual(self.facts["image"]["image_base"], IMAGE_BASE)
        sections = {s["name"]: s["virtual_address"] for s in self.facts["image"]["sections"]}
        self.assertEqual(sections, SECTION_ADDRESSES)

    def test_accept_f15_f_every_file_creating_entry_point_reference(self) -> None:
        sites = self.facts["write_api_sites"]
        self.assertEqual(tuple(row["site_va"] for row in sites["CreateFileA"]), CREATE_FILE_SITES)
        self.assertEqual(tuple(row["site_va"] for row in sites["WriteFile"]), WRITE_FILE_SITES)
        self.assertEqual(
            tuple(row["site_va"] for row in sites["CreateDirectoryA"]),
            CREATE_DIRECTORY_SITES,
        )
        self.assertEqual(
            tuple(row["site_va"] for row in sites["GetTempPathA"]),
            GET_TEMP_PATH_SITES,
        )
        self.assertEqual(len(sites["fopen"]), 26)
        self.assertEqual(len(sites["fwrite"]), 45)
        self.assertEqual(len(sites["fprintf"]), 110)
        self.assertEqual(len(sites["tmpfile"]), 2)
        self.assertEqual(len(sites["remove"]), 3)
        self.assertEqual(len(sites["rename"]), 1)
        self.assertEqual(len(sites["_mkdir"]), 3)
        # A memory-mapped or second writable path would need one of these; the
        # probed-absent list also pins every writer the finding names absent.
        self.assertEqual(self.facts["present_probed_absent_apis"], {})
        context = self.facts["context_api_sites"]
        self.assertEqual(
            {api: len(s) for api, s in context.items()},
            {
                "RegCreateKeyExA": 4,
                "RegSetValueExA": 6,
                "RegDeleteKeyA": 2,
                "ShellExecuteA": 1,
                "GetSaveFileNameA": 1,
                "GetPrivateProfileStringA": 1,
                "SetFilePointer": 6,
                "fseek": 29,
                "fflush": 27,
                "fclose": 57,
            },
        )
        # The write-mode opens named in the finding: the labeler must resolve
        # even the one-character modes or the audit's labels say nothing.
        fopen_modes = {
            row["site_va"]: row["pushed_string"] for row in sites["fopen"]
        }
        self.assertEqual(fopen_modes[0x41A7E8], "wb+")
        self.assertEqual(fopen_modes[0x4E2283], "wb+")
        self.assertEqual(fopen_modes[0x521457], "wb")
        self.assertEqual(fopen_modes[0x5BB854], "w")
        self.assertEqual(fopen_modes[0x404769], "r")
        self.assertEqual(fopen_modes[0x5B747E], "r")

    def test_accept_f15_f_the_only_cache_strings_are_sound_residency_names(self) -> None:
        self.assertEqual(
            self.facts["cache_strings"],
            [
                (0x60C3F0, "Expected st_cached or st_streamed"),
                (0x60D3C8, "st_cached"),
            ],
        )
        for text, addresses in VOCABULARY_SITES.items():
            self.assertEqual(tuple(self.facts["vocabulary_strings"][text]), addresses, text)

    def test_accept_f15_f_the_cited_output_paths_and_source_labels(self) -> None:
        for text, addresses in PATH_SITES.items():
            self.assertEqual(tuple(self.facts["path_strings"][text]), addresses, text)
        self.assertEqual(
            tuple(self.facts["source_labels"][probe.ZRDR_SOURCE_LABEL]), (0x6398BC,)
        )
        self.assertEqual(
            tuple(self.facts["source_labels"][probe.ZBD_SOURCE_LABEL]), (0x62E354,)
        )

    def test_accept_f15_f_the_naming_sites_are_the_measured_ones(self) -> None:
        naming = self.facts["naming_sites"]
        # The sound parser reads PURGEABLE as a string child of a sound
        # definition (this push is its comparison operand).
        self.assertEqual(naming["PURGEABLE_pushed_at"], [0x592D85])
        # The zreader staleness warning and the label it logs with.
        self.assertEqual(naming["source_stale_message_pushed_at"], [0x579D7B])
        self.assertIn(0x579D85, naming["zrdr_source_label_pushed_at"])

    def test_accept_f15_f_retail_reader_container_holds_compiled_string_nodes(self) -> None:
        retail = self.facts["retail_reader_container"]
        if not retail or retail.get("present") is False:
            raise unittest.SkipTest(
                f"{probe.GAME_DIR_ENV_VAR} must name the read-only retail "
                f"installation holding {probe.RETAIL_READER_CONTAINER}"
            )
        self.assertEqual(retail["purgeable_node_count"], RETAIL_PURGEABLE_NODES)
        self.assertEqual(retail["purgeable_node_offsets"][0], RETAIL_FIRST_PURGEABLE_OFFSET)


if __name__ == "__main__":
    unittest.main()
