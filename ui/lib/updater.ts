import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";
import { api } from "./api";
import { app, toast } from "./store.svelte";

/** Looks for a newer Kobold Client release on GitHub. */
export async function checkAppUpdate(silent: boolean) {
  try {
    const update = await check();
    if (update) {
      app.appUpdate = update;
      api.log("info", `app update available: ${update.version}`);
    } else if (!silent) {
      toast("Kobold Client is up to date");
    }
  } catch (e) {
    api.log("warn", `app update check failed: ${String(e)}`);
    if (!silent) toast("Couldn't check for Kobold Client updates right now.", { kind: "error" });
  }
}

/** Downloads, verifies (signature) and installs the update, then restarts. */
export async function installAppUpdate() {
  const update = app.appUpdate;
  if (!update) return;
  try {
    toast(`Downloading Kobold Client ${update.version}…`);
    await update.downloadAndInstall();
    await relaunch();
  } catch (e) {
    api.log("error", `app update failed: ${String(e)}`);
    toast("The update couldn't be installed. Try again later.", { kind: "error" });
  }
}
