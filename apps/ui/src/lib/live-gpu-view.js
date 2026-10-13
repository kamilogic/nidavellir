import { writable } from "svelte/store";

// How the Live GPU section draws: "monitor" (strip charts, the default) or "gauges" (Settings).
const KEY = "nidavellir-live-gpu-view";

function initialView() {
  try {
    return localStorage.getItem(KEY) === "gauges" ? "gauges" : "monitor";
  } catch {
    return "monitor";
  }
}

export const liveGpuView = writable(initialView());
liveGpuView.subscribe((value) => {
  try {
    localStorage.setItem(KEY, value);
  } catch {}
});
