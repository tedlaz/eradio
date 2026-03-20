# eRadio

A desktop internet radio browser and player built with Rust.

Browse thousands of stations from the [Radio Browser](https://www.radio-browser.info/) directory, search by name, and listen — all from a lightweight native app.

## Features

- Search and browse internet radio stations
- Audio playback with live ICY metadata (artist / song title)
- Animated now-playing display with flowing color effects
- Dark purple-themed UI
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

## Releasing

The project uses GitHub Actions to build and publish releases automatically. To create a new release:

- open build.rs and in line 856 change the version (eg v0.1.2)
- Open Cargo.toml and set version (eg v0.1.2)

```sh
git tag v0.1.2
git push origin v0.1.2
```

This triggers the CI workflow which builds binaries for Linux and Windows, then creates a GitHub release with the artifacts attached.

## Tech Stack

| Area | Crate |
|------|-------|
| GUI | egui / eframe |
| Audio | rodio / cpal |
| HTTP | reqwest |
| Notifications | notify-rust |

## Author

Ted Lazaros

## License

All rights reserved.
