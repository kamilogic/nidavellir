use nidavellir_core::detector::MotherboardInfo;
use nidavellir_core::sensor_input::SensorInput;
use nidavellir_core::sensor_meta::SensorSource;

/// GPU-only product (2026-10-05): CPU temperature, Vcore and Super I/O rails are no longer read
/// through PawnIO. Each read was a driver round trip every 2 s, and without LpcIO.bin every read also
/// logged a warning (25k lines a day).
pub fn gather_sensor_input(motherboard: &MotherboardInfo) -> SensorInput {
    let mut input = SensorInput::from_driver_parts(motherboard.clone(), None, None, false);

    if let Some(mv) = nidavellir_gpu_nvapi::read_core_voltage_mv()
        .filter(|mv| (400..=1500).contains(mv))
    {
        input.gpu_voltage_mv = Some(mv);
        input.gpu_voltage_source = Some(SensorSource::Nvapi);
    }

    input
}
