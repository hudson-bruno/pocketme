//! Build script: the game slots, the linker scripts and the demo card's
//! games.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use game_api::{Crc32, GameHeader, layout, read_game_header};

fn main() {
    linker_be_nice();
    check_xtensa_linker_available();
    let here = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let feature = |f: &str| {
        let var = format!("CARGO_FEATURE_{}", f.to_uppercase().replace('-', "_"));
        std::env::var_os(var).is_some()
    };
    let radio = feature("pad");

    // The two game slots, as linker symbols for game-slots.x. The numbers
    // come from game_api::layout and nowhere else (the games are linked
    // with the same text, see games/build-chip.sh). Every build, the
    // radio (pad) one too, has the same slots.
    let mut addresses =
        String::from("/* Made by build.rs from game_api::layout. Do not edit. */\n");
    layout::write_linker_symbols(&mut addresses).unwrap();
    std::fs::write(out.join("game-slot-addresses.x"), addresses).unwrap();
    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=game-slots.x");
    println!("cargo:rustc-link-arg=-T{here}/game-slots.x");
    // The features of this build, as one line for the log and stats.
    let features: Vec<&str> = [
        "demo-card",
        "sd-card",
        "touch",
        "link",
        "tour",
        "wide-strips",
        "pad",
        "store",
        "core2",
        "send-ab",
        "hang-test",
        "xip-probe",
        "boot-gather",
        "game-flash",
        "pair",
    ]
    .into_iter()
    .filter(|f| feature(f))
    .collect();
    println!("cargo:rustc-env=CYD_FEATURES={}", features.join(" "));
    // Measuring only: the flash code probe (src/xip_probe.rs).
    if feature("xip-probe") {
        println!("cargo:warning=MEASURING BUILD: xip-probe");
    }

    // The demo card's real games (only when the demo card is the card).
    if feature("demo-card") && !feature("sd-card") {
        demo_games(&here, &out);
    }

    // pad (the radio): four ROM addresses the radio's library needs and
    // the ROM table leaves out (the file of experiments/bt-classic-probe,
    // used from there, read only).
    if radio {
        // A memory map of the build, to see what the radio puts where.
        println!(
            "cargo:rustc-link-arg=-Wl,-Map={}",
            out.join("radio.map").display()
        );
        let rom = format!("{here}/../../experiments/bt-classic-probe/esp32-rom-bluetooth.x");
        println!("cargo:rerun-if-changed={rom}");
        println!("cargo:rustc-link-arg=-T{rom}");
        // Part of the radio chip's set-up code into the RTC fast memory,
        // to leave room in the executable RAM below the game slot.
        println!("cargo:rerun-if-changed=rtc-fast-phy.x");
        println!("cargo:rustc-link-arg=-T{here}/rtc-fast-phy.x");
    }

    // Game flash parts (README "Game flash parts"): two windows at the
    // very start of the firmware's flash code and read-only data, with
    // the parts of the games that have one. game-flash.x must come
    // before hot-block.x and linkall.x (its sections are inserted
    // before esp-hal's `.text` and after its `.flash.appdesc`).
    if feature("game-flash") {
        game_flash(&here, &out);
        println!("cargo:rustc-link-arg=-Tgame-flash.x");
    }

    // The timed screen paths in one block of flash (and, for the lines
    // that say so, in RAM): hot-block.x, made from hot-block.list. It
    // must come before linkall.x, so that its patterns are matched
    // before esp-hal's catch-all.
    hot_block(&here, &out, feature("core2"));
    println!("cargo:rustc-link-arg=-Thot-block.x");

    // linkall.x must be the last linker script.
    println!("cargo:rustc-link-arg=-Tlinkall.x");
}

/// The smallest the hot block's parts may be, in bytes: (present part,
/// window part), for two cores and for one. Below a floor the link
/// stops: an empty block, or a big function gone from it. Measured on the
/// PC in every feature set boards/cyd/check.sh builds (2026-11-18, before
/// the beam logic moved to send-plan, which left the sizes unchanged):
/// present part 6,667 to 7,280 bytes with two cores and 4,795 to 4,804
/// with one; window part 13,513 to 13,720 with two cores and 14,569 to
/// 14,904 with one. Each floor leaves 700 to 2,300 bytes of room. A
/// single small function lost (the smallest `must` ones are about 130
/// bytes) is not caught by a floor; the framing symbols of each `must`
/// line catch that.
struct HotFloors {
    two: (u32, u32),
    one: (u32, u32),
}
const HOT_FLOORS: HotFloors = HotFloors {
    two: (5120, 12288),
    one: (4096, 12288),
};

/// Writes hot-block.x from hot-block.list (README "The timed paths and
/// the flash cache"): one output section `.text_hot` right after the
/// flash code (`.text`) with the `present` and then the `window`
/// functions of this build's core mode, each with its literal pool just
/// before its code, and one `.rwtext_hot` right after the executable
/// RAM code (`.rwtext.wifi`) for the `ram` lines.
///
/// Both sections give their address themselves and name no memory
/// region: this script is read before esp-hal's memory.x, where the
/// regions are declared (like game-slots.x). Being their own output
/// sections also keeps the linker from sharing their literals with
/// other code: every literal of the block stays in the block.
///
/// The script also stops the link (so a plain `cargo build` fails, not
/// only check.sh's hot-check.py) when
/// - a `must` line of this core mode placed nothing (a renamed or newly
///   inlined function: each line is framed by two symbols);
/// - the window part covers more than 512 cache lines (16 KB, one way of
///   the cache) or the block is over 32 KB (both ways);
/// - a part is smaller than its floor ([`HOT_FLOORS`]), which catches an
///   empty block or a big function gone missing;
/// - the block is not inside the flash code region (ROTEXT).
///
/// Measuring only: `CYD_LAYOUT_PAD=<before>[,<between>]` (bytes,
/// multiples of 4) puts that much empty space in front of the block,
/// which moves it against all other code and the read-only data, and
/// between its `present` and `window` parts, which moves its functions
/// against each other. Unset (the default) it adds nothing.
fn hot_block(here: &str, out: &Path, core2: bool) {
    let list = format!("{here}/hot-block.list");
    println!("cargo:rerun-if-changed={list}");
    println!("cargo:rerun-if-env-changed=CYD_LAYOUT_PAD");
    let text = std::fs::read_to_string(&list).unwrap_or_else(|e| panic!("{list}: {e}"));
    let (mut present, mut window, mut ram) = (String::new(), String::new(), String::new());
    let mut musts = String::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split_whitespace().collect();
        let [place, cores, need, glob] = cols[..] else {
            panic!("{list}:{}: want `place cores need glob`", n + 1);
        };
        assert!(
            matches!(need, "must" | "may")
                && glob
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_*?".contains(&b)),
            "{list}:{}: bad line",
            n + 1
        );
        let here_too = match cores {
            "all" => true,
            "two" => core2,
            "one" => !core2,
            _ => panic!("{list}:{}: cores must be all, two or one", n + 1),
        };
        if !here_too {
            continue;
        }
        let mut rule = format!("    *(.literal.{glob} .text.{glob})\n");
        if need == "must" {
            // Two symbols around the line; the line must place something.
            let n = n + 1;
            rule = format!(
                "    _cyd_hot_line_{n}_start = ABSOLUTE(.);\n{rule}    _cyd_hot_line_{n}_end = ABSOLUTE(.);\n"
            );
            writeln!(
                musts,
                "ASSERT(_cyd_hot_line_{n}_end > _cyd_hot_line_{n}_start,\n\
                 \x20      \"HOT BLOCK: hot-block.list line {n} ({place} must) placed nothing: `{glob}` matches no function of this build. Fix the line (hot-check.py names the rest).\")"
            )
            .unwrap();
        }
        match place {
            "present" => present.push_str(&rule),
            "window" => window.push_str(&rule),
            "ram" => ram.push_str(&rule),
            _ => panic!("{list}:{}: place must be present, window or ram", n + 1),
        }
    }
    let (before, between) = match std::env::var("CYD_LAYOUT_PAD") {
        Ok(v) if !v.is_empty() => {
            let mut it = v.split(',').map(|n| {
                let n: u32 = n
                    .trim()
                    .parse()
                    .unwrap_or_else(|_| panic!("CYD_LAYOUT_PAD: `{v}` is not <bytes>[,<bytes>]"));
                assert!(
                    n.is_multiple_of(4),
                    "CYD_LAYOUT_PAD: {n} is not a multiple of 4"
                );
                n
            });
            let before = it.next().unwrap_or(0);
            let between = it.next().unwrap_or(0);
            println!("cargo:warning=MEASURING BUILD: CYD_LAYOUT_PAD={before},{between}");
            (before, between)
        }
        _ => (0, 0),
    };
    let pad = |n: u32| {
        if n == 0 {
            String::new()
        } else {
            format!("    . += {n}; /* CYD_LAYOUT_PAD: measuring only */\n")
        }
    };
    let (before, between) = (pad(before), pad(between));
    let (mode, (present_min, window_min)) = if core2 {
        ("two cores", HOT_FLOORS.two)
    } else {
        ("one core", HOT_FLOORS.one)
    };
    let script = format!(
        "/* Made by build.rs from hot-block.list. Do not edit. */\n\
         SECTIONS\n{{\n  .text_hot ADDR(.text) + SIZEOF(.text) :\n  {{\n\
         \x20   . = ALIGN(32);\n{before}    _cyd_hot_start = ABSOLUTE(.);\n{present}{between}\
         \x20   _cyd_hot_window = ABSOLUTE(.);\n{window}\
         \x20   _cyd_hot_end = ABSOLUTE(.);\n  }}\n}}\nINSERT AFTER .text;\n\n\
         SECTIONS\n{{\n  .rwtext_hot ALIGN(ADDR(.rwtext.wifi) + SIZEOF(.rwtext.wifi), 4) :\n  {{\n\
         \x20   _cyd_hot_ram_start = ABSOLUTE(.);\n{ram}\
         \x20   . = ALIGN(4);\n    _cyd_hot_ram_end = ABSOLUTE(.);\n  }}\n}}\nINSERT AFTER .rwtext.wifi;\n\n\
         /* The end of all the console's own RAM code (mem.rs: iram_left). */\n\
         _cyd_iram_end = MAX(_rwtext_end, _cyd_hot_ram_end);\n\
         ASSERT(_cyd_iram_end <= GAME_CODE_ORIGIN,\n\
         \x20      \"GAME SLOT: the console's RAM code (with hot-block.list's ram lines) has grown into the game's code slot. Move lines from ram to the flash block.\")\n\n\
         /* The hot block's guards (build.rs hot_block). */\n\
         ASSERT(_cyd_hot_start >= ORIGIN(ROTEXT) && _cyd_hot_end <= ORIGIN(ROTEXT) + LENGTH(ROTEXT),\n\
         \x20      \"HOT BLOCK: .text_hot is not inside the flash code region (ROTEXT).\")\n\
         ASSERT((_cyd_hot_end - 1) / 32 - _cyd_hot_window / 32 + 1 <= 512,\n\
         \x20      \"HOT BLOCK: the window part covers more than 512 flash cache lines (16 KB, one way of the cache): its lines would push each other out. Move lines to the present part or make the code smaller.\")\n\
         ASSERT(_cyd_hot_end - _cyd_hot_start <= 32768,\n\
         \x20      \"HOT BLOCK: the block is over 32 KB (both ways of the flash cache): three of its lines would share a set.\")\n\
         ASSERT(_cyd_hot_window - _cyd_hot_start >= {present_min},\n\
         \x20      \"HOT BLOCK: the present part is under {present_min} bytes ({mode}): a line of hot-block.list probably stopped matching (hot-check.py names it). If the code really shrank, lower HOT_FLOORS in build.rs.\")\n\
         ASSERT(_cyd_hot_end - _cyd_hot_window >= {window_min},\n\
         \x20      \"HOT BLOCK: the window part is under {window_min} bytes ({mode}): a line of hot-block.list probably stopped matching (hot-check.py names it). If the code really shrank, lower HOT_FLOORS in build.rs.\")\n\
         {musts}",
    );
    std::fs::write(out.join("hot-block.x"), script).unwrap();
}

/// The end marks in the last word of each window: "GFCW" (code) and
/// "GFRW" (read-only data), little-endian. They give each window real
/// contents up to its very end, so the window, esp-hal's `.text` (or
/// `.rodata`) and everything between stay one flash segment.
const FLASH_CODE_END_MARK: u32 = u32::from_le_bytes(*b"GFCW");
const FLASH_RO_END_MARK: u32 = u32::from_le_bytes(*b"GFRW");

/// One flash part found in `<game files>/flash/<NAME>/`.
struct HeldPart {
    place: &'static layout::FlashPlace,
    code: PathBuf,
    code_len: u32,
    code_crc: u32,
    ro: PathBuf,
    ro_len: u32,
    ro_crc: u32,
}

/// The CRC-32 of a game file (game_api::Crc32), 0 for no bytes.
fn crc32_of(bytes: &[u8]) -> u32 {
    if bytes.is_empty() {
        return 0;
    }
    let mut crc = Crc32::new();
    crc.update(bytes);
    crc.value()
}

/// Feature `game-flash` (README "Game flash parts"). Writes
/// - `game-flash-addresses.x`: the two windows from game_api::layout;
/// - `game_flash.rs` (included by src/games.rs): one `#[used]` static per
///   held part in the input section `.game_flash_code.<NAME>` or
///   `.game_flash_ro.<NAME>`, and `HELD`, what the console compares a
///   game's flash part with;
/// - `game-flash.x`: `.game_flash_code` (the whole code window, inserted
///   before esp-hal's `.text` in `irom_seg`) and `.game_flash_ro` (the
///   whole data window, after `.flash.appdesc` in `drom_seg`), each part
///   at its place, zeros between, an end mark in the last word, and
///   ASSERTs that stop the link when a window is not where layout says or
///   does not run straight into esp-hal's own section.
///
/// The parts come from `<game files>/flash/<NAME>/{CODE,RO}.BIN` for every
/// game in `layout::FLASH_PLACES` (games/build-chip.sh writes them). When
/// that game's `<NAME>.BIN` lies beside them, its flash-part extension
/// must name exactly these bytes, or the build stops: the two would come
/// from different game builds. No files: a warning, empty windows (the
/// layout still moves), and every game with a flash part is refused.
///
/// The region names must be esp-hal's `irom_seg`/`drom_seg`: its aliases
/// `ROTEXT`/`RODATA` are not declared yet when this script is read, and
/// naming them is a fatal "redefinition of memory region alias". The
/// linker warns about the two names (README "Game flash parts").
fn game_flash(here: &str, out: &Path) {
    let mut addresses =
        String::from("/* Made by build.rs from game_api::layout. Do not edit. */\n");
    layout::write_flash_window_symbols(&mut addresses).unwrap();
    std::fs::write(out.join("game-flash-addresses.x"), addresses).unwrap();

    let files = game_files(here);
    let dir = files.join("flash");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut held = Vec::new();
    for place in layout::FLASH_PLACES {
        let name = place.game;
        let part = dir.join(name);
        let code = part.join("CODE.BIN");
        let ro = part.join("RO.BIN");
        println!("cargo:rerun-if-changed={}", code.display());
        println!("cargo:rerun-if-changed={}", ro.display());
        let Ok(code_bytes) = std::fs::read(&code) else {
            println!(
                "cargo:warning=game-flash: no flash part for {name} in {} (build the games first: GAMES_TARGET={here}/target {here}/../../games/build-chip.sh); {name} will be refused",
                part.display()
            );
            continue;
        };
        let ro_bytes = std::fs::read(&ro).unwrap_or_else(|e| panic!("{}: {e}", ro.display()));
        let (code_len, ro_len) = (code_bytes.len() as u32, ro_bytes.len() as u32);
        assert!(
            code_len > 0
                && code_len.is_multiple_of(4)
                && code_len <= place.code_max
                && ro_len.is_multiple_of(4)
                && ro_len <= place.ro_max,
            "{}: code {code_len} bytes, data {ro_len} bytes: not whole words, empty code, or bigger than {name}'s place in game_api::layout (code {}, data {}). Build the games again: GAMES_TARGET={here}/target {here}/../../games/build-chip.sh",
            part.display(),
            place.code_max,
            place.ro_max
        );
        let (code_crc, ro_crc) = (crc32_of(&code_bytes), crc32_of(&ro_bytes));
        // The game file built with these parts must name exactly them.
        let bin = files.join(format!("{name}.BIN"));
        println!("cargo:rerun-if-changed={}", bin.display());
        match std::fs::read(&bin) {
            Ok(bytes) => {
                let ext = match read_game_header(&bytes) {
                    Ok((_, Some(ext))) => ext,
                    other => panic!(
                        "\n\n{}: not a game file with a flash part ({other:?}), but {} holds one.\n\
                         Build the games again: GAMES_TARGET={here}/target {here}/../../games/build-chip.sh\n\n",
                        bin.display(),
                        part.display()
                    ),
                };
                let fits = ext.code_addr == place.code_addr()
                    && ext.code_size == code_len
                    && ext.code_crc32 == code_crc
                    && ext.ro_size == ro_len
                    && ext.ro_crc32 == ro_crc
                    && (ro_len == 0 || ext.ro_addr == place.ro_addr());
                assert!(
                    fits,
                    "\n\n{}: its flash part (code {:#x} + {} crc32={:#010x}, data {:#x} + {} crc32={:#010x}) is not the one in {} \
                     (code {:#x} + {code_len} crc32={code_crc:#010x}, data {:#x} + {ro_len} crc32={ro_crc:#010x}).\n\
                     Build the games again: GAMES_TARGET={here}/target {here}/../../games/build-chip.sh\n\n",
                    bin.display(),
                    ext.code_addr,
                    ext.code_size,
                    ext.code_crc32,
                    ext.ro_addr,
                    ext.ro_size,
                    ext.ro_crc32,
                    part.display(),
                    place.code_addr(),
                    place.ro_addr(),
                );
            }
            Err(_) => println!(
                "cargo:warning=game-flash: {} is missing, so {name}'s flash part is not compared with its game file",
                bin.display()
            ),
        }
        held.push(HeldPart {
            place,
            code,
            code_len,
            code_crc,
            ro,
            ro_len,
            ro_crc,
        });
    }

    // game_flash.rs
    let mut rs = String::from(
        "// Made by build.rs (game_flash): the game flash parts this firmware\n\
         // holds. Do not edit. Nothing in Rust reads these statics: they lie in\n\
         // flash (the code ones in the instruction window, which takes only\n\
         // whole words) and are read only through their addresses (games.rs).\n",
    );
    let mut list = String::new();
    for h in &held {
        let name = h.place.game;
        writeln!(
            rs,
            "#[used]\n#[unsafe(link_section = \".game_flash_code.{name}\")]\n\
             static GAME_FLASH_CODE_{name}: [u8; {}] = *include_bytes!({:?});",
            h.code_len,
            h.code.display().to_string()
        )
        .unwrap();
        if h.ro_len > 0 {
            writeln!(
                rs,
                "#[used]\n#[unsafe(link_section = \".game_flash_ro.{name}\")]\n\
                 static GAME_FLASH_RO_{name}: [u8; {}] = *include_bytes!({:?});",
                h.ro_len,
                h.ro.display().to_string()
            )
            .unwrap();
        }
        writeln!(
            list,
            "    Held {{ game: {name:?}, code_addr: {:#010x}, code_len: {}, code_crc: {:#010x}, ro_addr: {:#010x}, ro_len: {}, ro_crc: {:#010x} }},",
            h.place.code_addr(),
            h.code_len,
            h.code_crc,
            h.place.ro_addr(),
            h.ro_len,
            h.ro_crc
        )
        .unwrap();
    }
    writeln!(
        rs,
        "/// The parts this firmware holds.\npub const HELD: &[Held] = &[\n{list}];"
    )
    .unwrap();
    std::fs::write(out.join("game_flash.rs"), rs).unwrap();

    // game-flash.x
    let (mut code_parts, mut ro_parts, mut asserts) = (String::new(), String::new(), String::new());
    for h in &held {
        let p = h.place;
        let name = p.game;
        writeln!(
            code_parts,
            "    . = _game_flash_code_window + {:#x};\n\
             \x20   _game_flash_code_{name}_start = ABSOLUTE(.);\n\
             \x20   KEEP(*(.game_flash_code.{name}))\n\
             \x20   _game_flash_code_{name}_end = ABSOLUTE(.);",
            p.code_off
        )
        .unwrap();
        writeln!(
            asserts,
            "ASSERT(_game_flash_code_{name}_start == {:#010x} && _game_flash_code_{name}_end - _game_flash_code_{name}_start == {},\n\
             \x20      \"GAME FLASH: {name}'s flash code is not at its place in game_api::layout, or not whole\")",
            p.code_addr(),
            h.code_len
        )
        .unwrap();
        if h.ro_len > 0 {
            writeln!(
                ro_parts,
                "    . = _game_flash_ro_window + {:#x};\n\
                 \x20   _game_flash_ro_{name}_start = ABSOLUTE(.);\n\
                 \x20   KEEP(*(.game_flash_ro.{name}))\n\
                 \x20   _game_flash_ro_{name}_end = ABSOLUTE(.);",
                p.ro_off
            )
            .unwrap();
            writeln!(
                asserts,
                "ASSERT(_game_flash_ro_{name}_start == {:#010x} && _game_flash_ro_{name}_end - _game_flash_ro_{name}_start == {},\n\
                 \x20      \"GAME FLASH: {name}'s flash data is not at its place in game_api::layout, or not whole\")",
                p.ro_addr(),
                h.ro_len
            )
            .unwrap();
        }
    }
    let script = format!(
        "/* Made by build.rs (game_flash) from game_api::layout and the held\n   \
         flash parts. Do not edit. README \"Game flash parts\". */\n\
         INCLUDE game-flash-addresses.x\n\n\
         SECTIONS\n{{\n  .game_flash_code :\n  {{\n\
         \x20   _game_flash_code_window = .;\n\
         {code_parts}\
         \x20   . = _game_flash_code_window + GAME_FLASH_CODE_WINDOW_LENGTH - 4;\n\
         \x20   LONG({FLASH_CODE_END_MARK:#010x}) /* \"GFCW\": the window's end mark */\n\
         \x20   _game_flash_code_window_end = .;\n  }} > irom_seg\n}}\nINSERT BEFORE .text;\n\n\
         SECTIONS\n{{\n  .game_flash_ro :\n  {{\n\
         \x20   _game_flash_ro_window = .;\n\
         {ro_parts}\
         \x20   . = _game_flash_ro_window + GAME_FLASH_RO_WINDOW_LENGTH - 4;\n\
         \x20   LONG({FLASH_RO_END_MARK:#010x}) /* \"GFRW\": the window's end mark */\n\
         \x20   _game_flash_ro_window_end = .;\n  }} > drom_seg\n}}\nINSERT AFTER .flash.appdesc;\n\n\
         ASSERT(_game_flash_code_window == GAME_FLASH_CODE_WINDOW_ORIGIN,\n\
         \x20      \"GAME FLASH: the code window is not at game_api::layout's address (did esp-hal's irom_seg move?)\")\n\
         ASSERT(_game_flash_ro_window == GAME_FLASH_RO_WINDOW_ORIGIN,\n\
         \x20      \"GAME FLASH: the data window is not right after the app descriptor at game_api::layout's address (did its size change?)\")\n\
         ASSERT(_game_flash_code_window_end == ADDR(.text),\n\
         \x20      \"GAME FLASH: the firmware's flash code must follow the code window directly (one flash segment)\")\n\
         ASSERT(_game_flash_ro_window_end == ADDR(.rodata_merge),\n\
         \x20      \"GAME FLASH: the firmware's read-only data must follow the data window directly (one flash segment)\")\n\
         {asserts}"
    );
    std::fs::write(out.join("game-flash.x"), script).unwrap();
}

/// Where games/build-chip.sh put the chip game files for this crate:
/// `GAMES_TARGET=boards/cyd/target games/build-chip.sh` (check.sh does it
/// first). `CYD_GAME_FILES` names another folder.
fn game_files(here: &str) -> PathBuf {
    println!("cargo:rerun-if-env-changed=CYD_GAME_FILES");
    match std::env::var_os("CYD_GAME_FILES") {
        Some(dir) => PathBuf::from(dir),
        None => Path::new(here).join("target/chip-games/files"),
    }
}

/// Writes `demo_games.rs`: the real games (GAME.BIN, INFO.TXT and, when
/// there, the tile picture ART.BIN of each folder in
/// `<game files>/card/CONSOLE/GAMES`, built by games/build-chip.sh), each
/// GAME.BIN checked against game_api::layout here, so a game file made
/// for other slots stops the build. ART.BIN is only checked to be a
/// version 1 picture file of the right length; the console checks it
/// fully when it reads it (console-core `art`). Then, for measuring
/// only, the files named in `CYD_DEMO_EXTRA` (`FOLDER=path;FOLDER=path`),
/// not checked at all: test files the console must refuse.
fn demo_games(here: &str, out: &Path) {
    let files = game_files(here);
    let games = files.join("card/CONSOLE/GAMES");
    println!("cargo:rerun-if-changed={}", games.display());
    let mut folders: Vec<PathBuf> = match std::fs::read_dir(&games) {
        Ok(dir) => dir
            .map(|e| e.unwrap().path())
            .filter(|p| p.is_dir())
            .collect(),
        Err(e) => panic!(
            "\n\nThe demo card needs the chip game files, and {} cannot be read ({e}).\n\
             Build them first (boards/cyd/check.sh does):\n\n    \
             GAMES_TARGET={here}/target {here}/../../games/build-chip.sh\n\n",
            games.display()
        ),
    };
    folders.sort();
    assert!(
        !folders.is_empty(),
        "no games in {}: run games/build-chip.sh",
        games.display()
    );
    let mut code = String::from(
        "// Made by build.rs: the demo card's real games. Do not edit.\n\
         const BUILT: &[Game] = &[\n",
    );
    for folder in &folders {
        let name = folder.file_name().unwrap().to_str().unwrap();
        let bin = folder.join("GAME.BIN");
        let info = folder.join("INFO.TXT");
        println!("cargo:rerun-if-changed={}", bin.display());
        println!("cargo:rerun-if-changed={}", info.display());
        let bytes = std::fs::read(&bin).unwrap_or_else(|e| panic!("{}: {e}", bin.display()));
        // A plain file ("GAME") or one with a flash part ("GAMF", whose
        // 32-byte extension follows the header): read_game_header checks
        // the RAM part of both the same way, and the extension's own
        // rules. Whether this firmware holds the flash part is checked at
        // run time (src/games.rs) and, for the parts built beside these
        // files, by game_flash below.
        let checked = read_game_header(&bytes);
        let head = match bytes.first_chunk::<{ GameHeader::SIZE }>() {
            Some(h) => GameHeader::from_bytes(h),
            None => panic!("{}: shorter than a game header", bin.display()),
        };
        if let Err(e) = checked {
            panic!(
                "\n\n{}: {e:?}. It was built for other slots, is too big or is damaged \
                 (game_api::layout: code {:#x} + {}, code at most {} with the spill of {} at {:#x} shared with the data, data {:#x} + {}; \
                 the file: code {:#x} + {} ({} past the slot), data {:#x} + {}).\n\
                 Build the games again: GAMES_TARGET={here}/target {here}/../../games/build-chip.sh\n\n",
                bin.display(),
                layout::CODE_SLOT_ADDR,
                layout::CODE_SLOT_SIZE,
                layout::CODE_MAX,
                layout::CODE_SPILL_MAX,
                layout::CODE_SPILL_ADDR,
                layout::DATA_SLOT_ADDR,
                layout::DATA_SLOT_SIZE,
                head.code_addr,
                head.code_size,
                layout::spill_of(head.code_size),
                head.data_addr,
                head.data_size + head.bss_size,
            );
        }
        let head_size = match checked {
            Ok((_, Some(_))) => game_api::FlashPartHeader::FILE_HEAD,
            _ => GameHeader::SIZE,
        };
        assert_eq!(
            head.payload_size() as usize,
            bytes.len() - head_size,
            "{}: size does not match its header",
            bin.display()
        );
        let art = folder.join("ART.BIN");
        let art = if art.is_file() {
            println!("cargo:rerun-if-changed={}", art.display());
            let a = std::fs::read(&art).unwrap_or_else(|e| panic!("{}: {e}", art.display()));
            // "ART", version 1, then width, height, colours, bits per
            // pixel (games/art/make_art.py describes the format).
            let fits = a.len() >= 8 && &a[..4] == b"ART\x01" && a[7] == 4 && {
                let (w, h, n) = (a[4] as usize, a[5] as usize, a[6] as usize);
                a.len() == 8 + 2 * n + h * w.div_ceil(2)
            };
            assert!(
                fits,
                "{}: not a version 1 tile picture. Build the games again: \
                 GAMES_TARGET={here}/target {here}/../../games/build-chip.sh",
                art.display()
            );
            format!("include_bytes!({:?})", art.display().to_string())
        } else {
            "&[]".to_string()
        };
        writeln!(
            code,
            "    Game {{ folder: {name:?}, info: include_bytes!({:?}), bin: include_bytes!({:?}), art: {art} }},",
            info.display().to_string(),
            bin.display().to_string()
        )
        .unwrap();
    }
    // Measuring only: extra test files (never in a normal build).
    println!("cargo:rerun-if-env-changed=CYD_DEMO_EXTRA");
    if let Ok(extra) = std::env::var("CYD_DEMO_EXTRA") {
        for item in extra.split(';').filter(|i| !i.is_empty()) {
            let (name, path) = item
                .split_once('=')
                .unwrap_or_else(|| panic!("CYD_DEMO_EXTRA: `{item}` is not FOLDER=path"));
            assert!(
                name.len() <= 8
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()),
                "CYD_DEMO_EXTRA: folder `{name}` must be 8 capitals or digits at most"
            );
            let path = std::fs::canonicalize(path).unwrap_or_else(|e| panic!("{path}: {e}"));
            println!("cargo:rerun-if-changed={}", path.display());
            println!(
                "cargo:warning=demo card: TEST FILE {name} = {}",
                path.display()
            );
            let info = format!("NAME={name}\nTAG=TEST FILE\nTILE=802020\n");
            writeln!(
                code,
                "    Game {{ folder: {name:?}, info: {info:?}.as_bytes(), bin: include_bytes!({:?}), art: &[] }},",
                path.display().to_string()
            )
            .unwrap();
        }
    }
    code.push_str("];\n");
    std::fs::write(out.join("demo_games.rs"), code).unwrap();
}

#[cfg(unix)]
fn check_xtensa_linker_available() {
    println!("cargo:rerun-if-env-changed=PATH");
    let target = std::env::var("TARGET").unwrap_or_default();
    println!(
        "cargo:rerun-if-env-changed={}",
        cargo_linker_env_var(&target)
    );
    let linker = xtensa_linker(&target);
    let error = match std::process::Command::new(&linker)
        .arg("--version")
        .output()
    {
        Ok(_) => return,
        Err(error) => error,
    };
    let export_file = std::env::var("HOME")
        .map(|home| format!("{home}/export-esp.sh"))
        .unwrap_or_else(|_| "$HOME/export-esp.sh".to_string());
    panic!(
        "Xtensa linker `{linker}` was not found in PATH or could not be run: {error}.\n\
         Source the esp toolchain's settings first: `. {export_file}`"
    );
}

#[cfg(not(unix))]
fn check_xtensa_linker_available() {}

#[cfg(unix)]
fn xtensa_linker(target: &str) -> String {
    if let Ok(linker) = std::env::var(cargo_linker_env_var(target)) {
        return linker;
    }
    target
        .strip_prefix("xtensa-")
        .and_then(|target| target.strip_suffix("-none-elf"))
        .map(|chip| format!("xtensa-{chip}-elf-gcc"))
        .unwrap_or_else(|| "xtensa-esp32-elf-gcc".to_string())
}

#[cfg(unix)]
fn cargo_linker_env_var(target: &str) -> String {
    let target = target.replace('-', "_").to_ascii_uppercase();
    format!("CARGO_TARGET_{target}_LINKER")
}

/// Turns some link errors into a hint a person can act on.
fn linker_be_nice() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let kind = &args[1];
        let what = &args[2];
        if kind == "undefined-symbol" {
            match what.as_str() {
                "_stack_start" => eprintln!("\nIs the linker script `linkall.x` missing?\n"),
                what if what.starts_with("esp_rtos_") => {
                    eprintln!("\nesp-rtos is not started or not linked.\n")
                }
                "free" | "malloc" | "calloc" => {
                    eprintln!("\nSomething wants a C heap: esp-alloc's `compat` feature is off.\n")
                }
                _ => (),
            }
            std::process::exit(0);
        }
        std::process::exit(1);
    }
    println!(
        "cargo:rustc-link-arg=-Wl,--error-handling-script={}",
        std::env::current_exe().unwrap().display()
    );
}
