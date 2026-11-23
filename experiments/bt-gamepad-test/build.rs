//! Linker set-up, as in bt-classic-probe (whose ROM address file is used
//! from there, not copied).

fn main() {
    let here = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let rom = format!("{here}/../bt-classic-probe/esp32-rom-bluetooth.x");
    println!("cargo:rerun-if-changed={rom}");
    println!("cargo:rustc-link-arg=-T{rom}");
    // Board tests: addresses passed at build time, never kept in a file.
    println!("cargo:rerun-if-env-changed=PAD_PEER");
    println!("cargo:rerun-if-env-changed=PAD_TEST_PAGE");
    // linkall.x must be the last linker script.
    println!("cargo:rustc-link-arg=-Tlinkall.x");
}
