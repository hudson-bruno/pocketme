/* Three functions in the ESP32's ROM that Espressif's Bluetooth library
 * refers to and that the ROM table in esp-rom-sys 0.1.5 leaves out. Without
 * them the firmware does not link (checked for this experiment: build with
 * BLE_SCAN_NO_ROM_FIX=1 to see the three "undefined reference" errors).
 *
 * Same file as an earlier firmware of the author's on this board uses.
 * The addresses are Espressif's own, from
 *   esp-idf, components/esp_rom/esp32/ld/esp32.rom.ld
 * ESP-IDF is Copyright Espressif Systems (Shanghai) CO LTD and licensed
 * under the Apache License 2.0.
 *
 * All three belong to classic Bluetooth (inquiry, paging), which this
 * firmware never starts.
 *
 * PROVIDE defines a name only if nothing else does. Once an esp-rom-sys
 * with these lines is published and in use, this file does nothing and
 * can be deleted, together with its lines in build.rs.
 */
PROVIDE ( ld_pscan_evt_start_cbk = 0x4003e924 );
PROVIDE ( ld_iscan_evt_start_cbk = 0x4003b58c );
PROVIDE ( ld_page_evt_start_cbk = 0x4003cf40 );
