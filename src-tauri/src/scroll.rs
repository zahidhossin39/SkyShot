//! Scrolling capture: scroll the area under the selection with the mouse wheel, grab it after
//! each step, and stitch the frames into one tall image by matching pixel rows.
//!
//! The overlap search (`find_shift`) is pure and unit-tested; the loop does the I/O.

use image::RgbaImage;

/// Safety limits so a page that never ends can't run forever.
const MAX_STEPS: usize = 150; // ~2 min worst case at ~0.8 s per step
const MAX_HEIGHT: u32 = 20_000;

/// Longest wait after each scroll for the area to stop changing. Smooth scrolling and
/// scroll-triggered fade-ins need ~0.5 s; at 350 ms the stitch caught images half faded in.
pub const SETTLE_MS: u64 = 700;
/// Grab again this often while waiting for the area to settle.
const POLL_MS: u64 = 40;

/// One vertical strip of a frame: per row, its signature and whether it is blank.
type Strip = Vec<(u64, bool)>;

/// Share of row-strips alike between two frames at the same place (a blinking caret or a small
/// spinner doesn't count as "still changing").
fn alike(a: &[Strip], b: &[Strip]) -> bool {
    let pairs = a.iter().zip(b).flat_map(|(p, n)| p.iter().zip(n));
    let (same, all) = pairs.fold((0u32, 0u32), |(s, t), (p, n)| (s + (p.0.abs_diff(n.0) <= SIG_TOLERANCE) as u32, t + 1));
    same as f32 >= 0.995 * all.max(1) as f32
}

/// Grab until the area stops changing (two grabs in a row alike) after moving away from
/// `before`, or `max_ms` passes. A fixed 700 ms wait made every step slow when most apps settle
/// in ~100 ms; fades and smooth scrolling still get the full wait. "Moved away from before"
/// keeps a scroll that hasn't started yet from passing as the end of the page.
fn settled(
    grab: &mut impl FnMut() -> Result<RgbaImage, String>,
    before: &[Strip],
    max_ms: u64,
) -> Result<(RgbaImage, Vec<Strip>), String> {
    let start = std::time::Instant::now();
    let mut frame = grab()?;
    let mut sig = rows(&frame);
    while start.elapsed().as_millis() < max_ms as u128 {
        std::thread::sleep(std::time::Duration::from_millis(POLL_MS));
        let next = grab()?;
        let next_sig = rows(&next);
        let calm = alike(&sig, &next_sig) && !alike(&next_sig, before);
        (frame, sig) = (next, next_sig);
        if calm {
            break;
        }
    }
    Ok((frame, sig))
}

/// A row's signature: average of R+G+B per pixel, x16 for precision. Not an exact hash, because
/// browsers re-render text a touch differently after a scroll (~5% of pixels off by 1).
fn row_sig(row: &[u8]) -> u64 {
    let px = (row.len() / 4).max(1) as u64;
    let sum: u64 = row.as_chunks::<4>().0.iter().map(|p| p[0] as u64 + p[1] as u64 + p[2] as u64).sum();
    sum * 16 / px
}

/// Rows count as the same within 2 levels per pixel; a wrong shift was 18-90 apart on real pages.
const SIG_TOLERANCE: u64 = 2 * 16;

/// A row that's one colour carries no position info (blank space matches any shift).
fn is_blank(row: &[u8]) -> bool {
    row.as_chunks::<4>().0.iter().all(|px| px == &row[..4])
}

/// Vertical strips matched separately, so a moving part of the page (an animated card, a video,
/// a blinking cursor) only spoils its own strips instead of every row it crosses.
const STRIPS: usize = 8;

/// Per strip: (hash, blank) per row.
fn rows(img: &RgbaImage) -> Vec<Strip> {
    let w = img.width() as usize;
    let strip = w.div_ceil(STRIPS).max(1);
    let rows: Vec<&[u8]> = img.as_raw().chunks_exact(w * 4).collect();
    (0..w)
        .step_by(strip)
        .map(|x0| {
            let x1 = (x0 + strip).min(w);
            rows.iter().map(|r| &r[x0 * 4..x1 * 4]).map(|r| (row_sig(r), is_blank(r))).collect()
        })
        .collect()
}

/// How many rows `next` is scrolled down from `prev` (0 = didn't move), or None if no shift fits.
/// Each strip lists every shift it's consistent with; the shift the most strips accept wins.
/// (One vote per strip failed on repeating lines: every text strip fit any multiple of the line
/// height, and only the line-number strip knew the real one, so it was outvoted.)
fn find_shift(prev: &[Strip], next: &[Strip]) -> Option<usize> {
    if frame_fit(prev, next, 0) >= 0.99 {
        return Some(0); // nothing changed, even where there's nothing to match on
    }
    let fits: Vec<Vec<Option<(f32, f32)>>> = prev.iter().zip(next).map(|(p, n)| strip_fits(p, n)).collect();
    let shifts = fits.iter().map(Vec::len).min().unwrap_or(0);
    // (shift, strips accepting it, mean share, mean difference)
    let scored: Vec<(usize, usize, f32, f32)> = (0..shifts)
        .filter_map(|s| {
            let ok: Vec<(f32, f32)> = fits.iter().filter_map(|f| f[s]).collect();
            let n = ok.len();
            (n > 0).then(|| (s, n, ok.iter().map(|f| f.0).sum::<f32>() / n as f32, ok.iter().map(|f| f.1).sum::<f32>() / n as f32))
        })
        .collect();
    let most = scored.iter().map(|c| c.1).max()?;
    // A lone strip is a coincidence (a repeating grid, a scrollbar edge): it said 70 px on a real
    // site that moved 300. `run` falls back to `expected_shift` instead.
    if most < 2 && prev.len() > 1 {
        return None;
    }
    let mut top: Vec<_> = scored.into_iter().filter(|c| c.1 == most).collect();
    // On a tie, movement beats "didn't move": a sticky column stays put while the page scrolls.
    if top.iter().any(|c| c.0 > 0) {
        top.retain(|c| c.0 > 0);
    }
    // Neighbouring shifts can score almost as well; among the near-best, the closest fit wins.
    let share = top.iter().map(|c| c.2).fold(0.0, f32::max);
    top.into_iter().filter(|c| c.2 >= share - 0.05).min_by(|a, b| a.3.total_cmp(&b.3)).map(|c| c.0)
}

/// When the strips can't agree (an overlap that's mostly blank, content fading in, a section that
/// expands as you scroll), take the shift near the expected step that fits the whole frame best,
/// blank rows included: a black/white section edge then pins it down exactly.
fn expected_shift(prev: &[Strip], next: &[Strip], guess: usize) -> (usize, f32) {
    let h = prev[0].len().min(next[0].len());
    let hi = (guess * 5 / 4).min(h.saturating_sub(2 * (h / 10) + 17));
    let mut best = (guess.min(hi), -1.0f32);
    for s in guess / 2..=hi {
        let f = frame_fit(prev, next, s);
        let closer = s.abs_diff(guess) < best.0.abs_diff(guess);
        if f > best.1 + 0.01 || (f >= best.1 - 0.01 && closer) {
            best = (s, f.max(best.1));
        }
    }
    best
}

/// Share of rows (all strips, blank rows too, top/bottom 10% ignored) that line up at shift `s`.
fn frame_fit(prev: &[Strip], next: &[Strip], s: usize) -> f32 {
    let h = prev[0].len().min(next[0].len());
    let m = h / 10;
    let pairs = prev.iter().zip(next).flat_map(|(p, n)| (m..h - s - m).map(move |i| (p[i + s].0, n[i].0)));
    let (same, all) = pairs.fold((0, 0), |(a, b), (p, n)| (a + (p.abs_diff(n) <= SIG_TOLERANCE) as u32, b + 1));
    same as f32 / all.max(1) as f32
}

/// The refined guess is trusted from this fit up (right ones scored 0.98+ on a real site). Below
/// it (a pinned section, content swapping in place) the plain step size was right more often:
/// refined guesses there scored 0.64-0.71 and one doubled a row of text.
const GOOD_FIT: f32 = 0.8;

/// Steps that didn't move before calling it the end. A pinned section (the page holds still while
/// you scroll through it) gave 2 in a row on a real site and ended the capture at 6500 of 14000 px.
const END_STILL_STEPS: usize = 4;

/// Unmatched steps in a row before giving up. A pinned section on a real site took 4+ in a row;
/// a frame that didn't change at all counts as "didn't move" instead, so a blank end still stops.
const MAX_GUESSES: usize = 10;

/// The typical distance per notch (median), for guessing. Not the max: one odd step shouldn't skew it.
fn median(v: &[f32]) -> f32 {
    let mut v = v.to_vec();
    v.sort_by(f32::total_cmp);
    v.get(v.len() / 2).copied().unwrap_or(0.0)
}

/// Share of rows that must line up. Scroll effects (text recolouring, images fading in, a sticky
/// header over content) changed ~1/3 of the overlap on a real site; a wrong shift matched <10%.
const MIN_MATCH: f32 = 0.6;

/// One strip: for each shift, Some((share of rows matching, mean difference of those rows)) if it
/// fits. Ignores the top and bottom 10% of the frame: sticky headers/footers stay put while
/// content moves.
fn strip_fits(prev: &[(u64, bool)], next: &[(u64, bool)]) -> Vec<Option<(f32, f32)>> {
    let h = prev.len().min(next.len());
    let m = h / 10;
    let mut fits = Vec::new();
    for s in 0..h.saturating_sub(2 * m + 16) {
        // next row i shows what prev row i + s showed
        let (mut seen, mut same, mut diff) = (0, 0, 0u64);
        for i in m..h - s - m {
            let d = next[i].0.abs_diff(prev[i + s].0);
            if next[i].1 && prev[i + s].1 && d <= SIG_TOLERANCE {
                continue; // the same blank on both sides: tells us nothing
            }
            seen += 1;
            if d <= SIG_TOLERANCE {
                same += 1;
                diff += d;
            }
        }
        let share = same as f32 / seen.max(1) as f32;
        fits.push((seen >= 8 && share >= MIN_MATCH).then(|| (share, diff as f32 / same as f32)));
    }
    fits
}

/// Rows at the bottom that stay put while the page scrolls: a chat's message box, a sticky footer,
/// a cookie bar. Appending each frame's bottom rows repeated them down the image (a chat app's
/// composer showed up 3 times), so `run` stitches from just above them and adds them once at the
/// end. Per strip, so an animated bit (a spinner in the message box) only breaks its own strip.
/// Over-counting is harmless: the page rows held back are stitched from the next frame instead.
fn fixed_bottom(prev: &[Strip], next: &[Strip]) -> usize {
    let h = prev[0].len().min(next[0].len());
    // The whole matching run counts (a message box has blank padding above it), but only if it
    // has a non-blank row: a strip of plain margin matches at any shift and proves nothing.
    let strip_run = |(p, n): (&Strip, &Strip)| {
        let run = (0..h).rev().take_while(|&i| p[i].0.abs_diff(n[i].0) <= SIG_TOLERANCE).count();
        let anchored = n[h - run..].iter().any(|r| !r.1);
        if anchored { run } else { 0 }
    };
    prev.iter().zip(next).map(strip_run).max().unwrap_or(0).min(h / 3)
}

/// True while a scrolling capture runs, so a new capture can't start on top of it.
pub static ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Wheel notches for the next step: aim to move ~1/3 of the area, using the LARGEST distance one
/// notch has moved so far. (A fixed small step made a 7400 px page take 99 steps; sizing from
/// the last step alone overshot on smooth-scrolling sites, where a notch moved 96-136 px. Half
/// the area left too little overlap on an animated site: 17 guessed steps vs 4 at a third.)
fn next_notches(area_h: u32, max_per_notch: f32) -> i32 {
    if max_per_notch <= 0.0 {
        return 1;
    }
    ((area_h as f32 * 0.35 / max_per_notch) as i32).clamp(1, 15)
}

/// `run_with` without waiting for the area to settle (fake pages are still the moment they move).
#[cfg(test)]
fn run(grab: impl FnMut() -> Result<RgbaImage, String>, scroll: impl FnMut(i32) -> bool) -> Result<RgbaImage, String> {
    run_with(grab, scroll, 0)
}

/// Runs the whole capture. `grab` captures the area; `scroll(n)` scrolls `n` notches and
/// returns false if the user wants to stop (what's stitched so far is kept). After each scroll
/// it waits up to `settle_ms` for the area to stop changing.
pub fn run_with(
    mut grab: impl FnMut() -> Result<RgbaImage, String>,
    mut scroll: impl FnMut(i32) -> bool,
    settle_ms: u64,
) -> Result<RgbaImage, String> {
    let first = grab()?;
    let (width, area_h) = first.dimensions();
    let (stride, h) = (width as usize * 4, area_h as usize);
    let mut out = first.as_raw().clone();
    let mut height = area_h;
    let mut prev = rows(&first);
    let mut prev_frame = first;
    let mut tail = 0; // bottom rows of `prev_frame` not in `out` yet (fixed rows, held back)
    let mut still = 0;
    let mut notches = 1;
    let mut max_per_notch = 0f32;
    let mut per_notch = Vec::new(); // px per notch of each matched step
    let mut guesses = 0; // unmatched steps in a row
    for _ in 0..MAX_STEPS {
        if !scroll(notches) {
            crate::log!("[scroll] stopped by the user");
            break;
        }
        let (frame, next) = settled(&mut grab, &prev, settle_ms)?;
        let mut shift = find_shift(&prev, &next);
        let guess = (notches as f32 * median(&per_notch)).round() as usize;
        let mut fit = None;
        if shift.is_none() && guess > 0 && guesses < MAX_GUESSES {
            guesses += 1;
            let (s, f) = expected_shift(&prev, &next, guess);
            fit = Some(f);
            shift = Some(if f >= GOOD_FIT { s } else { guess });
        } else if shift.is_some() {
            guesses = 0;
        }
        crate::log!("[scroll] step: {notches} notch(es), shift {shift:?} (guess fit {fit:?}), height so far {height}");
        match shift {
            Some(0) => {
                still += 1;
                if still >= END_STILL_STEPS {
                    break; // reached the end (or the area doesn't scroll)
                }
            }
            Some(s) => {
                still = 0;
                let s = s.min(h);
                let fixed = fixed_bottom(&prev, &next).min(h.saturating_sub(s + 1));
                if s + tail > h {
                    // Moved further than the held-back rows reach: keep them (a rare gap either way).
                    out.extend_from_slice(&prev_frame.as_raw()[(h - tail) * stride..]);
                    tail = 0;
                }
                // Take the fixed rows back off the stitch; they're added once, at the end.
                let cut = fixed.saturating_sub(tail);
                out.truncate(out.len() - cut * stride);
                tail += cut;
                // The stitch ends at frame row h - tail - s; add the new rows down to the fixed ones.
                out.extend_from_slice(&frame.as_raw()[(h - tail - s) * stride..(h - fixed) * stride]);
                tail = fixed;
                height = (out.len() / stride) as u32;
                if guesses == 0 {
                    per_notch.push(s as f32 / notches as f32);
                    max_per_notch = max_per_notch.max(s as f32 / notches as f32);
                    notches = next_notches(area_h, max_per_notch);
                }
            }
            None => break, // scrolled too far to line up (or the content changed): stop cleanly
        }
        prev = next;
        prev_frame = frame;
        if height >= MAX_HEIGHT {
            break;
        }
    }
    out.extend_from_slice(&prev_frame.as_raw()[(h - tail) * stride..]); // the fixed rows, once
    height = (out.len() / stride) as u32;
    RgbaImage::from_raw(width, height, out).ok_or_else(|| "stitch size mismatch".into())
}

/// The user took over: moved the mouse away from where we parked it, or pressed Esc.
#[cfg(windows)]
pub fn user_interrupted(x: i32, y: i32) -> bool {
    use windows::Win32::{
        Foundation::POINT,
        UI::{Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE}, WindowsAndMessaging::GetCursorPos},
    };
    unsafe {
        let mut p = POINT::default();
        let moved = GetCursorPos(&mut p).is_ok() && ((p.x - x).abs() > 4 || (p.y - y).abs() > 4);
        moved || GetAsyncKeyState(VK_ESCAPE.0 as i32) as u16 & 0x8000 != 0
    }
}

/// Wheel at the current cursor position (the caller parks the cursor once, then leaves it).
#[cfg(windows)]
pub fn wheel(notches: i32) {
    use windows::Win32::UI::{
        Input::KeyboardAndMouse::{SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_WHEEL, MOUSEINPUT},
        WindowsAndMessaging::WHEEL_DELTA,
    };
    unsafe {
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

// macOS / Linux X11 via enigo. macOS asks for the Accessibility permission the first time.
// Wayland doesn't let apps send input, so these do nothing there and the capture just ends.
#[cfg(not(windows))]
fn with_enigo(f: impl FnOnce(&mut enigo::Enigo)) {
    match enigo::Enigo::new(&enigo::Settings::default()) {
        Ok(mut e) => f(&mut e),
        Err(e) => crate::log!("[scroll] no input access: {e}"),
    }
}
#[cfg(not(windows))]
pub fn wheel(notches: i32) {
    use enigo::Mouse;
    with_enigo(|e| {
        let _ = e.scroll(notches, enigo::Axis::Vertical);
    });
}
/// Moved the mouse away from where we parked it. (No Esc check here: reading key state
/// globally needs more permissions than it's worth.)
#[cfg(not(windows))]
pub fn user_interrupted(x: i32, y: i32) -> bool {
    use enigo::Mouse;
    let mut moved = false;
    with_enigo(|e| {
        if let Ok((px, py)) = e.location() {
            moved = (px - x).abs() > 4 || (py - y).abs() > 4;
        }
    });
    moved
}
#[cfg(not(windows))]
pub fn move_cursor(x: i32, y: i32) {
    use enigo::Mouse;
    with_enigo(|e| {
        let _ = e.move_mouse(x, y, enigo::Coordinate::Abs);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A "page" of distinct rows; frame k shows rows offset..offset+h, plus a sticky header.
    fn page_frame(offset: u32, h: u32, header: u32) -> RgbaImage {
        RgbaImage::from_fn(20, h, |x, y| {
            let v = if y < header { 7 } else { offset + y }; // header rows identical every frame
            // Pseudo-random per page row (like real content), so rows far apart don't look alike.
            let h = v.wrapping_mul(2_654_435_761).rotate_left(7) ^ (x * 40_503);
            image::Rgba([h as u8, (h >> 8) as u8, (h >> 16) as u8, 255])
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
    #[cfg(not(target_os = "linux"))]
    #[test]
    fn real_scroll_capture() {
        let Ok(spec) = std::env::var("SCROLL_TEST") else { return };
        let p: Vec<&str> = spec.split(',').collect();
        let n = |i: usize| p[i].parse::<u32>().unwrap();
        let (x, y, w, h) = (n(0), n(1), n(2), n(3));
        let mon = xcap::Monitor::all().unwrap().remove(0);
        let grab = || Ok(image::imageops::crop_imm(&mon.capture_image().unwrap(), x, y, w, h).to_image());
        let t = std::time::Instant::now();
        move_cursor((x + w / 2) as i32, (y + h / 2) as i32);
        let go_on = |n| {
            wheel(n);
            std::thread::sleep(std::time::Duration::from_millis(60));
            true
        };
        let img = run_with(grab, go_on, SETTLE_MS).unwrap();
        println!("stitched {}x{} in {} ms", img.width(), img.height(), t.elapsed().as_millis());
        img.save(p[4]).unwrap();
    }

    /// Regression (zaidhossain.com): a live animated card on the right changes every frame.
    /// Whole-row matching then never reached 95% and the capture stopped after one frame.
    #[test]
    fn animated_region_doesnt_break_matching() {
        let frame = |offset: u32, tick: u32| {
            let mut img = page_frame(offset, 200, 0);
            for y in 20..180 {
                for x in 14..20 {
                    img.put_pixel(x, y, image::Rgba([(tick * 37 % 255) as u8, (y * tick % 255) as u8, 9, 255]));
                }
            }
            img
        };
        assert_eq!(find_shift(&rows(&frame(0, 1)), &rows(&frame(40, 2))), Some(40));
        assert_eq!(find_shift(&rows(&frame(40, 2)), &rows(&frame(40, 3))), Some(0), "only the animation changed");
    }

    /// Regression (zaidhossain.com): a sticky left column voted "didn't move" as often as the
    /// scrolling right side voted 153, and the tie ended the capture before the footer.
    #[test]
    fn sticky_column_doesnt_win_a_tie() {
        let (mut prev, mut next) = (page_frame(0, 200, 0), page_frame(30, 200, 0));
        for y in 0..200 {
            for x in 0..9 {
                next.put_pixel(x, y, *prev.get_pixel(x, y)); // left 3 strips stay put
            }
            for x in 18..20 {
                prev.put_pixel(x, y, image::Rgba([255; 4])); // last strip blank: no vote, so 3 vs 3
                next.put_pixel(x, y, image::Rgba([255; 4]));
            }
        }
        assert_eq!(find_shift(&rows(&prev), &rows(&next)), Some(30));
    }

    /// Regression (zaidhossain.com): browsers re-render text slightly differently after a scroll
    /// (95% of pixels identical, the rest off by ~1), so exact row matching found nothing.
    #[test]
    fn tiny_rendering_noise_still_matches() {
        let noisy = |offset: u32, seed: u32| {
            let mut img = page_frame(offset, 200, 0);
            for (x, _, px) in img.enumerate_pixels_mut() {
                if x % 3 == seed % 3 {
                    px.0[0] = px.0[0].saturating_add(1); // a pixel in every strip row is off by one
                }
            }
            img
        };
        assert_eq!(find_shift(&rows(&noisy(0, 1)), &rows(&noisy(60, 3))), Some(60));
    }

    /// Regression (zaidhossain.com): scroll-triggered effects (text changing colour, images fading
    /// in, a sticky header over content) change a third of the overlap across the full width.
    #[test]
    fn content_changing_in_the_overlap_still_matches() {
        let mut next = page_frame(40, 200, 0);
        for y in 90..130 {
            for x in 0..20 {
                next.put_pixel(x, y, image::Rgba([200, 30, (x * 9) as u8, 255]));
            }
        }
        assert_eq!(find_shift(&rows(&page_frame(0, 200, 0)), &rows(&next)), Some(40));
    }

    /// Regression (zaidhossain.com): an overlap that's all blank except one changing strip. The lone
    /// strip's vote was wrong; the black/white section edge gives the real shift.
    #[test]
    fn mostly_blank_overlap_uses_the_section_edge() {
        let frame = |edge: u32, tick: u8| {
            RgbaImage::from_fn(20, 200, |x, y| match (x, y) {
                (19.., 120..180) => image::Rgba([tick, tick.wrapping_mul(7), 3, 255]), // fading-in bit
                (_, y) if y < edge => image::Rgba([10, 10, 10, 255]),
                _ => image::Rgba([255, 255, 255, 255]),
            })
        };
        let (a, b) = (rows(&frame(150, 1)), rows(&frame(50, 90)));
        assert_eq!(find_shift(&a, &b), None, "a lone strip doesn't decide");
        assert_eq!(expected_shift(&a, &b, 96).0, 100, "edge moved 150 -> 50");
        assert!(expected_shift(&a, &b, 96).1 >= GOOD_FIT);
        let swapped = rows(&page_frame(900, 200, 0)); // pinned section: new content, same place
        assert!(expected_shift(&b, &swapped, 96).1 < GOOD_FIT, "nothing fits: use the plain step");
    }

    /// A stretch of page with nothing to match on (blank, one flickering column) is crossed by
    /// guessing, and the stitch still equals the page.
    #[test]
    fn crosses_a_stretch_with_nothing_to_match() {
        let whole = page_frame(0, 400, 0);
        let content = |x: u32, y: u32| if (120..220).contains(&y) { image::Rgba([255; 4]) } else { *whole.get_pixel(x, y) };
        let page = |offset: u32, tick: u32| {
            RgbaImage::from_fn(20, 100, |x, y| {
                if x == 19 { image::Rgba([(tick * 53) as u8, 9, 9, 255]) } else { content(x, offset + y) } // x=19 flickers
            })
        };
        let (offset, tick) = (std::cell::Cell::new(0u32), std::cell::Cell::new(0u32));
        let img = run(
            || {
                tick.set(tick.get() + 1);
                Ok(page(offset.get(), tick.get()))
            },
            |n| {
                offset.set((offset.get() + 10 * n as u32).min(300));
                true
            },
        )
        .unwrap();
        assert_eq!(img.height(), 400);
        for y in 0..400 {
            for x in 0..19 {
                assert_eq!(*img.get_pixel(x, y), content(x, y), "pixel {x},{y}");
            }
        }
    }

    /// A page that ends in blank space stops instead of guessing its way down 10 more steps.
    #[test]
    fn blank_end_of_page_stops() {
        let offset = std::cell::Cell::new(0u32);
        let page = |o: u32| RgbaImage::from_fn(20, 100, |x, y| if o + y < 150 { *page_frame(0, 150, 0).get_pixel(x, o + y) } else { image::Rgba([255; 4]) });
        let img = run(|| Ok(page(offset.get())), |n| { offset.set((offset.get() + 10 * n as u32).min(300)); true }).unwrap();
        // Once the window is all blank, moving and stopped look the same: it stops, content kept.
        assert!((150..=300).contains(&img.height()), "got {}", img.height());
        assert_eq!(*img.get_pixel(5, 149), *page_frame(0, 150, 0).get_pixel(5, 149));
    }

    /// Regression (Claude desktop app): a chat's message box stays put at the bottom, with blank
    /// padding above it and a spinner in it. It was stitched in on every step and repeated.
    #[test]
    fn fixed_message_box_appears_once_at_the_end() {
        let whole = page_frame(0, 400, 0);
        let message_box = page_frame(9_000, 30, 0);
        let frame = |offset: u32, tick: u32| {
            RgbaImage::from_fn(20, 100, |x, y| match y {
                0..70 => *whole.get_pixel(x, offset + y),
                70..74 => image::Rgba([30, 30, 30, 255]), // padding: blank, the same every frame
                _ if x >= 17 && (84..92).contains(&y) => image::Rgba([(tick * 61) as u8, 9, 9, 255]), // spinner
                _ => *message_box.get_pixel(x, y - 70),
            })
        };
        let (offset, tick) = (std::cell::Cell::new(0u32), std::cell::Cell::new(0u32));
        let img = run(
            || {
                tick.set(tick.get() + 1);
                Ok(frame(offset.get(), tick.get()))
            },
            |n| {
                offset.set((offset.get() + 10 * n as u32).min(330));
                true
            },
        )
        .unwrap();
        assert_eq!(img.height(), 430, "whole page, then the box once");
        for y in 0..430 {
            for x in 0..17 {
                let want = if y < 400 { *whole.get_pixel(x, y) } else { *frame(330, 0).get_pixel(x, y - 330) };
                assert_eq!(*img.get_pixel(x, y), want, "pixel {x},{y}");
            }
        }
    }

    /// No fixed bar, but the page has a tall band of identical coloured rows (a banner, a code
    /// block background). As it scrolls past the bottom, its rows match at the same y and get
    /// mistaken for a fixed footer. Held-back rows are re-taken from the next frame, so the
    /// stitch must still be the page, exactly: nothing lost, nothing doubled.
    #[test]
    fn content_mistaken_for_a_fixed_bar_is_still_stitched_exactly() {
        let page = RgbaImage::from_fn(20, 400, |x, y| {
            if (150..210).contains(&y) { image::Rgba([200, 40, 90, 255]) } else { *page_frame(0, 400, 0).get_pixel(x, y) }
        });
        let offset = std::cell::Cell::new(0u32);
        let mut mistaken = 0;
        let mut last = rows(&image::imageops::crop_imm(&page, 0, 0, 20, 100).to_image());
        let img = run(
            || {
                let f = image::imageops::crop_imm(&page, 0, offset.get(), 20, 100).to_image();
                let r = rows(&f);
                mistaken = mistaken.max(fixed_bottom(&last, &r));
                last = r;
                Ok(f)
            },
            |n| {
                offset.set((offset.get() + 10 * n as u32).min(300));
                true
            },
        )
        .unwrap();
        assert!(mistaken > 0, "the band should have been taken for a fixed bar at least once");
        assert_eq!(img.height(), 400);
        assert!(img.as_raw() == page.as_raw(), "stitch differs from the page");
    }

    /// Plain margins on both sides match at any shift; they must not count as a fixed footer.
    #[test]
    fn blank_margins_are_not_a_fixed_footer() {
        let framed = |offset: u32| {
            let page = page_frame(offset, 100, 0);
            RgbaImage::from_fn(20, 100, |x, y| if !(5..15).contains(&x) { image::Rgba([255; 4]) } else { *page.get_pixel(x, y) })
        };
        assert_eq!(fixed_bottom(&rows(&framed(0)), &rows(&framed(30))), 0);
    }

    /// Waits through "hasn't moved yet" and "still moving", and takes the first calm frame.
    #[test]
    fn waits_until_the_area_has_moved_and_settled() {
        let before = page_frame(0, 100, 0);
        let seq = [page_frame(0, 100, 0), page_frame(0, 100, 0), page_frame(20, 100, 0), page_frame(35, 100, 0), page_frame(40, 100, 0), page_frame(40, 100, 0)];
        let i = std::cell::Cell::new(0);
        let mut grab = || {
            i.set(i.get() + 1);
            Ok(seq[(i.get() - 1).min(seq.len() - 1)].clone())
        };
        let (frame, _) = settled(&mut grab, &rows(&before), 5_000).unwrap();
        assert_eq!(frame, page_frame(40, 100, 0));
        assert_eq!(i.get(), 6, "stopped at the second calm grab, not at the unmoved ones");
        let (_, _) = settled(&mut || Ok(before.clone()), &rows(&before), 100).unwrap(); // end of page: gives up after max_ms
    }

    #[test]
    fn blank_frames_dont_fake_a_shift() {
        let blank = RgbaImage::from_pixel(20, 200, image::Rgba([255, 255, 255, 255]));
        assert_eq!(find_shift(&rows(&blank), &rows(&blank)), Some(0), "nothing changed");
    }

    /// Regression (Notepad, "Line N - the quick brown fox..."): every text strip fits any multiple
    /// of the 25 px line height; only the line-number strip knows it moved 3 lines, not 4.
    #[test]
    fn repeating_lines_use_the_strip_that_differs() {
        let lines = |first: u32| {
            RgbaImage::from_fn(40, 300, |x, y| {
                let (line, dy) = (first + y / 25, y % 25);
                let ink = if x < 5 {
                    (line.wrapping_mul(2_654_435_761) >> (x * 5)) as u8 // "digits": differ per line
                } else {
                    (x * 31 + dy * 17) as u8 // "text": the same on every line
                };
                if dy < 18 { image::Rgba([ink, ink, ink, 255]) } else { image::Rgba([255; 4]) }
            })
        };
        assert_eq!(find_shift(&rows(&lines(0)), &rows(&lines(3))), Some(75));
    }

    #[test]
    fn stitches_until_the_page_stops() {
        // Page of 400 rows, 100-row window, scrolls 30 per step, then stops at the bottom.
        let offset = std::cell::Cell::new(0u32);
        let img = run(
            || Ok(page_frame(offset.get(), 100, 0)),
            |n| {
                offset.set((offset.get() + 10 * n as u32).min(300)); // 10 px per notch
                true
            },
        )
        .unwrap();
        assert_eq!(img.height(), 400);
        assert_eq!(img, page_frame(0, 400, 0), "stitched image equals the whole page");
    }

    #[test]
    fn steps_aim_for_a_third_of_the_area_using_the_biggest_notch() {
        assert_eq!(next_notches(600, 70.0), 3, "70 px per notch -> 3 notches ~ 210 px");
        assert_eq!(next_notches(660, 150.0), 1, "site: 150 px per notch, 660 px area -> 1 notch");
        assert_eq!(next_notches(400, 0.0), 1, "nothing measured yet");
        assert_eq!(next_notches(2000, 20.0), 15, "capped");
        assert_eq!(next_notches(100, 400.0), 1, "never below one notch");
    }

    #[test]
    fn user_can_stop_and_keeps_what_was_stitched() {
        let offset = std::cell::Cell::new(0u32);
        let steps = std::cell::Cell::new(0);
        let img = run(
            || Ok(page_frame(offset.get(), 100, 0)),
            |n| {
                steps.set(steps.get() + 1);
                if steps.get() > 2 {
                    return false; // user moved the mouse
                }
                offset.set(offset.get() + 10 * n as u32);
                true
            },
        )
        .unwrap();
        assert!(img.height() > 100 && img.height() < 400, "partial page, got {}", img.height());
        assert_eq!(img, page_frame(0, img.height(), 0));
    }
}
