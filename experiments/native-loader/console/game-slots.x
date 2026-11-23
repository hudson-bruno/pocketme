/* The console's side of the game slots.
 *
 * A game is linked for two fixed places in memory (game-api/src/lib.rs has
 * the numbers; build.rs writes them into game-slot-addresses.x). The game
 * does not exist when the console is linked, so nothing here stops the
 * console itself from growing into those places. This file does.
 *
 * Each slot is guarded twice:
 *   - it is declared as a real section that takes up the whole slot, so it
 *     shows up in `readelf -S` and in the map file, and the linker refuses
 *     any other section that overlaps it;
 *   - an ASSERT with a message a person can understand.
 *
 * NOLOAD: the section reserves the addresses and puts nothing in the
 * firmware image.
 *
 * The sections name no memory region ("> iram_seg"): this file is read
 * before esp-hal's memory.x, where the regions are declared, and the
 * linker does not accept a region it has not seen yet. Each section gives
 * its address itself, and the ASSERTs check both of its ends.
 *
 * This file is given to the linker before esp-hal's own linkall.x (see
 * build.rs), in the same way as esp32-rom-bluetooth.x.
 */

INCLUDE game-slot-addresses.x

SECTIONS
{
  /* Executable RAM. The console's own RAM code (.vectors, .rwtext,
   * .rwtext.wifi) starts at 0x40080000 and must end before this. */
  .game_code GAME_CODE_ORIGIN (NOLOAD) :
  {
    _game_code_slot_start = ABSOLUTE(.);
    . += GAME_CODE_LENGTH;
    _game_code_slot_end = ABSOLUTE(.);
  }
}
INSERT AFTER .rwtext.wifi;

SECTIONS
{
  /* Data RAM. The console's heap (.dram2_uninit) starts at 0x3FFE7E30 and
   * must end before this. */
  .game_data GAME_DATA_ORIGIN (NOLOAD) :
  {
    _game_data_slot_start = ABSOLUTE(.);
    . += GAME_DATA_LENGTH;
    _game_data_slot_end = ABSOLUTE(.);
  }
}
INSERT AFTER .dram2_uninit;

ASSERT(_rwtext_end <= GAME_CODE_ORIGIN,
       "GAME SLOT: the console's own RAM code (.rwtext, .rwtext.wifi) has grown into the game's code slot. Shrink the console or move the slot in game-api/src/lib.rs (every game must then be rebuilt).")
ASSERT(GAME_CODE_ORIGIN + GAME_CODE_LENGTH <= 0x400A0000,
       "GAME SLOT: the game's code slot runs past the end of the executable RAM (0x400A0000).")
ASSERT(ADDR(.dram2_uninit) + SIZEOF(.dram2_uninit) <= GAME_DATA_ORIGIN,
       "GAME SLOT: the console's heap has grown into the game's data slot. Make the heap smaller or move the slot in game-api/src/lib.rs (every game must then be rebuilt).")
ASSERT(GAME_DATA_ORIGIN + GAME_DATA_LENGTH <= 0x40000000,
       "GAME SLOT: the game's data slot runs past the end of the data RAM (0x40000000).")
