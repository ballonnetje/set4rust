//! [ours] The video picture (s4-video's decoded frames): the Y, U and V planes as three textures,
//! turned into RGB in the shader (BT.601, limited range: what ffmpeg assumes for Bink [inf]) and
//! scaled to fit the window with the aspect kept, on black. While shown it is the whole frame
//! ([`crate::gpu::Gpu::video`]).

const SHADER: &str = r#"
struct U { rect: vec4<f32>, srgb: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var ty: texture_2d<f32>;
@group(0) @binding(2) var tu: texture_2d<f32>;
@group(0) @binding(3) var tv: texture_2d<f32>;
@group(0) @binding(4) var smp: sampler;

struct V { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };

@vertex
fn vs(@builtin(vertex_index) i: u32) -> V {
    let c = vec2<f32>(f32(i & 1u), f32(i >> 1u));
    var o: V;
    // rect: left, top, right, bottom in clip space.
    o.pos = vec4<f32>(mix(u.rect.x, u.rect.z, c.x), mix(u.rect.y, u.rect.w, c.y), 0.0, 1.0);
    o.uv = c;
    return o;
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

@fragment
fn fs(v: V) -> @location(0) vec4<f32> {
    let y = (textureSample(ty, smp, v.uv).r - 16.0 / 255.0) * 1.164383;
    let cb = textureSample(tu, smp, v.uv).r - 0.501961;
    let cr = textureSample(tv, smp, v.uv).r - 0.501961;
    var rgb = clamp(vec3<f32>(y + 1.596027 * cr, y - 0.391762 * cb - 0.812968 * cr, y + 2.017232 * cb), vec3<f32>(0.0), vec3<f32>(1.0));
    if (u.srgb.x > 0.5) {
        rgb = to_linear(rgb);
    }
    return vec4<f32>(rgb, 1.0);
}
"#;

pub struct VideoLayer {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    /// The planes' textures and their bind group, for the current picture size.
    planes: Option<([wgpu::Texture; 3], wgpu::BindGroup, (u32, u32))>,
}

impl VideoLayer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> VideoLayer {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("video"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let tex = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("video"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                tex(1),
                tex(2),
                tex(3),
                wgpu::BindGroupLayoutEntry { binding: 4, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        // Linear: the 640×480 picture is scaled to the window.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("video"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("video"), bind_group_layouts: &[&layout], push_constant_ranges: &[] });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("video"),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor { label: Some("video"), size: 32, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        VideoLayer { pipeline, layout, sampler, uniforms, planes: None }
    }

    /// Show a picture: 8-bit 4:2:0 planes, tightly packed (`u` / `v` half size, rounded up).
    pub fn set_frame(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, size: (u32, u32), y: &[u8], u: &[u8], v: &[u8]) {
        let (w, h) = size;
        let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
        if self.planes.as_ref().is_none_or(|p| p.2 != size) {
            let plane = |pw, ph| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("video plane"),
                    size: wgpu::Extent3d { width: pw, height: ph, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::R8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                })
            };
            let t = [plane(w, h), plane(cw, ch), plane(cw, ch)];
            let views: Vec<_> = t.iter().map(|t| t.create_view(&Default::default())).collect();
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("video"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.uniforms.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&views[0]) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&views[1]) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&views[2]) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ],
            });
            self.planes = Some((t, bind, size));
        }
        let Some((t, _, _)) = &self.planes else { return };
        for (tex, data, (pw, ph)) in [(&t[0], y, (w, h)), (&t[1], u, (cw, ch)), (&t[2], v, (cw, ch))] {
            queue.write_texture(
                tex.as_image_copy(),
                data,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(pw), rows_per_image: Some(ph) },
                wgpu::Extent3d { width: pw, height: ph, depth_or_array_layers: 1 },
            );
        }
    }

    /// The picture's rectangle in a `target`-sized frame (px: left, top, width, height): as large
    /// as fits with its aspect kept, centred.
    pub fn fit(picture: (u32, u32), target: (u32, u32)) -> (f32, f32, f32, f32) {
        let k = (target.0 as f32 / picture.0 as f32).min(target.1 as f32 / picture.1 as f32);
        let (w, h) = (picture.0 as f32 * k, picture.1 as f32 * k);
        ((target.0 as f32 - w) / 2.0, (target.1 as f32 - h) / 2.0, w, h)
    }

    /// Clear `target` to black and draw the picture (nothing but black before the first one).
    pub fn draw(&self, queue: &wgpu::Queue, enc: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, size: (u32, u32), srgb: bool) {
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("video"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            ..Default::default()
        });
        let Some((_, bind, pic)) = &self.planes else { return };
        let (x, y, w, h) = Self::fit(*pic, size);
        let (sw, sh) = (size.0 as f32, size.1 as f32);
        let rect = [x / sw * 2.0 - 1.0, 1.0 - y / sh * 2.0, (x + w) / sw * 2.0 - 1.0, 1.0 - (y + h) / sh * 2.0, srgb as u8 as f32, 0.0, 0.0, 0.0];
        let bytes: Vec<u8> = rect.iter().flat_map(|f| f.to_le_bytes()).collect();
        queue.write_buffer(&self.uniforms, 0, &bytes);
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bind, &[]);
        pass.draw(0..4, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_keeps_the_aspect() {
        assert_eq!(VideoLayer::fit((640, 480), (1920, 1080)), (240.0, 0.0, 1440.0, 1080.0));
        assert_eq!(VideoLayer::fit((640, 480), (800, 600)), (0.0, 0.0, 800.0, 600.0));
        assert_eq!(VideoLayer::fit((640, 480), (1024, 1024)), (0.0, 128.0, 1024.0, 768.0));
    }
}
