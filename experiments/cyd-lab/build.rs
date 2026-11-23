//! Build script: the game slots and the linker scripts.

/// The two places in RAM kept free for a game. The same numbers as
/// experiments/native-loader/game-api/src/lib.rs (`layout`), the layout
/// proven on this board. A game built for other numbers will not run, so
/// change them only together with the game interface.
///
/// Code slot: executable RAM (IRAM), 64 KB, up to the end of IRAM.
const CODE_SLOT_ADDR: u32 = 0x4009_0000;
const CODE_SLOT_SIZE: u32 = 0x1_0000;
/// Measuring only (feature `radio-fit`): the radio's RAM code does not
/// leave room for the 64 KB code slot (4,220 bytes too many, measured
/// 2026-11-17). So that the rest can still be measured on the board, a
/// radio-fit build starts the code slot 4,352 bytes later: 61,184 bytes
/// instead of 65,536. A game built for the normal slot cannot run on it.
const RADIO_FIT_CODE_SLOT_ADDR: u32 = 0x4009_1100;
/// Data slot: data RAM (DRAM), 48 KB, up to the end of DRAM.
const DATA_SLOT_ADDR: u32 = 0x3FFF_4000;
const DATA_SLOT_SIZE: u32 = 0xC000;

fn main() {
    linker_be_nice();
    check_xtensa_linker_available();
    let here = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());

    let radio_fit = std::env::var_os("CARGO_FEATURE_RADIO_FIT").is_some();
    // Lab: the second core's cache RAM holds the radio's PHY code, so the
    // normal 64 KB slot fits again (measured in this lab).
    let cpu1_cache = std::env::var_os("CARGO_FEATURE_CPU1_CACHE_RAM").is_some();
    let (code_addr, code_size) = if radio_fit && !cpu1_cache {
        let end = CODE_SLOT_ADDR + CODE_SLOT_SIZE;
        (RADIO_FIT_CODE_SLOT_ADDR, end - RADIO_FIT_CODE_SLOT_ADDR)
    } else {
        (CODE_SLOT_ADDR, CODE_SLOT_SIZE)
    };

    // The slot numbers as linker symbols, for game-slots.x.
    let addresses = format!(
        "/* Made by build.rs. Do not edit. */\n\
         GAME_CODE_ORIGIN = {code_addr:#010x};\n\
         GAME_CODE_LENGTH = {code_size:#x};\n\
         GAME_DATA_ORIGIN = {DATA_SLOT_ADDR:#010x};\n\
         GAME_DATA_LENGTH = {DATA_SLOT_SIZE:#x};\n",
    );
    std::fs::write(out.join("game-slot-addresses.x"), addresses).unwrap();
    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=game-slots.x");
    println!("cargo:rustc-link-arg=-T{here}/game-slots.x");
    // The heap size in main.rs comes from the data slot's address.
    println!("cargo:rustc-env=CYD_DATA_SLOT_ADDR={DATA_SLOT_ADDR}");
    println!("cargo:rustc-env=CYD_DATA_SLOT_SIZE={DATA_SLOT_SIZE}");
    println!("cargo:rustc-env=CYD_CODE_SLOT_ADDR={code_addr}");
    println!("cargo:rustc-env=CYD_CODE_SLOT_SIZE={code_size}");
    // The features of this build, as one line for the log and stats.
    let features: Vec<&str> = [
        "demo-card",
        "sd-card",
        "touch",
        "link",
        "tour",
        "wide-strips",
        "radio-fit",
        "cpu1-cache-ram",
        "cpu1-wipe-test",
        "cpu1-start-test",
        "core2",
        "core2-swap",
        "fs-bench",
    ]
    .into_iter()
    .filter(|f| {
        let var = format!("CARGO_FEATURE_{}", f.to_uppercase().replace('-', "_"));
        std::env::var_os(var).is_some()
    })
    .collect();
    println!("cargo:rustc-env=CYD_FEATURES={}", features.join(" "));

    // Lab (core2): the second core's stack in bytes. Measuring only:
    // CYD_CORE2_STACK=<bytes> tries a smaller one (default 8192, as in
    // esp-rtos's own example).
    println!("cargo:rerun-if-env-changed=CYD_CORE2_STACK");
    let core2_stack = std::env::var("CYD_CORE2_STACK").unwrap_or_else(|_| "8192".into());
    assert!(
        core2_stack
            .parse::<usize>()
            .is_ok_and(|n| n >= 1024 && n % 16 == 0),
        "CYD_CORE2_STACK must be a multiple of 16, at least 1024"
    );
    println!("cargo:rustc-env=CYD_CORE2_STACK={core2_stack}");

    // Lab: a code section in the second core's cache RAM (cpu1-cache.x).
    // Read before linkall.x, like game-slots.x.
    println!("cargo:rerun-if-changed=cpu1-cache.x");
    if cpu1_cache {
        println!("cargo:rustc-link-arg=-T{here}/cpu1-cache.x");
        if !radio_fit {
            println!(
                "cargo:rustc-link-arg=-Wl,-Map={}",
                out.join("cpu1-cache.map").display()
            );
        }
    }

    // radio-fit: four ROM addresses the radio's library needs and the
    // ROM table leaves out (the file of experiments/bt-classic-probe,
    // used from there, read only).
    if radio_fit {
        // A memory map of the build, to see what the radio puts where.
        println!(
            "cargo:rustc-link-arg=-Wl,-Map={}",
            out.join("radio-fit.map").display()
        );
        let rom = format!("{here}/../../experiments/bt-classic-probe/esp32-rom-bluetooth.x");
        println!("cargo:rerun-if-changed={rom}");
        println!("cargo:rustc-link-arg=-T{rom}");
    }

    // linkall.x must be the last linker script.
    println!("cargo:rustc-link-arg=-Tlinkall.x");
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
