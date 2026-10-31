<script>
  import {
    Activity,
    ShieldCheck,
    Trash2,
    TriangleAlert,
    X,
  } from "@lucide/svelte";
  import ForgeSettingsPage from "./ForgeSettingsPage.svelte";
  import SentinelAdvice from "./SentinelAdvice.svelte";
  import ForgeProgress from "./ForgeProgress.svelte";
  import AutoResumeToggle from "./AutoResumeToggle.svelte";
  import PreForgeSheet from "./PreForgeSheet.svelte";
  import RestartWarning from "./RestartWarning.svelte";
  import { distinctForgeProfiles, forgePrimaryAction, nvidiaGpu } from "../../forge-workflow.js";
  import LiveGpu from "./LiveGpu.svelte";
  import { liveGpuView } from "../../live-gpu-view.js";
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
  let eraseInput = $state(null);
  let eraseText = $state("");
  // Full Reset erases all GPU learning, so it waits for the typed word; Soft Reset keeps failures.
  const resetConfirmed = $derived(resetMode !== "full" || eraseText.trim().toUpperCase() === "ERASE");
  let compareOpen = $state(false);
  let safetyDetailsOpen = $state(false);
  let safetyDetails = $state(null);

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
  // Nothing applied: the GPU runs NVIDIA's own curve, so the Stock column is the live one.
  const stockActive = $derived(!applied?.core && !applied?.mem_offset_mhz);
  const stockClock = $derived(finite(powerSweep?.stock_clock_mhz) == null ? null : Math.round(finite(powerSweep?.stock_clock_mhz)));
  const stockPower = $derived(finite(powerSweep?.stock_power_p99_w) == null ? null : Math.round(finite(powerSweep?.stock_power_p99_w)));
  // Whether stock already runs into the power limit under heavy load (≥95% of it, as the Forge
  // reads a power-bound drop): then undervolting gives back clock, otherwise it mostly saves power.
  const stockRegime = $derived.by(() => {
    const limit = finite(powerSweep?.power_limit_w);
    const watts = finite(powerSweep?.stock_power_p99_w);
    if (limit == null || limit <= 0 || watts == null) return null;
    return watts >= limit * 0.95
      ? { text: `Held back by its ${Math.round(limit)} W limit`, title: "Under heavy load at stock this GPU runs into its power limit, so a lower voltage gives back clock as well as power." }
      : { text: `Below its ${Math.round(limit)} W limit`, title: "Under heavy load at stock this GPU stays under its power limit, so a lower voltage mostly saves power and heat." };
  });
  // Runes: Othala (inheritance, the factory curve), Thurisaz (Thor; Brokkr forged Mjölnir),
  // Berkano (B for Brokkr), Laguz (water, calm).
  const PROFILE_RUNES = { godforge: "ᚦ", brokkrs: "ᛒ", deep_calm: "ᛚ" };
  const PROFILE_TAGS = { godforge: "Highest clock", brokkrs: "Recommended", deep_calm: "Lowest power" };
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
  // Each mode's time budget comes from the Core (Long's longer dwells scale Standard's 9 h).
  const RUN_MODES = { standard: "Standard · up to 9 h", long: "Long · up to about 20 h", clean: "Clean Run · up to 9 h" };
  let preForgeOpen = $state(false);
  // The sheet only starts a new run: it closes when the main action becomes anything else.
  $effect(() => {
    if (preForgeOpen && (primaryAction.kind !== "start" || primaryActionDisabled)) preForgeOpen = false;
  });
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
  // Shown in Settings under the Start over section while both resets are unavailable.
  const fullResetDisabledReason = $derived(
    powerRunning ? "Unavailable during a Forge. Stop the run first."
      : !serviceReady ? "The Core Service must be online."
      : rebootRequired ? "Restart Windows before any tuning action."
      : !gpuDetected ? "Waiting for the NVIDIA GPU."
      : !safeLoopKnown ? "Waiting for the Safe Loop status."
      : actionBusy || fullResetBusy ? "Another action is running."
      : "",
  );

  function finite(value) {
    if (value == null || value === "") return null;
    const number = Number(value);
    if (Number.isFinite(number)) return number;
    return null;
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

  /** The clock a profile holds under heavy load, as the Forge ranks it: its target, or its measured
   * p5 when that sits more than one 15 MHz bin under (an RTX 50 moves its own curve under load). */
  function heldClock(point) {
    const target = finite(point?.target_clock_mhz ?? point?.clock_mhz);
    const p5 = finite(point?.p5_clock_mhz);
    return target != null && p5 != null && p5 + 15 < target ? p5 : target;
  }

  function profileTarget(point) {
    const clock = heldClock(point);
    return clock == null ? "—" : `${clock.toFixed(0)} MHz`;
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

  function profileClock(point) {
    const clock = heldClock(point);
    return clock == null ? "—" : clock.toFixed(0);
  }

  function profileFacts(point) {
    const watts = typicalPower(point);
    return [profileVoltage(point), watts == null ? null : `${watts.toFixed(0)} W`].filter(Boolean).join(" · ");
  }

  /** Clock and typical power against the stock baseline the same Forge measured. */
  function profileDelta(point) {
    const clock = heldClock(point);
    const watts = typicalPower(point);
    if (!stockClock || !stockPower || clock == null || watts == null) return null;
    const signed = (value, unit) => `${value > 0 ? "+" : value < 0 ? "−" : ""}${Math.abs(value)}${unit}`;
    const mhz = Math.round(clock - stockClock);
    const power = Math.round(((watts - stockPower) / stockPower) * 100);
    return { mhz, power, mhzText: signed(mhz, " MHz"), powerText: signed(power, "% power") };
  }

  /** Typical power as a share of stock, for the thin bar under each column. */
  function profileBar(point) {
    const watts = typicalPower(point);
    if (!stockPower || watts == null) return 100;
    return Math.max(4, Math.min(100, Math.round((watts / stockPower) * 100)));
  }

  function recommended(profile) {
    return profile.key === "brokkrs" || Boolean(profile.roles?.includes("Brokkr’s Best"));
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
    else if (primaryAction.kind === "start") preForgeOpen = true;
  }

  function reportProfile(profile) {
    if (powerRunning || !serviceReady || rebootRequired || !pointFor(profile.key)) return;
    onReportProfileUnstable?.(profile.key);
  }

  function selectMode(event) {
    onForgeModeChange?.(event.currentTarget.value);
  }

  function navigate(target) {
    onViewChange?.(["forge", "report", "log", "sentinel", "settings"].includes(target) ? target : "forge");
  }

  function openStartOver() {
    navigate("settings");
    requestAnimationFrame(() => document.getElementById("start-over")?.scrollIntoView({ block: "start" }));
  }

  function openResetConfirmation(event, mode = "full") {
    if (fullResetDisabled) return;
    resetMode = mode;
    eraseText = "";
    resetTrigger = event.currentTarget;
    resetConfirmOpen = true;
    requestAnimationFrame(() => {
      resetDialog?.showModal();
      // Full Reset waits for the typed word; Soft Reset starts on Cancel, the safe default.
      (mode === "full" ? eraseInput : resetCancelButton)?.focus();
    });
  }

  function closeResetConfirmation() {
    if (fullResetBusy) return;
    resetDialog?.close();
    resetConfirmOpen = false;
    requestAnimationFrame(() => resetTrigger?.focus());
  }

  async function confirmFullReset() {
    if (fullResetBusy || !resetConfirmed) return;
    const mode = resetMode;
    closeResetConfirmation();
    // The reset is started from Settings; its result (no profiles, a fresh Forge) shows on the Forge.
    navigate("forge");
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

{#snippet stockColumn()}
  <article class="profile-column" class:applied={profilesReady && stockActive}>
    <span class="profile-tag">Baseline</span>
    <h3 class="profile-name"><span class="rune" aria-hidden="true">ᛟ</span>Stock</h3>
    {#if profilesReady && stockClock != null}
      <div class="profile-clock">{stockClock}<span>MHz</span></div>
      <div class="profile-facts">NVIDIA default curve{stockPower != null ? ` · ${stockPower} W` : ""}</div>
      <div class="profile-delta" title={stockRegime?.title}>{stockRegime?.text ?? "The reference for every number"}</div>
      <div class="profile-bar" aria-hidden="true"><i style="width: 100%"></i></div>
      {#if stockActive}
        {@render appliedMark()}
      {:else}
        <button class="profile-apply ghost" type="button" onclick={onReset} disabled={!serviceReady || !safeLoopKnown || actionBusy || fullResetBusy || powerRunning}>Use stock</button>
      {/if}
    {:else}
      <p class="profile-goal">Your GPU as NVIDIA ships it. The Forge measures it first, and every profile is compared with it.</p>
    {/if}
  </article>
{/snippet}

{#snippet appliedMark()}
  <div class="applied-row">
    <span class="applied-plate"><img src={copperPlate} alt="" /><span>Applied</span></span>
    <span class="running"><i aria-hidden="true"></i>Running now</span>
  </div>
{/snippet}

{#snippet profileColumn(profile)}
  {@const point = pointFor(profile.key)}
  {@const active = profileActive(profile.key)}
  {@const shared = (profile.roles?.length ?? 0) > 1}
  <article class="profile-column" class:applied={active}>
    <span class="profile-tag" class:recommended={recommended(profile)}>{shared ? "Shared setting" : PROFILE_TAGS[profile.key]}</span>
    <h3 class="profile-name"><span class="rune" aria-hidden="true">{PROFILE_RUNES[profile.key]}</span>{shared ? profile.roles.join(" + ") : profile.name}</h3>
    {#if profilesReady && point}
      {@const delta = profileDelta(point)}
      <div class="profile-clock">{profileClock(point)}<span>MHz</span></div>
      <div class="profile-facts">{profileFacts(point)}</div>
      <div class="profile-delta">
        {#if delta}<span class:delta-up={delta.mhz > 0}>{delta.mhzText}</span> · <span class:delta-save={delta.power < 0}>{delta.powerText}</span>{/if}
      </div>
      <div class="profile-bar" aria-hidden="true"><i style={`width: ${profileBar(point)}%`}></i></div>
      {#if active}
        {@render appliedMark()}
      {:else}
        <button class="profile-apply" type="button" aria-label={`Apply ${profile.name}`} onclick={() => profileAction(profile.key)} disabled={!canApply(profile.key)}>Apply</button>
      {/if}
    {:else}
      <p class="profile-goal">{profile.summary}</p>
      <span class="profile-pending">Available after Forge</span>
    {/if}
  </article>
{/snippet}

{#snippet profileCompare()}
  <div class="profile-compare" id="profile-compare">
    <table>
      <thead>
        <tr>
          <th scope="col"><span class="sr-only">Measure</span></th>
          <th scope="col">Stock</th>
          {#each displayedProfiles as profile}<th scope="col">{profile.name}</th>{/each}
        </tr>
      </thead>
      <tbody>
        <tr>
          <th scope="row">Clock</th>
          <td>{stockClock != null ? `${stockClock} MHz` : "—"}</td>
          {#each displayedProfiles as profile}<td>{profileTarget(pointFor(profile.key))}</td>{/each}
        </tr>
        <tr>
          <th scope="row">Voltage</th>
          <td>NVIDIA curve</td>
          {#each displayedProfiles as profile}<td>{profileVoltage(pointFor(profile.key))}</td>{/each}
        </tr>
        <tr>
          <th scope="row" title="Power p99 in the representative test load">Typical power</th>
          <td>{stockPower != null ? `${stockPower} W` : "—"}</td>
          {#each displayedProfiles as profile}<td>{typicalPower(pointFor(profile.key)) != null ? `${typicalPower(pointFor(profile.key)).toFixed(1)} W` : "—"}</td>{/each}
        </tr>
        <tr>
          <th scope="row" title="Highest power seen in the heaviest test; the GPU power limit caps it">Heavy-load peak</th>
          <td>—</td>
          {#each displayedProfiles as profile}<td>{profilePeakPowerText(pointFor(profile.key))}</td>{/each}
        </tr>
        <tr>
          <th scope="row" title="Clock per watt in the representative load, compared with stock">Efficiency vs stock</th>
          <td>Reference</td>
          {#each displayedProfiles as profile}<td>{profileEfficiencyVsStock(pointFor(profile.key))}</td>{/each}
        </tr>
        <tr>
          <th scope="row">In your games</th>
          <td></td>
          {#each displayedProfiles as profile}
            <td><button class="field-failure" type="button" aria-label={`Mark ${profile.name} unstable`} onclick={() => reportProfile(profile)} disabled={powerRunning || !serviceReady || rebootRequired || !pointFor(profile.key)}>Mark unstable</button></td>
          {/each}
        </tr>
      </tbody>
    </table>
  </div>
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
              <button type="button" onclick={openStartOver}>Start over options</button>
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

{#snippet commandTelemetry()}
  <section class="band live-band" aria-labelledby="live-gpu-title">
    <div class="band-head">
      <h2 id="live-gpu-title">Live GPU</h2>
      <span class="band-note">{$liveGpuView === "gauges" ? "Live" : "Last 60 s"} · against {applied?.core ? (activeKey ? activeName : "the applied setting") : "stock"}</span>
    </div>
    <LiveGpu
      {gpu}
      {sparks}
      target={applied?.core ? { clock: applied.core.freq_mhz, voltage: applied.core.voltage_mv } : null}
      view={$liveGpuView}
    />
  </section>
{/snippet}

{#snippet commandProfiles()}
  <section class="band profiles-band" aria-labelledby="command-profiles-title">
    <div class="band-head">
      <h2 id="command-profiles-title">{profilesReady ? "Forged profiles" : "Profile goals"}</h2>
      <span class="band-note">{profilesReady ? "Measured on this GPU" : "Created after qualification"}</span>
      {#if profilesReady}
        <button class="band-link" type="button" aria-expanded={compareOpen} aria-controls="profile-compare" onclick={() => (compareOpen = !compareOpen)}>{compareOpen ? "Hide details" : "Compare details"} ›</button>
      {/if}
    </div>
    <div class="profile-columns" style={`--profile-count: ${displayedProfiles.length + 1}`}>
      {@render stockColumn()}
      {#each displayedProfiles as profile}{@render profileColumn(profile)}{/each}
    </div>
    {#if profilesReady && compareOpen}{@render profileCompare()}{/if}
  </section>
{/snippet}

{#snippet commandProgress()}
  <div class="band forge-band">
    <ForgeProgress
      {powerSweep}
      {powerRunning}
      {safeLoop}
      {forgeMode}
      {onStopPower}
      {onStartPower}
      {onRecoverContinue}
      {onResumePower}
      onOpenReport={() => navigate("report")}
      onOpenLog={() => navigate("log")}
    />
  </div>
{/snippet}

<section class="forge-theme-screen command" style={backgroundStyle}>
  <header class="command-header">
    <button class="brand command-brand" onclick={() => navigate("forge")} aria-label="Nidavellir Forge home">
      <img src={commandMark} alt="" />
      <span>NIDAVELLIR</span>
    </button>
    <nav class="command-nav" aria-label="Primary navigation">
      {#each [["forge", "Forge"], ["log", "Live log"], ["sentinel", "Sentinel"], ["settings", "Settings"]] as [view, label]}
        <!-- The Forge report is a page of the Forge, so Forge stays lit there. -->
        {@const current = activeView === view || (view === "forge" && activeView === "report")}
        <button class:active={current} aria-current={current ? "page" : undefined} onclick={() => navigate(view)}>{label}</button>
      {/each}
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
    <div class="command-page">
      <ForgeSettingsPage
        {powerSweep}
        {serviceReady}
        resetDisabled={fullResetDisabled}
        resetDisabledReason={fullResetDisabledReason}
        onOpenReset={openResetConfirmation}
      />
    </div>
  {:else if activeView === "report"}
    <div class="command-page"><ForgeProgress layout="report" {powerSweep} {powerRunning} {safeLoop} {forgeMode} /></div>
  {:else if activeView === "log" || activeView === "sentinel"}
    <div class="command-page">{@render children?.()}</div>
  {:else}
  <div class="command-body">
    <section class="command-hero" class:compact={progressPriority} class:attention={state === "ATTENTION"}>
      <span class="eyebrow hero-eyebrow">LOCAL NVIDIA GPU</span>
      <div class="command-identity">
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
        <div class="hero-options">
          <div class="hero-option">
            <span class="option-text">
              <span class="option-eyebrow">Run mode</span>
              <span class="option-value">{RUN_MODES[forgeMode] ?? RUN_MODES.standard}</span>
            </span>
            <!-- The native select sits over "Change": one click opens the mode list. -->
            <label class="mode-change" class:disabled={runModeDisabled}>
              <span aria-hidden="true">Change</span>
              <select id="command-run-mode" value={forgeMode} onchange={selectMode} disabled={runModeDisabled} aria-label="Run mode">
                <option value="standard">Standard · recommended</option>
                <option value="long">Long · extended qualification</option>
                <option value="clean">Clean Run · rebuild measurements</option>
              </select>
            </label>
          </div>
          <div class="hero-option"><AutoResumeToggle {powerSweep} disabled={!serviceReady} /></div>
        </div>
        <!-- Starting a Forge explains itself in the pre-Forge sheet; other actions say why here. -->
        <small id="primary-action-reason" class:sr-only={primaryAction.kind === "start"}>{primaryActionReason}</small>
        <SentinelAdvice {powerSweep} disabled={!serviceReady} />
      </div>
    </section>

    <PreForgeSheet
      bind:open={preForgeOpen}
      {actionLabel}
      {forgeMode}
      modeDisabled={runModeDisabled}
      {powerSweep}
      {serviceReady}
      onModeChange={(mode) => onForgeModeChange?.(mode)}
      onStart={() => onStartPower?.(forgeMode)}
    />

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

  <!-- On every page: the Core may restart Windows so a run can continue. -->
  <RestartWarning {powerSweep} />

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
      {#if resetMode === "full"}
        <label class="erase-confirm">
          <span>Type ERASE to confirm</span>
          <input bind:this={eraseInput} bind:value={eraseText} type="text" autocomplete="off" spellcheck="false" />
        </label>
      {/if}
      <div class="reset-dialog-actions">
        <button bind:this={resetCancelButton} class="reset-cancel" type="button" onclick={closeResetConfirmation} disabled={fullResetBusy}>Cancel</button>
        <button class="reset-confirm" type="button" onclick={confirmFullReset} disabled={fullResetBusy || !resetConfirmed}>
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
    /* clip, not hidden: hidden would make this a scroll box and unstick the header from the window. */
    overflow-x: clip;
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

  .reset-confirm:hover:not(:disabled) {
    border-color: #d27361;
    background: rgba(145, 50, 35, 0.14);
    color: #f0b1a4;
  }

  .reset-dialog-actions button:focus-visible,
  .reset-feedback button:focus-visible {
    outline: 2px solid #cf8f79;
    outline-offset: 3px;
  }

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

  .erase-confirm {
    display: grid;
    gap: 8px;
    margin-top: 20px;
  }

  .erase-confirm span {
    color: #a5aaad;
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }

  .erase-confirm input {
    height: 40px;
    border: 1px solid rgba(255, 255, 255, 0.22);
    border-radius: 6px;
    padding: 0 12px;
    background: rgba(3, 5, 6, 0.6);
    color: #eceeef;
    font: inherit;
    font-size: 15px;
    letter-spacing: 0.08em;
  }

  .erase-confirm input:focus-visible {
    outline: 2px solid #c5864f;
    outline-offset: 1px;
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
    /* The menus stay at the top of the window while a page scrolls (user request 2026-10-23). */
    position: sticky;
    top: 0;
    z-index: 30;
    display: grid;
    /* The nav takes its own width (four items); the status items share what is left. */
    grid-template-columns: max-content max-content minmax(0, 1fr);
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
    gap: 2px;
    padding-right: 18px;
  }

  .command-nav button {
    position: relative;
    border: 0;
    padding: 0 13px;
    white-space: nowrap;
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
    --body-gutter: clamp(20px, 2.35vw, 36px);
    padding: 20px var(--body-gutter) 40px;
  }

  /* Sections are full-width darker bands, so they stay distinct while their items stay seamless. */
  .band {
    margin-inline: calc(-1 * var(--body-gutter));
    border-top: 1px solid rgba(164, 171, 177, 0.3);
    padding: 22px var(--body-gutter) 28px;
    background: rgba(5, 8, 9, 0.5);
  }

  .band-head {
    display: flex;
    align-items: baseline;
    gap: 18px;
    margin-bottom: 18px;
  }

  .band-head h2 {
    margin: 0;
    color: #e9ba79;
    font-size: 13px;
    font-weight: 700;
    letter-spacing: 0.14em;
    text-transform: uppercase;
    white-space: nowrap;
  }

  .band-note {
    margin-left: auto;
    color: #8f979c;
    font-size: 13px;
    white-space: nowrap;
  }

  .band-link {
    border: 0;
    padding: 0;
    background: transparent;
    color: #e0a96b;
    font: inherit;
    font-size: 13px;
    white-space: nowrap;
    cursor: pointer;
  }

  .band-link:hover { color: #f0c48c; }

  .profile-columns {
    display: grid;
    grid-template-columns: repeat(var(--profile-count, 4), minmax(0, 1fr));
  }

  .profile-column {
    position: relative;
    display: flex;
    min-width: 0;
    flex-direction: column;
    border-left: 1px solid rgba(255, 255, 255, 0.1);
    padding: 16px 24px 18px;
  }

  /* Stock keeps the same inner margin: the applied highlight boxes any column. */
  .profile-column:first-child {
    border-left: 0;
  }

  .profile-tag {
    min-height: 16px;
    color: #8f979c;
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }

  .profile-tag.recommended { color: #e3a83b; }

  .profile-name {
    margin: 6px 0 0;
    color: #eceeef;
    font-size: 19px;
    font-weight: 600;
  }

  .rune {
    display: inline-block;
    width: 30px;
    margin-right: 4px;
    color: #c5864f;
    font-family: "Segoe UI Historic", "Segoe UI", sans-serif;
    font-size: 30px;
    font-weight: 400;
    line-height: 1;
    vertical-align: -4px;
  }

  .profile-clock {
    margin-top: 10px;
    color: #f0f0ef;
    font-size: 34px;
    font-weight: 500;
    font-variant-numeric: tabular-nums;
    line-height: 1;
    letter-spacing: -0.02em;
  }

  .profile-clock span {
    margin-left: 5px;
    color: #8f979c;
    font-size: 14px;
    letter-spacing: 0;
  }

  .profile-facts {
    margin-top: 8px;
    color: #b8bbbe;
    font-size: 14px;
  }

  .profile-delta {
    min-height: 19px;
    margin-top: 4px;
    color: #8f979c;
    font-size: 13px;
  }

  .delta-up { color: #e0a96b; }
  .delta-save { color: #8fc04a; }

  .profile-bar {
    height: 2px;
    margin: 14px 0 16px;
    background: rgba(255, 255, 255, 0.1);
  }

  .profile-bar i {
    display: block;
    height: 2px;
    background: #7c858a;
  }

  .profile-apply {
    align-self: flex-start;
    min-height: 34px;
    border: 1px solid rgba(197, 134, 79, 0.6);
    border-radius: 6px;
    padding: 0 16px;
    background: rgba(10, 12, 13, 0.35);
    color: #e9ba79;
    font: inherit;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }

  .profile-apply.ghost {
    border-color: rgba(174, 181, 186, 0.35);
    color: #c3c8cb;
  }

  .profile-apply:hover:not(:disabled) { background: rgba(197, 134, 79, 0.12); }
  .profile-apply:disabled { cursor: not-allowed; opacity: 0.45; }

  .profile-goal {
    margin: 10px 0 0;
    color: #8f979c;
    font-size: 13.5px;
    line-height: 1.45;
    text-wrap: pretty;
  }

  .profile-pending {
    margin-top: auto;
    padding-top: 14px;
    color: #c58a51;
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }

  /* The applied profile looks like heated metal: warm light from a glowing top edge. */
  .profile-column.applied {
    background: linear-gradient(180deg, rgba(197, 134, 79, 0.2) 0%, rgba(197, 134, 79, 0.07) 42%, rgba(197, 134, 79, 0) 88%);
  }

  .profile-column.applied::before {
    position: absolute;
    top: 0;
    right: 0;
    left: 0;
    height: 2px;
    background: #d6935a;
    box-shadow: 0 0 14px rgba(233, 186, 121, 0.55);
    content: "";
  }

  .profile-column.applied .profile-name { color: #e9ba79; }
  .profile-column.applied .rune { color: #f2c48c; text-shadow: 0 0 12px rgba(242, 196, 140, 0.65); }
  .profile-column.applied .profile-clock { color: #f6dcb6; }
  .profile-column.applied .profile-bar i { background: #c5864f; }

  .applied-row {
    display: flex;
    min-height: 34px;
    align-items: center;
    gap: 12px;
  }

  .applied-plate {
    position: relative;
    display: inline-grid;
    width: 124px;
    height: 34px;
    flex: 0 0 auto;
    place-items: center;
  }

  .applied-plate img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: fill;
    filter: saturate(0.85) brightness(1.08);
  }

  .applied-plate span {
    position: relative;
    color: #1b130c;
    font-size: 12px;
    font-weight: 800;
    letter-spacing: 0.16em;
    text-transform: uppercase;
  }

  .running {
    color: #c9a77f;
    font-size: 12.5px;
    white-space: nowrap;
  }

  .running i {
    display: inline-block;
    width: 7px;
    height: 7px;
    margin-right: 6px;
    border-radius: 50%;
    background: #e9ba79;
    box-shadow: 0 0 0 3px rgba(233, 186, 121, 0.18);
    vertical-align: 1px;
    animation: running-pulse 2.4s ease-in-out infinite;
  }

  @keyframes running-pulse {
    50% { box-shadow: 0 0 0 5px rgba(233, 186, 121, 0.05); }
  }

  .profile-compare {
    margin-top: 18px;
    border-top: 1px solid rgba(255, 255, 255, 0.08);
    padding-top: 6px;
    overflow-x: auto;
  }

  .profile-compare table {
    width: 100%;
    border-collapse: collapse;
    font-size: 13.5px;
    font-variant-numeric: tabular-nums;
  }

  .profile-compare th,
  .profile-compare td {
    border-bottom: 1px solid rgba(255, 255, 255, 0.06);
    padding: 9px 12px;
    text-align: left;
  }

  .profile-compare thead th {
    color: #a5aaad;
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }

  .profile-compare tbody th {
    color: #8f979c;
    font-weight: 500;
  }

  .profile-compare td { color: #d8dbde; }

  .field-failure {
    min-height: 30px;
    border: 1px solid rgba(226, 101, 106, 0.4);
    border-radius: 6px;
    padding: 0 12px;
    background: transparent;
    color: #e89a9e;
    font: inherit;
    font-size: 12.5px;
    cursor: pointer;
  }

  .field-failure:disabled { cursor: not-allowed; opacity: 0.45; }

  /* Pages (report, Live log, Sentinel, Settings): bands bleed to the edges through --page-gutter. */
  .command-page {
    --page-gutter: 36px;
    min-height: calc(100vh - 88px);
    padding: 28px var(--page-gutter) 44px;
  }

  /* No box: the hero sits on the forge texture. The button column starts at the GPU name, below
     the eyebrow, and its options end with the text on the left. */
  .command-hero {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 310px;
    grid-template-rows: auto auto;
    align-content: center;
    column-gap: clamp(40px, 5vw, 64px);
    min-height: 224px;
    padding: 6px 0 4px;
  }

  .command-hero.compact { min-height: 156px; }

  .hero-eyebrow {
    grid-column: 1;
    grid-row: 1;
  }

  .command-identity {
    grid-column: 1;
    grid-row: 2;
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
    grid-column: 2;
    grid-row: 2;
    min-width: 0;
    flex-direction: column;
    padding-top: 6px;
  }

  .hero-options {
    margin-top: auto;
    padding-top: 16px;
  }

  .hero-option {
    display: flex;
    align-items: center;
    gap: 12px;
    border-top: 1px solid rgba(255, 255, 255, 0.12);
    padding: 11px 2px;
  }

  .hero-option:last-child { border-bottom: 1px solid rgba(255, 255, 255, 0.12); }
  .hero-option > :global(.auto-resume) { flex: 1; min-width: 0; }

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
  }

  .mode-change {
    position: relative;
    color: #e0a96b;
    font-size: 13px;
    cursor: pointer;
  }

  .mode-change select {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    opacity: 0;
    cursor: pointer;
  }

  .mode-change:hover { color: #f0c48c; }
  .mode-change:focus-within span { outline: 2px solid #c5864f; outline-offset: 3px; }
  .mode-change.disabled { color: #6f787e; cursor: not-allowed; }
  .mode-change.disabled select { cursor: not-allowed; }

  .command-cta > small {
    margin-top: 10px;
    color: #8f979c;
    font-size: 12px;
    line-height: 1.45;
    text-wrap: pretty;
  }

  .plate-button {
    position: relative;
    width: 100%;
    height: 72px;
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
    font-size: 20px;
    font-weight: 600;
  }

  .plate-button:disabled {
    cursor: not-allowed;
    filter: grayscale(0.55);
    opacity: 0.54;
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

  /* Return to stock sits after the divider instead of taking the divider's column. */

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

  .command button:focus-visible,
  .command select:focus-visible {
    outline: 2px solid #cf955d;
    outline-offset: 3px;
  }

  .command-body :global(.forge-progress) {
    --progress-surface: rgba(7, 10, 12, 0.58);
    --progress-outline: rgba(126, 136, 143, 0.34);
    min-width: 0;
    border-radius: 11px;
  }

  @media (max-width: 1180px) {
    .command-brand { padding-inline: 20px; }
    .command-nav { padding-right: 12px; }
    .command-nav button { padding-inline: 10px; }
    .system-item { padding-inline: 10px; }
    .command-hero { grid-template-columns: minmax(0, 1fr) 300px; }
  }

  @media (max-width: 820px) {
    .reset-dialog { padding: 20px; }
    .reset-dialog-actions { flex-direction: column-reverse; }
    .reset-dialog-actions button { width: 100%; }
    .reset-confirm { min-width: 0; }
    .reset-feedback { right: 12px; bottom: 12px; width: calc(100% - 24px); }
    .command-header { grid-template-columns: auto minmax(0, 1fr); height: auto; min-height: 82px; }
    .command-brand { padding-left: 20px; }
    .command-nav { justify-self: end; overflow-x: auto; height: 58px; padding-right: 10px; }
    .command-system-status { grid-column: 1 / -1; min-height: 64px; border-top: 1px solid rgba(255, 255, 255, 0.08); border-left: 0; padding: 8px 12px; }
    .command-page { --page-gutter: 18px; min-height: calc(100vh - 146px); padding: 22px var(--page-gutter) 36px; }
    .command-hero { grid-template-columns: 1fr; }
    .command-cta { grid-column: 1; grid-row: 3; max-width: 360px; padding-top: 18px; }
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
    .plate-button { width: min(100%, 320px); }
  }

  @media (prefers-reduced-motion: reduce) {
    .reset-dialog-actions button,
  .plate-button,
  .command-alert button,
  .command-nav button {
      transition: none;
    }

    .plate-button:active:not(:disabled) {
      transform: none;
    }

    .running i { animation: none; }
  }
</style>
