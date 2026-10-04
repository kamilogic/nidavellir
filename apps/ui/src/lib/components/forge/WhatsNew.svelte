<script>
  import { onMount } from "svelte";
  import { getVersion } from "@tauri-apps/api/app";
  import { ArrowRight, Sparkles } from "@lucide/svelte";
  import ReleaseNotes from "./ReleaseNotes.svelte";
  import { bundledNotes } from "../../bundled-notes.js";
  import { compareVersions, releasesSince } from "../../release-notes.js";

  // Once after an update: the notes of every version since the one that ran before.
  const LAST_VERSION = "nidavellir-last-version";
  let dialog = $state(null);
  let releases = $state([]);
  let from = $state(null);
  let to = $state(null);

  onMount(() => {
    let active = true;
    getVersion()
      .then((current) => {
        if (!active) return;
        let previous = null;
        let onboarded = false;
        try {
          previous = localStorage.getItem(LAST_VERSION);
          onboarded = localStorage.getItem("nidavellir-gpu-onboarded") === "true";
          localStorage.setItem(LAST_VERSION, current);
        } catch {
          return;
        }
        // A fresh install starts with onboarding; a reinstall or a downgrade shows nothing.
        if (previous ? compareVersions(current, previous) <= 0 : !onboarded) return;
        releases = releasesSince(previous, current, bundledNotes);
        from = previous;
        to = current;
        if (releases.length) dialog?.showModal();
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  });
</script>

<dialog class="whats-new" bind:this={dialog} aria-labelledby="whats-new-title">
  <header>
    <span class="icon" aria-hidden="true"><Sparkles size={20} strokeWidth={1.9} /></span>
    <div>
      <h2 id="whats-new-title">Updated to {to}</h2>
      <p class="version">
        {#if from}
          <span class="from">{from}</span>
          <ArrowRight size={13} strokeWidth={2} aria-hidden="true" />
        {/if}
        <span class="to">{to}</span>
        <span class="caption">What changed in this update</span>
      </p>
    </div>
  </header>

  <div class="body">
    {#each releases as release}
      <section class="release" aria-label={`Version ${release.version}`}>
        {#if releases.length > 1}
          <h3 class="release-version">Version {release.version}</h3>
        {/if}
        <ReleaseNotes notes={release.notes} headingLevel={releases.length > 1 ? 4 : 3} />
      </section>
    {/each}
  </div>

  <footer>
    <button type="button" onclick={() => dialog?.close()}>Got it</button>
  </footer>
</dialog>

<style>
  .whats-new {
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

  .whats-new[open] {
    display: flex;
    flex-direction: column;
    animation: dialog-in 180ms ease-out;
  }

  .whats-new::backdrop {
    background: rgba(5, 8, 11, 0.7);
    backdrop-filter: blur(2px);
  }

  header {
    display: flex;
    align-items: center;
    gap: 14px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.06);
    padding: 22px 26px 20px;
  }

  .icon {
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

  .version {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    margin: 5px 0 0;
    color: #8e979d;
    font-size: 0.82rem;
    font-variant-numeric: tabular-nums;
  }

  .from {
    color: #a2a9ac;
  }

  .to {
    color: #d6a04c;
    font-weight: 650;
  }

  .caption::before {
    margin: 0 6px 0 2px;
    color: #5c6774;
    content: "·";
  }

  .body {
    overflow-y: auto;
    padding: 20px 26px 8px;
    scrollbar-color: rgba(255, 255, 255, 0.16) transparent;
    scrollbar-width: thin;
  }

  .release + .release {
    border-top: 1px solid rgba(255, 255, 255, 0.06);
    padding-top: 18px;
  }

  .release-version {
    margin: 0 0 12px;
    color: #a2a9ac;
    font-size: 0.8rem;
    font-weight: 650;
    letter-spacing: 0.04em;
  }

  footer {
    display: flex;
    justify-content: flex-end;
    border-top: 1px solid rgba(255, 255, 255, 0.06);
    padding: 16px 26px 18px;
    background: rgba(0, 0, 0, 0.14);
  }

  footer button {
    min-height: 40px;
    border: 0;
    border-radius: 10px;
    padding: 0 22px;
    background: #d6a04c;
    color: #14110b;
    font: inherit;
    font-size: 0.9rem;
    font-weight: 650;
    cursor: pointer;
  }

  footer button:hover {
    background: #e2ad58;
  }

  @keyframes dialog-in {
    from {
      opacity: 0;
      transform: translateY(6px) scale(0.985);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .whats-new[open] {
      animation: none;
    }
  }
</style>
