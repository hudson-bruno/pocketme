//! A card built into the firmware (feature `demo-card`), so games can be
//! started with no card in the slot and nothing ever written to one.
//!
//! - The chip games (every game in games/: BENCH, CAMBER, DRIVE3D,
//!   HAIRPIN, KESTREL, LANDER, LANTERN, MERCATOR, PADTEST, PROTO3D,
//!   ROOFTOP, SKIMMER, STARDUST, TUNDRA): GAME.BIN, INFO.TXT and
//!   the tile picture ART.BIN (when the game has one) from the card
//!   folder games/build-chip.sh makes.
//!   build.rs puts them in (`demo_games.rs`) and checks each header
//!   against game_api::layout, so a file built for other slots stops the
//!   build. (The four made-up games the card had before are gone, as on
//!   boards/desktop's sample card.) Build order:
//!   `GAMES_TARGET=boards/cyd/target games/build-chip.sh` first, then
//!   this crate (check.sh does both).
//! - Measuring only: test files named in `CYD_DEMO_EXTRA` at build time
//!   (see build.rs), never in a normal build.
//!
//! Read only, like every card in this round.

use board_api::{DirEntry, Files, FsError};

struct Game {
    folder: &'static str,
    info: &'static [u8],
    bin: &'static [u8],
    /// The tile picture (ART.BIN); empty: the game has none, and its
    /// folder has no such file.
    art: &'static [u8],
}

// `BUILT`: the games (and any test files), made by build.rs, in the
// order of their folder names.
include!(concat!(env!("OUT_DIR"), "/demo_games.rs"));

/// Every game on the demo card.
fn games() -> impl Iterator<Item = &'static Game> {
    BUILT.iter()
}

/// What a path names on the demo card.
enum Node {
    Root,
    Console,
    Games,
    Folder(&'static Game),
    Info(&'static Game),
    Bin(&'static Game),
    Art(&'static Game),
}

/// Splits `path` ("/CONSOLE/GAMES/LANDER/INFO.TXT") into its parts and
/// finds them. Names match whatever their case, like FAT.
fn find(path: &str) -> Result<Node, FsError> {
    if !path.starts_with('/') {
        return Err(FsError::BadName);
    }
    let mut parts = path.split('/').filter(|p| !p.is_empty());
    let eq = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
    let Some(a) = parts.next() else {
        return Ok(Node::Root);
    };
    if !eq(a, "CONSOLE") {
        return Err(FsError::NotFound);
    }
    let Some(b) = parts.next() else {
        return Ok(Node::Console);
    };
    if !eq(b, "GAMES") {
        return Err(FsError::NotFound);
    }
    let Some(c) = parts.next() else {
        return Ok(Node::Games);
    };
    let game = games().find(|g| eq(g.folder, c)).ok_or(FsError::NotFound)?;
    let node = match parts.next() {
        None => Node::Folder(game),
        Some(f) if eq(f, "INFO.TXT") => Node::Info(game),
        Some(f) if eq(f, "GAME.BIN") => Node::Bin(game),
        Some(f) if eq(f, "ART.BIN") && !game.art.is_empty() => Node::Art(game),
        Some(_) => return Err(FsError::NotFound),
    };
    if parts.next().is_some() {
        return Err(FsError::NotFound);
    }
    Ok(node)
}

pub struct DemoCard;

impl Files for DemoCard {
    fn list<F: FnMut(&DirEntry<'_>) -> bool>(
        &mut self,
        dir: &str,
        mut each: F,
    ) -> Result<(), FsError> {
        let dir_entry = |name| DirEntry {
            name,
            size: 0,
            is_dir: true,
        };
        match find(dir)? {
            Node::Root => {
                each(&dir_entry("CONSOLE"));
            }
            Node::Console => {
                each(&dir_entry("GAMES"));
            }
            Node::Games => {
                for g in games() {
                    if !each(&dir_entry(g.folder)) {
                        break;
                    }
                }
            }
            Node::Folder(g) => {
                let info = DirEntry {
                    name: "INFO.TXT",
                    size: g.info.len() as u32,
                    is_dir: false,
                };
                let bin = DirEntry {
                    name: "GAME.BIN",
                    size: g.bin.len() as u32,
                    is_dir: false,
                };
                let art = DirEntry {
                    name: "ART.BIN",
                    size: g.art.len() as u32,
                    is_dir: false,
                };
                let _ = (g.art.is_empty() || each(&art)) && each(&bin) && each(&info);
            }
            Node::Info(_) | Node::Bin(_) | Node::Art(_) => return Err(FsError::NotFound),
        }
        Ok(())
    }

    fn size(&mut self, path: &str) -> Result<u32, FsError> {
        match find(path)? {
            Node::Info(g) => Ok(g.info.len() as u32),
            Node::Bin(g) => Ok(g.bin.len() as u32),
            Node::Art(g) => Ok(g.art.len() as u32),
            _ => Err(FsError::NotFound),
        }
    }

    fn read_at(&mut self, path: &str, offset: u32, buf: &mut [u8]) -> Result<usize, FsError> {
        match find(path)? {
            Node::Info(g) => Ok(copy_from(g.info, offset, buf)),
            Node::Bin(g) => Ok(copy_from(g.bin, offset, buf)),
            Node::Art(g) => Ok(copy_from(g.art, offset, buf)),
            _ => Err(FsError::NotFound),
        }
    }
}

/// Copies what `file` holds from `offset` on into `buf`; 0 past the end.
fn copy_from(file: &[u8], offset: u32, buf: &mut [u8]) -> usize {
    let from = (offset as usize).min(file.len());
    let n = buf.len().min(file.len() - from);
    buf[..n].copy_from_slice(&file[from..from + n]);
    n
}
