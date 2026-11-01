//! `CEntityTask` (MapObjects/EntityTask.cpp): one step of a settler's to-do list.

/// `CEntityTask::*` task types used by the ported roles (values as in `s4_formats::gamedata::TaskType`).
pub mod task_type {
    pub const GO_VIRTUAL: u8 = 0;
    pub const WORK_VIRTUAL: u8 = 1;
    pub const WAIT_VIRTUAL: u8 = 2;
    pub const PRODUCE_VIRTUAL: u8 = 3;
    pub const PUT_GOOD_VIRTUAL: u8 = 4;
    pub const GET_GOOD_VIRTUAL: u8 = 5;
    pub const GO: u8 = 6;
    pub const GO_TO_POS: u8 = 7;
    pub const GO_TO_TARGET: u8 = 10;
    pub const WORK: u8 = 16;
    pub const WAIT: u8 = 17;
    pub const CHECKIN: u8 = 24;
}

/// `CEntityTask`, field by field with the original byte offset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EntityTask {
    /// +4 `CEntityTask::*`.
    pub task: u8,
    /// +5 good / entity define (`entity` in jobInfo.xml).
    pub good: u8,
    /// +6 direction 0..5, −1 = keep.
    pub dir: i8,
    /// +7 duration in ticks (−1 for "computed by the role"; 0 in the XML becomes the frame count).
    pub duration: i8,
    /// +8 number of frames of the gfx job (`m_iCycleFrames`).
    pub frames: u8,
    /// +9 TRIGGER_* fired on the home building when the task starts.
    pub trigger: u8,
    /// +10 / +12 cell offset.
    pub x: i16,
    pub y: i16,
    /// +14 settler gfx job (`jobPart`).
    pub job_part: u16,
    /// +16 entity id, taken over by `InitCommonTaskValues` when non-zero.
    pub entity: u16,
    /// +18 (never set by the loaders).
    pub w18: u16,
    /// +20.
    pub visible: bool,
    /// +21 play frames forwards.
    pub forward: bool,
}

impl EntityTask {
    /// `CEntityTask::CEntityTask(task, jobPart, x, y, duration, frames, dir, forward, visible,
    /// good, w16, w18, trigger)` (release 0x531150), same argument order.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        task: u8,
        job_part: u16,
        x: i16,
        y: i16,
        duration: i8,
        frames: u8,
        dir: i8,
        forward: bool,
        visible: bool,
        good: u8,
        entity: u16,
        w18: u16,
        trigger: u8,
    ) -> EntityTask {
        EntityTask { task, good, dir, duration, frames, trigger, x, y, job_part, entity, w18, visible, forward }
    }

    pub(crate) fn hash(&self, h: &mut crate::hash::StateHasher) {
        h.u8(self.task);
        h.u8(self.good);
        h.u8(self.dir as u8);
        h.u8(self.duration as u8);
        h.u8(self.frames);
        h.u8(self.trigger);
        h.u16(self.x as u16);
        h.u16(self.y as u16);
        h.u16(self.job_part);
        h.u16(self.entity);
        h.u16(self.w18);
        h.bool(self.visible);
        h.bool(self.forward);
    }
}
