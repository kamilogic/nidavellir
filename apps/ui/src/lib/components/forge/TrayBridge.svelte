<script>
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { Info, Power } from "@lucide/svelte";

  // Events from the program's Rust side: Exit during a Forge run, and a tray action that failed.
  let dialog = $state(null);
  let stopping = $state(false);
  let exitError = $state(null);
  let notice = $state(null);
  let noticeTimer;

  function askExit() {
    exitError = null;
    stopping = false;
    if (dialog && !dialog.open) dialog.showModal();
  }

  function showNotice(message) {
    notice = String(message);
    clearTimeout(noticeTimer);
    noticeTimer = setTimeout(() => (notice = null), 10_000);
  }

  async function stopAndExit() {
    stopping = true;
    exitError = null;
    try {
      await invoke("exit_program");
    } catch (error) {
      exitError = String(error);
      stopping = false;
    }
  }

  function keepRunning(event) {
    event?.preventDefault();
    if (!stopping) dialog?.close();
  }

  onMount(() => {
    const stops = [];
    const handlers = { "exit-requested": askExit, "tray-notice": (event) => showNotice(event.payload) };
    for (const [name, handler] of Object.entries(handlers)) {
      // Outside the desktop app there is no event bridge; nothing to listen to.
      listen(name, handler).then((stop) => stops.push(stop)).catch(() => {});
    }
    return () => {
      clearTimeout(noticeTimer);
      for (const stop of stops) stop().catch(() => {});
    };
  });
</script>

<dialog class="exit-dialog" bind:this={dialog} oncancel={keepRunning} aria-labelledby="exit-title">
  <header>
    <span class="exit-icon"><Power size={22} strokeWidth={1.8} /></span>
    <div>
      <h2 id="exit-title">Exit Nidavellir?</h2>
      <p>A Forge run is in progress.</p>
    </div>
  </header>
  <p class="exit-body">
    Exiting stops the run and returns the GPU to stock. The run stays saved: you can continue it from the
    Forge screen the next time you open Nidavellir.
  </p>
  {#if exitError}
    <p class="exit-error" role="alert">{exitError}</p>
  {/if}
  <footer>
    <button class="exit-keep" type="button" onclick={keepRunning} disabled={stopping}>Keep running</button>
    <button class="exit-stop" type="button" onclick={stopAndExit} disabled={stopping}>
      {stopping ? "Stopping the run…" : "Stop run and exit"}
    </button>
  </footer>
</dialog>

{#if notice}
  <div class="tray-notice" role="alert">
    <Info size={17} strokeWidth={2} />
    <span>{notice}</span>
    <button type="button" aria-label="Dismiss" onclick={() => (notice = null)}>×</button>
  </div>
{/if}

<style>
  .exit-dialog {
    width: min(480px, calc(100vw - 32px));
    border: 1px solid rgba(214, 160, 76, 0.35);
    border-radius: 14px;
    padding: 22px;
    background: #12171d;
    color: #e3e3df;
    box-shadow: 0 24px 60px rgba(0, 0, 0, 0.55);
  }

  .exit-dialog::backdrop {
    background: rgba(5, 8, 11, 0.72);
  }

  header {
    display: flex;
    gap: 14px;
    align-items: flex-start;
  }

  .exit-icon {
    display: grid;
    flex: 0 0 auto;
    width: 42px;
    height: 42px;
    place-items: center;
    border-radius: 10px;
    background: rgba(214, 160, 76, 0.14);
    color: #d6a04c;
  }

  h2 {
    margin: 0;
    font-size: 1.25rem;
    font-weight: 650;
  }

  header p {
    margin: 4px 0 0;
    color: #a2a9ac;
    font-size: 0.9rem;
  }

  .exit-body {
    margin: 16px 0 0;
    color: #d7d9d7;
    font-size: 0.9rem;
    line-height: 1.55;
  }

  .exit-error {
    margin: 14px 0 0;
    border-radius: 9px;
    padding: 9px 12px;
    background: rgba(191, 97, 106, 0.1);
    box-shadow: inset 0 0 0 1px rgba(191, 97, 106, 0.35);
    color: #f0b8bc;
    font-size: 0.82rem;
    line-height: 1.45;
  }

  footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 20px;
  }

  footer button {
    min-height: 40px;
    border: 0;
    border-radius: 9px;
    padding: 0 16px;
    font: inherit;
    font-weight: 650;
    cursor: pointer;
  }

  .exit-keep {
    background: rgba(255, 255, 255, 0.07);
    color: #d7d9d7;
  }

  .exit-stop {
    background: #d6a04c;
    color: #14110b;
  }

  footer button:disabled {
    cursor: wait;
    opacity: 0.6;
  }

  .tray-notice {
    position: fixed;
    right: 22px;
    bottom: 22px;
    z-index: 50;
    display: flex;
    max-width: min(440px, calc(100vw - 44px));
    align-items: flex-start;
    gap: 10px;
    border-radius: 11px;
    padding: 12px 12px 12px 14px;
    background: #1b2027;
    box-shadow: 0 0 0 1px rgba(191, 97, 106, 0.4), 0 14px 34px rgba(0, 0, 0, 0.5);
    color: #f0d6d8;
    font-size: 0.86rem;
    line-height: 1.45;
  }

  .tray-notice :global(svg) {
    flex: 0 0 auto;
    margin-top: 1px;
    color: #e2858d;
  }

  .tray-notice span {
    flex: 1 1 auto;
  }

  .tray-notice button {
    min-height: 0;
    border: 0;
    padding: 0 4px;
    background: transparent;
    color: #a2a9ac;
    font-size: 1.1rem;
    line-height: 1;
    cursor: pointer;
  }
</style>
