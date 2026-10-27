//! FDTD on the GPU (same scheme as `fdtd::run_cpu`).

use std::sync::atomic::{AtomicBool, Ordering};

use bytemuck::{Pod, Zeroable};
use num_complex::Complex64;
use wgpu::util::DeviceExt;

use crate::fdtd::{COURANT, FdtdGrid, Spectra};
use crate::gpu::Gpu;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GridParams {
    cells: u32,
    nx: u32,
    nxy: u32,
    n_src: u32,
    l2: f32,
    lam: f32,
    n_recv: u32,
    nf: u32,
    dt: f32,
    _pad: [u32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct StepParams {
    n: u32,
    s: f32,
    _p: [u32; 62], // 256-byte stride for dynamic offsets
}

const STEPS_PER_SUBMIT: usize = 128;
/// cell_info: the air bit (bits 0–5: air neighbours; see fdtd.wgsl).
const AIR_BIT: u32 = 64;

fn dispatch(groups: u32) -> (u32, u32) {
    if groups <= 65535 { (groups, 1) } else { (65535, groups.div_ceil(65535)) }
}

impl Gpu {
    #[allow(clippy::too_many_arguments)]
    pub fn run_fdtd(
        &self,
        g: &FdtdGrid,
        source: &[(usize, f64)],
        signal: &[f64],
        receivers: &[usize],
        freqs: &[f64],
        cancel: &AtomicBool,
        progress: &(dyn Fn(f64) + Sync),
    ) -> Result<Option<Spectra>, String> {
        let dev = &self.device;
        let cells = g.cells();
        let bytes = cells as u64 * 4;
        if bytes > self.max_binding {
            return Err(format!("grid of {} M cells exceeds the GPU buffer limit", cells / 1_000_000));
        }
        if signal.len() >= 1 << 17 {
            return Err("too many time steps for the GPU phase accumulator".into());
        }
        if g.boundary.len() >= 1 << 25 {
            return Err("too many boundary nodes for the GPU layout".into());
        }
        // Source taps, one per cell (weights of a cell given twice add up).
        let mut merged: Vec<(usize, f64)> = Vec::with_capacity(source.len());
        for &(cell, w) in source {
            match merged.iter_mut().find(|m| m.0 == cell) {
                Some(m) => m.1 += w,
                None => merged.push((cell, w)),
            }
        }
        let taps: Vec<[f32; 2]> = merged.iter().map(|&(cell, w)| [f32::from_bits(cell as u32), w as f32]).collect();
        let meta: Vec<u32> = g
            .flags
            .iter()
            .zip(&g.bidx)
            .map(|(f, b)| (*f as u32 & 0x7f) | if *b == crate::fdtd::NO_BOUNDARY { 0 } else { (b + 1) << 7 })
            .collect();
        let mut bstart: Vec<u32> = Vec::with_capacity(g.boundary.len() + 1);
        let mut slots: Vec<[f32; 4]> = Vec::new();
        for b in &g.boundary {
            bstart.push(slots.len() as u32);
            slots.extend(b.slots.iter().map(|s| [s.inv_d as f32, s.cv as f32, s.k as f32, s.w as f32]));
        }
        bstart.push(slots.len() as u32);
        // One bit per cell: plain interior air (six air neighbours, no
        // boundary, no source). Those skip reading their cell info — the
        // bulk of a room's grid; a SIMD group shares one word.
        let interior = AIR_BIT | 63;
        let mut segs = vec![0u32; cells.div_ceil(32)];
        for (i, m) in meta.iter().enumerate() {
            if *m == interior {
                segs[i / 32] |= 1 << (i % 32);
            }
        }
        let qs: Vec<[f32; 2]> = freqs
            .iter()
            .map(|f| {
                let x = f * g.dt;
                // 7 significant bits: n (< 2^17) · q_hi is exact in f32.
                let e = 6 - x.log2().floor() as i32;
                let s = 2f64.powi(e);
                let hi = (x * s).round() / s;
                [hi as f32, (x - hi) as f32]
            })
            .collect();
        let params = GridParams {
            cells: cells as u32,
            nx: g.n[0] as u32,
            nxy: (g.n[0] * g.n[1]) as u32,
            n_src: taps.len() as u32,
            l2: (COURANT * COURANT) as f32,
            lam: COURANT as f32,
            n_recv: receivers.len() as u32,
            nf: freqs.len() as u32,
            dt: g.dt as f32,
            _pad: [0; 3],
        };
        let st = wgpu::BufferUsages::STORAGE;
        let init = |label: &str, data: &[u8], usage: wgpu::BufferUsages| {
            dev.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: if data.is_empty() { &[0u8; 16] } else { data },
                usage,
            })
        };
        let zeros = |label: &str, size: u64, extra: wgpu::BufferUsages| {
            dev.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: size.max(16), usage: st | extra, mapped_at_creation: false })
        };
        let grid_buf = init("grid", bytemuck::bytes_of(&params), wgpu::BufferUsages::UNIFORM);
        let steps: Vec<StepParams> =
            signal.iter().enumerate().map(|(n, s)| StepParams { n: n as u32, s: *s as f32, _p: [0; 62] }).collect();
        let meta_buf = init("meta", bytemuck::cast_slice(&meta), st);
        let recv: Vec<u32> = receivers.iter().map(|r| *r as u32).collect();
        let recv_buf = init("recv", bytemuck::cast_slice(&recv), st);
        let q_buf = init("q", bytemuck::cast_slice(&qs), st);
        let acc_size = (receivers.len() * freqs.len() * 8) as u64;
        let acc_buf = zeros("acc", acc_size, wgpu::BufferUsages::COPY_SRC);
        let bstart_buf = init("bstart", bytemuck::cast_slice(&bstart), st);
        let slot_buf = init("slots", bytemuck::cast_slice(&slots), st);
        let sstate_buf = zeros("sstate", (slots.len() * 8) as u64, wgpu::BufferUsages::empty());
        let seg_buf = init("segments", bytemuck::cast_slice(&segs), st);
        let taps_buf = init("taps", bytemuck::cast_slice(&taps), st);
        let a = zeros("p0", bytes, wgpu::BufferUsages::empty());
        let b = zeros("p1", bytes, wgpu::BufferUsages::empty());

        let module = dev.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fdtd"),
            source: wgpu::ShaderSource::Wgsl(include_str!("fdtd.wgsl").into()),
        });
        let entry = |binding: u32, ty: wgpu::BufferBindingType, dynamic: bool| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: dynamic, min_binding_size: None },
            count: None,
        };
        let ro = wgpu::BufferBindingType::Storage { read_only: true };
        let rw = wgpu::BufferBindingType::Storage { read_only: false };
        let layout = dev.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fdtd"),
            entries: &[
                entry(0, wgpu::BufferBindingType::Uniform, false),
                entry(1, wgpu::BufferBindingType::Uniform, true),
                entry(2, ro, false),
                entry(3, ro, false),
                entry(4, rw, false),
                entry(5, ro, false),
                entry(6, ro, false),
                entry(7, rw, false),
                entry(8, ro, false),
                entry(9, ro, false),
                entry(10, rw, false),
                entry(11, ro, false),
                entry(12, ro, false),
            ],
        });
        let pl = dev.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fdtd"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry: &str| {
            dev.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&pl),
                module: &module,
                entry_point: Some(entry),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            })
        };
        let (update, record, inject) = (pipeline("update"), pipeline("record"), pipeline("inject"));
        let tap_groups = (taps.len() as u32).div_ceil(64).max(1);
        let (ux, uy) = dispatch(cells.div_ceil(64) as u32);
        let (rx, ry) = dispatch((receivers.len() * freqs.len()).div_ceil(64).max(1) as u32);

        let total = signal.len();
        let mut done = 0;
        while done < total {
            if cancel.load(Ordering::Relaxed) {
                return Ok(None);
            }
            let end = (done + STEPS_PER_SUBMIT).min(total);
            // Per-submit step uniform, indexed with dynamic offsets.
            let step_buf = init("steps", bytemuck::cast_slice(&steps[done..end]), wgpu::BufferUsages::UNIFORM);
            let make = |cur: &wgpu::Buffer, prev: &wgpu::Buffer| {
                dev.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("fdtd"),
                    layout: &layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: grid_buf.as_entire_binding() },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                buffer: &step_buf,
                                offset: 0,
                                size: std::num::NonZeroU64::new(256),
                            }),
                        },
                        wgpu::BindGroupEntry { binding: 2, resource: meta_buf.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 3, resource: cur.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 4, resource: prev.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 5, resource: recv_buf.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 6, resource: q_buf.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 7, resource: acc_buf.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 8, resource: bstart_buf.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 9, resource: slot_buf.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 10, resource: sstate_buf.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 11, resource: seg_buf.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 12, resource: taps_buf.as_entire_binding() },
                    ],
                })
            };
            let (bind_ab, bind_ba) = (make(&a, &b), make(&b, &a));
            let mut enc = dev.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("fdtd") });
            {
                let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("fdtd"), timestamp_writes: None });
                for n in done..end {
                    // Even steps: p = a, prev = b; odd steps swap.
                    let bind = if n % 2 == 0 { &bind_ab } else { &bind_ba };
                    let off = ((n - done) * 256) as u32;
                    pass.set_bind_group(0, bind, &[off]);
                    pass.set_pipeline(&record);
                    pass.dispatch_workgroups(rx, ry, 1);
                    pass.set_pipeline(&update);
                    pass.dispatch_workgroups(ux, uy, 1);
                    pass.set_pipeline(&inject);
                    pass.dispatch_workgroups(tap_groups, 1, 1);
                }
            }
            let t0 = std::time::Instant::now();
            self.queue.submit([enc.finish()]);
            dev.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| e.to_string())?;
            crate::rest_after(t0.elapsed());
            done = end;
            progress(done as f64 / total as f64);
        }

        let download = dev.create_buffer(&wgpu::BufferDescriptor {
            label: Some("download"),
            size: acc_size.max(16),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = dev.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("fdtd-read") });
        enc.copy_buffer_to_buffer(&acc_buf, 0, &download, 0, acc_size.max(16));
        self.queue.submit([enc.finish()]);
        let slice = download.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        dev.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| e.to_string())?;
        let data = slice.get_mapped_range().map_err(|e| format!("{e:?}"))?;
        let v: Vec<[f32; 2]> = bytemuck::pod_collect_to_vec(&data);
        Ok(Some(v.into_iter().take(receivers.len() * freqs.len()).map(|x| Complex64::new(x[0] as f64, x[1] as f64)).collect()))
    }
}
