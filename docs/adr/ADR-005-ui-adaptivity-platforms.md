# ADR-005: UI adaptivity and target platforms

**Status:** Accepted — 2026-07-04 (v1.0 implements only Compact; the seams now, the
Medium/Expanded layouts and the desktop — additively later)
**Date:** 2026-07-04
**Decided by:** project owner
**Related:** [app-ui.md](../design/app-ui.md) (section "Adaptivity" — the source of truth for
layouts, Russian), [call-ui.md](../design/call-ui.md) (the call grid),
[ADR-003](ADR-003-engine-api.md) — the engine ↔ client boundary (the foundation that makes
this decision possible at all).

## Context (the problem)

The question came from discussing the participant grid in a video call and grew into a
general one: **how do we build the UI now so that we do not rewrite everything when
tablets, foldables, landscape and the desktop arrive?** Two traps are easy to fall into:

1. **"A layout per device."** Phone portrait, tablet, desktop as separate screens. There are
   endlessly many devices (foldables, split screen, a desktop window at 40 % width, phone
   landscape) — a set of layouts for them does not scale and drifts away from the logic.
2. **Navigation as a stack of screens.** When "open a chat" = `navController.push`, a "list on
   the left + chat on the right" mode (tablet) requires rewriting navigation rather than
   re-laying out the same components.

A constraint and an asset of the environment: **the engine ↔ UI boundary is already
decoupled** (ADR-003). All business logic is in the Rust engine; the UI is a thin projection
of a state snapshot ([invariants](../architecture/invariants.md) §5). That is the foundation:
a new platform or layout rewrites **only the presentation**, not the logic.

## Decision

### Platforms (priority)

| Platform | Verdict |
|---|---|
| **Android phone (Compact)** | v1.0. The only one fully implemented before release. |
| **Android tablet / foldable / landscape** | Comes almost for free if the seams below are kept. Implementation — **after** 1.0. |
| **Desktop (Linux/Windows/macOS)** | Realistic and valuable (on the desktop the transport is even simpler — no background limits). A `desktop` crate (Slint) exists, but it predates the engine and does not currently build; the path is a desktop client on the same engine. |
| **iOS** | **Not a goal of v1.0 and probably never in the current form.** iOS kills a persistent background mesh node (background execution limits), plus App Store friction with P2P. If ever — only a "light client" to someone else's node, which is a different architecture. We **do not design for iOS now**. |
| **Web** | Impossible as a real mesh node (no raw sockets in a browser). At most a companion to a local node. Skipped. |

### Layouts by size class, not by device

We use **window width breakpoints** (Compose `WindowSizeClass`), not devices:

- **Compact** (<600dp): phone portrait. One pane.
- **Medium** (600–840dp): tablet portrait, foldable, phone landscape. A nav rail on the left
  + one content pane. (Added to app-ui.md as a cheap middle — otherwise the jump
  Compact → Expanded is abrupt.)
- **Expanded** (>840dp): tablet landscape, desktop. Three columns (rail → list → details).

**"How many layouts to prepare":** not N layouts per device, but **one set of size-agnostic
content composables + three shells** that lay them out.

## Implementation layers (what to do in Compose)

Two strict tiers of composables are the core of the decision:

**a) Content composables** (`ChatMessageList`, `CallGrid`, `ContactRow`, `MessageBubble`). They
know nothing about navigation, panes or screen size. They take data + callbacks and are reused
verbatim in all size classes (conceptually — the same widgets on the desktop too).

**b) Scaffolds / shells** (`NavigationSuiteScaffold` / `ListDetailPaneScaffold`). **The only
place where `WindowSizeClass` is read.** They decide how many panes there are and where the
navigation goes, and put the content composables from (a) inside.

Two rules without which (a)/(b) do not work:

- **Navigation is data, not a stack of screens.** `selectedChatId` lives in state. In Compact
  "a chat is selected" = a pushed destination; in Expanded the same `selectedChatId` = the
  right pane. One state renders both modes without branches.
- **All sizing through `Tokens.*`** (CLAUDE.md invariant #5) — density, scale and font are one
  lever for all size classes.

**The call grid is an algorithm, not a set of layouts.** `CallGrid` computes rows×cols from
(the number of participants + the container's aspect ratio + the available size). One
composable for everything: portrait (a stack / two in a row), landscape (wider), tablet (more
tiles), a mini overlay window (one active speaker). No `if tablet`. That is the answer to the
original question about the grid.

## Options considered

1. **Separate screens for phone / tablet / desktop.** Rejected — does not scale to the real
   zoo of sizes, duplicates and desynchronises the presentation, breaks on split screen and
   foldables.
2. **Compact only, adaptivity "some day, from scratch".** Rejected — "from scratch later" in
   practice = rewriting navigation and every screen; exactly the cost this ADR avoids.
3. **A full three-pane tablet UI right now.** Rejected as over-investment — it does not move
   the v1.0 deliverable (an Android phone) and takes time away from it.
4. **Seams now, only Compact implemented (chosen).** Content composables are written
   size-agnostic, there is one adaptive shell with the Compact branch implemented, and
   navigation is modelled as data. Medium/Expanded/desktop are additive work later.

## The trade-off

- **We pay now** with discipline: content/shell separation, navigation as data, strict
  `Tokens.*`. A little more structure than "just draw the screen".
- **We get later** tablet / foldable / landscape / desktop as *additive* work (a new branch
  in one shell), not a rewrite. iOS and Web are deliberately out of scope — we do not spend
  design on what is architecturally alien.

## Consequences

- **v1.0:** **only Compact** is implemented, fully. The seams are laid, but the
  Expanded/Medium branches of the shell are NOT written before release.
- The component boundary of the presentation is fixed: content composables do not import
  navigation or `WindowSizeClass`; size is read only in the shell.
- app-ui.md gains the **Medium** class (it had Compact + Expanded).
- call-ui.md gains the invariant "the grid is an algorithm of space and count".

## Action items

- [x] app-ui.md: add **Medium** to the "Adaptivity" section (nav rail + one pane). *(2026-07-04)*
- [x] call-ui.md: record the grid invariant (`CallGrid` computes rows×cols from participants +
      aspect ratio + size; no per-device layouts). *(2026-07-04)*
- [x] Compose: the shell seam — `WidthClass` is read only in `MainActivity`, `Root` dispatches
      by width, the body → `CompactShell`. Medium/Expanded are TODO branches. *(2026-07-04)*
- [ ] Compose: while refining screens, keep the two tiers (content / shell); read size only in
      the shell. Check on review of new screens.
- [x] Compose: navigation is already "selection = data" (`screen`/`activeFilter` in state,
      `when(screen)` — no `NavController` stack). Hoisting the selection into a shared holder —
      when an ExpandedShell appears (the second consumer). *(checked 2026-07-04)*
- [ ] After v1.0: implement the Medium/Expanded branches of the shell (additively).
