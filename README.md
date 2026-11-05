<p align="center">
  <img src="assets/logo.svg" width="96" alt="">
</p>

<h1 align="center">Kobold Client</h1>

<p align="center">A small, fast addon manager for <b>World of Warcraft: Forever</b>.<br>
Find addons, install them, keep them updated, and remove them cleanly.</p>

---

## Features

- **Finds your game on its own.** Kobold looks at the running game, Battle.net's install data and the usual folders, so most people never have to pick a folder.
- **Forever builds only.** Browse shows only addons that publish a Forever build. Anything installed by hand that isn't built for Forever gets a clear label.
- **CurseForge, Wago, WoWInterface, GitHub and zip files.** Search CurseForge and Wago for Forever builds, search WoWInterface or paste a link, add a GitHub repository, or import a `.zip` you downloaded.
- **Verified downloads.** Downloads are checked against the checksum the source publishes (SHA-1 or MD5 from CurseForge, MD5 from WoWInterface, SHA-256 from GitHub). A file that doesn't match is thrown away and nothing changes. Wago doesn't publish checksums, so Wago downloads rely on HTTPS.
- **Shows which addon broke.** Kobold reads the game's error log and lists Lua errors by the addon that caused them, with buttons to update or disable it and to copy the errors for a bug report.
- **Settings backups.** Before addons update, Kobold zips the `WTF` folder (addon settings, keybinds, macros) if it changed since the last backup. Restore any backup from Settings; the folder it replaces is backed up first.
- **Recognizes addons you installed by hand.** If CurseForge knows an addon you copied in yourself, Kobold links it so it gets updates like the rest. Nothing on disk changes.
- **Take your addons anywhere.** Save your addon list to a file and install the same set on another PC, or share it.
- **Dependencies handled.** If an addon needs another addon to work, Kobold installs both and warns you before you remove one that's still needed.
- **Installs that can't half-fail.** New files are staged next to `AddOns` and swapped in at once. If anything goes wrong, the old version is put back. If the PC crashes mid-install, the next launch repairs it.
- **Safe delete.** Removed addons go to the Recycle Bin, with an Undo button. Libraries that another addon still uses are kept. Saved settings stay unless you choose to delete them.
- **Play button.** Start the game from Kobold. It installs pending addon updates first (you can turn that off), then starts the game through Battle.net so you stay signed in.
- **Disable without deleting**, pin a version, update one addon or all of them.
- **Four themes:** Ember, Deepstone, Parchment and Fel.
- **Updates itself.** New versions of Kobold are offered in the sidebar and install with one click.
- **Light.** A 3 MB download, about 11 MB installed. It uses the WebView2 runtime that ships with Windows and runs nothing in the background.

## Install

1. Download `Kobold.Client_x.y.z_x64-setup.exe` from the [latest release](https://github.com/Riqqqque/KoboldClient/releases/latest).
2. Run it. Windows may show "Windows protected your PC" because the installer isn't code-signed. Click **More info**, then **Run anyway**.
3. Open Kobold Client. It finds WoW Forever and you're ready.

## Connecting CurseForge

CurseForge asks every app to use an API key. Kobold walks you through it in **Browse** or **Settings → CurseForge**:

1. Open the [CurseForge for Studios console](https://console.curseforge.com/) and sign in.
2. Open **API keys** and copy your key. If none is offered, use the request form linked from CurseForge's [getting-started page](https://docs.curseforge.com/rest-api/#getting-started).
3. Paste it into Kobold and press **Connect**.

The key is checked with CurseForge, then stored encrypted for your Windows account. WoWInterface, GitHub and zip installs work without it.

Some authors only allow downloads from the CurseForge website. For those, Kobold opens the addon page. Download the zip there and use **Import .zip**.

## Other sources

- **Wago.** Wago's API needs a personal key, which Wago gives to its subscribers ([addons.wago.io/patreon](https://addons.wago.io/patreon)). Paste it in **Settings → Wago** or the Wago tab in Browse.
- **WoWInterface.** No key needed. WoWInterface doesn't record which addons support Forever, so search shows everything that matches and addons with "Forever" in the name come first. After installing, Kobold labels any addon that isn't built for Forever.
- **GitHub.** Works without an account, but GitHub allows only 60 requests an hour that way. A [fine-grained token](https://github.com/settings/personal-access-tokens/new) with no permissions raises it to 5,000; paste it in **Settings → GitHub**.

Keys and tokens are stored encrypted for your Windows account and only sent to the service they belong to.

## Troubleshooting

- Logs: `%LOCALAPPDATA%\com.rique.koboldclient\logs` (one file per day, 14 days kept). **Settings → Troubleshooting** has a button to open the folder.
- Turn on **Detailed logging** before reproducing a problem.
- **Export diagnostics** saves a zip with logs, settings and the addon list (no API key, Windows user name masked) to your Downloads folder. Attach it to an [issue](https://github.com/Riqqqque/KoboldClient/issues).
- Kobold's data (addon list, settings) lives in `%APPDATA%\com.rique.koboldclient`. Settings backups are in `%LOCALAPPDATA%\com.rique.koboldclient\backups`. Its work folders are `Interface\.kobold-*` inside the game folder, and the game ignores them.
- **Addon errors** come from `_forever_\Logs\General.log`, which the game starts fresh every launch. Kobold reads it on launch and whenever the game starts or stops.

## Building from source

Requirements: Windows 10/11, [Rust](https://rustup.rs) (stable), [Node.js](https://nodejs.org) 20+, and the Visual Studio C++ Build Tools.

```bash
npm install
npm run tauri dev      # run with hot reload
npm run tauri build    # installer in src-tauri/target/release/bundle/nsis
```

`npm run dev` on its own serves the UI in a normal browser with sample data, which is handy for styling work.

Tests:

```bash
cd src-tauri
cargo test
```

### Layout

```
src-tauri/src/
  service.rs     high-level operations the UI calls
  commands.rs    Tauri command bindings
  game.rs        finding the Forever install, running-game check
  toc.rs         .toc parsing and Forever compatibility
  install.rs     extract, normalize, swap in, rollback, crash recovery
  remove.rs      Recycle Bin delete, undo, enable/disable
  manifest.rs    installed-addon list with rolling backups
  backup.rs      WTF folder backups and restore
  luaerrors.rs   reading Lua errors from the game log
  fingerprint.rs CurseForge fingerprints for hand-installed addons
  addonlist.rs   saving and loading addon lists
  net.rs         HTTP allow-list, retries, verified downloads
  sources/       CurseForge, Wago, WoWInterface and GitHub
ui/              Svelte interface (views, components, themes)
scripts/         release and housekeeping helpers
docs/            development plan and the original mock-ups
```

### Releasing

Add a section to `CHANGELOG.md`, then run:

```bash
./scripts/release.ps1 -Version 0.2.0
```

This bumps the version, tags it and pushes. GitHub Actions builds the signed update bundle and publishes the release, and installed copies pick it up on their next launch.

## License

[MIT](LICENSE) © 2026 Rique
