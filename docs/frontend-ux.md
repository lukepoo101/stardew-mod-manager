# Stardew Mod Manager — Frontend Architecture and UX Guide

## 1. Desktop Shell Architecture

The user interface uses a persistent desktop shell layout providing continuous awareness of the active game, active profile, and system health.

```text
+----------------------------------------------------------------------------+
| Sidebar       | Game: Stardew Valley (Steam)       Profile: Main           |
|               | Health: Healthy                    Launch: [> Play]        |
| [Overview]    +------------------------------------------------------------+
| [Mods]        |                                                            |
| [Profiles]    |                                                            |
| [Diagnostics] |                      Current Route                         |
| [Activity]    |                                                            |
|               |                                                            |
| ------------- |                                                            |
| [Settings]    |                                                            |
+----------------------------------------------------------------------------+
```

### Core Components
- **Sidebar**: Primary navigation between Overview, Mods, Profiles, Diagnostics, Activity, and Settings.
- **Context Header**: Displays active game installation, active profile switcher, health badge, and the primary Launch control.
- **Main Viewport**: Renders the routed view component based on the active hash route.

---

## 2. Route Map

All routes are hash-based via React Router (`createHashRouter`):

| Route Path | View / Feature | Purpose |
| :--- | :--- | :--- |
| `/onboarding` | Onboarding Wizard | Guided onboarding (welcome, discover, modded alert, review, SMAPI install) |
| `/app/overview` | Playthrough Dashboard | High-level summary of active profile, mod count, health, last session, quick actions |
| `/app/mods` | Mod Inventory | Filterable, searchable data table of installed mods in the active profile |
| `/app/mods/:profileComponentId` | Mod Details Panel | Inspect version, author, provenance, manifest, and dependencies |
| `/app/mods/install/:operationId` | Install Review Route | Inspect staging preview, incoming versions, dependencies, and commit |
| `/app/profiles` | Profile Management | List profiles, switch active profile, create clean profile |
| `/app/profiles/:profileId` | Profile Configuration | Profile details, description, and settings |
| `/app/diagnostics` | Diagnostics View | Session history, verification results, and structured finding summaries |
| `/app/diagnostics/sessions/:sessionId` | Session Inspection | Detailed session log viewer with SMAPI log stream |
| `/app/activity` | Activity History | Audit log of durable operations, step progress, and semantic effects |
| `/app/activity/:operationId` | Operation Details | Step-by-step progress and recovery interface for a specific operation |
| `/app/settings/game` | Game Settings | Manage detected installations, add manual folders, view support state |
| `/app/settings/runtime` | Runtime Settings | Pinned SMAPI release info, observed SMAPI status, installation tools |
| `/app/settings/storage` | Storage Settings | Data directory, package store, cache, and database locations |
| `/app/settings/appearance` | Appearance Settings | System / Light / Dark theme configuration |
| `/app/settings/advanced` | Advanced Settings | Expert options, logs directory, and troubleshooting tools |

---

## 3. Server State & Cache Management

State owned by the Rust backend is managed using **TanStack Query**:
- **Query Keys**: Structured hierarchical keys (`["bootstrap"]`, `["games"]`, `["profiles", gameId]`, `["mods", profileId]`, `["operation", opId]`).
- **Targeted Invalidation**: Mutations only invalidate affected query keys (e.g. committing a mod install invalidates `["mods", profileId]` and `["operations"]`, without refetching game discovery).
- **Event Bridge**: Tauri backend emits lightweight events (`operation_changed`, `profile_changed`, `inventory_changed`, `session_changed`, `health_changed`). A central listener triggers cache invalidation automatically.

---

## 4. Design System & Accessibility

- **Theme Tokens**: Semantic CSS variables (`--bg-primary`, `--bg-surface`, `--accent-primary`, `--fg-primary`, `--fg-muted`, `--border`, etc.) support seamless System / Light / Dark transitions without hardcoded colors.
- **Icons**: Clean icon set powered by `lucide-react`. Semantic emojis are avoided for interactive controls.
- **Accessible Controls**: Modal dialogs, dropdowns, and form inputs utilize Radix UI primitives for keyboard navigation, ARIA labeling, and focus trapping.
- **Multi-channel Status**: Status indicators combine color, icons, and explicit text labels so status is never conveyed by color alone.

## Global search and keyboard use

`Ctrl+K` (`Cmd+K` on macOS) or the header Search button opens one search box for
pages, settings, profiles and installed mods. It is a combobox: the arrow keys,
Home and End move through results, Enter opens the highlighted one and Escape
closes; the active result is announced through `aria-activedescendant`. Ranking
is deterministic (best match, then kind, then title), every word must match, and
case and accents are ignored. Choosing a mod opens the Mods page with its
UniqueID as the search, carried as `?q=` on the hash route.

Lists of rows (the Mods list, the profile cards) are `role="list"` and use
`shared/a11y/rowNavigation.ts`: Up and Down move focus to the same control in
the previous or next row, Home and End to the first or last row. Tab still
walks every control in order, Space toggles the focused checkbox, and text
fields and selects keep their own arrow keys.

## 5. Empty, loading and failed states

A view never shows "nothing here" while its data is loading or after a load
failed. Features use the shared `EmptyState`, `Loading` and `LoadFailed`
components (`components/ui/EmptyState.tsx`) and own their copy and actions:

| View | Empty | Filtered to nothing | Failed |
| --- | --- | --- | --- |
| Mods | "No mods in this profile yet", with Choose mod ZIP (local, no account) | names the search or filter, with Show all mods | Could not load, with Try again |
| Activity | "Nothing has happened yet", with Install a mod | n/a | Could not load, with Try again |
| Diagnostics / Overview health | "No known issues" only once a report exists | the filter bar counts what is shown | "Not checked" rather than "No known issues" |

The primary action comes straight after the title in tab order. Settings >
Interface > "Show guidance text" hides the explanations for experienced users;
actions are never hidden.

## 6. Organising mods

The Mods page can be sorted (name, author, version, recently installed,
favourites first, enabled first) in either direction, with ties always broken
by name and then UniqueID so the order is stable; versions compare by their
numeric parts. Sort and direction are UI preferences and compose with search
and the filters (enabled state, tag, favourites only, needs attention). A mod
can be starred as a favourite and given tags and a note from its details panel.
Favourites, tags and notes are stored in the preferences table under
`mod_annotations`, keyed by UniqueID case-insensitively, so they follow the mod
into every profile. They are presentation data only: nothing that installs,
enables or launches reads them.

"Show mod folder" and "Show original archive" open the system file manager. The
backend resolves the path from the mod's records (its deployment folder, live or
disabled, and its retained package); the frontend never sends a path.


Filters on the Mods page combine with AND: search, Enabled/Disabled, Tag,
Favourites only and **Needs attention** must all match. Needs attention shows
mods with a required dependency or content-pack host that is missing, disabled
or too old (from the backend's dependency relations, `list_mod_problems`) or
that SMAPI skipped in the last session; those rows carry a *Missing
requirement* or *Skipped last session* badge. **Show all mods** clears every
filter.

## 7. Dialogs and keyboard use

Every modal dialog uses `components/ui/Modal.tsx`, built on Radix Dialog: focus
moves into the dialog when it opens, Tab and Shift+Tab stay inside it, and
focus returns to the control that opened it. Escape (and a click outside) does
the same as the dialog's Cancel, for example cancelling a prepared install or
removal, and does nothing while that dialog's work is running. Buttons are laid
out Cancel first, so the first focused control is never the destructive one.

## 8. Narrow windows, scaling and touch

- The window's minimum size is 800 × 600, and Settings offers text and control
  sizes from 90% to 150%.
- Below the `lg` breakpoint (1024 CSS pixels, which a large UI scale or a
  handheld reaches), the sidebar shows icons only. Every link keeps its name
  as its accessible name and tooltip, and a bulk job in progress shows as a
  compact count.
- On coarse pointers (touch screens, handhelds), buttons, fields, selects and
  disclosure summaries are at least 44 px tall, and checkboxes and radios at
  least 24 px.
- Lists wrap rather than scroll sideways, and dialogs scroll inside themselves.
