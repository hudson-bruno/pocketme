//! Exact pixels for each drawing helper, including the parts cut off at
//! the edges.

use console_video::{CLEAR, FONT_H, Frame, H, Sprite, W, text_width};

fn frame() -> Box<Frame> {
    Box::new(Frame::new())
}

/// The frame as text: one character per pixel ('.' for 0, the number's
/// last digit otherwise), only the rows and columns asked for.
fn picture(f: &Frame, x0: u16, y0: u16, w: u16, h: u16) -> Vec<String> {
    (y0..y0 + h)
        .map(|y| {
            (x0..x0 + w)
                .map(|x| match f.get(x, y) {
                    0 => '.',
                    c => char::from(b'0' + c % 10),
                })
                .collect()
        })
        .collect()
}

/// Every pixel outside the given rectangle is still 0.
fn untouched_outside(f: &Frame, x0: u16, y0: u16, w: u16, h: u16) {
    for y in 0..H as u16 {
        for x in 0..W as u16 {
            let inside = x >= x0 && x < x0 + w && y >= y0 && y < y0 + h;
            if !inside {
                assert_eq!(f.get(x, y), 0, "pixel ({x},{y}) drawn");
            }
        }
    }
}

#[test]
fn clear_and_pixel() {
    let mut f = frame();
    f.clear(3);
    assert!(f.0.iter().all(|&p| p == 3));
    f.clear(0);
    f.pixel(2, 1, 7);
    f.pixel(-1, 0, 7);
    f.pixel(0, -1, 7);
    f.pixel(160, 0, 7);
    f.pixel(0, 120, 7);
    assert_eq!(picture(&f, 0, 0, 4, 3), ["....", "..7.", "...."]);
    untouched_outside(&f, 2, 1, 1, 1);
}

#[test]
fn lines() {
    let mut f = frame();
    f.hline(1, 1, 3, 1);
    f.vline(5, 0, 3, 2);
    assert_eq!(
        picture(&f, 0, 0, 7, 4),
        [".....2.", ".111.2.", ".....2.", "......."]
    );
    // Cut at the edges, and zero length draws nothing.
    let mut g = frame();
    g.hline(-2, 0, 4, 1);
    g.hline(158, 119, 10, 1);
    g.vline(159, -5, 6, 2);
    g.hline(50, 50, 0, 3);
    assert_eq!(picture(&g, 0, 0, 3, 1), ["11."]);
    assert_eq!(picture(&g, 157, 119, 3, 1), [".11"]);
    assert_eq!(picture(&g, 159, 0, 1, 2), ["2", "."]);
    assert_eq!(g.0.iter().filter(|&&p| p != 0).count(), 2 + 2 + 1);
}

#[test]
fn filled_rectangle_and_outline() {
    let mut f = frame();
    f.rect(1, 1, 3, 2, 4);
    f.outline(6, 0, 4, 4, 5);
    assert_eq!(
        picture(&f, 0, 0, 11, 5),
        [
            "......5555.",
            ".444..5..5.",
            ".444..5..5.",
            "......5555.",
            "...........",
        ]
    );
    // Cut on all four sides: only the picture is filled.
    let mut g = frame();
    g.rect(-10, -10, 400, 400, 6);
    assert!(g.0.iter().all(|&p| p == 6));
    // Wholly outside, or empty: nothing.
    let mut h = frame();
    h.rect(160, 0, 5, 5, 1);
    h.rect(0, -5, 5, 5, 1);
    h.rect(3, 3, 0, 5, 1);
    h.outline(3, 3, 0, 0, 1);
    assert!(h.0.iter().all(|&p| p == 0));
}

const ARROW: Sprite<'static> = Sprite::new(
    3,
    3,
    &[
        CLEAR, 1, CLEAR, //
        2, 3, 4, //
        CLEAR, 5, CLEAR,
    ],
);

#[test]
fn sprite_with_see_through_colour() {
    let mut f = frame();
    f.clear(9);
    f.sprite(1, 1, &ARROW, CLEAR);
    assert_eq!(
        picture(&f, 0, 0, 5, 5),
        ["99999", "99199", "92349", "99599", "99999"]
    );
    // The same sprite without a see-through colour copies everything.
    f.blit(1, 1, &ARROW);
    assert_eq!(picture(&f, 1, 1, 3, 1), ["515"]);
}

#[test]
fn sprite_cut_at_every_edge() {
    let mut f = frame();
    f.sprite(-1, -1, &ARROW, CLEAR);
    assert_eq!(picture(&f, 0, 0, 3, 3), ["34.", "5..", "..."]);
    let mut g = frame();
    g.sprite(158, 118, &ARROW, CLEAR);
    assert_eq!(picture(&g, 157, 117, 3, 3), ["...", "..1", ".23"]);
    // Far away: nothing, and no crash.
    let mut h = frame();
    h.sprite(-100, 50, &ARROW, CLEAR);
    h.sprite(i16::MAX, i16::MIN, &ARROW, CLEAR);
    h.blit(i16::MIN, i16::MAX, &ARROW);
    assert!(h.0.iter().all(|&p| p == 0));
}

#[test]
fn text_in_the_built_in_font() {
    let mut f = frame();
    let end = f.text(1, 1, "Hi!", 7);
    // H is 4 wide, I is 3, ! is 1, one empty column after each.
    assert_eq!(end, 1 + 5 + 4 + 2);
    assert_eq!(text_width("Hi!"), 4 + 1 + 3 + 1 + 1);
    assert_eq!(
        picture(&f, 0, 0, 13, 7),
        [
            ".............",
            ".7..7.777.7..",
            ".7..7..7..7..",
            ".7777..7..7..",
            ".7..7..7.....",
            ".7..7.777.7..",
            ".............",
        ]
    );
    assert_eq!(FONT_H, 5);
}

#[test]
fn text_digits_dot_and_unknown_signs() {
    let mut f = frame();
    f.text(0, 0, "1.", 1);
    assert_eq!(
        picture(&f, 0, 0, 5, 5),
        [".1...", "11...", ".1...", ".1...", "111.1"]
    );
    // A sign the font lacks is drawn as "?". Small letters are capitals.
    let (mut a, mut b) = (frame(), frame());
    a.text(0, 0, "~é", 1);
    b.text(0, 0, "??", 1);
    assert!(a.0 == b.0);
    let (mut c, mut d) = (frame(), frame());
    c.text(0, 0, "abc xyz", 1);
    d.text(0, 0, "ABC XYZ", 1);
    assert!(c.0 == d.0);
}

#[test]
fn text_scaled_and_cut() {
    let mut f = frame();
    let end = f.text_scaled(0, 0, "I", 3, 2);
    assert_eq!(end, 8);
    assert_eq!(
        picture(&f, 0, 0, 7, 3),
        ["666666.", "666666.", "..66..."].map(|r| r.replace('6', "3"))
    );
    // Partly off the left and the bottom: cut, and the returned x is the
    // same as if it had all been drawn.
    let mut g = frame();
    let end = g.text(-3, 117, "AB", 1);
    assert_eq!(end, -3 + 5 + 5);
    // Only the last column of "A" shows (x 0), then "B" from x 2.
    assert_eq!(picture(&g, 0, 117, 6, 3), ["..111.", "1.1..1", "1.111."]);
}
