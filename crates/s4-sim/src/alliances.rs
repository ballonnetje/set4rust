//! `CAlliances` (release .data 0x12dccf8..0x12dd008): the teams of a game. Own reverse
//! engineering of the shipped exe.
//!
//! The tables are filled once, when a game starts or a savegame is loaded (they are not saved):
//! 0x4fe510 clears the block and, for every used player slot of the game setup
//! (`GlobalGameState +0x90` team per slot, +0x1b0 slot unused, +0x338 slot count), stores team
//! `slot team + 1` (1..8; 9 = no team) at 0x12dcd0c[player] and counts the members of real
//! teams (0x12dcd78[team]); then 0x5ab700 gives every player without a team the first team id
//! with no members (scanning on from the last one given, never past 8) and builds the masks:
//! 0x12dcd08 all players, 0x12dcd30[p] the players of p's team (allies, p included),
//! 0x12dcd54[p] every other player (enemies). Player bits are `(1 << p) >> 1`, so player 0 has
//! none and is nobody's ally or enemy. The writers of these tables are only those two functions
//! (Ghidra write references), so alliances never change during a game.
//!
//! Readers ported here: the war map (team strengths 0x12dcd0c; `warmap::team` / `hostile` /
//! `allied`), the soldiers' searches (enemy masks 0x12dcd54, medics' ally masks 0x12dcd30), the
//! fighting strength on own-team land (`statistic`), the healer (team equality, 0x54ef60), the
//! saboteur's camouflage (0x5801f0), the fog (ally bits, 0x5ae6d0 / 0x5ae210 / 0x5aed30 /
//! 0x5aeef0), the tiling path finder and `Game.ArePlayerAreasConnected` (0x5c6150).

/// The `CAlliances` tables, indexed by player id 0..8 (0 unused).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Alliances {
    /// 0x12dcd0c: the team (alliance id) of each player, 0 = not in the game.
    pub alliance_id: [u8; 9],
    /// 0x12dcd30: bit `p − 1` for every player `p` of the same team (`CAlliances::PlayerBit`).
    pub ally_bits: [u16; 9],
    /// 0x12dcd54: the players of the game not in the team.
    pub enemy_bits: [u16; 9],
}

impl Default for Alliances {
    /// Every player 1..8 its own team (what [`Alliances::new`] gives for eight players without
    /// teams).
    fn default() -> Self {
        Alliances::new([true; 8], [None; 8])
    }
}

/// `CAlliances::PlayerBit(player)`: `(1 << player) >> 1`.
pub fn player_bit(player: u8) -> u16 {
    1u32.checked_shl(player as u32).map_or(0, |b| (b >> 1) as u16)
}

impl Alliances {
    /// 0x4fe510 + 0x5ab700 for the players `p = i + 1` with `present[i]`: `teams[i]` is the
    /// setup's team 0..7, `None` (or 8) no team; a team above 8 leaves the player out of the
    /// game, as in the release.
    pub fn new(present: [bool; 8], teams: [Option<u8>; 8]) -> Alliances {
        let mut team = [0u8; 9];
        let mut count = [0u32; 9];
        let mut last = 0;
        for i in 0..8 {
            if !present[i] {
                continue;
            }
            let t = teams[i].map_or(9, |t| t as u32 + 1);
            if !(1..=9).contains(&t) {
                continue;
            }
            last = i + 1;
            team[i + 1] = t as u8;
            if t != 9 {
                count[t as usize] += 1;
            }
        }
        let mut k = 1;
        for t in team.iter_mut().take(last + 1).skip(1) {
            if *t == 9 {
                while k < 8 && count[k] != 0 {
                    k += 1;
                }
                *t = k as u8;
                count[k] = 1;
            }
        }
        let mut all = 0u16;
        let mut members = [0u16; 9];
        for p in 1..=8u8 {
            let t = team[p as usize];
            if t != 0 {
                all |= player_bit(p);
                members[t as usize] |= player_bit(p);
            }
        }
        let mut a = Alliances { alliance_id: team, ally_bits: [0; 9], enemy_bits: [0; 9] };
        for (p, &t) in team.iter().enumerate().skip(1) {
            if t != 0 {
                a.ally_bits[p] = members[t as usize];
                a.enemy_bits[p] = !members[t as usize] & all;
            }
        }
        a
    }

    fn idx(p: u8) -> usize {
        (p as usize).min(8)
    }

    /// 0x12dcd0c: the team of a player (0 for player 0 and players not in the game).
    pub fn team(&self, p: u8) -> usize {
        self.alliance_id[Self::idx(p)] as usize
    }

    /// 0x12dcd54: whether `b`'s units are enemies of `a`.
    pub fn hostile(&self, a: u8, b: u8) -> bool {
        self.enemy_bits[Self::idx(a)] & player_bit(b) != 0
    }

    /// 0x12dcd30: whether `b` belongs to `a`'s team (`a` included).
    pub fn allied(&self, a: u8, b: u8) -> bool {
        self.ally_bits[Self::idx(a)] & player_bit(b) != 0
    }

    pub(crate) fn hash(&self, h: &mut crate::hash::StateHasher) {
        for p in 0..9 {
            h.u32(self.alliance_id[p] as u32 | (self.ally_bits[p] as u32) << 8 | (self.enemy_bits[p] as u32) << 16);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solo_players_take_free_team_ids() {
        // Players 1 and 3 in setup team 1 (id 2), players 2 and 4 alone, 5..8 absent.
        let a = Alliances::new([true, true, true, true, false, false, false, false], [Some(1), None, Some(1), None, None, None, None, None]);
        assert_eq!(&a.alliance_id[..5], &[0, 2, 1, 2, 3]);
        assert_eq!(a.ally_bits[1], 0b0101);
        assert_eq!(a.enemy_bits[1], 0b1010);
        assert_eq!(a.enemy_bits[2], 0b1101);
        assert!(a.hostile(1, 2) && !a.hostile(1, 3) && a.allied(1, 3) && !a.hostile(1, 0) && !a.hostile(0, 1));
        let d = Alliances::default();
        assert!((1..=8).all(|p| d.team(p) == p as usize && d.enemy_bits[p as usize] == 0xff & !player_bit(p)));
    }
}
