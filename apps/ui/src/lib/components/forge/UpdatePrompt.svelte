<script>
  import { onMount } from "svelte";
  import { ArrowRight, Check, Download, ShieldCheck } from "@lucide/svelte";
  import { parseNotes, releaseDate } from "../../release-notes.js";
  import { checkForUpdate, dismissUpdate, installUpdate, reviewUpdate, updateState } from "../../updates.js";

  // An update restarts the Core Service, and a new build cannot resume a saved run.
  let { forgeBusy = false, savedRun = false } = $props();

  let dialog = $state(null);
  const offering = $derived(["available", "downloading", "installing"].includes($updateState.status));
  const working = $derived(["downloading", "installing"].includes($updateState.status));
  const notes = $derived(parseNotes($updateState.notes));
  const released = $derived(releaseDate($updateState.date));
  const empty = $derived(!notes.summary.length && !notes.changes.length && !notes.fixes.length && !notes.closing.length);

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
  <header class="update-head">
    <span class="update-icon" aria-hidden="true"><Download size={20} strokeWidth={1.9} /></span>
    <div>
      <h2 id="update-title">Update available</h2>
      <p class="update-version">
        {#if $updateState.currentVersion}
          <span class="version-from" aria-label={`Installed version ${$updateState.currentVersion}`}>{$updateState.currentVersion}</span>
          <ArrowRight size={13} strokeWidth={2} aria-hidden="true" />
        {/if}
        <span class="version-to" aria-label={`New version ${$updateState.version}`}>{$updateState.version}</span>
        {#if released}<span class="version-date">Released {released}</span>{/if}
      </p>
    </div>
  </header>

  <div class="update-body">
    {#if empty}
      <p class="update-lead">This version includes fixes and improvements.</p>
    {/if}
    {#each notes.summary as line}
      <p class="update-lead">{line}</p>
    {/each}

    {#if notes.changes.length}
      <section class="update-section" aria-labelledby="update-new">
        <h3 id="update-new">What's new</h3>
        <ul>
          {#each notes.changes as change}
            <li><span class="marker" aria-hidden="true"></span>{change}</li>
          {/each}
        </ul>
      </section>
    {/if}

    {#if notes.fixes.length}
      <section class="update-section" aria-labelledby="update-fixed">
        <h3 id="update-fixed">Fixes</h3>
        <ul class="fixes">
          {#each notes.fixes as fix}
            <li><Check size={14} strokeWidth={2.4} aria-hidden="true" />{fix}</li>
          {/each}
        </ul>
      </section>
    {/if}

    {#each notes.closing as line}
      <p class="update-closing">{line}</p>
    {/each}
  </div>

  {#if forgeBusy || savedRun || $updateState.error || working}
    <div class="update-status">
      {#if forgeBusy}
        <p class="update-hold" role="status">A Forge run is active. Finish or stop it first: updating restarts the Core Service.</p>
      {:else if savedRun}
        <p class="update-hold" role="status">The saved run cannot be resumed after updating. Its measurements and safety history stay saved.</p>
      {/if}
      {#if $updateState.status === "downloading"}
        <div class="update-progress" role="progressbar" aria-label="Downloading update" aria-valuemin="0" aria-valuemax="100" aria-valuenow={$updateState.progress ?? undefined}>
          <span>Downloading</span>
          <strong>{$updateState.progress != null ? `${$updateState.progress}%` : "…"}</strong>
          <i style={`width: ${$updateState.progress ?? 6}%`}></i>
        </div>
      {:else if $updateState.status === "installing"}
        <p class="update-hold" role="status">Installing. Nidavellir closes and reopens by itself.</p>
      {/if}
      {#if $updateState.error}
        <p class="update-error" role="alert">The update could not be installed: {$updateState.error}. Try again, or download it from github.com/kamilogic/nidavellir/releases/latest.</p>
      {/if}
    </div>
  {/if}

  <footer class="update-foot">
    <p class="update-safety">
      <ShieldCheck size={16} strokeWidth={1.8} aria-hidden="true" />
      <span>Windows asks for permission. The GPU returns to stock while Nidavellir updates; your profiles and safety history are kept.</span>
    </p>
    <div class="update-actions">
      <button class="update-later" type="button" onclick={dismissUpdate} disabled={working}>Later</button>
      <button class="update-install" type="button" onclick={installUpdate} disabled={working || forgeBusy}>
        <Download size={16} strokeWidth={2} aria-hidden="true" />{working ? "Updating…" : "Update now"}
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
    width: min(580px, calc(100vw - 32px));
    max-height: calc(100vh - 48px);
    overflow: hidden;
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 16px;
    padding: 0;
    background: #12171d;
    color: #e3e3df;
    box-shadow: 0 30px 80px rgba(0, 0, 0, 0.6), 0 0 0 1px rgba(214, 160, 76, 0.06);
  }

  .update-dialog[open] {
    display: flex;
    flex-direction: column;
    animation: dialog-in 180ms ease-out;
  }

  .update-dialog::backdrop {
    background: rgba(5, 8, 11, 0.7);
    backdrop-filter: blur(2px);
  }

  .update-head {
    display: flex;
    align-items: center;
    gap: 14px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.06);
    padding: 22px 26px 20px;
  }

  .update-icon {
    display: grid;
    flex: 0 0 auto;
    width: 42px;
    height: 42px;
    place-items: center;
    border-radius: 11px;
    background: rgba(214, 160, 76, 0.13);
    box-shadow: inset 0 0 0 1px rgba(214, 160, 76, 0.22);
    color: #d6a04c;
  }

  h2 {
    margin: 0;
    font-size: 1.15rem;
    font-weight: 650;
    letter-spacing: -0.01em;
  }

  .update-version {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    margin: 5px 0 0;
    color: #8e979d;
    font-size: 0.82rem;
    font-variant-numeric: tabular-nums;
  }

  .version-from {
    color: #a2a9ac;
  }

  .version-to {
    color: #d6a04c;
    font-weight: 650;
  }

  .version-date::before {
    margin: 0 6px 0 2px;
    color: #5c6774;
    content: "·";
  }

  .update-body {
    overflow-y: auto;
    padding: 20px 26px 8px;
    scrollbar-color: rgba(255, 255, 255, 0.16) transparent;
    scrollbar-width: thin;
  }

  .update-lead {
    margin: 0 0 18px;
    color: #eceee9;
    font-size: 0.98rem;
    line-height: 1.55;
    text-wrap: pretty;
  }

  .update-section {
    margin-bottom: 18px;
  }

  h3 {
    margin: 0 0 8px;
    color: #d6a04c;
    font-size: 0.7rem;
    font-weight: 750;
    letter-spacing: 0.13em;
    text-transform: uppercase;
  }

  ul {
    display: grid;
    gap: 2px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: grid;
    grid-template-columns: 16px minmax(0, 1fr);
    gap: 10px;
    padding: 5px 0;
    color: #c4c9cb;
    font-size: 0.88rem;
    line-height: 1.5;
    text-wrap: pretty;
  }

  .marker {
    width: 6px;
    height: 6px;
    margin: 8px 0 0 5px;
    border-radius: 50%;
    background: #d6a04c;
  }

  .fixes :global(svg) {
    margin-top: 4px;
    color: #5fbf8f;
  }

  .update-closing {
    margin: 2px 0 14px;
    border-top: 1px solid rgba(255, 255, 255, 0.06);
    padding-top: 14px;
    color: #8e979d;
    font-size: 0.82rem;
    line-height: 1.5;
  }

  .update-status {
    display: grid;
    gap: 10px;
    padding: 4px 26px 14px;
  }

  .update-hold,
  .update-error {
    margin: 0;
    border-radius: 9px;
    padding: 9px 12px;
    font-size: 0.8rem;
    line-height: 1.45;
  }

  .update-hold {
    background: rgba(214, 160, 76, 0.08);
    box-shadow: inset 0 0 0 1px rgba(214, 160, 76, 0.26);
    color: #e2c79a;
  }

  .update-error {
    background: rgba(191, 97, 106, 0.1);
    box-shadow: inset 0 0 0 1px rgba(191, 97, 106, 0.35);
    color: #f0b8bc;
  }

  .update-progress {
    position: relative;
    display: flex;
    justify-content: space-between;
    overflow: hidden;
    border-radius: 9px;
    padding: 9px 12px;
    background: rgba(255, 255, 255, 0.04);
    font-size: 0.8rem;
  }

  .update-progress span,
  .update-progress strong {
    position: relative;
    z-index: 1;
  }

  .update-progress strong {
    color: #d6a04c;
    font-variant-numeric: tabular-nums;
  }

  .update-progress i {
    position: absolute;
    inset: 0 auto 0 0;
    background: rgba(214, 160, 76, 0.18);
    transition: width 200ms ease;
  }

  .update-foot {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 14px 18px;
    border-top: 1px solid rgba(255, 255, 255, 0.06);
    padding: 16px 26px 18px;
    background: rgba(0, 0, 0, 0.14);
  }

  .update-safety {
    display: flex;
    flex: 1 1 250px;
    gap: 9px;
    margin: 0;
    color: #8e979d;
    font-size: 0.75rem;
    line-height: 1.45;
  }

  .update-safety :global(svg) {
    flex: 0 0 auto;
    margin-top: 1px;
    color: #6f8f80;
  }

  .update-actions {
    display: flex;
    gap: 8px;
  }

  .update-actions button,
  .update-pill {
    display: inline-flex;
    min-height: 40px;
    align-items: center;
    gap: 8px;
    border-radius: 10px;
    padding: 0 16px;
    font: inherit;
    font-size: 0.9rem;
    font-weight: 650;
    cursor: pointer;
  }

  .update-later {
    border: 1px solid rgba(255, 255, 255, 0.12);
    background: transparent;
    color: #d7d9d7;
  }

  .update-later:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.05);
  }

  .update-install,
  .update-pill {
    border: 0;
    background: #d6a04c;
    color: #14110b;
  }

  .update-install:hover:not(:disabled),
  .update-pill:hover {
    background: #e2ad58;
  }

  .update-actions button:disabled {
    cursor: not-allowed;
    opacity: 0.55;
  }

  .update-pill {
    position: fixed;
    right: 22px;
    bottom: 22px;
    z-index: 40;
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.45);
  }

  @keyframes dialog-in {
    from {
      opacity: 0;
      transform: translateY(6px) scale(0.985);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .update-dialog[open] {
      animation: none;
    }

    .update-progress i {
      transition: none;
    }
  }
</style>
