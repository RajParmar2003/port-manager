# Port Manager

[![CI](https://github.com/RajParmar2003/port-manager/actions/workflows/ci.yml/badge.svg)](https://github.com/RajParmar2003/port-manager/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/RajParmar2003/port-manager?include_prereleases&sort=semver)](https://github.com/RajParmar2003/port-manager/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Platform: macOS](https://img.shields.io/badge/platform-macOS-lightgrey)](https://github.com/RajParmar2003/port-manager)

A native macOS desktop app that scans every TCP/UDP port held by every process on your machine, groups them by the git repository the process was launched from, and lets you kill them — without dropping into `lsof | grep | kill`.

## 30-second pitch

If you're juggling more than two services locally, this is the loop you know:

```
$ npm run dev
error: listen EADDRINUSE: address already in use :::3000
$ lsof -i :3000
COMMAND   PID  USER   FD  TYPE  ...
node    14823  raj   24u  IPv6  ...
$ kill -9 14823
$ npm run dev
```

Six commands, two windows, all to do one thing. Port Manager replaces that loop with a window: scroll to the port, click kill, done. And because every process is tagged with the git repo it came from, you can see at a glance that the `node` on 3000 belongs to `client-dashboard`, not to the API service you were debugging an hour ago.

## Quick demo

1. **Scan** — Port Manager opens, shows every listening port grouped by category or by git project.
2. **Find** — search by process name, port number, or command. Or filter by category.
3. **Kill** — single row, whole project, or multi-selected ports. SIGTERM by default; SIGKILL on opt-in.
4. **Need a port** — type a starting number, click **Next**, get the first free port copied to your clipboard.

## Features

- **Live port scanning** — `lsof`-driven, refreshes every five seconds, sub-100ms per scan
- **Group by category** — Dev Servers, Databases, Web / Proxy, Containers, AI/ML, Message Queues, System, Apps
- **Group by git project** — each port tagged with the repo its process was launched from; "Unassigned" bucket for system daemons
- **Next-free-port suggester** — type a base port, get the next unused one copied to your clipboard
- **Kill flows** — single row, whole category, whole project, multi-selected. Confirmation modal names the scope. SIGTERM by default; SIGKILL on opt-in.
- **Process supervision awareness** — `launchd`-managed services get `launchctl stop` instead of raw kill, so supervisors don't respawn-loop
- **Docker context** — container-backed ports show the container name, image, and status
- **Orphan detection** — flags processes with no controlling terminal, no parent, and no supervisor — the classic "I forgot I started that" case
- **Port history** — every port that comes up or dies gets logged to `~/Library/Application Support/port-manager/history.json` for retrospective lookup
- **macOS menubar tray** — right-click for a current-state summary without opening the main window
- **Native notifications** — port conflicts (same port appearing on multiple sockets) raise a system notification
- **Theme** — dark and light, switched via CSS custom properties, preference persists across launches
- **Click to copy** — port numbers, PIDs, full paths, all single-click

## Architecture

Two-process model via [Tauri 1.x](https://tauri.app):

- **Rust backend** (`src-tauri/src/main.rs`, ~1,200 lines) — runs `lsof`, parses field-mode output, gathers process metadata via `ps`, detects supervision via `launchctl` and `docker ps`, walks filesystems looking for `.git` to resolve project roots, exposes Tauri IPC commands for scan / kill / history.
- **Vanilla HTML / CSS / JS frontend** (`src/index.html`, ~1,200 lines) — single file, no build step, no React, no bundler. Renders state via `document.body.innerHTML` replacement. Theming via CSS custom properties.
- **IPC** — Tauri's JSON-over-IPC bridge.

For the design rationale behind each major decision (single-file frontend, filesystem-walking project detection, per-scan PID cache, lsof field-mode parsing, theme system, supervision-aware kill paths), see [`docs/architecture.md`](docs/architecture.md).

## Install

### From source (currently the only path)

Prerequisites:

```bash
# Rust toolchain
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Tauri CLI
cargo install tauri-cli

# Xcode command-line tools (macOS)
xcode-select --install
```

Then:

```bash
git clone https://github.com/RajParmar2003/port-manager.git
cd port-manager
cargo tauri dev      # development mode with hot-reload frontend
cargo tauri build    # release .app + .dmg in src-tauri/target/release/bundle/
```

The release build produces a ~6 MB native binary. The current release is **unsigned** — first launch requires a right-click → Open in Finder to bypass Gatekeeper.

### UI-only preview (no Rust)

Open `src/index.html` directly in any browser. The frontend detects the absence of the Tauri bridge and falls back to mock data — useful for iterating on the UI without a Rust toolchain.

## Configuration

There is no configuration file in v0.1.0. Theme preference and grouping mode persist via `localStorage`. Port history is written to `~/Library/Application Support/port-manager/history.json`.

In v0.2.0 the planned `.port-manager.yml` will let projects declare their expected ports — see [`docs/project-manifests-roadmap.md`](docs/project-manifests-roadmap.md).

## Design decisions worth knowing

| Decision | Why |
|---|---|
| Single-file vanilla JS frontend, zero build step | Codebase is small enough that a component framework adds tooling cost without paying back; `git diff` shows the actual rendered code; no transpilation surface to debug. |
| Walk filesystem for `.git` rather than shelling out to git | A handful of `stat` calls per process beats forking `git` 50 times per scan cycle. Walk capped at 20 levels, `canonicalize` resolves symlinks first. |
| Per-scan PID cache (not global) | PIDs get recycled; a long-lived cache would map a new process to a dead project. Per-scan scope dedupes within a scan without taking on recycle-safety. |
| `lsof` field-mode parsing rather than column parsing | Field mode is stable and unambiguous; column parsing breaks on whitespace in process names or bracketed IPv6 literals. |
| Supervision-aware kill paths | Killing a `launchd`-managed process directly is futile — the supervisor respawns it. Asking the supervisor to stop produces the expected behavior. |

Full rationale for each in [`docs/architecture.md`](docs/architecture.md).

## What this does not do

Setting expectations honestly:

- Does **not** monitor remote machines or cloud ports — local-only by design
- Does **not** launch processes — observation only, no `npm run dev` from the UI
- Does **not** support Linux or Windows — macOS-specific (`lsof`, `launchctl`, `.icns` icons, native notifications)
- Does **not** modify firewall rules or system network state
- Does **not** show ESTABLISHED / TIME_WAIT connections — filters to LISTEN only (the things actually serving)
- Does **not** require kernel extensions or root privileges to scan (kill flows on root-owned processes may need `sudo`)
- Does **not** send any telemetry, ever — fully local, no network calls beyond what the scanned processes themselves make
- Does **not** auto-update — install-time-fixed binary

## Future work

The next planned release is **v0.2.0**, focused on Layer 2 — turning Port Manager from a diagnostic tool into a workflow tool:

- **Project manifests** — read `.port-manager.yml` and `docker-compose.yml` from project roots, surface "N/M running" badge on each project group, show expected-but-not-running ports as dashed ghost rows. ([Roadmap](docs/project-manifests-roadmap.md))

Possible future directions (no commitment):

- Port history intelligence — "port 3000 last ran here, killed at 4:23 yesterday by you"
- VS Code sidebar extension showing this workspace's ports
- Conflict prevention — warn before starting a service that will collide with a project's declared port

## License

MIT — see [LICENSE](LICENSE).

## Author

Built by [Rajwinder Parmar](https://github.com/RajParmar2003).
