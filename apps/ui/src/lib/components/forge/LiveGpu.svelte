<script>
  // Live GPU readings against the applied profile: strip charts of the last minute (the monitor,
  // default) or arc gauges of the moment. `target` is the applied profile's clock and voltage cap.
  let { gpu = null, sparks = {}, target = null, view = "monitor", historyCap = 30 } = $props();

  const W = 600;
  const H = 64;

  function num(value) {
    const n = Number(value);
    return value == null || !Number.isFinite(n) ? null : n;
  }

  const whole = (value) => (value == null ? "—" : String(Math.round(value)));

  const clock = $derived(num(gpu?.core_clock_mhz));
  const voltage = $derived(num(gpu?.voltage_mv));
  const power = $derived(num(gpu?.power_w));
  const temperature = $derived(num(gpu?.temperature_c));
  const fan = $derived(num(gpu?.fan_speed_pct));
  const usage = $derived(num(gpu?.utilization_pct));
  const memory = $derived(num(gpu?.memory_clock_mhz));
  const vramTotal = $derived(num(gpu?.vram_total_mb));
  const powerLimit = $derived(num(gpu?.power_limit_w));
  const targetClock = $derived(num(target?.clock));
  const voltageCap = $derived(num(target?.voltage));

  const clockStatus = $derived.by(() => {
    if (clock == null) return { text: "no reading" };
    if (targetClock == null) return { text: "stock curve" };
    if (Math.abs(clock - targetClock) <= 15) return { text: "at target", tone: "ok" };
    if (clock > targetClock) return { text: "above target", tone: "warn" };
    return { text: usage != null && usage < 40 ? "light load" : "below target" };
  });

  const voltageStatus = $derived.by(() => {
    if (voltage == null) return { text: "no reading" };
    if (voltageCap == null) return { text: "stock curve" };
    return voltage <= voltageCap + 6 ? { text: "within cap", tone: "ok" } : { text: "above cap", tone: "warn" };
  });

  const strips = $derived([
    {
      key: "core", label: "Clock", unit: "MHz", value: clock, ref: targetClock, refTone: "copper", minSpan: 60,
      sub: targetClock != null ? `target ${whole(targetClock)}` : "stock curve", status: clockStatus,
    },
    {
      key: "voltage", label: "Voltage", unit: "mV", value: voltage, ref: voltageCap, refTone: "copper", minSpan: 40,
      sub: voltageCap != null ? `cap ${whole(voltageCap)}` : "stock curve", status: voltageStatus,
    },
    {
      key: "power", label: "Power", unit: "W", value: power, ref: powerLimit, refTone: "limit", floor: 0,
      sub: powerLimit != null ? `limit ${whole(powerLimit)} W` : "limit unknown",
      status: { text: power != null && powerLimit ? `${Math.round((power / powerLimit) * 100)}% of limit` : "" },
    },
    {
      key: "temp", label: "Temperature", unit: "°C", value: temperature, ref: null, minSpan: 20,
      sub: fan != null ? `fan ${whole(fan)}%` : "fan unknown",
      status: temperature != null && temperature >= 83 ? { text: "hot", tone: "warn" } : { text: "" },
    },
  ]);

  /** Vertical scale that keeps the reference line in view; power always starts at zero. */
  function scale(values, strip) {
    const all = [...values, strip.ref].filter((value) => value != null && Number.isFinite(value));
    if (!all.length) return null;
    if (strip.floor != null) return { lo: strip.floor, hi: Math.max(...all) * 1.12 || 1 };
    const lo = Math.min(...all);
    const hi = Math.max(...all);
    const span = Math.max(hi - lo, strip.minSpan);
    const mid = (hi + lo) / 2;
    return { lo: mid - span * 0.62, hi: mid + span * 0.62 };
  }

  const yOf = (value, range) => H - 6 - ((value - range.lo) / (range.hi - range.lo)) * (H - 12);

  /** The trace grows from the right edge ("now"), one step per sample of the 60 s history. */
  function trace(strip) {
    const values = (sparks?.[strip.key] ?? []).map(Number).filter(Number.isFinite);
    const range = scale(values, strip);
    if (!range) return null;
    const step = W / Math.max(1, historyCap - 1);
    const points = values.map((value, index) => `${(W - (values.length - 1 - index) * step).toFixed(1)},${yOf(value, range).toFixed(1)}`);
    const last = values.at(-1);
    return {
      points: points.join(" "),
      ref: strip.ref == null ? null : yOf(strip.ref, range),
      now: last == null ? null : (yOf(last, range) / H) * 100,
    };
  }

  // Gauges: a 240° arc with the profile reference as a copper tick.
  const CX = 110;
  const CY = 104;
  const R = 82;

  function arcPoint(fraction, radius = R) {
    const angle = ((210 - 240 * fraction) * Math.PI) / 180;
    return [CX + radius * Math.cos(angle), CY - radius * Math.sin(angle)];
  }

  function arc(from, to) {
    const [x0, y0] = arcPoint(from);
    const [x1, y1] = arcPoint(to);
    return `M${x0.toFixed(1)} ${y0.toFixed(1)} A${R} ${R} 0 ${(to - from) * 240 > 180 ? 1 : 0} 1 ${x1.toFixed(1)} ${y1.toFixed(1)}`;
  }

  const fraction = (value, lo, hi) => Math.min(1, Math.max(0, (value - lo) / (hi - lo)));

  const gauges = $derived([
    {
      label: "Clock", unit: "MHz", value: clock, lo: 0, ref: targetClock,
      hi: Math.ceil(Math.max(num(gpu?.max_core_clock_mhz) ?? 0, (targetClock ?? 0) * 1.1, (clock ?? 0) * 1.05, 2000) / 100) * 100,
      ctx: targetClock != null ? `Target ${whole(targetClock)} MHz` : "Stock curve", status: clockStatus,
    },
    {
      label: "Voltage", unit: "mV", value: voltage, lo: 600, hi: 1100, ref: voltageCap,
      ctx: voltageCap != null ? `Cap ${whole(voltageCap)} mV` : "Stock curve", status: voltageStatus,
    },
    {
      label: "Power", unit: "W", value: power, lo: 0, ref: powerLimit,
      hi: Math.ceil((powerLimit ? powerLimit * 1.1 : Math.max((power ?? 0) * 1.3, 250)) / 10) * 10,
      ctx: powerLimit != null ? `${power != null ? Math.round((power / powerLimit) * 100) : "—"}% of the ${whole(powerLimit)} W limit` : "Limit unknown",
    },
    {
      label: "Temperature", unit: "°C", value: temperature, lo: 30, hi: 95, zone: 83,
      ctx: fan != null ? `Fan ${whole(fan)}%` : "Fan unknown",
    },
  ]);

  function tick(gauge) {
    if (gauge.ref == null) return null;
    const f = fraction(gauge.ref, gauge.lo, gauge.hi);
    const [x0, y0] = arcPoint(f, R - 12);
    const [x1, y1] = arcPoint(f, R + 8);
    return { x0, y0, x1, y1 };
  }

  const vramGb = $derived(vramTotal == null ? null : vramTotal / 1024);
</script>

{#if view === "gauges"}
  <div class="gauges">
    {#each gauges as gauge}
      {@const mark = tick(gauge)}
      {@const [sx, sy] = arcPoint(0)}
      {@const [ex, ey] = arcPoint(1)}
      <div class="gauge">
        <svg viewBox="0 0 220 166" aria-hidden="true">
          <path d={arc(0, 1)} class="gauge-track" />
          {#if gauge.zone}<path d={arc(fraction(gauge.zone, gauge.lo, gauge.hi), 1)} class="gauge-zone" />{/if}
          {#if gauge.value != null && fraction(gauge.value, gauge.lo, gauge.hi) > 0}
            <path d={arc(0, fraction(gauge.value, gauge.lo, gauge.hi))} class="gauge-fill" />
          {/if}
          {#if mark}<line x1={mark.x0} y1={mark.y0} x2={mark.x1} y2={mark.y1} class="gauge-tick" />{/if}
          <text x={CX} y={CY + 4} class="gauge-value">{whole(gauge.value)}</text>
          <text x={CX} y={CY + 27} class="gauge-unit">{gauge.unit}</text>
          <text x={sx} y={sy + 16} class="gauge-end">{gauge.lo}</text>
          <text x={ex} y={ey + 16} class="gauge-end">{gauge.hi}</text>
        </svg>
        <span class="eyebrow">{gauge.label}</span>
        <span class="gauge-ctx">
          {#if gauge.status?.tone === "ok"}<i class="dot" aria-hidden="true"></i>{/if}{gauge.ctx}
        </span>
      </div>
    {/each}
  </div>
{:else}
  <div class="monitor">
    {#each strips as strip}
      {@const line = trace(strip)}
      <div class="strip">
        <div class="strip-label"><span class="eyebrow">{strip.label}</span><span class="strip-sub">{strip.sub}</span></div>
        <div class="strip-trace">
          {#if line}
            <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" aria-hidden="true">
              {#each [1, 2, 3, 4, 5] as k}<line x1={(k * W) / 6} y1="0" x2={(k * W) / 6} y2={H} class="grid" />{/each}
              {#if line.ref != null}<line x1="0" y1={line.ref} x2={W} y2={line.ref} class={`ref ${strip.refTone}`} />{/if}
              <polyline points={line.points} class="trace" />
            </svg>
            {#if line.now != null}<i class="now" style={`top: ${line.now}%`} aria-hidden="true"></i>{/if}
          {/if}
        </div>
        <div class="strip-value">
          <b>{whole(strip.value)}</b><span>{strip.unit}</span>
          <em class={strip.status.tone ?? ""}>{#if strip.status.tone === "ok"}<i class="dot" aria-hidden="true"></i>{/if}{strip.status.text}</em>
        </div>
      </div>
    {/each}
    <div class="monitor-axis" aria-hidden="true">
      <span></span>
      <span class="axis"><span>−60 s</span><span>−40 s</span><span>−20 s</span><span>now</span></span>
      <span></span>
    </div>
  </div>
{/if}

<div class="readouts">
  <span><span class="eyebrow">VRAM</span>{memory != null ? `${whole(memory)} MHz` : "—"}{vramGb != null ? ` · ${vramGb >= 10 ? vramGb.toFixed(0) : vramGb.toFixed(1)} GB` : ""}</span>
  <span><span class="eyebrow">GPU load</span>{usage != null ? `${whole(usage)}%` : "—"}</span>
  <span><span class="eyebrow">Fan</span>{fan != null ? `${whole(fan)}%` : "—"}</span>
</div>

<style>
  .eyebrow {
    color: #a5aaad;
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }

  .dot {
    display: inline-block;
    width: 7px;
    height: 7px;
    margin-right: 6px;
    border-radius: 50%;
    background: #79b52f;
    vertical-align: 1px;
  }

  .monitor {
    background: rgba(2, 4, 5, 0.78);
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.04);
  }

  .strip,
  .monitor-axis {
    display: grid;
    grid-template-columns: 150px minmax(0, 1fr) 190px;
    align-items: center;
  }

  .strip + .strip,
  .monitor-axis {
    border-top: 1px solid rgba(255, 255, 255, 0.06);
  }

  .strip-label {
    display: grid;
    gap: 4px;
    padding: 0 18px;
  }

  .strip-sub,
  .monitor-axis {
    color: #6f7a82;
    font-family: Consolas, "Cascadia Mono", monospace;
    font-size: 12px;
  }

  .strip-trace {
    position: relative;
    height: 64px;
  }

  .strip-trace svg {
    display: block;
    width: 100%;
    height: 100%;
  }

  .grid {
    stroke: rgba(255, 255, 255, 0.05);
    vector-effect: non-scaling-stroke;
  }

  .ref {
    stroke-dasharray: 5 5;
    stroke-width: 1.2;
    vector-effect: non-scaling-stroke;
  }

  .ref.copper { stroke: #c5864f; }
  .ref.limit { stroke: rgba(226, 101, 106, 0.55); }

  .trace {
    fill: none;
    stroke: #b9c4cb;
    stroke-linejoin: round;
    stroke-width: 1.7;
    vector-effect: non-scaling-stroke;
  }

  .now {
    position: absolute;
    right: -3px;
    width: 7px;
    height: 7px;
    margin-top: -3.5px;
    border-radius: 50%;
    background: #e9ba79;
  }

  .strip-value {
    padding: 0 18px;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  .strip-value b {
    color: #f0f0ef;
    font-size: 28px;
    font-weight: 500;
    letter-spacing: -0.01em;
  }

  .strip-value span {
    margin-left: 5px;
    color: #8f979c;
    font-size: 13px;
  }

  .strip-value em {
    display: block;
    min-height: 16px;
    margin-top: 2px;
    color: #8f979c;
    font-size: 12px;
    font-style: normal;
  }

  .strip-value em.ok { color: #9fcf5c; }
  .strip-value em.warn { color: #ebb85a; }

  .monitor-axis {
    padding: 8px 0;
  }

  .axis {
    display: flex;
    justify-content: space-between;
  }

  .gauges {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
  }

  .gauge {
    display: grid;
    min-width: 0;
    justify-items: center;
    border-left: 1px solid rgba(255, 255, 255, 0.1);
    padding: 0 18px;
    text-align: center;
  }

  .gauge:first-child { border-left: 0; }

  .gauge svg {
    display: block;
    width: 100%;
    max-width: 220px;
  }

  .gauge-track,
  .gauge-zone,
  .gauge-fill {
    fill: none;
    stroke-width: 7;
  }

  .gauge-track { stroke: #283035; stroke-linecap: round; }
  .gauge-zone { stroke: rgba(226, 101, 106, 0.45); }
  .gauge-fill { stroke: #c9d2d8; stroke-linecap: round; }
  .gauge-tick { stroke: #e9ba79; stroke-linecap: round; stroke-width: 2.4; }

  .gauge-value {
    fill: #f0f0ef;
    font-size: 34px;
    font-weight: 500;
    text-anchor: middle;
  }

  .gauge-unit {
    fill: #8f979c;
    font-size: 13px;
    text-anchor: middle;
  }

  .gauge-end {
    fill: #6f7a82;
    font-family: Consolas, "Cascadia Mono", monospace;
    font-size: 11px;
    text-anchor: middle;
  }

  .gauge .eyebrow { margin-top: 6px; }

  .gauge-ctx {
    margin-top: 4px;
    color: #8f979c;
    font-size: 13px;
  }

  .readouts {
    display: flex;
    gap: 30px;
    margin-top: 14px;
    color: #b8bbbe;
    font-size: 13px;
  }

  .readouts .eyebrow {
    margin-right: 8px;
    font-size: 11px;
  }
</style>
