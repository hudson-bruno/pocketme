//! Reading, checking and copying a game file.

use board_api::{Files, FsError};
use game_api::{Crc32, FlashPartHeader, GameHeader, MAGIC, MAGIC_FLASH, MIN_GAME_VERSION};

use crate::{FlashHeld, GameSlots, LoadError, PIECE, Slots};

/// What one load cost, for the log and for the tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LoadStats {
    /// Calls to `Files::read_at`.
    pub reads: u32,
    /// Bytes those calls gave.
    pub bytes: u32,
}

/// A game that passed every check and is now in the slots, ready to
/// start. Only [`load`] makes one, and only when nothing went wrong.
///
/// It is not `Clone`: start the game from the `Loaded` of the last load
/// only. Once another load has begun, the slots hold other bytes, even
/// if that load failed.
#[must_use]
#[derive(Debug, PartialEq, Eq)]
pub struct Loaded {
    header: GameHeader,
    flash: Option<FlashPartHeader>,
    stats: LoadStats,
}

impl Loaded {
    /// The address of the game's entry function, inside the code slot.
    /// On the chip the board turns this into a `game_api::GameEntry`
    /// and calls it.
    pub fn entry(&self) -> u32 {
        self.header.entry
    }

    /// The file's header (sizes, version, checksum), for the log.
    pub fn header(&self) -> &GameHeader {
        &self.header
    }

    /// The flash part the game runs with ("GAMF" files only), for the
    /// log. The board said it holds exactly these bytes.
    pub fn flash_part(&self) -> Option<&FlashPartHeader> {
        self.flash.as_ref()
    }

    /// What the load cost.
    pub fn stats(&self) -> LoadStats {
        self.stats
    }
}

/// Bytes before the payload: the header, and for "GAMF" the extension.
fn head_size(header: &GameHeader) -> u32 {
    if header.magic == MAGIC_FLASH {
        FlashPartHeader::FILE_HEAD as u32
    } else {
        GameHeader::SIZE as u32
    }
}

/// Checks a header against the console's slots. Nothing is read or
/// written. [`load`] does this first; it is public so that tests and
/// tools can ask about a header alone.
///
/// A game made for a later contract version is let through: the
/// contract says a newer game may run on an older console, and the game
/// itself then checks the console's tables (crates/game-api README).
///
/// Code past the code slot runs on into the spill, the top of the data
/// slot (`Slots::spill_size`). It is let through when the code fits in
/// the slot and the spill, and the spill, the data and the zeroed
/// variables fit in the data slot together (`game_api::layout::fits`).
/// So a refused file never writes a byte, and a good one never writes
/// data over its own code.
///
/// A "GAMF" header (a game with a flash part) follows the same rules for
/// its RAM part; its extension is checked by [`load`].
pub fn check_header(header: &GameHeader, slots: &Slots) -> Result<(), LoadError> {
    if header.magic != MAGIC && header.magic != MAGIC_FLASH {
        return Err(LoadError::NotAGame);
    }
    if header.api_version < MIN_GAME_VERSION {
        return Err(LoadError::TooOld {
            version: header.api_version,
        });
    }
    if header.code_addr != slots.code_addr {
        return Err(LoadError::OtherCodeSlot {
            addr: header.code_addr,
        });
    }
    if header.data_addr != slots.data_addr {
        return Err(LoadError::OtherDataSlot {
            addr: header.data_addr,
        });
    }
    let words = |n: u32| n.is_multiple_of(4);
    if !(words(header.code_size) && words(header.data_size) && words(header.bss_size)) {
        return Err(LoadError::OddSize);
    }
    // The spill is the top of the data slot, so it is never more than
    // the data slot. Sizes are added as 64-bit numbers: two huge sizes
    // must not wrap round to a small one.
    let code_room = slots.code_room();
    if u64::from(header.code_size) > code_room {
        return Err(LoadError::CodeTooBig {
            size: header.code_size,
            room: u32::try_from(code_room).unwrap_or(u32::MAX),
        });
    }
    let data_all = u64::from(header.data_size) + u64::from(header.bss_size);
    if data_all > u64::from(slots.data_size) {
        return Err(LoadError::DataTooBig {
            size: u32::try_from(data_all).unwrap_or(u32::MAX),
            room: slots.data_size,
        });
    }
    // Code past the slot takes the data slot from the top, the data from
    // the bottom: they must not meet. (No spill: the check above.)
    let spill = header.code_size.saturating_sub(slots.code_size);
    if data_all + u64::from(spill) > u64::from(slots.data_size) {
        return Err(LoadError::CodeMeetsData {
            spill,
            // At most the data slot (checked above), so it fits.
            data: u32::try_from(data_all).unwrap_or(u32::MAX),
            room: slots.data_size,
        });
    }
    // The whole file (header, code, data) must have a size that fits in
    // 32 bits, as the card gives sizes. The chip's slots are far smaller,
    // so only made-up test slots can fail this. It lets `load` add the
    // three sizes without any risk of going round past u32::MAX.
    let head = head_size(header);
    let file_size = u64::from(head) + u64::from(header.code_size) + u64::from(header.data_size);
    if file_size > u64::from(u32::MAX) {
        return Err(LoadError::DataTooBig {
            size: header.data_size,
            room: (u32::MAX - head).saturating_sub(header.code_size),
        });
    }
    // The entry must be inside the code. An empty code fails here too.
    let inside = header
        .entry
        .checked_sub(header.code_addr)
        .is_some_and(|offset| offset < header.code_size);
    if !inside {
        return Err(LoadError::BadEntry {
            entry: header.entry,
        });
    }
    Ok(())
}

/// Loads the game file at `path` into the slots.
///
/// # The order of work
///
/// 1. Read the 36-byte header and check it against `slots`
///    ([`check_header`]). For a "GAMF" file, read the 32-byte flash part
///    extension too and check it (`game_api::FlashPartHeader::validate`).
/// 2. Ask the card for the file's size. It must be exactly the header
///    (and extension), the code and the data. A file that was cut short
///    (a copy that did not finish) is refused here.
///
///    2b. Only for "GAMF": ask the board whether its firmware holds the
///    flash part ([`GameSlots::flash_part`]). If not, refuse.
///
///    Nothing has been written yet. A wrong file costs nothing.
/// 3. Read the rest in pieces of at most [`PIECE`] bytes. Add each piece
///    to the checksum, then hand it to the board: code as 32-bit words
///    (each one read back at once), data as bytes.
/// 4. Ask for one byte past the end: there must be none.
/// 5. Compare the checksum (it covers the extension too). Only now is
///    the game known to be whole.
/// 6. Zero the game's variables that start at zero.
///
///    6b. Only for a game with code past the code slot: read the
///    spill's words once more and compare their checksum with the one
///    taken while they were written. The spill is the same RAM as the
///    top of the data slot, so this proves that no data or zero byte
///    landed on the code (on the chip: that the mirror is what
///    `game_api::layout` says). A game that fits in the code slot skips
///    this and pays nothing.
/// 7. Return [`Loaded`].
///
/// # Why this order is safe
///
/// A wrong checksum is only known at the end, after the slots were
/// written. So a failed load may leave rubbish in the slots. That is
/// harmless, because:
///
/// - nobody can start that rubbish: the only way to learn the entry
///   address is the [`Loaded`] that step 7 returns, and a failed load
///   returns an error instead;
/// - the loader keeps nothing between calls, and every load writes all
///   the bytes its game uses (code, data, zeroed variables). So the next
///   load does not care what the last one left behind.
///
/// Checking the whole file first and copying it in a second pass would
/// keep the slots clean, but it reads the card twice, and the file could
/// still change between the two passes, so the copy pass would need the
/// checksum again anyway. Nothing lives in the slots while a game is
/// being loaded, so clean slots buy nothing. The crate README has the
/// numbers.
pub fn load<F, S>(
    files: &mut F,
    path: &str,
    slots: &Slots,
    out: &mut S,
) -> Result<Loaded, LoadError>
where
    F: Files + ?Sized,
    S: GameSlots + ?Sized,
{
    let (header, flash, stats) = run(files, path, slots, out, true)?;
    Ok(Loaded {
        header,
        flash,
        stats,
    })
}

/// Reads and checks the whole file like [`load`], but writes nothing.
/// (So it skips step 6b of [`load`]: nothing was written that could be
/// overwritten.)
///
/// [`load`] does not need this. It is here for the "check first, copy
/// after" way (see the README), and for a console that wants to know a
/// game is whole without loading it (on the home screen, say).
///
/// It checks the file only: a "GAMF" file's flash part is not asked
/// about (there is no board here), so a whole file passes whether or not
/// the firmware holds its part.
pub fn check_file<F>(files: &mut F, path: &str, slots: &Slots) -> Result<LoadStats, LoadError>
where
    F: Files + ?Sized,
{
    run(files, path, slots, &mut Nowhere(0), false).map(|(_, _, stats)| stats)
}

/// Slots that keep nothing but the last code word (so the read-back
/// check passes), and say every flash part is held. Used by
/// [`check_file`].
struct Nowhere(u32);

impl GameSlots for Nowhere {
    fn write_code_word(&mut self, _offset: u32, word: u32) {
        self.0 = word;
    }
    fn read_code_word(&mut self, _offset: u32) -> u32 {
        self.0
    }
    fn write_data(&mut self, _offset: u32, _bytes: &[u8]) {}
    fn zero_data(&mut self, _offset: u32, _len: u32) {}
    fn flash_part(&mut self, _part: &FlashPartHeader) -> FlashHeld {
        FlashHeld::Held
    }
}

/// What [`run`] gives back: the header, the flash part extension (for
/// "GAMF") and the cost.
type Ran = (GameHeader, Option<FlashPartHeader>, LoadStats);

/// The work of [`load`] and [`check_file`]. `recheck`: do step 6b (the
/// spill read back at the end).
fn run<F, S>(
    files: &mut F,
    path: &str,
    slots: &Slots,
    out: &mut S,
    recheck: bool,
) -> Result<Ran, LoadError>
where
    F: Files + ?Sized,
    S: GameSlots + ?Sized,
{
    let mut stats = LoadStats::default();
    // The one buffer. The header is read into its start, then each piece
    // of the file in turn.
    let mut buf = [0u8; PIECE];

    // 1. The header.
    let got = read_full(files, path, 0, &mut buf[..GameHeader::SIZE], &mut stats)?;
    if got < 4 || (buf[..4] != MAGIC.to_le_bytes() && buf[..4] != MAGIC_FLASH.to_le_bytes()) {
        return Err(LoadError::NotAGame);
    }
    let Some(head) = buf.first_chunk::<{ GameHeader::SIZE }>() else {
        // Cannot happen: PIECE is at least the header (checked while
        // compiling). Written this way so that no panic is possible.
        return Err(LoadError::NotAGame);
    };
    if got < GameHeader::SIZE {
        return Err(LoadError::CutShort {
            size: got as u32,
            want: GameHeader::SIZE as u32,
        });
    }
    let header = GameHeader::from_bytes(head);
    check_header(&header, slots)?;
    let mut crc = Crc32::new();
    crc.update(&head[..GameHeader::CHECKED]);

    // 1b. The flash part extension ("GAMF" only), right after the header.
    let head_len = head_size(&header);
    let mut flash = None;
    if header.magic == MAGIC_FLASH {
        let ext_buf = &mut buf[..FlashPartHeader::SIZE];
        let got = read_full(files, path, GameHeader::SIZE as u32, ext_buf, &mut stats)?;
        if got < FlashPartHeader::SIZE {
            return Err(LoadError::CutShort {
                size: (GameHeader::SIZE + got) as u32,
                want: head_len,
            });
        }
        let Some(ext) = buf.first_chunk::<{ FlashPartHeader::SIZE }>() else {
            // Cannot happen: PIECE is bigger than the extension.
            return Err(LoadError::BadFlashPart);
        };
        crc.update(ext);
        let ext = FlashPartHeader::from_bytes(ext);
        ext.validate().map_err(|_| LoadError::BadFlashPart)?;
        flash = Some(ext);
    }

    // 2. The size. `check_header` made sure the sum fits in 32 bits.
    let end = head_len + header.code_size + header.data_size;
    let size = files.size(path).map_err(LoadError::Card)?;
    if size < end {
        return Err(LoadError::CutShort { size, want: end });
    }
    if size > end {
        return Err(LoadError::TooLong { size, want: end });
    }

    // 2b. The flash part must be in this firmware, byte for byte. Asked
    // before any slot is written.
    if let Some(part) = &flash {
        match out.flash_part(part) {
            FlashHeld::Held => {}
            FlashHeld::Missing => return Err(LoadError::FlashPartMissing),
            FlashHeld::Differs { got_code, got_ro } => {
                return Err(LoadError::FlashPartDiffers { got_code, got_ro });
            }
        }
    }

    // 3. The code and the data, in pieces. Each piece ends at a multiple
    // of PIECE in the file (or at the end), so the reads line up with
    // the card's blocks. All piece lengths are multiples of 4, because
    // the header, PIECE and both sizes are.
    let code_end = header.code_size;
    // The checksum of the code past the code slot, as it was written
    // (step 6b).
    let mut spill_crc = Crc32::new();
    let mut pos = head_len;
    while pos < end {
        let len = (PIECE as u32 - pos % PIECE as u32).min(end - pos);
        let piece = &mut buf[..len as usize];
        if read_full(files, path, pos, piece, &mut stats)? < piece.len() {
            // The size said there was more.
            return Err(LoadError::Changed);
        }
        crc.update(piece);

        // Where this piece starts in the code-then-data that follows the
        // header.
        let mut at = pos - head_len;
        let mut rest: &[u8] = piece;
        if at < code_end {
            let (code, data) = rest.split_at(((code_end - at) as usize).min(rest.len()));
            let (words, _none) = code.as_chunks::<4>();
            for &bytes in words {
                let word = u32::from_le_bytes(bytes);
                if at >= slots.code_size {
                    spill_crc.update(&bytes);
                }
                out.write_code_word(at, word);
                let read = out.read_code_word(at);
                if read != word {
                    return Err(LoadError::CodeSlotFailed {
                        offset: at,
                        wrote: word,
                        read,
                    });
                }
                at += 4;
            }
            rest = data;
        }
        if !rest.is_empty() {
            out.write_data(at - code_end, rest);
        }
        pos += len;
    }

    // 4. Nothing may follow.
    match files.read_at(path, end, &mut buf[..1]) {
        Ok(0) => stats.reads += 1,
        Ok(_) => return Err(LoadError::Changed),
        Err(e) => return Err(LoadError::Card(e)),
    }

    // 5. The checksum.
    if crc.value() != header.crc32 {
        return Err(LoadError::Damaged {
            in_file: header.crc32,
            computed: crc.value(),
        });
    }

    // 6. The variables that start at zero come right after the data.
    out.zero_data(header.data_size, header.bss_size);

    // 6b. The code past the slot, read once more after every data byte
    // and zero was written.
    if recheck && header.code_size > slots.code_size {
        let mut again = Crc32::new();
        let mut at = slots.code_size;
        while at < header.code_size {
            again.update(&out.read_code_word(at).to_le_bytes());
            at += 4;
        }
        if again.value() != spill_crc.value() {
            return Err(LoadError::CodeOverwritten {
                spill: header.code_size - slots.code_size,
            });
        }
    }
    Ok((header, flash, stats))
}

/// Reads from `offset` until `buf` is full or the file ends. Returns the
/// bytes read: less than `buf.len()` only when the file ended.
fn read_full<F: Files + ?Sized>(
    files: &mut F,
    path: &str,
    offset: u32,
    buf: &mut [u8],
    stats: &mut LoadStats,
) -> Result<usize, LoadError> {
    let mut filled = 0;
    while filled < buf.len() {
        let rest = &mut buf[filled..];
        let n = files
            .read_at(path, offset + filled as u32, rest)
            .map_err(LoadError::Card)?;
        stats.reads += 1;
        if n == 0 {
            break;
        }
        if n > rest.len() {
            // The board said it gave more than there was room for. A bug
            // in the board; treated as a read error rather than trusted.
            return Err(LoadError::Card(FsError::Io));
        }
        stats.bytes += n as u32;
        filled += n;
    }
    Ok(filled)
}
