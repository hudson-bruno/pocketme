//! Prints the game slots as linker-script symbols (`game_api::layout`).
//!
//! games/build-chip.sh runs it and links every game with what it prints,
//! so the games' slot numbers come from `game_api::layout` and nowhere
//! else:
//!
//! ```sh
//! cargo run --example slots_x > slots.x
//! cargo run --example slots_x -- --flash MERCATOR > flash-place.x
//! ```
//!
//! With `--flash NAME` it prints that game's flash place instead (the
//! `flash-place.x` of its second link), and fails for a game that has no
//! place in `layout::FLASH_PLACES`.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [] => {
            let mut text = String::from(
                "/* Where a game lives in the chip's memory. Made by games/build-chip.sh\n \
                 * from crates/game-api (layout); do not edit. */\n",
            );
            game_api::layout::write_linker_symbols(&mut text).expect("a String takes any text");
            print!("{text}");
        }
        [flag, name] if flag == "--flash" => {
            let mut text = format!(
                "/* Where {name}'s flash part lies. Made by games/build-chip.sh\n \
                 * from crates/game-api (layout::FLASH_PLACES); do not edit. */\n"
            );
            match game_api::layout::write_flash_place_symbols(name, &mut text) {
                Some(r) => r.expect("a String takes any text"),
                None => {
                    eprintln!(
                        "slots_x: {name} has no flash place: add it to game_api::layout::FLASH_PLACES"
                    );
                    std::process::exit(1);
                }
            }
            print!("{text}");
        }
        _ => {
            eprintln!("usage: slots_x [--flash NAME]");
            std::process::exit(2);
        }
    }
}
