use serde::{Deserialize, Serialize};
use winreg::enums::*;
use winreg::RegKey;

/// GPU-only product (2026-10-11): CPU, RAM and board detection left with the PawnIO driver.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareInfo {
    pub gpu: Vec<GpuInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuInfo {
    pub vendor: String,
    pub model: String,
    pub vram_mb: u32,
    pub max_core_clock_mhz: Option<u32>,
    pub max_memory_clock_mhz: Option<u32>,
}

pub fn detect_hardware() -> HardwareInfo {
    HardwareInfo { gpu: detect_gpu() }
}

fn detect_gpu() -> Vec<GpuInfo> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let gpu_base = r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";

    let Ok(base) = hklm.open_subkey_with_flags(gpu_base, KEY_READ) else {
        return vec![];
    };

    let smi_vram = query_vram_nvidia_smi();
    let smi_clocks = query_gpu_max_clocks_nvidia_smi();
    let reg_vram = query_vram_registry();
    let wmic_vram = query_vram_wmic();

    let mut gpus = Vec::new();
    for i in 0..32 {
        let path = format!("{i:04}");
        let Ok(sub) = base.open_subkey_with_flags(&path, KEY_READ) else {
            continue;
        };
        let Ok(model): Result<String, _> = sub.get_value("DriverDesc") else {
            continue;
        };

        let vendor = if model.to_lowercase().contains("nvidia") {
            "NVIDIA"
        } else if model.to_lowercase().contains("amd") || model.to_lowercase().contains("radeon") {
            "AMD"
        } else if model.to_lowercase().contains("intel") {
            "Intel"
        } else if model.to_lowercase().contains("microsoft")
            || model.to_lowercase().contains("basic")
        {
            continue;
        } else {
            "Unknown"
        }
        .to_string();

        let normalized = model.trim().to_lowercase();
        let vram_mb = smi_vram
            .get(&normalized)
            .copied()
            .or_else(|| {
                reg_vram
                    .get(&normalized)
                    .copied()
                    .filter(|&b| b > 0)
                    .map(|b| (b / (1024 * 1024)) as u32)
            })
            .or_else(|| wmic_vram.get(&normalized).copied().filter(|&v| v > 0))
            .unwrap_or(0);

        gpus.push(GpuInfo {
            vendor,
            model: model.trim().to_string(),
            vram_mb,
            max_core_clock_mhz: smi_clocks.get(&normalized).map(|v| v.0),
            max_memory_clock_mhz: smi_clocks.get(&normalized).map(|v| v.1),
        });
    }
    gpus
}

fn query_gpu_max_clocks_nvidia_smi() -> std::collections::HashMap<String, (u32, u32)> {
    let mut map = std::collections::HashMap::new();
    let Ok(output) = std::process::Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,clocks.max.graphics,clocks.max.memory",
            "--format=csv,noheader,nounits",
        ])
        .output()
    else {
        return map;
    };
    if !output.status.success() {
        return map;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<_> = line.split(',').map(|v| v.trim()).collect();
        if parts.len() < 3 {
            continue;
        }
        let name = parts[0].to_lowercase();
        let core = parts[1].parse::<u32>().ok().filter(|&v| v > 0);
        let mem = parts[2].parse::<u32>().ok().filter(|&v| v > 0);
        if let (Some(core_mhz), Some(mem_mhz)) = (core, mem) {
            map.insert(name, (core_mhz, mem_mhz));
        }
    }
    map
}

fn query_vram_wmic() -> std::collections::HashMap<String, u32> {
    let mut map = std::collections::HashMap::new();
    let output = std::process::Command::new("wmic").args([
        "path", "Win32_VideoController",
        "get", "Name,AdapterRAM",
        "/format:list",
    ]).output();
    if let Ok(o) = output {
        if o.status.success() {
            let text = String::from_utf8_lossy(&o.stdout);
            let mut current_name = String::new();
            let mut current_vram: u64 = 0;
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() {
                    if !current_name.is_empty() && current_vram > 0 {
                        map.insert(current_name.clone(), (current_vram / (1024 * 1024)) as u32);
                    }
                    current_name.clear();
                    current_vram = 0;
                    continue;
                }
                if let Some(val) = line.strip_prefix("Name=") {
                    current_name = val.trim().to_lowercase();
                } else if let Some(val) = line.strip_prefix("AdapterRAM=") {
                    current_vram = val.trim().parse().unwrap_or(0);
                }
            }
            if !current_name.is_empty() && current_vram > 0 {
                map.insert(current_name, (current_vram / (1024 * 1024)) as u32);
            }
        }
    }
    map
}

fn query_vram_registry() -> std::collections::HashMap<String, u64> {
    let mut map = std::collections::HashMap::new();
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let gpu_base = r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";
    let Ok(base) = hklm.open_subkey_with_flags(gpu_base, KEY_READ) else {
        return map;
    };
    for i in 0..32 {
        let path = format!("{i:04}");
        let Ok(sub) = base.open_subkey_with_flags(&path, KEY_READ) else {
            continue;
        };
        let Ok(model): Result<String, _> = sub.get_value("DriverDesc") else {
            continue;
        };
        let normalized = model.trim().to_lowercase();
        let vram: Option<u64> = sub
            .get_value::<u64, _>("HardwareInformation.qwMemorySize")
            .ok()
            .or_else(|| sub.get_value::<u64, _>("HardwareInformation.AdapterRAM").ok())
            .or_else(|| {
                sub.get_value::<u32, _>("HardwareInformation.AdapterRAM")
                    .ok()
                    .map(|b| b as u64)
            });
        if let Some(bytes) = vram {
            map.insert(normalized, bytes);
        }
    }
    map
}

fn query_vram_nvidia_smi() -> std::collections::HashMap<String, u32> {
    let mut map = std::collections::HashMap::new();
    let Ok(output) = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"])
        .output()
    else {
        return map;
    };
    if !output.status.success() {
        return map;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(comma) = line.rfind(',') {
            let name = line[..comma].trim().to_lowercase();
            if let Ok(mib) = line[comma + 1..].trim().parse::<u32>() {
                if mib > 0 {
                    map.insert(name, mib);
                }
            }
        }
    }
    map
}
