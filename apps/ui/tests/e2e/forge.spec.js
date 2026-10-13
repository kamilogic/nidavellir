import { test, expect } from "@playwright/test";

// This replaces Tauri at the browser boundary. It cannot open the real service pipe.
async function openForge(page, scenario = "ready") {
  await page.addInitScript(({ scenario }) => {
    if (!scenario.startsWith("onboarding")) localStorage.setItem("nidavellir-gpu-onboarded", "true");
    // 0.5.0 ran before; the app now reports 0.5.2 (only once, so a reload keeps what WhatsNew wrote).
    if (scenario === "updated" && !localStorage.getItem("nidavellir-last-version")) localStorage.setItem("nidavellir-last-version", "0.5.0");
    const pending = ["recover", "missing", "ack-failure", "double-click"].includes(scenario);
    const state = {
      calls: [], offline: scenario === "offline", acknowledged: false,
      window: { closeToTray: true, minimizeToTray: false, startWithWindows: false },
      applied: JSON.parse(sessionStorage.getItem("fixture-applied") || "null") || { type: "GpuApply", core: null, label: null },
      safe: { type: "SafeLoop", state: pending ? "unstable" : "idle", safe_mode: false, boot_flag_armed: false, recovery_pending_ack: pending, gpu_reboot_required: scenario === "reboot", blacklist: [], condemnations: [] },
      power: { type: "PowerSweep", phase: scenario === "missing" ? "idle" : pending ? "interrupted" : "idle", running: false, auto_resume: true, resume_available: false, points: [], log: [], start_block_reason: scenario === "safety-limit" ? "Forge safety limit reached. Soft Reset preserves known failures; Full Reset erases all GPU learning." : null },
    };
    if (["qualified", "apply-failure", "unqualified", "collapsed"].includes(scenario)) {
      const point = { clock_mhz: 1800, target_clock_mhz: 1800, voltage_mv: 950, vf_table_voltage_mv: 950, comparison_power_p99_w: 155, power_p99_w: 199.8, max_power_w: 201, perf_per_watt: 11.6, apply_qualified: scenario !== "unqualified" };
      Object.assign(state.power, { phase: "finished", is_undervolt: true, frontier_complete: true, profile_search_complete: scenario !== "collapsed", profiles_qualified: scenario !== "unqualified", godforge: { ...point, target_clock_mhz: scenario === "collapsed" ? 1800 : 1830 }, brokkrs: point, deep_calm: { ...point, target_clock_mhz: scenario === "collapsed" ? 1800 : 1740 }, stock_clock_mhz: 1800, stock_power_p99_w: 200 });
    }
    window.__forgeTest = state;
    // Events the program's Rust side emits (tray notices, Exit during a run).
    const listeners = new Map();
    let nextCallback = 1;
    window.__tauriListening = (event) => listeners.has(event);
    window.__tauriEmit = (event, payload) => {
      for (const handler of listeners.get(event) ?? []) handler({ event, id: 0, payload });
    };
    window.__TAURI_INTERNALS__ = {
      transformCallback: (callback) => {
        const id = nextCallback++;
        window[`_${id}`] = callback;
        return id;
      },
      invoke: async (command, args = {}) => {
        const { method } = args;
        if (command === "plugin:event|listen") {
          listeners.set(args.event, [...(listeners.get(args.event) ?? []), window[`_${args.handler}`]]);
          return args.handler;
        }
        if (command === "get_window_settings") return structuredClone(state.window);
        if (command === "set_window_settings") {
          state.calls.push("set_window_settings");
          state.window = { ...args.settings };
          return structuredClone(state.window);
        }
        if (command === "exit_program") {
          state.calls.push("exit_program");
          return null;
        }
        if (command === "plugin:updater|check") {
          return scenario === "update"
            ? { rid: 7, currentVersion: "0.5.0", version: "0.5.1", date: "2026-10-03T20:31:52Z", body: "Nidavellir now lives in the tray.\n\n- Right-click the tray icon to switch profiles.\n- Fixed: a console window opened next to Nidavellir.\n\nThe run stays saved.", rawJson: {} }
            : null;
        }
        if (command === "plugin:updater|download_and_install") {
          state.calls.push("download_and_install");
          return null;
        }
        if (command === "plugin:app|version" && scenario === "updated") return "0.5.2";
        // Other scenarios answer like a page without the program: no reapply to wait for.
        if (command === "core_session_ready" && scenario === "starting") return false;
        if (command !== "service_request") throw new Error(`Unexpected command ${command}`);
        state.calls.push(method);
        if (scenario === "connecting") return new Promise(() => {});
        if (state.offline) throw new Error("Core Service unavailable");
        // A first launch: the program is still starting the Core for the first two detections.
        if (scenario === "onboarding-starting" && method === "DetectHardware" && state.calls.filter((m) => m === method).length < 3) {
          throw new Error("Core Service unavailable: The system cannot find the file specified. (os error 2)");
        }
        const ok = (data) => ({ ok: true, data: structuredClone(data) });
        if (method === "DetectHardware") {
          if (scenario === "onboarding-malformed") return ok({ type: "Unexpected" });
          return ok({ type: "Hardware", gpu: scenario === "onboarding-unsupported" ? [{ vendor: "AMD", model: "Radeon" }] : [{ vendor: "NVIDIA", model: "NVIDIA GeForce RTX 3060 Ti" }] });
        }
        if (method === "GetSafeLoopStatus") return ok(state.safe);
        if (method === "GetPowerSweepProgress") return ok(state.power);
        if (method === "SetSentinelCanary") {
          state.sentinelRequests = [...(state.sentinelRequests ?? []), args.params.enabled];
          state.power.sentinel_canary = args.params.enabled;
          if (args.params.enabled) state.power.sentinel_advice = null;
          return ok(state.power);
        }
        if (method === "GetAppliedProfile") return ok(state.applied);
        if (method === "GetSentinelStatus" && scenario === "slow-reset" && state.resetCompleted) {
          return new Promise(() => {});
        }
        if (method === "ExportForgeLog") return ok({ type: "ForgeLogExport", note: "Diagnostic report saved", path: "C:/reports/forge-safety.txt" });
        // A test may set gpu readings; a callable entry yields a fresh reading on each poll.
        if (method === "ReadSensors") return ok({ type: "Sensors", gpu: (state.sensors ?? []).map((entry) => (typeof entry === "function" ? entry(state.calls.filter((m) => m === "ReadSensors").length) : entry)) });
        if (method === "ResetGpuTuning") {
          if (scenario === "double-click") await new Promise((resolve) => setTimeout(resolve, 250));
          state.applied = { type: "GpuApply", core: null, label: null };
          sessionStorage.removeItem("fixture-applied");
          return ok(state.applied);
        }
        if (method === "ApplyPowerBrokkrs") {
          if (scenario === "apply-failure") return { ok: false, error: "Exact qualified descriptor was refused" };
          state.applied = { type: "GpuApply", label: "Brokkr's Best", core: { freq_mhz: 1800, voltage_mv: 950 } };
          sessionStorage.setItem("fixture-applied", JSON.stringify(state.applied));
          return ok(state.applied);
        }
        if (method === "AcknowledgeForgeIncident") {
          if (scenario === "ack-failure") return { ok: false, error: "Safety history could not be saved" };
          state.safe.state = "idle";
          state.safe.recovery_pending_ack = false;
          state.power.resume_available = scenario !== "missing";
          return ok(state.safe);
        }
        if (method === "StartPowerSweep" || method === "ResumePowerSweep") {
          state.power.phase = "preparing";
          state.power.running = true;
          return ok(state.power);
        }
        if (method === "ResetGpuTuningFull" || method === "ResetGpuTuningSoft") {
          if (scenario === "slow-reset") {
            await new Promise((resolve) => { state.releaseReset = resolve; });
            state.resetCompleted = true;
          }
          if (scenario === "reset-failure") return { ok: false, error: "GPU stock restoration could not be verified" };
          if (scenario === "old-core") return ok({ type: "GpuApply", active: false, message: "Full reset to stock; positive learning cleared and all negative safety evidence preserved" });
          state.safe.state = "idle";
          state.safe.recovery_pending_ack = false;
          if (method === "ResetGpuTuningFull") {
            state.safe.blacklist = [];
            state.safe.condemnations = [];
            state.power.start_block_reason = null;
          }
          state.power = { ...state.power, phase: "idle", running: false, resume_available: false, profiles_qualified: false, godforge: null, brokkrs: null, deep_calm: null, points: [] };
          return ok({ type: "GpuApply", active: false, message: method === "ResetGpuTuningFull" ? "Full Reset completed" : "Soft Reset completed; known failures preserved" });
        }
        return ok({ type: "Unused", running: false });
      },
    };
  }, { scenario });
  page.on("dialog", (dialog) => dialog.accept());
  await page.goto("/");
}

// Start over lives in Settings. Full Reset waits for the typed word; a confirmed reset shows the Forge.
async function openReset(page, mode) {
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByRole("button", { name: `${mode} Reset…`, exact: true }).click();
  await expect(page.getByRole("dialog")).toHaveCount(1);
}

async function confirmReset(page, mode) {
  const dialog = page.getByRole("dialog");
  if (mode === "Full") {
    await expect(dialog.locator(".reset-confirm")).toBeDisabled();
    await dialog.getByLabel("Type ERASE to confirm").fill("ERASE");
  }
  await dialog.locator(".reset-confirm").click();
}

// A new Forge opens the pre-Forge sheet first; its copper plate starts the run.
async function startFromSheet(page) {
  const sheet = page.getByRole("dialog", { name: "Before the Forge starts" });
  await expect(sheet).toBeVisible();
  await sheet.getByRole("button", { name: "Start Forge" }).click();
  await expect(sheet).toBeHidden();
}

test("J01: default run needs no mode selection and only one start", async ({ page }, testInfo) => {
  await openForge(page);
  const primary = page.locator(".plate-button");
  await expect(primary).toHaveText("Forge GPU");
  await expect(page.getByRole("heading", { name: "NVIDIA GeForce RTX 3060 Ti", exact: true })).toBeVisible();
  await expect(page.locator(".hero-option").first()).toContainText("Standard · up to 8 h");
  await page.screenshot({ path: testInfo.outputPath("ready.png"), fullPage: true });
  await primary.click();
  await startFromSheet(page);
  await expect(primary).toBeDisabled();
  expect(await page.evaluate(() => window.__forgeTest.calls.filter((m) => m.startsWith("Start")))).toEqual(["StartPowerSweep"]);
});

test("J02: offline and reconnect update the actual primary action", async ({ page }) => {
  await openForge(page, "offline");
  await expect(page.locator(".plate-button")).toBeDisabled();
  await page.evaluate(() => { window.__forgeTest.offline = false; });
  await expect(page.locator(".plate-button")).toHaveText("Forge GPU", { timeout: 6000 });
  await expect(page.locator(".plate-button")).toBeEnabled();
});

test("J03: recovery resumes a compatible interrupted run", async ({ page }) => {
  await openForge(page, "recover");
  await page.locator(".plate-button").filter({ hasText: "Recover Forge" }).click();
  await expect(page.locator(".plate-button")).toHaveText("Forging…");
  expect(await page.evaluate(() => window.__forgeTest.calls.filter((m) => /^(Reset|Acknowledge|Resume|Start)/.test(m)))).toEqual(["ResetGpuTuning", "AcknowledgeForgeIncident", "ResumePowerSweep"]);
});

test("J04: missing checkpoint resolves at stock without starting", async ({ page }) => {
  await openForge(page, "missing");
  await page.locator(".plate-button").filter({ hasText: "Recover Forge" }).click();
  await expect(page.locator(".plate-button")).toHaveText("Forge GPU");
  await expect(page.getByText(/Recovery completed at stock/)).toBeVisible();
  expect(await page.evaluate(() => window.__forgeTest.calls.some((m) => /^(Start|Resume)/.test(m)))).toBe(false);
});

test("J05: acknowledgement failure stays visible and never resumes", async ({ page }) => {
  await openForge(page, "ack-failure");
  await page.locator(".plate-button").filter({ hasText: "Recover Forge" }).click();
  await expect(page.getByText(/Safety history could not be saved/).first()).toBeVisible();
  expect(await page.evaluate(() => window.__forgeTest.calls.includes("ResumePowerSweep"))).toBe(false);
});

test("J06: Full Reset clears the alert and prepares Clean without starting", async ({ page }) => {
  await openForge(page, "recover");
  await openReset(page, "Full");
  await confirmReset(page, "Full");
  await expect(page.locator(".plate-button")).toHaveText("Forge GPU");
  await expect(page.locator("#command-run-mode")).toHaveValue("clean");
  expect(await page.evaluate(() => window.__forgeTest.calls.some((m) => /^(Start|Resume)/.test(m)))).toBe(false);
});

for (const mode of ["Full", "Soft"]) {
test(`${mode} Reset responds before cleanup and does not wait for Sentinel refresh`, async ({ page }, testInfo) => {
  await openForge(page, "slow-reset");
  const nativeDialogs = [];
  page.on("dialog", (dialog) => nativeDialogs.push(dialog.message()));
  await openReset(page, mode);
  await confirmReset(page, mode);
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(page.getByRole("status").filter({ hasText: `${mode} Reset in progress` })).toBeVisible();
  await expect(page.locator(".plate-button")).toBeVisible();
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  const button = page.getByRole("button", { name: `${mode} Reset…`, exact: true });
  await expect(button).toBeDisabled();
  await expect(page.getByText("Reset completed", { exact: true })).toHaveCount(0);
  await page.screenshot({ path: testInfo.outputPath("reset-in-progress.png"), fullPage: true });
  await page.evaluate(() => window.__forgeTest.releaseReset());
  await expect(button).toBeEnabled();
  await expect(page.getByText("Reset completed", { exact: true })).toBeVisible();
  expect(nativeDialogs).toEqual([]);
  expect(await page.evaluate(() => window.__forgeTest.calls.filter((m) => /^(Reset|Authorize|Start|Resume)/.test(m)))).toEqual([
    mode === "Full" ? "ResetGpuTuningFull" : "ResetGpuTuningSoft",
  ]);
});
}

test("J07: current-boot driver reset disables tuning and Full Reset", async ({ page }) => {
  await openForge(page, "reboot");
  await expect(page.locator(".plate-button")).toHaveText("Restart Windows");
  await expect(page.locator(".plate-button")).toBeDisabled();
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await expect(page.getByRole("button", { name: "Full Reset…", exact: true })).toBeDisabled();
  await expect(page.getByRole("button", { name: "Soft Reset…", exact: true })).toBeDisabled();
  await expect(page.getByText("Restart Windows before any tuning action.")).toBeVisible();
});

test("Full Reset explicitly erases known failures without starting a run", async ({ page }) => {
  await openForge(page, "safety-limit");
  await page.evaluate(() => { window.__forgeTest.safe.blacklist = [{ center: {}, radius: 1 }]; });
  await openReset(page, "Full");
  await expect(page.getByRole("dialog")).toContainText("Previously rejected points may be tested again");
  await page.getByRole("dialog").getByRole("button", { name: "Cancel" }).click();
  expect(await page.evaluate(() => window.__forgeTest.calls.includes("ResetGpuTuningFull"))).toBe(false);
  await openReset(page, "Full");
  await confirmReset(page, "Full");
  await expect(page.locator(".plate-button")).toHaveText("Forge GPU");
  await expect(page.getByText(/No saved GPU learning will be reused/)).toBeVisible();
  expect(await page.evaluate(() => window.__forgeTest.safe.blacklist)).toEqual([]);
  expect(await page.evaluate(() => window.__forgeTest.calls.filter((m) => m === "ResetGpuTuningFull"))).toHaveLength(1);
  expect(await page.evaluate(() => window.__forgeTest.calls.some((m) => /^(Start|Resume|Apply)/.test(m)))).toBe(false);
});

test("failed stock recovery never announces a completed Full Reset", async ({ page }) => {
  await openForge(page, "reset-failure");
  await openReset(page, "Full");
  await confirmReset(page, "Full");
  await expect(page.getByText(/Full Reset failed:.*stock restoration/)).toBeVisible();
  await expect(page.getByText(/No saved GPU learning will be reused/)).toHaveCount(0);
});

test("an older Core cannot falsely confirm that Full Reset erased failures", async ({ page }) => {
  await openForge(page, "old-core");
  await openReset(page, "Full");
  await confirmReset(page, "Full");
  await expect(page.getByText(/running Core still uses the old reset behavior/)).toBeVisible();
  await expect(page.getByText(/No saved GPU learning will be reused/)).toHaveCount(0);
});

test("duplicate recovery clicks cannot launch two transactions", async ({ page }) => {
  await openForge(page, "double-click");
  await expect(page.locator(".plate-button")).toHaveText("Recover Forge");
  await page.locator(".plate-button").evaluate((button) => { button.click(); button.click(); });
  await expect(page.locator(".plate-button")).toHaveText("Forging…");
  expect(await page.evaluate(() => window.__forgeTest.calls.filter((m) => m === "ResetGpuTuning").length)).toBe(1);
});

test("J01: onboarding finds the NVIDIA GPU by itself and needs no CPU driver step", async ({ page }, testInfo) => {
  await openForge(page, "onboarding-ready");
  await expect(page.getByLabel("GPU detection")).toContainText("NVIDIA GeForce RTX 3060 Ti");
  await expect(page.getByRole("heading", { name: "Before you forge" })).toBeVisible();
  await expect(page.getByText(/PawnIO/)).toHaveCount(0);
  await page.screenshot({ path: testInfo.outputPath("welcome.png"), animations: "disabled" });
  await page.getByRole("button", { name: "I understand, open the Forge" }).click();
  await expect(page.locator(".plate-button")).toHaveText("Forge GPU");
  expect(await page.evaluate(() => window.__forgeTest.calls.includes("GetDriverStatus"))).toBe(false);
});

test("J01: a first launch waits for the starting Core instead of reporting an error", async ({ page }, testInfo) => {
  await openForge(page, "onboarding-starting");
  await expect(page.getByLabel("GPU detection")).toContainText("Starting the Core Service…");
  await page.screenshot({ path: testInfo.outputPath("first-launch.png"), animations: "disabled" });
  await expect(page.getByLabel("GPU detection")).toContainText("NVIDIA GeForce RTX 3060 Ti");
  await expect(page.getByRole("button", { name: "I understand, open the Forge" })).toBeEnabled();
  await expect(page.locator(".welcome .error")).toHaveCount(0);
  expect(await page.evaluate(() => window.__forgeTest.calls.filter((m) => m === "DetectHardware").length)).toBe(3);
});

for (const scenario of ["onboarding-unsupported", "onboarding-malformed"]) {
  test("J01: " + scenario + " cannot advance", async ({ page }) => {
    await openForge(page, scenario);
    await expect(page.locator(".welcome .error")).toBeVisible();
    await expect(page.getByRole("button", { name: "I understand, open the Forge" })).toBeDisabled();
    await expect(page.getByRole("button", { name: "Try again" })).toBeVisible();
  });
}

test("J09: Soft Reset preserves the block and offers diagnostics", async ({ page }, testInfo) => {
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await openForge(page, "safety-limit");
  const review = page.getByRole("button", { name: "Review safety block", exact: true });
  await expect(review).toBeEnabled();
  await review.click();
  await expect(page.getByRole("button", { name: "Export diagnostic report" })).toBeVisible();
  await expect(page.getByText(/A required Windows restart or development authorization must be completed separately/)).toBeVisible();
  await expect(page.getByText("Safe Loop needs attention", { exact: true })).toHaveCount(0);
  await page.getByRole("button", { name: "Start over options" }).click();
  await page.getByRole("button", { name: "Soft Reset…", exact: true }).click();
  await page.getByRole("dialog").getByRole("button", { name: "Clear measurements, keep failures" }).click();
  await expect(page.getByText(/Automatic tuning is still blocked/)).toBeVisible();
  await expect(page.getByText("Reset completed; tuning blocked", { exact: true })).toBeVisible();
  await expect(review).toBeEnabled();
  await page.getByRole("button", { name: "Export diagnostic report" }).click();
  await expect(page.getByText(/Diagnostic report saved/).first()).toBeVisible();
  await expect(page.getByText(/next Forge is armed as a Clean Run/)).toHaveCount(0);
  expect(await page.evaluate(() => window.__forgeTest.calls.some((m) => /^(Start|Resume)/.test(m)))).toBe(false);
  expect(await page.evaluate(() => window.__forgeTest.calls.filter((m) => m === "ExportForgeLog"))).toHaveLength(1);
  await page.screenshot({ path: testInfo.outputPath("safety-guidance.png"), fullPage: true });
  await page.getByRole("button", { name: "View safety history" }).click();
  await expect(page.getByRole("heading", { name: "Sentinel", level: 1 })).toBeVisible();
  await expect(page.getByRole("heading", { name: "What happened" })).toBeVisible();
  expect(errors).toEqual([]);
});

test("J09: reviewing a safety block never enables saved profile Apply", async ({ page }) => {
  await openForge(page, "qualified");
  await page.evaluate(() => { window.__forgeTest.power.start_block_reason = "Forge safety limit reached"; });
  await expect(page.getByRole("button", { name: "Review safety block", exact: true })).toBeEnabled();
  await expect(page.getByRole("button", { name: "Apply Brokkr’s Best", exact: true })).toBeDisabled();
  expect(await page.evaluate(() => window.__forgeTest.calls.some((m) => /^(Start|Resume|Apply)/.test(m)))).toBe(false);
});

test("J12: shows single-run development authorization and its consumed state", async ({ page }, testInfo) => {
  await openForge(page);
  await page.evaluate(() => {
    window.__forgeTest.power.development_validation_note = "One development validation is authorized. Existing unsafe points remain excluded.";
  });
  await expect(page.getByText("Development validation", { exact: true })).toBeVisible();
  await expect(page.getByText(/One development validation is authorized/)).toBeVisible();
  expect(await page.evaluate(() => window.__forgeTest.calls.some(m => /^(Authorize|Start)/.test(m)))).toBe(false);
  await page.getByRole("button", { name: "Forge GPU", exact: true }).click();
  await startFromSheet(page);
  await expect.poll(() => page.evaluate(() => window.__forgeTest.calls.filter(m => m.startsWith("Start")))).toEqual(["StartPowerSweep"]);
  await page.evaluate(() => {
    Object.assign(window.__forgeTest.power, {running:false, phase:"idle",
      start_block_reason:"Development validation ended. Export the report for review.",
      development_validation_note:"Development validation ended. No further run, Resume or profile Apply is authorized in this service session."});
  });
  await page.getByRole("button", { name:"Review safety block", exact:true }).click();
  await expect(page.getByRole("button", {name:"Export diagnostic report",exact:true})).toBeVisible();
  await page.screenshot({path:testInfo.outputPath("development-validation.png"),fullPage:true});
  expect(await page.evaluate(() => window.__forgeTest.calls.filter(m => /^(Authorize|Start|Resume|Apply)/.test(m)))).toEqual(["StartPowerSweep"]);
});

test("J08: stopping and service loss never imply a qualified success", async ({ page }) => {
  await openForge(page);
  await page.locator(".plate-button").click();
  await startFromSheet(page);
  await expect(page.getByRole("button", { name: "Stop safely" })).toBeVisible();
  await page.evaluate(() => {
    window.__forgeTest.power.phase = "stopping";
    window.__forgeTest.power.running = true;
  });
  await expect(page.getByRole("button", { name: "Stopping…", exact: true })).toBeDisabled();
  await page.evaluate(() => { window.__forgeTest.offline = true; });
  await expect(page.locator(".plate-button")).toBeDisabled();
  await expect(page.locator(".plate-button")).toHaveText("Core Service unavailable");
  expect(await page.evaluate(() => window.__forgeTest.calls.some((m) => m.startsWith("Apply")))).toBe(false);
});

test("Profile results: identical pairs form one card with incomplete coverage disclosed", async ({ page }) => {
  await openForge(page, "collapsed");
  await expect(page.locator(".profile-column")).toHaveCount(2);
  await expect(page.locator(".profile-column").nth(1)).toContainText("Shared setting");
  await expect(page.getByText("1 distinct setting · qualified", { exact: true })).toBeVisible();
  await expect(page.getByText(/Economic search coverage is incomplete/)).toBeVisible();
  await page.getByRole("button", { name: /Compare details/ }).click();
  await expect(page.getByText("155.0 W", { exact: true })).toBeVisible();
  await expect(page.getByText("201 W", { exact: true })).toBeVisible();
  expect(await page.evaluate(() => window.__forgeTest.calls.some((m) => m.startsWith("Apply")))).toBe(false);
});

test("Qualification: screening never appears as a qualified profile or permits Apply", async ({ page }) => {
  await openForge(page);
  await page.evaluate(() => {
    Object.assign(window.__forgeTest.power, {
      run_id: "qualification-test", running: true, phase: "descend",
      current_task: "frontier_descent", current_clock_mhz: 1890, current_voltage_mv: 875,
      last_outcome: "Validated", profiles_qualified: false,
    });
  });
  const progress = page.getByRole("region", { name: "Forging your GPU" });
  const lastDecision = progress.locator(".task-card.last");
  await expect(lastDecision).toContainText("Stage passed");
  await expect(lastDecision).toContainText("complete qualification matrix");
  await expect(progress.locator(".phase-rail [aria-current=step]")).toContainText("Test candidates");

  await page.evaluate(() => {
    Object.assign(window.__forgeTest.power, {
      phase: "apply-qualify", current_task: "apply_qualification",
      last_outcome: "EligibleForQualification",
    });
  });
  await expect(lastDecision).toContainText("Eligible for qualification");
  await expect(lastDecision).toContainText("before this pair can become a profile or allow refinement");
  await expect(progress.locator(".task-card.current")).toContainText("Qualifying the current candidate");
  await expect(progress.locator(".phase-rail [aria-current=step]")).toContainText("Test candidates");
  await expect(page.locator(".profile-apply:enabled")).toHaveCount(0);

  await page.evaluate(() => { window.__forgeTest.power.last_outcome = "CandidateQualified"; });
  await expect(lastDecision).toContainText("Candidate qualified");
  await expect(lastDecision).toContainText("complete qualification matrix and stock restoration checks");
  await expect(page.locator(".profile-apply:enabled")).toHaveCount(0);
  expect(await page.evaluate(() => window.__forgeTest.calls.some((m) => /^(Start|Resume|Apply)/.test(m)))).toBe(false);
});

test("Qualification: power limits and driver failures remain distinct from a skipped point", async ({ page }) => {
  await openForge(page);
  await page.evaluate(() => {
    Object.assign(window.__forgeTest.power, {
      run_id: "outcome-test", running: true, phase: "descend", last_outcome: "PowerBoundClockDrop",
    });
  });
  const lastDecision = page.getByRole("region", { name: "Forging your GPU" }).locator(".task-card.last");
  await expect(lastDecision).toContainText("Inconclusive · power limit");
  await page.evaluate(() => { window.__forgeTest.power.last_outcome = "Inconclusive"; });
  await expect(lastDecision).toContainText("Evidence was insufficient");
  await expect(lastDecision).not.toContainText("low residency");
  await page.evaluate(() => { window.__forgeTest.power.last_outcome = "CandidateCrash"; });
  await expect(lastDecision).toContainText("Interrupted · recovery");
  await expect(lastDecision).not.toContainText("Skipped for safety");
  await page.evaluate(() => { window.__forgeTest.power.last_outcome = "OperationalFailure"; });
  await expect(lastDecision).toContainText("Interrupted · safety check");
  await expect(lastDecision).not.toContainText("Evidence recorded");
  await page.evaluate(() => { window.__forgeTest.power.last_outcome = "Cancelled"; });
  await expect(lastDecision).toContainText("Candidate stopped");
  await expect(lastDecision).toContainText("spent search budget is retained");
});

test("Qualification: shows bounded coverage and retains it on Resume", async ({ page }, testInfo) => {
  await openForge(page);
  await page.evaluate(() => {
    Object.assign(window.__forgeTest.power, {
      run_id: "bounded-test", running: false, phase: "paused", resume_available: true,
      last_outcome: "BandClosedIntegrityError",
      discovery_search: {
        attempts_used: 7, attempts_limit: 24, elapsed_ms: 7_200_000, time_budget_ms: 28_800_000,
        bands: [
          { id: "performance", status: "closed", last_qualified_clock_mhz: 1890, last_qualified_voltage_mv: 900, stop_reason: "integrity_error_region_closed" },
          { id: "balanced", status: "pending" },
          { id: "efficiency", status: "pending" },
        ],
      },
    });
  });
  // Search coverage lives on the Forge report page; Resume stays on the Forge screen.
  const forge = page.getByRole("button", { name: "Forge", exact: true });
  const report = page.getByRole("button", { name: "Forge report ›" });
  await report.click();
  await expect(page.getByRole("heading", { name: "Forge paused", level: 1 })).toBeVisible();
  const coverage = page.getByRole("region", { name: "Candidate search coverage" });
  await expect(coverage).toContainText("7 / 24 candidate attempts");
  await expect(coverage).toContainText("1 / 3 regions with a qualified candidate");
  await expect(coverage).toContainText("Run budget: 8h 0m · used 2h 0m");
  await expect(coverage).toContainText("Performance · Closed");
  await expect(coverage).toContainText("Qualified: 1890 MHz @ 900 mV");
  await expect(coverage).toContainText("An integrity error ended exploration of this region.");
  await expect(coverage).toContainText("does not classify untested points as unstable");
  await page.screenshot({ path: testInfo.outputPath("candidate-budget.png"), fullPage: true, animations: "disabled" });
  await forge.click();
  await expect(page.getByRole("region", { name: "Forge paused" }).locator(".task-card.last")).toContainText("Region closed after an error");
  await page.getByRole("region", { name: "Forge paused" }).getByRole("button", { name: "Resume Forge", exact: true }).click();
  await expect(page.getByRole("region", { name: "Forging your GPU" })).toBeVisible();
  await report.click();
  await expect(coverage).toContainText("7 / 24 candidate attempts");
  await expect(coverage).toContainText("Performance · Closed");
  expect(await page.evaluate(() => window.__forgeTest.calls.filter((m) => /^(Start|Resume|Apply)/.test(m)))).toEqual(["ResumePowerSweep"]);
  await page.evaluate(() => {
    Object.assign(window.__forgeTest.power, { running: false, phase: "finished", last_outcome: "SearchBudgetExhausted" });
    window.__forgeTest.power.discovery_search.stop_reason = "attempt_budget_exhausted";
  });
  await expect(coverage).toContainText("Search ended: Candidate attempt budget reached.");
  await expect(coverage).not.toContainText("attempt_budget_exhausted");
});

test("Qualification: economics wait for top and missing evidence does not claim a boundary", async ({ page }) => {
  await openForge(page, "ready");
  await page.evaluate(() => Object.assign(window.__forgeTest.power, {
    running: true, phase: "qualify",
    discovery_search: { version: 2, attempts_used: 1, attempts_limit: 24, elapsed_ms: 1000, time_budget_ms: 28800000,
      bands: [{ id: "performance", status: "in_flight" }, { id: "balanced", status: "waiting_for_top" }, { id: "efficiency", status: "waiting_for_top" }] }
  }));
  await page.getByRole("button", { name: "Forge report ›" }).click();
  const coverage=page.getByRole("region", { name: "Candidate search coverage" });
  await expect(coverage).toContainText("Balance · Waiting for the level above");
  await expect(coverage).toContainText("First qualify the highest sustainable clock across heavy loads");
  await page.evaluate(() => { window.__forgeTest.power.discovery_search.stop_reason="evidence_incomplete_no_boundary_inferred"; });
  await expect(coverage).toContainText("No hardware boundary was inferred");
});

test("J10: a qualified result requires explicit Apply and survives a status reload", async ({ page }) => {
  await openForge(page, "qualified");
  const applied = page.locator(".profile-column.applied");
  await expect(applied).toContainText("Stock");
  const apply = page.getByRole("button", { name: "Apply Brokkr’s Best", exact: true });
  await expect(apply).toBeEnabled();
  expect(await page.evaluate(() => window.__forgeTest.calls.some((m) => m.startsWith("Apply")))).toBe(false);
  await apply.click();
  await expect(applied).toContainText("Brokkr’s Best");
  await expect(applied).toContainText("Running now");
  await expect(apply).toHaveCount(0);
  await page.reload();
  await expect(applied).toContainText("Brokkr’s Best");
  expect(await page.evaluate(() => window.__forgeTest.calls.some((m) => m.startsWith("Apply")))).toBe(false);
  await page.getByRole("button", { name: "Use stock", exact: true }).click();
  await expect(page.getByRole("button", { name: "Apply Brokkr’s Best", exact: true })).toBeEnabled();
  await expect(applied).toContainText("Stock");
  await expect(applied).not.toContainText("Brokkr’s Best");
  expect(await page.evaluate(() => window.__forgeTest.calls.filter((m) => m.startsWith("Reset")))).toEqual(["ResetGpuTuning"]);
});

for (const scenario of ["unqualified", "apply-failure"]) {
  test("J10: " + scenario + " never displays Applied", async ({ page }) => {
    await openForge(page, scenario);
    const apply = page.getByRole("button", { name: "Apply Brokkr’s Best", exact: true });
    if (scenario === "unqualified") await expect(apply).toBeDisabled();
    else {
      await apply.click();
      await expect(page.getByText("Error: Exact qualified descriptor was refused", { exact: false }).first()).toBeVisible();
    }
    await expect(page.locator(".profile-column.applied")).not.toContainText("Brokkr’s Best");
  });
}

test("UX: an active Forge with its armed candidate is not a Safe Loop alert", async ({ page }) => {
  await openForge(page);
  await page.evaluate(() => {
    Object.assign(window.__forgeTest.power, { running: true, phase: "power", run_id: "ux-run" });
    window.__forgeTest.safe.boot_flag_armed = true;
  });
  await expect(page.getByRole("region", { name: "Forging your GPU" })).toBeVisible();
  await expect(page.getByText("FORGING", { exact: true })).toBeVisible();
  await expect(page.getByText("Safe Loop needs attention", { exact: true })).toHaveCount(0);
});

test("UX: an apply's survival window reads as Verifying; a leftover flag is still an alert", async ({ page }, testInfo) => {
  await openForge(page, "qualified");
  await page.evaluate(() => {
    window.__forgeTest.applied = { type: "GpuApply", label: "Brokkr's Best", core: { freq_mhz: 1800, voltage_mv: 950 } };
    Object.assign(window.__forgeTest.safe, { boot_flag_armed: true, survival_window: true });
  });
  const safeLoop = page.locator(".system-item").filter({ hasText: "SAFE LOOP" });
  const primary = page.locator(".plate-button");
  await expect(safeLoop).toContainText("Verifying");
  await expect(primary).toHaveText("Verifying profile…");
  await expect(primary).toBeDisabled();
  await expect(page.getByText("Safe Loop needs attention", { exact: true })).toHaveCount(0);
  await page.screenshot({ path: testInfo.outputPath("survival-window.png"), fullPage: true });
  await page.evaluate(() => { window.__forgeTest.safe.survival_window = false; });
  await expect(page.getByText("Safe Loop needs attention", { exact: true })).toBeVisible();
  await expect(primary).toHaveText("Return to stock");
  await page.evaluate(() => { window.__forgeTest.safe.boot_flag_armed = false; });
  await expect(safeLoop).toContainText("Protected");
  await expect(page.getByRole("button", { name: "Use stock", exact: true })).toBeEnabled();
});

test("UX: profile columns show clock, voltage, typical power and the change from stock at a glance", async ({ page }, testInfo) => {
  await openForge(page, "qualified");
  const brokkrs = page.locator(".profile-column").filter({ hasText: "Brokkr’s Best" });
  await expect(brokkrs).toContainText("Recommended");
  await expect(brokkrs.locator(".profile-clock")).toHaveText("1800MHz");
  await expect(brokkrs).toContainText("950 mV · 155 W");
  await expect(brokkrs).toContainText("−22% power");
  await expect(page.locator(".profile-column").first()).toContainText("Stock");
  await page.screenshot({ path: testInfo.outputPath("profiles.png"), fullPage: true, animations: "disabled" });
  await expect(page.getByText(/distinct settings? ·/)).toHaveCount(0);
});

test("UX: automatic continuation is offered before a run starts", async ({ page }) => {
  await openForge(page);
  const toggle = page.getByRole("switch", { name: "Continue automatically after a driver crash" }).first();
  await expect(toggle).toBeEnabled();
  await toggle.click();
  await expect.poll(() => page.evaluate(() => window.__forgeTest.calls.includes("SetForgeAutoResume"))).toBe(true);
  expect(await page.evaluate(() => window.__forgeTest.calls.some((m) => /^(Start|Resume)/.test(m)))).toBe(false);
});

test("Updates: a found update waits in the corner, a run holds it, and one click installs it", async ({ page }, testInfo) => {
  await openForge(page, "update");
  const update = page.getByRole("button", { name: /^Update to 0\.5\.1/ });
  await expect(update).toBeVisible({ timeout: 10000 });
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await page.screenshot({ path: testInfo.outputPath("update-button.png"), animations: "disabled" });
  await update.hover();
  await page.waitForTimeout(300);
  await page.screenshot({ path: testInfo.outputPath("update-button-hover.png"), clip: { x: 880, y: 700, width: 300, height: 120 } });
  await page.evaluate(() => Object.assign(window.__forgeTest.power, { running: true, phase: "power" }));
  await expect(page.getByRole("button", { name: /^Update after the Forge run/ })).toBeDisabled();
  await page.evaluate(() => Object.assign(window.__forgeTest.power, { running: false, phase: "paused" }));
  await update.click();
  const confirm = page.getByRole("dialog", { name: "Update now?" });
  await expect(confirm).toContainText("cannot be resumed after updating");
  await confirm.getByRole("button", { name: "Not now" }).click();
  await expect(confirm).toBeHidden();
  expect(await page.evaluate(() => window.__forgeTest.calls.includes("download_and_install"))).toBe(false);
  await update.click();
  await confirm.getByRole("button", { name: "Update anyway" }).click();
  await expect.poll(() => page.evaluate(() => window.__forgeTest.calls.includes("download_and_install"))).toBe(true);
  await expect(page.getByRole("button", { name: /^Updating/ })).toBeDisabled();
});

test("Updates: after an update, the notes since the previous version show once", async ({ page }, testInfo) => {
  await openForge(page, "updated");
  const dialog = page.getByRole("dialog", { name: "Updated to 0.5.2" });
  await expect(dialog).toBeVisible({ timeout: 10000 });
  await page.screenshot({ path: testInfo.outputPath("whats-new.png"), animations: "disabled" });
  await expect(dialog.getByRole("heading", { name: "Version 0.5.2" })).toBeVisible();
  await expect(dialog.getByRole("heading", { name: "Version 0.5.1" })).toBeVisible();
  await expect(dialog.getByRole("heading", { name: "Version 0.5.0" })).toHaveCount(0);
  await expect(dialog.getByRole("region", { name: "Version 0.5.1" }).getByRole("region", { name: "Fixes" })).toBeVisible();
  await dialog.getByRole("button", { name: "Got it" }).click();
  await expect(dialog).toBeHidden();
  await page.reload();
  await expect(page.locator(".plate-button")).toBeVisible();
  expect(await page.evaluate(() => localStorage.getItem("nidavellir-last-version"))).toBe("0.5.2");
  await expect(page.getByRole("dialog", { name: "Updated to 0.5.2" })).toBeHidden();
});

test("Program: window and startup options save through the desktop bridge", async ({ page }, testInfo) => {
  await openForge(page);
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  const close = page.getByRole("switch", { name: /Close to tray/ });
  const start = page.getByRole("switch", { name: /Start with Windows/ });
  await expect(close).toBeChecked();
  await expect(start).not.toBeChecked();
  await page.getByRole("heading", { name: "Window and startup" }).scrollIntoViewIfNeeded();
  await page.screenshot({ path: testInfo.outputPath("settings-window.png") });
  await start.click();
  await expect(start).toBeChecked();
  expect(await page.evaluate(() => window.__forgeTest.window)).toEqual({ closeToTray: true, minimizeToTray: false, startWithWindows: true });
});

test("Sentinel: the GPU check is off by default and is switched in Settings", async ({ page }, testInfo) => {
  await openForge(page);
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  const check = page.getByRole("switch", { name: /GPU check while gaming/ });
  await expect(check).not.toBeChecked();
  await page.getByRole("heading", { name: "Sentinel" }).scrollIntoViewIfNeeded();
  await page.screenshot({ path: testInfo.outputPath("settings-sentinel.png") });
  await expect(check).toBeEnabled();
  await check.click();
  await expect(check).toBeChecked();
  expect(await page.evaluate(() => window.__forgeTest.sentinelRequests)).toEqual([true]);
  await check.click();
  await expect(check).not.toBeChecked();
  expect(await page.evaluate(() => window.__forgeTest.sentinelRequests)).toEqual([true, false]);
  expect(await page.evaluate(() => window.__forgeTest.calls.some((m) => /^(Start|Resume|Apply)/.test(m)))).toBe(false);
});

test("Sentinel: a recorded problem asks for the GPU check and one click turns it on", async ({ page }, testInfo) => {
  await openForge(page);
  const advice = "Sentinel recorded a problem. Turn on the GPU check to analyse the applied profile.";
  await page.evaluate((text) => { window.__forgeTest.power.sentinel_advice = text; }, advice);
  await expect(page.getByText(advice)).toBeVisible();
  await page.getByText(advice).scrollIntoViewIfNeeded();
  await page.screenshot({ path: testInfo.outputPath("sentinel-advice.png") });
  await page.getByRole("button", { name: "Turn on the GPU check" }).click();
  await expect(page.getByText(advice)).toHaveCount(0);
  expect(await page.evaluate(() => window.__forgeTest.sentinelRequests)).toEqual([true]);
  // The check already runs: no advice even if the service still reported one.
  await page.evaluate((text) => { window.__forgeTest.power.sentinel_advice = text; }, advice);
  await page.waitForTimeout(1200);
  await expect(page.getByText(advice)).toHaveCount(0);
});

test("Program: hidden in the tray the window stops polling the Core, and catches up when shown", async ({ page }) => {
  await openForge(page);
  const count = (method) => page.evaluate((m) => window.__forgeTest.calls.filter((c) => c === m).length, method);
  await page.waitForFunction(() => window.__tauriListening("window-visibility"));
  // Idle (no run): the status refreshes about every 2 s, not every 500 ms.
  const idleStart = await count("GetPowerSweepProgress");
  await page.waitForTimeout(4200);
  expect((await count("GetPowerSweepProgress")) - idleStart).toBeLessThanOrEqual(3);
  await page.evaluate(() => window.__tauriEmit("window-visibility", false));
  await page.waitForTimeout(300);
  const hidden = await page.evaluate(() => window.__forgeTest.calls.length);
  await page.waitForTimeout(3000);
  expect(await page.evaluate(() => window.__forgeTest.calls.length)).toBe(hidden);
  await page.evaluate(() => window.__tauriEmit("window-visibility", true));
  await expect.poll(() => count("ReadSensors")).toBeGreaterThan(0);
  await expect.poll(() => page.evaluate(() => window.__forgeTest.calls.length)).toBeGreaterThan(hidden);
});

test("Program: Exit during a Forge run asks first, then stops the run and exits", async ({ page }, testInfo) => {
  await openForge(page);
  await page.waitForFunction(() => window.__tauriListening("exit-requested"));
  await page.evaluate(() => window.__tauriEmit("exit-requested", null));
  const dialog = page.getByRole("dialog", { name: "Exit Nidavellir?" });
  await expect(dialog).toContainText("returns the GPU to stock");
  await page.screenshot({ path: testInfo.outputPath("exit-dialog.png") });
  await dialog.getByRole("button", { name: "Keep running" }).click();
  await expect(dialog).toBeHidden();
  expect(await page.evaluate(() => window.__forgeTest.calls.includes("exit_program"))).toBe(false);
  await page.evaluate(() => window.__tauriEmit("exit-requested", null));
  await dialog.getByRole("button", { name: "Stop run and exit" }).click();
  await expect.poll(() => page.evaluate(() => window.__forgeTest.calls.includes("exit_program"))).toBe(true);
});

test("Program: a failed tray action is shown in the window", async ({ page }) => {
  await openForge(page);
  await page.waitForFunction(() => window.__tauriListening("tray-notice"));
  await page.evaluate(() => window.__tauriEmit("tray-notice", "Could not apply Godforge: Exact qualified descriptor was refused"));
  await expect(page.getByRole("alert").filter({ hasText: "Could not apply Godforge" })).toBeVisible();
});

test("Startup: a startup screen covers the Core start and the profile reapply", async ({ page }, testInfo) => {
  await openForge(page, "starting");
  await page.waitForFunction(() => window.__tauriListening("core-session"));
  const forge = page.locator("section.forge");
  const done = page.locator(".startup li.done");
  const running = page.locator(".startup li[aria-current='step']");
  await expect(done).toHaveText(["Starting the Core Service", "Checking Safe Loop"]);
  // The program names the profile the Core saved, whichever the user applied.
  await page.evaluate(() => window.__tauriEmit("core-session", { ready: false, profile: "Brokkr's Best" }));
  await expect(running).toHaveText("Applying Brokkr’s Best…");
  await expect(forge).toHaveAttribute("inert", "");
  await page.waitForTimeout(300); // the new step's entrance
  await page.screenshot({ path: testInfo.outputPath("startup.png") });
  await page.evaluate(() => window.__tauriEmit("core-session", { ready: true }));
  await expect(page.locator(".startup")).toHaveCount(0);
  await expect(forge).not.toHaveAttribute("inert");
  await expect(page.locator(".plate-button")).toHaveText("Forge GPU");
});

test("Startup: with no saved profile the startup screen shows the stock confirmation", async ({ page }) => {
  await openForge(page, "starting");
  await page.waitForFunction(() => window.__tauriListening("core-session"));
  await page.evaluate(() => window.__tauriEmit("core-session", { ready: false, profile: null }));
  await expect(page.locator(".startup li[aria-current='step']")).toHaveText("Confirming stock settings…");
  await expect(page.locator(".startup li").filter({ hasText: "Applying" })).toHaveCount(0);
  await page.evaluate(() => window.__tauriEmit("core-session", { ready: true }));
  await expect(page.locator(".startup")).toHaveCount(0);
});

test("UX: the state divider follows a long state word", async ({ page }) => {
  await openForge(page, "connecting");
  const state = page.locator(".state-status .state-value");
  await expect(state).toHaveText("CONNECTING");
  const [word, status] = await Promise.all([state.boundingBox(), page.locator(".state-status > div + div").boundingBox()]);
  expect(word.x + word.width).toBeLessThan(status.x);
});

test("Navigation: Live log and Sentinel are pages; the diagnostic tools for development are gone", async ({ page }, testInfo) => {
  await openForge(page);
  await page.getByRole("button", { name: "Live log", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Live log", level: 1 })).toBeVisible();
  await page.getByRole("button", { name: "Sentinel", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Sentinel", level: 1 })).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath("sentinel-page.png"), fullPage: true, animations: "disabled" });
  await page.getByRole("button", { name: "Forge", exact: true }).click();
  await expect(page.locator(".plate-button")).toBeVisible();
  await expect(page.getByText(/Advanced diagnostics|Game Trace|Manual point|Detector Lab/)).toHaveCount(0);
  expect(await page.evaluate(() => window.__forgeTest.calls.some((m) => /GameTrace|ManualDiagnostic|DetectorLab/.test(m)))).toBe(false);
});

test("Layout: at the minimum window width the header never wraps, clips or overlaps", async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 1100, height: 720 });
  await openForge(page, "qualified");
  const nav = await page.locator(".command-nav").boundingBox();
  const status = await page.locator(".command-system-status").boundingBox();
  expect(nav.x + nav.width).toBeLessThanOrEqual(status.x + 1);
  const wrapped = await page.locator(".command-nav button").evaluateAll((buttons) =>
    buttons.filter((button) => {
      const range = document.createRange();
      range.selectNodeContents(button);
      return range.getClientRects().length > 1;
    }).map((button) => button.textContent),
  );
  expect(wrapped).toEqual([]);
  const clipped = await page.locator(".system-item small, .system-item strong").evaluateAll((items) =>
    items.filter((item) => item.scrollWidth > item.clientWidth).map((item) => item.textContent),
  );
  expect(clipped).toEqual([]);
  for (const view of ["Forge", "Live log", "Sentinel", "Settings"]) {
    await page.getByRole("button", { name: view, exact: true }).click();
    await page.mouse.move(600, 400);
    // The nav color eases over 150 ms.
    await expect.poll(() => page.locator(".command-nav button").evaluateAll((buttons) =>
      buttons.filter((button) => getComputedStyle(button).color === "rgb(233, 186, 121)").map((button) => button.textContent),
    )).toEqual([view]);
    await page.screenshot({ path: testInfo.outputPath(`${view.replace(" ", "-").toLowerCase()}-1100.png`), fullPage: true, animations: "disabled" });
  }
  await page.getByRole("button", { name: "Full Reset…", exact: true }).click();
  await page.getByRole("dialog").getByLabel("Type ERASE to confirm").fill("ERA");
  await page.screenshot({ path: testInfo.outputPath("full-reset-dialog-1100.png") });
});

test("Pre-Forge: the sheet explains the run, Cancel starts nothing and Start uses the chosen mode", async ({ page }, testInfo) => {
  await openForge(page);
  await page.locator(".plate-button").click();
  const sheet = page.getByRole("dialog", { name: "Before the Forge starts" });
  await expect(sheet).toBeVisible();
  await expect(sheet.getByRole("button", { name: "Cancel" })).toBeFocused();
  await expect(sheet).toContainText("The screen may flicker or go black");
  await expect(sheet).toContainText("The failed point is saved and never tested again");
  await expect(sheet.getByRole("switch", { name: "Continue automatically after a driver crash" })).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath("pre-forge.png"), animations: "disabled" });
  await sheet.getByRole("button", { name: "Cancel" }).click();
  await expect(sheet).toBeHidden();
  expect(await page.evaluate(() => window.__forgeTest.calls.some((m) => /^(Start|Resume)/.test(m)))).toBe(false);
  await page.locator(".plate-button").click();
  await sheet.getByRole("radio", { name: "Long" }).click();
  await expect(sheet.getByRole("radio", { name: "Long" })).toHaveAttribute("aria-checked", "true");
  await expect(sheet).toContainText("Up to about 18 h");
  await sheet.getByRole("button", { name: "Start Forge" }).click();
  await expect.poll(() => page.evaluate(() => window.__forgeTest.calls.filter((m) => m.startsWith("Start")))).toEqual(["StartPowerSweepLong"]);
});

test("Live GPU: the monitor charts the readings against the applied profile, and Settings switches to gauges", async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 1180, height: 1300 });
  await openForge(page, "qualified");
  await page.evaluate(() => {
    window.__forgeTest.applied = { type: "GpuApply", label: "Brokkr's Best", core: { freq_mhz: 1800, voltage_mv: 950 } };
    window.__forgeTest.sensors = [(n) => ({
      name: "NVIDIA GeForce RTX 3060 Ti", utilization_pct: 97, vram_total_mb: 8192, memory_clock_mhz: 7001,
      core_clock_mhz: n % 4 === 0 ? 1785 : 1800, voltage_mv: 950, power_w: 150 + (n % 5) * 6, temperature_c: 60 + (n % 6),
      fan_speed_pct: 55, power_limit_w: 200,
    })];
  });
  const live = page.locator(".live-band");
  await expect(live).toContainText("against Brokkr’s Best");
  const clock = live.locator(".strip").filter({ hasText: "Clock" });
  await expect(clock).toContainText("target 1800");
  await expect(live.locator(".strip").filter({ hasText: "Voltage" })).toContainText("within cap");
  await expect(live.locator(".strip").filter({ hasText: "Power" })).toContainText("limit 200 W");
  await expect.poll(() => clock.locator("polyline").getAttribute("points").then((p) => (p ?? "").split(" ").filter(Boolean).length), { timeout: 10_000 }).toBeGreaterThan(2);
  await live.scrollIntoViewIfNeeded();
  await page.screenshot({ path: testInfo.outputPath("live-monitor.png"), fullPage: true, animations: "disabled" });
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByRole("button", { name: /Gauges/ }).click();
  await page.getByRole("button", { name: "Forge", exact: true }).click();
  await expect(live.locator(".gauge")).toHaveCount(4);
  await expect(live).toContainText("Target 1800 MHz");
  await page.screenshot({ path: testInfo.outputPath("live-gauges.png"), fullPage: true, animations: "disabled" });
  await page.reload();
  await expect(page.locator(".live-band .gauge")).toHaveCount(4);
});

test("Last Forge: a settled run shows its levels and the report holds the details", async ({ page }, testInfo) => {
  await openForge(page, "qualified");
  await page.evaluate(() => {
    Object.assign(window.__forgeTest.power, {
      elapsed_ms: 20_880_000, note: "F2 forge summary",
      discovery_search: {
        attempts_used: 24, attempts_limit: 30, elapsed_ms: 20_880_000, time_budget_ms: 28_800_000, stop_reason: "all_regions_closed",
        bands: [
          { id: "performance", status: "closed", last_qualified_clock_mhz: 1920, last_qualified_voltage_mv: 912 },
          { id: "compensated", status: "closed", last_qualified_clock_mhz: 1830, last_qualified_voltage_mv: 900 },
          { id: "balanced", status: "closed", last_qualified_clock_mhz: 1800, last_qualified_voltage_mv: 837 },
          { id: "efficiency", status: "closed", last_qualified_clock_mhz: 1740, last_qualified_voltage_mv: 787 },
        ],
      },
    });
  });
  const last = page.getByRole("region", { name: "Last Forge" });
  await expect(last.locator(".levels li")).toHaveCount(4);
  await expect(last).toContainText("24 candidate tests");
  await expect(last.locator(".levels li").first()).toContainText("1920 MHz @ 912 mV");
  await expect(last.locator(".levels li").first()).toContainText("Highest clock that held");
  await expect(last.locator(".levels li").nth(1)).toContainText("Godforge runs at 950 mV");
  await expect(last).not.toContainText("Run budget");
  await page.screenshot({ path: testInfo.outputPath("last-forge.png"), fullPage: true, animations: "disabled" });
  await last.getByRole("button", { name: "Forge report ›" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toContainText("Forge");
  await expect(page.getByRole("region", { name: "Candidate search coverage" })).toContainText("Run budget");
  await expect(page.getByRole("button", { name: "Forge", exact: true })).toHaveAttribute("aria-current", "page");
});

test("Live log: events read in English with a symbol per tone, filter, show details and mark what runs now", async ({ page }, testInfo) => {
  await openForge(page);
  const at = (minutes) => Date.UTC(2026, 9, 12, 21, minutes, 0);
  await page.evaluate(([t0, t1, t2, t3, t4]) => {
    Object.assign(window.__forgeTest.power, {
      running: true, phase: "qualify",
      log: ["linha técnica"],
      log_entries: [
        { at_ms: t0, tone: "step", phase: "Stock baseline", text: "Measuring the GPU at stock until it settles", detail: "up to 6 windows of 30 s" },
        { at_ms: t1, tone: "pass", phase: "Stock baseline", text: "Stock baseline ready: 1740 MHz at 71 °C" },
        { at_ms: t2, tone: "attention", phase: "Performance", text: "1920 MHz at 912 mV · texture test: clock dropped" },
        { at_ms: t3, tone: "fail", phase: "Performance", text: "1920 MHz at 900 mV failed: integrity error", detail: "SilentError" },
        { at_ms: t4, tone: "step", phase: "Balance", text: "Testing 1800 MHz at 837 mV · DX12 game load, test 4 of 7" },
      ],
    });
  }, [at(4), at(6), at(15), at(21), at(30)]);
  // On the Forge screen a running Forge shows its three latest events and a way to the Live log.
  const recent = page.getByRole("region", { name: "Forging your GPU" }).locator(".recent li");
  await expect(recent).toHaveCount(3);
  await expect(recent.last()).toContainText("Testing 1800 MHz at 837 mV");
  await page.screenshot({ path: testInfo.outputPath("forge-running.png"), fullPage: true, animations: "disabled" });
  await page.getByRole("button", { name: "Live log ›" }).click();
  await expect(page.getByRole("heading", { name: "Live log", level: 1 })).toBeVisible();
  await page.getByRole("button", { name: "Forge", exact: true }).click();
  await page.getByRole("button", { name: "Live log", exact: true }).click();
  const log = page.getByRole("log");
  await expect(log.locator(".line")).toHaveCount(5);
  await expect(log.locator(".line.tone-fail .glyph")).toHaveText("✕");
  await expect(log.locator(".line.tone-pass .glyph")).toHaveText("✓");
  await expect(log.locator(".line.tone-attention .glyph")).toHaveText("!");
  await expect(log.locator(".line.now")).toContainText("Testing 1800 MHz at 837 mV");
  await expect(log.locator(".line.now .glyph")).toHaveText("›");
  await expect(log).not.toContainText("linha técnica");
  await expect(log).not.toContainText("SilentError");
  await page.getByRole("switch", { name: "Technical details" }).check();
  await expect(log).toContainText("SilentError");
  await page.screenshot({ path: testInfo.outputPath("live-log.png"), fullPage: true, animations: "disabled" });
  await page.getByRole("button", { name: /^Results/ }).click();
  await expect(log.locator(".line:not(.detail)")).toHaveCount(2);
  await page.getByRole("button", { name: /^Attention/ }).click();
  await expect(log.locator(".line:not(.detail)")).toHaveCount(2);
  await expect(page.getByRole("button", { name: /^All/ })).toContainText("5");
  // A run saved before the Live log only has technical lines; they still read, without a phase.
  await page.evaluate(() => Object.assign(window.__forgeTest.power, { running: false, log_entries: [], log: ["Busca encerrada: attempt_budget_exhausted"] }));
  await page.getByRole("button", { name: /^All/ }).click();
  await expect(log.locator(".line")).toHaveCount(1);
  await expect(log).toContainText("Busca encerrada");
});

test("Sentinel: the page shows protection, what happened and the blocked points next to the profiles", async ({ page }, testInfo) => {
  await openForge(page, "qualified");
  await page.evaluate(() => {
    window.__forgeTest.applied = { type: "GpuApply", label: "Brokkr's Best", core: { freq_mhz: 1800, voltage_mv: 950 } };
    window.__forgeTest.safe.condemnations = [
      { timestamp: "2026-10-05T22:14:00Z", kind: "field-tdr", severity: "rigid", target_mhz: 1830, vf_bin_mv: 925, rehabilitated: false },
      { timestamp: "2026-10-02T03:10:00Z", kind: "candidate-crash", severity: "rigid", target_mhz: 1890, vf_bin_mv: 900, rehabilitated: false },
    ];
    window.__forgeTest.safe.blacklist = [{ center: { axes: { gpu_freq_mhz: 1740, gpu_vf_bin_mv: 900 } }, radius: 1 }];
  });
  await page.getByRole("button", { name: "Sentinel", exact: true }).click();
  const sentinel = page.getByRole("region", { name: "Sentinel" });
  await expect(sentinel).toContainText("Watching Brokkr's Best");
  await expect(sentinel).toContainText("Always on");
  const events = sentinel.locator(".events li");
  await expect(events).toHaveCount(2);
  await expect(events.first()).toContainText("Driver crash while playing");
  await expect(events.first()).toContainText("At 1830 MHz and 925 mV");
  await expect(sentinel.locator(".map-side")).toContainText("3 points");
  await expect(sentinel.locator("circle.profile")).toHaveCount(3);
  await page.screenshot({ path: testInfo.outputPath("sentinel.png"), fullPage: true, animations: "disabled" });
  await sentinel.getByRole("button", { name: "Turn on" }).click();
  await expect.poll(() => page.evaluate(() => window.__forgeTest.calls.includes("SetSentinelCanary"))).toBe(true);
  await sentinel.getByRole("button", { name: /Show the list/ }).click();
  await expect(sentinel.locator(".blocked-list li")).toHaveCount(3);
});
