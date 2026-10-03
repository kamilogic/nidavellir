import { get, writable } from "svelte/store";
import { check } from "@tauri-apps/plugin-updater";

/**
 * In-app updates from the GitHub release `latest.json` (Tauri updater, signed installers).
 * Development builds never offer an install: `tauri dev` is not the installed app.
 */
const installable = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window &&
  !(import.meta.env?.DEV && import.meta.env?.MODE !== "e2e");

export const updateState = writable({
  status: installable ? "idle" : "unavailable",
  version: null,
  currentVersion: null,
  notes: "",
  date: null,
  error: null,
  progress: null,
  dismissed: false,
});

let pending = null;

/** `quiet` keeps the automatic startup check silent when GitHub cannot be reached. */
export async function checkForUpdate({ quiet = true } = {}) {
  if (!installable) return;
  // A known update stays on offer; a background check never interrupts a download.
  const { status } = get(updateState);
  if (["checking", "downloading", "installing"].includes(status) || (quiet && status === "available")) return;
  updateState.update((state) => ({ ...state, status: "checking", error: null }));
  try {
    const found = await check();
    await pending?.close().catch(() => {});
    pending = found;
    updateState.update((state) => found
      ? { ...state, status: "available", version: found.version, currentVersion: found.currentVersion, notes: found.body ?? "", date: found.date ?? null }
      : { ...state, status: "current" });
  } catch (error) {
    updateState.update((state) => ({ ...state, status: quiet ? "idle" : "error", error: String(error?.message ?? error) }));
  }
}

export function reviewUpdate() {
  updateState.update((state) => ({ ...state, dismissed: false }));
}

export function dismissUpdate() {
  updateState.update((state) => ({ ...state, dismissed: true }));
}

/** On Windows the app exits when the installer starts; the installer reopens it when done. */
export async function installUpdate() {
  if (!pending) return;
  let total = 0;
  let received = 0;
  updateState.update((state) => ({ ...state, status: "downloading", progress: 0, error: null }));
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
