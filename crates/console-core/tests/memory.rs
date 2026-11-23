//! RAM the console needs, next to the renderer's buffers. The console
//! lives in one `static` on the chip; these numbers are its whole state.
//! Home's game list and tile pictures are not in it: they are in the RAM
//! the board lends while no game is loaded (on the chip, the empty game
//! data slot; see `console_core::lent`).

use console_core::Console;
use console_core::art::{Art, SLOT_BYTES};
use console_core::console_video::Video;
use console_core::game::Run;
use console_core::game_api::layout::DATA_SLOT_SIZE;
use console_core::home::{Home, LENT_BYTES, MAX_GAMES};
use console_core::input::InputState;
use console_core::lent::{GAME_BYTES, LIST_BYTES, PICTURES_BYTES};
use console_core::notify::Notices;
use console_core::theme::ART_SLOTS;

#[test]
fn memory_stays_inside_its_budget() {
    let video = size_of::<Video>();
    let console = size_of::<Console>();
    let home = size_of::<Home>();
    println!("Video {video} bytes (the renderer's two pictures, palette, areas)");
    println!(
        "Console {console} bytes in all; its own state {} bytes",
        console - video
    );
    println!(
        "  Home {home} (selection, scroll, card state; of it Art {}: which picture is in which slot)",
        size_of::<Art>()
    );
    println!("  Notices {}", size_of::<Notices>());
    println!("  InputState {}", size_of::<InputState>());
    println!(
        "  Run {} (a running game; 5,440 of it keep the picture under the overlays)",
        size_of::<Run>()
    );
    println!(
        "Lent RAM home uses: {LENT_BYTES} of {DATA_SLOT_SIZE} bytes \
         (stamp 4, game list {LIST_BYTES} = {MAX_GAMES} x {GAME_BYTES}, \
         tile pictures {PICTURES_BYTES} = {ART_SLOTS} x {SLOT_BYTES}); {} left",
        DATA_SLOT_SIZE as usize - LENT_BYTES
    );
    assert!(video <= 40 * 1024, "Video is {video} bytes");
    // A running game: under 5.7 KB (the last input handed to the game,
    // with four players, is 176 bytes of it).
    assert!(
        size_of::<Run>() <= 5760,
        "Run is {} bytes",
        size_of::<Run>()
    );
    // Home in the console: no list, no pictures, only where it is.
    assert!(home <= 128, "Home is {home} bytes");
    assert!(size_of::<Art>() <= 16, "Art is {} bytes", size_of::<Art>());
    // Home's data fits the game data slot with room to spare.
    assert!(LENT_BYTES <= DATA_SLOT_SIZE as usize / 4);
    // The console's own state: under 1 KB beside the renderer and the
    // game's part.
    assert!(
        console <= video + size_of::<Run>() + 1024,
        "Console is {console} bytes"
    );
}
