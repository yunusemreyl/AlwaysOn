# AlwaysOn (Rust)

A lightweight, zero-overhead Windows background utility that prevents your computer from sleeping. 
Written entirely in Rust using raw Win32 APIs (no bulky UI frameworks). 
Uses less than 1MB of RAM and 0% CPU!

## Features
- **Zero Overhead**: Direct Win32 API calls (`SetThreadExecutionState`).
- **Timers**: Keep awake indefinitely, or for 1 / 3 hours.
- **Autostart**: Optionally run silently at Windows startup.
- **Localization**: Automatically adapts to Turkish and English based on your Windows display language.
- **Single Executable**: Multi-resolution icons are embedded directly into the `.exe`.

## Building
Run `cargo build --release` to build a standalone executable.
