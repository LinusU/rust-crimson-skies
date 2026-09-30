# F17-B follow-up: the Rgb565 expansion policy and the coverage-key policy

Date: 2026-09-30. Task: #408 "Decide and implement the Rgb565 texel
expansion policy for the renderer adapter"
(`F17-B-followup-rgb565-expansion`), the follow-up F17-B raised from
`docs/findings/2026-09-30-f17-b-canonical-mesh-and-image-to-bevy.md`.
Sheets: `specs/F17-rendering-material-fidelity-and-scalable-presentation.md`
(whole sheet, stages `### F17-B` and `### F17-D`); shared contract
`docs/contracts/IDENTITY-CONTENT.md`. Test prefix: `accept_f17_b_rgb565_`.
Capabilities used: ordinary build/test for the ten synthetic tests, and
read-only `retail` (`$CS_GAME_DIR`) for the one census test. No evidence
report: the task's evidence is a decision plus a count, not a
fingerprinted original observation, and nothing here is
`verified_original`.

## The gap this closes

F17-B's `cs_app::render::bevy_image::upload_image` refused three
presentation facts, and this task decides two of them:

| F17-B refusal | Decided here | Where |
| --- | --- | --- |
| `ColorSpaceUnknown` | no — the ZBD reader declares `ColorSpace::Unknown` for every package row and the original's color space is still unmeasured | F17-D |
| `AlphaSourceUnknown` | no — `AlphaSource::Unknown` names no plane | still unknown; F17-D |
| `AlphaTestUnknown` | no — the ZBD reader declares `AlphaTest::Unknown` for every package row | F17-A facts / F17-D |
| `AddressModeUnknown` | no — the material never declared addressing | F17-D |
| **`Rgb565ExpansionUnknown`** | **yes — removed; a 565 image uploads** | this task |
| **`PaletteKeyCoverage`** | **yes — removed; the key reaches the alpha channel** | this task |
| **`StoredValueKeyCoverage`** | **yes — removed; the key reaches the alpha channel** | this task |

The expansion and the two keys are decided *as a policy*, in
`crates/cs_app/src/render/rgb565.rs`, which is Bevy-free and holds no
Bevy asset. `cs_app::render::bevy_image` applies it: every texel's color
and alpha now come from that module, and the three refusals above are gone
from `ImageAdapterError`. What the adapter still refuses is the four
decisions nobody has made.

## Decision 1: the expansion is `designed`, not an F17-D gate

**The claim, and its class.** Given the established 5/6/5 layout of a
stored word (`cs_formats::texture::PixelFormat::Rgb565`), widening an
*n*-bit unsigned channel to eight bits is bit replication,
`(level << (8 - n)) | (level >> n)`. **Claim class:
`ClaimStatus::Designed`** — the same kind of claim as F17-A's
class-to-phase map, a new-engine design decision over a *format*
property.

**Why it needs no original-run evidence to be usable.** Three
reasons, in order of weight:

1. It is a property of the *format*, not of the game. `R5G6B5` is a
   Direct3D 7/8 hardware format; "sample it as 8 bits per channel" has
   one standard answer. There is no per-content choice, no tuning table
   and no variant to look up, so there is nothing an original run could
   disambiguate *case by case*.
2. It is the only candidate besides the fixed-point scale that reaches
   both endpoints exactly. `Rule::reaches_white` is the edge: 31 → 255
   and 63 → 255, or the rule cannot represent a full-scale channel.
3. F17-B's rule is that an *unestablished* fact becomes a refusal. This
   fact is established — as a format definition — so refusing it was
   over-cautious, and the refusal covered 100% of the texture set.

**What stays unmeasured, and how far it can move.** Whether the
original uploaded the words and let the card expand, or expanded them in
its own loader, and whether a loader expansion rounded differently, is
**unknown** and stays unknown. The honest mitigation is not a claim but
a *bound*, computed over the whole input domain by
`Rule::max_channel_deviation`:

| Pair | Worst per-channel difference |
| --- | --- |
| replication vs fixed-point scale | **1/255** |
| replication vs truncation | **7/255** (5-bit), 3/255 (6-bit) |
| fixed-point scale vs truncation | 7/255 |

So the two rules that both represent white cannot differ by more than
1/255 on any channel of any word, and the one rule that differs more is
excluded on a separate ground: truncation maps `0xFFFF` to
`(248, 252, 248)`, so it cannot render white, and the installation
stores **6,009,498 saturated 565 texels** (3.6% of all texels), which
a truncation expansion would have rendered as dull off-white. That is an
*inference from the original data* — `ClaimStatus::Inferred`, not
`VerifiedOriginal` — and it excludes truncation; it does not confirm
replication over the fixed-point scale, which remains a ≤1/255 unknown
for F17-D.

**Gamma-corrected expansion is not a candidate.** A gamma correction is
a choice about a *transfer function*; a 565 word stores no transfer
function. Correcting one would double-correct the sampler, which spec F17
non-negotiable 3 forbids. It is named here so nobody adds it later.

**The sRGB interaction, stated because a reviewer will ask.** Spec F17
non-negotiable 3 says decoding and GPU sampling must not double-correct.
The expansion does not affect that, and the order is the same on both
routes: a 5/6/5 word is widened to 8-bit unorm (by a card's expander on
the original, by [`Rule::Replication`] here), and *then* the sRGB unit
decodes. Uploading the widened bytes as `Rgba8UnormSrgb` therefore
reproduces "widen, then decode" exactly; it does not decode first and
widen afterwards, which is the mistake that would double-correct. Note
this is still gated behind the color-space refusal: the ZBD reader
declares `ColorSpace::Unknown` for all 37,004 package rows, so no upload
format is chosen for any of them yet.

## Decision 2: both coverage keys survive, and neither needs a decision

F08's decoder already does the hard part. The `AlphaSource::PaletteKey`
doc says the index plane is kept "so palette-key transparency can be
evaluated at presentation instead of being baked in here", and
`DecodedImage::index` / `DecodedImage::texel565` are the two retained
views of the stored bytes. So a key is read on the CPU while its plane
is intact and becomes a coverage **byte**, which is all a GPU alpha
channel is.

| Stored key | Read from | Coverage byte |
| --- | --- | --- |
| `StoredValueKey { value }` | `DecodedImage::texel565` | 255 if the stored word differs, 0 if it equals |
| `PaletteKey { index }` | `DecodedImage::index` | 255 if the stored index differs, 0 if it equals |

F17-B's stated reason for refusing was that "the index plane is not a
GPU input". It is not, and it does not need to be. This is the same
composition the adapter already performs for a stored alpha channel and
a separate coverage plane, and it is **not** the forbidden baking of
non-negotiable #1: nothing is written into a color texel, and
`accept_f17_b_rgb565_a_stored_word_key_marks_exactly_the_texels_storing_the_key`
asserts the keyed texel still stores its own word afterwards.

**The property that makes the expansion decision safe for keyed rows:**
a key is compared on the *stored* value, never on the expanded one, so a
coverage result provably cannot depend on `Rule`. `expand_texel` reads
the two halves separately for that reason, and
`accept_f17_b_rgb565_a_coverage_key_does_not_depend_on_the_expansion_rule`
holds every non-opaque source against every rule, and then shows a rule
that really does change the color (`Truncation`) leaving every alpha
where it was.

**The honest limit of that property, found by mutation probe.** The two
orders are *currently the same function*. Every rule in [`Rule::ALL`]
happens to be **injective** — checked over all 32 and all 64 levels by
`Rule::is_injective` and again by brute force in
`..._the_deviation_between_rules_is_computed_and_bounded` — so
`expand(w1) == expand(w2)` holds exactly when `w1 == w2`. A mutation that
made `StoredValueKey` coverage compare `expand(word)` against
`expand(key)` therefore **passed the whole suite**; it is a semantic
no-op, and no test in this task can tell it from the stored-value
compare. The stored-value compare is kept anyway, because injectivity is
not something the format guarantees to a rule written later: a
non-injective widening (a gamma curve, a tone curve) would make the two
orders disagree, and the stored value is the one F08 says the key is
stated in terms of. This is recorded so nobody later claims a test pins
that choice.

**And the key stays exact where a color-first order would lose it.** A
565 palette may map two indices to the same word, and then
`index == key` is *not* `color == palette[key]`. This is not
hypothetical: **1,506 of the 6,125 retail palettes hold a duplicate
word, and up to 3 indices share one word.** A resolve-then-compare order
would be wrong for a quarter of the real palettes. The index plane is
the only faithful source, which is why
`accept_f17_b_rgb565_a_palette_index_key_survives_a_duplicate_palette_entry`
uses a palette whose entries 0 and 4 are the same word and requires the
keyed index to be transparent and its twin opaque.

## Retail census: what the decision reaches and what it does not

Read-only over `$CS_GAME_DIR` by
`accept_f17_b_rgb565_retail_565_rows_and_coverage_keys_are_counted`
(`#[ignore = "requires CS_GAME_DIR"]`, ~60 s). 49 `ZBD` texture archives,
**37,004 package textures, all of them 565-packed** — directly, or through
a local 565 palette, because `DecodedFormat::Rgb565` covers both. That is
why the expansion refusal was not a corner case: it blocked the whole
texture set.

| Rows (direct 565) | Count | Coverage | After this decision |
| --- | --- | --- | --- |
| `NO_ALPHA` | 15,399 | opaque | expansion decided; still gated on color space + addressing |
| full alpha plane | 15,343 | stored plane | as above, plus the unestablished alpha test |
| `HAS_ALPHA` simple | 137 | `StoredValueKey 0x0000` | as above; the key itself is now representable |
| Rows (local 565 palette) | Count | Coverage | |
| `NO_ALPHA` | 3,089 | opaque | as above |
| full alpha plane | 3,014 | stored plane | as above, plus the alpha test |
| `HAS_ALPHA` simple | 22 | **`AlphaSource::Unknown`** | **still refused**: no plane names the covered texels |
| **`PaletteKey` rows** | **0** | — | the refusal costs no retail content today |
| **`StoredValueKey` rows** | **137** | | the decision's reason for existing |

| Measured | Value |
| --- | --- |
| 565 texels in the installation | 166,547,780 |
| distinct 565 words used | 31,679 |
| saturated texels (a channel at full scale) | 6,009,498 (3.6%) |
| texels where replication and truncation differ | 128,851,813 (77.4%) |
| texels where replication and the fixed-point scale differ (by 1/255) | 39,459,393 (23.7%) |
| texels that *are* the stored key, across the 137 keyed rows | 303,063 (≈2,212 per row) |
| 565 palettes | 6,125 |
| palettes holding a duplicate word | 1,506 (24.6%), up to 3 indices sharing a word |
| palette entries where the two rules differ | 862,331 |

**The honest reading of that table.** The expansion choice is *not*
inconsequential: 77.4% of retail texels sit at a level where the rules
disagree. What makes the decision acceptable is that the disagreement is
bounded (≤1/255 between the two rules that can represent white, and
truncation is excluded on the saturated-texel evidence) and that no
decision can change a coverage answer. A reviewer who wants the residual
risk closed must run F17-D; this task does not claim that.

**This decision unblocks no row on its own.** Every one of the 37,004
package rows is *separately* gated: the ZBD reader declares
`ColorSpace::Unknown` and `AlphaTest::Unknown` for all of them, which the
census asserts as a single measured row (`("unknown", "unknown") → 37,004`).
Deciding the expansion is **necessary** for a 565 row to reach a GPU and
is **not sufficient**. F17-B's other four refusals are untouched by this
task and none of them is guessed away.

## What is refused, and with which reason code

`ImageAdapterError` after this task: the four decisions nobody has made
(`ColorSpaceUnknown`, `AlphaSourceUnknown`, `AlphaTestUnknown`,
`AddressModeUnknown`), plus `Rgb565Policy(Rgb565PolicyError)`, which
forwards the policy's own code unchanged. The policy's four refusals each
have a stable code, and a test asserts the four codes are distinct so a
consumer can group by them.

| Refusal | Code | What is missing |
| --- | --- | --- |
| `CoverageSourceUnknown` | `coverage_source_unknown` | the stored `AlphaSource` is `Unknown`; affects 22 retail rows. The adapter maps this one to its own `alpha_source_unknown`, which is F17-B's existing code for the same fact. |
| `KeyPlaneAbsent` | `coverage_key_plane_absent` | the image does not store the plane the key names (a bug, not content) |
| `TexelOutOfBounds` | `texel_out_of_bounds` | the coordinate is outside the image |
| `NotRgb565` | `not_rgb565` | the image stores no 16-bit word |
| `ExpansionPolicyError(Unknown \| Contradicted)` | — | a refusal is not an expansion, so it cannot be built as a policy |

One behaviour change worth naming: the adapter used to `expect` the alpha
channel and the coverage plane to be there, and would have panicked on an
image that did not carry the plane its `AlphaSource` named. It now goes
through `coverage_byte`, which reports `KeyPlaneAbsent` instead. The
descriptor normally prevents that pairing, so this is defence in depth
rather than a fixed crash, but the adapter no longer has a panic path in
that loop.

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/render/rgb565.rs` (new): the policy.
- `crates/cs_app/tests/accept_f17_b_rgb565_expansion.rs` (new): 10
  synthetic policy tests + 1 retail census.
- `crates/cs_app/src/render/bevy_image.rs`: the adapter now **applies**
  the policy. `ImageAdapterError` loses `Rgb565ExpansionUnknown`,
  `PaletteKeyCoverage` and `StoredValueKeyCoverage` and gains
  `Rgb565Policy(Rgb565PolicyError)`, which reports the policy's own reason
  code. `CoveragePlane` gains `KeyedWord { key }` and `KeyedIndex { key }`
  so the upload *records* which key it composed. Every texel — color and
  alpha — now comes from `render::rgb565`; the adapter widens and keys
  nothing itself. The fingerprint descriptor gains the expansion rule and
  is versioned `v1` → `v2` (a `v1` digest can no longer be read as a `v2`
  one, and no `v1` digest for a 565 image can exist because a 565 image
  used to be refused). `ImageUpload::expansion()` reports the rule.
- `crates/cs_app/tests/render/adapters.rs`: 3 new `accept_f17_b_rgb565_`
  tests at the adapter level, plus the F17-B refusal test trimmed to the
  three refusals that are still unestablished (it listed six).
- `crates/cs_app/src/render/mod.rs`: one `pub mod rgb565;` line (wiring).

**One observable failure:** if `expand_texel` did not read the coverage
from the `CoverageSource` it was handed — hardcoding `Opaque`, say, or
recomputing the word comparison itself — the keyed fixtures fail.
Observed, then reverted (rows M8 and M8b below).

## Where the adapter-level tests live, and why

The policy tests are a standalone binary
(`crates/cs_app/tests/accept_f17_b_rgb565_expansion.rs`); the adapter tests
are three cases inside F17-B's own `crates/cs_app/tests/render/adapters.rs`.
Both carry the `accept_f17_b_rgb565_` prefix, so one task selection finds
both. The split is deliberate: the adapter tests need F17-B's shared
fixture, and duplicating that fixture would have been a second copy of the
same production-fed inputs that could drift from the first.

That fixture turned out to be a better discriminator than expected. It
packs its colours into 5/6/5 by dropping each channel's low bits, and one
of them, `(16, 32, 48)`, has a blue level of 6 that is **not** exactly
representable: replication gives `(6 << 3) | (6 >> 2)` = 49 where
truncation gives 48. So the adapter test separates the decided rule from
the lossy one on a real upload, and the fixed-point scale happens to land
on 49 as well, so it separates replication from scale only through the
whole-domain sweep and the 1/255 bound. That is stated in the test rather
than glossed.

## Tests and what each acceptance criterion is pinned by

14 tests carry the prefix: 10 synthetic policy tests, 3 adapter tests, and
1 retail census. The synthetic and adapter ones need no `CS_GAME_DIR`; the
census is `#[ignore]`d and fails loudly (exit 101) without it.

| Criterion | Test |
| --- | --- |
| expansion over every 5/6/5 level, and out-of-domain masking | `..._every_channel_level_widens_to_the_decided_value` |
| expansion over every stored word (all 65,536) | `..._every_one_of_the_65_536_stored_words_expands` |
| the whole-image production path, texel by texel | `..._an_image_expands_texel_by_texel_through_the_policy` |
| the decision's class, and that a refusal is not a policy | `..._the_decision_is_designed_and_a_refusal_is_not_an_expansion` |
| the deviation bound and injectivity, computed not asserted | `..._the_deviation_between_rules_is_computed_and_bounded` |
| every `AlphaSource` projects to its plane | `..._every_alpha_source_projects_to_the_plane_its_key_lives_in` |
| `StoredValueKey` coverage | `..._a_stored_word_key_marks_exactly_the_texels_storing_the_key` |
| `PaletteKey` coverage, through a duplicate entry | `..._a_palette_index_key_survives_a_duplicate_palette_entry` |
| coverage is independent of the rule | `..._a_coverage_key_does_not_depend_on_the_expansion_rule` |
| the remaining policy refusals | `..._an_absent_key_plane_and_an_out_of_range_texel_stay_refusals` |
| **a 565 image uploads, widened, and the widening is in its identity** | `render::adapters::..._a_565_image_uploads_widened_by_the_decided_expansion` |
| **both keys reach the alpha channel and the texel keeps its colour** | `render::adapters::..._both_coverage_keys_reach_the_alpha_channel` |
| **the adapter still refuses a key plane the image lacks** | `render::adapters::..._a_key_plane_the_image_lacks_is_still_refused` |
| affected retail content quantified | `..._retail_565_rows_and_coverage_keys_are_counted` (ignored, `retail`) |

The expected channel values are **tables written into the test file**,
not the production formula recomputed: `REPLICATION_5` and
`REPLICATION_6` list all 32 and all 64 levels as literals, so a change to
`expand5`/`expand6`/`Rule` shows up as a wrong channel at a named level.
`Rule::max_channel_deviation` and `Rule::is_injective` are additionally
cross-checked against brute-force sweeps in the test, over every ordered
pair of rules and every pair of levels. The adapter's texel expectations
are written out per coordinate, and the isolation case for the fingerprint
is a hand-authored 8-bit image with the *same* texel bytes, coverage, alpha
test, color space and sampler, so the only variable is whether a widening
happened.

## Mutation verification (applied, run, reverted)

Every row was applied to the module it names, the relevant target was run,
and the change reverted; the tree was clean afterwards. The "not caught"
rows are real and are reported as such.

### Policy module

| # | Mutation | Caught by |
| --- | --- | --- |
| M1 | `expand5` → `level << 3` (truncation) | **5 tests** fail: `..._every_channel_level_widens_to_the_decided_value`, `..._every_one_of_the_65_536_stored_words_expands`, `..._a_stored_word_key_...`, `..._a_palette_index_key_...`, `..._the_deviation_between_rules_...` |
| M2 | `expand6` → `level * 255 / 63` (fixed-point scale) | **2 tests** fail: `..._every_channel_level_widens_to_the_decided_value` (6-bit level 16 is 64, not 65) and `..._every_one_of_the_65_536_stored_words_expands` |
| M3 | `StoredValueKey` coverage compares `expand(word)` against `expand(key)` | **NOT CAUGHT — semantic no-op.** Every rule is injective, so this is the same function. Recorded, not hidden; see Decision 2's honest limit. |
| M4 | `PaletteIndex` coverage ignores the key and returns opaque | `..._a_palette_index_key_survives_a_duplicate_palette_entry` |
| M5 | `PaletteIndex` coverage compares against index `0` instead of the key | `..._a_palette_index_key_survives_a_duplicate_palette_entry` — this is why that test keys entry **4**, not entry 0 |
| M6 | `CoverageSource::from_source(Unknown)` → `Ok(Opaque)` | `..._every_alpha_source_projects_to_the_plane_its_key_lives_in` (an unestablished source became opaque) |
| M7 | `Rule::max_channel_deviation` always returns 0 | `..._the_deviation_between_rules_is_computed_and_bounded` (against the brute-force sweep) |
| M8 | `expand_texel` recomputes the stored-word comparison instead of calling `coverage_byte` | **NOT CAUGHT — semantic no-op**, the same comparison by another route (M3's cause) |
| M8b | `expand_texel` hardcodes `CoverageSource::Opaque` instead of the source it was handed | **4 tests** fail: `..._a_stored_word_key_...`, `..._a_palette_index_key_...`, `..._a_coverage_key_does_not_depend...`, `..._an_image_expands_texel_by_texel...` |
| M9 | `stores_texel_words` returns `true` for every format | `..._an_image_expands_texel_by_texel_through_the_policy` — **only after the probe found it escaped**: the first version asserted the predicate nowhere, and the retail census cannot catch it either, because every ZBD row is 565-backed. The test now covers the whole closed `DecodedFormat` set. |
| M10 | `expand5` drops the `& 0x1F` level mask | `..._every_one_of_the_65_536_stored_words_expands` — **only after the probe found it escaped**: the first version never passed a level wider than the channel. The test now sweeps levels 32..=255 and 64..=255. |
| M11 | a missing key plane returns `Ok(0)` instead of `KeyPlaneAbsent` | `..._an_absent_key_plane_and_an_out_of_range_texel_stay_refusals` |

### The adapter wiring

| # | Mutation | Caught by |
| --- | --- | --- |
| N1 | the adapter refuses a 565 image again | **2 tests** fail: `..._a_565_image_uploads_widened_by_the_decided_expansion` and `..._both_coverage_keys_reach_the_alpha_channel` |
| N2 | the adapter widens with `Rule::Truncation` | **2 tests** fail, on the unrepresentable blue channel: 48 instead of 49 |
| N3 | `CoveragePlane::from(StoredWord)` maps a key onto `Separate` | `..._both_coverage_keys_reach_the_alpha_channel` (the upload would misreport which key it composed) |
| N4 | `ImageUpload::expansion()` always reports `None` | `..._a_565_image_uploads_widened_by_the_decided_expansion` |
| N5 | the expansion byte is dropped from the fingerprint descriptor | `..._a_565_image_uploads_widened_by_the_decided_expansion` — **only after the probe found it escaped**: the first version compared a 565 upload against an RGBA8 one, which differ in the coverage and the alpha test too, so it proved nothing about the expansion byte. The test now builds a hand-authored 8-bit image with the *same* texels, coverage, alpha test, color space and sampler, and the assertion is that those two uploads are not the same texture. |
| N6 | the descriptor version stays `v1` | **NOT CAUGHT**, and it is not pinned on purpose. A version tag's whole job is to differ from what it replaced, and no digest from the old layout is stored in the workspace to compare against — the derived cache under `private/` is git-ignored and machine-local. Pinning a literal digest would catch this and would also fail on any future, intended descriptor change, which would be a worse trade. |

**Three of these were real gaps in my own tests** (M9, M10, N5) and were
fixed by the probe rather than argued away. Two are genuine semantic no-ops
(M3, M8) and one is an unpinned tag (N6); the honest response to those is
the injectivity check, the note above and the stated reason, not a claim
that a test pins them.

## Recorded unknowns and limitations

- **The original's expansion is unmeasured.** Whether the original let
  the card expand the words or expanded them on the CPU, and with which
  rounding, is unknown. Bounded above at ≤1/255 between the two rules
  that can represent white; `ClaimStatus::Inferred` excludes truncation
  from the retail saturated-texel evidence. **Resolving task: F17-D**
  (original-run screenshot matrix, `gpu` + `retail`).
- **The color space of every package row is `Unknown`**, so no 565 row
  can be sampled correctly yet. Not this task's fact; it is the
  `ColorSpaceUnknown` refusal. **Resolving task: F17-D**, with the
  original screenshot matrix, which is also waiting on the owner's file
  lookup order (#341), archive-member collisions (#342) and
  texture-archive selection (#352).
- **The alpha test is `Unknown` for every package row.** Even the 18,488
  opaque rows need an addressing mode, and the 18,357 rows with a
  stored coverage plane additionally need a threshold. **Resolving
  task: F17-A facts (material records) and F17-D.**
- **Texture addressing is never declared**, so no row reaches a sampler.
  Untouched; F17-D.
- **`AlphaSource::Unknown` (22 retail rows) is still refused** and stays
  named: the ZBD reader describes `HAS_ALPHA` without `FULL_ALPHA` on a
  palette texture as `Unknown` because the pinned source skips the flag
  for palette textures. No plane is named, so there is nothing to read.
  **Resolving task: F17-D**, or an evidence-backed palette-key
  declaration.
- **The engine never uploads a native 565 texture.** wgpu's 565 formats
  carry no sRGB variant, and the F17 non-negotiable 3 double-correction
  rule means the stored color space decides the upload format; the
  expansion therefore has to happen on the CPU at least for an sRGB row.
  Choosing a CPU expansion for every row is the consistent choice, and
  it is what this policy does. A future stage may revisit a native
  upload for a `Linear` row; that is an F17-C enhancement, not a change
  to this decision.
- **No mip levels, no stretch, no filtering.** `PresentationUnknown::Stretch`
  is on every row and is untouched.

## What is *not* claimed

At most **checked**. No original-data behavior is reproduced, no
original renderer was run, nothing is `verified_original` or
`release_approved`, and the `Designed` status on the expansion is not an
endorsement of it. The census is a count over read-only original files
under the `retail` capability; `retail` is file access, not proof that
the original executable ever ran.
