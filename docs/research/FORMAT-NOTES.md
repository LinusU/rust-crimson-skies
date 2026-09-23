# Concrete format notes and safe research recipes

Status: **observed_tool**, not verified_original. Implement only a measured profile; unknown variations remain explicit.

## ROF observed structure [S05]

The inspected extractor begins at a root directory at offset zero. A directory contains two little-endian u32 fields (entry count, name table byte length), a table of 24-byte records and a NUL-separated names block. Each record stores six little-endian u32 values: start, length, length_on_disk, flags, name_length and id. Flag bits 1/2 are used for directory/compression. Directory recursion follows absolute seeks. Compressed members are passed to zlib.

**Unresolved:** the reference uses `length` as compressed read length and ignores `length_on_disk`. Do not assign stored/unpacked semantics from English field names. Preserve both values, compare against stream boundaries and independent extraction on at least two members whose compressed and decoded lengths differ. Also validate empty entries, nested trees and padding. The synthetic fixture deliberately tests only the uncompressed observed subset.

The production reader adds bounds checks, count/size overflow protection, recursion/cycle limits, duplicate handling and path safety absent from a quick extraction script. Never write source-controlled retail extraction output.

## BM observed subset [S09, S10]

Header: little-endian u16 **height**, then u16 **width**. Let N = width*height. Payload planes: RGB base (3N), mask1 (N), mask2 (N), mask3 (N), RGBA overlay (4N). Total is 4+10N. The tool flips exported planes vertically. Its composition applies three color-mask multiplications then RGBA alpha composition. The last plane's name in the script does not prove physical specular semantics.

Use the supplied asymmetric synthetic fixture for size/order/channel tests. Compare decoding independently from color composition and from GPU gamma/alpha. Evaluate mask endpoint behavior and preserve per-faction/per-instance variants. Do not bake all planes into one default faction.

## INTERP observed subset [S07]

Header: u32 signature `0x08971119`, u32 version `7`, u32 script count. Index entry: 120-byte padded name, u32 timestamp and u32 offset (128 bytes). A script line is u32 byte size, u32 argument/NUL count, then that many bytes. A zero size ends a script. The inspected reader checks NUL count against argument count and joins NUL-separated data for its output.

The new parser retains raw token boundaries instead of irreversibly joining strings. Validate entry extents, termination, offsets, encoding and unused tails. This container does not by itself identify the full mission language or native functions.

## GameZ/planes and images [S02, S03, S08]

Use the pinned CS-capable legacy source as an inspection reference. It exposes scene nodes, meshes and material records; polygons carry per-corner attributes and may use triangle strips. Avoid transplanting Blender coordinate transforms or lossy face cleanup. Texture aliases and duplicate names need context-preserving resolution. Some material-field interpretation changed across source versions.

Before each parser slice, make a field worksheet: offset/size, primitive representation, observed constraints, source revision, sample count, meaning/confidence and consumers. A fixture whose writer and reader share the same wrong assumption is not independent validation. Byte-exact round trip is useful but does not prove semantic meaning.

## Suggested private reference workflow

1. Clone the open-source reference to a separate research directory and resolve the exact commit. Review its license before building or copying anything.
2. Build its documented CLI from that revision, or verify the identity of an appropriate released binary. Do not silently download and run the latest executable.
3. Read the owner's installation only; write extracts into a private directory outside this repository.
4. For covered operations use the **verified CLI from that revision**, such as `unzbd cs gamez <PLANES.ZBD> <private-output.zip>` and `unzbd cs textures <texture.zbd> <private-output.zip>`. Validate help/arguments locally first.
5. Compare fields, members and decoded arrays; keep full output private. Record version, command, inputs, exit status and discrepancies in the ledger.
6. A reference error or unsupported operation is a research blocker, not permission to declare the original file invalid or skip the affected mission.

No external source implementation or executable is included in this pack. These notes are independently authored engineering descriptions.
