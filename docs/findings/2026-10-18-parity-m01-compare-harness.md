# PARITY-M01-COMPARE-HARNESS (#1646): our M01 frames beside original Drydock frames

Tool: `cs_xtask parity` (`tools/cs_xtask/src/parity.rs`). Tests: `cargo test -p cs_xtask -- accept_parity_m01_compare_`.

## Command

```sh
# 1. ours: the owner-command smoke (docs/PLAYTEST.md), frames are frame_NNNNN_label.png, 1/60 s each
target/release/cs --cs-path <retail> --mission M01 --smoke-seconds 45 --capture-dir private/parity-m01/ours

# 2. originals: Drydock screenshots (e.g. camp1fly.json, one per 5 s of virtual time) copied into
#    private/parity-m01/original/, each name carrying its mission time: `..._t<seconds>.png`
#    (otherwise the first number in the name; --ref-unit ms|frame60 changes the unit)

# 3. compare
cargo run --locked -p cs_xtask -- parity \
  --ref-dir private/parity-m01/original --ours-dir private/parity-m01/ours \
  --out private/parity-m01/sheet --offset-seconds <S> --tolerance-seconds 1.0
```

`--out` must have a `private` path component; the tool refuses anything else, so
derived original frames cannot land in Git. Nothing is copied into the repo.

## What it writes

- `sheet.png`: one row per pair, original left, ours right (320x240 thumbnails, time order).
- `report.md` / `report.json`: per pair the two file names and times, the pairing error, the
  mean-colour difference of 8 horizontal bands (top to bottom, 0..255), the luminance-histogram
  distance (32 bins, 0 identical .. 1 disjoint) and both edge densities (share of pixels whose
  right+down luminance step exceeds 48). Original frames with no frame of ours within the tolerance
  are listed. Both carry the sentence **visual comparison needed**: no number here is a parity claim.

Both sides are resampled (triangle filter) to 1024x768, aspect kept, centred on black (4:3 letterbox).

## Time alignment

Pairing is by mission time: `ours = original + --offset-seconds`, nearest of our frames within
`--tolerance-seconds`. Our frame time is `frame / 60` (the smoke advances exactly 1/60 s per frame);
the original time is the one in the Drydock file name. The offset is **supplied by the operator and
recorded in the report**; it must be stated with every comparison. Recommended anchor: original
time 0 = the first frame after the launch cinematic ends (first control frame), ours = the frame
at which our mission hands control to the player. Our smoke does not yet expose that frame, so until
it does the offset is a judgement and the pairing a reading aid only.

## Run on current main

No original Drydock frames exist on the machine that built this (`ref-drydock/` and `drydock86`
were absent), and none are fabricated. The pipeline was run on main's own smoke capture
(13 frames, 13 checks passing) against a stand-in "original" directory made from three of those
very frames renamed by time: 3 pairs, all differences 0, sheet in `private/parity-m01/sheet/`.
That proves the tooling only. **Open:** a real comparison needs the owner's Drydock frames.
