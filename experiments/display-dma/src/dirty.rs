//! Finding what changed between two logical screens.
//!
//! Each row is compared as 40 words (4 pixels each). A changed row gives
//! a span from its first to its last changed word. Runs of changed rows
//! are merged into bands, each band one rectangle covering the union of
//! its rows' spans. Too many bands are merged until at most
//! [`MAX_RECTS`] are left: every rectangle costs a window command set
//! on the wire.
//!
//! Spans are in whole words, so every rectangle starts and ends on a
//! multiple of 4 logical pixels: what 12-bit mode needs.

use crate::pipeline::{Frame, LH, LW, Rect};

pub const MAX_RECTS: usize = 8;
const WORDS: usize = LW / 4;

/// Span of changed words in one row, or `None`.
#[inline(always)]
fn row_span(a: &[u32], b: &[u32]) -> Option<(usize, usize)> {
    let first = a.iter().zip(b).position(|(x, y)| x != y)?;
    // There is a difference, so a last one exists too.
    let last = WORDS
        - 1
        - a.iter()
            .rev()
            .zip(b.iter().rev())
            .position(|(x, y)| x != y)
            .unwrap_or(0);
    Some((first, last))
}

/// Compares two screens. Writes up to [`MAX_RECTS`] rectangles into
/// `out` and returns how many.
#[esp_hal::ram] // in RAM: no flash cache misses; ram also means not inlined
pub fn diff(prev: &Frame, cur: &Frame, out: &mut [Rect; MAX_RECTS]) -> usize {
    // Bands as (y0, y1 exclusive, word0, word1 inclusive). At most one
    // band per two rows (a band needs a changed row and an unchanged
    // one after it).
    let mut bands = [(0u16, 0u16, 0u16, 0u16); LH / 2 + 1];
    let mut count = 0;
    let (pw, cw) = (prev.words(), cur.words());
    let mut open = false;
    for y in 0..LH {
        let span = row_span(&pw[y * WORDS..][..WORDS], &cw[y * WORDS..][..WORDS]);
        match (span, open) {
            (Some((a, b)), true) => {
                let band = &mut bands[count - 1];
                band.1 = y as u16 + 1;
                band.2 = band.2.min(a as u16);
                band.3 = band.3.max(b as u16);
            }
            (Some((a, b)), false) => {
                bands[count] = (y as u16, y as u16 + 1, a as u16, b as u16);
                count += 1;
                open = true;
            }
            (None, _) => open = false,
        }
    }
    // Too many: merge the two neighbours with the smallest gap between
    // them, again and again.
    while count > MAX_RECTS {
        let mut best = 0;
        let mut best_gap = u16::MAX;
        for i in 0..count - 1 {
            let gap = bands[i + 1].0 - bands[i].1;
            if gap < best_gap {
                best_gap = gap;
                best = i;
            }
        }
        let next = bands[best + 1];
        let band = &mut bands[best];
        band.1 = next.1;
        band.2 = band.2.min(next.2);
        band.3 = band.3.max(next.3);
        bands.copy_within(best + 2..count, best + 1);
        count -= 1;
    }
    for (rect, band) in out.iter_mut().zip(&bands[..count]) {
        *rect = Rect {
            x: band.2 * 4,
            y: band.0,
            w: (band.3 - band.2 + 1) * 4,
            h: band.1 - band.0,
        };
    }
    count
}
