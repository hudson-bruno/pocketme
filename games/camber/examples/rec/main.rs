//! `rec`: the PC tool for CAMBER's measuring recordings (feature `rec`).
//!
//! ```text
//! cargo run --offline --features rec --example rec -- <capture.txt> [--out DIR]
//! ```
//!
//! Reads captured log text (`boardctl log` output; any other lines,
//! prefixes and timestamps around the recording's lines are skipped),
//! finds the recording, checks its numbering and CRC, replays it and
//! races the rivals against it as a ghost. Prints all of it; with
//! `--out` also writes `recording.bin` (the runs' bytes), `head.txt`,
//! `runs.txt`, `replay.txt` and `ghost.txt` there.
//!
//! Exit status: 0 the recording is good and the replay reproduces the
//! race (or it is cut), 2 refused (damaged or no recording), 3 the
//! replay gives another race than the console's, 1 bad use.
//!
//! An example and not a binary on purpose: the chip build makes the
//! crate a static library (`cargo rustc --crate-type staticlib`), which
//! leaves examples out, and a std program must never reach it.

#[path = "capture.rs"]
mod capture;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut file = None;
    let mut out = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--out" => out = it.next().cloned(),
            "-h" | "--help" => {
                println!("usage: rec <capture.txt> [--out DIR]");
                return ExitCode::SUCCESS;
            }
            _ if file.is_none() => file = Some(a.clone()),
            _ => {
                eprintln!("rec: unexpected argument {a:?}");
                return ExitCode::from(1);
            }
        }
    }
    let Some(file) = file else {
        eprintln!("usage: rec <capture.txt> [--out DIR]");
        return ExitCode::from(1);
    };
    let bytes = match std::fs::read(&file) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("rec: cannot read {file}: {e}");
            return ExitCode::from(1);
        }
    };
    let text = String::from_utf8_lossy(&bytes);
    let found = match capture::decode(&text) {
        Ok(f) => f,
        Err(e) => {
            println!("{e}");
            println!("CRC: NOT MATCHED (recording refused)");
            return ExitCode::from(2);
        }
    };
    for (id, problems) in &found.sends {
        if problems.is_empty() {
            println!("send of recording {id:04x}: complete, CRC matched");
        } else {
            for p in problems {
                println!("send of recording {id:04x}: DAMAGED, skipped: {p}");
            }
        }
    }
    let r = &found.rec;
    let (_, played) = capture::replay(r, |_| {});
    let sum = capture::summary(r, &played);
    print!("{sum}");
    println!("CRC: MATCHED");
    let gt = capture::measures_text(r);
    print!("{gt}");
    if let Some(dir) = out {
        let dir = std::path::Path::new(&dir);
        let write = |name: &str, data: &[u8]| {
            if let Err(e) =
                std::fs::create_dir_all(dir).and_then(|_| std::fs::write(dir.join(name), data))
            {
                eprintln!("rec: cannot write {name}: {e}");
            }
        };
        write("recording.bin", &r.data);
        write(
            "head.txt",
            format!("{:#?}\nfinish {}\nplace {}\n", r.head, r.finish, r.place).as_bytes(),
        );
        write("runs.txt", capture::runs_text(r).as_bytes());
        write("replay.txt", sum.as_bytes());
        write("ghost.txt", gt.as_bytes());
    }
    if played.matches || r.head.cut {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(3)
    }
}
