//! Direct3D 12 crash evidence (field report 2026-10-19: an RTX 5070 hung its GPU in the DX12
//! composite load at stock, while Vulkan passed). Windows' Device Removed Extended Data records the
//! commands each queue had started and finished, and the faulting address, so a report names the
//! command that hung instead of only "device lost".

use windows::core::{Interface, PCWSTR};
use windows::Win32::Graphics::Direct3D12::{
    D3D12GetDebugInterface, ID3D12Device, ID3D12DeviceRemovedExtendedData1,
    ID3D12DeviceRemovedExtendedDataSettings, D3D12_AUTO_BREADCRUMB_NODE1,
    D3D12_DRED_ALLOCATION_NODE1, D3D12_DRED_ENABLEMENT_FORCED_ON,
};

/// Turns on breadcrumbs and page-fault tracking for every D3D12 device this process creates from
/// now on; wgpu creates its device when it enumerates adapters, so this runs before that.
pub(crate) fn enable() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| unsafe {
        let mut settings: Option<ID3D12DeviceRemovedExtendedDataSettings> = None;
        match D3D12GetDebugInterface(&mut settings) {
            Ok(()) => {
                if let Some(settings) = settings {
                    settings.SetAutoBreadcrumbsEnablement(D3D12_DRED_ENABLEMENT_FORCED_ON);
                    settings.SetPageFaultEnablement(D3D12_DRED_ENABLEMENT_FORCED_ON);
                }
            }
            Err(error) => log::warn!("DX12 crash evidence (DRED) unavailable: {error}"),
        }
    });
}

/// Why the device was removed and the GPU's last commands; `None` while it works.
pub(crate) fn removal_report(device: &ID3D12Device) -> Option<String> {
    let reason = unsafe { device.GetDeviceRemovedReason() }.err()?.code().0 as u32;
    let mut report = format!("DX12 device removed: {} ({reason:#010x})", reason_name(reason));
    if let Ok(dred) = device.cast::<ID3D12DeviceRemovedExtendedData1>() {
        if let Ok(output) = unsafe { dred.GetAutoBreadcrumbsOutput1() } {
            report += &unsafe { breadcrumbs(output.pHeadAutoBreadcrumbNode) };
        }
        if let Ok(fault) = unsafe { dred.GetPageFaultAllocationOutput1() } {
            if fault.PageFaultVA != 0 {
                report += &format!(
                    "\n  page fault at {:#x}; allocations there: {}; recently freed: {}",
                    fault.PageFaultVA,
                    unsafe { allocations(fault.pHeadExistingAllocationNode) },
                    unsafe { allocations(fault.pHeadRecentFreedAllocationNode) }
                );
            }
        }
    }
    Some(report)
}

pub(crate) fn reason_name(code: u32) -> &'static str {
    match code {
        0x887A_0006 => "GPU hung (a command ran too long)",
        0x887A_0007 => "device reset (a badly formed command)",
        0x887A_0020 => "driver internal error",
        0x887A_0005 => "device removed",
        0x887A_0001 => "invalid call",
        0x8007_000E => "out of memory",
        _ => "other",
    }
}

/// Command lists the GPU did not finish, with the commands around the one it stopped at.
unsafe fn breadcrumbs(mut node: *const D3D12_AUTO_BREADCRUMB_NODE1) -> String {
    let mut out = String::new();
    for _ in 0..64 {
        let Some(crumb) = node.as_ref() else { break };
        let total = crumb.BreadcrumbCount as usize;
        let done = crumb.pLastBreadcrumbValue.as_ref().map_or(0, |value| *value as usize);
        if done < total && !crumb.pCommandHistory.is_null() {
            let ops: Vec<i32> = std::slice::from_raw_parts(crumb.pCommandHistory, total).iter().map(|op| op.0).collect();
            out += &format!(
                "\n  queue {:?} list {:?}: {done} of {total} commands done: {}",
                name(crumb.pCommandQueueDebugNameW),
                name(crumb.pCommandListDebugNameW),
                op_window(&ops, done)
            );
        }
        node = crumb.pNext;
    }
    if out.is_empty() {
        out = "\n  no unfinished command list recorded".into();
    }
    out
}

unsafe fn allocations(mut node: *const D3D12_DRED_ALLOCATION_NODE1) -> String {
    let mut names = Vec::new();
    for _ in 0..16 {
        let Some(allocation) = node.as_ref() else { break };
        names.push(format!("{}:{}", allocation.AllocationType.0, name(allocation.ObjectNameW)));
        node = allocation.pNext;
    }
    if names.is_empty() { "none".into() } else { names.join(", ") }
}

/// An unnamed queue, command list or allocation has a null name (wgpu names no queue): reading it
/// as a string crashed the Core at the first DX12 hang it reported (field report 2026-10-20).
unsafe fn name(text: PCWSTR) -> String {
    if text.is_null() { String::new() } else { text.to_string().unwrap_or_default() }
}

/// The commands just before the first unfinished one (marked `>>`) and a few after it.
pub(crate) fn op_window(ops: &[i32], done: usize) -> String {
    let start = done.saturating_sub(4);
    let end = (done + 4).min(ops.len());
    ops[start..end]
        .iter()
        .enumerate()
        .map(|(offset, op)| {
            let marker = if start + offset == done { ">>" } else { "" };
            format!("{marker}{}", op_name(*op))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn op_name(op: i32) -> String {
    let name = match op {
        0 => "SetMarker",
        1 => "BeginEvent",
        2 => "EndEvent",
        3 => "DrawInstanced",
        4 => "DrawIndexedInstanced",
        5 => "ExecuteIndirect",
        6 => "Dispatch",
        7 => "CopyBufferRegion",
        8 => "CopyTextureRegion",
        9 => "CopyResource",
        12 => "ClearRenderTargetView",
        13 => "ClearUnorderedAccessView",
        14 => "ClearDepthStencilView",
        15 => "ResourceBarrier",
        18 => "ResolveQueryData",
        19 => "BeginSubmission",
        20 => "EndSubmission",
        26 => "WriteBufferImmediate",
        45 => "Barrier",
        46 => "BeginCommandList",
        _ => return format!("op{op}"),
    };
    name.into()
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_report_marks_the_command_the_gpu_stopped_at() {
        // BeginCommandList, barrier, draw, copy x3, dispatch, end: stopped at the dispatch.
        let ops = [46, 15, 3, 7, 7, 7, 6, 20];
        assert_eq!(
            super::op_window(&ops, 6),
            "DrawInstanced CopyBufferRegion CopyBufferRegion CopyBufferRegion >>Dispatch EndSubmission"
        );
        assert_eq!(super::op_window(&ops, 0), ">>BeginCommandList ResourceBarrier DrawInstanced CopyBufferRegion");
        assert_eq!(super::op_window(&[99], 0), ">>op99");
        assert_eq!(super::reason_name(0x887A_0006), "GPU hung (a command ran too long)");
    }

    #[test]
    fn unnamed_queues_lists_and_allocations_are_reported_without_reading_a_null_name() {
        use windows::Win32::Graphics::Direct3D12::{
            D3D12_AUTO_BREADCRUMB_NODE1, D3D12_AUTO_BREADCRUMB_OP, D3D12_DRED_ALLOCATION_NODE1,
        };
        let history = [46, 6, 20].map(D3D12_AUTO_BREADCRUMB_OP);
        let done = 1u32;
        let crumb = D3D12_AUTO_BREADCRUMB_NODE1 {
            BreadcrumbCount: 3,
            pLastBreadcrumbValue: &done,
            pCommandHistory: history.as_ptr(),
            ..Default::default()
        };
        let allocation = D3D12_DRED_ALLOCATION_NODE1::default();
        unsafe {
            assert_eq!(
                super::breadcrumbs(&crumb),
                "\n  queue \"\" list \"\": 1 of 3 commands done: BeginCommandList >>Dispatch EndSubmission"
            );
            assert_eq!(super::allocations(&allocation), "0:");
        }
    }
}
