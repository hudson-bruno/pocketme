/* Where the pieces of a game go.
 *
 * A game is linked for two fixed places in the console's memory, so that
 * the console can copy it in and run it without changing a single byte.
 *
 *   CODE  executable RAM. Only whole 32-bit words can be read or written
 *         there, so it gets machine code and literal pools and NOTHING
 *         else. (A literal pool is the list of 32-bit constants and
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

INCLUDE game-slot-addresses.x

MEMORY
{
  CODE (rx) : ORIGIN = GAME_CODE_ORIGIN, LENGTH = GAME_CODE_LENGTH
  DATA (rw) : ORIGIN = GAME_DATA_ORIGIN, LENGTH = GAME_DATA_LENGTH
  IMG  (r)  : ORIGIN = 0, LENGTH = GAME_CODE_LENGTH + GAME_DATA_LENGTH
}

/* The function the console calls first. Naming it here also stops the
 * linker from throwing the whole game away as "never called". */
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
     * That is fine: `l32r` reaches 256 KB backwards and the whole slot is
     * 64 KB. A slot larger than 256 KB would need the pools interleaved. */
    *(.literal .literal.* .text .text.*)
    /* The console copies whole words, so the code ends on a word. */
    . = ALIGN(4);
    _game_code_end = .;
  } > CODE AT> IMG

  .rodata : ALIGN(4)
  {
    _game_data_start = .;
    *(.rodata .rodata.*)
    . = ALIGN(4);
  } > DATA AT> IMG

  .data : ALIGN(4)
  {
    *(.data .data.*)
    . = ALIGN(4);
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
    *(.xt.prop .xt.prop.*)
    *(.xt.lit .xt.lit.*)
    *(.xtensa.info)
    *(.comment)
  }
}

ASSERT(_game_bss_start == _game_data_end, "the zeroed memory must start right where the data ends")
