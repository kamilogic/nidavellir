# Nidavellir — Beta delivery plan

Updated: 2026-09-17. This is the current execution checklist. `product.md` defines the
contract; `memory.md` records checkpoints; `handoff.md` identifies the next action.

## 2026-09-24 — accepted discovery redesign implemented; physical acceptance next

User accepted longer qualification after the September18 reboot. Complete same-pair matrix
now precedes refinements; three stock-derived bands share24 admissions/8h Standard, persistent
through manual pauses. Stock secondary oracle and peer failure propagation are fixed. Frontier29 /
ExactApply32 replace the old proof contracts; old exhaustive/vertical-repair flow removed.
692 workspace tests passed (3 hardware tests ignored),18 UI unit and33 browser cases passed;
release Core and production UI built. Evidence:target/beta/qualified-discovery-20260924/.
Next: user opens scripts/dev-launch.bat, Full Reset -> Clean Run, exports resulting report.
No service/load/reset was started during implementation. D5 physical acceptance and remaining
installer/product acceptance are still open. Do not infer a finished product from software checks.

## 2026-09-17 — DX11 active exposure correction

Follow-up: first v4 Clean Run achieved numeric active exposure at1875@937 but saw +15MHz
and stopped globally. Control-refused pairs now have bounded separate routing and per-phase
diagnostics; simultaneous energy refusal stays visible. 717 software tests passed. Hardware
containment cause and end-to-end qualification still pending; user BAT rebuild required.

Exact Apply v31 separates heavy-load integrity from fenced active target exposure within the
existing lane budget. Idle cannot qualify a clock; unexercised pairs return to selection without
voltage repair or blacklist. 714 workspace tests plus a separate short stock DX11 integration
passed; release/sidecar rebuilt. Tuned clean-start acceptance and complete profile publication
remain pending. Evidence: target/beta/active-residency-20260916/verification.json.
Older dated plans elsewhere are historical evidence.

## Completion rule

Release a restricted NVIDIA/Windows beta only after all five deliveries have evidence.
A build or unit-test pass alone does not complete hardware or installer acceptance.
Freeze shaders, qualification contracts, modes, OC features and broad visual changes.
A reproducible release blocker may justify a surgical change, recorded here first.

Brokkr's Best is the recommended result. Other qualified profiles remain available.
Unsupported hardware or no adequately qualified improvement is an honest result at
stock, without repeated Clean/Reset/Resume attempts.

Operator acceptance priority (2026-09-16): every new manual validation starts with Full Reset
and Clean Run, rebuilding the hardware measurements and profiles without prior learning.
Warm-start/Resume success does not close fresh-start acceptance. Diagnose and repair failures
in that path before expanding scope. As explicitly requested on2026-09-17, Full Reset erases
negative history too; Soft Reset retains known failures. This resets saved search knowledge,
not the physical GPU or the requirement to verify stock/reboot readiness.

Telemetry blocker correction (2026-09-16): the overnight Clean Run exposed voltage sampling
by loop count and ambiguous Discovery refusal labels. The dwell now reuses NVML, schedules
voltage by elapsed time, preserves specific reasons and records per-phase sample count/clock
maximum. Offline checks pass; a read-only host probe confirms cadence without load. This closes
the demonstrated software sampling defect, not D3: loaded coverage, DX11 target residency and
independent profile discovery remain pending. No shader, pass threshold or safety policy changed.
Evidence: docs/clean-run-results-2026-09-16.md and target/beta/sampler-fix-20260916/.

## Deliveries

| ID | Work and acceptance evidence | Status |
|---|---|---|
| D1.1 | One beta contract replaces conflicting product/mode promises. | Done: product.md |
| D1.2 | Recommended default; optional modes secondary; measured results and explicit Apply/Return to stock. | Software journeys passed, including result/stock |
| D2.1 | Compatible, incompatible and missing checkpoint recovery without false Resume promises or automatic fresh runs. | Software journeys passed |
| D2.2 | Each recovery IPC failure stops the sequence; duplicate clicks cannot overlap workflows. | Software journeys passed |
| D2.3 | Cancel, Full Reset, offline/reconnect and safety refusals have valid next actions and preserve negatives. | Software journeys and installed idle-service offline/reconnect passed; GPU-load stop pending |
| D2.4 | Terminal outcomes and timeout/reboot limits are explicit; no false success after failure. | IPC deadlines, bounded process shutdown and listener-failure regressions passed; hardware evidence pending |
| D2.5 | Startup crash attribution belongs to the interrupted transaction, never an older BSOD. | Timestamp guard and 3 regressions passed; historical ledger unchanged |
| D2.6 | After manual/full reset, distinguish cleared recovery from a persistent tuning block and expose next steps. | Three-theme software journeys and actual installed ordinary-user guidance/export/history passed; no recovery pending, 3/2 block preserved |
| D2.7 | Explicit development-only single-run authorization, preserving exclusions and refusing automatic retries. | Implemented; 8 new backend tests, 3 theme cases, 4 isolated command scenarios and release build passed; live authorization/run pending operator |
| D3.1 | Read-only acceptance preflight: binary identity, GPU/driver, service, persisted safety readiness. | Done; backend confirms 3 effective crashes / limit 2; blocked |
| D3.2 | One authorized monitored acceptance on eligible hardware with a frozen build and retained measurements/journals. | Pending hardware |
| D3.3 | Qualified Apply, real use, restart/reapply, stock restoration and actually tested support list. | Pending hardware |
| D4.1 | Automated recovery journeys with injected IPC and persisted-state backend regressions, without GPU load. | 17 JS + 4 native pipe tests passed; J10 UI and J11 injected lifecycle passed |
| D4.2 | Repeatable offline command and CI gate: Rust tests, UI journeys, production build. | Local gate passed; CI defined, remote run pending |
| D4.3 | Browser verification: ready, offline, recovery, reset; distinguish mocked transport from hardware evidence. | 15 existing + 4 final J09 Playwright cases passed with mocked IPC; installed J09 guidance/export/history separately passed with native IPC |
| D5.1 | Build scripts fail on native errors, reject stale binaries, verify sidecar identity and source provenance. | Final NSIS build + source/artifact manifest passed |
| D5.2 | Installer/service lifecycle failures are explicit; no unrelated CPU-driver requirement. | Live lifecycle and missing-binary/SCM start refusal/exact restoration passed; failed shutdown scenarios covered in software |
| D5.3 | Build installer; prove Windows install/update/uninstall; publish only with evidence. | Authorized host install/reinstalls/Stop/uninstall, start-failure and actual published legacy-build migration passed; clean OS pending |
| D5.4 | Copyable manifest-pinned executor and resumable evidence for install/reinstall/Stop/uninstall. | VM or authorized ExistingPc; all six host steps and unelevated Ping passed, 192-file backup retained |
| D5.5 | Installed desktop UI uses real IPC as an ordinary user; shortcuts and navigation work. | Online/onboarding/recovery/Settings/shortcuts/offline/reconnect passed; cleanup complete |
| D5.6 | Uninstall after legacy upgrade removes the six retired bundled CPU resources, retaining unrelated files and the installed PawnIO driver. | Actual leftover reproduction, exact-path NSIS fix, rebuild and live cleanup/preservation regression passed |

## Fixed journey matrix

| ID | Scenario | Required result |
|---|---|---|
| J01 | Fresh start, known service/GPU/Safe Loop | Recommended Standard run; one request |
| J02 | Offline service or malformed status | No GPU action; explain readiness |
| J03 | Pending incident, compatible checkpoint | Review, confirmed stock, durable ACK, same-run Resume |
| J04 | Missing/incompatible checkpoint | Resolve at stock; new run requires a separate explicit start |
| J05 | Stock reset or ACK fails | No subsequent Resume/Start/Apply; preserve reason |
| J06 | Full/Soft Reset after interruption | Confirm stock; Full forgets all learning, Soft preserves negatives; no automatic start |
| J07 | Driver reset in current Windows boot | Require reboot; no continuation/full-reset bypass |
| J08 | Cancel or service exit during work | Preserve stock/recovery truth; no qualified success inferred |
| J09 | No eligible result / safety budget exhausted | Explain refusal; Reset cannot erase safety limits |
| J10 | Qualified Apply, restart, Return to stock | Exact qualified descriptor; effective state verified |
| J11 | Install/update/uninstall failure | Actionable failure; no stale artifact released |
| J12 | Explicit development authorization after incident review | One Standard run; same historical exclusions; consume on first new crash or termination; no automatic authorization/Start/Resume/Apply |

## Intermittent execution protocol

1. Read this plan and latest memory checkpoint; inspect git status and current processes.
2. Check Codex usage at start and checkpoints. The 5-hour window is a usage window,
   not five hours of uninterrupted work. Do not use reset credits or schedule wakeups
   without a separate request.
3. Finish bounded changes, record commands/results and next action before long operations
   or stopping. After interruption, verify actual completion before repeating anything.
4. Preserve pre-existing uncommitted work. Do not broad-stage, reset, clean or publish.
5. Software gates precede hardware acceptance. Re-read live safety state; never delete
   negative evidence to force eligibility. Pending evidence is not a pass.
6. During development, use the release console service plus ordinary-user Tauri dev UI;
   rebuild/restart between runs, keeping code and processes fixed during each acceptance run.
   The operator performs the run and brings logs back for later analysis. Avoid the optional
   cargo-watch service restart in scripts/dev.ps1 during hardware acceptance. Re-test the
   installer only for installer changes or the frozen release candidate.
   The operator launches Desktop dev.bat (delegates to scripts/dev-launch.bat), performs
   Full Reset in UI, confirms the development authorization in the BAT, then starts Clean Run
   in UI. Do not pre-start or pre-authorize that manual acceptance session on his behalf.

## Evidence log

- Baseline (2026-09-11): Full Reset fix, 478 service tests, dev service and UI production
  builds passed. Software evidence only.
- Starting condition: 1920@943 remained a pending CandidateCrash. Re-read live state
  before any operational action.
- Session 1: workspace Rust tests, 13 Node tests, 8 browser journeys and UI production
  build passed. `scripts/validate-offline.ps1 -Browser` is the repeatable gate. Browser
  tests replace Tauri transport and cannot reach the real service. Targeted Vite/PostCSS/
  Nano ID patches leave npm audit at zero vulnerabilities.
- Read-only report: `target/beta/acceptance-preflight.json`; RTX 3060 Ti, driver 610.88,
  no running service, pending 1920@943, no checkpoint/BootFlag/applied descriptor.
  Three v29 CandidateCrash rows exist; backend must calculate effective eligibility.
  Release/sidecar are still the older August 25 build, not current source.
- Session 2 next: bound native IPC without retrying uncertain mutations; report actual
  start refusals (currently some map to “already running”); then checked installer lifecycle.
- Session 2 checkpoint: async Tauri IPC uses Tokio already in the dependency graph; 5s
  connection / 30s exchange deadlines, bounded framing, no mutation retry, owned I/O cleanup.
  Service pipe now closes on error too. Four isolated real Windows-pipe tests passed.
  Read polling is coalesced; 16 Node / 13 browser / 478 service tests passed.
- Service start refusals retain their true errors. `start_block_reason` exposes unreadable
  safety history / effective crash budget before a start. Reset preserves that refusal.
  NVIDIA onboarding refuses missing/malformed/unsupported hardware and has no PawnIO step.
- Installer bug confirmed in generated NSIS: Tauri installs `nidavellir-service.exe`, while
  old hooks searched `nidavellir-service-*.exe`. New hooks use the correct path, checked
  stop/change/create/start/delete and finite waits, retain registration during Prepare and
  preserve ProgramData. Injected Windows-service lifecycle tests passed without host changes.
  The initial full packaging/provenance build is running; inspect its log before resuming.
- Isolated install acceptance is currently unavailable: Hyper-V Get-VM is denied to this
  session and Windows Sandbox was not found. This is an environment gate, not a passed test.
- Final software checks: 680 Rust tests passed (2 explicit hardware smokes ignored), 16 browser
  journeys passed, UI build passed. J10 covers explicit qualified Apply, rejection, reload without
  reapply, and ordinary stock reset without Full Reset. First NSIS package/source manifest passed.
- Concrete next D2 blocker: service_impl.rs currently stops Detector Lab/manual point, then
  records clean shutdown and reports Stopped; it does not wait for every mutating Forge worker.
  Review a bounded service shutdown using the existing worker stop/quiescence contract before
  installed update/uninstall or a hardware campaign. Do not mark D2 complete from mocked Stop UI.

## Remaining execution sequence

Session 11 (2026-09-14 08:21Z): operator approved a controlled development authorization rather
than deleting safety history. `console --development-validation` plus the separate explicit
AuthorizeDevelopmentValidation IPC permits one fresh Standard run in that process. It requires
idle/recovered state, no saved checkpoint/profile, checked history and confirmed stock. Negative
events/cones remain intact; a new/changed crash, completion, Stop, Reset or exit consumes permission.
No Resume, Long, other GPU workers or Apply is covered. Synced audit snapshots/claim/finish records
are retained and linked by ExportForgeLog. Three themes show the runtime authorization state.
700 workspace Rust tests (498 service), 17 Node tests, 3 J12 browser cases, 4 command cases with
isolated Windows pipes, UI build and release service build passed. No live authorization/stock
reset or GPU load was performed. Native ordinary preflight still confirms the original 3/2 block.
Release service SHA-256: 4F48C61154457C5B06C9917AB579244DF5D58783F5F4B4EB7B09D8B2D26828B5.
Evidence: target/beta/development-validation/. Instructions: docs/development-validation.md.
NEXT: operator starts the command-based service, explicitly authorizes and initiates Standard,
then exports logs for review. D3 remains pending hardware evidence. Installer was not rebuilt;
its earlier manifest describes the older package, not this new standalone service/source tree.

Session 10 (2026-09-14T02:10Z / September 13 locally): the new UI installer passed the bounded
ordinary-user native guidance/export/history check on the authorized host. Actual report contains
the three acknowledged incidents; Safe Loop remained idle/protected and tuning stayed blocked.
Normal close and uninstall passed; all 193 original data files retained and critical hashes equal
backup. No service/process/shortcuts/debug listener/active tuning files remain. Evidence and screenshots:
target/beta/installed-safety-guidance/. This closes the previously unavailable native export check.
Two tracked image assets are now deleted (gpu-hero.png, themes/command-gpu.png); preserved, no source
references found. Other 138 source entries and pinned installer hash match. No rebuild or code change.
D3 is still blocked by the 3/2 policy; clean-Windows and remote CI evidence remain pending.

Session 9 (2026-09-14T01:54:50Z / September 13 locally): addressed the operator's warning after
manual Reset. Actual recovery was clear; the UI incorrectly treated a persistent start refusal as
an unresolved Safe Loop incident. All themes now expose Review safety block, the reason and Reset
limitations, diagnostic export and direct history navigation. Refreshed Reset feedback does not
promise another run when blocked; saved profile Apply remains disabled. 17 Node tests, 15 existing
browser journeys and four final J09 cases passed; visual screenshots reviewed. Native export could
not be verified at the final probe because the service was no longer running; no restart was done.
Current installer: 08937E759599244469D94B48E9F50BE6E87C115756232E5284E2ACB203780417; all 140
sources match, service byte-identical. Kit/ZIP refreshed; this UI build has not been reinstalled.
The reset cleared the pending incident, but the preserved 3/2 exploration block remains. D3 is
still pending; this UI fix does not authorize or implement a safety-policy change.

Session 8 completed 2026-09-13T19:25:50Z. Actual published v0.3.1 -> frozen current upgrade,
service Ping and uninstall passed. Both package versions are 0.1.0, despite different release tags.
The initial UI-hash assertion was corrected against the exact installer payload; Tauri's three-byte
bundle marker explains the difference from the standalone build. Original reports are preserved.
Uninstall exposed six retired bundled CPU resource files. A narrow hooks.nsh cleanup and new build
passed on those actual leftovers, including preservation of an unrelated file and the existing driver.
Evidence: target/beta/legacy-package-v0.3.1/{upgrade-completion,legacy-cleanup-before,legacy-cleanup-after}.json.
New installer DFD6D122593DB92BD80033DEDADC8714E531143885BCA0470B692ADF3CCFD63C; only the NSIS
hook changed across 140 source entries, service binary unchanged. No repeated Rust/UI campaign.
No service/UI/shortcuts remain, safety hashes unchanged. D3 remains blocked: read-only policy review
records two explicit TDRs plus one uncertain armed restart and the pair-wide effect of rehabilitation.
Await operator input on another GPU versus a separate policy decision; no automatic unblock or load.
Pristine-Windows acceptance and remote CI remain pending. Do not rerun successful installer journeys.

Session 7 completed 2026-09-13T07:57:32Z. The operator accepted elevation and the final
installed-desktop-remainder/ session passed all pending checks: real SCM Stop, offline disabled
actions, explicit missing-binary helper failure, failed SCM start, hash-verified restoration/start,
automatic UI reconnect, normal window close and uninstall. post-verification.json confirms absent
service/processes/app binaries/shortcuts/debug listener and unchanged safety hashes. An earlier
wrapper's exit-reading issue is retained with its phase journals; owned process handles passed
explicit exit 0/1 checks and the final session. No production files or qualification contracts changed.
GitHub v0.3.1 actually distributes a 0.1.0-named installer, so tags cannot establish a different
installer-version upgrade. Legacy package migration, clean-Windows acceptance and remote CI remain
unverified. No historical package was installed on this host. Do not repeat successful live checks.

Session 6 (2026-09-13): actual installed desktop acceptance uses
C:\Program Files\Nidavellir Desktop Acceptance with an isolated WebView data folder. Unelevated
process token, real NVIDIA onboarding, native SafeLoop/PowerSweep/GpuApply responses, honest safety
refusal, navigation and both shortcuts passed. Evidence: target/beta/installed-desktop-acceptance/.
Windows canceled elevation for Stop; the phase never ran. The offline wait is therefore unexercised,
not an application regression. UI was closed normally; debug port is closed. The service remains
Running/Automatic with no applied profile or armed transaction; safety hashes match its backup.
Next dependent action needs acceptance of Windows UAC: finish Stop/offline, missing-binary failure
and exact restore/start, reconnect, then normal close/uninstall. Do not reinstall or retry blindly.

Session 4: read-only attribution audit found that WER/1001's latest 0x116 is from
2026-08-04T23:29:22Z (record 42397), while the September 10 reboot has Kernel-Power/41
BugcheckCode=0 (record 52958). This does not identify the outage's cause, but the startup
reader demonstrably reused an old BSOD without checking its date. The new timestamp guard
returns Unknown outside the interrupted transaction's time window. Three regressions passed;
the conservative armed-candidate policy and persisted negative history remain unchanged.
Audit: target/beta/restart-attribution-audit.json. Entire Rust workspace: 692 tests passed.

The operator authorized this PC instead of a VM. ExistingPc mode now backs up real ProgramData
without moving or replacing safety state. Actual execution passed at 2026-09-13T02:40:51Z:
192-file hash-verified backup, install/Ping, running reinstall, SCM Stop, stopped reinstall,
uninstall with history retention. Unelevated named-pipe Ping passed separately. Report/backup:
target/beta/installer-host-acceptance/. Service/processes and UI/service binaries are absent;
Safe Loop and ledger hashes are unchanged. Only normal heartbeat/Sentinel/clean-shutdown files changed.
Current package: 2026-09-13T02:39:30Z; target/release/release-manifest.json; all 140 source hashes match.
Installer SHA-256: 7524EFB1DE40464F0356B748E1AD70042F7D7EA3775CDCCFD791466A336A301C.
This proves installation with existing data, not clean-Windows or full desktop UI acceptance.
The post-install read-only preflight still reports 3 effective crashes versus limit 2 and pending
1920@943 ACK. No Forge, Apply, reset or acknowledgement was executed. VM access is no longer
the prerequisite for host installer acceptance; no need to repeat these passed lifecycle steps.

Session 3 software checkpoint: 689 Rust tests passed (2 hardware smokes ignored), 16 Node and
16 Playwright journeys passed, UI build and injected installer lifecycle passed. Logs:
target/beta/shutdown-offline-gate.log and target/beta/shutdown-final-rust-tests.log. The shared
shutdown now supervises worker/Sentinel quiescence and required stock recovery, refuses clean
success on timeout, and terminates the failed process. Tests use isolated subprocesses and
injected cleanup, never the real Core Service/GPU. Installer retries preserve nonzero stop errors.
SCM Running now requires listener readiness; listener creation failure no longer loops forever.
Final NSIS package passed at 2026-09-12T20:19:50 UTC; target/release/release-manifest.json freezes
source and artifact identity. A reproduced npm/cmd → Windows PowerShell module-path failure was
fixed by importing each build host's Utility manifest. Rebuilt preflight at 20:20:39 still reports
3 effective crashes / limit 2, pending ACK, no service and unchanged safety hashes (proof in
target/beta/shutdown-safety-hashes-verified.json). No host installer or GPU workload was run.

1. **D2 live evidence:** package and read-only preflight are complete. Confirm actual Stop/SCM
   outcomes in the environments below; software
   process tests do not establish physical recovery from a driver hang. Keep qualification frozen.
2. **D5 remaining Windows evidence:** host lifecycle, installed UI, offline/reconnect and bounded
   missing-binary/start failure/exact restore now pass. Still verify migration from a historical
   distributed package and pristine Windows. A disposable VM remains an option for pristine Windows
   and failure scenarios; it is not a product dependency. Preserve recorded safety history and
   installer/source identity. Do not report same-version reinstalls as different-version upgrades.
3. **D3 eligibility:** run scripts/acceptance-preflight.ps1 with the rebuilt release executable.
   Stop at any blocker. A safety-budget refusal is a terminal outcome, not permission to delete
   incidents, rehabilitate without evidence, or increment the qualification contract. If this GPU
   remains ineligible, acceptance needs eligible hardware or a separately justified policy decision.
4. **D3 qualified run:** on eligible hardware and a frozen binary, retain driver/GPU/build identity,
   before/after stock readback, Standard run export, all terminal outcomes and qualification proofs.
   Cancel/interrupt recovery must be checked, then explicit qualified Apply, measured savings,
   real-use observation, reboot/reapply and Return to stock. Report only the combinations actually
   exercised; no universal GPU/game claim. A failure/inconclusive result closes the attempt and
   becomes one bounded defect investigation; do not restart Clean automatically.
5. **Release decision:** reconcile D1–D5 evidence in this file, run the software gate once for the
   final source, rebuild/freeze the manifest, and complete the installer/hardware evidence against
   that build. The release workflow creates a draft; no public beta until these gates pass.
