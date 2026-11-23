//! The swap rule (feature `swap`, README "Swap: two cores, two
//! pictures").
//!
//! - `Plain` below is the rule without `swap` (src/video.rs), rebuilt
//!   from the crate's public parts. `swap_sends_what_the_plain_rule_sends`
//!   runs in BOTH builds: without the feature it shows that `Plain` is the
//!   old renderer; with it, that the swap renderer sends the same areas,
//!   the same pixels and the same reports, frame by frame.
//! - `Slow` (feature `swap` only) is a sender like a second core: it
//!   keeps the picture handed over and reads it later, in pieces, while
//!   the test goes on drawing. It fails the test if any byte it reads
//!   differs from the picture at hand-over time, if it is handed a second
//!   picture before it was let go, or if the console's drawing picture is
//!   ever the one it holds.
//!
//! Seeded, so every run is the same.

use board_api::fake::{FakeScreen, Walk};
use board_api::{Rect, Screen, SendReport};
use console_video::{Areas, Frame, H, MAX_AREAS, Palette, Source, Video, View, W, diff};

/// A small, fixed random number maker (xorshift64*).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

// ---- The plain rule (what `present` does without `swap`) -------------

/// The renderer without `swap`: draw into `cur`, send the changes,
/// copy `cur` into `shown`. The palette is kept as plain colours plus a
/// version, so that a "game" can write a colour without the version
/// seeing it, as through `Video::game_buffers`.
struct Plain {
    cur: Frame,
    shown: Frame,
    colours: [u16; 256],
    version: u32,
    shown_palette: u32,
    in_step: bool,
    border: u16,
    areas: Areas,
}

impl Plain {
    fn new() -> Self {
        Plain {
            cur: Frame::new(),
            shown: Frame::new(),
            colours: [0; 256],
            version: 0,
            shown_palette: 0,
            in_step: false,
            border: 0,
            areas: Areas::new(),
        }
    }

    fn present<S: Screen>(&mut self, screen: &mut S) -> SendReport {
        let view = View::fit(S::PANEL_W, S::PANEL_H);
        if !self.in_step || self.version != self.shown_palette {
            self.areas = Areas::full();
        } else {
            diff(&self.shown, &self.cur, &mut self.areas);
        }
        if self.areas.is_empty() {
            return SendReport::default();
        }
        let mut panel = [Rect::EMPTY; MAX_AREAS];
        let n = self.areas.len();
        for (p, a) in panel.iter_mut().zip(self.areas.as_slice()) {
            *p = view.to_panel(*a);
        }
        if !self.in_step {
            panel[0] = Rect::new(0, 0, S::PANEL_W, S::PANEL_H);
        }
        let mut pal = Palette::new();
        pal.set_all(&self.colours);
        let src = Source::new(&self.cur, &pal, view, self.border);
        let report = screen.present(&src, &panel[..n]);
        self.shown.0.copy_from_slice(&self.cur.0);
        self.shown_palette = self.version;
        self.in_step = true;
        report
    }
}

// ---- Random changes, made the same way to both ------------------------

/// One change to the picture, the palette or the border.
#[derive(Clone, Debug)]
enum Op {
    /// Single pixels, often far apart (many small areas).
    Pixels(Vec<(u16, u16, u8)>),
    /// A rectangle, sometimes sticking out of the picture.
    Rect(Rect, u8),
    /// A full redraw.
    Clear(u8),
    /// A "game" writing pixels through `game_buffers` (index, value).
    GamePixels(Vec<(usize, u8)>),
    /// A colour set the normal way (the version follows).
    Colour(u8, u16),
    /// A "game" writing a colour through `game_buffers`; `true`: it then
    /// asks for the palette to count as changed (`touch`).
    GameColour(u8, u16, bool),
    Border(u16),
    Full,
}

fn random_op(rng: &mut Rng) -> Op {
    match rng.below(100) {
        0..=29 => Op::Pixels(
            (0..1 + rng.below(30))
                .map(|_| {
                    (
                        rng.below(W as u64) as u16,
                        rng.below(H as u64) as u16,
                        rng.below(256) as u8,
                    )
                })
                .collect(),
        ),
        30..=59 => Op::Rect(
            Rect::new(
                rng.below(W as u64 + 10) as u16,
                rng.below(H as u64 + 10) as u16,
                1 + rng.below(70) as u16,
                1 + rng.below(50) as u16,
            ),
            rng.below(256) as u8,
        ),
        60..=77 => Op::GamePixels(
            (0..1 + rng.below(60))
                .map(|_| (rng.below((W * H) as u64) as usize, rng.below(256) as u8))
                .collect(),
        ),
        78..=82 => Op::Colour(rng.below(256) as u8, rng.next() as u16),
        83..=88 => Op::GameColour(rng.below(256) as u8, rng.next() as u16, rng.below(2) == 0),
        89..=90 => Op::Border(rng.next() as u16),
        91..=92 => Op::Full,
        93..=95 => Op::Clear(rng.below(256) as u8),
        // Nothing this time.
        _ => Op::Pixels(Vec::new()),
    }
}

fn apply_video(op: &Op, v: &mut Video) {
    match op {
        Op::Pixels(px) => {
            for &(x, y, c) in px {
                v.frame_mut().set(x, y, c);
            }
        }
        Op::Rect(r, c) => v.frame_mut().fill_rect(*r, *c),
        Op::Clear(c) => v.frame_mut().clear(*c),
        Op::GamePixels(px) => {
            let (pixels, _) = v.game_buffers();
            for &(i, c) in px {
                pixels[i] = c;
            }
        }
        Op::Colour(i, c) => v.palette_mut().set(*i, *c),
        Op::GameColour(i, c, touch) => {
            let (_, colours) = v.game_buffers();
            colours[usize::from(*i)] = *c;
            if *touch {
                v.palette_mut().touch();
            }
        }
        Op::Border(c) => v.set_border(*c),
        Op::Full => v.force_full(),
    }
}

fn apply_plain(op: &Op, p: &mut Plain) {
    match op {
        Op::Pixels(px) => {
            for &(x, y, c) in px {
                p.cur.set(x, y, c);
            }
        }
        Op::Rect(r, c) => p.cur.fill_rect(*r, *c),
        Op::Clear(c) => p.cur.clear(*c),
        Op::GamePixels(px) => {
            for &(i, c) in px {
                p.cur.0[i] = c;
            }
        }
        Op::Colour(i, c) => {
            let i = usize::from(*i);
            if p.colours[i] != *c {
                p.colours[i] = *c;
                p.version = p.version.wrapping_add(1);
            }
        }
        Op::GameColour(i, c, touch) => {
            p.colours[usize::from(*i)] = *c;
            if *touch {
                p.version = p.version.wrapping_add(1);
            }
        }
        Op::Border(c) => {
            if p.border != *c {
                p.border = *c;
                p.in_step = false;
            }
        }
        Op::Full => p.in_step = false,
    }
}

/// An overlay drawn just before a present and taken off right after
/// (like the console's notifications and pause menu over a game): the
/// rectangle's old pixels, kept to be put back.
struct Cover {
    r: Rect,
    under: Vec<u8>,
}

fn cover(f: &mut Frame, r: Rect, c: u8) -> Cover {
    let r = r.intersect(&Rect::new(0, 0, W as u16, H as u16));
    let mut under = Vec::new();
    for y in r.y..r.bottom() {
        let at = usize::from(y) * W + usize::from(r.x);
        under.extend_from_slice(&f.0[at..at + usize::from(r.w)]);
    }
    f.fill_rect(r, c);
    Cover { r, under }
}

fn put_back(f: &mut Frame, c: &Cover) {
    let w = usize::from(c.r.w);
    for (i, y) in (c.r.y..c.r.bottom()).enumerate() {
        let at = usize::from(y) * W + usize::from(c.r.x);
        f.0[at..at + w].copy_from_slice(&c.under[i * w..(i + 1) * w]);
    }
}

fn random_cover(rng: &mut Rng) -> Option<(Rect, u8)> {
    (rng.below(4) == 0).then(|| {
        (
            Rect::new(
                rng.below(W as u64) as u16,
                rng.below(H as u64) as u16,
                1 + rng.below(80) as u16,
                1 + rng.below(36) as u16,
            ),
            rng.below(256) as u8,
        )
    })
}

fn screen(walk: Walk) -> Box<FakeScreen> {
    let mut s = Box::new(FakeScreen::new());
    s.walk = walk;
    s
}

const WALKS: [Walk; 3] = [Walk::Rows, Walk::Columns, Walk::Indexed];

/// Runs in both builds. Without `swap`: `Plain` is the old renderer.
/// With it: the swap renderer (through `present`, which hands over to
/// the screen at once) sends exactly what the plain rule sends.
#[test]
fn swap_sends_what_the_plain_rule_sends() {
    for (k, walk) in WALKS.into_iter().enumerate() {
        let mut rng = Rng(0x5EED_0000 + k as u64);
        let mut video = Box::new(Video::new());
        let mut plain = Box::new(Plain::new());
        let (mut s_video, mut s_plain) = (screen(walk), screen(walk));
        for frame in 0..1500 {
            for _ in 0..rng.below(5) {
                let op = random_op(&mut rng);
                apply_video(&op, &mut video);
                apply_plain(&op, &mut plain);
            }
            let covered = random_cover(&mut rng).map(|(r, c)| {
                let a = cover(video.frame_mut(), r, c);
                let b = cover(&mut plain.cur, r, c);
                (a, b)
            });
            let rv = video.present(&mut *s_video);
            let rp = plain.present(&mut *s_plain);
            if let Some((a, b)) = covered {
                put_back(video.frame_mut(), &a);
                put_back(&mut plain.cur, &b);
            }
            let at = format!("walk {walk:?}, frame {frame}");
            assert_eq!(rv, rp, "{at}: report");
            assert_eq!(
                video.last_areas().as_slice(),
                plain.areas.as_slice(),
                "{at}: areas"
            );
            assert_eq!(s_video.presents, s_plain.presents, "{at}: presents");
            assert_eq!(
                &s_video.last_areas[..s_video.last_area_count.min(32)],
                &s_plain.last_areas[..s_plain.last_area_count.min(32)],
                "{at}: panel areas"
            );
            assert!(s_video.image == s_plain.image, "{at}: panel pixels differ");
            assert!(video.frame() == &plain.cur, "{at}: picture differs");
            assert_eq!(video.palette().colours(), &plain.colours, "{at}: palette");
        }
    }
}

/// `Video` is the same size with and without `swap` (the extra flag
/// fits in padding): 39,064 bytes on a 64-bit PC, 39,056 on the ESP32.
#[test]
fn video_keeps_its_size() {
    let expected = if size_of::<usize>() == 8 {
        39_064
    } else {
        39_056
    };
    assert_eq!(size_of::<Video>(), expected);
}

// ---- The slow sender (feature `swap`) ----------------------------------

#[cfg(feature = "swap")]
mod slow {
    use super::*;
    use console_video::{Handed, Sender};

    /// What a sender keeps of one hand-over: the picture's address (it
    /// reads the picture there, later), a copy of it taken at hand-over
    /// time (to check every byte it reads), and copies of the small parts
    /// it may use only during the call (palette, areas, view, border).
    struct Held {
        at: *const Frame,
        snap: Box<Frame>,
        palette: Palette,
        view: View,
        border: u16,
        areas: Vec<Rect>,
        /// Areas sent so far, and bytes checked so far (in order).
        sent: usize,
        checked: usize,
    }

    /// A sender like a second core: it reads and sends the picture it
    /// holds in pieces ([`Slow::pump`]) while the console goes on, and
    /// finishes it in `wait_free`.
    pub struct Slow {
        held: Option<Held>,
        pub screen: Box<FakeScreen>,
        pub hand_overs: u32,
        pub waits: u32,
        pub bytes_read: u64,
    }

    impl Slow {
        pub fn new(walk: Walk) -> Self {
            Slow {
                held: None,
                screen: screen(walk),
                hand_overs: 0,
                waits: 0,
                bytes_read: 0,
            }
        }

        /// The copy of the held picture taken at hand-over time.
        pub fn held_copy(&self) -> Option<&Frame> {
            self.held.as_ref().map(|h| &*h.snap)
        }

        /// The address of the picture held, if any.
        pub fn held_at(&self) -> Option<*const u8> {
            self.held.as_ref().map(|h| h.at.cast())
        }

        /// Reads `n` more bytes of the held picture (wrapping round) and
        /// checks them against the copy taken at hand-over time.
        fn check(h: &mut Held, n: usize, read: &mut u64) {
            // SAFETY (test model of the board's second core): the picture
            // lives in the test's boxed `Video`, which is never moved or
            // dropped while held; the rule under test says the console
            // does not write it until `wait_free` returned. Only read.
            let now: &Frame = unsafe { &*h.at };
            for _ in 0..n {
                let i = h.checked % (W * H);
                assert_eq!(
                    now.0[i], h.snap.0[i],
                    "byte {i} of the picture held by the sender changed after hand-over"
                );
                h.checked += 1;
            }
            *read += n as u64;
        }

        /// Sends the next held area, straight from the held picture.
        fn send_one(h: &mut Held, screen: &mut FakeScreen) {
            if h.sent < h.areas.len() {
                // SAFETY: as in `check`.
                let frame: &Frame = unsafe { &*h.at };
                let src = Source::new(frame, &h.palette, h.view, h.border);
                screen.present(&src, &h.areas[h.sent..h.sent + 1]);
                h.sent += 1;
            }
        }

        /// Some work of the "second core" while the console goes on:
        /// check some bytes, maybe send one area.
        pub fn pump(&mut self, rng: &mut Rng) {
            if let Some(h) = self.held.as_mut() {
                Self::check(h, rng.below(4000) as usize, &mut self.bytes_read);
                if rng.below(3) == 0 {
                    Self::send_one(h, &mut self.screen);
                }
            }
        }
    }

    impl Sender for Slow {
        fn wait_free(&mut self) {
            self.waits += 1;
            if let Some(mut h) = self.held.take() {
                // The rest of the areas, then every byte once more: the
                // whole picture must still be as handed over.
                while h.sent < h.areas.len() {
                    Self::send_one(&mut h, &mut self.screen);
                }
                h.checked = 0;
                Self::check(&mut h, W * H, &mut self.bytes_read);
            }
        }

        fn hand_over(&mut self, h: &Handed<'_>) -> SendReport {
            assert!(
                self.held.is_none(),
                "a picture was handed over while the sender still held one"
            );
            self.hand_overs += 1;
            self.held = Some(Held {
                at: h.frame,
                snap: Box::new(h.frame.clone()),
                palette: h.palette.clone(),
                view: h.view,
                border: h.border,
                areas: h.areas.to_vec(),
                sent: 0,
                checked: 0,
            });
            SendReport {
                areas: h.areas.len() as u16,
                pixels: h.areas.iter().map(|a| a.area()).sum(),
                send_us: 0,
                wait_us: 0,
            }
        }
    }
}

#[cfg(feature = "swap")]
use slow::Slow;

/// The picture the console draws into now (the address a game gets).
#[cfg(feature = "swap")]
fn drawn_at(v: &mut Video) -> *const u8 {
    v.game_buffers().0.as_ptr()
}

/// The core test of the rule: a sender that reads late and in pieces
/// only ever sees the picture as it was handed over, the console never
/// draws into the picture held, and the panel ends up the same as with
/// the plain rule.
#[cfg(feature = "swap")]
#[test]
fn a_slow_sender_only_ever_reads_the_picture_as_handed_over() {
    use console_video::Sender;
    for (k, walk) in WALKS.into_iter().enumerate() {
        let mut rng = Rng(0xC0DE_0000 + k as u64);
        let mut video = Box::new(Video::new());
        let mut plain = Box::new(Plain::new());
        let mut s_plain = screen(walk);
        let mut slow = Slow::new(walk);
        let mut flushes = 0;
        for frame in 0..1500 {
            let at = format!("walk {walk:?}, frame {frame}");
            // Drawing, with the "second core" working in between.
            for _ in 0..rng.below(6) {
                slow.pump(&mut rng);
                let op = random_op(&mut rng);
                assert_ne!(
                    Some(drawn_at(&mut video)),
                    slow.held_at(),
                    "{at}: the console draws into the picture the sender holds"
                );
                apply_video(&op, &mut video);
                apply_plain(&op, &mut plain);
            }
            slow.pump(&mut rng);
            let covered = random_cover(&mut rng).map(|(r, c)| {
                let a = cover(video.frame_mut(), r, c);
                let b = cover(&mut plain.cur, r, c);
                (a, b)
            });
            let rv = video.present_handing::<FakeScreen, _>(&mut slow);
            let rp = plain.present(&mut *s_plain);
            assert_eq!((rv.areas, rv.pixels), (rp.areas, rp.pixels), "{at}: report");
            assert_eq!(
                video.last_areas().as_slice(),
                plain.areas.as_slice(),
                "{at}: areas"
            );
            slow.pump(&mut rng);
            assert_ne!(
                Some(drawn_at(&mut video)),
                slow.held_at(),
                "{at}: after present, the console draws into the picture handed over"
            );
            if let Some((a, b)) = covered {
                put_back(video.frame_mut(), &a);
                put_back(&mut plain.cur, &b);
            }
            assert!(video.frame() == &plain.cur, "{at}: picture differs");
            // Now and then the board waits for the sender (as before the
            // backlight): the panel must then show what the plain rule
            // shows.
            if rng.below(5) == 0 {
                slow.wait_free();
                flushes += 1;
                assert!(slow.screen.image == s_plain.image, "{at}: panel differs");
            }
        }
        slow.wait_free();
        assert!(
            slow.screen.image == s_plain.image,
            "walk {walk:?}: panel differs at the end"
        );
        println!(
            "walk {walk:?}: {} hand-overs, {} waits ({flushes} by the test), {} bytes read and checked",
            slow.hand_overs, slow.waits, slow.bytes_read
        );
        assert!(slow.hand_overs > 1000 && slow.bytes_read > 10_000_000);
    }
}

/// Nothing changed: no wait, no hand-over, no swap (the drawn picture
/// keeps its address). A change: one wait, then one hand-over, and the
/// console goes on in the other picture.
#[cfg(feature = "swap")]
#[test]
fn a_present_waits_then_hands_over_and_swaps_only_when_something_changed() {
    let mut video = Box::new(Video::new());
    let mut slow = Slow::new(Walk::Rows);
    let first = drawn_at(&mut video);
    video.present_handing::<FakeScreen, _>(&mut slow);
    assert_eq!(
        (slow.waits, slow.hand_overs),
        (1, 1),
        "the first present sends all"
    );
    assert_eq!(
        slow.held_at(),
        Some(first),
        "the drawn picture was handed over"
    );
    let second = drawn_at(&mut video);
    assert_ne!(first, second, "the console goes on in the other picture");
    // Nothing changed.
    let r = video.present_handing::<FakeScreen, _>(&mut slow);
    assert_eq!(r, SendReport::default());
    assert_eq!((slow.waits, slow.hand_overs), (1, 1));
    assert_eq!(drawn_at(&mut video), second);
    // One pixel.
    video.frame_mut().set(3, 4, 9);
    video.present_handing::<FakeScreen, _>(&mut slow);
    assert_eq!((slow.waits, slow.hand_overs), (2, 2));
    assert_eq!(slow.held_at(), Some(second));
    assert_eq!(drawn_at(&mut video), first);
    // The caught-up picture holds the pixel too.
    assert_eq!(video.frame().get(3, 4), 9);
}

/// After each present, the picture the console goes on drawing in is
/// equal to the one just handed over (the catch-up copy brought it up to
/// date), and the console only ever uses two pictures.
#[cfg(feature = "swap")]
#[test]
fn after_each_present_the_drawn_picture_equals_the_one_handed_over() {
    let mut rng = Rng(77);
    let mut video = Box::new(Video::new());
    let mut slow = Slow::new(Walk::Rows);
    let mut seen = std::collections::HashSet::new();
    let mut checked = 0;
    for _ in 0..400 {
        for _ in 0..rng.below(4) {
            apply_video(&random_op(&mut rng), &mut video);
        }
        video.present_handing::<FakeScreen, _>(&mut slow);
        if let Some(snap) = slow.held_copy() {
            assert!(video.frame() == snap, "the drawn picture was not caught up");
            checked += 1;
        }
        seen.insert(drawn_at(&mut video));
    }
    assert!(checked > 300);
    assert_eq!(seen.len(), 2, "the console draws into exactly two pictures");
}
