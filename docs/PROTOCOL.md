# Aerox 9 Wireless HID protocol notes

These notes cover what MouseForge uses. The DPI, polling rate, battery and save commands come from [rivalcfg](https://github.com/flozz/rivalcfg) (WTFPL). The button table was worked out from USB captures of SteelSeries GG, made with [USBPcap](https://desowin.org/usbpcap/).

## Transport

| Mode | VID:PID | Command flag |
|---|---|---|
| 2.4 GHz receiver | `1038:1858` (WOW edition `1038:1874`) | `cmd \| 0x40` |
| USB cable | `1038:185A` (WOW edition `1038:1876`) | none |

- Commands are 64-byte **output reports** (report ID 0) on **interface 3** (usage page `0xFFC0`). On the wire they are `SET_REPORT` control transfers.
- Every command gets a reply on the interrupt IN endpoint. The first byte of the reply echoes the command (including the `0x40` flag).
- **2.4 GHz caveat:** read each reply before sending the next command. Writes made without reading the replies are accepted by Windows but silently ignored by the mouse.
- With the mouse in Bluetooth mode, the receiver still answers but replies `40 ff …` (no mouse linked).

## Commands (cable values; OR with `0x40` on 2.4 GHz)

| Cmd | Payload | Meaning |
|---|---|---|
| `0x11` | `00` | Save the current settings to onboard memory |
| `0x2A` | see below | Button table (3 pages) |
| `0x2B` | `00`=1000 Hz, `01`=500, `02`=250, `03`=125 | Polling rate. **Re-initialises the sensor and resets DPI**, so send DPI afterwards. |
| `0x2D` | `count, active, v1 … vN` | DPI presets (1–5); `active` is 0-based. Values come from the TrueMove Air table (100–18000 DPI in steps of 100) in [`device.rs`](../src-tauri/src/device.rs). |
| `0x92` | none | Battery. Reply byte 1: bit 7 = charging, `(b & 0x7F) - 1) * 5` = percent. |

## Button table (`0x2A`)

There are 20 slots of 5 bytes each, sent as three pages: `[0x2A|flag, page, slot × n]` with 6, 6 and 8 slots, followed by the save command.

A slot is `[type, code, 0, 0, 0]`. Known types:

| Type | Meaning |
|---|---|
| `0x01` `0x02` `0x03` | Left, right and middle click |
| `0x51` | Keyboard key; `code` = HID usage (e.g. `0x1E` = `1`, `0x68` = F13 … `0x73` = F24) |
| `0x33` `0x34` `0x06` `0x31` `0x32` | Unidentified (scroll / DPI functions). MouseForge writes them back exactly as GG does. |

Slot order:

| Slots | Contents |
|---|---|
| 1–6 | left, right, middle, `0x33`, `0x34`, `0x06` |
| 7–18 | side buttons **3, 6, 9, 12, 2, 5, 8, 11, 1, 4, 7, 10** (the panel's numbering, column by column) |
| 19–20 | `0x31`, `0x32` |

Factory side keys are `1`–`9`, `0`, `-`, `=` (HID `0x1E`–`0x26`, `0x27`, `0x2D`, `0x2E`).

Modifier keys, macros and the unidentified slot types haven't been decoded yet. Captures of GG setting those are welcome.
