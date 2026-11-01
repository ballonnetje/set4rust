//! Settler jobs: the to-do lists of `CEntityToDoListMgr` (jobInfo.xml) and their execution by
//! `CSettler` and its `ISettlerRole` (MapObjects/Settler/*.cpp, decompiled debug build; release
//! addresses where they were needed to read vtables).
//!
//! How the original runs a job:
//! * A settler holds a to-do list (a shared jobInfo.xml list, or one its role built) and an
//!   iterator into it. `CSettler::NewToDoList` / `TakeAnimList` / `TakeWaitList` install a list
//!   and call the role's `TakeJob` for its first task.
//! * `TakeJob` copies the task into the role (`InitCommonTaskValues`: task type, duration
//!   counter, frame count, gfx job, direction, visibility, building trigger) and does the
//!   task-specific start, normally `RegisterForLogicUpdate(n)`.
//! * When the schedule reaches the settler, `CSettler::LogicUpdate` → `ISettlerRole::LogicUpdate`
//!   handles WAIT itself and hands every other task to the role's `LogicUpdateJob`. That
//!   advances the animation (`ISettlerRole::Update`: frames by elapsed ticks, the job sound on
//!   its frame, war action for warriors), counts the task's counter down and either
//!   re-registers or moves to the next task (`GetNextJob` → `TakeJob`).
//! * Walking tasks call into pathing: `WalkToXY`, then one `Go` per step (walk speed 9 ticks,
//!   7/8 on roads for carriers and donkeys); arrival takes the next job.
//!
//! There is no central task interpreter: every role has its own `TakeJob` / `LogicUpdateJob`
//! switch over the task types it supports. Ported roles: [`house_worker`] (`CHouseWorkerRole`),
//! [`carrier`] (`CCarrierRole`), [`digger`] (`CDiggerRole`), [`builder`] (`CBuilderRole`).
//!
//! Boundaries: pathing is called only through [`walking::Pathing`] (stubbed, see
//! [`walking::ActivePathing`]); buildings, piles, vehicles and eco-sectors through [`hooks`]
//! (not ported yet, they record [`crate::event::SimEvent::Unported`]). Sounds and war action
//! leave the sim as [`crate::event::SimEvent`]s.
//!
//! No floats: the house worker's float pixel offsets are fixed point ([`state::OFFSET_ONE`]).

pub mod builder;
pub mod carrier;
pub mod core;
pub mod dark_gardener;
pub mod data;
pub mod digger;
pub mod donkey;
pub mod door;
pub mod flee;
pub mod free_worker;
pub mod gardener;
pub mod geologist;
pub mod hooks;
pub mod house_worker;
pub mod priest;
pub mod hunter;
pub mod mushroom_farmer;
pub mod pioneer;
pub mod roles;
pub mod save_settler;
pub mod shaman;
pub mod slave;
pub mod soldier;
pub mod state;
pub mod task;
pub mod thief;
pub mod walking;
pub mod warrior;

pub use core::{actual_task, logic_update, new_to_do_list, set_to_do_list, take_anim_list, take_wait_list};
pub use data::{JobData, JobSoundInfo};
pub use roles::new_role;
pub use state::{JobState, ToDo, role};
pub use task::EntityTask;
