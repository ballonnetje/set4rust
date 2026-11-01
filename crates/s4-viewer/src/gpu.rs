//! GPU renderer: device/surface setup and the frame. A frame is the terrain pass followed by the
//! sprite pass (`IGfxEngine::RenderFrame`, 0x2f5b8f0: the landscape is drawn completely before
//! `RenderObjectLayer`).

use std::sync::Arc;

use winit::window::Window;

use crate::clouds::{Clouds, Land};
use crate::effects::{Effects, PostFx};
use crate::sprites::{SpriteRenderer, SpriteScene};
use crate::terrain::TerrainMesh;
use crate::terrain_gpu::TerrainGpu;
use s4_formats::gfx::landscape::TexturePage;

/// Camera: world offset (top-left of view) and zoom factor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub x: f32,
    pub y: f32,
    pub zoom: f32,
}

/// The headless renderers' instance, kept for [`Gpu::live_resources`].
static HEADLESS_INSTANCE: std::sync::OnceLock<wgpu::Instance> = std::sync::OnceLock::new();

pub struct Gpu {
    /// None in headless (capture) mode.
    surface: Option<wgpu::Surface<'static>>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    /// Surface config; in headless mode only width/height/format are meaningful.
    config: wgpu::SurfaceConfiguration,
    pub terrain: Option<TerrainGpu>,
    pub sprites: Option<SpriteRenderer>,
    /// Optional effects (all off = the original look).
    pub effects: Effects,
    /// Display scale factor used when `effects.hidpi` is on.
    pub scale_factor: f32,
    post: Option<PostFx>,
    clouds: Option<Clouds>,
    /// The map's land, kept for when the clouds are first drawn.
    land: Option<Land>,
    /// The clouds' clock ([`crate::clouds::seconds`]), set by the client before each frame.
    pub cloud_time: f64,
    /// The GUI panel overlay (gui.rs), drawn last, unaffected by the effects.
    pub gui: Option<crate::gui::GuiOverlay>,
    /// The GUI's state and composer (what `gui` shows).
    pub ui: Option<crate::gui::Gui>,
    /// The panel needs composing again ([`crate::client::recompose`]): done once, by the next
    /// frame or GUI mouse event ([`Gpu::flush_gui`]), however many changes asked for it.
    pub gui_dirty: bool,
    /// Drawn over everything, captures included (the game's monitor); see [`Overlay`].
    pub overlay: Option<Box<dyn Overlay>>,
    /// A video is playing ([`crate::video`]): it is the whole frame, nothing else is drawn.
    pub video: Option<crate::video::VideoLayer>,
    pub show_gui: bool,
    /// Outline the node under the cursor (the debug viewer's picking aid; the game has none:
    /// the original draws nothing at the cursor's node outside the build and work-area modes).
    pub show_hover: bool,
    /// Camera window: its view and its rectangle in panel px (`EnableCameraWindow`).
    pub camera_window: Option<(View, (u32, u32, u32, u32))>,
    /// Screen px per panel px.
    pub gui_scale: f32,
    /// Where the panel's (0, 0) is on the screen (screen px): the start screens are centred
    /// (`Frontend::fit`), the game's panel is at the corner.
    pub gui_origin: (f32, f32),
    /// The zoom's events since the last take, `(id, lParam)`: 0x25d (0x5199a0, the slider value
    /// after a zoom step or at the start) and 0x12 (the slider moved, `Tutorial.SetZoom`), for
    /// the event handlers that listen to them (`CTutorial::OnEvent` 0x4fa780).
    pub zoom_events: Vec<(u32, i32)>,
    /// Headless: [`Gpu::draw`] renders into this texture (benchmarks), else nothing.
    offscreen: Option<wgpu::Texture>,
}

impl Gpu {
    pub fn new(window: Arc<Window>) -> anyhow::Result<Self> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
        let surface = instance.create_surface(window.clone())?;
        let (adapter, device, queue) = Self::device(&instance, Some(&surface))?;
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or_else(|| anyhow::anyhow!("surface unsupported by adapter"))?;
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&device, &config);
        Self::with_device(Some(surface), device, queue, config)
    }

    /// Offscreen renderer for `--capture`. Renders into `Rgba8UnormSrgb`, the usual swapchain
    /// format, so captures show what the window shows.
    ///
    /// All headless renderers of a process share one device and queue, created on first use.
    /// Creating a Vulkan device per renderer deadlocked parallel test runs on the NVIDIA driver:
    /// `vkCreateDevice` holds a driver lock while it loads a library (the loader lock), and a
    /// thread exiting at that moment holds the loader lock while the driver's
    /// `DllMain(DLL_THREAD_DETACH)` waits for the driver lock (stacks of a hung `s4_game` test
    /// exe, 2026-10-22). One creation per process leaves one such window instead of one per test.
    pub fn headless(width: u32, height: u32) -> anyhow::Result<Self> {
        static SHARED: std::sync::Mutex<Option<(wgpu::Device, wgpu::Queue)>> = std::sync::Mutex::new(None);
        let (device, queue) = {
            let mut shared = SHARED.lock().unwrap_or_else(|e| e.into_inner());
            if shared.is_none() {
                let instance = HEADLESS_INSTANCE.get_or_init(|| wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default()));
                let (_, device, queue) = Self::device(instance, None)?;
                *shared = Some((device, queue));
            }
            shared.clone().expect("created above")
        };
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        };
        Self::with_device(None, device, queue, config)
    }

    /// The live wgpu objects of the headless device (leak checks, `s4-game --soak`): buffers,
    /// textures, texture views, bind groups.
    pub fn live_resources() -> Option<[usize; 4]> {
        let hub = HEADLESS_INSTANCE.get()?.generate_report()?.hub;
        Some([hub.buffers.num_allocated, hub.textures.num_allocated, hub.texture_views.num_allocated, hub.bind_groups.num_allocated])
    }

    fn device(
        instance: &wgpu::Instance,
        surface: Option<&wgpu::Surface<'static>>,
    ) -> anyhow::Result<(wgpu::Adapter, wgpu::Device, wgpu::Queue)> {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: surface,
            force_fallback_adapter: false,
        }))?;
        tracing::info!("adapter: {:?}", adapter.get_info());
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("s4rs"),
            ..Default::default()
        }))?;
        Ok((adapter, device, queue))
    }

    fn with_device(
        surface: Option<wgpu::Surface<'static>>,
        device: wgpu::Device,
        queue: wgpu::Queue,
        config: wgpu::SurfaceConfiguration,
    ) -> anyhow::Result<Self> {
        Ok(Gpu { surface, device, queue, config, terrain: None, sprites: None, effects: Effects::default(), scale_factor: 1.0, post: None, clouds: None, land: None, cloud_time: 0.0, gui: None, show_gui: true, show_hover: false, gui_scale: 1.0, gui_origin: (0.0, 0.0), camera_window: None, ui: None, gui_dirty: false, overlay: None, video: None, zoom_events: Vec::new(), offscreen: None })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            if let Some(s) = &self.surface {
                s.configure(&self.device, &self.config);
            }
        }
    }

    pub fn format(&self) -> wgpu::TextureFormat {
        self.config.format
    }

    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// Create the terrain renderer from the 44 hardware landscape pages.
    pub fn set_terrain_pages(&mut self, pages: &[TexturePage]) {
        self.terrain = Some(TerrainGpu::new(&self.device, &self.queue, self.config.format, pages));
    }

    /// Upload changed terrain bands.
    pub fn sync_terrain(&mut self, mesh: &mut TerrainMesh) {
        let _t = crate::perf::span("terrain: gpu sync");
        mesh.set_ao(self.effects.ao);
        if let Some(t) = &mut self.terrain {
            t.sync(&self.device, &self.queue, mesh);
        }
    }

    /// A new map's land for the cloud shadows.
    pub fn set_land(&mut self, mesh: &TerrainMesh) {
        let land = Land::of(&mesh.nodes, mesh.width, mesh.height);
        if let Some(c) = &mut self.clouds {
            c.set_land(&self.device, &self.queue, &land);
        }
        self.land = Some(land);
    }

    /// Create the sprite renderer (atlas + palettes) for a map's sprites.
    pub fn set_sprites(&mut self, scene: &mut SpriteScene) {
        self.sprites = Some(SpriteRenderer::new(&self.device, &self.queue, self.config.format, scene));
    }

    /// Headless: let [`Gpu::draw`] render offscreen (`true`) or skip the frame (`false`).
    pub fn set_offscreen(&mut self, on: bool) {
        self.offscreen = on.then(|| self.target_texture("offscreen"));
    }

    /// Wait until the GPU has run everything submitted, the buffer and texture writes not yet
    /// submitted included (headless, nothing else submits them and their staging memory stays).
    pub fn wait(&self) -> anyhow::Result<()> {
        self.queue.submit(std::iter::empty());
        self.device.poll(wgpu::PollType::Wait)?;
        Ok(())
    }

    fn target_texture(&self, label: &str) -> wgpu::Texture {
        let (w, h) = self.size();
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.config.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
    }

    /// Present one frame to the window.
    pub fn draw(&mut self, view: View) {
        let Some(surface) = &self.surface else {
            if let Some(t) = &self.offscreen {
                let target = t.create_view(&Default::default());
                self.render(&target, view);
            }
            return;
        };
        let acquire = crate::perf::span("gpu: acquire (vsync)");
        let frame = match surface.get_current_texture() {
            Ok(f) => f,
            Err(wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Lost) => {
                surface.configure(&self.device, &self.config);
                return;
            }
            Err(e) => {
                tracing::warn!("surface: {e}");
                return;
            }
        };
        drop(acquire);
        let target = frame.texture.create_view(&Default::default());
        self.render(&target, view);
        let _t = crate::perf::span("gpu: present");
        frame.present();
    }

    /// Render one frame offscreen and return it as tightly packed RGBA8 (the bytes as stored in
    /// the render target).
    pub fn capture(&mut self, view: View) -> anyhow::Result<Vec<u8>> {
        let (w, h) = self.size();
        let tex = self.target_texture("capture");
        self.render(&tex.create_view(&Default::default()), view);
        let row = (w * 4).div_ceil(256) * 256;
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("capture readback"),
            size: (row * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            tex.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        self.queue.submit([enc.finish()]);
        let slice = buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::PollType::Wait)?;
        let data = slice.get_mapped_range();
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h as usize {
            out.extend_from_slice(&data[y * row as usize..y * row as usize + w as usize * 4]);
        }
        if matches!(self.config.format, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb) {
            out.as_chunks_mut::<4>().0.iter_mut().for_each(|p| p.swap(0, 2));
        }
        Ok(out)
    }

    /// Compose the GUI panel and upload it if a change asked for it since the last time.
    pub fn flush_gui(&mut self) {
        if !std::mem::take(&mut self.gui_dirty) {
            return;
        }
        let _t = crate::perf::span("gui: compose");
        let Some(ui) = self.ui.as_mut() else { return };
        match ui.render() {
            Ok(panel) => {
                if let Some(g) = &mut self.gui {
                    g.set_panel(&self.device, &self.queue, panel);
                }
            }
            Err(e) => tracing::warn!("GUI: {e}"),
        }
    }

    fn render(&mut self, target: &wgpu::TextureView, view: View) {
        let (w, h) = self.size();
        if let Some(v) = &self.video {
            let mut enc = self.device.create_command_encoder(&Default::default());
            v.draw(&self.queue, &mut enc, target, (w, h), self.config.format.is_srgb());
            self.queue.submit([enc.finish()]);
            return;
        }
        self.flush_gui();
        let fx = self.effects;
        let view = if fx.hidpi { View { zoom: view.zoom * self.scale_factor, ..view } } else { view };
        if let Some(t) = &mut self.terrain {
            t.ao_enabled = fx.ao;
        }
        if let Some(s) = &mut self.sprites {
            s.smooth_shadows = fx.smooth_shadows;
        }
        // The start screens have no world to shade.
        let clouds = if fx.clouds && self.terrain.is_some() {
            Some(&*self.clouds.get_or_insert_with(|| Clouds::new(&self.device, &self.queue, self.config.format, self.land.as_ref())))
        } else {
            None
        };
        // Sharpen: draw the frame offscreen, then filter it into the target.
        let scene_view = if fx.sharpen {
            let post = self.post.get_or_insert_with(|| PostFx::new(&self.device, self.config.format));
            Some(post.scene_view(&self.device, (w, h)))
        } else {
            None
        };
        let target_final = target;
        let target = scene_view.as_ref().unwrap_or(target);
        if let Some(t) = &self.terrain {
            t.write_uniforms(&self.queue, view, (w, h), self.config.format.is_srgb());
        }
        if let Some(s) = &mut self.sprites {
            s.ui_apart = clouds.is_some();
            let _t = crate::perf::span("gpu: sprite prepare");
            s.prepare(&self.device, &self.queue, view, (w, h));
        }
        if let Some(c) = clouds {
            c.prepare(&self.queue, view, (w, h), self.cloud_time);
        }

        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            if let Some(t) = &self.terrain {
                t.draw(&mut pass, view, (w, h));
            }
            if let Some(s) = &self.sprites {
                s.draw(&mut pass);
            }
            if let Some(c) = clouds {
                c.draw(&mut pass);
            }
            if let Some(s) = &self.sprites {
                s.draw_ui(&mut pass);
            }
        }
        if scene_view.is_some()
            && let Some(post) = &self.post
        {
            post.apply(&mut enc, target_final);
        }
        // The camera window (GUI slot 2) is a second view of the world in its own viewport; the
        // terrain and sprite uniforms are rewritten for it, so the main frame is submitted first.
        if self.show_gui
            && let Some((cview, rect)) = self.camera_window
        {
            self.queue.submit([enc.finish()]);
            enc = self.device.create_command_encoder(&Default::default());
            let k = self.gui_scale;
            let (rx, ry) = ((rect.0 as f32 * k) as u32, (rect.1 as f32 * k) as u32);
            let (rw, rh) = ((rect.2 as f32 * k) as u32, (rect.3 as f32 * k) as u32);
            let (rw, rh) = (rw.min(w.saturating_sub(rx)), rh.min(h.saturating_sub(ry)));
            if rw > 0 && rh > 0 {
                let cview = View { zoom: cview.zoom * k, ..cview };
                if let Some(t) = &self.terrain {
                    t.write_uniforms(&self.queue, cview, (rw, rh), self.config.format.is_srgb());
                }
                if let Some(s) = &mut self.sprites {
                    s.prepare(&self.device, &self.queue, cview, (rw, rh));
                }
                if let Some(c) = clouds {
                    c.prepare(&self.queue, cview, (rw, rh), self.cloud_time);
                }
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("camera window"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target_final,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                    })],
                    ..Default::default()
                });
                pass.set_viewport(rx as f32, ry as f32, rw as f32, rh as f32, 0.0, 1.0);
                pass.set_scissor_rect(rx, ry, rw, rh);
                if let Some(t) = &self.terrain {
                    t.draw(&mut pass, cview, (rw, rh));
                }
                if let Some(s) = &self.sprites {
                    s.draw(&mut pass);
                }
                if let Some(c) = clouds {
                    c.draw(&mut pass);
                }
                if let Some(s) = &self.sprites {
                    s.draw_ui(&mut pass);
                }
            }
        }
        if self.show_gui
            && let Some(g) = &self.gui
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("gui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_final,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                ..Default::default()
            });
            g.draw(&self.queue, &mut pass, (w, h), self.gui_scale, self.gui_origin);
        }
        if let Some(o) = &mut self.overlay {
            o.draw(&self.device, &self.queue, &mut enc, target_final, (w, h), view);
        }
        self.queue.submit([enc.finish()]);
    }
}

/// Something the client draws over the finished frame (the GUI included), not part of the game's
/// look: s4-game's monitor.
pub trait Overlay {
    /// Record the drawing into `encoder` onto `target` (`size` px); `view` is the frame's camera as
    /// drawn (zoom times the scale factor under HiDPI): a world px is at `(p − view.xy) · zoom`.
    fn draw(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, size: (u32, u32), view: View);
    /// The overlay itself, for its owner.
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}
