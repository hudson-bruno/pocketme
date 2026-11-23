//! Files: the SD card on the CYD, a normal folder on the PC.
//!
//! There are no file handles. Each call names the file and where to
//! read. That keeps borrowing simple (nothing stays open between calls)
//! and costs little: a board may keep the last file open by itself.
//!
//! Paths start with `/` and use `/` between folders, for example
//! `/CONSOLE/GAMES/SNAKE.BIN`. Every name is an 8.3 name in capitals
//! (up to 8 letters, a dot, up to 3 letters), because the card's file
//! system (FAT) without long names takes only those. The PC board keeps
//! the same rule, so a file that works on the PC also works on the card.

/// Longest path in bytes. 4 folders of 8.3 names with their slashes fit.
pub const PATH_MAX: usize = 64;
/// Longest name of one entry in bytes ("12345678.123").
pub const NAME_MAX: usize = 12;

/// What went wrong with a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FsError {
    /// No card in the slot, or it cannot be read at all.
    NoCard,
    /// The file or folder is not there.
    NotFound,
    /// The path breaks the 8.3 rule, or is longer than [`PATH_MAX`].
    BadName,
    /// The card (or the PC folder) gave a read error.
    Io,
    /// This board cannot do this.
    Unsupported,
}

/// One entry of a folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirEntry<'a> {
    /// The 8.3 name, for example `SNAKE.BIN`. At most [`NAME_MAX`] bytes.
    pub name: &'a str,
    /// Size in bytes (0 for a folder).
    pub size: u32,
    pub is_dir: bool,
}

/// The card (or the PC folder).
pub trait Files {
    /// Calls `each` once per entry of folder `dir`. The order is the
    /// order on the card. `each` returns `true` to go on, `false` to stop.
    fn list<F: FnMut(&DirEntry<'_>) -> bool>(&mut self, dir: &str, each: F) -> Result<(), FsError>;

    /// Size of the file in bytes. A folder is not a file: `NotFound`.
    fn size(&mut self, path: &str) -> Result<u32, FsError>;

    /// Reads from byte `offset` of the file into `buf`. Returns how many
    /// bytes were read: less than `buf.len()` only at the end of the
    /// file, and 0 at or past the end.
    fn read_at(&mut self, path: &str, offset: u32, buf: &mut [u8]) -> Result<usize, FsError>;
}
