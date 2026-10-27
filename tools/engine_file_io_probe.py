#!/usr/bin/env python3
"""Read-only static probe: what does the original engine write, and cache?

Task #403 / F15-F.  The question this probe answers is narrow and factual:

* which file-creating or file-writing Win32/MSVCRT entry points the
  owner-supplied decrypted engine image (`$CS_ENGINE_IMAGE`) imports;
* where in the image each of them is referenced, and in which function;
* which literal path and open-mode strings sit next to those references;
* where the engine's cache/residency vocabulary lives, and where the retail
  data's `PURGEABLE` sound attribute is parsed.

It does **not** disassemble the image and it prints no instruction bytes or
decompiled code.  It reports addresses, API names and short string literals —
facts a finding may cite (owner exception of 2026-10-14).  Nothing is written:
the image is opened `rb`, hashed, and only read.  `$CS_GAME_DIR`, when set, is
only used to check that the retail reader container's bytes contain the
compiled node tags the loader consumes.

Usage:

    python3 tools/engine_file_io_probe.py --image "$CS_ENGINE_IMAGE"
    python3 tools/engine_file_io_probe.py --image "$CS_ENGINE_IMAGE" --json

Exit status is 0 when the analysis ran (whatever it found), 2 when the image is
missing, unreadable or hashed to bytes other than the measured image, and 3 on a
malformed PE.  A finding must never be derived from an image that fails the
digest guard.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import struct
import sys
from dataclasses import dataclass, field
from typing import Iterable

# ---------------------------------------------------------------------------
# The measured input
# ---------------------------------------------------------------------------

IMAGE_ENV_VAR = "CS_ENGINE_IMAGE"
GAME_DIR_ENV_VAR = "CS_GAME_DIR"

#: SHA-256 of the owner-supplied decrypted image every address below was
#: measured in (the same constant the F16-E production helper checks).
RECORDED_IMAGE_SHA256 = (
    "43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75"
)

#: PE image base: `va == file offset + 0x400000` for the first three sections
#: of this image; later sections (`.idata`, `.rsrc`, `.reloc`) sit at higher
#: virtual addresses, so all access goes through the section table.
IMAGE_VA_BASE = 0x400000

#: File-creating / writing entry points.  `WriteFile`-family and MSVCRT
#: `fwrite` are both audited: the engine is MSVC and does both.
WRITE_APIS: tuple[str, ...] = (
    "CreateFileA",
    "WriteFile",
    "CreateDirectoryA",
    "MoveFileA",
    "DeleteFileA",
    "RemoveDirectoryA",
    "GetTempPathA",
    "SetFileAttributesA",
    "fopen",
    "fwrite",
    "tmpfile",
    "freopen",
    "remove",
    "rename",
    "_mkdir",
    "_rmtmp",
    "_chmod",
)

#: Entry points whose *absence* from the import table matters to the finding:
#: a memory-mapped persistent store would need `CreateFileMappingA/W`.
PROBED_ABSENT_APIS: tuple[str, ...] = (
    "CreateFileMappingA",
    "CreateFileMappingW",
    "MapViewOfFile",
)

#: The cache/residency vocabulary, by exact string.  These are the only strings
#: in the whole image that contain "cache" (see `analysis`'s `cache_strings`),
#: which is why the finding can say the engine's use of the word is about
#: sound residency and not about a persistent derived-asset store.
VOCABULARY_STRINGS: tuple[str, ...] = (
    "st_cached",
    "st_streamed",
    "Expected st_cached or st_streamed",
    "PURGEABLE",
    "volatile",
    "nonvolatile",
)

#: Output-path and mode strings the finding cites.  Each is checked to occur
#: in the image; the probe reports every virtual address it occurs at.
PATH_STRINGS: tuple[str, ...] = (
    "SavedGames\\%s\\%s",
    "SavedGames\\%s\\AutoSave.sav",
    "SavedGames\\*",
    "%s\\%s.sav",
    "Status.dat",
    "location.zrd",
    "location.bak",
    "dynamics.txt",
    "zbench.out",
    "%s.bmp",
    "Save Captured Image",
    "%1!s!\\Snap_%2!d!_",
    "%1!d!.png",
    "PrintUsedTextures",
    "Writing gamez.zbd; node %s has action callback defined.",
    "Error writing texture directory.",
    "Source file (%s >> %s) newer than in compiled version",
    "Cannot find file '%s'",
    "Problem reading file '%s'",
    "gamez.err",
    "gamez.out",
    "~RCSA~",
)

#: The zreader node reader's own source label and the sound-set token the
#: parser consumes.  Their citing addresses are asserted by the acceptance
#: test, so a drifted image cannot keep backing these claims.
ZRDR_SOURCE_LABEL = "D:\\zipper\\gamez\\zreader\\zrdr_main.cpp"
ZBD_SOURCE_LABEL = "D:\\zipper\\gamez\\zclass\\cls_zbd.c"

#: The retail reader container and the compiled-node byte pattern the finding
#: measures in it: tag 3 (string node), length 9, "PURGEABLE".  The loader reads
#: exactly those tags (`zRdrRead`, VA 0x57a630).
RETAIL_READER_CONTAINER = os.path.join("ZBD", "zrdr.zbd")
COMPILED_STRING_NODE_TAG = 3
PURGEABLE_STRING_NODE = struct.pack("<I", COMPILED_STRING_NODE_TAG) + struct.pack(
    "<I", len("PURGEABLE")
) + b"PURGEABLE"


class ProbeError(Exception):
    """The image cannot back this finding."""


class PeError(ProbeError):
    """The bytes are not a PE32 image the probe can read."""


# ---------------------------------------------------------------------------
# PE parsing (just enough: sections, import names, IAT addresses)
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class Section:
    name: str
    virtual_address: int
    virtual_size: int
    raw_offset: int
    raw_size: int

    def contains_rva(self, rva: int) -> bool:
        return self.virtual_address <= rva < self.virtual_address + max(
            self.virtual_size, self.raw_size
        )


@dataclass
class PeImage:
    """A PE32 image in memory, with the lookups this probe needs."""

    data: bytes
    image_base: int
    sections: list[Section]
    imports: dict[str, int] = field(default_factory=dict)
    import_libraries: dict[str, str] = field(default_factory=dict)

    # -- construction -------------------------------------------------------

    @classmethod
    def parse(cls, data: bytes) -> "PeImage":
        if len(data) < 0x40 or data[:2] != b"MZ":
            raise PeError("not a DOS/PE image: no MZ header")
        e_lfanew = struct.unpack_from("<I", data, 0x3C)[0]
        if e_lfanew + 24 > len(data) or data[e_lfanew : e_lfanew + 4] != b"PE\0\0":
            raise PeError("no PE signature at e_lfanew")
        coff = e_lfanew + 4
        number_of_sections = struct.unpack_from("<H", data, coff + 2)[0]
        size_of_optional_header = struct.unpack_from("<H", data, coff + 16)[0]
        optional = coff + 20
        if optional + size_of_optional_header > len(data):
            raise PeError("optional header runs past the end of the image")
        magic = struct.unpack_from("<H", data, optional)[0]
        if magic != 0x10B:
            raise PeError(f"not PE32: optional-header magic {magic:#x}")
        image_base = struct.unpack_from("<I", data, optional + 28)[0]

        section_table = optional + size_of_optional_header
        sections: list[Section] = []
        for index in range(number_of_sections):
            entry = section_table + index * 40
            if entry + 40 > len(data):
                raise PeError("section table runs past the end of the image")
            raw_name = data[entry : entry + 8].split(b"\0", 1)[0]
            name = raw_name.decode("ascii", "replace")
            virtual_size, virtual_address, raw_size, raw_offset = struct.unpack_from(
                "<IIII", data, entry + 8
            )
            sections.append(
                Section(name, virtual_address, virtual_size, raw_offset, raw_size)
            )

        image = cls(data, image_base, sections)
        image._parse_imports(optional, size_of_optional_header)
        return image

    def _parse_imports(self, optional: int, size_of_optional_header: int) -> None:
        # PE32 data directory 1 (import table) sits at optional-header + 96 + 8.
        if size_of_optional_header < 104:
            return
        import_rva, import_size = struct.unpack_from("<II", self.data, optional + 96 + 8)
        if import_rva == 0:
            return
        offset = self.rva_to_offset(import_rva)
        if offset is None:
            return
        while True:
            descriptor = self.data[offset : offset + 20]
            if len(descriptor) < 20:
                return
            (
                original_first_thunk,
                _timestamp,
                _forwarder,
                name_rva,
                first_thunk,
            ) = struct.unpack("<IIIII", descriptor)
            if original_first_thunk == 0 and name_rva == 0 and first_thunk == 0:
                return
            library = self.read_c_string(self.image_base + name_rva) or "?"
            thunk_rva = original_first_thunk or first_thunk
            thunk_offset = self.rva_to_offset(thunk_rva)
            if thunk_offset is None:
                offset += 20
                continue
            index = 0
            while True:
                (thunk,) = struct.unpack_from("<I", self.data, thunk_offset + index * 4)
                if thunk == 0:
                    break
                if thunk & 0x80000000:
                    name = f"ordinal_{thunk & 0xFFFF}"
                else:
                    name = self.read_c_string(self.image_base + thunk + 2) or "?"
                # The IAT slot is what code references.
                self.imports.setdefault(name, self.image_base + first_thunk + index * 4)
                self.import_libraries.setdefault(name, library)
                index += 1
            offset += 20

    # -- lookups ------------------------------------------------------------

    def section_of_rva(self, rva: int) -> Section | None:
        for section in self.sections:
            if section.contains_rva(rva):
                return section
        return None

    def rva_to_offset(self, rva: int) -> int | None:
        section = self.section_of_rva(rva)
        if section is None:
            return None
        delta = rva - section.virtual_address
        if delta >= section.raw_size:
            return None
        return section.raw_offset + delta

    def offset_to_va(self, offset: int) -> int | None:
        for section in self.sections:
            if section.raw_offset <= offset < section.raw_offset + section.raw_size:
                return self.image_base + section.virtual_address + (
                    offset - section.raw_offset
                )
        return None

    def read(self, offset: int, length: int) -> bytes:
        return self.data[offset : offset + length]

    def read_c_string(self, va: int, limit: int = 200) -> str | None:
        offset = self.rva_to_offset(va - self.image_base)
        if offset is None:
            return None
        end = self.data.find(b"\0", offset, offset + limit)
        if end < 0:
            return None
        try:
            return self.data[offset:end].decode("ascii")
        except UnicodeDecodeError:
            return None

    def find_string_bytes(self, needle: bytes | str) -> list[int]:
        """Every virtual address whose bytes are exactly `needle`."""
        raw = needle.encode("ascii") if isinstance(needle, str) else needle
        addresses = []
        start = 0
        while True:
            index = self.data.find(raw, start)
            if index < 0:
                break
            va = self.offset_to_va(index)
            if va is not None:
                addresses.append(va)
            start = index + 1
        return addresses

    def strings_containing(self, needle: bytes | str) -> list[tuple[int, str]]:
        """Every NUL-terminated printable string that contains `needle`."""
        raw = needle.encode("ascii") if isinstance(needle, str) else needle
        found: list[tuple[int, str]] = []
        for offset in self._occurrences(raw):
            # Walk back to the start of the string and forward to its end.
            begin = offset
            while begin > 0 and 0x20 <= self.data[begin - 1] <= 0x7E:
                begin -= 1
                if offset - begin > 200:
                    break
            end = self.data.find(b"\0", offset)
            if end < 0 or end - begin > 200:
                continue
            try:
                text = self.data[begin:end].decode("ascii")
            except UnicodeDecodeError:
                continue
            va = self.offset_to_va(begin)
            if va is not None and (va, text) not in found:
                found.append((va, text))
        return found

    def _occurrences(self, needle: bytes) -> Iterable[int]:
        start = 0
        while True:
            index = self.data.find(needle, start)
            if index < 0:
                return
            yield index
            start = index + 1

    # -- x86 references (patterns, not a disassembler) ----------------------

    def references_to(self, address: int) -> list[tuple[int, str]]:
        """Code sites that name `address` as an IAT slot.

        Returns `(site_va, kind)` where kind is `"call"` for
        `call dword ptr [address]` (`FF 15`), `"load"` for a register load
        (`8B /r` with mod=00 rm=101, or `A1`) that a later `call reg` reaches,
        and `"immediate"` for any other 4-byte occurrence in the text.
        The direct-call form is exact; the other two are pattern matches whose
        sites the finding lists individually so they can be checked by hand.
        """
        encoded = struct.pack("<I", address)
        sites: list[tuple[int, str]] = []
        text = next((s for s in self.sections if s.name == ".text"), None)
        if text is None:
            return sites
        start = text.raw_offset
        end = text.raw_offset + text.raw_size
        index = self.data.find(encoded, start, end)
        while index >= 0:
            if index >= 2 and self.data[index - 2] == 0xFF and self.data[index - 1] == 0x15:
                kind = "call"
            elif index >= 2 and self.data[index - 2] == 0xFF and self.data[index - 1] == 0x25:
                kind = "jump"
            elif index >= 2 and self.data[index - 2] in (0x8B, 0x8D) and (
                self.data[index - 1] & 0xC7
            ) == 0x05:
                kind = "load"
            elif index >= 1 and self.data[index - 1] == 0xA1:
                kind = "load"
            else:
                kind = "immediate"
            va = self.offset_to_va(index - (2 if kind in ("call", "jump") else 1))
            if va is None:
                va = self.offset_to_va(index)
            if va is not None:
                sites.append((va, kind))
            index = self.data.find(encoded, index + 1, end)
        return sites

    def push_immediates(self, value: int) -> list[int]:
        """Every `push imm32` in `.text` whose immediate is `value`.

        This is how the probe locates a code site that *names* a string, e.g.
        the sound parser's `PURGEABLE` comparison, without disassembling.
        """
        text = next((s for s in self.sections if s.name == ".text"), None)
        if text is None:
            return []
        pattern = b"\x68" + struct.pack("<I", value)
        sites = []
        start = text.raw_offset
        end = text.raw_offset + text.raw_size
        index = self.data.find(pattern, start, end)
        while index >= 0:
            va = self.offset_to_va(index)
            if va is not None:
                sites.append(va)
            index = self.data.find(pattern, index + 1, end)
        return sites

    def push_string_before(self, site: int, window: int = 72) -> tuple[int, str] | None:
        """The closest `push imm32` naming an ASCII string before `site`.

        This is the probe's only heuristic and it is used solely to label a
        call site with the literal it passes; the site address itself is exact.
        The scan stops at a call, a return, or the start of the window.
        """
        offset = self.rva_to_offset(site - self.image_base)
        if offset is None:
            return None
        begin = max(0, offset - window)
        index = offset - 5
        while index >= begin:
            if self.data[index] == 0x68:
                value = struct.unpack_from("<I", self.data, index + 1)[0]
                text = self.read_c_string(value)
                if text and text.isprintable() and len(text) >= 2:
                    return value, text
            if self.data[index] in (0xC3, 0xCB) or (
                self.data[index] == 0xFF and self.data[index - 1] in (0x15, 0xD0, 0xD1, 0xD2, 0xD3)
            ):
                return None
            index -= 1
        return None

    def containing_function(self, site: int, limit: int = 8192) -> int | None:
        """Heuristic function start: after the nearest >=2-byte int3/nop run."""
        offset = self.rva_to_offset(site - self.image_base)
        if offset is None:
            return None
        low = max(0, offset - limit)
        index = offset - 1
        while index > low:
            if self.data[index] in (0xCC, 0x90) and self.data[index - 1] in (0xCC, 0x90):
                forward = index
                while forward < offset and self.data[forward] in (0xCC, 0x90):
                    forward += 1
                return self.offset_to_va(forward)
            index -= 1
        return None


# ---------------------------------------------------------------------------
# The analysis
# ---------------------------------------------------------------------------


def load_image(path: str) -> PeImage:
    with open(path, "rb") as handle:
        data = handle.read()
    verify_measured_bytes(path, data)
    return PeImage.parse(data)


def verify_measured_bytes(path: str, data: bytes) -> None:
    """Refuse bytes that are not the image every recorded address was read in."""
    digest = hashlib.sha256(data).hexdigest()
    if digest != RECORDED_IMAGE_SHA256:
        raise ProbeError(
            f"{path} hashes to {digest}, not to the measured {RECORDED_IMAGE_SHA256}: "
            "it cannot back evidence about the original"
        )


def analysis_data(image: PeImage, game_dir: str | None = None) -> dict:
    """The measured facts the finding cites, as plain data."""
    api_sites: dict[str, list[dict]] = {}
    other_reference_counts: dict[str, int] = {}
    for api in WRITE_APIS:
        address = image.imports.get(api)
        if address is None:
            api_sites[api] = []
            other_reference_counts[api] = 0
            continue
        rows = []
        others = 0
        for site, kind in image.references_to(address):
            if kind == "immediate":
                others += 1
                continue
            pushed = image.push_string_before(site)
            rows.append(
                {
                    "site_va": site,
                    "kind": kind,
                    "function_va": image.containing_function(site),
                    "pushed_string_va": pushed[0] if pushed else None,
                    "pushed_string": pushed[1] if pushed else None,
                }
            )
        api_sites[api] = rows
        other_reference_counts[api] = others

    vocabulary = {}
    for text in VOCABULARY_STRINGS:
        vocabulary[text] = image.find_string_bytes(text)

    path_addresses = {}
    for text in PATH_STRINGS:
        path_addresses[text] = image.find_string_bytes(text)

    labels = {}
    for text in (ZRDR_SOURCE_LABEL, ZBD_SOURCE_LABEL):
        labels[text] = image.find_string_bytes(text)

    absent = {
        api: image.imports.get(api) for api in PROBED_ABSENT_APIS if api in image.imports
    }

    cache_strings = image.strings_containing("cache")

    naming_sites = {
        "PURGEABLE_pushed_at": image.push_immediates(vocabulary["PURGEABLE"][0]),
        "zrdr_source_label_pushed_at": image.push_immediates(labels[ZRDR_SOURCE_LABEL][0]),
        "source_stale_message_pushed_at": image.push_immediates(
            path_addresses["Source file (%s >> %s) newer than in compiled version"][0]
        ),
        "zbd_source_label_pushed_at": image.push_immediates(labels[ZBD_SOURCE_LABEL][0]),
    }

    retail = None
    if game_dir:
        container = os.path.join(game_dir, RETAIL_READER_CONTAINER)
        if os.path.exists(container):
            with open(container, "rb") as handle:
                container_bytes = handle.read()
            offsets = []
            start = 0
            while True:
                index = container_bytes.find(PURGEABLE_STRING_NODE, start)
                if index < 0:
                    break
                offsets.append(index)
                start = index + 1
            retail = {
                "container": RETAIL_READER_CONTAINER,
                "size": len(container_bytes),
                "purgeable_node_offsets": offsets[:8],
                "purgeable_node_count": len(offsets),
                "compiled_string_node_tag": COMPILED_STRING_NODE_TAG,
            }
        else:
            retail = {"container": RETAIL_READER_CONTAINER, "present": False}

    return {
        "image": {
            "sha256": RECORDED_IMAGE_SHA256,
            "image_base": image.image_base,
            "sections": [
                {
                    "name": section.name,
                    "virtual_address": section.virtual_address,
                    "virtual_size": section.virtual_size,
                    "raw_offset": section.raw_offset,
                    "raw_size": section.raw_size,
                }
                for section in image.sections
            ],
        },
        "write_api_sites": api_sites,
        "other_reference_counts": other_reference_counts,
        "vocabulary_strings": vocabulary,
        "path_strings": path_addresses,
        "source_labels": labels,
        "present_probed_absent_apis": absent,
        "cache_strings": cache_strings,
        "naming_sites": naming_sites,
        "retail_reader_container": retail,
    }


def summarise(facts: dict) -> str:
    """A human-readable digest of `facts` (no instruction bytes, no code)."""
    lines: list[str] = []
    image = facts["image"]
    lines.append(f"image sha256 {image['sha256']} (matches the measured image)")
    lines.append(f"image base {image['image_base']:#x}")
    lines.append("sections: " + ", ".join(
        f"{s['name']} {s['virtual_address']:#x}+{s['virtual_size']:#x}"
        for s in image["sections"]
    ))
    lines.append("")
    lines.append("write/creation entry points:")
    for api, rows in facts["write_api_sites"].items():
        if not rows:
            lines.append(f"  {api}: not imported or not referenced")
            continue
        lines.append(f"  {api}: {len(rows)} reference(s)")
        for row in rows:
            suffix = ""
            if row["pushed_string"]:
                suffix = f"  [{row['pushed_string_va']:#x} {row['pushed_string']!r}]"
            function = f"{row['function_va']:#x}" if row["function_va"] else "?"
            lines.append(
                f"    {row['site_va']:#x} {row['kind']:<5} in {function}{suffix}"
            )
        if facts["other_reference_counts"].get(api):
            lines.append(
                f"    (+{facts['other_reference_counts'][api]} unclassified 4-byte occurrence(s))"
            )
    lines.append("")
    lines.append("probed-absent entry points (would be needed by a memory-mapped store):")
    lines.append(
        "  " + (", ".join(
            f"{api} {va:#x}" for api, va in facts["present_probed_absent_apis"].items()
        ) or "none imported")
    )
    lines.append("")
    lines.append("cache/residency vocabulary:")
    for text, addresses in facts["vocabulary_strings"].items():
        lines.append(f"  {text!r}: {[hex(a) for a in addresses]}")
    lines.append("")
    lines.append("every string containing 'cache':")
    for va, text in facts["cache_strings"]:
        lines.append(f"  {va:#x} {text!r}")
    lines.append("")
    lines.append("output-path and mode strings:")
    for text, addresses in facts["path_strings"].items():
        lines.append(f"  {text!r}: {[hex(a) for a in addresses]}")
    lines.append("")
    lines.append("source labels:")
    for text, addresses in facts["source_labels"].items():
        lines.append(f"  {text!r}: {[hex(a) for a in addresses]}")
    lines.append("")
    lines.append("code sites that name those strings (push imm32):")
    for label, sites in facts["naming_sites"].items():
        lines.append(f"  {label}: {[hex(a) for a in sites]}")
    retail = facts["retail_reader_container"]
    lines.append("")
    if retail is None:
        lines.append("retail reader container: not inspected (CS_GAME_DIR unset)")
    elif retail.get("present") is False:
        lines.append(f"retail reader container: {retail['container']} not found")
    else:
        lines.append(
            f"retail reader container {retail['container']} ({retail['size']} bytes): "
            f"{retail['purgeable_node_count']} compiled string node(s) tagged "
            f"{retail['compiled_string_node_tag']} holding 'PURGEABLE', first at "
            f"{[hex(o) for o in retail['purgeable_node_offsets']]}"
        )
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--image",
        default=os.environ.get(IMAGE_ENV_VAR),
        help=f"the owner-supplied decrypted engine image (default: ${IMAGE_ENV_VAR})",
    )
    parser.add_argument(
        "--game-dir",
        default=os.environ.get(GAME_DIR_ENV_VAR),
        help=f"the read-only retail installation (default: ${GAME_DIR_ENV_VAR})",
    )
    parser.add_argument("--json", action="store_true", help="print the facts as JSON")
    args = parser.parse_args(argv)
    if not args.image:
        print(
            f"{IMAGE_ENV_VAR} is not set: it must name the owner-supplied "
            "decrypted engine image (crimson.decrypted.exe)",
            file=sys.stderr,
        )
        return 2
    try:
        image = load_image(args.image)
    except OSError as error:
        print(f"cannot read {args.image}: {error}", file=sys.stderr)
        return 2
    except PeError as error:
        print(f"{args.image} is not a PE32 image this probe can read: {error}", file=sys.stderr)
        return 3
    except ProbeError as error:
        print(str(error), file=sys.stderr)
        return 2
    facts = analysis_data(image, args.game_dir)
    if args.json:
        print(json.dumps(facts, indent=2, sort_keys=True))
    else:
        print(summarise(facts))
    return 0


if __name__ == "__main__":
    sys.exit(main())
