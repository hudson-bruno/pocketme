/* Lab (feature cpu1-cache-ram): code in the second core's cache RAM.
 *
 * The ESP32 has 64 KB of RAM at 0x4007_0000 that its flash caches use:
 *   0x4007_0000 .. 0x4007_8000  the first core's cache (always in use:
 *                               all code and constants in flash go
 *                               through it)
 *   0x4007_8000 .. 0x4008_0000  the second core's cache. Only in use
 *                               once the second core is started.
 * This firmware never starts the second core, so the second half is
 * plain RAM that can hold code. This file puts a section there.
 *
 * WHO PUTS THE BYTES THERE. Not the bootloader: it runs from this very
 * RAM while it loads the firmware (its log says "load:0x40078000"), and
 * it refuses to load anything over itself. So the section is linked to
 * RUN at 0x4007_8000 but is STORED (its "load address") at the start of
 * the game code slot, 0x4009_0000: RAM no game uses yet at start-up.
 * espflash puts it there in the image, the bootloader loads it there
 * like any other RAM code, and the firmware's first lines
 * (cpu1_cache::start) copy it to 0x4007_8000, check every word, and
 * clear the slot again.
 *
 * (Tried first and measured: storing it at the start of the heap's RAM,
 * 0x3FFE_7E30. The bootloader refused the firmware: "Segment 2
 * 0x400b8190-0x400b81cc invalid: bad load address range". That RAM is
 * also visible as code RAM at 0x400B_xxxx, above where the bootloader
 * allows code, so it refuses to load data there at all.)
 *
 * What goes in:
 *   - .cpu1_cache_text*: functions of this firmware marked for it;
 *   - with radio-fit: the radio's PHY library's RAM code (from
 *     libphy.a, its .phyiram and .iram1 parts). It runs only after the
 *     radio is started, long after the copy.
 *
 * Read before esp-hal's linkall.x; INSERT BEFORE .rwtext puts these
 * lines ahead of esp-hal's own, so the PHY's parts land here and not in
 * .rwtext.wifi.
 */

/* The second half of esp-hal's "reserved_cache_seg" (0x40070000, 64 KB),
 * as a region of its own. */
MEMORY
{
  cpu1_cache_seg (RX) : ORIGIN = 0x40078000, LENGTH = 32K
}

SECTIONS
{
  .cpu1_cache_text : AT(GAME_CODE_ORIGIN)
  {
    _cpu1_cache_text_start = ABSOLUTE(.);
    *(.cpu1_cache_text .cpu1_cache_text.*)
    _cpu1_cache_own_end = ABSOLUTE(.);
    *libphy.a:(.phyiram .phyiram.* .iram1 .iram1.*)
    . = ALIGN(4);
    _cpu1_cache_text_end = ABSOLUTE(.);
  } > cpu1_cache_seg
  _cpu1_cache_text_load = LOADADDR(.cpu1_cache_text);
}
INSERT BEFORE .rwtext;

ASSERT(_cpu1_cache_text_end <= 0x40080000,
       "CPU1 CACHE: the code for the second core's cache RAM is larger than its 32 KB.")
ASSERT(_cpu1_cache_text_load + (_cpu1_cache_text_end - _cpu1_cache_text_start) <= GAME_CODE_ORIGIN + GAME_CODE_LENGTH,
       "CPU1 CACHE: the stored copy does not fit in the game code slot.")
