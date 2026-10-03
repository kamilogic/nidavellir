# Nidavellir

Where silicon is forged to its prime.

**Nidavellir** is a safety-first GPU tuning system for Windows.

It learns how your specific GPU behaves under real load, builds GPU-specific knowledge over time, and forges transparent performance profiles instead of applying generic undervolt or overclock presets.

Nidavellir is named after the legendary realm of the dwarf smiths in Norse mythology — where impossible artifacts were forged beyond the limits of raw material.

The same idea applies here:

> Every GPU is different. Nidavellir does not assume what your silicon can do. It measures, validates, learns, and forges.

[![License: GPL-3.0](https://img.shields.io/badge/License-GPLv3-blue.svg)](./LICENSE)
![Platform](https://img.shields.io/badge/platform-Windows%2010%2F11%20x64-0f766e)
![Status](https://img.shields.io/badge/status-v0.5%20beta%20%E2%80%94%20NVIDIA%20GPU%20forge-1d4ed8)

---

## Current Scope

Nidavellir is focused on:

```text
NVIDIA GPU tuning on Windows
```

* GPU V/F curve tuning (undervolting at a chosen clock)
* automatic, measured profile generation
* Safe Loop crash recovery
* persistent GPU knowledge
* transparent results

Out of scope:

* CPU and RAM tuning (they may become separate programs later)
* AMD GPU support
* Linux support

The interface is in English. A Portuguese option is planned.

---

## Why Nidavellir Exists

Most GPUs ship with aggressive boost behavior:

* high voltage headroom;
* clocks that may not sustain under real power limits;
* unnecessary heat and fan noise;
* performance limited by power or thermals;
* no clear explanation of what the best tuning point actually is.

Manual undervolting can fix this, but it requires experience.

Nidavellir aims to make this process accessible:

```text
Detect GPU
→ Forge GPU
→ Learn safe operating regions
→ Build transparent profiles
→ Apply the profile you prefer
→ Recover safely if something fails
```

---

## Profiles

Nidavellir forges three profile types.

### Godforge

Maximum sustainable performance.

Godforge is for users who want the highest stable performance their GPU can sustain under load.

It is not based on advertised boost clocks or short-lived peaks.

It is based on measured, validated, sustainable behavior.

---

### Brokkr's Best

Recommended for most users.

Brokkr's Best aims to preserve nearly all gaming performance while significantly reducing power draw, heat, and fan noise.

It is the balanced profile:

```text
strong performance
+
lower power
+
stability-first validation
```

---

### Deep Calm

Maximum efficiency.

Deep Calm prioritizes the best performance-per-watt result, even if some performance is sacrificed.

It is intended for users who want:

* lower power draw;
* lower temperatures;
* quieter operation;
* efficient daily use.

---

### Example result

One Forge run on the test RTX 3060 Ti (200 W limit). At stock, this card sustains 1740 MHz at about 200 W under the test load. Every GPU differs.

| Profile | Clock | Voltage | Typical power | vs stock |
| --- | --- | --- | --- | --- |
| Godforge | 1890 MHz | 937 mV | 196 W | +150 MHz, −2% power |
| Brokkr's Best | 1800 MHz | 875 mV | 175 W | +60 MHz, −12% power |
| Deep Calm | 1710 MHz | 825 mV | 160 W | −30 MHz, −20% power |

---

## How a Forge run works

1. Normalize the GPU at stock and record stock references for every test API.
2. Find the highest clock the card sustains, then lower the voltage step by step until the first failure: a silent render error, an unstable result or a driver crash.
3. Repeat at about 5% and 10% below that clock.
4. Every passing point must survive the full matrix: DX11, DX12 and Vulkan render loads, load steps, field concurrency and an endurance lane.
5. Publish each profile with a game margin above the lowest passing voltage, and restore stock.

A Standard run takes about 6 hours.

---

## Forge Knowledge

Nidavellir does not apply a fixed formula.

It builds GPU-specific knowledge over time.

Example:

```text
1800 MHz @ 875 mV → qualified
1800 MHz @ 837 mV → qualified (lowest pass)
1800 MHz @ 825 mV → silent error
1710 MHz @ 775 mV → driver crash
```

That knowledge is preserved and used to avoid repeating unsafe regions.

The long-term goal is for each GPU to become better understood over time.

---

## Safe Loop

Nidavellir is designed around the assumption that tuning can fail.

The Safe Loop system protects the user by tracking risky steps and recovering after interrupted or failed tuning attempts:

* a boot flag is armed before every risky step, and boot-time recovery reads it;
* a driver-crash (TDR) watcher stops the Forge, saves the incident and returns the GPU to stock;
* failed points and a safety cone below each crash are never tested or applied again;
* known-bad profiles never persist;
* a run interrupted by a crash continues from where it stopped, by itself if you allow it;
* a profile marked unstable in real use is removed and blocked.

Safety is part of the product, not an afterthought.

---

## Installing

Nidavellir is in beta, validated on one GPU so far. Try a profile in your own games before relying on it.

1. Download `Nidavellir_x.y.z_x64-setup.exe` from [GitHub Releases](https://github.com/kamilogic/nidavellir/releases).
2. Run it. Windows asks for administrator permission, because the Core Service talks to the GPU.
3. Open Nidavellir and press **Forge GPU**.

Updates: Nidavellir checks for a new version when it starts, shows what changed and installs it when you choose **Update now**. Versions before 0.5 must be updated by hand once.

Requirements:

* Windows 10/11 x64
* NVIDIA GPU
* Administrator permission for the Core Service

---

## Building from Source

### Prerequisites

* Windows 10/11 x64
* [Rust](https://rustup.rs/) with MSVC toolchain

```powershell
rustup default stable-x86_64-pc-windows-msvc
```

* [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)

  * Desktop development with C++
* [Node.js](https://nodejs.org/) 20+

---

## Development Workflow

`scripts\dev-launch.bat` builds the Core Service, starts it elevated in a console and opens the UI. If the dev service is registered with Windows (`scripts\dev-service-boot.ps1 -Action Install`, elevated), it uses that service instead. The service then starts before login and continues runs through reboots.

For one explicitly reviewed development validation after an exhausted crash budget, see
[command-based manual validation](docs/development-validation.md). This opt-in console flow
preserves historical exclusions and does not change the installed application's default policy.

### Core Service

The service requires administrator privileges for hardware-level operations.

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/dev-service-admin.ps1
```

Alternative:

```powershell
cargo run -p nidavellir-service -- console
```

### UI

```powershell
cd apps/ui
npm install
npm run tauri:dev
```

---

## Releasing

Releases are built by CI from a version tag. The installers are signed, and the app updates itself from GitHub Releases. See [docs/releasing.md](docs/releasing.md) for the signing key setup, release notes and the publish step.

A signed local build:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-full-release.ps1
```

Expected output:

```text
target/release/bundle/nsis/Nidavellir_*_x64-setup.exe (+ .sig)
```

---

## Architecture

```text
Tauri + Svelte UI
        |
        | named pipe IPC
        v
Nidavellir Core Service
        |
        | NVAPI / NVML / GPU stress workloads
        v
GPU
```

The UI runs without administrator privileges.

The Core Service owns privileged hardware interactions.

---

## Project Layout

```text
nidavellir/
├── apps/
│   └── ui/                 Tauri + Svelte frontend
├── crates/
│   ├── core/               Shared core logic
│   ├── gpu-nvapi/          NVIDIA GPU control and V/F curve access
│   ├── gpu-stress/         GPU qualification workloads (DX11, DX12, Vulkan)
│   ├── driver-pawnio/      Legacy PawnIO backend (CPU work, out of scope)
│   └── service/            Windows service and tuning orchestration
├── docs/
│   └── release-notes/      What users see in the update window
└── scripts/                Development and release scripts
```

---

## Current Development Status

Working:

* NVIDIA V/F curve read/write through the modern NVAPI path;
* multi-clock frontier search: a staircase descent with a game margin;
* exact-Apply qualification matrix (DX11, DX12, Vulkan, endurance, load steps);
* three distinct profiles: Godforge, Brokkr's Best and Deep Calm;
* Safe Loop recovery, the TDR watcher and failure cones;
* automatic continuation after a driver crash (opt-in);
* signed installer with in-app updates.

Next:

* game validation of the profiles across more GPUs;
* Portuguese interface;
* code cleanup after the GPU-only scope change.

---

## Tests

```powershell
cargo test --workspace
cd apps/ui; npm test; npx playwright test
```

Additional GPU-specific tests may require compatible NVIDIA hardware and should be treated carefully.

---

## License

GPL-3.0-or-later
