# Eyelash60 RMK configuration

The `dongle` branch builds two nRF52840 firmware images:

- `peripheral`: the 5×14 Eyelash60 BLE split peripheral generated from
  `keyboard.toml`.
- `central`: the USB/BLE dongle with a 128×64 SH1106 Bongo Cat display.

The central uses the allocation-free
[`rmk-dongle-display`](https://github.com/hitsmaxft/rmk-dongle-display) at
`48c7f177e20d35fd62e1b2ab259f771c821a577e`, with only its `bongo-cat`
feature enabled. It uses I2C0 at address `0x3c`, SDA `P0.17`, SCL `P0.20`, a
128×64 page framebuffer, and the SH1106's two-column visible-area offset.

## Build

Push the branch or run the **Build Eyelash60 RMK firmware** workflow. It uses
`rmkit` to generate RMK's normal split project, then replaces only the central
entry point with `firmware/central.rs` and `firmware/display.rs`. `Cargo.toml`
and `memory.x` are deliberately root-level because `rmkit create` copies them
into the generated project.

The display is a registered RMK polling processor (10 ms). It maps keyboard,
layer, WPM, modifiers, host LEDs, output mode, sleep, and the one peripheral's
connection state to semantic display events. A press triggers Bongo Cat. The
adapter sends dirty SH1106 pages only; after an I2C failure it reinitializes
the panel and forces all eight pages to be sent.

## Hardware boundary

The source/build configuration is not OLED hardware validation. Before
flashing, confirm the dongle's actual nRF52840 bootloader map and I2C wiring.
After flashing, require an I2C ACK at `0x3c`, a visible full 128×64 frame,
and a key-triggered Bongo Cat animation. If the particular panel is wired
inverted, apply inversion exactly once in `firmware/display.rs`: use either
the controller's A7 mode or byte inversion, never both.
