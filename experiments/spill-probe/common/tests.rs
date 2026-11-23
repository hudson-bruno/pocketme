// The probe on the PC: included by spill/tests/probe.rs and
// spillmax/tests/probe.rs after `use <crate> as probe;`.
//
// On the PC the functions lie wherever the PC put them, so `below` and
// `above` mean nothing here; only the sums are checked.

use game_kit::testing::TestConsole;

#[test]
fn the_run_time_sum_is_the_compilers_sum() {
    let c = probe::self_check();
    assert_eq!(c.fns as usize, probe::PAD);
    assert_eq!(c.below + c.above, c.fns);
    assert_eq!(c.sum, probe::EXPECTED);
    assert!(c.pass);
}

#[test]
fn every_bulk_function_is_its_own() {
    // Each table entry gives the formula's value for its own K, and no
    // two give the same: a call that lands in a wrong function, or two
    // functions merged into one, changes the sum.
    let mut seen = std::collections::BTreeSet::new();
    for (k, f) in probe::TABLE.iter().enumerate() {
        let v = f(probe::SEED);
        assert_eq!(v, probe::mix(k as u32, probe::SEED), "pad::<{k}>");
        assert!(seen.insert(v), "pad::<{k}> gives the same as another");
    }
}

#[test]
fn a_wrong_table_fails_the_check() {
    let mut t = probe::TABLE;
    t.swap(0, probe::PAD - 1);
    assert!(!probe::run_table(&t, probe::SEED).pass);
    assert!(!probe::run_table(&probe::TABLE[1..], probe::SEED).pass);
    assert!(!probe::run_table(&probe::TABLE, probe::SEED ^ 1).pass);
}

#[test]
fn the_probe_passes_and_its_data_holds() {
    // SAFETY: the game's own entry function, made by `game_kit::game!`.
    let mut c = unsafe { TestConsole::new(probe::game_entry) };
    c.run_ms(2_500);
    // The record holds each call's log text quoted, with `\n` between
    // lines: split it back into lines.
    let logs: Vec<&str> = c
        .log_lines()
        .into_iter()
        .flat_map(|l| l.trim_matches('"').split("\\n"))
        .filter(|l| !l.is_empty())
        .collect();
    let checks: Vec<&str> = logs
        .iter()
        .copied()
        .filter(|l| l.starts_with("spill: fns="))
        .collect();
    // At the start and once a second.
    assert!(checks.len() >= 3, "{logs:?}");
    for l in &checks {
        assert!(l.contains(" check=PASS "), "{l}");
        assert!(l.contains(" data=PASS bad_words=0 fails=0"), "{l}");
        assert!(
            l.contains(&format!("fns={} ", probe::PAD)),
            "{l}: want fns={}",
            probe::PAD
        );
    }
    assert!(
        logs.iter().any(|l| l.starts_with("spill phase=A fn=0x")),
        "{logs:?}"
    );
    assert!(c.running());
}

#[test]
fn the_data_check_finds_a_changed_word() {
    // Not through the game: the buffer's own check, on a copy of its
    // rules (the game's buffer is shared by every test in this file).
    assert_ne!(probe::pattern(1, 7), probe::pattern(2, 7));
    assert_ne!(probe::pattern(1, 7), probe::pattern(1, 8));
}
