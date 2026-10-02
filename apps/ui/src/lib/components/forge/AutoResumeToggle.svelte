<script>
  import { serviceCall } from "../../service.js";

  // Opt-in automatic continuation after a TDR at a step edge. The service persists it across runs.
  let { powerSweep = null, disabled = false } = $props();

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
  <label title="If the GPU driver crashes at a test edge, the installed service resets only the driver, or the run waits for the next Windows start, then the same run continues.">
    <input
      type="checkbox"
      checked={enabled}
      disabled={busy || disabled}
      onchange={(event) => setEnabled(event.currentTarget.checked)}
    />
    <span>Continue automatically after a driver crash</span>
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
    font-size: 0.8rem;
    color: #c9cbc6;
  }

  .auto-resume label {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
  }

  .auto-resume input {
    accent-color: #d6a04c;
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
  }
</style>
