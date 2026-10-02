# Nidavellir — Project Memory

## Current — TDR autonomy (2026-10-01 c, branch forge-tdr-autonomy-2026-10-01)

- **Goal:** an overnight Clean Run finishes on its own, including edge TDRs, with nobody logged in.
- **Implemented:** see decisions.md 2026-10-01 (c).
  - A per-run ceiling of 6 TDRs.
  - Opt-in auto-resume: 120 s countdown, acknowledges only this run's CandidateCrash, then Resume.
  - Experimental driver-only reset when the boot still holds the TDR. The service stops at stock,
    runs `pnputil /restart-device` and exits non-zero. SCM restarts it, and `gpu_driver_reset.json`
    lets the new process skip exactly that TDR.
  - A Resume with stock more than 60 MHz below the run's earlier stock stops and asks for a reboot.
- **Product installer and dev script:** SCM recovery (restart). The dev service is registered by
  `scripts/dev-service-boot.ps1 -Action Install`, run elevated by the user.
- **Not hardware-validated:** pnputil on driver 595.97, a Forge in session 0, the SCM restart cycle.
- **Build revision:** Resume needs the same build. Never update the service during a run.
- **Still open from search 11:** v38 runs no longer re-synthesize into applicable profiles.

## Previous — load steps + hot-bin anchor (2026-10-01, search 11, ExactApply39/Frontier34)

- **Why 1815@887 TDR'd once and 881…843 passed later:**
  - TDR is ~7% per idle→heavy step, and the matrix had only ~6 steps per pair.
  - Below ~870 mV the GPU self-relieved to the 1800 hot bin in the critical phase.
- **(a) Texture Hop r5:** a `load-step` phase gives ~66 steps per pair. Lanes are 138 s and
  screening 34.5 s; existing phases keep their durations.
- **(b) Hot-bin relief:** a pass whose critical phases ran >50% at the hot bin stays a pass but
  anchors the game margin +10 mV. Search (`QualifiedHotBin`) and synthesis (`hot_bin_relief`) share
  the rule. Never inconclusive.
- **Selection** treats a one-hot-bin p5 as the target (F2 only).
- **Open then:** TDR handling (implemented in 2026-10-01 c above). The PIN only blocks the dev BAT:
  the installed service starts before login.

## Previous — first search-10 run and selection fix (2026-10-01)

- **Run 1790850465550 finished.**
  - Top 1905 (1920@943 had a broken measurement): lowest 893.
  - 1815: lowest 843, silent error at 831.
  - 1725: lowest 793; TDR at 781 escalated to bugcheck 0x116, then Resume.
- **It published** Godforge 1905@943 and Brokkr's 1905@931, because a one-bin p5 dip at 931
  read as a clock trade.
- **Fix:** selection counts the held hot bin as the target (F2 only). Restore re-synthesis then
  gives 1905@931 / 1815@881 / 1725@831.
- **Watch:** Brokkr's 1815@881 is 6 mV above the Overwatch crash at 1815@875. Edges vary ~40 mV
  between runs.

## Previous — three distinct profiles (2026-09-30, search 10)

- **Run 1790761502529 (search 9, Full Reset → Clean) is paused after a TDR, not resumed.**
  - Top 1920: lowest pass 906, 900 silent error.
  - Compensated 1905@900 passed. Godforge 1905@937 was lost to a false p99 "anomaly": the
    adjacent-bin rule was applied across 37 mV.
  - The 1815 level took a real TDR at 887.
- **Physics learned:** PowerRender p99 ≈ 184.7 W + 0.32 W/mV·(V−900) + 0.16 W per clock bin
  (max residual 0.6 W). Power follows voltage, so a profile is only cheaper at a lower voltage.
- **Now:**
  - The p99 recheck compares only within 13 mV.
  - Lower levels must end two steps below their start, else they retry up to two clock bins lower.
    A level that never went below its start publishes nothing.
  - Floors: Brokkr's 92%, Deep Calm 87%. Deep Calm draws less than Brokkr's.
  - Attempt budget 30.
- **Expected on the test card:** Godforge 1905@937; Brokkr's ~1785–1800 @ 912–925; Deep Calm
  lower. The +36 mV margin keeps Brokkr's above the user's 1800@875.
- **Open:** margin by failure class (TDR vs silent error) not adopted. The 1815@887 test TDR is 12 mV
  above the field crash (1815@875).

## Previous — game-margin compensation (2026-09-29, search 9)

- **The search-8 run worked, but its profiles were too thin.** Godforge 1920@925 (two bins above
  the test edge) TDR'd in Overwatch after ~23 min. The user's references: 1800@875 is permanently
  stable, 1815@875 crashes Overwatch in under 30 min. The matrix approved 1830@856, so it is ~6
  bins less sensitive than games.
- **Now:**
  - Profiles need a pass ≥36 mV lower at the same clock.
  - Compensated top: the clock drops until the margin fits under the power-free voltage, with
    the margin pair verified.
  - The −5%/−10% levels follow the compensated top and use two-bin steps.
  - Balanced prefers lower power within 2%.
  - Restore re-synthesizes profiles, so a field failure drops only its own pair.
- **From the current run's proofs:** 1830@893 for all three profiles (≈ the user's 1800@875).
- **Expected next run:** Godforge ≈1890@937, Balanced ≈1800@881, Deep Calm ≈1710@~850.
- **Open:** the margin is calibrated on one GPU; test and field protection remain the backstop.

## Previous — staircase descent (2026-09-28, search 8, ExactApply38)

- Run 1790617016985: the DX11 fix worked (1920@937 qualified).
  - The old bands then wasted admissions at 937 and died at 931: a one-bin hot ClockDrop at all
    clocks, which is the GPU's thermal boost, not instability. No profile.
- **Now:**
  - The top descends voltage to its first failure. −5% and −10% levels start at the previous
    level's lowest pass and descend again.
  - One hot bin below target is held everywhere.
  - A TDR at a level edge pauses: reboot + acknowledge + Resume continues the same run.
  - The crash budget blocks only exploration, not Apply/publication.
- **Budget:** ~18–21 admissions expected (24 limit). Each TDR edge costs a reboot.
- **Open:** the NVML power-cap bit is uninformative in DX11. Nothing in this package ran on hardware.

## Previous — thermal/margin package (2026-09-27)

- Run1790537155912: top 1920@937 passed DX11 (98% power-limited, fix works), Vulkan, DX12; Endurance
  stopped the run: heavy near-limit phases drop one bin when hot (texture-rop 1905), SW thermal bit
  (fires at 70 °C) blocked the power-cap excuse. Light/medium phases held 1920 up to 79 °C.
- Now: heavy sustain holds one bin below (exposure/residency/discovery stay exact); F2 thermal =
  HW slowdown; Endurance needs the exact target within 3 °C of lane max; profiles need a proven
  lower voltage at the same clock; one margin probe below the top before economics.
- Acceptance: play the chosen profile with Safe Loop active (field TDR ladder: +1 bin, then stock;
  boot reconcile after wedge/BSOD). Open suggestions are listed in the 27/09 report to the user.
- Follow-up (27/09 b): margin probe last on a reserved, uncounted admission; DX11 light phase
  (ExactApply36, 6 phases); `clock_temp` cells in phase metrics; `.gitattributes`. User rejected
  cooling before discovery: tests must reflect continuous hot gaming.
- Follow-up (27/09 c): run 1790544997509 died at the top's DX11 light phase (6.5 s of 30 s at
  target) after 4/24 admissions. Light is now paced at 50% duty on 2/7 of the lane, and coverage
  merges back-to-back batches of one phase (ExactApply37, dx11-game-v6).
- An Inconclusive unqualified top now descends one clock bin at the same voltage, once per run
  (`inconclusive_descent_used`). A second Inconclusive still ends the run with no profile.
- The NVML power-cap bit is uninformative in DX11: it was set on >99% of samples, even at target.

## Current — representative-load power contract, search7 (2026-09-26 evening)

- Run1790448315552 (search5) spent 24/24 admissions in 11 min stepping 1920 from 1081 to 937 mV;
  every capped PowerRender sample settled at ~937–945 mV; 1920@937 passed at 199.18 W; the 30 s
  Texture screening then went `heavy_phase_telemetry_low` (13-sample opening/closing phases), so
  every short screening under Frontier31 was structurally inconclusive.
- User decision: representative load (PowerRender) defines the power envelope; heavier matrix
  loads may reach the limit. Worst-load DX11 ceiling (~824 mV@1905, ~848@1800) sat below the
  user's manual 1800@875 stability, so it could not produce realistic profiles.
- Frontier32/ExactApply35/search7: power-bound jumps to the lowest bin above the measured
  equilibrium voltage (2 admissions instead of 24 in that run); integrity error there descends
  one clock. Qualification samples below target with the NVML SW power-cap bit (no thermal bit)
  count as held (the 97%-of-limit condition was removed the same night: NVML power is a 1 s average
  on Ampere, see decisions.md); DX11 reports `power_limited_active_ms` apart from `target_active_ms`; short heavy phases
  are skipped; selection uses PowerRender p99 < limit. Codex's search6 (25 mV capped jump,
  midpoint probe, `power_bound_voltage_mv`) was replaced before any run used it.
- PowerRender is memory-bound (equal frames at 1740 and 1920), so its power depends on voltage
  only; do not use a V²·f model for it. See decisions.md and qualification-rules-2026-09-25.md.
- Run1790466472114 (first search7 run) stopped at DX11 `dx11_target_unexercised` because of the
  97% rule; fixed as above. Economic bands will likely reach this card's stability edge, where it
  historically TDRs, so the user chose B: after an attributed crash, publish profiles from pairs
  already proven (recomputed TDR cone excluded), with Apply latched until acknowledgement.
  Inconclusive now closes only its band (top-phase inconclusive still ends the search).
- Software only: no GPU workload, reset or live setting change. Physical acceptance: user closes
  Core, BAT rebuild, Full Reset → Clean. Expect top near 1920@937 if the matrix passes it.

## Previous — explicit nominal clock envelope (2026-09-26)

- Run1790446614161 used search4, stopped7/24 at1905@1043: repeated measured max1920
  (+15), voltage max1043, no integrity/TDR, clean reset. User explicitly chose containment OR
  bounded allowance; repeated refusal of normal-sized excursions is not a viable product.
- Discovery9/Frontier31/ExactApply34/search5 now qualify nominal..nominal+15MHz. NVML still
  requests nominal; no voltage/power relaxation. Heavy residence credits only target..target+15;
  brief upper visits never promote a higher nominal profile. Beyond+15 remains control failure.
- Persist absolute max_clock_mhz through dwell/report/observation. Current positive evidence
  requires peak present, positive and within ceiling. Older positives cannot Apply. DX11 phase
  diagnostics still show nominal excursions but containment uses explicit upper envelope.
- Evidence snapshot/tests: target/beta/clock-envelope-20260926/. No GPU workload or live
  setting change; physical acceptance remains pending. Read updated qualification rules.
-697 Rust tests passed/3 hardware ignored; release cargo check and UI build passed. Core still
  running PID14884, so executable not replaced; user closes Core then BAT rebuild/Full Reset/Clean.

## Previous — premature control stop in discovery (2026-09-26)

- Run f2-forge-1790445480585 used search3 and correctly started1920@1081;8 power-bound
  results descended voltage to1037. Attempt9 at1920@1031 ended operational_failure in6.6min,
  not budget exhaustion. Zero integrity/TDR, stock reset and BootFlag cleanup confirmed.
- Last reason control_failure_outside_requested_pair; saved voltage max1031 equals anchor,
  so code indicates a clock excursion, but old report omitted absolute peak (p95 only1755).
  Exact overshoot magnitude/timing and driver cause are unknown. Snapshot under
  target/beta/control-stop-20260926/. Do not assert a1935 peak or hardware fix.
- Search4 permits ONE neutral discovery control reapplication per entire run, same pair,
  only persisted current-contract evidence with clean recovery/no integrity fault; consumes
  new admission and survives Resume. Second excursion stops control_reapplication_failed.
  Qualification/apply/reset/driver faults still stop immediately; no tolerance relaxed.
- Refusal now records requested pair and absolute measured clock/voltage peaks. This is
  bounded recovery plus diagnosis, not a demonstrated fix to the driver's clock containment.
  Core PID8152 was left running; new code must be rebuilt/loaded through BAT after Core closes.
- Verified696 Rust tests passed/3 hardware ignored, release cargo check and UI production build
  passed. Release executable not replaced while Core is running; physical retry untested.

## Previous — organic top seed correction (2026-09-26)

- Fixed remaining seed bug: heavy stock p5 (1740 in user's run) capped the initial target.
  Seed now uses maximum real target from the sane post-preheat stock VF domain, with nearest
  plannable stock voltage. Curve top is a hypothesis, not measured sustainable performance.
- Search3: power-bound lowers voltage at same clock; at voltage floor without proof, lowers
  clock. First attributed integrity error before any qualified top lowers one physical clock;
  after power descent, also backs up one voltage bin. Lower ceiling prevents revisiting rejected
  upper target. Every new pair requires full proof; two errors/TDR/budgets still stop the search.
- Inconclusive never establishes a frontier. No GPU-specific seed, no hardware run or live
  setting change. Search2 Resume is incompatible; user must start a fresh run with new Core.
- 693 workspace tests passed, 3 hardware tests ignored. Evidence: target/beta/top-seed-20260926/.
  Release build could not replace target/release/nidavellir-service.exe (Windows access denied;
  Core PID22656 still running). No process interrupted. User must stop run/close Core and relaunch
  scripts/dev-launch.bat to build/load the correction; the live process still uses the old code.
  See docs/qualification-rules-2026-09-25.md for updated algorithm and finite-search limitations.

## Previous — top-first and worst-load qualification implemented (2026-09-25)

- Discovery8 / Frontier30 / ExactApply33 / search2. Only performance starts; complete pass raises
  clock first, power-bound descends voltage at same clock, economic seeds derive from qualified top.
- Maximum sampled clock (no +15 tolerance), voltage authority also on ClockDrop, all-lane power
  strictly below board cap; heavy DX11 phases each30s/95%, other heavy phases20samples/95%.
- Removed power-bound5% residency exception. Cancel preserves neutral status; telemetry stalls and
  current ClockDrop do not establish instability. Contaminated dwell faults skip nominal blacklist;
  independent Sentinel/BootFlag TDR protections still conservative and not automatically erased.
- See docs/qualification-rules-2026-09-25.md for full criteria, transitions and physical limitations.
-690 Rust tests passed/3 hardware ignored; final classifier recheck476 service tests passed.
 18 UI unit +6 qualification browser tests passed; Core release/UI production builds passed.
 Logs, hashes and backups: target/beta/worst-load-20260925/verification.json. No hardware run,
 service start, reset, acknowledgement or live tuning. Next: user-started Full Reset/Clean via
 scripts/dev-launch.bat, inspect heavy phase evidence/control refusals before accepting hardware.


NVIDIA GPU undervolting for Windows, with a Tauri v2 + Svelte UI and Rust Core Service.
The restricted beta contract is in product.md; CPU/RAM tuning is outside its scope.
Historical hardware observations do not substitute acceptance of the current build.

This file is the continuity index. See also: `AGENTS.md` (canonical product/agent
governance), `architecture.md`, `decisions.md`, `roadmap.md`, `handoff.md`,
`product.md`, and the methodology doc `docs/gpu-forge.md`.

## Discovery objective clarified (2026-09-16)
- Operator's manual 1800@875 is a provisional comparison for this RTX 3060 Ti, found by limited
  trial; it is not the optimum, an algorithm target/seed/fallback, or evidence for another GPU.
- Discover each GPU independently from Full Reset -> Clean Run, compare qualified candidates
  by measured performance/power for all three profile objectives, and retain higher-clock/higher-
  voltage alternatives when evidence and safety policy permit. Do not stop at the first stable pair.
- Validate the method against comparable or better trade-offs, not a forced exact 1800@875 result;
  explain discrepancies without tuning thresholds to pass the reference. Product contract:
  product.md, Profiles and evidence. This clarification changes documentation only; no GPU run.

## Previous (2026-09-25) — run exposes search-objective mismatch; worst-load contract confirmed
- User stopped run1790356252776; reaffirmed TOP qualified clock under board power limit FIRST,
  then profile exploration within10% below that top. Existing3 bands around stock do not satisfy it.
-74.36min,5 attempts,25 clean observations; qualified1740@937/1665@893/1575@850; no clock>1740
  tested despite observed boost1920 (boost is not stability proof). No final profiles published.
-1740@931 DX11 only25.946s/30s target exposure, no integrity/upper-clock failure; Inconclusive
  closed performance band before clock ascent.1665@887 manual cancel was also misrouted to
  Inconclusive and closed balanced band.8h/24 budget was not the limiting factor.
- Read docs/clean-run-results-2026-09-25.md; preserved snapshot/hashes under
  target/beta/clean-run-review-20260925/. No code change, service start, Resume or GPU load.
- User explicitly chose sustained target BELOW board power limit in ALL loads including heaviest
  stress, rejecting representative-load-only eligibility.1740@937 passed old contract but initial
 100%-duty DX11 phase max active1680; reduced-duty exposure cannot prove the new objective.
- User clarified: downclock from low demand/idle is acceptable; exceeding requested upper clock
  even by15MHz invalidates pair attribution. First establish containment and actual clock/voltage
  exposure, then learn/qualify. Distinguish low demand from inability under heavy active workload.
  Over-ceiling evidence must not approve or automatically condemn the nominal lower-clock pair.
- NEXT: top-first clock/voltage search, worst-load sustained-clock/power qualification and local
  refusal transitions; preserve test integrity/recovery; fix cancel semantics. Then derive economic
  domain[90%,100%] of qualified top. Do not enlarge budget/relax exposure/force manual1800@875.
  Worst-load top may be lower than gaming clocks; observed boost1920 is not sustained proof.

## Previous (2026-09-24) — qualified discovery implemented; manual hardware run next
- User accepted longer tests after September18 reboot. New pure qualified_search.rs replaces
  exhaustive frontier/vertical repair:3 stock-derived bands,24 durable admissions/8h Standard,
  Long time scaled by matrix duration. Full same-pair ordered matrix before refinement/publication.
- Single attempt per short phase/matrix lane; one confirmed power-bound preparation per band.
  Integrity error closes its band; two end search. TDR/operational failure ends run. Manual pause
  keeps counters; TDR cannot Resume. Economic bands follow90% qualified performance one proven
  clock bin at a time without reopening closed bands. No manual1800@875 seed or untested margin.
- Secondary stress uses stock TextureRop reference on same adapter/backend; peer failures stop
  promptly, strongest verdict preserved, both workers joined before cleanup. Frontier29/Apply32.
- Fixed simultaneous Stop masking physical errors, known pair exclusion aborting unrelated bands,
  mixed-clock summary attribution, budget stop reporting, atomic admission readback and qualification
  offset reference. Old exclusive algorithms/tests removed; current safety regressions retained.
-692 workspace tests passed/3 hardware tests ignored;18 UI unit +33 browser passed; UI production
  and Core release builds passed. No Rust warnings. Verification, logs/source hashes/exe SHA256:
  target/beta/qualified-discovery-20260924/verification.json. Details:
  docs/undervolt-discovery-proposal-2026-09-18.md; IPC:docs/contracts/ui-backend.md.
- NO Core start, GPU load, reset or incident acknowledgement. User next closes old UI/Core if open,
  opens scripts/dev-launch.bat (rebuilds release), Full Reset -> Clean Run, exports report.
  No installer update needed. Hardware/game stability and end-to-end product acceptance remain open.
- Preserve previous reboot evidence. Full Reset forgets all GPU learning; Soft keeps negatives.
  The8h run budget is independent from Codex5h quota. Continue from new run evidence, not more
  speculative stress variants. No automatic campaign or tuning point is authorized by this checkpoint.

## Previous (2026-09-18) — stronger discovery proposal prepared, not implemented
- User accepts longer tests to reduce disruptive TDRs. Audit/design:
  docs/undervolt-discovery-proposal-2026-09-18.md. Short Texture and Endurance have
  different load distributions; the875 pass does not certify the fatal868 candidate.
- Concrete detector gaps: secondary canary uses candidate self-reference (no stock
  golden); its failure is merged only after primary completion, without prompt stop.
  Neither is established as the cause of the latest bugcheck.
- Proposed: fix those gaps, require the existing complete ordered Apply matrix before
  refinement, explore fewer profile-relevant pairs with persistent finite budgets and
  per-band integrity-error stops.12 attempts/4h Standard are pilot limits pending reach
  replay; allow at most one explicitly power-bound preparation step per stock-derived band.
- Keep censored regions separate from measured failures; no manual1800@875 seed,
  no cross-pair proof, no guarantee of zero TDR/global optimum. Full Reset still forgets all.
- Documentation only; no algorithm change, build, service start, reset or GPU workload.
  NEXT implementation: stock oracle + concurrent first-failure propagation, then bounded
  candidate selection/full qualification and focused offline acceptance before a manual run.

## Previous (2026-09-18 afternoon) — fresh Clean Run hit a real driver bugcheck
- User reported reboot; read-only inspection confirms run f2-forge-1789720117824 (05:28 local)
  armed1890@868 at05:47:13 during Texture Frontier qualification. nvlddmkm153 starts05:47:34;
  Sentinel persisted rigid CandidateCrash and requested cooperative Stop at05:47:36.
- Windows boot05:49:04; WER1001 bugcheck0x116/VIDEO_TDR_FAILURE; WER1019 names nvlddmkm.sys.
  Candidate instability is supported, not exclusive root-cause proof. Dump access denied;
  no stack diagnosis. BootFlag remains armed; stock cleanup for the fatal dwell is unconfirmed.
-38 complete observations:30 validated,6 power-bound drops,2 SilentError (1920/1905@893).
  All38 completed cleanups confirmed.1890@875 passed short Discovery/Texture only; no final
  Apply matrix or profiles. The868 dwell is absent, represented by incident/ledger/BootFlag.
- Core process absent at inspection; checkpoint running=true is stale interruption state.
  No Core start, acknowledgement, reset, GPU load or changes to live learning in this turn.
- Report:docs/clean-run-results-2026-09-18.md. Snapshot/events/XML/hash evidence:
  target/beta/restart-investigation-20260918-151401/. Earlier05:01 bugcheck0xDE predates this run.
- NEXT: examine cancellation and stock recovery after the first TDR; do not claim Stop ensured
  recovery or that all driver bugchecks can be prevented. Preserve evidence before new reset.

## Previous (2026-09-18) — four post-run profile-search corrections implemented
- Full Reset's forget-all behavior was already delivered; it remains unchanged. Added durable
  per-clock search outcomes/policy-censored floors outside the40-line log, including exports.
- Removed common1%/198W F2 publication veto. Board limit remains unchanged; stress reaching it
  cannot waive target exposure, clock containment, thermal/integrity, all4 lanes or cleanup.
- Added comparison_power_p99_w from confirmed PowerRender at each exact Apply anchor. Rank
  all candidates with that same workload; retain worst Apply p99/peak separately. Clock/W is
  explicitly a proxy, not game FPS. v31 physical qualification requirements remain unchanged.
- Economic search can extend once down to90% of final Godforge sustained p5; only unfinished
  clocks are visited, budget persists across Resume. A further gap stays explicit rather than
  renewing the budget. profile_search_complete is independent of point qualification.
- UI groups identical Apply pairs, displays distinct-setting count/coverage/censoring and both
  comparison/stress power. Terminal note and diagnostic report state the limitations too.
- Validation:725 workspace tests passed (3 hardware tests ignored); final208 sweep tests passed;
  18 UI unit tests, production UI build and28 browser journeys passed. Release-mode cargo check
  passed. Clippy completed with
  existing warnings. Offline replay:9 old clean DX11 lanes would continue,3 unexercised stay
  refused; missing lanes are NOT approved by replay. Hardware acceptance is still pending.
- Read-only IPC: Core7696, started05:02:59 local, idle/no run. Left it running; no GPU workload,
  reset, authorization or history mutation. Running Core remains the old build. Close UI/Core
  normally, reopen scripts/dev-launch.bat (rebuilds release), then user Full Reset -> Clean Run.
- Evidence/before copies/scripts/logs/replay:target/beta/profile-policy-20260918/.
  Contracts first section, decisions and docs/profile-selection-analysis-2026-09-17.md updated.
- NEXT: inspect that manual clean run's distinct trade-offs, coverage and exposure outcomes;
  do not claim an optimum or insert1800@875 as a target. Actual release executable not replaced
  while Core remained open; launcher compiles the sources on the next start.

## Previous (2026-09-17 evening) — normal launcher and responsive reset confirmation
- User requested no terminal authorization and at most one UI confirmation for Soft/Full Reset.
  BAT now starts ordinary console mode, opens the UI and exits; no S/N or authorization IPC.
  The explicit development exception remains a separate command-based investigation workflow.
- Reset modal closes on confirmation; progress appears immediately, controls stay disabled
  until Core cleanup and mandatory readiness refresh finish. Optional Sentinel refresh no longer
  delays completion; ordinary status polling pauses during reset. No reset semantics changed.
- Production UI build and10 reset browser cases passed, including gated slow cleanup and hung
  optional Sentinel query. Evidence:target/beta/reset-ui-flow-20260917/.
- Existing Core17960 remains untouched in the older session. Operator must close UI/Core and
  reopen the same BAT to load ordinary console mode. No hardware reset, Start or GPU load.
- docs/development-validation.md and docs/contracts/ui-backend.md describe the updated flow.

## Previous (2026-09-17 evening) — console close corrected; post-run native exit still unproven
- Operator confirmed recurrent delay AFTER `shutdown complete` and NVAPI release. Core17032
  appeared in initial CIM inventory, then disappeared before thread/stack capture. Clean marker
  23:02:40 UTC; last heartbeat23:02:38 UTC. No forced termination or GPU operation by Codex.
- Development launcher now builds before elevation and launches the Core executable directly
  with RunAs; removed cargo/elevated PowerShell `-NoExit` parents retaining the console.
- Console callback no longer writes logs outside the cleanup deadline. X/logoff/shutdown use
  4s grace (Windows normally grants5s); Ctrl+C/Break keep30s. Failed cleanup preserves recovery.
- 8 shutdown tests passed, including subprocess callback with blocked console logging;9 mocked
  launcher cases passed. Release/sidecar rebuilt and hash matched, also delivering Full/Soft Reset.
- These fixes do not establish the cause of native termination delay after a complete run.
  NEXT: verify operator's next normal post-run close; if still stuck, capture live thread stacks
  before it exits. Do not claim software subprocess tests qualify driver/kernel teardown.
- Evidence/launcher fixture/logs:target/beta/console-close-20260917/;
  report updated:docs/shutdown-investigation-2026-09-17.md. No service start or GPU load.

## Previous (2026-09-17) — Full/Soft Reset implemented; release replacement was pending Core exit
- Explicit user policy supersedes all older negative-retention statements for Full Reset.
  Full now deletes active positive/negative GPU learning, blacklist/incidents/crash history,
  condemnation ledger/cones, applied descriptor/checkpoint and generated learning archives.
  Soft uses the prior negative-preserving reset and also clears last_validated and archives.
- Both wait for workers and acquire bounded Sentinel gates, confirm stock and clear owned
  BootFlag before erasure. Current-boot reboot requirements and developer authorization remain.
  Pending deletion marker blocks tuning/reapply after partial failure until Full retry finishes.
  In-memory legacy results are reset too; old Core responses cannot falsely claim forget-all.
- UI has distinct confirmations/buttons; BAT authorization text now describes forget-all.
  Clean by itself remains positive-only. Operational watchdog cursors, audit and exported
  diagnostic reports are not learning inputs and remain; no import can resurrect those files.
- 722 workspace tests passed (3 hardware tests ignored),17 UI unit tests, Clippy existing warnings;
  production UI build and25 browser journeys passed, including old-Core compatibility.
- Release build hit access denied: OLD Core17032 from15:06 remains open after its finished run.
  User asked asynchronously to close it; do not kill or reset it. Rebuild release and sync sidecar
  once gone. No real Full/Soft Reset or GPU load executed; actual user data remains unchanged.
- Evidence/before sources/patches/logs:target/beta/reset-modes-20260917/.
  Contract:docs/contracts/ui-backend.md first section; decisions.md latest; BAT workflow remains
  Full Reset -> S authorization -> Clean Run. Other profile-search corrections remain pending.

## Previous (2026-09-17) — completed run analyzed; profile discovery quality remains inadequate
- Run f2-forge-1789668446810 (15:07–17:15 local) finished,72 observations, all three profiles
  identical1710@868,p99=196.668W. This pair passed all4 Apply lanes. No physical failures/new
  blacklist/failed cleanup in observations; max77C.13 DX11 lanes consumed91min.
-12 higher pairs rejected by common198W publication ceiling;9 passed DX11 but exceeded power,
  3 also lacked target exposure. All13 report no active upper-clock excursion. Residency alone
  is not the main blocker now. Same1710@868 p99:PowerRender171.867W vsDX11196.668W.
- Clean starts fresh positive evidence but preserves3 old v29 CandidateCrash cones. Projection
  limits lower-clock descent;1800 reached900 boundary/906 Apply and never tested875.1920@943
  source is restart reconciliation, not a timestamped new Windows TDR in its note; review attribution
  and inferred scope, do not erase confirmed negatives. Other incidents also constrain1800.
- Discovery floor1710=last bin>=90% of initialCmax1890. After Apply leaves1710 as maximum,
  no lower efficiency candidates remain. Selector duplicates the sole point for all profiles;
  point qualification is real but sufficient coverage/three objective quality is not established.
- Persisted40-line log loses initial cone/clock-stop reasons; live tail240. Report proposes
  persistent measured-vs-censored boundary reasons, energy/objective comparison redesign,
  bounded completion of missing economic domain, and honest single-profile presentation.
- Analysis only: no algorithm/history/service mutation or workload. Snapshot/audit/hash/summary:
  target/beta/profile-selection-20260917/. Report:docs/profile-selection-analysis-2026-09-17.md.
  NEXT: implement those corrections in order; no blind full rerun or threshold relaxation.
  Post-full-run shutdown timing was not measured in this turn; no service process at collection.

## Previous (2026-09-17) — delayed Core exit investigated; NVAPI lifecycle balanced
- Old Core10272 wrote clean shutdown at14:15:10 local, but Windows still enumerated one thread
  consuming89% CPU in kernel mode at14:18:17. User reported spontaneous exit; absent by14:29:53.
  No stack was captured, so exact kernel cause and post-run UI lag remain unproven.
- Fixed11 repeated NVAPI initialization sites: all production wrapper reads/writes share one
  successful initialization; terminal shutdown unloads once after GPU workers/readers quiesce.
  Failed unload cannot commit clean shutdown. Closed runtime cannot reopen in the same process.
- 719 workspace tests passed,3 hardware tests ignored. Explicit read-only subprocess separately
  passed128 voltage reads + unload + process exit in123.0905ms. Clippy passed with existing warnings.
  No GPU workload/control writes, Reset, service start or forced termination performed.
- Release and UI sidecar rebuilt/hash-matched0797CA2592C3864F8D7E471F2887E8B9746E25D6D99B8CF7345AC709F2410180;
  includes the preceding clock-control corrections. No Core process running at final check.
- Evidence:target/beta/shutdown-lag-20260917/; report:docs/shutdown-investigation-2026-09-17.md.
  NEXT: user's bounded manual BAT/Clean Run, inspect diagnostics and confirm prompt post-run exit.
  Short read-only exit is not acceptance of shutdown after a complete tuning workload.

## Previous (2026-09-17) — clock-control routing and diagnostics corrected; hardware cause pending
- Added optional active_target.diagnostics: per-phase active maximum/count, upper count and bounded
  sampled duration, first-event timestamp/voltage/temperature and optional post-read anchor curve.
  Curve reads are capped at5 raw excursions/lane; they are context, not atomic containment proof.
- Complete reset-clean upper excursion now yields ExactApplyRejected: ClockControlExceeded.
  Exclude only that pair, no voltage repair/blacklist; at most one alternate control-refused pair
  before terminal explanatory note. Same-run/GPU/v31 observations restore budget/exclusions on Resume.
- Concurrent power refusal is saved/logged without hiding the control budget. Physical errors,
  failed cleanup and mismatched telemetry retain stop priority. No threshold or workload change;
  198W publication ceiling on this200W board remains. NVML request success is not physical proof.
- 717 workspace tests passed,2 existing hardware tests ignored; core/service all-target Clippy passed
  with existing warnings. No GPU workload, control writes, reset, authorization or service restart.
- Evidence/before sources/patches/logs/manifest:target/beta/clock-control-20260917/.
  Service10272 remains on the OLD release. Release/sidecar not replaced while it is open; the user's
  BAT rebuilds release after the old service is closed. Source fixes are compiled/tested offline.
- NEXT: bounded manual Clean Run with new diagnostics, inspect active excursion phase/curve before
  changing clock controls. Actual overshoot cause and full profile acceptance remain unproven.
  Report:docs/clean-run-results-2026-09-17.md (implementation follow-up).

## Previous (2026-09-17) — first DX11 v4 Clean Run analyzed; clock containment unresolved
- Run f2-forge-1789633001432 ended incomplete at05:55 local after38m34s:54 observations,
  Discovery26 validated/4 power-bound, Frontier22 validated, Apply2 inconclusive; no profiles.
- 1890@943:19.062s exact active target/73.239s observed (26.03%), p99=199.795W; power exclusion
  correctly selected1875@937 next, without the old constant-voltage fast-drop cascade.
- 1875@937:30.007s/76.524s (39.21%), five phases; numeric exposure requirements met marginally,
  but sampled active clock exceeded target (aggregate max1890), causing global ExactApplyInconclusive.
  p99=200.006W also exceeds198W publication ceiling; removing clock veto alone cannot qualify it.
- All54 cleanup records clean, max73C, no physical failures/blacklist recorded; BootFlag absent.
  Service10272 remained open at collection; run not running. No GPU action or code change this turn.
- Existing writer already requests NVML max=target. Missing active overshoot count/time/phase and
  curve correlation prevent causal attribution; do not blame temperature or claim a missing lock.
- NEXT: instrument/diagnose effective ceiling, give out-of-target control a specific bounded
  routing outcome without publishing/blacklisting it, retain simultaneous power/coverage causes,
  then bounded end-to-end acceptance. No blind overnight rerun or threshold relaxation.
- Full analysis: docs/clean-run-results-2026-09-17.md; snapshots/hashes:target/beta/clean-run-20260917/.

## Previous (2026-09-17) — active DX11 residency implemented, tuned acceptance pending
- User authorized correction after the bounded comparison. Exact Apply is now v31 / DX11 v4:
  same total duration, five equal phases continuous/75/50/25/continuous. Middle phases alternate
  checked 100 ms windows with idle; shaders, checksums, GPU writer and other API lanes unchanged.
- New dx11_residency.rs records fenced GPU-work intervals. Only clock/voltage queries wholly
  inside middle-phase work contribute, with sample support capped at ±15 ms, neighboring sample
  midpoints and work boundaries. No idle/CPU checksum credit or interpolation across sensor gaps.
- Gate requires >=60 s observed active coverage, >=30 s at exact target and >=35% active-target
  fraction, all five phases completed, sane voltage <= anchor, no sampled above-target work clock.
  These are initial operational thresholds pending hardware acceptance, not universal stability proof.
- Optional persisted coverage.active_target carries the proof; core publication rechecks it for
  DX11 v31, and historical positives are invalidated without erasing negative safety evidence.
- A complete target-unexercised lane excludes only the pair from this run and resynthesizes:
  no identical retries, voltage raise or blacklist. Power refusal still has its separate routing.
  Worst continuous-phase/whole-lane p99 prevents idle dilution of the unchanged198 W example cap.
  Old numeric-off-cap structural repair now also requires low limiter fraction, including Resume.
- Workspace suite: 714 passed, two existing hardware tests ignored. Separately ran the existing
  DX11 stock integration test: passed in3.08 s, including submit/fence markers, wrong-checksum drain
  and cancellation. No tuned GPU workload, service authorization, Reset or Clean Run this turn.
- Evidence/before sources/tests/clippy/build: target/beta/active-residency-20260916/.
  Release and sidecar rebuilt and hash-matched; final source/binary hashes in verification.json.
  NEXT: bounded manual Full Reset -> Clean Run acceptance
  of v31 active exposure, inspect coverage.active_target before any overnight run. Do not equate
  the earlier idle-inclusive comparison or this stock marker test with tuned qualification.

## Previous (2026-09-16 22:15 local) — one bounded load comparison completed
- User authorized the proposed same-point variable-load diagnosis. Added CLI `diagnose-f2-loads`
  alongside the existing point diagnostic; uses its single 1830@943 F2 transaction, stock goldens,
  120 s stock preheat, five 30 s phases (continuous / 75 / 50 / 25 / continuous requested duty).
  Reduced-duty phases alternate checked 100 ms work windows with bounded idle, not claimed GPU util.
  Existing DX11 production workload, pass thresholds and profile publication remain unchanged.
- Executed once, process 4160, audit 4160-1789607375060970900, 2026-09-17 01:09–01:14 UTC.
  Phase residence exactly at 1830: 1.01 / 42.23 / 63.39 / 78.42 / 0.68%; average power:
  195.56 / 160.05 / 124.75 / 88.53 / 196.52 W. All render/compute checks matched, max 73 C,
  no sampled clock above 1830. Continuous-load power-cap flags ~99–100%.
- Readback initially confirms 1740 base +90 offset =1830 at943. At the first phase transition,
  base/live briefly moved to1725/1815 while offset remained +90, then returned1740/1830.
  This is observed curve movement, not a commanded voltage increase; temperature causality not isolated.
- LIMIT: phase samples include idle windows and clock/voltage reads are sequential, not atomic.
  Duty phases demonstrate load-sensitive residence, NOT loaded target qualification. Future exposure
  accounting must tag active/drain/idle intervals and reject idle credit; do not simply lower35%.
  Voltage-lock readback after Apply returned ArgumentExceedMaxSize (journal retained); actual voltage
  stayed <=943. After reset readback Ok([]), all touched offsets zero, BootFlag cleared.
- Result intentionally Inconclusive/nonpublishable (diagnostic scope), not a failed phase: all five
  phases finished without cancellation. No search/profile learning. SafeLoop/condemnation/observations
  hashes unchanged; reviewed paused checkpoint temporarily archived for authorization then restored
  byte-for-byte. No service remains running. Evidence: target/beta/load-comparison-20260916/.
- 506 service tests passed, release and sidecar rebuilt/hash matched C5E5293E...EE9B0F. The earlier
  power-routing fix is now built too. Next: use these data to design separate heavy-load checks and
  bounded active target exposure, plus resolve numeric "off-cap" classification versus limiter flags.

## Previous (2026-09-16 evening) — loaded collection verified; power fallback corrected
- Operator manually ran f2-forge-1789586811426, then stopped it. Persisted phase paused, stock reset
  confirmed; 62 observations, all reset/BootFlag proofs clean. No service restart or GPU action by Codex.
- All 36 Discovery observations have 8 voltage reads; no Discovery telemetry inconclusives. One
  Frontier SilentError at 1830@912 was retained as real negative evidence. Maximum recorded 75 C.
- Seven full DX11 dwells at 1890..1800@943 consumed 49 min; all target_residency_low and p99
  199.71–199.85 W >198 W. Godforge's priority fast-drop carried the rejected voltage down the clock
  ladder, ahead of independently discovered lower-voltage candidates (including 1800@900/906).
- Fixed f2_godforge_fast_drop_candidate to decline ExactApplyPowerCeilingExceeded; normal synthesis
  resumes with existing candidates. Physical-failure fallback remains; no thresholds or safety history
  relaxed. This removes the observed priority cascade, not the unresolved DX11 residency problem.
- 505 service tests passed, including a regression for all seven exhausted targets. Evidence and
  pre-edit source: target/beta/power-routing-20260916/. Release rebuild blocked by Windows access
  denied removing the executable while service PID 20400 is open; sidecar not replaced. Close service
  normally before next BAT build. Source/test verification succeeded; no updated release claimed.
- Do not request another blind overnight run: next investigation is remaining DX11 residency / candidate
  selection, using preserved data first. Operator still owns BAT starts; current process does not hot reload.
- Residency review: DX11 requires >=target in >=35% of samples (zero lower tolerance). Latest seven
  dwells report SW power-cap fractions 99.85–99.94%; 1800@943 averages ~1640 MHz, residency 0.087%.
  Earlier 1710@868 p99 ~194.7–194.9 W ALSO reports cap fractions >99.8%; the historical "off-cap"
  label came from numeric power classification, not proof that the limiter was inactive. Resolve this
  discrepancy before inferring a voltage repair from structural clock drop. Proposed, NOT implemented:
  separate heavy-load integrity/performance from target exposure with bounded variable-load phases;
  retain upper-clock/voltage checks and sufficient timed exposure, no blanket lowering of residency.

## Previous (2026-09-16) — telemetry correction implemented; loaded acceptance pending
- Reused persistent NvmlSampler in the dwell; replaced the every-16-loops voltage schedule with
  elapsed-time 500 ms attempts. Discovery keeps its 6 s ramp discard; qualification retains active
  opening/transition voltage. No fabricated catch-up samples, no reduction of the minimum 3 reads.
- Dwell/report/observation now carry inconclusive_reason. Non-power Discovery refusals use
  DiscoveryInconclusive; true missing/inconsistent power and ambiguous cap retain their power label.
  Reasons appear in service logs and human/JSON exports. Legacy records deserialize unchanged.
- Qualification phase metrics now retain actual sample_count and clock_max, exposing rare upper
  bins hidden by p95. Cross-phase NVML reads are excluded from per-phase coverage. These are
  diagnostics, not a new strict hardware ceiling or a relaxation of qualification.
- Offline suite: 707 passed / 0 failed / 2 existing hardware tests ignored. Read-only 10 s host probe:
  8 post-ramp voltage samples, 39–40 clock/power samples per 1.2 s, persistent-query median 39 us.
  No tuning/workload/service start/reset/authorization was performed. Evidence and pre-edit copies:
  target/beta/sampler-fix-20260916/. Release/sidecar rebuilt and hash-matched; final source/build
  manifest is verification.json there. Clippy passes with existing warnings; whitespace check clean.
- NEXT: verify sampling under real load before another overnight campaign; the off-cap DX11 target
  residency failure and strict ceiling semantics are not closed by this change. Keep 1800@875 only
  as this GPU's provisional comparison, never a hardcoded discovery target. Operator owns BAT start.
- Old service PID 19348 remains present and a new bounded Ping still timed out. A newly built file
  does not update that process. Its persisted run is incomplete/not running, no BootFlag/applied file.

## Previous (2026-09-16 afternoon) — overnight Clean Run inspected, incomplete
- Operator's run f2-forge-1789548115932 was clean_run/Standard, 05:41:55–08:49:45 local
  (187.825 min), 103 observations, no qualified profile. All observations belong to this run;
  pre-run positives archived. Full findings: docs/clean-run-results-2026-09-16.md.
- Counts: Discovery 32 Validated / 5 PowerBoundClockDrop / 13 PowerTelemetryInconclusive;
  Frontier Texture 25 Validated / 7 boost_edge_telemetry_low; exact Apply DX11 21 Inconclusive
  across 18 pairs, all full 420 s (147.025 min), all target_residency_low. No later final lane ran.
- Early power routing worked for 17 pairs with DX11 p99 >198 W. Still no profile: 1710@868
  repeated 3 off-cap dwells with p99 194.7–194.9 W, p95 1695 MHz, target residency 0.255–0.976%.
- Concrete measurement defect: all 13 Discovery inconclusives have only 1–2 voltage readings,
  below enforce_voltage_authority's minimum 3, despite usable numeric p99 and in-tolerance clock.
  Voltage is sampled every 16 sampler iterations; 10 s measurements yielded only 24–38 main
  samples. Generic mapping calls all Discovery Inconclusive PowerTelemetryInconclusive, masking
  this reason. Final 1710@875 had clock/p5/p95=1710, p99 170.47 W, one voltage sample; repair failed.
- NEXT: repair short-dwell voltage sampling and precise reason reporting without lowering proof
  requirements; inspect the 7 short BoostEdge coverage gaps. Off-cap DX11 residency remains
  unisolated (no aligned per-dwell curve readback in this run). No source fix was made during review.
- 75 C max, no integrity/unstable/device-loss/TDR or thermal throttle. All 103 reset/BootFlag proofs
  clean, Safe Loop idle/no pending incident, no applied profile, ledger unchanged. Driver 616.92
  matches both Sept15 point diagnostics. Audit finished incomplete; no new authorization/Start.
- Service PID 19348 still existed at inspection, but two bounded IPC connections timed out before
  connecting; UI absent. Used final files/audit instead; service not stopped/restarted. Investigate
  that unresponsiveness separately before the operator's next BAT launch. GPU snapshot 44 C / 9%.
- Evidence snapshot: target/beta/clean-run-20260916/. Active and run-archive JSONL hashes match
  C3053817...84D848; summarize.cjs regenerates summary.json. Preserve before any Reset.

## Previous (2026-09-16 08:35Z) — operator owns BAT launch, reset and Clean Run
- Operator explicitly wants to open the program through his Desktop dev.bat and exercise the
  complete manual first-use path. Do not prepare/start a service or authorize on his behalf
  for this attempt. Every new validation remains Full Reset → Clean Run with no positive reuse.
- Found launcher sequencing bug: Desktop dev.bat authorized before opening UI, so the user's
  subsequent Full Reset consumed permission before Start. Added scripts/dev-launch.bat as the
  canonical launcher and changed Desktop dev.bat to call it; original hash-verified copy is in
  target/beta/overnight-run-20260916/desktop-dev-before.bat. No product/GPU algorithm change.
- New order: BAT starts release development service → opens UI in its own terminal → operator
  exports/resets in UI → returns to BAT and explicitly answers S → selects Clean Run and clicks
  Forge GPU in UI. BAT never sends Reset/Start, auto-renews permission or retries a failed grant.
  The extra authorization is specific to this previously crashed development GPU, not ordinary
  first use. docs/development-validation.md documents this primary path.
- Stopped the Codex-prepared service 15504 cooperatively at 08:34:20Z, exit code 0 and fresh
  clean-shutdown marker confirmed. No service/UI remains, no BootFlag/applied profile, safety
  hashes unchanged, no run started. Operator will launch Desktop dev.bat himself.
- Verified embedded readiness PowerShell syntax and diff whitespace; launcher was not executed
  so the real manual first-use sequence remains the operator's test. Do not launch it for him.
  Heartbeat stays paused; preserve logs and await the manual result for algorithm analysis.

## Previous (2026-09-16 08:28Z) — prepared session, subsequently stopped for manual BAT launch
- Operator requires EVERY new validation to exercise Full Reset → Clean Run, with no reuse
  of prior measurements/frontiers/profiles. This is the primary fresh-start acceptance path;
  repair its failures instead of switching to Persistent/Resume to get a result. Preserve
  actual incident history separately; this GPU is not literally one with no safety history.
- Today's authorization at 08:19:44Z was finished by Full Reset at 08:20:24Z before any claim/run.
  Reviewed the export and audit: no new incident, idle/stock, no checkpoint/applied profile,
  Safe Loop and condemnation hashes match the previous checkpoint. No additional Reset needed.
- Old service 8716 exited through cooperative console Ctrl+C. Helper could not read its exit
  code; the fresh clean_shutdown.txt at 08:26:03.685Z independently confirmed completed cleanup.
  New release service 15504, wrapper 6140, started 08:27:05Z; stdout/stderr captured in
  target/beta/overnight-run-20260916/service.log. Same release SHA-256 207C1EB6...2963BC.
- Explicit one-run development authorization granted 08:27:32Z after review, covering the
  operator's planned overnight Clean Run (Standard timing). Verified running=false, block=null,
  Safe Loop idle, BootFlag false, history unchanged. UI remains open; operator clicks Forge GPU.
  Do not reset, restart, rebuild or reauthorize this prepared session. No run was started by Codex.
- Evidence: target/beta/overnight-run-20260916/ (report, before/ready snapshots, authorization,
  clean shutdown marker and service logs). Monitoring heartbeat stays paused; no new scheduled
  monitoring was requested. When resuming, inspect live state before doing anything.
- docs/development-validation.md now makes the order explicit: export/review → Full Reset →
  authorization → Clean Run. A reset after authorization consumes permission before Start too.

## Previous (2026-09-16) — DX11 v3 and power routing complete; full-run acceptance pending
- DX11 v2 drained the GPU before each 9 MiB CPU checksum (7.718 ms in a release reproduction).
  DX11 v3 overlaps one bounded batch with CPU hashing of the fenced staging copy. Normal/Stop
  completion checks the last batch; errors drain queued work before reset. Exact Apply is v30;
  CandidateCrash safety retains its separate v29 floor across budget, cones and reconciliation.
- SECOND AND FINAL point diagnostic finished 2026-09-15 22:02:29Z, exit 0. Same 1830 MHz /
  943 mV, 120 s stock preheat + 420 s dwell: 261424 frames/16339 checks (+28.9%), 98.85% GPU
  utilization, 199.911 W p99 on a 200 W board, 76 C max, residency 0.052% versus 35% required.
  Still Inconclusive, no integrity/TDR/device loss. The first window had 5.253% residency and
  only 180.102 W p99: its 98.07% driver cap flag alone did NOT prove power saturation.
- Full reset-clean DX11 with numeric p99 above existing publication headroom now returns
  ExactApplyPowerCeilingExceeded before identical retries/remaining lanes. Raw Inconclusive
  stays durable; no blacklist. This routing also closes upward-voltage repair for that clock,
  allowing the existing lower-clock synthesis path. Writer/0 MHz/35% gates remain unchanged.
- Validation: cargo test --workspace: 704 passed, 2 hardware ignored; real DX11 stock smoke
  passed separately (wrong render/compute goldens, mid-run cancellation and reuse included).
  Release built; SHA-256 207C1EB65863218C0088EF48FD8FD1BF13C6F55CB32F2BEAF041ED9DEC2963BC.
  The paired physical run used the previous pipeline build, before the new power-routing patch.
- Both authorized diagnostic windows are consumed. No service/load, Safe Loop idle, pending
  incident null, BootFlag/applied profile absent; Safe Loop/ledger hashes unchanged. Heartbeat
  remains paused. Evidence: target/beta/clean-run-20260914/, full report:
  docs/clean-run-investigation-2026-09-14.md. Do not relaunch a point diagnostic or a full Forge.
- NEXT: the operator's command-based manual run must use the new release and its own eligible
  development authorization. It must verify early power rejection and eventual four-lane profile
  approval (or an honest explained refusal). No installer rebuild is needed for this workflow.
  Full product/hardware acceptance remains open; the paired point was not approved.

## Previous (2026-09-15) — bounded point diagnostic complete; algorithm investigation
- Operator stopped f2-forge-1789410879755 at 23:38:41Z on September 14; cooperative
  Stop confirmed stock/idle, cleared BootFlag, saved checkpoint and consumed authorization.
  Export/snapshots/audit preserved in target/beta/clean-run-20260914/. Heartbeat PAUSED.
  After the operator's later reboot, the bounded diagnostic was run once and no
  service or diagnostic workload is now running.
- Full findings: docs/clean-run-investigation-2026-09-14.md. 103 observations: 31 Discovery
  stable, 9 clock_drop, 24 Frontier stable, 39 exact-Apply DX11 inconclusives across 13 pairs.
  38 full 420 s attempts plus one canceled; all target_residency_low (0.020–1.799%, required
  35%), no recorded integrity/crash failure. DX11 alone consumed 272.08 minutes.
- Pure planner replay confirms a descending curve after the anchor: 943 mV raised to
  1830 MHz, next 950 mV left at 1755 MHz; seven higher bins below target. Existing verifier
  accepts this capped shape. Base changes were observed in stock controls while the run
  reuses its initial base; exact clock residency is not established by offset verification.
  Physical cause is not yet isolated: voltage lock, curve shape, base changes and workload
  interact. Do not claim thermal drift or driver normalization alone explains this run.
- Added --replay-plan-1830-943 to dx11-stock-probe; release build/replay passed without GPU
  calls/load. The one authorized point diagnostic then completed a 420 s DX11 dwell:
  5.253% target residency versus 35% required, avg/p5/p50/p95 1767/1665/1785/1830 MHz,
  98.07% power-capped fraction, 71 C maximum, no integrity/TDR/device-lost event.
  Readback showed 943 mV at +90 MHz and effective 1830 MHz at 943/950 mV throughout
  42 dwell snapshots; reset succeeded, BootFlag cleared, no profile was persisted.
- The planner's software valley did not appear in the effective curve during this dwell.
  Its 180.102 W p99 did not support the initial saturation hypothesis based on the driver flag.
- Historical next action (now completed above): one matched comparison, two 420 s windows
  total. Keep exact 0 MHz / 35% thresholds and preserve the mixed worktree and evidence.
  Audio issue was resolved by the operator and is outside this investigation.

## Historical monitoring resumed (2026-09-14 23:24Z) — user requested continuation
- Re-enabled acompanhar-clean-run-do-nidavellir at the existing 15-minute interval. Five-hour
  usage is now 1%; weekly usage remains 96%, so keep each normal check minimal and quiet.
- Collector PID 8268 stayed active through the pause. Latest sample 23:23:39Z: same run still
  qualifying 1830 MHz / 943 mV, GPU 71 C, no pending incident/reboot, 22 condemnation rows,
  and no qualified profiles yet. No service restart or GPU mutation was performed.
- Continue to completion/failure or the existing September 15 monitoring cutoff. Preserve
  the final report/audit and pause the heartbeat when this run ends; never auto-start another.

## Monitoring checkpoint (2026-09-14 22:37Z) — Codex heartbeat paused for quota
- Same Clean Run remains active; last sample 22:36:28Z: exact-Apply qualification at 1845 MHz /
  937 mV, GPU 70 C, no pending incident/reboot, 22 historical condemnation rows, no qualified
  profiles yet. Several earlier DX11 qualifications were inconclusive due to low target residency;
  the engine exhausted its three attempts per pair and moved through voltage/clock candidates.
- Account usage reached 98% of the five-hour window and 96% weekly. Paused heartbeat
  acompanhar-clean-run-do-nidavellir; no further Codex reviews are scheduled while paused.
  Read-only collector PID 8268 remains active until run completion, read failure or its 24-hour
  limit (approximately September 15 18:44Z). Safe Loop protection is still the service's job.
- Evidence: target/beta/clean-run-20260914/quota-pause.json and quota-pause-snapshot.json, plus
  continuing samples.jsonl/latest.json. No Stop/Reset/ACK/Apply/authorization or service restart.
- NEXT on return: inspect collector status/final snapshot and current run identity, export the
  matching run if accessible, preserve final audit and review coverage failures. Do not claim
  completion or qualification based on this intermediate checkpoint; do not automatically retry.

## Active monitoring (2026-09-14 18:42Z) — operator-started Clean Run
- User started Standard/Clean Run f2-forge-1789410879755, then authorized unattended monitoring
  and necessary intervention while away. Service PID 20080, UI PID 7160; IPC build matches the
  standalone release ending dirty-0ef50ca0cca1. Keep code, service and UI frozen during this run.
- Stock preheat converged at 68 C; latest sampled frontier test was 1905 MHz / 950 mV, GPU 67 C,
  with no pending incident/reboot requirement and 22 unchanged historical condemnation rows.
  These are intermediate observations, not final profile or hardware acceptance.
- Read-only collector PID 8268 saves native IPC and bounded nvidia-smi samples every minute under
  target/beta/clean-run-20260914/. Start with monitor-status.json and the last two samples.jsonl
  rows; latest.json contains the full snapshot. Stops on completion/run change, three read failures,
  or 24 hours. No GPU mutation, automatic recovery or renewed authorization is implemented there.
- Thread heartbeat acompanhar-clean-run-do-nidavellir checks every 15 minutes and stays quiet
  while normal. On completion/failure, preserve evidence, export the matching run if accessible,
  report outcome and pause heartbeat. If an actual safety problem leaves load active, cooperative
  StopPowerSweep is authorized; verify stock. Never automatically retry a run or erase history.
- Collector setup initially exposed invalid control characters in PowerShell-rendered JSON output;
  evidence retained in errors.jsonl. Collector now reads native IPC JSON directly and was verified
  live. A transient busy-pipe connection was confirmed harmless (service/process/heartbeat and GPU
  test kept progressing); collector-only restart added bounded connection retries. Production code
  remains untouched. Codex checks depend on app/quota availability; immediate
  protection remains the service Safe Loop. No service restart, tuning command or build was run.

## Current workflow decision (2026-09-14) — commands during development; operator runs acceptance
- The operator prefers command-based execution until end-to-end validation, then sends the
  run results/logs for analysis across Codex quota windows. Use a release console service and
  ordinary-user Tauri dev UI. Rebuild/restart between runs; freeze code/processes during a run.
- Avoid scripts/dev.ps1 for hardware acceptance: when cargo-watch exists it automatically
  rebuilds/restarts the service on Rust edits. Direct console execution has no such watcher.
  Revisit installation only for installer-specific changes or the frozen release candidate.
- No service/UI/build/workload was started for this workflow discussion. The 3/2 refusal still
  requires an explicit, justified resolution before the operator's planned manual acceptance.

## Active (2026-09-14 08:21Z) — explicit single-run development authorization implemented
- Operator approved the proposed controlled release of a new validation while preserving every
  historical exclusion. Implementation is opt-in console only: --development-validation enables
  AuthorizeDevelopmentValidation {reason}; ordinary console/SCM policy stays unchanged.
- Preconditions: service-wide idle lease/reboot guard, Safe Loop Idle/no pending recovery, no
  BootFlag/last validated/applied descriptor/checkpoint, strict ledger reads and verified stock
  reset. Authorization itself starts no workload and never ACKs or deletes positive/negative data.
- Runtime permission covers one fresh Standard run, durably claimed before worker spawn. All
  old CandidateCrash rows still feed the same TDR cone. First new/changed crash latches permission
  closed, including after history rollback; completion/Stop/Reset/exit also consumes it. No Resume,
  Long, other tuning workers or Apply in this opt-in session. Audit never reloads permission.
- Synced audit under ProgramData/Nidavellir/development-validations contains reason, exact build/
  GPU/driver, original Safe Loop/condemnation snapshots and claim/finish events. ExportForgeLog
  links it. All themes display development_validation_note; saved checkpoints cannot restore it.
- scripts/dev-service-admin.ps1 supports -Release -DevelopmentValidation and refuses an existing
  process/installed service. scripts/authorize-validation.ps1 is read-only by default; -Authorize
  -Reason explicitly authorizes with bounded waits/no retries. Guide: docs/development-validation.md.
- Verified: 700 workspace Rust tests (498 service, including 8 new), 17 Node tests, 3 J12 browser
  cases, 4 command scenarios over isolated Windows named pipes, UI build and release service build.
  First command probe found PowerShell task-return output/Dispose issues; fixed and rechecked.
  One new checkpoint test initially omitted required fixture fields; fixed, whole workspace passed.
  Evidence: target/beta/development-validation/ (earlier service log also under target/beta/).
- New standalone release SHA-256 4F48C61154457C5B06C9917AB579244DF5D58783F5F4B4EB7B09D8B2D26828B5
  (12,641,792 bytes; built 08:19:15Z). Build revision ends dirty-0ef50ca0cca1. No installer rebuild;
  target/release/release-manifest.json/old sidecar/kit refer to the earlier package and must not be
  treated as matching this standalone service. Source deletions of the two unused images preserved.
- No live service/UI, authorization, stock reset, ACK or GPU workload was started. Read-only native
  ordinary preflight still reports 3/2. Safe Loop 1898C808...EE30F and ledger 490A5B75...98802 unchanged;
  no service/process/active tuning files at final host inventory.
- NEXT: operator opens the command-based session and explicitly authorizes then starts Standard,
  retaining logs for review. Do not silently launch Forge. D3/Apply/real-use acceptance stays pending.

## Previous checkpoint (2026-09-14 02:10Z) — installed safety guidance and real diagnostic export passed
- Tested the pinned 08937E75...80417 installer on the authorized PC in a fresh temporary
  installation. Actual ordinary-user Tauri UI and native IPC passed: Protected Safe Loop,
  Review safety block guidance, real report export with all three acknowledged incidents,
  and direct Sentinel/Rejected hardware points navigation. Screenshots visually reviewed.
- Native before/after reads remained idle/non-running, no applied profile, no recovery ACK
  pending, with the same 3/2 start refusal. No Reset/ACK/Start/Resume/Apply or GPU workload ran.
- Normal UI close and checked uninstall passed. Post-check: all 193 original data files remain;
  only normal heartbeat, clean-shutdown and Sentinel-startup markers changed. Safe Loop and
  condemnation ledger hashes match the backup. No service/process/shortcuts/debug listener or
  active tuning files remain. Only the expected direct-test uninstaller stub remains in
  C:\Program Files\Nidavellir Safety Guidance Acceptance.
- Evidence and verified backup: target/beta/installed-safety-guidance/. session.json,
  ui-result.json, post-verification.json, guidance.png/history.png and exported-diagnostic.txt.
  Real exported report: C:\ProgramData\Nidavellir\nidavellir-forge-log-2026-09-14T02-09-10-292804+00-00.txt
  (3,108 bytes, SHA-256 693F0942617131453D16EA543B98F5CA49C6CEB9C3751077531C69F1D22225E4).
- Source preflight found two assets deleted since the previous manifest check: gpu-hero.png
  and themes/command-gpu.png under apps/ui/src/lib/assets. Preserved those deletions; no current
  source references were found. The other 138 source entries match. The installer itself and
  extracted/installed UI payload match exactly; this pass does not claim all current files
  match the build manifest. No rebuild or production-code change was needed for this check.
- NEXT: installed D2.6 is verified; do not repeat it. D3 hardware acceptance stays blocked by
  the preserved 3/2 policy. Eligible hardware or a separately justified policy decision is
  needed; clean-Windows and remote CI evidence remain pending. Keep the mixed worktree intact.

## Previous checkpoint (2026-09-14 01:54Z) — actionable safety block after reset; updated installer built
- The operator reported that manual Reset still left an attention warning with no remedy. At the
  start of this continuation, persisted Safe Loop was idle, safe_mode=false, pending incident=null.
  Live read-only PowerSweep IPC was idle/non-running with the persistent 3/2 refusal. This supersedes
  older checkpoints that still show September's incident pending. We did not run Reset or ACK.
- UI bug: start_block_reason was mixed into Safe Loop recovery attention and left the main button
  disabled. Separate recovery status from automatic tuning refusal. The main action now opens
  Review safety block, with reason, Reset limitations, existing diagnostic export and direct
  Sentinel/history navigation in all three themes. Review cannot Start/Resume/Apply a GPU profile.
- Full Reset refreshes readiness before promising another run. Persistent refusal now shows
  Reset completed; tuning blocked. Recovery acknowledgement also explains a remaining refusal.
  Existing profile Apply stays disabled. No backend/qualification policy or ledger was changed.
- Validation: 17 Node tests; 15 existing browser journeys passed, followed by four final J09 tests
  (three themes + saved-profile Apply refusal). Screenshots visually checked. Initial history-link
  test found the wrong target tab; fixed it and rechecked. Browser IPC is simulated, not hardware.
  Evidence: target/beta/safe-loop-guidance/, safe-loop-guidance-*-final.log and live-state.json.
- The final optional native export probe could not connect: service and UI processes were absent
  by then. safe-loop-guidance-native-export.json explicitly records unavailable, not a passed export.
  No service was restarted. At final check safe_loop.json hash is
  1898C808F49171AB409F54E99C9B25F283D46E341454363D1E859008F67EE30F and the condemnation ledger
  remains 490A5B753C8E48CFFCFB7747328614B1C44F54B02FC4A7BB0DCFC338A3898802.
- Current installer built 2026-09-14T01:54:50Z (September 13 locally):
  target/release/bundle/nsis/Nidavellir_0.1.0_x64-setup.exe, SHA-256
  08937E759599244469D94B48E9F50BE6E87C115756232E5284E2ACB203780417.
  All 140 source entries match; service binary is byte-identical. Build log:
  target/beta/build-full-release-safe-loop-guidance-final.log. Canonical acceptance kit/ZIP refreshed;
  diff-check passed. This new UI package was built, not reinstalled on the host in this continuation.
- NEXT: D3 remains blocked by preserved negative history. The user's current request was the
  missing UI explanation/actions, now fixed; no separate policy-change decision has been made.
  Retain prior installer acceptance; do not repeat hardware/algorithm gates without a new reason.

## Previous checkpoint (2026-09-13 19:25Z) — published legacy upgrade and retired-resource cleanup passed
- Downloaded and hash-verified the actual GitHub v0.3.1 installer (198B1AAB...65036).
  It and the current package both report 0.1.0; this proves migration between distributed builds,
  not a numeric version increase. Old hooks reproduced the service-name mismatch: no legacy
  service or UI ran. All 193 original data files remained byte-identical after old installation.
- Actual upgrade to the frozen 7524EFB1...A301C package started the current service; Ping passed.
  Evidence: target/beta/legacy-package-v0.3.1/upgrade-acceptance.json and upgrade-completion.json.
  The first runner stopped on a wrong UI hash expectation. Tauri changes only its bundle marker
  UNK to NSS, then restores the standalone executable. Installed UI exactly matched the payload
  extracted from the pinned installer. Original failure evidence is retained; no reinstall was needed.
- Real uninstall left six retired CPU resource files. Fixed only windows/hooks.nsh: exact Delete
  paths and non-recursive empty-directory cleanup; no shared driver uninstall. Rebuilt NSIS and
  verified over the actual retained resources, including an unrelated file that must survive.
  legacy-cleanup-before.json reproduces six leftovers; legacy-cleanup-after.json passed at
  2026-09-13T19:25:50Z: all six removed, unrelated fixture preserved then removed by its test,
  existing PawnIO driver unchanged. Service/UI/shortcuts absent; only direct-test uninstall.exe remains.
- Current installer: target/release/bundle/nsis/Nidavellir_0.1.0_x64-setup.exe, built 19:24:15Z,
  SHA-256 DFD6D122593DB92BD80033DEDADC8714E531143885BCA0470B692ADF3CCFD63C.
  Previous frozen installer/manifest retained in legacy-package-v0.3.1/current-before-cleanup-setup.exe
  and manifest-before-cleanup.json. All 140 source entries match; only hooks.nsh changed between
  source manifests. Service binary is byte-identical; no reason to repeat unchanged Rust/UI journeys.
- Final post-verification.json (19:29:12Z): no service/process/shortcuts/debug listener/active tuning
  state; no original data file missing; only clean_shutdown.txt, heartbeat.txt and Sentinel startup
  marker changed normally. Safety hashes match backup. Refreshed acceptance-preflight-after-legacy.json
  retains the 3/2 refusal. Canonical installer-acceptance kit/ZIP refreshed and identity verified;
  git diff --check passed. Packaged Inspect correctly refuses a new lifecycle from this unelevated
  shell and occupied test directory (direct-test uninstaller stubs). This is not a new live-test pass
  and does not invalidate the completed phase-specific acceptance.
- Read-only safety-policy-review.json confirms three effective v29 incidents: two explicit August
  TDRs and September's armed restart of uncertain cause. Count spans runs with no expiry/campaign ID.
  Pair rehabilitation also removes condemnation/cone sources; it is not a counter-only correction.
  No ACK/reset, safety-policy/contract edit, negative-history rewrite or GPU workload occurred.
- NEXT: operator response on another NVIDIA GPU versus a separate policy review; D3 stays blocked.
  Pristine-Windows acceptance and remote CI remain unverified. Do not repeat passed live scenarios.
  Keep the current narrow package cleanup and pre-existing mixed worktree; no commit/push/release.

## Previous checkpoint (2026-09-13) — installed desktop offline/reconnect/failure tests passed; cleanup complete
- The operator resumed and accepted Windows elevation. Final session passed at
  2026-09-13T07:57:32Z in target/beta/installed-desktop-remainder/finish-session.json.
  Actual SCM Stop, disabled UI mutation actions while offline, missing-binary helper exit 1,
  failed real SCM start (return 8, remained Stopped), exact binary restoration/start, automatic
  UI reconnection, normal window close and uninstall all passed. Native state stayed idle/stock.
- post-verification.json confirms no Nidavellir service/process, UI/service binaries, test
  Desktop/Start Menu shortcuts, debug listener or active tuning files. Safe Loop and ledger are
  byte-identical to the 193-file backup. NSIS direct-test uninstaller stubs may remain in test dirs.
- An earlier orchestration attempt misreported successful child phases as failed; Stop/Uninstall
  phase journals show they succeeded. Kept that evidence in installed-desktop-acceptance/.
  The follow-up uses an owned Diagnostics.Process handle; explicit exit 0/1 probes and the final
  full session passed. This was a test-runner issue; no production source change was needed.
- Frozen installer remains 7524EFB1DE40464F0356B748E1AD70042F7D7EA3775CDCCFD791466A336A301C;
  all 140 recorded source hashes match. No reason to repeat the unchanged Rust/UI software gate.
- target/beta/acceptance-preflight-after-desktop.json still reports pending 1920@943 recovery
  and 3 effective v29 incidents versus limit 2. No ACK/reset/Forge/Apply/GPU workload was run.
- NEXT: D3 eligible-hardware acceptance or a separately justified safety-policy decision; legacy
  release upgrade/clean Windows evidence and remote CI remain unverified. Do not repeat passed
  installer or desktop tests. GitHub read-only inspection confirms published v0.3.1 carries an
  installer named Nidavellir_0.1.0_x64-setup.exe (digest 198b1aab9d788326f7588ba15cd2cfe97a5c46c0ff4b541ab635fd5a81665036),
  so milestone tag names are not installer versions. No old package was downloaded or executed.

## Previous checkpoint (2026-09-13) — desktop online passed; canceled Windows UAC
- Using the already authorized PC. Installed the frozen 02:39:30Z package into
  C:\Program Files\Nidavellir Desktop Acceptance. Evidence/193-file backup and phase controller:
  target/beta/installed-desktop-acceptance/. Do not run Install again over this test.
- UI process 6832 opened the actual installed executable with an isolated WebView data directory
  and loopback debugging port 9226. Its token is not elevated. Public Desktop and Start Menu
  shortcuts point to this executable. Onboarding detected RTX 3060 Ti and the Forge page displayed
  real online service / stock / pending recovery / 3-versus-2 safety refusal. No GPU commands ran.
- Native control is via Playwright CDP (agent-browser CLI unavailable). An initial harness assertion
  incorrectly expected incident MHz instead of the higher-priority budget refusal; corrected once.
  assertion-mismatch.json preserves that harness failure; ui-onboarding.json passed with no JS errors.
- Actual native read requests returned SafeLoop, PowerSweep (idle, running=false, 3/2 refusal) and
  GpuApply (no profile). The Tauri invoke property is immutable; the attempted observer wrapper did
  not install. An empty ipcTrace is not evidence of absent traffic. native-read-responses.json is
  the actual response evidence. Settings/Forge navigation passed with no JS errors.
- Windows UAC for Stop returned cancellation before the phase began. No Stop-result.json exists;
  ui-offline.json timed out because service stayed Running, not because offline behavior failed.
  Do not silently retry the canceled elevation. The window was closed normally at 07:47:57Z;
  no UI process or debug port remains. NidavellirCore remains installed and Running/Automatic.
  Safe Loop/ledger remain byte-identical; BootFlag/applied/checkpoint remain absent.
- NEXT: with Windows consent available, complete test cleanup in the existing install. To finish
  remaining evidence first, reopen the isolated UI, approve Stop, verify offline, run MissingBinary
  (restore exact binary in finally, then Start), verify reconnect, close UI and Uninstall. The host
  scope is already authorized; Windows UAC still requires interactive acceptance. Inspect phase
  reports before any retry. Summary: target/beta/installed-desktop-acceptance/desktop-summary.json.

## Previous checkpoint (2026-09-12) — authorized host installer lifecycle passed
- The operator explicitly authorized this PC instead of a VM. ExistingPc mode is now documented
  in docs/installer-acceptance.md; VM is optional. No further permission is needed for that scope.
- The real Windows 11 Pro build 26200 / ASUS host passed all six recorded steps at
  2026-09-13T02:40:51Z: 192-file verified backup, installation with retained history and Ping,
  running reinstall, cooperative SCM Stop, stopped reinstall, uninstall/history retention.
  Separate unelevated named-pipe Ping passed. This is not a clean-Windows or desktop-UI test.
- Evidence and backup: target/beta/installer-host-acceptance/acceptance-report.json,
  post-lifecycle-verification.json, unelevated-ping.json, data-before-install/.
  Exit code 0. Service/processes and installed UI/service binaries are absent after uninstall.
- Safe Loop and condemnation ledger remain byte-identical to the pre-install hashes. Only
  heartbeat.txt and sentinel_watcher_startup.json changed; clean_shutdown.txt was added normally.
  No ACK/reset/Forge/Apply command or GPU workload ran. Do not restore old operational data.
- Current package: 2026-09-13T02:39:30Z; log target/beta/build-full-release-host-lifecycle.log.
  Installer SHA-256: 7524EFB1DE40464F0356B748E1AD70042F7D7EA3775CDCCFD791466A336A301C.
  All 140 source hashes match. Runner parser, injected lifecycle checks and packaging passed;
  Rust/UI logic is unchanged since the prior 692 Rust / 16 Node / 16 browser gate.
- Refreshed target/beta/acceptance-preflight-after-host-install.json still refuses the GPU:
  3 effective v29 crashes versus limit 2, pending 1920@943 acknowledgement. No active/reapply state.
- NEXT: remaining D5 desktop UI, different-version update and failure-path evidence; D3 needs
  eligible hardware or a separately justified policy decision. Preserve negative history and
  frozen contracts. Do not repeat the already-passed lifecycle just because the session resumes.
  No public release, commits/pushes, usage-reset redemption or automatic wakeup was performed.

## Previous checkpoint (2026-09-12) — before authorized host execution
- Current session: corrected stale startup bugcheck attribution. The reader previously reused
  August 4's 0x116 during September 10 recovery. It now requires a report within [BootFlag time, now];
  otherwise classification is Unknown. Three new regressions pass; all 692 workspace Rust tests pass.
  target/beta/restart-attribution-audit.json contains exact read-only Windows events and ledger hashes.
  Kernel-Power/41 code zero does not establish the cause, and armed-candidate policy stays frozen.
- Added scripts/installer-acceptance.ps1 and docs/installer-acceptance.md. Prepare/Inspect do not
  install. Run requires a supported disposable VM, elevation, no physical NVIDIA and pristine paths.
  It journals install/Ping/hash, running/stopped reinstalls, SCM stop and uninstall/history retention.
  Windows PowerShell 5.1 host refusal passed; a runner changed after its manifest was also refused.
  Actual VM execution remains untested. Hyper-V is present, but this unelevated session cannot Get-VM.
- Current packaging passed at 2026-09-13T01:51:49 UTC (September 12 locally); log:
  target/beta/build-full-release-attribution.log. target/beta/installer-acceptance.zip contains the
  installer, manifest, pinned runner and guide. Packaged Inspect and host Run refusal passed in
  Windows PowerShell 5.1; all 140 recorded source file hashes match the frozen manifest.
- The operator authorized D1–D5 across intermittent usage windows. roadmap.md owns the checklist
  and remaining sequence; product.md freezes the NVIDIA/Windows beta. Preserve the mixed worktree.
- Completed this block: truthful recovery/reset/Resume, actual start refusal errors, persistent
  start_block_reason, NVIDIA onboarding, Standard default, explicit qualified Apply/Return to stock.
  Async Tauri IPC has 5s connect / 30s exchange deadlines, no mutation retry, bounded framing and
  owned cleanup. Service pipe error paths close handles; slow read polling is coalesced.
- Passed: 692 Rust tests (2 explicit hardware smokes ignored), 16 Node tests, 16 Playwright journeys,
  injected Windows-service lifecycle tests, UI production build and diff-check. Browser transport
  is simulated; native pipe tests use isolated test pipes. CI is defined but not remotely executed.
- Shared shutdown.rs closes IPC admission, supervises all workers/Sentinel, confirms required stock
  recovery, preserves the applied descriptor/pending incident/negatives and commits clean shutdown
  only after success. Console grace is 30s, SCM 20s, worker wait 10s. Timeout exits the whole process
  nonzero; it never resumes service beside an abandoned GPU thread. Windows may preempt the grace.
- GPU validation now cancels between stages and retains running ownership through context teardown.
  Sentinel Event Log/canary use independent activity gates and release them during 60s cooldown.
  Untouched installs do not require a GPU reset; prior tuning cannot use driver absence as proof.
- SCM starts as StartPending and requires a 5s pipe-ready handshake before Running. Listener/ACL
  failures are fatal, client disconnects retry, and a later listener failure triggers cleanup.
  Installer stop checks reject nonzero service exit codes even when retried against Stopped.
- New evidence: 6 shutdown tests (including real isolated process termination with a stalled cleanup),
  worker ownership and 2 listener lifecycle regressions. target/beta/shutdown-offline-gate.log has
  the full offline gate; target/beta/shutdown-final-rust-tests.log has final Rust results.
- Current source/artifact hashes are in target/release/release-manifest.json. Installer SHA-256:
  4597BDDCF51C6055345652D287EBDF8D760B54DC2537512EECDD0C12CCCD0FFA.
  Earlier September 12 UTC packages are superseded. No installer was launched or published.
- Packaging exposed inherited PowerShell 7 module paths inside npm/cmd's Windows PowerShell 5.1.
  Reproduced Get-FileHash lookup failure; both build scripts now import their host's Utility manifest.
  The same full packaging command then passed, including identical release/sidecar SHA-256.
- Read-only backend acceptance-preflight confirms: RTX 3060 Ti / driver 610.88; 3 effective v29
  CandidateCrash incidents exceed the current limit of 2; 1920@943 ACK remains pending. No service
  running, no checkpoint/BootFlag. target/beta/acceptance-preflight.json retains the exact report
  and embedded revision 069af31eb29fde958c15fbb1520b5b3e02f8bf31-dirty-e6ca7fed980b.
  Refreshed at 2026-09-13T01:53:19 UTC. Safe Loop/condemnation hashes still match the read-only
  attribution audit and prior shutdown-safety-hashes-verified.json. No live ACK/reset/GPU load ran.
- NEXT: isolated install/update/uninstall (Hyper-V access denied here; Sandbox not found), followed
  by eligible-hardware acceptance. Do not erase negatives, fabricate rehabilitation or bump the
  qualification contract to unblock this GPU. D2/D3/D5 are not complete from software tests.
- The operator was asked whether a disposable VM is available or must be prepared in this PC's
  Hyper-V; no answer has arrived at this checkpoint. Confirm the environment before dependent work.
- Usage at session-4 final checkpoint: 63% consumed in this renewed 300-minute window. No reset credits,
  wakeups, commits, pushes or host service replacement. Resume by reading this section/roadmap
  and checking actual files/processes; do not repeat passed checks without a new change.

## Latest (2026-09-11) — Full Reset completes Safe Loop recovery
- Fixed the stale recovery state after Full Reset: the confirmed reset now acknowledges the previous
  incident after checked stock recovery and BootFlag clearing, before deleting the old checkpoint.
  This also handles the already-reset state where the checkpoint is absent but the incident remains.
- Reuses strict CandidateCrash ledger acknowledgement; history, blacklist, Rigid/Quarantine, TDR cone
  and Sentinel remain intact. Same-boot reboot requirements block Full Reset in the backend as well.
  Ordinary stock reset still leaves acknowledgement and the same-run checkpoint available separately.
- Resumed after the operator's power outage and verified saved changes against the pre-edit snapshot.
  All 478 service tests pass, including pending-alert recovery with/without a checkpoint and refusal
  on armed/corrupt safety state. Service dev build, production UI build and diff-check pass.
- No live reset, acknowledgement, service restart or GPU workload was performed. The real
  `1920@943` incident remains pending; start the rebuilt dev service and use Full Reset to resolve it.

## Latest (2026-08-31) — Command Deck hierarchy and truthful readiness pass
- The desktop-first Command Deck now leads with four explicit global facts (Core Service, local GPU,
  Safe Loop and applied profile), identifies the GPU only from local hardware/sensor payloads, and no
  longer renders the generic GPU artwork. Ready, active and terminal runs use real DOM order so the
  relevant profiles/progress/telemetry appear first without CSS reordering.
- Core Service readiness is now an explicit tri-state owned by `Forge.svelte`; stale payloads cannot
  keep Forge, Resume, Apply or Full Reset enabled after a failed poll. Hardware detection retries,
  action errors are no longer cleared by the 500 ms status poll, and destructive actions retain
  defense-in-depth guards for offline, reboot-required and unknown Safe Loop states.
- `ForgeProgress` now presents the canonical five-stage rail, structured current pair and step count,
  Now / Last decision / Next, a run-scoped outcome latch, and separate measured ETA versus conservative
  ceiling. It does not invent API lane, profile role or attempt data absent from IPC. Timer updates were
  removed from live regions and reduced-motion/focus/navigation semantics were completed.
- Safe frontend validation only: production Vite build passed and the 1180x820 browser check showed no
  error overlay, non-empty content, correct page headings/focus and working Forge/Settings/Advanced
  navigation. No Core Service, tuning action or GPU workload was started.

## Latest (2026-08-25) — fail-closed release hardening complete; hardware run deferred
- Independent adversarial reviews found and closed four release blockers before any GPU work:
  CandidateCrash could become terminal without durable ledger proof; Sentinel startup lacked a ready
  handshake; Benchmark could reapply an F2 profile through the F1 writer; and legacy real/memory
  workers could continue after incomplete Safe Loop arming.
- CandidateCrash persistence now flushes the append-only ledger with `sync_data` and requires strict
  readback of the exact Rigid v29 event. A failed append/readback leaves the checkpoint
  `needs_attention`, with raw lane and pending incident intact; startup retries the transaction before
  it may become `interrupted`. All live F2 discovery/synthesis/gate/publication reads are strict.
- Sentinel now releases startup reapply only after the Event Log seed/floor is durably persisted and
  the watcher confirms readiness. F2 Benchmark uses the proof-aware F2 writer. Legacy workers use
  checked preflight, owned BootFlag arm/revalidation and owner-matched clear; unreadable Safe Loop,
  boot flag or condemnation state refuses hardware.
- Apply/reapply additionally require exact GPU/run/contract29 identity and the required number of
  complete ordered matrix-v27 proofs. Full Reset is a positive-learning reset: it quiesces every
  mutating worker and clears reusable profiles/validations transactionally while preserving pending,
  blacklist, Rigid/Quarantine, the TDR cone and Sentinel history.
- Offline validation is green: 674 workspace tests pass with two explicit hardware smokes ignored,
  workspace check, production UI build, Clippy (pre-existing warnings only) and diff-check. Per the
  operator, no service, acknowledgement, run or GPU workload is executed in this session. The next
  session starts from the still-pending `1860@900` incident and a new Standard run after explicit ACK.
  Release and Tauri sidecar are byte-identical (12,515,840 bytes, SHA-256
  `F90E8DF2878422F543656FAAF4551F6941E53586C77E4D3DA58F55F26DC09BCA`).

## Latest (2026-08-14) — split v7/v28/v29 contracts, finite TDR cone and same-run recovery
- Contract identity is now deliberately split: **Discovery v7** for the 10 s PowerRender descent,
  **Frontier v28** for Texture qualification during descent, and **Exact Apply v29** for profile
  publication. The four-lane DX11 v2/Vulkan/DX12/Endurance recipe and workload fingerprints remain
  **matrix v27**; a contract bump does not imply a new synthetic workload.
- Discovery v7 and Frontier v28 each allow at most one adjacent physical clock bin of runtime
  elasticity. Exact Apply v29 remains strict at the labeled target. Three homogeneous structural
  DX11 inconclusives at one exact pair aggregate to `DX11StructuralClockDrop`; that token permits at
  most one vertical repair for the target and never becomes a pass or a ledger condemnation.
- **Clean** now means remeasure positive discovery/profile evidence. It never forgets negative safety
  knowledge: effective `Rigid`, `Quarantine` and the TDR safety cone govern Clean, Standard, Long,
  calibration and exact Apply alike. Older text saying Clean ignores durable boundaries is superseded.
- A rigid v29 `CandidateCrash` projects a finite **1 clock-bin : 1 voltage-bin** cone down the physical
  target/VF tables. Bins at or below the cone are censored before arm/write/dwell as
  `TdrRiskGuard/CensoredBoundary`; censorship is neither pass nor fail and writes no observation or
  condemnation. The persistent frontier crash budget is two effective CandidateCrash incidents for
  the GPU/current contract campaign; once the count exceeds two, new crash-seeking starts fail closed.
- A Sentinel-attributed TDR owns terminal truth: progress becomes `interrupted`, `last_outcome` is
  `TdrOrCrash`, publication stays blocked, and the raw workload row remains honestly unchanged. Resume
  is explicit and transactional: only after a Windows reboot and acknowledgement, and only for the
  same run, exact service build, GPU and driver. Recovery never substitutes a new selected mode/run.
- Real hardware evidence on 2026-08-14 produced v29 CandidateCrash incidents at `1920@931` and
  `1860@900`; both were followed by stock recovery and a required Windows reboot. These events justify
  the cone and exhaust the intended two-incident exploration allowance, but they are not yet a
  successful three-profile acceptance run.

## Historical (2026-08-11, superseded by the split v28/v29 safety contract) — Discovery v7 closes a coherent measured frontier
- `F2_DISCOVERY_CONTRACT_VERSION = 7` permits one adjacent 15 MHz boost bin of clock elasticity
  only during PowerRender discovery. Texture boundary qualification and the complete exact-Apply
  qualification remain contract v27 with exact target residency (`0 MHz` tolerance).
- Boundary reasoning now uses one GPU/run-wide context for resume pruning, live decisions and the
  final summary. A sustainable boundary closes only when current Discovery and current Texture
  qualification belong to the same target/anchor pair. A reset-clean `ClockDrop` can be dominated by
  that same target at a strictly lower voltage, or by a harder target at the same/lower voltage, only
  within the same GPU/run. Direct instability, silent error, TDR/device loss and reset failure remain
  authoritative.
- A dominated `ClockDrop` may continue inside the current discovery call, but only as a bounded-writer
  offset baseline followed by the next strictly lower physical VF bin. It is never promoted to a
  stable point and the path may not revisit or climb a bin.
- Publication uses a monotonic projection over measured evidence: targets are processed in ascending
  clock order and select the lowest current Discovery+qualification anchor at or above the previously
  selected anchor. Equal-voltage plateaus are allowed; a target without a compatible measured pair is
  omitted. No voltage is interpolated, relabeled or invented.
- Apply margin is exactly the next valid physical VF-table bin above the learned boundary, not a fixed
  `+12 mV`; the effective millivolt delta remains observable because physical spacing is non-uniform.
  Before exact qualification, a candidate may reach v27 up to the numeric power cap. Publication then
  requires the worst measured v27 Apply power to retain 1% headroom (`<= 99%` of the cap).
- Contract v7 quarantines v6 positive discovery evidence. The next acceptance run must therefore be a
  new Clean Run, never Resume of the paused v6 checkpoint. Integrated validation passes: 604 workspace
  tests with two hardware smokes ignored, production UI build and workspace Clippy (baseline warnings
  only). Hardware acceptance of the new policy remains pending.

## Historical (2026-08-10, superseded by Discovery v7) — F2 voltage-order reconciliation blocks inverted frontiers
- A 129-minute Clean Run exposed a real architecture bug: after resume, freshly qualified
  `1935@906`/`1920@900` coexisted with earlier reset-clean `ClockDrop` records at 968 mV for
  1905–1800 MHz. Those drops were reused as unconditional `first_bad` voltage boundaries, so the
  easier clocks were not allowed to descend and synthesis produced the inverted preview
  `1875@975 → 1935@906`.
- `ClockDrop` remains append-only evidence, but it no longer acts as a voltage boundary when the
  same GPU/run has both current Discovery and current Qualification for a harder clock at the same
  or lower voltage. Direct `SilentError`, `Unstable`, TDR/device loss and reset failures are never
  relaxed by this rule. The affected Resume will reopen 13 contradicted drops in the real JSONL.
- Publication has an independent fail-closed invariant: qualified frontier voltage must be
  non-decreasing as target clock rises. Any inversion clears the synthesis input and publishes no
  profile until Resume reconciles discovery. Equal-voltage plateaus remain valid when independently
  measured; equality is not an inversion.
- Regression validation: core 110/110 and service 422/422 pass. The paused checkpoint was backed up,
  migrated only to the rebuilt content-addressed revision, and live-loaded with
  `resume_available=true`; no run was started automatically.

## Historical evidence (2026-08-10) — v27 passed the former post-margin candidate on driver 610.62
- Fixed the UI-to-Core control channel after a real `0x80070005` reset failure. The elevated Core
  now creates `NidavellirCore` with an explicit protected, local-only pipe ACL: SYSTEM and
  Administrators retain full access, while the unelevated interactive user receives read/write
  access; remote pipe clients are rejected. An unelevated live probe successfully called Ping,
  Safe Loop status, sweep progress and applied-profile status against the rebuilt service. Service
  tests pass 420/420. The failed UI reset did not execute or delete learning.
- A real `matrix_v27` run at exact `1815@887` passed DX11 v2 420 s, Vulkan 120 s, DX12
  120 s and Endurance 300 s: 192,370 frames, 22,434 checks, 100% target residency in every
  lane, 887 mV min/avg/max, no nvlddmkm event and a checked return to stock.
- The historical safe boundary `1800@875` remained electrically stable for 420 s but delivered
  1785 MHz throughout. The exact-residency gate correctly returned `Inconclusive`
  (`target_residency_low`) and stopped before the remaining lanes. At 887 mV, short delivery probes
  held both 1800 and 1815 MHz exactly. This directly supports retaining the 12 mV margin and upward
  physical-bin snap; relaxing clock residency would certify a point the workload did not exercise.
- Driver/build state is part of the evidence. A label proved on 595.97 is not assumed to retain exact
  delivery on 610.62. Qualification remains finite evidence, not universal stability proof.
- Detector Lab now reports continuous progress inside `running_matrix_v27` and identifies each
  60-second stock API lane instead of appearing frozen. Workloads, timings and verdict criteria are
  unchanged.

## Latest (2026-08-04) — v27 is a finite DX11-first gate with novice-safe TDR containment
- Contract 27 keeps descent single-lane and runs the expensive matrix only on deduplicated final
  Apply pairs: DX11 v2 resident 420 s → Vulkan 120/300 s → DX12 120/300 s → Endurance 300/1,200 s.
  The learned frontier still receives 12 mV plus upward snap to a physical VF bin.
- Trusted `1800@875` passed DX11 for 420.248 s. Rigid `1860@868` had already failed in resident DX11
  around 347 s, matching the Overwatch horizon, but later passed exact repeats of 420.230 s and
  600.247 s. The detector is relevant but stochastic; longer dwell alone does not close the false
  negative, so v27 freezes workload/time instead of repeating or creating v28 variants.
- A physical failure rejects/repairs; `Inconclusive` blocks without false condemnation. Sentinel now
  hands a TDR to active Detector Lab for cooperative cancellation as it already does for Forge.
  `SafeLoopStatus` exposes `gpu_reboot_required`/event; Forge hides same-boot recovery/reset and tells
  a novice only to restart Windows, with the point and learning already saved.
- The honest product claim is limited qualification plus containment, not deterministic certification.
  Full evidence: `docs/f2-disqualification-audit-2026-08-04.md`.

## Historical (2026-08-04) — qualification v26 API matrix
- The operator chose to keep one-click synthetic qualification and require every exact Apply
  candidate to pass equivalent Vulkan, native DX11 and DX12 lanes. This supersedes the earlier
  plan to leave DX11 v2 as a one-off challenger only.
- `F2_QUALIFICATION_CONTRACT_VERSION = 26`. Publication is run-scoped and fail-closed on the full
  `[Texture/Vulkan, Dx11Game, Dx12Game, Endurance]` set. `Fail` rejects; `Inconclusive` blocks;
  missing or pre-v26 positive evidence cannot unlock Apply.
- Wgpu backend selection is explicit. Vulkan and DX12 capture independent stock goldens and run the
  same `V8Texture` recipe for the same duration. The exact-gate order is Vulkan → DX11 v2 → DX12 →
  Endurance; discovery is not tripled.
- DX11 v2 replaces the serialized 768² ALU probe with a 1536² sampled-texture render, alpha/ROP,
  D24 depth, 48-iteration pixel ALU, a 65,536-element compute/UAV kernel and paired render+compute
  FNV checks every 16 queued frames. There is no per-frame Flush/wait. It remains offscreen so all
  three API lanes have the same headless one-click execution model; Present/process isolation remain
  explicit limitations for the hardware bake-off.
- Stock preflight now validates all three APIs before any candidate write. Standard runs each API
  lane for 120 s plus 300 s Endurance (~11 min/pair before overhead); Long runs 300 s per API plus
  1,200 s Endurance (~35 min/pair). ETA uses the same four-entry ladder as execution.
- Software validation: `cargo test --workspace` passes 588 tests with two hardware smokes ignored;
  the ignored DX11 v2 render+compute smoke and explicit Vulkan/DX12 backend golden smoke both pass
  when run individually on this RTX 3060 Ti. Hardware discrimination of `1860@868` is still pending;
  if v26 passes that rigid positive, stop adding synthetic variants and report the limitation.

## Latest (2026-08-04) — Overwatch confirmed the exact false negative and isolated the DX11 gap
- Audit verdict is **NO GO for precise disqualification**. The user's manual labels came from MSI
  Afterburner flat curves, so `1815@875`/`1800@869` did not prove those exact pairs failed. A live
  voltage-locked campaign completed all 13 candidate runs at 60 seconds, including corrected-bin
  hypotheses `1830@875` and `1815@868`, without TDR.
- Diagnostic-only `curve_v25` applies the anchored curve and max-clock ceiling without voltage lock.
  `1815@875` stayed at its anchor for 60 seconds. The strongest historical curve-positive,
  `1860@868`, crossed its old ~253.9-second failure window and finished 300 seconds with every phase
  stable, although clock ranged to 1845 MHz and voltage escaped to 1037 mV. The workload missed the
  available positive; the setting was not validated.
- Workload outcome and application fidelity are now separate journal dimensions:
  `workload_result`, `application_mode`, `anchor_voltage_escaped`, anchor and authoritative ceiling.
  Curve escape is expected metadata, while low/missing telemetry remains inconclusive. Curve runs
  never publish and always reset to stock.
- Detector Lab also flushes candidate recipe/segment checkpoints and has non-learning Safe Loop
  recovery: return stock without blacklist, crash streak or Safe Mode contamination.
- A supervised Overwatch 2 session then made `1860@868` an exact positive: Game Trace captured
  32,525 valid samples and every sample was 1860 MHz at 868 mV. The last high-load block lasted
  83.8 s; the final 60 s averaged 98.83% utilization and 155.37 W. The first nvlddmkm-153 became an
  eight-event cascade, nvlddmkm-14 and bugcheck 0x116; WER named nvlddmkm.sys.
- Overwatch was DX11 (one graphics/compute/copy queue, 2544x1353, 600-FPS cap, Reflex). The current
  qualifier is Vulkan/wgpu. The legacy native DX11 probe is only a serialized 768x768 offscreen ALU
  shader with per-frame Flush/wait, measured 99-133 W historically and never rejected a candidate.
  The missing discriminator is API/work-graph scheduling, not duration, heat or aggregate power.
- Nine live TextureRop canaries passed; the last ended 11.5 s before the TDR. The monitor closed the
  trace about 1.06 s after the first event, but stock reset did not complete before the cascade.
  After reboot the GPU is physically stock and Safe Loop is idle/non-learning, while the diagnostic
  boot flag remains armed for reconciliation; the service is stopped.
- Historical pre-v26 gate (superseded by the matrix above): one native DX11 v2 challenger with realistic Present, frames-in-flight,
  texture/depth/compute/copy work and process isolation. Matrix: stock 2x120 s, safe `1800@875`
  curve 2x120 s, positive `1860@868` curve 1x120 s. If it misses, stop synthetic-detector work and
  use conservative field probation. Full report: `docs/f2-disqualification-audit-2026-08-04.md`.

## Latest (2026-07-23) — v25 makes the selected F2 voltage and clock evidence authoritative
- The `1815@875` Game Trace proved the previous semantic gap: loaded operation was mostly 1800 MHz
  and voltage could exceed 875 mV. Curve flattening plus a max-clock ceiling was an elastic upper
  envelope, not proof that the labeled hardware point had been exercised.
- F2 discovery, qualification, exact Apply and Manual Point now combine the anchored curve with a
  max-only clock ceiling and an exact, read-back-verified NVAPI voltage lock. Clock remains free to
  step down; the active voltage is not allowed above the resolved physical VF bin.
- Discovery contract v6 requires p5 at the exact target. Qualification contract v25 counts residency
  only at the exact target or above and refuses positive evidence when voltage telemetry is missing,
  insufficient or above the selected bin. Old positive evidence is quarantined.
- Detector Lab canonical key is `control_v25` (`control_v24`/`control_v23` remain backend aliases).
  Candidate execution now uses the same sampled qualification coverage as Forge; low clock residence
  or unproven voltage returns `inconclusive` and stock, never a diagnostic pass.
- Every reset/recovery path releases the voltage lock. This intentionally accepts the higher TDR
  exposure of a fixed rail in exchange for testing the requested point; the clock is not min=max
  pinned. Validation passed: workspace tests 583/0 (1 hardware smoke ignored), workspace check,
  production UI build, clippy with baseline warnings only and `git diff --check`. No hardware
  workload was executed during implementation.
- First live Apply returned `Ok` from ClockBoostLock but no retained lock because the private API
  entry id had been mistaken for graphics domain `0`. Read-only hardware inspection showed slots
  `0..6`; the voltage-bearing slot is the highest reported entry. Lock now clears the live table in
  one write, applies only the highest slot in the next, and polls readback for up to one second.
  Unlock clears the whole live table, and verification still requires the exact sole entry/voltage.
- A subsequent Apply showed that driver 595.97 rejects `GetClockBoostLock` with
  `ArgumentExceedMaxSize` after the VF curve and NVML ceiling are active. The shared F2 transaction
  now establishes and readback-verifies voltage first in the stock control state, then applies the
  anchored curve and max-only clock ceiling; any later failure still resets everything. Software
  validation passed (service 415/0, gpu-nvapi 40/0, service check and diff check); live Apply retry
  remains the hardware acceptance gate.

## Latest (2026-07-23) — v24 removes self-failing device churn from Field Concurrency
- The 10-minute `1800@875` Detector Lab journal never applied the candidate: the complete v23 stock
  mirror failed after entering Field Concurrency. A preceding `1815@875` journal failed the same way
  at stock. Those runs test the recipe/driver environment, not either VF point.
- Root cause was structural: Field Concurrency created, stressed and destroyed a fresh wgpu
  device/context every few seconds. Long controls therefore accumulated dozens of device lifetime
  transitions and could reset the driver at stock.
- Contract v24 / Texture Hop v13-r3 keeps the independent secondary TextureRop queue but creates it
  once and leaves it resident for the whole concurrency phase. Initialization, worker panic or
  missing secondary checksum coverage is explicit environment-level `Inconclusive`; it cannot
  become candidate instability, qualification evidence or blacklist.
- Fingerprints are `f2q-texhop-v13-r3/v13-persistent-field-concurrency` and
  `f2q-texhop-v13-r3/endurance-persistent-field-concurrency`. Detector Lab exposes
  `control_v24`; the backend accepts `control_v23` only as a mixed-version alias.
- The manual diagnostic point remains an anchored curve with a 210 MHz-to-target max-clock ceiling,
  not a fixed clock pin. Validation passed: gpu-stress 20/0 (1 hardware smoke ignored), core 106/0,
  service 415/0, workspace check, UI production build and clippy with baseline warnings only. No
  hardware workload was executed while implementing this correction.

## Latest (2026-07-22) — post-TDR runs are quarantined and v23 gets an exact stock control
- The Detector Lab bake-off produced `nvlddmkm-153` at the final TextureRop immediately after Field
  Concurrency three times. Two were at the trusted `1800@875` control after the first TDR, while dense
  v14 passed between them. Those post-TDR v23 results are environment/recipe-contaminated, not valid
  evidence that the control point is unstable.
- A process-local latch is rebuilt from Event Log timestamps newer than the current Windows boot.
  Any new TDR sets `reboot_required`; restarting the service cannot clear it. GPU-mutating IPC routes,
  including Forge/resume, profile/manual Apply and Detector Lab, stay closed until reboot. Reset and
  read-only routes remain available, and no Lab TDR writes blacklist/condemnation.
- Production v23 now proves the entire 60 s Texture Hop sequence at stock once before candidates,
  including the previously untested Field Concurrency → TextureRop handoff. Detector Lab mirrors the
  selected control-v23 duration at stock before applying the point and scopes every journal segment
  as `stock` or `candidate`.
- Detector Lab retains the actual panic payload and promotes a session-correlated Event 153 to
  `tdr` / `reboot_required` with phase/timestamp instead of reporting a generic worker environment
  error. The UI disables new point/Lab actions and explains that Windows must restart.
- Qualification fingerprints and candidate thresholds remain v23; this strengthens environmental
  validity and recovery semantics without importing diagnostic failures into hardware knowledge.

## Latest (2026-07-22) — Detector Lab v14 is isolated from Forge evidence
- Advanced Diagnostics now has an operator-controlled Detector Lab on top of the existing temporary
  manual point. It compares `control_v23` (current Texture Hop) with `dense_v14`, whose short
  HeavySpike/FrameCadence/MixedGame/BoostEdge perturbations each land in a dense golden-checked
  TextureRop canary. Duration is selectable from 15 to 600 seconds; no hardware load was run here.
- The lab is deliberately non-publishable. It does not enter `f2_observations.jsonl`, Standard/Long,
  exact Apply, profiles, blacklist or condemnation. It writes only a separate flushed
  `detector-lab-<epoch>.jsonl` journal, including the segment about to start for post-reboot
  attribution. Qualification contract v23 and production workload fingerprints are unchanged.
- Start requires an active verified manual point. The worker calibrates stock, reapplies that exact
  physical VF bin under Safe Loop, and automatically returns stock on detected failure, Stop, panic
  or environment error. A clean diagnostic pass leaves the temporary point active for a second
  recipe or Game Trace; it is not treated as stability proof.
- UI adds recipe cards, a 15–600 s slider, live phase/time/progress, animated active progress,
  Stop/stock recovery, journal opening and expandable per-segment evidence. Rust tests (core,
  gpu-stress and service) and the UI production build pass. Hardware bake-off is the next gate.

## Latest (2026-07-22) — v23 preserves qualified points and makes cap evidence numeric
- Clean run `f2-forge-1784759763148` completed the search in about 25.7 minutes with no hardware
  incident but no published profiles. Qualified points such as `1860@937`, `1815@937`, `1725@925`
  and `1695@900` were later displaced in the shared frontier by deeper discovery-only passes; exact
  Apply then selected unqualified pairs and reconciliation correctly refused them. Eight pairs also
  generated 24 power-telemetry inconclusives in the 98–99% zone, amplified by a BoostEdge phase too
  short to distinguish numeric board power from sticky `SW_POWER_CAP` samples.
- Qualification contract v23 keeps two views of the same evidence: the physical frontier remains the
  deepest current Discovery pass for learning, while the publishable frontier is the deepest pair at
  each target with every required current qualification pass. Only the latter feeds exact Apply and
  profile synthesis. Inconclusive remains useful boundary evidence, never a published proof or a
  blacklist reason.
- Texture Hop v13-r2 keeps the Field Concurrency workload and dwell totals but raises BoostEdge from
  1% to 4% (about 1.2 s in Standard's 30 s frontier dwell). Texture/Endurance coverage requires 20
  samples and classifies power-bound from BoostEdge p95 >=99% of a valid numeric board limit; the raw
  cap flag is fallback-only. Fingerprints are `f2q-texhop-v13-r2/v13-field-concurrency` and
  `f2q-texhop-v13-r2/endurance-field-concurrency`; old positives cannot unlock Apply.
- Discovery uses stateful cap hysteresis: >=99% NearCap, <=98% OffCap, middle-band measurements retain
  their previous state and a fresh sequence starts conservatively NearCap. No threshold was weakened,
  no per-GPU value was introduced and ambiguous evidence remains outside the blacklist.
- Validation: `cargo check --workspace`, `cargo test --workspace` (576 passed, 1 hardware smoke
  ignored), UI production build and focused frontier/coverage/hysteresis/workload regressions. Clippy
  completes with pre-existing repository warnings only; `git diff --check` is clean.
- Next physical gate: one Clean/Standard run. Confirm that a shallower qualified point reaches exact
  Apply when a deeper point is inconclusive, and that clearly off-cap BoostEdge phases no longer burn
  three retries because of sampled cap flags.

## Latest (2026-07-20) — inconclusive qualification no longer re-hammers an unproven bin
- Clean run `f2-forge-1784423357172` (contract v22 / Texture Hop v13) TDR'd at `1890@925` on the 6th
  texture dwell of that pair. The persisted observations show the 6 `QualificationInconclusive` rows
  were `boost_edge_power_bound` (4×) + `phase_contrast_low` (2×) — deterministic power-cap
  interference (p99 192–195 W vs the 200 W cap), not silicon evidence. The engine then treated the
  unproven bin as good frontier memory and, via the warm-start fallback, re-descended to the same pair
  and re-ran the heaviest qualifier until the driver watchdog fired. The real TDR was blacklisted.
- Fix is engine-only (`gpu_undervolt.rs`, `gpu_power_sweep.rs`); no workload/contract/threshold change,
  no per-GPU constant, no version bump. Inconclusive = unproven, never good-frontier memory or a repeat
  target, and still never blacklisted (ambiguity must not over-condemn top clocks near the cap):
  (1) reason-aware retry — cap-interference reasons no longer burn the retry budget and are logged;
  (2) inconclusive anchors excluded from the `last_good_mv` next-clock seed; (3) warm-start fallback
  suppressed on an inconclusive stop + a guard skipping re-qualification of an already-inconclusive
  pair this run. Pure helpers: `qualification_inconclusive_reason_retryable`,
  `f2_inconclusive_only_anchors`, `f2_pair_qualification_exhausted`.
- Validation: `cargo check --workspace`, service 411/0 (3 new regression tests), clippy baseline. NO
  hardware run. Acceptance = next Clean/Standard run after acknowledge + manual Reset: cap-bound bins
  log their reason and run one texture dwell; skipped clocks seed from the last clean point; no texture
  re-run on the same pair after an inconclusive skip. Watch: 1905/1890/1875 may be unqualifiable under
  v22's 200 W cap (boost always clips) — workload/threshold issue, not silicon; do not weaken the gate.

## Latest (2026-07-18h) — Texture Hop v13 reproduces field context concurrency; calibration stays clean
- Game Trace `1784411518295` showed `1800@831` holding 1800 MHz at 97–100% utilization and about
  145–152 W before power collapsed, nvlddmkm-153 fired twice and the machine rebooted with bugcheck
  `0x133`. Texture Hop v12 and Endurance had already reached virtually the same p95 power (150.5 W and
  151.9 W), so the missing discriminator was not more watts: real gameplay overlapped an independent
  live-Sentinel TextureRop context/queue that the single-device synthetic stack never reproduced.
- Qualification contract v22 / Texture Hop v13 keeps the v12 primary Texture Stack resident while a
  secondary thread repeatedly creates a fresh `GpuCtx`, runs the live 700 ms TextureRop self-check and
  destroys it across compressed irregular gaps. Both contexts keep integrity checks; SilentError,
  DeviceLost and TDR reject organically. A 15 s stock preflight exercises the same dual-device path
  before any candidate write, so an unsupported environment aborts without blacklist.
- Fingerprints are `f2q-texhop-v13-r1/v13-field-concurrency` and
  `f2q-texhop-v13-r1/endurance-field-concurrency`; pre-v22 positives cannot unlock Apply. Standard
  remains uncapped globally and retains the previously selected compact dwell durations.
- The known-failing trace point is calibration evidence only: it was not written to Safe Loop or the
  condemnation ledger and no hardcoded voltage/margin was added. Next acceptance gate is a Clean
  Standard run that rejects it through the reported `field-concurrency` phase.
- Future real-use recovery is transactional: applied profiles now persist `applied_at`, a later
  OC-class WER bugcheck can be correlated to that exact session, live Event Log polling is 1 s, and
  the event cursor advances only after failure accounting/recovery returns. Legacy profiles have no
  timestamp and cannot retroactively import this incident. Game Trace v3 records live canary active/
  sequence markers so the next field failure can prove or disprove overlap.

## Latest (2026-07-18g) — Texture Hop v12 stacks heterogeneous texture load; Standard has no wall cutoff
- Qualification contract v21 turns the legacy `CompositeGameLoad` phase into Texture Stack: cache-
  bound TextureRop, VRAM-bound TextureStream, power render and scattered near-full-VRAM gather share
  one submit. The final render lane rotates and each lane is compared with its own stock golden.
- Texture Hop v12 keeps the standalone TextureRop oracle but replaces the preventive banded
  TextureStream tail with the unbanded Texture Stack. No pre-hang wall-time threshold aborts the
  active stack; SilentError, DeviceLost and Windows TDR all reject the point. A per-frame queue fence
  remains to model present/fence cadence and prevent a false TDR from unbounded submission flooding.
- Standard retains 30 s frontier qualification and 2 min Texture Hop + 5 min Endurance per exact
  Apply pair, but the 59-minute active-work watchdog is removed. It runs the hardware-derived plan to
  completion unless the operator presses Stop or a real failure interrupts it. Long remains the
  explicit exhaustive 60 s + 5 min + 20 min proof.
- Fingerprints are `f2q-texhop-v12-r1/v12-texture-stack` and
  `f2q-texhop-v12-r1/endurance-texture-stack`; pre-v21 positives cannot unlock Apply. Hardware
  acceptance still requires a clean run, optionally followed by Game Trace at a reproducibly bad
  point to compare the real-game failure signature with Texture Stack.

## Latest (2026-07-18f) — Texture Hop v11 promotes the strongest efficient co-load
- Qualification contract v20 keeps TextureRop as the precise stock-golden oracle because collected
  organic silent-error rejections in v17/v18 all reported `texture-rop`; the v19 failures were
  inconclusive rather than a stronger detector finding a new error.
- Texture Hop v11 now runs TextureRop, a short idle-to-CompositeGameLoad slam, then TextureRop again.
  CompositeGameLoad is the suite's highest combined game-like draw (render + texture + near-full
  VRAM gather in one submit). It appears once per cycle to avoid repeating its large VRAM-pool setup;
  about 71% of each dwell is assigned to this binding pair before the broader acceptance coverage.
- Only Texture Hop's exact semantic fingerprint changes to `f2q-texhop-v11-r1/v11-texture`; the
  contract bump quarantines all pre-v20 positive qualification evidence. Standard/Long durations,
  clean-run learning rules, blacklist policy and the one-hour Standard ceiling are unchanged.
- Software tests can verify construction and fail-closed provenance, not the hardware boundary. A
  clean run must still prove that v11 rejects the known-bad neighborhood without rejecting the known
  good control; neither voltage is encoded as a global default or blacklist.

## Latest (2026-07-18e) — manual diagnostic point replaces Texture Lab
- Advanced Diagnostics now applies one operator-selected clock/VF-bin request directly for a real
  workload. Requests resolve to the nearest physical bin within 8 mV and use the normal bounded,
  elastic anchored F2 curve plus max-clock ceiling; no diagnostic hard-voltage lock remains.
- The point is temporary: it clears any saved GPU profile, writes no Forge observations/profiles and
  holds its Safe Loop boot intent until explicit stock reset. Concurrent GPU writes are blocked and
  graceful service shutdown resets it. Game Trace remains independent and can run alongside it.
- Texture Lab IPC/UI, runtime texture tuning, in-memory method history and all unproven external/
  synthetic experimental helpers were removed. Forge qualification was v19 at that checkpoint.

## Latest (2026-07-18d) — manual gameplay oracle and resilient Game Trace
- Exact `1800@868` survived the current synthetic matrix and a 172.5 W p99 Heaven + checksum
  co-load. Overwatch automation reached only the menu (~55 W), so it cannot replace the user's
  known failure-producing gameplay. The active experiment is manual Overwatch at `1800@869`.
- Game Trace contract `game-trace-v2` snapshots the effective VF curve and TDR baseline, timestamps
  every row against Windows events, records real sampler gaps/NVML validity, separates attempted
  from successful voltage reads, and flushes every 50 samples to preserve pre-reset evidence.
- Clean stop appends a summary with power/clock/voltage envelopes, missing telemetry, maximum sample
  gap and before/after `nvlddmkm` TDR correlation. The recorder remains strictly read-only.

## Latest (2026-07-18c) — operator-calibrated Texture Hop
- Texture Lab's Texture Hop now accepts three bounded runtime controls: 8–256 dependent shader
  rounds, 1–64 frames between queue-drain hops and 0–500 ms of true idle transition gap. These are
  real shader/cadence inputs, not display-only sliders or new named workload versions.
- Each unique configuration receives its own stock checksum golden and its parameters are visible in
  current status and bounded trial history. Other lab methods retain fixed plans; the versioned v19
  Forge workload is unchanged and cannot inherit lab settings.
- Hardware acceptance remains empirical: find one configuration that rejects both known-bad points
  repeatably inside 55 seconds, then promote the measured winner into a later Forge contract change.

## Latest (2026-07-18b) — exact-point Texture Lab before more full Forge runs
- The last interrupted run was not organic: its header reported persistent learning and its clock
  results reused BlacklistedBoundary. Clean Run is now always present in every forge-themed run
  selector; Full Reset still auto-selects it, but manual clean experiments no longer depend on that
  temporary UI state.
- Advanced Diagnostics now contains a temporary Texture Lab. It tests one operator-selected point
  with one of four existing qualifier patterns for 10–55 seconds, shows the requested-to-physical VF
  bin resolution, never descends the curve and keeps a bounded comparison history in service memory.
- Lab reset-clean failures do not enter F2 observations, the operational blacklist or the durable
  condemnation ledger. Safe Mode, boot-flag recovery, exact writer verification, reset confirmation,
  device-loss accounting and the service-wide GPU write lease remain active.
- Next hardware work: alternate methods on the known-bad 1800@869 and 1815@875 requests until one
  rejects both repeatably in under one minute. Promote only that proven discriminator into
  Standard/Long; no further full clean run is useful before this detector gate is met.

## Latest (2026-07-18) — Texture Hop v10, bounded Standard and explicit Long
- Qualification contract v19 replaces Texture v9 with Texture Hop v10. TextureRop now performs
  denser dependent sampling and is exercised immediately through irregular multi-period load
  transitions; all workload fingerprints moved to `f2q-texhop-v10-r1`, so pre-v19 positives cannot
  unlock Apply.
- Standard now uses 30 s frontier qualification and 2 min Texture Hop + 5 min Endurance per exact
  pair. A 59-minute active-work watchdog reserves the final minute for reset/checkpoint, preserves
  completed learning and blocks every incomplete profile. Only explicitly selected Long may exceed
  one hour; it retains 60 s frontier + 5 min Texture Hop + 20 min Endurance.
- Fast no longer exists as a user-facing or provisional mode. Its old IPC variant remains only as a
  compatibility alias to Standard. The main forge-themed Command Deck now exposes the Standard/Long
  selector beside Forge GPU; a Full-Reset-armed Clean Run uses the Standard budget.
- Software validation passed (`gpu-stress` 16, core 102, service 404, UI production build). The
  decisive hardware gate is still pending: a clean Standard run must reject the known-bad
  `1800@869` neighborhood and converge toward the repeatedly trusted `1800@875` boundary in at most
  one hour. Those numbers are this GPU's regression oracle, not global blacklist/default values.

## Latest (2026-07-17) — organic reset handoff, full-gate power and clean Forge UX
- A successful Full Reset now arms a one-shot `clean` in the UI, so the next Forge starts
  organically without the operator having to remember the mode; the selector returns to Standard
  after that run completes. Active observations/profiles/Sentinel history are
  cleared; hardware-derived durable condemnations remain stored but are ignored as pre-run input by
  the run-scoped Clean Run ledger.
- Exact Apply now performs an energy-envelope screen after Texture v9. If p99/peak already exceeds
  the common 94%-of-cap publication ceiling, Endurance is skipped and the pair is removed only from
  this run's selection — no blacklist, condemnation or fake stability repair. Eligible pairs still
  require the complete Texture + Endurance proof.
- Selection/publication now use the worst p99 from the complete Texture + Endurance gate. The final
  converged stock window publishes `stock_power_p99_w`, enabling per-GPU efficiency-vs-stock in the
  profile UI. The progress panel is progress-only; profiles are responsive disclosures with concise
  target, voltage, maximum power and efficiency metrics.
- Workload composition remains contract v18. It is already TextureRop-first and adversarial
  Endurance-first; do not tighten weights speculatively before the required new hardware Clean Run.

## Latest (2026-07-16d) — profile-aware vertical closure + qualification contract v18
- Exact-Apply physical failures now close every viable higher same-clock bin; the arbitrary
  two-repair budget is gone. The durable condemnation view refreshes for every decision.
  Inconclusive/coverage failures remain incomplete without blacklist or voltage inference, and only
  an actual SilentError is persisted under the exact-Apply silent-error quarantine kind.
- The conservative 94%-of-cap publication ceiling is retained. Godforge can climb the physical
  voltage domain under that ceiling; Brokkr's stays one real bin below Godforge and Deep Calm one bin
  below the lowest stronger profile. Exhausted Godforge clocks fast-drop to the next real clock at
  the carried voltage, then receive fresh power calibration and the full gate.
- Qualification v18 is TextureRop-first Texture v9 plus a continuous 20-minute Endurance whose
  aggressive rejection tier runs first. Mandatory DX11 and standalone TransitionShock were removed,
  including DX11 golden capture; legacy evidence remains readable. A clean passing exact pair now
  costs 25 minutes of dwell instead of 38, while failures may reject earlier.
- Software validation passed (`gpu-stress` 16, core 102, service 403). Hardware validation is still
  pending and must be a new Clean Run; no Apply/Forge/hardware write was executed for this change.

## Latest (2026-07-16c) — manual pause/resume and complete operator-facing Forge status
- Manual Stop reaches `paused` only after stock reset and checkpoint persistence. Explicit
  `ResumePowerSweep` requires the exact program build, GPU/adapter and driver; it keeps the same
  run ID, reuses only compatible completed evidence from that run, retries incomplete work and keeps
  elapsed time cumulative. Ordinary Start and TDR/recovery paths never auto-resume.
- Structured current/next backend tasks drive a novice-facing timeline with live task/total timers
  and ETAs. Sentinel displays effective condemnations plus the separate operational blacklist; log
  red/white/green tone remains presentation-only.
- Live UI telemetry now includes VRAM clock/capacity/usage, NVAPI core voltage and averaged NVML fan
  duty (`nvidia-smi` fallback), preserving valid 0% and unavailable `None`. All three themes and the
  dashboard received responsive/wrapping refinements.
- The item-1/2 review recorded here is superseded by qualification v18 above; it remains historical
  rationale rather than current behavior.

## Latest (2026-07-16b) — modos de aprendizado: clean run experimental vs produção
- `StartPowerSweepClean` (IPC additivo) / modo UI "Clean run · Experimental": busca 100% orgânica
  para avaliar versões do algoritmo — arquiva observações/forge_state em `forge-archive/<run_id>/`,
  faz snapshot e remove regiões GPU V/F do `safe_loop.json` (Safe Mode/crashes/incidentes intactos)
  e lê o ledger durável só no escopo da run. Escritas no ledger global nunca param; falhas da
  própria run continuam bloqueando e orientando o reparo vertical. Produção = comportamento P0.
- A PRÓXIMA validação de hardware deve usar o modo clean run (obrigatório durante desenvolvimento).

## 2026-07-16 — condemnation ledger (P0) + vertical Apply repair (P1) implemented
- Root causes fixed: gate failures no longer kill the clock (vertical repair climbs +1/+2 real VF
  bins under the 94%-of-cap publication ceiling, budget 2/clock/run, full gate always re-runs), and
  hard failures now live in the append-only `condemnation_ledger.jsonl` that survives every reset
  (the 07-15 manual reset had wiped 1890@900's Endurance failure; it then passed a single ladder).
- Severity model: Rigid (TDR/crash/device-lost — at-or-below refused, manual rehab only) vs
  Quarantine (exact-Apply SilentError — strictly-below refused; re-attempt of the exact pair needs
  double full-gate proof, or a single pass under a stronger contract). Floor = safe_loop ∪ ledger
  at every confirmed preflight/descent/restore/Apply path. Dominance pre-gate: only gate-APPROVED
  points can veto a candidate before its ladder.
- Ledger seeded from run-log history (2 rigid: 1920@918 field TDR, 1845@856 CandidateCrash;
  10 quarantine pairs incl. 1890@900, 1905@893/900/906, 1920@912, 1875@875/881, 1860@868).
- Validation run pending (see handoff). Deferred: Godforge fast-drop (needs per-profile selection
  overrides), Texture v9 + Endurance front-load + DX11/TransitionShock removal (P2, contract v18).

## 2026-07-15 — durable restart incidents and field-failure feedback implemented
- Startup parity is closed: both installed-service and console paths reconcile a persisted running
  Forge and start the Event Log TDR sentinel. The reconciliation runs before normal Safe Loop startup
  recovery, so an armed exact candidate is not lost; unknown candidates stay unknown.
- `PowerSweepProgress` persists a run identity/sequence and active candidate throughout execution.
  Reconciled incidents survive reboot in `safe_loop.json`, force `needs_attention`, keep Start/Apply/
  boot-reapply at stock, and require explicit acknowledgement. The UI never auto-resumes them.
- Normal reset preserves the interrupted checkpoint and all durable learning. Full reset remains the
  deliberate clean-algorithm-evaluation path and removes checkpoint, blacklist and incident history.
- A profile card can be marked unstable after confirmed real use. The service derives the exact pair
  from the current profile, adds durable local blacklist evidence and invalidates the profile set;
  source policy contains no per-GPU blacklist constants. The operator confirmed `1845@862` repeatedly
  unstable in this session.
- Forge export is scoped to the ordered current run sequence and includes incidents. Dirty builds now
  carry a source-content suffix rather than the ambiguous plain `-dirty` identity.

## Latest (2026-07-15) — evidence-integrity rebuild + first HW cycle; restart attribution is P0
- The post-reset Standard hardware cycle spans two run IDs (127 observations, then 59 after resume).
  The resumed run completed in 175.1 min and published `1890@893`, `1845@862` and `1740@800` with
  qualification v16. All persisted rows report confirmed stock reset and boot-flag cleanup.
- Operator evidence supersedes the recorded-row summary: while descent was unattended, the PC was
  later found at the Windows login screen after a reported TDR/reboot. Reopening the app resumed Forge
  from its checkpoint. No row recorded `DeviceLost`/`tdr_or_crash` and no sentinel event survived the
  full-reset cycle, so the exact active candidate cannot be assigned. Treat the export's apparent
  zero-TDR result as incomplete.
- Open P0: an unexplained boot/run discontinuity must preserve the active intent, reconcile it into
  explicit crash evidence, stop at Needs Attention and require acknowledgement before Forge resumes.
  The exported log must filter/group by run ID and include reconciled incidents.
- The long gate caught stochastic failures that short phases missed: `1905@900` passed Endurance in
  the first run and failed in the resumed run; `1860@868` also failed Endurance. Conversely,
  `1845@862` passed the complete synthetic gate despite its historical in-game failure, so the new
  workload is not yet a sufficient proxy for deployability.
- All v16 rows identify the build only as `d449b63...-dirty`. This records dirty state but not the
  patch content; before the next evidence-compatible update, use a clean commit or embed a content
  hash so two different dirty builds cannot share provenance.
- The field trace did **not** prove that sustained BoostEdge/anchor-bin residence caused the game
  TDR: the lobby survived essentially the same external clock, voltage, utilization and power
  envelope. Contract v15's causal wording was wrong. The likely missing variable is workload/driver
  composition, so old positives are quarantined by qualification contract v16.
- Every new F2 dwell carries build revision/dirty state, workload fingerprint, actual wgpu backend,
  adapter/driver identity, checksum method and golden configuration. A discovery, qualification or
  exact-Apply positive is current only when reset-to-stock **and** boot-flag cleanup are proven.
- Forge normalizes stock first in bounded PowerRender windows and fails closed unless temperature
  and sustained p5 converge twice without thermal throttle. Ctable (static physical table), Cboost
  (post-preheat live top) and Cmax (first workload-proven sustainable clock) are now distinct facts
  in IPC/UI. Clock candidates require an index present in the static table and the normalized live
  curve.
- Power-cap policy is hysteretic: p99 >=99% is `NearCap`, <=98% is `OffCap`, and the middle band is
  `Ambiguous`. Ambiguous data repeats up to the bounded p99 budget and remains inconclusive if it
  never resolves; a numeric limit always outranks sampled cap flags.
- Discovery now owns each usable final attempt as one Candidate Transaction: select/precheck,
  Safe Loop arm, curve apply/verify, PowerDiscovery and immediate boundary qualification share the
  same applied curve. Bounded p99 rechecks close cleanly between attempts; the decisive off-cap
  attempt transitions to qualification without reapply, then performs one reset/boot-flag cleanup.
  Qualification records are persisted before discovery positives, and no positive is exposed if
  reset, boot-flag clear, blacklist persistence or observation persistence fails.
- MixedGame now records BoostEdge + TextureRop + PowerRender in one encoder/frame/submit and rotates
  the checked final output. BoostEdge/MixedGame reduce and compare on-GPU every 16 frames instead of
  serializing every light frame; checksum mismatch evidence remains accumulated.
- Sentinel canary execution is owned synchronously by its dedicated thread; the false 3 s
  "pre-hang before a 2 s watchdog" claim and detached worker were removed. Canary/Event Log recovery
  ownership is atomically deduplicated. Its TextureRop reference remains execution-local, so it is an
  auxiliary consistency detector, not a stock-golden or game-correctness oracle.
- Qualification v17 adds a native offscreen Direct3D 11 gate only at exact Apply. Its checksum golden
  is captured at stock on the explicitly selected NVIDIA adapter; candidate runs must match the same
  adapter LUID and pass bounded completion/readback checks. The deployability order is now Texture
  5 min + DX11 5 min + TransitionShock 8 min + Endurance 20 min. No duration was shortened.
- `1845 MHz @ 862 mV` remains historical field evidence for this GPU, but the intentional full reset
  removed the old blacklist before evaluating the new method. The clean learning cycle re-published
  that exact pair, which makes it the primary v17 DX11 discriminator against known-safe controls.

## Latest (2026-07-06, late) — v12: regime lift replaces reconciliation exclusion (code-complete)
- v11 run data: detect-before-TDR worked (zero TDRs, texture-rop silent errors as graceful killer,
  frontier monotonic except 1875@912 outlier) — but exact-Apply ran only 3 patterns (hardcoded
  `final_gate_passes=3`; a passed 15-min soak at 1875@925 was refused for missing Memory evidence;
  FIXED to `REQUIRED_QUALIFICATION_PATTERNS.len()`), and reconciliation excluded 11/13 again.
- Key insight from the log: the reconciliation's required voltages were RIGHT — lifting 1800's
  Apply to the 1830-regime requirement = 875 mV, the user's hand-validated daily driver. v12:
  `apply_f2_margin_policy` records `base_apply_mv` (new additive field) and LIFTS the Apply bin to
  the sustained regime's requirement computed from base applies (no cascade possible); the strict
  reconciliation stays as a fail-closed net computed from bases. Lift runs before the p99 backfill.
  Top-of-frontier without measured regime still excluded (future: direct regime dwell).
- Tests 488/0. Pending: safety audit (v11 A2/A3/C1 + v12 gate change) before commit; HW rerun
  should now ship 3 profiles with regime-priced voltages.

## Earlier (2026-07-06) — v11 engine hardening: detect-before-TDR (code-complete, NOT HW-tested)
- v10 field result: silent errors vanished, straight TDRs appeared (1920@906 crash), Discord voice
  froze per burst — the memory-latency texture REPLACED the graceful detector and rendered giant
  non-preemptible draws. v11: TextureRop reverted to L2-resident (graceful detector back);
  scattered VRAM sampling → new banded `TextureStream` phase (16 scissor bands, 1 submit each =
  preemptible + per-band timing); band > 500 ms or NVML-starvation `prehang_stall_detected` or
  sustained frame time > 2× stock reference ⇒ new `StabilityResult::Unstable` verdict BEFORE the
  driver watchdog; severity ladder (hang-prone detectors last); crash-proximity margin in core
  synthesis (boundary ≥ 12 mV ≈ 2 bins above any crash anchor — user's "TDR taints the bin above"
  insight, generalized). Contract v11. Tests 487/0.
- Pending: safety audit of A2/A3/C1 verdict semantics before commit; HW gate per handoff.
- **Next agreed (waiting on v11 run data)**: regime confirmation — for each selected profile pair
  (T, V_apply), before exact-Apply, re-anchor at the measured sustained clock (S′, V_apply) and
  run the 4×60 s set; failure lifts that clock's boundary to the next validated descent bin and
  resynthesizes (no cascade). Replaces the cross-target p95 reconciliation. Full design in
  `docs/qualification-v8-plan.md` §"Confirmação de regime". Contract v12 + safety audit.

## Earlier (2026-07-05, late) — 1.7 texture-stream + 1.8 upward recovery implemented (contract v10)
- 1.7: TextureRop samples a FIXED 8192² (256 MB) VRAM-resident GPU-filled source with per-pixel
  scattered tap chains — TMU + memory controller concurrent, cache-defeating. Size deliberately
  not runtime-probed (golden/qualifier determinism across GpuCtx instances). Contract v10: v9
  positives (proven false negatives) cannot unlock Apply.
- 1.8: `warm_start_rejected`/`sustainable` now require a FULL-set qualified bin (single-pattern
  pass no longer masks rejection); outer ladder climbs +1 physical bin per retry (max 4, generic
  constant) on start-bin QualificationRejected — ClockDrop is never retried. User's GPU data
  stays validation-gate-only, never in code.
- Validation: 486/0 tests. HW gate: reject 1815@856/1860@875-class points; 1800 boundary must
  land near the known ~875 via climb. Watch TextureRop frame times (memory-bound, longer).

## Earlier (2026-07-05) — HW run verdict: v8 still ~4-5 bins optimistic; ground truth recorded; IPC freeze fixed
- **Ground truth (RTX 3060 Ti, user's manual long-term validation)**: 1800@875 mV stable (daily
  driver); 1800@868 UNSTABLE in game; 1830@875 UNSTABLE in game. The 2026-07-05 v8 run approved
  1815@843 and 1860@875 (4×60 s golden) → false negatives; measured fidelity gap ≈ 4–5 bins.
  Full regression table in `docs/qualification-v8-plan.md`.
- **Run findings**: ALL v8 failures fired in `texture-rop` (TMU is this chip's sensitive path) —
  but the source texture is 1024² ≈ L2-resident; games sample GBs with constant cache misses.
  The strict p95 regime reconciliation cascaded (delivered clock runs +1–2 bins above the anchored
  plateau — NVIDIA temp compensation) and excluded ALL candidates → zero profiles, which this time
  CORRECTLY blocked bad profiles. Do NOT relax it before fidelity closes. Warm-start entered 1800
  below the real boundary and the clock was discarded instead of climbing up — lost the user's
  best-known point.
- **Approved next (Fase 1.7–1.9, priority over Fases 2–3)**: 1.7 texture path sampling from a
  LARGE VRAM-resident texture set (512 MB–1 GB, OOM-guarded) with cache-defeating dependent UVs —
  TMU+DRAM concurrent in the same shader; 1.8 upward recovery when a clock's starting bin fails
  qualification (~4 climbs before discarding); 1.9 physical gate against the regression table,
  then calibrate Fase 2.1 margins with the residual gap. User philosophy: failure-seeking — force
  the error so margin remains.
- **IPC freeze fixed**: `ConnectNamedPipe` returning `ERROR_PIPE_CONNECTED` (0x80070217, client
  connected between create/connect) was treated as fatal and leaked the instance without closing
  the handle — the connected UI waited forever (the observed hang). Now treated as success;
  other connect errors close the handle. `ipc_server.rs`; service tests 355/0.

## Earlier (2026-07-05) — Phase 1 complete: full v8 workload set, contract v9 (code-complete, NOT HW-tested)
- Motivation: v8 with FrameCadence alone still passed known-unstable points (user's manual tests).
- New: `VramPressure` (≤8×256 MB OOM-guarded tables, cache-defeating gathers, known-answer) and
  `GeometryDepth` (procedural instanced triangles + depth test, unique depths ⇒ deterministic,
  golden `RenderGoldens.geometry`). Patterns renamed V7→V8 and extended; NEW `V8Memory` pattern
  (VRAM-dominant, 11 phases). Qualification set = **4 patterns** (HighFps/Texture/Transitions/
  Memory), boundary 4×60 s, exact-Apply 4×5 min. Contract v9; completeness gates now index the
  canonical `REQUIRED_QUALIFICATION_PATTERNS` array in core. Item 1.5 = pure
  `qualification_failure_histogram()` (wiring deferred). Item 1.6 verified covered (MixedGame
  decomposes to golden-checked workloads; ComputeBurst known-answer).
- Validation: 485/0 workspace tests, clippy baseline. HW gate: 5 goldens capture, 4 patterns run,
  and the known-unstable points must now be rejected. Details in `handoff.md` + plan doc.

## Earlier (2026-07-04) — v8 item 1.1 implemented: FrameCadence phase
- New `VfWorkload::FrameCadence` + `VfQualifierPhase::FrameCadence` (code 8, `frame-cadence`):
  one heavy 1-instance RENDER_SHADER frame (~10-20 ms = a game frame) → poll(Wait) → idle gap
  cycling 2/4/6/8 ms — VRM droop-release transients at real frame cadence, the failure mode the
  750 ms idle pulses never exercised. Own stock golden (`RenderGoldens.cadence`; the 1-instance
  image differs from the 8-instance power golden) — four goldens are captured now.
- Segments inserted into the three qualification plans (HighFps ×2, Texture ×1, Transitions ×3).
  Coverage denominator became pattern-specific via `qualifier_expected_phases()` (FSGL 8, v7/v8
  plans 9) — the old fixed `[false; 8]` bitmap would panic on phase code 8. Skipping FrameCadence
  ⇒ Inconclusive. `F2_QUALIFICATION_CONTRACT_VERSION = 8` (pre-cadence positives can't unlock
  Apply). Discovery contract v4 untouched.
- Validation: cargo check clean; workspace tests 484/0; clippy = pre-existing baseline only. No
  hardware was touched. HW gate (manual): Forge Standard end-to-end, then re-test the known
  game-crashing point — v8 must reject it at the voltage v7 accepted. Details in `handoff.md`.

## Earlier (2026-07-04) — direction approved: qualification v8 plan
- Full plan in `docs/qualification-v8-plan.md`. Root cause of in-game crashes/TDR despite v7:
  (a) deterministic workload gap — no frame-cadence droop transients (idle pulses are 750 ms vs
  6–16 ms game cadence), no VRAM/memory-controller pressure (working set ~L2-resident), no
  vertex/geometry/depth units, no cold-start check; (b) statistical gap — Vmin is a rare-event
  rate over hours; no finite test proves it, only guard band + event-driven correction.
- Approved phases: **1** v8 workloads (FrameCadence, VramPressure, geometry/depth, v8 patterns,
  failure-phase telemetry, MixedGame golden coverage check) with a physical regression suite =
  points that pass v7 but crash in-game must be rejected by v8 at the same voltage; **2**
  per-profile apply margin in bins (Godforge +2, Brokkr +3, Deep Calm +3) + graceful degradation
  (`provisional` profiles instead of ZERO-profile runs); **3** cold-start verification on boot;
  **4** event-driven post-crash step-up (Safe Loop reacts to TDR/reboot events it already
  observes — nothing runs during gameplay), gated on Phase 1 results; **5** discovery efficiency
  (coarse-to-fine descent, cross-clock monotonic seeding, dominance pruning); **6** fleet priors.
- User preference recorded: try synthetic-fidelity improvements (Phase 1) before relying on the
  event-driven loop; Phase 4 stays as insurance for the statistical tail.

## Latest (2026-07-03) — stage-aware Forge ETA and conservative total ceiling
- `PowerSweepProgress` now carries additive/defaulted `estimated_total_upper_ms`,
  `cmax_clock_mhz`, `frontier_floor_clock_mhz` and `frontier_clock_count`. Legacy/restored payloads
  remain compatible and interrupted runs clear a stale upper estimate.
- The run clock starts before preflight/golden capture. Before Cmax, the upper total stays absent and
  the UI says `Refining`; once Cmax is confirmed the service publishes the exact inclusive
  Cmax→90% real-clock domain and refreshes its conservative clean-run ceiling after each target.
- The ceiling is phase-aware: every possible frontier candidate includes discovery plus current v7
  qualification, p99 calibration reserves up to three attempts per missing Apply bin, and final
  qualification reserves up to three unique Apply pairs at three five-minute patterns each.
  Calibration and Apply progress then reduce their remaining work live; terminal state preserves the
  measured elapsed total.
- Forge Progress shows human-readable stage, live remaining time, current estimated total,
  conservative maximum and elapsed time with tabular numerals. It never parses log copy for Cmax or
  tuning policy and shows `Refining` for payloads without the new ceiling.
- Validation was code-only: targeted core/service checks and tests plus the UI production build. No
  Forge, VF write, Apply or hardware stress was run.

## Latest (2026-07-03) — automated qualification v7, strict p95 regime and responsive Forge
- `F2_QUALIFICATION_CONTRACT_VERSION = 7`. Standard/Long and exact Apply now require three
  independent deterministic patterns: High-FPS, Texture and Transitions. Legacy FSGL1/2/3 positives
  remain readable but cannot unlock Apply.
- Discovery remains the homogeneous textured PowerRender and keeps confirmed sustained-p99 power
  calibration unchanged. The v7 patterns add rapid boost cadence, texture/ROP-heavy mixed graphics
  and repeated idle→burst→heavy transitions with stock-golden verification.
- P5 remains the performance floor; p95 is now the electrical support regime. Any sustained p95 above
  the configured target maps to the nearest measured target with zero one-bin tolerance, inherits the
  conservative Apply voltage across that span and requires current three-pattern evidence. Exact-Apply
  v7 p95 is reconciled again after the soak, so a higher regime discovered there forces re-synthesis.
- Live GPU loops receive the Forge cancellation token. Stop immediately reports `stopping`, checks
  cancellation between bounded frames/dispatches, records no positive/bad evidence from cancellation,
  and still executes the checked stock reset.
- Forge UI refreshes cannot overlap. While running, progress/safety keep the 500 ms cadence and
  secondary diagnostics move to 3 s; Stop changes optimistically to “Stopping…”. The IPC-visible log
  is capped at 240 lines while completed evidence remains in `f2_observations.jsonl`.
- Code/tests only in this checkpoint. No Forge, VF write, Apply or hardware stress was run
  automatically; the next gate is a supervised v7 Forge on the RTX 3060 Ti.

## Latest (2026-07-02) — profile synthesis reconciles the sustained electrical regime
- Profile candidates tolerate at most one 15 MHz physical bin between configured target and p5. A
  larger gap maps to the nearest measured target at/above p5 and inherits the conservative maximum
  Apply anchor across that target span. `1860@893` with p5 1890 is therefore excluded when the
  qualified 1890 regime requires 918 mV; synthesis uses the canonical support or a lower target.
- Standard/Long require current A+B boundary evidence for both the candidate and its p5 regime.
  A failed/inconclusive exact Apply also blocks every lower-anchor candidate that aliases that regime.
- Selected cards publish the larger sustained p99 measured by confirmed PowerRender calibration or
  the approved exact-Apply FSGL3 A+B pair. Frontier scoring stays PowerRender-homogeneous; restored
  qualified v6 snapshots refresh the conservative watt from the observation log.
- Boundary FSGL3 evidence no longer makes the post-margin point deployable. Standard/Long first
  synthesize provisional candidates, then run a five-minute FSGL3 A and five-minute FSGL3 B at each
  unique exact `(target_clock_mhz, vf_table_voltage_mv)` selected by the three profiles.
- `F2_QUALIFICATION_CONTRACT_VERSION = 6`; exact passes are persisted as distinct
  `ApplyQualification` evidence. Old/restored points default to unqualified and F2 Apply fails closed.
- Any `Inconclusive` creates debt for that pattern and requires two consecutive clean passes.
  Reset-clean rejection excludes only that target/Apply pair and re-synthesizes from remaining
  measured candidates; device/reset/write failures still abort.
- Clock p95 now flows beside target, average and p5 so the UI exposes the upper sustained boost regime
  actually exercised at Apply. Exact-Apply failures do not rewrite the lower-voltage frontier bracket.
- Code/tests only. The currently applied pre-v6 profile must be reset/re-forged before reuse; no
  hardware write, Forge or game validation was run during this implementation.

## Latest (2026-07-01) — adaptive F2 frontier search without weakening qualification
- A prior compatible discovery-v4 boundary for the same GPU/clock is the strongest start predictor.
  Otherwise the last 3–4 qualified clocks are projected with a non-increasing isotonic trend. The
  Forge starts one physical voltage bin above the prediction; predictions never count as evidence.
- Predictor inputs that disagree by more than 25 mV fall back to the existing sequential warm start.
  A compatible historical offset may bound the writer's cross-run +15 MHz progression, but the
  predicted bin still requires fresh PowerRender and FSGL3 evidence.
- While confirmed p99 remains at 99%+ of cap, the p5 deficit selects a stride of 4 bins (>=90 MHz),
  2 bins (45–89 MHz) or 1 bin. Every actual jump is additionally limited to 25 mV and the writer's
  existing positive-offset step cap.
- A reset-clean off-cap failure after a jump opens a safe/failed bracket and tests only upward
  midpoints. Once an off-cap point passes FSGL3 A+B (or PowerRender in provisional Fast), discovery
  returns to adjacent-bin descent and stops at the already-measured failure without retesting it.
- Apply-bin p99 backfill, p99 anomaly consensus, thermal invalidation, FSGL3/goldens, Safe Loop,
  qualification margins and the +12 mV Apply policy are unchanged. Hardware validation is pending.

## Latest (2026-07-01) — F2 frontier and profile calibration use confirmed sustained p99 power
- Discovery keeps the existing textured `PowerRender`; the compute-only `POWER_SHADER` remains
  outside the live F2 path. Mean, sustained p99 and the highest post-ramp sample remain distinct
  through `SingleDwell`, the F2 step report, append-only observations and `PowerSweepPoint`.
- `POWER_PEAK_PERCENTILE = 99`; p99 uses every retained post-ramp power sample. Fewer than 100 samples
  fall back explicitly to the measured raw maximum; zero samples produce no value and fail closed.
- `F2_DISCOVERY_CONTRACT_VERSION = 4`. v3 positive and power-bound evidence cannot seed the new
  frontier. Adjacent-bin p99 jumps larger than both 8 W and 5% in the same p5 regime repeat the exact
  bin up to three total attempts; two must agree and the highest measured p99 is retained. No
  consensus is neutral/ineligible, never interpolated.
- Profile synthesis first applies the unchanged +12 mV policy, then resolves a current reset-clean
  PowerRender observation for that exact apply bin. It scores Godforge/Brokkr's/Deep Calm with p99
  power and the p5 clock observed at the apply bin, not boundary-bin mean or a one-sample maximum.
  When cross-clock warm-start pruning skipped that exact target/apply pair, Forge fills only the
  missing power telemetry with supervised discovery-only PowerRender and the same v4 p99 consensus;
  the backfill itself does not qualify stability. The 2026-07-02 gate now runs FSGL3 separately.
- A discovery `ClockDrop` whose p99 remains at 99%+ of the numeric cap is `PowerBoundClockDrop` and
  continues voltage descent even after the clock previously sustained. It remains calibration
  telemetry, never stability evidence; off-cap `ClockDrop` retains the normal boundary behavior.
  `Validated` at cap also continues, and Standard/Long defer FSGL3 until confirmed p99 is off-cap.
  NVML software/hardware thermal slowdown makes a discovery dwell `Inconclusive`, not a bad undervolt.
- UI cards and Forge Progress show sustained p99 with an explicit “not a hard power limit” caveat;
  raw mean/maximum remain available. Apply rejects restored F2 profiles without valid p99.
  Qualification v4 was current for this checkpoint; later contracts supersede its deployability rule.
- Hardware on 2026-07-01 confirmed p99 kept a 1950 MHz descent moving through power-bound clock
  drops while the cap stayed near 200 W. The first reset-clean FSGL3 rejection exposed a ladder
  control bug: `completed = false` stopped every lower clock. It now completes only that target and
  continues toward the real qualified Cmax; FSGL3/p5 policy itself is unchanged. Hardware rerun pending.

## Latest (2026-06-30) — F2 margin boundary, continuity and supervised recovery
- FSGL3 qualification now derives a like-for-like heavy-phase p5 signal per A/B pattern. A candidate
  becomes `ClockDrop` when that p5 falls more than `MARGIN_DROP_TOL_MHZ = 30` below the median of at
  least two prior stable candidates at the same clock/pattern, or more than 30 MHz below target.
- `Inconclusive` gets `INCONCLUSIVE_RETRY_BUDGET = 2` retries at the same point; retries use a 1.5×
  dwell. Exhaustion skips only the current clock and never becomes a global Forge abort. Hard device,
  reset, arm, apply, verify and persistence failures remain fail-closed.
- `finished` is now reserved for a complete frontier with qualified profiles. Complete Fast results
  are `provisional`; partial safe endings are `incomplete`; retained recovery is `interrupted`.
- Safe Loop classifies VIDEO TDR 0x116/0x117 as OC instability. An exact
  `f2_undervolt_probe` TDR/Unknown recovery blacklists and recedes without consuming the normal-use
  Safe Mode crash budget; unrelated crashes and non-Forge phases still count. DeviceLost is accounted
  once at startup, and duplicate blacklist regions are not appended.
- An interrupted Forge automatically performs the existing non-destructive Reset+Start sequence once
  when the UI reconnects, using the persisted original mode. Manual Stop does not create an
  auto-resume state; F2 observations remain the resume source.
- Apply policy requests `APPLY_MARGIN_MV = 12`, snaps upward to the first exact valid physical VF bin
  and clamps to the highest valid anchor. `boundary_voltage_mv` and `apply_margin_mv` are additive IPC
  fields; `vf_table_voltage_mv` remains the exact applied bin.
- `PREHANG_STALL_MS = 300` is telemetry-only in this Leva. Proactive reset remains disabled until the
  hardware gate validates signal precision and a cooperative stress cancellation path exists.
- No hardware Forge, VF write, Apply or reboot was run during implementation. Leva 2 remains blocked
  on the supervised hardware gate.

## Latest (2026-06-30) — F2 FSGL3 golden-sample qualifier
- Discovery remains the proven steady `PowerRender`; it only measures/characterizes power,
  p5-clock, cap behavior and `ClockDrop`. It no longer decides deployable stability.
- Before Standard/Long descent, stock captures one deterministic REDUCE3 golden for each render
  configuration (power, boost and texture/ROP), using a fresh `GpuCtx` per configuration. Any stock
  divergence or device loss aborts Forge; goldens are session-only and are not persisted.
- FSGL3 A/B is now the interleaved per-bin qualifier. It biases TextureRop/MixedGame, introduces
  short six-frame/4 ms droop bursts and compares every rendered frame on-GPU against the stock
  golden. FSGL1/FSGL2 remain available with their previous self-reference/250 ms behavior.
- `F2_QUALIFICATION_CONTRACT_VERSION = 4`. Apply counts only current-contract FSGL3 `Pass` evidence
  with distinct A+B patterns; FSGL1, FSGL2, discovery and old-contract positives remain provisional.
- If FSGL3 rejects a candidate, the service records that bin as unstable and keeps the last
  FSGL3-qualified physical bin as the accepted boundary. `Inconclusive` retries once and then blocks
  Apply without marking the bin bad.
- Standard/Long no longer qualify an old `prior_good` directly. Previous positives can guide resume,
  but a deployable boundary must be rediscovered by the current run before qualification begins.
- `ResetGpuTuning` is now an explicit recovery path outside the normal start/apply lease: after a
  TDR/interrupted Forge it can stop marked-running work, reset to stock, clear Safe Loop, and release
  the Forge handle. It also clears the visible `forge_state.json` checkpoint so the UI can start from
  an idle run state again, without deleting automatic F2 observation history.
- UI recovery now separates the normal path from destructive reset: post-TDR Needs Attention /
  Interrupted offers **Recover & continue** (ResetGpuTuning, then selected StartPowerSweep mode,
  preserving F2 observations) and a clearly separate **Full reset** for `ResetGpuTuningFull`.
- No IPC or frontend payload changed. No hardware Forge was run. Before the first FSGL3 run, clear
  persisted Forge state so FSGL2 floors cannot seed the trial; then verify the known 1920 MHz @ 912 mV
  and 1935 MHz @ 918 mV failure points under supervision.

## Latest (2026-06-28) — F2 qualification refinement
- Fast traverses the full physical frontier with 10 s discovery dwells but remains provisional;
  frontend and backend block F2 Apply until `profiles_qualified`.
- Standard qualifies each discovered boundary with 2 independent 60 s reset/reapply passes; Long
  uses 3×120 s. A failed qualification backs off one physical VF bin and restarts all passes.
- Frontier coverage now reaches 90% Cmax so Deep Calm has measured candidates matching its policy.
- Each lower clock starts one real VF bin above the previous minimum stable anchor; the previous
  power-bound ClockDrop is retained as fallback if that optimized warm-start is rejected.
- No hardware Forge was run for this implementation. Next step is supervised Fast/Standard QA.

## Latest (2026-06-28) — F2 durability, cross-clock reuse and live progress
- Real Fast Forge evidence proved learning was durable (72 JSONL observations) but partial progress
  was not restored visibly, and reset-clean SilentErrors incorrectly polluted the crash counter.
- Fixed crash semantics, durable partial `forge_state` checkpoints, structured ETA/progress fields,
  permanent Technical Power Sweep log, and conservative cross-clock voltage reuse.
- Deployable profiles require the complete Cmax→90% frontier plus Standard/Long qualification; partial learning remains available
  for resume and previous complete profiles are retained.
- Do not run another supervised hardware Forge automatically. Next manual validation should confirm
  1905 MHz is no longer refused after SilentError, the next clock starts near the prior boundary, and
  UI progress/log update every dwell.

## Latest (2026-06-28) — F2 integrated frontier corrected — code-complete, hardware checkpoint pending
- **Clock discovery**: the live Forge resets to stock, then starts at the highest real live-VF clock bin (1950 MHz on the
  current RTX 3060 Ti table), not a short preselected list. A pre-sustain `ClockDrop` at 99–100% of
  the numeric power cap keeps the same clock and lowers voltage; once off-cap it moves to the next
  real clock. The first reset-clean sustained target becomes Cmax.
- **Unlimited voltage discovery**: autonomous target/ladder/live Forge paths have no arbitrary 3- or
  6-step cap. They walk every physically valid VF anchor until the first silent error, instability,
  sustained-clock drop after the target has held, device loss/TDR, reset failure, cancellation, or
  the hardware floor. Explicit `--steps N` remains an operator-selected manual boundary only.
- **Complete frontier**: after Cmax, every real clock bin down through 90% of Cmax is characterized.
  Profiles are synthesized only after that complete Cmax→90% frontier exists; partial,
  cancelled, or safety-aborted runs leave the last good forge snapshot untouched.
- **Modes are evidence, not breadth**: Fast / Standard / Long traverse the same complete frontier.
  Fast = provisional 10 s discovery; Standard = 10 s discovery + 2×60 s qualification; Long =
  10 s discovery + 3×120 s qualification. Confidence comes from real dwell duration, sample count,
  and repeated evidence.
- **Learning and resume**: every candidate is appended immediately to `f2_observations.jsonl`,
  scoped by NVML GPU UUID. A restart resumes below the deepest reset-clean observation or reuses an
  existing good/bad bracket; later failures invalidate stale equal/deeper validations.
- **Safety closeout**: service-wide IPC GPU lease; arm failures stop before writes; modern VF reset is
  write/readback checked; startup recovery retains the boot flag until reapply accounting; direct
  `DeviceLost`/unconfirmed reset aborts the Forge and keeps recovery armed. F2 cap evidence remains
  diagnostic and does not make an actually sustained F2 point ineligible for profile synthesis.
- **Validation**: `cargo check --workspace`; core 64 / NVAPI 40 / service 309 tests; `git diff --check`
  clean. Read-only 1950 MHz auto-sweep dry-run now reports stock ceiling 1950 and 83 physical anchors
  (previously failed at legacy 1920/+15). No confirmed Forge, VF write, TDR attempt, apply, reboot, or
  other hardware execution was performed. Next action is the explicit supervised hardware checkpoint.

## Latest (2026-06-27) — F2 Phase 2 Apply contract closed — implemented and validated
- **Backend**: `ApplyPower*` routes F2 profiles to the anchored-undervolt writer and preserves legacy F1.
  Apply is Safe-Loop armed, verified, persisted, reapplied on boot, reversible, and fail-closed.
- **Frontend**: F2 profile Apply is enabled; Discovered yields to Active after success. Matching uses the
  deterministic target/anchor carried in the existing `GpuApplyStatus.core` point.
- **Contract evidence**: `PowerSweepPoint.confidence` / `validation_count` and
  `PowerSweepProgress.power_bound_collapse` are structured, backward-compatible payload fields.
- **Safety closeout**: a memory-offset failure after the F2 core write now resets the GPU to stock before
  returning an error. Apply/reapply also requires the exact validated VF anchor bin; a changed table fails
  closed instead of silently selecting a deeper undervolt. The legacy read-only F1 curve verifier reports
  F2 profiles as metadata-only.
- **Hardware status**: code/tests only. No Apply click, VF write, `--confirm`, reboot, or hardware run was
  performed during this closeout; one supervised manual apply remains the next operational validation.
- **Validation**: workspace `cargo check`; core 61 / NVAPI 38 / service 300 tests; UI production build;
  clippy completed with no new warnings (repository baseline warnings remain); `git diff --check` clean.

## Earlier (2026-06-27) — Forge mode split-button dropdown — implemented, committed via 9119eec
- **Frontend only**: Forge GPU / Refine Profiles is now a compact split action. The main segment
  starts the selected mode; the mode segment opens a product-styled Fast / Standard / Long dropdown.
  Standard is the initial default; modes map to `StartPowerSweepFast`, `StartPowerSweep`, and
  `StartPowerSweepLong`.
- **UX/safety**: each mode explains depth, relative duration, and confidence behavior. All copy keeps
  the run supervised and fail-closed, states that nothing is auto-applied, and leaves profile apply
  as a separate confirm-in-game step. The dropdown uses the existing forge tokens and closes on
  selection, outside click/focus, or Escape.
- **Compatibility**: stop/progress/apply paths are unchanged; the UI does not parse `note` or `log`
  for mode logic. Files: `Forge.svelte`, `RecommendedAction.svelte`, UI contract.
- **Validation**: `npm.cmd run build` and `git diff --check` passed; committed via `9119eec`.

## Earlier (2026-06-26) — multi-clock/confidence UI contract pass — implemented, pushed via e60a6f7
- **Frontend only**: Forge profile cards/progress now distinguish target vs measured/p5 clock, label
  the deterministic VF bin separately, show all 3 profile points, and surface optional Wilson
  confidence + exact-point confirmation count when the backend provides them.
- **Honest collapse**: structured `power_bound_collapse` is preferred; identical Godforge/Brokkr's
  points remain a backward-compatible fallback. The UI explicitly refuses to invent a difference.
- **IPC blocker documented**: `docs/contracts/ui-backend.md` requests optional `confidence`,
  `validation_count`, `power_bound_collapse`, and an additive start request carrying bounded
  `validation_passes`. "Build confidence now" remains unsurfaced until it can be functional.
- **Scope/validation**: no backend/Rust/IPC implementation changed. `npm.cmd run build` and
  `git diff --check` passed; merged and pushed via `e60a6f7`.

## Earlier (2026-06-23) — F2 multi-clock profile package (Brokkr's 0.95 + descending ladder + confidence opt-in) — implemented, NOT committed
- **What**: 3 approved backend changes toward the v0.5 multi-clock profile frontier. Implemented + validated +
  safety-audited (GO). No hardware run; NOT committed (awaiting operator approval).
- **Margin answer**: applied 906 vs reached 868 is the **Wilson confidence gate** (0.85), NOT a voltage margin —
  `synthesize_forge_profiles` selection is voltage-agnostic; a once-validated point (~0.21 confidence) is filtered
  until it earns repeat confirmations.
- **Part 1**: Brokkr's floor 0.98→0.95 (`ForgePolicy::balanced`; Deep Calm 0.90, gate 0.85). Selection-only.
- **Part 2 (Caminho B)**: `ladder_target_descent_bounds` — descending ladder starts each lower clock at the prior
  clock's last-good (ceiling) with the base floor; ascending unchanged.
- **Part 3**: `--validation-passes N` (default 1, cap 20) — opt-in re-validates the deepest point N-1 extra times in
  one session to earn confidence WITHOUT lowering the gate; default = no-op; idle auto-validation = future.
- **UI**: contract for Codex in `docs/contracts/ui-backend.md` (multi-clock profiles, 95% Brokkr's, honest collapse,
  confidence-gate messaging, "Build confidence now" opt-in default OFF, idle future).
- **Validation**: nvapi 38 / core 59 / service 292 pass; clippy clean; safety audit GO (8/8). No apply/persist/promote,
  no hardware, no commit. Observation store unchanged (8 records / last_good 962 mV).
- **Next**: operator approves → commit/push; then a supervised confirmed descending ladder to build the frontier.

## Earlier (2026-06-22) — F2 LEARNED OFFSET HORIZON implemented (+210 abs / +15 step); HW run HELD
- **What**: target-sweep-specific progressive absolute-offset horizon. Commit `c40a78d`
  (`feat(service): add f2 target sweep learned offset horizon`) + docs commit, pushed to `origin/master`.
- **Change**: gpu-nvapi `TARGET_SWEEP_HORIZON_MAX_MHZ = +210` + `PositiveOffsetLimits::target_sweep_learning_horizon`
  (abs +210, per-step STILL +15 — unlike `manual_prior` which widens both). Only `--auto-sweep` uses it;
  default/ladder/manual-prior keep `conservative` (+30/+15). The +210 is reachable ONLY via validated chained +15
  steps. NOT a global cap widening. 8 new tests.
- **Validation**: cargo check clean; gpu-nvapi 38 / core 59 / service 284 tests pass; clippy zero new warnings;
  independent safety audit **GO** (all 11 PASS — no unsafe clock/floor bypass; no single +210 jump; confirmed
  sweep still bounded by `F2_CONFIRMED_MAX_STEPS`=3; no profile persist).
- **MATERIAL FINDING**: today's live curve has 3 bins within +30 at the top (981/975/968), so a confirmed run
  (cap 3, descent restarts from the curve top) reaches only **968 mV** — shallower than the 962 frontier — and
  would NOT advance discovery. The +30 cap is NOT today's binding limit; the 3-step budget + descent-start is.
  The horizon correctly unblocks the PLANNER (dry-run plans 962/+45, 956/+45, 950/+60) but the confirmed run
  can't reach those without resuming the descent START near the baseline.
- **Decision — HW run HELD** (operator choice): no `--confirm`. A TDR-risk run that only re-validates known-good
  points without advancing the frontier is poor value. Observation store still 8 records / `last_good 962 mV` /
  `first_bad None`; no profile apply/persist/promotion; no Safe Loop change. Tree clean.
- **Next**: scoped, separately-reviewed follow-up so the confirmed sweep RESUMES ITS DESCENT START near the
  validated baseline (deep candidates then fall within the 3-step budget) → one supervised run advances the
  frontier. Alt: bounded LADDER over 1815/1830.

## Earlier (2026-06-22) — F2 1800 MHz second confirmed chained run; frontier saturated at +30 cap — PASS
- **What**: third confirmed official target sweep `undervolt-probe --target-mhz 1800 --auto-sweep --confirm`
  at HEAD `01b97ca` (no code change — hardware validation only). One confirmed command, operator present.
- **Result — PASS** (exit 0): **3/3 Validated**, `CompletedAllPlanned`. #1 981/+15 (1815/1815, 191 W),
  #2 975/+15 (1803/1800, 198 W), #3 968/+30 (1815/1815, 193 W). All reset + boot-flag cleared; no TDR/crash/
  DeviceLost/Unstable/ClockDrop. `first_bad None`, frontier updated, ended safe.
- **Key finding**: the 1800 MHz conservative sweep is **absolute-cap-bounded** at +30. This session's VF read
  sat higher (boost top 1935), so the deepest reachable bin was 968 mV/+30 (next needs +45 → fail-closed). The
  chained baseline relaxes only the per-step cap, never the absolute cap, so `last_good` stays **962 mV** (the
  prior run's deeper point). Re-running 1800 only adds confidence — the frontier is at its conservative floor.
- **Cleanup all correct**: `gpu_applied.json`/`boot_flag.json` absent; forge_state/gpu_knowledge/heartbeat/
  safe_loop byte-identical (no persist/apply/promote, no new blacklist); `f2_observations.jsonl` 5→8 (7
  validated + 1 preserved abort). git clean.
- **Next**: pivot to a bounded multi-target LADDER (1815/1830) for the multi-clock frontier — supervised, one
  confirmed run at a time — rather than re-running the saturated 1800 sweep.

## Earlier (2026-06-22) — F2 CHAINED DESCENT refinement + first FULL-descent HW run (1800 @ 962 mV) — PASS
- **What**: implemented observation-aware chained same-target descent (commit `fcdf04d`), then ran the first
  confirmed sweep with it: `undervolt-probe --target-mhz 1800 --auto-sweep --confirm`. One confirmed command,
  operator present, no second run.
- **Fix**: the confirmed motor bounds each candidate's per-step increase against the LAST VALIDATED offset
  (prior candidate this run, only reached after it validated; or the deepest prior validated same-target/
  same-GPU observation for candidate 0; 0 when none) instead of stock +0. The absolute +30 cap still bounds
  each candidate. `validated_descent_baseline` (core) + `chained_prev_offset` (service); gpu-nvapi writer,
  `apply_vf_ceiling_monotone`, verifier, and manual-prior (+250) cap UNCHANGED. A no-write `AbortedBySafetyGate`
  record is never a baseline/first_bad/blacklist, so the prior 968/+30 abort does not block replanning.
- **Result — PASS** (exit 0): **3/3 Validated**, `CompletedAllPlanned`. #1 975/+15 (avg 1803/p5 1770, 198 W),
  #2 968/+15 (1800/1800, 190 W), #3 **962/+30** (1800/1800, 191 W) — the +30 point that aborted in the
  PASS-PARTIAL run now validates. New min stable voltage **962 mV** (was 975), `first_bad None`, frontier
  updated. No TDR/DeviceLost/Unstable/ClockDrop.
- **Cleanup all correct**: reset + boot-flag cleared for all 3; `gpu_applied.json`/`boot_flag.json` absent
  after; forge_state/gpu_knowledge/heartbeat/safe_loop byte-identical (no persist/apply/promote, no new
  blacklist). `f2_observations.jsonl` 2→5 (prior 2 incl. old abort preserved). git clean. Tests: core 59,
  service 282, gpu-nvapi 33 — all green.

## Earlier (2026-06-22) — F2 OFFICIAL target sweep FIRST HARDWARE RUN (1800 @ 975 mV) — PASS-PARTIAL
- **What**: first bounded hardware run of the OFFICIAL F2 target sweep (progressive anchored descent, NOT
  manual-prior): `undervolt-probe --target-mhz 1800 --auto-sweep --confirm` at HEAD `8dbd296`. One confirmed
  command, operator present, no second run.
- **Result — PASS-PARTIAL** (exit 0): #1 **Validated** 975 mV / base 1785 / +15 → 1800; **RaiseVerified**;
  dwell **Stable** avg/p5 **1815 MHz**, **191 W**, no silent error. #2 **aborted_by_safety_gate** (planner
  per-step +30 > +15 cap; **no VF write**, `not_run`). `last_good=975 mV`, `first_bad=None`, frontier updated.
  No TDR/DeviceLost/Unstable/ClockDrop/reboot.
- **Cleanup all correct**: reset_to_stock + boot_flag_cleared true for both; `gpu_applied.json`/`boot_flag.json`
  absent after; forge_state/gpu_knowledge/heartbeat byte-identical; safe_loop unchanged (safe_mode=false). 2
  observations now in `f2_observations.jsonl` (first official observation file). No profile persisted/applied/
  promoted. git clean.
- **Algorithm insight (not changed this task)**: candidates restart from stock (+0); the +15 per-step cap
  makes only the base-within-+15 anchor (1785→+15) reachable, so deeper anchors self-abort and the 1800 sweep
  validates ONE point per run. Bracketing below 975 mV needs a planner that carries the prior validated offset
  forward (same-target descent) — a future reviewed refinement, not this run.

## Earlier (2026-06-22) — F2 discovery/learning algorithm IMPLEMENTED (not yet HW-validated)
- **What**: the four-block F2 discovery/learning algorithm. **Code + tests + docs only — no hardware, no
  `--confirm`, no VF write, no profile apply/persist/promote.** Commits `0df6179` (store + target sweep),
  `cb125b6` (ladder + learned frontier).
- **Block 1 (observation store)**: `crates/core/src/f2_observation.rs` — `F2Observation` + append-only JSONL
  `F2ObservationStore` at `default_data_dir()/f2_observations.jsonl` (learning data, NOT a profile). Pure
  queries: last_good (lowest validated), first_bad (highest failure), bracket (Vmin in (first_bad,
  last_good]), is_known_bad, learned_frontier.
- **Block 2 (target sweep)**: `undervolt-probe --auto-sweep` — autonomous same-target min-stable-voltage
  discovery via the OFFICIAL progressive anchored descent (conservative +30/+15 caps, NOT manual-prior);
  bounded by F2_CONFIRMED_MAX_STEPS; records one observation per candidate on --confirm only.
- **Block 3 (ladder sweep)**: `--ladder-sweep --targets a,b,c` — per-target sweeps in order; a lower
  target's last-good is a conservative FLOOR only (never assumed to hold a higher clock); stops the ladder
  on a safety failure (ResetFailed/crash). A normal bad candidate stops only that target.
- **Block 4 (learned frontier + bridge)**: `learned_frontier` → per-target `F2FrontierEntry`;
  `to_power_sweep_point` builds the canonical `(PowerSweepPoint, conf)`; `classify_f2_frontier_summary`
  runs the EXISTING `synthesize_forge_profiles` (balanced) READ-ONLY to preview Godforge/Brokkr's/Deep
  Calm — no new scoring, nothing applied/persisted/promoted.
- **Untouched**: default progressive + manual-prior; F1/build-frontier; apply_vf_ceiling_monotone; Safe
  Loop; reset_to_stock; verifier; synthesize_forge_profiles. v1 GPU-only; CPU/RAM/UI deferred. Instability
  that resets clean is learning data, not a safety failure.
- **Validated (no HW)**: core 56/0, service 278/0, nvapi 33/0; clippy clean; dry-runs write nothing
  (`f2_observations.jsonl`/`boot_flag.json`/`gpu_applied.json` absent). **NEXT**: first bounded hardware
  run of the official target sweep — `undervolt-probe --target-mhz 1800 --auto-sweep --confirm` (operator
  present); not another manual validation.

## Earlier (2026-06-21) — F2 MANUAL-PRIOR anchor mode HARDWARE VALIDATED (1800 @ 875 mV, +210) — PASS
- **What**: opt-in `--manual-prior` for `undervolt-probe` — anchor at an explicit `--start-mv` with a
  SEPARATE larger bounded offset cap, to validate a KNOWN point fast (`1800 MHz @ 875 mV`). NOT the
  default, NOT for unknown GPUs. **Code + tests + docs only — no hardware, no `--confirm`, no VF write.**
- **Default unchanged**: progressive anchored descent + conservative caps (+30/+15) remain the official
  unknown-GPU path; manual-prior branches BEFORE the default dispatch (gated on `args.manual_prior`).
- **Cap**: `F2_MANUAL_PRIOR_MAX_POSITIVE_OFFSET_MHZ = 250` (default +30 untouched); fail-closed (offset
  above cap REFUSED, never clamped; stock clock ceiling still caps effective clock). Gate: `--manual-prior`
  requires `--start-mv`; confirmed requires `--steps 1`; reuses `run_confirmed_f2_step`/`RealF2Ops` with
  manual limits; one candidate; no persist/apply/promote. F1/`apply_vf_ceiling_monotone`/Safe Loop/reset/
  verifier untouched.
- **Dry-run `1800 @ 875`**: selected 875 mV, base 1590 MHz, required +210 MHz, cap +250, within bounds,
  AnchoredRaiseVerified, no-op/no-write. Default `1800 --steps 3` unchanged (975/968/962). 269 service + 33
  nvapi tests pass; manual safety review no blockers.
- **HARDWARE PASS (one confirmed run, operator present)**: `undervolt-probe --target-mhz 1800 --start-mv
  875 --steps 1 --manual-prior --confirm` → exit 0, **Validated**. Anchor 875 mV / base 1590 / +210 → 1800;
  **AnchoredRaiseVerified**; dwell **Stable** avg/p5 **1815 MHz**, **157 W** (~26 W under the 975 mV/183 W
  run — same clock, lower voltage); reset_to_stock OK (all bins cleared); boot flag cleared; not
  blacklisted; **no persist/apply/promote** (`last_validated` null). No TDR/crash/reboot; `safe_loop.json`
  byte-identical (mtime-only); `boot_flag.json`/`gpu_applied.json` absent. Impl commit `34581d0`.
- **NEXT**: clocks above 1800 at 875 mV NOT assumed (discover progressively). Either descend below 875 mV
  for 1800 (min stable voltage) or progressive discovery for 1815+. No second confirmed run made.

## Earlier (2026-06-21) — F2 ANCHORED multi-step descent IMPLEMENTED (not yet HW-validated)
- **What**: bounded SAME-TARGET ANCHORED multi-step descent for `undervolt-probe`. `--steps 2..=3` (anchored)
  runs a short sequence of anchored candidates at ONE target, safer/higher voltage → lower voltage, stopping
  at the first non-stable candidate and keeping the last good point. **Code + tests + docs only — no hardware,
  no `--confirm`, no VF write, no Safe Loop mutation outside tests.** Files: `gpu_undervolt.rs` (+ 12 tests),
  `main.rs` (doc comment only).
- **Cap**: `F2_CONFIRMED_MAX_STEPS = 3`, enforced by `confirmed_f2_multi_refusal` (`--steps` 1..=3 else fail
  closed). `--steps 1` keeps the validated single-step path; `--simple` stays single-step.
- **Design**: `plan_anchored_undervolt_descent` (anchored analog of `plan_undervolt_probe`, chains the +15
  per-step cap, stops at first rejection) → `run_confirmed_f2_multi_step` drives the SAME validated
  `run_confirmed_f2_step` motor per candidate via the `F2MultiStepOps` cursor trait (`select(i)` re-checks
  Safe Loop + blacklist before each write). Continues only on stable `Validated`; stops on VerifierFailed/
  Unstable/DeviceLost/ClockDrop/ResetFailed/Blacklisted. New `F2DwellOutcome::ClockDrop` (p5 < target − 30 MHz
  on an otherwise-stable dwell) — additive; single-step Stable/Unstable/DeviceLost unchanged.
- **Validated (no HW)**: 256 service + 33 nvapi tests pass (incl. F1/build-frontier + single-step). Dry-run
  `--target-mhz 1800 --steps 3` → 3 candidates (975 mV +15 → 968 mV +30 → 962 mV +30, stop=budget), preflight
  OK, no-op line; `--help`/`--steps 1` unchanged. **NEXT HW (one confirmed run, operator, stop after first
  non-stable)**: `undervolt-probe --target-mhz 1800 --steps 3 --confirm`. No persist/apply/promote.

## Earlier (2026-06-21) — F2 ANCHORED undervolt FIRST confirmed hardware validation (1800 MHz @ 975 mV, +15) — PASS
- **One supervised confirmed run** (operator present, ONE confirmed command, no second) of the `747a11b` anchored
  branch: `undervolt-probe --target-mhz 1800 --steps 1 --confirm`. **First real ANCHORED positive-offset VF write.**
  HEAD = origin/master = `747a11b`, tree clean; fresh worktree binary built first (was absent; mtime > `747a11b`).
- **Preflight PASS**: `gpu_applied.json`/`boot_flag.json` absent; `safe_mode=false`; `boot_flag_armed=false`;
  `consecutive_crashes=1`; anchored point NOT blacklisted. Help = usage only; dry-run = mode ANCHORED, exactly 1
  candidate + no-op line.
- **Result: exit 0, `Validated`.** No TDR/black-screen/reboot/DeviceLost/Unstable/silent-error. Candidate: target
  **1800 MHz**, anchor bin **975 mV**, base **1785 MHz**, **+15 MHz**; **27** bins capped to 1800 (max -150), **59**
  elastic (within +15 step / +30 abs caps).
- **Motor end-to-end**: Safe Loop armed BEFORE write → `apply_bounded_anchored_positive_offset` →
  `verify_anchored_positive_offset` = **`AnchoredRaiseVerified`** → dwell **Stable** (avg **1815 MHz**, p5 **1815 MHz**,
  **183 W**) → `reset_to_stock` ran + confirmed stock (all written bins cleared) → boot flag cleared after clean reset.
  Not blacklisted; **no profile persisted/applied/promoted** (Validated reported only).
- **Post-run**: `boot_flag.json`/`gpu_applied.json` absent; `safe_loop.json` byte-identical (mtime-only);
  `forge_state.json`/`gpu_knowledge.json`/`heartbeat.txt` unchanged; tree clean; HEAD `747a11b`.
- **Boost constrained vs prior simple F2**: simple boosted above target (avg **1868**, p5 **1845**, **199 W**);
  anchored pins a flat plateau (avg **1815** = p5 **1815**, **183 W**, ~**16 W** lower) → caps prevent boost above
  1800; +15 over target is within the 15 MHz verifier tolerance.
- **Meaning**: F2 ANCHORED-undervolt hardware path PROVEN at one bounded point — classic `MHz @ mV` SHAPE (anchor
  raise + plateau cap + elastic lower bins) holds + the **arm → write → verify → dwell → reset → clear** motor is
  recoverable. First direct support for the method (map stable voltage per clock → repeat → synthesize Godforge /
  Brokkr's Best / Deep Calm). Does NOT yet prove the MINIMUM stable voltage for 1800 MHz.
- **Next (don't re-run a confirmed command yet)**: bounded, supervised, same-target **MULTI-STEP** anchored probe at
  1800 MHz descending voltage until verifier fail / instability / clock drop / floor / budget. Detail in
  `decisions.md` / `handoff.md` (top entries).

## Latest (2026-06-21) — F2 ANCHORED undervolt planning IMPLEMENTED (no hardware)
- **What**: F2 now plans a true CLASSIC anchored undervolt point — RAISE the anchor bin to target AND
  CAP every higher-voltage bin DOWN to the same target (≤ 0 offsets), lower bins elastic. **ANCHORED is
  the DEFAULT** mode; `--simple` keeps the old single-bin descent. Code + tests + docs only — **no
  `--confirm`, no VF write, no Safe Loop mutation, no build-frontier/stress/sweep.**
- **Why**: the prior confirmed run proved the positive-offset MOTOR but was not anchored — the GPU still
  boosted ABOVE the 1800 MHz target. Classic `MHz @ mV` undervolt must test an anchored curve point.
- **New symbols** (SEPARATE from F1; `apply_vf_ceiling_monotone` UNTOUCHED):
  `plan_bounded_anchored_positive_offset` / `apply_bounded_anchored_positive_offset` /
  `AnchoredPositiveOffsetPlan` (gpu-nvapi — anchor reuses the bounded single-bin planner →inherits all
  bounds); `verify_anchored_positive_offset` / `AnchoredOffsetVerification::AnchoredRaiseVerified`
  (gpu_verify); `UndervoltMode` / `plan_anchored_undervolt` / `anchored_plan_lines` (gpu_undervolt).
- **Confirmed branch (anchored, NOT executed)**: ONE anchored curve plan, single-step (`--steps 1`), arms
  Safe Loop before write, resets on every post-arm exit, clears boot flag only after a confirmed reset,
  confirms ALL written bins read ~0, no persistence/apply/promotion.
- **Validation**: `cargo check` clean; service **240** tests + gpu-nvapi **33** tests pass (F1 + simple-F2
  still green). Read-only dry-run (`--target-mhz 1800 --steps 1`): anchor **981 mV/1785 +15 → 1800**, **25**
  bins capped to 1800 (max -135), **61** elastic, self-check `AnchoredRaiseVerified`, no write.
- **Hardware NOT validated for anchored mode.** First future anchored run: `--target-mhz 1800 --steps 1
  --confirm` — ONE candidate, operator present, NOT multi-step. Detail in `decisions.md`/`handoff.md` (top).

## (2026-06-21) — F2 true-undervolt FIRST confirmed hardware validation (1800 MHz @ 981 mV, +15) — PASS
- **One supervised confirmed run** (operator present, ONE confirmed command, no second) of the `78ecfc7` F2
  branch: `undervolt-probe --target-mhz 1800 --steps 1 --confirm`. **First real positive-offset VF write.**
  HEAD = origin/master = `78ecfc7`, tree clean; fresh worktree binary built first (was absent; mtime > `78ecfc7`).
- **Preflight PASS**: `gpu_applied.json`/`boot_flag.json` absent; `safe_mode=false`; `boot_flag_armed=false`;
  `consecutive_crashes=1`; point NOT blacklisted (`blacklisted_points=0`). Help = usage only; dry-run = exactly 1
  candidate + no-op line.
- **Result: exit 0, `Validated`.** No TDR/black-screen/reboot/DeviceLost/Unstable/silent-error. Candidate: target
  **1800 MHz**, bin **981 mV**, base **1785 MHz**, **+15 MHz** (within +15 step / +30 abs caps).
- **Motor end-to-end**: Safe Loop armed BEFORE write → `apply_bounded_positive_offset` → `verify_positive_offset`
  = **`RaiseVerified`** → dwell **Stable** (avg **1868 MHz**, p5 **1845 MHz**, **199 W**) → `reset_to_stock` ran +
  confirmed stock → boot flag cleared after clean reset. Not blacklisted; **no profile persisted/applied/promoted**
  (Validated reported only, no `last_validated` write).
- **Post-run**: `boot_flag.json`/`gpu_applied.json` absent; `safe_loop.json` byte-identical (mtime-only);
  `forge_state.json`/`gpu_knowledge.json`/`heartbeat.txt` unchanged; tree clean; HEAD `78ecfc7`.
- **Meaning**: F2 hardware path PROVEN at one bounded positive-offset point — **arm → write → verify → dwell →
  reset → clear** is viable + recoverable. Does NOT prove an optimal profile (minimum-viable only). Dwell clock
  above 1800 MHz (1868 avg) is EXPECTED — probe doesn't lock the clock; GPU still boosts per curve/power.
- **Next (don't re-run a confirmed command yet)**: (1) bounded F2 MULTI-STEP for the same target, (2) explicit
  `--start-mv` confirmed single-step if unsupported, or (3) result recording / Forge Knowledge for validated F2
  candidates without promotion. First optimization = search the lower-voltage limit around 1800 MHz with the same
  Safe Loop / verification / reset guarantees. Detail in `decisions.md` / `handoff.md` (top entries).

## (2026-06-20) — F2 CONFIRMED single-step branch IMPLEMENTED, not executed (no hardware)
- **First real confirmed F2 branch** (`undervolt-probe --confirm`): single-target, single-step only.
  IMPLEMENTED but NOT run — no `--confirm`, no VF write, no Safe Loop mutation this task.
- **State machine** (`gpu_undervolt.rs`, trait-isolated + mock-tested `run_confirmed_f2_step`/`F2Ops`):
  arm boot flag → apply ONE bounded positive offset → verify (offset-presence, idle freq=None) → dwell →
  `reset_to_stock` on EVERY exit → clear flag ONLY after a CONFIRMED reset (real reset re-reads the bin and
  fails closed if not ~0). DeviceLost/reset-fail RETAIN the flag; DeviceLost/Unstable blacklist; only
  Stable+confirmed-reset → Validated (reported only, no `last_validated` write). No persist/apply/promote.
- **Preflight** `confirmed_f2_refusal`: requires `--steps 1`; refuses Safe Mode / armed flag /
  consecutive_crashes ≥ 3 / no candidate / out-of-bounds / blacklisted (3-axis + 2-axis). `--confirm` runs
  startup recovery first.
- **Help fixed**: `--help`/`-h` prints usage + `--confirm` may-write-VF/operator warning; no hardware read.
- **F1 untouched**: `apply_vf_ceiling_monotone` + build-frontier unchanged; `gpu_power_sweep.rs` edits are
  additive/visibility (`reset_to_stock` pub(crate); `single_load_dwell`/`SingleDwell` reuse
  `load_and_measure`). Dry-run output unchanged except footer + help.
- **Validation**: `cargo check` clean; service tests **228/0** (+15); gpu-nvapi **25/0**; dry-run + help
  read-only. **Hardware NOT validated.** First future run: `undervolt-probe --target-mhz 1800 --steps 1
  --confirm` (operator present, one run). Detail in `decisions.md`/`handoff.md` (top entries).

## Checkpoint (2026-06-20) — F2 true-undervolt foundation IMPLEMENTED (pure, no hardware)
- **First isolated F2 path** for TRUE undervolt: bounded POSITIVE VF offsets (raise a lower-voltage bin to hold
  the target clock) — the opposite of F1/build-frontier flatten-down. F1 stays intact; F2 gets its own symbols.
- **gpu-nvapi**: pure `plan_bounded_positive_offset` + windows `apply_bounded_positive_offset`;
  `PositiveOffsetPlan`/`PositiveOffsetLimits`; consts `POS_OFFSET_MAX_MHZ=+30`, `POS_OFFSET_STEP_MAX_MHZ=+15`.
  `apply_vf_ceiling_monotone` NOT touched.
- **gpu_verify**: pure `verify_positive_offset` → `PositiveOffsetVerification` (RaiseVerified/RaiseIncomplete/
  OverRaise/Unverifiable); intended raise is the success case (no flatten-down overshoot veto); flatten-down
  verifier unchanged.
- **gpu_undervolt.rs (NEW)**: pure `plan_undervolt_probe` (descend real bins, compute bounded offset to hold
  target, stop at first bound/floor violation) + pure `undervolt_preflight` (Safe Loop read-only refusal) +
  windows `run_undervolt_probe` (dry-run; `--confirm` fails closed). CLI: `undervolt-probe` (`--target-mhz`,
  `--start-mv`, `--steps`; `--confirm` parsed but REFUSED this task).
- **Fail-closed**: empty/foreign/non-sane base, non-real bin, below floor, offset≤0, offset>+30, step>+15,
  clock>ceiling all → explicit Err (never clamps); bounds are constants, not CLI-widenable.
- **NOT touched**: F1 flatten-down writer/verifier, Safe Loop, boot flag, reset_to_stock, blacklist,
  last-known-good, power-limit/TDP/clock-lock. No persist/apply/promote, no multi-target loop, no crash-seeking.
- **Hardware BLOCKED.** Next: dry-run review of `undervolt-probe`, THEN a first supervised one-step confirmed
  F2 validation. Detail in `decisions.md`/`handoff.md` (top entries).

## Checkpoint (2026-06-20) — F1c bounded-tail confirmed PASS + tail-richness follow-up
- **Confirmed run (2026-06-20) of the bounded tail (`8667bf0`) = PASS**. Safety clean (exit 0, no
  TDR/crash/reboot, reset_to_stock ran, no persist/apply, state byte-identical, monotone positive_offsets=0).
  Phase B focus 1800, started 1056 mV (below 1062 floor), crossed knee (pcf 1.000@1012 → **0.215@1006 mV**),
  **continued past the first off-cap point to 1000 mV, captured 2 useful points**, `KneeTailComplete`;
  **synthesis became `differentiated`** (was collapse).
- **Remaining issue**: both tail points ~199 W → Godforge/Brokkr's/Deep Calm coincided (~1811 MHz/1006 mV/199 W).
  Differentiated but THIN.
- **Follow-up (2026-06-20)**: enrich the tail — `PHASE_B_MIN_USEFUL_POINTS` 2→**4**,
  `PHASE_B_POST_KNEE_TAIL_BINS` 3→**5** (synthesis collapse threshold `MIN_USEFUL_FRONTIER_POINTS` stays 2).
  Bounded: 4 useful OR 5 post-knee bins; opt-in/default OFF; no new CLI flag; `--phase-b-probes`/global
  `--max-probes` bound it; failure/verifier/instability/floor/budget keep precedence.
- **Unchanged**: Phase A, synthesis, bind-seeking, safety chain. File: `gpu_power_sweep.rs` only.
- **Hardware**: one confirmed validation authorized (same flags) to test whether power drops below the knee and
  the three profiles separate. Detail in `decisions.md`/`handoff.md` (top entries).

## Checkpoint (2026-06-16) — F1c follow-up: Phase B captures a bounded below-knee TAIL (commit 8667bf0) — pure, no hardware
- **Driver**: FIRST confirmed knee-seeking run (2026-06-16) = **PASS-PARTIAL**. Found the real knee at
  **~1025 mV** (Phase B started 1056 mV, below the 1062 Phase-A floor; pcf dropped **1.000→0.437 in one 6 mV
  bin** — steep knee). Safety PASS (exit 0, no TDR/crash/reboot, reset_to_stock ran, no persist/apply, state
  byte-identical, monotone writer positive_offsets=0). But Phase B stopped at the FIRST off-cap point → only
  **1** useful point → synthesis correctly still `POWER-BOUND COLLAPSE`. Stop policy, not budget, was the limit.
- **What landed**: `descend_phase_b` now captures a BOUNDED below-knee tail. After the knee crossing (first
  `pcf < POWER_BOUND_FRAC` point) it keeps descending until `PHASE_B_MIN_USEFUL_POINTS` (=2) useful off-cap
  points OR `PHASE_B_POST_KNEE_TAIL_BINS` (=3) post-knee bins, then stops cleanly as new
  `BracketStop::KneeTailComplete`. ≥2 useful → existing synthesis differentiates; 1 → honest collapse.
- **Safety precedence preserved**: crash/abort/global-drain/verifier-fail/instability are checked BEFORE the
  tail and stop immediately; floor / `--phase-b-probes` / global `--max-probes` still bound it.
- **Unchanged**: Phase A, synthesis, bind-seeking, safety chain (writer/verifier/Safe Loop/reset_to_stock/
  floor/cluster/persistence/power-limit/clock-lock); opt-in / default OFF; no new CLI flag. File:
  `crates/service/src/gpu_power_sweep.rs` only.
- **Validation**: `cargo check` clean; `cargo test -p nidavellir-service` **203 / 0** (8 new). No hardware.
- **Hardware STILL BLOCKED**. Next: NEW dry-run-only review of the bounded-tail plan. Detail in
  `decisions.md` / `handoff.md` (top entries).

## Checkpoint (2026-06-16) — F1c follow-up: Phase B continues BELOW Phase-A floor (commit 9f35ec0) — pure, no hardware
- **What**: budget-efficiency fix for F1c Phase B (dry-run-review finding). Phase B now CONTINUES below the
  deepest bin Phase A already explored for the focused target, instead of re-probing the inert top bins.
  File: `crates/service/src/gpu_power_sweep.rs` only. Pure: no hardware, no `--confirm`.
- **Why**: fine VF curve (~6–7 mV/bin) — `0ef4e68` Phase B re-started from the cap, so `--phase-b-probes 12`
  reached only ~1006 mV (re-covered 1075/1068/1062), ~75 mV above the ~930 mV knee. Now each probe is a new,
  deeper bin.
- **How**: pure helpers `phase_a_deepest_bin` (focus target's deepest retained Phase-A bin) +
  `phase_b_start_below` (highest real bin strictly below it) → Phase-B start. Fallbacks: no Phase-A history
  → safe-start cap; Phase A at the floor → Phase B skipped cleanly. Dry-run plan gains a `knee start` line.
- **Unchanged**: Phase A, `descend_phase_b`, synthesis, safety chain (writer/verifier/Safe Loop/
  reset_to_stock/floor/cluster/persistence/power-limit/clock-lock); opt-in / default OFF; global
  `--max-probes` master cap.
- **Validation**: `cargo check` clean; `cargo test -p nidavellir-service` **195 / 0** (5 new). No hardware.
- **Hardware STILL BLOCKED**. Next: NEW dry-run-only review of the improved plan. Budget sizing still the
  operator's call (~20+ Phase-B probes to cross a ~930 mV knee from a ~1062 mV floor); default budget
  unchanged (12). Detail in `decisions.md` / `handoff.md` (top entries).

## Checkpoint (2026-06-15) — F1c power-bound knee-seeking two-phase prototype IMPLEMENTED (commit 0ef4e68) — pure, no hardware
- **What landed**: OPT-IN (default OFF) two-phase power-bound knee-seeking for `build-frontier` — the
  design-audit direction `NEED DEEPER POWER-BOUND DESCENT`. Files: `crates/service/src/gpu_power_sweep.rs`
  + `crates/service/src/main.rs` (2 CLI flags). Pure: no hardware, no `--confirm`, no dry-run, no VF write.
- **Why shallow collapse ≠ terminal**: the validated `0996769` run only walked the top ~13 mV (bins
  `1075/1068/1062`), ~130 mV above the card's ~930 mV operating voltage, so the VF ceiling was INERT and
  pcf stayed 1.000 — honest diagnostic for a SHALLOW descent, not proof no frontier exists.
- **Phase A** = the existing broad/shallow single-pass descent, extracted VERBATIM into
  `run_target_descents` → byte-for-byte unchanged when OFF. **Phase B** runs ONLY after a Phase-A
  power-bound collapse AND the opt-in is set: `detect_plateau_clock` (median power-bound clock) →
  `select_phase_b_target` (lowest candidate ≥ plateau) → `descend_phase_b` (deep descent on ONE focused
  target through real VF bins, bounded budget, full trajectory) → `detect_power_bound_knee` (first pcf
  crossing below 0.95). Merge + re-synthesize via existing `synthesize_forge_profiles`.
- **Knee mental model**: above-knee `pcf ≥ 0.95` (ceiling inert — keep descending); knee = first pcf drop
  < 0.95; clean deep stop at `pcf ≤ 0.50`; below-knee tail → Brokkr's/Deep Calm; Godforge = highest
  sustained off-cap clock (knee region), NOT highest requested clock. No knee ⇒ honest `PowerBoundCollapse`
  preserved.
- **Flags**: `--power-bound-knee-seeking` (default OFF) + `--phase-b-probes N` (default None → 12).
  Global `--max-probes` stays the MASTER cap; Phase-B budget only bounds the focused descent depth;
  `--phase-b-probes 0` fails closed.
- **Safety surfaces UNCHANGED**: VF (monotone static-base) writer, verifier gates, Safe Loop,
  `reset_to_stock` (runs after every build, both paths), floor/cluster derivation, per-target cap,
  warm-start default OFF, persistence/knowledge writes, power-limit/TDP/clock-lock.
- **Validation**: `cargo check -p nidavellir-service` clean (0 warnings); `cargo test -p
  nidavellir-service` **190 passed / 0 failed** (17 new). No hardware run.
- **Hardware STILL BLOCKED.** Next: SEPARATE dry-run-only review of the new opt-in `--power-bound-knee-seeking`
  plan output (no `--confirm`); no confirmed run until that review; no same-config rerun. Detail in
  `decisions.md` / `handoff.md` (top entries).

## Checkpoint (2026-06-15) — F1b power-bound collapse classification FIRST CONFIRMED HARDWARE VALIDATION (commit 0996769) — PASS
- **One supervised confirmed run** validating `0996769` (docs `4880153`); HEAD = origin/master = `4880153`,
  tree clean; fresh worktree binary. Dry-run gate passed first. `build-frontier --confirm --max-targets 7
  --max-probes 21 --max-probes-per-target 3 --safe-start-cap 1075 --bind-seeking`. **Exit 0; ~5.7 min.**
- **Safety PASS**: no TDR/crash/reset/reboot; `reset_to_stock` ran; GPU back at stock/idle. After:
  `gpu_applied.json`/`boot_flag.json` absent; `safe_loop.json` idle/disarmed (mtime-only change);
  `forge_state.json`/`gpu_knowledge.json`/`heartbeat.txt` unchanged; tree clean. Every probe
  `write_mode=monotone_static`, `positive_offsets=0`; no overshoot veto.
- **Mechanics**: 19 probes / 17 dwells; `--max-probes 21` not exhausted; 6/7 targets characterized (1920 dropped
  on benign verifier `LiveMismatch`, run-variance; 1890 later LiveMismatch kept deepest verified). All dwells
  PowerLimited, `power_capped_frac=1.000`, ~199 W, ~1784–1825 MHz.
- **Reporting honesty PASS**: no `BoundBinding`, no `reason=Clock`. **Clock-arm retirement validated** (probes
  that would false-bind under v2 avg-clock did NOT bind → `PerTargetCap`). **`LeftPowerRegime` validated
  negatively** (no false-fire; no target had pcf ≤ 0.50 so none stopped by it). **`PowerBound`/collapse positive**:
  6 `[power-bound]` / 0 useful; explicit *"power-bound collapse — cannot build a differentiated VF frontier under
  this workload/regime"*; Godforge/Brokkr's/Deep Calm collapsed to one best-effort point (1815 MHz/199 W, R=0.00,
  conf 0.21), flagged not-differentiated — no fake frontier.
- **Verdict PASS** (safety + honesty). Frontier still not useful: card pinned at ~199 W cap, now reported
  honestly. **Caveat**: `LeftPowerRegime` validated negatively only (positive stop needs pcf ≤ 0.50). **Next**:
  accept patch; **keep hardware BLOCKED for this config**; don't repeat the run / bump per-target cap / touch
  power-limit yet. Design decision next: non-cap-saturating workload, targets below the power-bound plateau, or a
  "cannot differentiate" presentation pass. See `decisions.md` + `handoff.md`.

## (2026-06-15) — F1b power-bound collapse classification IMPLEMENTED (commit 0996769) — pure, no hardware
- **Commit `0996769 fix(service): classify power-bound frontier collapse`** (pushed to `origin/master`). Scope:
  `crates/service/src/gpu_power_sweep.rs` ONLY. The SIMPLIFY patch from the audit below. No hardware.
- **Retired bind-seeking's Clock arm** → `classify_binding` regime-only: bind (stop early) ONLY on leaving the
  power-limited regime (`power_capped_frac <= 0.50`). Removed `BIND_OVERSHOOT_MHZ` / `overshoot_mhz` /
  `BindReason::Clock`; start-bin guard kept. Renamed `BracketStop::BoundBinding → LeftPowerRegime`.
- **Power-bound classification** (`POWER_BOUND_FRAC = 0.95`, pure helpers `is_power_bound_frac/_point`,
  `useful_frontier_points`, `frontier_power_bound_collapse`): a pcf-saturated stable dwell is a valid raw
  bracket but NOT useful clock-frontier diversity; invalid/missing pcf → not power-bound (fail open), still
  fail-closed for regime binding.
- **Collapse-aware synthesis**: `synthesize_forge_profiles` excludes power-bound points; < 2 useful → flagged
  best-effort + "power-bound collapse — cannot build a differentiated VF frontier…" (new
  `ForgeProfiles.power_bound_excluded` / `power_bound_collapse`). Catches jittery ~1798–1819 MHz @ pcf 1.0 that
  exact-distinct-clock missed. No power-bound points → legacy path unchanged. RESULT prints per-point pcf +
  `frontier classes` summary.
- **Unchanged safety surfaces**: writer, verifier, Safe Loop, reset_to_stock, floor/cluster, per-target cap,
  warm-start default OFF, persistence/knowledge, power-limit/clock-lock. `cargo check` clean; `cargo test`
  **173 passed**. **Hardware STILL BLOCKED** (pure patch; review new diagnostics in a dry-run before any run).
  See `handoff.md` + `decisions.md`.

## (2026-06-15) — build-frontier / F1b algorithm audit — verdict SIMPLIFY (read-only, pre-implementation)
- **Read-only audit** of `crates/service/src/gpu_power_sweep.rs` + continuity docs. **No code/tests/hardware/
  `--confirm`/VF-write/stress/power-sweep** run. Recorded BEFORE implementation to set the next patch's north
  star. Full rationale: `decisions.md` (top) + `handoff.md` (Latest backend checkpoint).
- **Verdict: SIMPLIFY CURRENT DIRECTION** — not a redesign, not a full rollback. Don't run more hardware before
  the next pure/pure-ish patch; don't keep adding bind-seeking complexity. The discovery → descent → synthesis
  skeleton is still valid; the drift is concentrated in **bind-seeking / `BoundBinding`** semantics.
- **Bind-seeking conclusion**: `BoundBinding` is the wrong combined abstraction — a **bad Clock arm**
  (false-binds under power cap) + a **useful Regime arm** (`pcf <= 0.5`). The v2 start-bin guard was useful +
  validated but did NOT fix physical frontier collapse: the confirmed v2 run (`bf02971`) stayed power-limited
  (`power_capped_frac=1.000`, ~199 W, ~1798–1819 MHz, confidence 0.21, profiles collapsed). ⇒ remaining issue =
  **regime/power-bound collapse, not scheduler depth or per-target probe count.**
- **Decision**: retire/neutralize the Clock arm; keep the regime signal as **`LeftPowerRegime`**; add a
  first-class **`PowerBound`/`PowerBoundCollapse`** classification; strengthen `synthesize_forge_profiles` to
  detect the pcf-saturated plateau (today it keys on exact-distinct clocks, so jittery ~1800 MHz reads as
  "differentiated" and the warning never fires) and emit *"power-bound collapse — cannot build a differentiated
  VF frontier under this workload/regime."* Power-limited samples = valid bracket, **not** useful clock-frontier
  diversity; raw synthesis input but filtered out of differentiated selection; primary collapse signal.
- **KEEP (load-bearing)**: hardware-derived floor; cluster selection / sane-core filtering; real-bin descent;
  per-target cap; typed hard/soft stops; confidence gate + best-effort fallback; monotone writer; verifier
  gates; Safe Loop; `reset_to_stock`; no persistence during build-frontier.
- **Non-goals / hardware**: pure-ish patch in `gpu_power_sweep.rs` + synthetic-sample tests only; no hardware
  run, no power-limit/TDP/clock-lock changes, no target-gen redesign yet, no warm-start/per-target-cap/safety
  changes, no version bump. **Hardware BLOCKED** until the classification + collapse report land and a fresh
  dry-run shows them.

## (2026-06-15) — FIRST confirmed hardware validation of bind-seeking F1b v2 strictness (commit bf02971) — mechanism PASS, frontier PARTIAL
- Supervised confirmed run (operator present) validating `bf02971`; docs at `3b8774c`
  (HEAD/origin/master = `3b8774c`, tree clean). A **fresh worktree binary was built first** — the
  worktree-local `target/debug/nidavellir-service.exe` was absent and the only existing binary was stale
  (main-repo, built 2026-06-07, predating bind-seeking): `cargo build -p nidavellir-service` → worktree
  binary created after the build; stale main-repo binary NOT used; tree stayed clean.
- **Dry-run gate passed** (no `--confirm`): bind-seeking ENABLED; v2 start-bin-not-eligible note; thresholds
  `avg_clock_overshoot <= 30 MHz` + `power_capped_frac <= 0.50`; coverage-bounded scheduler;
  `max_probes=Some(21)`; `max_probes_per_target=Some(3)`; targets `[1935,1905,1875,1845,1815,1785,1755]`;
  first-pass bins `[1075,1068,1062]`; warm-start OFF; no applied-profile / Safe Loop warning; dry-run no-op
  line. Confirmed: `build-frontier --confirm --max-targets 7 --max-probes 21 --max-probes-per-target 3
  --safe-start-cap 1075 --bind-seeking`.
- **Safety PASS**: exit 0; no TDR/driver-reset/black-screen/reboot/crash; `reset_to_stock` ran; GPU back at
  stock idle. After: `boot_flag.json`/`gpu_applied.json` absent; `forge_state.json`/`gpu_knowledge.json`/
  `heartbeat.txt` unchanged; `safe_loop.json` idle (`safe_mode:false`), size unchanged, mtime touched by
  startup recovery only.
- **Probe**: 15 dwells; all 7 targets characterized; 6 via **`BoundBinding`** (1935/1905/1875/1845/1815/1785),
  1 via **`PerTargetCap`** (1755); none dropped; `--max-probes 21` not exhausted (15/21); no `overshoot_veto`;
  all probes `write_mode=monotone_static`, `positive_offsets=0`.
- **v2 mechanism PASS (start-bin guard)**: every 1075 mV start bin `eligible=false/bound=false`; all 7
  descended to 1068, 1755 to 1062; earliest bind only after a real descent (6 bound at 1068, `reason=Clock`);
  bind telemetry present (eligible/bound/reason/avg_clock_mhz/p5_clock_mhz/power_capped_frac); regime arm never
  fired (pcf=1.000).
- **Frontier PARTIAL (did NOT de-collapse)**: all dwells PowerLimited, `power_capped_frac=1.000` throughout
  (~199 W flat); clocks clustered **~1798–1819 MHz**; confidence stayed **0.21**; Godforge/Brokkr/Deep Calm
  collapsed to ~1800 MHz/199 W. v2 fixed the **procedural** start-bin bug; the remaining collapse is
  **power/regime**, not scheduler depth and not the per-target cap.
- **Direction**: don't repeat the run; don't bump the per-target cap as the immediate next step; don't jump to
  risky power-limit/clock-lock changes. Next design (analysis first): **regime-aware binding**, distinguish
  `Clock` from `PowerLimitedPlateau`/`PowerBoundCollapse`, veto `Clock` binding when pcf is saturated ~1.0,
  add collapse diagnostics + power-headroom/power-drop telemetry. **Stop for analysis before any further
  confirmed run.** See `handoff.md` + `decisions.md`.

## (2026-06-15) — bind-seeking F1b v2 strictness IMPLEMENTED + pushed (commit bf02971) — hardware-validated (see entry above)
- **Commit `bf02971 fix(service): tighten bind-seeking stop criteria`**, pushed to `origin/master`
  (HEAD = origin/master = `bf02971`). Scope: `crates/service/src/gpu_power_sweep.rs` only.
- **Why**: v1's first supervised hardware run was safety/mechanics **PASS** but semantic **PARTIAL** — v1
  bound on the **first/start bin (1075 mV)**, so every viable target stopped immediately, no descent occurred,
  frontier stayed degenerate (single-bin ~1075 mV / ~199 W, Forge confidence ~0.21).
- **v2**: start bin NOT bind-eligible (earliest bind = 2nd probed real VF bin); clock binding uses the
  **average/achieved clock** (`avg - target <= 30`), not p5/sustained (p5 = telemetry only); regime arm
  `power_capped_frac <= 0.5` kept but **invalid/missing cap_frac fails closed**. New `BindReason`/`BindDecision`
  + per-probe bind telemetry (eligible / bound / reason / avg_clock_mhz / p5_clock_mhz / power_capped_frac);
  dry-run reports the start-bin-not-eligible caveat.
- **Precedence preserved**: crash → abort → budget drain → verifier failure → dwell instability → binding →
  per-target cap → floor. **Safety unchanged**: monotone writer, verifier gates, Safe Loop, `reset_to_stock`,
  persistence/apply, hardware-floor derivation, warm-start default OFF.
- **Validation (no hardware)**: `cargo check` clean; `cargo test -p nidavellir-service` **169 passed**;
  dry-run only passed; no hardware boundary crossed.
- **Hardware-validated 2026-06-15** (see the FIRST confirmed hardware validation entry above) via
  `build-frontier --confirm --max-targets 7 --max-probes 21 --max-probes-per-target 3 --safe-start-cap 1075
  --bind-seeking` — mechanism PASS (start-bin guard), frontier PARTIAL (still power-limited / collapsed).

## 2026-06-14 — bind-seeking F1b v1 IMPLEMENTED + pushed (commit 08f745e), hardware-validated PARTIAL → superseded by v2
- **Commit `08f745e feat(service): add opt-in bind-seeking to build-frontier`**, pushed to `origin/master`
  (HEAD = origin/master = `08f745e`). Scope: `crates/service/src/gpu_power_sweep.rs` +
  `crates/service/src/main.rs` only. Builds the bind-seeking direction from the `5248758` run.
- **Feature**: opt-in CLI flag **`--bind-seeking`** + `FrontierLimits.bind_seeking`, **default OFF** (absent =
  current behavior byte-for-byte). Per target the descent stops at the first verified+stable **binding** point
  instead of walking a fixed bin count, so targets can differentiate (vs the prior 1832–1867 MHz / 194–199 W
  collapse).
- **Binding v1 (Clock + regime)** — pure `classify_binding`: verified + stable AND either
  `sustained - target <= BIND_OVERSHOOT_MHZ (30)` (sustained = p5 else avg) OR `power_capped_frac <=
  BIND_CAP_FRAC (0.5)`. **Power-drop is intentionally NOT a v1 stop-condition** (no top-power reference
  tracking; telemetry/log later, not binding logic now).
- **Scheduler**: new `BracketStop::BoundBinding` — clean (`is_hard_failed()==false`), carry-forward eligible
  with a `lowest_verified_mv`. Binding checked only on a verified+stable sample, after the failure arms.
  Precedence preserved: crash → aborted → global budget drained → verifier-failure/unverified →
  dwell-unstable/silent-error → **binding** → per-target cap / floor.
- **Invariants**: `--max-probes` = hard global cap; `--max-probes-per-target` = per-target attempt/depth cap
  (bind-seeking may stop earlier); **warm-start default OFF**. Unchanged: monotone static-base writer, verifier
  gates, Safe Loop, `reset_to_stock`, hardware-derived floor, persistence/profile apply. No
  power-limit/clock-lock changes.
- **Validation (no hardware)**: `cargo check -p nidavellir-service` clean; `cargo test -p nidavellir-service`
  **165 passed / 0 failed**. Dry-run only (no `--confirm`): `--max-targets 7 --max-probes 21
  --max-probes-per-target 3 --safe-start-cap 1075 --bind-seeking` → exit 0; `bind-seeking: ENABLED`, thresholds
  + caveat, warm-start OFF, no Safe Loop arm / apply / dwell / VF write.
- **Hardware validation NOT yet done for `08f745e`.** Next (separate, operator-present): clean confirming
  dry-run, then `build-frontier --confirm --max-targets 7 --max-probes 21 --max-probes-per-target 3
  --safe-start-cap 1075 --bind-seeking`. No hardware commands run in the implementation or docs pass. See
  `handoff.md` + `decisions.md`.

## Latest (2026-06-13) — FIRST confirmed hardware validation of F1b `--max-probes-per-target` (commit 5248758) — coverage PASS, profile PARTIAL
- Supervised run, operator present, after a clean confirming dry-run (no plan drift; HEAD/origin/master
  `5248758`; `47f39be`/`f90981d`/`8503182` present; `gpu_applied.json`/`boot_flag.json` absent;
  `safe_loop.json` idle/`safe_mode:false`):
  `build-frontier --confirm --max-targets 7 --max-probes 14 --max-probes-per-target 2 --safe-start-cap 1075`
  — **warm-start OFF**. Exit 0; ~4 min; no TDR/driver-reset/black-screen/reboot/crash.
- **Safety PASS**: Safe Loop armed→cleared **per probe** (idle); `reset_to_stock` ran ("GPU restored to stock;
  no profile applied or persisted"). `boot_flag.json`/`gpu_applied.json` absent before+after;
  `forge_state.json`/`gpu_knowledge.json`/`heartbeat.txt` unchanged (no forge-state persistence, no knowledge
  write); `safe_loop.json` content/size unchanged (idle, `safe_mode` false), mtime touched only — **no new
  blacklist/crash entry**. GPU back at stock idle.
- **Coverage PASS (the fix works)**: **13 dwells across all 7 targets** (vs prior 34-on-1935 depth-first). 6
  targets stopped via **`PerTargetCap`** (`probes_used=2`, bins **1075 + 1068 mV**); global `--max-probes 14`
  **not exhausted** (13 used) — the cap stopped one target from eating the whole budget. **1905 dropped** at
  probe 1 (`LiveMismatch`, `overshoot_veto=true`, `eff_cov=0.963` — conservative verifier reject; neighbors
  passed). All probes `write_mode=monotone_static`, `positive_offsets=0`; `NoDownCapNeededCeiling` (1935) +
  `VerifiedCurve` (1875–1755). No writer/verifier/Safe-Loop/reset/persistence regression. Shallow only
  (1075/1068 mV); did **not** touch 875/868/862/856/850.
- **Profile PARTIAL**: achieved clocks clustered **1832–1867 MHz**, power **194–199 W**; lower targets not
  distinct; Godforge/Brokkr/Deep Calm collapsed to **1860 MHz / 194 W**; FORGE confidence 0.21. **Shallow
  near-stock coverage (1075/1068 mV) is non-binding on this hard power-capped 3060 Ti** — the ceiling does
  not govern the achieved clock at that high-voltage band; the cap solved budget *distribution*, not binding.
- **Direction — bind-seeking F1b**: don't repeat the flags / use cap 3 / enable warm-start / jump to
  power-limit/clock-lock changes next. Per target: descend while stable-but-non-binding, stop when it BINDS /
  fails verifier/dwell / hits a cap. Goal = first useful (binding) point per target, not deepest voltage. No
  further hardware commands were run. See `handoff.md` + `decisions.md`.

## (2026-06-13) — FIRST confirmed hardware validation of the bin-based floor (commit f90981d) — PASS (safe)
- Supervised bounded run, operator present, after a clean dry-run on a fresh debug build (HEAD/origin/master
  at `c99dbf1`+`f90981d`; `23b70c4`/`8503182` present; `gpu_applied.json`/`boot_flag.json` absent;
  `safe_loop.json` idle/`safe_mode:false`):
  `build-frontier --confirm --max-targets 7 --max-probes 34 --safe-start-cap 1075 --warm-start-brackets`.
  `--max-probes 34` reaches 868 mV (one bin below the old 875 floor) and stops before 862 mV (reboot-zone).
- **Safety PASS**: exit 0; no TDR/driver-reset/black-screen/reboot/crash. Startup recovery clean; Safe Loop
  armed→cleared (idle); `reset_to_stock` ran ("GPU restored to stock; no profile applied or persisted").
  `boot_flag.json`/`gpu_applied.json` absent before+after; `forge_state.json`/`gpu_knowledge.json`/
  `heartbeat.txt` unchanged; `safe_loop.json` byte-identical (idle, `safe_mode` false, size unchanged),
  mtime touched at run start only — no new blacklist/crash entry. GPU back at stock idle.
- **Coverage**: 34 dwells **all on target 1935** (1075→868 mV). Reached 875 + 868 mV; **did not reach 862**
  (no `ceiling_mv=862`; the 35th scheduler step hit `BudgetExhausted` before write/dwell). 1905/1875/1845/
  1815/1785/1755 NOT physically characterized (budget spent on the hardest target). Warm-start: B1 1935 from
  cap, B2 1905 carried 893 mV (868+25). All probes `write_mode=monotone_static`, `positive_offsets=0`,
  `down_caps=0`, no `overshoot_veto`, all `NoDownCapNeededCeiling`, `eff_cov=1.000`.
- **Interpretation**: validated safe WRITING/descent of the static ceiling to 868 mV; did NOT prove core
  stability when RUN at 868 — GPU stayed **power-limited ~198 W**, ceiling **non-binding**. Frontier point
  1935 → 1839 MHz @ 868 mV vf_bin / 198 W. **PASS for first bin-based floor validation; partial for profile
  synthesis** (single clock 1800 MHz → FORGE confidence 0.21, profiles collapse). 862/855 reboot-zone
  blacklist is offset-keyed (different regime from this zero-offset power-limited descent).
- **Direction**: don't jump to `--max-probes 40`; `--max-probes 35` could deliberately touch 862 for
  boundary mapping (862 blacklist keyed freq=1755 won't match a 1935 ceiling → Safe Loop is backstop) but
  won't yield useful profiles. **Primary next: pivot to F1b / multi-clock, and/or make the ceiling BIND
  (raise power limit) before descending deeper.** No further hardware commands were run. See `handoff.md`
  + `decisions.md`.

## Bin-based floor shipped (2026-06-13) — build-frontier floor is hardware-derived / bin-based (commit f90981d, pushed)
- `f90981d feat(service): derive build-frontier floor from real VF bins` removes the hardcoded active
  **875 mV** descent floor. The floor is now the lowest real graphics-core VF bin
  (`seed.cluster_v_min_mv`); no replacement constant (no 825/800). Descent is **bin-based**: walks real
  VF/core-cluster bins only, never invents off-curve 25 mV voltages. Warm-start maps its margin to the
  conservative real bin **≥** the requested target and never starts below the previous
  `lowest_verified_mv`. `--max-probes` stays the exposure cap. Empty bin domain → fail closed before any
  hardware write. Dry-run prints the hardware floor + exact bin sequence + bin/dwell counts.
- Scope: only `crates/service/src/gpu_power_sweep.rs`. **Unchanged**: monotone static-base writer,
  verifier gates, Safe Loop, `reset_to_stock`, persistence, profile apply. `cargo check` clean; service
  tests 142 passed.
- The historical `1755 @ 875` validations remain valid for that point but are no longer an active floor;
  runs may now go **below 875**. **First confirmed hardware validation done 2026-06-13 (safe to 868 mV;
  see the Latest entry above).** First runs must be bounded
  (`--safe-start-cap`/`--max-probes`), dry-run reviewed before `--confirm`, operator present (descent may
  reach **below the ~855 mV reboot zone**). See `handoff.md` / `decisions.md` for the suggested dry-run.

## Current status (2026-06-05)
- `master`, tag **v0.3.1** (forge-state persistence pushed). Worktree branch
  `claude/vibrant-almeida-dfb6c7`.
- Active work: **foundation reviews before F1b** (F1b on hold, direction not final).
  Review 1 (persistence/startup) **done** → forge-state persistence shipped (below).
  Applied-Curve-Verification review **done** (investigation; see handoff).
  Review 2 (Sensor Quality Audit) **done** (investigation; key decision below).
- **F1b warm-start voltage-bracket carry-forward — SHIPPED + HARDWARE-VALIDATED (2026-06-13, commits
  23b70c4, 6f2f061)**: generic ordered-descent scheduler primitive (NOT Godforge-specific) behind
  opt-in **`--warm-start-brackets` (default OFF)** — an easier target reuses the previous harder
  target's verified + dwell-stable bracket (`lowest_verified_mv + 1 step`), skipping dominated
  high-V probes. Preserves monotone writer / verifier gates / `overshoot_veto` / Safe Loop /
  `reset_to_stock` / persistence / 875 mV floor. **Validation PASS**: supervised
  `build-frontier --confirm --max-targets 7 --max-probes 40 --safe-start-cap 1075 --warm-start-brackets`
  — exit 0; no TDR/reboot; Safe Loop clean; `reset_to_stock` ran; `boot_flag.json`/`gpu_applied.json`
  absent after; `forge_state.json`/`gpu_knowledge.json`/`heartbeat.txt` unchanged; GPU stock idle.
  **33 probes**, all 7 targets produced points; **B1/B2/B3 held, B2 exercised** (1905 failed verify at
  warm-start 900 mV → fell back once to cap 1075, target preserved). **−5 probes vs from-cap (38)** for
  an identical frontier (32 baseline ≈ flat); modest on RTX 3060 Ti (mid targets stop early on
  verify-axis residual overshoot). `1755 @ 900`/`@ 875` re-validated (`NoDownCapNeededCeiling`,
  overshoot 0, plateau 1665..1755 / 1620..1755, ≈1755 MHz @ 875 mV ≈176 W); `write_mode=monotone_static`,
  `positive_offsets=0`. Follow-up `6f2f061` surfaces scheduler `result.log` before `result.profiles.log`
  (log-only, deduped). **Keep default OFF**; next (later): more runs, benign-zero-only seeding
  refinement, broader confidence work; do NOT mix with persistence. See `handoff.md` + `decisions.md`.
- **F1b Phase 2B.2-c — monotone static-base VF writer HARDWARE-VALIDATED (2026-06-12, commit
  8503182)**: supervised `build-frontier --confirm --max-targets 7 --max-probes 40 --safe-start-cap
  1075` on a fresh `origin/master` debug build at `8503182`, after a clean bounded dry-run.
  **Safety held** (exit 0; no TDR/reboot; Safe Loop armed/cleared; `reset_to_stock` ran;
  `boot_flag.json`/`gpu_applied.json` absent after; `forge_state.json`/`gpu_knowledge.json`/
  `heartbeat.txt` unchanged; GPU back at stock idle). **Writer confirmed**: all 32 probes
  `write_mode=monotone_static`, `positive_offsets=0`, `static_base_points=132`. **Primary fix —
  `1755 @ 900 mV`**: OLD plateau 1755..1845 / `overshoot_veto=true` / `LiveMismatch` → NEW plateau
  1665..1755 / overshoot=0 / `NoDownCapNeededCeiling` (pass). Run continued to **`1755 @ 875 mV`**
  and verified (`NoDownCapNeededCeiling`, overshoot=0, plateau 1620..1755, ~19 s dwell, ≈1755 MHz @
  875 mV ≈179 W). Minor residual: a few non-1755 low-ceiling probes still show single-bin 15 MHz
  overshoot (not a blocker). FORGE synthesis low confidence (best 0.21) is the unrelated Wilson
  metric. **Next**: design warm-started voltage-bracket reuse for F1b/Godforge; do NOT mix with
  persistence/profile apply yet. See `handoff.md` + `decisions.md`.
- **F1b Phase 2B.2-c — FIRST confirmed run (2026-06-11, SAFE) + c.1 verifier fix (IMPLEMENTED, not
  committed)**: first supervised `build-frontier --confirm --max-targets 1 --max-probes 6
  --safe-start-cap 1075` ran after a Fable 5 GO audit + clean dry-run. **Safety held end-to-end**
  (no TDR/reboot; Safe Loop armed/cleared per probe; reset-to-stock on reject + at run end; no
  persistence; GPU back at stock) but **0 frontier points**: the target=1935 (stock boost top)
  probe was rejected `LiveMismatch offsets=20/27 plateau=1935..1935 overshoot=0` — flatten-to-top
  needs zero offset on bins already at target, so the ≥90% presence gate under-counts. **c.1**:
  narrow stock-equivalent path (`is_stock_equivalent_ceiling`, gpu_verify.rs) — only on
  LiveMismatch, only for targets within tol of the caller-passed stock top, all offsets readable,
  no overshoot (even in-tol), all bins in-tol below target, zero-offset bins EXACTLY at target;
  surfaced as service-internal `LiveCeilingEval.stock_equivalent` (IPC untouched);
  `verify_applied_curve` passes None (byte-identical); probe logs `verify=StockEquivalentCeiling`.
  Condition 1 directional (rejects targets above stock top). `cargo check` clean · service
  **109/109** (+11) · core 46/46. Files: `gpu_verify.rs`,
  `gpu_power_sweep.rs`. Next: bounded dry-run on rebuilt binary, then re-attempt the same bounded
  --confirm (user approval). Chain b.1→c.0 IS pushed (6881cd7); c.1 awaits commit approval.
- **F1b Phase 2B.2-c.0 — first-run limiter flags (2026-06-08) — pushed (6881cd7)**: added
  `build-frontier` flags `--max-targets N` / `--max-probes N` / `--safe-start-cap MV` to bound the
  first supervised run. Pure `FrontierLimits`/`validate_limits`/`apply_frontier_limits`
  (gpu_power_sweep) + `parse_frontier_limits` (main.rs). FAIL CLOSED on absurd values (0 / cap ≤
  crash floor / non-numeric / missing); cap never raises above the derived top nor below the floor;
  max-probes short-circuits remaining probes then resets to stock. Defaults preserve the full plan.
  No IPC/core/contract/apps-ui/Safe-Loop/persistence change, no hardware. `cargo check` clean ·
  service **95/95** (+7) · core 46/46. Files: `gpu_power_sweep.rs`, `main.rs`. **Dry-run QA**
  (`--max-targets 1 --max-probes 6 --safe-start-cap 1075`, stock, no --confirm, no state writes):
  targets=[1935], descent 1075→875 (9 bins), 6 dwells (~120 s capped). --confirm still forbidden.
- **F1b Phase 2B.2-b.4 — stock core VF cluster seeding (2026-06-07) — IMPLEMENTED, not pushed**:
  refines b.3 so `safe_start`/boost come from the actual contiguous core VF cluster, not the global
  max of sane points (which gave 1150 mV). `select_core_cluster` (pure): sort by voltage, split on
  gaps > 60 mV, pick the largest run (≥ 8 pts else FAIL CLOSED), derive safe_start/boost from the
  cluster top; isolated high-V points reported as rejected outliers. b.3 generic hard guards
  (500..3500 MHz, 600..1150 mV) retained. Dry-run prints cluster range + outliers + safe_start
  source + applied-profile warning. No IPC/core/contract/apps-ui/Safe-Loop/gpu_apply/nvml_gpu/
  Phase-3/11D change, no auto-reset, no hardware. `cargo check` clean · service **88/88** · core
  46/46. File: `gpu_power_sweep.rs`. **Stock dry-run QA pending user's manual reset; --confirm still
  forbidden.** (b.3 + b.4 both uncommitted — eventual commit bundles them unless split.)
- **F1b Phase 2B.2-b.3 — core-domain seeding guard (2026-06-07) — IMPLEMENTED, not pushed**: the
  first dry-run exposed seeding from the UNFILTERED global max of `read_vf_curve_modern()` (picked up
  memory-domain points → bogus plan: targets 7001..6311 MHz, safe_start 1237 mV; the dry-run gate
  blocked it, no hardware). Fix (pure): `sane_core_points` (freq 500..3500 MHz, voltage 600..1150 mV)
  + `derive_core_seed` (seed from sane points only; reject diagnostics; soft-warn >3200 MHz / >1125
  mV; FAIL CLOSED if no sane points or > hard guard). `run_build_frontier` aborts with no
  arm/apply/dwell/VF-write on fail-closed or any target > 3500 MHz. Re-run dry-run: 132 raw → 88 sane
  / 44 rejected (incl. 7001/1237), boost~1935, targets 1755..1935, 84 dwells (~1680 s), safe_start
  1150 mV (flagged soft-max). NO hardware, NO state writes (mtimes unchanged), NO --confirm. `cargo
  check` clean · service **86/86** (+5) · core 46/46. File: `gpu_power_sweep.rs`. `--confirm` still
  forbidden pending review. NB: plan reflects the currently-applied curve; a stock read is cleaner.
- **F1b Phase 2B.2-b.2 — real probe + supervised `build-frontier` (2026-06-07) — IMPLEMENTED (code
  only, NOT run), not pushed**: added the real Windows probe `real_probe_step` (snap bin → arm Safe
  Loop → `apply_vf_ceiling` → shared `classify_live_ceiling` verify + 11C diag → `load_and_measure`
  → clear → `measured_to_probe` + `vf_bin_mv`; dwell-crash → reset + abort-flag short-circuit) and
  `run_build_frontier(store, confirm)` (always prints the plan; dry-run read-only; `--confirm` runs
  the real frontier then ALWAYS resets to stock). Console subcommand `build-frontier` in `main.rs`
  (`--confirm` runs startup recovery first; dry-run does not). **No auto-apply, no forge_state, no
  gpu_knowledge writes, no IPC/contract/core/apps-ui change, hardware path NOT executed.** Conservative
  first-run consts (lowest_safe=875 mV, 25 mV step, idle Unconstrained→PowerLimited). `cargo check`
  clean · service **81/81** (+1) · core 46/46. Files: `gpu_power_sweep.rs`, `main.rs`. Dry-run:
  `nidavellir-service.exe build-frontier`; confirmed (NOT run): `... build-frontier --confirm`.
  Supervised QA = 2B.2-c (separately gated); 11D after Phase 2B.
- **F1b Phase 2B.2-b.1 — seeding + dry-run plan + vf_bin propagation (2026-06-07) — IMPLEMENTED, not
  pushed**: exposed `classify_live_ceiling`/`LiveCeilingEval`/`CurveDiag` `pub(crate)` (intra-crate;
  no IPC/contract change); added pure `derive_descent` (FrontierDescent from live curve bins + crash
  floor) + read-only `plan_frontier` (dry-run worst-case dwell count/wall-time + safety notice);
  added internal `ProbeSample.vf_bin_mv` (NOT IPC) so `probe_to_point` records the actually-applied
  snapped bin (fallback = descent vbin); `measured_to_probe` leaves it None (the real probe fills it).
  NO real probe / apply / load / sweep / stress / subcommand / Safe-Loop / startup-recovery /
  persistence / Phase-3 / 11D / apps-ui / core / contract change, NO hardware. `cargo check` clean ·
  service **80/80** (+7) · core 46/46. Files: `gpu_power_sweep.rs`, `gpu_verify.rs`. 2B.2-b.2 (real
  probe + supervised `--confirm`) separately gated.
- **F1b Phase 2B.2-a — shared live-ceiling classifier (2026-06-07) — IMPLEMENTED, not pushed**:
  factored `classify_live_ceiling` (read-only) + pure `eval_ceiling_evidence` → `LiveCeilingEval`
  out of `verify_applied_curve` so the verifier and the future transient-ceiling probe (2B.2-b)
  share one classification path. **VerifyAppliedProfile output byte-identical** (same offset-presence
  `classify_curve` gate + 11C diag; voltage never affects classification). Service-internal only —
  no core/contract/apps-ui/Safe-Loop/synthesis change, no hardware, no apply/load/sweep/stress.
  `cargo check` clean · service **73/73** (+5 pure tests) · core 46/46. File:
  `crates/service/src/gpu_verify.rs`. Seeding helpers deferred to 2B.2-b (avoid dead code). 2B.2-b
  (real probe + supervised `--confirm` console entry) separately gated.
- **F1b Phase 2B.1 — pure probe-mapping prep (2026-06-07) — IMPLEMENTED, not pushed**: added pure
  `measured_to_probe` (Measured→ProbeSample, conservative: Stable only on ≥Medium clock/power
  telemetry + p5 present; SilentError/Crash/TDR→Unstable; p5 preserved 0→None; missing voltage None
  not 0) + additive `PowerSweepPoint.target_clock_mhz: Option<u32>` (serde default, backward-
  compatible, no schema bump). Phase 2A `probe_to_point` stamps the target; single-clock live sweep
  sets None. **NO hardware path, NO real probe, NO apply/sweep/stress, NO apps-ui/Safe-Loop/synthesis
  /Phase-3/11D change.** `cargo check` clean · service **68/68** (+7) · core **46/46** (+2). Files:
  `crates/service/src/gpu_power_sweep.rs`, `crates/core/src/ipc.rs`, contract, decisions/memory/
  handoff. Phase 2B.2 (real probe closure + supervised console entry) and the hardware QA run remain
  separately gated; 11D deferred to after Phase 2B.
- **Patch 11C — read-only live VF-ceiling diagnostic (2026-06-06) — IMPLEMENTED, not pushed**:
  extended the read-only verifier (`gpu_verify::verify_applied_curve` / `verify-applied`) with a pure
  `compute_curve_diag` (first modified bin idx/mv, modified vs expected count, GetStatus freq-match,
  GetStatus plateau min/max, max target overshoot/undershoot, 3 offset samples) + a single read-only
  `LiveSnapshot` (NVAPI voltage + first NVML clock/power/util/temp/limit/cap). Surfaced via additive
  `Option`/`serde(default)` fields on `ApplyVerificationStatus` + one `apply_verify_diag:` log line.
  **Classifier unchanged** (offset-presence gate; live voltage above anchor never downgrades; GetStatus
  freq diagnostic only). Exact-offset verification deferred (needs persisted stock base or validating
  the GetStatus `base` tuple). Files: `crates/service/src/gpu_verify.rs`, `crates/core/src/ipc.rs`,
  `docs/contracts/ui-backend.md`, decisions/memory/handoff. **No apply/Safe-Loop/synthesis/`apps/ui`/
  `nvml_gpu.rs` change; no hardware writes.** `cargo check` clean · service **61/61** (+9 diag) · core
  44/44. **Runtime QA** (`verify-applied`, read-only, no writes — all state-file mtimes unchanged):
  `VerifiedCurve` 62/64 offsets present, but diagnostic showed `anchor_offset=+255000`,
  `highest_bin_offset=−120000`, GetStatus plateau **1770–1830** (overshoot 45), live
  `voltage=1068 mV, clock=1815, util=6%` — consistent with both a curve-flatten-shaped offset set AND
  the open overshoot suspect; GetStatus idle noise (18/64) makes it non-conclusive (as designed).
- **Applied voltage behavior — investigation + Patch 11A docs (2026-06-06) — DOCS ONLY, not pushed**:
  confirmed (read-only) that the elastic VF ceiling (`apply_vf_ceiling`) writes **per-point
  FREQUENCY offsets** to every modern VF point at/above the deterministic `vf_table_voltage_mv`
  bin (flatten to `target_mhz`); it writes **no voltage** and does **not** hard-cap measured/rail
  voltage. `vf_table_voltage_mv` (VF/curve bin) = the deterministic apply/verify/frontier key;
  `measured_voltage_mv` / HWiNFO "GPU Core Voltage" are a different (rail, load-line/droop) domain
  and may read ABOVE the bin (idle ~1.075 V and in-game ~0.887–0.956 V for an ~850 mV bin are
  EXPECTED, not a mismatch). Nidavellir must not imply a hard voltage cap; a true cap = the legacy
  voltage-lock (TDR) path, rejected by safety-first. **Patch 11A** records this in `decisions.md` +
  `docs/contracts/ui-backend.md` (incl. a Codex wording request: drop "MHz @ mV", use "target" +
  "VF bin", keep measured voltage separate) + `handoff.md`. **No backend code, no `apps/ui`, no
  apply/verify/F1b/hardware change.** Open suspect (read-only-testable, deferred to 11C): apply
  offsets are `target − GetStatus_base` and GetStatus under-reports at idle → a plateau applied at
  idle may land above target (~1815–1830 vs ~1785, on top of normal 15 MHz boost-bin quantization).
- **Applied curve verifier — Patch A (this session) — IMPLEMENTED, not pushed**: read-only
  `VerifyAppliedProfile` IPC + `crate::gpu_verify`. Classifies the live modern VF curve vs
  the applied profile into `CurveVerification` = NotApplicable / MetadataOnly /
  VerifiedCurve / LiveMismatch / VerificationFailed. **Table-to-table only**: re-derives the
  deterministic ceiling bin via `nearest_vf_bin_at_or_above(core.voltage_mv)` (same as apply),
  reads `read_vf_curve_modern` (GetStatus) + `vf_get_point_khz` (offset corroboration, logged);
  expected = points ≥ ceiling read target ±15 MHz, ≥90% match → VerifiedCurve. **Read-only**:
  no apply/reapply/write/stress. No telemetry/load/context/stock-fingerprint yet (Patches B/C).
  Additive IPC (`ApplyVerificationStatus`), contract noted. Tests: check clean · service 26/26
  (+7 verifier). **Read-only runtime path**: `nidavellir-service.exe verify-applied` console
  subcommand runs the verifier with NO startup-recovery/heartbeat/`reapply_on_boot`/pipe server
  → no apply, no VF write (proven: `gpu_applied.json` mtime unchanged).
- **F1b Phase 2A — simulated multi-clock outer-loop scaffolding (2026-06-06) — DONE, not pushed**:
  `build_frontier(candidate_clocks, &FrontierDescent, &ForgePolicy, probe: impl Fn(u32,u32)->
  ProbeSample)` in `gpu_power_sweep.rs` proves the outer loop, per-target voltage-bin descent,
  stopping rules, known-unsafe boundary, frontier assembly, and synthesis wiring **with NO
  hardware** — the probe closure is the only seam to (future) hardware. No `load_and_measure`,
  no `apply_vf_ceiling`, no VF write, no GPU stress, no Safe Loop interaction, no real power sweep.
  Frontier points use `vf_table_voltage_mv` (deterministic bin); measured voltage stays telemetry.
  Inner loop keeps deepest stable, stops on first instability or simulated `curve_verified=false`,
  never probes below `lowest_safe_mv`. 3060 Ti (1830/1815/1740) and 4090 (2880/2860/2700) proven
  through the loop. No IPC/persistence field added. `cargo check` clean · service **52/52** (+8 sim).
  **Phase 2B (future)**: real probe closure (apply ceiling → Safe-Loop-armed dwell → offset-readback
  VerifiedCurve gate) behind a supervised/approval-gated run. **Phase 3** (knowledge re-keying)
  remains future. See `decisions.md`.
- **F1b Phase 1 — policy-driven multi-clock synthesis (2026-06-06) — DONE, not pushed**: pure
  service-internal logic in `gpu_power_sweep.rs`. `ForgePolicy` (Balanced 0.98/0.90/0.85 +
  Conservative/Aggressive presets); `synthesize_forge_profiles` now takes `&ForgePolicy` and
  applies clock floors: Godforge = highest **sustained** clock (prefers `p5_clock_mhz`, falls back
  to `clock_mhz`; ties→lowest power); Brokkr's = **max R within the Brokkr's clock floor**; Deep
  Calm = max MHz/W within the Deep Calm floor. Measured voltage is NOT a selection axis
  (`vf_table_voltage_mv` stays the deterministic apply axis). Single-clock collapse detected +
  logged (returns all three, no panic). Added `Regime` enum + pure `classify_regime` +
  `candidate_clocks` (Phase-2 helpers). **4090 example resolved: Brokkr's = 2860** (max-R-within-
  floor). No IPC/apps-ui/Safe-Loop/hardware change. `cargo check` clean · service **44/44** (F1a
  assertions unchanged, +9 F1b tests). **Phase 2 NOT started** — needs simulated outer-loop
  scaffolding before any (supervised/approval-gated) hardware multi-clock sweep. See `decisions.md`.
- **Forge action consolidation audit (2026-06-06) — recorded, no code change**: backend has two
  engine generations. **Canonical = `gpu_power_sweep.rs` (Power Sweep)**: offset + elastic VF
  ceiling, game-power dwell, no voltage lock → the Forge GPU core path (apply via
  `ApplyPower*`). **Legacy (voltage-lock, TDR risk) = `gpu_sweep_real.rs` (Real Sweep) +
  `gpu_forge_all.rs` (Forge Everything)** + the legacy `ApplyGodforge/Brokkrs/DeepCalm` trio →
  hide from normal UI, remove later (keep IPC wired for now). **Memory sweep** (`gpu_mem_sweep.rs`)
  = no core voltage lock but runs independent of the forged core → Advanced Diagnostic until the
  VRAM redesign. **Product action model**: primary = **Forge GPU** (→ **Refine Profiles** once
  profiles exist); **Advanced Diagnostics** = GetGpuCurve / StartGpuValidation / StartBenchmark /
  VerifyAppliedProfile / StartMemSweep; legacy paths hidden/developer-only. VRAM = future Forge GPU
  pipeline step, never a separate primary button. See `decisions.md` + `docs/contracts/ui-backend.md`.
- **Patch B — load-state classification (2026-06-06) — DONE, not pushed**: adds a second
  orthogonal LOAD axis to `ApplyVerificationStatus` (`load_state: LoadVerification` =
  NotEvaluated / VerifiedUnderLoad / TelemetryInsufficient / LoadMismatch /
  WorkloadStateMismatch(reserved) / LoadVerificationFailed) + diagnostic dwell fields. Derived
  from the applied point's EXISTING synthetic-dwell stats (read-only `load_restored_progress()`
  reads `forge_state.json`; matches the point by label→named slot, fallback unique points entry).
  Rules: load only evaluated when curve verified; `p5_clock ≥ target−30 MHz` + `telemetry_quality
  ≥ Medium` → VerifiedUnderLoad; voltage is telemetry-only; `stable=false`→LoadMismatch; bad power
  →LoadVerificationFailed. Derivation: load upgrades VerifiedCurve→VerifiedUnderLoad, never
  downgrades. `status` stays the curve axis; additive serde-default fields. **Runtime QA**
  (`verify-applied`, read-only, no writes): curve=VerifiedCurve(63/65), load=TelemetryInsufficient
  ("legacy point without dwell quality" — persisted point predates the dwell-stats patch),
  status=verified_curve. Tests: check clean · service 35/35 (+10). Next: Forge Action Consolidation.
- **Patch A.1 — offset-based curve verification (2026-06-06) — DONE, not pushed**: runtime QA
  proved GetStatus actual-freq is unreliable at idle (it under-reported the plateau 31/65 even
  though the flatten offsets were resident 63/65). `classify_curve` now gates on the **GET-control
  offset readback** (`vf_get_point_khz`): a point ≥ ceiling counts as flattened if it carries a
  **non-zero** offset (presence, not exact value — per-point stock base isn't persisted); ≥90% →
  VerifiedCurve. GetStatus freq match is kept as logged diagnostic only. Unreadable offsets →
  VerificationFailed (safer than mismatch). **Re-ran `verify-applied`**: now `VerifiedCurve`
  (offset_match 63/65, getstatus 31/65 diagnostic), no write (`gpu_applied.json` mtime unchanged).
  Tests: check clean · service 25/25 (6 offset-based verifier tests). Patch B unblocked.
- **Richer dwell stats (this session) — IMPLEMENTED, not pushed**: second patch off the
  Sensor Audit. `PowerSweepPoint` gains optional `min_clock_mhz`/`p5_clock_mhz`,
  measured-voltage `avg/min/max` + `voltage_sample_count`, `dwell_sample_count`/
  `dwell_duration_ms`, `start/end/avg_temp_c`, and `voltage_quality`/`telemetry_quality`
  (new `DwellQuality` enum: high/medium/low/unavailable). Voltage stats are now
  **ramp-filtered + sanity-checked (500–1250 mV)**; the legacy unfiltered voltage max is
  **unchanged** so the apply-key behavior is untouched. Per-point `dwell_stats:` log line.
  No UI / Safe Loop / synthesis / F1b change; additive serde-default fields (old
  `forge_state.json` loads; `PowerSweepPoint` stays `Copy`). Tests: `cargo check -p
  nidavellir-service` clean · core 44/44 · service 19/19. **Limitations**: full NVML
  limiter reasons deferred; voltage cadence still ~480 ms; no per-sample timestamps; no
  hotspot/fan; `arduous_validate` soak path doesn't yet use the richer stats.
- **Voltage field separation (this session) — IMPLEMENTED, not pushed**: first patch
  off the Sensor Audit decision. `PowerSweepPoint` now separates `measured_voltage_mv`
  (telemetry) from `vf_table_voltage_mv` (deterministic apply/frontier key); legacy
  `voltage_mv` kept for compat/display. **Apply path snaps the measured voltage to a
  real VF-table bin (`nearest_vf_bin_at_or_above`) before `apply_vf_ceiling`** — it no
  longer keys the ceiling on raw measured voltage. Persisted state stays
  backward-compatible (no schema bump; old JSON loads new optional fields as `None`;
  `VfPoint`/`gpu_applied.json` unchanged → apply re-snaps at runtime). Additive IPC
  fields noted in `docs/contracts/ui-backend.md`. Tests: `cargo check -p
  nidavellir-service` clean · gpu-nvapi 5/5 · service 15/15. **Limitations**: the
  frequency-only flatten is unchanged; the ~1062 mV unfocused/desktop state is NOT
  solved by this patch; richer dwell stats + apply verification still pending.
- **Sensor Quality Audit (this session, investigation-only — no code)**: GPU telemetry
  sources are right (NVML clock/power/cap/temp/util; NVAPI curve), but three structural
  gaps found: (1) two disconnected telemetry worlds — "sensor world" (`SensorEngine`/
  `GpuSensors`, **30 s cache, `voltage_mv` always `None`** → UI never gets GPU voltage)
  vs "sweep world" (`load_and_measure`, NVML 30 ms + NVAPI voltage ~480 ms **max**);
  (2) voltage is the weakest signal — string-parsed, sparse, max-only, then **reused as
  the deterministic apply ceiling key**; (3) one type name `voltage_mv` carries three
  incompatible meanings. **Key decision** (see `decisions.md`): split voltage into
  `vf_table_voltage_mv` (apply/frontier key) · `measured_voltage_mv` (telemetry only) ·
  `effective_rail_voltage_mv` (future). **F1b must NOT key on measured dwell voltage.**
- **Forge-state persistence (this session)**: new `forge_state.json` persists the
  final `PowerSweepProgress` (profiles, points, stock baseline) on successful sweep
  completion; startup restores the `PowerSweepHandle` from it when the GPU key
  matches (else idle). Fixes a service restart losing forged profiles/points/apply
  buttons. Backend-only; no UI, IPC, Safe Loop, synthesis or knowledge-schema change.
  **Does not** solve live VF-curve ownership/mismatch — deferred.
- Product model: 3 profiles forged from a clock×power frontier
  (Godforge/Brokkr's/Deep Calm). See `product.md`.
- **V1** continuous per-GPU stability knowledge: implemented, committed, HW-validated.
- **V2** confidence-gated selection: implemented + unit-tested, **committed** (5d72342).
- **F1a**: pure 3-profile synthesis (`synthesize_forge_profiles`) + tests —
  Godforge=clock / Brokkr's=R / Deep Calm=MHz/W; not yet wired (F1b). 6 tests pass.
  **committed** (95753de). See `decisions.md`.
- Branch **reconciled with master governance** (AGENTS.md / CLAUDE.md /
  docs/contracts/ui-backend.md + Codex UI Phases 1–3). UI owned by Codex.

## Completed work (this arc)
- **GPU-first UI Phase 1 cleanup**: Forge is now the default post-onboarding
  screen, and the large `Forge.svelte` view was split into focused UI components
  under `apps/ui/src/lib/components/forge`. No tuning, IPC, or service logic changed.
- **GPU-first UI Phase 2 IA pass**: the Forge view is now organized as GPU Hero
  Status -> Recommended Action -> Profile Comparison -> Forge Knowledge -> Forge
  Progress -> Advanced Diagnostics. Safe Loop status is surfaced with existing IPC.
- **Profile-state UX pass**: active profile cards now show `Applied ✓`, disable
  repeat apply clicks, and emphasize outcome-first expected results.
- **GPU-first UI Phase 3 visual system pass**: Forge Home now uses shared forged
  silicon tokens, reusable status badges, stronger Forge State hierarchy, profile
  identity variants, and a clearer Advanced Diagnostics disclosure. Frontend only;
  no tuning logic, IPC names, or backend contracts changed.
- **Phase 3 visual cleanup**: reduced background texture noise, compacted the GPU
  Hero into a focused control-panel summary, made the forge progression rail
  subtle, and set the desktop window to 1180x820 with a 1100x720 minimum.
- **Forge action cleanup (Codex UI pass, 2026-06-06)**: Forge Home now exposes a
  single primary Forge GPU / Refine Profiles action on the canonical Power Sweep
  path, applies only `ApplyPower*` profiles, moves curve/validation/benchmark/
  applied-profile verification/memory diagnostics into Advanced Diagnostics, and
  labels memory sweep as experimental/future pipeline work. Frontend only; no
  backend, IPC, tuning, or Safe Loop logic changed.
- **Modern NvAPI V/F curve (ClkVfPoints) read + write + apply + reset** work on
  driver 595.97 (the old `nvapi` crate's `SetClockBoostTable` is rejected). Elastic
  "VF ceiling" (Afterburner-style flatten) verified to control the live clock under
  load without a voltage lock. Integrated into `apply_core` (fallback = offset+NVML
  cap). UI shows support. Supported GPUs documented (desktop Pascal+).
- **Game-power dwell**: the sweep now stresses with the FurMark-class textured
  render (~199 W, saturates the 200 W cap like Overwatch), not the old compute load
  (~159 W, never capped). Made repeatable by bounding per-frame work.
- **Brokkr's = max efficiency (MHz/W)**, off-cap, NOT lowest voltage.
- **Continuous per-GPU knowledge (V1)**: severity-separated frontier + per-point
  stats persisted, data-driven margin (no fixed MHz). See `decisions.md`.
- 3-tier failure classification; Safe Loop reboot protection confirmed working.
- **V2 selection (this session)**: Wilson lower-bound confidence gate over
  accumulated trials; picks best `score()` (MHz/W) clearing the profile threshold
  (Balanced .85 active), else falls back to V1. Selection now reads the persisted
  knowledge (V1 only wrote it); off-cap invariant kept via an offset join. Code-only,
  no data-model/schema change. `cargo check` clean; 3 unit tests pass.

## Known issues / open questions
- A deep undervolt (+255 offset / ~855 mV) **hard-rebooted** the PC once — deep
  undervolt bugchecks (not just a recoverable TDR). Now learned + never re-probed.
- In-sweep, a HARD REBOOT does not auto-update `gpu_knowledge.json` (only
  SilentError/TDR do); a reboot is recorded via the Safe Loop boot-flag and must be
  folded into the knowledge (currently manual). → roadmap "Safe-Loop→knowledge".
- Render is heavier than real games → conservative bias (good) but exploring near
  the frontier under it still carries crash risk → SUPERVISED runs only.
- Run-to-run thermal variance in measured power (deferred refinement).
- 2 `.exe` binaries committed inflate the repo — confirm intent vs `.gitignore`/LFS.

## Next recommended actions
Post-audit sequencing (both foundation reviews now done; F1b stays on hold until 1–3):
1. **Split voltage fields + stop keying apply on measured voltage** (must-fix):
   `vf_table_voltage_mv` (apply/frontier key) vs `measured_voltage_mv` (telemetry) vs
   `effective_rail_voltage_mv` (future). Fix voltage acquisition (dense, validated,
   mean/min/max not just max).
2. **Richer dwell stats**: min/p5 clock, voltage avg/min/max, full NVML `ThrottleReasons`
   limiter, sample_count, timestamps, workload-context tag.
3. **Finalize Applied Curve Verification**: post-apply readback comparing the VF-table
   plateau via modern GetStatus (table-to-table, NOT against measured voltage); add the
   read-only verify IPC + `GpuApplyStatus.verification`.
4. **F1b** (only after 1–3): extend the safe flatten sweep to multiple target clocks →
   real game-power clock×power frontier; key by (clock + VF-table point), NOT measured
   voltage; wire `synthesize_forge_profiles` into the live sweep. Needs a supervised HW run.
5. Then F2–F7 (see `product.md` / `roadmap.md`).
6. In-game apply test (user present); optional one more supervised sweep → +240.
- Contract additions to draft on approval (`docs/contracts/ui-backend.md`, no `apps/ui`
  edits): populate `GpuSensors.voltage_mv`, add `dwell_quality`, `GpuApplyStatus.verification`,
  `workload_context`.
