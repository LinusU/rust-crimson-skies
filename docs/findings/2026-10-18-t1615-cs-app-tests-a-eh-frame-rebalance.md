# #1615: `cs_app_tests_a` rebalanced — the F40 cinematics set carries ~4.8 MiB of `__eh_frame`

Date: 2026-10-18. Task: **#1615** "cs_app_tests_a's `__eh_frame` exceeds the 16
MiB compact-unwind limit on macOS" (key `T336-EH-FRAME-OVER-LIMIT`), found
during review of #1596 and reproduced identically on clean origin/main. Host:
macOS aarch64, default `test` profile, `size -m`.

## 1. The failure

`accept_t336_linked_images_keep_eh_frame_under_the_encoding_limit` (macOS-only,
so CI's ubuntu-latest never runs it) failed on origin/main: `cs_app_tests_a`
linked 20,291,368 B of `__eh_frame` against the 16,777,216 B limit; ld also
printed `__eh_frame section too large`. The other four binaries were still
under — the growth was not workspace-wide, it was local to the code only
suite_a reached.

## 2. Which modules carry the size (ablation, not line counts)

Per `docs/findings/2026-10-17-t1543-cs-app-test-binaries.md` a test binary's
`__eh_frame` is almost entirely dependency code its tests *reach*, so the only
reliable measure is building the union. A `tmp_probe_a` copy of `suite_a.rs`
(identical 20,291,368 B) with module groups removed measured:

| removed from a | `__eh_frame` | Δ |
| --- | --- | --- |
| F40 cinematics + audio/music/radio set (23 mods) | 15,322,240 | −4,969,128 |
| **F40 cinematics only (5 mods)** | **15,452,156** | **−4,839,212** |
| audio/music/radio only (18 mods) | 20,161,580 | −129,788 |
| whole `mod campaign {}` block | 19,676,256 | −615,112 |
| all top-level `evidence_report_*` | 19,659,188 | −632,180 |

The F40 cinematics members (`accept_f40_d1/d3/d4`, `evidence_report_f40_d1/d4`)
are the sole reachers of ~4.8 MiB — slopmpeg (164 symbols, absent from every
other binary) plus the monomorphized device-playback/capture code. The
audio/music/radio members carry almost nothing of their own: rodio/cpal are in
every suite already (the composed app owns an `AudioDevice`).

Per-crate attribution (dwarfdump FDE → `nm -n` symbol → v0 crate tag) shows the
bulk everywhere is `core`/`alloc`/`bevy_ecs`/`bevy_reflect` monomorphizations —
which types a suite instantiates decides its size, not its file count.

## 3. Where the members went, and why there

Standalone probes of candidate unions:

| probe | `__eh_frame` |
| --- | --- |
| `cs_app_tests_b` + the 5 cinematics mods | **15,055,296** (+151,872 over b) |
| `cs_app_tests_d` + the 5 cinematics mods | 18,490,164 (over) |
| the 23-mod media set alone | 19,659,188 (over — no new binary helps) |

`cs_app_tests_b` already reaches the device-playback/render path through
`playtest_*`, `render/` and the F22/F24 device and flight-wiring suites, so the
cinematics set adds only ~0.15 MiB there — against ~4.8 MiB anywhere else. The
media set cannot be its own sixth binary either: it reaches the whole app plus
cinematics and links over the limit alone.

The m02–m05 campaign members (`m02_a`, `m02_b`, `m02_b_fu2`, `m02_t3`, `m03_a`,
`m03_b`, `m04_a`, `m04_b`, `m05_a`, `m05_b`) moved to `cs_app_tests_d`'s
`campaign/mod.rs`, where their m06–m24 siblings and the shared control-program
machinery already live (the mod.rs doc map already described `m02_b`, `m02_t3`,
`m04_b`, `m05_b`). They cost d +181,812 B and buy a another −275,536 B of
headroom. `m02_b_fu2`'s `crate::campaign::m02_b` seam moves inside the same
binary; nothing outside the set references it.

## 4. Resulting sizes (all under the 16 MiB limit)

| binary | before | after |
| --- | --- | --- |
| `cs_app_tests_a` | 20,291,368 | **15,176,620** |
| `cs_app_tests_b` | 14,903,424 | **15,055,296** |
| `cs_app_tests_c` | 15,441,176 | 15,441,176 |
| `cs_app_tests_d` | 13,657,856 | **13,839,668** |
| `cs_app_tests_e` | 15,475,944 | 15,475,944 |

Re-measured on the review rebase (`4c9fbadb5`, onto `8226c7aa4`): main's
`3fb201596` had landed two more F40-D members (`accept_f40_d_story_cinematics`,
`evidence_report_f40_d`) inside `cs_app_tests_a`; they joined their siblings in
`cs_app_tests_b`, where they reach code the binary already links (+12,924 B).
Re-measured `size -m`: a 15,176,856, b 15,068,220, c 15,441,180, d 13,845,200,
e 15,475,948, `cs` 15,348,152 — all under the 16,777,216 B limit. Note `c`/`e`
keep only ~1.3 MiB of headroom, and `accept_t336` measures only `cs` plus the
binary hosting it (`a`): a suite b–e overflow would not trip the gate today.

`cs_app_tests_d` stays the lightest, so it remains the overflow target from the
#1543 findings. `cargo test --locked -p cs_app` on the rebased commit: 2,329
non-ignored tests pass, 0 failures (a 894, b 274, c 526, d 438, e 11, lib 185,
doc 1); the T336 gate passes in binary a; `accept_test_bin_cs_app_*` and
`cs_xtask test-layout` stay green with the same five targets.
