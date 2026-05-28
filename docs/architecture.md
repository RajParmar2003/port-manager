# Architecture

Notes on the design decisions that shape Port Manager. Each entry covers the decision, the alternatives that were considered, why the current choice was made, and what would have to change if the project grew significantly.

---

## 1. Single-file vanilla JavaScript frontend

**Decision.** The entire UI lives in `src/index.html` — one file containing the HTML scaffold, inline CSS, and inline JavaScript. No React, no TypeScript, no bundler, no build step.

**Alternatives considered.** A React + Vite frontend with typed Tauri IPC bindings; a Svelte frontend; a Yew (Rust → WASM) frontend.

**Why this choice.** The UI surface is small enough (~1,200 lines including styling) that a component framework would add tooling cost without paying back in maintainability. `git diff` shows the actual rendered code. There is no dependency tree to audit, no transpilation step to debug, and `open src/index.html` in any browser produces a working UI with mock data — useful for quick iteration without spinning up the Rust runtime.

**What would change at scale.** If the codebase grew past ~3,000 lines or needed multiple views with non-trivial state, the lack of a component model would become painful. The render loop currently rebuilds the entire DOM via string concatenation and slams the result into `document.body.innerHTML`; that works at this scale, but a virtual-DOM library would be the right next step.

---

## 2. Project-to-PID resolution via filesystem walking

**Decision.** For each process holding a port, the backend reads its current working directory via `lsof -p PID -a -d cwd -Fn` and walks upward looking for `.git`. The first ancestor containing `.git` is treated as the project root.

**Alternatives considered.** Asking `git` directly via `git -C <cwd> rev-parse --show-toplevel`; reading `package.json` / `Cargo.toml` / `Procfile` to determine project roots; using a configured project registry.

**Why this choice.** Shelling out to `git` per PID adds subprocess overhead — for a machine with 50 listening processes, that is 50 `git` invocations per scan cycle. Walking the filesystem with `Path::canonicalize` and `Path::join(".git").exists()` is a handful of `stat` calls per process, no subprocess. The walk is capped at 20 parent levels so pathological cases (processes whose CWD is `/`) cannot loop indefinitely. `.git` is also more universal than language-specific manifests, which would need a heuristic for monorepos.

**What would change at scale.** If users start asking for non-git project detection (Cargo workspaces with no `.git`, monorepo subprojects, Docker bind mounts that resolve to surprising roots), `find_git_root` becomes a generic `find_project_root` that takes a list of anchor filenames in priority order. The walk-cap and canonicalize calls stay; only the predicate changes.

---

## 3. Per-scan PID cache, deliberately not global

**Decision.** The project-resolution result is cached in a `HashMap<u32, Option<String>>` that lives only for the duration of one scan cycle. Across scans, the cache is discarded and rebuilt.

**Alternatives considered.** A global cache invalidated on process death; a cache with a TTL; no cache at all.

**Why this choice.** PIDs are recycled by the kernel. A long-lived cache would associate a freshly-launched process with the project of a dead process whose PID got reused — a subtle and hard-to-diagnose correctness bug. The per-scan cache eliminates redundant resolution within a single scan (a `postgres` process listening on eight ports gets one lookup, seven cache hits) without taking on the recycle-safety problem. Scans run every five seconds and the project resolution itself is cheap, so rebuilding the cache each cycle is a non-issue.

**What would change at scale.** If scans got slow enough that the cache rebuild was a measurable cost, the right move would be a global cache keyed by `(pid, process_start_time)` rather than `pid` alone — start time disambiguates a recycled PID from a continuous one. The current code returns enough metadata to support that change without changing the call sites.

---

## 4. lsof field-mode parsing rather than table parsing

**Decision.** The scan invokes `lsof -i -P -n -F pcLfPnT`. The `-F` flag puts lsof into field mode — each output line is a single field prefixed by a one-letter code (`p` for PID, `c` for command name, `L` for login user, `n` for socket address, `T` for TCP state, etc.). The parser is a small state machine that starts a new record on each `p` line.

**Alternatives considered.** Parsing the human-readable column layout (`lsof -i -P -n | grep LISTEN`); using a third-party lsof-parsing crate; calling kernel APIs directly via `libproc`.

**Why this choice.** Field mode is documented, stable, and unambiguous. Column parsing breaks the moment a field contains whitespace (process names with spaces, socket addresses with bracketed IPv6 literals). Third-party lsof crates exist but add a dependency for what fits in fifty lines. Direct kernel calls would be faster but tie the project to macOS-specific APIs and require unsafe Rust; the current `lsof` approach is one fewer system surface to maintain.

**What would change at scale.** If scans needed to run more often than every couple seconds, or needed to enumerate hundreds of processes per second, the `libproc` route would become worth the complexity. At today's scale, the overhead of forking `lsof` is invisible.

---

## 5. Theme system via CSS custom properties

**Decision.** All colors flow through CSS custom properties — `var(--accent)`, `var(--surface)`, `var(--text-dim)`, etc. — defined in two `:root` blocks gated by `[data-theme="light"]` and `[data-theme="dark"]`. Switching themes is one attribute change on the root element, persisted to `localStorage`.

**Alternatives considered.** A CSS preprocessor with two compiled stylesheets; a runtime style injection system; baking the dark theme in and letting the OS handle it via `prefers-color-scheme`.

**Why this choice.** Custom properties cascade naturally; every styled element automatically respects the active theme without any per-element wiring. The two-stylesheet approach would require swapping `<link>` tags or duplicating selectors. `prefers-color-scheme` alone would be cleaner but would not give users explicit control — the toggle is a feature, not a fallback.

**What would change at scale.** If a third theme (high-contrast, dim) were added, the structure handles it trivially — add another `[data-theme="X"]` block, no refactor needed. The current code is already extensible in the direction it needs to go.

---

## 6. Process-supervision-aware kill paths

**Decision.** Ports owned by processes managed by `launchd` are detected via a per-scan `launchctl list` map. The UI surfaces a `launchctl stop <label>` option for these processes instead of a raw `kill`. Docker container ports are detected via `docker ps` and surface the container name and image.

**Alternatives considered.** Always sending `SIGTERM` / `SIGKILL` regardless of supervision; not detecting supervision at all and leaving the user to figure it out; offering both options for every process.

**Why this choice.** Killing a `launchd`-supervised process directly is futile in the common case — the supervisor will respawn it immediately, and the user thinks the kill failed when really it was overridden. Asking the supervisor to stop the service produces the expected behavior. Same logic applies to Docker — killing the container's process from outside is a path to confusion when `docker stop <container>` is what the user actually wants. The UI shows the supervision context so the user understands why the action is different.

**What would change at scale.** Adding more supervision systems (systemd on Linux, sv on Void, runit) follows the same pattern: detect at scan time, surface in the UI, route the kill action through the right tool. The current code already has the abstraction shape needed; only the detection logic would be added.

---

## Future work

The roadmap files in this directory track planned and shipped features:

- [`project-grouping-roadmap.md`](./project-grouping-roadmap.md) — shipped in v0.1.0
- [`next-free-port-roadmap.md`](./next-free-port-roadmap.md) — shipped in v0.1.0
- [`project-manifests-roadmap.md`](./project-manifests-roadmap.md) — planned for v0.2.0 (reads `.port-manager.yml` and `docker-compose.yml` from project roots, surfaces expected vs running ports)

The roadmaps follow a consistent format: why, what, non-goals, design decisions, phases with acceptance criteria, test matrix, risk register, change log. They are kept for reference rather than as living planning docs.
