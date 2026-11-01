//! Campaign progress, persisted as TOML next to the settings (`%APPDATA%\s4rs\campaign.toml`).
//!
//! The original keeps one status per campaign type (`GetCampaignStatus`, release 0x501ba0, and
//! `SetCampaignStatus`, 0x501d90: config section `MISCDATA2`, keys `Data01..Data04` hold one byte
//! per type plus a checksum; a bad checksum reads as 0): the number of missions won. The game's end
//! (0x5145a0) raises it to `level + 1` when the player's team won campaign mission `level`
//! (0-based) and the status is lower. Campaign types: 1 Roman, 2 Viking, 3 Maya (three missions
//! each, `CStateCampaign3X3`), 4 Dark Tribe (twelve, `CStateCampaignDark`); the add-ons' types
//! keep a nibble each (Data05 5..9, Data06 11..16, Data07 17..20: Mission CD 2's Roman, Viking,
//! Maya, Trojan, five missions each); types 21..24 (Mission CD 3, four missions) read as 15,
//! every mission open.
//!
//! The selection screens read the status in their constructors (`CStateCampaign3X3` 0x511300:
//! types 1 / 2 / 3; `CStateCampaignDark` 0x5115b0 / 0x511630: type 4) and their inits grey out
//! every mission after `status + 1` (0x47bf90, 0x47d7b0). `unlock_all = true` in the progress file
//! is our extra that opens every mission.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Progress {
    /// Missions won per campaign type.
    pub roman: u32,
    pub viking: u32,
    pub maya: u32,
    pub dark: u32,
    /// The add-on campaign types (5 and up), by type number.
    pub addon: std::collections::BTreeMap<String, u32>,
    /// Every mission open (not in the original; off by default).
    pub unlock_all: bool,
}

/// Missions per campaign type (`CAMPAIGN_3C_COUNT` 3, `CAMPAIGN_DC_COUNT` 12).
pub fn mission_count(campaign: u8) -> u32 {
    match campaign {
        1..=3 | 8 | 9 => 3,
        4 | 15 => 12,
        10 | 16 => 1,
        11..=14 | 21..=24 => 4,
        _ => 5,
    }
}

/// The mission's map (`Map\Campaign\roman%02i.map`, `viking`, `maya`, `dark`; 1-based number).
pub fn map_path(campaign: u8, level: u32) -> PathBuf {
    let name = match campaign {
        1 => "roman",
        2 => "viking",
        3 => "maya",
        _ => "dark",
    };
    s4_formats::path::resolve(s4_formats::game_dir().join("Map").join("Campaign").join(format!("{name}{:02}.map", level + 1)))
}

/// The progress file (`%APPDATA%\s4rs\campaign.toml`).
pub fn path() -> Option<PathBuf> {
    Some(crate::settings::user_dir()?.join("campaign.toml"))
}

impl Progress {
    pub fn load() -> Progress {
        match path() {
            Some(p) => Progress::load_from(&p),
            None => Progress::default(),
        }
    }

    pub fn load_from(p: &std::path::Path) -> Progress {
        match std::fs::read_to_string(p) {
            Ok(s) => toml::from_str(&s).unwrap_or_else(|e| {
                tracing::warn!("{}: {e}", p.display());
                Progress::default()
            }),
            Err(_) => Progress::default(),
        }
    }

    pub fn save_to(&self, p: &std::path::Path) {
        let res = p.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| {
            std::fs::write(p, toml::to_string_pretty(self).map_err(|e| std::io::Error::other(e.to_string()))?)
        });
        if let Err(e) = res {
            tracing::warn!("{}: {e}", p.display());
        }
    }

    /// `GetCampaignStatus(type)` as the selection screens see it (all open with `unlock_all`).
    pub fn status(&self, campaign: u8) -> u32 {
        if (21..=24).contains(&campaign) {
            return 15;
        }
        // The Mission CD bonus: set once Data05 reads 0x33555 (types 5..9 all won).
        if campaign == 10 && [5, 6, 7, 8, 9].iter().all(|&t| self.wins(t) >= mission_count(t)) {
            return 1;
        }
        if self.unlock_all { mission_count(campaign) - 1 } else { self.wins(campaign) }
    }

    /// The recorded missions won.
    pub fn wins(&self, campaign: u8) -> u32 {
        match campaign {
            1 => self.roman,
            2 => self.viking,
            3 => self.maya,
            4 => self.dark,
            t => self.addon.get(&t.to_string()).copied().unwrap_or(0),
        }
    }

    /// Whether mission `level` (0-based) can be started: the screens disable every mission
    /// after `status + 1`.
    pub fn open(&self, campaign: u8, level: u32) -> bool {
        level <= self.status(campaign)
    }

    /// `CStateGame::EndGame` after a won mission: `SetCampaignStatus(type, level + 1)` when that
    /// is higher. Returns whether it changed.
    pub fn won(&mut self, campaign: u8, level: u32) -> bool {
        let slot = match campaign {
            1 => &mut self.roman,
            2 => &mut self.viking,
            3 => &mut self.maya,
            4 => &mut self.dark,
            t => self.addon.entry(t.to_string()).or_insert(0),
        };
        let new = (level + 1).min(mission_count(campaign));
        if new > *slot {
            *slot = new;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlocking() {
        // The default locks like the original; `unlock_all` opens everything.
        let all = Progress { unlock_all: true, ..Default::default() };
        assert!(all.open(1, 2) && all.open(4, 11));
        assert!(!toml::from_str::<Progress>("roman = 1").unwrap().unlock_all);
        let mut p = Progress::default();
        assert!(p.open(1, 0) && !p.open(1, 1));
        assert!(p.won(1, 0));
        assert!(p.open(1, 1) && !p.open(1, 2));
        // Winning an earlier mission again does not lower or raise anything.
        assert!(!p.won(1, 0));
        assert!(p.won(4, 11));
        assert_eq!(p.dark, 12);
        let s = toml::to_string_pretty(&p).unwrap();
        assert_eq!(toml::from_str::<Progress>(&s).unwrap(), p);
        assert_eq!(p.roman, 1);
        // Mission CD 2's Maya (type 19): a nibble of its own; Mission CD 3 always open.
        assert!(p.won(19, 0) && p.status(19) == 1 && p.status(18) == 0);
        assert_eq!(p.status(22), 15);
        let s = toml::to_string_pretty(&p).unwrap();
        assert_eq!(toml::from_str::<Progress>(&s).unwrap(), p);
    }

    #[test]
    fn maps_exist() {
        for (c, n) in [(1, 3), (2, 3), (3, 3), (4, 12)] {
            for l in 0..n {
                let p = map_path(c, l);
                assert!(!s4_formats::game_dir().exists() || p.exists(), "{}", p.display());
            }
        }
    }
}
