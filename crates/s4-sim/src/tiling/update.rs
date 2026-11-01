//! Incremental tiling updates (Tiling.cpp): a node becomes blocked / unblocked or changes owner,
//! and the tiles, links, sectors and eco-sectors around it are patched in place. Ids follow the
//! original's allocation (free list, `CSectors`, `CEcoSectorMgr` slots), so they match what the
//! game would assign.
//!
//! Central piece is `NeighborAnalysis` (globals.cpp ~39088): the six neighbour tiles of the node,
//! grouped into "areas" (runs of equal tile ids around the hex ring, the first and last run joined
//! when equal). Whether the old tile falls apart, which neighbour tiles must merge, and which
//! sectors split or join is decided from these areas.
//!
//! `CEcoSectorMgr` notifications are queued as [`EcoNote`]s for the economy; fogging is
//! TODO(port).

use std::collections::{HashSet, VecDeque};

use super::{EcoNote, FIRST_REAL, MAX_USED_TILES, SENTINEL, TILE_CATAPULT, Tile, Tiling, UNUSED, borderstone_bit};
use super::{ground_tile_type, owner_tile_type, square_parity};
use crate::grid::{Grid, Y16X16, flag};
use crate::spiral;

/// The world layers the tiling reads and writes.
pub struct Ctx<'a> {
    pub grid: Grid,
    pub ground: &'a [u8],
    pub flags: &'a mut [u8],
    pub gfx_bits: &'a mut [u8],
    /// `CWorldManager::MapObjectId` / `PileId` layers: nodes holding one get per-node
    /// [`EcoNote::Node`] notifications.
    pub map_object: &'a [u16],
    pub deco_object: &'a [u16],
}

impl Ctx<'_> {
    fn blocked(&self, n: usize) -> bool {
        self.flags[n] & flag::BLOCKED_LAND != 0
    }
    fn water(&self, n: usize) -> bool {
        self.ground[n] <= 8
    }
    fn index(&self, x: i32, y: i32) -> usize {
        self.grid.index(x as u32, y as u32)
    }
    /// `CTilingWorld::WorldInInnerWorld2`.
    fn inner2(&self, x: i32, y: i32) -> bool {
        self.grid.in_inner_world(x, y, 2)
    }
}

/// `g_sNeighborPoints`, as packed `Y16X16` deltas (`Y16X16::g_iNeighborY16X16Modifiers`).
const NB: [(i32, i32); 6] = super::NEIGHBORS;

/// `CTiling::TileType(owner, ground, x, y)`.
fn tile_type(owner: u8, ground: u8, x: i32, y: i32) -> u32 {
    owner_tile_type(owner) | ground_tile_type(ground) | square_parity(x, y)
}

/// `NeighborAnalysis` state for one modification.
#[derive(Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Na {
    catapult: bool,
    x: i32,
    y: i32,
    node: usize,
    /// Neighbour tile ids (0 for pseudo / invalid), `s_iNeighborTileIds`.
    nb: [u16; 6],
    /// `(tile, neighbour direction)` per area, `s_iTileAreaTileIds` / `s_iTileAreaDirections`.
    areas: Vec<(u16, usize)>,
    old_tile: u16,
    old_owner: u8,
    /// `s_iOldTileChangeCount`: areas made of the old tile; `s_iOldTileFirstArea`.
    old_count: usize,
    old_first: usize,
    new_type: u32,
    new_tile: u16,
    new_created: bool,
}

impl Na {
    /// `CalcTileAreas`.
    fn calc_areas(&mut self) {
        self.areas.clear();
        self.areas.push((self.nb[0], 0));
        for i in 1..6 {
            if self.nb[i - 1] != self.nb[i] {
                self.areas.push((self.nb[i], i));
            }
        }
        if self.areas.len() > 1 && self.nb[0] == self.nb[5] {
            let last = self.areas.pop().expect("len > 1");
            self.areas[0].1 = last.1;
        }
    }
    fn last(&self) -> usize {
        self.areas.len() - 1
    }
}

impl Tiling {
    fn side(&self, ctx: &Ctx) -> usize {
        ctx.grid.size() as usize / 16
    }

    /// `NeighborAnalysis::Init(type, x, y)`.
    fn na_init(&self, ctx: &Ctx, catapult: bool, x: i32, y: i32) -> Na {
        let mut na = Na { catapult, x, y, node: ctx.index(x, y), ..Na::default() };
        self.na_refresh(ctx, &mut na);
        na
    }

    /// The neighbour part of `Init` (also `ReInit`, which keeps the old/new tile state).
    fn na_refresh(&self, ctx: &Ctx, na: &mut Na) {
        let ids = self.ids(na.catapult);
        for (i, &(dx, dy)) in NB.iter().enumerate() {
            let t = ids[ctx.index(na.x + dx, na.y + dy)];
            na.nb[i] = if self.is_valid_real(t) { t } else { 0 };
        }
        na.calc_areas();
    }

    /// `NeighborAnalysis::InitOld`.
    fn na_init_old(na: &mut Na, old: u16, owner: u8) {
        na.old_tile = old;
        na.old_owner = owner;
        na.old_count = 0;
        if let Some(i) = na.areas.iter().position(|a| a.0 == old) {
            na.old_first = i;
            na.old_count = na.areas[i..].iter().filter(|a| a.0 == old).count();
        }
    }

    // ---- tiles and links (CTilesEx / CTilesAndLinks / CLinks) ----

    /// `CTilesEx::CalculateListOfFreeTiles`.
    fn rebuild_free_list(&mut self) {
        self.free = (FIRST_REAL..SENTINEL).filter(|&i| !self.tiles[i as usize].used()).collect();
        self.num_used = (FIRST_REAL..SENTINEL).filter(|&i| self.tiles[i as usize].used()).count() as u32;
        self.last_used = (FIRST_REAL..SENTINEL).rev().find(|&i| self.tiles[i as usize].used()).unwrap_or(9);
    }

    /// `CTilesEx::PushTileUndef`: the head of the free list.
    fn push_tile_undef(&mut self) -> Option<u16> {
        if self.num_used >= MAX_USED_TILES {
            self.rebuild_free_list();
            if self.num_used >= MAX_USED_TILES {
                return None;
            }
        }
        let id = self.free.pop_front()?;
        self.tiles[id as usize] = Tile { ty: 0, ..Tile::unused() };
        self.num_used += 1;
        self.last_used = self.last_used.max(id);
        Some(id)
    }

    /// `CTilesEx::PushTile(type, center)`.
    fn push_tile(&mut self, ctx: &Ctx, ty: u32, center: Y16X16) -> u16 {
        let id = self.push_tile_undef().expect("tile limit");
        self.tiles[id as usize] = Tile { ty, center, ..Tile::unused() };
        let side = self.side(ctx);
        self.insert_square_list(id, side);
        // COwnerMapEx::NotifyCreateTile (0x5b00a0).
        self.owner_map_add(center, self.tiles[id as usize].owner());
        id
    }

    /// `CTilesEx::DuplicateTile`: copy of the tile without links and size.
    fn duplicate_tile(&mut self, ctx: &Ctx, src: u16) -> u16 {
        let id = self.push_tile_undef().expect("tile limit");
        let mut t = self.tiles[src as usize].clone();
        t.links.clear();
        t.set_size(0);
        self.tiles[id as usize] = t;
        let side = self.side(ctx);
        self.insert_square_list(id, side);
        let (c, o) = (self.tiles[id as usize].center, self.tiles[id as usize].owner());
        self.owner_map_add(c, o);
        id
    }

    /// `CTilesAndLinks::DeleteTile`: unlink from all neighbours, then `CTilesEx::DeleteTile`.
    fn delete_tile(&mut self, ctx: &Ctx, id: u16) {
        if !self.is_valid_real(id) {
            return; // "DeleteTile(): Not a used real tile!"
        }
        let links = self.tiles[id as usize].links.clone();
        for l in links {
            self.delete_link(l, id);
        }
        let side = self.side(ctx);
        self.delete_square_list(id, side);
        let (c, o) = (self.tiles[id as usize].center, self.tiles[id as usize].owner());
        self.owner_map_remove(c, o, side);
        self.tiles[id as usize] = Tile::unused();
        self.free.push_front(id);
        self.num_used -= 1;
        if self.last_used == id {
            let mut i = id - 1;
            while self.tiles[i as usize].ty == UNUSED {
                i -= 1;
            }
            self.last_used = i;
        }
    }

    /// `CTilesAndLinks::AddLink`: append if absent.
    fn add_link(&mut self, owner: u16, link: u16) {
        let l = &mut self.tiles[owner as usize].links;
        if !l.contains(&link) {
            l.push(link);
        }
    }

    /// `CLinks::LinkListRemoveLinkTileId`: the last entry takes the removed one's place.
    fn delete_link(&mut self, owner: u16, link: u16) {
        let l = &mut self.tiles[owner as usize].links;
        if let Some(i) = l.iter().position(|&x| x == link) {
            l.swap_remove(i);
        }
    }

    /// `CTilesAndLinks::ReplaceLink`.
    fn replace_link(&mut self, owner: u16, old: u16, new: u16) {
        let l = &mut self.tiles[owner as usize].links;
        match l.iter().position(|&x| x == old) {
            Some(i) => l[i] = new,
            None => self.add_link(owner, new), // "OldLinkTileId not found [may be ignored]!"
        }
    }

    /// `CTiling::CalcIntLinkList`: breadth-first over the tile's nodes from `center`; neighbour
    /// real tiles in discovery order. Returns the list and the visited nodes (`CSquareVisited`,
    /// which also marks examined non-tile neighbours).
    fn calc_int_link_list(&self, ctx: &Ctx, catapult: bool, center: Y16X16) -> (Vec<u16>, HashSet<usize>) {
        let ids = self.ids(catapult);
        let start = ctx.index(center.x(), center.y());
        let tile = ids[start];
        let mut list = Vec::new();
        let mut seen = HashSet::from([start]);
        let mut q = VecDeque::from([start]);
        while let Some(n) = q.pop_front() {
            let (x, y) = (ctx.grid.x(n) as i32, ctx.grid.y(n) as i32);
            for &(dx, dy) in &NB {
                let m = ctx.index(x + dx, y + dy);
                if seen.insert(m) {
                    let t = ids[m];
                    if t == tile {
                        q.push_back(m);
                    } else if self.is_valid_real(t) && !list.contains(&t) {
                        list.push(t);
                    }
                }
            }
        }
        (list, seen)
    }

    /// `CTiling::CalcTileLinks` + `CTilesAndLinks::AddLinkList`.
    fn calc_tile_links(&mut self, ctx: &Ctx, id: u16) -> HashSet<usize> {
        let t = &self.tiles[id as usize];
        let (list, seen) = self.calc_int_link_list(ctx, t.catapult(), t.center);
        self.tiles[id as usize].links = list.clone();
        for l in list {
            self.add_link(l, id);
        }
        seen
    }

    /// `CTiling::RecalcTileLinks`: new list, old one unlinked first (`DeleteLinkList`).
    fn recalc_tile_links(&mut self, ctx: &Ctx, id: u16) -> HashSet<usize> {
        let t = &self.tiles[id as usize];
        let (list, seen) = self.calc_int_link_list(ctx, t.catapult(), t.center);
        let old = std::mem::take(&mut self.tiles[id as usize].links);
        for l in old {
            self.delete_link(l, id);
        }
        self.tiles[id as usize].links = list.clone();
        for l in list {
            self.add_link(l, id);
        }
        seen
    }

    /// `CTiling::ChangeTileIds`: flood the old tile's nodes from `start` to `new`.
    fn change_tile_ids(&mut self, ctx: &Ctx, catapult: bool, start: usize, new: u16) {
        let old = self.ids(catapult)[start];
        let mut count = 1;
        self.ids_mut(catapult)[start] = new;
        let mut q = VecDeque::from([start]);
        while let Some(n) = q.pop_front() {
            let (x, y) = (ctx.grid.x(n) as i32, ctx.grid.y(n) as i32);
            for &(dx, dy) in &NB {
                let m = ctx.index(x + dx, y + dy);
                if self.ids(catapult)[m] == old {
                    self.ids_mut(catapult)[m] = new;
                    q.push_back(m);
                    count += 1;
                }
            }
        }
        self.tiles[old as usize].change_size(-count);
        self.tiles[new as usize].change_size(count);
    }

    /// `CTiling::SplitTile(type, xy)`: the part of the tile reachable from `xy` becomes a new tile.
    fn split_tile(&mut self, ctx: &Ctx, catapult: bool, x: i32, y: i32) -> HashSet<usize> {
        let n = ctx.index(x, y);
        let old = self.ids(catapult)[n];
        let new = self.duplicate_tile(ctx, old);
        self.change_tile_ids(ctx, catapult, n, new);
        self.tiles[new as usize].center = Y16X16::pack(x, y);
        self.calc_tile_links(ctx, new)
    }

    /// `CTiling::CheckBorderLink(xy, tile)`: does the tile at `xy` still touch `tile`? Walks the
    /// tile's border: a node whose six neighbours all belong to its own tile takes back the
    /// pushes it made (`TLimitedFIFO::UndoPush`).
    fn check_border_link(&self, ctx: &Ctx, x: i32, y: i32, tile: u16, catapult: bool) -> bool {
        let ids = self.ids(catapult);
        let start = ctx.index(x, y);
        let own = ids[start];
        let mut seen = HashSet::new();
        let mut q: VecDeque<usize> = VecDeque::from([start]);
        while let Some(n) = q.pop_front() {
            let mut pushed = 0;
            let mut interior = true;
            if seen.insert(n) {
                let (nx, ny) = (ctx.grid.x(n) as i32, ctx.grid.y(n) as i32);
                for &(dx, dy) in &NB {
                    let m = ctx.index(nx + dx, ny + dy);
                    let t = ids[m];
                    if t == tile {
                        return true;
                    }
                    if t == own {
                        if !seen.contains(&m) {
                            q.push_back(m);
                            pushed += 1;
                        }
                    } else {
                        interior = false;
                    }
                }
                if interior {
                    for _ in 0..pushed {
                        q.pop_back();
                    }
                }
            }
        }
        false
    }

    // ---- sectors and eco-sectors ----

    /// `CSectors::DeleteSector`.
    fn delete_sector(&mut self, s: u16) {
        if self.sector_count > 0 && s > 0 {
            self.sector_count -= 1;
            self.sector_used[s as usize] = 0;
            self.sector_sizes[s as usize] = u32::MAX;
        }
    }

    /// `CTiling::NotifyDeleteEcoSector` (+ freeing the `CEcoSectorMgr` slot).
    fn delete_eco(&mut self, e: u16) {
        self.eco_sizes[e as usize] = -1;
        self.eco_used[e as usize] = false;
        self.eco_min_free = self.eco_min_free.min(e);
        self.eco_notes.push(EcoNote::Delete { id: e });
    }

    /// `CTiling::NotifyChangeEcoSector(node, old, new, true)`: per-node eco-sector sizes, and the
    /// manager's per-node handler where the node holds something.
    fn notify_eco(&mut self, ctx: &Ctx, n: usize, old: u16, new: u16) {
        if old != 0 {
            self.eco_sizes[old as usize] -= 1;
        }
        if new != 0 {
            self.eco_sizes[new as usize] += 1;
        }
        if ctx.map_object[n] != 0 || ctx.deco_object[n] != 0 {
            self.eco_notes.push(EcoNote::Node { node: n as u32, old, new });
        }
    }

    /// `WalkTileNotifyEcoSectorChange(tile, old, new)`: every node of the tile, breadth first from
    /// its centre (`CSquareVisited`, neighbours in direction order).
    fn notify_eco_tile(&mut self, ctx: &Ctx, tile: u16, old: u16, new: u16) {
        let c = self.tiles[tile as usize].center;
        let start = ctx.index(c.x(), c.y());
        let mut seen = HashSet::from([start]);
        let mut q = VecDeque::from([start]);
        while let Some(n) = q.pop_front() {
            self.notify_eco(ctx, n, old, new);
            let (x, y) = (ctx.grid.x(n) as i32, ctx.grid.y(n) as i32);
            for &(dx, dy) in &NB {
                if let Some(m) = ctx.grid.checked_index(x + dx, y + dy)
                    && seen.insert(m)
                    && self.normal[m] == tile
                {
                    q.push_back(m);
                }
            }
        }
    }

    fn sector_mask(&self, id: u16) -> u32 {
        self.tiles[id as usize].ty & 0x3C00_0000
    }

    /// `WalkTilesChangeSector`.
    fn walk_change_sector(&mut self, start: u16, new: u16) {
        let old = self.tiles[start as usize].sector;
        let mask = self.sector_mask(start);
        let mut q = VecDeque::from([start]);
        while let Some(id) = q.pop_front() {
            if self.tiles[id as usize].sector != old {
                continue;
            }
            self.tiles[id as usize].sector = new;
            for &l in &self.tiles[id as usize].links {
                if mask & self.tiles[l as usize].ty != 0 {
                    q.push_back(l);
                }
            }
        }
    }

    /// `WalkTilesSetVisited`: tiles reachable over links sharing the start's sector mask. (The
    /// decompiled loop tests the popped tile instead of the neighbour; read as a decompiler
    /// artefact — taken literally it would visit only the start tile.)
    fn walk_set_visited(&self, start: u16, visited: &mut HashSet<u16>) {
        let mask = self.sector_mask(start);
        visited.insert(start);
        let mut q = VecDeque::from([start]);
        while let Some(id) = q.pop_front() {
            for &l in &self.tiles[id as usize].links {
                if mask & self.tiles[l as usize].ty != 0 && visited.insert(l) {
                    q.push_back(l);
                }
            }
        }
    }

    /// `WalkTilesChangeEcoSector` (`notify`: `...AndNotify`, which also moves the nodes' sizes).
    fn walk_change_eco(&mut self, ctx: &Ctx, start: u16, new: u16, notify: bool) {
        let owner = self.tiles[start as usize].owner();
        let old = self.tiles[start as usize].eco_sector;
        let mut q = VecDeque::from([start]);
        while let Some(id) = q.pop_front() {
            if self.tiles[id as usize].eco_sector != old {
                continue;
            }
            self.tiles[id as usize].eco_sector = new;
            if notify {
                self.notify_eco_tile(ctx, id, old, new);
            }
            for &l in &self.tiles[id as usize].links {
                if self.tiles[l as usize].owner() == owner {
                    q.push_back(l);
                }
            }
        }
    }

    /// `WalkTilesEcoSectorSetVisitedAndCountSize`: sizes are added when a tile is pushed, so a
    /// tile reached from several visited tiles counts several times (as in the original).
    fn walk_eco_count(&self, start: u16, visited: &mut HashSet<u16>) -> i32 {
        let eco = self.tiles[start as usize].eco_sector;
        let mut size = self.tiles[start as usize].size() as i32;
        let mut q = VecDeque::from([start]);
        while let Some(id) = q.pop_front() {
            if visited.insert(id) {
                for &l in &self.tiles[id as usize].links {
                    if self.tiles[l as usize].eco_sector == eco {
                        size += self.tiles[l as usize].size() as i32;
                        q.push_back(l);
                    }
                }
            }
        }
        size
    }

    /// `CTiling::MergeSectors(a, b)`: b's sector becomes a's.
    fn merge_sectors(&mut self, a: u16, b: u16) {
        let sa = self.tiles[a as usize].sector;
        let sb = self.tiles[b as usize].sector;
        self.walk_change_sector(b, sa);
        self.delete_sector(sb);
    }

    /// `CTiling::MergeEcoSectors(a, b)`: the smaller eco-sector (by node count; ties: a's) joins
    /// the other; returns the surviving id.
    fn merge_eco(&mut self, ctx: &Ctx, a: u16, b: u16) -> u16 {
        let (mut from, mut ea, mut eb) = (a, self.tiles[a as usize].eco_sector, self.tiles[b as usize].eco_sector);
        if self.eco_sizes[ea as usize] > self.eco_sizes[eb as usize] {
            from = b;
            std::mem::swap(&mut ea, &mut eb);
        }
        self.eco_notes.push(EcoNote::MergeStart { keep: eb, gone: ea });
        self.walk_change_eco(ctx, from, eb, true);
        self.eco_notes.push(EcoNote::Merged { keep: eb, gone: ea });
        self.delete_eco(ea);
        eb
    }

    /// `CTiling::MergeTiles(keep, gone)`.
    fn merge_tiles(&mut self, ctx: &Ctx, keep: u16, gone: u16) {
        if self.tiles[keep as usize].sector != self.tiles[gone as usize].sector {
            self.merge_sectors(keep, gone);
        }
        if self.tiles[keep as usize].eco_sector != self.tiles[gone as usize].eco_sector {
            self.merge_eco(ctx, keep, gone);
        }
        // WalkTileChangeTileId(gone, keep) from gone's centre.
        let catapult = self.tiles[gone as usize].catapult();
        let c = self.tiles[gone as usize].center;
        let start = ctx.index(c.x(), c.y());
        let mut q = VecDeque::from([start]);
        while let Some(n) = q.pop_front() {
            if self.ids(catapult)[n] != gone {
                continue;
            }
            self.ids_mut(catapult)[n] = keep;
            let (x, y) = (ctx.grid.x(n) as i32, ctx.grid.y(n) as i32);
            for &(dx, dy) in &NB {
                let m = ctx.index(x + dx, y + dy);
                if self.ids(catapult)[m] == gone {
                    q.push_back(m);
                }
            }
        }
        let size = self.tiles[gone as usize].size() as i32;
        self.tiles[keep as usize].change_size(size);
        self.tiles[gone as usize].set_size(0);
        // CTilesAndLinks::MergeTileLinkLists(keep, gone).
        let links = self.tiles[gone as usize].links.clone();
        for l in links {
            if !self.tiles[keep as usize].links.contains(&l) && l != keep {
                self.add_link(keep, l);
                self.replace_link(l, gone, keep);
            }
        }
        self.delete_tile(ctx, gone);
    }

    // ---- the N* steps ----

    /// `NRemoveElementFromTile`: the node left the old tile, which still exists; move its centre
    /// off the node if needed.
    fn n_remove_element(&mut self, na: &Na) {
        let t = &mut self.tiles[na.old_tile as usize];
        if t.center == Y16X16::pack(na.x, na.y) {
            let (dx, dy) = NB[na.areas[na.old_first].1];
            t.center = Y16X16::pack(na.x + dx, na.y + dy);
        }
    }

    /// `NCheckSectorAndEcoSectorDelete`: the one-node old tile vanished; delete its sector /
    /// eco-sector if no neighbour shares it.
    fn n_check_sector_and_eco_delete(&mut self, na: &Na) {
        let s = self.tiles[na.old_tile as usize].sector;
        let e = self.tiles[na.old_tile as usize].eco_sector;
        let del_s = na.areas.iter().all(|a| self.tiles[a.0 as usize].sector != s);
        let del_e = na.old_owner != 0 && na.areas.iter().all(|a| self.tiles[a.0 as usize].eco_sector != e);
        if del_s {
            self.delete_sector(s);
        }
        if del_e {
            self.delete_eco(e);
        }
    }

    /// `NCheckEcoSectorDelete`.
    fn n_check_eco_delete(&mut self, na: &Na) {
        let e = self.tiles[na.old_tile as usize].eco_sector;
        if e != 0 && !na.areas.iter().any(|a| self.tiles[a.0 as usize].eco_sector == e) {
            self.delete_eco(e);
        }
    }

    /// `NUpdateLinksAndCheckTileSpliting`.
    fn n_update_links_and_split(&mut self, ctx: &Ctx, na: &mut Na) {
        if na.old_count == 1 {
            for i in 0..na.areas.len() {
                let (t, dir) = na.areas[i];
                if t != na.old_tile && self.is_valid_real(t) {
                    let (dx, dy) = NB[dir];
                    if !self.check_border_link(ctx, na.x + dx, na.y + dy, na.old_tile, na.catapult) {
                        self.delete_link(na.old_tile, t);
                        self.delete_link(t, na.old_tile);
                    }
                }
            }
        } else if na.old_count > 1 {
            self.n_check_tile_split(ctx, na);
            self.na_refresh(ctx, na);
        }
    }

    /// `NCheckTileSplit`: re-link the old tile; every neighbour of the old tile its walk did not
    /// reach starts a split-off tile.
    fn n_check_tile_split(&mut self, ctx: &Ctx, na: &Na) {
        let seen = self.recalc_tile_links(ctx, na.old_tile);
        let at = |j: usize| ctx.index(na.x + NB[j].0, na.y + NB[j].1);
        let mut reached: [bool; 6] = std::array::from_fn(|i| seen.contains(&at(i)));
        for j in 0..6 {
            if na.nb[j] == na.old_tile && !reached[j] {
                let (dx, dy) = NB[j];
                let seen2 = self.split_tile(ctx, na.catapult, na.x + dx, na.y + dy);
                for (k, r) in reached.iter_mut().enumerate() {
                    *r |= seen2.contains(&at(k));
                }
            }
        }
    }

    /// Area grouping shared by `NCheckSectorSplit` and `NCheckEcoSectorSplit`: consecutive
    /// qualifying areas form one group; an area whose tile already appeared two or more areas
    /// earlier rejoins that area's group. Returns one representative tile per group.
    fn area_groups(&self, na: &Na, qualifies: impl Fn(&Tile) -> bool) -> Vec<u16> {
        let mut reps = Vec::new();
        let mut group = vec![-1i32; na.areas.len()];
        let mut cur = -1i32;
        for i in 0..na.areas.len() {
            let t = na.areas[i].0;
            if !qualifies(&self.tiles[t as usize]) {
                cur = -1;
            } else {
                if cur < 0 {
                    for j in (0..i.saturating_sub(1)).rev() {
                        if na.areas[j].0 == t {
                            cur = group[j];
                            break;
                        }
                    }
                }
                if cur < 0 {
                    cur = reps.len() as i32;
                    reps.push(t);
                }
            }
            group[i] = cur;
        }
        reps
    }

    /// `NCheckSectorSplit`.
    fn n_check_sector_split(&mut self, na: &Na) {
        let reps = self.area_groups(na, |t| t.sector != 0 && t.sector != 0xFFFF);
        if reps.len() > 1 {
            let s0 = self.tiles[reps[0] as usize].sector;
            let mut visited = HashSet::new();
            self.walk_set_visited(reps[0], &mut visited);
            for &r in &reps[1..] {
                if self.tiles[r as usize].sector == s0 && !visited.contains(&r) {
                    let s = self.create_sector();
                    self.walk_change_sector(r, s);
                }
            }
        }
    }

    /// `NCheckEcoSectorSplit(old owner)`: if the owner's neighbour tiles fell into several
    /// pieces of one eco-sector, the largest keeps the id, the others get new eco-sectors.
    fn n_check_eco_split(&mut self, ctx: &Ctx, na: &Na, owner: u8) {
        let reps = self.area_groups(na, |t| t.owner() == owner);
        if reps.len() <= 1 {
            return;
        }
        let e0 = self.tiles[reps[0] as usize].eco_sector;
        let mut visited = HashSet::new();
        let mut parts = vec![(reps[0], self.walk_eco_count(reps[0], &mut visited))];
        for &r in &reps[1..] {
            if self.tiles[r as usize].eco_sector == e0 && !visited.contains(&r) {
                let size = self.walk_eco_count(r, &mut visited);
                parts.push((r, size));
            }
        }
        if parts.len() > 1 {
            let mut best = 0;
            for m in 1..parts.len() {
                if parts[best].1 < parts[m].1 {
                    best = m;
                }
            }
            for (n, &(r, _)) in parts.iter().enumerate() {
                if n != best {
                    let e = self.create_eco(owner);
                    self.walk_change_eco(ctx, r, e, true);
                    self.eco_notes.push(EcoNote::Split { old: e0, new: e });
                }
            }
        }
    }

    /// `NCalcNewTileId(type)`: the node joins the first neighbour area tile of the same type, or a
    /// new tile is pushed for it.
    fn n_calc_new_tile_id(&mut self, ctx: &Ctx, na: &mut Na, ty: u32) {
        let found = na.areas.iter().map(|a| a.0).find(|&t| self.tiles[t as usize].ty & 0xFFC0_1FFF == ty);
        let (id, created) = match found {
            Some(t) => {
                self.ids_mut(na.catapult)[na.node] = t;
                (t, false)
            }
            None => {
                let t = self.push_tile(ctx, ty, Y16X16::pack(na.x, na.y));
                self.ids_mut(na.catapult)[na.node] = t;
                self.calc_tile_links(ctx, t);
                (t, true)
            }
        };
        na.new_type = ty;
        na.new_tile = id;
        na.new_created = created;
        self.tiles[id as usize].change_size(1);
    }

    /// `NCheckSectorJoin`: a new tile takes the sector of the first real neighbour area and
    /// merges the others into it, or gets a new sector.
    fn n_check_sector_join(&mut self, na: &Na) {
        let s = na.areas.iter().find(|a| a.0 != 0).map_or(0, |a| self.tiles[a.0 as usize].sector);
        if s != 0 {
            self.tiles[na.new_tile as usize].sector = s;
            // NCheckSectorMerge.
            for i in 0..na.areas.len() {
                let t = na.areas[i].0;
                if t != 0 {
                    let st = self.tiles[t as usize].sector;
                    if st != s {
                        self.walk_change_sector(t, s);
                        self.delete_sector(st);
                    }
                }
            }
        } else {
            let s = self.create_sector();
            self.tiles[na.new_tile as usize].sector = s;
        }
    }

    /// `NCheckEcoSectorJoin(owner)`.
    fn n_check_eco_join(&mut self, ctx: &Ctx, na: &Na, owner: u8) {
        if owner == 0 {
            return;
        }
        let e = na
            .areas
            .iter()
            .find(|a| self.tiles[a.0 as usize].owner() == owner)
            .map_or(0, |a| self.tiles[a.0 as usize].eco_sector);
        if e != 0 {
            self.tiles[na.new_tile as usize].eco_sector = e;
            self.notify_eco(ctx, ctx.index(na.x, na.y), 0, e);
            // NCheckEcoSectorMerge.
            let mut e = e;
            for i in 0..na.areas.len() {
                let t = na.areas[i].0;
                if self.tiles[t as usize].owner() == owner && self.tiles[t as usize].eco_sector != e {
                    e = self.merge_eco(ctx, na.new_tile, t);
                }
            }
        } else {
            let e = self.create_eco(owner);
            self.tiles[na.new_tile as usize].eco_sector = e;
            self.notify_eco(ctx, ctx.index(na.x, na.y), 0, e);
        }
    }

    /// `NCheckTileMerge`: the node joined an existing tile; neighbour tiles of the same type merge
    /// into it, the others are linked (merging their sectors / same-owner eco-sectors).
    fn n_check_tile_merge(&mut self, ctx: &Ctx, na: &mut Na) {
        let keep = na.new_tile;
        let ty = self.tiles[keep as usize].ty & 0xFFC0_1FFF;
        for i in 0..na.areas.len() {
            let t = na.areas[i].0;
            if t == 0 || t == keep {
                continue;
            }
            let tt = self.tiles[t as usize].ty;
            if tt & 0xFFC0_1FFF == ty {
                self.merge_tiles(ctx, keep, t);
                na.areas[i].0 = keep;
                for j in i + 2..na.areas.len() {
                    if na.areas[j].0 == t {
                        na.areas[j].0 = keep;
                    }
                }
            } else if !self.tiles[keep as usize].links.contains(&t) {
                self.add_link(keep, t);
                self.add_link(t, keep);
                let (sk, st) = (self.tiles[keep as usize].sector, self.tiles[t as usize].sector);
                if sk != st && sk != 0 && st != 0 {
                    self.merge_sectors(keep, t);
                }
                let (ok, ot) = (ty & 0xF, tt & 0xF);
                if ok != 0 && ok == ot && self.tiles[keep as usize].eco_sector != self.tiles[t as usize].eco_sector {
                    self.merge_eco(ctx, keep, t);
                }
            }
        }
    }

    // ---- the entry points ----

    /// `CTiling::SetBlockingEx(type, x, y)`: the node leaves its real tile.
    fn set_blocking_ex(&mut self, ctx: &Ctx, catapult: bool, x: i32, y: i32) {
        let n = ctx.index(x, y);
        let old = self.ids(catapult)[n];
        if !self.is_valid_real(old) {
            return;
        }
        let owner = self.tiles[old as usize].owner();
        let mut na = self.na_init(ctx, catapult, x, y);
        Self::na_init_old(&mut na, old, owner);
        self.ids_mut(catapult)[n] = owner as u16; // OwnerPseudoTileId
        self.tiles[old as usize].change_size(-1);
        if owner != 0 {
            let e = self.tiles[old as usize].eco_sector;
            self.notify_eco(ctx, n, e, 0);
        }
        if na.old_count > 0 {
            self.n_remove_element(&na);
        } else {
            self.n_check_sector_and_eco_delete(&na);
            self.delete_tile(ctx, old);
        }
        if na.last() >= 3 {
            self.n_update_links_and_split(ctx, &mut na);
            self.n_check_sector_split(&na);
            if owner != 0 {
                self.n_check_eco_split(ctx, &na, owner);
            }
        }
    }

    /// `CTiling::ClearBlockingEx(type, x, y)`: the node (holding a pseudo tile) joins the tiling.
    fn clear_blocking_ex(&mut self, ctx: &Ctx, catapult: bool, x: i32, y: i32) {
        let n = ctx.index(x, y);
        let pseudo = self.ids(catapult)[n];
        if pseudo > 8 {
            return;
        }
        let owner = pseudo as u8;
        let mut na = self.na_init(ctx, catapult, x, y);
        let ty = if catapult { square_parity(x, y) | TILE_CATAPULT } else { tile_type(owner, ctx.ground[n], x, y) };
        self.n_calc_new_tile_id(ctx, &mut na, ty);
        if na.new_created {
            self.n_check_sector_join(&na);
            if owner != 0 {
                self.n_check_eco_join(ctx, &na, owner);
            }
        } else {
            if owner != 0 {
                let e = self.tiles[na.new_tile as usize].eco_sector;
                self.notify_eco(ctx, n, 0, e);
            }
            self.n_check_tile_merge(ctx, &mut na);
        }
    }

    /// `CTiling::CalculateBorderstoneBits7`.
    fn borderstones7(&self, ctx: &mut Ctx, x: i32, y: i32) {
        borderstone_bit(self, ctx.grid, ctx.ground, ctx.gfx_bits, x, y);
        for (dx, dy) in NB {
            borderstone_bit(self, ctx.grid, ctx.ground, ctx.gfx_bits, x + dx, y + dy);
        }
    }

    /// `CTiling::SetBlockedLand(x, y)` (Tiling.cpp:4093): normal tiling at the node, catapult
    /// tiling on the hex-radius-2 spiral around it.
    pub fn set_blocked_land(&mut self, ctx: &mut Ctx, x: i32, y: i32) {
        if !ctx.inner2(x, y) {
            return; // "(X, Y) must be in inner inner world!"
        }
        let n = ctx.index(x, y);
        if ctx.blocked(n) {
            return;
        }
        ctx.flags[n] |= flag::BLOCKED_LAND;
        self.set_blocking_ex(ctx, false, x, y);
        for i in 0..19 {
            self.set_blocking_ex(ctx, true, x + spiral::dx(i), y + spiral::dy(i));
        }
        self.borderstones7(ctx, x, y);
    }

    /// `CTiling::ClearBlockedLand(x, y)` (Tiling.cpp:4277): catapult cells only come back when no
    /// blocked node is left within radius 2 of them.
    pub fn clear_blocked_land(&mut self, ctx: &mut Ctx, x: i32, y: i32) {
        if !ctx.inner2(x, y) {
            return;
        }
        let n = ctx.index(x, y);
        if ctx.water(n) || !ctx.blocked(n) {
            return;
        }
        ctx.flags[n] &= !flag::BLOCKED_LAND;
        self.clear_blocking_ex(ctx, false, x, y);
        for i in 0..19 {
            let (px, py) = (x + spiral::dx(i), y + spiral::dy(i));
            let near_blocked = (0..19).any(|j| ctx.blocked(ctx.index(px + spiral::dx(j), py + spiral::dy(j))));
            if !near_blocked {
                self.clear_blocking_ex(ctx, true, px, py);
            }
        }
        self.borderstones7(ctx, x, y);
    }

    /// `CTiling::SetBlockedWater(x, y)`: water flag 0x200, the node leaves the normal tiling.
    pub fn set_blocked_water(&mut self, ctx: &mut Ctx, x: i32, y: i32) {
        if !ctx.inner2(x, y) {
            return;
        }
        let n = ctx.index(x, y);
        if self.water_flags[n] & 0xC300 != 0xC000 {
            return; // CWaterFlags::IsBlockedWater
        }
        self.water_flags[n] |= 0x200;
        self.set_blocking_ex(ctx, false, x, y);
    }

    /// `CTiling::ClearBlockedWater(x, y)`.
    pub fn clear_blocked_water(&mut self, ctx: &mut Ctx, x: i32, y: i32) {
        if !ctx.inner2(x, y) {
            return;
        }
        let n = ctx.index(x, y);
        if self.water_flags[n] < 0xC000 || self.water_flags[n] & 0x200 == 0 {
            return;
        }
        self.water_flags[n] &= !0x200;
        if self.water_flags[n] & 0xCB00 == 0xC000 {
            self.clear_blocking_ex(ctx, false, x, y);
        }
    }

    /// `CTiling::ChangeOwner(x, y, owner)` (release 0x5b79a0, CTiling vtbl+0x1c; debug Tiling.cpp:4499).
    pub fn change_owner(&mut self, ctx: &mut Ctx, x: i32, y: i32, owner: u8) {
        if !ctx.grid.in_inner_world(x, y, 1) || owner > 8 {
            return;
        }
        let n = ctx.index(x, y);
        let old = self.normal[n];
        let old_owner = self.tiles[old as usize].owner();
        let sector = self.tiles[old as usize].sector;
        if old_owner == owner {
            return;
        }
        // 0x5b79f7: the per-owner node counts follow every change, before the blocked check.
        self.owner_nodes[old_owner as usize] -= 1;
        self.owner_nodes[owner as usize] += 1;
        if ctx.blocked(n) {
            if !ctx.water(n) {
                self.normal[n] = owner as u16;
                self.borderstones7(ctx, x, y);
                self.fog_notes.push((x, y, owner));
            }
            return;
        }
        if !self.is_valid_real(old) {
            return;
        }
        let ty = tile_type(owner, ctx.ground[n], x, y);
        let mut na = self.na_init(ctx, false, x, y);
        Self::na_init_old(&mut na, old, old_owner);
        self.normal[n] = 0;
        self.tiles[old as usize].change_size(-1);
        let e = self.tiles[old as usize].eco_sector;
        if e != 0 {
            self.notify_eco(ctx, n, e, 0);
        }
        if na.old_count > 0 {
            self.n_remove_element(&na);
        } else {
            self.n_check_eco_delete(&na);
            self.delete_tile(ctx, old);
        }
        if na.last() >= 3 {
            self.n_update_links_and_split(ctx, &mut na);
            if old_owner != 0 {
                self.n_check_eco_split(ctx, &na, old_owner);
            }
        }
        self.n_calc_new_tile_id(ctx, &mut na, ty);
        if na.new_created {
            self.tiles[na.new_tile as usize].sector = sector;
            self.n_check_eco_join(ctx, &na, owner);
        } else {
            if owner != 0 {
                let e = self.tiles[na.new_tile as usize].eco_sector;
                self.notify_eco(ctx, n, 0, e);
            }
            self.n_check_tile_merge(ctx, &mut na);
        }
        self.borderstones7(ctx, x, y);
        self.blocked_land_surrounding_check(ctx, x, y);
        self.fog_notes.push((x, y, owner));
    }

    /// `CTiling::BlockedLandSurroundingCheck(x, y)` (Tiling.cpp:6007): after a node changed owner,
    /// follow the boundary of the blocked land next to it; if the walk closes around the node's
    /// side (turning count < 0), blocked land regions on that boundary owned by someone else are
    /// flood-filled to the node's owner (pseudo tiles), so enclosed rocks/buildings change hands.
    fn blocked_land_surrounding_check(&mut self, ctx: &mut Ctx, x: i32, y: i32) {
        let n0 = ctx.index(x, y);
        let owner = self.owner_of(n0);
        if owner == 0 {
            return;
        }
        let mut last_land_dir = None;
        let mut blocked = 0;
        for (i, &(dx, dy)) in NB.iter().enumerate() {
            let m = ctx.index(x + dx, y + dy);
            if ctx.blocked(m) {
                blocked += 1;
                if !ctx.water(m) {
                    last_land_dir = Some(i);
                }
            }
        }
        let Some(start_dir) = last_land_dir else { return };
        if blocked >= 6 {
            return;
        }
        let right = |d: usize| if d == 5 { 0 } else { d + 1 };
        let left = |d: usize| if d == 0 { 5 } else { d - 1 };
        let mut turns = 1i32;
        let mut d = right(start_dir);
        let (mut cx, mut cy) = (x, y);
        let mut closed = false;
        for _ in 0..0x40000 {
            let (nx, ny) = (cx + NB[d].0, cy + NB[d].1);
            let m = ctx.index(nx, ny);
            if ctx.blocked(m) {
                if ctx.water(m) {
                    break;
                }
                turns += 1;
                d = right(d);
            } else {
                if self.owner_of(m) != owner {
                    break;
                }
                turns -= 1;
                d = left(d);
                cx = nx;
                cy = ny;
                if cx == x && cy == y {
                    closed = turns < 0;
                    break;
                }
            }
        }
        if !closed {
            return;
        }
        let mut d = start_dir;
        let (mut cx, mut cy) = (x, y);
        loop {
            let (nx, ny) = loop {
                let (nx, ny) = (cx + NB[d].0, cy + NB[d].1);
                let m = ctx.index(nx, ny);
                if !ctx.blocked(m) {
                    break (nx, ny);
                }
                if self.owner_of(m) != owner {
                    let mut q = VecDeque::from([(nx, ny)]);
                    while let Some((qx, qy)) = q.pop_front() {
                        let k = ctx.index(qx, qy);
                        if ctx.blocked(k) && !ctx.water(k) && self.owner_of(k) != owner {
                            self.normal[k] = owner as u16;
                            self.fog_notes.push((qx, qy, owner));
                            for (dx, dy) in NB {
                                q.push_back((qx + dx, qy + dy));
                            }
                        }
                    }
                }
                self.borderstones7(ctx, nx, ny);
                d = right(d);
            };
            // Step onto the free node found, then turn left.
            d = left(d);
            cx = nx;
            cy = ny;
            if cx == x && cy == y {
                break;
            }
        }
    }
}
