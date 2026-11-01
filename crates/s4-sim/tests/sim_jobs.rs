//! Job lists (`CEntityToDoListMgr`) and the house worker's job execution (`CHouseWorkerRole`).
//! Skipped when the install is absent.

use s4_sim::event::SimEvent;
use s4_sim::jobs::{self, ToDo, role, task::task_type};
use s4_sim::{EntityData, Grid, Player, Rules, World, tick};
use std::sync::OnceLock;

const SAWMILLWORKER: u16 = 12;
const JOB_SAWMILLWORKER_WORK: u16 = 197;
const JOB_SAWMILLWORKER_CHECKIN: u16 = 78;

fn rules() -> Option<&'static Rules> {
    static R: OnceLock<Option<Rules>> = OnceLock::new();
    R.get_or_init(|| {
        if !s4_formats::game_dir().join("GameData").exists() {
            eprintln!("skipping: install not found");
            return None;
        }
        Some(Rules::load_default().expect("rules"))
    })
    .as_ref()
}

fn world() -> Option<World> {
    let mut w = World::new(Grid::new(64).unwrap(), rules()?.clone());
    w.players[1] = Some(Player { id: 1, race: 0, start_x: 0, start_y: 0 });
    // Land owned by player 1, with a real tiling (state_hash walks the tiles).
    w.layers.ground.fill(16);
    let owner = vec![1u8; w.grid.len()];
    let l = &mut w.layers;
    w.tiling = s4_sim::tiling::init(s4_sim::tiling::InitLayers {
        grid: w.grid,
        ground: &l.ground,
        flags: &mut l.flags,
        gfx_bits: &mut l.gfx_bits,
        owner: &owner,
    });
    Some(w)
}

fn job(w: &World, i: u16) -> &jobs::JobState {
    match &w.entities.by_index(i as u32).unwrap().data {
        EntityData::Settler(s) => &s.job,
        _ => panic!("not a settler"),
    }
}

fn job_mut(w: &mut World, i: u16) -> &mut jobs::JobState {
    match &mut w.entities.by_index_mut(i as u32).unwrap().data {
        EntityData::Settler(s) => &mut s.job,
        _ => panic!("not a settler"),
    }
}

#[test]
fn job_lists_follow_the_loader() {
    let Some(r) = rules() else { return };
    let j = &r.jobs;
    // jobInfo.xml JOB_SAWMILLWORKER_WORK, first node: SW_WALK GO_VIRTUAL (3,6) duration -1 dir 1.
    let work = j.list(0, JOB_SAWMILLWORKER_WORK).expect("sawmill work list");
    let t0 = work[0];
    assert_eq!((t0.task, t0.x, t0.y, t0.duration, t0.dir, t0.forward, t0.visible), (task_type::GO_VIRTUAL, 3, 6, -1, 1, true, true));
    assert_eq!(t0.frames, j.frame_count(0, t0.job_part, 1), "frames = gfx job frames of the node's direction");
    assert!(t0.frames > 1);
    // The 4th node (GET_GOOD_VIRTUAL, duration 0 in the XML) takes the frame count as duration.
    let t3 = work[3];
    assert_eq!(t3.task, task_type::GET_GOOD_VIRTUAL);
    assert_eq!(t3.duration as u8, t3.frames);
    assert_ne!(t3.good, 0, "GOOD_LOG");

    // Implicit wait list `type` (one WAIT, duration 1, dir -1, frames of direction 2).
    let first = j.first_job(0, SAWMILLWORKER);
    assert_ne!(first, 0);
    let wait = j.list(0, SAWMILLWORKER).expect("wait list");
    assert_eq!(wait.len(), 1);
    assert_eq!((wait[0].task, wait[0].job_part, wait[0].duration, wait[0].dir), (task_type::WAIT, first, 1, -1));
    assert_eq!(wait[0].frames, j.frame_count(0, first, 2));
    // Races 1..4 share race 0's list object (CEntityToDoListMgr ctor: `lists[i][k] = lists[0][k]`).
    for race in 1..5 {
        assert!(j.same_list_object((0, SAWMILLWORKER), (race, SAWMILLWORKER)), "race {race}");
    }
    // Check-in list 66 + type: GO_TO_TARGET + CHECKIN, then any XML nodes appended.
    let checkin = j.list(0, JOB_SAWMILLWORKER_CHECKIN).expect("checkin list");
    assert_eq!((checkin[0].task, checkin[1].task), (task_type::GO_TO_TARGET, task_type::CHECKIN));
    assert_eq!((checkin[1].forward, checkin[1].dir), (false, 1));
    eprintln!("checkin list: {} tasks; work list: {} tasks", checkin.len(), work.len());
    // SettlerValues.xml: anim list 1 of the sawmill worker is its work list.
    assert_eq!(j.anim_list(0, SAWMILLWORKER, 1), Some(JOB_SAWMILLWORKER_WORK));
}

/// One sawmill worker in its building (home id 7) runs `JOB_SAWMILLWORKER_WORK` to its last task.
/// Expected in-building move ticks, from `CHouseWorkerRole::TakeJob` by hand:
/// (0,0)→(3,6): offsets (0,72) px: max(|72|/12+½, |72|/24+½) = max(6,3) → 9·6 = 54 ticks;
/// (3,6)→(4,6): (24,0) → max(0, 1) → 9; (4,6)→(4,3): (36,−36) → max(3, 3) → 27.
#[test]
fn house_worker_runs_its_work_list() {
    let Some(mut w) = world() else { return };
    let s = s4_sim::create::add_settler(&mut w, 20, 20, 1, SAWMILLWORKER).unwrap();
    jobs::new_role(&mut w, s, role::HOUSE_WORKER, 7);
    jobs::take_anim_list(&mut w, s, 1);
    assert_eq!(job(&w, s).list_id, JOB_SAWMILLWORKER_WORK as i16);
    assert_eq!(job(&w, s).todo, ToDo::Shared { race: 0, list: JOB_SAWMILLWORKER_WORK });
    let list = w.rules.jobs.list(0, JOB_SAWMILLWORKER_WORK).unwrap().clone();

    let mut changes = vec![(0u32, 0u16, job(&w, s).task)];
    for _ in 0..3000 {
        tick(&mut w, &[]);
        let j = job(&w, s);
        let last = *changes.last().unwrap();
        if (j.cursor, j.task) != (last.1, last.2) {
            changes.push((w.tick, j.cursor, j.task));
        }
        if j.task == task_type::CHECKIN {
            break;
        }
    }
    for c in &changes {
        eprintln!("tick {:4} cursor {:2} task {:2}", c.0, c.1, c.2);
    }
    // Every task of the list ran, in order, with its own type.
    for (k, c) in changes.iter().take(list.len()).enumerate() {
        assert_eq!(c.1 as usize, k);
        assert_eq!(c.2, list[k].task, "task {k}");
    }
    // The list ends with CHECKIN, which does not take a next job: the worker stays checked in
    // until the building hands out the next list.
    let last = changes.last().unwrap();
    assert_eq!((last.1 as usize, last.2), (list.len() - 1, task_type::CHECKIN));
    // The first registration happens before tick 1 (cursor 0), so it is seen one tick late.
    assert_eq!(changes[1].0 - changes[0].0, 54 + 1);
    assert_eq!(changes[2].0 - changes[1].0, 9);
    assert_eq!(changes[3].0 - changes[2].0, 27);
    // Ended unregistered (GetNextJob sets WAIT without re-registering; the building gives the next list).
    let e = w.entities.by_index(s as u32).unwrap();
    assert_eq!(e.flags & s4_sim::entity::flag::REGISTERED, 0);
    // The good handling went to the (unported) pile code.
    let unported: Vec<&str> = w
        .events
        .iter()
        .filter_map(|e| match e {
            SimEvent::Unported { what, .. } => Some(*what),
            _ => None,
        })
        .collect();
    assert!(unported.contains(&"CPile::DecreaseUnforeseen"), "{unported:?}");
    assert!(unported.contains(&"CPile::IncreaseUnforeseen"), "{unported:?}");
}

/// The check-in list walks to the destination with the stub pathing (one node per 9 ticks for a
/// non-carrier), then CHECKIN hides the settler and enters the building.
#[test]
fn checkin_walks_then_enters() {
    let Some(mut w) = world() else { return };
    let s = s4_sim::create::add_settler(&mut w, 20, 20, 1, SAWMILLWORKER).unwrap();
    jobs::new_role(&mut w, s, role::HOUSE_WORKER, 7);
    job_mut(&mut w, s).destination = s4_sim::Y16X16::pack(23, 20);
    jobs::take_anim_list(&mut w, s, 0);
    assert_eq!(job(&w, s).list_id, JOB_SAWMILLWORKER_CHECKIN as i16);
    assert_eq!(job(&w, s).task, task_type::GO);
    let mut arrived = None;
    for _ in 0..200 {
        tick(&mut w, &[]);
        if job(&w, s).task == task_type::CHECKIN {
            arrived = Some(w.tick);
            break;
        }
    }
    let e = w.entities.by_index(s as u32).unwrap();
    assert_eq!((e.x(), e.y()), (23, 20));
    assert_eq!(w.layers.map_object[w.grid.index(23, 20)], s);
    assert_eq!(w.layers.map_object[w.grid.index(20, 20)], 0);
    assert_eq!(e.flags & s4_sim::entity::flag::VISIBLE, 0, "CHECKIN clears VISIBLE");
    // TakeJob's Go steps at once; then a step every 9 ticks; the Go after the 3rd step arrives.
    // (+1: the first registration is made before tick 1.)
    eprintln!("arrived at tick {arrived:?}");
    assert_eq!(arrived, Some(3 * 9 + 1));
    assert!(w.events.iter().any(|e| matches!(e, SimEvent::Unported { what: "CBuilding::SettlerEnter", entity: 7 })));
}

#[test]
fn job_execution_is_deterministic() {
    let Some(base) = world() else { return };
    let run = || {
        let mut w = base.clone();
        let s = s4_sim::create::add_settler(&mut w, 20, 20, 1, SAWMILLWORKER).unwrap();
        jobs::new_role(&mut w, s, role::HOUSE_WORKER, 7);
        jobs::take_anim_list(&mut w, s, 1);
        let mut hashes = Vec::new();
        for t in 0..400 {
            tick(&mut w, &[]);
            if t % 50 == 0 {
                hashes.push(w.state_hash());
            }
        }
        hashes
    };
    let (a, b) = (run(), run());
    assert_eq!(a, b);
    assert!(a.windows(2).any(|p| p[0] != p[1]), "job state changes the hash");
}

const CARRIER: u16 = 1;

/// A carrier on its wait list counts idle WAIT updates (standing still) up to its threshold
/// (`Rand() % 300 + 300`), then plays one idle list (154..156: WORK, then WAIT). It stays on that
/// list's WAIT, keeps counting, and the next threshold starts another idle list.
#[test]
fn carrier_idles() {
    let Some(mut w) = world() else { return };
    let s = s4_sim::create::add_settler(&mut w, 20, 20, 1, CARRIER).unwrap();
    let threshold = job(&w, s).carrier.idle_after;
    assert!((300..600).contains(&threshold), "{threshold}");
    assert_eq!(job(&w, s).list_id, CARRIER as i16);
    let mut starts = Vec::new();
    let mut last = (job(&w, s).list_id, job(&w, s).cursor);
    let mut waits_on_idle_list = false;
    for _ in 0..20_000 {
        tick(&mut w, &[]);
        let j = job(&w, s);
        if (j.list_id, j.cursor) != last {
            if (154..=156).contains(&(j.list_id as u16)) && j.cursor == 0 {
                starts.push((w.tick, j.list_id));
                assert!((300..600).contains(&j.carrier.idle_after));
            }
            if (154..=156).contains(&(j.list_id as u16)) && j.cursor == 1 && j.task == task_type::WAIT {
                waits_on_idle_list = true;
            }
            last = (j.list_id, j.cursor);
        }
        if starts.len() >= 3 {
            break;
        }
    }
    eprintln!("idle lists started at {starts:?} (first threshold {threshold})");
    assert!(starts.len() >= 3, "idle animations repeat");
    // WAIT re-registers after Rand() % 4 + 1 ticks: `threshold` idle updates take 1..4 ticks each.
    let first = starts[0].0;
    assert!(first >= threshold as u32 && first <= 4 * threshold as u32 + 4);
    assert!(waits_on_idle_list, "after the idle WORK the carrier waits on the idle list");
}

/// Event 10 (transport) runs JOB_CARRIER_TRANSPORT_GOOD: walk to the source pile, take its good,
/// walk to the destination pile showing the carry animation (gfx job + good), put it down, then
/// SetFree (wait list) and walk back to the start.
#[test]
fn carrier_transports_one_good() {
    let Some(mut w) = world() else { return };
    let src = s4_sim::create::add_pile(&mut w, 24, 20, 9, 5, 2).unwrap();
    let dst = s4_sim::create::add_pile(&mut w, 24, 26, 9, 0, 2).unwrap();
    let s = s4_sim::create::add_settler(&mut w, 20, 20, 1, CARRIER).unwrap();
    {
        let j = job_mut(&mut w, s);
        j.source_pile = src;
        j.carrier.dest_pile = dst;
    }
    // CEcoSector::TransportGood: the event, then the reservations on both piles.
    jobs::core::set_event(&mut w, s, jobs::state::EntityEvent { event: 10, kind: 0, data: [0, src as i32, dst as i32] });
    s4_sim::pile::attach_and_inc_amount_leaving(&mut w, src, s, 1, s4_sim::pile::target::PILE_SRC);
    s4_sim::pile::attach_and_inc_amount_coming(&mut w, dst, s);
    let list = w.rules.jobs.list(0, jobs::carrier::LIST_TRANSPORT_GOOD).unwrap().clone();
    eprintln!("transport list: {:?}", list.iter().map(|t| t.task).collect::<Vec<_>>());
    let mut seen = Vec::new();
    let mut carry_ok = None;
    for _ in 0..600 {
        tick(&mut w, &[]);
        let j = job(&w, s);
        let key = (j.list_id, j.cursor, j.task);
        if seen.last() != Some(&key) {
            seen.push(key);
        }
        if j.list_id == jobs::carrier::LIST_TRANSPORT_GOOD as i16 && list[j.cursor as usize].task == 19 && j.task == task_type::GO {
            carry_ok = Some(j.job_part == list[j.cursor as usize].job_part + j.carrier.good as u16 && j.carrier.good == 9);
        }
        if j.list_id == CARRIER as i16 && seen.iter().any(|k| k.0 == jobs::carrier::LIST_TRANSPORT_GOOD as i16) && j.task == task_type::WAIT {
            break;
        }
    }
    for k in &seen {
        eprintln!("{k:?}");
    }
    assert_eq!(carry_ok, Some(true), "carrying shows gfx job + good");
    assert_eq!(job(&w, s).carrier.good, 0, "good put down");
    // get + put: CPile::ChangeAmountAndDetach on both piles.
    let amounts = (s4_sim::pile::get(&w, src).map(|p| (p.amount, p.leaving)), s4_sim::pile::get(&w, dst).map(|p| (p.amount, p.coming)));
    assert_eq!(amounts, (Some((4, 0)), Some((1, 0))), "one good moved");
    let e = w.entities.by_index(s as u32).unwrap();
    eprintln!("carrier ends at ({}, {})", e.x(), e.y());
}
