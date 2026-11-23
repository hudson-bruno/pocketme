/* The console's side of the game slots (copied from
 * experiments/native-loader/console/game-slots.x, where it was proven).
 *
 * A game is linked for two fixed places in memory (build.rs has the
 * numbers and writes them into game-slot-addresses.x). The game does
 * not exist when the console is linked, so nothing else stops the
 * console itself from growing into those places. This file does.
 *
 * Each slot is guarded twice:
 *   - it is a real section that takes up the whole slot, so it shows up
 *     in `readelf -S`, and the linker refuses any other section that
 *     overlaps it;
 *   - an ASSERT with a message a person can understand.
 *
 * NOLOAD: the section reserves the addresses and puts nothing in the
 * firmware image.
 *
 * The sections name no memory region: this file is read before
 * esp-hal's memory.x, where the regions are declared. Each section gives
 * its address itself, and the ASSERTs check both of its ends.
 *
 * Stage 1: a game's code may run on past the code slot into "the spill"
 * (game_api::layout::CODE_SPILL_ADDR): SRAM1 seen from the instruction
 * bus, 0x400A0000 on. Its words are the TOP of the data slot, in reverse
 * word order (layout::sram1_iram_to_dram; quoted from ESP-IDF, checked
 * by the firmware at every start-up; seen to hold on the CYD, 2026-11-22).
 * .game_spill reserves those instruction addresses so that no console
 * code can ever be placed there.
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

  /* The spill: the same RAM as the data slot (above), seen from the
   * instruction bus. Nothing of the console may be here; the game's code
   * past the code slot is put here at run time, never by the boot
   * loader (NOLOAD: no bytes in the image). */
  .game_spill GAME_SPILL_ORIGIN (NOLOAD) :
  {
    _game_spill_start = ABSOLUTE(.);
    . += GAME_SPILL_LENGTH;
    _game_spill_end = ABSOLUTE(.);
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
       "GAME SLOT: the console's own RAM code (.rwtext, .rwtext.wifi) has grown into the game's code slot. Shrink the console or move the slot (every game must then be rebuilt).")
ASSERT(GAME_CODE_ORIGIN + GAME_CODE_LENGTH <= 0x400A0000,
       "GAME SLOT: the game's code slot runs past the end of the executable RAM (0x400A0000).")
ASSERT(ADDR(.dram2_uninit) + SIZEOF(.dram2_uninit) <= GAME_DATA_ORIGIN,
       "GAME SLOT: the console's heap has grown into the game's data slot. Make the heap smaller or move the slot (every game must then be rebuilt).")
ASSERT(GAME_DATA_ORIGIN + GAME_DATA_LENGTH <= 0x40000000,
       "GAME SLOT: the game's data slot runs past the end of the data RAM (0x40000000).")
ASSERT(_game_spill_start == _game_code_slot_end,
       "GAME SLOT: the spill must follow the code slot directly.")
ASSERT(GAME_SPILL_ORIGIN == 0x400A0000 && GAME_DATA_ORIGIN + GAME_DATA_LENGTH == 0x40000000,
       "GAME SLOT: the spill is SRAM1's instruction-bus start (0x400A0000), which mirrors the TOP of the data slot only if the data slot ends at 0x40000000.")
ASSERT(GAME_SPILL_LENGTH <= GAME_DATA_LENGTH,
       "GAME SLOT: the spill cannot be larger than the data slot (it is the data slot).")
ASSERT(_stack_start_cpu0 <= ADDR(.dram2_uninit),
       "GAME SLOT: the first core's stack must stay below SRAM1's heap and the data slot (SRAM1 is also the spill).")
