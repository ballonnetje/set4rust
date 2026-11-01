//! The engine's tasks and building events (release 0x648d70..0x64ae20, 0x655510..0x658380): the
//! manager's events, the dispatch of the entries' virtuals, and every class except the problem
//! solving ([`super::problems`]). Offsets are the release's object layout.

use super::engine::{Data, Engine, rec};
use super::manager::Manager;
use super::filter::{Entry as FEntry, Filter};
use super::problems;
use crate::spiral::grid_distance;

/// Hex distance between two packed positions (`y << 16 | x`).
pub fn dist(a: i32, b: i32) -> i32 {
    grid_distance((a & 0xffff) - (b & 0xffff), (a >> 16) - (b >> 16))
}

/// `x / 16` as the release rounds it (`cdq; and 0xf; add; sar 4`: towards zero).
pub fn cell(v: i32) -> i32 {
    v / 16
}

// ---- the classes' fields ----------------------------------------------------------------------

/// `CPeopleManagerTask` (kind 8, 0x44 bytes, made by 0x650470).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct People {
    /// +0x2c the wishes of the last check: 1 diggers, 2 builders, 4 a residence.
    pub flags: i32,
    /// +0x34 the result of the residence problem (1 / 2).
    pub result: i32,
    /// +0x38 the residence type asked for.
    pub kind: i32,
    /// +0x3c a residence may be asked for (cleared by unfinished residences or free beds).
    pub residence_ok: bool,
    /// +0x40 a countdown from 200.
    pub countdown: i32,
}

impl People {
    pub fn new() -> People {
        People { flags: 0, result: 0, kind: 0, residence_ok: true, countdown: 0 }
    }
}

/// `CBuildingManagerTask` (kind 3, 0x14c bytes, ctor 0x648d70).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BuildingMgr {
    /// +0x30 own sites, +0x34 the planned building's state, +0x38 its type, +0x3c its id.
    pub sites: i32,
    pub plan_state: i32,
    pub plan_kind: i32,
    pub plan_id: i32,
    /// +0x40 / +0x44 sites with all material, +0x48 sites still needing some.
    pub ready: i32,
    pub ready2: i32,
    pub waiting: i32,
    /// +0x4c carriers, +0x50 builders, +0x54 diggers.
    pub carriers: i32,
    pub builders: i32,
    pub diggers: i32,
    /// +0x58 the result of the toolsmith problem.
    pub result: i32,
    /// +0x5c the toolsmith (0x15) that takes the tool orders.
    pub toolsmith: i32,
    /// +0x60: 11 goods (boards, stone, gold, the tools) with what is needed (+4), offered (+8)
    /// and 80 (+0xc) (rdata 0x10714c0 …).
    pub goods: [[i32; 4]; 11],
    /// +0x110: 5 building types asked for; +0x124: 10 building types being placed.
    pub asked: [i32; 5],
    pub placing: [i32; 10],
}

impl Default for BuildingMgr {
    fn default() -> Self {
        BuildingMgr::new()
    }
}

impl BuildingMgr {
    pub fn new() -> BuildingMgr {
        let g = |good| [good, 0, 0, 80];
        BuildingMgr {
            sites: 0,
            plan_state: 2,
            plan_kind: 0,
            plan_id: 0,
            ready: 0,
            ready2: 0,
            waiting: 0,
            carriers: 0,
            builders: 0,
            diggers: 0,
            result: 0,
            toolsmith: 0,
            goods: [g(7), g(32), g(14), g(4), g(18), g(25), g(31), g(28), g(29), g(27), g(8)],
            asked: [0; 5],
            placing: [0; 10],
        }
    }

    /// 0x649ab0: the index of `good` in the goods (11 when not there).
    fn index(&self, good: i32) -> usize {
        self.goods.iter().position(|g| g[0] == good).unwrap_or(11)
    }

    /// `goods[k][1] += 1` with the release's index 11 (one past the goods: the +0x110 list's
    /// second slot) kept.
    fn need_plus(&mut self, k: usize) {
        if k < 11 {
            self.goods[k][1] += 1;
        } else {
            self.asked[1] += 1;
        }
    }

    /// 0x6492d0: into the first free slot of +0x110.
    pub fn ask(&mut self, t: i32) {
        if let Some(s) = self.asked.iter_mut().find(|s| **s == 0) {
            *s = t;
        }
    }

    /// 0x649310: out of +0x110; whether it was there.
    pub fn unask(&mut self, t: i32) -> bool {
        match self.asked.iter_mut().find(|s| **s == t) {
            Some(s) => {
                *s = 0;
                true
            }
            None => false,
        }
    }

    /// 0x649350: into the first free slot of +0x124.
    pub fn place(&mut self, t: i32) -> bool {
        match self.placing.iter_mut().find(|s| **s == 0) {
            Some(s) => {
                *s = t;
                true
            }
            None => false,
        }
    }

    /// 0x649390.
    pub fn placing_has(&self, t: i32) -> bool {
        self.placing.contains(&t)
    }

    /// 0x6493c0: out of +0x124.
    pub fn unplace(&mut self, t: i32) -> bool {
        match self.placing.iter_mut().find(|s| **s == t) {
            Some(s) => {
                *s = 0;
                true
            }
            None => false,
        }
    }
}

/// `CMaterialManagerTask` (kind 4, 0x44 bytes).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Material {
    /// +0x2c the stock counted, +0x30 the state (1..4), +0x34 the good, +0x38 the result,
    /// +0x3c a countdown (5).
    pub stock: i32,
    pub state: i32,
    pub good: i32,
    pub result: i32,
    pub countdown: i32,
}

impl Material {
    pub fn new(good: i32) -> Material {
        Material { stock: 0, state: 1, good, result: 0, countdown: 0 }
    }
}

/// `CResourceManagerTask` (kind 5, 100 bytes; 0x6579d0 is its destructor, vtbl+0).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Resource {
    /// +0x2c the resource (0x70 trees, 0x60 stone, …; −2..−6 ground classes), +0x30 a
    /// threshold, +0x34 the cells found.
    pub res: i32,
    pub threshold: i32,
    pub cells: i32,
    /// +0x48 the result of its expandation.
    pub result: i32,
    /// +0x50 the first building type whose record uses the resource (0x64cb70).
    pub kind: i32,
    /// +0x54 the scored cells (`CFilter` of 8, `CResourceFilter` entries).
    pub filter: Option<Filter>,
    /// +0x5c it wants more land, +0x60 a countdown (200).
    pub wants: i32,
    pub countdown: i32,
}

impl Resource {
    pub fn new(res: i32, threshold: i32) -> Resource {
        Resource { res, threshold, cells: 0, result: 0, kind: 0, filter: None, wants: 0, countdown: 200 }
    }
}

/// `CExpandationManagerTask` (kind 6, 0x6c bytes, ctor 0x64a1f0).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ExpandMgr {
    /// +0x34 the border cells (`cy << 16 | cx`, slot 0x100).
    pub border: Vec<i32>,
    /// +0x40 the state (2 at first), +0x44 a countdown, +0x48 go, +0x4c the resource index.
    pub state: i32,
    pub countdown: i32,
    pub go: bool,
    pub index: i32,
    /// +0x50: the resources it cycles through (only the first five are used).
    pub res: [i32; 7],
}

impl Default for ExpandMgr {
    fn default() -> Self {
        ExpandMgr::new()
    }
}

impl ExpandMgr {
    pub fn new() -> ExpandMgr {
        ExpandMgr { border: Vec::new(), state: 2, countdown: 0, go: true, index: 0, res: [0x10, 0x60, 0x20, 0x30, 0x70, -3, -4] }
    }

    /// 0x64a460 `(cx, cy)`: a border cell.
    pub fn is_border(&self, cx: i32, cy: i32) -> bool {
        self.border.contains(&((cy << 16) + cx))
    }
}

/// `CExpandationEvent` (kind 7, 0x6c bytes, ctors 0x64a510 / 0x64a5d0).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ExpandEvent {
    /// +0x2c the scored targets (`CFilter` of 5, landscape entries).
    pub filter: Option<Filter>,
    /// +0x30 foreign cells with the resource (slot 0xbc), +0x3c foreign cells with the ground
    /// class (slot 0xfc).
    pub cells: Vec<super::manager::CellValue>,
    pub class_cells: Vec<super::manager::CellValue>,
    /// +0x4c the main problem's id, +0x50 the resource, +0x54 / +0x58 set by 0x6550b0.
    pub sub: i32,
    pub res: i32,
    pub f54: i32,
    pub f58: i32,
    /// +0x60 the distance allowance (+50 per empty filter), +0x64 the result, +0x68 a class
    /// resource (the cells are in +0x3c).
    pub radius: i32,
    pub result: i32,
    pub classes: bool,
}

impl ExpandEvent {
    /// 0x64a510 `(link, _, res)`.
    pub fn new(res: i32) -> ExpandEvent {
        ExpandEvent { filter: None, cells: Vec::new(), class_cells: Vec::new(), sub: 0, res, f54: 0, f58: 0, radius: 0, result: 0, classes: false }
    }
}

/// `CProductivityControll` (kind 2, 0x70 bytes, ctor 0x656d90).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Productivity {
    /// +0x2c the building, +0x30 its status (1 planned, 2 site, 3 finished).
    pub building: i32,
    pub status: i32,
    /// +0x34, +0x38 a countdown (1), +0x3c, +0x40 the value (0x657670).
    pub f34: i32,
    pub countdown: i32,
    pub f3c: i32,
    pub value: i32,
    /// +0x44 / +0x48 the position (twice).
    pub pos: i32,
    pub pos2: i32,
    /// +0x4c the food it gets (mines: 0x18 / 0xb / 9 by type), +0x50.
    pub food: i32,
    pub f50: i32,
    /// +0x54 the building type.
    pub kind: i32,
    /// +0x59 an order is out, +0x5c the result of its problem, +0x60, +0x64 / +0x65 flags,
    /// +0x68 retries (5), +0x6c a countdown.
    pub ordered: bool,
    pub result: i32,
    pub f60: i32,
    pub clustered: bool,
    pub tool_asked: bool,
    pub retries: i32,
    pub tool_countdown: i32,
}

impl Productivity {
    /// The ctor (0x656d90) `(building, status, type, pos)`.
    pub fn new(building: i32, status: i32, kind: i32, pos: i32) -> Productivity {
        let food = match kind {
            0xe | 0xf => 0x18,
            0x10 | 0x12 => 0xb,
            0x11 => 9,
            _ => 0,
        };
        Productivity {
            building,
            status,
            f34: 0,
            countdown: 1,
            f3c: 0,
            value: 0,
            pos,
            pos2: pos,
            food,
            f50: 0,
            kind,
            ordered: false,
            result: 0,
            f60: 0,
            clustered: false,
            tool_asked: false,
            retries: 5,
            tool_countdown: 0,
        }
    }
}

/// `CBuildingEvent` (kind 9, 0x58 bytes).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BuildingEvent {
    /// +0x2c the type, +0x30 the building (0 while planned), +0x34 its status (1 planned, 2
    /// site, 3 finished), +0x38 the state (1 to build … 7 done).
    pub kind: i32,
    pub building: i32,
    pub status: i32,
    pub state: i32,
    /// +0x3c / +0x40 the cell, +0x44 the node.
    pub cx: i32,
    pub cy: i32,
    pub pos: i32,
    pub f48: i32,
    /// +0x50 a countdown (10 after the order).
    pub counter: i32,
    /// +0x54 its productivity control is made; +0x55 its tool was ordered.
    pub placed: bool,
    pub asked: bool,
}

// ---- the dispatch ------------------------------------------------------------------------------

/// vtbl+0x10 (`CanStart`).
pub fn can_start(e: &mut Engine, m: &mut dyn Manager, i: usize) -> bool {
    match &e.s.list[i].d {
        Data::Productivity(_) => productivity_alive(e, m, i),
        Data::BuildingMgr(_) | Data::People(_) | Data::Material(_) | Data::Resource(_) | Data::ExpandEvent(_) => true,
        Data::ExpandMgr(_) => true,
        Data::Building(_) => building_alive(e, m, i),
        Data::Main(_) | Data::Sub(_) | Data::Invoke(_) => problems::can_start(e, m, i),
    }
}

/// vtbl+0x14 (`Check`).
pub fn check(e: &mut Engine, m: &mut dyn Manager, i: usize) -> bool {
    match &e.s.list[i].d {
        Data::Productivity(_) => productivity_check(e, i),
        Data::BuildingMgr(_) => {
            building_mgr_update(e, m, i, false);
            matches!(&e.s.list[i].d, Data::BuildingMgr(b) if b.result != 0)
        }
        Data::People(_) => people_check(e, m, i),
        Data::Material(_) => material_check(e, i),
        Data::Resource(_) => resource_check(e, i),
        Data::ExpandMgr(_) => true,
        Data::ExpandEvent(_) => expand_event_check(e, i),
        Data::Building(_) => building_buildable(e, m, i),
        Data::Main(_) | Data::Sub(_) | Data::Invoke(_) => problems::check(e, m, i),
    }
}

/// vtbl+8 (`Run`): true when vtbl+0xc is due.
pub fn run(e: &mut Engine, m: &mut dyn Manager, i: usize) -> bool {
    match &e.s.list[i].d {
        Data::Productivity(_) => productivity_run(e, m, i),
        Data::BuildingMgr(_) => building_mgr_run(e, m, i),
        Data::People(_) => people_run(e, m, i),
        Data::Material(_) => material_run(e, m, i),
        Data::Resource(_) => resource_run(e, m, i),
        Data::ExpandMgr(_) => expand_mgr_run(e, m, i),
        Data::ExpandEvent(_) => expand_event_run(e, m, i),
        Data::Building(_) => building_run(e, m, i),
        Data::Main(_) | Data::Sub(_) | Data::Invoke(_) => problems::run(e, m, i),
    }
}

/// vtbl+0xc (`Finish`).
pub fn finish(e: &mut Engine, m: &mut dyn Manager, i: usize) {
    match &e.s.list[i].d {
        Data::Productivity(_) => productivity_finish(e, m, i),
        Data::BuildingMgr(_) => building_mgr_finish(e, m, i),
        Data::People(_) => people_finish(e, m, i),
        Data::Material(_) => material_finish(e, m, i),
        Data::Resource(_) => resource_finish(e, i),
        Data::ExpandMgr(_) => {}
        Data::ExpandEvent(_) => expand_event_finish(e, i),
        Data::Building(_) => building_finish(e, m, i),
        Data::Main(_) | Data::Sub(_) | Data::Invoke(_) => problems::finish(e, m, i),
    }
}

/// The result of a finished event for the entry `link` waits on (the switch repeated in the
/// events: kind 2 +0x5c (not when done), 3 +0x58, 4 +0x38, 5 +0x48 (and its link cleared),
/// 6 +0x40, 7 +0x64, 8 +0x34 (and its link cleared)).
pub fn tell(e: &mut Engine, link: i32, v: i32) {
    let Some(k) = e.find_id(link) else { return };
    let en = &mut e.s.list[k];
    let state = en.state;
    match &mut en.d {
        Data::Productivity(p) => {
            if state != 3 {
                p.result = v;
            }
        }
        Data::BuildingMgr(b) => b.result = v,
        Data::Material(x) => x.result = v,
        Data::Resource(r) => {
            r.result = v;
            en.link = 0;
        }
        Data::ExpandMgr(x) => x.state = v,
        Data::ExpandEvent(x) => x.result = v,
        Data::People(p) => {
            p.result = v;
            en.link = 0;
        }
        _ => {}
    }
}

fn bm(e: &Engine) -> Option<&BuildingMgr> {
    e.s.list.iter().find_map(|x| match &x.d {
        Data::BuildingMgr(b) => Some(&**b),
        _ => None,
    })
}

pub fn bm_mut(e: &mut Engine) -> Option<&mut BuildingMgr> {
    e.s.list.iter_mut().find_map(|x| match &mut x.d {
        Data::BuildingMgr(b) => Some(&mut **b),
        _ => None,
    })
}

fn expand_mgr(e: &Engine) -> Option<&ExpandMgr> {
    e.s.list.iter().find_map(|x| match &x.d {
        Data::ExpandMgr(b) => Some(b),
        _ => None,
    })
}

/// The first expandation manager's border list check (0x64a460 on the kind-6 entry).
pub fn is_border(e: &Engine, cx: i32, cy: i32) -> bool {
    expand_mgr(e).is_some_and(|x| x.is_border(cx, cy))
}

/// 0x6518f0 `(building, status)`: the productivity control (not done) of that building in that
/// status.
pub fn find_prod(e: &Engine, building: i32, status: i32) -> Option<usize> {
    e.s.list.iter().position(|x| x.state != 3 && matches!(&x.d, Data::Productivity(p) if p.building == building && p.status == status))
}

/// 0x652860 `(old, new, status)`: the productivity controls of building `old` move to `new`
/// with `status`.
pub fn rename_prod(e: &mut Engine, old: i32, new: i32, status: i32) {
    for x in &mut e.s.list {
        if x.state != 3
            && let Data::Productivity(p) = &mut x.d
            && p.building == old
        {
            p.building = new;
            p.status = status;
        }
    }
}

/// 0x651a90 `(type)`: how many productivity controls of a type are enough.
fn enough(e: &Engine, m: &mut dyn Manager, t: i32) -> i32 {
    match t {
        2 => e.prod_count(1) / 2 + 1,
        5 => (e.prod_count(10) + e.prod_count(0x27) + e.prod_count(0x25) + e.prod_count(0xc)) / 2 + 1,
        7 => (e.f20() as u32 >> 3 & 2) as i32,
        0xb => e.prod_count(0xc) + 1 + e.prod_count(9),
        0xe => {
            let _ = m;
            if e.f20() & 0x10 == 0 {
                return 0;
            }
            match e.s.race {
                4 => 1,
                1 => 1,
                0 => 1,
                2 => 2,
                _ => 0,
            }
        }
        0x19 if e.s.race == 1 => 2,
        _ => 0,
    }
}

/// 0x651bd0 `(type)`: there are enough productivity controls of the type.
fn have_enough(e: &Engine, m: &mut dyn Manager, t: i32) -> bool {
    let n = e.prod_count(t);
    enough(e, m, t) <= n
}

// ---- the manager's events (0x655510) -----------------------------------------------------------

/// 0x655510: every event in the eco AI's queue (slot 0x150, `Get(rec, 1)`); true when one was
/// handled (the step then ends). By type (`a` a building unless said otherwise), each only for
/// a building of this eco-sector (slot 0x60 == slot 0x84):
/// * 4 (gone): its building event (site) and productivity control end.
/// * 5 (a site placed): the building event at its position takes it (status 2, state 4, the
///   productivity controls renamed); a new productivity control unless one exists.
/// * 6 (finished): the building event (site) ends (state 7, status 3), controls → status 3.
/// * 7 (taken over): nothing beyond the check.
/// * 8 (a residence): the people manager may ask for a residence again.
/// * 10: the expandation events end (0x654f60).
/// * 0x1e (from the squads): the production orders of the weaponsmith (0x655110).
/// * anything else: handled, nothing done.
pub fn poll_events(e: &mut Engine, m: &mut dyn Manager) -> bool {
    let mut handled = false;
    while let Some(ev) = m.next_event() {
        let a = ev.a;
        match ev.kind {
            4 => {
                if m.eco_of_building(a) != m.eco() {
                    continue;
                }
                let mut status = 3;
                if let Some(k) = e.s.list.iter().position(|x| x.state != 3 && matches!(&x.d, Data::Building(b) if b.building == a && b.status == 2)) {
                    status = 2;
                    e.s.list[k].state = 3;
                }
                if let Some(k) = find_prod(e, a, status) {
                    e.s.list[k].state = 3;
                }
                handled = true;
            }
            5 => {
                if !m.own_building(a) || m.eco_of_building(a) != m.eco() {
                    continue;
                }
                let (x, y) = m.building_pos(a).unwrap_or((0, 0));
                if let Some(k) = e.s.list.iter().position(|x2| x2.state != 3 && matches!(&x2.d, Data::Building(b) if b.pos & 0xffff == x && b.pos >> 16 == y)) {
                    let old = match &e.s.list[k].d {
                        Data::Building(b) => b.building,
                        _ => 0,
                    };
                    rename_prod(e, old, a, 2);
                    if let Data::Building(b) = &mut e.s.list[k].d {
                        b.building = a;
                        b.status = 2;
                        b.state = 4;
                    }
                }
                let t = m.kind(a);
                if find_prod(e, a, 2).is_none() && rec(e.s.race, t, 14) != 0 {
                    e.push(2, Data::Productivity(Productivity::new(a, 2, t, (y << 16) + x)));
                }
                handled = true;
            }
            6 => {
                if !m.own_building(a) || m.eco_of_building(a) != m.eco() {
                    continue;
                }
                if let Some(k) = e.s.list.iter().position(|x| x.state != 3 && matches!(&x.d, Data::Building(b) if b.building == a && b.status == 2))
                    && let Data::Building(b) = &mut e.s.list[k].d
                {
                    b.state = 7;
                    b.status = 3;
                }
                rename_prod(e, a, a, 3);
                handled = true;
            }
            7 => {
                if m.eco_of_building(a) == m.eco() && find_prod(e, a, 3).is_some() {
                    handled = true;
                }
            }
            8 => {
                let k = e.find_kind(8);
                if m.eco_of_building(a) == m.eco() {
                    if let Some(k) = k
                        && let Data::People(p) = &mut e.s.list[k].d
                    {
                        p.residence_ok = true;
                    }
                    handled = true;
                }
            }
            10 => {
                if m.eco_of_building(a) == m.eco() {
                    end_expansions(e, m, a);
                    handled = true;
                }
            }
            0x1e => {
                let _ = m.eco();
                weapon_orders(e, m, ev.player, ev.a);
                handled = true;
            }
            _ => handled = true,
        }
    }
    handled
}

/// 0x654f60 `(building)`: every expandation event ends and tells its waiting entry 2; the
/// expandation manager's +0x48 goes on.
fn end_expansions(e: &mut Engine, m: &mut dyn Manager, building: i32) {
    let _ = m.building_pos(building);
    let n = e.s.list.len();
    for k in 0..n {
        if e.s.list[k].kind == 7 {
            e.s.list[k].state = 3;
            let link = e.s.list[k].link;
            tell(e, link, 2);
        }
    }
    if let Some(k) = e.find_kind(6)
        && let Data::ExpandMgr(x) = &mut e.s.list[k].d
    {
        x.go = true;
    }
}

/// 0x655110 `(a, b)`: with a weaponsmith (0x16) in the eco-sector, the first finished and
/// working one gets its orders reset and then the missing goods ordered: `a` = soldiers wanted
/// (low: swords 0x22, high: bows 8), `b` (low: 3 + the race's 5 / 6 / 0x26, high: armour 3).
fn weapon_orders(e: &mut Engine, m: &mut dyn Manager, a: i32, b: i32) {
    if m.count_buildings(0x16) <= 0 {
        return;
    }
    let mut ids = Vec::new();
    m.buildings(0x16, &mut ids);
    let swords = (a & 0xffff) - m.offered(0x22);
    let bows = (a >> 16) - m.offered(8);
    let mut third = b & 0xffff;
    let armour = (b >> 16) - m.offered(3);
    match e.s.race {
        1 => third -= m.offered(5),
        2 => third -= m.offered(6),
        4 => third -= m.offered(0x26),
        _ => {}
    }
    let mut smith = 0;
    for &id in &ids {
        if m.finished_working(id) {
            smith = id;
            break;
        }
    }
    if ids.is_empty() || smith == 0 {
        return;
    }
    m.produce(smith, 0x22, 0);
    m.produce(smith, 8, 0);
    m.produce(smith, 3, 0);
    match e.s.race {
        1 => m.produce(smith, 5, 0),
        2 => m.produce(smith, 6, 0),
        4 => m.produce(smith, 0x26, 0),
        _ => {}
    }
    if swords > 0 {
        m.produce(smith, 0x22, swords);
    }
    if bows > 0 {
        m.produce(smith, 8, bows);
    }
    if armour > 0 {
        m.produce(smith, 3, armour);
    }
    if third > 0 {
        match e.s.race {
            1 => m.produce(smith, 5, third),
            2 => m.produce(smith, 6, third),
            4 => m.produce(smith, 0x26, third),
            _ => {}
        }
    }
}

// ---- CPeopleManagerTask -------------------------------------------------------------------------

fn people(e: &mut Engine, i: usize) -> &mut People {
    match &mut e.s.list[i].d {
        Data::People(p) => p,
        _ => unreachable!("people"),
    }
}

/// 0x656370: the planned carrier need: offered tools (8, 0x22) + 10 · (2 + the productivity
/// controls of types 1, 4 and 0x11).
fn people_need(e: &Engine, m: &mut dyn Manager) -> i32 {
    let t1 = e.prod_count(1);
    let t4 = e.prod_count(4);
    let t11 = e.prod_count(0x11);
    m.offered(8) + m.offered(0x22) + (t11 + t4 + t1 + 2) * 10
}

/// 0x656430: with more beds than carriers + 50 by a residence's size, the first finished and
/// working residence of that type is pulled down (types 0x28, 0x29, 0x2a in turn).
fn people_beds(m: &mut dyn Manager) {
    let carriers = m.player_carriers();
    let beds = m.total_beds();
    let sizes = [m.beds(0x28), m.beds(0x29), m.beds(0x2a)];
    for (k, t) in [0x28, 0x29, 0x2a].into_iter().enumerate() {
        if sizes[k] < beds - (carriers + 0x32) && m.count_buildings(t) > 0 {
            let mut ids = Vec::new();
            m.buildings(t, &mut ids);
            if !ids.is_empty() && m.finished_working(ids[0]) {
                m.destroy(ids[0]);
                return;
            }
        }
    }
}

/// 0x6565c0: an unfinished residence or one with free beds forbids asking for another.
fn people_residences(e: &mut Engine, m: &mut dyn Manager, i: usize) {
    for t in [0x28, 0x29, 0x2a] {
        let mut ids = Vec::new();
        m.buildings(t, &mut ids);
        for &id in &ids {
            if m.progress(id) < 100 || m.free_beds(id) > 0 {
                people(e, i).residence_ok = false;
            }
        }
    }
}

/// 0x6566d0: the carriers kept free: `(done 0x11 + done 4 + done 1) · 7 + 10`, unless the
/// carriers are not 10 more than that (then carriers − 10), clamped 5..80; extra carriers 12.
fn people_carriers(e: &Engine, m: &mut dyn Manager) {
    let k = (e.done_count(0x11) + e.done_count(4) + e.done_count(1)) * 7 + 10;
    let c = m.settlers(1);
    let mut n = c - 10;
    if c - k > 9 {
        n = k;
    }
    let n = n.clamp(5, 0x50);
    m.set_extra_carriers(0xc);
    m.set_min_carriers(n);
}

/// vtbl+8 (0x656070): the wishes: diggers below `clamp((need − 20) / 6 + 5, 5, 15)` (1, when
/// more are missing than shovels are offered), builders (2, hammers looked at), a residence
/// (4) when carriers are short of `min(diggers + builders missing, 10) + need` and no residence
/// is under way; true with the residence wish.
fn people_run(e: &mut Engine, m: &mut dyn Manager, i: usize) -> bool {
    let diggers = m.settlers(2);
    let builders = m.settlers(3);
    let carriers = m.settlers(1);
    let need = people_need(e, m);
    let want = ((need - 0x14) / 6 + 5).clamp(5, 0xf);
    people_beds(m);
    people_carriers(e, m);
    let link = e.s.list[i].link;
    let p = people(e, i);
    p.flags = 0;
    p.result = 0;
    let mut d = 0;
    if diggers < want {
        d = want - diggers;
        if d > 0 {
            let shovels = m.offered(0x1f);
            if d - shovels > 0 {
                people(e, i).flags |= 1;
            }
        }
    }
    let mut b = 0;
    if builders < want {
        b = want - builders;
        if b > 0 {
            m.offered(0x12);
            people(e, i).flags |= 2;
        }
    }
    let extra = (b + d).min(10);
    if carriers < extra + need && link == 0 {
        people(e, i).flags |= 4;
        return true;
    }
    false
}

/// vtbl+0x14 (0x656240), while it waits: the carriers and residences first; answer 1 (the
/// residence problem failed for land) → an expandation event for ground class −3 (unless one
/// exists or nothing is linked) and it runs again; any other answer takes 0x29 / 0x2a out of
/// the building manager's wishes (then a residence may be asked again), and with 2 the
/// countdown runs and it runs again once a residence may be asked.
fn people_check(e: &mut Engine, m: &mut dyn Manager, i: usize) -> bool {
    people_carriers(e, m);
    people_residences(e, m, i);
    let result = people(e, i).result;
    match result {
        0 => false,
        1 => {
            if e.s.list[i].link != 0 && e.find_kind(7).is_none() {
                let id = e.s.list[i].id;
                let new = e.push_linked(7, id, Data::ExpandEvent(ExpandEvent::new(-3)));
                e.s.list[i].link = new;
            }
            true
        }
        _ => {
            if let Some(b) = bm_mut(e)
                && (b.unask(0x29) || b.unask(0x2a))
            {
                people(e, i).residence_ok = true;
            }
            if people(e, i).result == 2 {
                let p = people(e, i);
                p.countdown -= 1;
                if p.residence_ok {
                    e.s.list[i].link = 0;
                    return true;
                }
            }
            false
        }
    }
}

/// vtbl+0xc (0x6561a0): with the residence wish, a main problem (9) for a residence (0x29, or
/// 0x2a with more than 2 of 0x29 or 2 each of the types 3 and 4) near the existing ones
/// (0x6536e0(0x28)); then it waits (state 1).
fn people_finish(e: &mut Engine, m: &mut dyn Manager, i: usize) {
    if people(e, i).flags & 4 == 0 {
        return;
    }
    let pos = home_pos(e, m, 0x28);
    let t = residence_kind(e, m);
    people(e, i).kind = t;
    let id = e.s.list[i].id;
    let new = problems::push_main(e, id, 9, t, pos, 0);
    e.s.list[i].link = new;
    e.s.list[i].state = 1;
    let p = people(e, i);
    p.result = 0;
    p.residence_ok = false;
    p.countdown = 200;
}

/// 0x653980: a big residence (0x2a) when there are more than 2 medium ones (0x29), or 2 each of
/// the types 3 and 4; else a medium one.
fn residence_kind(e: &Engine, m: &mut dyn Manager) -> i32 {
    if m.count_buildings(0x29) > 2 {
        return 0x2a;
    }
    let t3 = e.prod_count(3);
    if t3 > 1 && e.prod_count(4) > 1 {
        return 0x2a;
    }
    0x29
}

/// 0x6532f0 `(t)`: the player's buildings of `t` (slot 0xa0; flag 3) and the planned building
/// events of `t` (state 1; flag 1), as (id, flag).
fn buildings_and_plans(e: &Engine, m: &mut dyn Manager, t: i32) -> Vec<(i32, i32)> {
    let mut ids = Vec::new();
    m.buildings(t, &mut ids);
    let mut v: Vec<(i32, i32)> = ids.into_iter().map(|id| (id, 3)).collect();
    for x in &e.s.list {
        if x.state != 3
            && let Data::Building(b) = &x.d
            && b.state == 1
            && b.kind == t
        {
            v.push((b.building, 1));
        }
    }
    v
}

/// 0x6535a0 `(id)`: the position of the building event of building `id`.
pub fn plan_pos(e: &Engine, id: i32) -> Option<(i32, i32)> {
    e.s.list.iter().find_map(|x| match &x.d {
        Data::Building(b) if x.state != 3 && b.building == id => Some((b.pos & 0xffff, b.pos >> 16)),
        _ => None,
    })
}

/// 0x6536e0 `(t)`: where a building of type `t` should go: for the toolsmith (0x15) the mean of
/// up to two tool-workshop (0x14) buildings or plans; for residences (> 0x27, races 0..2 and 4)
/// the mean of up to two of type 3, else the first castle (0x2e); otherwise 0. Packed. A
/// position that cannot be read leaves the previous one (stack locals; 0 at first [inf]).
pub fn home_pos(e: &Engine, m: &mut dyn Manager, t: i32) -> i32 {
    let (mut x, mut y) = (0, 0);
    let mut mean = |m: &mut dyn Manager, list: &[(i32, i32)]| -> i32 {
        let (mut sx, mut sy, mut n) = (0, 0, 0);
        for &(id, flag) in list.iter().take(2) {
            if flag == 1 {
                if let Some(p) = plan_pos(e, id) {
                    (x, y) = p;
                }
            } else if let Some(p) = m.building_pos(id) {
                (x, y) = p;
            }
            sx += x;
            sy += y;
            n += 1;
        }
        if n > 0 { ((sy / n) << 16) + sx / n } else { 0 }
    };
    if t == 0x15 {
        let list = buildings_and_plans(e, m, 0x14);
        return if list.is_empty() || list[0].0 == 0 { 0 } else { mean(m, &list) };
    }
    let race = e.s.race;
    if t <= 0x27 || race < 0 || (race > 2 && race != 4) {
        return 0;
    }
    let list = buildings_and_plans(e, m, 3);
    if !list.is_empty() && list[0].0 != 0 {
        return mean(m, &list);
    }
    let mut ids = Vec::new();
    m.buildings(0x2e, &mut ids);
    if let Some(&first) = ids.first() {
        let (x, y) = m.building_pos(first).unwrap_or((0, 0));
        return (y << 16) + x;
    }
    0
}

// ---- CBuildingManagerTask ------------------------------------------------------------------------

/// 0x649400 `(mark)`: counts the eco-sector (diggers, builders, carriers), drops a lost
/// toolsmith, the goods offered and needed by the planned buildings (costs) and the sites
/// (their needs; with all material +0x40 / +0x44, else +0x48), the tools wanted by sites and
/// finished buildings without their worker (one each, marked with `mark`); the digger and
/// builder reservations and tool orders; the planned building's end; then 0x649960.
fn building_mgr_update(e: &mut Engine, m: &mut dyn Manager, i: usize, mark: bool) {
    let mut b = match &e.s.list[i].d {
        Data::BuildingMgr(b) => (**b).clone(),
        _ => return,
    };
    b.sites = 0;
    b.diggers = m.settlers(2);
    b.builders = m.settlers(3);
    b.carriers = m.settlers(1);
    b.ready = 0;
    b.ready2 = 0;
    b.waiting = 0;
    if b.toolsmith != 0 && !m.own_building(b.toolsmith) {
        b.toolsmith = 0;
    }
    for g in &mut b.goods {
        g[1] = 0;
        g[2] = m.offered(g[0]);
    }
    let n = e.s.list.len();
    for k in 0..n {
        let (state, kind) = (e.s.list[k].state, e.s.list[k].kind);
        if state == 3 {
            continue;
        }
        if let Data::Building(ev) = &e.s.list[k].d {
            let ev = ev.clone();
            if ev.status == 1 {
                for w in 0..3 {
                    b.goods[w][1] += m.cost(ev.kind, w as i32);
                }
            } else {
                if !m.own_building(ev.building) {
                    continue;
                }
                b.sites += 1;
                let mut all = true;
                for w in 0..3 {
                    let need = m.need(ev.building, w as i32);
                    b.goods[w][1] += need;
                    if need > 0 {
                        all = false;
                    }
                }
                if all {
                    b.ready += 1;
                    b.ready2 += 1;
                } else {
                    b.waiting += 1;
                }
            }
            // [the release reads the race-0 row of 0x1457598 here]
            let tool = super::tables::RECORDS[super::tables::RECORD_OF[0].get(ev.kind as usize).copied().unwrap_or(0) as usize][9];
            if tool != 0
                && let Some(p) = e.s.list.iter().position(|x| x.state != 3 && matches!(&x.d, Data::Productivity(p) if p.building == ev.building && p.status == ev.status))
            {
                let asked = matches!(&e.s.list[p].d, Data::Productivity(q) if q.tool_asked);
                if !asked && b.toolsmith != 0 {
                    if mark && let Data::Productivity(q) = &mut e.s.list[p].d {
                        q.tool_asked = true;
                    }
                    let idx = b.index(tool);
                    b.need_plus(idx);
                }
            }
            continue;
        }
        if kind == 2
            && let Data::Productivity(p) = &e.s.list[k].d
            && p.status == 3
        {
            let (bid, pk, asked) = (p.building, p.kind, p.tool_asked);
            if !m.own_building(bid) || asked || b.toolsmith == 0 {
                continue;
            }
            let tool = super::tables::RECORDS[super::tables::RECORD_OF[0].get(pk as usize).copied().unwrap_or(0) as usize][9];
            if tool == 0 || m.worker_home(bid) {
                continue;
            }
            let idx = b.index(tool);
            b.need_plus(idx);
            if mark && let Data::Productivity(q) = &mut e.s.list[k].d {
                q.tool_asked = true;
            }
        }
    }
    // The digger and builder targets.
    let smiths = e.s.list.iter().filter(|x| x.state != 3 && matches!(&x.d, Data::Productivity(p) if p.kind == 0x15 && p.status == 3)).count();
    let (dig_target, build_target);
    if smiths != 0 {
        dig_target = 10 - b.diggers;
        build_target = 0xf - b.builders;
        if b.toolsmith != 0 {
            m.produce(b.toolsmith, 0x1f, dig_target - b.diggers);
            if b.toolsmith != 0 {
                m.produce(b.toolsmith, 0x12, build_target - b.builders);
            }
        }
    } else {
        dig_target = 7;
        build_target = 7;
    }
    if b.diggers < dig_target && m.worker_delta(2) == 0 {
        let shovels = m.offered(0x1f);
        if shovels > 0 {
            let n = shovels.min(5);
            let player = m.player();
            let eco = m.eco();
            m.net_message(0x13a3, eco, 2, player, n);
        } else if b.toolsmith != 0 {
            m.produce(b.toolsmith, 0x1f, dig_target - b.diggers);
        }
    }
    if b.builders < build_target && m.worker_delta(3) == 0 {
        let n = m.offered(0x12) - 2;
        if n > 0 {
            let n = n.min(5);
            let player = m.player();
            let eco = m.eco();
            m.net_message(0x13a3, eco, 3, player, n);
        }
    }
    for want in [0, 1] {
        if b.plan_state == want && b.plan_id != 0 {
            if m.own_building(b.plan_id) && m.progress(b.plan_id) < 100 {
                store_bm(e, i, b);
                return;
            }
            b.plan_state = 2;
            b.plan_kind = 0;
            b.plan_id = 0;
        }
    }
    building_mgr_decide(e, m, &mut b);
    store_bm(e, i, b);
}

fn store_bm(e: &mut Engine, i: usize, b: BuildingMgr) {
    if let Data::BuildingMgr(x) = &mut e.s.list[i].d {
        **x = b;
    }
}

/// 0x649960: the next building to plan when material runs short: boards missing beyond the
/// logs offered and no finished woodcutter (1) → a woodcutter; no sawmill (3) → a sawmill; stone
/// short and no stonecutter (4) → one. With a planned type, its stone cost (with an id) above
/// the stone offered pulls a building down (0x6539f0).
fn building_mgr_decide(e: &mut Engine, m: &mut dyn Manager, b: &mut BuildingMgr) -> bool {
    let short = b.goods[0][1] - b.goods[0][2];
    if short > 0 {
        if short > m.offered(0x16) && e.done_count(1) == 0 {
            b.plan_state = 0;
            b.plan_id = 0;
            b.plan_kind = 1;
            return true;
        }
        if e.done_count(3) == 0 {
            b.plan_state = 0;
            b.plan_id = 0;
            b.plan_kind = 3;
            return true;
        }
    }
    if b.goods[1][2] - b.goods[1][1] < 0 && e.done_count(4) == 0 {
        b.plan_state = 0;
        b.plan_id = 0;
        b.plan_kind = 4;
        return true;
    }
    if b.plan_kind != 0 {
        let mut stone = 0;
        if b.plan_id != 0 {
            stone = m.cost(b.plan_kind, 1);
            m.cost(b.plan_kind, 0);
        } else if m.own_building(0) {
            stone = m.need(b.plan_id, 1);
            m.need(b.plan_id, 0);
        }
        if stone > m.offered(0x20) || stone > m.offered(0x20) {
            pull_down(e, m);
        }
    }
    false
}

/// 0x6539f0: with more than 40 beds above the carriers, the first finished and working big
/// residence (0x2a) goes; else, with more than 15, the first medium one (0x29). Whether one
/// went.
fn pull_down(_e: &mut Engine, m: &mut dyn Manager) -> bool {
    let carriers = m.player_carriers();
    let beds = m.total_beds();
    let mut chosen = 0;
    let spare = beds - carriers;
    let mut ids = Vec::new();
    if spare > 0x28 {
        m.buildings(0x2a, &mut ids);
        if !ids.is_empty() && m.finished_working(ids[0]) {
            chosen = ids[0];
        }
    }
    if chosen == 0 && spare > 0xf {
        m.buildings(0x29, &mut ids);
        if !ids.is_empty() && m.finished_working(ids[0]) {
            chosen = ids[0];
        }
    }
    if chosen != 0 {
        m.destroy(chosen);
    }
    chosen != 0
}

/// vtbl+8 (0x649040): the update (marking); with tool workshops (0x14) but no toolsmith
/// control (0x15) → true (vtbl+0xc asks for a toolsmith). Else the toolsmith (the first
/// finished and working 0x15) and the tool orders for the goods missing (hammers also reset the
/// builder reservation).
fn building_mgr_run(e: &mut Engine, m: &mut dyn Manager, i: usize) -> bool {
    building_mgr_update(e, m, i, true);
    if e.prod_count(0x15) == 0 && e.prod_count(0x14) > 0 {
        return true;
    }
    let Some(b) = bm(e).cloned() else { return false };
    let mut b = b;
    if m.count_buildings(0x15) > 0 && b.toolsmith == 0 {
        let mut ids = Vec::new();
        m.buildings(0x15, &mut ids);
        if !ids.is_empty() && m.finished_working(ids[0]) {
            b.toolsmith = ids[0];
        }
    }
    for k in 0..11 {
        let g = b.goods[k];
        if g[2] < g[1] {
            match g[0] {
                0x12 => {
                    let player = m.player();
                    let eco = m.eco();
                    m.net_message(0x13a3, eco, 3, player, 0);
                    if b.toolsmith != 0 {
                        m.produce(b.toolsmith, g[0], g[1] - g[2]);
                    }
                }
                4 | 0x19 | 0x1b | 0x1c | 0x1d | 0x1f if b.toolsmith != 0 => m.produce(b.toolsmith, g[0], g[1] - g[2]),
                _ => {}
            }
        }
    }
    store_bm(e, i, b);
    false
}

/// vtbl+0xc (0x649230): a main problem (9) for a toolsmith (0x15) near the tool workshops; it
/// waits for the result.
fn building_mgr_finish(e: &mut Engine, m: &mut dyn Manager, i: usize) {
    let pos = home_pos(e, m, 0x15);
    let id = e.s.list[i].id;
    let new = problems::push_main(e, id, 9, 0x15, pos, 0);
    e.s.list[i].link = new;
    e.s.list[i].state = 1;
}

/// 0x649ae0 `(event)`: may the building event go on.
pub fn building_allowed(e: &mut Engine, m: &mut dyn Manager, ev: usize) -> bool {
    let Some(k) = e.find_kind(3) else { return false };
    building_mgr_update(e, m, k, false);
    let Some(b) = bm(e).cloned() else { return false };
    let x = match &e.s.list[ev].d {
        Data::Building(x) => x.clone(),
        _ => return false,
    };
    if b.plan_state == 0 {
        if x.kind == 0x2e {
            return true;
        }
        if b.plan_kind != x.kind || (b.plan_id != 0 && b.plan_id != x.building) {
            return false;
        }
        if x.status != 1
            && let Some(bm) = bm_mut(e)
        {
            bm.plan_id = x.building;
        }
        return true;
    }
    if x.status == 1 {
        return b.sites < 0xf;
    }
    if m.progress(x.building) > 1 {
        return true;
    }
    if !x.asked {
        let r = super::tables::RECORD_OF[0].get(x.kind as usize).copied().unwrap_or(0) as usize;
        let tool = super::tables::RECORDS[r][9];
        if let Data::Building(y) = &mut e.s.list[ev].d {
            y.asked = true;
        }
        if tool != 0 {
            let settler = super::tables::RECORDS[r][11];
            let (ia, ib) = (b.index(tool), b.index(tool));
            let need = if ib < 11 { b.goods[ib][1] } else { b.asked[1] };
            let offered = if ia < 11 { b.goods[ia][2] } else { b.asked[2] };
            if need >= offered && m.settlers(settler) <= 0 {
                if b.toolsmith == 0 {
                    if let Data::Building(y) = &mut e.s.list[ev].d {
                        y.asked = false;
                    }
                    return false;
                }
                if let Data::Building(y) = &mut e.s.list[ev].d {
                    y.asked = true;
                }
                m.produce(b.toolsmith, tool, 1);
            }
        }
    }
    production_ready(e, m, x.kind)
}

/// 0x649c90 `(event)`: the buildings a type needs are there: finished productivity controls
/// of the input types, or with [`built_count`] also sites past a build progress.
fn production_ready(e: &Engine, m: &mut dyn Manager, t: i32) -> bool {
    let done = |k| e.done_count(k) != 0;
    match t {
        5 => done(9) || built_count(e, m, 0xc, 0) != 0,
        8 => built_count(e, m, 0xc, 10) != 0 && done(0x16),
        9 | 0xc => done(0xb),
        10 => built_count(e, m, 9, 5) != 0 && done(0x16),
        0x10 => done(0x18),
        0x18 => done(0x16),
        0x13 => built_count(e, m, 0x10, 0xf) != 0,
        0x14 => built_count(e, m, 0x11, 0) != 0 && built_count(e, m, 0xf, 0) != 0,
        0x15 => built_count(e, m, 0x14, 0xf) != 0,
        0x16 => done(0x14),
        0x2b => done(0x13),
        0x2c => done(0x2b),
        _ => true,
    }
}

/// 0x6534c0 `(t, p)`: the finished productivity controls of `t` (0x653560), plus with `p` ≠ 0
/// the building events of `t` past state 1 on a site (status ≠ 1) whose progress is above `p`.
fn built_count(e: &Engine, m: &mut dyn Manager, t: i32, p: i32) -> i32 {
    let mut n = e.done_count(t);
    for x in &e.s.list {
        if x.state != 3
            && let Data::Building(b) = &x.d
            && b.kind == t
            && b.state > 1
            && b.status != 1
            && p != 0
            && m.progress(b.building) > p
        {
            n += 1;
        }
    }
    n
}

// ---- CBuildingEvent ------------------------------------------------------------------------------

fn bev(e: &mut Engine, i: usize) -> &mut BuildingEvent {
    match &mut e.s.list[i].d {
        Data::Building(b) => b,
        _ => unreachable!("building event"),
    }
}

/// vtbl+0x10 (0x64a190): a placed building must still be one of the eco-sector (else done).
fn building_alive(e: &mut Engine, m: &mut dyn Manager, i: usize) -> bool {
    let (status, id) = {
        let b = bev(e, i);
        (b.status, b.building)
    };
    if status != 1 && !m.own_building(id) {
        e.s.list[i].state = 3;
        return false;
    }
    true
}

/// vtbl+0x14 (0x64a1c0): the type can be built at its node.
fn building_buildable(e: &mut Engine, m: &mut dyn Manager, i: usize) -> bool {
    let (t, pos) = {
        let b = bev(e, i);
        (b.kind, b.pos)
    };
    m.can_build(t, pos & 0xffff, pos >> 16)
}

/// vtbl+8 (0x649ee0).
fn building_run(e: &mut Engine, m: &mut dyn Manager, i: usize) -> bool {
    let (status, id, state) = {
        let b = bev(e, i);
        (b.status, b.building, b.state)
    };
    if status != 1 {
        if !m.own_building(id) {
            e.s.list[i].state = 3;
            return false;
        }
        if state == 5 || state == 4 {
            let w = m.working(id);
            bev(e, i).state = 5 - w as i32;
        }
    }
    if !bev(e, i).placed {
        let own = e.s.list[i].id;
        let b = bev(e, i);
        b.building = own;
        b.status = 1;
        b.state = 1;
        let (t, pos) = (b.kind, b.pos);
        if rec(e.s.race, t, 14) != 0 {
            e.push(2, Data::Productivity(Productivity::new(own, 1, t, pos)));
        }
        bev(e, i).placed = true;
    }
    match bev(e, i).state {
        1 => {
            let (t, pos) = {
                let b = bev(e, i);
                (b.kind, b.pos)
            };
            if let Some(b) = bm_mut(e) {
                b.unplace(t);
            }
            if m.can_build(t, pos & 0xffff, pos >> 16) {
                bev(e, i).counter = 10;
                return building_allowed(e, m, i);
            }
            building_fail(e, i);
            false
        }
        2 => {
            let b = bev(e, i);
            b.counter -= 1;
            if b.counter != 0 {
                return false;
            }
            building_fail(e, i);
            false
        }
        3 | 6 | 7 => true,
        4 => !building_allowed(e, m, i),
        5 => building_allowed(e, m, i),
        _ => false,
    }
}

/// The building event gives up (0x64a01b): done, its type asked again (0x6492d0), its
/// productivity control done.
fn building_fail(e: &mut Engine, i: usize) {
    e.s.list[i].state = 3;
    let (t, id, status) = {
        let b = bev(e, i);
        (b.kind, b.building, b.status)
    };
    if let Some(b) = bm_mut(e) {
        b.ask(t);
    }
    if let Some(k) = find_prod(e, id, status) {
        e.s.list[k].state = 3;
    }
}

/// vtbl+0xc (0x64a0c0): state 1 orders the building (slot 0x64) → 2; 3 → the productivity
/// controls renamed (0x652860(building, 0, 2)) → 4; 4 / 5 switch it (slot 0x114) → 5 / 4; 6 → 7;
/// 7 → done.
fn building_finish(e: &mut Engine, m: &mut dyn Manager, i: usize) {
    let (state, t, pos, id) = {
        let b = bev(e, i);
        (b.state, b.kind, b.pos, b.building)
    };
    match state {
        1 => {
            m.construct(t, pos & 0xffff, pos >> 16);
            bev(e, i).state = 2;
        }
        3 => {
            rename_prod(e, id, 0, 2);
            bev(e, i).state = 4;
        }
        4 => {
            m.switch(id);
            bev(e, i).state = 5;
        }
        5 => {
            m.switch(id);
            bev(e, i).state = 4;
        }
        6 => bev(e, i).state = 7,
        7 => e.s.list[i].state = 3,
        _ => {}
    }
}

// ---- CProductivityControll ------------------------------------------------------------------------

fn prod(e: &mut Engine, i: usize) -> &mut Productivity {
    match &mut e.s.list[i].d {
        Data::Productivity(p) => p,
        _ => unreachable!("productivity"),
    }
}

/// vtbl+0x10 (0x657610): a placed building must still be one of the eco-sector.
fn productivity_alive(e: &mut Engine, m: &mut dyn Manager, i: usize) -> bool {
    let (status, id) = {
        let p = prod(e, i);
        (p.status, p.building)
    };
    if status != 1 && !m.own_building(id) {
        e.s.list[i].state = 3;
        return false;
    }
    true
}

/// vtbl+0x14 (0x657640): after its problem answered (1 / 2), retries count down (+0x68); with
/// none left it starts again. A pending answer resets the retries to 5.
fn productivity_check(e: &mut Engine, i: usize) -> bool {
    let p = prod(e, i);
    if p.result == 1 || p.result == 2 {
        p.retries -= 1;
        e.s.list[i].link = 0;
        let p = prod(e, i);
        p.ordered = false;
        return p.retries < 1;
    }
    p.retries = 5;
    false
}

/// 0x657670: the value of the building: record +0x30, plus for types 1 and 4..6
/// `2 · record +0x34 · distance(+0x48, +0x44)`.
pub fn prod_value(e: &mut Engine, i: usize) -> i32 {
    let race = e.s.race;
    let p = prod(e, i).clone();
    let base = rec(race, p.kind, 12);
    if p.kind != 1 && (p.kind < 4 || p.kind > 6) {
        return base;
    }
    base + dist(p.pos2, p.pos) * rec(race, p.kind, 13) * 2
}

/// 0x657730: supply priorities: an input (record +0x18, +0x1c) among 7, 9, 0xa, 0xb, 0x10, 0x14,
/// 0x18, 0x24 with less than 3 on the building's pile gets priority 5 (5028) and ends this;
/// then a mine's food (+0x4c) under 3: food goods 0xb, 0x18, 9 that are offered.
fn prod_supply(e: &mut Engine, m: &mut dyn Manager, i: usize) -> bool {
    let race = e.s.race;
    let p = prod(e, i).clone();
    for w in [6, 7] {
        let g = rec(race, p.kind, w);
        if matches!(g, 7 | 9 | 0xa | 0xb | 0x10 | 0x14 | 0x18 | 0x24) && p.status == 3 && m.pile_amount(p.building, g) < 3 {
            let eco = m.eco();
            let player = m.player();
            m.net_message(0x13a4, (g << 16) + p.kind, (eco << 16) + 5, player, 0);
            return false;
        }
    }
    if p.status == 3 && p.food != 0 && m.pile_amount(p.building, p.food) < 3 {
        for g in [0xb, 0x18, 9] {
            if m.offered(g) > 0 {
                let eco = m.eco();
                let player = m.player();
                m.net_message(0x13a4, (g << 16) + p.kind, (eco << 16) + 5, player, 0);
            }
        }
    }
    true
}

/// vtbl+8 (0x656e80): per type, whether its building needs a problem solved (vtbl+0xc).
fn productivity_run(e: &mut Engine, m: &mut dyn Manager, i: usize) -> bool {
    let mgr = e.find_kind(3);
    let (status, id) = {
        let p = prod(e, i);
        (p.status, p.building)
    };
    if status != 1 && !m.own_building(id) {
        e.s.list[i].state = 3;
        return false;
    }
    let toolsmith = mgr.and_then(|k| match &e.s.list[k].d {
        Data::BuildingMgr(b) => Some(b.toolsmith),
        _ => None,
    });
    {
        let p = prod(e, i);
        p.tool_countdown -= 1;
    }
    if prod(e, i).tool_countdown < 0
        && toolsmith.unwrap_or(0) != 0
        && status != 1
        && !m.worker_home(id)
        && m.pile_amount(toolsmith.unwrap_or(0), 10) > 3
        && m.pile_amount(toolsmith.unwrap_or(0), 0x14) > 3
    {
        let p = prod(e, i);
        p.tool_asked = false;
        p.tool_countdown = 0x3c;
    }
    {
        let p = prod(e, i);
        p.countdown -= 1;
        if p.countdown > 0 {
            return false;
        }
        p.countdown = 1;
    }
    prod_supply(e, m, i);
    let p = prod(e, i).clone();
    let (x, y) = (p.pos & 0xffff, p.pos >> 16);
    let v = prod_value(e, i);
    prod(e, i).value = v;
    let race = e.s.race;
    let placing = |e: &Engine, t| bm(e).is_some_and(|b| b.placing_has(t));
    match p.kind {
        1 | 7 => {
            if p.status == 1 {
                return false;
            }
            move_working_area(e, m, p.building, p.kind);
            let r = resource_count(e, m, p.kind, x, y);
            if p.kind == 7 || p.ordered {
                return false;
            }
            if !near_same(e, 2, x, y, 0x32) && r <= rec(race, p.kind, 3) {
                return true;
            }
            false
        }
        2 => {
            if p.status == 3 {
                let r = resource_count(e, m, 1, x, y);
                if r > rec(race, p.kind, 4) && !prod(e, i).ordered {
                    prod(e, i).ordered = true;
                    m.switch(p.building);
                }
                if r < rec(race, p.kind, 3) && prod(e, i).ordered {
                    prod(e, i).ordered = false;
                    m.switch(p.building);
                }
            }
            false
        }
        4..=6 => {
            if p.status == 1 {
                return false;
            }
            if move_working_area(e, m, p.building, p.kind) {
                return false;
            }
            if p.status != 3 {
                return false;
            }
            resource_at_area(e, m, p.building, p.kind) <= rec(race, p.kind, 3)
        }
        10 | 0xc => !have_enough(e, m, 5) && !p.ordered && !near_same(e, 5, x, y, 0x96),
        0xb | 0x24 => {
            if p.status == 3 {
                move_planting_area(e, m, p.building, p.kind);
            }
            false
        }
        0xe..=0x12 => {
            prod(e, i).f60 = 0;
            if p.status == 3 && !mine_ok(e, m, i) {
                return false;
            }
            if p.status == 1 {
                return false;
            }
            if resource_at_area(e, m, p.building, p.kind) != 0 {
                return false;
            }
            prod(e, i).f60 = 2;
            true
        }
        0x16 => {
            if placing(e, 0x18) || p.ordered {
                return false;
            }
            e.prod_count(0x18) < 1
        }
        0x18 => {
            if p.status == 3 && !p.ordered {
                m.recruit(p.building, 0x1d, 2);
                prod(e, i).ordered = true;
            }
            false
        }
        0x23 | 0x25 | 0x27 => {
            if p.kind == 0x23 && p.status == 3 {
                move_planting_area(e, m, p.building, p.kind);
            }
            if placing(e, 0x2b) || p.status == 1 || p.ordered {
                return false;
            }
            e.prod_count(0x2b) < 1
        }
        0x2b => {
            if placing(e, 0x2c) {
                return false;
            }
            let t = match race {
                2 => Some(0x25),
                0 => Some(0x23),
                1 => Some(0x27),
                _ => None,
            };
            let ok = match t {
                Some(t) if e.done_count(t) > 0 => true,
                _ => race == 4 && e.done_count(0x51) > 0,
            };
            if !ok || p.status == 1 || p.ordered {
                return false;
            }
            e.prod_count(0x2c) < 1
        }
        _ => false,
    }
}

/// 0x657910: a mine with less than 3 of its resource around (0x654210 on its node) is pulled
/// down and done.
fn mine_ok(e: &mut Engine, m: &mut dyn Manager, i: usize) -> bool {
    let p = prod(e, i).clone();
    let v = problems::resource_score(e, m, p.kind, p.pos, 1);
    if v < 3 {
        m.destroy(p.building);
        e.s.list[i].state = 3;
        return false;
    }
    true
}

/// vtbl+0xc (0x6573e0): the problem the building needs: woodcutters (1 / 7) another forester
/// (2), fishers (10 / 0xc) a hunter… (main problem 9 of the type at the building); cutters of
/// 4 / 6 are pulled down; mines answer from +0x60; the chain (0x16 → 0x18, drinks → 0x2b →
/// 0x2c) asks for the next building and remembers it (+0x124).
fn productivity_finish(e: &mut Engine, m: &mut dyn Manager, i: usize) {
    let p = prod(e, i).clone();
    let id = e.s.list[i].id;
    let ask = |e: &mut Engine, t: i32| -> i32 { problems::push_main(e, id, 9, t, p.pos, 0) };
    let (new, placed) = match p.kind {
        1 | 7 => {
            prod(e, i).result = 0;
            (ask(e, 2), 2)
        }
        4 | 6 => {
            m.destroy(p.building);
            e.s.list[i].state = 3;
            return;
        }
        10 | 0xc => {
            prod(e, i).result = 0;
            (ask(e, 5), 5)
        }
        0xe..=0x12 => {
            let _ = prod(e, i).f60 != 2;
            return;
        }
        0x16 => {
            prod(e, i).result = 0;
            (ask(e, 0x18), 0x18)
        }
        0x23 | 0x25 | 0x27 => {
            prod(e, i).result = 0;
            let n = ask(e, 0x2b);
            e.s.list[i].link = n;
            if let Some(b) = bm_mut(e) {
                b.place(0x2b);
            }
            let p = prod(e, i);
            p.ordered = true;
            e.s.list[i].state = 1;
            return;
        }
        0x2b => {
            prod(e, i).result = 0;
            (ask(e, 0x2c), 0x2c)
        }
        _ => return,
    };
    e.s.list[i].link = new;
    if let Some(b) = bm_mut(e) {
        b.place(placed);
    }
    prod(e, i).ordered = true;
    e.s.list[i].state = 1;
}

// ---- working areas -------------------------------------------------------------------------------

/// 0x651d70 `(t, x, y)`: the resource around a working area: ground class 5 nodes in radius 0x14
/// (resource −6), ground class 2 own nodes in the working radius (−3 / −2), else the resource
/// sum of the area's square.
pub fn resource_count(e: &Engine, m: &mut dyn Manager, t: i32, x: i32, y: i32) -> i32 {
    let (cx, cy) = (cell(x), cell(y));
    let rad = m.working_radius(t);
    let res = rec(e.s.race, t, 10);
    if res == -6 {
        let n = m.spiral_end(0x14);
        let mut c = 0;
        for k in 0..n {
            let (dx, dy) = m.spiral_offset(k);
            if m.ground_is(5, x + dx, y + dy) {
                c += 1;
            }
        }
        return c;
    }
    if res == -3 || res == -2 {
        let n = m.spiral_end(rad);
        let mut c = 0;
        for k in 0..n {
            let (dx, dy) = m.spiral_offset(k);
            if m.ground_is(2, x + dx, y + dy) && m.own_node(x + dx, y + dy) {
                c += 1;
            }
        }
        return c;
    }
    m.resource_in_square(res, cx, cy)
}

/// 0x651c10 `(id, t)`: the resource at the building's working area: the cell sum (resources 0,
/// 0x10, 0x20, 0x50), the square sum (0x60, 0x70) or [`resource_count`] (−6, −3, −2).
pub fn resource_at_area(e: &Engine, m: &mut dyn Manager, id: i32, t: i32) -> i32 {
    let (wx, wy) = m.working_area(id);
    let (cx, cy) = (cell(wx), cell(wy));
    let res = rec(e.s.race, t, 10);
    match res {
        0 | 0x10 | 0x20 | 0x50 => m.cell_resource(res, cx, cy),
        0x60 | 0x70 => m.resource_in_square(res, cx, cy),
        -6 | -3 | -2 => resource_count(e, m, t, wx, wy),
        _ => 0,
    }
}

/// 0x651ef0 `(id, t)`: the working area moves to the best of the 3×3 cells around the building
/// when one has more of the resource than the current area (fishers: water cells, by water node
/// and distance). Whether it moved.
fn move_working_area(e: &Engine, m: &mut dyn Manager, id: i32, t: i32) -> bool {
    let (bx, by) = m.building_pos(id).unwrap_or((0, 0));
    let res = rec(e.s.race, t, 10);
    let mut best = resource_at_area(e, m, id, t);
    let (wx, wy) = m.working_area(id);
    let (mut tx, mut ty) = if best != 0 { (wx, wy) } else { (t, t) };
    let (cx, cy) = (cell(bx), cell(by));
    let mut v = 0;
    let mut found = false;
    for gx in cx - 1..=cx + 1 {
        for gy in cy - 1..=cy + 1 {
            match res {
                0x60 | 0x70 => v = m.resource_in_square(res, gx, gy),
                0 => {
                    if is_border(e, gx, gy) || m.cell_bit(0xe4, gx, gy) {
                        v = m.cell_resource(res, gx, gy);
                    }
                }
                0x10 | 0x20 | 0x50 => v = m.cell_resource(res, gx, gy),
                -1 => {
                    if t == 5 {
                        if m.cell_bit(0xe8, gx, gy) {
                            v = 500;
                        }
                        if m.cell_bit(0xe4, gx, gy) {
                            v = 1000;
                        }
                        let p = m.water_node(gx, gy);
                        v -= dist((by << 16) + bx, p);
                    }
                }
                -6 => v = m.cell_class(2, gx, gy),
                -3 => v = m.cell_class(5, gx, gy),
                -2 => v = m.cell_class(6, gx, gy),
                _ => {}
            }
            if v > best {
                best = v;
                tx = gx * 16 + 8;
                ty = gy * 16 + 8;
                if t == 5 {
                    let p = m.water_node(gx, gy);
                    tx = p & 0xffff;
                    ty = p >> 16;
                }
                found = true;
            }
        }
    }
    if found {
        m.set_working_area(id, tx, ty);
    }
    found
}

/// 0x652290 `(id, t)`: a planter's working area moves to the spot within 16 of the building
/// (in steps of half the working radius) with the most planting nodes, unless the current one
/// is full enough (0xb: > 50, 0x23: > 7, 0x24: > 110).
fn move_planting_area(e: &Engine, m: &mut dyn Manager, id: i32, t: i32) -> bool {
    let rad = m.working_radius(t);
    let (bx, by) = m.building_pos(id).unwrap_or((0, 0));
    let res = rec(e.s.race, t, 10);
    let (wx, wy) = m.working_area(id);
    let mut cur = m.plant_nodes(t, wx, wy);
    let (mut tx, mut ty) = (t, t);
    if cur != 0 {
        tx = wx;
        ty = wy;
        match t {
            0xb if cur > 0x32 => return false,
            0x23 if cur > 7 => return false,
            0x24 if cur > 0x6e => return false,
            _ => {}
        }
    }
    let step = rad / 2;
    if step <= 0 {
        return false; // [inf] the release would loop forever
    }
    let mut v = 0;
    let mut found = false;
    let mut gx = bx - 0x10;
    while gx <= bx + 0x10 {
        let mut gy = by - 0x10;
        while gy <= by + 0x10 {
            if res == -6 || res == -3 || res == -2 {
                v = m.plant_nodes(t, gx, gy);
            }
            if v > cur {
                tx = gx;
                ty = gy;
                cur = v;
                found = true;
            }
            gy += step;
        }
        gx += step;
    }
    if found {
        m.set_working_area(id, tx, ty);
    }
    found
}

/// 0x6527e0 `(t, x, y, limit)`: a productivity control or building event of type `t` lies
/// nearer than `limit` (0x652460), or `t` is being placed (+0x124).
fn near_same(e: &Engine, t: i32, x: i32, y: i32, limit: i32) -> bool {
    let p = (y << 16) + x;
    let mut best = 0;
    for en in &e.s.list {
        if en.state == 3 {
            continue;
        }
        let q = match &en.d {
            Data::Productivity(q) if q.kind == t => q.pos,
            Data::Building(b) if b.kind == t => b.pos,
            _ => continue,
        };
        let d = dist(p, q);
        if (d < best || best == 0) && d != 0 {
            best = d;
        }
    }
    if best > 0 && best < limit {
        return true;
    }
    bm(e).is_some_and(|b| b.placing_has(t))
}

// ---- CResourceManagerTask / CMaterialManagerTask / expandation ------------------------------------

fn res_task(e: &mut Engine, i: usize) -> &mut Resource {
    match &mut e.s.list[i].d {
        Data::Resource(r) => r,
        _ => unreachable!("resource"),
    }
}

/// vtbl+0x14 (0x6582e0): while the invoke event of its type is in state 2 the next entry runs
/// too (+0x24) and it keeps waiting. [inf] Without an invoke event the release reads through a
/// null pointer; here it goes on.
fn resource_check(e: &mut Engine, i: usize) -> bool {
    let t = res_task(e, i).kind;
    if let Some(k) = find_invoke(e, t)
        && let Data::Invoke(v) = &e.s.list[k].d
        && v.state == 2
    {
        e.s.list[i].go_on = 1;
        return false;
    }
    true
}

/// 0x6554b0 `(t)`: the invoke event of building type `t`.
pub fn find_invoke(e: &Engine, t: i32) -> Option<usize> {
    e.s.list.iter().position(|x| matches!(&x.d, Data::Invoke(v) if v.kind == t))
}

/// 0x64cb70 `(res)`: the building type of the first record (table order) whose resource is
/// `res`; 0 for −1 or none.
pub fn type_of_resource(res: i32) -> i32 {
    if res == -1 {
        return 0;
    }
    let rows = &super::tables::RECORDS;
    let mut k = 1;
    while k < rows.len() && rows[k][1] != 0 {
        if rows[k][10] == res {
            return rows[k][1];
        }
        k += 1;
    }
    0
}

/// 0x6525b0 `(t, out)`: for every productivity control of type `t` the 4×4 cells from
/// (cx − 2, cy − 2) around its node (`cy << 16 | cx`, x outer); the number of controls.
fn squares_of(e: &Engine, t: i32, out: &mut Vec<i32>) -> i32 {
    out.clear();
    let mut n = 0;
    for x in &e.s.list {
        if x.state == 3 {
            continue;
        }
        let Data::Productivity(p) = &x.d else { continue };
        if p.kind != t {
            continue;
        }
        let cx = (p.pos >> 4) & 0xfff;
        let cy = cell(p.pos >> 16);
        for gx in cx - 2..cx + 2 {
            for gy in cy - 2..cy + 2 {
                out.push((gy << 16) + gx);
            }
        }
        n += 1;
    }
    n
}

/// One of the resource task's chain wishes (0x657ae0): with fewer productivity controls of
/// `t` than [`enough`] the countdown runs; once a building of `need` is finished and the
/// countdown is below 0, a main problem (9) for `t` at the production position of `near`
/// (0x654a90), countdown 200.
fn resource_wish(e: &mut Engine, m: &mut dyn Manager, i: usize, t: i32, need: i32, near: i32) {
    res_task(e, i).countdown -= 1;
    if e.done_count(need) > 0 && res_task(e, i).countdown < 0 {
        let pos = problems::production_pos(e, m, near);
        let id = e.s.list[i].id;
        problems::push_main(e, id, 9, t, pos, 0);
        res_task(e, i).countdown = 200;
    }
}

/// vtbl+8 (0x657ae0), never asking for vtbl+0xc: the chain wishes (trees: woodcutters 7 when
/// a smith 0x16 is finished, near the 0x18; resource 0x50: mines 0xe likewise; resource 0x10:
/// Vikings' 0x19 / Trojans' 0x1c after a 0x2c, near the 0x16), then the scored cells: for a
/// ground class (−6, −4..−2) every own cell of the class (slot 0xf8, classes 2, 1, 5, 6) by
/// its count; for a resource every own cell with it (slot 0xb4) by its value, less the claims
/// of the type's productivity controls (0x6525b0; per claim 20 for stone, 5 for trees, else
/// 1), a border cell of trees / stone counting its square instead (slot 0xb0). The cells left
/// are summed; when nothing waits on it, below the threshold it wants land (+0x5c).
fn resource_run(e: &mut Engine, m: &mut dyn Manager, i: usize) -> bool {
    let res = res_task(e, i).res;
    res_task(e, i).filter = None;
    let kind = type_of_resource(res);
    res_task(e, i).kind = kind;
    let mut claims = Vec::new();
    squares_of(e, kind, &mut claims);
    if res == 0x70 && enough(e, m, 7) > e.prod_count(7) {
        resource_wish(e, m, i, 7, 0x16, 0x18);
    }
    if res == 0x50 && enough(e, m, 0xe) > e.prod_count(0xe) {
        resource_wish(e, m, i, 0xe, 0x16, 0x18);
    }
    if res == 0x10 {
        if e.s.race == 1 && !have_enough(e, m, 0x19) {
            resource_wish(e, m, i, 0x19, 0x2c, 0x16);
        }
        if e.s.race == 4 && !have_enough(e, m, 0x1c) {
            resource_wish(e, m, i, 0x1c, 0x2c, 0x16);
        }
    }
    let mut filter = Filter::new(8);
    let mut total = 0;
    if res == -6 || (-4..=-2).contains(&res) {
        let class = match res {
            -4 => 1,
            -3 => 5,
            -2 => 6,
            _ => 2,
        };
        let mut cells = Vec::new();
        m.own_class_cells(class, &mut cells);
        res_task(e, i).cells = cells.len() as i32;
        for c in &cells {
            if c.value as i16 > 0 {
                filter.insert(FEntry::resource(c.cx as i16 as i32, c.cy as i16 as i32, c.value as i16 as i32));
            }
        }
    } else {
        let mut cells = Vec::new();
        m.own_resource_cells(res, &mut cells);
        res_task(e, i).cells = cells.len() as i32;
        let per = match res {
            0x60 => 0x14,
            0x70 => 5,
            _ => 1,
        };
        for c in &cells {
            let (cx, cy, value) = (c.cx as i16 as i32, c.cy as i16 as i32, c.value as i16 as i32);
            let packed = (cy << 16) + cx;
            let claimed = per * claims.iter().filter(|&&q| q == packed).count() as i32;
            let mut v = value;
            if (res == 0x70 || res == 0x60) && is_border(e, cx, cy) {
                v = m.resource_in_square(res, cx, cy);
            }
            v -= claimed;
            if v > 0 {
                filter.insert(FEntry::resource(cx, cy, value));
                total += v;
            }
        }
    }
    let link = e.s.list[i].link;
    let r = res_task(e, i);
    r.filter = Some(filter);
    if link == 0 {
        if total < r.threshold {
            if r.wants == 0 {
                r.wants = 1;
            }
        } else {
            r.wants = 0;
        }
    }
    false
}

/// vtbl+0xc (0x658260): unless an expandation event exists, one for its resource; it waits.
fn resource_finish(e: &mut Engine, i: usize) {
    if e.find_kind(7).is_some() {
        return;
    }
    let id = e.s.list[i].id;
    let res = res_task(e, i).res;
    let new = e.push_linked(7, id, Data::ExpandEvent(ExpandEvent::new(res)));
    e.s.list[i].link = new;
}

fn mat(e: &mut Engine, i: usize) -> &mut Material {
    match &mut e.s.list[i].d {
        Data::Material(x) => x,
        _ => unreachable!("material"),
    }
}

/// vtbl+0x14 (0x656b50): the result of its step: 1 goes to the next state (1 → 2 → 3 → 4 → 1),
/// 2 back to 1; a pending event that vanished counts as 1; states 3 / 4 also take the castle
/// (0x2e) out of +0x110 as 1.
fn material_check(e: &mut Engine, i: usize) -> bool {
    let link = e.s.list[i].link;
    let st = mat(e, i).state;
    if (st == 3 || st == 4)
        && let Some(b) = bm_mut(e)
        && b.unask(0x2e)
    {
        mat(e, i).result = 1;
    }
    let r = mat(e, i).result;
    let next = match (st, r) {
        (1, 1) => Some(2),
        (1, 2) => Some(1),
        (2, 1) => Some(3),
        (2, 2) => Some(1),
        (3, 2) | (4, 2) => Some(1),
        (3, 1) => Some(4),
        (4, 1) => Some(1),
        _ => None,
    };
    if let Some(n) = next {
        mat(e, i).state = n;
        e.s.list[i].link = 0;
        return true;
    }
    if link != 0 && e.find_id(link).is_none() {
        mat(e, i).result = 1;
    }
    false
}

/// vtbl+8 (0x656870): every 5th call, whether the goal for its good (0x658590 at the tick)
/// exceeds the stock (0x653250).
fn material_run(e: &mut Engine, m: &mut dyn Manager, i: usize) -> bool {
    {
        let x = mat(e, i);
        x.countdown -= 1;
        if x.countdown > 0 {
            return false;
        }
        x.countdown = 5;
    }
    let good = mat(e, i).good;
    let tick = m.tick();
    let want = e.goal_amount(m, good, tick);
    mat(e, i).stock = 0;
    let have = problems::stock(e, m, good);
    mat(e, i).stock = have;
    want > have
}

/// vtbl+0xc (0x656930): per state: 1 a main problem (3) for the good (boards 0xb, stone 0x20
/// and 0x25 skip to state 2 and go on running); 2 a main problem (10) for its production at
/// 0x654a90; 3 an expandation event for the good's resource; 4 the combined expandation event
/// (0x64a5d0, not linked back). With an expandation event already there, states 3 and 4 go
/// back to 1 and go on running. Otherwise it waits on the new entry.
fn material_finish(e: &mut Engine, m: &mut dyn Manager, i: usize) {
    let (good, st) = {
        let x = mat(e, i);
        x.result = 0;
        (x.good, x.state)
    };
    let res = match good {
        7 => 0x70,
        0x20 => 0x60,
        0xb => 0,
        0x14 | 0x22 => 0x10,
        0xe => 0x30,
        0x21 => 0x40,
        0x23 => -6,
        _ => -4,
    };
    let id = e.s.list[i].id;
    let new = match st {
        1 => {
            if good == 0xb || good == 0x20 || good == 0x25 {
                mat(e, i).state = 2;
                return;
            }
            problems::push_main(e, id, 3, good, 0, 0)
        }
        2 => {
            let pos = problems::production_pos(e, m, good);
            problems::push_main(e, id, 10, good, 0, pos)
        }
        3 | 4 => {
            if e.find_kind(7).is_some() {
                mat(e, i).state = 1;
                return;
            }
            if st == 3 {
                e.push_linked(7, id, Data::ExpandEvent(ExpandEvent::new(res)))
            } else {
                let ev = combined_expansion(e);
                e.push(7, Data::ExpandEvent(ev))
            }
        }
        _ => {
            e.s.list[i].state = 1;
            return;
        }
    };
    e.s.list[i].link = new;
    e.s.list[i].state = 1;
}

/// vtbl+8 (0x64a330): the border cells (slot 0x100); state 0 (no answer yet) restarts the cycle
/// after the countdown; state 1 moves to the next resource; with go and the countdown below 490
/// and no expandation event, one for the next resource (countdown 499).
fn expand_mgr_run(e: &mut Engine, m: &mut dyn Manager, i: usize) -> bool {
    let id = e.s.list[i].id;
    let mut border = Vec::new();
    m.border_cells(&mut border);
    let have7 = e.find_kind(7).is_some();
    let Data::ExpandMgr(x) = &mut e.s.list[i].d else { return false };
    x.border = border;
    match x.state {
        0 => {
            if x.countdown < 10 {
                x.index += 1;
                x.state = 1;
                x.go = true;
                x.countdown = 500;
            }
        }
        st => {
            if st == 1 {
                x.index += 1;
                x.go = true;
                if x.index > 4 {
                    x.index = 0;
                }
            }
            if x.go && x.countdown < 0x1ea && !have7 {
                let res = x.res[x.index as usize];
                let new = e.push_linked(7, id, Data::ExpandEvent(ExpandEvent::new(res)));
                e.s.list[i].link = new;
                let Data::ExpandMgr(x) = &mut e.s.list[i].d else { return false };
                x.countdown = 499;
                x.state = 0;
                x.go = false;
                return false;
            }
        }
    }
    if let Data::ExpandMgr(x) = &mut e.s.list[i].d {
        x.countdown -= 1;
    }
    false
}

fn xev(e: &mut Engine, i: usize) -> &mut ExpandEvent {
    match &mut e.s.list[i].d {
        Data::ExpandEvent(x) => x,
        _ => unreachable!("expandation event"),
    }
}

/// vtbl+0x14 (0x64ae20): the sub problem answered: 1 or 2 to its waiting entry, then done.
fn expand_event_check(e: &mut Engine, i: usize) -> bool {
    let (result, link) = (xev(e, i).result, e.s.list[i].link);
    if result == 1 {
        if link != 0 {
            tell(e, link, 1);
            e.s.list[i].link = 0;
            e.s.list[i].state = 3;
        }
        return false;
    }
    if result == 2 {
        if link != 0 {
            tell(e, link, 2);
        }
        e.s.list[i].state = 3;
    }
    false
}

/// 0x653620 `(cx, cy)`: the border cell (of the expandation manager) nearest to the cell,
/// nearer than 100; packed `cy << 16 | cx`, else 0.
fn nearest_border(e: &Engine, cx: i32, cy: i32) -> i32 {
    let mut best = 100;
    let (mut bx, mut by) = (0, 0);
    if let Some(x) = expand_mgr(e) {
        for &p in &x.border {
            let (px, py) = (p & 0xffff, p >> 16);
            let d = crate::spiral::grid_distance(px - cx, py - cy);
            if d < best {
                best = d;
                bx = px;
                by = py;
            }
        }
    }
    (by << 16) + bx
}

/// vtbl+8 (0x64a920): the first run fetches the foreign cells: a ground class resource (−3, −2,
/// −4, −6: classes 5, 6, 1, 2) through slot 0xfc into +0x3c, any other through slot 0xbc into
/// +0x30 (none: done). Later the filter is made once (5 entries): every cell whose nearest
/// border cell is at most the allowance + 40 away, scored by its value − 3 · that distance.
/// An empty filter widens the allowance by 50; else it runs on (vtbl+0xc) when no other
/// expandation event and no castle (0x2e / 0x2f) building event exist (0x64a4b0).
fn expand_event_run(e: &mut Engine, m: &mut dyn Manager, i: usize) -> bool {
    let (empty, res) = {
        let x = xev(e, i);
        (x.class_cells.is_empty() && x.cells.is_empty(), x.res)
    };
    if empty {
        let class = match res {
            -3 => Some(5),
            -2 => Some(6),
            -4 => Some(1),
            -6 => Some(2),
            _ => None,
        };
        let mut cells = Vec::new();
        match class {
            Some(c) => {
                m.foreign_class_cells(c, &mut cells);
                let x = xev(e, i);
                x.classes = true;
                x.class_cells = cells;
            }
            None => {
                m.foreign_resource_cells(res, &mut cells);
                let none = cells.is_empty();
                let x = xev(e, i);
                x.classes = false;
                x.cells = cells;
                if none {
                    e.s.list[i].state = 3;
                }
            }
        }
        return false;
    }
    if xev(e, i).filter.is_none() {
        let mut filter = Filter::new(5);
        let (cells, radius) = {
            let x = xev(e, i);
            (if x.classes { x.class_cells.clone() } else { x.cells.clone() }, x.radius)
        };
        for c in &cells {
            let (cx, cy) = (c.cx as i16 as i32, c.cy as i16 as i32);
            let near = nearest_border(e, cx, cy);
            let d = dist((cy << 16) + cx, near);
            if d <= radius + 0x28 {
                filter.insert(FEntry::landscape(cx, cy, c.value as i16 as i32, near, d));
            }
        }
        xev(e, i).filter = Some(filter);
    }
    if xev(e, i).filter.as_ref().is_some_and(|f| f.count == 0) {
        xev(e, i).radius += 0x32;
        return false;
    }
    alone(e, i)
}

/// 0x64a4b0 `(event)` on the expandation manager: no other expandation event and no building
/// event of a castle (0x2e) or 0x2f.
fn alone(e: &Engine, i: usize) -> bool {
    let own = e.s.list[i].id;
    !e.s.list.iter().any(|x| match &x.d {
        Data::ExpandEvent(_) => x.id != own,
        Data::Building(b) => b.kind == 0x2e || b.kind == 0x2f,
        _ => false,
    })
}

/// 0x64a5d0: the combined expandation event (resource −5, no link): its filter (5) takes from
/// every resource task the first scored cell whose +0x18 is nonzero as a landscape entry
/// (+0x18 / +0x1c copied), its value scaled by the resource (stone / trees · 2, 0x10 · 4, 0x20
/// · 3; any other keeps the previous factor, 0 at first; the release multiplies in float, exact
/// here). [inf] The resource tasks' entries are 0x18-byte `CResourceFilter`s: +0x18 / +0x1c lie
/// past them (heap block header, nonzero, in the release). Here the first entry is taken with
/// border and distance 0.
fn combined_expansion(e: &Engine) -> ExpandEvent {
    let mut ev = ExpandEvent::new(-5);
    let mut filter = Filter::new(5);
    let mut factor = 0;
    for x in &e.s.list {
        let Data::Resource(r) = &x.d else { continue };
        let Some(f) = &r.filter else { continue };
        let Some(c) = f.entries().first() else { continue };
        factor = match r.res {
            0x60 | 0x70 => 2,
            0x10 => 4,
            0x20 => 3,
            _ => factor,
        };
        filter.insert(FEntry::landscape(c.cx, c.cy, c.value * factor, 0, 0));
    }
    ev.filter = Some(filter);
    ev
}

/// vtbl+0xc (0x64ada0): a main problem of kind 0xb for its resource, linked to it (+0x4c its
/// id); it waits.
fn expand_event_finish(e: &mut Engine, i: usize) {
    let id = e.s.list[i].id;
    let res = xev(e, i).res;
    let new = problems::push_main(e, id, 0xb, res, 0, 0);
    xev(e, i).sub = new;
    e.s.list[i].state = 1;
}
