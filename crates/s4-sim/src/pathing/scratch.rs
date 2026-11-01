//! Working memory of the path searches, kept between searches ([ours]: the original allocates
//! nothing per search either). A search used to allocate and zero its node tables (40 KB for
//! `CAStar64`, 288 KB for `CAStarTiling`) every time; now a table is "zeroed" by bumping a
//! generation number, and an entry not written in this search reads as its default — the same
//! values as a fresh zeroed table, so the searches and their results are unchanged.

/// A table whose entries read as `T::default()` until written in the current generation.
pub(super) struct Stamped<T> {
    generation: u32,
    stamps: Vec<u32>,
    values: Vec<T>,
}

impl<T: Copy + Default> Stamped<T> {
    pub(super) const fn new() -> Self {
        Stamped { generation: 0, stamps: Vec::new(), values: Vec::new() }
    }

    /// Every entry back to its default; at least `len` entries.
    pub(super) fn reset(&mut self, len: usize) {
        if self.stamps.len() < len {
            self.stamps.resize(len, 0);
            self.values.resize(len, T::default());
        }
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            self.stamps.fill(0);
            self.generation = 1;
        }
    }

    #[inline]
    pub(super) fn get(&self, i: usize) -> T {
        if self.stamps[i] == self.generation { self.values[i] } else { T::default() }
    }

    #[inline]
    pub(super) fn set(&mut self, i: usize, v: T) {
        self.stamps[i] = self.generation;
        self.values[i] = v;
    }

    /// The entry to change in place (its default first when not written yet).
    #[inline]
    pub(super) fn get_mut(&mut self, i: usize) -> &mut T {
        if self.stamps[i] != self.generation {
            self.stamps[i] = self.generation;
            self.values[i] = T::default();
        }
        &mut self.values[i]
    }
}
