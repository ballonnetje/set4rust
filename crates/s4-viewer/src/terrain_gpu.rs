//! GPU side of the terrain: the 44 landscape pages as texture arrays, the node and piece storage
//! buffers (see terrain.rs), and one non-indexed draw per visible band, in band order so
//! primitive order = the original's draw order.

use crate::gpu::View;
use crate::terrain::TerrainMesh;
use crate::terrain_mips::cached_padded_mips;
use s4_formats::gfx::landscape::TexturePage;

pub const PAGE_SIZE: u32 = 256;
const UNIFORM_BYTES: u64 = 48;

/// Texture sampling mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    /// Point sampling of the raw pages: the original with `Filtering = 0`.
    Point,
    /// Bilinear sampling of the raw pages, no mips, wrapping: the original with `Filtering = 1`
    /// (the default). The device setup 0x65c980 sets stage 0's `D3DTSS_MAGFILTER` (0x10) and
    /// `D3DTSS_MINFILTER` (0x11) to `D3DTFG_LINEAR` (2) when gfx flag 8 (`Filtering`, 0x505e50) is
    /// set, else `POINT` (1); nothing in the release changes them afterwards (the other setter,
    /// 0x671a60, belongs to the hardware object engine, which is off: 0x116928e = 0). The pattern
    /// UVs' half-texel inset ([`s4_formats::gfx::landscape::PATTERN_UV_BIAS`]) keeps the footprint
    /// inside the cells.
    Linear,
}

pub struct TerrainGpu {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniforms: wgpu::Buffer,
    raw_view: wgpu::TextureView,
    padded_view: wgpu::TextureView,
    point: wgpu::Sampler,
    linear: wgpu::Sampler,
    trilinear: wgpu::Sampler,
    nodes: Option<wgpu::Buffer>,
    pieces: Option<wgpu::Buffer>,
    ao: Option<wgpu::Buffer>,
    binds: Option<[wgpu::BindGroup; 3]>,
    map_w: u32,
    bands: Vec<(f32, f32, std::ops::Range<u32>)>,
    pub filter: Filter,
    /// Use the mip chain (trilinear) when zoomed out below 1.
    pub mipmaps: bool,
    pub lighting: bool,
    /// Fog fade step 0..15 between each node's old and new fog level.
    pub fog_fade: u32,
    /// `g_iFadeYBorder`: vertices of rows above it use fade step 0.
    pub fog_border: i32,
    /// Optional heightfield ambient occlusion (effects.rs).
    pub ao_enabled: bool,
}

impl TerrainGpu {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat, pages: &[TexturePage]) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("terrain"),
            source: wgpu::ShaderSource::Wgsl(include_str!("terrain.wgsl").into()),
        });
        let storage = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("terrain"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                storage(3),
                storage(4),
                storage(5),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("terrain"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("terrain"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            // No culling (the original sets D3DCULL_NONE); winding differs between Up and Down.
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });

        // Raw pages (point sampling, faithful) and padded pages with a full mip chain.
        let n = pages.len().max(1) as u32;
        let levels = PAGE_SIZE.ilog2() + 1;
        let texture = |label, mips| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: PAGE_SIZE, height: PAGE_SIZE, depth_or_array_layers: n },
                mip_level_count: mips,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let raw = texture("landscape pages", 1);
        let padded = texture("landscape pages (padded, mips)", levels);
        let write = |tex: &wgpu::Texture, layer: u32, level: u32, data: &[u8]| {
            let s = PAGE_SIZE >> level;
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: tex,
                    mip_level: level,
                    origin: wgpu::Origin3d { x: 0, y: 0, z: layer },
                    aspect: wgpu::TextureAspect::All,
                },
                data,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(s * 4), rows_per_image: Some(s) },
                wgpu::Extent3d { width: s, height: s, depth_or_array_layers: 1 },
            );
        };
        for (i, p) in pages.iter().enumerate() {
            assert_eq!(p.size, PAGE_SIZE, "terrain renderer expects the HQ (256²) page set");
            write(&raw, i as u32, 0, &p.rgba);
        }
        for (i, page_levels) in cached_padded_mips(pages).iter().enumerate() {
            for (l, data) in page_levels.iter().enumerate() {
                write(&padded, i as u32, l as u32, data);
            }
        }
        let array_view = |tex: &wgpu::Texture, mip_count: Option<u32>| {
            tex.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                mip_level_count: mip_count,
                ..Default::default()
            })
        };
        let sampler = |mag, min, mip| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("terrain"),
                address_mode_u: wgpu::AddressMode::Repeat,
                address_mode_v: wgpu::AddressMode::Repeat,
                mag_filter: mag,
                min_filter: min,
                mipmap_filter: mip,
                ..Default::default()
            })
        };
        use wgpu::FilterMode::{Linear, Nearest};
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("terrain uniforms"),
            size: UNIFORM_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        TerrainGpu {
            pipeline,
            layout,
            uniforms,
            raw_view: array_view(&raw, None),
            padded_view: array_view(&padded, None),
            point: sampler(Nearest, Nearest, Nearest),
            linear: sampler(Linear, Linear, Nearest),
            // Anisotropic trilinear. The pages are squashed 2:1 vertically on screen (32 texels
            // over 12 px rows, 24 px wide), so an isotropic mip choice follows the vertical rate and
            // blurs twice too much horizontally. wgpu only allows anisotropy with all filters linear;
            // this sampler is used below zoom 1 only, where nothing is magnified anyway.
            trilinear: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("terrain (mips)"),
                address_mode_u: wgpu::AddressMode::Repeat,
                address_mode_v: wgpu::AddressMode::Repeat,
                mag_filter: Linear,
                min_filter: Linear,
                mipmap_filter: Linear,
                anisotropy_clamp: 16,
                ..Default::default()
            }),
            nodes: None,
            pieces: None,
            ao: None,
            binds: None,
            map_w: 0,
            bands: Vec::new(),
            filter: Filter::Point,
            mipmaps: false,
            lighting: true,
            fog_fade: 0,
            fog_border: -50000,
            ao_enabled: false,
        }
    }

    /// Upload what changed in `mesh` (everything the first time) and clear its dirty marks.
    pub fn sync(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, mesh: &mut TerrainMesh) {
        let w = mesh.width;
        let as_bytes = |v: &[u32]| {
            let mut out = Vec::with_capacity(v.len() * 4);
            for x in v {
                out.extend_from_slice(&x.to_le_bytes());
            }
            out
        };
        let fresh = self.map_w != w as u32
            || self.nodes.as_ref().is_none_or(|b| b.size() != mesh.nodes.len() as u64 * 4)
            || self.pieces.as_ref().is_none_or(|b| b.size() != mesh.pieces.len() as u64 * 4);
        if fresh {
            let buffer = |label, data: &[u32]| {
                let b = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(label),
                    size: data.len() as u64 * 4,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                queue.write_buffer(&b, 0, &as_bytes(data));
                b
            };
            let nodes = buffer("terrain nodes", &mesh.nodes);
            let pieces = buffer("terrain pieces", &mesh.pieces);
            let ao = buffer("terrain ao", &crate::effects::pack_bytes(&mesh.ao));
            let bind = |view: &wgpu::TextureView, sampler: &wgpu::Sampler| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("terrain"),
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: self.uniforms.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(view) },
                        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
                        wgpu::BindGroupEntry { binding: 3, resource: nodes.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 4, resource: pieces.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 5, resource: ao.as_entire_binding() },
                    ],
                })
            };
            self.binds = Some([
                bind(&self.raw_view, &self.point),
                bind(&self.raw_view, &self.linear),
                bind(&self.padded_view, &self.trilinear),
            ]);
            self.nodes = Some(nodes);
            self.pieces = Some(pieces);
            self.ao = Some(ao);
            self.map_w = w as u32;
        } else {
            use crate::terrain::join_ranges;
            if let Some(buf) = &self.nodes {
                for (a, b) in join_ranges(&mesh.dirty.nodes) {
                    queue.write_buffer(buf, (a * w * 4) as u64, &as_bytes(&mesh.nodes[a * w..(b + 1) * w]));
                }
            }
            if let Some(buf) = &self.ao {
                for (a, b) in join_ranges(&mesh.dirty.ao) {
                    // Whole u32 words covering the rows.
                    let (s, e) = ((a * w) / 4, ((b + 1) * w).div_ceil(4));
                    let packed = crate::effects::pack_bytes(&mesh.ao[s * 4..(e * 4).min(mesh.ao.len())]);
                    queue.write_buffer(buf, (s * 4) as u64, &as_bytes(&packed));
                }
            }
            if let Some(buf) = &self.pieces {
                let cw = (w - 1) * 2;
                for (a, b) in join_ranges(&mesh.dirty.pieces) {
                    queue.write_buffer(buf, (a * cw * 4) as u64, &as_bytes(&mesh.pieces[a * cw..(b + 1) * cw]));
                }
            }
        }
        mesh.dirty = Default::default();
        let tris_per_row = ((w - 1) * 2) as u32;
        self.bands = (0..mesh.bands.len())
            .map(|b| {
                let rows = mesh.band_rows(b);
                let range = rows.start as u32 * tris_per_row * 3..rows.end as u32 * tris_per_row * 3;
                (mesh.bands[b].y_min, mesh.bands[b].y_max, range)
            })
            .collect();
    }

    pub fn write_uniforms(&self, queue: &wgpu::Queue, view: View, (w, h): (u32, u32), srgb_target: bool) {
        let mut u = Vec::with_capacity(UNIFORM_BYTES as usize);
        let (w, h) = (w as f32, h as f32);
        for f in [view.x, view.y, 2.0 * view.zoom / w, -2.0 * view.zoom / h, 1.0 / w, -1.0 / h] {
            u.extend_from_slice(&f.to_le_bytes());
        }
        let flags = srgb_target as u32 | ((!self.lighting) as u32) << 1 | (self.ao_enabled as u32) << 2;
        for x in [self.fog_fade.min(15), flags, self.map_w, self.fog_border as u32, 0, 0] {
            u.extend_from_slice(&x.to_le_bytes());
        }
        queue.write_buffer(&self.uniforms, 0, &u);
    }

    /// Record the draws of the bands that intersect the view vertically.
    pub fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, view: View, (_, h): (u32, u32)) {
        let Some(binds) = &self.binds else { return };
        let bind = if self.mipmaps && view.zoom < 1.0 {
            &binds[2]
        } else if self.filter == Filter::Linear {
            &binds[1]
        } else {
            &binds[0]
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bind, &[]);
        let (top, bottom) = (view.y - 1.0, view.y + h as f32 / view.zoom + 1.0);
        for (y_min, y_max, range) in &self.bands {
            if *y_max >= top && *y_min <= bottom && !range.is_empty() {
                pass.draw(range.clone(), 0..1);
            }
        }
    }
}
