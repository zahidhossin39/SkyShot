//! Offline OCR using the engine built into Windows 10/11 (Windows.Media.Ocr).

/// Recognize text in an RGBA image. Returns lines joined with '\n'.
#[cfg(windows)]
pub fn recognize(rgba: &[u8], width: u32, height: u32) -> Result<String, String> {
    use windows::{
        Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap},
        Media::Ocr::OcrEngine,
        Storage::Streams::DataWriter,
        Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED},
    };
    let err = |e: windows::core::Error| e.message().to_string();

    // WinRT needs the calling thread initialized; harmless if it already is.
    unsafe {
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
    }

    let engine = OcrEngine::TryCreateFromUserProfileLanguages()
        .map_err(|_| "No OCR language installed. Add one in Windows Settings > Language.".to_string())?;

    let run = |rgba: &[u8], w: u32, h: u32| -> Result<Vec<Vec<Word>>, String> {
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
    };

    // Pass 1 at native size. If the text turned out small, a 2x upscale reads symbols
    // and punctuation much better (measured on code and API-key samples), while big
    // text reads slightly worse upscaled, so only pass 2 when it helps.
    let lines = run(rgba, width, height)?;
    let max_dim = OcrEngine::MaxImageDimension().map_err(err)?;
    if median_height(&lines) < SMALL_TEXT_PX && can_upscale(width, height, max_dim) {
        let img = image::RgbaImage::from_raw(width, height, rgba.to_vec()).ok_or("bad image buffer")?;
        let big = image::imageops::resize(&img, width * 2, height * 2, image::imageops::FilterType::CatmullRom);
        return Ok(layout(run(big.as_raw(), width * 2, height * 2)?));
    }
    Ok(layout(lines))
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
pub fn recognize(_rgba: &[u8], _width: u32, _height: u32) -> Result<String, String> {
    Err("OCR is only implemented on Windows so far.".into())
}

/// Stay under the engine's size limit, and cap pixels so big selections stay fast.
fn can_upscale(width: u32, height: u32, max_dim: u32) -> bool {
    width * 2 <= max_dim && height * 2 <= max_dim && (width as u64 * height as u64) <= 2_500_000
}

#[derive(Clone)]
struct Word {
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
                let text = recognize(img.as_raw(), img.width(), img.height()).unwrap();
                println!(
                    "{:<10} {:>4}ms  {}",
                    path.file_stem().unwrap().to_string_lossy(),
                    t.elapsed().as_millis(),
                    text.replace('\n', " | ")
                );
            }
        }
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
