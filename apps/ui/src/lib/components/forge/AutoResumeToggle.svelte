<script>
  import { serviceCall } from "../../service.js";

  // Automatic continuation after a TDR at a step edge: on unless the user turns it off. The service
  // persists the choice. `detail` adds the full explanation (the pre-Forge sheet).
  let { powerSweep = null, disabled = false, detail = false } = $props();

  let busy = $state(false);
  let override = $state(null);
  let error = $state(null);
  let now = $state(Date.now());
  const enabled = $derived(override ?? Boolean(powerSweep?.auto_resume));
  const seconds = $derived(
    powerSweep?.auto_resume_at_ms ? Math.max(0, Math.ceil((powerSweep.auto_resume_at_ms - now) / 1000)) : null,
  );

  $effect(() => {
    powerSweep?.auto_resume;
    override = null;
  });

  $effect(() => {
    if (!powerSweep?.auto_resume_at_ms) return;
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });

  async function setEnabled(next) {
    busy = true;
    override = next;
    error = null;
    try {
      const response = await serviceCall("SetForgeAutoResume", { enabled: next });
      if (response?.ok === false) throw new Error(response.error ?? "The service refused the change.");
    } catch (e) {
      override = null;
      error = String(e?.message ?? e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="auto-resume">
  <label class="option-row" title="If the GPU driver crashes at a test edge, the installed service resets only the driver, or the run waits for the next Windows start, then the same run continues.">
    <span class="option-text">
      <span class="option-eyebrow">After a driver crash</span>
      <span class="option-value">Continue automatically<em>Recommended</em></span>
      {#if detail}
        <span class="option-detail">Nidavellir waits 2 minutes (you can cancel), then continues the same run. The failed point is saved and never tested again. When this is off, the run stops and waits for you.</span>
      {/if}
    </span>
    <input
      type="checkbox"
      role="switch"
      aria-label="Continue automatically after a driver crash"
      checked={enabled}
      disabled={busy || disabled}
      onchange={(event) => setEnabled(event.currentTarget.checked)}
    />
  </label>
  {#if seconds != null}
    <p class="auto-resume-countdown" role="status">
      Continuing in {seconds}s
      <button type="button" onclick={() => setEnabled(false)}>Cancel</button>
    </p>
  {/if}
  {#if error}<p class="auto-resume-error" role="alert">{error}</p>{/if}
</div>

<style>
  .auto-resume {
    display: grid;
    gap: 6px;
  }

  .option-row {
    display: flex;
    align-items: center;
    gap: 12px;
    cursor: pointer;
  }

  .option-text {
    display: grid;
    flex: 1;
    min-width: 0;
    gap: 3px;
  }

  .option-eyebrow {
    color: #a5aaad;
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }

  .option-value {
    color: #e2e4e5;
    font-size: 14px;
    white-space: nowrap;
  }

  .option-value em {
    margin-left: 6px;
    color: #9fc95f;
    font-size: 11px;
    font-style: normal;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .option-detail {
    margin-top: 3px;
    color: #8f979c;
    font-size: 13px;
    line-height: 1.45;
    text-wrap: pretty;
  }

  input {
    position: relative;
    width: 40px;
    height: 22px;
    margin: 0;
    flex: 0 0 auto;
    appearance: none;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.14);
    cursor: pointer;
    transition: background-color 150ms ease;
  }

  input::after {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: #e8ebe9;
    content: "";
    transition: transform 150ms ease;
  }

  input:checked { background: #d0a15f; }
  input:checked::after { transform: translateX(18px); }

  input:focus-visible {
    outline: 2px solid #d0a15f;
    outline-offset: 2px;
  }

  input:disabled {
    cursor: wait;
    opacity: 0.6;
  }

  .auto-resume-countdown {
    margin: 0;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
    border-radius: 9px;
    padding: 6px 10px;
    background: rgba(214, 160, 76, 0.08);
    box-shadow: inset 0 0 0 1px rgba(214, 160, 76, 0.3);
    color: #c9cbc6;
    font-size: 13px;
  }

  .auto-resume-countdown button {
    min-height: 0;
    border: 0;
    border-radius: 7px;
    padding: 4px 10px;
    background: rgba(255, 255, 255, 0.08);
    color: inherit;
    cursor: pointer;
  }

  .auto-resume-error {
    margin: 0;
    color: #e8b4b8;
    font-size: 13px;
  }

  @media (prefers-reduced-motion: reduce) {
    input,
    input::after { transition: none; }
  }
</style>
