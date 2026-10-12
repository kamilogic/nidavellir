//! Shared types of the GPU tuning domain: a point on the V/F curve and the outcome of a stability
//! test. The F1 step-machine sweep engine that used to live here left with the legacy workers
//! (0.6.0).

use serde::{Deserialize, Serialize};

/// A point on the voltage/frequency curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VfPoint {
    pub freq_mhz: u32,
    pub voltage_mv: u32,
}

/// Outcome of evaluating a candidate point (roadmap §12: silent errors matter).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StabilityResult {
    /// Compute validation matched the known answer and no WHEA/crash occurred.
    Stable,
    /// Driver/output produced wrong results without crashing — the real gap.
    SilentError,
    /// Hard fault (driver TDR / crash / WHEA). Caught by the Safe Loop on reboot.
    Crash,
    /// Behaviourally unstable without a wrong result or crash: a pre-hang stall (a render band
    /// ran toward the TDR watchdog) or a sustained throughput collapse versus the stock
    /// reference. The point is bad; the driver was never allowed to reset.
    Unstable,
}

impl StabilityResult {
    pub fn is_stable(self) -> bool {
        matches!(self, StabilityResult::Stable)
    }
}
