//! Why a game was not loaded.

use board_api::FsError;

/// Why a game was not loaded. Each one has a short text for the screen
/// ([`LoadError::notice`]). The numbers inside are for the log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadError {
    /// The card said no (no card, no such file, a read error).
    Card(FsError),
    /// The file does not start with the word "GAME" (or "GAMF", a game
    /// with a flash part).
    NotAGame,
    /// Built for an older contract than this console runs (a version
    /// below `game_api::MIN_GAME_VERSION`). The number is the version in
    /// the file.
    TooOld { version: u32 },
    /// Built for a code slot at another address. A game is linked for
    /// one fixed address and cannot run anywhere else: it must be built
    /// again for this console.
    OtherCodeSlot { addr: u32 },
    /// Built for a data slot at another address.
    OtherDataSlot { addr: u32 },
    /// A size in the header is not a whole number of 32-bit words. The
    /// game build tool never makes such a file.
    OddSize,
    /// The code is bigger than this console's code slot and spill
    /// together (`room`). A console with no spill (an older one, or one
    /// whose start-up mirror check failed) has the code slot alone.
    CodeTooBig { size: u32, room: u32 },
    /// The data and the zeroed variables together are bigger than the
    /// data slot.
    DataTooBig { size: u32, room: u32 },
    /// The data would fit alone, but the code past the code slot
    /// (`spill` bytes, taken from the top of the data slot) and the data
    /// and zeroed variables (`data` bytes, from the bottom) do not fit in
    /// the data slot (`room`) together.
    CodeMeetsData { spill: u32, data: u32, room: u32 },
    /// The address where the game starts is not inside its code.
    BadEntry { entry: u32 },
    /// The file is shorter than its header says: probably a copy that
    /// did not finish.
    CutShort { size: u32, want: u32 },
    /// The file is longer than its header says.
    TooLong { size: u32, want: u32 },
    /// The file changed while it was being read (the card said one size,
    /// then gave fewer or more bytes).
    Changed,
    /// The checksum is wrong: some byte of the header, the code or the
    /// data is not what the game build tool wrote.
    Damaged { in_file: u32, computed: u32 },
    /// A word read back from the code slot was not the word just written
    /// there. Not the file's fault: the board's code slot is not working.
    CodeSlotFailed { offset: u32, wrote: u32, read: u32 },
    /// The code past the code slot (`spill` bytes) did not read back the
    /// same at the end of the load as while it was written: its checksum
    /// changed. Something wrote over it (the board's mirror is not what
    /// `game_api::layout` says, or a loader bug). Not the file's fault.
    /// The checksum cannot tell which word changed.
    CodeOverwritten { spill: u32 },
    /// A "GAMF" file whose flash part extension
    /// (`game_api::FlashPartHeader`) is wrong: a size other than 32, a
    /// half outside its window or not on whole words, an empty code half,
    /// or a reserved word that is not 0.
    BadFlashPart,
    /// A "GAMF" file whose flash part this console's firmware does not
    /// hold (no part at those addresses and sizes, or a firmware built
    /// without flash parts). Nothing was written.
    FlashPartMissing,
    /// The firmware holds a part at the file's addresses and sizes, but
    /// not the same bytes: the game and the firmware were built apart.
    /// `got_code` and `got_ro` are the CRC-32s the board found (or its
    /// build-time ones, when those already differ). Nothing was written.
    FlashPartDiffers { got_code: u32, got_ro: u32 },
}

impl LoadError {
    /// A short text for the console to show: capitals only (the
    /// console's font has no small letters), at most [`NOTICE_MAX`]
    /// letters.
    pub fn notice(&self) -> &'static str {
        match self {
            LoadError::Card(FsError::NoCard) => "NO CARD",
            LoadError::Card(FsError::NotFound) => "GAME NOT FOUND",
            LoadError::Card(FsError::BadName) => "BAD GAME FILE NAME",
            LoadError::Card(FsError::Io) => "CARD READ ERROR",
            LoadError::Card(FsError::Unsupported) => "CARD CANNOT DO THIS",
            LoadError::NotAGame => "NOT A GAME FILE",
            LoadError::TooOld { .. } => "GAME IS TOO OLD",
            LoadError::OtherCodeSlot { .. } => "GAME FOR OTHER CODE SLOT",
            LoadError::OtherDataSlot { .. } => "GAME FOR OTHER DATA SLOT",
            LoadError::OddSize => "BAD SIZES IN GAME FILE",
            LoadError::CodeTooBig { .. } => "GAME CODE TOO BIG",
            LoadError::DataTooBig { .. } => "GAME DATA TOO BIG",
            LoadError::CodeMeetsData { .. } => "GAME TOO BIG FOR SLOTS",
            LoadError::BadEntry { .. } => "BAD GAME START ADDRESS",
            LoadError::CutShort { .. } => "GAME FILE CUT SHORT",
            LoadError::TooLong { .. } => "GAME FILE TOO LONG",
            LoadError::Changed => "FILE CHANGED WHILE READ",
            LoadError::Damaged { .. } => "GAME FILE IS DAMAGED",
            LoadError::CodeSlotFailed { .. } => "CODE SLOT WRITE FAILED",
            LoadError::CodeOverwritten { .. } => "GAME CODE OVERWRITTEN",
            LoadError::BadFlashPart => "BAD FLASH PART IN FILE",
            LoadError::FlashPartMissing => "FLASH PART MISSING",
            LoadError::FlashPartDiffers { .. } => "FLASH PART DIFFERS",
        }
    }
}

/// The longest [`LoadError::notice`] text, in letters.
pub const NOTICE_MAX: usize = 24;
