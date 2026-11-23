//! The computer's skaters. Each one decides every few ticks (fewer on
//! HARD) what to do, from the match as it stands, and keeps skating that
//! way in between. Chance comes from the match's own [`crate::fixed::Rng`],
//! so a match with the same input plays the same every time.
//!
//! The plan, by who has the puck:
//!
//! * **This skater**: skate for a spot in front of the other goal,
//!   steering round anyone in the way; shoot for the far side of the
//!   goalie when close enough; pass when crowded (sometimes).
//! * **Its team mate**: skate ahead on the other side, ready for a pass.
//! * **Nobody, or the other team**: the one nearer the puck chases it
//!   (and checks the carrier); the other stands between the puck and its
//!   own goal.

use crate::fixed::{PX, UNIT, len, norm};
use crate::play::{CX, CY, Intent, MOUTH, Match, Owner, Phase, YB, YT, attack, goal_x};

/// What computer skater `i` does this tick.
pub fn intent(m: &mut Match, i: usize) -> Intent {
    if m.phase != Phase::Live {
        m.sk[i].think = 0;
        return Intent::default();
    }
    let sk = m.skill();
    let s = m.sk[i];
    let t = i / 2;
    let dir = attack(t);
    let mut it = Intent {
        speed: sk.speed,
        ..Intent::default()
    };
    let deciding = s.think == 0;
    if deciding {
        m.sk[i].think = sk.think;
    } else {
        m.sk[i].think -= 1;
        (it.dx, it.dy) = s.want;
        return it;
    }

    let p = m.puck;
    let (tx, ty);
    if m.carrier() == Some(i) {
        // Attack: the goal, the spot in front of it, and the far side.
        let gx = goal_x(1 - t);
        let gk_y = m.gk[1 - t].y;
        let to_goal = len(gx - s.x, CY - s.y);
        let in_front = (gx - s.x) * dir > 10 * PX;
        let crowd = nearest_foe(m, i).filter(|&(_, d)| d < 12 * PX);
        if in_front && (to_goal < sk.shot_range || s.carry > 240) {
            let side = if gk_y > CY { -1 } else { 1 };
            let aim_y = CY + side * (MOUTH - 3 * PX) + m.rng.spread(sk.aim_err);
            it.shot = Some((gx - p.x, aim_y - p.y, sk.shot_speed));
        } else if crowd.is_some() && m.rng.one_in(sk.pass_odds) && open_mate(m, i) {
            it.act = true;
        }
        // Skate for a spot 34 pixels out, on this skater's side.
        tx = gx - dir * 34 * PX;
        ty = CY + (s.y - CY).clamp(-16 * PX, 16 * PX);
        let (dx, mut dy) = (tx - s.x, ty - s.y);
        // Round a skater in the way: sideways, away from it.
        if let Some((k, d)) = crowd {
            let o = m.sk[k];
            if (o.x - s.x) * dir > 0 && d < 16 * PX {
                let away = if o.y > s.y { -1 } else { 1 };
                dy += away * 3 * len(dx, dy) / 4;
            }
        }
        (it.dx, it.dy) = toward(dx, dy);
    } else if m.team_has_puck(t) {
        // Support: ahead of the carrier, on the other side of the ice.
        let (cx, cy) = match m.puck.owner {
            Owner::Skater(c) => (m.sk[usize::from(c)].x, m.sk[usize::from(c)].y),
            _ => (p.x, p.y),
        };
        tx = (cx + dir * 26 * PX).clamp(CX - 60 * PX, CX + 60 * PX);
        let mirror = CY - (cy - CY);
        ty = if (mirror - cy).abs() < 22 * PX {
            cy + if cy > CY { -26 * PX } else { 26 * PX }
        } else {
            mirror
        }
        .clamp(YT + 8 * PX, YB - 8 * PX);
        (it.dx, it.dy) = toward(tx - s.x, ty - s.y);
    } else {
        // Defend: the nearer one chases, the other covers the goal.
        let lead = sk.lead;
        let (px, py) = (p.x + p.vx * lead, p.y + p.vy * lead);
        let mate = m.sk[i ^ 1];
        let mine = len(px - s.x, py - s.y);
        let theirs = len(px - mate.x, py - mate.y);
        let chaser = mine < theirs || (mine == theirs && i.is_multiple_of(2));
        if chaser {
            (it.dx, it.dy) = toward(px - s.x, py - s.y);
            if let Owner::Skater(c) = p.owner {
                let c = usize::from(c);
                let d = len(m.sk[c].x - s.x, m.sk[c].y - s.y);
                if d < 10 * PX && s.cool == 0 && m.rng.one_in(sk.check_odds) {
                    it.act = true;
                }
            }
        } else {
            let gx = goal_x(t);
            tx = gx + (p.x - gx) * 2 / 5 + dir * 10 * PX;
            ty = CY + (p.y - CY) / 2;
            (it.dx, it.dy) = toward(tx - s.x, ty - s.y);
        }
    }
    m.sk[i].want = (it.dx, it.dy);
    it
}

/// The way to a point: full speed when far, slower when close, still
/// when there.
fn toward(dx: i32, dy: i32) -> (i32, i32) {
    let d = len(dx, dy);
    if d < 2 * PX {
        return (0, 0);
    }
    let speed = if d > 10 * PX {
        UNIT
    } else {
        UNIT * d / (10 * PX)
    };
    norm(dx, dy, speed.max(UNIT / 3))
}

/// The nearest skater of the other team, and how far it is.
fn nearest_foe(m: &Match, i: usize) -> Option<(usize, i32)> {
    let s = m.sk[i];
    let foes = if i / 2 == 0 { 2..4 } else { 0..2 };
    foes.map(|k| (k, len(m.sk[k].x - s.x, m.sk[k].y - s.y)))
        .min_by_key(|&(_, d)| d)
}

/// Whether skater `i`'s team mate has no other skater close to it.
fn open_mate(m: &Match, i: usize) -> bool {
    let mate = m.sk[i ^ 1];
    let foes = if i / 2 == 0 { 2..4 } else { 0..2 };
    foes.into_iter()
        .all(|k| len(m.sk[k].x - mate.x, m.sk[k].y - mate.y) > 12 * PX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::play::{FACEOFF_TICKS, Length, Level};

    /// Four computer skaters, `secs` seconds: the goals scored.
    fn play(level: Level, seed: u32, secs: u32) -> [u8; 2] {
        let mut m = Match::new(Length::Min3, level, false, seed);
        for _ in 0..secs * 60 {
            let mut its = [Intent::default(); 4];
            for (i, it) in its.iter_mut().enumerate() {
                *it = intent(&mut m, i);
            }
            m.tick(&its);
            while m.take_event().is_some() {}
            if m.phase == Phase::Over {
                break;
            }
        }
        m.score
    }

    #[test]
    fn computer_teams_score_on_both_levels() {
        for level in [Level::Easy, Level::Hard] {
            let mut total = 0;
            for seed in 1..4 {
                let s = play(level, seed, 180);
                total += u32::from(s[0]) + u32::from(s[1]);
            }
            assert!(total >= 3, "{level:?}: {total} goals in 9 minutes");
        }
    }

    #[test]
    fn nobody_moves_during_the_face_off() {
        let mut m = Match::new(Length::Min2, Level::Hard, false, 5);
        for _ in 0..FACEOFF_TICKS - 1 {
            for i in 0..4 {
                assert_eq!(intent(&mut m, i), Intent::default());
            }
            m.tick(&[Intent::default(); 4]);
        }
    }
}

#[cfg(test)]
mod stats {
    extern crate std;
    use super::*;
    use crate::play::{Ev, Length, Level};

    #[test]
    #[ignore]
    fn print_stats() {
        for level in [Level::Easy, Level::Hard] {
            for seed in 1..6 {
                let mut m = Match::new(Length::Min3, level, false, seed);
                let (mut shots, mut saves, mut passes, mut steals, mut takes) = (0, 0, 0, 0, 0);
                let mut pos = [0u32; 4];
                for _ in 0..200 * 60 {
                    let mut its = [Intent::default(); 4];
                    for (i, it) in its.iter_mut().enumerate() {
                        *it = intent(&mut m, i);
                    }
                    m.tick(&its);
                    if let Some(c) = m.carrier() {
                        pos[c] += 1;
                    }
                    while let Some(e) = m.take_event() {
                        match e {
                            Ev::Shot { .. } => shots += 1,
                            Ev::Save { .. } => saves += 1,
                            Ev::Pass { .. } => passes += 1,
                            Ev::Steal { .. } => steals += 1,
                            Ev::Take { .. } => takes += 1,
                            _ => {}
                        }
                    }
                }
                std::println!(
                    "{level:?} seed {seed}: score {:?} shots {shots} saves {saves} passes {passes} steals {steals} takes {takes} carry {pos:?} phase {:?}",
                    m.score,
                    m.phase
                );
            }
        }
    }
}
