//! The games linked into the simulator.
//!
//! On the chip a game is a file on the card (GAME.BIN) that the loader
//! copies into memory. On the PC the same game source is linked into this
//! program instead, and the board finds it by its card folder name. The
//! home screen still lists what is on the card: a game shows up only when
//! the card has its folder (with a GAME.BIN and an INFO.TXT), and it
//! starts only when it is also in this list. A folder on the card with no
//! game here shows "GAME NOT FOUND".
//!
//! To add a game: make the crate in games/ (see games/README.md), add it
//! to Cargo.toml, add one line below, and give it a folder on the sample
//! card (`LINKED` in tools/make_sample_card.py, then run it).

use game_api::LinkedGame;

/// Every linked game, by its folder on the card.
// SAFETY (each line): the entry function is made by `game_kit::game!`.
pub static GAMES: &[LinkedGame] = &[
    unsafe { LinkedGame::new("BENCH", bench::game_entry) },
    unsafe { LinkedGame::new("CAMBER", camber::game_entry) },
    unsafe { LinkedGame::new("DRIVE3D", drive3d::game_entry) },
    unsafe { LinkedGame::new("HAIRPIN", hairpin::game_entry) },
    unsafe { LinkedGame::new("KESTREL", kestrel::game_entry) },
    unsafe { LinkedGame::new("LANDER", lander::game_entry) },
    unsafe { LinkedGame::new("LANTERN", lantern::game_entry) },
    unsafe { LinkedGame::new("MERCATOR", mercator::game_entry) },
    unsafe { LinkedGame::new("PADTEST", padtest::game_entry) },
    unsafe { LinkedGame::new("PROTO3D", proto3d::game_entry) },
    unsafe { LinkedGame::new("ROOFTOP", rooftop::game_entry) },
    unsafe { LinkedGame::new("SKIMMER", skimmer::game_entry) },
    unsafe { LinkedGame::new("STARDUST", stardust::game_entry) },
    unsafe { LinkedGame::new("TUNDRA", tundra::game_entry) },
];
