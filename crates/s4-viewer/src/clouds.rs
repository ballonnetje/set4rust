//! Cloud shadows: soft dark and light patches drifting over the landscape and everything on it.
//! Optional, off by default (effects.rs); nothing like it exists in the original renderer.
//!
//! One full-screen triangle at the end of the world's pass multiplies what is already there, so
//! terrain and sprites are shaded alike and neither shader knows about it. The fog's black stays
//! black, the GUI comes in a later pass and so do the sprites that are interface, not world (build
//! marks, selection and health icons, the cursor building: [`crate::sprites::SpriteRenderer::draw_ui`]).
//!
//! The clouds are one field of fractal noise, baked once into a tiling texture (no asset), that
//! travels with the wind as a whole: every detail stays where it is on its cloud. Bodies change
//! only as real ones do, slowly: a second, heavily blurred sample of the tile drifts a little
//! against the first and raises or lowers how much of it counts as cloud, so over minutes clouds
//! grow, thin out and part. The field is cut by a wide ramp, so a body is a dark core in a torn,
//! uneven halo. The blurred sample also lifts the wide open areas a little: sunlit stretches.
//! Water takes [`SEA`] of all this: a texture of the map ([`Land`]) says where, filtered, so the
//! change follows the coast softly.
//!
//! The field lies on the ground, not on the screen: the same texture holds the terrain's height,
//! and a pixel looks up the clouds where its ground is, so an edge bends over a hill. A pixel has
//! no height of its own, so this is the height of the land at the place it would show if it were
//! at the water's level: close on the gentle slopes of a map, and a tall sprite takes the shade of
//! the ground around it.
//!
//! Its clock ([`crate::gpu::Gpu::cloud_time`]) is real time while the game runs: a pause stops the
//! clouds and they drift alike at every game speed. Each client has its own; nothing goes back to
//! the simulation. Captures use [`seconds`] of the tick, so they repeat.

use rayon::prelude::*;

use crate::gpu::View;
use crate::terrain::{ROW_H, TRI_W};
use crate::waves::TICK_MS;

/// Texels per side of the noise tile.
const TILE: u32 = 1024;
/// Mip levels of the tile (the blurred sample reads [`BLUR_LEVEL`] in the shader).
const MIPS: u32 = 6;
/// Lattice cells per tile side of the coarsest octave, and the octaves on top of it.
const CELLS: usize = 6;
const OCTAVES: u32 = 7;
/// Share of each octave in the next finer one's amplitude: higher tears the edges more.
const ROUGHNESS: f32 = 0.56;
/// World px per tile of the two samples (no common divisor, so the sum does not repeat on a map).
const PERIODS: [f64; 2] = [5120.0, 7919.0];
/// Drift of the two samples, ground px per second. The small difference is all the clouds' change.
const WINDS: [(f64, f64); 2] = [(30.0, 11.0), (33.0, 8.5)];
/// The clouds' travel, ground px per second.
pub const WIND: (f64, f64) = WINDS[0];
/// World y px to ground: the view squeezes the rows (12 px per row, 24 px × √3/2 on the ground),
/// so patches round on the ground come out flattened, as the land does.
pub const GROUND_Y: f64 = 1.732;
/// Brightness on the raw 8-bit values under a cloud and in the gaps between clouds.
const SHADE: f32 = 0.72;
const SUN: f32 = 1.10;
/// How much of the shading the water gets.
const SEA: f32 = 0.8;
/// Screen px per height unit (`CalcFinalHeightOffset`).
const HEIGHT_PX: f32 = 1.2375;

/// The clouds' clock of a capture at `tick`, in seconds.
pub fn seconds(tick: f64) -> f64 {
    tick * TICK_MS as f64 / 1000.0
}

fn hash(seed: u32, i: u32) -> f32 {
    let mut h = (i ^ seed.wrapping_mul(0x9e37_79b9)).wrapping_mul(0x2545_f491).wrapping_add(seed);
    h ^= h >> 15;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    (h ^ (h >> 16)) as f32 / u32::MAX as f32
}

/// One octave of tiling gradient noise.
struct Octave {
    cells: usize,
    gradients: Vec<[f32; 2]>,
}

impl Octave {
    fn new(seed: u32, cells: usize) -> Self {
        let gradient = |i| {
            let a = hash(seed, i as u32) * std::f32::consts::TAU;
            [a.cos(), a.sin()]
        };
        Octave { cells, gradients: (0..cells * cells).map(gradient).collect() }
    }

    /// At `(x, y)`, in tiles.
    fn at(&self, x: f32, y: f32) -> f32 {
        let c = self.cells;
        let (fx, fy) = (x.rem_euclid(1.0) * c as f32, y.rem_euclid(1.0) * c as f32);
        let (ix, iy) = (fx as usize, fy as usize);
        let (tx, ty) = (fx - ix as f32, fy - iy as f32);
        let corner = |dx: usize, dy: usize| {
            let g = self.gradients[((iy + dy) % c) * c + (ix + dx) % c];
            g[0] * (tx - dx as f32) + g[1] * (ty - dy as f32)
        };
        let fade = |t: f32| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
        let (u, v) = (fade(tx), fade(ty));
        let top = corner(0, 0) + (corner(1, 0) - corner(0, 0)) * u;
        let bottom = corner(0, 1) + (corner(1, 1) - corner(0, 1)) * u;
        top + (bottom - top) * v
    }
}

/// [`OCTAVES`] from [`CELLS`] up, each twice as fine and [`ROUGHNESS`] as strong as the one before.
fn octaves(seed: u32) -> Vec<Octave> {
    (0..OCTAVES).map(|o| Octave::new(seed + o, CELLS << o)).collect()
}

fn fractal(layers: &[Octave], x: f32, y: f32) -> f32 {
    let mut amp = 1.0;
    layers
        .iter()
        .map(|o| {
            let v = o.at(x, y) * amp;
            amp *= ROUGHNESS;
            v
        })
        .sum()
}

/// `size`² of fractal noise that tiles in both directions. By rank: a value is the share of
/// the tile below it, so a threshold in the shader is directly how much of the sky is cloud.
pub fn noise(size: usize) -> Vec<u8> {
    let shape = octaves(1);
    let v: Vec<f32> = (0..size * size)
        .into_par_iter()
        .map(|i| {
            let (x, y) = ((i % size) as f32 / size as f32, (i / size) as f32 / size as f32);
            fractal(&shape, x, y)
        })
        .collect();
    let mut sorted = v.clone();
    sorted.par_sort_unstable_by(f32::total_cmp);
    let last = v.len() - 1;
    v.par_iter().map(|x| (sorted.partition_point(|s| s < x) * 255 / last) as u8).collect()
}

/// The next mip level of a `size`² image.
fn halved(v: &[u8], size: usize) -> Vec<u8> {
    let at = |x: usize, y: usize| v[y * size + x] as u32;
    (0..size / 2)
        .flat_map(|y| (0..size / 2).map(move |x| (x, y)))
        .map(|(x, y)| ((at(x * 2, y * 2) + at(x * 2 + 1, y * 2) + at(x * 2, y * 2 + 1) + at(x * 2 + 1, y * 2 + 1) + 2) / 4) as u8)
        .collect()
}

/// A layer's UV at the viewport's top-left (wrapped into 0..1, so a long game keeps its
/// precision) and across the viewport.
fn layer_uv(view: View, (w, h): (u32, u32), time: f64, period: f64, wind: (f64, f64)) -> [f32; 4] {
    let zoom = view.zoom as f64;
    let at = |p: f64, drift: f64| ((p - drift * time) / period).rem_euclid(1.0) as f32;
    [
        at(view.x as f64, wind.0),
        at(view.y as f64 * GROUND_Y, wind.1),
        (w as f64 / zoom / period) as f32,
        (h as f64 / zoom * GROUND_Y / period) as f32,
    ]
}

/// The ground, node by node. Read once per map: the water does not change in a game, and the
/// levelling of a building site is too small to show in the clouds.
#[derive(Clone, Debug, PartialEq)]
pub struct Land {
    width: usize,
    height: usize,
    /// Two bytes per node: 255 land, 0 water (`IsWater`: ground type <= 8), and the height.
    texels: Vec<u8>,
    /// How far the water's surface is drawn above its row, px: a pixel has no height, so the mask
    /// is laid at this one.
    water_px: f32,
}

impl Land {
    /// From the terrain's packed nodes (`height | ground << 8 | ...`).
    pub fn of(nodes: &[u32], width: usize, height: usize) -> Self {
        let (mut sum, mut water) = (0u64, 0u64);
        let texels = nodes
            .iter()
            .flat_map(|&n| {
                let is_water = (n >> 8) & 255 <= 8;
                sum += if is_water { (n & 255) as u64 } else { 0 };
                water += is_water as u64;
                [if is_water { 0 } else { 255 }, n as u8]
            })
            .collect();
        Land { width, height, texels, water_px: sum as f32 / water.max(1) as f32 * HEIGHT_PX }
    }

    /// No map: flat land.
    fn none() -> Self {
        Land { width: 1, height: 1, texels: vec![255, 0], water_px: 0.0 }
    }

    /// The texture's UV at the viewport's top-left and per viewport in x and y (u, u, v).
    fn uv(&self, view: View, (w, h): (u32, u32)) -> ([f32; 2], [f32; 3]) {
        let (mw, mh) = (self.width as f32, self.height as f32);
        let (across, down) = (w as f32 / view.zoom, h as f32 / view.zoom);
        // Node of a world point: row (y + water) / 12, column (x + 12 row) / 24.
        let row = (view.y + self.water_px) / ROW_H as f32;
        let column = (view.x + ROW_H as f32 * row) / TRI_W as f32;
        (
            [(column + 0.5) / mw, (row + 0.5) / mh],
            [across / TRI_W as f32 / mw, down / TRI_W as f32 / mw, down / ROW_H as f32 / mh],
        )
    }
}

/// The shader's uniforms for a viewport of `size` px showing `view` at `time`.
fn uniforms(view: View, size: (u32, u32), time: f64, srgb: bool, land: &Land) -> [f32; 20] {
    let (a, b) = (layer_uv(view, size, time, PERIODS[0], WINDS[0]), layer_uv(view, size, time, PERIODS[1], WINDS[1]));
    // An sRGB target blends in linear light: 2.2 keeps the factors on the raw values.
    let encode = |f: f32| if srgb { f.powf(2.2) } else { f };
    let (at, per) = land.uv(view, size);
    // How far up a layer's v the full height is: the ground lies below the pixel by its height.
    let lift = |period: f64| (255.0 * HEIGHT_PX as f64 * GROUND_Y / period) as f32;
    [
        a[0], a[1], a[2], a[3],
        b[0], b[1], b[2], b[3],
        encode(SHADE), encode(SUN), per[2], SEA,
        at[0], at[1], per[0], per[1],
        lift(PERIODS[0]), lift(PERIODS[1]), 0.0, 0.0,
    ]
}

/// The pipeline and its noise tile, made when the effect is first drawn.
pub struct Clouds {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    bind: wgpu::BindGroup,
    uniforms: wgpu::Buffer,
    noise: wgpu::TextureView,
    sampler: wgpu::Sampler,
    land_sampler: wgpu::Sampler,
    land: Land,
    srgb_target: bool,
}

impl Clouds {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat, land: Option<&Land>) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("clouds"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("clouds"),
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
                        view_dimension: wgpu::TextureViewDimension::D2,
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
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("clouds"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        // 2 × source × target: a source of 0.5 leaves the pixel alone, so the factor can pass 1.
        let modulate2x = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Dst,
            dst_factor: wgpu::BlendFactor::Src,
            operation: wgpu::BlendOperation::Add,
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("clouds"),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState { color: modulate2x, alpha: wgpu::BlendComponent::REPLACE }),
                    write_mask: wgpu::ColorWrites::COLOR,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("cloud noise"),
            size: wgpu::Extent3d { width: TILE, height: TILE, depth_or_array_layers: 1 },
            mip_level_count: MIPS,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let mut level = noise(TILE as usize);
        for mip in 0..MIPS {
            let side = TILE >> mip;
            queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: mip, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                &level,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(side), rows_per_image: Some(side) },
                wgpu::Extent3d { width: side, height: side, depth_or_array_layers: 1 },
            );
            level = halved(&level, side as usize);
        }
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("cloud noise"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cloud uniforms"),
            size: 80,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let land_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("land"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let noise = texture.create_view(&Default::default());
        let land = land.cloned().unwrap_or_else(Land::none);
        let bind = Self::bind_group(device, queue, &layout, &uniforms, &noise, &sampler, &land_sampler, &land);
        Clouds { pipeline, layout, bind, uniforms, noise, sampler, land_sampler, land, srgb_target: format.is_srgb() }
    }

    #[allow(clippy::too_many_arguments)]
    fn bind_group(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        uniforms: &wgpu::Buffer,
        noise: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
        land_sampler: &wgpu::Sampler,
        land: &Land,
    ) -> wgpu::BindGroup {
        let size = wgpu::Extent3d { width: land.width as u32, height: land.height as u32, depth_or_array_layers: 1 };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("land"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rg8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            texture.as_image_copy(),
            &land.texels,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(land.width as u32 * 2), rows_per_image: Some(land.height as u32) },
            size,
        );
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("clouds"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(noise) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&texture.create_view(&Default::default())),
                },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(land_sampler) },
            ],
        })
    }

    /// A new map's land.
    pub fn set_land(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, land: &Land) {
        self.bind = Self::bind_group(device, queue, &self.layout, &self.uniforms, &self.noise, &self.sampler, &self.land_sampler, land);
        self.land = land.clone();
    }

    /// Set the view the next [`Clouds::draw`] shows, in a viewport of `size` px.
    pub fn prepare(&self, queue: &wgpu::Queue, view: View, size: (u32, u32), time: f64) {
        let u = uniforms(view, size, time, self.srgb_target, &self.land);
        let bytes: Vec<u8> = u.iter().flat_map(|f| f.to_le_bytes()).collect();
        queue.write_buffer(&self.uniforms, 0, &bytes);
    }

    /// Inside the world's pass, after the sprites.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind, &[]);
        pass.draw(0..3, 0..1);
    }
}

const SHADER: &str = r#"
struct U {
    a: vec4<f32>,      // the field's UV at the viewport's top-left (xy), across the viewport (zw)
    b: vec4<f32>,      // the same of the blurred sample
    factor: vec4<f32>, // brightness under a cloud (x), in the sun (y), ground v across the
                       // viewport (z), the water's share (w)
    ground: vec4<f32>, // ground UV at the viewport's top-left (xy), u across the viewport by x, y (zw)
    lift: vec4<f32>,   // v of a sample per unit of the ground texture's height (xy)
};
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var noise: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;
@group(0) @binding(3) var ground: texture_2d<f32>;
@group(0) @binding(4) var ground_samp: sampler;

struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) a: vec2<f32>,
    @location(1) b: vec2<f32>,
    @location(2) ground: vec2<f32>,
};

// From the viewport, not the pixel position: the camera window is a viewport inside the frame.
@vertex
fn vs(@builtin(vertex_index) i: u32) -> Out {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: Out;
    o.pos = vec4<f32>(p.x * 2.0 - 1.0, 1.0 - p.y * 2.0, 0.0, 1.0);
    o.a = u.a.xy + p * u.a.zw;
    o.b = u.b.xy + p * u.b.zw;
    o.ground = u.ground.xy + vec2<f32>(dot(p, u.ground.zw), p.y * u.factor.z);
    return o;
}

const BLUR_LEVEL = 4.0;

@fragment
fn fs(i: Out) -> @location(0) vec4<f32> {
    let g = textureSample(ground, ground_samp, i.ground);
    let field = textureSample(noise, samp, i.a + vec2<f32>(0.0, g.g * u.lift.x)).r;
    let weather = textureSampleLevel(noise, samp, i.b + vec2<f32>(0.0, g.g * u.lift.y), BLUR_LEVEL).r;
    // The field is uniform in 0..1 (by rank): the ramp's ends are the share of the sky in a cloud's
    // halo and in its core.
    let cover = smoothstep(0.52, 0.94, field + (weather - 0.5) * 0.8);
    let sun = mix(u.factor.y, 1.0, smoothstep(0.34, 0.58, weather));
    let shade = mix(sun, u.factor.x, cover);
    return vec4<f32>(vec3<f32>(mix(1.0, shade, mix(u.factor.w, 1.0, g.r)) * 0.5), 1.0);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: usize = 128;

    #[test]
    fn noise_tiles_and_fills_the_range() {
        let n = noise(SIZE);
        let at = |x: usize, y: usize| n[(y % SIZE) * SIZE + x % SIZE] as i32;
        // The wrap is no bigger a step than the worst one inside the tile.
        let inside = (0..SIZE).flat_map(|y| (0..SIZE - 1).map(move |x| (x, y))).map(|(x, y)| (at(x, y) - at(x + 1, y)).abs()).max().unwrap();
        let wrap_x = (0..SIZE).map(|y| (at(SIZE - 1, y) - at(0, y)).abs()).max().unwrap();
        let wrap_y = (0..SIZE).map(|x| (at(x, SIZE - 1) - at(x, 0)).abs()).max().unwrap();
        assert!(wrap_x <= inside && wrap_y <= inside, "{wrap_x} {wrap_y} > {inside}");
        assert_eq!((n.iter().min(), n.iter().max()), (Some(&0), Some(&255)));
    }

    #[test]
    fn pattern_stays_on_the_world_when_the_camera_moves() {
        let base = View { x: 5000.0, y: 3000.0, zoom: 1.0 };
        let (a, b) = (uniforms(base, (1920, 1080), 10.0, false, &Land::none()), uniforms(View { x: base.x + 100.0, ..base }, (1920, 1080), 10.0, false, &Land::none()));
        // The same world point, 100 px further left on the screen.
        let step = (b[0] - a[0]).rem_euclid(1.0);
        assert!((step - (100.0 / PERIODS[0]) as f32).abs() < 1e-5, "{step}");
    }

    #[test]
    fn zoom_changes_only_how_much_of_the_pattern_shows() {
        let near = uniforms(View { x: 500.0, y: 300.0, zoom: 2.0 }, (1920, 1080), 1.0, false, &Land::none());
        let far = uniforms(View { x: 500.0, y: 300.0, zoom: 1.0 }, (1920, 1080), 1.0, false, &Land::none());
        assert_eq!(near[..2], far[..2]);
        assert!((far[2] / near[2] - 2.0).abs() < 1e-5 && (far[3] / near[3] - 2.0).abs() < 1e-5);
    }

    #[test]
    fn clock_keeps_its_precision_in_a_long_game() {
        // A week of game time: the wrapped offsets still move by the drift of one tick.
        let view = View { x: 0.0, y: 0.0, zoom: 1.0 };
        let t = seconds(8.5e6);
        let (a, b) = (uniforms(view, (640, 480), t, false, &Land::none()), uniforms(view, (640, 480), t + seconds(1.0), false, &Land::none()));
        let step = (a[0] - b[0]).rem_euclid(1.0);
        let want = (WINDS[0].0 * seconds(1.0) / PERIODS[0]) as f32;
        assert!((step - want).abs() < want * 0.05, "{step} {want}");
    }

    #[test]
    fn srgb_target_keeps_the_factors_on_the_raw_values() {
        let view = View { x: 0.0, y: 0.0, zoom: 1.0 };
        let (raw, srgb) = (uniforms(view, (64, 64), 0.0, false, &Land::none()), uniforms(view, (64, 64), 0.0, true, &Land::none()));
        assert_eq!((raw[8], raw[9]), (SHADE, SUN));
        assert!(srgb[8] < raw[8] && srgb[9] > raw[9]);
    }

    #[test]
    fn noise_is_uniform_by_rank() {
        let n = noise(SIZE);
        let share = |v: u8| n.iter().filter(|&&x| x < v).count() as f32 / n.len() as f32;
        assert!((share(64) - 0.25).abs() < 0.01 && (share(128) - 0.5).abs() < 0.01 && (share(192) - 0.75).abs() < 0.01);
    }

    #[test]
    fn only_a_small_drift_separates_the_two_samples() {
        let (dx, dy) = (WINDS[0].0 - WINDS[1].0, WINDS[0].1 - WINDS[1].1);
        assert!(dx.hypot(dy) < WINDS[0].0.hypot(WINDS[0].1) * 0.2);
    }

    #[test]
    fn land_mask_marks_water_by_ground_type() {
        let node = |height: u32, ground: u32| height | ground << 8;
        let land = Land::of(&[node(10, 0), node(30, 8), node(99, 9), node(99, 16)], 2, 2);
        assert_eq!(land.texels, [0, 10, 0, 30, 255, 99, 255, 99]);
        assert_eq!(land.water_px, 20.0 * HEIGHT_PX);
    }

    #[test]
    fn land_uv_finds_the_node_under_a_pixel() {
        let land = Land { width: 100, height: 80, texels: vec![255; 16000], water_px: 24.0 };
        // Node (40, 20): world (24·40 − 12·20, 12·20 − 24) at the water's height.
        let view = View { x: 720.0, y: 216.0, zoom: 1.0 };
        let (at, per) = land.uv(view, (240, 120));
        assert!((at[0] - 40.5 / 100.0).abs() < 1e-5 && (at[1] - 20.5 / 80.0).abs() < 1e-5, "{at:?}");
        // 240 px right: 10 columns on; 120 px down: 10 rows and 5 columns on (the rows lean).
        assert!((per[0] - 0.10).abs() < 1e-5 && (per[1] - 0.05).abs() < 1e-5 && (per[2] - 10.0 / 80.0).abs() < 1e-5, "{per:?}");
    }
}
