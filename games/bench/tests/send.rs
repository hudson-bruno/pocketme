//! What each level costs the screen: the console's own rule
//! (`console_video::diff`) on the pictures of two calls in a row. Run
//! with `--nocapture` for the table.

mod common;

use bench::load::{Kind, TESTS};
use common::Driver;
use console_video::{Areas, Frame, MAX_AREAS, diff};

/// Pixels sent in `calls` calls (after 40 to settle), the most in one
/// call, the band's share, and the most areas in one call.
fn sent(i: usize, lv: usize, calls: u32) -> (u32, u32, u32, usize) {
    let mut d = Driver::new();
    d.pick(i, lv);
    d.idle(40);
    let mut shown: Box<Frame> = Box::new(d.frame.as_ref().clone());
    let mut areas = Areas::new();
    let (mut sum, mut most, mut band, mut most_areas) = (0, 0, 0, 0);
    for _ in 0..calls {
        d.call(0);
        diff(&shown, &d.frame, &mut areas);
        let px = areas.pixels();
        sum += px;
        most = most.max(px);
        most_areas = most_areas.max(areas.len());
        band += areas
            .as_slice()
            .iter()
            .filter(|r| r.y < 20)
            .map(|r| r.area())
            .sum::<u32>();
        *shown = d.frame.as_ref().clone();
    }
    (sum, most, band, most_areas)
}

#[test]
fn what_each_level_sends() {
    let calls = 90; // three seconds at 30
    println!("level                 load   avg px/call  most px  band px in {calls} calls  areas");
    for (i, t) in TESTS.iter().enumerate() {
        for lv in 0..t.loads.len() {
            let load = t.loads[lv];
            if load > 4096 && matches!(t.kind, Kind::Tri(_) | Kind::Vert) {
                continue;
            }
            let (sum, most, band, areas) = sent(i, lv, calls);
            let avg = sum / calls;
            println!(
                "{:<20} {load:>6}   {avg:>11}  {most:>7}  {band:>21}  {areas:>5}",
                t.name
            );
            assert!(areas <= MAX_AREAS);
            match t.kind {
                // Static: only the band, at most a few times.
                Kind::Fill | Kind::Tri(_) | Kind::Vert => {
                    assert_eq!(sum, band, "{}: only the band changes", t.name);
                    assert!(band <= 4 * 160 * 20, "{}: band {band}", t.name);
                }
                Kind::Rows => {
                    let rows = load.min(100) * 120 / 100;
                    let changed = if rows >= 120 { 160 * 120 } else { rows * 160 };
                    assert!(most >= changed && most <= changed + 160 * 20, "{}", t.name);
                }
                Kind::Dots => {
                    let changed = load * 240 / 100 * 64;
                    assert!(most >= changed, "the console sends what changed");
                }
                Kind::Scene(_) | Kind::Spin | Kind::City | Kind::Island => assert!(avg > 0),
            }
        }
    }
}

#[test]
fn the_band_changes_at_most_once_a_second() {
    // FILL at 30: 30 calls a second; the band's numbers change when a
    // window ends.
    let mut d = Driver::new();
    d.pick(0, 1);
    d.idle(40);
    let mut shown: Box<Frame> = Box::new(d.frame.as_ref().clone());
    let mut areas = Areas::new();
    let mut changes = 0;
    for _ in 0..300 {
        d.call(0);
        diff(&shown, &d.frame, &mut areas);
        if !areas.is_empty() {
            changes += 1;
        }
        *shown = d.frame.as_ref().clone();
    }
    // 10 s; the steady numbers are the same in each window.
    assert!(changes <= 10, "{changes} band changes in 10 s");
}

#[test]
fn the_send_levels_change_the_share_they_say() {
    // 30 calls: 1 s at 30, so the band's numbers change at most once.
    let band_once = 160 * 20;
    let (s0, ..) = sent(8, 0, 30);
    assert!(s0 <= band_once, "0 %: only the band");
    let (s25, ..) = sent(8, 1, 30);
    assert!(s25 >= 30 * 30 * 160 && s25 <= 30 * 30 * 160 + band_once);
    let (s50, ..) = sent(8, 2, 30);
    assert!(s50 >= 30 * 60 * 160 && s50 <= 30 * 60 * 160 + band_once);
    let (s100, ..) = sent(8, 4, 30);
    assert_eq!(s100, 30 * 160 * 120, "100 %: everything, every call");
}
