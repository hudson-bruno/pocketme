//! The game's own menus (AREAS-SPEC.md, section 1).
//!
//! The pause menu ([`Menu`]) on START: the detail setting, RESTART and
//! AREAS.
//! * START opens it (play stops; the console's own menu stays on Home).
//! * UP and DOWN choose a line (the d-pad, or the left stick pushed past
//!   half way, one move per push).
//! * A or START takes the line: LOW, MID, HIGH or MAX keeps that setting and
//!   closes, RESTART starts the area again, AREAS opens the area menu. B
//!   closes and keeps the old setting.
//!
//! The area menu ([`AreaMenu`]): open at the start, and from AREAS.
//! * UP and DOWN as above; A or START starts the chosen area.
//! * B goes back to play, but only when opened from AREAS (at the start
//!   there is nothing to go back to).
//! * Touch: a tap on a line chooses it (on release); a tap outside the box
//!   goes back (when B would).
//!
//! Saving the setting waits for the console's save service (the spec,
//! section 5): it is lost when the game ends.

use game_kit::button;

use crate::area::Area;
use crate::play::Detail;

/// Left stick past this (of 32,767) counts as a push in the menu.
const MENU_STICK: i16 = 16_384;

/// The pause menu's lines: the four settings, then these two.
pub const RESTART: u8 = 4;
pub const AREAS: u8 = 5;
/// Its line names, in order.
pub const MENU_LINES: [&str; 6] = ["LOW", "MID", "HIGH", "MAX", "RESTART", "AREAS"];
/// The pause menu's box (x, y, width, height) and its first line's top;
/// lines are 10 pixels apart (six lines end at row 102, the box at 106).
pub const MENU_BOX: (i16, i16, u16, u16) = (40, 28, 80, 80);
pub const MENU_LINE0: i16 = 44;

/// The area menu's box (x, y, width, height) and its first line's top;
/// lines are 10 pixels apart.
pub const AREA_BOX: (i16, i16, u16, u16) = (40, 30, 80, 62);
pub const AREA_LINE0: i16 = 44;

/// Where the left stick is for the menus: -1 pushed up, 1 down, 0 not
/// past half way.
fn stick_dir(stick_y: i16) -> i8 {
    if stick_y < -MENU_STICK {
        -1
    } else if stick_y > MENU_STICK {
        1
    } else {
        0
    }
}

/// One move of the choice from this call's presses and the stick (an
/// edge: one move per push).
fn moved(pressed: u32, stick_y: i16, stick_was: &mut i8) -> i8 {
    let stick = stick_dir(stick_y);
    let push = if stick != *stick_was { stick } else { 0 };
    *stick_was = stick;
    if pressed & button::UP != 0 || push < 0 {
        -1
    } else if pressed & button::DOWN != 0 || push > 0 {
        1
    } else {
        0
    }
}

/// The pause menu's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Menu {
    pub open: bool,
    /// The highlighted line (0 LOW, 1 MID, 2 HIGH, 3 MAX, 4 RESTART,
    /// 5 AREAS).
    pub sel: u8,
    /// The stick was pushed up or down at the last call (-1, 0, 1).
    stick_was: i8,
}

/// What the pause menu did in one call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuOut {
    /// Still open (or still closed).
    Stay,
    /// Closed with this setting chosen.
    Chosen(Detail),
    /// Closed: start the area again.
    Restart,
    /// Closed: open the area menu.
    Areas,
    /// Closed with no change.
    Closed,
}

impl Default for Menu {
    fn default() -> Menu {
        Menu::new()
    }
}

impl Menu {
    pub const fn new() -> Menu {
        Menu {
            open: false,
            sel: 1,
            stick_was: 0,
        }
    }

    /// Opens on the setting in use. `stick_y` is the left stick's y as
    /// the menu opens: a stick already held then moves nothing until it
    /// is let go and pushed again.
    pub fn open(&mut self, now: Detail, stick_y: i16) {
        self.open = true;
        self.sel = now.level();
        self.stick_was = stick_dir(stick_y);
    }

    /// One call while open: `pressed` are this call's new presses,
    /// `stick_y` the left stick's y.
    pub fn update(&mut self, pressed: u32, stick_y: i16) -> MenuOut {
        if !self.open {
            return MenuOut::Stay;
        }
        match moved(pressed, stick_y, &mut self.stick_was) {
            -1 => self.sel = self.sel.saturating_sub(1),
            1 => self.sel = (self.sel + 1).min(AREAS),
            _ => {}
        }
        if pressed & (button::A | button::START) != 0 {
            self.open = false;
            return match self.sel {
                RESTART => MenuOut::Restart,
                AREAS => MenuOut::Areas,
                s => MenuOut::Chosen(Detail::ALL[usize::from(s)]),
            };
        }
        if pressed & button::B != 0 {
            self.open = false;
            return MenuOut::Closed;
        }
        MenuOut::Stay
    }
}

/// The area menu's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AreaMenu {
    pub open: bool,
    /// The highlighted line ([`Area::ALL`] order).
    pub sel: u8,
    stick_was: i8,
    /// B (or a tap outside) may go back to play: false at the start.
    pub back_ok: bool,
}

/// What the area menu did in one call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AreaOut {
    Stay,
    /// Closed: start this area.
    Chosen(Area),
    /// Closed: back to play with nothing changed.
    Back,
}

/// A touch for the menus: where (game pixels) and whether the finger
/// just came up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tap {
    pub x: i16,
    pub y: i16,
    pub ended: bool,
}

impl Default for AreaMenu {
    fn default() -> AreaMenu {
        AreaMenu::new()
    }
}

impl AreaMenu {
    /// Closed (the pictures of tests that draw no area menu).
    pub const CLOSED: AreaMenu = AreaMenu {
        open: false,
        sel: 0,
        stick_was: 0,
        back_ok: true,
    };

    /// Open, as at the start: on ISLAND, no going back.
    pub const fn new() -> AreaMenu {
        AreaMenu {
            open: true,
            sel: 0,
            stick_was: 0,
            back_ok: false,
        }
    }

    /// Opens on the area in use; `back_ok`: B may go back to play.
    /// `stick_y` as for [`Menu::open`].
    pub fn open(&mut self, now: Area, back_ok: bool, stick_y: i16) {
        self.open = true;
        self.sel = now as u8;
        self.stick_was = stick_dir(stick_y);
        self.back_ok = back_ok;
    }

    /// The line under a point (game pixels), if any.
    pub fn line_at(x: i16, y: i16) -> Option<u8> {
        let (bx, _, bw, _) = AREA_BOX;
        if x < bx || x >= bx + bw as i16 || y < AREA_LINE0 {
            return None;
        }
        let n = (y - AREA_LINE0) / 10;
        (n < Area::ALL.len() as i16).then_some(n as u8)
    }

    /// Whether a point is inside the box.
    pub fn in_box(x: i16, y: i16) -> bool {
        let (bx, by, bw, bh) = AREA_BOX;
        x >= bx && x < bx + bw as i16 && y >= by && y < by + bh as i16
    }

    /// One call while open: this call's presses, the left stick's y and
    /// the touch.
    pub fn update(&mut self, pressed: u32, stick_y: i16, tap: Option<Tap>) -> AreaOut {
        if !self.open {
            return AreaOut::Stay;
        }
        let last = Area::ALL.len() as u8 - 1;
        match moved(pressed, stick_y, &mut self.stick_was) {
            -1 => self.sel = self.sel.saturating_sub(1),
            1 => self.sel = (self.sel + 1).min(last),
            _ => {}
        }
        if let Some(t) = tap {
            if let Some(n) = AreaMenu::line_at(t.x, t.y) {
                self.sel = n;
                if t.ended {
                    self.open = false;
                    return AreaOut::Chosen(Area::ALL[usize::from(n)]);
                }
            } else if t.ended && self.back_ok && !AreaMenu::in_box(t.x, t.y) {
                self.open = false;
                return AreaOut::Back;
            }
        }
        if pressed & (button::A | button::START) != 0 {
            self.open = false;
            return AreaOut::Chosen(Area::ALL[usize::from(self.sel)]);
        }
        if pressed & button::B != 0 && self.back_ok {
            self.open = false;
            return AreaOut::Back;
        }
        AreaOut::Stay
    }
}
