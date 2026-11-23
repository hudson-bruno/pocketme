//! Lab: a stand-in for a game's update ("burn").
//!
//! It reads and writes 4 KB of its own memory, round and round, for a
//! set time. So it keeps a core busy AND uses the memory bus, like a
//! real game's update would. Each core has its own 4 KB.

use core::cell::UnsafeCell;

use core::sync::atomic::{AtomicU32, Ordering::Relaxed};

use crate::lcd::{CPU_MHZ, cycles};

/// Second reader's check: how much work the stand-in really got done.
/// The stand-in runs for a set TIME, so if the other core slows its
/// memory down it does less WORK in that time, and its time alone would
/// not show it. Per core: rounds of 64 memory steps, and the time spent.
static ROUNDS: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];
static BURN_US: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];

/// Rounds per millisecond on each core since the last call (then zero).
/// 0 when that core did no stand-in work.
pub fn take_rate() -> [u32; 2] {
    let mut out = [0; 2];
    for (k, o) in out.iter_mut().enumerate() {
        let r = ROUNDS[k].swap(0, Relaxed);
        let us = BURN_US[k].swap(0, Relaxed);
        if us >= 1000 {
            *o = (u64::from(r) * 1000 / u64::from(us)) as u32;
        }
    }
    out
}

struct Work(UnsafeCell<[u32; 1024]>);
// SAFETY: each core uses only its own buffer (see `burn`).
unsafe impl Sync for Work {}

static WORK: [Work; 2] = [
    Work(UnsafeCell::new([0; 1024])),
    Work(UnsafeCell::new([0; 1024])),
];

/// Works for `us` microseconds on this core's 4 KB. Returns a number so
/// that the compiler cannot leave the work out.
pub fn burn(us: u32) -> u32 {
    let core = esp_hal::system::Cpu::current() as usize;
    // SAFETY: each core uses only its own buffer, and only one burn at a
    // time runs on a core (it is never called from an interrupt).
    let buf = unsafe { &mut *WORK[core & 1].0.get() };
    let span = us * CPU_MHZ;
    let start = cycles();
    let mut x = 0x1234_5678u32;
    let mut i = 0usize;
    let mut rounds = 0u32;
    while cycles().wrapping_sub(start) < span {
        rounds += 1;
        for _ in 0..64 {
            let v = buf[i & 1023];
            x = x.rotate_left(5) ^ v.wrapping_add(i as u32);
            buf[i & 1023] = x;
            i = i.wrapping_add(7);
        }
    }
    ROUNDS[core & 1].fetch_add(rounds, Relaxed);
    BURN_US[core & 1].fetch_add(cycles().wrapping_sub(start) / CPU_MHZ, Relaxed);
    x
}
