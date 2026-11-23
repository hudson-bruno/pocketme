/* Radio builds only (build.rs adds it with the pad feature).
 *
 * The radio library's own RAM code goes into the executable RAM below
 * the game's code slot (esp-hal's .rwtext.wifi). With the radio there is
 * almost no room left in that RAM. This file moves part of the radio
 * chip's set-up code (libphy: its .phyiram code and the .iram1 code of
 * phy_chip_v7.o, about 7.6 KB) into the RTC fast memory instead: 8 KB of
 * executable RAM at 0x400C0000 that nothing else in this firmware uses.
 * It is neither the game slot nor the second core's cache area.
 *
 * Rules of that memory, and how they are kept:
 *   - only the first core (PRO_CPU) can run code from it: the console,
 *     its interrupts and the radio's task all run on the first core;
 *   - the bootloader loads it like any other RAM part of the image
 *     (address = load address, so no copying at start-up);
 *   - it is the same 8 KB as esp-hal's .rtc_fast.* sections (code, and
 *     data through the data bus at 0x3FF80000), which esp-hal places at
 *     the start of that memory too. They are all empty in this firmware;
 *     the ASSERTs below stop the build if one ever is not.
 *
 * The section is placed before .rwtext.wifi in the linker's list, so its
 * patterns are matched first and those parts never reach .rwtext.wifi.
 * It names no memory region (this file is read before esp-hal's
 * memory.x) and gives its address itself.
 */

SECTIONS
{
  .cyd_rtc_phy 0x400C0000 :
  {
    _cyd_rtc_phy_start = ABSOLUTE(.);
    *libphy.a:(.phyiram .phyiram.*)
    *libphy.a:phy_chip_v7.o(.iram1 .iram1.*)
    . = ALIGN(4);
    _cyd_rtc_phy_end = ABSOLUTE(.);
  }
}
INSERT BEFORE .rwtext.wifi;

ASSERT(_cyd_rtc_phy_end <= 0x400C2000,
       "RTC FAST: the radio code moved by rtc-fast-phy.x is larger than the 8 KB RTC fast memory.")
ASSERT(SIZEOF(.rtc_fast.text) == 0 && SIZEOF(.rtc_fast.data) == 0
       && SIZEOF(.rtc_fast.bss) == 0 && SIZEOF(.rtc_fast.persistent) == 0,
       "RTC FAST: something now uses esp-hal's .rtc_fast.* sections, which share the RTC fast memory with the radio code of rtc-fast-phy.x.")
