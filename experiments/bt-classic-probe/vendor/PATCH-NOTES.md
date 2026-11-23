# Patch notes: vendor/esp-radio

`vendor/esp-radio` is a copy of the crates.io package **esp-radio
1.0.0-beta.1** (taken from the cargo registry, the file `.cargo-ok` left out).
The experiment uses it through `[patch.crates-io]` in `../Cargo.toml`.
Nothing in `~/.cargo` was changed.

Every change is behind a new feature. With neither feature on, the copy
builds the same code as the original (only the source-path strings in
`.rodata` differ, 840 bytes).

- `bt-dual`: start the controller in dual mode (classic + Low Energy).
- `bt-dual-idf-patches`: `bt-dual`, plus ESP-IDF's fixed classic functions
  in place of the ROM's ones.

The experiment's feature `dual` turns both on; `dual-rom` turns on only
`bt-dual`.

To see the full diff:

```
diff -ru ~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/esp-radio-1.0.0-beta.1 vendor/esp-radio
```

Line numbers below are the line numbers in the copy.

## 1. `Cargo.toml`, lines 231-235 (added, after `__docs_build = []`)

```toml
# PATCH (bt-classic-probe): start the ESP32 controller in dual mode (BTDM).
bt-dual = []
# PATCH (bt-classic-probe): also put the library's fixed classic functions
# in place of the ROM's, as ESP-IDF does (config_bt_funcs_reset).
bt-dual-idf-patches = ["bt-dual"]
```

## 2. `src/ble/btdm.rs`, lines 53-58 (the `extern` block)

Old (line 53):

```rust
    fn btdm_controller_enable(mode: esp_bt_mode_t);
```

New:

```rust
    #[cfg(not(feature = "bt-dual"))]
    fn btdm_controller_enable(mode: esp_bt_mode_t);
    // PATCH (bt-classic-probe): it returns an int (-1 if the mode differs
    // from the one given to btdm_controller_init).
    #[cfg(feature = "bt-dual")]
    fn btdm_controller_enable(mode: esp_bt_mode_t) -> i32;
```

Why: the library function returns an `int`. In LE mode the old line is
kept as it was.

## 3. `src/ble/btdm.rs`, lines 380-387 (the call)

Old (line 375):

```rust
        btdm_controller_enable(esp_bt_mode_t_ESP_BT_MODE_BLE);
```

New:

```rust
        #[cfg(not(feature = "bt-dual"))]
        btdm_controller_enable(esp_bt_mode_t_ESP_BT_MODE_BLE);
        // PATCH (bt-classic-probe): same mode as in the config, and check.
        #[cfg(feature = "bt-dual")]
        {
            let res = btdm_controller_enable(esp_bt_mode_t_ESP_BT_MODE_BTDM);
            assert!(res == 0, "btdm_controller_enable returned {}", res);
        }
```

Why: the mode given to enable must be the mode given to init, or the
library refuses (-1).

## 4. `src/ble/os_adapter_esp32.rs`, lines 175-177 (the `extern` block of `patch_apply`)

Added:

```rust
        // PATCH (bt-classic-probe): the classic half of the same job.
        #[cfg(feature = "bt-dual-idf-patches")]
        fn config_bt_funcs_reset();
```

## 5. `src/ble/os_adapter_esp32.rs`, lines 182-186 (the body of `patch_apply`)

Added, between `config_btdm_funcs_reset();` and `config_ble_funcs_reset();`:

```rust
        // PATCH (bt-classic-probe): ESP-IDF's patch_apply calls this in
        // every mode except Low Energy only. It puts the library's fixed
        // classic functions in place of the ROM's.
        #[cfg(feature = "bt-dual-idf-patches")]
        config_bt_funcs_reset();
```

Why: this is the order ESP-IDF uses (btdm, then bt when not LE only, then
ble). It pulls in about 10.5 KB of IRAM and 46 KB of flash code from the
controller library. That code needs one more ROM symbol,
`lm_get_total_pkt_type`, which is given in the experiment's own
`esp32-rom-bluetooth.x` (not in the copy).

## 6. `src/ble/os_adapter_esp32.rs`, lines 385-389 (`create_ble_config`, `mode`)

Old (line 377):

```rust
        mode: esp_bt_mode_t_ESP_BT_MODE_BLE as _,
```

New:

```rust
        #[cfg(not(feature = "bt-dual"))]
        mode: esp_bt_mode_t_ESP_BT_MODE_BLE as _,
        // PATCH (bt-classic-probe): dual mode, classic + Low Energy.
        #[cfg(feature = "bt-dual")]
        mode: esp_bt_mode_t_ESP_BT_MODE_BTDM as _,
```

## 7. `src/ble/os_adapter_esp32.rs`, lines 391-398 (`bt_max_acl_conn`)

Old (line 379):

```rust
        bt_max_acl_conn: 0,
```

New:

```rust
        #[cfg(not(feature = "bt-dual"))]
        bt_max_acl_conn: 0,
        // PATCH (bt-classic-probe): classic links. ESP-IDF refuses 0 and
        // uses 2 by default. Measured: with 1, and the IDF classic functions
        // (bt-dual-idf-patches), every name request or page is refused with
        // 0x09 "connection limit exceeded". With 2 it works.
        #[cfg(feature = "bt-dual")]
        bt_max_acl_conn: 2,
```

Cost: 1,280 bytes of heap per link (measured: radio heap 18,300 B with 1,
19,580 B with 2).

## 8. `src/ble/os_adapter_esp32.rs`, lines 402-407 (`bt_max_sync_conn`)

Old (line 383):

```rust
        bt_max_sync_conn: 1,
```

New:

```rust
        #[cfg(not(feature = "bt-dual"))]
        bt_max_sync_conn: 1,
        // PATCH (bt-classic-probe): no audio (SCO) links, as ESP-IDF forces.
        // The memory zeroed at start ends where 0 audio links end.
        #[cfg(feature = "bt-dual")]
        bt_max_sync_conn: 0,
```

## Not in the copy, but part of the same change

`../esp32-rom-bluetooth.x` (the experiment's own linker file) has one more
line than the one in `experiments/ble-scan`:

```
PROVIDE ( lm_get_total_pkt_type = 0x4004f928 );
```

The address was checked against ESP-IDF's `esp32.rom.ld` (Apache-2.0),
line 567. Without it the `dual` build does not link.

## Licence texts

`esp-radio/LICENSE-MIT` and `esp-radio/LICENSE-APACHE` are not part of the
published crate package. They were added here from the esp-hal repository
(https://github.com/esp-rs/esp-hal), where `esp-radio` lives, so that the
copy carries the licence its `Cargo.toml` names (MIT OR Apache-2.0).
