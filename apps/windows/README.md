# Agent Burn for Windows

Native Windows system-tray and dashboard application, powered by Tauri v2 and the Rust Agent Burn CLI engine.
Windows 10 and 11 (x64 and ARM64).

The system-tray tooltip provides live quota remaining for detected providers. Polling continues while the dashboard is hidden and follows the refresh interval selected in Settings. Left-clicking opens the dashboard; right-clicking provides refresh and quit actions.

## Features

- **System Tray Integration**: Tray tooltip with remaining quota and explicit saved-state labeling when dashboard readings become stale.
- **Background Quota Polling**: Silent worker that keeps running when the dashboard window is closed to the tray.
- **Unified Dashboard**: View API-equivalent value vs. monthly plan costs across Claude, Codex, Cursor, Gemini, Copilot, etc.
- **Interactive Stacked Charts**: Day-by-day token burn rate and model breakdowns.
- **Native Windows shell**: Powered by Microsoft Edge WebView2, included with supported Windows versions.
- **Local-First & Offline**: Reads local logs directly. No source code or prompts ever leave your machine.
- **Recoverable history**: Atomic cache and backup files plus an append-only journal recover the latest report after cache corruption.

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
└── build.ps1           # Stages the bundled CLI and builds the application
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
pnpm dev             # Stage the CLI and start development mode
pnpm build           # Stage the CLI and compile the .exe / installer
```

The release bundle contains `agent-burn.exe`; the desktop application never downloads a replacement CLI at runtime. Settings can select another local executable, configure Codex homes, enable cached/offline mode, select the quota source, and change the refresh interval.

## Updating an installed copy

Build or download an installer with a version newer than the installed copy, close Agent Burn from its tray menu, then launch it in update mode:

```powershell
Start-Process '.\Agent Burn_0.1.14_x64-setup.exe' -ArgumentList '/UPDATE' -Wait
```

Update mode replaces the application in place without showing the uninstall/data-removal flow. Dashboard settings and history under `%LOCALAPPDATA%\Agent Burn` are preserved. Automatic network updates are not enabled, so release installers remain the explicit update channel until a signed release feed is configured.

Unknown subscription records are displayed as Free instead of receiving a fabricated monthly price. Antigravity Pro is valued at $20/month; Antigravity Ultra users select either $100 or $200/month in Settings. Provider-qualified and effort-qualified model identifiers are normalized only through tested aliases backed by the embedded pricing snapshot.
