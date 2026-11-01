//! Magic on roman02, driven by `tick()` only (`magic`, `buildings::temple`, `jobs::priest`;
//! release 0x4f5570 CastSpell, 0x54f870.. small temple, 0x5356a0.. big temple, 0x57f590..
//! priest). Skipped without the game data.

use s4_sim::World;
use s4_sim::command::{Command, PlayerCommand};
use s4_sim::entity::EntityData;
use s4_sim::event::SimEvent;

const SMALLTEMPLE: u8 = 43;
const BIGTEMPLE: u8 = 44;
const PRIEST: u16 = 45;
const SERVANT: u16 = 56;
const WINE: u8 = 37;

fn roman02() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    if !p.exists() {
        return None;
    }
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    let (mut w, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok()?;
    let towers: Vec<(i32, i32)> = w
        .entities
        .iter()
        .filter_map(|(id, e)| match &e.data {
            EntityData::Building(_) if s4_sim::buildings::have_inhabitant(&w, id) => Some((e.x(), e.y())),
            _ => None,
        })
        .collect();
    for (x, y) in towers {
        s4_sim::territory::set_owner(&mut w, x, y);
    }
    Some(w)
}

/// No map object (a settler) on the building's cells: placing it on one would cut the settler
/// off its eco-sector (a real building is placed by its site).
fn free_footprint(w: &World, x: i32, y: i32, building: u8) -> bool {
    w.rules.building(0, building).building_cells.iter().all(|&(dx, dy)| {
        w.grid.checked_index(x + dx as i32, y + dy as i32).is_some_and(|n| w.layers.map_object[n] == 0)
    })
}

/// A finished building of player 1 on the free site nearest its castle.
fn place(w: &mut World, building: u8) -> u16 {
    let castle = w.entities.iter().find(|(id, e)| e.owner == 1 && matches!(&e.data, EntityData::Building(_)) && s4_sim::buildings::have_inhabitant(w, *id)).map(|(_, e)| (e.x(), e.y())).unwrap();
    let size = w.grid.size() as i32;
    let &(x, y, _) = s4_sim::build::possible_sites(w, 1, building, (0, 0), (size, size)).iter().filter(|s| free_footprint(w, s.0 as i32, s.1 as i32, building)).min_by_key(|s| (s.0 as i32 - castle.0).abs() + (s.1 as i32 - castle.1).abs()).expect("a site");
    s4_sim::create::add_building(w, x as i32, y as i32, 1, building).expect("building")
}

fn mana(w: &World) -> (i32, i32) {
    let p = w.magic.pool(1).unwrap();
    (p.mana, p.gained)
}

#[test]
fn costs_from_the_install() {
    let Some(w) = roman02() else { return };
    // Magic.cfg: SpellCostsRoman = 12, 17, 10, 10, 17, 25, 40, 20.
    assert_eq!(w.magic.pool(1).unwrap().costs, [12, 17, 10, 10, 17, 25, 40, 20]);
    assert_eq!(mana(&w), (0, 0));
    let c = &w.rules.magic;
    assert_eq!((c.cost_increase(0), c.cost_increase(2), c.max_cost(0, 6)), (25, 12, 160));
    assert_eq!(s4_sim::magic::raise_cost(40, 25, 160), 43);
    assert_eq!(s4_sim::magic::raise_cost(5, 25, 20), 6);
}

/// (tick of the first donation, mana and total after 3000 ticks, servant type, state hash).
fn small_temple_run() -> Option<(u32, (i32, i32), u16, u64)> {
    let mut w = roman02()?;
    let t = place(&mut w, SMALLTEMPLE);
    let pile = s4_sim::buildings::temple::small_state(&w, t)?.pile;
    assert_eq!(s4_sim::pile::get(&w, pile).map(|p| p.good), Some(WINE));
    // Three bottles as if carriers had brought them.
    if let Some(EntityData::Pile(p)) = w.entities.by_index_mut(pile as u32).map(|e| &mut e.data) {
        p.amount = 3;
    }
    let mut first = 0;
    for _ in 0..3000 {
        s4_sim::tick(&mut w, &[]);
        if first == 0 && mana(&w).0 > 0 {
            first = w.tick;
        }
    }
    let servant = s4_sim::buildings::role(&w, t).map_or(0, |r| r.base.inhabitant);
    let kind = w.entities.by_index(servant as u32).map_or(0, |e| e.kind);
    Some((first, mana(&w), kind, w.state_hash()))
}

#[test]
fn small_temple_turns_wine_into_mana() {
    let Some(a) = small_temple_run() else { return };
    eprintln!("{a:?}");
    assert_eq!(a.2, SERVANT);
    assert!(a.0 > 0, "no donation");
    // ManaPerDonation 1: one per bottle.
    assert_eq!(a.1, (3, 3));
    assert_eq!(Some(a), small_temple_run(), "not deterministic");
}

#[test]
fn priest_casts_with_deferral() {
    let Some(mut w) = roman02() else { return };
    let t = place(&mut w, BIGTEMPLE);
    // ManaPerBigTemple 12 once.
    assert_eq!(mana(&w), (12, 12));
    let door = match &w.entities.by_index(t as u32).unwrap().data {
        EntityData::Building(d) => d.door,
        _ => unreachable!(),
    };
    let priest = s4_sim::create::add_settler(&mut w, door.x(), door.y() + 1, 1, PRIEST).expect("priest");
    let job = |w: &World| match &w.entities.by_index(priest as u32).unwrap().data {
        EntityData::Settler(s) => s.job.clone(),
        _ => unreachable!(),
    };
    assert_eq!(job(&w).role, s4_sim::jobs::role::PRIEST);
    for _ in 0..20 {
        s4_sim::tick(&mut w, &[]);
    }
    // Spell 0 (12 mana) 25 nodes away: the priest walks until within DefaultSpellRange (15).
    let p = w.entities.by_index(priest as u32).unwrap().pos;
    let (tx, ty) = (p.x() + 25, p.y());
    let cmd = PlayerCommand { player: 1, command: Command::CastSpell { priest, spell: 0, x: tx as u16, y: ty as u16 } };
    s4_sim::tick(&mut w, &[cmd]);
    let mut cast = 0;
    let mut spell_run = 0;
    let mut effect = None;
    let piles = |w: &World| w.entities.iter().filter(|(_, e)| matches!(e.data, EntityData::Pile(_)) && s4_sim::spiral::grid_distance(e.x() - tx, e.y() - ty) <= 8).count();
    let piles_before = piles(&w);
    for _ in 0..600 {
        w.events.clear();
        s4_sim::tick(&mut w, &[]);
        for e in &w.events {
            match *e {
                SimEvent::SpellCast { player: 1, spell: 0, x, y } if (x, y) == (tx, ty) => cast = w.tick,
                // The spell's own effect (SpellEffectsRoman[0] = 24 with sound 65) comes after it ran.
                SimEvent::Effect { effect: 24, sound: 65, x, y } if (x, y) == (tx, ty) => {
                    effect = Some((w.tick, 65));
                    spell_run = w.tick;
                }
                _ => {}
            }
        }
        if spell_run != 0 {
            break;
        }
    }
    assert!(cast > 0, "not cast");
    let d = {
        let p = w.entities.by_index(priest as u32).unwrap().pos;
        s4_sim::spiral::grid_distance(tx - p.x(), ty - p.y())
    };
    assert!(d <= 15, "cast from {d} away");
    assert_eq!(spell_run, cast + 12, "deferred by 12 ticks");
    assert_eq!(effect, Some((cast + 12, 65)));
    // The gift (0x4f35d0): 3..5 piles (SpellLimitsRoman[0]) around the target.
    let gifts = piles(&w) - piles_before;
    assert!((3..=5).contains(&gifts), "{gifts} gifts");
    let pool = w.magic.pool(1).unwrap();
    assert_eq!((pool.mana, pool.gained, pool.costs[0], pool.casts[0]), (0, 12, 13, 1));
    assert_eq!(job(&w).priest.spell, -1);
    // Without mana the next order walks him there but the cast fails: back to waiting.
    let cmd = PlayerCommand { player: 1, command: Command::CastSpell { priest, spell: 0, x: tx as u16, y: ty as u16 } };
    s4_sim::tick(&mut w, &[cmd]);
    for _ in 0..100 {
        w.events.clear();
        s4_sim::tick(&mut w, &[]);
        assert!(!w.events.iter().any(|e| matches!(e, SimEvent::SpellCast { .. })));
    }
    assert_eq!(w.magic.pool(1).unwrap().casts[0], 1);
}

#[test]
fn big_temple_makes_priests() {
    let Some(mut w) = roman02() else { return };
    let t = place(&mut w, BIGTEMPLE);
    let priests = |w: &World| w.entities.iter().filter(|(_, e)| e.owner == 1 && e.kind == PRIEST && matches!(e.data, EntityData::Settler(_))).count();
    assert_eq!(priests(&w), 0);
    let mut ticks = Vec::new();
    for _ in 0..7200 {
        let before = priests(&w);
        s4_sim::tick(&mut w, &[]);
        if priests(&w) > before {
            ticks.push(w.tick);
        }
    }
    eprintln!("priests at {ticks:?}");
    // Roman productionDelay 250: one at the first update (registered for 31 at tick 0, it runs
    // in tick 32), then every (250 + 2)·14 + 1.
    assert_eq!(ticks, [32, 32 + 3529, 32 + 2 * 3529]);
    assert_eq!(s4_sim::buildings::temple::big_state(&w, t).unwrap().priests, 3);
}
