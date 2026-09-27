import test from "node:test";
import assert from "node:assert/strict";
import { distinctForgeProfiles, forgePrimaryAction, nvidiaGpu, recoverForge, requireServiceData } from "./forge-workflow.js";

test("profile objectives share one card only for the same exact Apply pair", () => {
  const metadata = ["godforge", "brokkrs", "deep_calm"].map((key) => ({ key, name: key }));
  const point = { target_clock_mhz: 1710, vf_table_voltage_mv: 868 };
  const progress = Object.fromEntries(metadata.map(({ key }) => [key, { ...point }]));
  const shared = distinctForgeProfiles(metadata, progress);
  assert.equal(shared.length, 1);
  assert.deepEqual(shared[0].roles, metadata.map(({ key }) => key));
  assert.equal(shared[0].key, "godforge");
  progress.deep_calm.vf_table_voltage_mv = 862;
  assert.equal(distinctForgeProfiles(metadata, progress).length, 2);
  progress.brokkrs.target_clock_mhz = 1725;
  assert.equal(distinctForgeProfiles(metadata, progress).length, 3);
  assert.equal(distinctForgeProfiles(metadata, { godforge: {}, brokkrs: {} }).length, 2);
  assert.equal(distinctForgeProfiles(metadata, {}).length, 0);
});

const safe = { type: "SafeLoop", state: "idle", safe_mode: false, boot_flag_armed: false, recovery_pending_ack: false, gpu_reboot_required: false };
const progress = { type: "PowerSweep", phase: "idle", running: false, resume_available: false };
const ready = { serviceStatus: "online", gpuDetected: true, safeLoop: safe, powerSweep: progress };
const ok = (data) => ({ ok: true, data });

test("J01: NVIDIA identification ignores the integrated GPU and rejects absent targets", () => {
  const nvidia = { vendor: "NVIDIA", model: "GeForce RTX 3060 Ti" };
  assert.equal(nvidiaGpu({ gpu: [{ vendor: "Intel", model: "UHD" }, nvidia] }), nvidia);
  assert.equal(nvidiaGpu({ gpu: [{ vendor: "AMD", model: "Radeon" }] }), null);
  assert.equal(nvidiaGpu(null), null);
});

test("J09: persistent safety refusal cannot turn into Start over or Resume", () => {
  for (const phase of ["idle", "needs_attention", "interrupted", "paused"]) {
    const action = forgePrimaryAction({ ...ready, powerSweep: { ...progress, phase, resume_available: true, start_block_reason: "Safety limit reached" } });
    assert.equal(action.kind, "review");
    assert.equal(action.disabled, false);
    assert.equal(action.reason, "Safety limit reached");
  }
});

test("J01/J02/J07: primary action follows readiness and never stale profiles", () => {
  assert.equal(forgePrimaryAction(ready).kind, "start");
  for (const patch of [
    { serviceStatus: "offline", hasProfiles: true }, { serviceStatus: "connecting" },
    { gpuDetected: false }, { safeLoop: null }, { powerSweep: null }, { busy: true },
    { powerSweep: { ...progress, running: true } },
    { safeLoop: { ...safe, gpu_reboot_required: true } },
  ]) assert.equal(forgePrimaryAction({ ...ready, ...patch }).disabled, true);
});

test("J03/J04: pending recovery is distinct from proven Resume", () => {
  const pending = { ...safe, state: "unstable", recovery_pending_ack: true };
  assert.equal(forgePrimaryAction({ ...ready, safeLoop: pending }).kind, "recover");
  for (const phase of ["paused", "interrupted", "needs_attention"]) {
    assert.equal(forgePrimaryAction({ ...ready, powerSweep: { ...progress, phase } }).kind, "start_over");
    assert.equal(forgePrimaryAction({ ...ready, powerSweep: { ...progress, phase, resume_available: true } }).kind, "resume");
  }
  assert.equal(forgePrimaryAction({ ...ready, safeLoop: { ...safe, safe_mode: true } }).kind, "reset");
});

function transport({ resumable = true, failAt, malformedAt, pendingAfterAck = false, reboot = false, running = false } = {}) {
  const calls = [];
  let acknowledged = false;
  const call = async (method) => {
    calls.push(method);
    if (method === failAt) return { ok: false, error: "injected failure" };
    if (method === malformedAt) return ok({ type: "Unexpected" });
    if (method === "GetSafeLoopStatus") return ok({ ...safe, recovery_pending_ack: true, gpu_reboot_required: reboot });
    if (method === "GetPowerSweepProgress") return ok({ ...progress, running, phase: "interrupted", resume_available: acknowledged && resumable });
    if (method === "ResetGpuTuning") return ok({ type: "GpuApply", active: false });
    if (method === "AcknowledgeForgeIncident") {
      acknowledged = true;
      return ok({ ...safe, recovery_pending_ack: pendingAfterAck });
    }
    if (method === "ResumePowerSweep") return ok({ ...progress, phase: "preparing", running: true });
    throw new Error(`Unexpected method ${method}`);
  };
  return { calls, call };
}

test("J03: recover at stock, acknowledge, recheck compatibility, resume same run", async () => {
  const ipc = transport();
  const result = await recoverForge(ipc.call);
  assert.deepEqual(ipc.calls, ["GetSafeLoopStatus", "GetPowerSweepProgress", "ResetGpuTuning", "AcknowledgeForgeIncident", "GetPowerSweepProgress", "ResumePowerSweep"]);
  assert.equal(result.resumed, true);
  assert.equal(result.powerSweep.running, true);
});

test("J04: missing/incompatible checkpoint ends at stock without a new run", async () => {
  const ipc = transport({ resumable: false });
  const result = await recoverForge(ipc.call);
  assert.equal(result.resumed, false);
  assert.equal(result.safeLoop.recovery_pending_ack, false);
  assert.equal(result.applied.active, false);
  assert.equal(ipc.calls.some((method) => /^(Start|Resume|Apply)/.test(method)), false);
});

test("J09: acknowledgement with a persistent block explains the stock outcome without resuming", async () => {
  const ipc = transport();
  const result = await recoverForge(async (method) => {
    const response = await ipc.call(method);
    if (response.data?.type === "PowerSweep") response.data.start_block_reason = "Safety limit reached";
    return response;
  });
  assert.equal(result.resumed, false);
  assert.match(result.message, /Automatic tuning is still blocked: Safety limit reached/);
  assert.equal(ipc.calls.some((method) => /^(Start|Resume|Apply)/.test(method)), false);
});

for (const method of ["ResetGpuTuning", "AcknowledgeForgeIncident", "ResumePowerSweep"]) {
  for (const failure of ["failAt", "malformedAt"]) {
    test(`J05: ${method} ${failure} stops the workflow`, async () => {
      const ipc = transport({ [failure]: method });
      await assert.rejects(recoverForge(ipc.call));
      assert.equal(ipc.calls.at(-1), method);
      assert.equal(ipc.calls.some((name) => name.startsWith("Start")), false);
    });
  }
}

test("J05: acknowledged response with a remaining latch cannot resume", async () => {
  const ipc = transport({ pendingAfterAck: true });
  await assert.rejects(recoverForge(ipc.call), /still pending/);
  assert.equal(ipc.calls.at(-1), "AcknowledgeForgeIncident");
});

test("J07/J08: reboot or a newly running Forge prevents all mutations", async () => {
  for (const options of [{ reboot: true }, { running: true }]) {
    const ipc = transport(options);
    await assert.rejects(recoverForge(ipc.call));
    assert.deepEqual(ipc.calls, ["GetSafeLoopStatus", "GetPowerSweepProgress"]);
  }
});

test("J02: malformed/failed IPC cannot masquerade as known safety state", () => {
  for (const response of [null, {}, { data: safe }, { ok: false, data: safe }, ok({ type: "Other" })]) {
    assert.throws(() => requireServiceData(response, "SafeLoop"));
  }
});
