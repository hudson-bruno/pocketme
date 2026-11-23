//! Starting games: the game loader (crates/game-loader) on the chip's two
//! real game slots.
//!
//! - At start-up [`GameSlots::check`] compares the slots the linker made
//!   (game-slots.x, from game_api::layout) with the loader's
//!   `Slots::CONSOLE` and logs one line. If they differ, no game starts.
//! - [`ChipSlots`] puts the bytes the loader hands over into the slots:
//!   the code slot is executable RAM, where ONLY whole, aligned 32-bit
//!   words may be read or written (a byte access stops the chip); the
//!   data slot is ordinary RAM.
//! - [`GameSlots::start`] loads `/CONSOLE/GAMES/<folder>/GAME.BIN`, calls
//!   the game's entry and reads its table. The `Loaded` and the entry
//!   address live only inside that call.
//! - [`GameSlots::end`] wipes both slots (all zero), so nothing of a game
//!   is left after it, and a stale call into the code slot meets zeros:
//!   on the ESP32's Xtensa core the all-zero word is the ILL instruction,
//!   so it stops at once instead of running old code.
//! - [`GameSlots::lend`] lends the data slot to the console while no game
//!   is loaded: the home screen keeps its game list and tile pictures
//!   there. A start writes the game over them (and the wipes clear them),
//!   so home reads them from the card again after every game.
//! - Stage 1, the spill: a game's code may run on past the code slot
//!   (game_api::layout::CODE_SPILL_ADDR, 0x400A0000). That is SRAM1 seen
//!   from the instruction bus: the same RAM as the TOP of the data slot,
//!   in reverse word order (layout::sram1_iram_to_dram, quoted from
//!   ESP-IDF; seen to hold on the CYD, 2026-11-22). Like the code slot, it is
//!   only read and written as whole aligned 32-bit words. At start-up
//!   [`GameSlots::check`] tests that order ([`spill_mirror_check`]); if it
//!   does not hold, the spill is set to 0: games that need it are refused
//!   ("GAME CODE TOO BIG"), every other game runs as before.
//! - Game flash parts (feature `game-flash`, README "Game flash parts"):
//!   a game file with a flash part ("GAMF") names code and read-only data
//!   that lie not in the file but in this firmware's flash, in two
//!   windows build.rs reserved at game_api::layout's addresses. At
//!   start-up [`GameSlots::check`] compares the linked windows with the
//!   layout (one log line). Before a load writes anything,
//!   `ChipSlots::flash_part` finds the part among those this firmware
//!   holds (`HELD`) and computes the CRC-32s of the bytes as they lie in
//!   flash: the game runs only when both match the file's. The console
//!   never writes, erases or remaps that flash. Without the feature every
//!   such game is refused ("FLASH PART MISSING").
//!
//! All of the board's `unsafe` for games is in this file.

use core::sync::atomic::{AtomicBool, Ordering};

use board_api::game::{ConsoleApi, GameCallbacks, GameEntry};
use board_api::{Files, FsError, StartError};
use game_api::card;
#[cfg(feature = "game-flash")]
use game_api::{Crc32, FlashPartHeader, layout};
#[cfg(feature = "game-flash")]
use game_loader::FlashHeld;
use game_loader::{LoadError, Slots};
use log::{info, warn};

use crate::board::now_us;

unsafe extern "C" {
    // The two slots as game-slots.x placed them (NOLOAD sections that
    // reserve the whole slot).
    static _game_code_slot_start: u32;
    static _game_code_slot_end: u32;
    static _game_data_slot_start: u32;
    static _game_data_slot_end: u32;
    // The spill (NOLOAD, reserves the instruction addresses only).
    static _game_spill_start: u32;
    static _game_spill_end: u32;
}

#[cfg(feature = "game-flash")]
unsafe extern "C" {
    // The two flash windows as game-flash.x (build.rs `game_flash`)
    // placed them: start, and one past the end mark in the last word.
    static _game_flash_code_window: u32;
    static _game_flash_code_window_end: u32;
    static _game_flash_ro_window: u32;
    static _game_flash_ro_window_end: u32;
}

/// One game flash part this firmware holds (made by build.rs
/// `game_flash` from `<game files>/flash/<NAME>/`): where its two halves
/// lie and the CRC-32s of the bytes built in.
#[cfg(feature = "game-flash")]
#[derive(Clone, Copy)]
struct Held {
    game: &'static str,
    code_addr: u32,
    code_len: u32,
    code_crc: u32,
    ro_addr: u32,
    ro_len: u32,
    ro_crc: u32,
}

/// The parts themselves (statics in the windows) and `HELD`.
#[cfg(feature = "game-flash")]
mod held {
    use super::Held;
    include!(concat!(env!("OUT_DIR"), "/game_flash.rs"));
}

/// Whether the flash windows the linker made are game_api::layout's
/// (one log line either way). If not, every game with a flash part is
/// refused. Without the feature: no windows, one line saying so.
fn flash_windows_check() -> bool {
    #[cfg(feature = "game-flash")]
    {
        // black_box: as in `linked_slots`, the comparison is made at run
        // time, not folded away by the compiler.
        let code = core::hint::black_box((&raw const _game_flash_code_window) as u32);
        let code_end = core::hint::black_box((&raw const _game_flash_code_window_end) as u32);
        let ro = core::hint::black_box((&raw const _game_flash_ro_window) as u32);
        let ro_end = core::hint::black_box((&raw const _game_flash_ro_window_end) as u32);
        let ok = code == layout::FLASH_CODE_ADDR
            && code_end.wrapping_sub(code) == layout::FLASH_CODE_SIZE
            && ro == layout::FLASH_RO_ADDR
            && ro_end.wrapping_sub(ro) == layout::FLASH_RO_SIZE;
        if ok {
            info!(
                "game flash: code {code:#010x}+{}, data {ro:#010x}+{}, parts held: {}",
                layout::FLASH_CODE_SIZE,
                layout::FLASH_RO_SIZE,
                held::HELD.len()
            );
            for h in held::HELD {
                info!(
                    "game flash: {} code {}, data {} bytes",
                    h.game, h.code_len, h.ro_len
                );
            }
        } else {
            warn!(
                "game flash: windows {code:#010x} {ro:#010x} DIFFER from layout; flash parts refused"
            );
        }
        ok
    }
    #[cfg(not(feature = "game-flash"))]
    {
        info!("game flash: off; games with a flash part are refused");
        false
    }
}

/// The CRC-32 (game_api::Crc32, 0 for no bytes) of `len` bytes of a
/// flash window at `addr`, read as whole aligned 32-bit words.
#[cfg(feature = "game-flash")]
fn flash_crc(addr: u32, len: u32) -> u32 {
    let in_window =
        |start: u32, size: u32| addr >= start && len <= size && addr - start <= size - len;
    assert!(
        addr.is_multiple_of(4)
            && len.is_multiple_of(4)
            && (in_window(layout::FLASH_CODE_ADDR, layout::FLASH_CODE_SIZE)
                || in_window(layout::FLASH_RO_ADDR, layout::FLASH_RO_SIZE))
    );
    if len == 0 {
        return 0;
    }
    let mut crc = Crc32::new();
    for at in (addr..addr + len).step_by(4) {
        // SAFETY: an aligned 32-bit load inside one of the two flash
        // windows that game-flash.x reserved (checked above against
        // game_api::layout; at start-up the linked windows were found
        // equal to it, or this is never called). Both are mapped flash
        // for the whole program: the code window is read through the
        // instruction bus, which takes only whole aligned words (this is
        // one), the data window through the data bus. Only reads; nothing
        // in the program writes there.
        let word = unsafe { (at as *const u32).read_volatile() };
        crc.update(&word.to_le_bytes());
    }
    crc.value()
}

/// The slots as the linker placed them.
pub fn linked_slots() -> Slots {
    let code = (&raw const _game_code_slot_start) as u32;
    let code_end = (&raw const _game_code_slot_end) as u32;
    let data = (&raw const _game_data_slot_start) as u32;
    let data_end = (&raw const _game_data_slot_end) as u32;
    let spill = (&raw const _game_spill_start) as u32;
    let spill_end = (&raw const _game_spill_end) as u32;
    Slots {
        code_addr: code,
        code_size: code_end.wrapping_sub(code),
        // The loader puts spill code at `code_addr + offset`, so the
        // spill must follow the code slot directly (game-slots.x checks
        // it too). If not, a size no layout has, so the comparison with
        // `Slots::CONSOLE` fails and no game starts.
        //
        // black_box: the two are different statics to the compiler, so
        // it takes `spill == code_end` as always false and drops the
        // whole "slots ok" path (it did, in the first stage-1 build: every
        // game refused). Hidden from it, the test is made at run time.
        // boards/cyd/check.sh looks for that path's log lines in the
        // owner's builds.
        spill_size: if core::hint::black_box(spill) == core::hint::black_box(code_end) {
            spill_end.wrapping_sub(spill)
        } else {
            u32::MAX
        },
        data_addr: data,
        data_size: data_end.wrapping_sub(data),
    }
}

/// The console's own table for games. `abort` never returns.
static CHIP_CONSOLE_API: ConsoleApi = ConsoleApi {
    size: size_of::<ConsoleApi>() as u32,
    version: game_api::API_VERSION,
    abort: chip_abort,
};

/// A game ended itself for good (its panic handler, or a console table
/// it cannot use). There is no way back into the console yet (the
/// recovery point is a later round), so this goes the way of any panic
/// on this board: esp-backtrace prints the message and a backtrace to
/// the serial port, then the chip stops with interrupts off (esp-hal
/// turned every watchdog off at start). Only a reset brings it back.
extern "C-unwind" fn chip_abort(code: u32, detail: u32) -> ! {
    let what = match code {
        game_api::abort_code::PANIC => "panic, source line",
        game_api::abort_code::BAD_API => "console tables too old, detail",
        _ => "unknown reason, detail",
    };
    panic!("game aborted: {what} {detail} (code {code})")
}

/// The board's side of the slots: whether they can be used, and what the
/// last load and wipe did (for the log and `boardctl stats`).
pub struct GameSlots {
    /// The slots, when they match `Slots::CONSOLE`; `None` refuses games.
    slots: Option<Slots>,
    /// A game is open (between a good start and `end`).
    pub running: bool,
    /// The last start: what it wrote and how long it took.
    pub last: StartReport,
    /// Starts tried, games opened, wipes done.
    pub starts: u32,
    pub opened: u32,
    pub wipes: u32,
    /// The last wipe of both slots, in microseconds.
    pub wipe_us: u64,
    /// After the last wipe: spill words that still read non-zero from the
    /// instruction bus (0 is right; always 0 when `spill_read` is 0).
    pub spill_left: u32,
    /// Bytes of the spill read back after every wipe: the whole spill
    /// once the start-up mirror check's read step passed (also when its
    /// write step then failed and the spill was turned off, so words it
    /// left would be counted), 0 when the reads were wrong or there is no
    /// spill.
    spill_read: u32,
    /// When the last game was opened (for the time to its first frame).
    pub opened_at_us: Option<u64>,
    /// The flash windows are game_api::layout's (feature `game-flash`):
    /// games with a flash part may be checked; `false` refuses them.
    flash_ok: bool,
}

/// What one start did.
#[derive(Clone, Copy, Default)]
pub struct StartReport {
    /// Code words written, data bytes written, data bytes zeroed.
    pub code_words: u32,
    /// Of the code words, those past the code slot (in the spill).
    pub spill_words: u32,
    pub data_bytes: u32,
    pub zeroed: u32,
    /// Microseconds in the loader (card reads, checks, slot writes).
    pub load_us: u64,
    /// Microseconds in the game's entry and the table check.
    pub open_us: u64,
    /// Bytes of the file read.
    pub file_bytes: u32,
    /// Microseconds spent checking the game's flash part (0: none).
    pub flash_check_us: u64,
}

impl GameSlots {
    /// Compares the linked slots with `Slots::CONSOLE` (one log line),
    /// checks the spill's mirror order ([`spill_mirror_check`]; on a
    /// failure the spill is set to 0), then wipes both slots so they hold
    /// no game.
    ///
    /// Only the first call gets the slots: a second `GameSlots` refuses
    /// every game and lends nothing, so two owners can never write or
    /// lend the same RAM.
    pub fn check() -> Self {
        static TAKEN: AtomicBool = AtomicBool::new(false);
        if TAKEN.swap(true, Ordering::Relaxed) {
            warn!("game slots: already in use by another owner; games will not start here");
            return Self::unused();
        }
        let linked = linked_slots();
        let want = Slots::CONSOLE;
        let ok = linked == want;
        if ok {
            info!(
                "game slots ok: code {:#010x} + {} bytes, spill {:#010x} + {} bytes, data {:#010x} + {} bytes (linker = game_api::layout)",
                linked.code_addr,
                linked.code_size,
                linked.code_addr + linked.code_size,
                linked.spill_size,
                linked.data_addr,
                linked.data_size
            );
        } else {
            warn!(
                "game slots DIFFER, games will not start: linker code {:#010x} + {}, spill {}, data {:#010x} + {}; game_api::layout code {:#010x} + {}, spill {}, data {:#010x} + {}",
                linked.code_addr,
                linked.code_size,
                linked.spill_size,
                linked.data_addr,
                linked.data_size,
                want.code_addr,
                want.code_size,
                want.spill_size,
                want.data_addr,
                want.data_size
            );
        }
        let mut slots = linked;
        let mut spill_read = 0;
        if ok {
            let mirror = spill_mirror_check(&slots);
            if mirror != Err(MirrorFault::Reads) {
                spill_read = slots.spill_size;
            }
            if mirror.is_err() {
                // Games that fit the code slot still run; the loader
                // refuses the others before it writes anything.
                slots.spill_size = 0;
                warn!("game slots: spill OFF; games with code past the code slot will be refused");
            }
        }
        let flash_ok = flash_windows_check();
        let mut me = Self {
            slots: ok.then_some(slots),
            spill_read,
            flash_ok,
            ..Self::unused()
        };
        if ok {
            me.wipe();
            info!(
                "game slots wiped in {} us; spill words not zero after wipe: {}",
                me.wipe_us, me.spill_left
            );
        }
        me
    }

    /// No slots: refuses every game, lends nothing.
    fn unused() -> Self {
        Self {
            slots: None,
            running: false,
            last: StartReport::default(),
            starts: 0,
            opened: 0,
            wipes: 0,
            wipe_us: 0,
            spill_left: 0,
            spill_read: 0,
            opened_at_us: None,
            flash_ok: false,
        }
    }

    /// Loads and opens the game in `/CONSOLE/GAMES/<folder>/`.
    pub fn start<F: Files>(
        &mut self,
        files: &mut F,
        folder: &str,
    ) -> Result<GameCallbacks, StartError> {
        self.starts += 1;
        self.last = StartReport::default();
        let Some(slots) = self.slots else {
            warn!("game {folder}: refused, the game slots differ from game_api::layout");
            return Err(StartError::Refused("GAME SLOTS NOT READY"));
        };
        if self.running {
            // The console ends a game before it starts another; if it
            // ever did not, the old game's functions must not be kept.
            warn!("game {folder}: a game was still open; wiping its slots first");
            self.end();
        }
        let mut path_buf = [0u8; 64];
        let Some(path) = game_path(&mut path_buf, folder) else {
            warn!("game {folder}: the folder name is too long for a path");
            return Err(StartError::NotFound);
        };

        let mut chip = ChipSlots::new(slots, self.flash_ok);
        let t0 = now_us();
        let got = game_loader::load(files, path, &slots, &mut chip);
        let t1 = now_us();
        self.last.code_words = chip.code_words;
        self.last.spill_words = chip.spill_words;
        self.last.data_bytes = chip.data_bytes;
        self.last.zeroed = chip.zeroed;
        self.last.load_us = t1 - t0;
        self.last.flash_check_us = chip.flash_check_us;
        // The entry address, from the `Loaded` of this load only; the
        // `Loaded` itself ends here.
        let entry_addr = match got {
            Ok(loaded) => {
                self.last.file_bytes = loaded.stats().bytes;
                loaded.entry()
            }
            Err(e) => {
                warn!(
                    "game {folder}: {} ({e:?}); {} us; slots written: {} code words, {} data bytes",
                    e.notice(),
                    t1 - t0,
                    chip.code_words,
                    chip.data_bytes
                );
                // A load that failed half way left rubbish behind. Nothing
                // can run it (only a good load gives an entry), but the
                // slots are cleared anyway, so they hold a game or zeros.
                if chip.touched() {
                    self.wipe();
                    info!(
                        "game slots wiped in {} us; spill words not zero after wipe: {}",
                        self.wipe_us, self.spill_left
                    );
                }
                return Err(match e {
                    LoadError::Card(FsError::NotFound) => StartError::NotFound,
                    other => StartError::Refused(other.notice()),
                });
            }
        };
        // The new code is in the slot: make sure the core fetches it, not
        // something it held from before.
        instruction_sync();
        // SAFETY: the address comes from the load that just passed every check:
        // the header was built for these very slots (address and size), the
        // checksum over the header, the code and the data is right, every
        // code word was read back as written, the zeroed variables are
        // zero, and the entry address is inside the code just loaded. That
        // is the most the console can know about a game; the contract
        // (game_api::GameEntry) is the game's promise. The address is used
        // only here and not kept.
        let entry: GameEntry =
            unsafe { core::mem::transmute::<usize, GameEntry>(entry_addr as usize) };
        let t2 = now_us();
        // SAFETY: as above; CHIP_CONSOLE_API lives for the whole program.
        let opened = unsafe { game_api::open_game(entry, &CHIP_CONSOLE_API) };
        let t3 = now_us();
        self.last.open_us = t3 - t2;
        match opened {
            Ok(cb) => {
                self.running = true;
                self.opened += 1;
                self.opened_at_us = Some(t3);
                info!(
                    "game {folder}: started, file {} bytes in {} us (code {} words, {} past the slot, data {} bytes, zeroed {}), entry {entry_addr:#010x} opened in {} us",
                    self.last.file_bytes,
                    self.last.load_us,
                    self.last.code_words,
                    self.last.spill_words,
                    self.last.data_bytes,
                    self.last.zeroed,
                    self.last.open_us
                );
                Ok(cb)
            }
            Err(e) => {
                warn!("game {folder}: its table was refused ({e:?})");
                self.wipe();
                Err(StartError::BadTable(e))
            }
        }
    }

    /// The game data slot, lent to the console while no game is loaded
    /// (the rules: `board_api::lent`). The home screen keeps its game
    /// list and tile pictures there (about 5.4 KB of the 48 KB), so they
    /// cost the console's own RAM nothing. `None` while a game runs, or
    /// when the slots are not in use (they differ from game_api::layout).
    ///
    /// Why this cannot be used while a game is loaded:
    /// - while `running` (a good start up to `end`) it gives nothing;
    /// - the slice borrows this `GameSlots` mutably, so while the console
    ///   holds it, `start`, `end`, the wipe and `nonzero_words` cannot be
    ///   called at all (the compiler refuses it): the loader never writes
    ///   the slot while the slice exists, and a game never starts while
    ///   it exists;
    /// - only one `GameSlots` can use the slots ([`GameSlots::check`]).
    ///
    /// The code slot is never lent: it takes only whole aligned 32-bit
    /// words, and the console reads and writes single bytes.
    ///
    /// The lent bytes are also the spill's RAM (its top, seen from the
    /// instruction bus). That is fine: nothing is lent while a game is
    /// loaded, and a load writes over them.
    pub fn lend(&mut self) -> Option<&mut [u8]> {
        if self.running {
            return None;
        }
        let slots = self.slots?;
        // SAFETY: the data slot that game-slots.x reserved
        // (_game_data_slot_start.._end, NOLOAD, checked equal to
        // game_api::layout at start-up): ordinary byte-addressable RAM,
        // no Rust value of the firmware lives there, and no game is loaded
        // (`running` is false), so no game code uses it either. The slice
        // lives no longer than this `&mut self`, and every other access to
        // the slot (the loader's writes, the wipe, `nonzero_words`) goes
        // through `&mut self` or `&self` of this one `GameSlots`, so
        // nothing else touches the bytes while the slice exists. Any byte
        // value is a valid `u8`.
        Some(unsafe {
            core::slice::from_raw_parts_mut(slots.data_addr as *mut u8, slots.data_size as usize)
        })
    }

    /// The game has ended: forget it and wipe both slots.
    pub fn end(&mut self) {
        self.running = false;
        self.wipe();
        info!(
            "game ended; slots wiped in {} us; spill words not zero after wipe: {}",
            self.wipe_us, self.spill_left
        );
    }

    /// Sets both slots to zero (code: whole words only), and measures it.
    ///
    /// The spill needs no writes of its own: it is the top of the data
    /// slot, which is zeroed here through the data bus. Afterwards the
    /// spill is read back through the instruction bus (`spill_left`, not
    /// in `wipe_us`), so every wipe shows that a stale call into it would
    /// meet zeros (the ILL instruction), as in the code slot. Not when the
    /// start-up mirror check's read step failed (`spill_read` 0).
    fn wipe(&mut self) {
        let Some(slots) = self.slots else { return };
        let t0 = now_us();
        let mut chip = ChipSlots::new(slots, false);
        let mut at = 0;
        while at < slots.code_size {
            chip.write_word(at, 0);
            at += 4;
        }
        chip.zero(0, slots.data_size);
        instruction_sync();
        self.wipe_us = now_us() - t0;
        self.wipes += 1;
        self.spill_left = count_nonzero(slots.code_addr + slots.code_size, self.spill_read);
        if self.spill_left != 0 {
            warn!(
                "game slots: {} spill words not zero after the wipe (the data slot was zeroed)",
                self.spill_left
            );
        }
    }

    /// Words in the code slot, the data slot and the spill (read from the
    /// instruction bus) that are not zero, to check a wipe from the debug
    /// link. Reads whole words only.
    pub fn nonzero_words(&self) -> Option<(u32, u32, u32)> {
        let slots = self.slots?;
        Some((
            count_nonzero(slots.code_addr, slots.code_size),
            count_nonzero(slots.data_addr, slots.data_size),
            count_nonzero(slots.code_addr + slots.code_size, self.spill_read),
        ))
    }

    /// One log line: `nonzero_words`, and whether a game runs.
    pub fn log_slots(&self) {
        match self.nonzero_words() {
            // At home the data slot holds home's game list and tile
            // pictures (`lend`), so data words not zero are expected
            // there, and the spill is the same RAM (its top); the code
            // slot stays zero until a game starts.
            Some((code, data, spill)) => info!(
                "game slots: {code} code words, {data} data words and {spill} spill words not zero (running={}; at home the data slot holds home's list and pictures)",
                self.running
            ),
            None => info!("game slots: not in use (they differ from game_api::layout)"),
        }
    }
}

/// Whole words in `addr..addr + size` that are not zero. Only called on
/// the slots and the spill as the linker reserved them (checked equal to
/// game_api::layout), so every read is a whole, aligned word of game RAM.
fn count_nonzero(addr: u32, size: u32) -> u32 {
    let mut n = 0;
    let mut at = 0;
    while at < size {
        // SAFETY: a whole, aligned word inside a slot or the spill the
        // linker reserved for games (see above). On the instruction bus
        // (code slot, spill) a 32-bit load is the only allowed access, and
        // this is one.
        if unsafe { ((addr + at) as *const u32).read_volatile() } != 0 {
            n += 1;
        }
        at += 4;
    }
    n
}

/// Which step of [`spill_mirror_check`] found the spill wrong.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MirrorFault {
    /// Step 2: a spill word read from the instruction side was not the
    /// data word the rule names. Nothing was written there.
    Reads,
    /// Step 3: a word written from the instruction side did not show on
    /// the data side.
    Writes,
}

/// MEMW: the core finishes every load and store before it starts the
/// next one, so a store is in RAM before the other bus reads it.
fn memory_wait() {
    // SAFETY: MEMW only orders memory accesses; it changes no memory.
    unsafe { core::arch::asm!("memw", options(nostack, preserves_flags)) };
}

/// The pattern the mirror check writes into data word `d`: the word's own
/// address, so a word read back from the other side says where it is.
const fn mirror_pattern(d: u32) -> u32 {
    d ^ 0xA5C3_0F96
}

/// Checks, once at start-up, that the spill is the data slot's top in
/// reverse word order (game_api::layout::sram1_iram_to_dram), before any
/// game code is ever written there. Runs while no game exists and before
/// home has lent the data slot; the wipe after it clears the patterns.
///
/// 1. Data side: every word of the data slot gets [`mirror_pattern`] of
///    its own address (our own slot; the wipe writes it anyway).
/// 2. Instruction side, read only: every spill word must show the
///    pattern of the data word `sram1_iram_to_dram` names. If not, the
///    first wrong word and what it shows are logged and nothing is
///    written on the instruction side, so even another mapping (which
///    could reach the ROM data below the heap) is never harmed.
/// 3. Instruction side, write: three words (first, middle, last of the
///    spill) are written there and read back from the data side. A
///    failure here has written up to three words on the instruction side.
///
/// Every step ends its stores with MEMW before the other side reads, so
/// a store still on its way cannot look like a wrong mirror.
///
/// Logs one line either way. `Err` means: use no spill.
fn spill_mirror_check(slots: &Slots) -> Result<(), MirrorFault> {
    use game_api::layout::sram1_iram_to_dram;

    let spill = slots.code_addr + slots.code_size;
    let words = slots.spill_size / 4;
    let data_end = slots.data_addr + slots.data_size;
    if slots.spill_size == 0 {
        return Ok(());
    }
    let t0 = now_us();
    // 1. Data side.
    let mut d = slots.data_addr;
    while d < data_end {
        // SAFETY: a whole, aligned word of the data slot that
        // game-slots.x reserved (checked equal to game_api::layout before
        // this runs); ordinary RAM, no Rust value lives there, nothing is
        // lent or loaded yet (`check` runs once, before anything else).
        unsafe { (d as *mut u32).write_volatile(mirror_pattern(d)) };
        d += 4;
    }
    memory_wait();
    // 2. Instruction side, read only.
    let mut wrong = 0u32;
    let mut first: Option<(u32, u32)> = None;
    let mut i = spill;
    while i < spill + slots.spill_size {
        // SAFETY: one whole, aligned 32-bit load inside the spill that
        // game-slots.x reserved (.game_spill, nothing of the console is
        // there). A load cannot change anything, whatever RAM it shows.
        let v = unsafe { (i as *const u32).read_volatile() };
        if v != mirror_pattern(sram1_iram_to_dram(i)) {
            wrong += 1;
            first.get_or_insert((i, v));
        }
        i += 4;
    }
    if let Some((i, v)) = first {
        let shows = mirror_pattern(v); // the pattern is its own inverse
        let want = sram1_iram_to_dram(i);
        if (slots.data_addr..data_end).contains(&shows) && shows.is_multiple_of(4) {
            warn!(
                "spill mirror WRONG: I {i:#010x} shows D {shows:#010x} (expected {want:#010x}); {wrong} of {words} words wrong; nothing written on the instruction side"
            );
        } else {
            warn!(
                "spill mirror WRONG: I {i:#010x} shows memory outside the data slot (word {v:#010x}, expected D {want:#010x}); {wrong} of {words} words wrong; nothing written on the instruction side"
            );
        }
        return Err(MirrorFault::Reads);
    }
    // 3. Instruction side, write: first, middle and last spill word.
    for i in [
        spill,
        spill + ((slots.spill_size / 2) & !3),
        spill + slots.spill_size - 4,
    ] {
        let word = !mirror_pattern(i);
        let d = sram1_iram_to_dram(i);
        // SAFETY: a whole, aligned 32-bit store into the spill (step 2
        // showed that this word is data word `d` of our own data slot),
        // then one aligned load of that data word.
        let back = unsafe {
            (i as *mut u32).write_volatile(word);
            memory_wait();
            (d as *const u32).read_volatile()
        };
        if back != word {
            warn!(
                "spill mirror WRONG: writing I {i:#010x} did not change D {d:#010x} (read {back:#010x}, wrote {word:#010x}); up to 3 words written on the instruction side"
            );
            return Err(MirrorFault::Writes);
        }
    }
    info!(
        "game slots: spill mirror ok, {words} words, I {spill:#010x} = D {:#010x}, in {} us",
        sram1_iram_to_dram(spill),
        now_us() - t0
    );
    Ok(())
}

/// `/CONSOLE/GAMES/<folder>/GAME.BIN` in `buf`.
fn game_path<'a>(buf: &'a mut [u8; 64], folder: &str) -> Option<&'a str> {
    let parts = [card::GAMES_DIR, "/", folder, "/", card::GAME_FILE];
    let mut len = 0;
    for p in parts {
        let end = len + p.len();
        buf.get_mut(len..end)?.copy_from_slice(p.as_bytes());
        len = end;
    }
    core::str::from_utf8(&buf[..len]).ok()
}

/// ISYNC: the core waits until earlier writes are done and fetches
/// instructions afresh. Xtensa asks for it after instruction memory was
/// written and before the new code runs.
fn instruction_sync() {
    // SAFETY: ISYNC only orders the pipeline; it touches no memory.
    unsafe { core::arch::asm!("isync", options(nostack, preserves_flags)) };
}

/// The real slots, as the loader sees them. Offsets count from the start
/// of a slot; the loader keeps them inside the sizes it was given (the
/// same `Slots` this was made with) and every code offset a multiple of 4.
pub struct ChipSlots {
    slots: Slots,
    code_words: u32,
    /// Code words written past the code slot (into the spill).
    spill_words: u32,
    data_bytes: u32,
    zeroed: u32,
    /// One past the highest code offset written: what of the data slot's
    /// top this load's code holds (`code_top - code_size`, when larger).
    code_top: u32,
    /// The flash windows are in place (`GameSlots::check`); `false` for
    /// the wipes and in a firmware without `game-flash`.
    #[cfg_attr(not(feature = "game-flash"), allow(dead_code))]
    flash_ok: bool,
    /// Microseconds the flash part check took.
    flash_check_us: u64,
}

impl ChipSlots {
    fn new(slots: Slots, flash_ok: bool) -> Self {
        Self {
            slots,
            code_words: 0,
            spill_words: 0,
            data_bytes: 0,
            zeroed: 0,
            code_top: 0,
            flash_ok,
            flash_check_us: 0,
        }
    }

    /// Code offsets allowed: the code slot and the spill
    /// (`Slots::code_room`, the loader's own rule).
    fn code_room(&self) -> u64 {
        self.slots.code_room()
    }

    /// The data slot's bytes from 0 that data may use: all of it, less
    /// the top that this load's code took through the spill. The loader
    /// keeps data out of it already; this turns a loader bug into a clear
    /// stop instead of code silently overwritten.
    fn data_room(&self) -> u32 {
        self.slots.data_room(self.code_top)
    }

    /// Whether anything was written.
    fn touched(&self) -> bool {
        self.code_words + self.data_bytes + self.zeroed > 0
    }

    /// One whole word into the code slot. The checks stop the firmware
    /// with a clear message instead of writing outside the slot.
    fn write_word(&mut self, offset: u32, word: u32) {
        assert!(offset.is_multiple_of(4) && u64::from(offset) < self.code_room());
        // SAFETY: a whole, aligned 32-bit word inside the code slot that
        // game-slots.x reserved (_game_code_slot_start.._end) or the spill
        // right after it (.game_spill; `linked_slots` checked that it
        // follows directly), both checked equal to game_api::layout at
        // start-up; the spill is in use only when the start-up mirror
        // check passed. The executable RAM takes only whole words: this is
        // a single 32-bit store.
        unsafe { ((self.slots.code_addr + offset) as *mut u32).write_volatile(word) }
        self.code_top = self.code_top.max(offset + 4);
    }

    fn zero(&mut self, offset: u32, len: u32) {
        let room = self.data_room();
        assert!(offset <= room && len <= room - offset);
        // SAFETY: inside the data slot that game-slots.x reserved
        // (_game_data_slot_start.._end); ordinary RAM, no Rust value lives
        // there.
        unsafe {
            core::ptr::write_bytes((self.slots.data_addr + offset) as *mut u8, 0, len as usize)
        }
    }
}

impl game_loader::GameSlots for ChipSlots {
    fn write_code_word(&mut self, offset: u32, word: u32) {
        self.write_word(offset, word);
        self.code_words += 1;
        if offset >= self.slots.code_size {
            self.spill_words += 1;
        }
    }

    fn read_code_word(&mut self, offset: u32) -> u32 {
        assert!(offset.is_multiple_of(4) && u64::from(offset) < self.code_room());
        // SAFETY: as in `write_word`: one aligned 32-bit load in the slot
        // or the spill.
        unsafe { ((self.slots.code_addr + offset) as *const u32).read_volatile() }
    }

    fn write_data(&mut self, offset: u32, bytes: &[u8]) {
        let len = bytes.len() as u32;
        let room = self.data_room();
        assert!(offset <= room && len <= room - offset);
        // SAFETY: inside the data slot (see `zero`); `bytes` is firmware
        // memory, never the slot itself, so the two do not overlap.
        unsafe {
            core::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                (self.slots.data_addr + offset) as *mut u8,
                bytes.len(),
            )
        }
        self.data_bytes += len;
    }

    fn zero_data(&mut self, offset: u32, len: u32) {
        self.zero(offset, len);
        self.zeroed += len;
    }

    /// Without `game-flash` the trait's default answers: every flash part
    /// is missing. With it: the part must be one this firmware holds, at
    /// the same addresses and sizes, with the same CRC-32s both as built
    /// in and as read from flash now. Asked before any slot is written.
    #[cfg(feature = "game-flash")]
    fn flash_part(&mut self, part: &FlashPartHeader) -> FlashHeld {
        if !self.flash_ok {
            return FlashHeld::Missing;
        }
        let Some(h) = held::HELD.iter().find(|h| {
            h.code_addr == part.code_addr
                && h.code_len == part.code_size
                && h.ro_len == part.ro_size
                && (h.ro_len == 0 || h.ro_addr == part.ro_addr)
        }) else {
            warn!(
                "game flash: no part held at code {:#010x}+{}, data {:#010x}+{}",
                part.code_addr, part.code_size, part.ro_addr, part.ro_size
            );
            return FlashHeld::Missing;
        };
        let t0 = now_us();
        let got_code = flash_crc(h.code_addr, h.code_len);
        let got_ro = flash_crc(h.ro_addr, h.ro_len);
        self.flash_check_us = now_us() - t0;
        let fits = got_code == h.code_crc
            && got_ro == h.ro_crc
            && got_code == part.code_crc32
            && got_ro == part.ro_crc32;
        if fits {
            info!(
                "game {}: flash part ok, code {} data {} bytes, checked in {} us",
                h.game, h.code_len, h.ro_len, self.flash_check_us
            );
            FlashHeld::Held
        } else {
            warn!(
                "game {}: flash part DIFFERS: crc32 code {got_code:#010x} data {got_ro:#010x}",
                h.game
            );
            FlashHeld::Differs { got_code, got_ro }
        }
    }
}
