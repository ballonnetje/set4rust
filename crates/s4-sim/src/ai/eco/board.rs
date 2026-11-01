//! The reservation board (0x64f4a0, the engine's +0xc; the scheduler's +0): places the planner
//! has taken for planned buildings, so later alternatives keep clear of them, and the planning
//! lock.
//!
//! Entries (`CReserveEntry` 0x106cfd0, 0x2c bytes; `CGrpReserveEntry` 0x106cfb8, 0x34 bytes):
//! +0xc type, +0x10 / +0x14 the place (cell or node coordinates, by its maker), +0x18 the id
//! (+0xc counter), +0x1c a score, +0x20 the node, +0x28 a class byte; group entries add +0x2c
//! the group and +0x30 their children. Groups are decimal paths: group `g`'s children are
//! `10·g + k` (0x64fc30 tests the prefix relation). Lists: +0 plain entries, +4 the group tree's
//! top entries, +8 `CRefGrpEntry` (0x106cfc4: id → entry) in id order from +0x10 (1).

/// One entry.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Entry {
    pub a: i32,
    pub b: i32,
    pub c: i32,
    pub id: i32,
    pub score: i32,
    pub d: i32,
    pub e: i8,
    pub group: i32,
    pub children: Vec<usize>,
}

/// One record of 0x64f9f0 / 0x6519d0 (0x2c bytes, read by 0x64fd50): +0xc type (0 ends the
/// list), +0x10 / +0x14 place, +0x18 id, +0x20 node, +0x24 a node whose 8×8 block is taken.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Record {
    pub a: i32,
    pub b: i32,
    pub c: i32,
    pub id: i32,
    pub d: i32,
    pub block: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Board {
    /// The entries (an arena; the lists hold indices).
    pub entries: Vec<Entry>,
    /// +0 plain entries, +4 the top group entries, +8 (id, entry).
    pub plain: Vec<usize>,
    pub top: Vec<usize>,
    pub refs: Vec<(i32, usize)>,
    /// +0xc the id counter, +0x10 the first id of +8.
    pub counter: i32,
    pub base: i32,
    /// +0x14 the main problem that plans (0 none), +0x18 the one waiting for it.
    pub lock: i32,
    pub waiting: i32,
}

impl Default for Board {
    fn default() -> Self {
        Board::new()
    }
}

/// 0x64fc30 `(a, b)`: one is a decimal prefix of the other (`b / 10^k == a` or the reverse).
pub fn prefix(a: i32, b: i32) -> bool {
    if a < b {
        let mut v = b;
        loop {
            v /= 10;
            if v == 0 {
                break;
            }
            if v == a {
                return true;
            }
        }
    }
    if a <= b {
        return a == b;
    }
    let mut v = a;
    loop {
        v /= 10;
        if v == 0 {
            return a == b;
        }
        if v == b {
            return true;
        }
    }
}

impl Board {
    /// The ctor (0x64f4a0).
    pub fn new() -> Board {
        Board { entries: Vec::new(), plain: Vec::new(), top: Vec::new(), refs: Vec::new(), counter: 0, base: 1, lock: 0, waiting: 0 }
    }

    /// 0x64f530 `(a, b, c, d, e, score, group)`: a new entry with the next id; without a group a
    /// plain one, else a group entry hung under the deepest group on its path (the top list
    /// without one; not at all when its group already exists) and listed in +8. Its id.
    #[allow(clippy::too_many_arguments)]
    pub fn reserve(&mut self, a: i32, b: i32, c: i32, d: i32, e: i8, score: i32, group: i32) -> i32 {
        self.counter += 1;
        let id = self.counter;
        let k = self.entries.len();
        self.entries.push(Entry { a, b, c, id, score, d, e, group, children: Vec::new() });
        if group == 0 {
            self.plain.push(k);
            return id;
        }
        let (exact, parent) = self.find(group, 0);
        if !exact {
            match parent {
                Some(p) => self.entries[p].children.push(k),
                None => self.top.push(k),
            }
        }
        self.refs.push((id, k));
        id
    }

    /// 0x64f7a0 `(group, out, a)`: the group entry of `group` (and type `a` unless 0) in the
    /// tree: true and it, or false and the deepest entry on the path to it (None if not even a
    /// top entry is on it).
    pub fn find(&self, group: i32, a: i32) -> (bool, Option<usize>) {
        for &t in &self.top {
            let e = &self.entries[t];
            if e.group == group && (e.a == a || a == 0) {
                return (true, Some(t));
            }
            if prefix(e.group, group) {
                let mut cur = t;
                let mut i = 0;
                while i < self.entries[cur].children.len() {
                    let c = self.entries[cur].children[i];
                    let ce = &self.entries[c];
                    if ce.group == group && (ce.a == a || a == 0) {
                        return (true, Some(c));
                    }
                    if prefix(ce.group, group) {
                        cur = c;
                        i = 0;
                    } else {
                        i += 1;
                    }
                }
                return (false, Some(cur));
            }
        }
        (false, None)
    }

    /// The exact match of [`Board::find`] with its group checked again, as the callers do:
    /// its id, else 0 (0x64f6f0 for `a` ≠ 0 or 0).
    pub fn id_of(&self, group: i32, a: i32) -> i32 {
        match self.find(group, a) {
            (true, Some(k)) => {
                let e = &self.entries[k];
                if e.group == group && (e.a == a || a == 0) { e.id } else { 0 }
            }
            _ => 0,
        }
    }

    /// 0x64fcb0 `(id)`: the group entry with this id (through +8, from position id − base
    /// downwards).
    pub fn get(&self, id: i32) -> Option<&Entry> {
        let n = self.refs.len() as i32;
        let mut i = id - self.base;
        if i >= n && n > 0 {
            i = n - 1;
        }
        if i < 0 || i >= n {
            return None;
        }
        loop {
            let (rid, k) = self.refs[i as usize];
            if rid == id {
                return Some(&self.entries[k]);
            }
            if i < 1 {
                return None;
            }
            i -= 1;
        }
    }

    /// 0x64fb70 `(id)`: its place, (0, 0) without.
    pub fn place(&self, id: i32) -> (i32, i32) {
        self.get(id).map_or((0, 0), |e| (e.b, e.c))
    }

    /// 0x64f8d0 `(group, parent)`: every group entry on `group`'s path (all with 0) under
    /// `parent` (the top list without) is taken out with its subtree.
    pub fn remove(&mut self, group: i32, parent: Option<usize>) {
        let mut i = 0;
        loop {
            let len = match parent {
                Some(p) => self.entries[p].children.len(),
                None => self.top.len(),
            };
            if i >= len {
                return;
            }
            let k = match parent {
                Some(p) => self.entries[p].children[i],
                None => self.top[i],
            };
            if group == 0 || prefix(self.entries[k].group, group) {
                if !self.entries[k].children.is_empty() {
                    self.remove(group, Some(k));
                }
                match parent {
                    Some(p) => {
                        self.entries[p].children.remove(i);
                    }
                    None => {
                        self.top.remove(i);
                    }
                }
            } else {
                i += 1;
            }
        }
    }

    /// 0x64f9f0 `(group, out)`: after the records already in `out`, every group entry on
    /// `group`'s path: the top entries on it and, under each, the children on it (going down
    /// at a proper prefix, on along the siblings at the group itself). True if any; a top
    /// entry of the group itself ends it. The child index is not reset between top entries
    /// (release).
    pub fn path_records(&self, group: i32, out: &mut Vec<Record>) -> bool {
        let rec = |e: &Entry| Record { a: e.a, b: e.b, c: e.c, id: e.id, d: e.d, block: 0 };
        let mut found = false;
        let mut i = 0;
        for &t in &self.top {
            let e = &self.entries[t];
            if !prefix(e.group, group) {
                continue;
            }
            out.push(rec(e));
            found = true;
            if e.group == group {
                return true;
            }
            let mut cur = t;
            while i < self.entries[cur].children.len() {
                let c = self.entries[cur].children[i];
                let ce = &self.entries[c];
                if prefix(ce.group, group) {
                    out.push(rec(ce));
                    if ce.group == group {
                        i += 1;
                    } else {
                        cur = c;
                        i = 0;
                    }
                } else {
                    i += 1;
                }
            }
        }
        found
    }

    /// 0x64fdf0 `(id)`: take the lock when free and nobody else waits.
    pub fn try_lock(&mut self, id: i32) -> bool {
        if self.lock == 0 && (self.waiting == 0 || self.waiting == id) {
            self.lock = id;
            self.waiting = 0;
            return true;
        }
        false
    }

    /// 0x64fe20 `(id)` (and the same lines in the main problem): the holder lets go, and the
    /// group tree and its index are emptied (the plain entries stay).
    pub fn unlock(&mut self, id: i32) -> bool {
        if self.lock != id {
            return false;
        }
        self.lock = 0;
        self.remove(0, None);
        self.refs.clear();
        self.base = 1;
        self.counter = 0;
        if self.plain.is_empty() {
            self.entries.clear();
        }
        true
    }
}
