//! `CRandom16` / `CRandom16Ex` (Logic/CRandom16.cpp): a 32-bit LCG whose high half is the result.

use crate::hash::StateHasher;

/// Seed of the game RNG, `CGameData::CGameData` → `CRandom16Ex::Init(&m_sRandom, 290671, 0)`.
pub const GAME_SEED: u32 = 290_671;
/// Seed of the animal manager's own RNG, `CAnimalMgr` ctor → `Init(this + 380, 688265585, 0)`.
pub const ANIMAL_SEED: u32 = 688_265_585;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Random16 {
    /// `m_uSeed`.
    pub seed: u32,
    /// `m_uIteration`: number of `rand` calls since `init`.
    pub calls: u32,
}

impl Random16 {
    /// `CRandom16Ex::Init(seed, iteration)`.
    pub fn new(seed: u32, calls: u32) -> Random16 {
        Random16 { seed, calls }
    }

    /// `CRandom16::Rand` (0x12fd540): `seed = 1812433253 * seed + 1`, returns `seed >> 16`.
    pub fn rand(&mut self) -> u32 {
        self.calls = self.calls.wrapping_add(1);
        self.seed = self.seed.wrapping_mul(1_812_433_253).wrapping_add(1);
        self.seed >> 16
    }

    /// `CRandom16::PercentValue` (0x1301600): `p` percent on the 0..65536 scale of [`Self::rand`].
    pub fn percent_value(p: u32) -> u32 {
        (p << 16) / 100
    }

    pub(crate) fn hash(&self, h: &mut StateHasher) {
        h.u32(self.seed);
        h.u32(self.calls);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_original_sequence() {
        let mut r = Random16::new(GAME_SEED, 0);
        assert_eq!([r.rand(), r.rand(), r.rand()], [16747, 58658, 43838]);
        assert_eq!((r.seed, r.calls), (0xab3ed752, 3));
        assert_eq!(Random16::percent_value(50), 32768);
    }
}
