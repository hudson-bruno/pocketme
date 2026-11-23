//! The console menu, in two kinds:
//!
//! - Over a game ([`Kind::Game`]): Home (or the board's BOOT button)
//!   during a game opens it. RESUME, BRIGHTNESS, BACK TO HOME. The game
//!   is not called while it shows.
//! - On the home screen ([`Kind::Home`]): the pad's Home button, or a tap
//!   on the gear at the right end of the bar, opens it. BRIGHTNESS,
//!   CLOSE; with feature `boot-gather` also the boot animation (BOOT
//!   ANIM: FULL, SHORT or OFF) between them.
//!
//! Up and down choose, the confirm button (or BOOT) takes the choice, a
//! touch takes the item touched. Back or Home close the menu, as RESUME
//! and CLOSE do.
//!
//! The BRIGHTNESS item is a sun and five bars, one per step (the bars up
//! to the step in use are filled). While it is chosen, left and right
//! change the step by one. Confirm (or BOOT) on it goes one step up, and
//! from the brightest back to the dimmest, so BOOT alone can reach every
//! step. A touch on a bar sets that step; a finger that went down on the
//! item and slides sets the step under it. The menu itself does not
//! light the screen: it changes the step, and the console tells the
//! board (see `Console::brightness`).
//!
//! The BOOT ANIM item (feature `boot-gather`) shows its value. Left and
//! right go to the value before or after; confirm (or BOOT), or a touch
//! on it, goes to the next one, and from OFF back to FULL. The value is
//! for the next start (see [`update_boot`]).
//!
//! The touch acts when the finger goes DOWN (`Input::down`), as before
//! step 4. The menu has nothing to drag except the bars, so it does not
//! wait for the finger to lift (the home row does, see `input`).

use console_video::Frame;

#[cfg(feature = "boot-gather")]
use crate::boot::Anim;
use crate::gfx::{self, CX, SUN_ICON};
use crate::input::{Input, act};
use crate::theme::{
    BRIGHT_STEPS, GOLD, INK, LINE, MENU_BRIGHT_H, MENU_H, MENU_ITEM_H, MENU_PAD, MENU_W, MENU_X,
    MENU_Y, MUTE, PANEL, PAPER,
};

/// Which menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Over a running game.
    Game,
    /// On the home screen.
    Home,
}

/// One line of a menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    /// Back to the game.
    Resume,
    /// The brightness bars.
    Bright,
    /// End the game and go home.
    Quit,
    /// Close the home screen's menu.
    Close,
    /// The boot animation's setting (feature `boot-gather`).
    #[cfg(feature = "boot-gather")]
    Boot,
}

/// The items over a game, top to bottom.
pub const GAME_ITEMS: [Item; 3] = [Item::Resume, Item::Bright, Item::Quit];
/// The items on the home screen, top to bottom.
#[cfg(not(feature = "boot-gather"))]
pub const HOME_ITEMS: [Item; 2] = [Item::Bright, Item::Close];
/// The items on the home screen, top to bottom.
#[cfg(feature = "boot-gather")]
pub const HOME_ITEMS: [Item; 3] = [Item::Bright, Item::Boot, Item::Close];

impl Kind {
    pub const fn items(self) -> &'static [Item] {
        match self {
            Kind::Game => &GAME_ITEMS,
            Kind::Home => &HOME_ITEMS,
        }
    }

    /// Where an item is in this menu, if it has it.
    pub fn index_of(self, item: Item) -> Option<usize> {
        self.items().iter().position(|&i| i == item)
    }
}

impl Item {
    /// Its words (the BRIGHTNESS item shows a sun instead).
    pub const fn label(self) -> &'static str {
        match self {
            Item::Resume => "RESUME",
            Item::Bright => "BRIGHTNESS",
            Item::Quit => "BACK TO HOME",
            Item::Close => "CLOSE",
            #[cfg(feature = "boot-gather")]
            Item::Boot => "BOOT ANIM",
        }
    }

    /// Its height in picture pixels.
    pub const fn h(self) -> u16 {
        match self {
            Item::Bright => MENU_BRIGHT_H,
            _ => MENU_ITEM_H,
        }
    }
}

/// What the player chose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// Nothing yet: the menu stays.
    Stay,
    /// Close the menu (RESUME over a game, CLOSE at home).
    Resume,
    /// End the game and go home.
    Home,
}

/// The menu box's height: its items and a margin above and below.
pub const fn box_h(kind: Kind) -> u16 {
    let items = kind.items();
    let mut h = 2 * MENU_PAD;
    let mut i = 0;
    while i < items.len() {
        h += items[i].h();
        i += 1;
    }
    h
}

/// The menu box's top edge (the box is centred).
pub const fn box_y(kind: Kind) -> i16 {
    (120 - box_h(kind) as i16) / 2
}

/// The top edge of item `i`.
pub const fn item_y(kind: Kind, i: usize) -> i16 {
    let items = kind.items();
    let mut y = box_y(kind) + MENU_PAD as i16;
    let mut k = 0;
    while k < i && k < items.len() {
        y += items[k].h() as i16;
        k += 1;
    }
    y
}

// The game's menu box is exactly the area whose picture `game` keeps.
const _: () = {
    assert!(box_h(Kind::Game) == MENU_H);
    assert!(box_y(Kind::Game) == MENU_Y);
};

/// The item under a picture point, if any (the item's whole width inside
/// the box).
pub fn item_at(kind: Kind, x: u16, y: u16) -> Option<usize> {
    let (x, y) = (x as i16, y as i16);
    if !(MENU_X..MENU_X + MENU_W as i16).contains(&x) {
        return None;
    }
    (0..kind.items().len()).find(|&i| {
        let top = item_y(kind, i);
        (top..top + kind.items()[i].h() as i16).contains(&y)
    })
}

/// The brightness bars: width, gap, and the left edge of the first.
pub const BAR_W: u16 = 9;
pub const BAR_GAP: u16 = 3;
pub const BARS_X: i16 = MENU_X + 17;

/// The left edge of bar `i`.
pub const fn bar_x(i: u8) -> i16 {
    BARS_X + i as i16 * (BAR_W + BAR_GAP) as i16
}

/// The height of bar `i`: they grow, 3 pixels to 7.
pub const fn bar_h(i: u8) -> u16 {
    3 + i as u16
}

/// The step whose bar is under picture column `x` (with half the gap on
/// each side). `None` left of the bars (on the sun).
pub fn step_at(x: u16) -> Option<u8> {
    let left = BARS_X - (BAR_GAP / 2) as i16;
    let x = x as i16;
    if x < left {
        return None;
    }
    let i = (x - left) / (BAR_W + BAR_GAP) as i16;
    Some((i as u8).min(BRIGHT_STEPS - 1))
}

/// One frame of the menu. `sel` is the item chosen so far; `step` the
/// brightness step (0 dimmest .. `BRIGHT_STEPS - 1`), changed here when
/// the player changes it.
pub fn update(kind: Kind, sel: &mut u8, step: &mut u8, input: &Input) -> Choice {
    let items = kind.items();
    let last = items.len() as u8 - 1;
    let top = BRIGHT_STEPS - 1;
    if input.acts & (act::BACK | act::HOME) != 0 {
        return Choice::Resume;
    }
    if input.acts & act::UP != 0 {
        *sel = sel.saturating_sub(1);
    }
    if input.acts & act::DOWN != 0 {
        *sel = (*sel + 1).min(last);
    }
    *sel = (*sel).min(last);
    let on_bright = |s: u8| items[usize::from(s)] == Item::Bright;
    if on_bright(*sel) {
        if input.acts & act::LEFT != 0 {
            *step = step.saturating_sub(1);
        }
        if input.acts & act::RIGHT != 0 {
            *step = (*step + 1).min(top);
        }
    }
    let mut take = input.acts & act::OK != 0;
    if let Some((x, y)) = input.down
        && let Some(i) = item_at(kind, x, y)
    {
        *sel = i as u8;
        if on_bright(*sel) {
            // A bar sets its step. The sun only chooses the line.
            if let Some(s) = step_at(x) {
                *step = s;
            }
            take = false;
        } else {
            take = true;
        }
    }
    // A finger that went down on the bars and slides: the step follows it.
    if let (Some(d), Some((x, _))) = (input.drag, input.touch)
        && let Some(i) = item_at(kind, d.from.0, d.from.1)
        && on_bright(i as u8)
        && let Some(s) = step_at(x)
    {
        *step = s;
    }
    *step = (*step).min(top);
    if !take {
        return Choice::Stay;
    }
    match items[usize::from(*sel)] {
        Item::Resume | Item::Close => Choice::Resume,
        Item::Quit => Choice::Home,
        // Its value changes in `update_boot`.
        #[cfg(feature = "boot-gather")]
        Item::Boot => Choice::Stay,
        Item::Bright => {
            // Confirm (or BOOT) on the bars: one step up, then round.
            *step = if *step >= top { 0 } else { *step + 1 };
            Choice::Stay
        }
    }
}

/// The BOOT ANIM item's change, after [`update`] for the same input
/// (feature `boot-gather`; `sel` is the item chosen after it). Returns
/// the new value: left and right go to the one before or after (no
/// wrap), confirm (or BOOT) or a touch on the item to the next one
/// (round). Other items: `anim` as it is.
#[cfg(feature = "boot-gather")]
pub fn update_boot(kind: Kind, sel: u8, anim: Anim, input: &Input) -> Anim {
    let items = kind.items();
    if items.get(usize::from(sel)) != Some(&Item::Boot) {
        return anim;
    }
    if input.acts & (act::BACK | act::HOME) != 0 {
        return anim;
    }
    let touched = input
        .down
        .and_then(|(x, y)| item_at(kind, x, y))
        .is_some_and(|i| items[i] == Item::Boot);
    if input.acts & act::OK != 0 || touched {
        anim.next()
    } else if input.acts & act::LEFT != 0 {
        anim.prev()
    } else if input.acts & act::RIGHT != 0 {
        anim.later()
    } else {
        anim
    }
}

/// The BOOT ANIM item's words for a value: "BOOT ANIM: FULL" and so on.
#[cfg(feature = "boot-gather")]
pub const fn boot_text(anim: Anim) -> &'static str {
    match anim {
        Anim::Full => "BOOT ANIM: FULL",
        Anim::Short => "BOOT ANIM: SHORT",
        Anim::Off => "BOOT ANIM: OFF",
    }
}

/// Draws the menu (only inside its box: `MENU_X`, [`box_y`], `MENU_W`,
/// [`box_h`]). `step` is the brightness step in use.
pub fn draw(f: &mut Frame, kind: Kind, sel: u8, step: u8) {
    draw_items(f, kind, sel, step, "");
}

/// The home screen's menu with the BOOT ANIM item showing `anim`
/// (feature `boot-gather`).
#[cfg(feature = "boot-gather")]
pub fn draw_home(f: &mut Frame, sel: u8, step: u8, anim: Anim) {
    draw_items(f, Kind::Home, sel, step, boot_text(anim));
}

/// [`draw`], with `_boot` the BOOT ANIM item's words.
fn draw_items(f: &mut Frame, kind: Kind, sel: u8, step: u8, _boot: &str) {
    let (y0, h) = (box_y(kind), box_h(kind));
    f.rect(MENU_X, y0, MENU_W, h, INK);
    f.rect(MENU_X + 1, y0 + 1, MENU_W - 2, h - 2, PANEL);
    for (i, &item) in kind.items().iter().enumerate() {
        let y = item_y(kind, i);
        let on = usize::from(sel) == i;
        if on {
            f.rect(MENU_X + 3, y, MENU_W - 6, item.h(), GOLD);
        }
        if item == Item::Bright {
            let (sun, full, empty) = if on {
                (INK, INK, INK)
            } else {
                (MUTE, PAPER, LINE)
            };
            SUN_ICON.draw(f, MENU_X + 6, y + 2, 1, SUN_ICON.h, sun);
            for b in 0..BRIGHT_STEPS {
                let (x, bh) = (bar_x(b), bar_h(b));
                let top = y + 10 - bh as i16;
                if b <= step {
                    f.rect(x, top, BAR_W, bh, full);
                } else {
                    f.outline(x, top, BAR_W, bh, empty);
                }
            }
        } else {
            let c = if on { INK } else { PAPER };
            let text = match item {
                #[cfg(feature = "boot-gather")]
                Item::Boot => _boot,
                _ => item.label(),
            };
            gfx::text_centred(f, CX, y + 2, text, c, 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::Drag;

    fn acts(a: u8) -> Input {
        Input {
            acts: a,
            ..Input::default()
        }
    }

    fn down(x: u16, y: i16) -> Input {
        Input {
            down: Some((x, y as u16)),
            ..Input::default()
        }
    }

    #[test]
    fn the_game_menu_moves_and_chooses() {
        let (mut sel, mut step) = (0, 4);
        let g = Kind::Game;
        assert_eq!(update(g, &mut sel, &mut step, &acts(0)), Choice::Stay);
        assert_eq!(
            update(g, &mut sel, &mut step, &acts(act::DOWN)),
            Choice::Stay
        );
        assert_eq!(sel, 1);
        assert_eq!(
            update(g, &mut sel, &mut step, &acts(act::DOWN)),
            Choice::Stay
        );
        assert_eq!(
            update(g, &mut sel, &mut step, &acts(act::DOWN)),
            Choice::Stay
        );
        assert_eq!(sel, 2, "stops at the last item");
        assert_eq!(update(g, &mut sel, &mut step, &acts(act::OK)), Choice::Home);
        sel = 1;
        assert_eq!(
            update(g, &mut sel, &mut step, &acts(act::UP | act::OK)),
            Choice::Resume
        );
        assert_eq!(
            update(g, &mut sel, &mut step, &acts(act::BACK)),
            Choice::Resume
        );
        sel = 2;
        assert_eq!(
            update(g, &mut sel, &mut step, &acts(act::HOME)),
            Choice::Resume
        );
        assert_eq!(step, 4, "nothing here changed the brightness");
    }

    #[test]
    fn left_and_right_change_the_step_only_on_the_bars() {
        let (mut sel, mut step) = (0, 4);
        let g = Kind::Game;
        update(g, &mut sel, &mut step, &acts(act::LEFT));
        assert_eq!(step, 4, "RESUME is chosen: no change");
        sel = 1;
        update(g, &mut sel, &mut step, &acts(act::LEFT));
        assert_eq!(step, 3);
        for _ in 0..9 {
            update(g, &mut sel, &mut step, &acts(act::LEFT));
        }
        assert_eq!(step, 0, "never below the dimmest");
        update(g, &mut sel, &mut step, &acts(act::RIGHT));
        assert_eq!(step, 1);
        for _ in 0..9 {
            update(g, &mut sel, &mut step, &acts(act::RIGHT));
        }
        assert_eq!(step, 4, "never above full");
    }

    #[test]
    fn confirm_on_the_bars_goes_up_and_round() {
        let (mut sel, mut step) = (1, 2);
        let g = Kind::Game;
        let mut seen = [0; 6];
        for s in &mut seen {
            assert_eq!(update(g, &mut sel, &mut step, &acts(act::OK)), Choice::Stay);
            *s = step;
        }
        assert_eq!(seen, [3, 4, 0, 1, 2, 3]);
    }

    #[test]
    fn a_touch_takes_the_item_under_it() {
        let (mut sel, mut step) = (0, 4);
        let g = Kind::Game;
        let t = |i: usize| down(80, item_y(g, i) + 2);
        assert_eq!(update(g, &mut sel, &mut step, &t(2)), Choice::Home);
        assert_eq!(update(g, &mut sel, &mut step, &t(0)), Choice::Resume);
        let above = down(80, MENU_Y + 1);
        assert_eq!(update(g, &mut sel, &mut step, &above), Choice::Stay);
        let h = Kind::Home;
        sel = 0;
        let close = h.index_of(Item::Close).unwrap();
        assert_eq!(
            update(h, &mut sel, &mut step, &down(80, item_y(h, close) + 2)),
            Choice::Resume
        );
    }

    #[test]
    fn a_touch_on_a_bar_sets_its_step() {
        let g = Kind::Game;
        let y = item_y(g, 1) + 5;
        for b in 0..BRIGHT_STEPS {
            for x in [bar_x(b), bar_x(b) + BAR_W as i16 - 1] {
                let (mut sel, mut step) = (0, 4 - b.min(1));
                let c = update(g, &mut sel, &mut step, &down(x as u16, y));
                assert_eq!((c, sel, step), (Choice::Stay, 1, b), "bar {b} at x {x}");
            }
        }
        // The sun only chooses the line.
        let (mut sel, mut step) = (0, 2);
        update(g, &mut sel, &mut step, &down((MENU_X + 8) as u16, y));
        assert_eq!((sel, step), (1, 2));
    }

    #[test]
    fn a_finger_sliding_on_the_bars_sets_the_step_under_it() {
        let g = Kind::Game;
        let y = (item_y(g, 1) + 5) as u16;
        let (mut sel, mut step) = (1, 0);
        let slide = Input {
            touch: Some((bar_x(3) as u16 + 2, y + 30)),
            drag: Some(Drag {
                from: (bar_x(0) as u16, y),
                dx: 40,
                dy: 0,
            }),
            ..Input::default()
        };
        assert_eq!(update(g, &mut sel, &mut step, &slide), Choice::Stay);
        assert_eq!(step, 3);
        // A drag that started elsewhere does nothing.
        let elsewhere = Input {
            drag: Some(Drag {
                from: (80, 10),
                ..Drag::default()
            }),
            touch: Some((bar_x(1) as u16, y)),
            ..Input::default()
        };
        update(g, &mut sel, &mut step, &elsewhere);
        assert_eq!(step, 3);
    }

    #[test]
    fn the_items_fit_the_box() {
        for kind in [Kind::Game, Kind::Home] {
            let n = kind.items().len();
            let bottom = item_y(kind, n - 1) + kind.items()[n - 1].h() as i16;
            assert_eq!(bottom, box_y(kind) + box_h(kind) as i16 - MENU_PAD as i16);
            for t in kind.items() {
                assert!(gfx::width(t.label(), 1) <= MENU_W as i16 - 8);
            }
            #[cfg(feature = "boot-gather")]
            for a in Anim::ALL {
                assert!(gfx::width(boot_text(a), 1) <= MENU_W as i16 - 8);
            }
        }
        // The last bar ends inside the gold line.
        let right = bar_x(BRIGHT_STEPS - 1) + BAR_W as i16;
        assert!(right <= MENU_X + MENU_W as i16 - 5);
        assert!(bar_x(0) > MENU_X + 6 + SUN_ICON.w as i16);
    }

    #[cfg(feature = "boot-gather")]
    #[test]
    fn the_boot_item_changes_its_value_only_when_chosen() {
        let h = Kind::Home;
        let boot = h.index_of(Item::Boot).unwrap() as u8;
        let bright = h.index_of(Item::Bright).unwrap() as u8;
        assert_eq!(boot, bright + 1, "under the brightness");
        assert_eq!(
            h.index_of(Item::Boot),
            h.index_of(Item::Close).map(|c| c - 1)
        );
        assert_eq!(Kind::Game.index_of(Item::Boot), None, "not over a game");
        // Not chosen: nothing changes.
        let ok = acts(act::OK);
        assert_eq!(update_boot(h, bright, Anim::Full, &ok), Anim::Full);
        // Confirm goes round; left and right stop at the ends.
        let mut a = Anim::Full;
        let mut seen = [Anim::Full; 4];
        for s in &mut seen {
            a = update_boot(h, boot, a, &ok);
            *s = a;
        }
        assert_eq!(seen, [Anim::Short, Anim::Off, Anim::Full, Anim::Short]);
        assert_eq!(
            update_boot(h, boot, Anim::Full, &acts(act::LEFT)),
            Anim::Full
        );
        assert_eq!(
            update_boot(h, boot, Anim::Full, &acts(act::RIGHT)),
            Anim::Short
        );
        assert_eq!(
            update_boot(h, boot, Anim::Off, &acts(act::RIGHT)),
            Anim::Off
        );
        assert_eq!(
            update_boot(h, boot, Anim::Off, &acts(act::LEFT)),
            Anim::Short
        );
        // A touch on the item: the next value, and the menu stays.
        let (mut sel, mut step) = (0, 4);
        let t = down(80, item_y(h, usize::from(boot)) + 2);
        assert_eq!(update(h, &mut sel, &mut step, &t), Choice::Stay);
        assert_eq!(sel, boot);
        assert_eq!(update_boot(h, sel, Anim::Short, &t), Anim::Off);
        assert_eq!(step, 4, "the brightness is not touched");
        // Back closes the menu without a change.
        assert_eq!(
            update_boot(h, boot, Anim::Short, &acts(act::BACK)),
            Anim::Short
        );
    }
}
