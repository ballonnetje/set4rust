//! Optional modern extras: none of them exist in the original renderer. They are all off by
//! default, and with all of them off the output is exactly the original look. Each one is
//! switched by its own flag and key:
//!
//! | effect | flag | key | where |
//! |---|---|---|---|
//! | heightfield ambient occlusion | `--ao` | F1 | per-node factor on the terrain's diffuse light ([`heightfield_ao`]) |
//! | sharpen | `--sharpen` | F2 | full-screen pass after the frame ([`PostFx`]) |
//! | smooth shadows | `--smooth-shadows` | F3 | sprite shadow pixels as 50 % black instead of the dither |
//! | HiDPI scaling | `--hidpi [factor]` | F4 | zoom × the window's scale factor |
//! | cloud shadows | `--clouds` | F5 | drifting dark and light patches over the world ([`crate::clouds`]) |
//!
//! **AO.** The terrain is a heightfield we know exactly, so occlusion is computed from the node
//! heights instead of from the depth buffer (there is none). For every node, six directions along
//! the hex grid are walked up to [`AO_RADIUS`] nodes; the steepest rise in each direction gives a
//! horizon angle, and the mean of their sines is the occluded fraction. The result (0..=255,
//! 255 = open) multiplies the vertex diffuse colour before fog, so shading, fog and specular keep
//! their original meaning.

/// Effect switches.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Effects {
    pub ao: bool,
    pub sharpen: bool,
    pub smooth_shadows: bool,
    /// Multiply the zoom by the display scale factor (window: the monitor's; capture: the flag's).
    pub hidpi: bool,
    pub clouds: bool,
}

/// Nodes walked per direction.
pub const AO_RADIUS: i32 = 10;
/// Node spacing on the ground plane and the height of one height unit, in the same units (the
/// renderer's 24 px triangle width; heights use the screen offset per unit, 1.2375 px, scaled for
/// the 2:1 view foreshortening).
const AO_SPACING: f32 = 24.0;
const AO_HEIGHT: f32 = 1.2375 * 2.0;
/// How much of the occluded fraction darkens.
const AO_STRENGTH: f32 = 0.9;

/// Ambient-occlusion factor (255 = unoccluded) of the nodes in rows `y0..=y1` of a `w × h` grid,
/// row-major into `out` (which holds the whole grid).
pub fn heightfield_ao(w: usize, h: usize, height: &dyn Fn(usize, usize) -> u8, y0: usize, y1: usize, out: &mut [u8]) {
    const DIRS: [(i32, i32); 6] = [(1, 0), (1, 1), (0, 1), (-1, 0), (-1, -1), (0, -1)];
    for y in y0..=y1.min(h - 1) {
        for x in 0..w {
            let h0 = height(x, y) as f32 * AO_HEIGHT;
            let mut occ = 0.0;
            for (dx, dy) in DIRS {
                let mut best = 0.0f32;
                for r in 1..=AO_RADIUS {
                    let (nx, ny) = (x as i32 + dx * r, y as i32 + dy * r);
                    if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                        break;
                    }
                    let rise = height(nx as usize, ny as usize) as f32 * AO_HEIGHT - h0;
                    best = best.max(rise / (r as f32 * AO_SPACING));
                }
                // sin(atan(t)) = t / sqrt(1 + t²)
                occ += best / (1.0 + best * best).sqrt();
            }
            let open = 1.0 - AO_STRENGTH * occ / DIRS.len() as f32;
            out[y * w + x] = (open.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
}

/// Pack per-node bytes four to a `u32` (little-endian), as the terrain shader reads them.
pub fn pack_bytes(b: &[u8]) -> Vec<u32> {
    b.chunks(4).map(|c| c.iter().enumerate().fold(0u32, |a, (i, &v)| a | (v as u32) << (8 * i))).collect()
}

// ------------------------------------------------------------------------------------------------
// Full-screen post pass (sharpen)

/// The frame is drawn into `scene` first, then sharpened into the real target.
pub struct PostFx {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    scene: Option<(wgpu::Texture, wgpu::BindGroup, (u32, u32))>,
    format: wgpu::TextureFormat,
}

impl PostFx {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sharpen"),
            source: wgpu::ShaderSource::Wgsl(SHARPEN.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("post"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sharpen"),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { label: Some("post"), ..Default::default() });
        PostFx { pipeline, layout, sampler, scene: None, format }
    }

    /// The offscreen texture the frame is rendered into (recreated on resize).
    pub fn scene_view(&mut self, device: &wgpu::Device, size: (u32, u32)) -> wgpu::TextureView {
        if self.scene.as_ref().is_none_or(|s| s.2 != size) {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("post scene"),
                size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("post"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&tex.create_view(&Default::default())),
                    },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ],
            });
            self.scene = Some((tex, bind, size));
        }
        self.scene.as_ref().unwrap().0.create_view(&Default::default())
    }

    /// Sharpen the offscreen frame into `target`.
    pub fn apply(&self, enc: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let Some((_, bind, _)) = &self.scene else { return };
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("sharpen"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bind, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// Unsharp mask over the 4-neighbourhood: `c + k·(c − mean(neighbours))`.
const SHARPEN: &str = r#"
@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

fn at(p: vec2<i32>, d: vec2<i32>) -> vec3<f32> {
    let size = vec2<i32>(textureDimensions(scene));
    return textureLoad(scene, clamp(p + d, vec2<i32>(0), size - 1), 0).rgb;
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<i32>(pos.xy);
    let c = at(p, vec2<i32>(0, 0));
    let n = (at(p, vec2<i32>(1, 0)) + at(p, vec2<i32>(-1, 0)) + at(p, vec2<i32>(0, 1)) + at(p, vec2<i32>(0, -1))) * 0.25;
    // Touch the sampler so the layout stays valid on every backend.
    let _unused = textureSampleLevel(scene, samp, vec2<f32>(0.0), 0.0);
    return vec4<f32>(clamp(c + 0.6 * (c - n), vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
"#;
