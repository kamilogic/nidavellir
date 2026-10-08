<script>
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { open } from "@tauri-apps/plugin-shell";
  import { serviceCall } from "../service.js";
  import { nvidiaGpu, recoverForge, requireServiceData } from "../forge-workflow.js";
  import AdvancedDiagnosticsHub from "../components/forge/AdvancedDiagnosticsHub.svelte";
  import ForgeThemeScreen from "../components/forge/ForgeThemeScreen.svelte";
  import StartupScreen from "../components/forge/StartupScreen.svelte";
  import UpdateButton from "../components/forge/UpdateButton.svelte";
  import { updateHold } from "../updates.js";
  import { windowVisible } from "../visibility.js";

  let { theme = "command", onThemeChange } = $props();

  let error = $state(null);
  let serviceStatus = $state("connecting");
  let serviceError = $state(null);
  let hardwareError = $state(null);
  let timer = $state(null);
  let hardware = $state(null);
  let safeLoop = $state(null);
  let activeView = $state("forge");
  let diagnosticsTab = $state("log");
  let applied = $state(null);
  let exporting = $state(false);
  let exportMsg = $state("");
  let exportFailed = $state(false);
  // v17 sentinel: last automatic action + recommendation (sentinel_status.json via IPC).
  let sentinel = $state(null);
  // Game-trace: read-only NVML/NVAPI workload logger.
  let gameTrace = $state(null);
  let gameTraceBusy = $state(false);
  let gameTraceActionError = $state("");
  let gameTraceExportBusy = $state(false);
  let gameTraceExportMsg = $state("");
  let manualPoint = $state(null);
  let manualPointBusy = $state(false);
  let manualPointActionError = $state("");
  let detectorLab = $state(null);
  let detectorLabBusy = $state(false);
  let detectorLabActionError = $state("");
  let fullResetBusy = $state(false);
  let actionBusy = $state(false);
  let fullResetFeedback = $state(null);
  // Live GPU telemetry (ReadSensors) + rolling sparkline buffers for the monitoring panel.
  let sensors = $state(null);
  let sparks = $state({ core: [], mem: [], temp: [], power: [], fan: [], voltage: [], usage: [] });
  const SPARK_CAP = 20;
  let powerSweep = $state(null);
  let forgeMode = $state("standard");
  let resetCleanRunArmed = $state(false);
  let hardwareLoading = false;
  let lastHardwareAttemptAt = 0;
  let refreshInFlight = false;
  let lastSlowRefreshAt = 0;
  // Offline means the Core is gone, not starting or busy: a starting Core needs a few seconds and an
  // apply holds its single pipe for ~9 s. Until then a failed read keeps the last state.
  const OFFLINE_GRACE_MS = 20_000;
  let lastContactAt = Date.now();
  // Startup screen: the Core starts, then the program's first heartbeat reapplies the profile (~10 s
  // with one). It ends with the first status read after that, so the window never steps through
  // Offline and the reapply's armed Safe Loop on the way.
  const STARTUP_LIMIT_MS = 25_000;
  let booting = $state(true);
  let coreAnswered = $state(false);
  // What the program's first heartbeat does: reapply this profile name, or only confirm stock
  // (null). Undefined when this window did not see that heartbeat start.
  let sessionProfile = $state(undefined);
  let sessionReadyAt = $state(0);
  let snapshotAt = $state(0);

  const powerRunning = $derived(Boolean(powerSweep?.running));
  // Updating restarts the Core: a run blocks it, and a saved run cannot resume afterwards.
  $effect(() => {
    updateHold.set({ forgeBusy: powerRunning, savedRun: ["paused", "interrupted"].includes(powerSweep?.phase) });
  });

  function responseData(response, type, label) {
    return requireServiceData(response, type, label);
  }

  async function loadHardware(force = false) {
    const now = Date.now();
    if (hardware || hardwareLoading || (!force && now - lastHardwareAttemptAt < 3000)) return;
    hardwareLoading = true;
    lastHardwareAttemptAt = now;
    try {
      const hw = await serviceCall("DetectHardware");
      hardware = responseData(hw, "Hardware", "hardware detection");
      hardwareError = null;
    } catch (e) {
      hardwareError = String(e);
    } finally {
      hardwareLoading = false;
    }
  }

  // Without a run nothing changes fast: the timer refreshes every 2 s instead of every 500 ms.
  // Explicit refreshes after an action always run.
  const IDLE_REFRESH_MS = 2000;
  let lastRefreshAt = 0;
  function tick() {
    if (powerRunning || Date.now() - lastRefreshAt >= IDLE_REFRESH_MS) void refresh();
  }

  async function refresh(forceSlow = false) {
    if (refreshInFlight || fullResetBusy) return;
    refreshInFlight = true;
    lastRefreshAt = Date.now();
    try {
      const now = Date.now();
      const slowDue = forceSlow || !powerRunning || now - lastSlowRefreshAt >= 3000;
      const [ps, sl] = await Promise.all([
        serviceCall("GetPowerSweepProgress"),
        serviceCall("GetSafeLoopStatus"),
      ]);
      powerSweep = responseData(ps, "PowerSweep", "Forge status");
      safeLoop = responseData(sl, "SafeLoop", "Safe Loop status");
      if (slowDue) {
        const ap = await serviceCall("GetAppliedProfile");
        applied = responseData(ap, "GpuApply", "applied profile");
        lastSlowRefreshAt = now;
        // First contact identifies the GPU at once instead of after the retry pause.
        void loadHardware(serviceStatus !== "online");
      }
      serviceStatus = "online";
      serviceError = null;
      lastContactAt = Date.now();
      snapshotAt = now;
    } catch (e) {
      // A pipe that vanished after contact means the Core stopped: offline at once.
      if ((serviceStatus === "online" && /unavailable/i.test(String(e))) || Date.now() - lastContactAt >= OFFLINE_GRACE_MS) {
        serviceStatus = "offline";
        serviceError = String(e);
      }
    } finally {
      refreshInFlight = false;
    }
  }

  async function call(method, set) {
    try {
      const r = await serviceCall(method);
      if (r?.ok === false) throw new Error(r.error || `${method} failed`);
      set(r);
      error = null;
    } catch (e) {
      error = String(e);
    }
  }

  const setApplied = (r) => (applied = responseData(r, "GpuApply", "GPU apply"));
  const resetTuning = async () => {
    if (actionBusy || fullResetBusy || serviceStatus !== "online") return;
    const confirmed = globalThis.confirm?.(
      "Return the GPU to stock? The saved run and safety history stay preserved. A pending incident still requires recovery acknowledgement.",
    ) ?? true;
    if (!confirmed) return;
    actionBusy = true;
    try {
      await call("ResetGpuTuning", setApplied);
      await refresh();
    } finally {
      actionBusy = false;
    }
  };
  async function refreshForgeStateAfterReset() {
    const [ps, sl, ap] = await Promise.all([
      serviceCall("GetPowerSweepProgress"),
      serviceCall("GetSafeLoopStatus"),
      serviceCall("GetAppliedProfile"),
    ]);
    const failed = [ps, sl, ap].find((response) => response?.ok === false);
    if (failed) throw new Error(failed.error || "Unable to refresh Forge state");
    if (ps?.data?.type !== "PowerSweep") throw new Error("Invalid Forge status response");
    if (sl?.data?.type !== "SafeLoop") throw new Error("Invalid Safe Loop status response");
    if (ap?.data?.type !== "GpuApply") throw new Error("Invalid applied profile response");
    powerSweep = ps.data;
    safeLoop = sl.data;
    applied = ap.data;
    serviceStatus = "online";
    serviceError = null;
    lastContactAt = lastSlowRefreshAt = Date.now();
    void refreshSentinel();
  }

  async function fullResetTuning(mode = "full") {
    const full = mode !== "soft";
    const resetName = full ? "Full Reset" : "Soft Reset";
    if (fullResetBusy || actionBusy) return false;
    if (serviceStatus !== "online" || safeLoop?.gpu_reboot_required) {
      fullResetFeedback = {
        tone: "error",
        message: safeLoop?.gpu_reboot_required
          ? "Restart Windows once before any tuning reset. The failed point and Forge learning are already saved."
          : `Core Service must be online before ${resetName}.`,
      };
      return false;
    }
    fullResetBusy = true;
    fullResetFeedback = {
      tone: "progress",
      title: `${resetName} in progress`,
      message: "Restoring the GPU to stock and clearing saved learning. Please wait.",
    };
    try {
      const response = await serviceCall(full ? "ResetGpuTuningFull" : "ResetGpuTuningSoft");
      if (response?.ok === false) {
        throw new Error(response.error || "Unable to reset all GPU learning");
      }
      if (response?.data?.type !== "GpuApply") {
        throw new Error("Invalid full reset response");
      }

      applied = response.data;
      const message = response.data.message || `${resetName} completed`;
      if (/^reset failed/i.test(message)) throw new Error(message);
      if (full && /negative safety evidence preserved/i.test(message)) {
        throw new Error("The running Core still uses the old reset behavior. Close it and reopen your launcher to load the Full Reset update.");
      }
      const partial = /some state could not be cleared/i.test(message);
      resetCleanRunArmed = false;
      if (!partial) forgeMode = "clean";
      const feedbackMessage = partial ? message : `${message}. Checking whether another Forge can start.`;
      fullResetFeedback = {
        tone: partial ? "warning" : "success",
        message: feedbackMessage,
      };

      try {
        await refreshForgeStateAfterReset();
        if (!partial) {
          const blocked = Boolean(powerSweep?.start_block_reason);
          resetCleanRunArmed = !blocked;
          fullResetFeedback = {
            tone: blocked ? "warning" : "success",
            title: blocked ? "Reset completed; tuning blocked" : "Reset completed",
            message: blocked
              ? `${message}. Automatic tuning is still blocked. Choose Review safety block for the remaining requirement.`
              : `${message}. The next Forge is prepared as a Clean Run. ${full ? "No saved GPU learning will be reused." : "Known failures stay preserved."}`,
          };
        }
      } catch (refreshError) {
        fullResetFeedback = {
          tone: "warning",
          message: `${feedbackMessage}. The reset ran, but the UI could not refresh immediately: ${String(refreshError)}`,
        };
      }
      return true;
    } catch (resetError) {
      fullResetFeedback = {
        tone: "error",
        message: `${resetName} failed: ${String(resetError)}`,
      };
      return false;
    } finally {
      fullResetBusy = false;
    }
  }
  const setPower = (r) => (powerSweep = responseData(r, "PowerSweep", "Forge"));
  const POWER_START = {
    standard: "StartPowerSweep",
    long: "StartPowerSweepLong",
    clean: "StartPowerSweepClean",
  };
  const selectForgeMode = (mode) => {
    forgeMode = ["standard", "long", "clean"].includes(mode) ? mode : "standard";
    if (mode !== "clean") resetCleanRunArmed = false;
  };
  const requireWindowsRestart = () => {
    error = "The GPU driver recovered and Nidavellir stopped the test. Restart Windows once to continue; the failed point and Forge learning are already saved.";
  };
  const startPower = async (mode = forgeMode) => {
    if (actionBusy || fullResetBusy || powerRunning) return;
    if (serviceStatus !== "online") {
      error = "Core Service is not ready. Start the elevated service before forging.";
      return;
    }
    if (!nvidiaGpu(hardware) || !safeLoop) {
      error = "Nidavellir is still confirming the local GPU and Safe Loop state.";
      return;
    }
    if (safeLoop?.gpu_reboot_required) return requireWindowsRestart();
    if (safeLoop?.recovery_pending_ack) return recoverAndStartPower(mode);
    if (safeLoop.safe_mode || safeLoop.boot_flag_armed || safeLoop.state === "unstable") {
      error = "Return to stock before starting Forge.";
      return;
    }
    if (powerSweep?.start_block_reason) {
      error = powerSweep.start_block_reason;
      return;
    }
    actionBusy = true;
    try {
      await call(POWER_START[mode] ?? POWER_START.standard, setPower);
      await refresh(true);
    } finally {
      actionBusy = false;
    }
  };
  const recoverAndStartPower = async () => {
    if (actionBusy || fullResetBusy || powerRunning) return;
    if (serviceStatus !== "online") {
      error = "Core Service is not ready. The interrupted run remains preserved.";
      return;
    }
    if (safeLoop?.gpu_reboot_required) return requireWindowsRestart();
    const incident = safeLoop?.pending_forge_incident;
    const point = incident?.target_mhz && incident?.anchor_mv
      ? ` Candidate context: ${incident.target_mhz} MHz at ${incident.anchor_mv} mV VF bin.`
      : " No candidate will be inferred because the interrupted point was not attributable.";
    const confirmed = globalThis.confirm?.(
      `Recover Forge? Nidavellir will return the GPU to stock and acknowledge the incident while preserving safety history. It will resume only if the saved run is compatible. Otherwise it stays at stock; a new run requires a separate action.${point}`,
    ) ?? true;
    if (!confirmed) return;
    actionBusy = true;
    try {
      const result = await recoverForge(serviceCall);
      applied = result.applied;
      safeLoop = result.safeLoop;
      powerSweep = result.powerSweep;
      fullResetFeedback = { tone: "success", title: "Forge recovery", message: result.message };
      error = null;
      await refresh();
    } catch (e) {
      error = String(e);
      await refresh();
    } finally {
      actionBusy = false;
    }
  };
  const REPORT_UNSTABLE = {
    godforge: "ReportPowerGodforgeUnstable",
    brokkrs: "ReportPowerBrokkrsUnstable",
    deep_calm: "ReportPowerDeepCalmUnstable",
  };
  const reportProfileUnstable = async (key) => {
    const point = powerSweep?.[key];
    if (!point) return;
    const target = point.target_clock_mhz ?? point.clock_mhz;
    const anchor = point.vf_table_voltage_mv ?? point.boundary_voltage_mv ?? point.voltage_mv;
    const confirmed = globalThis.confirm?.(
      `Confirm repeated real-use instability at ${target} MHz · ${anchor} mV VF bin? This records a durable, hardware-local blacklist point and invalidates the current profile set.`,
    ) ?? true;
    if (!confirmed) return;
    try {
      const response = await serviceCall(REPORT_UNSTABLE[key]);
      setPower(response);
      await refresh(true);
      error = null;
    } catch (e) {
      error = String(e);
    }
  };
  const stopPower = async () => {
    if (!powerSweep?.running || powerSweep?.phase === "stopping") return;
    powerSweep = {
      ...powerSweep,
      phase: "stopping",
      note: "Stopping Forge and restoring stock safely…",
    };
    await call("StopPowerSweep", setPower);
  };
  const resumePower = async () => {
    if (actionBusy || fullResetBusy || powerSweep?.running || !powerSweep?.resume_available) return;
    if (powerSweep.start_block_reason) { error = powerSweep.start_block_reason; return; }
    if (serviceStatus !== "online") {
      error = "Core Service is not ready. The paused run remains preserved.";
      return;
    }
    if (safeLoop?.gpu_reboot_required) return requireWindowsRestart();
    actionBusy = true;
    try {
      await call("ResumePowerSweep", setPower);
      await refresh(true);
    } finally {
      actionBusy = false;
    }
  };
  const POWER_APPLY = {
    godforge: "ApplyPowerGodforge",
    brokkrs: "ApplyPowerBrokkrs",
    deep_calm: "ApplyPowerDeepCalm",
  };
  const applyPower = async (which) => {
    if (actionBusy || fullResetBusy || powerRunning) return;
    if (serviceStatus !== "online" || !safeLoop || safeLoop.gpu_reboot_required || safeLoop.safe_mode || safeLoop.state === "unstable") {
      error = safeLoop?.gpu_reboot_required
        ? "Restart Windows once before applying a profile."
        : "Safe Loop and Core Service must be ready before applying a profile.";
      return;
    }
    actionBusy = true;
    try {
      await call(POWER_APPLY[which], setApplied);
    } finally {
      actionBusy = false;
    }
  };

  async function exportLog() {
    if (exporting) return;
    exporting = true;
    exportMsg = "";
    exportFailed = false;
    try {
      const r = await serviceCall("ExportForgeLog");
      if (r?.data?.type === "ForgeLogExport") {
        exportMsg = `${r.data.note} → ${r.data.path}`;
        error = null;
      } else {
        exportFailed = true;
        exportMsg = r?.error ? String(r.error) : "Unable to export the Forge log.";
      }
    } catch (e) {
      exportFailed = true;
      error = String(e);
      exportMsg = String(e);
    } finally {
      exporting = false;
    }
  }

  async function refreshSentinel() {
    try {
      const r = await serviceCall("GetSentinelStatus");
      if (r?.ok === false) throw new Error(r.error || "Unable to read Sentinel status");
      if (r?.data?.type === "SentinelStatus") {
        sentinel = r.data.status ? JSON.parse(r.data.status) : null;
      }
    } catch {
      /* sentinel status is best-effort UI info */
    }
  }

  async function refreshGameTrace(clearRecoveredError = true) {
    try {
      const r = await serviceCall("GetGameTraceStatus");
      if (r?.ok === false) throw new Error(r.error || "Unable to read Game Trace status");
      if (r?.data?.type === "GameTrace") {
        gameTrace = r.data;
        if (clearRecoveredError && !gameTraceBusy) gameTraceActionError = "";
      }
    } catch {
      /* game-trace status is best-effort UI info */
    }
  }

  async function toggleGameTrace() {
    if (gameTraceBusy) return;
    gameTraceBusy = true;
    gameTraceActionError = "";
    try {
      const method = gameTrace?.running ? "StopGameTrace" : "StartGameTrace";
      const r = await serviceCall(method);
      if (r?.ok === false) throw new Error(r.error || "Unable to update Game Trace");
      if (r?.data?.type !== "GameTrace") throw new Error("Invalid Game Trace response");
      gameTrace = r.data;
      gameTraceExportMsg = "";
    } catch (e) {
      gameTraceActionError = String(e);
    } finally {
      await refreshGameTrace(false);
      gameTraceBusy = false;
    }
  }

  async function openGameTraceLog() {
    if (!gameTrace?.out_path || gameTraceExportBusy) return;
    gameTraceExportBusy = true;
    gameTraceExportMsg = "";
    try {
      await open(gameTrace.out_path);
      gameTraceExportMsg = "The exported JSONL was opened with your default application.";
    } catch (e) {
      gameTraceExportMsg = `Unable to open the exported log: ${String(e)}`;
    } finally {
      gameTraceExportBusy = false;
    }
  }

  async function refreshManualPoint(clearRecoveredError = true) {
    try {
      const r = await serviceCall("GetManualDiagnosticPointStatus");
      if (r?.ok === false) throw new Error(r.error || "Unable to read manual point status");
      if (r?.data?.type === "ManualDiagnosticPoint") {
        manualPoint = r.data;
        if (clearRecoveredError && !manualPointBusy) manualPointActionError = "";
      }
    } catch {
      /* diagnostic status is best-effort UI information */
    }
  }

  async function applyManualPoint(config) {
    if (manualPointBusy || manualPoint?.active) return;
    const confirmed = globalThis.confirm?.(
      `Apply the temporary diagnostic point ${config.target_mhz} MHz @ ${config.voltage_mv} mV? Clock may step down, but the resolved voltage will be locked. It may cause a driver reset during real workloads. The point will not become a profile; use Return to stock when the test is finished.`,
    ) ?? true;
    if (!confirmed) return;
    manualPointBusy = true;
    manualPointActionError = "";
    try {
      const r = await serviceCall("ApplyManualDiagnosticPoint", config);
      if (r?.ok === false) throw new Error(r.error || "Unable to apply the manual point");
      if (r?.data?.type !== "ManualDiagnosticPoint") throw new Error("Invalid manual point response");
      manualPoint = r.data;
    } catch (e) {
      manualPointActionError = String(e);
    } finally {
      await refreshManualPoint(false);
      manualPointBusy = false;
    }
  }

  async function resetManualPoint() {
    if (manualPointBusy) return;
    manualPointBusy = true;
    manualPointActionError = "";
    try {
      const r = await serviceCall("ResetManualDiagnosticPoint");
      if (r?.ok === false) throw new Error(r.error || "Unable to return the GPU to stock");
      if (r?.data?.type === "ManualDiagnosticPoint") manualPoint = r.data;
    } catch (e) {
      manualPointActionError = String(e);
    } finally {
      await refreshManualPoint(false);
      manualPointBusy = false;
    }
  }

  async function refreshDetectorLab(clearRecoveredError = true) {
    try {
      const r = await serviceCall("GetDetectorLabStatus");
      if (r?.ok === false) throw new Error(r.error || "Unable to read Detector Lab status");
      if (r?.data?.type === "DetectorLab") {
        detectorLab = r.data;
        if (clearRecoveredError && !detectorLabBusy) detectorLabActionError = "";
      }
    } catch {
      /* experimental diagnostic status is best-effort UI information */
    }
  }

  async function startDetectorLab(config) {
    if (detectorLabBusy || detectorLab?.running || !manualPoint?.active) return;
    const recipeLabel = config.recipe === "dense_v14" ? "v14 dense candidate" : "v25 control";
    const confirmed = globalThis.confirm?.(
      `Run ${recipeLabel} for ${config.duration_s} seconds at ${manualPoint.target_mhz} MHz @ ${manualPoint.resolved_voltage_mv} mV? A marginal point may cause a TDR. Detector Lab will not qualify a profile or write blacklist.`,
    ) ?? true;
    if (!confirmed) return;
    detectorLabBusy = true;
    detectorLabActionError = "";
    try {
      const r = await serviceCall("StartDetectorLab", config);
      if (r?.ok === false) throw new Error(r.error || "Unable to start Detector Lab");
      if (r?.data?.type !== "DetectorLab") throw new Error("Invalid Detector Lab response");
      detectorLab = r.data;
    } catch (e) {
      detectorLabActionError = String(e);
    } finally {
      await Promise.all([refreshDetectorLab(false), refreshManualPoint(false)]);
      detectorLabBusy = false;
    }
  }

  async function stopDetectorLab() {
    if (detectorLabBusy || !detectorLab?.running) return;
    detectorLabBusy = true;
    detectorLabActionError = "";
    try {
      const r = await serviceCall("StopDetectorLab");
      if (r?.ok === false) throw new Error(r.error || "Unable to stop Detector Lab");
      if (r?.data?.type === "DetectorLab") detectorLab = r.data;
    } catch (e) {
      detectorLabActionError = String(e);
    } finally {
      await refreshDetectorLab(false);
      detectorLabBusy = false;
    }
  }

  async function openDetectorLabLog() {
    if (!detectorLab?.out_path) return;
    try {
      await open(detectorLab.out_path);
    } catch (e) {
      detectorLabActionError = `Unable to open the Detector Lab journal: ${String(e)}`;
    }
  }

  function closeAdvancedDiagnostics() {
    changeView("forge");
  }

  function changeView(view) {
    activeView = view === "advanced" || view === "settings" ? view : "forge";
    requestAnimationFrame(() => {
      window.scrollTo({ top: 0, behavior: "auto" });
      document.querySelector("[data-forge-heading]")?.focus({ preventScroll: true });
    });
  }

  function pushSpark(arr, value) {
    const n = Number(value);
    if (!Number.isFinite(n)) return arr;
    const next = [...arr, n];
    return next.length > SPARK_CAP ? next.slice(next.length - SPARK_CAP) : next;
  }

  async function refreshSensors() {
    try {
      const r = await serviceCall("ReadSensors");
      if (r?.data?.type !== "Sensors") return;
      sensors = r.data;
      const g = sensors.gpu?.[0];
      if (!g) return;
      sparks = {
        core: pushSpark(sparks.core, g.core_clock_mhz),
        mem: pushSpark(sparks.mem, g.memory_clock_mhz),
        temp: pushSpark(sparks.temp, g.temperature_c),
        power: pushSpark(sparks.power, g.power_w),
        fan: pushSpark(sparks.fan, g.fan_speed_pct),
        voltage: pushSpark(sparks.voltage, g.voltage_mv),
        usage: pushSpark(sparks.usage, g.utilization_pct),
      };
    } catch {
      /* telemetry is best-effort UI info */
    }
  }

  const primarySensorGpu = $derived(sensors?.gpu?.[0] ?? null);
  const logLines = $derived(powerSweep?.log ?? []);
  const sentinelState = $derived.by(() => {
    if (!sentinel) return "No events";
    if (sentinel.action === "bump") return "Automatic adjustment";
    if (String(sentinel.action ?? "").startsWith("stock")) return "Returned to stock";
    return "Event recorded";
  });
  // Sentinel status records differ by event; a TDR record carries no tuning pair.
  const sentinelSummary = $derived.by(() => {
    if (!sentinel) return "No automatic recovery action recorded.";
    const pair = sentinel.target_mhz && sentinel.failed_mv ? `${sentinel.target_mhz} MHz @ ${sentinel.failed_mv} mV` : null;
    if (sentinel.action === "bump" && pair) {
      return `Kept ${sentinel.target_mhz} MHz and moved the unstable point from ${sentinel.failed_mv} to ${sentinel.new_mv} mV (strike ${sentinel.strike}/3).`;
    }
    if (sentinel.event === "tdr") return `The GPU driver crashed${pair ? ` at ${pair}` : ""}; the GPU returned to stock.`;
    return pair ? `Returned the GPU to stock after a failure at ${pair}.` : "Returned the GPU to stock after a stability event.";
  });

  $effect(() => {
    if (
      resetCleanRunArmed &&
      !powerRunning &&
      powerSweep?.learning === "clean_run" &&
      ["finished", "provisional"].includes(powerSweep?.phase)
    ) {
      forgeMode = "standard";
      resetCleanRunArmed = false;
    }
  });

  // Only steps the window can observe. The Core runs its Safe Loop startup check before it accepts
  // a connection, so that step is done as soon as the Core answers. Its first heartbeat always
  // confirms stock first (~3 s), then reapplies the saved profile if there is one.
  const startupSteps = $derived.by(() => {
    const coreUp = coreAnswered || serviceStatus === "online";
    const steps = [{ label: "Starting the Core Service", done: coreUp }];
    if (coreUp) steps.push({ label: "Checking Safe Loop", done: true });
    if (sessionProfile !== undefined) {
      steps.push({
        label: sessionProfile ? `Applying ${sessionProfile.replaceAll("'", "’")}` : "Confirming stock settings",
        done: Boolean(sessionReadyAt),
      });
    }
    return steps;
  });

  function sessionReady() {
    coreAnswered = true;
    sessionReadyAt = Date.now();
    lastRefreshAt = 0; // the next tick reads the Core as the reapply left it
  }

  $effect(() => {
    // Listen first: the program sets its flag before it emits, so asking afterwards cannot miss it.
    const stop = listen("core-session", ({ payload }) => {
      if (payload?.ready) return sessionReady();
      coreAnswered = true;
      sessionProfile = payload?.profile ?? null;
    }).catch(() => () => {});
    stop.then(() => invoke("core_session_ready"))
      // Ready before this view existed (after onboarding, or a reloaded page): every read is
      // already after the reapply.
      .then((ready) => { if (ready && !sessionReadyAt) { coreAnswered = true; sessionReadyAt = 1; } })
      // No program around this page (browser preview, tests): no reapply to wait for.
      .catch(() => { if (!sessionReadyAt) sessionReadyAt = 1; });
    return () => stop.then((unlisten) => unlisten());
  });

  $effect(() => {
    if (booting && sessionReadyAt && snapshotAt > sessionReadyAt && hardware) booting = false;
  });

  $effect(() => {
    // Counted on screen only: a program started in the tray shows its window later.
    if (!booting || !$windowVisible) return;
    const limit = setTimeout(() => (booting = false), STARTUP_LIMIT_MS);
    return () => clearTimeout(limit);
  });

  $effect(() => {
    // Hidden in the tray or minimized: no polling. Showing the window again catches up at once.
    if (!$windowVisible) return;
    loadHardware();
    refresh();
    refreshSentinel();
    refreshSensors();
    refreshGameTrace();
    refreshManualPoint();
    refreshDetectorLab();
    timer = setInterval(tick, 500);
    const sentinelTimer = setInterval(refreshSentinel, 10_000);
    const sensorTimer = setInterval(refreshSensors, 2000);
    const gameTraceTimer = setInterval(refreshGameTrace, 1000);
    const manualPointTimer = setInterval(refreshManualPoint, 2000);
    const detectorLabTimer = setInterval(refreshDetectorLab, 1000);
    return () => {
      clearInterval(timer);
      clearInterval(sentinelTimer);
      clearInterval(sensorTimer);
      clearInterval(gameTraceTimer);
      clearInterval(manualPointTimer);
      clearInterval(detectorLabTimer);
    };
  });
</script>

<StartupScreen visible={booting} steps={startupSteps} />
<section class={`forge theme-${theme}`} inert={booting}>
  <ForgeThemeScreen
    {theme}
    {hardware}
    gpu={primarySensorGpu}
    {sparks}
    {powerSweep}
    {safeLoop}
    {applied}
    {forgeMode}
    {powerRunning}
    {fullResetBusy}
    {actionBusy}
    {fullResetFeedback}
    {error}
    {serviceStatus}
    {serviceError}
    {hardwareError}
    {exporting}
    {exportMsg}
    {exportFailed}
    onExportLog={exportLog}
    onViewSafetyHistory={() => { diagnosticsTab = "sentinel"; changeView("advanced"); }}
    {onThemeChange}
    onForgeModeChange={selectForgeMode}
    onStartPower={startPower}
    onRecoverContinue={recoverAndStartPower}
    onStopPower={stopPower}
    onResumePower={resumePower}
    onApplyPower={applyPower}
    onReportProfileUnstable={reportProfileUnstable}
    onFullReset={fullResetTuning}
    onReset={resetTuning}
    onDismissError={() => (error = null)}
    onDismissFullResetFeedback={() => (fullResetFeedback = null)}
    {activeView}
    onViewChange={changeView}
  >
    <AdvancedDiagnosticsHub
      bind:activeTab={diagnosticsTab}
      {theme}
      embedded
      {powerSweep}
      {logLines}
      {exporting}
      {exportMsg}
      {exportFailed}
      {sentinel}
      {safeLoop}
      {sentinelState}
      {sentinelSummary}
      {gameTrace}
      {gameTraceBusy}
      {gameTraceActionError}
      {gameTraceExportBusy}
      {gameTraceExportMsg}
      {manualPoint}
      {manualPointBusy}
      {manualPointActionError}
      {detectorLab}
      {detectorLabBusy}
      {detectorLabActionError}
      onExportLog={exportLog}
      onToggleGameTrace={toggleGameTrace}
      onOpenGameTraceLog={openGameTraceLog}
      onApplyManualPoint={applyManualPoint}
      onResetManualPoint={resetManualPoint}
      onStartDetectorLab={startDetectorLab}
      onStopDetectorLab={stopDetectorLab}
      onOpenDetectorLabLog={openDetectorLabLog}
      onClose={closeAdvancedDiagnostics}
    />
  </ForgeThemeScreen>
  <UpdateButton />

</section>

<style>
  .forge {
    display: block;
  }
</style>
