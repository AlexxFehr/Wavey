//! GPU accumulation with wgpu (Metal on macOS; Vulkan/DX12 elsewhere).

use std::sync::OnceLock;

use bytemuck::{Pod, Zeroable};
use num_complex::Complex32;
use wgpu::util::DeviceExt;

use crate::accumulate::AccumulateJob;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    nf: u32,
    n_recv: u32,
    cells: u32,
    omni: u32,
    r_ref: f32,
    _p: [u32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuPath {
    r_hi: f32,
    r_lo: f32,
    refl_start: u32,
    order: u32,
    idx: [u32; 4],
    w: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuFreq {
    h0: [f32; 2],
    q_hi: f32,
    q_lo: f32,
    air: f32,
    _p: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuSurf {
    r: [f32; 2],
    zeta: [f32; 2],
    spec: f32,
    has_zeta: u32,
    _p: [u32; 2],
}

pub struct Gpu {
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    pub(crate) max_binding: u64,
    pub name: String,
}

/// The shared GPU context, or None if no adapter is available.
pub fn gpu() -> Option<&'static Gpu> {
    static GPU: OnceLock<Option<Gpu>> = OnceLock::new();
    GPU.get_or_init(|| Gpu::new().ok()).as_ref()
}

/// Splits `x` into a high part with `bits` significant bits and the remainder.
fn split(x: f64, bits: i32) -> (f32, f32) {
    if x == 0.0 {
        return (0.0, 0.0);
    }
    let e = bits - 1 - x.abs().log2().floor() as i32;
    let scale = 2f64.powi(e);
    let hi = (x * scale).round() / scale;
    (hi as f32, (x - hi) as f32)
}

impl Gpu {
    fn new() -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(|e| e.to_string())?;
        let limits = adapter.limits();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("wavey"),
            required_features: wgpu::Features::empty(),
            required_limits: limits.clone(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .map_err(|e| e.to_string())?;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ism"),
            source: wgpu::ShaderSource::Wgsl(include_str!("ism.wgsl").into()),
        });
        let storage = |binding: u32, read_only: bool| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ism"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                storage(1, true),
                storage(2, true),
                storage(3, true),
                storage(4, true),
                storage(5, true),
                storage(6, true),
                storage(7, false),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ism"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("ism"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
        let info = adapter.get_info();
        Ok(Gpu {
            device,
            queue,
            pipeline,
            layout,
            max_binding: (limits.max_storage_buffer_binding_size as u64).min(limits.max_buffer_size),
            name: format!("{} ({:?})", info.name, info.backend),
        })
    }

    /// Same result as `accumulate_cpu`, computed on the GPU in f32.
    pub fn accumulate(&self, job: &AccumulateJob) -> Result<Vec<Complex32>, String> {
        let nf = job.spectra.freqs.len();
        let nr = job.receivers();
        let c = job.spectra.k[0] / (2.0 * std::f64::consts::PI * job.spectra.freqs[0]); // 1/c
        let freqs: Vec<GpuFreq> = (0..nf)
            .map(|fi| {
                let (q_hi, q_lo) = split(job.spectra.freqs[fi] * c, 8);
                GpuFreq {
                    h0: [job.h0[fi].re as f32, job.h0[fi].im as f32],
                    q_hi,
                    q_lo,
                    air: job.spectra.air_np[fi] as f32,
                    _p: [0.0; 3],
                }
            })
            .collect();
        let surf: Vec<GpuSurf> = job
            .surfaces
            .iter()
            .flat_map(|s| {
                (0..nf).map(move |fi| GpuSurf {
                    r: [s.r_fixed[fi].re as f32, s.r_fixed[fi].im as f32],
                    zeta: s.zeta.as_ref().map_or([0.0; 2], |z| [z[fi].re as f32, z[fi].im as f32]),
                    spec: s.specular[fi] as f32,
                    has_zeta: s.zeta.is_some() as u32,
                    _p: [0; 2],
                })
            })
            .collect();
        let dir: Vec<[f32; 2]> = match &job.directivity.grid {
            Some(g) => g.mag_db.iter().zip(&g.phase_deg).map(|(m, p)| [*m, *p]).collect(),
            None => vec![[0.0, 0.0]],
        };
        let refl: Vec<[u32; 2]> = job.paths.reflections.iter().map(|(s, c)| [*s, c.to_bits()]).collect();

        let init = |label: &str, data: &[u8], usage: wgpu::BufferUsages| {
            self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                // Bindings may not be empty.
                contents: if data.is_empty() { &[0u8; 16] } else { data },
                usage,
            })
        };
        let st = wgpu::BufferUsages::STORAGE;
        let freq_buf = init("freqs", bytemuck::cast_slice(&freqs), st);
        let surf_buf = init("surf", bytemuck::cast_slice(&surf), st);
        let dir_buf = init("dir", bytemuck::cast_slice(&dir), st);
        let refl_buf = init("refl", bytemuck::cast_slice(&refl), st);

        // Chunk receivers so path and output buffers stay within binding limits.
        let budget = (self.max_binding / 2).min(256 << 20);
        let mut out = vec![Complex32::new(0.0, 0.0); nr * nf];
        let mut r0 = 0;
        while r0 < nr {
            let mut r1 = r0 + 1;
            let fits = |r1: usize| {
                let paths = (job.paths.offsets[r1] - job.paths.offsets[r0]) as u64 * 48;
                let outb = ((r1 - r0) * nf * 8) as u64;
                paths <= budget && outb <= budget
            };
            let mut step = 1;
            while r1 < nr && fits((r1 + step).min(nr)) {
                r1 = (r1 + step).min(nr);
                step *= 2;
            }
            while r1 < nr && fits(r1 + 1) {
                r1 += 1;
            }
            self.run_chunk(job, r0, r1, &freq_buf, &surf_buf, &dir_buf, &refl_buf, &mut out[r0 * nf..r1 * nf])?;
            r0 = r1;
        }
        Ok(out)
    }

    #[allow(clippy::too_many_arguments)]
    fn run_chunk(
        &self,
        job: &AccumulateJob,
        r0: usize,
        r1: usize,
        freq_buf: &wgpu::Buffer,
        surf_buf: &wgpu::Buffer,
        dir_buf: &wgpu::Buffer,
        refl_buf: &wgpu::Buffer,
        out: &mut [Complex32],
    ) -> Result<(), String> {
        let nf = job.spectra.freqs.len();
        let base = job.paths.offsets[r0];
        let offsets: Vec<u32> = job.paths.offsets[r0..=r1].iter().map(|o| o - base).collect();
        let paths: Vec<GpuPath> = job.paths.paths[base as usize..job.paths.offsets[r1] as usize]
            .iter()
            .map(|p| {
                // Keep r_hi exactly representable with 16 significant bits.
                let bits = 16 - (p.r + 1.0).log2().ceil().max(0.0) as i32;
                let scale = 2f64.powi(bits.clamp(0, 8));
                let r_hi = (p.r * scale).round() / scale;
                GpuPath {
                    r_hi: r_hi as f32,
                    r_lo: (p.r - r_hi) as f32,
                    refl_start: p.refl_start,
                    order: p.order as u32,
                    idx: p.cell.idx,
                    w: p.cell.w,
                }
            })
            .collect();
        let params = Params {
            nf: nf as u32,
            n_recv: (r1 - r0) as u32,
            cells: job.directivity.cells_per_freq() as u32,
            omni: job.directivity.grid.is_none() as u32,
            r_ref: job.r_ref as f32,
            _p: [0; 3],
        };
        let dev = &self.device;
        let init = |label: &str, data: &[u8], usage: wgpu::BufferUsages| {
            dev.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: if data.is_empty() { &[0u8; 16] } else { data },
                usage,
            })
        };
        let st = wgpu::BufferUsages::STORAGE;
        let params_buf = init("params", bytemuck::bytes_of(&params), wgpu::BufferUsages::UNIFORM);
        let off_buf = init("offsets", bytemuck::cast_slice(&offsets), st);
        let path_buf = init("paths", bytemuck::cast_slice(&paths), st);
        let out_size = ((r1 - r0) * nf * 8) as u64;
        let out_buf = dev.create_buffer(&wgpu::BufferDescriptor {
            label: Some("out"),
            size: out_size,
            usage: st | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let download = dev.create_buffer(&wgpu::BufferDescriptor {
            label: Some("download"),
            size: out_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let entries: Vec<wgpu::BindGroupEntry> = [
            &params_buf, &off_buf, &path_buf, refl_buf, freq_buf, dir_buf, surf_buf, &out_buf,
        ]
        .iter()
        .enumerate()
        .map(|(i, b)| wgpu::BindGroupEntry { binding: i as u32, resource: b.as_entire_binding() })
        .collect();
        let bind = dev.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("ism"), layout: &self.layout, entries: &entries });

        let total = (r1 - r0) * nf;
        let groups = total.div_ceil(64) as u32;
        let (gx, gy) = if groups <= 65535 { (groups, 1) } else { (65535, groups.div_ceil(65535)) };
        let mut enc = dev.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("ism") });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("ism"), timestamp_writes: None });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(gx, gy, 1);
        }
        enc.copy_buffer_to_buffer(&out_buf, 0, &download, 0, out_size);
        self.queue.submit([enc.finish()]);
        let slice = download.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        dev.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| e.to_string())?;
        let data = slice.get_mapped_range().map_err(|e| format!("{e:?}"))?;
        let values: Vec<[f32; 2]> = bytemuck::pod_collect_to_vec(&data);
        for (o, v) in out.iter_mut().zip(values) {
            *o = Complex32::new(v[0], v[1]);
        }
        Ok(())
    }
}
