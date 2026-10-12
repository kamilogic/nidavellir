pub mod condemnation;
pub mod detector;
pub mod f2_observation;
pub mod gpu_sweep;
pub mod ipc;
pub mod nvml_gpu;
pub mod safe_loop;
pub mod sensor_input;
pub mod sensor_meta;
pub mod sensors;

pub use detector::{detect_hardware, HardwareInfo};
pub use ipc::{IpcRequest, IpcResponse};
pub use safe_loop::{
    decide_recovery, BootFlag, RecoveryAction, SafeLoopRecord, SafeLoopState, SafeLoopStore,
    TuningPoint,
};
pub use sensor_input::SensorInput;
pub use sensors::SensorReadings;
