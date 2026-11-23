//! One whole step of play (hero, camera, collectibles), and the read
//! interface the drawing uses. Nothing here draws.
//!
//! Order in a step (AREAS-SPEC.md, section 2):
//! 1. the area's button actions ([`Kit::before`]);
//! 2. the hero's tuning for this step ([`Kit::body`]);
//! 3. the hero moves (with the camera's yaw of the last step);
//! 4. the area moves its machines ([`Kit::after`]), the movers' records
//!    are rebuilt and a hero standing on a moving top rides it;
//! 5. the camera follows;
//! 6. the collectibles;
//! 7. the shadow and the landing ring are worked out for the picture.
//!
//! On ISLAND (and every test world made with [`Play::on`]) there is no
//! area state: steps 1, 2 and 4 do nothing and the blocks are the world's
//! own, so everything is as before the areas.

use crate::area::{Area, Kit};
use crate::camera::Cam;
use crate::collect::{Coins, exists};
use crate::hero::{Body, Hero, HeroEvents, Pose, Ring, Shadow};
use crate::island::{Block, ISLAND, Prop, World, flag, top_under};
use crate::life::{Life, LifeEvents, STUN_FREE};
use crate::maths::{FNV_START, cm, fnv};
use crate::step::Pad;

/// The most blocks an area may have (static ones and movers).
pub const MAX_LIVE: usize = 12;

/// An area's blocks as they are now: the world's static blocks, then the
/// movers. Block numbers never change during play. Empty (`n` 0) on
/// ISLAND, whose blocks never move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Live {
    pub blocks: [Block; MAX_LIVE],
    pub n: u8,
}

impl Live {
    /// No blocks: the world's own are used.
    pub const EMPTY: Live = Live {
        blocks: [Block::flat([0; 3], [0; 3], 0, 0, flag::HIDE); MAX_LIVE],
        n: 0,
    };

    /// The world's records, the movers at their starting places.
    pub fn of(world: &World) -> Live {
        let mut l = Live::EMPTY;
        let n = world.blocks.len().min(MAX_LIVE);
        l.blocks[..n].copy_from_slice(&world.blocks[..n]);
        l.n = n as u8;
        l
    }
}

/// The blocks to play on: the live ones if the area has any, else the
/// world's.
fn blocks_of<'a>(world: &'static World, live: &'a Live) -> &'a [Block] {
    if live.n == 0 {
        world.blocks
    } else {
        &live.blocks[..usize::from(live.n)]
    }
}

/// The detail setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Detail {
    Low = 0,
    Mid = 1,
    High = 2,
    /// The fourth level, above HIGH: a rounder hero, finer ground near
    /// him, rounder trees, rocks and blocks, more ground decoration.
    Max = 3,
}

impl Detail {
    /// HIGH, the setting at the start (the owner's decision of
    /// 2026-11-23: every game starts at its level called HIGH; MID
    /// before).
    pub const START: Detail = Detail::High;

    /// The setting's number (0 LOW, 1 MID, 2 HIGH, 3 MAX), as the records'
    /// lowest-detail fields use it.
    pub const fn level(self) -> u8 {
        self as u8
    }

    /// Its name for the menu.
    pub const fn name(self) -> &'static str {
        match self {
            Detail::Low => "LOW",
            Detail::Mid => "MID",
            Detail::High => "HIGH",
            Detail::Max => "MAX",
        }
    }

    pub const ALL: [Detail; 4] = [Detail::Low, Detail::Mid, Detail::High, Detail::Max];
}

/// What happened in the last step.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Events {
    pub hero: HeroEvents,
    /// The coin taken in this step.
    pub took: Option<u8>,
    /// What the creatures did.
    pub life: LifeEvents,
}

/// The state of play.
#[derive(Clone, Copy, Debug)]
pub struct Play {
    /// The area's records (props, coins, start; its blocks as they were
    /// at the start: [`Play::blocks`] gives them as they are now).
    pub world: &'static World,
    /// Which area, and its own state.
    pub area: Area,
    pub kit: Kit,
    /// The blocks as they are now (empty on ISLAND).
    pub live: Live,
    /// The area's goal is reached: the rules stop.
    pub done: bool,
    /// [`Play::steps`] when it was reached (at most 65,535).
    pub done_steps: u16,
    pub hero: Hero,
    pub cam: Cam,
    pub coins: Coins,
    /// The creatures (ISLAND's; none elsewhere).
    pub life: Life,
    pub detail: Detail,
    /// Steps run since the start.
    pub steps: u32,
    /// The shadow and the landing ring after the last step.
    pub shadow: Option<Shadow>,
    pub ring: Option<Ring>,
    pub events: Events,
}

impl Play {
    /// Play on the test island at [`Detail::START`].
    pub fn new() -> Play {
        Play::on(&ISLAND, Detail::START)
    }

    /// Play in an area from its start. ISLAND is [`Play::on`] its
    /// records.
    pub fn start(area: Area, detail: Detail) -> Play {
        let mut p = Play::on(area.world(), detail);
        p.area = area;
        p.kit = Kit::new(area);
        if area != Area::Island {
            p.live = Live::of(p.world);
            let n = usize::from(p.live.n);
            let m = p.kit.movers_n().min(n);
            p.kit.movers(&mut p.live.blocks[n - m..n]);
            p.shadow = p.hero.shadow(p.blocks());
        }
        p
    }

    /// Play on any world with no area state (tests build their own).
    pub fn on(world: &'static World, detail: Detail) -> Play {
        let s = world.start;
        let pos = [cm(s[0]), cm(s[1]), cm(s[2])];
        let top = top_under(world.blocks, pos[0], pos[2], pos[1]).map_or(0, |(t, _)| t);
        let hero = Hero::new(pos, top);
        let mut cam = Cam::new(hero.yaw, pos[1]);
        cam.snap(&hero, world.blocks, world.props, detail.level());
        let mut p = Play {
            world,
            area: Area::Island,
            kit: Kit::None,
            live: Live::EMPTY,
            done: false,
            done_steps: 0,
            hero,
            cam,
            coins: Coins::new(),
            life: Life::new(world.critters),
            detail,
            steps: 0,
            shadow: None,
            ring: None,
            events: Events::default(),
        };
        p.shadow = p.hero.shadow(world.blocks);
        p
    }

    /// One step of 1/30 s. Once the area is done, nothing moves.
    // Kept out of the game's update: 88 bytes less code on the chip
    // (MEASURED, chip build, 2026-11-22).
    #[inline(never)]
    pub fn step(&mut self, pad: &Pad) {
        if self.done {
            return;
        }
        let w = self.world;
        let d = self.detail.level();
        self.kit.before(pad, &self.hero, blocks_of(w, &self.live));
        let body: Body = self.kit.body(&self.hero);
        // Knocked back by a creature: no input for a moment.
        let hp = if self.hero.stun > STUN_FREE {
            &Pad::NONE
        } else {
            pad
        };
        let before = self.hero.pos[1];
        let ev = self
            .hero
            .step_with(hp, self.cam.yaw, blocks_of(w, &self.live), &body);
        let n = usize::from(self.live.n);
        if n > 0 {
            let old = self.live;
            let done = self.kit.after(&mut self.hero, &ev, &old.blocks[..n]);
            let m = self.kit.movers_n().min(n);
            self.kit.movers(&mut self.live.blocks[n - m..n]);
            self.hero.ride(&old.blocks[..n], &self.live.blocks[..n]);
            self.done = done;
        }
        let blocks = blocks_of(w, &self.live);
        let life = self.life.step(
            w.critters,
            &mut self.hero,
            before,
            blocks,
            self.steps.wrapping_add(1),
        );
        if ev.respawned {
            self.cam.snap(&self.hero, blocks, w.props, d);
        } else if self.hero.frozen == 0 {
            self.cam.step(pad, &self.hero, blocks, w.props, d);
        }
        let took = self.coins.step(&self.hero, w, d);
        self.shadow = self.hero.shadow(blocks);
        self.ring = self.hero.ring_with(hp, self.cam.yaw, blocks, &body);
        self.events = Events {
            hero: ev,
            took,
            life,
        };
        self.steps = self.steps.wrapping_add(1);
        if self.done {
            self.done_steps = self.steps.min(u32::from(u16::MAX)) as u16;
        }
    }

    /// The blocks as they are now: the camera, the collectibles, the
    /// shadow and the drawing use these.
    pub fn blocks(&self) -> &[Block] {
        blocks_of(self.world, &self.live)
    }

    /// Changes the detail setting.
    pub fn set_detail(&mut self, d: Detail) {
        self.detail = d;
    }

    // ---- The read interface ------------------------------------------------------

    /// The hero's feet middle (subs).
    pub fn hero_pos(&self) -> [i32; 3] {
        self.hero.pos
    }

    /// The hero's facing (1/65536 turn).
    pub fn hero_yaw(&self) -> u16 {
        self.hero.yaw
    }

    /// The pose to draw.
    pub fn pose(&self) -> Pose {
        self.hero.pose()
    }

    /// The block whose top the hero stands on.
    pub fn ground(&self) -> Option<u8> {
        self.hero.ground
    }

    /// The camera's eye and aim point (subs).
    pub fn eye(&self) -> [i32; 3] {
        self.cam.eye
    }

    pub fn aim(&self) -> [i32; 3] {
        self.cam.aim
    }

    /// Bit `i`: collectible `i` taken.
    pub fn taken(&self) -> u8 {
        self.coins.taken
    }

    /// The counter: taken and total at this detail.
    pub fn count(&self) -> (u8, u8) {
        self.coins.count(self.world, self.detail.level())
    }

    /// Whether prop `i` exists at this detail.
    pub fn prop_shown(&self, i: usize) -> bool {
        self.world
            .props
            .get(i)
            .is_some_and(|p: &Prop| p.min_detail() <= self.detail.level())
    }

    /// Whether collectible `i` is there to draw (exists at this detail
    /// and not taken).
    pub fn coin_shown(&self, i: usize) -> bool {
        self.coins.shown(self.world, i, self.detail.level())
    }

    /// Whether collectible `i` exists at this detail (taken or not).
    pub fn coin_exists(&self, i: usize) -> bool {
        exists(self.world, i, self.detail.level())
    }

    /// A short fingerprint of the whole state: the same inputs give the
    /// same number.
    pub fn fingerprint(&self) -> u64 {
        let h = &self.hero;
        let c = &self.cam;
        let mut f = FNV_START;
        for v in h.pos.iter().chain(&h.vel).chain(&c.eye).chain(&c.aim) {
            f = fnv(f, i64::from(*v));
        }
        let rest = [
            i64::from(h.yaw),
            h.ground.map_or(-1, i64::from),
            i64::from(h.air_steps),
            i64::from(h.jump_buffer),
            i64::from(h.frozen),
            i64::from(h.falls),
            i64::from(h.jumps),
            i64::from(c.yaw),
            i64::from(c.pitch),
            i64::from(c.tilt),
            i64::from(self.coins.taken),
            i64::from(self.steps),
            // Everything else that shows later.
            i64::from(h.safe[0]),
            i64::from(h.safe[1]),
            i64::from(h.safe[2]),
            i64::from(h.safe_top),
            i64::from(h.on_top),
            i64::from(h.on_steps),
            i64::from(h.squash),
            i64::from(h.run_phase),
            i64::from(h.running),
            i64::from(c.ground_y),
            i64::from(c.turn_left),
            i64::from(c.turn_n),
            i64::from(c.shoulders),
            i64::from(c.idle),
            i64::from(c.placed),
            i64::from(self.coins.spin),
            i64::from(self.coins.bob_step),
        ];
        for v in rest {
            f = fnv(f, v);
        }
        // The creatures (ISLAND only).
        if !self.world.critters.is_empty() {
            f = self.life.hash(fnv(f, i64::from(h.stun)));
        }
        // The areas' state (never on ISLAND, so its numbers stay).
        if self.area != Area::Island {
            f = self.kit.hash(f);
            for b in &self.live.blocks[..usize::from(self.live.n)] {
                for v in b.lo.iter().chain(&b.hi) {
                    f = fnv(f, i64::from(*v));
                }
                f = fnv(f, i64::from(b.flags));
            }
            let more = [
                self.area as i64,
                i64::from(h.glide_left),
                i64::from(h.gliding),
                i64::from(self.done),
                i64::from(self.done_steps),
            ];
            for v in more {
                f = fnv(f, v);
            }
        }
        f
    }
}

impl Default for Play {
    fn default() -> Play {
        Play::new()
    }
}
