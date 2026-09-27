# GPU Forge — real GPU tuning (v0.3, NVAPI)

This documents the real, hardware-level GPU tuning built on the `feat/v0.3-nvapi`
work: how it characterizes, validates, applies and persists CPU/GPU profiles,
the methodology, the problems we hit, and how we solved them.

> Status: **real hardware** (NVIDIA / NVAPI). No simulation. Verified on an
> RTX 3060 Ti + i7-13700K dev rig. Deployability is decided automatically by the
> conservative qualification contract, but no finite synthetic suite proves every game/driver
> path. Runtime field failures remain first-class Safe Loop evidence and can tighten the next Forge;
> manual voltage knowledge is never encoded as source policy.

## Components

| Crate / module | Role |
|---|---|
| `crates/gpu-nvapi` | NVAPI bindings (via the `nvapi` crate). Read V/F curve; write core clock offset, core voltage lock, memory clock offset; `reset_all`. |
| `crates/gpu-stress` | Real GPU compute/render battery via **wgpu** (actual selected backend/adapter/driver recorded per F2 dwell): known-answer ALU, render/ROP/texture, memory-bound VRAM, pointer-chase, bandwidth and combined core+mem. |
| `crates/service/gpu_real.rs` | `Validate stability` battery (VRAM + ALU + memory + mixed). |
| `crates/service/gpu_sweep_real.rs` | Core undervolt/OC sweep (lock voltage, raise clock, combined load, Phase-E soak). |
| `crates/service/gpu_mem_sweep.rs` | Memory sweep — finds the GDDR6 **effective-bandwidth peak** with consistency + combined soak. |
| `crates/service/gpu_apply.rs` | Apply a profile to hardware, **persist**, and **re-apply on boot** (Safe Loop gated). |
| `crates/service/gpu_forge_all.rs` | One-click full pipeline. |
| `apps/ui` (Forge tab) | Read curve (chart), validate, sweeps (terminal log), apply, "Forge everything". EN/PT i18n. |

## The methodology (order matters)

```
1. VRAM integrity gate (stock)         — bad memory? stop, tuning won't help.
2. Core undervolt/OC  (combined load)  — Vcore is the shared rail; fix it FIRST.
3. Apply core.
4. Memory bandwidth peak (combined, at the applied core) — valid vs final Vcore.
5. Final whole-package soak (core+mem together) — the real-world judge.
6. Apply + persist  → re-applied on every boot (volatile offsets), Safe Loop gated.
```

**Why this order:** the GPU **core voltage (Vcore) powers both the shaders and
the on-die memory controller (MC)**. If you tune memory first and then undervolt
the core, the MC loses voltage and the memory OC becomes unstable (re-validation
cascade). Fixing Vcore first means memory is tuned against the final condition.

**Why combined load every step:** in a game the core and memory work at once,
loading the shared rail + power + thermals. Testing either axis in isolation
passes clocks that stutter in games. Every dwell runs `run_combined`, which
saturates three things simultaneously: **ALU** (shader cores), a
**bandwidth-streaming VRAM kernel** (memory-controller / DRAM throughput — the
real current draw through the shared rail, like a game), and a **pointer-chase**
(memory latency/addressing). A latency-bound pointer-chase alone leaves memory
util low (~20 %) and under-loads the rail; the bandwidth stream is what makes the
dwell game-realistic. ALU + chase are known-answer checked; the bandwidth stream
is load-only.

### Live F2 render workload split

The live F2 Forge asks two separate questions: homogeneous `PowerRender` characterizes power and
the voltage boundary; the failure-seeking qualifier tries to reject a point before it can become a
deployable profile. Evidence identity is split by responsibility: discovery **v7**, frontier Texture
**v28** and exact Apply **v30**. The finite DX11/Vulkan/DX12/Endurance matrix retains its four
lanes and durations. DX11 v3 overlaps one GPU batch with CPU checksum work instead of leaving
the GPU idle during each checksum. Pre-v30 positive evidence cannot approve the new cadence;
CandidateCrash history from v29 onward still constrains the crash budget and physical cone.

**Deterministic stock normalization and clock domain.** Before any candidate write, Forge resets to
stock and runs up to six 10 s preheat windows. It requires two consecutive usable windows with no
thermal throttle or telemetry failure, an end-temperature difference no greater than 2 °C, and a p5
difference no greater than 30 MHz. Failure to converge aborts before tuning. Forge then reports three
different facts instead of treating every upper clock as “Cmax”:

- **Ctable** — the maximum clock and bin count in the sane static physical V/F table.
- **Cboost** — the maximum live boost observed after stock preheat.
- **Cmax** — the first reset-clean sustainable clock actually proved by discovery.

Only live bins that also have a sane static-table identity and do not exceed Cboost enter the initial
candidate domain. Once Cmax exists, the measured profile frontier remains the inclusive Cmax→90%
Cmax range.

**Candidate Transaction (discovery).** One candidate attempt is one owned transaction:

1. Arm the Safe Loop boot flag, apply the anchored curve once, and verify the positive offset.
2. Run `PowerRender` and, for Standard/Long, the active qualification phases without resetting or
   reapplying between them; every phase therefore observes the same curve instance.
3. Perform one checked reset to stock, then clear the boot flag exactly once.
4. Persist same-curve `Qualification` observations before the `Discovery` observation.

No positive phase is reusable before step 3 proves both `reset_to_stock_ok` and
`boot_flag_cleared`. A reset, clear or persistence failure is terminal/inconclusive, never positive;
device loss retains the boot flag for recovery. A p99-consensus retry closes its current transaction
cleanly before a new attempt is armed.

**Power-cap hysteresis and discovery residency.** A valid numeric board limit outranks the sampled
cap flag. Sustained p99 is `NearCap` at **≥99%** and `OffCap` at **≤98%**. A validated point in the
numeric 98–99% band may still run Texture instead of losing its last stable bin merely because it
inherited `NearCap`; at or above 99% it remains power-bound. Discovery v7 accepts p5 one adjacent
physical clock bin below the requested target as runtime elasticity. Frontier v28 gives the same
one-bin allowance to Texture during descent so the physical boundary is not rejected for normal
boost-bin motion. Exact Apply v30 retains zero clock-drop tolerance, so neither descent allowance can
certify the labeled profile pair.

**Coherent boundary and monotonic projection.** Resume pruning, the live decision and the final
summary consume the same GPU/run-wide context. A sustainable boundary requires current Discovery and
current Texture qualification at the same target and voltage. Only a reset-clean `ClockDrop` may be
dominated by a qualified same-target pair at strictly lower voltage or a harder target at the same or
lower voltage; direct integrity, device-loss and cleanup failures remain authoritative. A dominated
drop continues only to the next lower physical bin and is never promoted to positive evidence.
Publication then processes the Cmax→90% domain in ascending clock order and chooses only measured,
currently qualified anchors that do not decrease in voltage. Plateaus are valid; a target with no
compatible measured alternative is omitted rather than interpolated or relabeled.

**Split qualification provenance and integrity.** Every current dwell records the service build
version/revision, semantic workload fingerprint, selected backend, adapter and driver identity,
checksum method, stock-golden configuration and checked cleanup. Older JSONL remains readable, but
pre-v7 Discovery, pre-v28 Frontier and pre-v30 Exact Apply positives cannot unlock their respective
stages. The matrix-v27 qualifier is an orthogonal finite rejection test, not a replacement for power
characterization or proof over every future game/driver schedule.

**Applied-bin power and electrical reconciliation.** Apply selects exactly the next valid physical
V/F-table bin above the learned boundary; there is no fixed millivolt addition. The effective delta is
kept in `apply_margin_mv` because real bin spacing is non-uniform. Profile synthesis requires current,
thermally valid p99/p5 calibration at that exact Apply bin. An unqualified candidate may reach the
finite discriminator up to the numeric board cap. After Exact Apply v30/matrix v27 completes,
publication requires the worst measured Apply power to remain at or below 99% of the cap; missing
power fails closed.

A complete, reset-clean DX11 lane with numeric p99 already above that publication ceiling
can reject the pair immediately, even if residency alone is Inconclusive. Later lanes cannot
reduce the worst measured power. The raw observation remains Inconclusive, while the routing
result is `ExactApplyPowerCeilingExceeded`: no identical retries, remaining lanes, voltage
increase at that clock or blacklist. A cap flag alone, a peak, incomplete coverage or a hardware
failure does not qualify for this screen. Lower-clock synthesis still requires fresh qualification.

**Exact-Apply stability closure.** Every unique selected `(target, Apply VF bin)` runs, in order,
**native DX11 v3 for 420 seconds**, **Vulkan Texture Hop**, **DX12 Texture Hop**, and **Endurance**.
Standard uses 120 seconds for each wgpu API and 300 seconds for Endurance (960 seconds total per
pair); Long uses 300, 300 and 1,200 seconds respectively (2,220 seconds total). Each API uses its own
stock-session golden and explicit backend/adapter provenance. Exact Apply v30 remains strict. Three
homogeneous structural DX11 inconclusives at one exact pair aggregate to
`DX11StructuralClockDrop`; this permits at most one vertical repair for that target, then closes it if
the token repeats. The token is not a pass, physical failure or ledger event. Other `Inconclusive`
outcomes block publication; a reset-clean physical rejection removes or vertically repairs the
candidate and triggers re-synthesis. Device loss/TDR requires a Windows restart. The timings and
workloads remain frozen as matrix v27: this is bounded qualification with containment, not
repeat-until-failure certification.

**Cooperative cancellation and UI headroom.** Every live discovery/qualification render receives
the Forge cancellation token and checks it between bounded GPU frames/dispatches. Stop enters
`stopping`, submits no new batches, drains the current bounded work and performs checked transaction
cleanup. Cancellation is recorded as inconclusive/cancelled, never as bad or validated evidence. The
UI reads structured progress fields rather than parsing logs; completed evidence remains durable in
`f2_observations.jsonl`.

**Clean and finite TDR exclusion.** Clean archives/rebuilds positive discovery and profile state but
always loads effective global `Rigid`, `Quarantine` and TDR-cone safety evidence. Each effective rigid
v29-or-later `CandidateCrash` projects downward over the real physical tables with one voltage-bin of relief
per lower clock bin; overlapping cones take the highest floor. A point at or below that floor is
refused before Safe Loop arm, GPU write or dwell as `TdrRiskGuard/CensoredBoundary`. This censorship
does not claim stability or instability and appends no observation/condemnation row. The first bin
strictly above the floor still must pass Frontier v28, while final Apply remains Exact v30. The
persistent intended exploration allowance is two CandidateCrash incidents for the GPU since the
v29 safety floor; a positive-contract bump does not reset it. A durable count greater than two
fails closed before more candidate exploration.

**Transactional safety I/O (2026-08-25).** Safety evidence is authoritative only when it is durable
and readable. Sentinel persists and verifies its Event Log seed/floor, then completes a synchronous
watcher-ready handshake before boot reapply. A CandidateCrash append is flushed with `sync_data` and
must be found by the strict reader before Forge can store `interrupted`; failure leaves
`needs_attention`, the raw lane and pending incident intact for startup reconciliation. Live F2
discovery, synthesis, calibration, repair, exact gate and publication all use strict observation and
condemnation reads. F2 Benchmark routes through the same proof-aware writer as profile Apply; legacy
workers use checked preflight plus owned BootFlag arm/revalidation/clear. Full Reset quiesces those
workers and removes only reusable positive evidence, preserving every negative safety source.

## Problems hit → solutions

- **The GPU wasn't actually being stressed** (sat at ~4% util / 64 W). Kernels
  were tiny (~6e9 ops, <1 ms); the elapsed time was CPU-side overhead.
  **Fix:** sustained back-to-back dispatch loops that saturate the GPU
  (100 % / ~177 W), with an **LCG jump-ahead** (affine fast-exponentiation) so
  the CPU reference is O(log n) regardless of how many rounds ran.

- **Detecting instability before a crash.** Undervolt fails "gently" (silent
  compute errors — caught by known-answer tests *before* a hard hang). Raising
  the clock fails "hard" (TDR / device lost, often with **no** silent-error
  warning — a hung shader, not a wrong number). You cannot always pre-empt a
  TDR; the goal is to make it rare, informative, and recoverable. **Mitigations:**
  - **Near-cliff fine stepping:** once we know a cliff from a higher voltage,
    the clock step shrinks (e.g. 15 → 5 MHz) as we approach it, so the next
    probe is likelier to land in the silent-error zone than to TDR.
  - **Device-lost is non-fatal:** a TDR sets the ceiling at the last stable
    reading, the wgpu device is **recreated**, and the sweep continues with the
    remaining voltages and the long validation — it never throws away the work
    it already found, and the pipeline still delivers/persists a profile.
  - longer dwell, stop at first silent error, large margin, and the **Safe Loop**
    (boot-flag) so a crash that does reach the driver recovers on reboot and the
    bad profile is not re-applied.

- **VRAM truncated curve.** NVAPI splits the V/F table into two arrays; reading
  only the first cut the curve at ~943 mV. **Fix:** read both → full 450–1087 mV
  curve, matching MSI Afterburner.

- **GDDR6 memory validation is hard.** On-die link CRC *corrects/retries* errors,
  hiding them from a linear read/verify, while consumer cards expose no ECC
  counters. **Fixes:**
  - **Pointer-chase** test: a wrong read derails the whole chase (cascade) —
    far more sensitive to uncorrected/addressing errors than linear verify.
  - **Bandwidth consistency**, not peak: taking the peak hid the dips that *are*
    the in-game stutters (CRC retries). We measure (peak, min) per clock; a
    clean clock holds steady, an unstable one dips. Stop at the first
    **inconsistent** clock (min/peak < ~93 %).
  - **Combined core+mem soak**: memory-only tests passed clocks (e.g. +900 MHz)
    that stuttered in games because the shared rail wasn't loaded. The combined
    soak + back-off recedes until a clock survives game-like load.

- **Bandwidth peak ≠ best.** Past the GDDR6 ECC/CRC wall, more MHz = more
  correction = *less* real bandwidth. We find the effective-bandwidth peak/knee,
  not the max clock — better than Afterburner's "crank until artifacts".

## The elastic V/F ceiling (how the undervolt is applied)

A hard voltage lock or a rigid clock pin makes the GPU run a fixed clock at a fixed
voltage. Under a heavy, near-power-cap game load that removes the card's ability to
manage its own power, and it **TDRs** (driver reset / black screen). MSI Afterburner
avoids this by editing the **V/F curve** instead: it keeps the curve free below a
chosen point and **flattens it to the right** of that point. The card still drops
clocks/voltage on light load (elasticity preserved), but never boosts past the
validated point.

Nidavellir does the same via the modern NVAPI **`ClkVfPoints`** family (per-point
curve offsets), which is what Afterburner/the NVIDIA app use on current drivers:

- **Read** the live curve — `(index → voltage → frequency)` per point via
  `ClkVfPointsGetStatus`.
- **Apply a ceiling** at the validated `(voltage Vp, clock Fp)`: every point whose
  voltage ≥ Vp gets a per-point frequency offset that flattens it to Fp; lower-
  voltage points are left untouched (elastic). No voltage lock, no clock pin.
- **Reset** zeroes every point's offset.

Verified on an RTX 3060 Ti (driver 595.97): applying a ceiling drops the clock the
card sustains under load to the ceiling value (and its power with it), and reset
restores stock boost — all while the card keeps managing its own power.

If the modern API is **not** available (older driver, or a GPU that doesn't expose
it), apply falls back to a global clock offset + an NVML max-clock cap — less
elastic, but it works everywhere. The Forge view shows which mode is active.

### Supported GPUs for the V/F-curve method

The elastic ceiling needs NVIDIA's per-point curve API, present on **desktop
Pascal and newer**:

- **Supported:** GTX 10-series (Pascal), GTX 16-series (Turing), RTX 20 (Turing),
  RTX 30 (Ampere), RTX 40 (Ada), RTX 50 (Blackwell) — desktop cards on a current
  driver (R550+; verified on 595.97).
- **Fallback (offset + clock cap):** Maxwell and older; cards/drivers that don't
  expose `ClkVfPoints`; most **laptop** GPUs (vendor-locked curves).
- **Not supported:** non-NVIDIA GPUs (NVAPI is NVIDIA-only) — AMD is on the roadmap.

The program **detects this at runtime** (`vf_curve_supported()`), so the UI always
reflects what your exact GPU + driver actually allow rather than a static list.

## Apply, persist & report field failure

GPU offsets are **volatile** (lost on reboot/driver reload). "Apply" writes the
profile (lock voltage + clock offset / memory offset) **and persists it**; the
Core Service **re-applies it on every boot** — gated by the Safe Loop: if the
boot-flag is still armed (last apply crashed), Safe Mode is active, or a Forge incident awaits
operator acknowledgement, it is **not**
re-applied. "Reset to stock" clears it.

If a forged profile repeatedly fails under real use, **Mark unstable** resolves that profile's exact
hardware-derived clock/voltage pair, records durable local failure evidence and invalidates the
published set. The coordinates are not product constants. Normal recovery and Full Reset may clear
active/positive Forge state, but effective Rigid, Quarantine and TDR-cone safety evidence remains
authoritative even for the next Clean run.

An interrupted run never resumes silently. A surviving running checkpoint enters **Needs Attention**,
keeps the GPU at stock and requires explicit acknowledgement. When the active boot flag identifies an
exact candidate, only that candidate is blacklisted; otherwise the incident is retained as
unattributed rather than guessing from adjacent observations. A Sentinel-attributed TDR projects the
run terminal as `interrupted/TdrOrCrash` without relabeling the raw workload row, and closes the boot
to GPU mutation. After Windows restarts and the incident is acknowledged, the UI may request only
same-run Resume; the checkpoint must match the exact build, GPU and driver, and recovery never falls
back to Start under a newly selected mode.

## Honest limits

- No synthetic test fully certifies consumer GDDR6 stability (ECC masks errors,
  no counters). The tool gets close (combined load, consistency, long soak) and
  applies margin — **final confirmation is a real game/benchmark session.**
- Finding the absolute OC ceiling (Godforge) inherently risks a TDR/black screen;
  the Safe Loop makes it recoverable. For safety, prefer undervolt + moderate OC.

## Condemnation ledger + vertical Apply repair (2026-07-16)

- **`condemnation_ledger.jsonl`** (append-only, per-GPU, `crates/core/src/condemnation.rs`) holds
  hard failures across every reset: **Rigid** (field TDR, machine crash with a candidate armed,
  device-lost, operator report — the pair and everything at-or-below it at the same clock is
  refused; only a manual `rehabilitated` append lifts it) and **Quarantine** (Texture/Endurance
  SilentError at exact-Apply — strictly-below refused; the exact pair may be re-attempted but
  publishing needs two independent full-gate passes, or one pass under a stronger contract).
  Descent 60 s boundary failures stay in `safe_loop.json` (operational). Every confirmed hardware
  preflight, the descent boundary check, profile restore and the IPC Apply guard consult the UNION
  of the field floor and the ledger.
- **TDR cone (2026-08-14):** effective rigid v29-or-later CandidateCrash rows additionally generate the finite
  1:1 physical-bin cone described above. It applies to descent, calibration and exact Apply in every
  mode, including Clean. Cone-only censorship produces no pass/fail/ledger evidence. More than two
  persistent CandidateCrash rows closes further crash-seeking starts fail-closed.
- **Vertical repair**: a failed exact-Apply pair condemns the *bin*, not the clock. The same clock
  may climb the real VF curve after a reset-clean repairable failure (skipping condemned bins),
  bounded by the candidate board cap and the profile voltage ceiling; PowerRender calibration
  fills unmeasured bins. A measured Apply energy-envelope refusal closes vertical repair outright.
  The repaired pair always re-runs the full gate; descent evidence only orients power/order.
  `DX11StructuralClockDrop` is narrower: only one repair is allowed for that target. TDR/device loss
  never performs a same-boot repair; it interrupts the run and becomes cone input after reboot.
  A candidate is skipped without a ladder only when an already gate-approved point dominates it
  (≥ sustained clock, ≤ selection power).

## Next steps (deferred)

- AMD path (ADLX) — currently NVIDIA/NVAPI only.
- Persisted knowledge base / community priors (roadmap v0.7+).
- CPU and RAM tuning (project is GPU-focused first by design).
- UI polish (test layer for now).
