//! The panel's camera window, dialog 2 (screens at least 1024 tall), and the input processor's
//! mode 10 that picks what it follows, as the shipped exe runs them (own RE;
//! `tools/re/gui-2026-10-22/{extras.c,extras3.c}`, `tools/re/gui-2026-10-23/{miniview.c,follow.c}`).
//!
//! * `ToggleUISurfaceVisibility` (0x488650): output height > 0x3ff → dialog 2 (procedure 0x47c870)
//!   and `SetMiniView(1, 16, 818, 174, 190)` (0x65b360, the window's surface and rectangle).
//! * The procedure 0x47c870:
//!   * code 0: the shown node (0x12938c0 x, 0x12938b8 y) = the local player's start (0x149b5d8 /
//!     0x149b5dc + 0x3c · player), then 0x47c830: nothing followed (0x12938c4 id, 0x12938c8 uid,
//!     0x12938bc = 0), scale 0x11637a8 = 24 (0x65b440(0x180000)).
//!   * code 3: 0x282 → UI event 0x1e7 (states 0, 1, 3, 4, 7, 8 → 10, handler 0x4eeb90: mode
//!     +0x50 = 10); 0x283 / 0x284 / 0x285 → scale 10 / 24 / 48 (0x65b440(scale << 16)).
//!   * code 7 on 0x282 ([`MiniView::context`]): event 0x13 centres the main view on the followed
//!     entity or the shown node; then `SetMiniView` again and the scale re-applied (as code 0xb).
//!   * codes 1 / 8 / 9: sounds 6 / 7 and the tooltips.
//! * State 10: a map click (UI 0x1dd → 1, handler 0x4eeba0, [`MiniView::pick`]) picks what to
//!   follow; a right click (0x1e0 → 1) is the leave handler 0x4edb10. Nothing else has a
//!   transition there (no 0x192: the tabs do not end it).
//! * Every logic tick 0x4fdc60 (from `GameLogicTick` 0x4fe060, before `Logic::Update`) places the
//!   window's camera ([`MiniView::place`], [`MiniView::view`]; constants .rdata 0x1070e10 0.5,
//!   0x1070e58 24, 0x1070e68 40, 0x1070e50 10, 0x1070e70 87, 0x1070e78 95, 0x1070e88 / 0x1070fc0
//!   ±255).

use crate::selection::{BBox, Candidate};

/// The dialog slot.
pub const DIALOG: usize = 2;
/// The "follow" button: pick (code 3), centre the main view (code 7).
pub const FOLLOW: u16 = 0x282;
/// UI event of the follow button (→ state 10).
pub const PICK_EVENT: u16 = 0x1e7;
/// `SetMiniView`'s rectangle: x, y, width, height.
pub const RECT: (u32, u32, u32, u32) = (16, 818, 174, 190);

/// The window's state (globals of the release).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MiniView {
    /// 0x12938c0 / 0x12938b8: the node shown while nothing is followed; the follow update keeps
    /// it at the followed entity's node.
    pub node: (i32, i32),
    /// 0x12938c4 / 0x12938c8: the followed entity, (id, uid). Only the fog check of the update
    /// 0x4fdc60 (`CFogging::IsPositionVisible` 0x5aee70) clears it; another uid at the id, flag
    /// 0x8000 or position 0 keep it and show the stored node ([`MiniView::place`]).
    pub follow: Option<(u16, u32)>,
    /// 0x12938bc: the entity a right click on the follow button last centred on (a pointer the
    /// release never clears otherwise).
    pub centred: Option<u16>,
    /// 0x11637a8: the scale (10, 24 or 48; the main view's default zoom is 24).
    pub scale: i32,
}

impl Default for MiniView {
    fn default() -> Self {
        MiniView { node: (0, 0), follow: None, centred: None, scale: 24 }
    }
}

impl MiniView {
    /// The procedure's code 0 with the local player's start.
    pub fn init(&mut self, start: (i32, i32)) {
        self.node = start;
        // 0x47c830.
        self.follow = None;
        self.centred = None;
        self.scale = 24;
    }

    /// Code 3 on `link`: the UI event it sends.
    pub fn command(&mut self, link: u16) -> Option<u16> {
        match link {
            FOLLOW => return Some(PICK_EVENT),
            0x283 => self.scale = 10,
            0x284 => self.scale = 24,
            0x285 => self.scale = 48,
            _ => {}
        }
        None
    }

    /// Code 7 on `link`: the node event 0x13 centres the main view on. `entity(id)` = the uid
    /// and node of the entity with that id, if any (0x436b20 reads `Entity[id] +4`).
    pub fn context(&mut self, link: u16, entity: &dyn Fn(u16) -> Option<(u32, (i32, i32))>) -> Option<(i32, i32)> {
        if link != FOLLOW {
            return None;
        }
        let (id, uid) = self.follow.unwrap_or((0, 0));
        let now = if id == 0 { 0 } else { entity(id).map_or(0, |e| e.0) };
        if now == uid {
            self.centred = (now != 0).then_some(id);
        }
        // [inf] A stale 0x12938bc (its entity gone) centres on the entity now at that id, or on
        // the node without one; the release reads freed memory.
        Some(self.centred.and_then(entity).map_or(self.node, |e| e.1))
    }

    /// 0x4fdc60: the window's camera point (`SetCameraViewPosition`, px at the window's scale
    /// `s`) for the followed entity as `entity` reports it (`None`: no entity at the id).
    /// * The id holds another uid, no entity, flag 0x8000 or position 0: the shown node,
    ///   `((x − y·0.5)·s − s/24·40 − 87, s·0.5·y − s/24·10 − 95)` (no height); the follow stays.
    /// * Its node not visible (`CFogging::IsPositionVisible` 0x5aee70, `visible`): the follow is
    ///   dropped, the node shown the same way.
    /// * Else the node becomes the shown node and the point is the same formula at the entity's
    ///   node minus its render height (`GetRenderHeight`, `height(x, y, s)`: px at scale `s`;
    ///   [inf] the release reads it at the engine's current scale); a moving entity
    ///   (`IMovingEntity`) with a distance (+0x41) moves `(n − p)·distance/255` (each axis
    ///   truncated) towards its neighbour `n` in direction +0x40 (0: x − 1, 1: x − 1 and y − 1,
    ///   2: y − 1, 3: x + 1, 4: x + 1 and y + 1, 5: y + 1).
    pub fn place(&mut self, entity: &dyn Fn(u16) -> Option<Followed>, visible: &dyn Fn(i32, i32) -> bool, height: &dyn Fn(i32, i32, i32) -> i32) -> (i32, i32) {
        let s = self.scale as f64;
        let at = |x: f64, y: f64, h: f64| (((x - y * 0.5) * s - 87.0) - s / 24.0 * 40.0, (s * 0.5 * y - s / 24.0 * 10.0) - h - 95.0);
        let still = |n: (i32, i32)| {
            let (x, y) = at(n.0 as f64, n.1 as f64, 0.0);
            (x as i32, y as i32)
        };
        let Some((id, uid)) = self.follow else { return still(self.node) };
        let e = entity(id).filter(|e| e.uid == uid && !e.on_board && e.node != (0, 0));
        let Some(e) = e else { return still(self.node) };
        self.node = e.node;
        if !visible(e.node.0, e.node.1) {
            self.follow = None;
            return still(self.node);
        }
        let (px, py) = at(e.node.0 as f64, e.node.1 as f64, height(e.node.0, e.node.1, self.scale) as f64);
        let (px, py) = (px as i32, py as i32);
        let Some((dir, d)) = e.walk.filter(|w| w.1 != 0) else { return (px, py) };
        let (mut nx, mut ny) = e.node;
        match dir {
            0 => nx -= 1,
            1 => (nx, ny) = (nx - 1, ny - 1),
            2 => ny -= 1,
            3 => nx += 1,
            4 => (nx, ny) = (nx + 1, ny + 1),
            5 => ny += 1,
            _ => {}
        }
        let (qx, qy) = at(nx as f64, ny as f64, height(nx, ny, self.scale) as f64);
        let (qx, qy) = (qx as i32, qy as i32);
        let d = d as f64;
        let x = px + (((px - qx).abs() as f64 / if qx < px { -255.0 } else { 255.0 }) * d) as i32;
        let y = py + (((py - qy).abs() as f64 / if py <= qy { 255.0 } else { -255.0 }) * d) as i32;
        (x, y)
    }

    /// [`MiniView::place`]'s point as the renderer's view of the window (world px at zoom 1,
    /// zoom `scale / 24`).
    pub fn view(&self, point: (i32, i32)) -> crate::gpu::View {
        let k = self.scale as f32 / 24.0;
        crate::gpu::View { x: point.0 as f32 / k, y: point.1 as f32 / k, zoom: k }
    }

    /// 0x4eeba0 (UI 0x1dd in state 10) at the clicked node `at`: `occupied` = the spiral nodes
    /// (radius 2, 19 nodes, table 0x1169968) that have a map object, in spiral order; `best` =
    /// what `GetTheBestEntity` (0x4ed490) chose from [`candidates`] (none when there are none).
    /// The node becomes the last occupied one; with an entity it is followed, else the clicked
    /// node is shown.
    pub fn pick(&mut self, at: (i32, i32), occupied: &[(i32, i32)], best: Option<(u16, u32)>) {
        if let Some(&n) = occupied.last() {
            self.node = n;
        }
        match best {
            Some(e) => self.follow = Some(e),
            None => {
                self.follow = None;
                self.node = at;
            }
        }
    }
}

/// What [`MiniView::place`] reads of the followed entity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Followed {
    /// +4.
    pub uid: u32,
    /// Flag 0x8000 (`ON_BOARD`: in a building, a ship, a manakopter).
    pub on_board: bool,
    /// +0x18.
    pub node: (i32, i32),
    /// An `IMovingEntity`'s direction +0x40 and distance +0x41; `None` for other entities.
    pub walk: Option<(u8, u8)>,
}

/// 0x4eeba0's candidates on the map objects `objects` (the map object layer 0x15630dc: settlers
/// outside buildings, vehicles, animals) of the spiral nodes: animals (object type 0x80) of
/// anyone, and the `local` player's settlers, ships and land vehicles (types 1, 2, 4).
pub fn candidates(objects: &[Candidate], local: u8) -> Vec<Candidate> {
    objects.iter().filter(|c| c.obj == 0x80 || (c.owner == local && matches!(c.obj, 1 | 2 | 4))).copied().collect()
}

/// `GetTheBestEntity` (0x4ed490) over `candidates` at the clicked node's screen point `click`:
/// those with a box (a gfx job, `+0x26 != 0`) holding it, the largest y wins (the first on
/// ties). `screen(x, y)` = a node's screen point, `bbox` = (width, height, left, top).
pub fn best(candidates: &[Candidate], click: (f32, f32), screen: &dyn Fn(u16, u16) -> (f32, f32), bbox: &dyn Fn(&Candidate) -> Option<BBox>) -> Option<u16> {
    let mut best: Option<&Candidate> = None;
    for c in candidates {
        let Some((w, h, left, top)) = bbox(c) else { continue };
        let (sx, sy) = screen(c.x, c.y);
        let (bx, by) = (sx - left, sy - top);
        if bx <= click.0 && bx + w >= click.0 && by <= click.1 && by + h >= click.1 && c.y as u32 > best.map_or(0, |b| b.y as u32) {
            best = Some(c);
        }
    }
    best.map(|c| c.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_and_context() {
        let mut m = MiniView::default();
        m.init((40, 50));
        assert_eq!(m.command(0x283), None);
        assert_eq!(m.scale, 10);
        assert_eq!(m.command(0x285), None);
        assert_eq!(m.scale, 48);
        assert_eq!(m.command(FOLLOW), Some(PICK_EVENT));
        let alive = |id: u16| (id == 7).then_some((99, (12, 13)));
        // Nothing followed: the node.
        assert_eq!(m.context(FOLLOW, &alive), Some((40, 50)));
        m.pick((20, 21), &[(19, 20), (21, 21)], Some((7, 99)));
        assert_eq!((m.node, m.follow), ((21, 21), Some((7, 99))));
        assert_eq!(m.context(FOLLOW, &alive), Some((12, 13)));
        assert_eq!(m.centred, Some(7));
        // The entity's uid changed: the stale 0x12938bc still points at id 7.
        m.follow = Some((7, 98));
        assert_eq!(m.context(FOLLOW, &alive), Some((12, 13)));
        // Nothing found: the clicked node, nothing followed.
        m.pick((30, 31), &[(29, 30)], None);
        assert_eq!((m.node, m.follow), ((30, 31), None));
    }

    /// 0x4fdc60 at scale 24 on flat ground (height 0 but at (11, 10): 7 px): the shown node
    /// without an entity, the followed entity's node minus its height, `distance/255` of the way
    /// to the neighbour in its direction, the stored node while it is aboard (follow kept), the
    /// follow dropped where the fog hides it.
    #[test]
    fn camera_point_follows_like_0x4fdc60() {
        let mut m = MiniView::default();
        m.init((10, 10));
        let height = |x: i32, y: i32, s: i32| if (x, y) == (11, 10) { 7 * s / 24 } else { 0 };
        let all = |_: i32, _: i32| true;
        let none = |_: u16| None;
        // (10 − 5)·24 − 87 − 40 = −7; 12·10 − 10 − 95 = 15.
        assert_eq!(m.place(&none, &all, &height), (-7, 15));
        m.follow = Some((5, 77));
        let mut e = Followed { uid: 77, on_board: false, node: (20, 10), walk: Some((0, 0)) };
        let ent = |e: Followed| move |id: u16| (id == 5).then_some(e);
        // (20 − 5)·24 − 127 = 233.
        assert_eq!(m.place(&ent(e), &all, &height), (233, 15));
        assert_eq!(m.node, (20, 10));
        // Half way (128/255) from (20, 10) towards direction 3's (21, 10): +24 · 128/255 = 12.04.
        e.walk = Some((3, 128));
        assert_eq!(m.place(&ent(e), &all, &height), (233 + 12, 15));
        // Towards direction 2's (20, 9): x − 12 · 128/255 → −6 (truncated), y − 12 · 128/255 → −6.
        e.walk = Some((2, 128));
        assert_eq!(m.place(&ent(e), &all, &height), (233 + 6, 15 - 6));
        // Towards (11, 10) from (10, 10) at height 7: y moves 7 · 255/255 up.
        e.node = (10, 10);
        e.walk = Some((3, 255));
        assert_eq!(m.place(&ent(e), &all, &height), (-7 + 24, 15 - 7));
        // Aboard: the stored node, the follow kept.
        e.on_board = true;
        assert_eq!(m.place(&ent(e), &all, &height), (-7, 15));
        assert_eq!(m.follow, Some((5, 77)));
        // Hidden by the fog: dropped.
        e.on_board = false;
        e.node = (20, 10);
        assert_eq!(m.place(&ent(e), &|x, _| x < 15, &height), (233, 15));
        assert_eq!((m.follow, m.node), (None, (20, 10)));
        // Scale 48: twice the distances.
        m.scale = 48;
        m.node = (10, 10);
        assert_eq!(m.place(&none, &all, &height), ((5.0 * 48.0 - 87.0 - 80.0) as i32, (240.0 - 20.0 - 95.0) as i32));
        let v = m.view((73, 125));
        assert_eq!((v.x, v.y, v.zoom), (36.5, 62.5, 2.0));
    }
}
