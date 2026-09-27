# Nidavellir — Restricted beta product contract

Updated: 2026-09-25. Governance: `AGENTS.md`. Acceptance/status: `roadmap.md`.
IPC details: `docs/contracts/ui-backend.md`.

> Where silicon is forged to its prime.

Nidavellir helps a person with no GPU-tuning knowledge measure and apply an individual
NVIDIA GPU undervolt on Windows. The beta prioritizes a qualified, useful daily-use
result and a clear return to stock. Every claimed clock, voltage and saving comes
from this GPU's measurements. Synthetic qualification is bounded evidence; universal
game stability and an improvement on every GPU are not promised.

## Normal journey

Development-only validation can be explicitly authorized from the console after incident review
without deleting historical exclusions. It is limited to one Standard run and is not part of the
installed beta's one-click flow. Procedure and limits: `docs/development-validation.md`.

1. Open the installed app. Confirm service, supported hardware and safety readiness.
2. Click **Forge GPU**. Standard is the recommended default; no diagnostic-mode choice
   or tuning expertise is required. Show progress and cancellation.
3. Finish with qualified profiles and measured trade-offs, or explain why no suitable
   improvement was qualified. Missing evidence never becomes a successful result.
4. Recommend **Brokkr's Best**. The user explicitly applies a qualified result;
   **Return to stock** remains accessible through the recovery rules.
5. After interruption, provide an accurate recovery action. Require Windows reboot
   when necessary; preserve history and never silently substitute a new run.

“One click” means one guided automatic measurement workflow without manual clocks,
voltages, file editing or terminal commands. Apply consent remains explicit, as does
a required Windows reboot.

## Profiles and evidence

- **Brokkr's Best:** recommended balance of sustained performance and measured power.
- **Godforge:** maximum qualified sustainable performance within the current policy.
- **Deep Calm:** qualified efficiency with an explicit performance trade-off.

Discovery must explore each GPU's measured voltage/frequency behavior and compare qualified
candidates for these objectives, rather than stop at the first stable point. Higher clocks at
higher voltages remain candidates when supported by this GPU's evidence and safety policy.
Profile selection uses measured performance and power; clock alone does not establish efficiency.
"Best" means the best qualified result within the bounded search and existing profile policy,
not an exhaustive proof of the silicon's global optimum.

The operator's manually found 1800 MHz / 875 mV is a provisional experimental reference for
this RTX 3060 Ti only. Its long gaming history motivates checking whether independent discovery
can recover comparable or better trade-offs; it is neither a proven optimum nor a required exact
output. Do not hardcode it as a search seed, target, limit, fallback or automatic pass, and do not
transfer it to other GPUs. First-use acceptance remains Full Reset followed by Clean Run without
reusing any saved GPU learning, including negative history. A discrepancy with the manual reference requires an explanation
of configuration, coverage and measurements, not thresholds adjusted just to approve that point.

Keep existing synthesis/safety policy during closure. Do not fabricate distinct results
when roles coincide or lack an eligible candidate. Partial-profile publication needs a
separately verified backend contract; the UI cannot override qualification. Clock
differences must not be presented as measured game FPS gains.

Current contracts: Discovery9, Frontier32, ExactApply35 and matrix27; search7.
The power envelope is the representative load (PowerRender): it must hold the target strictly
below the board limit. Heavier stress lanes may reach the limit and drop onto stock curve points;
that is not a failure, but integrity/containment/cleanup still are.
Profiles specify nominal clock with an explicit measured upper allowance of15MHz. NVML still
requests the nominal cap. Heavy exposure must reach nominal within this envelope; transient
peaks do not qualify a higher profile. Voltage/power bounds are unchanged. Absolute peak clock
is persisted and required for current positive evidence, independently of sustained p95.
Discovery qualifies the highest sustainable clock under the board limit across all heavy loads
first, then explores economic candidates within10% below it. See
`docs/qualification-rules-2026-09-25.md` for exact disqualification rules. These are diagnostic details,
not user decisions. Finite candidate/phase policies do not guarantee a wall-clock
deadline for an unresponsive driver. Duration is an estimate with a stated basis.

## Optional modes and lifecycle

Standard is default; Long and Clean are optional advanced choices. Clean rebuilds
positive measurements with Standard-duration proof and preserves safety boundaries.
Full Reset erases all GPU learning, including blacklist, failure history and learning archives,
after confirmed stock recovery. Soft Reset clears positive learning and profiles while keeping
known failures. Both prepare Clean for the next explicit run; neither starts a run or bypasses
a required Windows restart or development authorization. The beta makes no five-minute
promise and performs no background stress testing during play.

Forged/Tempered/Refined/Legendary are naming vocabulary, not a promise of implemented
automatic maturity or permission to disable monitoring. Automatic maturity, community
learning, CPU/RAM/motherboard tuning, AMD support and new OC modes are deferred.

## Recovery and terminal outcomes

- Qualified result: preserve exact evidence; enable only its qualified Apply path.
- No eligible improvement / exhausted safety budget: keep stock, explain the outcome,
  and avoid resetting repeatedly to retry an unchanged safety refusal.
- Compatible interruption: after reboot when required, confirmed stock and explicit
  acknowledgement, resume the same compatible build/GPU/driver/run.
- Missing/incompatible checkpoint: resolve the incident at stock, explain the refusal;
  starting again is a separate explicit action.
- Full Reset: confirm stock, acknowledge safely, remove positives/checkpoint, preserve
  blacklist, incident history and condemnation.
- Corruption, failed stock reset or uncertain ownership: preserve evidence, refuse
  further tuning and show the actual error. Never report success using default state.

## Support and release claims

The local RTX 3060 Ti is the engineering reference. Historical driver tests support
those exact conditions, not automatic acceptance of the beta. D3 must list combinations
actually exercised. Building an installer does not prove installation, upgrade or removal.
The beta remains unreleased until the evidence required in `roadmap.md` is complete.
