<script>
  import {
    Activity,
    ChevronDown,
    ChevronRight,
    CircleGauge,
    Cpu,
    Fan,
    Feather,
    Gauge,
    Hammer,
    ShieldCheck,
    Star,
    Thermometer,
    Trash2,
    TriangleAlert,
    X,
    Zap,
  } from "@lucide/svelte";
  import ForgeSettingsPage from "./ForgeSettingsPage.svelte";
  import SentinelAdvice from "./SentinelAdvice.svelte";
  import ForgeProgress from "./ForgeProgress.svelte";
  import AutoResumeToggle from "./AutoResumeToggle.svelte";
  import { distinctForgeProfiles, forgePrimaryAction, nvidiaGpu } from "../../forge-workflow.js";
  import TelemetrySpark from "./TelemetrySpark.svelte";
  import commandMark from "../../assets/themes/nidavellir-mark.png";
  import copperPlate from "../../assets/themes/copper-plate.png";
  import forgeTexture from "../../assets/themes/forge-texture.png";

  let {
    children,
    activeView = "forge",
    hardware = null,
    gpu = null,
    sparks = null,
    powerSweep = null,
    safeLoop = null,
    applied = null,
    forgeMode = "standard",
    powerRunning = false,
    fullResetBusy = false,
    actionBusy = false,
    fullResetFeedback = null,
    error = null,
    serviceStatus = "connecting",
    serviceError = null,
    hardwareError = null,
    exporting = false,
    exportMsg = "",
    exportFailed = false,
    onExportLog,
    onViewSafetyHistory,
    onForgeModeChange,
    onStartPower,
    onRecoverContinue,
    onStopPower,
    onResumePower,
    onApplyPower,
    onReportProfileUnstable,
    onFullReset,
    onReset,
    onDismissError,
    onDismissFullResetFeedback,
    onViewChange,
  } = $props();

  let resetConfirmOpen = $state(false);
  let resetMode = $state("full");
  let resetDialog = $state(null);
  let resetTrigger = $state(null);
  let resetCancelButton = $state(null);
  let expandedProfile = $state(null);
  let safetyDetailsOpen = $state(false);
  let safetyDetails = $state(null);
  let progressDetailsOpen = $state(false);

  const profileMeta = [
    {
      key: "godforge",
      name: "Godforge",
      line: "Maximum sustainable performance",
      summary: "Forged at the edge of sustainable performance for the highest clock this GPU proved it can hold.",
    },
    {
      key: "brokkrs",
      name: "Brokkr’s Best",
      line: "Balanced daily performance",
      summary: "Recommended for daily use, balancing sustained performance with lower measured power.",
    },
    {
      key: "deep_calm",
      name: "Deep Calm",
      line: "Maximum efficiency",
      summary: "Refined for cool, quiet efficiency and the most useful performance from every watt.",
    },
  ];

  const primaryGpu = $derived(nvidiaGpu(hardware));
  const gpuName = $derived.by(() => {
    const detected = String(primaryGpu?.model ?? gpu?.name ?? "").trim();
    return detected || "NVIDIA GPU";
  });
  const gpuDetected = $derived(Boolean(primaryGpu));
  const serviceReady = $derived(serviceStatus === "online");
  const serviceLabel = $derived(serviceReady ? "Online" : serviceStatus === "offline" ? "Offline" : "Connecting");
  const gpuConnectionLabel = $derived(gpuDetected ? "Detected" : serviceReady ? "Waiting" : "Unavailable");
  const hasCompleteProfileSet = $derived(
    Boolean(powerSweep?.godforge && powerSweep?.brokkrs && powerSweep?.deep_calm),
  );
  const isUndervolt = $derived(Boolean(powerSweep?.is_undervolt));
  const profilesReady = $derived(
    Boolean(
      hasCompleteProfileSet &&
        !powerRunning &&
        (powerSweep?.frontier_complete || !powerSweep?.is_undervolt),
    ),
  );
  const profilesQualified = $derived(
    Boolean(profilesReady && (!isUndervolt || powerSweep?.profiles_qualified)),
  );
  const displayedProfiles = $derived(profilesReady ? distinctForgeProfiles(profileMeta, powerSweep) : profileMeta);
  const censoredClocks = $derived((powerSweep?.clock_search ?? []).filter((clock) => clock.censored_floor_mv != null).length);
  // Three distinct qualified profiles need no banner; only the exceptions are worth reading.
  const profileCaveat = $derived(
    displayedProfiles.length < 3 || !profilesQualified || censoredClocks > 0 ||
      (!powerSweep?.discovery_search && !powerSweep?.profile_search_complete),
  );
  const hasForgeRun = $derived(Boolean(powerSweep && powerSweep.phase !== "idle"));
  // Goal placeholders explain a first run; once a run exists only real profiles are worth the space.
  const showProfiles = $derived(profilesReady || !hasForgeRun);
  const forgePaused = $derived(powerSweep?.phase === "paused");
  const rebootRequired = $derived(Boolean(safeLoop?.gpu_reboot_required));
  const forgeBlocked = $derived(Boolean(powerSweep?.start_block_reason));
  // A running Forge arms the boot flag for every candidate by design, and so does an apply for its
  // 8-s survival window; only a leftover flag needs review.
  const verifying = $derived(Boolean(safeLoop?.boot_flag_armed && safeLoop?.survival_window && !powerRunning));
  const safetyNeedsAttention = $derived(
    Boolean(safeLoop?.safe_mode || safeLoop?.state === "unstable" || (safeLoop?.boot_flag_armed && !powerRunning && !verifying) || safeLoop?.recovery_pending_ack),
  );
  const runFinished = $derived(powerSweep?.phase === "finished");
  const runNeedsAttention = $derived(
    ["needs_attention", "incomplete", "field_rejected", "interrupted", "provisional"].includes(powerSweep?.phase),
  );
  const progressPriority = $derived(Boolean(powerRunning || forgePaused || runNeedsAttention || safetyNeedsAttention));
  const state = $derived.by(() => {
    if (serviceStatus === "connecting") return "CONNECTING";
    if (!serviceReady) return "OFFLINE";
    if (!gpuDetected) return "WAITING";
    if (rebootRequired || safetyNeedsAttention || runNeedsAttention || forgeBlocked) return "ATTENTION";
    if (powerRunning) return "FORGING";
    if (profilesReady && profileMeta.some((profile) => appliedMatches(profile))) return "FORGED";
    if (profilesReady) return "REFINED";
    return "RAW";
  });
  const activeKey = $derived.by(() => {
    return profileMeta.find((profile) => appliedMatches(profile))?.key ?? null;
  });
  const activeName = $derived(profileMeta.find((item) => item.key === activeKey)?.name ?? "Stock");
  const safeLoopKnown = $derived(Boolean(safeLoop));
  const recoveryPending = $derived(Boolean(safeLoop?.recovery_pending_ack));
  const protectedState = $derived(
    Boolean(safeLoopKnown && !rebootRequired && !safetyNeedsAttention),
  );
  const protectionLabel = $derived(
    !safeLoopKnown
      ? "Awaiting"
      : rebootRequired
        ? "Restart Windows"
        : recoveryPending
          ? "Recovery needed"
          : protectedState
            ? (verifying ? "Verifying" : "Protected")
            : "Needs attention",
  );
  const protectionMessage = $derived(
    !safeLoopKnown
      ? "Waiting for Safe Loop status."
      : rebootRequired
        ? "The GPU driver recovered. Restart Windows once before any tuning action."
      : recoveryPending
        ? "A previous Forge was interrupted. Recover Forge to return to stock and check whether that run can continue. Safety history is saved."
      : protectedState
        ? (verifying ? "Profile applied. Safe Loop is watching its first seconds."
          : forgeBlocked ? "Safe Loop recovery is clear. Automatic tuning is blocked separately; review the reason below." : "Your GPU is monitored and ready.")
        : "Choose Return to stock to clear the active recovery state. If an incident remains pending, choose Recover Forge to acknowledge it. Safety history stays preserved.",
  );
  const primaryAction = $derived(forgePrimaryAction({
    serviceStatus, gpuDetected, safeLoop, powerSweep,
    busy: actionBusy || fullResetBusy, hasProfiles: profilesReady,
  }));
  const primaryActionDisabled = $derived(primaryAction.disabled);
  const runModeDisabled = $derived(
    Boolean(actionBusy || fullResetBusy || powerRunning || forgePaused || recoveryPending || forgeBlocked || !serviceReady || !gpuDetected || !safeLoopKnown || rebootRequired),
  );
  const actionLabel = $derived(primaryAction.label);
  const primaryActionReason = $derived(primaryAction.reason);
  const heroMessage = $derived.by(() => {
    if (state === "OFFLINE") return "Core Service is unavailable. No GPU action can start.";
    if (state === "CONNECTING") return "Connecting to the protected local Core Service.";
    if (state === "WAITING") return "Waiting for the local NVIDIA GPU to be identified.";
    if (powerSweep?.start_block_reason) return powerSweep.start_block_reason;
    // The Core's own note is technical; it stays readable under Run details.
    if (state === "ATTENTION") {
      if (rebootRequired) return "A driver crash stopped the test. Progress and safety history are saved.";
      if (recoveryPending || !runNeedsAttention) return protectionMessage;
      return {
        interrupted: "The last run stopped before finishing. Its progress is saved; see Forge progress below.",
        provisional: "Preview profiles are ready. A Standard run qualifies them before they can be applied.",
        field_rejected: "A profile was marked unstable in real use. Forge again to replace it.",
      }[powerSweep?.phase] ?? "The last run ended without new profiles. Open Run details below for the reason.";
    }
    if (verifying) return protectionMessage;
    if (state === "FORGED") return "Profiles are qualified and ready for daily use.";
    if (state === "REFINED") return "Measured profiles are ready for review.";
    if (state === "FORGING") return "Qualification is active; progress and safety take priority below.";
    return "Ready to begin a supervised one-click forge.";
  });
  const alertTitle = $derived(
    rebootRequired
      ? "Restart Windows before continuing"
      : safetyNeedsAttention
        ? "Safe Loop needs attention"
        : forgeBlocked
          ? "Automatic tuning blocked"
        : !serviceReady && serviceStatus === "offline"
          ? "Core Service is offline"
          : "Action could not be completed",
  );
  // An interrupted run explains its own restart in Forge progress; the banner covers every other case.
  const runExplainsReboot = $derived(rebootRequired && powerSweep?.phase === "interrupted");
  const alertMessage = $derived(
    error
      || ((rebootRequired || safetyNeedsAttention) && !runExplainsReboot ? protectionMessage : null)
      || (forgeBlocked && serviceReady ? powerSweep.start_block_reason : null)
      || (!serviceReady && serviceStatus === "offline"
        ? "Nidavellir could not reach the elevated Core Service. Start it to restore hardware detection and GPU actions."
        : serviceError)
      || (!gpuDetected ? hardwareError : null),
  );
  const fullResetDisabled = $derived(Boolean(actionBusy || fullResetBusy || powerRunning || !serviceReady || !gpuDetected || !safeLoopKnown || rebootRequired));

  function finite(value) {
    if (value == null || value === "") return null;
    const number = Number(value);
    if (Number.isFinite(number)) return number;
    return null;
  }

  const temperature = $derived(finite(gpu?.temperature_c));
  const power = $derived(finite(gpu?.power_w));
  const clock = $derived(finite(gpu?.core_clock_mhz));
  const memory = $derived(finite(gpu?.memory_clock_mhz));
  const vramTotal = $derived(finite(gpu?.vram_total_mb));
  const voltage = $derived(finite(gpu?.voltage_mv));
  const fan = $derived(finite(gpu?.fan_speed_pct));
  const usage = $derived(finite(gpu?.utilization_pct));
  const commandMetrics = $derived.by(() => {
    const metrics = [
      { key: "temp", label: "Temperature", value: temperature, unit: "°C" },
      { key: "power", label: "Power", value: power, unit: "W" },
      { key: "core", label: "Clock", value: clock, unit: "MHz" },
      { key: "mem", label: "VRAM", value: memory, unit: "MHz", hint: vramCapacity(vramTotal) },
      { key: "voltage", label: "Voltage", value: voltage, unit: "mV", hint: voltage == null ? "Sensor not exposed" : "Live core voltage" },
      { key: "fan", label: "Fan", value: fan, unit: "%", hint: fan == null ? "Sensor not exposed" : "Average duty" },
      { key: "usage", label: "Utilization", value: usage, unit: "%" },
    ];
    return powerRunning ? metrics.filter((metric) => ["temp", "power", "core", "usage"].includes(metric.key)) : metrics;
  });

  function values(key) {
    const live = sparks?.[key] ?? [];
    return live.length > 1 ? live : [];
  }

  function display(value, digits = 0) {
    return value == null ? "—" : Number(value).toFixed(digits);
  }

  function vramCapacity(value) {
    if (value == null) return "Capacity unavailable";
    const gigabytes = value / 1024;
    return `${gigabytes >= 10 ? gigabytes.toFixed(0) : gigabytes.toFixed(1)} GB total`;
  }

  function pointFor(key) {
    return powerSweep?.[key] ?? null;
  }

  function sameNumber(a, b) {
    if (a == null || b == null) return false;
    return Number(a) === Number(b);
  }

  function normalize(value) {
    return String(value ?? "").toLowerCase().replace(/[^a-z0-9]/g, "");
  }

  function appliedMatches(profile) {
    const point = pointFor(profile.key);
    if (!profilesReady || !point || !applied?.core) return false;
    if (normalize(applied.label) !== normalize(profile.name)) return false;
    const targetClock = point.target_clock_mhz ?? point.clock_mhz;
    if (!sameNumber(applied.core.freq_mhz, targetClock)) return false;
    const applyVoltage = point.vf_table_voltage_mv ?? point.voltage_mv;
    return applyVoltage == null || sameNumber(applied.core.voltage_mv, applyVoltage);
  }

  function profilePower(point) {
    const sustainedP99 = finite(point?.power_p99_w);
    if (sustainedP99 != null && sustainedP99 > 0) return sustainedP99;
    const peak = finite(point?.max_power_w);
    if (peak != null && peak > 0) return peak;
    const average = finite(point?.power_w);
    return average != null && average > 0 ? average : null;
  }

  function profileTarget(point) {
    const target = finite(point?.target_clock_mhz ?? point?.clock_mhz);
    return target == null ? "—" : `${target.toFixed(0)} MHz`;
  }

  function profilePeakPowerText(point) {
    const peak = finite(point?.max_power_w);
    if (peak != null && peak > 0) return `${peak.toFixed(0)} W`;
    const measuredPower = profilePower(point);
    return measuredPower == null ? "—" : `${measuredPower.toFixed(0)} W`;
  }

  function profileVoltage(point) {
    const voltage = finite(point?.vf_table_voltage_mv ?? point?.voltage_mv);
    return voltage == null ? "—" : `${voltage.toFixed(0)} mV`;
  }


  function profileEfficiencyVsStock(point) {
    if (isUndervolt && !(finite(point?.comparison_power_p99_w) > 0)) return "—";
    const stockClock = finite(powerSweep?.stock_clock_mhz);
    const stockPower = finite(powerSweep?.stock_power_p99_w);
    const efficiency = finite(point?.perf_per_watt);
    if (stockClock == null || stockClock <= 0 || stockPower == null || stockPower <= 0 || efficiency == null) {
      return "—";
    }
    const delta = ((efficiency / (stockClock / stockPower)) - 1) * 100;
    return `${delta >= 0 ? "+" : ""}${delta.toFixed(0)}%`;
  }

  /** Representative-load power (PowerRender p99): what separates the profiles. The heaviest lane
   * reaches the power limit for most of them, so it is shown only as the heavy-load peak. */
  function typicalPower(point) {
    const watts = finite(point?.comparison_power_p99_w);
    return watts != null && watts > 0 ? watts : null;
  }

  function profileFacts(point) {
    const watts = typicalPower(point);
    return [profileTarget(point), profileVoltage(point), watts == null ? null : `${watts.toFixed(0)} W`]
      .filter(Boolean)
      .join(" · ");
  }

  function profileVsStock(point) {
    const stockClock = finite(powerSweep?.stock_clock_mhz);
    const stockPower = finite(powerSweep?.stock_power_p99_w);
    const target = finite(point?.target_clock_mhz ?? point?.clock_mhz);
    const watts = typicalPower(point);
    if (!stockClock || !stockPower || target == null || watts == null) return null;
    const signed = (value, unit) => `${value > 0 ? "+" : value < 0 ? "−" : ""}${Math.abs(value)}${unit}`;
    const mhz = Math.round(target - stockClock);
    const power = Math.round(((watts - stockPower) / stockPower) * 100);
    return `${signed(mhz, " MHz")} · ${signed(power, "% power")} vs stock`;
  }

  function recommended(profile) {
    return profile.key === "brokkrs" || Boolean(profile.roles?.includes("Brokkr’s Best"));
  }

  function profileExpanded(key) {
    return profilesReady && expandedProfile === key;
  }

  function toggleProfile(key) {
    if (!profilesReady) return;
    expandedProfile = expandedProfile === key ? null : key;
  }

  function profileStatus(key) {
    if (!profilesReady) return "Available after Forge";
    if (profileActive(key)) return "Applied";
    if (canApply(key)) return "Ready";
    return profilesQualified ? "Measured" : "Qualification pending";
  }

  function profileActive(key) {
    if (activeKey === key) return true;
    if (!activeKey) return false;
    const current = pointFor(activeKey);
    const candidate = pointFor(key);
    return Boolean(current && candidate &&
      sameNumber(current.target_clock_mhz ?? current.clock_mhz, candidate.target_clock_mhz ?? candidate.clock_mhz) &&
      sameNumber(current.vf_table_voltage_mv ?? current.voltage_mv, candidate.vf_table_voltage_mv ?? candidate.voltage_mv));
  }

  function canApply(key) {
    const point = pointFor(key);
    if (actionBusy || fullResetBusy || recoveryPending || !profilesReady || !point || profileActive(key) || !serviceReady || !gpuDetected || !safeLoopKnown || rebootRequired || safetyNeedsAttention || verifying || forgeBlocked) return false;
    if (!isUndervolt) return true;
    const sustainedP99 = finite(point.power_p99_w);
    return Boolean(
      profilesQualified &&
        point.apply_qualified &&
        sustainedP99 != null &&
        sustainedP99 > 0,
    );
  }

  function profileAction(key) {
    if (!canApply(key)) return;
    onApplyPower?.(key);
  }

  function runForge(event) {
    if (primaryActionDisabled) return;
    if (primaryAction.kind === "recover") onRecoverContinue?.();
    else if (primaryAction.kind === "review") {
      safetyDetailsOpen = true;
      requestAnimationFrame(() => {
        safetyDetails?.querySelector("summary")?.focus();
        safetyDetails?.scrollIntoView({ block: "nearest" });
      });
    }
    else if (primaryAction.kind === "resume") onResumePower?.();
    else if (primaryAction.kind === "reset") onReset?.();
    else if (primaryAction.kind === "start_over") openResetConfirmation(event);
    else if (primaryAction.kind === "start") onStartPower?.(forgeMode);
  }

  function reportProfile(profile) {
    if (powerRunning || !serviceReady || rebootRequired || !pointFor(profile.key)) return;
    onReportProfileUnstable?.(profile.key);
  }

  function selectMode(event) {
    onForgeModeChange?.(event.currentTarget.value);
  }

  function navigate(target) {
    onViewChange?.(["forge", "advanced", "settings"].includes(target) ? target : "forge");
  }

  function openResetConfirmation(event, mode = "full") {
    if (fullResetDisabled) return;
    resetMode = mode;
    resetTrigger = event.currentTarget;
    resetConfirmOpen = true;
    requestAnimationFrame(() => {
      resetDialog?.showModal();
      resetCancelButton?.focus();
    });
  }

  function closeResetConfirmation() {
    if (fullResetBusy) return;
    resetDialog?.close();
    resetConfirmOpen = false;
    requestAnimationFrame(() => resetTrigger?.focus());
  }

  async function confirmFullReset() {
    if (fullResetBusy) return;
    const mode = resetMode;
    closeResetConfirmation();
    await onFullReset?.(mode);
  }

  function handleResetDialogCancel(event) {
    event.preventDefault();
    closeResetConfirmation();
  }

  function handleResetDialogKeydown(event) {
    if (event.key !== "Escape") return;
    event.preventDefault();
    closeResetConfirmation();
  }

  const backgroundStyle = `--forge-texture: url('${forgeTexture}')`;
</script>

{#snippet profileDisclosure(profile, variant)}
  {@const point = pointFor(profile.key)}
  <article
    class={`forge-profile-card ${variant}`}
    class:active={profileActive(profile.key)}
    class:expanded={profileExpanded(profile.key)}
    class:preview={!profilesReady}
  >
    <button
      class="profile-disclosure"
      type="button"
      onclick={() => toggleProfile(profile.key)}
      disabled={!profilesReady}
      aria-expanded={profileExpanded(profile.key)}
      aria-controls={`profile-details-${variant}-${profile.key}`}
    >
      <span class="profile-card-icon">
        {#if profile.key === "godforge"}<Hammer size={27} />
        {:else if profile.key === "brokkrs"}<Star size={29} />
        {:else}<Feather size={27} />{/if}
      </span>
      <span class="profile-card-copy">
        <strong>{profile.name}{#if recommended(profile)}<em class="profile-tag">Recommended</em>{/if}</strong>
        {#if profilesReady && point}
          <span class="profile-facts">{profileFacts(point)}</span>
          {#if profileVsStock(point)}<small>{profileVsStock(point)}</small>{/if}
        {:else}
          <small>{profile.summary}</small>
        {/if}
      </span>
      <span class="profile-card-state">
        <small>{profileStatus(profile.key)}</small>
        {#if profilesReady}<ChevronDown size={21} strokeWidth={1.7} />{/if}
      </span>
    </button>

    {#if profileExpanded(profile.key)}
      <div class="profile-card-details" id={`profile-details-${variant}-${profile.key}`}>
        <p class="profile-card-summary">{profile.summary}</p>
        <div class="profile-card-metrics">
          <span><small>Clock</small><strong>{profileTarget(point)}</strong></span>
          <span><small>Voltage</small><strong>{profileVoltage(point)}</strong></span>
          <span title="Power p99 in the representative test load"><small>Typical power</small><strong>{typicalPower(point) != null ? `${typicalPower(point).toFixed(1)} W` : "—"}</strong></span>
          <span title="Highest power seen in the heaviest test; the GPU power limit caps it"><small>Heavy-load peak</small><strong>{profilePeakPowerText(point)}</strong></span>
          <span title="Clock per watt in the representative load, compared with stock"><small>Efficiency vs stock</small><strong>{profileEfficiencyVsStock(point)}</strong></span>
        </div>
        <div class="profile-card-actions">
          <button class="profile-apply" type="button" onclick={() => profileAction(profile.key)} disabled={!canApply(profile.key)}>
            {#if profileActive(profile.key)}<ShieldCheck size={17} />Applied{:else}Apply {profile.name}{/if}
          </button>
          <button class="field-failure" type="button" onclick={() => reportProfile(profile)} disabled={powerRunning || !serviceReady || rebootRequired || !point}>Mark unstable</button>
        </div>
      </div>
    {/if}
  </article>
{/snippet}

{#snippet safetyNotice()}
  {#if profilesReady && profileCaveat}
    <aside class="command-alert" role="status">
      <ShieldCheck size={22} strokeWidth={1.8} />
      <div>
        <strong>{displayedProfiles.length} distinct {displayedProfiles.length === 1 ? "setting" : "settings"} · {profilesQualified ? "qualified" : "qualification pending"}</strong>
        <p>{powerSweep?.discovery_search
          ? "Profiles use the candidates qualified within this run's search budget. Better trade-offs may remain unexplored; shared objectives are shown once."
          : powerSweep?.profile_search_complete ? "The economic clock range was explored. Shared objectives are shown once." : "Economic search coverage is incomplete or unavailable in this saved result. Better trade-offs may remain unexplored."}</p>
        {#if censoredClocks > 0}<p>{censoredClocks} clock searches had voltage ranges excluded by known-failure policy. Those exclusions are not measured stability limits.</p>{/if}
      </div>
    </aside>
  {/if}
  {#if powerSweep?.development_validation_note}
    <aside class="command-alert" role="status">
      <ShieldCheck size={22} strokeWidth={1.8} />
      <div><strong>Development validation</strong><p>{powerSweep.development_validation_note}</p></div>
    </aside>
  {/if}
  {#if alertMessage}
    <aside class="command-alert" class:critical={Boolean(error || rebootRequired || safetyNeedsAttention || forgeBlocked || serviceStatus === "offline")} role={error || rebootRequired || serviceStatus === "offline" ? "alert" : "status"}>
      <TriangleAlert size={22} strokeWidth={1.8} />
      <div>
        <strong>{alertTitle}</strong><p>{alertMessage}</p>
        {#if forgeBlocked && serviceReady}
          <p>Soft Reset keeps known failures. Full Reset erases all GPU learning, including failure history. A required Windows restart or development authorization still applies.</p>
          <details class="safety-details" bind:this={safetyDetails} bind:open={safetyDetailsOpen}>
            <summary>Why tuning is blocked and what to do</summary>
            {#if alertMessage !== powerSweep.start_block_reason}<p>{powerSweep.start_block_reason}</p>{/if}
            <p>Keep the GPU at its default settings. Review the reason above. Soft Reset preserves known failures; Full Reset erases them. A required Windows restart or development authorization must be completed separately. Export the diagnostic report if you need help.</p>
            <div class="safety-actions">
              {#if applied?.core || applied?.mem_offset_mhz}
                <button type="button" onclick={onReset} disabled={actionBusy || fullResetBusy || powerRunning || rebootRequired}>Return to stock</button>
              {/if}
              <button type="button" onclick={onExportLog} disabled={exporting || !onExportLog}>{exporting ? "Exporting…" : "Export diagnostic report"}</button>
              <button type="button" onclick={onViewSafetyHistory}>View safety history</button>
            </div>
            {#if exportMsg}<p class:export-error={exportFailed}>{exportMsg}</p>{/if}
          </details>
        {/if}
      </div>
      {#if error && onDismissError}
        <button type="button" onclick={onDismissError} aria-label="Dismiss error"><X size={18} /></button>
      {/if}
    </aside>
  {/if}
{/snippet}

{#snippet fullResetControl()}
  {#if !powerRunning}
    <section class="full-reset-strip" aria-label="Full Reset">
      <div class="full-reset-copy">
        <span>START OVER</span>
        <p>Soft Reset remeasures and keeps known failures blocked. Full Reset erases all GPU learning.</p>
      </div>
      <button class="soft-reset-action" type="button" onclick={(event) => openResetConfirmation(event, "soft")} disabled={fullResetDisabled}>Soft Reset</button>
      <button class="full-reset-action" type="button" onclick={openResetConfirmation} disabled={fullResetDisabled} title={!serviceReady ? "Core Service must be online" : rebootRequired ? "Restart Windows before tuning actions" : undefined}>
        <Trash2 size={18} strokeWidth={1.7} />
        <span>Full Reset</span>
      </button>
    </section>
  {/if}
{/snippet}

{#snippet commandTelemetry()}
  <section
    class="command-telemetry"
    class:essential={powerRunning}
    aria-labelledby="command-telemetry-title"
  >
    <h2 id="command-telemetry-title" class="sr-only">Live GPU telemetry</h2>
    {#each commandMetrics as metric}
      <article class="command-metric">
        <div class="metric-title">
          {#if metric.key === "temp"}<Thermometer size={25} />
          {:else if metric.key === "power"}<Zap size={25} />
          {:else if metric.key === "core"}<Gauge size={25} />
          {:else if metric.key === "fan"}<Fan size={25} />
          {:else if metric.key === "voltage"}<Activity size={25} />
          {:else if metric.key === "usage"}<CircleGauge size={25} />
          {:else}<Cpu size={25} />{/if}
          <span>{metric.label}</span>
        </div>
        <div class="metric-reading"><strong>{display(metric.value)}</strong><span>{metric.unit}</span></div>
        {#if metric.hint}<small>{metric.hint}</small>{/if}
        <TelemetrySpark values={values(metric.key)} color="#80bd31" fill="rgba(128, 189, 49, 0.08)" height={40} />
      </article>
    {/each}
  </section>
{/snippet}

{#snippet commandProfiles()}
  <section class="command-profiles" class:profile-overview={!profilesReady} aria-labelledby="command-profiles-title">
    <div class="section-label">
      <h2 id="command-profiles-title">{profilesReady ? "Forged profiles" : "Profile goals"}</h2>
      <span>{profilesReady ? "Measured on this GPU" : "Created after qualification"}</span>
      {#if applied?.core || applied?.mem_offset_mhz}
        <button class="profile-apply" onclick={onReset} disabled={!serviceReady || !safeLoopKnown || actionBusy || fullResetBusy || powerRunning}>Return to stock</button>
      {/if}
    </div>
    {#each displayedProfiles as profile}
      {@render profileDisclosure(profile, "command")}
    {/each}
  </section>
{/snippet}

{#snippet commandProgress()}
  <ForgeProgress
    {powerSweep}
    {powerRunning}
    {safeLoop}
    {forgeMode}
    {onStopPower}
    {onStartPower}
    {onRecoverContinue}
    {onResumePower}
    bind:detailsOpen={progressDetailsOpen}
  />
{/snippet}

<section class="forge-theme-screen command" style={backgroundStyle}>
  <header class="command-header">
    <button class="brand command-brand" onclick={() => navigate("forge")} aria-label="Nidavellir Forge home">
      <img src={commandMark} alt="" />
      <span>NIDAVELLIR</span>
    </button>
    <nav class="command-nav" aria-label="Primary navigation">
      <button class:active={activeView === "forge"} aria-current={activeView === "forge" ? "page" : undefined} onclick={() => navigate("forge")}>Forge</button>
      <button class:active={activeView === "settings"} aria-current={activeView === "settings" ? "page" : undefined} onclick={() => navigate("settings")}>Settings</button>
    </nav>
    <div class="command-system-status" aria-label="System readiness">
      <span class="system-item" class:pending={!serviceReady} class:problem={serviceStatus === "offline"}>
        <small>CORE SERVICE</small><strong><i></i>{serviceLabel}</strong>
      </span>
      <span class="system-item" class:pending={!gpuDetected} title={gpuName}>
        <small>GPU</small><strong><i></i>{gpuConnectionLabel}</strong>
      </span>
      <span class="system-item" class:pending={!protectedState || verifying} class:problem={rebootRequired || safetyNeedsAttention}>
        <small>SAFE LOOP</small><strong><i></i>{protectionLabel}</strong>
      </span>
      <span class="system-item profile-state">
        <small>PROFILE</small><strong>{activeName}</strong>
      </span>
    </div>
  </header>

  {#if activeView === "settings"}
    <div class="command-page"><ForgeSettingsPage {powerSweep} {serviceReady} /></div>
  {:else if activeView === "advanced"}
    <div class="command-page">{@render children?.()}</div>
  {:else}
  <div class="command-body">
    <section class="command-hero" class:compact={progressPriority} class:attention={state === "ATTENTION"}>
      <div class="command-identity">
        <span class="eyebrow">LOCAL NVIDIA GPU</span>
        <h1 data-forge-heading tabindex="-1">{gpuName}</h1>
        <span class="gpu-source">{gpuDetected ? (primaryGpu?.driver ?? "Identified by local sensors") : "Waiting for local hardware detection"}</span>
        <div class="state-status">
          <div><span>STATE</span><strong class="state-value" class:problem={["OFFLINE", "ATTENTION"].includes(state)} class:pending={["CONNECTING", "WAITING"].includes(state)} class:working={["RAW", "FORGING", "REFINED"].includes(state)}>{state}</strong></div>
          <div><span>STATUS</span><strong class="protected" class:pending={!safeLoopKnown || verifying} class:problem={rebootRequired || safetyNeedsAttention}><ShieldCheck size={38} />{protectionLabel}</strong></div>
        </div>
        <p>{heroMessage}</p>
      </div>
      <div class="command-cta">
        <button class="plate-button" onclick={runForge} disabled={primaryActionDisabled} aria-describedby="primary-action-reason">
          <img src={copperPlate} alt="" />
          <span>{actionLabel}</span>
        </button>
        <div class="command-run-mode">
          <details>
            <summary>Run options · {forgeMode === "standard" ? "Standard recommended" : forgeMode === "clean" ? "Clean Run" : "Long"}</summary>
          <label for="command-run-mode">
            <span>OPTIONAL</span>
            <select id="command-run-mode" value={forgeMode} onchange={selectMode} disabled={runModeDisabled}>
              <option value="standard">Standard · recommended</option>
              <option value="long">Long · extended qualification</option>
              <option value="clean">Clean Run · rebuild measurements</option>
            </select>
            <ChevronDown size={18} />
          </label>
          </details>
          <small id="primary-action-reason">{primaryActionReason}</small>
          <AutoResumeToggle {powerSweep} disabled={!serviceReady} /><SentinelAdvice {powerSweep} disabled={!serviceReady} />
        </div>
      </div>
    </section>

    {@render safetyNotice()}

    {#if progressPriority}
      {#if hasForgeRun}{@render commandProgress()}{/if}
      {@render commandTelemetry()}
      {#if showProfiles}{@render commandProfiles()}{/if}
    {:else if profilesReady || runFinished}
      {#if showProfiles}{@render commandProfiles()}{/if}
      {#if hasForgeRun}{@render commandProgress()}{/if}
      {@render commandTelemetry()}
    {:else}
      {#if showProfiles}{@render commandProfiles()}{/if}
      {@render commandTelemetry()}
      {#if hasForgeRun}{@render commandProgress()}{/if}
    {/if}

    {@render fullResetControl()}

    <button class="command-advanced" onclick={() => navigate("advanced")}
      ><span>Advanced diagnostics <ChevronRight size={22} /></span><small>Live log, Sentinel, Game Trace and manual point</small><ChevronDown size={24} /></button
    >
  </div>
  {/if}

  {#if fullResetFeedback?.message}
    <aside
      class={`reset-feedback ${fullResetFeedback.tone ?? "success"}`}
      role={fullResetFeedback.tone === "error" ? "alert" : "status"}
      aria-live={fullResetFeedback.tone === "error" ? "assertive" : "polite"}
    >
      {#if fullResetFeedback.tone === "success"}
        <ShieldCheck size={22} strokeWidth={1.8} />
      {:else if fullResetFeedback.tone === "progress"}
        <Activity size={22} strokeWidth={1.8} />
      {:else}
        <TriangleAlert size={22} strokeWidth={1.8} />
      {/if}
      <div>
        <strong>{fullResetFeedback.title ?? (fullResetFeedback.tone === "success" ? "Reset completed" : fullResetFeedback.tone === "warning" ? "Reset needs review" : "Reset failed")}</strong>
        <p>{fullResetFeedback.message}</p>
      </div>
      <button type="button" onclick={onDismissFullResetFeedback} aria-label="Fechar mensagem do reset">
        <X size={18} />
      </button>
    </aside>
  {/if}

  {#if resetConfirmOpen}
    <dialog
      bind:this={resetDialog}
      class="reset-dialog"
      aria-labelledby="reset-dialog-title"
      aria-describedby="reset-dialog-description"
      oncancel={handleResetDialogCancel}
      onkeydown={handleResetDialogKeydown}
    >
      <div class="reset-dialog-heading">
        <span class="reset-dialog-icon"><TriangleAlert size={25} strokeWidth={1.7} /></span>
        <div>
          <span>CONFIRM START OVER</span>
          <h2 id="reset-dialog-title">{resetMode === "full" ? "Full Reset" : "Soft Reset"}</h2>
        </div>
      </div>
      <p id="reset-dialog-description">{resetMode === "full" ? "Return the GPU to stock and permanently erase all saved GPU learning: profiles, measurements, checkpoints, learning archives, blacklist and failure history. The next Forge discovers this GPU from scratch." : "Return the GPU to stock, clear profiles, successful measurements and the saved run, and acknowledge recovery. Known failures, blacklist and their safety boundaries stay preserved."}</p>
      <p class="reset-dialog-note">{resetMode === "full" ? "This cannot be undone. Previously rejected points may be tested again. This does not bypass a required Windows restart or grant development authorization." : "The next Forge takes fresh measurements while avoiding known failures. No run starts automatically."}</p>
      <div class="reset-dialog-actions">
        <button bind:this={resetCancelButton} class="reset-cancel" type="button" onclick={closeResetConfirmation} disabled={fullResetBusy}>Cancel</button>
        <button class="reset-confirm" type="button" onclick={confirmFullReset} disabled={fullResetBusy}>
          <Trash2 size={18} strokeWidth={1.8} />
          <span>{fullResetBusy ? "Resetting…" : resetMode === "full" ? "Erase all GPU learning" : "Clear measurements, keep failures"}</span>
        </button>
      </div>
    </dialog>
  {/if}
</section>

<style>
  :global(body) {
    overflow-x: hidden;
    background: #080b0c;
  }

  .forge-theme-screen {
    position: relative;
    min-height: 100vh;
    overflow-x: hidden;
    color: #d8dbde;
    background-color: #0a0d0e;
    background-image: var(--forge-texture);
    background-blend-mode: normal;
    background-size: 1536px 1024px;
    background-repeat: repeat;
    font-family: "Segoe UI", system-ui, sans-serif;
    -webkit-font-smoothing: antialiased;
  }

  .forge-theme-screen::before {
    content: "";
    position: absolute;
    z-index: 0;
    inset: 0;
    background: #0a0c0d;
    opacity: 0.55;
    pointer-events: none;
  }

  .forge-theme-screen > * {
    position: relative;
    z-index: 1;
  }

  .forge-theme-screen,
  .forge-theme-screen * {
    box-sizing: border-box;
  }

  button,
  select {
    font: inherit;
  }

  button {
    color: inherit;
  }

  button:active:not(:disabled) {
    scale: 0.96;
  }

  .full-reset-strip {
    display: flex;
    min-width: 0;
    align-items: center;
    justify-content: space-between;
    gap: 18px;
    border: 1px solid rgba(174, 91, 72, 0.2);
    padding: 10px 16px;
    background: rgba(40, 15, 12, 0.08);
  }

  .full-reset-copy {
    display: grid;
    min-width: 0;
    grid-template-columns: auto 1fr;
    align-items: baseline;
    gap: 3px 14px;
  }

  .full-reset-copy > span {
    grid-column: 1 / -1;
    color: #936f66;
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.12em;
  }

  .full-reset-copy > p {
    margin: 0;
    color: #878c8e;
    font-size: 12px;
    line-height: 1.45;
    text-wrap: pretty;
  }

  .soft-reset-action,
  .full-reset-action,
  .reset-dialog-actions button {
    display: inline-flex;
    min-height: 38px;
    align-items: center;
    justify-content: center;
    gap: 9px;
    border: 1px solid rgba(193, 96, 76, 0.65);
    padding: 0 16px;
    background: transparent;
    color: #d8a398;
    cursor: pointer;
    transition:
      color 150ms ease,
      background-color 150ms ease,
      border-color 150ms ease,
      opacity 150ms ease;
  }

  .soft-reset-action:hover:not(:disabled),
  .full-reset-action:hover:not(:disabled),
  .reset-confirm:hover:not(:disabled) {
    border-color: #d27361;
    background: rgba(145, 50, 35, 0.14);
    color: #f0b1a4;
  }

  .soft-reset-action:focus-visible,
  .full-reset-action:focus-visible,
  .reset-dialog-actions button:focus-visible,
  .reset-feedback button:focus-visible {
    outline: 2px solid #cf8f79;
    outline-offset: 3px;
  }

  .soft-reset-action:disabled,
  .full-reset-action:disabled,
  .reset-dialog-actions button:disabled {
    cursor: not-allowed;
    opacity: 0.48;
  }

  .reset-dialog::backdrop {
    background: rgba(3, 5, 6, 0.82);
    backdrop-filter: blur(4px);
  }

  .reset-dialog {
    box-sizing: border-box;
    width: min(570px, 100%);
    max-height: calc(100vh - 48px);
    margin: auto;
    border: 1px solid #765249;
    padding: 26px;
    background-color: #111516;
    background-image: var(--forge-texture);
    background-size: 1024px 683px;
    color: #dedbd7;
    overflow-y: auto;
    box-shadow:
      inset 0 0 0 3px rgba(0, 0, 0, 0.32),
      0 28px 80px rgba(0, 0, 0, 0.68);
  }

  .reset-dialog-heading {
    display: flex;
    align-items: center;
    gap: 14px;
    padding-bottom: 18px;
    border-bottom: 1px solid rgba(204, 116, 96, 0.28);
  }

  .reset-dialog-icon {
    display: grid;
    width: 46px;
    height: 46px;
    flex: 0 0 auto;
    place-items: center;
    border: 1px solid #8f5b50;
    color: #dd8d7b;
    background: rgba(117, 41, 29, 0.16);
  }

  .reset-dialog-heading > div {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .reset-dialog-heading > div > span {
    color: #9b746b;
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.12em;
  }

  .reset-dialog h2 {
    margin: 0;
    color: #eee9e5;
    font-size: 25px;
    font-weight: 560;
    letter-spacing: -0.02em;
  }

  .reset-dialog > p {
    margin: 20px 0 0;
    color: #d4ccc7;
    font-size: 15px;
    line-height: 1.6;
    text-wrap: pretty;
  }

  .reset-dialog > .reset-dialog-note {
    margin-top: 10px;
    color: #9c8e89;
    font-size: 12px;
  }

  .reset-dialog-actions {
    display: flex;
    justify-content: flex-end;
    gap: 10px;
    margin-top: 24px;
  }

  .reset-dialog-actions .reset-cancel {
    border-color: #4d5457;
    color: #c7c9c8;
  }

  .reset-dialog-actions .reset-cancel:hover:not(:disabled) {
    border-color: #727a7d;
    background: rgba(255, 255, 255, 0.04);
    color: #f0f0ee;
  }

  .reset-confirm {
    min-width: 226px;
  }

  .reset-feedback {
    position: fixed;
    z-index: 260;
    right: 24px;
    bottom: 24px;
    display: grid;
    width: min(500px, calc(100% - 48px));
    grid-template-columns: auto 1fr auto;
    align-items: start;
    gap: 12px;
    border: 1px solid #4f585b;
    border-left: 3px solid #6ca25b;
    padding: 14px 12px 14px 15px;
    background: #111617;
    color: #7fbb6d;
    box-shadow: 0 18px 52px rgba(0, 0, 0, 0.55);
  }

  .reset-feedback.warning {
    border-left-color: #c19057;
    color: #d2a267;
  }

  .reset-feedback.error {
    border-left-color: #c56857;
    color: #d98270;
  }

  .reset-feedback > div {
    min-width: 0;
  }

  .reset-feedback strong {
    color: #e1dfda;
    font-size: 13px;
    font-weight: 600;
  }

  .reset-feedback p {
    margin: 4px 0 0;
    color: #a5aaab;
    font-size: 12px;
    line-height: 1.5;
    overflow-wrap: anywhere;
  }

  .reset-feedback button {
    display: grid;
    width: 44px;
    min-height: 44px;
    place-items: center;
    border: 0;
    background: transparent;
    color: #8f9698;
    cursor: pointer;
  }

  /* Command Deck */
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    clip-path: inset(50%);
    white-space: nowrap;
  }

  .command-header {
    display: grid;
    grid-template-columns: max-content minmax(180px, 0.55fr) minmax(500px, 1.45fr);
    align-items: center;
    min-height: 88px;
    border-bottom: 1px solid rgba(164, 171, 177, 0.35);
    background: rgba(5, 8, 9, 0.92);
    box-shadow: 0 12px 36px rgba(0, 0, 0, 0.2);
  }

  .brand {
    border: 0;
    background: transparent;
    cursor: pointer;
  }

  .command-brand {
    display: flex;
    min-width: 0;
    height: 100%;
    align-items: center;
    gap: 12px;
    padding: 0 28px;
    color: #aeb0b2;
    font-size: 20px;
    font-weight: 650;
    letter-spacing: 0.17em;
    white-space: nowrap;
  }

  .command-brand img {
    width: 42px;
    height: 42px;
    object-fit: contain;
  }

  .command-nav {
    display: flex;
    min-width: 0;
    height: 100%;
    align-items: stretch;
    gap: 4px;
  }

  .command-nav button {
    position: relative;
    min-width: 82px;
    border: 0;
    padding: 0 14px;
    background: transparent;
    color: #a5a9ae;
    font-size: 16px;
    cursor: pointer;
  }

  .command-nav button.active,
  .command-nav button:hover {
    color: #e9ba79;
  }

  .command-nav button.active::after {
    content: "";
    position: absolute;
    right: 10px;
    bottom: 17px;
    left: 10px;
    height: 2px;
    background: #c5864f;
  }

  .command-system-status {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    align-items: center;
    min-height: 52px;
    border-left: 1px solid rgba(255, 255, 255, 0.18);
    padding-right: 20px;
  }

  .system-item {
    display: flex;
    min-width: 0;
    min-height: 48px;
    flex-direction: column;
    justify-content: center;
    gap: 5px;
    border-left: 1px solid rgba(255, 255, 255, 0.09);
    padding: 0 14px;
  }

  .system-item:first-child { border-left: 0; }

  .system-item small {
    overflow: hidden;
    color: #92999d;
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .system-item strong {
    display: flex;
    min-width: 0;
    align-items: center;
    gap: 7px;
    overflow: hidden;
    color: #a9cf73;
    font-size: 13px;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .system-item i {
    flex: 0 0 auto;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: #79b52f;
    box-shadow: 0 0 0 2px rgba(121, 181, 47, 0.2);
  }

  .system-item.pending strong { color: #9aa2a7; }
  .system-item.pending i {
    background: #687177;
    box-shadow: 0 0 0 2px rgba(104, 113, 119, 0.18);
  }

  .system-item.problem strong { color: #d89479; }
  .system-item.problem i {
    background: #c56857;
    box-shadow: 0 0 0 2px rgba(197, 104, 87, 0.18);
  }

  .system-item.profile-state strong { color: #d7d9d7; }

  .command-body {
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 20px clamp(20px, 2.35vw, 36px) 40px;
  }

  .command-page {
    min-height: calc(100vh - 88px);
    padding: 28px 36px 44px;
  }

  .command-hero {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(290px, 355px);
    align-items: center;
    gap: clamp(28px, 4vw, 68px);
    min-height: 224px;
    border: 1px solid rgba(126, 136, 143, 0.32);
    border-radius: 12px;
    padding: 24px clamp(24px, 3vw, 44px);
    background:
      linear-gradient(115deg, rgba(198, 134, 79, 0.055), transparent 42%),
      rgba(8, 11, 12, 0.7);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.025), 0 18px 45px rgba(0, 0, 0, 0.18);
  }

  .command-hero.compact { min-height: 156px; padding-block: 18px; }
  .command-hero.attention { border-color: rgba(197, 104, 87, 0.48); }

  .command-identity {
    align-self: center;
    min-width: 0;
    padding: 0;
  }

  .eyebrow,
  .state-status span {
    color: #a5aaad;
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.1em;
  }

  .command-identity h1 {
    margin: 4px 0 2px;
    color: #f0f0ef;
    font-size: clamp(32px, 3.2vw, 46px);
    font-weight: 540;
    line-height: 1.08;
    letter-spacing: -0.025em;
    text-wrap: balance;
  }

  .command-identity h1:focus { outline: none; }

  .gpu-source {
    display: block;
    margin-bottom: 18px;
    color: #92999d;
    font-size: 12px;
  }

  .state-status {
    display: grid;
    /* The divider follows the state word: a long one ("CONNECTING") moves it instead of crossing it. */
    grid-template-columns: max-content minmax(0, 1fr);
    column-gap: clamp(24px, 3vw, 46px);
    width: min(100%, 460px);
    border-bottom: 1px solid rgba(255, 255, 255, 0.28);
    padding-bottom: 14px;
  }

  .state-status > div {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .state-status > div + div {
    border-left: 1px solid rgba(255, 255, 255, 0.3);
    padding-left: clamp(24px, 3vw, 46px);
  }

  .state-status strong {
    color: #79b72e;
    font-size: 28px;
    line-height: 1;
  }

  .state-status .state-value.problem { color: #d98270; }
  .state-status .state-value.pending { color: #9aa2a7; }
  .state-status .state-value.working { color: #d0a15f; }

  .state-status .protected {
    display: flex;
    align-items: center;
    gap: 9px;
    font-size: 18px;
    font-weight: 500;
  }

  .state-status .protected.pending { color: #8e979d; }
  .state-status .protected.problem { color: #d98270; }

  .command-identity p {
    max-width: 62ch;
    margin: 12px 0 0;
    color: #b8bbbe;
    font-size: 15px;
    line-height: 1.45;
  }

  .command-cta {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
  }

  .plate-button {
    position: relative;
    width: min(100%, 315px);
    height: 98px;
    overflow: visible;
    border: 0;
    background: transparent;
    cursor: pointer;
    transition: filter 150ms ease, transform 100ms ease;
  }

  .plate-button:hover:not(:disabled) { filter: brightness(1.07); }
  .plate-button:active:not(:disabled) { transform: scale(0.96); }

  .plate-button img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: fill;
    filter: saturate(0.85) brightness(1.1);
  }

  .plate-button span {
    position: relative;
    z-index: 1;
    color: #15110d;
    font-size: 23px;
    font-weight: 600;
  }

  .plate-button:disabled {
    cursor: not-allowed;
    filter: grayscale(0.55);
    opacity: 0.54;
  }

  .command-run-mode {
    display: grid;
    width: min(100%, 315px);
    gap: 7px;
  }

  .command-run-mode label {
    position: relative;
    display: grid;
    min-height: 62px;
    grid-template-columns: 1fr auto;
    align-content: center;
    border: 1px solid #4a5054;
    padding: 8px 42px 8px 15px;
    background: rgba(9, 13, 15, 0.82);
  }

  .command-run-mode label > span {
    grid-column: 1 / -1;
    color: #92999d;
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.14em;
  }

  .command-run-mode select {
    width: 100%;
    appearance: none;
    border: 0;
    padding: 2px 0 0;
    background: transparent;
    color: #d8dbde;
    font-size: 15px;
    font-weight: 600;
    outline: none;
    cursor: pointer;
  }

  .command-run-mode label :global(svg) {
    position: absolute;
    right: 15px;
    bottom: 15px;
    color: #c58a55;
    pointer-events: none;
  }

  .command-run-mode label:focus-within {
    border-color: #b37c4d;
    box-shadow: 0 0 0 2px rgba(179, 124, 77, 0.16);
  }

  .command-run-mode select:disabled {
    cursor: not-allowed;
    opacity: 0.62;
  }

  .command-run-mode small {
    color: #92999d;
    font-size: 12px;
    line-height: 1.45;
    text-align: center;
  }

  .command-alert {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: start;
    gap: 12px;
    border: 1px solid rgba(190, 145, 86, 0.42);
    border-left: 3px solid #c19057;
    border-radius: 9px;
    padding: 14px 16px;
    background: rgba(39, 29, 20, 0.56);
    color: #d2a267;
  }

  .command-alert.critical {
    border-color: rgba(197, 104, 87, 0.42);
    border-left-color: #c56857;
    background: rgba(43, 23, 20, 0.58);
    color: #d98270;
  }

  .command-alert strong { color: #e3e0da; font-size: 14px; font-weight: 650; }
  .command-alert p { margin: 4px 0 0; color: #b9b3ad; font-size: 13px; line-height: 1.5; }
  .command-alert button {
    display: grid;
    width: 44px;
    min-height: 44px;
    place-items: center;
    border: 0;
    background: transparent;
    color: #aaa39e;
    cursor: pointer;
  }

  .safety-details { margin-top: 12px; }
  .safety-details summary { cursor: pointer; font-weight: 600; color: #eee5d7; }
  .safety-actions { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 12px; }
  .command-alert .safety-actions button {
    width: auto;
    padding: 8px 14px;
    border: 1px solid #8a7663;
    border-radius: 6px;
    color: #f2e9dd;
    background: #312b25;
  }
  .command-alert .safety-actions button:disabled { opacity: 0.5; cursor: default; }
  .safety-details .export-error { color: #f29c8c; }

  .command-telemetry {
    display: grid;
    grid-template-columns: repeat(7, minmax(0, 1fr));
    gap: 1px;
    min-height: 0;
    border: 1px solid #62696e;
    background: #555c61;
  }

  .command-telemetry.essential { grid-template-columns: repeat(4, minmax(0, 1fr)); }

  .command-metric {
    position: relative;
    min-width: 0;
    min-height: 142px;
    padding: 18px clamp(12px, 1.15vw, 20px) 12px;
    background: rgba(8, 11, 12, 0.94);
  }

  .command-metric + .command-metric {
    border-left: 0;
  }

  .metric-title {
    display: flex;
    align-items: center;
    gap: 9px;
    color: #a1a9ae;
    font-size: 12px;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }

  .metric-reading {
    display: flex;
    min-width: 0;
    align-items: baseline;
    gap: clamp(6px, 0.65vw, 10px);
    margin: 9px 0 8px;
  }

  .metric-reading strong {
    color: #eceeed;
    font-size: clamp(28px, 2.5vw, 38px);
    font-weight: 500;
    line-height: 1;
    font-variant-numeric: tabular-nums;
  }

  .metric-reading span {
    flex: 0 0 auto;
    color: #b4b7b9;
    font-size: 13px;
  }

  .command-metric small {
    display: block;
    margin-top: 2px;
    color: #93989d;
    font-size: 12px;
    text-align: left;
    min-height: 1.25rem;
    overflow-wrap: anywhere;
  }

  .section-label {
    display: grid;
    grid-template-columns: auto auto minmax(24px, 1fr);
    align-items: center;
    gap: 12px;
    min-height: 32px;
    margin-bottom: 7px;
  }

  .section-label::after {
    content: "";
    height: 1px;
    background: #4e555a;
  }

  .section-label::after { flex: 1; }

  .section-label h2 {
    margin: 0;
    color: #d5d8d8;
    font-size: 16px;
    font-weight: 620;
    letter-spacing: -0.01em;
  }

  .section-label > span {
    color: #92999d;
    font-size: 12px;
  }

  /* Return to stock sits after the divider instead of taking the divider's column. */
  .section-label:has(> button) {
    grid-template-columns: auto auto minmax(24px, 1fr) auto;
  }

  .section-label > button {
    grid-column: 4;
    grid-row: 1;
    border: 0;
    border-radius: 8px;
    padding: 0 14px;
    background: transparent;
    box-shadow: inset 0 0 0 1px rgba(133, 141, 143, 0.32);
    color: #aeb4b4;
    font: inherit;
    font-size: 12px;
    font-weight: 670;
    cursor: pointer;
  }

  .section-label > button:hover:not(:disabled) {
    background-color: rgba(133, 141, 143, 0.12);
    color: #f0eee9;
  }

  .section-label > button:disabled {
    cursor: default;
    opacity: 0.52;
  }

  .command-body :global(.forge-progress) {
    border-radius: 11px;
    box-shadow: 0 14px 38px rgba(0, 0, 0, 0.16);
  }

  .field-failure {
    min-height: 40px !important;
    margin-top: 0 !important;
    border: 0 !important;
    padding: 0 6px !important;
    background: transparent !important;
    color: #9b9189 !important;
    font-size: 12px !important;
    letter-spacing: 0.03em;
    text-decoration: underline;
    text-underline-offset: 3px;
    cursor: pointer;
  }

  .field-failure:disabled {
    cursor: default;
    opacity: 0.45;
  }

  .command-advanced {
    display: grid;
    width: 100%;
    min-height: 66px;
    grid-template-columns: auto 1fr auto;
    align-items: center;
    border: 1px solid #51595f;
    padding: 0 34px;
    background: rgba(6, 9, 10, 0.35);
    color: #c2c5c8;
    cursor: pointer;
    text-align: left;
    transition: background-color 150ms ease, border-color 150ms ease, transform 100ms ease;
  }

  .command-advanced:hover {
    border-color: #747d82;
    background: rgba(255, 255, 255, 0.025);
  }

  .command-advanced:active { transform: scale(0.99); }

  .command button:focus-visible,
  .command select:focus-visible {
    outline: 2px solid #cf955d;
    outline-offset: 3px;
  }

  .command-advanced > span {
    display: flex;
    align-items: center;
    gap: 24px;
    font-size: 18px;
  }

  .command-advanced > small {
    color: #858b91;
    font-size: 14px;
  }

  .command-body :global(.forge-progress) {
    --progress-surface: rgba(7, 10, 12, 0.58);
    --progress-outline: rgba(126, 136, 143, 0.34);
    min-width: 0;
    border-radius: 11px;
  }

  @media (max-width: 1180px) {
    .command-header { grid-template-columns: 238px 174px minmax(0, 1fr); }
    .command-brand { padding-inline: 20px; }
    .command-nav { gap: 2px; }
    .command-nav button { min-width: auto; padding-inline: 10px; }
    .system-item { padding-inline: 10px; }
    .command-hero { grid-template-columns: minmax(0, 1fr) minmax(280px, 320px); }
  }

  @media (min-width: 561px) and (max-width: 980px) {
    .command-telemetry { grid-template-columns: repeat(2, minmax(0, 1fr)); }
    .command-telemetry.essential { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  }

  @media (max-width: 820px) {
    .full-reset-strip {
      min-height: 0;
      align-items: stretch;
      flex-direction: column;
      gap: 14px;
      padding: 16px;
    }
    .full-reset-copy { grid-template-columns: 1fr; }
    .full-reset-copy > span,
    .full-reset-copy > p { grid-column: 1; }
    .soft-reset-action,
  .full-reset-action { width: 100%; }
    .reset-dialog { padding: 20px; }
    .reset-dialog-actions { flex-direction: column-reverse; }
    .reset-dialog-actions button { width: 100%; }
    .reset-confirm { min-width: 0; }
    .reset-feedback { right: 12px; bottom: 12px; width: calc(100% - 24px); }
    .command-header { grid-template-columns: auto minmax(0, 1fr); height: auto; min-height: 82px; }
    .command-brand { padding-left: 20px; }
    .command-nav { justify-self: end; overflow-x: auto; height: 58px; padding-right: 10px; }
    .command-system-status { grid-column: 1 / -1; min-height: 64px; border-top: 1px solid rgba(255, 255, 255, 0.08); border-left: 0; padding: 8px 12px; }
    .command-page { min-height: calc(100vh - 146px); padding: 22px 18px 36px; }
    .command-hero { grid-template-columns: 1fr; }
    .command-cta { align-items: flex-start; }
    .command-telemetry { grid-template-columns: 1fr 1fr; }
  }

  @media (max-width: 560px) {
    .command-body { padding-inline: 12px; }
    .command-brand span { font-size: 17px; }
    .command-hero { min-height: 0; }
    .command-identity h1 { font-size: 34px; }
    .command-system-status { grid-template-columns: repeat(2, minmax(0, 1fr)); }
    .system-item:nth-child(3) { border-left: 0; }
    .state-status { width: 100%; grid-template-columns: 1fr 1fr; }
    .state-status > div + div { padding-left: 20px; }
    .state-status strong { font-size: 30px; }
    .command-cta { flex-direction: column; }
    .plate-button,
    .command-run-mode { width: min(100%, 320px); }
    .command-telemetry { grid-template-columns: 1fr; }
    .command-telemetry.essential { grid-template-columns: 1fr; }
  }

  /* Shared profile disclosure: the three visual themes keep their own color language while the
     information hierarchy and responsive behavior stay identical. */
  .forge-profile-card {
    --profile-accent: #c78a54;
    --profile-surface: rgba(7, 10, 12, 0.58);
    min-width: 0;
    overflow: hidden;
    border-radius: 11px;
    background: var(--profile-surface);
    box-shadow: inset 0 0 0 1px rgba(126, 136, 143, 0.34);
  }

  .forge-profile-card + .forge-profile-card {
    margin-top: 8px;
  }

  .forge-profile-card.active {
    background: color-mix(in srgb, var(--profile-accent) 8%, var(--profile-surface));
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--profile-accent) 72%, transparent);
  }

  .profile-disclosure {
    display: grid;
    width: 100%;
    min-height: 88px;
    grid-template-columns: 52px minmax(0, 1fr) minmax(120px, auto);
    align-items: center;
    gap: 15px;
    border: 0;
    border-radius: 11px;
    padding: 14px 16px;
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition: background-color 150ms ease, transform 100ms ease;
  }

  .profile-disclosure:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.028);
  }

  .profile-disclosure:active:not(:disabled) {
    transform: scale(0.992);
  }

  .profile-disclosure:disabled {
    cursor: default;
    opacity: 1;
  }

  .profile-card-icon {
    display: grid;
    width: 48px;
    height: 48px;
    place-items: center;
    border-radius: 9px;
    background: color-mix(in srgb, var(--profile-accent) 9%, transparent);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--profile-accent) 30%, transparent);
    color: #8e989f;
  }

  .forge-profile-card.active .profile-card-icon {
    color: var(--profile-accent);
  }

  .profile-card-copy {
    display: flex;
    min-width: 0;
    flex-direction: column;
    gap: 5px;
  }

  .profile-card-copy strong {
    color: #e3e3df;
    font-size: 18px;
    font-weight: 580;
    letter-spacing: -0.01em;
  }

  .profile-card-copy small {
    max-width: 74ch;
    color: #92999d;
    font-size: 12px;
    line-height: 1.5;
    text-wrap: pretty;
  }

  .profile-tag {
    margin-left: 10px;
    border-radius: 999px;
    padding: 2px 8px;
    background: rgba(214, 160, 76, 0.12);
    color: var(--profile-accent, #d6a04c);
    font-size: 11px;
    font-style: normal;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    vertical-align: middle;
  }

  .profile-facts {
    color: #d7d9d7;
    font-size: 14px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }

  .profile-card-summary {
    grid-column: 1 / -1;
    margin: 0;
    color: #92999d;
    font-size: 12px;
    line-height: 1.5;
  }

  .profile-card-state {
    display: flex;
    min-width: 0;
    align-items: center;
    justify-content: flex-end;
    gap: 10px;
    color: var(--profile-accent);
  }

  .profile-card-state small {
    overflow: hidden;
    font-size: 12px;
    font-weight: 680;
    letter-spacing: 0.04em;
    text-overflow: ellipsis;
    text-transform: uppercase;
    white-space: nowrap;
  }

  .profile-card-state :global(svg) {
    flex: 0 0 auto;
    transition: transform 160ms ease;
  }

  .forge-profile-card.expanded .profile-card-state :global(svg) {
    transform: rotate(180deg);
  }

  .profile-card-details {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: end;
    gap: 16px;
    margin: 0 14px 14px;
    border-radius: 9px;
    padding: 14px;
    background: rgba(0, 0, 0, 0.2);
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.055);
  }

  .profile-card-metrics {
    display: grid;
    grid-template-columns: repeat(5, minmax(105px, 1fr));
    gap: 8px;
  }

  .profile-card-metrics > span {
    display: flex;
    min-width: 0;
    min-height: 62px;
    flex-direction: column;
    justify-content: center;
    gap: 5px;
    border-radius: 8px;
    padding: 9px 10px;
    background: rgba(255, 255, 255, 0.025);
  }

  .profile-card-metrics small {
    color: #92999d;
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .profile-card-metrics strong {
    overflow: hidden;
    color: #d7d9d7;
    font-size: 14px;
    font-weight: 580;
    font-variant-numeric: tabular-nums;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .profile-card-actions {
    display: flex;
    align-items: stretch;
    gap: 8px;
  }

  .profile-card-actions button {
    display: inline-flex;
    min-height: 44px !important;
    align-items: center;
    justify-content: center;
    gap: 7px;
    border: 0 !important;
    border-radius: 8px;
    margin: 0 !important;
    padding: 0 14px;
    background: transparent;
    color: #aeb4b4;
    font: inherit;
    font-size: 12px;
    font-weight: 670;
    cursor: pointer;
    transition: background-color 150ms ease, box-shadow 150ms ease, color 150ms ease, transform 100ms ease;
  }

  .profile-card-actions .profile-apply {
    background: color-mix(in srgb, var(--profile-accent) 12%, transparent);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--profile-accent) 46%, transparent);
    color: color-mix(in srgb, var(--profile-accent) 78%, white);
  }

  .profile-card-actions .field-failure {
    box-shadow: inset 0 0 0 1px rgba(133, 141, 143, 0.32);
  }

  .profile-card-actions button:hover:not(:disabled) {
    background-color: color-mix(in srgb, var(--profile-accent) 18%, transparent);
    color: #f0eee9;
  }

  .profile-card-actions button:active:not(:disabled) {
    transform: scale(0.96);
  }

  .profile-card-actions button:disabled {
    cursor: default;
    opacity: 0.52;
  }

  @media (max-width: 1080px) {
    .profile-card-details {
      grid-template-columns: 1fr;
    }
    .profile-card-metrics {
      grid-template-columns: repeat(3, minmax(105px, 1fr));
    }
    .profile-card-actions {
      justify-content: flex-end;
    }
  }

  @media (max-width: 820px) {
    .profile-disclosure {
      grid-template-columns: 48px minmax(0, 1fr) auto;
      padding-inline: 12px;
    }
    .profile-card-state small {
      display: none;
    }
  }

  @media (max-width: 560px) {
    .profile-disclosure {
      grid-template-columns: 44px minmax(0, 1fr) auto;
      gap: 11px;
    }
    .profile-card-icon {
      width: 44px;
      height: 44px;
    }
    .profile-card-copy strong {
      font-size: 16px;
    }
    .profile-card-metrics {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
    .profile-card-actions {
      flex-direction: column;
    }
    .profile-card-actions button {
      width: 100%;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .soft-reset-action,
  .full-reset-action,
    .reset-dialog-actions button,
    .plate-button,
    .command-advanced,
    .command-alert button,
    .command-nav button,
    .profile-disclosure,
    .profile-card-state :global(svg),
    .profile-card-actions button {
      transition: none;
    }

    .plate-button:active:not(:disabled),
    .command-advanced:active,
    .profile-disclosure:active:not(:disabled),
    .profile-card-actions button:active:not(:disabled) {
      transform: none;
    }
  }
</style>
