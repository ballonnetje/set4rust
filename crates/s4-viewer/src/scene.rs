//! What gets drawn: the terrain (from a [`TerrainSource`]) and the map's sprites.

use crate::fog::{self, FogDriver};
use crate::gpu::Gpu;
use crate::sprites::SpriteScene;
use crate::terrain::{FogView, Grid, ROW_H, TRI_W, TerrainMesh, TerrainSource, node_pos, object_fog, row_fade};
use s4_formats::gfx::landscape::{LandscapeTextures, Rgb16, TextureSet, TexturePage, texture_path};

pub struct Scene {
    pub source: Grid,
    pub mesh: TerrainMesh,
    pages: Vec<TexturePage>,
    /// None for the synthetic grid or with `--no-sprites`.
    pub sprites: Option<SpriteScene>,
    /// What the map/world holds (for the GUI panel's texts and race); None for the synthetic grid.
    pub population: Option<std::sync::Arc<crate::population::Population>>,
    /// Fog of war driven by the test pattern (`--fog`).
    fog: Option<FogState>,
    /// The minimap surface, kept current frame by frame ([`crate::minimap::Minimap`]).
    minimap: Option<crate::minimap::Minimap>,
    /// The minimap's object groups and colours (`MinimapViewFlags`, `MINIMAP_COLORS`).
    pub minimap_layer: crate::minimap::ObjectLayer,
    tick: u64,
    /// The settlers' draw-time animation (walk displacement, frames).
    pub animator: crate::animate::Animator,
    /// The grid's heights and grounds are `population`'s ([`Scene::set_population`] compares
    /// only the rows where the next population differs from it).
    terrain_synced: bool,
}

struct FogState {
    /// The test pattern's driver (`--fog`); None when the fog comes from outside
    /// ([`Scene::set_fog_layer`]).
    driver: Option<FogDriver>,
    centre: (usize, usize),
    /// Road nodes (for the road-under-fog rule) and whether each is currently hidden.
    roads: Vec<(usize, usize, bool)>,
    /// Fog bits of byte 3 per node (bits 0..2 old, 3..5 new level), fade step and fade border.
    gfx: Vec<u8>,
    fade: u32,
    border: i32,
    /// The fade step and border the sprites' fog values were last computed with (then only
    /// rows whose bits changed are computed again).
    sprite_fade: Option<(u32, i32)>,
}

/// Rows within this many of each other are re-meshed as one run (a run's AO reaches
/// `AO_RADIUS` rows beyond it, so closer runs would compute rows twice).
const RUN_GAP: usize = 2 * crate::effects::AO_RADIUS as usize + 2;

/// Road nodes of a grid, none hidden yet.
fn road_nodes(source: &Grid) -> Vec<(usize, usize, bool)> {
    (0..source.h)
        .flat_map(|y| (0..source.w).map(move |x| (x, y)))
        .filter(|&(x, y)| matches!(source.node(x, y).ground, 28 | 29))
        .map(|(x, y)| (x, y, false))
        .collect()
}

impl Scene {
    /// `raw_map`: draw the map chunks as they are instead of the simulation's world.
    pub fn load(map: Option<&str>, sprites: bool, raw_map: bool) -> anyhow::Result<Self> {
        // Classic set 2 only: the History Edition "new textures" (41.*) are deliberately not used.
        let t = crate::perf::span("load: textures");
        let path = texture_path(&s4_formats::game_dir(), TextureSet::Classic, true, Rgb16::Rgb565);
        let tex = LandscapeTextures::open(&path, Rgb16::Rgb565)?;
        let pages = tex.hardware_pages().ok_or_else(|| anyhow::anyhow!("{}: no 16-bit pages", path.display()))?.to_vec();
        // The GPU's mip levels of the pages, here (often off the main thread) rather than in
        // `upload`.
        crate::terrain_mips::cached_padded_mips(&pages);
        drop(t);
        let mut population = None;
        let (source, sprites) = match map {
            Some(m) => {
                let file = s4_formats::map::MapFile::open(resolve_map(m))?;
                let sprites = if sprites {
                    let t = std::time::Instant::now();
                    let span = crate::perf::span("load: population");
                    let pop = if raw_map {
                        crate::population::from_map(&file)?
                    } else {
                        crate::population::from_world(&crate::population::world_from_map(&file)?)
                    };
                    drop(span);
                    let span = crate::perf::span("load: sprite scene");
                    let s = SpriteScene::from_population(&pop)?;
                    drop(span);
                    population = Some(pop);
                    let st = &s.stats;
                    tracing::info!(
                        "sprites: {} objects, {} buildings, {} settlers, {} vehicles, {} piles, {} waves, {} border stones; {} unique images in {} atlas layers ({:.2}s); {} skipped",
                        st.objects,
                        st.buildings,
                        st.settlers,
                        st.vehicles,
                        st.piles,
                        st.waves,
                        st.border_stones,
                        st.unique_sprites,
                        st.atlas_layers,
                        t.elapsed().as_secs_f32(),
                        st.skipped.len()
                    );
                    tracing::debug!("owners (count, first position): {:?}", st.owners);
                    for k in st.skipped.iter().take(20) {
                        tracing::debug!("skipped: {k}");
                    }
                    Some(s)
                } else {
                    None
                };
                (Grid::from_file(&file)?, sprites)
            }
            None => (Grid::synthetic(), None),
        };
        let span = crate::perf::span("load: terrain mesh");
        let mesh = TerrainMesh::build(&FogView { grid: &source, fade: 0, border: -50000 });
        drop(span);
        tracing::info!(
            "terrain: {}×{} nodes, {} triangles, {} bands, {:.1} MB GPU data",
            mesh.width,
            mesh.height,
            mesh.triangle_count(),
            mesh.bands.len(),
            mesh.bytes() as f64 / 1e6
        );
        Ok(Scene { source, mesh, pages, sprites, population: population.map(std::sync::Arc::new), fog: None, minimap: None, minimap_layer: crate::minimap::ObjectLayer::load(), tick: 0, animator: Default::default(), terrain_synced: false })
    }

    pub fn upload(&mut self, gpu: &mut Gpu) {
        let t = crate::perf::span("upload: terrain pages");
        gpu.set_terrain_pages(&self.pages);
        drop(t);
        let t = crate::perf::span("upload: terrain mesh");
        gpu.sync_terrain(&mut self.mesh);
        gpu.set_land(&self.mesh);
        drop(t);
        if let Some(s) = &mut self.sprites {
            let _t = crate::perf::span("upload: sprites");
            gpu.set_sprites(s);
        }
    }

    /// World position (zoom-1 px) of a node, at its height.
    pub fn node_world(&self, x: usize, y: usize) -> [f32; 2] {
        let (w, h) = self.source.size();
        let (x, y) = (x.min(w - 1), y.min(h - 1));
        node_pos(x, y, self.source.node(x, y).height)
    }

    /// Map centre in world coordinates (flat, ignoring height).
    pub fn centre(&self) -> [f32; 2] {
        let (w, h) = self.source.size();
        let (x, y) = (w as f32 / 2.0, h as f32 / 2.0);
        [TRI_W as f32 * x - TRI_W as f32 / 2.0 * y, ROW_H as f32 * y]
    }

    /// The minimap after `frames` more rendered frames (the first call draws it whole, as the
    /// engine's start does), with the view rectangle: `view` = the view's top-left in nodes,
    /// `rect` = its size on the minimap ([`crate::client::minimap_view`]); `local` = the local
    /// player (the object groups' own / ally / foe colours).
    pub fn minimap(&mut self, view: (i32, i32), rect: (i32, i32), frames: u32, local: u8) -> Option<Vec<u8>> {
        let sprites = self.sprites.as_ref()?;
        let size = self.source.w;
        let src = &self.source;
        let node = |n: usize| {
            let t = src.nodes[n];
            (t.ground, t.gradient, t.gfx_bits)
        };
        let border = |n: usize| sprites.border_bit(n % size, n / size);
        let layer = self.minimap_layer;
        let pop = self.population.as_ref();
        let owners = sprites.owners();
        let object = |n: usize| {
            let pop = pop?;
            let obj = pop.minimap_obj.get(n).copied().unwrap_or(0);
            layer.color(obj, owners.get(n).copied().unwrap_or(0), local, &pop.teams)
        };
        let nodes = crate::minimap::Nodes { size, node: &node, owner: owners, border: &border, object: &object };
        match &mut self.minimap {
            Some(m) => {
                for _ in 0..frames {
                    m.frame(&nodes);
                }
            }
            None => {
                let mut m = crate::minimap::Minimap::new(size);
                m.full_draw(&nodes);
                self.minimap = Some(m);
            }
        }
        let mut img = self.minimap.as_ref()?.image().to_vec();
        crate::minimap::mark_area(&mut img, size, view, rect);
        Some(img)
    }

    /// Turn on the fog-of-war test pattern centred on node `centre` (`CFogging::Start`).
    pub fn enable_fog(&mut self, gpu: &mut Gpu, centre: (usize, usize)) {
        let w = self.source.w;
        let driver = FogDriver::start(w, &fog::test_pattern(w, centre, self.tick));
        let roads = road_nodes(&self.source);
        let (gfx, fade, border) = (driver.gfx.clone(), driver.fade, driver.border);
        self.fog = Some(FogState { driver: Some(driver), centre, roads, gfx, fade, border, sprite_fade: None });
        self.apply_fog(gpu, vec![(0, self.source.h - 1)]);
    }

    /// Show fog computed elsewhere (the game client's `CFogging`): `gfx` = byte 3 fog bits per
    /// node (bits 0..2 old, 3..5 new), `fade` = `g_iFogFadeStep`, `border` = `g_iFadeYBorder`.
    /// Only the rows that changed are re-meshed.
    pub fn set_fog_layer(&mut self, gpu: &mut Gpu, gfx: &[u8], fade: u32, border: i32) {
        let w = self.source.w;
        let first = self.fog.is_none();
        if first {
            let roads = road_nodes(&self.source);
            self.fog = Some(FogState { driver: None, centre: (0, 0), roads, gfx: vec![0xFF; gfx.len()], fade, border, sprite_fade: None });
        }
        let Some(f) = &mut self.fog else { return };
        let mut rows = Vec::new();
        for (y, (old, new)) in f.gfx.chunks_mut(w).zip(gfx.chunks(w)).enumerate() {
            if old != new {
                old.copy_from_slice(new);
                rows.push(y);
            }
        }
        let changed = !rows.is_empty() || (f.fade, f.border) != (fade, border);
        (f.fade, f.border) = (fade, border);
        if changed || first {
            self.apply_fog(gpu, crate::terrain::row_runs(&rows, RUN_GAP));
        }
    }

    /// Show the simulation's next population: entities created or removed since the last one,
    /// territory (border stones, minimap), heights and ground types (the terrain rows that changed
    /// are re-meshed, e.g. a construction site's ground 28, path wear, levelled sites).
    ///
    /// Every changed node goes through `IGfxEngine::UpdateWorldPosition` (release 0x65a530): its
    /// shading and that of the nodes above and below are recomputed from the heights (0x66d7b0,
    /// [`crate::terrain::Grid::refresh_shading`]), and its row joins the redraw range
    /// (0x1169294 / 0x1169298). [inf] The byte-3 bit 0x40 that 0x66d7b0 also sets is not kept.
    pub fn set_population(&mut self, gpu: &mut Gpu, pop: impl Into<std::sync::Arc<crate::population::Population>>) -> anyhow::Result<()> {
        let pop = pop.into();
        let w = self.source.w;
        let t = crate::perf::span("pop: terrain");
        let mut rows = Vec::new();
        let mut touched = Vec::new();
        // Nodes whose dark land (sim flag 0x04) differs from what is drawn (byte 2 bit 0x40):
        // `CDarkTribe` 0x4cbb60 → 0x65b210 → `SetDarkLandFlag` 0x66d500 in the release.
        let mut dark = Vec::new();
        if pop.size == w {
            let flags = pop.flags.len() == pop.height.len();
            // Rows equal in the last population are equal in the grid.
            let last = self.population.as_ref().filter(|p| {
                self.terrain_synced && p.height.len() == pop.height.len() && p.ground.len() == pop.ground.len() && p.flags.len() == pop.flags.len()
            });
            for (y, row) in self.source.nodes.chunks_mut(w).enumerate() {
                let r = y * w..(y + 1) * w;
                if last.is_some_and(|p| p.height[r.clone()] == pop.height[r.clone()] && p.ground[r.clone()] == pop.ground[r.clone()] && (!flags || p.flags[r.clone()] == pop.flags[r.clone()])) {
                    continue;
                }
                for (i, n) in r.zip(row) {
                    let (h, g) = (pop.height[i], pop.ground[i]);
                    if n.height != h || n.ground != g {
                        (n.height, n.ground) = (h, g);
                        touched.push(i);
                        if rows.last() != Some(&y) {
                            rows.push(y);
                        }
                    }
                    if flags && (pop.flags[i] & 4 != 0) != n.dark() {
                        dark.push((i, pop.flags[i] & 4 != 0));
                    }
                }
            }
        }
        for &(i, d) in &dark {
            let (x, y) = (i % w, i / w);
            self.source.set_dark(x, y, d);
            // The crossing bits of the rows above and below change too.
            rows.extend([y.saturating_sub(1), y, (y + 1).min(self.source.h - 1)]);
        }
        if !dark.is_empty() {
            rows.sort_unstable();
            rows.dedup();
        }
        // With the final heights: a node re-shaded by an earlier change in the same snapshot is
        // re-shaded again by the later one, as the original's sequence of calls ends up.
        for &i in &touched {
            let (x, y) = (i % w, i / w);
            self.source.refresh_shading(x, y);
            if y > 0 {
                self.source.refresh_shading(x, y - 1);
            }
            if y + 1 < self.source.h {
                self.source.refresh_shading(x, y + 1);
            }
        }
        // Only the changed rows' runs: one changed node at the top and one at the bottom do not
        // re-mesh the whole map.
        if !rows.is_empty() {
            let (fade, border) = self.fog.as_ref().map_or((0, -50000), |f| (f.fade, f.border));
            for (y0, y1) in crate::terrain::row_runs(&rows, RUN_GAP) {
                self.mesh.update_rows(&FogView { grid: &self.source, fade, border }, y0.saturating_sub(1), y1 + 1);
            }
            gpu.sync_terrain(&mut self.mesh);
        }
        drop(t);
        if let Some(s) = &mut self.sprites {
            s.set_population(&pop)?;
            let _t = crate::perf::span("pop: sprite sync");
            if let Some(r) = &mut gpu.sprites {
                r.sync(&gpu.device, &gpu.queue, s);
            }
        }
        self.terrain_synced = pop.size == w;
        self.population = Some(pop);
        Ok(())
    }

    /// Show accessory icons (build markers) at nodes and the building under the cursor, see
    /// `SpriteScene::set_markers` / `set_ghost`, and the selected settlers' icons (`set_selection`).
    pub fn set_markers(
        &mut self,
        gpu: &mut Gpu,
        markers: &[(u16, u16, u8)],
        ghost: Option<(u16, u16, u8, u8, u8)>,
        selection: &[(u16, u16, u16, u8)],
    ) -> anyhow::Result<()> {
        if let Some(s) = &mut self.sprites {
            s.set_markers(markers)?;
            s.set_ghost(ghost)?;
            s.set_selection(selection)?;
            if let Some(r) = &mut gpu.sprites {
                r.sync(&gpu.device, &gpu.queue, s);
            }
        }
        Ok(())
    }

    /// A map effect (`CEffects::AddEffect`, see [`SpriteScene::add_effect`]) starting at game tick
    /// `tick`.
    pub fn add_effect(&mut self, gpu: &mut Gpu, effect: u8, x: u16, y: u16, tick: u64) -> anyhow::Result<()> {
        if let Some(s) = &mut self.sprites {
            s.add_effect(effect, x, y, tick)?;
            if let Some(r) = &mut gpu.sprites {
                r.sync(&gpu.device, &gpu.queue, s);
            }
        }
        Ok(())
    }

    /// A tribe effect of `player` (`CEffects` vtbl+0x14, see [`SpriteScene::add_tribe_effect`])
    /// in the settler file of `race` (the player's tribe, `Population::races`), uploaded.
    #[allow(clippy::too_many_arguments)]
    pub fn add_tribe_effect(&mut self, gpu: &mut Gpu, effect: u8, x: u16, y: u16, player: u8, race: u8, tick: u64) -> anyhow::Result<()> {
        if let Some(s) = &mut self.sprites {
            let race = s4_formats::gamedata::Race::from_u16(race as u16).unwrap_or(s4_formats::gamedata::Race::ROMAN);
            s.add_tribe_effect(effect, x, y, player, race, tick)?;
            if let Some(r) = &mut gpu.sprites {
                r.sync(&gpu.device, &gpu.queue, s);
            }
        }
        Ok(())
    }

    /// A lasting effect (`CEffects` vtbl+0x28, see [`SpriteScene::add_lasting_effect`]),
    /// uploaded. Returns its handle (0: none).
    pub fn add_lasting_effect(&mut self, gpu: &mut Gpu, effect: u8, x: u16, y: u16, tick: u64) -> anyhow::Result<u32> {
        let Some(s) = &mut self.sprites else { return Ok(0) };
        let handle = s.add_lasting_effect(effect, x, y, tick)?;
        if let Some(r) = &mut gpu.sprites {
            r.sync(&gpu.device, &gpu.queue, s);
        }
        Ok(handle)
    }

    /// `CEffects` vtbl+0x24: remove the lasting effect `handle`.
    pub fn remove_effect(&mut self, gpu: &mut Gpu, handle: u32) {
        if let Some(s) = &mut self.sprites {
            s.remove_effect(handle);
            if let Some(r) = &mut gpu.sprites {
                r.sync(&gpu.device, &gpu.queue, s);
            }
        }
    }

    /// A flying effect (`sprites::SpriteScene::add_flight`), uploaded.
    #[allow(clippy::too_many_arguments)]
    pub fn add_flight(&mut self, gpu: &mut Gpu, effect: u8, x: u16, y: u16, from: (f32, f32), to: (f32, f32), ticks: u32, tick: u64) -> anyhow::Result<()> {
        if let Some(s) = &mut self.sprites {
            s.add_flight(effect, x, y, from, to, ticks, tick)?;
            if let Some(r) = &mut gpu.sprites {
                r.sync(&gpu.device, &gpu.queue, s);
            }
        }
        Ok(())
    }

    /// A projectile (`sprites::SpriteScene::add_projectile`), uploaded.
    #[allow(clippy::too_many_arguments)]
    pub fn add_projectile(&mut self, gpu: &mut Gpu, kind: &crate::sprites::ProjectileKind, from: (u16, u16), to: (u16, u16), delay: i32, offset: (i32, i32), tick: u64) -> anyhow::Result<()> {
        if let Some(s) = &mut self.sprites {
            s.add_projectile(kind, from, to, delay, offset, tick)?;
            if let Some(r) = &mut gpu.sprites {
                r.sync(&gpu.device, &gpu.queue, s);
            }
        }
        Ok(())
    }

    /// Advance animations and the fog to game tick `tick` and upload what changed.
    pub fn set_tick(&mut self, gpu: &mut Gpu, tick: u64) {
        let mut rows: Option<(usize, usize)> = None;
        let mut fog_changed = false;
        if let Some(f) = &mut self.fog
            && let Some(d) = &mut f.driver
        {
            let w = self.source.w;
            for t in self.tick + 1..=tick {
                let (fade, border) = (d.fade, d.border);
                let centre = f.centre;
                if let Some((a, b)) = d.tick(|| fog::test_pattern(w, centre, t)) {
                    rows = Some(rows.map_or((a, b), |(x, y)| (x.min(a), y.max(b))));
                }
                fog_changed |= rows.is_some() || (fade, border) != (d.fade, d.border);
            }
            if fog_changed {
                f.gfx.copy_from_slice(&d.gfx);
                (f.fade, f.border) = (d.fade, d.border);
            }
        }
        self.tick = self.tick.max(tick);
        if fog_changed {
            self.apply_fog(gpu, rows.into_iter().collect());
        }
        if let (Some(s), Some(r)) = (&mut self.sprites, &mut gpu.sprites)
            && s.set_tick(tick)
        {
            r.set_scene(s);
        }
    }

    /// Push the fog driver's state to the terrain (gfx bits, fade, border, road rule) and the
    /// sprites (per-node fog values). `rows` = runs `(first, last)` holding the rows whose gfx
    /// bits changed, ascending.
    fn apply_fog(&mut self, gpu: &mut Gpu, rows: Vec<(usize, usize)>) {
        let Some(f) = &mut self.fog else { return };
        let (w, h) = (self.source.w, self.source.h);
        let (fade, border) = (f.fade, f.border);
        for &(a, b) in &rows {
            for i in a * w..(b + 1) * w {
                let n = &mut self.source.nodes[i];
                n.gfx_bits = (n.gfx_bits & 0xC0) | (f.gfx[i] & 0x3F);
            }
        }
        let view = FogView { grid: &self.source, fade, border };
        let mut dirty: Vec<usize> = rows.iter().flat_map(|&(a, b)| a..=b).collect();
        for r in &mut f.roads {
            let hidden = view.road_hidden(r.0, r.1);
            if hidden != r.2 {
                r.2 = hidden;
                dirty.push(r.1);
            }
        }
        dirty.sort_unstable();
        dirty.dedup();
        if !dirty.is_empty() {
            let _t = crate::perf::span("fog: terrain rows");
            for (a, b) in crate::terrain::row_runs(&dirty, RUN_GAP) {
                self.mesh.update_rows(&view, a, b.min(h - 1));
            }
            gpu.sync_terrain(&mut self.mesh);
        }
        if let Some(t) = &mut gpu.terrain {
            t.fog_fade = fade;
            t.fog_border = border;
        }
        if let (Some(s), Some(r)) = (&mut self.sprites, &mut gpu.sprites) {
            let t = crate::perf::span("fog: object values");
            let mut lut = [[[0u8; 8]; 8]; 16];
            for (fd, a) in lut.iter_mut().enumerate() {
                for (old, b) in a.iter_mut().enumerate() {
                    for (new, c) in b.iter_mut().enumerate() {
                        *c = object_fog(fd as u32, old as u32, new as u32);
                    }
                }
            }
            let value = |i: usize| {
                let g = self.source.nodes[i].gfx_bits as usize;
                lut[row_fade(i / w, fade, border) as usize][g & 7][(g >> 3) & 7]
            };
            // With the same fade step and border only the rows whose bits changed get new values.
            let values = if f.sprite_fade == Some((fade, border)) && s.fog().len() == w * h {
                let mut v = s.fog().to_vec();
                for &(a, b) in &rows {
                    for (i, x) in v.iter_mut().enumerate().take((b + 1) * w).skip(a * w) {
                        *x = value(i);
                    }
                }
                v
            } else {
                (0..w * h).map(value).collect()
            };
            f.sprite_fade = Some((fade, border));
            drop(t);
            s.set_fog(values);
            r.set_scene(s);
        }
    }

    /// Flatten ground around a node (demo of the dirty-band rebuild).
    pub fn flatten(&mut self, gpu: &mut Gpu, x: usize, y: usize) {
        let h = self.source.node(x, y).height;
        let (y0, y1) = self.source.flatten(x, y, 6, h);
        self.terrain_synced = false;
        // Heights changed: their gradients and those of the rows above/below too.
        let (fade, border) = self.fog.as_ref().map_or((0, -50000), |f| (f.fade, f.border));
        self.mesh.update_rows(&FogView { grid: &self.source, fade, border }, y0.saturating_sub(1), y1 + 1);
        gpu.sync_terrain(&mut self.mesh);
        if let (Some(s), Some(r)) = (&mut self.sprites, &mut gpu.sprites) {
            s.update_heights(&|x, y| self.source.node(x, y).height);
            r.set_scene(s);
        }
    }
}

/// A map path as given, or relative to the game's `Map\` directory (`.map` optional). A random
/// map's name `RD_<key>` (also a missing file of that name) is the generated map
/// (`s4_mapgen::map_path`; the release generates it from the name at the game start, 0x5069b0).
pub fn resolve_map(m: &str) -> std::path::PathBuf {
    let p = std::path::PathBuf::from(m);
    if p.exists() {
        return p;
    }
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    if stem.starts_with("RD_") && s4_mapgen::key::parse(&stem).is_some() {
        match s4_mapgen::map_path(&stem) {
            Ok(g) => return g,
            Err(e) => tracing::warn!("{stem}: {e}"),
        }
    }
    let base = s4_formats::game_dir().join("Map");
    let q = s4_formats::path::resolve(base.join(m));
    if q.exists() { q } else { s4_formats::path::resolve(base.join(format!("{m}.map"))) }
}
