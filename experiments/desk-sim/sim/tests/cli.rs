//! Tests of the `desk-sim headless` command itself: mistakes give a clear
//! message and a failing exit code, never a crash, and nothing that was
//! asked for is silently skipped.

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

/// Runs `desk-sim headless` with these options in `dir`.
fn headless(dir: &PathBuf, args: &[&str]) -> (Output, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_desk-sim"))
        .current_dir(dir)
        .arg("headless")
        .arg("--quiet")
        .args(args)
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(!err.contains("panicked"), "it crashed: {err}");
    (out, err)
}

#[test]
fn a_shot_after_the_last_frame_is_an_error() {
    let dir = folder("late-shot");
    let (out, err) = headless(&dir, &["--frames", "10", "--shot", "3,50"]);
    assert!(!out.status.success());
    assert!(err.contains("--shot 50") && err.contains("0 to 9"), "{err}");
    // `last` is always fine.
    let (out, _) = headless(&dir, &["--frames", "10", "--shot", "last"]);
    assert!(out.status.success());
    assert!(dir.join("shots/frame-00009.png").exists());
}

#[test]
fn script_lines_after_the_end_give_a_warning() {
    let dir = folder("late-script");
    std::fs::write(dir.join("s.txt"), "0 press start\n100 press a\n").unwrap();
    let (out, err) = headless(&dir, &["--frames", "10", "--script", "s.txt"]);
    assert!(out.status.success());
    assert!(
        err.contains("1 script line(s) were not used") && err.contains("--frames 101"),
        "{err}"
    );
    // No warning when the whole script is used.
    let (out, err) = headless(&dir, &["--frames", "101", "--script", "s.txt"]);
    assert!(out.status.success());
    assert!(!err.contains("warning"), "{err}");
}

#[test]
fn a_picture_that_cannot_be_saved_fails_the_run() {
    let dir = folder("cannot-save");
    // A file where the folder should be: the folder cannot be made.
    std::fs::write(dir.join("blocked"), "").unwrap();
    let (out, err) = headless(
        &dir,
        &["--frames", "2", "--shot", "0", "--out", "blocked/x"],
    );
    assert!(!out.status.success());
    assert!(err.contains("could not save"), "{err}");
}

#[test]
fn a_script_with_a_mistake_names_the_line() {
    let dir = folder("bad-script");
    std::fs::write(dir.join("s.txt"), "0 press start\n\n3 pres a\n").unwrap();
    let (out, err) = headless(&dir, &["--script", "s.txt"]);
    assert!(!out.status.success());
    assert!(
        err.contains("script line 3") && err.contains("`pres`"),
        "{err}"
    );
    let (out, err) = headless(&dir, &["--script", "missing.txt"]);
    assert!(!out.status.success());
    assert!(err.contains("cannot read missing.txt"), "{err}");
}
