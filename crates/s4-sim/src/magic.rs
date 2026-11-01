//! Magic: the players' mana, `Config\Magic.cfg` and `CastSpell` (own reverse engineering of the
//! shipped exe; SIM.md §20).
//!
//! * **Mana pool** (per player, the first 0x70 bytes of the player's `CPlayerGameData`, .bss
//!   0x149b7f8 + 0x8c·player; chunk 140 of the savegames): +0 bit 0 "the cached cast counts are
//!   stale", +4 mana `^ 0x27356334`, +8 mana gained in total `^ 0x17a65b3c`, +0xc a value never
//!   read (`^ 0x83476350`, 0 in every save), +0x10 + 4·i the current cost of spell `i`
//!   `^ (−1 − i)·0x6cb89cbb`, +0x30 + 4·i the casts of spell `i` `^ (0xbdbdbe07 − i·0x38567497)`,
//!   +0x50 + 4·i the GUI's cache of affordable casts (−1 = not computed). The XOR keys only
//!   obscure the values in memory; [`ManaPool`] keeps them plain (`crate::save` decodes them).
//! * **Reset** 0x4f6e00 and **init** 0x4f5400 (game init 0x4fdb42, every player after the
//!   statistic): costs from `SpellCosts<race>` clamped to 1..=1000.
//! * **Income**: a small temple's donation 0x4f58f0 (`ManaPerDonation`, 1..=99), a big temple
//!   built 0x4f5960 (`ManaPerBigTemple`, 0..=999, once in its `Init`); both also add to the
//!   total. A Viking war machine's shot takes `VikingWarmachineShotCost` (0x4f59f0, at least 1,
//!   at most 999; mana does not go below 0). The Dark Tribe's temple spends mana on its units
//!   (0x545760, stage C).
//! * **`CastSpell(player, spell, x, y, flags, caster)`** 0x4f5570, see [`cast_spell`].

pub mod spells;

use crate::cfg::Cfg;
use crate::event::SimEvent;
use crate::hash::StateHasher;
use crate::world::World;

/// Spells per race.
pub const SPELLS: usize = 8;
/// Races with a spell table (Roman, Viking, Maya, Dark Tribe, Trojan).
pub const RACES: usize = 5;
/// The race names in `Magic.cfg` keys, in race order.
pub const RACE_NAMES: [&str; RACES] = ["Roman", "Viking", "Maya", "Dark", "Trojan"];

/// The XOR keys of the mana block (0x4f6e00 / 0x4f5400).
pub mod keys {
    /// +4 mana.
    pub const MANA: u32 = 0x2735_6334;
    /// +8 mana gained.
    pub const GAINED: u32 = 0x17a6_5b3c;
    /// +0xc.
    pub const UNKNOWN_C: u32 = 0x8347_6350;

    /// +0x10 + 4·i, the cost of spell `i`: `(−1 − i)·0x6cb89cbb`.
    pub const fn cost(i: usize) -> u32 {
        (-1 - i as i32).wrapping_mul(0x6cb8_9cbb) as u32
    }

    /// +0x30 + 4·i, the casts of spell `i`: `0xbdbdbe07 − i·0x38567497`.
    pub const fn casts(i: usize) -> u32 {
        0xbdbd_be07u32.wrapping_sub((i as u32).wrapping_mul(0x3856_7497))
    }
}

/// The `CastSpell` flags.
pub mod cast_flag {
    /// No mana is needed or taken (and the cost does not rise).
    pub const FREE: u32 = 1;
    /// Half the cost; the cost rises by half the increase.
    pub const HALF: u32 = 2;
    /// No range check from the caster.
    pub const NO_RANGE: u32 = 4;
    /// The race is bits 24..=26 (not the player's): no caster, [`FREE`] and [`NO_RANGE`]
    /// (the script functions 0x5c4040 / 0x5c8ff0 cast with this).
    pub const RACE: u32 = 0x800_0000;
    /// Pay now, the spell happens 12 ticks later (future event 3; priests, the shaman).
    pub const DEFER: u32 = 0x1000_0000;
}

/// `CFutureEvents` type of a deferred spell (handler 0x4e5300 → 0x4f5c50).
pub const FUTURE_EVENT: u8 = 3;
/// Its delay in ticks.
pub const DEFER_TICKS: u32 = 12;

/// The `[MAGIC]` values of `Config\Magic.cfg`, registered by the static initialisers
/// 0x41df00.. (`TStaticConfigIntArray<8>` per race, objects 0x11ab4d0..0x11abca0, values at
/// object + 8) and 0x420160.. (scalars, 0x11ab44c and 0x11abca0..0x11abe04). Per-race tables are
/// indexed by race (the pointer tables 0x1000784 costs, 0x1000900 radii, 0x10008ec / 0xffff44
/// limits, 0xfffb4c durations, 0x100091c / 0x10008d8 effects, 0x100062c / 0x100028c / 0xffef0c
/// presents are in race order). [`MagicCfg::default`] holds the exe's defaults.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MagicCfg {
    pub costs: [[i32; SPELLS]; RACES],
    pub radii: [[i32; SPELLS]; RACES],
    pub limits_min: [[i32; SPELLS]; RACES],
    pub limits_max: [[i32; SPELLS]; RACES],
    /// In seconds (14 ticks).
    pub durations: [[i32; SPELLS]; RACES],
    pub effects: [[i32; SPELLS]; RACES],
    pub sound_effects: [[i32; SPELLS]; RACES],
    pub present_goods: [[i32; SPELLS]; RACES],
    pub present_chances: [[i32; SPELLS]; RACES],
    pub present_amounts: [[i32; SPELLS]; RACES],
    pub convert_good_src: [i32; RACES],
    pub convert_good_dst: [i32; RACES],
    pub terrain_src: [i32; RACES],
    pub terrain_dst: [i32; RACES],
    pub lightning_bolt_damage_min: i32,
    pub lightning_bolt_damage_max: i32,
    pub shield_dmg_decrease256: i32,
    pub bloodlust_dmg_increase256: i32,
    pub faster_crops_decrease256: i32,
    pub viking_warmachine_shot_cost: i32,
    /// Percent per cast (0x11abdd4).
    pub spell_cost_increase: i32,
    /// Percent above the base cost (0x11abde0).
    pub spell_cost_max_increase: i32,
    pub mana_per_donation: i32,
    pub mana_per_big_temple: i32,
    pub default_spell_range: i32,
    /// The Dark Tribe's `[AI]` values of `Config\Balancing.cfg` (stage C).
    #[cfg_attr(feature = "serde", serde(default))]
    pub dark: DarkMagicCfg,
}

/// The Dark Tribe's magic values: `CAIConfigInt`s registered under `AI` by static initialisers
/// (value at object + 8), filled from `Config\Balancing.cfg` `[AI]` where the file has them.
/// [`DarkMagicCfg::default`] holds the exe's defaults.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DarkMagicCfg {
    /// The dark temple's unit costs in mana (`CAIMain` vtbl+0x34, 0x438a10, by settler type):
    /// `DTCostMushroomFarmer` 53 (0x415150 → 0x118292c, 5), `DTCostDarkGardener` 52 (0x1182938,
    /// 5), `DTCostShaman` 54 (0x1182944, 10), `DTCostSwordman` 29 (0x1182950, 15),
    /// `DTCostBowman` 32 (0x118295c, 15). Not in the install's Balancing.cfg.
    pub cost_mushroom_farmer: i32,
    pub cost_dark_gardener: i32,
    pub cost_shaman: i32,
    pub cost_swordman: i32,
    pub cost_bowman: i32,
    /// `DTManaCostRot` (0x424980 → 0x11cdb84, 5; Balancing.cfg 1): the shaman's rot spell.
    pub mana_cost_rot: i32,
    /// `DTManaCostTeleport` (0x424a20 → 0x11cdb90, 5; Balancing.cfg 4): its goods teleport.
    pub mana_cost_teleport: i32,
    /// `DTManaCostEnslave` (0x424ac0 → 0x11cdb9c, 5; Balancing.cfg 4): one enslaving hit.
    pub mana_cost_enslave: i32,
    /// `DTShamanManaPool` (0x424b60 → 0x11cdba8, 10; Balancing.cfg 43): a shaman's own mana,
    /// refilled on dark land.
    pub shaman_mana_pool: i32,
    /// `DTManakopterHitpoints` (0x424520 → 0x11cdb24, 250; Balancing.cfg 250): a new
    /// manakopter's hit points (250 when 256 or more), also what it heals to on dark land.
    #[cfg_attr(feature = "serde", serde(default = "default_manakopter_hitpoints"))]
    pub manakopter_hitpoints: i32,
    /// `DTManakopterArmor` (0x4245c0 → 0x11cdb30, 10; Balancing.cfg 3): taken off each hit.
    #[cfg_attr(feature = "serde", serde(default = "default_manakopter_armor"))]
    pub manakopter_armor: i32,
    /// `DTManakopterRequiredMana` / `…Boards` / `…Iron` (0x423f40 → 0x11cdab8, 0x423fe0 →
    /// 0x11cdac4, 0x424080 → 0x11cdad0; 4 / 5 / 5, Balancing.cfg the same): what the manakopter
    /// hall uses up for one manakopter.
    #[cfg_attr(feature = "serde", serde(default = "default_manakopter_required"))]
    pub manakopter_required: [i32; 3],
    /// `DTMaxSoldiersPerTaskForce` (0x117d068, value 0x117d070; 6, Balancing.cfg 16).
    #[cfg_attr(feature = "serde", serde(default = "default_max_soldiers_per_task_force"))]
    pub max_soldiers_per_task_force: i32,
}

#[cfg(feature = "serde")]
fn default_manakopter_hitpoints() -> i32 {
    DarkMagicCfg::default().manakopter_hitpoints
}
#[cfg(feature = "serde")]
fn default_manakopter_armor() -> i32 {
    DarkMagicCfg::default().manakopter_armor
}
#[cfg(feature = "serde")]
fn default_manakopter_required() -> [i32; 3] {
    DarkMagicCfg::default().manakopter_required
}
#[cfg(feature = "serde")]
fn default_max_soldiers_per_task_force() -> i32 {
    DarkMagicCfg::default().max_soldiers_per_task_force
}

impl Default for DarkMagicCfg {
    fn default() -> Self {
        DarkMagicCfg {
            cost_mushroom_farmer: 5,
            cost_dark_gardener: 5,
            cost_shaman: 10,
            cost_swordman: 15,
            cost_bowman: 15,
            mana_cost_rot: 5,
            mana_cost_teleport: 5,
            mana_cost_enslave: 5,
            shaman_mana_pool: 10,
            manakopter_hitpoints: 250,
            manakopter_armor: 10,
            manakopter_required: [4, 5, 5],
            max_soldiers_per_task_force: 6,
        }
    }
}

impl DarkMagicCfg {
    /// The exe defaults overridden by the `[AI]` section of `cfg`.
    pub fn from_cfg(cfg: &Cfg) -> DarkMagicCfg {
        let d = DarkMagicCfg::default();
        let get = |k: &str, d: i32| cfg.get("AI", k).and_then(|v| v.trim().parse().ok()).unwrap_or(d);
        DarkMagicCfg {
            cost_mushroom_farmer: get("DTCostMushroomFarmer", d.cost_mushroom_farmer),
            cost_dark_gardener: get("DTCostDarkGardener", d.cost_dark_gardener),
            cost_shaman: get("DTCostShaman", d.cost_shaman),
            cost_swordman: get("DTCostSwordman", d.cost_swordman),
            cost_bowman: get("DTCostBowman", d.cost_bowman),
            mana_cost_rot: get("DTManaCostRot", d.mana_cost_rot),
            mana_cost_teleport: get("DTManaCostTeleport", d.mana_cost_teleport),
            mana_cost_enslave: get("DTManaCostEnslave", d.mana_cost_enslave),
            shaman_mana_pool: get("DTShamanManaPool", d.shaman_mana_pool),
            manakopter_hitpoints: get("DTManakopterHitpoints", d.manakopter_hitpoints),
            manakopter_armor: get("DTManakopterArmor", d.manakopter_armor),
            manakopter_required: [
                get("DTManakopterRequiredMana", d.manakopter_required[0]),
                get("DTManakopterRequiredBoards", d.manakopter_required[1]),
                get("DTManakopterRequiredIron", d.manakopter_required[2]),
            ],
            max_soldiers_per_task_force: get("DTMaxSoldiersPerTaskForce", d.max_soldiers_per_task_force),
        }
    }

    /// `CAIMain` vtbl+0x34 (0x438a10) `+8`: the mana a dark temple unit of settler type `kind`
    /// costs (`None` for other types; the original returns a null object there).
    pub fn unit_cost(&self, kind: u16) -> Option<i32> {
        Some(match kind {
            29 => self.cost_swordman,
            32 => self.cost_bowman,
            52 => self.cost_dark_gardener,
            53 => self.cost_mushroom_farmer,
            54 => self.cost_shaman,
            _ => return None,
        })
    }
}

impl Default for MagicCfg {
    fn default() -> Self {
        let same = |a: [i32; SPELLS]| [a; RACES];
        let five = |r: [i32; SPELLS], v: [i32; SPELLS], m: [i32; SPELLS], d: [i32; SPELLS], t: [i32; SPELLS]| [r, v, m, d, t];
        MagicCfg {
            costs: five([10, 20, 15, 30, 20, 40, 20, 30], [10, 20, 30, 30, 20, 40, 20, 40], [10, 20, 30, 30, 20, 40, 20, 40], [10, 20, 30, 30, 20, 40, 20, 40], [10, 20, 30, 30, 20, 40, 20, 40]),
            radii: five([8; 8], [8, 8, 50, 8, 8, 8, 8, 8], [8; 8], [8; 8], [8, 8, 8, 8, 20, 8, 8, 8]),
            limits_min: five([3, 10, 40, 10, 10, 10, 10, 40], [3, 10, 3, 10, 10, 10, 10, 10], [3, 10, 10, 10, 10, 10, 10, 4], [3, 10, 10, 10, 10, 10, 10, 4], [3, 10, 10, 10, 10, 10, 10, 4]),
            limits_max: five([5, 10, 60, 10, 10, 10, 10, 60], [5, 10, 6, 10, 10, 10, 10, 10], [5, 10, 10, 10, 10, 10, 10, 8], [5, 10, 10, 10, 10, 10, 10, 8], [5, 10, 10, 10, 10, 10, 10, 10]),
            durations: five([20; 8], [20, 20, 50, 20, 20, 20, 20, 20], [20, 20, 20, 20, 20, 20, 20, 0], [20, 20, 20, 20, 20, 20, 20, 0], [20, 20, 20, 20, 20, 20, 20, 200]),
            effects: five([24, 27, 48, 30, 39, 49, 35, 36], [24, 27, 42, 30, 44, 33, 46, 0], [24, 27, 37, 30, 38, 40, 41, 47], [0, 0, 101, 0, 0, 0, 0, 102], [24, 27, 0, 30, 0, 108, 105, 103]),
            sound_effects: five([65, 67, 74, 64, 72, 75, 68, 83], [65, 67, 73, 64, 82, 70, 60, 62], [65, 67, 66, 64, 0, 84, 81, 71], [65, 67, 0, 0, 65, 67, 0, 0], [65, 57, 59, 97, 98, 57, 56, 58]),
            present_goods: same([7, 32, 20, 14, 34, 8, 25, 18]),
            present_chances: same([125, 250, 375, 500, 625, 750, 875, 1000]),
            present_amounts: same([8, 8, 6, 6, 4, 4, 4, 4]),
            convert_good_src: [11, 32, 7, 7, 33],
            convert_good_dst: [32, 20, 14, 14, 20],
            terrain_src: [80, 64, 16, 16, 16],
            terrain_dst: [16, 16, 64, 64, 144],
            lightning_bolt_damage_min: 100,
            lightning_bolt_damage_max: 100,
            shield_dmg_decrease256: 128,
            bloodlust_dmg_increase256: 256,
            faster_crops_decrease256: 128,
            viking_warmachine_shot_cost: 15,
            spell_cost_increase: 25,
            spell_cost_max_increase: 400,
            mana_per_donation: 1,
            mana_per_big_temple: 8,
            default_spell_range: 15,
            dark: DarkMagicCfg::default(),
        }
    }
}

impl MagicCfg {
    /// The exe defaults overridden by the `[MAGIC]` section of `cfg`. An array key replaces the
    /// values it lists, in order (entries it does not list keep their defaults [inf]).
    pub fn from_cfg(cfg: &Cfg) -> MagicCfg {
        let mut m = MagicCfg::default();
        let tables: [(&str, &mut [[i32; SPELLS]; RACES]); 10] = [
            ("SpellCosts", &mut m.costs),
            ("SpellRadii", &mut m.radii),
            ("SpellLimitsMin", &mut m.limits_min),
            ("SpellLimitsMax", &mut m.limits_max),
            ("SpellDurations", &mut m.durations),
            ("SpellEffects", &mut m.effects),
            ("SpellSoundEffects", &mut m.sound_effects),
            ("PresentGoods", &mut m.present_goods),
            ("PresentChances", &mut m.present_chances),
            ("PresentAmounts", &mut m.present_amounts),
        ];
        for (key, table) in tables {
            for (r, name) in RACE_NAMES.iter().enumerate() {
                if let Some(v) = cfg.get("MAGIC", &format!("{key}{name}")) {
                    for (o, x) in table[r].iter_mut().zip(v.split(',')) {
                        if let Ok(x) = x.trim().parse() {
                            *o = x;
                        }
                    }
                }
            }
        }
        let scalar = |k: &str, d: i32| cfg.get("MAGIC", k).and_then(|v| v.trim().parse().ok()).unwrap_or(d);
        for (r, name) in RACE_NAMES.iter().enumerate() {
            m.convert_good_src[r] = scalar(&format!("ConvertGood{name}Src"), m.convert_good_src[r]);
            m.convert_good_dst[r] = scalar(&format!("ConvertGood{name}Dst"), m.convert_good_dst[r]);
            m.terrain_src[r] = scalar(&format!("Terrain{name}Src"), m.terrain_src[r]);
            m.terrain_dst[r] = scalar(&format!("Terrain{name}Dst"), m.terrain_dst[r]);
        }
        m.lightning_bolt_damage_min = scalar("LightningBoltDamageMin", m.lightning_bolt_damage_min);
        m.lightning_bolt_damage_max = scalar("LightningBoltDamageMax", m.lightning_bolt_damage_max);
        m.shield_dmg_decrease256 = scalar("ShieldDmgDecrease256", m.shield_dmg_decrease256);
        m.bloodlust_dmg_increase256 = scalar("BloodlustDmgIncrease256", m.bloodlust_dmg_increase256);
        m.faster_crops_decrease256 = scalar("FasterCropsDecrease256", m.faster_crops_decrease256);
        m.viking_warmachine_shot_cost = scalar("VikingWarmachineShotCost", m.viking_warmachine_shot_cost);
        m.spell_cost_increase = scalar("SpellCostIncrease", m.spell_cost_increase);
        m.spell_cost_max_increase = scalar("SpellCostMaxIncrease", m.spell_cost_max_increase);
        m.mana_per_donation = scalar("ManaPerDonation", m.mana_per_donation);
        m.mana_per_big_temple = scalar("ManaPerBigTemple", m.mana_per_big_temple);
        m.default_spell_range = scalar("DefaultSpellRange", m.default_spell_range);
        m
    }

    /// `Config\Magic.cfg` of the install (the defaults without one), with the Dark Tribe's
    /// values from `Config\Balancing.cfg`.
    pub fn load_default() -> MagicCfg {
        let mut m = MagicCfg::from_cfg(&Cfg::load_default("Magic.cfg"));
        m.dark = DarkMagicCfg::from_cfg(&Cfg::load_default("Balancing.cfg"));
        m
    }

    fn at(table: &[[i32; SPELLS]; RACES], race: u8, spell: u8) -> i32 {
        table.get(race as usize).and_then(|t| t.get(spell as usize)).copied().unwrap_or(0)
    }

    /// 0x4f3270: the base cost, 1..=1000.
    pub fn base_cost(&self, race: u8, spell: u8) -> i32 {
        Self::at(&self.costs, race, spell).clamp(1, 1000)
    }

    /// 0x4f32d0: the highest cost, `base · (100 + SpellCostMaxIncrease) / 100`.
    pub fn max_cost(&self, race: u8, spell: u8) -> i32 {
        self.base_cost(race, spell).wrapping_mul(self.spell_cost_max_increase.wrapping_add(100)) / 100
    }

    /// 0x4f32a0: the rise per cast in 1/256, `(SpellCostIncrease >> half) · 256 / 100`.
    pub fn cost_increase(&self, flags: u32) -> i32 {
        ((self.spell_cost_increase >> ((flags >> 1) & 1)) << 8) / 100
    }

    /// 0x4f33d0: the duration in ticks, `clamp(SpellDurations, 1, 600) · 14`.
    pub fn duration(&self, race: u8, spell: u8) -> i32 {
        Self::at(&self.durations, race, spell).clamp(1, 600) * 14
    }

    /// `SpellRadii`, 0..=30 (0x4f56a3).
    pub fn radius(&self, race: u8, spell: u8) -> i32 {
        Self::at(&self.radii, race, spell).clamp(0, 30)
    }

    /// 0x4f53b0: how far from the caster a spell may land — `DefaultSpellRange` within 5..=50;
    /// unlimited (0x800) for the Maya's spell 4 and the Trojans' spell 7.
    pub fn range(&self, race: u8, spell: u8) -> i32 {
        if (race, spell) == (2, 4) || (race, spell) == (4, 7) {
            return 0x800;
        }
        self.default_spell_range.clamp(5, 50)
    }
}

/// 0x4f3310 `(cost, rise, max)`: the cost after a cast — `cost + cost·rise/256`, at least one
/// more, at most `max`; unchanged without a rise.
pub fn raise_cost(cost: i32, rise: i32, max: i32) -> i32 {
    if rise <= 0 {
        return cost;
    }
    let n = (cost.wrapping_mul(rise) >> 8).wrapping_add(cost);
    let n = n + (n == cost) as i32;
    n.min(max)
}

/// One player's mana (see the module docs for the original's layout).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ManaPool {
    /// +4.
    pub mana: i32,
    /// +8: everything ever added by temples (a script reads it, 0x5ca270).
    pub gained: i32,
    /// +0x10: the current cost of each spell.
    pub costs: [i32; SPELLS],
    /// +0x30: casts per spell (read by a script function, 0x5ca4f0).
    pub casts: [u32; SPELLS],
}

/// The mana pools, index = player (slot 0 unused).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Magic {
    pub players: Vec<ManaPool>,
}

impl Default for Magic {
    fn default() -> Self {
        Magic { players: vec![ManaPool::default(); 9] }
    }
}

impl Magic {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        for p in &self.players {
            h.u32(p.mana as u32);
            h.u32(p.gained as u32);
            p.costs.iter().for_each(|&c| h.u32(c as u32));
            p.casts.iter().for_each(|&c| h.u32(c));
        }
    }

    pub fn pool(&self, player: u8) -> Option<&ManaPool> {
        self.players.get(player as usize)
    }
}

fn valid_player(world: &World, player: u8) -> bool {
    (player as i32 - 1) < world.player_count() as i32
}

fn pool_mut(world: &mut World, player: u8) -> Option<&mut ManaPool> {
    world.magic.players.get_mut(player as usize)
}

/// 0x4f5400 (with the reset 0x4f6e00 first): the player's pool is emptied and the costs set to
/// the race's base costs. Called for every player by the game init (0x4fdb42) and by 0x4f5550.
pub fn init_player(world: &mut World, player: u8) {
    if !valid_player(world, player) {
        return;
    }
    let race = world.race_of(player);
    let costs = std::array::from_fn(|i| world.rules.magic.base_cost(race, i as u8));
    if let Some(p) = pool_mut(world, player) {
        *p = ManaPool::default();
        if (race as usize) < RACES {
            p.costs = costs;
        }
    }
}

/// [`init_player`] for every player (0x4fdb42).
pub fn init(world: &mut World) {
    world.magic = Magic::default();
    for p in 1..=world.player_count() {
        init_player(world, p);
    }
}

/// 0x4f58f0: a small temple's donation, `ManaPerDonation` (1..=99) to the mana and the total.
pub fn donate(world: &mut World, player: u8) {
    let n = world.rules.magic.mana_per_donation.clamp(1, 99);
    add(world, player, n);
}

/// The player's mana (+4).
pub fn mana(world: &World, player: u8) -> i32 {
    world.magic.pool(player).map_or(0, |p| p.mana)
}

/// `CAIMain` vtbl+0x30 (0x4389c0, from the dark temple's `SettlerEnter` 0x545cc3, a mushroom
/// farmer arriving): for a player in the game (`player − 1 < PlayerCount`) mana += the player's
/// AI value `DTManaPerSphere` (record 0x1292648 + player · 0x17c, +0x2c of the column it points
/// to; [`crate::ai::vars`]). It writes +0 (dirty bit) and +4 only — the release's
/// references to the gained counter +8 (0x149b800) are 0x4f58f0, 0x4f5960 and the script
/// function 0x5ca270, so this income is not counted as gained.
pub fn add_sphere(world: &mut World, player: u8) {
    if !valid_player(world, player) {
        return;
    }
    let n = world.ai.vars.get(player, crate::ai::vars::var::DT_MANA_PER_SPHERE);
    add_mana(world, player, n);
}

/// 0x436380 `(player, n)`: for a player in the game the mana (+4) grows by `n` (and the cache
/// bit +0 is set); the total gained (+8) is not touched. Called by the Dark Tribe player AI's
/// extra mana (0x43b33e) and inlined in [`add_sphere`].
pub fn add_mana(world: &mut World, player: u8, n: i32) {
    if !valid_player(world, player) {
        return;
    }
    if let Some(p) = pool_mut(world, player) {
        p.mana = p.mana.wrapping_add(n);
    }
}

/// 0x4f5960: a big temple is built, `ManaPerBigTemple` (0..=999) to the mana and the total.
pub fn big_temple(world: &mut World, player: u8) {
    let n = world.rules.magic.mana_per_big_temple.clamp(0, 999);
    add(world, player, n);
}

/// The dark temple's purchase of a unit (0x545760): with `cost` not above the player's mana
/// (compared unsigned) the mana goes down by it (and the pool's cache bit +0 is set, which only
/// the GUI reads). False for a player out of range or too little mana.
pub fn spend_dark_unit(world: &mut World, player: u8, cost: i32) -> bool {
    if !valid_player(world, player) {
        return false;
    }
    let Some(p) = pool_mut(world, player) else { return false };
    if (cost as u32) > (p.mana as u32) {
        return false;
    }
    p.mana = p.mana.wrapping_sub(cost);
    true
}

fn add(world: &mut World, player: u8, n: i32) {
    let valid = valid_player(world, player);
    let Some(p) = pool_mut(world, player) else { return };
    if valid {
        p.mana = p.mana.wrapping_add(n);
    }
    // The total grows even for a player out of range (0x4f5944).
    p.gained = p.gained.wrapping_add(n);
}

/// 0x4f59f0: a Viking war machine's shot, `VikingWarmachineShotCost` (1..=999); mana stops at 0.
pub fn warmachine_shot(world: &mut World, player: u8) {
    let cost = world.rules.magic.viking_warmachine_shot_cost.clamp(1, 999);
    if !valid_player(world, player) {
        return;
    }
    if let Some(p) = pool_mut(world, player) {
        p.mana = if (p.mana as u32) < cost as u32 { 0 } else { p.mana - cost };
    }
}

/// The cost `CastSpell` takes with `flags` (0 when free, half with [`cast_flag::HALF`]).
fn paid_cost(cost: i32, flags: u32) -> i32 {
    if flags & cast_flag::FREE != 0 { 0 } else { cost >> ((flags >> 1) & 1) }
}

/// 0x4f57f0: the mana left after casting `spell` with `flags`.
pub fn mana_after(world: &World, player: u8, spell: u8, flags: u32) -> i32 {
    let Some(p) = world.magic.pool(player) else { return 0 };
    let cost = p.costs.get(spell as usize).copied().unwrap_or(0);
    p.mana.wrapping_sub(paid_cost(cost, flags))
}

/// 0x4f58b0: how often the player cast `spell`.
pub fn casts(world: &World, player: u8, spell: u8) -> u32 {
    if !valid_player(world, player) || spell as usize >= SPELLS {
        return 0;
    }
    world.magic.pool(player).map_or(0, |p| p.casts[spell as usize])
}

/// 0x4f5a50 (the spell bar, 0x56a670): how many casts of `spell` the mana pays for, with the
/// cost rising after each (at most 99). The original caches the result per spell until the mana
/// changes; the cache is not simulation state.
pub fn affordable(world: &World, player: u8, spell: u8) -> i32 {
    let race = world.race_of(player);
    if !valid_player(world, player) || race as usize >= RACES || spell as usize >= SPELLS {
        return 0;
    }
    let Some(p) = world.magic.pool(player) else { return 0 };
    let cfg = &world.rules.magic;
    let (rise, max) = (cfg.cost_increase(0), cfg.max_cost(race, spell));
    let (mut mana, mut cost, mut n) = (p.mana, p.costs[spell as usize], 0);
    while n < 99 && cost <= mana {
        mana -= cost;
        n += 1;
        if rise > 0 {
            let c = ((cost.wrapping_mul(rise)) >> 8).wrapping_add(cost);
            cost = (c + (c == cost) as i32).min(max);
        }
    }
    n
}

/// The spell context 0x4f3410 builds (`this` of the spell functions).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpellCtx {
    /// +0, +4.
    pub player: u8,
    pub race: u8,
    /// +8, +0xc: the target node.
    pub x: i32,
    pub y: i32,
    /// +0x10 `SpellRadii` (0..=30), +0x14 a draw between `SpellLimitsMin` and `SpellLimitsMax`
    /// (0x4f3360), +0x18 the duration in ticks (0x4f33d0).
    pub radius: i32,
    pub limit: i32,
    pub duration: i32,
    /// +0x1c, +0x20.
    pub flags: u32,
    pub caster: u16,
    /// The spell (not in the original's context: the caller picks the function by it).
    pub spell: u8,
}

/// 0x4f3360: the spell's limit — `SpellLimitsMin` when it is not below the maximum, else a draw
/// on the game RNG in `min..=max` (both clamped to 1..=1000).
fn limit(world: &mut World, race: u8, spell: u8) -> i32 {
    let cfg = &world.rules.magic;
    let lo = MagicCfg::at(&cfg.limits_min, race, spell).clamp(1, 1000);
    let hi = MagicCfg::at(&cfg.limits_max, race, spell).clamp(1, 1000);
    if hi <= lo {
        return lo;
    }
    lo + (world.rng.rand() % ((hi - lo) as u32 + 1)) as i32
}

/// The context 0x4f3410 builds: `c` with its radius, duration and (a draw) limit filled in.
pub fn context(world: &mut World, mut c: SpellCtx) -> SpellCtx {
    c.radius = world.rules.magic.radius(c.race, c.spell);
    c.duration = world.rules.magic.duration(c.race, c.spell);
    c.limit = limit(world, c.race, c.spell);
    c
}

/// `CastSpell(player, spell, x, y, flags, caster)` (0x4f5570). Returns −1 without enough mana,
/// 0 when nothing was cast, 1 when it was.
///
/// With [`cast_flag::RACE`] the race comes from the flags, the caster is dropped and the cast is
/// free and unlimited in range. The cost is the spell's current cost (none when free, half with
/// [`cast_flag::HALF`]); with too little mana −1. The target must be on the map. A caster
/// (entity id > 0) without [`cast_flag::NO_RANGE`] must stand within [`MagicCfg::range`] of the
/// target (hex distance). Then the spell (table .rdata 0x1000838, `[race·8 + spell]`) runs on
/// the context (radius, duration, then the limit's draw); a spell that does nothing makes the
/// cast fail. With [`cast_flag::DEFER`] the spell is not run now: future event 3 in 12 ticks on
/// the caster carries `(x << 16 | spell << 8 | player, flags << 16 | y)` ([`cast_deferred`]).
/// A paid cast takes the mana and raises the spell's cost ([`raise_cost`] by
/// [`MagicCfg::cost_increase`] up to [`MagicCfg::max_cost`]); every cast counts. Not
/// simulation: the AI notice (0x146b148 vtbl+0x2c(1, player, spell, pos)) and the script's
/// callback (0x5bdd60 when `GameScriptManager +0x5f4 > 0`), which are [`SimEvent::SpellCast`].
pub fn cast_spell(world: &mut World, player: u8, spell: u8, x: i32, y: i32, flags: u32, caster: u16) -> i32 {
    if !valid_player(world, player) || spell as usize >= SPELLS {
        return 0;
    }
    let (mut race, mut flags, mut caster) = (world.race_of(player), flags, caster);
    if flags & cast_flag::RACE != 0 {
        caster = 0;
        race = ((flags >> 24) & 7) as u8;
        flags = (flags & 0xefff_fffd) | cast_flag::FREE | cast_flag::NO_RANGE;
    }
    if race as usize >= RACES {
        return 0;
    }
    let Some(p) = world.magic.pool(player) else { return 0 };
    let (mana, cost) = (p.mana, p.costs[spell as usize]);
    let paid = paid_cost(cost, flags);
    if mana < paid {
        return -1;
    }
    let size = world.grid.size();
    if x as u32 >= size || y as u32 >= size {
        return 0;
    }
    if caster > 0 && flags & cast_flag::NO_RANGE == 0 {
        // The caster's position (0x4f566a; the original reads the entity without a check).
        let Some(c) = world.entities.by_index(caster as u32) else { return 0 };
        let d = crate::spiral::grid_distance(x - c.x(), y - c.y());
        if world.rules.magic.range(race, spell) < d {
            return 0;
        }
    }
    if flags & cast_flag::DEFER == 0 {
        let ctx = context(world, SpellCtx { player, race, x, y, radius: 0, limit: 0, duration: 0, flags, caster, spell });
        if run_spell(world, &ctx) == 0 {
            return 0;
        }
    } else {
        let a = ((x as u32) << 16 | (spell as u32) << 8 | player as u32) as i32;
        let b = (flags << 16 | (y as u32 & 0xffff)) as i32;
        crate::future::add(world, FUTURE_EVENT, DEFER_TICKS, caster, a, b);
    }
    if paid > 0 {
        let cfg = &world.rules.magic;
        let raised = raise_cost(cost, cfg.cost_increase(flags), cfg.max_cost(race, spell));
        if let Some(p) = pool_mut(world, player) {
            p.mana -= paid;
            p.costs[spell as usize] = raised;
        }
    }
    if let Some(p) = pool_mut(world, player) {
        p.casts[spell as usize] = p.casts[spell as usize].wrapping_add(1);
    }
    world.events.push(SimEvent::SpellCast { player, spell, x, y });
    1
}

/// 0x4f5c50 `(caster, a, b)`, future event 3 (0x4e5300): the deferred spell of [`cast_spell`].
/// The race is the player's again, the context is built (the limit's draw now), the spell runs
/// (its result is not looked at) and the spell's effect starts at the target: `SpellEffects`
/// when in 1..=0x55 or 0x65..=0x73, with `SpellSoundEffects` when in 1..=0x6c.
pub fn cast_deferred(world: &mut World, caster: u16, a: i32, b: i32) {
    let (a, b) = (a as u32, b as u32);
    let player = (a & 0xff) as u8;
    if !valid_player(world, player) {
        return;
    }
    let spell = ((a >> 8) & 0xff) as u8;
    let (x, y, flags) = ((a >> 16) as i32, (b & 0xffff) as i32, b >> 16);
    let race = world.race_of(player);
    let ctx = context(world, SpellCtx { player, race, x, y, radius: 0, limit: 0, duration: 0, flags, caster, spell });
    run_spell(world, &ctx);
    let effect = MagicCfg::at(&world.rules.magic.effects, race, spell);
    if (1..0x56).contains(&effect) || (0x65..0x74).contains(&effect) {
        let sound = MagicCfg::at(&world.rules.magic.sound_effects, race, spell);
        let sound = if (1..=0x6c).contains(&sound) { sound as u16 } else { 0 };
        world.events.push(SimEvent::Effect { effect: effect as u8, sound, x, y });
    }
}

/// The spell functions, `[race][spell]` (.rdata 0x1000838). 0x4f35c0 (the Dark Tribe's empty
/// slots) does nothing.
pub const SPELL_TABLE: [[u32; SPELLS]; RACES] = [
    [0x4f35d0, 0x4f3850, 0x4f3b60, 0x4f39d0, 0x4f3d00, 0x4f3d20, 0x4f3e60, 0x4f3fd0],
    [0x4f35d0, 0x4f3850, 0x4f4190, 0x4f39d0, 0x4f42e0, 0x4f4300, 0x4f4320, 0x4f4340],
    [0x4f35d0, 0x4f3850, 0x4f4450, 0x4f39d0, 0x4f4550, 0x4f4780, 0x4f48f0, 0x4f4ad0],
    [0x4f35c0, 0x4f35c0, 0x4f4c70, 0x4f35c0, 0x4f35c0, 0x4f35c0, 0x4f35c0, 0x4f4d10],
    [0x4f35d0, 0x4f3850, 0x4f4e40, 0x4f39d0, 0x4f5000, 0x4f5180, 0x4f51a0, 0x4f5380],
];

/// Runs the spell of `ctx` (its function from [`SPELL_TABLE`]); returns what it returns (0 =
/// nothing happened).
pub fn run_spell(world: &mut World, ctx: &SpellCtx) -> i32 {
    let f = SPELL_TABLE.get(ctx.race as usize).and_then(|t| t.get(ctx.spell as usize)).copied().unwrap_or(0x4f35c0);
    match f {
        0x4f35c0 => 0,
        0x4f35d0 => spells::gift(world, ctx),
        0x4f3850 => spells::convert(world, ctx),
        0x4f3b60 => spells::fish(world, ctx),
        0x4f39d0 => spells::terrain(world, ctx),
        0x4f3fd0 => spells::mountains(world, ctx),
        0x4f4190 => spells::animals(world, ctx),
        0x4f4450 => spells::crops(world, ctx),
        0x4f4e40 => spells::mines(world, ctx),
        0x4f3d00 => spells::shield(world, ctx),
        0x4f3d20 => spells::lightning(world, ctx),
        0x4f3e60 => spells::convert_soldiers(world, ctx),
        0x4f42e0 => spells::freeze(world, ctx),
        0x4f4300 => spells::bloodlust(world, ctx),
        0x4f4320 => spells::confuse(world, ctx),
        0x4f4340 => spells::strike(world, ctx),
        0x4f4550 => spells::teleport(world, ctx),
        0x4f4780 => spells::bowmen(world, ctx),
        0x4f48f0 => spells::upgrade(world, ctx),
        0x4f4ad0 => spells::stones(world, ctx),
        0x4f5000 => spells::doors(world, ctx),
        0x4f5180 => spells::hide(world, ctx),
        0x4f51a0 => spells::disarm(world, ctx),
        0x4f5380 => spells::vision(world, ctx),
        0x4f4c70 => spells::rot(world, ctx),
        0x4f4d10 => spells::teleport_goods(world, ctx),
        _ => {
            // TODO(port): the spells (stage B).
            world.events.push(SimEvent::Unported { what: "spell", entity: ctx.caster });
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cfg_overrides_the_defaults() {
        let cfg = Cfg::parse(
            "[MAGIC]\n{\n;  comment\nSpellCostsRoman[8] = 12, 17, 10, 10, 17, 25, 40, 20,\nSpellRadiiViking[8] = 8, 10\nManaPerBigTemple = 12\nSpellCostIncrease = 10\nSpellCostMaxIncrease = 300\n}\n",
        );
        let m = MagicCfg::from_cfg(&cfg);
        assert_eq!(m.costs[0], [12, 17, 10, 10, 17, 25, 40, 20]);
        // Entries not listed keep the exe's defaults [inf].
        assert_eq!(m.radii[1], [8, 10, 50, 8, 8, 8, 8, 8]);
        assert_eq!(m.costs[1], MagicCfg::default().costs[1]);
        assert_eq!((m.mana_per_big_temple, m.mana_per_donation, m.default_spell_range), (12, 1, 15));
        assert_eq!((m.cost_increase(0), m.cost_increase(HALF_FLAG), m.max_cost(0, 6)), (25, 12, 160));
        assert_eq!((m.duration(4, 7), m.duration(2, 7), m.radius(1, 2)), (2800, 14, 30));
        assert_eq!((m.range(0, 0), m.range(2, 4), m.range(4, 7)), (15, 0x800, 0x800));
    }

    const HALF_FLAG: u32 = cast_flag::HALF;

    #[test]
    fn cost_rise() {
        assert_eq!(raise_cost(40, 25, 160), 43);
        // At least one more, at most the maximum, nothing without a rise.
        assert_eq!(raise_cost(5, 25, 20), 6);
        assert_eq!(raise_cost(159, 25, 160), 160);
        assert_eq!(raise_cost(40, 0, 160), 40);
        assert_eq!((paid_cost(25, 0), paid_cost(25, cast_flag::HALF), paid_cost(25, cast_flag::FREE | cast_flag::HALF)), (25, 12, 0));
    }
}
