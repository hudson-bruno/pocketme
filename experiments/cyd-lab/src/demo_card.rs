//! A made-up card in flash (feature `demo-card`): four of the games on
//! boards/desktop's sample card (boards/desktop/card; that card has more
//! since the PC track added PADTEST and STARDUST), so the home screen has
//! something to show with no card in the slot.
//!
//! INFO.TXT texts are copied byte for byte from that folder. GAME.BIN
//! files have the same sizes, but no contents: this board cannot start
//! games yet, so reading one answers `Unsupported`.
//!
//! Read only, like every card in this round.

use board_api::{DirEntry, Files, FsError};

struct Game {
    folder: &'static str,
    info: &'static [u8],
    bin_size: u32,
}

/// Same order as a card written folder by folder (the console sorts).
const GAMES: [Game; 4] = [
    Game {
        folder: "BLOCKS",
        info: b"NAME=BLOCKS\nTAG=PUZZLE\nTILE=3A6BC2\n",
        bin_size: 2852,
    },
    Game {
        folder: "COMET",
        info: b"NAME=COMET\nTAG=ARCADE\nTILE=20204A\n",
        bin_size: 5412,
    },
    Game {
        folder: "SPROUT",
        info: b"NAME=SPROUT\nTAG=IDLE GAME\nTILE=3C8F4A\n",
        bin_size: 3620,
    },
    Game {
        folder: "TRAIL",
        info: b"NAME=TRAIL\nTAG=USES GPS\nTILE=C28A3A\nNEEDS=GPS\n",
        bin_size: 5156,
    },
];

/// What a path names on the demo card.
enum Node {
    Root,
    Console,
    Games,
    Folder(&'static Game),
    Info(&'static Game),
    Bin(&'static Game),
}

/// Splits `path` ("/CONSOLE/GAMES/BLOCKS/INFO.TXT") into its parts and
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
    let game = GAMES
        .iter()
        .find(|g| eq(g.folder, c))
        .ok_or(FsError::NotFound)?;
    let node = match parts.next() {
        None => Node::Folder(game),
        Some(f) if eq(f, "INFO.TXT") => Node::Info(game),
        Some(f) if eq(f, "GAME.BIN") => Node::Bin(game),
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
                for g in &GAMES {
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
                    size: g.bin_size,
                    is_dir: false,
                };
                let _ = each(&bin) && each(&info);
            }
            Node::Info(_) | Node::Bin(_) => return Err(FsError::NotFound),
        }
        Ok(())
    }

    fn size(&mut self, path: &str) -> Result<u32, FsError> {
        match find(path)? {
            Node::Info(g) => Ok(g.info.len() as u32),
            Node::Bin(g) => Ok(g.bin_size),
            _ => Err(FsError::NotFound),
        }
    }

    fn read_at(&mut self, path: &str, offset: u32, buf: &mut [u8]) -> Result<usize, FsError> {
        match find(path)? {
            Node::Info(g) => {
                let from = (offset as usize).min(g.info.len());
                let n = buf.len().min(g.info.len() - from);
                buf[..n].copy_from_slice(&g.info[from..from + n]);
                Ok(n)
            }
            Node::Bin(_) => Err(FsError::Unsupported),
            _ => Err(FsError::NotFound),
        }
    }
}
