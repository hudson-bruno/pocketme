//! Packs a game for the console, and inspects packed games.
//!
//! ```text
//! game-pack pack  <game.elf> <OUT.BIN>   make the file the console loads
//! game-pack check <FILE.BIN>             print a packed file's header and verify it
//! ```
//!
//! `pack` reads the ELF file the linker produced and writes:
//!
//! ```text
//! header (36 bytes) | code (.text) | data (.rodata, then .data)
//! ```
//!
//! It also refuses to pack a game that would not survive loading: a section
//! it does not know, a section outside its slot, or anything other than
//! code in the code slot. The linker script should make all of these
//! impossible; this is the second lock on the same door.

use std::{env, fs, process::ExitCode};

use game_api::{API_VERSION, Crc32, GameHeader, MAGIC, layout};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        [_, "pack", elf, out] => pack(elf, out),
        [_, "check", file] => check(file),
        _ => Err("usage: game-pack pack <game.elf> <OUT.BIN> | game-pack check <FILE.BIN>".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("game-pack: {message}");
            ExitCode::FAILURE
        }
    }
}

/// One section of the ELF file that takes up memory on the chip.
struct Section {
    name: String,
    addr: u32,
    size: u32,
    /// The bytes in the file. Empty for `.bss`, which has none.
    bytes: Vec<u8>,
    executable: bool,
    writable: bool,
    /// True for `.bss`: memory that is zeroed, not loaded.
    zeroed: bool,
}

/// The two section-header flags and one type this tool cares about, as the
/// ELF format numbers them.
const SHF_WRITE: u32 = 1;
const SHF_ALLOC: u32 = 2;
const SHF_EXECINSTR: u32 = 4;
const SHT_NOBITS: u32 = 8;

fn u16_at(bytes: &[u8], at: usize) -> Result<u16, String> {
    bytes
        .get(at..at + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or_else(|| "the ELF file is cut short".to_string())
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32, String> {
    bytes
        .get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| "the ELF file is cut short".to_string())
}

/// Reads the entry address and every section that occupies memory.
///
/// Only what a 32-bit little-endian ELF file needs: that is what the
/// Xtensa linker writes.
fn read_elf(elf: &[u8]) -> Result<(u32, Vec<Section>), String> {
    if elf.get(..4) != Some(b"\x7fELF") {
        return Err("not an ELF file".into());
    }
    if elf.get(4) != Some(&1) || elf.get(5) != Some(&1) {
        return Err("not a 32-bit little-endian ELF file".into());
    }
    let entry = u32_at(elf, 0x18)?;
    let table = u32_at(elf, 0x20)? as usize;
    let entry_size = u16_at(elf, 0x2E)? as usize;
    let count = u16_at(elf, 0x30)? as usize;
    let names_index = u16_at(elf, 0x32)? as usize;

    let header = |index: usize, field: usize| u32_at(elf, table + index * entry_size + field * 4);
    let names_at = header(names_index, 4)? as usize;

    let mut sections = Vec::new();
    for index in 0..count {
        let flags = header(index, 2)?;
        let size = header(index, 5)?;
        if flags & SHF_ALLOC == 0 || size == 0 {
            continue;
        }
        let name_at = names_at + header(index, 0)? as usize;
        let name_len = elf
            .get(name_at..)
            .and_then(|rest| rest.iter().position(|&byte| byte == 0))
            .ok_or("a section name runs off the end of the file")?;
        let name = String::from_utf8_lossy(&elf[name_at..name_at + name_len]).into_owned();
        let zeroed = header(index, 1)? == SHT_NOBITS;
        let offset = header(index, 4)? as usize;
        let bytes = if zeroed {
            Vec::new()
        } else {
            elf.get(offset..offset + size as usize)
                .ok_or_else(|| format!("section {name} runs off the end of the file"))?
                .to_vec()
        };
        sections.push(Section {
            name,
            addr: header(index, 3)?,
            size,
            bytes,
            executable: flags & SHF_EXECINSTR != 0,
            writable: flags & SHF_WRITE != 0,
            zeroed,
        });
    }
    sections.sort_by_key(|section| section.addr);
    Ok((entry, sections))
}

fn pack(elf_path: &str, out_path: &str) -> Result<(), String> {
    let elf = fs::read(elf_path).map_err(|error| format!("cannot read {elf_path}: {error}"))?;
    let (entry, sections) = read_elf(&elf)?;

    let code_slot = layout::CODE_SLOT_ADDR..layout::CODE_SLOT_ADDR + layout::CODE_SLOT_SIZE;
    let data_slot = layout::DATA_SLOT_ADDR..layout::DATA_SLOT_ADDR + layout::DATA_SLOT_SIZE;

    println!("sections that take up memory on the chip:");
    println!("  {:<10} {:>10} {:>7}  {:<5} {}", "name", "address", "bytes", "slot", "kind");

    let mut code = Vec::new();
    let mut data = Vec::new();
    let mut bss_size = 0u32;
    // Where the next section of each slot has to start for the file to
    // have no holes.
    let mut code_end = layout::CODE_SLOT_ADDR;
    let mut data_end = layout::DATA_SLOT_ADDR;

    for section in &sections {
        let end = section.addr.checked_add(section.size).ok_or("a section wraps around")?;
        let in_code = code_slot.contains(&section.addr) && end <= code_slot.end;
        let in_data = data_slot.contains(&section.addr) && end <= data_slot.end;
        let kind = match (section.executable, section.writable, section.zeroed) {
            (true, _, _) => "code",
            (false, false, false) => "constants",
            (false, true, false) => "variables with a value",
            (false, _, true) => "zeroed variables",
        };
        let slot = if in_code {
            "code"
        } else if in_data {
            "data"
        } else {
            "NONE"
        };
        println!("  {:<10} {:#010x} {:>7}  {:<5} {}", section.name, section.addr, section.size, slot, kind);

        match section.name.as_str() {
            ".text" if in_code && section.executable && !section.zeroed => {
                if section.addr != code_end {
                    return Err(format!(".text starts at {:#x}, not at the start of the code slot", section.addr));
                }
                code.extend_from_slice(&section.bytes);
                code_end = end;
            }
            ".rodata" | ".data" if in_data && !section.executable && !section.zeroed => {
                if bss_size != 0 {
                    return Err(format!("{} comes after .bss; the file format needs .bss last", section.name));
                }
                if section.addr != data_end {
                    return Err(format!(
                        "{} starts at {:#x} but the data before it ends at {data_end:#x}: there is a hole",
                        section.name, section.addr
                    ));
                }
                data.extend_from_slice(&section.bytes);
                data_end = end;
            }
            ".bss" if in_data && section.zeroed => {
                if section.addr != data_end {
                    return Err(format!(".bss starts at {:#x}, not where the data ends ({data_end:#x})", section.addr));
                }
                bss_size += section.size;
                data_end = end;
            }
            // Anything else would be silently left out of the file, or
            // land where it cannot be used. Stop here instead.
            other => {
                return Err(format!(
                    "section {other} at {:#x} does not belong in a game file \
                     (only .text in the code slot, and .rodata, .data, .bss in the data slot)",
                    section.addr
                ));
            }
        }
    }

    if code.is_empty() {
        return Err("the game has no code".into());
    }
    if code.len() % 4 != 0 || data.len() % 4 != 0 || bss_size % 4 != 0 {
        return Err("a section size is not a multiple of 4; the linker script should pad them".into());
    }

    let mut header = GameHeader {
        magic: MAGIC,
        api_version: API_VERSION,
        code_addr: layout::CODE_SLOT_ADDR,
        code_size: code.len() as u32,
        data_addr: layout::DATA_SLOT_ADDR,
        data_size: data.len() as u32,
        bss_size,
        entry,
        crc32: 0,
    };
    header.validate().map_err(|error| format!("the game does not fit the console: {error:?}"))?;

    let mut crc = Crc32::new();
    crc.update(&header.to_bytes()[..GameHeader::CHECKED]);
    crc.update(&code);
    crc.update(&data);
    header.crc32 = crc.value();

    let mut file = header.to_bytes().to_vec();
    file.extend_from_slice(&code);
    file.extend_from_slice(&data);
    fs::write(out_path, &file).map_err(|error| format!("cannot write {out_path}: {error}"))?;

    println!();
    print_header(&header);
    println!();
    println!(
        "code slot: {} of {} bytes used ({:.1} %)",
        header.code_size,
        layout::CODE_SLOT_SIZE,
        100.0 * f64::from(header.code_size) / f64::from(layout::CODE_SLOT_SIZE)
    );
    let data_used = header.data_size + header.bss_size;
    println!(
        "data slot: {} of {} bytes used ({:.1} %): {} loaded + {} zeroed",
        data_used,
        layout::DATA_SLOT_SIZE,
        100.0 * f64::from(data_used) / f64::from(layout::DATA_SLOT_SIZE),
        header.data_size,
        header.bss_size
    );
    println!("wrote {out_path}: {} bytes", file.len());
    Ok(())
}

fn check(path: &str) -> Result<(), String> {
    let file = fs::read(path).map_err(|error| format!("cannot read {path}: {error}"))?;
    let first: &[u8; GameHeader::SIZE] = file
        .get(..GameHeader::SIZE)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or("the file is shorter than a header")?;
    let header = GameHeader::from_bytes(first);
    print_header(&header);

    header.validate().map_err(|error| format!("the console would refuse this file: {error:?}"))?;
    let expected_len = GameHeader::SIZE + header.payload_size() as usize;
    if file.len() != expected_len {
        return Err(format!("the file is {} bytes; its header says {expected_len}", file.len()));
    }
    let mut crc = Crc32::new();
    crc.update(&file[..GameHeader::CHECKED]);
    crc.update(&file[GameHeader::SIZE..]);
    if crc.value() != header.crc32 {
        return Err(format!("checksum is {:#010x}; the header says {:#010x}", crc.value(), header.crc32));
    }
    println!("{path}: {} bytes, header and checksum are good", file.len());
    Ok(())
}

fn print_header(header: &GameHeader) {
    println!("header:");
    println!("  magic        {:#010x}", header.magic);
    println!("  api version  {}", header.api_version);
    println!("  code         {:#010x}, {} bytes", header.code_addr, header.code_size);
    println!("  data         {:#010x}, {} bytes", header.data_addr, header.data_size);
    println!("  zeroed       {} bytes", header.bss_size);
    println!("  entry        {:#010x}", header.entry);
    println!("  crc32        {:#010x}", header.crc32);
}
