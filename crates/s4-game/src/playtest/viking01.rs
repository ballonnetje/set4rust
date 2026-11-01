//! User bug B3 on the viking01 mission (its real script, `missions::Mission`).

use s4_sim::entity::{ObjType};

/// User bug B3 (viking01: the Mayans' shipped troops never move): the script lands soldiers of
/// player 2 at (14..25, 158..172) with `Settlers.AddSettlers` and calls `AI.NewSquad(2,
/// AI.CMD_MOVE_HOME)`. The player AI passes the event on to its sector AIs (0x43a420), which
/// group the new units (`sector::new_squad`) and send them home: within 2000 ticks of the first
/// landing (tick ~14 006) they are in a group task force and away from the landing place.
#[test]
fn viking01_landed_troops_move() {
    let Some(m) = crate::missions::Mission::start("viking01") else { return };
    let landed = |m: &crate::missions::Mission| {
        m.ask(|w| {
            w.entities
                .iter()
                .filter(|(_, e)| e.obj_type == ObjType::Settler && e.owner == 2 && e.flags & 0xf != 0 && !e.is_dead() && (14..=25).contains(&e.x()) && (158..=172).contains(&e.y()))
                .map(|(i, _)| i)
                .collect::<Vec<u16>>()
        })
    };
    let mut troops = Vec::new();
    while m.tick() < 20_000 && troops.is_empty() {
        // The first landing comes at minute 15..19 (`Vars.Save2`); sent home, the troops leave
        // the landing place within a few hundred ticks.
        m.sim.step(if m.tick() < 12_000 { 1000 } else { 20 });
        troops = landed(&m);
    }
    assert!(!troops.is_empty(), "no landing by tick {}", m.tick());
    let at = m.tick();
    m.sim.step(2000);
    let ids = troops.clone();
    let (grouped, away) = m.ask(move |w| {
        let grouped = ids.iter().filter(|i| matches!(w.ai.infos.get(i), Some(s4_sim::ai::player::TfRef::Group(2, _, _)))).count();
        let away = ids.iter().filter(|&&i| w.entities.by_index(i as u32).is_some_and(|e| s4_sim::spiral::grid_distance(e.x() - 20, e.y() - 165) > 20)).count();
        (grouped, away)
    });
    eprintln!("landing of {} at tick {at}: after 2000 ticks {grouped} in a group, {away} more than 20 from the landing", troops.len());
    assert!(grouped > 0 && away > 0, "the landed troops stay: {grouped} grouped, {away} away of {}", troops.len());
}
