//! Copies a game file into the two slots.
//!
//! The file is taken as a stream: the loader asks for the next few hundred
//! bytes, puts them where they belong, and asks again. A game never has to
//! fit in memory twice, and the same code loads from the firmware's own
//! flash and from a file on the SD card.
//!
//! Order of work:
//!
//! 1. Read the header and check it ([`GameHeader::validate`]). Nothing has
//!    been written yet, so a wrong file costs nothing.
//! 2. Copy the code into the code slot, one 32-bit word at a time, and read
//!    every word back.
//! 3. Read the data straight into the data slot.
//! 4. Compare the checksum. Only now is the game known to be whole.
//! 5. Zero the game's zeroed variables.
//!
//! A game that fails at step 2 to 4 leaves rubbish in the slots. That is
//! harmless as long as nobody jumps there, and the next load overwrites it.

use core::{ptr, slice};

use esp_hal::time::Instant;
use game_api::{Crc32, GameEntry, GameHeader, HeaderError, layout};

/// Where a game file comes from.
pub trait Source {
    /// Puts the next bytes of the file into `buffer` and says how many.
    /// Fewer than asked is allowed; 0 means the file has ended.
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, SourceError>;
}

/// The storage behind a [`Source`] failed (the SD card, in practice).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceError;

/// A game file that is already in memory: the one built into the firmware.
pub struct SliceSource<'a>(pub &'a [u8]);

impl Source for SliceSource<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, SourceError> {
        let count = buffer.len().min(self.0.len());
        let (now, later) = self.0.split_at(count);
        buffer[..count].copy_from_slice(now);
        self.0 = later;
        Ok(count)
    }
}

/// Why a game was not loaded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadError {
    /// The storage failed.
    Source,
    /// The file ended before the header said it would.
    TooShort,
    /// The file goes on after the header said it would end.
    TooLong,
    /// The header is not one this console can load.
    Header(HeaderError),
    /// A word read back from the code slot was not the word written.
    CodeReadBack { address: u32, wrote: u32, read: u32 },
    /// The file is damaged.
    Checksum { in_file: u32, computed: u32 },
}

/// Where the time went, in microseconds.
#[derive(Clone, Copy, Debug, Default)]
pub struct Timing {
    /// Waiting for the source to hand over bytes (the card, or a copy out
    /// of flash). For the data part this includes the copy into the slot,
    /// because the source writes there directly.
    pub read: u64,
    /// Computing the checksum.
    pub checksum: u64,
    /// Writing the code slot word by word and reading each word back.
    pub code_write: u64,
    /// Zeroing the game's zeroed variables.
    pub zero: u64,
    /// From the first byte asked for to "ready to call".
    pub total: u64,
}

/// A game that is in the slots and ready to be called.
pub struct Loaded {
    pub header: GameHeader,
    pub entry: GameEntry,
    pub timing: Timing,
}

/// Bytes handled at a time. One SD card block is 512 bytes.
const CHUNK: usize = 1024;

/// A buffer whose start is on a 4-byte boundary, so that it can be read as
/// 32-bit words. A plain `[u8; N]` may start at any address.
#[repr(align(4))]
struct Aligned([u8; CHUNK]);

/// Fills `buffer` completely, or reports that the file ended early.
fn read_exact(source: &mut dyn Source, buffer: &mut [u8], timing: &mut Timing) -> Result<(), LoadError> {
    let started = Instant::now();
    let mut filled = 0;
    while filled < buffer.len() {
        match source.read(&mut buffer[filled..]) {
            Ok(0) => return Err(LoadError::TooShort),
            Ok(count) => filled += count,
            Err(SourceError) => return Err(LoadError::Source),
        }
    }
    timing.read += started.elapsed().as_micros();
    Ok(())
}

fn add_to_checksum(crc: &mut Crc32, bytes: &[u8], timing: &mut Timing) {
    let started = Instant::now();
    crc.update(bytes);
    timing.checksum += started.elapsed().as_micros();
}

/// Loads a game. On success the game has not run yet: call
/// [`Loaded::entry`] to start it.
pub fn load(source: &mut dyn Source) -> Result<Loaded, LoadError> {
    let started = Instant::now();
    let mut timing = Timing::default();
    let mut crc = Crc32::new();

    // 1. The header.
    let mut first = [0u8; GameHeader::SIZE];
    read_exact(source, &mut first, &mut timing)?;
    let header = GameHeader::from_bytes(&first);
    header.validate().map_err(LoadError::Header)?;
    add_to_checksum(&mut crc, &first[..GameHeader::CHECKED], &mut timing);

    // 2. The code. The code slot is executable RAM, which the chip lets a
    // program read and write only as whole 32-bit words at addresses that
    // are multiples of 4. `copy_from_slice` would copy bytes and stop the
    // program, so each word is written by hand. "Volatile" tells the
    // compiler to do exactly these writes and not to replace the loop with
    // a call to its byte-copy routine.
    let mut chunk = Aligned([0; CHUNK]);
    let mut address = header.code_addr;
    let code_end = header.code_addr + header.code_size;
    while address < code_end {
        // `validate` made sure the code size is a multiple of 4, so every
        // piece is too.
        let count = ((code_end - address) as usize).min(CHUNK);
        let bytes = &mut chunk.0[..count];
        read_exact(source, bytes, &mut timing)?;
        add_to_checksum(&mut crc, bytes, &mut timing);

        let writing = Instant::now();
        for word in bytes.chunks_exact(4) {
            let word = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
            let place = address as *mut u32;
            // SAFETY: `validate` checked that the code lies inside the
            // code slot, which the linker keeps free of anything else
            // (game-slots.x). The address is a multiple of 4.
            let read = unsafe {
                place.write_volatile(word);
                place.read_volatile()
            };
            if read != word {
                return Err(LoadError::CodeReadBack { address, wrote: word, read });
            }
            address += 4;
        }
        timing.code_write += writing.elapsed().as_micros();
    }

    // 3. The data. The data slot is ordinary RAM, so the source can put
    // the bytes there itself.
    // SAFETY: `validate` checked that data and zeroed memory lie inside
    // the data slot, which the linker keeps free of anything else. No game
    // is running: the caller starts one only after this function returns.
    let data = unsafe { slice::from_raw_parts_mut(header.data_addr as *mut u8, header.data_size as usize) };
    for piece in data.chunks_mut(CHUNK) {
        read_exact(source, piece, &mut timing)?;
        add_to_checksum(&mut crc, piece, &mut timing);
    }

    // The file must end here. A longer file is not the file that was
    // packed.
    let mut extra = [0u8; 1];
    match source.read(&mut extra) {
        Ok(0) => {}
        Ok(_) => return Err(LoadError::TooLong),
        Err(SourceError) => return Err(LoadError::Source),
    }

    // 4. The checksum.
    if crc.value() != header.crc32 {
        return Err(LoadError::Checksum { in_file: header.crc32, computed: crc.value() });
    }

    // 5. The zeroed variables come right after the data.
    let zeroing = Instant::now();
    // SAFETY: as for the data above.
    unsafe { ptr::write_bytes((header.data_addr + header.data_size) as *mut u8, 0, header.bss_size as usize) };
    timing.zero = zeroing.elapsed().as_micros();

    // SAFETY: `validate` checked that the address is inside the code just
    // written, and the checksum says that code is what the packer wrote.
    // That it really is a function of this type is the packer's promise:
    // it takes the address from the ELF file's entry point.
    let entry = unsafe { core::mem::transmute::<usize, GameEntry>(header.entry as usize) };

    timing.total = started.elapsed().as_micros();
    Ok(Loaded { header, entry, timing })
}

/// Fills both slots with rubbish, so that a load that follows cannot pass
/// by accident on what an earlier load left behind. Returns the time it
/// took to fill the code slot and the data slot, in microseconds.
///
/// The code slot gets zero words: the processor reads a zero as an illegal
/// instruction and stops, which is the best thing a stray jump can hit.
/// The data slot gets 0xA5 bytes, so a game whose "zeroed" variables are
/// not zero was not zeroed by the loader.
pub fn scrub() -> (u64, u64) {
    let started = Instant::now();
    let mut address = layout::CODE_SLOT_ADDR;
    while address < layout::CODE_SLOT_ADDR + layout::CODE_SLOT_SIZE {
        // SAFETY: a whole word inside the code slot; no game is loaded.
        unsafe { (address as *mut u32).write_volatile(0) };
        address += 4;
    }
    let code = started.elapsed().as_micros();

    let started = Instant::now();
    // SAFETY: the data slot; no game is loaded.
    unsafe { ptr::write_bytes(layout::DATA_SLOT_ADDR as *mut u8, 0xA5, layout::DATA_SLOT_SIZE as usize) };
    (code, started.elapsed().as_micros())
}

/// Checks that the linker reserved the slots where `game-api` says they
/// are. They come from the same numbers, so a mismatch means the build is
/// broken, and it is better to hear it at start-up than from a game.
pub fn slots_are_where_expected() -> bool {
    unsafe extern "C" {
        static _game_code_slot_start: u8;
        static _game_code_slot_end: u8;
        static _game_data_slot_start: u8;
        static _game_data_slot_end: u8;
    }
    let code = (&raw const _game_code_slot_start) as u32;
    let code_end = (&raw const _game_code_slot_end) as u32;
    let data = (&raw const _game_data_slot_start) as u32;
    let data_end = (&raw const _game_data_slot_end) as u32;
    code == layout::CODE_SLOT_ADDR
        && code_end == layout::CODE_SLOT_ADDR + layout::CODE_SLOT_SIZE
        && data == layout::DATA_SLOT_ADDR
        && data_end == layout::DATA_SLOT_ADDR + layout::DATA_SLOT_SIZE
}
