//! Where the rivals lose their shields (a look for the person tuning).
use camber::race::{Class, Mode, Race, SHIPS};
use camber::track::TRACKS;

#[test]
#[ignore]
fn where_shields_go() {
    for track in 0..TRACKS.len() as u8 {
        let mut r = Race::new();
        r.setup(track, Class::Brisk, Mode::Race, 0);
        let tr = r.tr();
        let mut loss = vec![0i32; tr.n()];
        let mut bump = 0i32;
        for _ in 0..60 * 200 {
            let before: Vec<i16> = r.ships.iter().map(|s| s.shield).collect();
            let hits: Vec<u8> = r.ships.iter().map(|s| s.hit).collect();
            r.step(None);
            while r.take_ev().is_some() {}
            for i in 0..SHIPS {
                let d = i32::from(before[i]) - i32::from(r.ships[i].shield);
                if d > 0 {
                    let sh = &r.ships[i];
                    let half = tr.at(sh.s).half * 256;
                    if sh.x.abs() > half - 65536 * 2 {
                        loss[tr.index(sh.s)] += d;
                    } else {
                        bump += d;
                    }
                }
                let _ = hits[i];
            }
            if r.ships.iter().all(|s| s.finished()) {
                break;
            }
        }
        let mut v: Vec<(i32, usize)> = loss
            .iter()
            .enumerate()
            .map(|(i, &l)| (l, i))
            .filter(|x| x.0 > 0)
            .collect();
        v.sort_by(|a, b| b.cmp(a));
        eprintln!(
            "{}: bumps {} walls total {} worst {:?}",
            tr.name,
            bump,
            loss.iter().sum::<i32>(),
            &v[..v.len().min(12)]
        );
        for &(_, j) in v.iter().take(6) {
            let s = tr.seg(j);
            eprintln!(
                "  seg {j}: yaw {} pitch {} bank {} half {} (1/16 m) kind {} line {} fast {} next-bend {}",
                s.yaw,
                s.pitch,
                s.bank,
                s.half,
                s.kind,
                s.line,
                s.fast,
                camber::fixed::turn(s.yaw, tr.seg(j + 1).yaw)
            );
        }
    }
}

/// The tracks' lengths (`--ignored --nocapture`).
#[test]
#[ignore]
fn track_lengths() {
    for t in camber::track::TRACKS.iter() {
        eprintln!(
            "{}: {} segments, {} m",
            t.name,
            t.n(),
            t.len() / camber::track::FM
        );
    }
}
