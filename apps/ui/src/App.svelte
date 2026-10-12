<script>
  import Onboarding from "./lib/views/Onboarding.svelte";
  import Forge from "./lib/views/Forge.svelte";
  import TrayBridge from "./lib/components/forge/TrayBridge.svelte";
  import WhatsNew from "./lib/components/forge/WhatsNew.svelte";
  import "./lib/theme.css";

  function initialOnboarded() {
    if (typeof window !== "undefined" && !("__TAURI_INTERNALS__" in window)) return true;
    try {
      return localStorage.getItem("nidavellir-gpu-onboarded") === "true";
    } catch {
      return false;
    }
  }

  let onboarded = $state(initialOnboarded());

  function finishOnboarding(_goal) {
    onboarded = true;
    try {
      localStorage.setItem("nidavellir-gpu-onboarded", "true");
    } catch {}
  }
</script>

<main>
  <TrayBridge />
  <WhatsNew />
  {#if !onboarded}
    <Onboarding onComplete={finishOnboarding} />
  {:else}
    <Forge />
  {/if}
</main>

<style>
  :global(:root) {
    --forge-void: #0b0f14;
    --forge-night: #0b0f14;
    --forge-iron: #10161e;
    --forge-panel: #141b24;
    --forge-panel-raised: #1b232e;
    --forge-graphite: #222c38;
    --forge-line: rgba(189, 166, 126, 0.18);
    --forge-line-strong: rgba(214, 168, 93, 0.34);
    --forge-border-neutral: #2a3441;
    --forge-text: #e8ecf1;
    --forge-muted: #8a96a3;
    --forge-dim: #5c6774;
    --forge-steel: #9caabd;
    --forge-blue: #7eadbe;
    --forge-green: #33c481;
    --forge-green-bg: #1f4436;
    --forge-gold: #e3a83b;
    --forge-copper: #b9754b;
    --forge-red: #e2545a;
    --forge-red-bg: #3a2226;
    --forge-teal: #3fd0c9;
    --forge-ink: #0b0f14;
    --forge-panel-bg: var(--forge-panel);
    --forge-panel-edge: inset 0 0 0 0 transparent;
    --forge-shadow-panel: none;
    --forge-shadow-active: 0 0 0 1px rgba(227, 168, 59, 0.3);

    --nord-void: var(--forge-void);
    --nord-night: var(--forge-night);
    --nord-deep: var(--forge-iron);
    --nord-surface: var(--forge-panel);
    --nord-surface-2: var(--forge-panel-raised);
    --nord-raised: var(--forge-graphite);
    --nord-border: var(--forge-line);
    --nord-border-card: var(--forge-line);
    --nord-frost: var(--forge-blue);
    --nord-frost-bright: var(--forge-blue);
    --nord-frost-dim: var(--forge-dim);
    --nord-silver: var(--forge-text);
    --nord-mist: var(--forge-muted);
    --nord-dim: var(--forge-dim);
    --nord-aurora: var(--forge-green);
    --nord-aurora-glow: rgba(51, 196, 129, 0.22);
    --nord-ember: var(--forge-copper);
    --nord-ember-bright: var(--forge-gold);
    --nord-twilight: rgba(185, 117, 75, 0.08);
    --nord-danger: var(--forge-red);
  }

  :global(body) {
    margin: 0;
    min-height: 100vh;
    font-family: "Source Sans Pro", Inter, "Segoe UI", system-ui, sans-serif;
    color: var(--nord-silver);
    background-color: var(--nord-void);
  }

  :global(html) {
    -webkit-font-smoothing: antialiased;
    -moz-osx-font-smoothing: grayscale;
  }

  :global(h1, h2, h3, h4) {
    text-wrap: balance;
  }

  :global(p, li, small) {
    text-wrap: pretty;
  }

  :global(button) {
    min-height: 2.5rem;
    transition-property: background-color, border-color, color, box-shadow, scale;
    transition-duration: 150ms;
    transition-timing-function: ease-out;
  }

  :global(button:active:not(:disabled)) {
    scale: 0.96;
  }

  :global(button:focus-visible),
  :global(a:focus-visible),
  :global(summary:focus-visible),
  :global(select:focus-visible),
  :global(input:focus-visible) {
    outline: 2px solid rgba(214, 168, 93, 0.78);
    outline-offset: 3px;
  }

  main {
    width: 100%;
    margin: 0;
    padding: 0;
  }
</style>
