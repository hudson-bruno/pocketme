//! The fixed step's clock (the spec's section 0).

use drive3d::clock::Clock;
use drive3d::tune::{MAX_STEPS, STEP_US};

#[test]
fn the_store_starts_half_full() {
    // 33,333 / 2 = 16,666: a first update of 16,667 us already runs a
    // step (16,666 + 16,667 = 33,333).
    let mut c = Clock::new();
    assert_eq!(c.store, 16_666);
    assert_eq!(c.add(16_666), 0);
    let mut c = Clock::new();
    assert_eq!(c.add(16_667), 1);
    assert_eq!(c.store, 0);
}

#[test]
fn odd_deltas_run_one_step_per_33333_us_in_the_long_run() {
    // Deltas of 7, 19, 31 and 41 ms going round: 98 ms each round of
    // four, so the clock runs 0, 1 or 2 steps on each. 100 rounds are
    // 9,800,000 us; with the half step at the start, (16,666 +
    // 9,800,000) / 33,333 = 294.5: 294 steps.
    let mut c = Clock::new();
    let deltas = [7_000u32, 19_000, 31_000, 41_000];
    let mut steps = 0u32;
    let mut total = 0u64;
    for r in 0..100 {
        for d in deltas {
            let n = c.add(d);
            assert!(n <= MAX_STEPS);
            steps += u32::from(n);
            total += u64::from(d);
            // Never more than two steps' time waits.
            assert!(c.store <= 2 * STEP_US, "round {r}");
        }
    }
    let want = (16_666 + total) / u64::from(STEP_US);
    assert_eq!(u64::from(steps), want);
    assert_eq!(steps, 294);
}

#[test]
fn a_long_stall_runs_at_most_two_steps_and_drops_the_rest() {
    // One second at once: 2 steps now; the backlog is dropped and the
    // store is half a step again, so nothing is caught up later and
    // the next steady updates run one step each.
    //
    // Changed in the final round (review rules F1): this test used to
    // say the store keeps two steps' time (66,666) and runs them on the
    // next update. That kept a whole number of steps, lost the
    // half-step phase for good, and made jittered updates run 0, 2, 0,
    // 2 steps afterwards; the test held the faulty rule in place.
    let mut c = Clock::new();
    assert_eq!(c.add(1_000_000), 2);
    assert_eq!(c.store, STEP_US / 2);
    assert_eq!(c.add(0), 0);
    assert_eq!(c.add(STEP_US), 1);
    assert_eq!(c.store, STEP_US / 2);
}

#[test]
fn sixty_updates_a_second_run_thirty_steps() {
    // 16,667 us each: steps on every other update.
    let mut c = Clock::new();
    let n: u32 = (0..60).map(|_| u32::from(c.add(16_667))).sum();
    assert_eq!(n, 30);
}
