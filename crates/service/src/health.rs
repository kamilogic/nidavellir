//! A Core health line in core.log every 10 minutes, so a long session shows when the Core or the
//! NVIDIA driver starts to cost more. On 2026-10-05 a third-party hook spun inside nvlddmkm for
//! hours and nothing in the log showed it.
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

const EVERY: Duration = Duration::from_secs(600);
const SLOW_SENSOR_READ_MS: u64 = 1_000;

static SLOWEST_SENSOR_READ_MS: AtomicU64 = AtomicU64::new(0);

pub(crate) fn record_sensor_read(elapsed: Duration) {
    let ms = elapsed.as_millis() as u64;
    SLOWEST_SENSOR_READ_MS.fetch_max(ms, Ordering::Relaxed);
    if ms >= SLOW_SENSOR_READ_MS {
        tracing::warn!("sensor read took {ms} ms; the NVIDIA driver is answering slowly");
    }
}

pub(crate) fn spawn() {
    std::thread::spawn(|| {
        let (mut kernel, mut user) = cpu_ms();
        loop {
            std::thread::sleep(EVERY);
            let (k, u) = cpu_ms();
            tracing::info!(
                "health: last 10 min Core CPU kernel {} ms, user {} ms; nonpaged pool {} MB; slowest sensor read {} ms",
                k.saturating_sub(kernel),
                u.saturating_sub(user),
                nonpaged_pool_mb(),
                SLOWEST_SENSOR_READ_MS.swap(0, Ordering::Relaxed)
            );
            (kernel, user) = (k, u);
        }
    });
}

fn cpu_ms() -> (u64, u64) {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    let mut times = [FILETIME::default(); 4];
    let [created, exited, kernel, user] = &mut times;
    if unsafe { GetProcessTimes(GetCurrentProcess(), created, exited, kernel, user) }.is_err() {
        return (0, 0);
    }
    let ms = |t: &FILETIME| ((u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime)) / 10_000;
    (ms(kernel), ms(user))
}

fn nonpaged_pool_mb() -> usize {
    use windows::Win32::System::ProcessStatus::{K32GetPerformanceInfo, PERFORMANCE_INFORMATION};
    let mut info = PERFORMANCE_INFORMATION::default();
    let size = std::mem::size_of::<PERFORMANCE_INFORMATION>() as u32;
    if !unsafe { K32GetPerformanceInfo(&mut info, size) }.as_bool() {
        return 0;
    }
    info.KernelNonpaged * info.PageSize / (1024 * 1024)
}
