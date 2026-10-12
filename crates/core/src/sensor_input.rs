use crate::sensor_meta::SensorSource;

/// Read-only live GPU voltage supplied by the service's NVAPI layer. Core cannot query NVAPI
/// directly without reversing the crate dependency. CPU and board inputs left with the PawnIO
/// driver (GPU-only product, 2026-10-11).
#[derive(Debug, Clone, Default)]
pub struct SensorInput {
    pub gpu_voltage_mv: Option<u32>,
    pub gpu_voltage_source: Option<SensorSource>,
}
