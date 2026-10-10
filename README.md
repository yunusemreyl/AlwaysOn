<div align="center">
  <img src="assets/icon.png" width="128" alt="AlwaysOn Icon">
  <h1>AlwaysOn (Rust)</h1>
</div>

A lightweight, zero-overhead Windows background utility designed to **prevent your computer screen from turning off or going to sleep**. 

If you are downloading large files, rendering videos, or simply reading long documents and constantly fighting your PC's sleep settings, **AlwaysOn** sits quietly in your system tray and keeps your machine awake!

Written entirely in Rust using raw Win32 APIs (no bulky UI frameworks). It uses **less than 1MB of RAM** and **0% CPU**.

## Features
- **Prevent Sleep & Screen Lock**: Direct Win32 API calls (`SetThreadExecutionState`) to ensure your display and system stay awake.
- **Timers**: Choose to keep your PC awake indefinitely, or set a timer for 1 or 3 hours (automatically reverts to normal sleep behavior afterwards).
- **Autostart**: Optionally run silently at Windows startup so you never have to remember to open it.
- **Localization**: Automatically adapts to Turkish and English based on your Windows display language.
- **Single Executable**: Multi-resolution Ultra-HD icons are embedded directly into the `.exe`. No messy folders!

## Download
Grab the latest single `.exe` file from the [Releases](../../releases) page!

## Building from source
If you want to compile it yourself:
```bash
cargo build --release
```
