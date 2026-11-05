# Changelog

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
