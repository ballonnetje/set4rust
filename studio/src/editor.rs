//! The slot editor's state: building data and anchors in an undoable working copy that is written
//! to the pack only by Save. No UI here; `ui/editor.rs` draws it and feeds it input.

use std::collections::BTreeMap;

use anyhow::Result;
use serde_json::Value;

use crate::geometry::{Bounds, Extent, cell_px, data_points, is_cell, snap_cell};
use crate::layers::{LayerId, LayerState, layer};
use crate::model::{BLD_KEYS, Building, Group, Pile, Seq, Slot, half_k};
use crate::pack::{Conf, Pack, frame_anchor, frame_path, js_round, num, seq_mult};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Paint,
    Erase,
    Add,
    Anchor,
}

/// `index` -1: a layer with one item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sel {
    pub layer: LayerId,
    pub index: i32,
}

#[derive(Clone, Debug)]
enum Drag {
    Paint { before: Box<Building>, add: bool },
    Move { before: Box<Building>, adding: bool },
    Image { start: [f64; 2], before: Vec<[f64; 2]> },
}

#[derive(Clone, Debug)]
enum Change {
    Building { before: Box<Building>, after: Box<Building> },
    Conf { key: String, before: Conf, after: Conf },
}

#[derive(Clone, Debug)]
struct Entry {
    label: String,
    change: Change,
    /// Interaction it belongs to: edits of one inspector field merge into one step.
    merge: Option<u64>,
}

/// What the editor reads from the outside.
#[derive(Clone, Copy)]
pub struct Scope<'a> {
    pub g: &'a Group,
    pub slot: &'a Slot,
    pub pack: &'a Pack,
    pub layers: &'a [LayerState],
}

pub struct Editor {
    /// Index into the slot's sequences.
    pub seq: usize,
    pub tool: Tool,
    /// Area layer Paint and Erase work on.
    pub paint: LayerId,
    /// What Add places.
    pub add: LayerId,
    pub sel: Option<Sel>,
    /// The building while a drag is under way.
    draft: Option<Building>,
    anchor_draft: Option<Vec<[f64; 2]>>,
    drag: Option<Drag>,
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    pub route: usize,
    pub parts: bool,
    pub this_frame: bool,
    /// The cell under the cursor with Paint or Erase.
    pub hover: Option<[i32; 2]>,
    /// Your frame on show.
    pub frame: usize,
    work_b: Option<Building>,
    work_conf: BTreeMap<String, Conf>,
    /// Bumped by every change of the working copy.
    pub ver: u64,
}

impl Editor {
    pub fn new(g: &Group, slot: &Slot, pack: &Pack) -> Editor {
        let seq = slot.seqs.iter().position(|q| q.key == "1").unwrap_or(0);
        Editor {
            seq,
            tool: Tool::Select,
            paint: LayerId::Footprint,
            add: LayerId::Piles,
            sel: None,
            draft: None,
            anchor_draft: None,
            drag: None,
            undo: Vec::new(),
            redo: Vec::new(),
            route: 0,
            parts: true,
            this_frame: false,
            hover: None,
            frame: 0,
            work_b: pack.saved_building(g, slot).map(|(b, _)| b),
            work_conf: BTreeMap::new(),
            ver: 0,
        }
    }

    /// What the views show: the working copy, or the item being dragged.
    pub fn building(&self) -> Option<&Building> {
        self.draft.as_ref().or(self.work_b.as_ref())
    }

    /// A sequence's settings: the unsaved ones win over the stored.
    pub fn conf<'a>(&'a self, s: Scope<'a>, seq: &'a Seq) -> &'a Conf {
        self.work_conf.get(&seq.key).unwrap_or_else(|| s.pack.conf(s.g, s.slot, seq))
    }

    pub fn mult(&self, s: Scope<'_>, seq: &Seq) -> u32 {
        seq_mult(s.pack, s.slot, seq, self.conf_for(s, seq))
    }

    fn conf_for<'a>(&'a self, s: Scope<'a>, seq: &Seq) -> &'a Conf {
        self.work_conf.get(&seq.key).unwrap_or_else(|| s.pack.conf(s.g, s.slot, seq))
    }

    pub fn anchor(&self, s: Scope<'_>, seq: &Seq, j: usize) -> [f64; 2] {
        frame_anchor(s.pack, s.slot, seq, self.conf_for(s, seq), j)
    }

    pub fn current_seq<'a>(&self, slot: &'a Slot) -> &'a Seq {
        &slot.seqs[self.seq]
    }

    /// The anchors of every frame of yours in the edited sequence.
    pub fn anchors_now(&self, s: Scope<'_>) -> Vec<[f64; 2]> {
        if let Some(a) = &self.anchor_draft {
            return a.clone();
        }
        let seq = self.current_seq(s.slot);
        let n = seq.frames.len() * self.mult(s, seq) as usize;
        (0..n).map(|j| self.anchor(s, seq, j)).collect()
    }

    /// The world box the large view shows.
    pub fn bounds(&self, s: Scope<'_>) -> Bounds {
        let seq = self.current_seq(s.slot);
        let k = self.mult(s, seq) as usize;
        let sc = s.pack.scale();
        let b = self.building();
        let mut e = Extent::new(24.0);
        for fr in &seq.frames {
            let hk = half_k(fr);
            e.add(-fr[3] as f64 * hk, -fr[4] as f64 * hk);
            e.add((fr[1] - fr[3]) as f64 * hk, (fr[2] - fr[4]) as f64 * hk);
        }
        // Parts are drawn at the finished image's top-left (their own left/top are ignored).
        if self.parts && b.is_some() && seq.key == "1" {
            let m = &seq.frames[0];
            let mk = half_k(m);
            for p in s.slot.seqs.iter().filter(|p| p.patch.is_some()) {
                for fr in &p.frames {
                    let hk = half_k(fr);
                    e.add(-m[3] as f64 * mk + fr[1] as f64 * hk, -m[4] as f64 * mk + fr[2] as f64 * hk);
                }
            }
        }
        for j in 0..seq.frames.len() * k {
            let Some(rec) = s.pack.file(&frame_path(s.g, s.slot, seq, j)).filter(|r| r.w > 0) else { continue };
            let [ax, ay] = self.anchor(s, seq, j);
            e.add(-ax / sc, -ay / sc);
            e.add((rec.w as f64 - ax) / sc, (rec.h as f64 - ay) / sc);
        }
        if let Some(b) = b {
            for [x, y] in data_points(b) {
                e.add(x as f64, y as f64);
            }
        }
        // Room to move things outwards; the view pans for more.
        e.pad(36.0)
    }

    /// The item under world point (wx, wy), top layer first.
    pub fn hit(&self, s: Scope<'_>, wx: f64, wy: f64, z: f64) -> Option<Sel> {
        let b = self.building();
        let tol = 7.0 / z;
        let near = |x: f64, y: f64| (x - wx).hypot(y - wy) <= tol.max(4.0);
        let near_cell = |c: [i32; 2]| {
            let p = cell_px(c);
            near(p[0] as f64, p[1] as f64)
        };
        for l in s.layers.iter().filter(|l| l.vis) {
            let def = layer(l.id);
            if l.id == LayerId::Yours {
                let j = self.frame;
                if let Some(rec) = s.pack.file(&frame_path(s.g, s.slot, self.current_seq(s.slot), j)).filter(|r| r.w > 0) {
                    let sc = s.pack.scale();
                    let [ax, ay] = self.anchors_now(s).get(j).copied().unwrap_or([0.0, 0.0]);
                    let (x, y) = (-ax / sc, -ay / sc);
                    if wx >= x && wy >= y && wx <= x + rec.w as f64 / sc && wy <= y + rec.h as f64 / sc {
                        return Some(Sel { layer: l.id, index: -1 });
                    }
                }
            }
            let Some(b) = b.filter(|_| def.bld) else { continue };
            let one = Some(Sel { layer: l.id, index: -1 });
            let many = |i: Option<usize>| i.map(|i| Sel { layer: l.id, index: i as i32 });
            let found = match l.id {
                LayerId::Door => near_cell(b.door).then_some(one).flatten(),
                LayerId::Flag => near_cell(b.flag).then_some(one).flatten(),
                LayerId::Work => b.work.filter(|c| is_cell(*c) && near_cell(*c)).and(one),
                LayerId::Builders => many(b.builders.iter().position(|q| near_cell([q[0], q[1]]))),
                LayerId::Piles => many(b.piles.iter().position(|p| is_cell(p.cell) && near_cell(p.cell))),
                LayerId::SoldiersPx => many(b.soldiers_px.iter().position(|q| near(q[0] as f64, q[1] as f64))),
                LayerId::MiniFlagPx => b.mini_flag_px.filter(|q| near(q[0] as f64, q[1] as f64)).and(one),
                _ => None,
            };
            if found.is_some() {
                return found;
            }
        }
        None
    }

    /// Left button down at a world point. `Err`: what to tell the user.
    pub fn press(&mut self, s: Scope<'_>, wx: f64, wy: f64, z: f64) -> Result<(), String> {
        let building = self.building().cloned();
        match (self.tool, building) {
            (Tool::Anchor, _) => {
                // The clicked pixel of your frame becomes the anchor; the other frames shift with it.
                let j = self.frame;
                let seq = self.current_seq(s.slot);
                if s.pack.file(&frame_path(s.g, s.slot, seq, j)).is_none_or(|r| r.w == 0) {
                    return Err("No frame of yours here to set the anchor on.".to_string());
                }
                let sc = s.pack.scale();
                let before = self.anchors_now(s);
                let [ax, ay] = before[j];
                let px = [js_round((wx + ax / sc) * sc) as f64, js_round((wy + ay / sc) * sc) as f64];
                let d = [px[0] - ax, px[1] - ay];
                let after = self.shifted(&before, d, j);
                self.commit_anchors(s, &before, Some(after), "set anchor");
            }
            (Tool::Paint | Tool::Erase, Some(b)) => {
                let add = self.tool == Tool::Paint;
                let mut draft = b.clone();
                paint_at(&mut draft, self.paint, wx, wy, add);
                self.draft = Some(draft);
                self.drag = Some(Drag::Paint { before: Box::new(b), add });
            }
            (Tool::Add, Some(b)) => {
                let mut draft = b.clone();
                self.sel = Some(add_at(&mut draft, self.add, wx, wy));
                self.draft = Some(draft);
                self.drag = Some(Drag::Move { before: Box::new(b), adding: true });
            }
            (_, b) => {
                // Select: an item, or your image (dragging it moves the anchors).
                self.sel = self.hit(s, wx, wy, z);
                match (self.sel, b) {
                    (Some(sel), _) if sel.layer == LayerId::Yours => {
                        self.drag = Some(Drag::Image { start: [wx, wy], before: self.anchors_now(s) });
                    }
                    (Some(_), Some(b)) => {
                        self.draft = Some(b.clone());
                        self.drag = Some(Drag::Move { before: Box::new(b), adding: false });
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// The pointer moved while the button is held.
    pub fn drag_to(&mut self, s: Scope<'_>, wx: f64, wy: f64) {
        match &self.drag {
            Some(Drag::Paint { add, .. }) => {
                let add = *add;
                let layer = self.paint;
                if let Some(d) = self.draft.as_mut() {
                    paint_at(d, layer, wx, wy, add);
                }
            }
            Some(Drag::Move { .. }) => {
                if let (Some(sel), Some(d)) = (self.sel, self.draft.as_mut()) {
                    move_sel(d, sel, wx, wy);
                }
            }
            Some(Drag::Image { start, before }) => {
                let sc = s.pack.scale();
                let d = [js_round((start[0] - wx) * sc) as f64, js_round((start[1] - wy) * sc) as f64];
                let (before, j) = (before.clone(), self.frame);
                self.anchor_draft = Some(self.shifted(&before, d, j));
            }
            None => {}
        }
    }

    /// The button came up.
    pub fn release(&mut self, s: Scope<'_>) {
        match self.drag.take() {
            Some(Drag::Image { before, .. }) => {
                if let Some(after) = self.anchor_draft.clone() {
                    self.commit_anchors(s, &before, Some(after), "move image");
                }
            }
            Some(Drag::Paint { before, add }) => {
                let label = format!("{} {}", if add { "paint" } else { "erase" }, layer(self.paint).label);
                if let Some(d) = self.draft.take() {
                    self.commit_building(*before, d, &label);
                }
            }
            Some(Drag::Move { before, adding }) => {
                let label = if adding {
                    format!("add {}", layer(self.add).label)
                } else {
                    format!("move {}", layer(self.sel.map_or(LayerId::Door, |s| s.layer)).label)
                };
                if let Some(d) = self.draft.take() {
                    self.commit_building(*before, d, &label);
                }
            }
            None => {}
        }
    }

    fn shifted(&self, before: &[[f64; 2]], d: [f64; 2], j: usize) -> Vec<[f64; 2]> {
        before.iter().enumerate().map(|(i, a)| if self.this_frame && i != j { *a } else { [a[0] + d[0], a[1] + d[1]] }).collect()
    }

    fn push(&mut self, label: &str, change: Change, merge: Option<u64>) {
        if let (Some(m), Some(last)) = (merge, self.undo.last_mut())
            && last.merge == Some(m)
        {
            match (&mut last.change, change) {
                (Change::Building { after, .. }, Change::Building { after: a, .. }) => *after = a,
                (Change::Conf { after, .. }, Change::Conf { after: a, .. }) => *after = a,
                (slot, change) => *slot = change,
            }
        } else {
            self.undo.push(Entry { label: label.to_string(), change, merge });
        }
        self.redo.clear();
        self.ver += 1;
    }

    /// An undoable edit of the building data; nothing is written until Save.
    pub fn commit_building(&mut self, before: Building, after: Building, label: &str) {
        self.commit_building_merged(before, after, label, None);
    }

    fn commit_building_merged(&mut self, before: Building, after: Building, label: &str, merge: Option<u64>) {
        self.draft = None;
        if before == after {
            return;
        }
        self.work_b = Some(after.clone());
        self.push(label, Change::Building { before: Box::new(before), after: Box::new(after) }, merge);
    }

    /// An undoable edit of the building data through `f`, from an inspector field. `merge`: edits
    /// with the same number are one undo step.
    pub fn edit(&mut self, label: &str, merge: Option<u64>, f: impl FnOnce(&mut Building)) {
        let Some(before) = self.building().cloned() else { return };
        let mut after = before.clone();
        f(&mut after);
        self.commit_building_merged(before, after, label, merge);
    }

    /// An undoable edit of the anchors in the edited sequence's settings; `None`: the originals'.
    pub fn commit_anchors(&mut self, s: Scope<'_>, before: &[[f64; 2]], after: Option<Vec<[f64; 2]>>, label: &str) {
        self.commit_anchors_merged(s, before, after, label, None);
    }

    pub fn commit_anchors_merged(&mut self, s: Scope<'_>, before: &[[f64; 2]], after: Option<Vec<[f64; 2]>>, label: &str, merge: Option<u64>) {
        self.anchor_draft = None;
        if after.as_deref() == Some(before) {
            return;
        }
        let seq = self.current_seq(s.slot);
        let prev = self.conf_for(s, seq).clone();
        let mut conf = prev.clone();
        conf.bad = false;
        conf.map.remove("anchor");
        conf.map.remove("anchors");
        let pair = |a: &[f64; 2]| Value::Array(vec![num(a[0]), num(a[1])]);
        if let Some(a) = &after {
            if a.iter().all(|x| *x == a[0]) {
                conf.map.insert("anchor".into(), pair(&a[0]));
            } else {
                conf.map.insert("anchors".into(), Value::Array(a.iter().map(pair).collect()));
            }
        }
        let key = seq.key.clone();
        self.work_conf.insert(key.clone(), conf.clone());
        self.push(label, Change::Conf { key, before: prev, after: conf }, merge);
    }

    /// Undo (`back`) or redo; `Err` says there is nothing, `Ok` what happened.
    pub fn undo_step(&mut self, back: bool) -> Result<String, String> {
        let from = if back { &mut self.undo } else { &mut self.redo };
        let Some(e) = from.pop() else { return Err(if back { "Nothing to undo." } else { "Nothing to redo." }.to_string()) };
        match &e.change {
            Change::Building { before, after } => self.work_b = Some(if back { (**before).clone() } else { (**after).clone() }),
            Change::Conf { key, before, after } => {
                self.work_conf.insert(key.clone(), if back { before.clone() } else { after.clone() });
            }
        }
        let msg = format!("{}: {}", if back { "Undid" } else { "Redid" }, e.label);
        let mut e = e;
        e.merge = None;
        if back { self.redo.push(e) } else { self.undo.push(e) }
        self.sel = None;
        self.ver += 1;
        Ok(msg)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Whether the working copy differs from what is stored.
    pub fn is_dirty(&self, g: &Group, slot: &Slot, pack: &Pack) -> bool {
        let building = slot.building.is_some() && self.work_b != pack.saved_building(g, slot).map(|(b, _)| b);
        building || self.conf_changes(g, slot, pack).next().is_some()
    }

    fn conf_changes<'a>(&'a self, g: &'a Group, slot: &'a Slot, pack: &'a Pack) -> impl Iterator<Item = (&'a Seq, &'a Conf)> {
        self.work_conf.iter().filter_map(move |(k, c)| {
            let q = slot.seqs.iter().find(|q| &q.key == k)?;
            (!c.same(pack.conf(g, slot, q))).then_some((q, c))
        })
    }

    /// Writes the working copy to the pack. `Ok(false)`: nothing to save.
    pub fn save(&mut self, g: &Group, slot: &Slot, pack: &mut Pack) -> Result<bool> {
        if !self.is_dirty(g, slot, pack) {
            return Ok(false);
        }
        if let Some(b) = &self.work_b
            && pack.saved_building(g, slot).map(|(s, _)| s).as_ref() != Some(b)
        {
            pack.put_building(g, slot, b)?;
        }
        let changes: Vec<(String, Conf)> = self.conf_changes(g, slot, pack).map(|(q, c)| (q.key.clone(), c.clone())).collect();
        for (key, mut c) in changes {
            c.bad = false;
            if let Some(q) = slot.seqs.iter().find(|q| q.key == key) {
                pack.put_conf(g, slot, q, &c)?;
            }
        }
        self.work_b = pack.saved_building(g, slot).map(|(b, _)| b);
        self.work_conf.clear();
        self.ver += 1;
        Ok(true)
    }

    /// Drops every unsaved change and the undo history.
    pub fn discard(&mut self, g: &Group, slot: &Slot, pack: &Pack) {
        self.work_b = pack.saved_building(g, slot).map(|(b, _)| b);
        self.work_conf.clear();
        self.undo.clear();
        self.redo.clear();
        self.sel = None;
        self.draft = None;
        self.anchor_draft = None;
        self.drag = None;
        self.ver += 1;
    }

    /// Keys of the shown building data that differ from the original game data.
    pub fn changed_keys(&self, slot: &Slot) -> Vec<&'static str> {
        changed_keys(self.building(), slot)
    }

    /// Deletes the selected item. `Err`: what to tell the user.
    pub fn delete_sel(&mut self) -> Result<(), String> {
        let Some(s) = self.sel else { return Ok(()) };
        if s.layer == LayerId::Yours {
            return Ok(());
        }
        let def = layer(s.layer);
        if matches!(s.layer, LayerId::Door | LayerId::Flag) {
            return Err(format!("A building needs a {}: move it instead.", def.label.to_lowercase()));
        }
        let Some(before) = self.building().cloned() else { return Ok(()) };
        let mut after = before.clone();
        let i = s.index as usize;
        match s.layer {
            LayerId::Work => after.work = None,
            LayerId::MiniFlagPx => after.mini_flag_px = None,
            LayerId::Builders if i < after.builders.len() => {
                after.builders.remove(i);
            }
            LayerId::Piles if i < after.piles.len() => {
                after.piles.remove(i);
            }
            LayerId::SoldiersPx if i < after.soldiers_px.len() => {
                after.soldiers_px.remove(i);
            }
            _ => {}
        }
        self.sel = None;
        self.commit_building(before, after, &format!("delete {}", def.label));
        Ok(())
    }

    /// Moves the selection by arrow keys: cells by one, pixel data and images by `dx`, `dy`.
    pub fn nudge(&mut self, s: Scope<'_>, dx: i32, dy: i32) {
        let Some(sel) = self.sel else { return };
        if sel.layer == LayerId::Yours {
            let before = self.anchors_now(s);
            let after = self.shifted(&before, [-dx as f64, -dy as f64], self.frame);
            self.commit_anchors(s, &before, Some(after), "nudge image");
            return;
        }
        let Some(before) = self.building().cloned() else { return };
        let mut after = before.clone();
        let step = |c: [i32; 2]| [c[0] + dx.signum(), c[1] + dy.signum()];
        let i = sel.index as usize;
        match sel.layer {
            LayerId::Door => after.door = step(after.door),
            LayerId::Flag => after.flag = step(after.flag),
            LayerId::Work => after.work = after.work.map(step),
            LayerId::Builders => {
                let q = after.builders[i];
                let c = step([q[0], q[1]]);
                after.builders[i] = [c[0], c[1], q[2]];
            }
            LayerId::Piles => after.piles[i].cell = step(after.piles[i].cell),
            LayerId::SoldiersPx => {
                let q = &mut after.soldiers_px[i];
                q[0] += dx;
                q[1] += dy;
            }
            LayerId::MiniFlagPx => after.mini_flag_px = after.mini_flag_px.map(|q| [q[0] + dx, q[1] + dy]),
            _ => {}
        }
        self.commit_building(before, after, &format!("nudge {}", layer(sel.layer).label));
    }

    /// `key` back to the original game data (written on Save).
    pub fn reset_key(&mut self, slot: &Slot, key: &str, label: &str) {
        let (Some(b), Some(orig)) = (self.building().cloned(), slot.building.as_ref()) else { return };
        let mut after = b.clone();
        after.reset_key(orig, key);
        self.commit_building(b, after, &format!("reset {label}"));
    }

    pub fn reset_all(&mut self, slot: &Slot) {
        let (Some(b), Some(orig)) = (self.building().cloned(), slot.building.as_ref()) else { return };
        let mut after = b.clone();
        for k in BLD_KEYS {
            after.reset_key(orig, k);
        }
        self.commit_building(b, after, "reset all to original");
    }
}

/// Keys of `b` that differ from the slot's original data.
pub fn changed_keys(b: Option<&Building>, slot: &Slot) -> Vec<&'static str> {
    match (b, slot.building.as_ref()) {
        (Some(b), Some(orig)) => BLD_KEYS.iter().copied().filter(|k| !b.same_key(orig, k)).collect(),
        _ => Vec::new(),
    }
}

/// Item of the draft at the selection, moved to world point (wx, wy).
fn move_sel(b: &mut Building, s: Sel, wx: f64, wy: f64) {
    let c = snap_cell(wx, wy);
    let p = [js_round(wx) as i32, js_round(wy) as i32];
    let i = s.index as usize;
    match s.layer {
        LayerId::Door => b.door = c,
        LayerId::Flag => b.flag = c,
        LayerId::Work => b.work = Some(c),
        LayerId::Builders => b.builders[i] = [c[0], c[1], b.builders[i][2]],
        LayerId::Piles => b.piles[i].cell = c,
        LayerId::SoldiersPx => b.soldiers_px[i] = [p[0], p[1], b.soldiers_px[i][2], b.soldiers_px[i][3]],
        LayerId::MiniFlagPx => b.mini_flag_px = Some(p),
        _ => {}
    }
}

fn add_at(b: &mut Building, l: LayerId, wx: f64, wy: f64) -> Sel {
    let c = snap_cell(wx, wy);
    let p = [js_round(wx) as i32, js_round(wy) as i32];
    let last = |n: usize| Sel { layer: l, index: n as i32 - 1 };
    match l {
        LayerId::Piles => {
            let good = b.piles.last().and_then(|x| x.good.clone());
            b.piles.push(Pile { cell: c, good, px: cell_px(c) });
            last(b.piles.len())
        }
        LayerId::Builders => {
            b.builders.push([c[0], c[1], 0]);
            last(b.builders.len())
        }
        LayerId::SoldiersPx => {
            b.soldiers_px.push([p[0], p[1], 0, 0]);
            last(b.soldiers_px.len())
        }
        LayerId::MiniFlagPx => {
            b.mini_flag_px = Some(p);
            Sel { layer: l, index: -1 }
        }
        LayerId::Door => {
            b.door = c;
            Sel { layer: l, index: -1 }
        }
        LayerId::Flag => {
            b.flag = c;
            Sel { layer: l, index: -1 }
        }
        _ => {
            b.work = Some(c);
            Sel { layer: LayerId::Work, index: -1 }
        }
    }
}

/// Paint (`add`) or erase the cell under (wx, wy) in an area layer.
fn paint_at(b: &mut Building, l: LayerId, wx: f64, wy: f64, add: bool) {
    let c = snap_cell(wx, wy);
    let cells = match l {
        LayerId::Block => &mut b.block,
        LayerId::Dig => &mut b.dig,
        _ => &mut b.footprint,
    };
    let has = cells.contains(&c);
    if add && !has {
        cells.push(c);
    }
    if !add && has {
        cells.retain(|q| *q != c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layers::default_layers;
    use crate::model::{Spec, test_spec};
    use crate::pack::tests::{png_bytes, temp};
    use crate::pack::{bld_path, conf_path};
    use std::path::PathBuf;

    struct Fix {
        dir: PathBuf,
        pack: Pack,
        spec: Spec,
        layers: Vec<LayerState>,
    }

    fn fix(name: &str) -> Fix {
        let dir = temp(name);
        let pack = Pack::open(&dir).unwrap();
        let spec = test_spec(&dir);
        Fix { dir, pack, spec, layers: default_layers() }
    }

    impl Fix {
        fn mill(&self) -> (&Group, &Slot) {
            self.spec.slot(self.spec.find("buildings-roman/BUILDING_MILL").unwrap())
        }

        fn job(&self) -> (&Group, &Slot) {
            self.spec.slot(self.spec.find("settlers-roman/JOB_X").unwrap())
        }

        fn scope<'a>(&'a self, g: &'a Group, slot: &'a Slot) -> Scope<'a> {
            Scope { g, slot, pack: &self.pack, layers: &self.layers }
        }
    }

    impl Drop for Fix {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn starts_on_the_finished_image_with_the_saved_data() {
        let f = fix("e0");
        let (g, slot) = f.mill();
        let ed = Editor::new(g, slot, &f.pack);
        assert_eq!(ed.current_seq(slot).key, "1");
        assert!(!ed.is_dirty(g, slot, &f.pack));
        assert_eq!(ed.building().unwrap(), &**slot.building.as_ref().unwrap());
    }

    #[test]
    fn dragging_a_dot_is_one_undo_step_and_stays_unsaved() {
        let f = fix("e1");
        let (g, slot) = f.mill();
        let s = f.scope(g, slot);
        let mut ed = Editor::new(g, slot, &f.pack);
        // The door lies at cell (0, 2) = (-24, 24) px.
        ed.press(s, -24.0, 24.0, 2.0).unwrap();
        assert_eq!(ed.sel, Some(Sel { layer: LayerId::Door, index: -1 }));
        ed.drag_to(s, 0.0, 24.0);
        ed.drag_to(s, 24.0, 24.0);
        assert!(ed.building().unwrap().door != slot.building.as_ref().unwrap().door, "the view follows the drag");
        ed.release(s);
        let want = snap_cell(24.0, 24.0);
        assert_eq!(ed.building().unwrap().door, want);
        assert!(ed.is_dirty(g, slot, &f.pack));
        assert!(!f.pack.has(&bld_path(g, slot)), "nothing is written before Save");
        assert_eq!(ed.undo_step(true).unwrap(), "Undid: move Door");
        assert!(!ed.is_dirty(g, slot, &f.pack));
        assert_eq!(ed.undo_step(false).unwrap(), "Redid: move Door");
        assert_eq!(ed.building().unwrap().door, want);
        assert_eq!(ed.undo_step(false).unwrap_err(), "Nothing to redo.");
    }

    #[test]
    fn a_click_without_moving_changes_nothing() {
        let f = fix("e2");
        let (g, slot) = f.mill();
        let s = f.scope(g, slot);
        let mut ed = Editor::new(g, slot, &f.pack);
        ed.press(s, -24.0, 24.0, 2.0).unwrap();
        ed.release(s);
        assert!(!ed.can_undo());
        assert_eq!(ed.undo_step(true).unwrap_err(), "Nothing to undo.");
    }

    #[test]
    fn paint_and_erase_cells() {
        let f = fix("e3");
        let (g, slot) = f.mill();
        let s = f.scope(g, slot);
        let mut ed = Editor::new(g, slot, &f.pack);
        ed.tool = Tool::Paint;
        ed.press(s, 48.0, 0.0, 2.0).unwrap();
        ed.drag_to(s, 72.0, 0.0);
        ed.release(s);
        assert_eq!(ed.building().unwrap().footprint, vec![[0, 0], [1, 0], [2, 0], [3, 0]]);
        ed.tool = Tool::Erase;
        ed.paint = LayerId::Block;
        ed.press(s, -12.0, 12.0, 2.0).unwrap();
        ed.release(s);
        assert!(ed.building().unwrap().block.is_empty());
        assert_eq!(ed.undo_step(true).unwrap(), "Undid: erase Blocked");
        assert_eq!(ed.undo_step(true).unwrap(), "Undid: paint Building area");
        assert!(!ed.is_dirty(g, slot, &f.pack));
    }

    #[test]
    fn add_places_items_and_moves_singles() {
        let f = fix("e4");
        let (g, slot) = f.mill();
        let s = f.scope(g, slot);
        let mut ed = Editor::new(g, slot, &f.pack);
        ed.tool = Tool::Add;
        ed.add = LayerId::Piles;
        ed.press(s, 96.0, 0.0, 2.0).unwrap();
        ed.release(s);
        let b = ed.building().unwrap();
        assert_eq!(b.piles[1], Pile { cell: [4, 0], good: Some("GOOD_PLANK".into()), px: [96, 0] });
        assert_eq!(ed.sel, Some(Sel { layer: LayerId::Piles, index: 1 }));
        ed.add = LayerId::SoldiersPx;
        ed.press(s, 3.4, 5.5, 2.0).unwrap();
        ed.release(s);
        assert_eq!(ed.building().unwrap().soldiers_px[1], [3, 6, 0, 0]);
        ed.add = LayerId::Work;
        ed.press(s, 24.0, 0.0, 2.0).unwrap();
        ed.release(s);
        assert_eq!(ed.building().unwrap().work, Some([1, 0]));
    }

    #[test]
    fn delete_refuses_door_and_flag() {
        let f = fix("e5");
        let (g, slot) = f.mill();
        let mut ed = Editor::new(g, slot, &f.pack);
        ed.sel = Some(Sel { layer: LayerId::Door, index: -1 });
        assert_eq!(ed.delete_sel().unwrap_err(), "A building needs a door: move it instead.");
        ed.sel = Some(Sel { layer: LayerId::Piles, index: 0 });
        ed.delete_sel().unwrap();
        assert!(ed.building().unwrap().piles.is_empty());
        assert_eq!(ed.sel, None);
        ed.sel = Some(Sel { layer: LayerId::MiniFlagPx, index: -1 });
        ed.delete_sel().unwrap();
        assert_eq!(ed.building().unwrap().mini_flag_px, None);
    }

    #[test]
    fn nudge_moves_cells_by_one_and_pixels_by_the_step() {
        let f = fix("e6");
        let (g, slot) = f.mill();
        let s = f.scope(g, slot);
        let mut ed = Editor::new(g, slot, &f.pack);
        ed.sel = Some(Sel { layer: LayerId::Door, index: -1 });
        ed.nudge(s, 10, 0);
        assert_eq!(ed.building().unwrap().door, [1, 2]);
        ed.sel = Some(Sel { layer: LayerId::SoldiersPx, index: 0 });
        ed.nudge(s, -10, 1);
        assert_eq!(ed.building().unwrap().soldiers_px[0], [-7, 5, 1, 1]);
        ed.sel = Some(Sel { layer: LayerId::Builders, index: 0 });
        ed.nudge(s, 0, -1);
        assert_eq!(ed.building().unwrap().builders[0], [2, 1, 1]);
    }

    #[test]
    fn save_writes_only_the_changes_and_reset_deletes_the_file() {
        let mut f = fix("e7");
        let spec = test_spec(&f.dir);
        let (g, slot) = spec.slot(spec.find("buildings-roman/BUILDING_MILL").unwrap());
        let mut ed = Editor::new(g, slot, &f.pack);
        assert!(!ed.save(g, slot, &mut f.pack).unwrap(), "nothing to save");
        ed.edit("flag", None, |b| b.flag = [7, 7]);
        assert_eq!(ed.changed_keys(slot), vec!["flag"]);
        assert!(ed.save(g, slot, &mut f.pack).unwrap());
        assert_eq!(f.pack.file(&bld_path(g, slot)).unwrap().text.as_deref(), Some(r#"{"flag":[7,7]}"#));
        assert!(!ed.is_dirty(g, slot, &f.pack));
        ed.reset_all(slot);
        assert!(ed.is_dirty(g, slot, &f.pack));
        ed.save(g, slot, &mut f.pack).unwrap();
        assert!(!f.pack.has(&bld_path(g, slot)));
    }

    #[test]
    fn discard_drops_the_working_copy_and_history() {
        let f = fix("e8");
        let (g, slot) = f.mill();
        let mut ed = Editor::new(g, slot, &f.pack);
        ed.edit("flag", None, |b| b.flag = [7, 7]);
        ed.discard(g, slot, &f.pack);
        assert!(!ed.is_dirty(g, slot, &f.pack) && !ed.can_undo());
    }

    #[test]
    fn set_anchor_shifts_all_frames_or_this_one() {
        let mut f = fix("e9");
        let spec = test_spec(&f.dir);
        let (g, slot) = spec.slot(spec.find("settlers-roman/JOB_X").unwrap());
        let q = &slot.seqs[1];
        for i in 0..4 {
            f.pack.put(&frame_path(g, slot, q, i), &png_bytes(16, 24)).unwrap();
        }
        let layers = default_layers();
        let s = Scope { g, slot, pack: &f.pack, layers: &layers };
        let mut ed = Editor::new(g, slot, &f.pack);
        ed.seq = 1;
        ed.tool = Tool::Anchor;
        assert_eq!(ed.anchors_now(s)[0], [8.0, 20.0]);
        // World (2, 3) is your pixel (10, 23), which becomes the anchor of every frame.
        ed.press(s, 2.0, 3.0, 2.0).unwrap();
        assert_eq!(ed.anchors_now(s), vec![[10.0, 23.0]; 4]);
        assert_eq!(ed.conf(s, q).map.get("anchor").unwrap().to_string(), "[10,23]");
        assert!(ed.is_dirty(g, slot, &f.pack));
        ed.this_frame = true;
        ed.frame = 2;
        ed.press(s, 0.0, 0.0, 2.0).unwrap();
        let a = ed.anchors_now(s);
        assert_eq!((a[2], a[0]), ([10.0, 23.0], [10.0, 23.0]), "the anchor pixel stays");
        ed.press(s, 1.0, 0.0, 2.0).unwrap();
        let a = ed.anchors_now(s);
        assert_eq!((a[2], a[0]), ([11.0, 23.0], [10.0, 23.0]));
        assert!(ed.conf(s, q).map.contains_key("anchors") && !ed.conf(s, q).map.contains_key("anchor"));
        ed.save(g, slot, &mut f.pack).unwrap();
        assert_eq!(f.pack.file(&conf_path(g, slot, q)).unwrap().text.as_deref(), Some(r#"{"anchors":[[10,23],[10,23],[11,23],[10,23]]}"#));
    }

    #[test]
    fn no_frame_no_anchor_tool() {
        let f = fix("e10");
        let (g, slot) = f.job();
        let s = f.scope(g, slot);
        let mut ed = Editor::new(g, slot, &f.pack);
        ed.tool = Tool::Anchor;
        assert_eq!(ed.press(s, 0.0, 0.0, 2.0).unwrap_err(), "No frame of yours here to set the anchor on.");
    }

    #[test]
    fn dragging_your_image_moves_the_anchors_and_undo_restores_the_settings() {
        let mut f = fix("e11");
        let spec = test_spec(&f.dir);
        let (g, slot) = spec.slot(spec.find("settlers-roman/JOB_X").unwrap());
        let q = &slot.seqs[0];
        f.pack.put(&frame_path(g, slot, q, 0), &png_bytes(16, 24)).unwrap();
        f.pack.put(&conf_path(g, slot, q), br#"{"frame_multiplier":1,"note":"keep"}"#).unwrap();
        let layers = default_layers();
        let s = Scope { g, slot, pack: &f.pack, layers: &layers };
        let mut ed = Editor::new(g, slot, &f.pack);
        ed.seq = 0;
        ed.press(s, 0.0, -5.0, 1.0).unwrap();
        assert_eq!(ed.sel, Some(Sel { layer: LayerId::Yours, index: -1 }));
        ed.drag_to(s, 3.0, -5.0);
        assert_eq!(ed.anchors_now(s)[0], [5.0, 20.0], "the image moves right: the anchor moves left");
        ed.release(s);
        let c = ed.conf(s, q);
        assert_eq!(c.map.get("anchor").unwrap().to_string(), "[5,20]");
        assert_eq!(c.map.get("note").and_then(Value::as_str), Some("keep"));
        ed.undo_step(true).unwrap();
        assert!(!ed.is_dirty(g, slot, &f.pack));
        // Back to the originals.
        ed.commit_anchors(s, &[[1.0, 1.0]], None, "original anchors");
        assert!(!ed.conf(s, q).has_anchor());
    }

    #[test]
    fn inspector_edits_of_one_field_are_one_step() {
        let f = fix("e12");
        let (g, slot) = f.mill();
        let mut ed = Editor::new(g, slot, &f.pack);
        ed.edit("x", Some(1), |b| b.flag = [1, 1]);
        ed.edit("x", Some(1), |b| b.flag = [2, 2]);
        ed.edit("y", Some(2), |b| b.flag = [3, 3]);
        ed.undo_step(true).unwrap();
        assert_eq!(ed.building().unwrap().flag, [2, 2]);
        ed.undo_step(true).unwrap();
        assert_eq!(ed.building().unwrap().flag, slot.building.as_ref().unwrap().flag);
        assert!(!ed.can_undo());
    }

    #[test]
    fn hit_respects_hidden_layers_and_order() {
        let mut f = fix("e13");
        for l in f.layers.iter_mut() {
            if l.id == LayerId::Door {
                l.vis = false;
            }
        }
        let (g, slot) = f.mill();
        let s = f.scope(g, slot);
        let ed = Editor::new(g, slot, &f.pack);
        assert_eq!(ed.hit(s, -24.0, 24.0, 2.0), None);
        assert_eq!(ed.hit(s, 48.0, 0.0, 2.0), Some(Sel { layer: LayerId::Piles, index: 0 }));
        assert_eq!(ed.hit(s, 3.0, 4.0, 2.0), Some(Sel { layer: LayerId::SoldiersPx, index: 0 }));
    }

    #[test]
    fn bounds_cover_parts_and_data_with_room() {
        let f = fix("e14");
        let (g, slot) = f.mill();
        let s = f.scope(g, slot);
        let ed = Editor::new(g, slot, &f.pack);
        let b = ed.bounds(s);
        assert!(b.x0 <= -24.0 - 36.0 && b.y0 <= -24.0 - 36.0, "{b:?}");
    }
}
