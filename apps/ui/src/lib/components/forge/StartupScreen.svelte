<script>
  import { Check } from "@lucide/svelte";
  import { fade, fly } from "svelte/transition";
  import mark from "../../assets/themes/nidavellir-mark.png";

  // Real startup steps only, in order; the last unfinished one is running.
  let { visible = true, steps = [] } = $props();
  const still = globalThis.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;
</script>

{#if visible}
  <div class="startup" out:fade={{ duration: still ? 0 : 260 }}>
    <img src={mark} alt="" />
    <span class="wordmark">NIDAVELLIR</span>
    <div class="track" aria-hidden="true"><i></i></div>
    <ol aria-label="Startup" aria-live="polite">
      {#each steps as step (step.label)}
        <li class:done={step.done} aria-current={step.done ? undefined : "step"} in:fly={{ y: 6, duration: still ? 0 : 220 }}>
          <span class="mark" aria-hidden="true">{#if step.done}<Check size={13} strokeWidth={2.6} />{:else}<i></i>{/if}</span>
          {step.label}{step.done ? "" : "…"}
        </li>
      {/each}
    </ol>
  </div>
{/if}

<style>
  .startup {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: flex;
    flex-direction: column;
    align-items: center;
    /* Anchored from the top: new steps grow the log downward without moving the mark. */
    padding-top: calc(50vh - 112px);
    background:
      radial-gradient(circle at 50% 44%, rgba(185, 117, 75, 0.09), transparent 38%),
      var(--forge-void);
  }

  img {
    width: 72px;
    height: 72px;
    object-fit: contain;
  }

  .wordmark {
    margin-top: 16px;
    color: #aeb0b2;
    font-size: 20px;
    font-weight: 650;
    letter-spacing: 0.17em;
    /* The trailing letter-spacing would push the word off center. */
    text-indent: 0.17em;
  }

  .track {
    position: relative;
    width: 168px;
    height: 2px;
    margin-top: 26px;
    overflow: hidden;
    border-radius: 1px;
    background: rgba(174, 181, 186, 0.14);
  }

  .track i {
    position: absolute;
    inset: 0 auto 0 0;
    width: 36%;
    border-radius: inherit;
    background: var(--forge-gold);
    animation: sweep 1.5s cubic-bezier(0.65, 0, 0.35, 1) infinite;
  }

  ol {
    display: grid;
    gap: 7px;
    min-width: 210px;
    margin: 20px 0 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: flex;
    align-items: center;
    gap: 9px;
    color: #c9cdcd;
    font-size: 13px;
    letter-spacing: 0.02em;
  }

  li.done {
    color: #737b7e;
  }

  .mark {
    display: inline-grid;
    width: 13px;
    height: 13px;
    place-items: center;
    color: var(--forge-copper);
  }

  .mark i {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--forge-gold);
    animation: pulse 1.2s ease-in-out infinite;
  }

  @keyframes sweep {
    from { translate: -100% 0; }
    to { translate: 280% 0; }
  }

  @keyframes pulse {
    50% { opacity: 0.35; }
  }

  @media (prefers-reduced-motion: reduce) {
    .track i {
      width: 100%;
      opacity: 0.45;
      animation: none;
    }

    .mark i {
      animation: none;
    }
  }
</style>
