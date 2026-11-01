//! The scheduler's evaluation helpers (0x6528d0..0x654a90): place scores for the planner, the
//! resource value of a spot, the production chain of a type with its bottleneck, the stock of a
//! good and where a good is produced.

use super::board::Record;
use super::engine::{Data, Engine, rec};
use super::manager::Manager;
use super::places::Place;
use super::tasks::{self, cell, dist, is_border};
use crate::sfloat::Sf;

/// 0x64cc00 `(good)`: the building type of the first record (table order) producing the good
/// (record word 8); 0 for none or good 0.
pub fn producer(good: i32) -> i32 {
    if good == 0 {
        return 0;
    }
    let rows = &super::tables::RECORDS;
    let mut k = 1;
    while k < rows.len() && rows[k][1] != 0 {
        if rows[k][8] == good {
            return rows[k][1];
        }
        k += 1;
    }
    0
}

/// The first resource task of a resource (0x6518c0).
fn resource_task(e: &Engine, res: i32) -> Option<usize> {
    e.s.list.iter().position(|x| matches!(&x.d, Data::Resource(r) if r.res == res))
}

/// 0x654210 `(t, pos, _, scale)`: how much of the type's resource lies at a spot. With `scale`
/// ≠ 1 the position is a cell: x = x·scale + scale/2, and y the same of the new x (release).
/// * Woodcutters / stonecutters (1, 4, 6, 7): the resource task's scored cells within 20 of the
///   spot, each value·20 − distance; none: the square's resource sum (slot 0xb0).
/// * Mines (0xe..0x12): the resource over spiral radius 3.
/// * Planters (0xb, 0x23, 0x24, 0x26): per scored cell value·10, less the distance beyond the
///   record's word 2; the last cell's value counts (release).
/// * Fishers (5): 1000 on a cell with water (slot 0xe4 with the node coordinates).
pub fn resource_score(e: &Engine, m: &mut dyn Manager, t: i32, pos: i32, scale: i32) -> i32 {
    let race = e.s.race;
    let res = rec(race, t, 10);
    if res == -1 {
        return 0;
    }
    let mut x = pos & 0xffff;
    let mut y = pos >> 16;
    if scale != 1 {
        let half = scale / 2;
        x = x * scale + half;
        y = x * scale + half;
    }
    let near = |e: &Engine| -> Vec<(i32, i32, i32)> {
        resource_task(e, res)
            .and_then(|k| match &e.s.list[k].d {
                Data::Resource(r) => r.filter.as_ref().map(|f| f.entries().iter().map(|c| (c.cx, c.cy, c.value)).collect()),
                _ => None,
            })
            .unwrap_or_default()
    };
    match t {
        1 | 4 | 6 | 7 => {
            let mut sum = 0;
            for (cx, cy, v) in near(e) {
                let d = crate::spiral::grid_distance(cx * 16 - x + 8, cy * 16 - y + 8);
                if d < 20 {
                    let w = v * 20 - d;
                    if w > 0 {
                        sum += w;
                    }
                }
            }
            if sum != 0 {
                return sum;
            }
            m.resource_in_square(res, cell(x), cell(y))
        }
        0xe..=0x12 => {
            let n = m.spiral_end(3);
            let mut sum = 0;
            for k in 0..n {
                let (dx, dy) = m.spiral_offset(k);
                sum += m.resource_at(res, x + dx, y + dy);
            }
            sum
        }
        0xb | 0x23 | 0x24 | 0x26 => {
            if resource_task(e, res).is_none() {
                return 0;
            }
            let lim = rec(race, t, 2);
            let mut last = 0;
            for (cx, cy, v) in near(e) {
                let base = v * 10;
                let d = crate::spiral::grid_distance(cx * 16 - x + 8, cy * 16 - y + 8);
                last = if d <= lim { base } else { base - d };
            }
            last
        }
        5 if m.cell_bit(0xe4, x, y) => 1000,
        _ => 0,
    }
}

/// 0x658310 `(x, y)` of a resource task: a scored cell with cx ≥ x and cy ≥ y (release: the
/// differences are compared with `< 1`).
fn filter_has_beyond(f: &super::filter::Filter, x: i32, y: i32) -> bool {
    f.entries().iter().any(|c| x - c.cx < 1 && y - c.cy < 1)
}

/// 0x654530 `(t, x, y)`: the crowding of a cell (taken as a cell even when nodes are passed):
/// water around (2, not for fishers 5 / 6 and castles 0x2e), a border cell (+10), ground class 2
/// (+30), then per resource task with a scored cell at or beyond it one more, and more by its
/// resource and the type.
pub fn crowding(e: &Engine, m: &mut dyn Manager, t: i32, x: i32, y: i32) -> i32 {
    let mut v = 0;
    if !((5..=6).contains(&t) || t == 0x2e) {
        if m.cell_bit(0xe8, x, y) {
            v = 2;
        }
        if m.cell_bit(0xe0, x, y) {
            v = 2;
        }
        if is_border(e, x, y) {
            v += 10;
        }
    }
    if m.cell_class(2, x, y) > 0 {
        v += 0x1e;
    }
    for en in &e.s.list {
        let snap = v;
        let Data::Resource(r) = &en.d else { continue };
        let Some(f) = &r.filter else { continue };
        if !filter_has_beyond(f, x, y) {
            continue;
        }
        v += 1;
        match r.res {
            0x70 => {
                if (2..=4).contains(&t) || t == 6 {
                    v += 1;
                }
            }
            0x60 => {
                if t == 4 {
                    v = snap;
                }
                v += 1;
            }
            0 => {
                if !(5..=6).contains(&t) {
                    v = snap + 2;
                }
            }
            0x10 | 0x20 => {
                v = snap + if matches!(t, 0xf | 0x11 | 0x14 | 0x15 | 0x16) { 1 } else { 3 };
            }
            0x30 | 0x40 if t == 0xf || t == 0x11 => v = snap + 2,
            _ => {}
        }
    }
    v
}

/// 0x653cd0 `(t, _, pos, near, far, flag, crowd)`: the score of a building of type `t` at node
/// `pos`:
/// * resource users (1, 4, 6, 7, 0xb, 0xe..0x12, 0x23, 0x24, 0x26, 0x52): 10 · (resource −
///   crowding of its cell);
/// * the others of the chains (2, 3, 8..0xa, 0xc, 0x13..0x16, 0x19, 0x25, 0x27, 0x2b, 0x51):
///   10 · (200 − 3 · crowding (of the node as a cell) − `crowd`) + 10 · (100 − distance to
///   `near`); with `far` > 0, 200 per step the place is nearer to `far` than `near` is, and with
///   `flag` 10 · (100 − distance to `far`);
/// * fishers (5): by the water of its cell (60 / 80 − the water node's distance) · 25 − the
///   distance to `near`;
/// * castles / towers (0x2e, 0x2f): 1000 − 3 · `crowd`, −1000 per own castle nearer than 20,
///   −5 · the distance to `far`;
/// * residences, 0x18, 0x2c: 1000 − 80 · `crowd`.
#[allow(clippy::too_many_arguments)]
pub fn place_score(e: &Engine, m: &mut dyn Manager, t: i32, pos: i32, near: i32, far: i32, flag: bool, crowd: i32) -> i32 {
    let hd = |a: i32, b: i32| dist(a, b);
    match t {
        1 | 4 | 6 | 7 | 0xb | 0xe..=0x12 | 0x23 | 0x24 | 0x26 | 0x52 => {
            let cx = (pos >> 4) & 0xfff;
            let cy = cell(pos >> 16);
            let r = resource_score(e, m, t, pos, 1);
            let c = crowding(e, m, t, cx, cy);
            r * 10 - c * 10
        }
        2 | 3 | 8..=0xa | 0xc | 0x13..=0x16 | 0x19 | 0x25 | 0x27 | 0x2b | 0x51 => {
            let (x, y) = (pos & 0xffff, pos >> 16);
            let c = crowding(e, m, t, x, y);
            let mut v = (200 - 3 * c - crowd) * 10;
            v += (100 - hd(near, pos)) * 10;
            if far > 0 {
                let d2 = hd(far, pos);
                let d3 = hd(far, near);
                if d3 < d2 {
                    v += (d3 - d2) * 200;
                }
                if flag {
                    v += (100 - d2) * 10;
                }
            }
            v
        }
        5 => {
            let (x, y) = (pos & 0xffff, pos >> 16);
            let (nx, ny) = (near & 0xffff, near >> 16);
            let (cx, cy) = (x >> 4, cell(y));
            let w = if m.cell_bit(0xe4, cx, cy) {
                let wn = m.water_node(cx, cy);
                hd(pos, wn)
            } else {
                0x19
            };
            let d = crate::spiral::grid_distance(nx - x, ny - y);
            let mut v = 0;
            if m.cell_bit(0xe8, cx, cy) {
                v = (60 - w) * 25 - d;
            }
            if m.cell_bit(0xe4, cx, cy) {
                v = (80 - w) * 25 - d;
            }
            v
        }
        0x2e | 0x2f => {
            let (x, y) = (pos & 0xffff, pos >> 16);
            let mut v = 1000 - 3 * crowd;
            let mut ids = Vec::new();
            m.buildings(t, &mut ids);
            let (mut bx, mut by) = (0, 0);
            for &id in &ids {
                if let Some(p) = m.building_pos(id) {
                    (bx, by) = p;
                }
                let d = crate::spiral::grid_distance(x - bx, y - by);
                if d < 20 {
                    v -= 1000;
                }
            }
            let (fx, fy) = (far & 0xffff, far >> 16);
            v - 5 * crate::spiral::grid_distance(fx - x, fy - y)
        }
        0x18 | 0x28..=0x2a | 0x2c => 1000 - 80 * crowd,
        _ => 0,
    }
}

/// 0x64fd50 `(manager, t, pos, records)`: no record's building would overlap (slot 0x28) one of
/// type `t` at `pos`, and no record takes the 8×8 block of `pos`.
fn free_spot(m: &mut dyn Manager, t: i32, pos: i32, recs: &[Record]) -> bool {
    for r in recs {
        if r.a == 0 {
            break;
        }
        if m.overlap(r.a, r.d, t, pos) {
            return false;
        }
        if r.block != 0 && (r.block ^ pos) & 0xfff8 == 0 && cell8(pos >> 16) == cell8(r.block >> 16) {
            return false;
        }
    }
    true
}

fn cell8(v: i32) -> i32 {
    v / 8
}

/// 0x653bc0 `(t, place, records, near, far, flag)`: the best free node of a place (every
/// (n/20 + 1)-th of its nodes) by [`place_score`] above 0: (score, node index). Fishers measure
/// against the water node of the place.
#[allow(clippy::too_many_arguments)]
pub fn best_node(e: &Engine, m: &mut dyn Manager, t: i32, place: &Place, recs: &[Record], near: i32, far: i32, flag: bool) -> Option<(i32, usize)> {
    let n = place.nodes.len() as i32;
    let step = n / 20;
    let mut far = far;
    if t == 5 {
        far = m.water_node(place.x, place.y);
    }
    let mut best = 0;
    let mut idx = 0;
    let mut found = false;
    let mut i = 0;
    while i < n {
        let (pos, class) = place.nodes[i as usize];
        let pos = pos as i32;
        if free_spot(m, t, pos, recs) {
            let s = place_score(e, m, t, pos, near, far, flag, class as i8 as i32);
            if s > best {
                best = s;
                idx = i as usize;
                found = true;
            }
        }
        i += step + 1;
    }
    found.then_some((best, idx))
}

/// 0x654770: the cells within two of a border cell (−2..+1) that are not own land, the best five
/// `cy << 16 | cx`. Release quirk: the score is a running sum over the border cell's block (reset
/// per border cell, 0x654840), each cell adding its resources 0x10 + 0x30 + 0x20, 20 for water,
/// 0x60, 0x50.
pub fn combined_cells(e: &Engine, m: &mut dyn Manager) -> Vec<i32> {
    let mut f = super::filter::Filter::new(5);
    let border = e.s.list.iter().find_map(|x| match &x.d {
        Data::ExpandMgr(b) => Some(b.border.clone()),
        _ => None,
    });
    for w in border.unwrap_or_default() {
        let (bx, by) = (w & 0xffff, w >> 16);
        let mut v = 0;
        for gx in bx - 2..bx + 2 {
            for gy in by - 2..by + 2 {
                if m.own_node(gx * 16 + 8, gy * 16 + 8) {
                    continue;
                }
                v += m.resource_at(0x10, gx, gy);
                v += m.resource_at(0x30, gx, gy);
                v += m.resource_at(0x20, gx, gy);
                v += if m.cell_bit(0xe4, gx, gy) { 20 } else { 0 };
                v += m.resource_at(0x60, gx, gy);
                v += m.resource_at(0x50, gx, gy);
                f.insert(super::filter::Entry::resource(gx, gy, v));
            }
        }
    }
    f.entries().iter().map(|c| (c.cy << 16) + c.cx).collect()
}

/// One member of a production cluster (16 bytes): building, status, type, node.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Member {
    pub building: i32,
    pub status: i32,
    pub kind: i32,
    pub pos: i32,
}

/// A node of the production tree (0x200 bytes on the stack): +0x1e0 members, +0x1f0 type,
/// +0x1f4 the other node of the same type (7 none), +0x1f8 value sum, +0x1fc capacity.
#[derive(Clone, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct TreeNode {
    kind: i32,
    link: usize,
    sum: i32,
    cap: i32,
    members: Vec<Member>,
}

/// 0x6530b0 `(nodes, _, good, total)`: from the leaves up, each node's capacity: its value sum,
/// shared by ratio with its twin (float), capped by its inputs'; the node with the largest cut
/// (or, first, a node without production and twin) is the bottleneck. The capacities of the
/// nodes making `good` add to `total`.
fn bottleneck(nodes: &mut [TreeNode; 7], race: i32, good: i32, total: &mut i32) -> usize {
    let mut best = 0;
    let mut result = 0;
    let mut cut = 0;
    let parent = |k: usize| if k == 0 { 0 } else { (k - 1) / 2 };
    for k in (0..7).rev() {
        if nodes[k].kind == 0 {
            continue;
        }
        let sum = nodes[k].sum;
        let link = nodes[k].link;
        if link == 7 {
            nodes[k].cap = sum;
        } else {
            let mut ratio = Sf::ONE;
            let tot = nodes[parent(link)].sum + nodes[parent(k)].sum;
            nodes[k].cap = 0;
            if tot > 0 {
                ratio = Sf::from_i32(nodes[parent(k)].sum) / Sf::from_i32(tot);
            }
            nodes[k].cap = (Sf::from_i32(sum) * ratio).to_i32_trunc();
        }
        if sum == 0 && link == 7 {
            return k;
        }
        if k < 3 {
            for c in [2 * k + 1, 2 * k + 2] {
                if nodes[c].kind == 0 {
                    continue;
                }
                if nodes[c].cap < nodes[k].cap {
                    cut = sum - nodes[c].cap;
                    nodes[k].cap = nodes[c].cap;
                }
                if cut > best {
                    best = cut;
                    result = c;
                    cut = 0;
                }
            }
        }
        if good != 0 && rec(race, nodes[k].kind, 8) == good {
            *total += nodes[k].cap;
        }
    }
    result
}

/// 0x6528d0 `(t, out, good, total)`: the production clusters of type `t`: the tree of `t` and
/// its input producers (binary, 7 nodes: children 2k+1 / 2k+2 from the record's input words 6 /
/// 7; type 5 counts as none), then, while productivity controls are left, a cluster of them
/// (each within the record's word 2 of a member already taken) is summed into the tree and its
/// bottleneck found (0x6530b0); per cluster a member goes to `out`: at the bottleneck's parent
/// members' mean (status 3) or, without, the cluster's mean (its first member's building and
/// status). False when a round finds no controls at all.
pub fn production_tree(e: &mut Engine, t: i32, out: &mut Vec<Member>, good: i32, total: &mut i32) -> bool {
    *total = 0;
    let race = e.s.race;
    for x in &mut e.s.list {
        if x.state != 3
            && let Data::Productivity(p) = &mut x.d
        {
            p.clustered = false;
        }
    }
    let mut nodes: [TreeNode; 7] = Default::default();
    for n in nodes.iter_mut() {
        n.kind = -1; // [inf] not yet set: the release reads stack memory
    }
    nodes[0] = TreeNode { kind: t, link: 7, ..TreeNode::default() };
    for k in 1..7 {
        let p = (k - 1) / 2;
        let w = if (k - 1) & 1 == 0 { 6 } else { 7 };
        let mut nt = producer(rec(race, nodes[p].kind, w));
        if nt == 5 {
            nt = 0;
        }
        nodes[k].kind = nt;
        let j = nodes.iter().position(|n| n.kind == nt).unwrap_or(7);
        nodes[k].link = 7;
        if j != k {
            nodes[k].link = j;
            if j < 7 {
                nodes[j].link = k;
            }
        }
        nodes[k].cap = 0;
        nodes[k].sum = 0;
        nodes[k].members.clear();
    }
    let lim = rec(race, t, 2);
    loop {
        let mut more = false;
        let mut cluster: Vec<Member> = Vec::new();
        loop {
            let mut found = false;
            let n = e.s.list.len();
            for i in 0..n {
                let x = &e.s.list[i];
                if x.state == 3 {
                    continue;
                }
                let Data::Productivity(p) = &x.d else { continue };
                if p.clustered {
                    continue;
                }
                let Some(j) = nodes.iter().position(|nd| nd.kind == p.kind) else { continue };
                let m = Member { building: p.building, status: p.status, kind: p.kind, pos: p.pos };
                if !cluster.is_empty() && !cluster.iter().any(|v| dist(m.pos, v.pos) < lim) {
                    more = true;
                    continue;
                }
                cluster.push(m);
                if let Data::Productivity(p) = &mut e.s.list[i].d {
                    p.clustered = true;
                }
                let value = tasks::prod_value(e, i);
                nodes[j].sum += value;
                let l = nodes[j].link;
                if l != 7 {
                    nodes[l].sum += value;
                    nodes[l].members.push(m);
                }
                nodes[j].members.push(m);
                found = true;
            }
            if !found {
                break;
            }
        }
        if cluster.is_empty() {
            return false;
        }
        let r = bottleneck(&mut nodes, race, good, total);
        let mean = |list: &[Member]| -> Option<i32> {
            let (mut sx, mut sy) = (0, 0);
            for v in list {
                sx += v.pos & 0xffff;
                sy += v.pos >> 16;
            }
            let n = list.len() as i32;
            (n > 0).then(|| ((sy / n) << 16) + sx / n)
        };
        let at = if r != 0 {
            mean(&nodes[(r - 1) / 2].members)
        } else {
            let mut both = nodes[1].members.clone();
            both.extend_from_slice(&nodes[2].members);
            mean(&both)
        };
        match at {
            Some(pos) => out.push(Member { building: 0, status: 3, kind: nodes[r].kind, pos }),
            None => {
                if let Some(pos) = mean(&cluster) {
                    out.push(Member { building: cluster[0].building, status: cluster[0].status, kind: nodes[r].kind, pos });
                }
            }
        }
        for n in nodes.iter_mut() {
            n.members.clear();
            n.sum = 0;
            n.cap = 0;
        }
        if !more {
            break;
        }
    }
    !out.is_empty()
}

/// 0x653250 `(good)`: the capacity of the producers of a good over all clusters (0x6528d0's
/// total).
pub fn stock(e: &mut Engine, good: i32) -> i32 {
    let t = producer(good);
    let mut out = Vec::new();
    let mut total = 0;
    production_tree(e, t, &mut out, good, &mut total);
    total
}

/// 0x654a90 `(good)`: the mean node of up to two productivity controls of the good's maker
/// (9 → 0x11, 0x18 → 0xf, 0xb → 0x10, 0x16 → 1, 7 → 3; others 0). A control whose building
/// is no longer the eco-sector's keeps the previous position (0 at first [inf]).
pub fn production_pos(e: &Engine, m: &mut dyn Manager, good: i32) -> i32 {
    let t = match good {
        9 => 0x11,
        0x18 => 0xf,
        0xb => 0x10,
        0x16 => 1,
        7 => 3,
        _ => return 0,
    };
    let list: Vec<(i32, i32)> = e
        .s
        .list
        .iter()
        .filter_map(|x| match &x.d {
            Data::Productivity(p) if x.state != 3 && p.kind == t => Some((p.building, p.status)),
            _ => None,
        })
        .collect();
    if list.is_empty() || list[0].0 == 0 {
        return 0;
    }
    let (mut x, mut y) = (0, 0);
    let (mut sx, mut sy, mut n) = (0, 0, 0);
    for &(id, status) in list.iter().take(2) {
        if id == 0 {
            break;
        }
        if m.own_building(id) {
            if status == 1 {
                if let Some(p) = tasks::plan_pos(e, id) {
                    (x, y) = p;
                }
            } else {
                (x, y) = m.building_pos(id).unwrap_or((0, 0));
            }
        }
        sx += x;
        sy += y;
        n += 1;
    }
    ((sy / n) << 16) + sx / n
}
