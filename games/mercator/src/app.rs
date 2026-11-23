//! The game's screens and controls round the world: the title, the
//! station (market, outfit, chart), flying, the jump, the lost ship, and
//! the pause menu. [`App::update`] takes one call's [`Keys`] and the
//! game time; [`App::draw`] draws. Tests drive these two directly.
//!
//! **Controls (pad).** Left stick or d-pad: nose up/down, left/right.
//! Right stick sideways or L/R: roll. A: fire. X: faster (at top speed,
//! X again: CRUISE; where cruise is refused, X pressed again and held:
//! OVERBOOST while its charge lasts, no firing). B: slower (drops
//! cruise). Right stick up/down or the triggers: speed too. Y:
//! auto-dock on/off. SELECT: chart. START: pause. In menus: d-pad and A,
//! B to go back. In the market, a held buy or sell key goes on trading
//! ([`REPEAT_FIRST`]). Player 2's pad is the gunner: holding A fires the
//! dorsal turret, which aims itself at a raider in reach.
//!
//! **Touch.** A finger on the view steers towards it and fires when a
//! raider is in the sights. Panel: radar = chart, left bars = slower,
//! right bars = faster (at top speed: cruise; where it is refused, a new
//! touch held there overboosts), AUTO = auto-dock, the top line of the
//! view = pause. Every menu line can be tapped.

use game_kit::{Frame, Touch, button};

use crate::art::{self, COLOURS_LEN};
use crate::chart;
use crate::fixed::Mat3;
use crate::fx;
use crate::hud::{self, Txt};
use crate::market::GOODS;
use crate::models;
use crate::planet::{self, Below, Outcome, space::Ring};
use crate::poly::{Rect, Renderer, Stats, View};
use crate::scene::{self, Detail, Eye, VIEW};
use crate::world::{self, Career, Event as WEvent, Kind, Pad, State, TICK_US, World};

/// At most this many ticks run in one call.
pub const MAX_CATCH_UP: u64 = 4;
pub const DEAD_ZONE: i32 = 4000;
/// Ticks before the lost card takes a press.
pub const CARD_WAIT: u32 = 60;
/// Ticks the jump tunnel lasts.
pub const JUMP_TICKS: u32 = 80;
/// A cost line for the log this often while flying (4 s).
pub const COST_EVERY: u32 = 240;
pub const NOTE_TICKS: u16 = 90;
/// Holding a buy or sell key in the market trades again by itself,
/// counted in game ticks (1/60 s), so at the same real speed at 30 and
/// at 60 pictures a second. The first repeat comes this many ticks
/// after the press (0.45 s): a normal press never trades twice.
pub const REPEAT_FIRST: u32 = 27;
/// Then one trade every this many ticks (10 ticks: 6 a second)...
pub const REPEAT_SLOW: u32 = 10;
/// ...one tick less every [`REPEAT_EASE`] ticks held, down to this (3
/// ticks: 20 a second). 27 + 7 x 9 = 90 ticks: full speed after 1.5 s.
pub const REPEAT_FAST: u32 = 3;
pub const REPEAT_EASE: u32 = 9;
pub const TITLE_LINES: u8 = 8;
pub const PAUSE_LINES: u8 = 6;
/// Galaxies to choose from.
pub const GALAXIES: u8 = 10;
/// The galaxy numbers as the title menu shows them.
const GALAXY_NUMS: [&str; GALAXIES as usize] = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "10"];
/// The whole picture as one view (docked, title), made at compile time.
const FULL: View = View::new(Rect::new(0, 0, 160, 120), 96);
/// Step M5: the shipyard's turning model ([`hud::YARD_VIEW`]).
const YARD: View = View::new(
    Rect::new(
        hud::YARD_VIEW.0 as i32,
        hud::YARD_VIEW.1 as i32,
        hud::YARD_VIEW.2 as i32,
        hud::YARD_VIEW.3 as i32,
    ),
    64,
);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Keys {
    pub held: u32,
    pub pressed: u32,
    pub left: (i16, i16),
    pub right: (i16, i16),
    pub triggers: (u16, u16),
    pub touch: Option<Touch>,
    /// Player 2 holds A: the dorsal turret fires.
    pub gunner: bool,
}

impl Keys {
    fn pressed(&self, b: u32) -> bool {
        self.pressed & b != 0
    }
    fn held(&self, b: u32) -> bool {
        self.held & b != 0
    }
    fn tap(&self) -> Option<(i16, i16)> {
        self.touch.filter(|t| t.started).map(|t| (t.x, t.y))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    Title,
    Docked,
    Market,
    Outfit,
    /// Step M5: ships to buy.
    Shipyard,
    Chart,
    Flight,
    Jump,
    Lost,
    /// The guild's charter is granted: the game is won.
    Won,
    /// Step G1: on the planet, below the ring (`planet`).
    Below,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub detail: Detail,
    pub eye: Eye,
    pub fast: bool,
    pub invert: bool,
    /// 1..=GALAXIES: the seed a new game uses.
    pub galaxy: u8,
}

impl Options {
    /// The settings byte of a save.
    pub fn byte(&self) -> u8 {
        let d = Detail::ALL
            .iter()
            .position(|&x| x == self.detail)
            .unwrap_or(0) as u8;
        d | u8::from(self.eye == Eye::Chase) << 2
            | u8::from(self.fast) << 3
            | u8::from(self.invert) << 4
    }

    /// The four setting lines of the title and pause menus.
    #[inline(never)]
    pub fn lines(&self) -> [(&'static str, &'static str); 4] {
        [
            ("DETAIL", self.detail.name()),
            ("VIEW", self.eye.name()),
            ("PICTURES", hud::rate_name(self.fast)),
            ("PITCH", hud::pitch_name(self.invert)),
        ]
    }
}

/// The seed of galaxy `n`.
#[inline(never)]
pub fn seed_of(n: u8) -> u32 {
    crate::fixed::hash(0x3E2C_A7F0, u32::from(n))
}

/// The note for a scooped canister, by good (the last: a raider's TAG,
/// `world::TAG`).
const SCOOPED: [&str; GOODS + 1] = [
    "SCOOPED: GRAIN",
    "SCOOPED: ICE",
    "SCOOPED: ORE",
    "SCOOPED: ALLOYS",
    "SCOOPED: SPARES",
    "SCOOPED: MEDS",
    "SCOOPED: CHIPS",
    "SCOOPED: ART",
    "SCOOPED: RAIDER TAG",
];

/// Something worth a log line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    NewGame {
        galaxy: u8,
    },
    Docked {
        system: u8,
        credits: u32,
        day: u16,
    },
    Jumped {
        to: u8,
        fuel: u8,
    },
    Killed {
        kind: Kind,
    },
    /// TAGs paid for at a dock.
    Bounty {
        sum: u32,
    },
    Lost {
        credits: u32,
        kills: u16,
    },
    Won {
        day: u16,
    },
    /// Step M5: a ship bought (`from`, `to`: [`world::CRAFTS`] numbers).
    Ship {
        from: u8,
        to: u8,
        credits: u32,
    },
    Settings,
    Cost {
        stats: Stats,
    },
    Quit,
}

/// A buy or sell key held in the market.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Repeat {
    /// 0 nothing, 1 buy, 2 sell.
    pub what: u8,
    /// The market row it trades on.
    pub row: u8,
    /// Game ticks held since the press, and the tick of the next trade.
    pub held: u32,
    pub next: u32,
}

pub struct App {
    pub scene: Scene,
    pub scene_t: u32,
    /// The selected line of the screen's menu.
    pub sel: u8,
    pub pause: Option<u8>,
    pub opts: Options,
    pub world: World,
    pub render: Renderer,
    pub ticks: u64,
    pub note: Option<(&'static str, u16)>,
    /// Credits the note ends with ("BOUNTY PAID: 225 CR"); 0: none.
    pub note_cr: u32,
    /// The chart: selection, zoom, and the screen to go back to.
    pub chart_sel: u8,
    pub chart_galaxy: bool,
    chart_back: Scene,
    /// The jump under way: where to.
    pub jump_to: u8,
    /// A career is being played (PLAY continues it).
    pub playing: bool,
    /// The career at the last dock (a lost ship goes back to it).
    pub last_dock: Career,
    /// The palette for the shown system, and which system it is for.
    pub colours: [u16; COLOURS_LEN],
    /// Step G1: the system the palette is for, and 0x100 while below.
    colours_for: Option<(u32, u16)>,
    stick_dir: (i8, i8),
    /// A held buy or sell key in the market.
    pub repeat: Repeat,
    /// The last tick's controls (tests read them).
    pub last_pad: Pad,
    events: [Option<Event>; 6],
    /// Step G1: the planet mode (`below.on` while the ship is there).
    pub below: Below,
}

impl Default for App {
    fn default() -> Self {
        App::new()
    }
}

impl App {
    pub fn new() -> App {
        let mut a = App::blank();
        a.load();
        a
    }

    /// The game before the first career is made: a `const`.
    pub const fn blank() -> App {
        App {
            scene: Scene::Title,
            scene_t: 0,
            sel: 0,
            pause: None,
            opts: Options {
                // The owner's starting settings (2026-11-23): HIGH, the
                // chase view, 60 a second.
                detail: Detail::High,
                eye: Eye::Chase,
                fast: true,
                invert: false,
                galaxy: 1,
            },
            world: World::empty(),
            render: Renderer::new(),
            ticks: 0,
            note: None,
            note_cr: 0,
            chart_sel: 0,
            chart_galaxy: false,
            chart_back: Scene::Docked,
            jump_to: 0,
            playing: false,
            last_dock: World::empty().car,
            colours: [0; COLOURS_LEN],
            colours_for: None,
            below: Below::EMPTY,
            stick_dir: (0, 0),
            repeat: Repeat {
                what: 0,
                row: 0,
                held: 0,
                next: 0,
            },
            last_pad: Pad {
                steer: (0, 0),
                roll: 0,
                throttle: 0,
                fire: false,
                cruise: false,
                auto: false,
                turret: false,
                boost: false,
            },
            events: [None; 6],
        }
    }

    /// Makes the title's galaxy (no career yet).
    #[inline(never)]
    pub fn load(&mut self) {
        self.world.begin(Career::new(seed_of(self.opts.galaxy)));
        self.below.on = false;
        self.below.taken_day = None;
    }

    pub fn take_event(&mut self) -> Option<Event> {
        let e = self.events[0];
        for i in 1..self.events.len() {
            self.events[i - 1] = self.events[i];
        }
        self.events[self.events.len() - 1] = None;
        e
    }

    #[inline(never)]
    fn event(&mut self, e: Event) {
        if let Some(slot) = self.events.iter_mut().find(|s| s.is_none()) {
            *slot = Some(e);
        }
    }

    /// The palette, when it must be set (a new system); None when the
    /// one set already fits.
    #[inline(never)]
    pub fn new_colours(&mut self) -> Option<&[u16]> {
        // Step G2: the loss card below stays on the planet's picture.
        let below = self.scene == Scene::Below || (self.scene == Scene::Lost && self.below.on);
        // Step G3: the storm's step below (bits 9 and up).
        let storm = if below {
            planet::clock::step(&self.below)
        } else {
            0
        };
        let now = (
            self.world.car.seed,
            u16::from(self.world.car.system)
                | if below { 0x100 } else { 0 }
                | u16::from(storm) << 9,
        );
        if self.colours_for == Some(now) {
            return None;
        }
        self.colours_for = Some(now);
        let sys = &self.world.sys;
        if below {
            planet::palette::below(&mut self.colours, sys.look, sys.star, sys.h, storm);
        } else {
            art::palette(&mut self.colours, sys.look, sys.star, sys.h);
        }
        Some(&self.colours)
    }

    /// The palette as it is now (to set it again after the console
    /// asked for a redraw).
    #[inline(never)]
    pub fn colours(&mut self) -> &[u16] {
        let _ = self.new_colours();
        &self.colours
    }

    /// Step M3: whether the live docked scene is on screen (DOCKED,
    /// MARKET, OUTFIT, step M5's SHIPYARD): the game draws it at 30
    /// pictures a second even in the fast mode.
    pub fn docked_view(&self) -> bool {
        matches!(
            self.scene,
            Scene::Docked | Scene::Market | Scene::Outfit | Scene::Shipyard
        )
    }

    /// Whether nothing moves until the player does something.
    pub fn still(&self) -> bool {
        if self.note.is_some() {
            return false;
        }
        if self.pause.is_some() {
            return true;
        }
        match self.scene {
            // A held market key trades on by itself: the game must be
            // called at the picture rate meanwhile.
            // Step M3: unless switched off, the docked scenes live on
            // ([`fx::DOCK_LIVE_ON`]): the console no longer rests there.
            Scene::Market => !fx::DOCK_LIVE_ON && self.repeat.what == 0,
            Scene::Docked | Scene::Outfit => !fx::DOCK_LIVE_ON,
            // Step M5: its model turns.
            Scene::Shipyard => false,
            Scene::Chart => self.chart_back == Scene::Docked,
            Scene::Lost | Scene::Won => self.scene_t >= CARD_WAIT + 60,
            _ => false,
        }
    }

    /// One call. False when the player chose LEAVE.
    #[inline(never)]
    pub fn update(&mut self, k: &Keys, now_us: u64) -> bool {
        let due = now_us / u64::from(TICK_US);
        let mut n = due.saturating_sub(self.ticks).min(MAX_CATCH_UP);
        self.ticks = due.max(self.ticks);
        let stick = stick_steps(k.left, &mut self.stick_dir);
        let nav = Nav {
            up: k.pressed(button::UP) || stick.1 < 0,
            down: k.pressed(button::DOWN) || stick.1 > 0,
            left: k.pressed(button::LEFT) || stick.0 < 0,
            right: k.pressed(button::RIGHT) || stick.0 > 0,
        };
        if let Some((t, left)) = self.note {
            let left = u64::from(left).saturating_sub(n.max(1));
            self.note = (left > 0).then_some((t, left as u16));
        }
        if let Some(sel) = self.pause {
            n = 0;
            self.pause_menu(k, sel, nav);
        } else {
            match self.scene {
                Scene::Title => {
                    if !self.title_menu(k, nav) {
                        self.event(Event::Quit);
                        return false;
                    }
                }
                Scene::Docked => self.docked_keys(k, nav),
                Scene::Market => self.market_keys(k, nav, n as u32),
                Scene::Outfit => self.outfit_keys(k, nav),
                Scene::Shipyard => self.shipyard_keys(k, nav),
                Scene::Chart => self.chart_keys(k, nav),
                Scene::Flight => self.flight_keys(k),
                // Step G1: below, START (or a tap on the top line)
                // pauses; B is read by the tick.
                Scene::Below => {
                    if k.pressed(button::START) || k.tap().is_some_and(|(_, y)| y < 10) {
                        self.pause = Some(0);
                    }
                }
                Scene::Jump => {}
                Scene::Lost | Scene::Won => self.lost_keys(k),
            }
            if self.pause.is_some() {
                n = 0;
            }
        }
        for _ in 0..n {
            self.tick(k);
        }
        true
    }

    #[inline(never)]
    fn go(&mut self, s: Scene) {
        self.scene = s;
        self.scene_t = 0;
        self.sel = 0;
        // A held market key never trades on another screen.
        self.repeat.what = 0;
    }

    #[inline(never)]
    pub fn note(&mut self, text: &'static str) {
        self.note = Some((text, NOTE_TICKS));
        self.note_cr = 0;
    }

    /// Changes setting `what` (0 detail, 1 view, 2 rate, 3 pitch).
    #[inline(never)]
    fn change(&mut self, what: u8, by: i32) {
        match what {
            0 => self.opts.detail = step(&Detail::ALL, self.opts.detail, by),
            1 => self.opts.eye = step(&Eye::ALL, self.opts.eye, by),
            2 => self.opts.fast = !self.opts.fast,
            _ => self.opts.invert = !self.opts.invert,
        }
        self.event(Event::Settings);
    }

    /// A new career in the chosen galaxy, docked at its start.
    #[inline(never)]
    pub fn new_game(&mut self) {
        self.world.begin(Career::new(seed_of(self.opts.galaxy)));
        self.below.on = false;
        self.below.taken_day = None;
        self.last_dock = self.world.car;
        self.playing = true;
        self.chart_sel = self.world.car.system;
        self.event(Event::NewGame {
            galaxy: self.opts.galaxy,
        });
        self.go(Scene::Docked);
    }

    /// Back to the career as it was at the last dock, but with the
    /// cargo and the TAGs lost ([`Career::wrecked`]). That is the last
    /// dock now, so a second loss before the next dock finds the hold
    /// empty too.
    #[inline(never)]
    pub fn restore(&mut self) {
        self.world.begin(self.last_dock.wrecked());
        self.below.on = false;
        self.last_dock = self.world.car;
        self.chart_sel = self.world.car.target;
        self.go(Scene::Docked);
    }

    #[inline(never)]
    fn title_menu(&mut self, k: &Keys, nav: Nav) -> bool {
        let card = (hud::TITLE_CARD, hud::TITLE_Y, TITLE_LINES);
        let (line, mut by) = menu_keys(k, nav, self.sel, card);
        let mut sel = line.unwrap_or(self.sel);
        if k.pressed(button::START) {
            sel = 0;
            by = CHOSEN;
        }
        self.sel = sel;
        let act = by == CHOSEN;
        match sel {
            0 if act => {
                if self.playing {
                    self.go(Scene::Docked);
                    if self.world.state == State::Flying {
                        // Step G1: back below when the ship is there.
                        self.scene = if self.below.on {
                            Scene::Below
                        } else {
                            Scene::Flight
                        };
                    }
                } else {
                    self.new_game();
                }
            }
            1 if act => self.new_game(),
            2 if by != 0 => {
                // galaxy 1..=GALAXIES, by -1 or 1.
                let g = i32::from(self.opts.galaxy)
                    .wrapping_sub(1)
                    .wrapping_add(by.min(1));
                self.opts.galaxy = g.rem_euclid(i32::from(GALAXIES)) as u8 + 1;
                if !self.playing {
                    self.load();
                }
            }
            3..=6 if by != 0 => self.change(sel - 3, by.min(1)),
            7 if act => return false,
            _ => {}
        }
        true
    }

    #[inline(never)]
    fn pause_menu(&mut self, k: &Keys, sel: u8, nav: Nav) {
        let card = (hud::PAUSE_CARD, hud::PAUSE_Y, PAUSE_LINES);
        let (line, by) = menu_keys(k, nav, sel, card);
        let Some(sel) = line.filter(|_| !k.pressed(button::START | button::B)) else {
            self.pause = None;
            return;
        };
        self.pause = Some(sel);
        match sel {
            0 if by == CHOSEN => self.pause = None,
            1..=4 if by != 0 => self.change(sel - 1, by.min(1)),
            5 if by == CHOSEN => {
                self.pause = None;
                self.go(Scene::Title);
            }
            _ => {}
        }
    }

    /// Up/down/A on a list of `lines`; returns the line chosen, if any.
    #[inline(never)]
    fn list(&mut self, k: &Keys, nav: Nav, lines: u8) -> Option<u8> {
        self.sel = up_down(self.sel, nav, lines);
        k.pressed(button::A).then_some(self.sel)
    }

    #[inline(never)]
    fn docked_keys(&mut self, k: &Keys, nav: Nav) {
        if k.pressed(button::START) {
            self.pause = Some(0);
            return;
        }
        let card = (hud::DOCK_CARD, hud::DOCK_Y, hud::DOCK_LINES);
        let (line, by) = menu_keys(k, nav, self.sel, card);
        self.sel = line.unwrap_or(self.sel);
        let mut pick = (by == CHOSEN).then_some(self.sel);
        if k.pressed(button::SELECT) {
            pick = Some(3);
        }
        match pick {
            Some(0) => {
                self.world.launch();
                self.go(Scene::Flight);
            }
            Some(1) => self.go(Scene::Market),
            Some(2) => self.go(Scene::Outfit),
            Some(3) => self.open_chart(Scene::Docked),
            Some(4) => {
                self.go(Scene::Shipyard);
                self.sel = self.world.car.ship;
            }
            _ => {}
        }
    }

    #[inline(never)]
    fn back_to_dock(&mut self, line: u8) {
        self.go(Scene::Docked);
        self.sel = line;
        self.last_dock = self.world.car;
    }

    /// The market's keys; `n`: the game ticks this call covers. A press
    /// buys or sells one tonne at once. Held, the same key goes on
    /// trading by itself, by game time: first after [`REPEAT_FIRST`]
    /// ticks, then faster ([`repeat_gap`]). It stops for good, with one
    /// note, at the first trade that cannot be made; and at once when the
    /// key or finger lets go or the row changes.
    #[inline(never)]
    fn market_keys(&mut self, k: &Keys, nav: Nav, n: u32) {
        let _ = self.list(k, nav, GOODS as u8);
        let mut buy = k.pressed(button::A) || nav.right;
        let mut sell = k.pressed(button::X) || nav.left;
        let mut back = k.pressed(button::B | button::START);
        // What is held now: 1 buy, 2 sell. The keys and the stick, or a
        // finger on the selected row's BUY or SELL column. A buy and a
        // sell key held together hold nothing.
        let hb = k.held(button::A | button::RIGHT) || self.stick_dir.0 > 0;
        let hs = k.held(button::X | button::LEFT) || self.stick_dir.0 < 0;
        let mut held = if hb == hs {
            0
        } else if hb {
            1
        } else {
            2
        };
        if let Some(t) = k.touch {
            held = 0;
            if t.started && hud::in_box(t.x, t.y, hud::BACK_BOX) {
                back = true;
            } else if let Some(row) = row_at(t.y, GOODS as u8) {
                let col = match t.x {
                    44..=71 => 1,
                    72..=99 => 2,
                    _ => 0,
                };
                if t.started {
                    self.sel = row;
                    buy = col == 1;
                    sell = col == 2;
                }
                // A finger slid to another row holds nothing.
                if t.down && row == self.sel {
                    held = col;
                }
            }
        }
        let g = self.sel;
        let mut r = self.repeat;
        self.repeat.what = 0;
        if buy || sell {
            // A new press: one trade now (both, as ever, when both keys
            // came in the same call); while its key stays held it repeats.
            let b = !buy || self.trade(1, g);
            let s = !sell || self.trade(2, g);
            let what = if buy { 1 } else { 2 };
            if b && s && held == what {
                self.repeat = Repeat {
                    what,
                    row: g,
                    held: 0,
                    next: REPEAT_FIRST,
                };
            }
        } else if r.what != 0 && r.what == held && r.row == g {
            // Still held on the same row: tick by tick of game time, the
            // repeats that fell due (at most one a tick).
            for _ in 0..n {
                // Ticks held: 2^32 ticks are 2 years.
                r.held = r.held.wrapping_add(1);
                if r.held >= r.next {
                    r.next = r.next.wrapping_add(repeat_gap(r.next));
                    if !self.trade(r.what, g) {
                        // Its note shows once; nothing more until a new
                        // press.
                        r.what = 0;
                        break;
                    }
                }
            }
            self.repeat = r;
        }
        if back {
            self.back_to_dock(1);
        }
    }

    /// One trade in the market: `what` 1 buys a tonne of good `g`, 2
    /// sells one. False (with the reason as a note) when it cannot.
    #[inline(never)]
    fn trade(&mut self, what: u8, g: u8) -> bool {
        let g = usize::from(g);
        if what == 1 {
            if self.world.buy(g) {
                return true;
            }
            let c = &self.world.car;
            self.note(if c.room() == 0 {
                "THE HOLD IS FULL"
            } else if self.world.stock[g] == 0 {
                "NONE LEFT HERE"
            } else {
                "NOT ENOUGH CREDITS"
            });
            false
        } else {
            let ok = self.world.sell(g);
            if !ok {
                self.note("NONE IN THE HOLD");
            }
            ok
        }
    }

    #[inline(never)]
    fn outfit_keys(&mut self, k: &Keys, nav: Nav) {
        let mut pick = self.list(k, nav, hud::OUTFIT_LINES);
        let mut back = k.pressed(button::B | button::START);
        if let Some((x, y)) = k.tap() {
            if hud::in_box(x, y, hud::BACK_BOX) {
                back = true;
            } else if let Some(row) = row_at(y, hud::OUTFIT_LINES) {
                // The first tap selects, the second buys.
                if row == self.sel {
                    pick = Some(row);
                }
                self.sel = row;
            }
        }
        match pick {
            Some(0) => {
                if self.world.buy_fuel() == 0 {
                    self.note(if self.world.car.fuel >= world::TANK {
                        "THE TANK IS FULL"
                    } else {
                        "NOT ENOUGH CREDITS"
                    });
                }
            }
            Some(1) => {
                if self.world.repair() == 0 {
                    let c = &self.world.car;
                    self.note(if i32::from(c.hull) >= c.hull_max() {
                        "THE HULL IS SOUND"
                    } else {
                        "NOT ENOUGH CREDITS"
                    });
                }
            }
            Some(n) => match self.world.buy_gear(usize::from(n - 2)) {
                Ok(()) => self.note("FITTED"),
                Err(why) => self.note(why),
            },
            None => {}
        }
        if back {
            self.back_to_dock(2);
        }
    }

    /// Step M5: the shipyard's keys. Up/down pick a ship, A (or a tap
    /// on the picked row) buys it with the flown one traded in.
    #[inline(never)]
    fn shipyard_keys(&mut self, k: &Keys, nav: Nav) {
        let mut pick = self.list(k, nav, hud::YARD_LINES);
        let mut back = k.pressed(button::B | button::START);
        if let Some((x, y)) = k.tap() {
            if hud::in_box(x, y, hud::BACK_BOX) {
                back = true;
            } else if let Some(row) = row_at(y, hud::YARD_LINES) {
                // The first tap selects, the second buys.
                if row == self.sel {
                    pick = Some(row);
                }
                self.sel = row;
            }
        }
        if let Some(n) = pick {
            let was = self.world.car.ship;
            match self.world.buy_ship(usize::from(n)) {
                Ok(()) => {
                    self.event(Event::Ship {
                        from: was,
                        to: n,
                        credits: self.world.car.credits,
                    });
                    self.note("NEW SHIP: ENJOY");
                }
                Err(why) => self.note(why),
            }
        }
        if back {
            self.back_to_dock(4);
        }
    }

    #[inline(never)]
    fn open_chart(&mut self, from: Scene) {
        self.chart_back = from;
        self.chart_sel = self.world.car.target;
        self.go(Scene::Chart);
    }

    #[inline(never)]
    fn chart_keys(&mut self, k: &Keys, nav: Nav) {
        let seed = self.world.car.seed;
        let dir = if nav.up {
            Some((0, -1))
        } else if nav.down {
            Some((0, 1))
        } else if nav.left {
            Some((-1, 0))
        } else if nav.right {
            Some((1, 0))
        } else {
            None
        };
        if let Some((dx, dy)) = dir {
            self.chart_sel = chart::step(seed, self.chart_sel, dx, dy);
        }
        let mut act = k.pressed(button::A);
        let mut back = k.pressed(button::B | button::SELECT);
        let mut zoom = k.pressed(button::X);
        if k.pressed(button::START) {
            self.pause = Some(0);
            return;
        }
        if let Some((x, y)) = k.tap() {
            match chart::box_at(x, y) {
                Some(0) => zoom = true,
                Some(1) => act = true,
                Some(_) => back = true,
                None if y < 96 => {
                    if let Some(s) = chart::system_at(&self.world, self.chart_galaxy, x, y) {
                        act = s == self.chart_sel;
                        self.chart_sel = s;
                    }
                }
                None => {}
            }
        }
        if zoom {
            self.chart_galaxy = !self.chart_galaxy;
        }
        self.world.car.target = self.chart_sel;
        if act {
            if self.chart_back == Scene::Docked {
                self.note("COURSE SET");
            } else {
                match self.world.can_jump(self.chart_sel) {
                    Ok(_) => {
                        self.jump_to = self.chart_sel;
                        self.go(Scene::Jump);
                        return;
                    }
                    Err(why) => self.note(why),
                }
            }
        }
        if back {
            let to = self.chart_back;
            self.go(to);
            if to == Scene::Docked {
                self.sel = 3;
            }
        }
    }

    #[inline(never)]
    fn flight_keys(&mut self, k: &Keys) {
        if k.pressed(button::START) {
            self.pause = Some(0);
            return;
        }
        if k.pressed(button::SELECT) {
            self.open_chart(Scene::Flight);
            return;
        }
        if let Some((x, y)) = k.tap() {
            if y < 10 {
                self.pause = Some(0);
            } else if y >= hud::PANEL_Y && x < 38 {
                self.open_chart(Scene::Flight);
            }
        }
    }

    #[inline(never)]
    fn lost_keys(&mut self, k: &Keys) {
        if self.scene_t < CARD_WAIT {
            return;
        }
        if self.scene == Scene::Won {
            if k.pressed(button::A | button::B | button::START) || k.tap().is_some() {
                self.go(Scene::Docked);
            }
        } else if k.pressed(button::A | button::START) || k.tap().is_some_and(|(_, y)| y < 66) {
            self.restore();
        } else if k.pressed(button::B) || k.tap().is_some() {
            self.go(Scene::Title);
            self.playing = false;
            self.load();
        }
    }

    /// The pilot's wishes from the pad and the finger.
    #[inline(never)]
    pub fn controls(&self, k: &Keys) -> Pad {
        let dead = |v: i16| -> i32 {
            let v = i32::from(v);
            if v.abs() < DEAD_ZONE {
                0
            } else {
                (v - v.signum() * DEAD_ZONE) * 256 / (32_767 - DEAD_ZONE)
            }
        };
        let mut sx = dead(k.left.0);
        let mut sy = -dead(k.left.1);
        if k.held(button::LEFT) {
            sx = -256;
        }
        if k.held(button::RIGHT) {
            sx = 256;
        }
        if k.held(button::UP) {
            sy = 256;
        }
        if k.held(button::DOWN) {
            sy = -256;
        }
        let mut roll = dead(k.right.0);
        if k.held(button::L) {
            roll = -256;
        }
        if k.held(button::R) {
            roll = 256;
        }
        let trig = |t: u16| i32::from(t) * 256 / 32_767;
        let mut throttle = trig(k.triggers.1) - trig(k.triggers.0) - dead(k.right.1);
        if k.held(button::X) {
            throttle = 256;
        }
        if k.held(button::B) {
            throttle = -256;
        }
        let me = &self.world.me;
        let mut fire = k.held(button::A);
        let mut auto = k.pressed(button::Y);
        let mut faster = k.pressed(button::X);
        // Step M4: the faster key held keeps an overboost running.
        let mut boost = k.held(button::X);
        let mut slower = k.pressed(button::B) || throttle < -64;
        if let Some(t) = k.touch.filter(|t| t.down) {
            if t.y >= 10 && t.y < hud::PANEL_Y {
                sx = ((i32::from(t.x) - 80) * 256 / 40).clamp(-256, 256);
                sy = ((48 - i32::from(t.y)) * 256 / 30).clamp(-256, 256);
                fire = fire || self.world.foe_in_sights();
            } else if t.y >= hud::PANEL_Y {
                match t.x {
                    38..=81 => {
                        throttle = -256;
                        slower |= t.started;
                    }
                    82..=121 => {
                        throttle = 256;
                        faster |= t.started;
                        boost = true;
                    }
                    122.. => auto |= t.started,
                    _ => {}
                }
            }
        }
        if self.opts.invert {
            sy = -sy;
        }
        // Step M5: at the ship's own top speed.
        let cruise =
            (faster && !me.cruise && me.throttle >= self.world.top()) || (slower && me.cruise);
        Pad {
            steer: (sx.clamp(-256, 256), sy.clamp(-256, 256)),
            roll: roll.clamp(-256, 256),
            throttle: throttle.clamp(-256, 256),
            fire,
            cruise,
            auto,
            turret: k.gunner,
            boost,
        }
    }

    #[inline(never)]
    fn tick(&mut self, k: &Keys) {
        self.scene_t = self.scene_t.saturating_add(1);
        match self.scene {
            Scene::Flight => {}
            Scene::Lost => {
                self.world.tick(&Pad::default());
                return;
            }
            Scene::Jump => {
                if self.scene_t >= JUMP_TICKS {
                    let to = self.jump_to;
                    self.world.jump(to);
                    self.event(Event::Jumped {
                        to,
                        fuel: self.world.car.fuel,
                    });
                    self.go(Scene::Flight);
                    self.note("ARRIVED");
                }
                return;
            }
            Scene::Chart if self.chart_back == Scene::Flight => {
                // The ship flies on (straight) while the chart is open.
                self.world.tick(&Pad::default());
                self.world_events();
                return;
            }
            // Step M3: docked, only the scenery moves (no rule time).
            Scene::Docked | Scene::Market | Scene::Outfit | Scene::Shipyard => {
                self.world.scenery();
                return;
            }
            // Step G1: below, space is frozen (no World::tick).
            Scene::Below => {
                self.below_tick(k);
                return;
            }
            _ => return,
        }
        let pad = self.controls(k);
        self.last_pad = pad;
        let old = self.world.me.pos;
        self.world.tick(&pad);
        self.world_events();
        // Step G1: the planet's ring (a crossing enters, or is refused).
        if self.scene == Scene::Flight {
            match planet::space::ring_check(&self.world, old) {
                Ring::Enter => self.enter_below(),
                Ring::Refused(text) => self.note(text),
                Ring::Miss => {}
            }
        }
        if self.world.ticks.is_multiple_of(COST_EVERY) {
            self.event(Event::Cost {
                stats: self.render.stats,
            });
        }
    }

    /// Step G1: the ship crossed the ring: the site is built and the
    /// scene goes below (space stays as it is until the lift-off).
    #[inline(never)]
    fn enter_below(&mut self) {
        if planet::begin(&mut self.below, &self.world) {
            self.go(Scene::Below);
        }
    }

    /// Step G1: back to space over the ring.
    #[inline(never)]
    fn lift_off(&mut self) {
        planet::lift_off(&mut self.world, &self.below);
        self.below.on = false;
        self.go(Scene::Flight);
    }

    /// Step G2: one tick below: the pad and the finger as the lander's
    /// input, the tick, its notes, and the way out (the take-off's end,
    /// or the ship lost).
    #[inline(never)]
    fn below_tick(&mut self, k: &Keys) {
        let dead = |v: i16| -> i32 {
            let v = i32::from(v);
            if v.abs() < DEAD_ZONE {
                0
            } else {
                (v - v.signum() * DEAD_ZONE) * 256 / (32_767 - DEAD_ZONE)
            }
        };
        let mut push = [dead(k.left.0), -dead(k.left.1)];
        for (bit, axis, by) in [
            (button::LEFT, 0, -256),
            (button::RIGHT, 0, 256),
            (button::UP, 1, 256),
            (button::DOWN, 1, -256),
        ] {
            if k.held(bit) {
                push[axis] = by;
            }
        }
        let trig = i32::from(k.triggers.1) * 256 / 32_767;
        let mut lift = trig.max(-dead(k.right.1)).max(0);
        if k.held(button::R) {
            lift = 256;
        }
        let input = planet::lander::In {
            held: k.held,
            // Every tick of the call gets the call's presses: the lander
            // counts each once (its `prev`).
            pressed: k.pressed,
            push,
            lift,
            finger: k.touch.filter(|t| t.down).map(|t| (t.x, t.y)),
            // Step G3: the right stick on foot (PITCH INVERTED applies).
            look: [
                dead(k.right.0),
                if self.opts.invert {
                    dead(k.right.1)
                } else {
                    -dead(k.right.1)
                },
            ],
        };
        let out = planet::tick(&mut self.below, &input);
        if let Some(text) = self.below.note.take() {
            self.note(text);
        }
        match out {
            Outcome::Lifted => self.lift_off(),
            Outcome::Lost => {
                // As a loss in space: the hull at 0, the ship dies, the
                // LOST card (over the planet's last picture), and A
                // goes back to the last dock without cargo or TAGs.
                self.world.car.hull = 0;
                self.world.lose();
                self.world_events();
            }
            Outcome::Stay => {
                if self.scene_t.is_multiple_of(COST_EVERY) {
                    self.event(Event::Cost {
                        stats: self.render.stats,
                    });
                }
            }
        }
    }

    #[inline(never)]
    fn world_events(&mut self) {
        while let Some(e) = self.world.take_event() {
            match e {
                WEvent::Docked => {
                    let c = self.world.car;
                    self.event(Event::Docked {
                        system: c.system,
                        credits: c.credits,
                        day: c.day,
                    });
                    let win = !c.won && c.charter_ready();
                    self.world.car.won |= win;
                    self.back_to_dock(1);
                    if win {
                        self.event(Event::Won { day: c.day });
                        self.go(Scene::Won);
                    } else {
                        self.note("DOCKED");
                    }
                }
                WEvent::Crash => self.note("COLLISION"),
                WEvent::Killed { kind } => {
                    self.event(Event::Killed { kind });
                    self.note(if kind == Kind::Raider {
                        "RAIDER DOWN: SCOOP ITS TAG"
                    } else {
                        "SHIP DESTROYED"
                    });
                }
                WEvent::Scooped { good, .. } => {
                    self.note(SCOOPED[usize::from(good) % SCOOPED.len()]);
                }
                WEvent::HoldFull => self.note("HOLD FULL"),
                WEvent::Hit => {}
                WEvent::Overheat => self.note("OVERHEATING"),
                WEvent::Wanted => self.note("OUTLAW: WARDENS WILL HUNT YOU"),
                WEvent::Surveyed => self.dock_note("NEW SYSTEM SURVEYED: FEE PAID", 0),
                WEvent::Bounty { sum } => {
                    self.event(Event::Bounty { sum });
                    self.dock_note("BOUNTY PAID: ", sum);
                }
                WEvent::Unpaid { lawless } => self.dock_note(
                    if lawless {
                        "NO BOUNTY OFFICE HERE"
                    } else {
                        "OUTLAW: NO BOUNTY PAID"
                    },
                    0,
                ),
                WEvent::Died => {
                    let c = self.world.car;
                    self.event(Event::Lost {
                        credits: c.credits,
                        kills: c.kills,
                    });
                    self.go(Scene::Lost);
                }
                WEvent::AutoOff => self.note("AUTO DOCK OFF"),
                WEvent::Towed => self.note("TOW LANE: THE STATION TAKES YOU IN"),
                WEvent::AutoNo => self.note("NO AUTO DOCK: UNDER ATTACK"),
                WEvent::CruiseNo => self.note("NO CRUISE: TOO NEAR"),
                WEvent::Fuel => self.note("TANK FULL"),
                WEvent::Raiders => {
                    if self.world.sys.danger > 0 {
                        self.note("RAIDERS ON THE RADAR");
                    }
                }
            }
        }
    }

    /// A note at a dock (none over the charter's card); `cr` above 0
    /// ends it with that sum.
    #[inline(never)]
    fn dock_note(&mut self, text: &'static str, cr: u32) {
        if self.scene != Scene::Won {
            self.note(text);
            self.note_cr = cr;
        }
    }

    /// Draws the whole picture.
    #[inline(never)]
    pub fn draw(&mut self, s: &mut Frame) {
        self.render.begin();
        match self.scene {
            Scene::Title => self.draw_title(s),
            Scene::Docked | Scene::Won | Scene::Market | Scene::Outfit | Scene::Shipyard => {
                let all = FULL;
                // Step M3: the docked view drifts (Won: `show` is 0 there
                // unless it came from a dock, the old still view's turn).
                let cam = scene::station_camera(&self.world, scene::dock_turn(&self.world));
                // Behind MARKET's and OUTFIT's darkened panels the scene
                // is drawn at LOW detail: it costs less and hardly shows.
                let detail = match self.scene {
                    Scene::Market | Scene::Outfit | Scene::Shipyard => Detail::Low,
                    _ => self.opts.detail,
                };
                scene::draw(s, &mut self.render, &self.world, detail, &all, &cam);
                match self.scene {
                    Scene::Won => hud::lost(s, &self.world, self.scene_t >= CARD_WAIT, true, false),
                    // Step M3: MARKET and OUTFIT are panels over the
                    // live scene.
                    Scene::Market => hud::market(s, &self.world, self.sel),
                    Scene::Outfit => hud::outfit(s, &self.world, self.sel),
                    Scene::Shipyard => {
                        hud::shipyard(s, &self.world, self.sel);
                        // Step M5: the picked ship, turning, 75 m away
                        // (its whole sphere fits the view box).
                        let spin = (self.ticks as u32).wrapping_mul(200) as u16;
                        scene::showcase(
                            s,
                            &mut self.render,
                            &YARD,
                            models::craft_model(self.sel),
                            Mat3::rotation(spin, 2500, 0),
                            75 * world::M,
                        );
                    }
                    _ => hud::docked(
                        s,
                        &self.world,
                        self.sel,
                        planet::space::cut_today(&self.below, &self.world),
                    ),
                }
            }
            Scene::Chart => chart::draw(
                s,
                &self.world,
                self.chart_sel,
                self.chart_galaxy,
                self.chart_back == Scene::Docked,
            ),
            // Step G2: lost below: the planet's last picture under the
            // card.
            Scene::Lost if self.below.on => {
                planet::draw(s, &mut self.render, &self.below, self.opts.detail);
                planet::panel(s, &self.below);
                let pilot = planet::on_foot(&self.below);
                hud::lost(s, &self.world, self.scene_t >= CARD_WAIT, false, pilot);
            }
            Scene::Flight | Scene::Lost => {
                let eye = if self.scene == Scene::Lost {
                    Eye::Cockpit
                } else {
                    self.opts.eye
                };
                let cam = scene::camera(&self.world, eye);
                scene::draw(
                    s,
                    &mut self.render,
                    &self.world,
                    self.opts.detail,
                    &VIEW,
                    &cam,
                );
                if self.scene == Scene::Flight {
                    hud::view_marks(s, &self.world, &cam, eye == Eye::Cockpit);
                }
                hud::panel(s, &self.world);
                if self.scene == Scene::Lost {
                    hud::lost(s, &self.world, self.scene_t >= CARD_WAIT, false, false);
                }
            }
            // Step G1: the planet below.
            Scene::Below => {
                planet::draw(s, &mut self.render, &self.below, self.opts.detail);
                planet::panel(s, &self.below);
            }
            Scene::Jump => {
                hud::tunnel(s, self.scene_t);
                hud::panel(s, &self.world);
                let to = crate::galaxy::system(self.world.car.seed, self.jump_to);
                let mut t = Txt::new();
                t.s("JUMP TO ").b(to.name().bytes());
                hud::note(s, t.bytes(), 40);
            }
        }
        if let Some(sel) = self.pause {
            let [a, b, c, d] = self.opts.lines();
            let lines = [("RESUME", ""), a, b, c, d, ("TITLE", "")];
            hud::menu(s, hud::PAUSE_CARD, hud::PAUSE_Y, sel, &lines, 50);
            hud::centred(s, hud::PAUSE_CARD.1 + 3, b"PAUSED", art::ACCENT);
        }
        if let Some((text, _)) = self.note {
            let y = match self.scene {
                Scene::Flight | Scene::Jump | Scene::Below => 11,
                Scene::Docked => 20,
                _ => 86,
            };
            let mut t = Txt::new();
            t.s(text);
            if self.note_cr > 0 {
                t.num(self.note_cr).s(" CR");
            }
            hud::note(s, t.bytes(), y);
        }
    }

    #[inline(never)]
    fn draw_title(&mut self, s: &mut Frame) {
        // The start system's station and planet, slowly circled.
        let all = FULL;
        let turn = (self.ticks as u32).wrapping_mul(40) as u16;
        let cam = scene::station_camera(&self.world, turn);
        scene::draw(
            s,
            &mut self.render,
            &self.world,
            self.opts.detail,
            &all,
            &cam,
        );
        const SHIP: View = View::new(Rect::new(0, 18, 160, 34), 80);
        let ship = SHIP;
        let spin = (self.ticks as u32).wrapping_mul(200) as u16;
        scene::showcase(
            s,
            &mut self.render,
            &ship,
            // Step M5: the ship of the career on show.
            models::craft_model(self.world.car.ship),
            Mat3::rotation(spin, 2500, 0),
            110 * world::M,
        );
        let name = "MERCATOR";
        let w = game_kit::text_width(name) * 2;
        s.text_scaled(80 - (w / 2) as i16, 4, name, art::ACCENT, 2);
        let [a, b, c, d] = self.opts.lines();
        let lines = [
            (if self.playing { "CONTINUE" } else { "PLAY" }, ""),
            ("NEW GAME", ""),
            (
                "GALAXY",
                GALAXY_NUMS[usize::from(self.opts.galaxy.wrapping_sub(1)) % GALAXY_NUMS.len()],
            ),
            a,
            b,
            c,
            d,
            ("LEAVE", ""),
        ];
        hud::menu(s, hud::TITLE_CARD, hud::TITLE_Y, self.sel, &lines, 50);
    }
}

/// Menu moves of one call.
#[derive(Clone, Copy)]
struct Nav {
    up: bool,
    down: bool,
    left: bool,
    right: bool,
}

/// What [`menu_keys`] gives for a line chosen with A or a tap.
const CHOSEN: i32 = 2;

/// A menu card: its box, the first line's y, and the number of lines.
type Card = ((i16, i16, u16, u16), i16, u8);

/// Up/down, left/right, A and taps on a menu card. Returns the selected
/// line (None: a tap outside the card) and the change asked for: -1 or
/// 1 (left/right), [`CHOSEN`] (A or a tap on a line), or 0.
#[inline(never)]
fn menu_keys(k: &Keys, nav: Nav, mut sel: u8, card: Card) -> (Option<u8>, i32) {
    let (rect, first, lines) = card;
    sel = up_down(sel, nav, lines);
    let mut by = i32::from(nav.right) - i32::from(nav.left);
    if k.pressed(button::A) {
        by = CHOSEN;
    }
    if let Some((x, y)) = k.tap() {
        match hud::line_at(x, y, rect, first, lines) {
            Some(l) => {
                sel = l;
                by = CHOSEN;
            }
            None if hud::in_box(x, y, rect) => {}
            None => return (None, 0),
        }
    }
    (Some(sel), by)
}

/// Up and down a list of `lines` (at most a dozen), round at the ends.
#[inline(never)]
fn up_down(sel: u8, nav: Nav, lines: u8) -> u8 {
    // sel < lines <= 12: no sum wraps.
    let lines = lines.max(1);
    let mut sel = sel % lines;
    if nav.up {
        sel = sel.wrapping_add(lines).wrapping_sub(1) % lines;
    }
    if nav.down {
        sel = sel.wrapping_add(1) % lines;
    }
    sel
}

/// Ticks from a market repeat due at tick `at` (counted from the press)
/// to the next: [`REPEAT_SLOW`] at first, one less every
/// [`REPEAT_EASE`] ticks, never below [`REPEAT_FAST`].
pub fn repeat_gap(at: u32) -> u32 {
    let less = at.saturating_sub(REPEAT_FIRST) / REPEAT_EASE;
    REPEAT_SLOW.saturating_sub(less).max(REPEAT_FAST)
}

/// The row of the market or outfit list a tap at `y` hits.
#[inline(never)]
fn row_at(y: i16, rows: u8) -> Option<u8> {
    let r = (y - hud::LIST_Y + 2).div_euclid(hud::LIST_STEP);
    (y >= hud::LIST_Y - 2 && r < i16::from(rows)).then_some(r as u8)
}

#[inline(never)]
fn step<T: Copy + PartialEq>(all: &[T], now: T, by: i32) -> T {
    let i = all.iter().position(|v| *v == now).unwrap_or(0) as i32;
    all[(i + by).rem_euclid(all.len() as i32) as usize]
}

#[inline(never)]
fn stick_steps(stick: (i16, i16), last: &mut (i8, i8)) -> (i8, i8) {
    let dir = |v: i16| -> i8 {
        if v < -16000 {
            -1
        } else if v > 16000 {
            1
        } else {
            0
        }
    };
    let now = (dir(stick.0), dir(stick.1));
    let out = (
        if now.0 != last.0 { now.0 } else { 0 },
        if now.1 != last.1 { now.1 } else { 0 },
    );
    *last = now;
    out
}
