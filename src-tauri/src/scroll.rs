//! Scrolling capture: scroll the area under the selection with the mouse wheel, grab it after
//! each step, and stitch the frames into one tall image by matching pixel rows.
//!
//! The overlap search (`find_shift`) is pure and unit-tested; the loop does the I/O.

use image::RgbaImage;

/// Safety limits so a page that never ends can't run forever.
const MAX_STEPS: usize = 150; // ~1 min worst case at ~0.4 s per step
const MAX_HEIGHT: u32 = 20_000;

fn row_hash(row: &[u8]) -> u64 {
    // FNV-1a: fast, and exact matches are all we need (screenshots are pixel-identical when static).
    row.iter().fold(0xcbf29ce484222325, |h, &b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

/// A row that's one colour carries no position info (blank space matches any shift).
fn is_blank(row: &[u8]) -> bool {
    row.chunks_exact(4).all(|px| px == &row[..4])
}

/// (hash, blank) per row.
fn rows(img: &RgbaImage) -> Vec<(u64, bool)> {
    img.as_raw().chunks_exact(img.width() as usize * 4).map(|r| (row_hash(r), is_blank(r))).collect()
}

/// How many rows `next` is scrolled down from `prev` (0 = didn't move), or None if no shift fits.
/// Ignores the top and bottom 10% of the frame: sticky headers/footers stay put while content moves.
fn find_shift(prev: &[(u64, bool)], next: &[(u64, bool)]) -> Option<usize> {
    let h = prev.len().min(next.len());
    let m = h / 10;
    let mut best: Option<(usize, f32)> = None;
    for s in 0..h.saturating_sub(2 * m + 16) {
        // next row i shows what prev row i + s showed
        let (mut seen, mut same) = (0, 0);
        for i in m..h - s - m {
            if next[i].1 && prev[i + s].1 {
                continue; // both blank: tells us nothing
            }
            seen += 1;
            same += (next[i].0 == prev[i + s].0) as usize;
        }
        if seen < 8 {
            continue;
        }
        let score = same as f32 / seen as f32;
        if score >= 0.95 && best.is_none_or(|(_, b)| score > b) {
            best = Some((s, score));
        }
    }
    best.map(|(s, _)| s)
}

/// Bottom `s` rows of `frame`, appended to `out`.
fn append_new_rows(out: &mut Vec<u8>, frame: &RgbaImage, s: usize) {
    let stride = frame.width() as usize * 4;
    let raw = frame.as_raw();
    out.extend_from_slice(&raw[raw.len() - s * stride..]);
}

/// Runs the whole capture. `grab` captures the area; `scroll` sends one scroll step.
pub fn run(mut grab: impl FnMut() -> Result<RgbaImage, String>, mut scroll: impl FnMut()) -> Result<RgbaImage, String> {
    let first = grab()?;
    let width = first.width();
    let mut out = first.as_raw().clone();
    let mut height = first.height();
    let mut prev = rows(&first);
    let mut still = 0;
    for _ in 0..MAX_STEPS {
        scroll();
        let frame = grab()?;
        let next = rows(&frame);
        match find_shift(&prev, &next) {
            Some(0) => {
                still += 1;
                if still >= 2 {
                    break; // reached the end (or the area doesn't scroll)
                }
            }
            Some(s) => {
                still = 0;
                append_new_rows(&mut out, &frame, s);
                height += s as u32;
            }
            None => break, // scrolled too far to line up (or the content changed): stop cleanly
        }
        prev = next;
        if height >= MAX_HEIGHT {
            break;
        }
    }
    RgbaImage::from_raw(width, height, out).ok_or_else(|| "stitch size mismatch".into())
}

#[cfg(windows)]
pub fn wheel_at(x: i32, y: i32, notches: i32) {
    use windows::Win32::UI::{
        Input::KeyboardAndMouse::{SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_WHEEL, MOUSEINPUT},
        WindowsAndMessaging::{SetCursorPos, WHEEL_DELTA},
    };
    unsafe {
        let _ = SetCursorPos(x, y);
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT { mouseData: (-(WHEEL_DELTA as i32)) as u32, dwFlags: MOUSEEVENTF_WHEEL, ..Default::default() },
            },
        };
        for _ in 0..notches {
            SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
        }
    }
}

/// Put the cursor back where the user left it.
#[cfg(windows)]
pub fn move_cursor(x: i32, y: i32) {
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::SetCursorPos(x, y);
    }
}

#[cfg(not(windows))]
pub fn wheel_at(_: i32, _: i32, _: i32) {}
#[cfg(not(windows))]
pub fn move_cursor(_: i32, _: i32) {}

#[cfg(test)]
mod tests {
    use super::*;

    /// A "page" of distinct rows; frame k shows rows offset..offset+h, plus a sticky header.
    fn page_frame(offset: u32, h: u32, header: u32) -> RgbaImage {
        RgbaImage::from_fn(20, h, |x, y| {
            let v = if y < header { 7 } else { offset + y }; // header rows identical every frame
            image::Rgba([(v % 251) as u8, (v / 251) as u8, (x * 9) as u8, 255])
        })
    }

    #[test]
    fn finds_scroll_amount_despite_sticky_header() {
        let a = rows(&page_frame(0, 200, 12));
        let b = rows(&page_frame(37, 200, 12));
        assert_eq!(find_shift(&a, &b), Some(37));
        assert_eq!(find_shift(&a, &a), Some(0), "no movement");
        let far = rows(&page_frame(500, 200, 12));
        assert_eq!(find_shift(&a, &far), None, "scrolled past: no overlap");
    }

    /// Real run on screen. SCROLL_TEST="x,y,w,h,out.png" (primary monitor px).
    #[test]
    fn real_scroll_capture() {
        let Ok(spec) = std::env::var("SCROLL_TEST") else { return };
        let p: Vec<&str> = spec.split(',').collect();
        let n = |i: usize| p[i].parse::<u32>().unwrap();
        let (x, y, w, h) = (n(0), n(1), n(2), n(3));
        let mon = xcap::Monitor::all().unwrap().remove(0);
        let grab = || Ok(image::imageops::crop_imm(&mon.capture_image().unwrap(), x, y, w, h).to_image());
        let t = std::time::Instant::now();
        let img = run(grab, || {
            wheel_at((x + w / 2) as i32, (y + h / 2) as i32, (h / 250).clamp(1, 5) as i32);
            std::thread::sleep(std::time::Duration::from_millis(350));
        })
        .unwrap();
        println!("stitched {}x{} in {} ms", img.width(), img.height(), t.elapsed().as_millis());
        img.save(p[4]).unwrap();
    }

    #[test]
    fn blank_frames_dont_fake_a_match() {
        let blank = RgbaImage::from_pixel(20, 200, image::Rgba([255, 255, 255, 255]));
        assert_eq!(find_shift(&rows(&blank), &rows(&blank)), None);
    }

    #[test]
    fn stitches_until_the_page_stops() {
        // Page of 400 rows, 100-row window, scrolls 30 per step, then stops at the bottom.
        let offset = std::cell::Cell::new(0u32);
        let img = run(
            || Ok(page_frame(offset.get(), 100, 0)),
            || offset.set((offset.get() + 30).min(300)),
        )
        .unwrap();
        assert_eq!(img.height(), 400);
        assert_eq!(img, page_frame(0, 400, 0), "stitched image equals the whole page");
    }
}
