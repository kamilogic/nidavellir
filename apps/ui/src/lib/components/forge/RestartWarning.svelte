<script>
  import commandMark from "../../assets/themes/nidavellir-mark.png";
  import { serviceCall } from "../../service.js";

  // The Core restarts Windows when a driver crash needs a fresh boot and the driver-only restart
  // could not give one (user decision 2026-10-13). Unanswered, the restart happens; Cancel keeps
  // the PC running and the run waits for the user. Escape never dismisses it.
  let { powerSweep = null } = $props();

  let dialog = $state(null);
  let now = $state(Date.now());
  let cancelling = $state(false);
  let cancelledAt = $state(null);
  let error = $state(null);
  const at = $derived(powerSweep?.auto_restart_at_ms ?? null);
  const showing = $derived(Boolean(at) && at !== cancelledAt);
  const seconds = $derived(at ? Math.max(0, Math.ceil((at - now) / 1000)) : 0);
  const clock = $derived(`${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`);

  $effect(() => {
    if (!dialog) return;
    if (showing && !dialog.open) {
      error = null;
      dialog.showModal();
    } else if (!showing && dialog.open) {
      dialog.close();
    }
  });

  $effect(() => {
    if (!showing) return;
    now = Date.now();
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });

  async function cancelRestart() {
    cancelling = true;
    error = null;
    try {
      const response = await serviceCall("CancelForgeAutoContinue");
      if (response?.ok === false) throw new Error(response.error ?? "The service refused to cancel.");
      cancelledAt = at;
    } catch (e) {
      error = String(e?.message ?? e);
    } finally {
      cancelling = false;
    }
  }
</script>

<dialog
  bind:this={dialog}
  class="restart"
  aria-labelledby="restart-title"
  aria-describedby="restart-lead"
  oncancel={(event) => event.preventDefault()}
>
  <div class="mark"><img src={commandMark} alt="" /><span>Forge</span></div>
  <h2 id="restart-title">Windows restarts in <span class="clock">{clock}</span></h2>
  <p id="restart-lead" class="lead">
    A driver crash needs a fresh Windows start. Nidavellir restarts Windows by itself, and the run
    continues after the restart.
  </p>
  <p class="warn">Save your work now: open programs close without saving.</p>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  <div class="actions">
    <button type="button" class="cancel" onclick={cancelRestart} disabled={cancelling}>
      {cancelling ? "Cancelling…" : "Cancel restart"}
    </button>
  </div>
</dialog>

<style>
  .restart {
    width: min(520px, calc(100vw - 48px));
    border: 1px solid rgba(197, 134, 79, 0.45);
    border-radius: 12px;
    padding: 28px 32px 26px;
    background: rgba(11, 14, 15, 0.98);
    color: #d8dbde;
    box-shadow: 0 30px 80px rgba(0, 0, 0, 0.55);
    font-family: "Segoe UI", system-ui, sans-serif;
  }

  .restart::backdrop {
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
    margin: 10px 0 6px;
    color: #f0f0ef;
    font-size: 26px;
    font-weight: 540;
    letter-spacing: -0.01em;
  }

  .clock {
    color: #e9ba79;
    font-variant-numeric: tabular-nums;
  }

  .lead {
    margin: 0;
    color: #c3c8cb;
    font-size: 14px;
    line-height: 1.5;
    text-wrap: pretty;
  }

  .warn {
    margin: 14px 0 0;
    color: #ebb85a;
    font-size: 14px;
    font-weight: 600;
  }

  .error {
    margin: 12px 0 0;
    color: #f0b8bc;
    font-size: 13px;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    margin-top: 24px;
  }

  .cancel {
    min-height: 40px;
    border: 1px solid rgba(197, 134, 79, 0.6);
    border-radius: 6px;
    padding: 0 18px;
    background: rgba(10, 12, 13, 0.35);
    color: #e9ba79;
    font: inherit;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
  }

  .cancel:hover:not(:disabled) {
    background: rgba(197, 134, 79, 0.12);
  }

  .cancel:focus-visible {
    outline: 2px solid #c5864f;
    outline-offset: 2px;
  }

  .cancel:disabled {
    cursor: wait;
    opacity: 0.6;
  }
</style>
