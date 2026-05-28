# Project Manifests — Implementation Roadmap

**Branch:** `project-manifests` (forked from `next-free-port` @ `09711de`)
**Source idea:** Layer 2 of the Maya moat-building sequence (replaces / reframes IEFIV candidate 5)
**Persona target:** Maya pain #5 — onboarding a new codebase, "what's supposed to be running?"
**Status:** ⚪ Not started (roadmap drafted, pending approval)
**Last updated:** 2026-04-21

---

## Why

Project grouping (Layer 1, shipped) answered *"which of my projects does this running port belong to?"*. Layer 2 answers the inverse: *"which ports does this project expect, and is everything running?"*. That inversion is what turns Port Manager from a **diagnostic tool** into a **workflow tool**.

Concrete Maya moment: she clones a new client repo on Monday morning. Today: `cat README.md`, grep for port numbers, try to remember which terminal runs which service. With Layer 2: she opens Port Manager, sees the new project, sees `expected: 3000, 4000, 5432 · 1/3 running`, knows instantly what's missing.

## What

When the backend scans ports, it also reads a small set of manifest files from each detected project root. The frontend surfaces the result in two places:

1. **Project group header** — a "running count" badge: `2/3 running` (or `all 3 running` when complete)
2. **Ghost rows** in the project group table for expected-but-not-running ports (dashed styling, "not running" pill, no kill button)

### Non-goals (v1 — explicit)

- **No process launching.** Port Manager observes; it does not execute `npm run dev`. That's the original candidate-5 scope creep and stays killed.
- **No package.json / Procfile / .env parsing in v1.** Those cover edge cases; we start with two formats that give 80% coverage: `docker-compose.yml` and our own `.port-manager.yml`.
- **No framework-default inference** ("this project has `next` in package.json, so default expected port is 3000"). Explicit declarations only.
- **No port reservation** or conflict prevention. Read-only surfacing.
- **No manifest editing UI.** Users edit the file themselves; we re-read on next scan.

## Design decisions

| Decision | Choice | Rationale |
|---|---|---|
| Supported formats (v1) | `.port-manager.yml` + `docker-compose.yml` | Our format (simple) + most common multi-service manifest. Covers Maya's primary use case. |
| Parser | `serde_yaml` crate | Small, well-maintained, handles edge cases (anchors, flow style) better than regex. New dep accepted. |
| When parsing runs | Once per scan, per unique project root | Same pattern as project detection — scan-scoped cache. |
| Where results live | New `expected_by_project: HashMap<String, Vec<ExpectedPort>>` in scan result | Decoupled from PortEntry — expected ports belong to projects, not individual port rows. |
| `ExpectedPort` shape | `{ port: u16, label: Option<String>, source: String }` | `source` = which file the expectation came from (e.g. `"docker-compose.yml"`), useful for debugging and future UI affordances. |
| Malformed manifest | Silent fallback — empty expected list for that project, no toast | A broken file shouldn't break the UI. Log to devtools for the curious. |
| docker-compose port specs | Parse short form `"3000:3000"` and long form `{ published: 3000 }`; skip env-var interpolation (`${PORT}`) | Covers common cases. Complex substitution needs runtime context we don't have. |
| `.port-manager.yml` format | `ports: [3000, 4000]` or `ports: [{ port: 3000, label: "web" }]` | Minimal surface; easy to write by hand; extensible. |
| Ghost row styling | Dashed bottom border + muted text + "not running" pill | Visually distinct from real rows without screaming. Theme-adaptive via CSS vars (same pattern as Unassigned bucket). |
| Header badge copy | `2/3 running` (mismatch) or `all 3 running` (complete) | Fast-read, count-forward. |
| Badge color | Accent when complete, `--red` tint when missing | Matches the existing "attention" pattern. |

## Phases

### Phase 1 — Rust parsing + scan integration

- [ ] Add `serde_yaml = "0.9"` to `src-tauri/Cargo.toml`
- [ ] New struct `ExpectedPort { port: u16, label: Option<String>, source: String }` with `Serialize` derive
- [ ] New function `read_port_manager_yml(root: &Path) -> Vec<ExpectedPort>` — reads `.port-manager.yml` if present, parses `ports:` list (both short-int form and object form)
- [ ] New function `read_docker_compose(root: &Path) -> Vec<ExpectedPort>` — reads `docker-compose.yml` / `docker-compose.yaml`, walks `services.*.ports`, extracts host-side port from short/long forms, skips `${VAR}` patterns
- [ ] New function `read_project_manifests(root: &Path) -> Vec<ExpectedPort>` — combines both sources, dedupes by port number (later source wins on label)
- [ ] Scan populates `expected_by_project: HashMap<String, Vec<ExpectedPort>>` alongside `Vec<PortEntry>` and returns both
- [ ] `cargo check` and `cargo build --release` clean with zero warnings

**Acceptance:** A test project with `.port-manager.yml` containing `ports: [3000, 4000]` yields `[{port:3000, ...}, {port:4000, ...}]`. A `docker-compose.yml` with `services.web.ports: ["3000:3000"]` yields `[{port:3000, source:"docker-compose.yml"}]`. A malformed file yields `[]`.

### Phase 2 — Frontend rendering

- [ ] New state `state.expectedByProject` populated from scan result
- [ ] Helper `getExpectedForProject(projectPath)` — returns `{expected: [...], running: [...], missing: [...]}` derived from expected list + current ports
- [ ] Project group header gains a running-count badge (`2/3 running` or `all N running`), styled per design-decisions table
- [ ] Project group table appends ghost rows for each missing expected port — dashed row treatment, "not running" pill, no interactive affordances
- [ ] Category mode untouched — manifests only surface in project mode (deliberate; category cuts across projects)
- [ ] Unassigned bucket untouched — no manifest concept for non-git ports
- [ ] Mock data extended to include two projects with expected-port variants: one "all running", one partial

**Acceptance:** Project mode shows `2/3 running` on a group with one missing port. That group's table shows the missing port as a ghost row with "not running" pill. Toggle to category mode hides all manifest UI.

### Phase 3 — Verification & integration pass

- [ ] `cargo check` + `cargo build --release` clean (zero warnings)
- [ ] JS parses clean (`new Function` on extracted script block)
- [ ] Live-app scan: create a real `.port-manager.yml` in a test repo, verify the expected ports surface in the app
- [ ] Live-app scan: create a real `docker-compose.yml` in a test repo with `services.web.ports: ["3000:3000"]`, verify parsing
- [ ] Malformed-manifest test: corrupt YAML in a manifest → app still renders, other projects unaffected
- [ ] Category mode regression: toggle away from project mode, confirm no manifest UI leaks
- [ ] Dark + light screenshot matrix (project mode with partial running, all running, and no manifest states)
- [ ] Kill-project and kill-row flows untouched by manifest changes

**Acceptance:** All 12 test-matrix rows below green. Screenshots captured.

---

## Test matrix

| # | Scenario | Expected | Phase |
|---|---|---|---|
| 1 | `.port-manager.yml` with short-int ports list | Ports surface as expected | 1 |
| 2 | `.port-manager.yml` with object-form ports (with labels) | Labels surface, ports correct | 1 |
| 3 | `docker-compose.yml` short form `"3000:3000"` | Port 3000 in expected list | 1 |
| 4 | `docker-compose.yml` long form `{ published: 3000 }` | Port 3000 in expected list | 1 |
| 5 | `docker-compose.yml` with `${PORT}` interpolation | Entry skipped, no crash | 1 |
| 6 | Malformed YAML | Empty expected list, no panic | 1 |
| 7 | Project with 3 expected, 2 running | Header badge `2/3 running`, 1 ghost row | 2 |
| 8 | Project with all expected running | Header badge `all N running`, no ghost rows | 2 |
| 9 | Project with no manifest | No badge, no ghost rows (current behavior preserved) | 2 |
| 10 | Category mode | Zero manifest UI visible | 2 |
| 11 | Dark + light screenshot consistency | Ghost rows + badge readable in both themes | 3 |
| 12 | Release build clean, JS parses | 0 warnings | 3 |

---

## Risk register

| # | Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| R1 | `serde_yaml` pulls transitive deps that bloat binary | Low | Low | Check `cargo tree` after Phase 1; if severe, switch to hand-parser for the two specific formats we support |
| R2 | docker-compose spec has variant we don't handle → user thinks feature is broken | Medium | Medium | Log parse failures to devtools; document supported subset in README |
| R3 | Ghost rows confuse users who don't know what "expected" means | Low | Medium | Tooltip on the header badge explaining the mechanic; keep ghost-row pill copy unambiguous ("not running") |
| R4 | Reading manifests on every scan hits slow file I/O on network-mounted repos | Low | Low | Cache per scan scope; if real-world-slow, add manifest-hash cache across scans |
| R5 | Expected-port list conflicts with actual running port on a different project (e.g. two projects both expect 3000) | Medium | Low | Expected is scoped per-project — no cross-project matching. If both projects are running on 3000, only one row matches; the other shows as ghost. Correct behavior. |

---

## Execution

Three phases, one commit per phase. B1 = Phase 1 (Rust), B2 = Phase 2 (frontend), B3 = Phase 3 (verification) — same cadence as candidate 1. No need for atomic bundling; each phase is independently reviewable.

## Progress tracker

| Phase | Status | Opened | Closed | Notes |
|---|---|---|---|---|
| 1 — Rust parsing | ⚪ Not started | — | — | — |
| 2 — Frontend rendering | ⚪ Not started | — | — | Blocks on 1 |
| 3 — Verification | ⚪ Not started | — | — | Blocks on 2 |

Legend: ⚪ Not started · 🟠 Written · 🟡 Committed, unverified · 🟢 Verified

## Change log

- **2026-04-21** — Roadmap drafted. Branch `project-manifests` forked from `next-free-port@09711de`. Pending user approval.
