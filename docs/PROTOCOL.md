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
| `0x21` | `count, (zone, r, g, b) × count` | **Lighting colour**: zones 0, 1, 2. GG sends `01 zone r g b` for one zone, or `03` with all three. Takes effect immediately and isn't saved. |
| `0x22` | `ff` | Onboard rainbow effect (rivalcfg) |
| `0x23` | `0f 01 01 00, ms (3 bytes LE)` | **Lighting dim timer** (GG: 30 s = `30 75 00`, 7 min = `a0 68 06`) |
| `0x26` | `01 00 r g b`, or `00 00 00 00 00` for off | **Click flash (reactive lighting)**, done by the mouse itself. It overlays streamed colours and is kept after a save. |
| `0x27` | `01 00` = rainbow, `00 00` = off | Lighting at power-on and whenever nothing is streaming colours (needs a save) |
| `0x28` | `00` | Unknown; GG sends it at startup |
| `0x29` | `ms (3 bytes LE)` | **Sleep timer** (5 min = `e0 93 04`, 10 min = `c0 27 09`) |
| `0x90` | `00` | Version query, **without** the `0x40` flag. Reply is ASCII with no echo byte: `31 2e 32 2e 34` = "1.2.4". GG also sends `0xD0`. |
| `0xBC` | `00` | Unknown query; reply byte 1 = `01` |

## What GG does in software

Seen in captures; these never reach the mouse as settings:

- **Lighting effects** (static, breathe, colour shift): Prism renders every frame on the PC and streams `0x21` colour frames at about 20 per second. An app that wants effects has to animate them itself.
- **Angle snapping, acceleration/deceleration**: toggling them sends no command, so they're PC-side filters.

## Startup sequence

When GG starts (or the mouse reconnects), it pushes its whole configuration: `90`, `D0`, lighting `21` frames, `26`, `29`, `2B`, `2D`, `28`, `23`, `27`, the 3 `2A` pages, `11` (save), `BC`, then battery `92`.

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

Modifier keys, macros, media keys, the mouse-button types and the unidentified slot types haven't been decoded yet. Slot 6 (`0x06`) is probably the DPI button. MouseForge does all of these in software anyway, so they're only needed for settings that should live on the mouse itself. Captures of GG setting them are welcome.

Not yet tested: whether the mouse reports presses of its own DPI button to the PC. Nothing showed up on interfaces 3 or 4 during the captures, but the button wasn't pressed while they were recording.
