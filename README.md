# ⌘ Port Manager

A native macOS app that scans all open ports on your machine, groups them by category, and lets you kill them — no terminal commands required.

Built with **Tauri** (Rust backend) + vanilla HTML/CSS/JS frontend.

![Port Manager](https://img.shields.io/badge/platform-macOS-blue) ![Tauri](https://img.shields.io/badge/built_with-Tauri-orange)

---

## Features

- **Real port scanning** — runs `lsof -i -P -n | grep LISTEN` under the hood
- **Smart grouping** — auto-categorizes ports into Dev Servers, Databases, Web/Proxy, Containers, Apps, and System
- **Search & filter** — find ports by process name, port number, or command
- **Kill individual, group, or batch** — select multiple ports and kill them all at once
- **SIGTERM or SIGKILL** — choose graceful shutdown or force kill
- **Safety guards** — confirmation modal before every kill; refuses to kill PID 0 or 1
- **Live rescan** — refresh the port list after killing processes
- **Browser preview mode** — works with mock data if opened in a browser (no Tauri needed to preview the UI)

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

## How It Works

### Scanning Ports (Rust → `lsof`)

The backend runs:
```bash
lsof -i -P -n | grep LISTEN
```

It parses each line to extract PID, process name, port, protocol, user, and state. Then it fetches the full command line for each PID via:
```bash
ps -p <PID> -o command=
```

### Categorization

Each port is auto-categorized based on:
- **Process name** — e.g. `postgres` → Databases, `node` → Dev Servers
- **Port range** — e.g. 3000–3999, 8000–8099 → Dev Servers; 49152+ → Apps
- **User** — `root` or `_*` system users → System

### Killing Ports

The app sends either:
- `kill -15 <PID>` — graceful SIGTERM (default)
- `kill -9 <PID>` — force SIGKILL (toggle in the confirmation modal)

Safety: PID 0 and PID 1 are always refused.

---

## Customization

### Adding categories

Edit the `categorize_port()` function in `src-tauri/src/main.rs` and the `CATEGORIES` object in `src/index.html`.

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
