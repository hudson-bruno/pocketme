//! The computer's mallets, EASY, NORMAL or HARD.
//!
//! Each computer mallet looks at the table every few ticks (EASY seldom,
//! HARD often) and picks a point to go to:
//!
//! * the puck on its half: get behind it, on the line from the puck to
//!   the other goal (or to its mirror in a long rail, for a bank shot),
//!   then drive through it; around it first when standing on the wrong
//!   side;
//! * the puck coming its way: stand where it will cross the guard line
//!   (NORMAL and HARD follow it off the rails; EASY just follows the
//!   puck up and down);
//! * otherwise: back home, in front of its goal.
//!
//! The levels differ in top speed, how often they look, how well they
//! aim, and whether they guess the puck's path. NORMAL now and then plays
//! a bank shot. No level dashes: measured over 80 minutes of computer
//! against computer, dashing and bank shots made HARD concede more than
//! they won it (`print_duels`). All whole numbers; chance comes from the
//! match's own [`crate::fixed::Rng`], so a match is the same every time
//! it gets the same input.

use crate::fixed::{PX, UNIT, len, mul_div, norm};
use crate::table::{
    CX, CY, GOAL_HALF, HIT_R, Intent, Level, MALLET_R, Match, PUCK_R, Phase, YB, YT, attack,
    goal_line, home, zone_x,
};

/// What a level can do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Skill {
    /// Top speed (position units a tick).
    pub speed: i32,
    /// Ticks between looks at the table.
    pub think: u8,
    /// The aim is off by up to this much (position units).
    pub aim: i32,
    /// Follows the puck's path off the rails when guarding.
    pub predict: bool,
    /// Dashes into a strike.
    pub dash: bool,
    /// Plays bank shots off the long rails, one in this many strikes
    /// (0: never).
    pub bank: u32,
}

pub fn skill(l: Level) -> Skill {
    match l {
        Level::Easy => Skill {
            speed: 2 * PX,
            think: 10,
            aim: 16 * PX,
            predict: false,
            dash: false,
            bank: 0,
        },
        Level::Normal => Skill {
            speed: 3 * PX,
            think: 4,
            aim: 9 * PX,
            predict: true,
            dash: false,
            bank: 4,
        },
        Level::Hard => Skill {
            speed: 3 * PX + PX / 2,
            think: 2,
            aim: 4 * PX,
            predict: true,
            dash: false,
            bank: 0,
        },
    }
}

/// The intent of computer mallet `i` for this tick.
pub fn intent(m: &mut Match, i: usize) -> Intent {
    let sk = skill(m.rules.level);
    let mind = m.mind[i];
    if mind.wait > 0 {
        m.mind[i].wait -= 1;
        return Intent::Go {
            x: mind.tx,
            y: mind.ty,
            v: sk.speed,
            dash: false,
        };
    }
    m.mind[i].wait = sk.think;
    let (tx, ty, dash) = think(m, i, &sk);
    m.mind[i].tx = tx;
    m.mind[i].ty = ty;
    Intent::Go {
        x: tx,
        y: ty,
        v: sk.speed,
        dash,
    }
}

/// Where the puck crosses the line `x`, its path folded off the long
/// rails; none when it is not heading there.
pub fn cross_y(m: &Match, x: i32, fold: bool) -> Option<i32> {
    let p = m.puck;
    if p.vx == 0 || (x - p.x).signum() != p.vx.signum() {
        return None;
    }
    let t = i64::from(x - p.x) / i64::from(p.vx);
    let y = i64::from(p.y) + i64::from(p.vy) * t;
    if !fold {
        return Some(y.clamp(i64::from(YT), i64::from(YB)) as i32);
    }
    let (lo, hi) = (i64::from(YT + PUCK_R), i64::from(YB - PUCK_R));
    let span = hi - lo;
    let mut r = (y - lo).rem_euclid(2 * span);
    if r > span {
        r = 2 * span - r;
    }
    Some((lo + r) as i32)
}

fn think(m: &mut Match, i: usize, sk: &Skill) -> (i32, i32, bool) {
    let side = i % 2;
    let a = attack(side);
    let four = m.rules.four;
    let front = four && i >= 2;
    let (x0, x1) = zone_x(i, four);
    let me = m.m[i];
    let p = m.puck;
    let (hx, hy) = home(i, four);
    // The guard line: in front of the goal (the back mallet), or where
    // the front mallet stands now.
    let guard_x = if front {
        me.x
    } else {
        goal_line(side) + a * 16 * PX
    };

    match m.phase {
        Phase::Live => {}
        Phase::Serve { side: s, .. } if usize::from(s) == side => {
            // Get ready behind the puck, aimed at the far goal.
            let (bx, by, _) = strike(m, i, sk, false);
            return (bx, by, false);
        }
        _ => {
            // New aim for the next rally.
            m.mind[i].aim = m.rng.spread(sk.aim);
            return (hx, hy, false);
        }
    }

    let on_my_half = (p.x - CX) * a < 0;
    let coming = p.vx * a < 0;
    let reach = p.x >= x0 - HIT_R && p.x <= x1 + HIT_R;
    let fast = len(p.vx, p.vy) > 3 * PX;
    // How far ahead of the guard line the puck is.
    let ahead = (p.x - guard_x) * a;

    if on_my_half && reach && !(coming && fast && ahead > 30 * PX) {
        // A front mallet leaves a puck behind it to the back mallet.
        if !(front && (p.x - me.x) * a < -MALLET_SLACK) {
            return strike(m, i, sk, true);
        }
    }
    if coming {
        let y = if sk.predict {
            cross_y(m, guard_x, true).unwrap_or(p.y)
        } else {
            p.y
        };
        let lim = if front { 40 * PX } else { GOAL_HALF + 6 * PX };
        return (guard_x, y.clamp(CY - lim, CY + lim), false);
    }
    // Waiting: home, leaning towards the puck.
    m.mind[i].aim = m.rng.spread(sk.aim);
    (hx, hy + (p.y - hy) / 3, false)
}

/// A front mallet lets a puck this far behind it go.
const MALLET_SLACK: i32 = 4 * PX;

/// The point to drive at to strike the puck towards the other goal (or
/// a bank off a long rail), and whether to dash. `go` false: only the
/// spot behind the puck (waiting for a serve).
fn strike(m: &mut Match, i: usize, sk: &Skill, go: bool) -> (i32, i32, bool) {
    let side = i % 2;
    let me = m.m[i];
    let p = m.puck;
    // Lead a moving puck a little.
    let lead = (len(p.x - me.x, p.y - me.y) / sk.speed.max(1)).clamp(0, 12);
    let (px, py) = (p.x + p.vx * lead, p.y + p.vy * lead);
    // The aim: the far goal, or its mirror in a long rail (a bank).
    let goal_y = CY + m.mind[i].aim;
    let bank = sk.bank > 0 && m.mind[i].aim.rem_euclid(sk.bank as i32) == 0 && go;
    let ay = if !bank {
        goal_y
    } else if py < CY {
        2 * YT - goal_y
    } else {
        2 * YB - goal_y
    };
    let (mut dx, mut dy) = norm(goal_line(1 - side) - px, ay - py, UNIT);
    if (dx, dy) == (0, 0) {
        return (px, py, false);
    }
    let mut behind = (
        px - mul_div(dx, HIT_R + 4 * PX, UNIT),
        py - mul_div(dy, HIT_R + 4 * PX, UNIT),
    );
    // A puck lying against a rail: the spot behind it is off the
    // mallet's part of the table. Then the shot goes from the nearest
    // spot it can reach, as long as that still sends the puck forwards
    // (never back towards its own goal: then it waits, and TOO SLOW
    // takes the puck away). Without this, EASY and HARD hovered beside
    // a puck resting on a long rail until TOO SLOW.
    let (x0, x1) = zone_x(i, m.rules.four);
    let (y0, y1) = (YT + MALLET_R, YB - MALLET_R);
    let reach = (behind.0.clamp(x0, x1), behind.1.clamp(y0, y1));
    if go && reach != behind {
        let (ex, ey) = norm(px - reach.0, py - reach.1, UNIT);
        if ex * attack(side) > UNIT / 4 {
            (dx, dy) = (ex, ey);
            // A flat shot runs along the rail and comes back the same
            // way: two EASY computers passed it to and fro for ever
            // (the stall rule never fires, the puck crosses the centre).
            // Hit it into the near rail instead, so it comes off at an
            // angle.
            if dy.abs() < UNIT / 8 {
                let rail = if py < CY { -1 } else { 1 };
                (dx, dy) = norm(attack(side) * 2 * UNIT, rail * UNIT, UNIT);
            }
            behind = (
                px - mul_div(dx, HIT_R + 4 * PX, UNIT),
                py - mul_div(dy, HIT_R + 4 * PX, UNIT),
            );
            behind = (behind.0.clamp(x0, x1), behind.1.clamp(y0, y1));
        }
    }
    if !go {
        return (behind.0, behind.1, false);
    }
    // Where the mallet stands against the line of the shot.
    let (rx, ry) = (me.x - px, me.y - py);
    let along = mul_div(rx, dx, UNIT) + mul_div(ry, dy, UNIT);
    let across = mul_div(rx, -dy, UNIT) + mul_div(ry, dx, UNIT);
    if along < 0 && across.abs() < HIT_R / 2 {
        // Lined up: through the puck.
        let near = len(rx, ry) < 26 * PX;
        let through = (
            px + mul_div(dx, 12 * PX, UNIT),
            py + mul_div(dy, 12 * PX, UNIT),
        );
        return (through.0, through.1, sk.dash && near);
    }
    if along < HIT_R / 2 {
        return (behind.0, behind.1, false);
    }
    // On the wrong side: round the puck, the way it already stands.
    let s = if across >= 0 { 1 } else { -1 };
    let r = HIT_R + 6 * PX;
    (
        px - mul_div(dx, r, UNIT) + s * mul_div(-dy, r, UNIT),
        py - mul_div(dy, r, UNIT) + s * mul_div(dx, r, UNIT),
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::{Ev, Length, Rules};

    /// Computer against computer for `ticks`: the goals of each side.
    fn duel(left: Level, right: Level, seed: u32, ticks: u32) -> [u32; 2] {
        let mut m = Match::new(
            Rules {
                length: Length::First11,
                level: left,
                ..Rules::DEFAULT
            },
            0,
            seed,
        );
        let mut goals = [0u32; 2];
        for _ in 0..ticks {
            let mut its = [Intent::Stay; 4];
            m.rules.level = left;
            its[0] = intent(&mut m, 0);
            m.rules.level = right;
            its[1] = intent(&mut m, 1);
            m.tick(&its);
            while let Some(e) = m.take_event() {
                if let Ev::Goal { side } = e {
                    goals[usize::from(side)] += 1;
                }
            }
            if m.phase == Phase::Over {
                m = Match::new(m.rules, 0, m.rng.0);
            }
        }
        goals
    }

    #[test]
    fn computers_score_and_each_level_beats_the_one_below() {
        let ten_min = 60 * 60 * 10;
        let g = duel(Level::Hard, Level::Easy, 11, ten_min);
        assert!(g[0] > 2 * g[1], "hard {} easy {}", g[0], g[1]);
        let g = duel(Level::Normal, Level::Hard, 11, ten_min);
        assert!(g[1] > g[0], "normal {} hard {}", g[0], g[1]);
        let g = duel(Level::Easy, Level::Normal, 11, ten_min);
        assert!(g[1] > g[0], "easy {} normal {}", g[0], g[1]);
        let g = duel(Level::Normal, Level::Normal, 5, ten_min);
        assert!(g[0] + g[1] > 10, "{g:?}");
    }

    #[test]
    fn the_guess_folds_off_the_rails() {
        let mut m = Match::new(Rules::DEFAULT, 0, 1);
        m.puck.x = CX;
        m.puck.y = YT + 10 * PX;
        m.puck.vx = -PX;
        m.puck.vy = -PX;
        let y = cross_y(&m, CX - 40 * PX, true).unwrap();
        assert!(y > YT + PUCK_R && y < YB - PUCK_R);
        assert_eq!(cross_y(&m, CX + 40 * PX, true), None);
    }

    /// Prints goals for each pairing (`cargo test -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn print_duels() {
        let ten_min = 60 * 60 * 10;
        for (a, b) in [
            (Level::Easy, Level::Easy),
            (Level::Normal, Level::Easy),
            (Level::Hard, Level::Easy),
            (Level::Hard, Level::Normal),
            (Level::Hard, Level::Hard),
        ] {
            // Four seeds, each pairing from both ends.
            let mut sum = [0u32; 2];
            for seed in 1..=4 {
                let g = duel(a, b, seed, ten_min);
                let h = duel(b, a, seed, ten_min);
                sum[0] += g[0] + h[1];
                sum[1] += g[1] + h[0];
            }
            std::println!(
                "{} vs {}: {} - {} in 80 minutes",
                a.name(),
                b.name(),
                sum[0],
                sum[1]
            );
        }
    }
}
