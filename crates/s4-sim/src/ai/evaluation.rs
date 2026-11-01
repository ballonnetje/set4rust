//! The player evaluation (`CAIAgentEvaluation`, 0x146b1a4, period 15 on `CAIMain`'s scheduler):
//! each run the next player (1..=player count) is evaluated by 0x4378b0 into its record of 0x58
//! bytes at 0x1266e28 + 0x58·p (`[evaluation +0x28] + 0x58·p`). Own reverse engineering of the
//! shipped exe; decompiles in `tools/re/decompiled/script/ai/s2/` (`agents.c`, `more1.c`). Not in
//! chunk 150: a loaded game evaluates afresh.
//!
//! The record by word (offset / 4):
//! * 0: the AI tick of the evaluation;
//! * 1..=5 (0x437930, goods on the player's piles, statistic +0xa04 + 4·good): swords, bows, the
//!   tribe's third weapon (Vikings battle-axes, Maya blowguns, Trojans backpack catapults, else
//!   0), their sum, gold bars;
//! * 6..=16 (0x437a00, settlers per type: `CSettlerMgr` 0x1545658 all of them, 0x1545fc4 those on
//!   its warrior lists, i.e. not inside a building): free swordsmen, free bowmen, swordsmen,
//!   bowmen, specialists, squad leaders, priests, and four sums (see [`units`]);
//! * 17..=21 (0x437d10, `CBuildingMgr` counts 0x14ef620 (sites and finished) of the player's small
//!   towers, large towers, castles, their sum and the sum weighted 1 / 2 / 4).

use crate::World;

/// Words per record.
pub const WORDS: usize = 22;

/// The record of player `p` (all 0 before its first evaluation).
pub fn of(world: &World, p: u8) -> [i32; WORDS] {
    world.ai.evals.get(p as usize).copied().unwrap_or([0; WORDS])
}

/// 0x4378b0 `(player)`.
pub fn evaluate(world: &mut World, p: u8) {
    let mut v = [0i32; WORDS];
    v[0] = world.ai.tick as i32;
    goods(world, p, &mut v);
    units(world, p, &mut v);
    buildings(world, p, &mut v);
    if world.ai.evals.len() < 9 {
        world.ai.evals.resize(9, [0; WORDS]);
    }
    if let Some(r) = world.ai.evals.get_mut(p as usize) {
        *r = v;
    }
}

/// 0x437930.
fn goods(world: &World, p: u8, v: &mut [i32; WORDS]) {
    let g = |k: usize| world.statistic.players.get(p as usize).and_then(|s| s.goods.get(k)).copied().unwrap_or(0);
    v[1] = g(34);
    v[2] = g(8);
    v[3] = match world.race_of(p) {
        1 => g(5),
        2 => g(6),
        4 => g(38),
        _ => 0,
    };
    v[4] = v[2] + v[1] + v[3];
    v[5] = g(14);
}

/// .rdata 0x1163238: (settler type, class, value) over `CSettlerMgr` 0x1545658.
const ALL: [(u16, usize, i32); 20] = [
    (29, 3, 2), (30, 3, 3), (31, 3, 4), (32, 4, 2), (33, 4, 3), (34, 4, 4), (35, 5, 3), (36, 5, 4), (37, 5, 5), (38, 5, 3),
    (39, 5, 4), (40, 5, 5), (41, 5, 3), (42, 5, 4), (43, 5, 5), (44, 6, 10), (45, 7, 0), (61, 5, 3), (62, 5, 4), (63, 5, 5),
];
/// .rdata 0x11631a8: the same over the warrior lists' lengths 0x1545fc4.
const FREE: [(u16, usize, i32); 6] = [(29, 1, 2), (30, 1, 3), (31, 1, 4), (32, 2, 2), (33, 2, 3), (34, 2, 4)];

/// 0x437a00: per class 1..7 a count and a value (count · value per type): 1 / 2 free swordsmen /
/// bowmen, 3 / 4 swordsmen / bowmen, 5 specialists, 6 squad leaders, 7 priests. Words 6..=12 the
/// counts of 1..=7; 13 the fighters (3 + 4 + 5) plus leaders and priests; 14 the fighters' value
/// plus the leaders'; 15 the free swordsmen, free bowmen and specialists plus leaders and
/// priests; 16 their value plus the leaders'.
fn units(world: &World, p: u8, v: &mut [i32; WORDS]) {
    let mut count = [0i32; 8];
    let mut value = [0i32; 8];
    for (kind, class, val) in ALL {
        let n = world.settler_lists.settlers(p, kind);
        count[class] += n;
        value[class] += val * n;
    }
    for (kind, class, val) in FREE {
        let n = world.settler_lists.count(p, kind);
        count[class] += n;
        value[class] += val * n;
    }
    let fighters = count[3] + count[4] + count[5];
    let fighter_value = value[3] + value[4] + value[5];
    v[6] = count[1];
    v[7] = count[2];
    v[8] = count[3];
    v[9] = count[4];
    v[10] = count[5];
    v[11] = count[6];
    v[12] = count[7];
    v[13] = fighters + v[12] + v[11];
    v[14] = fighter_value + value[6];
    v[15] = v[12] + v[11] + count[2] + count[1] + count[5];
    v[16] = value[6] + value[1] + value[2] + value[5];
}

/// 0x437d10.
fn buildings(world: &World, p: u8, v: &mut [i32; WORDS]) {
    let n = |t: i32| world.buildings_amount(p as i32, t, crate::query::ALL);
    v[17] = n(46);
    v[18] = n(47);
    v[19] = n(48);
    v[20] = v[17] + v[18] + v[19];
    v[21] = v[17] + 2 * v[18] + 4 * v[19];
}
