<script>
  import { Activity, ArrowRight, Clock3, Play, ShieldCheck, Square, Timer } from "@lucide/svelte";

  let {
    powerSweep = null,
    powerRunning = false,
    safeLoop = null,
    forgeMode = "standard",
    onStopPower,
    onRecoverContinue,
    onResumePower,
  } = $props();

  const FORGE_PHASES = [
    { id: "prepare", label: "Prepare" },
    { id: "test", label: "Test candidates" },
    { id: "compare", label: "Compare" },
    { id: "restore", label: "Restore" },
  ];
  const finishFormatter = new Intl.DateTimeFormat(undefined, {
    hour: "2-digit",
    minute: "2-digit",
  });

  let now = $state(Date.now());
  let observedElapsed = $state(null);
  let observedRemaining = $state(null);
  let elapsedBase = $state(null);
  let remainingBase = $state(null);
  let timingObservedAt = $state(Date.now());
  let observedTask = $state(null);
  let taskElapsedBase = $state(0);
  let taskObservedAt = $state(Date.now());
  let observedRunId = $state(null);
  let latchedLastOutcome = $state(null);

  const hasRun = $derived(Boolean(powerSweep && powerSweep.phase !== "idle"));
  const isInterrupted = $derived(powerSweep?.phase === "interrupted");
  const isPaused = $derived(powerSweep?.phase === "paused");
  const isStopping = $derived(powerSweep?.phase === "stopping");
  const isFinished = $derived(powerSweep?.phase === "finished");
  const isProvisional = $derived(powerSweep?.phase === "provisional");
  const reportedElapsedMs = $derived(validDuration(powerSweep?.elapsed_ms));
  const reportedRemainingMs = $derived(validDuration(powerSweep?.estimated_remaining_ms));
  const estimatedTotalUpperMs = $derived(validDuration(powerSweep?.estimated_total_upper_ms));
  const currentTaskReportedMs = $derived(validDuration(powerSweep?.current_task_elapsed_ms) ?? 0);
  const elapsedMs = $derived(
    elapsedBase == null
      ? null
      : elapsedBase + (powerRunning ? Math.max(0, now - timingObservedAt) : 0),
  );
  const remainingMs = $derived(
    remainingBase == null
      ? null
      : Math.max(0, remainingBase - (powerRunning ? Math.max(0, now - timingObservedAt) : 0)),
  );
  const upperRemainingMs = $derived(
    estimatedTotalUpperMs == null || elapsedMs == null
      ? null
      : Math.max(0, estimatedTotalUpperMs - elapsedMs),
  );
  const conservativeRemainingMs = $derived(
    upperRemainingMs == null
      ? null
      : remainingMs == null
        ? upperRemainingMs
        : Math.max(remainingMs, upperRemainingMs),
  );
  const taskElapsedMs = $derived(
    powerRunning ? taskElapsedBase + Math.max(0, now - taskObservedAt) : taskElapsedBase,
  );
  const taskEstimatedTotalMs = $derived(validDuration(powerSweep?.current_task_estimated_total_ms));
  const taskRemainingMs = $derived(
    taskEstimatedTotalMs == null ? null : Math.max(0, taskEstimatedTotalMs - taskElapsedMs),
  );
  const phaseInfo = $derived(stageInfo(powerSweep?.phase, powerRunning, powerSweep?.profiles_qualified));
  const currentTaskLabel = $derived(taskLabel(powerSweep?.current_task) ?? phaseInfo.label);
  const nextTaskLabel = $derived(
    taskLabel(powerSweep?.next_task) ?? terminalNextTask(powerSweep?.phase, powerSweep?.resume_available),
  );
  const nextTaskDurationMs = $derived(validDuration(powerSweep?.next_task_estimated_duration_ms));
  const completedSteps = $derived(Math.max(0, Number(powerSweep?.completed_steps ?? 0)));
  const totalSteps = $derived(Math.max(0, Number(powerSweep?.total_steps_estimate ?? 0)));
  const currentPhaseIndex = $derived(forgePhaseIndex(powerSweep?.current_task, powerSweep?.phase));
  const progressPercent = $derived.by(() => {
    if (isFinished) return 100;
    if (totalSteps > 0) return clampPercent((completedSteps / totalSteps) * 100);
    if (powerRunning && elapsedMs != null && remainingMs != null && elapsedMs + remainingMs > 0) {
      return clampPercent((elapsedMs / (elapsedMs + remainingMs)) * 100);
    }
    if (hasRun && currentPhaseIndex != null) {
      return clampPercent((currentPhaseIndex / FORGE_PHASES.length) * 100);
    }
    return powerRunning ? 3 : 0;
  });
  const remainingEstimate = $derived(
    powerRunning ? (remainingMs == null ? "Calculating" : duration(remainingMs)) : "—",
  );
  const remainingCeiling = $derived(
    powerRunning
      ? (conservativeRemainingMs == null ? "Calculating" : duration(conservativeRemainingMs))
      : "—",
  );
  const estimatedFinishWindow = $derived(
    finishWindow(now, remainingMs, conservativeRemainingMs, powerRunning, isFinished),
  );
  const currentPairLabel = $derived(pairLabel(powerSweep, powerRunning));
  const currentPairHeading = $derived(powerRunning ? "Current pair" : "Last measured pair");
  const stepLabel = $derived(stepProgress(completedSteps, totalSteps));
  const lastDecision = $derived(
    outcomeInfo(latchedLastOutcome, isFinished, powerSweep?.profiles_qualified),
  );
  const discoverySearch = $derived(powerSweep?.discovery_search ?? null);
  const searchBands = $derived(discoverySearch?.bands ?? []);
  const qualifiedBands = $derived(searchBands.filter((band) => band.last_qualified_clock_mhz > 0 && band.last_qualified_voltage_mv > 0).length);
  const rebootRequired = $derived(Boolean(safeLoop?.gpu_reboot_required));
  const canResume = $derived(
    Boolean(!rebootRequired && !powerRunning && isPaused && powerSweep?.resume_available),
  );
  const safetyLabel = $derived.by(() => {
    if (!safeLoop) return "Protection pending";
    if (rebootRequired) return "Restart Windows";
    if (safeLoop.safe_mode || safeLoop.state === "unstable") return "Needs attention";
    if (safeLoop.boot_flag_armed || safeLoop.recovery_pending_ack) return "Recovery ready";
    return "Protected";
  });
  const title = $derived(
    rebootRequired
      ? "Restart Windows to continue"
      : isInterrupted
        ? "Forge interrupted"
        : isPaused
          ? "Forge paused"
          : isProvisional
            ? "Forge preview ready"
            : isFinished
              ? "Forge complete"
              : powerRunning
                ? "Forging your GPU"
                : "Forge progress",
  );
  const runState = $derived(
    rebootRequired
      ? "Restart required"
      : isStopping
        ? "Stopping safely"
        : powerRunning
          ? "Running"
          : isInterrupted
            ? "Interrupted"
            : isPaused
              ? "Paused"
              : isProvisional
                ? "Qualification pending"
                : isFinished
                  ? "Complete"
                  : "Idle",
  );
  const liveAnnouncement = $derived.by(() => {
    const parts = [runState];
    if (powerSweep?.current_task) parts.push(currentTaskLabel);
    if (latchedLastOutcome) parts.push(lastDecision.label);
    return parts.join(". ");
  });

  function validDuration(value) {
    if (value == null || value === "") return null;
    const number = Number(value);
    return Number.isFinite(number) && number >= 0 ? number : null;
  }

  function clampPercent(value) {
    return Math.min(100, Math.max(0, value));
  }

  function duration(value) {
    const milliseconds = validDuration(value);
    if (milliseconds == null) return "Calculating";
    const totalSeconds = Math.max(0, Math.round(milliseconds / 1000));
    const hours = Math.floor(totalSeconds / 3600);
    const minutes = Math.floor((totalSeconds % 3600) / 60);
    const seconds = totalSeconds % 60;
    if (hours) return `${hours}h ${minutes}m`;
    if (minutes) return `${minutes}m ${seconds}s`;
    return `${seconds}s`;
  }

  function finishWindow(timestamp, estimate, ceiling, running, finished) {
    if (finished) return "Complete";
    if (!running) return "—";
    const estimateMs = validDuration(estimate);
    const ceilingMs = validDuration(ceiling);
    if (estimateMs == null && ceilingMs == null) return "Calculating";
    if (estimateMs == null) return `By ${finishFormatter.format(new Date(timestamp + ceilingMs))}`;
    if (ceilingMs == null) return `Around ${finishFormatter.format(new Date(timestamp + estimateMs))}`;
    const earliest = Math.min(estimateMs, ceilingMs);
    const latest = Math.max(estimateMs, ceilingMs);
    const start = finishFormatter.format(new Date(timestamp + earliest));
    const end = finishFormatter.format(new Date(timestamp + latest));
    return start === end ? `Around ${start}` : `${start}–${end}`;
  }

  function taskLabel(task) {
    if (!task) return null;
    const labels = {
      prepare_stock: "Preparing a clean stock state",
      stock_preheat: "Normalizing temperature and stock clock",
      capture_goldens: "Capturing stock render references",
      frontier_descent: "Screening the current candidate",
      candidate_screening: "Screening the current candidate",
      candidate_selection: "Choosing the next candidate within the search budget",
      candidate_qualification: "Qualifying the current candidate",
      power_calibration: "Measuring real Apply power",
      profile_synthesis: "Forging the three profile goals",
      apply_calibration: "Measuring exact Apply pairs",
      synthesize_profiles: "Forging the three profile goals",
      apply_qualification: "Qualifying the current candidate",
      publish_profiles: "Publishing qualified profiles",
      final_stock_reset: "Restoring and verifying stock state",
      final_reset: "Restoring and verifying stock state",
    };
    return labels[task] ?? String(task).replaceAll("_", " ");
  }

  function forgePhaseIndex(task, phase) {
    const taskPhases = {
      prepare_stock: 0,
      stock_preheat: 0,
      capture_goldens: 0,
      frontier_descent: 1,
      candidate_screening: 1,
      candidate_selection: 1,
      candidate_qualification: 1,
      power_calibration: 1,
      profile_synthesis: 2,
      apply_calibration: 1,
      synthesize_profiles: 2,
      apply_qualification: 1,
      publish_profiles: 2,
      final_stock_reset: 3,
      final_reset: 3,
    };
    if (task && taskPhases[task] != null) return taskPhases[task];
    const phaseIndexes = {
      preheat: 0,
      power: 1,
      descend: 1,
      calibrate: 1,
      synthesize: 2,
      provisional: 1,
      "apply-qualify": 1,
      validate: 1,
      stopping: 3,
      finished: 3,
    };
    return phaseIndexes[phase] ?? null;
  }

  function phaseStatus(index) {
    if (isFinished) return "complete";
    if (currentPhaseIndex == null) return "pending";
    if (index < currentPhaseIndex) return "complete";
    if (index === currentPhaseIndex) return "active";
    return "pending";
  }

  function pairLabel(progress, running) {
    const clock = Number(progress?.current_clock_mhz);
    const voltage = Number(progress?.current_voltage_mv);
    const hasClock = Number.isFinite(clock) && clock > 0;
    const hasVoltage = Number.isFinite(voltage) && voltage > 0;
    if (hasClock && hasVoltage) return `${clock} MHz @ ${voltage} mV`;
    if (hasClock) return `${clock} MHz · voltage pending`;
    if (hasVoltage) return `${voltage} mV · clock pending`;
    return running ? "Selecting a measured pair" : "—";
  }

  function stepProgress(completed, estimatedTotal) {
    if (estimatedTotal > 0) return `${Math.min(completed, estimatedTotal)} done · ~${estimatedTotal} planned`;
    if (completed > 0) return `${completed} completed`;
    return "Awaiting the first measured step";
  }

  function bandLabel(id) {
    return { performance: "Performance", balanced: "Balance", efficiency: "Efficiency" }[id] ?? id;
  }

  function bandStatus(status) {
    return { waiting_for_top: "Waiting for qualified top", pending: "Pending", in_flight: "Testing", closed: "Closed" }[status] ?? "Pending";
  }

  function searchStopReason(reason) {
    const labels = {
      attempt_budget_exhausted: "Candidate attempt budget reached.",
      time_budget_exhausted: "There is not enough run time left for another complete qualification.",
      driver_failure_recovery_required: "A driver failure requires recovery before tuning can continue.",
      operational_failure: "An operation could not be completed. Review the run details.",
      integrity_error_budget_exhausted: "Integrity errors reached this run's limit.",
      power_integrity_boundary: "The next clock needs more voltage than the tested power envelope permits.",
      qualified_top_unavailable: "No sustainable top was qualified. Review the measurement refusals before a new run.",
      evidence_incomplete_no_boundary_inferred: "Evidence was insufficient. No hardware boundary was inferred; review the exact refusal in the run log.",
      physical_clock_domain_exhausted: "Reached the end of the admissible clock domain.",
      control_reapplication_failed: "Clock or voltage containment failed again after the one allowed reapplication. No point was approved from that test.",
      all_regions_closed: "All search regions have been closed.",
      physical_domain_exhausted: "No further admissible point is available in this region.",
      power_preparation_exhausted: "Power-limited preparation reached its allowed limit.",
      integrity_error_region_closed: "An integrity error ended exploration of this region.",
      inconclusive_region_closed: "The available evidence could not support further exploration.",
      invalid_search_plan: "The candidate search plan is incomplete or invalid. A new compatible run is required.",
      incompatible_search_version: "This saved search uses a different discovery version. Start a new run.",
    };
    return labels[reason] ?? String(reason).replaceAll("_", " ");
  }

  function outcomeInfo(outcome, finished, qualified) {
    if (finished && !qualified) {
      return {
        label: "No profile published",
        detail: "The run ended without enough qualified evidence to publish a profile.",
        tone: "caution",
      };
    }
    if (finished && qualified && !outcome) {
      return {
        label: "Profiles qualified",
        detail: "The final Apply pairs passed the required qualification gates.",
        tone: "success",
      };
    }
    if (!outcome) {
      return {
        label: "No decision yet",
        detail: "The first measured result will appear here.",
        tone: "neutral",
      };
    }

    const raw = String(outcome);
    const key = raw.toLowerCase().replaceAll(/[^a-z0-9]/g, "");
    if (["tdrrisk", "censored", "skipped"].some((token) => key.includes(token))) {
      return {
        label: "Skipped for safety",
        detail: "Nidavellir refused this point before treating it as positive evidence.",
        tone: "caution",
      };
    }
    if (["tdrorcrash", "devicelost", "crash"].some((token) => key.includes(token))) {
      return {
        label: "Interrupted · recovery",
        detail: "The device or driver stopped responding; the run failed closed.",
        tone: "danger",
      };
    }
    if (["armfailed", "applyfailed", "verifyfailed", "resetfailed", "operationalfailure", "aborted"].some((token) => key.includes(token))) {
      return {
        label: "Interrupted · safety check",
        detail: "A protected write, verification or reset step could not be confirmed.",
        tone: "danger",
      };
    }
    if (key === "cancelled") {
      return {
        label: "Candidate stopped",
        detail: "This attempt did not qualify the pair. Its spent search budget is retained.",
        tone: "neutral",
      };
    }
    if (["silenterror", "unstable", "rejected", "failed"].some((token) => key.includes(token))) {
      return {
        label: "Rejected · unstable",
        detail: "The measured point did not meet the stability requirement.",
        tone: "danger",
      };
    }
    if (key.includes("powerbound")) {
      return {
        label: "Inconclusive · power limit",
        detail: "The power envelope prevented a clean stability decision for this point.",
        tone: "caution",
      };
    }
    if (["clockdrop", "residency"].some((token) => key.includes(token))) {
      return {
        label: "Inconclusive · low residency",
        detail: "The load did not hold the target strongly enough to accept or condemn the point.",
        tone: "caution",
      };
    }
    if (key.includes("inconclusive")) {
      return {
        label: "Inconclusive",
        detail: "Evidence was insufficient to qualify or condemn this point. Review the recorded reason.",
        tone: "caution",
      };
    }
    if (key === "searchbudgetexhausted") {
      return {
        label: "Search budget reached",
        detail: "Exploration ended within the run budget. Untested points are not classified as unstable.",
        tone: "caution",
      };
    }
    if (key === "bandclosedintegrityerror") {
      return {
        label: "Region closed after an error",
        detail: "The measured candidate failed integrity checks. Further exploration of this region has stopped.",
        tone: "caution",
      };
    }
    if (key === "candidatequalified") {
      return {
        label: "Candidate qualified",
        detail: "The recorded pair passed the complete qualification matrix and stock restoration checks.",
        tone: "success",
      };
    }
    if (key === "eligibleforqualification") {
      return {
        label: "Eligible for qualification",
        detail: "Screening passed. The complete matrix is still required before this pair can become a profile or allow refinement.",
        tone: "neutral",
      };
    }
    if (["qualified", "validated", "stable", "passed", "pass"].some((token) => key.includes(token))) {
      return {
        label: "Stage passed",
        detail: "This stage passed. A profile requires the complete qualification matrix and verified stock restoration.",
        tone: "neutral",
      };
    }
    return {
      label: "Evidence recorded",
      detail: raw.replaceAll(/([a-z])([A-Z])/g, "$1 $2"),
      tone: "neutral",
    };
  }

  function stageInfo(phase, running, qualified) {
    if (!running && phase === "finished") {
      return qualified
        ? { label: "Profiles forged", detail: "Profiles were selected from fully qualified candidates. The GPU returned to its verified final state." }
        : { label: "Search ended", detail: "The run ended without enough qualified evidence to publish a profile." };
    }
    if (!running && phase === "provisional") {
      return { label: "Preview complete", detail: "The map is ready, but final qualification is still required." };
    }
    if (!running && phase === "paused") {
      return { label: "Learning saved safely", detail: "The GPU is at stock and this compatible run can be resumed." };
    }
    if (!running && phase === "interrupted") {
      return { label: "Recovery required", detail: "Saved learning is intact; review recovery before continuing." };
    }
    if (!running) return { label: "Ready to forge", detail: "Progress appears here when the next run begins." };
    const stages = {
      preheat: ["Preparing the forge", "Normalizing the GPU before the first measurement."],
      power: ["Finding sustainable performance", "Locating the highest clock the hardware can hold cleanly."],
      descend: ["Screening a candidate", "Screening can reject a point early. Each eligible pair needs complete qualification before further refinement."],
      calibrate: ["Measuring profile power", "Recording real power at the exact Apply points."],
      synthesize: ["Forging the profiles", "Selecting performance, balance and efficiency from measured evidence."],
      "apply-qualify": ["Qualifying a candidate", "DX11, Vulkan, DX12 and Endurance must all pass at this exact pair before it can become a profile or allow refinement."],
      stopping: ["Stopping safely", "Saving learning and returning the GPU to stock."],
    };
    const [label, detail] = stages[phase] ?? ["Refining the forge", "The next estimate arrives with the current hardware task."];
    return { label, detail };
  }

  function nextStageLabel(phase) {
    const labels = {
      preheat: "Find sustainable performance",
      power: "Screen a candidate",
      descend: "Qualify an eligible candidate",
      calibrate: "Forge the three profile goals",
      synthesize: "Publish qualified profiles",
      "apply-qualify": "Compare evidence and remaining search budget",
      stopping: "Confirm the safe stock state",
    };
    return labels[phase] ?? "Prepare the next forge stage";
  }

  function terminalNextTask(phase, resumeAvailable) {
    if (phase === "finished") return "No further task";
    if (phase === "provisional") return "Run final qualification";
    if (phase === "interrupted") return "Review recovery before continuing";
    if (phase === "paused") {
      return resumeAvailable ? "Resume the saved Forge run" : "Resolve resume compatibility";
    }
    return nextStageLabel(phase);
  }

  $effect(() => {
    if (reportedElapsedMs !== observedElapsed || reportedRemainingMs !== observedRemaining) {
      observedElapsed = reportedElapsedMs;
      observedRemaining = reportedRemainingMs;
      elapsedBase = reportedElapsedMs;
      remainingBase = reportedRemainingMs;
      timingObservedAt = Date.now();
    }
  });

  $effect(() => {
    const task = powerSweep?.current_task ?? null;
    if (task !== observedTask || currentTaskReportedMs > taskElapsedBase) {
      observedTask = task;
      taskElapsedBase = currentTaskReportedMs;
      taskObservedAt = Date.now();
    }
  });

  $effect(() => {
    const runId = powerSweep?.run_id ?? null;
    const reportedOutcome = powerSweep?.last_outcome;
    if (runId !== observedRunId) {
      observedRunId = runId;
      latchedLastOutcome = reportedOutcome == null || String(reportedOutcome).trim() === ""
        ? null
        : reportedOutcome;
      return;
    }
    if (reportedOutcome != null && String(reportedOutcome).trim() !== "") {
      latchedLastOutcome = reportedOutcome;
    }
  });

  $effect(() => {
    const interval = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(interval);
  });
</script>

<section class="forge-progress" aria-labelledby="forge-progress-title">
  <p class="sr-only" aria-live="polite" aria-atomic="true">{liveAnnouncement}</p>

  <header class="progress-header">
    <div class="progress-heading">
      <span class="eyebrow">Forge progress</span>
      <h3 id="forge-progress-title"><Activity size={19} strokeWidth={1.8} />{title}</h3>
      <p>{phaseInfo.detail}</p>
    </div>
    <div class="progress-actions">
      <span class:warning={rebootRequired} class="safety-pill"><ShieldCheck size={14} strokeWidth={1.9} />{safetyLabel}</span>
      <span class:live={powerRunning && !rebootRequired} class:warning={isInterrupted || rebootRequired} class="run-pill">{runState}</span>
      {#if powerRunning && !rebootRequired}
        <button class="progress-button stop" type="button" onclick={onStopPower} disabled={isStopping}>
          <Square size={14} strokeWidth={1.9} />{isStopping ? "Stopping…" : "Stop safely"}
        </button>
      {:else if canResume}
        <button class="progress-button resume" type="button" onclick={onResumePower}>
          <Play size={14} strokeWidth={1.9} />Resume Forge
        </button>
      {:else if isInterrupted && !rebootRequired}
        <button class="progress-button resume" type="button" onclick={() => onRecoverContinue?.(forgeMode)}>
          <Play size={14} strokeWidth={1.9} />Review & continue
        </button>
      {/if}
    </div>
  </header>

  {#if rebootRequired}
    <p class="resume-note reboot-note" role="alert">
      The GPU driver stopped responding and the test was interrupted. Restart Windows once; the incident and recovery state are saved.
    </p>
  {/if}

  <ol class="phase-rail" aria-label="Forge stages">
    {#each FORGE_PHASES as forgePhase, index}
      <li
        class:complete={phaseStatus(index) === "complete"}
        class:active={phaseStatus(index) === "active"}
        aria-current={phaseStatus(index) === "active" ? "step" : undefined}
      >
        <span class="phase-marker" aria-hidden="true">{phaseStatus(index) === "complete" ? "✓" : index + 1}</span>
        <span>{forgePhase.label}</span>
        <span class="sr-only">{phaseStatus(index)}</span>
      </li>
    {/each}
  </ol>

  <div class="progress-overview">
    <div class="progress-copy">
      <span>{phaseInfo.label}</span>
      <strong>{Math.round(progressPercent)}%</strong>
    </div>
    <div
      class="progress-track"
      class:forging={powerRunning}
      role="progressbar"
      aria-label="Estimated Forge completion"
      aria-valuemin="0"
      aria-valuemax="100"
      aria-valuenow={Math.round(progressPercent)}
      aria-valuetext={`${Math.round(progressPercent)} percent estimated`}
    >
      <span style={`width: ${progressPercent}%`}></span>
    </div>
  </div>

  {#if hasRun}
    <div class="run-context" aria-label="Current Forge context">
      <article>
        <span>{currentPairHeading}</span>
        <strong>{currentPairLabel}</strong>
      </article>
      <article>
        <span>Measured progress</span>
        <strong>{stepLabel}</strong>
      </article>
    </div>

    {#if discoverySearch}
      <section class="search-budget" aria-label="Candidate search coverage">
        <div class="search-summary">
          <strong>{discoverySearch.attempts_used} / {discoverySearch.attempts_limit} candidate attempts</strong>
          <span>{qualifiedBands} / {searchBands.length} regions with a qualified candidate</span>
          <span>Run budget: {duration(discoverySearch.time_budget_ms)} · used {duration(discoverySearch.elapsed_ms)}</span>
        </div>
        <p>First qualify the highest sustainable clock across heavy loads. Then explore efficiency within 10% below the qualified top. Every candidate needs complete qualification; Resume preserves the budget.</p>
        <p>The requested clock is nominal. Tests allow up to +15 MHz, with the same voltage and power limits. A brief peak does not qualify a higher-clock profile.</p>
        {#if discoverySearch.stop_reason}<p class="search-stop">Search ended: {searchStopReason(discoverySearch.stop_reason)}</p>{/if}
        <ul class="search-bands">
          {#each searchBands as band}
            <li class:closed={band.status === "closed"}>
              <strong>{bandLabel(band.id)} <span>· {bandStatus(band.status)}</span></strong>
              {#if band.last_qualified_clock_mhz > 0 && band.last_qualified_voltage_mv > 0}
                <p>Qualified: {band.last_qualified_clock_mhz} MHz @ {band.last_qualified_voltage_mv} mV</p>
              {:else}
                <p>No qualified candidate yet.</p>
              {/if}
              {#if band.stop_reason}<p>{searchStopReason(band.stop_reason)}</p>{/if}
            </li>
          {/each}
        </ul>
        {#if searchBands.some((band) => band.status === "closed")}
          <p>Closing a region stops further exploration; it does not classify untested points as unstable.</p>
        {/if}
      </section>
    {/if}

    <div class="task-flow">
      <article class="task-card current">
        <span class="task-icon"><Timer size={20} strokeWidth={1.75} /></span>
        <div>
          <small>Now{powerRunning ? ` · ${duration(taskElapsedMs)}` : ""}</small>
          <strong>{currentTaskLabel}</strong>
          <p>{powerRunning && taskRemainingMs != null ? `Current task estimate: ${duration(taskRemainingMs)} remaining.` : phaseInfo.detail}</p>
        </div>
      </article>
      <article class={`task-card last ${lastDecision.tone}`}>
        <span class="task-icon text-icon">LAST</span>
        <div>
          <small>Last decision</small>
          <strong>{lastDecision.label}</strong>
          <p>{lastDecision.detail}</p>
        </div>
      </article>
      <article class="task-card next">
        <span class="task-icon text-icon">NEXT</span>
        <div>
          <small>{isFinished ? "Run complete" : powerRunning && taskRemainingMs != null ? `Starts in about ${duration(taskRemainingMs)}` : "Next planned task"}</small>
          <strong>{nextTaskLabel}</strong>
          <p>{isFinished ? "The GPU has returned to its verified final state." : nextTaskDurationMs == null ? "Duration updates from measured hardware evidence." : `Expected duration: ${duration(nextTaskDurationMs)}.`}</p>
        </div>
      </article>
    </div>
  {/if}

  <div class="run-timing">
    <article>
      <Clock3 size={17} strokeWidth={1.7} />
      <span>Elapsed<strong>{hasRun && elapsedMs != null ? duration(elapsedMs) : "—"}</strong></span>
    </article>
    <article>
      <Timer size={17} strokeWidth={1.7} />
      <span>Remaining estimate<strong>{remainingEstimate}</strong></span>
    </article>
    <article>
      <Activity size={17} strokeWidth={1.7} />
      <span>Conservative ceiling<strong>{remainingCeiling}</strong></span>
    </article>
    <article>
      <ArrowRight size={17} strokeWidth={1.7} />
      <span>Finish window<strong>{estimatedFinishWindow}</strong></span>
    </article>
  </div>

  {#if isPaused && !powerSweep?.resume_available}
    <p class="resume-note" role="status">
      Resume unavailable: {powerSweep?.resume_block_reason ?? "This checkpoint no longer matches the current program, GPU or driver."}
    </p>
  {/if}
</section>

<style>
  .forge-progress {
    --panel-radius: 11px;
    --inner-radius: 9px;
    display: grid;
    gap: 12px;
    padding: 18px;
    border: 0;
    border-radius: var(--panel-radius);
    background: var(--progress-surface, rgba(7, 10, 12, 0.58));
    box-shadow: inset 0 0 0 1px var(--progress-outline, rgba(126, 136, 143, 0.34));
    color: #e3e3df;
    font-family: inherit;
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    clip-path: inset(50%);
    white-space: nowrap;
  }

  .progress-header,
  .progress-actions,
  .progress-copy,
  .run-timing article {
    display: flex;
    align-items: center;
  }

  .progress-header { justify-content: space-between; gap: 18px; }
  .progress-heading { min-width: 0; }

  .eyebrow {
    display: block;
    margin-bottom: 5px;
    color: #889196;
    font-size: 0.75rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  h3 {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0;
    color: #e3e3df;
    font-size: 1.125rem;
    font-weight: 580;
    letter-spacing: -0.01em;
  }

  .progress-heading p {
    max-width: 700px;
    margin: 5px 0 0;
    color: #a2a9ac;
    font-size: 0.75rem;
    line-height: 1.5;
    text-wrap: pretty;
  }

  .progress-actions { justify-content: flex-end; gap: 8px; flex-wrap: wrap; }

  .safety-pill,
  .run-pill {
    display: inline-flex;
    min-height: 30px;
    align-items: center;
    gap: 6px;
    border-radius: 999px;
    padding: 0 10px;
    background: rgba(126, 184, 78, 0.1);
    box-shadow: inset 0 0 0 1px rgba(126, 184, 78, 0.34);
    color: #bce49a;
    font-size: 0.75rem;
    font-weight: 780;
    letter-spacing: 0.055em;
    text-transform: uppercase;
    white-space: nowrap;
  }

  .run-pill {
    background: rgba(255, 255, 255, 0.035);
    box-shadow: inset 0 0 0 1px var(--forge-line);
    color: #a5adb1;
  }

  .run-pill.live {
    background: rgba(214, 168, 93, 0.1);
    box-shadow: inset 0 0 0 1px rgba(214, 168, 93, 0.38);
    color: var(--forge-gold);
  }

  .run-pill.warning,
  .safety-pill.warning {
    background: rgba(191, 97, 106, 0.12);
    box-shadow: inset 0 0 0 1px rgba(191, 97, 106, 0.4);
    color: #f3b9bd;
  }

  .progress-button {
    display: inline-flex;
    min-height: 40px;
    align-items: center;
    justify-content: center;
    gap: 7px;
    border: 0;
    border-radius: 9px;
    padding: 0 13px;
    background: rgba(126, 184, 78, 0.13);
    box-shadow: inset 0 0 0 1px rgba(126, 184, 78, 0.4);
    color: #c6eba8;
    font: inherit;
    font-size: 0.75rem;
    font-weight: 720;
    cursor: pointer;
    transition: background-color 150ms ease, box-shadow 150ms ease, transform 100ms ease;
  }

  .progress-button.stop {
    background: rgba(191, 97, 106, 0.12);
    box-shadow: inset 0 0 0 1px rgba(191, 97, 106, 0.4);
    color: #f3b9bd;
  }

  .progress-button:hover:not(:disabled) {
    background-color: rgba(214, 168, 93, 0.16);
    box-shadow: inset 0 0 0 1px rgba(214, 168, 93, 0.48);
  }

  .progress-button:focus-visible { outline: 2px solid var(--forge-gold); outline-offset: 2px; }
  .progress-button:active:not(:disabled) { transform: scale(0.96); }
  .progress-button:disabled { cursor: wait; opacity: 0.58; }

  .phase-rail {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 0;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .phase-rail li {
    position: relative;
    display: grid;
    min-width: 0;
    justify-items: center;
    gap: 6px;
    color: #7f898e;
    font-size: 0.75rem;
    font-weight: 680;
    text-align: center;
  }

  .phase-rail li:not(:last-child)::after {
    position: absolute;
    z-index: 0;
    top: 14px;
    left: calc(50% + 17px);
    width: calc(100% - 34px);
    height: 1px;
    background: rgba(255, 255, 255, 0.1);
    content: "";
  }

  .phase-rail li.complete,
  .phase-rail li.active { color: #d8dbd8; }
  .phase-rail li.complete:not(:last-child)::after { background: rgba(126, 184, 78, 0.42); }

  .phase-marker {
    position: relative;
    z-index: 1;
    display: grid;
    width: 29px;
    height: 29px;
    place-items: center;
    border-radius: 50%;
    background: #111619;
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.13);
    color: #818b90;
    font-size: 0.75rem;
    font-variant-numeric: tabular-nums;
  }

  .complete .phase-marker {
    background: rgba(126, 184, 78, 0.12);
    box-shadow: inset 0 0 0 1px rgba(126, 184, 78, 0.4);
    color: #bce49a;
  }

  .active .phase-marker {
    background: rgba(214, 168, 93, 0.13);
    box-shadow: inset 0 0 0 1px rgba(214, 168, 93, 0.55), 0 0 14px rgba(214, 168, 93, 0.13);
    color: var(--forge-gold);
  }

  .progress-overview {
    padding: 12px 14px;
    border-radius: var(--inner-radius);
    background: rgba(0, 0, 0, 0.2);
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.055);
  }

  .progress-copy { justify-content: space-between; gap: 12px; color: #a2a9ac; font-size: 0.75rem; }
  .progress-copy strong { color: var(--forge-gold); font-size: 0.875rem; font-variant-numeric: tabular-nums; }

  .progress-track {
    height: 9px;
    margin-top: 9px;
    overflow: hidden;
    border-radius: 999px;
    background: rgba(0, 0, 0, 0.52);
    box-shadow: inset 0 0 0 1px rgba(214, 168, 93, 0.18);
  }

  .progress-track > span {
    position: relative;
    display: block;
    height: 100%;
    overflow: hidden;
    border-radius: inherit;
    background: linear-gradient(90deg, #9a6039, var(--forge-gold), #a6cf69);
    box-shadow: 0 0 16px rgba(214, 168, 93, 0.28);
    transition: width 240ms ease-out;
  }

  .progress-track.forging > span::after {
    position: absolute;
    inset: 0;
    content: "";
    background: linear-gradient(100deg, transparent 20%, rgba(255, 245, 215, 0.08) 38%, rgba(255, 250, 229, 0.5) 50%, rgba(255, 245, 215, 0.08) 62%, transparent 80%);
    transform: translateX(-140%);
    animation: forge-progress-sheen 1.9s linear infinite;
    will-change: transform;
  }

  @keyframes forge-progress-sheen { to { transform: translateX(140%); } }

  .run-context { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; }

  .run-context article {
    min-width: 0;
    padding: 10px 12px;
    border-radius: var(--inner-radius);
    background: rgba(255, 255, 255, 0.025);
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.055);
  }

  .run-context span { display: block; color: #8e979b; font-size: 0.75rem; }

  .run-context strong {
    display: block;
    margin-top: 3px;
    overflow: hidden;
    color: #d7d9d7;
    font-size: 0.85rem;
    font-variant-numeric: tabular-nums;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .search-budget { padding: 12px; border-radius: var(--inner-radius); background: rgba(0, 0, 0, 0.2); }
  .search-summary { display: flex; flex-wrap: wrap; gap: 8px 18px; font-size: 0.8rem; }
  .search-summary strong { color: var(--forge-gold); }
  .search-summary span { color: #a2a9ac; }
  .search-budget p { margin: 6px 0 0; color: #a2a9ac; font-size: 0.75rem; line-height: 1.5; }
  .search-budget .search-stop { color: var(--forge-gold); }
  .search-bands { display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 8px; margin: 10px 0 0; padding: 0; list-style: none; }
  .search-bands li { min-width: 0; padding: 10px; border-radius: var(--inner-radius); box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.1); }
  .search-bands strong { font-size: 0.8rem; font-weight: 670; }
  .search-bands strong span { color: #a2a9ac; font-weight: 500; }
  .search-bands li.closed { box-shadow: inset 0 0 0 1px rgba(214, 168, 93, 0.3); }

  .task-flow { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 8px; }

  .task-card {
    display: grid;
    min-width: 0;
    min-height: 112px;
    grid-template-columns: 40px minmax(0, 1fr);
    align-items: center;
    gap: 11px;
    padding: 12px;
    border-radius: var(--inner-radius);
    background: rgba(0, 0, 0, 0.2);
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.055);
  }

  .task-card.current { background: rgba(214, 168, 93, 0.07); box-shadow: inset 0 0 0 1px rgba(214, 168, 93, 0.24); }
  .task-card.last.success { box-shadow: inset 0 0 0 1px rgba(126, 184, 78, 0.3); }
  .task-card.last.caution { box-shadow: inset 0 0 0 1px rgba(214, 168, 93, 0.3); }
  .task-card.last.danger { box-shadow: inset 0 0 0 1px rgba(191, 97, 106, 0.36); }

  .task-icon {
    display: grid;
    width: 40px;
    height: 40px;
    place-items: center;
    border-radius: 9px;
    background: rgba(214, 168, 93, 0.11);
    box-shadow: inset 0 0 0 1px rgba(214, 168, 93, 0.24);
    color: var(--forge-gold);
  }

  .text-icon { color: #9aa3a7; font-size: 0.75rem; font-weight: 800; letter-spacing: 0.06em; }
  .task-card div { min-width: 0; }
  .task-card small { color: #929b9f; font-size: 0.75rem; font-variant-numeric: tabular-nums; }

  .task-card strong {
    display: block;
    margin-top: 4px;
    color: #d7d9d7;
    font-size: 0.875rem;
    font-weight: 670;
    text-wrap: balance;
  }

  .task-card.last.success strong { color: #bce49a; }
  .task-card.last.caution strong { color: var(--forge-gold); }
  .task-card.last.danger strong { color: #f3b9bd; }

  .task-card p {
    margin: 5px 0 0;
    color: #a0a7aa;
    font-size: 0.75rem;
    line-height: 1.4;
    text-wrap: pretty;
  }

  .run-timing { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 8px; }

  .run-timing article {
    min-width: 0;
    gap: 9px;
    padding: 10px 12px;
    border-radius: 9px;
    background: rgba(255, 255, 255, 0.025);
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.05);
    color: #929b9f;
  }

  .run-timing span {
    min-width: 0;
    color: #929b9f;
    font-size: 0.75rem;
    font-weight: 760;
    letter-spacing: 0.045em;
    text-transform: uppercase;
  }

  .run-timing strong {
    display: block;
    margin-top: 3px;
    overflow: hidden;
    color: #d7d9d7;
    font-size: 0.82rem;
    font-variant-numeric: tabular-nums;
    text-overflow: ellipsis;
    text-transform: none;
    white-space: nowrap;
  }

  .resume-note {
    margin: 0;
    border-radius: 9px;
    padding: 10px 12px;
    background: rgba(191, 97, 106, 0.09);
    box-shadow: inset 0 0 0 1px rgba(191, 97, 106, 0.28);
    color: #e8b4b8;
    font-size: 0.75rem;
    line-height: 1.45;
  }

  @media (max-width: 1040px) {
    .progress-header { align-items: flex-start; flex-direction: column; }
    .progress-actions { justify-content: flex-start; }
    .task-flow { grid-template-columns: 1fr; }
    .task-card { min-height: 92px; }
    .run-timing { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  }

  @media (max-width: 680px) {
    .forge-progress { padding: 14px; }
    .run-context,
    .run-timing { grid-template-columns: 1fr; }
    .progress-button { min-height: 44px; }
  }

  @media (prefers-reduced-motion: reduce) {
    .progress-track > span,
    .progress-button { transition: none; }
    .progress-track.forging > span::after { animation: none; }
  }
</style>
