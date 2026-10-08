/** Validate every IPC boundary before allowing the next action. */
export function nvidiaGpu(hardware) {
  return hardware?.gpu?.find((gpu) => /nvidia/i.test(`${gpu?.vendor ?? ""} ${gpu?.model ?? ""}`)) ?? null;
}

export function requireServiceData(response, type, label = type) {
  if (response?.ok !== true) throw new Error(response?.error || `${label} failed`);
  if (response?.data?.type !== type) throw new Error(`Invalid ${label} response`);
  return response.data;
}

/** One card per exact Apply setting; several objectives may legitimately select the same pair. */
export function distinctForgeProfiles(metadata, progress) {
  const groups = [];
  for (const profile of metadata) {
    const point = progress?.[profile.key];
    if (!point) continue;
    const clock = point.target_clock_mhz ?? point.clock_mhz;
    const voltage = point.vf_table_voltage_mv ?? point.voltage_mv;
    // Incomplete identities are never merged merely because both fields are missing.
    const identity = Number.isFinite(clock) && Number.isFinite(voltage)
      ? `${clock}@${voltage}` : profile.key;
    const existing = groups.find((group) => group.identity === identity);
    if (existing) {
      existing.roles.push(profile.name);
      existing.name = "Shared measured setting";
      existing.summary = `Same measured setting selected for ${existing.roles.join(", ")}.`;
    } else {
      groups.push({ ...profile, identity, roles: [profile.name] });
    }
  }
  return groups;
}

/** Shared by the rendered primary action and its tests. No hardware writes here. */
export function forgePrimaryAction({ serviceStatus, gpuDetected, safeLoop, powerSweep, busy, hasProfiles }) {
  const action = (kind, label, reason, disabled = false) => ({ kind, label, reason, disabled });
  if (serviceStatus !== "online") return action("wait", "Core Service unavailable", "Waiting for the local Core Service. Reopen Nidavellir if it does not reconnect.", true);
  if (!gpuDetected || !safeLoop || !powerSweep) return action("wait", "Checking readiness", "Confirming the local GPU and safety state.", true);
  if (busy) return action("wait", "Working…", "Completing the current action.", true);
  if (powerSweep.running) return action("wait", "Forging…", "Measurements are running. Stop remains available.", true);
  if (safeLoop.gpu_reboot_required) return action("reboot", "Restart Windows", "Restart Windows once; the incident and safety history are saved.", true);
  if (safeLoop.recovery_pending_ack) return action("recover", "Recover Forge", "Return to stock and review recovery. Resume is checked against the saved run, build, GPU and driver.");
  if (safeLoop.safe_mode || safeLoop.state === "unstable" || (safeLoop.boot_flag_armed && !safeLoop.survival_window)) return action("reset", "Return to stock", "Release recovery at stock. The saved run and safety history stay preserved.");
  if (safeLoop.boot_flag_armed) return action("wait", "Verifying profile…", "Safe Loop is watching the first seconds after the apply.", true);
  if (powerSweep.start_block_reason) return action("review", "Review safety block", powerSweep.start_block_reason);
  if (powerSweep.resume_available) return action("resume", "Resume Forge", "Continue the same compatible run and its original mode.");
  if (["paused", "interrupted", "needs_attention"].includes(powerSweep.phase)) return action("start_over", "Start over", "The saved run cannot resume. Review Full Reset to forget all GPU learning, or Soft Reset to keep known failures.");
  return action("start", hasProfiles ? "Refine Profiles" : "Forge GPU", "Standard is recommended. A qualified result or a clear stock outcome will be shown.");
}

/** Explicit recovery only: never substitute a fresh Start for a refused Resume. */
export async function recoverForge(call) {
  const [statusResponse, progressResponse] = await Promise.all([
    call("GetSafeLoopStatus"),
    call("GetPowerSweepProgress"),
  ]);
  const status = requireServiceData(statusResponse, "SafeLoop");
  const before = requireServiceData(progressResponse, "PowerSweep");
  if (status.gpu_reboot_required) throw new Error("Restart Windows before recovering Forge.");
  if (before.running) throw new Error("Stop the active Forge before recovering an incident.");

  const applied = requireServiceData(await call("ResetGpuTuning"), "GpuApply", "Stock recovery");
  const safeLoop = requireServiceData(await call("AcknowledgeForgeIncident"), "SafeLoop", "Incident acknowledgement");
  if (safeLoop.recovery_pending_ack || safeLoop.safe_mode || safeLoop.boot_flag_armed || safeLoop.gpu_reboot_required || safeLoop.state === "unstable") {
    throw new Error("Recovery is still pending. No Forge run was started.");
  }
  let powerSweep = requireServiceData(await call("GetPowerSweepProgress"), "PowerSweep");
  const resumed = powerSweep.resume_available === true && !powerSweep.start_block_reason;
  if (resumed) {
    powerSweep = requireServiceData(await call("ResumePowerSweep"), "PowerSweep", "Forge resume");
  }
  return {
    applied, safeLoop, powerSweep, resumed,
    message: resumed
      ? "Recovery completed. The same Forge run has resumed."
      : powerSweep.start_block_reason
        ? `Recovery completed at stock. Automatic tuning is still blocked: ${powerSweep.start_block_reason}`
        : "Recovery completed at stock. The saved run cannot resume. Starting a new run requires a separate action; safety history is preserved.",
  };
}
