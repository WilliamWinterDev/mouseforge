# MouseForge

A small, fast Windows tray app for the **SteelSeries Aerox 9 Wireless**, built to replace SteelSeries GG.

> MouseForge is an independent project. It is not affiliated with, endorsed by, or supported by SteelSeries. "SteelSeries" and "Aerox" are trademarks of their owner and are used here only to say which hardware is supported.

## Features

- **DPI:** up to 5 presets (100–18,000), live switching, separate presets per profile, polling rate, and saving to the mouse's onboard memory.
- **Buttons:** remap the wheel click and all 12 side buttons to:
  - keyboard shortcuts (held for as long as you hold the button)
  - media and volume keys
  - DPI next / previous / set / hold-to-snipe
  - macros, opening apps or URLs, or typing text
- **Per-app profiles:** switch automatically when an app (by `.exe`) is focused. Buttons a profile leaves unset use the Default profile's binding.
- **Battery:** shown in the tray, with Windows notifications at the levels you choose and when fully charged.
- **No GG needed:** MouseForge sets the side panel itself (see [docs/PROTOCOL.md](docs/PROTOCOL.md)).

Left and right click can never be remapped, so a bad profile can't lock you out of your mouse.

## Supported hardware

| Device | Status |
|---|---|
| Aerox 9 Wireless, 2.4 GHz receiver | Tested |
| Aerox 9 Wireless, USB cable | Should work (same protocol, untested) |
| Aerox 9 Wireless WOW Edition | Should work (IDs included, untested) |
| Bluetooth mode | Not supported (no configuration channel) |

## Install

Build it (below), then run:

```bash
powershell -ExecutionPolicy Bypass -File install.ps1
```

This installs to `%LOCALAPPDATA%\Programs\MouseForge` with no admin rights needed. It also turns on start with Windows (minimised to the tray) and creates a Start Menu shortcut. **The shortcut is required for notifications**: Windows only shows toasts from a desktop app whose ID (`com.mouseforge.app`) is attached to a Start Menu shortcut.

Then:

1. Quit SteelSeries GG (or uninstall it). It also writes to the mouse and overrides DPI.
2. In MouseForge, open **Setup** and click **Set side panel to F13–F24**. Out of the box the side buttons send `1`–`=`, which Windows can't tell apart from your keyboard. **Restore number keys** undoes this.

Settings are stored in `%APPDATA%\MouseForge\config.json`.

## Build

You need Rust (MSVC toolchain) and WebView2, which is built into Windows 11. Node isn't needed; the UI is plain HTML, CSS and JS in [`ui/`](ui).

```bash
cd src-tauri
cargo build --release
```

The output is `src-tauri/target/release/mouseforge.exe`. If linking fails with `LNK1104`, the path is too long for Windows. Set `CARGO_TARGET_DIR` to something short such as `C:\mf-target`.

## How it works

| File | Role |
|---|---|
| [`device.rs`](src-tauri/src/device.rs) | HID protocol on its own thread, with reconnects, battery polling, DPI and button table writes |
| [`input.rs`](src-tauri/src/input.rs) | Low-level mouse and keyboard hooks (wheel click, F13–F24), foreground-app tracking, and action dispatch |
| [`actions.rs`](src-tauri/src/actions.rs) | `SendInput` for keys, mouse buttons, media keys, macros and text |
| [`config.rs`](src-tauri/src/config.rs) | Profiles and bindings, stored as JSON |
| [`main.rs`](src-tauri/src/main.rs) | Tauri app, tray icon, notifications and the commands the UI calls |

Global input hooks are the standard Windows way to remap input, but some anti-cheat systems dislike them. If a game complains, quit MouseForge from the tray while you play it; DPI and the side panel's F13–F24 stay set on the mouse.

## Credits

- [rivalcfg](https://github.com/flozz/rivalcfg) by Fabien Loison (WTFPL): DPI table and the DPI, polling, battery and save commands.
- The button table was decoded from USB captures of SteelSeries GG.

## License

[MIT](LICENSE)
