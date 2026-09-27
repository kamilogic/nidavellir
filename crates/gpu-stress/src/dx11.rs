use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::{Dx11AdapterIdentity, Dx11Golden, Dx11QualificationResult};
use nidavellir_core::gpu_sweep::StabilityResult;
use windows::core::PCSTR;
use windows::Win32::Foundation::BOOL;
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::{
    ID3DBlob, D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_0, D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_D24_UNORM_S8_UINT, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1};

const NVIDIA_VENDOR_ID: u32 = 0x10de;
const TARGET_WIDTH: u32 = 1536;
const TARGET_HEIGHT: u32 = 1536;
const SOURCE_DIM: u32 = 1024;
const COMPUTE_ELEMENTS: u32 = 65_536;
const GOLDEN_MIN_CHECKS: u32 = 3;
const CHECK_INTERVAL_FRAMES: u64 = 16;
/// Heavy frame: four overlapping full-screen instances. Light frame: one (about a quarter of the
/// pixel work) with the same compute dispatch, so it runs the target below the power cap.
const HEAVY_INSTANCES: u32 = 4;
const LIGHT_INSTANCES: u32 = 1;
const GPU_COMPLETION_TIMEOUT: Duration = Duration::from_millis(2_000);

const SHADER_SOURCE: &[u8] = br#"
struct VsOut { float4 position : SV_Position; float2 uv : TEXCOORD0; };

Texture2D<float4> source_texture : register(t0);
SamplerState source_sampler : register(s0);
RWStructuredBuffer<uint> compute_output : register(u0);

VsOut vs_main(uint id : SV_VertexID) {
    float2 p = float2((id << 1) & 2, id & 2);
    VsOut o;
    o.position = float4(p * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
    o.uv = p;
    return o;
}

float4 ps_main(VsOut input) : SV_Target {
    float3 v = source_texture.Sample(source_sampler, input.uv * 7.0).rgb + 0.03125;
    [unroll] for (uint i = 0; i < 48; ++i) {
        v = frac(v.yzx * float3(1.6180339, 1.4142135, 1.7320508)
            + v.zxy * 0.375 + float3(0.013, 0.017, 0.019));
        v = mad(v, 0.875, v.zxy * 0.125);
    }
    return float4(v, 0.42);
}

[numthreads(256, 1, 1)]
void cs_main(uint3 id : SV_DispatchThreadID) {
    uint v = id.x ^ 0x9e3779b9u;
    [unroll] for (uint i = 0; i < 64; ++i) {
        v = v * 1664525u + 1013904223u;
        v ^= v >> 13;
    }
    compute_output[id.x] = v;
}
"#;

pub struct Dx11Qualifier {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    render_target: ID3D11Texture2D,
    staging: ID3D11Texture2D,
    render_target_view: ID3D11RenderTargetView,
    depth_view: ID3D11DepthStencilView,
    depth_state: ID3D11DepthStencilState,
    blend_state: ID3D11BlendState,
    source_view: ID3D11ShaderResourceView,
    sampler: ID3D11SamplerState,
    vertex_shader: ID3D11VertexShader,
    pixel_shader: ID3D11PixelShader,
    compute_shader: ID3D11ComputeShader,
    compute_buffer: ID3D11Buffer,
    compute_staging: ID3D11Buffer,
    compute_uav: ID3D11UnorderedAccessView,
    completion_query: ID3D11Query,
    adapter: Dx11AdapterIdentity,
}

impl Dx11Qualifier {
    pub fn new() -> Result<Self, String> {
        unsafe {
            let (adapter, adapter_identity) = select_nvidia_adapter()?;
            let mut device = None;
            let mut context = None;
            D3D11CreateDevice(
                &adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                None,
                D3D11_CREATE_DEVICE_FLAG(0),
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
            .map_err(|e| format!("D3D11CreateDevice failed: {e}"))?;
            let device = device.ok_or_else(|| "D3D11 device was not returned".to_string())?;
            let context =
                context.ok_or_else(|| "D3D11 immediate context was not returned".to_string())?;

            let vertex_blob = compile_shader(b"vs_main\0", b"vs_5_0\0")?;
            let pixel_blob = compile_shader(b"ps_main\0", b"ps_5_0\0")?;
            let compute_blob = compile_shader(b"cs_main\0", b"cs_5_0\0")?;
            let vertex_bytes = std::slice::from_raw_parts(
                vertex_blob.GetBufferPointer().cast::<u8>(),
                vertex_blob.GetBufferSize(),
            );
            let pixel_bytes = std::slice::from_raw_parts(
                pixel_blob.GetBufferPointer().cast::<u8>(),
                pixel_blob.GetBufferSize(),
            );
            let compute_bytes = std::slice::from_raw_parts(
                compute_blob.GetBufferPointer().cast::<u8>(),
                compute_blob.GetBufferSize(),
            );
            let mut vertex_shader = None;
            device
                .CreateVertexShader(vertex_bytes, None, Some(&mut vertex_shader))
                .map_err(|e| format!("CreateVertexShader failed: {e}"))?;
            let mut pixel_shader = None;
            device
                .CreatePixelShader(pixel_bytes, None, Some(&mut pixel_shader))
                .map_err(|e| format!("CreatePixelShader failed: {e}"))?;
            let mut compute_shader = None;
            device
                .CreateComputeShader(compute_bytes, None, Some(&mut compute_shader))
                .map_err(|e| format!("CreateComputeShader failed: {e}"))?;

            let target_desc = D3D11_TEXTURE2D_DESC {
                Width: TARGET_WIDTH,
                Height: TARGET_HEIGHT,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
                CPUAccessFlags: 0,
                MiscFlags: 0,
            };
            let mut render_target = None;
            device
                .CreateTexture2D(&target_desc, None, Some(&mut render_target))
                .map_err(|e| format!("CreateTexture2D(render target) failed: {e}"))?;
            let render_target =
                render_target.ok_or_else(|| "D3D11 render target was not returned".to_string())?;
            let mut render_target_view = None;
            device
                .CreateRenderTargetView(&render_target, None, Some(&mut render_target_view))
                .map_err(|e| format!("CreateRenderTargetView failed: {e}"))?;

            let depth_desc = D3D11_TEXTURE2D_DESC {
                Format: DXGI_FORMAT_D24_UNORM_S8_UINT,
                BindFlags: D3D11_BIND_DEPTH_STENCIL.0 as u32,
                ..target_desc
            };
            let mut depth_texture = None;
            device
                .CreateTexture2D(&depth_desc, None, Some(&mut depth_texture))
                .map_err(|e| format!("CreateTexture2D(depth) failed: {e}"))?;
            let depth_texture =
                depth_texture.ok_or_else(|| "D3D11 depth texture was not returned".to_string())?;
            let mut depth_view = None;
            device
                .CreateDepthStencilView(&depth_texture, None, Some(&mut depth_view))
                .map_err(|e| format!("CreateDepthStencilView failed: {e}"))?;
            let depth_desc = D3D11_DEPTH_STENCIL_DESC {
                DepthEnable: BOOL(1),
                DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ALL,
                DepthFunc: D3D11_COMPARISON_ALWAYS,
                ..Default::default()
            };
            let mut depth_state = None;
            device
                .CreateDepthStencilState(&depth_desc, Some(&mut depth_state))
                .map_err(|e| format!("CreateDepthStencilState failed: {e}"))?;

            let mut blend_desc = D3D11_BLEND_DESC::default();
            blend_desc.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
                BlendEnable: BOOL(1),
                SrcBlend: D3D11_BLEND_SRC_ALPHA,
                DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
                BlendOp: D3D11_BLEND_OP_ADD,
                SrcBlendAlpha: D3D11_BLEND_ONE,
                DestBlendAlpha: D3D11_BLEND_ZERO,
                BlendOpAlpha: D3D11_BLEND_OP_ADD,
                RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
            };
            let mut blend_state = None;
            device
                .CreateBlendState(&blend_desc, Some(&mut blend_state))
                .map_err(|e| format!("CreateBlendState failed: {e}"))?;

            let mut source_data = vec![0u8; (SOURCE_DIM * SOURCE_DIM * 4) as usize];
            for (index, pixel) in source_data.chunks_exact_mut(4).enumerate() {
                let hash = (index as u32).wrapping_mul(2_654_435_761);
                pixel[0] = (hash >> 24) as u8;
                pixel[1] = (hash >> 16) as u8;
                pixel[2] = (hash >> 8) as u8;
                pixel[3] = 255;
            }
            let source_desc = D3D11_TEXTURE2D_DESC {
                Width: SOURCE_DIM,
                Height: SOURCE_DIM,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_IMMUTABLE,
                BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                CPUAccessFlags: 0,
                MiscFlags: 0,
            };
            let source_initial = D3D11_SUBRESOURCE_DATA {
                pSysMem: source_data.as_ptr().cast::<c_void>(),
                SysMemPitch: SOURCE_DIM * 4,
                SysMemSlicePitch: 0,
            };
            let mut source_texture = None;
            device
                .CreateTexture2D(
                    &source_desc,
                    Some(&source_initial),
                    Some(&mut source_texture),
                )
                .map_err(|e| format!("CreateTexture2D(source) failed: {e}"))?;
            let source_texture = source_texture
                .ok_or_else(|| "D3D11 source texture was not returned".to_string())?;
            let mut source_view = None;
            device
                .CreateShaderResourceView(&source_texture, None, Some(&mut source_view))
                .map_err(|e| format!("CreateShaderResourceView failed: {e}"))?;
            let sampler_desc = D3D11_SAMPLER_DESC {
                Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                AddressU: D3D11_TEXTURE_ADDRESS_WRAP,
                AddressV: D3D11_TEXTURE_ADDRESS_WRAP,
                AddressW: D3D11_TEXTURE_ADDRESS_WRAP,
                MaxLOD: f32::MAX,
                ..Default::default()
            };
            let mut sampler = None;
            device
                .CreateSamplerState(&sampler_desc, Some(&mut sampler))
                .map_err(|e| format!("CreateSamplerState failed: {e}"))?;

            let staging_desc = D3D11_TEXTURE2D_DESC {
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                ..target_desc
            };
            let mut staging = None;
            device
                .CreateTexture2D(&staging_desc, None, Some(&mut staging))
                .map_err(|e| format!("CreateTexture2D(staging) failed: {e}"))?;

            let compute_desc = D3D11_BUFFER_DESC {
                ByteWidth: COMPUTE_ELEMENTS * 4,
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_UNORDERED_ACCESS.0 as u32,
                CPUAccessFlags: 0,
                MiscFlags: D3D11_RESOURCE_MISC_BUFFER_STRUCTURED.0 as u32,
                StructureByteStride: 4,
            };
            let mut compute_buffer = None;
            device
                .CreateBuffer(&compute_desc, None, Some(&mut compute_buffer))
                .map_err(|e| format!("CreateBuffer(compute) failed: {e}"))?;
            let compute_buffer = compute_buffer
                .ok_or_else(|| "D3D11 compute buffer was not returned".to_string())?;
            let mut compute_uav = None;
            device
                .CreateUnorderedAccessView(&compute_buffer, None, Some(&mut compute_uav))
                .map_err(|e| format!("CreateUnorderedAccessView failed: {e}"))?;
            let compute_staging_desc = D3D11_BUFFER_DESC {
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: 0,
                ..compute_desc
            };
            let mut compute_staging = None;
            device
                .CreateBuffer(&compute_staging_desc, None, Some(&mut compute_staging))
                .map_err(|e| format!("CreateBuffer(compute staging) failed: {e}"))?;

            let query_desc = D3D11_QUERY_DESC {
                Query: D3D11_QUERY_EVENT,
                MiscFlags: 0,
            };
            let mut completion_query = None;
            device
                .CreateQuery(&query_desc, Some(&mut completion_query))
                .map_err(|e| format!("CreateQuery failed: {e}"))?;

            Ok(Self {
                device,
                context,
                render_target,
                staging: staging
                    .ok_or_else(|| "D3D11 staging texture was not returned".to_string())?,
                render_target_view: render_target_view
                    .ok_or_else(|| "D3D11 render-target view was not returned".to_string())?,
                depth_view: depth_view
                    .ok_or_else(|| "D3D11 depth view was not returned".to_string())?,
                depth_state: depth_state
                    .ok_or_else(|| "D3D11 depth state was not returned".to_string())?,
                blend_state: blend_state
                    .ok_or_else(|| "D3D11 blend state was not returned".to_string())?,
                source_view: source_view
                    .ok_or_else(|| "D3D11 source view was not returned".to_string())?,
                sampler: sampler.ok_or_else(|| "D3D11 sampler was not returned".to_string())?,
                vertex_shader: vertex_shader
                    .ok_or_else(|| "D3D11 vertex shader was not returned".to_string())?,
                pixel_shader: pixel_shader
                    .ok_or_else(|| "D3D11 pixel shader was not returned".to_string())?,
                compute_shader: compute_shader
                    .ok_or_else(|| "D3D11 compute shader was not returned".to_string())?,
                compute_buffer,
                compute_staging: compute_staging
                    .ok_or_else(|| "D3D11 compute staging buffer was not returned".to_string())?,
                compute_uav: compute_uav
                    .ok_or_else(|| "D3D11 compute UAV was not returned".to_string())?,
                completion_query: completion_query
                    .ok_or_else(|| "D3D11 completion query was not returned".to_string())?,
                adapter: adapter_identity,
            })
        }
    }

    pub fn adapter_identity(&self) -> Dx11AdapterIdentity {
        self.adapter.clone()
    }

    pub fn capture_golden(&self, sample_ms: u64) -> Result<Dx11Golden, String> {
        let started = Instant::now();
        let ((checksum, compute_checksum), frames) =
            self.capture_checksums(sample_ms, HEAVY_INSTANCES)?;
        let elapsed_us = started.elapsed().as_micros().max(1) as u64;
        let ((light_checksum, light_compute), _) =
            self.capture_checksums(sample_ms, LIGHT_INSTANCES)?;
        if light_compute != compute_checksum {
            return Err("DX11 light frame changed the stock compute checksum".into());
        }
        Ok(Dx11Golden {
            checksum,
            compute_checksum,
            adapter_luid: self.adapter.adapter_luid,
            frame_reference_us: (elapsed_us / frames.max(1)).clamp(1, u64::from(u32::MAX)) as u32,
            light_checksum,
        })
    }

    fn capture_checksums(&self, sample_ms: u64, instances: u32) -> Result<((u32, u32), u64), String> {
        let started = Instant::now();
        let mut checks = Vec::new();
        let mut frames = 0u64;
        while started.elapsed() < Duration::from_millis(sample_ms)
            || checks.len() < GOLDEN_MIN_CHECKS as usize
        {
            self.draw_frame(instances);
            frames = frames.saturating_add(1);
            if frames.is_multiple_of(CHECK_INTERVAL_FRAMES) {
                checks.push(self.readback_checksums()?);
            }
        }
        let Some(&first) = checks.first() else {
            return Err("DX11 golden captured no checksum".into());
        };
        if checks.iter().any(|candidate| *candidate != first) {
            return Err("DX11 stock golden was not deterministic".into());
        }
        Ok((first, frames))
    }

    pub fn run_with_golden(
        &self,
        duration_ms: u64,
        golden: Dx11Golden,
        cancel: Option<&AtomicBool>,
    ) -> Dx11QualificationResult {
        self.run_with_golden_observed(duration_ms, golden, cancel, &mut |_| {})
    }

    /// Reports submitted GPU work until its completion fence. CPU-only checksum work and idle
    /// are excluded. The callback is diagnostic/coverage only and must not alter GPU settings.
    pub fn run_with_golden_observed(
        &self,
        duration_ms: u64,
        golden: Dx11Golden,
        cancel: Option<&AtomicBool>,
        activity: &mut dyn FnMut(bool),
    ) -> Dx11QualificationResult {
        self.run_frames(duration_ms, golden, cancel, activity, false)
    }

    /// Same checks on the light frame, against `golden.light_checksum`.
    pub fn run_light_with_golden_observed(
        &self,
        duration_ms: u64,
        golden: Dx11Golden,
        cancel: Option<&AtomicBool>,
        activity: &mut dyn FnMut(bool),
    ) -> Dx11QualificationResult {
        self.run_frames(duration_ms, golden, cancel, activity, true)
    }

    fn run_frames(
        &self,
        duration_ms: u64,
        golden: Dx11Golden,
        cancel: Option<&AtomicBool>,
        activity: &mut dyn FnMut(bool),
        light: bool,
    ) -> Dx11QualificationResult {
        let instances = if light { LIGHT_INSTANCES } else { HEAVY_INSTANCES };
        let expected = (
            if light { golden.light_checksum } else { golden.checksum },
            golden.compute_checksum,
        );
        let started = Instant::now();
        if golden.adapter_luid != self.adapter.adapter_luid {
            return result(
                StabilityResult::Stable,
                0,
                0,
                0,
                started,
                false,
                Some("stock/candidate adapter LUID mismatch".into()),
            );
        }
        let cancelled = || cancel.is_some_and(|token| token.load(Ordering::SeqCst));
        if cancelled() || duration_ms == 0 {
            return result(
                StabilityResult::Stable,
                0,
                0,
                0,
                started,
                false,
                Some("dx11_no_work_requested".into()),
            );
        }
        self.submit_batch(instances);
        activity(true);
        let mut frames = CHECK_INTERVAL_FRAMES;
        let mut checks = 0u32;
        let mut compute_checks = 0u32;
        loop {
            let mut queued_next = false;
            // Copy and fence the current batch before queuing the next one. CPU checksums read
            // the separate staging resources while the GPU renders into the original resources.
            // At most one batch is ahead; normal/cancelled exits drain and check every batch.
            let readback = self.readback_checksums_with(|| {
                activity(false);
                if !cancelled() && started.elapsed() < Duration::from_millis(duration_ms) {
                    self.submit_batch(instances);
                    activity(true);
                    frames = frames.saturating_add(CHECK_INTERVAL_FRAMES);
                    queued_next = true;
                }
            });
            let failed = !matches!(&readback, Ok(pair) if *pair == expected);
            // A failed check may have one batch in flight. Fence it before returning to the
            // caller's stock reset; a device failure while draining takes precedence.
            let readback = if failed && queued_next {
                match self.wait_for_gpu_completion() {
                    Ok(()) => readback,
                    Err(error) => Err(error),
                }
            } else {
                readback
            };
            if failed { activity(false); }
            match readback {
                Ok(pair) if pair == expected => {
                    checks = checks.saturating_add(1);
                    compute_checks = compute_checks.saturating_add(1);
                }
                Ok(_) => {
                    return result(
                        StabilityResult::SilentError,
                        frames,
                        checks,
                        compute_checks,
                        started,
                        false,
                        None,
                    )
                }
                Err(error) => {
                    let timed_out = error.contains("completion timeout");
                    return result(
                        if timed_out {
                            StabilityResult::Unstable
                        } else {
                            StabilityResult::Crash
                        },
                        frames,
                        checks,
                        compute_checks,
                        started,
                        timed_out,
                        None,
                    );
                }
            }
            if !queued_next {
                break;
            }
        }
        let verdict = if checks == 0 {
            StabilityResult::Crash
        } else {
            StabilityResult::Stable
        };
        result(
            verdict,
            frames,
            checks,
            compute_checks,
            started,
            false,
            None,
        )
    }

    fn draw_frame(&self, instances: u32) {
        unsafe {
            self.context.OMSetRenderTargets(
                Some(&[Some(self.render_target_view.clone())]),
                &self.depth_view,
            );
            self.context
                .ClearRenderTargetView(&self.render_target_view, &[0.0, 0.0, 0.0, 1.0]);
            self.context
                .ClearDepthStencilView(&self.depth_view, D3D11_CLEAR_DEPTH.0, 1.0, 0);
            self.context.OMSetDepthStencilState(&self.depth_state, 0);
            self.context
                .OMSetBlendState(&self.blend_state, None, u32::MAX);
            self.context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: TARGET_WIDTH as f32,
                Height: TARGET_HEIGHT as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            }]));
            self.context
                .IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            self.context.VSSetShader(&self.vertex_shader, None);
            self.context.PSSetShader(&self.pixel_shader, None);
            self.context
                .PSSetShaderResources(0, Some(&[Some(self.source_view.clone())]));
            self.context
                .PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));
            self.context.DrawInstanced(3, instances, 0, 0);

            self.context.CSSetShader(&self.compute_shader, None);
            let uavs = [Some(self.compute_uav.clone())];
            self.context
                .CSSetUnorderedAccessViews(0, 1, Some(uavs.as_ptr()), None);
            self.context.Dispatch(COMPUTE_ELEMENTS.div_ceil(256), 1, 1);
        }
    }

    fn submit_batch(&self, instances: u32) {
        for _ in 0..CHECK_INTERVAL_FRAMES {
            self.draw_frame(instances);
        }
        // Dispatch queued work before the CPU starts hashing the previous readback.
        unsafe { self.context.Flush() };
    }

    fn readback_checksums(&self) -> Result<(u32, u32), String> {
        self.readback_checksums_with(|| {})
    }

    fn readback_checksums_with(&self, before_hash: impl FnOnce()) -> Result<(u32, u32), String> {
        unsafe {
            let no_uavs: [Option<ID3D11UnorderedAccessView>; 1] = [None];
            self.context
                .CSSetUnorderedAccessViews(0, 1, Some(no_uavs.as_ptr()), None);
            self.context
                .CopyResource(&self.staging, &self.render_target);
            self.context
                .CopyResource(&self.compute_staging, &self.compute_buffer);
            self.wait_for_gpu_completion()?;
            before_hash();

            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context
                .Map(&self.staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .map_err(|e| format!("DX11 staging Map failed: {e}"))?;
            let mut hash = 0x811c9dc5u32;
            for row in 0..TARGET_HEIGHT as usize {
                let bytes = std::slice::from_raw_parts(
                    (mapped.pData as *const u8).add(row * mapped.RowPitch as usize),
                    TARGET_WIDTH as usize * 4,
                );
                for byte in bytes {
                    hash = hash.wrapping_mul(0x01000193) ^ u32::from(*byte);
                }
            }
            self.context.Unmap(&self.staging, 0);

            let mut compute_mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context
                .Map(
                    &self.compute_staging,
                    0,
                    D3D11_MAP_READ,
                    0,
                    Some(&mut compute_mapped),
                )
                .map_err(|e| format!("DX11 compute staging Map failed: {e}"))?;
            let compute_bytes = std::slice::from_raw_parts(
                compute_mapped.pData.cast::<u8>(),
                (COMPUTE_ELEMENTS * 4) as usize,
            );
            let mut compute_hash = 0x811c9dc5u32;
            for byte in compute_bytes {
                compute_hash = compute_hash.wrapping_mul(0x01000193) ^ u32::from(*byte);
            }
            self.context.Unmap(&self.compute_staging, 0);
            Ok((hash, compute_hash))
        }
    }

    fn wait_for_gpu_completion(&self) -> Result<(), String> {
        unsafe {
            self.context.End(&self.completion_query);
            self.context.Flush();
            let wait_started = Instant::now();
            loop {
                let mut completed = BOOL(0);
                match self.context.GetData(
                    &self.completion_query,
                    Some((&mut completed as *mut BOOL).cast::<c_void>()),
                    std::mem::size_of::<BOOL>() as u32,
                    D3D11_ASYNC_GETDATA_DONOTFLUSH.0 as u32,
                ) {
                    Ok(()) if completed.as_bool() => return Ok(()),
                    Ok(()) => {}
                    Err(e) => return Err(format!("DX11 completion query failed: {e}")),
                }
                if wait_started.elapsed() >= GPU_COMPLETION_TIMEOUT {
                    let reason = self.device.GetDeviceRemovedReason().err();
                    return Err(format!(
                        "DX11 completion timeout; device_removed={reason:?}"
                    ));
                }
                std::thread::yield_now();
            }
        }
    }
}

fn result(
    verdict: StabilityResult,
    frames: u64,
    checks: u32,
    compute_checks: u32,
    started: Instant,
    timed_out: bool,
    inconclusive_reason: Option<String>,
) -> Dx11QualificationResult {
    let elapsed = started.elapsed();
    Dx11QualificationResult {
        result: verdict,
        frames,
        checks,
        compute_checks,
        fps: frames as f64 / elapsed.as_secs_f64().max(f64::EPSILON),
        elapsed_ms: elapsed.as_millis().try_into().unwrap_or(u64::MAX),
        timed_out,
        inconclusive_reason,
    }
}

unsafe fn select_nvidia_adapter() -> Result<(IDXGIAdapter1, Dx11AdapterIdentity), String> {
    let factory: IDXGIFactory1 =
        CreateDXGIFactory1().map_err(|e| format!("CreateDXGIFactory1 failed: {e}"))?;
    let mut candidates = Vec::new();
    for index in 0..32 {
        let Ok(adapter) = factory.EnumAdapters1(index) else {
            break;
        };
        let desc = adapter
            .GetDesc1()
            .map_err(|e| format!("IDXGIAdapter1::GetDesc1 failed: {e}"))?;
        if desc.VendorId == NVIDIA_VENDOR_ID {
            candidates.push((desc.DedicatedVideoMemory, adapter, desc));
        }
    }
    candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.0));
    let (_, adapter, desc) = candidates
        .into_iter()
        .next()
        .ok_or_else(|| "no NVIDIA DX11 adapter found".to_string())?;
    let name_end = desc
        .Description
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(desc.Description.len());
    let luid = (i64::from(desc.AdapterLuid.HighPart) << 32) | i64::from(desc.AdapterLuid.LowPart);
    Ok((
        adapter,
        Dx11AdapterIdentity {
            name: String::from_utf16_lossy(&desc.Description[..name_end]),
            vendor_id: desc.VendorId,
            device_id: desc.DeviceId,
            adapter_luid: luid,
        },
    ))
}

unsafe fn compile_shader(entry: &[u8], target: &[u8]) -> Result<ID3DBlob, String> {
    let mut code = None;
    let mut errors = None;
    let compiled = D3DCompile(
        SHADER_SOURCE.as_ptr().cast::<c_void>(),
        SHADER_SOURCE.len(),
        PCSTR::null(),
        None,
        None,
        PCSTR(entry.as_ptr()),
        PCSTR(target.as_ptr()),
        0,
        0,
        &mut code,
        Some(&mut errors),
    );
    if let Err(error) = compiled {
        let details = errors.map(|blob: ID3DBlob| {
            let bytes = std::slice::from_raw_parts(
                blob.GetBufferPointer().cast::<u8>(),
                blob.GetBufferSize(),
            );
            String::from_utf8_lossy(bytes)
                .trim_end_matches('\0')
                .to_string()
        });
        return Err(format!(
            "D3DCompile failed: {error}; {}",
            details.unwrap_or_default()
        ));
    }
    code.ok_or_else(|| "D3DCompile returned no shader bytecode".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "stock-GPU integration smoke; run explicitly on Windows hardware"]
    fn stock_golden_and_candidate_readback_are_stable() {
        let stock = Dx11Qualifier::new().expect("native DX11 stock context");
        assert_eq!(stock.adapter_identity().vendor_id, NVIDIA_VENDOR_ID);
        let golden = stock
            .capture_golden(1_000)
            .expect("deterministic stock golden");
        drop(stock);
        let candidate = Dx11Qualifier::new().expect("fresh native DX11 candidate context");
        let mut activity = Vec::new();
        let run = candidate.run_with_golden_observed(1_000, golden, None, &mut |active| activity.push(active));
        assert_eq!(activity.first(), Some(&true));
        assert_eq!(activity.last(), Some(&false));
        assert!(activity.windows(2).all(|pair| pair[0] != pair[1]));
        assert_eq!(activity.len() as u64, run.frames / CHECK_INTERVAL_FRAMES * 2);
        assert_eq!(run.result, StabilityResult::Stable);
        assert!(run.checks > 0);
        assert!(run.compute_checks > 0);
        assert!(!run.timed_out);
        assert_eq!(run.frames, u64::from(run.checks) * CHECK_INTERVAL_FRAMES);
        assert_eq!(run.checks, run.compute_checks);
        assert_ne!(golden.light_checksum, golden.checksum, "light frame is a different image");
        let light = candidate.run_light_with_golden_observed(1_000, golden, None, &mut |_| {});
        assert_eq!((light.result, light.checks > 0), (StabilityResult::Stable, true));
        let wrong_light = Dx11Golden { light_checksum: golden.light_checksum ^ 1, ..golden };
        let failed = candidate.run_light_with_golden_observed(1_000, wrong_light, None, &mut |_| {});
        assert_eq!(failed.result, StabilityResult::SilentError);

        for bad in [
            Dx11Golden {
                checksum: golden.checksum ^ 1,
                ..golden
            },
            Dx11Golden {
                compute_checksum: golden.compute_checksum ^ 1,
                ..golden
            },
        ] {
            let mut activity = Vec::new();
            let failed = candidate.run_with_golden_observed(1_000, bad, None, &mut |active| activity.push(active));
            assert_eq!(activity.last(), Some(&false), "failure must drain and close activity before stock reset");
            assert_eq!(failed.result, StabilityResult::SilentError);
            assert_eq!(failed.checks, 0);
            assert!(failed.frames <= 2 * CHECK_INTERVAL_FRAMES);
            // The preceding error drained its queued batch; the context remains usable.
            let recovered = candidate.run_with_golden(100, golden, None);
            assert_eq!(recovered.result, StabilityResult::Stable);
            assert_eq!(
                recovered.frames,
                u64::from(recovered.checks) * CHECK_INTERVAL_FRAMES
            );
        }
        let stop = AtomicBool::new(true);
        let cancelled = candidate.run_with_golden(1_000, golden, Some(&stop));
        assert_eq!(cancelled.frames, 0);
        assert!(cancelled.inconclusive_reason.is_some());
        stop.store(false, Ordering::SeqCst);
        let cancelled = std::thread::scope(|scope| {
            scope.spawn(|| {
                std::thread::sleep(Duration::from_millis(100));
                stop.store(true, Ordering::SeqCst);
            });
            candidate.run_with_golden(10_000, golden, Some(&stop))
        });
        assert!(cancelled.elapsed_ms < 5_000);
        assert_eq!(cancelled.result, StabilityResult::Stable);
        assert_eq!(
            cancelled.frames,
            u64::from(cancelled.checks) * CHECK_INTERVAL_FRAMES
        );
    }
}
