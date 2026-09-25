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
- **Instant timeline switching**: Persisted period caches open immediately; stale selections refresh unobtrusively while startup preloads the remaining periods in the background.
- **Antigravity without the desktop app**: Live weekly and five-hour quotas first use the Windows Credential Manager and Code Assist API, then the local language server, with the authenticated `agy` CLI retained only as a compatibility fallback.
- **Pricing snapshot**: The embedded `models.dev` snapshot was regenerated on 2026-09-18 and validated for GPT-5.6, Antigravity/Gemini, Kimi K3, and DeepSeek models, including long-context tiers.

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
│   ├── background.rs   # Concurrent provider refresh coordinator
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
Start-Process '.\Agent Burn_0.1.70_x64-setup.exe' -ArgumentList '/UPDATE' -Wait
```

Update mode replaces the application in place without showing the uninstall/data-removal flow. Dashboard settings and history under `%LOCALAPPDATA%\Agent Burn` are preserved. Before installation, active CLI scans are stopped so the bundled executable can be replaced; they resume if the update is cancelled or fails. Automatic signed update checks are enabled by default and run at most once per day while the app is open, including at startup. When an update is found, its version remains available in the footer across app restarts; selecting it checks the release feed again before offering installation. A brief notice is shown only on the first discovery of each version. You can also check manually with **Check now**. An update is never installed without confirmation. The release feed is configured in `tauri.conf.json`.

To publish an updater release, add the contents of `%LOCALAPPDATA%\Agent Burn\updater\signing.key` as the GitHub Actions secret `TAURI_SIGNING_PRIVATE_KEY`. Keep that private key backed up and private. Pushing a `windows-v<version>` tag then publishes the setup installer, signed updater bundle, and rolling update feed.

Unknown subscription records are displayed as Free instead of receiving a fabricated monthly price. Antigravity Pro is valued at $20/month; Antigravity Ultra users select either $100 or $200/month in Settings. Provider-qualified and effort-qualified model identifiers are normalized only through tested aliases backed by the embedded pricing snapshot.
To validate an actual production installer without deleting the existing data,
run `verify-update.ps1` with the setup path. It keeps a temporary backup and
checks settings, report cache, and quota history after `/UPDATE`.
