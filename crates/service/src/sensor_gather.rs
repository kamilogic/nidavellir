use nidavellir_core::sensor_input::SensorInput;
use nidavellir_core::sensor_meta::SensorSource;

/// GPU-only product: the live core voltage from NVAPI, filtered to a plausible range. CPU
/// temperature, Vcore and Super I/O rails stopped being read through PawnIO on 2026-10-05, and the
/// driver itself left on 2026-10-11.
pub fn gather_sensor_input() -> SensorInput {
    match nidavellir_gpu_nvapi::read_core_voltage_mv().filter(|mv| (400..=1500).contains(mv)) {
        Some(mv) => SensorInput {
            gpu_voltage_mv: Some(mv),
            gpu_voltage_source: Some(SensorSource::Nvapi),
        },
        None => SensorInput::default(),
    }
}
