# Agent Burn

Native desktop apps (macOS & Windows) and local CLI for coding-agent usage, limits, and subscription value.

Site: [agent-burn.melvynx.dev](https://agent-burn.melvynx.dev)

`summary` is the all-up local spend view. `harness <claude|codex>` is the weekly subscription-limit view. The npm package also installs `burn` as a short alias.

## Install

**Mac app:** [Download](https://agent-burn.melvynx.dev/download), unzip, move to Applications. macOS 14+, Apple Silicon and Intel. The app bundles the CLI.

**Windows app:** Download the installer (`Agent Burn_<version>_x64-setup.exe`) from [Releases](https://github.com/DavidSZD/agent-burn/releases). Windows 10/11 (x64). Integrates into the system tray and bundles the CLI. Build locally with `pnpm --prefix apps/windows build`. Details: [apps/windows](https://github.com/DavidSZD/agent-burn/tree/main/apps/windows).

**CLI:**

```bash
npx agent-burn@latest summary --value
pnpm dlx agent-burn@latest harness claude --value
bunx agent-burn@latest harness codex --value
```

## Commands

```bash
agent-burn
agent-burn summary
agent-burn summary today
agent-burn summary week --value
agent-burn harness claude --value
agent-burn harness codex --value
agent-burn summary --json
```

macOS app: menu-bar quota for Codex, Claude, and Cursor, plus a dashboard across detected harnesses. Build locally with `just macos::run`. Details: [apps/macos](https://github.com/Melvynx/agent-burn/tree/main/apps/macos).

Windows app: system-tray quota tooltip, live background polling, and responsive dashboard for Claude, Codex, Cursor, Antigravity, and Gemini. Build locally with `pnpm --prefix apps/windows build` or `just windows::run`. Details: [apps/windows](https://github.com/DavidSZD/agent-burn/tree/main/apps/windows).

## Subscription Value

`--value` compares local API-equivalent usage with known or supplied monthly plan prices.

```bash
agent-burn summary --value
agent-burn summary --value --claude-plan max-20x --codex-plan pro
agent-burn harness claude --value --claude-plan 200
agent-burn harness codex --value --codex-plan plus
```

Plan overrides:

- Claude: `pro`, `max-5x`, `max-20x`, or a monthly price
- Codex: `plus`, `pro`, or a monthly price
- Cursor: `pro`, `pro+`, `ultra`, or a monthly price
- Antigravity: `pro` ($20), `ultra` ($100 or $200), or a monthly price

## Shared Options

```bash
--since <YYYYMMDD>       Start date
--until <YYYYMMDD>       End date
--json                   JSON output
--jq <filter>            Apply a jq filter to JSON output
--mode <auto|calculate|display>
--breakdown              Include model breakdowns
--offline                Use embedded pricing and skip live requests
--no-cost                Hide cost fields
--timezone <tz>          Date grouping timezone
--compact                Force compact table layout
--config <path>          Load a config file
```

## Data Sources

Reads local logs. Nothing is uploaded.

| Source | Default location |
| --- | --- |
| Claude Code | `~/.claude`, `~/.config/claude/projects` |
| Codex | `${CODEX_HOME:-~/.codex}` |
| Cursor | Cursor `state.vscdb` plus the signed-in dashboard usage API |
| Antigravity | `%APPDATA%\Antigravity`, `~/.gemini/antigravity/brain` |

## Acknowledgments

Agent Burn started from [ccusage](https://github.com/ccusage/ccusage) by [ryoppippi](https://github.com/ryoppippi). The original local log readers, cost aggregation, and CLI report patterns are the prior work this project builds on.

## License

MIT. Copyright (c) 2025 ryoppippi and 2026 Melvynx.
