# Agent Burn for Windows

Native Windows system-tray and dashboard application, powered by Tauri v2 and the Rust Agent Burn CLI engine.
Windows 10 and 11 (x64 and ARM64).

The system-tray icon provides live quota remaining for Claude Code, Codex, and Cursor, with automatic background polling every 60 seconds. Left-clicking opens the sleek dark-themed dashboard; right-clicking provides quick access to refresh, change periods, or quit.

## Features

- **System Tray Integration**: Dynamic tray icon displaying remaining quota percentage and status tooltip.
- **Background Quota Polling**: Silent background worker running every 60 seconds to track quota usage and reset countdowns without freezing the interface.
- **Unified Dashboard**: View API-equivalent value vs. monthly plan costs across Claude, Codex, Cursor, Gemini, Copilot, etc.
- **Interactive Stacked Charts**: Day-by-day token burn rate and model breakdowns.
- **Ultra-lightweight**: Powered by Microsoft Edge WebView2 (native to Windows 10/11) with a total memory footprint under 20 MB.
- **Local-First & Offline**: Reads local logs directly. No source code or prompts ever leave your machine.

## Architecture

```
apps/windows/
├── Cargo.toml          # Rust dependencies (Tauri v2, tray-icon, tokio, serde)
├── tauri.conf.json     # Window, system-tray and application manifest
├── build.rs            # Tauri build script
├── src/
│   ├── main.rs         # Application entry point and Tauri setup
│   ├── app.rs          # App state, settings, and CLI path resolver
│   ├── tray.rs         # Windows System Tray (Taskbar Notification Area)
│   ├── background.rs   # Async background polling worker (every 60s)
│   └── commands.rs     # Tauri IPC commands (get_summary, get_harness, refresh)
├── ui/
│   ├── index.html      # Responsive dashboard UI (Geist / dark flame theme)
│   ├── app.js          # Reactive state, charts, and IPC communication
│   └── style.css       # High-contrast, accessibility-compliant styling
├── justfile            # Task runner recipes for Windows build
└── build.ps1           # PowerShell automated build script
```

## Build and Run

### Prerequisites
- Node.js 22+ & pnpm
- Rust 1.85+ (MSVC toolchain: `rustup default stable-x86_64-pc-windows-msvc`)
- Visual Studio Build Tools (C++ development workload)
- Microsoft Edge WebView2 (preinstalled on Windows 10/11)

### Commands

```powershell
# In apps/windows
pnpm install
cargo tauri dev      # Start development mode with hot reload
cargo tauri build    # Compile production executable (.exe / .msi)
```
