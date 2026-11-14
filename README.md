<div align="center">
  <img src="assets/icon.png" width="128" alt="AlwaysOn Icon">
  <h1>AlwaysOn (Rust)</h1>
</div>

A lightweight, native Windows 11 utility built in pure Rust + Win32 APIs to **keep your PC and display awake** without heavy runtime frameworks.

If you are downloading large files, rendering builds/videos, or reading without touching your keyboard and mouse, **AlwaysOn** sits quietly in the system tray and prevents sleep with minimal memory and zero idle CPU overhead.

<div align="center">
  <img src="assets/screenshot.png" width="760" alt="AlwaysOn Control Panel Preview">
</div>

## Features
- **WinUI 3 Fluent Control Panel**: Native bottom-right flyout built directly with Win32 + DWM `Mica Alt`, `Segoe UI Variable` typography, `Segoe Fluent Icons`, SDF anti-aliased `SettingsCard` surfaces, pearl-white 3D squircle branding, and Fluent `SelectorBar` / `ToggleSwitch` controls.
- **Light, Dark & System Theme Modes**: Automatically follows Windows' `AppsUseLightTheme` setting (including live `WM_SETTINGCHANGE` updates) or lets you explicitly select **Light** (`Açık`) or **Dark** (`Koyu`) mode with matching DWM frame styling and palettes.
- **Event-Driven 60fps Animations**: Smooth flyout entrance slide, stretching toggle switch thumbs, sliding `SelectorBar` / 2D language grid indicator pills, and horizontal view navigation transitions—automatically stopping the animation timer when idle so CPU stays at `0%`.
- **Ultra-Low Resource Footprint**: Reuses cached GDI fonts and DIB surfaces during interaction, releases all paint buffers and trims process working set (`SetProcessWorkingSetSize`) when minimized to the tray, and suspends heartbeat timers when sleep prevention is off.
- **Flexible Modes & Display Control**: Choose between **Indefinite (Süresiz)**, **30 Minutes**, **1 Hour**, or **3 Hours**, plus toggle whether to keep the display powered or only prevent system sleep (`SetThreadExecutionState`).
- **Dedicated Settings View (`⚙`)**: Toggle **Run at Windows startup**, choose **Close button (`✕`) behavior** between **Minimize to Tray** (`Alta Al (Tepsi)`) or **Exit Application** (`Uygulamayı Kapat`), switch **App Theme** (**System**, **Light**, **Dark**), and pick from **System** + **11 languages** (`Türkçe`, `English`, `Deutsch`, `Français`, `Español`, `Italiano`, `Português`, `Русский`, `中文`, `日本語`, `한국어`), persisted under `HKCU\Software\AlwaysOn`.
- **Single-File Portable Executable**: Zero installer, zero runtime dependencies, and zero background file copying — run `AlwaysOn.exe` directly from any folder. On first launch (or anytime via the **Persistence Note** pill in the footer), AlwaysOn shows a helpful tip recommending you copy the portable `.exe` to a permanent trusted folder for seamless autostart persistence, alongside a direct link to **[github.com/yunusemreyl](https://github.com/yunusemreyl)**.
- **Reliable Autostart & Tray Integration**: Registers the running portable executable path directly in Windows Startup (`HKCU\...\Run`) when autostart is enabled, recovers gracefully on `TaskbarCreated`, and guards single-instance execution per desktop.
- **Dynamic Localization**: Defaults to the Windows display language (`GetUserDefaultUILanguage`) and lets you switch the UI, window title, tray tooltip, context menu, and notifications on the fly across 11 languages.

## Portable Usage & Persistence
Download `AlwaysOn.exe` from the [GitHub Releases](https://github.com/yunusemreyl/AlwaysOn/releases) page and run it anywhere:
1. On first launch, review the **Portable • Single EXE** welcome card (which recommends keeping `AlwaysOn.exe` in a permanent trusted directory such as `C:\Tools\AlwaysOn\` or `%LOCALAPPDATA%\Programs\AlwaysOn\` before enabling **Run at Windows startup**).
2. Click the **`github.com/yunusemreyl`** badge in the welcome card or main footer to visit the author's GitHub profile anytime.

## Building & Testing
Run unit/scenario tests and build the release executable:
```bash
cargo test --release
cargo build --release
```
