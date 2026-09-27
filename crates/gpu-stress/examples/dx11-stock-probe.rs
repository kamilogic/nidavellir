//! A bounded DX11 stock control; optional 1830 MHz ceiling, always restored afterwards.
//! `--replay-plan-1830-943` only replays the captured curve through the pure planner.
#[cfg(windows)]
fn main() -> Result<(), String> {
    use nidavellir_core::nvml_gpu::NvmlSampler;
    use nidavellir_gpu_stress::Dx11Qualifier;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use std::time::Duration;

    fn print_curve(label: &str) {
        let base = nidavellir_gpu_nvapi::read_vf_base_curve_modern();
        let live = nidavellir_gpu_nvapi::read_vf_curve_modern();
        for (index, mv, base_mhz) in base
            .into_iter()
            .filter(|(_, mv, _)| (900..=975).contains(mv))
        {
            let live_mhz = live
                .iter()
                .find(|(i, _, _)| *i == index)
                .map(|(_, _, mhz)| *mhz);
            println!("# curve={label}; index={index}; mv={mv}; base_mhz={base_mhz}; live_mhz={live_mhz:?}");
        }
    }

    struct StockClockRestore;
    impl Drop for StockClockRestore {
        fn drop(&mut self) {
            if let Err(error) = nidavellir_core::nvml_gpu::reset_core_clock_lock() {
                eprintln!("CLOCK RESET FAILED: {error}");
            }
        }
    }
    let (ceiling, duration_ms) = match std::env::args().nth(1).as_deref() {
        None => (false, 30_000),
        Some("--ceiling-1830") => (true, 30_000),
        Some("--warm-120") => (false, 120_000),
        Some("--plan-1830-943" | "--replay-plan-1830-943") => (false, 0),
        Some(_) => {
            return Err(
                "Use --ceiling-1830, --warm-120, --plan-1830-943 or --replay-plan-1830-943".into(),
            )
        }
    };

    for name in ["boot_flag.json", "gpu_applied.json"] {
        if std::path::Path::new("C:/ProgramData/Nidavellir")
            .join(name)
            .exists()
        {
            return Err(format!("Refusing stock probe while {name} exists"));
        }
    }
    if duration_ms == 0 {
        let replay = std::env::args().nth(1).as_deref() == Some("--replay-plan-1830-943");
        let base: Vec<_> = if replay {
            // Captured stock curve excerpt: target/beta/clean-run-20260914/anchor-plan-1830-943.txt.
            // This mode performs no NVAPI/NVML calls and starts no workload.
            vec![
                (77, 931, 1725),
                (78, 937, 1740),
                (79, 943, 1755),
                (80, 950, 1755),
                (81, 956, 1770),
                (82, 962, 1785),
                (83, 968, 1785),
                (84, 975, 1800),
                (85, 981, 1815),
                (86, 987, 1815),
                (87, 993, 1830),
                (88, 1000, 1830),
                (89, 1006, 1845),
                (90, 1012, 1845),
                (91, 1018, 1860),
                (92, 1025, 1860),
                (93, 1031, 1875),
            ]
        } else {
            nidavellir_gpu_nvapi::read_vf_base_curve_modern()
                .into_iter()
                .filter(|(_, mv, mhz)| (600..=1150).contains(mv) && (500..=3500).contains(mhz))
                .collect()
        };
        let anchor = base
            .iter()
            .find(|(_, mv, _)| *mv == 943)
            .ok_or("943 mV bin missing")?
            .0;
        let limits = nidavellir_gpu_nvapi::PositiveOffsetLimits::hardware_frontier(
            base.iter()
                .map(|(_, mv, _)| *mv)
                .min()
                .ok_or("Empty curve")?,
            1830,
            base.iter()
                .map(|(_, _, mhz)| *mhz)
                .min()
                .ok_or("Empty curve")?,
        );
        let plan = nidavellir_gpu_nvapi::plan_bounded_anchored_positive_offset(
            &base, anchor, 1830, 0, &limits,
        )?;
        for entry in plan
            .entries
            .iter()
            .filter(|e| (931..=1031).contains(&e.voltage_mv))
        {
            println!("{entry:?}");
        }
        let mut ordered = plan.entries.clone();
        ordered.sort_by_key(|e| e.voltage_mv);
        let descents = ordered
            .windows(2)
            .filter(|pair| pair[1].effective_mhz < pair[0].effective_mhz)
            .count();
        let higher_below_target = ordered
            .iter()
            .filter(|e| e.voltage_mv > plan.anchor.voltage_mv && e.effective_mhz < plan.target_mhz)
            .count();
        println!("# replay={replay}; descending_edges={descents}; higher_bins_below_target={higher_below_target}");
        return Ok(());
    }
    print_curve("before");
    let golden = Dx11Qualifier::new()?.capture_golden(2_000)?;
    let restore = if ceiling {
        nidavellir_core::nvml_gpu::lock_core_clock_max_mhz(1830)?;
        Some(StockClockRestore)
    } else {
        None
    };
    let ctx = Dx11Qualifier::new()?;
    let (sampler, limit) = NvmlSampler::init(0)?;
    println!(
        "# adapter={:?}; power_limit_w={limit:?}",
        ctx.adapter_identity()
    );
    println!("# golden={golden:?}");
    let done = Arc::new(AtomicBool::new(false));
    let done_for_sampler = done.clone();
    let worker = std::thread::spawn(move || {
        let mut samples = Vec::new();
        let mut next_curve_ms = 15_000;
        while !done_for_sampler.load(Ordering::SeqCst) {
            let sample = sampler.sample();
            if duration_ms == 120_000 && sample.t_ms >= next_curve_ms {
                print_curve(&format!("during-{}ms-{:?}C", sample.t_ms, sample.temp_c));
                next_curve_ms += 30_000;
            }
            samples.push(sample);
            std::thread::sleep(Duration::from_millis(30));
        }
        samples
    });
    let result = ctx.run_with_golden(duration_ms, golden, None);
    print_curve("after");
    done.store(true, Ordering::SeqCst);
    let samples = worker.join().map_err(|_| "Telemetry thread panicked")?;
    if restore.is_some() {
        nidavellir_core::nvml_gpu::reset_core_clock_lock()?;
        println!("# clock_reset=confirmed");
    }
    drop(restore);
    println!("# result={result:?}");
    println!("t_ms,power_w,core_mhz,mem_mhz,util_pct,temp_c,throttle_bits");
    for s in samples {
        println!(
            "{},{},{},{},{},{},{}",
            s.t_ms,
            s.power_w.map(|v| v.to_string()).unwrap_or_default(),
            s.core_mhz.map(|v| v.to_string()).unwrap_or_default(),
            s.mem_mhz.map(|v| v.to_string()).unwrap_or_default(),
            s.util_pct.map(|v| v.to_string()).unwrap_or_default(),
            s.temp_c.map(|v| v.to_string()).unwrap_or_default(),
            s.throttle_bits.map(|v| v.to_string()).unwrap_or_default()
        );
    }
    if result.result != nidavellir_core::gpu_sweep::StabilityResult::Stable
        || result.inconclusive_reason.is_some()
        || result.checks == 0
        || result.compute_checks == 0
    {
        return Err("DX11 stock control did not complete cleanly".into());
    }
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("DX11 stock control requires Windows");
    std::process::exit(1);
}
