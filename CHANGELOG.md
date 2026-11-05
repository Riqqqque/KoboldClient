# Changelog

## 0.2.0

- **See which addon is causing Lua errors.** Kobold reads the game's error log and groups errors by addon, including errors in the game's own UI that an addon set off ("tainted by"). The Installed list flags those addons; the errors window can update or disable them and copy the errors for a bug report.
- **Addon settings backups.** The `WTF` folder is backed up before addons update, whenever it changed since the last backup. **Settings → Addon settings backups** lists them, makes one on demand, and restores any of them. A restore backs up the current folder first and refuses to run while the game is open.
- **Update before Play.** Play installs pending addon updates first. Turn it off in Settings; it never waits more than 20 seconds for the update check.
- **Hand-installed addons are recognized.** Folders you copied in yourself (and zip imports) are fingerprinted and looked up on CurseForge. Exact matches become normal CurseForge addons that get updates.
- **Save and load your addon list.** Save every installed addon to a file, then install the same set on another PC. Addons installed by hand are listed with a link so you can fetch them yourself.
- **Wago.** Search and install Forever releases from Wago Addons with your Wago API key.
- **WoWInterface.** Search WoWInterface or paste an addon link. Downloads are checked against WoWInterface's MD5; addons that turn out not to be built for Forever are flagged.
- **GitHub token.** An optional token raises GitHub's limit from 60 to 5,000 requests an hour.

## 0.1.2

- **Required addons are installed too.** When a CurseForge addon lists another addon as required, Kobold installs that one first. Questie 12.6, for example, needs QuestieDB and QuestieTrace and showed `LibQuestieDB` Lua errors without them.
- Addons installed with an older version are checked once on launch, and missing required addons are added automatically.
- The Installed list marks an addon that's missing a required addon ("Needs …") and offers **Install missing**.
- Removing an addon that another one needs now says so first.

## 0.1.1

- **Play button.** Start WoW Forever from the sidebar. It launches through Battle.net so you stay signed in, falls back to `Wow.exe` if Battle.net isn't installed, and switches to "Playing" once the game is up.
- Undo after a delete now says so when the folders can't be restored (for example on drives without a Recycle Bin) instead of listing an addon whose files are gone.

## 0.1.0

First release.

- Finds your WoW Forever install automatically (running game, Battle.net data, common folders, then a quick drive search).
- Browse and install CurseForge addons that have a Forever build, with real addon icons.
- Add addons from GitHub releases (picks the Forever build from `release.json` or the file names).
- Import a downloaded `.zip`.
- Every download is checked against the source's published SHA-1 or SHA-256 before anything is installed.
- Installs are all-or-nothing: if anything fails, the previous version is put back. Interrupted installs are repaired on the next launch.
- Delete sends folders to the Recycle Bin, keeps libraries other addons still use, and offers Undo. Saved settings are only removed if you tick the box.
- Enable/disable without deleting, pin a version, update one or all.
- Four themes: Ember, Deepstone, Parchment and Fel.
- Daily log files, a detailed-logging switch and one-click diagnostics export.
- The app updates itself from GitHub releases.
