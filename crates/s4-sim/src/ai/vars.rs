//! The AI player records (0x1292640, 9 × 0x17c bytes, player 0..8): the level, the script
//! flags and four columns of the 23 script variables (`AI.SetPlayerVar`).
//!
//! * Record: +0 the level (column 0..3), +4 the script flags (`AI.SetFlagBits` 0x43c570,
//!   `AI.ClearFlagBits` 0x43c5a0, the Dark Tribe switches 0x43c5d0), +8 a pointer to the column
//!   of the level (0x4345c0 / 0x434630), +0xc the columns, 23 values each.
//! * Defaults (0x43da20 at start-up, into 0x15aa090): column 0 all 0, columns 1..3 the
//!   variable's `CAIConfigIntEx` values (+8 / +0xc / +0x10, exe initialisers; the install's
//!   `Balancing.cfg` sets none of them) clamped to its range. Table at 0xff0630: per variable
//!   the config object (name at +0x14), min, max.
//! * `Init` (0x43c8c0) sets every record to level 2, flags 0, the default columns. The game start
//!   (0x4ff5f0) gives AI players level 1 or 2.
//! * Saved in chunk 150 (0x43c9b0 / 0x43c910 / record load 0x43c7a0).

use crate::hash::StateHasher;

/// Variables per column.
pub const VARS: usize = 23;
/// Columns: 0 (all zero), then the three levels `AI.SetPlayerVar` sets.
pub const COLUMNS: usize = 4;

/// 0xff0630: name, min, max and the defaults of columns 1..3 (CAIConfigIntEx +8 / +0xc / +0x10).
pub const VAR_TABLE: [(&str, i32, i32, [i32; 3]); VARS] = [
    ("AttackMode", 0, 3, [2, 2, 2]),
    ("AttackDelay", 1, 600, [75, 45, 30]),
    ("AttackChancePartial", 1, 20, [20, 20, 20]),
    ("AttackThresholdPartial", 1, 1000, [175, 175, 175]),
    ("AttackThresholdTotal", 1, 1000, [225, 225, 225]),
    ("SoldierLimitAbsolute", 0, 10000, [250, 500, 0]),
    ("SoldierLimitRelative", 0, 1000, [200, 250, 0]),
    ("WarmachineMode", 0, 99, [0, 0, 0]),
    ("MagicMode", 0, 99, [0, 0, 0]),
    ("SpellMask", 0, 255, [127, 255, 255]),
    ("SpellFlags", 0, 2147483647, [0, 0, 0]),
    ("DTManaPerSphere", 1, 1000, [2, 6, 8]),
    ("DTMaxFarmsBase", 0, 1000, [3, 5, 0]),
    ("DTMaxFarmsPerHour", 0, 1000, [12, 15, 0]),
    ("DTExtraManaBaseAmount", 0, 1000, [1, 2, 3]),
    ("DTExtraManaAmountPerFarm2", 0, 1000, [1, 2, 3]),
    ("DTExtraManaDelay", 1, 1000, [60, 60, 60]),
    ("DTExtraManaLimit", 0, 10000, [100, 200, 0]),
    ("UsePioneers", 0, 2, [1, 2, 2]),
    ("DTManakopterHallDelay", 0, 3600, [240, 120, 60]),
    ("DTManakopterDelay", 0, 3600, [600, 300, 150]),
    ("DTManakopterLimit", 1, 1, [1, 1, 1]),
    ("DTAttackCheckDelay", 1, 1000, [10, 5, 1]),
];

/// Index of a variable by name (`stricmp`, as 0x43ced0 compares).
pub fn var_index(name: &str) -> Option<usize> {
    VAR_TABLE.iter().position(|v| v.0.eq_ignore_ascii_case(name))
}

/// Variable indices the game reads by number.
pub mod var {
    pub const DT_MANA_PER_SPHERE: usize = 11;
}

/// Script flag bits (record +4).
pub mod flag {
    /// `AI.DarkTribeUseDarkGardeners`.
    pub const DT_DARK_GARDENERS: u32 = 0x1000_0000;
    /// `AI.DarkTribeUseShamans`.
    pub const DT_SHAMANS: u32 = 0x2000_0000;
    /// `AI.DarkTribeDbgIgnoreFighters`.
    pub const DT_IGNORE_FIGHTERS: u32 = 0x4000_0000;
}

/// 0x15aa090 (0x43da20).
pub fn default_columns() -> [[i32; VARS]; COLUMNS] {
    let mut c = [[0; VARS]; COLUMNS];
    for (i, &(_, min, max, d)) in VAR_TABLE.iter().enumerate() {
        for k in 0..3 {
            c[k + 1][i] = d[k].clamp(min, max);
        }
    }
    c
}

/// One record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Record {
    /// +0: the column in use (0..3).
    pub level: i32,
    /// +4.
    pub flags: u32,
    /// +0xc.
    pub columns: [[i32; VARS]; COLUMNS],
}

impl Default for Record {
    /// 0x43c8c0.
    fn default() -> Self {
        Record { level: 2, flags: 0, columns: default_columns() }
    }
}

impl Record {
    /// The value of variable `i` at the record's level (`*(+8))[i]`).
    pub fn get(&self, i: usize) -> i32 {
        self.columns.get(self.level as usize).and_then(|c| c.get(i)).copied().unwrap_or(0)
    }
}

/// The nine records (player 0..8).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PlayerVars {
    pub records: [Record; 9],
}

impl PlayerVars {
    /// 0x43c8c0.
    pub fn reset(&mut self) {
        *self = PlayerVars::default();
    }

    pub(crate) fn hash(&self, h: &mut StateHasher) {
        for r in &self.records {
            h.u32(r.level as u32);
            h.u32(r.flags);
            for c in &r.columns {
                c.iter().for_each(|&v| h.u32(v as u32));
            }
        }
    }

    /// The value of variable `i` for `player` at its level (0 outside 0..=8).
    pub fn get(&self, player: u8, i: usize) -> i32 {
        self.records.get(player as usize).map_or(0, |r| r.get(i))
    }

    /// 0x434630 / the game start: player 1..=8, the level as given (the release keeps any value
    /// the caller passes; 0x434630 ignores 0).
    pub fn set_level_unchecked(&mut self, player: u8, level: i32) {
        if (1..=8).contains(&player)
            && let Some(r) = self.records.get_mut(player as usize)
        {
            r.level = level;
        }
    }

    /// `AI.SetFlagBits` / `AI.ClearFlagBits` / the Dark Tribe switches (0x43c5d0): player 1..=8.
    pub fn set_flags(&mut self, player: u8, bits: u32, on: bool) {
        if (1..=8).contains(&player)
            && let Some(r) = self.records.get_mut(player as usize)
        {
            if on {
                r.flags |= bits;
            } else {
                r.flags &= !bits;
            }
        }
    }

    /// `AI.SetPlayerVar(player, name, easy, normal, hard)` (0x43ced0, levels by 0x43cb30).
    /// `easy` / `normal` / `hard` arrive as the binding reads them (0x43caa0): −1 missing or not
    /// a number, −2 negative, else the value. Each value ≥ 0 is clamped to the variable's range;
    /// then a missing normal copies easy and a missing hard copies normal (only when easy is
    /// set). The call does nothing unless all three end up ≥ 0. Player 0 sets columns 1..3 of
    /// players 1..=8; another player must be 1..=8 and in the game (`in_game`). Unknown names
    /// are ignored.
    pub fn set_player_var(&mut self, player: i32, name: &str, easy: i32, normal: i32, hard: i32, in_game: impl Fn(u8) -> bool) {
        if !(player == 0 || ((1..=8).contains(&player) && in_game(player as u8))) {
            return;
        }
        let Some(i) = var_index(name) else { return };
        let (_, min, max, _) = VAR_TABLE[i];
        let clamp = |v: i32| if v < 0 { v } else { v.clamp(min, max) };
        let (e, mut n, mut h) = (clamp(easy), clamp(normal), clamp(hard));
        if e >= 0 {
            if n == -1 {
                n = e;
            }
            if n >= 0 && h == -1 {
                h = n;
            }
        }
        if e < 0 || n < 0 || h < 0 {
            return;
        }
        let players: Vec<usize> = if player == 0 { (1..=8).collect() } else { vec![player as usize] };
        for p in players {
            let r = &mut self.records[p];
            r.columns[1][i] = e;
            r.columns[2][i] = n;
            r.columns[3][i] = h;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_player_var_rules() {
        let mut v = PlayerVars::default();
        let all = |_| true;
        // Clamped, normal and hard copied down.
        v.set_player_var(2, "attackmode", 9, -1, -1, all);
        assert_eq!([v.records[2].columns[1][0], v.records[2].columns[2][0], v.records[2].columns[3][0]], [3, 3, 3]);
        // An explicit negative (read as −2) cancels the call.
        v.set_player_var(2, "AttackDelay", 10, -2, -1, all);
        assert_eq!(v.records[2].columns[1][1], 75);
        // Player 0: players 1..8, record 0 untouched.
        v.set_player_var(0, "DTManaPerSphere", 4, 5, 6, all);
        assert!((1..=8).all(|p| v.records[p].columns[1..].iter().map(|c| c[11]).eq([4, 5, 6])));
        assert_eq!(v.records[0].columns[2][11], 6);
        // Not in the game.
        v.set_player_var(3, "AttackMode", 0, 0, 0, |p| p != 3);
        assert_eq!(v.records[3].columns[1][0], 2);
        assert_eq!(v.get(1, var::DT_MANA_PER_SPHERE), 5);
    }
}
