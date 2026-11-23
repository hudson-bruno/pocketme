//! Tells the linker where the game goes.
//!
//! The slot addresses are written once, in the `game-api` crate. This
//! script copies them into a small linker file, so that `game.x` (written
//! by hand, next to this file) can use them by name.

use std::{env, fs, path::PathBuf};

use game_api::{API_VERSION, layout};

fn main() {
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let here = env::var("CARGO_MANIFEST_DIR").unwrap();

    let addresses = format!(
        "/* Made by build.rs from game-api/src/lib.rs. Do not edit. */\n\
         GAME_CODE_ORIGIN = {:#010x};\n\
         GAME_CODE_LENGTH = {:#x};\n\
         GAME_DATA_ORIGIN = {:#010x};\n\
         GAME_DATA_LENGTH = {:#x};\n\
         GAME_API_VERSION = {};\n",
        layout::CODE_SLOT_ADDR,
        layout::CODE_SLOT_SIZE,
        layout::DATA_SLOT_ADDR,
        layout::DATA_SLOT_SIZE,
        API_VERSION,
    );
    fs::write(out.join("game-slot-addresses.x"), addresses).unwrap();

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=game.x");
    // Where the linker finds the generated file that game.x includes.
    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rustc-link-arg=-T{here}/game.x");
    // A map file: which function and which table went to which address.
    println!("cargo:rustc-link-arg=-Wl,-Map={here}/target/hello-game.map");
}
