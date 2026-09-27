# Nidavellir — Architecture

Windows-only. Tauri v2 desktop app (Svelte 5 UI) talks to a Rust **Core Service**
over a named pipe; the service does all hardware access (NVAPI, NVML, PawnIO).

## Components
- **apps/ui** (Svelte 5 runes, Tauri): the front end. `Forge.svelte` is the main
  tuning view; `VfChart.svelte` draws the V/F curve. i18n en/pt in `lib/i18n.js`.
- **apps/ui/src-tauri**: Tauri shell; bundles the Core Service sidecar. The NVIDIA beta installer
  has no CPU/PawnIO driver setup requirement.
- **crates/core**: hardware detection, sensors, V/F sweep types, the **Safe Loop**,
  and all IPC request/response types (`ipc.rs`). No HW writes here.
- **crates/service**: the Windows service + IPC server (`ipc_server.rs`). Owns the
  background runners: `gpu_power_sweep.rs` (the Brokkr's/Godforge engine),
  `gpu_apply.rs`, `gpu_benchmark.rs`, `gpu_sweep_real.rs`, `gpu_real.rs`.
  `shutdown.rs` owns the shared bounded console/SCM cleanup and clean-marker commit; IPC admission
  closes before cleanup, and SCM Running requires listener readiness.
- **crates/gpu-nvapi**: NVAPI access — read the V/F curve, set offsets, and the
  modern **ClkVfPoints** FFI (the VF ceiling). Most `unsafe` lives here.
- **crates/gpu-stress**: wgpu (Vulkan/DX12) loads — `run_render_stress` (steady
  FurMark-class textured render = game power), `run_vf_qualifier_stress`
  (FailureSeekingGameLoop: render/ROP/texture/compute/idle transients),
  `run_power_load` (compute), `run_combined`, bandwidth. `MixedGame` records
  BoostEdge + TextureRop + PowerRender in every frame/submit; BoostEdge and
  MixedGame use sparse GPU-side reduction/compare and accumulate every sampled
  mismatch. Each load returns a `StabilityResult` (Stable / SilentError / Crash).
- **crates/driver-pawnio**: MSR / SuperIO access via the PawnIO driver (CPU/RAM
  factory-clock detection, fan/sensor reads).

## IPC
Named pipe `\\.\pipe\NidavellirCore`. **Param-free methods** (the UI/scripts call a
method by name; state lives server-side). `scripts/ipc.ps1 -Method <Name>` is the
headless client used for sweeps/benchmarks. Requests/responses are the
`IpcRequest`/`ResponseData` enums in `core/src/ipc.rs`.

## Key subsystems
- **Safe Loop** (`core/src/safe_loop.rs`): reboot-surviving crash recovery. Arms a
  boot-flag (the tuning point) before a risky apply/measure; on reboot a still-armed
  flag means the last op crashed → don't re-apply and retain the attributed point.
  Sentinel TDRs project the owning Forge terminal as `interrupted/TdrOrCrash` without
  rewriting the raw workload row. The current boot is mutation-closed; after reboot and
  acknowledgement, legacy Resume checks the same run/build/GPU/driver. The current bounded
  discovery does not Resume after TDR; manual clean pauses retain their budget. Persists to ProgramData.
- **Mutation transaction boundary** (`tdr_sentinel.rs` + `gpu_apply.rs`): startup persists and reads
  back the Event Log seed/floor and waits for a watcher-ready handshake before reapply. Every GPU
  writer must pass checked Safe Loop/BootFlag/condemnation reads, own the exact BootFlag transaction,
  revalidate immediately before the write and clear only that owner. F2 Apply/Benchmark also require
  exact GPU/run/contract32 plus the complete ordered matrix proof. Corruption is never absence.
- **Live F2 Forge** (`gpu_power_sweep.rs`, `gpu_undervolt.rs`, `qualified_search.rs`,
  current2026-09-26): stock-VF-top-first search7 (not heavy stock p5; power-bound jumps to the measured
  equilibrium voltage), then economic candidates within90–100% of qualified top.24 admissions/8h
  Standard, persistent admission readback before hardware. Discovery9/Frontier32/ExactApply35 require
  clock in nominal..nominal+15, contained voltage, heavy-phase target held (or at the board power
  limit) and the representative PowerRender load strictly below the board limit. Missing evidence is not
  instability; generic inconclusive does not establish a hardware boundary. Full same-pair/run/GPU
  matrix and confirmed stock cleanup precede refinement/publication; no hidden lane retries.
  Detailed transitions, thresholds and remaining measurement limits:
  docs/qualification-rules-2026-09-25.md. No global-optimum or universal game-stability claim.
- **Concurrent integrity oracle** (`gpu-stress`): secondary TextureRop canary uses stock golden
  and an independent device on the same adapter/backend. First peer failure stops further
  submissions and strongest verdict wins; both workers join before returning to stock cleanup.
  Texture/Endurance r4 fingerprints invalidate earlier proof. Native driver waits can still hang.
- **Anchored VF undervolt** (`gpu-nvapi`): raises exactly one real lower-voltage
  anchor and caps higher-voltage bins to the target via per-point ClkVfPoints
  offsets, applies a max-only NVML clock ceiling, then sets and verifies the exact
  NVAPI voltage rail. Clock may step down; voltage may not escape above the selected
  physical bin. Reset releases both locks and is write/readback checked.
- **Legacy F1 sweep/ceiling** (`gpu_power_sweep.rs`): retained for legacy
  `is_undervolt == false` payloads; no longer backs the live Forge button.
- **Continuous knowledge** (`gpu_power_sweep.rs`): `GpuKnowledge` per GPU — a
  severity-separated frontier + per-offset stats, persisted and accumulated across
  runs. Drives the data-driven exploration ceiling.

## Persistence (C:\ProgramData\Nidavellir\)
- `safe_loop.json` — Safe Loop record (state, consecutive_crashes, blacklist).
- `gpu_applied.json` — the currently applied profile (re-applied on boot).
- `gpu_knowledge.json` — per-GPU stability knowledge (frontier + per-point stats).
- `f2_observations.jsonl` — append-only, GPU-UUID-scoped F2 discovery/qualification evidence,
  split v7/v29/v32 contract versions, full matrix-v27 provenance, cleanup proof, coverage summaries and
  crash-safe resume checkpoints.
- `forge_state.json` — current run checkpoint, qualified points, candidate bands, spent
  admission/time counters and stop reasons; partial runs carry explicit readiness state.
- `boot_flag.json` / `heartbeat.txt` — Safe Loop liveness/boot detection.
- `condemnation_ledger.jsonl` — append-only Rigid/Quarantine hardware truth. CandidateCrash appends
  are flushed and strictly read back before terminal state can advance.
- `sentinel_baseline.txt` — durable Event Log seed/floor proven before any boot reapply.

## Platform constraints
NVIDIA-only (NVAPI). Modern VF curve needs desktop Pascal+ on a current driver
(verified 595.97 and 610.62). Falls back to global offset + NVML clock cap where unavailable.
