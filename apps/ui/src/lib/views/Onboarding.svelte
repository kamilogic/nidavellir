<script>
  import { onMount } from "svelte";
  import { CircleCheck, Gpu, LoaderCircle, RefreshCw, RotateCcw, ShieldCheck, TriangleAlert } from "@lucide/svelte";
  import mark from "../assets/themes/nidavellir-mark.png";
  import { serviceCall } from "../service.js";
  import { nvidiaGpu, requireServiceData } from "../forge-workflow.js";
  import { t } from "../i18n.js";

  let { onComplete } = $props();
  let detecting = $state(true);
  let error = $state(null);
  let gpu = $state(null);

  async function detect() {
    detecting = true;
    error = null;
    try {
      gpu = nvidiaGpu(requireServiceData(await serviceCall("DetectHardware"), "Hardware", "GPU detection"));
      if (!gpu) {
        throw new Error("No NVIDIA GPU was detected. This beta requires an NVIDIA GPU and its Windows driver. Check the driver installation, then try again.");
      }
    } catch (e) {
      gpu = null;
      error = e?.message ?? String(e);
    } finally {
      detecting = false;
    }
  }

  onMount(() => {
    detect();
  });
</script>

<div class="welcome">
  <header class="brand">
    <img class="brand-mark" src={mark} alt="" />
    <h1>Nidavellir</h1>
    <p>{$t("app.tagline")}</p>
  </header>

  <section class="gpu-card" class:found={gpu} class:failed={error} aria-live="polite" aria-label="GPU detection">
    <span class="gpu-icon" aria-hidden="true">
      {#if detecting}
        <LoaderCircle class="spin" size={24} strokeWidth={1.8} />
      {:else if gpu}
        <Gpu size={24} strokeWidth={1.6} />
      {:else}
        <TriangleAlert size={24} strokeWidth={1.8} />
      {/if}
    </span>
    <div class="gpu-copy">
      <span class="kicker">{detecting ? "Detecting" : gpu ? "GPU found" : "GPU not ready"}</span>
      {#if detecting}
        <strong>Looking for your NVIDIA GPU…</strong>
      {:else if gpu}
        <strong>{gpu.model}</strong>
        <small>{gpu.driver ? `Driver ${gpu.driver}` : "Detected by the Nidavellir Core"}</small>
      {:else}
        <p class="error">{error}</p>
      {/if}
    </div>
    {#if gpu}
      <span class="gpu-ok" aria-hidden="true"><CircleCheck size={22} strokeWidth={1.9} /></span>
    {:else if error}
      <button class="retry" type="button" onclick={detect}><RefreshCw size={15} strokeWidth={2} />Try again</button>
    {/if}
  </section>

  <section class="notice" aria-labelledby="notice-title">
    <h2 id="notice-title">Before you forge</h2>
    <ul>
      <li>
        <TriangleAlert size={18} strokeWidth={1.8} />
        <span>Nidavellir finds your card's limits by testing near the edge of stability. A test can crash the display driver or restart Windows.</span>
      </li>
      <li>
        <ShieldCheck size={18} strokeWidth={1.8} />
        <span>Safe Loop records every risky step before it runs, and keeps the GPU at stock after a failure.</span>
      </li>
      <li>
        <RotateCcw size={18} strokeWidth={1.8} />
        <span>Stock is always one click away, and exiting Nidavellir returns the GPU to stock.</span>
      </li>
    </ul>
  </section>

  <button class="go" type="button" onclick={() => onComplete?.("gpu")} disabled={!gpu || detecting}>
    I understand, open the Forge
  </button>
</div>

<style>
  .welcome {
    display: flex;
    width: min(560px, calc(100vw - 32px));
    min-height: 100vh;
    margin: 0 auto;
    padding: 48px 0;
    box-sizing: border-box;
    flex-direction: column;
    justify-content: center;
    gap: 18px;
    animation: rise 320ms ease-out both;
  }

  .brand {
    margin-bottom: 14px;
    text-align: center;
  }

  .brand-mark {
    display: block;
    width: 68px;
    height: 68px;
    margin: 0 auto;
    object-fit: contain;
  }

  h1 {
    margin: 16px 0 0;
    color: var(--forge-text);
    font-size: 1.9rem;
    font-weight: 800;
    letter-spacing: 0.22em;
    text-transform: uppercase;
  }

  .brand p {
    margin: 8px 0 0;
    color: var(--forge-muted);
    font-size: 0.95rem;
    font-style: italic;
  }

  .gpu-card,
  .notice {
    border-radius: 14px;
    background: var(--forge-panel);
    box-shadow: inset 0 0 0 1px var(--forge-line);
  }

  .gpu-card {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: center;
    gap: 16px;
    padding: 18px 20px;
    transition: box-shadow 200ms ease;
  }

  .gpu-card.found {
    box-shadow: inset 0 0 0 1px rgba(51, 196, 129, 0.35);
  }

  .gpu-card.failed {
    box-shadow: inset 0 0 0 1px rgba(226, 84, 90, 0.4);
  }

  .gpu-icon {
    display: grid;
    width: 46px;
    height: 46px;
    place-items: center;
    border-radius: 12px;
    background: var(--forge-panel-raised);
    color: var(--forge-steel);
  }

  .found .gpu-icon {
    color: var(--forge-green);
  }

  .failed .gpu-icon {
    color: var(--forge-red);
  }

  .gpu-copy {
    display: flex;
    min-width: 0;
    flex-direction: column;
    gap: 3px;
  }

  .kicker {
    color: var(--forge-dim);
    font-size: 0.68rem;
    font-weight: 800;
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }

  .gpu-copy strong {
    overflow: hidden;
    color: var(--forge-text);
    font-size: 1.12rem;
    font-weight: 650;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .gpu-copy small {
    color: var(--forge-muted);
    font-size: 0.8rem;
  }

  .error {
    margin: 2px 0 0;
    color: #f0b8bc;
    font-size: 0.86rem;
    line-height: 1.45;
  }

  .gpu-ok {
    color: var(--forge-green);
  }

  .retry {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    border: 0;
    border-radius: 9px;
    padding: 0 14px;
    background: var(--forge-panel-raised);
    color: var(--forge-text);
    font: inherit;
    font-size: 0.85rem;
    font-weight: 650;
    cursor: pointer;
  }

  .notice {
    padding: 18px 20px 6px;
  }

  h2 {
    margin: 0 0 4px;
    color: var(--forge-gold);
    font-size: 0.72rem;
    font-weight: 800;
    letter-spacing: 0.14em;
    text-transform: uppercase;
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: grid;
    grid-template-columns: 18px minmax(0, 1fr);
    gap: 12px;
    border-top: 1px solid rgba(255, 255, 255, 0.05);
    padding: 12px 0;
    color: var(--forge-muted);
    font-size: 0.9rem;
    line-height: 1.5;
  }

  li:first-child {
    border-top: 0;
  }

  li :global(svg) {
    margin-top: 2px;
    color: var(--forge-gold);
  }

  .go {
    min-height: 48px;
    margin-top: 6px;
    border: 0;
    border-radius: 11px;
    background: var(--forge-gold);
    color: var(--forge-ink);
    font: inherit;
    font-size: 1rem;
    font-weight: 750;
    cursor: pointer;
  }

  .go:hover:not(:disabled) {
    background: #eab452;
  }

  .go:disabled {
    background: var(--forge-panel-raised);
    color: var(--forge-dim);
    cursor: not-allowed;
  }

  .welcome :global(.spin) {
    animation: spin 900ms linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(8px);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .welcome,
    .welcome :global(.spin) {
      animation: none;
    }
  }
</style>
