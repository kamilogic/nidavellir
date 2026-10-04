<script>
  import { onMount } from "svelte";
  import { Download, LoaderCircle } from "@lucide/svelte";
  import { cancelUpdate, checkForUpdate, installUpdate, requestUpdate, updateHold, updateState } from "../../updates.js";

  // A found update waits here; one click installs it, and WhatsNew shows the notes after the restart.
  let dialog = $state(null);
  const visible = $derived(["available", "downloading", "installing"].includes($updateState.status));
  const working = $derived(["downloading", "installing"].includes($updateState.status));
  const blocked = $derived($updateHold.forgeBusy && !working);
  const label = $derived(
    $updateState.status === "downloading"
      ? `Updating${$updateState.progress != null ? ` · ${$updateState.progress}%` : "…"}`
      : $updateState.status === "installing"
        ? "Installing…"
        : blocked
          ? "Update after the Forge run"
          : $updateState.error
            ? "Retry update"
            : `Update to ${$updateState.version}`,
  );
  const hint = $derived(
    $updateState.error
      ? `The update could not be installed: ${$updateState.error}`
      : blocked
        ? "Updating restarts the Core Service, so it waits for the Forge run to finish."
        : "Windows asks for permission. Nidavellir restarts by itself, and the GPU returns to stock while it updates.",
  );

  $effect(() => {
    if (!dialog) return;
    if ($updateState.confirming && !dialog.open) dialog.showModal();
    else if (!$updateState.confirming && dialog.open) dialog.close();
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
    cancelUpdate();
  }
</script>

{#if visible}
  <button
    class="update-button"
    class:blocked
    class:failed={$updateState.error && !working}
    class:expanded={working || ($updateState.error && !working)}
    type="button"
    title={hint}
    aria-label={`${label}. ${hint}`}
    onclick={requestUpdate}
    disabled={working || blocked}
  >
    {#if working}
      <LoaderCircle class="spin" size={16} strokeWidth={2.2} aria-hidden="true" />
    {:else}
      <Download size={16} strokeWidth={2.2} aria-hidden="true" />
    {/if}
    <span>{label}</span>
    {#if $updateState.status === "downloading"}
      <i style={`width: ${$updateState.progress ?? 6}%`} aria-hidden="true"></i>
    {/if}
  </button>
{/if}

<dialog class="confirm-dialog" bind:this={dialog} oncancel={cancel} aria-labelledby="update-confirm-title">
  <h2 id="update-confirm-title">Update now?</h2>
  <p>The saved Forge run cannot be resumed after updating. Its measurements and safety history stay saved.</p>
  <footer>
    <button class="confirm-later" type="button" onclick={cancelUpdate}>Not now</button>
    <button class="confirm-go" type="button" onclick={installUpdate}>Update anyway</button>
  </footer>
</dialog>

<style>
  /* Discord-like: a round icon in the corner that opens into its label on hover or focus. */
  .update-button {
    position: fixed;
    right: 22px;
    bottom: 22px;
    z-index: 40;
    display: inline-flex;
    overflow: hidden;
    min-height: 44px;
    align-items: center;
    border: 0;
    border-radius: 999px;
    padding: 0 13px;
    background: #d6a04c;
    color: #14110b;
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.45), 0 0 0 1px rgba(255, 255, 255, 0.06);
    font: inherit;
    font-size: 0.88rem;
    font-weight: 700;
    cursor: pointer;
    animation: rise-in 220ms ease-out;
  }

  .update-button:hover:not(:disabled) {
    background: #e2ad58;
  }

  .update-button span {
    position: relative;
    z-index: 1;
    max-width: 0;
    overflow: hidden;
    font-variant-numeric: tabular-nums;
    opacity: 0;
    white-space: nowrap;
    transition:
      max-width 220ms ease,
      margin 220ms ease,
      opacity 160ms ease;
  }

  .update-button:hover span,
  .update-button:focus-visible span,
  .update-button.expanded span {
    max-width: 260px;
    margin: 0 4px 0 8px;
    opacity: 1;
  }

  .update-button :global(svg) {
    position: relative;
    z-index: 1;
    flex: 0 0 auto;
  }

  .update-button i {
    position: absolute;
    inset: 0 auto 0 0;
    background: rgba(20, 17, 11, 0.16);
    transition: width 200ms ease;
  }

  .update-button:disabled {
    cursor: default;
  }

  .update-button.blocked {
    background: #1d232b;
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.45), inset 0 0 0 1px rgba(214, 160, 76, 0.35);
    color: #c9b48d;
  }

  .update-button.failed {
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.45), 0 0 0 2px rgba(226, 84, 90, 0.75);
  }

  .update-button :global(.spin) {
    animation: spin 900ms linear infinite;
  }

  .confirm-dialog {
    width: min(420px, calc(100vw - 32px));
    border: 1px solid rgba(214, 160, 76, 0.35);
    border-radius: 14px;
    padding: 22px;
    background: #12171d;
    color: #e3e3df;
    box-shadow: 0 24px 60px rgba(0, 0, 0, 0.55);
  }

  .confirm-dialog::backdrop {
    background: rgba(5, 8, 11, 0.7);
  }

  h2 {
    margin: 0;
    font-size: 1.1rem;
    font-weight: 650;
  }

  .confirm-dialog p {
    margin: 10px 0 0;
    color: #c4c9cb;
    font-size: 0.88rem;
    line-height: 1.5;
  }

  footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 20px;
  }

  footer button {
    min-height: 40px;
    border-radius: 10px;
    padding: 0 16px;
    font: inherit;
    font-weight: 650;
    cursor: pointer;
  }

  .confirm-later {
    border: 1px solid rgba(255, 255, 255, 0.12);
    background: transparent;
    color: #d7d9d7;
  }

  .confirm-go {
    border: 0;
    background: #d6a04c;
    color: #14110b;
  }

  @keyframes rise-in {
    from {
      opacity: 0;
      transform: translateY(8px);
    }
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .update-button,
    .update-button :global(.spin) {
      animation: none;
    }

    .update-button i,
    .update-button span {
      transition: none;
    }
  }
</style>
