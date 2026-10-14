<script>
  import commandMark from "../../assets/themes/nidavellir-mark.png";
  import copperPlate from "../../assets/themes/copper-plate.png";
  import AutoResumeToggle from "./AutoResumeToggle.svelte";

  // Shown before a new Forge starts: the run mode, what to expect, and the auto-continue choice.
  let {
    open = $bindable(false),
    actionLabel = "Forge GPU",
    forgeMode = "standard",
    modeDisabled = false,
    powerSweep = null,
    serviceReady = false,
    onModeChange,
    onStart,
  } = $props();

  // Budgets from the Core: Standard and Clean Run stop at 8 h; Long's longer dwells scale it to ~18 h.
  const MODES = [
    { id: "standard", label: "Standard", note: "Recommended. Reuses what this GPU already proved and measures the rest. Up to 8 h." },
    { id: "long", label: "Long", note: "Tests each candidate more than twice as long, for extra confidence. Up to about 18 h." },
    { id: "clean", label: "Clean Run", note: "Measures everything again from scratch. Known failures stay blocked. Up to 8 h." },
  ];

  let dialog = $state(null);
  let cancelButton = $state(null);
  const mode = $derived(MODES.find((entry) => entry.id === forgeMode) ?? MODES[0]);

  $effect(() => {
    if (!dialog) return;
    if (open && !dialog.open) {
      dialog.showModal();
      // Cancel is the safe default: a stray Enter never starts a run of several hours.
      cancelButton?.focus();
    } else if (!open && dialog.open) {
      dialog.close();
    }
  });

  function close() {
    open = false;
  }

  function start() {
    open = false;
    onStart?.();
  }
</script>

<dialog
  bind:this={dialog}
  class="preforge"
  aria-labelledby="preforge-title"
  oncancel={(event) => { event.preventDefault(); close(); }}
>
  <div class="mark"><img src={commandMark} alt="" /><span>{actionLabel}</span></div>
  <h2 id="preforge-title">Before the Forge starts</h2>
  <p class="lead">A Forge takes hours and runs by itself. You can stop it at any time.</p>

  <span class="label" id="preforge-mode-label">Run mode</span>
  <div class="modes" role="radiogroup" aria-labelledby="preforge-mode-label">
    {#each MODES as entry}
      <button
        type="button"
        role="radio"
        aria-checked={forgeMode === entry.id}
        class:on={forgeMode === entry.id}
        disabled={modeDisabled}
        onclick={() => onModeChange?.(entry.id)}
      >{entry.label}</button>
    {/each}
  </div>
  <p class="mode-note">{mode.note}</p>

  <span class="label">What to expect</span>
  <ul class="expect">
    <li>Close games and GPU-heavy apps. The tests need the GPU to themselves.</li>
    <li>The screen may flicker or go black for a few seconds when a test finds the GPU's limit. That is expected.</li>
    <li>Keep the PC on and save your work in other programs. After a driver crash, Nidavellir may restart Windows, after a 2-minute warning you can cancel.</li>
  </ul>

  <div class="auto"><AutoResumeToggle {powerSweep} disabled={!serviceReady} detail /></div>

  <div class="actions">
    <button bind:this={cancelButton} type="button" class="cancel" onclick={close}>Cancel</button>
    <button type="button" class="plate" onclick={start}><img src={copperPlate} alt="" /><span>Start Forge</span></button>
  </div>
</dialog>

<style>
  .preforge {
    width: min(600px, calc(100vw - 48px));
    max-height: calc(100vh - 48px);
    overflow-y: auto;
    border: 1px solid rgba(197, 134, 79, 0.45);
    border-radius: 12px;
    padding: 28px 32px 26px;
    background: rgba(11, 14, 15, 0.98);
    color: #d8dbde;
    box-shadow: 0 30px 80px rgba(0, 0, 0, 0.55);
    font-family: "Segoe UI", system-ui, sans-serif;
  }

  .preforge::backdrop {
    background: rgba(3, 4, 5, 0.72);
  }

  .mark {
    display: flex;
    align-items: center;
    gap: 10px;
    color: #a5aaad;
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }

  .mark img {
    width: 26px;
    height: 26px;
    object-fit: contain;
  }

  h2 {
    margin: 10px 0 2px;
    color: #f0f0ef;
    font-size: 26px;
    font-weight: 540;
    letter-spacing: -0.01em;
  }

  .lead,
  .mode-note {
    margin: 0;
    color: #8f979c;
    font-size: 14px;
    text-wrap: pretty;
  }

  .label {
    display: block;
    margin-top: 24px;
    color: #a5aaad;
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }

  .modes {
    display: flex;
    gap: 6px;
    margin-top: 4px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.12);
  }

  .modes button {
    position: relative;
    border: 0;
    padding: 6px 12px 10px;
    background: transparent;
    color: #a5a9ae;
    font: inherit;
    font-size: 14px;
    cursor: pointer;
  }

  .modes button.on {
    color: #e9ba79;
  }

  .modes button.on::after {
    position: absolute;
    right: 8px;
    bottom: 0;
    left: 8px;
    height: 2px;
    background: #c5864f;
    content: "";
  }

  .modes button:disabled {
    cursor: not-allowed;
    opacity: 0.5;
  }

  .modes button:focus-visible,
  .cancel:focus-visible,
  .plate:focus-visible {
    outline: 2px solid #c5864f;
    outline-offset: 2px;
  }

  .mode-note {
    margin-top: 9px;
    font-size: 13px;
  }

  .expect {
    display: grid;
    gap: 9px;
    margin: 10px 0 0;
    padding: 0;
    list-style: none;
    color: #c3c8cb;
    font-size: 14px;
    line-height: 1.45;
  }

  .expect li {
    display: grid;
    grid-template-columns: 16px minmax(0, 1fr);
    text-wrap: pretty;
  }

  .expect li::before {
    color: #c5864f;
    content: "–";
  }

  .auto {
    margin-top: 22px;
    border-top: 1px solid rgba(255, 255, 255, 0.12);
    border-bottom: 1px solid rgba(255, 255, 255, 0.12);
    padding: 12px 2px;
  }

  .actions {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 22px;
    margin-top: 24px;
  }

  .cancel {
    min-height: 40px;
    border: 0;
    padding: 0 6px;
    background: transparent;
    color: #a5aaad;
    font: inherit;
    font-size: 15px;
    cursor: pointer;
  }

  .cancel:hover {
    color: #e2e4e5;
  }

  .plate {
    position: relative;
    display: grid;
    width: 230px;
    height: 70px;
    place-items: center;
    border: 0;
    padding: 0;
    background: transparent;
    cursor: pointer;
    transition: filter 150ms ease, transform 100ms ease;
  }

  .plate:hover { filter: brightness(1.07); }
  .plate:active { transform: scale(0.96); }

  .plate img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: fill;
    filter: saturate(0.85) brightness(1.1);
  }

  .plate span {
    position: relative;
    color: #15110d;
    font-size: 19px;
    font-weight: 600;
  }

  @media (prefers-reduced-motion: reduce) {
    .plate { transition: none; }
  }
</style>
