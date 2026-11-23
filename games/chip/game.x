/* How a game is linked for the chip. Copied from the loader experiment
 * (experiments/native-loader/game/game.x, which was measured on the board);
 * only the slot file's name, the EXTERN line and the unwind tables in
 * /DISCARD/ are new, and (second reader, step 2) the data pieces end on
 * 8 bytes, with checks at the end. Used by games/build-chip.sh.
 *
 * Where the pieces of a game go.
 *
 * A game is linked for two fixed places in the console's memory, so that
 * the console can copy it in and run it without changing a single byte.
 *
 *   CODE  executable RAM. Only whole 32-bit words can be read or written
 *         there, so it gets machine code and literal pools and NOTHING
 *         else. It is the code slot and, right after it, the spill: code
 *         that does not fit in the slot runs on into the TOP of the data
 *         slot, seen from the instruction bus (stage 1; crates/game-api,
 *         layout::CODE_SPILL_ADDR). A game whose code fits in the slot
 *         is linked exactly as before. (A literal pool is the list of 32-bit constants and
 *         addresses a function needs; the processor fetches them with a
 *         word-sized instruction called L32R, which can only reach a short
 *         way backwards. That is why each pool must stay right before its
 *         function.)
 *   DATA  ordinary RAM: constants and strings (.rodata), variables that
 *         start with a value (.data), variables that start at zero (.bss).
 *   IMG   not memory at all. It is a made-up area that stands for "the
 *         payload of the game file". Giving every loaded section a "load
 *         address" in it (AT> IMG) makes the linker lay them out one after
 *         the other, which is exactly the order they have in the file.
 *         Without it, a plain `objcopy -O binary` would try to fill the
 *         13 MB between the two slots with zeroes.
 */

/* slots.x: GAME_CODE_ORIGIN/LENGTH, GAME_DATA_ORIGIN/LENGTH and
 * GAME_SPILL_ORIGIN/LENGTH (and the spill's first and last data word, for
 * check_load.py). build-chip.sh writes it from crates/game-api (layout) next to its
 * results and puts that folder first on the linker's search path; there is
 * no copy in this folder. */
INCLUDE slots.x

MEMORY
{
  /* The code slot and the spill after it. Only the ASSERT at the end
   * keeps the spill clear of the data. */
  CODE (rx) : ORIGIN = GAME_CODE_ORIGIN, LENGTH = GAME_CODE_LENGTH + GAME_SPILL_LENGTH
  DATA (rw) : ORIGIN = GAME_DATA_ORIGIN, LENGTH = GAME_DATA_LENGTH
  /* Code + data. Spill + data fit in the data slot (the ASSERT), so the
   * payload never needs more than this. */
  IMG  (r)  : ORIGIN = 0, LENGTH = GAME_CODE_LENGTH + GAME_DATA_LENGTH
}

/* The function the console calls first (made by game_kit::game!).
 * EXTERN makes the linker take it out of the game's library file;
 * ENTRY names it as the start. Together they also stop the linker from
 * throwing the whole game away as "never called". */
EXTERN(game_entry)
ENTRY(game_entry)

/* Routines that are already in the chip's ROM, at addresses Espressif
 * publishes (esp-idf, components/esp_rom/esp32/ld/esp32.rom.libgcc.ld and
 * esp32.rom.newlib-funcs.ld; the console's own crates use the same list,
 * see esp-rom-sys/ld/esp32/rom).
 *
 * The compiler calls these without being asked: the first four to copy,
 * fill or compare memory, the rest for 64-bit multiply and divide, which
 * the processor cannot do in one instruction.
 *
 * Rust's own library also has a copy of each. Naming the ROM's address
 * here wins over that copy, and the linker then leaves the copy out, which
 * measured 1.4 KB less code in this small game.
 */
memcpy     = 0x4000c2c8;
memset     = 0x4000c44c;
memmove    = 0x4000c3c0;
memcmp     = 0x4000c260;
__muldi3   = 0x4000c9fc;
__divdi3   = 0x4000ca84;
__moddi3   = 0x4000cd4c;
__udivdi3  = 0x4000cff8;
__umoddi3  = 0x4000d280;

SECTIONS
{
  .text : ALIGN(4)
  {
    _game_code_start = .;
    /* Literals and code go into the same section. With fat LTO the game
     * is one object file, and the linker then puts all of its literal
     * pools first and all functions after them (seen in the map file).
     * That is fine: `l32r` reaches 256 KB backwards and the code slot and
     * the spill together are 104 KB. More than 256 KB of code would need
     * the pools interleaved. A function may cross from the slot into the
     * spill: the instruction bus sees one unbroken range of addresses. */
    *(.literal .literal.* .text .text.*)
    /* The console copies whole words, so the code ends on a word. */
    . = ALIGN(4);
    _game_code_end = .;
  } > CODE AT> IMG

  .rodata : ALIGN(4)
  {
    _game_data_start = .;
    *(.rodata .rodata.*)
    /* 8, not 4: the next piece may hold a 64-bit number, which must start
     * on 8 bytes. With 4, the linker would leave a 4-byte gap in memory
     * that is not in the file, and the console (which copies the data as
     * one block) would put everything after it 4 bytes too early. */
    . = ALIGN(8);
  } > DATA AT> IMG

  .data : ALIGN(4)
  {
    *(.data .data.*)
    . = ALIGN(8); /* the same reason, for the zeroed memory */
    _game_data_end = .;
  } > DATA AT> IMG

  /* Not in the file: the console fills it with zeroes. */
  .bss (NOLOAD) : ALIGN(4)
  {
    _game_bss_start = .;
    *(.bss .bss.* COMMON)
    . = ALIGN(4);
    _game_bss_end = .;
  } > DATA

  /* Bookkeeping the compiler adds that no game needs at run time. */
  /DISCARD/ :
  {
    *(.eh_frame .eh_frame.*)
    /* Tables for unwinding after a panic. A game never unwinds on the
     * chip: a panic ends in the console's `abort`, which never returns.
     * The game kit still gets one such table, because `abort` is an
     * "unwind-allowed" function. Kept, it would land between the data
     * and need a routine the game does not have (rust_eh_personality). */
    *(.gcc_except_table .gcc_except_table.*)
    *(.xt.prop .xt.prop.*)
    *(.xt.lit .xt.lit.*)
    *(.xtensa.info)
    *(.comment)
  }
}

ASSERT(ADDR(.data) == ADDR(.rodata) + SIZEOF(.rodata), "the starting values must follow the constants with no gap (alignment over 8?)")
ASSERT(_game_bss_start == _game_data_end, "the zeroed memory must start right where the data ends (alignment over 8?)")

/* Bytes of code past the code slot (0 for a game that fits in it). They
 * are the TOP of the data slot, seen from the instruction bus in reverse
 * word order (game_api::layout::sram1_iram_to_dram; quoted from ESP-IDF,
 * seen to hold on the CYD, 2026-11-22). The rule is game_api::layout::fits; it
 * is kept in step by hand here and in check_load.py. */
_game_code_spill = MAX(_game_code_end, GAME_SPILL_ORIGIN) - GAME_SPILL_ORIGIN;

ASSERT(GAME_CODE_ORIGIN + GAME_CODE_LENGTH == GAME_SPILL_ORIGIN,
  "the code past the slot must follow the slot directly")
ASSERT(GAME_SPILL_LENGTH <= GAME_DATA_LENGTH,
  "the spill can only be the data slot")
ASSERT(_game_bss_end <= GAME_DATA_ORIGIN + GAME_DATA_LENGTH - _game_code_spill,
  "GAME TOO BIG: its code past the code slot and its data meet in the data slot. build-chip.sh prints the numbers.")
