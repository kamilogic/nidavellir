import { readable } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// Whether the window is on screen. The program's Rust side reports hiding to the tray, minimizing
// and restoring; the page's own visibility covers the rest (and test or dev browsers).
export const windowVisible = readable(true, (set) => {
  let shown = true;
  const update = () => set(shown && document.visibilityState === "visible");
  invoke("window_visible").then((visible) => { shown = visible; update(); }).catch(() => {});
  const stop = listen("window-visibility", (event) => { shown = event.payload; update(); }).catch(() => () => {});
  document.addEventListener("visibilitychange", update);
  return () => {
    document.removeEventListener("visibilitychange", update);
    stop.then((unlisten) => unlisten());
  };
});
