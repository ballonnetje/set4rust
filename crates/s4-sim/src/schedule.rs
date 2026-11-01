//! The logic-update schedule of `CMapObjectMgr` (MapObjects/MapObjectMgr.cpp): a ring of 32
//! slots, each a deque of entity ids, plus a parallel ring of "killed" deques.
//!
//! * `RegisterForLogicUpdate(delta, id)` (release 0x56d5d0): clamp `delta` to 1..31, slot =
//!   `(cursor + delta) % 32`; if the entity's last slot is set and is not the current slot, remove it
//!   from there; **push_front** into the new slot.
//! * `LogicUpdate` (release 0x56d280): fold the slot's ids and all slot sizes into the CRC, pop_front
//!   until the current slot is empty (each popped entity gets its update), then destroy every
//!   entity in the slot's killed deque, then advance the cursor.
//! * `Kill` (release 0x56d030) queues the id in killed slot `(cursor + 31) % 32`, i.e. the entity is
//!   destroyed (and its id freed) at the end of the 31st following `LogicUpdate`.
//!
//! The push_front + pop_front order means entities registered for the same tick run in reverse
//! registration order. That order is observable (RNG draws, "first come" searches), so it is kept.

use std::collections::VecDeque;

use crate::hash::StateHasher;

/// `MAX_LOGIC_UPDATE_SLOTS`.
pub const SLOTS: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Schedule {
    /// `m_vLogicUpdateSlots`.
    pub(crate) slots: Vec<VecDeque<u16>>,
    /// `m_vKilledEntities`.
    pub(crate) killed: Vec<VecDeque<u16>>,
    /// `m_iCurrentLogicTick` (0..31).
    pub(crate) cursor: u8,
    /// `m_uCRCLogicUpdate`: the original's desync checksum, starts at 1.
    pub(crate) crc: u32,
}

impl Default for Schedule {
    fn default() -> Self {
        Schedule { slots: vec![VecDeque::new(); SLOTS], killed: vec![VecDeque::new(); SLOTS], cursor: 0, crc: 1 }
    }
}

impl Schedule {
    pub fn cursor(&self) -> u8 {
        self.cursor
    }

    /// `CMapObjectMgr::CRCLogicUpdate`.
    pub fn crc(&self) -> u32 {
        self.crc
    }

    pub fn slot(&self, slot: u8) -> &VecDeque<u16> {
        &self.slots[slot as usize]
    }

    /// Slot an entity registered with `delta` lands in (after the original's clamping).
    pub fn target_slot(&self, delta: i32) -> u8 {
        let d = if delta > 0 { delta.min(SLOTS as i32 - 1) } else { 1 };
        ((self.cursor as i32 + d) % SLOTS as i32) as u8
    }

    /// Queue part of `RegisterForLogicUpdate`; returns the slot. The caller passes the entity's
    /// `last_slot` and stores the returned slot there (`IAnimatedEntity::SetLastLogicUpdate`).
    pub(crate) fn register(&mut self, delta: i32, id: u16, last_slot: Option<u8>) -> u8 {
        let slot = self.target_slot(delta);
        if let Some(last) = last_slot
            && last != self.cursor
        {
            self.unregister(last, id);
        }
        self.slots[slot as usize].push_front(id);
        slot
    }

    /// `UnRegisterFromLogicUpdate(slot, id)`: removes the first occurrence; returns whether found.
    pub(crate) fn unregister(&mut self, slot: u8, id: u16) -> bool {
        let q = &mut self.slots[slot as usize];
        match q.iter().position(|&e| e == id) {
            Some(p) => {
                q.remove(p);
                true
            }
            None => false,
        }
    }

    /// The `Kill` half: destroy at the end of the update 31 ticks from now.
    pub(crate) fn queue_destroy(&mut self, id: u16) {
        let slot = (self.cursor as usize + SLOTS - 1) % SLOTS;
        self.killed[slot].push_front(id);
    }

    /// CRC step at the start of `LogicUpdate`: the current slot's ids (as u16 LE, the original hashes
    /// them in 1024-entry chunks, which Adler-32 makes equivalent to one pass), then the 32 slot
    /// sizes as i32 LE.
    pub(crate) fn update_crc(&mut self) {
        let mut a = Adler32::new(self.crc);
        for &id in &self.slots[self.cursor as usize] {
            a.update(&id.to_le_bytes());
        }
        for q in &self.slots {
            a.update(&(q.len() as i32).to_le_bytes());
        }
        self.crc = a.finish();
    }

    pub(crate) fn hash(&self, h: &mut StateHasher) {
        h.u8(self.cursor);
        h.u32(self.crc);
        for q in self.slots.iter().chain(&self.killed) {
            h.u32(q.len() as u32);
            for &id in q {
                h.u16(id);
            }
        }
    }
}

/// zlib Adler-32 as in `Adler32(buf, len, adler)` (globals.cpp 0x2f03690). [ours] The modulo is
/// taken once per 5552 bytes (zlib's NMAX: the sums cannot overflow before) and at the end, not
/// per byte; the same result.
struct Adler32 {
    a: u32,
    b: u32,
    /// Bytes added since the last modulo.
    n: u32,
}

impl Adler32 {
    fn new(start: u32) -> Adler32 {
        Adler32 { a: start & 0xFFFF, b: start >> 16, n: 0 }
    }
    fn update(&mut self, data: &[u8]) {
        for &x in data {
            self.a += x as u32;
            self.b += self.a;
            self.n += 1;
            if self.n == 5552 {
                self.a %= 65521;
                self.b %= 65521;
                self.n = 0;
            }
        }
    }
    fn finish(&self) -> u32 {
        ((self.b % 65521) << 16) | (self.a % 65521)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adler32_reference() {
        let mut a = Adler32::new(1);
        a.update(b"Wikipedia");
        assert_eq!(a.finish(), 0x11E60398);
    }

    /// The deferred modulo gives the per-byte modulo's result, from the largest start sums, over
    /// 0xff bytes (the worst case for overflow) fed in pieces.
    #[test]
    fn adler32_deferred_modulo() {
        let per_byte = |start: u32, data: &[u8]| {
            let (mut a, mut b) = (start & 0xFFFF, start >> 16);
            for &x in data {
                a = (a + x as u32) % 65521;
                b = (b + a) % 65521;
            }
            (b << 16) | a
        };
        let data: Vec<u8> = (0..40_000u32).map(|i| if i % 7 == 0 { (i * 31) as u8 } else { 0xff }).collect();
        for start in [1, 0xFFF0_FFF0, (65520 << 16) | 65520] {
            let mut a = Adler32::new(start);
            for piece in data.chunks(3) {
                a.update(piece);
            }
            assert_eq!(a.finish(), per_byte(start, &data));
        }
    }

    #[test]
    fn register_clamps_and_moves() {
        let mut s = Schedule::default();
        assert_eq!(s.target_slot(0), 1);
        assert_eq!(s.target_slot(40), 31);
        let a = s.register(3, 10, None);
        let b = s.register(3, 11, None);
        assert_eq!((a, b), (3, 3));
        // push_front: 11 runs before 10.
        assert_eq!(s.slot(3).iter().copied().collect::<Vec<_>>(), [11, 10]);
        // Re-registering 10 from another slot removes it from slot 3.
        let c = s.register(5, 10, Some(3));
        assert_eq!(c, 5);
        assert_eq!(s.slot(3).iter().copied().collect::<Vec<_>>(), [11]);
        // A last slot equal to the cursor is not searched (the entity was just popped from it).
        s.slots[0].push_back(12);
        s.register(1, 12, Some(0));
        assert_eq!(s.slot(0).len(), 1);
    }
}
