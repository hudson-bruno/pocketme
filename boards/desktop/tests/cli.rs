//! Tests of the `pocketme-sim` program itself, with no window: mistakes
//! give a clear message and a failing exit code, never a crash, and the
//! same run prints the same fingerprints every time.

use std::path::PathBuf;
use std::process::{Command, Output};

/// A fresh, empty folder for one test, next to the test build.
fn folder(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("cli")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Runs the program with these options in `dir`. With the boot
/// animation built in, it is OFF (these tests are about the rest; the
/// boot has its own below).
fn sim(dir: &PathBuf, args: &[&str]) -> (Output, String, String) {
    let boot: &[&str] = if cfg!(feature = "boot-gather") {
        &["--boot", "off"]
    } else {
        &[]
    };
    sim_as_is(dir, &[boot, args].concat())
}

/// Runs the program with exactly these options (and `--quiet`).
fn sim_as_is(dir: &PathBuf, args: &[&str]) -> (Output, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_pocketme-sim"))
        .current_dir(dir)
        .arg("--quiet")
        .args(args)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(!err.contains("panicked"), "it crashed: {err}");
    (out, text, err)
}

#[test]
fn help_lists_the_options_and_every_key() {
    let dir = folder("help");
    let (out, text, _) = sim(&dir, &["--help"]);
    assert!(out.status.success());
    for word in [
        "--frames",
        "--script",
        "--window",
        "--close-after",
        "--card",
        "--boot",
    ] {
        assert!(text.contains(word), "{word} missing from --help");
    }
    let keymap = &desktop_board::keymap::KEYS;
    let player2 = &desktop_board::keymap::PLAYER2_KEYS;
    for (key, _, label) in keymap.iter().chain(player2.iter()) {
        assert!(text.contains(key) && text.contains(label), "{key}");
    }
}

#[test]
fn the_readme_lists_every_key_and_button_name() {
    let readme = include_str!("../README.md");
    let keys = desktop_board::keymap::KEYS.iter().map(|k| k.0);
    let sim_keys = desktop_board::keymap::SIM_KEYS.iter().map(|k| k.0);
    let player2 = desktop_board::keymap::PLAYER2_KEYS.iter().map(|k| k.0);
    for key in keys.chain(sim_keys).chain(player2) {
        assert!(
            readme.contains(&format!("`{key}`")),
            "key {key} missing from README.md"
        );
    }
    for (name, _) in desktop_board::keymap::BUTTONS {
        assert!(
            readme.contains(&format!("`{name}`")),
            "button {name} missing from README.md"
        );
    }
}

#[test]
fn shots_and_fingerprints_are_the_same_every_run() {
    let dir = folder("repeat");
    std::fs::write(
        dir.join("s.txt"),
        "2 press south\n3 release south\n5 shot\n",
    )
    .unwrap();
    let args = [
        "--frames", "8", "--script", "s.txt", "--shot", "0,last", "--prints",
    ];
    let (out, first, err) = sim(&dir, &args);
    assert!(out.status.success(), "{err}");
    let png = std::fs::read(dir.join("shots/frame-00005.png")).unwrap();
    for f in ["frame-00000.png", "frame-00007.png"] {
        assert!(dir.join("shots").join(f).exists(), "{f}");
    }
    let (_, second, _) = sim(&dir, &args);
    let prints = |t: &str| -> Vec<String> {
        t.lines()
            .filter(|l| l.contains("fingerprint"))
            .map(|l| l.split(" in ").next().unwrap().to_string()) // drop the time
            .collect()
    };
    assert_eq!(prints(&first).len(), 8 + 1);
    assert_eq!(prints(&first), prints(&second));
    assert_eq!(
        png,
        std::fs::read(dir.join("shots/frame-00005.png")).unwrap()
    );
}

#[test]
fn a_shot_after_the_last_frame_is_an_error() {
    let dir = folder("late-shot");
    let (out, _, err) = sim(&dir, &["--frames", "10", "--shot", "3,50"]);
    assert!(!out.status.success());
    assert!(err.contains("--shot 50") && err.contains("0 to 9"), "{err}");
}

#[test]
fn script_lines_after_the_end_are_an_error() {
    let dir = folder("late-script");
    std::fs::write(dir.join("s.txt"), "0 press plus\n100 press south\n").unwrap();
    let (out, _, err) = sim(&dir, &["--frames", "10", "--script", "s.txt"]);
    assert!(!out.status.success());
    assert!(
        err.contains("1 script line(s) never ran") && err.contains("--frames 101"),
        "{err}"
    );
    let (out, _, err) = sim(&dir, &["--frames", "101", "--script", "s.txt"]);
    assert!(out.status.success(), "{err}");
}

#[test]
fn a_script_with_a_mistake_names_the_line() {
    let dir = folder("bad-script");
    std::fs::write(dir.join("s.txt"), "0 press plus\n\n3 pres south\n").unwrap();
    let (out, _, err) = sim(&dir, &["--script", "s.txt"]);
    assert!(!out.status.success());
    assert!(
        err.contains("script line 3") && err.contains("`pres`"),
        "{err}"
    );
}

#[test]
fn a_picture_that_cannot_be_saved_fails_the_run() {
    let dir = folder("cannot-save");
    // A file where the folder should be: the folder cannot be made.
    std::fs::write(dir.join("blocked"), "").unwrap();
    let (out, _, err) = sim(
        &dir,
        &["--frames", "2", "--shot", "0", "--out", "blocked/x"],
    );
    assert!(!out.status.success());
    assert!(err.contains("could not save"), "{err}");
}

#[test]
fn wrong_options_are_refused() {
    let dir = folder("options");
    for (args, says) in [
        (&["--jump"][..], "unknown option"),
        (&["--frames"][..], "needs a value"),
        (&["--frames", "ten"][..], "whole number"),
        (&["--fps", "25"][..], "30 or 60"),
        (&["--scale", "9"][..], "1 to 6"),
        (&["--close-after", "60"][..], "0.5 to 5"),
        (&["--card", "no/such/folder"][..], "no such folder"),
        (&["--shot", "x"][..], "frame numbers"),
    ] {
        let (out, _, err) = sim(&dir, args);
        assert!(!out.status.success(), "{args:?}");
        assert!(err.contains(says), "{args:?}: {err}");
    }
}

#[test]
fn the_store_is_kept_in_a_file() {
    let dir = folder("store");
    let (out, _, err) = sim(&dir, &["--frames", "1", "--store", "s.bin"]);
    assert!(out.status.success(), "{err}");
    std::fs::write(dir.join("bad.bin"), [0u8; 3]).unwrap();
    let (out, _, err) = sim(&dir, &["--frames", "1", "--store", "bad.bin"]);
    assert!(!out.status.success());
    assert!(err.contains("3 bytes"), "{err}");
}

#[test]
fn the_brightness_survives_a_restart_with_a_store_file() {
    let dir = folder("store-brightness");
    // One step down at frame 63; saved 2 s later.
    std::fs::write(
        dir.join("s.txt"),
        "60 press home\n61 release home\n63 press left\n64 release left\n",
    )
    .unwrap();
    let (out, text, err) = sim(
        &dir,
        &["--frames", "200", "--script", "s.txt", "--store", "s.bin"],
    );
    assert!(out.status.success(), "{err}");
    assert!(text.contains("; backlight 156; "), "{text}");
    // A second run with no script starts at that brightness.
    let (out, text, err) = sim(&dir, &["--frames", "3", "--store", "s.bin", "--trace"]);
    assert!(out.status.success(), "{err}");
    assert!(text.contains("frame     0 backlight 156"), "{text}");
    // Without the file it is the default again.
    let (_, text, _) = sim(&dir, &["--frames", "3", "--trace"]);
    assert!(text.contains("frame     0 backlight 255"), "{text}");
}

#[test]
fn the_summary_and_the_trace_show_the_backlight() {
    let dir = folder("backlight");
    std::fs::write(
        dir.join("s.txt"),
        "60 press home\n61 release home\n63 press left\n64 release left\n64 backlight 156\n",
    )
    .unwrap();
    let (out, text, err) = sim(&dir, &["--frames", "70", "--script", "s.txt", "--trace"]);
    assert!(out.status.success(), "{err}");
    let lights: Vec<&str> = text.lines().filter(|l| l.contains(" backlight ")).collect();
    assert_eq!(lights.len(), 3, "{text}");
    assert_eq!(lights[0], "frame     0 backlight 255");
    assert_eq!(lights[1], "frame    63 backlight 156");
    assert!(
        lights[2].contains("; backlight 156; fingerprint "),
        "{text}"
    );
    // A wrong value fails the run and says why.
    std::fs::write(dir.join("w.txt"), "5 backlight 16\n").unwrap();
    let (out, _, err) = sim(&dir, &["--frames", "10", "--script", "w.txt"]);
    assert!(!out.status.success());
    assert!(
        err.contains("frame 5: the backlight is 255, the script expected 16"),
        "{err}"
    );
}

/// `--boot`: FULL plays the whole animation (home after 2.72 s, frame
/// 82 at 30 a second), SHORT under a second, OFF none; the setting is
/// saved with the store file. Without the feature `--boot` is refused.
#[test]
fn the_boot_option_picks_how_the_boot_plays() {
    let dir = folder("boot");
    if !cfg!(feature = "boot-gather") {
        let (out, _, err) = sim_as_is(&dir, &["--boot", "full"]);
        assert!(!out.status.success());
        assert!(err.contains("no boot animation"), "{err}");
        return;
    }
    let home_at = |args: &[&str]| -> u64 {
        let (out, text, err) = sim_as_is(&dir, &[args, &["--frames", "100", "--prints"]].concat());
        assert!(out.status.success(), "{err}");
        // The frame from which the picture no longer changes.
        let prints: Vec<&str> = text
            .lines()
            .filter_map(|l| l.split("fingerprint ").nth(1))
            .collect();
        let last = prints.last().unwrap();
        prints.iter().position(|p| p == last).unwrap() as u64
    };
    let full = home_at(&[]);
    assert_eq!(full, home_at(&["--boot", "full"]), "FULL is the default");
    let short = home_at(&["--boot", "short"]);
    let off = home_at(&["--boot", "off"]);
    assert!((75..=83).contains(&full), "FULL still at frame {full}");
    assert!((20..=30).contains(&short), "SHORT still at frame {short}");
    assert!(off <= 2, "OFF still at frame {off}");
    let (out, _, err) = sim_as_is(&dir, &["--boot", "long"]);
    assert!(!out.status.success());
    assert!(err.contains("--boot must be full, short or off"), "{err}");
}
