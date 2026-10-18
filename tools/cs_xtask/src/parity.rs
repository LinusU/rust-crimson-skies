//! `cs_xtask parity`: side-by-side parity comparison for M01 (#1646 `PARITY-M01-COMPARE-HARNESS`).
//!
//! Takes a directory of original reference frames (Drydock screenshots, named
//! by mission time) and a directory of our `cs --mission M01 --capture-dir`
//! output, letterboxes both to the original's 1024x768, pairs them by time and
//! writes a contact sheet plus per-pair numeric differences.
//!
//! The numbers are a reading aid, never a verdict: the report always says
//! "visual comparison needed". Original frames are only ever read, and the
//! output directory must sit under a `private/` directory so nothing derived
//! from them can be committed by accident.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use image::imageops::{self, FilterType};
use image::{Rgb, RgbImage};

/// The original's screenshot size; everything is compared at this size.
pub const WIDTH: u32 = 1024;
/// See [`WIDTH`].
pub const HEIGHT: u32 = 768;
/// Horizontal bands whose mean colour is reported.
pub const BANDS: usize = 8;
/// Luminance histogram bins.
pub const HIST_BINS: usize = 32;
/// Summed absolute neighbour luminance difference above which a pixel is an edge.
pub const EDGE_THRESHOLD: i32 = 48;
/// Thumbnail size of one frame on the contact sheet.
pub const THUMB_W: u32 = 320;
/// See [`THUMB_W`].
pub const THUMB_H: u32 = 240;

/// The sentence every report carries: numbers never claim parity.
pub const VERDICT: &str = "visual comparison needed";

/// How a file name's number is converted to seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TimeUnit {
    Seconds,
    Millis,
    /// Frames of 1/60 s, the unit of our own `frame_NNNNN_label.png` names.
    Frames60,
}

impl TimeUnit {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "s" => Some(Self::Seconds),
            "ms" => Some(Self::Millis),
            "frame60" => Some(Self::Frames60),
            _ => None,
        }
    }

    fn to_seconds(self, n: f64) -> f64 {
        match self {
            Self::Seconds => n,
            Self::Millis => n / 1000.0,
            Self::Frames60 => n / 60.0,
        }
    }
}

/// One frame on disk with its mission time.
#[derive(Clone, Debug)]
pub struct Frame {
    pub path: PathBuf,
    pub seconds: f64,
}

/// The mission time a file name carries: the number directly after a `t`
/// (`camp1fly_t0005.png`, `t12.5.png`), otherwise the first number in the stem.
/// Our own `frame_00033_world.png` therefore reads as frame 33.
pub fn time_from_name(name: &str, unit: TimeUnit) -> Option<f64> {
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    let b = stem.as_bytes();
    let mut tokens: Vec<(usize, f64)> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            let start = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            if i + 1 < b.len() && b[i] == b'.' && b[i + 1].is_ascii_digit() {
                i += 1;
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
            }
            tokens.push((start, stem[start..i].parse().ok()?));
        } else {
            i += 1;
        }
    }
    let after_t = tokens
        .iter()
        .find(|(s, _)| *s > 0 && b[*s - 1].eq_ignore_ascii_case(&b't'));
    after_t.or(tokens.first()).map(|(_, n)| unit.to_seconds(*n))
}

/// Every PNG/JPEG in `dir` whose name carries a time, sorted by time.
pub fn list_frames(dir: &Path, unit: TimeUnit) -> Result<Vec<Frame>, String> {
    let rd = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut out = Vec::new();
    for entry in rd {
        let path = entry.map_err(|e| e.to_string())?.path();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase);
        if !matches!(ext.as_deref(), Some("png" | "jpg" | "jpeg")) {
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if let Some(seconds) = time_from_name(name, unit) {
            out.push(Frame { path, seconds });
        }
    }
    out.sort_by(|a, b| a.seconds.total_cmp(&b.seconds).then(a.path.cmp(&b.path)));
    Ok(out)
}

/// Scale `img` to fit 1024x768 keeping its aspect ratio, centred on black.
pub fn letterbox(img: &RgbImage) -> RgbImage {
    let mut canvas = RgbImage::from_pixel(WIDTH, HEIGHT, Rgb([0, 0, 0]));
    if img.width() == 0 || img.height() == 0 {
        return canvas;
    }
    let scale = (WIDTH as f64 / img.width() as f64).min(HEIGHT as f64 / img.height() as f64);
    let w = ((img.width() as f64 * scale).round() as u32).clamp(1, WIDTH);
    let h = ((img.height() as f64 * scale).round() as u32).clamp(1, HEIGHT);
    let scaled = if (w, h) == img.dimensions() {
        img.clone()
    } else {
        imageops::resize(img, w, h, FilterType::Triangle)
    };
    imageops::replace(
        &mut canvas,
        &scaled,
        i64::from((WIDTH - w) / 2),
        i64::from((HEIGHT - h) / 2),
    );
    canvas
}

/// Load and letterbox one frame.
pub fn load_letterboxed(path: &Path) -> Result<RgbImage, String> {
    let img = image::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(letterbox(&img.to_rgb8()))
}

/// What is measured on one 1024x768 frame.
#[derive(Clone, Debug)]
pub struct Metrics {
    /// Mean RGB of each of [`BANDS`] horizontal bands, top to bottom.
    pub band_means: [[f32; 3]; BANDS],
    /// Normalised luminance histogram (sums to 1).
    pub histogram: [f32; HIST_BINS],
    /// Fraction of pixels whose neighbour luminance step exceeds [`EDGE_THRESHOLD`].
    pub edge_density: f32,
}

fn luma(p: &Rgb<u8>) -> i32 {
    (299 * i32::from(p[0]) + 587 * i32::from(p[1]) + 114 * i32::from(p[2])) / 1000
}

pub fn measure(img: &RgbImage) -> Metrics {
    let (w, h) = img.dimensions();
    let band_h = (h as usize / BANDS).max(1);
    let mut sums = [[0u64; 3]; BANDS];
    let mut counts = [0u64; BANDS];
    let mut hist = [0u64; HIST_BINS];
    let mut edges = 0u64;
    for y in 0..h {
        let band = (y as usize / band_h).min(BANDS - 1);
        for x in 0..w {
            let p = img.get_pixel(x, y);
            for c in 0..3 {
                sums[band][c] += u64::from(p[c]);
            }
            counts[band] += 1;
            let l = luma(p);
            hist[(l as usize * HIST_BINS / 256).min(HIST_BINS - 1)] += 1;
            let step = if x + 1 < w {
                (l - luma(img.get_pixel(x + 1, y))).abs()
            } else {
                0
            } + if y + 1 < h {
                (l - luma(img.get_pixel(x, y + 1))).abs()
            } else {
                0
            };
            if step > EDGE_THRESHOLD {
                edges += 1;
            }
        }
    }
    let total = u64::from(w) * u64::from(h);
    let total = total.max(1) as f32;
    let mut band_means = [[0f32; 3]; BANDS];
    for b in 0..BANDS {
        for c in 0..3 {
            band_means[b][c] = sums[b][c] as f32 / counts[b].max(1) as f32;
        }
    }
    let mut histogram = [0f32; HIST_BINS];
    for (o, n) in histogram.iter_mut().zip(hist) {
        *o = n as f32 / total;
    }
    Metrics {
        band_means,
        histogram,
        edge_density: edges as f32 / total,
    }
}

/// Numeric differences between two frames' metrics.
#[derive(Clone, Debug)]
pub struct Diff {
    /// Mean absolute RGB difference per band (0..255).
    pub band_delta: [f32; BANDS],
    /// Half the L1 distance of the luminance histograms (0 identical, 1 disjoint).
    pub histogram_distance: f32,
    /// `ours - original` edge density.
    pub edge_density_delta: f32,
}

pub fn diff(original: &Metrics, ours: &Metrics) -> Diff {
    let mut band_delta = [0f32; BANDS];
    for (b, d) in band_delta.iter_mut().enumerate() {
        *d = (0..3)
            .map(|c| (original.band_means[b][c] - ours.band_means[b][c]).abs())
            .sum::<f32>()
            / 3.0;
    }
    let histogram_distance = 0.5
        * original
            .histogram
            .iter()
            .zip(&ours.histogram)
            .map(|(a, b)| (a - b).abs())
            .sum::<f32>();
    Diff {
        band_delta,
        histogram_distance,
        edge_density_delta: ours.edge_density - original.edge_density,
    }
}

/// One compared pair.
#[derive(Clone, Debug)]
pub struct Pair {
    pub original: Frame,
    pub ours: Frame,
    pub original_metrics: Metrics,
    pub ours_metrics: Metrics,
    pub diff: Diff,
    /// `ours.seconds - (original.seconds + offset)`; how far the pairing is off.
    pub time_error: f64,
    thumbs: (RgbImage, RgbImage),
}

/// Settings of one comparison.
#[derive(Clone, Debug)]
pub struct Options {
    /// Our mission time that corresponds to the original's time 0 (the alignment).
    pub offset_seconds: f64,
    /// Largest accepted `|time_error|`.
    pub tolerance_seconds: f64,
}

/// The result of [`compare`].
pub struct Comparison {
    pub options: Options,
    pub pairs: Vec<Pair>,
    /// Original frames with no frame of ours inside the tolerance.
    pub unpaired: Vec<Frame>,
}

/// Pair every original frame with our nearest frame (after the offset) and measure.
pub fn compare(
    originals: &[Frame],
    ours: &[Frame],
    options: Options,
) -> Result<Comparison, String> {
    let mut pairs = Vec::new();
    let mut unpaired = Vec::new();
    for o in originals {
        let want = o.seconds + options.offset_seconds;
        let nearest = ours.iter().min_by(|a, b| {
            (a.seconds - want)
                .abs()
                .total_cmp(&(b.seconds - want).abs())
        });
        let Some(n) = nearest.filter(|n| (n.seconds - want).abs() <= options.tolerance_seconds)
        else {
            unpaired.push(o.clone());
            continue;
        };
        let oi = load_letterboxed(&o.path)?;
        let ni = load_letterboxed(&n.path)?;
        let (om, nm) = (measure(&oi), measure(&ni));
        pairs.push(Pair {
            diff: diff(&om, &nm),
            original_metrics: om,
            ours_metrics: nm,
            time_error: n.seconds - want,
            original: o.clone(),
            ours: n.clone(),
            thumbs: (
                imageops::resize(&oi, THUMB_W, THUMB_H, FilterType::Triangle),
                imageops::resize(&ni, THUMB_W, THUMB_H, FilterType::Triangle),
            ),
        });
    }
    Ok(Comparison {
        options,
        pairs,
        unpaired,
    })
}

/// Contact sheet: one row per pair, original on the left, ours on the right,
/// separated by a 4 px grey gutter (rows are in time order; the report names them).
pub fn contact_sheet(c: &Comparison) -> RgbImage {
    const GUTTER: u32 = 4;
    let rows = c.pairs.len().max(1) as u32;
    let mut sheet = RgbImage::from_pixel(
        2 * THUMB_W + 3 * GUTTER,
        rows * (THUMB_H + GUTTER) + GUTTER,
        Rgb([96, 96, 96]),
    );
    for (i, p) in c.pairs.iter().enumerate() {
        let y = i64::from(GUTTER) + (i as i64) * i64::from(THUMB_H + GUTTER);
        imageops::replace(&mut sheet, &p.thumbs.0, i64::from(GUTTER), y);
        imageops::replace(&mut sheet, &p.thumbs.1, i64::from(2 * GUTTER + THUMB_W), y);
    }
    sheet
}

fn name(f: &Frame) -> String {
    f.path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

fn json_str(s: &str) -> String {
    let mut o = String::from("\"");
    for ch in s.chars() {
        match ch {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            c if (c as u32) < 0x20 => {
                let _ = write!(o, "\\u{:04x}", c as u32);
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// Machine-readable report.
pub fn report_json(c: &Comparison) -> String {
    let list = |v: &[f32]| {
        v.iter()
            .map(|x| format!("{x:.3}"))
            .collect::<Vec<_>>()
            .join(",")
    };
    let mut s = String::new();
    let _ = write!(
        s,
        "{{\"verdict\":{},\"offset_seconds\":{},\"tolerance_seconds\":{},\"pairs\":[",
        json_str(VERDICT),
        c.options.offset_seconds,
        c.options.tolerance_seconds
    );
    for (i, p) in c.pairs.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let _ = write!(
            s,
            "{{\"row\":{i},\"original\":{},\"original_seconds\":{},\"ours\":{},\"ours_seconds\":{},\
             \"time_error_seconds\":{:.4},\"band_delta\":[{}],\"histogram_distance\":{:.4},\
             \"edge_density_original\":{:.4},\"edge_density_ours\":{:.4},\"edge_density_delta\":{:.4}}}",
            json_str(&name(&p.original)),
            p.original.seconds,
            json_str(&name(&p.ours)),
            p.ours.seconds,
            p.time_error,
            list(&p.diff.band_delta),
            p.diff.histogram_distance,
            p.original_metrics.edge_density,
            p.ours_metrics.edge_density,
            p.diff.edge_density_delta,
        );
    }
    s.push_str("],\"unpaired_originals\":[");
    s.push_str(
        &c.unpaired
            .iter()
            .map(|f| json_str(&name(f)))
            .collect::<Vec<_>>()
            .join(","),
    );
    s.push_str("]}\n");
    s
}

/// Human-readable report listing what was compared.
pub fn report_markdown(c: &Comparison) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "# M01 parity comparison: {VERDICT}\n");
    let _ = writeln!(
        s,
        "These numbers are a reading aid. They never establish parity; look at `sheet.png` \
         (left original, right ours, one row per pair, in the order below).\n"
    );
    let _ = writeln!(
        s,
        "Alignment: our mission time = original time + {} s; pairing tolerance {} s.\n",
        c.options.offset_seconds, c.options.tolerance_seconds
    );
    let _ = writeln!(
        s,
        "| row | original (t s) | ours (t s) | Δt | hist dist | edge orig | edge ours | band Δ (top→bottom) |\n|---|---|---|---|---|---|---|---|"
    );
    for (i, p) in c.pairs.iter().enumerate() {
        let bands = p
            .diff
            .band_delta
            .iter()
            .map(|d| format!("{d:.0}"))
            .collect::<Vec<_>>()
            .join(" ");
        let _ = writeln!(
            s,
            "| {i} | {} ({:.2}) | {} ({:.2}) | {:+.2} | {:.3} | {:.3} | {:.3} | {bands} |",
            name(&p.original),
            p.original.seconds,
            name(&p.ours),
            p.ours.seconds,
            p.time_error,
            p.diff.histogram_distance,
            p.original_metrics.edge_density,
            p.ours_metrics.edge_density,
        );
    }
    if !c.unpaired.is_empty() {
        let _ = writeln!(s, "\nOriginal frames with no frame of ours in tolerance:");
        for f in &c.unpaired {
            let _ = writeln!(s, "- {} ({:.2} s)", name(f), f.seconds);
        }
    }
    s
}

/// The output directory must have a `private` path component: originals' derivatives stay out of Git.
pub fn check_private_out(out: &Path) -> Result<(), String> {
    if out.components().any(|c| c.as_os_str() == "private") {
        Ok(())
    } else {
        Err(format!(
            "--out {} must be under a `private/` directory (derived original frames are never committed)",
            out.display()
        ))
    }
}

/// Write `sheet.png`, `report.json` and `report.md` into `out`.
pub fn write_outputs(c: &Comparison, out: &Path) -> Result<(), String> {
    check_private_out(out)?;
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    contact_sheet(c)
        .save(out.join("sheet.png"))
        .map_err(|e| e.to_string())?;
    std::fs::write(out.join("report.json"), report_json(c)).map_err(|e| e.to_string())?;
    std::fs::write(out.join("report.md"), report_markdown(c)).map_err(|e| e.to_string())
}

const USAGE: &str = "usage: cs_xtask parity --ref-dir <dir> --ours-dir <dir> --out <private/...> \
[--offset-seconds S] [--tolerance-seconds T] [--ref-unit s|ms|frame60]";

/// The `cs_xtask parity` command: returns the one-line summary to print.
pub fn run(args: &[String]) -> Result<String, String> {
    let mut args = args.iter();
    let (mut ref_dir, mut ours_dir, mut out) = (None, None, None);
    let (mut offset, mut tol, mut unit) = (0.0, 1.0, TimeUnit::Seconds);
    while let Some(a) = args.next() {
        let mut val = || {
            args.next()
                .cloned()
                .ok_or_else(|| format!("{a} needs a value\n{USAGE}"))
        };
        match a.as_str() {
            "--ref-dir" => ref_dir = Some(PathBuf::from(val()?)),
            "--ours-dir" => ours_dir = Some(PathBuf::from(val()?)),
            "--out" => out = Some(PathBuf::from(val()?)),
            "--offset-seconds" => offset = val()?.parse().map_err(|e| format!("{e}"))?,
            "--tolerance-seconds" => tol = val()?.parse().map_err(|e| format!("{e}"))?,
            "--ref-unit" => unit = TimeUnit::parse(&val()?).ok_or("unit is s, ms or frame60")?,
            _ => return Err(format!("unknown argument {a}\n{USAGE}")),
        }
    }
    let (Some(ref_dir), Some(ours_dir), Some(out)) = (ref_dir, ours_dir, out) else {
        return Err(USAGE.into());
    };
    check_private_out(&out)?;
    let originals = list_frames(&ref_dir, unit)?;
    let ours = list_frames(&ours_dir, TimeUnit::Frames60)?;
    if originals.is_empty() || ours.is_empty() {
        return Err(format!(
            "need frames on both sides (found {} original, {} ours); names must carry a time",
            originals.len(),
            ours.len()
        ));
    }
    let c = compare(
        &originals,
        &ours,
        Options {
            offset_seconds: offset,
            tolerance_seconds: tol,
        },
    )?;
    write_outputs(&c, &out)?;
    Ok(format!(
        "compared {} pair(s), {} original frame(s) unpaired; wrote {}/{{sheet.png,report.json,report.md}} -- {VERDICT}",
        c.pairs.len(),
        c.unpaired.len(),
        out.display()
    ))
}
