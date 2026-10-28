//! Offline OCR using the engine built into Windows 10/11 (Windows.Media.Ocr).

/// OCR result: the text in reading order, plus each engine line's word boxes
/// (in the input image's pixels) for Smart Redact.
#[derive(serde::Serialize)]
pub struct Ocr {
    pub text: String,
    pub lines: Vec<Vec<Word>>,
}

impl Ocr {
    fn new(lines: Vec<Vec<Word>>) -> Self {
        Ocr { text: layout(lines.clone()), lines }
    }
}

/// Recognize text in an RGBA image.
#[cfg(windows)]
pub fn recognize(rgba: &[u8], width: u32, height: u32) -> Result<Ocr, String> {
    init();
    let t = std::time::Instant::now();
    let max_dim = windows::Media::Ocr::OcrEngine::MaxImageDimension().map_err(|e| e.message().to_string())?;

    // Small text reads much better at 2x (symbols, punctuation; measured on code and
    // API-key samples), big text slightly worse, and we only learn the size from a pass.
    // So the 2x pass starts right away in parallel and is used only if text is small.
    let (tx, rx) = std::sync::mpsc::channel();
    if can_upscale(width, height, max_dim) {
        let src = rgba.to_vec();
        std::thread::spawn(move || {
            init();
            let _ = tx.send(read_words(&double(&src, width), width * 2, height * 2));
        });
    }
    let lines = read_words(rgba, width, height)?;
    if median_height(&lines) < SMALL_TEXT_PX {
        if let Ok(big) = rx.recv() {
            println!("[ocr] {width}x{height} (2x): {} ms", t.elapsed().as_millis());
            let mut big = big?;
            for w in big.iter_mut().flatten() {
                (w.x, w.y, w.w, w.h) = (w.x / 2.0, w.y / 2.0, w.w / 2.0, w.h / 2.0);
            }
            return Ok(Ocr::new(big));
        }
    }
    println!("[ocr] {width}x{height}: {} ms", t.elapsed().as_millis());
    Ok(Ocr::new(lines))
}

/// WinRT needs each calling thread initialized; harmless if it already is.
#[cfg(windows)]
pub(crate) fn init() {
    use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
    unsafe {
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
    }
}

/// One OCR pass: word boxes grouped by the engine's lines.
#[cfg(windows)]
fn read_words(rgba: &[u8], w: u32, h: u32) -> Result<Vec<Vec<Word>>, String> {
    use windows::{
        Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap},
        Media::Ocr::OcrEngine,
        Storage::Streams::DataWriter,
    };
    let err = |e: windows::core::Error| e.message().to_string();

    let engine = OcrEngine::TryCreateFromUserProfileLanguages()
        .map_err(|_| "No OCR language installed. Add one in Windows Settings > Language.".to_string())?;
    let mut bgra = rgba.to_vec();
    for px in bgra.chunks_exact_mut(4) {
        px.swap(0, 2); // RGBA -> BGRA
    }
    let writer = DataWriter::new().map_err(err)?;
    writer.WriteBytes(&bgra).map_err(err)?;
    let buffer = writer.DetachBuffer().map_err(err)?;
    let bitmap = SoftwareBitmap::CreateCopyWithAlphaFromBuffer(
        &buffer,
        BitmapPixelFormat::Bgra8,
        w as i32,
        h as i32,
        BitmapAlphaMode::Premultiplied,
    )
    .map_err(err)?;
    let result = engine.RecognizeAsync(&bitmap).map_err(err)?.join().map_err(err)?;

    let mut lines = Vec::new();
    for line in result.Lines().map_err(err)? {
        let mut words = Vec::new();
        for word in line.Words().map_err(err)? {
            let r = word.BoundingRect().map_err(err)?;
            words.push(Word {
                x: r.X,
                y: r.Y,
                w: r.Width,
                h: r.Height,
                text: word.Text().map_err(err)?.to_string(),
            });
        }
        if !words.is_empty() {
            lines.push(words);
        }
    }
    Ok(lines)
}

/// 2x upscale by pixel doubling. Reads as well as a smooth resize on the samples and is
/// ~30x faster than `image::imageops::resize`.
fn double(rgba: &[u8], width: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.len() * 4);
    for row in rgba.chunks_exact(width as usize * 4) {
        let start = out.len();
        for px in row.chunks_exact(4) {
            out.extend_from_slice(px);
            out.extend_from_slice(px);
        }
        out.extend_from_within(start..);
    }
    out
}

/// Word-box height (px) below which text counts as "small". Picked from the samples:
/// code at 14px gained a lot from 2x, normal UI text at 17px got slightly worse.
const SMALL_TEXT_PX: f32 = 16.0;

fn median_height(lines: &[Vec<Word>]) -> f32 {
    let mut hs: Vec<f32> = lines.iter().flatten().map(|w| w.h).collect();
    if hs.is_empty() {
        return 0.0; // nothing found: tiny text is a likely reason, so try upscaled
    }
    hs.sort_by(f32::total_cmp);
    hs[hs.len() / 2]
}

#[cfg(not(windows))]
pub fn recognize(_rgba: &[u8], _width: u32, _height: u32) -> Result<Ocr, String> {
    Err("OCR is only implemented on Windows so far.".into())
}

/// Stay under the engine's size limit, and cap pixels so big selections stay fast.
fn can_upscale(width: u32, height: u32, max_dim: u32) -> bool {
    width * 2 <= max_dim && height * 2 <= max_dim && (width as u64 * height as u64) <= 2_500_000
}

#[derive(Clone, serde::Serialize)]
pub struct Word {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    text: String,
}

/// Rebuild reading order from word boxes. Windows sometimes splits one visual line
/// into several out-of-order lines (common with code), so we merge lines that sit on
/// the same row and are close horizontally, then sort words left to right.
/// Far-apart blocks on the same row (columns) stay separate.
fn layout(lines: Vec<Vec<Word>>) -> String {
    struct Row {
        top: f32,
        bottom: f32,
        left: f32,
        right: f32,
        words: Vec<Word>,
    }
    let bbox = |ws: &[Word]| {
        let top = ws.iter().map(|w| w.y).fold(f32::MAX, f32::min);
        let bottom = ws.iter().map(|w| w.y + w.h).fold(f32::MIN, f32::max);
        let left = ws.iter().map(|w| w.x).fold(f32::MAX, f32::min);
        let right = ws.iter().map(|w| w.x + w.w).fold(f32::MIN, f32::max);
        (top, bottom, left, right)
    };

    let mut rows: Vec<Row> = Vec::new();
    for words in lines {
        let (top, bottom, left, right) = bbox(&words);
        let height = bottom - top;
        let found = rows.iter_mut().find(|r| {
            let overlap = r.bottom.min(bottom) - r.top.max(top);
            let min_h = (r.bottom - r.top).min(height);
            let gap = (left - r.right).max(r.left - right);
            overlap >= 0.5 * min_h && gap < 3.0 * min_h
        });
        match found {
            Some(r) => {
                r.top = r.top.min(top);
                r.bottom = r.bottom.max(bottom);
                r.left = r.left.min(left);
                r.right = r.right.max(right);
                r.words.extend(words);
            }
            None => rows.push(Row { top, bottom, left, right, words }),
        }
    }

    rows.sort_by(|a, b| a.top.total_cmp(&b.top).then(a.left.total_cmp(&b.left)));
    rows.into_iter()
        .map(|mut r| {
            r.words.sort_by(|a, b| a.x.total_cmp(&b.x));
            r.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    /// Runs OCR on the spike images. Set OCR_SAMPLES to a folder of <name>.png files.
    #[test]
    fn ocr_samples() {
        let Ok(dir) = std::env::var("OCR_SAMPLES") else { return };
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "png") {
                let img = image::open(&path).unwrap().to_rgba8();
                let t = std::time::Instant::now();
                let text = recognize(img.as_raw(), img.width(), img.height()).unwrap().text;
                println!(
                    "{:<10} {:>4}ms  {}",
                    path.file_stem().unwrap().to_string_lossy(),
                    t.elapsed().as_millis(),
                    text.replace('\n', " | ")
                );
            }
        }
    }

    /// A realistic selection: all samples tiled into one ~1200x800 image.
    #[test]
    fn ocr_big_selection() {
        let Ok(dir) = std::env::var("OCR_SAMPLES") else { return };
        let mut big = image::RgbaImage::from_pixel(1200, 800, image::Rgba([255, 255, 255, 255]));
        let mut y = 0;
        // Small-text samples only, repeated: the worst case (forces the 2x pass).
        let small: Vec<_> = ["code", "dark", "small"]
            .iter()
            .map(|n| image::open(format!("{dir}/{n}.png")).unwrap().to_rgba8())
            .collect();
        'fill: loop {
            for img in &small {
                if y + img.height() > 800 { break 'fill; }
                image::imageops::overlay(&mut big, img, 0, y as i64);
                image::imageops::overlay(&mut big, img, 650, y as i64);
                y += img.height();
            }
        }
        let t = std::time::Instant::now();
        let text = recognize(big.as_raw(), 1200, 800).unwrap().text;
        println!("big {} ms, {} lines", t.elapsed().as_millis(), text.lines().count());
    }

    /// Boxes from the 2x pass are halved back; they must land where the native pass put
    /// the same word. Also writes <name>.json next to each sample for the JS detector.
    #[test]
    fn ocr_boxes_match_native_pass() {
        let Ok(dir) = std::env::var("OCR_SAMPLES") else { return };
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if !path.extension().is_some_and(|e| e == "png") { continue; }
            let img = image::open(&path).unwrap().to_rgba8();
            let (w, h) = img.dimensions();
            let native: Vec<Word> = read_words(img.as_raw(), w, h).unwrap().into_iter().flatten().collect();
            let ocr = recognize(img.as_raw(), w, h).unwrap();
            let mut matched = 0;
            for word in ocr.lines.iter().flatten() {
                if let Some(n) = native.iter().find(|n| n.text == word.text) {
                    assert!((n.x - word.x).abs() < 4.0 && (n.y - word.y).abs() < 4.0, "{:?}: '{}' moved", path, word.text);
                    matched += 1;
                }
            }
            assert!(matched > 0 || native.is_empty(), "{path:?}: no words in common");
            std::fs::write(path.with_extension("json"), serde_json::to_string(&ocr).unwrap()).unwrap();
        }
    }

    #[test]
    fn double_repeats_each_pixel_2x2() {
        // 2x1 image: red, blue
        let src = [255, 0, 0, 255, 0, 0, 255, 255];
        let r = [255, 0, 0, 255];
        let b = [0, 0, 255, 255];
        let row = [r, r, b, b].concat();
        assert_eq!(double(&src, 2), [row.clone(), row].concat());
    }

    #[test]
    fn layout_merges_split_lines_and_sorts_words() {
        let w = |x: f32, text: &str| Word { x, y: 10.0, w: 30.0, h: 12.0, text: text.into() };
        // Windows returned the right half of a row before the left half.
        let lines = vec![vec![w(80.0, "b"), w(120.0, "c")], vec![w(0.0, "const"), w(40.0, "a")]];
        assert_eq!(layout(lines), "const a b c");
    }

    #[test]
    fn layout_keeps_far_columns_and_rows_apart() {
        let word = |x: f32, y: f32, text: &str| Word { x, y, w: 30.0, h: 12.0, text: text.into() };
        let lines = vec![
            vec![word(0.0, 40.0, "second")],
            vec![word(0.0, 10.0, "left")],
            vec![word(600.0, 10.0, "right")],
        ];
        assert_eq!(layout(lines), "left\nright\nsecond");
    }
}
