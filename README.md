# Mythic Raider Companion

Desktop sync app that brings **top-ranked raiders' talent builds** from
[Warcraft Logs](https://www.warcraftlogs.com/) into World of Warcraft — no copy-paste.

This is the source for the signed installer distributed at
[logsbyhltg.com](https://logsbyhltg.com). You can build it yourself, or just read
exactly what it does before running it.

## Why it exists

WoW addons are sandboxed — they can't reach the internet. So talent data has to get
into the game some other way. The companion is that bridge:

```
logsbyhltg.com          Mythic Raider Companion        Mythic Raider Talents
(crawls Warcraft Logs,  (this app: fetches data,        (in-game addon: raid > boss
 serves the talents)     writes Data.lua)                 > top 10, one-click import)
```

## What it does — and what it doesn't

The whole job is: download the talent data and write **one file**,
`World of Warcraft/_retail_/Interface/AddOns/MythicRaiderTalents/Data.lua`.

- Auto-detects your WoW install (Windows registry + common install paths); you can
  also pick the `_retail_` folder by hand.
- **Refuses to write** unless the download really is Mythic Raider talent data — it
  checks for the `MythicRaiderTalentsDB` signature first, so a server hiccup or error
  page can't overwrite a good file with garbage.
- Writes the addon's `Data.lua`, **not** SavedVariables — so you can sync while the
  game is open and just `/reload`, no restart.
- Touches **nothing** outside that one `Data.lua`: no account access, no credentials,
  no other addons, no other files.
- Auto-updates are cryptographically signed (minisign). An unsigned or tampered
  update is rejected.

You also need the **Mythic Raider Talents** addon installed in-game (from CurseForge).
The companion writes the data; the addon reads and imports it.

## Build from source

Prerequisites: [Rust](https://rustup.rs/), [Node.js](https://nodejs.org/), and the
[Tauri v2 system prerequisites](https://v2.tauri.app/start/prerequisites/).

```bash
npm install
npm run tauri dev      # run locally
npm run tauri build    # produce an installer
```

Frontend is vanilla HTML/CSS/JS in [`src/`](src/); the backend is Rust in
[`src-tauri/src/lib.rs`](src-tauri/src/lib.rs).

## Tech stack

- [Tauri 2](https://v2.tauri.app/) — Rust core + the system WebView
- `reqwest` (HTTP), `winreg` (install detection), `rfd` (native folder picker)
- Signed auto-updates via `tauri-plugin-updater`

## License

[MIT](LICENSE)
