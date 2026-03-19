# eRadio

A desktop internet radio browser and player built with Rust.

Browse thousands of stations from the [Radio Browser](https://www.radio-browser.info/) directory, search by name, and listen — all from a lightweight native app.

## Features

- Search and browse internet radio stations
- Audio playback with live ICY metadata (artist / song title)
- Animated now-playing display with flowing color effects
- Dark purple-themed UI
- System tray integration (minimize on close)
- Desktop notifications on song change
- Persistent state (favorites, last station) across sessions
- Cross-platform: Linux and Windows

## Screenshots

*Coming soon*

## Building

### Requirements

- Rust 1.70+
- Linux: ALSA development libraries (`libasound2-dev` on Debian/Ubuntu)

### Build and run

```sh
cargo build --release
./target/release/eradio
```

### Cross-compile for Windows

```sh
cross build --release --target x86_64-pc-windows-gnu
```

## Tech Stack

| Area | Crate |
|------|-------|
| GUI | egui / eframe |
| Audio | rodio / cpal |
| HTTP | reqwest |
| Tray (Linux) | ksni |
| Tray (Windows/macOS) | tray-icon / muda |
| Notifications | notify-rust |

## Author

Ted Lazaros

## License

All rights reserved.
