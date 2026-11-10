# ADR-0020: Downloads from Nexus Mods, in Stages

## Status
Accepted (stage 1)

## Date
2026-11-09

## Context
Most Stardew Valley mods are published on Nexus Mods, so most updates the
check finds (ADR-0019) still need a manual download. Nexus's API can
download files for a user, but its
[API Acceptable Use Policy](https://help.nexusmods.com/article/114-api-acceptable-use-policy)
sets the terms:

- An app for a wider audience must be **registered** with Nexus, which
  issues it an application slug for sign-in. Nexus asks for a working
  testing build when the app applies, and can refuse.
- A user's **personal API key** is allowed only in an app being tested or
  for personal use, never in a public app.
- Every request must send `Application-Name` and `Application-Version`.
- Keys must not be kept on a server, and requests must be the user's own
  doing, not background scraping.

Downloading through the API is allowed for **Premium** members only. Anyone
else uses the "Mod manager download" button on the site, which opens an
`nxm://` link carrying a short-lived key for that one file. Vortex and
Mod Organizer 2 also handle `nxm://` links.

The registration needs a working build first, so the integration is built
in stages around it.

## Decision

### Stage 1: everything that needs no registration (this change)
1. **Pure parts in `manager_core::nexus`.** The `nxm://` link parser reads
   Stardew Valley links to one mod file only. It refuses other games,
   collections and anything malformed, and keeps the link's key out of its
   `Debug` output. `nexus_source` reads `Nexus:1234[@Subkey]` update keys.
   `choose_nexus_file` is as strict as `choose_release_asset`: it takes a
   main, update or optional ZIP whose version is exactly the suggested one,
   skips old, archived and source files, and picks the one main file of
   several. Otherwise it picks nothing.
2. **A port and client for Nexus's v1 API** (`NexusApiPort`,
   `manager_infra::nexus::NexusApi`). It covers `users/validate`, a mod's
   `files`, `download_link` (with an `nxm://` link's key when there is one)
   and `md5_search`.
   - Every request carries the application headers and the key in a
     sensitive `apikey` header.
   - Redirects are not followed.
   - 401 (key refused), 403 (Premium needed) and 429 (rate limit) are
     reported apart, and only Nexus's own https download hosts are
     accepted.
   - Nothing from an answer's body is kept beyond the id, name and Premium
     status (`users/validate` also returns the key and email).
3. **Downloads go through the existing pipeline** (`NexusService`), as
   GitHub ones do. The size must be the size Nexus lists, and the MD5 is
   looked up on Nexus:
   - A file Nexus knows as another file is refused.
   - A file Nexus does not recognise is offered unconfirmed, and the review
     says so.
   - An update is refused unless the archive holds the mod at the suggested
     version.

   The acquisition records `provider: "nexus"` with the mod and file ids,
   the version and whether the MD5 was confirmed. Activity and restore
   points read this back through the now provider-neutral
   `provenance::PackageOrigin`. Nexus downloads share GitHub's
   one-download-per-profile slot.
4. **`nxm://` links reach the open window through an inbox.** The manager
   allows one window per data folder (its presence lock), so a process
   started for a link writes the link to `incoming-links/` in the data
   folder and exits quietly. The window empties the inbox at start and on
   each tick of its outside-change watcher. It hands each link to the Nexus
   service, refreshes, and comes to the front. The single-instance plugin
   was not used: it would be a second, per-app-id idea of "one window" next
   to the per-data-folder lock. A waiting link's key stays in the backend;
   the window sees the mod, the file and which installed mod it updates.
5. **Registering for `nxm://` is the user's choice.** No installer or
   startup step does it. Settings turns it on: on Windows that writes
   `HKCU\Software\Classes\nxm`, and on Linux it writes a hidden desktop
   entry and runs `xdg-mime default`. The handler it replaced is remembered,
   and turning it off gives the links back. Links can also be pasted.
6. **A personal key only in a build for testing.** The `nexus-personal-key`
   cargo feature (`bun run desktop:dev:nexus`) lets a build save a personal
   key, checked with Nexus before it is saved, in the keychain like the
   GitHub token. It also enables the link handler. Without the feature,
   which is how every release is built:
   - Settings says Nexus is not connected yet.
   - The keychain is never read for it and nothing is sent to Nexus.
   - Updates keep "Open mod page".
7. **Manual downloads are matched.** Whatever the build, an archive chosen
   from this computer that is the same mod at the version the last check
   suggested is named in the review as that update.

### Stage 2: registration
Apply to Nexus with a stage 1 testing build, the public source, a name,
description and logo, and how the manager handles data (see
`docs/nexus-registration.md`). Confirm the sign-in flow they want for a new
application (OAuth 2.0 with PKCE, or the older SSO websocket) and the
redirect address.

### Stage 3: after approval
- Sign in through Nexus in place of the personal key, with tokens kept in
  the keychain and refreshed.
- Remove the `nexus-personal-key` feature.
- Turn on the Nexus download button and the link handler in release builds.
- Let Update all include Premium Nexus downloads.

Later, possibly: identifying mods the manager did not install by MD5, and
importing collections.

## Consequences
### Positive
- A Premium member updates a Nexus mod in one click, and anyone else with
  "Mod manager download", with the same review, replace-with-rollback and
  history as GitHub downloads.
- The policy's terms are kept: no personal key in a public build, the
  application headers on every request, only user-started requests, and
  nothing kept but on the user's own computer.
- The registration application can point at a build that works, and at the
  code.

### Negative
- Until registration, release builds gain nothing from Nexus except the
  manual-download match in the review.
- MD5 is weak as a checksum. It is used because it is what Nexus publishes,
  and only to confirm the file is the one Nexus lists; the stored package
  is still fingerprinted with SHA-256.
- Taking `nxm://` links from Vortex or MO2 means their links come here until
  the user turns it off (or the other app takes them back).
- The inbox is polled every 1.5 seconds, so a link can take that long to
  appear.

## Alternatives Considered
- **Shipping a personal key path in releases**: against the policy.
- **Registering `nxm://` from the installer** (Tauri's deep-link
  configuration does this): silent takeover from other mod managers.
- **`tauri-plugin-single-instance`**: a second instance-exclusion scheme
  next to the presence lock, and one keyed on the app rather than the data
  folder.
- **Sending the raw link to the window**: would put the link's key in the
  web view; keeping it in the backend costs nothing.
