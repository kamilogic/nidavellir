<script>
  import { onMount } from "svelte";
  import { Download } from "@lucide/svelte";
  import { checkForUpdate, dismissUpdate, installUpdate, reviewUpdate, updateState } from "../../updates.js";

  // An update restarts the Core Service, and a new build cannot resume a saved run.
  let { forgeBusy = false, savedRun = false } = $props();

  let dialog = $state(null);
  const offering = $derived(["available", "downloading", "installing"].includes($updateState.status));
  const working = $derived(["downloading", "installing"].includes($updateState.status));

  $effect(() => {
    if (!dialog) return;
    if (offering && !$updateState.dismissed) {
      if (!dialog.open) dialog.showModal();
    } else if (dialog.open) {
      dialog.close();
    }
  });

  onMount(() => {
    const first = setTimeout(() => checkForUpdate(), 4000);
    const periodic = setInterval(() => checkForUpdate(), 6 * 60 * 60 * 1000);
    return () => {
      clearTimeout(first);
      clearInterval(periodic);
    };
  });

  function cancel(event) {
    event.preventDefault();
    if (!working) dismissUpdate();
  }
</script>

<dialog class="update-dialog" bind:this={dialog} oncancel={cancel} aria-labelledby="update-title">
  <header>
    <span class="update-icon"><Download size={22} strokeWidth={1.8} /></span>
    <div>
      <h2 id="update-title">Update available</h2>
      <p>Nidavellir {$updateState.version} is ready to install. You have {$updateState.currentVersion}.</p>
    </div>
  </header>

  <section class="update-notes" aria-label="What's new">
    <h3>What's new</h3>
    <div>{$updateState.notes || "This version includes fixes and improvements."}</div>
  </section>

  {#if forgeBusy}
    <p class="update-hold" role="status">A Forge run is active. Finish or stop it first: updating restarts the Core Service.</p>
  {:else if savedRun}
    <p class="update-hold" role="status">The saved run cannot be resumed after updating. Its measurements and safety history stay saved.</p>
  {/if}

  {#if $updateState.status === "downloading"}
    <div class="update-progress" role="progressbar" aria-label="Downloading update" aria-valuemin="0" aria-valuemax="100" aria-valuenow={$updateState.progress ?? undefined}>
      <span>Downloading{$updateState.progress != null ? ` · ${$updateState.progress}%` : "…"}</span>
      <i style={`width: ${$updateState.progress ?? 8}%`}></i>
    </div>
  {:else if $updateState.status === "installing"}
    <p class="update-hold" role="status">Installing. Nidavellir closes and reopens by itself.</p>
  {/if}

  {#if $updateState.error}
    <p class="update-error" role="alert">The update could not be installed: {$updateState.error}. Try again, or download it from github.com/kamilogic/nidavellir/releases.</p>
  {/if}

  <footer>
    <small>Windows asks for permission to install. The GPU returns to stock while the Core Service restarts; profiles and safety history stay on this PC.</small>
    <div>
      <button class="update-later" type="button" onclick={dismissUpdate} disabled={working}>Later</button>
      <button class="update-install" type="button" onclick={installUpdate} disabled={working || forgeBusy}>
        {working ? "Updating…" : "Update now"}
      </button>
    </div>
  </footer>
</dialog>

{#if offering && $updateState.dismissed}
  <button class="update-pill" type="button" onclick={reviewUpdate}>
    <Download size={16} strokeWidth={2} />Update {$updateState.version}
  </button>
{/if}

<style>
  .update-dialog {
    width: min(560px, calc(100vw - 32px));
    border: 1px solid rgba(214, 160, 76, 0.35);
    border-radius: 14px;
    padding: 22px;
    background: #12171d;
    color: #e3e3df;
    box-shadow: 0 24px 60px rgba(0, 0, 0, 0.55);
  }

  .update-dialog::backdrop {
    background: rgba(5, 8, 11, 0.72);
  }

  header {
    display: flex;
    gap: 14px;
    align-items: flex-start;
  }

  .update-icon {
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

  .update-notes {
    margin-top: 18px;
    border-radius: 10px;
    padding: 12px 14px;
    background: rgba(0, 0, 0, 0.25);
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.06);
  }

  h3 {
    margin: 0 0 8px;
    color: #d6a04c;
    font-size: 0.75rem;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .update-notes div {
    max-height: 240px;
    overflow: auto;
    color: #d7d9d7;
    font-size: 0.88rem;
    line-height: 1.55;
    white-space: pre-wrap;
  }

  .update-hold,
  .update-error {
    margin: 14px 0 0;
    border-radius: 9px;
    padding: 9px 12px;
    font-size: 0.82rem;
    line-height: 1.45;
  }

  .update-hold {
    background: rgba(214, 160, 76, 0.08);
    box-shadow: inset 0 0 0 1px rgba(214, 160, 76, 0.28);
    color: #e2c79a;
  }

  .update-error {
    background: rgba(191, 97, 106, 0.1);
    box-shadow: inset 0 0 0 1px rgba(191, 97, 106, 0.35);
    color: #f0b8bc;
  }

  .update-progress {
    position: relative;
    margin-top: 14px;
    overflow: hidden;
    border-radius: 9px;
    padding: 9px 12px;
    background: rgba(255, 255, 255, 0.04);
    font-size: 0.82rem;
  }

  .update-progress span {
    position: relative;
    z-index: 1;
  }

  .update-progress i {
    position: absolute;
    inset: 0 auto 0 0;
    background: rgba(214, 160, 76, 0.22);
    transition: width 200ms ease;
  }

  footer {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 14px;
    margin-top: 18px;
  }

  footer small {
    flex: 1 1 240px;
    color: #8e979d;
    font-size: 0.75rem;
    line-height: 1.45;
  }

  footer div {
    display: flex;
    gap: 8px;
  }

  footer button,
  .update-pill {
    min-height: 40px;
    border: 0;
    border-radius: 9px;
    padding: 0 16px;
    font: inherit;
    font-weight: 650;
    cursor: pointer;
  }

  .update-later {
    background: rgba(255, 255, 255, 0.07);
    color: #d7d9d7;
  }

  .update-install,
  .update-pill {
    background: #d6a04c;
    color: #14110b;
  }

  footer button:disabled {
    cursor: not-allowed;
    opacity: 0.55;
  }

  .update-pill {
    position: fixed;
    right: 22px;
    bottom: 22px;
    z-index: 40;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.45);
  }
</style>
