use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::nvml_gpu::read_nvidia_gpus_nvml;
use crate::sensor_input::SensorInput;
use crate::sensor_meta::SensorSource;

// Dashboard/Forge poll live GPU cards once per second. A 30 s cache made clock,
// voltage, fan and power look frozen even while the workload changed.
const GPU_CACHE_TTL: Duration = Duration::from_secs(1);
// GPU-only product (2026-10-11): CPU, memory, board and WHEA readings left with the PawnIO driver
// and the legacy Dashboard; no screen showed them.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorReadings {
    pub gpu: Vec<GpuSensors>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuSensors {
    pub name: String,
    pub utilization_pct: Option<f64>,
    pub vram_used_mb: Option<u64>,
    pub vram_total_mb: Option<u64>,
    pub core_clock_mhz: Option<u32>,
    pub memory_clock_mhz: Option<u32>,
    pub max_core_clock_mhz: Option<u32>,
    pub max_memory_clock_mhz: Option<u32>,
    pub fan_speed_pct: Option<u32>,
    pub voltage_mv: Option<u32>,
    pub voltage_source: Option<String>,
    pub temperature_c: Option<f32>,
    pub power_w: Option<f32>,
    pub temperature_source: Option<String>,
    pub power_source: Option<String>,
}

pub struct SensorEngine {
    cached_gpu: Option<(Instant, Vec<GpuSensors>)>,
}

impl Default for SensorEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl SensorEngine {
    pub fn new() -> Self {
        Self { cached_gpu: None }
    }

    pub fn read(&mut self, input: &SensorInput) -> SensorReadings {
        let mut gpu = self.read_gpu_cached();
        if let Some(primary) = gpu.first_mut() {
            primary.voltage_mv = input.gpu_voltage_mv;
            primary.voltage_source = input
                .gpu_voltage_source
                .map(|source| source.as_str().to_string());
        }

        SensorReadings { gpu }
    }
}

impl SensorEngine {
    fn read_gpu_cached(&mut self) -> Vec<GpuSensors> {
        let now = Instant::now();
        if let Some((ts, val)) = &self.cached_gpu {
            if now.duration_since(*ts) < GPU_CACHE_TTL {
                return val.clone();
            }
        }
        let val = read_gpu_sensors();
        self.cached_gpu = Some((now, val.clone()));
        val
    }
}

fn read_gpu_sensors() -> Vec<GpuSensors> {
    let mut by_name: HashMap<String, GpuSensors> = HashMap::new();

    for nv in read_nvidia_gpus_nvml() {
        by_name.insert(
            nv.name.clone(),
            GpuSensors {
                name: nv.name,
                utilization_pct: nv.utilization_pct,
                vram_used_mb: nv.vram_used_mb,
                vram_total_mb: nv.vram_total_mb,
                core_clock_mhz: nv.core_clock_mhz,
                memory_clock_mhz: nv.memory_clock_mhz,
                max_core_clock_mhz: None,
                max_memory_clock_mhz: None,
                fan_speed_pct: nv.fan_speed_pct,
                voltage_mv: None,
                voltage_source: None,
                temperature_c: nv.temperature_c,
                power_w: nv.power_w,
                temperature_source: nv.temperature_c.map(|_| SensorSource::Nvml.as_str().to_string()),
                power_source: nv.power_w.map(|_| SensorSource::Nvml.as_str().to_string()),
            },
        );
    }

    // No nvidia-smi merge (2026-10-05): it spawned a process with its own NVML session on every
    // read, and the max clocks it alone supplied are shown nowhere.
    if by_name.is_empty() {
        return read_gpu_sensors_wmi();
    }

    let mut out: Vec<_> = by_name.into_values().collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn read_gpu_sensors_wmi() -> Vec<GpuSensors> {
    let ps_cmd = "Get-CimInstance Win32_VideoController \
      | Where-Object { $_.Name -and $_.Name -notmatch 'Microsoft|Basic' } \
      | Select-Object Name, AdapterRAM \
      | ConvertTo-Json -Compress";
    let Ok(output) = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", ps_cmd])
        .output()
    else {
        return vec![];
    };
    if !output.status.success() {
        return vec![];
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let val: serde_json::Value = match serde_json::from_str(text.trim()) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    let arr = match val {
        serde_json::Value::Array(a) => a,
        serde_json::Value::Object(_) => vec![val],
        _ => return vec![],
    };
    let mut out = Vec::new();
    for item in arr {
        let name = item
            .get("Name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if name.is_empty() {
            continue;
        }
        let vram_total_mb = item
            .get("AdapterRAM")
            .and_then(|v| v.as_u64())
            .filter(|&b| b > 0)
            .map(|b| b / (1024 * 1024));
        out.push(GpuSensors {
            name,
            utilization_pct: None,
            vram_used_mb: None,
            vram_total_mb,
            core_clock_mhz: None,
            memory_clock_mhz: None,
            max_core_clock_mhz: None,
            max_memory_clock_mhz: None,
            fan_speed_pct: None,
            voltage_mv: None,
            voltage_source: None,
            temperature_c: None,
            power_w: None,
            temperature_source: None,
            power_source: None,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out.dedup_by(|a, b| a.name == b.name);
    out
}
