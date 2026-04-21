# Next-Free-Port Suggester — Implementation Roadmap

**Branch:** `next-free-port` (forked from `project-grouping` @ `37ac3c6`)
**Source idea:** Candidate 4 from the IEFIV feature review (2026-04-20)
**Persona target:** Maya pain #2 — daily `EADDRINUSE` when starting services
**Status:** ⚪ Not started (roadmap drafted, pending approval)
**Last updated:** 2026-04-21

---

## Why

Maya starts services all day. Every `next dev` / `python -m uvicorn` / `docker compose up` that hits `EADDRINUSE: address already in use :::3000` costs her ~30 seconds of `lsof | grep | kill` or guessing the next free number. Port Manager already has every LISTEN port in state — we can answer "what's the next free port starting from N?" instantly, with zero new system calls.

## What

A small "Next free" affordance beside the existing "FREE" input:

- User types a starting port (e.g. `3000`)
- Clicks **Next** (or presses ⇧Enter) → app finds the first port ≥ start that's not currently in `state.ports`, copies it to clipboard, shows toast `Next free: 3002 · copied`
- Existing **FREE** button behavior is untouched (still kills the process on typed port)

### Non-goals

- No port-availability probing beyond what the existing scan knows (no `nc`/`lsof` roundtrip per suggestion — scan data is authoritative enough for Maya's use case)
- No port *reservation* (that's Layer 2 / manifests territory)
- No range picker, no "give me 3 free ports" — single-port answer only
- No UDP handling (we only display TCP LISTEN today; consistent scope)

## Design decisions

| Decision | Choice | Rationale |
|---|---|---|
| Backend or frontend | **Frontend-only** | We already have scanned LISTEN ports in `state.ports`. Zero new Rust. Keeps LOC ~40. |
| Collision source | `state.ports` (current scan) | Authoritative for what Maya cares about — "will `next dev` bind here right now" |
| Search range cap | 100 ports from start | Prevents runaway loops; if no free port in 100 we show `No free port in ${start}–${start+99}` |
| Default start port | `3000` if empty | Matches the single most common dev port |
| UX placement | Sibling button to existing FREE | Reuses the same input; one mental model per widget |
| Post-suggestion action | Copy to clipboard + toast | Maya pastes into `.env` / CLI. No modal, no friction. |
| Keyboard | ⇧Enter on the input | Enter already triggers FREE; ⇧Enter becomes the non-destructive sibling |
| Invalid input handling | Silent noop + placeholder hint | No error modal for a benign mistake |

## Phases

### Phase 1 — Logic + handler (frontend)

- [ ] New helper `findNextFreePort(start, limit=100)` returning `number | null`
- [ ] New handler `doSuggestFreePort()` that reads the input, calls helper, copies to clipboard, shows toast (success or "no free port in range")
- [ ] ⇧Enter on `.free-port-input` triggers `doSuggestFreePort()`; plain Enter still triggers `doFreePort()`

**Acceptance:** With `state.ports` containing `[3000, 3001, 3002]`, `findNextFreePort(3000)` returns `3003`. With `state.ports = []`, returns `start`. With all 100 taken, returns `null`.

### Phase 2 — UI surface + verification

- [ ] New **Next** button rendered beside FREE (neutral accent, not red)
- [ ] Button click invokes `doSuggestFreePort()`
- [ ] Toast copy reads `Next free: 3002 · copied` on success, `No free port in 3000–3099` on exhaustion
- [ ] Screenshot matrix captured (dark + light; empty state + normal state)
- [ ] `cargo build --release` clean (sanity — no backend changes, should be instant)
- [ ] `node -e` JS parse clean
- [ ] Clipboard actually receives the suggested port (verified live)

**Acceptance:** Type `3000` with 3000 taken → click Next → toast shows next free port, clipboard holds the number.

---

## Test matrix

| # | Scenario | Expected | Result |
|---|---|---|---|
| 1 | Type `3000` with `3000` in use, `3001` free | Toast: `Next free: 3001 · copied`, clipboard=`3001` | — |
| 2 | Type `3000` with nothing in use | Toast: `Next free: 3000 · copied` | — |
| 3 | Type nothing, click Next | Uses default `3000` | — |
| 4 | Type non-numeric (`abc`) | Silent noop (same as existing FREE) | — |
| 5 | All 100 ports from start taken (synthetic) | Toast: `No free port in N–N+99`, no clipboard write | — |
| 6 | ⇧Enter triggers suggest, Enter still triggers FREE | Two keyboard paths, no conflict | — |
| 7 | Dark + light screenshot of new button | Consistent with existing toolbar | — |
| 8 | `cargo build --release` clean | 0 warnings | — |
| 9 | JS parse clean | No syntax errors | — |
| 10 | Existing FREE flow untouched | Regression check | — |

---

## Risk register

| # | Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| R1 | `state.ports` stale — port actually free in OS but shown as used | Low | Low | Next scan (auto-tick) corrects it; Maya retries. Not worth a live probe. |
| R2 | Suggested port gets claimed by another process between suggest + Maya's `npm run dev` | Medium | Low | Nature of the problem; clipboard→paste is faster than a human race. |
| R3 | ⇧Enter conflicting with browser/OS shortcut | Low | Low | Tested live in Phase 2 verification. |

---

## Execution

One session, two phases, one commit per phase. No batching needed (~40 LOC total).

## Progress tracker

| Phase | Status | Opened | Closed | Notes |
|---|---|---|---|---|
| 1 — Logic + handler | ⚪ Not started | — | — | — |
| 2 — UI + verification | ⚪ Not started | — | — | — |

Legend: ⚪ Not started · 🟠 Written · 🟡 Committed, unverified · 🟢 Verified

## Change log

- **2026-04-21** — Roadmap drafted. Branch `next-free-port` forked from `project-grouping@37ac3c6`. Pending user approval.
