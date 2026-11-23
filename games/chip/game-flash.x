/* games/chip/game-flash.x: the flash part of
 * a game that has a games/<name>/flash.list. Linked IN FRONT of the
 * unchanged game.x:
 *
 *   xtensa-esp32-elf-ld -L $OUT/flash/<NAME> -L $OUT -L chip \
 *       -T chip/game-flash.x -T chip/game.x --gc-sections ...
 *
 * so a game without a flash part is linked exactly as before (game.x and
 * its command line do not change).
 *
 * flash-place.x (made by build-chip.sh from game_api::layout's place of
 * this game) gives GAME_FLASH_CODE_ORIGIN/LENGTH and
 * GAME_FLASH_RO_ORIGIN/LENGTH. flash-code.x and flash-rodata.x (made by
 * flash_split.py from the game's flash.list and its pass-1 link) name the
 * input sections. All three sit in $OUT/flash/<NAME>/, first on the path.
 *
 * Both sections come before game.x's .text and .rodata (INSERT BEFORE),
 * so their patterns win over game.x's catch-alls. Each flash function's
 * literal pool (.literal.<f>) goes with it: L32R only reaches backwards,
 * 256 KB at most, and the place is far smaller.
 *
 * Neither section is in the game file: build-chip.sh drops them from the
 * payload (objcopy -R) and writes them as flash/<NAME>/CODE.BIN and
 * flash/<NAME>/RO.BIN, which the firmware build puts at these very addresses.
 */
INCLUDE flash-place.x

MEMORY
{
  /* Flash code, read by the instruction fetch through the cache: whole
   * aligned 32-bit words only (like the code slot). */
  FCODE (rx) : ORIGIN = GAME_FLASH_CODE_ORIGIN, LENGTH = GAME_FLASH_CODE_LENGTH
  /* Flash read-only data, read through the data cache: any byte. */
  FRO   (r)  : ORIGIN = GAME_FLASH_RO_ORIGIN,   LENGTH = GAME_FLASH_RO_LENGTH
}

SECTIONS
{
  .flash_text : ALIGN(4)
  {
    _game_flash_code_start = .;
    INCLUDE flash-code.x
    /* The firmware checks whole words. */
    . = ALIGN(4);
    _game_flash_code_end = .;
  } > FCODE

  .flash_rodata : ALIGN(4)
  {
    _game_flash_ro_start = .;
    INCLUDE flash-rodata.x
    . = ALIGN(4);
    _game_flash_ro_end = .;
  } > FRO
}
INSERT BEFORE .text;

ASSERT(_game_flash_code_end > _game_flash_code_start,
  "FLASH PART: flash.list put no function into flash (remove the file, or fix its lines)")
ASSERT(_game_flash_code_end <= GAME_FLASH_CODE_ORIGIN + GAME_FLASH_CODE_LENGTH,
  "FLASH PART: the flash code is bigger than this game's place in game_api::layout")
ASSERT(_game_flash_ro_end <= GAME_FLASH_RO_ORIGIN + GAME_FLASH_RO_LENGTH,
  "FLASH PART: the flash read-only data is bigger than this game's place in game_api::layout")
