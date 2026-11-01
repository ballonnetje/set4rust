//! State fingerprint for replay and desync tests: 64-bit FNV-1a over a canonical little-endian
//! byte stream. Not the original's checksum (that is `CMapObjectMgr::CRCLogicUpdate`, an
//! Adler-32 over the update queues, which [`crate::Schedule`] also reproduces).
//!
//! The hasher runs two FNV lanes over the same bytes: the whole stream, and the current part (for
//! [`crate::World::hashes`], the state hash and the per-subsystem hashes in one pass). FNV is one
//! multiply per byte in a chain, so the second, independent chain costs next to nothing.

const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const PRIME: u64 = 0x0000_0100_0000_01b3;

#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StateHasher {
    whole: u64,
    part: u64,
}

impl Default for StateHasher {
    fn default() -> Self {
        StateHasher { whole: OFFSET, part: OFFSET }
    }
}

impl StateHasher {
    pub fn finish(&self) -> u64 {
        self.whole
    }

    /// The part lane starts again (the whole stream goes on).
    pub(crate) fn start_part(&mut self) {
        self.part = OFFSET;
    }

    /// The bytes since [`Self::start_part`], hashed on their own.
    pub(crate) fn part(&self) -> u64 {
        self.part
    }

    pub fn bytes(&mut self, b: &[u8]) {
        let (mut h, mut p) = (self.whole, self.part);
        for &x in b {
            h ^= x as u64;
            h = h.wrapping_mul(PRIME);
            p ^= x as u64;
            p = p.wrapping_mul(PRIME);
        }
        self.whole = h;
        self.part = p;
    }

    pub fn u8(&mut self, v: u8) {
        self.bytes(&[v]);
    }
    pub fn u16(&mut self, v: u16) {
        self.bytes(&v.to_le_bytes());
    }
    pub fn u32(&mut self, v: u32) {
        self.bytes(&v.to_le_bytes());
    }
    pub fn u64(&mut self, v: u64) {
        self.bytes(&v.to_le_bytes());
    }
    pub fn bool(&mut self, v: bool) {
        self.u8(v as u8);
    }

    /// Length-prefixed so that adjacent sequences cannot alias.
    pub fn u16s(&mut self, v: &[u16]) {
        self.u32(v.len() as u32);
        for &x in v {
            self.u16(x);
        }
    }

    pub fn opt_u8(&mut self, v: Option<u8>) {
        match v {
            Some(x) => {
                self.u8(1);
                self.u8(x);
            }
            None => self.u8(0),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn fnv1a_reference_values() {
        let mut h = super::StateHasher::default();
        assert_eq!(h.finish(), 0xcbf29ce484222325);
        h.bytes(b"a");
        assert_eq!(h.finish(), 0xaf63dc4c8601ec8c);
    }

    /// The part lane equals a fresh hasher over the part's bytes; the whole lane is unaffected.
    #[test]
    fn part_lane() {
        let mut h = super::StateHasher::default();
        h.bytes(b"head");
        h.start_part();
        h.bytes(b"a");
        assert_eq!(h.part(), 0xaf63dc4c8601ec8c);
        let mut w = super::StateHasher::default();
        w.bytes(b"heada");
        assert_eq!(h.finish(), w.finish());
    }
}
