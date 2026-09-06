// Help ▸ New Version — asks GitHub what the newest release is and, if it is
// newer than this build, fetches its installer and hands it to the system.
//
// Not the Tauri updater plugin: that wants every release signed with a key pair
// this project does not have and an update feed it does not publish. The
// releases API is public and already describes exactly what the workflow
// uploaded, so the check is a plain fetch.
//
// The check only ever runs when the menu item is chosen. Nothing phones home on
// startup.

import { invoke } from "@tauri-apps/api/core";
import { ask, message } from "@tauri-apps/plugin-dialog";
import { t } from "./i18n.js";

const LATEST_RELEASE = "https://api.github.com/repos/nebosa-company/justcode/releases/latest";

// The installer to look for, per platform, best first. Windows gets the NSIS
// setup rather than the .msi because that is the one the workflow publishes as
// a self-contained installer; macOS gets the disk image; Linux prefers the
// AppImage, which runs without a package manager.
const INSTALLERS = [
  [/windows|win32|win64/i, [/-setup\.exe$/i, /\.exe$/i, /\.msi$/i]],
  [/mac|darwin|iphone/i, [/\.dmg$/i]],
  [/linux|x11/i, [/\.AppImage$/i, /\.deb$/i]],
];

/**
 * Whether `latest` is a later version than `current`, comparing the numbers
 * rather than the text: "0.2.10" is newer than "0.2.9", which a string compare
 * gets backwards. A part that is not a number sorts as 0, so a `-beta` suffix
 * never reads as an upgrade.
 */
export function isNewer(latest, current) {
  const parts = (version) => String(version).replace(/^v/, "").split(/[.\-+]/).map(Number);
  const a = parts(latest);
  const b = parts(current);
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    const left = Number.isFinite(a[i]) ? a[i] : 0;
    const right = Number.isFinite(b[i]) ? b[i] : 0;
    if (left !== right) return left > right;
  }
  return false;
}

/** The asset this platform can install, or undefined if the release has none. */
export function installerFor(assets, userAgent) {
  const patterns = INSTALLERS.find(([platform]) => platform.test(userAgent))?.[1];
  if (!patterns) return undefined;
  for (const pattern of patterns) {
    const found = assets.find((asset) => pattern.test(asset.name || ""));
    if (found) return found;
  }
  return undefined;
}

/**
 * The whole flow, from the menu item to the running installer. `flash` writes a
 * line in the status bar and `quit` closes the editor — both belong to the main
 * module, which is what keeps this file free of the app's own plumbing.
 */
export async function checkForNewVersion(currentVersion, { flash, quit }) {
  const title = t("help.newVersion");
  let release;
  try {
    const response = await fetch(LATEST_RELEASE, { headers: { Accept: "application/vnd.github+json" } });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    release = await response.json();
  } catch (error) {
    await message(t("update.failed", { error }), { title, kind: "error" });
    return;
  }

  const latest = String(release.tag_name || "").replace(/^v/, "");
  if (!isNewer(latest, currentVersion)) {
    await message(t("update.upToDate", { version: currentVersion }), { title });
    return;
  }

  const installer = installerFor(release.assets || [], navigator.userAgent);
  if (!installer) {
    await message(t("update.noAsset", { version: latest }), { title, kind: "warning" });
    return;
  }

  const download = await ask(t("update.available", { latest, current: currentVersion }), {
    title,
    okLabel: t("update.download"),
    cancelLabel: t("dialog.cancel"),
  });
  if (!download) return;

  let path;
  try {
    flash(t("update.downloading", { name: installer.name }));
    path = await invoke("download_update", {
      url: installer.browser_download_url,
      fileName: installer.name,
    });
  } catch (error) {
    await message(t("update.failed", { error }), { title, kind: "error" });
    return;
  }

  // "Install" hands the file to the system and leaves: on Windows the setup
  // cannot replace a running JustCode, and on the other two the installer is
  // not JustCode's to drive. Declining still leaves the download in place, so
  // the other button opens the folder rather than throwing the work away.
  const install = await ask(t("update.installQuestion", { name: installer.name }), {
    title,
    okLabel: t("update.install"),
    cancelLabel: t("update.reveal"),
  });
  try {
    await invoke(install ? "install_update" : "reveal_in_file_manager", { path });
  } catch (error) {
    await message(t("update.failed", { error }), { title, kind: "error" });
    return;
  }
  if (install) await quit();
}
