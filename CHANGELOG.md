# Changelog

All notable changes to Port Manager are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Planned for v0.2.0: project manifest reading. See [`docs/project-manifests-roadmap.md`](docs/project-manifests-roadmap.md).

## [0.1.0] — 2026-04-21

First tagged release. The feature set below represents everything currently on `main`.

### Added — port scanning and process metadata

- Live port scanning every five seconds via `lsof -i -P -n -F pcLfPnT` (field-mode parsing for stability against whitespace in process names)
- Per-port metadata: PID, command, full command line, executable path, user, CPU% (lifetime average; surfaced as a dash because `ps pcpu` is not real-time), memory (RSS), uptime
- Auto-categorization into Dev Servers, Databases, Web / Proxy, Containers, AI/ML, Message Queues, System, Apps — matched on process name + well-known ports + user
- Orphan detection — processes with no controlling terminal, no parent (or parent is `launchd`), and no supervisor
- `launchd` supervision detection via per-scan `launchctl list` map
- Docker container detection via `docker ps` mapping; surfaces container name, image, status

### Added — UI

- Two grouping modes: by category (default) and by git project
- Project grouping: each process's working directory is walked upward (capped at 20 levels, with `canonicalize` to resolve symlinks) looking for `.git`. Supports both directory and file forms of `.git` (worktrees and submodules)
- Project group affordances: folder icon, basename, full path tooltip, click-to-copy path, kill-project button (hidden on the Unassigned bucket), group-select checkbox
- Unassigned bucket for processes not launched from any git repo — muted red outline to distinguish from real project groups
- Category filter tabs (hidden in project mode to avoid dual-axis filtering confusion)
- Search across process name, port, and command — works in both grouping modes
- Next-free-port suggester — type a base port (or leave blank to default to 3000), click "Next" or press Shift+Enter, get the first unused port in the range copied to clipboard
- FREE button — direct kill of the process on a typed port (red, destructive); kept distinct from Next for unambiguous safe/destructive separation
- Confirmation modals for all kill flows; modal title names the scope ("Kill 3 ports in client-dashboard?")
- Kill flow signals: SIGTERM by default; SIGKILL on opt-in via the modal
- Multi-select via checkboxes with bulk-kill bar
- macOS menubar (system tray) icon with dynamic current-state menu
- Native macOS notifications for port conflicts (same port on multiple sockets)
- Click-to-copy on port numbers, PIDs, full paths, and project paths
- Light/dark theme toggle driven by CSS custom properties; preference persists in `localStorage`
- Toast notifications for kill results, copy actions, and suggester output

### Added — history

- Port history view — every port that comes up or dies is logged with PID, process, port, start time, end time, end reason (`killed_by_user`, `exited`)
- History persists to `~/Library/Application Support/port-manager/history.json` across launches
- History search (filter rows by process or port)

### Added — packaging and infrastructure

- Tauri 1.x packaging — single-binary `.app` and `.dmg` (~6 MB release build)
- macOS system tray integration
- GitHub Actions CI: `cargo check`, `cargo clippy -D warnings`, `cargo build --release`, JS parse check on the inline `<script>` block
- Architecture documentation at `docs/architecture.md` covering the six major design decisions

### Known limitations

- macOS only (uses `lsof`, `launchctl`, `docker`, native notifications, `.icns` icons)
- TCP/UDP **LISTEN** state only — does not show ESTABLISHED / TIME_WAIT connections
- Unsigned release binary — first launch requires right-click → Open in Finder to bypass Gatekeeper
- CPU% column shows a dash (lifetime average from `ps` is misleading at scan cadence; Activity Monitor remains the right tool for real-time CPU)
- No remote / cloud port visibility — local-only by design
- No process launching — observation only

[Unreleased]: https://github.com/RajParmar2003/port-manager/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/RajParmar2003/port-manager/releases/tag/v0.1.0
