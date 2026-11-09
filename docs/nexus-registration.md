# Registering with Nexus Mods

Stage 2 of [ADR-0020](adr/0020-nexus-mods-downloads.md): what to send Nexus
Mods when applying to register the manager as an application, and what to
ask them. Nexus wants a working testing build with the application, which
stage 1 provides.

## Before applying

- [ ] Build the testing build: `bun run desktop:dev:nexus`, or
      `bun run tauri build --features nexus-personal-key` for an installer.
      Never publish that installer: a personal key is for testing and
      personal use only.
- [ ] Try it with your own key:
  - [ ] Connect in Settings and check the account name and Premium status.
  - [ ] Download an update (needs Premium).
  - [ ] Turn on "Open Mod manager download links" and use the button on a
        mod's Files tab.
  - [ ] Paste an `nxm://` link.
  - [ ] Check that Activity and restore points name the Nexus file.
- [ ] Watch the request count. The manager asks Nexus only when the user
      acts. One download is `files`, `download_link` and `md5_search`, three
      requests; Connect and Test are one each.

## What to send

Send these through Nexus Mods support:

| Item | Content |
| --- | --- |
| Name | Stardew Mod Manager (sent as `Application-Name: StardewModManager`) |
| Short description | An open-source mod manager for Stardew Valley on Windows and Linux: guided SMAPI setup, isolated per-profile Mods folders, reviewed installs with rollback, and update checks through smapi.io. |
| Logo | `apps/desktop/src-tauri/icons/icon.png` |
| Source | https://github.com/lukepoo101/stardew-mod-manager (MIT) |
| Testing build | A `nexus-personal-key` build (link, or attach) |
| Version header | `Application-Version`, the manager's version (`0.1.0` now) |

### How the manager handles data

- The user's credential is kept in the operating system's keychain only:
  Windows Credential Manager, or the Secret Service on Linux. It is never
  stored in the manager's files, logs, support bundles or error details,
  and is never sent anywhere but `api.nexusmods.com`.
- The manager has no server. Nothing is sent to its developers.
- Requests are made only when the user acts: checking an account,
  downloading an update, or downloading a file chosen with "Mod manager
  download". There is no background polling or scraping. Update checks use
  smapi.io, not the Nexus API.
- From `users/validate` it reads only the user id, name and Premium status.
- A downloaded file is checked against the size Nexus lists and looked up by
  MD5 to confirm it is the file asked for, then reviewed by the user before
  anything is installed.
- `nxm://` links are taken only when the user turns that on, and turning it
  off gives them back to the previous handler.

## What to ask

- Which sign-in flow a new application should use: OAuth 2.0 with PKCE (and
  its client id and allowed redirect addresses; a loopback address such as
  `http://127.0.0.1:<port>/callback` suits a desktop app), or the SSO
  websocket and its application slug.
- The current rate limits, and the headers that report them (the manager
  reads `x-rl-hourly-remaining`, `x-rl-daily-remaining` and
  `x-rl-hourly-reset`).
- How long an `nxm://` link's key lasts.
- Whether the v1 endpoints used here (`users/validate`, `files`,
  `download_link`, `md5_search`) or the v2 GraphQL API should be used for a
  new application.

## After approval (stage 3)

See ADR-0020, stage 3.
