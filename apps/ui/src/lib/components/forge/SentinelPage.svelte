<script>
  import { ShieldCheck } from "@lucide/svelte";
  import { serviceCall } from "../../service.js";

  // The Sentinel page answers three questions: am I protected, what happened, and what is
  // blocked on this GPU. Events come from the Sentinel's last action and the failure history.
  let { sentinel = null, safeLoop = null, powerSweep = null, applied = null, serviceReady = false } = $props();

  const DAY_MS = 24 * 60 * 60 * 1000;
  const dateFormat = new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", hourCycle: "h23" });

  let canaryOverride = $state(null);
  let canarySaving = $state(false);
  let canaryError = $state(null);
  let listOpen = $state(false);
  const canaryOn = $derived(canaryOverride ?? Boolean(powerSweep?.sentinel_canary));

  $effect(() => {
    powerSweep?.sentinel_canary;
    canaryOverride = null;
  });

  async function setCanary(enabled) {
    canarySaving = true;
    canaryOverride = enabled;
    canaryError = null;
    try {
      const response = await serviceCall("SetSentinelCanary", { enabled });
      if (response?.ok === false) throw new Error(response.error ?? "The service refused the change.");
    } catch (error) {
      canaryOverride = null;
      canaryError = `The setting was not saved: ${error?.message ?? error}`;
    } finally {
      canarySaving = false;
    }
  }

  const pair = (clock, voltage) => (clock != null && voltage != null ? `${clock} MHz and ${voltage} mV` : null);
  const time = (value) => {
    const date = new Date(value ?? "");
    return Number.isNaN(date.getTime()) ? null : date;
  };

  /** The Sentinel's last action, told the way a player would. */
  function sentinelEvent(status) {
    if (!status) return null;
    const where = pair(status.target_mhz, status.failed_mv);
    if (status.action === "bump") {
      return { tone: "attention", title: "Sentinel raised the voltage after an error", body: `${where ? `At ${where}. ` : ""}It moved to ${status.new_mv} mV at the same clock.` };
    }
    if (status.event === "tdr" || String(status.action ?? "").includes("reboot")) {
      return { tone: "fail", title: "Driver crash while a profile was applied", body: `${where ? `At ${where}. ` : ""}Sentinel returned the GPU to stock. Restart Windows before tuning again.` };
    }
    return { tone: "fail", title: "The GPU check found an error while you played", body: `${where ? `At ${where}. ` : ""}Sentinel returned the GPU to stock.${status.suggested_mv ? ` A new Forge can qualify ${status.suggested_mv} mV.` : ""}` };
  }

  /** One failure-history record as an event. */
  function ledgerEvent(record) {
    const where = pair(record.target_mhz, record.vf_bin_mv);
    const kinds = {
      "field-tdr": { tone: "fail", title: "Driver crash while playing", body: `${where ? `At ${where}. ` : ""}Sentinel returned the GPU to stock and blocked that point.` },
      "candidate-crash": { tone: "fail", title: "Driver crash during a Forge test", body: `${where ? `At ${where}. ` : ""}The Forge blocked that point.` },
      "apply-gate-silent-error": { tone: "fail", title: "Integrity error in a final check", body: `${where ? `At ${where}. ` : ""}The point was blocked before it could become a profile.` },
      rehabilitated: { tone: "pass", title: "A blocked point was released", body: `${where ? `${where.replace(" and ", " at ")} ` : "It "}can be tested again.` },
    };
    const known = kinds[record.kind];
    if (known) return known;
    return { tone: "attention", title: "A point was blocked", body: `${where ? `At ${where}. ` : ""}${String(record.kind ?? "stability evidence").replaceAll("-", " ")}.` };
  }

  const events = $derived.by(() => {
    const list = [];
    const last = sentinelEvent(sentinel);
    if (last) list.push({ ...last, at: time(sentinel.ts) });
    for (const record of safeLoop?.condemnations ?? []) list.push({ ...ledgerEvent(record), at: time(record.timestamp) });
    return list.sort((a, b) => (b.at?.getTime() ?? 0) - (a.at?.getTime() ?? 0)).slice(0, 8);
  });

  const lastEventDays = $derived(events[0]?.at ? Math.floor((Date.now() - events[0].at.getTime()) / DAY_MS) : null);
  const statusLine = $derived(
    !events.length
      ? "No events recorded"
      : lastEventDays === 0
        ? `Last event today`
        : `No events in the last ${lastEventDays} day${lastEventDays === 1 ? "" : "s"}`,
  );

  const profileName = $derived(applied?.core ? applied.label ?? "your profile" : null);

  // Blocked points: active failure records plus blacklisted regions with a clock and a voltage.
  const blocked = $derived.by(() => {
    const points = (safeLoop?.condemnations ?? [])
      .filter((record) => !record.rehabilitated && record.target_mhz != null && record.vf_bin_mv != null)
      .map((record) => ({ clock: record.target_mhz, voltage: record.vf_bin_mv }));
    for (const region of safeLoop?.blacklist ?? []) {
      const axes = region?.center?.axes ?? {};
      if (axes.gpu_freq_mhz != null && axes.gpu_vf_bin_mv != null) points.push({ clock: axes.gpu_freq_mhz, voltage: axes.gpu_vf_bin_mv });
    }
    const seen = new Set();
    return points.filter((point) => {
      const key = `${point.clock}@${point.voltage}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    });
  });

  const profilePoints = $derived(
    [["Godforge", "godforge"], ["Brokkr’s Best", "brokkrs"], ["Deep Calm", "deep_calm"]]
      .map(([name, key]) => {
        const point = powerSweep?.[key];
        const clock = point?.target_clock_mhz ?? point?.clock_mhz;
        const voltage = point?.vf_table_voltage_mv ?? point?.voltage_mv;
        return clock != null && voltage != null ? { name, clock, voltage } : null;
      })
      .filter(Boolean),
  );

  const W = 640;
  const H = 290;
  const PLOT = { left: 56, right: 630, top: 14, bottom: 246 };

  const scale = $derived.by(() => {
    const all = [...blocked, ...profilePoints];
    if (!all.length) return null;
    const clocks = all.map((point) => point.clock);
    const voltages = all.map((point) => point.voltage);
    const pad = (lo, hi, min) => {
      const span = Math.max(hi - lo, min);
      const mid = (lo + hi) / 2;
      return [Math.floor((mid - span * 0.6) / 25) * 25, Math.ceil((mid + span * 0.6) / 25) * 25];
    };
    const [clockLo, clockHi] = pad(Math.min(...clocks), Math.max(...clocks), 150);
    const [voltLo, voltHi] = pad(Math.min(...voltages), Math.max(...voltages), 100);
    return { clockLo, clockHi, voltLo, voltHi };
  });

  const x = (clock) => PLOT.left + ((clock - scale.clockLo) / (scale.clockHi - scale.clockLo)) * (PLOT.right - PLOT.left);
  const y = (voltage) => PLOT.top + ((scale.voltHi - voltage) / (scale.voltHi - scale.voltLo)) * (PLOT.bottom - PLOT.top);

  const overlaps = (a, b) => a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;

  /** Profile labels take the first side (right, left, above, below) that touches no other mark. */
  const labels = $derived.by(() => {
    if (!scale) return [];
    const marks = [...blocked, ...profilePoints].map((point) => ({ x: x(point.clock) - 7, y: y(point.voltage) - 7, w: 14, h: 14 }));
    const placed = [];
    return profilePoints.map((point) => {
      const cx = x(point.clock);
      const cy = y(point.voltage);
      const w = point.name.length * 7 + 4;
      const h = 14;
      const sides = [
        { x: cx + 11, y: cy - 7, anchor: "start" },
        { x: cx - 11 - w, y: cy - 7, anchor: "end" },
        { x: cx - w / 2, y: cy - 25, anchor: "middle" },
        { x: cx - w / 2, y: cy + 11, anchor: "middle" },
      ];
      const fits = (side) => side.x >= 0 && side.x + w <= W && side.y >= 0 && ![...marks, ...placed].some((box) => overlaps(box, { x: side.x, y: side.y, w, h }));
      const side = sides.find(fits) ?? sides[0];
      placed.push({ x: side.x, y: side.y, w, h });
      const tx = side.anchor === "start" ? side.x : side.anchor === "end" ? side.x + w : side.x + w / 2;
      return { name: point.name, x: tx, y: side.y + 11, anchor: side.anchor };
    });
  });
</script>

<section class="sentinel" aria-labelledby="sentinel-title">
  <header class="page-title">
    <span class="eyebrow">Automatic protection</span>
    <h1 id="sentinel-title" data-forge-heading tabindex="-1">Sentinel</h1>
    <div class="watching" class:standby={!profileName}>
      <ShieldCheck size={34} strokeWidth={1.8} />
      <div>
        <strong>{profileName ? `Watching ${profileName}` : "Standing by"}</strong>
        <span>{profileName ? statusLine : "No profile is applied; the GPU runs at stock."}</span>
      </div>
    </div>
  </header>

  <section class="block" aria-labelledby="protection-title">
    <h2 id="protection-title">Protection</h2>
    <div class="layers">
      <div class="layer">
        <span class="eyebrow">Driver crash watch</span>
        <strong class="on">Always on</strong>
        <p>Reads Windows driver events every second while a profile is applied. It puts no load on the GPU.</p>
      </div>
      <div class="layer">
        <span class="eyebrow">GPU check while gaming</span>
        <div class="layer-state">
          <strong class:on={canaryOn}>{canaryOn ? "On" : "Off"}</strong>
          <button class="layer-button" type="button" onclick={() => setCanary(!canaryOn)} disabled={canarySaving || !serviceReady}>{canaryOn ? "Turn off" : "Turn on"}</button>
        </div>
        <p>Tests the GPU for a moment every 20 s while you play, about 3.5% of GPU time. It catches silent errors before they show up as artifacts.</p>
        {#if canaryError}<p class="error" role="alert">{canaryError}</p>{/if}
      </div>
    </div>
  </section>

  <section class="block" aria-labelledby="events-title">
    <h2 id="events-title">What happened</h2>
    {#if events.length}
      <ol class="events">
        {#each events as event}
          <li>
            <span class="eyebrow when">{event.at ? dateFormat.format(event.at) : "—"}</span>
            <span class={`mark ${event.tone}`} aria-hidden="true">{event.tone === "pass" ? "✓" : event.tone === "attention" ? "!" : "✕"}</span>
            <div><strong>{event.title}</strong><p>{event.body}</p></div>
          </li>
        {/each}
      </ol>
    {:else}
      <p class="quiet">Nothing has happened yet. Sentinel records what it does here.</p>
    {/if}
  </section>

  <section class="block" aria-labelledby="blocked-title">
    <div class="block-head">
      <h2 id="blocked-title">Blocked on this GPU</h2>
      <span class="note">Full Reset clears this list</span>
      {#if blocked.length}
        <button class="link" type="button" aria-expanded={listOpen} onclick={() => (listOpen = !listOpen)}>{listOpen ? "Hide the list" : "Show the list"} ›</button>
      {/if}
    </div>
    {#if scale && blocked.length}
      <div class="map">
        <svg viewBox={`0 0 ${W} ${H}`} role="img" aria-label="Blocked clock and voltage points next to your profiles">
          <line x1={PLOT.left} y1={PLOT.top} x2={PLOT.left} y2={PLOT.bottom} class="axis" />
          <line x1={PLOT.left} y1={PLOT.bottom} x2={PLOT.right} y2={PLOT.bottom} class="axis" />
          <text x="6" y={PLOT.top + 4} class="tick">{scale.voltHi} mV</text>
          <text x="6" y={PLOT.bottom + 4} class="tick">{scale.voltLo}</text>
          <text x={PLOT.left} y={PLOT.bottom + 22} class="tick">{scale.clockLo} MHz</text>
          <text x={PLOT.right} y={PLOT.bottom + 22} class="tick end">{scale.clockHi}</text>
          {#each blocked as point}
            <path d={`M${x(point.clock) - 5} ${y(point.voltage) - 5} l10 10 m0 -10 l-10 10`} class="cross" />
          {/each}
          {#each profilePoints as point}
            <circle cx={x(point.clock)} cy={y(point.voltage)} r="6" class="profile" />
          {/each}
          {#each labels as label}
            <text x={label.x} y={label.y} text-anchor={label.anchor} class="profile-label">{label.name}</text>
          {/each}
        </svg>
        <div class="map-side">
          <strong>{blocked.length} point{blocked.length === 1 ? "" : "s"}</strong>
          <p>failed a test or crashed. The Forge and your profiles never use them again.</p>
          <div class="legend"><span><b class="fail">✕</b> blocked point</span><span><b class="copper">●</b> your profiles</span></div>
        </div>
      </div>
      {#if listOpen}
        <ul class="blocked-list">
          {#each blocked as point}<li>{point.clock} MHz at {point.voltage} mV</li>{/each}
        </ul>
      {/if}
    {:else}
      <p class="quiet">Nothing is blocked on this GPU.</p>
    {/if}
  </section>
</section>

<style>
  .sentinel {
    display: grid;
    gap: 34px;
    color: #d8dbde;
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

  .watching {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-top: 14px;
    color: #8fc04a;
  }

  .watching.standby { color: #8f979c; }

  .watching strong {
    display: block;
    font-size: 20px;
    font-weight: 500;
  }

  .watching span {
    color: #8f979c;
    font-size: 13.5px;
  }

  .block h2,
  .block-head h2 {
    margin: 0 0 16px;
    color: #e9ba79;
    font-size: 13px;
    font-weight: 700;
    letter-spacing: 0.14em;
    text-transform: uppercase;
  }

  /* Full-width bands, as on the Forge screen: sections stay distinct, their items stay seamless. */
  .block {
    margin-inline: calc(-1 * var(--page-gutter, 36px));
    border-top: 1px solid rgba(164, 171, 177, 0.3);
    padding: 22px var(--page-gutter, 36px) 28px;
    background: rgba(5, 8, 9, 0.5);
  }

  .block + .block { margin-top: -18px; }

  .layers {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .layer {
    min-width: 0;
    border-left: 1px solid rgba(255, 255, 255, 0.1);
    padding: 2px 24px 4px;
  }

  .layer:first-child {
    border-left: 0;
    padding-left: 0;
  }

  .layer strong {
    display: block;
    margin-top: 6px;
    color: #8f979c;
    font-size: 20px;
    font-weight: 500;
  }

  .layer strong.on { color: #8fc04a; }

  .layer p {
    max-width: 46ch;
    margin: 6px 0 0;
    color: #8f979c;
    font-size: 13.5px;
    line-height: 1.45;
    text-wrap: pretty;
  }

  .layer p.error { color: #ee7a7e; }

  .layer-state {
    display: flex;
    align-items: center;
    gap: 16px;
  }

  .layer-button {
    min-height: 34px;
    margin-top: 6px;
    border: 1px solid rgba(197, 134, 79, 0.6);
    border-radius: 6px;
    padding: 0 14px;
    background: rgba(10, 12, 13, 0.35);
    color: #e9ba79;
    font: inherit;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }

  .layer-button:disabled { cursor: not-allowed; opacity: 0.5; }

  .events {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .events li {
    display: grid;
    grid-template-columns: 140px 22px minmax(0, 1fr);
    gap: 10px;
    align-items: baseline;
    border-top: 1px solid rgba(255, 255, 255, 0.08);
    padding: 13px 0;
  }

  .events li:first-child {
    border-top: 0;
    padding-top: 2px;
  }

  .events strong {
    color: #e2e4e5;
    font-size: 15px;
    font-weight: 500;
  }

  .events p {
    margin: 3px 0 0;
    color: #8f979c;
    font-size: 13.5px;
    line-height: 1.45;
  }

  .mark { font-weight: 700; }
  .mark.pass { color: #8fc04a; }
  .mark.attention { color: #e3a83b; }
  .mark.fail { color: #e2656a; }

  .quiet {
    margin: 0;
    color: #8f979c;
    font-size: 14px;
  }

  .block-head {
    display: flex;
    align-items: baseline;
    gap: 18px;
  }

  .note {
    margin-left: auto;
    color: #8f979c;
    font-size: 13px;
  }

  .link {
    border: 0;
    padding: 0;
    background: transparent;
    color: #e0a96b;
    font: inherit;
    font-size: 13px;
    cursor: pointer;
  }

  .map {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 280px;
    gap: 40px;
    align-items: center;
  }

  .map svg {
    display: block;
    width: 100%;
  }

  .axis { stroke: rgba(255, 255, 255, 0.12); }

  .tick {
    fill: #7d878d;
    font-size: 12px;
  }

  .tick.end { text-anchor: end; }

  .cross {
    stroke: #e2656a;
    stroke-linecap: round;
    stroke-width: 2.2;
  }

  .profile { fill: #c5864f; }

  .profile-label {
    fill: #e0a96b;
    font-size: 12px;
  }

  .map-side strong {
    color: #f0f0ef;
    font-size: 34px;
    font-weight: 500;
  }

  .map-side p {
    margin: 8px 0 0;
    color: #8f979c;
    font-size: 14px;
    line-height: 1.5;
  }

  .legend {
    display: flex;
    gap: 20px;
    margin-top: 16px;
    color: #8f979c;
    font-size: 13px;
  }

  .legend .fail { color: #e2656a; }
  .legend .copper { color: #c5864f; }

  .blocked-list {
    display: flex;
    flex-wrap: wrap;
    gap: 8px 24px;
    margin: 16px 0 0;
    padding: 0;
    list-style: none;
    color: #b8bbbe;
    font-size: 13.5px;
    font-variant-numeric: tabular-nums;
  }
</style>
