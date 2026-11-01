//! `CFilter` (vtable 0x106ccb4, ctor 0x6481b0): a best-first list of scored entries. The entries
//! (`IFilterEntry` 0x106cca4) score through their vtbl+8 as an x87 value that the filter stores
//! as a single float and truncates (`fstp dword; cvttss2si`):
//! * `CResourceFilter` (0x106d04c, 0x18 bytes, score 0x658430): +0xc cx, +0x10 cy, +0x14 value;
//!   score = value.
//! * `CFilterResourceLandscape` (0x106cd20, 0x20 bytes, ctor 0x64b050, score 0x64b0d0): +0xc cx,
//!   +0x10 cy, +0x14 value, +0x18 the nearest border cell, +0x1c its distance; score = value −
//!   3·distance + 1000.
//! * `CFilterPlaceScoring` (0x106cc88, score 0x6484c0): a double at +0x20 (see the solver).
//!
//! The int scores are exact through the float as long as they stay below 2^24 (they are cell
//! sums and distances); [`Entry::score`] holds the truncated value the release compares.

/// One entry: what the users read and its score.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Entry {
    /// +0xc / +0x10 the cell (or the payload of a place score).
    pub cx: i32,
    pub cy: i32,
    /// +0x14 the value.
    pub value: i32,
    /// +0x18 / +0x1c (landscape entries: the nearest border cell, its distance; place scores:
    /// their own words).
    pub w18: i32,
    pub w1c: i32,
    /// The truncated score.
    pub score: i32,
}

impl Entry {
    /// A `CResourceFilter` entry.
    pub fn resource(cx: i32, cy: i32, value: i32) -> Entry {
        Entry { cx, cy, value, w18: 0, w1c: 0, score: value }
    }

    /// A `CFilterResourceLandscape` entry.
    pub fn landscape(cx: i32, cy: i32, value: i32, border: i32, dist: i32) -> Entry {
        Entry { cx, cy, value, w18: border, w1c: dist, score: value - 3 * dist + 1000 }
    }
}

/// An entry's truncated score.
pub trait Scored {
    fn score(&self) -> i32;
}

impl Scored for Entry {
    fn score(&self) -> i32 {
        self.score
    }
}

/// The filter: the list (+4, a `CDynList` whose count is its real length), +8 the number of
/// valid entries (at most +0xc), +0xc the maximum, +0x10 the lowest score still taken.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Filter<T = Entry> {
    pub list: Vec<T>,
    pub count: i32,
    pub max: i32,
    pub min: i32,
}

impl<T: Scored> Filter<T> {
    pub fn new(max: i32) -> Filter<T> {
        Filter { list: Vec::new(), count: 0, max, min: 0 }
    }

    /// 0x6482b0: below the minimum the entry is dropped. Else it goes before the first entry
    /// with a lower score (after equal ones). Walking on, once more than `max` entries have been
    /// passed the minimum becomes that entry's score and the walk ends (the list is not cut);
    /// an entry not yet placed is then dropped. At the end of the list it is appended. +8
    /// follows the list length while that is at most `max`. Whether it was taken.
    pub fn insert(&mut self, e: T) -> bool {
        let score = e.score();
        if score < self.min {
            return false;
        }
        let mut placed = false;
        let mut k = 1;
        let mut i = 0;
        let mut e = Some(e);
        while i < self.list.len() {
            let node = self.list[i].score();
            if node < score && !placed {
                let e = e.take().expect("placed once");
                self.list.insert(i, e);
                placed = true;
                i += 1;
            }
            if k > self.max {
                self.min = node;
                return placed;
            }
            k += 1;
            i += 1;
        }
        if let Some(e) = e {
            self.list.push(e);
            placed = true;
        }
        if self.list.len() as i32 <= self.max {
            self.count = self.list.len() as i32;
        }
        placed
    }

    /// The valid entries (the first +8).
    pub fn entries(&self) -> &[T] {
        &self.list[..(self.count.max(0) as usize).min(self.list.len())]
    }
}
