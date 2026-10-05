<script>
  import { serviceCall } from "../../service.js";

  // Shown only while the Sentinel's GPU check is off and the service says something calls for it.
  let { powerSweep = null, disabled = false } = $props();

  let busy = $state(false);
  let turnedOn = $state(false);
  let error = $state(null);
  const advice = $derived(powerSweep?.sentinel_canary || turnedOn ? null : powerSweep?.sentinel_advice);

  $effect(() => {
    powerSweep?.sentinel_canary;
    turnedOn = false;
  });

  async function turnOn() {
    busy = true;
    error = null;
    try {
      const response = await serviceCall("SetSentinelCanary", { enabled: true });
      if (response?.ok === false) throw new Error(response.error ?? "The service refused the change.");
      turnedOn = true;
    } catch (e) {
      error = String(e?.message ?? e);
    } finally {
      busy = false;
    }
  }
</script>

{#if advice}
  <div class="sentinel-advice" role="status">
    <p><strong>{advice}</strong></p>
    <p>The check runs a short GPU self-test every 20 s under load and can cause brief stutters in games. Turn it off in Settings when the analysis is done.</p>
    <button type="button" onclick={turnOn} disabled={busy || disabled}>Turn on the GPU check</button>
    {#if error}<p class="sentinel-advice-error" role="alert">{error}</p>{/if}
  </div>
{/if}

<style>
  .sentinel-advice {
    display: grid;
    gap: 6px;
    max-width: 560px;
    border-radius: 9px;
    padding: 8px 12px;
    font-size: 0.8rem;
    color: #c9cbc6;
    background: rgba(214, 160, 76, 0.08);
    box-shadow: inset 0 0 0 1px rgba(214, 160, 76, 0.3);
  }

  .sentinel-advice p {
    margin: 0;
  }

  .sentinel-advice button {
    justify-self: start;
    min-height: 0;
    border: 0;
    border-radius: 7px;
    padding: 5px 12px;
    background: rgba(214, 160, 76, 0.18);
    color: inherit;
    cursor: pointer;
  }

  .sentinel-advice-error {
    color: #e8b4b8;
  }
</style>
