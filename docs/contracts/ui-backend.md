\# UI ↔ Backend Contract

## 2026-09-30: three distinct profiles, search 10 (current)

Search VERSION 10: a v9 checkpoint cannot resume. Contracts unchanged (Discovery10, Frontier33,
ExactApply38). Additive only.
- **`ForgeDiscoveryBand.level_start_voltage_mv`** (null): the voltage a lower level started at (the
  lowest pass of the level above).
- **`ForgeDiscoveryBand.clock_retries_used`** (0): a lower level that did not end two steps below
  its start moved one clock bin lower this many times (at most 2). The UI shows
  "Lower clock try n/2" while it is open.
- **New stop reason `no_distinct_profile`:** even two clock bins lower, the level never passed below
  its start, so it publishes no profile of its own.
- **Attempt budget:** 30 (was 24); the 8 h time budget is unchanged.
- **Profiles:**
  - Brokkr's floor is 92% of Godforge's clock (was 95%); Deep Calm's is 87% (was 90%).
  - Deep Calm must draw less power than Brokkr's whenever such a pair exists.

## 2026-09-29: game-margin compensation, search 9

Search VERSION 9: a v8 checkpoint cannot resume. Contracts unchanged (Discovery10, Frontier33,
ExactApply38). Additive only.
- **New band `compensated`:** placed after `performance`, labeled "Game-margin top".
  - First status is `waiting_for_top`.
  - It then either closes (`compensation_not_needed`, `compensated_top_unavailable`) or tests its
    margin pair and then its publication pair (`published`).
- **`ForgeDiscoveryBand.first_qualified_voltage_mv`** (null): the first voltage that qualified at
  the band's clock. For `performance` it is the power-free voltage.
- **`ForgeDiscoveryBand.publishing`** (false): the band is testing its publication pair. The UI
  shows "Publication check".
- **New stop reasons:** `published`, `publication_unproven`, `compensation_not_needed`,
  `compensated_top_unavailable`.
- **Profiles:** a profile needs another pass ≥36 mV lower at its clock. On load, the service may
  re-synthesize `godforge`/`brokkrs`/`deep_calm` from the run's evidence, so a field-condemned
  pair no longer blocks the other profiles.

## 2026-09-28: staircase search 8, one hot bin held

Search VERSION 8: a v7 checkpoint cannot resume. Contracts: Discovery10, Frontier33, ExactApply38. No
field removals.
- **`ForgeDiscoveryBand`:**
  - Bands run one at a time: performance, then balanced (95%), then efficiency (90%).
  - `waiting_for_top` now means "waiting for the level above".
  - Status `margin_probe_waiting` no longer occurs, and `margin_probe_used` stays false.
  - New `stop_reason` values: `integrity_edge`, `tdr_edge`, `evidence_boundary`,
    `voltage_floor_reached`, `dominated_pair_failed`, `clock_level_exhausted`,
    `tdr_budget_exhausted`.
- **`F2ActiveClockPhase`:** `target_active_us` includes one bin below the target.
  `one_bin_below_active_us` is that share of it (diagnostic), no longer added separately.
- **TDR checkpoint:** after a `tdr_edge`, `phase` is `interrupted` with the search still open.
  - `resume_block_reason` reads "reinicie o Windows e reconheça o incidente; Retomar continua em X
    MHz @ Y mV" until reboot + acknowledgement; then `resume_available` becomes true.
  - Resume continues the same `run_id`.

## 2026-09-27 (c): paced light DX11

ExactApply37, DX11 fingerprint `dx11-game-v6/active-residency-heavy-variable-paced-light`. No field
changes. The light phase (`F2ActiveClockPhase.light`, index 5) now reports `requested_duty_pct`
50 instead of 100 and runs 2/7 of the lane. DX11 phase `observed_active_us` counts back-to-back
batches of one phase as one span, so it is higher than before for the same load.

Additive: `ForgeDiscoveryBand.inconclusive_descent_used` (false). After an Inconclusive with no
qualified clock, the performance band goes back to `pending` one clock bin lower at the same
voltage, once per run. The UI can keep showing it as a normal pending band.

## 2026-09-27 (b): margin probe last, light DX11, clock×temp cells

ExactApply36 (Frontier32, Discovery9, search7 unchanged). Additive, legacy defaults:
- `ForgeDiscoveryBand.status` may be `margin_probe_waiting`: the performance band's single margin
  probe holds the last admission and runs after the economic bands.
- `F2ActiveTargetCoverage.light_target_active_ms` (0) and `F2ActiveClockPhase.light` (false): the
  DX11 lane has 6 phases; the continuous light phase must hold the exact target >= 30 s.
  New refusal `dx11_light_target_unexercised`. DX11 fingerprint `dx11-game-v5/...-light`.
- `F2QualificationPhaseMetric.clock_temp`: `[clock MHz, whole °C, samples, cap samples]` cells,
  omitted when empty.

## 2026-09-27: hot-load bin, thermal semantics and publication margin

Same contract versions. Additive, legacy defaults:
- `F2ActiveClockPhase.one_bin_below_active_us` (0): heavy-phase time one bin below target without
  the power-cap bit; held for heavy sustain only, never exposure.
- `ForgeDiscoveryBand.margin_probe_used` (false): the performance band admitted its single probe one
  bin below the qualified top. New band stop reasons: `top_margin_proven`, `top_margin_edge`,
  `top_margin_unproven`. New qualification reason: `thermal_target_coverage_low`.
- Observation `thermal_throttled` now means NVML HW thermal slowdown for new F2 dwells (was SW|HW).
- Published `godforge`/`brokkrs`/`deep_calm` come only from pairs with a lower proven voltage at the
  same clock; `points` still lists every qualified pair.

## 2026-09-26 (evening): representative-load power contract

Frontier32 / ExactApply35 / search7; Discovery9 and matrix27 unchanged. Supersedes the worst-load
energy rule in the section below; everything else there still applies.
- Only PowerRender (the representative load) must hold the target strictly below the board limit.
  Qualification lanes may reach the limit: a sample below target counts as held when NVML reports
  the SW power cap without thermal slowdown (sampled power is a 1 s average on Ampere and cannot
  decide it). Integrity, containment and cleanup are unchanged.
- Additive, legacy default 0: `F2ActiveTargetCoverage.power_limited_active_ms` and
  `F2ActiveClockPhase.power_limited_active_us`. `target_active_ms` stays real target exposure;
  the DX11 30 s / 35% exposure rule uses target + power-limited time.
- Removed (never released): `ForgeDiscoveryBand.power_bound_voltage_mv`. A PowerRender power-bound
  result now jumps straight to the lowest bin above the measured equilibrium voltage.
- Profile admissibility uses `comparison_power_p99_w` (PowerRender) < limit; `power_p99_w` keeps the
  worst lane and may sit at the limit. Search6 checkpoints cannot Resume.
- After an attributed CandidateCrash, a terminal `interrupted` progress may now carry
  `godforge`/`brokkrs`/`deep_calm`, `frontier_complete: true` and `profiles_qualified` from pairs
  proven before the crash (TDR cone excluded). Apply still fails while
  `SafeLoopStatus.recovery_pending_ack` is true; the note says profiles were published.
- Inconclusive closes only its band (band `stop_reason` `evidence_incomplete_no_boundary_inferred`).
  Before a qualified top the search stop reason becomes `qualified_top_unavailable`.

## 2026-09-26: top-first / worst-load qualification

Supersedes conflicting discovery/residency/energy rules below.
ExactApply34, search5; Discovery9/Frontier31; matrix27 unchanged. See ../qualification-rules-2026-09-25.md.
Current clock contract is nominal..nominal+15MHz with unchanged requested cap, voltage and power
bounds. Observation `max_clock_mhz` is an optional absolute sampled peak (legacy default None);
current positive evidence requires it within the envelope. p95 remains distinct. DX11 upper counts
still show nominal excursions; only beyond-envelope samples fail containment. No peak promotes
a higher profile. Old search/positive versions cannot Resume/Apply under this contract.
`ForgeDiscoverySearch.control_retries_used` defaults to0 for legacy JSON and persists the single
clean discovery same-pair retry. Repeated excursions stop with `control_reapplication_failed`.
Search3 checkpoints cannot Resume under search4; qualification/driver/reset failures still stop.
Search3 starts at the stock VF domain top instead of the heavy preheat p5. Upper candidates
remain unqualified; bounded downward transitions do not inherit stability. Search2 cannot Resume.
`ForgeDiscoveryBand.status` also supports `waiting_for_top`; economic regions start only after
performance closes with a fully qualified candidate. Current outcomes preserve exact refusal
reasons; cancelled workloads do not close a region as unstable. `F2ActiveTargetCoverage` adds
`heavy_target_proven` (legacy default false). `F2ActiveClockPhase` adds `observed_active_us` and
`target_active_us` (legacy default0), carrying bounded sampled time, not continuous HW tracing.
New stop reasons: `qualified_top_unavailable`, `evidence_incomplete_no_boundary_inferred`,
`physical_clock_domain_exhausted`, `power_integrity_boundary`. These do not prove global optimum.
Budgets remain24/8h Standard. Old saved search versions cannot Resume under this policy.


## 2026-09-24: qualification before refinement and a durable search budget

This is the historical September24 discovery contract. It supersedes the exhaustive clock/voltage descent,
post-discovery qualification and one-off economic extension described in historical sections.
The measured-power ranking and distinct-result presentation below remain in effect.

- Discovery v7, Frontier v29 and Exact Apply v32 are separate evidence contracts. Texture and
  Endurance now check their concurrent secondary context against a stock golden and propagate
  the first worker failure to its peer. Old positives remain readable but cannot qualify the
  new Frontier/Apply contract. This does not certify untested pairs or guarantee game stability.
- Each admitted pair receives one PowerRender calibration, short Texture screening and, if
  eligible, the complete ordered exact-Apply matrix: native DX11, Vulkan, DX12, Endurance.
  A complete current-run/current-GPU proof at the exact clock/voltage pair, including confirmed
  stock cleanup, is required before ordinary refinement or profile publication. An incomplete
  or inconclusive matrix is never promoted by a short pass or by another pair's result.
- Three persistent regions (`performance`, `balanced`, `efficiency`) begin from this GPU's
  stock measurements and physical curve bins. Standard permits at most 24 admitted candidates
  and an 8-hour run budget; Long scales the time allowance to its longer matrix. Admission
  reserves enough estimated time for the whole candidate before hardware work. Cleanup can
  extend beyond this scheduling budget; it is not a forced process-exit deadline.
- `PowerSweepProgress.discovery_search: Option<ForgeDiscoverySearch>` is additive and defaults
  to `None` for historical results. It describes search policy, not evidence of stability:
  `version`, `attempts_used`, `attempts_limit`, `time_budget_ms`, `elapsed_ms`, `stop_reason`,
  `integrity_errors`, `next_band_index` and `bands`. Attempts and cumulative elapsed time persist
  across Resume; cancellation/interruption does not refund an admission. The service writes
  and confirms the admission checkpoint before arming the candidate.
- Each `ForgeDiscoveryBand` contains `id`, pending `target_clock_mhz`/`voltage_mv`,
  `clock_ceiling_mhz`, `status`, `attempts`, `stop_reason`, `power_preparation_used`,
  `last_qualified_clock_mhz`, `last_qualified_voltage_mv` and `next_raise_clock`. Status is
  `pending`, `in_flight` or `closed`; none is an approval. Only the explicit last-qualified
  fields identify the last fully proven pair. A pending refinement is not that pair.
- One confirmed power-limited preparation step per region may try the next lower physical
  voltage without claiming qualification. Inconclusive evidence closes its region. An integrity
  failure closes its region; two such failures end the run. A driver or operational failure
  ends all regions. Closed regions and untested points are not automatically blacklisted.
- Global stop reasons include `attempt_budget_exhausted`, `time_budget_exhausted`,
  `driver_failure_recovery_required`, `operational_failure`, `integrity_error_budget_exhausted`,
  `all_regions_closed` and `invalid_search_plan`. Region reasons additionally include
  `physical_domain_exhausted`, `power_preparation_exhausted`, `integrity_error_region_closed`
  and `inconclusive_region_closed`. UI copy translates these reasons and shows the spent
  candidate/time budgets and each region's qualified pair independently of the log tail.
- `Validated`/`Pass` means **Stage passed**. `EligibleForQualification` means short screening
  passed and the complete matrix is pending. Only `CandidateQualified` means that candidate
  has complete proof. `PowerBoundClockDrop` is a power-limited inconclusive result;
  `Inconclusive` alone does not assert low residency. `CandidateCrash` and `OperationalFailure`
  show interruption/recovery, while `BandClosedIntegrityError` explains the regional stop.
  `SearchBudgetExhausted` discloses finite exploration, not a failed physical test.
- The progress rail is Prepare -> Test candidates -> Compare -> Restore. Qualification belongs
  inside Test candidates. `profiles_qualified` and existing backend Apply checks still gate
  publication/use; no stage label or search-policy field can enable Apply.
- `profile_search_complete` stays false for this bounded sparse search: a qualified result does
  not prove the whole economic domain was explored. The UI states that better trade-offs may
  remain untested and continues grouping objectives that select the same exact pair.
- A compatible manually paused run can Resume with its spent budget. A terminated or
  version-incompatible search cannot renew that budget through Resume; a driver failure requires
  recovery and a new run. Full Reset retains the explicit forget-all semantics below.

## Historical — 2026-09-18: comparable power, bounded economic search and distinct results

This section supersedes older F2 profile-selection descriptions requiring 1% publication
headroom or ranking by the maximum power of different Apply workloads.

- `PowerSweepPoint.comparison_power_p99_w: Option<f32>` is confirmed PowerRender p99 at
  the exact Apply anchor. F2 ranking requires this finite positive metric, including when
  the board cap is unknown. `power_p99_w` still discloses the maximum including the complete
  Apply matrix; `max_power_w` remains peak telemetry. `perf_per_watt` uses sustained p5 /
  comparison p99. This is a clock/W proxy, not game FPS or a universal savings promise.
- The configured hardware power limit is unchanged. Near-cap stress power alone no longer
  rejects a pair or skips its remaining qualification lanes. Target exposure, upper-clock
  containment, thermal/integrity evidence, four complete lanes and transaction cleanup remain
  mandatory. Contract v31 physical evidence is unchanged; missing lanes are never inferred.
  The legacy diagnostics field `publication_power_ceiling_w` now reports the configured
  board limit; `board_power_limit_observed` is telemetry, not a publication veto.
- `clock_search: ForgeClockSearch[]` persists target, last approved voltage, first known
  failure, optional policy-censored floor, completion and raw stop reason. A known failure
  may be historical; the policy floor is not a measured minimum. Export includes these
  records independently of the bounded log tail. Full Reset clears them with the checkpoint.
- `economic_extension_cmax_mhz: Option<u32>` records a single extension to the real clock
  bins down to 90% of final Godforge's sustained p5 (the selector's performance metric).
  Only unfinished clocks are visited in this extension; complete same-run Apply evidence
  remains reusable. The allowance survives Resume. Stop, unsafe termination or lack of a
  qualified Godforge does not trigger an extension. A further required extension is disclosed,
  not executed recursively.
- `profile_search_complete: bool` reports coverage of that final economic domain separately
  from `profiles_qualified` (point qualification). Missing legacy fields default to false/None.
  A completed clock search can have policy-censored voltages; these are disclosed separately.
- All three objective fields remain for IPC compatibility and explicit Apply routes. The UI
  groups identical exact Apply pairs into one card, names their shared objectives, and warns
  when economic coverage is incomplete/unknown. A qualified point remains usable without
  pretending three distinct alternatives or a global optimum were demonstrated.
- Old results without the comparison metric do not show a stock efficiency percentage.
  A new Full Reset -> Clean Run is the requested first-use acceptance path; no previous
  GPU's clock/voltage pair is encoded as a target or fallback.

## 2026-09-17: explicit Full Reset and Soft Reset semantics

- The normal BAT launches ordinary console mode without command-line development authorization.
  Each reset has one UI confirmation, which closes immediately while progress is shown.
  Completion waits for the Core response and readiness refresh, but not optional Sentinel info.
- User-directed policy change supersedes older entries that make negatives survive Full Reset.
  `ResetGpuTuningFull` now forgets all active GPU learning: profiles, legacy knowledge, every F2
  observation, checkpoint, Safe Loop blacklist/incidents/crash history and condemnation ledger.
  Derived TDR cones therefore disappear. Generated learning archives are removed too.
- `ResetGpuTuningSoft` is the old positive-learning reset: clear profiles, successful measurements,
  checkpoint, learning archives and last_validated; preserve active negative/inconclusive evidence, blacklist and incident
  history. Acknowledge the incident only after the existing durable recovery checks succeed.
- Both stop and await workers, gate concurrent Sentinel activity, confirm stock and clear only
  the owned BootFlag before clearing knowledge. Full also discards in-memory legacy results.
  Failure reports `ok:false`; no new workload is launched. Current-boot reboot requirements remain.
- Full writes `full_reset_pending` before deleting stores. Interrupted/partial erasure blocks
  tuning IPC and apply-on-boot across restarts until explicit Full Reset retry completes.
- The UI names both actions and confirms their different effects. Full warns that forgotten
  rejected pairs can be tested again. After either reset, refresh readiness and select Clean;
  a remaining reboot/development requirement is still shown. Reset does not grant authorization.
- Watchdog event cursors, operational diagnostics, developer audit and user-exported reports
  are not learning inputs and remain. No automatic import of old exports or repository evidence.
  Clean Run by itself still preserves negatives; Full Reset is the explicit forget-all action.

## 2026-09-17: exact Apply v31, native DX11 v4 active exposure

- Follow-up after run1789633001432: optional `active_target.diagnostics` adds requested maximum,
  anchor, independent refusal reasons, publication power ceiling and five per-phase clock records.
  Each phase carries active maximum/count, above-target count, bounded sampled support in
  microseconds and first-event clock/voltage/temperature/time. Optional anchor curve/offset
  snapshots are read after telemetry (at most five raw excursions per lane), not atomic evidence;
  idle excursions can consume this diagnostic budget without counting as active violations.
- A complete reset-clean DX11 upper excursion becomes `ExactApplyRejected: ClockControlExceeded`.
  The pair is excluded locally, without voltage repair/blacklist. One distinct alternate may be
  considered; a second refused pair stops with an actionable note. Same-run/GPU/current-contract
  observations restore exclusions and the budget on Resume. Physical/cleanup failures retain
  their original stop priority; missing/mismatched telemetry cannot use this continuation path.
- Simultaneous power refusal is persisted/logged independently, without hiding the control
  recurrence budget. The 1% publication margin, workload duration, clock/exposure thresholds and
  qualification version remain unchanged. Diagnostic additions do not invalidate v31 positives.
- DX11 keeps its existing total lane duration but splits it equally into five duty phases:
  continuous, 75%, 50%, 25%, continuous. Middle phases alternate checked 100 ms work windows
  with idle; these percentages describe scheduling, not reported GPU utilization. Other API
  lanes, Discovery v7, Frontier v28 and negative CandidateCrash version floor 29 are unchanged.
- GPU batch submission/completion callbacks delimit work. CPU-only checksums and idle receive
  no active exposure credit. Clock/voltage reads must fit wholly within a middle-phase work
  interval. Sample time support is bounded by the interval, adjacent sample midpoints and ±15 ms;
  sensor gaps cannot manufacture exposure. This is sampled evidence, not continuous hardware tracing.
- New optional `qualification_coverage.active_target` contains `observed_active_ms`,
  `target_active_ms`, `required_target_ms`, `sample_count`, `phases_completed`,
  `upper_clock_exceeded`. Historical absence deserializes as None, never positive proof.
  DX11 approval requires five completed phases, at least 60 s of observed active coverage,
  at least 30 s exactly at target and at least 35% target/active coverage; voltage must be sane
  and <= anchor. Any sampled upper excursion within work refuses this proof. These are initial
  conservative contract thresholds, not a universal guarantee of game stability.
- Heavy-load integrity errors retain priority. The power screen uses the worse of whole-lane
  and continuous-phase p99, so idle cannot dilute the existing 99%-of-board publication limit.
- `dx11_target_unexercised` remains raw Inconclusive; its bounded lane is not identically retried.
  The pair leaves only this run's selection as `ExactApplyRejected: TargetUnexercised`, with no
  voltage repair, condemnation, blacklist or inherited failure. Other candidates are considered.
  Missing active telemetry, incomplete phases and upper excursions remain diagnostic refusals.
- Numeric power below the cap no longer overrides a predominantly asserted limiter flag when
  inferring an old DX11 structural voltage repair, including restoration from saved observations.
- Pre-v31 positives cannot publish; current DX11 positives also require explicit persisted active
  proof. New provenance: `dx11-game-v4/active-residency-heavy-variable`. Stock control and the
  legacy detector helper retain their continuous DX11 v3 provenance. No new IPC command.

## Historical — 2026-09-16: exact Apply v30, native DX11 v3

- DX11 queues one 16-frame batch while the CPU hashes the preceding fenced staging copy.
  Render/compute goldens, checksum cadence, the 420 s window and exact 0 MHz / 35% target
  residency gate remain. Completion/cancellation drains the last batch; failed checks drain
  any queued GPU work before the Safe Loop reset. No IPC field was added.
- Exact Apply is now v30; pre-v30 positives cannot publish. Discovery v7 and Frontier v28
  remain. The matrix keeps its four lanes/durations; DX11 provenance is
  `dx11-game-v3/offscreen-rgba8-texture-depth-compute-pipelined`.
- CandidateCrash safety history has a separate minimum version, 29. Its crash budget,
  physical cone and startup reconciliation survive later positive-evidence revisions.
- A complete, reset-clean DX11 lane already above the existing 99%-of-board publication
  ceiling stops the pair as `ExactApplyPowerCeilingExceeded`, including when coverage is
  `target_residency_low`. The raw observation stays Inconclusive; no blacklist is written.
  Numeric p99 is required: a driver cap flag or isolated peak does not trigger this screen.
  Identical retries, remaining lanes and voltage increases at the same clock are skipped.
  Short/cancelled dwells, hardware failures and missing telemetry retain their own handling.
- The bounded CLI `diagnose-f2-point` records one 1830 MHz / 943 mV diagnostic under explicit
  point-scoped development authorization, then resets; it never publishes profile evidence.

## 2026-09-14: opt-in development validation

- `AuthorizeDevelopmentValidation { reason: string }` is available only when this console
  process was explicitly started with `--development-validation`. SCM/ordinary console refuse it.
  It requires the service-wide idle lease and reboot guard, no pending recovery/applied profile/
  saved checkpoint, checked history and hardware-only verified stock reset. It starts no workload.
- Returns `PowerSweep` with `start_block_reason` refreshed and optional
  `development_validation_note`. Polling exposes the same note in all themes. The note is not
  deserialized from checkpoints and is never evidence of authorization by itself.
- The authorization is audited before activation and durably claimed before the single Standard
  worker starts. Negative events/cones stay unchanged. New/changed crash evidence, completion,
  cancellation or reset ends permission; process exit never restores it. Resume, Long, other
  tuning routes and Apply are refused in this opt-in session. Read/export/stock recovery remain.
- `ExportForgeLog` includes the development status and audit path while the session is active.
  The command client has bounded IPC waits and never retries uncertain authorization requests.
  Normal Full Reset semantics and default persistent crash budget are unchanged.

> NOTE (2026-07-08): the Claude/Codex backend-frontend split was retired — Claude now owns the whole
> stack. This file is no longer a cross-agent handoff; it is REFERENCE documentation of the IPC
> surface (methods + payload shapes). Keep it current when the IPC changes.


\## 2026-08-10 (current transport contract): elevated Core, unelevated local UI

\- **Privilege boundary:** the Core service remains elevated for GPU mutation. The Tauri UI remains
  unelevated and opens a fresh local `NidavellirCore` named-pipe connection per request.
\- **Pipe authorization:** the server supplies an explicit protected DACL instead of inheriting the
  elevated process default. SYSTEM and Administrators receive full access; the local Interactive
  Users principal receives generic read/write access required by the request/response protocol.
\- **Network boundary:** `PIPE_REJECT_REMOTE_CLIENTS` rejects remote named-pipe connections. Do not
  replace the Interactive Users grant with Everyone or require the desktop UI to run as
  administrator.
\- **Regression evidence:** an unelevated live client successfully completed Ping, Safe Loop status,
  power-sweep progress and applied-profile requests against the rebuilt elevated service. The
  service suite passes 420/420.


\## 2026-08-25 (current safety contract): transactional Sentinel, CandidateCrash and reset

\- **Startup ordering:** the backend may reapply a persisted profile only after checked Event Log
  reconciliation, durable seed/floor readback and a watcher-ready handshake. Any failure keeps stock
  and latches recovery; spawning a thread alone is not readiness.
\- **CandidateCrash commit:** append is flushed and the exact Rigid v29 row must be visible through the
  strict ledger reader before Forge stores `phase = "interrupted"`. Failure stores/retains
  `needs_attention`, pending incident and raw lane so startup can repair the same transaction.
\- **Mutation routes:** F2 Benchmark uses the proof-aware F2 Apply path. Apply/reapply require exact
  GPU/run/contract29 and the required complete ordered matrix proofs. Legacy real/memory routes use
  checked Safe Loop, BootFlag and condemnation preflight, an owner-identified arm and owner-matched
  clear. Corrupt state returns an IPC failure before hardware.
\- **Soft Reset (formerly Full Reset; renamed 2026-09-17):** remeasure positive learning. Backend first
  quiesces every mutating worker, then transactionally removes validated F2 positives/profiles while
  preserving operational blacklist, Rigid/Quarantine ledger, cone and Sentinel history. As of
  2026-09-10, explicit Full Reset confirmation also acknowledges the previous incident after stock
  recovery and owned BootFlag clearing, before discarding its checkpoint. CandidateCrash requires
  the same durable exact-event ledger proof as `AcknowledgeForgeIncident`; incident history remains.
  This also resolves pending incidents left by older resets with no checkpoint. The confirmation
  explains this acknowledgement; no run starts automatically. Same-boot reboot requirements remain
  enforced by the backend. A timeout/corrupt input/partial commit returns `ok:false`.


\## 2026-08-14 (current F2 contract): Discovery7, Frontier28, ExactApply29 and finite TDR recovery

\- **Version identity:** `F2_DISCOVERY_CONTRACT_VERSION = 7`,
  `F2_FRONTIER_QUALIFICATION_CONTRACT_VERSION = 28` and
  `F2_EXACT_APPLY_QUALIFICATION_CONTRACT_VERSION = 29`. The compatibility alias
  `F2_QUALIFICATION_CONTRACT_VERSION` denotes Exact Apply v29. The four-lane recipe and semantic
  workload fingerprint remain `matrix_v27`; UI text must not call the workload “v29”.
\- **Residency semantics:** Discovery7 PowerRender and Frontier28 Texture may each accept no more than
  one adjacent physical clock bin below the requested target. ExactApply29 remains strict at the
  labeled clock/voltage pair. Frontier elasticity is discovery evidence only and cannot unlock Apply.
\- **Homogeneous DX11 structural aggregation:** three structural DX11 inconclusives of the same class
  at one exact pair aggregate to `DX11StructuralClockDrop`. It authorizes one run-local vertical repair
  for that target; a repeated token closes the target. The token is neither `Pass` nor a physical
  `Fail` and must not create Rigid/Quarantine ledger evidence.
\- **Clean semantics:** `StartPowerSweepClean` remeasures positive discovery/profile evidence with the
  Standard dwell policy. It still loads and applies effective global Rigid, Quarantine and TDR-cone
  constraints. This paragraph supersedes older statements that Clean ignores durable boundaries or
  scopes the condemnation ledger to the new run.
\- **TDR cone:** effective Rigid `CandidateCrash` entries for the GPU/current exact contract project a
  1 clock-bin : 1 voltage-bin floor down the real physical tables; overlapping projections take the
  highest voltage. A pair at/below the floor is refused before arm/write/dwell and surfaced as
  `TdrRiskGuard/CensoredBoundary`. Censorship emits no positive, physical failure or condemnation row.
  The first physical bin above the floor still requires Frontier28 evidence and Apply still requires
  Exact29. A durable CandidateCrash count greater than two fails closed before further candidate work.
\- **TDR terminal truth:** a Sentinel-attributed TDR makes the owning `PowerSweep` terminal
  `phase = "interrupted"`, `last_outcome = "TdrOrCrash"`, with publication blocked. The raw lane row
  remains unchanged; the service projects terminal truth rather than falsifying workload evidence.
  `SafeLoopStatus.gpu_reboot_required` remains true for the rest of that Windows boot.
\- **Recovery transaction:** after reboot, `AcknowledgeForgeIncident` clears the pending incident and
  recomputes resumability. The UI must then call `ResumePowerSweep` only; it must not offer a recovery
  mode selector and must never fall back to `StartPowerSweep*`. Resume retains the original run/mode
  and is compatible only with the exact service build/revision, GPU and driver. Any mismatch is shown
  as a refusal and requires an explicit new run.


\## 2026-08-11 (historical, superseded by Frontier28/ExactApply29): Discovery v7 with exact qualification v27

\- **Discovery residency:** `F2_DISCOVERY_CONTRACT_VERSION = 7`. PowerRender may classify p5 at most
  one adjacent 15 MHz boost bin below the requested target as discovery-valid. This is runtime
  elasticity evidence only; it does not certify the labeled Apply pair.
\- **Exact qualification:** Texture boundary qualification and the complete exact-Apply matrix remain
  `F2_QUALIFICATION_CONTRACT_VERSION = 27` with zero clock-drop tolerance. The UI must not describe
  the discovery-only boost-bin allowance as an Apply tolerance.
\- **Coherent run context:** pruning, live descent and summary share GPU/run-wide evidence. A good
  boundary requires current Discovery plus current Texture qualification at the exact same
  target/anchor pair. A reset-clean `ClockDrop` may be dominated only by same-run/same-GPU evidence at
  the same target and strictly lower voltage, or a harder target at the same/lower voltage. All direct
  error outcomes remain authoritative.
\- **Same-call continuation:** a dominated drop may continue without returning from the current
  discovery call, using its offset only as a bounded-writer baseline and moving to the next strictly
  lower physical VF bin. It is never displayed or persisted as a stable point.
\- **Monotonic measured projection:** targets are processed in ascending clock order. Publication
  chooses the lowest measured/currently-qualified anchor that is not below the previously selected
  anchor. Equal-voltage plateaus are valid. A target without a compatible pair is omitted; backend and
  UI must never interpolate, relabel or imply an invented voltage.
\- **Apply margin:** the candidate is exactly the next valid physical VF-table bin above the measured
  boundary, not a fixed `+12 mV`. Existing `boundary_voltage_mv` and `apply_margin_mv` continue to
  expose the measured boundary and effective non-uniform millivolt delta.
\- **Stage-aware power gate:** an unqualified candidate may enter v27 up to the numeric GPU power cap.
  After the complete gate, publication requires worst measured Apply power at or below 99% of that cap
  (1% headroom). Exceeding or missing required power evidence blocks publication, not discovery.
\- **Compatibility and next run:** v6 positive discovery evidence remains readable but cannot seed v7.
  The next acceptance attempt must use `StartPowerSweepClean`; `ResumePowerSweep` on the paused v6 run
  is incompatible. Integrated validation passes 604 workspace tests (two hardware smokes ignored),
  the production UI build and workspace Clippy with baseline warnings only. Hardware acceptance is
  not yet claimed.


\## 2026-08-04 (historical semantics; workload retained as matrix v27): qualification v27 API matrix and reboot containment

\- **Physical voltage order:** publication consumes the v7 monotonic measured projection above. Any
  inversion that remains after projection still blocks synthesis fail-closed. Equal-voltage plateaus
  remain valid measured evidence.
\- **ClockDrop safety boundary:** the v7 domination/continuation rules above supersede the earlier
  harder-target-only Resume repair. `SilentError`, `Unstable`, device loss/TDR, reset failure and
  Apply-gate failures remain unchanged.
\- **One-click behavior:** Standard/Long Forge automatically run the exact-Apply gate; users do not
  choose an API, configure a detector or decide how many times to repeat a marginal point.
\- **Required evidence:** `F2_QUALIFICATION_CONTRACT_VERSION = 27`. Publication requires current,
  same-run, reset-clean `Pass` evidence for `Dx11Game`, `Texture` (explicit Vulkan), `Dx12Game` and
  `Endurance`. A missing or `Inconclusive` lane blocks Apply; a physical `Fail` rejects the pair.
\- **Backend identity:** Vulkan and DX12 are explicitly selected rather than left to wgpu adapter
  preference. Each captures independent stock goldens and runs the identical `V8Texture` recipe and
  duration. Evidence provenance records the actual render backend.
\- **DX11 v2:** native D3D11 now uses a 1536×1536 sampled-texture render, alpha/ROP, D24 depth,
  pixel ALU, compute/UAV and copy/readback. It queues 16 frames between paired framebuffer/compute
  checks instead of flushing and waiting every frame.
\- **Order and duration:** exact Apply runs DX11 v2 resident for 420 s first, then Vulkan, DX12 and
  Endurance. Standard uses 120 s for Vulkan/DX12 and 300 s Endurance (960 s total); Long uses 300 s
  for Vulkan/DX12 and 1,200 s Endurance (2,220 s total). ETA derives from the same finite four-entry
  ladder as execution.
\- **Environment gate:** before any candidate write, Forge captures API-specific stock goldens and
  runs stock controls for Vulkan, DX11 v2 and DX12. Backend initialization, missing checksum or
  insufficient telemetry is environment-level `Inconclusive`, never silicon failure.
\- **Compatibility:** pre-v27 positive evidence remains readable but cannot publish. All lanes are
  currently headless/offscreen; swapchain Present and process isolation are not claimed by this
  contract.
\- **TDR containment:** Sentinel hands a new driver-reset event to the active Forge or Detector Lab
  owner, which requests cooperative cancellation and preserves attribution before cleanup. No
  concurrent reset races the workload. The current Windows boot remains closed to GPU mutation.
\- **Safe Loop UI payload:** `SafeLoopStatus` adds defaulted `gpu_reboot_required: bool` and
  `gpu_reboot_event: Option<String>`. While required, Forge exposes only `Restart Windows`, hides
  recover/reset continuation actions and explains that the failed point and useful learning are
  already saved. A service restart cannot clear this guard; only a newer Windows boot can.


\## 2026-08-04 (current live diagnostic): elastic curve + Game Trace canary

\- **Backend-only apply:** `ApplyManualDiagnosticCurvePoint { target_mhz, voltage_mv }` applies an
  operator-owned anchored curve and max-clock ceiling without voltage lock. The response reuses
  `ManualDiagnosticPointStatus`; no frontend control is exposed yet.
\- **Game monitoring:** while that diagnostic boot-flag phase is active and real GPU utilization is
  at least 30%, Sentinel runs its existing ~700 ms Texture/ROP self-check every 20 seconds. Game Trace
  records the canary sequence alongside clock, voltage, power and utilization samples.
\- **Failure behavior:** a returned non-stable canary verdict or new `nvlddmkm-153` event claims the
  recovery episode once, resets/disarms the manual curve and writes Sentinel status. It does not
  blacklist, auto-bump or create profile evidence. The external monitor stops Game Trace and closes
  the stale manual-status view after observing the reset.
\- **Safety:** Detector Lab and Forge remain excluded from the manual canary path. A TDR still latches
  the existing reboot-required guard for the rest of the Windows boot.


\## 2026-08-04 (current diagnostic contract): curve envelope and workload result are separate

\- **Experimental backend-only recipe:** `StartDetectorLab.recipe` accepts `curve_v25` in addition to
  `control_v25` and `dense_v14`. It is intentionally not exposed as a production/UI recipe and never
  creates publishable evidence.
\- **Application semantics:** `curve_v25` writes the bounded anchored curve and max-only clock
  ceiling, but does not set a voltage lock. Clock and voltage may move within the driver-selected VF
  envelope. `control_v25`/`dense_v14` retain the exact voltage-lock behavior below.
\- **Classification semantics:** a voltage above the requested/resolved anchor is expected fidelity
  metadata in `curve_v25`; it is not a workload failure or an `Inconclusive`. Missing or insufficient
  voltage telemetry remains `Inconclusive`. Voltage above the selected bin remains `Inconclusive` for
  voltage-locked recipes.
\- **Journal only:** terminal records now include `workload_result`, `application_mode`,
  `anchor_voltage_escaped`, `voltage_mv.anchor`, and a nullable `voltage_mv.ceiling`. No IPC response
  shape changed.
\- **Safe finish:** `curve_v25` always resets to stock and disarms Safe Loop after a normal terminal
  result, including a workload pass. A pass means only that the detector observed no error; it does
  not validate an exact pair or qualify a profile.


\## 2026-08-04 (historical operational correction): Detector Lab interruption attribution

\- **Candidate journal:** after `point_reapplied`, the Lab flushes `candidate_recipe_start` and a
  `segment_start` with `scope: candidate` before every workload segment. Stock keeps the equivalent
  `scope: stock` records. A blocked driver call therefore leaves its active phase on disk.
\- **Recovery boundary:** a boot flag with phase `detector_lab` means an interrupted diagnostic
  experiment, not Forge or field learning. Startup stays at stock and retains the flag until
  apply-on-boot resets driver controls, but does not write blacklist/crash history or consume the
  Safe Mode crash budget.
\- **State at that change:** IPC payload shapes, the then-current v25 qualification/discovery versions,
  fingerprints, thresholds and dwell durations were unchanged. The later v26 matrix is documented
  above; the diagnostic-only `curve_v25` key remains separate.


\## 2026-07-23 (historical basis): authoritative F2 point, qualification v25

\- **Clock and voltage semantics:** active F2 discovery, qualification, exact Apply and Manual Point
  write the bounded anchored curve, apply a max-only NVML clock ceiling and then set/read back an
  exact graphics-domain voltage lock at the resolved physical VF bin. Clock remains free to step
  down, but voltage is no longer elastic and may not exceed the selected bin while the point is
  active.
\- **Fail-closed authority:** a stable workload is `Inconclusive` when voltage-lock readback fails,
  voltage telemetry is missing/insufficient, measured maximum voltage exceeds the selected bin, or
  fewer than 35% of qualification samples are at the exact target clock. The adjacent lower
  15/30 MHz boost bin no longer counts as target residency or positive discovery evidence.
\- **Evidence identity:** `F2_DISCOVERY_CONTRACT_VERSION = 6` and
  `F2_QUALIFICATION_CONTRACT_VERSION = 25`. Older positive evidence was produced under elastic
  voltage and/or adjacent-bin clock tolerance and cannot seed the new frontier or unlock Apply.
\- **Detector Lab:** the canonical key/UI label is `control_v25`; `control_v24` and `control_v23`
  remain backend compatibility aliases. Candidate execution now uses the same sampled
  qualification/authority gate as Forge. An unproven point returns stock with
  `DetectorLabStatus.result = inconclusive`, never `stable`.
\- **Reset/reapply:** every stock/recovery path releases both the NVML clock ceiling and NVAPI
  voltage lock. Boot reapply proactively clears stale driver-resident locks before any profile gate.
\- **IPC payloads:** request and response shapes are unchanged.


\## 2026-07-23 (historical): qualification v24 and persistent Field Concurrency

\- **Corrected workload:** `V8Texture`/Endurance keep a primary Texture Stack and an independent
  secondary TextureRop queue, but the secondary `GpuCtx` is created once and stays resident for the
  whole Field Concurrency phase. Repeated create/destroy churn is no longer part of candidate
  qualification.
\- **Fail-closed attribution:** secondary initialization failure, worker panic or insufficient
  secondary checksum coverage yields environment-level Inconclusive evidence. Field Concurrency is
  omitted from completed coverage, so it cannot qualify, condemn or blacklist a VF point.
\- **Evidence identity:** `F2_QUALIFICATION_CONTRACT_VERSION = 24`; fingerprints are
  `f2q-texhop-v13-r3/v13-persistent-field-concurrency` and
  `f2q-texhop-v13-r3/endurance-persistent-field-concurrency`. Pre-v24 positives cannot unlock Apply.
\- **Detector Lab:** the current recipe key and UI label are `control_v24`. The backend still accepts
  `control_v23` as a mixed-version compatibility alias and canonicalizes status/journal output to
  `control_v24`. `dense_v14` is unchanged.
\- **IPC payloads:** no request or response shape changed. `DetectorLabStatus.result` continues to use
  `environment_error` when the corrected recipe cannot produce candidate-attributable evidence.


\## 2026-07-22 (historical basis): reboot quarantine and like-for-like v23 stock control

\- **Per-boot TDR latch:** a new `nvlddmkm-153` sets `reboot_required` until Windows boots again.
  Service restart does not clear it because startup compares the newest Event Log timestamp with the
  current boot epoch. GPU-mutating starts/applies, `ResumePowerSweep` and `StartDetectorLab` fail with
  an explicit reboot message; Reset and read-only requests remain available.
\- **Production environment gate:** before any candidate, Forge captures deterministic goldens and
  executes one complete 60 s `V8Texture`/v23 sequence at stock. This includes the exact
  `Field Concurrency → TextureRop` handoff absent from the former standalone 15 s concurrency
  preflight. A stock failure aborts without candidate blacklist.
\- **Detector Lab control semantics:** `control_v23` mirrors the user-selected duration and segment
  order at stock before reapplying the manual point. Journal `segment_start` records add
  `scope: stock|candidate`; `stock_recipe_start` and `stock_recipe_result` delimit the control.
  `dense_v14` keeps golden capture only and does not pay an unrelated v23 stock dwell.
\- **TDR result:** a session-correlated Event 153 returns `result: tdr`,
  `stage: reboot_required`, preserves `failure_phase`, and appends `tdr_detected` with event timestamp,
  recovery and panic detail. This remains non-publishable and writes no blacklist/condemnation.
\- **UI:** Advanced Diagnostics labels the terminal state `TDR detected · reboot required`, disables
  manual-point/Lab starts and explains that Windows must restart. IPC payload shapes and contract-v23
  workload fingerprints are unchanged.


\## 2026-07-22 (historical basis): isolated Detector Lab v14 bake-off

\- **Advanced Diagnostics only.** Detector Lab compares the existing v23 Texture Hop
  (`control_v23`) with the experimental dense canary recipe (`dense_v14`) at an already applied and
  verified manual point. It is not part of Standard, Long, Clean Run, exact Apply or profile
  synthesis; qualification contract v23 and its production fingerprints are unchanged.
\- **Parameterized IPC:** `StartDetectorLab { recipe, duration_s }` accepts only the two recipe keys
  above and 15–600 seconds. `StopDetectorLab` is cooperative and `GetDetectorLabStatus` is read-only.
  All responses use `DetectorLab` with `DetectorLabStatus { running, recipe, target_mhz, voltage_mv,
  stage, current_phase, current_segment, duration_ms, elapsed_ms, progress_pct, result,
  failure_phase, frames, checksum_count, phase_results, out_path, note }`. Each phase result contains
  `{ phase, result, duration_ms, frames, checksum_count }`.
\- **Fail-closed evidence boundary:** lab results never append `f2_observations.jsonl`, unlock Apply,
  qualify or persist a profile, or write Safe Loop blacklist/condemnation. The append-only
  `detector-lab-<epoch>.jsonl` journal is a separate diagnostic artifact; each segment-start record is
  flushed before GPU work so a reboot can still attribute the active segment.
\- **Point lifecycle:** Start is refused without an active, verified manual point. The worker returns
  to stock for golden capture, reapplies the exact resolved physical point with a Safe Loop boot
  intent, then runs the selected recipe. Stable leaves the temporary point active for comparison;
  detected failure, Stop, panic or environment error returns stock and disarms it. A machine reset
  during the load remains covered by normal startup recovery.
\- **UI contract:** the Manual Point panel owns recipe selection, a 15–600 s duration slider,
  live stage/phase/time/progress, Stop, journal access and expandable segment evidence. The interface
  explicitly labels the result non-publishable and does not present a clean lab pass as profile proof.


\## 2026-07-22 (historical basis): qualification v23, publishable frontier and numeric cap evidence

\- **Physical discovery and profile publication are separate frontiers.** The physical frontier keeps
  the deepest current-contract Discovery pass for boundary learning. The publishable frontier selects
  the deepest candidate at each target that also passed every required current-contract qualification
  pattern at that exact clock/voltage pair. An inconclusive deeper point therefore remains useful
  hardware knowledge without displacing the last shallower qualified point used by exact Apply and
  profile synthesis.
\- **Texture/Endurance cap evidence is numeric first.** When a valid board power limit exists,
  BoostEdge is power-bound only when its measured p95 reaches 99% of that limit. The sampled
  `SW_POWER_CAP` fraction remains diagnostic and is only a fallback when no valid numeric limit is
  available. Texture Hop and Endurance require at least 20 BoostEdge samples; insufficient residence
  is retryable telemetry coverage, not silicon evidence.
\- **Texture Hop v13-r2 preserves discrimination while making coverage measurable.** BoostEdge owns
  4% of the plan, giving the 30 s Standard frontier dwell about 1.2 s of residence while retaining the
  same total dwell and Field Concurrency workload. Fingerprints are
  `f2q-texhop-v13-r2/v13-field-concurrency` and
  `f2q-texhop-v13-r2/endurance-field-concurrency`; pre-v23 positive evidence cannot unlock Apply.
\- **Power-cap hysteresis is stateful.** At or above 99% is NearCap and at or below 98% is OffCap.
  Measurements inside the 98–99% band inherit the preceding state instead of becoming a third
  terminal result; a fresh sequence starts conservatively NearCap. No threshold was weakened, no
  candidate was blacklisted from ambiguity, and IPC payload shapes did not change.


\## 2026-07-18 (historical): always-available Clean Run and temporary manual diagnostic point

\- **Clean Run is a permanent selector choice.** Command, Instrument and Workshop always expose
  Clean, alongside Standard and Long. Full Reset still selects it automatically for one run, but
  manual selection does not require a preceding reset. **Historical behavior, superseded
  2026-08-14:** the original implementation prevented pre-run blacklist/condemnation evidence from
  steering the search. Current Clean always applies effective Rigid, Quarantine and TDR-cone safety
  boundaries while remeasuring positive evidence.
\- **Parameterized IPC:** `ApplyManualDiagnosticPoint` accepts `target_mhz` and `voltage_mv`.
  `ResetManualDiagnosticPoint` returns the GPU to stock and `GetManualDiagnosticPointStatus` is
  read-only. All three responses use `ManualDiagnosticPoint` with
  `ManualDiagnosticPointStatus { active, target_mhz, requested_voltage_mv, resolved_voltage_mv,
  applied_at_epoch_ms, verified, note }`.
\- **Point semantics:** the requested voltage resolves to the nearest physical VF bin on the live
  GPU (maximum 8 mV difference). Apply uses the existing bounded anchored F2 curve writer plus the
  absolute target-clock ceiling; it does not use a hard voltage lock, start a synthetic workload,
  descend the curve, write observations or publish a profile.
\- **Superseded by v25:** the diagnostic point now also uses a verified voltage lock. The no-lock
  behavior above describes only the historical v24-and-earlier implementation.
\- **Safety and lifetime:** apply is refused in Safe Mode or while a recovery boot flag is already
  armed. The service-wide GPU lease blocks concurrent tuning operations. The manual point clears a
  previously persisted GPU profile and remains temporary, with its Safe Loop intent armed for the
  entire real-workload test. Explicit reset and graceful service shutdown restore stock and disarm
  that intent; a crash/reboot leaves it available to startup recovery.


\## 2026-07-18 (historical): qualification v22, Field Concurrency and uncapped Standard

\- **Texture Hop v13 is the early hardware discriminator.** The measured primary Texture Stack
  remains resident on one device/queue while a secondary thread repeatedly creates a fresh `GpuCtx`,
  runs the live Sentinel's 700 ms TextureRop self-check and destroys it. Compressed irregular gaps
  reproduce several independent context/scheduling overlaps within a short dwell. Primary outputs
  retain stock-golden comparison and the secondary canary uses in-run self-reference. The exact
  fingerprint is `f2q-texhop-v13-r1/v13-field-concurrency`; positive evidence from pre-v22 contracts
  cannot unlock Apply.
\- **Stock preflight separates capability from instability.** After deterministic golden capture and
  before any candidate write, the backend runs the same dual-device Field Concurrency path for 15 s
  at stock. A backend/driver that cannot sustain it aborts Forge as an environment failure and writes
  no candidate blacklist. Under a candidate, SilentError, DeviceLost and Windows TDR reject the point;
  neither context has a preventive pre-hang wall-time abort. Safe Loop recovery remains armed.
\- **Standard is compact, not time-capped.** Frontier candidates receive 30 s of Texture Hop v13. Each
  unique exact-Apply pair that remains publishable must then pass 120 s of Texture Hop plus 300 s of
  Endurance. There is no global 59/60-minute watchdog; the run closes when its planned hardware-
  derived search and required proof finish, or when Stop/failure interrupts it. Individual dwell
  durations and probe policy are unchanged.
\- **Long remains the explicit exhaustive mode.** Long retains 60 s frontier qualification plus
  300 s Texture Hop v13 and 1200 s Endurance per exact pair. Its difference from Standard is evidence
  depth, not permission to cross an arbitrary wall-clock boundary.
\- **Fast is retired as a product mode.** It is absent from every current selector and has no
  provisional publication path. `StartPowerSweepFast` remains only as a deprecated mixed-version
  wire alias and executes the Standard policy; it never restores the former Fast semantics.
\- **Primary Forge selector.** The main forge-themed Command Deck exposes Clean, Standard and Long
  beside the Forge action. Clean always remains selectable and uses the Standard compact proof; Full
  Reset merely selects it automatically for the next run. Instrument/workshop use the same set.
\- **Recovery/trace additions are additive.** Persisted applied profiles include optional
  `applied_at`; only an OC-class WER bugcheck after that timestamp can be attributed at startup.
  Legacy payloads default to `None` and cannot import older crashes. The live TDR cursor advances only
  after durable accounting/recovery returns. `game-trace-v3` rows add `sentinel_canary_active` and
  `sentinel_canary_sequence`; header/summary expose the sequence bounds. No IPC method changed.


\## 2026-07-17 (additive): organic reset handoff, honest full-gate power and simplified Forge UX

\- **Full Reset arms the experiment:** after a successful `ResetGpuTuningFull`, the frontend selects
  `clean`, so the next Forge request is `StartPowerSweepClean` without requiring the operator to
  remember the mode change. This automatic arm is one-shot: after that Clean Run reaches a terminal
  finished/provisional state, the selector returns to Standard. Historical implementations cleared
  broader active state; the current 2026-08-25 contract above supersedes that behavior and preserves
  all negative safety evidence while removing only reusable positives.
\- **Early energy-envelope refusal:** exact Apply still starts with Texture v9. If its reset-clean
  measured p99 or peak already exceeds the shared 94%-of-board-cap publication ceiling, the pair is
  removed from this run's profile selection and Endurance is skipped. This is explicitly power-bound,
  not instability: it writes no blacklist/condemnation and does not trigger vertical voltage repair.
  For Godforge, the existing closure may still calibrate the next lower clock at the same voltage
  (fast-drop); that new pair needs the complete gate.
\- **Complete-gate power basis:** a published profile now requires and scores the worst sustained p99
  from one complete current-contract Texture + Endurance gate. Endurance can therefore raise
  `power_p99_w`/`perf_per_watt`; a shorter Texture measurement can no longer make a dominated profile
  look efficient. `max_power_w` remains the maximum recorded power shown to the user and the stricter
  peak/p99 off-cap guard still applies.
\- **Stock-relative efficiency:** `PowerSweepProgress` adds defaulted
  `stock_power_p99_w: Option<f32>`, captured from the final thermally converged stock-preheat window.
  Together with `stock_clock_mhz`, it lets the UI calculate profile MHz/W improvement against this
  exact GPU's measured stock state. Missing legacy/sensor data renders as unavailable, never inferred.
\- **Progress presentation:** structured current/next task fields remain canonical, but the home UI
  intentionally renders only a progress bar, friendly current/next task cards, task elapsed/countdown,
  total elapsed/remaining/estimated-total and estimated finish. Tested points, raw candidate details,
  power targets and generated-profile telemetry do not belong in this progress surface. Forged
  profiles are separate disclosures: collapsed name + forge-themed purpose; expanded target MHz/mV,
  maximum measured power, stock-relative efficiency, MHz/W and actions.


\## 2026-07-16 (additive): explicit Forge pause/resume, structured tasks and durable condemnations

\- **Manual cooperative pause:** `StopPowerSweep` remains the existing unit method. While the
  active GPU task cooperates, progress reports `phase: "stopping"`. Only after the worker has
  stopped, restored/confirmed stock and persisted all completed F2 observations plus
  `forge_state.json`, progress becomes `phase: "paused"`. A TDR, panic, recovery Reset or Sentinel
  cancellation is not labelled as a manual pause.
\- **New explicit resume method:** `ResumePowerSweep`, wire
  `{"method":"ResumePowerSweep"}`. It returns the normal `PowerSweep` response. A plain
  `StartPowerSweep*` never implicitly resumes a manual Stop; the frontend must invoke this method.
  Resume continues the same `run_id`/`run_sequence`, so already completed evidence is reused and a
  clean-run checkpoint retains its learning mode. **Superseded safety detail:** all current modes,
  including a resumed Clean checkpoint, use the global effective negative ledger and TDR cone.
\- **Fail-closed compatibility:** a paused checkpoint is resumable only when all of the exact
  `program_version`, embedded `build_revision`, NVML GPU identity, selected adapter name, driver
  name and driver version/details match the current backend. Legacy checkpoints without that
  identity remain inspectable but cannot resume. Additive/defaulted progress fields:
  `resume_compatibility: Option<ForgeResumeCompatibility>`, `resume_available: bool` and
  `resume_block_reason: Option<String>`.
\- **Cumulative timing:** `elapsed_ms` remains cumulative across resume sessions and never restarts
  near zero. Existing `estimated_remaining_ms` / `estimated_total_upper_ms` keep their total-run
  meanings.
\- **Structured current/next work:** `PowerSweepProgress` adds defaulted
  `current_task: Option<String>`, `current_task_elapsed_ms: u64`,
  `current_task_estimated_total_ms: Option<u64>`, `next_task: Option<String>` and
  `next_task_estimated_duration_ms: Option<u64>`. Stable task IDs currently include
  `prepare_stock`, `stock_preheat`, `capture_goldens`, `frontier_descent`, `profile_synthesis`,
  `power_calibration`, `apply_qualification` and `final_stock_reset`. These fields are the canonical
  UI source; do not infer task/progress from localized log text. Current/next task IDs clear in
  non-running terminal states.
\- **Sentinel durable blacklist view:** `SafeLoopStatus` adds defaulted
  `condemnations: Vec<CondemnationEvent>`. It is the newest 100 effective (non-rehabilitated)
  append-only ledger events and includes `target_mhz`, `vf_bin_mv`, `severity`, `kind`, `run_id`,
  timestamp and optional note/GPU identity. `blacklist` remains the operational Safe Loop regions;
  Sentinel should display both because durable rigid/quarantine evidence can survive their reset.
\- **Live GPU telemetry:** `GpuSensors` adds defaulted/optional `fan_speed_pct` and
  `voltage_source`. `voltage_mv` is now populated read-only from NVAPI when sane; fan duty is the
  average of every NVML fan exposed by the card, with `nvidia-smi fan.speed` as fallback. `None`
  means unavailable and must render as such; numeric zero is a valid reading and must not be hidden.
  Existing `memory_clock_mhz`, `vram_total_mb` and `vram_used_mb` remain the VRAM speed/capacity
  contract. GPU sensor caching is one second so the live cards track the existing UI polling cadence.
\- **Live-log tone is presentation only:** the frontend may classify localized line text into
  red/neutral/green for readability, but no safety, resume, qualification or profile decision may be
  inferred from that color. All behavior continues to use the structured fields above.



\## 2026-07-16 (additive): `StartPowerSweepClean` — experimental organic clean run

\- **New IPC method** (unit method, no params, same request/response shape as `StartPowerSweep`):
  `StartPowerSweepClean`. Wire: `{"method":"StartPowerSweepClean"}`. Response, progress
  (`GetPowerSweepProgress`), stop (`StopPowerSweep`) and apply methods are all UNCHANGED.
\- **Semantics**: Standard dwell policy, but a fully ORGANIC search for algorithm evaluation during
  development. At start the backend archives `f2_observations.jsonl` + `forge_state.json` under
  `forge-archive/<run_id>/`, snapshots `safe_loop.json` and strips its GPU V/F blacklist regions,
  and reads the durable condemnation ledger RUN-SCOPED (only this run's failures). Failures during
  the run still block and steer vertical repair; ledger writes keep flowing to the global file.
  Sentinel, startup recovery and Safe Mode are unaffected. At the end the run's observations are
  copied into the same archive folder; the next clean run starts organic again.
\- **UI**: the Forge mode selector gains a fourth option `clean` ("Clean run · Experimental") →
  `StartPowerSweepClean`. Existing modes and mappings unchanged.
\- **New additive field (2026-07-17)**: `PowerSweepProgress.learning: Option<String>` —
  `"clean_run"` or `"persistent"` (`None` on legacy payloads). Printed in the run-log export
  header (`learning :`); the clean-run pre-flight also writes
  `forge-archive/<run_id>/clean-run-manifest.txt` as log-independent proof of the mode. Added
  after the 2026-07-17 run proved the live-log tail cannot evidence which policy executed.

\## Historical F2 reference (2026-07-18; superseded by the 2026-08-14 contract): contract v22, Texture Hop v13 and uncapped Standard

This section was normative for v22 and supersedes only the older dated v4/v6/v7 runtime descriptions
below it. The 2026-08-14 section at the top is current. Historical notes remain in place to explain
payload evolution. No IPC method or existing field was removed.

\- **Evidence contract v22.** Every current F2 dwell persists `evidence_provenance` with the service
  build version/revision, semantic workload fingerprint, actual selected render backend, adapter name,
  driver name/details, checksum method and stock-golden configuration/values. Pre-v22 positive evidence
  remains readable but cannot unlock Apply. Positive discovery, frontier qualification and exact-Apply
  qualification additionally require `reset_to_stock_ok == true` and `boot_flag_cleared == true`.

\- **Deterministic preheat and distinct clocks.** Before any candidate write, backend phase
  `"preheat"` runs up to six 10 s stock windows and requires two consecutive usable windows converged
  within 2 °C and 30 MHz p5, with no throttle or telemetry failure. It fails closed before tuning.
  Ctable is the sane physical-table ceiling/count, Cboost is the live maximum observed after preheat,
  and Cmax remains the first reset-clean sustainable clock proved by discovery. They are not aliases.

\- **Candidate Transaction for discovery.** Each candidate attempt arms Safe Loop and applies/verifies
  the curve once, then runs PowerRender plus any active qualification phases under that same curve and
  performs one checked stock reset/boot-flag clear. Same-curve Qualification observations are persisted
  before Discovery, preventing resume from seeing an unpaired positive discovery. A p99 retry closes the
  current transaction cleanly before the next attempt; reset/clear failure can never become positive.

\- **Power-cap hysteresis.** With a valid numeric board limit, p99 ≥99% is `NearCap`, p99 ≤98% is
  `OffCap`, and the interval between them is `Ambiguous`. The sampled cap flag is only a fallback when
  the numeric limit is unavailable. Ambiguous evidence receives bounded retries and remains
  inconclusive if it does not resolve.

\- **Interleaved MixedGame and sparse integrity.** Every MixedGame frame records BoostEdge,
  TextureRop and PowerRender as three passes in one encoder/frame/submit. BoostEdge and MixedGame run
  GPU reduction/compare every 16 frames; mismatch state accumulates across all sampled checks and
  `checksum_count` reports the checks actually executed. The UI must not describe this as 100% frame
  checksum coverage.

\- **Texture Hop v13 and mode-specific exact-Apply duration.** Texture Hop v13 enters the
  golden-checked TextureRop detector immediately, then assigns half the dwell to Field Concurrency:
  one resident primary Texture Stack plus repeatedly created independent TextureRop canary devices.
  The primary preserves cache/VRAM/power variety and rotating stock goldens; the secondary preserves
  the live canary's self-reference and device/queue churn. The standalone banded TextureStream phase
  is no longer part of this active pattern, and neither Field Concurrency context has a pre-hang
  timeout. A 15 s stock preflight proves this topology before the first candidate write.
  Standard requires 30 s at
  each frontier candidate and, for every
  unique selected `(target, Apply VF bin)`, 2 min Texture Hop + 5 min continuous Endurance. Long
  requires 60 s at the frontier and 5 min Texture Hop + 20 min Endurance. Endurance front-loads
  TextureRop, Field Concurrency and cap-slam cycles so a bad candidate can reject before its thermal
  tier; a pass still completes the selected mode's entire transaction.
  DX11 and standalone TransitionShock are legacy-readable but no longer execute in the mandatory gate,
  and current startup no longer captures a DX11 golden. Passing-pair dwell is 25 rather than 38 minutes.
  A point already above the 94% publication ceiling after Texture is removed as power-bound before
  Endurance, without blacklist; every point still eligible for publication must complete Endurance.

\- **Profile-aware vertical closure.** A reset-clean physical gate failure excludes the exact bin and
  tries every viable higher physical bin at the same clock; there is no attempt-count budget. The
  backend refreshes the effective condemnation view before each decision. Inconclusive, coverage and
  orchestration failures stop fail-closed without blacklist or inferred voltage movement. Only an
  actual exact-Apply SilentError is persisted under the silent-error quarantine kind.

\- **Profile electrical roles.** The common publication ceiling remains 94% of the numeric board cap.
  Godforge may climb the full physical voltage domain under it. Brokkr's voltage ceiling is one real
  bin below Godforge, and Deep Calm one real bin below the lowest stronger profile. After Godforge
  exhausts a clock, it may carry that voltage to the next lower real clock, but the carried pair must
  receive fresh exact-bin power calibration and the complete v22 gate before publication.

\- **Additive/defaulted `PowerSweepProgress` fields:**
  - `observed_boost_clock_mhz: Option<u32>` — Cboost observed after deterministic stock preheat.
  - `clock_table_bin_count: Option<u32>` — number of sane static physical V/F bins (Ctable domain).
  - `clock_table_ceiling_mhz: Option<u32>` — highest sane static physical V/F clock (Ctable ceiling).
  - `preheat_converged: Option<bool>` — `false` while normalization is unresolved, `true` only after
    deterministic convergence.
  - `preheat_temperature_c: Option<f32>` — converged stock temperature.
  - `stock_power_p99_w: Option<f32>` — sustained-p99 power from the final converged stock window.

  Existing `cmax_clock_mhz` retains the proved Cmax meaning. The simplified progress surface uses the
  structured task/timing fields and labels `phase == "preheat"` as stock normalization without
  exposing Ctable/Cboost candidate telemetry. Legacy/interrupted payloads deserialize new fields as
  `None`; frontend fallback must remain display-only and must not infer safety or eligibility from logs.



\## Purpose



This document defines the IPC surface between the frontend (UI/UX) and backend (GPU tuning and service layer).



The goal is to:



\- document IPC methods

\- document payloads

\- track requested cross-team changes

\- avoid breaking integrations



\---



\# Current IPC Methods



To be documented as the frontend/backend contract stabilizes.



\---



\# Change Requests



\## Frontend request (2026-06-06): Forge action consolidation (backend → Codex)



The UI currently exposes too many tuning/test buttons, several of which are LEGACY

voltage-lock paths (TDR risk) that should not be normal user actions. Backend audit

result (see `decisions.md`):



\- \*\*Primary action\*\*: a single \*\*Forge GPU\*\* (→ \*\*Refine Profiles\*\* once profiles exist).

&#x20; Canonical backend path = `StartPowerSweep` (+ progress `GetPowerSweepProgress`) and apply

&#x20; via \*\*`ApplyPowerGodforge` / `ApplyPowerBrokkrs` / `ApplyPowerDeepCalm`\*\* only.



\- \*\*Move to Advanced Diagnostics\*\* (secondary, collapsed — safe, non-primary):

&#x20; `GetGpuCurve` (Read curve), `StartGpuValidation` (Validate stability),

&#x20; `StartBenchmark` (Benchmark), `VerifyAppliedProfile`, `StartMemSweep`

&#x20; (label it "Memory sweep (experimental)").



\- \*\*Hide as legacy / developer-only\*\* (do NOT surface as normal actions; do NOT call):

&#x20; `StartForgeAll` (Forge Everything), `StartRealSweep` + `StartRealSweepFast` (Real Sweep),

&#x20; and the legacy `ApplyGodforge` / `ApplyBrokkrs` / `ApplyDeepCalm` trio (these read the

&#x20; legacy voltage-lock `real_sweep` profiles). Backend keeps these IPC methods wired for now

&#x20; (removal scheduled after F1b); the UI should simply stop exposing them.



\- \*\*VRAM optimization\*\*: represent as a \*\*future pipeline step INSIDE Forge GPU\*\*, not a

&#x20; separate primary button. VRAM tuning must run AFTER the core VF curve is forged + validated

&#x20; and adapt to it. Until the VRAM redesign, memory sweep stays under Advanced Diagnostics only.



\- \*\*Labels\*\*: `Forge GPU`, `Refine Profiles`, `Advanced Diagnostics`. Avoid exposing

&#x20; "Real sweep" / "Forge everything" as user actions.



\- \*\*Migration / compatibility\*\*: no IPC fields change; this is a visibility/labelling request.

&#x20; All listed methods remain available. Backend does not edit `apps/ui/**`.



\## Frontend request (2026-06-06): Voltage semantics wording — no hard voltage cap (backend → Codex)



The VF ceiling caps FREQUENCY, not voltage (see `decisions.md`: "Elastic VF ceiling caps

frequency, not effective voltage"). The current "MHz @ mV" wording implies a hard voltage cap

that the backend does NOT provide. Backend audit result:



\- \*\*Replace "X MHz @ Y mV"\*\* wherever it implies a voltage cap. The `Y mV` is a VF-table

&#x20; CURVE BIN (the deterministic `vf_table_voltage_mv` apply key), NOT a guaranteed rail-voltage

&#x20; ceiling. Measured / HWiNFO "GPU Core Voltage" is a different domain and may read ABOVE it.



\- \*\*Prefer\*\* wording such as `1785 MHz target · 843 mV VF bin` (or "curve bin"): "target" for

&#x20; the clock, "VF bin" / "curve bin" for the voltage.



\- \*\*Keep measured voltage separate\*\* from the deterministic VF bin. When available, show the

&#x20; measured-under-load voltage (avg/min/max from the applied point's dwell stats) as a SEPARATE

&#x20; value — never merge it into one "@ mV" figure.



\- \*\*Do NOT imply a hard effective-voltage cap\*\* anywhere in copy. Nidavellir guarantees a

&#x20; frequency plateau and preserved power-management elasticity, not a voltage ceiling.



\- \*\*Migration / compatibility\*\*: wording/labelling only. No backend methods, IPC names, or

&#x20; payload fields change. Backend does not edit `apps/ui/**`.



\## Additive (2026-06-05): PowerSweepPoint voltage fields



`PowerSweepPoint` (in `GetPowerSweepProgress` / `ApplyPower*` payloads) gains two

OPTIONAL fields (`#[serde(default)]`, backend-only, backward-compatible):



\- `measured_voltage_mv: Option<u32>` — measured effective dwell voltage (telemetry

&#x20; only; descriptive). Same source/value as the legacy `voltage_mv`.

\- `vf_table_voltage_mv: Option<u32>` — deterministic VF-table bin voltage (the apply

&#x20; key). `None` for legacy points produced before the split.



The legacy `voltage_mv` is retained for display/back-compat and still means the

measured max. UI must keep treating voltage as MEASURED telemetry, NOT as a

guaranteed cap; the deterministic key is `vf_table_voltage_mv` when present. No UI

change is required (missing optional fields tolerated). Rationale: `decisions.md`

→ "Voltage is three concepts, not one number".



\## Additive (2026-06-05): PowerSweepPoint richer dwell stats



`PowerSweepPoint` gains further OPTIONAL `#[serde(default)]` fields (backend-only,

backward-compatible; `None` on points measured before this change):



\- Clock sustainability: `min_clock_mhz`, `p5_clock_mhz` (Option<u32>).

\- Measured-voltage distribution (telemetry only, ramp-filtered + sanity-checked):

&#x20; `avg_measured_voltage_mv`, `min_measured_voltage_mv`, `max_measured_voltage_mv`,

&#x20; `voltage_sample_count` (Option<u32>).

\- Dwell meta: `dwell_sample_count` (Option<u32>), `dwell_duration_ms` (Option<u64>).

\- Temperature: `start_temp_c`, `end_temp_c`, `avg_temp_c` (Option<f32>).

\- Confidence: `voltage_quality`, `telemetry_quality` — new enum `DwellQuality`

&#x20; serializing as `"high"`/`"medium"`/`"low"`/`"unavailable"`.



These are descriptive telemetry for UI explanation/confidence. `voltage_quality`

is typically `medium` (voltage is sampled sparsely). The legacy `voltage_mv` /

`measured_voltage_mv` (max) and the deterministic `vf_table_voltage_mv` apply key

are UNCHANGED. No UI change required. Rationale: `decisions.md` Sensor Quality Audit.



\## Additive (2026-06-06): VerifyAppliedProfile (read-only curve verifier)



New OPTIONAL read-only IPC method `VerifyAppliedProfile` (Patch A — curve-only). It

reads the live modern VF curve and classifies it against the applied profile; it

NEVER applies, reapplies, or mutates GPU state. `GetAppliedProfile` stays the cheap

metadata path — verification is explicit/opt-in.



Response: `ResponseData::ApplyVerification(ApplyVerificationStatus)`:



\- `status`: enum `CurveVerification` → `"not_applicable"` / `"metadata_only"` /

&#x20; `"verified_curve"` / `"live_mismatch"` / `"verification_failed"`.

\- `live_curve_match: bool` (structured; UI must not parse `message`).

\- `label`, `target_mhz`, `vf_table_voltage_mv` (deterministic ceiling bin used for

&#x20; comparison), `legacy_voltage_mv` (diagnostic), `matched_points`,

&#x20; `expected_points`, `message`.



Comparison is table-to-table against the deterministic VF-table bin (re-derived like

apply), NOT measured voltage. `stock_detected`/`external_unknown` and live real-game

workload context are NOT included yet (later patches). Rationale: `decisions.md`

Applied Curve Verification.



\### Additive (2026-06-06): load axis (Patch B)



`ApplyVerificationStatus` gains a second, orthogonal LOAD axis derived from the applied

point's EXISTING synthetic-dwell stats (no new stress run). All additive optional fields:



\- `load_state`: enum `LoadVerification` → `"not_evaluated"` / `"verified_under_load"` /

&#x20; `"telemetry_insufficient"` / `"load_mismatch"` / `"workload_state_mismatch"` (reserved,

&#x20; not produced yet) / `"load_verification_failed"`.

\- `load_reason: Option<String>`, `telemetry_match: Option<bool>`.

\- Diagnostic dwell stats of the matched point: `p5_clock_mhz`, `min_clock_mhz`,

&#x20; `avg/min/max_measured_voltage_mv`, `voltage_sample_count`, `voltage_quality`,

&#x20; `telemetry_quality`.



`status` remains the CURVE axis. Effective headline derivation: `verified_under_load`

only when curve is `verified_curve` AND `load_state == verified_under_load`; absent/weak

load data NEVER downgrades a verified curve. `verified_under_load` here means verified

from stored synthetic-dwell stats, NOT live real-game telemetry. UI must use the

structured `status` + `load_state` fields, not parse `message`. No UI change required.



\## Additive (2026-06-06): Voltage semantics clarification (frequency-only VF ceiling)



Documentation-only clarification of fields already in the contract (no schema change):



\- The applied core profile flattens the modern VF curve to a frequency PLATEAU at/above the

&#x20; deterministic `vf_table_voltage_mv` bin via per-point FREQUENCY offsets. It writes no voltage

&#x20; and does not hard-cap measured / rail voltage in any P-state.

\- `vf_table_voltage_mv` (the VF / curve bin) is the deterministic apply / verify / frontier key.

\- `measured_voltage_mv` / `avg|min|max_measured_voltage_mv` and HWiNFO "GPU Core Voltage" are a

&#x20; DIFFERENT domain (measured rail incl. load-line / droop) — telemetry + cross-check only, and

&#x20; may legitimately read ABOVE the VF bin. Measured ≠ the bin is EXPECTED, not a mismatch.

\- `VerifyAppliedProfile` proves the frequency-flatten OFFSETS are resident (plus a load axis from

&#x20; stored dwell stats); it proves nothing about effective / measured voltage. A verified curve is

&#x20; NOT a verified voltage cap.



No UI change is required by this note (it documents existing fields); the wording request above is

the actionable UI item. Rationale: `decisions.md` → "Elastic VF ceiling caps frequency, not

effective voltage".



\## Additive (2026-06-06): VerifyAppliedProfile read-only live diagnostic (Patch 11C)



`ApplyVerificationStatus` gains OPTIONAL `#[serde(default)]` diagnostic fields (backend-only,

backward-compatible; `None` on older payloads). They are populated by the read-only verifier

(`VerifyAppliedProfile` / `verify-applied`). \*\*None of them affect `status` / classification\*\*,

and the `live_*` snapshot is telemetry only — a single read at verification time, NOT load

verification, and it does NOT imply a hard voltage cap.



Curve / offset evidence:



\- `first_modified_bin: Option<u32>`, `first_modified_mv: Option<u32>` — first plateau bin carrying

&#x20; a non-zero flatten offset, and its VF-table voltage.

\- `modified_bin_count: Option<u32>`, `expected_bin_count: Option<u32>` — modified vs expected

&#x20; (points at/above the anchor).

\- `getstatus_freq_match_count: Option<u32>` — GetStatus plateau points within tolerance of target

&#x20; (diagnostic only; GetStatus is unreliable at idle).

\- `getstatus_plateau_min_mhz` / `getstatus_plateau_max_mhz: Option<u32>` — observed plateau spread.

\- `max_target_overshoot_mhz` / `max_target_undershoot_mhz: Option<i32>` — plateau vs target

&#x20; (`Some(0)` when flat; `None` only when no plateau points).

\- `first_modified_offset_khz`, `anchor_offset_khz`, `highest_bin_offset_khz: Option<i32>` —

&#x20; representative offset samples (kHz).



Live telemetry snapshot (telemetry only; unavailable → `None`, never a fake zero):



\- `live_voltage_mv: Option<u32>` (NVAPI measured core voltage), `live_clock_mhz: Option<u32>`,

&#x20; `live_power_w: Option<f32>`, `live_utilization_pct: Option<f32>`, `live_temperature_c:

&#x20; Option<f32>`, `live_power_limit_w: Option<f32>`, `live_power_capped: Option<bool>`.

\- `diagnostic_message: Option<String>` — compact human-readable note (UI must NOT parse it for

&#x20; logic; use the structured fields).



UI is NOT required to use these now. `live_voltage_mv` may legitimately read ABOVE

`vf_table_voltage_mv` — it is measured rail telemetry, not a cap. Rationale: `decisions.md`

→ "Read-only live diagnostic for the elastic VF ceiling (Patch 11C)".



\## Additive (2026-06-07): PowerSweepPoint.target_clock_mhz (F1b Phase 2B.1)



`PowerSweepPoint` (in `GetPowerSweepProgress` / `ApplyPower*` payloads) gains one OPTIONAL

`#[serde(default)]` field (backend-only, backward-compatible):



\- `target_clock_mhz: Option<u32>` — the TARGET clock the point was probed at in the F1b

&#x20; multi-clock frontier. Distinct from `clock_mhz`, which is the MEASURED achieved clock (the

&#x20; two may differ by boost-bin behavior). `None` for single-clock / pre-2B.1 points.



No schema bump; old `forge_state.json` / `GetPowerSweepProgress` payloads load with the field as

`None`. No UI change required (UI may later show target vs measured clock). Rationale:

`decisions.md` → "F1b Phase 2B.1".



\## Frontend request (2026-06-23): Multi-clock profile discovery + confidence opt-in (backend → Codex)



Backend is building the v0.5 multi-clock frontier that finally differentiates the three

profiles. Three UI-relevant changes; all backend data is additive/optional.



\### 1. Profiles come from a MEASURED multi-clock frontier (not a single clock)



The official sweep now descends MULTIPLE clock targets (anchored at the max sustained clock,

stepping down toward the Deep Calm clock), producing a frontier of points

`(target_clock, measured_clock, p5_clock, voltage_mv, watts, confidence)`. The three profiles are

SELECTION POLICIES over that frontier (no new scoring):



\- \*\*Godforge\*\* = highest sustained clock (top of the frontier).

\- \*\*Brokkr's Best\*\* = best benefit/cost (`%power_saved ÷ %clock_lost`) keeping \*\*≥ 95%\*\* of

&#x20; Godforge's clock (relaxed from 98% → 95%, so the efficiency knee may sit up to 5% below

&#x20; Godforge for much larger watt savings).

\- \*\*Deep Calm\*\* = best MHz/W keeping \*\*≥ 90%\*\* of Godforge's clock (lowest power, still usable).



\*\*UI:\*\* present all three as distinct points (clock / mV / watts / MHz-per-watt). \*\*Honest

collapse:\*\* on a hard power-limited GPU the knee can coincide with the top — when the backend

flags `power_bound_collapse` (or Godforge and Brokkr's resolve to the same point), the UI should

say so plainly (e.g. "Brokkr's ≡ Godforge on this GPU — power-limited, no headroom above the

efficiency point") rather than imply a fake difference. Do NOT manufacture a distinction.



\### 2. Confidence is a STABILITY GATE, not a voltage margin — surface it



Why an applied point can sit ABOVE the deepest voltage the sweep reached (e.g. applied 906 mV

while the sweep validated down to 868 mV): selection is \*\*voltage-agnostic\*\* and gates each point

on accumulated \*\*Wilson stability confidence\*\* (default ≥ 0.85). A point validated only once has

low confidence (~0.21) and is NOT trusted yet; the deepest point that has earned enough repeat

confirmations wins. It is NOT a fixed safety margin.



\*\*UI:\*\* per profile point, show its \*\*confidence\*\* and \*\*validation count\*\* (e.g. "confidence 0.84

· 12 confirmations"), so the user understands a deeper point is "not yet confirmed enough" rather

than "blocked by a margin".



\### 3. Confidence opt-in: "Build confidence now" (longer run) — DEFAULT OFF



New backend option (`validation_passes`, default 1): an OPT-IN that spends a longer single session

doing extra validation passes on the deepest discovered point so it earns the confidence gate

WITHIN one session, instead of waiting across days/runs. Bounded (max 20 passes). The default

(`1`) is exactly today's behavior and is UNCHANGED.



\- \*\*Mode 1 (default, keep)\*\*: confidence accrues across normal runs over time.

\- \*\*Opt-in\*\*: user chooses a longer "build confidence now" run (more passes) to skip the wait.



\*\*UI:\*\* a clear optional control (toggle + passes/time selector) labelled as a LONGER run, with a

note that it re-validates the deepest point repeatedly (more GPU time/heat) and is optional.

Default OFF. \*\*Future (not in this delivery):\*\* automatic confidence-building while the PC is IDLE —

leave conceptual room for it but do not build it yet.



\*\*Compatibility:\*\* all backend additions are optional/additive (no payload renames/removals). The

`validation_passes` knob will need an IPC parameter when the Forge action is wired; until then it

is a service-level option. Rationale + algorithm details: `decisions.md`, `handoff.md`.



\## Frontend implementation checkpoint (2026-06-26): UI ready, IPC additions requested (Codex → backend)



Codex applied the frontend-only parts of the 2026-06-23 request:



\- Profile cards and Forge Progress use `target_clock_mhz` when available and keep measured

&#x20; `clock_mhz` / `p5_clock_mhz` separate.

\- All three profiles show clock, VF bin, watts and MHz/W.

\- Honest collapse copy uses structured `power_bound_collapse` when available, with equality of the

&#x20; Godforge/Brokkr's points as a backward-compatible fallback.

\- Per-profile stability evidence renders when the optional `confidence` and `validation_count`

&#x20; fields are present. Missing fields remain silent; the UI does not fabricate values.



The profile evidence remains optional and silent on legacy payloads. The additive backend fields
requested here were delivered in the 2026-06-27 Phase 2 contract closeout.

The delivered fields are:



\- `PowerSweepPoint.confidence: Option<f64>` — structured stability confidence (0–1) for

&#x20; the selected point. F1 uses its Wilson model; F2 uses its learned-frontier confidence model.

\- `PowerSweepPoint.validation_count: Option<u32>` — successful confirmations at that exact selected

&#x20; point. This is NOT the total observation count across other voltages or outcomes.

\- `PowerSweepProgress.power_bound_collapse: bool` (`#[serde(default)]`)

&#x20; — structured synthesis result; the UI must not infer it from logs or notes.

The start-control dependency is now delivered by the fixed Fast / Standard / Long modes documented

below. That bounded mode contract supersedes the earlier free-form `validation_passes` UI request;

Codex should wire the three explicit start methods rather than expose a numeric pass selector.



\## Frontend request (2026-06-26): Forge GPU button MODES — Fast / Standard / Long (backend → Codex)



The live multi-clock forge (`StartPowerSweep`) now supports three MODES. Two NEW additive IPC methods

select the non-default modes; the existing `StartPowerSweep` is UNCHANGED and means the proven

\*\*Standard\*\* mode. This realises the `validation_passes` "IPC parameter when the Forge action is

wired" noted in the 2026-06-23 entry — delivered as a bounded MODE, not a free-form integer.



\- \*\*New IPC methods\*\* (unit methods, no params, same shape as `StartPowerSweep`):



&#x20; `StartPowerSweepFast` and `StartPowerSweepLong`. Wire: `{"method":"StartPowerSweepFast"}`.

&#x20; Response + progress (`GetPowerSweepProgress`) + stop (`StopPowerSweep`) + apply

&#x20; (`ApplyPowerGodforge` / `ApplyPowerBrokkrs` / `ApplyPowerDeepCalm`) are all UNCHANGED.



\- \*\*Expected UI\*\*: a 3-way mode selector on the Forge GPU / Refine Profiles action. Default =

&#x20; \*\*Standard\*\* → keep sending the plain `StartPowerSweep` (no behavior change). \*\*Fast\*\* →

&#x20; `StartPowerSweepFast`. \*\*Long\*\* → `StartPowerSweepLong`. Only the START method changes per mode;

&#x20; stop / progress / apply are identical across modes.



\- \*\*Mode semantics\*\* (for toggle copy / tooltips):



&#x20; \*\*Fast\*\* — quicker discovery (fewer probes, shallower per-clock depth); ONE ceiling soak per

&#x20; profile. Cross-run confidence is left to IDLE / later manual runs. Shortest supervised run.



&#x20; \*\*Standard\*\* — today's proven, hardware-validated default. Unchanged.



&#x20; \*\*Long\*\* — broader + deeper discovery AND repeated ceiling soaks per profile, so a deep point

&#x20; earns its confidence in ONE session (no waiting for IDLE). Longest supervised run.



\- \*\*Progress payload UNCHANGED\*\*: `PowerSweepProgress` gains NO fields. The selected mode and the

&#x20; per-profile validation count appear in the `note` / `log` TEXT only (display — do NOT parse for

&#x20; logic). If a structured `mode` field is wanted, request it separately (additive).



\- \*\*Safety (reflect in copy)\*\*: all three modes run the SAME fail-closed supervised motor; every

&#x20; applied profile is validated at its discovered ceiling at least once; NOTHING is auto-applied —

&#x20; apply stays the separate `ApplyPower*` step ("confirme em jogo"). Fast only REDUCES exposure;

&#x20; Long's extra passes can only REJECT a marginal pick, never widen it.



\- \*\*Migration / compatibility\*\*: purely additive. `StartPowerSweep` keeps current behavior; no

&#x20; payload field renames/removals. Backend does not edit `apps/ui/**`. Rationale + knob values:

&#x20; `decisions.md`, `handoff.md`.



\## Backend → Frontend (2026-06-27): forge button is now F2 undervolt; Apply is REFUSED in Phase 1

The forge button's backend method PIVOTED from F1 flatten-down to \*\*F2 anchored undervolt\*\*. Reason:
the RTX 3060 Ti is power-bound (pinned at its 200 W limit), and F1 flatten-down cannot lower power on
a power-bound card. F2 holds the clock at a lower voltage and drops power directly (proven −43 W at the
same clock). F2 produces REAL differentiated Godforge / Brokkr's / Deep Calm profiles.

\- \*\*No IPC method changes.\*\* `StartPowerSweep` / `StartPowerSweepFast` / `StartPowerSweepLong`,
&#x20; `GetPowerSweepProgress`, and `ApplyPowerGodforge` / `ApplyPowerBrokkrs` / `ApplyPowerDeepCalm`
&#x20; are all unchanged in name/shape. The forge button keeps using exactly these.

\- \*\*New additive field\*\*: `PowerSweepProgress.is_undervolt: bool` (`#[serde(default)]`, false on legacy /
&#x20; pre-pivot payloads). `true` means the current forge result is an F2 undervolt profile.

\- \*\*Apply is GATED in Phase 1\*\*: when `is_undervolt == true`, the three `ApplyPower*` requests RETURN A
&#x20; FAILURE — `"F2 undervolt apply not yet wired (Phase 2) — profile discovered but not applicable"`.
&#x20; The profiles are DISCOVERED + persisted and safe to display, but cannot be applied yet (the F2 apply
&#x20; path lands in Phase 2). Until then the UI should, when `is_undervolt` is true:
&#x20; surface the 3 profiles as DISCOVERED, and either hide/disable the Apply action or show a clear
&#x20; "apply coming soon (Phase 2)" state instead of letting Apply fail silently.

\- \*\*Legacy F1 unchanged\*\*: with `is_undervolt == false` (old `real_sweep`/F1 payloads), Apply behaves
&#x20; exactly as before. This is additive + backward-compatible; no migration needed.

\- \*\*Migration / compatibility\*\*: additive field only; no renames/removals. Backend does not edit
&#x20; `apps/ui/**`. Rationale + phased plan: `decisions.md` top entry ("FORGE PIVOTS TO F2 UNDERVOLT").



\## Frontend implementation checkpoint (2026-06-27): F2 discovery state wired (Codex)

\- `PowerSweepProgress.is_undervolt` now drives a structured frontend state; the UI does not infer
&#x20; F2/apply availability from `note`, `log`, or error text.

\- When `is_undervolt == true`, all three profiles remain visible and are labelled \*\*Discovered\*\*.
&#x20; Apply controls are disabled with clear "Apply coming in Phase 2" copy, plus a defensive action
&#x20; guard prevents accidental `ApplyPower*` requests.

\- When `is_undervolt == false` or the field is missing, the existing F1 Apply behavior is unchanged.
&#x20; No Rust, IPC, persistence, profile synthesis, or hardware logic changed.

\- \*\*SUPERSEDED by the Phase 2 backend note below\*\* — Apply is now WIRED; the UI should un-gate.

\## Backend → Frontend (2026-06-27): F2 apply is WIRED (Phase 2) — Apply now applies; un-gate the UI

Phase 2 supersedes the Phase 1 "Apply is REFUSED" note above. The three apply methods now APPLY the F2
anchored undervolt when `is_undervolt == true` — they no longer return the
*"F2 undervolt apply not yet wired (Phase 2)"* failure.

\- \*\*No IPC method changes.\*\* `ApplyPowerGodforge` / `ApplyPowerBrokkrs` / `ApplyPowerDeepCalm`,
&#x20; `GetPowerSweepProgress`, `StopPowerSweep` and the Fast/Standard/Long start methods are all unchanged
&#x20; in name/shape. The response stays `ResponseData::GpuApply(GpuApplyStatus)` as before. The existing
&#x20; `core` status point carries the deterministic F2 target/anchor for UI compatibility.

\- \*\*UI action required\*\*: REMOVE the Phase-1 "apply coming soon / disabled" state for `is_undervolt`
&#x20; results. When `is_undervolt == true`, Apply Godforge/Brokkr's/Deep Calm is a normal, enabled action.
&#x20; On success the status message reads e.g. `Applied Godforge: 1800 MHz @ 875 mV VF bin (undervolt)`;
&#x20; on a fail-closed write it reads `Apply failed: …` (the GPU is reset to stock — nothing left applied).

\- \*\*Behavior\*\*: apply arms the Safe Loop, writes the anchored undervolt, VERIFIES it, persists it
&#x20; (`gpu_applied.json`, re-applied on every boot, fail-closed: a crash leaves it un-re-applied), and is
&#x20; reversible via the existing GPU reset. Still NO auto-apply — apply remains the explicit user step.

\- \*\*Legacy F1 unchanged\*\*: `is_undervolt == false` payloads still apply the F1 flatten ceiling exactly
&#x20; as before. The persisted-profile shape gains an internal `undervolt` descriptor (service-side only;
&#x20; NOT an IPC payload field). Additive + backward-compatible; no migration. Rationale: `decisions.md`
&#x20; top entry + `handoff.md`.

\## Frontend implementation checkpoint (2026-06-27): Phase 2 Apply un-gated (Codex)

\- Removed the Phase-1 disabled/"Apply coming in Phase 2" state and its defensive action guard.
\- F2 profile actions now call the unchanged `ApplyPower*` methods normally.
\- Applied-state matching uses the deterministic F2 target clock and anchor exposed through the existing
&#x20; `GpuApplyStatus.core` point; legacy F1 matching remains measured-clock based.
\- The Discovered badge remains until a profile is applied, then yields to the existing Active state.
\- Structured `confidence`, `validation_count`, and `power_bound_collapse` evidence is now delivered;
&#x20; legacy payloads continue to render without fabricated values.


\## Frontend implementation checkpoint (2026-06-26): Forge modes wired (Codex)



\- Added a compact, product-styled Fast / Standard / Long dropdown inside the Forge GPU /

&#x20; Refine Profiles split action.

\- The main segment starts the selected mode; the compact mode segment opens the selector.

\- Standard is the initial default and continues to call `StartPowerSweep`.

\- Fast calls `StartPowerSweepFast`; Long calls `StartPowerSweepLong`.

\- Stop, progress polling and profile apply paths remain unchanged.

\- Mode copy reflects discovery depth, confidence behavior, relative duration and the shared

&#x20; fail-closed supervised safety model. The UI does not parse `note` or `log` for mode state.

&#x20; \*\*SUPERSEDED by the 2026-06-28 mode-semantics note below\*\*: all modes now traverse the same

&#x20; complete frontier; copy must describe dwell/evidence, not discovery breadth.


\## Backend → Frontend (2026-06-28): corrected F2 frontier + mode semantics

The live Forge algorithm now matches the intended integrated F2 search. This note supersedes the
2026-06-26 descriptions of Fast as “fewer/shallower probes” and Long as “broader/deeper discovery.”

\- \*\*Start methods stay unchanged.\*\* Keep the existing mappings:

&#x20; Fast → `StartPowerSweepFast`; Standard → `StartPowerSweep`; Long → `StartPowerSweepLong`.

\- \*\*Identical discovery frontier in every mode.\*\* All three reset to stock and start at the highest real live-VF
&#x20; clock, discover the first sustainable Cmax through voltage descent, then characterize every real
&#x20; clock bin through 90% of Cmax. No mode tries fewer clocks or a shallower voltage range.

\- \*\*Mode semantics are evidence only:\*\*

&#x20; \*\*Fast\*\* — full-frontier discovery with 10 s dwells and no qualification pass. It produces
&#x20; a provisional preview only; `ApplyPower*` remains locked.

&#x20; \*\*Standard\*\* — 10 s discovery, then two independent 60 s reset/reapply qualification passes
&#x20; at every discovered boundary.

&#x20; \*\*Long\*\* — 10 s discovery, then three independent 120 s reset/reapply qualification passes
&#x20; at every discovered boundary. Longest run and strongest initial confidence.

\- \*\*Qualification and Apply:\*\* `PowerSweepProgress.profiles_qualified` is additive/default-false.
&#x20; The frontend must label unqualified F2 results as provisional and disable Apply. The service also
&#x20; rejects `ApplyPower*` for provisional F2 results, so stale or custom clients fail closed.

\- \*\*Expected fresh-GPU wall time:\*\* Fast ≈20–30 min, Standard ≈55–75 min, Long ≈90–120 min.
&#x20; Learned GPUs normally resume faster. These remain estimates, not deadlines.

\- \*\*Progress/safety:\*\* no definitive profiles are returned from a partial, cancelled, or
&#x20; safety-aborted run. Every mode uses the same arm→write→verify→dwell→checked-reset motor; nothing
&#x20; is auto-applied. A real run may still TDR/reboot and remains a supervised action.

\## Backend → Frontend (2026-06-28): durable F2 progress, ETA and cross-clock reuse

`GetPowerSweepProgress` gains additive, defaulted fields. Legacy payloads remain valid:

\- `mode: Option<String>`
\- `current_clock_mhz: Option<u32>`
\- `current_voltage_mv: Option<u32>`
\- `completed_steps: u32`
\- `total_steps_estimate: u32`
\- `elapsed_ms: u64`
\- `estimated_remaining_ms: Option<u64>`
\- `learned_points: u32`
\- `last_outcome: Option<String>`
\- `learning_saved: bool`
\- `frontier_complete: bool`
\- `profiles_qualified: bool`

The total and ETA are explicitly estimates: Cmax and cross-clock pruning become exact while the run
learns the frontier. The frontend must use these structured fields for the progress bar and current
target; `log` remains display-only.

Every completed candidate is appended to `f2_observations.jsonl` before the progress event says
`learning_saved`. `forge_state.json` now checkpoints live/partial progress as well as complete results,
so an interrupted service restores the run as `phase = "interrupted"` and `running = false`. A partial
run retains previous profile points for inspection but clears qualification until a complete
Standard/Long run establishes that the active boundaries are still safe. A complete Fast run may
synthesize provisional profiles, but cannot make them deployable.

The Technical Power Sweep log is permanent in Forge Progress and receives per-candidate lines for
planning, `Testing clock @ voltage`, outcome, p5/power and durable-save confirmation.

The next lower clock starts one physical VF bin above the previous clock's minimum stable anchor.
The previous clock's last power-bound `ClockDrop` remains the conservative fallback. If the optimized
warm-start cannot sustain (or plans no valid candidate), the same target retries from that fallback.
This skips known-redundant higher-voltage dwells without treating an aggressive warm-start as proof
that the clock is unsustainable.

Safe Loop semantics are also corrected: reset-clean `SilentError`/`Unstable` points are blacklisted
as frontier knowledge but do not increment `consecutive_crashes`. Only `DeviceLost`/TDR counts as a
crash and retains recovery state.

\## Backend → Frontend (2026-06-29): F2 qualification uses FailureSeekingGameLoop evidence

No IPC method, payload field, mode duration, pass count or Apply rule changes.

\- Fast and every discovery candidate continue using the steady power-heavy render. Cmax,
  near-power-limit behavior, p5 and `ClockDrop` semantics are unchanged.

\- Standard and Long reset/reapply qualification passes now use the versioned
  `FailureSeekingGameLoop`: PowerOpening, BoostEdge, HeavySpike, TextureRop, ComputeBurst,
  IdlePulse, MixedGame and PowerClosing. Each phase has independent checksum/coverage evidence.

\- Aggregate p5 from the mixed qualifier is diagnostic only and cannot produce `ClockDrop`, because
  its light phases intentionally do not represent the sustained discovery load.

\- Apply qualification now counts only current-contract qualification `Pass` evidence. Legacy or
  discovery positives may seed discovery but cannot unlock Apply. `Inconclusive` coverage does not
  mark the point bad; it retries once and then leaves the run unqualified/fail-closed.

\- A qualification `Fail` backs off automatically to the next higher physical VF bin, runs fresh
  steady `PowerRender` discovery there, and restarts all qualification passes. No manual bad-point
  registry or UI-provided prior is involved.

\- Standard/Long do not qualify old `prior_good` boundaries directly. The backend requires a fresh
  current-run `PowerRender` rediscovery before qualification can produce deployable evidence.

\- `ResetGpuTuning` remains the recovery escape hatch after TDR/interruption and is intentionally not
  blocked by the normal start/apply tuning lease. On success it resets stock, clears Safe Loop, clears
  the visible Forge checkpoint (`forge_state.json`) and returns the run view to `idle`; it does not
  erase the automatic F2 observation history. The frontend should keep Reset reachable when a run is
  stuck or Safe Loop recovery is pending.

\- Frontend action required: none. Existing Fast provisional copy, Standard/Long durations,
  `profiles_qualified` gate, progress polling and Apply behavior remain correct.



\## Backend → Frontend (2026-06-29): Reset all releases Safe Mode; new deep "forget everything" reset

Fixes the reported state where the app gets stuck in Needs Attention / Interrupted with no usable
option, persisting across manual PC restarts. Two related items — the first needs NO frontend change,
the second is a small additive request.

\- \*\*`ResetGpuTuning` now actually releases the Safe Loop latch.\*\* Previously it reset the GPU to
  stock and cleared the boot-flag + applied profile, but never rewrote `safe_loop.json`, so `safe_mode`
  and `consecutive_crashes` were effectively a one-way latch — "Reset all" could not clear a Needs
  Attention / Safe Mode state, and it survived reboots. Reset now also clears `safe_mode`, zeroes
  `consecutive_crashes` and returns Safe Loop `state` to `idle`, while PRESERVING learning (the
  unstable-region blacklist, `last_validated`, crash history) and the F2 observation frontier. UI
  effect: pressing the existing \*\*Reset all\*\* while `safe_loop.safe_mode` (or `state == "unstable"`)
  now returns the card to a normal, forgeable state. \*\*Frontend action required: none\*\* — just keep
  Reset all reachable in the Needs Attention / Interrupted branches (it already is).

\- \*\*New IPC `ResetGpuTuningFull` (additive, no params).\*\* A deeper "forget everything / start the GPU
  from zero" reset, requested alongside the normal Reset all. It does everything `ResetGpuTuning` does
  AND wipes all learning: the Safe Loop blacklist (whole record reset to default), the F2 observation
  frontier (`f2_observations.jsonl`) and legacy `gpu_knowledge.json`. Returns the same `GpuApply`
  status shape as `ResetGpuTuning`. \*\*Frontend request:\*\* add a second, clearly-secondary control near
  Reset all — e.g. "Full reset" / "Reset completo (apagar aprendizado)" — behind a stronger confirm
  dialog that spells out that learned profiles/observations are discarded. Normal Reset all stays the
  default; Full reset is the rare, destructive option.

\## Frontend implementation checkpoint (2026-06-29): post-TDR continuation wired (Codex)

\- In Needs Attention / Interrupted, the recommended action now offers \*\*Recover & continue\*\* as the
  primary path. It calls `ResetGpuTuning` to return stock and release the Safe Loop latch while
  preserving learning, then starts the selected Forge mode so the backend can continue from saved F2
  observations.

\- The existing mode picker remains available in that recovery branch, because selecting Fast/Standard/
  Long is harmless UI state and does not touch hardware until the combined recovery/start action runs.

\- `Reset all` remains a non-destructive recovery control. `Full reset` is now wired separately to
  `ResetGpuTuningFull` with a stronger confirmation that learned observations/knowledge/blacklist are
  discarded.

\- \*\*Crash accounting no longer inflated by clean restarts (informational, no payload change).\*\* A
  clean boot while already in Safe Mode no longer increments `consecutive_crashes`, and a user-initiated
  PC restart while a forge/apply was in flight is now recorded as a clean interruption (via a
  graceful-stop marker) instead of a phantom crash. `GetSafeLoopStatus.consecutive_crashes` therefore
  reads more truthfully; no field changed.



\## Backend → Frontend (2026-06-29): Cmax descent interleaves qualification (ETA may grow; no IPC change)

No IPC method or payload field changes. Standard/Long F2 discovery now qualifies each VF bin as it
descends (instead of qualifying only the deepest PowerRender point at the end), so the failure-seeking
qualifier never runs more than one bin below a proven point. Two frontend-visible effects:

\- \*\*Longer Standard/Long runs\*\* — qualification dwells now scale with the number of bins that qualify,
  not a single boundary. The existing "supervised, can take a while" framing still holds.

\- \*\*`estimated_remaining_ms` / `total_steps_estimate` start low and grow\*\* as deeper bins qualify, then
  settle. The progress bar may step backward early in a clock. These were already documented as
  estimates; no UI change is required, but avoid presenting the ETA as a firm countdown. `completed_steps`
  and the per-candidate log lines remain accurate.

\- \*\*Frontend action required: none.\*\* `profiles_qualified`, Apply gating and progress polling are
  unchanged.


\## Backend → Frontend (2026-06-30): FSGL2 default qualification (no IPC change)

No IPC method or payload field changes. This supersedes the earlier wording that a qualification
failure reruns fresh PowerRender and all qualification passes, and the temporary FSGL1 descent filter.

\- \*\*PowerRender remains measurement only.\*\* It finds sustainable/power-characterized bins and keeps
  Cmax, p5, cap and `ClockDrop` semantics comparable. It is not deployable stability evidence.

\- \*\*FSGL2 is now the descent qualifier.\*\* Standard/Long run FSGL2 pattern A 60 s and pattern B
  60 s while descending physical VF bins. FSGL1 remains available as a legacy/light profile but is not
  used by the current Standard/Long path.

\- \*\*FSGL2 is required for Apply.\*\* A deployable point must pass FSGL2 pattern A 60 s and pattern B
  60 s. `profiles_qualified == true` now means the synthesized points have current-contract FSGL2 A+B
  evidence. FSGL1-only and legacy/current discovery evidence remain provisional and keep Apply locked.

\- \*\*FSGL2 failure behavior.\*\* A real FSGL2 fail records that bin as unstable and stops the descent
  with the last FSGL2-qualified physical bin. `Inconclusive` retries once and then blocks Apply without
  marking the bin bad.

\- \*\*Frontend action required: none.\*\* Existing `profiles_qualified` UI gating, Apply enablement and
  progress polling remain correct.


\## Backend → Frontend (2026-06-30): FSGL3 golden-sample default (no IPC change)

No IPC method or payload field changes. This supersedes the FSGL2 default qualification note above.

\- \*\*FSGL3 is now the deployable qualifier.\*\* Standard/Long capture deterministic stock render
  goldens, then run FSGL3 A+B with per-frame on-GPU verification and deliberate droop bursts.
  PowerRender discovery and its Cmax/p5/`ClockDrop` semantics remain unchanged.

\- \*\*Apply now requires contract v4 FSGL3 A+B.\*\* `profiles_qualified == true` means every
  synthesized point has both current FSGL3 patterns. FSGL1/FSGL2, discovery-only and old-contract
  evidence remain provisional.

\- \*\*Stock capture may fail closed before descent.\*\* If any power/boost/texture-ROP golden is
  non-deterministic or the GPU device is lost, Forge ends at stock with a clear progress note.

\- \*\*Frontend action required: none.\*\* Existing progress polling, `profiles_qualified` gating and
  Apply enablement remain correct.



\## Backend → Frontend (2026-06-30): margin boundary, honest finish and automatic interrupted resume

The live F2 payload remains backward-compatible. Two optional `PowerSweepPoint` fields are additive:

\- `boundary_voltage_mv: Option<u32>` — learned F2 margin boundary before application policy.

\- `apply_margin_mv: Option<u32>` — effective upward margin after snapping to a physical VF bin.

`vf_table_voltage_mv` remains the exact physical bin used by Apply. For a current F2 point, UI copy
must distinguish the learned boundary from the applied VF bin instead of presenting them as the same
measurement.

`PowerSweepProgress.phase` now uses honest terminal states:

\- `finished` — complete frontier and qualified profiles; Apply may be available.

\- `provisional` — complete discovery/profile preview without qualification.

\- `incomplete` — safe partial ending; learning is preserved.

\- `interrupted` — recovery is retained. The Forge UI performs one automatic non-destructive
`ResetGpuTuning` + original `StartPowerSweep*` attempt when it reconnects. The persisted `mode` is now
the stable id `fast`, `standard` or `long`; legacy localized values remain accepted by the UI.

No new IPC method is required. Manual Stop must not be auto-resumed. Pre-hang telemetry is not a UI
safety state and must not be inferred from logs.


\## Backend → Frontend (2026-07-01): confirmed applied-bin sustained p99 and thermal validity

The live F2 payload remains backward-compatible. Existing `power_w` keeps its documented meaning as
steady-state mean power. `max_power_w` carries the real highest post-ramp PowerRender sample.
Additive `power_p99_w: Option<f32>` carries the sustained p99 and is the headline F2 profile/card
power.

\- Profile watts are calibrated at `vf_table_voltage_mv` after the unchanged application margin, not
  at `boundary_voltage_mv`.

\- F2 `perf_per_watt`, profile selection and power-bound frontier decisions use apply-bin/discovery
  p99, never mean power or the raw one-sample maximum.

\- `POWER_PEAK_PERCENTILE = 99`. P99 uses nearest-rank over every retained post-ramp sample; fewer
  than 100 samples fall back to measured raw max. No valid sample leaves p99 absent and profile
  calibration fails closed.

\- Discovery v4 compares adjacent PowerRender bins only while their p5 remains in the same clock
  regime. A p99 jump larger than both 8 W and 5% repeats the exact physical bin, with a maximum of
  three reset-clean attempts. At least two readings must agree; accepted groups use the highest
  actually measured p99. No interpolation or synthetic monotonic correction is allowed.

\- Additive observation telemetry records the attempt count/confirmation state, measured voltage
  min/avg/max/count and workload frames/FPS. A group without consensus is power-telemetry
  inconclusive and cannot enter synthesis or profile calibration.

\- `Validated` discovery still at 99% or more of the numeric cap continues to the next lower voltage
  bin. Standard/Long launch FSGL3 only from a confirmed off-cap discovery bin. FSGL3 itself, its
  golden, retry/continuity/recovery behavior and PowerRender discovery load are unchanged.

\- After the frontier is qualified and the Apply margin snaps upward, the backend fills any missing
  exact target/apply-bin p99 with a supervised discovery-only PowerRender dwell. The same v4
  anomaly/consensus rules apply. This backfill itself does not promote stability; qualification
  later runs the separate exact-Apply FSGL3 gate. Failure to confirm the backfill leaves profiles
  unavailable rather than inventing power.

\- Two optional/additive `PowerSweepPoint` fields are available: `max_temp_c: Option<f32>` and
  `thermal_throttled: bool`. Thermally throttled discovery is not eligible for profile calibration.

\- Card copy describes `power_p99_w` as measured sustained p99 and states that it is not a hard power
  limit. Frontend tolerates old payloads by falling back to `max_power_w`, then `power_w`.

\- Discovery contract is v4; v3 positive/power-bound evidence cannot enter v4 synthesis or resume.
  F2 Apply also rejects any restored profile that lacks a valid measured `power_p99_w`. The
  qualification-v4 sentence formerly here is superseded by the current contract below.

\## Backend runtime note (2026-07-01): adaptive F2 scheduling (no IPC change)

\- Compatible same-GPU discovery-v4 history and an isotonic trend over the last 3–4 qualified clocks
  may suggest the next frontier. Forge begins one physical bin above the prediction; the prediction
  is never evidence and is discarded when its inputs disagree by more than 25 mV.

\- While confirmed p99 remains at 99%+ of cap, discovery may skip 4/2/1 physical bins according to
  p5 deficit. Every jump remains bounded by 25 mV and the existing writer offset-step limit.

\- A reset-clean failure reached by a jump causes upward-only midpoint recovery. After the first
  approved off-cap point, discovery returns to adjacent-bin qualification. FSGL3, thermal handling,
  Safe Loop, Apply-bin p99 backfill, profile payloads and Apply behavior are unchanged.

\## Backend → Frontend (2026-07-02): electrical-regime reconciliation + exact-Apply v6

\- `PowerSweepPoint` adds optional/backward-compatible `p95_clock_mhz`,
  `apply_qualified` (default `false`) and `apply_qualification_version`.

\- The card keeps `target_clock_mhz` as the configured target. Display measured average, electrical
  regime p5 and sustained p95 as separate facts; neither measured percentile is a configured target.

\- A target/p5 gap beyond one 15 MHz physical bin maps to the nearest measured target at/above p5.
  The candidate inherits the maximum Apply anchor across that span. Under-anchored aliases are
  removed before synthesis; no profile power or voltage is interpolated.

\- Standard/Long set `profiles_qualified == true` only after every selected unique profile point has
  current A+B boundary evidence for its p5 regime and FSGL3 A+B evidence at its exact post-margin
  target/VF pair under qualification contract v6.
  Old/restored points lack that seal and Apply rejects them.

\- Exact Apply A and B run for five minutes each. Any inconclusive attempt requires two subsequent
  consecutive clean passes for that pattern. A reset-clean rejection also blocks lower-anchor
  aliases of the same p5 regime before backend re-synthesis; hard safety failures abort. No IPC
  method changed.

\- After A+B approval, `power_p99_w` on each selected profile is the maximum of its confirmed
  PowerRender calibration p99 and the p99 measured by the approved exact-Apply A+B pair. Frontier
  scoring remains PowerRender-homogeneous. Restored qualified v6 snapshots refresh this published
  value from `f2_observations.jsonl`; no new IPC field is required.

\## Backend ↔ Frontend (2026-07-03): automated qualification v7 + cooperative Stop

\- Qualification contract v7 replaces deployable FSGL3 A+B evidence with three automatic patterns:
  `high_fps`, `texture` and `transitions`. Standard/Long require all three at the frontier and at
  every selected exact Apply pair. Older positive qualification evidence remains readable but cannot
  unlock Apply.

\- Electrical support now uses measured `p95_clock_mhz` with zero physical-bin tolerance. `p5` remains
  the sustained performance floor; `p95` selects the highest sustained electrical regime whose
  measured Apply anchor and current v7 qualification must cover the candidate. Missing support,
  missing p95 or missing exact p99 fails closed. The highest p95 from the exact-Apply v7 set is
  reconciled again before profiles become final; a newly exposed higher regime causes re-synthesis.

\- `StopPowerSweep` is cooperative inside discovery and qualification GPU loops. Backend progress
  changes immediately to `phase == "stopping"` while the current bounded batch drains and the normal
  checked stock reset runs. A cancellation can never become positive or bad-point evidence.

\- No IPC payload field or method was removed. During a running Forge, the UI prevents overlapping
  refreshes, polls `GetPowerSweepProgress` + `GetSafeLoopStatus` at the existing fast cadence, and
  refreshes secondary diagnostics every 3 seconds. The Stop control updates optimistically to
  “Stopping…” and ignores repeated clicks.

\- The IPC-visible Forge log is bounded to its latest 240 lines to avoid cloning/serializing an
  unbounded payload. Completed measurement and qualification evidence remains durable in
  `f2_observations.jsonl`.


\## Backend ↔ Frontend (2026-07-03): stage-aware Forge time ceiling

The Forge Progress UI now presents the live remaining estimate, elapsed wall time, current estimated
run total and a separate conservative total ceiling. It does not infer Cmax or tuning policy from log
copy.

\- `elapsed_ms`, `estimated_remaining_ms`, `completed_steps`, `total_steps_estimate` and `phase`
  backward-compatible. `estimated_remaining_ms` should remain the backend's current best remaining
  estimate: frontier discovery keeps its elapsed/step self-correction, while calibration and final
  Apply use their explicit stage durations.

\- These additive/defaulted fields are now part of `PowerSweepProgress`:
  - `estimated_total_upper_ms: Option<u64>` — conservative estimated wall time from run start through
    completion. It must include work not yet appended to `total_steps_estimate`, including possible
    exact-bin power backfills and, until profile synthesis deduplicates them, up to three unique
    exact-Apply v7 qualification pairs.
  - `cmax_clock_mhz: Option<u32>` — first reset-clean sustainable real clock found by the current run.
  - `frontier_floor_clock_mhz: Option<u32>` — lowest real clock included by the 90%-of-Cmax rule.
  - `frontier_clock_count: Option<u32>` — number of real clocks in that inclusive physical domain.

\- Before Cmax, the three frontier fields and `estimated_total_upper_ms` remain `None`: a trustworthy
  inclusive 90% domain does not exist yet. As soon as Cmax is known, publish all four together and
  compute the upper estimate from the exact real-clock domain.

\- The upper estimate is not a deadline: inconclusive debt, retries or a newly exposed p95 support
  regime may raise it. It may tighten downward as uncertainty is removed, must never be below
  `elapsed_ms`, and must be refreshed when Cmax is found, a target plan is pruned/completed,
  calibration gaps are known, final Apply pairs are deduplicated, or a retry is scheduled.

\- Frontend fallback remains intentional for legacy/interrupted payloads: when
  `estimated_total_upper_ms` is absent, the UI shows “Refining” rather than manufacturing a maximum
  from duplicated tuning constants.



\## Backend ↔ Frontend (2026-07-15): Forge incident acknowledgement and field feedback

All changes are additive/defaulted. The frontend must use structured fields and must never resume an
interrupted Forge merely because the persisted phase is `interrupted`.

\- New unit request `AcknowledgeForgeIncident` releases only the acknowledgement latch. It returns the
  normal `SafeLoop` response; blacklist and incident history remain durable.

\- `SafeLoopStatus` adds `recovery_pending_ack: bool` and
  `pending_forge_incident: Option<ForgeIncident>`. When pending, Start and Apply are blocked and the
  primary action must be presented as review/continue rather than automatic recovery.

\- `PowerSweepProgress` adds `run_id: Option<String>` and ordered `run_sequence: Vec<String>`. Both
  default empty for old checkpoints. `needs_attention` is an explicit non-running phase.

\- New unit requests `ReportPowerGodforgeUnstable`, `ReportPowerBrokkrsUnstable` and
  `ReportPowerDeepCalmUnstable` resolve the chosen point from the current backend profile set. They
  add durable real-use evidence and invalidate qualification; the frontend sends no clock/voltage.

\- `ForgeLogExport` adds `run_ids: Vec<String>` and `incident_count: usize`. The human log and its
  companion JSONL are scoped to that sequence. Legacy checkpoints without run identity remain
  exportable but are labeled as legacy/global rather than presented as a clean current-run result.

\- `ResetGpuTuning` resets hardware and releases the Safe Mode latch but preserves an interrupted
  Forge checkpoint and its run identity. `ResetGpuTuningFull` remains the explicit destructive path.

(No other active backend → frontend requests)



\---



\# Rules



Backend may:

\- add new optional fields

\- add new IPC methods



Backend must not:

\- rename payload fields without updating this document

\- remove fields without migration notes



Frontend must:

\- tolerate missing optional fields

\- avoid relying on display strings for logic

\- use structured payload fields whenever possible



Frontend must not:

\- infer safety state from logs

\- infer profile state from text messages

## Beta workflow closure (2026-09-11)

- `forge-workflow.js` is the shared primary-action decision and explicit recovery coordinator.
  Recovery strictly validates every IPC envelope, confirms current status is not running/reboot-
  blocked, calls stock Reset, acknowledges, then reads updated Resume availability. It resumes only
  when the service reports compatibility; otherwise it finishes at stock with no implicit Start.
- `resume_available` is authoritative for both paused and recovered interrupted runs. Pending ACK
  shows Recover Forge, not a promise of Resume. Missing/incompatible checkpoints expose Start over.
- An in-flight action disables duplicate Start/Resume/Apply/Reset workflows. Polling may update
  state but does not clear an action error. The default Standard mode requires no mode selection.
- Node journey tests plus Playwright with an injected Tauri transport verify the UI boundary without
  starting a real service. Hardware qualification and installer lifecycle remain separate evidence.
- Native `service_request`/`service_ping` commands are asynchronous. Connection is limited to 5s;
  the single write/response exchange to 30s and 16 MiB. A lost/timed-out reply has an unknown action
  outcome and is never retried. Tokio owns/cancels pipe I/O; the service closes failed instances too.
  This deadline keeps the UI responsive; it does not prove a GPU worker/driver has stopped.
- `PowerSweep.start_block_reason` is an additive optional field recomputed by GetPowerSweepProgress
  from effective current-GPU safety history. It prevents new exploration/Resume, survives Soft Reset
  and does not prevent explicit incident recovery at stock. All starts repeat the guard. Underlying
  unreadable record/ledger errors are returned directly instead of “already running”.
- Read polling shares in-flight requests; mutations are never coalesced. NVIDIA onboarding validates
  the response and detected target before advancing; it has no CPU/PawnIO requirement. Qualified
  results require explicit Apply, and the default result view exposes ordinary Return to stock.
- `nidavellir-service acceptance-preflight` reads persisted safety and the effective crash limit,
  prints JSON including the embedded source revision, and does not enter service startup/recovery.
  A clear report still requires live Sentinel/stock/driver verification before hardware acceptance.
- Current delivery checklist: `roadmap.md`. Service exit during an active workload and isolated
  install/update/uninstall remain required evidence; do not equate pipe deadlines with those gates.

## Service lifecycle boundary (2026-09-12)

- SCM remains StartPending through startup recovery and listener creation. Running requires a
  successful pipe-ready handshake (5s); listener/ACL creation failure is fatal, while ordinary
  client disconnects remain retryable. A later listener failure triggers the same shutdown path.
- Console and SCM shutdown close IPC admission before waiting for AppState, signal all workers,
  pause the current Forge, and wait for ownership flags plus in-flight Sentinel activity. Validation
  includes context teardown in its running state. Sentinel cooldown sleeps do not own the GPU gate.
- The cleanup supervisor allows 30s for console, 20s for SCM (worker quiescence is limited to 10s).
  Timeout/error exits nonzero and does not commit a clean marker. Windows console-close/system
  shutdown may preempt this grace; the deadline is not a hardware recovery guarantee.
- The cleanup confirms required stock restoration and clears only the matching BootFlag. It
  preserves the qualified descriptor, pending incident and negative history. An untouched install
  needs no GPU reset; an unavailable driver never proves previously active tuning was restored.
- Only the supervisor can write clean_shutdown.txt after completion and an absent, readable flag.
  Late cleanup cannot commit it. After timeout the service requests process termination, with no
  replacement GPU task alongside the stalled thread. Pending kernel I/O can delay actual process
  disappearance; the marker proves application cleanup, not OS process release. The installer
  checks nonzero SCM stop codes even on retry.
- Production NVAPI wrapper calls share one successful initialization. Terminal cleanup releases
  its reference once, after admission closes and GPU users quiesce, before committing the marker.
  Unload failure/timeout is unclean; later accesses cannot reopen that runtime. A read-only short
  subprocess exit passed on2026-09-17; shutdown after a complete tuned run remains unverified.
- Controlled cleanup, late completion, native subprocess termination, worker ownership, listener
  failure and injected installer tests are software evidence. Installed SCM and GPU acceptance
  still require their own isolated/eligible environments.

## Startup bugcheck correlation (2026-09-12)

- Startup reads the latest WER SystemErrorReporting/1001 as timestamped JSON. A crash class is
  accepted only when its report time is at/after the armed BootFlag time and no later than now.
  Missing, malformed, older or future evidence yields Unknown, never a reused historical BSOD.
- This does not weaken interrupted-candidate recovery. An armed, non-clean Forge restart is still
  conservatively recorded under the current policy, even if its cause is unknown. Previously
  recorded CandidateCrash entries are not automatically removed/reclassified by this change.

## Recovery status versus a persistent exploration block (2026-09-13)

- An idle SafeLoop without recovery latches can be protected while PowerSweep.start_block_reason
  still refuses further exploration. Do not label this as an unresolved Safe Loop incident.
- After recovery gates, the primary action for start_block_reason opens Review safety block.
  It is a local disclosure only: no Start, Resume or Apply. Existing profile Apply stays disabled.
- All three themes expose the backend reason, the Full/Soft Reset distinction, ExportForgeLog as a
  diagnostic report, and a direct link to the Sentinel/history tab. Export keeps its existing IPC
  and local-file behavior; it does not transmit a report or change safety history.
- Full Reset refreshes backend readiness before announcing a new run. A remaining refusal shows
  Reset completed; tuning blocked, and does not arm the next run. Recovery acknowledgement with
  a persistent refusal likewise ends at stock with the block explained. No safety policy changed.

## Additive F2 measurement diagnostics (2026-09-16)

- Persisted/exported `F2Observation` adds optional `inconclusive_reason`. Historical records default
  to null; do not infer a specific cause from their old generic outcome. New non-power Discovery
  refusals use `discovery_inconclusive` for outcome/dwell_result. Missing/inconsistent p99 and an
  ambiguous power-cap decision retain `power_telemetry_inconclusive`. Neither is positive or bad-
  boundary evidence. Qualification keeps `qualification_inconclusive` with its specific reason.
- Causes include `voltage_telemetry_low`, `voltage_telemetry_missing`, `voltage_ceiling_exceeded`,
  `clock_ceiling_exceeded`, `thermal_throttled`, `thermal_clock_drop`, `power_telemetry_missing`,
  `power_p99_inconsistent`, `power_cap_ambiguous`, `cancelled`, and existing coverage reasons.
- `F2QualificationPhaseMetric` adds optional `sample_count` and `clock_max`. Missing historical
  values stay unknown. Maximum is the highest retained sample, not a continuous hardware bound;
  count is measured, not inferred from intended polling cadence or phase duration.
- ExportForgeLog's text table includes reason; its raw JSONL includes the additive fields. No new
  IPC command, tuning authorization, shader or qualification threshold is introduced.
