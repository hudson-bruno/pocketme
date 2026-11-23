//! A new `Console` is all zero bytes, so a `static` holding it lands in
//! `.bss` (see the README, "Starting as zeros").
//!
//! Why it matters: a program's variables that start with some nonzero
//! byte go in `.data`. The whole value is then stored in the flash and
//! copied to RAM at start-up. Variables that start as all zeros go in
//! `.bss`: nothing is stored, the start-up code just clears that RAM.
//! The console is about 46 KB, so on the chip this saves 46 KB of flash
//! and a 46 KB copy at every start.
//!
//! The test asks the compiler itself. It puts a new `Console` in a
//! `static` of this test program, then reads this program's own file
//! (Linux keeps it in ELF format) and looks up which section that
//! `static` was put in. The compiler makes the same choice for the chip.
//! One nonzero byte anywhere in `Console::new()` puts it in `.data`.

/// A new console, in a `static` with a name easy to find.
///
/// `static mut`: a plain `static` that is never changed may go to the
/// read-only section (`.rodata`) instead, which says nothing. Nothing
/// here ever reads or writes it, so no `unsafe` is needed.
#[used]
#[unsafe(no_mangle)]
static mut ZERO_CHECK_CONSOLE: console_core::Console = console_core::Console::new();

/// Little-endian numbers out of the file.
fn u16_at(b: &[u8], at: usize) -> usize {
    u16::from_le_bytes(b[at..at + 2].try_into().unwrap()).into()
}

fn u32_at(b: &[u8], at: usize) -> usize {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap()) as usize
}

fn u64_at(b: &[u8], at: usize) -> usize {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap()) as usize
}

/// A zero-ended name out of a table of names.
fn name_at(b: &[u8], at: usize) -> &[u8] {
    let end = b[at..].iter().position(|&c| c == 0).unwrap();
    &b[at..at + end]
}

/// The name of the section that holds the symbol `wanted`, read from a
/// 64-bit little-endian ELF file.
///
/// The file starts with a header that says where the list of sections
/// is. One section is the symbol table: for each name (a function, a
/// `static`), the number of the section it lives in.
fn section_of(elf: &[u8], wanted: &[u8]) -> String {
    assert_eq!(&elf[..4], b"\x7fELF", "not an ELF file");
    assert_eq!((elf[4], elf[5]), (2, 1), "not 64-bit little-endian");
    // Where the section list is, how big one entry is, how many there
    // are, and which section holds the section names.
    let list = u64_at(elf, 0x28);
    let entry = u16_at(elf, 0x3A);
    let count = u16_at(elf, 0x3C);
    let names = u16_at(elf, 0x3E);
    // One section's entry: (name, kind, where in the file, size, link).
    let section = |i: usize| {
        let s = list + i * entry;
        (
            u32_at(elf, s),
            u32_at(elf, s + 4),
            u64_at(elf, s + 0x18),
            u64_at(elf, s + 0x20),
            u32_at(elf, s + 0x28),
        )
    };
    let section_names = section(names).2;

    // Kind 2 is the symbol table. Its link is the section with the
    // symbol names. Each symbol is 24 bytes: name at 0, section at 6.
    let (_, _, table, size, link) = (0..count)
        .map(section)
        .find(|s| s.1 == 2)
        .expect("no symbol table: was the test program stripped?");
    let symbol_names = section(link).2;
    for sym in (table..table + size).step_by(24) {
        if name_at(elf, symbol_names + u32_at(elf, sym)) == wanted {
            let (name, ..) = section(u16_at(elf, sym + 6));
            return String::from_utf8_lossy(name_at(elf, section_names + name)).into_owned();
        }
    }
    panic!("symbol not found")
}

#[test]
#[cfg(target_os = "linux")]
fn a_new_console_is_all_zeros_so_its_static_needs_no_flash() {
    let me = std::fs::read("/proc/self/exe").unwrap();
    let section = section_of(&me, b"ZERO_CHECK_CONSOLE");
    assert!(
        section.starts_with(".bss"),
        "a new Console has a nonzero byte: its static went to {section}, not .bss"
    );
}
