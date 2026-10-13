<script>
  import { FileDown } from "@lucide/svelte";
  import { classifyForgeLogLine } from "../../logTone.js";

  // The Live log page: the Core's English events, each with its tone and Forge phase. A color
  // always comes with its symbol. Older saved runs only have technical lines; they still read.
  let { powerSweep = null, exporting = false, exportMsg = "", exportFailed = false, onExportLog } = $props();

  const GLYPH = { pass: "✓", step: "·", attention: "!", fail: "✕" };
  const FILTERS = [
    { id: "all", label: "All", keep: () => true },
    { id: "results", label: "Results", keep: (entry) => entry.tone === "pass" || entry.tone === "fail" },
    { id: "attention", label: "Attention", keep: (entry) => entry.tone === "attention" || entry.tone === "fail" },
  ];
  const LEGACY_TONE = { good: "pass", bad: "fail", process: "step" };
  const clock = new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit", second: "2-digit", hourCycle: "h23" });

  let filter = $state("all");
  let showDetails = $state(false);

  const entries = $derived.by(() => {
    const structured = powerSweep?.log_entries ?? [];
    if (structured.length) return structured;
    return (powerSweep?.log ?? []).map((text) => ({ tone: LEGACY_TONE[classifyForgeLogLine(text)] ?? "step", phase: "", text }));
  });
  const visible = $derived(entries.filter(FILTERS.find((entry) => entry.id === filter)?.keep ?? (() => true)));
  const running = $derived(Boolean(powerSweep?.running));
  // While a run is live, its newest routine step is what the GPU is doing now.
  const liveIndex = $derived(running && visible.at(-1)?.tone === "step" ? visible.length - 1 : -1);

  function count(id) {
    const keep = FILTERS.find((entry) => entry.id === id)?.keep;
    return keep ? entries.filter(keep).length : 0;
  }

  function time(entry) {
    return entry.at_ms ? clock.format(new Date(entry.at_ms)) : "";
  }

  function autoscroll(node, _dep) {
    const toBottom = () => (node.scrollTop = node.scrollHeight);
    toBottom();
    return { update: toBottom };
  }
</script>

<section class="live-log" aria-labelledby="live-log-title">
  <header class="log-title">
    <div>
      <span class="eyebrow">Real-time activity</span>
      <h1 id="live-log-title" data-forge-heading tabindex="-1">Live log</h1>
    </div>
    <button class="export" type="button" onclick={onExportLog} disabled={exporting || !onExportLog}>
      <FileDown size={16} strokeWidth={1.8} />
      <span>{exporting ? "Exporting…" : "Export full log"}</span>
    </button>
  </header>
  {#if exportMsg}<p class="export-msg" class:failed={exportFailed} role="status">{exportMsg}</p>{/if}

  <div class="toolbar">
    <div class="filters" role="group" aria-label="Show">
      {#each FILTERS as option}
        <button type="button" aria-pressed={filter === option.id} class:on={filter === option.id} onclick={() => (filter = option.id)}>
          {option.label}<small>{count(option.id)}</small>
        </button>
      {/each}
    </div>
    <label class="details-toggle">
      <span>Technical details</span>
      <input type="checkbox" role="switch" bind:checked={showDetails} />
    </label>
  </div>

  <div class="log" role="log" aria-live="polite" use:autoscroll={visible.length}>
    {#each visible as entry, index}
      <div class={`line tone-${entry.tone}`} class:now={index === liveIndex}>
        <span class="time">{time(entry)}</span>
        <span class="glyph" aria-hidden="true">{index === liveIndex ? "›" : GLYPH[entry.tone] ?? "·"}</span>
        <span class="phase">{entry.phase}</span>
        <span class="text">{entry.text}{#if index === liveIndex}<i class="cursor" aria-hidden="true"></i>{/if}</span>
      </div>
      {#if showDetails && entry.detail}
        <div class="line detail"><span></span><span></span><span></span><span class="text">{entry.detail}</span></div>
      {/if}
    {:else}
      <p class="empty">No events yet. Start a Forge to follow each step here.</p>
    {/each}
  </div>

  <div class="legend">
    <span><b class="pass">✓</b> passed</span>
    <span><b>·</b> routine step</span>
    <span><b class="attention">!</b> needs attention</span>
    <span><b class="fail">✕</b> failed</span>
    <span><b class="live">›</b> running now</span>
    <span class="note">Each color always comes with its symbol</span>
  </div>
</section>

<style>
  .live-log {
    display: grid;
    gap: 18px;
    color: #d8dbde;
  }

  .log-title {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 20px;
  }

  .eyebrow {
    color: #a5aaad;
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }

  h1 {
    margin: 6px 0 0;
    color: #f0f0ef;
    font-size: clamp(1.55rem, 2.4vw, 2.15rem);
    font-weight: 580;
    letter-spacing: -0.025em;
  }

  h1:focus { outline: none; }

  .export {
    display: inline-flex;
    min-height: 36px;
    align-items: center;
    gap: 8px;
    border: 1px solid rgba(174, 181, 186, 0.35);
    border-radius: 6px;
    padding: 0 14px;
    background: rgba(10, 12, 13, 0.35);
    color: #c3c8cb;
    font: inherit;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }

  .export:disabled { cursor: not-allowed; opacity: 0.5; }

  .export-msg {
    margin: -6px 0 0;
    color: #9fcf5c;
    font-size: 13px;
  }

  .export-msg.failed { color: #ee7a7e; }

  .toolbar {
    display: flex;
    align-items: flex-end;
    border-bottom: 1px solid rgba(255, 255, 255, 0.12);
  }

  .filters {
    display: flex;
    gap: 6px;
  }

  .filters button {
    position: relative;
    border: 0;
    padding: 6px 12px 10px;
    background: transparent;
    color: #a5a9ae;
    font: inherit;
    font-size: 14px;
    cursor: pointer;
  }

  .filters button small {
    margin-left: 6px;
    color: #6f787e;
    font-size: 12px;
  }

  .filters button.on { color: #e9ba79; }

  .filters button.on::after {
    position: absolute;
    right: 8px;
    bottom: 0;
    left: 8px;
    height: 2px;
    background: #c5864f;
    content: "";
  }

  .details-toggle {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-left: auto;
    padding-bottom: 9px;
    color: #8f979c;
    font-size: 13px;
    cursor: pointer;
  }

  .details-toggle input {
    position: relative;
    width: 38px;
    height: 20px;
    margin: 0;
    appearance: none;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.14);
    cursor: pointer;
  }

  .details-toggle input::after {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: #e8ebe9;
    content: "";
  }

  .details-toggle input:checked { background: #d0a15f; }
  .details-toggle input:checked::after { transform: translateX(18px); }
  .details-toggle input:focus-visible { outline: 2px solid #d0a15f; outline-offset: 2px; }

  .log {
    max-height: calc(100vh - 330px);
    min-height: 260px;
    overflow-y: auto;
    padding: 14px 4px;
    background: rgba(3, 5, 6, 0.72);
  }

  .line {
    display: grid;
    grid-template-columns: 78px 22px 128px minmax(0, 1fr);
    gap: 8px;
    align-items: baseline;
    padding: 5px 18px;
    color: #dfe2e4;
    font-size: 14px;
  }

  .time {
    color: #66717a;
    font-family: Consolas, "Cascadia Mono", monospace;
    font-size: 12.5px;
    font-variant-numeric: tabular-nums;
  }

  .glyph {
    color: #6f787e;
    font-weight: 700;
    text-align: center;
  }

  .phase {
    overflow: hidden;
    color: #8f979c;
    font-size: 12.5px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .text { text-wrap: pretty; }

  .tone-pass .glyph,
  .tone-pass .text { color: #9fcf5c; }

  .tone-attention { background: rgba(227, 168, 59, 0.06); }
  .tone-attention .glyph,
  .tone-attention .text { color: #ebb85a; }

  .tone-fail { background: rgba(226, 84, 90, 0.08); }
  .tone-fail .glyph,
  .tone-fail .text { color: #ee7a7e; }

  .line.now .glyph,
  .line.now .text { color: #e9ba79; }

  .cursor {
    display: inline-block;
    width: 7px;
    height: 14px;
    margin-left: 4px;
    background: #e9ba79;
    vertical-align: -2px;
    animation: blink 1.1s steps(1) infinite;
  }

  @keyframes blink {
    50% { opacity: 0; }
  }

  .detail .text {
    color: #75818a;
    font-family: Consolas, "Cascadia Mono", monospace;
    font-size: 12.5px;
  }

  .empty {
    margin: 0;
    padding: 6px 18px;
    color: #8f979c;
    font-size: 14px;
  }

  .legend {
    display: flex;
    flex-wrap: wrap;
    gap: 22px;
    color: #8f979c;
    font-size: 12.5px;
  }

  .legend b { font-weight: 700; }
  .legend .pass { color: #8fc04a; }
  .legend .attention { color: #e3a83b; }
  .legend .fail { color: #e2656a; }
  .legend .live { color: #e0a96b; }
  .legend .note { margin-left: auto; }

  @media (prefers-reduced-motion: reduce) {
    .cursor { animation: none; }
  }
</style>
