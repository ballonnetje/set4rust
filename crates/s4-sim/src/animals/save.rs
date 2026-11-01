//! Saved land animals (savegame chunk 165, `CAnimalMgr` save / load 0x52d8c0 / 0x52dbc0), as test
//! oracles. The chunk is a text stream: version 1, the count, then per animal its persistence
//! record, which the load constructors read in this order:
//!
//! * class id (4), `IEntity` (0x566e00): version 2, id, type, packed position, health, object type
//!   128, race | player << 4, flags, unique id;
//! * `IAnimatedEntity` (0x5329b0): version 1, frame +0x24, +0x25, job +0x26, +0x28, +0x2a, +0x2c,
//!   logic slot +0x30, the number of queued entity events (each a persistence record);
//! * `IMovingEntity` (0x56dd80): version 1, direction +0x40, step +0x41, +0x42, +0x48, whether a
//!   walking follows; the walking (0x5baf10): control id, version 1, type, flags, 15 ints, an end
//!   marker;
//! * `CAnimal` (0x52b950): version 2, step flags +0x68, speed +0x69, task type +0x6a, +0x6b,
//!   frames +0x6c, +0x6d, +0x78, +0x70, state +0x58;
//! * `CLandAnimal` (0x531460): version 3, +0x7c, +0x80, +0x84, enemy +0x90, home +0x94, flee
//!   direction +0x98, last position +0x9c, the task index, +0x8c, +0x88.

use crate::grid::Y16X16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedAnimal {
    pub id: u16,
    pub kind: u16,
    pub pos: Y16X16,
    pub flags: u32,
    pub frame: u8,
    pub job: u16,
    pub events: u32,
    pub dir: u8,
    pub step: u8,
    /// The walking's state (+8), when saved.
    pub walk_state: Option<i32>,
    pub moved: u8,
    pub speed: u8,
    pub task_type: u8,
    pub frames: u8,
    pub dead_ticks: i32,
    pub state: i32,
    pub search: i32,
    pub roam: i32,
    pub flee_steps: i32,
    pub enemy: i32,
    pub home: i32,
    pub flee_dir: i32,
    pub last_pos: i32,
    pub task: u8,
    pub stuck: i32,
    pub wrong_land: i32,
}

struct Cursor<'a> {
    toks: Vec<&'a [u8]>,
    at: usize,
}

impl Cursor<'_> {
    fn int(&mut self) -> Option<i64> {
        let t = self.toks.get(self.at)?;
        self.at += 1;
        std::str::from_utf8(t).ok()?.parse().ok()
    }
    fn expect(&mut self, v: i64) -> Option<()> {
        (self.int()? == v).then_some(())
    }
}

/// The animals of a chunk-165 text, `None` when the layout does not match (a record with queued
/// events, whose layout is not decoded, ends the parse too).
pub fn parse(text: &[u8]) -> Option<Vec<SavedAnimal>> {
    let toks = text.split(|c| c.is_ascii_whitespace() || *c == 0).filter(|t| !t.is_empty()).collect();
    let mut c = Cursor { toks, at: 0 };
    c.expect(1)?;
    let n = c.int()?;
    let mut out = Vec::new();
    for _ in 0..n {
        c.expect(4)?;
        c.expect(2)?;
        let id = c.int()? as u16;
        let kind = c.int()? as u16;
        let pos = Y16X16(c.int()? as u32);
        let _health = c.int()?;
        c.expect(128)?;
        let _player = c.int()?;
        let flags = c.int()? as u32;
        let _uid = c.int()?;
        c.expect(1)?;
        let frame = c.int()? as u8;
        let _ = c.int()?;
        let job = c.int()? as u16;
        for _ in 0..4 {
            c.int()?;
        }
        let events = c.int()? as u32;
        if events != 0 {
            return None;
        }
        c.expect(1)?;
        let dir = c.int()? as u8;
        let step = c.int()? as u8;
        let _ = c.int()?;
        let _ = c.int()?;
        let walk_state = if c.int()? != 0 {
            c.int()?;
            c.expect(1)?;
            c.int()?;
            c.int()?;
            let s = c.int()? as i32;
            for _ in 0..14 {
                c.int()?;
            }
            c.int()?;
            Some(s)
        } else {
            None
        };
        c.expect(2)?;
        let moved = c.int()? as u8;
        let speed = c.int()? as u8;
        let task_type = c.int()? as u8;
        let _ = c.int()?;
        let frames = c.int()? as u8;
        let _ = c.int()?;
        let _ = c.int()?;
        let dead_ticks = c.int()? as i32;
        let state = c.int()? as i32;
        c.expect(3)?;
        let mut v = [0i32; 7];
        for x in &mut v {
            *x = c.int()? as i32;
        }
        let task = c.int()? as u8;
        let stuck = c.int()? as i32;
        let wrong_land = c.int()? as i32;
        out.push(SavedAnimal {
            id,
            kind,
            pos,
            flags,
            frame,
            job,
            events,
            dir,
            step,
            walk_state,
            moved,
            speed,
            task_type,
            frames,
            dead_ticks,
            state,
            search: v[0],
            roam: v[1],
            flee_steps: v[2],
            enemy: v[3],
            home: v[4],
            flee_dir: v[5],
            last_pos: v[6],
            task,
            stuck,
            wrong_land,
        });
    }
    Some(out)
}
