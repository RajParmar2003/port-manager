# Project-Aware Grouping — Implementation Roadmap

**Branch:** `project-grouping` (forked from `v2-features` @ `664305c`)
**Source idea:** Candidate 1 from the IEFIV feature review (2026-04-20)
**Status:** ⚪ Not started (roadmap locked, Phase 1 next)
**Last updated:** 2026-04-20

---

## Why

Port Manager today groups ports by **category** (Dev Servers, Databases, Web / Proxy, ...). That categorization is accurate but cuts across the mental model devs actually use. When you're debugging a stale dev server, you don't think "which of my *Dev Servers* is stale?" — you think "which of my *client-dashboard* ports is stale?"

A single Next.js project can open three or four ports (next dev, webpack HMR, storybook, mock API). Today they scatter across the Dev Servers group, indistinguishable from other projects' dev servers. Project-aware grouping re-anchors the UI on the project, which is the unit devs think in.

## What

Add a **"Group by: Category | Project"** toggle. In project mode, ports are grouped by the git repo their process was launched from (detected by walking upward from the process CWD until a `.git` directory appears). Ports whose process has no discoverable repo fall into an **"Unassigned"** bucket.

Non-goals (explicit):
- No renaming, pinning, or reordering projects (phase-2 polish item if demand warrants).
- No auto-detection of non-git project roots (`package.json`, `Cargo.toml`, etc.). `.git` is near-universal and avoids ambiguity.
- No cross-machine / remote project resolution.

## Design decisions

| Decision | Choice | Rationale |
|---|---|---|
| Detection anchor | `.git` directory (walk up from CWD) | Near-universal, unambiguous, cheap. `package.json` would conflict with monorepos; nested repos are rare enough to ignore. |
| CWD source | `lsof -p PID -a -d cwd -Fn` | Already using lsof; no new system dependency. macOS-friendly. |
| Cache key | PID | Processes don't change CWD after boot (almost always). |
| Cache lifetime | One scan cycle | Re-resolve each scan in case a process dies and PID is reused (rare but possible). |
| Display name | Basename of project root path | Short, readable. Full path as hover tooltip. |
| Unassigned bucket | "Unassigned" pseudo-project, sorted last | Clear, non-judgmental naming. |
| Toggle persistence | `localStorage['pm-grouping']`, default `'category'` | Same mechanism as existing theme preference. |
| Category tabs in project mode | Hidden | Avoids confusing dual-axis filtering in v1. Search stays active. |
| Empty project mode (no projects detected) | Shows helpful empty state pointing back at Category mode | Avoids dead-end UI. |
| Kill-project button | Present on each project header, same confirmation as kill-group | Consistency with existing destructive-action pattern. |

## Phases

Each phase has an acceptance checklist. A phase is complete when every box ticks.

### Phase 1 — Backend detection (Rust)

Add project resolution to the scan pipeline.

- [x] New field `project: Option<String>` on `PortEntry`
- [x] New function `get_project_for_pid(pid) -> Option<String>` — uses `lsof -p PID -a -d cwd -Fn` to get CWD, walks upward looking for `.git`, returns the project root absolute path (no trailing slash)
- [x] Walk caps at 20 parent levels; uses `canonicalize` to resolve symlinks and detect loops
- [x] `scan_ports` populates `project` for every entry, in-scan PID cache prevents double-lookup for multi-port PIDs
- [x] `cargo check` passes with zero warnings (verified 2026-04-20, `Finished dev profile in 34.13s`, no warnings)
- [ ] Manual sanity: scan shows non-None `project` for at least one process whose CWD is within a git repo *(pending live-app run — requires Tauri runtime)*

**Acceptance test:** Run the app with a node dev server in a git repo and a service not in any git repo (e.g. a system daemon). Scan output (as seen in frontend devtools `state.ports`) must show the dev server's `project` field as the repo root and the system daemon's `project` as `null`.

### Phase 2 — Grouping mode toggle + render (frontend)

Add the toggle. Wire project mode through `render()`.

- [x] New state: `state.groupBy` (`'category'` | `'project'`), loaded from `localStorage['pm-grouping']`, default `'category'`
- [x] New segmented toggle in the category tab bar area: **Group: [Category | Project]**
- [x] `setGroupBy(mode)` handler updates state and persists
- [x] New helper `getGroupedByProject(filtered)` returns sorted `[{key, label, path, items, isUnassigned}]` with Unassigned last
- [x] `render()` branches on `state.groupBy` to call either `getGrouped` (category) or `getGroupedByProject` (project)
- [x] Project mode hides the category filter tabs (they still exist for category mode)
- [x] `getFiltered` ignores `state.filterCat` in project mode so filter doesn't silently leak across modes

**Acceptance test:** Toggle switches between modes. Refresh page — preference persists. In project mode, ports group by detected project path; orphans fall under "Unassigned."

### Phase 3 — Project headers & affordances

Give project groups the same quality of affordances as category groups.

- [x] Project group header shows: folder-icon SVG, basename, port count, full path as `title` tooltip (plus an inline truncated path hint)
- [x] Project group header has a "Kill project" button (accent-tinted, same sizing as "Kill group"). Hidden on Unassigned bucket (killing "not in a repo" doesn't parse semantically)
- [x] Kill-project flows through the existing confirmation modal via `openModal('project', projectKey)`
- [x] Group-select checkbox in the header toggles selection of all ports in the project via `toggleProjectGroup(projectKey)`
- [x] Clicking the header basename copies the full path to clipboard (disabled for Unassigned)
- [x] Mock data seeded with three projects' worth of variety: client-dashboard (2 ports), api-server (1 port), 4 ports with `project: null` for Unassigned bucket

**Acceptance test:** Each project group has a folder icon + basename + count + "Kill project" button. Hovering the basename shows the full path. Clicking "Kill project" opens the confirmation modal naming the project.

### Phase 4 — Filter/search integration + persistence

Make the two modes co-exist cleanly with the other UI.

- [ ] Search query filters ports in both modes identically (already happens via `getFiltered` — just verify)
- [ ] In project mode, category filter defaults to "All" and the category tabs are hidden (no mode leakage)
- [ ] Switching from project → category mode restores the last-used category filter
- [ ] Empty states are context-aware in project mode too (no ports / no matches / no detectable projects)
- [ ] `localStorage['pm-grouping']` round-trips correctly across reloads

**Acceptance test:** Search for "node" in project mode → only matching ports remain, projects with zero matches are hidden. Toggle to category → search persists. Clear search, toggle back to project → all projects visible again.

### Phase 5 — Verification & integration pass

Full matrix. Nothing merges until this is green.

- [ ] `cargo check` clean, `cargo build --release` clean
- [ ] JS parse clean (no inline eval errors; tested via `node -e` on the extracted script block)
- [ ] Full keyboard flow in project mode: ⌘F search, Esc to clear, Enter to confirm modal
- [ ] Theme toggle works in project mode (both light and dark screenshots captured)
- [ ] Kill flows: single-row kill, kill-project, kill-selected, all produce correct modals and correct toast reports
- [ ] Orphan/managed/docker meta tags still render correctly on rows in project mode
- [ ] History view untouched
- [ ] Browser preview (mock data) works end-to-end in both modes
- [ ] Screenshot matrix attached to this doc:
  - [ ] Category mode — dark
  - [ ] Category mode — light
  - [ ] Project mode — dark
  - [ ] Project mode — light
  - [ ] Unassigned bucket visible with at least one entry

**Acceptance test:** Run the full test matrix below. Every row must pass.

---

## Execution batches

The 5 phases ship in 4 batches. Phase 2 and Phase 3 bundle together because the frontend is one coherent surface: shipping the toggle without the headers it renders would mean a broken intermediate.

| Batch | Contents | Rationale | Gate to close |
|---|---|---|---|
| B1 | Commit Phase 1 Rust + this roadmap | Unblocks everything; converts Phase 1 from 🟠 to 🟡 | Commit on branch, `cargo check` still green |
| B2 | Phases 2 + 3 (state.groupBy, toggle, getGroupedByProject, render branching, headers, mock data) | Frontend must ship atomically — partial would break preview | Toggle works end-to-end in browser preview with mock data; JS parses; commit lands |
| B3 | Phase 4 (search both modes, tab hiding, filter restore on toggle-back, empty states, localStorage round-trip) | Pure polish on top of B2 | Matrix rows 4, 10, 11, 12, 13 all pass in preview; commit lands |
| B4 | Phase 5 (release build, JS parse, keyboard/theme/kill checks, meta tags, history, 5-screenshot matrix, PR off v2-features) | Verification is cheaper done once against the integrated result | All 20 matrix rows ✅; PR opened |

## Live progress tracker

Update here as phases close. Also mirror to the checkboxes in each phase above.

| Phase | Status | Opened | Closed | Notes |
|---|---|---|---|---|
| 1 — Backend detection | 🟡 Committed, unverified | 2026-04-20 | — | Field, function, cache, scan wiring in. `cargo check` clean. Live-app verification deferred to B4 (Phase 5 integration pass). |
| 2 — Mode toggle + render | 🟡 Committed, unverified | 2026-04-21 | 2026-04-21 | B2 landed: state + toggle + getGroupedByProject + render branching + getFiltered mode-aware. JS parses. Live visual deferred to B4. |
| 3 — Project headers | 🟡 Committed, unverified | 2026-04-21 | 2026-04-21 | B2 landed: folder icon, basename-as-copy-button, path hint + tooltip, Kill-project button, group-select checkbox, Unassigned bucket, 3-project mock data. |
| 4 — Integration/persistence | ⚪ Not started | — | — | Batch B3, requires B2 |
| 5 — Verification | ⚪ Not started | — | — | Batch B4, requires B3 |
| 3 — Project headers | ⚪ Not started | — | — | Blocked on Phase 2 |
| 4 — Integration/persistence | ⚪ Not started | — | — | Blocked on Phase 3 |
| 5 — Verification | ⚪ Not started | — | — | Blocked on Phase 4 |

Legend: ⚪ Not started · 🟠 Written (uncommitted) · 🟡 Committed, unverified · 🟢 Verified · 🔴 Blocked

"Written" = edits exist on disk in the worktree. "Committed" = on the branch, reviewable, survives a worktree wipe. "Verified" = exercised against the running app (or the browser preview with mock data, for frontend-only work).

---

## Test matrix

Run at end of each phase. Record results (✅/❌) with date.

| # | Scenario | Expected | Phase | Result |
|---|---|---|---|---|
| 1 | Scan shows `project` field populated for a git-repo process | Non-null absolute path | 1 | — |
| 2 | Scan shows `project` as `null` for a non-git-repo process (sshd) | `null` | 1 | — |
| 3 | Toggle Category → Project updates the list in place (no scan) | Instant re-render, same ports | 2 | — |
| 4 | Toggle preference survives page reload | `localStorage.pm-grouping` persists | 4 | — |
| 5 | Project header shows basename, count, full path tooltip | ✓ all three | 3 | — |
| 6 | Hover basename → full path tooltip appears | Native `title` on span | 3 | — |
| 7 | Click basename → full path copies to clipboard | Success toast "Copied …" | 3 | — |
| 8 | "Kill project" button opens modal naming the project | Modal title includes basename | 3 | — |
| 9 | Ports with `project===null` appear in "Unassigned" bucket, sorted last | Unassigned always last | 2 | — |
| 10 | Search filter narrows ports in project mode identically to category | Same match count | 4 | — |
| 11 | Project mode hides category filter tabs | Tabs not rendered | 4 | — |
| 12 | Switching project → category restores prior category filter | `state.filterCat` preserved | 4 | — |
| 13 | Empty project mode (no projects detected) shows helpful empty state | Copy mentions "no projects found" | 3 | — |
| 14 | Kill-project kills all ports in that project, updates list, shows toast | Ports gone, toast ok | 3 | — |
| 15 | `cargo check` green after Rust changes | No warnings | 1 | ✅ 2026-04-20 (34.13s, zero warnings) |
| 16 | `node -e` parse of script block green after frontend changes | No syntax errors | 2/3 | — |
| 17 | Theme toggle still works in project mode | Sun/moon toggles, theme applies | 5 | — |
| 18 | Meta tags (orphan/managed/docker) render in project mode rows | Tags present | 5 | — |
| 19 | History view unchanged | Opens, renders, search works | 5 | — |
| 20 | Screenshot matrix captured | 5 screenshots saved | 5 | — |

---

## Risk register

| # | Risk | Likelihood | Impact | Mitigation | Trigger signal |
|---|---|---|---|---|---|
| R1 | `lsof -p PID -d cwd` returns "access denied" for processes owned by other users | High on macOS | Medium — those ports all fall under "Unassigned" | Document as known limitation; consider sudo hint in tooltip later | Many/all ports land in Unassigned when they shouldn't |
| R2 | `.git` walk performance under many-process scans | Low | Medium | PID-keyed cache within a scan; walk caps at 20 levels | Scan takes noticeably longer (>500ms) |
| R3 | Symlinked CWDs (e.g. Docker-for-Mac bind-mounted paths) resolve to surprising roots | Medium | Low | Use `canonicalize`; document resolution rule | User reports "this port shows wrong project" |
| R4 | User expects project to mean package.json / Cargo.toml root, not git root | Low | Low | Document behavior in README. Could add heuristic in v2 | User feedback says "why is my project showing /Users/me instead of /Users/me/proj?" |
| R5 | Grouping toggle takes up header space, crowding the layout | Low | Low | Compact segmented control; hide on narrow widths if needed | Visual review flags crowding |
| R6 | v2-features still has uncommitted Rust refactor in the main working tree — merge conflict risk | Medium | Medium | Keep project-grouping on its own worktree; don't touch main.rs Cargo.toml in ways that conflict | Merge back fails with conflicts |

---

## How progress gets checked

Three nested feedback loops. Each answers the question "is this phase on track?" with progressively more rigor.

**Loop 1 — Per-edit self-check (every edit):**
- Rust: `cargo check` after any backend change
- Frontend: `node -e "new Function(script)"` to confirm no syntax errors
- Visual: screenshot after any render-affecting change

**Loop 2 — Per-phase acceptance (end of each phase):**
- Tick every box in that phase's acceptance checklist
- Update this doc's status tracker and close the phase with a date
- Run every test-matrix row tagged with that phase number
- If any test fails: don't advance; note failure in Open Issues; fix; re-test

**Loop 3 — Final integration pass (end of Phase 5):**
- Run the entire test matrix top-to-bottom
- Capture the 5-screenshot matrix
- `cargo build --release` must succeed
- Open Issues list must be empty

If a phase slips by more than one session, the phase opens an entry in the **Open Issues** section below with the blocker and the next concrete step.

---

## Open issues

Track slippage and unknowns here. Empty = on track.

_(none yet — roadmap just created)_

---

## Change log

Every material decision or scope change gets a line. Append-only.

- **2026-04-20** — Roadmap created. Branch `project-grouping` forked from `v2-features@664305c`. Scope: Candidate 1 from IEFIV review.
- **2026-04-20** — Explicit non-goals locked: no renaming/pinning, no package.json detection, no remote resolution.
- **2026-04-20** — Phase 1 backend code written. `PortEntry.project: Option<String>`, `get_cwd_for_pid`, `find_git_root`, `get_project_for_pid` with per-scan PID cache, wired into `do_scan_ports`. `src-tauri/Cargo.toml` detached from the parent workspace with an empty `[workspace]` table. `cargo check` green in 34.13s, zero warnings. **Uncommitted at this point.**
- **2026-04-20** — Batching plan locked: B1 commits Phase 1, B2 lands Phases 2+3 together (frontend ships atomically), B3 is Phase 4 polish, B4 is Phase 5 verification + PR.
- **2026-04-20** — Status legend revised to 3 gates (Written / Committed / Verified) after noticing that "code complete" was being conflated with "on the branch."
- **2026-04-20** — B1 committed: Phase 1 Rust + roadmap on `project-grouping`. Phase 1 moves 🟠 → 🟡.
- **2026-04-21** — B1 pushed to `origin/project-grouping`.
- **2026-04-21** — B2 shipped: `state.groupBy` + localStorage init, `setGroupBy` handler, `getGroupedByProject` helper, `basename` helper, `toggleProjectGroup` handler, `openModal` extended to handle `'project'` scope, `getFiltered` made mode-aware so category filter doesn't silently leak into project mode. Extracted `renderPortRow` to share row markup between both grouping branches. Frontend gets a "Group by [Category | Project]" segmented control; project mode hides category tabs; project headers get folder icon + basename (click-to-copy path) + truncated path hint + Kill-project button + group-select checkbox. Mock data seeded with 3 projects' worth of spread. JS parses clean. Live visual verification deferred to B4 — the preview tool is locked to a different port/source and can't navigate to the worktree's preview server (known tooling limitation, not a code issue).

---

## Notes for future me

- If we ever add project pinning or reordering, the sort key moves from "project path ascending" to a user-defined order in localStorage.
- If we add non-git project detection (`package.json` etc.), `get_project_for_pid` takes a list of anchor filenames and the first match wins.
- The `project` field on `PortEntry` is `Option<String>` precisely so a future v2 can make it richer (struct with path + display-name-override) without breaking the wire format — JSON serializes `None` to `null` either way.
