import { get, writable } from "svelte/store";
import { check } from "@tauri-apps/plugin-updater";

/**
 * In-app updates from the GitHub release `latest.json` (Tauri updater, signed installers), Discord
 * style (user, 2026-10-03): a found update waits as a corner button, one click installs it, and the
 * notes show after the restart (WhatsNew). Development builds never offer an install.
 */
const installable = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window &&
  !(import.meta.env?.DEV && import.meta.env?.MODE !== "e2e");

export const updateState = writable({
  status: installable ? "idle" : "unavailable",
  version: null,
  currentVersion: null,
  error: null,
  progress: null,
  /** A saved run must be acknowledged before updating. */
  confirming: false,
});

/** Set by the Forge view: an active run blocks the update; a saved run cannot resume after it. */
export const updateHold = writable({ forgeBusy: false, savedRun: false });

let pending = null;
const busy = (status) => ["checking", "downloading", "installing"].includes(status);

/**
 * `quiet` is the background check: it never shows an error, and an update on offer stays until a
 * newer one replaces it, so a program left open in the tray never offers a stale version.
 */
export async function checkForUpdate({ quiet = true } = {}) {
  if (!installable) return;
  const { status } = get(updateState);
  if (busy(status)) return;
  const offered = status === "available";
  if (!(quiet && offered)) updateState.update((state) => ({ ...state, status: "checking", error: null }));
  let found;
  try {
    found = await check();
  } catch (error) {
    updateState.update((state) => offered
      ? { ...state, status: "available" }
      : { ...state, status: quiet ? "idle" : "error", error: String(error?.message ?? error) });
    return;
  }
  if (found && pending?.version === found.version) {
    await found.close().catch(() => {});
    updateState.update((state) => ({ ...state, status: "available" }));
    return;
  }
  await pending?.close().catch(() => {});
  pending = found;
  updateState.update((state) => found
    ? { ...state, status: "available", version: found.version, currentVersion: found.currentVersion, error: null }
    : { ...state, status: "current" });
}

/** The corner button and Settings: an active run blocks it, a saved run asks first. */
export function requestUpdate() {
  const { forgeBusy, savedRun } = get(updateHold);
  if (forgeBusy || get(updateState).status !== "available") return;
  if (savedRun) updateState.update((state) => ({ ...state, confirming: true }));
  else installUpdate();
}

export function cancelUpdate() {
  updateState.update((state) => ({ ...state, confirming: false }));
}

/** On Windows the app exits when the installer starts; the installer reopens it when done. */
export async function installUpdate() {
  if (!pending) return;
  let total = 0;
  let received = 0;
  updateState.update((state) => ({ ...state, status: "downloading", progress: 0, error: null, confirming: false }));
  try {
    await pending.downloadAndInstall((event) => {
      if (event.event === "Started") total = event.data.contentLength ?? 0;
      if (event.event === "Progress") {
        received += event.data.chunkLength;
        const progress = total ? Math.min(100, Math.round((received / total) * 100)) : null;
        updateState.update((state) => ({ ...state, progress }));
      }
      if (event.event === "Finished") updateState.update((state) => ({ ...state, status: "installing" }));
    });
  } catch (error) {
    updateState.update((state) => ({ ...state, status: "available", error: String(error?.message ?? error) }));
  }
}
