# ⌘ Port Manager

A native macOS app that scans all open ports on your machine, groups them by category, and lets you kill them — no terminal commands required.

Built with **Tauri** (Rust backend) + vanilla HTML/CSS/JS frontend.

![Port Manager](https://img.shields.io/badge/platform-macOS-blue) ![Tauri](https://img.shields.io/badge/built_with-Tauri-orange)

---

## Features

- **Real port scanning** — parses machine-readable `lsof -F` output under the hood
- **Smart grouping** — auto-categorizes ports into Dev Servers, Databases, Web/Proxy, Containers, Apps, and System (see [Categorization](#categorization))
- **Auto-rescan every 3s** — silent background refresh; footer pulse dot goes amber if updates stall
- **Search & filter** — find ports by process name, port number, or command, with a live "showing N of M" summary
- **Sort** — by port, process, or PID, ascending or descending
- **Click-to-copy** — port, PID, or command path copy to clipboard in one click
- **Kill individual, group, or selected** — single-row kill is one-click; group and bulk kills require confirmation
- **SIGTERM or SIGKILL** — graceful stop by default; force-kill toggle explained inline in the modal
- **Safety guards** — the backend refuses PID 0 and PID 1; rows owned by `root`/system users show a shield warning
- **Keyboard-driven** — ⌘R rescan, ⌘F search, ⌘1–5 filter, Esc clears, Enter confirms modal
- **Light/dark theme** — persists across sessions
- **Resizable window** with minimum 700×500
- **Browser preview mode** — open `src/index.html` in any browser for mock-data preview (no Tauri needed)

---

## Project Structure

```
port-manager/
├── package.json
├── README.md
├── src/
│   └── index.html          ← Full frontend (HTML + CSS + JS)
└── src-tauri/
    ├── Cargo.toml
    ├── build.rs
    ├── tauri.conf.json      ← Tauri window & app config
    └── src/
        └── main.rs          ← Rust backend (lsof parsing, kill commands)
```

---

## Prerequisites

1. **Rust** — install via [rustup.rs](https://rustup.rs/)
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. **Tauri CLI** — install globally
   ```bash
   cargo install tauri-cli
   ```

3. **Xcode Command Line Tools** (macOS)
   ```bash
   xcode-select --install
   ```

---

## Getting Started

### 1. Clone / copy the project

```bash
cd port-manager
```

### 2. Run in development mode

```bash
cargo tauri dev
```

This compiles the Rust backend and opens the app window. The frontend hot-reloads on save.

### 3. Build for production

```bash
cargo tauri build
```

This produces a `.app` bundle and `.dmg` installer in `src-tauri/target/release/bundle/`.

### 4. Preview UI only (no Tauri)

Just open `src/index.html` in any browser — it falls back to mock data automatically.

```bash
open src/index.html
```

---

## Keyboard shortcuts

| Shortcut | Action |
|----------|--------|
| `⌘R` | Rescan ports manually |
| `⌘F` | Focus the search box |
| `⌘1` | Show All |
| `⌘2` | Filter to Dev Servers |
| `⌘3` | Filter to Databases |
| `⌘4` | Filter to Web / Proxy |
| `⌘5` | Filter to Containers |
| `Esc` | Close modal → dismiss error toast → clear search → blur input (in that order) |
| `Enter` | Confirm kill (when modal is open) |

The `⌘1–5` mapping is fixed and does not shift based on which categories currently have open ports, so muscle memory works even after you kill everything in a category.

---

## How It Works

### Scanning ports (Rust → `lsof -F`)

The backend calls `lsof` with field-output mode for machine-readable parsing — not line-based `grep`:

```bash
lsof -i -P -n -F pcLfPnT
```

See [`parse_lsof_field_output`](src-tauri/src/main.rs) in [main.rs](src-tauri/src/main.rs) for the parser. Each record carries PID, command name, user, protocol, port, and TCP state. The full command line for each PID comes from `ps -p <PID> -o command=`.

The frontend auto-polls the backend every 3 seconds ([`AUTO_SCAN_MS`](src/index.html) in [index.html](src/index.html)). Silent polls only re-render when the port set actually changes; the footer pulse dot turns amber if a scan hasn't completed in 10s.

### Categorization

Each port is auto-categorized in [`categorize_port`](src-tauri/src/main.rs) based on process name, well-known port number, and user:

| Category      | Triggers (examples) |
|---------------|---------------------|
| Dev Servers   | `node`, `python`, `go`, `bun`, ports 3000–3999, 4000–4999, 5000–5999, 8000–8099, 8080–8089, 9000–9099 |
| Databases     | `postgres`, `mysql`, `redis-server`, `mongod`, ports 5432, 3306, 6379, 27017, 11211, 9042, 5984, 7474, 9200, 8123 |
| Web / Proxy   | `nginx`, `httpd`, `caddy`, `traefik`, ports 80, 443, 8443 |
| Containers    | `docker`, `containerd`, `kubelet`, `podman`, ports 2375, 2376, 2377, 10250, 10255 |
| Apps          | `spotify`, `slack`, `chrome`, `figma`, ephemeral ports > 49152 |
| System        | `sshd`, `launchd`, `mDNSResponder`, user `root`, user `_*`, ports 22, 53, 631 |
| Other         | anything that doesn't match above |

### Killing ports

The app sends either:
- `kill -15 <PID>` — graceful SIGTERM (default). The process gets a chance to clean up.
- `kill -9 <PID>` — force SIGKILL (toggle in the confirmation modal). Immediate termination; the process cannot clean up.

Single-row kill (the red ✕ on a row) fires immediately with SIGTERM — this is an intentional per-row action. Group kill and bulk kill always open a confirmation modal first. Modal copy tells you exactly which signal will be sent.

### Safety rails

- The Rust backend refuses any request to kill PID 0 or PID 1 ([main.rs](src-tauri/src/main.rs)).
- Rows owned by `root` or system users (`_*`) display an amber shield icon — a visual warning that killing them may destabilize your machine.
- `Permission denied` errors (e.g. trying to kill another user's process without sudo) surface as sticky error toasts that must be dismissed explicitly.

---

## Customization

### Adding categories

Edit the `categorize_port()` function in [src-tauri/src/main.rs](src-tauri/src/main.rs) (authoritative) and add a matching entry to both `CATEGORIES` and `CATEGORY_ICON_PATHS` in [src/index.html](src/index.html) (color + SVG icon paths).

### Changing the window size

Edit `src-tauri/tauri.conf.json` → `tauri.windows[0]`.

### Adding an app icon

Replace the placeholder icon paths in `tauri.conf.json` with your own `.icns` / `.png` files in `src-tauri/icons/`.

---

## Tech Stack

| Layer    | Technology       | Why                                      |
|----------|-----------------|------------------------------------------|
| Backend  | Rust + Tauri    | Native performance, small binary (~5MB)  |
| Frontend | Vanilla HTML/JS | Zero build step, instant hot reload      |
| IPC      | Tauri invoke    | Type-safe Rust ↔ JS communication        |
| Styling  | CSS variables   | Dark theme, easy to customize            |

---

## Troubleshooting

**"Failed to run lsof"** — The app needs shell access. Make sure you're running on macOS and that `lsof` is available at `/usr/sbin/lsof`.

**"Permission denied" when killing a port** — Some ports (especially system ones running as `root`) require admin privileges. Run the app with `sudo` or use `cargo tauri dev` from a terminal with elevated permissions.

**Port still showing after kill** — Some processes take a moment to release the port. Click "Rescan" after a second or two.

---

## License

MIT — do whatever you want with it.
